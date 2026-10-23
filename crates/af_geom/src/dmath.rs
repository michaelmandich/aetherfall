//! Trigonometry and powers that give the same bits on every platform.
//!
//! `f32::sin` and friends call the platform's math library (glibc, msvcrt,
//! Apple's libm, ...), which differ in the last bit. A lockstep match
//! simulated on a Windows PC, a Mac and a Raspberry Pi must agree bit for
//! bit, so simulation code uses these (pure Rust, from the `libm` crate)
//! instead. Square roots, rounding and the four operations are exact
//! everywhere and need nothing special.

use bevy_math::Vec2;

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
}
