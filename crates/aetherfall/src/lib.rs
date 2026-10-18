//! Aetherfall app wiring, shared by the binary and integration tests.
//!
//! * [`add_sim_plugins`] — everything the simulation needs (headless-safe).
//! * [`add_client_plugins`] — input, UI and rendering (needs a window).
//! * [`headless_app`] / [`run_headless`] — fast-forward a match with no window.
//! * [`skirmish_setup`] — a `MatchSetup` from simple options.
//! * [`match_report`] — a text summary of the current match state.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use af_core::prelude::*;
use af_core::CorePlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;

pub mod profile;

pub use profile::{add_sim_profiler, SimProfile};

pub fn add_sim_plugins(app: &mut App) {
    app.add_plugins((
        CorePlugin,
        af_map::MapPlugin,
        af_movement::MovementPlugin,
        af_behavior::BehaviorPlugin,
        af_combat::CombatPlugin,
        af_economy::EconomyPlugin,
        af_fog::FogPlugin,
        af_ai::AiPlugin,
    ));
}

pub fn add_client_plugins(app: &mut App) {
    app.add_plugins((af_input::RtsInputPlugin, af_ui::HudPlugin, af_render::BoxRenderPlugin));
}

/// Options for a quick skirmish.
#[derive(Clone, Debug)]
pub struct SkirmishOptions {
    pub seed: u64,
    /// Faction ids per player slot; empty = cycle through all factions.
    pub factions: Vec<String>,
    pub players: usize,
    /// Slot 0 is human (else everyone is AI and the local view spectates).
    pub human: bool,
    pub fog: bool,
    pub reveal: bool,
    pub map_size: f32,
    pub start_age: u8,
    pub res_mult: f32,
    /// Teams: None = free for all.
    pub teams: Option<Vec<u8>>,
}

impl Default for SkirmishOptions {
    fn default() -> Self {
        SkirmishOptions {
            seed: 1,
            factions: Vec::new(),
            players: 2,
            human: true,
            fog: true,
            reveal: false,
            map_size: 0.0,
            start_age: 1,
            res_mult: 1.0,
            teams: None,
        }
    }
}

pub fn skirmish_setup(data: &GameData, o: &SkirmishOptions) -> Result<MatchSetup, String> {
    let n = o.players.clamp(1, 8);
    let mut players = Vec::new();
    for i in 0..n {
        let fid = match o.factions.get(i) {
            Some(id) => data.faction_id(id).ok_or_else(|| {
                format!("unknown faction `{id}` (have: {})", data.factions.iter().map(|f| f.id.as_str()).collect::<Vec<_>>().join(", "))
            })?,
            None => FactionId(((i + o.seed as usize) % data.factions.len()) as u8),
        };
        let human = o.human && i == 0;
        players.push(PlayerSetup {
            name: if human { "You".to_string() } else { format!("{} (AI)", data.faction(fid).name) },
            faction: fid,
            team: o.teams.as_ref().and_then(|t| t.get(i).copied()).unwrap_or(i as u8),
            controller: if human { Controller::Human } else { Controller::Ai },
            color: None,
        });
    }
    Ok(MatchSetup {
        seed: o.seed,
        map_size: o.map_size,
        players,
        local_player: if o.human { Some(0) } else { None },
        fog: o.fog,
        reveal_map: o.reveal,
        start_age: o.start_age.clamp(1, MAX_AGE),
        start_resources_mult: o.res_mult,
    })
}

/// A windowless app that runs `ticks_per_update` sim ticks per `update()`.
pub fn headless_app(data: Arc<GameData>, setup: MatchSetup, ticks_per_update: u32) -> App {
    let mut app = App::new();
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins((MinimalPlugins, StatesPlugin, bevy::log::LogPlugin { level: bevy::log::Level::WARN, ..default() }));
    app.insert_state(GameState::Playing);
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(ticks_per_update.max(1)));
    app.insert_resource(Data(data));
    app.insert_resource(setup);
    add_sim_plugins(&mut app);
    app.world_mut().resource_mut::<Time<Virtual>>().set_max_delta(Duration::from_secs(3600));
    app
}

/// Run a headless match for `ticks` sim ticks, calling `report` every
/// `report_every` ticks (0 = never). Stops early when the match ends.
pub fn run_headless(app: &mut App, ticks: u64, report_every: u64, mut report: impl FnMut(&mut App)) {
    app.update(); // primes the clock; enters Playing
    let mut last = 0;
    // Each update runs `ticks_per_update` ticks; also stop if the sim never
    // starts (e.g. no map plugin), instead of spinning forever.
    let mut stalled = 0;
    loop {
        app.update();
        if app.world().resource::<SimClock>().tick == last {
            stalled += 1;
            if stalled > 50 {
                eprintln!("headless: the simulation is not advancing (no Players/Nav resource? map setup failed?)");
                break;
            }
        } else {
            stalled = 0;
        }
        let tick = app.world().resource::<SimClock>().tick;
        if report_every > 0 && tick / report_every != last / report_every {
            report(app);
        }
        last = tick;
        if tick >= ticks || app.world().resource::<GameOutcome>().ended {
            break;
        }
    }
}

/// Human-readable state of the match (per player: age, resources, pop,
/// units/buildings by type, stats).
pub fn match_report(world: &mut World) -> String {
    let mut s = String::new();
    let clock = *world.resource::<SimClock>();
    let data = world.resource::<Data>().0.clone();
    let outcome = world.resource::<GameOutcome>().clone();
    let _ = writeln!(s, "--- t={:.0}s tick={} ---", clock.elapsed, clock.tick);
    if outcome.ended {
        let _ = writeln!(s, "MATCH OVER: winning team {:?} at {:.0}s", outcome.winning_team, outcome.time);
    }
    let Some(players) = world.get_resource::<Players>().cloned() else {
        let _ = writeln!(s, "(no players)");
        return s;
    };
    let mut q_fx = world.query::<&FxBox>();
    let fx = q_fx.iter(world).count();
    let mut q_nodes = world.query::<&ResourceNode>();
    let nodes = q_nodes.iter(world).count();
    for p in &players.list {
        let res: Vec<String> = data
            .resource_ids()
            .map(|r| format!("{}={}", data.resource(r).id, p.resources.get(r).floor()))
            .collect();
        let _ = writeln!(
            s,
            "P{} {} [{}] team {} age {} ({}){} pop {}/{} | {}",
            p.id.0,
            p.name,
            data.faction(p.faction).id,
            p.team,
            p.age,
            data.age(p.age).name,
            if p.defeated { " DEFEATED" } else { "" },
            p.pop_used,
            p.pop_cap,
            res.join(" ")
        );
        let units: Vec<String> = data
            .unit_ids()
            .filter(|u| p.units_owned[u.index()] > 0)
            .map(|u| format!("{}x{}", data.unit(u).id, p.units_owned[u.index()]))
            .collect();
        let buildings: Vec<String> = data
            .building_ids()
            .filter(|b| p.buildings_any[b.index()] > 0)
            .map(|b| {
                let done = p.buildings_owned[b.index()];
                let any = p.buildings_any[b.index()];
                if done == any { format!("{}x{}", data.building(b).id, any) } else { format!("{}x{}({} building)", data.building(b).id, any, any - done) }
            })
            .collect();
        let st = &p.stats;
        let gathered: f32 = st.gathered.total();
        let _ = writeln!(s, "    units: {}", if units.is_empty() { "-".into() } else { units.join(", ") });
        let _ = writeln!(s, "    buildings: {}", if buildings.is_empty() { "-".into() } else { buildings.join(", ") });
        let _ = writeln!(
            s,
            "    gathered {:.0} | trained {} lost {} | built {} lost {} | kills {} | techs {}",
            gathered, st.units_trained, st.units_lost, st.buildings_built, st.buildings_lost, st.kills, st.techs
        );
    }
    let _ = writeln!(s, "    world: {nodes} resource nodes, {fx} fx boxes");
    s
}
