//! af_behavior's systems have a fixed order against everything else in
//! the simulation that touches the same data.

use af_core::ext::economy::EconomyIntake;
use af_core::prelude::*;
use af_core::testing::{sim_app, test_data};
use af_behavior::BehaviorPlugin;
use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
use bevy::prelude::*;

/// Stands in for af_economy's command intake: reads the tick's commands
/// and posts notices, as the real one does.
fn economy_intake(tick: Res<TickCommands>, clock: Res<SimClock>, mut notices: ResMut<Notices>) {
    for c in &tick.list {
        notices.push(Some(c.player), NoticeKind::Warning, "economy", None, clock.elapsed);
    }
}

/// The unit command intake runs after the economy's: both post notices
/// (and write deaths), so leaving them unordered would let the scheduler
/// pick a different order from run to run.
#[test]
fn unit_intake_runs_after_the_economy_intake() {
    let mut app = sim_app(test_data());
    app.add_plugins(BehaviorPlugin)
        .configure_sets(FixedUpdate, EconomyIntake.in_set(SimSet::Commands))
        .add_systems(FixedUpdate, economy_intake.in_set(EconomyIntake));
    let world = app.world_mut();
    let mut schedule = world.resource_mut::<Schedules>().remove(FixedUpdate).expect("FixedUpdate");
    schedule.set_build_settings(ScheduleBuildSettings { ambiguity_detection: LogLevel::Error, ..default() });
    if let Err(e) = schedule.initialize(world) {
        panic!("{}", e.to_string(schedule.graph(), world));
    }
}
