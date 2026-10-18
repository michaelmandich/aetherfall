//! Move and Follow orders.

use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::prelude::*;

use crate::brain::Brain;
use crate::motion;
use crate::targeting::TargetData;
use crate::tuning::*;

#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Traveller {
    entity: Entity,
    pos: &'static Position,
    facing: &'static Facing,
    footprint: &'static Footprint,
    path: &'static PathState,
    orders: &'static mut Orders,
    brain: &'static mut Brain,
    goal: Option<&'static mut MoveGoal>,
    gatherer: Has<Gatherer>,
}

/// Move: walk to the point, done on arrival (or when af_movement gives
/// up). Follow: stay close to the target until it is gone. A unit that
/// cannot carry anything turns a gathering order into a walk to the source.
pub(crate) fn run_travel_orders(
    mut commands: Commands,
    mut units: Query<Traveller, (With<Unit>, Without<Dead>)>,
    targets: Query<TargetData, Without<Dead>>,
) {
    for mut u in &mut units {
        if !u.brain.synced(&u.orders) {
            continue;
        }
        match u.orders.current().copied() {
            Some(Order::Move { target }) => {
                motion::go_to(&mut commands, u.entity, &mut u.goal, target, MOVE_ARRIVE);
                if u.pos.0.distance(target) <= MOVE_ARRIVE || motion::settled(&u.goal, u.path) {
                    u.orders.queue.pop_front();
                }
            }
            Some(Order::Follow { target }) => {
                let Ok(t) = targets.get(target) else {
                    u.orders.queue.pop_front();
                    continue;
                };
                let gap = u.footprint.obb(u.pos.0, u.facing.0).gap(&t.obb());
                motion::approach(&mut commands, u.entity, &mut u.goal, u.path, target, t.pos.0, FOLLOW_ARRIVE, gap);
            }
            Some(Order::Gather { target }) if !u.gatherer => match targets.get(target) {
                Ok(t) => u.brain.replace_front(&mut u.orders, Order::Move { target: t.pos.0 }),
                Err(_) => {
                    u.orders.queue.pop_front();
                }
            },
            Some(Order::ReturnCargo) if !u.gatherer => {
                u.orders.queue.pop_front();
            }
            _ => {}
        }
    }
}
