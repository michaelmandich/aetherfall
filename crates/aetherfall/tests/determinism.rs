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
