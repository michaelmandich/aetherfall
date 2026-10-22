//! Resource node placement: the overlap bookkeeping, contested spots,
//! highland ore, dune nodes, forests and their thickets (the meteor sites'
//! nodes are in `sites.rs`).

use std::f32::consts::{FRAC_PI_2, TAU};

use af_core::ext::map::SiteKind;

use af_core::prelude::*;
use bevy::prelude::*;

use super::noise::Noise;
use super::starts::Frame;
use super::{grown, perp, polar, Blob, Gen, NodePlacement, MAP_MARGIN};

/// Gap kept around every node (each side), so nodes never touch.
pub(crate) const NODE_PAD: f32 = 0.15;
/// One forest cluster per this many square metres of map.
const AREA_PER_FOREST: f32 = 4000.0;
/// Chance that a scattered forest cluster grows beside another.
const WOOD_CHANCE: f32 = 0.5;
/// Tree spacing inside a forest cluster.
const TREE_SPACING: f32 = 1.9;

/// Where a node may go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Zone {
    /// Anywhere suitable: deliberate layout placements.
    Any,
    /// Only on cells outside the reserved zones: random scatter.
    Free,
}

/// Every box placed so far, bucketed on a coarse grid for overlap tests.
pub(crate) struct Occupancy {
    cell: f32,
    side: i32,
    buckets: Vec<Vec<u32>>,
    boxes: Vec<Obb>,
}

impl Occupancy {
    pub fn new(size: f32) -> Occupancy {
        let cell = 4.0;
        let side = (size / cell).ceil().max(1.0) as i32;
        Occupancy { cell, side, buckets: vec![Vec::new(); (side * side) as usize], boxes: Vec::new() }
    }

    fn bucket_range(&self, obb: &Obb) -> (i32, i32, i32, i32) {
        let (min, max) = obb.aabb();
        let clamp = |v: f32| ((v / self.cell).floor() as i32).clamp(0, self.side - 1);
        (clamp(min.x), clamp(min.y), clamp(max.x), clamp(max.y))
    }

    pub fn overlaps(&self, obb: &Obb) -> bool {
        let (x0, y0, x1, y1) = self.bucket_range(obb);
        (y0..=y1).any(|y| {
            (x0..=x1).any(|x| self.buckets[(y * self.side + x) as usize].iter().any(|i| self.boxes[*i as usize].overlaps(obb)))
        })
    }

    pub fn insert(&mut self, obb: Obb) {
        let i = self.boxes.len() as u32;
        let (x0, y0, x1, y1) = self.bucket_range(&obb);
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.buckets[(y * self.side + x) as usize].push(i);
            }
        }
        self.boxes.push(obb);
    }
}

/// Offsets of `count` points of a square lattice of `spacing` aligned with
/// `yaw` (so equal-yaw boxes on it line up face to face), nearest the
/// origin first with a little randomness for ragged clusters. The jitter is
/// small enough that up to five points always form a compact plus shape
/// (no point further than `spacing` from the centre).
pub(crate) fn lattice(count: u32, spacing: f32, yaw: f32, rng: &mut SimRng) -> Vec<Vec2> {
    let (right, forward) = (af_geom::right(yaw), af_geom::forward(yaw));
    let reach = (count as f32).sqrt().ceil() as i32;
    let mut points: Vec<(f32, Vec2)> = Vec::new();
    for j in -reach..=reach {
        for i in -reach..=reach {
            let p = (right * i as f32 + forward * j as f32) * spacing;
            points.push((p.length() + rng.range(0.0, 0.3) * spacing, p));
        }
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points.into_iter().take(count as usize).map(|(_, p)| p).collect()
}

/// Lattice spacing that keeps equal-yaw nodes of type `kind` apart.
pub(crate) fn cluster_spacing(data: &GameData, kind: NodeTypeId) -> f32 {
    let size = data.node(kind).size;
    size[0].max(size[2]) + 2.0 * NODE_PAD + 0.3
}

/// Nudge offsets tried around a wanted position, nearest first.
pub(crate) fn nudges() -> impl Iterator<Item = Vec2> {
    std::iter::once(Vec2::ZERO).chain((1..=4).flat_map(|ring| {
        (0..8).map(move |k| polar(k as f32 * TAU / 8.0 + ring as f32 * 0.39) * (ring as f32 * 0.6))
    }))
}

impl Gen<'_> {
    /// The box to reserve if a node of `kind` fits at `pos`/`yaw`: inside the
    /// map, on buildable terrain (respecting `zone`), out of every corridor,
    /// and clear of everything placed. Nodes that a building must claim (an
    /// aether vent) reserve that building's footprint too.
    pub(crate) fn node_fits(&self, kind: NodeTypeId, pos: Vec2, yaw: f32, zone: Zone) -> Option<Obb> {
        let def = self.data.node(kind);
        let node = Obb::from_size(pos, def.size, yaw);
        let footprint = match def.requires_building_id {
            Some(b) => {
                let s = self.data.building(b).size;
                Obb { half: node.half.max(Vec2::new(s[0], s[2]) * 0.5), ..node }
            }
            None => node,
        };
        if !self.inside(&footprint, MAP_MARGIN) || self.hits_corridor(&footprint) {
            return None;
        }
        let terrain_ok = self.all_covered(&footprint, |i| {
            self.buildable[self.terrain[i].index()] && (zone == Zone::Any || !self.reserved(i))
        });
        let reserve = grown(footprint, NODE_PAD);
        (terrain_ok && !self.occ.overlaps(&reserve)).then_some(reserve)
    }

    /// Place a node if it fits exactly here.
    pub(crate) fn place_node(&mut self, kind: Option<NodeTypeId>, pos: Vec2, yaw: f32, zone: Zone) -> bool {
        let Some(kind) = kind else { return false };
        let Some(reserve) = self.node_fits(kind, pos, yaw, zone) else { return false };
        self.occ.insert(reserve);
        self.nodes.push(NodePlacement { kind, pos, yaw });
        true
    }

    /// Place a node at `pos` or the nearest spot within ~2.4 m that fits.
    pub(crate) fn place_node_near(&mut self, kind: Option<NodeTypeId>, pos: Vec2, yaw: f32, zone: Zone) -> bool {
        nudges().any(|off| self.place_node(kind, pos + off, yaw, zone))
    }

    /// [`Gen::place_node_near`], trying only spots that pass `ok`.
    pub(crate) fn place_node_where(&mut self, kind: Option<NodeTypeId>, pos: Vec2, yaw: f32, zone: Zone, ok: impl Fn(Vec2) -> bool) -> bool {
        nudges().any(|off| ok(pos + off) && self.place_node(kind, pos + off, yaw, zone))
    }

    /// A lattice cluster of up to `count` nodes around `center`; returns how
    /// many were placed.
    fn place_cluster(&mut self, kind: Option<NodeTypeId>, center: Vec2, count: u32, zone: Zone, rng: &mut SimRng) -> u32 {
        let Some(k) = kind else { return 0 };
        let yaw = rng.range(0.0, FRAC_PI_2);
        let offsets = lattice(count, cluster_spacing(self.data, k), yaw, rng);
        offsets.into_iter().filter(|off| self.place_node_near(kind, center + *off, yaw, zone)).count() as u32
    }

    /// Between each pair of neighbouring starts: an aether vent, an adamant
    /// meteor and a little iron and copper. Small, open and hard to wall.
    /// (At the Shattered Fall each spot is a small crater, filled with the
    /// site nodes.)
    pub(super) fn place_contested_spots(&mut self, rng: &mut SimRng) {
        if self.layout.options.site == SiteKind::Shattered {
            return;
        }
        let spots: Vec<(Vec2, f32)> = self.layout.spots.iter().copied().zip(self.layout.bisectors.iter().copied()).collect();
        for (spot, angle) in spots {
            let frame = Frame::new(spot, spot + polar(angle));
            self.place_node_near(self.ids.vent, spot, frame.yaw(0.0), Zone::Any);
            let meteor = frame.point(Vec2::new(7.0, rng.range(-2.0, 2.0)));
            self.place_node_near(self.ids.meteor, meteor, rng.range(0.0, FRAC_PI_2), Zone::Any);
            self.place_cluster(self.ids.iron, frame.point(Vec2::new(-1.5, 6.5)), 2, Zone::Any, rng);
            let copper = 1 + rng.below(2);
            self.place_cluster(self.ids.copper, frame.point(Vec2::new(-1.5, -6.5)), copper, Zone::Any, rng);
        }
    }

    /// Iron and copper on top of each cliff highland (the top is reserved,
    /// so these are layout placements).
    pub(super) fn place_highland_ore(&mut self, rng: &mut SimRng) {
        for h in self.highlands.clone() {
            let dir = polar(rng.range(0.0, TAU)) * (h.top() * 0.35);
            self.place_node_near(self.ids.iron, h.center + dir, rng.range(0.0, FRAC_PI_2), Zone::Any);
            self.place_node_near(self.ids.copper, h.center - dir, rng.range(0.0, FRAC_PI_2), Zone::Any);
        }
    }

    /// Palms and quartz sand scattered over each dune region.
    pub(super) fn place_sand_nodes(&mut self, rng: &mut SimRng) {
        for region in self.sand_regions.clone() {
            let palms = 6 + rng.below(7);
            let silicon = 2 + rng.below(2);
            for (kind, count) in [(self.ids.silicon, silicon), (self.ids.palm, palms)] {
                for _ in 0..count {
                    self.scatter_on_sand(kind, region, rng);
                }
            }
        }
    }

    fn scatter_on_sand(&mut self, kind: Option<NodeTypeId>, region: Blob, rng: &mut SimRng) {
        for _ in 0..12 {
            let p = region.center + rng.in_disc(region.radius * 0.85);
            let on_sand = self.idx_at(p).is_some_and(|i| self.terrain[i] == self.ids.sand);
            if on_sand && self.place_node(kind, p, rng.range(0.0, FRAC_PI_2), Zone::Free) {
                return;
            }
        }
    }

    /// Oak/pine clusters: two framing each start area, the rest scattered as
    /// soft walls, about half of them beside another so they gather into
    /// woods. Every tree is its own node. Half the scattered clusters grow
    /// bramble thicket between their trees.
    pub(super) fn place_forests(&mut self, rng: &mut SimRng) {
        let mut clusters: Vec<Blob> = Vec::new();
        for i in 0..self.layout.starts.len() {
            let frame = self.layout.frame(i);
            for side in [-1.0, 1.0] {
                let radius = rng.range(6.0, 10.0);
                let angle = side * rng.range(100.0f32, 150.0).to_radians();
                let dist = super::START_AREA_RADIUS + radius * 0.6 + rng.range(0.0, 6.0);
                clusters.push(Blob { center: frame.point(polar(angle) * dist), radius });
            }
        }
        let framing = clusters.len();
        let target = ((self.size * self.size / AREA_PER_FOREST).round() as usize).max(framing + 4);
        for _ in 0..target * 8 {
            if clusters.len() >= target {
                break;
            }
            let radius = rng.range(6.0, 12.0);
            // Some clusters grow beside one already there, so the scatter
            // gathers into woods with glades between.
            let p = if clusters.len() > framing && rng.chance(WOOD_CHANCE) {
                let c = clusters[framing + rng.below((clusters.len() - framing) as u32) as usize];
                c.center + polar(rng.range(0.0, TAU)) * (c.radius + radius + rng.range(1.0, 5.0))
            } else {
                Vec2::new(rng.range(4.0, self.size - 4.0), rng.range(4.0, self.size - 4.0))
            };
            let open = self.idx_at(p).is_some_and(|i| !self.reserved(i) && self.terrain[i] == self.ids.grass);
            if open && clusters.iter().all(|c| c.center.distance(p) > c.radius + radius) {
                clusters.push(Blob { center: p, radius });
            }
        }

        let shape = Noise::new(rng, self.size, 7.0);
        let mut thickets = Vec::new();
        for (n, cluster) in clusters.iter().enumerate() {
            self.grow_forest(*cluster, &shape, rng);
            if n >= framing && rng.chance(0.5) {
                thickets.push(*cluster);
            }
        }
        self.paint_thickets(&thickets, rng);
    }

    fn grow_forest(&mut self, cluster: Blob, shape: &Noise, rng: &mut SimRng) {
        let oak_share = rng.range(0.2, 0.8);
        let right = polar(rng.range(0.0, FRAC_PI_2));
        let forward = perp(right);
        let reach = (cluster.radius * 1.3 / TREE_SPACING).ceil() as i32;
        for j in -reach..=reach {
            for i in -reach..=reach {
                let jitter = Vec2::new(rng.range(-0.25, 0.25), rng.range(-0.25, 0.25));
                let p = cluster.center + (right * i as f32 + forward * j as f32) * TREE_SPACING + jitter;
                let edge = cluster.radius * (0.7 + 0.6 * shape.at(p));
                if p.distance(cluster.center) > edge {
                    continue;
                }
                let kind = if rng.chance(oak_share) { self.ids.oak } else { self.ids.pine };
                self.place_node(kind, p, rng.range(0.0, FRAC_PI_2), Zone::Free);
            }
        }
    }

    /// Bramble thicket on open grass among the trees of `clusters` (never
    /// under a node: nodes only stand on buildable ground).
    fn paint_thickets(&mut self, clusters: &[Blob], rng: &mut SimRng) {
        let mut covered = vec![false; self.terrain.len()];
        for n in &self.nodes {
            let obb = Obb::from_size(n.pos, self.data.node(n.kind).size, n.yaw);
            self.for_covered(&obb, |i| covered[i] = true);
        }
        let noise = Noise::new(rng, self.size, 4.0);
        let (grass, thicket) = (self.ids.grass, self.ids.thicket);
        for c in clusters {
            self.paint_disc(c.center, c.radius + 2.0, |cell| {
                (!cell.reserved && cell.terrain == grass && !covered[cell.index] && noise.at(cell.center) > 0.55).then_some(thicket)
            });
        }
    }
}
