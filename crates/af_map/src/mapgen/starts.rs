//! Start areas: the shared resource template, main halls, extra start
//! buildings and start units.

use std::f32::consts::FRAC_PI_2;

use af_core::prelude::*;
use bevy::prelude::*;

use super::nodes::{cluster_spacing, lattice, nudges, Zone, NODE_PAD};
use super::{grown, polar, BuildingPlacement, Gen, Ids, NodePlacement, UnitPlacement, MAP_MARGIN, START_AREA_RADIUS};

/// Gap kept around start buildings so workers can walk around them.
const BUILDING_PAD: f32 = 1.5;
/// Gap kept around start units.
const UNIT_PAD: f32 = 0.2;
/// Template nodes are validated with a slightly bigger gap than the world
/// check, so rotating the template never creates an overlap.
const TEMPLATE_PAD: f32 = NODE_PAD + 0.05;
/// Template nodes keep this far from the start (the hall and its units).
const TEMPLATE_KEEP_OUT: f32 = 8.0;
/// Inner and outer radius of the forest edge behind each start.
const FOREST_EDGE: (f32, f32) = (20.5, 28.5);
/// The forest edge covers local angles beyond this (135 degrees) on both
/// sides, i.e. the back of the start.
const FOREST_EDGE_FROM: f32 = 135.0 * std::f32::consts::PI / 180.0;
/// The start's aether cluster stops this far short of the main site's rim.
pub(crate) const AETHER_RIM_CLEARANCE: f32 = 5.0;
/// ...and sits this far to one side of the corridor.
pub(crate) const AETHER_LATERAL: f32 = 9.0;

/// A local frame at `origin`: x points at a target, y to its left.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    origin: Vec2,
    x: Vec2,
    y: Vec2,
    /// Math angle of `x`.
    angle: f32,
}

impl Frame {
    pub fn new(origin: Vec2, toward: Vec2) -> Frame {
        let x = (toward - origin).normalize_or(Vec2::X);
        Frame { origin, x, y: super::perp(x), angle: super::angle_of(x) }
    }

    /// World position of a local point.
    pub fn point(&self, local: Vec2) -> Vec2 {
        self.origin + self.x * local.x + self.y * local.y
    }

    /// World direction of a local direction.
    pub fn dir(&self, local: Vec2) -> Vec2 {
        self.x * local.x + self.y * local.y
    }

    /// World yaw of a box with yaw `local` in this frame. Rotating the frame
    /// by `angle` counter-clockwise turns a box's yaw by `-angle` (yaw turns
    /// the other way; see `af_geom::right`).
    pub fn yaw(&self, local: f32) -> f32 {
        af_geom::wrap_angle(local - self.angle)
    }

    /// Yaw whose forward axis is this frame's x (facing the target).
    pub fn facing(&self) -> f32 {
        af_geom::yaw_of(self.x)
    }
}

/// One node of the start template, in a start's local frame.
#[derive(Clone, Copy, Debug)]
struct TemplateNode {
    kind: NodeTypeId,
    local: Vec2,
    yaw: f32,
}

/// The resources around every start, generated once per map and rotated
/// onto each start, so all players get identical distances.
pub(crate) struct StartTemplate {
    nodes: Vec<TemplateNode>,
    /// Local centre and radius of the dune patch under the quartz sand.
    pub sand: (Vec2, f32),
    /// Local centre of the aether crystal cluster.
    pub aether: Vec2,
}

impl StartTemplate {
    /// `to_center` is the start's distance from the map centre and
    /// `site_reach` how far the main meteor site (its rim's outer edge)
    /// reaches toward the starts.
    pub fn new(data: &GameData, ids: &Ids, rng: &mut SimRng, to_center: f32, site_reach: f32) -> StartTemplate {
        let mut t = StartTemplate { nodes: Vec::new(), sand: (Vec2::ZERO, 6.0), aether: Vec2::ZERO };
        let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
        let deg = |d: f32| d.to_radians();
        // Berries close in, stone and metals a little further out on the
        // flanks, wood behind, quartz sand further back, and the first aether
        // toward the crater (beside the corridor, never in it).
        t.cluster(data, ids.berry, polar(side * deg(rng.range(60.0, 80.0))) * 12.0, 6 + rng.below(3), rng);
        t.cluster(data, ids.stone, polar(-side * deg(rng.range(50.0, 65.0))) * 18.0, 2 + rng.below(2), rng);
        t.cluster(data, ids.copper, polar(side * deg(rng.range(95.0, 105.0))) * 20.0, 2 + rng.below(2), rng);
        t.cluster(data, ids.iron, polar(-side * deg(rng.range(95.0, 105.0))) * 22.0, 2 + rng.below(2), rng);
        t.forest_edge(data, ids, rng);

        let silicon_dir = polar(-side * deg(rng.range(95.0, 115.0)));
        t.sand = (silicon_dir * 38.0, 6.0);
        t.cluster(data, ids.silicon, silicon_dir * 38.0, 2 + rng.below(2), rng);
        t.cluster(data, ids.palm, silicon_dir * 42.5, 2 + rng.below(2), rng);

        // About 45-55 m out on the default map sizes, a few metres short of
        // the main site; closer only on small maps.
        let aether_side = if rng.chance(0.5) { 1.0 } else { -1.0 };
        let dist = (to_center - site_reach - AETHER_RIM_CLEARANCE).clamp(20.0, 55.0);
        let lateral = AETHER_LATERAL * aether_side;
        t.aether = Vec2::new((dist * dist - lateral * lateral).max(0.0).sqrt(), lateral);
        t.cluster(data, ids.crystal, t.aether, 3, rng);
        t
    }

    /// Add `count` nodes on a lattice around `center`, skipping any that
    /// would overlap the template or crowd the hall.
    fn cluster(&mut self, data: &GameData, kind: Option<NodeTypeId>, center: Vec2, count: u32, rng: &mut SimRng) {
        let Some(kind) = kind else { return };
        let yaw = rng.range(0.0, FRAC_PI_2);
        for off in lattice(count, cluster_spacing(data, kind), yaw, rng) {
            self.push(data, TemplateNode { kind, local: center + off, yaw });
        }
    }

    /// 40-80 oaks and pines in a ragged band behind the start.
    fn forest_edge(&mut self, data: &GameData, ids: &Ids, rng: &mut SimRng) {
        let target = 40 + rng.below(41) as usize;
        let oak_share = rng.range(0.3, 0.7);
        let (wave, phase) = (2.0 + rng.below(2) as f32, rng.range(0.0, std::f32::consts::TAU));
        let (inner, outer) = FOREST_EDGE;
        let mut spots = Vec::new();
        let spacing = 1.9;
        let reach = (outer / spacing).ceil() as i32;
        for j in -reach..=reach {
            for i in -reach..=0 {
                let p = Vec2::new(i as f32, j as f32) * spacing + Vec2::new(rng.range(-0.12, 0.12), rng.range(-0.12, 0.12));
                let (r, a) = (p.length(), super::angle_of(p).abs());
                // The band is 5-8 m deep, its outer edge waving with the angle.
                let depth = outer - 3.0 * (0.5 + 0.5 * ops::sin(a * wave + phase));
                if a >= FOREST_EDGE_FROM && (inner..depth).contains(&r) {
                    spots.push(p);
                }
            }
        }
        spots.sort_by(|a, b| a.length().total_cmp(&b.length()));
        let mut placed = 0;
        for p in spots {
            if placed == target {
                break;
            }
            let kind = if rng.chance(oak_share) { ids.oak } else { ids.pine };
            let Some(kind) = kind else { continue };
            if self.push(data, TemplateNode { kind, local: p, yaw: rng.range(0.0, FRAC_PI_2) }) {
                placed += 1;
            }
        }
    }

    /// Local centre and bounding radius of every template node.
    pub fn footprints<'a>(&'a self, data: &'a GameData) -> impl Iterator<Item = (Vec2, f32)> + 'a {
        self.nodes.iter().map(move |n| {
            let s = data.node(n.kind).size;
            (n.local, Vec2::new(s[0], s[2]).length() * 0.5)
        })
    }

    fn push(&mut self, data: &GameData, n: TemplateNode) -> bool {
        let obb = grown(Obb::from_size(n.local, data.node(n.kind).size, n.yaw), TEMPLATE_PAD);
        let clear = n.local.length() >= TEMPLATE_KEEP_OUT
            && self.nodes.iter().all(|o| !grown(Obb::from_size(o.local, data.node(o.kind).size, o.yaw), TEMPLATE_PAD).overlaps(&obb));
        if clear {
            self.nodes.push(n);
        }
        clear
    }
}

impl Gen<'_> {
    /// Each player's first start building (the main hall), complete, at the
    /// start position and facing the map centre.
    pub(super) fn place_halls(&mut self, setup: &MatchSetup) {
        for (i, ps) in setup.players.iter().enumerate() {
            let hall = self.data.factions.get(ps.faction.index()).and_then(|f| f.start.buildings.first()).and_then(|id| self.data.building_id(id));
            let Some(kind) = hall else {
                self.halls.push(None);
                continue;
            };
            let frame = self.layout.frame(i);
            let (pos, yaw) = (self.layout.starts[i], frame.facing());
            let obb = Obb::from_size(pos, self.data.building(kind).size, yaw);
            self.occ.insert(grown(obb, BUILDING_PAD));
            self.buildings.push(BuildingPlacement { player: i, kind, pos, yaw });
            self.halls.push(Some(obb));
        }
    }

    /// The template's resources around every start. Each node goes in at
    /// every start or at none (when a coast, the map edge or a crowded
    /// neighbour leaves one start no room for it), so the starts stay
    /// mirrored.
    pub(super) fn place_start_resources(&mut self) {
        let template: Vec<TemplateNode> = self.layout.template.nodes.clone();
        let frames: Vec<Frame> = (0..self.layout.starts.len()).map(|i| self.layout.frame(i)).collect();
        for n in &template {
            let spots: Option<Vec<(Vec2, f32, Obb)>> = frames
                .iter()
                .map(|f| {
                    let (pos, yaw) = (f.point(n.local), f.yaw(n.yaw));
                    nudges().find_map(|off| self.node_fits(n.kind, pos + off, yaw, Zone::Any).map(|reserve| (pos + off, yaw, reserve)))
                })
                .collect();
            let Some(spots) = spots else { continue };
            // Starts are far apart; only on tiny crowded maps can two
            // starts' copies of a node clash.
            let clash = spots.iter().enumerate().any(|(i, a)| spots[i + 1..].iter().any(|b| a.2.overlaps(&b.2)));
            if clash {
                continue;
            }
            for (pos, yaw, reserve) in spots {
                self.occ.insert(reserve);
                self.nodes.push(NodePlacement { kind: n.kind, pos, yaw });
            }
        }
    }

    /// Any further start buildings, complete, beside or behind the hall.
    pub(super) fn place_start_buildings(&mut self, setup: &MatchSetup) {
        for (i, ps) in setup.players.iter().enumerate() {
            let Some(faction) = self.data.factions.get(ps.faction.index()) else { continue };
            let frame = self.layout.frame(i);
            let hall = self.hall_or_point(i);
            for id in faction.start.buildings.iter().skip(1) {
                let Some(kind) = self.data.building_id(id) else { continue };
                // A building that must claim a node has nowhere to stand here.
                if self.data.building(kind).requires_node_id.is_some() {
                    continue;
                }
                if let Some(pos) = self.find_building_spot(&hall, &frame, kind) {
                    self.buildings.push(BuildingPlacement { player: i, kind, pos, yaw: frame.facing() });
                }
            }
        }
    }

    /// A free spot for a building of `kind` beside or behind `hall`, within
    /// the start area (the frame's origin is the start).
    fn find_building_spot(&mut self, hall: &Obb, frame: &Frame, kind: BuildingTypeId) -> Option<Vec2> {
        let size = self.data.building(kind).size;
        let yaw = frame.facing();
        let shape = Obb::from_size(Vec2::ZERO, size, yaw);
        let start = frame.point(Vec2::ZERO);
        for ring in 0..8 {
            for deg in [90.0f32, -90.0, 125.0, -125.0, 160.0, -160.0, 55.0, -55.0] {
                let dir = frame.dir(polar(deg.to_radians()));
                let pos = hall.center + dir * (hall.support(dir) + shape.support(dir) + 2.5 + ring as f32 * 2.0);
                let obb = Obb::from_size(pos, size, yaw);
                let fits = self.inside(&obb, MAP_MARGIN)
                    && pos.distance(start) + obb.bounding_radius() <= START_AREA_RADIUS
                    && !self.hits_corridor(&obb)
                    && self.all_covered(&obb, |c| self.buildable[self.terrain[c].index()])
                    && !self.occ.overlaps(&grown(obb, BUILDING_PAD));
                if fits {
                    self.occ.insert(grown(obb, BUILDING_PAD));
                    return Some(pos);
                }
            }
        }
        None
    }

    /// The faction's start units in rows in front of the hall, facing the
    /// map centre. A start entry `(unit, n)` is `n` trainings: squad units
    /// come as `n` squadrons.
    pub(super) fn place_start_units(&mut self, setup: &MatchSetup) {
        for (i, ps) in setup.players.iter().enumerate() {
            let Some(faction) = self.data.factions.get(ps.faction.index()) else { continue };
            let frame = self.layout.frame(i);
            let hall = self.hall_or_point(i);
            for (id, count) in &faction.start.units {
                let Some(kind) = self.data.unit_id(id) else { continue };
                let models = self.data.unit(kind).squad.max(1);
                let mobility = self.data.unit_mobility(kind, ps.faction);
                for _ in 0..*count {
                    let squad = (models > 1).then(|| {
                        self.next_squad += 1;
                        self.next_squad
                    });
                    for _ in 0..models {
                        if let Some((pos, yaw)) = self.find_unit_spot(&hall, &frame, kind, mobility) {
                            self.units.push(UnitPlacement { player: i, kind, pos, yaw, squad });
                        }
                    }
                }
            }
        }
    }

    fn find_unit_spot(&mut self, hall: &Obb, frame: &Frame, kind: UnitTypeId, mobility: Mobility) -> Option<(Vec2, f32)> {
        let size = self.data.unit(kind).size;
        let radius = Vec2::new(size[0], size[2]).length() * 0.5;
        let step = size[0].max(size[2]) + 2.0 * UNIT_PAD + 0.4;
        let terrain_count = self.data.terrain.len();
        for ring in 0..12 {
            let gap = radius + 1.2 + ring as f32 * (step + 0.3);
            // Rows fan out up to ~70 degrees either side of straight ahead,
            // filling from the middle.
            let dist = hall.half.max_element() + gap;
            let lateral_steps = (1.2 * dist / step).floor() as i32;
            for k in 0..=2 * lateral_steps {
                let s = if k % 2 == 1 { (k + 1) / 2 } else { -(k / 2) };
                let dir = frame.dir(polar(s as f32 * step / dist));
                let pos = hall.center + dir * (hall.support(dir) + gap);
                let yaw = af_geom::yaw_of(self.layout.center - pos);
                let obb = Obb::from_size(pos, size, yaw);
                let fits = self.inside(&obb, MAP_MARGIN)
                    && self.all_covered(&obb, |c| self.terrain[c].index() < terrain_count && self.data.terrain_passable(self.terrain[c], mobility))
                    && !self.occ.overlaps(&grown(obb, UNIT_PAD));
                if fits {
                    self.occ.insert(grown(obb, UNIT_PAD));
                    return Some((pos, yaw));
                }
            }
        }
        None
    }

    /// Player `i`'s hall footprint, or a point at the start when it has none.
    pub(super) fn hall_or_point(&self, i: usize) -> Obb {
        self.halls.get(i).copied().flatten().unwrap_or(Obb::new(self.layout.starts[i], Vec2::ZERO, 0.0))
    }
}
