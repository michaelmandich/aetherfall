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

/// The haze for a camera `distance` metres from what it looks at: (where it
/// starts, where it would be full, how thick it gets at most), all scaled
/// with the zoom. The game's view (pitched about 54 degrees, a 32 degree
/// field of view) sees the ground from about 0.85 to 1.5 times its distance,
/// so the haze starts just beyond the focus and only softens the top of the
/// view, the far horizon and the off-map skirt by the same amount at every
/// play zoom (25-210 m); a view from kilometres away (the dev overview)
/// thins it out, so the whole map stays clear and saturated.
pub fn haze(distance: f32) -> (f32, f32, f32) {
    let d = distance.max(1.0);
    let start = d * 1.12;
    let end = start + d * 3.6;
    let thickness = 1.0 - 0.8 * smoothstep(220.0, 900.0, d);
    (start, end, thickness)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How far the camera is from what it looks at: along its view to the
/// ground plane (the RTS rig's distance from its focus), else from the
/// map's centre height.
pub fn focus_distance(t: &Transform) -> f32 {
    let (eye, fwd) = (t.translation, t.forward());
    if fwd.y < -0.05 && eye.y > 0.0 { eye.y / -fwd.y } else { eye.y.abs().max(50.0) }
}

/// Fit the haze to the zoom (see [`haze`]) whenever the camera moves (the
/// RTS camera is a root entity, so its `Transform` is where it is).
pub(crate) fn fit_haze(mut cameras: Query<(&Transform, &mut DistanceFog), (With<RtsCamera>, Changed<Transform>)>) {
    for (t, mut fog) in &mut cameras {
        let (start, end, thickness) = haze(focus_distance(t));
        let same = match fog.falloff {
            FogFalloff::Linear { start: s, end: e } => (s - start).abs() < 0.5 && (e - end).abs() < 0.5,
            _ => false,
        };
        if !same || (fog.color.alpha() - thickness).abs() > 0.005 {
            fog.falloff = FogFalloff::Linear { start, end };
            fog.color = SKY.with_alpha(thickness);
        }
    }
}

/// The look of the game's camera: tonemapping, a light haze toward the
/// horizon in the sky's colour (fitted to the zoom by [`fit_haze`]),
/// livelier colour and a soft bloom over bright glows only (aether, the
/// Starspire).
pub fn camera_look() -> impl Bundle {
    // Until the camera first moves: the game's starting zoom.
    let (start, end, thickness) = haze(66.0);
    (
        Tonemapping::TonyMcMapface,
        DistanceFog {
            color: SKY.with_alpha(thickness),
            directional_light_color: Color::srgba(1.0, 0.92, 0.75, 0.35),
            directional_light_exponent: 24.0,
            falloff: FogFalloff::Linear { start, end },
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

#[cfg(test)]
mod tests {
    use super::*;

    /// How much haze lies over ground `at` metres from the camera.
    fn haze_at(distance: f32, at: f32) -> f32 {
        let (start, end, thickness) = haze(distance);
        thickness * ((at - start) / (end - start)).clamp(0.0, 1.0)
    }

    #[test]
    fn the_haze_follows_the_zoom() {
        // Every play zoom: what the camera looks at is clear, the top of
        // the view (about 1.3-1.5 times as far) only softened.
        for d in [25.0, 66.0, 150.0, 210.0] {
            assert_eq!(haze_at(d, d), 0.0, "{d} m: the focus is clear");
            let top = haze_at(d, 1.5 * d);
            assert!(top > 0.0 && top < 0.15, "{d} m: the far edge of the view is softened, not washed out: {top}");
        }
        // The dev overview, kilometres away, stays clear across the map.
        for d in [900.0, 1600.0, 2500.0] {
            assert!(haze_at(d, 1.6 * d) < 0.06, "{d} m: {}", haze_at(d, 1.6 * d));
        }
    }

    #[test]
    fn the_camera_hazes_by_its_distance_from_its_focus() {
        let mut app = App::new();
        app.add_systems(Update, fit_haze);
        let eye = Vec3::new(30.0, 120.0, 90.0);
        let focus = Vec3::new(10.0, 0.0, 20.0);
        let cam = app.world_mut().spawn((RtsCamera, Transform::from_translation(eye).looking_at(focus, Vec3::Y), camera_look())).id();
        app.update();
        let t = *app.world().get::<Transform>(cam).unwrap();
        assert!((focus_distance(&t) - eye.distance(focus)).abs() < 1e-3);
        let (start, end, thickness) = haze(eye.distance(focus));
        let fog = app.world().get::<DistanceFog>(cam).unwrap();
        match fog.falloff {
            FogFalloff::Linear { start: s, end: e } => assert!((s - start).abs() < 0.5 && (e - end).abs() < 0.5, "{s}..{e}"),
            _ => panic!("linear haze"),
        }
        assert!((fog.color.alpha() - thickness).abs() < 0.01);
    }
}
