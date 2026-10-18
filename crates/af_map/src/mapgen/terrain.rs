//! Terrain painting: the crater rim, water, marsh, cliffs, sand and the
//! start areas. Random features only paint cells outside reserved zones.

use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

use af_core::prelude::*;
use bevy::prelude::*;

use super::noise::Noise;
use super::{angle_of, cell_center, perp, polar, Blob, Gen, GAP_HALF_WIDTH, SPOT_RADIUS, START_CLEAR_RADIUS};

/// Marsh may appear up to this many cells from water.
const MARSH_REACH: u8 = 6;
/// Half-width of the ramp into a cliff highland.
const RAMP_HALF_WIDTH: f32 = 3.5;
/// How far beyond a cliff the way through it (a highland ramp, a ridge
/// opening) is kept free of trees and thicket.
const APPROACH: f32 = 4.0;
/// Fords are at most this far apart along a river (the brief asks for ~40 m).
const FORD_SPACING: (f32, f32) = (28.0, 36.0);
/// Smallest dune region, ridge half-length and highland ring radius a
/// crowded map shrinks those features to before leaving them out.
const MIN_SAND_RADIUS: f32 = 8.0;
const MIN_RIDGE_HALF_LEN: f32 = 10.0;
const MIN_HIGHLAND_RING: f32 = 8.0;
/// A dune's ragged edge reaches out to this multiple of its radius.
const SAND_EDGE: f32 = 1.25;

/// A cliff highland: a ring of cliff around a flat top, with one ramp up.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Highland {
    pub center: Vec2,
    /// Radius of the middle of the cliff ring.
    pub ring: f32,
    /// Half thickness of the ring.
    pub thick: f32,
    /// Unit direction from the centre out through the ramp.
    pub ramp: Vec2,
}

impl Highland {
    /// Radius of the flat top inside the ring.
    pub fn top(&self) -> f32 {
        self.ring - self.thick
    }

    /// Where the ramp reaches the top.
    pub fn ramp_top(&self) -> Vec2 {
        self.center + self.ramp * self.top()
    }

    /// Just beyond the foot of the ramp.
    pub fn ramp_foot(&self) -> Vec2 {
        self.center + self.ramp * (self.ring + self.thick + APPROACH)
    }

    /// Is `p` within `half_width` of the ramp's axis, between the centre
    /// and [`Highland::ramp_foot`]?
    pub fn on_ramp(&self, p: Vec2, half_width: f32) -> bool {
        let v = p - self.center;
        (0.0..=self.ring + self.thick + APPROACH).contains(&v.dot(self.ramp)) && v.dot(perp(self.ramp)).abs() < half_width
    }
}

/// A bowed cliff wall with one opening.
struct Ridge {
    center: Vec2,
    /// Unit direction along the wall.
    dir: Vec2,
    half_len: f32,
    /// Radius of the stamps the wall is painted with.
    thick: f32,
    /// Sideways bulge at the middle.
    bow: f32,
    /// Position of the opening along the wall (`-half_len..half_len`).
    gap_at: f32,
}

impl Ridge {
    /// The centreline, `u` metres along the wall from its middle.
    fn point(&self, u: f32) -> Vec2 {
        let t = u / self.half_len;
        self.center + self.dir * u + perp(self.dir) * self.bow * (1.0 - t * t)
    }

    /// Half the unpainted stretch around `gap_at`. Stamps reach `thick` into
    /// it from each side, so this leaves a ~7 m opening.
    fn gap_half(&self) -> f32 {
        self.thick + 3.5
    }
}

/// What a painter sees of one cell.
pub(crate) struct Cell {
    pub index: usize,
    pub center: Vec2,
    pub terrain: TerrainId,
    pub reserved: bool,
}

impl Gen<'_> {
    pub(super) fn paint_terrain(&mut self, rng: &mut SimRng) {
        self.paint_crater(&mut rng.fork());
        self.paint_water(&mut rng.fork());
        self.paint_marsh_near_water(&mut rng.fork());
        self.paint_cliffs(&mut rng.fork());
        self.paint_sand_regions(&mut rng.fork());
        self.paint_crater_surroundings(&mut rng.fork());
        self.paint_start_areas(&mut rng.fork());
    }

    /// Repaint every cell whose centre is within `r` of `c` with whatever
    /// `f` returns for it (None keeps the cell as it is).
    pub(crate) fn paint_disc(&mut self, c: Vec2, r: f32, mut f: impl FnMut(&Cell) -> Option<TerrainId>) {
        let (x0, y0, x1, y1) = self.cell_rect(c, r);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let center = cell_center(x, y);
                if center.distance_squared(c) > r * r {
                    continue;
                }
                let index = self.idx(x, y);
                let cell = Cell { index, center, terrain: self.terrain[index], reserved: self.reserved[index] };
                if let Some(t) = f(&cell) {
                    self.terrain[index] = t;
                }
            }
        }
    }

    fn is_water(&self, t: TerrainId) -> bool {
        t == self.ids.deep || t == self.ids.shallow
    }

    /// The cliff rim (with one ramp gap per start plus extras) and a
    /// grass/sand crater floor.
    fn paint_crater(&mut self, rng: &mut SimRng) {
        let (center, radius, rim_half) = (self.layout.center, self.layout.crater_radius, self.layout.rim_half);
        let gaps = self.layout.gaps.clone();
        let floor = Noise::new(rng, self.size, 10.0);
        let rim = Noise::new(rng, self.size, 6.0);
        let (grass, sand, cliff) = (self.ids.grass, self.ids.sand, self.ids.cliff);
        self.paint_disc(center, radius + rim_half + 1.0, |cell| {
            let v = cell.center - center;
            let d = v.length();
            let thick = rim_half * (0.85 + 0.3 * rim.at(cell.center));
            if (d - radius).abs() <= thick {
                let a = angle_of(v);
                let in_gap = gaps.iter().any(|g| af_geom::wrap_angle(a - g).abs() * d < GAP_HALF_WIDTH);
                Some(if in_gap { grass } else { cliff })
            } else if d < radius {
                Some(if floor.at(cell.center) > 0.6 { sand } else { grass })
            } else {
                None
            }
        });
    }

    /// One or two rivers or lakes, each along a bisector between neighbours.
    fn paint_water(&mut self, rng: &mut SimRng) {
        let mut angles = self.layout.bisectors.clone();
        let count = 1 + rng.below(2);
        for _ in 0..count {
            let angle = if angles.is_empty() {
                rng.range(0.0, TAU)
            } else {
                angles.swap_remove(rng.below(angles.len() as u32) as usize)
            };
            if rng.chance(0.6) {
                self.paint_river(angle, rng);
            } else {
                self.paint_lake(angle, rng);
            }
        }
    }

    /// A meandering river from just outside the contested spot to past the
    /// map edge: deep water with shallow banks, and shallow fords across it
    /// at most [`FORD_SPACING`] apart.
    fn paint_river(&mut self, angle: f32, rng: &mut SimRng) {
        let center = self.layout.center;
        let dir = polar(angle);
        let side = perp(dir);
        let start = self.layout.spot_ring + SPOT_RADIUS + 6.0;
        let end = self.size;
        let deep = rng.range(2.0, 3.0);
        let reach = deep + rng.range(1.5, 2.5);
        let amplitude = rng.range(2.0, 5.0);
        let wavelength = rng.range(35.0, 60.0);
        let phase = rng.range(0.0, TAU);
        let mut fords = Vec::new();
        let mut f = start + rng.range(8.0, 16.0);
        while f < end {
            fords.push(f);
            f += rng.range(FORD_SPACING.0, FORD_SPACING.1);
        }
        // Deep water needs a non-ford stretch within `deep`, so a ford of
        // half-length `deep + 3` leaves a ~6 m shallow crossing.
        let ford_half = deep + 3.0;

        let cells = (self.w * self.w) as usize;
        let mut near = vec![f32::INFINITY; cells];
        let mut near_deep = vec![f32::INFINITY; cells];
        let mut touched = Vec::new();
        let mut r = start;
        let mut next_feature = start;
        while r <= end {
            let q = center + dir * r + side * amplitude * ops::sin(r / wavelength * TAU + phase);
            let ford = fords.iter().any(|f| (r - f).abs() < ford_half);
            let (x0, y0, x1, y1) = self.cell_rect(q, reach);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = cell_center(x, y).distance(q);
                    if d >= reach {
                        continue;
                    }
                    let i = self.idx(x, y);
                    if near[i].is_infinite() {
                        touched.push(i);
                    }
                    near[i] = near[i].min(d);
                    if !ford {
                        near_deep[i] = near_deep[i].min(d);
                    }
                }
            }
            if r >= next_feature {
                self.features.push(Blob { center: q, radius: reach + 3.0 });
                next_feature += 8.0;
            }
            r += 0.75;
        }
        for i in touched {
            if !self.reserved[i] {
                self.terrain[i] = if near_deep[i] < deep { self.ids.deep } else { self.ids.shallow };
            }
        }
    }

    /// A lake on the bisector where it has the most room.
    fn paint_lake(&mut self, angle: f32, rng: &mut SimRng) {
        let dir = polar(angle);
        let mut best: Option<(Vec2, f32)> = None;
        let mut r = self.layout.spot_ring + SPOT_RADIUS + 8.0;
        while r < self.size * 0.5 {
            let p = self.layout.center + dir * r;
            let room = self.room(p);
            if best.is_none_or(|(_, b)| room > b) {
                best = Some((p, room));
            }
            r += 4.0;
        }
        let Some((c, room)) = best else { return };
        let radius = rng.range(8.0, 13.0).min(room - 2.0);
        if radius < 5.0 {
            return;
        }
        let bank = rng.range(2.0, 3.0);
        let shape = Noise::new(rng, self.size, 9.0);
        let (deep, shallow) = (self.ids.deep, self.ids.shallow);
        self.paint_disc(c, radius * 1.25, |cell| {
            if cell.reserved {
                return None;
            }
            let edge = radius * (0.8 + 0.4 * shape.at(cell.center));
            let d = cell.center.distance(c);
            if d >= edge {
                None
            } else if d < edge - bank {
                Some(deep)
            } else {
                Some(shallow)
            }
        });
        self.features.push(Blob { center: c, radius: radius * 1.25 });
    }

    /// Marsh patches on grass within [`MARSH_REACH`] cells of water.
    fn paint_marsh_near_water(&mut self, rng: &mut SimRng) {
        let noise = Noise::new(rng, self.size, 8.0);
        let cells = self.terrain.len();
        let mut dist = vec![u8::MAX; cells];
        let mut queue = VecDeque::new();
        for (i, t) in self.terrain.iter().enumerate() {
            if self.is_water(*t) {
                dist[i] = 0;
                queue.push_back(i);
            }
        }
        let w = self.w as usize;
        while let Some(i) = queue.pop_front() {
            if dist[i] >= MARSH_REACH {
                continue;
            }
            let (x, y) = (i % w, i / w);
            let neighbours = [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < w).then(|| i + w)];
            for j in neighbours.into_iter().flatten() {
                if dist[j] == u8::MAX {
                    dist[j] = dist[i] + 1;
                    queue.push_back(j);
                }
            }
        }
        for (i, d) in dist.into_iter().enumerate() {
            let center = cell_center((i % w) as i32, (i / w) as i32);
            if (1..=MARSH_REACH).contains(&d) && !self.reserved[i] && self.terrain[i] == self.ids.grass && noise.at(center) > 0.55 {
                self.terrain[i] = self.ids.marsh;
            }
        }
    }

    /// One to three cliff features: ridges with a gap, or highlands (a cliff
    /// ring with one ramp and ore on top). Climbers walk straight over both.
    /// Crowded maps get them shrunk rather than left out.
    fn paint_cliffs(&mut self, rng: &mut SimRng) {
        let count = 1 + rng.below(3);
        let (r_min, r_max) = (self.layout.crater_outer() + 20.0, self.size * 0.65);
        for _ in 0..count {
            if rng.chance(0.5) {
                let thick = rng.range(2.0, 3.0);
                let bow = rng.range(-4.0, 4.0);
                let ends = thick + bow.abs();
                let wanted = rng.range(MIN_RIDGE_HALF_LEN, 18.0) + ends;
                let Some((center, radius)) = self.feature_spot(rng, wanted, MIN_RIDGE_HALF_LEN + ends, r_min, r_max) else { continue };
                let half_len = radius - ends;
                let gap_at = rng.range(-0.4, 0.4) * half_len;
                self.paint_ridge(&Ridge { center, dir: polar(rng.range(0.0, PI)), half_len, thick, bow, gap_at });
                self.features.push(Blob { center, radius });
            } else {
                let thick = rng.range(1.5, 2.0);
                let wanted = rng.range(MIN_HIGHLAND_RING, 11.0) + thick;
                let Some((center, radius)) = self.feature_spot(rng, wanted, MIN_HIGHLAND_RING + thick, r_min, r_max) else { continue };
                // The ramp opens roughly toward the middle of the map, never
                // toward the map edge.
                let ramp = polar(angle_of(self.layout.center - center) + rng.range(-1.0, 1.0));
                let highland = Highland { center, ring: radius - thick, thick, ramp };
                self.paint_highland(&highland);
                self.features.push(Blob { center, radius });
                self.highlands.push(highland);
            }
        }
    }

    /// Paint `ridge` and reserve its opening (plus [`APPROACH`] on either
    /// side), so trees and thicket never close it.
    fn paint_ridge(&mut self, ridge: &Ridge) {
        let (grass, sand, cliff) = (self.ids.grass, self.ids.sand, self.ids.cliff);
        let mut u = -ridge.half_len;
        while u <= ridge.half_len {
            if (u - ridge.gap_at).abs() >= ridge.gap_half() {
                self.paint_disc(ridge.point(u), ridge.thick, |cell| {
                    (!cell.reserved && (cell.terrain == grass || cell.terrain == sand)).then_some(cliff)
                });
            }
            u += 0.5;
        }
        let (gap, dir, side) = (ridge.point(ridge.gap_at), ridge.dir, perp(ridge.dir));
        let (along, across) = (ridge.gap_half(), ridge.thick + APPROACH);
        self.reserve_where(gap, along + across, |p| (p - gap).dot(dir).abs() < along && (p - gap).dot(side).abs() <= across);
    }

    /// Paint `highland`'s cliff ring and reserve its top and its ramp (out
    /// to [`Highland::ramp_foot`]), so trees and thicket never seal the ore.
    fn paint_highland(&mut self, h: &Highland) {
        let (grass, sand, cliff) = (self.ids.grass, self.ids.sand, self.ids.cliff);
        self.paint_disc(h.center, h.ring + h.thick, |cell| {
            if cell.reserved || !(cell.terrain == grass || cell.terrain == sand) {
                return None;
            }
            let on_ring = (cell.center.distance(h.center) - h.ring).abs() <= h.thick;
            (on_ring && !h.on_ramp(cell.center, RAMP_HALF_WIDTH)).then_some(cliff)
        });
        let reach = h.ring + h.thick + APPROACH;
        self.reserve_where(h.center, reach, |p| p.distance(h.center) <= h.top() || h.on_ramp(p, RAMP_HALF_WIDTH + 1.0));
    }

    /// One or two dune regions (palms and quartz sand are placed later).
    /// Crowded maps get them shrunk rather than left out.
    fn paint_sand_regions(&mut self, rng: &mut SimRng) {
        let count = 1 + rng.below(2);
        let (r_min, r_max) = (self.layout.crater_outer() + 15.0, self.size * 0.65);
        for _ in 0..count {
            let wanted = rng.range(11.0, 17.0) * SAND_EDGE;
            let Some((c, outer)) = self.feature_spot(rng, wanted, MIN_SAND_RADIUS * SAND_EDGE, r_min, r_max) else { continue };
            let radius = outer / SAND_EDGE;
            let shape = Noise::new(rng, self.size, 10.0);
            let (grass, sand) = (self.ids.grass, self.ids.sand);
            self.paint_disc(c, outer, |cell| {
                let edge = radius * (0.75 + 0.5 * shape.at(cell.center));
                (!cell.reserved && cell.terrain == grass && cell.center.distance(c) < edge).then_some(sand)
            });
            self.sand_regions.push(Blob { center: c, radius });
            self.features.push(Blob { center: c, radius: outer });
        }
    }

    /// Marsh and bramble patches in a band just outside the rim, so terrain
    /// traits decide who reaches the crater first. Corridors stay clear.
    fn paint_crater_surroundings(&mut self, rng: &mut SimRng) {
        let marsh_noise = Noise::new(rng, self.size, 7.0);
        let thicket_noise = Noise::new(rng, self.size, 6.0);
        let (grass, marsh, thicket) = (self.ids.grass, self.ids.marsh, self.ids.thicket);
        let band = self.layout.crater_outer() + super::CRATER_MARGIN + 12.0;
        self.paint_disc(self.layout.center, band, |cell| {
            if cell.reserved || cell.terrain != grass {
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

    /// A small dune under each start's quartz sand, then the clear grass
    /// disc the main hall stands on.
    fn paint_start_areas(&mut self, rng: &mut SimRng) {
        let shape = Noise::new(rng, self.size, 5.0);
        let (sand_local, sand_radius) = self.layout.template.sand;
        let (grass, sand) = (self.ids.grass, self.ids.sand);
        for i in 0..self.layout.starts.len() {
            let c = self.layout.frame(i).point(sand_local);
            self.paint_disc(c, sand_radius * 1.2, |cell| {
                let edge = sand_radius * (0.8 + 0.3 * shape.at(cell.center));
                (cell.terrain == grass && cell.center.distance(c) < edge).then_some(sand)
            });
        }
        for s in self.layout.starts.clone() {
            self.paint_disc(s, START_CLEAR_RADIUS, |_| Some(grass));
        }
    }
}

#[cfg(test)]
mod tests {
    use af_core::testing::test_data;

    use super::*;
    use crate::mapgen::nodes::Zone;

    /// `n` AI players cycling through every faction.
    fn setup(data: &GameData, n: usize, seed: u64) -> MatchSetup {
        let players = (0..n)
            .map(|i| PlayerSetup {
                name: format!("P{i}"),
                faction: FactionId((i % data.factions.len()) as u8),
                team: i as u8,
                controller: Controller::Ai,
                color: None,
            })
            .collect();
        MatchSetup { seed, players, ..MatchSetup::default() }
    }

    /// (terrain, reserved) of the cell under `p`.
    fn cell(g: &Gen, p: Vec2) -> (TerrainId, bool) {
        let i = g.idx_at(p).expect("on the map");
        (g.terrain[i], g.reserved[i])
    }

    #[test]
    fn crowded_maps_shrink_features_instead_of_dropping_them() {
        let data = test_data();
        // Eight start areas squeezed onto the smallest map.
        let g = Gen::bare(&data, 8, 224.0, 1);
        let (r_min, r_max) = (g.layout.crater_outer() + 15.0, g.size * 0.65);
        let (best, room) = g.roomiest(r_min, r_max).expect("candidates in range");
        let mut rng = SimRng::new(9);
        // More than any spot has: the roomiest one, shrunk to fit.
        assert_eq!(g.feature_spot(&mut rng, room + 10.0, room - 1.0, r_min, r_max), Some((best, room)));
        assert_eq!(g.feature_spot(&mut rng, room + 10.0, room + 1.0, r_min, r_max), None, "not even the minimum fits");
        let (p, r) = g.feature_spot(&mut rng, 4.0, 4.0, r_min, r_max).expect("small features fit");
        assert!(r == 4.0 && g.room(p) >= 4.0);
    }

    #[test]
    fn ridge_openings_stay_open_and_reserved() {
        let data = test_data();
        let mut g = Gen::bare(&data, 2, 224.0, 3);
        let (center, room) = g.roomiest(g.layout.crater_outer() + 20.0, g.size * 0.65).expect("a spot");
        assert!(room > 15.0, "room for the test ridge: {room}");
        g.paint_ridge(&Ridge { center, dir: Vec2::X, half_len: 12.0, thick: 2.0, bow: 0.0, gap_at: 0.0 });
        let cliff = g.ids.cliff;
        assert_eq!(cell(&g, center + Vec2::X * 9.0).0, cliff, "the wall");
        assert_eq!(cell(&g, center - Vec2::X * 9.0).0, cliff, "the wall");
        for p in [center, center + Vec2::X * 2.5, center + Vec2::Y * 5.0, center - Vec2::Y * 5.0] {
            assert_eq!(cell(&g, p), (g.ids.grass, true), "the opening and its approaches at {p}");
        }
        assert!(!cell(&g, center + Vec2::Y * 9.0).1, "only the opening is reserved");
        assert!(!g.place_node(g.ids.oak, center + Vec2::Y * 4.0, 0.0, Zone::Free), "no tree in the opening");
    }

    #[test]
    fn highland_tops_and_ramps_stay_open_and_reserved() {
        let data = test_data();
        let mut g = Gen::bare(&data, 2, 224.0, 4);
        let (center, _) = g.roomiest(g.layout.crater_outer() + 20.0, g.size * 0.65).expect("a spot");
        let h = Highland { center, ring: 9.0, thick: 2.0, ramp: Vec2::X };
        g.paint_highland(&h);
        let (grass, cliff) = (g.ids.grass, g.ids.cliff);
        for side in [-Vec2::X, Vec2::Y, -Vec2::Y] {
            assert_eq!(cell(&g, center + side * 9.0).0, cliff, "the ring");
        }
        for p in [center, h.ramp_top(), center + Vec2::X * 9.0, h.ramp_foot() - Vec2::X * 0.5, center + Vec2::new(12.0, 3.5)] {
            assert_eq!(cell(&g, p), (grass, true), "the top, the ramp and its foot at {p}");
        }
        assert!(!cell(&g, center + Vec2::Y * 13.0).1 && !cell(&g, h.ramp_foot() + Vec2::X * 2.0).1, "nothing else is reserved");
        assert!(!g.place_node(g.ids.oak, center + Vec2::Y * 3.0, 0.0, Zone::Free), "no tree on the top");
        assert!(g.place_node(g.ids.iron, center + Vec2::Y * 3.0, 0.0, Zone::Any), "ore goes up there");
    }

    #[test]
    fn highland_ore_is_reachable_up_a_ramp_free_of_trees() {
        let data = test_data();
        let (iron, copper) = (data.node_id("iron_vein"), data.node_id("copper_vein"));
        let wood = data.resource_id("wood").expect("wood");
        let mut highlands = 0;
        for n in [2, 4, 6, 8] {
            for seed in 0..25 {
                let g = Gen::run(&data, &setup(&data, n, seed));
                assert!(g.cut_off_routes().is_empty(), "every start and highland top is reachable (seed {seed}, {n} players)");
                for h in &g.highlands {
                    highlands += 1;
                    let on_top = |kind: Option<NodeTypeId>| g.nodes.iter().any(|nd| Some(nd.kind) == kind && nd.pos.distance(h.center) < h.top());
                    assert!(on_top(iron) && on_top(copper), "iron and copper on the highland at {} (seed {seed})", h.center);
                    // A metre inside the reserved strip and top, so a tree
                    // straddling their edge does not count.
                    let in_the_way = |p: Vec2| {
                        let v = p - h.center;
                        let along = v.dot(h.ramp);
                        let on_ramp = (0.0..=h.ring + h.thick + APPROACH - 1.0).contains(&along) && v.dot(perp(h.ramp)).abs() < RAMP_HALF_WIDTH;
                        on_ramp || v.length() < h.top() - 1.0
                    };
                    let tree = g.nodes.iter().find(|nd| data.node(nd.kind).resource_id == wood && in_the_way(nd.pos));
                    assert!(tree.is_none(), "{tree:?} blocks the highland at {} (seed {seed}, {n} players)", h.center);
                }
            }
        }
        assert!(highlands >= 20, "only {highlands} highlands in the sample");
    }
}
