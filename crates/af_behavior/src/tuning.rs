//! Distances (metres, box edge to box edge unless noted) and timings of
//! unit behaviour.

/// `MoveGoal::arrive` for point moves (centre distance).
pub const MOVE_ARRIVE: f32 = 0.35;
/// How close a follower stays to the unit it follows.
pub const FOLLOW_ARRIVE: f32 = 1.5;
/// `MoveGoal::arrive` when walking up to a node, drop site or foundation.
pub const WORK_ARRIVE: f32 = 0.3;
/// A worker this close to its node, site or drop site can work it.
pub const WORK_REACH: f32 = 0.75;

/// A settled walk-up goal (to a node, site or followed unit) is re-issued
/// once the target is this much farther than `arrive` (it moved away after
/// we arrived). Chases re-issue as soon as the target is out of reach.
pub const RESUME_SLACK: f32 = 0.3;
/// Chasers aim for this fraction of their weapon range...
pub const CHASE_FRACTION: f32 = 0.9;
/// ...and stop chasing (stand and fire) inside this fraction of it.
pub const STOP_FRACTION: f32 = 0.95;
/// A unit inside every weapon's minimum range steps back to this much
/// beyond it.
pub const BACKOFF_MARGIN: f32 = 0.5;
/// A back-off goal is kept while the ideal spot stays this close to it.
pub const BACKOFF_SLACK: f32 = 1.0;
/// Path failures in a row before an order gives up.
pub const MAX_PATH_FAILURES: u8 = 3;
/// Ticks an unreachable hostile, node or drop site is left out of
/// automatic choices (~5 s).
pub const UNREACHABLE_TICKS: u64 = 150;
/// How many unreachable entities a unit remembers at once.
pub const UNREACHABLE_MEMORY: usize = 8;

/// Ticks between hostile scans while attack-moving, patrolling or holding (~0.3 s).
pub const SCAN_TICKS: u64 = 9;
/// Ticks between stance / healer decisions of idle units (~0.5 s).
pub const STANCE_TICKS: u64 = 15;
/// An engaged target farther than `vision * LEASH_VISION_MULT` is dropped.
pub const LEASH_VISION_MULT: f32 = 1.5;
/// Defensive units give up a chase this far (centre distance) from their post.
pub const DEFENSIVE_LEASH: f32 = 12.0;
/// A defensive unit back within this distance of its post stops returning.
pub const HOME_ARRIVE: f32 = 1.0;
/// Seconds after being hit during which an idle unit retaliates.
pub const RETALIATE_WINDOW: f32 = 3.0;
/// A new hostile must score this much better to replace the current one.
pub const RETARGET_MARGIN: f32 = 2.0;

/// Target-selection penalties (metres added to the distance) so units
/// prefer enemies that can fight back.
pub const PENALTY_HARMLESS_UNIT: f32 = 4.0;
pub const PENALTY_ARMED_BUILDING: f32 = 6.0;
pub const PENALTY_HARMLESS_BUILDING: f32 = 14.0;

/// Replacement node search around a depleted node: same kind first, then
/// anything yielding the same resource (centre distance).
pub const SAME_KIND_RADIUS: f32 = 25.0;
pub const SAME_RESOURCE_RADIUS: f32 = 45.0;
/// Builders of a new drop site look this far around it for work.
pub const NEW_DROP_SITE_RADIUS: f32 = 25.0;
/// A node is workable when an open nav cell centre lies within
/// `WORK_ARRIVE` + a worker's half-width + this of its box (as af_movement
/// walks up to it; metres).
pub const NODE_REACH_SLACK: f32 = 0.5;
/// A group sent to one node spills over to same-kind nodes this close to it.
pub const GATHER_SPREAD_RADIUS: f32 = 10.0;
/// Room each worker needs along a node's perimeter.
pub const GATHER_SPACING: f32 = 0.5;
pub const MAX_WORKERS_PER_NODE: usize = 8;

/// Total build speed of `n` builders is `n^BUILDER_EXPONENT` (mild
/// diminishing returns).
pub const BUILDER_EXPONENT: f32 = 0.8;
/// Hit-point fraction of a fresh foundation (matches af_core::spawn).
pub const FOUNDATION_HP: f32 = 0.05;
/// No unit is healed by more healers at once.
pub const MAX_HEALERS_PER_TARGET: u8 = 3;

/// A point order re-issued this close to the current one continues it.
pub const REISSUE_SLACK: f32 = 3.0;
/// Lateral and row gap between units in a formation.
pub const FORMATION_GAP: f32 = 0.5;
/// How far a blocked formation slot may be moved to open ground.
pub const SLOT_SEARCH: f32 = 6.0;

/// The same notice is not repeated to a player within this many seconds.
pub const NOTICE_COOLDOWN: f32 = 4.0;
