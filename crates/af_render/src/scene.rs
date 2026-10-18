//! Lighting and the per-match scene lifetime.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;

use af_core::ext::render::FogTexture;

use crate::fog::{FogOverlay, MemoryPool};
use crate::ghost::GhostPool;
use crate::overlay::RallyPool;
use crate::terrain::TerrainState;

/// Every top-level entity af_render spawns for a match (light, terrain,
/// fog overlay, ghosts, markers). Despawned on leaving `GameState::Playing`.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RenderScene;

const SKY: Color = Color::srgb(0.07, 0.08, 0.1);
/// Where sunlight travels. The RTS view looks along -X -Z
/// (`af_input::VIEW_YAW`) and sees the +Z and +X faces of every box: this
/// sun comes from the view's upper left, so the +Z faces (building fronts)
/// are lit, the +X faces fall into shade and shadows reach to the right,
/// which gives every box two clearly different visible sides.
pub const SUN_DIRECTION: Vec3 = Vec3::new(0.35, -1.0, -0.75);

pub(crate) fn setup_scene(mut commands: Commands) {
    commands.insert_resource(ClearColor(SKY));
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.85, 0.9, 1.0), brightness: 450.0, ..default() });
    // A low-ish afternoon sun so boxes throw readable shadows. The camera
    // sits 25-150 m from its focus (`af_input`), so the first cascade
    // covers the whole default view and the last reaches the far edge of
    // the widest one.
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight { color: Color::srgb(1.0, 0.96, 0.88), illuminance: 6500.0, shadow_maps_enabled: true, ..default() },
        Transform::default().looking_to(SUN_DIRECTION, Vec3::Y),
        CascadeShadowConfigBuilder { num_cascades: 3, minimum_distance: 5.0, first_cascade_far_bound: 110.0, maximum_distance: 320.0, overlap_proportion: 0.2 }
            .build(),
        RenderScene,
    ));
}

pub(crate) fn teardown_scene(
    mut commands: Commands,
    scene: Query<Entity, With<RenderScene>>,
    mut terrain: ResMut<TerrainState>,
    mut ghosts: ResMut<GhostPool>,
    mut rally: ResMut<RallyPool>,
    mut fog: ResMut<FogOverlay>,
    mut memories: ResMut<MemoryPool>,
) {
    for e in &scene {
        commands.entity(e).try_despawn();
    }
    terrain.reset();
    *ghosts = GhostPool::default();
    *rally = RallyPool::default();
    fog.reset();
    memories.reset();
    commands.remove_resource::<FogTexture>();
}
