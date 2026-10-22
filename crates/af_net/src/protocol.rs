//! What travels between a client and the server: one [`ClientMsg`] or
//! [`ServerMsg`] per frame (see [`crate::framing`]), encoded with postcard.
//!
//! Commands cross the wire as [`WireCommand`]s: plain numbers instead of
//! `Entity`s and typed ids, so the encoding does not depend on how Bevy or
//! af_data lay their types out. Sim entity ids are the same on every peer
//! (`af_core::sim_ids`), so an entity's bits name the same unit everywhere.

use af_core::ext::net::{Account, LeaderboardRow, LobbyView, MatchResultView, MatchStart, RankedMode, RankedSlotView, RoomOptions, RoomSummary};
use af_core::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Bumped whenever a message changes shape; client and server must agree.
pub const PROTOCOL_VERSION: u32 = 1;

/// A `PlayerCommand` in wire form.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WireCommand {
    pub player: u8,
    /// `Entity::to_bits` of each commanded entity.
    pub entities: Vec<u64>,
    pub kind: WireKind,
    pub queue: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum WireKind {
    Smart { point: [f32; 2], target: Option<u64> },
    Move([f32; 2]),
    AttackMove([f32; 2]),
    Attack(u64),
    Patrol([f32; 2]),
    Follow(u64),
    Stop,
    Hold,
    Gather(u64),
    Build(u64),
    Repair(u64),
    SetStance(u8),
    Delete,
    Resign,
    PlaceBuilding { kind: u16, pos: [f32; 2], facing: f32 },
    PlaceWall { kind: u16, from: [f32; 2], to: [f32; 2] },
    Train { unit: u16, count: u32 },
    Research { tech: u16 },
    UpgradeBuilding,
    CancelQueue { index: u32 },
    SetRally { point: [f32; 2], target: Option<u64> },
}

const STANCES: [Stance; 4] = [Stance::Aggressive, Stance::Defensive, Stance::StandGround, Stance::Passive];

fn v(p: Vec2) -> [f32; 2] {
    [p.x, p.y]
}

fn e(ent: Entity) -> u64 {
    ent.to_bits()
}

fn vec2(p: [f32; 2]) -> Vec2 {
    Vec2::new(p[0], p[1])
}

fn entity(bits: u64) -> Option<Entity> {
    Entity::try_from_bits(bits)
}

impl From<&PlayerCommand> for WireCommand {
    fn from(c: &PlayerCommand) -> WireCommand {
        let kind = match &c.kind {
            CommandKind::Smart { point, target } => WireKind::Smart { point: v(*point), target: target.map(e) },
            CommandKind::Move(p) => WireKind::Move(v(*p)),
            CommandKind::AttackMove(p) => WireKind::AttackMove(v(*p)),
            CommandKind::Attack(t) => WireKind::Attack(e(*t)),
            CommandKind::Patrol(p) => WireKind::Patrol(v(*p)),
            CommandKind::Follow(t) => WireKind::Follow(e(*t)),
            CommandKind::Stop => WireKind::Stop,
            CommandKind::Hold => WireKind::Hold,
            CommandKind::Gather(t) => WireKind::Gather(e(*t)),
            CommandKind::Build(t) => WireKind::Build(e(*t)),
            CommandKind::Repair(t) => WireKind::Repair(e(*t)),
            CommandKind::SetStance(s) => WireKind::SetStance(STANCES.iter().position(|x| x == s).unwrap_or(0) as u8),
            CommandKind::Delete => WireKind::Delete,
            CommandKind::Resign => WireKind::Resign,
            CommandKind::PlaceBuilding { kind, pos, facing } => WireKind::PlaceBuilding { kind: kind.0, pos: v(*pos), facing: *facing },
            CommandKind::PlaceWall { kind, from, to } => WireKind::PlaceWall { kind: kind.0, from: v(*from), to: v(*to) },
            CommandKind::Train { unit, count } => WireKind::Train { unit: unit.0, count: *count },
            CommandKind::Research { tech } => WireKind::Research { tech: tech.0 },
            CommandKind::UpgradeBuilding => WireKind::UpgradeBuilding,
            CommandKind::CancelQueue { index } => WireKind::CancelQueue { index: *index as u32 },
            CommandKind::SetRally { point, target } => WireKind::SetRally { point: v(*point), target: target.map(e) },
        };
        WireCommand { player: c.player.0, entities: c.entities.iter().map(|x| e(*x)).collect(), kind, queue: c.queue }
    }
}

impl WireCommand {
    /// The command, or `None` when an entity id is not one at all (a bad
    /// client; the sim never sees it).
    pub fn to_command(&self) -> Option<PlayerCommand> {
        let ents: Option<Vec<Entity>> = self.entities.iter().map(|b| entity(*b)).collect();
        let t = |o: Option<u64>| -> Option<Option<Entity>> {
            match o {
                None => Some(None),
                Some(b) => entity(b).map(Some),
            }
        };
        let kind = match &self.kind {
            WireKind::Smart { point, target } => CommandKind::Smart { point: vec2(*point), target: t(*target)? },
            WireKind::Move(p) => CommandKind::Move(vec2(*p)),
            WireKind::AttackMove(p) => CommandKind::AttackMove(vec2(*p)),
            WireKind::Attack(b) => CommandKind::Attack(entity(*b)?),
            WireKind::Patrol(p) => CommandKind::Patrol(vec2(*p)),
            WireKind::Follow(b) => CommandKind::Follow(entity(*b)?),
            WireKind::Stop => CommandKind::Stop,
            WireKind::Hold => CommandKind::Hold,
            WireKind::Gather(b) => CommandKind::Gather(entity(*b)?),
            WireKind::Build(b) => CommandKind::Build(entity(*b)?),
            WireKind::Repair(b) => CommandKind::Repair(entity(*b)?),
            WireKind::SetStance(i) => CommandKind::SetStance(*STANCES.get(*i as usize)?),
            WireKind::Delete => CommandKind::Delete,
            WireKind::Resign => CommandKind::Resign,
            WireKind::PlaceBuilding { kind, pos, facing } => CommandKind::PlaceBuilding { kind: BuildingTypeId(*kind), pos: vec2(*pos), facing: *facing },
            WireKind::PlaceWall { kind, from, to } => CommandKind::PlaceWall { kind: BuildingTypeId(*kind), from: vec2(*from), to: vec2(*to) },
            WireKind::Train { unit, count } => CommandKind::Train { unit: UnitTypeId(*unit), count: *count },
            WireKind::Research { tech } => CommandKind::Research { tech: TechId(*tech) },
            WireKind::UpgradeBuilding => CommandKind::UpgradeBuilding,
            WireKind::CancelQueue { index } => CommandKind::CancelQueue { index: *index as usize },
            WireKind::SetRally { point, target } => CommandKind::SetRally { point: vec2(*point), target: t(*target)? },
        };
        Some(PlayerCommand { player: PlayerId(self.player), entities: ents?, kind, queue: self.queue })
    }
}

/// Client to server.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ClientMsg {
    /// First message. `content_hash` is `af_data::content_hash` of the
    /// client's `assets/data`; it must match the server's.
    Hello { protocol: u32, content_hash: u64, name: String },
    /// Rejoin a match this connection was dropped from; the server resends
    /// the match start and every turn from `from_tick` on.
    Resume { match_id: u64, resume_token: u64, from_tick: u64 },
    ListRooms,
    CreateRoom { name: String },
    JoinRoom { id: u32 },
    /// Leave the casual room or the ranked waiting slot.
    LeaveLobby,
    SetFaction(Option<String>),
    SetTeam(u8),
    SetReady(bool),
    SetOptions(RoomOptions),
    /// Host: start the casual match.
    Start,
    /// A Supabase access token from the account service.
    Login { access_token: String },
    Logout,
    JoinRanked(RankedMode),
    RefreshRanked,
    Leaderboard,
    /// The match world is built; turns may start.
    Loaded,
    /// The local player's commands since the last message.
    Commands(Vec<WireCommand>),
    /// `af_core::hash::sim_hash` after sim tick `tick`.
    Hash { tick: u64, hash: u64 },
    /// Casual: this client's sim ended the match.
    MatchOver { winning_team: Option<u8>, time: f32 },
    /// Leave the running match (a forfeit in ranked play).
    LeaveMatch,
    Ping(u64),
}

/// Server to client.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ServerMsg {
    /// Answer to `Hello`.
    Welcome { motd: String, ranked_enabled: bool, supabase_url: String, supabase_anon_key: String, email_domain: String },
    /// `Hello` was turned down (wrong protocol or content); the server closes.
    Refused(String),
    /// An action failed; nothing else changed.
    Error(String),
    Rooms(Vec<RoomSummary>),
    Lobby(LobbyView),
    LeftLobby,
    LoggedIn(Account),
    LoggedOut,
    RankedSlots(Vec<RankedSlotView>),
    Leaderboard(Vec<LeaderboardRow>),
    StartMatch(MatchStart),
    /// Every command that executes in sim tick `tick`, in order. Sent for
    /// every tick, empty or not.
    Turn { tick: u64, commands: Vec<WireCommand> },
    /// Many turns at once (after a `Resume`).
    Turns(Vec<(u64, Vec<WireCommand>)>),
    /// These players' state hashes disagree at `tick`.
    Desync { tick: u64, players: Vec<String> },
    MatchEnded(MatchResultView),
    Pong(u64),
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).expect("postcard encodes every message")
}

pub fn decode<T: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<T, String> {
    postcard::from_bytes(bytes).map_err(|e| format!("bad message: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_round_trip() {
        let a = Entity::from_bits(((7u64) << 32) | 40_000);
        let b = Entity::from_bits(((1u64) << 32) | 40_001);
        let cmds = vec![
            PlayerCommand::new(PlayerId(2), vec![a, b], CommandKind::Smart { point: Vec2::new(1.5, -2.0), target: Some(b) }).queued(true),
            PlayerCommand::new(PlayerId(0), vec![a], CommandKind::SetStance(Stance::StandGround)),
            PlayerCommand::new(PlayerId(0), vec![], CommandKind::PlaceWall { kind: BuildingTypeId(3), from: Vec2::ZERO, to: Vec2::new(10.0, 0.0) }),
            PlayerCommand::new(PlayerId(1), vec![a], CommandKind::Train { unit: UnitTypeId(9), count: 5 }),
            PlayerCommand::new(PlayerId(1), vec![a], CommandKind::CancelQueue { index: 3 }),
            PlayerCommand::new(PlayerId(1), vec![a], CommandKind::SetRally { point: Vec2::new(3.0, 4.0), target: None }),
        ];
        for c in &cmds {
            let wire = WireCommand::from(c);
            let bytes = encode(&wire);
            let back: WireCommand = decode(&bytes).unwrap();
            assert_eq!(back, wire);
            assert_eq!(back.to_command().unwrap(), *c);
        }
    }

    #[test]
    fn bad_entity_bits_are_refused() {
        // Bevy keeps entity indices as "non-max" integers, so index bits of
        // all zeros never name an entity.
        assert!(Entity::try_from_bits(0).is_none());
        let bad = WireCommand { player: 0, entities: vec![0], kind: WireKind::Stop, queue: false };
        assert!(bad.to_command().is_none());
        let bad_stance = WireCommand { player: 0, entities: vec![], kind: WireKind::SetStance(9), queue: false };
        assert!(bad_stance.to_command().is_none());
    }

    #[test]
    fn messages_round_trip() {
        let msg = ServerMsg::Turn { tick: 42, commands: vec![WireCommand { player: 1, entities: vec![5], kind: WireKind::Hold, queue: false }] };
        let back: ServerMsg = decode(&encode(&msg)).unwrap();
        assert_eq!(back, msg);
        let hello = ClientMsg::Hello { protocol: PROTOCOL_VERSION, content_hash: 7, name: "Fin".into() };
        assert_eq!(decode::<ClientMsg>(&encode(&hello)).unwrap(), hello);
        assert!(decode::<ServerMsg>(&[0xff, 0xff, 0xff]).is_err());
    }
}
