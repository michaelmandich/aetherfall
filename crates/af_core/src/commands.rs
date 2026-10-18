//! Player commands: the only way players (human or AI) affect the sim.
//! Push into `CommandInbox`; they execute next tick from `TickCommands`.
//!
//! Who handles what (in `SimSet::Commands`):
//! * af_behavior: Smart, Move, AttackMove, Attack, Patrol, Stop, Hold,
//!   Gather, Build, Repair, Follow, SetStance, Delete.
//! * af_economy: PlaceBuilding, PlaceWall, Train, Research, UpgradeBuilding,
//!   CancelQueue, SetRally.
//!
//! Handlers must ignore entities not owned by `player` (except as targets)
//! and silently skip dead/missing entities.

use af_data::*;
use bevy::prelude::*;

use crate::components::{PlayerId, Stance};

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerCommand {
    pub player: PlayerId,
    /// The units (or buildings, for building commands) the command applies to.
    pub entities: Vec<Entity>,
    pub kind: CommandKind,
    /// Shift-queue after current orders instead of replacing them.
    pub queue: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommandKind {
    /// Right-click: af_behavior resolves by context (enemy -> attack,
    /// node/field -> gather, own foundation -> build, own damaged building ->
    /// repair, ground -> move). For buildings among `entities`, af_economy
    /// sets the rally point.
    Smart { point: Vec2, target: Option<Entity> },
    Move(Vec2),
    AttackMove(Vec2),
    Attack(Entity),
    Patrol(Vec2),
    Follow(Entity),
    Stop,
    Hold,
    Gather(Entity),
    /// Help build a foundation.
    Build(Entity),
    Repair(Entity),
    SetStance(Stance),
    /// Kill the listed own entities.
    Delete,
    /// Place a foundation; `entities` are the builders sent to it.
    PlaceBuilding { kind: BuildingTypeId, pos: Vec2, facing: f32 },
    /// Place a line of wall segments from `from` to `to`; builders = `entities`.
    PlaceWall { kind: BuildingTypeId, from: Vec2, to: Vec2 },
    /// Queue `count` trainings of `unit` at each building in `entities`
    /// (distributed: each queues at the least busy).
    Train { unit: UnitTypeId, count: u32 },
    Research { tech: TechId },
    /// Upgrade each building in `entities` along its `upgrade` chain.
    UpgradeBuilding,
    /// Cancel queue slot `index` of the first building in `entities`.
    CancelQueue { index: usize },
    SetRally { point: Vec2, target: Option<Entity> },
}

impl PlayerCommand {
    pub fn new(player: PlayerId, entities: Vec<Entity>, kind: CommandKind) -> PlayerCommand {
        PlayerCommand { player, entities, kind, queue: false }
    }
    pub fn queued(mut self, q: bool) -> PlayerCommand {
        self.queue = q;
        self
    }
}
