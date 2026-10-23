//! Connections: a non-blocking listener polled by the main loop, and per
//! connection a reader thread (frames in, [`Event`]s out) and a writer
//! thread (messages in, frames out). Worker threads for the account
//! service report through the same [`Event`] channel, so the main loop is
//! the only place that touches the server's state.

use std::collections::HashMap;
use std::io;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::Duration;

use af_core::ext::net::LeaderboardRow;
use af_net::auth::AuthUser;
use af_net::framing::{read_frame, write_frame};
use af_net::protocol::{decode, encode, ClientMsg, ServerMsg};

use crate::accounts::PlayerRow;

pub type ConnId = u64;

/// A client that sends nothing for this long is dropped (clients ping
/// every two seconds).
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);

pub enum Event {
    Connected { conn: ConnId, addr: String, tx: Sender<Option<ServerMsg>> },
    Msg { conn: ConnId, msg: ClientMsg },
    Closed { conn: ConnId },
    /// A worker looked a login token up.
    Login { conn: ConnId, result: Result<(AuthUser, PlayerRow), String> },
    /// A worker stored a match.
    Recorded { match_id: u64, result: Result<(), String> },
    Leaderboard { conn: ConnId, result: Result<Vec<LeaderboardRow>, String> },
}

/// Accepts whatever is waiting on `listener` (non-blocking) and starts the
/// threads of each new connection.
pub fn accept_pending(listener: &TcpListener, next_id: &mut ConnId, events: &Sender<Event>) {
    loop {
        match listener.accept() {
            Ok((stream, addr)) => {
                let id = *next_id;
                *next_id += 1;
                if let Err(e) = start_connection(id, stream, events.clone()) {
                    eprintln!("af_server: connection {id} from {addr}: {e}");
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
            Err(e) => {
                eprintln!("af_server: accept: {e}");
                return;
            }
        }
    }
}

fn start_connection(id: ConnId, stream: TcpStream, events: Sender<Event>) -> io::Result<()> {
    // On Windows an accepted socket inherits the listener's non-blocking
    // mode, and the reader thread would fail its first read at once.
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(IDLE_TIMEOUT))?;
    let addr = stream.peer_addr().map_or_else(|_| "?".to_string(), |a| a.to_string());
    let mut reader = stream.try_clone()?;
    let mut writer = stream.try_clone()?;
    let (tx, rx) = channel::<Option<ServerMsg>>();
    events.send(Event::Connected { conn: id, addr, tx }).map_err(|_| io::Error::other("server gone"))?;
    let read_events = events;
    thread::Builder::new().name(format!("af_server read {id}")).spawn(move || {
        loop {
            match read_frame(&mut reader) {
                Ok(bytes) => match decode::<ClientMsg>(&bytes) {
                    Ok(msg) => {
                        if read_events.send(Event::Msg { conn: id, msg }).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Err(_) => break,
            }
        }
        let _ = reader.shutdown(Shutdown::Both);
        let _ = read_events.send(Event::Closed { conn: id });
    })?;
    thread::Builder::new().name(format!("af_server write {id}")).spawn(move || {
        while let Ok(Some(msg)) = rx.recv() {
            if write_frame(&mut writer, &encode(&msg)).is_err() {
                break;
            }
        }
        let _ = writer.shutdown(Shutdown::Both);
    })?;
    Ok(())
}

/// Where to write to each connection.
#[derive(Default)]
pub struct Outbox {
    senders: HashMap<ConnId, Sender<Option<ServerMsg>>>,
}

impl Outbox {
    pub fn add(&mut self, conn: ConnId, tx: Sender<Option<ServerMsg>>) {
        self.senders.insert(conn, tx);
    }

    pub fn send(&self, conn: ConnId, msg: ServerMsg) {
        if let Some(tx) = self.senders.get(&conn) {
            let _ = tx.send(Some(msg));
        }
    }

    /// Close the connection (after what was queued is written).
    pub fn close(&mut self, conn: ConnId) {
        if let Some(tx) = self.senders.remove(&conn) {
            let _ = tx.send(None);
        }
    }

    pub fn remove(&mut self, conn: ConnId) {
        self.senders.remove(&conn);
    }
}
