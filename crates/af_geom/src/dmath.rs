//! Trigonometry and powers that give the same bits on every platform.
//!
//! `f32::sin` and friends call the platform's math library (glibc, msvcrt,
//! Apple's libm, ...), which differ in the last bit. A lockstep match
//! simulated on a Windows PC, a Mac and a Raspberry Pi must agree bit for
//! bit, so simulation code uses these (pure Rust, from the `libm` crate)
//! instead. Square roots, rounding and the four operations are exact
//! everywhere and need nothing special.

use bevy_math::{Vec2, Vec3};

#[inline]
pub fn sin(x: f32) -> f32 {
    libm::sinf(x)
}

#[inline]
pub fn cos(x: f32) -> f32 {
    libm::cosf(x)
}

#[inline]
pub fn tan(x: f32) -> f32 {
    libm::tanf(x)
}

#[inline]
pub fn atan2(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

#[inline]
pub fn powf(base: f32, exp: f32) -> f32 {
    libm::powf(base, exp)
}

/// `(cos a, sin a)`: the unit vector at angle `a` from +X, as
/// `Vec2::from_angle` gives it.
#[inline]
pub fn from_angle(a: f32) -> Vec2 {
    Vec2::new(cos(a), sin(a))
}

/// `v` turned `a` radians about +Y, as `Quat::from_rotation_y(a) * v`
/// turns it, in scalar arithmetic (glam's quaternions run on SSE2 or NEON,
/// whose sums need not round alike).
#[inline]
pub fn rotate_y(v: Vec3, a: f32) -> Vec3 {
    let (s, c) = (sin(a), cos(a));
    Vec3::new(v.x * c + v.z * s, v.y, v.z * c - v.x * s)
}

/// `v` turned toward `target` by at most `max_angle` radians (never past
/// it), as `Vec3::rotate_towards` does, in scalar arithmetic and `libm`.
pub fn rotate_towards(v: Vec3, target: Vec3, max_angle: f32) -> Vec3 {
    let lengths = (v.length_squared() * target.length_squared()).sqrt();
    if lengths == 0.0 {
        return v;
    }
    let between = libm::acosf((v.dot(target) / lengths).clamp(-1.0, 1.0));
    let angle = max_angle.clamp(between - std::f32::consts::PI, between);
    let axis = v.cross(target).try_normalize().unwrap_or_else(|| v.any_orthogonal_vector().normalize());
    // Rodrigues: v cos + (k x v) sin + k (k . v)(1 - cos).
    let (s, c) = (sin(angle), cos(angle));
    v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrees_with_std_closely_and_handles_the_axes() {
        for i in -40..40 {
            let x = i as f32 * 0.37;
            assert!((sin(x) - x.sin()).abs() < 1e-6, "sin {x}");
            assert!((cos(x) - x.cos()).abs() < 1e-6, "cos {x}");
            assert!((atan2(x, 1.3) - x.atan2(1.3)).abs() < 1e-6, "atan2 {x}");
            if (x.abs() % std::f32::consts::PI - std::f32::consts::FRAC_PI_2).abs() > 0.05 {
                assert!((tan(x) - x.tan()).abs() < 1e-4 * (1.0 + x.tan().abs()), "tan {x}");
            }
        }
        assert_eq!(atan2(0.0, 1.0), 0.0);
        assert_eq!(atan2(1.0, 0.0), std::f32::consts::FRAC_PI_2);
        assert_eq!(powf(2.0, 10.0), 1024.0);
        assert!((powf(0.5, 1.5) - 0.5f32.powf(1.5)).abs() < 1e-6);
        let v = from_angle(0.7);
        assert!((v - Vec2::from_angle(0.7)).length() < 1e-6);
    }

    /// Bevy's `ops` (map generation uses them) and glam must go through
    /// `libm` too: the workspace turns on `bevy_math/libm`. Without it they
    /// call the platform's math library and a PC and a Mac generate maps a
    /// bit apart, which desyncs online matches minutes later.
    #[test]
    fn bevy_ops_are_libm() {
        for i in -40..40 {
            let x = i as f32 * 0.37 + 0.011;
            assert_eq!(bevy_math::ops::sin(x).to_bits(), sin(x).to_bits(), "sin {x}");
            assert_eq!(bevy_math::ops::cos(x).to_bits(), cos(x).to_bits(), "cos {x}");
            assert_eq!(bevy_math::ops::atan2(x, 1.3).to_bits(), atan2(x, 1.3).to_bits(), "atan2 {x}");
            assert_eq!(bevy_math::ops::powf(x.abs(), 1.7).to_bits(), powf(x.abs(), 1.7).to_bits(), "powf {x}");
        }
    }

    #[test]
    fn rotations_match_glam_closely() {
        use bevy_math::{Quat, Vec3};
        for i in 0..50 {
            let a = i as f32 * 0.13 - 3.0;
            let v = Vec3::new(1.0 + i as f32 * 0.1, 0.3 * a, -0.7 + a);
            let ours = rotate_y(v, a);
            let glam = Quat::from_rotation_y(a) * v;
            assert!((ours - glam).length() < 1e-4 * (1.0 + v.length()), "rotate_y {v} by {a}: {ours} vs {glam}");
            let t = Vec3::new(-a, 1.0, 0.5 * a);
            for max in [0.05, 0.4, 3.0, -0.3] {
                let ours = rotate_towards(v, t, max);
                let glam = v.rotate_towards(t, max);
                assert!((ours - glam).length() < 1e-4 * (1.0 + v.length()), "rotate_towards {v} -> {t} by {max}: {ours} vs {glam}");
            }
        }
        // Already there: no further.
        let v = Vec3::new(0.0, 0.0, 2.0);
        assert!((rotate_towards(v, Vec3::new(0.0, 0.0, 5.0), 1.0) - v).length() < 1e-6);
    }
}
