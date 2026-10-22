//! Connectivity guarantee (`docs/design/maps-and-terrain.md` section 4):
//! from the main site's hub, every start front, contested spot and meteor
//! site is reachable for every ground class at its widest unit (Foot and
//! Mounted at radius 1.0, Wheeled at 2.5, Tracked at 3.0), and every
//! highland top on foot. Rivers, passes, rim gaps and causeways are planned
//! wide enough that this holds by construction; anything still cut off gets
//! a route carved as a last resort (thicket, marsh and rock become grass,
//! water a ford; the sea is never carved and no layout node is ever
//! removed), and whatever stays cut off is reported.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

use af_core::ext::map::SiteKind;
use af_core::prelude::*;
use bevy::prelude::*;

use super::{cell_center, field, terrain_nav, Gen};

/// A ground class and the radius of its widest unit.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Class {
    pub name: &'static str,
    pub flag: MoveFlag,
    pub radius: f32,
}

/// Every ground class the map must let through. NavGrid needs the
/// (2k + 1)^2 block round each step clear, k = ceil(radius - 0.5): 3 x 3
/// cells for most units, 5 x 5 for the war ordu and the sand engine, 7 x 7
/// for the titan.
pub(crate) const CLASSES: [Class; 4] = [
    Class { name: "Foot", flag: MoveFlag::Foot, radius: 1.0 },
    Class { name: "Mounted", flag: MoveFlag::Mounted, radius: 1.0 },
    Class { name: "Wheeled", flag: MoveFlag::Wheeled, radius: 2.5 },
    Class { name: "Tracked", flag: MoveFlag::Tracked, radius: 3.0 },
];
/// Distance in front of the hall used as each start's reachability point.
const FRONT_GAP: f32 = 2.5;
/// A contested spot counts as reached this close to its centre (its vent,
/// meteor and veins lie within 7 m; its reserved ground reaches 12 m).
const SPOT_REACH: f32 = 9.0;
/// Carving rounds before giving up on a place that stays cut off.
const MAX_CARVES: usize = 3;
/// A carved route is this much wider (each side) than its class's radius,
/// so its clearance square fits however the route runs.
const CARVE_MARGIN: f32 = 2.5;

/// A place every class must reach.
#[derive(Clone, Debug)]
struct Target {
    what: String,
    at: Vec2,
    /// Reached when any reached cell is this close.
    within: f32,
    /// Only on foot (a highland top: climbing up is the ramp's job).
    foot_only: bool,
    /// Where a carved route must run first (a highland's ramp), before
    /// heading for the nearest reached ground.
    lead: Vec<Vec2>,
}

/// What a cell holds for carving: nothing, a node carving may remove, or a
/// building or layout node it must go round.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    None,
    Removable,
    Fixed,
}

/// Cells reachable from the cells within `seed_radius` of `from` (or, if
/// none is clear, the nearest clear cell within 10 m) for a mover of
/// `mobility` and `radius` (4-neighbour flood over clear cells).
pub(crate) fn reachable(nav: &mut NavGrid, from: Vec2, seed_radius: f32, mobility: u16, radius: f32) -> Vec<bool> {
    nav.prepare(mobility, af_geom::NO_TEAM);
    let (w, h) = (nav.width() as i32, nav.height() as i32);
    let mut seen = vec![false; (w * h) as usize];
    let mut queue = VecDeque::new();
    let c = nav.cell_of(from);
    let r = seed_radius.ceil() as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            let n = c + IVec2::new(dx, dy);
            if nav.in_bounds(n) && nav.cell_center(n).distance(from) <= seed_radius && nav.clear_cell(n, mobility, radius, af_geom::NO_TEAM) {
                seen[nav.index(n)] = true;
                queue.push_back(n);
            }
        }
    }
    if queue.is_empty() {
        let Some(start) = nav.nearest_open(from, mobility, radius, af_geom::NO_TEAM, 10.0) else { return seen };
        let start = nav.cell_of(start);
        seen[nav.index(start)] = true;
        queue.push_back(start);
    }
    while let Some(c) = queue.pop_front() {
        for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let n = c + d;
            if nav.in_bounds(n) && !seen[nav.index(n)] && nav.clear_cell(n, mobility, radius, af_geom::NO_TEAM) {
                seen[nav.index(n)] = true;
                queue.push_back(n);
            }
        }
    }
    seen
}

impl Gen<'_> {
    /// Check every class and target; carve a route for anything cut off and
    /// check again, up to [`MAX_CARVES`] rounds. Records the routes actually
    /// carved in `carved` and what stays cut off in `cut_off`, and returns
    /// how many (class, place) pairs stay cut off (0 normally; warns
    /// otherwise). A (class, place) that no route can be carved to is not
    /// tried again.
    pub(super) fn ensure_connectivity(&mut self) -> usize {
        let targets = self.targets();
        let mut hopeless = vec![[false; CLASSES.len()]; targets.len()];
        let mut missed_any = false;
        for _ in 0..MAX_CARVES {
            let mut carved = false;
            let mut nav = self.obstacle_nav();
            for (c, class) in CLASSES.into_iter().enumerate() {
                let seen = self.flood(&mut nav, class);
                let missing: Vec<usize> = (0..targets.len())
                    .filter(|k| {
                        let t = &targets[*k];
                        let needed = !t.foot_only || class.flag == MoveFlag::Foot;
                        needed && !hopeless[*k][c] && !self.reached(&nav, &seen, t)
                    })
                    .collect();
                missed_any |= !missing.is_empty();
                for k in missing {
                    if self.carve_to(&nav, &seen, &targets[k], class) {
                        self.carved.push(describe(class, &targets[k]));
                        carved = true;
                    } else {
                        hopeless[k][c] = true;
                    }
                }
                if carved {
                    // The terrain changed: the next class sees the new grid.
                    nav = self.obstacle_nav();
                }
            }
            if !carved {
                break;
            }
        }
        self.cut_off = if missed_any { self.cut_off_places(&targets) } else { Vec::new() };
        if !self.cut_off.is_empty() {
            warn!("map gen: still cut off after carving: {}", self.cut_off.join("; "));
        }
        self.cut_off.len()
    }

    /// Every (class, place) not reachable from the main site's hub, as
    /// "class: place".
    fn cut_off_places(&self, targets: &[Target]) -> Vec<String> {
        let mut nav = self.obstacle_nav();
        let mut out = Vec::new();
        for class in CLASSES {
            let seen = self.flood(&mut nav, class);
            for t in targets {
                if !(t.foot_only && class.flag != MoveFlag::Foot) && !self.reached(&nav, &seen, t) {
                    out.push(describe(class, t));
                }
            }
        }
        out
    }

    /// [`Gen::cut_off_places`] for this map as it stands.
    #[cfg(test)]
    pub(super) fn cut_off_now(&self) -> Vec<String> {
        self.cut_off_places(&self.targets())
    }

    /// Cells `class` reaches from the main site's hub.
    fn flood(&self, nav: &mut NavGrid, class: Class) -> Vec<bool> {
        let main = self.layout.main();
        reachable(nav, main.hub, main.hub_clear.max(2.0), Mobility::of(class.flag).0, class.radius)
    }

    fn reached(&self, nav: &NavGrid, seen: &[bool], t: &Target) -> bool {
        let c = nav.cell_of(t.at);
        let r = t.within.ceil() as i32;
        (-r..=r).any(|dy| {
            (-r..=r).any(|dx| {
                let n = c + IVec2::new(dx, dy);
                nav.in_bounds(n) && nav.cell_center(n).distance(t.at) <= t.within && seen[nav.index(n)]
            })
        })
    }

    /// Every start front, contested spot, meteor site and highland top.
    fn targets(&self) -> Vec<Target> {
        let lay = &self.layout;
        let mut out: Vec<Target> = self
            .start_fronts()
            .into_iter()
            .enumerate()
            .map(|(i, at)| Target { what: format!("start {i}"), at, within: 4.0, foot_only: false, lead: Vec::new() })
            .collect();
        // At the Shattered Fall the spots are the small craters. A spot is
        // reached from its open ground, inside its ring of veins.
        out.extend(lay.spots.iter().enumerate().map(|(i, at)| Target { what: format!("spot {i}"), at: *at, within: SPOT_REACH, foot_only: false, lead: Vec::new() }));
        let main = lay.main();
        if main.kind == SiteKind::Scar {
            out.extend(main.basins.iter().skip(1).enumerate().map(|(i, b)| Target {
                what: format!("scar basin {i}"),
                at: b.center,
                within: b.radius * 0.9,
                foot_only: false,
                lead: Vec::new(),
            }));
        }
        if main.kind == SiteKind::Lake {
            out.push(Target { what: "the crater lake's shore".to_string(), at: main.center + super::polar(main.gaps[0]) * (main.radius - main.rim_half - 2.5), within: 2.5, foot_only: false, lead: Vec::new() });
        }
        out.extend(self.highlands.iter().enumerate().map(|(i, h)| Target {
            what: format!("highland {i}"),
            at: h.ramp_top(),
            within: 1.5,
            foot_only: true,
            lead: vec![h.ramp_top(), h.ramp_foot()],
        }));
        out
    }

    /// A point just in front of each start's hall, toward the centre.
    fn start_fronts(&self) -> Vec<Vec2> {
        (0..self.layout.starts.len())
            .map(|i| {
                let frame = self.layout.frame(i);
                let reach = self.hall_or_point(i).support(frame.dir(Vec2::X));
                frame.point(Vec2::X * (reach + FRONT_GAP))
            })
            .collect()
    }

    /// The terrain grid with every building and blocking node as an obstacle,
    /// as af_movement will see it on the first tick.
    fn obstacle_nav(&self) -> NavGrid {
        let mut nav = terrain_nav(self.data, self.w as u32, self.w as u32, &self.terrain);
        for b in &self.buildings {
            nav.add_obstacle(&Obb::from_size(b.pos, self.data.building(b.kind).size, b.yaw), 1);
        }
        for n in &self.nodes {
            let def = self.data.node(n.kind);
            if def.blocking {
                nav.add_obstacle(&Obb::from_size(n.pos, def.size, n.yaw), 1);
            }
        }
        nav
    }

    /// What blocks each cell for carving.
    fn blocks(&self) -> Vec<Block> {
        let mut blocks = vec![Block::None; self.terrain.len()];
        for b in &self.buildings {
            self.for_covered(&Obb::from_size(b.pos, self.data.building(b.kind).size, b.yaw), |i| blocks[i] = Block::Fixed);
        }
        for (k, n) in self.nodes.iter().enumerate() {
            let def = self.data.node(n.kind);
            if def.blocking {
                let kind = if k < self.layout_nodes { Block::Fixed } else { Block::Removable };
                self.for_covered(&Obb::from_size(n.pos, def.size, n.yaw), |i| {
                    if blocks[i] != Block::Fixed {
                        blocks[i] = kind;
                    }
                });
            }
        }
        blocks
    }

    /// Carve the cheapest route for `class` from target `t` (through its
    /// lead points first) to ground `seen` already reaches. The route keeps
    /// the class's clearance square clear of buildings and layout nodes
    /// (which carving never removes), so the class fits along it once it is
    /// carved. Returns whether anything changed (false: no route exists, or
    /// none needed carving).
    fn carve_to(&mut self, nav: &NavGrid, seen: &[bool], t: &Target, class: Class) -> bool {
        let mobility = Mobility::of(class.flag);
        let blocks = self.blocks();
        let w = self.w as usize;
        let fixed: Vec<bool> = blocks.iter().map(|b| *b == Block::Fixed).collect();
        let room = field::distance_field(&fixed, w, w);
        // The class's clearance square (k cells each way) must hold no fixed
        // cell: one at least k * sqrt(2) away never is in it.
        let k = nav.clearance_cells(class.radius) as f32;
        let fits = |i: usize| room[i] > k * std::f32::consts::SQRT_2 + 0.01;
        let half = class.radius + CARVE_MARGIN;
        let mut changed = false;
        for leg in t.lead.windows(2) {
            changed |= self.carve_segment(leg[0], leg[1], half, mobility);
        }
        // From open ground near the target (its centre may sit among nodes).
        let (at, within) = match t.lead.last() {
            Some(p) => (*p, 1.0),
            None => (t.at, t.within),
        };
        let r = within.ceil() as i32;
        let c = nav.cell_of(at);
        let mut starts: Vec<usize> = (-r..=r)
            .flat_map(|dy| (-r..=r).map(move |dx| c + IVec2::new(dx, dy)))
            .filter(|n| nav.in_bounds(*n) && nav.cell_center(*n).distance(at) <= within && fits(nav.index(*n)))
            .map(|n| nav.index(n))
            .collect();
        if starts.is_empty() {
            starts.push(nav.index(c));
        }
        if let Some(path) = self.cheapest_way(seen, &blocks, &fits, &starts, mobility) {
            for p in path {
                changed |= self.carve_disc(p, half, mobility);
            }
        }
        changed
    }

    /// Cells (as centres) of the cheapest way from any of `starts` to any
    /// cell in `seen`, over cells that `fits`, where crossing terrain the
    /// class cannot enter costs by how much carving it takes, and the sea,
    /// buildings and layout nodes cannot be crossed at all.
    fn cheapest_way(&self, seen: &[bool], blocks: &[Block], fits: &impl Fn(usize) -> bool, starts: &[usize], mobility: Mobility) -> Option<Vec<Vec2>> {
        #[derive(PartialEq)]
        struct Open(f32, usize);
        impl Eq for Open {}
        impl Ord for Open {
            fn cmp(&self, o: &Self) -> Ordering {
                o.0.total_cmp(&self.0).then_with(|| o.1.cmp(&self.1))
            }
        }
        impl PartialOrd for Open {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        let ids = &self.ids;
        let step_cost = |i: usize| -> Option<f32> {
            let t = self.terrain[i];
            if t == ids.ocean || blocks[i] == Block::Fixed || !fits(i) {
                return None;
            }
            let terrain = if self.data.terrain_passable(t, mobility) {
                1.0
            } else if t == ids.thicket || t == ids.marsh {
                3.0
            } else if t == ids.shallow || t == ids.deep {
                6.0
            } else if t == ids.cliff {
                10.0
            } else {
                12.0
            };
            Some(terrain + if blocks[i] == Block::Removable { 3.0 } else { 0.0 })
        };
        let w = self.w as usize;
        let mut g = vec![f32::INFINITY; w * w];
        let mut came = vec![u32::MAX; w * w];
        let mut heap = BinaryHeap::new();
        for s in starts {
            g[*s] = 0.0;
            heap.push(Open(0.0, *s));
        }
        while let Some(Open(d, i)) = heap.pop() {
            if d > g[i] {
                continue;
            }
            if seen[i] {
                let mut path = vec![i];
                let mut c = i;
                while came[c] != u32::MAX {
                    c = came[c] as usize;
                    path.push(c);
                }
                return Some(path.into_iter().map(|c| cell_center((c % w) as i32, (c / w) as i32)).collect());
            }
            let (x, y) = (i % w, i / w);
            let neighbours = [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < w).then(|| i + w)];
            for j in neighbours.into_iter().flatten() {
                let Some(cost) = step_cost(j) else { continue };
                if d + cost < g[j] {
                    g[j] = d + cost;
                    came[j] = i as u32;
                    heap.push(Open(d + cost, j));
                }
            }
        }
        None
    }

    /// [`Gen::carve_disc`] along `a`-`b`; whether anything changed.
    fn carve_segment(&mut self, a: Vec2, b: Vec2, half: f32, mobility: Mobility) -> bool {
        let steps = (a.distance(b) / 0.5).ceil().max(1.0) as usize;
        let mut changed = false;
        for k in 0..=steps {
            changed |= self.carve_disc(a.lerp(b, k as f32 / steps as f32), half, mobility);
        }
        changed
    }

    /// Make every cell within `half` of `p` passable for `mobility` (water
    /// becomes a ford, anything else grass; never the sea) and remove the
    /// removable nodes there. Returns whether anything changed.
    fn carve_disc(&mut self, p: Vec2, half: f32, mobility: Mobility) -> bool {
        let ids = (self.ids.ocean, self.ids.deep, self.ids.shallow, self.ids.ford, self.ids.grass);
        let data = self.data;
        let mut repainted = 0;
        self.paint_disc(p, half, |cell| {
            let (ocean, deep, shallow, ford, grass) = ids;
            let t = cell.terrain;
            let to = if t == ocean || data.terrain_passable(t, mobility) {
                None
            } else if t == deep || t == shallow {
                Some(ford)
            } else {
                Some(grass)
            };
            repainted += to.is_some() as usize;
            to
        });
        let (first, before) = (self.layout_nodes, self.nodes.len());
        let mut k = 0;
        self.nodes.retain(|n| {
            k += 1;
            let def = data.node(n.kind);
            k - 1 < first || !def.blocking || Obb::from_size(n.pos, def.size, n.yaw).distance_to_point(p) > half + 0.3
        });
        repainted > 0 || self.nodes.len() < before
    }
}

fn describe(class: Class, t: &Target) -> String {
    format!("{}: {} at ({:.0}, {:.0})", class.name, t.what, t.at.x, t.at.y)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use af_core::testing::test_data;

    use super::*;
    use crate::mapgen::{polar, BuildingPlacement};
    use crate::setup::default_setup;

    #[test]
    fn a_walled_in_start_is_carved_free() {
        let data = test_data();
        let mut g = Gen::bare(&data, 2, 224.0, 5);
        g.place_halls(&default_setup(&data));
        let start = g.layout.starts[0];
        let (cliff, mountain) = (g.ids.cliff, g.ids.mountain);
        g.paint_disc(start, 16.0, |cell| {
            let d = cell.center.distance(start);
            ((d - 12.0).abs() <= 1.5).then_some(cliff).or(((d - 14.5).abs() <= 1.0).then_some(mountain))
        });
        let cut = g.cut_off_now();
        assert_eq!(cut.len(), 4, "every class finds start 0 walled in: {cut:?}");
        assert!(cut.iter().all(|c| c.contains("start 0")));
        assert_eq!(g.ensure_connectivity(), 0);
        assert!(g.cut_off_now().is_empty() && g.cut_off.is_empty());
        assert!(!g.carved.is_empty());
    }

    #[test]
    fn a_start_that_carving_cannot_free_is_reported() {
        let data = test_data();
        let mut g = Gen::bare(&data, 2, 224.0, 6);
        g.place_halls(&default_setup(&data));
        // Carving clears terrain and scattered trees, never buildings.
        let (start, kind) = (g.layout.starts[1], g.buildings[0].kind);
        for k in 0..24 {
            let pos = start + polar(k as f32 * TAU / 24.0) * 16.0;
            g.buildings.push(BuildingPlacement { player: 1, kind, pos, yaw: 0.0 });
        }
        assert_eq!(g.ensure_connectivity(), 4, "start 1 stays walled in for every class: {:?}", g.cut_off);
        assert!(g.cut_off.iter().all(|c| c.contains("start 1")));
    }

    #[test]
    fn the_sea_is_never_carved() {
        let data = test_data();
        let mut g = Gen::bare(&data, 2, 224.0, 7);
        g.place_halls(&default_setup(&data));
        let start = g.layout.starts[0];
        let ocean = g.ids.ocean;
        g.paint_disc(start, 20.0, |cell| ((cell.center.distance(start) - 16.0).abs() <= 2.0).then_some(ocean));
        let terrain = g.terrain.clone();
        assert_eq!(g.ensure_connectivity(), 4);
        let still = g.idx_at(start + Vec2::X * 16.0).map(|i| g.terrain[i]);
        assert_eq!(still, Some(ocean));
        // Nothing could be carved: nothing is listed as carved and the
        // terrain is untouched.
        assert!(g.carved.is_empty(), "{:?}", g.carved);
        assert_eq!(g.terrain, terrain);
        assert!(g.cut_off.len() == 4 && g.cut_off.iter().all(|c| c.contains("start 0")), "{:?}", g.cut_off);
    }

    #[test]
    fn only_routes_actually_carved_are_listed() {
        let data = test_data();
        let mut g = Gen::bare(&data, 2, 224.0, 8);
        g.place_halls(&default_setup(&data));
        // Start 0 behind the sea (never carved), start 1 behind a cliff
        // ring (carved once for each class that needs it, then reached).
        let (a, b) = (g.layout.starts[0], g.layout.starts[1]);
        let (ocean, cliff) = (g.ids.ocean, g.ids.cliff);
        g.paint_disc(a, 20.0, |cell| ((cell.center.distance(a) - 16.0).abs() <= 2.0).then_some(ocean));
        g.paint_disc(b, 20.0, |cell| ((cell.center.distance(b) - 16.0).abs() <= 1.5).then_some(cliff));
        assert_eq!(g.ensure_connectivity(), 4, "only start 0 stays cut off: {:?}", g.cut_off);
        assert!(g.cut_off.iter().all(|c| c.contains("start 0")), "{:?}", g.cut_off);
        assert!(!g.carved.is_empty() && g.carved.len() <= 4, "{:?}", g.carved);
        assert!(g.carved.iter().all(|c| c.contains("start 1")), "{:?}", g.carved);
    }
}
