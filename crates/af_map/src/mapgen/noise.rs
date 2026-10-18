//! Seeded value noise for organic terrain shapes.

use af_core::prelude::SimRng;
use bevy::prelude::*;

/// Smooth value noise on a square lattice, in `[0, 1)`.
struct ValueNoise {
    side: usize,
    inv_cell: f32,
    values: Vec<f32>,
}

impl ValueNoise {
    fn new(rng: &mut SimRng, extent: f32, cell: f32) -> ValueNoise {
        let side = (extent / cell).ceil().max(1.0) as usize + 2;
        let values = (0..side * side).map(|_| rng.f32()).collect();
        ValueNoise { side, inv_cell: 1.0 / cell, values }
    }

    fn at(&self, p: Vec2) -> f32 {
        let last = (self.side - 2) as f32;
        let q = (p * self.inv_cell).clamp(Vec2::ZERO, Vec2::splat(last));
        let (x0, y0) = (q.x.floor() as usize, q.y.floor() as usize);
        let tx = smoothstep(q.x - x0 as f32);
        let ty = smoothstep(q.y - y0 as f32);
        let v = |dx: usize, dy: usize| self.values[(y0 + dy) * self.side + x0 + dx];
        let bottom = v(0, 0) + (v(1, 0) - v(0, 0)) * tx;
        let top = v(0, 1) + (v(1, 1) - v(0, 1)) * tx;
        bottom + (top - bottom) * ty
    }
}

#[inline]
fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Two octaves of value noise: blobs about `cell` metres across with finer
/// detail on their edges. Values lie in `[0, 1)`, clustered around 0.5.
pub(crate) struct Noise {
    coarse: ValueNoise,
    fine: ValueNoise,
}

impl Noise {
    /// Noise covering `0..extent` on both axes.
    pub fn new(rng: &mut SimRng, extent: f32, cell: f32) -> Noise {
        Noise { coarse: ValueNoise::new(rng, extent, cell), fine: ValueNoise::new(rng, extent, cell * 0.4) }
    }

    pub fn at(&self, p: Vec2) -> f32 {
        0.68 * self.coarse.at(p) + 0.32 * self.fine.at(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_bounded_smooth_and_seeded() {
        let a = Noise::new(&mut SimRng::new(7), 100.0, 10.0);
        let b = Noise::new(&mut SimRng::new(7), 100.0, 10.0);
        let mut lo: f32 = 1.0;
        let mut hi: f32 = 0.0;
        for i in 0..2000 {
            let p = Vec2::new((i % 50) as f32 * 2.3 - 5.0, (i / 50) as f32 * 2.9);
            let v = a.at(p);
            assert_eq!(v, b.at(p));
            assert!((0.0..1.0).contains(&v), "{v}");
            // Neighbouring samples 0.1 m apart differ only a little.
            assert!((v - a.at(p + Vec2::splat(0.1))).abs() < 0.12);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(hi - lo > 0.3, "noise should vary: {lo}..{hi}");
    }
}
