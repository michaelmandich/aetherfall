//! The notices feed: the viewed player's (and global) recent notices,
//! coloured by kind and fading out. Never blocks world clicks.

use af_core::prelude::*;
use bevy::prelude::*;

use super::{HudSet, HudView};
use crate::format::{self, NOTICE_LINES};
use crate::theme::{self, set_display, set_text, text, FONT_BODY};

#[derive(Component, Clone, Copy)]
struct NoticeLine(usize);

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, update_notices.in_set(HudSet::Panels));
}

pub(super) fn spawn(root: &mut ChildSpawnerCommands) {
    root.spawn((
        Node { position_type: PositionType::Absolute, top: px(78), left: px(10), max_width: px(620), flex_direction: FlexDirection::Column, row_gap: px(2), ..default() },
        Pickable::IGNORE,
    ))
    .with_children(|feed| {
        for i in 0..NOTICE_LINES {
            feed.spawn((
                text("", FONT_BODY, theme::TEXT),
                Node { padding: UiRect::axes(px(6), px(1)), display: Display::None, ..default() },
                BackgroundColor(Color::NONE),
                Pickable::IGNORE,
                NoticeLine(i),
            ));
        }
    });
}

fn kind_color(kind: NoticeKind) -> Color {
    match kind {
        NoticeKind::Info => theme::TEXT,
        NoticeKind::Warning => theme::TEXT_WARN,
        NoticeKind::Alert => theme::TEXT_BAD,
    }
}

fn update_notices(
    notices: Res<Notices>,
    view: Res<HudView>,
    clock: Res<SimClock>,
    mut lines: Query<(&NoticeLine, &mut Text, &mut TextColor, &mut BackgroundColor, &mut Node)>,
    mut shown: Local<Vec<usize>>,
) {
    format::recent_notices(&notices.list, view.player, clock.elapsed, &mut shown);
    for (line, mut t, mut color, mut bg, mut node) in &mut lines {
        let Some(n) = shown.get(line.0).and_then(|i| notices.list.get(*i)) else {
            set_display(&mut node, false);
            continue;
        };
        let alpha = format::notice_alpha(clock.elapsed - n.time);
        set_display(&mut node, true);
        set_text(&mut t, &n.text);
        theme::set_color(&mut color, kind_color(n.kind).with_alpha(alpha));
        let back = Color::srgba(0.0, 0.0, 0.0, 0.5 * alpha);
        if bg.0 != back {
            bg.0 = back;
        }
    }
}
