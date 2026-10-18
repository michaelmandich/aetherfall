//! Helpers for headless tests in any crate:
//!
//! ```ignore
//! let data = af_core::testing::test_data();
//! let mut app = af_core::testing::sim_app(data.clone());
//! app.add_plugins(MyPlugin);
//! let players = af_core::testing::flat_world(&mut app, 128.0, &[("ironhold", 0), ("dune", 1)]);
//! // spawn things with spawn_unit/spawn_building via app.world_mut().commands() or a system
//! af_core::testing::run_ticks(&mut app, 30); // one sim second
//! ```

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bevy::ecs::world::CommandQueue as BevyCommandQueue;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;

use crate::prelude::*;
use crate::CorePlugin;

/// The real game content, loaded once per test process.
pub fn test_data() -> Arc<GameData> {
    static DATA: OnceLock<Arc<GameData>> = OnceLock::new();
    DATA.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
        Arc::new(GameData::load_dir(&dir).unwrap_or_else(|e| panic!("{e}")))
    })
    .clone()
}

/// A headless app in `GameState::Playing` where every `app.update()` runs
/// exactly one sim tick (after the first update, which primes the clock;
/// `run_ticks` handles that).
pub fn sim_app(data: Arc<GameData>) -> App {
    let mut app = App::new();
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins((MinimalPlugins, StatesPlugin));
    app.insert_state(GameState::Playing);
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.insert_resource(Data(data));
    app.add_plugins(CorePlugin);
    app.world_mut().resource_mut::<Time<Virtual>>().set_max_delta(Duration::from_secs(10));
    app
}

/// A nav grid of `width` x `height` 1 m cells filled with `fill`, with every
/// terrain type's passability registered from `data`.
pub fn new_nav(data: &GameData, width: u32, height: u32, fill: TerrainId) -> NavGrid {
    let mut nav = NavGrid::new(width, height, 1.0, Vec2::ZERO);
    register_terrain(&mut nav, data);
    if fill.0 != 0 {
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                nav.set_terrain(IVec2::new(x, y), fill.0);
            }
        }
    }
    nav
}

/// Register every terrain type's mobility mask and buildability.
pub fn register_terrain(nav: &mut NavGrid, data: &GameData) {
    for t in data.terrain_ids() {
        nav.set_terrain_info(t.0, data.terrain_mask(t).0, data.terrain_def(t).buildable);
    }
}

/// Insert Players/MapInfo/Nav/LocalPlayer for an all-grass square map of
/// side `size` with the given (faction id, team) players. Players start at
/// evenly spaced points; nothing is spawned.
pub fn flat_world(app: &mut App, size: f32, players: &[(&str, u8)]) -> Vec<PlayerId> {
    let data = app.world().resource::<Data>().0.clone();
    let grass = data.terrain_id("grass").expect("grass terrain");
    let n = size.ceil() as u32;
    let nav = new_nav(&data, n, n, grass);
    let mut list = Vec::new();
    let mut starts = Vec::new();
    for (i, (f, team)) in players.iter().enumerate() {
        let fid = data.faction_id(f).unwrap_or_else(|| panic!("unknown faction {f}"));
        let fdef = data.faction(fid);
        let mut p = PlayerState::new(PlayerId(i as u8), fdef.name.clone(), fid, fdef.color, *team, Controller::Inert, &data);
        let a = i as f32 / players.len().max(1) as f32 * std::f32::consts::TAU;
        p.start_pos = Vec2::splat(size * 0.5) + Vec2::new(a.cos(), a.sin()) * size * 0.3;
        starts.push(p.start_pos);
        list.push(p);
    }
    let ids = list.iter().map(|p| p.id).collect();
    let world = app.world_mut();
    world.insert_resource(Players { list });
    world.insert_resource(MapInfo {
        size: Vec2::splat(size),
        seed: 1,
        start_positions: starts,
        crater_center: Vec2::splat(size * 0.5),
        crater_radius: size * 0.1,
    });
    world.insert_resource(Nav(nav));
    world.insert_resource(LocalPlayer(None));
    world.resource_mut::<SpatialIndex>().reset(Vec2::splat(size));
    ids
}

/// Run `n` sim ticks.
pub fn run_ticks(app: &mut App, n: u32) {
    // The very first update only primes the clock (0 fixed ticks).
    if app.world().resource::<SimClock>().tick == 0 && app.world().resource::<Time<Real>>().last_update().is_none() {
        app.update();
    }
    for _ in 0..n {
        app.update();
    }
}

/// Run a closure with `Commands` against the world and apply it
/// (spawn units/buildings in tests).
pub fn with_commands<R>(app: &mut App, f: impl FnOnce(&mut Commands, &GameData, &Players) -> R) -> R {
    let world = app.world_mut();
    let data = world.resource::<Data>().0.clone();
    let players = world.resource::<Players>().clone();
    let mut queue = BevyCommandQueue::default();
    let r = {
        let mut commands = Commands::new(&mut queue, world);
        f(&mut commands, &data, &players)
    };
    queue.apply(world);
    r
}

/// Push a command as if a player issued it (executes on the next tick).
pub fn command(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandInbox>().push(c);
}
