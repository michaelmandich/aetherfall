//! After collisions (`SimSet::Movement`, last): actual velocity, and stuck
//! detection for following units.
//!
//! * A unit that moved less than [`STUCK_MIN_SPEED`] x window (and did not
//!   turn) during a [`STUCK_WINDOW`] asks for a new path; after
//!   [`STUCK_FAIL_TIME`] of that it gives up with `PathStatus::Failed`.
//! * A ground unit that keeps moving yet gets no closer along its path gives
//!   up too: after [`MAX_FRUITLESS_REPATHS`] stuck repaths without progress
//!   (each hands it the same route through a gap it does not fit, so it
//!   paces between the same waypoints, which the first rule never sees), or
//!   after [`NO_PROGRESS_TIME`] of following without progress at all.
//!   Progress is the length of the way left shrinking by [`PROGRESS_STEP`]:
//!   along the path, plus the coarse route beyond it while the path ends
//!   at the end of a leg (see `pathing`), so a long detour is progress all
//!   the way. The count restarts with every new goal or path, except a
//!   path asked for because the unit was stuck, and the next leg of the
//!   same way. The next leg's path takes the place of the route's stretch
//!   it covers, and is often longer (the route leaves obstacles out): the
//!   best mark moves up by what it adds (`Mover::rebase_progress`), so a
//!   way round a wood or a wall is progress too.
//!
//! Time spent waiting for the path service does not count.

use af_core::prelude::af_geom::wrap_angle;
use af_core::prelude::*;
use bevy::prelude::*;

use crate::mover::Mover;

/// Length of one stuck-detection window (seconds).
const STUCK_WINDOW: f32 = 1.0;
/// Less progress than this (m/s) over a window counts as stuck.
const STUCK_MIN_SPEED: f32 = 0.1;
/// Turning this much (radians) in a window is progress (turning on the spot).
const STUCK_MIN_TURN: f32 = 0.2;
/// Stuck this long (seconds): `Failed`.
const STUCK_FAIL_TIME: f32 = 3.0;
/// Per-tick smoothing of `Mover::flow` toward the fraction just achieved.
const FLOW_SMOOTHING: f32 = 0.25;
/// Following this long (seconds) without the path length left shrinking
/// by [`PROGRESS_STEP`]: `Failed` (a crowd jostling at a choke for this
/// long has given up for practical purposes).
const NO_PROGRESS_TIME: f32 = 20.0;
/// Stuck repaths in a row without progress before `Failed`.
const MAX_FRUITLESS_REPATHS: u8 = 2;
/// Least gain (metres) that counts as progress along the path.
const PROGRESS_STEP: f32 = 0.5;

type ProgressQuery<'a> = (&'a Position, &'a Facing, &'a Mobile, &'a mut Velocity, &'a mut PathState, &'a mut Mover);

pub(crate) fn track_progress(mut units: Query<ProgressQuery, Without<Dead>>) {
    for (pos, facing, mob, mut vel, mut path, mut mover) in &mut units {
        let v = (pos.0 - mover.tick_start) / SIM_DT;
        if vel.0 != v {
            vel.0 = v;
        }
        let asked = mover.intended.length_squared();
        if asked > 1e-10 {
            let achieved = ((pos.0 - mover.tick_start).dot(mover.intended) / asked).clamp(0.0, 1.0);
            mover.flow += (achieved - mover.flow) * FLOW_SMOOTHING;
        } else if mover.flow < 1.0 {
            mover.flow = 1.0;
        }
        if mover.repath_cooldown > 0.0 {
            mover.repath_cooldown = (mover.repath_cooldown - SIM_DT).max(0.0);
        }
        // Not following, or waiting for the path service (a requested
        // repath, the end of a truncated leg): standing still is not being
        // stuck. Keep the window fresh so the count starts from here.
        if path.status != PathStatus::Following || mover.wants_repath {
            mover.restart_window(pos.0, facing.0);
            continue;
        }
        if !mob.flying && !making_progress(pos.0, &path, &mut mover) {
            path.status = PathStatus::Failed;
            continue;
        }
        mover.window_time += SIM_DT;
        if mover.window_time + 1e-4 < STUCK_WINDOW {
            continue;
        }
        let moved = pos.0.distance(mover.window_anchor);
        let turned = wrap_angle(facing.0 - mover.window_facing).abs();
        if moved < STUCK_MIN_SPEED * mover.window_time && turned < STUCK_MIN_TURN {
            path.stuck_time += mover.window_time;
            if path.stuck_time + 1e-3 >= STUCK_FAIL_TIME || mover.fruitless_repaths >= MAX_FRUITLESS_REPATHS {
                path.status = PathStatus::Failed;
            } else {
                mover.wants_repath = true;
                mover.stuck_repath = true;
                mover.fruitless_repaths += 1;
            }
        } else if path.stuck_time != 0.0 {
            path.stuck_time = 0.0;
        }
        mover.restart_window(pos.0, facing.0);
    }
}

/// One tick of following: updates the progress count (a new best path
/// length left, or more time without one) and returns false once the unit
/// has followed for [`NO_PROGRESS_TIME`] without progress.
fn making_progress(pos: Vec2, path: &PathState, mover: &mut Mover) -> bool {
    let left = mover.way_left(pos, path);
    if left < mover.progress_best - PROGRESS_STEP || !mover.progress_best.is_finite() {
        mover.progress_best = left;
        mover.progress_time = 0.0;
        mover.fruitless_repaths = 0;
        return true;
    }
    mover.progress_time += SIM_DT;
    mover.progress_time + 1e-4 < NO_PROGRESS_TIME
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit standing still while it waits for a repath is not stuck; once
    /// its requests are served at once, three stuck windows in a row fail it.
    #[test]
    fn waiting_for_the_path_service_is_not_being_stuck() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems(track_progress);
        let following = PathState { status: PathStatus::Following, ..PathState::default() };
        let waiting = Mover { wants_repath: true, ..Mover::default() };
        let e = world.spawn((Position(Vec2::ZERO), Facing(0.0), walker(), Velocity(Vec2::ZERO), following, waiting)).id();
        let ticks = |seconds: f32| (seconds / SIM_DT).round() as u32;
        for _ in 0..ticks(2.0 * STUCK_FAIL_TIME) {
            schedule.run(&mut world);
        }
        let path = world.get::<PathState>(e).expect("path");
        assert_eq!((path.status, path.stuck_time), (PathStatus::Following, 0.0), "a long wait in the queue");

        for _ in 0..ticks(STUCK_FAIL_TIME + STUCK_WINDOW) {
            world.get_mut::<Mover>(e).expect("mover").wants_repath = false;
            schedule.run(&mut world);
        }
        assert_eq!(world.get::<PathState>(e).expect("path").status, PathStatus::Failed);
    }

    fn walker() -> Mobile {
        Mobile { mobility: Mobility::of(MoveFlag::Foot), radius: 0.3, turn_rate: 6.0, flying: false }
    }

    /// Run a unit following a path 10 m along +X through `cycles` of: stand
    /// one stuck window (a stuck repath, served at once with the same
    /// route), walk back a metre, then forward to `gain` metres past where
    /// it stood. Returns the path status at the end.
    fn pace(cycles: usize, gain: f32) -> PathStatus {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems(track_progress);
        let path = PathState { status: PathStatus::Following, waypoints: vec![Vec2::new(10.0, 0.0)], ..PathState::default() };
        let e = world.spawn((Position(Vec2::ZERO), Facing(0.0), walker(), Velocity(Vec2::ZERO), path, Mover::default())).id();
        let mut x = 0.0f32;
        let window = (STUCK_WINDOW / SIM_DT).round() as usize;
        let mut step = |world: &mut World, x: f32| {
            world.get_mut::<Position>(e).expect("position").0 = Vec2::new(x, 0.0);
            schedule.run(world);
            let mut mover = world.get_mut::<Mover>(e).expect("mover");
            if mover.wants_repath {
                // The path service hands back the same route (see apply_path).
                mover.wants_repath = false;
                mover.stuck_repath = false;
            }
        };
        for _ in 0..cycles {
            for _ in 0..window {
                step(&mut world, x);
            }
            let t = |i: usize| (i + 1) as f32 / window as f32;
            for i in 0..window {
                step(&mut world, x - t(i));
            }
            for i in 0..window {
                step(&mut world, x - 1.0 + (1.0 + gain) * t(i));
            }
            x += gain;
        }
        world.get::<PathState>(e).expect("path").status
    }

    /// Pacing back and forth between stuck repaths that bring it no closer
    /// fails a unit, although it never stands still long enough for the
    /// stuck rule alone; the same pacing while it gains ground does not.
    #[test]
    fn pacing_without_progress_fails() {
        assert_eq!(pace(MAX_FRUITLESS_REPATHS as usize, 0.0), PathStatus::Following, "a stuck repath or two may help");
        assert_eq!(pace(MAX_FRUITLESS_REPATHS as usize + 1, 0.0), PathStatus::Failed);
        assert_eq!(pace(6, 1.0), PathStatus::Following, "progress between the repaths");
    }
}
