//! Aetherfall renderer: the land, units, buildings, nodes, weapons,
//! projectiles/FX, health bars, selection, fog, ghosts; animated glTF
//! models for the unit types that have one ([`model`]) and static glTF
//! models for the building and node types that have one ([`statics`]),
//! both listed in the visual registry ([`registry`]).
//!
//! The land ([`terrain`]) is a height field cut into chunk meshes, painted
//! by its own shaders (ground, water, props), with the fog of war drawn
//! into it; the light and the camera's look are in [`scene`].
//!
//! Everything else visible is a box: one shared unit cube mesh, scaled and
//! placed per box, with materials from a cache keyed by quantised colour,
//! opacity and shading ([`look::MatKey`]), so materials are never created
//! per frame and Bevy batches boxes that share one.
//!
//! The renderer only reads sim state (never writes it, except counting down
//! `ClickPing`s, which it owns). Its systems run in `Update`, in
//! `ClientSet::Render`, while `GameState::Playing`, in four chained steps
//! ([`RenderStep`]); model loading and the observers are listed in
//! [`model`] and [`statics`]:
//!
//! | step | what |
//! |---|---|
//! | `Build` | the land once per map; how the local team sees each building (live, or as last seen in the fog); visuals for new units/buildings/nodes/FX/pings; rebuilds on upgrade, completed construction or a model that finished loading; rigging of spawned model scenes |
//! | `Animate` | tick interpolation (`Prev* -> *` by `Time<Fixed>::overstep_fraction`), weapon animation, shields, cargo, flyer shadows, foundations (boxes or models rising), node shrink, FX, pings; model clips and props, corpses; felled trees and stumps |
//! | `Overlay` | health/shield/progress bars, selection outlines, rally markers, placement ghosts |
//! | `Fog` | hide what the local team cannot see; the fog-of-war texture and handing it to the land's shaders; remembered dead buildings and nodes |
//!
//! Sim entities get `Transform` + `Visibility` and a child "visual root"
//! with their boxes (or model), so despawning a sim entity removes its
//! visuals. Scene entities (light, terrain chunks, ghosts, markers,
//! corpses, felled trees, stumps) carry [`scene::RenderScene`] and are
//! despawned on leaving `Playing`.

pub mod assets;
pub mod entities;
pub mod fog;
pub mod fx;
pub mod ghost;
pub mod look;
pub mod model;
pub mod motion;
pub mod overlay;
pub mod parts;
pub mod registry;
pub mod scene;
pub mod statics;
pub mod terrain;

use af_core::ext::fog::FogGhosts;
use af_core::ext::input::PlacementPreview;
use af_core::prelude::*;
use bevy::gltf::Gltf;
use bevy::prelude::*;

/// The steps of af_render's frame, chained inside `ClientSet::Render`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderStep {
    Build,
    Animate,
    Overlay,
    Fog,
}

/// Draws the match (see the crate docs). A client plugin: it needs Bevy's
/// render and asset plugins (`DefaultPlugins`, added before it) and the
/// af_core `CorePlugin`; it never touches sim state beyond `ClickPing`.
/// Models are used only when the app has the glTF loader (or a
/// [`model::ModelLibrary`] or [`statics::StaticLibrary`] was inserted);
/// otherwise everything is boxes. Without the render stack (headless
/// tests) the land is built all the same, with plain materials.
pub struct BoxRenderPlugin;

impl Plugin for BoxRenderPlugin {
    fn build(&self, app: &mut App) {
        let has_statics = resource_exists::<statics::StaticLibrary>;
        terrain::add_terrain_materials(app);
        app.init_resource::<terrain::TerrainState>()
            .init_resource::<ghost::GhostPool>()
            .init_resource::<overlay::RallyPool>()
            .init_resource::<fog::FogOverlay>()
            .init_resource::<fog::MemoryPool>()
            .init_resource::<model::rig::TeamTints>()
            .init_resource::<statics::Remnants>()
            .configure_sets(
                Update,
                (RenderStep::Build, RenderStep::Animate, RenderStep::Overlay, RenderStep::Fog)
                    .chain()
                    .in_set(ClientSet::Render)
                    .run_if(in_state(GameState::Playing).and_then(resource_exists::<assets::BoxAssets>)),
            )
            .add_systems(Startup, assets::init_box_assets)
            .add_systems(Startup, registry::load_visuals.run_if(resource_exists::<Assets<Gltf>>))
            .add_systems(
                Update,
                (
                    model::library::finish_model_loads.run_if(resource_exists::<model::ModelLibrary>),
                    statics::library::finish_static_loads.run_if(has_statics),
                )
                    .run_if(resource_exists::<Assets<Gltf>>),
            )
            .add_observer(model::rig::mark_scene_ready)
            .add_observer(model::rig::keep_corpse)
            .add_observer(statics::fall::fell_tree)
            .add_observer(scene::dress_camera)
            .add_systems(OnEnter(GameState::Playing), scene::setup_scene)
            .add_systems(OnExit(GameState::Playing), scene::teardown_scene)
            .add_systems(
                Update,
                (
                    terrain::build_terrain.run_if(resource_exists::<Nav>.and_then(resource_exists::<MapInfo>)),
                    (
                        (entities::attach_units, model::rig::rig_models.run_if(resource_exists::<model::ModelLibrary>)).chain(),
                        (fog::view_buildings, entities::attach_buildings).chain(),
                    )
                        .run_if(resource_exists::<Players>),
                    entities::attach_nodes,
                    fx::attach_fx,
                    fx::attach_pings,
                )
                    .in_set(RenderStep::Build),
            )
            .add_systems(
                Update,
                (
                    entities::sync_units,
                    entities::sync_static,
                    entities::animate_parts,
                    entities::shrink_nodes,
                    fx::sync_fx,
                    fx::tick_pings,
                    (model::drive::drive_models, model::drive::drive_corpses).run_if(resource_exists::<model::ModelLibrary>),
                    (statics::fall::drive_falls.run_if(has_statics), statics::fall::sink_stumps),
                )
                    .in_set(RenderStep::Animate),
            )
            .add_systems(
                Update,
                (
                    (overlay::update_bars, overlay::update_outlines, overlay::update_rally).run_if(resource_exists::<Players>),
                    ghost::update_ghosts.run_if(resource_exists::<PlacementPreview>),
                )
                    .in_set(RenderStep::Overlay),
            )
            .add_systems(
                Update,
                (
                    (fog::fog_units, fog::fog_structures, fog::fog_fx, fog::update_fog_overlay, fog::update_fog_memories.run_if(resource_exists::<FogGhosts>))
                        .run_if(resource_exists::<Players>),
                    terrain::bind_fog.after(fog::update_fog_overlay),
                )
                    .in_set(RenderStep::Fog),
            );
    }
}
