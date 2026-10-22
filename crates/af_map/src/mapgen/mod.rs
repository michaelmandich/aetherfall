//! The pure, ECS-free map generator.
//!
//! [`generate_with`] turns content, a [`MatchSetup`] and the [`MapOptions`]
//! into a [`GeneratedMap`]: terrain per 1 m cell, start positions, the meteor
//! sites, every resource node and the start buildings and units, plus the
//! [`MapFeatures`] (resolved options, coasts, sites, rivers, mountains), all
//! derived from the seed alone (`docs/design/maps-and-terrain.md`).
//!
//! Generation order (each phase draws from its own forked [`SimRng`], so
//! changing what one phase draws does not reshuffle the others):
//!
//! 1. The options are resolved from the seed (`layout.rs`), the land is
//!    sized and its ocean edges picked ([`Land`]), and the [`Layout`] is
//!    planned on the land square: starts on a circle, the meteor sites and
//!    their rim gaps (`sites.rs`), the contested spots between neighbours,
//!    the shared start-resource template, and the reserved zones (start
//!    areas, corridors, spots, sites) that random features must not touch.
//! 2. Terrain (`terrain.rs`): coasts (`water.rs`), site rims and floors,
//!    rivers with fords (`rivers.rs`), dunes, mountains (`mountains.rs`),
//!    highlands and ridges, lakes and marsh (`water.rs`), the main
//!    site's surroundings, start-area sand patches and clear start discs.
//! 3. Start halls, start resources (one template rotated per start, so every
//!    player gets the same distances), extra start buildings, start units.
//! 4. Contested spots, site nodes, highland ore, dune nodes, forests and the
//!    thickets inside them (`nodes.rs`, `sites.rs`).
//! 5. Connectivity (`connect.rs`): every start, spot and site is reachable
//!    from the main site for every ground class at its widest unit, and
//!    every highland top on foot, carving a route as a last resort.

mod connect;
mod field;
mod layout;
mod mountains;
mod noise;
mod nodes;
mod rivers;
mod sites;
mod starts;
mod terrain;
mod water;

use std::cell::RefCell;

use af_core::ext::map::{MapFeatures, MapOptions, MapSize, MountainRange, ResolvedMapOptions, RiverLine};
use af_core::prelude::*;
use bevy::prelude::*;

pub use self::layout::resolve_map_options;
use self::layout::{Land, Layout};
use self::nodes::Occupancy;
use self::terrain::Highland;

/// Radius of the clear, buildable grass disc around every start position.
pub const START_CLEAR_RADIUS: f32 = 22.0;
/// Radius around every start kept free of random terrain features and of
/// random resource clusters; the start's own resources all sit inside it.
pub const START_AREA_RADIUS: f32 = 46.0;
/// Half-width of the node- and obstacle-free corridor from every start to
/// the main site's centre.
pub const CORRIDOR_HALF_WIDTH: f32 = 5.0;

/// Reserved radius around each start's aether crystal cluster.
pub(crate) const AETHER_SPOT_RADIUS: f32 = 8.0;
/// The open apron kept outside every rim gap reaches this far out.
const GAP_APRON: f32 = 8.0;
/// Minimum spacing between big terrain features (mountains, cliffs, lakes,
/// sand, water).
const FEATURE_SPACING: f32 = 6.0;
/// Grid step of the deterministic search for room ([`Gen::roomiest`]).
const ROOM_SCAN_STEP: f32 = 4.0;
/// Cell size of the room field ([`RoomField`]).
const ROOM_CELL: f32 = 2.0;
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
    /// Centre and rim radius of the main meteor site (`features.sites[0]`).
    pub crater_center: Vec2,
    pub crater_radius: f32,
    pub nodes: Vec<NodePlacement>,
    /// Start buildings; each player's main hall comes first.
    pub buildings: Vec<BuildingPlacement>,
    pub units: Vec<UnitPlacement>,
    /// The resolved options and the large features (`MapFeatures`).
    pub features: MapFeatures,
    /// Routes the connectivity pass had to carve open, as "class: place"
    /// (rare: crossings are planned so that carving is not needed).
    pub carved: Vec<String>,
    /// Places still cut off for some ground class after carving, as
    /// "class: place". Empty on every good map.
    pub cut_off: Vec<String>,
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

/// Land side used for `players` when `MatchSetup::map_size` is 0 and the
/// size option is the default (`Medium`).
pub fn map_size_for(players: usize) -> f32 {
    MapSize::Medium.land_side(players)
}

/// Generate the map for `setup` with the default [`MapOptions`].
pub fn generate(data: &GameData, setup: &MatchSetup) -> GeneratedMap {
    generate_with(data, setup, &MapOptions::default())
}

/// Generate the map for `setup` and `options`. Deterministic in
/// `setup.seed` and the options. Works for any number of player slots (1-8
/// are the supported range); slots whose faction does not exist get a start
/// area but no buildings or units.
pub fn generate_with(data: &GameData, setup: &MatchSetup, options: &MapOptions) -> GeneratedMap {
    Gen::run(data, setup, options).finish()
}

/// The land side for `setup`: its `map_size` rounded and clamped, or the
/// size option's side for its player count when that is 0.
fn land_side_of(setup: &MatchSetup, size: MapSize) -> f32 {
    if setup.map_size > 0.0 {
        setup.map_size.round().clamp(MIN_MAP_SIZE, MAX_MAP_SIZE)
    } else {
        size.land_side(setup.players.len())
    }
}

/// The size whose land side for `players` is nearest `side` (by ratio):
/// what a `MatchSetup::map_size` override plays and is published as, so
/// feature counts and the HUD follow the land actually generated.
pub fn size_for_side(side: f32, players: usize) -> MapSize {
    let off = |s: MapSize| {
        let r = side / s.land_side(players);
        r.max(1.0 / r)
    };
    MapSize::ALL.into_iter().min_by(|a, b| off(*a).total_cmp(&off(*b))).expect("four sizes")
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
/// Missing terrain falls back to the nearest older one (grass, cliff for
/// mountains, shallows for fords, deep water for ocean); a missing node
/// type is not placed.
pub(crate) struct Ids {
    pub grass: TerrainId,
    pub sand: TerrainId,
    pub shallow: TerrainId,
    pub deep: TerrainId,
    pub marsh: TerrainId,
    pub thicket: TerrainId,
    pub cliff: TerrainId,
    pub mountain: TerrainId,
    pub ford: TerrainId,
    pub scorched: TerrainId,
    pub ocean: TerrainId,
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
        let terrain = |id: &str, or: TerrainId| data.terrain_id(id).unwrap_or(or);
        let (shallow, deep, cliff) = (terrain("shallow_water", grass), terrain("deep_water", grass), terrain("cliff", grass));
        Ids {
            grass,
            sand: terrain("sand", grass),
            shallow,
            deep,
            marsh: terrain("marsh", grass),
            thicket: terrain("thicket", grass),
            cliff,
            mountain: terrain("mountain", cliff),
            ford: terrain("ford", shallow),
            scorched: terrain("scorched", grass),
            ocean: terrain("ocean", deep),
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

/// A disc-shaped feature (cliff, sand region, forest cluster).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Blob {
    pub center: Vec2,
    pub radius: f32,
}

/// How firmly a cell is reserved. Random features (lakes, mountains,
/// cliffs, dunes, marsh, forests, thickets) keep off every reserved cell;
/// rivers, which must get across the map, distinguish the kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Hold {
    Free,
    /// A start area beyond its clear disc: rivers stay out unless no other
    /// course exists (crowded maps).
    Area,
    /// A start's corridor to the main site: rivers cross it only as a ford.
    Corridor,
    /// Never water: clear start discs, start resources, spots, sites, fords,
    /// passes, highland tops and ramps, ridge openings.
    Hard,
}

/// Distance from every 2 m cell to the nearest reserved cell, the nearest
/// painted (non-grass) cell, the nearest water, rock or sand and the
/// nearest mountain, so [`Gen::room`] costs O(1).
pub(crate) struct RoomField {
    w: usize,
    reserved: Vec<f32>,
    painted: Vec<f32>,
    blocking: Vec<f32>,
    mountain: Vec<f32>,
}

impl RoomField {
    fn new(g: &Gen) -> RoomField {
        let w = (g.w as f32 / ROOM_CELL).ceil() as usize;
        let mut reserved = vec![false; w * w];
        let mut painted = vec![false; w * w];
        let mut blocking = vec![false; w * w];
        let mut mountain = vec![false; w * w];
        let ids = &g.ids;
        let per = ROOM_CELL as usize;
        for y in 0..g.w as usize {
            for x in 0..g.w as usize {
                let i = y * g.w as usize + x;
                let c = (y / per) * w + x / per;
                let t = g.terrain[i];
                reserved[c] |= g.hold[i] != Hold::Free;
                painted[c] |= t != ids.grass;
                blocking[c] |= [ids.deep, ids.shallow, ids.ford, ids.ocean, ids.mountain, ids.cliff, ids.sand].contains(&t);
                mountain[c] |= t == ids.mountain;
            }
        }
        // Distances between 2 m cell centres; a blocked 2 m cell may reach
        // a metre closer than its centre.
        let metres = |mask: &[bool]| field::distance_field(mask, w, w).into_iter().map(|d| d * ROOM_CELL - 1.0).collect();
        RoomField { w, reserved: metres(&reserved), painted: metres(&painted), blocking: metres(&blocking), mountain: metres(&mountain) }
    }

    /// (distance to reserved ground, to painted ground, to water, rock or
    /// sand, to mountains) at `p`.
    fn at(&self, p: Vec2) -> (f32, f32, f32, f32) {
        let cx = ((p.x / ROOM_CELL).floor() as i32).clamp(0, self.w as i32 - 1) as usize;
        let cy = ((p.y / ROOM_CELL).floor() as i32).clamp(0, self.w as i32 - 1) as usize;
        let i = cy * self.w + cx;
        (self.reserved[i], self.painted[i], self.blocking[i], self.mountain[i])
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
    /// How firmly each cell is reserved.
    pub hold: Vec<Hold>,
    /// Cells painted by a coast (ocean, its shallows and beaches), which
    /// marsh never lines.
    pub coast: Vec<bool>,
    /// Buildability by terrain id.
    pub buildable: [bool; 256],
    /// Every box placed so far (grown by its padding), for overlap tests.
    pub occ: Occupancy,
    pub nodes: Vec<NodePlacement>,
    /// Nodes before this index are layout nodes (start templates, spots,
    /// sites, highland ore), which carving never removes.
    pub layout_nodes: usize,
    pub buildings: Vec<BuildingPlacement>,
    pub units: Vec<UnitPlacement>,
    /// Main hall footprint per player slot (None: no hall).
    pub halls: Vec<Option<Obb>>,
    /// Extra spacing discs for [`Gen::room`] (features not painted yet).
    pub features: Vec<Blob>,
    pub sand_regions: Vec<Blob>,
    /// Cliff highlands (ore goes on top).
    pub highlands: Vec<Highland>,
    /// Rivers as painted, source first (a tributary ends on river 0).
    pub rivers: Vec<RiverLine>,
    pub mountains: Vec<MountainRange>,
    /// Next squadron group id.
    pub next_squad: u32,
    /// Room field, rebuilt on demand after the terrain or holds change.
    room_field: RefCell<Option<RoomField>>,
    pub carved: Vec<String>,
    pub cut_off: Vec<String>,
}

impl<'a> Gen<'a> {
    /// Every generation phase in order (see the module docs).
    fn run(data: &'a GameData, setup: &MatchSetup, options: &MapOptions) -> Gen<'a> {
        let mut options = resolve_map_options(options, setup.seed);
        let side = land_side_of(setup, options.size);
        if setup.map_size > 0.0 {
            // The override sizes the land; the size nearest it decides the
            // feature counts and is what the map is published as.
            options.size = size_for_side(side, setup.players.len());
        }
        let mut rng = SimRng::new(setup.seed ^ MAP_SEED_SALT);
        let ids = Ids::new(data);
        let land = Land::plan(side, options.water, &mut rng.fork());
        let layout = Layout::new(data, &ids, setup.players.len(), land, options, &mut rng.fork());
        let mut g = Gen::new(data, ids, layout);
        g.paint_terrain(&mut rng.fork());
        g.place_halls(setup);
        g.place_start_resources();
        g.place_start_buildings(setup);
        g.place_start_units(setup);
        g.place_contested_spots(&mut rng.fork());
        g.place_site_nodes(&mut rng.fork());
        g.place_highland_ore(&mut rng.fork());
        g.layout_nodes = g.nodes.len();
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
            hold: vec![Hold::Free; cells],
            coast: vec![false; cells],
            ids,
            layout,
            w,
            buildable,
            nodes: Vec::new(),
            layout_nodes: 0,
            buildings: Vec::new(),
            units: Vec::new(),
            halls: Vec::new(),
            features: Vec::new(),
            sand_regions: Vec::new(),
            highlands: Vec::new(),
            rivers: Vec::new(),
            mountains: Vec::new(),
            next_squad: 0,
            room_field: RefCell::new(None),
            carved: Vec::new(),
            cut_off: Vec::new(),
        };
        g.reserve_layout();
        g
    }

    fn reserve_layout(&mut self) {
        let lay = &self.layout;
        let mut discs: Vec<(Vec2, f32, Hold)> = Vec::new();
        for (i, s) in lay.starts.iter().enumerate() {
            discs.push((*s, START_AREA_RADIUS, Hold::Area));
            discs.push((*s, START_CLEAR_RADIUS + 2.0, Hold::Hard));
            // Every start resource's spot, so even a river squeezed through
            // a crowded start area never takes one.
            let frame = lay.frame(i);
            discs.extend(lay.template.footprints(self.data).map(|(local, r)| (frame.point(local), r + 1.5, Hold::Hard)));
        }
        discs.extend(lay.spots.iter().zip(&lay.spot_radius).map(|(s, r)| (*s, *r, Hold::Hard)));
        discs.extend(lay.start_aether.iter().map(|a| (*a, AETHER_SPOT_RADIUS, Hold::Hard)));
        let sites = lay.sites.clone();
        let corridors: Vec<(Vec2, Vec2)> = lay.corridors().collect();
        for (c, r, hold) in discs {
            self.reserve(c, r, hold, |p| p.distance_squared(c) <= r * r);
        }
        for site in sites {
            let margin = site.reserve_margin();
            self.reserve(site.center, site.outer() + margin + 4.0, Hold::Hard, |p| site.signed(p) <= margin);
            // An apron outside every rim gap, so no tree, thicket or marsh
            // closes the way in.
            for g in &site.gaps {
                let dir = polar(*g);
                let a = site.center + dir * site.reach_toward(*g);
                let b = a + dir * GAP_APRON;
                let r = sites::GAP_HALF_WIDTH;
                self.reserve((a + b) * 0.5, GAP_APRON * 0.5 + r, Hold::Hard, |p| af_geom::point_segment_distance(p, a, b) <= r);
            }
        }
        let r = CORRIDOR_HALF_WIDTH + 1.0;
        for (a, b) in corridors {
            self.reserve((a + b) * 0.5, a.distance(b) * 0.5 + r, Hold::Corridor, |p| af_geom::point_segment_distance(p, a, b) <= r);
            // A river may ford a corridor out in the open, never in the
            // start's own front yard.
            let yard = a + (b - a).normalize_or_zero() * START_AREA_RADIUS.min(a.distance(b));
            self.reserve((a + yard) * 0.5, START_AREA_RADIUS * 0.5 + r, Hold::Hard, |p| af_geom::point_segment_distance(p, a, yard) <= r);
        }
    }

    /// Reserve (at least as firmly as `hold`) the cells within `reach` of
    /// `c` whose centres pass `inside`.
    pub(crate) fn reserve(&mut self, c: Vec2, reach: f32, hold: Hold, inside: impl Fn(Vec2) -> bool) {
        let (x0, y0, x1, y1) = self.cell_rect(c, reach);
        for y in y0..=y1 {
            for x in x0..=x1 {
                if inside(cell_center(x, y)) {
                    let i = self.idx(x, y);
                    self.hold[i] = self.hold[i].max(hold);
                }
            }
        }
        self.touch();
    }

    /// Note that the terrain or the holds changed (the room field is stale).
    #[inline]
    pub(crate) fn touch(&mut self) {
        *self.room_field.get_mut() = None;
    }

    /// Is cell `i` reserved at all?
    #[inline]
    pub fn reserved(&self, i: usize) -> bool {
        self.hold[i] != Hold::Free
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

    /// Does `obb` intrude into any start's corridor to the main site?
    pub fn hits_corridor(&self, obb: &Obb) -> bool {
        self.layout.corridors().any(|(a, b)| obb.segment_distance(a, b) < CORRIDOR_HALF_WIDTH)
    }

    /// How far `p` is from everything big that random features must avoid
    /// (negative: inside something): the map edge, reserved ground,
    /// (by [`FEATURE_SPACING`]) every painted feature, water and coast, and
    /// (by [`mountains::WALK_GAP`], room for the widest ground unit) every
    /// mountain.
    pub fn room(&self, p: Vec2) -> f32 {
        let (edge, reserved, painted, _) = self.room_parts(p);
        let mut room = (edge - 2.0).min(reserved).min(painted - FEATURE_SPACING).min(self.mountain_distance(p) - mountains::WALK_GAP);
        for f in &self.features {
            room = room.min(p.distance(f.center) - f.radius - FEATURE_SPACING);
        }
        room
    }

    /// The parts of [`Gen::room`]: distance from `p` to the map edge, to
    /// the nearest reserved cell, to the nearest painted (non-grass) cell
    /// and to the nearest water, rock or sand, each to within a metre.
    pub fn room_parts(&self, p: Vec2) -> (f32, f32, f32, f32) {
        let edge = p.x.min(p.y).min(self.size - p.x).min(self.size - p.y);
        let mut cache = self.room_field.borrow_mut();
        let (reserved, painted, blocking, _) = cache.get_or_insert_with(|| RoomField::new(self)).at(p);
        (edge, reserved, painted, blocking)
    }

    /// Distance (metres between cell centres) from every cell to the
    /// nearest mountain cell.
    pub(crate) fn mountain_field(&self) -> Vec<f32> {
        let mask: Vec<bool> = self.terrain.iter().map(|t| *t == self.ids.mountain).collect();
        field::distance_field(&mask, self.w as usize, self.w as usize)
    }

    /// Distance from `p` to the nearest mountain cell, to within a metre.
    pub(crate) fn mountain_distance(&self, p: Vec2) -> f32 {
        let mut cache = self.room_field.borrow_mut();
        cache.get_or_insert_with(|| RoomField::new(self)).at(p).3
    }

    /// Where a disc feature of `radius` goes, between `r_min` and `r_max`
    /// from the centre: a random spot with room for all of it if one turns
    /// up, else the roomiest spot ([`Gen::roomiest`]) with the radius shrunk
    /// to fit. Returns the centre and the radius to use; None when not even
    /// `min_radius` fits anywhere.
    pub fn feature_spot(&self, rng: &mut SimRng, radius: f32, min_radius: f32, r_min: f32, r_max: f32) -> Option<(Vec2, f32)> {
        let random = (0..48).find_map(|_| {
            let p = self.layout.center + polar(rng.range(0.0, std::f32::consts::TAU)) * rng.range(r_min, r_max);
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

    /// A generator for `players` starts on a `size` m map (classic crater,
    /// one river, lakes) with the layout planned and nothing painted or
    /// placed yet.
    #[cfg(test)]
    pub(crate) fn bare(data: &'a GameData, players: usize, size: f32, seed: u64) -> Gen<'a> {
        use af_core::ext::map::{SiteKind, WaterLevel};
        let options = af_core::ext::map::ResolvedMapOptions { size: MapSize::Medium, water: WaterLevel::Lakes, rivers: 1, site: SiteKind::Crater };
        Gen::bare_with(data, players, size, seed, options)
    }

    /// [`Gen::bare`] with any options.
    #[cfg(test)]
    pub(crate) fn bare_with(data: &'a GameData, players: usize, size: f32, seed: u64, options: af_core::ext::map::ResolvedMapOptions) -> Gen<'a> {
        let ids = Ids::new(data);
        let mut rng = SimRng::new(seed);
        let land = Land::plan(size, options.water, &mut rng.fork());
        let layout = Layout::new(data, &ids, players, land, options, &mut rng);
        Gen::new(data, ids, layout)
    }

    /// What af_map publishes about the map besides its grid: the options as
    /// played (the size a `map_size` override plays as, and the rivers
    /// actually laid, should one find no course).
    fn features(&self) -> MapFeatures {
        let land = &self.layout.land;
        MapFeatures {
            options: ResolvedMapOptions { rivers: self.rivers.len() as u8, ..self.layout.options },
            land_min: land.min,
            land_max: land.max,
            ocean_edges: land.ocean.clone(),
            sites: self.layout.sites.iter().flat_map(|s| s.published()).collect(),
            rivers: self.rivers.clone(),
            mountains: self.mountains.clone(),
        }
    }

    fn finish(self) -> GeneratedMap {
        let features = self.features();
        let (crater_center, crater_radius) = (self.layout.main().center, self.layout.main().radius);
        GeneratedMap {
            size: self.size,
            width: self.w as u32,
            height: self.w as u32,
            crater_center,
            crater_radius,
            terrain: self.terrain,
            starts: self.layout.starts,
            nodes: self.nodes,
            buildings: self.buildings,
            units: self.units,
            features,
            carved: self.carved,
            cut_off: self.cut_off,
        }
    }
}
