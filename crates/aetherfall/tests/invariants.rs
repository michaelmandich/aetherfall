//! Simulation invariants, checked while computer players play out whole
//! matches headless with every sim crate and the real content.
//!
//! [`Watch`] rides along in the match. Every tick (right after `Prepare`,
//! when the spatial index and the player recount are fresh, so it sees the
//! world as the last tick left it) it checks that no boxes overlap, follows
//! movers, workers and production queues, and that dead entities are gone;
//! every [`SNAPSHOT_TICKS`] it checks the world as a whole; at `Cleanup` it
//! audits the deaths of the tick. The invariants:
//!
//! * at the end of every tick, no ground unit overlaps another ground unit,
//!   a building or a blocking node by more than [`OVERLAP_TOLERANCE`] (the
//!   core rule of the game: boxes never clip);
//! * every unit is inside the map, and every ground unit's centre is on
//!   terrain its mobility can enter;
//! * positions, facings, hit points, shields, loads, node and field amounts,
//!   effect boxes and resources are finite, and resources never negative;
//!   a living unit or building has hit points in `(0, max]`;
//! * `PlayerState` populations and per-type counts match a recount;
//! * no unit trying to reach a `MoveGoal` stays within [`STUCK_RADIUS`] for
//!   [`STUCK_SECS`];
//! * no worker on a Gather or ReturnCargo order goes [`GATHER_STALL_SECS`]
//!   without loading, unloading or getting closer to where it walks (its
//!   source or a drop site);
//! * no production queue stalls for [`QUEUE_STALL_SECS`] unless its front
//!   unit waits for population room;
//! * effect boxes stay under [`FX_LIMIT`] and no other entities pile up;
//! * every `Dead` entity has exactly one `EntityDied`, and is gone after
//!   `Cleanup`; no unit or building disappears without one;
//! * each player's kills equal the deaths credited to them, and each
//!   player's losses the deaths they suffered to enemy fire.
//!
//! Violations are hard (bookkeeping, numbers, leaks, terrain, any overlap
//! past the tolerance) or soft (a unit stuck in a jam, a worker stalled).
//! The quick matches, run by default (a few minutes of simulation each),
//! allow neither. The long sweeps, `#[ignore]`d, allow no hard violation
//! and at most one soft one per [`SOFT_BUDGET_UNIT_MINUTES`] unit-minutes
//! simulated, so they catch a regression without failing on the rare jam.
//! Each match also reports its overlap statistics (the deepest overlap of
//! either kind, and how many unit-pair-ticks passed each of
//! [`DEPTH_BUCKETS`]); a sweep ends with the totals over its matches, which
//! run in parallel:
//!
//! ```text
//! cargo test -p aetherfall --test invariants -- --ignored --nocapture
//! AF_SWEEP=6:9:30,4:40:20 cargo test -p aetherfall --test invariants sweep_custom -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use aetherfall::*;
use af_core::ext::movement::CollisionStats;
use af_core::prelude::*;
use af_core::spawn::SpawnStats;
use bevy::ecs::entity::Entities;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// Ticks between whole-world checks (two sim seconds).
const SNAPSHOT_TICKS: u64 = 60;
/// Deepest box overlap tolerated at the end of a tick (metres); anything
/// deeper is a hard violation.
const OVERLAP_TOLERANCE: f32 = 0.1;
/// Overlap depths counted in the statistics (metres).
const DEPTH_BUCKETS: [f32; 4] = [0.01, 0.05, 0.1, 0.3];
/// One-tick moves of a ground unit counted in the statistics (metres): the
/// fastest walker covers about 0.3 m a tick, so anything past these is a
/// unit popping from one spot to another before the player's eyes.
const JUMP_BUCKETS: [f32; 3] = [0.4, 0.75, 1.5];
/// Long sweeps allow one soft violation per this many unit-minutes.
const SOFT_BUDGET_UNIT_MINUTES: f32 = 300.0;
/// A mover that stays this close to one spot...
const STUCK_RADIUS: f32 = 2.0;
/// ...for this long (sim seconds) is stuck.
const STUCK_SECS: f32 = 20.0;
/// A worker this long without progress is stalled (sim seconds).
const GATHER_STALL_SECS: f32 = 30.0;
/// A worker getting this much closer to where it walks makes progress (metres).
const GATHER_APPROACH: f32 = 1.0;
/// A production queue this long without progress is blocked (sim seconds).
const QUEUE_STALL_SECS: f32 = 60.0;
/// Most effect boxes alive at once (af_combat's soft cap plus a tick's worth).
const FX_LIMIT: usize = 6000;
/// Entities that are not units, buildings, nodes or effects may grow by at
/// most this many over a match.
const STRAY_LIMIT: u32 = 64;

// ---------------------------------------------------------------------------
// The watcher
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Violation {
    time: f32,
    kind: &'static str,
    entity: Option<Entity>,
    detail: String,
}

/// Kinds of violation a crowd can cause now and then (see the module docs).
const SOFT_KINDS: [&str; 3] = ["stuck mover", "stalled worker", "blocked production queue"];

impl Violation {
    fn soft(&self) -> bool {
        SOFT_KINDS.contains(&self.kind)
    }
}

#[derive(Clone, Copy, Debug)]
struct MoverTrack {
    anchor: Vec2,
    since: f32,
    seen: u64,
}

#[derive(Clone, Copy, Debug)]
struct WorkerTrack {
    target: Option<Entity>,
    amount: f32,
    /// What the worker walks to (followed entity, else point) and the
    /// closest it has come since the last progress.
    heading_for: Option<(Option<Entity>, Vec2)>,
    best: f32,
    since: f32,
    seen: u64,
}

#[derive(Clone, Copy, Debug)]
struct QueueTrack {
    len: usize,
    progress: f32,
    since: f32,
    seen: u64,
}

/// The deepest overlap seen of one kind.
#[derive(Clone, Debug, Default)]
struct Deepest {
    depth: f32,
    time: f32,
    what: String,
}

/// Overlaps seen at the ends of ticks, of one kind (unit vs unit, or unit
/// vs building or blocking node).
#[derive(Clone, Debug, Default)]
struct OverlapStats {
    deepest: Deepest,
    /// Unit-pair-ticks deeper than each of [`DEPTH_BUCKETS`].
    over: [u64; DEPTH_BUCKETS.len()],
}

impl OverlapStats {
    fn record(&mut self, depth: f32, time: f32, what: impl FnOnce() -> String) {
        for (count, bucket) in self.over.iter_mut().zip(DEPTH_BUCKETS) {
            if depth > bucket {
                *count += 1;
            }
        }
        if depth > self.deepest.depth {
            self.deepest = Deepest { depth, time, what: what() };
        }
    }

    fn merge(&mut self, other: &OverlapStats, title: &str) {
        for (a, b) in self.over.iter_mut().zip(other.over) {
            *a += b;
        }
        if other.deepest.depth > self.deepest.depth {
            self.deepest = Deepest { what: format!("{title}: {}", other.deepest.what), ..other.deepest.clone() };
        }
    }

    fn describe(&self) -> String {
        let buckets: Vec<String> = DEPTH_BUCKETS.iter().zip(self.over).map(|(b, n)| format!(">{b}: {n}")).collect();
        let worst = if self.deepest.depth > 0.0 { format!("worst {:.3} m at t={:.1}s ({})", self.deepest.depth, self.deepest.time, self.deepest.what) } else { "none".to_string() };
        format!("{worst}; pair-ticks {}", buckets.join(", "))
    }
}

/// One-tick moves of ground units past [`JUMP_BUCKETS`], and the longest.
#[derive(Clone, Debug, Default)]
struct JumpStats {
    longest: Deepest,
    /// Unit-ticks moving farther than each of [`JUMP_BUCKETS`].
    over: [u64; JUMP_BUCKETS.len()],
}

impl JumpStats {
    fn record(&mut self, jump: f32, time: f32, what: impl FnOnce() -> String) {
        for (count, bucket) in self.over.iter_mut().zip(JUMP_BUCKETS) {
            if jump > bucket {
                *count += 1;
            }
        }
        if jump > self.longest.depth {
            self.longest = Deepest { depth: jump, time, what: what() };
        }
    }

    fn merge(&mut self, other: &JumpStats, title: &str) {
        for (a, b) in self.over.iter_mut().zip(other.over) {
            *a += b;
        }
        if other.longest.depth > self.longest.depth {
            self.longest = Deepest { what: format!("{title}: {}", other.longest.what), ..other.longest.clone() };
        }
    }

    fn describe(&self) -> String {
        let buckets: Vec<String> = JUMP_BUCKETS.iter().zip(self.over).map(|(b, n)| format!(">{b} m: {n}")).collect();
        format!("longest {:.2} m at t={:.1}s ({}); unit-ticks {}", self.longest.depth, self.longest.time, self.longest.what, buckets.join(", "))
    }
}

/// Everything the checks remember between ticks, and what they found.
#[derive(Resource, Debug, Default)]
struct Watch {
    violations: Vec<Violation>,
    /// (kind, entity) already reported: a lasting fault is reported once.
    reported: HashSet<(&'static str, Option<Entity>)>,
    movers: HashMap<Entity, MoverTrack>,
    workers: HashMap<Entity, WorkerTrack>,
    queues: HashMap<Entity, QueueTrack>,
    /// Entities whose `EntityDied` was written in the last tick.
    died_last_tick: Vec<Entity>,
    /// Every entity an `EntityDied` was ever written for.
    died: HashSet<Entity>,
    /// Units and buildings alive at the last snapshot.
    known: HashSet<Entity>,
    /// Kills each player should have been credited with, from `EntityDied`.
    kills: Vec<u32>,
    /// Deaths to enemy fire each player suffered, from `EntityDied`.
    losses: Vec<u32>,
    /// Entities other than units, buildings, nodes and effects at the first snapshot.
    strays: Option<u32>,
    peak_fx: usize,
    peak_units: usize,
    /// Units alive summed over snapshots, times the snapshot interval.
    unit_minutes: f32,
    snapshots: u32,
    /// Ground unit vs ground unit, and vs buildings and blocking nodes.
    unit_overlaps: OverlapStats,
    static_overlaps: OverlapStats,
    /// Where each ground unit stood at the last check, and its one-tick moves.
    last_pos: HashMap<Entity, Vec2>,
    jumps: JumpStats,
}

impl Watch {
    fn flag(&mut self, time: f32, kind: &'static str, entity: Option<Entity>, detail: impl FnOnce() -> String) {
        if self.reported.insert((kind, entity)) {
            self.violations.push(Violation { time, kind, entity, detail: detail() });
        }
    }
}

struct WatchPlugin;

impl Plugin for WatchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Watch>().add_systems(
            FixedUpdate,
            (
                (check_overlaps, follow_progress, check_world.run_if(|clock: Res<SimClock>| clock.tick.is_multiple_of(SNAPSHOT_TICKS))).chain().in_set(SimSet::Ai),
                audit_deaths.in_set(SimSet::Cleanup),
            ),
        );
    }
}

type UnitView<'a> = (
    Entity,
    &'a Unit,
    &'a Owner,
    &'a Position,
    &'a Facing,
    &'a Footprint,
    &'a Mobile,
    &'a Health,
    &'a PathState,
    &'a Orders,
    Option<&'a MoveGoal>,
    Option<&'a Gatherer>,
);

type BuildingView<'a> = (Entity, &'a Building, &'a Owner, &'a Position, &'a Facing, &'a Health, &'a ProductionQueue, Has<UnderConstruction>);

/// What the checks read.
#[derive(SystemParam)]
struct World<'w, 's> {
    data: Res<'w, Data>,
    players: Res<'w, Players>,
    nav: Res<'w, Nav>,
    spatial: Res<'w, SpatialIndex>,
    clock: Res<'w, SimClock>,
    units: Query<'w, 's, UnitView<'static>, Without<Dead>>,
    buildings: Query<'w, 's, BuildingView<'static>, Without<Dead>>,
    boxes: Query<'w, 's, (&'static Position, &'static Facing, &'static Footprint)>,
    nodes: Query<'w, 's, &'static ResourceNode>,
    fields: Query<'w, 's, &'static ResourceField>,
}

impl World<'_, '_> {
    fn unit_name(&self, e: Entity) -> String {
        match self.units.get(e) {
            Ok((_, u, o, p, ..)) => format!("{} {e} of P{} at ({:.1}, {:.1})", self.data.unit(u.kind).id, o.0.0, p.0.x, p.0.y),
            Err(_) => format!("{e}"),
        }
    }

    /// What is left in a node or field (`None` when it is gone).
    fn remaining(&self, e: Entity) -> Option<f32> {
        self.nodes.get(e).map(|n| n.remaining).or_else(|_| self.fields.get(e).map(|f| f.remaining)).ok()
    }

    /// Edge distance from a box to an entity's box (`None` when it is gone).
    fn gap_to(&self, me: &Obb, e: Entity) -> Option<f32> {
        self.boxes.get(e).ok().map(|(p, f, fp)| me.gap(&fp.obb(p.0, f.0)))
    }
}

/// Every tick, from the spatial index of the world as the last tick left
/// it: no ground unit overlaps another ground unit, a building (its own
/// team's gates aside) or a blocking node past the tolerance.
fn check_overlaps(w: World, gates: Query<&Owner, With<Gate>>, mut watch: ResMut<Watch>) {
    let now = w.clock.elapsed;
    let watch = &mut *watch;
    // One-tick moves (a unit's first tick has none).
    let last = std::mem::take(&mut watch.last_pos);
    for e in w.spatial.entries.iter().filter(|e| e.kind == SpatialKind::Unit && !e.flying) {
        if let Some(was) = last.get(&e.entity) {
            let jump = e.pos.distance(*was);
            watch.jumps.record(jump, now, || format!("{} from ({:.1}, {:.1})", w.unit_name(e.entity), was.x, was.y));
        }
        watch.last_pos.insert(e.entity, e.pos);
    }
    for e in &w.spatial.entries {
        if e.kind != SpatialKind::Unit || e.flying {
            continue;
        }
        w.spatial.for_each_near(e.pos, e.radius, |other| {
            let unit = match other.kind {
                SpatialKind::Unit if other.entity > e.entity && !other.flying => true,
                SpatialKind::Building if gates.get(other.entity).is_ok_and(|g| w.players.team(g.0) == e.team) => return,
                SpatialKind::Building => false,
                SpatialKind::Node if other.blocking => false,
                _ => return,
            };
            let Some(depth) = e.obb.mtv(&other.obb).map(|m| m.length()) else { return };
            let what = || {
                let name = match other.kind {
                    SpatialKind::Unit => w.unit_name(other.entity),
                    SpatialKind::Building => format!("{} {}", w.data.building(BuildingTypeId(other.type_index)).id, other.entity),
                    SpatialKind::Node => format!("{} {}", w.data.node(NodeTypeId(other.type_index)).id, other.entity),
                };
                format!("{} vs {name} by {depth:.3} m", w.unit_name(e.entity))
            };
            let stats = if unit { &mut watch.unit_overlaps } else { &mut watch.static_overlaps };
            stats.record(depth, now, what);
            if depth > OVERLAP_TOLERANCE {
                let kind = if unit { "ground units overlap" } else { "unit inside an obstacle" };
                watch.flag(now, kind, Some(e.entity), what);
            }
        });
    }
}

/// Every tick: dead entities are gone, and movers, workers and queues make progress.
fn follow_progress(w: World, dead: Query<Entity, With<Dead>>, everything: Query<Entity>, mut watch: ResMut<Watch>) {
    let now = w.clock.elapsed;
    let tick = w.clock.tick;
    let watch = &mut *watch;

    for e in &dead {
        watch.flag(now, "Dead entity survived Cleanup", Some(e), || format!("{e} still carries Dead after Cleanup"));
    }
    for e in std::mem::take(&mut watch.died_last_tick) {
        if everything.contains(e) {
            watch.flag(now, "EntityDied entity not despawned", Some(e), || format!("{e} died last tick and still exists"));
        }
    }

    for (e, _, _, pos, facing, fp, _, _, path, orders, goal, gatherer) in &w.units {
        // Movers: a unit trying to reach a goal keeps leaving the spot it is on.
        if goal.is_some() && path.status != PathStatus::Arrived {
            let t = watch.movers.entry(e).or_insert(MoverTrack { anchor: pos.0, since: now, seen: tick });
            t.seen = tick;
            if pos.0.distance(t.anchor) > STUCK_RADIUS {
                *t = MoverTrack { anchor: pos.0, since: now, seen: tick };
            } else if now - t.since > STUCK_SECS {
                let since = t.since;
                let detail = || {
                    let g = goal.expect("goal");
                    format!(
                        "{} within {STUCK_RADIUS} m since t={since:.0}s: {:?}, path {:?} ({} of {} waypoints, partial {}), goal ({:.1}, {:.1}) arrive {:.2} follow {:?}",
                        w.unit_name(e),
                        orders.current(),
                        path.status,
                        path.next,
                        path.waypoints.len(),
                        path.partial,
                        g.target.x,
                        g.target.y,
                        g.arrive,
                        g.follow
                    )
                };
                watch.flag(now, "stuck mover", Some(e), detail);
            }
        }

        // Workers: loading, unloading or closing in on their source or drop site.
        let Some(g) = gatherer else { continue };
        let target = match orders.current() {
            Some(Order::Gather { target }) => Some(*target),
            Some(Order::ReturnCargo) => None,
            _ => continue,
        };
        let me = fp.obb(pos.0, facing.0);
        let heading_for = goal.map(|g| (g.follow, g.target));
        let dist = match heading_for {
            Some((Some(e), _)) => w.gap_to(&me, e).unwrap_or(f32::INFINITY),
            Some((None, p)) => pos.0.distance(p),
            None => f32::INFINITY,
        };
        let fresh = WorkerTrack { target, amount: g.amount, heading_for, best: dist, since: now, seen: tick };
        let t = watch.workers.entry(e).or_insert(fresh);
        t.seen = tick;
        let new_walk = t.heading_for.map(|h| h.0) != heading_for.map(|h| h.0);
        if t.target != target || new_walk || (t.amount - g.amount).abs() > 1e-3 || dist < t.best - GATHER_APPROACH {
            *t = fresh;
        } else if now - t.since > GATHER_STALL_SECS {
            let since = t.since;
            let detail = || {
                format!(
                    "{} without progress since t={since:.0}s: {:?}, carrying {:.1} {:?}, source left {:?}, {:.1} m to go, path {:?}, goal {:?}",
                    w.unit_name(e),
                    orders.current(),
                    g.amount,
                    g.carrying,
                    target.and_then(|t| w.remaining(t)),
                    dist,
                    path.status,
                    goal.map(|g| (g.target, g.follow))
                )
            };
            watch.flag(now, "stalled worker", Some(e), detail);
        }
    }
    watch.movers.retain(|_, t| t.seen == tick);
    watch.workers.retain(|_, t| t.seen == tick);

    // Queues: the front item advances unless it waits for population room.
    for (e, b, owner, pos, _, _, queue, constructing) in &w.buildings {
        let Some(front) = queue.items.front().filter(|_| !constructing) else { continue };
        let t = watch.queues.entry(e).or_insert(QueueTrack { len: queue.items.len(), progress: queue.progress, since: now, seen: tick });
        t.seen = tick;
        if t.len != queue.items.len() || t.progress != queue.progress {
            *t = QueueTrack { len: queue.items.len(), progress: queue.progress, since: now, seen: tick };
            continue;
        }
        if now - t.since <= QUEUE_STALL_SECS {
            continue;
        }
        let p = w.players.get(owner.0);
        let housed_out = match front.kind {
            QueueKind::Unit(u) => p.pop_used + w.data.unit(u).pop * w.data.unit(u).squad > p.pop_cap,
            QueueKind::Tech(_) | QueueKind::Upgrade(_) => false,
        };
        if housed_out {
            t.since = now;
            continue;
        }
        let since = t.since;
        let detail = || {
            format!(
                "{} {e} of P{} at ({:.0}, {:.0}) stuck since t={since:.0}s on {:?} at {:.1}/{:.1}s (pop {}/{}, pop_blocked {})",
                w.data.building(b.kind).id,
                owner.0.0,
                pos.0.x,
                pos.0.y,
                front.kind,
                queue.progress,
                front.time,
                p.pop_used,
                p.pop_cap,
                queue.pop_blocked
            )
        };
        watch.flag(now, "blocked production queue", Some(e), detail);
    }
    watch.queues.retain(|_, t| t.seen == tick);
}

/// Every snapshot: terrain, finite numbers, recounts, leaks, vanished entities.
fn check_world(w: World, shields: Query<(Entity, &Shield)>, fx: Query<&FxBox>, entities: &Entities, map: Res<MapInfo>, mut watch: ResMut<Watch>) {
    let now = w.clock.elapsed;
    let watch = &mut *watch;
    watch.snapshots += 1;
    let size = map.size;
    let inside = |p: Vec2| p.x >= 0.0 && p.y >= 0.0 && p.x <= size.x && p.y <= size.y;

    // Terrain, map bounds.
    for (e, _, _, pos, facing, _, mob, ..) in &w.units {
        if !pos.0.is_finite() || !facing.0.is_finite() {
            watch.flag(now, "non-finite position", Some(e), || format!("{} at {:?} facing {}", w.unit_name(e), pos.0, facing.0));
            continue;
        }
        if !inside(pos.0) {
            watch.flag(now, "unit outside the map", Some(e), || w.unit_name(e));
        }
        if mob.flying {
            continue;
        }
        if !w.nav.terrain_passable(w.nav.cell_of(pos.0), mob.mobility.0) {
            let terrain = w.data.terrain_def(TerrainId(w.nav.terrain_at(w.nav.cell_of(pos.0)))).id.clone();
            watch.flag(now, "unit on forbidden terrain", Some(e), || format!("{} stands on {terrain}", w.unit_name(e)));
        }
    }

    // Finite, sane numbers.
    for (e, _, _, _, _, _, _, health, ..) in &w.units {
        check_health(watch, now, e, health, || w.unit_name(e));
    }
    for (e, b, owner, _, facing, health, queue, _) in &w.buildings {
        check_health(watch, now, e, health, || format!("{} {e} of P{}", w.data.building(b.kind).id, owner.0.0));
        if !facing.0.is_finite() {
            watch.flag(now, "non-finite position", Some(e), || format!("building {e} facing {}", facing.0));
        }
        if queue.items.len() > ProductionQueue::MAX || !queue.progress.is_finite() {
            watch.flag(now, "bad production queue", Some(e), || format!("{e}: {} items, progress {}", queue.items.len(), queue.progress));
        }
    }
    for (e, s) in &shields {
        if !(s.hp.is_finite() && s.max.is_finite()) || s.hp < -1e-3 || s.hp > s.max + 1e-3 {
            watch.flag(now, "bad shield", Some(e), || format!("{e}: shield {} / {}", s.hp, s.max));
        }
    }
    for (e, _, _, _, _, _, _, _, _, _, _, g) in &w.units {
        if let Some(g) = g.filter(|g| !g.amount.is_finite() || g.amount < -1e-3 || g.amount > g.capacity + 1e-3) {
            watch.flag(now, "bad cargo", Some(e), || format!("{} carries {} of {}", w.unit_name(e), g.amount, g.capacity));
        }
    }
    for n in &w.nodes {
        if !n.remaining.is_finite() || n.remaining < 0.0 {
            watch.flag(now, "bad node amount", None, || format!("{:?} node holds {}", n.kind, n.remaining));
        }
    }
    for f in &w.fields {
        if !f.remaining.is_finite() || f.remaining < 0.0 || f.users > f.slots {
            watch.flag(now, "bad field", None, || format!("field holds {}, {} of {} slots used", f.remaining, f.users, f.slots));
        }
    }
    for p in w.players.iter() {
        for r in w.data.resource_ids() {
            let v = p.resources.get(r);
            if !v.is_finite() || v < -1e-3 {
                watch.flag(now, "bad resource amount", None, || format!("P{} has {v} {}", p.id.0, w.data.resource(r).id));
            }
        }
    }
    let mut fx_count = 0;
    for b in &fx {
        fx_count += 1;
        if !(b.pos.is_finite() && b.dir.is_finite() && b.size.is_finite() && b.alpha.is_finite()) {
            watch.flag(now, "non-finite effect box", None, || format!("{b:?}"));
        }
    }

    // Recount (Prepare ran this tick; nothing has spawned since).
    for p in w.players.iter() {
        let mut units = vec![0u32; p.units_owned.len()];
        let mut done = vec![0u32; p.buildings_owned.len()];
        let mut any = vec![0u32; p.buildings_any.len()];
        let mut pop = 0;
        let mut cap = 0;
        for (_, u, o, ..) in &w.units {
            if o.0 == p.id {
                units[u.kind.index()] += 1;
                pop += w.data.unit(u.kind).pop;
            }
        }
        for (_, b, o, _, _, _, _, constructing) in &w.buildings {
            if o.0 == p.id {
                any[b.kind.index()] += 1;
                if !constructing {
                    done[b.kind.index()] += 1;
                    cap += w.data.building(b.kind).pop;
                }
            }
        }
        let cap = cap.min(POP_HARD_CAP);
        if (pop, cap) != (p.pop_used, p.pop_cap) || units != p.units_owned || done != p.buildings_owned || any != p.buildings_any {
            watch.flag(now, "player counts disagree with a recount", None, || {
                format!("P{}: pop {}/{} recounted {pop}/{cap}, per-type counts equal: units {}, buildings {}, any {}", p.id.0, p.pop_used, p.pop_cap, units == p.units_owned, done == p.buildings_owned, any == p.buildings_any)
            });
        }
    }

    // Leaks.
    let units = w.units.iter().count();
    let buildings = w.buildings.iter().count();
    let world_objects = (units + buildings + w.nodes.iter().count() + fx_count) as u32;
    let strays = entities.count_spawned().saturating_sub(world_objects);
    let baseline = *watch.strays.get_or_insert(strays);
    if strays > baseline + STRAY_LIMIT {
        watch.flag(now, "entities pile up", None, || format!("{strays} entities besides units, buildings, nodes and effects (was {baseline})"));
    }
    if fx_count > FX_LIMIT {
        watch.flag(now, "effect boxes pile up", None, || format!("{fx_count} effect boxes"));
    }
    watch.peak_fx = watch.peak_fx.max(fx_count);
    watch.peak_units = watch.peak_units.max(units);
    watch.unit_minutes += units as f32 * SNAPSHOT_TICKS as f32 / (60.0 * SIM_HZ as f32);

    // Units and buildings only leave through EntityDied.
    let alive: HashSet<Entity> = w.units.iter().map(|u| u.0).chain(w.buildings.iter().map(|b| b.0)).collect();
    let vanished: Vec<Entity> = watch.known.iter().filter(|e| !alive.contains(e) && !watch.died.contains(e)).copied().collect();
    for e in vanished {
        watch.flag(now, "vanished without EntityDied", Some(e), || format!("{e} is gone but never died"));
    }
    watch.known = alive;
}

fn check_health(watch: &mut Watch, now: f32, e: Entity, h: &Health, name: impl FnOnce() -> String) {
    if !(h.hp.is_finite() && h.max.is_finite()) || h.hp <= 0.0 || h.hp > h.max + 1e-3 {
        watch.flag(now, "bad hit points", Some(e), || format!("{} has {} / {} hp", name(), h.hp, h.max));
    }
}

/// At `Cleanup`: one `EntityDied` per `Dead` entity, and the kill books.
fn audit_deaths(clock: Res<SimClock>, players: Res<Players>, mut died: MessageReader<EntityDied>, dead: Query<Entity, With<Dead>>, mut watch: ResMut<Watch>) {
    let now = clock.elapsed;
    let watch = &mut *watch;
    let n = players.list.len();
    watch.kills.resize(n, 0);
    watch.losses.resize(n, 0);
    let mut this_tick = Vec::new();
    for d in died.read() {
        if !watch.died.insert(d.entity) {
            watch.flag(now, "died twice", Some(d.entity), || format!("{:?} got a second EntityDied", d));
        }
        this_tick.push(d.entity);
        let mortal = matches!(d.kind, EntityKind::Unit(_) | EntityKind::Building(_));
        if let (true, Some(killer), Some(owner)) = (mortal, d.killer, d.owner) {
            watch.losses[owner.index()] += 1;
            if players.hostile(killer, owner) {
                watch.kills[killer.index()] += 1;
            }
        }
    }
    for e in &dead {
        if !this_tick.contains(&e) {
            watch.flag(now, "Dead without EntityDied", Some(e), || format!("{e} was marked Dead without an EntityDied"));
        }
    }
    watch.died_last_tick = this_tick;
}

// ---------------------------------------------------------------------------
// Running matches
// ---------------------------------------------------------------------------

/// One AI match to watch.
#[derive(Clone, Debug)]
struct Match {
    seed: u64,
    factions: Vec<&'static str>,
    minutes: f32,
    start_age: u8,
    res_mult: f32,
    map_size: f32,
}

impl Match {
    fn new(seed: u64, factions: &[&'static str], minutes: f32) -> Match {
        Match { seed, factions: factions.to_vec(), minutes, start_age: 1, res_mult: 1.0, map_size: 0.0 }
    }

    /// Start late and rich on a small map, so armies meet within minutes.
    fn brawl(mut self) -> Match {
        self.start_age = 3;
        self.res_mult = 8.0;
        self.map_size = 160.0;
        self
    }
}

/// What a watched match found.
struct Outcome {
    title: String,
    violations: Vec<Violation>,
    summary: String,
    unit_minutes: f32,
    unit_overlaps: OverlapStats,
    static_overlaps: OverlapStats,
    jumps: JumpStats,
    collisions: CollisionStats,
    /// Trainings set down, and looks that found no room (`None`: the
    /// economy keeps no count).
    spawns: Option<SpawnStats>,
}

impl Outcome {
    fn describe(&self, limit: usize) -> String {
        let mut s = format!("{}: {}\n", self.title, self.summary);
        let _ = writeln!(s, "  unit vs unit:     {}", self.unit_overlaps.describe());
        let _ = writeln!(s, "  unit vs obstacle: {}", self.static_overlaps.describe());
        let _ = writeln!(s, "  one-tick moves:   {}", self.jumps.describe());
        let _ = writeln!(s, "  final word:       {}", self.collisions.describe());
        if let Some(spawns) = self.spawns {
            let _ = writeln!(s, "  trainings:        {} set down, {} looks found no room", spawns.spawned, spawns.waited);
        }
        let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
        for v in &self.violations {
            *by_kind.entry(v.kind).or_default() += 1;
        }
        for (kind, count) in &by_kind {
            let _ = writeln!(s, "  {count:>4} x {kind}");
        }
        for v in self.violations.iter().take(limit) {
            let _ = writeln!(s, "  t={:>6.1}s {} {:?}: {}", v.time, v.kind, v.entity, v.detail);
        }
        s
    }

    fn count(&self, kind: &str) -> usize {
        self.violations.iter().filter(|v| v.kind == kind).count()
    }
}

fn watch_match(m: &Match) -> Outcome {
    let data = af_core::testing::test_data();
    let opts = SkirmishOptions {
        seed: m.seed,
        players: m.factions.len(),
        factions: m.factions.iter().map(|s| s.to_string()).collect(),
        human: false,
        start_age: m.start_age,
        res_mult: m.res_mult,
        map_size: m.map_size,
        ..Default::default()
    };
    let setup = skirmish_setup(&data, &opts).expect("setup");
    let mut app = headless_app(data, setup, 30);
    app.add_plugins(WatchPlugin);
    run_headless(&mut app, (m.minutes * 60.0 * SIM_HZ as f32) as u64, 0, |_| {});

    let players = app.world().resource::<Players>().clone();
    let clock = *app.world().resource::<SimClock>();
    let mut watch = std::mem::take(&mut *app.world_mut().resource_mut::<Watch>());
    let collisions = app.world().resource::<CollisionStats>().clone();
    let spawns = app.world().get_resource::<SpawnStats>().copied();
    let now = clock.elapsed;
    for p in &players.list {
        let (kills, losses) = (watch.kills.get(p.id.index()).copied().unwrap_or(0), watch.losses.get(p.id.index()).copied().unwrap_or(0));
        if p.stats.kills != kills {
            watch.flag(now, "kill count disagrees with deaths", None, || format!("P{} credited {} kills, EntityDied names it killer of {kills} enemies", p.id.0, p.stats.kills));
        }
        if p.stats.units_lost + p.stats.buildings_lost != losses {
            watch.flag(now, "loss count disagrees with deaths", None, || {
                format!("P{} counts {} + {} losses, EntityDied shows {losses} deaths to fire", p.id.0, p.stats.units_lost, p.stats.buildings_lost)
            });
        }
    }
    let total_kills: u32 = players.list.iter().map(|p| p.stats.kills).sum();
    let total_losses: u32 = players.list.iter().map(|p| p.stats.units_lost + p.stats.buildings_lost).sum();
    let summary = format!(
        "{:.0}s simulated, {} snapshots, {:.0} unit-minutes, peak {} units, peak {} effect boxes, {total_kills} kills / {total_losses} losses, ages {:?}",
        clock.elapsed,
        watch.snapshots,
        watch.unit_minutes,
        watch.peak_units,
        watch.peak_fx,
        players.list.iter().map(|p| p.age).collect::<Vec<_>>()
    );
    watch.violations.sort_by(|a, b| a.time.total_cmp(&b.time));
    let title = format!("seed {} {} ({} min{})", m.seed, m.factions.join(","), m.minutes, if m.start_age > 1 { ", brawl" } else { "" });
    Outcome {
        title,
        violations: watch.violations,
        summary,
        unit_minutes: watch.unit_minutes,
        unit_overlaps: watch.unit_overlaps,
        static_overlaps: watch.static_overlaps,
        jumps: watch.jumps,
        collisions,
        spawns,
    }
}

/// Watch the matches side by side, one thread each (outcomes in order).
fn watch_matches(matches: &[Match]) -> Vec<Outcome> {
    std::thread::scope(|s| {
        let running: Vec<_> = matches.iter().map(|m| s.spawn(move || watch_match(m))).collect();
        running.into_iter().map(|h| h.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))).collect()
    })
}

/// How many soft violations a match may have.
#[derive(Clone, Copy, Debug)]
enum Tolerance {
    None,
    /// One per [`SOFT_BUDGET_UNIT_MINUTES`] unit-minutes (at least one).
    SoftBudget,
}

/// Watch every match, print what each found and the totals, and fail on
/// any hard violation and on more soft ones than `tolerance` allows.
fn assert_clean(matches: &[Match], tolerance: Tolerance) {
    let outcomes = watch_matches(matches);
    let mut failed = String::new();
    let (mut units, mut statics, mut jumps) = (OverlapStats::default(), OverlapStats::default(), JumpStats::default());
    let mut collisions = CollisionStats::default();
    let (mut hard_total, mut unit_minutes) = (0, 0.0);
    for o in &outcomes {
        println!("{}", o.describe(12));
        let soft = o.violations.iter().filter(|v| v.soft()).count();
        let hard = o.violations.len() - soft;
        let allowed = match tolerance {
            Tolerance::None => 0,
            Tolerance::SoftBudget => ((o.unit_minutes / SOFT_BUDGET_UNIT_MINUTES).floor() as usize).max(1),
        };
        if hard > 0 || soft > allowed {
            let _ = writeln!(failed, "{hard} hard and {soft} soft violations ({allowed} soft allowed)");
            failed.push_str(&o.describe(40));
        }
        units.merge(&o.unit_overlaps, &o.title);
        statics.merge(&o.static_overlaps, &o.title);
        jumps.merge(&o.jumps, &o.title);
        collisions.add(&o.collisions);
        hard_total += hard;
        unit_minutes += o.unit_minutes;
    }
    let soft_counts: Vec<String> = SOFT_KINDS.iter().map(|k| format!("{} {k}", outcomes.iter().map(|o| o.count(k)).sum::<usize>())).collect();
    println!(
        "TOTAL over {} matches ({unit_minutes:.0} unit-minutes): {hard_total} hard violations, {}\n  unit vs unit:     {}\n  unit vs obstacle: {}\n  one-tick moves:   {}\n  final word:       {}",
        outcomes.len(),
        soft_counts.join(", "),
        units.describe(),
        statics.describe(),
        jumps.describe(),
        collisions.describe()
    );
    assert!(failed.is_empty(), "invariant violations:\n{failed}");
}

const FACTIONS: [&str; 6] = ["ironhold", "sylvan", "dune", "tide", "steppe", "starborn"];

/// `n` factions, starting `offset` along the list.
fn factions_from(offset: usize, n: usize) -> Vec<&'static str> {
    (0..n).map(|k| FACTIONS[(offset + k) % FACTIONS.len()]).collect()
}

// ---------------------------------------------------------------------------
// Quick matches (default run)
// ---------------------------------------------------------------------------

/// The opening of a 1v1: economy, building, first troops.
#[test]
fn opening_1v1() {
    assert_clean(&[Match::new(4, &["ironhold", "dune"], 4.0)], Tolerance::None);
}

/// The opening of a three-player free-for-all.
#[test]
fn opening_three_players() {
    assert_clean(&[Match::new(3, &["sylvan", "tide", "steppe"], 3.0)], Tolerance::None);
}

/// Armies from every faction meet early: combat, projectiles, deaths.
#[test]
fn early_brawls() {
    assert_clean(&[Match::new(5, &["starborn", "ironhold"], 3.0).brawl(), Match::new(6, &["dune", "steppe", "sylvan", "tide"], 3.0).brawl()], Tolerance::None);
}

// ---------------------------------------------------------------------------
// Long sweeps (ignored)
// ---------------------------------------------------------------------------

/// Every faction against the next one, twenty minutes each.
#[test]
#[ignore = "long: cargo test -p aetherfall --test invariants -- --ignored --nocapture"]
fn sweep_1v1_every_faction() {
    let matches: Vec<Match> = (0..FACTIONS.len()).map(|i| Match::new(10 + i as u64, &factions_from(i, 2), 20.0)).collect();
    assert_clean(&matches, Tolerance::SoftBudget);
}

/// Six-player free-for-alls, thirty minutes each.
#[test]
#[ignore = "long: cargo test -p aetherfall --test invariants -- --ignored --nocapture"]
fn sweep_six_players() {
    let matches: Vec<Match> = [8, 21, 9].into_iter().map(|seed| Match::new(seed, &FACTIONS, 30.0)).collect();
    assert_clean(&matches, Tolerance::SoftBudget);
}

/// Three- and four-player brawls from the Starfall age, ten minutes each.
#[test]
#[ignore = "long: cargo test -p aetherfall --test invariants -- --ignored --nocapture"]
fn sweep_brawls() {
    let matches: Vec<Match> = (0..4).map(|i| Match::new(30 + i as u64, &factions_from(i, 3 + i % 2), 10.0).brawl()).collect();
    assert_clean(&matches, Tolerance::SoftBudget);
}

/// Seeds no tuning has seen, of every size, so the checks generalise
/// beyond the matches the solver was tuned against.
#[test]
#[ignore = "long: cargo test -p aetherfall --test invariants -- --ignored --nocapture"]
fn sweep_fresh_seeds() {
    let matches = [
        Match::new(41, &factions_from(1, 2), 25.0),
        Match::new(42, &factions_from(2, 3), 20.0),
        Match::new(43, &factions_from(3, 4), 20.0),
        Match::new(44, &FACTIONS, 25.0),
        Match::new(45, &factions_from(4, 2), 25.0),
        Match::new(46, &factions_from(5, 4), 20.0),
        Match::new(47, &factions_from(0, 3), 10.0).brawl(),
        Match::new(48, &factions_from(2, 4), 10.0).brawl(),
    ];
    assert_clean(&matches, Tolerance::SoftBudget);
}

/// The matches named in `AF_SWEEP` (see [`sweep_custom`]).
fn parse_sweep(spec: &str) -> Vec<Match> {
    spec.split(',')
        .map(|entry| {
            fn bad(entry: &str) -> ! {
                panic!("AF_SWEEP entry `{entry}`: want players:seed:minutes[:brawl][:o<offset>|:f<faction>+<faction>...]")
            }
            let parts: Vec<&str> = entry.trim().split(':').collect();
            let number = |i: usize| parts.get(i).and_then(|p| p.parse::<f32>().ok()).unwrap_or_else(|| bad(entry));
            let (players, seed, minutes) = (number(0) as usize, number(1) as u64, number(2));
            let (mut factions, mut brawl) = (factions_from(seed as usize, players.clamp(1, 8)), false);
            for flag in parts.iter().skip(3) {
                if let Some(n) = flag.strip_prefix('o').and_then(|n| n.parse().ok()) {
                    factions = factions_from(n, players.clamp(1, 8));
                } else if let Some(names) = flag.strip_prefix('f') {
                    let named: Option<Vec<&'static str>> = names.split('+').map(|n| FACTIONS.iter().copied().find(|f| *f == n)).collect();
                    factions = named.filter(|f| f.len() == players).unwrap_or_else(|| bad(entry));
                } else if *flag == "brawl" {
                    brawl = true;
                } else {
                    bad(entry);
                }
            }
            let m = Match::new(seed, &factions, minutes);
            if brawl { m.brawl() } else { m }
        })
        .collect()
}

#[test]
fn sweep_specs_parse() {
    let m = parse_sweep("2:4:30:fsylvan+tide, 6:21:30:o0, 3:7:10:brawl:o2");
    assert_eq!((m[0].seed, m[0].minutes, m[0].factions.as_slice()), (4, 30.0, ["sylvan", "tide"].as_slice()));
    assert_eq!(m[1].factions, FACTIONS);
    assert_eq!((m[2].start_age, m[2].factions.as_slice()), (3, ["dune", "tide", "steppe"].as_slice()));
}

/// The matches named in `AF_SWEEP`: comma-separated `players:seed:minutes`,
/// then optionally `:brawl` for a brawl, and `:o<N>` for the factions in
/// list order from the `N`th (default: from the seed's) or
/// `:f<faction>+<faction>...` to name them (one per player). Does nothing
/// without it.
#[test]
#[ignore = "on demand: AF_SWEEP=6:9:30:o0,4:40:20,2:4:30:fsylvan+tide cargo test -p aetherfall --test invariants sweep_custom -- --ignored --nocapture"]
fn sweep_custom() {
    let Ok(spec) = std::env::var("AF_SWEEP") else {
        println!("AF_SWEEP is not set: nothing to run");
        return;
    };
    assert_clean(&parse_sweep(&spec), Tolerance::SoftBudget);
}
