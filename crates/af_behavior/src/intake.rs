//! `SimSet::Commands`: this tick's unit commands become orders.
//!
//! Only alive units owned by the commanding player obey. Building commands
//! (placement, training, research, rally) belong to af_economy, and so do
//! buildings listed in a Smart command.

use af_core::ext::combat::{attack_reach, TargetClass};
use af_core::ext::movement::NavRegions;
use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::brain::Brain;
use crate::formation::{self, Member};
use crate::gather::node_room;
use crate::notice::notify;
use crate::reach::Reach;
use crate::tuning::*;

/// A unit that may receive orders.
#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Actor {
    unit: &'static Unit,
    owner: &'static Owner,
    pos: &'static Position,
    footprint: &'static Footprint,
    mobile: &'static Mobile,
    stats: &'static Stats,
    weapons: &'static Weapons,
    gatherer: Option<&'static Gatherer>,
    orders: &'static mut Orders,
    brain: &'static mut Brain,
    stance: &'static mut Stance,
    moving: Has<MoveGoal>,
    attacking: Has<AttackTarget>,
}

/// Whatever a command points at.
#[derive(QueryData)]
pub(crate) struct Pointee {
    pos: &'static Position,
    footprint: &'static Footprint,
    owner: Option<&'static Owner>,
    unit: Option<&'static Unit>,
    building: Option<&'static Building>,
    node: Option<&'static ResourceNode>,
    field: Option<&'static ResourceField>,
    drop_site: Option<&'static DropSite>,
    health: Option<&'static Health>,
    stats: Option<&'static Stats>,
    mobile: Option<&'static Mobile>,
    foundation: Has<UnderConstruction>,
}

impl PointeeItem<'_, '_> {
    fn class(&self) -> TargetClass {
        TargetClass::of(self.building.is_some(), self.mobile.is_some_and(|m| m.flying))
    }

    fn damaged(&self) -> bool {
        self.health.is_some_and(|h| h.hp < h.max)
    }
}

/// How a right-click resolves for one unit.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Smart {
    Move,
    /// Gather from the clicked node (spread over neighbours for groups).
    GatherNode,
    Order(Order),
}

#[derive(SystemParam)]
pub(crate) struct Intake<'w, 's> {
    data: Res<'w, Data>,
    players: Res<'w, Players>,
    nav: Res<'w, Nav>,
    regions: Option<Res<'w, NavRegions>>,
    spatial: Res<'w, SpatialIndex>,
    clock: Res<'w, SimClock>,
    notices: ResMut<'w, Notices>,
    died: MessageWriter<'w, EntityDied>,
    commands: Commands<'w, 's>,
    actors: Query<'w, 's, Actor, (With<Unit>, Without<Dead>)>,
    pointees: Query<'w, 's, Pointee, Without<Dead>>,
}

/// Applies the behaviour commands of this tick, in issue order.
pub(crate) fn intake_commands(tick: Res<TickCommands>, mut intake: Intake, mut doomed: Local<Vec<Entity>>) {
    doomed.clear();
    for cmd in &tick.list {
        intake.apply(cmd, &mut doomed);
    }
}

impl Intake<'_, '_> {
    fn apply(&mut self, cmd: &PlayerCommand, doomed: &mut Vec<Entity>) {
        let q = cmd.queue;
        match &cmd.kind {
            CommandKind::Smart { point, target } => self.smart(cmd, *point, *target),
            CommandKind::Move(p) => {
                let units = self.members(cmd);
                self.group_move(&units, *p, q, |_, slot| Order::Move { target: slot });
            }
            CommandKind::AttackMove(p) => {
                let units = self.members(cmd);
                self.group_move(&units, *p, q, |_, slot| Order::AttackMove { target: slot });
            }
            CommandKind::Patrol(p) => {
                let units = self.members(cmd);
                self.group_move(&units, *p, q, |start, slot| Order::Patrol { a: start, b: slot });
            }
            CommandKind::Attack(target) => self.attack(cmd, *target),
            CommandKind::Follow(target) => {
                for e in self.members(cmd) {
                    if e != *target && self.pointees.contains(*target) {
                        self.give(e, Order::Follow { target: *target }, q);
                    }
                }
            }
            CommandKind::Hold => {
                for e in self.members(cmd) {
                    self.give(e, Order::Hold, q);
                }
            }
            CommandKind::Stop => {
                for e in self.members(cmd) {
                    self.stop(e);
                }
            }
            CommandKind::Gather(target) => self.gather(cmd, *target),
            CommandKind::Build(site) => self.build(cmd, *site),
            CommandKind::Repair(target) => self.repair(cmd, *target),
            CommandKind::SetStance(stance) => {
                for e in self.members(cmd) {
                    self.set_stance(e, *stance);
                }
            }
            CommandKind::Delete => self.delete(cmd, doomed),
            CommandKind::PlaceBuilding { .. }
            | CommandKind::PlaceWall { .. }
            | CommandKind::Train { .. }
            | CommandKind::Research { .. }
            | CommandKind::UpgradeBuilding
            | CommandKind::CancelQueue { .. }
            | CommandKind::SetRally { .. } => {}
        }
    }

    /// The command's units this player may order, each once.
    fn members(&self, cmd: &PlayerCommand) -> Vec<Entity> {
        let mut out: Vec<Entity> = Vec::with_capacity(cmd.entities.len());
        for &e in &cmd.entities {
            if !out.contains(&e) && self.actors.get(e).is_ok_and(|a| a.owner.0 == cmd.player) {
                out.push(e);
            }
        }
        out
    }

    fn give(&mut self, e: Entity, order: Order, queue: bool) {
        let Ok(a) = self.actors.get_mut(e) else { return };
        let ActorItem { mut orders, mut brain, .. } = a;
        if !queue && brain.synced(&orders) && continues(orders.current(), &order) {
            // The same order nudged (an AI re-issuing its attack-move): keep
            // the unit's path and fight instead of restarting.
            orders.queue.truncate(1);
            brain.replace_front(&mut orders, order);
        } else {
            orders.set(order, queue);
        }
    }

    // --- point orders -------------------------------------------------------

    /// Spread `units` in a formation around `point`; `order(start, slot)`
    /// builds each unit's order from where it starts and its own slot.
    fn group_move(&mut self, units: &[Entity], point: Vec2, queue: bool, order: impl Fn(Vec2, Vec2) -> Order) {
        let members: Vec<Member> = units
            .iter()
            .filter_map(|&e| {
                let a = self.actors.get(e).ok()?;
                let from = if queue { planned_end(a.orders, a.pos.0) } else { a.pos.0 };
                Some(Member { entity: e, from, half: a.footprint.half })
            })
            .collect();
        for (e, slot) in formation::arrange(&members, point) {
            let slot = self.open_spot(e, slot);
            let start = members.iter().find(|m| m.entity == e).map_or(slot, |m| m.from);
            self.give(e, order(start, slot), queue);
        }
    }

    /// `p` inside the map, nudged to the nearest spot this unit fits: on
    /// ground it can get to (the bank facing a slot across the water from
    /// it, see `reach`), then clear of obstacles.
    fn open_spot(&self, e: Entity, p: Vec2) -> Vec2 {
        let Ok(a) = self.actors.get(e) else { return p };
        let (mask, radius, team) = (a.mobile.mobility.0, a.mobile.radius, self.players.team(a.owner.0));
        let p = Reach::of(self.regions.as_deref(), a.pos.0, a.mobile).nearest(self.nav.clamp_point(p, radius));
        if self.nav.clear_at(p, mask, radius, team) {
            p
        } else {
            self.nav.nearest_open(p, mask, radius, team, SLOT_SEARCH).unwrap_or(p)
        }
    }

    // --- right-click ----------------------------------------------------------

    fn smart(&mut self, cmd: &PlayerCommand, point: Vec2, target: Option<Entity>) {
        let units = self.members(cmd);
        let mut movers = Vec::new();
        let mut gatherers = Vec::new();
        for &e in &units {
            match target.map_or(Smart::Move, |t| self.resolve(e, t)) {
                Smart::Move => movers.push(e),
                Smart::GatherNode => gatherers.push(e),
                Smart::Order(order) => self.give(e, order, cmd.queue),
            }
        }
        if let Some(node) = target.filter(|_| !gatherers.is_empty()) {
            self.spread_gatherers(&gatherers, node, cmd.queue);
        }
        self.group_move(&movers, point, cmd.queue, |_, slot| Order::Move { target: slot });
    }

    /// Context resolution of a right-click on `target` for unit `e`.
    fn resolve(&self, e: Entity, target: Entity) -> Smart {
        let (Ok(a), Ok(t)) = (self.actors.get(e), self.pointees.get(target)) else { return Smart::Move };
        if e == target {
            return Smart::Move;
        }
        let me = a.owner.0;
        let them = t.owner.map(|o| o.0);
        if them.is_some_and(|o| self.players.hostile(me, o)) && t.health.is_some() {
            return match attack_reach(&self.data, a.stats, a.weapons, t.class()) {
                Some(_) => Smart::Order(Order::Attack { target }),
                None => Smart::Move,
            };
        }
        if let Some(node) = t.node {
            return if self.can_gather_node(&a, node) { Smart::GatherNode } else { Smart::Move };
        }
        let allied = them.is_some_and(|o| self.players.allied(me, o));
        let own = them == Some(me);
        let def = self.data.unit(a.unit.kind);
        let builds = def.gatherer.as_ref().is_some_and(|g| g.builds);
        let repairs = def.gatherer.as_ref().is_some_and(|g| g.repair > 0.0);
        if t.building.is_some() && allied {
            if t.foundation {
                return if builds { Smart::Order(Order::Build { site: target }) } else { Smart::Move };
            }
            if own {
                if t.field.is_some_and(|f| self.can_gather(&a, f.resource)) {
                    return Smart::Order(Order::Gather { target });
                }
                if repairs && t.damaged() {
                    return Smart::Order(Order::Repair { target });
                }
                let carrying = a.gatherer.and_then(|g| g.carrying.filter(|_| g.amount > 0.0));
                if carrying.is_some_and(|r| t.drop_site.is_some_and(|d| d.accepts(r))) {
                    return Smart::Order(Order::ReturnCargo);
                }
            }
            return Smart::Move;
        }
        if t.unit.is_some() && allied {
            let mechanical = t.stats.is_some_and(|s| s.tags.contains(Tag::Mechanical));
            if own && repairs && mechanical && t.damaged() {
                return Smart::Order(Order::Repair { target });
            }
            return Smart::Order(Order::Follow { target });
        }
        Smart::Move
    }

    fn can_gather(&self, a: &ActorReadOnlyItem, resource: ResourceId) -> bool {
        a.gatherer.is_some_and(|g| g.capacity > 0.0)
            && self.players.try_get(a.owner.0).is_some_and(|p| stats::gather_rate(&self.data, p, a.unit.kind, resource) > 0.0)
    }

    fn can_gather_node(&self, a: &ActorReadOnlyItem, node: &ResourceNode) -> bool {
        node.remaining > 0.0 && self.data.node(node.kind).requires_building_id.is_none() && self.can_gather(a, node.resource)
    }

    // --- gathering --------------------------------------------------------------

    fn gather(&mut self, cmd: &PlayerCommand, target: Entity) {
        let Ok(t) = self.pointees.get(target) else { return };
        let pos = t.pos.0;
        let units = self.members(cmd);
        if let Some(node) = t.node {
            let def = self.data.node(node.kind);
            if let Some(b) = def.requires_building_id {
                let text = format!("{} must be claimed with a {}", def.name, self.data.building(b).name);
                notify(&mut self.notices, cmd.player, NoticeKind::Warning, &text, Some(pos), self.clock.elapsed);
                return;
            }
            let node = *node;
            let (workers, others): (Vec<Entity>, Vec<Entity>) =
                units.iter().partition(|&&e| self.actors.get(e).is_ok_and(|a| self.can_gather_node(&a, &node)));
            self.spread_gatherers(&workers, target, cmd.queue);
            self.group_move(&others, pos, cmd.queue, |_, slot| Order::Move { target: slot });
        } else if let Some(field) = t.field {
            if t.owner.map(|o| o.0) != Some(cmd.player) {
                return;
            }
            let resource = field.resource;
            let (workers, others): (Vec<Entity>, Vec<Entity>) =
                units.iter().partition(|&&e| self.actors.get(e).is_ok_and(|a| self.can_gather(&a, resource)));
            for e in workers {
                self.give(e, Order::Gather { target }, cmd.queue);
            }
            self.group_move(&others, pos, cmd.queue, |_, slot| Order::Move { target: slot });
        }
    }

    /// Send `workers` to `node`, letting the overflow beyond what fits
    /// around it go to the nearest nodes of the same kind.
    fn spread_gatherers(&mut self, workers: &[Entity], node: Entity, queue: bool) {
        for (worker, target) in self.gather_assignment(workers, node) {
            self.give(worker, Order::Gather { target }, queue);
        }
    }

    fn gather_assignment(&self, workers: &[Entity], node: Entity) -> Vec<(Entity, Entity)> {
        let Ok(t) = self.pointees.get(node) else { return Vec::new() };
        let Some(kind) = t.node.map(|n| n.kind) else { return Vec::new() };
        let origin = t.pos.0;
        let mut sorted: Vec<(Entity, Vec2, Vec2)> =
            workers.iter().filter_map(|&e| self.actors.get(e).ok().map(|a| (e, a.pos.0, a.footprint.half))).collect();
        sorted.sort_by(|a, b| a.1.distance_squared(origin).total_cmp(&b.1.distance_squared(origin)).then(a.0.cmp(&b.0)));
        let worker_width = sorted.first().map_or(0.6, |w| w.2.x * 2.0);

        let mut nodes = vec![(node, node_room(t.footprint.half, worker_width))];
        let mut nearby: Vec<(f32, Entity, usize)> = Vec::new();
        self.spatial.for_each_near(origin, GATHER_SPREAD_RADIUS, |e| {
            if e.kind != SpatialKind::Node || e.type_index != kind.0 || e.entity == node || e.pos.distance(origin) > GATHER_SPREAD_RADIUS {
                return;
            }
            if let Ok(n) = self.pointees.get(e.entity)
                && n.node.is_some_and(|n| n.remaining > 0.0)
            {
                nearby.push((e.pos.distance_squared(origin), e.entity, node_room(n.footprint.half, worker_width)));
            }
        });
        nearby.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        nodes.extend(nearby.into_iter().map(|(_, e, c)| (e, c)));

        let mut slots = nodes.iter().flat_map(|&(e, c)| std::iter::repeat_n(e, c));
        sorted.iter().map(|&(w, _, _)| (w, slots.next().unwrap_or(node))).collect()
    }

    // --- targeted orders ----------------------------------------------------------

    fn attack(&mut self, cmd: &PlayerCommand, target: Entity) {
        let Ok(t) = self.pointees.get(target) else { return };
        if t.health.is_none() {
            return;
        }
        let (class, pos) = (t.class(), t.pos.0);
        let (mut able, mut unable) = (0, 0);
        for e in self.members(cmd) {
            if e == target {
                continue;
            }
            let can = self.actors.get(e).is_ok_and(|a| attack_reach(&self.data, a.stats, a.weapons, class).is_some());
            if can {
                self.give(e, Order::Attack { target }, cmd.queue);
                able += 1;
            } else {
                unable += 1;
            }
        }
        if able == 0 && unable > 0 {
            notify(&mut self.notices, cmd.player, NoticeKind::Warning, "Cannot attack that target", Some(pos), self.clock.elapsed);
        }
    }

    fn build(&mut self, cmd: &PlayerCommand, site: Entity) {
        let Ok(t) = self.pointees.get(site) else { return };
        let allied = t.owner.is_some_and(|o| self.players.allied(cmd.player, o.0));
        if !t.foundation || !allied {
            return;
        }
        for e in self.members(cmd) {
            if self.actors.get(e).is_ok_and(|a| self.data.unit(a.unit.kind).gatherer.as_ref().is_some_and(|g| g.builds)) {
                self.give(e, Order::Build { site }, cmd.queue);
            }
        }
    }

    fn repair(&mut self, cmd: &PlayerCommand, target: Entity) {
        let Ok(t) = self.pointees.get(target) else { return };
        if t.foundation {
            return self.build(cmd, target);
        }
        let own = t.owner.is_some_and(|o| o.0 == cmd.player);
        let repairable = t.building.is_some() || t.stats.is_some_and(|s| s.tags.contains(Tag::Mechanical));
        if !own || !repairable || !t.damaged() {
            return;
        }
        for e in self.members(cmd) {
            if e != target && self.actors.get(e).is_ok_and(|a| self.data.unit(a.unit.kind).gatherer.as_ref().is_some_and(|g| g.repair > 0.0)) {
                self.give(e, Order::Repair { target }, cmd.queue);
            }
        }
    }

    // --- immediate ------------------------------------------------------------------

    /// Drop every order and intent; the unit stays where it is.
    fn stop(&mut self, e: Entity) {
        let Ok(mut a) = self.actors.get_mut(e) else { return };
        a.orders.queue.clear();
        a.brain.reset_task();
        a.brain.active = None;
        a.brain.home = Some(a.pos.0);
        let (moving, attacking) = (a.moving, a.attacking);
        let mut ec = self.commands.entity(e);
        if moving {
            ec.try_remove::<MoveGoal>();
        }
        if attacking {
            ec.try_remove::<AttackTarget>();
        }
    }

    /// Change stance; an idle unit forgets what its old stance had it chase.
    fn set_stance(&mut self, e: Entity, stance: Stance) {
        let Ok(mut a) = self.actors.get_mut(e) else { return };
        if *a.stance == stance {
            return;
        }
        *a.stance = stance;
        if a.orders.queue.is_empty() && (a.brain.engaged.is_some() || a.brain.returning) {
            self.stop(e);
        }
    }

    fn delete(&mut self, cmd: &PlayerCommand, doomed: &mut Vec<Entity>) {
        for &e in &cmd.entities {
            let Ok(t) = self.pointees.get(e) else { continue };
            if t.owner.map(|o| o.0) != Some(cmd.player) || doomed.contains(&e) {
                continue;
            }
            let kind = match (t.unit, t.building) {
                (Some(u), _) => EntityKind::Unit(u.kind),
                (None, Some(b)) => EntityKind::Building(b.kind),
                (None, None) => continue,
            };
            doomed.push(e);
            self.commands.entity(e).try_insert(Dead);
            self.died.write(EntityDied { entity: e, owner: Some(cmd.player), kind, pos: t.pos.0, killer: None });
        }
    }
}

/// Is `new` the order already being carried out, re-issued to about the
/// same place?
fn continues(current: Option<&Order>, new: &Order) -> bool {
    match (current, new) {
        (Some(Order::Move { target: a }), Order::Move { target: b })
        | (Some(Order::AttackMove { target: a }), Order::AttackMove { target: b })
        | (Some(Order::Patrol { b: a, .. }), Order::Patrol { b, .. }) => a.distance(*b) <= REISSUE_SLACK,
        _ => false,
    }
}

/// Where a unit will be once its queued point orders are done.
fn planned_end(orders: &Orders, pos: Vec2) -> Vec2 {
    orders
        .queue
        .iter()
        .rev()
        .find_map(|o| match o {
            Order::Move { target } | Order::AttackMove { target } => Some(*target),
            Order::Patrol { b, .. } => Some(*b),
            _ => None,
        })
        .unwrap_or(pos)
}
