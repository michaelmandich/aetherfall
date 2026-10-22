//! Light, sky, the camera's look and the per-match scene lifetime.
//!
//! A bright, warm afternoon (`docs/design/maps-and-terrain.md` 6.4): a warm
//! sun, sky-tinted ambient light, a light hazy blue sky with distance fog
//! matched to it, a touch of extra saturation in the colour grading and a
//! soft bloom so aether glows.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};

use af_core::ext::input::RtsCamera;
use af_core::ext::render::{FogTexture, TerrainSurface};

use crate::fog::{FogOverlay, MemoryPool};
use crate::ghost::GhostPool;
use crate::overlay::RallyPool;
use crate::statics::Remnants;
use crate::terrain::TerrainState;

/// Every top-level entity af_render spawns for a match (light, terrain,
/// ghosts, markers, corpses, felled trees, stumps). Despawned on leaving
/// `GameState::Playing`.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RenderScene;

/// The sky: a light, hazy blue. The distance fog and the far edge of the
/// off-map skirt fade into it.
pub const SKY: Color = Color::srgb(0.63, 0.75, 0.86);
/// Where sunlight travels. The RTS view looks along -X -Z
/// (`af_input::VIEW_YAW`) and sees the +Z and +X faces of every box: this
/// sun comes from the view's upper left, so the +Z faces (building fronts)
/// are lit, the +X faces fall into shade and shadows reach to the right,
/// which gives every box two clearly different visible sides. It stands
/// about 40 degrees high: low enough that mountains and highland scarps
/// show their relief, high enough that shadows stay short.
pub const SUN_DIRECTION: Vec3 = Vec3::new(0.4, -0.78, -0.75);

pub(crate) fn setup_scene(mut commands: Commands) {
    commands.insert_resource(ClearColor(SKY));
    // Sky-tinted fill light, so shade is cool blue and sunlit sides warm.
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.8, 0.87, 1.0), brightness: 560.0, ..default() });
    // A warm afternoon sun. The camera sits 25-150 m from its focus
    // (`af_input`), so the first cascade covers the whole default view and
    // the last reaches the far edge of the widest one.
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            color: Color::srgb(1.0, 0.94, 0.82),
            illuminance: 7_500.0,
            shadow_maps_enabled: true,
            // Mountains and the props shadow the smooth ground: biased
            // enough against acne on long slopes, little enough that
            // shadows stay attached to unit and building feet.
            shadow_depth_bias: 0.03,
            shadow_normal_bias: 1.4,
            ..default()
        },
        Transform::default().looking_to(SUN_DIRECTION, Vec3::Y),
        CascadeShadowConfigBuilder { num_cascades: 3, minimum_distance: 5.0, first_cascade_far_bound: 110.0, maximum_distance: 320.0, overlap_proportion: 0.2 }
            .build(),
        RenderScene,
    ));
}

/// The look of the game's camera: tonemapping, a light haze toward the
/// horizon in the sky's colour, livelier colour and a soft bloom over
/// bright glows only (aether, the Starspire).
pub fn camera_look() -> impl Bundle {
    (
        Tonemapping::TonyMcMapface,
        DistanceFog {
            color: SKY,
            directional_light_color: Color::srgba(1.0, 0.92, 0.75, 0.35),
            directional_light_exponent: 24.0,
            falloff: FogFalloff::Linear { start: 170.0, end: 900.0 },
        },
        ColorGrading {
            global: ColorGradingGlobal { post_saturation: 1.04, ..default() },
            shadows: ColorGradingSection { saturation: 1.05, ..default() },
            midtones: ColorGradingSection { saturation: 1.04, contrast: 1.05, ..default() },
            highlights: ColorGradingSection::default(),
        },
        Bloom {
            intensity: 0.12,
            low_frequency_boost: 0.5,
            prefilter: BloomPrefilter { threshold: 1.0, threshold_softness: 0.5 },
            composite_mode: BloomCompositeMode::Additive,
            ..Bloom::NATURAL
        },
    )
}

/// Dress the RTS camera (af_input spawns it) in [`camera_look`].
pub(crate) fn dress_camera(add: On<Add, RtsCamera>, mut commands: Commands) {
    commands.entity(add.entity).try_insert(camera_look());
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn teardown_scene(
    mut commands: Commands,
    scene: Query<Entity, With<RenderScene>>,
    mut terrain: ResMut<TerrainState>,
    mut ghosts: ResMut<GhostPool>,
    mut rally: ResMut<RallyPool>,
    mut fog: ResMut<FogOverlay>,
    mut memories: ResMut<MemoryPool>,
    mut remnants: ResMut<Remnants>,
) {
    for e in &scene {
        commands.entity(e).try_despawn();
    }
    terrain.reset();
    *ghosts = GhostPool::default();
    *rally = RallyPool::default();
    fog.reset();
    memories.reset();
    remnants.reset();
    commands.remove_resource::<FogTexture>();
    commands.remove_resource::<TerrainSurface>();
}
