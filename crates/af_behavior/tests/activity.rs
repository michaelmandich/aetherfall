//! The shared `Activity` tracks what each unit is doing, tick by tick.

#[allow(dead_code)] // each test binary uses only part of the harness
mod common;

use af_core::prelude::*;
use bevy::prelude::*;
use common::Sim;

const P0: u8 = 0;
const P1: u8 = 1;

/// Run tick by tick until `done` holds (at most `secs`); returns whether it did.
fn run_until(sim: &mut Sim, secs: f32, mut done: impl FnMut(&Sim) -> bool) -> bool {
    for _ in 0..(secs * SIM_HZ as f32) as u32 {
        sim.ticks(1);
        if done(sim) {
            return true;
        }
    }
    false
}

fn activity(sim: &Sim, e: Entity) -> Activity {
    sim.get::<Activity>(e).expect("every unit has an Activity")
}

#[test]
fn units_spawn_idle() {
    let mut sim = Sim::new(&[("ironhold", 0)]);
    let worker = sim.unit(P0, "peasant", Vec2::new(40.0, 40.0));
    let soldier = sim.unit(P0, "spearman", Vec2::new(44.0, 40.0));
    sim.ticks(1);
    assert_eq!(activity(&sim, worker), Activity::Idle);
    assert_eq!(activity(&sim, soldier), Activity::Idle);
}

/// Idle on the way to the tree, chopping at it, idle again carrying the
/// load home, and chopping once back.
#[test]
fn a_woodcutter_chops_only_at_the_tree() {
    let mut sim = Sim::new(&[("ironhold", 0)]);
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let worker = sim.unit(P0, "peasant", Vec2::new(46.0, 40.0));
    let oak = sim.node("oak", Vec2::new(54.0, 40.0), None);
    sim.order(P0, &[worker], CommandKind::Gather(oak));
    sim.ticks(3);
    assert_eq!(sim.current_order(worker), Some(Order::Gather { target: oak }));
    assert_eq!(activity(&sim, worker), Activity::Idle, "walking to the tree");

    assert!(run_until(&mut sim, 10.0, |s| activity(s, worker) == Activity::Gather(WorkStyle::Chop)), "chops at the tree");
    assert!(sim.gap(worker, oak) <= 1.0, "at the tree: gap {}", sim.gap(worker, oak));
    // Chopping lasts until the load is full, then it is carried home.
    assert!(run_until(&mut sim, 20.0, |s| activity(s, worker) != Activity::Gather(WorkStyle::Chop)));
    let load = sim.get::<Gatherer>(worker).unwrap();
    assert!(load.amount >= load.capacity - 1e-3, "left the tree with a full load: {load:?}");
    assert_eq!(activity(&sim, worker), Activity::Idle, "carrying home");
    assert!(run_until(&mut sim, 20.0, |s| activity(s, worker) == Activity::Gather(WorkStyle::Chop)), "back at work");
}

#[test]
fn each_resource_is_worked_in_its_own_style_and_farms_are_farmed() {
    let mut sim = Sim::new(&[("ironhold", 0)]);
    sim.building(P0, "town_center", Vec2::new(40.0, 40.0), true);
    let farm = sim.building(P0, "farm", Vec2::new(60.0, 40.0), true);
    let jobs = [
        (sim.node("berry_bush", Vec2::new(40.0, 52.0), None), WorkStyle::Forage),
        (sim.node("stone_outcrop", Vec2::new(40.0, 28.0), None), WorkStyle::Mine),
        (farm, WorkStyle::Farm),
    ];
    let workers: Vec<Entity> = [Vec2::new(40.0, 48.0), Vec2::new(40.0, 32.0), Vec2::new(55.0, 40.0)]
        .into_iter()
        .map(|p| sim.unit(P0, "peasant", p))
        .collect();
    for (w, (target, _)) in workers.iter().zip(&jobs) {
        sim.order(P0, &[*w], CommandKind::Gather(*target));
    }
    sim.run_secs(6.0);
    for (w, (target, style)) in workers.iter().zip(&jobs) {
        assert_eq!(sim.current_order(*w), Some(Order::Gather { target: *target }));
        assert_eq!(activity(&sim, *w), Activity::Gather(*style), "{style:?}");
    }
}

#[test]
fn builders_build_and_repairers_repair_once_there() {
    let mut sim = Sim::new(&[("ironhold", 0)]);
    let site = sim.building(P0, "house", Vec2::new(50.0, 40.0), false);
    let house = sim.building(P0, "house", Vec2::new(50.0, 56.0), true);
    let max = sim.get::<Health>(house).unwrap().max;
    sim.set(house, Health { hp: 10.0, max });
    let builder = sim.unit(P0, "peasant", Vec2::new(42.0, 40.0));
    let repairer = sim.unit(P0, "peasant", Vec2::new(42.0, 56.0));
    sim.order(P0, &[builder], CommandKind::Build(site));
    sim.order(P0, &[repairer], CommandKind::Repair(house));
    sim.ticks(3);
    assert_eq!(activity(&sim, builder), Activity::Idle, "walking to the site");
    assert_eq!(activity(&sim, repairer), Activity::Idle);
    sim.run_secs(5.0);
    assert_eq!(activity(&sim, builder), Activity::Build);
    assert_eq!(activity(&sim, repairer), Activity::Repair);
    // Finished: back to idle.
    sim.finish(site);
    sim.ticks(2);
    assert_eq!(activity(&sim, builder), Activity::Idle);
}

#[test]
fn chasing_and_attacking_is_fighting_and_ends_with_the_foe() {
    let mut sim = Sim::new(&[("ironhold", 0), ("dune", 1)]);
    let spear = sim.unit(P0, "spearman", Vec2::new(40.0, 40.0));
    let enemy = sim.unit(P1, "peasant", Vec2::new(48.0, 40.0));
    let worker = sim.unit(P0, "peasant", Vec2::new(40.0, 44.0));
    sim.order(P0, &[spear], CommandKind::Attack(enemy));
    sim.ticks(2);
    assert_eq!(activity(&sim, spear), Activity::Fight, "chasing");
    assert!(sim.get::<MoveGoal>(spear).is_some());
    assert_eq!(activity(&sim, worker), Activity::Idle, "a passive worker stays out of it");
    // No af_combat here: the enemy dies by decree.
    sim.app.world_mut().despawn(enemy);
    sim.ticks(2);
    assert_eq!(sim.current_order(spear), None);
    assert_eq!(activity(&sim, spear), Activity::Idle);
}

#[test]
fn healers_heal_only_while_mending() {
    let mut sim = Sim::new(&[("ironhold", 0)]);
    let monk = sim.unit(P0, "monk", Vec2::new(40.0, 40.0));
    let hurt = sim.unit(P0, "spearman", Vec2::new(49.0, 40.0));
    let max = sim.get::<Health>(hurt).unwrap().max;
    sim.set(hurt, Health { hp: 10.0, max });
    assert!(run_until(&mut sim, 6.0, |s| activity(s, monk) == Activity::Heal), "mends once in reach");
    assert!(sim.gap(monk, hurt) <= 6.0 + 1e-3);
    // Mended to full: nothing left to do.
    sim.set(hurt, Health { hp: max, max });
    sim.ticks(2);
    assert_eq!(activity(&sim, monk), Activity::Idle);
}
