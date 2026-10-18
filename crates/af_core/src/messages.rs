//! Buffered messages (Bevy 0.19 `Message`). Register in CorePlugin.

use af_data::*;
use bevy::prelude::*;

use crate::components::PlayerId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    Unit(UnitTypeId),
    Building(BuildingTypeId),
    Node(NodeTypeId),
}

/// Something died (hp <= 0, depleted, deleted). Written by whoever inserts
/// `Dead` (af_combat for damage; af_behavior for depleted nodes/fields;
/// af_behavior for Delete). The entity still exists until Cleanup.
#[derive(Message, Clone, Copy, Debug)]
pub struct EntityDied {
    pub entity: Entity,
    pub owner: Option<PlayerId>,
    pub kind: EntityKind,
    pub pos: Vec2,
    pub killer: Option<PlayerId>,
}

/// One instance of damage to apply. Written by af_combat (weapons) or
/// anyone else; applied by af_combat in SimSet::Damage (armour, shields).
#[derive(Message, Clone, Copy, Debug)]
pub struct DamageMsg {
    pub target: Entity,
    /// Before armour (bonus multipliers already applied).
    pub amount: f32,
    pub dtype: DamageType,
    pub source: Option<Entity>,
    pub source_player: Option<PlayerId>,
}

/// A unit finished training / was spawned by production. Written by af_economy.
#[derive(Message, Clone, Copy, Debug)]
pub struct UnitSpawned {
    pub entity: Entity,
    pub owner: PlayerId,
    pub kind: UnitTypeId,
}

/// A foundation was placed. Written by af_economy.
#[derive(Message, Clone, Copy, Debug)]
pub struct BuildingPlaced {
    pub entity: Entity,
    pub owner: PlayerId,
    pub kind: BuildingTypeId,
}

/// Construction (or an upgrade) finished. Written by af_economy.
#[derive(Message, Clone, Copy, Debug)]
pub struct BuildingCompleted {
    pub entity: Entity,
    pub owner: PlayerId,
    pub kind: BuildingTypeId,
}

/// Written by af_economy.
#[derive(Message, Clone, Copy, Debug)]
pub struct TechResearched {
    pub player: PlayerId,
    pub tech: TechId,
}

/// Written by af_economy.
#[derive(Message, Clone, Copy, Debug)]
pub struct AgeAdvanced {
    pub player: PlayerId,
    pub age: u8,
}

/// Throttled "under attack" signal for alerts and AI. Written by af_combat
/// at most once per few seconds per victim area.
#[derive(Message, Clone, Copy, Debug)]
pub struct UnderAttack {
    pub victim: Entity,
    pub victim_owner: PlayerId,
    pub attacker: Option<Entity>,
    pub attacker_owner: Option<PlayerId>,
    /// The victim's position.
    pub pos: Vec2,
}
