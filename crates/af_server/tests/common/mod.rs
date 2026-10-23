//! A server on a loopback port, headless clients with the whole simulation
//! and the multiplayer client plugin, and a stand-in for the account
//! service's HTTP endpoints.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use af_core::ext::net::*;
use af_core::hash::sim_hash;
use af_core::prelude::*;
use af_net::NetClientPlugin;
use af_server::accounts::{Accounts, MemoryAccounts};
use af_server::{ServerConfig, SupabaseConfig};
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;

pub struct TestServer {
    pub addr: String,
    pub accounts: Option<Arc<MemoryAccounts>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn data_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/data")
}

/// A server; with `auth_url`, ranked play with in-memory accounts whose
/// tokens the mock auth service at that URL hands out.
pub fn start_server(auth_url: Option<&str>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap().to_string();
    let data = af_core::testing::test_data();
    let content_hash = af_data::content_hash(&data_dir()).expect("content hash");
    let accounts = auth_url.map(|_| Arc::new(MemoryAccounts::default()));
    let config = ServerConfig {
        bind: addr.clone(),
        supabase: auth_url.map(|url| SupabaseConfig { url: url.to_string(), anon_key: "anon".into(), service_key: "service".into() }),
        load_timeout: Duration::from_secs(3),
        forfeit_after: Duration::from_secs(2),
        ..ServerConfig::default()
    };
    let stop = Arc::new(AtomicBool::new(false));
    let accounts_dyn: Option<Arc<dyn Accounts>> = accounts.clone().map(|a| a as Arc<dyn Accounts>);
    let stop2 = stop.clone();
    let handle = thread::spawn(move || af_server::run(config, listener, data, content_hash, accounts_dyn, stop2));
    TestServer { addr, accounts, stop, handle: Some(handle) }
}

/// Every tick's state hash, for comparing two clients bit for bit.
#[derive(Resource, Default)]
pub struct Trace(pub Vec<(u64, u64)>);

fn trace_hash(world: &mut World) {
    if !world.contains_resource::<Players>() {
        return;
    }
    let tick = world.resource::<SimClock>().tick;
    let hash = sim_hash(world);
    world.resource_mut::<Trace>().0.push((tick, hash));
}

/// A headless game client in the menu.
pub fn client() -> App {
    let mut app = App::new();
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins((MinimalPlugins, StatesPlugin));
    app.insert_state(GameState::Menu);
    app.insert_resource(Data(af_core::testing::test_data()));
    af_sim::add_sim_plugins(&mut app);
    app.add_plugins(NetClientPlugin);
    app.init_resource::<Trace>();
    app.add_systems(FixedPostUpdate, trace_hash);
    app
}

pub fn act(app: &mut App, action: NetAction) {
    app.world_mut().resource_mut::<NetActions>().push(action);
}

pub fn session(app: &App) -> &NetSession {
    app.world().resource::<NetSession>()
}

pub fn tick(app: &App) -> u64 {
    app.world().get_resource::<SimClock>().map_or(0, |c| c.tick)
}

pub fn playing(app: &App) -> bool {
    *app.world().resource::<State<GameState>>().get() == GameState::Playing && app.world().contains_resource::<Players>()
}

/// Update every app until `done`, or panic with `what` after `timeout`.
pub fn pump(apps: &mut [&mut App], timeout: Duration, what: &str, mut done: impl FnMut(&[&mut App]) -> bool) {
    let deadline = Instant::now() + timeout;
    loop {
        for app in apps.iter_mut() {
            app.update();
        }
        if done(apps) {
            return;
        }
        if Instant::now() > deadline {
            let states: Vec<String> =
                apps.iter().map(|a| format!("status {:?} tick {} lobby {} error {:?}", session(a).status, tick(a), session(a).lobby.is_some(), session(a).error)).collect();
            panic!("timed out waiting for {what}: {states:?}");
        }
        thread::sleep(Duration::from_millis(2));
    }
}

/// Connect `app` to `addr` and wait until it is online.
pub fn connect(app: &mut App, addr: &str) {
    act(app, NetAction::Connect(addr.to_string()));
    pump(&mut [app], Duration::from_secs(10), "connection", |apps| session(apps[0]).status == NetStatus::Online);
}

/// The local player's living units.
pub fn own_units(app: &mut App) -> Vec<Entity> {
    let me = app.world().resource::<LocalPlayer>().0.expect("a local player");
    let world = app.world_mut();
    let mut q = world.query_filtered::<(Entity, &Owner), (With<Unit>, Without<Dead>)>();
    let mut units: Vec<Entity> = q.iter(world).filter(|(_, o)| o.0 == me).map(|(e, _)| e).collect();
    units.sort();
    units
}

pub fn position(app: &App, e: Entity) -> Option<Vec2> {
    app.world().get::<Position>(e).map(|p| p.0)
}

/// Push a command the way af_input does.
pub fn command(app: &mut App, cmd: PlayerCommand) {
    app.world_mut().resource_mut::<CommandInbox>().push(cmd);
}

/// The ticks both traces hold, and whether every one of them matches.
pub fn traces_agree(a: &App, b: &App) -> (usize, Vec<u64>) {
    let ta = &a.world().resource::<Trace>().0;
    let tb = &b.world().resource::<Trace>().0;
    let mut common = 0;
    let mut differ = Vec::new();
    for (tick, ha) in ta {
        if let Some((_, hb)) = tb.iter().find(|(t, _)| t == tick) {
            common += 1;
            if ha != hb {
                differ.push(*tick);
            }
        }
    }
    (common, differ)
}

/// A stand-in for Supabase Auth on a loopback port: `POST /auth/v1/token`
/// and `/auth/v1/signup` answer with the token `tok-<username>`; the
/// server's in-memory accounts are told which users those tokens are.
pub struct MockAuth {
    pub url: String,
    stop: Arc<AtomicBool>,
}

impl Drop for MockAuth {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
    }
}

pub fn start_mock_auth() -> MockAuth {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            if stop2.load(Ordering::Relaxed) {
                break;
            }
            let Ok(mut s) = stream else { continue };
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let request = loop {
                let Ok(n) = s.read(&mut chunk) else { break None };
                if n == 0 {
                    break None;
                }
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(i) = text.find("\r\n\r\n") {
                    let len = text[..i].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                    if buf.len() >= i + 4 + len {
                        break Some((text[..i].to_string(), String::from_utf8_lossy(&buf[i + 4..i + 4 + len]).to_string()));
                    }
                }
            };
            let Some((head, body)) = request else { continue };
            let (status, reply) = if head.starts_with("POST /auth/v1/token") || head.starts_with("POST /auth/v1/signup") {
                let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let email = json.get("email").and_then(|e| e.as_str()).unwrap_or("");
                let user = email.split('@').next().unwrap_or("");
                let password = json.get("password").and_then(|e| e.as_str()).unwrap_or("");
                if password == "wrong-password" {
                    (400, r#"{"error_code":"invalid_credentials","msg":"Invalid login credentials"}"#.to_string())
                } else {
                    (200, format!(r#"{{"access_token":"tok-{user}","token_type":"bearer","user":{{"id":"{user}"}}}}"#))
                }
            } else {
                (404, r#"{"msg":"not found"}"#.to_string())
            };
            let response = format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
            let _ = s.write_all(response.as_bytes());
        }
    });
    MockAuth { url, stop }
}
