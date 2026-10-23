//! Build and Repair orders.

use af_core::ext::movement::NavRegions;
use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::ecs::system::SystemParam;
use bevy::math::ops;
use bevy::prelude::*;

use crate::brain::{Brain, Phase};
use crate::motion;
use crate::reach::Reach;
use crate::tuning::*;

#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Builder {
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
    goal: Option<&'static mut MoveGoal>,
    gatherer: Option<&'static Gatherer>,
}

/// A foundation to build, or a building / machine to repair.
#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Structure {
    health: &'static mut Health,
    construction: Option<&'static mut UnderConstruction>,
    pos: &'static Position,
    facing: &'static Facing,
    footprint: &'static Footprint,
    owner: &'static Owner,
    building: Option<&'static Building>,
    field: Has<ResourceField>,
}

impl StructureReadOnlyItem<'_, '_> {
    fn obb(&self) -> Obb {
        self.footprint.obb(self.pos.0, self.facing.0)
    }
}

#[derive(SystemParam)]
pub(crate) struct Works<'w, 's> {
    data: Res<'w, Data>,
    players: Res<'w, Players>,
    spatial: Res<'w, SpatialIndex>,
    regions: Option<Res<'w, NavRegions>>,
    commands: Commands<'w, 's>,
    structures: Query<'w, 's, Structure, Without<Dead>>,
    nodes: Query<'w, 's, &'static ResourceNode, Without<Dead>>,
}

/// Builders walk up to their foundation and add progress together (with
/// mild diminishing returns); af_economy completes the building. Repairers
/// mend at their repair rate until full. They go in entity index order, not table
/// order (which client inserts on sim entities change): repairers of one
/// building add to its hit points one after another.
pub(crate) fn run_build_orders(
    mut builders: Query<Builder, (With<Unit>, Without<Dead>)>,
    mut works: Works,
    mut working: Local<Vec<Entity>>,
    mut order: Local<Vec<Entity>>,
) {
    working.clear();
    // Only synced brains building or repairing act (no unit changes another's brain).
    let busy = |u: &BuilderReadOnlyItem| {
        u.brain.synced(u.orders) && matches!(u.orders.current(), Some(Order::Build { .. } | Order::Repair { .. }))
    };
    af_core::by_entity_index(&mut order, builders.iter().filter(busy).map(|u| u.entity));
    for &e in order.iter() {
        let Ok(mut u) = builders.get_mut(e) else { continue };
        match u.orders.current().copied() {
            Some(Order::Build { site }) => {
                if works.build(&mut u, site) {
                    working.push(site);
                }
            }
            Some(Order::Repair { target }) => works.repair(&mut u, target),
            _ => {}
        }
    }
    works.add_progress(&mut working);
}

impl Works<'_, '_> {
    /// Returns true when the builder works on the site this tick.
    fn build(&mut self, u: &mut BuilderItem, site: Entity) -> bool {
        let builds = self.data.unit(u.unit.kind).gatherer.as_ref().is_some_and(|g| g.builds);
        let Some((obb, pos, owner, done)) = self.structures.get(site).ok().map(|s| (s.obb(), s.pos.0, s.owner.0, s.construction.is_none())) else {
            u.orders.queue.pop_front();
            return false;
        };
        if done {
            self.after_build(u, site);
            return false;
        }
        if !builds || !self.players.allied(u.owner.0, owner) {
            u.orders.queue.pop_front();
            return false;
        }
        self.reach(u, site, obb, pos)
    }

    fn repair(&mut self, u: &mut BuilderItem, target: Entity) {
        let rate = self.data.unit(u.unit.kind).gatherer.as_ref().map_or(0.0, |g| g.repair);
        let Some((obb, pos, owner, foundation, full)) =
            self.structures.get(target).ok().map(|s| (s.obb(), s.pos.0, s.owner.0, s.construction.is_some(), s.health.hp >= s.health.max))
        else {
            u.orders.queue.pop_front();
            return;
        };
        if foundation {
            u.brain.replace_front(&mut u.orders, Order::Build { site: target });
            u.brain.phase = Phase::Fresh;
            return;
        }
        if rate <= 0.0 || full || owner != u.owner.0 {
            motion::halt(&mut self.commands, u.entity, &mut u.goal);
            u.orders.queue.pop_front();
            return;
        }
        if self.reach(u, target, obb, pos)
            && let Ok(mut s) = self.structures.get_mut(target)
        {
            s.health.hp = (s.health.hp + rate * SIM_DT).min(s.health.max);
        }
    }

    /// Walk next to the target; true once there.
    fn reach(&mut self, u: &mut BuilderItem, target: Entity, obb: Obb, pos: Vec2) -> bool {
        let gap = u.footprint.obb(u.pos.0, u.facing.0).gap(&obb);
        let reach = if u.brain.phase == Phase::Work { WORK_REACH * 2.0 } else { WORK_REACH };
        if gap <= reach {
            motion::halt(&mut self.commands, u.entity, &mut u.goal);
            u.brain.phase = Phase::Work;
            u.brain.failures = 0;
            return true;
        }
        u.brain.phase = Phase::Approach;
        // Out of reach yet af_movement is done: blocked or a partial path.
        if motion::done_with(&u.goal, u.path, target) {
            u.brain.failures += 1;
            if u.brain.failures > MAX_PATH_FAILURES {
                motion::halt(&mut self.commands, u.entity, &mut u.goal);
                u.orders.queue.pop_front();
                // A Gather on this field queued it; it would only queue it again.
                if u.orders.current() == Some(&Order::Gather { target }) {
                    u.orders.queue.pop_front();
                }
                return false;
            }
        }
        motion::approach(&mut self.commands, u.entity, &mut u.goal, u.path, target, pos, WORK_ARRIVE, gap);
        false
    }

    /// The site is finished. A builder with nothing else to do starts
    /// working a new field, or a node near a new drop site.
    fn after_build(&mut self, u: &mut BuilderItem, site: Entity) {
        u.orders.queue.pop_front();
        if !u.orders.queue.is_empty() || u.gatherer.is_none() {
            return;
        }
        let Ok(s) = self.structures.get(site) else { return };
        let drop_site = s.building.filter(|b| !self.data.building(b.kind).drop_site_ids.is_empty());
        let next = if s.field { Some(site) } else { drop_site.and_then(|b| self.node_for_drop_site(u, b.kind, s.pos.0)) };
        if let Some(target) = next {
            u.orders.queue.push_back(Order::Gather { target });
        }
    }

    /// Nearest node around a new drop site that it accepts and the worker
    /// can gather (and get to: not across the river from it), preferring
    /// what the worker gathered last.
    fn node_for_drop_site(&self, u: &BuilderItem, kind: BuildingTypeId, at: Vec2) -> Option<Entity> {
        let accepts = &self.data.building(kind).drop_site_ids;
        let player = self.players.try_get(u.owner.0)?;
        let reach = Reach::of(self.regions.as_deref(), u.pos.0, u.mobile);
        let within = WORK_ARRIVE + u.mobile.radius + NODE_REACH_SLACK;
        let usable = |r: ResourceId| accepts.contains(&r) && stats::gather_rate(&self.data, player, u.unit.kind, r) > 0.0;
        let last = u.gatherer.and_then(|g| g.last_node_kind).map(|k| self.data.node(k).resource_id).filter(|r| usable(*r));
        let find = |wanted: &dyn Fn(ResourceId) -> bool| {
            self.spatial
                .nearest(at, NEW_DROP_SITE_RADIUS, |e| {
                    // `type_index` is a node type only for nodes.
                    if e.kind != SpatialKind::Node {
                        return false;
                    }
                    let def = self.data.node(NodeTypeId(e.type_index));
                    def.requires_building_id.is_none()
                        && wanted(def.resource_id)
                        && self.nodes.get(e.entity).is_ok_and(|n| n.remaining > 0.0)
                        && reach.within(&e.obb, within)
                })
                .map(|e| e.entity)
        };
        last.and_then(|r| find(&|x| x == r)).or_else(|| find(&usable))
    }

    /// Advance every worked foundation once: `n` builders build
    /// `n^BUILDER_EXPONENT` times as fast as one, and hit points grow with
    /// progress.
    fn add_progress(&mut self, working: &mut [Entity]) {
        working.sort_unstable();
        for group in working.chunk_by(|a, b| a == b) {
            let Ok(s) = self.structures.get_mut(group[0]) else { continue };
            let StructureItem { mut health, construction: Some(mut c), .. } = s else { continue };
            let speed = ops::powf(group.len() as f32, BUILDER_EXPONENT) / c.build_time.max(0.1);
            let before = c.progress;
            c.progress = (c.progress + speed * SIM_DT).min(1.0);
            let max = health.max;
            health.hp = (health.hp + max * (1.0 - FOUNDATION_HP) * (c.progress - before)).min(max);
        }
    }
}
