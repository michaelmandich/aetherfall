//! Gather and ReturnCargo orders: walk to a node or field, harvest into the
//! `Gatherer`, carry full loads to the nearest drop site, and move on to a
//! similar source when one runs out.
//!
//! A node holds only as many workers as fit round it ([`node_room`]): a
//! worker starting on a full one takes the nearest node of the same kind
//! with room instead (if there is one close by), so a crowd sent to a single
//! tree spreads over the grove rather than jamming the paths to it.
//!
//! Automatic choices (the next node, a free field, the drop site) only take
//! what the worker can get to over the terrain (see `reach`): a tree across
//! a river no ford crosses is never picked. Over a forded river a tree
//! counts as far as the way round by the ford, so one on the worker's own
//! bank wins unless it is that much farther; the drop site is the nearest
//! by the way round among the nearest few. A free field is the nearest in
//! a straight line. A source or drop site the terrain keeps out of reach is
//! remembered so until the regions change.

use af_core::ext::movement::NavRegions;
use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::brain::{Brain, Phase, Unreachable};
use crate::motion;
use crate::notice::notify;
use crate::reach::Reach;
use crate::tuning::*;

/// Drop sites compared by the way round (the nearest in a straight line first).
const DEPOT_CHOICES: usize = 3;

/// Nodes weighed by the way round at most when picking the next one (see
/// `Harvest::nearest_workable`).
const NODE_CHOICES: usize = 3;

/// Below this a node, field or load counts as empty / full.
const EPSILON: f32 = 1e-4;

#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Worker {
    entity: Entity,
    unit: &'static Unit,
    owner: &'static Owner,
    pos: &'static Position,
    facing: &'static Facing,
    footprint: &'static Footprint,
    mobile: &'static Mobile,
    path: &'static PathState,
    orders: &'static mut Orders,
    brain: &'static mut Brain,
    gatherer: &'static mut Gatherer,
    goal: Option<&'static mut MoveGoal>,
}

impl WorkerItem<'_, '_> {
    fn obb(&self) -> Obb {
        self.footprint.obb(self.pos.0, self.facing.0)
    }

    /// How near a node's box a worker must get to work it (as af_movement
    /// walks it up to one).
    fn node_reach(&self) -> f32 {
        WORK_ARRIVE + self.mobile.radius + NODE_REACH_SLACK
    }
}

/// A resource node or a resource-field building.
#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Source {
    pos: &'static Position,
    facing: &'static Facing,
    footprint: &'static Footprint,
    node: Option<&'static mut ResourceNode>,
    field: Option<&'static mut ResourceField>,
    building: Option<&'static Building>,
    owner: Option<&'static Owner>,
    foundation: Has<UnderConstruction>,
}

impl SourceReadOnlyItem<'_, '_> {
    fn obb(&self) -> Obb {
        self.footprint.obb(self.pos.0, self.facing.0)
    }

    fn resource(&self) -> Option<ResourceId> {
        self.node.map(|n| n.resource).or(self.field.map(|f| f.resource))
    }

    fn remaining(&self) -> f32 {
        self.node.map(|n| n.remaining).or(self.field.map(|f| f.remaining)).unwrap_or(0.0)
    }
}

type Sources<'w, 's> = Query<'w, 's, Source, (Or<(With<ResourceNode>, With<ResourceField>)>, Without<Dead>)>;

#[derive(QueryData)]
pub(crate) struct Depot {
    entity: Entity,
    drop_site: &'static DropSite,
    owner: &'static Owner,
    pos: &'static Position,
    facing: &'static Facing,
    footprint: &'static Footprint,
}

/// Everything gathering touches besides the workers themselves.
#[derive(SystemParam)]
pub(crate) struct Harvest<'w, 's> {
    data: Res<'w, Data>,
    players: ResMut<'w, Players>,
    spatial: Res<'w, SpatialIndex>,
    nav: Res<'w, Nav>,
    regions: Option<Res<'w, NavRegions>>,
    clock: Res<'w, SimClock>,
    notices: ResMut<'w, Notices>,
    died: MessageWriter<'w, EntityDied>,
    commands: Commands<'w, 's>,
    sources: Sources<'w, 's>,
    depots: Query<'w, 's, Depot, (Without<UnderConstruction>, Without<Dead>)>,
    /// Workers on each node: started Gather orders, recounted every tick.
    crowds: Local<'s, HashMap<Entity, usize>>,
}

/// How many workers of `worker_width` fit around a node of `half` extents.
pub(crate) fn node_room(half: Vec2, worker_width: f32) -> usize {
    let perimeter = 4.0 * (half.x + half.y);
    ((perimeter / (worker_width + GATHER_SPACING)) as usize).clamp(1, MAX_WORKERS_PER_NODE)
}

/// Workers go in entity index order, not table order (which client inserts on sim
/// entities change): they share nodes, field slots and their owner's
/// stock, and the first come takes the room, the last of a node and the
/// first word of a notice.
pub(crate) fn run_gather_orders(
    mut workers: Query<Worker, (With<Unit>, Without<Dead>)>,
    mut harvest: Harvest,
    mut order: Local<Vec<Entity>>,
) {
    harvest.recount_field_users(&workers);
    harvest.count_crowds(&workers);
    // Only synced brains gathering act (no worker changes another's brain).
    let gathering = |u: &WorkerReadOnlyItem| {
        u.brain.synced(u.orders) && matches!(u.orders.current(), Some(Order::Gather { .. } | Order::ReturnCargo))
    };
    af_core::by_entity_index(&mut order, workers.iter().filter(gathering).map(|u| u.entity));
    for &e in order.iter() {
        let Ok(mut u) = workers.get_mut(e) else { continue };
        match u.orders.current().copied() {
            Some(Order::Gather { target }) => harvest.gather(&mut u, target),
            Some(Order::ReturnCargo) => harvest.return_cargo(&mut u),
            _ => {}
        }
    }
}

impl Harvest<'_, '_> {
    /// Field slots are recounted from the workers holding them, so a worker
    /// that died or changed orders frees its slot without bookkeeping.
    fn recount_field_users(&mut self, workers: &Query<Worker, (With<Unit>, Without<Dead>)>) {
        for s in &mut self.sources {
            if let Some(mut f) = s.field.filter(|f| f.users != 0) {
                f.users = 0;
            }
        }
        for u in workers {
            if let Some(Ok(s)) = u.brain.field.map(|f| self.sources.get_mut(f))
                && let Some(mut f) = s.field
            {
                f.users += 1;
            }
        }
    }

    /// Workers on each node: those whose started order gathers it.
    fn count_crowds(&mut self, workers: &Query<Worker, (With<Unit>, Without<Dead>)>) {
        self.crowds.clear();
        for u in workers {
            if let (Some(Order::Gather { target }), false) = (u.orders.current(), u.brain.phase == Phase::Fresh)
                && u.brain.synced(u.orders)
                && self.sources.get(*target).is_ok_and(|s| s.node.is_some())
            {
                *self.crowds.entry(*target).or_default() += 1;
            }
        }
    }

    fn gather(&mut self, u: &mut WorkerItem, target: Entity) {
        if u.brain.phase == Phase::Fresh {
            self.start(u, target);
        }
        match u.brain.phase {
            Phase::Fresh => {}
            Phase::Approach => self.walk_to_source(u, target),
            Phase::Work => self.harvest(u, target),
            Phase::Deliver => self.deliver(u),
        }
    }

    fn return_cargo(&mut self, u: &mut WorkerItem) {
        if u.brain.phase == Phase::Fresh {
            u.brain.phase = Phase::Deliver;
        }
        self.deliver(u);
    }

    /// Set up a fresh Gather order: check the source, claim a field slot,
    /// and go (or first unload cargo of another kind).
    fn start(&mut self, u: &mut WorkerItem, target: Entity) {
        let Ok(src) = self.sources.get(target) else {
            // Gone before the order started (felled while it waited in the
            // queue). A fresh brain knows nothing of it, unless it was this
            // worker's last job: then what it yielded is known, and where it
            // stood is `gatherer.last_pos`, so a similar source can be found.
            if u.brain.resource.is_none() {
                u.brain.resource = u.brain.last_job.filter(|&(job, _)| job == target).map(|(_, r)| r);
            }
            return self.move_on(u);
        };
        let Some(resource) = src.resource() else { return self.give_up(u) };
        let rate = self.players.try_get(u.owner.0).map_or(0.0, |p| stats::gather_rate(&self.data, p, u.unit.kind, resource));
        if rate <= 0.0 || u.gatherer.capacity <= 0.0 {
            return self.give_up(u);
        }
        let (pos, node_kind, half) = (src.pos.0, src.node.map(|n| n.kind), src.footprint.half);
        if let Some(kind) = node_kind {
            if self.data.node(kind).requires_building_id.is_some() {
                return self.give_up(u);
            }
            let room = node_room(half, u.footprint.half.x * 2.0);
            let full = self.crowds.get(&target).copied().unwrap_or(0) >= room;
            let reach = self.reach(u);
            let in_reach = reach.within(&src.obb(), u.node_reach());
            if !in_reach {
                u.brain.unreachable.remember_for_good(target, reach.stamp());
            }
            if (full || !in_reach || !self.workable(u, &src.obb()))
                && let Some(other) = self.roomier_node(u, &reach, target, kind, pos)
            {
                u.brain.replace_front(&mut u.orders, Order::Gather { target: other });
                return self.start(u, other);
            }
            if !in_reach {
                // Across the water, and no node like it at hand on this side:
                // something similar round it that the worker can get to.
                u.brain.resource = Some(resource);
                u.gatherer.last_pos = pos;
                u.gatherer.last_node_kind = Some(kind);
                return self.move_on(u);
            }
            *self.crowds.entry(target).or_default() += 1;
        } else {
            if src.owner.map(|o| o.0) != Some(u.owner.0) {
                return self.give_up(u);
            }
            if src.foundation {
                // Build the field first; the Gather order stays queued behind it.
                let builds = self.data.unit(u.unit.kind).gatherer.as_ref().is_some_and(|g| g.builds);
                if builds {
                    u.orders.queue.push_front(Order::Build { site: target });
                    return;
                }
                return self.give_up(u);
            }
            if !self.claim_slot(target) {
                let reach = self.reach(u);
                return match self.free_field(&reach, u.owner.0, resource, pos, &u.brain.unreachable) {
                    Some(other) => u.brain.replace_front(&mut u.orders, Order::Gather { target: other }),
                    None => self.give_up(u),
                };
            }
            u.brain.field = Some(target);
        }
        // Recorded before the emptiness check: a source emptied earlier this
        // tick (by another worker; it is removed at the end of the tick) is
        // replaced by a similar one round it, which `move_on` looks up from
        // these.
        u.brain.resource = Some(resource);
        u.brain.last_job = Some((target, resource));
        u.gatherer.last_pos = pos;
        if node_kind.is_some() {
            u.gatherer.last_node_kind = node_kind;
        }
        if src_exhausted(&self.sources, target) {
            return self.move_on(u);
        }
        u.gatherer.target = Some(target);
        let other_cargo = u.gatherer.amount > EPSILON && u.gatherer.carrying.is_some_and(|c| c != resource);
        u.brain.phase = if other_cargo { Phase::Deliver } else { Phase::Approach };
    }

    fn claim_slot(&mut self, field: Entity) -> bool {
        let Ok(s) = self.sources.get_mut(field) else { return false };
        match s.field {
            Some(mut f) if f.users < f.slots => {
                f.users += 1;
                true
            }
            _ => false,
        }
    }

    fn walk_to_source(&mut self, u: &mut WorkerItem, target: Entity) {
        let Some((obb, pos)) = self.sources.get(target).ok().filter(|s| s.remaining() > EPSILON).map(|s| (s.obb(), s.pos.0)) else {
            return self.move_on(u);
        };
        let gap = u.obb().gap(&obb);
        if gap <= WORK_REACH {
            motion::halt(&mut self.commands, u.entity, &mut u.goal);
            u.brain.failures = 0;
            u.brain.phase = Phase::Work;
            return;
        }
        // Out of reach yet af_movement is done: blocked or a partial path.
        if motion::done_with(&u.goal, u.path, target) {
            u.brain.failures += 1;
            if u.brain.failures > MAX_PATH_FAILURES {
                let within = u.node_reach();
                self.remember_unreachable(u, target, &obb, within);
                return self.move_on(u);
            }
        }
        motion::approach(&mut self.commands, u.entity, &mut u.goal, u.path, target, pos, WORK_ARRIVE, gap);
    }

    /// The worker's reach over the terrain, its unreachable memory kept to
    /// the regions it reads.
    fn reach(&self, u: &mut WorkerItem) -> Reach {
        let reach = Reach::of(self.regions.as_deref(), u.pos.0, u.mobile);
        u.brain.unreachable.sync(reach.stamp());
        reach
    }

    /// af_movement could not get the worker to `target` (box `obb`, worked
    /// within `within`): skipped for a while, or until the regions change
    /// when the terrain keeps it out of reach.
    fn remember_unreachable(&self, u: &mut WorkerItem, target: Entity, obb: &Obb, within: f32) {
        let reach = self.reach(u);
        if reach.within(obb, within) {
            u.brain.unreachable.remember(target, self.clock.tick);
        } else {
            u.brain.unreachable.remember_for_good(target, reach.stamp());
        }
    }

    /// Take from the source into the worker's load at the worker's rate.
    fn harvest(&mut self, u: &mut WorkerItem, target: Entity) {
        let Some(player) = self.players.try_get(u.owner.0) else { return };
        let Ok(src) = self.sources.get(target) else { return self.after_source(u) };
        if src.remaining() <= EPSILON {
            return self.after_source(u);
        }
        if u.obb().gap(&src.obb()) > WORK_REACH * 2.0 {
            // Pushed away (crowding, a new building): walk back.
            u.brain.phase = Phase::Approach;
            return;
        }
        let Some(resource) = src.resource() else { return self.give_up(u) };
        let mult = src.node.map_or(1.0, |n| self.data.node(n.kind).gather_mult);
        let rate = stats::gather_rate(&self.data, player, u.unit.kind, resource) * mult;
        let g = &mut *u.gatherer;
        if g.carrying != Some(resource) {
            g.carrying = Some(resource);
            g.amount = 0.0;
        }
        let take = (rate * SIM_DT).min(g.capacity - g.amount).min(src.remaining()).max(0.0);
        g.amount += take;
        let full = g.amount >= g.capacity - EPSILON;
        let left = self.take_from(target, take);
        if left <= EPSILON {
            self.exhaust(target);
        }
        if full || left <= EPSILON {
            u.brain.phase = Phase::Deliver;
        }
    }

    /// Remove `amount` from a source; returns what is left.
    fn take_from(&mut self, target: Entity, amount: f32) -> f32 {
        let Ok(s) = self.sources.get_mut(target) else { return 0.0 };
        if let Some(mut n) = s.node {
            n.remaining = (n.remaining - amount).max(0.0);
            n.remaining
        } else if let Some(mut f) = s.field {
            f.remaining = (f.remaining - amount).max(0.0);
            f.remaining
        } else {
            0.0
        }
    }

    /// A source ran out: a node disappears; a field building is destroyed
    /// when being a field is all it does (a farm), else it only stops being
    /// one and goes on as a drop site, barracks or tower (a harbour).
    fn exhaust(&mut self, target: Entity) {
        let Ok(s) = self.sources.get(target) else { return };
        let pos = s.pos.0;
        let (kind, owner) = match (s.node, s.building) {
            (Some(n), _) => (EntityKind::Node(n.kind), None),
            (None, Some(b)) => (EntityKind::Building(b.kind), s.owner.map(|o| o.0)),
            (None, None) => return,
        };
        let faction = owner.and_then(|p| self.players.try_get(p)).map(|p| p.faction);
        if let (EntityKind::Building(b), Some(player)) = (kind, owner) {
            let name = &self.data.building(b).name;
            let keeps_standing = has_other_roles(&self.data, b, faction);
            let text = if keeps_standing {
                let resource = s.field.map_or("resource", |f| self.data.resource(f.resource).name.as_str());
                format!("{name}: {resource} exhausted")
            } else {
                format!("{name} exhausted")
            };
            notify(&mut self.notices, player, NoticeKind::Info, &text, Some(pos), self.clock.elapsed);
            if keeps_standing {
                self.commands.entity(target).try_remove::<ResourceField>();
                return;
            }
        }
        self.commands.entity(target).try_insert(Dead);
        self.died.write(EntityDied { entity: target, owner, kind, pos, killer: None });
    }

    /// The source is gone or empty: unload what we carry, else move on.
    fn after_source(&mut self, u: &mut WorkerItem) {
        if u.gatherer.amount > EPSILON {
            u.brain.phase = Phase::Deliver;
        } else {
            self.move_on(u);
        }
    }

    /// Carry the load to the nearest own drop site that takes it and was
    /// not found unreachable lately.
    fn deliver(&mut self, u: &mut WorkerItem) {
        let Some(resource) = u.gatherer.carrying.filter(|_| u.gatherer.amount > EPSILON) else {
            return self.after_delivery(u);
        };
        let site = match u.brain.drop_site.filter(|&s| self.depot_ok(s, u.owner.0, resource)) {
            Some(s) => Some(s),
            None => {
                let reach = self.reach(u);
                self.nearest_depot(&reach, u.pos.0, u.owner.0, resource, &u.brain.unreachable)
            }
        };
        let Some((site, obb, pos)) = site.and_then(|s| self.depots.get(s).ok().map(|d| (s, d.footprint.obb(d.pos.0, d.facing.0), d.pos.0))) else {
            let text = format!("No drop site for {}", self.data.resource(resource).name);
            notify(&mut self.notices, u.owner.0, NoticeKind::Warning, &text, Some(u.pos.0), self.clock.elapsed);
            motion::halt(&mut self.commands, u.entity, &mut u.goal);
            u.orders.queue.pop_front();
            return;
        };
        u.brain.drop_site = Some(site);
        let gap = u.obb().gap(&obb);
        if gap <= WORK_REACH {
            self.deposit(u, resource);
            return self.after_delivery(u);
        }
        // Out of reach yet af_movement is done: blocked or a partial path.
        if motion::done_with(&u.goal, u.path, site) {
            u.brain.failures += 1;
            if u.brain.failures > MAX_PATH_FAILURES {
                // Try the next drop site; give up once none is left.
                self.remember_unreachable(u, site, &obb, WORK_REACH);
                u.brain.drop_site = None;
                u.brain.failures = 0;
                return;
            }
        }
        motion::approach(&mut self.commands, u.entity, &mut u.goal, u.path, site, pos, WORK_ARRIVE, gap);
    }

    fn depot_ok(&self, site: Entity, owner: PlayerId, resource: ResourceId) -> bool {
        self.depots.get(site).is_ok_and(|d| d.owner.0 == owner && d.drop_site.accepts(resource))
    }

    /// The own drop site taking `resource` that the worker at `from` gets
    /// to soonest: of the [`DEPOT_CHOICES`] nearest in a straight line that
    /// it can reach over the terrain, the nearest by the way round (a
    /// straight line, unless a river lies between).
    fn nearest_depot(&self, reach: &Reach, from: Vec2, owner: PlayerId, resource: ResourceId, avoid: &Unreachable) -> Option<Entity> {
        let tick = self.clock.tick;
        let mut near: Vec<(f32, Entity, Vec2)> = self
            .depots
            .iter()
            .filter(|d| d.owner.0 == owner && d.drop_site.accepts(resource) && !avoid.skips(d.entity, tick))
            .filter(|d| reach.within(&d.footprint.obb(d.pos.0, d.facing.0), WORK_REACH))
            .map(|d| (d.pos.0.distance(from), d.entity, d.pos.0))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        near.truncate(DEPOT_CHOICES);
        if near.len() > 1 {
            for n in &mut near {
                n.0 = reach.route_distance(n.2).unwrap_or(f32::MAX);
            }
            near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        }
        near.first().map(|n| n.1)
    }

    fn deposit(&mut self, u: &mut WorkerItem, resource: ResourceId) {
        let amount = u.gatherer.amount;
        if let Some(p) = self.players.list.get_mut(u.owner.0.index()) {
            p.resources.add(resource, amount);
            p.stats.gathered.add(resource, amount);
        }
        u.gatherer.amount = 0.0;
        u.gatherer.carrying = None;
    }

    /// Back to work after unloading: the same source if it still yields,
    /// else a similar one nearby.
    fn after_delivery(&mut self, u: &mut WorkerItem) {
        u.brain.drop_site = None;
        u.brain.failures = 0;
        match u.orders.current().copied() {
            Some(Order::Gather { target }) => {
                if src_exhausted(&self.sources, target) {
                    self.move_on(u);
                } else {
                    u.brain.phase = Phase::Approach;
                }
            }
            _ => match u.gatherer.target.filter(|&t| !src_exhausted(&self.sources, t)) {
                Some(t) => {
                    u.brain.replace_front(&mut u.orders, Order::Gather { target: t });
                    u.brain.phase = Phase::Fresh;
                }
                None => {
                    u.orders.queue.pop_front();
                }
            },
        }
    }

    /// The source is gone (or unreachable): switch to the nearest node of
    /// the same kind, then anything yielding the same resource, or unload
    /// and stop. Sources found unreachable lately are skipped, so a worker
    /// does not bounce between two trees it cannot get to.
    fn move_on(&mut self, u: &mut WorkerItem) {
        let reach = self.reach(u);
        let next = self.replacement(u, &reach);
        if let Some(field) = u.brain.field.take()
            && let Ok(s) = self.sources.get_mut(field)
            && let Some(mut f) = s.field.filter(|f| f.users > 0)
        {
            f.users -= 1;
        }
        match next {
            Some(n) => {
                u.brain.replace_front(&mut u.orders, Order::Gather { target: n });
                u.brain.phase = Phase::Fresh;
                u.brain.failures = 0;
            }
            None if u.gatherer.amount > EPSILON && u.brain.phase != Phase::Deliver => {
                // The failures on the source are not the drop site's.
                u.brain.failures = 0;
                u.brain.phase = Phase::Deliver;
            }
            None => self.give_up(u),
        }
    }

    fn replacement(&self, u: &WorkerItem, reach: &Reach) -> Option<Entity> {
        let center = u.gatherer.last_pos;
        let avoid = &u.brain.unreachable;
        let tick = self.clock.tick;
        let usable = |e: &SpatialEntry| !avoid.skips(e.entity, tick) && !src_exhausted(&self.sources, e.entity);
        if let Some(kind) = u.gatherer.last_node_kind.filter(|k| Some(self.data.node(*k).resource_id) == u.brain.resource) {
            let same_kind = self.nearest_workable(u, reach, center, SAME_KIND_RADIUS, |e| {
                e.kind == SpatialKind::Node && e.type_index == kind.0 && e.pos.distance(center) <= SAME_KIND_RADIUS && usable(e)
            });
            if same_kind.is_some() {
                return same_kind;
            }
        }
        let resource = u.brain.resource?;
        let node = self.nearest_workable(u, reach, center, SAME_RESOURCE_RADIUS, |e| {
            e.kind == SpatialKind::Node
                && e.pos.distance(center) <= SAME_RESOURCE_RADIUS
                && self.data.node(NodeTypeId(e.type_index)).resource_id == resource
                && self.data.node(NodeTypeId(e.type_index)).requires_building_id.is_none()
                && usable(e)
        });
        node.or_else(|| self.free_field(reach, u.owner.0, resource, center, avoid))
    }

    /// Nearest node of `kind` to the full node `full` at `at` that still has
    /// room for this worker, not found unreachable lately.
    fn roomier_node(&self, u: &WorkerItem, reach: &Reach, full: Entity, kind: NodeTypeId, at: Vec2) -> Option<Entity> {
        let tick = self.clock.tick;
        let width = u.footprint.half.x * 2.0;
        self.nearest_workable(u, reach, at, GATHER_SPREAD_RADIUS, |e| {
            e.kind == SpatialKind::Node
                && e.type_index == kind.0
                && e.entity != full
                && e.pos.distance(at) <= GATHER_SPREAD_RADIUS
                && !u.brain.unreachable.skips(e.entity, tick)
                && !src_exhausted(&self.sources, e.entity)
                && self.crowds.get(&e.entity).copied().unwrap_or(0) < node_room(e.obb.half, width)
        })
    }

    /// Nearest entry (by box edge distance from `at`, within `radius`)
    /// passing `wanted` that the worker can get to over the terrain and
    /// that is [`workable`](Self::workable), counting how much longer than
    /// the straight line the worker's way to it is (a tree over a forded
    /// river is as far as the way round by the ford). The costly tests run
    /// only on the entries passing `wanted`, nearest first, until none left
    /// can beat the best, and the way round is weighed for [`NODE_CHOICES`]
    /// of them at most.
    fn nearest_workable(&self, u: &WorkerItem, reach: &Reach, at: Vec2, radius: f32, mut wanted: impl FnMut(&SpatialEntry) -> bool) -> Option<Entity> {
        let mut found: Vec<(f32, SpatialEntry)> = Vec::new();
        let within = u.node_reach();
        self.spatial.for_each_near(at, radius, |e| {
            let d = e.obb.distance_to_point(at);
            if d <= radius && wanted(e) && reach.within(&e.obb, within) {
                found.push((d, *e));
            }
        });
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.entity.cmp(&b.1.entity)));
        let mut best: Option<(f32, Entity)> = None;
        let mut weighed = 0;
        for (d, e) in found {
            // Nothing farther in a straight line can be nearer by the way round.
            if weighed == NODE_CHOICES || best.is_some_and(|(b, _)| d >= b) {
                break;
            }
            if !self.workable(u, &e.obb) {
                continue;
            }
            weighed += 1;
            let cost = d + reach.way_round(e.pos);
            if best.is_none_or(|(b, _)| cost < b) {
                best = Some((cost, e.entity));
            }
        }
        best.map(|(_, e)| e)
    }

    /// Is there open ground within a worker's reach of the node box `obb`?
    /// A tree in the middle of a forest is not, until the trees round it fall:
    /// a worker picking its next tree, or sent to a full or enclosed one,
    /// takes one it can walk up to.
    fn workable(&self, u: &WorkerItem, obb: &Obb) -> bool {
        let team = self.players.team(u.owner.0);
        let (mobility, radius) = (u.mobile.mobility.0, u.mobile.radius);
        let reach = WORK_ARRIVE + radius + NODE_REACH_SLACK;
        self.nav
            .nearest_open(obb.center, mobility, radius, team, obb.bounding_radius() + reach)
            .is_some_and(|p| obb.distance_to_point(p) <= reach)
    }

    /// Nearest own complete field of `resource` with a free slot that was
    /// not found unreachable lately and that the worker can get to.
    fn free_field(&self, reach: &Reach, owner: PlayerId, resource: ResourceId, center: Vec2, avoid: &Unreachable) -> Option<Entity> {
        let tick = self.clock.tick;
        self.spatial
            .nearest(center, SAME_RESOURCE_RADIUS, |e| {
                e.kind == SpatialKind::Building
                    && e.owner == Some(owner)
                    && !e.under_construction
                    && !avoid.skips(e.entity, tick)
                    && reach.within(&e.obb, WORK_REACH)
                    && self.sources.get(e.entity).is_ok_and(|s| {
                        s.field.is_some_and(|f| f.resource == resource && f.users < f.slots && f.remaining > EPSILON) && !s.foundation
                    })
            })
            .map(|e| e.entity)
    }

    /// Stop gathering: drop this order (the next one, if any, takes over).
    fn give_up(&mut self, u: &mut WorkerItem) {
        motion::halt(&mut self.commands, u.entity, &mut u.goal);
        u.orders.queue.pop_front();
    }
}

/// Gone, empty, or still a foundation.
fn src_exhausted(sources: &Sources, target: Entity) -> bool {
    !sources.get(target).is_ok_and(|s| s.remaining() > EPSILON && !s.foundation)
}

/// Does a building of `kind` (owned by `faction`) do anything besides being
/// gathered: take deposits, train, research or upgrade, fight, house people
/// or pay out? Such a building outlives its resource field.
pub(crate) fn has_other_roles(data: &GameData, kind: BuildingTypeId, faction: Option<FactionId>) -> bool {
    let def = data.building(kind);
    let produces = faction.is_some_and(|f| !data.trainable_at(kind, f).is_empty() || !data.researchable_at(kind, f).is_empty());
    !def.drop_site_ids.is_empty() || !def.weapon_ids.is_empty() || def.pop > 0 || !def.trickle_amounts.is_zero() || def.upgrade.is_some() || produces
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A farm is nothing but a field; a harbour is also a drop site,
    /// barracks and academy; a house is no field but still has a role.
    #[test]
    fn only_field_only_buildings_lack_other_roles() {
        let data = af_core::testing::test_data();
        let building = |id: &str| data.building_id(id).unwrap_or_else(|| panic!("no building `{id}`"));
        let faction = |id: &str| data.faction_id(id);
        assert!(!has_other_roles(&data, building("farm"), faction("ironhold")));
        assert!(!has_other_roles(&data, building("dune_quartz_sifter"), faction("dune")));
        assert!(has_other_roles(&data, building("tide_harbor"), faction("tide")));
        // Its drop site alone keeps it standing for any owner.
        assert!(has_other_roles(&data, building("tide_harbor"), None));
        assert!(has_other_roles(&data, building("house"), faction("ironhold")));
    }
}
