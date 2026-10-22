//! Steering (`SimSet::Movement`, first): arrival, waypoint following, local
//! avoidance, turning and integration of every unit with a path.
//!
//! * Arrival: point goals when the centre is within `arrive` of the target,
//!   follow goals when the box gap to the followed entity's box is within
//!   `arrive`; a unit that touched an arrived groupmate heading to the same
//!   point also counts as arrived (the group settles into a blob), but not
//!   in a passage on its way to a goal past it (the collision solver tells:
//!   it would stop in the gap the rest still need).
//! * After its last waypoint a unit walks straight at the goal (the box
//!   surface of a followed entity) until it arrives; the end of a partial
//!   path means the goal cannot be reached: `Failed`.
//! * Speed is `Stats::speed` x the terrain multiplier under the unit x a
//!   facing factor, so slow-turning machines turn on the spot first.
//! * Ground units look ahead along their heading and veer right of units in
//!   the way (left of ones on their right), so head-on pairs pass each other,
//!   unless the veer would take them into a wall just ahead.
//! * A unit faces along the path segment it follows while it moves within
//!   [`FACE_ALONG`] of it: getting back onto the path and dodging slide the
//!   box sideways instead of slewing it across a corridor, where a long
//!   rider would jam against the one beside it (translation cannot undo a
//!   turn).
//! * A ground unit turns only as far as it does not swing its box deeper
//!   into a neighbour's (as the tick began) than [`TURN_SLACK`], or than it
//!   already was: a long box turning in a crowd waits for the room the
//!   collision solver makes instead of cutting into units with none to give.

use std::f32::consts::FRAC_PI_2;

use af_core::prelude::af_geom::{turn_toward, wrap_angle, yaw_of};
use af_core::prelude::*;
use bevy::prelude::*;

use crate::bodies::BodyGrid;
use crate::mover::{FollowTarget, Mover};

/// A unit within this distance of an intermediate waypoint moves on to the next.
const WAYPOINT_REACH: f32 = 0.35;
/// Units aim this far (plus their radius) ahead of their projection on the
/// current path segment, so a unit pushed off the cleared corridor returns
/// to it instead of cutting a corner it does not fit around.
const CARROT_AHEAD: f32 = 1.0;
/// Tolerance for reaching the last waypoint when it is not the goal itself.
const LAST_WAYPOINT_REACH: f32 = 0.25;
/// A waypoint the unit has moved past along its leg counts as reached only
/// this close to it (a unit shoved sideways past a corner still rounds it).
const PASSED_WAYPOINT_RANGE: f32 = 2.0;
/// An arrived follower resumes when the gap grows past `arrive` + this.
const RESUME_MARGIN: f32 = 0.2;
/// Seconds of travel the avoidance looks ahead.
const AVOID_LOOKAHEAD: f32 = 0.6;
/// Extra lateral clearance wanted when passing another unit (metres).
const AVOID_MARGIN: f32 = 0.1;
/// How hard avoidance turns the heading (1 = up to 45 degrees).
const AVOID_GAIN: f32 = 1.0;
/// A dodge is only taken when the ground this far (plus the unit's radius)
/// ahead along it is open (metres).
const AVOID_PROBE: f32 = 0.2;
/// A unit keeps facing along its path segment while it moves within this
/// angle (cosine) of it, and turns to its direction of travel beyond.
const FACE_ALONG: f32 = 0.3;
/// Neighbours heading this much the same way are followed, not dodged.
const SAME_WAY: f32 = 0.7;
/// Cell size of the avoidance grid (metres).
const GRID_CELL: f32 = 2.5;
/// Speed never drops below this fraction of free speed when blocked.
const MIN_FLOW: f32 = 0.2;
/// Deepest overlap with a neighbour's box a turn may cut (or than the box
/// already does, if more), metres. The collision solver pushes the
/// neighbour that far aside within the tick; a long box swung deeper into a
/// crowd with no room to give is a jam that pushing cannot untangle (and a
/// tight cap would leave two big units unable ever to turn past each other).
const TURN_SLACK: f32 = 0.1;
/// Fractions of the tick's turn tried, largest first.
const TURN_FRACTIONS: [f32; 3] = [1.0, 0.5, 0.25];

/// A ground unit as seen by avoidance (start-of-tick snapshot).
#[derive(Clone, Copy, Debug)]
struct Neighbor {
    entity: Entity,
    pos: Vec2,
    /// Radius of the circle standing in for the box.
    rho: f32,
    /// Unit heading while moving, else zero.
    dir: Vec2,
    /// The box itself, and its circumscribed radius (room to turn).
    obb: Obb,
    radius: f32,
}

#[derive(Default)]
pub(crate) struct SteerScratch {
    neighbors: Vec<Neighbor>,
    bounds: Vec<(Vec2, Vec2)>,
    grid: BodyGrid,
    max_rho: f32,
    max_radius: f32,
    /// Per-neighbour visit stamps (a body sits in several cells).
    seen: Vec<u32>,
    stamp: u32,
    /// Boxes near a turning unit, and how deep its turned box may cut into each.
    near: Vec<Obb>,
    near_allowed: Vec<f32>,
}

/// Circle radius used for a box in avoidance.
#[inline]
fn rho(fp: &Footprint) -> f32 {
    0.5 * (fp.half.x + fp.half.y)
}

type SteerQuery<'a> = (
    Entity,
    &'a mut Position,
    &'a mut Facing,
    &'a Footprint,
    &'a Mobile,
    &'a Stats,
    Option<&'a MoveGoal>,
    &'a mut PathState,
    &'a mut Mover,
);

pub(crate) fn steer(data: Res<Data>, nav: Res<Nav>, mut scratch: Local<SteerScratch>, mut units: Query<SteerQuery, Without<Dead>>) {
    let s = &mut *scratch;
    s.neighbors.clear();
    s.bounds.clear();
    s.max_rho = 0.0;
    s.max_radius = 0.0;
    for (entity, pos, facing, fp, mob, _, goal, path, mut mover) in &mut units {
        mover.tick_start = pos.0;
        mover.intended = Vec2::ZERO;
        if mob.flying {
            continue;
        }
        let r = rho(fp);
        let dir = match (goal, path.waypoints.get(path.next)) {
            (Some(_), Some(wp)) if path.status == PathStatus::Following => (*wp - pos.0).normalize_or_zero(),
            _ => Vec2::ZERO,
        };
        let obb = fp.obb(pos.0, facing.0);
        s.neighbors.push(Neighbor { entity, pos: pos.0, rho: r, dir, obb, radius: fp.half.length() });
        s.bounds.push((pos.0 - Vec2::splat(r), pos.0 + Vec2::splat(r)));
        s.max_rho = s.max_rho.max(r);
        s.max_radius = s.max_radius.max(fp.half.length());
    }
    s.grid.rebuild(nav.origin(), nav.world_size(), GRID_CELL, &s.bounds);
    s.seen.clear();
    s.seen.resize(s.neighbors.len(), 0);

    for (entity, mut pos, mut facing, fp, mob, stats, goal, mut path, mut mover) in &mut units {
        let Some(goal) = goal else { continue };
        let me = fp.obb(pos.0, facing.0);
        let steering = match path.status {
            PathStatus::Following => true,
            // A followed entity that moved off: chase it straight away (the
            // path's end leads to its live box) and ask for a fresh path.
            PathStatus::Arrived => match mover.follow {
                Some(f) if goal.follow.is_some() && me.gap(&f.obb) > goal.arrive + RESUME_MARGIN => {
                    path.status = PathStatus::Following;
                    mover.wants_repath = true;
                    true
                }
                _ => false,
            },
            PathStatus::Idle | PathStatus::Pending | PathStatus::Failed => false,
        };
        if !steering {
            continue;
        }
        if goal.follow.is_some() && mover.follow.is_none() {
            path.status = PathStatus::Failed;
            continue;
        }
        if arrived(goal, mover.follow.as_ref(), &me) || (mover.crowd_contact && goal.follow.is_none()) {
            path.status = PathStatus::Arrived;
            continue;
        }
        let Some(leg) = next_leg(goal, mob, pos.0, &mut path, &mut mover) else { continue };
        let desired = leg.target - pos.0;
        let dist = desired.length();
        if dist < 1e-4 {
            continue;
        }
        let terrain = TerrainId(nav.terrain_at(nav.cell_of(pos.0)));
        let speed = stats.speed * data.terrain_speed(terrain, mob.mobility) * mover.flow.max(MIN_FLOW);
        let mut dir = desired / dist;
        if !mob.flying {
            let veered = s.avoid(entity, pos.0, dir, rho(fp), speed, leg.remaining);
            // Never dodge into a wall: in a passage, swinging a long box
            // across it jams it against whoever it meant to pass.
            let probe = |d: Vec2| open_ground(&nav, pos.0 + d * (rho(fp) + AVOID_PROBE), mob.mobility.0);
            if veered == dir || probe(veered) || !probe(dir) {
                dir = veered;
            }
        }
        // Small corrections back onto the path, and dodges, slide the box
        // sideways: it keeps facing along the path, so a long rider stays
        // lined up with the corridor it is in.
        let face = match leg.along {
            Some(along) if along.dot(dir) >= FACE_ALONG => along,
            _ => dir,
        };
        let want = yaw_of(face);
        let mut turned = turn_toward(facing.0, want, mob.turn_rate * SIM_DT);
        if !mob.flying && turned != facing.0 {
            turned = s.room_to_turn(entity, pos.0, fp.half, facing.0, turned);
        }
        let off = wrap_angle(want - turned).abs();
        let factor = if off >= FRAC_PI_2 { 0.0 } else { off.cos() };
        let mut step = speed * factor * SIM_DT;
        if leg.last {
            step = step.min(dist);
        }
        if turned != facing.0 {
            facing.0 = turned;
        }
        if step > 0.0 {
            pos.0 += dir * step;
            mover.intended = dir * step;
        }
    }
}

/// Movers may use the nav cell under `p` (terrain and static obstacles;
/// gates aside).
fn open_ground(nav: &NavGrid, p: Vec2, mobility: u16) -> bool {
    let c = nav.cell_of(p);
    nav.terrain_passable(c, mobility) && !nav.is_walk_blocked(c)
}

/// Is the goal fulfilled from the box `me`?
pub(crate) fn arrived(goal: &MoveGoal, follow: Option<&FollowTarget>, me: &Obb) -> bool {
    match (goal.follow, follow) {
        (Some(_), Some(f)) => me.gap(&f.obb) <= goal.arrive,
        (Some(_), None) => false,
        (None, _) => me.center.distance(goal.target) <= goal.arrive,
    }
}

/// Where to head this tick.
struct Leg {
    target: Vec2,
    /// `target` is the final point (do not overshoot it).
    last: bool,
    /// Distance left along the path to its end (for avoidance).
    remaining: f32,
    /// Direction of the path segment being followed (`None` past the last
    /// waypoint and for flyers).
    along: Option<Vec2>,
}

/// Advance past reached waypoints and pick the point to head for; `None`
/// when the unit should stand (end of a partial path). Writes `path` only
/// when something changes (change detection stays meaningful).
fn next_leg(goal: &MoveGoal, mob: &Mobile, pos: Vec2, path: &mut Mut<PathState>, mover: &mut Mover) -> Option<Leg> {
    if mob.flying {
        // Flyers aim at the live goal.
        let target = match mover.follow {
            Some(f) => f.obb.closest_point(pos),
            None => goal.target,
        };
        return Some(Leg { target, last: true, remaining: pos.distance(target), along: None });
    }
    while let Some(&wp) = path.waypoints.get(path.next) {
        let is_last = path.next + 1 == path.waypoints.len();
        let d = pos.distance(wp);
        let reached = if is_last {
            d <= LAST_WAYPOINT_REACH
        } else {
            let passed = d < PASSED_WAYPOINT_RANGE && (wp - leg_start(path, mover)).dot(pos - wp) > 0.0;
            d <= WAYPOINT_REACH || passed
        };
        if !reached {
            break;
        }
        path.next += 1;
    }
    if let Some(&wp) = path.waypoints.get(path.next) {
        let mut remaining = pos.distance(wp);
        for pair in path.waypoints[path.next..].windows(2) {
            remaining += pair[0].distance(pair[1]);
        }
        let start = leg_start(path, mover);
        let target = carrot(start, wp, pos, CARROT_AHEAD + mob.radius);
        let along = (wp - start).try_normalize();
        return Some(Leg { target, last: path.next + 1 == path.waypoints.len(), remaining, along });
    }
    // Past the last waypoint.
    if path.partial {
        if mover.truncated {
            mover.wants_repath = true;
        } else {
            path.status = PathStatus::Failed;
        }
        return None;
    }
    let target = match mover.follow {
        Some(f) => f.obb.closest_point(pos),
        None => goal.target,
    };
    Some(Leg { target, last: true, remaining: pos.distance(target), along: None })
}

/// Where the current path segment (toward `path.next`) begins.
fn leg_start(path: &PathState, mover: &Mover) -> Vec2 {
    if path.next == 0 { mover.path_start } else { path.waypoints[path.next - 1] }
}

/// The point `ahead` metres past `pos`'s projection on segment `a`-`b`,
/// never beyond `b`.
fn carrot(a: Vec2, b: Vec2, pos: Vec2, ahead: f32) -> Vec2 {
    let ab = b - a;
    let len = ab.length();
    if len < 1e-4 {
        return b;
    }
    let dir = ab / len;
    let along = (pos - a).dot(dir).clamp(0.0, len);
    a + dir * (along + ahead).min(len)
}

impl SteerScratch {
    /// The yaw a ground unit (`me`, its box of half extents `half` at `pos`
    /// facing `from`) turning toward `to` may take this tick: `to`, or the
    /// largest part of the turn that cuts no deeper into any neighbour's box
    /// (as the tick began) than [`TURN_SLACK`] or than the box already cuts
    /// into that one, else no turn at all. A rider in a crowd thus turns as
    /// the crowd makes room, instead of swinging into units that have none
    /// to give; and one already overlapping a unit (trained onto it) may
    /// not swing that deep into another.
    fn room_to_turn(&mut self, me: Entity, pos: Vec2, half: Vec2, from: f32, to: f32) -> f32 {
        let radius = half.length();
        let reach = Vec2::splat(radius + self.max_radius);
        let [x0, y0, x1, y1] = self.grid.range(pos - reach, pos + reach);
        self.stamp = self.stamp.wrapping_add(1);
        self.near.clear();
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &i in self.grid.items(self.grid.cell_index(x, y)) {
                    let i = i as usize;
                    if self.seen[i] == self.stamp {
                        continue;
                    }
                    self.seen[i] = self.stamp;
                    let n = &self.neighbors[i];
                    if n.entity != me && n.pos.distance(pos) < radius + n.radius {
                        self.near.push(n.obb);
                    }
                }
            }
        }
        if self.near.is_empty() {
            return to;
        }
        let depth = |yaw: f32, o: &Obb| Obb::new(pos, half, yaw).mtv(o).map_or(0.0, |m| m.length());
        self.near_allowed.clear();
        self.near_allowed.extend(self.near.iter().map(|o| depth(from, o).max(TURN_SLACK)));
        let (near, allowed) = (&self.near, &self.near_allowed);
        let fits = |yaw: f32| near.iter().zip(allowed).all(|(o, limit)| depth(yaw, o) <= *limit);
        let delta = wrap_angle(to - from);
        TURN_FRACTIONS
            .iter()
            .map(|f| if *f == 1.0 { to } else { wrap_angle(from + delta * f) })
            .find(|yaw| fits(*yaw))
            .unwrap_or(from)
    }

    /// Bend `dir` around units ahead within the lookahead (not beyond the destination).
    fn avoid(&mut self, me: Entity, pos: Vec2, dir: Vec2, rho: f32, speed: f32, remaining: f32) -> Vec2 {
        let look = rho + 0.75 + speed * AVOID_LOOKAHEAD;
        let reach = Vec2::splat(look + self.max_rho);
        let [x0, y0, x1, y1] = self.grid.range(pos - reach, pos + reach);
        self.stamp = self.stamp.wrapping_add(1);
        let mut push = 0.0f32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &i in self.grid.items(self.grid.cell_index(x, y)) {
                    let i = i as usize;
                    if self.seen[i] == self.stamp {
                        continue;
                    }
                    self.seen[i] = self.stamp;
                    let n = self.neighbors[i];
                    if n.entity == me {
                        continue;
                    }
                    let rel = n.pos - pos;
                    let ahead = rel.dot(dir);
                    if ahead <= 0.0 || ahead > look + n.rho || ahead > remaining + n.rho {
                        continue;
                    }
                    let lateral = dir.perp_dot(rel);
                    let min_sep = rho + n.rho + AVOID_MARGIN;
                    if lateral.abs() >= min_sep || n.dir.dot(dir) > SAME_WAY {
                        continue;
                    }
                    let urgency = (1.0 - ahead / (look + n.rho)) * (1.0 - lateral.abs() / min_sep);
                    // Neighbour on the right (lateral < 0): veer left; else (left or dead ahead) veer right.
                    push += if lateral < -1e-3 { -urgency } else { urgency };
                }
            }
        }
        if push == 0.0 {
            return dir;
        }
        let right = Vec2::new(dir.y, -dir.x);
        (dir + right * push.clamp(-1.0, 1.0) * AVOID_GAIN).normalize_or(dir)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const RIDER: Vec2 = Vec2::new(0.45, 0.95);

    /// Avoidance and turning scratch over a 64 m map holding these boxes.
    fn scratch(boxes: &[(Vec2, Vec2, f32)]) -> SteerScratch {
        let mut s = SteerScratch::default();
        for (i, &(pos, half, yaw)) in boxes.iter().enumerate() {
            let rho = 0.5 * (half.x + half.y);
            let entity = Entity::from_raw_u32(i as u32 + 1).expect("entity");
            s.neighbors.push(Neighbor { entity, pos, rho, dir: Vec2::ZERO, obb: Obb::new(pos, half, yaw), radius: half.length() });
            s.bounds.push((pos - Vec2::splat(rho), pos + Vec2::splat(rho)));
            s.max_rho = s.max_rho.max(rho);
            s.max_radius = s.max_radius.max(half.length());
        }
        s.grid.rebuild(Vec2::ZERO, Vec2::splat(64.0), GRID_CELL, &s.bounds);
        s.seen = vec![0; s.neighbors.len()];
        s
    }

    /// A rider turns freely in the open, only part of the way when a
    /// neighbour stands close, and not at all when either end of its box
    /// would swing into one standing flush beside it.
    #[test]
    fn a_turn_takes_only_the_room_there_is() {
        let me = Entity::from_raw_u32(1).expect("entity");
        let (pos, to) = (Vec2::new(30.0, 30.0), 0.4);
        let open = scratch(&[(pos, RIDER, 0.0)]);
        assert_eq!(open.turn(me, pos, to), to);

        // A rider beside it, a hand's breadth away: the corner swing of the
        // whole turn would cut too deep, a part of it does not.
        let beside = |gap: f32| (pos + Vec2::new(2.0 * RIDER.x + gap, 0.0), RIDER, 0.0);
        let yaw = scratch(&[(pos, RIDER, 0.0), beside(0.12)]).turn(me, pos, to);
        assert!(yaw > 0.0 && yaw < to, "part of the turn: {yaw}");
        // Flush against it: nothing, whichever way (one end or the other
        // swings into it).
        for to in [to, -to] {
            assert_eq!(scratch(&[(pos, RIDER, 0.0), beside(-0.01)]).turn(me, pos, to), 0.0);
        }
    }

    /// A rider whose front end is 0.3 m deep in a peasant at its front left
    /// may turn its front away from it, but only as far as that does not
    /// swing its front right corner more than the slack into the rider
    /// standing flush on its right: each neighbour bounds the turn by its own
    /// depth, not by the deepest of them (which would let the corner cut
    /// 0.3 m into a unit it did not touch).
    #[test]
    fn an_overlap_with_one_neighbour_licenses_no_cut_into_another() {
        let me = Entity::from_raw_u32(1).expect("entity");
        let pos = Vec2::new(30.0, 30.0);
        const PEASANT: Vec2 = Vec2::splat(0.3);
        let front_left = (pos + Vec2::new(-RIDER.x, 0.7), PEASANT, 0.0);
        let front_right = (pos + Vec2::new(2.0 * RIDER.x + 0.02, RIDER.y), RIDER, 0.0);
        let boxes = [(pos, RIDER, 0.0), front_left, front_right];
        let s = scratch(&boxes);
        let cut = |yaw: f32, n: usize| Obb::new(pos, RIDER, yaw).mtv(&s.neighbors[n].obb).map_or(0.0, |m| m.length());
        assert!((cut(0.0, 1) - 0.3).abs() < 1e-4 && cut(0.0, 2) == 0.0);
        assert!(cut(0.2, 1) < 0.3 && cut(0.2, 2) > TURN_SLACK, "half the turn eases the one and cuts into the other");
        let yaw = scratch(&boxes).turn(me, pos, 0.4);
        assert!(yaw > 0.0 && yaw < 0.2, "a little of the turn: {yaw}");
        assert!(cut(yaw, 2) <= TURN_SLACK + 1e-4 && cut(yaw, 1) <= 0.3 + 1e-4, "{} m into the rider, {} m into the peasant", cut(yaw, 2), cut(yaw, 1));
    }

    impl SteerScratch {
        /// `room_to_turn` for the rider `me` at `pos` facing 0, toward `to`.
        fn turn(mut self, me: Entity, pos: Vec2, to: f32) -> f32 {
            self.room_to_turn(me, pos, RIDER, 0.0, to)
        }
    }
}
