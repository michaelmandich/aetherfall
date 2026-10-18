//! The economy half of `PlayerCommand` handling ([`EconomyIntake`], inside
//! `SimSet::Commands`).
//!
//! One pass over the tick's commands in inbox order, so whatever a command
//! spends or refunds is seen by the next one, whatever its kind. Entities
//! not owned by the issuing player are ignored, dead or missing ones
//! skipped, non-finite coordinates refused, and refusals become a warning
//! notice for that player. Builders get their Build orders with the
//! placement (see [`crate::builders`]). af_behavior applies the tick's
//! deletes after this pass, so a building a `Delete` earlier in the tick
//! removes is already treated as gone here: it trains, researches,
//! upgrades, cancels and rallies nothing (nothing paid is lost with it).
//!
//! [`EconomyIntake`]: af_core::ext::economy::EconomyIntake

use af_core::ext::economy::PlacedSites;
use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::builders::{self, BuilderQuery};
use crate::{RallyFallback, warn};

type QueueQuery<'w, 's> = Query<'w, 's, (&'static Building, &'static Owner, &'static mut ProductionQueue, Has<UnderConstruction>), Without<Dead>>;

type RallyQuery<'w, 's> = Query<'w, 's, (&'static Owner, &'static mut Rally), (With<Building>, Without<Dead>)>;

/// Per-tick lists kept between ticks (no per-tick allocation).
#[derive(Default)]
pub(crate) struct IntakeScratch {
    /// (player, entity) of every `Delete` issued so far this tick.
    deleted: Vec<(PlayerId, Entity)>,
    /// The current command's entities that no earlier `Delete` removed.
    kept: Vec<Entity>,
}

/// Applies the economy commands of this tick, in issue order.
pub(crate) fn apply_commands(
    data: Res<Data>,
    tick: Res<TickCommands>,
    clock: Res<SimClock>,
    mut players: ResMut<Players>,
    mut notices: ResMut<Notices>,
    mut commands: Commands,
    mut placer: Placer,
    mut buildings: QueueQuery,
    mut rallies: RallyQuery,
    mut builder_units: BuilderQuery,
    mut scratch: Local<IntakeScratch>,
) {
    let data: &GameData = &data;
    let scratch = &mut *scratch;
    placer.begin_tick();
    scratch.deleted.clear();
    for (index, cmd) in tick.list.iter().enumerate() {
        let Some(p) = players.list.get_mut(cmd.player.index()) else { continue };
        let entities: &[Entity] = if cmd.entities.iter().any(|e| scratch.deleted.contains(&(cmd.player, *e))) {
            scratch.kept.clear();
            scratch.kept.extend(cmd.entities.iter().copied().filter(|e| !scratch.deleted.contains(&(cmd.player, *e))));
            &scratch.kept
        } else {
            &cmd.entities
        };
        let refusal = match cmd.kind {
            CommandKind::PlaceBuilding { kind, pos, facing } if kind.index() < data.buildings.len() => placer
                .place_building(&mut commands, data, p, index, kind, pos, facing)
                .err()
                .map(|e| (e.describe(data), finite(pos))),
            CommandKind::PlaceWall { kind, from, to } if kind.index() < data.buildings.len() && data.building(kind).wall => placer
                .place_wall(&mut commands, data, p, index, kind, from, to)
                .map(|e| (e.describe(data), finite(from))),
            CommandKind::Train { unit, count } if unit.index() < data.units.len() => {
                train(data, p, &mut buildings, entities, unit, count).map(|r| (r.describe(data), None))
            }
            CommandKind::Research { tech } if tech.index() < data.techs.len() => {
                research(data, p, &mut buildings, entities, tech).map(|r| (r.describe(data), None))
            }
            CommandKind::UpgradeBuilding => upgrade(data, p, &mut buildings, entities).map(|r| (r.describe(data), None)),
            CommandKind::CancelQueue { index } => {
                cancel(p, &mut buildings, entities, index);
                None
            }
            CommandKind::SetRally { point, target } | CommandKind::Smart { point, target } => {
                set_rally(&mut commands, &mut rallies, cmd.player, entities, point, target);
                None
            }
            CommandKind::Delete => {
                scratch.deleted.extend(cmd.entities.iter().map(|e| (cmd.player, *e)));
                None
            }
            _ => None,
        };
        if matches!(cmd.kind, CommandKind::PlaceBuilding { .. } | CommandKind::PlaceWall { .. }) {
            let list = &placer.sites.list;
            let first = list.iter().rposition(|s| s.0 != index).map_or(0, |i| i + 1);
            if first < list.len() {
                builders::send_builders(data, &mut commands, &mut builder_units, &tick.list, index, list[first..].iter().map(|s| s.1));
            }
        }
        if let Some((text, pos)) = refusal {
            warn(&mut notices, &clock, cmd.player, text, pos);
        }
    }
}

/// `p` when it is a usable map position (for a notice's location).
fn finite(p: Vec2) -> Option<Vec2> {
    p.is_finite().then_some(p)
}

/// Validates, pays for and spawns foundations, recording each in [`PlacedSites`].
#[derive(SystemParam)]
pub(crate) struct Placer<'w, 's> {
    nav: Res<'w, Nav>,
    spatial: Res<'w, SpatialIndex>,
    placed_msgs: MessageWriter<'w, BuildingPlaced>,
    nodes: Query<'w, 's, &'static mut ResourceNode>,
    sites: ResMut<'w, PlacedSites>,
    /// Footprints placed this tick, by every player.
    placed: Local<'s, Vec<Obb>>,
}

impl Placer<'_, '_> {
    fn begin_tick(&mut self) {
        self.placed.clear();
        self.sites.list.clear();
    }

    /// `PlaceBuilding`: one foundation, or why not (nothing is charged then).
    fn place_building(
        &mut self,
        commands: &mut Commands,
        data: &GameData,
        p: &mut PlayerState,
        command: usize,
        kind: BuildingTypeId,
        pos: Vec2,
        facing: f32,
    ) -> Result<(), PlacementError> {
        // check_placement lets NaN through every comparison.
        if !(pos.is_finite() && facing.is_finite()) {
            return Err(PlacementError::OutOfBounds);
        }
        self.place(commands, data, p, command, kind, pos, facing)
    }

    /// `PlaceWall`: lay each segment along the drag in order, paying as it
    /// goes. Returns the failure worth reporting, if any.
    fn place_wall(
        &mut self,
        commands: &mut Commands,
        data: &GameData,
        p: &mut PlayerState,
        command: usize,
        kind: BuildingTypeId,
        from: Vec2,
        to: Vec2,
    ) -> Option<PlacementError> {
        if !(from.is_finite() && to.is_finite()) {
            return Some(PlacementError::OutOfBounds);
        }
        // Anything past the map's diagonal would lie outside it anyway, and
        // wall_segments allocates one entry per segment of the drag. In f64,
        // where no finite drag overflows.
        let drag = to.as_dvec2() - from.as_dvec2();
        let (len, longest) = (drag.length(), f64::from(self.nav.world_size().length()));
        let to = if len > longest { from + (drag * (longest / len)).as_vec2() } else { to };
        let mut failure = None;
        for (pos, yaw) in wall_segments(data, kind, from, to) {
            match self.place(commands, data, p, command, kind, pos, yaw) {
                Ok(()) => {}
                // Rule refusals (resources, age...) hold for every later
                // segment too: stop at the first one.
                Err(e @ PlacementError::Rule(_)) => return Some(e),
                // A blocked segment leaves a gap; keep laying the rest.
                Err(e) => {
                    failure.get_or_insert(e);
                }
            }
        }
        failure
    }

    /// Place one foundation of `kind` for `p` on behalf of tick command
    /// `command`, or say why not (nothing is charged then).
    fn place(
        &mut self,
        commands: &mut Commands,
        data: &GameData,
        p: &mut PlayerState,
        command: usize,
        kind: BuildingTypeId,
        pos: Vec2,
        facing: f32,
    ) -> Result<(), PlacementError> {
        let spot = check_placement(data, &self.nav, &self.spatial, p, kind, pos, facing, true, &self.placed)?;
        if !p.pay(&stats::building_cost(data, p, kind)) {
            return Err(PlacementError::Rule(Reason::CannotAfford));
        }
        // Prepare recounts this next tick; bump it now so build limits also
        // hold for several placements within one tick.
        if let Some(n) = p.buildings_any.get_mut(kind.index()) {
            *n += 1;
        }
        let site = spawn_building(commands, data, p, kind, spot.pos, spot.facing, false);
        if let Some(node) = spot.node {
            if let Ok(mut n) = self.nodes.get_mut(node) {
                n.claimed_by = Some(site);
            }
            commands.entity(site).insert(ClaimsNode(node));
        }
        // Tested against the later placements of this tick exactly as
        // check_placement tests existing buildings: shrunk by the flush
        // tolerance, so wall segments laid end to end fit.
        self.placed.push(placed_footprint(data, kind, spot.pos, spot.facing));
        self.sites.list.push((command, site));
        self.placed_msgs.write(BuildingPlaced { entity: site, owner: p.id, kind });
        Ok(())
    }
}

/// Why a queue command was refused.
enum Refusal {
    Rule(Reason),
    QueueFull,
}

impl Refusal {
    fn describe(&self, data: &GameData) -> String {
        match self {
            Refusal::Rule(r) => r.describe(data),
            Refusal::QueueFull => "Production queue is full".into(),
        }
    }

    /// Lower is more useful to the player when several selected buildings
    /// refuse for different reasons: what more resources or houses would
    /// fix, then a full queue, then the requirements of what was asked for,
    /// and last the reasons that only say another building was selected too.
    fn rank(&self) -> u8 {
        match self {
            Refusal::Rule(r) if r.is_soft() => 0,
            Refusal::QueueFull => 1,
            Refusal::Rule(Reason::UnderConstruction) => 3,
            Refusal::Rule(Reason::WrongBuilding) => 4,
            Refusal::Rule(_) => 2,
        }
    }
}

/// Keep the more useful of the refusal so far and `r`.
fn keep_best(refusal: &mut Option<Refusal>, r: Refusal) {
    if refusal.as_ref().is_none_or(|old| r.rank() < old.rank()) {
        *refusal = Some(r);
    }
}

/// Queue `count` trainings, each at the eligible building with the shortest
/// queue. Asking for a unit whose line was upgraded trains the upgrade (the
/// unit `rules::train_menu` shows instead). `rules::can_train` checks the
/// population against living units only, so queued trainings reserve no
/// room: production holds a squad at the front of its queue until it fits.
fn train(data: &GameData, p: &mut PlayerState, buildings: &mut QueueQuery, entities: &[Entity], unit: UnitTypeId, count: u32) -> Option<Refusal> {
    let unit = p.mods.upgraded_unit(unit);
    let cost = stats::unit_cost(data, p, unit);
    let time = stats::unit_train_time(data, p, unit);
    for _ in 0..count {
        let mut refusal = None;
        let mut best: Option<(Entity, usize)> = None;
        for &e in entities {
            let Ok((b, owner, queue, under_construction)) = buildings.get(e) else { continue };
            if owner.0 != p.id {
                continue;
            }
            let len = queue.items.len();
            let verdict = if under_construction {
                Err(Refusal::Rule(Reason::UnderConstruction))
            } else if let Err(r) = rules::can_train(data, p, b.kind, unit, true) {
                Err(Refusal::Rule(r))
            } else if len >= ProductionQueue::MAX {
                Err(Refusal::QueueFull)
            } else {
                Ok(())
            };
            match verdict {
                Ok(()) if best.is_none_or(|(_, n)| len < n) => best = Some((e, len)),
                Ok(()) => {}
                Err(r) => keep_best(&mut refusal, r),
            }
        }
        let Some((e, _)) = best else { return refusal };
        if !p.pay(&cost) {
            return Some(Refusal::Rule(Reason::CannotAfford));
        }
        if let Ok((_, _, mut queue, _)) = buildings.get_mut(e) {
            queue.items.push_back(QueueItem { kind: QueueKind::Unit(unit), paid: cost, time });
        }
    }
    None
}

/// Queue `tech` at the least busy of the listed buildings that research it.
fn research(data: &GameData, p: &mut PlayerState, buildings: &mut QueueQuery, entities: &[Entity], tech: TechId) -> Option<Refusal> {
    let def = data.tech(tech);
    let mut refusal = None;
    let mut best: Option<(Entity, usize)> = None;
    for &e in entities {
        let Ok((b, owner, queue, under_construction)) = buildings.get(e) else { continue };
        if owner.0 != p.id {
            continue;
        }
        let len = queue.items.len();
        if b.kind != def.researched_at_id {
            keep_best(&mut refusal, Refusal::Rule(Reason::WrongBuilding));
        } else if under_construction {
            keep_best(&mut refusal, Refusal::Rule(Reason::UnderConstruction));
        } else if len >= ProductionQueue::MAX {
            keep_best(&mut refusal, Refusal::QueueFull);
        } else if best.is_none_or(|(_, n)| len < n) {
            best = Some((e, len));
        }
    }
    let Some((e, _)) = best else { return refusal };
    if let Err(r) = rules::can_research(data, p, def.researched_at_id, tech, true) {
        return Some(Refusal::Rule(r));
    }
    let paid = def.cost.amounts;
    if !p.pay(&paid) {
        return Some(Refusal::Rule(Reason::CannotAfford));
    }
    if let Some(busy) = p.researching.get_mut(tech.index()) {
        *busy = true;
    }
    if let Ok((_, _, mut queue, _)) = buildings.get_mut(e) {
        queue.items.push_back(QueueItem { kind: QueueKind::Tech(tech), paid, time: def.time });
    }
    None
}

/// Queue an upgrade at every listed building that may take one (one upgrade
/// per building at a time), paying each in turn; a building the player
/// cannot pay for is skipped, so cheaper ones later in the list still go.
fn upgrade(data: &GameData, p: &mut PlayerState, buildings: &mut QueueQuery, entities: &[Entity]) -> Option<Refusal> {
    let mut refusal = None;
    let mut queued_any = false;
    let mut short = false;
    for &e in entities {
        let Ok((b, owner, mut queue, under_construction)) = buildings.get_mut(e) else { continue };
        if owner.0 != p.id {
            continue;
        }
        if under_construction {
            keep_best(&mut refusal, Refusal::Rule(Reason::UnderConstruction));
            continue;
        }
        if queue.items.iter().any(|i| matches!(i.kind, QueueKind::Upgrade(_))) {
            keep_best(&mut refusal, Refusal::Rule(Reason::InProgress));
            continue;
        }
        if queue.items.len() >= ProductionQueue::MAX {
            keep_best(&mut refusal, Refusal::QueueFull);
            continue;
        }
        if let Err(r) = rules::can_upgrade(data, p, b.kind, false) {
            keep_best(&mut refusal, Refusal::Rule(r));
            continue;
        }
        let Some(up) = data.building(b.kind).upgrade.as_ref() else { continue };
        let paid = up.cost.amounts;
        if !p.pay(&paid) {
            short = true;
            continue;
        }
        queue.items.push_back(QueueItem { kind: QueueKind::Upgrade(up.to_id), paid, time: up.time });
        queued_any = true;
    }
    if short {
        Some(Refusal::Rule(Reason::CannotAfford))
    } else if queued_any {
        None
    } else {
        refusal
    }
}

/// Remove slot `index` of the first own building listed and refund it.
fn cancel(p: &mut PlayerState, buildings: &mut QueueQuery, entities: &[Entity], index: usize) {
    let Some(e) = entities.iter().copied().find(|e| buildings.get(*e).is_ok_and(|(_, owner, _, _)| owner.0 == p.id)) else {
        return;
    };
    let Ok((_, _, mut queue, _)) = buildings.get_mut(e) else { return };
    let Some(item) = queue.items.remove(index) else { return };
    p.refund(&item.paid);
    if let QueueKind::Tech(t) = item.kind
        && let Some(busy) = p.researching.get_mut(t.index())
    {
        *busy = false;
    }
    if index == 0 {
        queue.progress = 0.0;
        queue.pop_blocked = false;
    }
}

/// `SetRally`, and `Smart` for the buildings among its entities (af_behavior
/// handles the units). Rallying a building onto itself clears its rally; a
/// rally onto an entity also remembers the click as its fallback.
fn set_rally(commands: &mut Commands, rallies: &mut RallyQuery, player: PlayerId, entities: &[Entity], point: Vec2, target: Option<Entity>) {
    if !point.is_finite() {
        return;
    }
    for &e in entities {
        let Ok((owner, mut rally)) = rallies.get_mut(e) else { continue };
        if owner.0 != player {
            continue;
        }
        rally.0 = match target {
            Some(t) if t == e => None,
            Some(t) => {
                commands.entity(e).try_insert(RallyFallback(point));
                Some(RallyTarget::Entity(t))
            }
            None => Some(RallyTarget::Point(point)),
        };
    }
}
