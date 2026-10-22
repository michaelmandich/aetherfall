//! The terrain's three materials: [`TerrainMaterial`] (the ground),
//! [`WaterMaterial`] (the water surface) and [`PropMaterial`] (brambles,
//! reeds, scree, meteor rock, the spire). Each is a `StandardMaterial`
//! (so sunlight, shadows and the distance fog stay Bevy's) extended with
//! one of the embedded WGSL shaders, which paints the surface, darkens it
//! with the fog of war and greys it off the map. Created once per match;
//! animation reads `globals.time`, so no material changes per frame.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

pub const TERRAIN_SHADER: &str = "embedded://af_render/terrain/terrain.wgsl";
pub const WATER_SHADER: &str = "embedded://af_render/terrain/water.wgsl";
pub const PROP_SHADER: &str = "embedded://af_render/terrain/props.wgsl";
/// Shared WGSL (`af_render::terrain_common`: the uniforms, noise, the fog
/// of war), imported by the three shaders above.
pub const COMMON_SHADER: &str = "embedded://af_render/terrain/common.wgsl";

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExt>;
pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExt>;
pub type PropMaterial = ExtendedMaterial<StandardMaterial, PropExt>;

/// What every terrain shader knows of the map (`GroundParams` in
/// `common.wgsl`).
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType, Reflect)]
pub struct GroundParams {
    /// xy: the map's min corner, zw: its max corner (world x / z).
    pub map: Vec4,
    /// xy: the class images' min corner, zw: 1 / their size (metres).
    pub field: Vec4,
    /// xy: 1 / the fog grid's size in metres, z: 1 while the fog of war is
    /// drawn, w: the skirt's width (metres).
    pub fog: Vec4,
    /// What the skirt fades into toward its far edge (linear): the sky and
    /// haze colour, or the dark of unexplored ground while the fog of war
    /// is drawn.
    pub sky: Vec4,
    /// The sky the water mirrors (linear): always the sky, whatever the
    /// fog of war does to the skirt.
    pub reflection: Vec4,
}

/// The ground's extension: the fog of war and the two class images.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TerrainExt {
    #[uniform(100)]
    pub params: GroundParams,
    #[texture(101)]
    #[sampler(102)]
    pub fog: Option<Handle<Image>>,
    #[texture(103)]
    #[sampler(104)]
    pub classes_a: Option<Handle<Image>>,
    #[texture(105)]
    #[sampler(106)]
    pub classes_b: Option<Handle<Image>>,
}

impl MaterialExtension for TerrainExt {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER.into()
    }
}

/// The water surface's extension: the fog of war.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct WaterExt {
    #[uniform(100)]
    pub params: GroundParams,
    #[texture(101)]
    #[sampler(102)]
    pub fog: Option<Handle<Image>>,
}

impl MaterialExtension for WaterExt {
    fn fragment_shader() -> ShaderRef {
        WATER_SHADER.into()
    }
}

/// The props' extension: the fog of war (props glow by their vertex
/// colour's alpha).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct PropExt {
    #[uniform(100)]
    pub params: GroundParams,
    #[texture(101)]
    #[sampler(102)]
    pub fog: Option<Handle<Image>>,
}

impl MaterialExtension for PropExt {
    fn fragment_shader() -> ShaderRef {
        PROP_SHADER.into()
    }
}

/// The base of every terrain material: white (the shaders paint), matte,
/// with the boxes' low reflectance so colours stay saturated.
pub fn ground_base() -> StandardMaterial {
    StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, reflectance: crate::assets::BOX_REFLECTANCE, ..default() }
}

/// The water's base: glossy, reflective enough for sun glints.
pub fn water_base() -> StandardMaterial {
    StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.08, reflectance: 0.6, ..default() }
}

/// The props' base: matte, both sides drawn (blades and shards are thin).
pub fn prop_base() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        reflectance: crate::assets::BOX_REFLECTANCE,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}
