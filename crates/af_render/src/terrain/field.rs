//! The shape of the drawn terrain: what every cell looks like ([`Cover`])
//! and the height of every cell corner, over the map and a [`SKIRT`] of
//! off-map land around it. A pure function of the map (the nav terrain,
//! the seed, and `MapFeatures` when af_map publishes them), built once per
//! match.
//!
//! * Corners touching land are at that land's ground
//!   (`af_core::ext::render::corner_ground`), so every cell units walk on is
//!   drawn exactly where they stand.
//! * Corners touching only water are on its bed, falling with the distance
//!   from the shore: fords -0.32 m, shallows -0.5 to -0.8 m, deep water -1.4
//!   to -2.5 m, the sea to -4 m.
//! * Corners touching only mountains rise with the distance into the range,
//!   shaped by ridged noise, to peaks of 6.8-10.5 m.
//! * Off the map the edge carries on: rivers along their own winding
//!   course, narrowing into the distance; other water and mountains
//!   straight out; the sea beyond a sea edge; and otherwise meadow,
//!   woodland and a frame of mountains further out.

use af_core::ext::map::{MapEdge, MapFeatures, RiverLine, SiteKind};
use af_core::ext::render::{MAX_TERRAIN_HEIGHT, WATER_LEVEL, is_barrier_terrain, is_water_terrain, nearer_zero, terrain_ground};
use af_core::prelude::*;
use bevy::math::{IVec2, Vec2, Vec3};

use super::noise::{crowns, fbm, ridged, smoothstep, value};

/// Width of the off-map skirt around the map, in cells.
pub const SKIRT: i32 = 64;
/// How fast river water is drawn flowing, metres per second, mid-stream.
pub const RIVER_SPEED: f32 = 1.1;
/// How far (cells) a mountain cell looks for its range's crest; deeper
/// inside a massif than this, the rock rises to summits and saddles.
pub const RANGE_REACH: i32 = 9;
/// Width of a river's shallow banks beyond its deep core (metres), where
/// the field paints a river's course across the skirt.
pub(crate) const RIVER_BANK: f32 = 2.0;

/// A river's centreline (a point every ~0.75 m, source first) and the
/// half-width of its deep core at each point.
#[derive(Clone, Debug, Default)]
pub struct Course {
    pub points: Vec<Vec2>,
    pub half: Vec<f32>,
}

/// What a cell looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cover {
    Meadow,
    Sand,
    Marsh,
    /// Bramble thicket: forest floor under brambles.
    Thicket,
    /// Off-map woodland: forest floor, no brambles.
    Woodland,
    /// Climbable rocky highland: a low plateau.
    Highland,
    Mountain,
    /// A meteor site's burnt floor.
    Scar,
    Ford,
    Shallows,
    Deep,
    Ocean,
}

impl Cover {
    /// How a terrain type is drawn: by its id, else guessed from its rules.
    pub fn of(def: &TerrainDef) -> Cover {
        match def.id.as_str() {
            "grass" => Cover::Meadow,
            "sand" => Cover::Sand,
            "marsh" => Cover::Marsh,
            "thicket" => Cover::Thicket,
            "cliff" => Cover::Highland,
            "mountain" => Cover::Mountain,
            "scorched" => Cover::Scar,
            "ford" => Cover::Ford,
            "shallow_water" => Cover::Shallows,
            "deep_water" => Cover::Deep,
            "ocean" => Cover::Ocean,
            _ if is_water_terrain(def) => {
                if def.passable.contains(&MoveFlag::Foot) { Cover::Shallows } else { Cover::Deep }
            }
            _ if is_barrier_terrain(def) => Cover::Mountain,
            _ if def.passable.contains(&MoveFlag::Climber) => Cover::Highland,
            _ if def.height > 0.5 => Cover::Thicket,
            _ => Cover::Meadow,
        }
    }

    pub fn is_water(self) -> bool {
        matches!(self, Cover::Ford | Cover::Shallows | Cover::Deep | Cover::Ocean)
    }

    /// Ground units walk on it (or could): not water, not mountains.
    pub fn is_land(self) -> bool {
        !self.is_water() && self != Cover::Mountain
    }

    /// Which of the eight paint channels draws it (see `terrain.wgsl`):
    /// meadow, sand, marsh, forest floor | rock, mountain, scar, water bed.
    pub fn channel(self) -> usize {
        match self {
            Cover::Meadow => 0,
            Cover::Sand => 1,
            Cover::Marsh => 2,
            Cover::Thicket | Cover::Woodland => 3,
            Cover::Highland => 4,
            Cover::Mountain => 5,
            Cover::Scar => 6,
            Cover::Ford | Cover::Shallows | Cover::Deep | Cover::Ocean => 7,
        }
    }
}

/// The terrain over the map and its skirt. Cells and corners are indexed
/// in "field" coordinates: field cell (x, z) is map cell (x - skirt, z -
/// skirt), and field corner (x, z) is the min corner of field cell (x, z).
#[derive(Clone, Debug)]
pub struct Field {
    /// World position of field corner (0, 0).
    pub origin: Vec2,
    pub cell: f32,
    pub skirt: i32,
    /// Field cells per row and rows (map plus skirt on both sides).
    pub cols: i32,
    pub rows: i32,
    pub map_cols: i32,
    pub map_rows: i32,
    pub seed: u32,
    pub cover: Vec<Cover>,
    /// Ground of each cell (`terrain_ground`; 0 off the map).
    pub ground: Vec<f32>,
    /// Map cells something can be built on (false off the map): where no
    /// decoration may stand.
    pub buildable: Vec<bool>,
    /// Cells from each cell to the nearest cell across the shore (water to
    /// land, land to water).
    pub shore: Vec<f32>,
    /// Cells from each cell to the nearest cell across a mountain's edge.
    pub rock: Vec<f32>,
    /// How deep the range a mountain cell belongs to is around it: the
    /// greatest `rock` within [`RANGE_REACH`] cells (its crest's distance
    /// from the edge), so the flanks rise all the way to a sharp crest.
    pub depth: Vec<f32>,
    /// Cells from each off-map woodland cell to the wood's edge (0 elsewhere).
    pub wood: Vec<f32>,
    /// How far each cell of a meteor site's rim (highland in a site) lies
    /// inside it: cells to the nearest cell off the rim (1 at its edges,
    /// most along its crest); 0 off the rims.
    pub rim: Vec<f32>,
    /// Cells from each cell to the nearest cell of a meteor site's rim
    /// (capped at [`SKIRT`] when there is none).
    pub to_rim: Vec<f32>,
    /// River flow on water cells, metres per second (zero for still water).
    pub flow: Vec<Vec2>,
    /// Where a flowing cell lies along its river: (metres downstream from
    /// the source, metres left of the centreline). Smooth across the river,
    /// so the water shader can stream ripples and foam down it.
    pub course: Vec<Vec2>,
    /// How much a water cell is a river (0 lake or sea, 1 river).
    pub river: Vec<f32>,
    /// Height of every corner, `(cols + 1) * (rows + 1)`, row-major.
    pub heights: Vec<f32>,
    /// A crystal spire (centre, radius) drawn in place of a mountain massif.
    pub spire: Option<(Vec2, f32)>,
    /// Meteor sites (centre, radius), main first.
    pub sites: Vec<(Vec2, f32)>,
    /// The rivers' lines (af_map's, carried on across the skirt; none
    /// without `MapFeatures`), source first.
    pub courses: Vec<Course>,
}

/// What a [`Field`] is built from.
pub struct FieldSource<'a> {
    pub data: &'a GameData,
    pub nav: &'a NavGrid,
    pub seed: u64,
    pub features: Option<&'a MapFeatures>,
    /// The main meteor site (centre, radius) when there are no features.
    pub crater: Option<(Vec2, f32)>,
}

impl Field {
    pub fn build(src: &FieldSource) -> Field {
        let nav = src.nav;
        let (map_cols, map_rows) = (nav.width() as i32, nav.height() as i32);
        let skirt = SKIRT;
        let (cols, rows) = (map_cols + 2 * skirt, map_rows + 2 * skirt);
        let cell = nav.cell_size();
        let seed = (src.seed ^ (src.seed >> 32)) as u32;
        let n = (cols * rows) as usize;
        let mut f = Field {
            origin: nav.origin() - Vec2::splat(skirt as f32 * cell),
            cell,
            skirt,
            cols,
            rows,
            map_cols,
            map_rows,
            seed,
            cover: vec![Cover::Meadow; n],
            ground: vec![0.0; n],
            buildable: vec![false; n],
            shore: Vec::new(),
            rock: Vec::new(),
            depth: Vec::new(),
            wood: Vec::new(),
            rim: Vec::new(),
            to_rim: Vec::new(),
            flow: vec![Vec2::ZERO; n],
            course: vec![Vec2::ZERO; n],
            river: vec![0.0; n],
            heights: Vec::new(),
            spire: None,
            sites: Vec::new(),
            courses: Vec::new(),
        };
        let looks: Vec<(Cover, f32)> = src.data.terrain.iter().map(|d| (Cover::of(d), terrain_ground(d))).collect();
        let buildable: Vec<bool> = src.data.terrain.iter().map(|d| d.buildable).collect();
        let ocean_edges: Vec<MapEdge> = src.features.map(|m| m.ocean_edges.clone()).unwrap_or_default();
        // River lines, carried on across the skirt where they leave the map.
        let courses: Vec<Course> = src.features.map(|m| m.rivers.iter().map(|r| f.carry_on(r)).collect()).unwrap_or_default();
        let mouths = f.river_mouths(&courses);
        for z in 0..rows {
            for x in 0..cols {
                let i = f.index(x, z);
                let m = IVec2::new(x - skirt, z - skirt);
                if f.on_map(x, z) {
                    let t = nav.terrain_at(m) as usize;
                    let (cover, ground) = looks.get(t).copied().unwrap_or((Cover::Meadow, 0.0));
                    f.cover[i] = cover;
                    f.ground[i] = ground;
                    f.buildable[i] = buildable.get(t).copied().unwrap_or(true);
                } else {
                    // Off-map land is at 0, like the grass `corner_ground`
                    // assumes off the map (water and rock use their own rules).
                    let edge = IVec2::new(m.x.clamp(0, map_cols - 1), m.y.clamp(0, map_rows - 1));
                    let mut edge_cover = looks.get(nav.terrain_at(edge) as usize).map_or(Cover::Meadow, |l| l.0);
                    // A river leaving the map is drawn along its own course
                    // below, not straight on from the edge; but the first
                    // ring of cells beyond the edge always carries its water
                    // on, as `corner_ground` (`off_map_terrain`) assumes.
                    let first_ring = (m - edge).abs().max_element() <= 1;
                    if !first_ring && mouths[(edge.y * map_cols + edge.x) as usize] && edge_cover.is_water() && edge_cover != Cover::Ocean {
                        edge_cover = Cover::Meadow;
                    }
                    let sea = beyond_edges(m, map_cols, map_rows).iter().any(|e| ocean_edges.contains(e));
                    f.cover[i] = off_map_cover(f.world_cell(x, z), (m - edge).as_vec2().length() * cell, edge_cover, sea, seed);
                }
            }
        }
        f.paint_skirt_rivers(&courses);
        let (to_land, to_water) = (f.distance(|c| !c.is_water()), f.distance(Cover::is_water));
        f.shore = f.cover.iter().enumerate().map(|(i, c)| if c.is_water() { to_land[i] } else { to_water[i] }).collect();
        let (to_open, to_rock) = (f.distance(|c| c != Cover::Mountain), f.distance(|c| c == Cover::Mountain));
        f.rock = f.cover.iter().enumerate().map(|(i, c)| if *c == Cover::Mountain { to_open[i] } else { to_rock[i] }).collect();
        f.depth = f.range_depth(&to_open);
        f.wood = f.distance(|c| c != Cover::Woodland).into_iter().zip(&f.cover).map(|(d, c)| if *c == Cover::Woodland { d } else { 0.0 }).collect();
        f.lay_rivers(&courses);
        f.sites = match src.features {
            Some(m) => m.sites.iter().map(|s| (s.center, s.radius)).collect(),
            None => src.crater.into_iter().filter(|(_, r)| *r > 0.0).collect(),
        };
        f.lay_rims();
        f.spire = src.features.and_then(|m| {
            let site = m.sites.iter().find(|s| s.kind == SiteKind::Spire)?;
            let land = (m.land_max - m.land_min).min_element().max(1.0);
            Some((site.center, land * 0.04 + 1.5))
        });
        f.heights = f.corner_heights();
        f.courses = courses;
        f
    }

    pub fn index(&self, x: i32, z: i32) -> usize {
        (z * self.cols + x) as usize
    }

    pub fn on_map(&self, x: i32, z: i32) -> bool {
        let (mx, mz) = (x - self.skirt, z - self.skirt);
        mx >= 0 && mz >= 0 && mx < self.map_cols && mz < self.map_rows
    }

    /// Is a field cell on the map or in the first ring of cells round it
    /// (one that touches a corner of the map)?
    pub fn near_map(&self, x: i32, z: i32) -> bool {
        let (mx, mz) = (x - self.skirt, z - self.skirt);
        mx >= -1 && mz >= -1 && mx <= self.map_cols && mz <= self.map_rows
    }

    /// How far a world point lies beyond the map's edge (0 on the map).
    pub fn beyond_map(&self, p: Vec2) -> f32 {
        let lo = self.world_corner(self.skirt, self.skirt);
        let hi = self.world_corner(self.skirt + self.map_cols, self.skirt + self.map_rows);
        (lo - p).max(p - hi).max(Vec2::ZERO).length()
    }

    /// The cover of a field cell (clamped into the field).
    pub fn cover_at(&self, x: i32, z: i32) -> Cover {
        self.cover[self.index(x.clamp(0, self.cols - 1), z.clamp(0, self.rows - 1))]
    }

    /// World position of a field cell's centre.
    pub fn world_cell(&self, x: i32, z: i32) -> Vec2 {
        self.origin + (Vec2::new(x as f32, z as f32) + 0.5) * self.cell
    }

    /// World position of a field corner.
    pub fn world_corner(&self, x: i32, z: i32) -> Vec2 {
        self.origin + Vec2::new(x as f32, z as f32) * self.cell
    }

    /// The field cell holding a world point (unclamped).
    pub fn cell_of(&self, p: Vec2) -> IVec2 {
        ((p - self.origin) / self.cell).floor().as_ivec2()
    }

    /// Height of a field corner (clamped into the field).
    pub fn height(&self, x: i32, z: i32) -> f32 {
        let (x, z) = (x.clamp(0, self.cols), z.clamp(0, self.rows));
        self.heights[(z * (self.cols + 1) + x) as usize]
    }

    /// Smooth normal at a corner, from the heights around it, so chunks
    /// built separately meet without a seam in their shading.
    pub fn normal(&self, x: i32, z: i32) -> Vec3 {
        let dx = (self.height(x + 1, z) - self.height(x - 1, z)) / (2.0 * self.cell);
        let dz = (self.height(x, z + 1) - self.height(x, z - 1)) / (2.0 * self.cell);
        Vec3::new(-dx, 1.0, -dz).normalize()
    }

    /// The cells touching a corner (clamped into the field).
    pub fn corner_cells(&self, x: i32, z: i32) -> [usize; 4] {
        let c = |dx: i32, dz: i32| self.index((x + dx).clamp(0, self.cols - 1), (z + dz).clamp(0, self.rows - 1));
        [c(-1, -1), c(0, -1), c(-1, 0), c(0, 0)]
    }

    /// Does any cell around a corner hold water?
    pub fn corner_wet(&self, x: i32, z: i32) -> bool {
        self.corner_cells(x, z).iter().any(|i| self.cover[*i].is_water())
    }

    /// Is the cell a land cell, so its triangulation must match
    /// `ground_height` (the fixed diagonal)?
    pub fn cell_is_land(&self, x: i32, z: i32) -> bool {
        self.cover_at(x, z).is_land()
    }

    /// Is a world point within one of the meteor sites (rims included)?
    pub fn in_site(&self, p: Vec2) -> bool {
        self.sites.iter().any(|(c, r)| p.distance(*c) < r * 1.15 + 2.0)
    }

    /// The meteor sites' rims (their highland): how deep inside a rim each
    /// of its cells lies, and how far every cell is from a rim.
    fn lay_rims(&mut self) {
        let n = self.cover.len();
        let on_rim: Vec<bool> = (0..n)
            .map(|i| self.cover[i] == Cover::Highland && self.in_site(self.world_cell(i as i32 % self.cols, i as i32 / self.cols)))
            .collect();
        if !on_rim.contains(&true) {
            self.rim = vec![0.0; n];
            self.to_rim = vec![SKIRT as f32; n];
            return;
        }
        let inside = self.distance(|c| c != Cover::Highland);
        self.rim = (0..n).map(|i| if on_rim[i] { inside[i] } else { 0.0 }).collect();
        self.to_rim = self.distance_to(&on_rim);
    }

    /// Chamfer distance (in cells) from every cell to the nearest cell
    /// `target` holds for (capped at [`SKIRT`] when there is none).
    fn distance(&self, target: impl Fn(Cover) -> bool) -> Vec<f32> {
        let mask: Vec<bool> = self.cover.iter().map(|c| target(*c)).collect();
        self.distance_to(&mask)
    }

    /// Chamfer distance (in cells) from every cell to the nearest cell
    /// marked in `target` (capped at [`SKIRT`] when none is).
    pub(crate) fn distance_to(&self, target: &[bool]) -> Vec<f32> {
        const FAR: f32 = 1.0e6;
        const DIAG: f32 = std::f32::consts::SQRT_2;
        let (w, h) = (self.cols, self.rows);
        let mut d: Vec<f32> = target.iter().map(|t| if *t { 0.0 } else { FAR }).collect();
        if !d.contains(&0.0) {
            return vec![SKIRT as f32; d.len()];
        }
        let at = |x: i32, z: i32| (z * w + x) as usize;
        for z in 0..h {
            for x in 0..w {
                let i = at(x, z);
                let mut v = d[i];
                if x > 0 {
                    v = v.min(d[at(x - 1, z)] + 1.0);
                }
                if z > 0 {
                    v = v.min(d[at(x, z - 1)] + 1.0);
                    if x > 0 {
                        v = v.min(d[at(x - 1, z - 1)] + DIAG);
                    }
                    if x + 1 < w {
                        v = v.min(d[at(x + 1, z - 1)] + DIAG);
                    }
                }
                d[i] = v;
            }
        }
        for z in (0..h).rev() {
            for x in (0..w).rev() {
                let i = at(x, z);
                let mut v = d[i];
                if x + 1 < w {
                    v = v.min(d[at(x + 1, z)] + 1.0);
                }
                if z + 1 < h {
                    v = v.min(d[at(x, z + 1)] + 1.0);
                    if x + 1 < w {
                        v = v.min(d[at(x + 1, z + 1)] + DIAG);
                    }
                    if x > 0 {
                        v = v.min(d[at(x - 1, z + 1)] + DIAG);
                    }
                }
                d[i] = v;
            }
        }
        d
    }

    /// The greatest of `inside` (distances into the mountains) within
    /// [`RANGE_REACH`] cells of each mountain cell (a square max filter,
    /// rows then columns); 0 elsewhere.
    fn range_depth(&self, inside: &[f32]) -> Vec<f32> {
        let (w, h, r) = (self.cols, self.rows, RANGE_REACH);
        let rock = |i: usize| if self.cover[i] == Cover::Mountain { inside[i] } else { 0.0 };
        let mut rows = vec![0.0f32; inside.len()];
        for z in 0..h {
            for x in 0..w {
                let i = self.index(x, z);
                if self.cover[i] != Cover::Mountain {
                    continue;
                }
                rows[i] = ((x - r).max(0)..=(x + r).min(w - 1)).map(|xx| rock(self.index(xx, z))).fold(0.0, f32::max);
            }
        }
        let mut out = vec![0.0f32; inside.len()];
        for z in 0..h {
            for x in 0..w {
                let i = self.index(x, z);
                if self.cover[i] != Cover::Mountain {
                    continue;
                }
                // A column's rows were filled for its mountain cells only;
                // non-mountain cells add nothing.
                out[i] = ((z - r).max(0)..=(z + r).min(h - 1))
                    .map(|zz| {
                        let j = self.index(x, zz);
                        if self.cover[j] == Cover::Mountain { rows[j] } else { 0.0 }
                    })
                    .fold(0.0, f32::max);
            }
        }
        out
    }

    /// A river line carried on beyond the map: from each end that lies
    /// beyond the edge, on across the skirt and past it, bending gently
    /// (its meanders grow from nothing at the end, so the course has no
    /// kink) but always heading away from the map, and narrowing as it
    /// goes, so it fades into the distance instead of running out straight.
    fn carry_on(&self, river: &RiverLine) -> Course {
        let pts = &river.points;
        let half_at = |i: usize| river.half_widths.get(i).copied().unwrap_or(2.5);
        if pts.len() < 2 {
            return Course { points: pts.clone(), half: (0..pts.len()).map(half_at).collect() };
        }
        let lo = self.world_corner(self.skirt, self.skirt);
        let hi = self.world_corner(self.skirt + self.map_cols, self.skirt + self.map_rows);
        let length = self.skirt as f32 * self.cell * 1.7 + 12.0;
        let reach_back = 6.min(pts.len() - 1);
        let extend = |end: usize, inner: usize, salt: u32| -> Vec<(Vec2, f32)> {
            let p0 = pts[end];
            if self.beyond_map(p0) < 0.5 {
                return Vec::new();
            }
            // Away from the map: across the edge(s) the end lies beyond.
            let mut out = Vec2::ZERO;
            out.x = if p0.x < lo.x { -1.0 } else if p0.x > hi.x { 1.0 } else { 0.0 };
            out.y = if p0.y < lo.y { -1.0 } else if p0.y > hi.y { 1.0 } else { 0.0 };
            let out = out.normalize_or(Vec2::X);
            let start_dir = (p0 - pts[inner]).normalize_or(out);
            let (wave, phase) = (44.0 + 22.0 * value(Vec2::splat(salt as f32 * 0.37), self.seed ^ salt), value(Vec2::new(3.1, salt as f32), self.seed ^ 0x51ab) * std::f32::consts::TAU);
            let h0 = half_at(end);
            let (mut p, mut s, mut v) = (p0, 0.0f32, Vec::new());
            while s < length {
                let grow = smoothstep(0.0, 16.0, s);
                // Turn from the river's own heading to straight away from
                // the map, then wander about that.
                let base = start_dir.lerp(out, smoothstep(0.0, 28.0, s)).normalize_or(out);
                let bend = grow * (0.62 * (s * std::f32::consts::TAU / wave + phase).sin() + 0.55 * (value(Vec2::new(s * 0.05, salt as f32), self.seed ^ 0x51ac) - 0.5));
                let mut dir = Vec2::from_angle(bend).rotate(base);
                let away = dir.dot(out);
                if away < 0.35 {
                    dir = (dir + out * (0.35 - away) * 2.0).normalize_or(out);
                }
                p += dir * 0.75;
                s += 0.75;
                v.push((p, h0 * (1.0 - 0.65 * smoothstep(6.0, 56.0, self.beyond_map(p)))));
            }
            v
        };
        let head = extend(0, reach_back, 0x1d);
        let tail = extend(pts.len() - 1, pts.len() - 1 - reach_back, 0x2e);
        let mut course = Course { points: Vec::with_capacity(head.len() + pts.len() + tail.len()), half: Vec::new() };
        for (p, h) in head.into_iter().rev() {
            course.points.push(p);
            course.half.push(h);
        }
        for i in 0..pts.len() {
            course.points.push(pts[i]);
            course.half.push(half_at(i));
        }
        for (p, h) in tail {
            course.points.push(p);
            course.half.push(h);
        }
        course
    }

    /// The map's edge cells a river leaves by (indexed like the map), so the
    /// skirt beyond them follows the river's course instead of carrying the
    /// edge straight on.
    fn river_mouths(&self, courses: &[Course]) -> Vec<bool> {
        let mut out = vec![false; (self.map_cols * self.map_rows) as usize];
        let lo = self.world_corner(self.skirt, self.skirt);
        let hi = self.world_corner(self.skirt + self.map_cols, self.skirt + self.map_rows);
        for c in courses {
            for (p, half) in c.points.iter().zip(&c.half) {
                let reach = half + RIVER_BANK + 3.0;
                // Only points near the edge, inside or out.
                if self.beyond_map(*p) > reach + 2.0 || (*p - lo).min(hi - *p).min_element() > reach + self.cell {
                    continue;
                }
                let at = self.cell_of(*p) - IVec2::splat(self.skirt);
                let r = (reach / self.cell).ceil() as i32 + 1;
                for z in (at.y - r).max(0)..=(at.y + r).min(self.map_rows - 1) {
                    for x in (at.x - r).max(0)..=(at.x + r).min(self.map_cols - 1) {
                        let edge = x == 0 || z == 0 || x == self.map_cols - 1 || z == self.map_rows - 1;
                        if edge && self.world_cell(x + self.skirt, z + self.skirt).distance(*p) < reach {
                            out[(z * self.map_cols + x) as usize] = true;
                        }
                    }
                }
            }
        }
        out
    }

    /// Paint the rivers' courses over the skirt: a deep core within each
    /// point's half-width and shallow banks beyond it (the sea is left as
    /// it is, and so is the first ring of cells round the map, which
    /// carries the edge on as the ground units walk on assumes).
    fn paint_skirt_rivers(&mut self, courses: &[Course]) {
        let lo = self.world_corner(self.skirt, self.skirt);
        let hi = self.world_corner(self.skirt + self.map_cols, self.skirt + self.map_rows);
        for c in courses {
            for (p, half) in c.points.iter().zip(&c.half) {
                let reach = half + RIVER_BANK;
                // Points well inside the map paint nothing off it.
                if (*p - lo).min(hi - *p).min_element() > reach + self.cell {
                    continue;
                }
                let at = self.cell_of(*p);
                let r = (reach / self.cell).ceil() as i32 + 1;
                for z in (at.y - r).max(0)..=(at.y + r).min(self.rows - 1) {
                    for x in (at.x - r).max(0)..=(at.x + r).min(self.cols - 1) {
                        if self.near_map(x, z) {
                            continue;
                        }
                        let i = self.index(x, z);
                        let d = self.world_cell(x, z).distance(*p);
                        let now = self.cover[i];
                        if d < *half && now != Cover::Ocean {
                            self.cover[i] = Cover::Deep;
                        } else if d < reach && !now.is_water() {
                            self.cover[i] = Cover::Shallows;
                        }
                    }
                }
            }
        }
    }

    /// River flow and how river-like each water cell is: from af_map's
    /// river lines (carried on across the skirt) when it publishes them,
    /// else from the shape of each body of water (narrow bodies are rivers),
    /// drawn still.
    fn lay_rivers(&mut self, courses: &[Course]) {
        match courses {
            [_, ..] => {
                let mut best = vec![f32::MAX; self.cover.len()];
                for river in courses {
                    let pts = &river.points;
                    let mut arc = 0.0;
                    for i in 0..pts.len() {
                        if i > 0 {
                            arc += pts[i].distance(pts[i - 1]);
                        }
                        let tangent = (pts[(i + 1).min(pts.len() - 1)] - pts[i.saturating_sub(1)]).normalize_or_zero();
                        let half = river.half.get(i).copied().unwrap_or(2.5);
                        let reach = half + 3.5;
                        let c = self.cell_of(pts[i]);
                        let r = (reach / self.cell).ceil() as i32 + 1;
                        for z in (c.y - r).max(0)..=(c.y + r).min(self.rows - 1) {
                            for x in (c.x - r).max(0)..=(c.x + r).min(self.cols - 1) {
                                let j = self.index(x, z);
                                if !self.cover[j].is_water() || self.cover[j] == Cover::Ocean {
                                    continue;
                                }
                                let off = self.world_cell(x, z) - pts[i];
                                let d = off.length();
                                if d > reach || d >= best[j] {
                                    continue;
                                }
                                best[j] = d;
                                let core = 1.0 - smoothstep(half * 0.6, reach, d);
                                self.flow[j] = tangent * RIVER_SPEED * (0.35 + 0.65 * core);
                                self.course[j] = Vec2::new(arc + off.dot(tangent), tangent.perp_dot(off));
                                self.river[j] = 1.0 - smoothstep(reach - 1.0, reach, d) * 0.5;
                            }
                        }
                    }
                }
            }
            _ => self.guess_rivers(),
        }
    }

    /// Without river lines: a body of water whose deepest point is close to
    /// its shore is a river, a wide one a lake.
    fn guess_rivers(&mut self) {
        let mut body = vec![u32::MAX; self.cover.len()];
        let mut stack = Vec::new();
        let mut members = Vec::new();
        for start in 0..self.cover.len() {
            let c = self.cover[start];
            if !c.is_water() || c == Cover::Ocean || body[start] != u32::MAX {
                continue;
            }
            members.clear();
            stack.push(start);
            body[start] = start as u32;
            let mut widest: f32 = 0.0;
            while let Some(i) = stack.pop() {
                members.push(i);
                widest = widest.max(self.shore[i]);
                let (x, z) = ((i as i32) % self.cols, (i as i32) / self.cols);
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, nz) = (x + dx, z + dz);
                    if nx < 0 || nz < 0 || nx >= self.cols || nz >= self.rows {
                        continue;
                    }
                    let j = self.index(nx, nz);
                    let cj = self.cover[j];
                    if cj.is_water() && cj != Cover::Ocean && body[j] == u32::MAX {
                        body[j] = start as u32;
                        stack.push(j);
                    }
                }
            }
            let riverness = 1.0 - smoothstep(5.5, 9.0, widest * self.cell);
            for i in &members {
                self.river[*i] = riverness;
            }
        }
    }

    fn corner_heights(&self) -> Vec<f32> {
        let w = self.cols + 1;
        let corners = par_rows(self.rows + 1, |z| (0..=self.cols).map(|x| (self.corner_height(x, z), self.corner_cells(x, z).iter().all(|c| self.cover[*c].is_water()))).collect());
        let (mut h, bed): (Vec<f32>, Vec<bool>) = corners.into_iter().unzip();
        // Water beds only show through the water's colour: smooth them, so
        // deep water fades into shallows without the cells' steps.
        for _ in 0..3 {
            let prev = h.clone();
            for z in 1..self.rows {
                for x in 1..self.cols {
                    let i = (z * w + x) as usize;
                    if !bed[i] {
                        continue;
                    }
                    let (mut sum, mut n) = (0.0, 0.0);
                    for j in [i - 1, i + 1, i - w as usize, i + w as usize, i] {
                        if bed[j] {
                            sum += prev[j];
                            n += 1.0;
                        }
                    }
                    h[i] = sum / n;
                }
            }
        }
        h
    }

    /// The height rule for one corner (see the module docs).
    fn corner_height(&self, x: i32, z: i32) -> f32 {
        let cells = self.corner_cells(x, z);
        let mut land = None::<f32>;
        let mut bed = None::<f32>;
        let (mut rock, mut crest) = (f32::MAX, 0.0f32);
        for i in cells {
            let c = self.cover[i];
            if c.is_land() {
                land = Some(land.map_or(self.ground[i], |g| nearer_zero(g, self.ground[i])));
            } else if c.is_water() {
                let b = water_bed(c, (self.shore[i] - 0.5).max(0.0) * self.cell);
                bed = Some(bed.map_or(b, |v: f32| v.max(b)));
            } else {
                rock = rock.min(self.rock[i]);
                crest = crest.max(self.depth[i]);
            }
        }
        if let Some(g) = land {
            // Off the map nothing stands: the land rolls and woods rise.
            if cells.iter().all(|i| !self.on_map((*i as i32) % self.cols, (*i as i32) / self.cols)) {
                let wood = cells.iter().map(|i| self.wood[*i]).fold(0.0, f32::max) * self.cell;
                let shore = cells.iter().map(|i| if self.cover[*i].is_water() { 0.0 } else { self.shore[*i] }).fold(f32::MAX, f32::min) * self.cell;
                return g + skirt_relief(self.world_corner(x, z), self.beyond_map(self.world_corner(x, z)), wood, shore, self.seed);
            }
            return g;
        }
        if let Some(b) = bed {
            return b;
        }
        let p = self.world_corner(x, z);
        let d = (rock - 0.5).max(0.0) * self.cell;
        let height = mountain_height(p, d, (crest - 0.5).max(0.5) * self.cell, self.seed);
        match self.spire {
            Some((c, r)) if p.distance(c) < r + 2.0 => height.min(spire_base(d)),
            _ => height,
        }
    }

    /// The paint channel of a field cell (clamped into the field): its
    /// cover's, except that a crystal spire's massif is painted as the
    /// site's scorched floor around the crystal.
    fn paint_channel(&self, x: i32, z: i32) -> usize {
        let cover = self.cover_at(x, z);
        match self.spire {
            Some((c, r)) if cover == Cover::Mountain && self.world_cell(x, z).distance(c) < r + 2.5 => Cover::Scar.channel(),
            _ => cover.channel(),
        }
    }

    /// The class weights of every field cell for the terrain shader: two
    /// RGBA8 images (channels 0-3 and 4-7 of [`Cover::channel`]), one texel
    /// per cell, each a 3x3 blur of the cells' one-hot channels.
    pub fn class_texels(&self) -> [Vec<u8>; 2] {
        const K: [f32; 3] = [1.0, 2.0, 1.0];
        let texels = par_rows(self.rows, |z| {
            (0..self.cols)
                .map(|x| {
                    let mut acc = [0.0f32; 8];
                    for (dz, kz) in (-1..=1).zip(K) {
                        for (dx, kx) in (-1..=1).zip(K) {
                            acc[self.paint_channel(x + dx, z + dz)] += kx * kz;
                        }
                    }
                    acc.map(|v| (v / 16.0 * 255.0).round() as u8)
                })
                .collect()
        });
        let mut out = [Vec::with_capacity(texels.len() * 4), Vec::with_capacity(texels.len() * 4)];
        for t in texels {
            out[0].extend_from_slice(&t[..4]);
            out[1].extend_from_slice(&t[4..]);
        }
        out
    }
}

/// `row(z)` for every row `0..rows`, concatenated in order: bands of rows
/// run on the compute task pool when there is one.
fn par_rows<T: Send + 'static>(rows: i32, row: impl Fn(i32) -> Vec<T> + Sync) -> Vec<T> {
    let pool = bevy::tasks::ComputeTaskPool::try_get().filter(|p| p.thread_num() > 1 && rows > 64);
    let Some(pool) = pool else { return (0..rows).flat_map(&row).collect() };
    let band = (rows / (pool.thread_num() as i32 * 4)).max(16);
    let row = &row;
    pool.scope(|s| {
        for start in (0..rows).step_by(band as usize) {
            s.spawn(async move { (start..(start + band).min(rows)).flat_map(row).collect::<Vec<T>>() });
        }
    })
    .into_iter()
    .flatten()
    .collect()
}

/// Which map edges a map cell lies beyond.
fn beyond_edges(m: IVec2, cols: i32, rows: i32) -> Vec<MapEdge> {
    let mut v = Vec::new();
    if m.x < 0 {
        v.push(MapEdge::West);
    }
    if m.x >= cols {
        v.push(MapEdge::East);
    }
    if m.y < 0 {
        v.push(MapEdge::South);
    }
    if m.y >= rows {
        v.push(MapEdge::North);
    }
    v
}

/// The cover of an off-map cell `out` metres beyond the map edge, whose
/// nearest map cell has `edge`: the sea beyond a sea edge, water and
/// mountains straight on, else meadow and woodland with a frame of
/// mountains further out.
pub fn off_map_cover(p: Vec2, out: f32, edge: Cover, sea: bool, seed: u32) -> Cover {
    if sea || edge == Cover::Ocean {
        return Cover::Ocean;
    }
    match edge {
        Cover::Ford | Cover::Shallows => return Cover::Shallows,
        Cover::Deep => return Cover::Deep,
        Cover::Mountain => return Cover::Mountain,
        _ => {}
    }
    let ridge = ridged(p * 0.021, 3, seed ^ 0x0ff1) + (value(p * 0.11, seed ^ 0x0ff2) - 0.5) * 0.12;
    if out > 6.0 && ridge - 0.42 * (1.0 - smoothstep(10.0, 46.0, out)) > 0.6 {
        return Cover::Mountain;
    }
    if out > 7.0 && fbm(p * 0.035, 3, seed ^ 0x0ff3) > 0.56 { Cover::Woodland } else { Cover::Meadow }
}

/// Height of a water bed `d` metres from the shore.
pub fn water_bed(cover: Cover, d: f32) -> f32 {
    match cover {
        Cover::Ford => -0.32,
        Cover::Shallows => -0.5 - 0.3 * smoothstep(0.5, 3.0, d),
        Cover::Deep => -1.4 - 1.1 * smoothstep(1.0, 7.0, d),
        Cover::Ocean => -1.4 - 2.6 * smoothstep(1.0, 24.0, d),
        _ => WATER_LEVEL - 0.3,
    }
}

/// Height of mountain rock `d` metres inside a range whose crest lies
/// `crest_depth` metres in: steep flanks rising all the way to a sharp
/// crest (deep inside a massif, past [`RANGE_REACH`], the summits), its
/// summits 5-10.3 m high along ridged noise (one every 12-18 m), the flanks
/// cut by gullies and roughened by crags.
pub fn mountain_height(p: Vec2, d: f32, crest_depth: f32, seed: u32) -> f32 {
    let summit = ridged(p * 0.065, 3, seed ^ 0x3a7);
    let peak = 5.2 + 5.1 * summit * summit;
    let t = (d / crest_depth.clamp(1.0, RANGE_REACH as f32)).min(1.0);
    let crest = peak * t * (1.35 - 0.35 * t);
    let flank = smoothstep(0.8, 3.0, d) * (1.0 - t * 0.5);
    let warp = Vec2::new(value(p * 0.11, seed ^ 0x3a9), value(p * 0.11 + 7.3, seed ^ 0x3a9)) * 4.0;
    let gullies = (1.0 - ridged(p * 0.3 + warp, 2, seed ^ 0x3aa)) * 2.4 * flank;
    let crags = (fbm(p * 0.5, 2, seed ^ 0x3a8) - 0.5) * 1.2 * smoothstep(0.0, 2.0, d);
    (crest - gullies + crags).clamp(0.05, MAX_TERRAIN_HEIGHT)
}

/// The height of off-map land `out` metres beyond the map, `wood` metres
/// inside a wood and `shore` metres from water: low rolling hills rising
/// away from the edge, and the woods on them, a soft canopy of rounded
/// crowns (2-3 m) rising gently from the wood's edge; both settle down
/// toward water, so rivers and lakes keep low banks.
pub fn skirt_relief(p: Vec2, out: f32, wood: f32, shore: f32, seed: u32) -> f32 {
    let hills = smoothstep(3.0, 36.0, out) * (fbm(p * 0.025, 3, seed ^ 0x51c1) * 3.5 - 0.6).max(0.0) * smoothstep(1.0, 12.0, shore);
    let canopy = smoothstep(0.0, 7.0, wood) * (1.5 + 0.8 * crowns(p, 3.4, seed ^ 0x51c2) + 0.25 * crowns(p, 1.8, seed ^ 0x51c3)) * smoothstep(0.5, 4.0, shore);
    hills + canopy
}

/// The low rocky mound a crystal spire stands on.
fn spire_base(d: f32) -> f32 {
    1.1 * (1.0 - (-d / 1.5).exp())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use af_core::ext::map::{MeteorSite, MountainRange, ResolvedMapOptions, RiverLine, MapSize, WaterLevel};
    use af_core::ext::render::{corner_ground, ground_height};
    use af_core::testing::{new_nav, test_data};

    /// A map with every terrain: a mountain range across the middle, a
    /// river with a ford, a lake, marsh, thicket, a highland, dunes, scar.
    pub(crate) fn painted(data: &GameData, n: u32) -> NavGrid {
        let t = |id: &str| data.terrain_id(id).unwrap_or_else(|| panic!("{id}")).0;
        let mut nav = new_nav(data, n, n, data.terrain_id("grass").unwrap());
        for z in 0..n as i32 {
            for x in 0..n as i32 {
                let p = Vec2::new(x as f32 + 0.5, z as f32 + 0.5);
                let id = if (p.y - 40.0).abs() < 7.0 && (20.0..70.0).contains(&p.x) {
                    Some(t("mountain"))
                } else if (p.x - 90.0).abs() < 2.5 {
                    Some(if (p.y - 30.0).abs() < 4.0 { t("ford") } else { t("deep_water") })
                } else if (p.x - 90.0).abs() < 4.5 {
                    Some(t("shallow_water"))
                } else if p.distance(Vec2::new(30.0, 90.0)) < 10.0 {
                    Some(t("deep_water"))
                } else if p.distance(Vec2::new(30.0, 90.0)) < 13.0 {
                    Some(t("shallow_water"))
                } else if p.distance(Vec2::new(30.0, 90.0)) < 16.0 {
                    Some(t("marsh"))
                } else if p.distance(Vec2::new(60.0, 15.0)) < 6.0 {
                    Some(t("thicket"))
                } else if p.distance(Vec2::new(60.0, 85.0)) < 8.0 {
                    Some(t("cliff"))
                } else if p.distance(Vec2::new(15.0, 15.0)) < 9.0 {
                    Some(t("sand"))
                } else if p.distance(Vec2::new(70.0, 110.0)) < 7.0 {
                    Some(t("scorched"))
                } else {
                    None
                };
                if let Some(id) = id {
                    nav.set_terrain(IVec2::new(x, z), id);
                }
            }
        }
        nav
    }

    fn build(data: &GameData, nav: &NavGrid, features: Option<&MapFeatures>) -> Field {
        Field::build(&FieldSource { data, nav, seed: 7, features, crater: None })
    }

    /// What af_map would publish for [`painted`] at 128 m: a sea to the
    /// west, the Starspire on the range, the river's line running off both
    /// ends of the map.
    pub(crate) fn painted_features() -> MapFeatures {
        MapFeatures {
            options: ResolvedMapOptions { size: MapSize::Medium, water: WaterLevel::Coast, rivers: 1, site: SiteKind::Spire },
            land_min: Vec2::ZERO,
            land_max: Vec2::splat(128.0),
            ocean_edges: vec![MapEdge::West],
            sites: vec![MeteorSite { kind: SiteKind::Spire, center: Vec2::new(45.0, 40.0), radius: 12.0 }],
            rivers: vec![RiverLine { points: (0..200).map(|i| Vec2::new(90.0, 130.0 - i as f32 * 0.75)).collect(), half_widths: vec![2.5; 200], fords: vec![133] }],
            mountains: vec![MountainRange { spine: vec![Vec2::new(20.0, 40.0), Vec2::new(70.0, 40.0)], half_widths: vec![7.0; 2] }],
        }
    }

    #[test]
    fn walkable_corners_are_at_the_ground_units_stand_on() {
        let data = test_data();
        let nav = painted(&data, 128);
        let features = painted_features();
        for features in [None, Some(&features)] {
            let f = build(&data, &nav, features);
            for z in 0..=128 {
                for x in 0..=128 {
                    let (fx, fz) = (x + f.skirt, z + f.skirt);
                    let touches_land = f.corner_cells(fx, fz).iter().any(|i| f.cover[*i].is_land());
                    if touches_land {
                        assert_eq!(f.height(fx, fz), corner_ground(&data, &nav, IVec2::new(x, z)), "corner {x},{z} (features: {})", features.is_some());
                    }
                }
            }
        }
    }

    #[test]
    fn a_river_runs_off_the_map_at_its_wading_ground() {
        // The river leaves the map at both ends: the cells just beyond the
        // edge are its water (with or without its line carried on across
        // the skirt), and units wading in its last row stay at the same
        // ground as further in, instead of rising toward grass.
        let data = test_data();
        let nav = painted(&data, 128);
        let features = painted_features();
        for features in [None, Some(&features)] {
            let f = build(&data, &nav, features);
            for x in 86..=93 {
                for z in [-1, 128] {
                    let (fx, fz) = (x + f.skirt, z + f.skirt);
                    assert_eq!(f.cover_at(fx, fz).is_water(), f.cover_at(fx, fz.clamp(f.skirt, f.skirt + 127)).is_water(), "first ring at {x},{z}");
                }
            }
            for (edge, inside) in [(0.05, 6.5), (127.95, 120.5)] {
                for x in [88.6, 90.5, 91.2] {
                    let at_edge = ground_height(&data, &nav, Vec2::new(x, edge));
                    let further_in = ground_height(&data, &nav, Vec2::new(x, inside));
                    assert!((at_edge - further_in).abs() < 1e-6 && at_edge < WATER_LEVEL, "wading at {x},{edge}: {at_edge} vs {further_in}");
                }
            }
        }
    }

    #[test]
    fn mountains_stay_under_flyers_and_beds_under_the_water() {
        let data = test_data();
        let nav = painted(&data, 128);
        let f = build(&data, &nav, None);
        let mut peak: f32 = 0.0;
        for z in 0..=f.rows {
            for x in 0..=f.cols {
                let h = f.height(x, z);
                assert!(h <= MAX_TERRAIN_HEIGHT, "{h} at {x},{z}");
                let cells = f.corner_cells(x, z);
                if cells.iter().all(|i| f.cover[*i].is_water()) {
                    assert!(h < WATER_LEVEL - 0.1, "bed {h} at {x},{z} is under the water");
                }
                if cells.iter().all(|i| f.cover[*i] == Cover::Mountain) && f.on_map(x, z) {
                    peak = peak.max(h);
                }
            }
        }
        assert!(peak > 6.5, "the range reaches real peaks: {peak}");
    }

    #[test]
    fn the_skirt_carries_the_map_on() {
        let data = test_data();
        let nav = painted(&data, 128);
        let f = build(&data, &nav, None);
        // The river runs off the south edge and on, and so on the north.
        let river = f.cell_of(Vec2::new(90.5, -20.0));
        assert_eq!(f.cover_at(river.x, river.y), Cover::Deep);
        assert!(f.river[f.index(river.x, river.y)] > 0.9, "a narrow body is a river");
        let lake = f.cell_of(Vec2::new(30.5, 90.5));
        assert!(f.river[f.index(lake.x, lake.y)] < 0.1, "a wide one a lake");
        // A sea edge puts the sea beyond it.
        let features = painted_features();
        let f = build(&data, &nav, Some(&features));
        let west = f.cell_of(Vec2::new(-30.0, 60.0));
        assert_eq!(f.cover_at(west.x, west.y), Cover::Ocean);
        assert!(f.height(west.x, west.y) < -1.0);
        let east = f.cell_of(Vec2::new(150.0, 60.0));
        assert!(f.cover_at(east.x, east.y).is_land() || f.cover_at(east.x, east.y) == Cover::Mountain);
        // River water flows from the source (north) to the mouth: its
        // course runs downstream, across the river from bank to bank.
        let at = |x: f32, z: f32| f.index(f.cell_of(Vec2::new(x, z)).x, f.cell_of(Vec2::new(x, z)).y);
        let (up, down) = (at(90.5, 80.5), at(90.5, 60.5));
        assert!(f.flow[down].y < -0.5 && f.flow[down].x.abs() < 0.1, "{}", f.flow[down]);
        assert!((f.course[down].x - f.course[up].x - 20.0).abs() < 0.5, "{:?} -> {:?}", f.course[up], f.course[down]);
        assert!((f.course[at(92.5, 60.5)].y - f.course[at(88.5, 60.5)].y).abs() > 3.5, "across");
        assert_eq!(f.flow[at(30.5, 90.5)], Vec2::ZERO, "the lake lies still");
        // The spire's massif is a low base, not a peak.
        let c = f.cell_of(Vec2::new(45.0, 40.0));
        assert!(f.height(c.x, c.y) < 1.5, "{}", f.height(c.x, c.y));
    }

    #[test]
    fn rivers_wind_on_across_the_skirt_and_narrow() {
        // With its line, the river leaving the south edge winds on across
        // the skirt instead of running out straight, narrower far out, and
        // still flows.
        let data = test_data();
        let nav = painted(&data, 128);
        let f = build(&data, &nav, Some(&painted_features()));
        let row = |out: f32| -> Vec<f32> {
            let z = f.cell_of(Vec2::new(0.0, -out)).y;
            (0..f.cols).filter(|x| f.cover_at(*x, z).is_water() && f.cover_at(*x, z) != Cover::Ocean).map(|x| f.world_cell(x, z).x).filter(|x| *x > 40.0).collect()
        };
        let (near, far) = (row(3.0), row(54.0));
        assert!(!far.is_empty() && far.len() < near.len(), "narrower far out: {} cells wide at 3 m, {} at 54 m", near.len(), far.len());
        let centres: Vec<f32> = [24.0, 32.0, 40.0, 48.0, 56.0].iter().map(|o| row(*o)).filter(|r| !r.is_empty()).map(|r| r.iter().sum::<f32>() / r.len() as f32).collect();
        let (lo, hi) = centres.iter().fold((f32::MAX, f32::MIN), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
        assert!(centres.len() == 5 && hi - lo > 1.0, "the course winds: centres {centres:?}");
        let out = f.cell_of(Vec2::new(centres[2], -40.0));
        assert!(f.flow[f.index(out.x, out.y)].y < -0.3, "and flows on: {}", f.flow[f.index(out.x, out.y)]);
        // Without lines, water carries straight on.
        let plain = build(&data, &nav, None);
        let z = plain.cell_of(Vec2::new(0.0, -40.0)).y;
        assert!((86..94).all(|x| plain.cover_at(x + plain.skirt, z).is_water()));
    }

    #[test]
    fn class_texels_sum_to_one() {
        let data = test_data();
        let nav = painted(&data, 64);
        let f = build(&data, &nav, None);
        let [a, b] = f.class_texels();
        for i in (0..a.len()).step_by(4 * 37) {
            let sum: u32 = a[i..i + 4].iter().chain(&b[i..i + 4]).map(|v| *v as u32).sum();
            assert!((253..=257).contains(&sum), "{sum}");
        }
    }
}
