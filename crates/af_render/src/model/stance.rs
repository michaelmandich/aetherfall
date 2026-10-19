//! How a worker's model stands to its work.
//!
//! The simulation turns units only to walk and to aim weapons, and stops a
//! worker about 0.7 m short of the box it works on (the gap at which
//! gathering, building and repairing count). The work clips strike
//! straight ahead, `reach` model metres out (`docs/art/peasant.md`). So
//! while a unit works, its model alone steps up to the work and turns to
//! it, until the clip's blows land on the target's drawn surface: the
//! trunk, bush, crystal or rock of a node, the wall of a building.
//!
//! The step goes straight at the target's footprint (square to the face the
//! unit stands at), so workers side by side at one face step in side by
//! side, never into each other; only the turn aims at the drawn surface,
//! which may be narrower (a trunk, a crystal). The sim's position and
//! facing, and with them the footprint and collisions, stay where they are
//! (the selection outline and bars follow the model).

use af_core::prelude::*;
use bevy::math::Vec2;

/// How fast the model turns to its work and back (rad/s).
pub const TURN_RATE: f32 = 6.0;
/// How fast it steps in to its work and back (m/s).
pub const STEP_RATE: f32 = 2.5;
/// The model's feet stay at least this far outside the target's footprint
/// (metres).
pub const CLEARANCE: f32 = 0.15;

/// A work target as the renderer sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkTarget {
    /// Its footprint (the sim's box).
    pub footprint: af_geom::Obb,
    /// The drawn box the work lands on (the footprint itself for a
    /// building; a node's trunk, bush, crystal or rock).
    pub surface: af_geom::Obb,
}

/// The model's pose relative to its unit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lean {
    /// Yaw relative to the unit's (radians).
    pub yaw: f32,
    /// Where the model stands, in the unit's frame (x right, y forward;
    /// metres).
    pub offset: Vec2,
}

impl Lean {
    /// The lean of a unit at `pos` facing `facing` that works on `target`
    /// with a clip reaching `reach` game metres ahead (`None`: it does not
    /// step in, only turns). Upright (the default) when the unit stands
    /// inside the target's footprint (a field it works from within).
    pub fn toward(pos: Vec2, facing: f32, target: &WorkTarget, reach: Option<f32>) -> Lean {
        let to_footprint = target.footprint.closest_point(pos) - pos;
        let gap = to_footprint.length();
        if gap < 1e-3 {
            return Lean::default();
        }
        let n = to_footprint / gap;
        // How far along `n` the unit must step for the surface to be
        // `reach` away (as close as it gets when that is too far aside),
        // never into the footprint nor backwards.
        let to_surface = target.surface.closest_point(pos) - pos;
        let along = to_surface.dot(n);
        let aside = (to_surface - n * along).length();
        let room = (gap - CLEARANCE).max(0.0);
        let step = reach.map_or(0.0, |r| if r > aside { along - (r * r - aside * aside).sqrt() } else { along }).clamp(0.0, room);
        let at = pos + n * step;
        let face = target.surface.closest_point(at) - at;
        let world_yaw = af_geom::yaw_of(if face.length_squared() > 1e-6 { face } else { n });
        let offset = n * step;
        let local = Vec2::new(offset.dot(af_geom::right(facing)), offset.dot(af_geom::forward(facing)));
        Lean { yaw: af_geom::wrap_angle(world_yaw - facing), offset: local }
    }

    /// One frame on from `self` toward `want`: turned at most
    /// [`TURN_RATE`] and moved at most [`STEP_RATE`] times `dt`.
    pub fn step(self, want: Lean, dt: f32) -> Lean {
        let to = want.offset - self.offset;
        let max = STEP_RATE * dt;
        let offset = if to.length() <= max { want.offset } else { self.offset + to.normalize() * max };
        Lean { yaw: af_geom::turn_toward(self.yaw, want.yaw, TURN_RATE * dt), offset }
    }

    /// Where the model stands, in the unit's frame (x right, y forward).
    pub fn offset(self) -> Vec2 {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_geom::Obb;
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn near2(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-4
    }

    /// A pine as drawn: 0.9 m footprint, 0.27 m trunk.
    fn tree(at: Vec2) -> WorkTarget {
        WorkTarget { footprint: Obb::new(at, Vec2::splat(0.45), 0.0), surface: Obb::new(at, Vec2::splat(0.135), 0.0) }
    }

    fn crystal() -> WorkTarget {
        WorkTarget { footprint: Obb::new(Vec2::ZERO, Vec2::splat(0.7), 0.0), surface: Obb::new(Vec2::ZERO, Vec2::splat(0.385), 0.0) }
    }

    #[test]
    fn a_woodcutter_turns_to_the_trunk_and_steps_up_until_the_axe_reaches_it() {
        // Standing 1.44 m east of the trunk (a 0.69 m gap), facing north (+Z).
        let pos = Vec2::new(1.44, 0.0);
        let lean = Lean::toward(pos, 0.0, &tree(Vec2::ZERO), Some(0.95));
        assert!(near(lean.yaw, -FRAC_PI_2), "turned to face west: {}", lean.yaw);
        // The trunk's face is 1.305 m away; the axe reaches 0.95 m. West in
        // the unit's frame (facing +Z, its right is +X) is to its left.
        assert!(near2(lean.offset, Vec2::new(-(1.305 - 0.95), 0.0)), "{:?}", lean.offset);
        // Facing west already: no turn, and the step is straight ahead.
        let west = Lean::toward(pos, -FRAC_PI_2, &tree(Vec2::ZERO), Some(0.95));
        assert!(near(west.yaw, 0.0) && near2(west.offset, Vec2::new(0.0, 1.305 - 0.95)), "{west:?}");
    }

    #[test]
    fn workers_side_by_side_step_in_side_by_side() {
        // Two foragers at one face of a bush step straight at it.
        let bush = WorkTarget { footprint: Obb::new(Vec2::ZERO, Vec2::splat(0.7), 0.0), surface: Obb::new(Vec2::ZERO, Vec2::splat(0.7), 0.0) };
        let a = Lean::toward(Vec2::new(-0.35, -1.7), 0.0, &bush, Some(0.4));
        let b = Lean::toward(Vec2::new(0.35, -1.7), 0.0, &bush, Some(0.4));
        assert!(near(a.yaw, 0.0) && near(b.yaw, 0.0), "{a:?} {b:?}");
        assert!(near2(a.offset, Vec2::new(0.0, 0.6)) && near2(b.offset, a.offset), "{a:?} {b:?}");

        // Off to the side of a crystal narrower than its footprint, the
        // step stays square to the face and only the turn aims at the
        // crystal (to the left). The chisel cannot reach that far aside, so
        // it steps as close as it may.
        let pos = Vec2::new(0.6, -1.68);
        let side = Lean::toward(pos, 0.0, &crystal(), Some(0.1));
        assert!(near2(side.offset, Vec2::new(0.0, 1.68 - 0.7 - CLEARANCE)), "no sideways drift: {side:?}");
        assert!(side.yaw < -0.1, "turned left, toward the crystal: {side:?}");
        // Within reach to the side, it stops where the crystal is `reach` away.
        let lean = Lean::toward(pos, 0.0, &crystal(), Some(0.6));
        let at = pos + lean.offset;
        assert!(near((crystal().surface.closest_point(at) - at).length(), 0.6), "{lean:?}");
    }

    #[test]
    fn a_building_is_faced_square_on_and_a_field_from_within_is_left_alone() {
        let wall = Obb::new(Vec2::ZERO, Vec2::new(4.0, 1.0), 0.0);
        let site = WorkTarget { footprint: wall, surface: wall };
        // Beside a long wall, a builder faces it straight on (+Z), not its middle.
        let lean = Lean::toward(Vec2::new(3.0, -2.0), 0.0, &site, Some(0.5));
        assert!(near(lean.yaw, 0.0), "{}", lean.yaw);
        assert!(near2(lean.offset, Vec2::new(0.0, 0.5)), "1 m from the wall, reaching 0.5 m: {:?}", lean.offset);
        assert_eq!(Lean::toward(Vec2::new(1.0, 0.5), 0.0, &site, Some(0.5)), Lean::default(), "inside a field");
        // Off a corner of a trunk: toward that corner.
        let corner = Lean::toward(Vec2::new(1.0, 1.0), 0.0, &tree(Vec2::ZERO), Some(0.8));
        assert!(near(corner.yaw, af_geom::yaw_of(Vec2::new(-1.0, -1.0))), "{}", corner.yaw);
    }

    #[test]
    fn the_step_in_never_crosses_into_the_footprint_nor_backs_off() {
        // A crystal reached only by a short chisel: the step stops short of the footprint.
        let pos = Vec2::new(0.0, -1.68);
        let lean = Lean::toward(pos, 0.0, &crystal(), Some(0.1));
        assert!(near(lean.yaw, 0.0));
        assert!(near2(lean.offset, Vec2::new(0.0, 1.68 - 0.7 - CLEARANCE)), "{:?}", lean.offset);
        // A long reach from close in: no stepping back.
        assert_eq!(Lean::toward(pos, 0.0, &crystal(), Some(2.0)).offset, Vec2::ZERO);
        // No reach: turn only.
        assert_eq!(Lean::toward(pos, FRAC_PI_4, &crystal(), None), Lean { yaw: -FRAC_PI_4, offset: Vec2::ZERO });
    }

    #[test]
    fn the_model_turns_and_steps_at_bounded_rates() {
        let dt = 1.0 / 60.0;
        let want = Lean { yaw: FRAC_PI_2, offset: Vec2::new(0.0, 0.5) };
        let one = Lean::default().step(want, dt);
        assert!(near(one.yaw, TURN_RATE * dt) && near2(one.offset, Vec2::new(0.0, STEP_RATE * dt)), "{one:?}");
        assert_eq!(Lean { yaw: 1.5, offset: Vec2::new(0.0, 0.49) }.step(want, dt).offset, want.offset, "arrives without overshoot");
        // The short way round, across the seam.
        assert!(near(Lean { yaw: PI - 0.04, offset: Vec2::ZERO }.step(Lean { yaw: -PI + 0.04, offset: Vec2::ZERO }, dt).yaw, -PI + 0.04));
        // Back upright once the work is done.
        let mut l = want;
        for _ in 0..60 {
            l = l.step(Lean::default(), dt);
        }
        assert_eq!(l, Lean::default());
    }
}
