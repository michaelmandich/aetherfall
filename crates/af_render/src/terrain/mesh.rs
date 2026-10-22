//! Chunk meshes of the [`Field`]: the ground (a vertex at every corner,
//! normals from the whole field so chunks meet without seams), the water
//! surface over water cells, and the class images the terrain shader blends.

use af_core::ext::render::WATER_LEVEL;
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::field::{Cover, Field};
use super::noise::smoothstep;

/// Cells per chunk side.
pub const CHUNK: i32 = 64;
/// Ground chunks rising above this (mountains, highlands) cast shadows;
/// flat ones only receive.
pub const SHADOW_HEIGHT: f32 = 0.3;

/// One chunk: field cells `[x0, x1) x [z0, z1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkRect {
    pub x0: i32,
    pub z0: i32,
    pub x1: i32,
    pub z1: i32,
}

impl ChunkRect {
    pub fn cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (self.z0..self.z1).flat_map(move |z| (self.x0..self.x1).map(move |x| (x, z)))
    }

    pub fn contains(&self, x: i32, z: i32) -> bool {
        x >= self.x0 && x < self.x1 && z >= self.z0 && z < self.z1
    }
}

/// The chunks covering the whole field: the map in [`CHUNK`]-cell tiles
/// from its min corner, ringed by skirt tiles. Each comes with whether it
/// lies on the map.
pub fn chunks(field: &Field) -> Vec<(ChunkRect, bool)> {
    let breaks = |cells: i32| {
        let mut v = vec![0, field.skirt];
        let mut at = field.skirt;
        while at < field.skirt + cells {
            at = (at + CHUNK).min(field.skirt + cells);
            v.push(at);
        }
        v.push(cells + 2 * field.skirt);
        v
    };
    let (xs, zs) = (breaks(field.map_cols), breaks(field.map_rows));
    let mut out = Vec::new();
    for zw in zs.windows(2) {
        for xw in xs.windows(2) {
            let r = ChunkRect { x0: xw[0], z0: zw[0], x1: xw[1], z1: zw[1] };
            if r.x1 > r.x0 && r.z1 > r.z0 {
                let on_map = r.x0 >= field.skirt && r.z0 >= field.skirt && r.x1 <= field.skirt + field.map_cols && r.z1 <= field.skirt + field.map_rows;
                out.push((r, on_map));
            }
        }
    }
    out
}

/// A built chunk mesh with its bounds.
pub struct ChunkMesh {
    pub mesh: Mesh,
    pub aabb: Aabb,
    pub max_height: f32,
}

fn indices(n_vertices: usize, list: Vec<u32>) -> Indices {
    if n_vertices <= u16::MAX as usize { Indices::U16(list.into_iter().map(|i| i as u16).collect()) } else { Indices::U32(list) }
}

fn bounds(positions: &[[f32; 3]]) -> Aabb {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in positions {
        lo = lo.min(Vec3::from(*p));
        hi = hi.max(Vec3::from(*p));
    }
    Aabb::from_min_max(lo, hi)
}

/// Push a quad's two triangles (corners 00, 10, 01, 11 counter-clockwise
/// seen from above), split along 00-11 or 10-01.
fn quad(out: &mut Vec<u32>, [v00, v10, v01, v11]: [u32; 4], main_diagonal: bool) {
    if main_diagonal {
        out.extend([v00, v01, v11, v00, v11, v10]);
    } else {
        out.extend([v00, v01, v10, v10, v01, v11]);
    }
}

/// Which diagonal a cell is split along: land always along (min, min) to
/// (max, max) like `ground_height`; elsewhere the one with the smaller
/// height difference, so ridges and channels follow the terrain.
pub fn main_diagonal(field: &Field, x: i32, z: i32) -> bool {
    field.cell_is_land(x, z) || (field.height(x, z) - field.height(x + 1, z + 1)).abs() <= (field.height(x + 1, z) - field.height(x, z + 1)).abs()
}

/// The ground's vertex colour, which the terrain shader reads as data: how
/// close a corner is to water (r: 1 on the shore, 0 from 6 m away), to a
/// mountain's foot (g: 1 at the rock, 0 from 4 m away), and how much it
/// stands out of the ground around it (b: 0.5 flat, toward 1 on ridges and
/// peaks, toward 0 in gullies).
pub fn corner_data(field: &Field, x: i32, z: i32) -> [f32; 4] {
    let cells = field.corner_cells(x, z);
    let (mut shore, mut rock) = (f32::MAX, f32::MAX);
    for i in cells {
        let c = field.cover[i];
        shore = shore.min(if c.is_water() { 0.0 } else { field.shore[i] });
        rock = rock.min(if c == Cover::Mountain { 0.0 } else { field.rock[i] });
    }
    let wet = 1.0 - smoothstep(0.5, 6.5, shore * field.cell);
    let foot = 1.0 - smoothstep(0.5, 4.5, rock * field.cell);
    let around = (field.height(x - 1, z) + field.height(x + 1, z) + field.height(x, z - 1) + field.height(x, z + 1)) * 0.25;
    let bulge = ((field.height(x, z) - around) * 0.8 + 0.5).clamp(0.0, 1.0);
    [wet, foot, bulge, 1.0]
}

/// The ground mesh of one chunk.
pub fn ground_mesh(field: &Field, r: ChunkRect) -> ChunkMesh {
    let (w, h) = (r.x1 - r.x0 + 1, r.z1 - r.z0 + 1);
    let n = (w * h) as usize;
    let (mut positions, mut normals, mut colors) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    let mut max_height = f32::MIN;
    for z in r.z0..=r.z1 {
        for x in r.x0..=r.x1 {
            let p = field.world_corner(x, z);
            let y = field.height(x, z);
            max_height = max_height.max(y);
            positions.push([p.x, y, p.y]);
            normals.push(field.normal(x, z).to_array());
            colors.push(corner_data(field, x, z));
        }
    }
    let at = |x: i32, z: i32| ((z - r.z0) * w + (x - r.x0)) as u32;
    let mut list = Vec::with_capacity(((w - 1) * (h - 1) * 6) as usize);
    for (x, z) in r.cells() {
        quad(&mut list, [at(x, z), at(x + 1, z), at(x, z + 1), at(x + 1, z + 1)], main_diagonal(field, x, z));
    }
    let aabb = bounds(&positions);
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(indices(n, list));
    ChunkMesh { mesh, aabb, max_height }
}

/// Is a cell under the water surface: a water cell, or a cell next to one
/// (the surface reaches a cell under the banks, so the rising bank cuts the
/// shoreline)?
pub fn under_water(field: &Field, x: i32, z: i32) -> bool {
    (-1..=1).any(|dz| (-1..=1).any(|dx| field.cover_at(x + dx, z + dz).is_water()))
}

/// What the water shader knows of a corner, as vertex data.
pub struct WaterVertex {
    /// (depth / 4 m, river, ford, sea).
    pub color: [f32; 4],
    /// The river course: metres downstream, metres across (`Field::course`).
    pub course: [f32; 2],
    /// (metres from the shore, 0).
    pub shore: [f32; 2],
    /// The flow as a tangent: its direction (x, 0, z) and speed (w, m/s);
    /// all zero on still water.
    pub flow: [f32; 4],
}

pub fn water_vertex(field: &Field, x: i32, z: i32) -> WaterVertex {
    let depth = WATER_LEVEL - field.height(x, z);
    let (mut river, mut ford, mut sea, mut shore) = (0.0f32, 0.0f32, 0.0f32, f32::MAX);
    let (mut course, mut flow, mut flowing, mut wet) = (Vec2::ZERO, Vec2::ZERO, 0.0f32, 0.0f32);
    for i in field.corner_cells(x, z) {
        let c = field.cover[i];
        if !c.is_water() {
            shore = 0.0;
            continue;
        }
        wet += 1.0;
        river = river.max(field.river[i]);
        ford += f32::from(c == Cover::Ford);
        sea += f32::from(c == Cover::Ocean);
        shore = shore.min(field.shore[i] * field.cell);
        flow += field.flow[i];
        if field.flow[i] != Vec2::ZERO {
            course += field.course[i];
            flowing += 1.0;
        }
    }
    let course = course / flowing.max(1.0);
    let flow = flow / wet.max(1.0);
    let speed = flow.length();
    WaterVertex {
        color: [(depth / 4.0).clamp(-1.0, 1.0), river, ford * 0.25, sea * 0.25],
        course: course.to_array(),
        shore: [shore.min(64.0), 0.0],
        flow: if speed > 1e-4 { [flow.x / speed, 0.0, flow.y / speed, speed] } else { [0.0; 4] },
    }
}

/// The water surface of one chunk, if any of it is under water.
pub fn water_mesh(field: &Field, r: ChunkRect) -> Option<ChunkMesh> {
    let w = r.x1 - r.x0 + 1;
    let mut slot = vec![u32::MAX; (w * (r.z1 - r.z0 + 1)) as usize];
    let (mut positions, mut colors, mut courses, mut shores, mut flows) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut list = Vec::new();
    for (x, z) in r.cells() {
        if !under_water(field, x, z) {
            continue;
        }
        let mut corner = |cx: i32, cz: i32| {
            let s = ((cz - r.z0) * w + (cx - r.x0)) as usize;
            if slot[s] == u32::MAX {
                slot[s] = positions.len() as u32;
                let p = field.world_corner(cx, cz);
                positions.push([p.x, WATER_LEVEL, p.y]);
                let v = water_vertex(field, cx, cz);
                colors.push(v.color);
                courses.push(v.course);
                shores.push(v.shore);
                flows.push(v.flow);
            }
            slot[s]
        };
        let v = [corner(x, z), corner(x + 1, z), corner(x, z + 1), corner(x + 1, z + 1)];
        quad(&mut list, v, true);
    }
    if list.is_empty() {
        return None;
    }
    let n = positions.len();
    let aabb = bounds(&positions);
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, courses)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, shores)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, flows)
        .with_inserted_indices(indices(n, list));
    Some(ChunkMesh { mesh, aabb, max_height: WATER_LEVEL })
}

/// The terrain shader's two class images (see [`Field::class_texels`]),
/// linearly filtered.
pub fn class_images(field: &Field) -> [Image; 2] {
    field.class_texels().map(|texels| {
        let mut image = Image::new(
            Extent3d { width: field.cols as u32, height: field.rows as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            texels,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::linear();
        image
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::field::FieldSource;
    use af_core::ext::render::ground_height;
    use af_core::testing::test_data;

    #[test]
    fn chunks_cover_the_field_once() {
        let data = test_data();
        for n in [64u32, 100, 224] {
            let nav = crate::terrain::field::tests::painted(&data, n);
            let f = Field::build(&FieldSource { data: &data, nav: &nav, seed: 1, features: None, crater: None });
            let mut count = vec![0u8; (f.cols * f.rows) as usize];
            for (r, on_map) in chunks(&f) {
                assert!(r.x1 - r.x0 <= CHUNK && r.z1 - r.z0 <= CHUNK, "{r:?}");
                for (x, z) in r.cells() {
                    count[f.index(x, z)] += 1;
                    assert_eq!(f.on_map(x, z), on_map, "{r:?} is all map or all skirt");
                }
            }
            assert!(count.iter().all(|c| *c == 1), "every cell of the map and skirt in exactly one chunk ({n} m)");
        }
    }

    #[test]
    fn walkable_cells_are_drawn_where_units_stand() {
        let data = test_data();
        let nav = crate::terrain::field::tests::painted(&data, 128);
        let f = Field::build(&FieldSource { data: &data, nav: &nav, seed: 3, features: None, crater: None });
        // Sample inside every land cell of the map, on both triangles, and
        // compare the drawn mesh with ground_height.
        for (r, on_map) in chunks(&f) {
            if !on_map {
                continue;
            }
            let built = ground_mesh(&f, r);
            assert!(built.max_height <= af_core::ext::render::MAX_TERRAIN_HEIGHT);
            for (x, z) in r.cells() {
                if !f.cell_is_land(x, z) {
                    continue;
                }
                assert!(main_diagonal(&f, x, z));
                for (fx, fz) in [(0.7, 0.2), (0.2, 0.7), (0.5, 0.5)] {
                    let p = f.world_corner(x, z) + Vec2::new(fx, fz);
                    let (h00, h10, h01, h11) = (f.height(x, z), f.height(x + 1, z), f.height(x, z + 1), f.height(x + 1, z + 1));
                    let drawn = if fx >= fz { h00 + (h10 - h00) * fx + (h11 - h10) * fz } else { h00 + (h11 - h01) * fx + (h01 - h00) * fz };
                    assert!((drawn - ground_height(&data, &nav, p)).abs() < 1e-5, "cell {x},{z} at {p}");
                }
            }
        }
    }

    #[test]
    fn water_covers_water_cells_and_their_banks() {
        let data = test_data();
        let nav = crate::terrain::field::tests::painted(&data, 128);
        let f = Field::build(&FieldSource { data: &data, nav: &nav, seed: 3, features: None, crater: None });
        let mut quads = 0;
        for (r, _) in chunks(&f) {
            let wet = r.cells().filter(|(x, z)| under_water(&f, *x, *z)).count();
            match water_mesh(&f, r) {
                Some(m) => {
                    assert!(wet > 0);
                    quads += m.mesh.indices().unwrap().len() / 6;
                    assert_eq!(m.mesh.indices().unwrap().len() / 6, wet);
                }
                None => assert_eq!(wet, 0),
            }
        }
        let water_cells = (0..f.rows).flat_map(|z| (0..f.cols).map(move |x| (x, z))).filter(|(x, z)| f.cover_at(*x, *z).is_water()).count();
        assert!(quads > water_cells, "the surface reaches under the banks");
        let v = water_vertex(&f, f.skirt + 90, f.skirt + 60);
        assert!(v.color[0] > 0.3, "deep in the river: {:?}", v.color);
        assert_eq!(v.flow, [0.0; 4], "no river lines: still water");
    }
}
