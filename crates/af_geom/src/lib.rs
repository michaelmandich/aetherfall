//! Ground-plane geometry and navigation for Aetherfall.
//!
//! * [`obb`] — oriented boxes (every footprint in the game), SAT overlap and
//!   push-out, edge-to-edge gaps, segment distance, 3D ray picking, angle
//!   helpers; a [`Frame`] is a box with its axes worked out, for hot loops.
//!   Convention: `Vec2 = (world x, world z)`, yaw 0 faces +Z.
//! * [`grid`] — the navigation grid (terrain, obstacles with a walk margin,
//!   gates).
//! * [`path`] — A* with clearance, terrain costs and smoothing on the grid;
//!   constant-time clearance tables live in `clearance`.

mod clearance;
pub mod grid;
pub mod obb;
pub mod path;

pub use grid::{NavGrid, FLYING, NO_TEAM};
pub use obb::*;
pub use path::{PathRequest, PathResult, PathStep, AVOID_COST};
