//! Terrain painting: the order of the painters, highland ridges and
//! plateaus, dune regions and the start areas (coasts, lakes and marsh are
//! in `water.rs`, rivers in `rivers.rs`, mountains in `mountains.rs`, the
//! meteor sites in `sites.rs`). Random features only paint cells outside
//! reserved zones.

use std::f32::consts::PI;

use af_core::ext::map::MapSize;
use af_core::prelude::*;
use bevy::prelude::*;

use super::noise::Noise;
use super::{angle_of, cell_center, perp, polar, Blob, Gen, Hold, START_CLEAR_RADIUS};

/// Half-width of the ramp into a cliff highland.
const RAMP_HALF_WIDTH: f32 = 3.5;
/// Radius of the open apron kept just below a highland ramp's foot.
const RAMP_APRON: f32 = 5.0;
/// How far beyond a cliff the way through it (a highland ramp, a ridge
/// opening) is kept free of trees and thicket.
pub(crate) const APPROACH: f32 = 4.0;
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
    /// it from each side, so this leaves a ~10 m opening.
    fn gap_half(&self) -> f32 {
        self.thick + 5.0
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
        self.paint_coasts(&mut rng.fork());
        self.paint_sites(&mut rng.fork());
        self.paint_rivers(&mut rng.fork());
        self.paint_cliffs(&mut rng.fork());
        self.paint_sand_regions(&mut rng.fork());
        self.paint_mountains(&mut rng.fork());
        self.paint_lakes(&mut rng.fork());
        self.paint_marsh_near_water(&mut rng.fork());
        self.paint_site_surroundings(&mut rng.fork());
        self.paint_start_areas(&mut rng.fork());
    }

    /// Wild features go between `min` (beyond the main site) and this far
    /// from the layout centre: most of the way to the land's far corners.
    pub(crate) fn wild_reach(&self) -> f32 {
        let (land, c) = (&self.layout.land, self.layout.center);
        [land.min, land.max, Vec2::new(land.min.x, land.max.y), Vec2::new(land.max.x, land.min.y)].iter().map(|p| p.distance(c)).fold(0.0, f32::max) * 0.92
    }

    /// Extra features on bigger maps: 0 on Small and Medium, 1 on Large, 2
    /// on Huge.
    pub(crate) fn size_bonus(&self) -> u32 {
        match self.layout.options.size {
            MapSize::Small | MapSize::Medium => 0,
            MapSize::Large => 1,
            MapSize::Huge => 2,
        }
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
                let cell = Cell { index, center, terrain: self.terrain[index], reserved: self.reserved(index) };
                if let Some(t) = f(&cell) {
                    self.terrain[index] = t;
                }
            }
        }
        self.touch();
    }

    /// One or two cliff features (one more on Large maps, two on Huge):
    /// ridges with a gap, or highlands (a cliff ring with one ramp and ore
    /// on top). Climbers walk straight over both. Crowded maps get them
    /// shrunk rather than left out, and a map with no room for any gets one
    /// small highland closer to the main site.
    fn paint_cliffs(&mut self, rng: &mut SimRng) {
        let count = 1 + rng.below(2) + self.size_bonus();
        let (r_min, r_max) = (self.layout.main().outer() + 20.0, self.wild_reach());
        let mut placed = 0;
        for _ in 0..count {
            let ridge = rng.chance(0.5);
            placed += self.place_cliff(rng, ridge, r_min, r_max, 1.0) as u32;
        }
        if placed == 0 {
            self.place_cliff(rng, false, self.layout.main().outer() + 8.0, r_max, 0.85);
        }
    }

    /// A ridge or a highland between `r_min` and `r_max` from the centre,
    /// its smallest size scaled by `small`; false when none fits.
    fn place_cliff(&mut self, rng: &mut SimRng, ridge: bool, r_min: f32, r_max: f32, small: f32) -> bool {
        if ridge {
            let thick = rng.range(2.0, 3.0);
            let bow = rng.range(-4.0, 4.0);
            let ends = thick + bow.abs();
            let wanted = rng.range(MIN_RIDGE_HALF_LEN, 18.0) + ends;
            let Some((center, radius)) = self.feature_spot(rng, wanted, MIN_RIDGE_HALF_LEN * small + ends, r_min, r_max) else { return false };
            let half_len = radius - ends;
            let gap_at = rng.range(-0.4, 0.4) * half_len;
            self.paint_ridge(&Ridge { center, dir: polar(rng.range(0.0, PI)), half_len, thick, bow, gap_at });
            self.features.push(Blob { center, radius });
        } else {
            let thick = rng.range(1.5, 2.0);
            let wanted = rng.range(MIN_HIGHLAND_RING, 11.0) + thick;
            let Some((center, radius)) = self.feature_spot(rng, wanted, MIN_HIGHLAND_RING * small + thick, r_min, r_max) else { return false };
            // The ramp opens roughly toward the middle of the map, never
            // toward the map edge: of a few such directions, the one with
            // the most open ground below its foot (away from water and rock).
            let inward = angle_of(self.layout.center - center);
            let first = rng.range(-1.0, 1.0);
            let open = |a: f32| {
                let below = center + polar(a) * (radius + APPROACH + 6.0);
                let (edge, _, _, blocking) = self.room_parts(below);
                blocking.min(edge)
            };
            let angle = [first, first + 0.7, first - 0.7, first + 1.4, first - 1.4]
                .into_iter()
                .filter(|o| o.abs() <= 1.2)
                .map(|o| inward + o)
                .fold((f32::NEG_INFINITY, inward + first), |best, a| if open(a) > best.0 + 2.0 { (open(a), a) } else { best })
                .1;
            let ramp = polar(angle);
            let highland = Highland { center, ring: radius - thick, thick, ramp };
            self.paint_highland(&highland);
            self.features.push(Blob { center, radius });
            self.highlands.push(highland);
        }
        true
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
        self.reserve(gap, along + across, Hold::Hard, |p| (p - gap).dot(dir).abs() < along && (p - gap).dot(side).abs() <= across);
    }

    /// Paint `highland`'s cliff ring and reserve its top, its ramp (out to
    /// [`Highland::ramp_foot`]) and an apron below the ramp's foot, so trees
    /// and thicket never seal the ore.
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
        self.reserve(h.center, reach, Hold::Hard, |p| p.distance(h.center) <= h.top() || h.on_ramp(p, RAMP_HALF_WIDTH + 1.0));
        let apron = h.ramp_foot() + h.ramp * RAMP_APRON;
        self.reserve(apron, RAMP_APRON + 0.5, Hold::Hard, |p| p.distance(apron) <= RAMP_APRON);
    }

    /// One or two dune regions (one more on Large and Huge maps; palms and
    /// quartz sand are placed later). Crowded maps get them shrunk rather
    /// than left out.
    fn paint_sand_regions(&mut self, rng: &mut SimRng) {
        let count = 1 + rng.below(2) + self.size_bonus().min(1);
        let (r_min, r_max) = (self.layout.main().outer() + 15.0, self.wild_reach());
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
    use af_core::ext::map::MapOptions;
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
        (g.terrain[i], g.reserved(i))
    }

    #[test]
    fn crowded_maps_shrink_features_instead_of_dropping_them() {
        let data = test_data();
        // Eight start areas squeezed onto the smallest map.
        let g = Gen::bare(&data, 8, 224.0, 1);
        let (r_min, r_max) = (g.layout.main().outer() + 15.0, g.wild_reach());
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
        let (center, room) = g.roomiest(g.layout.main().outer() + 20.0, g.wild_reach()).expect("a spot");
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
        let (center, _) = g.roomiest(g.layout.main().outer() + 20.0, g.wild_reach()).expect("a spot");
        let h = Highland { center, ring: 9.0, thick: 2.0, ramp: Vec2::X };
        g.paint_highland(&h);
        let (grass, cliff) = (g.ids.grass, g.ids.cliff);
        for side in [-Vec2::X, Vec2::Y, -Vec2::Y] {
            assert_eq!(cell(&g, center + side * 9.0).0, cliff, "the ring");
        }
        for p in [center, h.ramp_top(), center + Vec2::X * 9.0, h.ramp_foot() - Vec2::X * 0.5, center + Vec2::new(12.0, 3.5)] {
            assert_eq!(cell(&g, p), (grass, true), "the top, the ramp and its foot at {p}");
        }
        // An apron below the foot stays open too, and nothing beyond.
        assert!(cell(&g, h.ramp_foot() + Vec2::X * 2.0).1, "the apron below the ramp");
        assert!(!cell(&g, center + Vec2::Y * 13.0).1 && !cell(&g, h.ramp_foot() + Vec2::X * (2.0 * RAMP_APRON + 1.0)).1, "nothing else is reserved");
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
                let g = Gen::run(&data, &setup(&data, n, seed), &MapOptions::default());
                assert!(g.cut_off.is_empty(), "everything is reachable for every class: {:?} (seed {seed}, {n} players)", g.cut_off);
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
