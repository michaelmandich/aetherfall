//! Building placement validity, shared by af_input (ghost colour) and
//! af_economy (authoritative). Units in the way do not block placement;
//! af_movement pushes them out of the new footprint.

use af_data::*;
use af_geom::{Obb, NavGrid};
use bevy::prelude::*;

use crate::resources::PlayerState;
use crate::rules::{self, Reason};
use crate::spatial::{SpatialIndex, SpatialKind};

/// Footprints are tested shrunk by this much, so buildings and wall
/// segments can sit exactly edge to edge despite float noise.
pub const FLUSH_TOLERANCE: f32 = 0.01;

/// Upper bound on segments in one wall drag.
pub const MAX_WALL_SEGMENTS: usize = 1024;

/// The footprint a placement is tested with: the def box at `pos`/`facing`
/// shrunk by [`FLUSH_TOLERANCE`]. Pass these as `extra` for things placed
/// earlier in the same batch (wall lines).
pub fn placed_footprint(data: &GameData, kind: BuildingTypeId, pos: Vec2, facing: f32) -> Obb {
    let mut o = Obb::from_size(pos, data.building(kind).size, facing);
    o.half = (o.half - Vec2::splat(FLUSH_TOLERANCE)).max(Vec2::ZERO);
    o
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementError {
    Rule(Reason),
    OutOfBounds,
    /// Terrain not buildable (water, cliff, thicket...) or already occupied.
    Blocked,
    /// Must be placed on a free node of the required type.
    NeedsNode,
    NodeClaimed,
}

impl PlacementError {
    pub fn describe(&self, data: &GameData) -> String {
        match self {
            PlacementError::Rule(r) => r.describe(data),
            PlacementError::OutOfBounds => "Outside the map".into(),
            PlacementError::Blocked => "Cannot build there".into(),
            PlacementError::NeedsNode => "Must be built on its resource".into(),
            PlacementError::NodeClaimed => "Already claimed".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Possibly snapped (onto a node) position.
    pub pos: Vec2,
    pub facing: f32,
    /// The node being claimed, if any.
    pub node: Option<Entity>,
}

/// Can `player` place `kind` at `pos`/`facing`? Checks faction/age/limit
/// rules (`check_cost` also checks resources), terrain, overlap with
/// buildings and blocking nodes, and node requirements (snapping onto the
/// node). `extra` are footprints already placed this tick (wall lines).
#[allow(clippy::too_many_arguments)] // the shared API of af_economy, af_input and af_ai
pub fn check_placement(
    data: &GameData,
    nav: &NavGrid,
    spatial: &SpatialIndex,
    player: &PlayerState,
    kind: BuildingTypeId,
    pos: Vec2,
    facing: f32,
    check_cost: bool,
    extra: &[Obb],
) -> Result<Placement, PlacementError> {
    if !(pos.is_finite() && facing.is_finite()) {
        return Err(PlacementError::OutOfBounds);
    }
    // Rules first, cost last, so the caller hears about the real blocker.
    rules::can_build(data, player, kind, false).map_err(PlacementError::Rule)?;
    let def = data.building(kind);
    let mut pos = pos;
    let mut facing = facing;
    let mut node = None;
    if let Some(req) = def.requires_node_id {
        // Snap onto the nearest node of the right type within reach.
        let found = spatial.nearest(pos, 4.0, |e| e.kind == SpatialKind::Node && e.type_index == req.0);
        let Some(n) = found else { return Err(PlacementError::NeedsNode) };
        pos = n.pos;
        facing = n.obb.yaw;
        node = Some(n.entity);
        // Claimed = a building already sits there.
        let claimed = spatial
            .within(pos, 0.5)
            .iter()
            .any(|e| e.kind == SpatialKind::Building && e.pos.distance(pos) < 0.5);
        if claimed {
            return Err(PlacementError::NodeClaimed);
        }
    }
    let obb = placed_footprint(data, kind, pos, facing);
    let (min, max) = obb.aabb();
    let size = nav.world_size() + nav.origin();
    if min.x < nav.origin().x || min.y < nav.origin().y || max.x > size.x || max.y > size.y {
        return Err(PlacementError::OutOfBounds);
    }
    if !nav.area_buildable(&obb) {
        return Err(PlacementError::Blocked);
    }
    let mut blocked = false;
    spatial.for_each_near(pos, obb.bounding_radius(), |e| {
        let solid = match e.kind {
            SpatialKind::Building => true,
            // Any node blocks except the one being claimed.
            SpatialKind::Node => true,
            SpatialKind::Unit => false,
        };
        if solid && Some(e.entity) != node && e.obb.overlaps(&obb) {
            blocked = true;
        }
    });
    if blocked || extra.iter().any(|o| o.overlaps(&obb)) {
        return Err(PlacementError::Blocked);
    }
    if check_cost {
        rules::can_build(data, player, kind, true).map_err(PlacementError::Rule)?;
    }
    Ok(Placement { pos, facing, node })
}

/// Split a wall drag from `from` to `to` into segment centres + yaw for a
/// wall building whose `size[0]` is the segment length. Segments are laid
/// end to end starting at `from`.
pub fn wall_segments(data: &GameData, kind: BuildingTypeId, from: Vec2, to: Vec2) -> Vec<(Vec2, f32)> {
    if !(from.is_finite() && to.is_finite()) {
        return Vec::new();
    }
    let seg = data.building(kind).size[0].max(0.5);
    let d = to - from;
    let len = d.length();
    if len < 0.01 {
        return vec![(from, 0.0)];
    }
    let dir = d / len;
    // yaw whose `right` axis is `dir`: right = (cos a, -sin a).
    let yaw = af_geom::dmath::atan2(-dir.y, dir.x);
    let n = ((len / seg).round().max(1.0) as usize).min(MAX_WALL_SEGMENTS);
    (0..n).map(|i| (from + dir * (seg * (i as f32 + 0.5)), yaw)).collect()
}
