//! Grid math with no ECS in it: cached disc shapes and disc stamping.

use bevy::math::Vec2;

/// A viewer's position inside its cell is rounded down to one of
/// `SUBCELLS` x `SUBCELLS` sub-cells, each with its own cached disc.
const SUBCELLS: usize = 4;
/// Disc radii are cached in steps of 1/RADIUS_STEPS of a cell, rounded up.
const RADIUS_STEPS: f32 = 8.0;
/// Largest disc radius, in cells (larger radii are clamped).
const MAX_DISC_CELLS: f32 = 256.0;

/// How much farther than its radius, in cells, a disc may reach: the cells
/// it lights all have a point within `radius + SIGHT_SLACK_CELLS` of the
/// viewer. The slack is the sub-cell's diagonal plus one radius step.
pub const SIGHT_SLACK_CELLS: f32 = std::f32::consts::SQRT_2 / SUBCELLS as f32 + 1.0 / RADIUS_STEPS;

/// One cached disc, relative to the viewer's cell: row `top + k` is lit
/// from column `spans[k].0` to `spans[k].1` inclusive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Disc<'a> {
    pub top: i32,
    pub spans: &'a [(i16, i16)],
}

/// Where a cached disc's rows live in `DiscCache::spans`.
#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    top: i16,
    start: u32,
    /// 0 until the disc is built (a disc always has a row).
    rows: u16,
}

/// Filled discs, cached per radius step and per sub-cell of the viewer.
#[derive(Debug, Default)]
pub(crate) struct DiscCache {
    slots: Vec<Slot>,
    spans: Vec<(i16, i16)>,
}

impl DiscCache {
    /// The cells lit by a viewer that sees `radius` cells and stands at
    /// `offset` inside its cell (each axis in 0..1): every cell holding a
    /// point within `radius` of the viewer, and none whose nearest point is
    /// farther than `radius + SIGHT_SLACK_CELLS`.
    pub(crate) fn disc(&mut self, radius: f32, offset: Vec2) -> Disc<'_> {
        // A NaN radius saturates to step 0: the viewer sees its own cell.
        let step = (radius.clamp(0.0, MAX_DISC_CELLS) * RADIUS_STEPS).ceil() as usize;
        let (qx, qy) = (subcell(offset.x), subcell(offset.y));
        let key = (step * SUBCELLS + qy) * SUBCELLS + qx;
        if key >= self.slots.len() {
            self.slots.resize(key + 1, Slot::default());
        }
        if self.slots[key].rows == 0 {
            let start = self.spans.len();
            let top = disc_spans(step as f32 / RADIUS_STEPS, qx, qy, &mut self.spans);
            self.slots[key] = Slot { top, start: start as u32, rows: (self.spans.len() - start) as u16 };
        }
        let slot = self.slots[key];
        let start = slot.start as usize;
        Disc { top: slot.top.into(), spans: &self.spans[start..start + slot.rows as usize] }
    }
}

/// The sub-cell index of an in-cell offset (out-of-range and NaN clamp).
fn subcell(offset: f32) -> usize {
    ((offset * SUBCELLS as f32) as usize).min(SUBCELLS - 1)
}

/// Append the rows of the disc of radius `r` cells seen from sub-cell
/// (`qx`, `qy`) and return the first row's offset. A cell is lit when some
/// point of it is within `r` of some point of the sub-cell, so the one disc
/// is right for every position the sub-cell stands for.
fn disc_spans(r: f32, qx: usize, qy: usize, out: &mut Vec<(i16, i16)>) -> i16 {
    let size = 1.0 / SUBCELLS as f32;
    let (x0, x1) = (qx as f32 * size, (qx + 1) as f32 * size);
    let (y0, y1) = (qy as f32 * size, (qy + 1) as f32 * size);
    // Cell `d` spans [d, d + 1]; it is within `w` of [a0, a1] when
    // a0 - 1 - w <= d <= a1 + w.
    let top = (y0 - 1.0 - r).ceil() as i32;
    let bottom = (y1 + r).floor() as i32;
    for dy in top..=bottom {
        let gap = (dy as f32 - y1).max(y0 - (dy + 1) as f32).max(0.0);
        let w = (r * r - gap * gap).max(0.0).sqrt();
        out.push(((x0 - 1.0 - w).ceil() as i16, (x1 + w).floor() as i16));
    }
    top as i16
}

/// Set every cell of `disc` around cell (`cx`, `cy`) to 1 in a row-major
/// `width` x `height` grid, clipped to the grid.
pub(crate) fn stamp_disc(grid: &mut [u8], width: u32, height: u32, cx: i32, cy: i32, disc: Disc) {
    let (w, h) = (i64::from(width), i64::from(height));
    let (cx, first) = (i64::from(cx), i64::from(cy) + i64::from(disc.top));
    let rows = disc.spans.len() as i64;
    // The disc rows that land inside the grid.
    let (k0, k1) = ((-first).clamp(0, rows), (h - first).clamp(0, rows));
    for (k, &(x0, x1)) in (k0..k1).zip(&disc.spans[k0 as usize..k1 as usize]) {
        let x0 = (cx + i64::from(x0)).max(0);
        let x1 = (cx + i64::from(x1)).min(w - 1);
        if x0 <= x1 {
            let row = (first + k) * w;
            if let Some(span) = grid.get_mut((row + x0) as usize..=(row + x1) as usize) {
                span.fill(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit_cells(grid: &[u8], width: u32) -> Vec<(i32, i32)> {
        grid.iter()
            .enumerate()
            .filter(|(_, v)| **v != 0)
            .map(|(i, _)| ((i as u32 % width) as i32, (i as u32 / width) as i32))
            .collect()
    }

    /// Distance from `p` to the unit cell (`x`, `y`).
    fn cell_distance(p: Vec2, x: i32, y: i32) -> f32 {
        let min = Vec2::new(x as f32, y as f32);
        p.distance(p.clamp(min, min + Vec2::ONE))
    }

    #[test]
    fn discs_light_every_cell_in_reach_and_little_more() {
        let mut cache = DiscCache::default();
        let offsets = [0.0, 0.01, 0.24, 0.25, 0.5, 0.74, 0.99];
        for radius in [0.0, 0.6, 1.0, 2.5, 4.75, 5.175, 9.3] {
            for ox in offsets {
                for oy in offsets {
                    let disc = cache.disc(radius, Vec2::new(ox, oy));
                    let mut grid = vec![0u8; 41 * 41];
                    stamp_disc(&mut grid, 41, 41, 20, 20, disc);
                    let viewer = Vec2::new(20.0 + ox, 20.0 + oy);
                    for y in 0..41 {
                        for x in 0..41 {
                            let d = cell_distance(viewer, x, y);
                            let lit = grid[(y * 41 + x) as usize] != 0;
                            let at = format!("r {radius} at {viewer}: cell ({x}, {y}) {d} away");
                            assert!(lit || d > radius, "{at} is dark");
                            assert!(!lit || d <= radius + SIGHT_SLACK_CELLS, "{at} is lit");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn discs_are_cached_per_radius_step_and_subcell() {
        let mut cache = DiscCache::default();
        let a = cache.disc(3.01, Vec2::new(0.3, 0.8)).spans.to_vec();
        let built = cache.spans.len();
        assert_eq!(cache.disc(3.1, Vec2::new(0.45, 0.76)).spans, a.as_slice(), "same step, same sub-cell");
        assert_eq!(cache.spans.len(), built, "served from the cache");
        assert_ne!(cache.disc(3.1, Vec2::new(0.5, 0.8)).spans, a.as_slice(), "another sub-cell");
        assert_ne!(cache.disc(3.2, Vec2::new(0.3, 0.8)).spans, a.as_slice(), "another step");
        let own = Disc { top: 0, spans: &[(0, 0)] };
        assert_eq!(cache.disc(f32::NAN, Vec2::splat(0.5)), own, "NaN sees only its own cell");
        assert_eq!(cache.disc(-4.0, Vec2::splat(0.6)), own);
        let corner = cache.disc(2.0, Vec2::ZERO).spans.to_vec();
        assert_eq!(cache.disc(2.0, Vec2::NAN).spans, corner.as_slice(), "a NaN offset counts as the cell's corner");
    }

    #[test]
    fn stamping_clips_at_the_edges() {
        let mut cache = DiscCache::default();
        let disc = cache.disc(2.0, Vec2::new(0.6, 0.3));
        // Stamped near or past the edges of a 5 x 4 grid, a disc lights
        // exactly the cells it lights in the same spot of a larger grid.
        for (cx, cy) in [(0, 0), (4, 3), (-1, 2), (6, 1), (2, -3), (-3, -3)] {
            let mut small = vec![0u8; 5 * 4];
            stamp_disc(&mut small, 5, 4, cx, cy, disc);
            let mut big = vec![0u8; 25 * 24];
            stamp_disc(&mut big, 25, 24, cx + 10, cy + 10, disc);
            let inside: Vec<_> =
                lit_cells(&big, 25).into_iter().map(|(x, y)| (x - 10, y - 10)).filter(|&(x, y)| (0..5).contains(&x) && (0..4).contains(&y)).collect();
            assert_eq!(lit_cells(&small, 5), inside, "centre ({cx}, {cy})");
        }
        // Centres far outside the grid (or saturated) touch nothing and never panic.
        let mut grid = vec![0u8; 5 * 4];
        stamp_disc(&mut grid, 5, 4, -10, 2, disc);
        stamp_disc(&mut grid, 5, 4, i32::MAX, i32::MIN, disc);
        stamp_disc(&mut grid, 5, 4, i32::MIN, i32::MAX, disc);
        stamp_disc(&mut [], 0, 0, 0, 0, disc);
        assert!(lit_cells(&grid, 5).is_empty());
    }
}
