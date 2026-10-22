//! The navigation grid: terrain per cell, static obstacles, gates.
//!
//! Cells are square (`cell` metres, normally 1.0). Cell `(x, y)` covers
//! world `origin + [x, x+1) * cell` on X and `origin.y + [y, y+1) * cell` on Z.
//! A cell is passable for a mobility mask when its terrain admits any of the
//! mask's bits, no static obstacle comes within the walk margin of its
//! centre, and (if it is a gate cell) the gate belongs to the mover's team.
//! Flying movers ignore the grid.
//!
//! Obstacles are rasterised twice:
//! * *covered* cells, whose centre lies inside the box: what building
//!   placement tests ([`NavGrid::area_buildable`], [`NavGrid::is_blocked`]),
//!   so footprints can sit flush against each other;
//! * *walk-blocked* cells, whose centre lies within
//!   [`NavGrid::walk_margin`] of the box: what movers avoid. A free cell
//!   then always leaves room for a mover of that radius at its centre, so a
//!   path never leads through a gap narrower than twice the margin between
//!   two obstacles that both just miss the cell centres around it.
//!
//! Callers may close further cells to movers ([`NavGrid::add_walk_blocks`]),
//! e.g. a slot between two obstacles whose centre clears both by the margin
//! yet is too narrow for a box to pass at an angle.

use std::sync::atomic::{AtomicU64, Ordering};

use bevy_math::{IVec2, Vec2};

use crate::obb::Obb;
use crate::regions::TerrainSnapshot;

/// Bit of `MoveFlag::Flying` in a mobility mask (must match af_data's
/// `Mobility::of(MoveFlag::Flying)`; af_core asserts this in a test).
pub const FLYING: u16 = 1 << 5;

/// Team value meaning "no team": passes no gates.
pub const NO_TEAM: u8 = u8::MAX;

/// Source of terrain stamps: every terrain change of every grid takes a
/// fresh one, so a stamp names one terrain layout across grids (a new map's
/// grid never repeats the last one's).
static TERRAIN_STAMPS: AtomicU64 = AtomicU64::new(1);

fn fresh_terrain_stamp() -> u64 {
    TERRAIN_STAMPS.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug)]
pub struct NavGrid {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) cell: f32,
    pub(crate) origin: Vec2,
    pub(crate) terrain: Vec<u8>,
    pub(crate) terrain_mask: [u16; 256],
    pub(crate) terrain_buildable: [bool; 256],
    /// Changes with the terrain or what it admits (see [`NavGrid::terrain_stamp`]).
    pub(crate) terrain_stamp: u64,
    /// Number of static obstacles covering each cell (centre inside the box).
    pub(crate) blockers: Vec<u16>,
    /// Number of static obstacles within `walk_margin` of each cell centre.
    pub(crate) walk_blockers: Vec<u16>,
    /// See [`NavGrid::set_walk_margin`].
    pub(crate) walk_margin: f32,
    /// 0 = no gate, else team + 1 of the gate's owner.
    pub(crate) gates: Vec<u8>,
    /// Bumped on every change; path caches compare against it.
    pub(crate) version: u64,
    /// Pathfinding scratch/caches (see path.rs).
    pub(crate) cache: crate::path::PathCache,
}

impl NavGrid {
    /// A grid of `width` x `height` cells, every cell terrain 0.
    pub fn new(width: u32, height: u32, cell: f32, origin: Vec2) -> NavGrid {
        let n = (width * height) as usize;
        NavGrid {
            width,
            height,
            cell,
            origin,
            terrain: vec![0; n],
            terrain_mask: [0; 256],
            terrain_buildable: [false; 256],
            terrain_stamp: fresh_terrain_stamp(),
            blockers: vec![0; n],
            walk_blockers: vec![0; n],
            walk_margin: 0.0,
            gates: vec![0; n],
            version: 1,
            cache: Default::default(),
        }
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }
    #[inline]
    pub fn cell_size(&self) -> f32 {
        self.cell
    }
    #[inline]
    pub fn origin(&self) -> Vec2 {
        self.origin
    }
    /// World-space size (x, z).
    pub fn world_size(&self) -> Vec2 {
        Vec2::new(self.width as f32, self.height as f32) * self.cell
    }
    #[inline]
    pub fn version(&self) -> u64 {
        self.version
    }

    #[inline]
    pub fn in_bounds(&self, c: IVec2) -> bool {
        c.x >= 0 && c.y >= 0 && (c.x as u32) < self.width && (c.y as u32) < self.height
    }

    #[inline]
    pub fn index(&self, c: IVec2) -> usize {
        (c.y as u32 * self.width + c.x as u32) as usize
    }

    /// Cell containing a world point (may be out of bounds).
    #[inline]
    pub fn cell_of(&self, p: Vec2) -> IVec2 {
        let l = (p - self.origin) / self.cell;
        IVec2::new(l.x.floor() as i32, l.y.floor() as i32)
    }

    #[inline]
    pub fn cell_center(&self, c: IVec2) -> Vec2 {
        self.origin + (c.as_vec2() + Vec2::splat(0.5)) * self.cell
    }

    /// Clamp a world point into the grid's bounds (inset by `margin`).
    pub fn clamp_point(&self, p: Vec2, margin: f32) -> Vec2 {
        let max = self.origin + self.world_size();
        Vec2::new(
            p.x.clamp(self.origin.x + margin, max.x - margin),
            p.y.clamp(self.origin.y + margin, max.y - margin),
        )
    }

    /// Register what a terrain id admits and whether it is buildable.
    pub fn set_terrain_info(&mut self, terrain: u8, mobility_mask: u16, buildable: bool) {
        self.terrain_mask[terrain as usize] = mobility_mask;
        self.terrain_buildable[terrain as usize] = buildable;
        self.terrain_stamp = fresh_terrain_stamp();
        self.version += 1;
    }

    pub fn terrain_at(&self, c: IVec2) -> u8 {
        if self.in_bounds(c) { self.terrain[self.index(c)] } else { 0 }
    }

    pub fn set_terrain(&mut self, c: IVec2, terrain: u8) {
        if self.in_bounds(c) {
            let i = self.index(c);
            self.terrain[i] = terrain;
            self.terrain_stamp = fresh_terrain_stamp();
            self.version += 1;
        }
    }

    /// Names the grid's terrain and what each terrain admits: it changes
    /// whenever either does (never for obstacles or gates), and no other
    /// terrain layout, on this grid or any other, ever had it. Cloning keeps
    /// it. Region maps ([`crate::regions`]) built from a snapshot carry it, so
    /// their users can tell when the terrain moved on.
    #[inline]
    pub fn terrain_stamp(&self) -> u64 {
        self.terrain_stamp
    }

    /// The grid's terrain as it stands now, for region maps (see
    /// [`crate::regions`]): dimensions, terrain ids, what each admits, and
    /// the [`terrain_stamp`](Self::terrain_stamp).
    pub fn terrain_snapshot(&self) -> TerrainSnapshot {
        TerrainSnapshot {
            width: self.width,
            height: self.height,
            cell: self.cell,
            origin: self.origin,
            terrain: self.terrain.clone(),
            masks: self.terrain_mask,
            stamp: self.terrain_stamp,
        }
    }

    /// Raw terrain ids, row-major (y * width + x).
    pub fn terrain_cells(&self) -> &[u8] {
        &self.terrain
    }

    /// Cells whose centres lie inside `obb` grown by `inflate` metres, plus
    /// always the cell under the centre.
    pub fn cells_covered(&self, obb: &Obb, inflate: f32) -> Vec<IVec2> {
        let grown = Obb { half: obb.half + Vec2::splat(inflate.max(0.0)), ..*obb };
        let (min, max) = grown.aabb();
        let c0 = self.cell_of(min);
        let c1 = self.cell_of(max);
        let mut out = Vec::new();
        for y in c0.y..=c1.y {
            for x in c0.x..=c1.x {
                let c = IVec2::new(x, y);
                if self.in_bounds(c) && grown.contains(self.cell_center(c)) {
                    out.push(c);
                }
            }
        }
        let center = self.cell_of(obb.center);
        if self.in_bounds(center) && !out.contains(&center) {
            out.push(center);
        }
        out
    }

    /// Cells whose centres lie within `margin` metres of `obb` (inside it
    /// included), plus always the cell under the centre.
    pub fn cells_near(&self, obb: &Obb, margin: f32) -> Vec<IVec2> {
        let margin = margin.max(0.0);
        let frame = obb.frame();
        let (min, max) = frame.aabb();
        let (c0, c1) = (self.cell_of(min - Vec2::splat(margin)), self.cell_of(max + Vec2::splat(margin)));
        let mut out = Vec::new();
        for y in c0.y..=c1.y {
            for x in c0.x..=c1.x {
                let c = IVec2::new(x, y);
                if self.in_bounds(c) && frame.distance_to_point(self.cell_center(c)) <= margin {
                    out.push(c);
                }
            }
        }
        let center = self.cell_of(obb.center);
        if self.in_bounds(center) && !out.contains(&center) {
            out.push(center);
        }
        out
    }

    /// How far (metres) movers keep the centre of the cells they use from
    /// static obstacles (0 by default: only covered cells block). Set it
    /// while the grid has no obstacles: obstacles are removed with the
    /// margin they were added with.
    pub fn set_walk_margin(&mut self, margin: f32) {
        debug_assert!(self.walk_blockers.iter().all(|b| *b == 0), "walk margin changed with obstacles registered");
        self.walk_margin = if margin.is_finite() { margin.max(0.0) } else { 0.0 };
        self.version += 1;
    }

    #[inline]
    pub fn walk_margin(&self) -> f32 {
        self.walk_margin
    }

    /// Add (`delta = 1`) or remove (`delta = -1`) a static obstacle footprint.
    /// Always remove with the same Obb that was added.
    pub fn add_obstacle(&mut self, obb: &Obb, delta: i32) {
        let bump = |count: &mut u16| *count = (*count as i32 + delta).clamp(0, u16::MAX as i32) as u16;
        for c in self.cells_covered(obb, 0.0) {
            let i = self.index(c);
            bump(&mut self.blockers[i]);
        }
        for c in self.cells_near(obb, self.walk_margin) {
            let i = self.index(c);
            bump(&mut self.walk_blockers[i]);
        }
        self.version += 1;
    }

    /// Close (`delta = 1`) or reopen (`delta = -1`) cells to movers although
    /// no obstacle comes within the walk margin of their centres: a slot
    /// between two obstacles that is too narrow to pass (af_movement finds
    /// them). Counted with the obstacles' walk blockers, so always reopen
    /// exactly the cells that were closed.
    pub fn add_walk_blocks(&mut self, cells: &[IVec2], delta: i32) {
        if cells.is_empty() {
            return;
        }
        for &c in cells {
            if self.in_bounds(c) {
                let i = self.index(c);
                self.walk_blockers[i] = (self.walk_blockers[i] as i32 + delta).clamp(0, u16::MAX as i32) as u16;
            }
        }
        self.version += 1;
    }

    /// Mark (`Some(team)`) or clear (`None`) a gate footprint.
    pub fn set_gate(&mut self, obb: &Obb, team: Option<u8>) {
        let v = team.map_or(0, |t| t.saturating_add(1));
        for c in self.cells_covered(obb, 0.0) {
            let i = self.index(c);
            self.gates[i] = v;
        }
        self.version += 1;
    }

    /// A static obstacle covers the cell's centre (or it is off the grid).
    #[inline]
    pub fn is_blocked(&self, c: IVec2) -> bool {
        !self.in_bounds(c) || self.blockers[self.index(c)] > 0
    }

    /// A static obstacle comes within the walk margin of the cell's centre
    /// (or it is off the grid): no mover uses the cell.
    #[inline]
    pub fn is_walk_blocked(&self, c: IVec2) -> bool {
        !self.in_bounds(c) || self.walk_blockers[self.index(c)] > 0
    }

    #[inline]
    pub fn gate_team(&self, c: IVec2) -> Option<u8> {
        if !self.in_bounds(c) {
            return None;
        }
        let g = self.gates[self.index(c)];
        if g == 0 { None } else { Some(g - 1) }
    }

    /// Can a mover with `mobility` on `team` stand in cell `c`?
    #[inline]
    pub fn passable(&self, c: IVec2, mobility: u16, team: u8) -> bool {
        if !self.in_bounds(c) {
            return false;
        }
        if mobility & FLYING != 0 {
            return true;
        }
        let i = self.index(c);
        if self.walk_blockers[i] > 0 {
            return false;
        }
        let g = self.gates[i];
        if g != 0 && (team == NO_TEAM || g != team.saturating_add(1)) {
            return false;
        }
        self.terrain_mask[self.terrain[i] as usize] & mobility != 0
    }

    /// Terrain at `c` admits `mobility` (ignores obstacles and gates).
    #[inline]
    pub fn terrain_passable(&self, c: IVec2, mobility: u16) -> bool {
        self.in_bounds(c) && (mobility & FLYING != 0 || self.terrain_mask[self.terrain[self.index(c)] as usize] & mobility != 0)
    }

    #[inline]
    pub fn passable_at(&self, p: Vec2, mobility: u16, team: u8) -> bool {
        self.passable(self.cell_of(p), mobility, team)
    }

    /// True when every cell under `obb` is buildable terrain with no
    /// obstacle and no gate.
    pub fn area_buildable(&self, obb: &Obb) -> bool {
        let cells = self.cells_covered(obb, 0.0);
        if cells.is_empty() {
            return false;
        }
        // Reject footprints that poke outside the map.
        let (min, max) = obb.aabb();
        let wmax = self.origin + self.world_size();
        if min.x < self.origin.x || min.y < self.origin.y || max.x > wmax.x || max.y > wmax.y {
            return false;
        }
        cells.iter().all(|c| {
            let i = self.index(*c);
            self.blockers[i] == 0 && self.gates[i] == 0 && self.terrain_buildable[self.terrain[i] as usize]
        })
    }
}

/// Visit, in order, every cell of a grid (cells of `cell` metres from
/// `origin`) that the segment `a`-`b` passes through (Amanatides-Woo
/// traversal). Stops and returns false as soon as `f` does.
pub(crate) fn traverse_cells(origin: Vec2, cell: f32, a: Vec2, b: Vec2, mut f: impl FnMut(IVec2) -> bool) -> bool {
    let pa = (a - origin) / cell;
    let pb = (b - origin) / cell;
    let mut c = IVec2::new(pa.x.floor() as i32, pa.y.floor() as i32);
    let end = IVec2::new(pb.x.floor() as i32, pb.y.floor() as i32);
    if !f(c) {
        return false;
    }
    let d = pb - pa;
    let step = IVec2::new(if d.x > 0.0 { 1 } else { -1 }, if d.y > 0.0 { 1 } else { -1 });
    let axis = |p: f32, cell: i32, s: i32, dv: f32| -> (f32, f32) {
        if dv.abs() < 1e-9 {
            (f32::INFINITY, f32::INFINITY)
        } else {
            let boundary = if s > 0 { cell as f32 + 1.0 } else { cell as f32 };
            ((boundary - p) / dv, 1.0 / dv.abs())
        }
    };
    let (mut tx, dtx) = axis(pa.x, c.x, step.x, d.x);
    let (mut ty, dty) = axis(pa.y, c.y, step.y, d.y);
    let steps = (end.x - c.x).abs() + (end.y - c.y).abs();
    for _ in 0..steps {
        if tx < ty {
            c.x += step.x;
            tx += dtx;
        } else {
            c.y += step.y;
            ty += dty;
        }
        if !f(c) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obstacles_and_gates() {
        let mut g = NavGrid::new(20, 20, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b1, true);
        let obb = Obb::new(Vec2::new(5.0, 5.0), Vec2::new(1.0, 1.0), 0.0);
        assert!(g.passable(IVec2::new(5, 5), 0b1, 0));
        g.add_obstacle(&obb, 1);
        assert!(!g.passable(IVec2::new(5, 5), 0b1, 0));
        assert!(g.passable(IVec2::new(5, 5), FLYING, 0));
        assert!(!g.area_buildable(&obb));
        g.add_obstacle(&obb, -1);
        assert!(g.passable(IVec2::new(5, 5), 0b1, 0));
        g.set_gate(&obb, Some(2));
        assert!(g.passable(IVec2::new(5, 5), 0b1, 2));
        assert!(!g.passable(IVec2::new(5, 5), 0b1, 1));
        assert!(!g.passable(IVec2::new(5, 5), 0b1, NO_TEAM));
    }

    /// With a walk margin, cells whose centres an obstacle almost reaches
    /// are closed to movers but stay free for building flush against it.
    #[test]
    fn the_walk_margin_closes_cells_an_obstacle_almost_covers() {
        let mut g = NavGrid::new(20, 20, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b1, true);
        g.set_walk_margin(0.25);
        // Right edge at x = 7.3: 0.2 short of the centres of column 7.
        let obb = Obb::new(Vec2::new(6.0, 5.5), Vec2::new(1.3, 0.4), 0.0);
        g.add_obstacle(&obb, 1);
        let (near, far) = (IVec2::new(7, 5), IVec2::new(8, 5));
        assert!(!g.passable(near, 0b1, 0) && g.is_walk_blocked(near));
        assert!(!g.is_blocked(near), "the centre is outside the box");
        assert!(g.area_buildable(&Obb::new(Vec2::new(7.8, 5.5), Vec2::new(0.45, 0.4), 0.0)), "flush placement still fits");
        assert!(g.passable(far, 0b1, 0));
        g.add_obstacle(&obb, -1);
        assert!(g.passable(near, 0b1, 0), "removal releases the margin too");
        assert!(g.walk_blockers.iter().chain(&g.blockers).all(|b| *b == 0));
    }

    /// Two berry bushes from a generated map leave a diagonal slot about a
    /// worker wide, and the only open cell between them has its centre a
    /// centimetre from one of them: with a margin it is no passage.
    #[test]
    fn a_sliver_between_two_obstacles_is_not_a_passage() {
        let mut g = NavGrid::new(20, 20, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b1, true);
        let a = Obb::new(Vec2::new(6.02, 5.00), Vec2::splat(0.7), 0.95);
        let b = Obb::new(Vec2::new(4.38, 3.84), Vec2::splat(0.7), 0.95);
        let cell = IVec2::new(5, 4);
        assert!(a.gap(&b) < 0.65 && a.distance_to_point(g.cell_center(cell)) < 0.05);
        for o in [&a, &b] {
            g.add_obstacle(o, 1);
        }
        assert!(g.passable(cell, 0b1, 0), "without a margin the sliver looks open");
        for o in [&a, &b] {
            g.add_obstacle(o, -1);
        }
        g.set_walk_margin(0.25);
        for o in [&a, &b] {
            g.add_obstacle(o, 1);
        }
        assert!(!g.passable(cell, 0b1, 0), "with a margin it is closed");
    }

    /// Extra walk blocks close cells like obstacles do, count with them,
    /// and reopen exactly what they closed.
    #[test]
    fn walk_blocks_count_with_obstacles() {
        let mut g = NavGrid::new(20, 20, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b1, true);
        let obb = Obb::new(Vec2::new(5.5, 5.5), Vec2::splat(0.4), 0.0);
        let (under, beside) = (IVec2::new(5, 5), IVec2::new(6, 5));
        let version = g.version();
        g.add_walk_blocks(&[under, beside, IVec2::new(-1, 3)], 1);
        assert!(g.version() > version, "path caches see the change");
        assert!(!g.passable(beside, 0b1, 0) && g.is_walk_blocked(beside) && !g.is_blocked(beside), "closed to movers, free to build on");
        g.add_obstacle(&obb, 1);
        g.add_walk_blocks(&[under, beside, IVec2::new(-1, 3)], -1);
        assert!(g.passable(beside, 0b1, 0));
        assert!(!g.passable(under, 0b1, 0), "the obstacle still closes its own cell");
        g.add_obstacle(&obb, -1);
        assert!(g.walk_blockers.iter().all(|b| *b == 0));
        let version = g.version();
        g.add_walk_blocks(&[], 1);
        assert_eq!(g.version(), version, "nothing to do, no change");
    }

    #[test]
    fn coverage_includes_center() {
        let g = NavGrid::new(10, 10, 1.0, Vec2::ZERO);
        let tiny = Obb::new(Vec2::new(3.2, 3.2), Vec2::splat(0.1), 0.0);
        assert_eq!(g.cells_covered(&tiny, 0.0), vec![IVec2::new(3, 3)]);
    }
}
