//! The land: ground, water and props, built once per match as soon as
//! `Nav` and `MapInfo` exist (`docs/design/maps-and-terrain.md`, section 6).
//!
//! * [`field`]: the shape: what every cell looks like and the height of
//!   every corner, over the map and an off-map skirt. Every cell units walk
//!   on is drawn exactly at `ground_height`; relief only rises in mountains
//!   and sinks in water beds.
//! * [`mesh`]: the field cut into chunks of 64 x 64 cells, one ground mesh
//!   and one water surface each (normals from the whole field, so chunks
//!   meet without seams); flat chunks cast no shadows.
//! * [`props`]: brambles, reeds, scree and meteor rock, merged into one
//!   mesh per chunk, casting no shadows; the Starspire of a `Spire` site
//!   is its own mesh and casts its tall shadow.
//! * [`material`]: the ground, water and prop materials, `StandardMaterial`
//!   extended with the embedded shaders `terrain.wgsl`, `water.wgsl` and
//!   `props.wgsl` (which import `common.wgsl`), created once per match.
//!   They draw the fog of war from [`FogTexture`] ([`bind_fog`]).
//!
//! The drawn surface is published as `af_core::ext::render::TerrainSurface`.
//! Apps without the render stack (headless tests) get the same chunk
//! entities with plain `StandardMaterial`s and no shaders.
//!
//! The meshes are built on the compute task pool: a 384 m map takes about
//! 60 ms, a 1024 m one about 300 ms (dev build, 24 threads).

pub mod field;
pub mod material;
pub mod mesh;
pub mod noise;
pub mod props;

use std::time::Instant;

use af_core::ext::map::MapFeatures;
use af_core::ext::render::{FogTexture, TerrainSurface, WATER_LEVEL};
use af_core::prelude::*;
use bevy::camera::primitives::Aabb;
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::shader::Shader;
use bevy::tasks::ComputeTaskPool;

use self::field::{Field, FieldSource};
use self::material::{GroundParams, PropExt, PropMaterial, TerrainExt, TerrainMaterial, WaterExt, WaterMaterial};
use self::mesh::{ChunkMesh, ChunkRect, SHADOW_HEIGHT, chunks, class_images, ground_mesh, water_mesh};
use crate::scene::{RenderScene, SKY};

/// The world beyond the skirt while the fog of war is drawn: the dark of
/// unexplored ground (`UNEXPLORED` in `common.wgsl`, in sRGB).
pub const BEYOND_FOG: Color = Color::srgb(0.05, 0.055, 0.065);

/// Marker on every terrain chunk entity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainChunk {
    /// A ground mesh, on the map or in the skirt around it.
    Ground { on_map: bool },
    Water,
    Props,
}

/// Keeps the shared WGSL module loaded so the shaders can import it.
#[derive(Resource)]
struct TerrainShaders(#[allow(dead_code)] Handle<Shader>);

/// Register the terrain materials and embed their shaders, when the app
/// renders (has `PbrPlugin`); headless apps fall back to plain materials.
pub(crate) fn add_terrain_materials(app: &mut App) {
    if !app.is_plugin_added::<bevy::pbr::PbrPlugin>() {
        return;
    }
    bevy::asset::embedded_asset!(app, "common.wgsl");
    bevy::asset::embedded_asset!(app, "terrain.wgsl");
    bevy::asset::embedded_asset!(app, "water.wgsl");
    bevy::asset::embedded_asset!(app, "props.wgsl");
    app.add_plugins((MaterialPlugin::<TerrainMaterial>::default(), MaterialPlugin::<WaterMaterial>::default(), MaterialPlugin::<PropMaterial>::default()));
    let common = app.world().resource::<AssetServer>().load::<Shader>(material::COMMON_SHADER);
    app.insert_resource(TerrainShaders(common));
}

/// A material handle: the terrain shader's, or a plain stand-in.
#[derive(Clone, Debug)]
pub enum Mat<M: Material> {
    Shaded(Handle<M>),
    Plain(Handle<StandardMaterial>),
}

impl<M: Material> Mat<M> {
    fn insert(&self, e: &mut EntityCommands) {
        match self {
            Mat::Shaded(h) => e.insert(MeshMaterial3d(h.clone())),
            Mat::Plain(h) => e.insert(MeshMaterial3d(h.clone())),
        };
    }
}

/// The match's three terrain materials.
#[derive(Clone, Debug)]
pub struct TerrainMaterials {
    pub ground: Mat<TerrainMaterial>,
    pub water: Mat<WaterMaterial>,
    pub props: Mat<PropMaterial>,
}

/// Which map the terrain was built for, and its materials (made once per
/// match).
#[derive(Resource, Default)]
pub struct TerrainState {
    built_for: Option<(u64, u32, u32, u32, u32, bool)>,
    materials: Option<TerrainMaterials>,
    /// The fog texture the materials were last given (`Some(None)`: none,
    /// everything shown); `None` until they get one.
    fog_bound: Option<Option<AssetId<Image>>>,
}

impl TerrainState {
    pub fn reset(&mut self) {
        *self = TerrainState::default();
    }

    pub fn materials(&self) -> Option<&TerrainMaterials> {
        self.materials.as_ref()
    }
}

/// The assets terrain building touches.
#[derive(SystemParam)]
pub(crate) struct TerrainAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    images: ResMut<'w, Assets<Image>>,
    plain: ResMut<'w, Assets<StandardMaterial>>,
    ground: Option<ResMut<'w, Assets<TerrainMaterial>>>,
    water: Option<ResMut<'w, Assets<WaterMaterial>>>,
    props: Option<ResMut<'w, Assets<PropMaterial>>>,
}

impl TerrainAssets<'_> {
    /// The terrain materials for a new match.
    fn materials(&mut self, params: GroundParams, field: &Field) -> TerrainMaterials {
        match (self.ground.as_mut(), self.water.as_mut(), self.props.as_mut()) {
            (Some(ground), Some(water), Some(props)) => {
                let [a, b] = class_images(field).map(|i| self.images.add(i));
                TerrainMaterials {
                    ground: Mat::Shaded(ground.add(TerrainMaterial {
                        base: material::ground_base(),
                        extension: TerrainExt { params, fog: None, classes_a: Some(a), classes_b: Some(b) },
                    })),
                    water: Mat::Shaded(water.add(WaterMaterial { base: material::water_base(), extension: WaterExt { params, fog: None } })),
                    props: Mat::Shaded(props.add(PropMaterial { base: material::prop_base(), extension: PropExt { params, fog: None } })),
                }
            }
            _ => TerrainMaterials {
                ground: Mat::Plain(self.plain.add(material::ground_base())),
                water: Mat::Plain(self.plain.add(StandardMaterial { base_color: Color::srgb(0.24, 0.6, 0.88), ..material::water_base() })),
                props: Mat::Plain(self.plain.add(material::prop_base())),
            },
        }
    }

    /// Point the shaded materials at a new map (a rebuild inside a match).
    fn rebind_map(&mut self, mats: &TerrainMaterials, params: GroundParams, field: &Field) {
        if let (Mat::Shaded(h), Some(assets)) = (&mats.ground, self.ground.as_mut()) {
            let [a, b] = class_images(field).map(|i| self.images.add(i));
            if let Some(mut m) = assets.get_mut(h) {
                m.extension.params = params;
                m.extension.classes_a = Some(a);
                m.extension.classes_b = Some(b);
            }
        }
        self.each_params(mats, |p| *p = params);
    }

    /// Change the uniforms of every shaded terrain material.
    fn each_params(&mut self, mats: &TerrainMaterials, mut edit: impl FnMut(&mut GroundParams)) {
        if let (Mat::Shaded(h), Some(a)) = (&mats.ground, self.ground.as_mut())
            && let Some(mut m) = a.get_mut(h)
        {
            edit(&mut m.extension.params);
        }
        if let (Mat::Shaded(h), Some(a)) = (&mats.water, self.water.as_mut())
            && let Some(mut m) = a.get_mut(h)
        {
            edit(&mut m.extension.params);
        }
        if let (Mat::Shaded(h), Some(a)) = (&mats.props, self.props.as_mut())
            && let Some(mut m) = a.get_mut(h)
        {
            edit(&mut m.extension.params);
        }
    }
}

/// The uniforms of a map, fog of war off.
fn ground_params(field: &Field) -> GroundParams {
    let map_min = field.origin + Vec2::splat(field.skirt as f32 * field.cell);
    let map_max = map_min + Vec2::new(field.map_cols as f32, field.map_rows as f32) * field.cell;
    let size = Vec2::new(field.cols as f32, field.rows as f32) * field.cell;
    let sky = SKY.to_linear();
    let sky = Vec4::new(sky.red, sky.green, sky.blue, 1.0);
    GroundParams {
        map: Vec4::new(map_min.x, map_min.y, map_max.x, map_max.y),
        field: Vec4::new(field.origin.x, field.origin.y, 1.0 / size.x, 1.0 / size.y),
        fog: Vec4::new(0.0, 0.0, 0.0, field.skirt as f32 * field.cell),
        sky,
        reflection: sky,
    }
}

/// Point the uniforms at the fog of war (`fog_extent`: the fog grid's size
/// in metres) or away from it: while it is drawn the skirt fades into the
/// dark of unexplored ground, else into the sky. The water's reflection
/// stays the sky either way.
fn set_fog(p: &mut GroundParams, fog_extent: Vec2, on: bool) {
    let beyond = if on { BEYOND_FOG } else { SKY }.to_linear();
    p.fog.x = 1.0 / fog_extent.x;
    p.fog.y = 1.0 / fog_extent.y;
    p.fog.z = if on { 1.0 } else { 0.0 };
    p.sky = Vec4::new(beyond.red, beyond.green, beyond.blue, 1.0);
}

/// The drawn top at every map corner, and how every map cell is split into
/// its triangles (for `TerrainSurface`).
pub(crate) fn surface_of(field: &Field) -> TerrainSurface {
    let (w, h) = (field.map_cols + 1, field.map_rows + 1);
    let mut top = Vec::with_capacity((w * h) as usize);
    for z in 0..h {
        for x in 0..w {
            let (fx, fz) = (x + field.skirt, z + field.skirt);
            let y = field.height(fx, fz);
            top.push(if field.corner_wet(fx, fz) { y.max(WATER_LEVEL) } else { y });
        }
    }
    let split = (0..field.map_rows).flat_map(|z| (0..field.map_cols).map(move |x| !mesh::main_diagonal(field, x + field.skirt, z + field.skirt))).collect();
    TerrainSurface { origin: field.world_corner(field.skirt, field.skirt), cell: field.cell, width: w as u32, height: h as u32, top, split }
}

/// What one chunk of the field turns into.
struct BuiltChunk {
    on_map: bool,
    ground: ChunkMesh,
    water: Option<ChunkMesh>,
    props: Option<(Mesh, Aabb)>,
}

/// Build every chunk's meshes, spread over the compute task pool (the
/// results keep the chunks' order, so the scene is the same every time).
fn build_chunks(field: &Field) -> Vec<BuiltChunk> {
    let build = |(rect, on_map): (ChunkRect, bool)| BuiltChunk {
        on_map,
        ground: ground_mesh(field, rect),
        water: water_mesh(field, rect),
        props: if on_map { props::prop_mesh(props::chunk_props(field, rect)) } else { None },
    };
    let rects = chunks(field);
    match ComputeTaskPool::try_get() {
        Some(pool) if pool.thread_num() > 1 => pool.scope(|s| {
            for r in rects {
                s.spawn(async move { build(r) });
            }
        }),
        _ => rects.into_iter().map(build).collect(),
    }
}

/// (Re)build the terrain when a new map appears.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_terrain(
    mut commands: Commands,
    mut assets: TerrainAssets,
    data: Res<Data>,
    nav: Res<Nav>,
    map: Res<MapInfo>,
    features: Option<Res<MapFeatures>>,
    mut state: ResMut<TerrainState>,
    old: Query<Entity, With<TerrainChunk>>,
) {
    let key = (map.seed, map.size.x.to_bits(), map.size.y.to_bits(), nav.width(), nav.height(), features.is_some());
    if state.built_for == Some(key) {
        return;
    }
    let started = Instant::now();
    for e in &old {
        commands.entity(e).try_despawn();
    }
    let crater = (map.crater_radius > 0.0).then_some((map.crater_center, map.crater_radius));
    let field = Field::build(&FieldSource { data: &data, nav: &nav, seed: map.seed, features: features.as_deref(), crater });
    let field_ms = started.elapsed().as_secs_f32() * 1000.0;
    let params = ground_params(&field);
    let mats = match state.materials.clone() {
        Some(mats) => {
            assets.rebind_map(&mats, params, &field);
            mats
        }
        None => assets.materials(params, &field),
    };
    let (mut count, mut prop_vertices) = (0, 0);
    for BuiltChunk { on_map, ground, water, props } in build_chunks(&field) {
        let mut e = commands.spawn((
            Name::new("Terrain chunk"),
            Mesh3d(assets.meshes.add(ground.mesh)),
            ground.aabb,
            Transform::IDENTITY,
            Visibility::Inherited,
            TerrainChunk::Ground { on_map },
            RenderScene,
        ));
        mats.ground.insert(&mut e);
        if ground.max_height < SHADOW_HEIGHT {
            e.insert(NotShadowCaster);
        }
        count += 1;
        if let Some(water) = water {
            let mut e = commands.spawn((
                Name::new("Water chunk"),
                Mesh3d(assets.meshes.add(water.mesh)),
                water.aabb,
                Transform::IDENTITY,
                Visibility::Inherited,
                TerrainChunk::Water,
                RenderScene,
                NotShadowCaster,
            ));
            mats.water.insert(&mut e);
            count += 1;
        }
        if let Some((mesh, aabb)) = props {
            prop_vertices += mesh.count_vertices();
            let mut e = commands.spawn((Name::new("Prop chunk"), Mesh3d(assets.meshes.add(mesh)), aabb, Transform::IDENTITY, Visibility::Inherited, TerrainChunk::Props, RenderScene, NotShadowCaster));
            mats.props.insert(&mut e);
            count += 1;
        }
    }
    if let Some((mesh, aabb)) = props::prop_mesh(props::spire_props(&field)) {
        let mut e = commands.spawn((Name::new("Starspire"), Mesh3d(assets.meshes.add(mesh)), aabb, Transform::IDENTITY, Visibility::Inherited, TerrainChunk::Props, RenderScene));
        mats.props.insert(&mut e);
        count += 1;
    }
    commands.insert_resource(surface_of(&field));
    info!("af_render: built the terrain of a {}x{} map in {:.0} ms (field {field_ms:.0} ms; {count} chunks, {prop_vertices} prop vertices)", nav.width(), nav.height(), started.elapsed().as_secs_f32() * 1000.0);
    state.built_for = Some(key);
    state.materials = Some(mats);
    state.fog_bound = None;
}

/// Give the terrain materials the fog-of-war texture while one is shown
/// (a player's view with fog on), and take it away for spectators and
/// fog-off matches. Changes the materials only when that changes. While
/// the fog is drawn, the world beyond the skirt is dark rather than sky,
/// as unexplored ground is.
pub(crate) fn bind_fog(
    texture: Option<Res<FogTexture>>,
    fog: Option<Res<FogOfWar>>,
    clear: Option<ResMut<ClearColor>>,
    mut state: ResMut<TerrainState>,
    mut assets: TerrainAssets,
) {
    let image = texture.map(|t| t.0.clone());
    let want = image.as_ref().map(|h| h.id());
    if state.fog_bound == Some(want) {
        return;
    }
    let Some(mats) = state.materials.clone() else { return };
    let extent = fog.as_ref().map_or(Vec2::ONE, |f| Vec2::new(f.width as f32, f.height as f32) * f.cell).max(Vec2::splat(1.0));
    let on = image.is_some();
    if let Some(mut clear) = clear {
        clear.0 = if on { BEYOND_FOG } else { SKY };
    }
    assets.each_params(&mats, |p| set_fog(p, extent, on));
    if let (Mat::Shaded(h), Some(a)) = (&mats.ground, assets.ground.as_mut())
        && let Some(mut m) = a.get_mut(h)
    {
        m.extension.fog = image.clone();
    }
    if let (Mat::Shaded(h), Some(a)) = (&mats.water, assets.water.as_mut())
        && let Some(mut m) = a.get_mut(h)
    {
        m.extension.fog = image.clone();
    }
    if let (Mat::Shaded(h), Some(a)) = (&mats.props, assets.props.as_mut())
        && let Some(mut m) = a.get_mut(h)
    {
        m.extension.fog = image.clone();
    }
    state.fog_bound = Some(want);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::field::tests::{painted, painted_features};
    use crate::terrain::mesh::main_diagonal;
    use af_core::testing::test_data;

    #[test]
    fn the_fog_darkens_the_skirt_but_not_the_waters_sky() {
        let data = test_data();
        let nav = painted(&data, 64);
        let field = Field::build(&FieldSource { data: &data, nav: &nav, seed: 2, features: None, crater: None });
        let mut p = ground_params(&field);
        let sky = SKY.to_linear();
        let sky = Vec4::new(sky.red, sky.green, sky.blue, 1.0);
        assert_eq!((p.sky, p.reflection), (sky, sky));
        set_fog(&mut p, Vec2::splat(128.0), true);
        assert!(p.sky.truncate().max_element() < 0.01, "the skirt fades into the dark: {}", p.sky);
        assert_eq!(p.reflection, sky, "the water still mirrors the sky");
        assert_eq!(p.fog.z, 1.0);
        set_fog(&mut p, Vec2::splat(128.0), false);
        assert_eq!((p.sky, p.reflection, p.fog.z), (sky, sky, 0.0));
    }

    #[test]
    fn the_published_surface_is_the_drawn_ground() {
        // Sample inside every dry map cell (mountains included, whose cells
        // split along either diagonal) and compare the published top with
        // the drawn triangles of the ground mesh.
        let data = test_data();
        let nav = painted(&data, 128);
        let features = painted_features();
        let field = Field::build(&FieldSource { data: &data, nav: &nav, seed: 4, features: Some(&features), crater: None });
        let surface = surface_of(&field);
        assert_eq!(surface.split.len(), 128 * 128);
        assert!(surface.split.iter().any(|s| *s), "some mountain cells split the other way");
        let mut worst: f32 = 0.0;
        for z in 0..128 {
            for x in 0..128 {
                let (fx, fz) = (x + field.skirt, z + field.skirt);
                if [(0, 0), (1, 0), (0, 1), (1, 1)].iter().any(|(dx, dz)| field.corner_wet(fx + dx, fz + dz)) {
                    continue;
                }
                let (h00, h10, h01, h11) = (field.height(fx, fz), field.height(fx + 1, fz), field.height(fx, fz + 1), field.height(fx + 1, fz + 1));
                for (u, v) in [(0.7, 0.2), (0.2, 0.7), (0.5, 0.5), (0.9, 0.85), (0.1, 0.15)] {
                    // The mesh's triangles: (00, 01, 11) and (00, 11, 10)
                    // along the main diagonal, (00, 01, 10) and (10, 01, 11)
                    // along the other.
                    let drawn = if main_diagonal(&field, fx, fz) {
                        if u >= v { h00 + (h10 - h00) * u + (h11 - h10) * v } else { h00 + (h11 - h01) * u + (h01 - h00) * v }
                    } else if u + v <= 1.0 {
                        h00 + (h10 - h00) * u + (h01 - h00) * v
                    } else {
                        h11 + (h01 - h11) * (1.0 - u) + (h10 - h11) * (1.0 - v)
                    };
                    let p = field.world_corner(fx, fz) + Vec2::new(u, v) * field.cell;
                    worst = worst.max((surface.top_at(p) - drawn).abs());
                }
            }
        }
        assert!(worst < 1e-4, "the published top strays {worst} m from the drawn ground");
    }
}
