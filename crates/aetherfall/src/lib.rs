//! Aetherfall app wiring, shared by the binary and integration tests.
//!
//! The simulation side (`add_sim_plugins`, `headless_app`, `run_headless`,
//! `skirmish_setup`, `match_report`, the profiler) lives in `af_sim` and is
//! re-exported here; this crate adds the client side:
//!
//! * [`add_client_plugins`] — input, UI, rendering and the multiplayer
//!   client (needs a window).

pub use af_sim::*;

use bevy::prelude::*;

pub fn add_client_plugins(app: &mut App) {
    app.add_plugins((af_input::RtsInputPlugin, af_ui::HudPlugin, af_render::BoxRenderPlugin, af_net::NetClientPlugin));
}
