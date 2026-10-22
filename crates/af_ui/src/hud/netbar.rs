//! The networked match's state, centred under the top bar: waiting for
//! the server, catching up, a lost connection, a desync. Hidden while all
//! is well, and once the match is over.

use af_core::ext::net::{Multiplayer, NetSession, NetStatus};
use af_core::prelude::*;
use bevy::prelude::*;

use super::HudSet;
use crate::theme::{self, set_display, set_text, text, FONT_BODY};

#[derive(Component)]
struct NetBar;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, update_netbar.in_set(HudSet::Panels));
}

pub(super) fn spawn(root: &mut ChildSpawnerCommands) {
    root.spawn((
        NetBar,
        Node { position_type: PositionType::Absolute, top: px(44), width: percent(100), justify_content: JustifyContent::Center, display: Display::None, ..default() },
        Pickable::IGNORE,
    ))
    .with_children(|row| {
        row.spawn((theme::panel_node(Node { padding: UiRect::axes(px(16), px(6)), ..default() }), theme::panel_colors(), Pickable::IGNORE))
            .with_child((text("", FONT_BODY, theme::TEXT_WARN), Pickable::IGNORE));
    });
}

/// What the bar says, if anything.
pub fn netbar_text(session: &NetSession, outcome_ended: bool) -> Option<(String, Color)> {
    if session.result.is_some() || outcome_ended {
        return None;
    }
    let l = &session.lockstep;
    if let NetStatus::Failed(e) = &session.status {
        return Some((format!("Connection lost: trying to get back to the server ({e})"), theme::TEXT_BAD));
    }
    if l.desynced {
        return Some(("Desynced: this game no longer matches the others".into(), theme::TEXT_BAD));
    }
    if l.stalled_secs >= 0.5 {
        return Some((format!("Waiting for the server... {:.0} s", l.stalled_secs), theme::TEXT_WARN));
    }
    if l.catching_up {
        return Some(("Catching up...".into(), theme::TEXT_WARN));
    }
    None
}

fn update_netbar(
    session: Option<Res<NetSession>>,
    multiplayer: Option<Res<Multiplayer>>,
    outcome: Option<Res<GameOutcome>>,
    mut bars: Query<(&mut Node, &Children), With<NetBar>>,
    mut panels: Query<&Children, Without<NetBar>>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let shown = match (session.as_deref(), multiplayer) {
        (Some(s), Some(_)) => netbar_text(s, outcome.is_some_and(|o| o.ended)),
        _ => None,
    };
    for (mut node, children) in &mut bars {
        set_display(&mut node, shown.is_some());
        let Some((line, color)) = &shown else { continue };
        for panel in children.iter() {
            let Ok(inner) = panels.get_mut(panel) else { continue };
            for child in inner.iter() {
                if let Ok((mut t, mut c)) = texts.get_mut(child) {
                    set_text(&mut t, line);
                    theme::set_color(&mut c, *color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::ext::net::{LockstepView, MatchResultView};

    #[test]
    fn the_bar_reports_trouble_only() {
        let mut s = NetSession { status: NetStatus::Online, ..Default::default() };
        assert_eq!(netbar_text(&s, false), None);
        s.lockstep = LockstepView { stalled_secs: 1.6, ..Default::default() };
        assert_eq!(netbar_text(&s, false).unwrap().0, "Waiting for the server... 2 s");
        s.lockstep = LockstepView { catching_up: true, ..Default::default() };
        assert_eq!(netbar_text(&s, false).unwrap().0, "Catching up...");
        s.lockstep = LockstepView { desynced: true, stalled_secs: 3.0, ..Default::default() };
        assert!(netbar_text(&s, false).unwrap().0.starts_with("Desynced"));
        s.status = NetStatus::Failed("timed out".into());
        assert!(netbar_text(&s, false).unwrap().0.contains("timed out"));
        s.result = Some(MatchResultView::default());
        assert_eq!(netbar_text(&s, false), None, "over: nothing to wait for");
        s.result = None;
        assert_eq!(netbar_text(&s, true), None);
    }
}
