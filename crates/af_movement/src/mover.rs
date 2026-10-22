//! af_movement's private per-unit bookkeeping, kept beside the contract's
//! `PathState` (which other crates read).

use af_core::prelude::*;
use bevy::prelude::*;

/// The followed entity as seen at the start of this tick's movement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FollowTarget {
    pub pos: Vec2,
    pub obb: Obb,
}

/// Movement state of one unit. Inserted by [`attach_movers`] on every unit.
#[derive(Component, Clone, Debug)]
pub(crate) struct Mover {
    /// Waiting in the path queue.
    pub queued: bool,
    /// The current goal needs a fresh path (stuck, blocked, target moved).
    pub wants_repath: bool,
    /// Seconds before a follow-target repath may be requested again.
    pub repath_cooldown: f32,
    /// Followed entity this tick (`None`: no follow goal, or it is gone).
    pub follow: Option<FollowTarget>,
    /// Where the current path was computed from and toward.
    pub path_start: Vec2,
    pub path_goal: Vec2,
    /// The current path stopped at the search budget, not at the goal.
    pub truncated: bool,
    /// Distance left to the goal when the last truncated path ran out
    /// (another repath is only worth it when it got closer).
    pub best_remaining: f32,
    /// Position at the start of this tick's movement (for `Velocity`).
    pub tick_start: Vec2,
    /// Displacement steering asked for this tick.
    pub intended: Vec2,
    /// Smoothed fraction of the asked displacement that survived collisions
    /// (1 = free, near 0 = pushing into a jam). Scales speed, so a jam is
    /// not squeezed harder than the solver can resolve.
    pub flow: f32,
    /// Stuck detection: position, facing and age of the current window.
    pub window_anchor: Vec2,
    pub window_facing: f32,
    pub window_time: f32,
    /// Touched a unit of the same owner that already arrived at (nearly) the
    /// same point: counts as arrived (lets a group settle into a blob).
    pub crowd_contact: bool,
    /// The pending repath was asked for because the unit was stuck: its path
    /// continues the progress count instead of starting a new one.
    pub stuck_repath: bool,
    /// Least path length left (metres) since the progress count started.
    pub progress_best: f32,
    /// Seconds of following since `progress_best` last improved.
    pub progress_time: f32,
    /// Stuck repaths since `progress_best` last improved.
    pub fruitless_repaths: u8,
    /// Ticks before the collision solver's final word may look for a free
    /// spot to set the unit down again (it found none lately).
    pub relocate_wait: u8,
    /// Spawned this tick (the collision solver has not placed it yet).
    pub fresh: bool,
    /// Ticks in a row the unit has begun overlapping another unit deeper
    /// than the solver's final tolerance (0: it began this tick apart). The
    /// solver keeps parting it, more boldly the longer it lasts.
    pub overlapped_for: u16,
    /// Order in which units got their movement state (see [`attach_movers`]):
    /// of two units, the one with the greater number is the younger. Entity
    /// ids do not tell age: Bevy orders them by opaque bits, the generation
    /// of a recycled slot first.
    pub born: u64,
    /// Ticks the unit goes on giving way to walkers it meets head-on (the
    /// collision solver sets it while the unit gives way, and counts it
    /// down after): a file that gave way to a column keeps giving way to
    /// the rest of that column, whatever their ages.
    pub give_way: u8,
    /// The collision solver found the unit waiting its turn this tick:
    /// pressed against a walker ahead of it going its way that moves on (a
    /// queue at a gap). Not being stuck (see `track_progress`).
    pub waiting: bool,
}

impl Default for Mover {
    fn default() -> Self {
        Mover {
            queued: false,
            wants_repath: false,
            repath_cooldown: 0.0,
            follow: None,
            path_start: Vec2::ZERO,
            path_goal: Vec2::ZERO,
            truncated: false,
            best_remaining: f32::INFINITY,
            tick_start: Vec2::ZERO,
            intended: Vec2::ZERO,
            flow: 1.0,
            window_anchor: Vec2::ZERO,
            window_facing: 0.0,
            window_time: 0.0,
            crowd_contact: false,
            stuck_repath: false,
            progress_best: f32::INFINITY,
            progress_time: 0.0,
            fruitless_repaths: 0,
            relocate_wait: 0,
            fresh: false,
            overlapped_for: 0,
            born: 0,
            give_way: 0,
            waiting: false,
        }
    }
}

impl Mover {
    /// Forget everything about the previous goal.
    pub fn reset_goal(&mut self, pos: Vec2, facing: f32) {
        let queued = self.queued;
        *self = Mover {
            queued,
            tick_start: self.tick_start,
            follow: self.follow,
            flow: self.flow,
            relocate_wait: self.relocate_wait,
            fresh: self.fresh,
            overlapped_for: self.overlapped_for,
            born: self.born,
            give_way: self.give_way,
            ..Mover::default()
        };
        self.restart_window(pos, facing);
    }

    pub fn restart_window(&mut self, pos: Vec2, facing: f32) {
        self.window_anchor = pos;
        self.window_facing = facing;
        self.window_time = 0.0;
    }

    /// Start counting progress afresh (a new path for a new reason).
    pub fn restart_progress(&mut self) {
        self.progress_best = f32::INFINITY;
        self.progress_time = 0.0;
        self.fruitless_repaths = 0;
    }
}

type Unattached = (With<Mobile>, Without<Mover>);

/// Give every unit its movement state (spawned units appear on their first
/// tick), numbered in the order they appear ([`Mover::born`]): units of
/// one tick by entity index, which for fresh slots is the order they were
/// spawned in, and in any case the same every run.
pub(crate) fn attach_movers(
    mut commands: Commands,
    mut born: Local<u64>,
    mut new: Local<Vec<(u32, Entity, Vec2, f32)>>,
    units: Query<(Entity, &Position, &Facing), Unattached>,
) {
    new.clear();
    new.extend(units.iter().map(|(e, pos, facing)| (e.index_u32(), e, pos.0, facing.0)));
    new.sort_unstable_by_key(|n| n.0);
    for &(_, e, pos, facing) in new.iter() {
        let mut m = Mover { tick_start: pos, fresh: true, born: *born, ..Mover::default() };
        *born += 1;
        m.restart_window(pos, facing);
        commands.entity(e).try_insert(m);
    }
}

/// Is the unit actively going somewhere (as opposed to standing)?
#[inline]
pub(crate) fn is_moving(goal: Option<&MoveGoal>, path: &PathState) -> bool {
    goal.is_some() && matches!(path.status, PathStatus::Pending | PathStatus::Following)
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};

    /// Units are numbered in the order they appear (by entity index within
    /// a tick), and a goal change keeps the number: one spawned later, into
    /// a freed slot or not, is younger than every unit before it.
    #[test]
    fn units_are_numbered_in_the_order_they_appear() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 64.0, &[("ironhold", 0)]);
        let foot = data.unit_ids().find(|u| data.unit(*u).locomotion == MoveFlag::Foot).expect("a foot unit");
        let spawn = |app: &mut App, x: f32| with_commands(app, |c, data, players| spawn_unit(c, data, players.get(p[0]), foot, Vec2::new(x, 10.0), 0.0));
        let born = |app: &App, e: Entity| app.world().get::<Mover>(e).expect("mover").born;
        let first: Vec<Entity> = (0..3).map(|i| spawn(&mut app, 10.0 + i as f32 * 2.0)).collect();
        run_ticks(&mut app, 1);
        assert!(born(&app, first[0]) < born(&app, first[1]) && born(&app, first[1]) < born(&app, first[2]));
        app.world_mut().entity_mut(first[1]).despawn();
        app.world_mut().entity_mut(first[0]).insert(MoveGoal::point(Vec2::new(40.0, 40.0), 0.5));
        let later = spawn(&mut app, 30.0);
        run_ticks(&mut app, 1);
        assert!(born(&app, later) > born(&app, first[0]) && born(&app, later) > born(&app, first[2]));
        assert!(born(&app, first[0]) < born(&app, first[2]), "a new goal keeps the number");
    }
}
