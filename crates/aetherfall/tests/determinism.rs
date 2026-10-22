//! The simulation replays identically whatever drives it.
//!
//! * No two `FixedUpdate` systems that touch the same data run in an
//!   unspecified order, so the multi-threaded executor cannot change a
//!   match (Bevy's ambiguity detection, applied to the whole sim).
//! * How many sim ticks run per frame does not matter: a windowed game runs
//!   one or two per frame, the headless harness thirty.
//!
//! `matches.rs` checks that the same seed replays identically.

use aetherfall::*;
use af_core::prelude::*;
use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
use bevy::prelude::*;

fn two_player_match(ticks_per_update: u32) -> App {
    let data = af_core::testing::test_data();
    let opts = SkirmishOptions { seed: 4, players: 2, factions: vec!["ironhold".into(), "dune".into()], human: false, ..Default::default() };
    headless_app(data.clone(), skirmish_setup(&data, &opts).expect("setup"), ticks_per_update)
}

/// Bevy's report of every pair of `FixedUpdate` systems with conflicting
/// data access and no ordering between them (empty when there are none).
fn fixed_update_ambiguities() -> String {
    let mut app = two_player_match(1);
    let world = app.world_mut();
    let mut schedule = world.resource_mut::<Schedules>().remove(FixedUpdate).expect("FixedUpdate");
    schedule.set_build_settings(ScheduleBuildSettings { ambiguity_detection: LogLevel::Error, ..default() });
    match schedule.initialize(world) {
        Ok(_) => String::new(),
        Err(e) => e.to_string(schedule.graph(), world),
    }
}

#[test]
fn sim_systems_touching_the_same_data_are_ordered() {
    let report = fixed_update_ambiguities();
    assert!(report.is_empty(), "{report}");
}

/// A unit as (type, position x, position z, facing), bit for bit.
type UnitBits = (u16, u32, u32, u32);

/// Every unit's type, position and facing, and every player's resources.
fn state(app: &mut App) -> (Vec<UnitBits>, Vec<[u32; MAX_RESOURCES]>) {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(&Unit, &Position, &Facing), Without<Dead>>();
    let mut units: Vec<_> = q.iter(world).map(|(u, p, f)| (u.kind.0, p.0.x.to_bits(), p.0.y.to_bits(), f.0.to_bits())).collect();
    units.sort_unstable();
    let resources = world.resource::<Players>().list.iter().map(|p| p.resources.0.map(f32::to_bits)).collect();
    (units, resources)
}

#[test]
fn ticks_per_frame_do_not_change_the_match() {
    let ticks = 60 * SIM_HZ as u64;
    let mut states = Vec::new();
    for ticks_per_update in [1, 30] {
        let mut app = two_player_match(ticks_per_update);
        run_headless(&mut app, ticks, 0, |_| {});
        assert_eq!(app.world().resource::<SimClock>().tick, ticks);
        states.push(state(&mut app));
    }
    assert!(states[0] == states[1], "one tick per frame and thirty diverge");
}

/// A unit as seen by the digest: type, position, facing and hit points, bit
/// for bit, keyed by entity so a divergence names the unit.
type UnitDigest = (Entity, u16, u32, u32, u32, u32, u32);

/// Every live unit (sorted by entity) and every player's resources, bit for
/// bit.
fn digest(app: &mut App) -> (Vec<UnitDigest>, Vec<[u32; MAX_RESOURCES]>) {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(Entity, &Unit, &Position, &Facing, Option<&Health>, Option<&Gatherer>), Without<Dead>>();
    let mut units: Vec<UnitDigest> = q
        .iter(world)
        .map(|(e, u, p, f, h, g)| (e, u.kind.0, p.0.x.to_bits(), p.0.y.to_bits(), f.0.to_bits(), h.map_or(0, |h| h.hp.to_bits()), g.map_or(0, |g| g.amount.to_bits())))
        .collect();
    units.sort_unstable_by_key(|d| d.0);
    let resources = world.resource::<Players>().list.iter().map(|p| p.resources.0.map(f32::to_bits)).collect();
    (units, resources)
}

/// A client-only marker like `Selected`, `BoxVisual` or a model marker: an
/// empty component the simulation never reads. Adding it moves the entity
/// to another archetype, which changes the order Bevy queries visit units.
#[derive(Component)]
struct Marker;

/// Marks every second unit (sorted by entity) with [`Marker`].
fn mark_every_second_unit(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut q = world.query_filtered::<Entity, With<Unit>>();
    let mut units: Vec<Entity> = q.iter(world).collect();
    units.sort_unstable();
    let odd: Vec<Entity> = units.iter().copied().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, e)| e).collect();
    for e in &odd {
        world.entity_mut(*e).insert(Marker);
    }
    odd.len()
}

/// One digest entry as text.
fn unit_line(d: &UnitDigest) -> String {
    format!(
        "{:?} kind {} pos ({}, {}) facing {} hp {} cargo {}",
        d.0,
        d.1,
        f32::from_bits(d.2),
        f32::from_bits(d.3),
        f32::from_bits(d.4),
        f32::from_bits(d.5),
        f32::from_bits(d.6)
    )
}

/// Where a unit started the tick and what it is up to (goal, path, order,
/// cargo), so a divergence shows whether the intent or the movement differs.
fn describe(world: &World, e: Entity) -> String {
    let Ok(entity) = world.get_entity(e) else { return "gone".into() };
    let prev = entity.get::<PrevPosition>().map(|p| (p.0.x, p.0.y));
    let prev_facing = entity.get::<PrevFacing>().map(|f| f.0);
    let vel = entity.get::<Velocity>().map(|v| (v.0.x, v.0.y));
    let goal = entity.get::<MoveGoal>().map(|g| format!("({}, {}) arrive {} follow {:?} seq {}", g.target.x, g.target.y, g.arrive, g.follow, g.seq));
    let path = entity
        .get::<PathState>()
        .map(|p| format!("{:?} next {}/{} partial {} seq {} waypoints {:?}", p.status, p.next, p.waypoints.len(), p.partial, p.goal_seq, p.waypoints));
    let order = entity.get::<Orders>().and_then(|o| o.current().cloned());
    let cargo = entity.get::<Gatherer>().map(|g| format!("{:?} {} target {:?}", g.carrying, g.amount, g.target));
    format!(
        "      tick start {prev:?} facing {prev_facing:?} velocity {vel:?}\n      goal {goal:?}\n      path {path:?}\n      order {order:?} cargo {cargo:?}"
    )
}

/// Describes the first tick two matches differ: the resources, then every
/// unit present in one only or differing between them, with its state.
fn divergence_report(plain: &App, marked: &App, a: &(Vec<UnitDigest>, Vec<[u32; MAX_RESOURCES]>), b: &(Vec<UnitDigest>, Vec<[u32; MAX_RESOURCES]>)) -> String {
    let mut report = String::new();
    if a.1 != b.1 {
        report.push_str(&format!("  resources: plain  {:?}\n             marked {:?}\n", a.1, b.1));
    }
    let (mut i, mut j) = (0, 0);
    while i < a.0.len() || j < b.0.len() {
        match (a.0.get(i), b.0.get(j)) {
            (Some(x), Some(y)) if x.0 == y.0 => {
                if x != y {
                    report.push_str(&format!("  plain:  {}\n{}\n", unit_line(x), describe(plain.world(), x.0)));
                    report.push_str(&format!("  marked: {}\n{}\n", unit_line(y), describe(marked.world(), y.0)));
                }
                i += 1;
                j += 1;
            }
            (Some(x), Some(y)) if x.0 < y.0 => {
                report.push_str(&format!("  only in plain:  {}\n", unit_line(x)));
                i += 1;
            }
            (Some(x), None) => {
                report.push_str(&format!("  only in plain:  {}\n", unit_line(x)));
                i += 1;
            }
            (_, Some(y)) => {
                report.push_str(&format!("  only in marked: {}\n", unit_line(y)));
                j += 1;
            }
            (None, None) => break,
        }
    }
    report
}

/// Client-only components (`Selected`, visual markers) are added between
/// ticks and move units to other archetypes. That must not change the match:
/// in lockstep multiplayer every peer has a different selection, and a
/// windowed match must replay like its headless twin.
#[test]
fn client_only_components_do_not_change_the_match() {
    const MARK_AT: u64 = 90;
    let ticks = 5 * 60 * SIM_HZ as u64;
    let mut plain = two_player_match(1);
    let mut marked = two_player_match(1);
    // Prime the clocks (enters Playing), like `run_headless`.
    plain.update();
    marked.update();
    let mut marked_count = 0;
    loop {
        plain.update();
        marked.update();
        let tick = plain.world().resource::<SimClock>().tick;
        assert_eq!(tick, marked.world().resource::<SimClock>().tick, "the two matches stepped differently");
        let a = digest(&mut plain);
        let b = digest(&mut marked);
        if a != b {
            let report = divergence_report(&plain, &marked, &a, &b);
            panic!("the matches diverge at tick {tick} (marked {marked_count} units at tick {MARK_AT})\n{report}");
        }
        if tick == MARK_AT {
            marked_count = mark_every_second_unit(&mut marked);
            assert!(marked_count > 0, "no units to mark");
        }
        if tick >= ticks || plain.world().resource::<GameOutcome>().ended {
            break;
        }
    }
    assert!(marked_count > 0, "the marker was never added");
}
