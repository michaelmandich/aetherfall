//! Rivers (`docs/design/maps-and-terrain.md` section 2.1).
//!
//! A river runs from a source on a land edge to a mouth on another land
//! edge, in the sea, or (a tributary) on the first river. Its course is
//! found with A* on a 4 m grid over the map: everything reserved is closed,
//! start areas included, start corridors beyond the start areas are open
//! (the river crosses them as a ford), land near a land edge costs extra and
//! its outermost strip is closed except near the river's own ends, value
//! noise makes it wander, and a second river keeps its distance from the
//! first and never crosses it. A crowded map whose starts, spots and site
//! close off the interior gets a course from a laxer [`Tier`]: start-area
//! outskirts open at a price, smaller margins, then a narrower river that
//! may cut a corner. The course is smoothed, resampled every 0.75 m and
//! given a gentle meander wherever that keeps it clear; then it is painted
//! deep with shallow banks, with fords across its whole width at most 36 m
//! apart and wherever it crosses a corridor.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::f32::consts::TAU;

use af_core::ext::map::{MapEdge, RiverLine};
use af_core::prelude::*;
use bevy::prelude::*;

use super::field::distance_field;
use super::noise::Noise;
use super::terrain::APPROACH;
use super::{cell_center, perp, polar, Gen, Hold, CORRIDOR_HALF_WIDTH};

/// Routing grid step.
const STEP: f32 = 4.0;
/// Centreline point spacing.
pub(crate) const SPACING: f32 = 0.75;
/// Fords are at most this far apart along a river (between ford centres).
pub(crate) const FORD_SPACING: (f32, f32) = (28.0, 36.0);
/// A ford runs `deep + FORD_EXTRA` either side of its centre along the river,
/// leaving at least 12 m of ford where the river is deepest.
const FORD_EXTRA: f32 = 6.0;
/// Land this close to a land edge (fraction of the land side) costs extra,
/// the more the nearer the edge...
const EDGE_BAND: f32 = 0.2;
/// ...except this close (fraction of the land side) to the river's own
/// source and mouth.
const END_FREE: f32 = 0.2;
/// A course may keep only a share of its middle (beyond [`END_FREE`] from
/// its ends) this close to a land edge (fraction of the land side; see
/// [`Tier::near_edge_share`]).
const NEAR_EDGE: f32 = 0.12;
/// Chance that a second river is a tributary of the first.
const TRIBUTARY_CHANCE: f32 = 0.4;
/// Courses tried per strictness tier.
const ATTEMPTS: u32 = 12;
/// A second river pays extra this close to the first.
const NEAR_OTHER: f32 = 28.0;
/// The river pokes this far past the map edge, so it visibly leaves the map.
const OVERHANG: f32 = 6.0;
/// Within this much of the least clearance, a course costs extra.
const HUG: f32 = 10.0;
/// Away from its ends, a river's water keeps at least this far inside the
/// land edges.
const EDGE_STRIP: f32 = 4.0;
/// ...except this close to its source and its mouth.
const STRIP_FREE: f32 = 20.0;
/// The routed course (before smoothing) keeps at least this far from a land
/// edge away from its ends, however narrow the river.
const STRIP_MIN: f32 = 12.0;

/// Where a river ends.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mouth {
    /// At a land edge, at this point.
    Edge(Vec2),
    /// In the sea, heading for this point.
    Sea(Vec2),
    /// In river `usize`.
    Joins(usize),
}

/// How strictly a course keeps its distance from what it must not touch.
/// The first tier is the spec; the others are for crowded maps only.
#[derive(Clone, Copy, Debug)]
struct Tier {
    /// Start areas are closed, or only cost extra.
    areas_closed: bool,
    /// Extra cost per routing cell in a start area when open.
    area_cost: f32,
    /// Clearance beyond the river's own reach from everything closed.
    margin: f32,
    /// Weight of the edge band cost.
    edge_weight: f32,
    /// Most of the course (beyond its ends) that may lie near a land edge.
    near_edge_share: f32,
    /// A narrower river ([`Size::narrowed`]), for the most crowded maps.
    narrow: bool,
    /// How far off the centre (fraction of the land side) the lines that
    /// pick a course's ends may pass.
    offset: f32,
}

const TIERS: [Tier; 4] = [
    Tier { areas_closed: true, area_cost: 0.0, margin: 4.0, edge_weight: 1.0, near_edge_share: 0.2, narrow: false, offset: 0.28 },
    Tier { areas_closed: false, area_cost: 25.0, margin: 2.5, edge_weight: 1.0, near_edge_share: 0.3, narrow: false, offset: 0.32 },
    Tier { areas_closed: false, area_cost: 6.0, margin: 1.0, edge_weight: 0.4, near_edge_share: 0.6, narrow: false, offset: 0.45 },
    Tier { areas_closed: false, area_cost: 6.0, margin: 0.5, edge_weight: 0.4, near_edge_share: 0.6, narrow: true, offset: 0.62 },
];

/// How far every cell is from what a river must keep clear of, for one
/// river at one tier.
struct Clearance {
    /// For a river that ends on land (the sea and the other rivers closed).
    land: Vec<f32>,
    /// For a river that ends in the sea (the coast open).
    sea: Vec<f32>,
    /// For a tributary (the rivers already painted open, so it can reach
    /// the one it joins; None: no river yet).
    join: Option<Vec<f32>>,
    /// Distance to the rivers already painted (None: none yet).
    other: Option<Vec<f32>>,
    /// Cells of the rivers already painted.
    river: Vec<bool>,
}

/// The routing grid for one course.
struct Grid {
    /// Cells per side.
    n: i32,
    /// Step cost per cell (infinite: closed).
    cost: Vec<f32>,
    /// Cells the course may end on (the sea, the first river), however
    /// closed.
    goal: Vec<bool>,
}

impl Grid {
    fn cell(&self, p: Vec2) -> Option<usize> {
        let (x, y) = ((p.x / STEP).floor() as i32, (p.y / STEP).floor() as i32);
        (x >= 0 && y >= 0 && x < self.n && y < self.n).then(|| (y * self.n + x) as usize)
    }

    fn center(&self, i: usize) -> Vec2 {
        Vec2::new((i as i32 % self.n) as f32 + 0.5, (i as i32 / self.n) as f32 + 0.5) * STEP
    }

    fn open(&self, i: usize) -> bool {
        self.cost[i].is_finite() || self.goal[i]
    }

    /// Cheapest course from `from` to a cell passing `goal` (A* toward
    /// `aim`), as cell indices from `from`.
    fn route(&self, from: usize, aim: Vec2, goal: impl Fn(usize) -> bool) -> Option<Vec<usize>> {
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
        let cells = (self.n * self.n) as usize;
        let mut g = vec![f32::INFINITY; cells];
        let mut came = vec![u32::MAX; cells];
        let mut heap = BinaryHeap::new();
        let h = |i: usize| self.center(i).distance(aim) / STEP;
        let step_cost = |i: usize| if self.goal[i] { 1.0 } else { self.cost[i] };
        g[from] = 0.0;
        heap.push(Open(h(from), from));
        while let Some(Open(f, i)) = heap.pop() {
            if f > g[i] + h(i) + 1e-3 {
                continue;
            }
            if goal(i) && i != from {
                let mut path = vec![i];
                let mut c = i;
                while c != from {
                    c = came[c] as usize;
                    path.push(c);
                }
                path.reverse();
                return Some(path);
            }
            if self.goal[i] && i != from {
                // Water that is not this course's goal: no way on through it.
                continue;
            }
            let (x, y) = (i as i32 % self.n, i as i32 / self.n);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= self.n || ny >= self.n {
                    continue;
                }
                let j = (ny * self.n + nx) as usize;
                // No squeezing diagonally between two closed cells.
                if !self.open(j) || (dx != 0 && dy != 0 && !(self.open((y * self.n + nx) as usize) && self.open((ny * self.n + x) as usize))) {
                    continue;
                }
                let step = if dx != 0 && dy != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
                let next = g[i] + step * 0.5 * (step_cost(i) + step_cost(j));
                if next < g[j] {
                    g[j] = next;
                    came[j] = i as u32;
                    heap.push(Open(next + h(j), j));
                }
            }
        }
        None
    }
}

/// A river's size, chosen before its course.
#[derive(Clone, Copy, Debug)]
struct Size {
    /// Deep core half-width at the source (it widens by a third to the mouth).
    deep: f32,
    /// Shallow banks beyond the deep core.
    bank: f32,
}

impl Size {
    /// Widest reach of the water from the centreline.
    fn reach(&self) -> f32 {
        self.deep * 4.0 / 3.0 + self.bank
    }

    /// The river at about two thirds of its width, for maps too crowded for
    /// a full one.
    fn narrowed(self) -> Size {
        Size { deep: self.deep * 0.6, bank: self.bank * 0.7 }
    }
}

/// The best course turned down for running too near the edges or for being
/// short.
struct Fallback {
    /// Lower is better: the share of it near an edge, plus 1 when short.
    score: f32,
    line: Vec<Vec2>,
    mouth: Mouth,
    tier: Tier,
}

impl Gen<'_> {
    /// One or two rivers (the resolved option). When no tier finds a course
    /// across the interior (a crowded layout whose starts, spots and site
    /// close it off), the course that keeps furthest from the edges is
    /// taken; a river that finds no course at all (only on tiny custom maps)
    /// is left out with a warning.
    pub(super) fn paint_rivers(&mut self, rng: &mut SimRng) {
        let wander = Noise::new(rng, self.size, (self.layout.land.side * 0.2).max(30.0));
        for k in 0..self.layout.options.rivers {
            let tributary = k > 0 && !self.rivers.is_empty() && rng.chance(TRIBUTARY_CHANCE);
            let full = Size { deep: rng.range(2.0, 3.0), bank: rng.range(1.5, 2.5) };
            let mut fallback = None;
            let mut course = None;
            // A tributary with nowhere to join becomes a river of its own; a
            // second river with no way across the land joins the first.
            let kinds: &[bool] = match (tributary, self.rivers.is_empty()) {
                (true, _) => &[true, false],
                (false, true) => &[false],
                (false, false) => &[false, true],
            };
            'search: for joins in kinds {
                for tier in TIERS {
                    let size = if tier.narrow { full.narrowed() } else { full };
                    if let Some((line, mouth)) = self.find_course(size, *joins, tier, &wander, &mut fallback, rng) {
                        course = Some((line, mouth, size));
                        break 'search;
                    }
                }
            }
            let course = course.or_else(|| {
                fallback.map(|f: Fallback| {
                    let size = if f.tier.narrow { full.narrowed() } else { full };
                    (self.meander(f.line, size, f.tier, f.mouth, rng), f.mouth, size)
                })
            });
            match course {
                Some((points, mouth, size)) => self.lay_river(points, mouth, size, rng),
                None => warn!("map gen: no course for river {} of {}", k + 1, self.layout.options.rivers),
            }
        }
    }

    /// What every cell must keep clear of at `tier`.
    fn river_clearance(&self, tier: Tier) -> Clearance {
        let w = self.w as usize;
        let mut land = vec![false; w * w];
        let mut sea = vec![false; w * w];
        let mut river = vec![false; w * w];
        let mut join = vec![false; w * w];
        for i in 0..w * w {
            river[i] = self.is_river_water(i);
            let held = match self.hold[i] {
                Hold::Hard => true,
                Hold::Area => tier.areas_closed,
                Hold::Corridor | Hold::Free => false,
            };
            let rough = self.terrain[i] != self.ids.grass && self.terrain[i] != self.ids.sand;
            land[i] = held || river[i] || rough;
            sea[i] = held || river[i] || rough && !self.coast[i];
            join[i] = held || rough && !river[i];
        }
        let other = self.rivers.first().map(|_| distance_field(&river, w, w));
        let join = self.rivers.first().map(|_| distance_field(&join, w, w));
        Clearance { land: distance_field(&land, w, w), sea: distance_field(&sea, w, w), join, other, river }
    }

    /// The routing grid for a river of `size` at `tier`, from `ends.0` to
    /// `ends.1`, ending as `mouth` says.
    fn river_grid(&self, clear: &Clearance, size: Size, tier: Tier, wander: &Noise, ends: (Vec2, Vec2), mouth: Mouth) -> Grid {
        let side = self.layout.land.side;
        let field = match (mouth, &clear.join) {
            (Mouth::Sea(_), _) => &clear.sea,
            (Mouth::Joins(_), Some(join)) => join,
            _ => &clear.land,
        };
        let n = (self.size / STEP).ceil() as i32;
        let cells = (n * n) as usize;
        let mut cost = vec![f32::INFINITY; cells];
        let mut goal = vec![false; cells];
        let band = side * EDGE_BAND;
        let need = size.reach() + tier.margin;
        for k in 0..cells {
            // A partial last cell on a map whose side is no multiple of the
            // step samples the map's edge.
            let p = (Vec2::new((k as i32 % n) as f32 + 0.5, (k as i32 / n) as f32 + 0.5) * STEP).min(Vec2::splat(self.size - 0.5));
            let Some(i) = self.idx_at(p) else { continue };
            goal[k] = match mouth {
                Mouth::Joins(_) => clear.river[i],
                Mouth::Sea(_) => self.coast[i] && (self.terrain[i] == self.ids.ocean || self.terrain[i] == self.ids.shallow),
                Mouth::Edge(_) => false,
            };
            if goal[k] || field[i] < need {
                continue;
            }
            let mut c = 1.0 + 2.5 * wander.at(p);
            // Keep a little further off than it must, rather than hugging
            // a crater rim or a start area.
            if field[i] < need + HUG {
                c += 3.0 * (1.0 - (field[i] - need) / HUG);
            }
            let edge = self.layout.land.land_edge_distance(p);
            let free = p.distance(ends.0).min(p.distance(ends.1)) < side * END_FREE;
            // Away from its own ends a river never runs along the very edge.
            if p.distance(ends.0).min(p.distance(ends.1)) >= STRIP_FREE && edge < (need + EDGE_STRIP).max(STRIP_MIN) {
                continue;
            }
            if edge < band && !free {
                let t = 1.0 - edge / band;
                c += 6.0 * t * (1.0 + t) * tier.edge_weight;
            }
            if self.hold[i] == Hold::Area {
                c += tier.area_cost;
            }
            if self.coast[i] {
                // Across a beach rather than along it.
                c += 4.0;
            }
            if let Some(other) = &clear.other
                && other[i] < NEAR_OTHER
            {
                c += 4.0 * (1.0 - other[i] / NEAR_OTHER);
            }
            cost[k] = c;
        }
        Grid { n, cost, goal }
    }

    /// Water painted by a river or a lake (not the sea).
    pub(crate) fn is_river_water(&self, i: usize) -> bool {
        !self.coast[i] && (self.terrain[i] == self.ids.deep || self.terrain[i] == self.ids.shallow || self.terrain[i] == self.ids.ford)
    }

    /// A course for a new river: its centreline (source first, every
    /// [`SPACING`] m, meander included) and its mouth. A course turned down
    /// only for keeping too near the edges goes to `fallback` if it keeps
    /// further from them than the one there.
    fn find_course(&self, size: Size, tributary: bool, tier: Tier, wander: &Noise, fallback: &mut Option<Fallback>, rng: &mut SimRng) -> Option<(Vec<Vec2>, Mouth)> {
        let land = &self.layout.land;
        let side = land.side;
        let clear = self.river_clearance(tier);
        let mut tried = 0;
        for _ in 0..ATTEMPTS * 4 {
            if tried == ATTEMPTS {
                break;
            }
            // A line in a random direction near the centre: the river enters
            // where it crosses the map edge on one side and (unless it is a
            // tributary) leaves on the other.
            let (a, b) = if tier.narrow && rng.chance(0.5) {
                // The most crowded maps may have room only in a corner.
                self.corner_cut(rng)
            } else {
                let dir = polar(rng.range(0.0, TAU));
                let base = land.center() + perp(dir) * rng.range(-tier.offset, tier.offset) * side;
                let (Some(a), Some(b)) = (self.edge_hit(base, -dir), self.edge_hit(base, dir)) else { continue };
                (a, b)
            };
            // Rivers never cross: a second river's line must stay on one
            // side of the first.
            if !tributary && self.rivers.iter().any(|r| crosses(a, b, &r.points)) {
                continue;
            }
            tried += 1;
            let (source, far) = match (self.on_land_edge(a), self.on_land_edge(b)) {
                (true, _) => (a, b),
                (false, true) => (b, a),
                (false, false) => continue,
            };
            let mouth = if tributary {
                Mouth::Joins(0)
            } else if self.on_land_edge(far) {
                Mouth::Edge(far)
            } else {
                Mouth::Sea(far)
            };
            let main_line = if tributary { Some(&self.rivers[0].points) } else { None };
            let aim = match (mouth, main_line) {
                (Mouth::Edge(p) | Mouth::Sea(p), _) => p,
                (Mouth::Joins(_), Some(l)) => nearest_on(l, source),
                (Mouth::Joins(_), None) => continue,
            };
            let grid = self.river_grid(&clear, size, tier, wander, (source, aim), mouth);
            let limit = Vec2::splat(self.size - 0.01);
            let Some(from) = grid.cell(source.clamp(Vec2::ZERO, limit)) else { continue };
            if grid.cost[from].is_infinite() {
                continue;
            }
            let goal_cell = grid.cell(aim.clamp(Vec2::ZERO, limit));
            let path = grid.route(from, aim, |i| match mouth {
                Mouth::Edge(_) => Some(i) == goal_cell,
                Mouth::Sea(_) => grid.goal[i],
                // Join the main river away from its very ends.
                Mouth::Joins(_) => {
                    grid.goal[i]
                        && main_line.is_some_and(|l| {
                            let k = nearest_index(l, grid.center(i));
                            k > l.len() / 6 && k < l.len() * 5 / 6
                        })
                }
            });
            let Some(path) = path else { continue };
            let mut points: Vec<Vec2> = Vec::with_capacity(path.len() + 2);
            points.push(source - self.inward(source) * OVERHANG);
            points.extend(path[1..path.len() - 1].iter().map(|i| grid.center(*i)));
            let end = grid.center(*path.last().expect("a path has an end"));
            let mouth_point = match (mouth, main_line) {
                (Mouth::Edge(p), _) => p - self.inward(p) * OVERHANG,
                (Mouth::Sea(_), _) => end + (end - points.last().copied().unwrap_or(source)).normalize_or_zero() * 10.0,
                (Mouth::Joins(_), l) => l.map_or(end, |l| nearest_on(l, end)),
            };
            points.push(mouth_point);
            let line = resample(&chaikin(&points, 3), SPACING);
            let length = line.len() as f32 * SPACING;
            let min_length = if tributary { 0.3 } else { 0.55 } * side;
            // Too short (a corner cut) or too near the edges: kept only as a
            // fallback, the long ones first, then the ones furthest in.
            let share = self.near_edge_share(&line);
            let long = length >= min_length;
            if !long || share > tier.near_edge_share {
                let score = share + if long { 0.0 } else { 1.0 };
                if length >= min_length * 0.5 && fallback.as_ref().is_none_or(|f| score < f.score) {
                    *fallback = Some(Fallback { score, line, mouth, tier });
                }
                continue;
            }
            let line = self.meander(line, size, tier, mouth, rng);
            return Some((line, mouth));
        }
        None
    }

    /// Two points on the two edges of a random map corner, each 15-35 % of
    /// the land side from it.
    fn corner_cut(&self, rng: &mut SimRng) -> (Vec2, Vec2) {
        let s = self.size;
        let corner = [Vec2::ZERO, Vec2::new(s, 0.0), Vec2::new(0.0, s), Vec2::splat(s)][rng.below(4) as usize];
        let inward = Vec2::new(if corner.x > 0.0 { -1.0 } else { 1.0 }, if corner.y > 0.0 { -1.0 } else { 1.0 });
        let side = self.layout.land.side;
        let (d1, d2) = (rng.range(0.15, 0.35) * side, rng.range(0.15, 0.35) * side);
        (corner + Vec2::new(inward.x * d1, 0.0), corner + Vec2::new(0.0, inward.y * d2))
    }

    /// Where the ray from `p` along `dir` leaves the map.
    fn edge_hit(&self, p: Vec2, dir: Vec2) -> Option<Vec2> {
        let s = self.size;
        let mut best = f32::INFINITY;
        for (o, d) in [(p.x, dir.x), (p.y, dir.y)] {
            if d > 1e-6 {
                best = best.min((s - o) / d);
            } else if d < -1e-6 {
                best = best.min(-o / d);
            }
        }
        (best.is_finite() && best > 0.0).then(|| (p + dir * best).clamp(Vec2::ZERO, Vec2::splat(s)))
    }

    /// Is `p` (on the map's boundary) on a land edge, beside the land
    /// rectangle?
    fn on_land_edge(&self, p: Vec2) -> bool {
        let land = &self.layout.land;
        let edge = self.edge_of(p);
        let beside_land = match edge {
            MapEdge::West | MapEdge::East => (land.min.y..=land.max.y).contains(&p.y),
            MapEdge::South | MapEdge::North => (land.min.x..=land.max.x).contains(&p.x),
        };
        !land.is_ocean(edge) && beside_land
    }

    /// The map edge nearest `p`.
    fn edge_of(&self, p: Vec2) -> MapEdge {
        let land = &self.layout.land;
        MapEdge::ALL.into_iter().min_by(|a, b| land.edge_distance(p, *a).total_cmp(&land.edge_distance(p, *b))).expect("four edges")
    }

    /// Unit vector into the map from the edge nearest `p`.
    fn inward(&self, p: Vec2) -> Vec2 {
        match self.edge_of(p) {
            MapEdge::West => Vec2::X,
            MapEdge::East => -Vec2::X,
            MapEdge::South => Vec2::Y,
            MapEdge::North => -Vec2::Y,
        }
    }

    /// Share of the line, beyond [`END_FREE`] of the land side from either
    /// end, within [`NEAR_EDGE`] of a land edge.
    fn near_edge_share(&self, line: &[Vec2]) -> f32 {
        let side = self.layout.land.side;
        let skip = ((side * END_FREE) / SPACING) as usize;
        if line.len() <= 2 * skip + 1 {
            return 0.0;
        }
        let middle = &line[skip..line.len() - skip];
        let near = middle.iter().filter(|p| self.layout.land.land_edge_distance(**p) < side * NEAR_EDGE).count();
        near as f32 / middle.len() as f32
    }

    /// A gentle meander (2-5 m, wavelength 35-60 m) wherever it keeps the
    /// river clear of what it must avoid, fading in from the ends.
    fn meander(&self, line: Vec<Vec2>, size: Size, tier: Tier, mouth: Mouth, rng: &mut SimRng) -> Vec<Vec2> {
        let amplitude = rng.range(2.0, 5.0);
        let wavelength = rng.range(35.0, 60.0);
        let phase = rng.range(0.0, TAU);
        let n = line.len();
        let mut offsets: Vec<f32> = Vec::with_capacity(n);
        for i in 0..n {
            let s = i as f32 * SPACING;
            let fade = (s.min((n - 1 - i) as f32 * SPACING) / 15.0).min(1.0);
            let normal = normal_at(&line, i);
            let end_free = s.min((n - 1 - i) as f32 * SPACING) < STRIP_FREE;
            let fits = |o: f32| {
                let q = line[i] + normal * o;
                self.water_fits(q, size.reach() + tier.margin * 0.5, tier, mouth) && (end_free || self.layout.land.land_edge_distance(q) >= size.reach() + EDGE_STRIP)
            };
            // Halve the swing until the shifted point keeps its distance.
            let mut o = amplitude * fade * ops::sin(s / wavelength * TAU + phase);
            for _ in 0..3 {
                if fits(o) {
                    break;
                }
                o *= 0.5;
            }
            offsets.push(if fits(o) { o } else { 0.0 });
        }
        // Smooth the offsets so a cut swing never kinks the course: the
        // window's mean, or its smallest swing where that is smaller.
        let radius = 8usize;
        let smooth: Vec<f32> = (0..n)
            .map(|i| {
                let window = &offsets[i.saturating_sub(radius)..=(i + radius).min(n - 1)];
                let least = window.iter().copied().fold(f32::INFINITY, |a: f32, b| if b.abs() < a.abs() { b } else { a });
                let mean = window.iter().sum::<f32>() / window.len() as f32;
                if least.abs() < mean.abs() { least } else { mean }
            })
            .collect();
        let shifted: Vec<Vec2> = (0..n).map(|i| line[i] + normal_at(&line, i) * smooth[i]).collect();
        resample(&shifted, SPACING)
    }

    /// Would water reaching `reach` round `p` stay off everything the river
    /// must keep clear of at `tier`? Off-map points always fit.
    fn water_fits(&self, p: Vec2, reach: f32, tier: Tier, mouth: Mouth) -> bool {
        let sea_ok = matches!(mouth, Mouth::Sea(_));
        let (x0, y0, x1, y1) = self.cell_rect(p, reach);
        for y in y0..=y1 {
            for x in x0..=x1 {
                if cell_center(x, y).distance_squared(p) > reach * reach {
                    continue;
                }
                let i = self.idx(x, y);
                let held = self.hold[i] == Hold::Hard || tier.areas_closed && self.hold[i] == Hold::Area;
                let rough = self.terrain[i] != self.ids.grass && self.terrain[i] != self.ids.sand && !(sea_ok && self.coast[i]);
                if held || rough || self.is_river_water(i) {
                    return false;
                }
            }
        }
        true
    }

    /// Paint the river along `points`, place its fords and reserve their
    /// banks, and record it.
    fn lay_river(&mut self, points: Vec<Vec2>, mouth: Mouth, size: Size, rng: &mut SimRng) {
        let n = points.len();
        let length = n as f32 * SPACING;
        let deep: Vec<f32> = (0..n).map(|i| size.deep * (1.0 + i as f32 / n as f32 / 3.0)).collect();
        let half = |i: usize| deep[i] + FORD_EXTRA;
        let at = |s: f32| ((s.max(0.0) / SPACING).round() as usize).min(n - 1);

        // Fords where the river crosses a start corridor...
        let corridors: Vec<(Vec2, Vec2)> = self.layout.corridors().collect();
        let in_corridor: Vec<bool> = points
            .iter()
            .map(|p| corridors.iter().any(|(a, b)| af_geom::point_segment_distance(*p, *a, *b) < CORRIDOR_HALF_WIDTH + 1.0 + size.bank))
            .collect();
        let mut crossings = Vec::new();
        let mut i = 0;
        while i < n {
            if in_corridor[i] {
                let start = i;
                while i < n && in_corridor[i] {
                    i += 1;
                }
                crossings.push((start + i - 1) / 2);
            }
            i += 1;
        }
        // ...and at most FORD_SPACING apart everywhere else.
        let mut fords: Vec<usize> = Vec::new();
        let mut next = rng.range(8.0, 16.0);
        let mut c = 0;
        loop {
            if c < crossings.len() && crossings[c] as f32 * SPACING <= next {
                fords.push(crossings[c]);
                next = crossings[c] as f32 * SPACING + rng.range(FORD_SPACING.0, FORD_SPACING.1);
                c += 1;
                continue;
            }
            if next >= length - 4.0 {
                break;
            }
            fords.push(at(next));
            next += rng.range(FORD_SPACING.0, FORD_SPACING.1);
        }
        fords.extend(&crossings[c..]);
        match mouth {
            Mouth::Edge(_) | Mouth::Sea(_) => {
                // The last deep stretch reaches the map edge or the sea.
                if fords.last().is_none_or(|f| length - *f as f32 * SPACING > FORD_SPACING.0 - 2.0) {
                    fords.push(at(length - rng.range(6.0, 12.0)));
                }
            }
            Mouth::Joins(r) => {
                // A tributary fords all the way into the river it joins, so
                // the two never make one long deep body.
                let into = self.rivers[r].half_widths.iter().copied().fold(0.0, f32::max) * 4.0 / 3.0 + size.bank + 2.0;
                let mut s = length - into - half(n - 1);
                while s < length {
                    fords.push(at(s));
                    s += 4.0;
                }
            }
        }
        fords.sort_unstable();
        fords.dedup();

        // Which points are within a ford.
        let mut ford_point = vec![false; n];
        for f in &fords {
            let reach = (half(*f) / SPACING).ceil() as usize;
            for flag in &mut ford_point[f.saturating_sub(reach)..(f + reach + 1).min(n)] {
                *flag = true;
            }
        }

        // Paint: deep within the core of non-ford points, ford within reach
        // of a ford point, shallow banks elsewhere; corridors only as a
        // ford; never on hard-reserved cells or the open sea.
        const WET: u8 = 1;
        const DEEP: u8 = 2;
        const FORD: u8 = 4;
        let w = self.w as usize;
        let mut flags = vec![0u8; w * w];
        let mut touched = Vec::new();
        for (k, p) in points.iter().enumerate() {
            let reach = deep[k] + size.bank;
            let (x0, y0, x1, y1) = self.cell_rect(*p, reach);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = cell_center(x, y).distance(*p);
                    if d >= reach {
                        continue;
                    }
                    let i = self.idx(x, y);
                    if flags[i] == 0 {
                        touched.push(i);
                    }
                    flags[i] |= WET;
                    if ford_point[k] {
                        flags[i] |= FORD;
                    } else if d < deep[k] {
                        flags[i] |= DEEP;
                    }
                }
            }
        }
        let (deep_id, shallow_id, ford_id, ocean_id) = (self.ids.deep, self.ids.shallow, self.ids.ford, self.ids.ocean);
        for i in touched {
            let f = flags[i];
            let t = match self.hold[i] {
                Hold::Hard => continue,
                _ if self.terrain[i] == ocean_id => continue,
                Hold::Corridor => ford_id,
                _ if f & DEEP != 0 => deep_id,
                _ if f & FORD != 0 => ford_id,
                _ => shallow_id,
            };
            self.terrain[i] = t;
            // A mouth in the coast's shallows is river water now.
            self.coast[i] = false;
        }
        self.touch();

        // Keep every ford's banks open: no mountain, tree, thicket or marsh.
        for f in &fords {
            let reach = (half(*f) / SPACING).ceil() as usize;
            let (lo, hi) = (f.saturating_sub(reach), (f + reach).min(n - 1));
            for k in (lo..=hi).step_by(2) {
                let (p, r) = (points[k], deep[k] + size.bank + APPROACH);
                self.reserve(p, r, Hold::Hard, |q| q.distance_squared(p) <= r * r);
            }
        }

        self.rivers.push(RiverLine { points, half_widths: deep, fords });
    }
}

/// Chaikin corner cutting, `rounds` times, keeping both ends.
fn chaikin(points: &[Vec2], rounds: usize) -> Vec<Vec2> {
    let mut pts = points.to_vec();
    for _ in 0..rounds {
        if pts.len() < 3 {
            break;
        }
        let mut next = Vec::with_capacity(pts.len() * 2);
        next.push(pts[0]);
        for w in pts.windows(2) {
            next.push(w[0] * 0.75 + w[1] * 0.25);
            next.push(w[0] * 0.25 + w[1] * 0.75);
        }
        next.push(*pts.last().expect("non-empty"));
        pts = next;
    }
    pts
}

/// Points every `spacing` m along the polyline `points`, both ends kept.
pub(crate) fn resample(points: &[Vec2], spacing: f32) -> Vec<Vec2> {
    let mut out = vec![points[0]];
    let mut carry = 0.0;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.distance(b);
        if len <= 1e-6 {
            continue;
        }
        let mut t = spacing - carry;
        while t <= len {
            out.push(a + (b - a) * (t / len));
            t += spacing;
        }
        carry = len - (t - spacing);
    }
    let last = *points.last().expect("non-empty");
    let n = out.len();
    if out[n - 1].distance(last) > spacing * 0.3 || n == 1 {
        out.push(last);
    } else {
        out[n - 1] = last;
    }
    out
}

/// Unit normal (left of the direction of travel) of the line at point `i`.
fn normal_at(line: &[Vec2], i: usize) -> Vec2 {
    let (a, b) = (line[i.saturating_sub(2)], line[(i + 2).min(line.len() - 1)]);
    perp((b - a).normalize_or(Vec2::X))
}

/// Does the segment `a`-`b` cross the polyline `line`?
fn crosses(a: Vec2, b: Vec2, line: &[Vec2]) -> bool {
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    line.iter().step_by(4).zip(line.iter().skip(4).step_by(4)).any(|(c, d)| {
        let (c, d) = (*c, *d);
        side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
    })
}

/// Index of the point of `line` nearest `p`.
fn nearest_index(line: &[Vec2], p: Vec2) -> usize {
    let mut best = (f32::INFINITY, 0);
    for (i, q) in line.iter().enumerate() {
        let d = q.distance_squared(p);
        if d < best.0 {
            best = (d, i);
        }
    }
    best.1
}

/// The point of `line` nearest `p`.
fn nearest_on(line: &[Vec2], p: Vec2) -> Vec2 {
    line[nearest_index(line, p)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_even_spacing_and_both_ends() {
        let line = resample(&[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 7.3)], SPACING);
        assert_eq!(line[0], Vec2::ZERO);
        assert_eq!(*line.last().unwrap(), Vec2::new(10.0, 7.3));
        for w in line.windows(2).take(line.len() - 2) {
            let d = w[0].distance(w[1]);
            // A corner shortens one step a little.
            assert!(d <= SPACING + 1e-4 && d > SPACING * 0.7, "{d}");
        }
        let smooth = chaikin(&[Vec2::ZERO, Vec2::new(8.0, 0.0), Vec2::new(8.0, 8.0)], 2);
        assert_eq!((smooth[0], *smooth.last().unwrap()), (Vec2::ZERO, Vec2::new(8.0, 8.0)));
        assert!(smooth.len() > 4);
    }
}
