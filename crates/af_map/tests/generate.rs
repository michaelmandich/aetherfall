//! The pure generator against the real content.

use std::collections::VecDeque;
use std::time::Instant;

use af_core::prelude::*;
use af_core::testing::test_data;
use af_map::*;
use bevy::prelude::*;

/// `n` AI players cycling through every faction.
fn setup(data: &GameData, n: usize, seed: u64) -> MatchSetup {
    let players = (0..n)
        .map(|i| {
            let faction = FactionId((i % data.factions.len()) as u8);
            PlayerSetup { name: format!("P{i}"), faction, team: i as u8, controller: Controller::Ai, color: None }
        })
        .collect();
    MatchSetup { seed, players, ..MatchSetup::default() }
}

fn node_obb(data: &GameData, n: &NodePlacement) -> Obb {
    Obb::from_size(n.pos, data.node(n.kind).size, n.yaw)
}

fn building_obb(data: &GameData, b: &BuildingPlacement) -> Obb {
    Obb::from_size(b.pos, data.building(b.kind).size, b.yaw)
}

fn unit_obb(data: &GameData, u: &UnitPlacement) -> Obb {
    Obb::from_size(u.pos, data.unit(u.kind).size, u.yaw)
}

/// Each player's main hall (the first building listed for it).
fn halls(map: &GeneratedMap) -> Vec<BuildingPlacement> {
    (0..map.starts.len()).filter_map(|p| map.buildings.iter().find(|b| b.player == p).copied()).collect()
}

/// Pairs of overlapping boxes, by sweep along x.
fn overlapping_pairs(boxes: &[Obb]) -> Vec<(usize, usize)> {
    let mut order: Vec<(f32, f32, usize)> = boxes
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let (min, max) = b.aabb();
            (min.x, max.x, i)
        })
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut pairs = Vec::new();
    for (k, &(_, max_x, i)) in order.iter().enumerate() {
        for &(min_x, _, j) in &order[k + 1..] {
            if min_x >= max_x {
                break;
            }
            if boxes[i].overlaps(&boxes[j]) {
                pairs.push((i.min(j), i.max(j)));
            }
        }
    }
    pairs
}

/// Every cell's centre and terrain.
fn cells(map: &GeneratedMap) -> impl Iterator<Item = (Vec2, TerrainId)> + '_ {
    let w = map.width as usize;
    map.terrain.iter().enumerate().map(move |(i, t)| (Vec2::new((i % w) as f32 + 0.5, (i / w) as f32 + 0.5), *t))
}

/// Bounding-box size of every 4-connected body of `kind` cells.
fn bodies(map: &GeneratedMap, kind: TerrainId) -> Vec<Vec2> {
    let (w, h) = (map.width as i32, map.height as i32);
    let mut seen = vec![false; map.terrain.len()];
    let mut sizes = Vec::new();
    for first in 0..map.terrain.len() {
        if seen[first] || map.terrain[first] != kind {
            continue;
        }
        seen[first] = true;
        let mut queue = VecDeque::from([IVec2::new(first as i32 % w, first as i32 / w)]);
        let (mut lo, mut hi) = (queue[0], queue[0]);
        while let Some(c) = queue.pop_front() {
            (lo, hi) = (lo.min(c), hi.max(c));
            for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let n = c + d;
                if n.x < 0 || n.y < 0 || n.x >= w || n.y >= h {
                    continue;
                }
                let i = (n.y * w + n.x) as usize;
                if !seen[i] && map.terrain[i] == kind {
                    seen[i] = true;
                    queue.push_back(n);
                }
            }
        }
        sizes.push((hi - lo + IVec2::ONE).as_vec2());
    }
    sizes
}

/// The terrain grid plus every building and blocking node as an obstacle.
fn nav_with_obstacles(data: &GameData, map: &GeneratedMap) -> NavGrid {
    let mut nav = map.nav(data);
    for b in &map.buildings {
        nav.add_obstacle(&building_obb(data, b), 1);
    }
    for n in map.nodes.iter().filter(|n| data.node(n.kind).blocking) {
        nav.add_obstacle(&node_obb(data, n), 1);
    }
    nav
}

#[test]
fn same_seed_same_map_and_different_seeds_differ() {
    let data = test_data();
    let a = generate(&data, &setup(&data, 4, 77));
    let b = generate(&data, &setup(&data, 4, 77));
    assert_eq!(a, b);
    let c = generate(&data, &setup(&data, 4, 78));
    assert_ne!(a.terrain, c.terrain);
    assert_ne!(a.starts, c.starts);
    assert_ne!(a.nodes, c.nodes);
}

#[test]
fn every_player_count_and_faction_gets_a_map() {
    let data = test_data();
    for n in 1..=8 {
        let map = generate(&data, &setup(&data, n, n as u64 * 11));
        assert_eq!(map.size, map_size_for(n));
        assert_eq!((map.width, map.height), (map.size as u32, map.size as u32));
        assert_eq!(map.terrain.len(), (map.width * map.height) as usize);
        assert_eq!(map.starts.len(), n);
        assert_eq!(map.crater_center, Vec2::splat(map.size * 0.5));
        for p in 0..n {
            let faction = data.faction(FactionId((p % data.factions.len()) as u8));
            let hall = map.buildings.iter().find(|b| b.player == p).expect("a main hall");
            assert_eq!(data.building(hall.kind).id, faction.start.buildings[0]);
            assert_eq!(hall.pos, map.starts[p]);
            let units = map.units.iter().filter(|u| u.player == p).count() as u32;
            let expected: u32 = faction.start.units.iter().map(|(id, n)| n * data.unit(data.unit_id(id).unwrap()).squad).sum();
            assert_eq!(units, expected, "start units of {}", faction.id);
            // Every start is inside the map with room for its whole area.
            assert!(map.starts[p].min_element() > 30.0 && map.starts[p].max_element() < map.size - 30.0);
        }
    }
}

#[test]
fn map_size_override_is_respected_and_clamped() {
    let data = test_data();
    let mut s = setup(&data, 2, 5);
    s.map_size = 300.4;
    assert_eq!(generate(&data, &s).size, 300.0);
    s.map_size = 20.0;
    let tiny = generate(&data, &s);
    assert_eq!(tiny.size, 160.0);
    assert_eq!(halls(&tiny).len(), 2);
}

#[test]
fn degenerate_setups_still_generate() {
    let data = test_data();
    let empty = generate(&data, &setup(&data, 0, 1));
    assert!(empty.starts.is_empty() && empty.buildings.is_empty() && empty.units.is_empty());
    assert!(!empty.nodes.is_empty(), "the crater and the wilds are still there");

    let mut crowded = setup(&data, 8, 2);
    crowded.map_size = 160.0;
    let map = generate(&data, &crowded);
    assert_eq!(halls(&map).len(), 8);
    let boxes: Vec<Obb> = map.nodes.iter().map(|n| node_obb(&data, n)).chain(map.buildings.iter().map(|b| building_obb(&data, b))).collect();
    assert!(overlapping_pairs(&boxes).is_empty());

    let mut unknown = setup(&data, 2, 3);
    unknown.players[1].faction = FactionId(250);
    let map = generate(&data, &unknown);
    assert_eq!(map.starts.len(), 2);
    assert!(map.buildings.iter().all(|b| b.player == 0) && map.units.iter().all(|u| u.player == 0));

    let mut huge = setup(&data, 2, 4);
    huge.map_size = 5000.0;
    assert_eq!(generate(&data, &huge).size, 1024.0);
}

#[test]
fn start_discs_are_clear_and_buildable_for_the_main_hall() {
    let data = test_data();
    for (n, seed) in [(2, 1), (4, 2), (6, 3), (8, 4), (8, 5)] {
        let map = generate(&data, &setup(&data, n, seed));
        let nav = map.nav(&data);
        let nodes: Vec<Obb> = map.nodes.iter().map(|n| node_obb(&data, n)).collect();
        for hall in halls(&map) {
            let obb = building_obb(&data, &hall);
            assert!(nav.area_buildable(&obb), "hall of player {} not buildable (seed {seed})", hall.player);
            assert!(nodes.iter().all(|n| !n.overlaps(&obb)), "a node sits on the hall");
            let start = map.starts[hall.player];
            let clear = START_CLEAR_RADIUS - 0.5;
            let mut p = start - Vec2::splat(clear);
            while p.y <= start.y + clear {
                while p.x <= start.x + clear {
                    if p.distance(start) <= clear {
                        let t = map.terrain_at(p).unwrap();
                        assert!(data.terrain_def(t).buildable, "start disc of {} has {}", hall.player, data.terrain_def(t).id);
                    }
                    p.x += 1.0;
                }
                p.x = start.x - clear;
                p.y += 1.0;
            }
        }
    }
}

#[test]
fn every_start_and_the_crater_are_reachable_on_foot() {
    let data = test_data();
    let foot = Mobility::of(MoveFlag::Foot).0;
    for (n, seed) in [(2, 10), (3, 11), (4, 12), (5, 13), (6, 14), (7, 15), (8, 16), (8, 17)] {
        let map = generate(&data, &setup(&data, n, seed));
        let nav = nav_with_obstacles(&data, &map);
        // Flood from the crater centre with a 1 m clearance radius (3x3 free cells).
        let origin = nav.cell_of(map.crater_center);
        assert!(nav.clear_cell(origin, foot, 1.0, af_geom::NO_TEAM), "crater centre blocked (seed {seed})");
        let mut seen = vec![false; (nav.width() * nav.height()) as usize];
        let mut queue = VecDeque::from([origin]);
        seen[nav.index(origin)] = true;
        while let Some(c) = queue.pop_front() {
            for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let next = c + d;
                if nav.in_bounds(next) && !seen[nav.index(next)] && nav.clear_cell(next, foot, 1.0, af_geom::NO_TEAM) {
                    seen[nav.index(next)] = true;
                    queue.push_back(next);
                }
            }
        }
        for hall in halls(&map) {
            let obb = building_obb(&data, &hall);
            let ahead = (map.crater_center - hall.pos).normalize();
            let front = hall.pos + ahead * (obb.support(ahead) + 2.5);
            assert!(seen[nav.index(nav.cell_of(front))], "player {} of {n} cannot walk to the crater (seed {seed})", hall.player);
        }
    }
}

#[test]
fn every_player_has_the_listed_start_resources() {
    let data = test_data();
    let node = |id: &str| data.node_id(id).unwrap();
    let wood = data.resource_id("wood").unwrap();
    // (what, min count, between these distances in metres from the start)
    let wanted: [(&str, usize, f32, f32); 7] = [
        ("berry_bush", 6, 7.0, 16.0),
        ("stone_outcrop", 2, 13.0, 22.0),
        ("copper_vein", 2, 15.0, 24.0),
        ("iron_vein", 2, 17.0, 26.0),
        ("trees", 40, 19.0, 31.0),
        ("silicon_deposit", 2, 33.0, 45.0),
        ("aether_crystal", 3, 45.0, 60.0),
    ];
    for (n, seed) in [(1, 20), (2, 21), (2, 27), (3, 22), (4, 23), (6, 24), (8, 25), (8, 26)] {
        let map = generate(&data, &setup(&data, n, seed));
        for (what, min, from, within) in wanted {
            let counts: Vec<usize> = map
                .starts
                .iter()
                .map(|s| {
                    map.nodes
                        .iter()
                        .filter(|nd| (from..=within).contains(&nd.pos.distance(*s)))
                        .filter(|nd| match what {
                            "trees" => data.node(nd.kind).resource_id == wood && nd.kind != node("palm"),
                            id => nd.kind == node(id),
                        })
                        .count()
                })
                .collect();
            assert!(counts.iter().all(|c| *c >= min), "{what} at {from}-{within} m: {counts:?} (seed {seed}, {n} players)");
        }
    }
}

#[test]
fn start_resources_are_mirrored_between_players() {
    let data = test_data();
    for (n, seed) in [(2, 31), (4, 32), (6, 33)] {
        let map = generate(&data, &setup(&data, n, seed));
        let wood = data.resource_id("wood").unwrap();
        let mut signatures = Vec::new();
        for s in &map.starts {
            let mut kinds: Vec<u16> = map
                .nodes
                .iter()
                .filter(|nd| nd.pos.distance(*s) <= 30.0 && data.node(nd.kind).resource_id != wood)
                .map(|nd| nd.kind.0)
                .collect();
            kinds.sort();
            signatures.push(kinds);
        }
        assert!(signatures.windows(2).all(|w| w[0] == w[1]), "start resources differ between players: {signatures:?} (seed {seed})");
    }
}

#[test]
fn no_boxes_overlap_and_nodes_stand_on_buildable_ground() {
    let data = test_data();
    for (n, seed) in [(2, 41), (4, 42), (8, 43)] {
        let map = generate(&data, &setup(&data, n, seed));
        let nav = map.nav(&data);
        let mut boxes: Vec<Obb> = map.nodes.iter().map(|nd| node_obb(&data, nd)).collect();
        for (nd, obb) in map.nodes.iter().zip(&boxes) {
            assert!(nav.area_buildable(obb), "{} at {:?} is not on buildable ground", data.node(nd.kind).id, nd.pos);
        }
        boxes.extend(map.buildings.iter().map(|b| building_obb(&data, b)));
        boxes.extend(map.units.iter().map(|u| unit_obb(&data, u)));
        let pairs = overlapping_pairs(&boxes);
        assert!(pairs.is_empty(), "{} overlapping boxes, e.g. {:?} (seed {seed})", pairs.len(), pairs.first().map(|(a, b)| (boxes[*a], boxes[*b])));
    }
}

#[test]
fn crater_and_contested_spots_hold_aether_and_adamant() {
    let data = test_data();
    let vent = data.node_id("aether_vent").unwrap();
    let meteor = data.node_id("adamant_meteor").unwrap();
    let crystal = data.node_id("aether_crystal").unwrap();
    for (n, seed) in [(1, 51), (2, 52), (3, 53), (5, 54), (8, 55)] {
        let map = generate(&data, &setup(&data, n, seed));
        let in_crater = |k: NodeTypeId| map.nodes.iter().filter(|nd| nd.kind == k && nd.pos.distance(map.crater_center) < map.crater_radius).count();
        assert!((1..=3).contains(&in_crater(vent)), "crater vents: {} (seed {seed})", in_crater(vent));
        assert!((2..=4).contains(&in_crater(meteor)), "crater meteors: {} (seed {seed})", in_crater(meteor));
        assert!((8..=12).contains(&in_crater(crystal)), "crater crystals: {} (seed {seed})", in_crater(crystal));
        // One spot between each pair of neighbours: both sides for two
        // players, opposite the start for one.
        let spots = n;
        let outside = |k: NodeTypeId| map.nodes.iter().filter(|nd| nd.kind == k && nd.pos.distance(map.crater_center) > map.crater_radius * 1.5).count();
        assert_eq!(outside(vent), spots, "one vent per contested spot (seed {seed})");
        assert!(outside(meteor) >= spots, "a meteor per contested spot (seed {seed})");
    }
}

#[test]
fn every_map_has_every_terrain_feature_in_the_wilds() {
    let data = test_data();
    let terrain = |id: &str| data.terrain_id(id).unwrap();
    let (palm, silicon) = (data.node_id("palm").unwrap(), data.node_id("silicon_deposit").unwrap());
    for n in [2, 4, 6, 8] {
        for seed in 0..40 {
            let map = generate(&data, &setup(&data, n, seed));
            // Outside the start areas (whose quartz patches are sand) and the
            // crater (whose rim is cliff and surroundings marsh and thicket).
            let wild = |p: Vec2| {
                p.distance(map.crater_center) > map.crater_radius + 5.0 && map.starts.iter().all(|s| p.distance(*s) > START_AREA_RADIUS + 1.0)
            };
            let count = |id: &str| cells(&map).filter(|(_, t)| *t == terrain(id)).count();
            let wild_count = |id: &str| cells(&map).filter(|(p, t)| *t == terrain(id) && wild(*p)).count();
            let wild_nodes = |k: NodeTypeId| map.nodes.iter().filter(|nd| nd.kind == k && wild(nd.pos)).count();
            let at = format!("seed {seed}, {n} players");
            assert!(wild_count("sand") > 60, "a dune region ({at})");
            assert!(wild_nodes(palm) > 0 && wild_nodes(silicon) > 0, "palms and quartz on the dunes ({at})");
            assert!(wild_count("cliff") > 30, "a ridge or highland ({at})");
            assert!(count("deep_water") > 0 && count("shallow_water") > 50, "water with shallow banks or fords ({at})");
            assert!(count("marsh") > 0 && count("thicket") > 0, "marsh and thicket ({at})");
            assert!(count("grass") > map.terrain.len() / 2, "mostly grass ({at})");
        }
    }
}

#[test]
fn deep_water_can_be_crossed_at_least_every_40_m() {
    let data = test_data();
    let deep = data.terrain_id("deep_water").unwrap();
    for n in [2, 4, 8] {
        for seed in 0..20 {
            let map = generate(&data, &setup(&data, n, seed));
            // Fords split every river into stretches: no deep body may run
            // on for more than ~40 m without a shallow crossing.
            for size in bodies(&map, deep) {
                assert!(size.max_element() <= 40.0, "{size} m of deep water without a ford (seed {seed}, {n} players)");
            }
        }
    }
}

#[test]
fn extra_start_buildings_and_squads_are_placed_in_the_start_area() {
    let mut data = (*test_data()).clone();
    let first = &mut data.factions[0].start;
    first.buildings.extend(["barracks".to_string(), "house".to_string(), "house".to_string()]);
    first.units.push(("rifle_squad".to_string(), 2));
    let rifles = data.unit_id("rifle_squad").unwrap();
    let models = data.unit(rifles).squad as usize;
    for (n, seed) in [(2, 81), (4, 82), (8, 83)] {
        let map = generate(&data, &setup(&data, n, seed));
        for p in (0..n).filter(|p| p % data.factions.len() == 0) {
            let start = map.starts[p];
            let own: Vec<&BuildingPlacement> = map.buildings.iter().filter(|b| b.player == p).collect();
            assert_eq!(own.len(), 4, "the hall and three extra buildings (seed {seed})");
            for b in &own[1..] {
                let obb = building_obb(&data, b);
                let id = &data.building(b.kind).id;
                assert!(b.pos.distance(start) + obb.bounding_radius() <= START_AREA_RADIUS, "{id} outside the start area");
                assert!(obb.segment_distance(start, map.crater_center) >= CORRIDOR_HALF_WIDTH, "{id} in the corridor");
            }
            let mut squads: Vec<(u32, usize)> = Vec::new();
            for u in map.units.iter().filter(|u| u.player == p && u.kind == rifles) {
                let group = u.squad.expect("squad models carry a squad group");
                match squads.iter_mut().find(|(g, _)| *g == group) {
                    Some((_, members)) => *members += 1,
                    None => squads.push((group, 1)),
                }
            }
            assert_eq!(squads.len(), 2, "two rifle squads (seed {seed})");
            assert!(squads.iter().all(|(_, members)| *members == models), "{squads:?}");
        }
        assert!(map.units.iter().filter(|u| u.kind != rifles).all(|u| u.squad.is_none()), "single units are no squads");
        let boxes: Vec<Obb> = map
            .nodes
            .iter()
            .map(|nd| node_obb(&data, nd))
            .chain(map.buildings.iter().map(|b| building_obb(&data, b)))
            .chain(map.units.iter().map(|u| unit_obb(&data, u)))
            .collect();
        assert!(overlapping_pairs(&boxes).is_empty(), "overlaps (seed {seed})");
    }
}

#[test]
fn a_384_m_map_generates_well_under_a_second() {
    let data = test_data();
    let s = setup(&data, 8, 99);
    let started = Instant::now();
    let map = generate(&data, &s);
    let took = started.elapsed();
    assert_eq!(map.size, 384.0);
    assert!(took.as_secs_f32() < 1.0, "took {took:?}");
    println!("384 m, 8 players: {took:?}, {} nodes", map.nodes.len());
}
