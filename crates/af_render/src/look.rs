//! Colours and material keys.
//!
//! Every box is drawn with the shared unit cube and one `StandardMaterial`
//! picked by a [`MatKey`]: the colour quantised to [`MatKey::COLOR_LEVELS`]
//! steps per channel, the opacity to [`MatKey::ALPHA_LEVELS`] steps, and the
//! [`Shade`]. Quantising keeps the material count small and bounded no
//! matter how many distinct colours the game computes (owner mixes, health
//! fills, fading effects), so materials are created once and Bevy can batch
//! everything that shares one.
//!
//! Colours are sRGB triples in 0..=1 (the content's `Rgb`); mixing happens in
//! sRGB space, which is what content authors picked the colours in.

use af_core::prelude::Rgb;

pub const WHITE: Rgb = [1.0, 1.0, 1.0];
pub const BLACK: Rgb = [0.0, 0.0, 0.0];

/// How a box reacts to light.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shade {
    /// Ordinary matte PBR surface.
    Lit,
    /// Lit, plus an emissive glow in its own colour (aether crystals, vents).
    Glow,
    /// Flat colour that ignores lighting (energy effects, bars, markers, ghosts).
    Unlit,
    /// Unlit, so the fog the land's shaders darken it with never dims it,
    /// and always drawn after everything else blended (ghosts, effects),
    /// which never hides it (click markers: feedback for the local
    /// player's own clicks, wherever they land).
    /// Gizmos (the unit routes) draw before any of it and write depth, so
    /// a route's stroke still shows across one.
    Marker,
}

/// What a box looks like: colour, opacity and shading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub color: Rgb,
    /// 0 (invisible) ..= 1 (opaque). Below 1 the material is alpha-blended.
    pub alpha: f32,
    pub shade: Shade,
}

impl Look {
    pub const fn lit(color: Rgb) -> Look {
        Look { color, alpha: 1.0, shade: Shade::Lit }
    }
    pub const fn glow(color: Rgb) -> Look {
        Look { color, alpha: 1.0, shade: Shade::Glow }
    }
    pub const fn unlit(color: Rgb, alpha: f32) -> Look {
        Look { color, alpha, shade: Shade::Unlit }
    }
    pub const fn marker(color: Rgb, alpha: f32) -> Look {
        Look { color, alpha, shade: Shade::Marker }
    }
}

/// Hashable, quantised [`Look`]: the material cache key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MatKey {
    rgb: [u8; 3],
    alpha: u8,
    shade: Shade,
}

impl MatKey {
    /// Steps per colour channel (64 levels: 4/255 apart, below what reads on a box face).
    pub const COLOR_LEVELS: f32 = 63.0;
    /// Opacity steps (fading effects pick a new material at most 32 times).
    pub const ALPHA_LEVELS: f32 = 32.0;

    pub fn new(look: Look) -> MatKey {
        let q = |v: f32| (v.clamp(0.0, 1.0) * Self::COLOR_LEVELS).round() as u8;
        MatKey {
            rgb: [q(look.color[0]), q(look.color[1]), q(look.color[2])],
            alpha: (look.alpha.clamp(0.0, 1.0) * Self::ALPHA_LEVELS).round() as u8,
            shade: look.shade,
        }
    }

    /// The quantised colour.
    pub fn color(&self) -> Rgb {
        self.rgb.map(|c| c as f32 / Self::COLOR_LEVELS)
    }

    /// The quantised opacity.
    pub fn alpha(&self) -> f32 {
        self.alpha as f32 / Self::ALPHA_LEVELS
    }

    pub fn is_opaque(&self) -> bool {
        self.alpha as f32 >= Self::ALPHA_LEVELS
    }

    pub fn shade(&self) -> Shade {
        self.shade
    }
}

/// Linear blend `a -> b` by `t` (0 = a, 1 = b).
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Multiply every channel by `k` (k < 1 darkens), clamped to 0..=1.
pub fn scale(c: Rgb, k: f32) -> Rgb {
    c.map(|v| (v * k).clamp(0.0, 1.0))
}

/// Blend toward white by `t`.
pub fn lighten(c: Rgb, t: f32) -> Rgb {
    mix(c, WHITE, t)
}

/// Health-bar fill: green when full, yellow at half, red when empty.
/// The fraction is quantised to 1/16 so a bar only switches material when
/// its colour visibly changes.
pub fn health_color(fraction: f32) -> Rgb {
    const GREEN: Rgb = [0.2, 0.85, 0.25];
    const YELLOW: Rgb = [0.95, 0.82, 0.15];
    const RED: Rgb = [0.9, 0.15, 0.1];
    let f = (fraction.clamp(0.0, 1.0) * 16.0).round() / 16.0;
    if f >= 0.5 { mix(YELLOW, GREEN, (f - 0.5) * 2.0) } else { mix(RED, YELLOW, f * 2.0) }
}

/// Small integer hash (lowbias32); deterministic across runs and platforms.
pub fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_quantise_nearby_colours_together() {
        let a = MatKey::new(Look::lit([0.5, 0.25, 0.75]));
        let b = MatKey::new(Look::lit([0.5 + 0.002, 0.25 - 0.003, 0.75]));
        assert_eq!(a, b);
        let c = MatKey::new(Look::lit([0.6, 0.25, 0.75]));
        assert_ne!(a, c);
    }

    #[test]
    fn keys_separate_shade_and_alpha() {
        let base = [0.3, 0.6, 0.9];
        let lit = MatKey::new(Look::lit(base));
        assert_ne!(lit, MatKey::new(Look::glow(base)));
        assert_ne!(lit, MatKey::new(Look::unlit(base, 1.0)));
        assert_ne!(MatKey::new(Look::unlit(base, 0.5)), MatKey::new(Look::unlit(base, 1.0)));
        assert!(lit.is_opaque());
        assert!(!MatKey::new(Look::unlit(base, 0.5)).is_opaque());
    }

    #[test]
    fn keys_clamp_and_round_trip() {
        let k = MatKey::new(Look { color: [-1.0, 2.0, 0.5], alpha: 3.0, shade: Shade::Lit });
        assert_eq!(k.color()[0], 0.0);
        assert_eq!(k.color()[1], 1.0);
        assert!((k.color()[2] - 0.5).abs() <= 0.501 / MatKey::COLOR_LEVELS);
        assert_eq!(k.alpha(), 1.0);
        // Bounded key space: every alpha maps to one of ALPHA_LEVELS + 1 values.
        let distinct: std::collections::HashSet<MatKey> = (0..1000).map(|i| MatKey::new(Look::unlit(base_colour(), i as f32 / 999.0))).collect();
        assert_eq!(distinct.len(), MatKey::ALPHA_LEVELS as usize + 1);
    }

    fn base_colour() -> Rgb {
        [0.9, 0.4, 0.1]
    }

    #[test]
    fn mixing() {
        let a = [0.0, 0.5, 1.0];
        let b = [1.0, 0.5, 0.0];
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 0.5), [0.5, 0.5, 0.5]);
        assert_eq!(mix(a, b, 7.0), b, "t is clamped");
        assert_eq!(scale([0.5, 0.8, 1.0], 0.5), [0.25, 0.4, 0.5]);
        assert_eq!(scale([0.8, 0.8, 0.8], 2.0), [1.0, 1.0, 1.0]);
        assert_eq!(lighten(BLACK, 1.0), WHITE);
    }

    #[test]
    fn health_colours_run_green_yellow_red() {
        let full = health_color(1.0);
        let half = health_color(0.5);
        let empty = health_color(0.0);
        assert!(full[1] > full[0], "full is green: {full:?}");
        assert!(half[0] > 0.8 && half[1] > 0.7 && half[2] < 0.3, "half is yellow: {half:?}");
        assert!(empty[0] > empty[1], "empty is red: {empty:?}");
        // Green falls monotonically toward red as health drops below half.
        let mut last = f32::MAX;
        for i in (0..=8).rev() {
            let g = health_color(i as f32 / 16.0)[1];
            assert!(g <= last + 1e-6);
            last = g;
        }
        assert_eq!(health_color(-1.0), empty);
        assert_eq!(health_color(2.0), full);
    }
}
