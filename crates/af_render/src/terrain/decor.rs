//! Decor: scenery models (mountain pieces, crater rims, reeds, sea cliffs,
//! bridges...) dressing the land. They are static models of their own kind
//! (`StaticKind::Decor`, the registry's `decor` section, built by the
//! static art pipeline into `models/static/decor/`), never game entities:
//! where each stands is a pure function of the map (the [`Field`] and
//! `MapFeatures`), hashed from positions and the seed, the same for every
//! player and untouched by the simulation ([`plan`]).
//!
//! The rules (docs/design/maps-and-terrain.md 6.3; nothing may look
//! gatherable, nothing stands on buildable ground, and nothing stands where
//! units walk unless it is low enough to wade through):
//!
//! | what | where |
//! |---|---|
//! | mountain peaks | the summits of the height field's ranges, sunk into them and scaled to the local height and width (snow on the tallest), never above [`DECOR_TOP`] |
//! | mountain ridges | along the ranges' spines between the peaks (`MapFeatures::mountains`) |
//! | foothills | at the ranges' ends and along their feet, on mountain cells only |
//! | crater rims | arcs along the rims of the rimmed sites, facing the centre, never in a gap |
//! | star cores, aether monoliths | on the Starspire's impassable footing; a star core in the Crater Lake's deep water |
//! | impact scars, meteor shards | sparse on rim and footing cells the arcs left free |
//! | reeds, river rocks, lily pads | shores and marsh; fords and river shallows; still lake water near the shore |
//! | brambles, ferns, shrubs, mossy boulders | thickets; thicket and marsh edges and woods; highland edges; the feet of mountains |
//! | sea stacks, beach rocks, sea cliffs, shipwrecks, driftwood | offshore; the waterline; the skirt's coast; the skirt's sea (and one far out on the map's) |
//! | bridges, standing stones, ruins, shrines, fallen logs, backdrops | off the map only: over the skirt's rivers, on its meadows and wood edges, and in its outer ring |
//!
//! Each rule draws from whatever variants of its models exist
//! ([`Catalogue`]); a rule whose model is missing places nothing, so with
//! no decor models the land looks exactly as it did without them.

use std::f32::consts::{PI, TAU};

use af_core::ext::map::{MapFeatures, SiteKind};
use af_core::ext::render::{MAX_TERRAIN_HEIGHT, WATER_LEVEL};
use bevy::prelude::*;

use super::field::{Cover, Field, RIVER_BANK};
use super::noise::{lattice, value};
use super::props::{PropKeep, surface};
use crate::statics::manifest::Fit;
use crate::statics::place::{base_centre, fit_scale};

/// The highest decor stands on the map: the terrain's own ceiling, below
/// every flyer (12 m and up).
pub const DECOR_TOP: f32 = MAX_TERRAIN_HEIGHT - 0.1;
/// How far beyond the map's edge decor goes: past this the skirt has
/// faded into the haze.
pub const SKIRT_REACH: f32 = 56.0;

/// Every decor model the game places, by its registry id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DecorKind {
    MountainPeak,
    MountainRidge,
    MountainFoothill,
    CraterRim,
    StarCore,
    AetherMonolith,
    ImpactScar,
    MeteorShard,
    Reeds,
    RiverRocks,
    LilyPads,
    SeaCliff,
    SeaStack,
    BeachRocks,
    Bramble,
    MossyBoulder,
    Shrub,
    Fern,
    StandingStones,
    RuinedWatchtower,
    WaysideShrine,
    FallenLog,
    Driftwood,
    Shipwreck,
    StoneBridge,
    WoodenFootbridge,
    BackdropMountains,
    BackdropForest,
}

/// Per kind, in the enum's order: its id, how it is fitted to its drawn
/// size and that size (w, h, d metres; the static build's defaults,
/// `art/static/lib/settings.py` `DECOR`, which a manifest's `size`
/// overrides), and the variants the art requests ask for
/// (`art/tripo/requests.json`).
const TABLE: [(DecorKind, &str, Fit, [f32; 3], &str); 28] = [
    (DecorKind::MountainPeak, "mountain_peak", Fit::Height, [13.0, 8.5, 13.0], "abcd"),
    (DecorKind::MountainRidge, "mountain_ridge", Fit::Footprint, [22.0, 6.0, 8.0], "abc"),
    (DecorKind::MountainFoothill, "mountain_foothill", Fit::Footprint, [10.0, 3.0, 8.0], "abc"),
    (DecorKind::CraterRim, "crater_rim", Fit::Footprint, [10.0, 2.5, 4.0], "abc"),
    (DecorKind::StarCore, "star_core", Fit::Footprint, [6.0, 3.5, 6.0], "ab"),
    (DecorKind::AetherMonolith, "aether_monolith", Fit::Height, [2.6, 8.0, 2.6], "a"),
    (DecorKind::ImpactScar, "impact_scar", Fit::Footprint, [5.0, 1.0, 4.0], "a"),
    (DecorKind::MeteorShard, "meteor_shard", Fit::Height, [1.4, 1.8, 1.2], "ab"),
    (DecorKind::Reeds, "reeds", Fit::Height, [1.2, 1.5, 1.2], "abc"),
    (DecorKind::RiverRocks, "river_rocks", Fit::Footprint, [2.0, 0.45, 1.4], "abc"),
    (DecorKind::LilyPads, "lily_pads", Fit::Footprint, [2.4, 0.15, 2.4], "a"),
    (DecorKind::SeaCliff, "sea_cliff", Fit::Footprint, [24.0, 7.0, 7.0], "abc"),
    (DecorKind::SeaStack, "sea_stack", Fit::Height, [4.5, 8.0, 4.5], "ab"),
    (DecorKind::BeachRocks, "beach_rocks", Fit::Footprint, [4.0, 0.8, 2.6], "a"),
    (DecorKind::Bramble, "bramble", Fit::Footprint, [2.2, 1.3, 2.2], "abc"),
    (DecorKind::MossyBoulder, "mossy_boulder", Fit::Footprint, [1.8, 1.2, 1.5], "abc"),
    (DecorKind::Shrub, "shrub", Fit::Footprint, [1.6, 1.3, 1.6], "ab"),
    (DecorKind::Fern, "fern", Fit::Footprint, [1.2, 0.8, 1.2], "ab"),
    (DecorKind::StandingStones, "standing_stones", Fit::Footprint, [5.0, 3.2, 3.0], "ab"),
    (DecorKind::RuinedWatchtower, "ruined_watchtower", Fit::Height, [5.0, 7.5, 5.0], "a"),
    (DecorKind::WaysideShrine, "wayside_shrine", Fit::Height, [1.4, 2.4, 1.2], "a"),
    (DecorKind::FallenLog, "fallen_log", Fit::Footprint, [5.0, 0.8, 1.2], "ab"),
    (DecorKind::Driftwood, "driftwood", Fit::Footprint, [3.5, 0.5, 1.2], "a"),
    (DecorKind::Shipwreck, "shipwreck", Fit::Footprint, [12.0, 5.0, 5.0], "a"),
    (DecorKind::StoneBridge, "stone_bridge", Fit::Footprint, [12.0, 3.0, 3.4], "a"),
    (DecorKind::WoodenFootbridge, "wooden_footbridge", Fit::Footprint, [10.0, 1.8, 2.0], "a"),
    (DecorKind::BackdropMountains, "backdrop_mountains", Fit::Footprint, [60.0, 12.0, 14.0], "ab"),
    (DecorKind::BackdropForest, "backdrop_forest", Fit::Footprint, [42.0, 7.0, 10.0], "ab"),
];

impl DecorKind {
    pub const COUNT: usize = TABLE.len();
    pub const ALL: [DecorKind; DecorKind::COUNT] = {
        let mut all = [DecorKind::MountainPeak; DecorKind::COUNT];
        let mut i = 0;
        while i < DecorKind::COUNT {
            all[i] = TABLE[i].0;
            i += 1;
        }
        all
    };

    pub fn index(self) -> usize {
        self as usize
    }

    /// The registry id (`mountain_peak`); source files are `<id>.glb` or
    /// `<id>_<variant>.glb`.
    pub fn id(self) -> &'static str {
        TABLE[self.index()].1
    }

    pub fn parse(id: &str) -> Option<DecorKind> {
        TABLE.iter().find(|t| t.1 == id).map(|t| t.0)
    }

    /// How it is fitted to [`DecorKind::size`] when its manifest says nothing.
    pub fn fit(self) -> Fit {
        TABLE[self.index()].2
    }

    /// The size it is drawn at before a placement's own scaling, when its
    /// manifest gives none (w, h, d metres; front +Z, long pieces along X).
    pub fn size(self) -> Vec3 {
        Vec3::from(TABLE[self.index()].3)
    }

    /// The variants the art requests ask for (`a`, `b`...).
    pub fn variant_names(self) -> impl Iterator<Item = String> {
        TABLE[self.index()].4.chars().map(String::from)
    }

    /// Never on the map: landmarks units would walk through, which stand
    /// only in the skirt beyond the edge (the shipwreck also far out in
    /// the map's open sea).
    pub fn skirt_only(self) -> bool {
        use DecorKind::*;
        matches!(self, StandingStones | RuinedWatchtower | WaysideShrine | FallenLog | Driftwood | StoneBridge | WoodenFootbridge | BackdropMountains | BackdropForest | SeaCliff)
    }

    /// Low enough for units to wade through where they stand on walkable
    /// (but unbuildable) ground: reeds, river rocks, lily pads, brambles,
    /// ferns, shrubs and beach rocks.
    pub fn wadeable(self) -> bool {
        use DecorKind::*;
        matches!(self, Reeds | RiverRocks | LilyPads | Bramble | Fern | Shrub | BeachRocks)
    }

    /// Stands on the rims of the meteor sites, which climbers walk: the
    /// rim arcs themselves and the debris by them.
    pub fn on_rims(self) -> bool {
        matches!(self, DecorKind::CraterRim | DecorKind::ImpactScar | DecorKind::MeteorShard)
    }

    /// Long and straight-sided: its footprint is its bounds' rectangle
    /// (others are round, their footprint the ellipse inside it).
    pub fn long(self) -> bool {
        use DecorKind::*;
        matches!(self, MountainRidge | CraterRim | SeaCliff | StoneBridge | WoodenFootbridge | FallenLog | Driftwood | Shipwreck | BackdropMountains | BackdropForest)
    }

    /// How much of its bounds' footprint its base covers: reeds, ferns and
    /// bushes spread out above a narrower foot, which is what must stand
    /// on allowed ground.
    pub fn foot(self) -> f32 {
        match self {
            DecorKind::Reeds => 0.6,
            DecorKind::Fern => 0.75,
            DecorKind::Shrub | DecorKind::Bramble => 0.85,
            _ => 1.0,
        }
    }
}

/// One variant of a decor model: its name (`a`, `b`... from its file) and
/// its bounds as built (base on y = 0, front +Z).
#[derive(Clone, Debug, PartialEq)]
pub struct DecorVariant {
    pub name: String,
    pub min: Vec3,
    pub max: Vec3,
}

impl DecorVariant {
    pub fn size(&self) -> Vec3 {
        (self.max - self.min).max(Vec3::splat(1e-3))
    }
}

/// A decor model that can be placed: how it is fitted to its drawn `size`,
/// and its loaded variants.
#[derive(Clone, Debug, PartialEq)]
pub struct DecorModel {
    pub fit: Fit,
    pub size: Vec3,
    pub variants: Vec<DecorVariant>,
}

/// The decor models there are, by kind (each with at least one variant).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalogue {
    models: Vec<Option<DecorModel>>,
}

impl Catalogue {
    pub fn insert(&mut self, kind: DecorKind, model: DecorModel) {
        if model.variants.is_empty() {
            return;
        }
        if self.models.len() < DecorKind::COUNT {
            self.models.resize(DecorKind::COUNT, None);
        }
        self.models[kind.index()] = Some(model);
    }

    pub fn get(&self, kind: DecorKind) -> Option<&DecorModel> {
        self.models.get(kind.index()).and_then(Option::as_ref)
    }

    pub fn has(&self, kind: DecorKind) -> bool {
        self.get(kind).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.models.iter().all(Option::is_none)
    }

    /// Every kind with every variant the art asks for, each a box of its
    /// drawn size (tests and tools).
    pub fn full() -> Catalogue {
        let mut c = Catalogue::default();
        for kind in DecorKind::ALL {
            let size = kind.size();
            let half = Vec3::new(size.x * 0.5, 0.0, size.z * 0.5);
            let variants = kind.variant_names().map(|name| DecorVariant { name, min: -half, max: half + Vec3::Y * size.y }).collect();
            c.insert(kind, DecorModel { fit: kind.fit(), size, variants });
        }
        c
    }
}

/// How much of the procedural brambles and reeds stays beside their
/// models: a share fills in under and between them.
pub fn keep_props(catalogue: &Catalogue) -> PropKeep {
    let keep = |kind| if catalogue.has(kind) { 0.4 } else { 1.0 };
    PropKeep { brambles: keep(DecorKind::Bramble), reeds: keep(DecorKind::Reeds) }
}

/// One placed decor model.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub kind: DecorKind,
    /// Index into the catalogue model's variants.
    pub variant: usize,
    /// Model space (as built) to the world.
    pub transform: Transform,
    /// Where its base's centre stands (world).
    pub at: Vec3,
    /// The radius of its footprint (metres): it covers no ground further
    /// from `at` than this.
    pub radius: f32,
    /// It casts a shadow (the big pieces; small scenery does not).
    pub shadow: bool,
}

impl Placed {
    /// How high its top stands (world y).
    pub fn top(&self, catalogue: &Catalogue) -> f32 {
        let v = &catalogue.get(self.kind).expect("placed from the catalogue").variants[self.variant];
        self.at.y + self.transform.scale.y * v.size().y
    }
}

/// Where every decor model of `catalogue` stands on this map: a pure
/// function of the field, the features and the catalogue (see the module
/// docs for the rules), in a fixed order.
pub fn plan(field: &Field, features: Option<&MapFeatures>, catalogue: &Catalogue) -> Vec<Placed> {
    if catalogue.is_empty() {
        return Vec::new();
    }
    let mut p = Planner::new(field, features, catalogue);
    p.peaks();
    p.ridges();
    p.foothills();
    p.rims();
    p.site_pieces();
    p.rim_debris();
    p.sea_stacks();
    p.sea_cliffs();
    p.shipwrecks();
    p.bridges();
    p.landmarks();
    p.backdrops();
    p.boulders_and_shrubs();
    p.thickets();
    p.wet_ground();
    p.waterline();
    // The small scenery comes last: on an extreme map it is what goes.
    p.out.truncate(MAX_DECOR);
    p.out
}

/// The ground a footprint of `kind` covers ([`DecorKind::long`]: its
/// rectangle; else the ellipse in it), centre `c`, half-extents `half.x`
/// along `axis` (a unit vector on the ground) and `half.y` across it.
fn footprint(kind: DecorKind, c: Vec2, axis: Vec2, half: Vec2) -> Vec<Vec2> {
    if kind.long() { rect_samples(c, axis, half) } else { samples(c, axis, half) }
}

/// A rectangle sampled about a metre apart, its edges and corners included.
fn rect_samples(c: Vec2, axis: Vec2, half: Vec2) -> Vec<Vec2> {
    let across = Vec2::new(-axis.y, axis.x);
    let (nx, nz) = (half.x.ceil().max(1.0) as i32, half.y.ceil().max(1.0) as i32);
    let mut v = Vec::with_capacity(((2 * nx + 1) * (2 * nz + 1)) as usize);
    for i in -nx..=nx {
        for j in -nz..=nz {
            v.push(c + axis * (i as f32 / nx as f32 * half.x) + across * (j as f32 / nz as f32 * half.y));
        }
    }
    v
}

/// The ellipse a footprint covers, sampled about a metre apart (and round
/// its rim): centre `c`, half-axes `half.x` along `axis` (a unit vector on
/// the ground) and `half.y` across it.
fn samples(c: Vec2, axis: Vec2, half: Vec2) -> Vec<Vec2> {
    let across = Vec2::new(-axis.y, axis.x);
    let (nx, nz) = (half.x.ceil().max(1.0) as i32, half.y.ceil().max(1.0) as i32);
    let mut v = Vec::with_capacity(((2 * nx + 1) * (2 * nz + 1) + 16) as usize);
    for i in -nx..=nx {
        for j in -nz..=nz {
            let (u, w) = (i as f32 / nx as f32, j as f32 / nz as f32);
            if u * u + w * w <= 1.0001 {
                v.push(c + axis * (u * half.x) + across * (w * half.y));
            }
        }
    }
    for k in 0..16 {
        let a = k as f32 / 16.0 * TAU;
        v.push(c + axis * (a.cos() * half.x) + across * (a.sin() * half.y));
    }
    v
}

/// The turn about +Y that lays a model's +X along `dir` (on the ground).
fn yaw_along(dir: Vec2) -> f32 {
    (-dir.y).atan2(dir.x)
}

/// The turn about +Y that faces a model's front (+Z) along `dir`.
fn yaw_facing(dir: Vec2) -> f32 {
    dir.x.atan2(dir.y)
}

fn unit_angle(a: f32) -> Vec2 {
    Vec2::new(a.cos(), a.sin())
}

/// What a footprint may cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ground {
    /// Unbuildable ground (units may walk there: the thing is low).
    Soft,
    /// Ground nobody walks on: mountains, deep water, the sea (and off the
    /// map, anything).
    Solid,
}

struct Planner<'a> {
    f: &'a Field,
    features: Option<&'a MapFeatures>,
    cat: &'a Catalogue,
    out: Vec<Placed>,
    /// Footprints (centre, radius) of the bigger pieces placed so far,
    /// which later ones keep clear of.
    taken: Vec<(Vec2, f32)>,
    /// Cells from each cell to the open sea (`SKIRT` everywhere without one).
    sea: Vec<f32>,
    /// The map's corners (world).
    lo: Vec2,
    hi: Vec2,
}

impl<'a> Planner<'a> {
    fn new(f: &'a Field, features: Option<&'a MapFeatures>, cat: &'a Catalogue) -> Planner<'a> {
        let ocean: Vec<bool> = f.cover.iter().map(|c| *c == Cover::Ocean).collect();
        let sea = f.distance_to(&ocean);
        Planner {
            f,
            features,
            cat,
            out: Vec::new(),
            taken: Vec::new(),
            sea,
            lo: f.world_corner(f.skirt, f.skirt),
            hi: f.world_corner(f.skirt + f.map_cols, f.skirt + f.map_rows),
        }
    }

    // --- the field -------------------------------------------------------

    /// A hash in 0..1 of a lattice point and a salt.
    fn rnd(&self, x: i32, z: i32, salt: u32) -> f32 {
        lattice(x, z, self.f.seed ^ salt.wrapping_mul(0x9e37_79b9))
    }

    fn idx(&self, p: Vec2) -> Option<usize> {
        let c = self.f.cell_of(p);
        (c.x >= 0 && c.y >= 0 && c.x < self.f.cols && c.y < self.f.rows).then(|| self.f.index(c.x, c.y))
    }

    fn cover(&self, p: Vec2) -> Option<Cover> {
        self.idx(p).map(|i| self.f.cover[i])
    }

    fn on_map(&self, p: Vec2) -> bool {
        p.x >= self.lo.x && p.y >= self.lo.y && p.x < self.hi.x && p.y < self.hi.y
    }

    fn beyond(&self, p: Vec2) -> f32 {
        self.f.beyond_map(p)
    }

    fn ground(&self, p: Vec2) -> f32 {
        surface(self.f, p)
    }

    fn in_spire(&self, p: Vec2, margin: f32) -> bool {
        self.f.spire.is_some_and(|(c, r)| p.distance(c) < r + margin)
    }

    /// May a footprint cover the cell at `p` (index `i`)? Never buildable
    /// ground; on the map, `Solid` pieces only where nobody walks; never
    /// further out than [`SKIRT_REACH`].
    fn allowed(&self, i: usize, p: Vec2, ground: Ground) -> bool {
        if !self.on_map(p) {
            return self.beyond(p) <= SKIRT_REACH;
        }
        if self.f.buildable[i] {
            return false;
        }
        match ground {
            Ground::Soft => true,
            Ground::Solid => matches!(self.f.cover[i], Cover::Mountain | Cover::Deep | Cover::Ocean),
        }
    }

    /// Every sample on a cell `ok` holds for (and `allowed`).
    fn fits(&self, pts: &[Vec2], ground: Ground, ok: impl Fn(usize, Vec2) -> bool) -> bool {
        pts.iter().all(|p| self.idx(*p).is_some_and(|i| self.allowed(i, *p, ground) && ok(i, *p)))
    }

    fn min_ground(&self, pts: &[Vec2]) -> f32 {
        pts.iter().map(|p| self.ground(*p)).fold(f32::MAX, f32::min)
    }

    fn max_ground(&self, pts: &[Vec2]) -> f32 {
        pts.iter().map(|p| self.ground(*p)).fold(f32::MIN, f32::max)
    }

    /// Clear of the bigger pieces placed so far (footprints may overlap
    /// by `slack` of their radii).
    fn clear(&self, p: Vec2, r: f32, slack: f32) -> bool {
        self.taken.iter().all(|(q, rq)| p.distance(*q) >= (r + rq) * (1.0 - slack))
    }

    /// Jittered lattice points about every `spacing` metres over the whole
    /// field, each with its lattice cell for hashing.
    fn lattice(&self, spacing: f32, salt: u32) -> Vec<(Vec2, i32, i32)> {
        let f = self.f;
        let size = Vec2::new(f.cols as f32, f.rows as f32) * f.cell;
        let (nx, nz) = ((size.x / spacing).ceil() as i32, (size.y / spacing).ceil() as i32);
        let mut v = Vec::with_capacity((nx * nz) as usize);
        for j in 0..nz {
            for i in 0..nx {
                let jitter = Vec2::new(self.rnd(i, j, salt ^ 0x11), self.rnd(i, j, salt ^ 0x22));
                v.push((f.origin + (Vec2::new(i as f32, j as f32) + Vec2::splat(0.15) + jitter * 0.7) * spacing, i, j));
            }
        }
        v
    }

    // --- the catalogue ---------------------------------------------------

    /// A variant of `kind`, preferring those named in `prefer` (any when
    /// none of them exists), chosen by `h` (0..1).
    fn pick(&self, kind: DecorKind, prefer: &[&str], h: f32) -> Option<usize> {
        let model = self.cat.get(kind)?;
        let named: Vec<usize> = (0..model.variants.len()).filter(|i| prefer.contains(&model.variants[*i].name.as_str())).collect();
        let pool: Vec<usize> = if named.is_empty() { (0..model.variants.len()).collect() } else { named };
        Some(pool[((h.clamp(0.0, 0.9999)) * pool.len() as f32) as usize])
    }

    fn variant(&self, kind: DecorKind, v: usize) -> &DecorVariant {
        &self.cat.get(kind).expect("picked from the catalogue").variants[v]
    }

    /// The scale that draws variant `v` at its kind's drawn size.
    fn nominal(&self, kind: DecorKind, v: usize) -> Vec3 {
        let model = self.cat.get(kind).expect("picked from the catalogue");
        fit_scale(model.fit, model.variants[v].size(), model.size)
    }

    /// Place variant `v` of `kind` with the centre of its base at `at`,
    /// turned by `yaw`, scaled by `scale`; on the map its top is held under
    /// [`DECOR_TOP`]. Returns its footprint's radius.
    fn put(&mut self, kind: DecorKind, v: usize, at: Vec3, yaw: f32, mut scale: Vec3, shadow: bool) -> f32 {
        let var = self.variant(kind, v).clone();
        let size = var.size();
        let radius = 0.5 * Vec2::new(scale.x * size.x, scale.z * size.z).length();
        if self.on_map(at.xz()) || self.beyond(at.xz()) < radius {
            let room = DECOR_TOP - at.y;
            if room <= 0.05 {
                return radius;
            }
            scale.y = scale.y.min(room / size.y);
        }
        let pivot = base_centre(var.min, var.max);
        let rotation = Quat::from_rotation_y(yaw);
        let transform = Transform { translation: at - rotation * (scale * pivot), rotation, scale };
        self.out.push(Placed { kind, variant: v, transform, at, radius, shadow });
        radius
    }

    // --- mountains -------------------------------------------------------

    /// Peaks on the summits: the height field's local maxima over mountain
    /// cells (the map's ranges and the skirt's), the highest first, each as
    /// wide as the rock around it allows, rising a little above the field
    /// (which stays the base) and sunk into it.
    fn peaks(&mut self) {
        const KIND: DecorKind = DecorKind::MountainPeak;
        if !self.cat.has(KIND) {
            return;
        }
        let f = self.f;
        let mut found: Vec<(f32, i32, i32)> = Vec::new();
        for z in 2..f.rows - 2 {
            for x in 2..f.cols - 2 {
                let i = f.index(x, z);
                if f.cover[i] != Cover::Mountain || f.rock[i] < 3.0 {
                    continue;
                }
                let h = f.height(x, z);
                if h < 4.5 {
                    continue;
                }
                let highest = (-3..=3).all(|dz: i32| (-3..=3).all(|dx: i32| (dx, dz) == (0, 0) || f.height(x + dx, z + dz) < h || (f.height(x + dx, z + dz) == h && (dz, dx) > (0, 0))));
                if highest {
                    found.push((h, x, z));
                }
            }
        }
        found.sort_by(|a, b| b.0.total_cmp(&a.0).then((a.1, a.2).cmp(&(b.1, b.2))));
        let mut peaks: Vec<(Vec2, f32)> = Vec::new();
        for (h, x, z) in found {
            let p = f.world_corner(x, z);
            if self.in_spire(p, 4.0) || self.beyond(p) > SKIRT_REACH - 6.0 {
                continue;
            }
            let rock = f.corner_cells(x, z).iter().map(|i| f.rock[*i]).fold(f32::MAX, f32::min);
            let r = ((rock - 0.6) * f.cell).min(8.5);
            if r < 2.6 || peaks.iter().any(|(q, rq)| p.distance(*q) < (r + rq) * 0.72 + 1.0) {
                continue;
            }
            let pts = samples(p, Vec2::X, Vec2::splat(r));
            if !self.fits(&pts, Ground::Solid, |i, _| f.cover[i] == Cover::Mountain) {
                continue;
            }
            let roll = self.rnd(x, z, 0x9ea1);
            let want = (h + 0.9 + 1.5 * roll).min(DECOR_TOP);
            let base = self.min_ground(&pts) - 0.3;
            // How a variant comes out here: its scale, keeping the model's
            // proportions within reason (narrower than the rock allows
            // rather than squashed, lower rather than stretched).
            let fit = |size: Vec3| {
                let mut sy = (want - base) / size.y;
                let sxz = (2.0 * r / size.x.max(size.z)).min(sy * 1.7);
                if sxz < sy * 0.6 {
                    sy = sxz / 0.6;
                }
                (sxz, sy, base + sy * size.y)
            };
            // Snow on the tallest summits, bare rock below, green flanks lowest.
            let prefer = |top: f32| -> &'static [&'static str] {
                if top >= 9.3 {
                    &["a", "b"]
                } else if top >= 7.6 {
                    &["c"]
                } else {
                    &["d", "c"]
                }
            };
            let snowy = |pl: &Planner, v: usize| matches!(pl.variant(KIND, v).name.as_str(), "a" | "b");
            let Some(mut v) = self.pick(KIND, prefer(want), self.rnd(x, z, 0x9ea2)) else { return };
            let (mut sxz, mut sy, mut top) = fit(self.variant(KIND, v).size());
            if snowy(self, v) && top < 9.3 {
                // It came out lower than meant: bare rock instead (when
                // there is a bare variant).
                let Some(w) = self.pick(KIND, prefer(top), self.rnd(x, z, 0x9ea2)) else { return };
                v = w;
                (sxz, sy, top) = fit(self.variant(KIND, v).size());
            }
            if top < h + 0.25 {
                continue; // it would hide inside the field's own summit
            }
            let size = self.variant(KIND, v).size();
            let radius = sxz * size.x.max(size.z) * 0.5;
            self.put(KIND, v, Vec3::new(p.x, base, p.y), self.rnd(x, z, 0x9ea3) * TAU, Vec3::new(sxz, sy, sxz), true);
            peaks.push((p, radius));
            self.taken.push((p, radius * 0.8));
            if peaks.len() >= 320 {
                break;
            }
        }
    }

    /// Ridges along the ranges' spines, between the peaks: each lies along
    /// the spine, as wide as the range there and a little above its crest.
    fn ridges(&mut self) {
        const KIND: DecorKind = DecorKind::MountainRidge;
        let Some(features) = self.features else { return };
        if !self.cat.has(KIND) {
            return;
        }
        let f = self.f;
        for (n, range) in features.mountains.iter().enumerate() {
            let spine = &range.spine;
            if spine.len() < 2 {
                continue;
            }
            let length: f32 = spine.windows(2).map(|w| w[0].distance(w[1])).sum();
            let step = 10.0;
            let mut s = step * (0.4 + 0.5 * self.rnd(n as i32, 0, 0x71d1));
            while s < length - 2.0 {
                let (p, t) = along(spine, s);
                let salt = (n as i32, (s * 4.0) as i32);
                s += step;
                // Ridges run on into the peaks (a chain of crags), but not
                // wholly inside one.
                if self.cover(p) != Some(Cover::Mountain) || self.in_spire(p, 4.0) || !self.clear(p, 2.5, 0.6) {
                    continue;
                }
                let Some(i) = self.idx(p) else { continue };
                let half_w = ((f.rock[i] - 0.6) * f.cell).min(6.5);
                if half_w < 1.8 {
                    continue;
                }
                // As long and wide as the rock allows (a bend or a narrow
                // stretch takes a shorter or slimmer piece).
                let mut fitted = None;
                'fit: for half_len in [step * 0.62, step * 0.45, step * 0.32] {
                    for across in [0.9, 0.7] {
                        let pts = footprint(KIND, p, t, Vec2::new(half_len, half_w * across));
                        if self.fits(&pts, Ground::Solid, |i, _| f.cover[i] == Cover::Mountain) {
                            fitted = Some((half_len, across, pts));
                            break 'fit;
                        }
                    }
                }
                let Some((half_len, across, pts)) = fitted else { continue };
                let crest = (-2..=2).map(|k| self.ground(p + t * (k as f32 * half_len * 0.45))).fold(f32::MIN, f32::max);
                let top = (crest + 0.5 + 0.9 * self.rnd(salt.0, salt.1, 0x71d2)).min(DECOR_TOP);
                let prefer: &[&str] = if top >= 9.0 {
                    &["b"]
                } else if top < 6.5 {
                    &["c"]
                } else {
                    &["a", "c"]
                };
                let Some(v) = self.pick(KIND, prefer, self.rnd(salt.0, salt.1, 0x71d3)) else { return };
                let size = self.variant(KIND, v).size();
                let base = self.min_ground(&pts) - 0.3;
                let (sx, sz) = (2.0 * half_len / size.x, 2.0 * half_w * across / size.z);
                let mean = (sx * sz).sqrt();
                let sy = ((top - base) / size.y).clamp(mean * 0.45, mean * 2.4);
                if base + sy * size.y < crest {
                    continue;
                }
                let flip = if self.rnd(salt.0, salt.1, 0x71d4) < 0.5 { PI } else { 0.0 };
                self.put(KIND, v, Vec3::new(p.x, base, p.y), yaw_along(t) + flip, Vec3::new(sx, sy, sz), true);
                self.taken.push((p, half_w.min(half_len)));
            }
        }
    }

    /// Low rocky knolls where the ranges end and along their feet, always
    /// on mountain cells, lying along the rock's edge.
    fn foothills(&mut self) {
        const KIND: DecorKind = DecorKind::MountainFoothill;
        if !self.cat.has(KIND) {
            return;
        }
        let f = self.f;
        let mut spots: Vec<(Vec2, Option<Vec2>, i32, i32)> = Vec::new();
        if let Some(features) = self.features {
            for (n, range) in features.mountains.iter().enumerate() {
                let s = &range.spine;
                if s.len() >= 2 {
                    for (end, inner, k) in [(s[0], s[1], 0), (s[s.len() - 1], s[s.len() - 2], 1)] {
                        // Where the range tapers out: a knoll carrying its line on.
                        let out = (end - inner).normalize_or(Vec2::X);
                        if let Some(d) = [3.0, 5.0, 7.0, 9.0].into_iter().find(|d| self.rock_at(end - out * *d) >= 2.5) {
                            spots.push((end - out * d, Some(out), n as i32, k));
                        }
                    }
                }
            }
        }
        for (p, i, j) in self.lattice(13.0, 0xf007) {
            if self.rnd(i, j, 0xf008) < 0.55 {
                spots.push((p, None, i, j));
            }
        }
        for (p, along_edge, i, j) in spots {
            let Some(c) = self.idx(p) else { continue };
            if f.cover[c] != Cover::Mountain || self.in_spire(p, 6.0) || f.rock[c] < 2.0 || f.rock[c] > 6.0 {
                continue;
            }
            // Lie along the rock's edge: across the way into the range.
            let into = Vec2::new(self.rock_at(p + Vec2::X) - self.rock_at(p - Vec2::X), self.rock_at(p + Vec2::Y) - self.rock_at(p - Vec2::Y));
            let axis = along_edge.unwrap_or_else(|| into.perp().normalize_or(Vec2::X));
            let r = ((f.rock[c] - 0.4) * f.cell).min(5.5);
            let pts = samples(p, axis, Vec2::new(r, r * 0.8));
            if r < 1.8 || !self.clear(p, r, 0.35) || !self.fits(&pts, Ground::Solid, |i, _| f.cover[i] == Cover::Mountain) {
                continue;
            }
            let Some(v) = self.pick(KIND, &[], self.rnd(i, j, 0xf009)) else { return };
            let size = self.variant(KIND, v).size();
            let base = self.min_ground(&pts) - 0.15;
            let rise = (r * 0.55).clamp(1.0, 3.0) * (0.8 + 0.4 * self.rnd(i, j, 0xf00a));
            let (sx, sz) = (2.0 * r / size.x, 1.6 * r / size.z);
            let sy = (rise / size.y).clamp((sx * sz).sqrt() * 0.4, (sx * sz).sqrt() * 2.0);
            let flip = if self.rnd(i, j, 0xf00b) < 0.5 { PI } else { 0.0 };
            self.put(KIND, v, Vec3::new(p.x, base, p.y), yaw_along(axis) + flip, Vec3::new(sx, sy, sz), true);
            self.taken.push((p, r * 0.8));
        }
    }

    /// Cells into the range at a point (0 off the mountains).
    fn rock_at(&self, p: Vec2) -> f32 {
        self.idx(p).filter(|i| self.f.cover[*i] == Cover::Mountain).map_or(0.0, |i| self.f.rock[i])
    }

    // --- meteor sites -----------------------------------------------------

    /// The sites with their kinds (without features: the field's, as craters).
    fn sites(&self) -> Vec<(SiteKind, Vec2, f32)> {
        match self.features {
            Some(m) => m.sites.iter().map(|s| (s.kind, s.center, s.radius)).collect(),
            None => self.f.sites.iter().map(|(c, r)| (SiteKind::Crater, *c, *r)).collect(),
        }
    }

    /// Along the ray from `c` in `dir`, the stretch of a site's rim nearest
    /// radius `r`: (inner, outer) radii.
    fn rim_band(&self, c: Vec2, dir: Vec2, r: f32) -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32)> = None;
        let mut consider = |a: f32, b: f32| {
            if best.is_none_or(|(b0, b1)| ((a + b) * 0.5 - r).abs() < ((b0 + b1) * 0.5 - r).abs()) {
                best = Some((a, b));
            }
        };
        let (mut s, end) = ((r * 0.5).max(1.0), r * 1.6 + 4.0);
        let mut start = None;
        while s <= end {
            let on = self.idx(c + dir * s).is_some_and(|i| self.f.rim[i] > 0.0);
            match (on, start) {
                (true, None) => start = Some(s),
                (false, Some(a)) => {
                    consider(a, s - 0.5);
                    start = None;
                }
                _ => {}
            }
            s += 0.5;
        }
        if let Some(a) = start {
            consider(a, end);
        }
        best
    }

    /// Arcs of rim along the rims of the rimmed sites (crater, shattered,
    /// lake, scar), their steep inner face toward the site's centre and
    /// their length along the rim, each wholly on rim cells (so never in a
    /// gap), as wide as the rim and rising 1.4-2.7 m above it.
    fn rims(&mut self) {
        const KIND: DecorKind = DecorKind::CraterRim;
        if !self.cat.has(KIND) {
            return;
        }
        let f = self.f;
        for (n, (kind, c, radius)) in self.sites().into_iter().enumerate() {
            if kind == SiteKind::Spire {
                continue;
            }
            // Straight pieces round a curved rim: short enough that the
            // arc strays little from its chord (a fifth of a metre), so the
            // footprint stays on the rim (whose edges step from cell to
            // cell); a little overlap between them.
            let half_piece = (2.0 * radius * 0.2).sqrt().clamp(2.2, 5.0);
            let count = ((TAU * radius / (2.0 * half_piece * 0.92)).ceil() as usize).clamp(8, 120);
            let phase = self.rnd(n as i32, 0, 0xc2a1) * TAU;
            for k in 0..count {
                let theta = phase + k as f32 / count as f32 * TAU;
                let dir = unit_angle(theta);
                let Some((r0, r1)) = self.rim_band(c, dir, radius) else { continue };
                let width = r1 - r0 + 0.5;
                if width < 1.5 {
                    continue;
                }
                let rc = (r0 + r1) * 0.5;
                let p = c + dir * rc;
                let tangent = dir.perp();
                // Its whole footprint on the rim: never in a gap, never over
                // the floor inside or the ground outside. Shorter or
                // narrower where the rim ends or bends.
                let full = (PI * rc / count as f32 / 0.92).min(half_piece * 1.1);
                let fitted = [(full, 0.8), (full, 0.65), (full, 0.5), (full * 0.65, 0.65), (full * 0.65, 0.5)].into_iter().find(|(half_len, across)| {
                    let pts = footprint(KIND, p, tangent, Vec2::new(*half_len, width * 0.5 * across));
                    pts.iter().all(|q| self.idx(*q).is_some_and(|j| f.rim[j] > 0.0 && !f.buildable[j]))
                });
                let Some((half_len, across)) = fitted else { continue };
                let roll = self.rnd(n as i32, k as i32, 0xc2a2);
                let prefer: &[&str] = if roll < 0.2 {
                    &["c"]
                } else if radius < 22.0 {
                    &["a"]
                } else {
                    &["b"]
                };
                let Some(v) = self.pick(KIND, prefer, self.rnd(n as i32, k as i32, 0xc2a3)) else { return };
                let size = self.variant(KIND, v).size();
                let pts = footprint(KIND, p, tangent, Vec2::new(half_len, width * 0.5 * across));
                let base = self.min_ground(&pts) - 0.2;
                let rise = if self.variant(KIND, v).name == "c" { 2.3 + 0.4 * roll } else { 1.4 + 0.6 * self.rnd(n as i32, k as i32, 0xc2a4) };
                let top = self.ground(p) + rise;
                let (sx, sz) = (2.0 * half_len / size.x, width * across / size.z);
                let mean = (sx * sz).sqrt();
                let sy = ((top - base) / size.y).clamp(mean * 0.35, mean * 2.5);
                self.put(KIND, v, Vec3::new(p.x, base, p.y), yaw_facing(-dir), Vec3::new(sx, sy, sz), true);
                self.taken.push((p, (width * 0.5).max(half_len * 0.7)));
            }
        }
    }

    /// The Starspire's monoliths and star core on its impassable footing,
    /// and a star core half sunk in the Crater Lake's deep water.
    fn site_pieces(&mut self) {
        if let Some((c, r)) = self.f.spire {
            if self.cat.has(DecorKind::AetherMonolith) {
                let count = 3 + (self.rnd(1, 0, 0x5a11) * 2.99) as usize;
                let phase = self.rnd(2, 0, 0x5a12) * TAU;
                for k in 0..count {
                    let a = phase + (k as f32 + 0.3 * self.rnd(k as i32, 3, 0x5a13)) / count as f32 * TAU;
                    let p = c + unit_angle(a) * r * (0.5 + 0.2 * self.rnd(k as i32, 4, 0x5a14));
                    let Some(v) = self.pick(DecorKind::AetherMonolith, &[], self.rnd(k as i32, 5, 0x5a15)) else { break };
                    let size = self.variant(DecorKind::AetherMonolith, v).size();
                    let base = self.ground(p) - 0.4;
                    let height = (5.0 + 3.5 * self.rnd(k as i32, 6, 0x5a16)).min(DECOR_TOP - base);
                    let s = (height / size.y).min(r * 0.28 * 2.0 / size.x.max(size.z));
                    let radius = s * size.x.max(size.z) * 0.5;
                    let pts = samples(p, Vec2::X, Vec2::splat(radius));
                    if !self.fits(&pts, Ground::Solid, |i, _| self.f.cover[i] == Cover::Mountain) {
                        continue;
                    }
                    self.put(DecorKind::AetherMonolith, v, Vec3::new(p.x, base, p.y), self.rnd(k as i32, 7, 0x5a17) * TAU, Vec3::splat(s), true);
                    self.taken.push((p, radius));
                }
            }
            if self.cat.has(DecorKind::StarCore) {
                let a = self.rnd(3, 1, 0x5a21) * TAU;
                let p = c + unit_angle(a) * r * 0.3;
                if let Some(v) = self.pick(DecorKind::StarCore, &["a"], self.rnd(3, 2, 0x5a22)) {
                    let size = self.variant(DecorKind::StarCore, v).size();
                    let s = (r * 0.7 / size.x.max(size.z)).min(self.nominal(DecorKind::StarCore, v).x * 1.2);
                    let radius = s * size.x.max(size.z) * 0.5;
                    let pts = samples(p, Vec2::X, Vec2::splat(radius));
                    if self.fits(&pts, Ground::Solid, |i, _| self.f.cover[i] == Cover::Mountain) {
                        // Sunk deep: its domed top and a side showing.
                        let base = self.ground(p) - s * size.y * 0.35;
                        self.put(DecorKind::StarCore, v, Vec3::new(p.x, base, p.y), self.rnd(3, 3, 0x5a23) * TAU, Vec3::splat(s), true);
                        self.taken.push((p, radius));
                    }
                }
            }
        }
        if !self.cat.has(DecorKind::StarCore) {
            return;
        }
        for (n, (kind, c, radius)) in self.sites().into_iter().enumerate() {
            if kind != SiteKind::Lake {
                continue;
            }
            // Round the deep ring between the island and the shore, from a
            // hashed start, the first spot deep enough all over.
            let phase = self.rnd(n as i32, 9, 0x5a31) * TAU;
            for k in 0..24 {
                let p = c + unit_angle(phase + k as f32 / 24.0 * TAU) * radius * 0.62;
                let Some(v) = self.pick(DecorKind::StarCore, &["b"], self.rnd(n as i32, k, 0x5a32)) else { return };
                let size = self.variant(DecorKind::StarCore, v).size();
                let s = self.nominal(DecorKind::StarCore, v).x * 0.8;
                let radius_m = s * size.x.max(size.z) * 0.5;
                let pts = samples(p, Vec2::X, Vec2::splat(radius_m + 1.0));
                if self.fits(&pts, Ground::Solid, |i, _| self.f.cover[i] == Cover::Deep) {
                    let base = WATER_LEVEL - s * size.y * 0.45;
                    self.put(DecorKind::StarCore, v, Vec3::new(p.x, base, p.y), self.rnd(n as i32, k, 0x5a33) * TAU, Vec3::splat(s), true);
                    self.taken.push((p, radius_m));
                    break;
                }
            }
        }
    }

    /// Impact scars and meteor shards, sparse, on the rim and footing
    /// cells the rim arcs left free (the rims' torn ends by the gaps, the
    /// Starspire's footing).
    fn rim_debris(&mut self) {
        let f = self.f;
        let debris = |p: &Planner, i: usize, q: Vec2| (f.rim[i] > 0.0 && !f.buildable[i]) || (f.cover[i] == Cover::Mountain && p.in_spire(q, 0.0));
        // The spire's heart (its crystal cluster) is left to the crystal.
        let heart = self.f.spire.map(|(c, r)| (c, r * 0.4));
        for (kind, spacing, chance, cap) in [(DecorKind::ImpactScar, 9.0, 0.45, 18), (DecorKind::MeteorShard, 5.0, 0.4, 60)] {
            if !self.cat.has(kind) {
                continue;
            }
            let mut placed = 0;
            for (p, i, j) in self.lattice(spacing, 0xd3b1 ^ kind.index() as u32) {
                if placed >= cap {
                    break;
                }
                let Some(c) = self.idx(p) else { continue };
                if !debris(self, c, p) || self.rnd(i, j, 0xd3b2) >= chance || heart.is_some_and(|(hc, hr)| p.distance(hc) < hr) {
                    continue;
                }
                let Some(v) = self.pick(kind, &[], self.rnd(i, j, 0xd3b3)) else { break };
                let size = self.variant(kind, v).size();
                let nominal = self.nominal(kind, v).x;
                let mut s = nominal * (0.7 + 0.5 * self.rnd(i, j, 0xd3b4));
                let yaw = self.rnd(i, j, 0xd3b5) * TAU;
                let axis = unit_angle(-yaw);
                // As big as fits (at least half its drawn size).
                while s >= nominal * 0.5 {
                    let half = Vec2::new(size.x, size.z) * s * 0.5;
                    let pts = footprint(kind, p, axis, half);
                    if self.clear(p, half.max_element(), 0.1) && self.fits(&pts, Ground::Soft, |i, q| debris(self, i, q)) {
                        let base = self.min_ground(&pts) - if kind == DecorKind::MeteorShard { 0.15 * s * size.y } else { 0.08 };
                        self.put(kind, v, Vec3::new(p.x, base, p.y), yaw, Vec3::splat(s), kind == DecorKind::ImpactScar);
                        self.taken.push((p, half.max_element() * 0.8));
                        placed += 1;
                        break;
                    }
                    s *= 0.8;
                }
            }
        }
    }

    // --- coasts ------------------------------------------------------------

    /// Sea stacks offshore, in the map's sea (sparse, far from the beach)
    /// and the skirt's.
    fn sea_stacks(&mut self) {
        const KIND: DecorKind = DecorKind::SeaStack;
        if !self.cat.has(KIND) {
            return;
        }
        let f = self.f;
        let (mut on_map, mut off_map) = (0, 0);
        for (p, i, j) in self.lattice(24.0, 0x5e01) {
            let Some(c) = self.idx(p) else { continue };
            let inside = self.on_map(p);
            // Off the headlands, not scattered over the whole sea.
            if f.cover[c] != Cover::Ocean || f.shore[c] < 6.0 || f.shore[c] > 18.0 || self.rnd(i, j, 0x5e02) > 0.4 || (inside && on_map >= 5) || (!inside && off_map >= 14) {
                continue;
            }
            let Some(v) = self.pick(KIND, &[], self.rnd(i, j, 0x5e03)) else { return };
            let size = self.variant(KIND, v).size();
            let base = self.ground(p) - 0.3;
            let top = 5.5 + 3.8 * self.rnd(i, j, 0x5e04);
            let s = (top - base) / size.y;
            let radius = s * size.x.max(size.z) * 0.5;
            let pts = samples(p, Vec2::X, Vec2::splat(radius + 1.0));
            if !self.clear(p, radius, 0.0) || !self.fits(&pts, Ground::Solid, |i, _| f.cover[i] == Cover::Ocean) {
                continue;
            }
            self.put(KIND, v, Vec3::new(p.x, base, p.y), self.rnd(i, j, 0x5e05) * TAU, Vec3::splat(s), true);
            self.taken.push((p, radius));
            if inside {
                on_map += 1;
            } else {
                off_map += 1;
            }
        }
    }

    /// The direction from `p` to the open sea, when there is some within
    /// `reach` metres.
    fn to_sea(&self, p: Vec2, reach: i32) -> Option<Vec2> {
        let mut sum = Vec2::ZERO;
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                let d = Vec2::new(dx as f32, dz as f32);
                if d.length() <= reach as f32 && d != Vec2::ZERO && self.cover(p + d * self.f.cell) == Some(Cover::Ocean) {
                    sum += d.normalize() / d.length();
                }
            }
        }
        (sum.length() > 0.2).then(|| sum.normalize())
    }

    /// Sea cliffs along the skirt's coast (where the land beyond the map
    /// meets the sea), their face (+Z) to the sea and their length along
    /// the shore.
    fn sea_cliffs(&mut self) {
        const KIND: DecorKind = DecorKind::SeaCliff;
        if !self.cat.has(KIND) {
            return;
        }
        let f = self.f;
        let mut placed = 0;
        for (p, i, j) in self.lattice(9.0, 0x5c11) {
            if placed >= 18 {
                break;
            }
            let Some(c) = self.idx(p) else { continue };
            // The skirt's land at the water, with the open sea close beyond
            // its shallows.
            if self.on_map(p) || self.beyond(p) < 8.0 || !f.cover[c].is_land() || f.shore[c] > 2.5 || self.sea[c] > 12.0 {
                continue;
            }
            let Some(out) = self.to_sea(p, 13) else { continue };
            let Some(v) = self.pick(KIND, &[], self.rnd(i, j, 0x5c12)) else { return };
            let size = self.variant(KIND, v).size();
            let half_len = 8.0 + 4.0 * self.rnd(i, j, 0x5c13);
            let s = 2.0 * half_len / size.x;
            let depth = size.z * s;
            // Its face at the waterline, its body on the land behind.
            let centre = p - out * (depth * 0.25);
            let pts = footprint(KIND, centre, out.perp(), Vec2::new(half_len, depth * 0.5));
            if !self.clear(centre, half_len * 0.8, 0.15) || !pts.iter().all(|q| !self.on_map(*q) && self.beyond(*q) >= 3.0 && self.beyond(*q) <= SKIRT_REACH) {
                continue;
            }
            let land = pts.iter().filter(|q| self.cover(**q).is_some_and(Cover::is_land)).count();
            let rock = pts.iter().filter(|q| self.cover(**q) == Some(Cover::Mountain)).count();
            if land * 10 < pts.len() * 3 || rock * 4 > pts.len() {
                continue;
            }
            let base = self.min_ground(&pts) - 0.4;
            let top = self.max_ground(&pts).max(0.0) + 3.5 + 2.5 * self.rnd(i, j, 0x5c14);
            let sy = ((top - base) / size.y).clamp(s * 0.5, s * 2.0);
            self.put(KIND, v, Vec3::new(centre.x, base, centre.y), yaw_facing(out), Vec3::new(s, sy, s), true);
            self.taken.push((centre, half_len * 0.8));
            placed += 1;
        }
    }

    /// Wrecks: on the skirt's coast a little offshore, and one far out in
    /// the map's own sea at most.
    fn shipwrecks(&mut self) {
        const KIND: DecorKind = DecorKind::Shipwreck;
        if !self.cat.has(KIND) {
            return;
        }
        let f = self.f;
        let (mut on_map, mut off_map) = (0, 0);
        for (p, i, j) in self.lattice(36.0, 0x5b01) {
            let Some(c) = self.idx(p) else { continue };
            let inside = self.on_map(p);
            let want = if inside { f.shore[c] >= 7.0 && on_map < 1 && self.rnd(i, j, 0x5b02) < 0.35 } else { (2.0..=6.0).contains(&f.shore[c]) && off_map < 3 && self.rnd(i, j, 0x5b02) < 0.6 };
            if f.cover[c] != Cover::Ocean || !want || (!inside && self.beyond(p) < 10.0) {
                continue;
            }
            let Some(v) = self.pick(KIND, &[], self.rnd(i, j, 0x5b03)) else { return };
            let size = self.variant(KIND, v).size();
            let s = self.nominal(KIND, v).x * (0.85 + 0.25 * self.rnd(i, j, 0x5b04));
            let yaw = self.rnd(i, j, 0x5b05) * TAU;
            let half = Vec2::new(size.x, size.z) * s * 0.5;
            let pts = footprint(KIND, p, unit_angle(-yaw), half + Vec2::splat(if inside { 3.0 } else { 0.5 }));
            if !self.clear(p, half.x, 0.0) || !self.fits(&pts, Ground::Solid, |i, _| f.cover[i] == Cover::Ocean) {
                continue;
            }
            // Stranded on its side, half under the water.
            let base = WATER_LEVEL - 0.45 * s * size.y;
            self.put(KIND, v, Vec3::new(p.x, base, p.y), yaw, Vec3::splat(s), true);
            self.taken.push((p, half.x));
            if inside {
                on_map += 1;
            } else {
                off_map += 1;
            }
        }
    }

    // --- off the map ---------------------------------------------------------

    /// A bridge over each river where its line runs on across the skirt
    /// (stone where it is wide, a wooden footbridge where it is narrow),
    /// spanning bank to bank; now and then a wayside shrine by its end.
    fn bridges(&mut self) {
        let (stone, wood) = (self.cat.has(DecorKind::StoneBridge), self.cat.has(DecorKind::WoodenFootbridge));
        if !stone && !wood {
            return;
        }
        let courses = self.f.courses.clone();
        for (n, course) in courses.iter().enumerate() {
            let pts = &course.points;
            if pts.len() < 8 {
                continue;
            }
            // Each end that runs on beyond the map: walk outward from the edge.
            let ends: [Vec<usize>; 2] = [(0..pts.len()).collect(), (0..pts.len()).rev().collect()];
            for (e, order) in ends.iter().enumerate() {
                if self.beyond(pts[order[0]]) < 1.0 {
                    continue; // this end is on the map (a tributary's mouth)
                }
                let want = 14.0 + 16.0 * self.rnd(n as i32, e as i32, 0xb1d1);
                // From the end inward, the point first that far out.
                let mut seq: Vec<usize> = order.iter().copied().take_while(|k| self.beyond(pts[*k]) > 0.5).collect();
                seq.reverse(); // now from the edge outward
                for (step, k) in seq.iter().copied().enumerate() {
                    if self.beyond(pts[k]) < want || step % 2 == 1 {
                        continue;
                    }
                    let p = pts[k];
                    let t = (pts[(k + 1).min(pts.len() - 1)] - pts[k.saturating_sub(1)]).normalize_or(Vec2::X);
                    let across = t.perp();
                    let half = course.half.get(k).copied().unwrap_or(2.0);
                    let span = half + RIVER_BANK + 1.4;
                    let ends_ok = [-1.0f32, 1.0].iter().all(|sg| {
                        let q = p + across * (span + 0.8) * *sg;
                        self.cover(q).is_some_and(|c| c.is_land() && c != Cover::Woodland) && !self.on_map(q) && self.beyond(q) >= 3.0
                    });
                    if !ends_ok || self.beyond(p) > SKIRT_REACH - 8.0 {
                        continue;
                    }
                    let kind = if (stone && half >= 1.6) || !wood { DecorKind::StoneBridge } else { DecorKind::WoodenFootbridge };
                    let Some(v) = self.pick(kind, &[], self.rnd(n as i32, e as i32, 0xb1d2)) else { break };
                    let size = self.variant(kind, v).size();
                    let s = (2.0 * (span + 0.8) / size.x).clamp(0.4, 2.6);
                    let banks = [self.ground(p + across * (span + 0.6)), self.ground(p - across * (span + 0.6))];
                    let base = banks[0].min(banks[1]).max(WATER_LEVEL) - 0.12;
                    self.put(kind, v, Vec3::new(p.x, base, p.y), yaw_along(across), Vec3::splat(s), true);
                    self.taken.push((p, span));
                    // A shrine by the road at one end.
                    if self.cat.has(DecorKind::WaysideShrine) && self.rnd(n as i32, e as i32, 0xb1d3) < 0.7 {
                        let side = if self.rnd(n as i32, e as i32, 0xb1d4) < 0.5 { 1.0 } else { -1.0 };
                        let q = p + across * (span + 2.6) * side + t * 2.4;
                        self.landmark_at(DecorKind::WaysideShrine, q, n as i32 * 7 + e as i32, 0xb1d5);
                    }
                    break;
                }
            }
        }
    }

    /// Put a landmark of `kind` at `q` in the skirt if it fits there: on
    /// open meadow (or a wood's edge for logs), clear of the rest.
    fn landmark_at(&mut self, kind: DecorKind, q: Vec2, i: i32, salt: u32) -> bool {
        let Some(v) = self.pick(kind, &[], self.rnd(i, 0, salt)) else { return false };
        let size = self.variant(kind, v).size();
        let s = self.nominal(kind, v).x * (0.85 + 0.3 * self.rnd(i, 1, salt));
        let yaw = self.rnd(i, 2, salt) * TAU;
        let half = Vec2::new(size.x, size.z) * s * 0.5;
        let pts = footprint(kind, q, unit_angle(-yaw), half + Vec2::splat(0.5));
        let ground_ok = |c: Cover| if kind == DecorKind::FallenLog { matches!(c, Cover::Meadow | Cover::Woodland) } else { c == Cover::Meadow };
        if !self.clear(q, half.max_element(), 0.0) || !pts.iter().all(|p| !self.on_map(*p) && self.beyond(*p) >= 4.0 && self.beyond(*p) <= SKIRT_REACH - 4.0 && self.cover(*p).is_some_and(ground_ok)) {
            return false;
        }
        let base = self.min_ground(&pts) - 0.08;
        let shadow = !matches!(kind, DecorKind::FallenLog | DecorKind::Driftwood);
        self.put(kind, v, Vec3::new(q.x, base, q.y), yaw, Vec3::splat(s), shadow);
        self.taken.push((q, half.max_element() + 4.0));
        true
    }

    /// Standing stones, ruins and shrines on the skirt's meadows, fallen
    /// logs at its woods' edges and driftwood on its coast, well apart.
    fn landmarks(&mut self) {
        let f = self.f;
        let meadow: Vec<DecorKind> = [(DecorKind::StandingStones, 3), (DecorKind::RuinedWatchtower, 1), (DecorKind::WaysideShrine, 2)]
            .into_iter()
            .filter(|(k, _)| self.cat.has(*k))
            .flat_map(|(k, w)| std::iter::repeat_n(k, w))
            .collect();
        let caps = |k: DecorKind| match k {
            DecorKind::RuinedWatchtower => 2,
            DecorKind::StandingStones => 4,
            DecorKind::WaysideShrine => 5,
            DecorKind::FallenLog => 10,
            DecorKind::Driftwood => 8,
            _ => 0,
        };
        // The shrines by the bridges count too.
        let mut placed = [0usize; DecorKind::COUNT];
        for p in &self.out {
            placed[p.kind.index()] += 1;
        }
        for (p, i, j) in self.lattice(30.0, 0x1a01) {
            if meadow.is_empty() || self.on_map(p) || !(10.0..=SKIRT_REACH - 12.0).contains(&self.beyond(p)) || self.rnd(i, j, 0x1a02) > 0.6 {
                continue;
            }
            let kind = meadow[(self.rnd(i, j, 0x1a03) * meadow.len() as f32) as usize % meadow.len()];
            if placed[kind.index()] < caps(kind) && self.landmark_at(kind, p, i * 131 + j, 0x1a04) {
                placed[kind.index()] += 1;
            }
        }
        for (kind, spacing, salt) in [(DecorKind::FallenLog, 14.0, 0x1a11), (DecorKind::Driftwood, 11.0, 0x1a21)] {
            if !self.cat.has(kind) {
                continue;
            }
            for (p, i, j) in self.lattice(spacing, salt) {
                if placed[kind.index()] >= caps(kind) {
                    break;
                }
                let Some(c) = self.idx(p) else { continue };
                if self.on_map(p) || self.beyond(p) < 6.0 || self.rnd(i, j, salt ^ 1) > 0.5 {
                    continue;
                }
                let ok = match kind {
                    DecorKind::FallenLog => {
                        ((f.cover[c] == Cover::Woodland && f.wood[c] <= 3.0) || (f.cover[c] == Cover::Meadow && self.wood_near(p, 3))) && self.landmark_at(kind, p, i * 97 + j, salt ^ 2)
                    }
                    _ => f.cover[c].is_land() && f.shore[c] <= 2.0 && self.sea[c] <= 12.0 && self.drift_at(p, i * 97 + j),
                };
                if ok {
                    placed[kind.index()] += 1;
                }
            }
        }
    }

    fn wood_near(&self, p: Vec2, reach: i32) -> bool {
        (-reach..=reach).any(|dz| (-reach..=reach).any(|dx| self.cover(p + Vec2::new(dx as f32, dz as f32) * self.f.cell) == Some(Cover::Woodland)))
    }

    /// Driftwood washed up at the skirt's waterline (on its land cells).
    fn drift_at(&mut self, q: Vec2, i: i32) -> bool {
        const KIND: DecorKind = DecorKind::Driftwood;
        let Some(v) = self.pick(KIND, &[], self.rnd(i, 0, 0x1a31)) else { return false };
        let size = self.variant(KIND, v).size();
        let s = self.nominal(KIND, v).x * (0.8 + 0.4 * self.rnd(i, 1, 0x1a31));
        let yaw = self.rnd(i, 2, 0x1a31) * TAU;
        let half = Vec2::new(size.x, size.z) * s * 0.5;
        let pts = footprint(KIND, q, unit_angle(-yaw), half);
        if !self.clear(q, half.x, 0.0) || !pts.iter().all(|p| !self.on_map(*p) && self.beyond(*p) >= 3.0 && self.cover(*p).is_some_and(|c| c.is_land() && c != Cover::Mountain)) {
            return false;
        }
        let base = self.min_ground(&pts) - 0.05;
        self.put(KIND, v, Vec3::new(q.x, base, q.y), yaw, Vec3::splat(s), false);
        self.taken.push((q, half.x));
        true
    }

    /// Distant mountains and forest in the skirt's outer ring, along the
    /// map's sides, facing the map: what the land fades into.
    fn backdrops(&mut self) {
        let (hills, woods) = (self.cat.has(DecorKind::BackdropMountains), self.cat.has(DecorKind::BackdropForest));
        if !hills && !woods {
            return;
        }
        // Their far side at the skirt's far edge (where the land has faded
        // into the haze), their near side well out.
        let far = self.f.skirt as f32 * self.f.cell - 1.5;
        // Each side, from corner to corner, every 40 m or so.
        let sides = [(Vec2::new(self.lo.x, self.lo.y), Vec2::new(self.hi.x, self.lo.y), -Vec2::Y), (Vec2::new(self.hi.x, self.lo.y), Vec2::new(self.hi.x, self.hi.y), Vec2::X), (Vec2::new(self.hi.x, self.hi.y), Vec2::new(self.lo.x, self.hi.y), Vec2::Y), (Vec2::new(self.lo.x, self.hi.y), Vec2::new(self.lo.x, self.lo.y), -Vec2::X)];
        for (s_i, (a, b, outward)) in sides.into_iter().enumerate() {
            let length = a.distance(b);
            let n = (length / 40.0).round().max(1.0) as usize;
            for k in 0..n {
                let t = (k as f32 + 0.5) / n as f32;
                let along = a.lerp(b, t) + (b - a).normalize_or(Vec2::X) * (self.rnd(s_i as i32, k as i32, 0xbd01) - 0.5) * 12.0;
                let Some(cover) = self.cover(along + outward * (far - 6.0)) else { continue };
                if cover.is_water() {
                    continue;
                }
                let forest = !hills || (woods && (cover == Cover::Woodland || self.rnd(s_i as i32, k as i32, 0xbd03) < 0.4));
                let kind = if forest { DecorKind::BackdropForest } else { DecorKind::BackdropMountains };
                let Some(v) = self.pick(kind, &[], self.rnd(s_i as i32, k as i32, 0xbd04)) else { continue };
                let size = self.variant(kind, v).size();
                let s = self.nominal(kind, v).x * (0.8 + 0.35 * self.rnd(s_i as i32, k as i32, 0xbd05));
                let half = Vec2::new(size.x, size.z) * s * 0.5;
                let p = along + outward * (far - half.y);
                let axis = (b - a).normalize_or(Vec2::X);
                let pts = footprint(kind, p, axis, half);
                if pts.iter().any(|q| self.cover(*q).is_none_or(Cover::is_water) || self.beyond(*q) < 30.0 || self.beyond(*q) > far + 0.5) {
                    continue;
                }
                let base = self.min_ground(&pts) - 0.6;
                self.put(kind, v, Vec3::new(p.x, base, p.y), yaw_facing(-outward), Vec3::splat(s), false);
            }
        }
    }

    // --- small scenery ---------------------------------------------------------

    /// Mossy boulders at the feet of mountains (and the skirt's woods),
    /// shrubs on highland and thicket edges and in the skirt.
    fn boulders_and_shrubs(&mut self) {
        let f = self.f;
        let edge = |p: &Planner, q: Vec2, of: Cover| [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y].iter().any(|d| p.cover(q + *d * f.cell).is_some_and(|c| c != of && c.is_land()));
        if self.cat.has(DecorKind::MossyBoulder) {
            for (p, i, j) in self.lattice(5.5, 0xb0b1) {
                let Some(c) = self.idx(p) else { continue };
                let roll = self.rnd(i, j, 0xb0b2);
                let here = match f.cover[c] {
                    Cover::Mountain => f.rock[c] <= 2.2 && roll < 0.3 && !self.in_spire(p, 5.0) && !f.in_site(p),
                    Cover::Woodland => f.wood[c] <= 2.5 && roll < 0.2,
                    _ => false,
                };
                if here {
                    self.small(DecorKind::MossyBoulder, p, i, j, Ground::Solid, |pl, i, q| matches!(pl.f.cover[i], Cover::Mountain) || !pl.on_map(q), true);
                }
            }
        }
        if self.cat.has(DecorKind::Shrub) {
            for (p, i, j) in self.lattice(4.3, 0x5b81) {
                let Some(c) = self.idx(p) else { continue };
                let roll = self.rnd(i, j, 0x5b82);
                let here = match f.cover[c] {
                    Cover::Highland => !f.in_site(p) && edge(self, p, Cover::Highland) && roll < 0.4,
                    Cover::Thicket => edge(self, p, Cover::Thicket) && roll < 0.14,
                    Cover::Woodland => f.wood[c] <= 2.0 && roll < 0.15,
                    Cover::Meadow => !self.on_map(p) && roll < 0.015,
                    _ => false,
                };
                if here {
                    self.small(DecorKind::Shrub, p, i, j, Ground::Soft, |pl, i, q| !pl.on_map(q) || matches!(pl.f.cover[i], Cover::Highland | Cover::Thicket | Cover::Marsh), true);
                }
            }
        }
    }

    /// Bramble models over the thickets, ferns in them, in marsh and at the
    /// skirt's woods' edges.
    fn thickets(&mut self) {
        let f = self.f;
        if self.cat.has(DecorKind::Bramble) {
            for (p, i, j) in self.lattice(2.3, 0xb4a1) {
                let Some(c) = self.idx(p) else { continue };
                if f.cover[c] == Cover::Thicket && self.rnd(i, j, 0xb4a2) < 0.8 {
                    self.small(DecorKind::Bramble, p, i, j, Ground::Soft, |pl, i, _| matches!(pl.f.cover[i], Cover::Thicket | Cover::Marsh | Cover::Highland), false);
                }
            }
        }
        if self.cat.has(DecorKind::Fern) {
            for (p, i, j) in self.lattice(3.1, 0xfe01) {
                let Some(c) = self.idx(p) else { continue };
                let roll = self.rnd(i, j, 0xfe02);
                let here = match f.cover[c] {
                    Cover::Thicket => roll < 0.2,
                    Cover::Marsh => roll < 0.06,
                    Cover::Woodland => f.wood[c] <= 3.0 && roll < 0.15,
                    _ => false,
                };
                if here {
                    self.small(DecorKind::Fern, p, i, j, Ground::Soft, |pl, i, q| !pl.on_map(q) || matches!(pl.f.cover[i], Cover::Thicket | Cover::Marsh), false);
                }
            }
        }
    }

    /// Reeds on shores and in marsh, river rocks in fords and river
    /// shallows, lily pads on still lake water near the shore.
    fn wet_ground(&mut self) {
        let f = self.f;
        if self.cat.has(DecorKind::Reeds) {
            for (p, i, j) in self.lattice(2.4, 0x4ee5) {
                let Some(c) = self.idx(p) else { continue };
                if f.in_site(p) {
                    continue;
                }
                let roll = self.rnd(i, j, 0x4ee6);
                let here = match f.cover[c] {
                    Cover::Marsh => roll < 0.45,
                    Cover::Shallows => f.shore[c] <= 1.6 && self.sea[c] > 12.0 && roll < 0.55,
                    _ => false,
                };
                if here {
                    let wet = f.cover[c] == Cover::Shallows;
                    self.wet(DecorKind::Reeds, p, i, j, if wet { 1.15 } else { 1.0 }, |pl, i| matches!(pl.f.cover[i], Cover::Marsh | Cover::Shallows | Cover::Thicket));
                }
            }
        }
        if self.cat.has(DecorKind::RiverRocks) {
            for (p, i, j) in self.lattice(3.6, 0x7a01) {
                let Some(c) = self.idx(p) else { continue };
                let roll = self.rnd(i, j, 0x7a02);
                let here = !f.in_site(p)
                    && match f.cover[c] {
                        Cover::Ford => roll < 0.3,
                        Cover::Shallows => f.river[c] >= 0.5 && roll < 0.1,
                        _ => false,
                    };
                if here {
                    self.river_rock(p, i, j);
                }
            }
        }
        if self.cat.has(DecorKind::LilyPads) {
            for (p, i, j) in self.lattice(3.0, 0x1171) {
                let Some(c) = self.idx(p) else { continue };
                let still = matches!(f.cover[c], Cover::Deep | Cover::Shallows) && f.river[c] < 0.25 && self.sea[c] > 15.0;
                let cove = value(p * 0.07, f.seed ^ 0x1172) > 0.42;
                if !still || !cove || !(1.0..=6.0).contains(&f.shore[c]) || f.in_site(p) || self.rnd(i, j, 0x1173) > 0.35 {
                    continue;
                }
                let Some(v) = self.pick(DecorKind::LilyPads, &[], self.rnd(i, j, 0x1174)) else { return };
                let s = self.nominal(DecorKind::LilyPads, v).x * (0.6 + 0.5 * self.rnd(i, j, 0x1175));
                let size = self.variant(DecorKind::LilyPads, v).size();
                let r = s * size.x.max(size.z) * 0.5;
                let pts = samples(p, Vec2::X, Vec2::splat(r));
                if self.fits(&pts, Ground::Soft, |i, _| f.cover[i].is_water() && f.cover[i] != Cover::Ford) {
                    self.put(DecorKind::LilyPads, v, Vec3::new(p.x, WATER_LEVEL + 0.004, p.y), self.rnd(i, j, 0x1176) * TAU, Vec3::splat(s), false);
                }
            }
        }
    }

    /// Beach rocks at the sea's waterline: in the sea's shallows by the
    /// beach, and at the skirt's shore.
    fn waterline(&mut self) {
        const KIND: DecorKind = DecorKind::BeachRocks;
        if !self.cat.has(KIND) {
            return;
        }
        let f = self.f;
        let mut placed = 0;
        for (p, i, j) in self.lattice(6.5, 0xbe01) {
            let Some(c) = self.idx(p) else { continue };
            // In the sea's shallows off the beach (not on the sand: a beach
            // can be built on), and at the skirt's shore.
            let here = match f.cover[c] {
                Cover::Shallows => self.sea[c] <= 10.0 && (1.5..=4.5).contains(&f.shore[c]),
                Cover::Ocean => !self.on_map(p) && f.shore[c] <= 2.5,
                _ => false,
            };
            if !here || self.rnd(i, j, 0xbe02) > 0.4 || placed >= 220 {
                continue;
            }
            let Some(v) = self.pick(KIND, &[], self.rnd(i, j, 0xbe03)) else { return };
            let size = self.variant(KIND, v).size();
            let sxz = self.nominal(KIND, v).x * (0.45 + 0.5 * self.rnd(i, j, 0xbe04));
            let yaw = self.rnd(i, j, 0xbe05) * TAU;
            let half = Vec2::new(size.x, size.z) * sxz * 0.5;
            let pts = samples(p, unit_angle(-yaw), half);
            if !self.fits(&pts, Ground::Soft, |i, _| f.cover[i].is_water()) {
                continue;
            }
            let base = self.min_ground(&pts) - 0.05;
            let top = WATER_LEVEL + 0.2 + 0.25 * self.rnd(i, j, 0xbe06);
            let sy = ((top - base) / size.y).clamp(sxz * 0.4, sxz * 3.0);
            self.put(KIND, v, Vec3::new(p.x, base, p.y), yaw, Vec3::new(sxz, sy, sxz), false);
            placed += 1;
        }
    }

    /// A small piece at its drawn size (give or take a quarter), standing on
    /// the ground, if its footprint fits.
    #[allow(clippy::too_many_arguments)]
    fn small(&mut self, kind: DecorKind, p: Vec2, i: i32, j: i32, ground: Ground, ok: impl Fn(&Planner, usize, Vec2) -> bool, keep_clear: bool) {
        let Some(v) = self.pick(kind, &[], self.rnd(i, j, 0x3a11 ^ kind.index() as u32)) else { return };
        let size = self.variant(kind, v).size();
        let s = self.nominal(kind, v) * (0.75 + 0.45 * self.rnd(i, j, 0x3a12));
        let yaw = self.rnd(i, j, 0x3a13) * TAU;
        let half = Vec2::new(size.x * s.x, size.z * s.z) * 0.5;
        let pts = footprint(kind, p, unit_angle(-yaw), half * kind.foot());
        if (keep_clear && !self.clear(p, half.max_element(), 0.1)) || !self.fits(&pts, ground, |i, q| ok(self, i, q)) {
            return;
        }
        let base = self.min_ground(&pts) - 0.06;
        self.put(kind, v, Vec3::new(p.x, base, p.y), yaw, s, false);
        if keep_clear {
            self.taken.push((p, half.max_element()));
        }
    }

    /// Reeds rising from marsh or a shallow bed (`lift` scales them up
    /// where they stand in water).
    fn wet(&mut self, kind: DecorKind, p: Vec2, i: i32, j: i32, lift: f32, ok: impl Fn(&Planner, usize) -> bool) {
        let Some(v) = self.pick(kind, &[], self.rnd(i, j, 0x3a21)) else { return };
        let size = self.variant(kind, v).size();
        let s = self.nominal(kind, v) * (0.75 + 0.45 * self.rnd(i, j, 0x3a22)) * lift;
        let half = Vec2::new(size.x * s.x, size.z * s.z) * 0.5;
        // Round (any turn): the circle round its foot.
        let pts = samples(p, Vec2::X, Vec2::splat(half.max_element() * kind.foot()));
        if !self.fits(&pts, Ground::Soft, |i, _| ok(self, i)) {
            return;
        }
        let base = self.ground(p) - 0.05;
        self.put(kind, v, Vec3::new(p.x, base, p.y), self.rnd(i, j, 0x3a23) * TAU, s, false);
    }

    /// Smooth stones half sunk in a ford or river shallows, just breaking
    /// the surface.
    fn river_rock(&mut self, p: Vec2, i: i32, j: i32) {
        const KIND: DecorKind = DecorKind::RiverRocks;
        let f = self.f;
        let Some(v) = self.pick(KIND, &[], self.rnd(i, j, 0x7a03)) else { return };
        let size = self.variant(KIND, v).size();
        let sxz = self.nominal(KIND, v).x * (0.7 + 0.5 * self.rnd(i, j, 0x7a04));
        let yaw = self.rnd(i, j, 0x7a05) * TAU;
        let half = Vec2::new(size.x, size.z) * sxz * 0.5;
        let pts = samples(p, unit_angle(-yaw), half);
        if !self.fits(&pts, Ground::Soft, |i, _| f.cover[i].is_water() && f.cover[i] != Cover::Ocean) {
            return;
        }
        let base = self.min_ground(&pts) - 0.05;
        let top = WATER_LEVEL + 0.08 + 0.16 * self.rnd(i, j, 0x7a06);
        let sy = ((top - base) / size.y).clamp(sxz * 0.4, sxz * 2.5);
        self.put(KIND, v, Vec3::new(p.x, base, p.y), yaw, Vec3::new(sxz, sy, sxz), false);
    }
}

/// Most decor a map may carry (it is drawn every frame, so its count is
/// bounded whatever the map).
pub const MAX_DECOR: usize = 9000;

/// The rules a plan must keep, checked placement by placement against the
/// field (for tests and tools): each footprint (a long piece's rectangle,
/// a round one's foot) off buildable ground; only low wade-through pieces
/// (and the rims' pieces, on the rims) where units walk; landmarks only
/// beyond the map's edge; nothing above [`DECOR_TOP`] on the map; nothing
/// past the faded skirt (the backdrops at most its far edge); at most
/// [`MAX_DECOR`] in all. Returns what is wrong, one line each.
pub fn violations(f: &Field, catalogue: &Catalogue, placed: &[Placed]) -> Vec<String> {
    let mut out = Vec::new();
    let lo = f.world_corner(f.skirt, f.skirt);
    let hi = f.world_corner(f.skirt + f.map_cols, f.skirt + f.map_rows);
    let on_map = |p: Vec2| p.x >= lo.x && p.y >= lo.y && p.x < hi.x && p.y < hi.y;
    if placed.len() > MAX_DECOR {
        out.push(format!("{} pieces of decor (at most {MAX_DECOR})", placed.len()));
    }
    for p in placed {
        let Some(v) = catalogue.get(p.kind).and_then(|m| m.variants.get(p.variant)) else {
            out.push(format!("{:?} variant {} is not in the catalogue", p.kind, p.variant));
            continue;
        };
        // Its footprint as built, transformed: the corners and edge
        // midpoints of a long piece's rectangle, the rim of a round one's
        // ellipse (at its foot), a little inside, and the centre.
        let (mn, mx) = (v.min, v.max);
        let (mid, half) = ((mn + mx) * 0.5, (mx - mn) * 0.5 * 0.9);
        let mut local: Vec<Vec2> = vec![Vec2::ZERO];
        if p.kind.long() {
            local.extend([(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)].map(|(u, w)| Vec2::new(u, w)));
        } else {
            local.extend((0..16).map(|k| unit_angle(k as f32 / 16.0 * TAU) * p.kind.foot()));
        }
        for l in local {
            let q = p.transform.transform_point(Vec3::new(mid.x + l.x * half.x, mn.y, mid.z + l.y * half.z)).xz();
            if q.distance(p.at.xz()) > p.radius + 0.5 {
                out.push(format!("{:?} at {} reaches {q}, past its radius {}", p.kind, p.at, p.radius));
            }
            if !on_map(q) {
                let reach = if matches!(p.kind, DecorKind::BackdropMountains | DecorKind::BackdropForest) { f.skirt as f32 * f.cell } else { SKIRT_REACH + 1.0 };
                if f.beyond_map(q) > reach {
                    out.push(format!("{:?} at {} reaches {q}, {} m beyond the map", p.kind, p.at, f.beyond_map(q)));
                }
                continue;
            }
            let c = f.cell_of(q);
            let i = f.index(c.x, c.y);
            let solid = matches!(f.cover[i], Cover::Mountain | Cover::Deep | Cover::Ocean);
            if f.buildable[i] {
                out.push(format!("{:?} at {} covers buildable ground at {q}", p.kind, p.at));
            } else if p.kind.on_rims() && !solid && f.rim[i] <= 0.0 {
                out.push(format!("{:?} at {} leaves the rims at {q} ({:?})", p.kind, p.at, f.cover[i]));
            } else if !p.kind.on_rims() && !p.kind.wadeable() && !solid {
                out.push(format!("{:?} at {} stands on {:?} at {q}, where units walk", p.kind, p.at, f.cover[i]));
            }
        }
        if p.kind.skirt_only() && (on_map(p.at.xz()) || f.beyond_map(p.at.xz()) <= 2.0) {
            out.push(format!("{:?} on the map at {}", p.kind, p.at));
        }
        if p.kind == DecorKind::CraterRim {
            let c = f.cell_of(p.at.xz());
            if f.rim[f.index(c.x, c.y)] <= 0.0 {
                out.push(format!("a rim arc off the rim at {}", p.at));
            }
        }
        if on_map(p.at.xz()) && p.top(catalogue) > DECOR_TOP + 1e-3 {
            out.push(format!("{:?} at {} rises to {} m", p.kind, p.at, p.top(catalogue)));
        }
        if !(p.transform.is_finite() && p.transform.scale.min_element() > 0.0) {
            out.push(format!("{:?} at {}: transform {:?}", p.kind, p.at, p.transform));
        }
    }
    out
}

/// The point `s` metres along a polyline and its direction there.
fn along(points: &[Vec2], s: f32) -> (Vec2, Vec2) {
    let mut left = s;
    for w in points.windows(2) {
        let d = w[1] - w[0];
        let len = d.length();
        if left <= len || len <= 0.0 {
            let t = if len > 0.0 { left / len } else { 0.0 };
            return (w[0] + d * t.clamp(0.0, 1.0), d.normalize_or(Vec2::X));
        }
        left -= len;
    }
    let n = points.len();
    (points[n - 1], (points[n - 1] - points[n - 2]).normalize_or(Vec2::X))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::field::FieldSource;
    use crate::terrain::field::tests::{painted, painted_features};
    use af_core::ext::map::{MapEdge, MeteorSite, MountainRange, ResolvedMapOptions, RiverLine, MapSize, WaterLevel};
    use af_core::prelude::*;
    use af_core::testing::{new_nav, test_data};

    #[test]
    fn the_table_follows_the_enum_and_round_trips() {
        for (i, kind) in DecorKind::ALL.into_iter().enumerate() {
            assert_eq!(kind.index(), i);
            assert_eq!(DecorKind::parse(kind.id()), Some(kind));
            assert!(kind.size().min_element() > 0.0);
        }
        assert_eq!(DecorKind::parse("castle"), None);
        assert_eq!(DecorKind::MountainPeak.variant_names().collect::<Vec<_>>(), ["a", "b", "c", "d"]);
    }

    /// A 192 m map with every kind of ground decor goes on: a mountain
    /// range with a spine, a rimmed crater, a river with a ford leaving the
    /// map north and south, a lake with marsh, a thicket, a highland, and
    /// the sea along the west edge.
    fn rich_map(data: &GameData) -> (NavGrid, MapFeatures) {
        let n = 192u32;
        let t = |id: &str| data.terrain_id(id).unwrap_or_else(|| panic!("{id}")).0;
        let mut nav = new_nav(data, n, n, data.terrain_id("grass").unwrap());
        let crater = Vec2::new(120.0, 120.0);
        for z in 0..n as i32 {
            for x in 0..n as i32 {
                let p = Vec2::new(x as f32 + 0.5, z as f32 + 0.5);
                let to_crater = p.distance(crater);
                let gap = (p - crater).normalize_or(Vec2::X).dot(Vec2::new(0.0, -1.0)) > 0.97;
                let id = if p.x < 26.0 {
                    Some(t("ocean"))
                } else if p.x < 31.0 {
                    Some(t("shallow_water"))
                } else if p.x < 34.0 {
                    Some(t("sand"))
                } else if (p.y - 50.0).abs() < 9.0 - (p.x - 100.0).abs() * 0.06 && (55.0..145.0).contains(&p.x) {
                    Some(t("mountain"))
                } else if (p.x - 170.0).abs() < 2.5 {
                    Some(if (p.y - 100.0).abs() < 6.0 { t("ford") } else { t("deep_water") })
                } else if (p.x - 170.0).abs() < 4.5 {
                    Some(t("shallow_water"))
                } else if (20.0..25.0).contains(&to_crater) && !gap {
                    Some(t("cliff"))
                } else if to_crater < 20.0 {
                    Some(t("scorched"))
                } else if p.distance(Vec2::new(60.0, 150.0)) < 11.0 {
                    Some(t("deep_water"))
                } else if p.distance(Vec2::new(60.0, 150.0)) < 14.0 {
                    Some(t("shallow_water"))
                } else if p.distance(Vec2::new(60.0, 150.0)) < 18.0 {
                    Some(t("marsh"))
                } else if p.distance(Vec2::new(60.0, 100.0)) < 8.0 {
                    Some(t("thicket"))
                } else if p.distance(Vec2::new(150.0, 160.0)) < 9.0 {
                    Some(t("cliff"))
                } else {
                    None
                };
                if let Some(id) = id {
                    nav.set_terrain(IVec2::new(x, z), id);
                }
            }
        }
        let features = MapFeatures {
            options: ResolvedMapOptions { size: MapSize::Medium, water: WaterLevel::Coast, rivers: 1, site: SiteKind::Crater },
            land_min: Vec2::new(26.0, 0.0),
            land_max: Vec2::splat(192.0),
            ocean_edges: vec![MapEdge::West],
            sites: vec![MeteorSite { kind: SiteKind::Crater, center: crater, radius: 22.5 }],
            rivers: vec![RiverLine { points: (0..280).map(|i| Vec2::new(170.0, 196.0 - i as f32 * 0.75)).collect(), half_widths: vec![2.5; 280], fords: vec![128] }],
            mountains: vec![MountainRange { spine: vec![Vec2::new(57.0, 50.0), Vec2::new(100.0, 50.0), Vec2::new(143.0, 50.0)], half_widths: vec![6.0, 9.0, 6.0] }],
        };
        (nav, features)
    }

    fn rich_field(data: &GameData) -> (Field, MapFeatures) {
        let (nav, features) = rich_map(data);
        let field = Field::build(&FieldSource { data, nav: &nav, seed: 11, features: Some(&features), crater: None });
        (field, features)
    }

    #[test]
    fn decor_never_stands_on_buildable_ground_or_where_units_walk() {
        let data = test_data();
        let (f, features) = rich_field(&data);
        let cat = Catalogue::full();
        let placed = plan(&f, Some(&features), &cat);
        let count = |k: DecorKind| placed.iter().filter(|p| p.kind == k).count();
        for kind in [
            DecorKind::MountainPeak,
            DecorKind::MountainRidge,
            DecorKind::MountainFoothill,
            DecorKind::CraterRim,
            DecorKind::Reeds,
            DecorKind::RiverRocks,
            DecorKind::Bramble,
            DecorKind::SeaStack,
            DecorKind::BeachRocks,
            DecorKind::StoneBridge,
            DecorKind::BackdropMountains,
        ] {
            assert!(count(kind) > 0, "no {kind:?} on the rich map: {:?}", DecorKind::ALL.map(|k| (k, count(k))));
        }
        let broken = violations(&f, &cat, &placed);
        assert!(broken.is_empty(), "{} broken rules, first: {:#?}", broken.len(), &broken[..broken.len().min(12)]);
        assert!(count(DecorKind::MountainPeak) <= 320 && count(DecorKind::Shipwreck) <= 4);
    }

    #[test]
    fn the_plan_is_the_same_every_time_and_follows_the_catalogue() {
        let data = test_data();
        let (f, features) = rich_field(&data);
        let cat = Catalogue::full();
        let a = plan(&f, Some(&features), &cat);
        let b = plan(&f, Some(&features), &cat);
        assert_eq!(a, b);
        // Nothing to place: nothing placed, and the props stay whole.
        assert!(plan(&f, Some(&features), &Catalogue::default()).is_empty());
        assert_eq!(keep_props(&Catalogue::default()), PropKeep::default());
        assert!(keep_props(&cat).brambles < 1.0 && keep_props(&cat).reeds < 1.0);
        // Only the reeds: only reeds, from the variants there are.
        let mut reeds = Catalogue::default();
        let only_b = DecorVariant { name: "b".into(), min: Vec3::new(-0.4, 0.0, -0.4), max: Vec3::new(0.4, 1.0, 0.4) };
        reeds.insert(DecorKind::Reeds, DecorModel { fit: Fit::Height, size: DecorKind::Reeds.size(), variants: vec![only_b] });
        let r = plan(&f, Some(&features), &reeds);
        assert!(!r.is_empty() && r.iter().all(|p| p.kind == DecorKind::Reeds && p.variant == 0));
        // A model's own size and proportions: drawn 1.5 m tall, give or take.
        for p in &r {
            let h = p.transform.scale.y * 1.0;
            assert!((0.9..=2.4).contains(&h), "reeds {h} m tall");
        }
    }

    #[test]
    fn snow_crowns_the_tallest_peaks() {
        let data = test_data();
        let (f, features) = rich_field(&data);
        let cat = Catalogue::full();
        let peaks: Vec<Placed> = plan(&f, Some(&features), &cat).into_iter().filter(|p| p.kind == DecorKind::MountainPeak).collect();
        assert!(peaks.len() >= 2, "{}", peaks.len());
        let names = &cat.get(DecorKind::MountainPeak).unwrap().variants;
        for p in &peaks {
            let snowy = matches!(names[p.variant].name.as_str(), "a" | "b");
            let top = p.top(&cat);
            assert!(!snowy || top >= 9.3 - 1e-3, "snow on a {top} m peak");
            assert!(top > 6.0, "a peak {top} m high");
        }
    }

    #[test]
    fn without_features_only_what_the_field_shows_is_placed() {
        // The painted test map (a range, a river, a lake, a thicket...) and
        // no MapFeatures: peaks, reeds and brambles still come; ridges
        // (which follow spines) and bridges (rivers' lines) do not.
        let data = test_data();
        let nav = painted(&data, 128);
        let f = Field::build(&FieldSource { data: &data, nav: &nav, seed: 3, features: None, crater: None });
        let placed = plan(&f, None, &Catalogue::full());
        assert!(placed.iter().any(|p| p.kind == DecorKind::Bramble) && placed.iter().any(|p| p.kind == DecorKind::Reeds));
        assert!(!placed.iter().any(|p| matches!(p.kind, DecorKind::MountainRidge | DecorKind::StoneBridge | DecorKind::WoodenFootbridge)));
        // With them, the Starspire's footing gets its monoliths.
        let features = painted_features();
        let f = Field::build(&FieldSource { data: &data, nav: &nav, seed: 3, features: Some(&features), crater: None });
        let placed = plan(&f, Some(&features), &Catalogue::full());
        let (c, r) = f.spire.unwrap();
        let monoliths: Vec<&Placed> = placed.iter().filter(|p| p.kind == DecorKind::AetherMonolith).collect();
        assert!(!monoliths.is_empty() && monoliths.iter().all(|p| p.at.xz().distance(c) < r), "{monoliths:?}");
    }
}
