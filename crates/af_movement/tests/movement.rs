//! Headless movement scenarios against the real content.

use std::f32::consts::FRAC_PI_2;
use std::time::{Duration, Instant};

use af_core::prelude::*;
use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};
use af_movement::MovementPlugin;
use bevy::prelude::*;

const TICKS_PER_SECOND: u32 = 30;

// ---------------------------------------------------------------------------
// Content lookups (by tags and fields, not ids)
// ---------------------------------------------------------------------------

/// The smallest common unit with this locomotion.
fn unit_with(data: &GameData, loco: MoveFlag) -> UnitTypeId {
    data.unit_ids()
        .filter(|u| data.unit(*u).locomotion == loco && !data.unit(*u).has_tag(Tag::Giant))
        .min_by(|a, b| data.unit(*a).size[0].total_cmp(&data.unit(*b).size[0]))
        .unwrap_or_else(|| panic!("no {loco:?} unit"))
}

/// The narrowest common Foot unit at least `width` wide (metres).
fn foot_at_least(data: &GameData, width: f32) -> UnitTypeId {
    data.unit_ids()
        .filter(|u| data.unit(*u).locomotion == MoveFlag::Foot && !data.unit(*u).has_tag(Tag::Giant) && data.unit(*u).size[0] >= width)
        .min_by(|a, b| data.unit(*a).size[0].total_cmp(&data.unit(*b).size[0]))
        .unwrap_or_else(|| panic!("no Foot unit {width} m wide"))
}

/// A faction whose Foot units get `flag`.
fn faction_granting(data: &GameData, flag: MoveFlag) -> String {
    data.factions
        .iter()
        .find(|f| f.traits.iter().any(|t| t.flag == flag && t.applies_to.contains(&MoveFlag::Foot)))
        .map(|f| f.id.clone())
        .unwrap_or_else(|| panic!("no faction grants {flag:?} to Foot"))
}

/// A faction whose Foot units get none of `flags`.
fn faction_without(data: &GameData, flags: &[MoveFlag]) -> String {
    data.factions
        .iter()
        .find(|f| !f.traits.iter().any(|t| flags.contains(&t.flag) && t.applies_to.contains(&MoveFlag::Foot)))
        .map(|f| f.id.clone())
        .expect("a faction without those Foot traits")
}

/// A plain building (no wall/gate/node requirement) whose footprint side is in `range`.
fn building_sized(data: &GameData, range: std::ops::RangeInclusive<f32>) -> BuildingTypeId {
    data.building_ids()
        .find(|b| {
            let d = data.building(*b);
            !d.wall && !d.gate && d.requires_node.is_none() && d.resource_field.is_none() && range.contains(&d.size[0]) && range.contains(&d.size[2])
        })
        .expect("a building of that size")
}

fn gate_building(data: &GameData) -> BuildingTypeId {
    data.building_ids().find(|b| data.building(*b).gate).expect("a gate building")
}

// ---------------------------------------------------------------------------
// World helpers
// ---------------------------------------------------------------------------

fn world(size: f32, players: &[(&str, u8)]) -> (App, Vec<PlayerId>) {
    let mut app = sim_app(test_data());
    app.add_plugins(MovementPlugin);
    let ids = flat_world(&mut app, size, players);
    (app, ids)
}

fn spawn(app: &mut App, player: PlayerId, kind: UnitTypeId, pos: Vec2) -> Entity {
    with_commands(app, |c, data, players| spawn_unit(c, data, players.get(player), kind, pos, 0.0))
}

fn spawn_building_at(app: &mut App, player: PlayerId, kind: BuildingTypeId, pos: Vec2, yaw: f32, complete: bool) -> Entity {
    with_commands(app, |c, data, players| spawn_building(c, data, players.get(player), kind, pos, yaw, complete))
}

fn order(app: &mut App, e: Entity, goal: MoveGoal) {
    app.world_mut().entity_mut(e).insert(goal);
}

fn pos(app: &App, e: Entity) -> Vec2 {
    app.world().get::<Position>(e).expect("position").0
}

fn status(app: &App, e: Entity) -> PathStatus {
    app.world().get::<PathState>(e).expect("path state").status
}

fn obb(app: &App, e: Entity) -> Obb {
    let w = app.world();
    obb_of(w.get::<Position>(e).unwrap(), w.get::<Facing>(e).unwrap(), w.get::<Footprint>(e).unwrap())
}

/// How deep two boxes overlap (0 when apart or touching).
fn penetration(a: &Obb, b: &Obb) -> f32 {
    a.mtv(b).map_or(0.0, |m| m.length())
}

/// Paint terrain `id` over the cells x0..x1, y0..y1 (exclusive ends).
fn paint(app: &mut App, id: &str, x: std::ops::Range<i32>, y: std::ops::Range<i32>) {
    let t = app.world().resource::<Data>().terrain_id(id).expect("terrain id");
    let mut nav = app.world_mut().resource_mut::<Nav>();
    for cy in y {
        for cx in x.clone() {
            nav.set_terrain(IVec2::new(cx, cy), t.0);
        }
    }
}

fn terrain_name(app: &App, p: Vec2) -> String {
    let w = app.world();
    let nav = w.resource::<Nav>();
    let data = w.resource::<Data>();
    data.terrain_def(TerrainId(nav.terrain_at(nav.cell_of(p)))).id.clone()
}

/// Tick until `done` holds (checking `each` every tick) or `seconds` pass.
fn run_until(app: &mut App, seconds: u32, mut each: impl FnMut(&App), mut done: impl FnMut(&App) -> bool) -> bool {
    for _ in 0..seconds * TICKS_PER_SECOND {
        run_ticks(app, 1);
        each(app);
        if done(app) {
            return true;
        }
    }
    false
}

fn all_units(app: &mut App) -> Vec<(Entity, Obb)> {
    let w = app.world_mut();
    let mut q = w.query_filtered::<(Entity, &Position, &Facing, &Footprint), With<Unit>>();
    q.iter(w).map(|(e, p, f, fp)| (e, obb_of(p, f, fp))).collect()
}

fn max_pair_penetration(units: &[(Entity, Obb)]) -> f32 {
    let mut worst = 0.0f32;
    for i in 0..units.len() {
        for j in i + 1..units.len() {
            worst = worst.max(penetration(&units[i].1, &units[j].1));
        }
    }
    worst
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

#[test]
fn walks_around_a_building_to_its_goal() {
    let (mut app, p) = world(64.0, &[("ironhold", 0)]);
    let data = test_data();
    let unit = spawn(&mut app, p[0], unit_with(&data, MoveFlag::Foot), Vec2::new(14.0, 32.0));
    let hall = spawn_building_at(&mut app, p[0], building_sized(&data, 8.0..=10.0), Vec2::new(32.0, 32.0), 0.3, true);
    let goal = Vec2::new(50.0, 32.0);
    order(&mut app, unit, MoveGoal::point(goal, 0.3));
    let mut worst = 0.0f32;
    let arrived = run_until(
        &mut app,
        40,
        |app| worst = worst.max(penetration(&obb(app, unit), &obb(app, hall))),
        |app| status(app, unit) == PathStatus::Arrived,
    );
    assert!(arrived, "status {:?} at {}", status(&app, unit), pos(&app, unit));
    assert!(pos(&app, unit).distance(goal) <= 0.3 + 1e-3);
    assert!(worst < 0.01, "walked into the building by {worst}");
    assert_eq!(app.world().get::<Velocity>(unit).unwrap().0, Vec2::ZERO, "stopped on arrival");
}

#[test]
fn two_units_swapping_places_both_arrive_without_overlapping() {
    let (mut app, p) = world(48.0, &[("ironhold", 0)]);
    let foot = unit_with(&test_data(), MoveFlag::Foot);
    let (a0, b0) = (Vec2::new(16.0, 24.0), Vec2::new(30.0, 24.0));
    let a = spawn(&mut app, p[0], foot, a0);
    let b = spawn(&mut app, p[0], foot, b0);
    order(&mut app, a, MoveGoal::point(b0, 0.2));
    order(&mut app, b, MoveGoal::point(a0, 0.2));
    let mut worst = 0.0f32;
    let done = run_until(
        &mut app,
        30,
        |app| worst = worst.max(penetration(&obb(app, a), &obb(app, b))),
        |app| status(app, a) == PathStatus::Arrived && status(app, b) == PathStatus::Arrived,
    );
    assert!(done, "a {:?} at {}, b {:?} at {}", status(&app, a), pos(&app, a), status(&app, b), pos(&app, b));
    assert!(worst < 0.01, "overlapped by {worst}");
}

#[test]
fn fifty_units_to_one_point_settle_without_overlap() {
    let (mut app, p) = world(96.0, &[("ironhold", 0)]);
    let foot = unit_with(&test_data(), MoveFlag::Foot);
    let mut units = Vec::new();
    for i in 0..50 {
        let at = Vec2::new(12.0 + (i % 10) as f32 * 1.5, 12.0 + (i / 10) as f32 * 1.5);
        units.push(spawn(&mut app, p[0], foot, at));
    }
    let goal = Vec2::new(70.0, 60.0);
    for &u in &units {
        order(&mut app, u, MoveGoal::point(goal, 0.5));
    }
    let (mut partial, mut jump) = (false, 0.0f32);
    let mut last: Vec<Vec2> = units.iter().map(|u| pos(&app, *u)).collect();
    let settled = run_until(
        &mut app,
        90,
        |app| {
            partial |= units.iter().any(|u| app.world().get::<PathState>(*u).is_some_and(|p| p.partial));
            for (u, prev) in units.iter().zip(&mut last) {
                jump = jump.max(pos(app, *u).distance(*prev));
                *prev = pos(app, *u);
            }
        },
        |app| units.iter().all(|u| status(app, *u) == PathStatus::Arrived),
    );
    let statuses: Vec<PathStatus> = units.iter().map(|u| status(&app, *u)).collect();
    assert!(settled, "not everyone arrived: {statuses:?}");
    assert!(!partial, "every path led to the goal");
    // Walking speed is ~0.15 m per tick; pushes in the crowd add little.
    assert!(jump < 0.5, "a unit was thrown {jump} m in one tick");
    // Let the last pushes settle, then nobody overlaps and nobody drifts.
    run_ticks(&mut app, 30);
    let all = all_units(&mut app);
    let worst = max_pair_penetration(&all);
    assert!(worst <= 0.05, "pair overlapping by {worst}");
    for &u in &units {
        assert!(pos(&app, u).distance(goal) < 8.0, "{u} ended far away at {} ({:?})", pos(&app, u), status(&app, u));
        assert!(app.world().get::<Velocity>(u).unwrap().0.length() < 0.05, "{u} still moving");
    }
}

#[test]
fn deep_water_admits_amphibious_foot_and_hover_only() {
    let data = test_data();
    let plain = faction_without(&data, &[MoveFlag::Amphibious]);
    let amphibious = faction_granting(&data, MoveFlag::Amphibious);
    let (mut app, p) = world(64.0, &[(&plain, 0), (&amphibious, 1)]);
    paint(&mut app, "deep_water", 28..34, 0..64);
    let foot = unit_with(&data, MoveFlag::Foot);
    let walker = spawn(&mut app, p[0], foot, Vec2::new(10.0, 16.0));
    let wader = spawn(&mut app, p[1], foot, Vec2::new(10.0, 32.0));
    let hover = spawn(&mut app, p[0], unit_with(&data, MoveFlag::Hover), Vec2::new(10.0, 48.0));
    order(&mut app, walker, MoveGoal::point(Vec2::new(54.0, 16.0), 0.5));
    order(&mut app, wader, MoveGoal::point(Vec2::new(54.0, 32.0), 0.5));
    order(&mut app, hover, MoveGoal::point(Vec2::new(54.0, 48.0), 0.5));
    let mut walker_wet = false;
    run_until(
        &mut app,
        40,
        |app| walker_wet |= terrain_name(app, pos(app, walker)) == "deep_water",
        |app| status(app, wader) == PathStatus::Arrived && status(app, hover) == PathStatus::Arrived && status(app, walker) == PathStatus::Failed,
    );
    assert_eq!(status(&app, wader), PathStatus::Arrived, "amphibious foot crosses");
    assert_eq!(status(&app, hover), PathStatus::Arrived, "hover crosses");
    assert_eq!(status(&app, walker), PathStatus::Failed, "plain foot cannot cross");
    assert!(!walker_wet && pos(&app, walker).x < 28.0, "plain foot entered deep water: {}", pos(&app, walker));
}

#[test]
fn climbers_cross_cliffs_while_others_path_around() {
    let data = test_data();
    let climbing = faction_granting(&data, MoveFlag::Climber);
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let (mut app, p) = world(64.0, &[(&climbing, 0), (&plain, 1)]);
    // A cliff band with a gap at the top (y >= 54).
    paint(&mut app, "cliff", 30..34, 0..54);
    let foot = unit_with(&data, MoveFlag::Foot);
    let climber = spawn(&mut app, p[0], foot, Vec2::new(10.0, 12.0));
    let walker = spawn(&mut app, p[1], foot, Vec2::new(10.0, 20.0));
    order(&mut app, climber, MoveGoal::point(Vec2::new(54.0, 12.0), 0.5));
    order(&mut app, walker, MoveGoal::point(Vec2::new(54.0, 20.0), 0.5));
    let (mut climber_dev, mut walker_top, mut walker_on_cliff) = (0.0f32, 0.0f32, false);
    let done = run_until(
        &mut app,
        90,
        |app| {
            climber_dev = climber_dev.max((pos(app, climber).y - 12.0).abs());
            walker_top = walker_top.max(pos(app, walker).y);
            walker_on_cliff |= terrain_name(app, pos(app, walker)) == "cliff";
        },
        |app| status(app, climber) == PathStatus::Arrived && status(app, walker) == PathStatus::Arrived,
    );
    assert!(done, "climber {:?} at {}, walker {:?} at {}", status(&app, climber), pos(&app, climber), status(&app, walker), pos(&app, walker));
    assert!(climber_dev < 1.0, "the climber went straight over the cliff (strayed {climber_dev})");
    assert!(walker_top > 53.0, "the walker used the gap (highest y {walker_top})");
    assert!(!walker_on_cliff, "the walker never stood on the cliff");
}

#[test]
fn gate_lets_its_team_through_but_not_enemies() {
    let data = test_data();
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let (mut app, p) = world(40.0, &[(&plain, 0), (&plain, 1)]);
    // A cliff wall at x = 19..21 with a gate filling the gap at y = 17..23.
    paint(&mut app, "cliff", 19..21, 0..17);
    paint(&mut app, "cliff", 19..21, 23..40);
    spawn_building_at(&mut app, p[0], gate_building(&data), Vec2::new(20.0, 20.0), -FRAC_PI_2, true);
    let foot = unit_with(&data, MoveFlag::Foot);
    let friend = spawn(&mut app, p[0], foot, Vec2::new(8.0, 18.0));
    let enemy = spawn(&mut app, p[1], foot, Vec2::new(8.0, 22.0));
    order(&mut app, friend, MoveGoal::point(Vec2::new(32.0, 18.0), 0.4));
    order(&mut app, enemy, MoveGoal::point(Vec2::new(32.0, 22.0), 0.4));
    let mut enemy_max_x = 0.0f32;
    run_until(
        &mut app,
        30,
        |app| enemy_max_x = enemy_max_x.max(pos(app, enemy).x),
        |app| status(app, friend) == PathStatus::Arrived && status(app, enemy) == PathStatus::Failed,
    );
    assert_eq!(status(&app, friend), PathStatus::Arrived, "owner's team passes at {}", pos(&app, friend));
    assert_eq!(status(&app, enemy), PathStatus::Failed);
    assert!(enemy_max_x < 19.4 - 0.3, "enemy got into the gate: x {enemy_max_x}");
}

#[test]
fn a_unit_inside_a_new_foundation_is_ejected() {
    let (mut app, p) = world(40.0, &[("ironhold", 0)]);
    let data = test_data();
    let unit = spawn(&mut app, p[0], unit_with(&data, MoveFlag::Foot), Vec2::new(20.0, 20.0));
    run_ticks(&mut app, 2);
    let site = spawn_building_at(&mut app, p[0], building_sized(&data, 3.5..=5.0), Vec2::new(20.6, 20.3), 0.2, false);
    assert!(app.world().get::<UnderConstruction>(site).is_some());
    assert!(penetration(&obb(&app, unit), &obb(&app, site)) > 0.5, "starts inside");
    run_ticks(&mut app, 3);
    assert!(penetration(&obb(&app, unit), &obb(&app, site)) < 0.01, "still inside the foundation at {}", pos(&app, unit));
}

#[test]
fn flyers_fly_straight_over_cliffs() {
    let (mut app, p) = world(64.0, &[("ironhold", 0)]);
    paint(&mut app, "cliff", 20..44, 0..64);
    let flyer = spawn(&mut app, p[0], unit_with(&test_data(), MoveFlag::Flying), Vec2::new(8.0, 30.0));
    run_ticks(&mut app, 1);
    // Face the goal so the straight line is exact from the first step.
    app.world_mut().get_mut::<Facing>(flyer).unwrap().0 = FRAC_PI_2;
    let goal = Vec2::new(56.0, 30.0);
    order(&mut app, flyer, MoveGoal::point(goal, 0.3));
    let mut stray = 0.0f32;
    let arrived = run_until(&mut app, 30, |app| stray = stray.max((pos(app, flyer).y - 30.0).abs()), |app| status(app, flyer) == PathStatus::Arrived);
    assert!(arrived, "{:?} at {}", status(&app, flyer), pos(&app, flyer));
    assert!(stray < 1e-3, "left the straight line by {stray}");
    assert_eq!(app.world().get::<PathState>(flyer).unwrap().waypoints, vec![goal]);
}

#[test]
fn despawned_and_changed_obstacles_update_the_grid() {
    let (mut app, p) = world(40.0, &[("ironhold", 0)]);
    let data = test_data();
    let kind = building_sized(&data, 3.5..=5.0);
    let b = spawn_building_at(&mut app, p[0], kind, Vec2::new(20.0, 20.0), 0.0, true);
    let foot = Mobility::of(MoveFlag::Foot).0;
    let blocked = |app: &App, c: IVec2| !app.world().resource::<Nav>().passable(c, foot, 0);
    run_ticks(&mut app, 1);
    assert!(blocked(&app, IVec2::new(20, 20)), "registered on the first tick");

    // A building upgrade swaps the footprint (here: moved and rotated).
    let moved = Obb::new(Vec2::new(30.0, 30.0), app.world().get::<Footprint>(b).unwrap().half, 0.5);
    app.world_mut().get_mut::<Obstacle>(b).unwrap().obb = moved;
    run_ticks(&mut app, 1);
    assert!(!blocked(&app, IVec2::new(20, 20)), "old footprint released");
    assert!(blocked(&app, IVec2::new(30, 30)), "new footprint registered");

    app.world_mut().entity_mut(b).despawn();
    run_ticks(&mut app, 1);
    assert!(!blocked(&app, IVec2::new(30, 30)), "despawn releases the cells");
    let nav = app.world().resource::<Nav>();
    assert!((0..40).all(|y| (0..40).all(|x| nav.passable(IVec2::new(x, y), foot, 0))), "nothing left behind");
}

#[test]
fn gatherers_reach_a_node_by_box_gap() {
    let (mut app, p) = world(48.0, &[("ironhold", 0)]);
    let data = test_data();
    let tree_kind = data.node_ids().find(|n| data.node(*n).blocking && data.node(*n).size[0] <= 1.0).expect("a small blocking node");
    let tree = with_commands(&mut app, |c, data, _| spawn_node(c, data, tree_kind, Vec2::new(34.3, 24.6), 0.4, None));
    let worker = spawn(&mut app, p[0], unit_with(&data, MoveFlag::Foot), Vec2::new(10.0, 22.0));
    order(&mut app, worker, MoveGoal::entity(tree, Vec2::new(34.3, 24.6), 0.3));
    let arrived = run_until(&mut app, 30, |_| {}, |app| status(app, worker) == PathStatus::Arrived);
    assert!(arrived, "{:?} at {}", status(&app, worker), pos(&app, worker));
    let gap = obb(&app, worker).gap(&obb(&app, tree));
    assert!(gap <= 0.3 + 1e-3, "arrived {gap} m from the node");
    assert!(penetration(&obb(&app, worker), &obb(&app, tree)) < 0.01);
}

/// The open cell facing the worker is a pocket inside a ring of trees: the
/// worker goes round to the node's open side instead of giving up.
#[test]
fn gatherers_reach_a_node_beside_a_pocket() {
    let (mut app, p) = world(64.0, &[("ironhold", 0)]);
    let data = test_data();
    let small = data.node_ids().filter(|n| data.node(*n).blocking).min_by(|a, b| data.node(*a).size[0].total_cmp(&data.node(*b).size[0])).expect("a tree");
    let tree_at = |app: &mut App, x: f32, y: f32| with_commands(app, |c, data, _| spawn_node(c, data, small, Vec2::new(x, y), 0.0, None));
    for (dx, dy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
        tree_at(&mut app, 32.5 + dx, 32.5 + dy);
    }
    let tree = tree_at(&mut app, 34.5, 32.5);
    let worker = spawn(&mut app, p[0], unit_with(&data, MoveFlag::Foot), Vec2::new(8.0, 32.5));
    order(&mut app, worker, MoveGoal::entity(tree, Vec2::new(34.5, 32.5), 0.3));
    let arrived = run_until(&mut app, 30, |_| {}, |app| status(app, worker) == PathStatus::Arrived);
    assert!(arrived, "{:?} at {}", status(&app, worker), pos(&app, worker));
    assert!(obb(&app, worker).gap(&obb(&app, tree)) <= 0.3 + 1e-3);
}

#[test]
fn removing_the_goal_stops_the_unit() {
    let (mut app, p) = world(48.0, &[("ironhold", 0)]);
    let unit = spawn(&mut app, p[0], unit_with(&test_data(), MoveFlag::Foot), Vec2::new(10.0, 10.0));
    order(&mut app, unit, MoveGoal::point(Vec2::new(40.0, 40.0), 0.3));
    run_ticks(&mut app, 30);
    assert_eq!(status(&app, unit), PathStatus::Following);
    app.world_mut().entity_mut(unit).remove::<MoveGoal>();
    run_ticks(&mut app, 2);
    let here = pos(&app, unit);
    run_ticks(&mut app, 10);
    assert_eq!(status(&app, unit), PathStatus::Idle);
    assert_eq!(pos(&app, unit), here);
}

#[test]
fn a_group_funnels_through_a_narrow_gap() {
    let data = test_data();
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let (mut app, p) = world(80.0, &[(&plain, 0)]);
    // A cliff wall with a 3-cell gap at y = 39..42.
    paint(&mut app, "cliff", 38..42, 0..39);
    paint(&mut app, "cliff", 38..42, 42..80);
    let foot = unit_with(&data, MoveFlag::Foot);
    let units: Vec<Entity> = (0..40).map(|i| spawn(&mut app, p[0], foot, Vec2::new(10.0 + (i % 8) as f32 * 1.4, 30.0 + (i / 8) as f32 * 1.6))).collect();
    for &u in &units {
        order(&mut app, u, MoveGoal::point(Vec2::new(65.0, 40.0), 0.5));
    }
    let mut on_cliff = false;
    let done = run_until(
        &mut app,
        60,
        |app| on_cliff |= units.iter().any(|u| terrain_name(app, pos(app, *u)) == "cliff"),
        |app| units.iter().all(|u| status(app, *u) == PathStatus::Arrived),
    );
    assert!(done, "{:?}", units.iter().map(|u| status(&app, *u)).collect::<Vec<_>>());
    assert!(!on_cliff, "nobody stood on the cliff");
    let all = all_units(&mut app);
    assert!(max_pair_penetration(&all) <= 0.05);
}

#[test]
fn idle_units_step_aside_and_hold_units_stand_firm() {
    let (mut app, p) = world(80.0, &[("ironhold", 0)]);
    let foot = unit_with(&test_data(), MoveFlag::Foot);
    let idle: Vec<Entity> = (0..9).map(|i| spawn(&mut app, p[0], foot, Vec2::new(40.0, 36.8 + i as f32 * 0.8))).collect();
    let holder = spawn(&mut app, p[0], foot, Vec2::new(40.0, 20.0));
    app.world_mut().entity_mut(holder).insert(Stance::StandGround);
    let through = spawn(&mut app, p[0], foot, Vec2::new(20.0, 40.0));
    let bump = spawn(&mut app, p[0], foot, Vec2::new(20.0, 20.0));
    order(&mut app, through, MoveGoal::point(Vec2::new(60.0, 40.0), 0.3));
    order(&mut app, bump, MoveGoal::point(Vec2::new(60.0, 20.0), 0.3));
    let starts: Vec<Vec2> = idle.iter().map(|e| pos(&app, *e)).collect();
    let done = run_until(&mut app, 30, |_| {}, |app| status(app, through) == PathStatus::Arrived && status(app, bump) == PathStatus::Arrived);
    assert!(done, "through {:?}, bump {:?}", status(&app, through), status(&app, bump));
    let shoved = idle.iter().zip(&starts).map(|(e, s)| pos(&app, *e).distance(*s)).fold(0.0f32, f32::max);
    assert!(shoved < 2.0, "idle units stepped aside instead of being bulldozed ({shoved} m)");
    assert!(pos(&app, holder).distance(Vec2::new(40.0, 20.0)) < 0.3, "the StandGround unit held");
}

/// Riders sent one after another to a rally point by a building, where the
/// earlier ones arrived and now stand idle: each settles at the edge of the
/// blob as soon as it reaches it, instead of pushing into the riders packed
/// against the building for seconds (round 4: such pushes were held back
/// tick after tick, freezing the crowd, or popped riders out of it).
#[test]
fn riders_rallied_to_where_idle_groupmates_stand_settle_at_the_blob() {
    let data = test_data();
    let (mut app, p) = world(96.0, &[("ironhold", 0)]);
    let rider = unit_with(&data, MoveFlag::Mounted);
    let farm = building_sized(&data, 5.5..=6.5);
    let wall = spawn_building_at(&mut app, p[0], farm, Vec2::new(40.0, 53.0), 0.0, true);
    run_ticks(&mut app, 1);
    let rally = Vec2::new(40.0, 48.4);
    let half = Footprint::from_size(data.unit(rider).size).half;
    // The earlier riders, standing packed about the rally point.
    let idle: Vec<Entity> = (0..8).map(|i| spawn(&mut app, p[0], rider, rally + Vec2::new((i as f32 - 3.5) * (2.0 * half.x + 0.1), 0.0))).collect();
    run_ticks(&mut app, 2);
    assert!(idle.iter().all(|e| obb(&app, *e).gap(&obb(&app, wall)) < 1.6), "the blob stands by the building");
    let starts: Vec<Vec2> = idle.iter().map(|e| pos(&app, *e)).collect();
    let held_before = app.world().resource::<af_core::ext::movement::CollisionStats>().held;
    let mut fresh = Vec::new();
    for k in 0..3 {
        let e = spawn(&mut app, p[0], rider, Vec2::new(40.0 - 4.0 * k as f32, 30.0));
        order(&mut app, e, MoveGoal::point(rally, 0.35));
        fresh.push(e);
        run_ticks(&mut app, 20);
    }
    let settled = run_until(&mut app, 20, |_| {}, |app| fresh.iter().all(|e| status(app, *e) == PathStatus::Arrived));
    let statuses: Vec<PathStatus> = fresh.iter().map(|e| status(&app, *e)).collect();
    assert!(settled, "the late riders settled: {statuses:?}");
    let held = app.world().resource::<af_core::ext::movement::CollisionStats>().held - held_before;
    assert!(held <= 10, "nobody pushed on into the blob ({held} unit-ticks held back)");
    let shoved = idle.iter().zip(&starts).map(|(e, s)| pos(&app, *e).distance(*s)).fold(0.0f32, f32::max);
    assert!(shoved < 0.5, "the blob was not shoved about ({shoved} m)");
    for &e in &fresh {
        assert!(pos(&app, e).distance(rally) < 8.0, "{e} at {}", pos(&app, e));
    }
    assert!(max_pair_penetration(&all_units(&mut app)) <= 0.05);
}

#[test]
fn slow_turning_vehicles_turn_before_driving() {
    let (mut app, p) = world(80.0, &[("ironhold", 0)]);
    let data = test_data();
    let vehicle = data
        .unit_ids()
        .find(|u| matches!(data.unit(*u).locomotion, MoveFlag::Tracked | MoveFlag::Wheeled) && data.unit(*u).turn_rate <= 90.0)
        .expect("a slow-turning vehicle");
    let e = spawn(&mut app, p[0], vehicle, Vec2::new(40.0, 40.0));
    run_ticks(&mut app, 1);
    // The goal is straight behind it (facing +Z, goal at -Z).
    order(&mut app, e, MoveGoal::point(Vec2::new(40.0, 20.0), 0.5));
    let mut moved_while_facing_away = false;
    let done = run_until(
        &mut app,
        20,
        |app| {
            let off = af_geom::wrap_angle(std::f32::consts::PI - app.world().get::<Facing>(e).unwrap().0).abs();
            moved_while_facing_away |= off > FRAC_PI_2 + 0.05 && app.world().get::<Velocity>(e).unwrap().0.length() > 1e-3;
        },
        |app| status(app, e) == PathStatus::Arrived,
    );
    assert!(done);
    assert!(!moved_while_facing_away, "turned on the spot first");
}

#[test]
fn wide_vehicles_fit_through_a_gate() {
    let data = test_data();
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let (mut app, p) = world(80.0, &[(&plain, 0)]);
    paint(&mut app, "cliff", 39..41, 0..37);
    paint(&mut app, "cliff", 39..41, 43..80);
    spawn_building_at(&mut app, p[0], gate_building(&data), Vec2::new(40.0, 40.0), -FRAC_PI_2, true);
    let wide = data
        .unit_ids()
        .filter(|u| data.unit(*u).locomotion == MoveFlag::Tracked && !data.unit(*u).has_tag(Tag::Giant))
        .max_by(|a, b| data.unit(*a).size[0].total_cmp(&data.unit(*b).size[0]))
        .expect("a tracked unit");
    let units: Vec<Entity> = (0..6).map(|i| spawn(&mut app, p[0], wide, Vec2::new(12.0 + (i % 3) as f32 * 5.0, 30.0 + (i / 3) as f32 * 8.0))).collect();
    for (i, &u) in units.iter().enumerate() {
        order(&mut app, u, MoveGoal::point(Vec2::new(65.0, 30.0 + i as f32 * 4.0), 0.8));
    }
    let done = run_until(&mut app, 60, |_| {}, |app| units.iter().all(|u| status(app, *u) == PathStatus::Arrived));
    assert!(done, "{:?}", units.iter().map(|u| (status(&app, *u), pos(&app, *u))).collect::<Vec<_>>());
}

#[test]
fn a_chaser_catches_and_keeps_up_with_a_moving_target() {
    let (mut app, p) = world(80.0, &[("ironhold", 0), ("dune", 1)]);
    let data = test_data();
    let fast = data.unit_ids().filter(|u| data.unit(*u).locomotion == MoveFlag::Mounted).max_by(|a, b| data.unit(*a).speed.total_cmp(&data.unit(*b).speed)).expect("cavalry");
    let slow = data
        .unit_ids()
        .filter(|u| data.unit(*u).locomotion == MoveFlag::Foot && !data.unit(*u).has_tag(Tag::Giant))
        .min_by(|a, b| data.unit(*a).speed.total_cmp(&data.unit(*b).speed))
        .expect("infantry");
    let prey = spawn(&mut app, p[1], slow, Vec2::new(40.0, 40.0));
    let hunter = spawn(&mut app, p[0], fast, Vec2::new(10.0, 10.0));
    order(&mut app, prey, MoveGoal::point(Vec2::new(75.0, 75.0), 0.3));
    order(&mut app, hunter, MoveGoal::entity(prey, Vec2::new(40.0, 40.0), 0.5));
    let caught = run_until(&mut app, 30, |_| {}, |app| status(app, hunter) == PathStatus::Arrived);
    assert!(caught && status(&app, prey) == PathStatus::Following, "caught it on the move: hunter {:?} at {}, prey {:?} at {}", status(&app, hunter), pos(&app, hunter), status(&app, prey), pos(&app, prey));
    let mut close = 0;
    for _ in 0..60 {
        run_ticks(&mut app, 1);
        if obb(&app, hunter).gap(&obb(&app, prey)) <= 0.5 + 0.5 {
            close += 1;
        }
    }
    assert!(close >= 55, "kept up ({close}/60 ticks within reach)");
}

/// Same inputs, same outcome: positions match bit for bit.
#[test]
fn movement_is_deterministic() {
    fn run() -> Vec<Vec2> {
        let (mut app, p) = world(64.0, &[("ironhold", 0)]);
        let data = test_data();
        spawn_building_at(&mut app, p[0], building_sized(&data, 8.0..=10.0), Vec2::new(32.0, 32.0), 0.7, true);
        let foot = unit_with(&data, MoveFlag::Foot);
        let units: Vec<Entity> = (0..12).map(|i| spawn(&mut app, p[0], foot, Vec2::new(10.0 + (i % 4) as f32, 26.0 + (i / 4) as f32 * 2.0))).collect();
        for (i, u) in units.iter().enumerate() {
            order(&mut app, *u, MoveGoal::point(Vec2::new(54.0, 30.0 + i as f32 * 0.3), 0.4));
        }
        run_ticks(&mut app, 240);
        units.iter().map(|u| pos(&app, *u)).collect()
    }
    assert_eq!(run(), run());
}

/// 1000 units crowding into four rally points (heavy collision load) stay
/// within a frame budget and never overlap much.
#[test]
fn a_thousand_units_crowd_within_budget() {
    let (mut app, p) = world(200.0, &[("ironhold", 0), ("sylvan", 1)]);
    let data = test_data();
    let hall = building_sized(&data, 8.0..=10.0);
    for (i, at) in [Vec2::new(100.0, 100.0), Vec2::new(100.0, 60.0), Vec2::new(60.0, 100.0)].into_iter().enumerate() {
        spawn_building_at(&mut app, p[0], hall, at, 0.4 * i as f32, true);
    }
    let foot = unit_with(&data, MoveFlag::Foot);
    let rally = [Vec2::new(60.0, 60.0), Vec2::new(140.0, 60.0), Vec2::new(60.0, 140.0), Vec2::new(140.0, 140.0)];
    let mut units = Vec::new();
    for i in 0..1000 {
        let at = Vec2::new(61.0 + (i % 40) as f32 * 2.0, 62.0 + (i / 40) as f32 * 3.0);
        let u = spawn(&mut app, p[i % 2], foot, at);
        order(&mut app, u, MoveGoal::point(rally[i % 4], 0.5));
        units.push(u);
    }
    run_ticks(&mut app, 60); // paths served, columns on the move
    let ticks = 540; // through the densest phase, crowds filling up
    let (mut worst, mut sim) = (0.0f32, Duration::ZERO);
    for t in 0..ticks {
        let start = Instant::now();
        run_ticks(&mut app, 1);
        sim += start.elapsed();
        if t % 60 == 59 {
            let all = all_units(&mut app);
            worst = worst.max(max_pair_penetration(&all));
        }
    }
    let per_tick = sim.as_secs_f64() * 1000.0 / ticks as f64;
    let arrived = units.iter().filter(|u| status(&app, **u) == PathStatus::Arrived).count();
    println!("1000 units crowding: {per_tick:.2} ms per tick, {arrived} arrived, worst overlap {worst:.3} m");
    assert!(arrived > 600, "crowds formed ({arrived} arrived)");
    assert!(worst < 0.1, "overlap {worst}");
    assert!(per_tick < 8.0, "{per_tick:.2} ms per tick");
}

/// A lane one cell wide along y = 32 through a cliff wall at x = 28..36,
/// on a map for a faction whose foot units do not climb.
fn lane_through_cliffs(size: f32, players: &[u8]) -> (App, Vec<PlayerId>, GameData) {
    let data = (*test_data()).clone();
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let setup: Vec<(&str, u8)> = players.iter().map(|t| (plain.as_str(), *t)).collect();
    let (mut app, p) = world(size, &setup);
    paint(&mut app, "cliff", 28..36, 0..32);
    paint(&mut app, "cliff", 28..36, 33..size as i32);
    (app, p, data)
}

/// Workers of two enemy players meet head-on in a lane one cell wide, one
/// of them with two more of its side filing in behind it: the other side
/// backs out of the lane, and everyone gets through without the boxes
/// overlapping.
#[test]
fn workers_meeting_head_on_in_a_lane_all_get_through() {
    let (mut app, p, data) = lane_through_cliffs(64.0, &[0, 1]);
    let foot = unit_with(&data, MoveFlag::Foot);
    let east: Vec<Entity> = (0..3).map(|i| spawn(&mut app, p[0], foot, Vec2::new(25.0 - i as f32 * 1.2, 32.5))).collect();
    let west = spawn(&mut app, p[1], foot, Vec2::new(39.0, 32.5));
    run_ticks(&mut app, 1);
    for (i, e) in east.iter().enumerate() {
        order(&mut app, *e, MoveGoal::point(Vec2::new(44.0 + i as f32 * 1.5, 32.5), 0.4));
    }
    order(&mut app, west, MoveGoal::point(Vec2::new(18.0, 32.5), 0.4));
    let all: Vec<Entity> = east.iter().copied().chain([west]).collect();
    let mut worst = 0.0f32;
    let done = run_until(
        &mut app,
        40,
        |app| {
            let boxes: Vec<(Entity, Obb)> = all.iter().map(|&e| (e, obb(app, e))).collect();
            worst = worst.max(max_pair_penetration(&boxes));
        },
        |app| all.iter().all(|&e| status(app, e) == PathStatus::Arrived),
    );
    let report: Vec<_> = all.iter().map(|&e| (status(&app, e), pos(&app, e))).collect();
    assert!(done, "{report:?}");
    assert!(worst < 0.1, "overlap {worst}");
}

/// A worker sent down a corridor two cells wide that a column of idle
/// riders fills, each rider several times its size, wades through: idle
/// units make way for walkers whatever their size.
#[test]
fn a_worker_wades_through_idle_riders_filling_a_corridor() {
    let data = (*test_data()).clone();
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let (mut app, p) = world(64.0, &[(&plain, 0)]);
    // Corridor x = 30..32 along y, cliffs either side.
    paint(&mut app, "cliff", 20..30, 20..44);
    paint(&mut app, "cliff", 32..42, 20..44);
    let rider = unit_with(&data, MoveFlag::Mounted);
    for i in 0..8 {
        spawn(&mut app, p[0], rider, Vec2::new(30.5 + (i % 2) as f32 * 1.0, 26.0 + (i / 2) as f32 * 2.2));
    }
    let worker = spawn(&mut app, p[0], unit_with(&data, MoveFlag::Foot), Vec2::new(31.0, 16.0));
    run_ticks(&mut app, 1);
    order(&mut app, worker, MoveGoal::point(Vec2::new(31.0, 48.0), 0.4));
    let arrived = run_until(&mut app, 40, |_| {}, |app| status(app, worker) == PathStatus::Arrived);
    assert!(arrived, "{:?} at {}", status(&app, worker), pos(&app, worker));
}

/// An idle rider standing across a corridor as wide as it is long, flush
/// against both cliffs, is walked along it by a worker coming through: a
/// unit is braced only against being pushed into a wall, not along it.
#[test]
fn a_rider_standing_across_a_corridor_is_walked_along_it() {
    let data = (*test_data()).clone();
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let (mut app, p) = world(64.0, &[(&plain, 0)]);
    let rider = unit_with(&data, MoveFlag::Mounted);
    let length = data.unit(rider).size[2];
    assert!(length < 2.0, "the rider fits across the corridor");
    paint(&mut app, "cliff", 20..30, 20..44);
    paint(&mut app, "cliff", 32..42, 20..44);
    // Across the corridor x = 30..32, its ends a hair from both cliffs.
    let across = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), rider, Vec2::new(31.0, 30.0), FRAC_PI_2));
    let worker = spawn(&mut app, p[0], unit_with(&data, MoveFlag::Foot), Vec2::new(31.0, 22.0));
    run_ticks(&mut app, 1);
    order(&mut app, worker, MoveGoal::point(Vec2::new(31.0, 48.0), 0.4));
    let arrived = run_until(&mut app, 40, |_| {}, |app| status(app, worker) == PathStatus::Arrived);
    assert!(arrived, "{:?} at {}, rider at {}", status(&app, worker), pos(&app, worker), pos(&app, across));
}

/// Two workers of one player meet head-on in a gap one unit wide: the
/// younger gives way (is pushed back out and steps aside), so both get
/// through instead of blocking each other until their paths fail.
#[test]
fn allies_meeting_head_on_in_a_narrow_gap_both_get_through() {
    let data = test_data();
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let (mut app, p) = world(48.0, &[(&plain, 0)]);
    // Cliffs leave a corridor one cell wide along y = 24, x = 20..28.
    paint(&mut app, "cliff", 20..28, 0..24);
    paint(&mut app, "cliff", 20..28, 25..48);
    let foot = unit_with(&data, MoveFlag::Foot);
    let (east, west) = (Vec2::new(34.5, 24.5), Vec2::new(13.5, 24.5));
    let a = spawn(&mut app, p[0], foot, Vec2::new(19.0, 24.5));
    let b = spawn(&mut app, p[0], foot, Vec2::new(29.0, 24.5));
    order(&mut app, a, MoveGoal::point(east, 0.4));
    order(&mut app, b, MoveGoal::point(west, 0.4));
    let mut worst = 0.0f32;
    let done = run_until(
        &mut app,
        30,
        |app| worst = worst.max(penetration(&obb(app, a), &obb(app, b))),
        |app| status(app, a) == PathStatus::Arrived && status(app, b) == PathStatus::Arrived,
    );
    assert!(done, "a {:?} at {}, b {:?} at {}", status(&app, a), pos(&app, a), status(&app, b), pos(&app, b));
    assert!(worst < 0.05, "overlap {worst}");
}

/// A crowd ordered through a gap one cell wide in a cliff band: the two
/// units that reach its mouth abreast slip in one after the other (round
/// the corners of the cliff) and the rest wait their turn, so all of them
/// get through and none gives up (before, two stood abreast in the mouth,
/// each held back by a corner of the gap, and the queue behind them gave
/// up within seconds, as the chokepoint bench found).
#[test]
fn a_crowd_files_through_a_cliff_gap_one_cell_wide() {
    let data = test_data();
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let (mut app, p) = world(80.0, &[(&plain, 0)]);
    // A cliff band six cells thick with a gap one cell wide at y = 40..41.
    paint(&mut app, "cliff", 38..44, 0..40);
    paint(&mut app, "cliff", 38..44, 41..80);
    // Infantry: two abreast (1.4 m) do not fit the gap, nor stand clear of it.
    let foot = foot_at_least(&data, 0.7);
    let units: Vec<Entity> = (0..24).map(|i| spawn(&mut app, p[0], foot, Vec2::new(33.0 - (i / 6) as f32 * 1.2, 37.5 + (i % 6) as f32 * 1.2))).collect();
    run_ticks(&mut app, 1);
    for &u in &units {
        order(&mut app, u, MoveGoal::point(Vec2::new(60.0, 40.5), 0.5));
    }
    let done = run_until(&mut app, 60, |_| {}, |app| units.iter().all(|u| status(app, *u) != PathStatus::Following));
    let report: Vec<_> = units.iter().map(|&u| (status(&app, u), pos(&app, u))).collect();
    assert!(done && units.iter().all(|&u| status(&app, u) == PathStatus::Arrived && pos(&app, u).x > 44.0), "{report:?}");
}

/// Two hostile columns crossing each other in a channel two cells wide and
/// eight long, their units of interleaved ages: a file that gives way to
/// the other column goes on giving way to the rest of it, and backs out
/// behind its front instead of squeezing it, so both columns get through
/// (before, the right of way flipped with every new pair that met, the
/// columns packed the channel and gave up in it).
#[test]
fn columns_crossing_in_a_channel_two_cells_wide_both_get_through() {
    let data = test_data();
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let (mut app, p) = world(80.0, &[(&plain, 0), (&plain, 1)]);
    // Cliffs leave a channel along y = 40..42, x = 36..44.
    paint(&mut app, "cliff", 36..44, 0..40);
    paint(&mut app, "cliff", 36..44, 42..80);
    let foot = unit_with(&data, MoveFlag::Foot);
    let (mut east, mut west) = (Vec::new(), Vec::new());
    for i in 0..12 {
        let (rank, col) = ((i / 4) as f32 * 1.2, (i % 4) as f32 * 1.2 - 1.8);
        east.push(spawn(&mut app, p[0], foot, Vec2::new(31.0 - rank, 41.0 + col)));
        west.push(spawn(&mut app, p[1], foot, Vec2::new(49.0 + rank, 41.0 + col)));
    }
    run_ticks(&mut app, 1);
    for &e in &east {
        order(&mut app, e, MoveGoal::point(Vec2::new(58.0, 41.0), 0.5));
    }
    for &w in &west {
        order(&mut app, w, MoveGoal::point(Vec2::new(22.0, 41.0), 0.5));
    }
    let all: Vec<Entity> = east.iter().chain(&west).copied().collect();
    let done = run_until(&mut app, 90, |_| {}, |app| all.iter().all(|u| status(app, *u) != PathStatus::Following));
    let report: Vec<_> = all.iter().map(|&u| (status(&app, u), pos(&app, u))).collect();
    let through = east.iter().all(|&e| pos(&app, e).x > 44.0) && west.iter().all(|&w| pos(&app, w).x < 36.0);
    assert!(done && through && all.iter().all(|&u| status(&app, u) == PathStatus::Arrived), "{report:?}");
}

/// A worker walking up to a bush behind two others that leave a slot
/// narrower than itself (a berry patch from a generated map) never ends a
/// tick wedged into them.
#[test]
fn a_unit_never_ends_wedged_between_obstacles() {
    let (mut app, p) = world(48.0, &[("ironhold", 0)]);
    let data = test_data();
    let bush = data
        .node_ids()
        .filter(|n| data.node(*n).blocking)
        .min_by(|a, b| (data.node(*a).size[0] - 1.4).abs().total_cmp(&(data.node(*b).size[0] - 1.4).abs()))
        .expect("a bush-sized node");
    let plant = |app: &mut App, x: f32, y: f32| with_commands(app, |c, data, _| spawn_node(c, data, bush, Vec2::new(x, y), 2.65, None));
    let target = plant(&mut app, 21.61, 20.19);
    let others = [plant(&mut app, 22.43, 22.90), plant(&mut app, 20.66, 21.96), plant(&mut app, 23.37, 21.14)];
    let worker = spawn(&mut app, p[0], unit_with(&data, MoveFlag::Foot), Vec2::new(21.8, 27.0));
    order(&mut app, worker, MoveGoal::entity(target, Vec2::new(21.61, 20.19), 0.3));
    let mut worst = 0.0f32;
    run_until(&mut app, 12, |app| worst = others.iter().map(|t| penetration(&obb(app, worker), &obb(app, *t))).fold(worst, f32::max), |_| false);
    assert!(worst <= 0.05 + 1e-4, "wedged {worst} m into a bush");
}

/// A unit whose straight line to its next waypoint grazes a building's
/// corner slides round it instead of grinding against it until it gives up
/// (a worker leaving a mining camp it stood beside).
#[test]
fn a_unit_slides_round_a_corner_its_path_grazes() {
    let (mut app, p) = world(48.0, &[("ironhold", 0)]);
    let data = test_data();
    let camp = building_sized(&data, 3.5..=4.5);
    let half = data.building(camp).size[0] * 0.5;
    let corner = Vec2::new(24.0, 24.0);
    spawn_building_at(&mut app, p[0], camp, corner + Vec2::new(-half, half), 0.0, true);
    // Just past the corner, its top edge a little above the building's bottom.
    let unit = unit_with(&data, MoveFlag::Foot);
    let worker = spawn(&mut app, p[0], unit, corner + Vec2::new(data.unit(unit).size[0] * 0.5 + 0.02, -0.03));
    run_ticks(&mut app, 1);
    let goal = corner + Vec2::new(-12.0, -0.6);
    order(&mut app, worker, MoveGoal::point(goal, 0.4));
    let arrived = run_until(&mut app, 15, |_| {}, |app| status(app, worker) == PathStatus::Arrived);
    assert!(arrived, "{:?} at {}", status(&app, worker), pos(&app, worker));
}

/// A worker inside a berry patch (from a generated map) walks up to a bush
/// whose side facing it lies behind two others: it goes round to where it
/// can reach the bush instead of pushing at the slot between them.
#[test]
fn a_worker_goes_round_to_reach_a_bush_inside_a_patch() {
    let (mut app, p) = world(48.0, &[("ironhold", 0)]);
    let data = test_data();
    let bush = data
        .node_ids()
        .filter(|n| data.node(*n).blocking)
        .min_by(|a, b| (data.node(*a).size[0] - 1.4).abs().total_cmp(&(data.node(*b).size[0] - 1.4).abs()))
        .expect("a bush-sized node");
    let patch = [(23.19, 25.52), (22.12, 27.20), (21.50, 24.44), (24.26, 23.83), (24.88, 26.59), (25.95, 24.90), (20.43, 26.13), (23.81, 28.28)];
    let bushes: Vec<Entity> = patch.iter().map(|&(x, y)| with_commands(&mut app, |c, data, _| spawn_node(c, data, bush, Vec2::new(x, y), 1.01, None))).collect();
    let (target, at) = (bushes[1], Vec2::new(22.12, 27.20));
    let worker = spawn(&mut app, p[0], unit_with(&data, MoveFlag::Foot), Vec2::new(24.65, 27.63));
    run_ticks(&mut app, 1);
    order(&mut app, worker, MoveGoal::entity(target, at, 0.3));
    let reached = run_until(&mut app, 15, |_| {}, |app| obb(app, worker).gap(&obb(app, target)) <= 0.75);
    assert!(reached, "{:?} at {}, {} m from the bush", status(&app, worker), pos(&app, worker), obb(&app, worker).gap(&obb(&app, target)));
}

/// A building placed over a unit standing next to another one, closer
/// than the unit is wide: pushed out of either it lands in the other, so it
/// is set down on open ground nearby instead of staying trapped inside.
#[test]
fn a_unit_trapped_under_a_new_building_is_set_down_outside() {
    let (mut app, p) = world(48.0, &[("ironhold", 0)]);
    let data = test_data();
    let house = building_sized(&data, 3.5..=4.5);
    let half = data.building(house).size[0] * 0.5;
    let first = spawn_building_at(&mut app, p[0], house, Vec2::new(20.0 - half, 24.0), 0.0, true);
    let unit = unit_with(&data, MoveFlag::Foot);
    let worker = spawn(&mut app, p[0], unit, Vec2::new(20.0 + data.unit(unit).size[0] * 0.5 + 0.05, 24.0));
    run_ticks(&mut app, 2);
    let second = spawn_building_at(&mut app, p[0], house, Vec2::new(20.0 + 0.4 + half, 24.0), 0.0, true);
    run_ticks(&mut app, 30);
    let inside = [first, second].iter().map(|b| penetration(&obb(&app, worker), &obb(&app, *b))).fold(0.0f32, f32::max);
    assert!(inside <= 0.05, "{inside} m inside a building at {}", pos(&app, worker));
}

/// Two groups of riders crossing head-on in a corridor just two riders
/// wide, between cliffs. Riders pushed back by a cliff are braced against
/// it, so the crowd makes room among itself instead of shoving them into
/// the cliff and back every solver pass; riders turn only as far as the
/// crowd leaves room, and what the passes cannot part the final word does:
/// no tick ends with two of them overlapping past the 0.1 m the game
/// allows (0.54 m with neither bracing nor the final word, 0.4 m with
/// bracing alone).
#[test]
fn riders_crossing_in_a_tight_corridor_stay_apart() {
    let data = test_data();
    let plain = faction_without(&data, &[MoveFlag::Climber]);
    let (mut app, p) = world(80.0, &[(&plain, 0)]);
    paint(&mut app, "cliff", 30..50, 0..39);
    paint(&mut app, "cliff", 30..50, 41..80);
    let rider = unit_with(&data, MoveFlag::Mounted);
    let mut units = Vec::new();
    for i in 0..8 {
        let (row, col) = ((i / 4) as f32, (i % 4) as f32);
        let west = spawn(&mut app, p[0], rider, Vec2::new(18.0 + col * 2.2, 36.0 + row * 3.0));
        let east = spawn(&mut app, p[0], rider, Vec2::new(56.0 + col * 2.2, 36.0 + row * 3.0));
        order(&mut app, west, MoveGoal::point(Vec2::new(62.0 + col * 2.2, 36.0 + row * 3.0), 0.5));
        order(&mut app, east, MoveGoal::point(Vec2::new(14.0 + col * 2.2, 36.0 + row * 3.0), 0.5));
        units.extend([west, east]);
    }
    let mut worst = 0.0f32;
    run_until(
        &mut app,
        40,
        |app| {
            let boxes: Vec<(Entity, Obb)> = units.iter().map(|&e| (e, obb(app, e))).collect();
            worst = worst.max(max_pair_penetration(&boxes));
        },
        |_| false,
    );
    assert!(worst <= MAX_OVERLAP, "riders overlapped by {worst} m");
}

/// Deepest overlap of two ground units, or of one with a building, that a
/// tick may end with (the game's rule; the solver aims at half of it).
const MAX_OVERLAP: f32 = 0.1;
/// Width of the lane in [`boxes_never_end_a_tick_overlapping_in_a_crowded_lane`] (metres).
const LANE_WIDTH: f32 = 2.4;

/// Deepest overlap of any unit with any other unit, and with any obstacle.
fn deepest_overlaps(app: &mut App) -> (f32, f32) {
    let units = all_units(app);
    let w = app.world_mut();
    let mut q = w.query_filtered::<(&Position, &Facing, &Footprint), With<Obstacle>>();
    let obstacles: Vec<Obb> = q.iter(w).map(|(p, f, fp)| obb_of(p, f, fp)).collect();
    let statics = units.iter().flat_map(|(_, u)| obstacles.iter().map(move |o| penetration(u, o))).fold(0.0f32, f32::max);
    (max_pair_penetration(&units), statics)
}

/// Siege engines, riders and workers from both ends of a lane between two
/// rows of buildings, every group to one point beyond the other end: big
/// boxes that turn slowly, a crowd that fills the lane, walls to be shoved
/// against. No tick ends with two units, or a unit and a building,
/// overlapping past the game's rule.
#[test]
fn boxes_never_end_a_tick_overlapping_in_a_crowded_lane() {
    let (mut app, p) = world(96.0, &[("ironhold", 0), ("sylvan", 1)]);
    let data = test_data();
    let house = building_sized(&data, 3.5..=4.5);
    let side = data.building(house).size[0];
    // Two rows of houses along x, a lane between them just wider than
    // the widest engine.
    let lane = LANE_WIDTH * 0.5;
    for i in 0..6 {
        let x = 34.0 + i as f32 * side;
        spawn_building_at(&mut app, p[0], house, Vec2::new(x, 48.0 - lane - side * 0.5), 0.0, true);
        spawn_building_at(&mut app, p[1], house, Vec2::new(x, 48.0 + lane + side * 0.5), 0.0, true);
    }
    let engine = data
        .unit_ids()
        .filter(|u| matches!(data.unit(*u).locomotion, MoveFlag::Tracked | MoveFlag::Wheeled))
        .max_by(|a, b| (data.unit(*a).size[0] * data.unit(*a).size[2]).total_cmp(&(data.unit(*b).size[0] * data.unit(*b).size[2])))
        .expect("a siege engine");
    let rider = unit_with(&data, MoveFlag::Mounted);
    let foot = unit_with(&data, MoveFlag::Foot);
    for (k, kind) in [engine, rider, rider, foot, foot, rider, engine, foot].into_iter().enumerate() {
        let side = (k % 2) as f32 * 2.0 - 1.0;
        let west = spawn(&mut app, p[0], kind, Vec2::new(20.0 - (k / 2) as f32 * 4.0, 48.0 + side));
        let east = spawn(&mut app, p[1], kind, Vec2::new(76.0 + (k / 2) as f32 * 4.0, 48.0 + side));
        order(&mut app, west, MoveGoal::point(Vec2::new(80.0, 48.0), 0.5));
        order(&mut app, east, MoveGoal::point(Vec2::new(16.0, 48.0), 0.5));
    }
    let (mut units, mut statics) = (0.0f32, 0.0f32);
    for _ in 0..45 * TICKS_PER_SECOND {
        run_ticks(&mut app, 1);
        let (u, s) = deepest_overlaps(&mut app);
        (units, statics) = (units.max(u), statics.max(s));
    }
    assert!(units <= MAX_OVERLAP && statics <= MAX_OVERLAP, "units overlapped by {units} m, a building by {statics} m");
}

/// A house laid over a row of workers standing in the slot between two
/// others, where pushed out of one building each lands in another: they
/// are set down outside, side by side, not all on the one nearest free
/// spot.
#[test]
fn units_under_a_new_building_are_set_down_apart() {
    let (mut app, p) = world(48.0, &[("ironhold", 0)]);
    let data = test_data();
    let house = building_sized(&data, 3.5..=4.5);
    let half = data.building(house).size[0] * 0.5;
    let left = spawn_building_at(&mut app, p[0], house, Vec2::new(20.0 - half, 24.0), 0.0, true);
    let right = spawn_building_at(&mut app, p[0], house, Vec2::new(20.0 + 1.0 + 2.0 * half + 0.6 + half, 24.0), 0.0, true);
    let unit = unit_with(&data, MoveFlag::Foot);
    let w = data.unit(unit).size[0];
    let workers: Vec<Entity> = (0..6).map(|i| spawn(&mut app, p[0], unit, Vec2::new(20.0 + w * 0.5 + 0.05, 22.6 + i as f32 * (w + 0.02)))).collect();
    run_ticks(&mut app, 2);
    let middle = spawn_building_at(&mut app, p[0], house, Vec2::new(20.0 + 0.3 + half, 24.0), 0.0, true);
    let mut worst = (0.0f32, 0.0f32);
    for _ in 0..30 {
        run_ticks(&mut app, 1);
        let (u, s) = deepest_overlaps(&mut app);
        worst = (worst.0.max(u), worst.1.max(s));
    }
    assert!(worst.0 <= MAX_OVERLAP, "workers overlapped by {} m", worst.0);
    let inside = workers.iter().flat_map(|w| [left, middle, right].map(|b| penetration(&obb(&app, *w), &obb(&app, b)))).fold(0.0f32, f32::max);
    assert!(inside <= 0.05, "{inside} m inside a building");
}
