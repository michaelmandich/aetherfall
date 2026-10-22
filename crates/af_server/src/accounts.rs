//! Accounts and ratings: who a login token belongs to, each player's row,
//! match records. [`Supabase`] talks to the project over its REST API with
//! the service-role key (the only place that key lives); [`Accounts`] is
//! the trait the server uses, so tests can swap in a fake.
//!
//! Everything blocks: the server calls it from worker threads.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use af_core::ext::net::LeaderboardRow;
use af_net::auth::{get_user, AuthUser};
use serde::{Deserialize, Serialize};
use ureq::Agent;

use crate::elo;

/// A row of `af_players`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PlayerRow {
    pub user_id: String,
    pub username: String,
    pub elo: i32,
    pub wins: u32,
    pub losses: u32,
}

/// One player of a finished ranked match.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MatchRecordPlayer {
    pub user_id: String,
    pub slot: u8,
    pub team: u8,
    pub faction: String,
    pub elo_before: i32,
    pub elo_after: i32,
    pub won: bool,
}

/// A finished ranked match, as stored.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MatchRecord {
    pub mode: String,
    pub seed: u64,
    pub winning_team: Option<u8>,
    pub duration_secs: f32,
    /// "" for a normal result, else why ratings did not move ("void: desync").
    pub note: String,
    pub players: Vec<MatchRecordPlayer>,
}

pub trait Accounts: Send + Sync {
    /// Who `access_token` belongs to.
    fn verify(&self, access_token: &str) -> Result<AuthUser, String>;
    /// The player's row, created at [`elo::START`] on first sight.
    fn get_or_create_player(&self, user: &AuthUser) -> Result<PlayerRow, String>;
    /// Store the match and apply the rating changes.
    fn record_match(&self, record: &MatchRecord) -> Result<(), String>;
    fn leaderboard(&self, limit: usize) -> Result<Vec<LeaderboardRow>, String>;
}

/// The Supabase project.
pub struct Supabase {
    pub url: String,
    /// The key handed to clients for the auth endpoint.
    pub anon_key: String,
    /// The service-role key: full access, never leaves the server.
    pub service_key: String,
    agent: Agent,
}

impl Supabase {
    pub fn new(url: &str, anon_key: &str, service_key: &str) -> Supabase {
        let agent = Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build().into();
        Supabase { url: url.trim_end_matches('/').to_string(), anon_key: anon_key.into(), service_key: service_key.into(), agent }
    }

    fn rest(&self, path: &str) -> String {
        format!("{}/rest/v1/{path}", self.url)
    }

    fn get(&self, path: &str) -> Result<String, String> {
        let mut response = self
            .agent
            .get(&self.rest(path))
            .header("apikey", &self.service_key)
            .header("Authorization", &format!("Bearer {}", self.service_key))
            .call()
            .map_err(|e| format!("database: {e}"))?;
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().map_err(|e| format!("database: {e}"))?;
        if (200..300).contains(&status) { Ok(text) } else { Err(format!("database answered {status}: {}", text.chars().take(200).collect::<String>())) }
    }

    fn post(&self, path: &str, body: &serde_json::Value, prefer: &str) -> Result<(u16, String), String> {
        let mut response = self
            .agent
            .post(&self.rest(path))
            .header("apikey", &self.service_key)
            .header("Authorization", &format!("Bearer {}", self.service_key))
            .header("Content-Type", "application/json")
            .header("Prefer", prefer)
            .send(body.to_string().as_bytes())
            .map_err(|e| format!("database: {e}"))?;
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().map_err(|e| format!("database: {e}"))?;
        Ok((status, text))
    }
}

impl Accounts for Supabase {
    fn verify(&self, access_token: &str) -> Result<AuthUser, String> {
        get_user(&self.url, &self.anon_key, access_token)
    }

    fn get_or_create_player(&self, user: &AuthUser) -> Result<PlayerRow, String> {
        let text = self.get(&format!("af_players?user_id=eq.{}&select=user_id,username,elo,wins,losses", user.id))?;
        let rows: Vec<PlayerRow> = serde_json::from_str(&text).map_err(|e| format!("database: bad player row ({e})"))?;
        if let Some(row) = rows.into_iter().next() {
            return Ok(row);
        }
        let body = serde_json::json!({ "user_id": user.id, "username": user.username, "elo": elo::START });
        let (status, text) = self.post("af_players", &body, "return=representation")?;
        if status == 409 {
            return Err("that username is taken".into());
        }
        if !(200..300).contains(&status) {
            return Err(format!("database answered {status}: {}", text.chars().take(200).collect::<String>()));
        }
        let rows: Vec<PlayerRow> = serde_json::from_str(&text).map_err(|e| format!("database: bad player row ({e})"))?;
        rows.into_iter().next().ok_or_else(|| "database returned no player row".into())
    }

    fn record_match(&self, record: &MatchRecord) -> Result<(), String> {
        let body = serde_json::json!({
            "p_mode": record.mode,
            "p_seed": record.seed.to_string(),
            "p_winning_team": record.winning_team,
            "p_duration_secs": record.duration_secs,
            "p_note": record.note,
            "p_players": record.players,
        });
        let (status, text) = self.post("rpc/af_record_match", &body, "return=minimal")?;
        if (200..300).contains(&status) { Ok(()) } else { Err(format!("database answered {status}: {}", text.chars().take(200).collect::<String>())) }
    }

    fn leaderboard(&self, limit: usize) -> Result<Vec<LeaderboardRow>, String> {
        let text = self.get(&format!("af_players?select=username,elo,wins,losses&order=elo.desc,wins.desc,username.asc&limit={limit}"))?;
        serde_json::from_str(&text).map_err(|e| format!("database: bad leaderboard ({e})"))
    }
}

/// Accounts kept in memory: tests, and a server run without Supabase
/// configured (`--ranked-memory`), where ratings last until it stops.
#[derive(Default)]
pub struct MemoryAccounts {
    inner: Mutex<MemoryInner>,
}

#[derive(Default)]
struct MemoryInner {
    /// token -> user
    tokens: HashMap<String, AuthUser>,
    players: HashMap<String, PlayerRow>,
    pub matches: Vec<MatchRecord>,
}

impl MemoryAccounts {
    /// Make `token` log in as `user`.
    pub fn add_token(&self, token: &str, user: AuthUser) {
        self.inner.lock().unwrap().tokens.insert(token.into(), user);
    }

    pub fn matches(&self) -> Vec<MatchRecord> {
        self.inner.lock().unwrap().matches.clone()
    }

    pub fn player(&self, user_id: &str) -> Option<PlayerRow> {
        self.inner.lock().unwrap().players.get(user_id).cloned()
    }
}

impl Accounts for MemoryAccounts {
    fn verify(&self, access_token: &str) -> Result<AuthUser, String> {
        self.inner.lock().unwrap().tokens.get(access_token).cloned().ok_or_else(|| "invalid token".into())
    }

    fn get_or_create_player(&self, user: &AuthUser) -> Result<PlayerRow, String> {
        let mut inner = self.inner.lock().unwrap();
        if inner.players.values().any(|p| p.user_id != user.id && p.username.eq_ignore_ascii_case(&user.username)) {
            return Err("that username is taken".into());
        }
        let row = inner
            .players
            .entry(user.id.clone())
            .or_insert_with(|| PlayerRow { user_id: user.id.clone(), username: user.username.clone(), elo: elo::START, wins: 0, losses: 0 });
        Ok(row.clone())
    }

    fn record_match(&self, record: &MatchRecord) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        for p in &record.players {
            if let Some(row) = inner.players.get_mut(&p.user_id) {
                row.elo = p.elo_after;
                if record.note.is_empty() {
                    if p.won {
                        row.wins += 1;
                    } else {
                        row.losses += 1;
                    }
                }
            }
        }
        inner.matches.push(record.clone());
        Ok(())
    }

    fn leaderboard(&self, limit: usize) -> Result<Vec<LeaderboardRow>, String> {
        let inner = self.inner.lock().unwrap();
        let mut rows: Vec<LeaderboardRow> =
            inner.players.values().map(|p| LeaderboardRow { username: p.username.clone(), elo: p.elo, wins: p.wins, losses: p.losses }).collect();
        rows.sort_by(|a, b| b.elo.cmp(&a.elo).then(b.wins.cmp(&a.wins)).then(a.username.cmp(&b.username)));
        rows.truncate(limit);
        Ok(rows)
    }
}
