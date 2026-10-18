//! The map as boxes: a ground slab under the whole map, one box per
//! greedy-merged rectangle of equal terrain (top at [`terrain_top`],
//! bottom on the slab, so steps show their sides), and a frame around the
//! edge. Built once per match as soon as `Nav` and `MapInfo` exist.

use af_core::ext::render::terrain_ground;
use af_core::prelude::*;
use bevy::prelude::*;

use crate::assets::Painter;
use crate::look::{Look, hash32, scale, variation};
use crate::parts::Part;
use crate::scene::RenderScene;

/// Longest side of a merged terrain rectangle, in cells. Caps the size of
/// one flat colour so the per-rectangle variation reads as a patchwork.
pub const MAX_RECT_CELLS: u32 = 32;
/// Terrain at or below this height is flat ground: it receives shadows but
/// casts none.
const FLAT_HEIGHT: f32 = 0.5;
const SLAB_COLOR: Rgb = [0.24, 0.2, 0.16];
const FRAME_COLOR: Rgb = [0.2, 0.19, 0.18];
const FRAME_WIDTH: f32 = 2.0;
const FRAME_HEIGHT: f32 = 1.2;
/// Tall terrain that nothing stands on (thickets) varies its top per
/// rectangle within this fraction of its height.
const TALL_VARIATION: f32 = 0.08;

/// Top of a terrain type's boxes. Raised ground that units stand on
/// (climbable highlands) is drawn at exactly its `terrain_ground`, which
/// keeps it low (see `af_core::ext::render::MAX_GROUND_OFFSET`); anything
/// else at its `TerrainDef::height` (water below 0, thickets that units
/// wade through).
pub fn terrain_top(def: &TerrainDef) -> f32 {
    let ground = terrain_ground(def);
    if ground > 0.0 { ground } else { def.height }
}

/// The drawn top of flat terrain (at most [`FLAT_HEIGHT`]: sand stands a
/// few centimetres proud of the plane units walk on), or 0 for tall
/// terrain, whose top markers should not climb onto.
pub fn flat_top(def: &TerrainDef) -> f32 {
    let top = terrain_top(def);
    if top <= FLAT_HEIGHT { top } else { 0.0 }
}

/// Tall terrain that nothing stands on gets a ragged top; flat ground and
/// ground that units stand on stay level.
fn ragged(def: &TerrainDef) -> bool {
    let top = terrain_top(def);
    top > FLAT_HEIGHT && terrain_ground(def) != top
}

/// Top of one terrain rectangle (see [`ragged`]).
fn rect_top(def: &TerrainDef, seed: u32) -> f32 {
    let top = terrain_top(def);
    if ragged(def) { top * (1.0 + TALL_VARIATION * ((seed % 1001) as f32 / 500.0 - 1.0)) } else { top }
}

/// Highest point any terrain box can reach (the fog overlay floats above it).
pub fn max_terrain_top(data: &GameData) -> f32 {
    data.terrain.iter().map(|t| if ragged(t) { terrain_top(t) * (1.0 + TALL_VARIATION) } else { terrain_top(t) }).fold(0.0, f32::max)
}

/// A rectangle of cells `[x, x + w) x [y, y + h)` that all have terrain `id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub id: u8,
}

/// Greedy-merge a row-major grid of terrain ids into rectangles of one id,
/// at most `max` cells on a side. Every cell ends up in exactly one
/// rectangle. Scans rows; each unclaimed cell starts a rectangle, taking
/// the larger of "run right, then grow down" and "run down, then grow
/// right" (ragged edges merge much better with the choice).
pub fn greedy_rects(cells: &[u8], width: u32, height: u32, max: u32) -> Vec<CellRect> {
    let (w, h) = (width as usize, height as usize);
    let max = max.max(1) as usize;
    debug_assert_eq!(cells.len(), w * h);
    let mut done = vec![false; w * h];
    let mut out = Vec::new();
    for y in 0..h {
        let mut x = 0;
        while x < w {
            let i = y * w + x;
            if done[i] {
                x += 1;
                continue;
            }
            let id = cells[i];
            let free = |j: usize| !done[j] && cells[j] == id;
            // Run right, then grow down while whole rows are free.
            let mut aw = 1;
            while x + aw < w && aw < max && free(i + aw) {
                aw += 1;
            }
            let mut ah = 1;
            while y + ah < h && ah < max && (0..aw).all(|k| free(i + ah * w + k)) {
                ah += 1;
            }
            // Run down, then grow right while whole columns are free.
            let mut bh = 1;
            while y + bh < h && bh < max && free(i + bh * w) {
                bh += 1;
            }
            let mut bw = 1;
            while x + bw < w && bw < max && (0..bh).all(|k| free(i + k * w + bw)) {
                bw += 1;
            }
            let (rw, rh) = if bw * bh > aw * ah { (bw, bh) } else { (aw, ah) };
            for yy in y..y + rh {
                done[yy * w + x..yy * w + x + rw].fill(true);
            }
            out.push(CellRect { x: x as u32, y: y as u32, w: rw as u32, h: rh as u32, id });
            x += rw;
        }
    }
    out
}

/// Marker on terrain boxes.
#[derive(Component)]
pub struct TerrainBox;

/// Which map the current terrain boxes were built for.
#[derive(Resource, Default)]
pub struct TerrainState {
    built_for: Option<(u64, u32, u32, u32, u32)>,
}

impl TerrainState {
    pub fn reset(&mut self) {
        self.built_for = None;
    }
}

/// The most common terrain id in a grid (ties go to the lower id).
fn dominant_terrain(cells: &[u8]) -> u8 {
    let mut counts = [0u32; 256];
    for c in cells {
        counts[*c as usize] += 1;
    }
    (0..=255u8).rev().max_by_key(|i| counts[*i as usize]).unwrap_or(0)
}

/// Every box of the terrain, as parts in world space.
///
/// Two layers keep the count low. The most common terrain (the "base",
/// normally grass) is one merged layer under every cell that is at least as
/// high as it, so islands of higher terrain (dunes, thickets, highlands)
/// do not cut it up; those sit on top of it from the base height to their
/// own. Terrain lower than the base (water, marsh) fills its cells from the
/// slab up to its height.
pub fn terrain_parts(data: &GameData, nav: &NavGrid) -> Vec<Part> {
    let cell = nav.cell_size();
    let origin = nav.origin();
    let size = nav.world_size();
    let cells = nav.terrain_cells();
    let height_of = |id: u8| data.terrain.get(id as usize).map_or(0.0, terrain_top);
    let min_h = data.terrain.iter().map(terrain_top).fold(0.0, f32::min);
    let slab_top = min_h - 0.15;
    let slab_bottom = min_h - 1.0;
    let base = dominant_terrain(cells);
    let base_h = height_of(base);
    let raised = |id: u8| id != base && height_of(id) > base_h + 0.01;
    let mut parts = Vec::new();
    let world_box = |x0: f32, z0: f32, x1: f32, z1: f32, y0: f32, y1: f32| {
        (Vec3::new((x0 + x1) * 0.5, (y0 + y1) * 0.5, (z0 + z1) * 0.5), Vec3::new(x1 - x0, y1 - y0, z1 - z0))
    };
    let mut push_rect = |r: CellRect, id: u8, y0: f32| {
        let Some(def) = data.terrain.get(id as usize) else { return };
        let seed = hash32(r.x.wrapping_mul(73_856_093) ^ r.y.wrapping_mul(19_349_663) ^ (id as u32).wrapping_mul(83_492_791));
        let top = rect_top(def, seed);
        let tall = top > FLAT_HEIGHT;
        let x0 = origin.x + r.x as f32 * cell;
        let z0 = origin.y + r.y as f32 * cell;
        let (c, s) = world_box(x0, z0, x0 + r.w as f32 * cell, z0 + r.h as f32 * cell, y0, top);
        let part = Part::new(c, s, Look::lit(scale(def.color, variation(seed))));
        parts.push(if tall { part } else { part.no_shadow() });
    };
    let on_base: Vec<u8> = cells.iter().map(|c| u8::from(*c == base || raised(*c))).collect();
    for r in greedy_rects(&on_base, nav.width(), nav.height(), MAX_RECT_CELLS) {
        if r.id == 1 {
            push_rect(r, base, slab_top);
        }
    }
    for r in greedy_rects(cells, nav.width(), nav.height(), MAX_RECT_CELLS) {
        if r.id != base {
            push_rect(r, r.id, if raised(r.id) { base_h } else { slab_top });
        }
    }
    let (c, s) = world_box(origin.x, origin.y, origin.x + size.x, origin.y + size.y, slab_bottom, slab_top);
    parts.push(Part::new(c, s, Look::lit(SLAB_COLOR)).no_shadow());
    // Frame: four boxes just outside the map edge, meeting at the corners.
    let (x0, z0, x1, z1) = (origin.x, origin.y, origin.x + size.x, origin.y + size.y);
    let f = FRAME_WIDTH;
    for (ax, az, bx, bz) in [(x0 - f, z0 - f, x1 + f, z0), (x0 - f, z1, x1 + f, z1 + f), (x0 - f, z0, x0, z1), (x1, z0, x1 + f, z1)] {
        let (c, s) = world_box(ax, az, bx, bz, slab_bottom, FRAME_HEIGHT);
        parts.push(Part::new(c, s, Look::lit(FRAME_COLOR)));
    }
    parts
}

/// (Re)build the terrain boxes when a new map appears.
pub(crate) fn build_terrain(
    mut commands: Commands,
    mut painter: Painter,
    data: Res<Data>,
    nav: Res<Nav>,
    map: Res<MapInfo>,
    mut state: ResMut<TerrainState>,
    old: Query<Entity, With<TerrainBox>>,
) {
    let key = (map.seed, map.size.x.to_bits(), map.size.y.to_bits(), nav.width(), nav.height());
    if state.built_for == Some(key) {
        return;
    }
    for e in &old {
        commands.entity(e).try_despawn();
    }
    let parts = terrain_parts(&data, &nav);
    for part in &parts {
        painter.spawn_root(&mut commands, part, (TerrainBox, RenderScene));
    }
    debug!("af_render: built {} terrain boxes for a {}x{} map", parts.len(), nav.width(), nav.height());
    state.built_for = Some(key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look::hash32;

    fn check_cover(cells: &[u8], w: u32, h: u32, max: u32) -> Vec<CellRect> {
        let rects = greedy_rects(cells, w, h, max);
        let mut count = vec![0u8; cells.len()];
        for r in &rects {
            assert!(r.w >= 1 && r.h >= 1 && r.w <= max && r.h <= max, "{r:?}");
            assert!(r.x + r.w <= w && r.y + r.h <= h, "{r:?} inside the grid");
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    let i = (y * w + x) as usize;
                    assert_eq!(cells[i], r.id, "{r:?} is uniform");
                    count[i] += 1;
                }
            }
        }
        assert!(count.iter().all(|c| *c == 1), "every cell exactly once");
        rects
    }

    #[test]
    fn flat_markers_lie_on_top_of_flat_terrain_only() {
        let data = af_core::testing::test_data();
        let def = |id: &str| data.terrain_def(data.terrain_id(id).unwrap());
        let sand = def("sand");
        assert!(flat_top(sand) > af_core::ext::render::terrain_ground(sand), "sand is drawn proud of the ground units walk on");
        assert_eq!(flat_top(sand), terrain_top(sand));
        assert_eq!(flat_top(def("thicket")), 0.0, "markers never climb tall brambles");
        assert!(flat_top(def("deep_water")) < 0.0);
    }

    #[test]
    fn greedy_covers_every_cell_exactly_once() {
        // Random grids with few and many distinct ids.
        for (seed, ids) in [(1u32, 2u32), (2, 3), (3, 7), (4, 1)] {
            let (w, h) = (37, 23);
            let cells: Vec<u8> = (0..w * h).map(|i| (hash32(i ^ seed.wrapping_mul(0x9e37)) % ids) as u8).collect();
            check_cover(&cells, w, h, 8);
        }
    }

    #[test]
    fn greedy_merges_uniform_areas() {
        let rects = check_cover(&[5; 64 * 64], 64, 64, 32);
        assert_eq!(rects.len(), 4);
        let rects = check_cover(&[0; 10 * 3], 10, 3, 32);
        assert_eq!(rects, vec![CellRect { x: 0, y: 0, w: 10, h: 3, id: 0 }]);
        // Two halves.
        let cells: Vec<u8> = (0..20 * 10).map(|i| u8::from(i % 20 >= 10)).collect();
        assert_eq!(check_cover(&cells, 20, 10, 32).len(), 2);
        assert!(greedy_rects(&[], 0, 0, 8).is_empty());
    }

    /// A 384 m map shaped like the design doc's: a crater ringed by cliffs,
    /// lakes with shallows and marsh, dune patches, thickets. Blobby noise
    /// boundaries are the worst case for merging.
    fn synthetic_map(n: u32) -> Vec<u8> {
        let (grass, sand, shallow, deep, marsh, thicket, cliff) = (0u8, 1, 2, 3, 4, 5, 6);
        let c = n as f32 * 0.5;
        let mut cells = vec![grass; (n * n) as usize];
        let blobs: Vec<(f32, f32, f32, u8)> = (0..40u32)
            .map(|i| {
                let hx = hash32(i * 3 + 1) % n;
                let hy = hash32(i * 3 + 2) % n;
                let r = 6.0 + (hash32(i * 3 + 3) % 14) as f32;
                let kind = [sand, deep, marsh, thicket, sand, deep, thicket, cliff][(i % 8) as usize];
                (hx as f32, hy as f32, r, kind)
            })
            .collect();
        for y in 0..n {
            for x in 0..n {
                let (fx, fy) = (x as f32, y as f32);
                // Wobbly radius from a coarse hash: ragged edges.
                let wob = (hash32((x / 3) * 7919 + (y / 3) * 104_729) % 100) as f32 / 100.0 * 3.0;
                let mut t = grass;
                for (bx, by, r, kind) in &blobs {
                    let d = ((fx - bx).powi(2) + (fy - by).powi(2)).sqrt() + wob;
                    if d < *r {
                        t = if *kind == deep && d > r - 3.0 { shallow } else { *kind };
                    }
                }
                let dc = ((fx - c).powi(2) + (fy - c).powi(2)).sqrt() + wob;
                let angle = (fy - c).atan2(fx - c);
                let gap = (angle * 2.0).sin().abs() < 0.15;
                if (38.0..44.0).contains(&dc) && !gap {
                    t = cliff;
                }
                cells[(y * n + x) as usize] = t;
            }
        }
        cells
    }

    #[test]
    fn a_full_size_map_stays_under_the_box_budget() {
        let data = af_core::testing::test_data();
        let n = 384;
        let ids = ["grass", "sand", "shallow_water", "deep_water", "marsh", "thicket", "cliff"].map(|t| data.terrain_id(t).unwrap().0);
        let cells: Vec<u8> = synthetic_map(n).into_iter().map(|c| ids[c as usize]).collect();
        check_cover(&cells, n, n, MAX_RECT_CELLS);
        let mut nav = af_core::testing::new_nav(&data, n, n, data.terrain_id("grass").unwrap());
        for (i, c) in cells.iter().enumerate() {
            nav.set_terrain(IVec2::new(i as i32 % n as i32, i as i32 / n as i32), *c);
        }
        let boxes = terrain_parts(&data, &nav).len();
        assert!(boxes < 3000, "{boxes} terrain boxes");
    }

    #[test]
    fn terrain_parts_use_drawn_heights_and_cast_shadows_only_when_tall() {
        let data = af_core::testing::test_data();
        let grass = data.terrain_id("grass").unwrap();
        let cliff = data.terrain_id("cliff").unwrap();
        let water = data.terrain_id("deep_water").unwrap();
        let mut nav = af_core::testing::new_nav(&data, 40, 40, grass);
        for y in 0..40 {
            for x in 30..40 {
                nav.set_terrain(IVec2::new(x, y), cliff.0);
            }
            for x in 0..5 {
                nav.set_terrain(IVec2::new(x, y), water.0);
            }
        }
        let parts = terrain_parts(&data, &nav);
        // (grass, cliff, water rectangles) + slab + 4 frame boxes.
        assert!(parts.len() >= 3 + 1 + 4);
        let (terrain, rest) = parts.split_at(parts.len() - 5);
        let slab_top = rest[0].y_range().1;
        let water_def = data.terrain_def(water);
        assert!(slab_top < water_def.height, "slab is below the deepest water");
        let tops: Vec<(f32, bool)> = terrain.iter().map(|p| (p.y_range().1, p.shadow)).collect();
        assert!(tops.iter().any(|(t, s)| t.abs() < 1e-4 && !*s), "flat grass, no shadow");
        // Highlands are drawn as a low plateau units stand on, until the
        // rest of the game measures heights from ground_height.
        let highland = terrain_top(data.terrain_def(cliff));
        assert!(highland > FLAT_HEIGHT && highland <= af_core::ext::render::MAX_GROUND_OFFSET, "{highland}");
        assert!(tops.iter().any(|(t, s)| (*t - highland).abs() < 1e-4 && *s), "raised highlands cast shadows");
        assert!(tops.iter().any(|(t, _)| (*t - water_def.height).abs() < 1e-4), "water below ground");
        // Every cell is one contiguous column from the slab up to its terrain height.
        for y in 0..40 {
            for x in 0..40 {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let mut column: Vec<(f32, f32)> = terrain
                    .iter()
                    .filter(|b| (b.center.x - p.x).abs() < b.size.x * 0.5 && (b.center.z - p.y).abs() < b.size.z * 0.5)
                    .map(|b| b.y_range())
                    .collect();
                column.sort_by(|a, b| a.0.total_cmp(&b.0));
                assert!(!column.is_empty(), "cell {x},{y} covered");
                assert!((column[0].0 - slab_top).abs() < 1e-4, "cell {x},{y} starts on the slab");
                for pair in column.windows(2) {
                    assert!((pair[0].1 - pair[1].0).abs() < 1e-4, "cell {x},{y}: stacked without gaps or overlap");
                }
                let want = terrain_top(data.terrain_def(TerrainId(nav.terrain_at(IVec2::new(x, y)))));
                let top = column.last().unwrap().1;
                assert!((top - want).abs() <= want.abs() * TALL_VARIATION + 1e-4, "cell {x},{y}: top {top} vs {want}");
                let ground = af_core::ext::render::ground_height(&data, &nav, p);
                assert!(ground == 0.0 || (ground - top).abs() < 1e-4, "cell {x},{y}: units stand on the drawn top ({ground} vs {top})");
                assert!(ground.abs() <= af_core::ext::render::MAX_GROUND_OFFSET, "cell {x},{y}: ground {ground} near y = 0");
                assert!(top <= max_terrain_top(&data) + 1e-4);
            }
        }
    }
}
