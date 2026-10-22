//! Mountains (`docs/design/maps-and-terrain.md` section 2.4): impassable
//! `mountain` terrain that only flyers cross, as ranges (a spine of a few
//! gently turning segments, wider at its peaks and narrower at its saddles,
//! with spurs), single round massifs, and massifs against a land edge or in
//! a corner. Long ranges keep a pass. Mountains keep clear of everything
//! reserved and far enough from water and other rock that the widest ground
//! unit can walk between them.

use std::f32::consts::{PI, TAU};

use af_core::ext::map::{MapEdge, MapSize, MountainRange};
use af_core::prelude::*;
use bevy::prelude::*;

use super::noise::Noise;
use super::terrain::APPROACH;
use super::{perp, polar, Gen, Hold};

/// Total length of a range's spine, and its half-width.
const RANGE_LENGTH: (f32, f32) = (30.0, 90.0);
const RANGE_HALF_WIDTH: (f32, f32) = (5.0, 10.0);
/// The smallest a crowded map shrinks a range to before leaving it out.
const MIN_RANGE_LENGTH: f32 = 22.0;
const MIN_HALF_WIDTH: f32 = 4.0;
/// Ranges longer than this keep one pass, 10-12 m wide.
const PASS_FROM: f32 = 60.0;
const PASS_HALF_WIDTH: (f32, f32) = (5.0, 6.0);
/// Radius of a single massif and of an edge massif.
const MASSIF_RADIUS: (f32, f32) = (8.0, 14.0);
const EDGE_MASSIF_RADIUS: (f32, f32) = (14.0, 26.0);
/// Mountains keep this far from water, other rock and dunes (and from the map
/// edge, unless they stand against it): room for a 7 x 7 cell unit.
const WALK_GAP: f32 = 10.0;
/// Spine sample spacing.
const SPINE_STEP: f32 = 2.0;
/// The outline wobbles between these fractions of the half-width.
const RAGGED: (f32, f32) = (0.8, 1.2);
/// Candidate places tried per mountain (shrinking every so often).
const TRIES: u32 = 72;

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
            let d = p.distance(a + ab * t);
            if d < best.0 {
                best = (d, (k as f32 + t) * SPINE_STEP, self.half[k] + (self.half[k + 1] - self.half[k]) * t);
            }
        }
        best
    }
}

impl Gen<'_> {
    /// Ranges, single massifs and edge massifs per the map size.
    pub(super) fn paint_mountains(&mut self, rng: &mut SimRng) {
        let (ranges, massifs, edges) = match self.layout.options.size {
            MapSize::Small => (1, rng.below(2), rng.below(2)),
            MapSize::Medium => (1 + rng.below(2), 1, 1 + rng.below(2)),
            MapSize::Large => (2 + rng.below(2), 1 + rng.below(2), 1 + rng.below(2)),
            MapSize::Huge => (3 + rng.below(2), 1 + rng.below(2), 2 + rng.below(2)),
        };
        let ragged = Noise::new(rng, self.size, 5.0);
        for _ in 0..ranges {
            self.place_range(rng, &ragged);
        }
        for _ in 0..massifs {
            self.place_massif(rng, &ragged);
        }
        for _ in 0..edges {
            self.place_edge_massif(rng, &ragged);
        }
    }

    /// Bigger lands get longer ranges (and bigger massifs, by the square
    /// root of this): 1 for a 300 m land square, 0.8-1.6.
    fn mountain_scale(&self) -> f32 {
        (self.layout.land.side / 300.0).clamp(0.8, 1.6)
    }

    /// Can rock reach `half` round spine point `p`? `edge` mountains may
    /// stand against the map edge.
    fn rock_fits(&self, p: Vec2, half: f32, edge: bool) -> bool {
        let (edge_room, reserved, _, blocking) = self.room_parts(p);
        let reach = half * RAGGED.1;
        reserved >= reach + 1.0 && blocking >= reach + WALK_GAP && (edge || edge_room >= reach + WALK_GAP)
    }

    /// The widest half-width rock may have round `p` (see
    /// [`Gen::rock_fits`]).
    fn rock_room(&self, p: Vec2) -> f32 {
        let (edge_room, reserved, _, blocking) = self.room_parts(p);
        (reserved - 1.0).min(blocking - WALK_GAP).min(edge_room - WALK_GAP) / RAGGED.1
    }

    /// A range between the main site and the land edge: a spine grown both
    /// ways from a roomy spot ([`Gen::grow_spine`]), its width varying along
    /// it (peaks and saddles) and narrowing wherever the open ground does,
    /// a spur or two, and a pass when it is long.
    fn place_range(&mut self, rng: &mut SimRng, ragged: &Noise) {
        let (r_min, r_max) = (self.layout.main().outer() + 18.0, self.wild_reach());
        let scale = self.mountain_scale();
        let mut length = rng.range(RANGE_LENGTH.0, RANGE_LENGTH.1) * scale;
        let mut width = rng.range(RANGE_HALF_WIDTH.0, RANGE_HALF_WIDTH.1);
        for attempt in 0..TRIES {
            if attempt > 0 && attempt % 12 == 0 {
                length = (length * 0.85).max(MIN_RANGE_LENGTH);
                width = (width * 0.9).max(MIN_HALF_WIDTH);
            }
            // The roomiest of a few random spots.
            let start = (0..6)
                .map(|_| self.layout.center + polar(rng.range(0.0, TAU)) * rng.range(r_min, r_max))
                .max_by(|a, b| self.rock_room(*a).total_cmp(&self.rock_room(*b)))
                .expect("six candidates");
            if !self.rock_fits(start, width, false) {
                continue;
            }
            let Some(spine) = self.grow_spine(start, length, width, rng) else { continue };
            // Spurs: short side branches away from the pass.
            let pass = (spine.length() > PASS_FROM).then(|| (spine.length() * rng.range(0.35, 0.65), rng.range(PASS_HALF_WIDTH.0, PASS_HALF_WIDTH.1)));
            let mut spurs = Vec::new();
            for _ in 0..rng.below(3) {
                let k = 2 + rng.below(spine.points.len().saturating_sub(4).max(1) as u32) as usize;
                let at = k as f32 * SPINE_STEP;
                if pass.is_some_and(|(s, _)| (at - s).abs() < 20.0) || k + 1 >= spine.points.len() {
                    continue;
                }
                let along = (spine.points[k + 1] - spine.points[k - 1]).normalize_or(Vec2::X);
                let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
                let dir = (perp(along) * side + along * rng.range(-0.6, 0.6)).normalize_or(Vec2::X);
                let reach = rng.range(8.0, 16.0);
                let steps = (reach / SPINE_STEP).ceil() as usize;
                let base = spine.half[k] * 0.7;
                let spur = Spine {
                    points: (0..=steps).map(|i| spine.points[k] + dir * i as f32 * SPINE_STEP).collect(),
                    half: (0..=steps).map(|i| base * (1.0 - i as f32 / (steps + 1) as f32 * 0.7)).collect(),
                };
                if spur.points.iter().zip(&spur.half).all(|(p, h)| self.rock_fits(*p, *h, false)) {
                    spurs.push(spur);
                }
            }
            self.paint_range(&spine, &spurs, pass, ragged);
            return;
        }
    }

    /// A spine through `start`: from the heading with the longest straight
    /// run of room, grown both ways [`SPINE_STEP`] m at a time (one arm
    /// taking whatever length the other could not use), turning gently (a
    /// slow random drift, steered away from what is in the way). The
    /// half-width swells and dips along it round `width`, tapers at the ends
    /// and never exceeds the room. None when it comes out shorter than half
    /// of `length`.
    fn grow_spine(&self, start: Vec2, length: f32, width: f32, rng: &mut SimRng) -> Option<Spine> {
        let thin = width * 0.45;
        let steps = (length / SPINE_STEP) as usize;
        let run = |dir: Vec2| (1..=steps / 2).take_while(|i| self.rock_fits(start + dir * *i as f32 * SPINE_STEP, thin, false)).count();
        let first = rng.range(0.0, PI);
        let mut best = (0, first);
        for k in 0..12 {
            let h = first + k as f32 * PI / 12.0;
            let total = run(polar(h)) + run(-polar(h));
            if total > best.0 {
                best = (total, h);
            }
        }
        let backward = self.grow_arm(start, best.1 + PI, steps / 2, thin, rng);
        let forward = self.grow_arm(start, best.1, steps - backward.len(), thin, rng);
        let points: Vec<Vec2> = backward.into_iter().rev().chain(std::iter::once(start)).chain(forward).collect();
        let n = points.len();
        if ((n - 1) as f32 * SPINE_STEP) < (length * 0.5).max(MIN_RANGE_LENGTH) {
            return None;
        }
        let (k1, k2) = (rng.range(0.0, TAU), rng.range(0.0, TAU));
        let (w1, w2) = (rng.range(20.0, 40.0), rng.range(9.0, 16.0));
        let half = (0..n)
            .map(|i| {
                let s = i as f32 * SPINE_STEP;
                let swell = 1.0 + 0.15 * ops::sin(s / w1 * TAU + k1) + 0.08 * ops::sin(s / w2 * TAU + k2);
                let taper = (s.min((n - 1 - i) as f32 * SPINE_STEP) / 14.0).clamp(0.0, 1.0);
                (width * swell * (0.45 + 0.55 * taper.sqrt())).min(self.rock_room(points[i]))
            })
            .collect();
        Some(Spine { points, half })
    }

    /// Up to `steps` spine points from `from`, heading `heading`.
    fn grow_arm(&self, from: Vec2, heading: f32, steps: usize, thin: f32, rng: &mut SimRng) -> Vec<Vec2> {
        let mut out = Vec::new();
        let (mut p, mut h, mut drift) = (from, heading, 0.0f32);
        for _ in 0..steps {
            drift = (drift + rng.range(-0.03, 0.03)).clamp(-0.06, 0.06);
            let clear = |nh: f32| self.rock_fits(p + polar(nh) * SPINE_STEP, thin, false) && self.rock_fits(p + polar(nh) * 4.0 * SPINE_STEP, thin, false);
            let Some(nh) = [0.0, 0.1, -0.1, 0.2, -0.2, 0.3, -0.3].into_iter().map(|turn| h + drift + turn).find(|nh| clear(*nh)) else { break };
            h = nh;
            p += polar(h) * SPINE_STEP;
            out.push(p);
        }
        out
    }

    /// Paint a range (and reserve its pass, plus [`APPROACH`] on each side,
    /// so nothing closes it), and publish it: the spine (split at the pass)
    /// and each spur.
    fn paint_range(&mut self, spine: &Spine, spurs: &[Spine], pass: Option<(f32, f32)>, ragged: &Noise) {
        let (grass, marsh, mountain) = (self.ids.grass, self.ids.marsh, self.ids.mountain);
        // The range narrows to a saddle toward its pass.
        let saddle = |s: f32| pass.map_or(1.0, |(at, half)| (((s - at).abs() - half) / 12.0).clamp(0.45, 1.0));
        let lines: Vec<&Spine> = std::iter::once(spine).chain(spurs).collect();
        let reach = lines.iter().flat_map(|l| l.half.iter()).copied().fold(0.0, f32::max) * RAGGED.1 + 1.0;
        let (lo, hi) = lines.iter().flat_map(|l| l.points.iter()).fold((Vec2::INFINITY, Vec2::NEG_INFINITY), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let (x0, y0, _, _) = self.cell_rect(lo, reach);
        let (_, _, x1, y1) = self.cell_rect(hi, reach);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let i = self.idx(x, y);
                if self.reserved(i) || !(self.terrain[i] == grass || self.terrain[i] == marsh) {
                    continue;
                }
                let p = super::cell_center(x, y);
                let wobble = RAGGED.0 + (RAGGED.1 - RAGGED.0) * ragged.at(p);
                let (d, s, h) = spine.nearest(p);
                let in_pass = pass.is_some_and(|(at, half)| (s - at).abs() < half);
                let on_spine = !in_pass && d < h * saddle(s) * wobble;
                let on_spur = spurs.iter().any(|sp| {
                    let (d, _, h) = sp.nearest(p);
                    d < h * wobble
                });
                if on_spine || on_spur {
                    self.terrain[i] = mountain;
                }
            }
        }
        self.touch();
        if let Some((at, half)) = pass {
            let k = ((at / SPINE_STEP).round() as usize).min(spine.points.len() - 1);
            let across = spine.half[k] * RAGGED.1 + APPROACH;
            let center = spine.points[k];
            let s = spine.clone();
            self.reserve(center, half + across + SPINE_STEP, Hold::Hard, |p| {
                let (d, sp, _) = s.nearest(p);
                (sp - at).abs() < half && d <= across
            });
            let split = ((at - half) / SPINE_STEP).floor().max(1.0) as usize;
            let resume = (((at + half) / SPINE_STEP).ceil() as usize).min(spine.points.len() - 1);
            for (a, b) in [(0, split), (resume, spine.points.len() - 1)] {
                if b > a {
                    self.mountains.push(MountainRange { spine: spine.points[a..=b].to_vec(), half_widths: spine.half[a..=b].to_vec() });
                }
            }
        } else {
            self.mountains.push(MountainRange { spine: spine.points.clone(), half_widths: spine.half.clone() });
        }
        for sp in spurs {
            self.mountains.push(MountainRange { spine: sp.points.clone(), half_widths: sp.half.clone() });
        }
    }

    /// A single round massif somewhere in the wilds.
    fn place_massif(&mut self, rng: &mut SimRng, ragged: &Noise) {
        let (r_min, r_max) = (self.layout.main().outer() + 18.0, self.wild_reach());
        let mut radius = rng.range(MASSIF_RADIUS.0, MASSIF_RADIUS.1) * self.mountain_scale().sqrt();
        for attempt in 0..TRIES {
            if attempt > 0 && attempt % 12 == 0 {
                radius = (radius * 0.85).max(MIN_HALF_WIDTH * 1.5);
            }
            let c = self.layout.center + polar(rng.range(0.0, TAU)) * rng.range(r_min, r_max);
            if self.rock_fits(c, radius, false) {
                self.paint_massif(c, radius, ragged);
                return;
            }
        }
    }

    /// A massif against a land edge or in a corner, partly off the map,
    /// framing the map in its dead corners.
    fn place_edge_massif(&mut self, rng: &mut SimRng, ragged: &Noise) {
        let land = self.layout.land.clone();
        let edges: Vec<MapEdge> = MapEdge::ALL.into_iter().filter(|e| !land.is_ocean(*e)).collect();
        if edges.is_empty() {
            return;
        }
        let mut radius = rng.range(EDGE_MASSIF_RADIUS.0, EDGE_MASSIF_RADIUS.1) * self.mountain_scale().sqrt();
        for attempt in 0..TRIES {
            if attempt > 0 && attempt % 12 == 0 {
                radius = (radius * 0.85).max(MASSIF_RADIUS.0);
            }
            let edge = edges[rng.below(edges.len() as u32) as usize];
            // Along the edge (a corner when the neighbouring edge is land
            // too), centred a little off the map or just inside it.
            let t = if rng.chance(0.35) { if rng.chance(0.5) { 0.0 } else { 1.0 } } else { rng.range(0.15, 0.85) };
            let out = rng.range(-0.3, 0.2) * radius;
            let (lo, hi) = match edge {
                MapEdge::West | MapEdge::East => (land.min.y, land.max.y),
                MapEdge::South | MapEdge::North => (land.min.x, land.max.x),
            };
            let along = lo + (hi - lo) * t;
            let c = match edge {
                MapEdge::West => Vec2::new(out, along),
                MapEdge::East => Vec2::new(self.size - out, along),
                MapEdge::South => Vec2::new(along, out),
                MapEdge::North => Vec2::new(along, self.size - out),
            };
            // Rings of samples over the part on the map.
            let fits = (0..12).all(|k| {
                [0.0, 0.5, 0.95].iter().all(|f| {
                    let q = c + polar(k as f32 * TAU / 12.0) * radius * f;
                    let on_map = q.x >= 0.0 && q.y >= 0.0 && q.x < self.size && q.y < self.size;
                    !on_map || self.rock_fits(q, radius * (1.0 - f) * 0.3 + 1.0, true)
                })
            });
            if fits {
                self.paint_massif(c, radius, ragged);
                return;
            }
        }
    }

    /// Paint a round massif with a ragged outline and publish it.
    fn paint_massif(&mut self, c: Vec2, radius: f32, ragged: &Noise) {
        let (grass, marsh, mountain) = (self.ids.grass, self.ids.marsh, self.ids.mountain);
        self.paint_disc(c, radius * RAGGED.1 + 1.0, |cell| {
            let edge = radius * (RAGGED.0 + (RAGGED.1 - RAGGED.0) * ragged.at(cell.center));
            (!cell.reserved && (cell.terrain == grass || cell.terrain == marsh) && cell.center.distance(c) < edge).then_some(mountain)
        });
        self.mountains.push(MountainRange { spine: vec![c], half_widths: vec![radius] });
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
    }
}
