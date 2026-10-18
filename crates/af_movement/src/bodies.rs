//! A uniform grid over the map listing, per cell, the bodies whose bounds
//! touch it. Rebuilt from current positions when its user needs it: once a
//! tick for the steering snapshot, and for the collision solver's candidate
//! pairs only when some body has moved far enough since the last build for
//! the pairs to go stale (see `collision::Contacts`). A rebuild costs time
//! in the number of bodies, never in the size of the map: only cells that
//! hold a body get a run in `items`, and only they are reset before the
//! next rebuild. Nothing allocates once the buffers have grown.

use bevy::prelude::*;

/// `slot` of a cell holding no body.
const EMPTY: u32 = u32::MAX;

#[derive(Debug, Default)]
pub(crate) struct BodyGrid {
    origin: Vec2,
    cell: f32,
    cols: i32,
    rows: i32,
    /// Per cell: its index in `occupied`, or [`EMPTY`].
    slot: Vec<u32>,
    /// Cells holding at least one body, in first-touch order.
    occupied: Vec<u32>,
    /// Offset of each occupied cell's run in `items` (`occupied.len() + 1` entries).
    start: Vec<u32>,
    cursor: Vec<u32>,
    items: Vec<u32>,
    spans: Vec<[i32; 4]>,
}

impl BodyGrid {
    /// Index `bounds[i]` (min, max) as body `i` over a map at `origin` of `size`.
    pub fn rebuild(&mut self, origin: Vec2, size: Vec2, cell: f32, bounds: &[(Vec2, Vec2)]) {
        let cols = (size.x / cell).ceil().max(1.0) as i32;
        let rows = (size.y / cell).ceil().max(1.0) as i32;
        let cells = (cols * rows) as usize;
        if (cols, rows) != (self.cols, self.rows) || self.slot.len() != cells {
            self.slot.clear();
            self.slot.resize(cells, EMPTY);
        } else {
            for &c in &self.occupied {
                self.slot[c as usize] = EMPTY;
            }
        }
        (self.origin, self.cell, self.cols, self.rows) = (origin, cell, cols, rows);
        self.occupied.clear();
        self.start.clear();
        self.start.push(0);
        self.spans.clear();
        for (min, max) in bounds {
            let span = self.range(*min, *max);
            self.spans.push(span);
        }
        // Count per occupied cell (start[slot + 1]), then prefix sums.
        for &[x0, y0, x1, y1] in &self.spans {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = (y * self.cols + x) as usize;
                    if self.slot[c] == EMPTY {
                        self.slot[c] = self.occupied.len() as u32;
                        self.occupied.push(c as u32);
                        self.start.push(0);
                    }
                    self.start[self.slot[c] as usize + 1] += 1;
                }
            }
        }
        for i in 1..self.start.len() {
            self.start[i] += self.start[i - 1];
        }
        self.cursor.clear();
        self.cursor.extend_from_slice(&self.start[..self.occupied.len()]);
        self.items.clear();
        self.items.resize(self.start[self.occupied.len()] as usize, 0);
        for (body, &[x0, y0, x1, y1]) in self.spans.iter().enumerate() {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let s = self.slot[(y * self.cols + x) as usize] as usize;
                    self.items[self.cursor[s] as usize] = body as u32;
                    self.cursor[s] += 1;
                }
            }
        }
    }

    /// Cells `[x0, y0, x1, y1]` (inclusive) touched by the box, clamped to the map.
    #[inline]
    pub fn range(&self, min: Vec2, max: Vec2) -> [i32; 4] {
        let (x0, y0) = self.cell_xy(min);
        let (x1, y1) = self.cell_xy(max);
        [x0, y0, x1, y1]
    }

    #[inline]
    pub fn cell_xy(&self, p: Vec2) -> (i32, i32) {
        let l = (p - self.origin) / self.cell;
        ((l.x.floor() as i32).clamp(0, self.cols - 1), (l.y.floor() as i32).clamp(0, self.rows - 1))
    }

    /// Cells holding at least one body (deterministic order).
    #[inline]
    pub fn occupied(&self) -> &[u32] {
        &self.occupied
    }

    #[inline]
    pub fn cell_index(&self, x: i32, y: i32) -> usize {
        (y * self.cols + x) as usize
    }

    /// Bodies touching cell index `c`, ascending.
    #[inline]
    pub fn items(&self, c: usize) -> &[u32] {
        match self.slot.get(c) {
            Some(&s) if s != EMPTY => &self.items[self.start[s as usize] as usize..self.start[s as usize + 1] as usize],
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_listed_in_every_touched_cell() {
        let mut g = BodyGrid::default();
        let bounds = [(Vec2::new(0.5, 0.5), Vec2::new(1.5, 1.5)), (Vec2::new(3.2, 0.1), Vec2::new(3.8, 0.9))];
        g.rebuild(Vec2::ZERO, Vec2::splat(8.0), 2.0, &bounds);
        assert_eq!(g.items(g.cell_index(0, 0)), &[0]);
        assert_eq!(g.items(g.cell_index(1, 0)), &[1]);
        assert!(g.items(g.cell_index(3, 3)).is_empty());
        assert_eq!(g.occupied().len(), 2);
        // A box straddling cells is in all of them.
        g.rebuild(Vec2::ZERO, Vec2::splat(8.0), 2.0, &[(Vec2::new(1.5, 1.5), Vec2::new(2.5, 2.5))]);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert_eq!(g.items(g.cell_index(x, y)), &[0], "cell {x},{y}");
        }
    }

    /// A rebuild forgets every body of the last one, on the same map or a
    /// new one, and keeps cells in first-touch order with bodies ascending.
    #[test]
    fn rebuilds_start_from_scratch() {
        let mut g = BodyGrid::default();
        g.rebuild(Vec2::ZERO, Vec2::splat(8.0), 2.0, &[(Vec2::new(6.5, 6.5), Vec2::new(7.0, 7.0)), (Vec2::new(0.5, 0.5), Vec2::new(1.0, 1.0))]);
        assert_eq!(g.occupied(), &[g.cell_index(3, 3) as u32, 0]);
        let bounds = [(Vec2::new(0.2, 0.2), Vec2::new(0.4, 0.4)), (Vec2::new(4.5, 0.5), Vec2::new(5.0, 1.0)), (Vec2::new(0.6, 0.6), Vec2::new(0.8, 0.8))];
        g.rebuild(Vec2::ZERO, Vec2::splat(8.0), 2.0, &bounds);
        assert!(g.items(g.cell_index(3, 3)).is_empty(), "last rebuild's body is gone");
        assert_eq!(g.occupied(), &[0, g.cell_index(2, 0) as u32]);
        assert_eq!(g.items(0), &[0, 2]);
        // A bigger map.
        g.rebuild(Vec2::ZERO, Vec2::splat(16.0), 2.0, &[(Vec2::new(14.5, 14.5), Vec2::new(15.0, 15.0))]);
        assert!(g.items(0).is_empty());
        assert_eq!(g.items(g.cell_index(7, 7)), &[0]);
    }
}
