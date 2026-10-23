//! Shared items added by af_render (other crates may use them).
//! Only af_render edits this file. Never modify items in other af_core files.

#[allow(unused_imports)]
use crate::prelude::*;
#[allow(unused_imports)]
use bevy::prelude::*;

/// The fog-of-war texture af_render keeps in step with [`FogOfWar`] for the
/// local team, about ten times a second: one RGBA pixel per fog cell, image
/// row `y` = fog row `y` (world +Z grows with the row index, so row 0 is the
/// map's z = 0 edge). Colour is black; alpha is 0 in sight, 128 explored,
/// 255 unexplored. Inserted by af_render while fog is shown (a player view
/// with fog enabled); absent for spectators and fog-off matches. af_ui may
/// draw it over the minimap (flip vertically if the minimap puts +Z up).
#[derive(Resource, Clone, Debug)]
pub struct FogTexture(pub Handle<Image>);

/// How far from y = 0 [`ground_height`] may put the visual ground.
///
/// The rest of the game treats the ground as the plane y = 0: af_core's
/// `SpatialIndex` stores `Altitude` as the base height, af_input picks
/// with a ray against those columns and the y = 0 plane, and af_combat
/// aims at `Altitude` + offset. So af_render keeps every surface that
/// something stands on within this distance of y = 0, and a box drawn on
/// it still overlaps the column the sim picks and shoots at (for an
/// infantry box, the ray through its centre passes through the column).
/// Climbable highlands are therefore drawn this high, not at their
/// `TerrainDef::height`, until those crates measure heights from
/// [`ground_height`].
pub const MAX_GROUND_OFFSET: f32 = 0.6;

/// Height of the visual ground at `p` that ground units, resource nodes,
/// buildings and markers stand on (af_render draws them there; flyers keep
/// their absolute `Altitude`). Terrain height is purely visual, so this is
/// the renderer's rule, shared so aiming (af_combat) and picking (af_input)
/// can match what is drawn:
///
/// * climbable terrain (its `passable` admits `Climber`: rocky highlands):
///   its top, so climbers walk on the rock;
/// * terrain below 0 (water, marsh): its surface;
/// * anything else (grass, dunes, thickets): 0, so units wade through
///   brambles rather than walk on them;
///
/// always within [`MAX_GROUND_OFFSET`] of 0.
pub fn ground_height(data: &GameData, nav: &NavGrid, p: Vec2) -> f32 {
    let id = nav.terrain_at(nav.cell_of(p));
    data.terrain.get(id as usize).map_or(0.0, terrain_ground)
}

/// The ground height [`ground_height`] uses for one terrain type.
pub fn terrain_ground(def: &TerrainDef) -> f32 {
    let surface = if def.passable.contains(&MoveFlag::Climber) { def.height } else { def.height.min(0.0) };
    surface.clamp(-MAX_GROUND_OFFSET, MAX_GROUND_OFFSET)
}

/// How many of the local player's selected units have their routes drawn
/// by af_render's path overlay (at most so many, those nearest the centre
/// of the view): `drawn` of the `wanted` that have a route to show (with
/// the paths shown, those going somewhere or with an order queued; else
/// the patrolling ones). Kept up to date by af_render each frame of a
/// match; af_ui says so when some are left out.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoutesShown {
    pub drawn: usize,
    pub wanted: usize,
}

impl RoutesShown {
    /// Some routes are left out.
    pub fn capped(&self) -> bool {
        self.wanted > self.drawn
    }
}
