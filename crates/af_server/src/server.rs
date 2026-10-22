//! The server's state and its main loop: clients, casual rooms, the three
//! ranked slots, and the matches in progress.
//!
//! One thread owns everything ([`run`]): it accepts connections, handles
//! every [`Event`] the connection and worker threads send, steps the
//! matches whose tick is due, and does the housekeeping (load timeouts,
//! forfeits, finished matches). Nothing here blocks on the network or the
//! account service: those run on their own threads and report back as
//! events.

use std::collections::{BTreeMap, HashMap};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use af_core::ext::net::*;
use af_core::prelude::*;
use af_net::auth::AuthUser;
use af_net::protocol::{ClientMsg, ServerMsg, PROTOCOL_VERSION};

use crate::accounts::{Accounts, MatchRecord, MatchRecordPlayer, PlayerRow};
use crate::config::ServerConfig;
use crate::elo;
use crate::live::{HashVerdict, LiveMatch, Participant};
use crate::net::{accept_pending, ConnId, Event, Outbox};

/// Rows the leaderboard shows.
const LEADERBOARD_ROWS: usize = 20;
/// Most ticks a match catches up per loop pass (a stalled server never
/// floods its clients).
const MAX_TICKS_PER_PASS: u32 = 10;
/// A casual match nobody is connected to for this long is dropped.
const ABANDONED_AFTER: Duration = Duration::from_secs(600);

struct Client {
    name: String,
    hello: bool,
    user: Option<AuthUser>,
    account: Option<PlayerRow>,
    lobby: Option<LobbyRef>,
    match_id: Option<u64>,
    login_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LobbyRef {
    Room(u32),
    Ranked(RankedMode),
}

struct Room {
    id: u32,
    name: String,
    host: ConnId,
    members: Vec<RoomMember>,
    options: RoomOptions,
    match_id: Option<u64>,
}

struct RoomMember {
    conn: ConnId,
    faction: Option<String>,
    team: u8,
    ready: bool,
}

struct RankedSlot {
    mode: RankedMode,
    waiting: Vec<RankedMember>,
    match_id: Option<u64>,
}

struct RankedMember {
    conn: ConnId,
    faction: Option<String>,
}

pub struct Server {
    config: ServerConfig,
    data: Arc<GameData>,
    content_hash: u64,
    accounts: Option<Arc<dyn Accounts>>,
    events: Sender<Event>,
    outbox: Outbox,
    clients: HashMap<ConnId, Client>,
    rooms: BTreeMap<u32, Room>,
    next_room: u32,
    ranked: Vec<RankedSlot>,
    matches: HashMap<u64, LiveMatch>,
    next_match: u64,
}

/// Run the server on `listener` until `stop` is set (checked a few times a
/// second). `accounts` enables ranked play.
pub fn run(config: ServerConfig, listener: TcpListener, data: Arc<GameData>, content_hash: u64, accounts: Option<Arc<dyn Accounts>>, stop: Arc<AtomicBool>) {
    listener.set_nonblocking(true).expect("non-blocking listener");
    let (tx, rx) = channel();
    let mut server = Server::new(config, data, content_hash, accounts, tx.clone());
    let mut next_conn: ConnId = 1;
    while !stop.load(Ordering::Relaxed) {
        accept_pending(&listener, &mut next_conn, &tx);
        while let Ok(event) = rx.try_recv() {
            server.handle(event);
        }
        let now = Instant::now();
        server.tick_matches(now);
        server.housekeeping(now);
        let wait = server.next_due().map_or(Duration::from_millis(5), |t| t.saturating_duration_since(Instant::now())).min(Duration::from_millis(5));
        match rx.recv_timeout(wait) {
            Ok(event) => server.handle(event),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

impl Server {
    pub fn new(config: ServerConfig, data: Arc<GameData>, content_hash: u64, accounts: Option<Arc<dyn Accounts>>, events: Sender<Event>) -> Server {
        Server {
            config,
            data,
            content_hash,
            accounts,
            events,
            outbox: Outbox::default(),
            clients: HashMap::new(),
            rooms: BTreeMap::new(),
            next_room: 1,
            ranked: RankedMode::ALL.iter().map(|m| RankedSlot { mode: *m, waiting: Vec::new(), match_id: None }).collect(),
            matches: HashMap::new(),
            next_match: 1,
        }
    }

    fn send(&self, conn: ConnId, msg: ServerMsg) {
        self.outbox.send(conn, msg);
    }

    fn error(&self, conn: ConnId, text: impl Into<String>) {
        self.send(conn, ServerMsg::Error(text.into()));
    }

    /// When the next match tick is due, if any match runs.
    fn next_due(&self) -> Option<Instant> {
        self.matches.values().filter(|m| m.running()).map(|m| m.next_tick_at).min()
    }

    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Connected { conn, addr, tx } => {
                self.outbox.add(conn, tx);
                self.clients.insert(conn, Client { name: addr, hello: false, user: None, account: None, lobby: None, match_id: None, login_pending: false });
            }
            Event::Msg { conn, msg } => {
                if self.clients.contains_key(&conn) {
                    self.on_msg(conn, msg);
                }
            }
            Event::Closed { conn } => self.on_closed(conn),
            Event::Login { conn, result } => {
                let Some(client) = self.clients.get_mut(&conn) else { return };
                client.login_pending = false;
                match result {
                    Ok((user, row)) => {
                        client.user = Some(user);
                        client.account = Some(row.clone());
                        self.send(conn, ServerMsg::LoggedIn(account_of(&row)));
                        self.send(conn, ServerMsg::RankedSlots(self.ranked_slots()));
                    }
                    Err(e) => self.error(conn, e),
                }
            }
            Event::Recorded { match_id, result } => {
                if let Err(e) = result {
                    eprintln!("af_server: match {match_id} was not stored: {e}");
                }
            }
            Event::Leaderboard { conn, result } => match result {
                Ok(rows) => self.send(conn, ServerMsg::Leaderboard(rows)),
                Err(e) => self.error(conn, e),
            },
        }
    }

    fn on_msg(&mut self, conn: ConnId, msg: ClientMsg) {
        let hello = self.clients.get(&conn).is_some_and(|c| c.hello);
        if let ClientMsg::Hello { protocol, content_hash, name } = &msg {
            if hello {
                return;
            }
            if *protocol != PROTOCOL_VERSION {
                self.send(conn, ServerMsg::Refused(format!("this server speaks protocol {PROTOCOL_VERSION}, the game protocol {protocol}: update one of them")));
                self.outbox.close(conn);
                return;
            }
            if *content_hash != self.content_hash {
                self.send(conn, ServerMsg::Refused("the game's content (assets/data) differs from the server's: both need the same version".into()));
                self.outbox.close(conn);
                return;
            }
            let name = self.unique_name(name);
            let client = self.clients.get_mut(&conn).expect("client");
            client.name = name;
            client.hello = true;
            let (ranked_enabled, supabase_url, supabase_anon_key) = match (&self.accounts, &self.config.supabase) {
                (Some(_), Some(s)) => (true, s.url.clone(), s.anon_key.clone()),
                (Some(_), None) => (true, String::new(), String::new()),
                (None, _) => (false, String::new(), String::new()),
            };
            self.send(conn, ServerMsg::Welcome { motd: self.config.motd.clone(), ranked_enabled, supabase_url, supabase_anon_key, email_domain: self.config.email_domain.clone() });
            return;
        }
        if !hello {
            self.send(conn, ServerMsg::Refused("say hello first".into()));
            self.outbox.close(conn);
            return;
        }
        match msg {
            ClientMsg::Hello { .. } => {}
            ClientMsg::Resume { match_id, resume_token, from_tick } => self.resume(conn, match_id, resume_token, from_tick),
            ClientMsg::ListRooms => self.send(conn, ServerMsg::Rooms(self.room_list())),
            ClientMsg::CreateRoom { name } => self.create_room(conn, name),
            ClientMsg::JoinRoom { id } => self.join_room(conn, id),
            ClientMsg::LeaveLobby => self.leave_lobby(conn),
            ClientMsg::SetFaction(f) => self.set_faction(conn, f),
            ClientMsg::SetTeam(t) => self.set_team(conn, t),
            ClientMsg::SetReady(r) => self.set_ready(conn, r),
            ClientMsg::SetOptions(o) => self.set_options(conn, o),
            ClientMsg::Start => self.start_room(conn),
            ClientMsg::Login { access_token } => self.login(conn, access_token),
            ClientMsg::Logout => {
                if matches!(self.clients.get(&conn).and_then(|c| c.lobby), Some(LobbyRef::Ranked(_))) {
                    self.leave_lobby(conn);
                }
                if let Some(c) = self.clients.get_mut(&conn) {
                    c.user = None;
                    c.account = None;
                }
                self.send(conn, ServerMsg::LoggedOut);
            }
            ClientMsg::JoinRanked(mode) => self.join_ranked(conn, mode),
            ClientMsg::RefreshRanked => self.send(conn, ServerMsg::RankedSlots(self.ranked_slots())),
            ClientMsg::Leaderboard => {
                let Some(accounts) = self.accounts.clone() else {
                    self.error(conn, "this server has no ranked play");
                    return;
                };
                let events = self.events.clone();
                thread::spawn(move || {
                    let _ = events.send(Event::Leaderboard { conn, result: accounts.leaderboard(LEADERBOARD_ROWS) });
                });
            }
            ClientMsg::Loaded => {
                if let Some(m) = self.match_of(conn)
                    && let Some(p) = m.participant_by_conn(conn)
                {
                    p.loaded = true;
                }
            }
            ClientMsg::Commands(cmds) => {
                if let Some(m) = self.match_of(conn)
                    && let Some(slot) = m.participant_by_conn(conn).map(|p| p.slot)
                {
                    m.queue(slot, cmds);
                }
            }
            ClientMsg::Hash { tick, hash } => self.hash_report(conn, tick, hash),
            ClientMsg::MatchOver { winning_team, time } => self.casual_over(conn, winning_team, time),
            ClientMsg::LeaveMatch => self.leave_match(conn),
            ClientMsg::Ping(n) => self.send(conn, ServerMsg::Pong(n)),
        }
    }

    fn on_closed(&mut self, conn: ConnId) {
        self.outbox.remove(conn);
        if self.clients.get(&conn).is_some_and(|c| c.lobby.is_some()) {
            self.leave_lobby(conn);
        }
        if let Some(id) = self.clients.get(&conn).and_then(|c| c.match_id)
            && let Some(m) = self.matches.get_mut(&id)
            && let Some(p) = m.participant_by_conn(conn)
        {
            p.conn = None;
            p.dropped_at = Some(Instant::now());
        }
        self.clients.remove(&conn);
    }

    /// `name`, or `name (2)` and so on when another client has it.
    fn unique_name(&self, name: &str) -> String {
        let base: String = name.trim().chars().filter(|c| !c.is_control()).take(24).collect();
        let base = if base.is_empty() { "Player".to_string() } else { base };
        let taken = |n: &str| self.clients.values().any(|c| c.hello && c.name == n);
        if !taken(&base) {
            return base;
        }
        (2..).map(|i| format!("{base} ({i})")).find(|n| !taken(n)).expect("a free name")
    }

    fn match_of(&mut self, conn: ConnId) -> Option<&mut LiveMatch> {
        let id = self.clients.get(&conn)?.match_id?;
        self.matches.get_mut(&id)
    }

    // -------------------------------------------------------------------
    // Casual rooms
    // -------------------------------------------------------------------

    fn room_list(&self) -> Vec<RoomSummary> {
        self.rooms
            .values()
            .map(|r| RoomSummary { id: r.id, name: r.name.clone(), humans: r.members.len() as u8, max_humans: CASUAL_MAX_HUMANS as u8, playing: r.match_id.is_some() })
            .collect()
    }

    /// Everyone in the menu gets the room list when it changes.
    fn broadcast_rooms(&self) {
        let rooms = self.room_list();
        for (conn, c) in &self.clients {
            if c.hello && c.lobby.is_none() && c.match_id.is_none() {
                self.send(*conn, ServerMsg::Rooms(rooms.clone()));
            }
        }
    }

    fn room_view(&self, room: &Room, viewer: ConnId) -> LobbyView {
        let mut slots: Vec<SlotView> = room
            .members
            .iter()
            .map(|m| SlotView {
                name: self.clients.get(&m.conn).map_or_else(|| "?".into(), |c| c.name.clone()),
                faction: m.faction.clone(),
                team: m.team,
                ready: m.ready,
                host: m.conn == room.host,
                you: m.conn == viewer,
                ai: false,
                elo: None,
            })
            .collect();
        for i in 0..room.options.ai_players {
            slots.push(SlotView { name: format!("Computer {}", i + 1), faction: None, team: ai_team(&room.members, i), ready: true, host: false, you: false, ai: true, elo: None });
        }
        LobbyView {
            id: room.id,
            name: room.name.clone(),
            kind: LobbyKind::Casual,
            slots,
            max_humans: CASUAL_MAX_HUMANS as u8,
            options: room.options,
            host_is_you: room.host == viewer,
            playing: room.match_id.is_some(),
        }
    }

    fn broadcast_room(&self, id: u32) {
        let Some(room) = self.rooms.get(&id) else { return };
        for m in &room.members {
            self.send(m.conn, ServerMsg::Lobby(self.room_view(room, m.conn)));
        }
    }

    fn create_room(&mut self, conn: ConnId, name: String) {
        let Some(client) = self.clients.get(&conn) else { return };
        if client.lobby.is_some() || client.match_id.is_some() {
            self.error(conn, "leave your current lobby first");
            return;
        }
        let name: String = name.trim().chars().filter(|c| !c.is_control()).take(32).collect();
        let name = if name.is_empty() { format!("{}'s room", client.name) } else { name };
        let id = self.next_room;
        self.next_room += 1;
        let team = 0;
        self.rooms.insert(id, Room { id, name, host: conn, members: vec![RoomMember { conn, faction: None, team, ready: false }], options: RoomOptions::default(), match_id: None });
        self.clients.get_mut(&conn).expect("client").lobby = Some(LobbyRef::Room(id));
        self.broadcast_room(id);
        self.broadcast_rooms();
    }

    fn join_room(&mut self, conn: ConnId, id: u32) {
        let Some(client) = self.clients.get(&conn) else { return };
        if client.lobby.is_some() || client.match_id.is_some() {
            self.error(conn, "leave your current lobby first");
            return;
        }
        let Some(room) = self.rooms.get_mut(&id) else {
            self.error(conn, "that room is gone");
            return;
        };
        if room.match_id.is_some() {
            self.error(conn, "that room is playing");
            return;
        }
        if room.members.len() >= CASUAL_MAX_HUMANS {
            self.error(conn, "that room is full");
            return;
        }
        let team = (0..MAX_MATCH_PLAYERS as u8).find(|t| !room.members.iter().any(|m| m.team == *t)).unwrap_or(0);
        room.members.push(RoomMember { conn, faction: None, team, ready: false });
        if room.members.len() + room.options.ai_players as usize > MAX_MATCH_PLAYERS {
            room.options.ai_players = (MAX_MATCH_PLAYERS - room.members.len()) as u8;
        }
        self.clients.get_mut(&conn).expect("client").lobby = Some(LobbyRef::Room(id));
        self.broadcast_room(id);
        self.broadcast_rooms();
    }

    fn leave_lobby(&mut self, conn: ConnId) {
        let Some(client) = self.clients.get_mut(&conn) else { return };
        let Some(lobby) = client.lobby.take() else { return };
        self.send(conn, ServerMsg::LeftLobby);
        match lobby {
            LobbyRef::Room(id) => {
                let mut gone = false;
                if let Some(room) = self.rooms.get_mut(&id) {
                    room.members.retain(|m| m.conn != conn);
                    if room.members.is_empty() {
                        gone = room.match_id.is_none();
                    } else if room.host == conn {
                        room.host = room.members[0].conn;
                    }
                }
                if gone {
                    self.rooms.remove(&id);
                } else {
                    self.broadcast_room(id);
                }
                self.broadcast_rooms();
            }
            LobbyRef::Ranked(mode) => {
                if let Some(slot) = self.ranked.iter_mut().find(|s| s.mode == mode) {
                    slot.waiting.retain(|m| m.conn != conn);
                }
                self.broadcast_ranked(mode);
                self.broadcast_ranked_slots();
            }
        }
    }

    fn set_faction(&mut self, conn: ConnId, faction: Option<String>) {
        if let Some(f) = &faction
            && self.data.faction_id(f).is_none()
        {
            self.error(conn, format!("unknown faction `{f}`"));
            return;
        }
        match self.clients.get(&conn).and_then(|c| c.lobby) {
            Some(LobbyRef::Room(id)) => {
                if let Some(room) = self.rooms.get_mut(&id)
                    && let Some(m) = room.members.iter_mut().find(|m| m.conn == conn)
                {
                    m.faction = faction;
                }
                self.broadcast_room(id);
            }
            Some(LobbyRef::Ranked(mode)) => {
                if let Some(slot) = self.ranked.iter_mut().find(|s| s.mode == mode)
                    && let Some(m) = slot.waiting.iter_mut().find(|m| m.conn == conn)
                {
                    m.faction = faction;
                }
                self.broadcast_ranked(mode);
            }
            None => {}
        }
    }

    fn set_team(&mut self, conn: ConnId, team: u8) {
        let Some(LobbyRef::Room(id)) = self.clients.get(&conn).and_then(|c| c.lobby) else { return };
        if team as usize >= MAX_MATCH_PLAYERS {
            return;
        }
        if let Some(room) = self.rooms.get_mut(&id)
            && let Some(m) = room.members.iter_mut().find(|m| m.conn == conn)
        {
            m.team = team;
        }
        self.broadcast_room(id);
    }

    fn set_ready(&mut self, conn: ConnId, ready: bool) {
        let Some(LobbyRef::Room(id)) = self.clients.get(&conn).and_then(|c| c.lobby) else { return };
        if let Some(room) = self.rooms.get_mut(&id)
            && let Some(m) = room.members.iter_mut().find(|m| m.conn == conn)
        {
            m.ready = ready;
        }
        self.broadcast_room(id);
    }

    fn set_options(&mut self, conn: ConnId, mut options: RoomOptions) {
        let Some(LobbyRef::Room(id)) = self.clients.get(&conn).and_then(|c| c.lobby) else { return };
        let Some(room) = self.rooms.get_mut(&id) else { return };
        if room.host != conn {
            self.error(conn, "only the host changes the settings");
            return;
        }
        options.ai_players = options.ai_players.min((MAX_MATCH_PLAYERS - room.members.len()) as u8);
        room.options = options;
        self.broadcast_room(id);
    }

    fn start_room(&mut self, conn: ConnId) {
        let Some(LobbyRef::Room(id)) = self.clients.get(&conn).and_then(|c| c.lobby) else { return };
        let Some(room) = self.rooms.get(&id) else { return };
        if room.host != conn {
            self.error(conn, "only the host starts the match");
            return;
        }
        if room.match_id.is_some() {
            return;
        }
        if room.members.len() + (room.options.ai_players as usize) < 2 {
            self.error(conn, "a match needs at least two players (add a computer player)");
            return;
        }
        let match_id = self.next_match;
        self.next_match += 1;
        let seed = fresh_seed(match_id);
        let mut rng = SimRng::new(seed ^ 0x5EED);
        let mut players = Vec::new();
        let mut participants = Vec::new();
        for (i, m) in room.members.iter().enumerate() {
            let name = self.clients.get(&m.conn).map_or_else(|| "?".into(), |c| c.name.clone());
            players.push(MatchPlayer { name: name.clone(), faction: self.pick_faction(&m.faction, &mut rng), team: m.team, ai: false });
            participants.push(Participant::new(i as u8, m.conn, name, None, 0, rng.next_u64()));
        }
        for i in 0..room.options.ai_players {
            let faction = self.pick_faction(&None, &mut rng);
            let fname = self.data.faction_id(&faction).map_or(faction.clone(), |f| self.data.faction(f).name.clone());
            players.push(MatchPlayer { name: format!("{fname} (AI)"), faction, team: ai_team(&room.members, i), ai: true });
        }
        let now = Instant::now();
        let live = LiveMatch::new(match_id, false, None, seed, players, room.options.fog, room.options.rule, participants, None, now);
        for p in &live.participants {
            if let Some(c) = p.conn {
                self.send(c, ServerMsg::StartMatch(live.start_for(Some(p.slot))));
                if let Some(client) = self.clients.get_mut(&c) {
                    client.match_id = Some(match_id);
                }
            }
        }
        self.matches.insert(match_id, live);
        if let Some(room) = self.rooms.get_mut(&id) {
            room.match_id = Some(match_id);
            for m in &mut room.members {
                m.ready = false;
            }
        }
        self.broadcast_room(id);
        self.broadcast_rooms();
    }

    /// `choice`, or a faction drawn from `rng`.
    fn pick_faction(&self, choice: &Option<String>, rng: &mut SimRng) -> String {
        match choice {
            Some(f) if self.data.faction_id(f).is_some() => f.clone(),
            _ => {
                let n = self.data.factions.len().max(1) as u32;
                self.data.factions.get(rng.below(n) as usize).map_or_else(String::new, |f| f.id.clone())
            }
        }
    }

    /// A casual match ended on a client.
    fn casual_over(&mut self, conn: ConnId, winning_team: Option<u8>, _time: f32) {
        let Some(id) = self.clients.get(&conn).and_then(|c| c.match_id) else { return };
        let Some(m) = self.matches.get(&id) else { return };
        if m.ranked || m.over.is_some() {
            return;
        }
        let forfeited: Vec<String> = m.participants.iter().filter(|p| p.forfeited).map(|p| p.name.clone()).collect();
        let result = MatchResultView { winning_team, changes: Vec::new(), forfeited, note: String::new() };
        self.finish_match(id, result);
    }

    /// The match is over: tell everyone, free the room or slot, drop it.
    fn finish_match(&mut self, id: u64, result: MatchResultView) {
        let Some(mut m) = self.matches.remove(&id) else { return };
        m.over = Some(result.clone());
        for c in m.all_connections() {
            self.send(c, ServerMsg::MatchEnded(result.clone()));
        }
        for client in self.clients.values_mut() {
            if client.match_id == Some(id) {
                client.match_id = None;
            }
        }
        if let Some(room_id) = self.rooms.values().find(|r| r.match_id == Some(id)).map(|r| r.id) {
            let empty = {
                let room = self.rooms.get_mut(&room_id).expect("room");
                room.match_id = None;
                room.members.is_empty()
            };
            if empty {
                self.rooms.remove(&room_id);
            } else {
                self.broadcast_room(room_id);
            }
            self.broadcast_rooms();
        }
        if let Some(slot) = self.ranked.iter_mut().find(|s| s.match_id == Some(id)) {
            slot.match_id = None;
            self.broadcast_ranked_slots();
        }
    }

    fn leave_match(&mut self, conn: ConnId) {
        let Some(id) = self.clients.get(&conn).and_then(|c| c.match_id) else { return };
        if let Some(client) = self.clients.get_mut(&conn) {
            client.match_id = None;
        }
        let Some(m) = self.matches.get_mut(&id) else { return };
        let ranked = m.ranked;
        // The connection stays on the participant: no more turns go to it,
        // but the result will.
        if let Some(p) = m.participant_by_conn(conn) {
            p.left = true;
            let slot = p.slot;
            m.resign(slot);
        }
        let everyone_left = m.participants.iter().all(|p| !p.active());
        if !ranked && everyone_left {
            self.finish_match(id, MatchResultView { note: "everyone left".into(), ..Default::default() });
        }
    }

    fn hash_report(&mut self, conn: ConnId, tick: u64, hash: u64) {
        let Some(id) = self.clients.get(&conn).and_then(|c| c.match_id) else { return };
        let Some(m) = self.matches.get_mut(&id) else { return };
        let Some(slot) = m.participant_by_conn(conn).map(|p| p.slot) else { return };
        let verdict = m.report_hash(slot, tick, hash);
        let HashVerdict::Desync(slots) = verdict else { return };
        let names: Vec<String> = m.participants.iter().filter(|p| slots.contains(&p.slot)).map(|p| p.name.clone()).collect();
        let connections = m.connections();
        let ranked = m.ranked;
        if ranked && self.config.desync_forfeits {
            for s in &slots {
                m.resign(*s);
            }
        }
        for c in connections {
            self.outbox.send(c, ServerMsg::Desync { tick, players: names.clone() });
        }
        if ranked && !self.config.desync_forfeits {
            let note = format!("void: {} desynced from the server at tick {tick}", names.join(", "));
            self.end_ranked(id, None, note);
        }
    }

    // -------------------------------------------------------------------
    // Ranked play
    // -------------------------------------------------------------------

    fn login(&mut self, conn: ConnId, access_token: String) {
        let Some(accounts) = self.accounts.clone() else {
            self.error(conn, "this server has no ranked play");
            return;
        };
        let Some(client) = self.clients.get_mut(&conn) else { return };
        if client.login_pending {
            return;
        }
        client.login_pending = true;
        let events = self.events.clone();
        thread::spawn(move || {
            let result = accounts.verify(&access_token).and_then(|user| accounts.get_or_create_player(&user).map(|row| (user, row)));
            let _ = events.send(Event::Login { conn, result });
        });
    }

    fn ranked_slots(&self) -> Vec<RankedSlotView> {
        self.ranked.iter().map(|s| RankedSlotView { mode: s.mode, waiting: s.waiting.len() as u8, playing: s.match_id.is_some() }).collect()
    }

    fn broadcast_ranked_slots(&self) {
        let slots = self.ranked_slots();
        for (conn, c) in &self.clients {
            if c.account.is_some() && c.match_id.is_none() {
                self.send(*conn, ServerMsg::RankedSlots(slots.clone()));
            }
        }
    }

    fn ranked_view(&self, slot: &RankedSlot, viewer: ConnId) -> LobbyView {
        let slots = slot
            .waiting
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let client = self.clients.get(&m.conn);
                SlotView {
                    name: client.map_or_else(|| "?".into(), |c| c.account.as_ref().map_or_else(|| c.name.clone(), |a| a.username.clone())),
                    faction: m.faction.clone(),
                    team: slot.mode.team_of(i),
                    ready: true,
                    host: false,
                    you: m.conn == viewer,
                    ai: false,
                    elo: client.and_then(|c| c.account.as_ref()).map(|a| a.elo),
                }
            })
            .collect();
        LobbyView {
            id: 0,
            name: format!("Ranked {}", slot.mode.label()),
            kind: LobbyKind::Ranked(slot.mode),
            slots,
            max_humans: slot.mode.players() as u8,
            options: RoomOptions { fog: true, rule: slot.mode.rule(), ai_players: 0 },
            host_is_you: false,
            playing: slot.match_id.is_some(),
        }
    }

    fn broadcast_ranked(&self, mode: RankedMode) {
        let Some(slot) = self.ranked.iter().find(|s| s.mode == mode) else { return };
        for m in &slot.waiting {
            self.send(m.conn, ServerMsg::Lobby(self.ranked_view(slot, m.conn)));
        }
    }

    fn join_ranked(&mut self, conn: ConnId, mode: RankedMode) {
        let Some(client) = self.clients.get(&conn) else { return };
        if client.account.is_none() {
            self.error(conn, "log in first");
            return;
        }
        if client.lobby.is_some() || client.match_id.is_some() {
            self.error(conn, "leave your current lobby first");
            return;
        }
        let Some(slot) = self.ranked.iter_mut().find(|s| s.mode == mode) else { return };
        if slot.match_id.is_some() {
            self.error(conn, format!("a ranked {} match is being played; try again when it ends", mode.label()));
            return;
        }
        if slot.waiting.len() >= mode.players() {
            self.error(conn, "that queue is full");
            return;
        }
        slot.waiting.push(RankedMember { conn, faction: None });
        self.clients.get_mut(&conn).expect("client").lobby = Some(LobbyRef::Ranked(mode));
        let full = slot.waiting.len() >= mode.players();
        self.broadcast_ranked(mode);
        self.broadcast_ranked_slots();
        if full {
            self.start_ranked(mode);
        }
    }

    fn start_ranked(&mut self, mode: RankedMode) {
        let Some(slot) = self.ranked.iter_mut().find(|s| s.mode == mode) else { return };
        let waiting: Vec<RankedMember> = std::mem::take(&mut slot.waiting);
        let match_id = self.next_match;
        self.next_match += 1;
        let seed = fresh_seed(match_id);
        let mut rng = SimRng::new(seed ^ 0x5EED);
        let mut players = Vec::new();
        let mut participants = Vec::new();
        for (i, m) in waiting.iter().enumerate() {
            let (name, user_id, elo) = match self.clients.get(&m.conn).and_then(|c| c.account.as_ref().map(|a| (a, c))) {
                Some((a, _)) => (a.username.clone(), Some(a.user_id.clone()), a.elo),
                None => ("?".into(), None, elo::START),
            };
            players.push(MatchPlayer { name: name.clone(), faction: self.pick_faction(&m.faction, &mut rng), team: mode.team_of(i), ai: false });
            participants.push(Participant::new(i as u8, m.conn, name, user_id, elo, rng.next_u64()));
        }
        let now = Instant::now();
        let setup = af_net::client::match_setup(&self.data, &MatchStart { match_id, seed, players: players.clone(), your_slot: None, fog: true, rule: mode.rule(), ranked: true, resume_token: 0 });
        let referee = af_sim::referee_app(self.data.clone(), setup);
        let live = LiveMatch::new(match_id, true, Some(mode), seed, players, true, mode.rule(), participants, Some(referee), now);
        for p in &live.participants {
            if let Some(c) = p.conn {
                self.send(c, ServerMsg::StartMatch(live.start_for(Some(p.slot))));
                if let Some(client) = self.clients.get_mut(&c) {
                    client.match_id = Some(match_id);
                    client.lobby = None;
                }
            }
        }
        self.matches.insert(match_id, live);
        if let Some(slot) = self.ranked.iter_mut().find(|s| s.mode == mode) {
            slot.match_id = Some(match_id);
        }
        self.broadcast_ranked_slots();
    }

    /// A ranked match is decided (or void): ratings, the record, the result.
    fn end_ranked(&mut self, id: u64, winning_team: Option<u8>, note: String) {
        let Some(m) = self.matches.get(&id) else { return };
        let rated = note.is_empty();
        let teams: Vec<(u8, i32)> = m.participants.iter().map(|p| (m.players.get(p.slot as usize).map_or(p.slot, |mp| mp.team), p.elo_before)).collect();
        let deltas = if rated { elo::changes(&teams, winning_team) } else { vec![0; teams.len()] };
        let mut changes = Vec::new();
        let mut record_players = Vec::new();
        for (p, delta) in m.participants.iter().zip(&deltas) {
            let after = p.elo_before + delta;
            changes.push(RatingChange { username: p.name.clone(), before: p.elo_before, after });
            let team = m.players.get(p.slot as usize).map_or(p.slot, |mp| mp.team);
            record_players.push(MatchRecordPlayer {
                user_id: p.user_id.clone().unwrap_or_default(),
                slot: p.slot,
                team,
                faction: m.players.get(p.slot as usize).map_or_else(String::new, |mp| mp.faction.clone()),
                elo_before: p.elo_before,
                elo_after: after,
                won: rated && Some(team) == winning_team,
            });
        }
        let forfeited: Vec<String> = m.participants.iter().filter(|p| p.forfeited).map(|p| p.name.clone()).collect();
        let record = MatchRecord { mode: m.mode.map_or("", |md| md.id()).to_string(), seed: m.seed, winning_team, duration_secs: m.elapsed_secs(), note: note.clone(), players: record_players };
        // The connected players see their new rating at once.
        let updates: Vec<(Option<ConnId>, i32, bool, bool)> =
            m.participants.iter().zip(&deltas).map(|(p, d)| (p.conn, *d, rated && Some(m.players.get(p.slot as usize).map_or(p.slot, |mp| mp.team)) == winning_team, rated)).collect();
        for (conn, delta, won, rated) in updates {
            let Some(conn) = conn else { continue };
            if let Some(client) = self.clients.get_mut(&conn)
                && let Some(account) = client.account.as_mut()
            {
                account.elo += delta;
                if rated {
                    if won {
                        account.wins += 1;
                    } else {
                        account.losses += 1;
                    }
                }
                let view = account_of(account);
                self.send(conn, ServerMsg::LoggedIn(view));
            }
        }
        if let Some(accounts) = self.accounts.clone() {
            let events = self.events.clone();
            thread::spawn(move || {
                let _ = events.send(Event::Recorded { match_id: id, result: accounts.record_match(&record) });
            });
        }
        self.finish_match(id, MatchResultView { winning_team, changes, forfeited, note });
    }

    // -------------------------------------------------------------------
    // Resume, ticking, housekeeping
    // -------------------------------------------------------------------

    fn resume(&mut self, conn: ConnId, match_id: u64, resume_token: u64, from_tick: u64) {
        let Some(m) = self.matches.get_mut(&match_id) else {
            self.error(conn, "that match is over");
            return;
        };
        let Some(p) = m.participants.iter_mut().find(|p| p.resume_token == resume_token && resume_token != 0) else {
            self.error(conn, "that match is not yours");
            return;
        };
        if p.left {
            self.error(conn, "you left that match");
            return;
        }
        if let Some(old) = p.conn.replace(conn)
            && old != conn
        {
            self.outbox.close(old);
            if let Some(c) = self.clients.get_mut(&old) {
                c.match_id = None;
            }
        }
        p.dropped_at = None;
        let slot = p.slot;
        if let Some(client) = self.clients.get_mut(&conn) {
            client.match_id = Some(match_id);
        }
        let start = m.start_for(Some(slot));
        let first = from_tick.max(1) as usize;
        let turns: Vec<(u64, Vec<_>)> = m.history.iter().enumerate().skip(first.saturating_sub(1)).map(|(i, t)| (i as u64 + 1, t.clone())).collect();
        self.send(conn, ServerMsg::StartMatch(start));
        self.send(conn, ServerMsg::Turns(turns));
    }

    fn tick_matches(&mut self, now: Instant) {
        let ids: Vec<u64> = self.matches.keys().copied().collect();
        for id in ids {
            let mut decided = None;
            if let Some(m) = self.matches.get_mut(&id) {
                let mut passes = 0;
                while m.due(now) && passes < MAX_TICKS_PER_PASS {
                    passes += 1;
                    let (tick, turn) = m.step();
                    let msg = ServerMsg::Turn { tick, commands: turn };
                    for c in m.connections() {
                        self.outbox.send(c, msg.clone());
                    }
                    if let Some(outcome) = m.referee_outcome() {
                        decided = Some(outcome.winning_team);
                        break;
                    }
                }
                if passes == MAX_TICKS_PER_PASS && m.due(now) {
                    // Too far behind (the machine stalled): resync the clock
                    // rather than racing to catch up.
                    m.next_tick_at = now;
                }
            }
            if let Some(winning_team) = decided {
                self.end_ranked(id, winning_team, String::new());
            }
        }
    }

    fn housekeeping(&mut self, now: Instant) {
        let load_timeout = self.config.load_timeout;
        let forfeit_after = self.config.forfeit_after;
        let mut to_drop = Vec::new();
        let mut forfeits = Vec::new();
        for m in self.matches.values_mut() {
            if !m.started && (m.all_loaded() || (m.any_loaded() && now.duration_since(m.created) > load_timeout)) {
                m.start_clock(now);
            }
            let nobody = m.deserted();
            if !m.ranked && nobody && m.participants.iter().all(|p| !p.active() || p.dropped_at.is_some_and(|t| now.duration_since(t) > ABANDONED_AFTER)) {
                to_drop.push(m.id);
            }
            if m.ranked {
                for p in &m.participants {
                    if p.active() && !p.forfeited && p.conn.is_none() && p.dropped_at.is_some_and(|t| now.duration_since(t) > forfeit_after) {
                        forfeits.push((m.id, p.slot));
                    }
                }
                if nobody && m.participants.iter().all(|p| p.dropped_at.is_some_and(|t| now.duration_since(t) > forfeit_after) || !p.active()) {
                    to_drop.push(m.id);
                }
            }
        }
        for (id, slot) in forfeits {
            if let Some(m) = self.matches.get_mut(&id) {
                m.resign(slot);
                if let Some(p) = m.participant_by_slot(slot) {
                    p.left = true;
                }
            }
        }
        for id in to_drop {
            let ranked = self.matches.get(&id).is_some_and(|m| m.ranked);
            if ranked {
                self.end_ranked(id, None, "void: every player left".into());
            } else {
                self.finish_match(id, MatchResultView { note: "everyone left".into(), ..Default::default() });
            }
        }
    }
}

fn account_of(row: &PlayerRow) -> Account {
    Account { username: row.username.clone(), elo: row.elo, wins: row.wins, losses: row.losses }
}

/// A seed nobody can guess ahead of the match.
fn fresh_seed(match_id: u64) -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    SimRng::new(nanos ^ match_id.rotate_left(32)).next_u64()
}

/// The team of the `i`th computer player of a room: the lowest team no
/// human sits on, counting up.
fn ai_team(members: &[RoomMember], i: u8) -> u8 {
    let free: Vec<u8> = (0..MAX_MATCH_PLAYERS as u8).filter(|t| !members.iter().any(|m| m.team == *t)).collect();
    free.get(i as usize).copied().unwrap_or(((MAX_MATCH_PLAYERS as u8).saturating_sub(1)).saturating_sub(i % MAX_MATCH_PLAYERS as u8))
}
