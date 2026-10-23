//! Shared items added by af_net (other crates may use them).
//! Only af_net edits this file. Never modify items in other af_core files.
//!
//! What the multiplayer client shows and does, in types the UI can read
//! and act on without knowing the wire protocol:
//!
//! * [`NetSession`]: the connection, the rooms and lobby the player sees,
//!   the ranked account, the last error, and how the networked match is
//!   doing (stalls, desync, result). Written by af_net, read by af_ui.
//! * [`NetActions`]: what the player asked for (connect, join, log in,
//!   start...). Pushed by af_ui, drained by af_net.
//! * [`Multiplayer`]: present while a networked match runs. af_input leaves
//!   the clock alone (no local pause or speed), af_ui shows the net state.
//! * The views ([`LobbyView`], [`Account`], [`MatchStart`]...) are what the
//!   server sends, so they also travel over the wire (serde).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::resources::VictoryRule;

/// Most humans in a casual room (AI players may fill up to the map's limit).
pub const CASUAL_MAX_HUMANS: usize = 6;
/// Most players (humans and AI) in any networked match: af_map's limit.
pub const MAX_MATCH_PLAYERS: usize = 8;
/// Username rules: 3 to 16 of `A-Z a-z 0-9 _`.
pub const USERNAME_MIN: usize = 3;
pub const USERNAME_MAX: usize = 16;
/// Password rule: at least this long.
pub const PASSWORD_MIN: usize = 8;

/// A username that the server and the account service accept: 3 to 16
/// letters, digits or underscores.
pub fn valid_username(name: &str) -> bool {
    let n = name.chars().count();
    (USERNAME_MIN..=USERNAME_MAX).contains(&n) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The ranked formats. One match of each runs at a time on the server.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RankedMode {
    /// 1v1.
    Duel,
    /// 1v1v1, free for all.
    Triple,
    /// 2v2, won by taking both enemy town halls.
    Teams,
}

impl RankedMode {
    pub const ALL: [RankedMode; 3] = [RankedMode::Duel, RankedMode::Triple, RankedMode::Teams];

    pub fn label(self) -> &'static str {
        match self {
            RankedMode::Duel => "1v1",
            RankedMode::Triple => "1v1v1",
            RankedMode::Teams => "2v2",
        }
    }

    pub fn players(self) -> usize {
        match self {
            RankedMode::Duel => 2,
            RankedMode::Triple => 3,
            RankedMode::Teams => 4,
        }
    }

    /// Team of slot `slot`: slots 0 and 1 against 2 and 3 in 2v2, everyone
    /// for themselves otherwise.
    pub fn team_of(self, slot: usize) -> u8 {
        match self {
            RankedMode::Teams => (slot / 2) as u8,
            _ => slot as u8,
        }
    }

    pub fn rule(self) -> VictoryRule {
        match self {
            RankedMode::Teams => VictoryRule::TownHalls,
            _ => VictoryRule::LastStanding,
        }
    }

    /// The name stored with a match record.
    pub fn id(self) -> &'static str {
        match self {
            RankedMode::Duel => "duel",
            RankedMode::Triple => "triple",
            RankedMode::Teams => "teams",
        }
    }
}

/// A casual room's settings (the host changes them).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoomOptions {
    pub fog: bool,
    pub rule: VictoryRule,
    /// Computer players added after the humans, up to [`MAX_MATCH_PLAYERS`].
    pub ai_players: u8,
}

impl Default for RoomOptions {
    fn default() -> Self {
        RoomOptions { fog: true, rule: VictoryRule::LastStanding, ai_players: 0 }
    }
}

/// One player slot of a lobby as the viewer sees it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SlotView {
    pub name: String,
    /// Faction id (`assets/data`), `None` = random.
    pub faction: Option<String>,
    pub team: u8,
    pub ready: bool,
    pub host: bool,
    /// The viewer's own slot.
    pub you: bool,
    pub ai: bool,
    /// Ranked rating, in ranked lobbies.
    pub elo: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyKind {
    Casual,
    Ranked(RankedMode),
}

/// The lobby the viewer is in.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LobbyView {
    pub id: u32,
    pub name: String,
    pub kind: LobbyKind,
    pub slots: Vec<SlotView>,
    /// Human slots the lobby holds.
    pub max_humans: u8,
    pub options: RoomOptions,
    pub host_is_you: bool,
    /// A match is running in this lobby.
    pub playing: bool,
}

impl LobbyView {
    pub fn teams_in_use(&self) -> bool {
        self.slots.iter().any(|a| self.slots.iter().any(|b| a.team == b.team && !std::ptr::eq(a, b)))
    }
}

/// A casual room in the list.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RoomSummary {
    pub id: u32,
    pub name: String,
    pub humans: u8,
    pub max_humans: u8,
    pub playing: bool,
}

/// How full a ranked format's single slot is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RankedSlotView {
    pub mode: RankedMode,
    pub waiting: u8,
    pub playing: bool,
}

/// The logged-in player's ranked account.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub username: String,
    pub elo: i32,
    pub wins: u32,
    pub losses: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LeaderboardRow {
    pub username: String,
    pub elo: i32,
    pub wins: u32,
    pub losses: u32,
}

/// One player's rating before and after a ranked match.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RatingChange {
    pub username: String,
    pub before: i32,
    pub after: i32,
}

/// One slot of a networked match.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MatchPlayer {
    pub name: String,
    /// Faction id (`assets/data`); already resolved from "random".
    pub faction: String,
    pub team: u8,
    pub ai: bool,
}

/// Everything a client needs to build the same match as everyone else.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MatchStart {
    pub match_id: u64,
    pub seed: u64,
    pub players: Vec<MatchPlayer>,
    /// The viewer's slot, `None` to spectate.
    pub your_slot: Option<u8>,
    pub fog: bool,
    pub rule: VictoryRule,
    pub ranked: bool,
    /// Lets this client rejoin the match after a dropped connection.
    pub resume_token: u64,
}

/// How a networked match ended, as the server reports it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchResultView {
    pub winning_team: Option<u8>,
    /// Ranked only: every player's rating change.
    pub changes: Vec<RatingChange>,
    /// Players who left or dropped and forfeited.
    pub forfeited: Vec<String>,
    /// Why the match ended when not by the victory rule ("void: desync").
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NetStatus {
    #[default]
    Offline,
    Connecting,
    Online,
    /// Could not connect, or the server dropped the connection.
    Failed(String),
}

/// How the networked simulation is pacing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LockstepView {
    /// Sim ticks the server is ahead of us (turns buffered).
    pub buffered: u32,
    /// Seconds the sim has been waiting for the next turn (0 = running).
    pub stalled_secs: f32,
    /// Catching up after a stall or a resume.
    pub catching_up: bool,
    /// Our state stopped matching the server's (or another player's).
    pub desynced: bool,
    /// Last measured round trip to the server.
    pub rtt_ms: Option<u32>,
}

/// The multiplayer client's state for the UI. Written by af_net.
#[derive(Resource, Clone, Debug, Default)]
pub struct NetSession {
    pub status: NetStatus,
    /// `host:port` of the server connected (or being connected) to; before
    /// any connection, the last one used.
    pub server: String,
    /// The display name used in casual rooms.
    pub name: String,
    /// The last username logged in with (to prefill the login form).
    pub saved_username: String,
    /// Server-side ranked play is set up (accounts, ratings).
    pub ranked_enabled: bool,
    /// What the server says on connect.
    pub motd: String,
    pub rooms: Vec<RoomSummary>,
    pub lobby: Option<LobbyView>,
    pub account: Option<Account>,
    /// A sign-up or login is in flight.
    pub auth_busy: bool,
    pub ranked_slots: Vec<RankedSlotView>,
    pub leaderboard: Vec<LeaderboardRow>,
    /// The last failure to show, cleared by the next action.
    pub error: Option<String>,
    /// The last networked match's result (ranked: with rating changes).
    pub result: Option<MatchResultView>,
    pub lockstep: LockstepView,
}

/// What the player asked the multiplayer client to do. af_ui pushes,
/// af_net drains every frame.
#[derive(Resource, Clone, Debug, Default)]
pub struct NetActions {
    pub queue: Vec<NetAction>,
}

impl NetActions {
    pub fn push(&mut self, action: NetAction) {
        self.queue.push(action);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NetAction {
    /// Connect to `host:port` (a bare host gets the default port).
    Connect(String),
    Disconnect,
    RefreshRooms,
    CreateRoom(String),
    JoinRoom(u32),
    /// Leave the casual room or the ranked waiting slot.
    LeaveLobby,
    /// Own faction (`None` = random).
    SetFaction(Option<String>),
    SetTeam(u8),
    SetReady(bool),
    /// Host only.
    SetOptions(RoomOptions),
    /// Host only: start the casual match.
    Start,
    SignUp { username: String, password: String },
    LogIn { username: String, password: String },
    LogOut,
    JoinRanked(RankedMode),
    RefreshRanked,
    Leaderboard,
    /// Leave the running match (a forfeit in ranked play).
    LeaveMatch,
    /// The display name used in casual rooms (remembered between runs).
    SetName(String),
}

/// Present while a networked match runs (inserted by af_net with the
/// match, removed when it ends or the player leaves). The clock belongs to
/// the server: af_input offers no pause or game speed.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Multiplayer {
    pub match_id: u64,
    pub ranked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_short_ascii_words() {
        assert!(valid_username("Fin_42"));
        assert!(valid_username("abc"));
        assert!(!valid_username("ab"), "too short");
        assert!(!valid_username("a_very_long_name_indeed"), "too long");
        assert!(!valid_username("no spaces"));
        assert!(!valid_username("émile"), "ascii only");
        assert!(!valid_username("a@b.c"));
    }

    #[test]
    fn ranked_modes_seat_players() {
        assert_eq!(RankedMode::Teams.players(), 4);
        assert_eq!((0..4).map(|s| RankedMode::Teams.team_of(s)).collect::<Vec<_>>(), vec![0, 0, 1, 1]);
        assert_eq!((0..3).map(|s| RankedMode::Triple.team_of(s)).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(RankedMode::Teams.rule(), VictoryRule::TownHalls);
        assert_eq!(RankedMode::Duel.rule(), VictoryRule::LastStanding);
        assert_eq!(RankedMode::ALL.iter().map(|m| m.id()).collect::<Vec<_>>(), vec!["duel", "triple", "teams"]);
    }
}
