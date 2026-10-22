//! Aetherfall map generation and match setup (terrain, resources, start
//! positions, initial spawns, teardown).
//!
//! * [`generate_with`] is a pure, deterministic generator: content, a
//!   `MatchSetup` and the `MapOptions` in, a [`GeneratedMap`] (terrain,
//!   starts, meteor sites, nodes, start buildings and units, and the
//!   `MapFeatures`) out. No ECS involved. [`generate`] uses the default
//!   options.
//! * [`MapPlugin`] builds the match world in `OnEnter(GameState::Playing)`
//!   and tears it down in `OnExit(GameState::Playing)`, both in
//!   [`MapSetupSet`].
//!
//! On enter, the map is generated for the inserted `MatchSetup` and
//! `MapOptions` (each defaulting when absent), and every match resource is
//! (re)inserted here and nowhere else: `Players`, `MapInfo`, `MapFeatures`,
//! `Nav` (terrain only: af_movement adds the obstacles from
//! `Added<Obstacle>` on the first tick), `LocalPlayer`, `SimRng`,
//! `SimClock`, `CommandInbox`, `TickCommands`, `GameOutcome`, `Notices`,
//! `FogOfWar` and the resolved `MatchSetup`; `SpatialIndex` is resized and
//! `CameraRig::focus` moved to the local player's start.
//!
//! On exit, every `Unit`, `Building`, `ResourceNode`, `FxBox` and
//! `ClickPing` is despawned, `Players`, `Nav`, `MapInfo` and `MapFeatures`
//! are removed, `SpatialIndex` is emptied and `LocalPlayer` set to `None`.

mod mapgen;
mod setup;

use af_core::ext::map::{MapFeatures, MapOptions, MapSetupSet};
use af_core::prelude::*;
use bevy::prelude::*;

pub use mapgen::{
    generate, generate_with, map_size_for, resolve_map_options, size_for_side, BuildingPlacement, GeneratedMap, NodePlacement, UnitPlacement,
    CORRIDOR_HALF_WIDTH, START_AREA_RADIUS, START_CLEAR_RADIUS,
};
pub use setup::{build_players, default_setup, new_fog, resolve_setup, FOG_CELL, MAX_PLAYERS, MIN_COLOR_DISTANCE};

/// Match setup and teardown (see the crate docs).
pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), start_match.in_set(MapSetupSet))
            .add_systems(OnExit(GameState::Playing), end_match.in_set(MapSetupSet));
    }
}

/// Generate the map for the inserted `MatchSetup` (or the default
/// skirmish) and `MapOptions` (or the defaults), spawn the start entities
/// and reset every match resource.
fn start_match(
    mut commands: Commands,
    data: Res<Data>,
    setup: Option<Res<MatchSetup>>,
    options: Option<Res<MapOptions>>,
    mut spatial: ResMut<SpatialIndex>,
    mut camera: ResMut<CameraRig>,
) {
    let setup = resolve_setup(&data, setup.as_deref());
    let map = generate_with(&data, &setup, &options.as_deref().copied().unwrap_or_default());
    let players = build_players(&data, &setup, &map.starts);
    spawn_start_entities(&mut commands, &data, &players, &map);
    let o = map.features.options;
    info!(
        "match: {} players on a {} m map (seed {}; {}, water {}, {} river(s), {}), {} resource nodes",
        players.list.len(),
        map.size,
        setup.seed,
        o.size.name(),
        o.water.name(),
        map.features.rivers.len(),
        o.site.name(),
        map.nodes.len()
    );

    let local = setup.local_player.map(|i| PlayerId(i as u8));
    camera.focus = local.and_then(|p| players.try_get(p)).map_or(map.crater_center, |p| p.start_pos);
    spatial.reset(Vec2::splat(map.size));
    commands.insert_resource(Nav(map.nav(&data)));
    commands.insert_resource(MapInfo {
        size: Vec2::splat(map.size),
        seed: setup.seed,
        start_positions: map.starts.clone(),
        crater_center: map.crater_center,
        crater_radius: map.crater_radius,
    });
    commands.insert_resource(map.features.clone());
    commands.insert_resource(new_fog(&setup, map.size, &players));
    commands.insert_resource(players);
    commands.insert_resource(LocalPlayer(local));
    commands.insert_resource(SimRng::new(setup.seed));
    commands.insert_resource(SimClock::default());
    commands.insert_resource(CommandInbox::default());
    commands.insert_resource(TickCommands::default());
    commands.insert_resource(GameOutcome::default());
    commands.insert_resource(Notices::default());
    commands.insert_resource(setup);
}

/// Spawn the start buildings (complete), start units (squads sharing a
/// `SquadMember`) and every resource node, through `af_core::spawn`.
fn spawn_start_entities(commands: &mut Commands, data: &GameData, players: &Players, map: &GeneratedMap) {
    for b in &map.buildings {
        if let Some(owner) = players.list.get(b.player) {
            spawn_building(commands, data, owner, b.kind, b.pos, b.yaw, true);
        }
    }
    let mut squads: Vec<(u32, u32)> = Vec::new();
    for u in &map.units {
        let Some(owner) = players.list.get(u.player) else { continue };
        let unit = spawn_unit(commands, data, owner, u.kind, u.pos, u.yaw);
        if let Some(group) = u.squad {
            let squad = match squads.iter().find(|(g, _)| *g == group) {
                Some((_, id)) => *id,
                None => {
                    let id = af_core::next_seq();
                    squads.push((group, id));
                    id
                }
            };
            commands.entity(unit).insert(SquadMember { squad });
        }
    }
    for n in &map.nodes {
        spawn_node(commands, data, n.kind, n.pos, n.yaw, None);
    }
}

/// Despawn everything the match created and remove the per-match world
/// resources, so the menu can start a fresh match.
#[allow(clippy::type_complexity)]
fn end_match(
    mut commands: Commands,
    entities: Query<Entity, Or<(With<Unit>, With<Building>, With<ResourceNode>, With<FxBox>, With<ClickPing>)>>,
    mut spatial: ResMut<SpatialIndex>,
    mut local: ResMut<LocalPlayer>,
) {
    for e in &entities {
        commands.entity(e).try_despawn();
    }
    // After the despawns, so removal observers still find the grid.
    commands.remove_resource::<Players>();
    commands.remove_resource::<Nav>();
    commands.remove_resource::<MapInfo>();
    commands.remove_resource::<MapFeatures>();
    // The sim, which rebuilds the index each tick, stops without `Players`;
    // client systems keep running in the menu and must not get the despawned
    // entities (or a stale local slot) back.
    spatial.clear();
    local.0 = None;
}
