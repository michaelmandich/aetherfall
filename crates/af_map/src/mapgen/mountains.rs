//! Mountains (`docs/design/maps-and-terrain.md` section 2.4): impassable
//! `mountain` terrain that only flyers cross. Ranges are long, gently
//! curving spines of rock, broad at their peaks and narrow at their saddles,
//! with spurs and foothill knobs, cut by a pass (two on the longest ranges)
//! where a pass makes a useful lane; big single massifs, and massifs against
//! a land edge or in a corner, fill out and frame the map. Mountains keep
//! clear of everything reserved, of the contested spots and the lanes from
//! every start to its spots and from every spot to the main site, and far
//! enough from water and other rock that the widest ground unit walks
//! between them; woods and marsh keep off their feet and the lanes
//! ([`Gen::keep_open`]).

use std::f32::consts::{PI, TAU};

use af_core::ext::map::{MapEdge, MapSize, MountainRange};
use af_core::prelude::*;
use bevy::prelude::*;

use super::noise::Noise;
use super::terrain::APPROACH;
use super::{perp, polar, Gen, Hold};

/// A range's spine length, as a fraction of the land side (Small lands:
/// [`SMALL_RANGE_LENGTH`]).
const RANGE_LENGTH: (f32, f32) = (0.3, 0.56);
const SMALL_RANGE_LENGTH: (f32, f32) = (0.22, 0.4);
/// A range's base half-width: its peaks swell to about 1.4 times this and
/// its saddles dip to about 0.6 (Small lands: [`SMALL_RANGE_HALF_WIDTH`]).
const RANGE_HALF_WIDTH: (f32, f32) = (8.5, 10.5);
const SMALL_RANGE_HALF_WIDTH: (f32, f32) = (7.0, 9.0);
/// The smallest a crowded map shrinks a range to before leaving it out.
const MIN_RANGE_LENGTH: f32 = 24.0;
const MIN_HALF_WIDTH: f32 = 4.5;
/// Ranges at least this long keep a pass, and two from [`TWO_PASSES`].
const PASS_FROM: f32 = 64.0;
const TWO_PASSES: f32 = 200.0;
/// Half-width of a pass: 12-16 m of open ground.
const PASS_HALF_WIDTH: (f32, f32) = (6.0, 8.0);
/// A range narrows to a saddle over this many metres toward a pass.
const SADDLE: f32 = 8.0;
/// Radius of a big single massif and of an edge massif (times the square
/// root of the mountain scale), and of a corner massif's main peak as a
/// fraction of the land side.
const MASSIF_RADIUS: (f32, f32) = (13.0, 19.0);
const EDGE_MASSIF_RADIUS: (f32, f32) = (14.0, 24.0);
const CORNER_MASSIF: (f32, f32) = (0.07, 0.1);
/// The smallest a crowded map shrinks a massif to.
const MIN_MASSIF_RADIUS: f32 = 7.0;
/// Mountains keep this far from water, other rock and dunes (and from the
/// map edge, unless they stand against it): room for a 7 x 7 cell unit.
pub(crate) const WALK_GAP: f32 = 10.0;
/// Trees, thicket and marsh keep this far from mountains, so a mountain's
/// foot never closes with a wood or a marsh what [`WALK_GAP`] keeps open.
const ROCK_CLEARING: f32 = 9.0;
/// Rock keeps a metre beyond this from the lanes from every start to its
/// contested spots and from every spot to the main site...
const LANE_HALF_WIDTH: f32 = 6.0;
/// ...and this far beyond a contested spot's reserved ground.
const SPOT_ROCK_GAP: f32 = 10.0;
/// Trees, thicket and marsh keep this far from a lane's axis, so every
/// contested spot keeps an open way to its starts and the main site.
const LANE_CLEARING: f32 = 4.5;
/// Spine sample spacing.
const SPINE_STEP: f32 = 2.0;
/// The outline wobbles between these fractions of the half-width.
const RAGGED: (f32, f32) = (0.8, 1.2);
/// Candidate spines grown per range and round (the best is painted), and
/// rounds (each a little shorter and thinner) before a range is left out.
const CANDIDATES: u32 = 8;
const ROUNDS: u32 = 5;
/// Candidate places tried per massif (shrinking every so often).
const TRIES: u32 = 72;
/// Heading changes tried per spine step to steer round what is in the way.
const TURNS: [f32; 9] = [0.0, 0.07, -0.07, 0.14, -0.14, 0.21, -0.21, 0.3, -0.3];

/// One polyline of rock: points every [`SPINE_STEP`] m and the half-width
/// at each.
#[derive(Clone, Debug)]
struct Spine {
    points: Vec<Vec2>,
    half: Vec<f32>,
}

impl Spine {
    fn length(&self) -> f32 {
        (self.points.len().max(1) - 1) as f32 * SPINE_STEP
    }

    /// Distance from `p` to the spine, and the arc position of the nearest
    /// point and its half-width there.
    fn nearest(&self, p: Vec2) -> (f32, f32, f32) {
        let mut best = (f32::INFINITY, 0.0, 0.0);
        if self.points.len() == 1 {
            return (p.distance(self.points[0]), 0.0, self.half[0]);
        }
        for (k, w) in self.points.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            let d = p.distance_squared(a + ab * t);
            if d < best.0 {
                best = (d, (k as f32 + t) * SPINE_STEP, self.half[k] + (self.half[k + 1] - self.half[k]) * t);
            }
        }
        (best.0.sqrt(), best.1, best.2)
    }

    /// Unit direction along the spine at point `k`.
    fn tangent(&self, k: usize) -> Vec2 {
        let last = self.points.len() - 1;
        (self.points[(k + 1).min(last)] - self.points[k.saturating_sub(1)]).normalize_or(Vec2::X)
    }

    /// Where segment `a`-`b` crosses the spine (arc position), if it does.
    fn crossing(&self, a: Vec2, b: Vec2) -> Option<f32> {
        self.points.windows(2).enumerate().find_map(|(k, w)| segment_hit(w[0], w[1], a, b).map(|t| (k as f32 + t) * SPINE_STEP))
    }

    /// The widest half-width within `reach` m of arc position `at`.
    fn widest_near(&self, at: f32, reach: f32) -> f32 {
        self.half.iter().enumerate().filter(|(k, _)| (*k as f32 * SPINE_STEP - at).abs() <= reach).map(|(_, h)| *h).fold(0.0, f32::max)
    }
}

/// Where segment `p0`-`p1` crosses segment `a`-`b`, as a fraction along
/// `p0`-`p1`.
fn segment_hit(p0: Vec2, p1: Vec2, a: Vec2, b: Vec2) -> Option<f32> {
    let (r, s) = (p1 - p0, b - a);
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (a - p0).perp_dot(s) / denom;
    let u = (a - p0).perp_dot(r) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

/// A range as it is painted.
struct Range {
    spine: Spine,
    spurs: Vec<Spine>,
    /// Foothill knobs on its flanks (centre, radius).
    knobs: Vec<(Vec2, f32)>,
    /// Arc position and half-width of each pass, in order along the spine.
    passes: Vec<(f32, f32)>,
}

/// What every mountain shares: the outline noise and the lanes rock keeps
/// out of.
struct Rock {
    /// Fine wobble of the outline.
    ragged: Noise,
    /// Coarse lumps along the flanks.
    lumps: Noise,
    /// From every start to its contested spots, and from every spot to the
    /// main site.
    lanes: Vec<(Vec2, Vec2)>,
    /// Contested spots (centre, and how near rock may come).
    spots: Vec<(Vec2, f32)>,
    /// Between neighbouring starts: where a pass makes the most useful lane.
    neighbours: Vec<(Vec2, Vec2)>,
}

impl Rock {
    /// Distance from `p` to the edge of the nearest lane, or to the ground
    /// kept open round the nearest contested spot.
    fn lane_room(&self, p: Vec2) -> f32 {
        let lanes = self.lanes.iter().map(|(a, b)| af_geom::point_segment_distance(p, *a, *b)).fold(f32::INFINITY, f32::min) - LANE_HALF_WIDTH;
        self.spots.iter().map(|(c, r)| p.distance(*c) - r).fold(lanes, f32::min)
    }

    /// How far rock reaches round `p`, as a fraction of the local
    /// half-width (within [`RAGGED`]): a fine wobble and coarse lumps.
    fn wobble(&self, p: Vec2) -> f32 {
        RAGGED.0 + (RAGGED.1 - RAGGED.0) * (0.55 * self.ragged.at(p) + 0.45 * self.lumps.at(p))
    }
}

impl Gen<'_> {
    /// Ranges, big single massifs and edge or corner massifs per the map
    /// size: ranges first (they need the most room), then the massifs.
    pub(super) fn paint_mountains(&mut self, rng: &mut SimRng) {
        let (ranges, massifs, edges) = match self.layout.options.size {
            MapSize::Small => (1, (rng.below(3) == 0) as u32, 1 + rng.below(2)),
            MapSize::Medium => (2 + rng.below(2), rng.below(2), 2 + rng.below(2)),
            MapSize::Large => (2 + rng.below(2), 1, 2 + rng.below(2)),
            MapSize::Huge => (3 + rng.below(2), 1 + rng.below(2), 3 + rng.below(2)),
        };
        let spots = self.layout.spots.iter().zip(&self.layout.spot_radius).map(|(c, r)| (*c, r + SPOT_ROCK_GAP)).collect();
        let rock = Rock { ragged: Noise::new(rng, self.size, 5.0), lumps: Noise::new(rng, self.size, 14.0), lanes: self.lanes(), spots, neighbours: self.neighbour_lines() };
        for _ in 0..ranges {
            self.place_range(rng, &rock);
        }
        for _ in 0..massifs {
            self.place_massif(rng, &rock);
        }
        let mut corners = Vec::new();
        for _ in 0..edges {
            self.place_edge_massif(rng, &rock, &mut corners);
        }
    }

    /// Bigger lands get bigger massifs (by the square root of this): 1 for
    /// a 300 m land square, 0.8-1.6.
    fn mountain_scale(&self) -> f32 {
        (self.layout.land.side / 300.0).clamp(0.8, 1.6)
    }

    /// The lanes rock keeps out of, so no range stands between a start and
    /// its own contested spots (which would favour one neighbour) or
    /// between a spot and the main site.
    pub(crate) fn lanes(&self) -> Vec<(Vec2, Vec2)> {
        let lay = &self.layout;
        let n = lay.starts.len();
        let mut out = Vec::new();
        for (k, spot) in lay.spots.iter().enumerate() {
            let adjacent: Vec<usize> = if n <= 2 { (0..n).collect() } else { vec![k, (k + 1) % n] };
            out.extend(adjacent.into_iter().map(|i| (lay.starts[i], *spot)));
            out.push((*spot, lay.center));
        }
        out
    }

    /// Cells that trees, thicket and marsh keep off: within
    /// [`ROCK_CLEARING`] of a mountain (so a mountain's foot never closes
    /// with a wood or a marsh) or [`LANE_CLEARING`] of a lane (so every
    /// contested spot keeps an open way to its starts and the main site).
    pub(crate) fn keep_open(&self) -> Vec<bool> {
        let mut open: Vec<bool> = self.mountain_field().into_iter().map(|d| d < ROCK_CLEARING).collect();
        for (a, b) in self.lanes() {
            let (x0, y0, x1, y1) = self.cell_rect((a + b) * 0.5, a.distance(b) * 0.5 + LANE_CLEARING);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if af_geom::point_segment_distance(super::cell_center(x, y), a, b) < LANE_CLEARING {
                        let i = self.idx(x, y);
                        open[i] = true;
                    }
                }
            }
        }
        open
    }

    /// The straight lines between neighbouring starts (three or more).
    fn neighbour_lines(&self) -> Vec<(Vec2, Vec2)> {
        let s = &self.layout.starts;
        if s.len() < 3 {
            return Vec::new();
        }
        (0..s.len()).map(|i| (s[i], s[(i + 1) % s.len()])).collect()
    }

    /// Can rock reach `reach` metres round `p`? `edge` mountains may stand
    /// against the map edge.
    fn rock_reaches(&self, rock: &Rock, p: Vec2, reach: f32, edge: bool) -> bool {
        let (edge_room, reserved, _, blocking) = self.room_parts(p);
        reserved.min(rock.lane_room(p)) >= reach + 1.0 && blocking >= reach + WALK_GAP && (edge || edge_room >= reach + WALK_GAP)
    }

    /// Can rock of half-width `half` (its ragged outline reaching
    /// [`RAGGED`]`.1` times that) stand round `p`?
    fn rock_fits(&self, rock: &Rock, p: Vec2, half: f32, edge: bool) -> bool {
        self.rock_reaches(rock, p, half * RAGGED.1, edge)
    }

    /// The widest half-width rock may have round `p` (see
    /// [`Gen::rock_fits`]).
    fn rock_room(&self, rock: &Rock, p: Vec2) -> f32 {
        let (edge_room, reserved, _, blocking) = self.room_parts(p);
        (reserved.min(rock.lane_room(p)) - 1.0).min(blocking - WALK_GAP).min(edge_room - WALK_GAP) / RAGGED.1
    }

    /// A range in the wilds: of a few candidate spines grown from roomy
    /// spots ([`Gen::grow_spine`]), the one with the most rock (a bonus
    /// for one that stands between neighbouring starts), dressed with its
    /// passes, spurs and knobs. Crowded maps get it shorter and thinner,
    /// round by round, before leaving it out.
    fn place_range(&mut self, rng: &mut SimRng, rock: &Rock) {
        let (r_min, r_max) = (self.layout.main().outer() + 18.0, self.wild_reach());
        let small = self.layout.options.size == MapSize::Small;
        let (lengths, widths) = if small { (SMALL_RANGE_LENGTH, SMALL_RANGE_HALF_WIDTH) } else { (RANGE_LENGTH, RANGE_HALF_WIDTH) };
        let mut length = self.layout.land.side * rng.range(lengths.0, lengths.1);
        let mut width = rng.range(widths.0, widths.1);
        for _ in 0..ROUNDS {
            let mut best: Option<(f32, Spine)> = None;
            for _ in 0..CANDIDATES {
                // The roomiest of a few random spots.
                let start = (0..6)
                    .map(|_| self.layout.center + polar(rng.range(0.0, TAU)) * rng.range(r_min, r_max))
                    .max_by(|a, b| self.rock_room(rock, *a).total_cmp(&self.rock_room(rock, *b)))
                    .expect("six candidates");
                if !self.rock_fits(rock, start, width * 0.45, false) {
                    continue;
                }
                let Some(spine) = self.grow_spine(rock, start, length, width, rng) else { continue };
                let score = self.range_score(rock, &spine);
                if best.as_ref().is_none_or(|(s, _)| score > *s) {
                    best = Some((score, spine));
                }
            }
            if let Some((_, spine)) = best {
                let range = self.dress_range(rock, spine, rng);
                self.paint_range(rock, &range);
                return;
            }
            length = (length * 0.8).max(MIN_RANGE_LENGTH);
            width = (width * 0.88).max(MIN_HALF_WIDTH);
        }
    }

    /// How much a range adds: its rock (the sum of its half-widths along
    /// it), a third more when it stands between neighbouring starts.
    fn range_score(&self, rock: &Rock, spine: &Spine) -> f32 {
        let area = spine.half.iter().sum::<f32>() * SPINE_STEP;
        let between = rock.neighbours.iter().any(|(a, b)| spine.crossing(*a, *b).is_some());
        area * if between { 1.3 } else { 1.0 }
    }

    /// A spine through `start`: from the heading with the longest straight
    /// run of room, grown both ways [`SPINE_STEP`] m at a time along a
    /// gentle bend and a slow wave, steering away from whatever is in the
    /// way (one arm takes whatever length the other could not use). The
    /// half-width swells and dips along it round `width` (peaks and
    /// saddles), tapers at the ends and never exceeds the room. None when
    /// it comes out shorter than about half of `length`.
    fn grow_spine(&self, rock: &Rock, start: Vec2, length: f32, width: f32, rng: &mut SimRng) -> Option<Spine> {
        let thin = width * 0.45;
        let steps = (length / SPINE_STEP) as usize;
        let run = |dir: Vec2| (1..=steps / 2).take_while(|i| self.rock_fits(rock, start + dir * *i as f32 * SPINE_STEP, thin, false)).count();
        // Most ranges run out from the middle (between neighbouring starts
        // that makes flanks and lanes), the rest wherever is longest.
        let outward = (start - self.layout.center).normalize_or(Vec2::X);
        let radial = rng.chance(0.6);
        let first = rng.range(0.0, PI);
        let mut best = (0.0, first);
        for k in 0..12 {
            let h = first + k as f32 * PI / 12.0;
            let total = (run(polar(h)) + run(-polar(h))) as f32;
            let total = if radial { total * (0.55 + 0.45 * polar(h).dot(outward).abs()) } else { total };
            if total > best.0 {
                best = (total, h);
            }
        }
        // Up to about 60 degrees of bend end to end, and a slow wave.
        let bend = rng.range(-1.1, 1.1) / steps.max(1) as f32;
        let (amp, wave, phase) = (rng.range(0.08, 0.26), rng.range(60.0, 120.0), rng.range(0.0, TAU));
        let heading = |i: f32| best.1 + bend * i + amp * ops::sin(i * SPINE_STEP / wave * TAU + phase);
        let mut back = self.grow_arm(rock, start, &heading, -1.0, 0, steps / 2, thin);
        let forward = self.grow_arm(rock, start, &heading, 1.0, 0, steps - back.len(), thin);
        if back.len() == steps / 2 && back.len() + forward.len() < steps {
            let from = back.last().copied().unwrap_or(start);
            let more = self.grow_arm(rock, from, &heading, -1.0, back.len(), steps - back.len() - forward.len(), thin);
            back.extend(more);
        }
        let points: Vec<Vec2> = back.into_iter().rev().chain(std::iter::once(start)).chain(forward).collect();
        let n = points.len();
        let total = (n - 1) as f32 * SPINE_STEP;
        if total < (length * 0.45).max(MIN_RANGE_LENGTH) {
            return None;
        }
        let (k1, k2) = (rng.range(0.0, TAU), rng.range(0.0, TAU));
        let (w1, w2) = (rng.range(28.0, 46.0), rng.range(10.0, 16.0));
        let half = (0..n)
            .map(|i| {
                let s = i as f32 * SPINE_STEP;
                let swell = 1.0 + 0.32 * ops::sin(s / w1 * TAU + k1) + 0.14 * ops::sin(s / w2 * TAU + k2);
                let taper = (s.min(total - s) / 20.0).clamp(0.0, 1.0);
                (width * swell * (0.35 + 0.65 * taper.sqrt())).min(self.rock_room(rock, points[i]))
            })
            .collect();
        Some(Spine { points, half })
    }

    /// Up to `steps` spine points on from `from`, which lies `done` steps
    /// out along the arm: forward (`sign` 1) or backward (-1) along
    /// `heading` (the spine's planned heading by step), steering away from
    /// whatever is in the way and easing back onto the plan.
    fn grow_arm(&self, rock: &Rock, from: Vec2, heading: &impl Fn(f32) -> f32, sign: f32, done: usize, steps: usize, thin: f32) -> Vec<Vec2> {
        let mut out = Vec::new();
        let (mut p, mut dev) = (from, 0.0f32);
        for k in done..done + steps {
            let planned = heading(sign * (k as f32 + 0.5)) + if sign < 0.0 { PI } else { 0.0 };
            let clear = |h: f32| [1.0, 3.0, 5.0].into_iter().all(|ahead| self.rock_fits(rock, p + polar(h) * ahead * SPINE_STEP, thin, false));
            let Some(d) = TURNS.into_iter().map(|t| dev * 0.9 + t).find(|d| d.abs() <= 1.2 && clear(planned + d)) else { break };
            dev = d;
            p += polar(planned + dev) * SPINE_STEP;
            out.push(p);
        }
        out
    }

    /// The passes, spurs and foothill knobs of a range along `spine`.
    fn dress_range(&self, rock: &Rock, spine: Spine, rng: &mut SimRng) -> Range {
        let len = spine.length();
        let n = spine.points.len();
        let windows: &[(f32, f32)] = if len < PASS_FROM {
            &[]
        } else if len < TWO_PASSES {
            &[(0.3, 0.7)]
        } else {
            &[(0.2, 0.42), (0.58, 0.8)]
        };
        // Where the range stands between neighbouring starts, a pass there
        // is their short way round.
        let crossings: Vec<f32> = rock.neighbours.iter().filter_map(|(a, b)| spine.crossing(*a, *b)).collect();
        let mut passes = Vec::new();
        for (lo, hi) in windows {
            let half = rng.range(PASS_HALF_WIDTH.0, PASS_HALF_WIDTH.1);
            let phase = rng.range(0.0, TAU);
            let (k0, k1) = (((len * lo) / SPINE_STEP) as usize, (((len * hi) / SPINE_STEP) as usize).min(n - 1));
            let score = |k: usize| self.pass_score(&spine, k, &crossings, phase);
            let k = (k0..=k1).max_by(|a, b| score(*a).total_cmp(&score(*b))).unwrap_or(k0);
            passes.push((k as f32 * SPINE_STEP, half));
        }
        let near_pass = |at: f32, by: f32| passes.iter().any(|(s, h)| (at - s).abs() < h + by);

        // Spurs: short side ridges away from the passes and the ends.
        let mut spurs = Vec::new();
        let count = 1 + rng.below(3) + (len > 120.0) as u32;
        for _ in 0..count {
            if n < 12 {
                break;
            }
            let k = 5 + rng.below((n - 10) as u32) as usize;
            let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
            let lean = rng.range(-0.7, 0.7);
            let (reach, curl, base) = (rng.range(14.0, 30.0), rng.range(-0.06, 0.06), spine.half[k] * rng.range(0.55, 0.75));
            if near_pass(k as f32 * SPINE_STEP, 18.0) {
                continue;
            }
            let along = spine.tangent(k);
            let mut h = super::angle_of(perp(along) * side + along * lean);
            let steps = (reach / SPINE_STEP).ceil() as usize;
            let mut points = vec![spine.points[k]];
            let mut half = vec![base];
            for i in 1..=steps {
                h += curl;
                let p = points[i - 1] + polar(h) * SPINE_STEP;
                let w = base * (1.0 - i as f32 / (steps + 1) as f32 * 0.7);
                if !self.rock_fits(rock, p, w, false) {
                    break;
                }
                points.push(p);
                half.push(w);
            }
            if points.len() >= 5 {
                spurs.push(Spine { points, half });
            }
        }

        // Foothill knobs bulging from the flanks.
        let mut knobs = Vec::new();
        for _ in 0..2 + rng.below(4) {
            let k = 2 + rng.below((n.max(5) - 4) as u32) as usize;
            let k = k.min(n - 1);
            let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
            let (out, size) = (rng.range(0.75, 1.0), rng.range(0.35, 0.55));
            if near_pass(k as f32 * SPINE_STEP, 14.0) {
                continue;
            }
            let r = (spine.half[k] * size).max(3.0);
            let c = spine.points[k] + perp(spine.tangent(k)) * side * spine.half[k] * out;
            if self.rock_fits(rock, c, r, false) {
                knobs.push((c, r));
            }
        }
        Range { spine, spurs, knobs, passes }
    }

    /// How good a pass at spine point `k` is: open ground on both sides
    /// (so it leads somewhere), near where the range stands between
    /// neighbouring starts, at a saddle, plus a gentle variation.
    fn pass_score(&self, spine: &Spine, k: usize, crossings: &[f32], phase: f32) -> f32 {
        let p = spine.points[k];
        let side = perp(spine.tangent(k)) * (spine.half[k] * RAGGED.1 + 8.0);
        let open = |q: Vec2| {
            let (edge, _, _, blocking) = self.room_parts(q);
            blocking.min(edge).clamp(0.0, 24.0)
        };
        let s = k as f32 * SPINE_STEP;
        let lane = crossings.iter().map(|x| (1.0 - (s - x).abs() / 40.0).max(0.0)).fold(0.0, f32::max) * 30.0;
        let saddle = (16.0 - spine.half[k]).max(0.0) * 0.5;
        open(p + side).min(open(p - side)) + lane + saddle + 4.0 * ops::sin(s * 0.37 + phase)
    }

    /// Paint a range and publish it: its passes (and [`APPROACH`] beyond
    /// the rock on each side) are reserved first, so nothing closes them;
    /// the spine is published split at its passes, then each spur and knob.
    fn paint_range(&mut self, rock: &Rock, range: &Range) {
        let spine = &range.spine;
        for (at, half) in range.passes.iter().copied() {
            let across = spine.widest_near(at, half + SADDLE) * RAGGED.1 + APPROACH;
            let center = spine.points[((at / SPINE_STEP).round() as usize).min(spine.points.len() - 1)];
            self.reserve(center, half + across + SPINE_STEP, Hold::Hard, |p| {
                let (d, s, _) = spine.nearest(p);
                (s - at).abs() < half && d <= across
            });
        }
        let (grass, marsh, mountain) = (self.ids.grass, self.ids.marsh, self.ids.mountain);
        // The range narrows to a saddle toward each pass.
        let saddle = |s: f32| range.passes.iter().map(|(at, half)| (((s - at).abs() - half) / SADDLE).clamp(0.7, 1.0)).fold(1.0, f32::min);
        let lines: Vec<&Spine> = std::iter::once(spine).chain(&range.spurs).collect();
        let reach = lines.iter().flat_map(|l| l.half.iter()).copied().fold(0.0, f32::max) * RAGGED.1 + 1.0;
        let (lo, hi) = lines
            .iter()
            .flat_map(|l| l.points.iter().copied())
            .chain(range.knobs.iter().map(|(c, _)| *c))
            .fold((Vec2::INFINITY, Vec2::NEG_INFINITY), |(lo, hi), p| (lo.min(p), hi.max(p)));
        let (x0, y0, _, _) = self.cell_rect(lo, reach);
        let (_, _, x1, y1) = self.cell_rect(hi, reach);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let i = self.idx(x, y);
                if self.reserved(i) || !(self.terrain[i] == grass || self.terrain[i] == marsh) {
                    continue;
                }
                let p = super::cell_center(x, y);
                let wobble = rock.wobble(p);
                let (d, s, h) = spine.nearest(p);
                let rocky = d < h * saddle(s) * wobble
                    || range.spurs.iter().any(|sp| {
                        let (d, _, h) = sp.nearest(p);
                        d < h * wobble
                    })
                    || range.knobs.iter().any(|(c, r)| p.distance(*c) < r * wobble);
                if rocky {
                    self.terrain[i] = mountain;
                }
            }
        }
        self.touch();
        let last = spine.points.len() - 1;
        let mut from = 0;
        for (at, half) in &range.passes {
            let to = (((at - half) / SPINE_STEP).floor().max(0.0) as usize).min(last);
            if to > from {
                self.mountains.push(MountainRange { spine: spine.points[from..=to].to_vec(), half_widths: spine.half[from..=to].to_vec() });
            }
            from = (((at + half) / SPINE_STEP).ceil() as usize).min(last);
        }
        if last > from {
            self.mountains.push(MountainRange { spine: spine.points[from..=last].to_vec(), half_widths: spine.half[from..=last].to_vec() });
        }
        for sp in &range.spurs {
            self.mountains.push(MountainRange { spine: sp.points.clone(), half_widths: sp.half.clone() });
        }
        for (c, r) in &range.knobs {
            self.mountains.push(MountainRange { spine: vec![*c], half_widths: vec![*r] });
        }
    }

    /// A big single massif somewhere in the wilds: a main peak with two to
    /// four lesser ones round its flanks.
    fn place_massif(&mut self, rng: &mut SimRng, rock: &Rock) {
        let (r_min, r_max) = (self.layout.main().outer() + 18.0, self.wild_reach());
        let mut radius = rng.range(MASSIF_RADIUS.0, MASSIF_RADIUS.1) * self.mountain_scale().sqrt();
        for attempt in 0..TRIES {
            if attempt > 0 && attempt % 12 == 0 {
                radius = (radius * 0.85).max(MIN_MASSIF_RADIUS);
            }
            let c = self.layout.center + polar(rng.range(0.0, TAU)) * rng.range(r_min, r_max);
            if !self.rock_fits(rock, c, radius, false) {
                continue;
            }
            let mut lumps = vec![(c, radius)];
            for _ in 0..2 + rng.below(3) {
                let r = radius * rng.range(0.4, 0.65);
                let q = c + polar(rng.range(0.0, TAU)) * radius * rng.range(0.55, 0.95);
                if self.rock_fits(rock, q, r, false) {
                    lumps.push((q, r));
                }
            }
            self.paint_lumps(rock, &lumps);
            return;
        }
    }

    /// A massif against a land edge, or in a corner where two land edges
    /// meet (each corner once), partly off the map: a main peak with lesser
    /// ones strung along the edge, framing the map and filling its dead
    /// corners.
    fn place_edge_massif(&mut self, rng: &mut SimRng, rock: &Rock, used: &mut Vec<(MapEdge, MapEdge)>) {
        let land = self.layout.land.clone();
        let edges: Vec<MapEdge> = MapEdge::ALL.into_iter().filter(|e| !land.is_ocean(*e)).collect();
        if edges.is_empty() {
            return;
        }
        let corners: Vec<(MapEdge, MapEdge)> = [MapEdge::West, MapEdge::East]
            .into_iter()
            .flat_map(|a| [(a, MapEdge::South), (a, MapEdge::North)])
            .filter(|(a, b)| edges.contains(a) && edges.contains(b) && !used.contains(&(*a, *b)))
            .collect();
        let scale = self.mountain_scale().sqrt();
        let (mut corner_r, mut edge_r) = (land.side * rng.range(CORNER_MASSIF.0, CORNER_MASSIF.1), rng.range(EDGE_MASSIF_RADIUS.0, EDGE_MASSIF_RADIUS.1) * scale);
        for attempt in 0..TRIES {
            if attempt > 0 && attempt % 12 == 0 {
                corner_r = (corner_r * 0.85).max(MIN_MASSIF_RADIUS);
                edge_r = (edge_r * 0.85).max(MIN_MASSIF_RADIUS);
            }
            let corner = (!corners.is_empty() && rng.chance(0.6)).then(|| corners[rng.below(corners.len() as u32) as usize]);
            let lumps = match corner {
                Some((a, b)) => self.corner_lumps(a, b, corner_r, rng),
                None => self.edge_lumps(edges[rng.below(edges.len() as u32) as usize], edge_r, rng),
            };
            if !self.lump_fits(rock, lumps[0]) {
                continue;
            }
            let lumps: Vec<(Vec2, f32)> = lumps.into_iter().enumerate().filter(|(k, l)| *k == 0 || self.lump_fits(rock, *l)).map(|(_, l)| l).collect();
            used.extend(corner);
            self.paint_lumps(rock, &lumps);
            return;
        }
    }

    /// A corner massif's peaks: the main one round the corner where edges
    /// `a` (west or east) and `b` (south or north) meet, and one or two
    /// lesser ones along each edge.
    fn corner_lumps(&self, a: MapEdge, b: MapEdge, radius: f32, rng: &mut SimRng) -> Vec<(Vec2, f32)> {
        let corner = Vec2::new(if a == MapEdge::West { 0.0 } else { self.size }, if b == MapEdge::South { 0.0 } else { self.size });
        let inward = Vec2::new(if a == MapEdge::West { 1.0 } else { -1.0 }, if b == MapEdge::South { 1.0 } else { -1.0 });
        let mut lumps = vec![(corner + inward.normalize() * radius * rng.range(-0.1, 0.35), radius)];
        for (along, normal) in [(Vec2::new(inward.x, 0.0), Vec2::new(0.0, inward.y)), (Vec2::new(0.0, inward.y), Vec2::new(inward.x, 0.0))] {
            let (mut d, mut r) = (radius * rng.range(0.7, 1.0), radius * rng.range(0.5, 0.7));
            for _ in 0..1 + rng.below(2) {
                lumps.push((corner + along * d + normal * r * rng.range(-0.3, 0.15), r));
                d += r * rng.range(0.9, 1.4);
                r *= rng.range(0.6, 0.85);
            }
        }
        lumps
    }

    /// An edge massif's peaks on `edge`: the main one a little off the map
    /// or just inside it, and lesser ones strung along the edge either side.
    fn edge_lumps(&self, edge: MapEdge, radius: f32, rng: &mut SimRng) -> Vec<(Vec2, f32)> {
        let land = &self.layout.land;
        let (lo, hi) = match edge {
            MapEdge::West | MapEdge::East => (land.min.y, land.max.y),
            MapEdge::South | MapEdge::North => (land.min.x, land.max.x),
        };
        let at = |along: f32, out: f32| match edge {
            MapEdge::West => Vec2::new(out, along),
            MapEdge::East => Vec2::new(self.size - out, along),
            MapEdge::South => Vec2::new(along, out),
            MapEdge::North => Vec2::new(along, self.size - out),
        };
        let along = lo + (hi - lo) * rng.range(0.15, 0.85);
        let mut lumps = vec![(at(along, rng.range(-0.3, 0.2) * radius), radius)];
        for dir in [-1.0, 1.0] {
            let (mut d, mut r) = (radius * rng.range(0.7, 1.0), radius * rng.range(0.45, 0.75));
            for _ in 0..rng.below(3) {
                lumps.push((at(along + dir * d, rng.range(-0.3, 0.15) * r), r));
                d += r * rng.range(0.9, 1.4);
                r *= rng.range(0.6, 0.85);
            }
        }
        lumps
    }

    /// Does a round peak of `radius` round `c` (which may lie off the map,
    /// against an edge) fit? Rings of samples over its part on the map.
    fn lump_fits(&self, rock: &Rock, (c, radius): (Vec2, f32)) -> bool {
        let on_map = |q: Vec2| q.x >= 0.0 && q.y >= 0.0 && q.x < self.size && q.y < self.size;
        [(0.0, 1), (0.4, 8), (0.8, 12), (1.1, 16)].into_iter().all(|(f, count)| {
            (0..count).all(|k| {
                let q = c + polar(k as f32 * TAU / count as f32 + f) * radius * f;
                let reach = if f == 0.0 { radius * RAGGED.1 } else { radius * (RAGGED.1 - f + 0.25) };
                !on_map(q) || self.rock_reaches(rock, q, reach.max(1.0), true)
            })
        })
    }

    /// Paint round peaks (centre, radius) with a ragged outline as one
    /// massif, and publish each.
    fn paint_lumps(&mut self, rock: &Rock, lumps: &[(Vec2, f32)]) {
        let (grass, marsh, mountain) = (self.ids.grass, self.ids.marsh, self.ids.mountain);
        let (lo, hi) = lumps.iter().fold((Vec2::INFINITY, Vec2::NEG_INFINITY), |(lo, hi), (c, r)| (lo.min(*c - *r * RAGGED.1), hi.max(*c + *r * RAGGED.1)));
        let (x0, y0, _, _) = self.cell_rect(lo, 1.0);
        let (_, _, x1, y1) = self.cell_rect(hi, 1.0);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let i = self.idx(x, y);
                if self.reserved(i) || !(self.terrain[i] == grass || self.terrain[i] == marsh) {
                    continue;
                }
                let p = super::cell_center(x, y);
                let wobble = rock.wobble(p);
                if lumps.iter().any(|(c, r)| p.distance(*c) < r * wobble) {
                    self.terrain[i] = mountain;
                }
            }
        }
        self.touch();
        for (c, r) in lumps {
            self.mountains.push(MountainRange { spine: vec![*c], half_widths: vec![*r] });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_spine_point_gives_distance_arc_and_width() {
        let spine = Spine { points: (0..=10).map(|i| Vec2::new(i as f32 * SPINE_STEP, 0.0)).collect(), half: (0..=10).map(|i| 4.0 + i as f32).collect() };
        assert_eq!(spine.length(), 20.0);
        let (d, at, h) = spine.nearest(Vec2::new(7.0, 3.0));
        assert!((d - 3.0).abs() < 1e-5 && (at - 7.0).abs() < 1e-5 && (h - 7.5).abs() < 1e-5, "{d} {at} {h}");
        let (d, at, _) = spine.nearest(Vec2::new(-4.0, 3.0));
        assert!((d - 5.0).abs() < 1e-5 && at == 0.0);
        let massif = Spine { points: vec![Vec2::ZERO], half: vec![9.0] };
        assert_eq!(massif.nearest(Vec2::new(3.0, 4.0)), (5.0, 0.0, 9.0));
        // A line across the spine at x = 5 crosses it 5 m along.
        assert_eq!(spine.crossing(Vec2::new(5.0, -3.0), Vec2::new(5.0, 3.0)), Some(5.0));
        assert_eq!(spine.crossing(Vec2::new(5.0, 1.0), Vec2::new(5.0, 3.0)), None);
        assert_eq!(spine.widest_near(4.0, 2.0), 7.0);
    }

    #[test]
    fn a_pass_stays_open_and_reserved_through_its_range() {
        let data = af_core::testing::test_data();
        let mut g = Gen::bare(&data, 2, 300.0, 11);
        let (center, room) = g.roomiest(g.layout.main().outer() + 20.0, g.wild_reach()).expect("a spot");
        assert!(room > 26.0, "room for the test range: {room}");
        let mut rng = SimRng::new(3);
        let rock = Rock { ragged: Noise::new(&mut rng, g.size, 5.0), lumps: Noise::new(&mut rng, g.size, 14.0), lanes: Vec::new(), spots: Vec::new(), neighbours: Vec::new() };
        // A straight range 48 m long, 8 m half-wide, with a 14 m pass in
        // its middle.
        let from = center - Vec2::X * 24.0;
        let spine = Spine { points: (0..=24).map(|i| from + Vec2::X * i as f32 * SPINE_STEP).collect(), half: vec![8.0; 25] };
        let range = Range { spine, spurs: Vec::new(), knobs: Vec::new(), passes: vec![(24.0, 7.0)] };
        g.paint_range(&rock, &range);
        let (mountain, grass) = (g.ids.mountain, g.ids.grass);
        let cell = |p: Vec2| g.idx_at(p).map(|i| (g.terrain[i], g.reserved(i))).expect("on the map");
        for dx in [-18.0, -12.0, 12.0, 18.0] {
            assert_eq!(cell(center + Vec2::X * dx).0, mountain, "the range {dx} m along");
        }
        // The pass: 14 m of open, reserved ground right across the range.
        for dx in [-6.5, -3.0, 0.0, 3.0, 6.5] {
            for dy in [-9.0, -4.0, 0.0, 4.0, 9.0] {
                assert_eq!(cell(center + Vec2::new(dx, dy)), (grass, true), "the pass at ({dx}, {dy})");
            }
        }
        // Published as the spine either side of its pass.
        assert_eq!(g.mountains.len(), 2);
        assert!(g.mountains.iter().all(|m| m.spine.len() >= 8 && m.spine.iter().all(|p| (p.x - center.x).abs() >= 6.0)));
    }
}
