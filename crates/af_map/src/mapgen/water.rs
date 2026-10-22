//! Standing water: ocean coasts beyond the land square, lakes and ponds,
//! and marsh along rivers and lakes (`docs/design/maps-and-terrain.md`
//! sections 2.2 and 2.3).

use std::collections::VecDeque;

use af_core::ext::map::{MapEdge, MapSize, WaterLevel};
use af_core::prelude::*;
use bevy::prelude::*;

use super::noise::Noise;
use super::{cell_center, Gen};

/// Marsh may appear up to this many cells from a river or lake.
const MARSH_REACH: u8 = 6;
/// Sand beach between the grass and the sea.
const BEACH: (f32, f32) = (2.0, 4.0);
/// Shallows between the beach and the open sea.
const SHALLOWS: (f32, f32) = (3.0, 8.0);
/// Where the coastline may lie, as fractions of the ocean band beyond the
/// land square: from a bay right at the square's edge out to a headland.
const COAST_REACH: (f32, f32) = (0.04, 0.55);
/// The sea keeps this far (beyond its beach) from reserved ground.
const COAST_CLEARANCE: f32 = 2.0;
/// Smallest lake radius worth painting.
const MIN_LAKE_RADIUS: f32 = 5.0;

impl Gen<'_> {
    /// Ocean beyond every ocean edge: a wobbling coastline (bays and
    /// headlands) that only ever lies outside the land square and never
    /// takes reserved ground, a sand beach on the land side, shallows, then
    /// open sea out to the map edge.
    pub(super) fn paint_coasts(&mut self, rng: &mut SimRng) {
        let land = self.layout.land.clone();
        if land.ocean.is_empty() {
            return;
        }
        let w = self.w as usize;
        let (grass, sand, shallow, ocean) = (self.ids.grass, self.ids.sand, self.ids.shallow, self.ids.ocean);
        struct Coast {
            edge: MapEdge,
            /// Coastline offset beyond the square, per metre along the edge.
            offset: Vec<f32>,
            beach: Noise,
            shallows: Noise,
        }
        let mut coasts = Vec::new();
        for edge in land.ocean.clone() {
            let shape = Noise::new(rng, self.size, (land.band * 1.6).max(24.0));
            let row = rng.range(0.0, self.size);
            let along = |t: f32| match edge {
                MapEdge::West | MapEdge::East => Vec2::new(row, t),
                MapEdge::South | MapEdge::North => Vec2::new(t, row),
            };
            let mut offset: Vec<f32> = (0..w).map(|t| land.band * (COAST_REACH.0 + (COAST_REACH.1 - COAST_REACH.0) * shape.at(along(t as f32 + 0.5)))).collect();
            // Reserved ground (start areas reaching past the square on small
            // maps) keeps its beach: push the coast out beyond it, smoothly.
            let mut need = vec![f32::NEG_INFINITY; w];
            for y in 0..w {
                for x in 0..w {
                    let i = y * w + x;
                    if self.reserved(i) {
                        let beyond = land.beyond_square(cell_center(x as i32, y as i32), edge);
                        if beyond > -BEACH.1 {
                            let t = along_edge(edge, x, y);
                            need[t] = need[t].max(beyond + BEACH.1 + COAST_CLEARANCE);
                        }
                    }
                }
            }
            let spread = 10usize;
            for t in 0..w {
                let lo = t.saturating_sub(spread);
                let hi = (t + spread).min(w - 1);
                let near = need[lo..=hi].iter().copied().fold(f32::NEG_INFINITY, f32::max);
                offset[t] = offset[t].max(near).min(land.band - SHALLOWS.0);
            }
            coasts.push(Coast { edge, offset, beach: Noise::new(rng, self.size, 12.0), shallows: Noise::new(rng, self.size, 20.0) });
        }
        for y in 0..w {
            for x in 0..w {
                let i = y * w + x;
                let p = cell_center(x as i32, y as i32);
                // How far past the coastline (positive: at sea), the least
                // of the edges' beaches, and the deepest verdict.
                let mut sea: Option<TerrainId> = None;
                let mut on_beach = false;
                for c in &coasts {
                    let past = land.beyond_square(p, c.edge) - c.offset[along_edge(c.edge, x, y)];
                    if past > 0.0 {
                        let shallows = SHALLOWS.0 + (SHALLOWS.1 - SHALLOWS.0) * c.shallows.at(p);
                        let t = if past > shallows { ocean } else { shallow };
                        sea = Some(if sea == Some(ocean) { ocean } else { t });
                    } else if past > -(BEACH.0 + (BEACH.1 - BEACH.0) * c.beach.at(p)) {
                        on_beach = true;
                    }
                }
                if let Some(t) = sea {
                    self.terrain[i] = t;
                    self.coast[i] = true;
                } else if on_beach && !self.reserved(i) && self.terrain[i] == grass {
                    self.terrain[i] = sand;
                    self.coast[i] = true;
                }
            }
        }
        self.touch();
    }

    /// Lakes and ponds for the water level (section 2.2): noisy blobs, deep
    /// in the middle with a shallow shore, kept clear of everything reserved
    /// and of the rivers. Lands bigger than Medium get more of them and
    /// bigger ones.
    pub(super) fn paint_lakes(&mut self, rng: &mut SimRng) {
        let (count, radius) = match self.layout.options.water {
            WaterLevel::Dry => (0, (0.0, 0.0)),
            WaterLevel::Ponds => (1 + rng.below(2), (6.0, 9.0)),
            WaterLevel::Lakes => (2 + rng.below(3), (8.0, 15.0)),
            WaterLevel::Coast => (1 + rng.below(2), (8.0, 13.0)),
            WaterLevel::Seas => (1 + rng.below(3), (8.0, 13.0)),
        };
        let medium = MapSize::Medium.land_side(self.layout.starts.len());
        // Land area relative to Medium: shared between more lakes (its
        // square root) and bigger ones (its fourth root).
        let area = (self.layout.land.side / medium).powi(2).max(1.0);
        let count = (count as f32 * area.sqrt()).round() as u32;
        let (r_min, r_max) = (self.layout.main().outer() + 12.0, self.wild_reach());
        for _ in 0..count {
            let wanted = rng.range(radius.0, radius.1) * area.sqrt().sqrt();
            let Some((c, outer)) = self.feature_spot(rng, wanted * 1.25, MIN_LAKE_RADIUS * 1.25, r_min, r_max) else { continue };
            let radius = outer / 1.25;
            let bank = rng.range(2.0, 3.0).min(radius * 0.4);
            let shape = Noise::new(rng, self.size, 9.0);
            let (grass, deep, shallow) = (self.ids.grass, self.ids.deep, self.ids.shallow);
            // Lakes only flood grass, never sand, water or rock.
            self.paint_disc(c, outer, |cell| {
                if cell.reserved || cell.terrain != grass {
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
        }
    }

    /// Marsh patches on grass within [`MARSH_REACH`] cells of a river or a
    /// lake (never of the sea); fewer the drier the map.
    pub(super) fn paint_marsh_near_water(&mut self, rng: &mut SimRng) {
        let noise = Noise::new(rng, self.size, 8.0);
        let threshold = match self.layout.options.water {
            WaterLevel::Dry => 0.63,
            WaterLevel::Ponds => 0.59,
            _ => 0.55,
        };
        let cells = self.terrain.len();
        let mut dist = vec![u8::MAX; cells];
        let mut queue = VecDeque::new();
        for (i, t) in self.terrain.iter().enumerate() {
            if !self.coast[i] && (*t == self.ids.deep || *t == self.ids.shallow || *t == self.ids.ford) {
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
            if (1..=MARSH_REACH).contains(&d) && !self.reserved(i) && !self.coast[i] && self.terrain[i] == self.ids.grass && noise.at(center) > threshold {
                self.terrain[i] = self.ids.marsh;
            }
        }
        self.touch();
    }
}

/// Position of cell (`x`, `y`) along `edge`, in whole metres.
#[inline]
fn along_edge(edge: MapEdge, x: usize, y: usize) -> usize {
    match edge {
        MapEdge::West | MapEdge::East => y,
        MapEdge::South | MapEdge::North => x,
    }
}
