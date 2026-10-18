//! Per-player AI memory: what the AI ordered and is waiting on, what it has
//! seen of its enemies, and its attack state. Lives in the [`AiBrains`]
//! resource across thinks.

use std::collections::BTreeMap;

use af_core::prelude::*;
use bevy::prelude::*;

use crate::army::{Objective, RAZE_MEMORY, SEARCH_MEMORY, SEARCH_STEP};
use crate::view::{AiView, KnownBuilding};
use crate::{PENDING_TIMEOUT, RESEND_DELAY};

/// A spot where a placement silently failed is avoided this long.
pub(crate) const BAD_SPOT_TIME: f32 = 60.0;
/// A rally order is not repeated within this many seconds, even when the
/// building's rally point still differs (the order may have been refused).
pub(crate) const RALLY_RESEND: f32 = 10.0;
/// Time constant (seconds) of the smoothed income estimate.
pub const INCOME_WINDOW: f32 = 60.0;
/// Sightings of hostile armies fade with this half-life (seconds).
pub const INTEL_HALF_LIFE: f32 = 150.0;
/// A player who hit us counts as an aggressor for this long (seconds).
pub const GRUDGE_TIME: f32 = 150.0;
/// Attack targets remembered, most recent last.
const TARGET_HISTORY: usize = 4;
/// Time constant (seconds) of the smoothed gatherer split.
pub const SPLIT_WINDOW: f32 = 20.0;
/// Time constant (seconds) of the smoothed army and building spending.
pub const SPEND_WINDOW: f32 = 90.0;

/// Memory of every AI player, indexed by `PlayerId`.
#[derive(Resource, Clone, Debug, Default)]
pub struct AiBrains {
    pub players: Vec<AiMemory>,
}

impl AiBrains {
    pub fn memory(&self, p: PlayerId) -> Option<&AiMemory> {
        self.players.get(p.index())
    }

    /// The player's memory, created on first use.
    pub fn memory_mut(&mut self, p: PlayerId) -> &mut AiMemory {
        if self.players.len() <= p.index() {
            self.players.resize_with(p.index() + 1, AiMemory::default);
        }
        &mut self.players[p.index()]
    }
}

/// A placement the AI issued that has not shown up as a building yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PendingBuild {
    pub kind: BuildingTypeId,
    pub pos: Vec2,
    pub obb: Obb,
    pub time: f32,
}

/// The last job given to a worker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkerOrder {
    pub target: Entity,
    pub time: f32,
}

/// Remote clusters of one resource found by a whole-map search.
pub const MAX_REMOTE_CLUSTERS: usize = 3;

/// Result of a whole-map search for a resource's nearest remote clusters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RemoteSearch {
    /// A gatherable node in each cluster found, nearest first (all None =
    /// there is none).
    pub clusters: [Option<Vec2>; MAX_REMOTE_CLUSTERS],
    /// Sim time of the search.
    pub at: f32,
}

impl RemoteSearch {
    /// The clusters found, nearest first.
    pub fn found(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.clusters.iter().flatten().copied()
    }
}

/// An "under attack" report for this player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alert {
    /// Where the victim was hit.
    pub at: Vec2,
    /// Where the attacker is (the victim's position when unknown).
    pub attacker: Vec2,
    /// Who attacked, when known.
    pub attacker_owner: Option<PlayerId>,
    pub time: f32,
}

/// What the AI has seen of hostile armies: population per unit type,
/// keeping recent peaks and fading over [`INTEL_HALF_LIFE`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnemyIntel {
    /// Seen hostile army population, by `UnitTypeId` index.
    pub pop: Vec<f32>,
    /// Sim time of the last update.
    pub at: f32,
}

impl EnemyIntel {
    /// Fold in one sighting: `seen` = hostile army pop in sight now, by unit type.
    pub fn observe(&mut self, now: f32, seen: &[f32]) {
        if self.pop.len() < seen.len() {
            self.pop.resize(seen.len(), 0.0);
        }
        let fade = 0.5f32.powf((now - self.at).max(0.0) / INTEL_HALF_LIFE);
        for (have, s) in self.pop.iter_mut().zip(seen.iter().chain(std::iter::repeat(&0.0))) {
            *have = (*have * fade).max(*s);
        }
        self.at = now;
    }

    /// Total remembered hostile army population.
    pub fn total(&self) -> f32 {
        self.pop.iter().sum()
    }

    /// Remembered population of unit types matching `f`.
    pub fn pop_where(&self, mut f: impl FnMut(UnitTypeId) -> bool) -> f32 {
        self.pop.iter().enumerate().filter(|(i, _)| f(UnitTypeId(*i as u16))).map(|(_, p)| p).sum()
    }
}

/// The army's state machine.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AttackState {
    /// The army collects at the rally point.
    #[default]
    Gathering,
    /// The attack group (`AiMemory::attackers`) is attack-moving at `goal`
    /// since `since` (sim seconds). `best_dist` is the closest any attacker
    /// has come to the goal, last improved at `progress_at`: an attack that
    /// stops getting closer is stuck (behind a wall, or the goal is out of
    /// reach). `razing`: the group stands in the goal's town and beat what
    /// defended it, so its building breakers tear the buildings down one by
    /// one while the others guard them (see `Think::raze`).
    Attacking { goal: Objective, since: f32, best_dist: f32, progress_at: f32, razing: bool },
}

impl AttackState {
    /// An attack on `goal`, launched at `since`, (re)aimed at `now`.
    pub fn attacking(goal: Objective, since: f32, now: f32) -> AttackState {
        AttackState::Attacking { goal, since, best_dist: f32::INFINITY, progress_at: now, razing: false }
    }

    /// Is the attack razing a town?
    pub fn razing(&self) -> bool {
        matches!(self, AttackState::Attacking { razing: true, .. })
    }
}

/// Construction progress of a foundation the AI staffed, to spot sites
/// that never get built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SiteWatch {
    pub site: Entity,
    /// Last progress seen (0..=1).
    pub progress: f32,
    /// When `progress` last grew (or the watch began).
    pub since: f32,
}

/// Everything one AI player remembers between thinks.
#[derive(Clone, Debug, Default)]
pub struct AiMemory {
    /// Placements issued and not yet seen as buildings.
    pub pending: Vec<PendingBuild>,
    /// Spots where a placement silently failed: (kind, pos, until).
    pub bad_spots: Vec<(BuildingTypeId, Vec2, f32)>,
    /// Spot searches that found nothing: (kind, retry after).
    pub no_spot: Vec<(BuildingTypeId, f32)>,
    /// Last job given to each worker (construction jobs have a PLACEHOLDER target).
    pub worker_orders: BTreeMap<Entity, WorkerOrder>,
    /// Nodes/fields workers failed to work: (target, until).
    pub bad_targets: Vec<(Entity, f32)>,
    /// Foundations a builder was sent to: (site, time).
    pub builder_sent: Vec<(Entity, f32)>,
    /// Progress of staffed foundations (stuck ones are deleted).
    pub site_watch: Vec<SiteWatch>,
    /// Research and upgrade orders recently sent (not repeated within `RESEND_DELAY`).
    pub research_sent: Vec<(TechId, f32)>,
    pub upgrade_sent: Vec<(Entity, f32)>,
    /// Production buildings recently told to rally: (building, time).
    pub rally_sent: Vec<(Entity, f32)>,
    /// Army units last moved (rally, defence, retarget): (unit, time).
    pub army_moved: Vec<(Entity, f32)>,
    /// Last whole-map search per resource with no nodes near home.
    pub remote: [Option<RemoteSearch>; MAX_RESOURCES],
    /// Hostile buildings seen, sorted by entity.
    pub known: Vec<KnownBuilding>,
    /// The AI's one intentional cheat: every unit, seen or not, of each
    /// hostile player in the game that has no building left that keeps it
    /// there, as (owner, unit, position), sorted by unit and refreshed every
    /// think (see `update_remnants` in the crate root).
    pub remnants: Vec<(PlayerId, Entity, Vec2)>,
    /// Remnant units a hunt could not close in on: (unit, until); see
    /// `Think::press_attack`.
    pub lost_prey: Vec<(Entity, f32)>,
    pub attack: AttackState,
    /// Units in the current attack group, sorted.
    pub attackers: Vec<Entity>,
    /// Hostile start positions already visited and found empty.
    pub visited: Vec<PlayerId>,
    /// Spots a hunt searched: (position, until); see `army::search_spot`.
    pub searched: Vec<(Vec2, f32)>,
    /// Attack goals the army could not get any closer to: (position, until).
    pub unreachable: Vec<(Vec2, f32)>,
    /// When reinforcements last left for the attack.
    pub last_reinforce: f32,
    /// When the last wave left (None before the first).
    pub last_wave: Option<f32>,
    /// No new attack before this time (after a retreat or recall).
    pub next_attack_after: f32,
    /// Recent shortfalls of purchases (buildings, and units for a short
    /// army), decaying; added to gathering demand.
    pub shortfall: ResAmounts,
    /// Alerts received since the last think.
    pub alerts: Vec<Alert>,
    /// Where defenders are sent, and until when the base counts as under attack.
    pub defend_pos: Vec2,
    pub defend_until: f32,
    /// Smoothed income per second, by resource (see [`AiMemory::observe_income`]).
    pub income: ResAmounts,
    /// `PlayerStats::gathered` at the last income sample, and when it was taken.
    pub income_sample: Option<(f32, ResAmounts)>,
    /// Hostile armies seen lately.
    pub intel: EnemyIntel,
    /// When each hostile player last hit us: (player, time).
    pub grudges: Vec<(PlayerId, f32)>,
    /// Players the last attacks were aimed at, most recent last.
    pub target_history: Vec<PlayerId>,
    /// Army population of the current attack group when it launched (or
    /// grew with reinforcements); a group far below it is losing.
    pub wave_pop: u32,
    /// When a wall was last ordered.
    pub wall_sent: Option<f32>,
    /// Gatherers wanted per resource, smoothed over [`SPLIT_WINDOW`] so that
    /// workers are not shuffled on every swing of demand; and when it was updated.
    pub split: Option<([f32; MAX_RESOURCES], f32)>,
    /// The age last seen and when it was first seen.
    pub age_seen: Option<(u8, f32)>,
    /// When the last debug status line was logged.
    pub status_logged: f32,
    /// When a wave last razed (see `AttackState::Attacking::razing`), and
    /// whose town.
    pub last_razing: Option<f32>,
    pub raze_victim: Option<PlayerId>,
    /// The building the attack razes (the goal), and the breakers sent at
    /// it or at buildings next to it: (breaker, building), sorted.
    pub raze_focus: Option<Entity>,
    pub raze_sent: Vec<(Entity, Entity)>,
    /// What army training and building placements actually spent lately,
    /// per second (smoothed over [`SPEND_WINDOW`]; see
    /// [`AiMemory::observe_spending`]).
    pub army_spend: ResAmounts,
    pub build_spend: ResAmounts,
    /// When spending was last observed.
    pub spend_at: Option<f32>,
}

impl AiMemory {
    /// Drop what is stale: resolved or expired placements, orders to dead
    /// entities, timed-out cooldowns.
    pub fn prune(&mut self, view: &AiView) {
        let now = view.time;
        let snap = view.snap;
        let mut expired = Vec::new();
        self.pending.retain(|p| {
            let placed = snap.buildings.iter().any(|b| b.kind == p.kind && b.pos.distance_squared(p.pos) < 1.0);
            let timed_out = now - p.time > PENDING_TIMEOUT;
            if timed_out && !placed {
                expired.push((p.kind, p.pos, now + BAD_SPOT_TIME));
            }
            !placed && !timed_out
        });
        self.bad_spots.extend(expired);
        self.bad_spots.retain(|s| s.2 > now);
        self.no_spot.retain(|s| s.1 > now);
        self.worker_orders.retain(|e, _| snap.is_worker(*e));
        self.bad_targets.retain(|t| t.1 > now);
        let foundation = |site: Entity| snap.building(site).is_some_and(|b| !b.complete);
        self.builder_sent.retain(|(site, t)| now - t < 3.0 * RESEND_DELAY && foundation(*site));
        self.site_watch.retain(|w| foundation(w.site));
        self.research_sent.retain(|(_, t)| now - t < RESEND_DELAY);
        self.upgrade_sent.retain(|(_, t)| now - t < RESEND_DELAY);
        self.rally_sent.retain(|(b, t)| now - t < RALLY_RESEND && snap.building(*b).is_some());
        self.army_moved.retain(|(u, t)| now - t < 3.0 * RESEND_DELAY && snap.is_army(*u));
        self.attackers.retain(|u| snap.is_army(*u));
        self.raze_sent.retain(|(u, _)| snap.is_army(*u));
        self.unreachable.retain(|u| u.1 > now);
        self.lost_prey.retain(|u| u.1 > now);
        self.searched.retain(|u| u.1 > now);
    }

    /// Update the smoothed income from the player's running `gathered`
    /// total (which includes trickle income). The first call only samples.
    pub fn observe_income(&mut self, now: f32, gathered: &ResAmounts) {
        if let Some((at, before)) = self.income_sample {
            let dt = now - at;
            if dt <= 0.0 {
                return;
            }
            let k = (dt / INCOME_WINDOW).min(1.0);
            for ((rate, now_total), then) in self.income.0.iter_mut().zip(gathered.0.iter()).zip(before.0.iter()) {
                let sample = (now_total - then).max(0.0) / dt;
                *rate += (sample - *rate) * k;
            }
        }
        self.income_sample = Some((now, *gathered));
    }

    /// Fold what this think's army training (`army`) and building
    /// placements (`buildings`) spent into the smoothed spending rates (the
    /// first call only starts the clock).
    pub fn observe_spending(&mut self, now: f32, army: &ResAmounts, buildings: &ResAmounts) {
        if let Some(at) = self.spend_at {
            let dt = now - at;
            if dt <= 0.0 {
                return;
            }
            let k = (dt / SPEND_WINDOW).min(1.0);
            for (rates, spent) in [(&mut self.army_spend, army), (&mut self.build_spend, buildings)] {
                for (rate, s) in rates.0.iter_mut().zip(spent.0.iter()) {
                    *rate += (s / dt - *rate) * k;
                }
            }
        }
        self.spend_at = Some(now);
    }

    /// Fold this think's wanted gatherers per resource into the smoothed
    /// split and return the smoothed split (the first call takes it as is).
    pub fn smooth_split(&mut self, now: f32, wanted: &[u32; MAX_RESOURCES]) -> [f32; MAX_RESOURCES] {
        let fresh = wanted.map(|n| n as f32);
        let split = match self.split {
            Some((old, at)) if now > at => {
                let k = ((now - at) / SPLIT_WINDOW).min(1.0);
                std::array::from_fn(|i| old[i] + (fresh[i] - old[i]) * k)
            }
            Some((old, _)) => old,
            None => fresh,
        };
        self.split = Some((split, now));
        split
    }

    /// Track the player's age; returns how long it has been in it.
    pub fn note_age(&mut self, age: u8, now: f32) -> f32 {
        match self.age_seen {
            Some((seen, since)) if seen == age => now - since,
            _ => {
                self.age_seen = Some((age, now));
                0.0
            }
        }
    }

    /// Seconds in the current age (0 before [`AiMemory::note_age`] ran).
    pub fn age_duration(&self, now: f32) -> f32 {
        self.age_seen.map_or(0.0, |(_, since)| now - since)
    }

    /// Did a wave raze within [`RAZE_MEMORY`] seconds of `now`? The army then
    /// keeps training building breakers (see `army::army_mix`).
    pub fn razing_lately(&self, now: f32) -> bool {
        self.attack.razing() || self.last_razing.is_some_and(|t| now - t < RAZE_MEMORY)
    }

    /// Is `player` the one a wave razed lately (see [`AiMemory::razing_lately`])?
    pub fn finishing(&self, player: PlayerId, now: f32) -> bool {
        self.raze_victim == Some(player) && self.razing_lately(now)
    }

    /// Remember that `player` attacked us at `now`.
    pub fn note_grudge(&mut self, player: PlayerId, now: f32) {
        match self.grudges.iter_mut().find(|(p, _)| *p == player) {
            Some(g) => g.1 = now,
            None => self.grudges.push((player, now)),
        }
    }

    /// Did `player` attack us within [`GRUDGE_TIME`]?
    pub fn holds_grudge(&self, player: PlayerId, now: f32) -> bool {
        self.grudges.iter().any(|(p, t)| *p == player && now - t < GRUDGE_TIME)
    }

    /// Record that an attack was aimed at `player`.
    pub fn note_target(&mut self, player: PlayerId) {
        self.target_history.push(player);
        if self.target_history.len() > TARGET_HISTORY {
            self.target_history.remove(0);
        }
    }

    pub fn is_bad_target(&self, e: Entity) -> bool {
        self.bad_targets.iter().any(|t| t.0 == e)
    }

    /// Was an attack on a goal at `pos` given up as unreachable lately?
    pub fn is_unreachable(&self, pos: Vec2) -> bool {
        self.unreachable.iter().any(|u| u.0.distance_squared(pos) < 1.0)
    }

    /// Was a spot near `pos` searched lately (within half a search step)?
    pub fn is_searched(&self, pos: Vec2) -> bool {
        let r = SEARCH_STEP * 0.5;
        self.searched.iter().any(|s| s.0.distance_squared(pos) < r * r)
    }

    /// Remember that the spot at `pos` was searched at `now`.
    pub fn note_searched(&mut self, pos: Vec2, now: f32) {
        self.searched.push((pos, now + SEARCH_MEMORY));
    }

    /// Was this worker given a job less than `secs` ago?
    pub fn ordered_within(&self, worker: Entity, now: f32, secs: f32) -> bool {
        self.worker_orders.get(&worker).is_some_and(|o| now - o.time < secs)
    }

    pub fn moved_within(&self, unit: Entity, now: f32, secs: f32) -> bool {
        self.army_moved.iter().any(|(u, t)| *u == unit && now - t < secs)
    }

    pub fn note_moved(&mut self, unit: Entity, now: f32) {
        match self.army_moved.iter_mut().find(|(u, _)| *u == unit) {
            Some(m) => m.1 = now,
            None => self.army_moved.push((unit, now)),
        }
    }

    pub fn research_recent(&self, t: TechId) -> bool {
        self.research_sent.iter().any(|(x, _)| *x == t)
    }

    pub fn is_attacker(&self, e: Entity) -> bool {
        self.attackers.binary_search(&e).is_ok()
    }

    pub fn add_attacker(&mut self, e: Entity) {
        if let Err(i) = self.attackers.binary_search(&e) {
            self.attackers.insert(i, e);
        }
    }

    /// Footprints of pending placements (for `check_placement`'s `extra`).
    pub fn pending_obbs(&self) -> impl Iterator<Item = Obb> + '_ {
        self.pending.iter().map(|p| p.obb)
    }

    pub fn pending_count(&self, kind: BuildingTypeId) -> u32 {
        self.pending.iter().filter(|p| p.kind == kind).count() as u32
    }

    /// Record a sighting (or refresh it after an upgrade changed the kind).
    pub fn see(&mut self, b: KnownBuilding) {
        match self.known.binary_search_by_key(&b.entity, |k| k.entity) {
            Ok(i) => self.known[i] = b,
            Err(i) => self.known.insert(i, b),
        }
    }

    pub fn knows(&self, e: Entity) -> bool {
        self.known.binary_search_by_key(&e, |k| k.entity).is_ok()
    }

    /// Where the remnant unit `e` (see [`AiMemory::remnants`]) is, if it is one.
    pub fn remnant(&self, e: Entity) -> Option<Vec2> {
        self.remnants.binary_search_by_key(&e, |r| r.1).ok().map(|i| self.remnants[i].2)
    }

    /// The hostile building `e`, as last seen.
    pub fn known_building(&self, e: Entity) -> Option<&KnownBuilding> {
        self.known.binary_search_by_key(&e, |k| k.entity).ok().map(|i| &self.known[i])
    }

    /// Note that the breaker `unit` was sent to tear down `building`.
    pub fn note_raze_sent(&mut self, unit: Entity, building: Entity) {
        match self.raze_sent.binary_search_by_key(&unit, |s| s.0) {
            Ok(i) => self.raze_sent[i].1 = building,
            Err(i) => self.raze_sent.insert(i, (unit, building)),
        }
    }

    /// The building the breaker `unit` was last sent to tear down.
    pub fn raze_target(&self, unit: Entity) -> Option<Entity> {
        self.raze_sent.binary_search_by_key(&unit, |s| s.0).ok().map(|i| self.raze_sent[i].1)
    }
}
