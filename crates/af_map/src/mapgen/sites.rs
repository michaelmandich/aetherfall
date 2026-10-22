//! Meteor sites: where and how the star fell (`docs/design/maps-and-terrain.md`
//! section 3). Planning (the shapes, sized from the land side), painting
//! (cliff rims with gaps, scar floors, the crater lake and its causeways,
//! the spire) and the aether and adamant on them.
//!
//! A rimmed site's floor is a union of discs ([`Basin`]s); its rim runs
//! round that union, open in a gap wherever a gap's ray from the site
//! centre crosses it. A gap's *spoke* (along its ray) is kept free of
//! nodes: from the open hub out through the gap for the gaps toward the
//! starts (and the Scar's ends), across the gap's mouth for the others, so
//! every start's way leads to the heart of the site and no gap is blocked.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use af_core::ext::map::{MeteorSite, SiteKind};
use af_core::prelude::*;
use bevy::prelude::*;

use super::noise::Noise;
use super::nodes::{cluster_spacing, lattice, Zone};
use super::starts::{AETHER_LATERAL, AETHER_RIM_CLEARANCE};
use super::{angle_of, perp, polar, Gen, AETHER_SPOT_RADIUS, START_AREA_RADIUS, START_CLEAR_RADIUS};

/// Rim radius of each kind's main site, as a fraction of the land side.
const CRATER_RING: f32 = 0.11;
const SHATTERED_RING: f32 = 0.07;
/// The Shattered Fall's central crater is never smaller than this, nor than
/// this much per start.
const SHATTERED_MIN_RADIUS: f32 = 18.0;
const SHATTERED_PER_START: f32 = 2.5;
const LAKE_RING: f32 = 0.12;
const SCAR_RING: f32 = 0.08;
/// The Skyfall Scar's two side basins: radius and distance from the centre.
const SCAR_SIDE_RING: f32 = 0.06;
const SCAR_SIDE_OFFSET: f32 = 0.13;
/// The smallest Scar basins (main and side), so even a small map's scar
/// holds its nodes beside the furrow kept clear along its middle.
const SCAR_MIN_RADIUS: f32 = 19.0;
const SCAR_SIDE_MIN_RADIUS: f32 = 14.0;
/// The Starspire's node ring and the spire itself.
const SPIRE_RING: f32 = 0.10;
const SPIRE_PEAK: f32 = 0.04;
/// The smallest spire, and the open ring kept round it.
const SPIRE_PEAK_MIN: f32 = 6.0;
const SPIRE_RING_MIN_WIDTH: f32 = 12.0;
/// The crater lake is never smaller than this, so it holds an island, deep
/// water and a shore.
const LAKE_MIN_RADIUS: f32 = 24.0;
/// The crater lake, from the rim in: a ring of dry shore inside the rim,
/// shallows, deep water, shallows again and the island (a fraction of the
/// rim radius).
const LAKE_SHORE: f32 = 6.0;
const LAKE_SHALLOWS: f32 = 2.0;
const LAKE_ISLAND: f32 = 0.42;
/// How deep into the island its causeway landings reach.
const LAKE_LANDING: f32 = 3.0;
/// Half-width of a causeway (ford) from a rim gap to the island.
const CAUSEWAY_HALF_WIDTH: f32 = 5.5;
/// The Shattered Fall's small craters at the contested spots: rim radius
/// (fraction of the land side, and the least that holds a spot's nodes).
pub(crate) const SPOT_CRATER_RING: f32 = 0.045;
pub(crate) const SPOT_CRATER_MIN: f32 = 12.0;
pub(crate) const SPOT_RIM_HALF: f32 = 1.5;
/// Half-width of each gap in a rim (metres, at any radius): an 11 m
/// opening, wide enough for the widest ground unit (whose 7 x 7 cell
/// clearance square needs about 10 m across a diagonal way).
pub(crate) const GAP_HALF_WIDTH: f32 = 5.5;
/// Width of the marsh and bramble band round the main site.
const SURROUNDINGS: f32 = 12.0;
/// ...which keeps this far beyond a contested spot's reserved ground.
const SPOT_APRON: f32 = 10.0;
/// Reserved margin outside a site's rim.
pub(crate) const SITE_MARGIN: f32 = 2.0;
/// The mouth of a gap without a full spoke: this far in from the rim's
/// outer edge.
const GAP_MOUTH: f32 = 9.0;
/// Nodes keep this far from a gap's spoke: room for the widest unit's 7 x 7
/// cell clearance square however the spoke runs.
const SPOKE_HALF_WIDTH: f32 = 5.0;
/// Extra rim gaps are at least this far (radians) from the others.
const GAP_SPACING: f32 = 0.6;

/// One disc of a site's floor.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Basin {
    pub center: Vec2,
    /// Radius of the middle of the rim round it.
    pub radius: f32,
}

/// Which part of a site a node goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Area {
    /// The floor inside the rim (or the Starspire's open ring).
    Floor,
    /// The crater lake's island.
    Island,
    /// The crater lake's dry inner shore.
    Shore,
}

/// One meteor site.
#[derive(Clone, Debug)]
pub(crate) struct Site {
    pub kind: SiteKind,
    pub center: Vec2,
    /// Published radius: the middle of the rim (the node ring without one).
    pub radius: f32,
    /// The floor is the union of these; a rim runs round it when `rimmed`.
    pub basins: Vec<Basin>,
    pub rimmed: bool,
    /// Half thickness of the rim.
    pub rim_half: f32,
    /// Math angles of the gaps in the rim, as rays from `center`.
    pub gaps: Vec<f32>,
    /// The open heart of the site (the connectivity flood starts here) and
    /// the radius round it kept free of nodes.
    pub hub: Vec2,
    pub hub_clear: f32,
    /// Spokes start this far from the centre.
    pub spoke_from: f32,
    /// How many of the gaps (the first ones: a main site's start gaps) get
    /// a spoke kept free of nodes. The others are open ways in, round the
    /// nodes.
    pub spoked: usize,
    /// The crater lake's island radius, or the Starspire's spire radius.
    pub inner: f32,
}

impl Site {
    /// A round rimmed site.
    fn crater(kind: SiteKind, center: Vec2, radius: f32, rim_half: f32, gaps: Vec<f32>) -> Site {
        Site {
            kind,
            center,
            radius,
            basins: vec![Basin { center, radius }],
            rimmed: true,
            rim_half,
            spoked: gaps.len(),
            gaps,
            hub: center,
            hub_clear: 6.0,
            spoke_from: 6.0,
            inner: 0.0,
        }
    }

    /// The main site of `kind` for a land square of `side` round `center`.
    /// Its gaps toward the starts come first and keep their spokes clear
    /// (with no starts, the first gap does).
    pub fn plan_main(kind: SiteKind, side: f32, center: Vec2, start_angles: &[f32], bisectors: &[f32], start_ring: f32, rng: &mut SimRng) -> Site {
        let mut site = Site::plan_kind(kind, side, center, start_angles, bisectors, start_ring, rng);
        // The Scar also keeps its furrow clear, out through both ends into
        // its side basins (with more starts, their corridors cross those
        // basins already).
        let ends = if kind == SiteKind::Scar && site.basins.len() > 1 && start_angles.len() <= 4 { 2 } else { 0 };
        site.spoked = (start_angles.len() + ends).max(1).min(site.gaps.len());
        site
    }

    fn plan_kind(kind: SiteKind, side: f32, center: Vec2, start_angles: &[f32], bisectors: &[f32], start_ring: f32, rng: &mut SimRng) -> Site {
        let rim_half = rng.range(2.0, 3.0);
        let n = start_angles.len();
        match kind {
            SiteKind::Crater => Site::crater(kind, center, side * CRATER_RING, rim_half, with_extra_gaps(start_angles, 1 + rng.below(2), rng)),
            // Only a gap per start: the small crater holds few nodes.
            SiteKind::Shattered => {
                // Wide enough that a meteor fits between every two start
                // corridors, however many there are.
                let radius = (side * SHATTERED_RING).max(SHATTERED_MIN_RADIUS).max(SHATTERED_PER_START * n as f32 + 7.0);
                let mut site = Site::crater(kind, center, radius, rim_half, with_extra_gaps(start_angles, (n == 0) as u32, rng));
                site.hub_clear = 5.0;
                site
            }
            SiteKind::Lake => {
                // Fewer extra causeways when many starts already have one.
                let extra = if n >= 6 { rng.below(2) } else { 1 + rng.below(2) };
                let radius = (side * LAKE_RING).max(LAKE_MIN_RADIUS);
                let mut site = Site::crater(kind, center, radius, rim_half, with_extra_gaps(start_angles, extra, rng));
                site.inner = radius * LAKE_ISLAND;
                // The island's nodes keep off its causeway landings (the
                // spokes start just inside its shore); the hub is the first
                // causeway's landing.
                site.spoke_from = site.inner - LAKE_LANDING;
                site.hub = center + polar(site.gaps.first().copied().unwrap_or(0.0)) * (site.inner - 2.0);
                site.hub_clear = 2.5;
                site
            }
            SiteKind::Scar => Site::scar(side, center, start_angles, bisectors, start_ring, rim_half, rng),
            SiteKind::Spire => {
                let peak = (side * SPIRE_PEAK).max(SPIRE_PEAK_MIN);
                let ring = (side * SPIRE_RING).max(peak + SPIRE_RING_MIN_WIDTH);
                // The hub sits on the open ring, on a corridor (corridors
                // are node-free), halfway between the spire and the ring.
                let hub_angle = start_angles.first().copied().unwrap_or_else(|| bisectors[0]);
                Site {
                    kind,
                    center,
                    radius: ring,
                    basins: vec![Basin { center, radius: ring }],
                    rimmed: false,
                    rim_half: 0.0,
                    gaps: Vec::new(),
                    hub: center + polar(hub_angle) * (peak + ring) * 0.5,
                    hub_clear: 3.5,
                    spoke_from: 0.0,
                    spoked: 0,
                    inner: peak,
                }
            }
        }
    }

    /// The Skyfall Scar: a main basin at the centre and two smaller ones on
    /// a line through it, the line between the starts (across the axis for
    /// two players). The side basins shrink toward the centre until they
    /// keep out of every start area (or, on crowded maps, every start's
    /// clear disc) and clear of every start's aether cluster, trying every
    /// line the starts allow; when none fits, the scar is a single basin as
    /// big as the Starfall Crater.
    fn scar(side: f32, center: Vec2, start_angles: &[f32], bisectors: &[f32], start_ring: f32, rim_half: f32, rng: &mut SimRng) -> Site {
        let n = start_angles.len();
        let mut lines: Vec<f32> = match n {
            0 => vec![rng.range(0.0, PI)],
            1 | 2 => vec![start_angles[0] + FRAC_PI_2],
            n if n % 2 == 0 => bisectors.to_vec(),
            // Odd: the far end would point at a start, so turn the line a
            // quarter of the start spacing off the axes.
            n => start_angles.iter().flat_map(|a| [a + PI / (2 * n) as f32, a - PI / (2 * n) as f32]).collect(),
        };
        let first = rng.below(lines.len() as u32) as usize;
        lines.rotate_left(first);
        let starts: Vec<Vec2> = start_angles.iter().map(|a| center + polar(*a) * start_ring).collect();
        let main = Basin { center, radius: (side * SCAR_RING).max(SCAR_MIN_RADIUS) };
        let mut basins = vec![Basin { radius: side * CRATER_RING, ..main }];
        let mut theta = lines[0];
        // Where the starts' aether clusters would go with these basins (the
        // shared template stops short of the widest reach; see starts.rs).
        let aether = |basins: &[Basin]| -> Vec<Vec2> {
            let probe = Site { basins: basins.to_vec(), ..Site::crater(SiteKind::Scar, center, main.radius, rim_half, Vec::new()) };
            let reach = start_angles.iter().map(|a| probe.reach_toward(*a)).fold(0.0, f32::max);
            let d = (start_ring - reach - AETHER_RIM_CLEARANCE).clamp(20.0, 55.0);
            let ahead = (d * d - AETHER_LATERAL * AETHER_LATERAL).max(0.0).sqrt();
            starts.iter().flat_map(|s| {
                let x = (center - *s).normalize_or_zero();
                [*s + x * ahead + perp(x) * AETHER_LATERAL, *s + x * ahead - perp(x) * AETHER_LATERAL]
            }).collect()
        };
        'fit: for keep_out in [START_AREA_RADIUS, START_CLEAR_RADIUS + 4.0] {
            for line in &lines {
                let dir = polar(*line);
                for step in 0..5 {
                    let scale = 1.0 - step as f32 * 0.1;
                    let radius = (side * SCAR_SIDE_RING).max(SCAR_SIDE_MIN_RADIUS) * scale;
                    // Overlapping the main basin, so the furrow runs on.
                    let offset = (side * SCAR_SIDE_OFFSET).max(main.radius * 0.6 + radius * 0.7) * scale;
                    let sides = [Basin { center: center + dir * offset, radius }, Basin { center: center - dir * offset, radius }];
                    let outer = radius + rim_half + SITE_MARGIN;
                    let clear_starts = sides.iter().all(|b| starts.iter().all(|s| s.distance(b.center) >= keep_out + outer));
                    let with: Vec<Basin> = std::iter::once(main).chain(sides).collect();
                    let clear_aether = aether(&with).iter().all(|a| sides.iter().all(|b| a.distance(b.center) >= outer + AETHER_SPOT_RADIUS + 1.0));
                    if clear_starts && clear_aether {
                        basins = with;
                        theta = *line;
                        break 'fit;
                    }
                }
            }
        }
        let mut gaps = start_angles.to_vec();
        for end in [theta, theta + PI] {
            if gaps.iter().all(|g| af_geom::wrap_angle(end - g).abs() > GAP_SPACING * 0.5) {
                gaps.push(end);
            }
        }
        Site { kind: SiteKind::Scar, center, radius: basins[0].radius, basins, rimmed: true, rim_half, spoked: gaps.len(), gaps, hub: center, hub_clear: 6.0, spoke_from: 6.0, inner: 0.0 }
    }

    /// A Shattered Fall crater at a contested spot, with a gap toward each
    /// of `toward` (the two starts the spot lies between).
    pub fn spot_crater(center: Vec2, radius: f32, toward: &[Vec2]) -> Site {
        let mut gaps: Vec<f32> = Vec::new();
        for t in toward {
            let a = angle_of(*t - center);
            let a = if gaps.iter().any(|g| af_geom::wrap_angle(a - g).abs() < GAP_SPACING) { a + PI } else { a };
            gaps.push(a);
        }
        let mut site = Site::crater(SiteKind::Shattered, center, radius, SPOT_RIM_HALF, gaps);
        // The spot's vent sits right at the centre; the spokes lead to the
        // edge of its extractor.
        site.hub_clear = 0.0;
        site.spoke_from = 2.5;
        site
    }

    /// Signed distance from `p` to the edge of the floor's union of discs
    /// (negative inside).
    pub fn signed(&self, p: Vec2) -> f32 {
        self.basins.iter().map(|b| p.distance(b.center) - b.radius).fold(f32::INFINITY, f32::min)
    }

    /// How far beyond the floor's edge ([`Site::signed`]) the site's
    /// reserved ground reaches: the rim plus a margin, or the Starspire's
    /// ragged scar ring plus a margin.
    pub fn reserve_margin(&self) -> f32 {
        if self.rimmed { self.rim_half + SITE_MARGIN } else { 4.0 + SITE_MARGIN }
    }

    /// Distance from the centre to the outer edge of the rim, in the
    /// farthest direction.
    pub fn outer(&self) -> f32 {
        self.basins.iter().map(|b| b.center.distance(self.center) + b.radius).fold(0.0, f32::max) + self.rim_half
    }

    /// Distance from the centre to the outer edge of the site along the ray
    /// at math angle `angle`.
    pub fn reach_toward(&self, angle: f32) -> f32 {
        let dir = polar(angle);
        let limit = self.outer() + 1.0;
        let mut t = 0.0;
        while t < limit && self.signed(self.center + dir * t) <= self.rim_half {
            t += 0.25;
        }
        t
    }

    /// Is `p` in the path of one of the rim's gaps?
    pub fn in_gap(&self, p: Vec2) -> bool {
        let v = p - self.center;
        self.gaps.iter().any(|g| {
            let d = polar(*g);
            v.dot(d) > 0.0 && v.dot(perp(d)).abs() < GAP_HALF_WIDTH
        })
    }

    /// Each gap's spoke, out to just beyond the rim: a spoked gap's from
    /// [`Site::spoke_from`], any other's only across its mouth (so nothing
    /// blocks the way in).
    pub fn spokes(&self) -> Vec<(Vec2, Vec2)> {
        self.gaps
            .iter()
            .enumerate()
            .map(|(k, g)| {
                let (d, reach) = (polar(*g), self.reach_toward(*g));
                let from = if k < self.spoked { self.spoke_from } else { (reach - GAP_MOUTH).max(self.spoke_from) };
                (self.center + d * from, self.center + d * (reach + 2.0))
            })
            .collect()
    }

    /// How this site is published in `MapFeatures::sites`: one entry per
    /// basin (the Scar's side basins are sites of their own).
    pub fn published(&self) -> Vec<MeteorSite> {
        let mut out = vec![MeteorSite { kind: self.kind, center: self.center, radius: self.radius }];
        out.extend(self.basins.iter().skip(1).map(|b| MeteorSite { kind: self.kind, center: b.center, radius: b.radius }));
        out
    }

    /// The crater lake's rings, from the centre out: island edge, start of
    /// the deep water, end of the deep water, inner edge of the shore.
    fn lake_rings(&self) -> (f32, f32, f32, f32) {
        let shore = self.radius - self.rim_half - LAKE_SHORE;
        (self.inner, self.inner + LAKE_SHALLOWS, shore - LAKE_SHALLOWS, shore)
    }
}

/// `starts` plus `extra` random angles at least [`GAP_SPACING`] from every
/// other gap (fewer when no such angle turns up).
fn with_extra_gaps(starts: &[f32], extra: u32, rng: &mut SimRng) -> Vec<f32> {
    let mut gaps = starts.to_vec();
    for _ in 0..extra {
        for _ in 0..16 {
            let a = rng.range(0.0, TAU);
            if gaps.iter().all(|g| af_geom::wrap_angle(a - g).abs() > GAP_SPACING) {
                gaps.push(a);
                break;
            }
        }
    }
    gaps
}

impl Gen<'_> {
    /// Paint every meteor site: rims, scar floors, the crater lake, the
    /// spire. Sites own their cells: they ignore reservations (theirs).
    pub(super) fn paint_sites(&mut self, rng: &mut SimRng) {
        let rim = Noise::new(rng, self.size, 6.0);
        let shape = Noise::new(rng, self.size, 9.0);
        for site in self.layout.sites.clone() {
            match site.kind {
                SiteKind::Lake => self.paint_crater_lake(&site, &rim, &shape),
                SiteKind::Spire => self.paint_spire(&site, &shape),
                _ => self.paint_rimmed(&site, &rim),
            }
        }
    }

    /// A cliff rim round the site's floor, open at the gaps; a scar floor.
    fn paint_rimmed(&mut self, site: &Site, rim: &Noise) {
        let (cliff, scorched) = (self.ids.cliff, self.ids.scorched);
        self.paint_disc(site.center, site.outer() + 1.0, |cell| {
            let s = site.signed(cell.center);
            let thick = site.rim_half * (0.85 + 0.3 * rim.at(cell.center));
            if s.abs() <= thick {
                Some(if site.in_gap(cell.center) { scorched } else { cliff })
            } else if s < 0.0 {
                Some(scorched)
            } else {
                None
            }
        });
    }

    /// The crater lake: the rim, a dry shore inside it, a ring of water
    /// (deep in the middle, shallow at both shores), the island of scar, and
    /// a ford causeway from every gap to the island.
    fn paint_crater_lake(&mut self, site: &Site, rim: &Noise, shape: &Noise) {
        let (cliff, scorched, shallow, deep, ford) = (self.ids.cliff, self.ids.scorched, self.ids.shallow, self.ids.deep, self.ids.ford);
        let (island, deep_from, deep_to, shore) = site.lake_rings();
        let causeway = |p: Vec2| {
            let v = p - site.center;
            site.gaps.iter().any(|g| {
                let d = polar(*g);
                v.dot(d) > 0.0 && v.dot(perp(d)).abs() < CAUSEWAY_HALF_WIDTH
            })
        };
        self.paint_disc(site.center, site.outer() + 1.0, |cell| {
            let d = cell.center.distance(site.center);
            let s = d - site.radius;
            let thick = site.rim_half * (0.85 + 0.3 * rim.at(cell.center));
            // The shorelines wobble a little.
            let wobble = (shape.at(cell.center) - 0.5) * 2.0;
            if s.abs() <= thick {
                Some(if site.in_gap(cell.center) { scorched } else { cliff })
            } else if s > 0.0 {
                None
            } else if d < island + wobble * 0.5 || d >= shore + wobble * 0.5 {
                Some(scorched)
            } else if causeway(cell.center) {
                Some(ford)
            } else if d < deep_from + wobble * 0.5 || d >= deep_to + wobble * 0.5 {
                Some(shallow)
            } else {
                Some(deep)
            }
        });
    }

    /// The Starspire: an impassable spire at the centre (mountain, drawn as
    /// a giant crystal) on an open ring of scar.
    fn paint_spire(&mut self, site: &Site, shape: &Noise) {
        let (mountain, scorched) = (self.ids.mountain, self.ids.scorched);
        self.paint_disc(site.center, site.radius + 4.0, |cell| {
            let d = cell.center.distance(site.center);
            let n = shape.at(cell.center);
            if d < site.inner * (0.9 + 0.2 * n) {
                Some(mountain)
            } else if d < site.radius + 1.0 + 3.0 * n {
                Some(scorched)
            } else {
                None
            }
        });
    }

    /// Marsh and bramble patches in a band just outside the main site, so
    /// terrain traits decide who reaches it first. Corridors stay clear, and
    /// so does the ground round the contested spots (everyone reaches
    /// those).
    pub(super) fn paint_site_surroundings(&mut self, rng: &mut SimRng) {
        let marsh_noise = Noise::new(rng, self.size, 7.0);
        let thicket_noise = Noise::new(rng, self.size, 6.0);
        let (grass, marsh, thicket) = (self.ids.grass, self.ids.marsh, self.ids.thicket);
        let main = self.layout.main().clone();
        let spots: Vec<(Vec2, f32)> = self.layout.spots.iter().zip(&self.layout.spot_radius).map(|(s, r)| (*s, r + SPOT_APRON)).collect();
        // A band 12 m wide beyond the site's reserved ground.
        let band = main.reserve_margin() + SURROUNDINGS;
        let open = self.keep_open();
        self.paint_disc(main.center, main.outer() + band, |cell| {
            let by_spot = spots.iter().any(|(s, r)| cell.center.distance_squared(*s) < r * r);
            if cell.reserved || cell.terrain != grass || main.signed(cell.center) > band || by_spot || open[cell.index] {
                None
            } else if thicket_noise.at(cell.center) > 0.66 {
                Some(thicket)
            } else if marsh_noise.at(cell.center) > 0.62 {
                Some(marsh)
            } else {
                None
            }
        });
    }

    /// The aether and adamant of every site. The main site holds 1-3 vents,
    /// 2-4 meteors and 8-12 crystals (half of each at the Shattered Fall,
    /// whose spot craters hold the rest); the crater lake keeps its vents
    /// and meteors on the island and its crystals on the shore and the
    /// island.
    pub(super) fn place_site_nodes(&mut self, rng: &mut SimRng) {
        let main = self.layout.main().clone();
        let (vents, meteors, crystals) = match main.kind {
            SiteKind::Shattered => (1 + rng.below(2), 1 + rng.below(2), 4 + rng.below(3)),
            _ => (1 + rng.below(3), 2 + rng.below(3), 8 + rng.below(5)),
        };
        let (vent, meteor, crystal) = (self.ids.vent, self.ids.meteor, self.ids.crystal);
        if main.kind == SiteKind::Lake {
            for (kind, total, cluster) in [(vent, vents, 1), (meteor, meteors, 1)] {
                let placed = self.scatter_in_site(&main, Area::Island, kind, total, cluster, rng);
                self.scatter_in_site(&main, Area::Shore, kind, total - placed, cluster, rng);
            }
            let on_shore = self.scatter_in_site(&main, Area::Shore, crystal, crystals - crystals / 3, 3, rng);
            let on_island = self.scatter_in_site(&main, Area::Island, crystal, crystals - on_shore, 2, rng);
            self.scatter_in_site(&main, Area::Shore, crystal, crystals - on_shore - on_island, 2, rng);
        } else {
            self.scatter_in_site(&main, Area::Floor, vent, vents, 1, rng);
            self.scatter_in_site(&main, Area::Floor, meteor, meteors, 1, rng);
            self.scatter_in_site(&main, Area::Floor, crystal, crystals, 4, rng);
        }
        if main.kind == SiteKind::Shattered {
            // The spot craters: each spot's usual vent, meteor and veins,
            // plus 2 crystals.
            for site in self.layout.sites.clone().iter().skip(1) {
                let yaw = af_geom::yaw_of(site.center - self.layout.center);
                self.place_node_near(vent, site.center, yaw, Zone::Any);
                self.scatter_in_site(site, Area::Floor, meteor, 1, 1, rng);
                self.scatter_in_site(site, Area::Floor, self.ids.iron, 2, 2, rng);
                self.scatter_in_site(site, Area::Floor, self.ids.copper, 1 + rng.below(2), 2, rng);
                self.scatter_in_site(site, Area::Floor, crystal, 2, 2, rng);
            }
        }
    }

    /// Place `total` nodes on `area` of `site` in clusters of up to
    /// `max_cluster`, each at a random spot; returns how many were placed.
    fn scatter_in_site(&mut self, site: &Site, area: Area, kind: Option<NodeTypeId>, total: u32, max_cluster: u32, rng: &mut SimRng) -> u32 {
        let Some(k) = kind else { return 0 };
        let spacing = cluster_spacing(self.data, k);
        let half = {
            let s = self.data.node(k).size;
            Vec2::new(s[0], s[2]).length() * 0.5
        };
        let spokes = site.spokes();
        // A node units walk over (a vent) only keeps off a spoke's axis.
        let way = if self.data.node(k).blocking { SPOKE_HALF_WIDTH } else { 1.0 };
        let fits = |p: Vec2| site_area_holds(site, area, &spokes, way, p, half);
        let mut placed = 0;
        for _ in 0..total * 40 {
            if placed >= total {
                break;
            }
            let size = if max_cluster > 1 { 2 + rng.below(max_cluster - 1) } else { 1 };
            let count = size.min(total - placed);
            let center = sample_area(site, area, rng);
            let yaw = rng.range(0.0, FRAC_PI_2);
            for off in lattice(count, spacing, yaw, rng) {
                if placed < total && self.place_node_where(kind, center + off, yaw, Zone::Any, fits) {
                    placed += 1;
                }
            }
        }
        placed
    }
}

/// Is `p` a place for a node reaching `half` from its centre on `area`,
/// keeping `way` clear on either side of every spoke?
fn site_area_holds(site: &Site, area: Area, spokes: &[(Vec2, Vec2)], way: f32, p: Vec2, half: f32) -> bool {
    let d = p.distance(site.center);
    let inside = match area {
        Area::Floor if site.kind == SiteKind::Spire => d >= site.inner + 2.0 + half && d <= site.radius - half,
        Area::Floor => site.signed(p) < -(site.rim_half + 1.0 + half),
        Area::Island => d <= site.inner - 1.0 - half,
        Area::Shore => {
            let (_, _, _, shore) = site.lake_rings();
            d >= shore + 0.5 + half && d <= site.radius - site.rim_half * 1.15 - 0.3 - half
        }
    };
    let ways_open = match area {
        // On the island only the causeways' landings must stay open.
        Area::Island => site.gaps.iter().all(|g| {
            let (dir, v) = (polar(*g), p - site.center);
            v.dot(dir) + half < site.spoke_from || v.dot(perp(dir)).abs() >= CAUSEWAY_HALF_WIDTH + half
        }),
        _ => spokes.iter().all(|(a, b)| af_geom::point_segment_distance(p, *a, *b) > way + half),
    };
    inside && ways_open && p.distance(site.hub) > site.hub_clear + half
}

/// A random point on `area` of `site` (not necessarily a valid one).
fn sample_area(site: &Site, area: Area, rng: &mut SimRng) -> Vec2 {
    let (lo, hi, at) = match area {
        Area::Island => (0.0, site.inner, site.center),
        Area::Shore => {
            let (_, _, _, shore) = site.lake_rings();
            (shore, site.radius - site.rim_half, site.center)
        }
        Area::Floor if site.kind == SiteKind::Spire => (site.inner, site.radius, site.center),
        Area::Floor => {
            let b = site.basins[rng.below(site.basins.len() as u32) as usize];
            let lo = if b.center == site.hub { site.hub_clear } else { 0.0 };
            (lo, b.radius - site.rim_half, b.center)
        }
    };
    // Uniform over the annulus.
    let r = (lo * lo + (hi * hi - lo * lo) * rng.f32()).sqrt();
    at + polar(rng.range(0.0, TAU)) * r
}
