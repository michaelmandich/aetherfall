//! Aetherfall unit behaviour: unit commands, order state machines,
//! gathering, building, repair, chasing, stances and healing.
//!
//! Schedule (FixedUpdate):
//!
//! * `SimSet::Commands` — `intake`: turns this tick's unit `PlayerCommand`s
//!   into `Orders` (formations, smart right-click resolution, stance
//!   changes, deletes).
//! * `SimSet::Orders`, chained:
//!   1. `brain::sync_brains` notices that the front order changed (from a
//!      command, af_economy, or a finished order) and cleans up after the
//!      previous one;
//!   2. one system per order family drives `MoveGoal` / `AttackTarget` and
//!      does the work: `travel` (Move, Follow), `fight` (Attack,
//!      AttackMove, Patrol, Hold), `gather` (Gather, ReturnCargo), `build`
//!      (Build, Repair);
//!   3. `heal`, then `idle` (stances, retaliation) for units without a
//!      task, then the `Idle` marker;
//!   4. `activity`: the shared `Activity` (gathering in which style,
//!      building, repairing, healing, fighting, or none) from what the
//!      brains did this tick.
//!
//! Each order system only acts on units whose [`Brain`] is in sync with the
//! front order, so an order popped by one system starts cleanly on the next
//! tick.

mod activity;
mod brain;
mod build;
mod fight;
mod formation;
mod gather;
mod heal;
mod idle;
mod intake;
mod motion;
mod notice;
mod targeting;
mod travel;
mod tuning;

use af_core::ext::economy::EconomyIntake;
use af_core::prelude::*;
use bevy::prelude::*;

pub use brain::Brain;

/// Unit commands and unit brains (see the crate docs for the schedule).
pub struct BehaviorPlugin;

impl Plugin for BehaviorPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(brain::attach_brain)
            .add_systems(FixedUpdate, intake::intake_commands.in_set(SimSet::Commands).after(EconomyIntake))
            .add_systems(
                FixedUpdate,
                (
                    brain::sync_brains,
                    travel::run_travel_orders,
                    fight::run_fight_orders,
                    gather::run_gather_orders,
                    build::run_build_orders,
                    heal::run_healers,
                    idle::run_idle_stances,
                    idle::mark_idle,
                    activity::update_activity,
                )
                    .chain()
                    .in_set(SimSet::Orders),
            );
    }
}
