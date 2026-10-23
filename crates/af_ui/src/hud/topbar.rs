//! Top bar (resource chips, population, age, clock, speed / PAUSED, the
//! spectator's player switcher) and the idle-worker button under it.

use std::fmt::Write as _;

use af_core::prelude::*;
use bevy::prelude::*;

use super::{HudSet, HudView, SpectatorChoice};
use crate::format;
use crate::skin::SkinPanel;
use crate::theme::{self, rgb, set_color, set_display, set_text, text, Tip, FONT_BODY, FONT_SMALL, TEXT, TEXT_DIM};

/// Width of the spectator's "viewing" name, and the characters of the
/// default (monospaced) font at `FONT_BODY` that fit in it.
const VIEW_NAME_WIDTH: f32 = 384.0;
const VIEW_NAME_CHARS: usize = (VIEW_NAME_WIDTH / (FONT_BODY * theme::GLYPH_ADVANCE)) as usize;

/// A text in the top bar and what it shows.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum TopLabel {
    Resource(ResourceId),
    Pop,
    Age,
    Clock,
    Speed,
    ViewName,
    IdleWorkers,
}

/// The spectator's player switcher (hidden when playing).
#[derive(Component)]
struct Switcher;

/// The dim name in front of a top bar value ("Food", "Pop"), dropped when
/// the window is too narrow for the bar to hold them.
#[derive(Component)]
struct ChipName;

/// Narrowest window (logical px) whose top bar holds the resource names
/// next to the values: about 1410 px of bar for a player, and 590 more
/// with the spectator's switcher.
const NAMES_MIN_WIDTH: f32 = 1440.0;
const NAMES_MIN_WIDTH_SPECTATING: f32 = 2010.0;

#[derive(Component, Clone, Copy)]
struct SwitchStep(i32);

#[derive(Component)]
struct IdleButton;

/// The local player's idle workers, in entity order, and which one the
/// button selects next.
#[derive(Resource, Default)]
pub(crate) struct IdleWorkers {
    pub list: Vec<Entity>,
    next: usize,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<IdleWorkers>().add_systems(
        Update,
        (track_idle_workers, (switch_view, idle_button, update_top_bar, fit_chip_names).after(track_idle_workers)).in_set(HudSet::Panels),
    );
}

/// Whether a window `width` px wide has room for the chip names.
fn names_fit(width: f32, spectating: bool) -> bool {
    width >= if spectating { NAMES_MIN_WIDTH_SPECTATING } else { NAMES_MIN_WIDTH }
}

/// Show the chip names only when the window is wide enough for them.
fn fit_chip_names(view: Res<HudView>, window: Query<&Window, With<bevy::window::PrimaryWindow>>, mut names: Query<&mut Node, With<ChipName>>) {
    // Without a window (tests) the names show.
    let wide = window.single().ok().is_none_or(|w| names_fit(w.width(), view.spectating));
    for mut node in &mut names {
        set_display(&mut node, wide);
    }
}

pub(super) fn spawn(root: &mut ChildSpawnerCommands, data: &GameData) {
    root.spawn((
        theme::panel_node(Node {
            position_type: PositionType::Absolute,
            top: px(0),
            left: px(0),
            right: px(0),
            height: px(36),
            padding: UiRect::axes(px(12), px(4)),
            column_gap: px(16),
            align_items: AlignItems::Center,
            border_radius: BorderRadius::ZERO,
            border: UiRect::bottom(px(1)),
            ..default()
        }),
        theme::panel_colors(),
        SkinPanel::Bar,
    ))
    .with_children(|bar| {
        for r in data.resource_ids() {
            let def = data.resource(r);
            bar.spawn((chip(), Tip(format!("{}: {}", def.name, def.description)))).with_children(|c| {
                c.spawn((
                    Node { width: px(14), height: px(14), border: UiRect::all(px(1)), ..default() },
                    BackgroundColor(rgb(def.color)),
                    BorderColor::all(Color::BLACK),
                ));
                // The name, so nobody has to learn the colours first.
                c.spawn((text(def.name.clone(), FONT_SMALL, TEXT_DIM), theme::text_shadow(), ChipName));
                c.spawn((text("0", FONT_BODY, TEXT), TopLabel::Resource(r), Node { min_width: px(36), ..default() }));
            });
        }
        bar.spawn((chip(), Tip("Population: used / capacity. Build houses for more.".into()))).with_children(|c| {
            c.spawn((text("Pop", FONT_SMALL, TEXT_DIM), theme::text_shadow(), ChipName));
            c.spawn((text("0/0", FONT_BODY, TEXT), theme::text_shadow(), TopLabel::Pop));
        });
        bar.spawn(Node { flex_grow: 1.0, ..default() });
        bar.spawn((Node { column_gap: px(6), align_items: AlignItems::Center, display: Display::None, ..default() }, Switcher)).with_children(|s| {
            s.spawn((text("Viewing (Tab)", FONT_SMALL, TEXT_DIM), theme::text_shadow()));
            s.spawn((theme::button(Node { padding: UiRect::axes(px(8), px(1)), ..default() }, theme::BUTTON), SwitchStep(-1)))
                .with_child(text("<", FONT_BODY, TEXT));
            // One line of fixed width, so a long name never wraps the bar
            // or shifts the arrows (it is cut short instead).
            s.spawn((
                text("", FONT_BODY, TEXT),
                // In the player's colour: the shadow keeps it readable on the planks.
                theme::text_shadow(),
                TextLayout::no_wrap(),
                TopLabel::ViewName,
                Node { width: px(VIEW_NAME_WIDTH), overflow: Overflow::clip(), justify_content: JustifyContent::Center, ..default() },
            ));
            s.spawn((theme::button(Node { padding: UiRect::axes(px(8), px(1)), ..default() }, theme::BUTTON), SwitchStep(1)))
                .with_child(text(">", FONT_BODY, TEXT));
        });
        bar.spawn((text("", FONT_BODY, theme::HIGHLIGHT), TopLabel::Age));
        bar.spawn((text("00:00", FONT_BODY, TEXT), TopLabel::Clock));
        bar.spawn((text("x1", FONT_BODY, TEXT), theme::text_shadow(), TopLabel::Speed, Node { min_width: px(64), ..default() }));
    });
    root.spawn((
        theme::button(Node { position_type: PositionType::Absolute, top: px(42), left: px(8), ..default() }, theme::BUTTON),
        IdleButton,
        Tip("Select the next idle worker".into()),
    ))
    .with_child((text("Idle workers: 0", FONT_SMALL, TEXT), TopLabel::IdleWorkers));
}

/// A hoverable row for a top bar value.
fn chip() -> (Node, Interaction) {
    (Node { column_gap: px(5), align_items: AlignItems::Center, ..default() }, Interaction::default())
}

type IdleQuery<'w, 's> = Query<'w, 's, (Entity, &'static Owner), (With<Idle>, With<Gatherer>, With<Unit>, Without<Dead>)>;

fn track_idle_workers(local: Res<LocalPlayer>, idle: IdleQuery, mut workers: ResMut<IdleWorkers>) {
    workers.list.clear();
    if let Some(me) = local.0 {
        workers.list.extend(idle.iter().filter(|(_, o)| o.0 == me).map(|(e, _)| e));
        workers.list.sort_unstable();
    }
}

/// Selects the next idle worker and centres the camera on it.
fn idle_button(
    mut commands: Commands,
    pressed: Query<&Interaction, (With<IdleButton>, Changed<Interaction>)>,
    mut workers: ResMut<IdleWorkers>,
    selected: Query<Entity, With<Selected>>,
    positions: Query<&Position>,
    mut rig: ResMut<CameraRig>,
) {
    if !pressed.iter().any(|i| *i == Interaction::Pressed) || workers.list.is_empty() {
        return;
    }
    let pick = workers.list[workers.next % workers.list.len()];
    workers.next = workers.next.wrapping_add(1);
    for e in &selected {
        if e != pick {
            commands.entity(e).try_remove::<Selected>();
        }
    }
    commands.entity(pick).try_insert(Selected);
    if let Ok(p) = positions.get(pick) {
        rig.focus = p.0;
    }
}

/// Spectators pick whose resources the bar shows (buttons or Tab).
fn switch_view(
    view: Res<HudView>,
    players: Option<Res<Players>>,
    keys: Res<ButtonInput<KeyCode>>,
    steps: Query<(&Interaction, &SwitchStep), Changed<Interaction>>,
    mut choice: ResMut<SpectatorChoice>,
) {
    let Some(players) = players else { return };
    let n = players.list.len();
    if !view.spectating || n == 0 {
        return;
    }
    let mut step: i32 = steps.iter().filter(|(i, _)| **i == Interaction::Pressed).map(|(_, s)| s.0).sum();
    if keys.just_pressed(KeyCode::Tab) {
        step += if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) { -1 } else { 1 };
    }
    if step != 0 {
        choice.0 = (choice.0 as i64 + step as i64).rem_euclid(n as i64) as usize;
    }
}

#[allow(clippy::too_many_arguments)]
fn update_top_bar(
    view: Res<HudView>,
    players: Option<Res<Players>>,
    data: Res<Data>,
    clock: Res<SimClock>,
    time: Res<Time<Virtual>>,
    workers: Res<IdleWorkers>,
    mut labels: Query<(&TopLabel, &mut Text, &mut TextColor)>,
    mut switcher: Query<&mut Node, (With<Switcher>, Without<IdleButton>)>,
    mut idle: Query<&mut Node, (With<IdleButton>, Without<Switcher>)>,
    mut s: Local<String>,
) {
    for mut node in &mut switcher {
        set_display(&mut node, view.spectating);
    }
    for mut node in &mut idle {
        set_display(&mut node, !view.spectating);
    }
    let player = players.as_ref().zip(view.player).and_then(|(ps, p)| ps.try_get(p));
    for (label, mut t, mut color) in &mut labels {
        s.clear();
        let mut c = TEXT;
        match *label {
            TopLabel::Resource(r) => {
                let _ = write!(s, "{}", player.map_or(0.0, |p| p.resources.get(r).max(0.0).floor()));
            }
            TopLabel::Pop => {
                if let Some(p) = player {
                    let _ = write!(s, "{}/{}", p.pop_used, p.pop_cap);
                    if p.pop_used >= p.pop_cap {
                        c = theme::TEXT_BAD;
                    }
                }
            }
            TopLabel::Age => {
                if let Some(p) = player {
                    s.push_str(&data.age(p.age).name);
                }
            }
            TopLabel::Clock => s.push_str(&format::clock(clock.elapsed)),
            TopLabel::Speed => {
                s.push_str(&format::speed(time.relative_speed(), time.is_paused()));
                if time.is_paused() {
                    c = theme::TEXT_BAD;
                }
            }
            TopLabel::ViewName => {
                if let Some(p) = player {
                    s.push_str(&format::elide(&p.name, VIEW_NAME_CHARS));
                    c = theme::player_text(p.color);
                }
            }
            TopLabel::IdleWorkers => {
                let _ = write!(s, "Idle workers: {}", workers.list.len());
                if workers.list.is_empty() {
                    c = TEXT_DIM;
                }
            }
        }
        set_text(&mut t, &s);
        set_color(&mut color, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_make_way_for_the_spectator_switcher() {
        assert!(names_fit(1600.0, false), "the default window shows them");
        assert!(!names_fit(1280.0, false));
        assert!(!names_fit(1600.0, true), "a spectator's bar needs the room for the switcher");
        assert!(names_fit(2560.0, true));
    }
}
