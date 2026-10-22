//! The unit paths toggle under the Menu button: shows whether the selected
//! units' routes are drawn ([`ShowPaths`], also P) and flips it.

use af_core::ext::input::ShowPaths;
use bevy::prelude::*;

use super::{HudSet, HudView};
use crate::theme::{self, set_color, set_display, set_text, text, Tip, FONT_SMALL};

#[derive(Component)]
struct PathsButton;

#[derive(Component)]
struct PathsLabel;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, (toggle_paths, show_paths_state).chain().in_set(HudSet::Panels));
}

pub(super) fn spawn(root: &mut ChildSpawnerCommands) {
    root.spawn((
        theme::button(Node { position_type: PositionType::Absolute, top: px(76), right: px(8), ..default() }, theme::BUTTON),
        PathsButton,
        Tip("Show where the selected units are going and the orders they have queued (P; hold Shift to look). A selected patrol always shows its route.".into()),
    ))
    .with_child((text(label(false), FONT_SMALL, theme::TEXT_DIM), PathsLabel));
}

/// The button's text for the toggle being on or off.
fn label(on: bool) -> &'static str {
    if on { "Paths: on (P)" } else { "Paths: off (P)" }
}

/// A click flips the toggle (af_input owns the resource; without it the
/// button does nothing).
fn toggle_paths(pressed: Query<&Interaction, (With<PathsButton>, Changed<Interaction>)>, paths: Option<ResMut<ShowPaths>>) {
    if let Some(mut paths) = paths
        && pressed.iter().any(|i| *i == Interaction::Pressed)
    {
        paths.on = !paths.on;
    }
}

/// The label says whether paths are on; spectators command nothing, so
/// they get no button.
fn show_paths_state(
    view: Res<HudView>,
    paths: Option<Res<ShowPaths>>,
    mut button: Query<&mut Node, With<PathsButton>>,
    mut labels: Query<(&mut Text, &mut TextColor), With<PathsLabel>>,
) {
    for mut node in &mut button {
        set_display(&mut node, !view.spectating);
    }
    let on = paths.is_some_and(|p| p.on);
    for (mut t, mut color) in &mut labels {
        set_text(&mut t, label(on));
        set_color(&mut color, if on { theme::HIGHLIGHT } else { theme::TEXT_DIM });
    }
}
