//! Constant-time clearance queries for [`NavGrid`].
//!
//! "Can a mover with clearance `k` stand in cell `c`?" means "is every cell of
//! the `(2k+1)^2` square around `c` passable?". Summed-area tables (integral
//! images) of blocked cells answer that with four reads per table instead of
//! `(2k+1)^2` cell checks.
//!
//! Passability splits into three independent parts, each with its own table,
//! so a change to one part rebuilds only that part:
//! * obstacles (`walk_blockers > 0`): one table shared by every mover, rebuilt
//!   whenever the grid version moves (buildings placed, trees felled);
//! * terrain the mover cannot enter: one table per distinct *set of admitted
//!   terrain ids* (Foot and Foot+Dunerunner admit the same set and share one),
//!   rebuilt only when terrain or terrain rules change;
//! * gates of other teams: one table per team, rebuilt only when gates change,
//!   and absent while the map has no gate at all.
//!
//! Tables are refreshed by [`NavGrid::prepare`]; queries through `&NavGrid`
//! fall back to per-cell checks while a table is stale.

use bevy_math::IVec2;

use crate::grid::{NavGrid, NO_TEAM};

/// Largest clearance (in cells) the tables answer; wider movers use the
/// per-cell fallback. The u16 sums are exact while `(2k+1)^2 < 65536`.
pub(crate) const MAX_TABLE_CLEARANCE: i32 = 127;

/// Summed-area table of a 0/1 "blocked" image, `(w+1) x (h+1)` entries with
/// wrapping u16 arithmetic (rectangle sums stay exact below 65536).
#[derive(Clone, Debug, Default)]
pub(crate) struct Sat(Vec<u16>);

impl Sat {
    fn rebuild(&mut self, w: usize, h: usize, blocked: impl Fn(usize) -> bool) {
        let stride = w + 1;
        self.0.clear();
        self.0.resize(stride * (h + 1), 0);
        for y in 0..h {
            let mut row = 0u16;
            for x in 0..w {
                row = row.wrapping_add(blocked(y * w + x) as u16);
                self.0[(y + 1) * stride + x + 1] = self.0[y * stride + x + 1].wrapping_add(row);
            }
        }
    }

    fn built(w: usize, h: usize, blocked: impl Fn(usize) -> bool) -> Sat {
        let mut s = Sat::default();
        s.rebuild(w, h, blocked);
        s
    }

    /// Number of blocked cells in the inclusive, in-bounds rectangle.
    #[inline]
    fn rect(&self, stride: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> u16 {
        let top = y0 * stride;
        let bottom = (y1 + 1) * stride;
        let d = self.0[bottom + x1 + 1];
        let b = self.0[top + x1 + 1];
        let c = self.0[bottom + x0];
        let a = self.0[top + x0];
        d.wrapping_sub(b).wrapping_sub(c).wrapping_add(a)
    }
}

#[derive(Clone, Debug)]
struct TerrainLayer {
    /// Bit `t` set = terrain id `t` admits the movers using this layer.
    admits: [u64; 4],
    sat: Sat,
}

/// The lazily maintained tables of one grid (lives in its `PathCache`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Clearance {
    /// Grid version the tables were validated against (0 = never).
    version: u64,
    blockers: Sat,
    terrain_seen: Vec<u8>,
    masks_seen: Vec<u16>,
    terrain: Vec<TerrainLayer>,
    /// Mobility mask -> index into `terrain`.
    by_mobility: Vec<(u16, usize)>,
    gates_seen: Vec<u8>,
    any_gates: bool,
    gates: Vec<(u8, Sat)>,
}

impl Clearance {
    /// Revalidate against the grid: rebuild the obstacle table and drop
    /// terrain/gate tables whose inputs changed.
    pub(crate) fn refresh(&mut self, g: &NavGrid) {
        if self.version == g.version {
            return;
        }
        let (w, h) = (g.width as usize, g.height as usize);
        self.blockers.rebuild(w, h, |i| g.walk_blockers[i] > 0);
        if self.terrain_seen != g.terrain || self.masks_seen[..] != g.terrain_mask[..] {
            self.terrain_seen.clone_from(&g.terrain);
            self.masks_seen = g.terrain_mask.to_vec();
            self.terrain.clear();
            self.by_mobility.clear();
        }
        if self.gates_seen != g.gates {
            self.gates_seen.clone_from(&g.gates);
            self.any_gates = g.gates.iter().any(|v| *v != 0);
            self.gates.clear();
        }
        self.version = g.version;
    }

    /// Build the tables movers of `mobility` on `team` need (after `refresh`).
    pub(crate) fn ensure(&mut self, g: &NavGrid, mobility: u16, team: u8) {
        let (w, h) = (g.width as usize, g.height as usize);
        if !self.by_mobility.iter().any(|(m, _)| *m == mobility) {
            let mut admits = [0u64; 4];
            for (t, mask) in g.terrain_mask.iter().enumerate() {
                if mask & mobility != 0 {
                    admits[t / 64] |= 1 << (t % 64);
                }
            }
            let idx = match self.terrain.iter().position(|l| l.admits == admits) {
                Some(i) => i,
                None => {
                    let sat = Sat::built(w, h, |i| {
                        let t = g.terrain[i] as usize;
                        admits[t / 64] & (1 << (t % 64)) == 0
                    });
                    self.terrain.push(TerrainLayer { admits, sat });
                    self.terrain.len() - 1
                }
            };
            self.by_mobility.push((mobility, idx));
        }
        if self.any_gates && !self.gates.iter().any(|(t, _)| *t == team) {
            let sat = Sat::built(w, h, |i| gate_blocks(g.gates[i], team));
            self.gates.push((team, sat));
        }
    }

    /// Constant-time query for this mover class, if its tables are fresh.
    pub(crate) fn query<'a>(&'a self, g: &NavGrid, mobility: u16, team: u8) -> Option<ClearQuery<'a>> {
        if self.version != g.version {
            return None;
        }
        let (_, idx) = self.by_mobility.iter().find(|(m, _)| *m == mobility)?;
        let gates = if self.any_gates { Some(&self.gates.iter().find(|(t, _)| *t == team)?.1) } else { None };
        Some(ClearQuery {
            w: g.width as i32,
            h: g.height as i32,
            stride: g.width as usize + 1,
            blockers: &self.blockers,
            terrain: &self.terrain[*idx].sat,
            gates,
        })
    }

    pub(crate) fn layer_count(&self) -> usize {
        1 + self.terrain.len() + self.gates.len()
    }
}

/// Does a gate cell value (0 = none, else owner team + 1) block `team`?
#[inline]
pub(crate) fn gate_blocks(gate: u8, team: u8) -> bool {
    gate != 0 && (team == NO_TEAM || gate != team.saturating_add(1))
}

/// Fresh tables of one mover class.
#[derive(Clone, Copy)]
pub(crate) struct ClearQuery<'a> {
    w: i32,
    h: i32,
    stride: usize,
    blockers: &'a Sat,
    terrain: &'a Sat,
    gates: Option<&'a Sat>,
}

impl ClearQuery<'_> {
    /// Every cell of the square of half-size `k` around `c` is in bounds and passable.
    #[inline]
    pub(crate) fn square_clear(&self, c: IVec2, k: i32) -> bool {
        let (x0, y0, x1, y1) = (c.x - k, c.y - k, c.x + k, c.y + k);
        if x0 < 0 || y0 < 0 || x1 >= self.w || y1 >= self.h {
            return false;
        }
        let (x0, y0, x1, y1) = (x0 as usize, y0 as usize, x1 as usize, y1 as usize);
        self.blockers.rect(self.stride, x0, y0, x1, y1) == 0
            && self.terrain.rect(self.stride, x0, y0, x1, y1) == 0
            && self.gates.is_none_or(|g| g.rect(self.stride, x0, y0, x1, y1) == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obb::Obb;
    use bevy_math::Vec2;

    /// Tables agree with the per-cell definition for every cell and clearance.
    #[test]
    fn tables_match_brute_force() {
        let mut g = NavGrid::new(24, 18, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b01, true);
        g.set_terrain_info(1, 0b10, false);
        for x in 5..9 {
            g.set_terrain(IVec2::new(x, 4), 1);
        }
        g.add_obstacle(&Obb::new(Vec2::new(14.0, 9.0), Vec2::new(2.0, 1.0), 0.4), 1);
        g.set_gate(&Obb::new(Vec2::new(4.0, 13.0), Vec2::new(1.5, 0.6), 0.0), Some(0));
        for team in [0u8, 1, NO_TEAM] {
            for mob in [0b01u16, 0b10, 0b11] {
                let mut cl = Clearance::default();
                cl.refresh(&g);
                cl.ensure(&g, mob, team);
                let q = cl.query(&g, mob, team).expect("fresh");
                for k in 0..3 {
                    for y in -1..19 {
                        for x in -1..25 {
                            let c = IVec2::new(x, y);
                            let brute = (-k..=k).all(|dy| (-k..=k).all(|dx| g.passable(c + IVec2::new(dx, dy), mob, team)));
                            assert_eq!(q.square_clear(c, k), brute, "cell {c} k {k} mob {mob} team {team}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn stale_after_change_and_shared_terrain_layers() {
        let mut g = NavGrid::new(8, 8, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b011, true);
        let mut cl = Clearance::default();
        cl.refresh(&g);
        cl.ensure(&g, 0b001, 0);
        cl.ensure(&g, 0b010, 0);
        // Both masks admit the same terrain set: one shared layer.
        assert_eq!(cl.layer_count(), 2);
        g.add_obstacle(&Obb::new(Vec2::new(3.5, 3.5), Vec2::splat(0.5), 0.0), 1);
        assert!(cl.query(&g, 0b001, 0).is_none(), "stale after the grid changed");
        cl.refresh(&g);
        let q = cl.query(&g, 0b001, 0).expect("terrain layer kept");
        assert!(!q.square_clear(IVec2::new(3, 3), 0));
    }
}
