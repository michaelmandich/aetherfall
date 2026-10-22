//! Unit behaviour end to end on a headless sim (af_movement replaced by a
//! straight-line walker, no af_combat or af_economy).

mod common;

use std::collections::BTreeSet;

use af_core::ext::combat::{in_weapon_range, weapon_reach};
use af_core::prelude::*;
use bevy::prelude::*;
use common::Sim;

const P0: u8 = 0;
const P1: u8 = 1;

fn solo() -> Sim {
    Sim::new(&[("ironhold", 0)])
}

fn duel() -> Sim {
    Sim::new(&[("ironhold", 0), ("dune", 1)])
}

#[test]
fn worker_gathers_wood_and_deposits_it_at_the_town_centre() {
    let mut sim = solo();
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 40.0));
    let oak = sim.node("oak", Vec2::new(50.0, 40.0), None);
    let wood = sim.resource(P0, "wood");
    sim.order(P0, &[worker], CommandKind::Gather(oak));
    sim.run_secs(30.0);
    assert!(sim.resource(P0, "wood") >= wood + 9.99, "wood {} -> {}", wood, sim.resource(P0, "wood"));
    assert!(sim.gathered(P0, "wood") >= 9.99);
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: oak }), "goes back to the tree");
    assert!(sim.get::<ResourceNode>(oak).unwrap().remaining < 130.0 - 9.99);
}

#[test]
fn depleted_node_dies_and_the_worker_goes_idle() {
    let mut sim = solo();
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 40.0));
    let bush = sim.node("berry_bush", Vec2::new(49.0, 40.0), Some(3.0));
    let food = sim.resource(P0, "food");
    sim.order(P0, &[worker], CommandKind::Gather(bush));
    sim.run_secs(15.0);
    assert!(!sim.alive(bush), "the bush is gone");
    assert!((sim.resource(P0, "food") - food - 3.0).abs() < 0.01, "its food was delivered");
    assert_eq!(sim.current_order(worker), None, "nothing similar nearby");
    assert!(sim.has::<Idle>(worker));
}

#[test]
fn worker_moves_on_to_a_similar_node() {
    let mut sim = solo();
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 40.0));
    let first = sim.node("oak", Vec2::new(49.0, 40.0), Some(2.0));
    let second = sim.node("oak", Vec2::new(52.0, 44.0), None);
    sim.order(P0, &[worker], CommandKind::Gather(first));
    sim.run_secs(12.0);
    assert!(!sim.alive(first));
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: second }));
}

/// A worker sent to a tree deep in a forest, with no open ground within
/// reach of it, takes a nearby tree it can walk up to instead; and when its
/// tree runs out it moves on to a reachable one, not to the enclosed one
/// that is closer.
#[test]
fn workers_pass_over_trees_they_cannot_reach() {
    let mut sim = solo();
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let enclosed = sim.node("oak", Vec2::new(50.5, 40.5), None);
    let open = sim.node("oak", Vec2::new(54.5, 46.5), None);
    // The forest round the enclosed tree, as the nav grid sees it.
    {
        let mut nav = sim.app.world_mut().resource_mut::<Nav>();
        for y in 37..44 {
            for x in 47..54 {
                nav.add_walk_blocks(&[IVec2::new(x, y)], 1);
            }
        }
    }
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 46.0));
    sim.order(P0, &[worker], CommandKind::Gather(enclosed));
    sim.ticks(3);
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: open }), "took the tree it can reach");

    // Its tree runs out: the enclosed one is nearer, the reachable one is next.
    let last = sim.node("oak", Vec2::new(57.5, 46.5), None);
    let field = sim.get::<ResourceNode>(open).unwrap();
    sim.set(open, ResourceNode { remaining: 1.0, ..field });
    sim.run_secs(20.0);
    assert!(!sim.alive(open));
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: last }));
}

/// Workers sent one by one to the same small tree (as the AI does) fill
/// the room round it and spill over to the nearest trees of the grove, so
/// a crowd does not jam the paths to a single tree.
#[test]
fn workers_sent_to_a_full_tree_spread_over_the_grove() {
    let mut sim = solo();
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let pines: Vec<Entity> = [(50.0, 40.0), (52.0, 43.0), (53.0, 38.0), (56.0, 41.0)].iter().map(|&(x, y)| sim.node("pine", Vec2::new(x, y), None)).collect();
    let workers: Vec<Entity> = (0..8).map(|i| sim.unit(P0, "peasant", Vec2::new(45.0, 36.0 + i as f32))).collect();
    for &w in &workers {
        sim.order(P0, &[w], CommandKind::Gather(pines[0]));
    }
    sim.ticks(3);
    let on = |sim: &Sim, node: Entity| workers.iter().filter(|w| sim.current_order(**w) == Some(Order::Gather { target: node })).count();
    let first = on(&sim, pines[0]);
    assert!((1..=4).contains(&first), "{first} workers on the first tree");
    assert_eq!(pines.iter().map(|p| on(&sim, *p)).sum::<usize>(), workers.len(), "everyone still gathers pine");
    assert!(on(&sim, pines[1]) + on(&sim, pines[2]) > 0, "the nearest trees took the rest");
}

#[test]
fn worker_gives_up_on_an_unreachable_tree_for_a_reachable_one() {
    let mut sim = solo();
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 40.0));
    let walled_in = sim.node("oak", Vec2::new(50.0, 40.0), None);
    let open = sim.node("oak", Vec2::new(50.0, 46.0), None);
    sim.make_unreachable(walled_in);
    sim.order(P0, &[worker], CommandKind::Gather(walled_in));
    sim.run_secs(2.0);
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: open }));
    sim.run_secs(5.0);
    assert!(sim.get::<Gatherer>(worker).unwrap().amount > 0.0, "working the reachable tree");
}

#[test]
fn worker_skips_two_unreachable_trees_for_a_reachable_one() {
    let mut sim = solo();
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 40.0));
    // Each walled-in tree is the other's nearest neighbour.
    let walled_in = [sim.node("oak", Vec2::new(50.0, 40.0), None), sim.node("oak", Vec2::new(50.0, 42.0), None)];
    let open = sim.node("oak", Vec2::new(50.0, 50.0), None);
    for tree in walled_in {
        sim.make_unreachable(tree);
    }
    sim.order(P0, &[worker], CommandKind::Gather(walled_in[0]));
    sim.run_secs(1.0);
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: open }));
    sim.run_secs(6.0);
    assert!(sim.get::<Gatherer>(worker).unwrap().amount > 0.0, "working the reachable tree");
}

#[test]
fn worker_delivers_elsewhere_when_the_nearest_drop_site_is_walled_off() {
    let mut sim = solo();
    let walled_off = sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    sim.building(P0, "town_center", Vec2::new(70.0, 40.0), true);
    sim.make_unreachable(walled_off);
    let worker = sim.unit(P0, "peasant", Vec2::new(51.5, 40.0));
    let oak = sim.node("oak", Vec2::new(53.0, 40.0), None);
    let load = sim.get::<Gatherer>(worker).unwrap();
    sim.set(worker, Gatherer { carrying: sim.data().resource_id("wood"), amount: load.capacity, ..load });
    let wood = sim.resource(P0, "wood");
    sim.order(P0, &[worker], CommandKind::Gather(oak));
    sim.run_secs(10.0);
    assert!(sim.resource(P0, "wood") >= wood + load.capacity - 0.01, "wood {} -> {}", wood, sim.resource(P0, "wood"));
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: oak }));
}

/// A worker that gave up on its tree with part of a load goes to unload it,
/// and gets every try at the drop site: the failures on the tree do not
/// count against it, so one blocked walk (a crowd at the town centre) does
/// not write off the only drop site.
#[test]
fn worker_leaving_an_unreachable_tree_gets_every_try_at_the_drop_site() {
    let mut sim = solo();
    let tc = sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(60.0, 40.0));
    let oak = sim.node("oak", Vec2::new(70.0, 40.0), None);
    sim.make_unreachable(oak);
    let load = sim.get::<Gatherer>(worker).unwrap();
    sim.set(worker, Gatherer { carrying: sim.data().resource_id("wood"), amount: 3.0, ..load });
    let wood = sim.resource(P0, "wood");
    sim.order(P0, &[worker], CommandKind::Gather(oak));
    let mut delivering = false;
    for _ in 0..300 {
        sim.ticks(1);
        if sim.get::<MoveGoal>(worker).is_some_and(|g| g.follow == Some(tc)) {
            delivering = true;
            break;
        }
    }
    assert!(delivering, "gave up on the tree and went to unload");
    sim.ticks(3);
    sim.fail_goal(worker);
    sim.ticks(3);
    assert!(!sim.noticed("No drop site for Wood"));
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: oak }));
    sim.run_secs(15.0);
    assert!(sim.resource(P0, "wood") >= wood + 2.99, "wood {} -> {}", wood, sim.resource(P0, "wood"));
}

/// A Gather starting on a node that is already empty (another worker took
/// the last of it earlier in the same tick; it is not removed until the
/// end of the tick) moves on to a similar node, as when the worker's own
/// node runs out.
#[test]
fn gather_starting_on_an_emptied_node_moves_on_to_a_similar_one() {
    let mut sim = solo();
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 40.0));
    let emptied = sim.node("oak", Vec2::new(49.0, 40.0), None);
    let next = sim.node("oak", Vec2::new(52.0, 44.0), None);
    let node = sim.get::<ResourceNode>(emptied).unwrap();
    sim.set(emptied, ResourceNode { remaining: 0.0, ..node });
    // What intake handed over before the node ran dry.
    sim.set(worker, Orders { queue: [Order::Gather { target: emptied }].into() });
    sim.ticks(2);
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: next }));
}

/// A worker sent back to its own tree after another order (shift-queued
/// behind a walk), when that tree was felled meanwhile, takes the next tree
/// of the grove instead of standing idle.
#[test]
fn worker_queued_back_on_its_felled_tree_takes_the_next_one() {
    let mut sim = solo();
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 40.0));
    let tree = sim.node("oak", Vec2::new(49.0, 40.0), None);
    let next = sim.node("oak", Vec2::new(52.0, 44.0), None);
    sim.order(P0, &[worker], CommandKind::Gather(tree));
    sim.run_secs(3.0);
    sim.order(P0, &[worker], CommandKind::Move(Vec2::new(46.0, 48.0)));
    sim.queue(P0, &[worker], CommandKind::Gather(tree));
    sim.ticks(2);
    // Another worker fells it while this one walks.
    sim.app.world_mut().despawn(tree);
    sim.run_secs(8.0);
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: next }));
    assert!(!sim.has::<Idle>(worker));
}

#[test]
fn smart_on_a_hostile_attacks_it() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(40.0, 40.0));
    let enemy = sim.unit(P1, "spearman", Vec2::new(45.0, 40.0));
    sim.order(P0, &[spear], CommandKind::Smart { point: Vec2::new(45.0, 40.0), target: Some(enemy) });
    sim.ticks(2);
    assert_eq!(sim.current_order(spear), Some(Order::Attack { target: enemy }));
    assert_eq!(sim.get::<AttackTarget>(spear), Some(AttackTarget { entity: enemy, forced: true }));
}

#[test]
fn smart_on_a_node_gathers_and_non_gatherers_walk_there() {
    let mut sim = solo();
    let worker = sim.unit(P0, "peasant", Vec2::new(40.0, 40.0));
    let soldier = sim.unit(P0, "spearman", Vec2::new(41.0, 40.0));
    let oak = sim.node("oak", Vec2::new(50.0, 40.0), None);
    sim.order(P0, &[worker, soldier], CommandKind::Smart { point: Vec2::new(50.0, 40.0), target: Some(oak) });
    sim.ticks(1);
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: oak }));
    assert!(matches!(sim.current_order(soldier), Some(Order::Move { .. })));
}

#[test]
fn smart_builds_foundations_repairs_buildings_and_follows_friends() {
    let mut sim = solo();
    let site = sim.building(P0, "house", Vec2::new(50.0, 40.0), false);
    let house = sim.building(P0, "house", Vec2::new(50.0, 52.0), true);
    let max = sim.get::<Health>(house).unwrap().max;
    sim.set(house, Health { hp: 100.0, max });
    let builder = sim.unit(P0, "peasant", Vec2::new(45.0, 40.0));
    let repairer = sim.unit(P0, "peasant", Vec2::new(45.0, 52.0));
    let soldier = sim.unit(P0, "spearman", Vec2::new(30.0, 30.0));
    let friend = sim.unit(P0, "spearman", Vec2::new(36.0, 30.0));
    sim.order(P0, &[builder], CommandKind::Smart { point: Vec2::new(50.0, 40.0), target: Some(site) });
    sim.order(P0, &[repairer], CommandKind::Smart { point: Vec2::new(50.0, 52.0), target: Some(house) });
    sim.order(P0, &[soldier], CommandKind::Smart { point: Vec2::new(36.0, 30.0), target: Some(friend) });
    sim.ticks(1);
    assert_eq!(sim.current_order(builder), Some(Order::Build { site }));
    assert_eq!(sim.current_order(repairer), Some(Order::Repair { target: house }));
    assert_eq!(sim.current_order(soldier), Some(Order::Follow { target: friend }));
    sim.run_secs(4.0);
    assert!(sim.get::<Health>(house).unwrap().hp > 105.0, "repaired at the peasant's rate");
    assert!(sim.gap(soldier, friend) <= 1.6, "follower caught up");
}

#[test]
fn worker_builds_a_foundation_to_full_progress() {
    let mut sim = solo();
    let site = sim.building(P0, "house", Vec2::new(50.0, 40.0), false);
    let worker = sim.unit(P0, "peasant", Vec2::new(44.0, 40.0));
    let hp0 = sim.get::<Health>(site).unwrap().hp;
    sim.order(P0, &[worker], CommandKind::Build(site));
    sim.run_secs(30.0);
    let progress = sim.get::<UnderConstruction>(site).unwrap().progress;
    assert!(progress >= 1.0, "progress {progress}");
    let hp = sim.get::<Health>(site).unwrap();
    assert!(hp.hp > hp0 && (hp.hp - hp.max).abs() < 1.0, "hp {hp:?}");
}

#[test]
fn builders_share_work_with_diminishing_returns() {
    let mut sim = solo();
    let pair_site = sim.building(P0, "house", Vec2::new(40.0, 40.0), false);
    let solo_site = sim.building(P0, "house", Vec2::new(60.0, 40.0), false);
    // Already standing next to their foundations (house is 4 m wide).
    let pair = [sim.unit(P0, "peasant", Vec2::new(42.9, 40.0)), sim.unit(P0, "peasant", Vec2::new(37.1, 40.0))];
    let lone = sim.unit(P0, "peasant", Vec2::new(62.9, 40.0));
    sim.order(P0, &pair, CommandKind::Build(pair_site));
    sim.order(P0, &[lone], CommandKind::Build(solo_site));
    sim.run_secs(3.0);
    let progress = |e| sim.get::<UnderConstruction>(e).unwrap().progress;
    let ratio = progress(pair_site) / progress(solo_site);
    assert!((ratio - 2f32.powf(0.8)).abs() < 0.05, "two builders are {ratio}x one");
}

#[test]
fn builder_of_a_new_drop_site_starts_gathering_what_it_takes() {
    let mut sim = solo();
    let camp = sim.building(P0, "lumber_camp", Vec2::new(50.0, 40.0), false);
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 40.0));
    // The bush is closer, but a lumber camp only takes wood.
    sim.node("berry_bush", Vec2::new(53.0, 37.0), None);
    let pine = sim.node("pine", Vec2::new(57.0, 40.0), None);
    sim.order(P0, &[worker], CommandKind::Build(camp));
    sim.run_secs(28.0);
    assert!(sim.get::<UnderConstruction>(camp).unwrap().progress >= 1.0);
    sim.finish(camp);
    sim.ticks(3);
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: pine }));
}

#[test]
fn engineer_finishing_a_mining_camp_starts_on_the_stone_beside_it() {
    // The engineer's and the camp's type indices lie beyond the node types.
    let mut sim = solo();
    let camp = sim.building(P0, "mining_camp", Vec2::new(50.0, 40.0), false);
    let engineer = sim.unit(P0, "engineer", Vec2::new(47.5, 40.0));
    let stone = sim.node("stone_outcrop", Vec2::new(55.0, 40.0), None);
    sim.order(P0, &[engineer], CommandKind::Build(camp));
    sim.ticks(3);
    sim.finish(camp);
    sim.ticks(3);
    assert_eq!(sim.current_order(engineer), Some(Order::Gather { target: stone }));
}

#[test]
fn builder_of_a_faction_drop_site_mines_and_one_of_a_stable_goes_idle() {
    let mut sim = solo();
    let mine = sim.building(P0, "ironhold_deep_mine", Vec2::new(40.0, 40.0), false);
    let stable = sim.building(P0, "stable", Vec2::new(70.0, 40.0), false);
    let iron = sim.node("iron_vein", Vec2::new(40.0, 45.0), None);
    let miner = sim.unit(P0, "peasant", Vec2::new(37.0, 40.0));
    let groom = sim.unit(P0, "peasant", Vec2::new(65.0, 40.0));
    sim.order(P0, &[miner], CommandKind::Build(mine));
    sim.order(P0, &[groom], CommandKind::Build(stable));
    sim.ticks(3);
    sim.finish(mine);
    sim.finish(stable);
    sim.ticks(3);
    assert_eq!(sim.current_order(miner), Some(Order::Gather { target: iron }));
    assert_eq!(sim.current_order(groom), None);
    assert!(sim.has::<Idle>(groom));
}

#[test]
fn unreachable_farm_foundation_ends_the_gather_order() {
    let mut sim = solo();
    let farm = sim.building(P0, "farm", Vec2::new(55.0, 40.0), false);
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 40.0));
    sim.make_unreachable(farm);
    sim.order(P0, &[worker], CommandKind::Gather(farm));
    sim.run_secs(2.0);
    assert_eq!(sim.current_order(worker), None, "does not loop between building and gathering");
    assert!(sim.has::<Idle>(worker));
}

#[test]
fn attack_order_closes_into_range_and_stops_to_shoot() {
    let mut sim = duel();
    let archer = sim.unit(P0, "archer", Vec2::new(30.0, 64.0));
    let enemy = sim.unit(P1, "spearman", Vec2::new(60.0, 64.0));
    sim.set(enemy, Stance::Passive);
    sim.order(P0, &[archer], CommandKind::Attack(enemy));
    sim.run_secs(12.0);
    let (stats, weapons) = (sim.get::<Stats>(archer).unwrap(), sim.get::<Weapons>(archer).unwrap());
    let range = stats::weapon_range(&sim.data(), &stats, weapons.slots[0].weapon);
    let gap = sim.gap(archer, enemy);
    assert!(gap <= range && gap > range * 0.8, "gap {gap} range {range}");
    assert_eq!(sim.get::<AttackTarget>(archer), Some(AttackTarget { entity: enemy, forced: true }));
    assert!(!sim.has::<MoveGoal>(archer), "stands still once in range");
}

/// Can one of `unit`'s weapons fire at `target`, by af_combat's rules?
fn can_fire(sim: &Sim, unit: Entity, target: Entity) -> bool {
    let data = sim.data();
    let (stats, weapons) = (sim.get::<Stats>(unit).unwrap(), sim.get::<Weapons>(unit).unwrap());
    let obb = |e| sim.get::<Footprint>(e).unwrap().obb(sim.pos(e), sim.get::<Facing>(e).unwrap().0);
    weapons.slots.iter().any(|s| in_weapon_range(&data, &stats, s.weapon, &obb(unit), &obb(target)))
}

#[test]
fn range_teched_engine_stops_where_af_combat_lets_it_fire() {
    let mut sim = Sim::new(&[("dune", 0), ("ironhold", 1)]);
    // +1 range for Mechanical units; the engine carries a Spray and a Nova.
    sim.research(P0, "targeting_computers");
    let engine = sim.unit(P0, "dune_sand_engine", Vec2::new(30.0, 64.0));
    let enemy = sim.unit(P1, "spearman", Vec2::new(60.0, 64.0));
    sim.set(enemy, Stance::Passive);
    assert_eq!(sim.get::<Stats>(engine).unwrap().range_add, 1.0);
    sim.order(P0, &[engine], CommandKind::Attack(enemy));
    sim.run_secs(20.0);
    assert!(can_fire(&sim, engine, enemy), "stopped at gap {}", sim.gap(engine, enemy));
    assert!(!sim.has::<MoveGoal>(engine), "stands to fire");
}

#[test]
fn melee_unit_closes_on_a_building_corner_until_it_can_strike() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(40.0, 40.0));
    // Approached diagonally, at its corner, where the box gap reads short.
    let house = sim.building(P1, "house", Vec2::new(50.0, 50.0), true);
    sim.order(P0, &[spear], CommandKind::Attack(house));
    sim.run_secs(8.0);
    assert!(can_fire(&sim, spear, house), "stopped at box gap {}", sim.gap(spear, house));
    assert!(!sim.has::<MoveGoal>(spear), "stands to strike");
}

#[test]
fn mangonel_backs_out_of_its_minimum_range_to_fire() {
    let mut sim = duel();
    let mangonel = sim.unit(P0, "mangonel", Vec2::new(40.0, 64.0));
    // 1 m away: inside the 3 m minimum range.
    let enemy = sim.unit(P1, "spearman", Vec2::new(42.25, 64.0));
    sim.set(enemy, Stance::Passive);
    assert!(!can_fire(&sim, mangonel, enemy));
    sim.order(P0, &[mangonel], CommandKind::Attack(enemy));
    sim.run_secs(5.0);
    assert!(can_fire(&sim, mangonel, enemy), "gap {}", sim.gap(mangonel, enemy));
    assert!(!sim.has::<MoveGoal>(mangonel), "stands to fire");
    assert_eq!(sim.get::<AttackTarget>(mangonel), Some(AttackTarget { entity: enemy, forced: true }));
}

#[test]
fn holding_mangonel_picks_the_enemy_it_can_fire_at() {
    let mut sim = duel();
    let mangonel = sim.unit(P0, "mangonel", Vec2::new(40.0, 64.0));
    let too_close = sim.unit(P1, "spearman", Vec2::new(42.25, 64.0));
    let in_band = sim.unit(P1, "spearman", Vec2::new(40.0, 73.25));
    sim.order(P0, &[mangonel], CommandKind::Hold);
    sim.run_secs(1.0);
    assert!(!can_fire(&sim, mangonel, too_close) && can_fire(&sim, mangonel, in_band));
    assert_eq!(sim.get::<AttackTarget>(mangonel).map(|a| a.entity), Some(in_band));
    assert_eq!(sim.pos(mangonel), Vec2::new(40.0, 64.0));
}

#[test]
fn melee_chaser_closes_in_again_when_the_target_steps_just_out_of_reach() {
    let mut sim = duel();
    let soldier = sim.unit(P0, "man_at_arms", Vec2::new(40.0, 64.0));
    let enemy = sim.unit(P1, "spearman", Vec2::new(46.0, 64.0));
    sim.set(enemy, Stance::Passive);
    sim.order(P0, &[soldier], CommandKind::Attack(enemy));
    sim.run_secs(0.5);
    let goal = sim.get::<MoveGoal>(soldier).expect("chasing");
    assert_eq!(goal.follow, Some(enemy));
    // af_movement reported the chase arrived, then the target ended up
    // 0.1 m beyond the sword's reach (it stepped back, or was shoved).
    let (stats, weapons) = (sim.get::<Stats>(soldier).unwrap(), sim.get::<Weapons>(soldier).unwrap());
    let reach = weapon_reach(&sim.data(), &stats, weapons.slots[0].weapon);
    let step = sim.gap(soldier, enemy) - (reach + 0.1);
    sim.set(enemy, Position(sim.pos(enemy) - Vec2::new(step, 0.0)));
    sim.set(soldier, PathState { status: PathStatus::Arrived, goal_seq: goal.seq, ..default() });
    sim.run_secs(1.0);
    assert!(sim.gap(soldier, enemy) <= reach, "stalled at gap {} (reach {reach})", sim.gap(soldier, enemy));
}

/// An Attack gives up after a few path failures in a row, not after a few
/// over the whole fight: a chaser that got to its target and struck it
/// between failures (af_movement gave up in a crowd, then the target
/// stepped away again) keeps its order.
#[test]
fn attack_order_survives_chase_failures_that_are_not_in_a_row() {
    let mut sim = duel();
    let soldier = sim.unit(P0, "man_at_arms", Vec2::new(40.0, 64.0));
    let enemy = sim.unit(P1, "spearman", Vec2::new(46.0, 64.0));
    sim.set(enemy, Stance::Passive);
    sim.order(P0, &[soldier], CommandKind::Attack(enemy));
    let (stats, weapons) = (sim.get::<Stats>(soldier).unwrap(), sim.get::<Weapons>(soldier).unwrap());
    let reach = weapon_reach(&sim.data(), &stats, weapons.slots[0].weapon);
    for round in 1..=6 {
        // The target steps away; the chase fails once, is retried, and gets there.
        sim.set(enemy, Position(sim.pos(soldier) + Vec2::new(6.0, 0.0)));
        sim.ticks(2);
        assert_eq!(sim.get::<MoveGoal>(soldier).and_then(|g| g.follow), Some(enemy), "round {round}: chasing");
        sim.fail_goal(soldier);
        sim.ticks(1);
        assert_eq!(sim.current_order(soldier), Some(Order::Attack { target: enemy }), "round {round}: order dropped");
        sim.run_secs(2.0);
        assert!(sim.gap(soldier, enemy) <= reach, "round {round}: gap {} (reach {reach})", sim.gap(soldier, enemy));
        assert_eq!(sim.get::<AttackTarget>(soldier), Some(AttackTarget { entity: enemy, forced: true }));
    }
}

#[test]
fn ground_only_weapons_refuse_a_flyer() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(40.0, 40.0));
    let balloon = sim.unit(P1, "war_balloon", Vec2::new(45.0, 40.0));
    sim.order(P0, &[spear], CommandKind::Attack(balloon));
    sim.ticks(2);
    assert_eq!(sim.current_order(spear), None);
    assert!(sim.noticed("Cannot attack that target"));
}

#[test]
fn group_move_spreads_the_group_and_everyone_arrives() {
    let mut sim = solo();
    let squad: Vec<Entity> = (0..9).map(|i| sim.unit(P0, "spearman", Vec2::new(20.0 + i as f32, 20.0))).collect();
    let dest = Vec2::new(80.0, 80.0);
    sim.order(P0, &squad, CommandKind::Move(dest));
    sim.ticks(2);
    let spots: Vec<Vec2> = squad
        .iter()
        .map(|&e| match sim.current_order(e) {
            Some(Order::Move { target }) => target,
            other => panic!("{other:?}"),
        })
        .collect();
    for (i, a) in spots.iter().enumerate() {
        for b in &spots[i + 1..] {
            assert!(a.distance(*b) >= 1.0, "spots {a} and {b} too close");
        }
    }
    let centre = spots.iter().sum::<Vec2>() / spots.len() as f32;
    assert!(centre.distance(dest) < 1.5, "formation centred at {centre}");
    sim.run_secs(35.0);
    for (&e, spot) in squad.iter().zip(&spots) {
        assert!(sim.pos(e).distance(*spot) <= 0.5);
        assert_eq!(sim.current_order(e), None);
        assert!(sim.has::<Idle>(e));
    }
}

#[test]
fn queued_orders_run_in_sequence() {
    let mut sim = solo();
    let unit = sim.unit(P0, "spearman", Vec2::new(20.0, 20.0));
    let (a, b) = (Vec2::new(30.0, 20.0), Vec2::new(30.0, 30.0));
    sim.order(P0, &[unit], CommandKind::Move(a));
    sim.queue(P0, &[unit], CommandKind::Move(b));
    sim.run_secs(2.0);
    assert_eq!(sim.current_order(unit), Some(Order::Move { target: a }));
    sim.run_secs(6.0);
    assert!(sim.pos(unit).distance(b) <= 0.5);
    assert_eq!(sim.current_order(unit), None);
}

#[test]
fn stop_drops_orders_and_movement() {
    let mut sim = solo();
    let unit = sim.unit(P0, "spearman", Vec2::new(20.0, 20.0));
    sim.order(P0, &[unit], CommandKind::Move(Vec2::new(60.0, 20.0)));
    sim.run_secs(1.0);
    sim.order(P0, &[unit], CommandKind::Stop);
    sim.ticks(2);
    let here = sim.pos(unit);
    sim.run_secs(1.0);
    assert_eq!(sim.current_order(unit), None);
    assert!(!sim.has::<MoveGoal>(unit));
    assert_eq!(sim.pos(unit), here);
}

#[test]
fn idle_aggressive_unit_engages_but_a_passive_worker_does_not() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(50.0, 50.0));
    let worker = sim.unit(P0, "peasant", Vec2::new(50.0, 53.0));
    let enemy = sim.unit(P1, "peasant", Vec2::new(56.0, 50.0));
    sim.run_secs(1.0);
    assert_eq!(sim.get::<AttackTarget>(spear), Some(AttackTarget { entity: enemy, forced: false }));
    assert!(!sim.has::<Idle>(spear), "busy fighting");
    assert!(sim.pos(spear).x > 50.0, "chasing");
    assert!(sim.get::<AttackTarget>(worker).is_none());
    assert!(sim.has::<Idle>(worker));
}

#[test]
fn hold_never_moves_and_only_shoots_what_is_in_range() {
    let mut sim = duel();
    let archer = sim.unit(P0, "archer", Vec2::new(50.0, 50.0));
    let near = sim.unit(P1, "peasant", Vec2::new(55.0, 50.0));
    let far = sim.unit(P1, "peasant", Vec2::new(50.0, 62.0));
    sim.order(P0, &[archer], CommandKind::Hold);
    sim.run_secs(1.0);
    assert_eq!(sim.get::<AttackTarget>(archer).map(|a| a.entity), Some(near));
    sim.app.world_mut().despawn(near);
    sim.run_secs(1.0);
    assert_eq!(sim.get::<AttackTarget>(archer), None, "{far} is out of range");
    assert_eq!(sim.pos(archer), Vec2::new(50.0, 50.0));
}

#[test]
fn attack_move_engages_on_the_way_and_resumes() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(20.0, 64.0));
    let enemy = sim.unit(P1, "peasant", Vec2::new(40.0, 66.0));
    let dest = Vec2::new(70.0, 64.0);
    sim.order(P0, &[spear], CommandKind::AttackMove(dest));
    sim.run_secs(6.0);
    assert_eq!(sim.get::<AttackTarget>(spear), Some(AttackTarget { entity: enemy, forced: false }));
    // af_combat would kill it; the unit then carries on to its destination.
    sim.app.world_mut().despawn(enemy);
    sim.run_secs(15.0);
    assert!(sim.pos(spear).distance(dest) <= 0.5, "at {}", sim.pos(spear));
    assert_eq!(sim.current_order(spear), None);
}

#[test]
fn patrol_walks_back_and_forth() {
    let mut sim = solo();
    let unit = sim.unit(P0, "spearman", Vec2::new(20.0, 20.0));
    sim.order(P0, &[unit], CommandKind::Patrol(Vec2::new(30.0, 20.0)));
    sim.run_secs(3.0);
    let outbound = sim.pos(unit).x;
    sim.run_secs(2.0);
    let turned = sim.pos(unit).x;
    assert!(outbound > 25.0 && turned < outbound, "out to {outbound}, back to {turned}");
    sim.run_secs(2.0);
    assert!(sim.pos(unit).x < 22.0, "back at the start");
    assert!(matches!(sim.current_order(unit), Some(Order::Patrol { .. })));
}

/// Patrol points shift-clicked after the first make one route, walked in
/// order and from the last straight back to the start, round and round;
/// an order queued after the patrols is never reached.
#[test]
fn shift_queued_patrol_points_walk_one_loop() {
    let mut sim = solo();
    let corners = [Vec2::new(20.0, 20.0), Vec2::new(30.0, 20.0), Vec2::new(30.0, 30.0)];
    let unit = sim.unit(P0, "spearman", corners[0]);
    sim.order(P0, &[unit], CommandKind::Patrol(corners[1]));
    sim.queue(P0, &[unit], CommandKind::Patrol(corners[2]));
    sim.queue(P0, &[unit], CommandKind::Move(Vec2::new(10.0, 40.0)));
    // The corners in the order they are reached (each visit once).
    let mut visits = vec![0];
    for _ in 0..(40.0 * SIM_HZ as f32) as u32 {
        sim.ticks(1);
        let at = corners.iter().position(|c| sim.pos(unit).distance(*c) < 0.6);
        if let Some(i) = at.filter(|i| visits.last() != Some(i)) {
            visits.push(i);
        }
    }
    assert!(visits.len() >= 7, "went round at least twice: {visits:?}");
    for (i, corner) in visits.iter().enumerate() {
        assert_eq!(*corner, i % 3, "a -> b -> c -> a ...: {visits:?}");
    }
    let orders = sim.get::<Orders>(unit).unwrap();
    assert_eq!(orders.queue.len(), 3, "patrols never pop, so the move waits forever");
    assert_eq!(af_core::ext::behavior::patrol_points(&orders.queue).collect::<Vec<_>>(), corners.to_vec());
}

/// A point shift-queued onto a running patrol joins the loop without
/// turning the unit round: walking back from b to a, it carries on to a,
/// then goes a -> b -> c -> a.
#[test]
fn a_point_queued_onto_a_running_patrol_keeps_the_unit_on_its_leg() {
    let mut sim = solo();
    let corners = [Vec2::new(20.0, 20.0), Vec2::new(30.0, 20.0), Vec2::new(30.0, 30.0)];
    let unit = sim.unit(P0, "spearman", corners[0]);
    sim.order(P0, &[unit], CommandKind::Patrol(corners[1]));
    // Out to b, then part of the way back.
    let mut at_b = false;
    for _ in 0..(20.0 * SIM_HZ as f32) as u32 {
        sim.ticks(1);
        at_b |= sim.pos(unit).distance(corners[1]) < 0.6;
        if at_b && sim.pos(unit).x < 26.0 {
            break;
        }
    }
    assert!(at_b && sim.pos(unit).x < 26.0, "walking back to a: at {}", sim.pos(unit));
    sim.queue(P0, &[unit], CommandKind::Patrol(corners[2]));
    let mut visits = Vec::new();
    for _ in 0..(30.0 * SIM_HZ as f32) as u32 {
        sim.ticks(1);
        let at = corners.iter().position(|c| sim.pos(unit).distance(*c) < 0.6);
        if let Some(i) = at.filter(|i| visits.last() != Some(i)) {
            visits.push(i);
        }
    }
    assert!(visits.len() >= 4, "went round: {visits:?}");
    for (i, corner) in visits.iter().enumerate() {
        assert_eq!(*corner, i % 3, "on to a first, then a -> b -> c -> a: {visits:?}");
    }
}

#[test]
fn defensive_unit_gives_up_a_long_chase_and_returns_to_its_post() {
    let mut sim = duel();
    let post = Vec2::new(50.0, 50.0);
    let guard = sim.unit(P0, "spearman", post);
    sim.set(guard, Stance::Defensive);
    let enemy = sim.unit(P1, "peasant", Vec2::new(57.0, 50.0));
    sim.run_secs(1.0);
    assert_eq!(sim.get::<AttackTarget>(guard).map(|a| a.entity), Some(enemy));
    // The enemy walks away a little slower than the guard can run.
    let mut farthest: f32 = 0.0;
    for _ in 0..300 {
        let p = sim.pos(enemy);
        sim.set(enemy, Position(p + Vec2::new(2.5 * SIM_DT, 0.0)));
        sim.ticks(1);
        farthest = farthest.max(sim.pos(guard).distance(post));
    }
    assert!(farthest > 10.0, "chased out to {farthest}");
    sim.run_secs(8.0);
    assert!(sim.pos(guard).distance(post) <= 1.5, "back at {}", sim.pos(guard));
    assert_eq!(sim.get::<AttackTarget>(guard), None);
}

#[test]
fn idle_unit_hits_back_at_an_attacker_beyond_its_sight() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(50.0, 50.0));
    // 11 m away: outside the spearman's 9 m sight.
    let archer = sim.unit(P1, "archer", Vec2::new(50.0, 61.0));
    sim.set(archer, Stance::Passive);
    sim.set(spear, LastAttacker { entity: Some(archer), player: Some(PlayerId(P1)), pos: Vec2::new(50.0, 61.0), time: 0.0 });
    sim.run_secs(0.6);
    assert_eq!(sim.get::<AttackTarget>(spear), Some(AttackTarget { entity: archer, forced: false }));
    assert!(sim.pos(spear).y > 50.0, "charging the archer");
}

#[test]
fn attack_move_passes_an_enemy_it_cannot_reach() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(20.0, 64.0));
    let on_a_cliff = sim.unit(P1, "peasant", Vec2::new(40.0, 67.0));
    sim.make_unreachable(on_a_cliff);
    let dest = Vec2::new(60.0, 64.0);
    sim.order(P0, &[spear], CommandKind::AttackMove(dest));
    sim.run_secs(20.0);
    assert!(sim.pos(spear).distance(dest) <= 0.5, "stuck at {}", sim.pos(spear));
    assert_eq!(sim.current_order(spear), None);
}

#[test]
fn attack_move_does_not_cycle_between_two_enemies_it_cannot_reach() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(20.0, 64.0));
    let cliffs = [sim.unit(P1, "peasant", Vec2::new(27.0, 67.0)), sim.unit(P1, "peasant", Vec2::new(28.0, 61.0))];
    for e in cliffs {
        sim.make_unreachable(e);
    }
    let dest = Vec2::new(60.0, 64.0);
    sim.order(P0, &[spear], CommandKind::AttackMove(dest));
    // Every chase is a path request. Each enemy is tried once, then left
    // alone for a while.
    let mut chases = BTreeSet::new();
    for _ in 0..120 {
        sim.ticks(1);
        if let Some(goal) = sim.get::<MoveGoal>(spear).filter(|g| g.follow.is_some_and(|f| cliffs.contains(&f))) {
            chases.insert(goal.seq);
        }
    }
    assert_eq!(chases.len(), 2, "chase attempts");
    sim.run_secs(20.0);
    assert!(sim.pos(spear).distance(dest) <= 0.5, "stuck at {}", sim.pos(spear));
    assert_eq!(sim.current_order(spear), None);
}

#[test]
fn idle_unit_walking_to_its_rally_point_hits_back() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(50.0, 50.0));
    let archer = sim.unit(P1, "archer", Vec2::new(56.0, 50.0));
    sim.set(archer, Stance::Passive);
    // af_economy sends a fresh unit to its rally point with a bare goal.
    sim.set(spear, MoveGoal::point(Vec2::new(50.0, 20.0), 0.35));
    sim.set(spear, LastAttacker { entity: Some(archer), player: Some(PlayerId(P1)), pos: Vec2::new(56.0, 50.0), time: 0.0 });
    sim.run_secs(0.6);
    assert_eq!(sim.get::<AttackTarget>(spear), Some(AttackTarget { entity: archer, forced: false }));
    assert_eq!(sim.get::<MoveGoal>(spear).and_then(|g| g.follow), Some(archer), "charging instead of walking on");
}

#[test]
fn idle_unit_prefers_a_harmless_unit_to_a_half_built_tower() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(50.0, 50.0));
    // A tower foundation 2 m away, which does not shoot yet; a monk 7 m away.
    sim.building(P1, "watchtower", Vec2::new(54.1, 50.0), false);
    let monk = sim.unit(P1, "monk", Vec2::new(50.0, 42.35));
    sim.run_secs(0.6);
    assert_eq!(sim.get::<AttackTarget>(spear).map(|a| a.entity), Some(monk));
}

#[test]
fn reissued_attack_move_keeps_the_fight_going() {
    let mut sim = duel();
    let spear = sim.unit(P0, "spearman", Vec2::new(30.0, 64.0));
    let enemy = sim.unit(P1, "peasant", Vec2::new(36.0, 64.0));
    let dest = Vec2::new(90.0, 64.0);
    sim.order(P0, &[spear], CommandKind::AttackMove(dest));
    sim.run_secs(1.0);
    let goal = sim.get::<MoveGoal>(spear).expect("chasing");
    assert_eq!(goal.follow, Some(enemy));
    // An AI repeating its command every half second must not reset the chase.
    sim.order(P0, &[spear], CommandKind::AttackMove(dest + Vec2::new(0.5, 0.0)));
    sim.ticks(1);
    assert_eq!(sim.get::<AttackTarget>(spear).map(|a| a.entity), Some(enemy));
    assert_eq!(sim.get::<MoveGoal>(spear).map(|g| g.seq), Some(goal.seq), "no repath");
}

#[test]
fn gather_order_on_a_non_gatherer_becomes_a_walk_to_the_source() {
    let mut sim = solo();
    let soldier = sim.unit(P0, "spearman", Vec2::new(40.0, 40.0));
    let oak = sim.node("oak", Vec2::new(50.0, 40.0), None);
    // As another crate might order it (a rally point on a tree).
    sim.set(soldier, Orders { queue: [Order::Gather { target: oak }].into() });
    sim.ticks(3);
    assert_eq!(sim.current_order(soldier), Some(Order::Move { target: Vec2::new(50.0, 40.0) }));
}

#[test]
fn healer_walks_to_an_injured_ally_and_heals_it() {
    let mut sim = solo();
    let monk = sim.unit(P0, "monk", Vec2::new(40.0, 40.0));
    let hurt = sim.unit(P0, "spearman", Vec2::new(49.0, 40.0));
    let max = sim.get::<Health>(hurt).unwrap().max;
    sim.set(hurt, Health { hp: 10.0, max });
    sim.run_secs(5.0);
    let hp = sim.get::<Health>(hurt).unwrap().hp;
    assert!(hp > 15.0, "hp {hp}");
    assert!(sim.gap(monk, hurt) <= 6.0);
}

#[test]
fn healer_leaves_a_patient_it_cannot_reach_for_one_it_can() {
    let mut sim = solo();
    let monk = sim.unit(P0, "monk", Vec2::new(40.0, 40.0));
    // Both in sight, both beyond the 6 m healing reach.
    let stranded = sim.unit(P0, "spearman", Vec2::new(50.0, 40.0));
    let reachable = sim.unit(P0, "spearman", Vec2::new(40.0, 49.0));
    let max = sim.get::<Health>(stranded).unwrap().max;
    // The stranded one is worse off, so it is picked first.
    sim.set(stranded, Health { hp: 5.0, max });
    sim.set(reachable, Health { hp: 10.0, max });
    sim.make_unreachable(stranded);
    sim.run_secs(5.0);
    assert!(sim.get::<Health>(reachable).unwrap().hp > 15.0, "hp {:?}", sim.get::<Health>(reachable));
    assert!(sim.gap(monk, reachable) <= 6.0);
}

fn wound(sim: &mut Sim, e: Entity, hp: f32) {
    let max = sim.get::<Health>(e).unwrap().max;
    sim.set(e, Health { hp, max });
}

fn hp(sim: &Sim, e: Entity) -> f32 {
    sim.get::<Health>(e).unwrap().hp
}

/// A healer attack-moving with the army (the only way the AI moves one)
/// mends the allies in reach on the way, and keeps its order.
#[test]
fn attack_moving_healer_mends_allies_on_the_march() {
    let mut sim = solo();
    let monk = sim.unit(P0, "monk", Vec2::new(40.0, 40.0));
    let spear = sim.unit(P0, "spearman", Vec2::new(41.5, 40.0));
    wound(&mut sim, spear, 10.0);
    sim.order(P0, &[monk, spear], CommandKind::AttackMove(Vec2::new(100.0, 40.0)));
    sim.run_secs(4.0);
    assert!(hp(&sim, spear) > 18.0, "hp {}", hp(&sim, spear));
    assert!(matches!(sim.current_order(monk), Some(Order::AttackMove { .. })));
    assert!(sim.pos(monk).x > 45.0, "still marching, at {}", sim.pos(monk));
}

/// A healer attack-moved into a fight stays with a wounded ally that stands
/// fighting and mends it, instead of walking on alone to the destination.
#[test]
fn attack_moving_healer_stays_to_mend_an_ally_in_a_fight() {
    let mut sim = duel();
    let monk = sim.unit(P0, "monk", Vec2::new(40.0, 40.0));
    let spear = sim.unit(P0, "spearman", Vec2::new(40.0, 42.0));
    let enemy = sim.unit(P1, "spearman", Vec2::new(50.0, 41.0));
    sim.set(enemy, Stance::Passive);
    wound(&mut sim, spear, 10.0);
    sim.order(P0, &[monk, spear], CommandKind::AttackMove(Vec2::new(100.0, 40.0)));
    sim.run_secs(10.0);
    assert_eq!(sim.get::<AttackTarget>(spear).map(|a| a.entity), Some(enemy), "the spearman fights");
    assert!(sim.gap(monk, spear) <= 6.0, "the monk walked on to {}", sim.pos(monk));
    assert!(hp(&sim, spear) > 30.0, "hp {}", hp(&sim, spear));
    assert!(matches!(sim.current_order(monk), Some(Order::AttackMove { .. })));
}

#[test]
fn patrolling_healer_mends_an_ally_on_its_beat() {
    let mut sim = solo();
    let monk = sim.unit(P0, "monk", Vec2::new(40.0, 40.0));
    let guard = sim.unit(P0, "spearman", Vec2::new(42.0, 42.0));
    wound(&mut sim, guard, 10.0);
    sim.order(P0, &[monk], CommandKind::Patrol(Vec2::new(46.0, 40.0)));
    sim.run_secs(6.0);
    assert!(hp(&sim, guard) > 20.0, "hp {}", hp(&sim, guard));
    assert!(matches!(sim.current_order(monk), Some(Order::Patrol { .. })));
}

/// A healer that also fights mends an ally in reach while it attacks.
#[test]
fn armed_healer_mends_an_ally_in_reach_while_attacking() {
    let mut sim = Sim::new(&[("sylvan", 0), ("dune", 1)]);
    let druid = sim.unit(P0, "sylvan_druid", Vec2::new(40.0, 40.0));
    let ally = sim.unit(P0, "spearman", Vec2::new(41.5, 40.0));
    let enemy = sim.unit(P1, "spearman", Vec2::new(40.0, 45.0));
    sim.set(enemy, Stance::Passive);
    sim.set(ally, Stance::Passive);
    wound(&mut sim, ally, 10.0);
    sim.order(P0, &[druid], CommandKind::Attack(enemy));
    sim.run_secs(3.0);
    assert_eq!(sim.get::<AttackTarget>(druid), Some(AttackTarget { entity: enemy, forced: true }));
    assert!(hp(&sim, ally) > 18.0, "hp {}", hp(&sim, ally));
}

#[test]
fn farm_takes_one_worker_and_is_destroyed_when_empty() {
    let mut sim = solo();
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let farm = sim.building(P0, "farm", Vec2::new(52.0, 40.0), true);
    let field = sim.get::<ResourceField>(farm).unwrap();
    sim.set(farm, ResourceField { remaining: 2.0, ..field });
    let workers = [sim.unit(P0, "peasant", Vec2::new(47.0, 39.0)), sim.unit(P0, "peasant", Vec2::new(47.0, 41.0))];
    let food = sim.resource(P0, "food");
    sim.order(P0, &workers, CommandKind::Gather(farm));
    sim.ticks(3);
    assert_eq!(sim.get::<ResourceField>(farm).unwrap().users, 1);
    let farming = workers.iter().filter(|&&w| sim.current_order(w) == Some(Order::Gather { target: farm })).count();
    assert_eq!(farming, 1, "one slot");
    sim.run_secs(12.0);
    assert!(!sim.alive(farm));
    assert!((sim.resource(P0, "food") - food - 2.0).abs() < 0.01);
    assert!(sim.noticed("Farm exhausted"));
}

/// A harbour is a field and also a drop site, barracks and academy: when its
/// fishing runs out it stays standing and keeps taking deposits, it only
/// stops being gathered.
#[test]
fn an_exhausted_harbour_stays_a_drop_site() {
    let mut sim = Sim::new(&[("tide", 0)]);
    let harbour = sim.building(P0, "tide_harbor", Vec2::new(52.0, 40.0), true);
    let field = sim.get::<ResourceField>(harbour).unwrap();
    sim.set(harbour, ResourceField { remaining: 2.0, ..field });
    let fisher = sim.unit(P0, "peasant", Vec2::new(45.0, 40.0));
    let food = sim.resource(P0, "food");
    sim.order(P0, &[fisher], CommandKind::Gather(harbour));
    sim.run_secs(12.0);
    assert!(sim.alive(harbour), "the harbour survives its field");
    assert!(!sim.has::<ResourceField>(harbour), "and is no longer gathered");
    assert!(sim.has::<DropSite>(harbour) && sim.has::<ProductionQueue>(harbour));
    assert!((sim.resource(P0, "food") - food - 2.0).abs() < 0.01, "the last catch was landed");
    assert!(sim.noticed("Harbour: Food exhausted"));
    assert_eq!(sim.current_order(fisher), None, "nothing else to fish");

    // Still a drop site: a woodcutter unloads there.
    let oak = sim.node("oak", Vec2::new(60.0, 40.0), None);
    let cutter = sim.unit(P0, "peasant", Vec2::new(62.0, 42.0));
    let wood = sim.resource(P0, "wood");
    sim.order(P0, &[cutter], CommandKind::Gather(oak));
    sim.run_secs(30.0);
    assert!(sim.resource(P0, "wood") >= wood + 9.99, "wood {wood} -> {}", sim.resource(P0, "wood"));
}

#[test]
fn delete_kills_own_units_and_buildings_only() {
    let mut sim = duel();
    let mine = sim.unit(P0, "spearman", Vec2::new(40.0, 40.0));
    let house = sim.building(P0, "house", Vec2::new(50.0, 40.0), true);
    let theirs = sim.unit(P1, "spearman", Vec2::new(60.0, 40.0));
    sim.order(P0, &[mine, house, theirs], CommandKind::Delete);
    sim.ticks(2);
    assert!(!sim.alive(mine));
    assert!(!sim.alive(house));
    assert!(sim.alive(theirs));
}
