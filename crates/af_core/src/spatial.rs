//! Uniform-grid spatial index of every unit, building and node, rebuilt at
//! the start of each tick (SimSet::Prepare). Positions are start-of-tick.
//! Use it for "what is near X" instead of scanning queries.
//!
//! Once the rebuild is done every cell lists its entries by entity index
//! ([`SpatialIndex::sort_cells`]), so every search visits them in an order
//! only the simulation decides: not the table order the rebuild's queries
//! yield them in, which the components a client puts on sim entities
//! change. (`entries` itself is in insertion order: nothing in the sim may
//! depend on its order.)

use af_geom::Obb;
use bevy::prelude::*;

use crate::components::PlayerId;
use crate::stats::TagSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialKind {
    Unit,
    Building,
    Node,
}

#[derive(Clone, Copy, Debug)]
pub struct SpatialEntry {
    pub entity: Entity,
    pub kind: SpatialKind,
    /// UnitTypeId / BuildingTypeId / NodeTypeId index, by `kind`.
    pub type_index: u16,
    pub pos: Vec2,
    pub obb: Obb,
    /// Circumscribed radius of `obb`.
    pub radius: f32,
    pub owner: Option<PlayerId>,
    /// Owner's team (`af_geom::NO_TEAM` for nodes).
    pub team: u8,
    pub flying: bool,
    /// Base height above ground and box height (for projectiles/picking).
    pub altitude: f32,
    pub height: f32,
    pub tags: TagSet,
    /// Buildings: still a foundation.
    pub under_construction: bool,
    /// Nodes: blocks movement. Buildings: always true. Units: false.
    pub blocking: bool,
}

#[derive(Resource, Clone, Debug)]
pub struct SpatialIndex {
    cell: f32,
    cols: i32,
    rows: i32,
    /// Per cell: the indices in `entries` of what lies there (by entity
    /// index after [`SpatialIndex::sort_cells`]).
    cells: Vec<Vec<u32>>,
    /// Everything indexed, in insertion order.
    pub entries: Vec<SpatialEntry>,
    /// The entity index of `entries[i]` at `i`: what
    /// [`SpatialIndex::sort_cells`] sorts by, packed tight (the entries are big).
    sort_keys: Vec<u32>,
    /// Cells an insert put out of order (a cell may be listed more than once).
    unsorted: Vec<u32>,
    max_radius: f32,
}

impl Default for SpatialIndex {
    fn default() -> Self {
        SpatialIndex::new(Vec2::splat(64.0), 8.0)
    }
}

impl SpatialIndex {
    pub fn new(world_size: Vec2, cell: f32) -> SpatialIndex {
        let cols = (world_size.x / cell).ceil().max(1.0) as i32;
        let rows = (world_size.y / cell).ceil().max(1.0) as i32;
        SpatialIndex {
            cell,
            cols,
            rows,
            cells: vec![Vec::new(); (cols * rows) as usize],
            entries: Vec::new(),
            sort_keys: Vec::new(),
            unsorted: Vec::new(),
            max_radius: 0.0,
        }
    }

    /// Resize for a new map (clears).
    pub fn reset(&mut self, world_size: Vec2) {
        *self = SpatialIndex::new(world_size, self.cell);
    }

    pub fn clear(&mut self) {
        for c in &mut self.cells {
            c.clear();
        }
        self.entries.clear();
        self.sort_keys.clear();
        self.unsorted.clear();
        self.max_radius = 0.0;
    }

    #[inline]
    fn cell_xy(&self, p: Vec2) -> (i32, i32) {
        (
            ((p.x / self.cell).floor() as i32).clamp(0, self.cols - 1),
            ((p.y / self.cell).floor() as i32).clamp(0, self.rows - 1),
        )
    }

    /// Add an entry (at the end of its cell: see [`SpatialIndex::sort_cells`]).
    pub fn insert(&mut self, e: SpatialEntry) {
        let (x, y) = self.cell_xy(e.pos);
        let i = self.entries.len() as u32;
        self.max_radius = self.max_radius.max(e.radius);
        let (c, key) = ((y * self.cols + x) as usize, e.entity.index_u32());
        let cell = &mut self.cells[c];
        if cell.last().is_some_and(|&last| self.sort_keys[last as usize] > key) {
            self.unsorted.push(c as u32);
        }
        cell.push(i);
        self.sort_keys.push(key);
        self.entries.push(e);
    }

    /// List every cell's entries by entity index (unique among live
    /// entities), whatever order they were inserted in. The rebuild calls it
    /// once it has inserted everything. Cheap: tables mostly hold their
    /// entities in spawn order, which is mostly index order, so most cells are
    /// in order already (`Entity`'s own order would reverse them), and only
    /// the cells an insert put out of order are sorted.
    pub fn sort_cells(&mut self) {
        self.unsorted.sort_unstable();
        self.unsorted.dedup();
        for &c in &self.unsorted {
            self.cells[c as usize].sort_unstable_by_key(|&i| self.sort_keys[i as usize]);
        }
        self.unsorted.clear();
    }

    /// Visit every entry whose box may intersect the circle (centre, radius).
    /// The callback still has to test precisely (e.g. `obb.distance_to_point`).
    pub fn for_each_near(&self, center: Vec2, radius: f32, mut f: impl FnMut(&SpatialEntry)) {
        let r = radius + self.max_radius;
        let (x0, y0) = self.cell_xy(center - Vec2::splat(r));
        let (x1, y1) = self.cell_xy(center + Vec2::splat(r));
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &i in &self.cells[(y * self.cols + x) as usize] {
                    let e = &self.entries[i as usize];
                    if e.pos.distance_squared(center) <= (radius + e.radius) * (radius + e.radius) {
                        f(e);
                    }
                }
            }
        }
    }

    /// Entries whose box is within `radius` of `center` (edge distance).
    pub fn within(&self, center: Vec2, radius: f32) -> Vec<SpatialEntry> {
        let mut out = Vec::new();
        self.for_each_near(center, radius, |e| {
            if e.obb.distance_to_point(center) <= radius {
                out.push(*e);
            }
        });
        out
    }

    /// Nearest entry (by box edge distance from `center`) within `max_dist`
    /// passing `filter`.
    pub fn nearest(&self, center: Vec2, max_dist: f32, mut filter: impl FnMut(&SpatialEntry) -> bool) -> Option<SpatialEntry> {
        let mut best: Option<(f32, SpatialEntry)> = None;
        self.for_each_near(center, max_dist, |e| {
            let d = e.obb.distance_to_point(center);
            if d <= max_dist && best.is_none_or(|(bd, _)| d < bd) && filter(e) {
                best = Some((d, *e));
            }
        });
        best.map(|(_, e)| e)
    }

    /// Nearest entry anywhere on the map passing `filter`, searching in
    /// growing rings (cheap when something is close).
    pub fn nearest_anywhere(&self, center: Vec2, mut filter: impl FnMut(&SpatialEntry) -> bool) -> Option<SpatialEntry> {
        let mut r = 16.0;
        let max = (self.cols.max(self.rows) as f32) * self.cell * 1.5;
        while r <= max * 2.0 {
            if let Some(e) = self.nearest(center, r, &mut filter) {
                return Some(e);
            }
            r *= 2.0;
        }
        None
    }

    /// Entry for an entity (linear scan; prefer queries when you can).
    pub fn find(&self, e: Entity) -> Option<&SpatialEntry> {
        self.entries.iter().find(|x| x.entity == e)
    }

    /// Topmost entry hit by a picking ray (closest along the ray).
    pub fn pick(&self, origin: Vec3, dir: Vec3, mut filter: impl FnMut(&SpatialEntry) -> bool) -> Option<SpatialEntry> {
        let mut best: Option<(f32, SpatialEntry)> = None;
        for e in &self.entries {
            if !filter(e) {
                continue;
            }
            if let Some(t) = e.obb.ray_hit(origin, dir, e.altitude, e.altitude + e.height)
                && best.is_none_or(|(bt, _)| t < bt)
            {
                best = Some((t, *e));
            }
        }
        best.map(|(_, e)| e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit's entry for entity index `i` at `pos`.
    fn entry(i: u32, pos: Vec2) -> SpatialEntry {
        let obb = Obb::new(pos, Vec2::splat(0.3), 0.0);
        SpatialEntry {
            entity: Entity::from_raw_u32(i).expect("entity"),
            kind: SpatialKind::Unit,
            type_index: 0,
            pos,
            obb,
            radius: obb.bounding_radius(),
            owner: Some(PlayerId(0)),
            team: 0,
            flying: false,
            altitude: 0.0,
            height: 1.0,
            tags: TagSet::default(),
            under_construction: false,
            blocking: false,
        }
    }

    /// The entities a search near `at` visits, in visiting order.
    fn visited(index: &SpatialIndex, at: Vec2) -> Vec<u32> {
        let mut out = Vec::new();
        index.for_each_near(at, 20.0, |e| out.push(e.entity.index_u32()));
        out
    }

    /// Whatever order the entries come in, a search visits them in the same
    /// order (each cell by entity index), and of equals the nearest-first
    /// search picks the same one: queries hand the rebuild their entities in
    /// table order, which client components change.
    #[test]
    fn searches_do_not_depend_on_insertion_order() {
        // Two cells; equal distances from the centre of the first.
        let spots = [
            (5, Vec2::new(10.0, 10.0)),
            (9, Vec2::new(12.0, 10.0)),
            (2, Vec2::new(8.0, 10.0)),
            (7, Vec2::new(10.0, 12.0)),
            (4, Vec2::new(19.0, 10.0)),
        ];
        let build = |order: &[usize]| {
            let mut index = SpatialIndex::new(Vec2::splat(64.0), 8.0);
            for &k in order {
                index.insert(entry(spots[k].0, spots[k].1));
            }
            index.sort_cells();
            index
        };
        let a = build(&[0, 1, 2, 3, 4]);
        let b = build(&[4, 3, 1, 0, 2]);
        assert_eq!(visited(&a, Vec2::new(10.0, 10.0)), visited(&b, Vec2::new(10.0, 10.0)));
        assert_eq!(visited(&a, Vec2::new(10.0, 10.0))[..4], [2, 5, 7, 9], "one cell, by entity index");
        // Entities 2, 7 and 9 lie equally far from 5.
        let nearest =
            |index: &SpatialIndex| index.nearest(Vec2::new(10.0, 10.0), 5.0, |e| e.entity.index_u32() != 5).map(|e| e.entity.index_u32());
        assert_eq!(nearest(&a), nearest(&b));
    }
}
