//! Control group bar above the selection panel: a button per non-empty
//! group (af_input's `ControlGroups`) with its live member count. Click
//! selects the group; shift-click adds it to the selection.

use af_core::ext::input::ControlGroups;
use af_core::prelude::*;
use bevy::prelude::*;

use super::HudSet;
use crate::theme::{self, set_display, set_text, text, Tip, FONT_SMALL, TEXT};

/// Digits in keyboard order.
const DIGITS: [usize; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];
/// Seconds between recounts of living members.
const RECOUNT: f32 = 0.25;

#[derive(Component, Clone, Copy)]
struct GroupButton(usize);

#[derive(Component, Clone, Copy)]
struct GroupButtonLabel(usize);

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, (select_group, refresh_groups).in_set(HudSet::Panels));
}

pub(super) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent.spawn((Node { column_gap: px(4), height: px(26), ..default() }, Pickable::IGNORE)).with_children(|row| {
        for digit in DIGITS {
            row.spawn((
                theme::button(Node { padding: UiRect::axes(px(8), px(1)), display: Display::None, ..default() }, theme::BUTTON),
                GroupButton(digit),
                Tip(format!("Control group {digit}: click to select, shift-click to add")),
            ))
            .with_child((text("", FONT_SMALL, TEXT), GroupButtonLabel(digit)));
        }
    });
}

fn refresh_groups(
    groups: Option<Res<ControlGroups>>,
    alive: Query<(), Without<Dead>>,
    time: Res<Time<Real>>,
    mut since: Local<f32>,
    mut buttons: Query<(&GroupButton, &mut Node)>,
    mut labels: Query<(&GroupButtonLabel, &mut Text)>,
) {
    let Some(groups) = groups else { return };
    *since += time.delta_secs();
    // Members die without the groups changing, so recount now and then.
    if !groups.is_changed() && *since < RECOUNT {
        return;
    }
    *since = 0.0;
    let count = |digit: usize| groups.members(digit).iter().filter(|e| alive.contains(**e)).count();
    for (b, mut node) in &mut buttons {
        set_display(&mut node, count(b.0) > 0);
    }
    for (l, mut t) in &mut labels {
        set_text(&mut t, &format!("{}  x{}", l.0, count(l.0)));
    }
}

fn select_group(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    groups: Option<Res<ControlGroups>>,
    buttons: Query<(&Interaction, &GroupButton), Changed<Interaction>>,
    selected: Query<Entity, With<Selected>>,
    alive: Query<(), Without<Dead>>,
) {
    let Some(groups) = groups else { return };
    let Some(digit) = buttons.iter().find(|(i, _)| **i == Interaction::Pressed).map(|(_, b)| b.0) else { return };
    let members = groups.members(digit);
    if !keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        for e in selected.iter().filter(|e| !members.contains(e)) {
            commands.entity(e).try_remove::<Selected>();
        }
    }
    for e in members.iter().filter(|e| alive.contains(**e)) {
        commands.entity(*e).try_insert(Selected);
    }
}
