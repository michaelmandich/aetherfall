//! A networked match as the server runs it: who is in it, the clock, the
//! turns, the state hashes, and for ranked play the referee simulation
//! that decides the result.
//!
//! Every tick the server takes the commands that arrived since the last
//! one (stamped with the sender's slot and, in ranked play, checked
//! against the referee's world: a player may only command what they own),
//! makes them the tick's turn, keeps it in `history` (for a client that
//! rejoins) and hands it out for broadcast. The ranked referee runs the
//! same turn through the same simulation as every client.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use af_core::ext::net::*;
use af_core::hash::sim_hash;
use af_core::prelude::*;
use af_net::client::HASH_EVERY;
use af_net::protocol::{WireCommand, WireKind};
use bevy::prelude::*;

use crate::net::ConnId;

/// Most commands one player gets into one tick.
const MAX_COMMANDS_PER_TICK: usize = 64;
/// Most entities one command may name.
const MAX_ENTITIES_PER_COMMAND: usize = 512;
/// Server hashes kept for comparison (ticks, in steps of `HASH_EVERY`).
const HASHES_KEPT: usize = 30;
/// A ranked player is desynced after this many hash mismatches in a row.
const DESYNC_STRIKES: u8 = 2;

/// One human in the match.
#[derive(Debug)]
pub struct Participant {
    pub slot: u8,
    pub conn: Option<ConnId>,
    pub name: String,
    /// Ranked: the account.
    pub user_id: Option<String>,
    pub elo_before: i32,
    pub resume_token: u64,
    pub loaded: bool,
    /// Left for good (or forfeited).
    pub left: bool,
    /// When the connection dropped (cleared on resume).
    pub dropped_at: Option<Instant>,
    pub forfeited: bool,
    desync_strikes: u8,
    pub desynced: bool,
    pending: Vec<WireCommand>,
}

impl Participant {
    pub fn new(slot: u8, conn: ConnId, name: String, user_id: Option<String>, elo_before: i32, resume_token: u64) -> Participant {
        Participant {
            slot,
            conn: Some(conn),
            name,
            user_id,
            elo_before,
            resume_token,
            loaded: false,
            left: false,
            dropped_at: None,
            forfeited: false,
            desync_strikes: 0,
            desynced: false,
            pending: Vec::new(),
        }
    }

    /// Still part of the match, connection or not.
    pub fn active(&self) -> bool {
        !self.left
    }
}

pub struct LiveMatch {
    pub id: u64,
    pub ranked: bool,
    pub mode: Option<RankedMode>,
    pub seed: u64,
    pub players: Vec<MatchPlayer>,
    pub fog: bool,
    pub rule: VictoryRule,
    pub participants: Vec<Participant>,
    pub created: Instant,
    /// The clock runs: the first tick is due at `next_tick_at`.
    pub started: bool,
    pub tick: u64,
    pub next_tick_at: Instant,
    /// Turn `t` is `history[t - 1]`.
    pub history: Vec<Vec<WireCommand>>,
    referee: Option<App>,
    server_hashes: VecDeque<(u64, u64)>,
    /// Casual: the hashes the clients reported, per tick.
    client_hashes: HashMap<u64, Vec<(u8, u64)>>,
    pub over: Option<MatchResultView>,
}

/// Seconds per tick, as a duration.
pub fn tick_duration() -> Duration {
    Duration::from_secs_f64(1.0 / SIM_HZ)
}

/// What a hash report found.
#[derive(Debug, PartialEq, Eq)]
pub enum HashVerdict {
    Fine,
    /// These slots disagree (with the server, or with the others).
    Desync(Vec<u8>),
}

impl LiveMatch {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: u64,
        ranked: bool,
        mode: Option<RankedMode>,
        seed: u64,
        players: Vec<MatchPlayer>,
        fog: bool,
        rule: VictoryRule,
        participants: Vec<Participant>,
        referee: Option<App>,
        now: Instant,
    ) -> LiveMatch {
        LiveMatch {
            id,
            ranked,
            mode,
            seed,
            players,
            fog,
            rule,
            participants,
            created: now,
            started: false,
            tick: 0,
            next_tick_at: now,
            history: Vec::new(),
            referee,
            server_hashes: VecDeque::new(),
            client_hashes: HashMap::new(),
            over: None,
        }
    }

    /// The match start for the player in `slot` (or a spectator).
    pub fn start_for(&self, slot: Option<u8>) -> MatchStart {
        let resume_token = slot.and_then(|s| self.participants.iter().find(|p| p.slot == s)).map_or(0, |p| p.resume_token);
        MatchStart { match_id: self.id, seed: self.seed, players: self.players.clone(), your_slot: slot, fog: self.fog, rule: self.rule, ranked: self.ranked, resume_token }
    }

    pub fn participant_by_conn(&mut self, conn: ConnId) -> Option<&mut Participant> {
        self.participants.iter_mut().find(|p| p.conn == Some(conn))
    }

    pub fn participant_by_slot(&mut self, slot: u8) -> Option<&mut Participant> {
        self.participants.iter_mut().find(|p| p.slot == slot)
    }

    /// Connections the turns go to: the players still in the match.
    pub fn connections(&self) -> Vec<ConnId> {
        self.participants.iter().filter(|p| !p.left).filter_map(|p| p.conn).collect()
    }

    /// Every connection that should hear how the match ended, players who
    /// left included (they are back in the menu, and want their result).
    pub fn all_connections(&self) -> Vec<ConnId> {
        self.participants.iter().filter_map(|p| p.conn).collect()
    }

    /// Nobody is connected who is still playing.
    pub fn deserted(&self) -> bool {
        self.participants.iter().all(|p| p.left || p.conn.is_none())
    }

    /// Everyone still in and connected has built the world.
    pub fn all_loaded(&self) -> bool {
        self.participants.iter().filter(|p| p.active() && p.conn.is_some()).all(|p| p.loaded)
    }

    pub fn any_loaded(&self) -> bool {
        self.participants.iter().any(|p| p.loaded)
    }

    pub fn running(&self) -> bool {
        self.started && self.over.is_none()
    }

    pub fn start_clock(&mut self, now: Instant) {
        if !self.started {
            self.started = true;
            self.next_tick_at = now;
        }
    }

    /// Sim seconds played.
    pub fn elapsed_secs(&self) -> f32 {
        self.tick as f32 / SIM_HZ as f32
    }

    /// Commands from the player in `slot`, to execute in the next tick.
    /// Stamped with the slot; in ranked play, entities the player does not
    /// own are dropped.
    pub fn queue(&mut self, slot: u8, commands: Vec<WireCommand>) {
        let referee = self.referee.as_ref().map(|app| app.world());
        let Some(p) = self.participants.iter_mut().find(|p| p.slot == slot) else { return };
        if p.left {
            return;
        }
        for mut cmd in commands {
            if p.pending.len() >= MAX_COMMANDS_PER_TICK {
                break;
            }
            cmd.player = slot;
            cmd.entities.truncate(MAX_ENTITIES_PER_COMMAND);
            if let Some(world) = referee {
                cmd.entities.retain(|bits| {
                    Entity::try_from_bits(*bits)
                        .is_some_and(|e| world.get::<Owner>(e).is_some_and(|o| o.0 == PlayerId(slot)) && world.get::<Dead>(e).is_none())
                });
                let needs_entities = !matches!(cmd.kind, WireKind::PlaceBuilding { .. } | WireKind::PlaceWall { .. } | WireKind::Resign);
                if needs_entities && cmd.entities.is_empty() {
                    continue;
                }
            }
            if matches!(cmd.kind, WireKind::Resign) {
                p.forfeited = true;
            }
            p.pending.push(cmd);
        }
    }

    /// The player gives up (left, dropped for too long, or caught desynced
    /// under the forfeit policy).
    pub fn resign(&mut self, slot: u8) {
        let Some(p) = self.participants.iter_mut().find(|p| p.slot == slot) else { return };
        if p.forfeited {
            return;
        }
        p.forfeited = true;
        p.pending.push(WireCommand { player: slot, entities: Vec::new(), kind: WireKind::Resign, queue: false });
    }

    /// Is a tick due at `now`?
    pub fn due(&self, now: Instant) -> bool {
        self.running() && now >= self.next_tick_at
    }

    /// Run one tick: gather the turn, feed the referee, keep the history.
    /// Returns the turn to broadcast.
    pub fn step(&mut self) -> (u64, Vec<WireCommand>) {
        self.tick += 1;
        self.next_tick_at += tick_duration();
        let mut turn = Vec::new();
        for p in &mut self.participants {
            turn.append(&mut p.pending);
        }
        if let Some(app) = self.referee.as_mut() {
            let commands: Vec<PlayerCommand> = turn.iter().filter_map(WireCommand::to_command).collect();
            app.world_mut().resource_mut::<CommandInbox>().pending.extend(commands);
            app.update();
            let clock = *app.world().resource::<SimClock>();
            if clock.tick != self.tick {
                warn!("referee of match {} is at tick {} after turn {}", self.id, clock.tick, self.tick);
            }
            if self.tick.is_multiple_of(HASH_EVERY) {
                let hash = sim_hash(app.world_mut());
                self.server_hashes.push_back((self.tick, hash));
                while self.server_hashes.len() > HASHES_KEPT {
                    self.server_hashes.pop_front();
                }
            }
        }
        self.history.push(turn.clone());
        (self.tick, turn)
    }

    /// The referee's outcome, once the match is decided.
    pub fn referee_outcome(&self) -> Option<GameOutcome> {
        let app = self.referee.as_ref()?;
        let outcome = app.world().resource::<GameOutcome>();
        outcome.ended.then(|| outcome.clone())
    }

    /// Player names and standing as the referee sees them.
    pub fn referee_players(&self) -> Option<Players> {
        self.referee.as_ref().map(|app| app.world().resource::<Players>().clone())
    }

    /// The server's hash at `tick`, in ranked play.
    pub fn server_hash(&self, tick: u64) -> Option<u64> {
        self.server_hashes.iter().find(|(t, _)| *t == tick).map(|(_, h)| *h)
    }

    /// A client's hash report.
    pub fn report_hash(&mut self, slot: u8, tick: u64, hash: u64) -> HashVerdict {
        if self.ranked {
            let Some(mine) = self.server_hash(tick) else { return HashVerdict::Fine };
            let Some(p) = self.participants.iter_mut().find(|p| p.slot == slot) else { return HashVerdict::Fine };
            if mine == hash {
                p.desync_strikes = 0;
                return HashVerdict::Fine;
            }
            p.desync_strikes += 1;
            if p.desync_strikes >= DESYNC_STRIKES && !p.desynced {
                p.desynced = true;
                return HashVerdict::Desync(vec![slot]);
            }
            return HashVerdict::Fine;
        }
        // Casual: the players check each other.
        let entries = self.client_hashes.entry(tick).or_default();
        if !entries.iter().any(|(s, _)| *s == slot) {
            entries.push((slot, hash));
        }
        let expected: Vec<u8> = self.participants.iter().filter(|p| p.active() && p.conn.is_some() && p.loaded).map(|p| p.slot).collect();
        if !expected.iter().all(|s| entries.iter().any(|(e, _)| e == s)) {
            return HashVerdict::Fine;
        }
        let entries = self.client_hashes.remove(&tick).unwrap_or_default();
        self.client_hashes.retain(|t, _| *t + 100 > tick);
        // The majority (the lowest hash on a tie) is taken as right.
        let mut counts: Vec<(u64, usize)> = Vec::new();
        for (_, h) in &entries {
            match counts.iter_mut().find(|(x, _)| x == h) {
                Some((_, n)) => *n += 1,
                None => counts.push((*h, 1)),
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let Some((majority, _)) = counts.first() else { return HashVerdict::Fine };
        let mut odd: Vec<u8> = entries.iter().filter(|(_, h)| h != majority).map(|(s, _)| *s).collect();
        for p in &mut self.participants {
            if odd.contains(&p.slot) {
                if p.desynced {
                    odd.retain(|s| *s != p.slot);
                } else {
                    p.desynced = true;
                }
            }
        }
        if odd.is_empty() { HashVerdict::Fine } else { HashVerdict::Desync(odd) }
    }

    pub fn referee_mut(&mut self) -> Option<&mut App> {
        self.referee.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(slot: u8) -> Participant {
        let mut p = Participant::new(slot, 100 + slot as u64, format!("p{slot}"), None, 1600, 7);
        p.loaded = true;
        p
    }

    fn casual() -> LiveMatch {
        LiveMatch::new(1, false, None, 5, Vec::new(), true, VictoryRule::LastStanding, vec![member(0), member(1), member(2)], None, Instant::now())
    }

    #[test]
    fn turns_collect_each_tick_and_are_stamped() {
        let mut m = casual();
        m.start_clock(Instant::now());
        let cmd = WireCommand { player: 9, entities: vec![1], kind: WireKind::Stop, queue: false };
        m.queue(1, vec![cmd.clone()]);
        m.queue(0, vec![cmd.clone()]);
        let (tick, turn) = m.step();
        assert_eq!(tick, 1);
        assert_eq!(turn.iter().map(|c| c.player).collect::<Vec<_>>(), vec![0, 1], "slot order, stamped with the sender's slot");
        assert_eq!(m.step(), (2, vec![]));
        assert_eq!(m.history.len(), 2);
        m.resign(2);
        let (_, turn) = m.step();
        assert_eq!(turn, vec![WireCommand { player: 2, entities: vec![], kind: WireKind::Resign, queue: false }]);
        assert!(m.participants[2].forfeited);
    }

    #[test]
    fn casual_hashes_single_out_the_odd_one() {
        let mut m = casual();
        assert_eq!(m.report_hash(0, 10, 1), HashVerdict::Fine, "waiting for the others");
        assert_eq!(m.report_hash(1, 10, 1), HashVerdict::Fine);
        assert_eq!(m.report_hash(2, 10, 2), HashVerdict::Desync(vec![2]));
        // Announced once.
        for s in 0..3 {
            let v = m.report_hash(s, 20, if s == 2 { 5 } else { 1 });
            assert_eq!(v, HashVerdict::Fine, "{s}");
        }
        // A player who left no longer holds the check up.
        m.participants[2].left = true;
        assert_eq!(m.report_hash(0, 30, 1), HashVerdict::Fine);
        assert_eq!(m.report_hash(1, 30, 1), HashVerdict::Fine);
        assert!(m.client_hashes.is_empty());
    }

    #[test]
    fn match_start_carries_the_slot_and_token() {
        let m = casual();
        let s = m.start_for(Some(1));
        assert_eq!((s.your_slot, s.resume_token, s.match_id), (Some(1), 7, 1));
        assert_eq!(m.start_for(None).resume_token, 0);
    }
}
