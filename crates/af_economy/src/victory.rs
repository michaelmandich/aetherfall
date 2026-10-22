//! Defeat, reveal and victory (`SimSet::Economy`, once per sim second).
//!
//! * **Defeat** (`VictoryRule::LastStanding`, the default): a player with no
//!   living unit and no complete building that matters is out. Walls, gates
//!   and pure resource fields (farms) do not matter, and neither do
//!   foundations (see
//!   [`keeps_player_alive`](af_core::ext::economy::keeps_player_alive)), so
//!   a beaten player cannot linger behind a few walls and fields.
//! * **Town halls** (`VictoryRule::TownHalls`, ranked 2v2): a whole team is
//!   out once none of its players owns a complete main hall (a
//!   `TownCenter`-tagged building). A player whose own hall fell stays in
//!   while a teammate holds one.
//! * **Reveal**: a player still in the game who owns no complete
//!   [production building](af_core::ext::economy::is_production_building)
//!   has every remaining building revealed to all other teams
//!   ([`RevealedPlayers`], shown by af_fog), StarCraft style, so the
//!   survivors can find and finish them. Announced to everyone.
//! * **Victory**: the match ends when every undefeated player is on one team
//!   (or nobody is left); a match that started with a single team never ends.

use af_core::ext::economy::{is_production_building, keeps_player_alive, RevealedPlayers};
use af_core::prelude::*;
use bevy::prelude::*;

/// No one can lose (or be revealed) before this many sim seconds (map
/// setup, slow starts).
const GRACE_SECONDS: f32 = 10.0;
/// Ticks between checks.
const CHECK_EVERY: u64 = SIM_HZ as u64;

pub(crate) fn check_victory(
    data: Res<Data>,
    clock: Res<SimClock>,
    setup: Option<Res<MatchSetup>>,
    mut players: ResMut<Players>,
    mut notices: ResMut<Notices>,
    mut outcome: ResMut<GameOutcome>,
    mut revealed: ResMut<RevealedPlayers>,
) {
    if outcome.ended || clock.elapsed < GRACE_SECONDS || !clock.tick.is_multiple_of(CHECK_EVERY) {
        return;
    }
    match setup.as_deref().map_or(VictoryRule::LastStanding, |s| s.rule) {
        VictoryRule::LastStanding => {
            for p in players.list.iter_mut().filter(|p| !p.defeated) {
                if !holds_anything(&data, p) {
                    p.defeated = true;
                    notices.push(None, NoticeKind::Alert, format!("{} has been defeated", p.name), None, clock.elapsed);
                }
            }
        }
        VictoryRule::TownHalls => {
            let mut fallen: Vec<u8> = Vec::new();
            for p in players.list.iter().filter(|p| !p.defeated) {
                let team_holds_hall = players.list.iter().any(|q| q.team == p.team && !q.defeated && holds_hall(&data, q));
                if !team_holds_hall && !fallen.contains(&p.team) {
                    fallen.push(p.team);
                }
            }
            for team in fallen {
                notices.push(None, NoticeKind::Alert, format!("Team {} has lost its last town hall", team + 1), None, clock.elapsed);
                for p in players.list.iter_mut().filter(|p| p.team == team && !p.defeated) {
                    p.defeated = true;
                    notices.push(None, NoticeKind::Alert, format!("{} has been defeated", p.name), None, clock.elapsed);
                }
            }
        }
    }
    update_reveals(&data, &players, &clock, &mut notices, &mut revealed);
    let mut teams = players.list.iter().map(|p| p.team);
    let first = teams.next();
    if teams.all(|t| Some(t) == first) {
        return;
    }
    let mut standing = players.list.iter().filter(|p| !p.defeated).map(|p| p.team);
    let winning_team = match standing.next() {
        None => None,
        Some(team) if standing.all(|t| t == team) => Some(team),
        Some(_) => return,
    };
    *outcome = GameOutcome { ended: true, winning_team, time: clock.elapsed };
}

/// Reveal the players still in the game who own no complete production
/// building, and hide again those who completed one. Defeated players are
/// not revealed (they are out).
fn update_reveals(data: &GameData, players: &Players, clock: &SimClock, notices: &mut Notices, revealed: &mut RevealedPlayers) {
    for p in &players.list {
        let reveal = !p.defeated && !holds_production(data, p);
        if !revealed.set(p.id, reveal) || p.defeated {
            continue;
        }
        if reveal {
            let text = format!("{} has lost every production building: its buildings are revealed", p.name);
            notices.push(None, NoticeKind::Alert, text, None, clock.elapsed);
        } else {
            notices.push(None, NoticeKind::Info, format!("{} is hidden again", p.name), None, clock.elapsed);
        }
    }
}

/// Uses the counts af_core takes in Prepare (living units, complete buildings).
fn holds_anything(data: &GameData, p: &PlayerState) -> bool {
    p.units_owned.iter().any(|n| *n > 0) || owns_complete(data, p, |kind| keeps_player_alive(data, p.faction, kind))
}

/// Does `p` own a complete main hall (`TownCenter`-tagged building)?
fn holds_hall(data: &GameData, p: &PlayerState) -> bool {
    owns_complete(data, p, |kind| data.building(kind).has_tag(Tag::TownCenter))
}

fn holds_production(data: &GameData, p: &PlayerState) -> bool {
    owns_complete(data, p, |kind| is_production_building(data, p.faction, kind))
}

/// Does `p` own a complete building of a type `f` accepts?
fn owns_complete(data: &GameData, p: &PlayerState, f: impl Fn(BuildingTypeId) -> bool) -> bool {
    data.building_ids().zip(&p.buildings_owned).any(|(kind, n)| *n > 0 && f(kind))
}

/// Forget the reveals of the previous match.
pub(crate) fn reset_reveals(mut revealed: ResMut<RevealedPlayers>) {
    revealed.list.clear();
}
