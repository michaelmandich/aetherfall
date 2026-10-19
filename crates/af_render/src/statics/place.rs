//! Pure placement maths for static models: fitting a model to the game's
//! box, the variant and turn picked from a position hash, how far a
//! foundation's model is sunk, and the timelines of a felled tree and its
//! stump. Unit tested without an app.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::manifest::Fit;

/// Per-axis scale that fits a model whose bounds measure `size` to a box of
/// `target` (width, height, depth), as [`Fit`] says.
pub fn fit_scale(fit: Fit, size: Vec3, target: Vec3) -> Vec3 {
    let size = size.max(Vec3::splat(1e-3));
    match fit {
        Fit::Box => target / size,
        Fit::Footprint => Vec3::splat(target.x.max(target.z) / size.x.max(size.z)),
        Fit::Height => Vec3::splat(target.y / size.y),
    }
}

/// The centre of the base of bounds `min..max`: what stands on the ground.
pub fn base_centre(min: Vec3, max: Vec3) -> Vec3 {
    Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5)
}

/// Where a model with bounds `min..max` stands in its owner's frame (x
/// right, y up from the box's base, z forward): fitted to `target`, the
/// centre of its bounds' base on the origin, turned by `yaw` about it.
pub fn fit_transform(fit: Fit, min: Vec3, max: Vec3, target: Vec3, yaw: f32) -> Transform {
    let scale = fit_scale(fit, max - min, target);
    let pivot = base_centre(min, max);
    let rotation = Quat::from_rotation_y(yaw);
    Transform { translation: -(rotation * (scale * pivot)), rotation, scale }
}

/// How far below its rest a foundation's model is drawn at build
/// `progress` (0..1), for a model `height` metres tall: it rises out of the
/// ground as it is built.
pub fn construction_sink(progress: f32, height: f32) -> f32 {
    (1.0 - progress.clamp(0.0, 1.0)) * height.max(0.0)
}

/// `rest` moved `sink` metres down.
pub fn sunk(rest: Transform, sink: f32) -> Transform {
    Transform { translation: rest.translation - Vec3::Y * sink, ..rest }
}

/// Salts that keep the position hashes of different choices apart.
const VARIANT_SALT: u32 = 1;
const YAW_SALT: u32 = 2;
const FALL_SALT: u32 = 3;

/// Murmur3's finaliser: every input bit reaches every output bit.
fn fmix(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

/// A hash of a ground position (the sim's, in 1/64 m steps) and `salt`:
/// the same between runs, machines and players, and independent of the
/// simulation's state and random numbers. Buildings and nodes never move,
/// so what is picked from it stays put.
pub fn position_hash(pos: Vec2, salt: u32) -> u32 {
    let step = |v: f32| (v * 64.0).round() as i32 as u32;
    let h = fmix(salt.wrapping_mul(0x9e37_79b9) ^ step(pos.x));
    fmix(h ^ step(pos.y).rotate_left(16))
}

/// A hash reduced to `0..n` without modulo bias worth the name.
fn reduce(h: u32, n: usize) -> usize {
    ((h as u64 * n as u64) >> 32) as usize
}

/// A fraction in `0..1` from a hash.
fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Which of `count` variants the model at `pos` is drawn with.
pub fn variant_index(pos: Vec2, count: usize) -> usize {
    if count == 0 { 0 } else { reduce(position_hash(pos, VARIANT_SALT), count) }
}

/// The turn (radians, `0..TAU`) of a `random_yaw` model at `pos`.
pub fn hashed_yaw(pos: Vec2) -> f32 {
    unit(position_hash(pos, YAW_SALT)) * TAU
}

/// A ground direction picked by position, for a tree no worker felled.
pub fn hashed_direction(pos: Vec2) -> Vec2 {
    let a = unit(position_hash(pos, FALL_SALT)) * TAU;
    Vec2::new(a.sin(), a.cos())
}

/// Which way a tree at `tree` falls: away from the worker who felled it
/// (at `feller`), else a hashed direction. A unit vector on the ground.
pub fn fall_direction(tree: Vec2, feller: Option<Vec2>) -> Vec2 {
    feller.map(|w| tree - w).filter(|d| d.length_squared() > 1e-6).map_or_else(|| hashed_direction(tree), Vec2::normalize)
}

/// Seconds a felled tree takes to fall to the ground.
pub const FALL_TIME: f32 = 1.2;
/// Seconds it then lies there.
pub const FALLEN_HOLD: f32 = 1.5;
/// Seconds it then takes to sink away.
pub const FALLEN_SINK: f32 = 2.0;
/// How far it tips (radians): not quite flat, as the canopy props it up.
pub const FALLEN_TILT: f32 = 85.0 * std::f32::consts::PI / 180.0;
/// Seconds a stump stands before it sinks.
pub const STUMP_HOLD: f32 = 90.0;
/// Seconds a stump (or anything else left lying) takes to sink away.
pub const STUMP_SINK: f32 = 3.0;

/// A falling tree's pose: how far it has tipped (radians) and how far it
/// has sunk (a fraction of the depth it sinks to).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FallPose {
    pub tilt: f32,
    pub sink: f32,
}

/// A felled tree `age` seconds after its wood ran out: it tips over,
/// faster and faster as if under gravity, for [`FALL_TIME`]; lies still
/// for [`FALLEN_HOLD`]; sinks for [`FALLEN_SINK`]; then it is gone (`None`).
pub fn fall_pose(age: f32) -> Option<FallPose> {
    let age = age.max(0.0);
    if age < FALL_TIME {
        let t = age / FALL_TIME;
        return Some(FallPose { tilt: FALLEN_TILT * t * t, sink: 0.0 });
    }
    let sinking = age - FALL_TIME - FALLEN_HOLD;
    if sinking < 0.0 {
        return Some(FallPose { tilt: FALLEN_TILT, sink: 0.0 });
    }
    (sinking < FALLEN_SINK).then(|| FallPose { tilt: FALLEN_TILT, sink: sinking / FALLEN_SINK })
}

/// How far something sinking for `duration` seconds to `depth` has gone
/// `elapsed` seconds in; `None` once it is under.
pub fn sink_drop(elapsed: f32, duration: f32, depth: f32) -> Option<f32> {
    let t = elapsed.max(0.0) / duration.max(1e-3);
    (t < 1.0).then_some(t * depth)
}

/// `rest` (a world transform) tipped by `tilt` radians toward the ground
/// direction `dir`, about `pivot` (the base of the trunk), and `drop`
/// metres down.
pub fn tipped(rest: Transform, pivot: Vec3, dir: Vec2, tilt: f32, drop: f32) -> Transform {
    let axis = Vec3::Y.cross(Vec3::new(dir.x, 0.0, dir.y)).normalize_or(Vec3::X);
    let mut t = rest;
    t.rotate_around(pivot, Quat::from_axis_angle(axis, tilt));
    t.translation.y -= drop;
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    /// The box corners a model's bounds land on.
    fn placed_bounds(t: &Transform, min: Vec3, max: Vec3) -> (Vec3, Vec3) {
        let corners = (0..8).map(|i| Vec3::new(if i & 1 == 0 { min.x } else { max.x }, if i & 2 == 0 { min.y } else { max.y }, if i & 4 == 0 { min.z } else { max.z }));
        corners.map(|c| t.transform_point(c)).fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)))
    }

    #[test]
    fn box_fit_matches_the_box_exactly_per_axis() {
        // A model off-centre and below zero: it still lands on the box.
        let (min, max) = (Vec3::new(-1.0, -0.5, 0.0), Vec3::new(3.0, 2.5, 2.0));
        let target = Vec3::new(4.0, 3.0, 4.0);
        let t = fit_transform(Fit::Box, min, max, target, 0.0);
        assert!(close(t.scale, Vec3::new(1.0, 1.0, 2.0)), "{}", t.scale);
        let (lo, hi) = placed_bounds(&t, min, max);
        assert!(close(lo, Vec3::new(-2.0, 0.0, -2.0)) && close(hi, Vec3::new(2.0, 3.0, 2.0)), "{lo} {hi}");
    }

    #[test]
    fn footprint_fit_matches_the_larger_side_and_height_follows() {
        let (min, max) = (Vec3::new(-1.0, 0.0, -0.5), Vec3::new(1.0, 1.0, 0.5));
        let t = fit_transform(Fit::Footprint, min, max, Vec3::new(2.4, 1.6, 3.0), 0.0);
        assert!(close(t.scale, Vec3::splat(1.5)), "the 2 m side becomes the box's 3 m: {}", t.scale);
        let (lo, hi) = placed_bounds(&t, min, max);
        assert!(close(lo, Vec3::new(-1.5, 0.0, -0.75)) && close(hi, Vec3::new(1.5, 1.5, 0.75)), "{lo} {hi}");
    }

    #[test]
    fn height_fit_matches_the_height_and_the_canopy_may_spread() {
        let (min, max) = (Vec3::new(-1.4, 0.0, -1.3), Vec3::new(1.4, 6.3, 1.3));
        let t = fit_transform(Fit::Height, min, max, Vec3::new(0.9, 6.0, 0.9), 0.0);
        let s = 6.0 / 6.3;
        assert!(close(t.scale, Vec3::splat(s)));
        let (lo, hi) = placed_bounds(&t, min, max);
        assert!((hi.y - 6.0).abs() < 1e-4 && lo.y.abs() < 1e-4);
        assert!(hi.x > 0.45, "wider than the 0.9 m trunk box");
    }

    #[test]
    fn a_turned_model_turns_about_its_own_centre() {
        let (min, max) = (Vec3::new(1.0, 0.0, 1.0), Vec3::new(3.0, 2.0, 2.0));
        for yaw in [0.3, 1.0, 2.5, 4.0] {
            let t = fit_transform(Fit::Footprint, min, max, Vec3::splat(2.0), yaw);
            let base_centre = t.transform_point(Vec3::new(2.0, 0.0, 1.5));
            assert!(close(base_centre, Vec3::ZERO), "yaw {yaw}: {base_centre}");
            let front = t.rotation * Vec3::Z;
            assert!(close(front, Quat::from_rotation_y(yaw) * Vec3::Z));
        }
    }

    #[test]
    fn foundations_rise_with_progress() {
        assert_eq!(construction_sink(0.0, 3.0), 3.0);
        assert!((construction_sink(0.25, 3.0) - 2.25).abs() < 1e-6);
        assert_eq!(construction_sink(1.0, 3.0), 0.0);
        assert_eq!(construction_sink(1.5, 3.0), 0.0, "clamped");
        assert_eq!(construction_sink(-1.0, 3.0), 3.0, "clamped");
        let rest = Transform::from_xyz(1.0, 0.5, 2.0).with_scale(Vec3::splat(2.0));
        let t = sunk(rest, 1.5);
        assert_eq!(t.translation, Vec3::new(1.0, -1.0, 2.0));
        assert_eq!((t.rotation, t.scale), (rest.rotation, rest.scale));
    }

    /// Pinned values: changing the hash re-dresses every map, so it must
    /// be deliberate.
    #[test]
    fn position_hashes_are_stable() {
        assert_eq!(position_hash(Vec2::new(0.0, 0.0), 0), 0);
        assert_eq!(position_hash(Vec2::new(12.5, 40.25), VARIANT_SALT), 0xf9c1_ad47);
        assert_eq!(position_hash(Vec2::new(-3.0, 7.0), YAW_SALT), 0x7f2e_39db);
        assert_eq!(variant_index(Vec2::new(12.5, 40.25), 3), 2);
        assert_eq!(variant_index(Vec2::new(100.0, 3.5), 3), 1);
        assert!((hashed_yaw(Vec2::new(-3.0, 7.0)) - 3.121_480_5).abs() < 1e-5, "{}", hashed_yaw(Vec2::new(-3.0, 7.0)));
        // Sub-step noise does not change the pick; a real move does.
        let p = Vec2::new(55.0, 21.0);
        assert_eq!(position_hash(p, 1), position_hash(p + Vec2::splat(0.004), 1));
        assert_ne!(position_hash(p, 1), position_hash(p + Vec2::new(0.5, 0.0), 1));
        // Salts give independent choices; x and z are not interchangeable.
        assert_ne!(position_hash(p, VARIANT_SALT), position_hash(p, YAW_SALT));
        assert_ne!(position_hash(Vec2::new(3.0, 8.0), 1), position_hash(Vec2::new(8.0, 3.0), 1));
    }

    #[test]
    fn variants_and_turns_spread_evenly_over_a_forest() {
        let mut counts = [0usize; 3];
        let mut quadrants = [0usize; 4];
        for x in 0..40 {
            for z in 0..40 {
                let p = Vec2::new(x as f32 * 1.7 + 0.3, z as f32 * 2.3 - 5.0);
                counts[variant_index(p, 3)] += 1;
                let yaw = hashed_yaw(p);
                assert!((0.0..TAU).contains(&yaw));
                quadrants[(yaw / (TAU / 4.0)) as usize] += 1;
            }
        }
        for c in counts {
            assert!((450..620).contains(&c), "{counts:?}");
        }
        for q in quadrants {
            assert!((330..470).contains(&q), "{quadrants:?}");
        }
        assert_eq!(variant_index(Vec2::ONE, 1), 0);
        assert_eq!(variant_index(Vec2::ONE, 0), 0);
    }

    #[test]
    fn a_tree_falls_away_from_its_feller() {
        let d = fall_direction(Vec2::new(10.0, 10.0), Some(Vec2::new(9.0, 10.0)));
        assert!((d - Vec2::X).length() < 1e-6);
        let h = fall_direction(Vec2::new(10.0, 10.0), None);
        assert!((h.length() - 1.0).abs() < 1e-5);
        assert_eq!(h, fall_direction(Vec2::new(10.0, 10.0), Some(Vec2::new(10.0, 10.0))), "a feller on the spot gives no direction");
        assert_eq!(h, hashed_direction(Vec2::new(10.0, 10.0)), "stable");
        // Tipping about the base toward +X: the top lands on the +X side.
        let rest = Transform::from_xyz(10.0, 0.0, 10.0);
        let t = tipped(rest, Vec3::new(10.0, 0.0, 10.0), d, FALLEN_TILT, 0.0);
        let top = t.transform_point(Vec3::Y * 6.0);
        assert!(top.x > 15.9 && top.y < 0.6 && (top.z - 10.0).abs() < 1e-4, "{top}");
        let t = tipped(rest, Vec3::new(10.0, 0.0, 10.0), Vec2::new(0.0, -1.0), FALLEN_TILT, 0.5);
        let top = t.transform_point(Vec3::Y * 6.0);
        assert!(top.z < 4.1 && top.y < 0.1, "falls toward -Z and sinks: {top}");
    }

    #[test]
    fn the_fall_timeline() {
        let at = |t: f32| fall_pose(t).expect("still there");
        assert_eq!(at(0.0), FallPose { tilt: 0.0, sink: 0.0 });
        assert!(at(FALL_TIME * 0.5).tilt < FALLEN_TILT * 0.5, "it speeds up as it falls");
        assert!(at(FALL_TIME * 0.9).tilt > at(FALL_TIME * 0.5).tilt);
        assert_eq!(at(FALL_TIME), FallPose { tilt: FALLEN_TILT, sink: 0.0 }, "on the ground");
        assert_eq!(at(FALL_TIME + FALLEN_HOLD * 0.9).sink, 0.0, "lying still");
        let half = at(FALL_TIME + FALLEN_HOLD + FALLEN_SINK * 0.5);
        assert!((half.sink - 0.5).abs() < 1e-5 && half.tilt == FALLEN_TILT);
        assert!(fall_pose(FALL_TIME + FALLEN_HOLD + FALLEN_SINK + 0.01).is_none(), "gone");
        const { assert!(FALL_TIME + FALLEN_HOLD + FALLEN_SINK < 6.0, "felled trees do not linger") };
        assert_eq!(sink_drop(0.0, STUMP_SINK, 0.6), Some(0.0));
        assert!((sink_drop(STUMP_SINK * 0.5, STUMP_SINK, 0.6).unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(sink_drop(STUMP_SINK, STUMP_SINK, 0.6), None);
    }
}
