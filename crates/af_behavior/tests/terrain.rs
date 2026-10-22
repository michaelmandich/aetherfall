//! Behaviour on maps the terrain splits: automatic choices only take what a
//! unit can get to over the terrain (af_movement's `NavRegions`), on the
//! headless harness of `behavior.rs` with a river painted on its map.

#[allow(dead_code)] // each test binary uses only part of the harness
mod common;

use af_core::ext::movement::NavRegions;
use af_core::prelude::*;
use bevy::prelude::*;
use common::Sim;

const P0: u8 = 0;
const P1: u8 = 1;

/// Paint deep water over x = 60..66 the whole height of the map (with a
/// ford over y in `ford` when given), and publish the regions as
/// af_movement does.
fn river(sim: &mut Sim, ford: Option<std::ops::Range<i32>>) {
    let data = sim.data();
    let deep = data.terrain_id("deep_water").expect("deep water").0;
    let crossing = data.terrain_id("ford").expect("ford").0;
    let world = sim.app.world_mut();
    let mut nav = world.resource_mut::<Nav>();
    for y in 0..nav.height() as i32 {
        for x in 60..66 {
            let t = if ford.as_ref().is_some_and(|f| f.contains(&y)) { crossing } else { deep };
            nav.set_terrain(IVec2::new(x, y), t);
        }
    }
    let regions = NavRegions::new(&nav);
    world.insert_resource(regions);
}

/// A worker whose tree runs out moves on to a tree on its own bank, not
/// to the nearer one across the river.
#[test]
fn a_worker_moves_on_to_a_tree_on_its_own_bank() {
    let mut sim = Sim::new(&[("ironhold", 0)]);
    river(&mut sim, None);
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(52.0, 40.0));
    let first = sim.node("oak", Vec2::new(56.0, 40.0), Some(2.0));
    let across = sim.node("oak", Vec2::new(69.0, 40.0), None);
    let this_side = sim.node("oak", Vec2::new(50.0, 27.0), None);
    assert!(Vec2::new(56.0, 40.0).distance(Vec2::new(69.0, 40.0)) < Vec2::new(56.0, 40.0).distance(Vec2::new(50.0, 27.0)));
    sim.order(P0, &[worker], CommandKind::Gather(first));
    sim.run_secs(12.0);
    assert!(!sim.alive(first));
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: this_side }), "not {across}");
}

/// A worker sent to a tree across the river takes one like it on its own
/// bank instead of walking to the water and failing there.
#[test]
fn a_worker_sent_across_the_river_takes_a_tree_on_its_bank() {
    let mut sim = Sim::new(&[("ironhold", 0)]);
    river(&mut sim, None);
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(52.0, 40.0));
    let across = sim.node("oak", Vec2::new(70.0, 40.0), None);
    let this_side = sim.node("oak", Vec2::new(55.0, 48.0), None);
    sim.order(P0, &[worker], CommandKind::Gather(across));
    sim.ticks(3);
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: this_side }));
}

/// Carrying wood, a worker takes the drop site it can walk to, though
/// another stands nearer across the river.
#[test]
fn a_worker_delivers_to_a_drop_site_on_its_bank() {
    let mut sim = Sim::new(&[("ironhold", 0)]);
    river(&mut sim, None);
    let near_across = sim.building(P0, "town_center", Vec2::new(74.0, 40.0), true);
    let far_this_side = sim.building(P0, "town_center", Vec2::new(30.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(55.0, 40.0));
    let oak = sim.node("oak", Vec2::new(57.0, 43.0), None);
    let wood = sim.resource(P0, "wood");
    sim.order(P0, &[worker], CommandKind::Gather(oak));
    sim.run_secs(25.0);
    assert!(sim.resource(P0, "wood") > wood, "delivered");
    assert!(sim.pos(worker).x < 60.0, "never crossed the water");
    let _ = (near_across, far_this_side);
}

/// With a ford far up the river, both drop sites can be reached, but the
/// one on the worker's bank is nearer by the way round.
#[test]
fn a_worker_delivers_to_the_drop_site_nearest_by_the_way_round() {
    let mut sim = Sim::new(&[("ironhold", 0)]);
    river(&mut sim, Some(112..120));
    let regions = sim.app.world().resource::<NavRegions>();
    let walker = Mobile { mobility: Mobility::of(MoveFlag::Foot), radius: 0.3, turn_rate: 6.0, flying: false };
    assert!(regions.reachable(Vec2::new(55.0, 40.0), Vec2::new(74.0, 40.0), walker.mobility, walker.radius, 1.0), "by the ford");
    sim.building(P0, "town_center", Vec2::new(74.0, 40.0), true);
    sim.building(P0, "town_center", Vec2::new(30.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(55.0, 40.0));
    let oak = sim.node("oak", Vec2::new(57.0, 43.0), None);
    let wood = sim.resource(P0, "wood");
    sim.order(P0, &[worker], CommandKind::Gather(oak));
    sim.run_secs(25.0);
    assert!(sim.resource(P0, "wood") > wood, "delivered");
    assert!(sim.pos(worker).x < 60.0, "the stand-in walker goes straight: it would have waded to the nearer camp");
}

/// A group ordered to a spot just across the water forms up on the near
/// bank, every slot on ground the units can get to.
#[test]
fn formation_slots_stay_on_the_near_bank() {
    let mut sim = Sim::new(&[("ironhold", 0)]);
    river(&mut sim, None);
    let squad: Vec<Entity> = (0..9).map(|i| sim.unit(P0, "spearman", Vec2::new(40.0, 30.0 + i as f32 * 2.0))).collect();
    sim.order(P0, &squad, CommandKind::Move(Vec2::new(68.0, 38.0)));
    sim.ticks(2);
    for &e in &squad {
        match sim.current_order(e) {
            Some(Order::Move { target }) => assert!(target.x < 60.0, "slot across the water: {target}"),
            other => panic!("{other:?}"),
        }
    }
}

/// An idle spearman does not take on a hostile across the river (it could
/// only walk to the bank and stand there), while an archer whose bow
/// reaches across does.
#[test]
fn soldiers_engage_across_the_water_only_what_their_weapons_reach() {
    let mut sim = Sim::new(&[("ironhold", 0), ("dune", 1)]);
    river(&mut sim, None);
    // Both within sight of the enemy (9 and 10 m), the archer's bow (8 m)
    // reaches it from the bank, the spear does not.
    let spear = sim.unit(P0, "spearman", Vec2::new(58.5, 44.0));
    let archer = sim.unit(P0, "archer", Vec2::new(57.0, 50.0));
    let enemy = sim.unit(P1, "peasant", Vec2::new(66.5, 47.0));
    assert!(sim.pos(enemy).distance(sim.pos(spear)) < 8.6 && sim.pos(enemy).distance(sim.pos(archer)) < 10.0);
    sim.run_secs(1.0);
    assert!(sim.get::<AttackTarget>(spear).is_none(), "the spearman stays");
    assert!(sim.has::<Idle>(spear));
    assert_eq!(sim.get::<AttackTarget>(archer).map(|a| a.entity), Some(enemy), "the archer shoots across");
}

/// With a ford far up the river the way round to the enemy is long either
/// way, but the archer gets within its bow's reach of it on its own bank:
/// it shoots across, while the spearman, which would have to go round,
/// stays.
#[test]
fn an_archer_shoots_across_a_forded_river_from_its_bank() {
    let mut sim = Sim::new(&[("ironhold", 0), ("dune", 1)]);
    river(&mut sim, Some(112..120));
    let spear = sim.unit(P0, "spearman", Vec2::new(58.5, 44.0));
    let archer = sim.unit(P0, "archer", Vec2::new(57.0, 50.0));
    let enemy = sim.unit(P1, "peasant", Vec2::new(66.5, 47.0));
    sim.run_secs(1.0);
    assert!(sim.get::<AttackTarget>(spear).is_none(), "the spearman stays");
    assert!(sim.has::<Idle>(spear));
    assert_eq!(sim.get::<AttackTarget>(archer).map(|a| a.entity), Some(enemy), "the archer shoots across");
}
