//! Custom meshes, `ExtendedMaterial`, embedded WGSL and shader imports
//! (section 18 of `docs/bevy-0.19-notes.md`). The same pattern af_render's
//! terrain uses.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{Shader, ShaderRef};

/// `embedded_asset!(app, "probe_ground.wgsl")` called from this file
/// (`src/custom_material.rs`) serves the file as
/// `embedded://<crate name>/probe_ground.wgsl`.
const GROUND_SHADER: &str = "embedded://zz_api_probe/probe_ground.wgsl";
/// Imported by the shader above (`#import zz_api_probe::common::wave`).
const COMMON_SHADER: &str = "embedded://zz_api_probe/probe_common.wgsl";

/// A `StandardMaterial` whose fragment shader we replace.
pub type GroundMaterial = ExtendedMaterial<StandardMaterial, GroundExt>;

/// Uniforms: one struct at a binding of 100 or more (the base material's
/// bindings use the low numbers). Pack into `Vec4`s to stay std140-safe.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType, Reflect)]
pub struct GroundParams {
    /// rgb: tint, a: wave strength.
    pub tint: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct GroundExt {
    #[uniform(100)]
    pub params: GroundParams,
    // `None` binds a default (white, alpha 1) image: use a flag in the
    // uniforms to tell "no texture" apart.
    #[texture(101)]
    #[sampler(102)]
    pub mask: Option<Handle<Image>>,
}

impl MaterialExtension for GroundExt {
    fn fragment_shader() -> ShaderRef {
        GROUND_SHADER.into()
    }
}

/// Keeps the imported module loaded: an `#import`ed shader must exist as a
/// `Shader` asset when pipelines compile.
#[derive(Resource)]
struct ImportedShaders(#[allow(dead_code)] Handle<Shader>);

pub struct CustomMaterialPlugin;

impl Plugin for CustomMaterialPlugin {
    fn build(&self, app: &mut App) {
        // Only with the render stack: `MaterialPlugin` needs it, and headless
        // apps (MinimalPlugins + AssetPlugin) have no `Shader` assets.
        if !app.is_plugin_added::<bevy::pbr::PbrPlugin>() {
            return;
        }
        // Needs AssetPlugin first (it adds the EmbeddedAssetRegistry).
        bevy::asset::embedded_asset!(app, "probe_common.wgsl");
        bevy::asset::embedded_asset!(app, "probe_ground.wgsl");
        app.add_plugins(MaterialPlugin::<GroundMaterial>::default());
        let common = app.world().resource::<AssetServer>().load::<Shader>(COMMON_SHADER);
        app.insert_resource(ImportedShaders(common))
            .add_systems(Startup, spawn_custom_mesh);
    }
}

/// A flat 8 x 8 m grid of quads with vertex colours, as one mesh.
pub fn grid_mesh(cells: u32, size: f32) -> (Mesh, Aabb) {
    let n = cells + 1;
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    for z in 0..n {
        for x in 0..n {
            let p = Vec3::new(x as f32, 0.0, z as f32) * size / cells as f32;
            positions.push(p.to_array());
            // Vertex colour multiplies the material's base colour (linear).
            colors.push([x as f32 / cells as f32, 0.6, z as f32 / cells as f32, 1.0]);
        }
    }
    let mut indices = Vec::new();
    for z in 0..cells {
        for x in 0..cells {
            let (a, b, c, d) = (z * n + x, z * n + x + 1, (z + 1) * n + x, (z + 1) * n + x + 1);
            // Counter-clockwise seen from above (+Y): front faces up.
            indices.extend([a, c, d, a, d, b].map(|i| i as u16));
        }
    }
    let count = positions.len();
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U16(indices));
    // RENDER_WORLD drops the CPU copy after upload: give the entity its
    // bounds yourself rather than rely on them being computed from it.
    (mesh, Aabb::from_min_max(Vec3::ZERO, Vec3::new(size, 0.0, size)))
}

fn spawn_custom_mesh(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<GroundMaterial>>) {
    let (mesh, aabb) = grid_mesh(8, 8.0);
    let material = materials.add(GroundMaterial {
        base: StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() },
        extension: GroundExt { params: GroundParams { tint: Vec4::new(1.0, 0.9, 0.8, 0.3) }, mask: None },
    });
    commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), aabb, Transform::from_xyz(-12.0, 0.01, -4.0)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_apps_skip_the_material_but_build_meshes() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default())).init_asset::<Mesh>();
        app.add_plugins(CustomMaterialPlugin);
        app.update();
        assert!(app.world().get_resource::<Assets<GroundMaterial>>().is_none(), "no MaterialPlugin without PbrPlugin");
        let (mesh, aabb) = grid_mesh(4, 2.0);
        assert_eq!(mesh.count_vertices(), 25);
        assert_eq!(mesh.indices().unwrap().len(), 4 * 4 * 6);
        assert_eq!(aabb.max().x, 2.0);
    }
}
