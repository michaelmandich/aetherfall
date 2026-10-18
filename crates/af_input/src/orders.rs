//! Turning clicks into `PlayerCommand`s: right-click `Smart`, and the click
//! that completes a placement or targeting `InputMode`.

use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::placement::{plan_building, plan_wall, wall_end, PlaceCtx, PlacementWorld};

/// Click marker colours (af_render draws them).
pub(crate) const PING_MOVE: [f32; 3] = [0.3, 1.0, 0.35];
pub(crate) const PING_ATTACK: [f32; 3] = [1.0, 0.25, 0.2];
/// Seconds a click marker lives.
const PING_SECS: f32 = 0.7;

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

    pub fn ping(&mut self, pos: Vec2, color: [f32; 3]) {
        self.commands.spawn(ClickPing { pos, time_left: PING_SECS, color });
    }
}

/// The local player's own selected, living units and buildings.
#[derive(SystemParam)]
pub(crate) struct OwnSelection<'w, 's> {
    local: Res<'w, LocalPlayer>,
    data: Res<'w, Data>,
    units: Query<'w, 's, (Entity, &'static Owner, &'static Unit), (With<Selected>, Without<Dead>)>,
    buildings: Query<'w, 's, (Entity, &'static Owner), (With<Selected>, With<Building>, Without<Dead>)>,
}

impl OwnSelection<'_, '_> {
    /// The commanding player; `None` while spectating.
    pub fn player(&self) -> Option<PlayerId> {
        self.local.0
    }

    fn units_where(&self, keep: impl Fn(&Unit) -> bool) -> Vec<Entity> {
        let Some(me) = self.local.0 else { return Vec::new() };
        let mut out: Vec<Entity> = self.units.iter().filter(|(_, o, u)| o.0 == me && keep(u)).map(|(e, ..)| e).collect();
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
    targets: Query<(&Position, Option<&Owner>)>,
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
    let Some(point) = cursor.ground.or(target_info.map(|(p, _)| p.0)) else { return };
    let hostile = target_info.and_then(|(_, o)| o).is_some_and(|o| players.hostile(me, o.0));
    issuer.send(PlayerCommand::new(me, entities, CommandKind::Smart { point, target }).queued(shift_held(&keys)));
    issuer.ping(point, if hostile { PING_ATTACK } else { PING_MOVE });
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
            Ok(Some((command, color))) => {
                issuer.send(command.queued(shift));
                issuer.ping(ground, color);
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
    /// Ready to send; the building or line ends at the point.
    Ready(PlayerCommand, Vec2),
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
            Placed::Ready(command, end) => {
                issuer.send(command.queued(shift));
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
    Placed::Ready(PlayerCommand::new(me, builders, command), ghost.pos)
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
    Placed::Ready(PlayerCommand::new(me, builders, CommandKind::PlaceWall { kind, from, to }), end)
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

/// The command a targeting click issues, with its marker colour.
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
) -> Result<Option<(PlayerCommand, [f32; 3])>, &'static str> {
    let (entities, kind, color) = match (action, clicked.hovered) {
        (TargetAction::Move, _) => (own.units(), CommandKind::Move(point), PING_MOVE),
        (TargetAction::AttackMove | TargetAction::Attack, _) => {
            let target = if action == TargetAction::Attack { clicked.mortal } else { clicked.foe };
            (own.units(), target.map_or(CommandKind::AttackMove(point), CommandKind::Attack), PING_ATTACK)
        }
        (TargetAction::Patrol, _) => (own.units(), CommandKind::Patrol(point), PING_MOVE),
        (TargetAction::Rally, target) => (own.buildings(), CommandKind::SetRally { point, target }, PING_MOVE),
        (TargetAction::Gather, Some(t)) => (own.units(), CommandKind::Gather(t), PING_MOVE),
        (TargetAction::Repair, Some(t)) => (own.units(), CommandKind::Repair(t), PING_MOVE),
        (TargetAction::Gather, None) => return Err("Choose a resource to gather"),
        (TargetAction::Repair, None) => return Err("Choose something to repair"),
    };
    if entities.is_empty() {
        return Ok(None);
    }
    Ok(Some((PlayerCommand::new(me, entities, kind), color)))
}
