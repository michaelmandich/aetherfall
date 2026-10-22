//! Shared items added by af_input (other crates may use them).
//! Only af_input edits this file. Never modify items in other af_core files.

use crate::placement::PlacementError;
use af_data::BuildingTypeId;
use bevy::prelude::*;

/// Marks the RTS camera. af_input spawns exactly one on entering
/// `GameState::Playing`, drives its `Transform` from `CameraRig`, and
/// despawns it on exit (af_ui owns the menu camera).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct RtsCamera;

/// The local player's control groups, indexed by digit key (`groups[0]` is
/// key 0). Written by af_input; af_ui may display them. Entries may refer to
/// entities that have since died; readers should skip missing ones.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct ControlGroups {
    pub groups: [Vec<Entity>; 10],
}

impl ControlGroups {
    /// Members of group `digit` (empty for digits above 9).
    pub fn members(&self, digit: usize) -> &[Entity] {
        self.groups.get(digit).map_or(&[], |g| g.as_slice())
    }

    /// Replace group `digit` with `entities` (deduplicated, stable order).
    pub fn set(&mut self, digit: usize, entities: &[Entity]) {
        if let Some(g) = self.groups.get_mut(digit) {
            g.clear();
            for e in entities {
                if !g.contains(e) {
                    g.push(*e);
                }
            }
        }
    }

    /// Append `entities` that are not yet in group `digit`.
    pub fn add(&mut self, digit: usize, entities: &[Entity]) {
        if let Some(g) = self.groups.get_mut(digit) {
            for e in entities {
                if !g.contains(e) {
                    g.push(*e);
                }
            }
        }
    }

    /// Drop every entity for which `alive` is false, from every group.
    pub fn retain_alive(&mut self, mut alive: impl FnMut(Entity) -> bool) {
        for g in &mut self.groups {
            g.retain(|e| alive(*e));
        }
    }

    pub fn clear(&mut self) {
        for g in &mut self.groups {
            g.clear();
        }
    }
}

/// A stand-in for the mouse: while it holds a position (logical window
/// pixels, like `Window::cursor_position`), af_input reads the cursor from
/// it instead of the window, and af_ui treats it as pointing at the world,
/// not the HUD, and the real mouse wheel does not zoom. For tooling that
/// must frame a screenshot without the real mouse interfering (edge
/// scrolling, wheel zoom, hover), or that points at a spot on
/// purpose (a placement ghost). Absent or `None` in normal play.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorOverride(pub Option<Vec2>);

/// One footprint a world click would place in the current placement mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacementGhost {
    pub kind: BuildingTypeId,
    /// Snapped position (e.g. onto a vent) when the spot itself is valid,
    /// even if the player cannot pay for it; else the cursor spot.
    pub pos: Vec2,
    pub facing: f32,
    /// Why it cannot be placed here, or `None` when valid.
    pub error: Option<PlacementError>,
}

/// What `InputMode::Place` / `PlaceWall` would build at the cursor this frame:
/// one ghost for a building, one per segment for a wall line (a single
/// ghost before the line's start is set). Empty in other modes or when the
/// cursor is off the map or over UI. Written by af_input in
/// `ClientSet::Input` with the same `check_placement` the economy uses,
/// judged while the fog is on from what the local team knows: buildings
/// and nodes it has never seen (`SeenBy`) neither block a ghost nor take a
/// node building's snap, and dead ones it still remembers (`FogGhosts`)
/// block, so the preview gives nothing away; the economy still runs the
/// full check when the order arrives. A wall line is planned the way af_economy lays it: segments in order,
/// each paid for as it is placed, so the segments past what the player can
/// afford (or past a build limit) carry that `Rule` error, and a blocked
/// segment only leaves a gap. af_render can draw it (green/red) and af_ui
/// can show `error`.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct PlacementPreview {
    pub ghosts: Vec<PlacementGhost>,
}

impl PlacementPreview {
    /// True when at least one ghost exists and every ghost is valid.
    pub fn all_valid(&self) -> bool {
        !self.ghosts.is_empty() && self.ghosts.iter().all(|g| g.error.is_none())
    }
}

/// Whether af_render draws the routes of the local player's selected
/// units: where each is walking and the orders it has queued (a selected
/// patrolling unit's patrol loop is drawn either way). af_input
/// initialises it, flips `on` with P and sets `held` each frame while
/// Shift is down (queued orders are being given, as in StarCraft); af_ui's
/// HUD button flips `on` too. A preference: it survives into the next
/// match.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShowPaths {
    /// The player's choice (P, or the HUD button).
    pub on: bool,
    /// Shift is held this frame.
    pub held: bool,
}

impl ShowPaths {
    /// Routes are drawn this frame.
    pub fn visible(&self) -> bool {
        self.on || self.held
    }
}
