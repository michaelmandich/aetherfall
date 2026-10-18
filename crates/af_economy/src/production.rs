//! Production queues of complete buildings (`SimSet::Economy`): unit
//! training gated by population, fresh units sent to their rally, research
//! completion and building upgrades.

use std::collections::VecDeque;

use af_core::ext::combat::TargetClass;
use af_core::prelude::*;
use af_core::spawn::{self, SpawnStats};
use bevy::ecs::query::QueryData;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::{RallyFallback, refresh, warn};

/// Minimum seconds between two "Need more houses" notices to one player.
const HOUSED_NOTICE_INTERVAL: f32 = 10.0;
/// Ticks a finished training that found no room beside its building waits
/// before looking again (a crowd rarely clears sooner, and a look that
/// finds nothing tries every spot within `af_core::spawn::SPAWN_SEARCH`).
const NO_ROOM_RETRY_TICKS: u64 = 15;
/// How close a fresh unit walking to its rally gets before it stops: the
/// arrival distance af_behavior gives a Move order (its `MOVE_ARRIVE`).
const RALLY_ARRIVE: f32 = 0.35;

/// A building finished upgrading into `to`; applied by [`apply_building_upgrades`].
#[derive(Message, Clone, Copy, Debug)]
pub(crate) struct UpgradeFinished {
    pub building: Entity,
    pub to: BuildingTypeId,
}

#[derive(SystemParam)]
pub(crate) struct ProductionMessages<'w> {
    units: MessageWriter<'w, UnitSpawned>,
    techs: MessageWriter<'w, TechResearched>,
    ages: MessageWriter<'w, AgeAdvanced>,
    upgrades: MessageWriter<'w, UpgradeFinished>,
}

/// Bookkeeping kept between ticks (no per-tick allocation).
#[derive(Default)]
pub(crate) struct ProductionScratch {
    /// Population spawned so far this tick; Prepare's `pop_used` lacks it.
    pop_spawned: Vec<u32>,
    /// `SimClock::elapsed` of the last housed notice.
    housed_notice_at: Vec<f32>,
    /// Boxes of the units set down so far this tick (and whether they fly):
    /// the spatial index does not hold them yet.
    placed: Vec<(Obb, bool)>,
    /// Buildings whose finished training found no room to set its units
    /// down, and the tick to look again.
    no_room: Vec<(Entity, u64)>,
    /// Spots of the squad being set down.
    spots: Vec<Vec2>,
}

/// Where fresh units may stand this tick: the grid, the units and buildings
/// as the tick began, and the units set down since.
struct Room<'a> {
    nav: &'a NavGrid,
    spatial: &'a SpatialIndex,
    placed: &'a mut Vec<(Obb, bool)>,
    spots: &'a mut Vec<Vec2>,
}

/// What a rally target is, to choose a fresh unit's first order.
#[derive(QueryData)]
pub(crate) struct RallyInfo {
    position: &'static Position,
    facing: &'static Facing,
    footprint: &'static Footprint,
    owner: Option<&'static Owner>,
    node: Option<&'static ResourceNode>,
    field: Option<&'static ResourceField>,
    under_construction: Has<UnderConstruction>,
    unit: Has<Unit>,
    building: Has<Building>,
    mobile: Option<&'static Mobile>,
}

type RallyTargets<'w, 's> = Query<'w, 's, RallyInfo, Without<Dead>>;

type QueueView = (
    Entity,
    &'static Owner,
    &'static Position,
    &'static Facing,
    &'static Footprint,
    &'static Rally,
    Option<&'static RallyFallback>,
    &'static mut ProductionQueue,
);

/// Advance the front item of every complete building's queue by one tick
/// and finish it when its time is up. A unit waits (and its building is
/// flagged `pop_blocked`) while its squad does not fit in the population,
/// and, once trained, while there is no room beside its building to set its
/// models down clear of everything: it looks again every
/// [`NO_ROOM_RETRY_TICKS`], showing as finished meanwhile.
pub(crate) fn advance_queues(
    data: Res<Data>,
    nav: Res<Nav>,
    spatial: Res<SpatialIndex>,
    clock: Res<SimClock>,
    mut spawn_stats: ResMut<SpawnStats>,
    mut players: ResMut<Players>,
    mut notices: ResMut<Notices>,
    mut commands: Commands,
    mut queues: Query<QueueView, (With<Building>, Without<UnderConstruction>, Without<Dead>)>,
    targets: RallyTargets,
    mut out: ProductionMessages,
    mut scratch: Local<ProductionScratch>,
) {
    let data: &GameData = &data;
    let n = players.list.len();
    let scratch = &mut *scratch;
    scratch.pop_spawned.clear();
    scratch.pop_spawned.resize(n, 0);
    scratch.housed_notice_at.resize(n, f32::NEG_INFINITY);
    scratch.placed.clear();
    // Forget the waits of buildings that stopped waiting (gone, cancelled).
    scratch.no_room.retain(|(_, at)| *at + NO_ROOM_RETRY_TICKS > clock.tick);
    for (e, owner, pos, facing, footprint, rally, fallback, mut queue) in &mut queues {
        let Some(item) = queue.items.front().copied() else { continue };
        let pi = owner.0.index();
        let Some(p) = players.list.get(pi) else { continue };
        if let QueueKind::Unit(u) = item.kind {
            let def = data.unit(u);
            if p.pop_used + scratch.pop_spawned[pi] + def.pop * def.squad > p.pop_cap {
                if !queue.pop_blocked {
                    queue.pop_blocked = true;
                }
                let last = &mut scratch.housed_notice_at[pi];
                if clock.elapsed - *last >= HOUSED_NOTICE_INTERVAL || clock.elapsed < *last {
                    *last = clock.elapsed;
                    warn(&mut notices, &clock, owner.0, Reason::PopCapped.describe(data), Some(pos.0));
                }
                continue;
            }
        }
        if queue.pop_blocked {
            queue.pop_blocked = false;
        }
        // Half a tick of slack so float accumulation never costs a tick.
        if queue.progress + SIM_DT < item.time - SIM_DT * 0.5 {
            queue.progress += SIM_DT;
            continue;
        }
        match item.kind {
            QueueKind::Unit(u) => {
                let waiting = scratch.no_room.iter().position(|(b, _)| *b == e);
                if waiting.is_some_and(|i| scratch.no_room[i].1 > clock.tick) {
                    show_finished(&mut queue, item.time);
                    continue;
                }
                // A line upgrade finished earlier this tick already applies.
                let u = players.list[pi].mods.upgraded_unit(u);
                let building = footprint.obb(pos.0, facing.0);
                let fallback = fallback.map(|f| f.0);
                let mut room = Room { nav: &nav, spatial: &spatial, placed: &mut scratch.placed, spots: &mut scratch.spots };
                let trained = spawn_trained(&mut commands, data, &mut room, &players, owner.0, &building, rally.0, fallback, &targets, u, &mut out.units);
                let Some(models) = trained else {
                    spawn_stats.waited += 1;
                    let retry = clock.tick + NO_ROOM_RETRY_TICKS;
                    match waiting {
                        Some(i) => scratch.no_room[i].1 = retry,
                        None => scratch.no_room.push((e, retry)),
                    }
                    show_finished(&mut queue, item.time);
                    continue;
                };
                if let Some(i) = waiting {
                    scratch.no_room.swap_remove(i);
                }
                spawn_stats.spawned += 1;
                queue.items.pop_front();
                queue.progress = 0.0;
                scratch.pop_spawned[pi] += data.unit(u).pop * models;
                players.list[pi].stats.units_trained += models;
            }
            QueueKind::Tech(t) => {
                queue.items.pop_front();
                queue.progress = 0.0;
                complete_tech(data, &mut players, &mut notices, &clock, owner.0, t, &mut out);
            }
            QueueKind::Upgrade(to) => {
                queue.items.pop_front();
                queue.progress = 0.0;
                out.upgrades.write(UpgradeFinished { building: e, to });
            }
        }
    }
}

/// A trained unit waiting for room shows as finished.
fn show_finished(queue: &mut ProductionQueue, time: f32) {
    if queue.progress != time {
        queue.progress = time;
    }
}

/// Spawn one training of `u` beside `building`, facing away from it toward
/// the rally (or the building's front), sent where the rally says. A rally
/// onto an entity that is gone (a cut tree, a killed enemy) falls back on
/// the point the player clicked. A plain walk (to a point, or up to an
/// entity there is nothing to do with) is a bare `MoveGoal`, not a Move
/// order: the unit has no task on the way, so af_behavior keeps its stance
/// running and it hits back at whoever hurts it, where a Move order means
/// "go there whatever happens". Squad models stand side by side and share
/// one `SquadMember` id. Every model is set down clear of the units around
/// it and of those set down earlier this tick (see [`place_squad`]); when
/// one finds no room nothing spawns and this returns `None` (the training
/// waits). Returns the number of models spawned.
fn spawn_trained(
    commands: &mut Commands,
    data: &GameData,
    room: &mut Room,
    players: &Players,
    owner: PlayerId,
    building: &Obb,
    rally: Option<RallyTarget>,
    fallback: Option<Vec2>,
    targets: &RallyTargets,
    u: UnitTypeId,
    spawned: &mut MessageWriter<UnitSpawned>,
) -> Option<u32> {
    let Some(p) = players.try_get(owner) else { return Some(0) };
    let def = data.unit(u);
    let mobility = data.unit_mobility(u, p.faction);
    // Same clearance radius spawn_unit gives the unit's `Mobile`.
    let radius = (def.size[0] * 0.5).max(0.2);
    let target = match rally {
        Some(RallyTarget::Entity(t)) => targets.get(t).ok().map(|info| (t, info)),
        _ => None,
    };
    let point = match rally {
        Some(RallyTarget::Point(point)) => Some(point),
        Some(RallyTarget::Entity(_)) if target.is_none() => fallback,
        _ => None,
    };
    let front = building.center + af_geom::forward(building.yaw) * (building.half.y + 8.0);
    let toward = target.as_ref().map(|(_, info)| info.position.0).or(point).unwrap_or(front);
    let facing = af_geom::yaw_of(spawn::exit_direction(building, toward));
    let newcomer = spawn::Newcomer::of(data, p, u, facing);
    if !place_squad(room, building, toward, &newcomer, def.size, def.squad) {
        return None;
    }
    let exit = room.spots[0];
    let squad = (def.squad > 1).then(|| SquadMember { squad: af_core::next_seq() });
    let order = match &target {
        Some((t, info)) => Some(entity_order(data, room.nav, players, p, u, (mobility, radius), *t, info, exit)),
        None => point.map(|target| Order::Move { target }),
    };
    for &pos in room.spots.iter() {
        let e = spawn_unit(commands, data, p, u, pos, facing);
        let mut unit = commands.entity(e);
        if let Some(s) = squad {
            unit.insert(s);
        }
        match order {
            Some(Order::Move { target }) => {
                unit.insert(MoveGoal::point(target, RALLY_ARRIVE)).remove::<Idle>();
            }
            Some(o) => {
                unit.insert(Orders { queue: VecDeque::from([o]) }).remove::<Idle>();
            }
            None => {}
        }
        spawned.write(UnitSpawned { entity: e, owner, kind: u });
    }
    Some(def.squad)
}

/// Find room for the `models` of one training (each `unit`'s box, `size`
/// big) beside `building` toward `toward`, side by side across their
/// facing: the middle one first, at the exit on that side
/// (`af_core::spawn::clear_exit_point`), then the others outward, each at
/// its spot beside it or as near as there is room. Leaves the spots in
/// `room.spots` (model order) and their boxes in `room.placed`; false, with
/// nothing kept, when some model finds no room.
fn place_squad(room: &mut Room, building: &Obb, toward: Vec2, unit: &spawn::Newcomer, size: [f32; 3], models: u32) -> bool {
    let flying = unit.mobility.0 & af_geom::FLYING != 0;
    let side = af_geom::right(unit.yaw);
    let spacing = size[0].max(size[2]) + 0.3;
    let offset = |i: u32| (i as f32 - (models.max(1) - 1) as f32 * 0.5) * spacing;
    let mut order: Vec<u32> = (0..models).collect();
    order.sort_by(|a, b| offset(*a).abs().total_cmp(&offset(*b).abs()).then(a.cmp(b)));
    let kept = room.placed.len();
    room.spots.clear();
    room.spots.resize(models as usize, Vec2::ZERO);
    let mut anchor = None;
    for i in order {
        let spot = match anchor {
            None => spawn::clear_exit_point(room.nav, room.spatial, room.placed, building, toward, unit),
            Some(a) => spawn::clear_spot(room.nav, room.spatial, room.placed, unit, a + side * offset(i)),
        };
        let Some(spot) = spot else {
            room.placed.truncate(kept);
            return false;
        };
        anchor.get_or_insert(spot - side * offset(i));
        room.spots[i as usize] = spot;
        room.placed.push((Obb::new(spot, unit.half, unit.yaw), flying));
    }
    true
}

/// First order of a fresh unit of type `u` (with its mobility and
/// clearance radius), leaving at `exit`, rallied onto `target`: attack
/// something hostile (when one of its weapons can hit that kind of target,
/// else walk up to it, as a right-click would); build an own foundation
/// (builders); gather a node or field (workers able to gather it); follow a
/// friendly unit; otherwise walk up to the near side of the target's box,
/// since its centre lies inside its obstacle.
fn entity_order(
    data: &GameData,
    nav: &NavGrid,
    players: &Players,
    p: &PlayerState,
    u: UnitTypeId,
    (mobility, radius): (Mobility, f32),
    target: Entity,
    info: &RallyInfoItem,
    exit: Vec2,
) -> Order {
    let def = data.unit(u);
    let builds = def.gatherer.as_ref().is_some_and(|g| g.builds);
    let walk_up = || {
        let target_box = info.footprint.obb(info.position.0, info.facing.0);
        Order::Move { target: exit_point(nav, &target_box, exit, mobility, radius, p.team) }
    };
    if let Some(owner) = info.owner {
        if players.hostile(p.id, owner.0) {
            // `u` is the unit as trained (line upgrades applied): its def's
            // weapons are the ones it spawns with. An Attack nothing can
            // carry out would be dropped on the spot, leaving the unit idle
            // at the exit.
            let class = TargetClass::of(info.building, info.mobile.is_some_and(|m| m.flying));
            let armed = def.weapon_ids.iter().any(|w| class.allowed_by(&data.weapon(*w).targets));
            return if armed { Order::Attack { target } } else { walk_up() };
        }
        if owner.0 == p.id && info.under_construction && builds {
            return Order::Build { site: target };
        }
    }
    let resource = match (info.node, info.field) {
        (Some(n), _) if data.node(n.kind).requires_building_id.is_none() => Some(n.resource),
        (_, Some(f)) if !info.under_construction => Some(f.resource),
        _ => None,
    };
    if let Some(r) = resource
        && stats::gather_rate(data, p, u, r) > 0.0
    {
        return Order::Gather { target };
    }
    if info.unit && info.owner.is_some_and(|o| players.allied(p.id, o.0)) {
        return Order::Follow { target };
    }
    walk_up()
}

/// Mark `tech` researched and apply its effects to the player's state.
/// Entities are brought up to date afterwards by `refresh::refresh_researched`,
/// which reads the `TechResearched` written here.
fn complete_tech(
    data: &GameData,
    players: &mut Players,
    notices: &mut Notices,
    clock: &SimClock,
    player: PlayerId,
    tech: TechId,
    out: &mut ProductionMessages,
) {
    let Some(p) = players.list.get_mut(player.index()) else { return };
    let def = data.tech(tech);
    if let Some(done) = p.researched.get_mut(tech.index()) {
        *done = true;
    }
    if let Some(busy) = p.researching.get_mut(tech.index()) {
        *busy = false;
    }
    p.stats.techs += 1;
    let mut new_age = None;
    for effect in &def.effects {
        if matches!(effect, Effect::AdvanceAge) {
            p.age = (p.age + 1).min(MAX_AGE);
            new_age = Some(p.age);
        } else {
            p.mods.apply_effect(data, effect);
        }
    }
    out.techs.write(TechResearched { player, tech });
    match new_age {
        Some(age) => {
            out.ages.write(AgeAdvanced { player, age });
            let text = format!("{} has advanced to the {}", p.name, data.age(age).name);
            notices.push(None, NoticeKind::Info, text, None, clock.elapsed);
        }
        None => notices.push(Some(player), NoticeKind::Info, format!("{} researched", def.name), None, clock.elapsed),
    }
}

/// A building being turned into another type in place.
#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct UpgradeView {
    building: &'static mut Building,
    owner: &'static Owner,
    position: &'static Position,
    facing: &'static Facing,
    name: &'static mut Name,
    footprint: &'static mut Footprint,
    obstacle: Option<&'static mut Obstacle>,
    stats: &'static mut Stats,
    health: &'static mut Health,
    weapons: &'static mut Weapons,
    shield: Option<&'static mut Shield>,
    field: Option<&'static ResourceField>,
}

/// Turn each building whose upgrade finished into its new type: kind, name,
/// footprint (the obstacle only when its shape changes), stats with the hp
/// fraction kept, shield, weapons and the role components. The production
/// queue and rally carry over.
pub(crate) fn apply_building_upgrades(
    data: Res<Data>,
    players: Res<Players>,
    clock: Res<SimClock>,
    mut notices: ResMut<Notices>,
    mut commands: Commands,
    mut finished: MessageReader<UpgradeFinished>,
    mut completed: MessageWriter<BuildingCompleted>,
    mut buildings: Query<UpgradeView, Without<Dead>>,
) {
    let data: &GameData = &data;
    for msg in finished.read() {
        let Ok(mut b) = buildings.get_mut(msg.building) else { continue };
        let Some(p) = players.try_get(b.owner.0) else { continue };
        let from = data.building(b.building.kind);
        let def = data.building(msg.to);
        b.building.kind = msg.to;
        *b.name = Name::new(def.name.clone());
        let footprint = Footprint::from_size(def.size);
        let gate_team = def.gate.then_some(p.team);
        if let Some(mut obstacle) = b.obstacle
            && (b.footprint.half != footprint.half || obstacle.gate_team != gate_team)
        {
            *obstacle = Obstacle { obb: footprint.obb(b.position.0, b.facing.0), gate_team };
        }
        if *b.footprint != footprint {
            *b.footprint = footprint;
        }
        let st = stats::building_stats(data, p, msg.to);
        refresh::rescale_health(&mut b.health, st.max_hp);
        if !b.weapons.slots.iter().map(|s| s.weapon).eq(def.weapon_ids.iter().copied()) {
            b.weapons.slots = def.weapon_ids.iter().map(|w| WeaponSlot::new(*w)).collect();
        }
        let mut entity = commands.entity(msg.building);
        refresh::sync_shield(&mut entity, b.shield, def.shield.as_ref(), st.shield_max, true);
        sync_roles(&mut entity, def, b.field);
        if *b.stats != st {
            *b.stats = st;
        }
        completed.write(BuildingCompleted { entity: msg.building, owner: p.id, kind: msg.to });
        if !def.wall {
            let text = format!("{} upgraded to {}", from.name, def.name);
            notices.push(Some(p.id), NoticeKind::Info, text, Some(b.position.0), clock.elapsed);
        }
    }
}

/// Insert or remove the components that follow from a building def
/// (the same ones `spawn_building` adds). A resource field of the same
/// resource keeps its remaining amount and users.
fn sync_roles(entity: &mut EntityCommands, def: &BuildingDef, field: Option<&ResourceField>) {
    let mask = def.drop_site_ids.iter().fold(0u16, |m, r| m | 1 << r.0);
    sync(entity, (mask != 0).then_some(DropSite { mask }));
    sync(entity, (!def.trickle_amounts.is_zero()).then_some(Trickle(def.trickle_amounts)));
    match &def.resource_field {
        Some(f) if field.is_some_and(|old| old.resource == f.resource_id) => {}
        Some(f) => {
            entity.try_insert(ResourceField { resource: f.resource_id, remaining: f.amount, slots: f.slots, users: 0 });
        }
        None => {
            entity.try_remove::<ResourceField>();
        }
    }
    sync(entity, def.wall.then_some(Wall));
    sync(entity, def.gate.then_some(Gate));
}

fn sync<C: Component>(entity: &mut EntityCommands, value: Option<C>) {
    match value {
        Some(c) => {
            entity.try_insert(c);
        }
        None => {
            entity.try_remove::<C>();
        }
    }
}
