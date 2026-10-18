//! Writing `MoveGoal`s without needless repaths, and reading back how
//! af_movement is doing with them.
//!
//! A goal is only rewritten when it actually changes, because every new
//! `MoveGoal` carries a fresh `seq` and forces a repath.

use af_core::prelude::*;
use bevy::prelude::*;

use crate::tuning::*;

/// The unit's current goal as seen through its query (`None` = no goal).
pub(crate) type Goal<'a> = Option<Mut<'a, MoveGoal>>;

/// Where to be relative to a target being shot at or healed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Plan {
    /// It can be acted on from here: stand. `reach` is the longest reach
    /// that does so.
    Stand { reach: f32 },
    /// Too far: chase until af_movement reports the box gap
    /// ([`Obb::gap`]) within `arrive`, which lies well inside reach so that
    /// a target hovering at the edge stays in it.
    Close { arrive: f32 },
    /// Inside the minimum range of every weapon that could hit it: step
    /// back until the gap is `min`.
    BackOff { min: f32 },
}

impl Plan {
    /// The plan for a single band `0..=reach`, with `gap` measured as
    /// af_movement does (healing).
    pub(crate) fn within(gap: f32, reach: f32) -> Plan {
        if gap <= reach {
            Plan::Stand { reach }
        } else {
            Plan::Close { arrive: reach * CHASE_FRACTION }
        }
    }

    pub(crate) fn stands(&self) -> bool {
        matches!(self, Plan::Stand { .. })
    }
}

/// A target to keep in reach: where it is, how far, and what to do about it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Engagement {
    pub pos: Vec2,
    /// Edge-to-edge distance now, as the plan measures it (af_combat's
    /// `edge_distance` for weapons).
    pub gap: f32,
    pub plan: Plan,
}

/// How af_movement is doing with `goal`. A path computed for an older goal
/// does not count: until af_movement catches up the goal is `Pending`.
pub(crate) fn status(goal: &Goal, path: &PathState) -> PathStatus {
    match goal {
        None => PathStatus::Idle,
        Some(g) if g.seq == path.goal_seq => path.status,
        Some(_) => PathStatus::Pending,
    }
}

/// af_movement gave up on the current goal.
fn failed(goal: &Goal, path: &PathState) -> bool {
    status(goal, path) == PathStatus::Failed
}

/// af_movement is done with the goal of reaching `target` (arrived, maybe
/// at the end of a partial path, or gave up).
pub(crate) fn done_with(goal: &Goal, path: &PathState, target: Entity) -> bool {
    goal.as_ref().is_some_and(|g| g.follow == Some(target)) && settled(goal, path)
}

/// af_movement gave up on what [`pursue`] asked of it for `target`: the
/// chase, or the step back out of its minimum range (a point goal).
pub(crate) fn engagement_failed(goal: &Goal, path: &PathState, target: Entity) -> bool {
    goal.as_ref().is_some_and(|g| g.follow.is_none_or(|f| f == target)) && failed(goal, path)
}

/// af_movement finished with the current goal, one way or the other.
pub(crate) fn settled(goal: &Goal, path: &PathState) -> bool {
    matches!(status(goal, path), PathStatus::Arrived | PathStatus::Failed)
}

/// Walk to a point.
pub(crate) fn go_to(commands: &mut Commands, entity: Entity, goal: &mut Goal, target: Vec2, arrive: f32) {
    match goal {
        Some(g) if g.follow.is_none() && g.target.distance_squared(target) < 0.01 && (g.arrive - arrive).abs() < 0.01 => {}
        Some(g) => **g = MoveGoal::point(target, arrive),
        None => {
            commands.entity(entity).try_insert(MoveGoal::point(target, arrive));
        }
    }
}

/// Walk until our box is within `arrive` of `target`'s box (`gap` is the
/// current [`Obb::gap`]). Once af_movement has settled, the goal is
/// re-issued if the target has moved [`RESUME_SLACK`] beyond `arrive` again.
#[allow(clippy::too_many_arguments)]
pub(crate) fn approach(commands: &mut Commands, entity: Entity, goal: &mut Goal, path: &PathState, target: Entity, target_pos: Vec2, arrive: f32, gap: f32) {
    let resume = gap > arrive + RESUME_SLACK && settled(goal, path);
    match goal {
        Some(g) if g.follow == Some(target) && (g.arrive - arrive).abs() < 0.01 => {
            if resume {
                **g = MoveGoal::entity(target, target_pos, arrive);
            }
        }
        Some(g) => **g = MoveGoal::entity(target, target_pos, arrive),
        None => {
            commands.entity(entity).try_insert(MoveGoal::entity(target, target_pos, arrive));
        }
    }
}

/// Stand still.
pub(crate) fn halt(commands: &mut Commands, entity: Entity, goal: &mut Goal) {
    if goal.take().is_some() {
        commands.entity(entity).try_remove::<MoveGoal>();
    }
}

/// Keep `target` in reach of our weapons (or heal): stand while something
/// can act on it, chase while it is too far, step back while it is inside
/// every minimum range. Returns whether the unit can act on it now.
///
/// While chasing, the unit only stops once comfortably inside reach (or
/// once af_movement settles), so a target hovering at the edge does not
/// make it stop and start every tick.
pub(crate) fn pursue(commands: &mut Commands, entity: Entity, goal: &mut Goal, path: &PathState, me: Vec2, target: Entity, en: &Engagement) -> bool {
    match en.plan {
        Plan::Stand { reach } => {
            let chasing = goal.as_ref().is_some_and(|g| g.follow == Some(target));
            if !chasing || en.gap <= reach * STOP_FRACTION || settled(goal, path) {
                halt(commands, entity, goal);
            }
            true
        }
        Plan::Close { arrive } => {
            chase(commands, entity, goal, path, target, en.pos, arrive);
            false
        }
        Plan::BackOff { min } => {
            back_off(commands, entity, goal, path, me, en, min);
            false
        }
    }
}

/// Chase `target` until af_movement reports the box gap within `arrive`.
///
/// A running chase is kept even when `arrive` drifts with the angle of
/// approach (a new goal means a repath). A finished one, while the target
/// is still out of reach, starts again at once: af_movement itself only
/// resumes a little beyond `arrive`, which for melee reaches lies outside
/// the weapon's reach, so the unit would stand there and never strike.
fn chase(commands: &mut Commands, entity: Entity, goal: &mut Goal, path: &PathState, target: Entity, target_pos: Vec2, arrive: f32) {
    let running = goal.as_ref().is_some_and(|g| g.follow == Some(target)) && !settled(goal, path);
    if running {
        return;
    }
    match goal {
        Some(g) => **g = MoveGoal::entity(target, target_pos, arrive),
        None => {
            commands.entity(entity).try_insert(MoveGoal::entity(target, target_pos, arrive));
        }
    }
}

/// Step straight away from the target until the gap is `min` plus
/// [`BACKOFF_MARGIN`]. A walk already heading about there is kept, and a
/// failed one is not retried from the same spot (the caller counts it).
fn back_off(commands: &mut Commands, entity: Entity, goal: &mut Goal, path: &PathState, me: Vec2, en: &Engagement, min: f32) {
    let away = (me - en.pos).try_normalize().unwrap_or(Vec2::X);
    let spot = me + away * (min + BACKOFF_MARGIN - en.gap);
    let underway =
        goal.as_ref().is_some_and(|g| g.follow.is_none() && g.target.distance_squared(spot) <= BACKOFF_SLACK * BACKOFF_SLACK) && !settled(goal, path);
    if !underway {
        go_to(commands, entity, goal, spot, MOVE_ARRIVE);
    }
}
