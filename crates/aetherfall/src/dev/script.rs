//! `--script`: scripted play for a windowed run, so a screenshot run can
//! act like a person at the keyboard and mouse. A script is a list of
//! steps, `<sim seconds> <action>`, separated by `;`:
//!
//! | action | does |
//! |---|---|
//! | `key <combo>` | taps a key, with modifiers (`key Z`, `key ctrl+1`, `key shift+R`, `key Esc`) |
//! | `move <x>,<y>` | moves the stand-in mouse (logical window px) |
//! | `click [left\|right] [shift\|ctrl]` | clicks in the world at the stand-in mouse |
//! | `down [left\|right]` / `up [left\|right]` | holds / releases a button (drags) |
//! | `press <label>` | presses the HUD button with that label, as `--press` |
//! | `select <what>` | selects, as `--select` (without moving the camera) |
//! | `camera <spot>[,dist]` | frames the view, as `--camera` |
//! | `defeat <player>` | deletes everything that player owns (to end a match) |
//! | `damage <what> <fraction>` | sets what `select <what>` would pick to that fraction of its hit points (a building to repair) |
//! | `shot <path>` | takes a screenshot (the run exits after the last one) |
//!
//! Keys and clicks act on the frame after the one that taps them is due,
//! one per frame, so a chord and the key after it never land together.
//! The stand-in mouse only points at the world: HUD buttons are reached
//! with `press` (and their hotkeys). Times are sim seconds, so a paused
//! game holds the script (shots still due are taken).

use std::path::PathBuf;

use af_core::prelude::*;
use bevy::prelude::*;

use super::{CameraArg, Pick, PickWorld, VirtualCursor};

/// One scripted action.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Step {
    Key { mods: Vec<KeyCode>, key: KeyCode },
    Move(Vec2),
    Click { button: MouseButton, mods: Vec<KeyCode> },
    Down(MouseButton),
    Up(MouseButton),
    Press(String),
    Select(Pick),
    Camera(CameraArg),
    Defeat(u8),
    /// Hit points of the picked entities set to this fraction of their most.
    Damage(Pick, f32),
    Shot(PathBuf),
}

impl Step {
    /// Applied by [`script_input`] (PreUpdate) rather than [`script_world`] (Update).
    fn is_input(&self) -> bool {
        matches!(self, Step::Key { .. } | Step::Move(_) | Step::Click { .. } | Step::Down(_) | Step::Up(_) | Step::Shot(_))
    }
}

/// The script's steps in time order, and how far it has got.
#[derive(Resource, Debug, Default)]
pub(super) struct Script {
    steps: Vec<(f32, Step)>,
    next: usize,
    /// Keys and buttons tapped last frame, released this frame.
    taps: Vec<KeyCode>,
    tap_buttons: Vec<MouseButton>,
}

impl Script {
    pub fn new(steps: Vec<(f32, Step)>) -> Script {
        Script { steps, ..default() }
    }

    /// The step due at `now`, if any.
    fn due(&self, now: f32) -> Option<&Step> {
        self.steps.get(self.next).filter(|(at, _)| now >= *at).map(|(_, s)| s)
    }
}

/// Parse a whole script, in time order (steps at the same time keep their order).
pub(super) fn parse(data: &GameData, script: &str) -> Result<Vec<(f32, Step)>, String> {
    let mut steps = Vec::new();
    for raw in script.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let err = |why: &str| format!("--script step `{raw}`: {why}");
        let (time, rest) = raw.split_once(char::is_whitespace).ok_or_else(|| err("want `<seconds> <action> ...`"))?;
        let at: f32 = time.parse().map_err(|_| err("starts with a time in sim seconds"))?;
        if !at.is_finite() || at < 0.0 {
            return Err(err("the time must be zero or more"));
        }
        let rest = rest.trim();
        let (verb, arg) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(v, a)| (v, a.trim()));
        let step = match verb {
            "key" => {
                let (mods, key) = combo(arg).ok_or_else(|| err("unknown key (try Q, 1, ctrl+1, shift+R, Esc, Space, Period, F1, Delete, Tab, Enter, Home, Up)"))?;
                Step::Key { mods, key }
            }
            "move" => Step::Move(super::pair("--script move", arg).map_err(|e| err(&e))?),
            "click" | "down" | "up" => {
                let mut button = MouseButton::Left;
                let mut mods = Vec::new();
                for word in arg.split_whitespace() {
                    match word {
                        "left" => button = MouseButton::Left,
                        "right" => button = MouseButton::Right,
                        "middle" => button = MouseButton::Middle,
                        other => mods.push(modifier(other).ok_or_else(|| err("want left|right|middle and shift|ctrl|alt"))?),
                    }
                }
                match verb {
                    "click" => Step::Click { button, mods },
                    _ if !mods.is_empty() => return Err(err("hold modifiers with `key`, not with down/up")),
                    "down" => Step::Down(button),
                    _ => Step::Up(button),
                }
            }
            "press" if !arg.is_empty() => Step::Press(arg.to_string()),
            "select" => Step::Select(Pick::parse(data, arg)?),
            "camera" => Step::Camera(CameraArg::parse(arg)?),
            "defeat" => Step::Defeat(super::number("--script defeat", arg)?),
            "damage" => {
                let (what, fraction) = arg.rsplit_once(' ').ok_or_else(|| err("want `damage <what> <fraction>`"))?;
                let fraction: f32 = super::number("--script damage", fraction)?;
                if !(0.0..=1.0).contains(&fraction) || fraction == 0.0 {
                    return Err(err("the fraction of hit points left must be in (0, 1]"));
                }
                Step::Damage(Pick::parse(data, what.trim())?, fraction)
            }
            "shot" if !arg.is_empty() => Step::Shot(PathBuf::from(arg)),
            _ => return Err(err("unknown action (key, move, click, down, up, press, select, camera, defeat, damage, shot)")),
        };
        steps.push((at, step));
    }
    // Stable: steps given for the same time stay in order.
    steps.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(steps)
}

fn modifier(name: &str) -> Option<KeyCode> {
    Some(match name.to_ascii_lowercase().as_str() {
        "shift" => KeyCode::ShiftLeft,
        "ctrl" | "control" => KeyCode::ControlLeft,
        "alt" => KeyCode::AltLeft,
        _ => return None,
    })
}

/// "ctrl+shift+1" -> ([ControlLeft, ShiftLeft], Digit1).
fn combo(s: &str) -> Option<(Vec<KeyCode>, KeyCode)> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    // A lone "+" (or "shift++") is the plus key itself.
    let (mods, key) = match parts.as_slice() {
        [.., "", ""] => (&parts[..parts.len() - 2], "+"),
        [mods @ .., key] => (mods, *key),
        [] => return None,
    };
    let mods = mods.iter().map(|m| modifier(m)).collect::<Option<Vec<_>>>()?;
    Some((mods, key_code(key)?))
}

const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
];

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

fn key_code(name: &str) -> Option<KeyCode> {
    let lower = name.to_ascii_lowercase();
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return match c {
            'a'..='z' => Some(LETTERS[c as usize - 'a' as usize]),
            '0'..='9' => Some(DIGITS[c as usize - '0' as usize]),
            '.' => Some(KeyCode::Period),
            '+' | '=' => Some(KeyCode::Equal),
            '-' => Some(KeyCode::Minus),
            _ => None,
        };
    }
    Some(match lower.as_str() {
        "esc" | "escape" => KeyCode::Escape,
        "space" => KeyCode::Space,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "delete" | "del" => KeyCode::Delete,
        "backspace" => KeyCode::Backspace,
        "period" => KeyCode::Period,
        "home" => KeyCode::Home,
        "pause" => KeyCode::Pause,
        "up" => KeyCode::ArrowUp,
        "down" => KeyCode::ArrowDown,
        "left" => KeyCode::ArrowLeft,
        "right" => KeyCode::ArrowRight,
        "f1" => KeyCode::F1,
        "f2" => KeyCode::F2,
        "f3" => KeyCode::F3,
        "f4" => KeyCode::F4,
        _ => return None,
    })
}

/// PreUpdate, after Bevy's input systems: release last frame's taps, then
/// apply the due input steps (any number of mouse moves, then at most one
/// key or button) so the frame's `Update` sees them as a real press.
pub(super) fn script_input(
    mut script: ResMut<Script>,
    clock: Res<SimClock>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut cursor: ResMut<VirtualCursor>,
) {
    let s = &mut *script;
    for k in s.taps.drain(..) {
        keys.release(k);
    }
    for b in s.tap_buttons.drain(..) {
        mouse.release(b);
    }
    while let Some(step) = s.due(clock.elapsed).filter(|s| s.is_input()).cloned() {
        s.next += 1;
        match step {
            Step::Move(p) => {
                cursor.0 = Some(p);
                continue;
            }
            // Taken by the shot plan.
            Step::Shot(_) => continue,
            Step::Key { mods, key } => {
                for k in mods.iter().chain([&key]) {
                    keys.press(*k);
                    s.taps.push(*k);
                }
            }
            Step::Click { button, mods } => {
                for k in &mods {
                    keys.press(*k);
                    s.taps.push(*k);
                }
                mouse.press(button);
                s.tap_buttons.push(button);
            }
            Step::Down(b) => mouse.press(b),
            Step::Up(b) => mouse.release(b),
            _ => unreachable!("only input steps get here"),
        }
        break;
    }
}

/// Update, between af_input and af_ui: the due steps that act on the game
/// (presses, selections, camera, defeats, damage); a press ends the frame's
/// turn so the HUD sees one press at a time.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn script_world(
    mut commands: Commands,
    mut script: ResMut<Script>,
    clock: Res<SimClock>,
    local: Res<LocalPlayer>,
    map: Res<MapInfo>,
    players: Res<Players>,
    picks: PickWorld,
    mut buttons: super::Buttons,
    mut rig: ResMut<CameraRig>,
    mut inbox: ResMut<CommandInbox>,
    owned: Query<(Entity, &Owner), (Or<(With<Unit>, With<Building>)>, Without<Dead>)>,
    mut health: Query<&mut Health, Without<Dead>>,
) {
    while let Some(step) = script.due(clock.elapsed).filter(|s| !s.is_input()).cloned() {
        match step {
            Step::Press(label) => {
                // Retried each frame until the button is shown.
                if !buttons.press(&label) {
                    return;
                }
                script.next += 1;
                info!("script: pressed {label:?}");
                return;
            }
            Step::Select(pick) => {
                let me = local.0.unwrap_or(PlayerId(0));
                let n = picks.select(&mut commands, pick, me).map_or(0, |(n, _)| n);
                info!("script: selected {n} ({pick:?})");
            }
            Step::Camera(arg) => arg.apply(&map, &players, local.0, &mut rig),
            Step::Defeat(p) => {
                let player = PlayerId(p);
                let entities: Vec<Entity> = owned.iter().filter(|(_, o)| o.0 == player).map(|(e, _)| e).collect();
                info!("script: defeating player {p} ({} entities)", entities.len());
                inbox.push(PlayerCommand::new(player, entities, CommandKind::Delete));
            }
            Step::Damage(pick, fraction) => {
                let picked = super::pick_entities(pick, &picks.candidates(local.0.unwrap_or(PlayerId(0))));
                for e in &picked {
                    if let Ok(mut h) = health.get_mut(*e) {
                        h.hp = h.max * fraction;
                    }
                }
                info!("script: damaged {} ({pick:?}) to {:.0}%", picked.len(), fraction * 100.0);
            }
            _ => unreachable!("only world steps get here"),
        }
        script.next += 1;
    }
}

/// The screenshots a script takes.
pub(super) fn shots(steps: &[(f32, Step)]) -> Vec<(f32, PathBuf)> {
    steps.iter().filter_map(|(at, s)| if let Step::Shot(p) = s { Some((*at, p.clone())) } else { None }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::test_data;

    #[test]
    fn scripts_parse_in_time_order() {
        let data = test_data();
        let steps = parse(&data, "2 key ctrl+1; 1.5 select workers; 1.5 press Economy ;3 move 800,450; 3.1 click right shift; 4 shot a.png").unwrap();
        let times: Vec<f32> = steps.iter().map(|(t, _)| *t).collect();
        assert_eq!(times, vec![1.5, 1.5, 2.0, 3.0, 3.1, 4.0]);
        assert_eq!(steps[0].1, Step::Select(Pick::Workers));
        assert_eq!(steps[1].1, Step::Press("Economy".into()), "same time keeps the given order");
        assert_eq!(steps[2].1, Step::Key { mods: vec![KeyCode::ControlLeft], key: KeyCode::Digit1 });
        assert_eq!(steps[3].1, Step::Move(Vec2::new(800.0, 450.0)));
        assert_eq!(steps[4].1, Step::Click { button: MouseButton::Right, mods: vec![KeyCode::ShiftLeft] });
        assert_eq!(shots(&steps), vec![(4.0, PathBuf::from("a.png"))]);
        let damage = parse(&data, "1 damage building:house 0.4").unwrap();
        assert_eq!(damage[0].1, Step::Damage(Pick::Building(data.building_id("house").unwrap()), 0.4));
    }

    #[test]
    fn keys_and_mistakes() {
        let data = test_data();
        let key = |s: &str| match parse(&data, &format!("0 key {s}")).map(|v| v[0].1.clone()) {
            Ok(Step::Key { mods, key }) => Some((mods, key)),
            _ => None,
        };
        assert_eq!(key("Esc"), Some((vec![], KeyCode::Escape)));
        assert_eq!(key("shift+r"), Some((vec![KeyCode::ShiftLeft], KeyCode::KeyR)));
        assert_eq!(key("+"), Some((vec![], KeyCode::Equal)));
        assert_eq!(key("."), Some((vec![], KeyCode::Period)));
        assert_eq!(key("F1"), Some((vec![], KeyCode::F1)));
        assert_eq!(key("hyper+q"), None);
        for bad in ["key", "x key Q", "-1 key Q", "1 dance", "1 click sideways", "1 down shift", "1 select everything", "1 press", "1 move 3", "1 damage hall", "1 damage hall 0", "1 damage hall 1.5", "1 damage nothing 0.5"] {
            assert!(parse(&data, bad).is_err(), "{bad}");
        }
        assert!(parse(&data, " ; ;").unwrap().is_empty());
    }

    #[test]
    fn taps_last_one_frame_and_moves_do_not_wait() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<SimClock>()
            .insert_resource(VirtualCursor(None))
            .insert_resource(Script::new(vec![
                (0.0, Step::Move(Vec2::new(5.0, 6.0))),
                (0.0, Step::Key { mods: vec![KeyCode::ControlLeft], key: KeyCode::Digit1 }),
                (0.0, Step::Click { button: MouseButton::Left, mods: vec![] }),
            ]))
            .add_systems(Update, script_input);
        app.update();
        assert_eq!(app.world().resource::<VirtualCursor>().0, Some(Vec2::new(5.0, 6.0)));
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.just_pressed(KeyCode::Digit1) && keys.pressed(KeyCode::ControlLeft));
        assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left), "one tap per frame");
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(!keys.pressed(KeyCode::Digit1) && keys.just_released(KeyCode::ControlLeft), "released the next frame");
        assert!(app.world().resource::<ButtonInput<MouseButton>>().just_pressed(MouseButton::Left));
    }
}
