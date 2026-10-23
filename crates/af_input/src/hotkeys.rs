//! The keys af_input owns (the other letters are af_ui's command-card
//! hotkeys): Esc, R (placement), H (main hall, not on the card), digits
//! (control groups), Period/F1 (idle worker), F2 (army), Home/Space (centre), Delete,
//! Pause/F3 and +/- (game speed), P (unit paths; Shift shows them while held).

use std::f32::consts::TAU;

use af_core::ext::input::{ControlGroups, ShowPaths};
use af_core::ext::ui::CardPage;
use af_core::prelude::*;
use bevy::prelude::*;

use crate::camera::focus_on;
use crate::orders::{shift_held, Issuer, OwnSelection};
use crate::selection::{Gesture, Selectables};

/// Rotation per R press while placing (shift+R: fine steps).
const ROTATE_STEP: f32 = TAU / 8.0;
const ROTATE_FINE_STEP: f32 = TAU / 24.0;
/// Pressing the same group digit twice within this many seconds also
/// centres the camera on the group.
const DOUBLE_TAP_SECS: f64 = 0.4;
/// Game speeds the +/- keys step through.
const SPEEDS: [f32; 9] = [0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0];

const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

/// The digit (0-9) pressed this frame, if any.
fn pressed_digit(keys: &ButtonInput<KeyCode>) -> Option<usize> {
    DIGITS.iter().position(|k| keys.just_pressed(*k))
}

fn ctrl_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
}

fn ctrl_or_alt_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::AltLeft, KeyCode::AltRight])
}

/// The next game speed up (or down) the ladder from `current`.
pub(crate) fn step_speed(current: f32, up: bool) -> f32 {
    const EPS: f32 = 1e-3;
    if up {
        SPEEDS.iter().copied().find(|s| *s > current + EPS).unwrap_or(SPEEDS[SPEEDS.len() - 1])
    } else {
        SPEEDS.iter().copied().rev().find(|s| *s < current - EPS).unwrap_or(SPEEDS[0])
    }
}

/// The entity after `last` in the sorted list `candidates`, wrapping round.
pub(crate) fn next_in_cycle(candidates: &[Entity], last: Option<Entity>) -> Option<Entity> {
    last.and_then(|l| candidates.iter().copied().find(|e| *e > l)).or(candidates.first().copied())
}

/// Mean of `points`, or `None` for none.
pub(crate) fn centroid(points: impl IntoIterator<Item = Vec2>) -> Option<Vec2> {
    let (sum, n) = points.into_iter().fold((Vec2::ZERO, 0u32), |(s, n), p| (s + p, n + 1));
    (n > 0).then(|| sum / n as f32)
}

/// Esc: drop a drag in progress, else leave the current mode, else step
/// back out of an open command card page (af_ui's [`CardPage`]), else
/// clear the selection. One step per press, so a player backs out of
/// placing a house, then the build page, then the selection.
pub(crate) fn escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<InputMode>,
    mut gesture: ResMut<Gesture>,
    mut drag: ResMut<DragSelect>,
    mut page: Option<ResMut<CardPage>>,
    sel: Selectables,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if gesture.cancel() {
        drag.0 = None;
    } else if *mode != InputMode::Normal {
        *mode = InputMode::Normal;
    } else if let Some(page) = page.as_mut().filter(|p| p.open) {
        page.close = true;
    } else {
        sel.select(&mut commands, &[]);
    }
}

/// F2: select the whole army (every own unit that is not a worker).
pub(crate) fn select_army(keys: Res<ButtonInput<KeyCode>>, data: Res<Data>, local: Res<LocalPlayer>, units: Query<(Entity, &Owner, &Unit), Without<Dead>>, sel: Selectables, mut commands: Commands) {
    if !keys.just_pressed(KeyCode::F2) {
        return;
    }
    let Some(me) = local.0 else { return };
    let mut army: Vec<Entity> = units.iter().filter(|(_, o, u)| o.0 == me && data.unit(u.kind).gatherer.is_none()).map(|(e, ..)| e).collect();
    if army.is_empty() {
        return;
    }
    army.sort();
    sel.select(&mut commands, &army);
}

/// H: select the main hall (a finished TownCenter-tagged building) and
/// look at it; pressed again, the next hall. Lets a player queue workers
/// without hunting for the town centre.
pub(crate) fn select_hall(
    keys: Res<ButtonInput<KeyCode>>,
    local: Res<LocalPlayer>,
    halls: Query<(Entity, &Owner, &Position, &Stats), (With<Building>, Without<UnderConstruction>, Without<Dead>)>,
    sel: Selectables,
    map: Option<Res<MapInfo>>,
    mut rig: ResMut<CameraRig>,
    mut last: Local<Option<Entity>>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::KeyH) || ctrl_or_alt_held(&keys) {
        return;
    }
    let Some(me) = local.0 else { return };
    let mut own: Vec<(Entity, Vec2)> = halls.iter().filter(|(_, o, _, st)| o.0 == me && st.tags.contains(Tag::TownCenter)).map(|(e, _, p, _)| (e, p.0)).collect();
    own.sort_by_key(|(e, _)| *e);
    let ids: Vec<Entity> = own.iter().map(|(e, _)| *e).collect();
    // Start from the first hall unless one is already the selection.
    let current = sel.current_own();
    let from = last.filter(|l| current == [*l]);
    let Some(next) = next_in_cycle(&ids, from) else { return };
    *last = Some(next);
    sel.select(&mut commands, &[next]);
    if let Some((_, pos)) = own.iter().find(|(e, _)| *e == next) {
        focus_on(&mut rig, *pos, map.as_deref());
    }
}

/// R turns the building being placed by 45 degrees (shift+R: 15 degrees).
pub(crate) fn rotate_placement(keys: Res<ButtonInput<KeyCode>>, mut mode: ResMut<InputMode>) {
    if !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    if let InputMode::Place { kind, facing } = *mode {
        let step = if shift_held(&keys) { ROTATE_FINE_STEP } else { ROTATE_STEP };
        *mode = InputMode::Place { kind, facing: (facing + step).rem_euclid(TAU) };
    }
}

/// Ctrl+digit sets a group, Shift+digit adds to it, digit recalls it, and
/// a quick second press also centres the camera on it. Setting or adding
/// with no own selection (nothing, or a foreign entity shown for info)
/// leaves the group alone rather than wiping it.
pub(crate) fn control_groups(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut groups: ResMut<ControlGroups>,
    sel: Selectables,
    alive: Query<&Position, Without<Dead>>,
    map: Option<Res<MapInfo>>,
    mut rig: ResMut<CameraRig>,
    mut last_tap: Local<Option<(usize, f64)>>,
    mut commands: Commands,
) {
    let Some(digit) = pressed_digit(&keys) else { return };
    let (ctrl, shift) = (ctrl_held(&keys), shift_held(&keys));
    if ctrl || shift {
        let own = sel.current_own();
        if own.is_empty() {
            return;
        }
        if ctrl {
            groups.set(digit, &own);
        } else {
            groups.add(digit, &own);
        }
        return;
    }
    groups.retain_alive(|e| alive.contains(e));
    let members = groups.members(digit).to_vec();
    if members.is_empty() {
        return;
    }
    sel.select(&mut commands, &members);
    let now = time.elapsed_secs_f64();
    if last_tap.is_some_and(|(d, at)| d == digit && now - at <= DOUBLE_TAP_SECS) {
        if let Some(c) = centroid(alive.iter_many(&members).map(|p| p.0)) {
            focus_on(&mut rig, c, map.as_deref());
        }
        *last_tap = None;
    } else {
        *last_tap = Some((digit, now));
    }
}

/// Period or F1: select the next idle worker and look at it.
pub(crate) fn select_idle_worker(
    keys: Res<ButtonInput<KeyCode>>,
    data: Res<Data>,
    local: Res<LocalPlayer>,
    idle: Query<(Entity, &Owner, &Unit, &Position), (With<Idle>, Without<Dead>)>,
    sel: Selectables,
    map: Option<Res<MapInfo>>,
    mut rig: ResMut<CameraRig>,
    mut last: Local<Option<Entity>>,
    mut commands: Commands,
) {
    if !keys.any_just_pressed([KeyCode::Period, KeyCode::F1]) {
        return;
    }
    let Some(me) = local.0 else { return };
    let mut workers: Vec<(Entity, Vec2)> = idle
        .iter()
        .filter(|(_, o, u, _)| o.0 == me && data.unit(u.kind).gatherer.is_some())
        .map(|(e, _, _, p)| (e, p.0))
        .collect();
    workers.sort_by_key(|(e, _)| *e);
    let ids: Vec<Entity> = workers.iter().map(|(e, _)| *e).collect();
    let Some(next) = next_in_cycle(&ids, *last) else { return };
    *last = Some(next);
    sel.select(&mut commands, &[next]);
    if let Some((_, pos)) = workers.iter().find(|(e, _)| *e == next) {
        focus_on(&mut rig, *pos, map.as_deref());
    }
}

/// Home or Space: look at the selection, else at the main hall (a
/// TownCenter-tagged building, finished ones first), else the start.
pub(crate) fn centre_camera(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Query<&Position, (With<Selected>, Without<Dead>)>,
    halls: Query<(Entity, &Owner, &Position, &Stats, Has<UnderConstruction>), (With<Building>, Without<Dead>)>,
    local: Res<LocalPlayer>,
    players: Res<Players>,
    map: Option<Res<MapInfo>>,
    mut rig: ResMut<CameraRig>,
) {
    if !keys.any_just_pressed([KeyCode::Home, KeyCode::Space]) {
        return;
    }
    let main_hall = || {
        let me = local.0?;
        halls
            .iter()
            .filter(|(_, o, _, st, _)| o.0 == me && st.tags.contains(Tag::TownCenter))
            .min_by_key(|(e, _, _, _, building)| (*building, *e))
            .map(|(_, _, p, ..)| p.0)
            .or_else(|| players.try_get(me).map(|p| p.start_pos))
    };
    if let Some(target) = centroid(selected.iter().map(|p| p.0)).or_else(main_hall) {
        focus_on(&mut rig, target, map.as_deref());
    }
}

/// Delete: kill the own selected units and buildings.
pub(crate) fn delete_selected(keys: Res<ButtonInput<KeyCode>>, own: OwnSelection, mut issuer: Issuer) {
    if !keys.just_pressed(KeyCode::Delete) {
        return;
    }
    let Some(me) = own.player() else { return };
    let entities = own.all();
    if !entities.is_empty() {
        issuer.send(PlayerCommand::new(me, entities, CommandKind::Delete));
    }
}

/// P shows or hides the selected units' routes ([`ShowPaths`]) and says
/// which in a notice; while Shift is held (queued orders being given) they
/// show whatever P says. Without a local player (spectating) there are no
/// routes to show, and P does nothing, as the HUD hides the Paths button.
pub(crate) fn toggle_paths(keys: Res<ButtonInput<KeyCode>>, local: Res<LocalPlayer>, mut paths: ResMut<ShowPaths>, mut issuer: Issuer) {
    let held = shift_held(&keys);
    if paths.held != held {
        paths.held = held;
    }
    if local.0.is_some() && keys.just_pressed(KeyCode::KeyP) && !ctrl_or_alt_held(&keys) {
        paths.on = !paths.on;
        issuer.notice(local.0, NoticeKind::Info, if paths.on { "Unit paths shown (P)" } else { "Unit paths hidden (P)" });
    }
}

/// Pause/F3 toggles the simulation clock; +/- (numpad or main row) steps
/// the game speed between 0.5x and 8x.
pub(crate) fn game_speed(keys: Res<ButtonInput<KeyCode>>, local: Res<LocalPlayer>, mut time: ResMut<Time<Virtual>>, mut issuer: Issuer) {
    if keys.any_just_pressed([KeyCode::Pause, KeyCode::F3]) {
        if time.is_paused() {
            time.unpause();
        } else {
            time.pause();
        }
    }
    let up = keys.any_just_pressed([KeyCode::NumpadAdd, KeyCode::Equal]);
    let down = keys.any_just_pressed([KeyCode::NumpadSubtract, KeyCode::Minus]);
    if up != down {
        let speed = step_speed(time.relative_speed(), up);
        if speed != time.relative_speed() {
            time.set_relative_speed(speed);
            issuer.notice(local.0, NoticeKind::Info, format!("Game speed {speed}x"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_ladder_steps_and_saturates() {
        assert_eq!(step_speed(1.0, true), 1.5);
        assert_eq!(step_speed(1.0, false), 0.75);
        assert_eq!(step_speed(8.0, true), 8.0);
        assert_eq!(step_speed(0.5, false), 0.5);
        // Off-ladder speeds snap to the neighbouring step.
        assert_eq!(step_speed(1.2, true), 1.5);
        assert_eq!(step_speed(1.2, false), 1.0);
    }

    #[test]
    fn cycling_wraps_round() {
        let mut world = World::new();
        let mut e: Vec<Entity> = (0..3).map(|_| world.spawn_empty().id()).collect();
        e.sort();
        assert_eq!(next_in_cycle(&e, None), Some(e[0]));
        assert_eq!(next_in_cycle(&e, Some(e[0])), Some(e[1]));
        assert_eq!(next_in_cycle(&e, Some(e[2])), Some(e[0]));
        assert_eq!(next_in_cycle(&[], Some(e[2])), None);
    }

    #[test]
    fn centroid_of_points() {
        assert_eq!(centroid([Vec2::new(0.0, 0.0), Vec2::new(4.0, 2.0)]), Some(Vec2::new(2.0, 1.0)));
        assert_eq!(centroid(std::iter::empty()), None);
    }

    #[test]
    fn control_group_bookkeeping() {
        let mut world = World::new();
        let e: Vec<Entity> = (0..4).map(|_| world.spawn_empty().id()).collect();
        let mut groups = ControlGroups::default();
        groups.set(1, &[e[0], e[1], e[0]]);
        assert_eq!(groups.members(1), &[e[0], e[1]]);
        groups.add(1, &[e[1], e[2]]);
        assert_eq!(groups.members(1), &[e[0], e[1], e[2]]);
        groups.set(0, &[e[3]]);
        groups.retain_alive(|x| x != e[1] && x != e[3]);
        assert_eq!(groups.members(1), &[e[0], e[2]]);
        assert!(groups.members(0).is_empty());
        assert!(groups.members(12).is_empty());
        groups.set(1, &[]);
        assert!(groups.members(1).is_empty());
    }
}
