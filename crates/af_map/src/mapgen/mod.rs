//! The pure, ECS-free map generator.
//!
//! [`generate`] turns content + a [`MatchSetup`] into a [`GeneratedMap`]:
//! terrain per 1 m cell, start positions, the Starfall Crater, every resource
//! node and the start buildings and units, all derived from the seed alone.
//!
//! Generation order (each phase draws from its own forked [`SimRng`], so
//! changing one phase does not reshuffle the others):
//!
//! 1. [`Layout`]: starts on a circle, the crater and its ramp gaps, the
//!    contested spots between neighbours, the shared start-resource template,
//!    and the reserved zones (start areas, corridors, spots, crater) that
//!    random features must not touch.
//! 2. Terrain (`terrain.rs`): crater rim, water with fords, marsh, cliffs,
//!    sand regions, start-area sand patches and clear start discs.
//! 3. Start halls, start resources (one template rotated per start, so every
//!    player gets the same distances), extra start buildings, start units.
//! 4. Contested spots, crater nodes, highland ore, sand-region nodes,
//!    forests and the thickets inside them (`nodes.rs`).
//! 5. Connectivity (`connect.rs`): every start and every highland top is
//!    reachable on foot from the crater floor, carving corridors if anything
//!    got in the way.

mod connect;
mod noise;
mod nodes;
mod starts;
mod terrain;

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use af_core::prelude::*;
use bevy::prelude::*;

use self::nodes::Occupancy;
use self::starts::{Frame, StartTemplate};
use self::terrain::Highland;

/// Radius of the clear, buildable grass disc around every start position.
pub const START_CLEAR_RADIUS: f32 = 22.0;
/// Radius around every start kept free of random terrain features and of
/// random resource clusters; the start's own resources all sit inside it.
pub const START_AREA_RADIUS: f32 = 46.0;
/// Half-width of the node- and obstacle-free corridor from every start to
/// the crater centre.
pub const CORRIDOR_HALF_WIDTH: f32 = 4.0;

/// Start positions sit on a circle of this fraction of the map size.
const START_RING: f32 = 0.36;
/// Crater rim radius as a fraction of the map size.
const CRATER_RING: f32 = 0.11;
/// Reserved radius around a contested spot between neighbours.
const SPOT_RADIUS: f32 = 12.0;
/// Reserved radius around each start's aether crystal cluster.
const AETHER_SPOT_RADIUS: f32 = 8.0;
/// Reserved margin outside the crater rim.
const CRATER_MARGIN: f32 = 2.0;
/// Half-width of each ramp gap in the crater rim (metres, at any radius).
const GAP_HALF_WIDTH: f32 = 4.5;
/// Minimum spacing between big terrain features (cliffs, lakes, sand).
const FEATURE_SPACING: f32 = 6.0;
/// Grid step of the deterministic search for room ([`Gen::roomiest`]).
const ROOM_SCAN_STEP: f32 = 4.0;
/// Nodes keep at least this far inside the map edge.
const MAP_MARGIN: f32 = 1.0;
/// Salt so the map does not share its random stream with the match `SimRng`.
const MAP_SEED_SALT: u64 = 0x4D41_505F_4745_4E31;

/// `MatchSetup::map_size` is rounded to whole metres and clamped to this range.
const MIN_MAP_SIZE: f32 = 160.0;
const MAX_MAP_SIZE: f32 = 1024.0;

/// A resource node to spawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodePlacement {
    pub kind: NodeTypeId,
    pub pos: Vec2,
    pub yaw: f32,
}

/// A complete start building to spawn for player slot `player`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildingPlacement {
    pub player: usize,
    pub kind: BuildingTypeId,
    pub pos: Vec2,
    pub yaw: f32,
}

/// A start unit to spawn for player slot `player`. Units with the same
/// `squad` group are one squadron (spawn them with a shared `SquadMember`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitPlacement {
    pub player: usize,
    pub kind: UnitTypeId,
    pub pos: Vec2,
    pub yaw: f32,
    pub squad: Option<u32>,
}

/// Everything needed to build a match world, with no ECS types.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedMap {
    /// Square side in metres; the map spans `0..size` on x and z.
    pub size: f32,
    /// Nav cells (1 m) along x and z.
    pub width: u32,
    pub height: u32,
    /// Terrain per cell, row-major (`y * width + x`).
    pub terrain: Vec<TerrainId>,
    /// Main hall position per player slot.
    pub starts: Vec<Vec2>,
    pub crater_center: Vec2,
    /// Radius of the middle of the crater's cliff rim.
    pub crater_radius: f32,
    pub nodes: Vec<NodePlacement>,
    /// Start buildings; each player's main hall comes first.
    pub buildings: Vec<BuildingPlacement>,
    pub units: Vec<UnitPlacement>,
}

impl GeneratedMap {
    /// Terrain of the cell containing `p`, if it is on the map.
    pub fn terrain_at(&self, p: Vec2) -> Option<TerrainId> {
        let (x, y) = (p.x.floor(), p.y.floor());
        if x < 0.0 || y < 0.0 || x >= self.width as f32 || y >= self.height as f32 {
            return None;
        }
        self.terrain.get(y as usize * self.width as usize + x as usize).copied()
    }

    /// The terrain-only navigation grid (1 m cells, terrain info registered).
    /// Obstacles are not included: af_movement registers them from
    /// `Obstacle` components.
    pub fn nav(&self, data: &GameData) -> NavGrid {
        terrain_nav(data, self.width, self.height, &self.terrain)
    }
}

/// Square map side used when `MatchSetup::map_size` is 0.
pub fn map_size_for(players: usize) -> f32 {
    match players {
        0..=2 => 224.0,
        3 => 256.0,
        4 => 288.0,
        5 => 320.0,
        6 => 352.0,
        _ => 384.0,
    }
}

/// Generate the map for `setup`. Deterministic in `setup.seed`. Works for
/// any number of player slots (1-8 are the supported range); slots whose
/// faction does not exist get a start area but no buildings or units.
pub fn generate(data: &GameData, setup: &MatchSetup) -> GeneratedMap {
    Gen::run(data, setup).finish()
}

/// The map side for `setup`: its `map_size` rounded and clamped, or
/// [`map_size_for`] its player count when that is 0.
fn map_size_of(setup: &MatchSetup) -> f32 {
    if setup.map_size > 0.0 {
        setup.map_size.round().clamp(MIN_MAP_SIZE, MAX_MAP_SIZE)
    } else {
        map_size_for(setup.players.len())
    }
}

/// A nav grid of `width` x `height` 1 m cells with every terrain type's
/// passability and buildability registered from `data`.
pub(crate) fn terrain_nav(data: &GameData, width: u32, height: u32, terrain: &[TerrainId]) -> NavGrid {
    let mut nav = NavGrid::new(width, height, 1.0, Vec2::ZERO);
    af_core::testing::register_terrain(&mut nav, data);
    for (i, t) in terrain.iter().enumerate() {
        if t.0 != 0 {
            let c = IVec2::new((i % width as usize) as i32, (i / width as usize) as i32);
            nav.set_terrain(c, t.0);
        }
    }
    nav
}

/// Unit vector at math angle `a` (counter-clockwise from +x in the x/z plane).
#[inline]
pub(crate) fn polar(a: f32) -> Vec2 {
    let (s, c) = ops::sin_cos(a);
    Vec2::new(c, s)
}

/// Math angle of `v` (inverse of [`polar`]).
#[inline]
pub(crate) fn angle_of(v: Vec2) -> f32 {
    ops::atan2(v.y, v.x)
}

/// `v` rotated a quarter turn counter-clockwise.
#[inline]
pub(crate) fn perp(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}

/// `obb` grown by `by` metres on every side.
#[inline]
pub(crate) fn grown(obb: Obb, by: f32) -> Obb {
    Obb { half: obb.half + Vec2::splat(by), ..obb }
}

/// Centre of nav cell `(x, y)` (1 m cells from the origin).
#[inline]
pub(crate) fn cell_center(x: i32, y: i32) -> Vec2 {
    Vec2::new(x as f32 + 0.5, y as f32 + 0.5)
}

/// The fixed framework ids the generator places (see the design doc).
/// Missing terrain falls back to grass; a missing node type is not placed.
pub(crate) struct Ids {
    pub grass: TerrainId,
    pub sand: TerrainId,
    pub shallow: TerrainId,
    pub deep: TerrainId,
    pub marsh: TerrainId,
    pub thicket: TerrainId,
    pub cliff: TerrainId,
    pub oak: Option<NodeTypeId>,
    pub pine: Option<NodeTypeId>,
    pub palm: Option<NodeTypeId>,
    pub berry: Option<NodeTypeId>,
    pub stone: Option<NodeTypeId>,
    pub copper: Option<NodeTypeId>,
    pub iron: Option<NodeTypeId>,
    pub silicon: Option<NodeTypeId>,
    pub crystal: Option<NodeTypeId>,
    pub vent: Option<NodeTypeId>,
    pub meteor: Option<NodeTypeId>,
}

impl Ids {
    fn new(data: &GameData) -> Ids {
        let grass = data.terrain_id("grass").unwrap_or_default();
        let terrain = |id: &str| data.terrain_id(id).unwrap_or(grass);
        Ids {
            grass,
            sand: terrain("sand"),
            shallow: terrain("shallow_water"),
            deep: terrain("deep_water"),
            marsh: terrain("marsh"),
            thicket: terrain("thicket"),
            cliff: terrain("cliff"),
            oak: data.node_id("oak"),
            pine: data.node_id("pine"),
            palm: data.node_id("palm"),
            berry: data.node_id("berry_bush"),
            stone: data.node_id("stone_outcrop"),
            copper: data.node_id("copper_vein"),
            iron: data.node_id("iron_vein"),
            silicon: data.node_id("silicon_deposit"),
            crystal: data.node_id("aether_crystal"),
            vent: data.node_id("aether_vent"),
            meteor: data.node_id("adamant_meteor"),
        }
    }
}

/// A disc-shaped feature (lake, cliff, sand region, forest cluster).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Blob {
    pub center: Vec2,
    pub radius: f32,
}

/// The large-scale plan every other phase works around.
pub(crate) struct Layout {
    pub size: f32,
    pub center: Vec2,
    pub starts: Vec<Vec2>,
    pub crater_radius: f32,
    /// Half thickness of the crater's cliff rim.
    pub rim_half: f32,
    /// Math angles of the ramp gaps in the rim (one per start + extras).
    pub gaps: Vec<f32>,
    /// Math angles halfway between neighbouring starts.
    pub bisectors: Vec<f32>,
    /// Distance of the contested spots from the centre.
    pub spot_ring: f32,
    /// Contested spot per bisector.
    pub spots: Vec<Vec2>,
    pub template: StartTemplate,
    /// World centre of each start's aether crystal cluster.
    pub start_aether: Vec<Vec2>,
}

impl Layout {
    fn new(data: &GameData, ids: &Ids, players: usize, size: f32, rng: &mut SimRng) -> Layout {
        let center = Vec2::splat(size * 0.5);
        let rotation = rng.range(0.0, TAU);
        let start_ring = size * START_RING;
        let start_angles: Vec<f32> = (0..players).map(|i| rotation + i as f32 * TAU / players as f32).collect();
        let starts = start_angles.iter().map(|a| center + polar(*a) * start_ring).collect();
        let bisectors = match players {
            0 => vec![rotation],
            1 => vec![rotation + PI],
            2 => vec![rotation + FRAC_PI_2, rotation - FRAC_PI_2],
            n => (0..n).map(|i| rotation + (i as f32 + 0.5) * TAU / n as f32).collect(),
        };
        let crater_radius = size * CRATER_RING;
        let rim_half = rng.range(2.0, 3.0);
        let spot_ring = (crater_radius + start_ring) * 0.5;
        let spots = bisectors.iter().map(|b| center + polar(*b) * spot_ring).collect();

        let mut gaps = start_angles;
        let extra = 1 + rng.below(2);
        for _ in 0..extra {
            for _ in 0..16 {
                let a = rng.range(0.0, TAU);
                if gaps.iter().all(|g| af_geom::wrap_angle(a - g).abs() > 0.6) {
                    gaps.push(a);
                    break;
                }
            }
        }

        let template = StartTemplate::new(data, ids, &mut rng.fork(), start_ring, crater_radius + rim_half);
        let mut layout = Layout {
            size,
            center,
            starts,
            crater_radius,
            rim_half,
            gaps,
            bisectors,
            spot_ring,
            spots,
            template,
            start_aether: Vec::new(),
        };
        layout.start_aether = (0..players).map(|i| layout.frame(i).point(layout.template.aether)).collect();
        layout
    }

    /// Start `i`'s local frame: x toward the map centre, y to its left.
    pub fn frame(&self, i: usize) -> Frame {
        Frame::new(self.starts[i], self.center)
    }

    /// Outer edge of the crater's cliff rim.
    pub fn crater_outer(&self) -> f32 {
        self.crater_radius + self.rim_half
    }

    /// The corridor segments, start -> crater centre.
    pub fn corridors(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        self.starts.iter().map(|s| (*s, self.center))
    }
}

/// Generator state: the layout plus everything placed so far.
pub(crate) struct Gen<'a> {
    pub data: &'a GameData,
    pub ids: Ids,
    pub layout: Layout,
    pub size: f32,
    /// Cells per side.
    pub w: i32,
    pub terrain: Vec<TerrainId>,
    /// Cells random features must leave alone.
    pub reserved: Vec<bool>,
    /// Buildability by terrain id.
    pub buildable: [bool; 256],
    /// Every box placed so far (grown by its padding), for overlap tests.
    pub occ: Occupancy,
    pub nodes: Vec<NodePlacement>,
    pub buildings: Vec<BuildingPlacement>,
    pub units: Vec<UnitPlacement>,
    /// Main hall footprint per player slot (None: no hall).
    pub halls: Vec<Option<Obb>>,
    /// Lakes, cliffs, sand regions and river stretches, for spacing.
    pub features: Vec<Blob>,
    pub sand_regions: Vec<Blob>,
    /// Cliff highlands (ore goes on top).
    pub highlands: Vec<Highland>,
    /// Next squadron group id.
    pub next_squad: u32,
}

impl<'a> Gen<'a> {
    /// Every generation phase in order (see the module docs).
    fn run(data: &'a GameData, setup: &MatchSetup) -> Gen<'a> {
        let mut rng = SimRng::new(setup.seed ^ MAP_SEED_SALT);
        let ids = Ids::new(data);
        let layout = Layout::new(data, &ids, setup.players.len(), map_size_of(setup), &mut rng.fork());
        let mut g = Gen::new(data, ids, layout);
        g.paint_terrain(&mut rng.fork());
        g.place_halls(setup);
        g.place_start_resources();
        g.place_start_buildings(setup);
        g.place_start_units(setup);
        g.place_contested_spots(&mut rng.fork());
        g.place_crater_nodes(&mut rng.fork());
        g.place_highland_ore(&mut rng.fork());
        g.place_sand_nodes(&mut rng.fork());
        g.place_forests(&mut rng.fork());
        g.ensure_connectivity();
        g
    }

    fn new(data: &'a GameData, ids: Ids, layout: Layout) -> Gen<'a> {
        let w = layout.size as i32;
        let cells = (w * w) as usize;
        let mut buildable = [false; 256];
        for t in data.terrain_ids() {
            buildable[t.index()] = data.terrain_def(t).buildable;
        }
        let mut g = Gen {
            data,
            size: layout.size,
            occ: Occupancy::new(layout.size),
            terrain: vec![ids.grass; cells],
            reserved: vec![false; cells],
            ids,
            layout,
            w,
            buildable,
            nodes: Vec::new(),
            buildings: Vec::new(),
            units: Vec::new(),
            halls: Vec::new(),
            features: Vec::new(),
            sand_regions: Vec::new(),
            highlands: Vec::new(),
            next_squad: 0,
        };
        g.reserve_layout();
        g
    }

    fn reserve_layout(&mut self) {
        let lay = &self.layout;
        let mut discs: Vec<(Vec2, f32)> = lay.starts.iter().map(|s| (*s, START_AREA_RADIUS)).collect();
        discs.extend(lay.spots.iter().map(|s| (*s, SPOT_RADIUS)));
        discs.extend(lay.start_aether.iter().map(|a| (*a, AETHER_SPOT_RADIUS)));
        discs.push((lay.center, lay.crater_outer() + CRATER_MARGIN));
        let corridors: Vec<(Vec2, Vec2)> = lay.corridors().collect();
        for (c, r) in discs {
            self.reserve_where(c, r, |p| p.distance_squared(c) <= r * r);
        }
        let r = CORRIDOR_HALF_WIDTH + 1.0;
        for (a, b) in corridors {
            self.reserve_where((a + b) * 0.5, a.distance(b) * 0.5 + r, |p| af_geom::point_segment_distance(p, a, b) <= r);
        }
    }

    /// Reserve the cells within `reach` of `c` whose centres pass `inside`.
    fn reserve_where(&mut self, c: Vec2, reach: f32, inside: impl Fn(Vec2) -> bool) {
        let (x0, y0, x1, y1) = self.cell_rect(c, reach);
        for y in y0..=y1 {
            for x in x0..=x1 {
                if inside(cell_center(x, y)) {
                    let i = self.idx(x, y);
                    self.reserved[i] = true;
                }
            }
        }
    }

    #[inline]
    pub fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    /// Index of the cell containing `p`, if on the map.
    #[inline]
    pub fn idx_at(&self, p: Vec2) -> Option<usize> {
        let (x, y) = (p.x.floor() as i32, p.y.floor() as i32);
        (x >= 0 && y >= 0 && x < self.w && y < self.w).then(|| self.idx(x, y))
    }

    /// Inclusive cell bounds of the square of half side `r` around `c`,
    /// clipped to the map (empty when entirely off the map).
    pub fn cell_rect(&self, c: Vec2, r: f32) -> (i32, i32, i32, i32) {
        let x0 = ((c.x - r).floor() as i32).max(0);
        let y0 = ((c.y - r).floor() as i32).max(0);
        let x1 = ((c.x + r).floor() as i32).min(self.w - 1);
        let y1 = ((c.y + r).floor() as i32).min(self.w - 1);
        (x0, y0, x1, y1)
    }

    /// Visit the cells a footprint covers, with the same rule as
    /// `NavGrid::cells_covered`: cells whose centres lie inside the box,
    /// plus the cell under the box centre.
    pub fn for_covered(&self, obb: &Obb, mut f: impl FnMut(usize)) {
        let (min, max) = obb.aabb();
        let x0 = (min.x.floor() as i32).max(0);
        let y0 = (min.y.floor() as i32).max(0);
        let x1 = (max.x.floor() as i32).min(self.w - 1);
        let y1 = (max.y.floor() as i32).min(self.w - 1);
        let center = self.idx_at(obb.center);
        let mut center_seen = false;
        for y in y0..=y1 {
            for x in x0..=x1 {
                if obb.contains(cell_center(x, y)) {
                    let i = self.idx(x, y);
                    center_seen |= Some(i) == center;
                    f(i);
                }
            }
        }
        if let (Some(i), false) = (center, center_seen) {
            f(i);
        }
    }

    /// Does every cell under `obb` pass `pred`?
    pub fn all_covered(&self, obb: &Obb, pred: impl Fn(usize) -> bool) -> bool {
        let mut ok = true;
        self.for_covered(obb, |i| ok &= pred(i));
        ok
    }

    /// Is `obb` inside the map, `margin` metres from every edge?
    pub fn inside(&self, obb: &Obb, margin: f32) -> bool {
        let (min, max) = obb.aabb();
        min.x >= margin && min.y >= margin && max.x <= self.size - margin && max.y <= self.size - margin
    }

    /// Does `obb` intrude into any start's corridor to the crater?
    pub fn hits_corridor(&self, obb: &Obb) -> bool {
        self.layout.corridors().any(|(a, b)| obb.segment_distance(a, b) < CORRIDOR_HALF_WIDTH)
    }

    /// How far `p` is from everything big that random features must avoid
    /// (negative: inside something).
    pub fn room(&self, p: Vec2) -> f32 {
        let lay = &self.layout;
        let mut room = p.x.min(p.y).min(self.size - p.x).min(self.size - p.y) - 2.0;
        room = room.min(p.distance(lay.center) - lay.crater_outer() - CRATER_MARGIN);
        for s in &lay.starts {
            room = room.min(p.distance(*s) - START_AREA_RADIUS);
            room = room.min(af_geom::point_segment_distance(p, *s, lay.center) - CORRIDOR_HALF_WIDTH - 1.0);
        }
        for s in &lay.spots {
            room = room.min(p.distance(*s) - SPOT_RADIUS);
        }
        for a in &lay.start_aether {
            room = room.min(p.distance(*a) - AETHER_SPOT_RADIUS);
        }
        for f in &self.features {
            room = room.min(p.distance(f.center) - f.radius - FEATURE_SPACING);
        }
        room
    }

    /// Where a disc feature of `radius` goes, between `r_min` and `r_max`
    /// from the centre: a random spot with room for all of it if one turns
    /// up, else the roomiest spot ([`Gen::roomiest`]) with the radius shrunk
    /// to fit. Returns the centre and the radius to use; None when not even
    /// `min_radius` fits anywhere.
    pub fn feature_spot(&self, rng: &mut SimRng, radius: f32, min_radius: f32, r_min: f32, r_max: f32) -> Option<(Vec2, f32)> {
        let random = (0..48).find_map(|_| {
            let p = self.layout.center + polar(rng.range(0.0, TAU)) * rng.range(r_min, r_max);
            (self.room(p) >= radius).then_some((p, radius))
        });
        random.or_else(|| {
            let (p, room) = self.roomiest(r_min, r_max)?;
            (room >= min_radius).then_some((p, room.min(radius)))
        })
    }

    /// The point with the most [`Gen::room`] between `r_min` and `r_max`
    /// from the centre, on a [`ROOM_SCAN_STEP`] grid, and its room.
    pub fn roomiest(&self, r_min: f32, r_max: f32) -> Option<(Vec2, f32)> {
        let steps = (self.size / ROOM_SCAN_STEP) as i32;
        let mut best: Option<(Vec2, f32)> = None;
        for y in 0..=steps {
            for x in 0..=steps {
                let p = Vec2::new(x as f32, y as f32) * ROOM_SCAN_STEP;
                if !(r_min..=r_max).contains(&p.distance(self.layout.center)) {
                    continue;
                }
                let room = self.room(p);
                if best.is_none_or(|(_, b)| room > b) {
                    best = Some((p, room));
                }
            }
        }
        best
    }

    /// A generator for `players` starts on a `size` m map with the layout
    /// planned and nothing painted or placed yet.
    #[cfg(test)]
    pub(crate) fn bare(data: &'a GameData, players: usize, size: f32, seed: u64) -> Gen<'a> {
        let ids = Ids::new(data);
        let layout = Layout::new(data, &ids, players, size, &mut SimRng::new(seed));
        Gen::new(data, ids, layout)
    }

    fn finish(self) -> GeneratedMap {
        GeneratedMap {
            size: self.size,
            width: self.w as u32,
            height: self.w as u32,
            terrain: self.terrain,
            starts: self.layout.starts,
            crater_center: self.layout.center,
            crater_radius: self.layout.crater_radius,
            nodes: self.nodes,
            buildings: self.buildings,
            units: self.units,
        }
    }
}
