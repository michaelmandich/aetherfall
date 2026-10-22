//! Distance fields: how far every cell of a grid is from the nearest cell of
//! a mask. The generator uses them for room checks ([`super::Gen::room`])
//! and to keep rivers clear of everything they must not touch.

/// Squared distances beyond any map.
const FAR: f64 = 1e18;

/// Euclidean distance transform of a `w` x `h` grid (row-major): the
/// distance, in cells, from every cell to the nearest cell where `mask` is
/// true; [`f32::INFINITY`] everywhere when no cell is. Exact (Felzenszwalb
/// and Huttenlocher's two passes of lower envelopes of parabolas), linear in
/// the cell count and free of floating-point transcendentals, so it is
/// deterministic.
pub(crate) fn distance_field(mask: &[bool], w: usize, h: usize) -> Vec<f32> {
    debug_assert_eq!(mask.len(), w * h);
    if !mask.iter().any(|m| *m) {
        return vec![f32::INFINITY; w * h];
    }
    let mut sq: Vec<f64> = mask.iter().map(|m| if *m { 0.0 } else { FAR }).collect();
    let n = w.max(h);
    let mut line = vec![0.0; n];
    let mut out = vec![0.0; n];
    let mut env = Envelope { v: vec![0; n], z: vec![0.0; n + 1] };
    for x in 0..w {
        for y in 0..h {
            line[y] = sq[y * w + x];
        }
        env.transform(&line[..h], &mut out[..h]);
        for y in 0..h {
            sq[y * w + x] = out[y];
        }
    }
    for y in 0..h {
        env.transform(&sq[y * w..(y + 1) * w], &mut out[..w]);
        sq[y * w..(y + 1) * w].copy_from_slice(&out[..w]);
    }
    sq.into_iter().map(|d| if d >= FAR * 0.5 { f32::INFINITY } else { d.sqrt() as f32 }).collect()
}

/// Scratch space for the 1D transform.
struct Envelope {
    /// Positions of the parabolas in the lower envelope.
    v: Vec<usize>,
    /// Boundaries between them.
    z: Vec<f64>,
}

impl Envelope {
    /// `out[q] = min_p (q - p)^2 + f[p]`.
    fn transform(&mut self, f: &[f64], out: &mut [f64]) {
        let n = f.len();
        let (v, z) = (&mut self.v, &mut self.z);
        let mut k = 0usize;
        v[0] = 0;
        z[0] = f64::NEG_INFINITY;
        z[1] = f64::INFINITY;
        for q in 1..n {
            let fq = f[q] + (q * q) as f64;
            let meet = |p: usize| (fq - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64));
            // z[0] is minus infinity and every `meet` is finite, so this
            // stops at k = 0 at the latest.
            let mut s = meet(v[k]);
            while s <= z[k] {
                k -= 1;
                s = meet(v[k]);
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f64::INFINITY;
        }
        k = 0;
        for (q, o) in out.iter_mut().enumerate() {
            while z[k + 1] < q as f64 {
                k += 1;
            }
            let p = v[k];
            let d = q as f64 - p as f64;
            *o = d * d + f[p];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_brute_force() {
        let (w, h) = (23, 17);
        let mask: Vec<bool> = (0..w * h).map(|i| (i * 7919 + i / 5) % 37 == 0 || i == 200).collect();
        let field = distance_field(&mask, w, h);
        for y in 0..h {
            for x in 0..w {
                let mut best = f32::INFINITY;
                for (j, m) in mask.iter().enumerate() {
                    if *m {
                        let (dx, dy) = ((j % w) as f32 - x as f32, (j / w) as f32 - y as f32);
                        best = best.min((dx * dx + dy * dy).sqrt());
                    }
                }
                assert!((field[y * w + x] - best).abs() < 1e-4, "({x}, {y}): {} vs {best}", field[y * w + x]);
            }
        }
    }

    #[test]
    fn empty_mask_is_infinitely_far() {
        assert!(distance_field(&[false; 12], 4, 3).iter().all(|d| d.is_infinite()));
        assert_eq!(distance_field(&[true], 1, 1), vec![0.0]);
    }
}
