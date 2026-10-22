//! Aetherfall movement: nav obstacle sync, path service, steering, collisions
//! (boxes never overlap).
//!
//! FixedUpdate, chained:
//!
//! | set | system | what it does |
//! |---|---|---|
//! | `Pathing` | `attach_movers` | gives each unit af_movement's private state, numbered in the order units appear (who is younger) |
//! | | `sync_obstacles` | registers added/changed `Obstacle`s in `Nav` (removals: an observer), and closes slots between two obstacles too narrow to pass |
//! | | `invalidate_blocked_paths` | flags paths cut by obstacles placed this tick |
//! | | `refresh_follow_targets` | looks up followed entities' positions and boxes |
//! | | `collect_requests` | new goals and repath wishes: a path at once when no search is needed (flyers, straight lines, a groupmate's search or route), else `Pending` + queue; a unit that got stuck asks for a way round the units near it |
//! | | `service_paths` | budgeted FIFO A*, each request first offered the routes found before it; a search the tick's budget cuts short goes on next tick |
//! | `Movement` | `steer` | arrival, waypoints, avoidance, turning (as far as there is room), terrain speed |
//! | | `resolve_collisions` | SAT push-out vs units, obstacles, terrain, map edge; then the final word on pairs: none ends the tick overlapping past 0.05 m, or deeper than it began, and an overlap a tick began with is parted within a few ticks without big jumps; notes who waits its turn in a queue |
//! | | `track_progress` | `Velocity`, stuck detection (waiting its turn in a queue that moves on is not being stuck) |
//!
//! Contract: reads `MoveGoal` (af_behavior, af_economy), writes `PathState`,
//! `Position`, `Facing` (while moving) and `Velocity` of units, and keeps
//! `Nav` in sync with `Obstacle`s. `Nav` is marked changed only when an
//! obstacle changes; path caches are updated without change detection.
//!
//! `PathStatus::Arrived` also covers crowd arrival: a unit with a point goal
//! that touches a unit of the same owner already arrived at (nearly) the same
//! point, or standing there (idle, or at work: say a groupmate of an earlier
//! order to that spot), within the radius the whole crowd needs around that
//! point, counts as arrived although its centre may be farther than `arrive`
//! from the goal. A group ordered to one spot thus settles into a blob
//! instead of pushing forever, and so do units rallied one by one to where
//! the earlier ones stand. Not in a passage (a gap in a cliff, a slot
//! between buildings) on the way to a goal outside it, though: the unit
//! would stop in the gap the rest still need. Follow goals always arrive by
//! box gap.
//!
//! [`af_core::ext::movement::CollisionStats`] counts what the collision
//! solver's final word did (for tests and reports).

mod bodies;
mod collision;
mod mover;
mod obstacles;
mod pathing;
mod progress;
mod steering;

use af_core::prelude::*;
use bevy::prelude::*;

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<obstacles::ObstacleRegistry>()
            .init_resource::<pathing::PathQueue>()
            .init_resource::<af_core::ext::movement::CollisionStats>()
            .add_observer(obstacles::forget_removed)
            .add_systems(
                FixedUpdate,
                (
                    mover::attach_movers,
                    obstacles::sync_obstacles,
                    pathing::invalidate_blocked_paths,
                    pathing::refresh_follow_targets,
                    pathing::collect_requests,
                    pathing::service_paths,
                )
                    .chain()
                    .in_set(SimSet::Pathing),
            )
            .add_systems(
                FixedUpdate,
                (steering::steer, collision::resolve_collisions, progress::track_progress).chain().in_set(SimSet::Movement),
            );
    }
}
