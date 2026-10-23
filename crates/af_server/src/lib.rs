//! The Aetherfall multiplayer server (see `docs/multiplayer.md`).
//!
//! * Casual rooms: up to six humans (plus computer players), relayed in
//!   lockstep. The server paces the clock and hands every player the same
//!   turns; it does not simulate.
//! * Ranked 1v1, 1v1v1 and 2v2, one match of each at a time: the server
//!   also runs the match itself (`af_sim`), validates every command
//!   against its own world, checks the players' state hashes, decides the
//!   result, moves the ratings and stores the match ([`accounts`]).
//!
//! [`server::run`] is the whole thing; `main.rs` reads the configuration.

pub mod accounts;
pub mod config;
pub mod elo;
pub mod live;
pub mod net;
pub mod server;

pub use config::{ServerConfig, SupabaseConfig};
pub use server::run;
