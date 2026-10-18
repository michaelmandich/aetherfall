//! Aetherfall box renderer: terrain, units, buildings, nodes, weapons,
//! projectiles/FX, health bars, selection, fog, ghosts.
//!
//! Everything visible is a box: one shared unit cube mesh, scaled and
//! placed per box, with materials from a cache keyed by quantised colour,
//! opacity and shading ([`look::MatKey`]), so materials are never created
//! per frame and Bevy batches boxes that share one.
//!
//! The renderer only reads sim state (never writes it, except counting down
//! `ClickPing`s, which it owns). Every system runs in `Update`, in
//! `ClientSet::Render`, while `GameState::Playing`, in four chained steps
//! ([`RenderStep`]):
//!
//! | step | what |
//! |---|---|
//! | `Build` | terrain once per map; how the local team sees each building (live, or as last seen in the fog); visuals for new units/buildings/nodes/FX/pings; rebuilds on upgrade or completed construction |
//! | `Animate` | tick interpolation (`Prev* -> *` by `Time<Fixed>::overstep_fraction`), weapon animation, shields, cargo, flyer shadows, foundations, node shrink, FX, pings |
//! | `Overlay` | health/shield/progress bars, selection outlines, rally markers, placement ghosts |
//! | `Fog` | hide what the local team cannot see; the fog overlay texture |
//!
//! Sim entities get `Transform` + `Visibility` and a child "visual root"
//! with their boxes, so despawning a sim entity removes its visuals.
//! Scene entities (light, terrain, overlay, ghosts, markers) carry
//! [`scene::RenderScene`] and are despawned on leaving `Playing`.

pub mod assets;
pub mod entities;
pub mod fog;
pub mod fx;
pub mod ghost;
pub mod look;
pub mod motion;
pub mod overlay;
pub mod parts;
pub mod scene;
pub mod terrain;

use af_core::ext::fog::FogGhosts;
use af_core::ext::input::PlacementPreview;
use af_core::prelude::*;
use bevy::prelude::*;

/// The steps of af_render's frame, chained inside `ClientSet::Render`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderStep {
    Build,
    Animate,
    Overlay,
    Fog,
}

/// Draws the match as boxes (see the crate docs). A client plugin: it needs
/// Bevy's render and asset plugins (`DefaultPlugins`) and the af_core
/// `CorePlugin`; it never touches sim state beyond `ClickPing`.
pub struct BoxRenderPlugin;

impl Plugin for BoxRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<terrain::TerrainState>()
            .init_resource::<ghost::GhostPool>()
            .init_resource::<overlay::RallyPool>()
            .init_resource::<fog::FogOverlay>()
            .init_resource::<fog::MemoryPool>()
            .configure_sets(
                Update,
                (RenderStep::Build, RenderStep::Animate, RenderStep::Overlay, RenderStep::Fog)
                    .chain()
                    .in_set(ClientSet::Render)
                    .run_if(in_state(GameState::Playing).and_then(resource_exists::<assets::BoxAssets>)),
            )
            .add_systems(Startup, assets::init_box_assets)
            .add_systems(OnEnter(GameState::Playing), scene::setup_scene)
            .add_systems(OnExit(GameState::Playing), scene::teardown_scene)
            .add_systems(
                Update,
                (
                    terrain::build_terrain.run_if(resource_exists::<Nav>.and_then(resource_exists::<MapInfo>)),
                    (entities::attach_units, (fog::view_buildings, entities::attach_buildings).chain()).run_if(resource_exists::<Players>),
                    entities::attach_nodes,
                    fx::attach_fx,
                    fx::attach_pings,
                )
                    .in_set(RenderStep::Build),
            )
            .add_systems(
                Update,
                (entities::sync_units, entities::sync_static, entities::animate_parts, entities::shrink_nodes, fx::sync_fx, fx::tick_pings)
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
                (fog::fog_units, fog::fog_structures, fog::fog_fx, fog::update_fog_overlay, fog::update_fog_memories.run_if(resource_exists::<FogGhosts>))
                    .run_if(resource_exists::<Players>)
                    .in_set(RenderStep::Fog),
            );
    }
}
