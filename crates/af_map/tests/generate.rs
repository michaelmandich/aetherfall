//! The pure generator against the real content
//! (`docs/design/maps-and-terrain.md` section 8).
//!
//! Most checks run over one shared sample of maps covering every map size,
//! water level and meteor-site kind (with player counts from 2 to 8 and
//! both river counts cycling through it); `--ignored` runs the full sweep
//! over every combination and several seeds.

use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::Instant;

use af_core::ext::map::{MapEdge, MapFeatures, MapOptions, MapSize, RiverCount, SiteChoice, SiteKind, WaterLevel};
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

fn options(size: MapSize, water: WaterLevel, rivers: u8, site: SiteKind) -> MapOptions {
    let rivers = if rivers == 1 { RiverCount::One } else { RiverCount::Two };
    MapOptions { size, water, rivers, site: SiteChoice::Kind(site) }
}

/// One generated map and what it was generated from.
struct Case {
    players: usize,
    seed: u64,
    options: MapOptions,
    map: GeneratedMap,
}

impl Case {
    fn new(data: &GameData, players: usize, seed: u64, options: MapOptions) -> Case {
        Case { players, seed, options, map: generate_with(data, &setup(data, players, seed), &options) }
    }

    fn at(&self) -> String {
        let o = self.map.features.options;
        format!("seed {}, {} players, {} water {} rivers {} {:?}", self.seed, self.players, o.size.name(), o.water.name(), o.rivers, o.site)
    }
}

/// Every map size x water level x site kind once, player counts cycling
/// through 2-8 and river counts through 1-2, generated once for every test.
fn sample() -> &'static [Case] {
    static SAMPLE: OnceLock<Vec<Case>> = OnceLock::new();
    SAMPLE.get_or_init(|| {
        let data = test_data();
        let mut cases = Vec::new();
        let mut k = 0u64;
        for size in MapSize::ALL {
            for water in WaterLevel::ALL {
                for site in SiteKind::ALL {
                    let players = 2 + (k % 7) as usize;
                    let rivers = 1 + (k / 7 % 2) as u8;
                    cases.push(Case::new(&data, players, 1000 + k, options(size, water, rivers, site)));
                    k += 1;
                }
            }
        }
        cases
    })
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

/// Every 4-connected body of cells passing `kind`: its cells.
fn bodies(map: &GeneratedMap, kind: impl Fn(TerrainId) -> bool) -> Vec<Vec<IVec2>> {
    let (w, h) = (map.width as i32, map.height as i32);
    let mut seen = vec![false; map.terrain.len()];
    let mut out = Vec::new();
    for first in 0..map.terrain.len() {
        if seen[first] || !kind(map.terrain[first]) {
            continue;
        }
        seen[first] = true;
        let mut queue = VecDeque::from([IVec2::new(first as i32 % w, first as i32 / w)]);
        let mut body = Vec::new();
        while let Some(c) = queue.pop_front() {
            body.push(c);
            for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let n = c + d;
                if n.x < 0 || n.y < 0 || n.x >= w || n.y >= h {
                    continue;
                }
                let i = (n.y * w + n.x) as usize;
                if !seen[i] && kind(map.terrain[i]) {
                    seen[i] = true;
                    queue.push_back(n);
                }
            }
        }
        out.push(body);
    }
    out
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

/// Cells a mover of `flag` and `radius` reaches from the clear cells
/// within `seed_radius` of `from` (4-neighbour flood).
fn flood(nav: &mut NavGrid, flag: MoveFlag, radius: f32, from: Vec2, seed_radius: f32) -> Vec<bool> {
    let mobility = Mobility::of(flag).0;
    nav.prepare(mobility, af_geom::NO_TEAM);
    let mut seen = vec![false; (nav.width() * nav.height()) as usize];
    let mut queue = VecDeque::new();
    let c = nav.cell_of(from);
    let r = seed_radius.ceil() as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            let n = c + IVec2::new(dx, dy);
            if nav.in_bounds(n) && nav.cell_center(n).distance(from) <= seed_radius && nav.clear_cell(n, mobility, radius, af_geom::NO_TEAM) {
                seen[nav.index(n)] = true;
                queue.push_back(n);
            }
        }
    }
    while let Some(c) = queue.pop_front() {
        for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let n = c + d;
            if nav.in_bounds(n) && !seen[nav.index(n)] && nav.clear_cell(n, mobility, radius, af_geom::NO_TEAM) {
                seen[nav.index(n)] = true;
                queue.push_back(n);
            }
        }
    }
    seen
}

/// Is any reached cell within `within` of `p`?
fn reached(nav: &NavGrid, seen: &[bool], p: Vec2, within: f32) -> bool {
    let c = nav.cell_of(p);
    let r = within.ceil() as i32;
    (-r..=r).any(|dy| {
        (-r..=r).any(|dx| {
            let n = c + IVec2::new(dx, dy);
            nav.in_bounds(n) && nav.cell_center(n).distance(p) <= within && seen[nav.index(n)]
        })
    })
}

/// Distance from `p` to the nearest map edge that is not ocean.
fn land_edge_distance(f: &MapFeatures, size: f32, p: Vec2) -> f32 {
    let mut d = f32::INFINITY;
    for (e, v) in [(MapEdge::West, p.x), (MapEdge::East, size - p.x), (MapEdge::South, p.y), (MapEdge::North, size - p.y)] {
        if !f.ocean_edges.contains(&e) {
            d = d.min(v);
        }
    }
    d
}

#[test]
fn same_seed_same_map_and_different_seeds_or_options_differ() {
    let data = test_data();
    let a = generate(&data, &setup(&data, 4, 77));
    let b = generate(&data, &setup(&data, 4, 77));
    assert_eq!(a, b);
    let c = generate(&data, &setup(&data, 4, 78));
    assert_ne!(a.terrain, c.terrain);
    assert_ne!(a.starts, c.starts);
    assert_ne!(a.nodes, c.nodes);
    // Explicit options are deterministic too, and change the map.
    let seas = options(MapSize::Large, WaterLevel::Seas, 2, SiteKind::Scar);
    let d = generate_with(&data, &setup(&data, 3, 9), &seas);
    assert_eq!(d, generate_with(&data, &setup(&data, 3, 9), &seas));
    assert_ne!(d.terrain, generate_with(&data, &setup(&data, 3, 9), &MapOptions { site: SiteChoice::Kind(SiteKind::Spire), ..seas }).terrain);
    // The random choices are resolved and published.
    let resolved = resolve_map_options(&MapOptions::default(), 77);
    assert_eq!(a.features.options, resolved);
    assert!((1..=2).contains(&resolved.rivers));
}

#[test]
fn every_player_count_and_faction_gets_a_map() {
    let data = test_data();
    for n in 1..=8 {
        let map = generate(&data, &setup(&data, n, n as u64 * 11));
        // The default options: Medium, lakes (no ocean), so the land is the
        // whole map and the main site is at its centre.
        assert_eq!(map.size, map_size_for(n));
        assert_eq!((map.width, map.height), (map.size as u32, map.size as u32));
        assert_eq!(map.terrain.len(), (map.width * map.height) as usize);
        assert_eq!(map.starts.len(), n);
        assert_eq!(map.crater_center, Vec2::splat(map.size * 0.5));
        assert_eq!((map.crater_center, map.crater_radius), (map.features.sites[0].center, map.features.sites[0].radius));
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
fn sizes_follow_the_table_and_ocean_bands_grow_the_map() {
    let data = test_data();
    for (players, size, side) in [(2, MapSize::Small, 176.0), (4, MapSize::Large, 376.0), (8, MapSize::Huge, 616.0), (3, MapSize::Medium, 256.0)] {
        let o = options(size, WaterLevel::Lakes, 1, SiteKind::Crater);
        let map = generate_with(&data, &setup(&data, players, 3), &o);
        assert_eq!(map.size, side, "{} for {players} players", size.name());
        let f = &map.features;
        assert_eq!((f.land_min, f.land_max), (Vec2::ZERO, Vec2::splat(side)));
        // One ocean band: the map grows by it and the land square is whole.
        let coast = generate_with(&data, &setup(&data, players, 3), &MapOptions { water: WaterLevel::Coast, ..o });
        let band = (side * 0.17).round();
        assert_eq!(coast.size, side + band);
        let f = &coast.features;
        assert_eq!(f.ocean_edges.len(), 1);
        let land = f.land_max - f.land_min;
        assert_eq!(land.min_element(), side);
        assert_eq!(land.max_element(), side + band);
    }
    // MatchSetup::map_size still overrides the land side.
    let mut s = setup(&data, 2, 5);
    s.map_size = 300.4;
    assert_eq!(generate_with(&data, &s, &options(MapSize::Huge, WaterLevel::Dry, 1, SiteKind::Crater)).size, 300.0);
}

#[test]
fn a_map_size_override_plays_and_is_published_as_the_nearest_size() {
    let data = test_data();
    // (players, metres, size option, the size it plays as)
    for (players, metres, size, want) in [(2, 700.0, MapSize::Small, MapSize::Huge), (2, 300.0, MapSize::Huge, MapSize::Large), (4, 288.0, MapSize::Huge, MapSize::Medium), (8, 160.0, MapSize::Large, MapSize::Small)] {
        assert_eq!(size_for_side(metres, players), want, "{metres} m for {players}");
        let mut s = setup(&data, players, 5);
        s.map_size = metres;
        let map = generate_with(&data, &s, &options(size, WaterLevel::Lakes, 1, SiteKind::Crater));
        assert_eq!(map.size, metres);
        assert_eq!(map.features.options.size, want, "{metres} m for {players} with {} asked", size.name());
    }
    // Without an override the size option stands.
    let map = generate_with(&data, &setup(&data, 2, 5), &options(MapSize::Large, WaterLevel::Lakes, 1, SiteKind::Crater));
    assert_eq!(map.features.options.size, MapSize::Large);
    // A big custom land gets the relief of a big map: Huge has at least
    // three ranges' worth of mountain, more than a Small one could hold.
    let mut s = setup(&data, 2, 6);
    s.map_size = 700.0;
    let big = generate_with(&data, &s, &options(MapSize::Small, WaterLevel::Lakes, 1, SiteKind::Crater));
    let mountain = data.terrain_id("mountain").unwrap();
    let rock = big.terrain.iter().filter(|t| **t == mountain).count() as f32 / big.terrain.len() as f32;
    assert!(rock > 0.03, "{:.1}% mountain on a 700 m land", rock * 100.0);
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
    assert!(!empty.nodes.is_empty(), "the meteor site and the wilds are still there");

    let mut crowded = setup(&data, 8, 2);
    crowded.map_size = 160.0;
    for site in SiteKind::ALL {
        let map = generate_with(&data, &crowded, &options(MapSize::Medium, WaterLevel::Seas, 2, site));
        assert_eq!(halls(&map).len(), 8);
        let boxes: Vec<Obb> = map.nodes.iter().map(|n| node_obb(&data, n)).chain(map.buildings.iter().map(|b| building_obb(&data, b))).collect();
        assert!(overlapping_pairs(&boxes).is_empty(), "{site:?}");
    }

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
    let grass = data.terrain_id("grass").unwrap();
    for case in sample() {
        let map = &case.map;
        let nav = map.nav(&data);
        let nodes: Vec<Obb> = map.nodes.iter().map(|n| node_obb(&data, n)).collect();
        for hall in halls(map) {
            let obb = building_obb(&data, &hall);
            assert!(nav.area_buildable(&obb), "hall of player {} not buildable ({})", hall.player, case.at());
            assert!(nodes.iter().all(|n| !n.overlaps(&obb)), "a node sits on the hall ({})", case.at());
            let start = map.starts[hall.player];
            assert_eq!(map.terrain_at(start), Some(grass), "the start is on grass ({})", case.at());
            let clear = START_CLEAR_RADIUS - 0.5;
            let mut p = start - Vec2::splat(clear);
            while p.y <= start.y + clear {
                while p.x <= start.x + clear {
                    if p.distance(start) <= clear {
                        let t = map.terrain_at(p).unwrap();
                        assert_eq!(t, grass, "start disc of {} has {} ({})", hall.player, data.terrain_def(t).id, case.at());
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
fn every_ground_class_reaches_every_start_spot_and_site() {
    let data = test_data();
    let vent = data.node_id("aether_vent").unwrap();
    // The widest unit of each ground class (connect.rs).
    let classes = [(MoveFlag::Foot, 1.0), (MoveFlag::Mounted, 1.0), (MoveFlag::Wheeled, 2.5), (MoveFlag::Tracked, 3.0)];
    let mut carved = 0;
    for case in sample() {
        let map = &case.map;
        assert!(map.cut_off.is_empty(), "the generator reports cut-off places: {:?} ({})", map.cut_off, case.at());
        carved += !map.carved.is_empty() as usize;
        let mut nav = nav_with_obstacles(&data, map);
        let main = map.features.sites[0];
        for (flag, radius) in classes {
            // From the main site's open floor (its spire is in the middle of
            // the Starspire).
            let seen = flood(&mut nav, flag, radius, main.center, main.radius);
            for hall in halls(map) {
                let obb = building_obb(&data, &hall);
                let ahead = (map.crater_center - hall.pos).normalize();
                let front = hall.pos + ahead * (obb.support(ahead) + 2.5);
                assert!(reached(&nav, &seen, front, 4.0), "{flag:?} cannot reach player {} ({})", hall.player, case.at());
            }
            for site in &map.features.sites[1..] {
                assert!(reached(&nav, &seen, site.center, site.radius * 0.9), "{flag:?} cannot reach the site at {} ({})", site.center, case.at());
            }
            // The contested spots: every vent outside the sites.
            let in_site = |p: Vec2| map.features.sites.iter().any(|s| p.distance(s.center) < s.radius);
            for n in map.nodes.iter().filter(|n| n.kind == vent && !in_site(n.pos)) {
                assert!(reached(&nav, &seen, n.pos, 9.0), "{flag:?} cannot reach the spot at {} ({})", n.pos, case.at());
            }
        }
    }
    // Crossings, gaps, passes and ramps are planned wide and open enough:
    // carving is rare (about one map in eighty over the full sweep).
    assert!(carved * 20 <= sample().len(), "{carved} of {} maps needed a route carved", sample().len());
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
        // Toward the main site, a few metres short of it: 45-55 m on the
        // default size, a little closer for the larger sites.
        ("aether_crystal", 3, 42.0, 60.0),
    ];
    for (n, seed) in [(1, 20), (2, 21), (2, 27), (3, 22), (4, 23), (6, 24), (8, 25), (8, 26)] {
        for site in SiteKind::ALL {
            let map = generate_with(&data, &setup(&data, n, seed), &options(MapSize::Medium, WaterLevel::Lakes, 2, site));
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
                assert!(counts.iter().all(|c| *c >= min), "{what} at {from}-{within} m: {counts:?} (seed {seed}, {n} players, {site:?})");
            }
        }
    }
}

#[test]
fn start_resources_are_mirrored_between_players() {
    let data = test_data();
    let wood = data.resource_id("wood").unwrap();
    for case in sample() {
        let map = &case.map;
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
        assert!(signatures.windows(2).all(|w| w[0] == w[1]), "start resources differ between players: {signatures:?} ({})", case.at());
    }
}

#[test]
fn no_boxes_overlap_and_nodes_stand_on_buildable_ground() {
    let data = test_data();
    for case in sample() {
        let map = &case.map;
        let nav = map.nav(&data);
        let mut boxes: Vec<Obb> = map.nodes.iter().map(|nd| node_obb(&data, nd)).collect();
        for (nd, obb) in map.nodes.iter().zip(&boxes) {
            assert!(nav.area_buildable(obb), "{} at {:?} is not on buildable ground ({})", data.node(nd.kind).id, nd.pos, case.at());
        }
        boxes.extend(map.buildings.iter().map(|b| building_obb(&data, b)));
        boxes.extend(map.units.iter().map(|u| unit_obb(&data, u)));
        let pairs = overlapping_pairs(&boxes);
        assert!(pairs.is_empty(), "{} overlapping boxes, e.g. {:?} ({})", pairs.len(), pairs.first().map(|(a, b)| (boxes[*a], boxes[*b])), case.at());
    }
}

#[test]
fn every_site_kind_holds_its_node_budget() {
    let data = test_data();
    let vent = data.node_id("aether_vent").unwrap();
    let meteor = data.node_id("adamant_meteor").unwrap();
    let crystal = data.node_id("aether_crystal").unwrap();
    let scorched = data.terrain_id("scorched").unwrap();
    let mut kinds_seen = Vec::new();
    for case in sample() {
        let map = &case.map;
        let f = &map.features;
        let kind = f.options.site;
        kinds_seen.push(kind);
        assert!(f.sites.iter().all(|s| s.kind == kind));
        let main = f.sites[0];
        // The land square is centred in the land rectangle.
        assert_eq!(main.center, (f.land_min + f.land_max) * 0.5, "the main site is at the land square's centre ({})", case.at());
        let count = |k: NodeTypeId, at: Vec2, r: f32| map.nodes.iter().filter(|nd| nd.kind == k && nd.pos.distance(at) < r).count();
        let in_any = |k: NodeTypeId| map.nodes.iter().filter(|nd| nd.kind == k && f.sites.iter().any(|s| nd.pos.distance(s.center) < s.radius)).count();
        let spots = case.players.max(1);
        let vents_outside = map.nodes.iter().filter(|nd| nd.kind == vent && f.sites.iter().all(|s| nd.pos.distance(s.center) >= s.radius)).count();
        match kind {
            SiteKind::Shattered => {
                // Half the budget in a smaller central crater; the rest fell
                // at the spots, each a small crater with its vent, meteor and
                // veins and 2 crystals.
                assert_eq!(f.sites.len(), 1 + spots, "a crater per spot ({})", case.at());
                assert!((1..=2).contains(&count(vent, main.center, main.radius)), "{}", case.at());
                assert!((1..=2).contains(&count(meteor, main.center, main.radius)), "{}", case.at());
                assert!((4..=6).contains(&count(crystal, main.center, main.radius)), "{}", case.at());
                for s in &f.sites[1..] {
                    assert_eq!((count(vent, s.center, s.radius), count(meteor, s.center, s.radius), count(crystal, s.center, s.radius)), (1, 1, 2), "spot crater at {} ({})", s.center, case.at());
                }
                assert_eq!(vents_outside, 0);
            }
            _ => {
                let n = (in_any(vent), in_any(meteor), in_any(crystal));
                assert!((1..=3).contains(&n.0) && (2..=4).contains(&n.1) && (8..=12).contains(&n.2), "site nodes {n:?} ({})", case.at());
                // One spot between each pair of neighbours (both sides for
                // two players), each with its vent and meteor.
                assert_eq!(vents_outside, spots, "one vent per contested spot ({})", case.at());
            }
        }
        match kind {
            SiteKind::Scar => assert!(f.sites.len() <= 3),
            SiteKind::Crater | SiteKind::Lake | SiteKind::Spire => assert_eq!(f.sites.len(), 1),
            SiteKind::Shattered => {}
        }
        // Site floors are scar.
        let floor = if kind == SiteKind::Spire { main.center + Vec2::X * (main.radius - 1.0) } else { main.center + Vec2::X * 1.5 };
        let floor_terrain = map.terrain_at(floor).unwrap();
        if kind != SiteKind::Lake {
            assert_eq!(floor_terrain, scorched, "{:?} floor is {} ({})", kind, data.terrain_def(floor_terrain).id, case.at());
        }
    }
    for kind in SiteKind::ALL {
        assert!(kinds_seen.contains(&kind));
    }
}

#[test]
fn the_skyfall_scar_is_fair_to_every_start() {
    let data = test_data();
    let kinds = ["aether_vent", "adamant_meteor", "aether_crystal"].map(|id| data.node_id(id).unwrap());
    let mut three = 0;
    for players in 1..=8 {
        for (seed, size) in [(3, MapSize::Medium), (4, MapSize::Large), (5, MapSize::Huge)] {
            let map = generate_with(&data, &setup(&data, players, seed), &options(size, WaterLevel::Lakes, 1, SiteKind::Scar));
            let sites = &map.features.sites;
            let at = format!("{players} players, seed {seed}, {}", size.name());
            if !matches!(players, 1 | 2 | 4) {
                // Side basins would lie by only some starts.
                assert_eq!(sites.len(), 1, "a single basin ({at})");
                continue;
            }
            if sites.len() < 3 {
                continue;
            }
            three += 1;
            // Every start lies as near a side basin as every other...
            let near: Vec<f32> = map.starts.iter().map(|s| sites[1..].iter().map(|b| b.center.distance(*s)).fold(f32::INFINITY, f32::min)).collect();
            let (lo, hi) = near.iter().fold((f32::INFINITY, 0.0f32), |(lo, hi), d| (lo.min(*d), hi.max(*d)));
            assert!(hi - lo < 0.5, "side basins {lo:.1}-{hi:.1} m from the starts ({at})");
            // ...and the two side basins hold the same nodes.
            let nearest = |p: Vec2| (0..3).min_by(|a, b| p.distance(sites[*a].center).total_cmp(&p.distance(sites[*b].center))).unwrap();
            for kind in kinds {
                let count = |k: usize| map.nodes.iter().filter(|n| n.kind == kind && n.pos.distance(sites[k].center) < sites[k].radius && nearest(n.pos) == k).count();
                assert_eq!(count(1), count(2), "{} split between the side basins ({at})", data.node(kind).id);
            }
        }
    }
    assert!(three >= 5, "only {three} three-basin scars in the sample");
}

#[test]
fn rivers_cross_the_land_away_from_the_edges_with_fords_at_most_36_m_apart() {
    let data = test_data();
    let deep = data.terrain_id("deep_water").unwrap();
    for case in sample() {
        let map = &case.map;
        let f = &map.features;
        let requested = if case.options.rivers == RiverCount::One { 1 } else { 2 };
        assert_eq!(f.rivers.len(), requested, "requested rivers ({})", case.at());
        assert_eq!(f.options.rivers as usize, f.rivers.len(), "the rivers laid are published ({})", case.at());
        let side = (f.land_max - f.land_min).min_element();
        for r in &f.rivers {
            assert_eq!(r.points.len(), r.half_widths.len());
            let length = (r.points.len() - 1) as f32 * 0.75;
            assert!(length > side * 0.3, "a river {length} m long on a {side} m land ({})", case.at());
            // Narrower (from 1.2 m) only on the most crowded maps.
            assert!(r.half_widths.iter().all(|h| (1.2..=4.0).contains(h)));
            // Fords: never more than 36 m apart, nor further from either
            // end (a tributary ends in a ford where it joins).
            let mut at: Vec<f32> = r.fords.iter().map(|i| *i as f32 * 0.75).collect();
            at.sort_by(f32::total_cmp);
            at.insert(0, 0.0);
            at.push(length);
            for w in at.windows(2) {
                assert!(w[1] - w[0] <= 36.0 + 0.75, "{} m between fords at {} and {} ({})", w[1] - w[0], w[0], w[1], case.at());
            }
            // Away from its ends the course never hugs a land edge.
            let skip = (25.0 / 0.75) as usize;
            if r.points.len() > 2 * skip {
                let middle = &r.points[skip..r.points.len() - skip];
                let nearest = middle.iter().map(|p| land_edge_distance(f, map.size, *p)).fold(f32::INFINITY, f32::min);
                assert!(nearest > 8.0, "a river comes within {nearest} m of the edge ({})", case.at());
            }
        }
        // Fords split every river into stretches: no deep body along a river
        // runs on for more than ~40 m without one. (Lakes and the crater
        // lake are bigger bodies of their own.)
        let near_river = |c: IVec2| {
            let p = c.as_vec2() + Vec2::splat(0.5);
            f.rivers.iter().any(|r| r.points.iter().step_by(4).any(|q| q.distance(p) < 6.0))
        };
        for body in bodies(map, |t| t == deep) {
            if !near_river(body[0]) {
                continue;
            }
            let (lo, hi) = body.iter().fold((IVec2::MAX, IVec2::MIN), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
            let size = (hi - lo + IVec2::ONE).as_vec2();
            assert!(size.max_element() <= 40.0, "{size} m of deep river without a ford ({})", case.at());
        }
    }
}

#[test]
fn ocean_lies_beyond_the_land_exactly_at_coast_and_seas() {
    let data = test_data();
    let ocean = data.terrain_id("ocean").unwrap();
    for case in sample() {
        let map = &case.map;
        let f = &map.features;
        let edges = f.options.water.ocean_edges();
        assert_eq!(f.ocean_edges.len(), edges, "{}", case.at());
        let sea: Vec<Vec2> = cells(map).filter(|(_, t)| *t == ocean).map(|(p, _)| p).collect();
        assert_eq!(!sea.is_empty(), edges > 0, "ocean present exactly with ocean edges ({})", case.at());
        // Never inside the land rectangle (coasts only wobble outward).
        assert!(sea.iter().all(|p| !(p.cmpge(f.land_min).all() && p.cmple(f.land_max).all())), "ocean on the land ({})", case.at());
        // Every ocean edge is ocean along its middle.
        for e in &f.ocean_edges {
            let mid = map.size * 0.5;
            let p = match e {
                MapEdge::West => Vec2::new(0.5, mid),
                MapEdge::East => Vec2::new(map.size - 0.5, mid),
                MapEdge::South => Vec2::new(mid, 0.5),
                MapEdge::North => Vec2::new(mid, map.size - 0.5),
            };
            assert_eq!(map.terrain_at(p), Some(ocean), "{e:?} edge ({})", case.at());
        }
    }
}

#[test]
fn every_map_has_its_terrain_features_in_the_wilds() {
    let data = test_data();
    let terrain = |id: &str| data.terrain_id(id).unwrap();
    let (palm, silicon) = (data.node_id("palm").unwrap(), data.node_id("silicon_deposit").unwrap());
    for case in sample() {
        let map = &case.map;
        let f = &map.features;
        // Outside the start areas (whose quartz patches are sand) and the
        // sites (rims are cliff, their surroundings marsh and thicket).
        let wild = |p: Vec2| f.sites.iter().all(|s| p.distance(s.center) > s.radius + 5.0) && map.starts.iter().all(|s| p.distance(*s) > START_AREA_RADIUS + 1.0);
        let count = |id: &str| cells(map).filter(|(_, t)| *t == terrain(id)).count();
        let wild_count = |id: &str| cells(map).filter(|(p, t)| *t == terrain(id) && wild(*p)).count();
        let wild_nodes = |k: NodeTypeId| map.nodes.iter().filter(|nd| nd.kind == k && wild(nd.pos)).count();
        let at = case.at();
        assert!(wild_count("sand") > 60, "a dune region ({at})");
        assert!(wild_nodes(palm) > 0 && wild_nodes(silicon) > 0, "palms and quartz on the dunes ({at})");
        assert!(wild_count("cliff") > 30, "a ridge or highland ({at})");
        assert!(count("deep_water") > 0 && count("ford") > 0, "rivers with fords ({at})");
        assert!(count("marsh") > 0 && count("thicket") > 0, "marsh and thicket ({at})");
        if f.options.size != MapSize::Small {
            assert!(count("mountain") > 0 && !f.mountains.is_empty(), "mountains on Medium and larger ({at})");
        }
        let land = cells(map).filter(|(p, _)| p.cmpge(f.land_min).all() && p.cmple(f.land_max).all()).count();
        let grass = cells(map).filter(|(p, t)| *t == terrain("grass") && p.cmpge(f.land_min).all() && p.cmple(f.land_max).all()).count();
        assert!(grass * 2 > land, "mostly grass on the land ({at})");
    }
}

/// Approximate distance (3-4 chamfer, in cells) from every cell to the
/// nearest cell where `mask` is true.
fn chamfer(mask: &[bool], w: usize, h: usize) -> Vec<f32> {
    let mut d: Vec<f32> = mask.iter().map(|m| if *m { 0.0 } else { f32::INFINITY }).collect();
    let (a, b) = (1.0, std::f32::consts::SQRT_2);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + a);
            }
            if y > 0 {
                v = v.min(d[i - w] + a);
                if x > 0 {
                    v = v.min(d[i - w - 1] + b);
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1] + b);
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1] + a);
            }
            if y + 1 < h {
                v = v.min(d[i + w] + a);
                if x + 1 < w {
                    v = v.min(d[i + w + 1] + b);
                }
                if x > 0 {
                    v = v.min(d[i + w - 1] + b);
                }
            }
            d[i] = v;
        }
    }
    d
}

#[test]
fn mountains_are_broad_ranges_that_keep_the_play_open() {
    let data = test_data();
    let (mountain, marsh, thicket) = (data.terrain_id("mountain").unwrap(), data.terrain_id("marsh").unwrap(), data.terrain_id("thicket").unwrap());
    let vent = data.node_id("aether_vent").unwrap();
    let wood = data.resource_id("wood").unwrap();
    let mut shares: Vec<(MapSize, f32)> = Vec::new();
    for case in sample() {
        let map = &case.map;
        let f = &map.features;
        let at = case.at();
        let w = map.width as usize;
        let on_land = |p: Vec2| p.cmpge(f.land_min).all() && p.cmple(f.land_max).all();
        let land = cells(map).filter(|(p, _)| on_land(*p)).count();
        let rock = cells(map).filter(|(p, t)| *t == mountain && on_land(*p)).count();
        let share = rock as f32 * 100.0 / land as f32;
        shares.push((f.options.size, share));
        if f.options.size != MapSize::Small {
            assert!((1.0..=14.0).contains(&share), "{share:.1}% mountain ({at})");
            // Broad enough somewhere for the renderer's real peaks.
            assert!(f.mountains.iter().any(|m| m.spine.len() > 1 && m.half_widths.iter().any(|h| *h >= 9.0)), "no broad range ({at})");
        }
        // The contested spots (at the Shattered Fall, its spot craters) and
        // the main site, which keep their lanes open.
        let main = f.sites[0];
        let spots: Vec<Vec2> = if f.options.site == SiteKind::Shattered {
            f.sites[1..].iter().map(|s| s.center).collect()
        } else {
            map.nodes.iter().filter(|n| n.kind == vent && f.sites.iter().all(|s| n.pos.distance(s.center) >= s.radius)).map(|n| n.pos).collect()
        };
        let mut lanes: Vec<(Vec2, Vec2)> = Vec::new();
        for s in &spots {
            lanes.push((*s, main.center));
            let mut near: Vec<Vec2> = map.starts.clone();
            near.sort_by(|a, b| a.distance(*s).total_cmp(&b.distance(*s)));
            lanes.extend(near.iter().take(2).map(|st| (*st, *s)));
        }
        for (p, t) in cells(map) {
            if t != mountain || (f.options.site == SiteKind::Spire && p.distance(main.center) < main.radius) {
                continue;
            }
            for s in &map.starts {
                assert!(p.distance(*s) > START_AREA_RADIUS, "mountain at {p} in the start area of {s} ({at})");
                assert!(af_geom::point_segment_distance(p, *s, main.center) > CORRIDOR_HALF_WIDTH + 1.0, "mountain at {p} in a corridor ({at})");
            }
            assert!(spots.iter().all(|s| p.distance(*s) > 20.0), "mountain at {p} by a contested spot ({at})");
            // Lanes are kept 6 m either side (the spot found by its vent,
            // which may sit a little off).
            assert!(lanes.iter().all(|(a, b)| af_geom::point_segment_distance(p, *a, *b) > 3.5), "mountain at {p} across a lane ({at})");
        }
        // Woods, thicket and marsh keep off the mountains' feet, so they
        // never close what the mountains leave open.
        let near_rock = chamfer(&map.terrain.iter().map(|t| *t == mountain).collect::<Vec<_>>(), w, map.height as usize);
        let rock_at = |p: Vec2| near_rock[p.y as usize * w + p.x as usize];
        for (p, t) in cells(map) {
            if t == marsh || t == thicket {
                assert!(rock_at(p) >= 8.0, "{} at {p}, {:.1} m from a mountain ({at})", data.terrain_def(t).id, rock_at(p));
            }
        }
        for n in map.nodes.iter().filter(|n| data.node(n.kind).resource_id == wood && n.kind != data.node_id("palm").unwrap()) {
            assert!(rock_at(n.pos) >= 7.5, "a tree at {} by a mountain ({at})", n.pos);
        }
    }
    // A sensible amount of mountain: a few percent of the land on Small,
    // about 4-8 % on Medium and larger.
    for size in MapSize::ALL {
        let of: Vec<f32> = shares.iter().filter(|(s, _)| *s == size).map(|(_, v)| *v).collect();
        let mean = of.iter().sum::<f32>() / of.len() as f32;
        let band = if size == MapSize::Small { 0.5..=4.5 } else { 3.5..=9.0 };
        assert!(band.contains(&mean), "{} maps average {mean:.1}% mountain", size.name());
    }
}

#[test]
fn mountains_are_impassable_to_every_ground_unit() {
    let data = test_data();
    let mountain = data.terrain_id("mountain").unwrap();
    for flag in [MoveFlag::Foot, MoveFlag::Mounted, MoveFlag::Wheeled, MoveFlag::Tracked, MoveFlag::Hover] {
        assert!(!data.terrain_passable(mountain, Mobility::of(flag)), "{flag:?}");
    }
    let ironhold = data.faction_id("ironhold").unwrap();
    let climber = data.unit_ids().find(|u| data.unit(*u).locomotion == MoveFlag::Foot).map(|u| data.unit_mobility(u, ironhold)).unwrap();
    assert!(!data.terrain_passable(mountain, climber), "not even climbers");
    assert!(data.terrain_passable(mountain, Mobility::of(MoveFlag::Flying)));
    let ford = data.terrain_id("ford").unwrap();
    assert!(data.terrain_passable(ford, Mobility::of(MoveFlag::Wheeled)), "wheeled machines cross at fords");
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

#[test]
fn a_huge_map_for_eight_generates_in_a_few_seconds() {
    let data = test_data();
    let s = setup(&data, 8, 98);
    let o = options(MapSize::Huge, WaterLevel::Seas, 2, SiteKind::Scar);
    let started = Instant::now();
    let map = generate_with(&data, &s, &o);
    let took = started.elapsed();
    assert!(map.size > 616.0);
    assert!(took.as_secs_f32() < 4.0, "took {took:?}");
    println!("{} m, 8 players, seas: {took:?}, {} nodes", map.size, map.nodes.len());
}

/// Lists every problem over a wide sample instead of stopping at the first
/// (a tuning aid).
#[test]
#[ignore = "diagnostic: cargo test -p af_map --test generate diagnose -- --ignored --nocapture"]
fn diagnose() {
    let data = test_data();
    let (vent, meteor, crystal) = (data.node_id("aether_vent").unwrap(), data.node_id("adamant_meteor").unwrap(), data.node_id("aether_crystal").unwrap());
    let terrain = |id: &str| data.terrain_id(id).unwrap();
    let mut problems = 0;
    for players in 2..=8 {
        for size in MapSize::ALL {
            for water in WaterLevel::ALL {
                for site in SiteKind::ALL {
                    let rivers = 1 + (players as u8 + water.level()) % 2;
                    let case = Case::new(&data, players, 7 + players as u64 * 31 + water.level() as u64, options(size, water, rivers, site));
                    let map = &case.map;
                    let f = &map.features;
                    let mut notes = Vec::new();
                    if f.rivers.len() != rivers as usize {
                        notes.push(format!("rivers {}/{rivers}", f.rivers.len()));
                    }
                    if !map.cut_off.is_empty() {
                        notes.push(format!("cut off {:?}", map.cut_off));
                    }
                    let main = f.sites[0];
                    let sites = if site == SiteKind::Shattered { 1 } else { f.sites.len() };
                    let count = |k: NodeTypeId| map.nodes.iter().filter(|nd| nd.kind == k && f.sites[..sites].iter().any(|s| nd.pos.distance(s.center) < s.radius)).count();
                    let n = (count(vent), count(meteor), count(crystal));
                    let ok = if site == SiteKind::Shattered {
                        (1..=2).contains(&n.0) && (1..=2).contains(&n.1) && (4..=6).contains(&n.2)
                    } else {
                        (1..=3).contains(&n.0) && (2..=4).contains(&n.1) && (8..=12).contains(&n.2)
                    };
                    if !ok {
                        notes.push(format!("site nodes {n:?} (r {})", main.radius));
                    }
                    let cliff = cells(map).filter(|(_, t)| *t == terrain("cliff")).count();
                    if cliff < 40 {
                        notes.push(format!("cliff {cliff}"));
                    }
                    if size != MapSize::Small && f.mountains.is_empty() {
                        notes.push("no mountains".to_string());
                    }
                    if !notes.is_empty() {
                        problems += 1;
                        println!("{}: {}", case.at(), notes.join("; "));
                    }
                }
            }
        }
    }
    println!("{problems} maps with problems");
}

/// Every player count, size, water level, river count and site kind, three
/// seeds each: everything reachable for every class, the rivers asked for,
/// clear starts.
#[test]
#[ignore = "long: cargo test -p af_map --test generate -- --ignored --nocapture"]
fn sweep_every_option() {
    let data = test_data();
    let grass = data.terrain_id("grass").unwrap();
    let started = Instant::now();
    // One thread per player count; every problem is listed before failing.
    let results: Vec<(usize, usize, Vec<String>, Vec<String>)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (2..=8usize)
            .map(|players| {
                let data = &data;
                scope.spawn(move || {
                    let (mut maps, mut carved, mut problems, mut why) = (0, 0, Vec::new(), Vec::new());
                    for size in MapSize::ALL {
                        for water in WaterLevel::ALL {
                            for rivers in [1, 2] {
                                for site in SiteKind::ALL {
                                    for seed in 0..3 {
                                        let case = Case::new(data, players, seed * 7919 + players as u64, options(size, water, rivers, site));
                                        let map = &case.map;
                                        maps += 1;
                                        carved += !map.carved.is_empty() as usize;
                                        // "Tracked: spot 3 at (..)" -> "Tracked: spot".
                                        why.extend(map.carved.iter().map(|c| c.split(|ch: char| ch.is_ascii_digit() || ch == '(').next().unwrap_or(c).trim().to_string()));
                                        if !map.cut_off.is_empty() {
                                            problems.push(format!("cut off {:?} ({})", map.cut_off, case.at()));
                                        }
                                        if map.features.rivers.len() != rivers as usize {
                                            problems.push(format!("{} river(s) ({})", map.features.rivers.len(), case.at()));
                                        }
                                        if !map.starts.iter().all(|s| map.terrain_at(*s) == Some(grass)) {
                                            problems.push(format!("a start off grass ({})", case.at()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    (maps, carved, problems, why)
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().expect("sweep thread")).collect()
    });
    let maps: usize = results.iter().map(|r| r.0).sum();
    let carved: usize = results.iter().map(|r| r.1).sum();
    let problems: Vec<&String> = results.iter().flat_map(|r| &r.2).collect();
    for p in &problems {
        println!("{p}");
    }
    let mut why: Vec<&String> = results.iter().flat_map(|r| &r.3).collect();
    why.sort();
    for kind in why.iter().copied().collect::<std::collections::BTreeSet<_>>() {
        println!("carved {:>4} x {kind}", why.iter().filter(|w| **w == kind).count());
    }
    println!("{maps} maps in {:?}; {carved} needed a carve; {} problems", started.elapsed(), problems.len());
    assert!(problems.is_empty());
}
