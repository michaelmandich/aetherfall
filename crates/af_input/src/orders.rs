//! Turning clicks into `PlayerCommand`s: right-click `Smart`, and the click
//! that completes a placement or targeting `InputMode`. Every order given
//! also spawns a click marker (`ClickPing` + `ClickMark`) saying which
//! order it was, for af_render to draw.

use af_core::ext::input::{ClickMark, OrderMark};
use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::placement::{plan_building, plan_wall, wall_end, PlaceCtx, PlacementWorld};

/// Sends commands, notices and click markers for the local player.
#[derive(SystemParam)]
pub(crate) struct Issuer<'w, 's> {
    inbox: ResMut<'w, CommandInbox>,
    notices: ResMut<'w, Notices>,
    clock: Res<'w, SimClock>,
    commands: Commands<'w, 's>,
}

impl Issuer<'_, '_> {
    pub fn send(&mut self, command: PlayerCommand) {
        self.inbox.push(command);
    }

    pub fn notice(&mut self, player: Option<PlayerId>, kind: NoticeKind, text: impl Into<String>) {
        self.notices.push(player, kind, text, None, self.clock.elapsed);
    }

    /// A click marker at `pos` for the order just sent.
    pub fn mark(&mut self, pos: Vec2, mark: ClickMark) {
        self.commands.spawn(mark.ping(pos));
    }
}

/// The local player's own selected, living units and buildings.
#[derive(SystemParam)]
pub(crate) struct OwnSelection<'w, 's> {
    local: Res<'w, LocalPlayer>,
    data: Res<'w, Data>,
    units: Query<'w, 's, (Entity, &'static Owner, &'static Unit, Option<&'static Gatherer>), (With<Selected>, Without<Dead>)>,
    buildings: Query<'w, 's, (Entity, &'static Owner), (With<Selected>, With<Building>, Without<Dead>)>,
}

impl OwnSelection<'_, '_> {
    /// The commanding player; `None` while spectating.
    pub fn player(&self) -> Option<PlayerId> {
        self.local.0
    }

    fn units_where(&self, keep: impl Fn(&Unit) -> bool) -> Vec<Entity> {
        let Some(me) = self.local.0 else { return Vec::new() };
        let mut out: Vec<Entity> = self.units.iter().filter(|(_, o, u, _)| o.0 == me && keep(u)).map(|(e, ..)| e).collect();
        out.sort();
        out
    }

    /// Own selected units, sorted (commands carry a stable entity order).
    pub fn units(&self) -> Vec<Entity> {
        self.units_where(|_| true)
    }

    /// Own selected units that can construct buildings.
    pub fn builders(&self) -> Vec<Entity> {
        self.units_where(|u| self.data.unit(u.kind).gatherer.as_ref().is_some_and(|g| g.builds))
    }

    /// Own selected buildings, sorted.
    pub fn buildings(&self) -> Vec<Entity> {
        let Some(me) = self.local.0 else { return Vec::new() };
        let mut out: Vec<Entity> = self.buildings.iter().filter(|(_, o)| o.0 == me).map(|(e, _)| e).collect();
        out.sort();
        out
    }

    /// Own selected units and buildings, sorted.
    pub fn all(&self) -> Vec<Entity> {
        let mut out = self.units();
        out.extend(self.buildings());
        out.sort();
        out
    }

    /// What the own selected units can do, for guessing a right-click's order.
    fn crew(&self) -> Crew {
        let mut crew = Crew::default();
        let Some(me) = self.local.0 else { return crew };
        for (_, owner, unit, gatherer) in &self.units {
            if owner.0 != me {
                continue;
            }
            crew.units = true;
            if let Some(g) = &self.data.unit(unit.kind).gatherer {
                crew.gathers |= g.capacity > 0.0;
                crew.builds |= g.builds;
                crew.repairs |= g.repair > 0.0;
            }
            if let Some(r) = gatherer.filter(|g| g.amount > 0.0).and_then(|g| g.carrying) {
                crew.cargo |= 1 << r.0;
            }
        }
        crew
    }
}

/// What the own selection can do, as far as a right-click's marker cares.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Crew {
    /// Some own unit is selected (else only buildings, which set a rally).
    units: bool,
    gathers: bool,
    builds: bool,
    repairs: bool,
    /// The resources selected workers carry, as a `DropSite` mask.
    cargo: u16,
}

/// Whatever a right-click points at.
#[derive(QueryData)]
pub(crate) struct Pointee {
    pos: &'static Position,
    owner: Option<&'static Owner>,
    health: Option<&'static Health>,
    stats: Option<&'static Stats>,
    unit: Has<Unit>,
    building: Has<Building>,
    node: Option<&'static ResourceNode>,
    field: Has<ResourceField>,
    foundation: Has<UnderConstruction>,
    drop_site: Option<&'static DropSite>,
}

/// What a right-clicked entity is, from the local player's side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pointed {
    /// Owned by a player hostile to the local one.
    hostile: bool,
    /// Owned by the local player or an ally.
    allied: bool,
    /// Owned by the local player.
    own: bool,
    /// Has hit points (can be attacked).
    mortal: bool,
    damaged: bool,
    mechanical: bool,
    unit: bool,
    building: bool,
    node: bool,
    /// A node only a building placed on it can gather.
    claim: bool,
    field: bool,
    foundation: bool,
    /// The resources it takes in as a drop site (a `DropSite` mask).
    drop_mask: u16,
}

impl Pointed {
    fn of(t: &PointeeItem, me: PlayerId, players: &Players, data: &GameData) -> Pointed {
        let owner = t.owner.map(|o| o.0);
        Pointed {
            hostile: owner.is_some_and(|o| players.hostile(me, o)),
            allied: owner.is_some_and(|o| players.allied(me, o)),
            own: owner == Some(me),
            mortal: t.health.is_some(),
            damaged: t.health.is_some_and(|h| h.hp < h.max),
            mechanical: t.stats.is_some_and(|s| s.tags.contains(Tag::Mechanical)),
            unit: t.unit,
            building: t.building,
            node: t.node.is_some(),
            claim: t.node.is_some_and(|n| data.node(n.kind).requires_building_id.is_some()),
            field: t.field,
            foundation: t.foundation,
            drop_mask: t.drop_site.map_or(0, |d| d.mask),
        }
    }
}

/// The marker for a right-click: what the order will most likely do, and
/// whether it hugs the entity pointed at. The sim resolves `Smart` per
/// unit (af_behavior's intake); this mirrors it for the selection as a
/// whole: attack a foe, gather a node (not one only a building can claim)
/// or an own field, build an allied foundation, repair an own damaged
/// building or machine, bring cargo home to an own drop site that takes
/// it (marked as gathering), follow an allied unit, else move to the
/// point. A move onto an allied building (the units stop beside it) is
/// marked round the building too: at the point, under its roof, the
/// marker would be hidden. A selection of buildings only sets its rally
/// there.
fn smart_mark(crew: Crew, pointed: Option<Pointed>) -> (OrderMark, bool) {
    if !crew.units {
        return (OrderMark::Rally, pointed.is_some());
    }
    let Some(t) = pointed else { return (OrderMark::Move, false) };
    let hug = |mark| (mark, true);
    if t.hostile && t.mortal {
        return hug(OrderMark::Attack);
    }
    if t.node {
        return if crew.gathers && !t.claim { hug(OrderMark::Gather) } else { (OrderMark::Move, false) };
    }
    if t.building && t.allied {
        return hug(if t.foundation {
            if crew.builds { OrderMark::Build } else { OrderMark::Move }
        } else if t.own && t.field && crew.gathers {
            OrderMark::Gather
        } else if t.own && t.damaged && crew.repairs {
            OrderMark::Repair
        } else if t.own && crew.cargo & t.drop_mask != 0 {
            OrderMark::Gather
        } else {
            OrderMark::Move
        });
    }
    if t.unit && t.allied {
        // Allied units are followed; a damaged own machine is repaired.
        return hug(if t.own && t.mechanical && t.damaged && crew.repairs { OrderMark::Repair } else { OrderMark::Move });
    }
    (OrderMark::Move, false)
}

pub(crate) fn shift_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
}

/// Right-click in Normal mode: `Smart` at the cursor for the own selection
/// (buildings in it set their rally point).
pub(crate) fn smart_command(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<InputMode>,
    cursor: Res<CursorWorld>,
    own: OwnSelection,
    targets: Query<Pointee>,
    players: Res<Players>,
    mut issuer: Issuer,
) {
    if !mouse.just_pressed(MouseButton::Right) || *mode != InputMode::Normal || cursor.over_ui {
        return;
    }
    let Some(me) = own.player() else { return };
    let entities = own.all();
    if entities.is_empty() {
        return;
    }
    let target = cursor.hovered;
    let target_info = target.and_then(|t| targets.get(t).ok());
    // A flyer against the sky has no ground point under the cursor.
    let Some(point) = cursor.ground.or(target_info.as_ref().map(|t| t.pos.0)) else { return };
    let queued = shift_held(&keys);
    issuer.send(PlayerCommand::new(me, entities, CommandKind::Smart { point, target }).queued(queued));
    let (kind, hug) = smart_mark(own.crew(), target_info.map(|t| Pointed::of(&t, me, &players, &own.data)));
    issuer.mark(point, ClickMark::new(kind, target.filter(|_| hug), queued));
}

/// Clicks while placing or targeting: left click completes the mode,
/// right click cancels it. Spectators have no modes.
pub(crate) fn mode_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<CursorWorld>,
    mut mode: ResMut<InputMode>,
    own: OwnSelection,
    world: PlacementWorld,
    mortals: Query<Option<&Owner>, (With<Health>, Without<Dead>)>,
    players: Res<Players>,
    mut issuer: Issuer,
) {
    if *mode == InputMode::Normal {
        return;
    }
    // Spectators (and a local slot without a player) cannot act.
    let Some((me, ctx)) = own.player().and_then(|me| Some((me, world.context(me)?))) else {
        *mode = InputMode::Normal;
        return;
    };
    if cursor.over_ui {
        return;
    }
    if mouse.just_pressed(MouseButton::Right) {
        *mode = InputMode::Normal;
        return;
    }
    let (true, Some(ground)) = (mouse.just_pressed(MouseButton::Left), cursor.ground) else { return };
    let shift = shift_held(&keys);
    let current = *mode;
    let next = match current {
        InputMode::Normal => current,
        InputMode::Place { kind, facing } => {
            let placed = place_building(me, &ctx, own.builders(), kind, ground, facing);
            placed.finish(me, &mut issuer, shift, current, |_| current)
        }
        InputMode::PlaceWall { kind, start: None } => InputMode::PlaceWall { kind, start: Some(ground) },
        InputMode::PlaceWall { kind, start: Some(from) } => {
            let placed = place_wall(me, &ctx, own.builders(), kind, from, ground);
            // Shift chains the next line from where this one ended.
            placed.finish(me, &mut issuer, shift, current, |end| InputMode::PlaceWall { kind, start: Some(end) })
        }
        InputMode::Target(action) => match target_command(me, &own, action, ground, Clicked::new(me, cursor.hovered, &mortals, &players)) {
            Ok(None) => InputMode::Normal,
            Ok(Some((command, mark))) => {
                issuer.send(command.queued(shift));
                issuer.mark(ground, ClickMark { queued: shift, ..mark });
                if shift { current } else { InputMode::Normal }
            }
            Err(why) => {
                issuer.notice(Some(me), NoticeKind::Warning, why);
                current
            }
        },
    };
    if next != current {
        *mode = next;
    }
}

/// Outcome of a placement click.
enum Placed {
    /// Nobody selected can build: leave the mode so the next click selects.
    NoBuilders,
    /// Nothing fits there (why): stay in the mode and try elsewhere.
    Invalid(String),
    /// Ready to send; the building or line ends at `end`. `marked` is
    /// where the click marker goes and what it shows.
    Ready { command: PlayerCommand, end: Vec2, marked: (Vec2, OrderMark) },
}

impl Placed {
    /// Send the command or explain the refusal; returns the next mode.
    /// `keep` builds the follow-up mode while shift is held.
    fn finish(self, me: PlayerId, issuer: &mut Issuer, shift: bool, current: InputMode, keep: impl FnOnce(Vec2) -> InputMode) -> InputMode {
        match self {
            Placed::NoBuilders => {
                issuer.notice(Some(me), NoticeKind::Warning, "Select workers to build");
                InputMode::Normal
            }
            Placed::Invalid(why) => {
                issuer.notice(Some(me), NoticeKind::Warning, why);
                current
            }
            Placed::Ready { command, end, marked: (at, mark) } => {
                issuer.send(command.queued(shift));
                issuer.mark(at, ClickMark::new(mark, None, shift));
                if shift { keep(end) } else { InputMode::Normal }
            }
        }
    }
}

/// Place one building with `builders`, if it fits.
fn place_building(me: PlayerId, ctx: &PlaceCtx, builders: Vec<Entity>, kind: BuildingTypeId, pos: Vec2, facing: f32) -> Placed {
    if builders.is_empty() {
        return Placed::NoBuilders;
    }
    let ghost = plan_building(ctx, kind, pos, facing);
    if let Some(err) = ghost.error {
        return Placed::Invalid(err.describe(ctx.data()));
    }
    let command = CommandKind::PlaceBuilding { kind, pos: ghost.pos, facing: ghost.facing };
    let marked = (ghost.pos, OrderMark::Place { kind, facing: ghost.facing });
    Placed::Ready { command: PlayerCommand::new(me, builders, command), end: ghost.pos, marked }
}

/// Place a wall line `from` -> `to` with `builders` if at least one segment
/// fits (the economy skips the ones that do not).
fn place_wall(me: PlayerId, ctx: &PlaceCtx, builders: Vec<Entity>, kind: BuildingTypeId, from: Vec2, to: Vec2) -> Placed {
    if builders.is_empty() {
        return Placed::NoBuilders;
    }
    let mut ghosts = Vec::new();
    plan_wall(ctx, kind, from, to, &mut ghosts);
    // Refuse only when no segment fits, giving the first segment's reason.
    if ghosts.iter().all(|g| g.error.is_some())
        && let Some(err) = ghosts.first().and_then(|g| g.error)
    {
        return Placed::Invalid(err.describe(ctx.data()));
    }
    let end = wall_end(ctx.data(), kind, from, to, ghosts.len());
    // The marker outlines the line's last segment that fits.
    let marked = ghosts.iter().rev().find(|g| g.error.is_none()).map_or((end, OrderMark::Build), |g| (g.pos, OrderMark::Place { kind, facing: g.facing }));
    Placed::Ready { command: PlayerCommand::new(me, builders, CommandKind::PlaceWall { kind, from, to }), end, marked }
}

/// What a targeting click landed on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Clicked {
    /// The hovered entity (one the local team can see).
    hovered: Option<Entity>,
    /// `hovered` when it can be attacked (it has `Health`).
    mortal: Option<Entity>,
    /// `mortal` when it belongs to a player hostile to the local one.
    foe: Option<Entity>,
}

impl Clicked {
    fn new(me: PlayerId, hovered: Option<Entity>, mortals: &Query<Option<&Owner>, (With<Health>, Without<Dead>)>, players: &Players) -> Clicked {
        let owner = hovered.and_then(|t| mortals.get(t).ok());
        let mortal = hovered.filter(|_| owner.is_some());
        let foe = mortal.filter(|_| owner.flatten().is_some_and(|o| players.hostile(me, o.0)));
        Clicked { hovered, mortal, foe }
    }
}

/// The command a targeting click issues, with its click marker (unqueued).
/// `Ok(None)`: nothing selected can do it (leave the mode);
/// `Err`: the click needs a target it did not hit (stay in the mode).
///
/// Attack-move on an enemy attacks that enemy (walls and gates included,
/// which attack-move never picks by itself); the forced Attack attacks
/// anything mortal clicked. Elsewhere both attack-move to the point.
fn target_command(
    me: PlayerId,
    own: &OwnSelection,
    action: TargetAction,
    point: Vec2,
    clicked: Clicked,
) -> Result<Option<(PlayerCommand, ClickMark)>, &'static str> {
    let on = |target| ClickMark::new(OrderMark::of_target(action), target, false);
    let (entities, kind, mark) = match (action, clicked.hovered) {
        (TargetAction::Move, _) => (own.units(), CommandKind::Move(point), on(None)),
        (TargetAction::AttackMove | TargetAction::Attack, _) => {
            let target = if action == TargetAction::Attack { clicked.mortal } else { clicked.foe };
            let mark = target.map_or(on(None), |t| ClickMark::new(OrderMark::Attack, Some(t), false));
            (own.units(), target.map_or(CommandKind::AttackMove(point), CommandKind::Attack), mark)
        }
        (TargetAction::Patrol, _) => (own.units(), CommandKind::Patrol(point), on(None)),
        (TargetAction::Rally, target) => (own.buildings(), CommandKind::SetRally { point, target }, on(target)),
        (TargetAction::Gather, Some(t)) => (own.units(), CommandKind::Gather(t), on(Some(t))),
        (TargetAction::Repair, Some(t)) => (own.units(), CommandKind::Repair(t), on(Some(t))),
        (TargetAction::Gather, None) => return Err("Choose a resource to gather"),
        (TargetAction::Repair, None) => return Err("Choose something to repair"),
    };
    if entities.is_empty() {
        return Ok(None);
    }
    Ok(Some((PlayerCommand::new(me, entities, kind), mark)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_marks_mirror_how_the_sim_resolves_a_right_click() {
        let workers = Crew { units: true, gathers: true, builds: true, repairs: true, cargo: 0 };
        let soldiers = Crew { units: true, ..Crew::default() };
        let halls = Crew::default();
        let own = Pointed { own: true, allied: true, mortal: true, ..Pointed::default() };
        let foe = Pointed { hostile: true, mortal: true, unit: true, ..Pointed::default() };
        let node = Pointed { node: true, ..Pointed::default() };
        let claimed_node = Pointed { claim: true, ..node };
        let foundation = Pointed { building: true, foundation: true, ..own };
        let allied_foundation = Pointed { own: false, ..foundation };
        let field = Pointed { building: true, field: true, ..own };
        let damaged_hall = Pointed { building: true, damaged: true, ..own };
        let hall = Pointed { building: true, ..own };
        let allied_hall = Pointed { own: false, ..hall };
        // A town centre taking food and wood; a damaged one.
        let drop_site = Pointed { drop_mask: 0b11, ..hall };
        let damaged_drop_site = Pointed { damaged: true, ..drop_site };
        let allied_drop_site = Pointed { own: false, ..drop_site };
        let carrying = Crew { cargo: 0b10, ..workers };
        let carrying_stone = Crew { cargo: 0b100, ..workers };
        let machine = Pointed { unit: true, mechanical: true, damaged: true, ..own };
        let friend = Pointed { unit: true, ..own };
        let cases = [
            (workers, None, (OrderMark::Move, false)),
            (workers, Some(foe), (OrderMark::Attack, true)),
            (soldiers, Some(foe), (OrderMark::Attack, true)),
            (workers, Some(node), (OrderMark::Gather, true)),
            (soldiers, Some(node), (OrderMark::Move, false)),
            (workers, Some(claimed_node), (OrderMark::Move, false)),
            (workers, Some(foundation), (OrderMark::Build, true)),
            (workers, Some(allied_foundation), (OrderMark::Build, true)),
            // A plain move onto an allied building is marked round it
            // (a point marker would be under its roof).
            (soldiers, Some(foundation), (OrderMark::Move, true)),
            (workers, Some(field), (OrderMark::Gather, true)),
            (workers, Some(damaged_hall), (OrderMark::Repair, true)),
            (soldiers, Some(damaged_hall), (OrderMark::Move, true)),
            (workers, Some(hall), (OrderMark::Move, true)),
            (soldiers, Some(hall), (OrderMark::Move, true)),
            (soldiers, Some(allied_hall), (OrderMark::Move, true)),
            (carrying, Some(drop_site), (OrderMark::Gather, true)),
            (workers, Some(drop_site), (OrderMark::Move, true)),
            (carrying_stone, Some(drop_site), (OrderMark::Move, true)),
            (carrying, Some(damaged_drop_site), (OrderMark::Repair, true)),
            (carrying, Some(allied_drop_site), (OrderMark::Move, true)),
            (workers, Some(machine), (OrderMark::Repair, true)),
            (soldiers, Some(machine), (OrderMark::Move, true)),
            (soldiers, Some(friend), (OrderMark::Move, true)),
            (halls, None, (OrderMark::Rally, false)),
            (halls, Some(foe), (OrderMark::Rally, true)),
        ];
        for (crew, pointed, want) in cases {
            assert_eq!(smart_mark(crew, pointed), want, "{crew:?} on {pointed:?}");
        }
    }
}
