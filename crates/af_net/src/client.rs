//! The multiplayer client: [`NetClientPlugin`].
//!
//! * Connection: a connect thread, then a reader and a writer thread per
//!   connection; the Bevy side polls their channels every frame
//!   ([`poll_link`]) and drains the UI's `NetActions` ([`apply_actions`]).
//!   Everything the UI needs to know lands in `NetSession`.
//! * Lobby: rooms, the lobby view, ranked slots and the account mirror the
//!   server's messages one for one.
//! * Match: `StartMatch` builds the same `MatchSetup` as every other peer
//!   and enters `GameState::Playing`. From then on the server's turns feed
//!   `CommandInbox` right before each tick ([`inject_turn`]), the local
//!   player's commands leave `CommandInbox` for the server instead of the
//!   sim ([`route_local_commands`]), the fixed clock advances only as far as
//!   the turns in hand allow ([`pace_clock`], via
//!   `TimeUpdateStrategy::ManualDuration`), and a state hash goes to the
//!   server every [`HASH_EVERY`] ticks ([`send_hash`]). A lost connection
//!   is retried and the match resumed from the next tick needed.
//!
//! Sign-up and login call the account service on a thread of their own;
//! the token that comes back is what the server gets.

use std::io::Write;
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use af_core::ext::map::MapOptions;
use af_core::ext::net::*;
use af_core::hash::sim_hash;
use af_core::prelude::*;
use bevy::prelude::*;
use bevy::time::{TimeSystems, TimeUpdateStrategy};

use crate::auth::SupabaseAuth;
use crate::config::ClientConfig;
use crate::framing::{read_frame, write_frame};
use crate::lockstep::{Pacer, TurnBuffer};
use crate::protocol::{decode, encode, ClientMsg, ServerMsg, WireCommand, PROTOCOL_VERSION};

/// A state hash goes to the server every this many sim ticks.
pub const HASH_EVERY: u64 = 10;
/// Seconds between pings.
const PING_EVERY: Duration = Duration::from_secs(2);
/// Seconds between reconnection attempts while a match is running.
const RECONNECT_EVERY: Duration = Duration::from_secs(3);
/// Give up resuming after this long without the server.
const RESUME_FOR: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);

/// Adds the multiplayer client. Needs `CorePlugin` (and the sim plugins to
/// play a match); works headless.
pub struct NetClientPlugin;

impl Plugin for NetClientPlugin {
    fn build(&self, app: &mut App) {
        let config = ClientConfig::load();
        let content_hash = af_data::find_data_dir().and_then(|d| af_data::content_hash(&d).ok()).unwrap_or(0);
        let mut session = NetSession { server: config.server.clone(), saved_username: config.username.clone(), ..default() };
        session.name = if config.name.is_empty() { "Player".into() } else { config.name.clone() };
        app.insert_resource(session)
            .init_resource::<NetActions>()
            .insert_resource(Link { config, content_hash, ..default() })
            .add_systems(First, pace_clock.before(TimeSystems).run_if(resource_exists::<Lockstep>))
            .add_systems(PreUpdate, (apply_actions, poll_link, poll_auth).chain())
            .add_systems(FixedUpdate, inject_turn.before(SimSet::Prepare).run_if(resource_exists::<Lockstep>))
            .add_systems(FixedPostUpdate, send_hash.run_if(resource_exists::<Lockstep>))
            .add_systems(Update, (send_loaded, watch_outcome, send_ping, update_lockstep_view).run_if(resource_exists::<Lockstep>))
            .add_systems(PostUpdate, route_local_commands.run_if(resource_exists::<Lockstep>))
            .add_systems(OnExit(GameState::Playing), leave_match);
    }
}

/// What the connection threads report.
enum LinkEvent {
    /// The TCP connection is up; `tx` reaches the writer thread.
    Connected { tx: Sender<ClientMsg>, stream: TcpStream },
    Msg(ServerMsg),
    Closed(String),
}

enum LinkState {
    Idle,
    Connecting(Receiver<LinkEvent>),
    Up { tx: Sender<ClientMsg>, rx: Receiver<LinkEvent>, stream: TcpStream },
}

impl Default for LinkState {
    fn default() -> Self {
        LinkState::Idle
    }
}

/// The connection and what it has learnt. One per app.
#[derive(Resource, Default)]
struct Link {
    state: Mutex<LinkState>,
    /// Said `Hello` and got `Welcome`.
    welcomed: bool,
    /// From `Welcome`: how to reach the account service.
    auth: Option<SupabaseAuth>,
    /// A sign-up or login in flight.
    auth_pending: Option<Mutex<Receiver<Result<(String, String), String>>>>,
    config: ClientConfig,
    content_hash: u64,
    last_ping: Option<Instant>,
    ping_sent: Option<(u64, Instant)>,
    /// A match to resume if the connection drops and comes back.
    resume: Option<(u64, u64)>,
    /// Trying to get back into the match: when the connection dropped and
    /// the last attempt.
    reconnecting: Option<(Instant, Instant)>,
}

impl Link {
    fn send(&self, msg: ClientMsg) {
        if let LinkState::Up { tx, .. } = &*self.state.lock().unwrap() {
            let _ = tx.send(msg);
        }
    }

    fn is_up(&self) -> bool {
        matches!(&*self.state.lock().unwrap(), LinkState::Up { .. })
    }

    fn disconnect(&mut self) {
        let state = std::mem::take(&mut *self.state.lock().unwrap());
        if let LinkState::Up { stream, .. } = state {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.welcomed = false;
        self.ping_sent = None;
    }

    fn connect(&mut self, addr: String) {
        self.disconnect();
        let (events_tx, events_rx) = channel();
        thread::Builder::new()
            .name("af_net connect".into())
            .spawn(move || connect_thread(addr, events_tx))
            .expect("spawn connect thread");
        *self.state.lock().unwrap() = LinkState::Connecting(events_rx);
    }
}

/// Resolve and connect; on success start the reader and writer threads.
fn connect_thread(addr: String, events: Sender<LinkEvent>) {
    let targets = match addr.to_socket_addrs() {
        Ok(t) => t.collect::<Vec<_>>(),
        Err(e) => {
            let _ = events.send(LinkEvent::Closed(format!("{addr}: {e}")));
            return;
        }
    };
    let mut last_err = format!("{addr}: no address");
    for target in targets {
        match TcpStream::connect_timeout(&target, CONNECT_TIMEOUT) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                let (tx, rx) = channel::<ClientMsg>();
                let Ok(reader) = stream.try_clone() else {
                    let _ = events.send(LinkEvent::Closed("could not clone the socket".into()));
                    return;
                };
                let Ok(mut writer) = stream.try_clone() else {
                    let _ = events.send(LinkEvent::Closed("could not clone the socket".into()));
                    return;
                };
                let _ = events.send(LinkEvent::Connected { tx, stream });
                let read_events = events.clone();
                thread::Builder::new()
                    .name("af_net read".into())
                    .spawn(move || {
                        let mut reader = reader;
                        loop {
                            match read_frame(&mut reader) {
                                Ok(bytes) => match decode::<ServerMsg>(&bytes) {
                                    Ok(msg) => {
                                        if read_events.send(LinkEvent::Msg(msg)).is_err() {
                                            return;
                                        }
                                    }
                                    Err(e) => {
                                        let _ = read_events.send(LinkEvent::Closed(e));
                                        return;
                                    }
                                },
                                Err(e) => {
                                    let _ = read_events.send(LinkEvent::Closed(format!("connection lost: {e}")));
                                    return;
                                }
                            }
                        }
                    })
                    .expect("spawn reader thread");
                thread::Builder::new()
                    .name("af_net write".into())
                    .spawn(move || {
                        while let Ok(msg) = rx.recv() {
                            if write_frame(&mut writer, &encode(&msg)).is_err() {
                                return;
                            }
                        }
                        let _ = writer.flush();
                        let _ = writer.shutdown(Shutdown::Both);
                    })
                    .expect("spawn writer thread");
                return;
            }
            Err(e) => last_err = format!("{addr}: {e}"),
        }
    }
    let _ = events.send(LinkEvent::Closed(last_err));
}

/// The networked match in progress.
#[derive(Resource)]
struct Lockstep {
    buffer: TurnBuffer,
    pacer: Pacer,
    ranked: bool,
    loaded_sent: bool,
    outcome_sent: bool,
    last_frame: Option<Instant>,
    stall_since: Option<Instant>,
    /// The server named this player among those whose game parted.
    desynced: bool,
    /// This player's name in the match (`None` when spectating): the
    /// server's desync reports name the players whose game parted.
    name: Option<String>,
    /// The server declared the match over.
    ended: bool,
}

impl Lockstep {
    fn new(start: &MatchStart) -> Lockstep {
        Lockstep {
            buffer: TurnBuffer::new(),
            pacer: Pacer::default(),
            ranked: start.ranked,
            loaded_sent: false,
            outcome_sent: false,
            last_frame: None,
            stall_since: None,
            desynced: false,
            name: start.your_slot.and_then(|s| start.players.get(s as usize)).map(|p| p.name.clone()),
            ended: false,
        }
    }

    /// Does a desync report naming `players` (none: everyone) say this
    /// game no longer matches? Another player's parting leaves ours with
    /// the rest.
    fn parted(&self, players: &[String]) -> bool {
        players.is_empty() || self.name.as_ref().is_some_and(|n| players.contains(n))
    }
}

/// Cut the connection without telling the server, as a network drop would
/// (the reader thread then reports it closed and the client tries to get
/// back into its match). For tests.
pub fn simulate_connection_loss(world: &mut World) {
    let link = world.resource::<Link>();
    if let LinkState::Up { stream, .. } = &*link.state.lock().unwrap() {
        let _ = stream.shutdown(Shutdown::Both);
    }
}

/// The `MatchSetup` every peer builds from the same `MatchStart`.
pub fn match_setup(data: &GameData, start: &MatchStart) -> MatchSetup {
    let players = start
        .players
        .iter()
        .map(|p| {
            let faction = data.faction_id(&p.faction).unwrap_or_else(|| {
                warn!("unknown faction `{}` in the match start; using the first", p.faction);
                FactionId(0)
            });
            PlayerSetup { name: p.name.clone(), faction, team: p.team, controller: if p.ai { Controller::Ai } else { Controller::Human }, color: None }
        })
        .collect();
    MatchSetup {
        seed: start.seed,
        map_size: 0.0,
        players,
        local_player: start.your_slot.map(|s| s as usize),
        fog: start.fog,
        reveal_map: false,
        start_age: 1,
        start_resources_mult: 1.0,
        rule: start.rule,
    }
}

/// The UI's requests.
fn apply_actions(mut actions: ResMut<NetActions>, mut link: ResMut<Link>, mut session: ResMut<NetSession>) {
    if actions.queue.is_empty() {
        return;
    }
    for action in std::mem::take(&mut actions.queue) {
        session.error = None;
        match action {
            NetAction::Connect(addr) => {
                let addr = crate::server_addr(&addr);
                session.server = addr.clone();
                session.status = NetStatus::Connecting;
                session.rooms.clear();
                session.lobby = None;
                session.account = None;
                session.ranked_slots.clear();
                link.reconnecting = None;
                link.connect(addr);
            }
            NetAction::Disconnect => {
                link.disconnect();
                link.resume = None;
                link.reconnecting = None;
                session.status = NetStatus::Offline;
                session.lobby = None;
                session.account = None;
                session.rooms.clear();
                session.ranked_slots.clear();
            }
            NetAction::SetName(name) => {
                let name: String = name.trim().chars().take(24).collect();
                if !name.is_empty() {
                    session.name = name.clone();
                    link.config.name = name;
                    link.config.save();
                }
            }
            NetAction::RefreshRooms => link.send(ClientMsg::ListRooms),
            NetAction::CreateRoom(name) => link.send(ClientMsg::CreateRoom { name: name.trim().chars().take(32).collect() }),
            NetAction::JoinRoom(id) => link.send(ClientMsg::JoinRoom { id }),
            NetAction::LeaveLobby => link.send(ClientMsg::LeaveLobby),
            NetAction::SetFaction(f) => link.send(ClientMsg::SetFaction(f)),
            NetAction::SetTeam(t) => link.send(ClientMsg::SetTeam(t)),
            NetAction::SetReady(r) => link.send(ClientMsg::SetReady(r)),
            NetAction::SetOptions(o) => link.send(ClientMsg::SetOptions(o)),
            NetAction::Start => link.send(ClientMsg::Start),
            NetAction::SignUp { username, password } => start_auth(&mut link, &mut session, true, username, password),
            NetAction::LogIn { username, password } => start_auth(&mut link, &mut session, false, username, password),
            NetAction::LogOut => {
                link.send(ClientMsg::Logout);
                session.account = None;
            }
            NetAction::JoinRanked(mode) => link.send(ClientMsg::JoinRanked(mode)),
            NetAction::RefreshRanked => link.send(ClientMsg::RefreshRanked),
            NetAction::Leaderboard => link.send(ClientMsg::Leaderboard),
            NetAction::LeaveMatch => {
                link.send(ClientMsg::LeaveMatch);
                link.resume = None;
            }
        }
    }
}

/// Sign up or log in on a thread; [`poll_auth`] picks up the answer.
fn start_auth(link: &mut Link, session: &mut NetSession, sign_up: bool, username: String, password: String) {
    let Some(auth) = link.auth.clone() else {
        session.error = Some("this server has no account service".into());
        return;
    };
    if link.auth_pending.is_some() {
        return;
    }
    let (tx, rx) = channel();
    let username = username.trim().to_string();
    thread::Builder::new()
        .name("af_net auth".into())
        .spawn(move || {
            let result = if sign_up { auth.sign_up(&username, &password) } else { auth.sign_in(&username, &password) };
            let _ = tx.send(result.map(|token| (username, token)));
        })
        .expect("spawn auth thread");
    link.auth_pending = Some(Mutex::new(rx));
    session.auth_busy = true;
}

/// The account service's answer: hand the token to the server.
fn poll_auth(mut link: ResMut<Link>, mut session: ResMut<NetSession>) {
    let Some(pending) = &link.auth_pending else { return };
    let result = match pending.lock().unwrap().try_recv() {
        Ok(r) => r,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => Err("the login thread died".into()),
    };
    link.auth_pending = None;
    match result {
        Ok((username, token)) => {
            link.config.username = username.clone();
            link.config.save();
            session.saved_username = username;
            link.send(ClientMsg::Login { access_token: token });
        }
        Err(e) => {
            session.auth_busy = false;
            session.error = Some(e);
        }
    }
}

/// Everything the connection threads delivered since the last frame.
#[allow(clippy::too_many_arguments)]
fn poll_link(
    mut commands: Commands,
    data: Res<Data>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut link: ResMut<Link>,
    mut session: ResMut<NetSession>,
    mut lockstep: Option<ResMut<Lockstep>>,
    mut notices: ResMut<Notices>,
    clock: Option<Res<SimClock>>,
    mut strategy: ResMut<TimeUpdateStrategy>,
    mut virt: ResMut<Time<Virtual>>,
    mut fixed: ResMut<Time<Fixed>>,
) {
    // Reconnect while a match is running.
    if let Some((since, last_try)) = link.reconnecting {
        let now = Instant::now();
        if now.duration_since(since) > RESUME_FOR {
            link.reconnecting = None;
            session.status = NetStatus::Failed("could not get back to the server".into());
        } else if matches!(&*link.state.lock().unwrap(), LinkState::Idle) && now.duration_since(last_try) >= RECONNECT_EVERY {
            link.reconnecting = Some((since, now));
            let addr = session.server.clone();
            link.connect(addr);
        }
    }
    let mut events = Vec::new();
    {
        let mut guard = link.state.lock().unwrap();
        let rx = match &mut *guard {
            LinkState::Idle => None,
            LinkState::Connecting(rx) | LinkState::Up { rx, .. } => Some(rx),
        };
        if let Some(rx) = rx {
            loop {
                match rx.try_recv() {
                    Ok(ev) => events.push(ev),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        events.push(LinkEvent::Closed("connection closed".into()));
                        break;
                    }
                }
            }
        }
    }
    for event in events {
        match event {
            LinkEvent::Connected { tx, stream } => {
                let mut guard = link.state.lock().unwrap();
                if let LinkState::Connecting(rx) = std::mem::take(&mut *guard) {
                    *guard = LinkState::Up { tx, rx, stream };
                }
                drop(guard);
                let name = session.name.clone();
                link.send(ClientMsg::Hello { protocol: PROTOCOL_VERSION, content_hash: link.content_hash, name });
            }
            LinkEvent::Closed(reason) => {
                link.disconnect();
                session.auth_busy = false;
                let in_match = lockstep.as_ref().is_some_and(|l| !l.ended) && link.resume.is_some();
                if in_match {
                    if link.reconnecting.is_none() {
                        link.reconnecting = Some((Instant::now(), Instant::now()));
                        notices.push(None, NoticeKind::Alert, "Connection lost: trying to get back...", None, clock.as_deref().map_or(0.0, |c| c.elapsed));
                    }
                    session.status = NetStatus::Failed(reason);
                } else {
                    session.status = match &session.status {
                        NetStatus::Online | NetStatus::Connecting => NetStatus::Failed(reason),
                        // A refusal just before the close already says why.
                        NetStatus::Failed(why) => NetStatus::Failed(why.clone()),
                        NetStatus::Offline => NetStatus::Offline,
                    };
                    session.lobby = None;
                    session.account = None;
                }
            }
            LinkEvent::Msg(msg) => match msg {
                ServerMsg::Welcome { motd, ranked_enabled, supabase_url, supabase_anon_key, email_domain } => {
                    link.welcomed = true;
                    link.auth = ranked_enabled.then(|| SupabaseAuth { url: supabase_url, anon_key: supabase_anon_key, email_domain });
                    session.status = NetStatus::Online;
                    session.ranked_enabled = ranked_enabled;
                    session.motd = motd;
                    link.config.server = session.server.clone();
                    link.config.save();
                    if let (Some((match_id, token)), Some(l)) = (link.resume, lockstep.as_deref()) {
                        link.reconnecting = None;
                        link.send(ClientMsg::Resume { match_id, resume_token: token, from_tick: l.buffer.next_tick() });
                        notices.push(None, NoticeKind::Info, "Back on the server", None, clock.as_deref().map_or(0.0, |c| c.elapsed));
                    } else {
                        link.send(ClientMsg::ListRooms);
                        link.send(ClientMsg::RefreshRanked);
                    }
                }
                ServerMsg::Refused(reason) => {
                    link.disconnect();
                    session.status = NetStatus::Failed(reason);
                }
                ServerMsg::Error(e) => {
                    session.auth_busy = false;
                    session.error = Some(e);
                }
                ServerMsg::Rooms(rooms) => session.rooms = rooms,
                ServerMsg::Lobby(lobby) => session.lobby = Some(lobby),
                ServerMsg::LeftLobby => session.lobby = None,
                ServerMsg::LoggedIn(account) => {
                    session.auth_busy = false;
                    session.account = Some(account);
                }
                ServerMsg::LoggedOut => session.account = None,
                ServerMsg::RankedSlots(slots) => session.ranked_slots = slots,
                ServerMsg::Leaderboard(rows) => session.leaderboard = rows,
                ServerMsg::StartMatch(start) => {
                    if lockstep.is_some() && link.reconnecting.is_none() && *state.get() == GameState::Playing {
                        // A resume answer while we still hold the match: just keep going.
                        continue;
                    }
                    link.resume = Some((start.match_id, start.resume_token));
                    session.result = None;
                    session.lockstep = LockstepView::default();
                    commands.insert_resource(match_setup(&data, &start));
                    // The match start carries no map options, so every peer
                    // (and a ranked referee, which inserts none) plays the
                    // default map, whatever the last skirmish here chose.
                    commands.insert_resource(MapOptions::default());
                    commands.insert_resource(Multiplayer { match_id: start.match_id, ranked: start.ranked });
                    commands.insert_resource(Lockstep::new(&start));
                    // The server's clock runs the match: the fixed clock takes
                    // only the time the turns in hand allow (`pace_clock`).
                    // This frame's clocks were already advanced, so take that
                    // time back too: not one sim tick may run before its turn
                    // is in hand, or the sim would never line up with the
                    // turns again.
                    *strategy = TimeUpdateStrategy::ManualDuration(Duration::ZERO);
                    virt.unpause();
                    virt.set_relative_speed(1.0);
                    virt.set_max_delta(Duration::from_secs(5));
                    virt.advance_by(Duration::ZERO);
                    let leftover = fixed.overstep();
                    fixed.discard_overstep(leftover);
                    next.set(GameState::Playing);
                }
                ServerMsg::Turn { tick, commands } => {
                    if let Some(l) = lockstep.as_deref_mut() {
                        l.buffer.insert(tick, commands.iter().filter_map(WireCommand::to_command).collect());
                    }
                }
                ServerMsg::Turns(turns) => {
                    if let Some(l) = lockstep.as_deref_mut() {
                        for (tick, commands) in turns {
                            l.buffer.insert(tick, commands.iter().filter_map(WireCommand::to_command).collect());
                        }
                    }
                }
                ServerMsg::Desync { tick, players } => {
                    if let Some(l) = lockstep.as_deref_mut()
                        && l.parted(&players)
                    {
                        l.desynced = true;
                        session.lockstep.desynced = true;
                    }
                    let who = if players.is_empty() { "this game".to_string() } else { players.join(", ") };
                    let against = if lockstep.as_deref().is_some_and(|l| l.ranked) { "the server" } else { "the other players" };
                    notices.push(None, NoticeKind::Alert, format!("Desync at tick {tick}: {who} no longer match {against}"), None, clock.as_deref().map_or(0.0, |c| c.elapsed));
                }
                ServerMsg::MatchEnded(result) => {
                    if let Some(l) = lockstep.as_deref_mut() {
                        l.ended = true;
                    }
                    link.resume = None;
                    link.reconnecting = None;
                    if !result.note.is_empty() {
                        notices.push(None, NoticeKind::Info, result.note.clone(), None, clock.as_deref().map_or(0.0, |c| c.elapsed));
                    }
                    session.result = Some(result);
                }
                ServerMsg::Pong(sent) => {
                    if let Some((id, at)) = link.ping_sent.take()
                        && id == sent
                    {
                        session.lockstep.rtt_ms = Some(at.elapsed().as_millis() as u32);
                    }
                }
            },
        }
    }
}

/// Feeds the fixed clock only the time the turns in hand allow (see
/// [`crate::lockstep::Pacer`]). Runs before Bevy updates the clocks.
fn pace_clock(mut lockstep: ResMut<Lockstep>, fixed: Res<Time<Fixed>>, mut strategy: ResMut<TimeUpdateStrategy>) {
    let now = Instant::now();
    let real_dt = lockstep.last_frame.map_or(Duration::ZERO, |t| now.duration_since(t)).min(Duration::from_millis(250));
    lockstep.last_frame = Some(now);
    let ready = lockstep.buffer.ready();
    let delta = lockstep.pacer.virtual_delta(real_dt, fixed.overstep(), fixed.timestep(), ready);
    *strategy = TimeUpdateStrategy::ManualDuration(delta);
    if ready == 0 && lockstep.loaded_sent && !lockstep.ended {
        if lockstep.stall_since.is_none() {
            lockstep.stall_since = Some(now);
        }
    } else {
        lockstep.stall_since = None;
    }
}

/// The next tick's turn, into the inbox the sim drains in `Prepare`.
fn inject_turn(mut lockstep: ResMut<Lockstep>, mut inbox: ResMut<CommandInbox>, clock: Res<SimClock>, players: Option<Res<Players>>) {
    if players.is_none() {
        return;
    }
    let tick = clock.tick + 1;
    if lockstep.buffer.next_tick() != tick {
        warn!("lockstep: the sim is at tick {} but the next turn held is {}", clock.tick, lockstep.buffer.next_tick());
        return;
    }
    match lockstep.buffer.take_next() {
        Some(cmds) => inbox.pending.extend(cmds),
        None => warn!("lockstep: tick {tick} ran without its turn"),
    }
}

/// The local human's commands go to the server, not into the sim; they
/// come back in a turn with everyone else's.
fn route_local_commands(mut inbox: ResMut<CommandInbox>, local: Res<LocalPlayer>, link: Res<Link>) {
    let Some(me) = local.0 else {
        return;
    };
    if !inbox.pending.iter().any(|c| c.player == me) {
        return;
    }
    let (mine, rest): (Vec<PlayerCommand>, Vec<PlayerCommand>) = std::mem::take(&mut inbox.pending).into_iter().partition(|c| c.player == me);
    inbox.pending = rest;
    link.send(ClientMsg::Commands(mine.iter().map(WireCommand::from).collect()));
}

/// Tell the server the world is built, once.
fn send_loaded(mut lockstep: ResMut<Lockstep>, players: Option<Res<Players>>, nav: Option<Res<Nav>>, link: Res<Link>) {
    if lockstep.loaded_sent || players.is_none() || nav.is_none() {
        return;
    }
    lockstep.loaded_sent = true;
    link.send(ClientMsg::Loaded);
}

/// The state hash after every [`HASH_EVERY`]th tick.
fn send_hash(world: &mut World) {
    let tick = world.resource::<SimClock>().tick;
    if tick == 0 || !tick.is_multiple_of(HASH_EVERY) || !world.contains_resource::<Players>() {
        return;
    }
    let hash = sim_hash(world);
    world.resource::<Link>().send(ClientMsg::Hash { tick, hash });
}

/// Casual matches end when a client's sim says so.
fn watch_outcome(mut lockstep: ResMut<Lockstep>, outcome: Option<Res<GameOutcome>>, link: Res<Link>) {
    let Some(outcome) = outcome else { return };
    if lockstep.outcome_sent || lockstep.ranked || !outcome.ended {
        return;
    }
    lockstep.outcome_sent = true;
    link.send(ClientMsg::MatchOver { winning_team: outcome.winning_team, time: outcome.time });
}

fn send_ping(mut link: ResMut<Link>) {
    if !link.is_up() {
        return;
    }
    let now = Instant::now();
    if link.last_ping.is_some_and(|t| now.duration_since(t) < PING_EVERY) {
        return;
    }
    link.last_ping = Some(now);
    let id = now.elapsed().as_nanos() as u64 ^ link.content_hash;
    link.ping_sent = Some((id, now));
    link.send(ClientMsg::Ping(id));
}

fn update_lockstep_view(lockstep: Res<Lockstep>, mut session: ResMut<NetSession>) {
    let view = LockstepView {
        buffered: lockstep.buffer.ready().min(u32::MAX as u64) as u32,
        stalled_secs: lockstep.stall_since.map_or(0.0, |t| t.elapsed().as_secs_f32()),
        catching_up: lockstep.pacer.catching_up(lockstep.buffer.ready()),
        desynced: lockstep.desynced,
        rtt_ms: session.lockstep.rtt_ms,
    };
    if session.lockstep != view {
        session.lockstep = view;
    }
}

/// Leaving the match screen: tell the server, give the clock back.
fn leave_match(
    mut commands: Commands,
    lockstep: Option<Res<Lockstep>>,
    mut link: ResMut<Link>,
    mut strategy: ResMut<TimeUpdateStrategy>,
    mut virt: ResMut<Time<Virtual>>,
    mut session: ResMut<NetSession>,
) {
    let Some(lockstep) = lockstep else { return };
    if !lockstep.ended {
        link.send(ClientMsg::LeaveMatch);
    }
    link.resume = None;
    link.reconnecting = None;
    commands.remove_resource::<Lockstep>();
    commands.remove_resource::<Multiplayer>();
    *strategy = TimeUpdateStrategy::Automatic;
    virt.unpause();
    virt.set_relative_speed(1.0);
    virt.set_max_delta(Duration::from_millis(250));
    session.lockstep = LockstepView::default();
    // Back in the menu: the lobby list is stale.
    if link.is_up() {
        link.send(ClientMsg::ListRooms);
        link.send(ClientMsg::RefreshRanked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_named_players_game_has_parted() {
        let player = |name: &str| MatchPlayer { name: name.into(), faction: "ironhold".into(), team: 0, ai: false };
        let start = |slot| MatchStart {
            match_id: 1,
            seed: 2,
            players: vec![player("Ann"), player("Bob"), player("Cy")],
            your_slot: slot,
            fog: true,
            rule: VictoryRule::default(),
            ranked: false,
            resume_token: 0,
        };
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let bob = Lockstep::new(&start(Some(1)));
        assert!(bob.parted(&names(&["Bob"])) && bob.parted(&names(&["Ann", "Bob"])));
        assert!(!bob.parted(&names(&["Cy"])), "Cy's game parted, not Bob's");
        assert!(bob.parted(&[]), "nobody named: everyone's");
        let spectator = Lockstep::new(&start(None));
        assert!(!spectator.parted(&names(&["Ann"])));
    }
}
