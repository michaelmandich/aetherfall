//! Small deterministic noise for the terrain's shape and props (the shaders
//! have their own for colour). Pure functions of position and seed; never
//! touches `SimRng`.

use bevy::math::{IVec2, Vec2};

use crate::look::hash32;

/// A hash of a lattice point, in 0..1.
pub fn lattice(x: i32, y: i32, seed: u32) -> f32 {
    let h = hash32((x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f));
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth value noise in 0..1, one lattice cell per unit.
pub fn value(p: Vec2, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let (x, y) = (i.x as i32, i.y as i32);
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let a = lattice(x, y, seed);
    let b = lattice(x + 1, y, seed);
    let c = lattice(x, y + 1, seed);
    let d = lattice(x + 1, y + 1, seed);
    let top = a + (b - a) * u.x;
    let bottom = c + (d - c) * u.x;
    top + (bottom - top) * u.y
}

/// Fractal value noise in 0..1: `octaves` layers, each twice the frequency
/// and half the amplitude of the last.
pub fn fbm(p: Vec2, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut q) = (0.0, 1.0, 0.0, p);
    for o in 0..octaves {
        sum += value(q, seed.wrapping_add(o * 0x9e37)) * amp;
        norm += amp;
        amp *= 0.5;
        // Rotate a little between octaves so the lattices never line up.
        q = Vec2::new(q.x * 1.6 - q.y * 1.2, q.x * 1.2 + q.y * 1.6);
    }
    sum / norm
}

/// Ridged fractal noise in 0..1: sharp crests where the noise crosses its
/// middle (mountain ridges and peaks).
pub fn ridged(p: Vec2, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut q) = (0.0, 1.0, 0.0, p);
    for o in 0..octaves {
        let n = 1.0 - (value(q, seed.wrapping_add(o * 0x51ed)) * 2.0 - 1.0).abs();
        sum += n * n * amp;
        norm += amp;
        amp *= 0.5;
        q = Vec2::new(q.x * 1.7 - q.y * 1.1, q.x * 1.1 + q.y * 1.7);
    }
    sum / norm
}

/// Rounded tree crowns seen from above, in 0..1: 1 at the middle of a crown
/// (one jittered crown per `size`-metre cell, of varied size), 0 between
/// them.
pub fn crowns(p: Vec2, size: f32, seed: u32) -> f32 {
    let g = p / size;
    let i = g.floor();
    let mut best: f32 = 0.0;
    for dz in -1..=1 {
        for dx in -1..=1 {
            let c = IVec2::new(i.x as i32 + dx, i.y as i32 + dz);
            let centre = Vec2::new(c.x as f32 + 0.15 + 0.7 * lattice(c.x, c.y, seed), c.y as f32 + 0.15 + 0.7 * lattice(c.x, c.y, seed ^ 0x77));
            let r = 0.55 + 0.35 * lattice(c.x, c.y, seed ^ 0x99);
            let d = g.distance(centre) / r;
            best = best.max(1.0 - d * d);
        }
    }
    best
}

/// A hash of a cell (and a salt) in 0..1, for props and per-cell choices.
pub fn cell_hash(x: i32, y: i32, salt: u32) -> f32 {
    lattice(x, y, salt ^ 0x5bd1_e995)
}

/// Hermite smoothstep.
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_bounded_and_varied() {
        let mut lo: f32 = 1.0;
        let mut hi: f32 = 0.0;
        for i in 0..2000 {
            let p = Vec2::new(i as f32 * 0.37 - 300.0, (i * 7 % 113) as f32 * 0.91);
            for v in [value(p, 3), fbm(p, 4, 3), ridged(p, 3, 3)] {
                assert!((0.0..=1.0).contains(&v), "{v} at {p}");
                lo = lo.min(v);
                hi = hi.max(v);
            }
            assert_eq!(fbm(p, 3, 9), fbm(p, 3, 9));
        }
        assert!(lo < 0.15 && hi > 0.85, "{lo}..{hi}");
        assert_ne!(value(Vec2::new(0.5, 0.5), 1), value(Vec2::new(0.5, 0.5), 2), "seeds differ");
        assert_eq!(value(Vec2::new(3.0, 4.0), 7), lattice(3, 4, 7), "lattice points are exact");
        let tops: Vec<f32> = (0..400).map(|i| crowns(Vec2::new((i % 20) as f32 * 0.7, (i / 20) as f32 * 0.7), 3.0, 1)).collect();
        assert!(tops.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(tops.iter().any(|v| *v > 0.9) && tops.iter().any(|v| *v < 0.2), "crowns and gaps");
    }
}
