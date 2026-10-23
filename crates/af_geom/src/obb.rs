//! Oriented boxes on the ground plane.
//!
//! Ground-plane convention used everywhere in Aetherfall: a `Vec2` is
//! `(world x, world z)`. A yaw of 0 faces +Z. With yaw `a`:
//! `forward = (sin a, cos a)`, `right = (cos a, -sin a)`, matching Bevy's
//! `Quat::from_rotation_y(a)` applied to +Z and +X.

use bevy_math::{Vec2, Vec3};

/// Unit forward vector for a yaw.
#[inline]
pub fn forward(yaw: f32) -> Vec2 {
    Vec2::new(crate::dmath::sin(yaw), crate::dmath::cos(yaw))
}

/// Unit right vector for a yaw.
#[inline]
pub fn right(yaw: f32) -> Vec2 {
    Vec2::new(crate::dmath::cos(yaw), -crate::dmath::sin(yaw))
}

/// Yaw that faces along `dir` (any length > 0).
#[inline]
pub fn yaw_of(dir: Vec2) -> f32 {
    crate::dmath::atan2(dir.x, dir.y)
}

/// Wrap an angle to (-PI, PI].
#[inline]
pub fn wrap_angle(a: f32) -> f32 {
    let mut a = a % std::f32::consts::TAU;
    if a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    } else if a <= -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

/// Rotate `from` toward `to` by at most `max_step` radians.
#[inline]
pub fn turn_toward(from: f32, to: f32, max_step: f32) -> f32 {
    let d = wrap_angle(to - from);
    if d.abs() <= max_step { to } else { wrap_angle(from + max_step * d.signum()) }
}

/// An oriented box footprint on the ground plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obb {
    pub center: Vec2,
    /// Half extents: x along `right`, y along `forward`.
    pub half: Vec2,
    pub yaw: f32,
}

impl Obb {
    pub fn new(center: Vec2, half: Vec2, yaw: f32) -> Obb {
        Obb { center, half, yaw }
    }

    /// From a def box size `[w, h, d]`.
    pub fn from_size(center: Vec2, size: [f32; 3], yaw: f32) -> Obb {
        Obb { center, half: Vec2::new(size[0] * 0.5, size[2] * 0.5), yaw }
    }

    /// (right, forward) unit axes.
    #[inline]
    pub fn axes(&self) -> (Vec2, Vec2) {
        (right(self.yaw), forward(self.yaw))
    }

    /// The box with its axes worked out, for several queries in a row.
    /// Every `Obb` query goes through one and gives the same result.
    #[inline]
    pub fn frame(&self) -> Frame {
        Frame { center: self.center, half: self.half, right: right(self.yaw), forward: forward(self.yaw) }
    }

    pub fn corners(&self) -> [Vec2; 4] {
        self.frame().corners()
    }

    /// Axis-aligned bounds (min, max).
    pub fn aabb(&self) -> (Vec2, Vec2) {
        self.frame().aabb()
    }

    /// Radius of the circumscribed circle.
    #[inline]
    pub fn bounding_radius(&self) -> f32 {
        self.half.length()
    }

    /// Point in this box's local frame (x along right, y along forward).
    #[inline]
    pub fn to_local(&self, p: Vec2) -> Vec2 {
        self.frame().to_local(p)
    }

    #[inline]
    pub fn to_world(&self, local: Vec2) -> Vec2 {
        self.frame().to_world(local)
    }

    pub fn contains(&self, p: Vec2) -> bool {
        self.frame().contains(p)
    }

    pub fn closest_point(&self, p: Vec2) -> Vec2 {
        self.frame().closest_point(p)
    }

    /// Distance from `p` to the box (0 inside).
    pub fn distance_to_point(&self, p: Vec2) -> f32 {
        self.frame().distance_to_point(p)
    }

    /// Half-width of the box projected on unit direction `dir`.
    #[inline]
    pub fn support(&self, dir: Vec2) -> f32 {
        self.frame().support(dir)
    }

    /// Separating-axis overlap test (touching counts as not overlapping).
    pub fn overlaps(&self, other: &Obb) -> bool {
        self.frame().overlaps(&other.frame())
    }

    /// Minimum translation vector that moves `self` out of `other`, or None
    /// if they do not overlap. The vector points away from `other`.
    pub fn mtv(&self, other: &Obb) -> Option<Vec2> {
        self.frame().mtv(&other.frame())
    }

    /// Exact edge-to-edge distance between two boxes (0 when they overlap
    /// or touch). Two disjoint convex polygons are closest at a corner of
    /// one of them, so this is the smallest corner-to-box distance both
    /// ways. Used for weapon ranges, arrival and reach checks.
    pub fn gap(&self, other: &Obb) -> f32 {
        self.frame().gap(&other.frame())
    }

    /// Distance from the segment `a`-`b` to the box (0 if it touches/crosses).
    pub fn segment_distance(&self, a: Vec2, b: Vec2) -> f32 {
        self.frame().segment_distance(a, b)
    }

    /// Ray (3D) vs this box extruded from `y0` to `y1`. Returns the distance
    /// along `dir` (dir need not be normalised; result is in units of `dir`).
    pub fn ray_hit(&self, origin: Vec3, dir: Vec3, y0: f32, y1: f32) -> Option<f32> {
        let (r, f) = self.axes();
        let o = Vec2::new(origin.x, origin.z) - self.center;
        let d = Vec2::new(dir.x, dir.z);
        let lo = Vec3::new(o.dot(r), origin.y, o.dot(f));
        let ld = Vec3::new(d.dot(r), dir.y, d.dot(f));
        let min = Vec3::new(-self.half.x, y0, -self.half.y);
        let max = Vec3::new(self.half.x, y1, self.half.y);
        let mut tmin = f32::NEG_INFINITY;
        let mut tmax = f32::INFINITY;
        for i in 0..3 {
            let (o, d, mn, mx) = (lo[i], ld[i], min[i], max[i]);
            if d.abs() < 1e-8 {
                if o < mn || o > mx {
                    return None;
                }
            } else {
                let t1 = (mn - o) / d;
                let t2 = (mx - o) / d;
                tmin = tmin.max(t1.min(t2));
                tmax = tmax.min(t1.max(t2));
            }
        }
        if tmax >= tmin.max(0.0) { Some(tmin.max(0.0)) } else { None }
    }
}

/// An [`Obb`] with its unit axes stored, so queries need no trigonometry.
/// Get one with [`Obb::frame`]; move it by setting `center`. Hot loops that
/// test one box against many keep a `Frame` instead of an `Obb`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub center: Vec2,
    /// Half extents: x along `right`, y along `forward`.
    pub half: Vec2,
    pub right: Vec2,
    pub forward: Vec2,
}

impl Frame {
    pub fn corners(&self) -> [Vec2; 4] {
        let rx = self.right * self.half.x;
        let fy = self.forward * self.half.y;
        [
            self.center - rx - fy,
            self.center + rx - fy,
            self.center + rx + fy,
            self.center - rx + fy,
        ]
    }

    /// Half extents of the axis-aligned bounds.
    #[inline]
    pub fn extents(&self) -> Vec2 {
        let (r, f) = (self.right, self.forward);
        Vec2::new(r.x.abs() * self.half.x + f.x.abs() * self.half.y, r.y.abs() * self.half.x + f.y.abs() * self.half.y)
    }

    /// Axis-aligned bounds (min, max).
    #[inline]
    pub fn aabb(&self) -> (Vec2, Vec2) {
        let ext = self.extents();
        (self.center - ext, self.center + ext)
    }

    /// Point in the box's local frame (x along right, y along forward).
    #[inline]
    pub fn to_local(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        Vec2::new(d.dot(self.right), d.dot(self.forward))
    }

    #[inline]
    pub fn to_world(&self, local: Vec2) -> Vec2 {
        self.center + self.right * local.x + self.forward * local.y
    }

    pub fn contains(&self, p: Vec2) -> bool {
        let l = self.to_local(p);
        l.x.abs() <= self.half.x && l.y.abs() <= self.half.y
    }

    pub fn closest_point(&self, p: Vec2) -> Vec2 {
        let l = self.to_local(p);
        self.to_world(Vec2::new(l.x.clamp(-self.half.x, self.half.x), l.y.clamp(-self.half.y, self.half.y)))
    }

    /// Distance from `p` to the box (0 inside).
    pub fn distance_to_point(&self, p: Vec2) -> f32 {
        let l = self.to_local(p);
        let d = Vec2::new((l.x.abs() - self.half.x).max(0.0), (l.y.abs() - self.half.y).max(0.0));
        d.length()
    }

    /// Half-width of the box projected on unit direction `dir`.
    #[inline]
    pub fn support(&self, dir: Vec2) -> f32 {
        dir.dot(self.right).abs() * self.half.x + dir.dot(self.forward).abs() * self.half.y
    }

    /// Projection interval of the box on axis `axis` (unit length).
    #[inline]
    fn project(&self, axis: Vec2) -> (f32, f32) {
        let c = self.center.dot(axis);
        let e = self.support(axis);
        (c - e, c + e)
    }

    /// How far the two boxes' projections on unit `axis` overlap: the
    /// distance `self` must move along it to clear `other` that way
    /// (0 or less when the axis separates them).
    #[inline]
    pub fn overlap_along(&self, other: &Frame, axis: Vec2) -> f32 {
        let (a0, a1) = self.project(axis);
        let (b0, b1) = other.project(axis);
        (a1 - b0).min(b1 - a0)
    }

    /// Separating-axis overlap test (touching counts as not overlapping).
    pub fn overlaps(&self, other: &Frame) -> bool {
        for axis in [self.right, self.forward, other.right, other.forward] {
            let (a0, a1) = self.project(axis);
            let (b0, b1) = other.project(axis);
            if a1 <= b0 || b1 <= a0 {
                return false;
            }
        }
        true
    }

    /// Minimum translation vector that moves `self` out of `other`, or None
    /// if they do not overlap. The vector points away from `other`.
    pub fn mtv(&self, other: &Frame) -> Option<Vec2> {
        let mut best_depth = f32::INFINITY;
        let mut best_axis = Vec2::ZERO;
        for axis in [self.right, self.forward, other.right, other.forward] {
            let depth = self.overlap_along(other, axis);
            if depth <= 0.0 {
                return None;
            }
            if depth < best_depth {
                best_depth = depth;
                best_axis = axis;
            }
        }
        let dir = self.center - other.center;
        if dir.dot(best_axis) < 0.0 {
            best_axis = -best_axis;
        }
        Some(best_axis * best_depth)
    }

    /// Exact edge-to-edge distance between two boxes (see [`Obb::gap`]).
    pub fn gap(&self, other: &Frame) -> f32 {
        let separated = [self.right, self.forward, other.right, other.forward].into_iter().any(|axis| {
            let (a0, a1) = self.project(axis);
            let (b0, b1) = other.project(axis);
            a1 <= b0 || b1 <= a0
        });
        if !separated {
            return 0.0;
        }
        let a_to_b = self.corners().into_iter().map(|c| other.distance_to_point(c));
        let b_to_a = other.corners().into_iter().map(|c| self.distance_to_point(c));
        a_to_b.chain(b_to_a).fold(f32::MAX, f32::min)
    }

    /// Distance from the segment `a`-`b` to the box (0 if it touches/crosses).
    pub fn segment_distance(&self, a: Vec2, b: Vec2) -> f32 {
        let la = self.to_local(a);
        let lb = self.to_local(b);
        if segment_hits_aabb(la, lb, self.half) {
            return 0.0;
        }
        let mut best = self.distance_to_point(a).min(self.distance_to_point(b));
        for c in self.corners() {
            best = best.min(point_segment_distance(c, a, b));
        }
        best
    }
}

/// Distance from point `p` to segment `a`-`b`.
pub fn point_segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-12 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Liang-Barsky: does segment `a`-`b` touch the AABB `[-half, half]`?
fn segment_hits_aabb(a: Vec2, b: Vec2, half: Vec2) -> bool {
    let d = b - a;
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;
    for (p, q) in [
        (-d.x, a.x + half.x),
        (d.x, half.x - a.x),
        (-d.y, a.y + half.y),
        (d.y, half.y - a.y),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn axes_match_bevy_convention() {
        let f = forward(FRAC_PI_2);
        assert!((f - Vec2::new(1.0, 0.0)).length() < 1e-5);
        let r = right(FRAC_PI_2);
        assert!((r - Vec2::new(0.0, -1.0)).length() < 1e-5);
        assert!((yaw_of(Vec2::new(1.0, 0.0)) - FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn sat_overlap_and_mtv() {
        let a = Obb::new(Vec2::ZERO, Vec2::splat(1.0), 0.0);
        let b = Obb::new(Vec2::new(1.5, 0.0), Vec2::splat(1.0), 0.0);
        assert!(a.overlaps(&b));
        let m = a.mtv(&b).unwrap();
        assert!((m - Vec2::new(-0.5, 0.0)).length() < 1e-5, "{m:?}");
        let c = Obb::new(Vec2::new(3.0, 0.0), Vec2::splat(1.0), 0.0);
        assert!(!a.overlaps(&c));
        assert!((a.gap(&c) - 1.0).abs() < 1e-5);
        // Rotated 45 degrees: diamond reaches sqrt(2).
        let d = Obb::new(Vec2::new(2.3, 0.0), Vec2::splat(1.0), std::f32::consts::FRAC_PI_4);
        assert!(a.overlaps(&d));
    }

    #[test]
    fn gap_is_exact() {
        let a = Obb::new(Vec2::ZERO, Vec2::splat(1.0), 0.0);
        let diagonal = Obb::new(Vec2::new(5.0, 5.0), Vec2::splat(1.0), 0.0);
        assert!((a.gap(&diagonal) - 3.0 * std::f32::consts::SQRT_2).abs() < 1e-4);
        let turned = Obb::new(Vec2::new(4.0, 0.0), Vec2::splat(1.0), std::f32::consts::FRAC_PI_4);
        assert!((a.gap(&turned) - (3.0 - std::f32::consts::SQRT_2)).abs() < 1e-4);
        assert!((a.gap(&turned) - turned.gap(&a)).abs() < 1e-6);
    }

    #[test]
    fn distances() {
        let a = Obb::new(Vec2::ZERO, Vec2::new(2.0, 1.0), 0.0);
        assert_eq!(a.distance_to_point(Vec2::new(1.0, 0.5)), 0.0);
        assert!((a.distance_to_point(Vec2::new(5.0, 0.0)) - 3.0).abs() < 1e-5);
        assert_eq!(a.segment_distance(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)), 0.0);
        assert!((a.segment_distance(Vec2::new(-5.0, 3.0), Vec2::new(5.0, 3.0)) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn ray_picking() {
        let a = Obb::new(Vec2::new(0.0, 10.0), Vec2::splat(1.0), 0.3);
        let hit = a.ray_hit(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, -0.4, 1.0).normalize(), 0.0, 2.0);
        assert!(hit.is_some());
        let miss = a.ray_hit(Vec3::new(5.0, 5.0, 0.0), Vec3::new(0.0, -0.4, 1.0).normalize(), 0.0, 2.0);
        assert!(miss.is_none());
    }

    /// A frame answers every query exactly as its box does.
    #[test]
    fn frames_agree_with_their_boxes() {
        let boxes = [
            Obb::new(Vec2::new(0.3, -0.2), Vec2::new(1.2, 0.7), 0.4),
            Obb::new(Vec2::new(1.4, 0.5), Vec2::new(0.5, 0.9), -2.1),
            Obb::new(Vec2::new(4.0, 3.0), Vec2::splat(1.0), std::f32::consts::FRAC_PI_4),
        ];
        let p = Vec2::new(2.2, -0.7);
        for a in &boxes {
            let fa = a.frame();
            assert_eq!(fa.aabb(), a.aabb());
            assert_eq!(fa.corners(), a.corners());
            assert_eq!(
                (fa.distance_to_point(p), fa.closest_point(p), fa.contains(p)),
                (a.distance_to_point(p), a.closest_point(p), a.contains(p))
            );
            for b in &boxes {
                let fb = b.frame();
                assert_eq!(fa.mtv(&fb), a.mtv(b));
                assert_eq!(fa.overlaps(&fb), a.overlaps(b));
                assert_eq!(fa.gap(&fb), a.gap(b));
            }
        }
    }

    #[test]
    fn turning() {
        let t = turn_toward(3.0, -3.0, 0.1);
        assert!(!(-3.0..=3.0).contains(&t), "turns through PI, not through 0: {t}");
    }
}
