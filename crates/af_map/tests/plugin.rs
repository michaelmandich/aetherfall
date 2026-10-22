//! MapPlugin in a headless app: match setup, default skirmish, map
//! options, teardown.

use std::sync::Arc;

use af_core::ext::map::{MapFeatures, MapOptions, MapSize, RiverCount, SiteChoice, SiteKind, WaterLevel};
use af_core::prelude::*;
use af_core::testing::{run_ticks, sim_app, test_data};
use af_map::*;
use bevy::prelude::*;

fn setup(data: &GameData, factions: &[&str]) -> MatchSetup {
    let players = factions
        .iter()
        .enumerate()
        .map(|(i, f)| PlayerSetup {
            name: f.to_string(),
            faction: data.faction_id(f).unwrap(),
            team: i as u8,
            controller: if i == 0 { Controller::Human } else { Controller::Ai },
            color: None,
        })
        .collect();
    MatchSetup { seed: 4242, players, local_player: Some(0), ..MatchSetup::default() }
}

fn count<C: Component>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), With<C>>().iter(world).count()
}

#[test]
fn headless_match_builds_the_whole_world() {
    let data = test_data();
    let s = setup(&data, &["ironhold", "sylvan", "dune", "tide", "steppe", "starborn"]);
    let expected = generate(&data, &s);
    let mut app = sim_app(data.clone());
    app.insert_resource(s.clone());
    app.add_plugins(MapPlugin);
    run_ticks(&mut app, 2);

    let world = app.world();
    assert_eq!(world.resource::<SimClock>().tick, 2, "the sim runs once the world exists");
    let info = world.resource::<MapInfo>();
    assert_eq!(info.size, Vec2::splat(expected.size));
    assert_eq!(info.start_positions, expected.starts);
    assert_eq!(info.seed, 4242);
    let features = world.resource::<MapFeatures>();
    assert_eq!(*features, expected.features, "the map's features are published");
    assert_eq!((info.crater_center, info.crater_radius), (features.sites[0].center, features.sites[0].radius));
    let nav = world.resource::<Nav>();
    assert_eq!((nav.width(), nav.height()), (expected.width, expected.height));
    assert_eq!(nav.terrain_cells().len(), expected.terrain.len());
    assert_eq!(world.resource::<LocalPlayer>().0, Some(PlayerId(0)));
    assert_eq!(world.resource::<CameraRig>().focus, expected.starts[0]);
    let fog = world.resource::<FogOfWar>();
    assert!(fog.enabled);
    assert_eq!((fog.width, fog.cell, fog.teams.len()), ((expected.size / 2.0).ceil() as u32, 2.0, 6));

    let players = world.resource::<Players>().clone();
    assert_eq!(players.list.len(), 6);
    for p in &players.list {
        let faction = data.faction(p.faction);
        assert_eq!(p.start_pos, expected.starts[p.id.index()]);
        let town_centers: u32 = data.building_ids().filter(|b| data.building(*b).has_tag(Tag::TownCenter)).map(|b| p.buildings_owned[b.index()]).sum();
        assert!(town_centers >= 1, "{} owns a town centre", faction.id);
        for (id, n) in &faction.start.units {
            let kind = data.unit_id(id).unwrap();
            assert_eq!(p.units_owned[kind.index()], n * data.unit(kind).squad, "{} starts with its {id}", faction.id);
        }
        assert!(p.pop_cap > 0 && p.pop_used > 0);
    }
    assert_eq!(count::<ResourceNode>(&mut app), expected.nodes.len());
    assert_eq!(count::<Building>(&mut app), expected.buildings.len());
    assert_eq!(count::<Unit>(&mut app), expected.units.len());
}

#[test]
fn start_squads_share_one_squad_id_per_squadron() {
    let mut data = (*test_data()).clone();
    let first = data.factions[0].id.clone();
    data.factions[0].start.units.push(("rifle_squad".to_string(), 2));
    data.factions[0].start.buildings.push("barracks".to_string());
    let data = Arc::new(data);
    let rifles = data.unit_id("rifle_squad").unwrap();
    let barracks = data.building_id("barracks").unwrap();
    let models = data.unit(rifles).squad as usize;
    let mut app = sim_app(data.clone());
    app.insert_resource(setup(&data, &[&first, "tide", &first]));
    app.add_plugins(MapPlugin);
    run_ticks(&mut app, 2);

    let world = app.world_mut();
    let mut squads: Vec<(u32, Option<PlayerId>, usize)> = Vec::new();
    for (unit, owner, member) in world.query::<(&Unit, Option<&Owner>, &SquadMember)>().iter(world) {
        assert_eq!(unit.kind, rifles, "only squad units get a SquadMember");
        let owner = owner.map(|o| o.0);
        match squads.iter_mut().find(|(id, _, _)| *id == member.squad) {
            Some((_, o, members)) => {
                assert_eq!(*o, owner, "a squad has one owner");
                *members += 1;
            }
            None => squads.push((member.squad, owner, 1)),
        }
    }
    assert_eq!(squads.len(), 4, "two squads for each of the two players of that faction: {squads:?}");
    assert!(squads.iter().all(|(_, _, members)| *members == models), "{squads:?}");
    let players = world.resource::<Players>();
    for p in [0, 2] {
        assert_eq!(players.list[p].units_owned[rifles.index()], 2 * models as u32);
        assert_eq!(players.list[p].buildings_owned[barracks.index()], 1, "player {p} starts with its barracks");
    }
}

#[test]
fn missing_setup_starts_the_default_skirmish() {
    let data = test_data();
    let mut app = sim_app(data.clone());
    app.add_plugins(MapPlugin);
    run_ticks(&mut app, 2);
    let world = app.world();
    let players = world.resource::<Players>();
    assert_eq!(players.list.len(), 2);
    assert_eq!((players.list[0].faction, players.list[1].faction), (FactionId(0), FactionId(1)));
    assert_eq!(players.list[0].controller, Controller::Human);
    assert_eq!(world.resource::<LocalPlayer>().0, Some(PlayerId(0)));
    assert!(world.resource::<FogOfWar>().enabled);
    assert_eq!(world.resource::<MatchSetup>().players.len(), 2, "the setup used is recorded");
    assert_eq!(world.resource::<MapInfo>().size, Vec2::splat(224.0));
}

#[test]
fn map_options_shape_the_map_and_are_published_resolved() {
    let data = test_data();
    let s = setup(&data, &["ironhold", "tide", "dune"]);
    let options = MapOptions { size: MapSize::Large, water: WaterLevel::Coast, rivers: RiverCount::Two, site: SiteChoice::Kind(SiteKind::Lake) };
    let expected = generate_with(&data, &s, &options);
    let mut app = sim_app(data.clone());
    app.insert_resource(s);
    app.insert_resource(options);
    app.add_plugins(MapPlugin);
    run_ticks(&mut app, 1);
    let world = app.world();
    let features = world.resource::<MapFeatures>();
    assert_eq!(*features, expected.features);
    let o = features.options;
    assert_eq!((o.size, o.water, o.rivers, o.site), (MapSize::Large, WaterLevel::Coast, 2, SiteKind::Lake));
    assert_eq!(features.ocean_edges.len(), 1);
    assert_eq!(features.rivers.len(), 2);
    // A Large land for three players plus one ocean band.
    let side = MapSize::Large.land_side(3);
    assert_eq!(world.resource::<MapInfo>().size, Vec2::splat(side + (side * 0.17).round()));
    assert_eq!(world.resource::<Nav>().width(), expected.width);
}

#[test]
fn spectating_focuses_the_crater_and_reveal_explores_everything() {
    let data = test_data();
    let mut s = setup(&data, &["tide", "tide", "tide"]);
    s.local_player = None;
    s.reveal_map = true;
    s.fog = false;
    let mut app = sim_app(data.clone());
    app.insert_resource(s);
    app.add_plugins(MapPlugin);
    run_ticks(&mut app, 1);
    let world = app.world();
    assert_eq!(world.resource::<LocalPlayer>().0, None);
    let info = world.resource::<MapInfo>();
    assert_eq!(world.resource::<CameraRig>().focus, info.crater_center);
    let fog = world.resource::<FogOfWar>();
    assert!(!fog.enabled);
    assert!(fog.teams.iter().all(|t| t.explored.iter().all(|e| *e) && t.visible.iter().all(|v| *v > 0)));
    let colors: Vec<[f32; 3]> = world.resource::<Players>().iter().map(|p| p.color).collect();
    assert!(colors[0] != colors[1] && colors[1] != colors[2] && colors[0] != colors[2], "{colors:?}");
}

#[test]
fn leaving_the_match_tears_it_down_and_a_new_one_can_start() {
    let data = test_data();
    let mut app = sim_app(data.clone());
    app.insert_resource(setup(&data, &["steppe", "starborn"]));
    app.add_plugins(MapPlugin);
    run_ticks(&mut app, 3);
    assert!(count::<ResourceNode>(&mut app) > 100);
    assert!(!app.world().resource::<SpatialIndex>().entries.is_empty());
    let fx = FxBox { pos: Vec3::ONE, prev_pos: Vec3::ONE, dir: Vec3::Z, size: Vec3::splat(0.2), color: [1.0; 3], glow: true, alpha: 1.0 };
    app.world_mut().spawn(fx);
    app.world_mut().spawn(ClickPing { pos: Vec2::ONE, time_left: 5.0, color: [1.0; 3] });

    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Menu);
    app.update();
    assert_eq!(count::<Unit>(&mut app), 0);
    assert_eq!(count::<Building>(&mut app), 0);
    assert_eq!(count::<ResourceNode>(&mut app), 0);
    assert_eq!(count::<FxBox>(&mut app), 0);
    assert_eq!(count::<ClickPing>(&mut app), 0);
    let world = app.world();
    assert!(!world.contains_resource::<Players>() && !world.contains_resource::<Nav>() && !world.contains_resource::<MapInfo>());
    assert!(!world.contains_resource::<MapFeatures>());
    assert!(world.resource::<SpatialIndex>().entries.is_empty(), "no despawned entities left to find from the menu");
    assert_eq!(world.resource::<LocalPlayer>().0, None);

    let mut next = setup(&data, &["dune", "ironhold", "sylvan"]);
    next.seed = 7;
    app.insert_resource(next);
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Playing);
    app.update();
    app.update();
    let world = app.world();
    assert_eq!(world.resource::<Players>().list.len(), 3);
    assert_eq!(world.resource::<MapInfo>().seed, 7);
    assert!(world.contains_resource::<MapFeatures>());
    assert!(world.resource::<SimClock>().tick <= 2, "the clock restarted");
    assert_eq!(count::<Building>(&mut app), 3);
}
