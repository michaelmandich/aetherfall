//! Schedules and system sets. THE ordering contract between crates.
//!
//! The simulation runs in `FixedUpdate` at [`SIM_HZ`] while
//! `GameState::Playing`. Use [`SIM_DT`] (not wall-clock time) in sim systems.
//! Sim systems never read `Transform`; they read/write the sim components
//! (`Position`, `Facing`, ...). Visual crates turn sim state into
//! transforms/meshes in `Update`.
//!
//! FixedUpdate, chained in this order:
//!
//! | set           | owner crate  | what happens                                                    |
//! |---------------|--------------|-----------------------------------------------------------------|
//! | `Prepare`     | af_core      | tick clock, copy Prev*, drain CommandInbox -> TickCommands, recount players, rebuild SpatialIndex |
//! | `Ai`          | af_ai        | computer players push PlayerCommands into CommandInbox (applied next tick) |
//! | `Commands`    | af_behavior, af_economy | consume TickCommands (orders / placement / training / research) |
//! | `Orders`      | af_behavior  | unit brains: orders -> MoveGoal / AttackTarget, gather, build, repair, heal |
//! | `Economy`     | af_economy   | production queues, research, construction completion, upgrades, trickle, victory |
//! | `Pathing`     | af_movement  | sync Obstacle -> Nav, service path requests                    |
//! | `Movement`    | af_movement  | steering, integration, collisions (no box overlaps)            |
//! | `Combat`      | af_combat    | target acquisition in range, weapon windup/cooldown, firing     |
//! | `Projectiles` | af_combat    | move projectiles/FX, resolve hits -> DamageMsg                  |
//! | `Damage`      | af_combat    | apply DamageMsg (armour, shields), regen, deaths -> Dead + EntityDied |
//! | `Vision`      | af_fog       | fog-of-war visibility                                           |
//! | `Cleanup`     | af_core      | despawn entities marked `Dead` (in entity order)                |
//!
//! Each `FixedMain` tick runs between [`crate::sim_ids::SimTickOpen`] and
//! [`crate::sim_ids::SimTickClose`]: sim spawns in it get ids that client
//! spawns between ticks cannot shift (see [`crate::sim_ids`]).
//!
//! Update, chained: `ClientSet::Pointer` (af_ui decides if the cursor is over
//! UI) -> `Input` (af_input: camera, selection, commands) -> `Ui` (af_ui HUD)
//! -> `Render` (af_render: sim state -> transforms/meshes).

use bevy::prelude::*;

/// Simulation ticks per second.
pub const SIM_HZ: f64 = 30.0;
/// Seconds per simulation tick. Use this in sim systems.
pub const SIM_DT: f32 = (1.0 / SIM_HZ) as f32;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    Prepare,
    Ai,
    Commands,
    Orders,
    Economy,
    Pathing,
    Movement,
    Combat,
    Projectiles,
    Damage,
    Vision,
    Cleanup,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientSet {
    Pointer,
    Input,
    Ui,
    Render,
}

/// Top-level app flow. Headless runs start directly in `Playing`.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    /// Skirmish setup screen (af_ui).
    #[default]
    Menu,
    /// A match is running (world built on enter by af_map).
    Playing,
}
