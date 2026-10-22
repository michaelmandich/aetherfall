//! Where simulation time goes: a six-player AI match with the `SimSet`
//! profiler attached. A measurement, not a check, so it is `#[ignore]`d:
//!
//! ```text
//! cargo test -p aetherfall --test profile -- --ignored --nocapture
//! ```
//!
//! `AF_PROFILE_MINUTES` (default 30), `AF_PROFILE_SEED` (default 8) and
//! `AF_PROFILE_PLAYERS` (default 6) pick the match, `AF_PROFILE_MAP_SIZE`
//! (metres; default from the options and the player count) and
//! `AF_PROFILE_MAP` (`size:water:rivers:site`, say `huge:3:2:random`; each
//! part as the CLI takes it) its map; `AF_PROFILE_SINGLE_THREADED=1` runs
//! `FixedUpdate` on the single-threaded executor. A big map:
//!
//! ```text
//! AF_PROFILE_PLAYERS=8 AF_PROFILE_MAP_SIZE=768 AF_PROFILE_MAP=huge:2:2:random cargo test --release -p aetherfall --test profile -- --ignored --nocapture
//! ```
//! The report shows every set over the whole match,
//! over the match after its first second (the first tick registers every
//! obstacle of the map) and over its last five minutes (the busiest
//! stretch). Maxima on a machine busy with other work include its stalls.

use std::time::Instant;

use aetherfall::*;
use af_core::ext::map::{MapOptions, MapSize, RiverCount, SiteChoice, WaterLevel};
use af_core::prelude::*;
use bevy::ecs::schedule::SingleThreadedExecutor;
use bevy::prelude::*;

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// The map options named in `AF_PROFILE_MAP` (defaults for what it leaves out).
fn map_options() -> MapOptions {
    let spec = std::env::var("AF_PROFILE_MAP").unwrap_or_default();
    let mut parts = spec.split(':').map(str::trim);
    let mut o = MapOptions::default();
    let mut next = || parts.next().filter(|p| !p.is_empty());
    if let Some(size) = next().and_then(MapSize::parse) {
        o.size = size;
    }
    if let Some(water) = next().and_then(WaterLevel::parse) {
        o.water = water;
    }
    if let Some(rivers) = next().and_then(RiverCount::parse) {
        o.rivers = rivers;
    }
    if let Some(site) = next().and_then(SiteChoice::parse) {
        o.site = site;
    }
    o
}

#[test]
#[ignore = "measurement: cargo test -p aetherfall --test profile -- --ignored --nocapture"]
fn six_player_match_profile() {
    let minutes: f32 = env_or("AF_PROFILE_MINUTES", 30.0);
    let seed: u64 = env_or("AF_PROFILE_SEED", 8);
    let players: usize = env_or("AF_PROFILE_PLAYERS", 6);
    let map_size: f32 = env_or("AF_PROFILE_MAP_SIZE", 0.0);
    let data = af_core::testing::test_data();
    let opts = SkirmishOptions { seed, players, human: false, map_size, ..Default::default() };
    let setup = skirmish_setup(&data, &opts).expect("setup");
    let mut app = headless_app(data, setup, 30);
    let map = map_options();
    println!("{players} players, map {map:?}{}", if map_size > 0.0 { format!(", {map_size} m") } else { String::new() });
    app.insert_resource(map);
    if env_or("AF_PROFILE_SINGLE_THREADED", 0u8) != 0 {
        app.edit_schedule(FixedUpdate, |s| {
            s.set_executor(SingleThreadedExecutor::new());
        });
        println!("FixedUpdate on the single-threaded executor");
    }
    add_sim_profiler(&mut app);
    let ticks = (minutes * 60.0 * SIM_HZ as f32) as u64;
    let started = Instant::now();
    let every = (300.0 * SIM_HZ) as u64;
    run_headless(&mut app, ticks, every, |app| {
        let profile = app.world().resource::<SimProfile>();
        let n = profile.ticks.len();
        println!("t={:.0}s wall {:.1}s, last 5 min: {:.3} ms/tick", n as f32 / SIM_HZ as f32, started.elapsed().as_secs_f32(), profile.mean_tick(Some(n.saturating_sub(every as usize)..n)).as_secs_f64() * 1e3);
    });
    let wall = started.elapsed();
    println!("{}", match_report(app.world_mut()));
    let profile = app.world().resource::<SimProfile>();
    let n = profile.ticks.len();
    println!("whole match ({n} ticks, {:.1} s wall):\n{}", wall.as_secs_f32(), profile.report(None));
    let setup = SIM_HZ as usize;
    println!("after the first second:\n{}", profile.report(Some(setup.min(n)..n)));
    println!("last five minutes:\n{}", profile.report(Some(n.saturating_sub(every as usize)..n)));
}
