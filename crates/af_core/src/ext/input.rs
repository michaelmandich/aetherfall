//! Shared items added by af_input (other crates may use them).
//! Only af_input edits this file. Never modify items in other af_core files.

use crate::components::ClickPing;
use crate::placement::PlacementError;
use crate::resources::TargetAction;
use af_data::{BuildingTypeId, Rgb};
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

/// What a click marker confirms: the kind of order the local player just
/// gave. It picks the marker's colour ([`OrderMark::color`]) and shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OrderMark {
    Move,
    Attack,
    AttackMove,
    Gather,
    Build,
    Repair,
    Rally,
    Patrol,
    /// A building placed: its footprint, centred on the ping and turned by
    /// `facing`, flashes on the ground before the foundation appears.
    Place { kind: BuildingTypeId, facing: f32 },
}

impl OrderMark {
    /// The marker's colour (sRGB), also written to `ClickPing::color`.
    pub fn color(self) -> Rgb {
        match self {
            OrderMark::Move => [0.3, 1.0, 0.35],
            OrderMark::Attack => [1.0, 0.2, 0.16],
            OrderMark::AttackMove => [1.0, 0.45, 0.1],
            OrderMark::Gather => [1.0, 0.84, 0.22],
            OrderMark::Build | OrderMark::Place { .. } => [1.0, 0.64, 0.2],
            OrderMark::Repair => [0.45, 0.68, 0.98],
            OrderMark::Rally => [1.0, 0.95, 0.72],
            OrderMark::Patrol => [0.25, 0.9, 1.0],
        }
    }

    /// The mark for a targeting click that hit no entity (on the ground or
    /// the minimap): attacks there are attack-moves.
    pub fn of_target(action: TargetAction) -> OrderMark {
        match action {
            TargetAction::Move => OrderMark::Move,
            TargetAction::AttackMove | TargetAction::Attack => OrderMark::AttackMove,
            TargetAction::Patrol => OrderMark::Patrol,
            TargetAction::Rally => OrderMark::Rally,
            TargetAction::Gather => OrderMark::Gather,
            TargetAction::Repair => OrderMark::Repair,
        }
    }
}

/// Carried next to [`ClickPing`] on a click marker (a client entity, never
/// a unit): which order the click gave, so af_render can draw it as that
/// order. Written by af_input (world clicks) and af_ui (minimap clicks),
/// spawned together with its ping by [`ClickMark::ping`]; a `ClickPing`
/// without one is drawn as a plain point marker in its colour.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ClickMark {
    pub kind: OrderMark,
    /// The entity the order points at (the unit attacked or followed, the
    /// node or field gathered, the foundation built, the building or
    /// machine repaired, a rally's entity): the marker hugs its footprint
    /// and follows it while it moves.
    pub target: Option<Entity>,
    /// A shift-queued order: drawn smaller and quicker, with a dot that
    /// lingers so a chain of queued clicks reads.
    pub queued: bool,
}

impl ClickMark {
    /// Seconds a marker lives (its ping's starting `time_left`); af_render
    /// plays the whole animation within it.
    pub const SECS: f32 = 0.75;
    /// Seconds a queued marker lives (its dot lingers).
    pub const QUEUED_SECS: f32 = 1.2;

    pub fn new(kind: OrderMark, target: Option<Entity>, queued: bool) -> ClickMark {
        ClickMark { kind, target, queued }
    }

    /// The marker's whole life in seconds.
    pub fn life(&self) -> f32 {
        if self.queued { Self::QUEUED_SECS } else { Self::SECS }
    }

    /// What to spawn for this mark at `pos`: the ping, coloured and timed
    /// for the order, and the mark itself.
    pub fn ping(self, pos: Vec2) -> (ClickPing, ClickMark) {
        (ClickPing { pos, time_left: self.life(), color: self.kind.color() }, self)
    }
}
