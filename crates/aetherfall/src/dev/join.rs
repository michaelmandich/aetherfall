//! `--join SERVER[,NAME]`: an online test client. On the menu it connects
//! to SERVER (`host[:port]`), takes the first room that is waiting for
//! players and readies up, as a player clicking through the multiplayer
//! screens would; the host starts the match. The other dev flags then act
//! on that match: a `--script` plays it and `--shots` record it, timed by
//! its sim clock.

use af_core::ext::net::{NetAction, NetActions, NetSession, NetStatus};
use af_core::prelude::*;
use af_ui::menu::MenuScreen;
use bevy::prelude::*;

/// Real seconds between two requests to the server (a room list, a join,
/// a ready), so one slow answer is not asked for again every frame.
const RETRY_SECS: f32 = 1.5;

/// Where `--join` connects, and how far it got.
#[derive(Resource, Debug)]
pub(super) struct JoinPlan {
    server: String,
    name: Option<String>,
    /// Real seconds of the last request; `None` before connecting.
    asked_at: Option<f32>,
}

impl JoinPlan {
    /// `SERVER[,NAME]`.
    pub(super) fn parse(arg: &str) -> Result<JoinPlan, String> {
        let (server, name) = match arg.split_once(',') {
            Some((server, name)) => (server.trim(), Some(name.trim())),
            None => (arg.trim(), None),
        };
        if server.is_empty() || name.is_some_and(str::is_empty) {
            return Err(format!("--join wants SERVER[,NAME] (like 127.0.0.1:7777,Mac), got `{arg}`"));
        }
        Ok(JoinPlan { server: server.to_string(), name: name.map(str::to_string), asked_at: None })
    }
}

pub(super) fn install(app: &mut App, plan: JoinPlan) {
    app.insert_resource(plan).add_systems(Update, auto_join.run_if(in_state(GameState::Menu)));
}

/// Connect, join the first waiting room, ready up; after a match the
/// lobby comes back and it readies again for the next one.
fn auto_join(
    mut plan: ResMut<JoinPlan>,
    session: Res<NetSession>,
    mut actions: ResMut<NetActions>,
    mut screen: ResMut<MenuScreen>,
    real: Res<Time<Real>>,
) {
    let now = real.elapsed_secs();
    let Some(asked_at) = plan.asked_at else {
        *screen = MenuScreen::Multiplayer;
        if let Some(name) = &plan.name {
            actions.push(NetAction::SetName(name.clone()));
        }
        actions.push(NetAction::Connect(plan.server.clone()));
        plan.asked_at = Some(now);
        return;
    };
    if session.status != NetStatus::Online || now - asked_at < RETRY_SECS {
        return;
    }
    let action = match &session.lobby {
        Some(lobby) if lobby.playing => return,
        Some(lobby) => match lobby.slots.iter().find(|s| s.you) {
            Some(slot) if !slot.ready => NetAction::SetReady(true),
            _ => return,
        },
        None => match session.rooms.iter().find(|r| !r.playing && r.humans < r.max_humans) {
            Some(room) => NetAction::JoinRoom(room.id),
            None => NetAction::RefreshRooms,
        },
    };
    actions.push(action);
    plan.asked_at = Some(now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_join_flag_takes_a_server_and_an_optional_name() {
        let p = JoinPlan::parse("127.0.0.1:7777,Mac").unwrap();
        assert_eq!((p.server.as_str(), p.name.as_deref()), ("127.0.0.1:7777", Some("Mac")));
        let p = JoinPlan::parse("example.org").unwrap();
        assert_eq!((p.server.as_str(), p.name), ("example.org", None));
        for bad in ["", ",Mac", "host,"] {
            assert!(JoinPlan::parse(bad).is_err(), "`{bad}` was accepted");
        }
    }
}
