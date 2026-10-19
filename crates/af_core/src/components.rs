//! Components shared between crates. The comment on each says which crate
//! WRITES it; anyone may read.
//!
//! Ground-plane convention: `Vec2 = (world x, world z)`; yaw 0 faces +Z;
//! `af_geom::forward(yaw)` / `right(yaw)`.

use std::collections::VecDeque;

use af_data::*;
use af_geom::Obb;
use bevy::prelude::*;

use crate::stats::TagSet;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// A player slot. Index into `Players::list`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId(pub u8);

impl PlayerId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Who owns this entity. Resource nodes have no Owner. Written at spawn.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Owner(pub PlayerId);

/// A unit of type `kind`. Written at spawn (af_core::spawn); af_economy may
/// change `kind` on UpgradeUnit (and must then refresh Stats/Weapons).
#[derive(Component, Clone, Copy, Debug)]
pub struct Unit {
    pub kind: UnitTypeId,
}

/// A building of type `kind`. af_economy changes `kind` on a building upgrade.
#[derive(Component, Clone, Copy, Debug)]
pub struct Building {
    pub kind: BuildingTypeId,
}

/// A harvestable map object (tree, ore vein, crystal...).
/// `remaining` is decremented by af_behavior when gathered.
#[derive(Component, Clone, Copy, Debug)]
pub struct ResourceNode {
    pub kind: NodeTypeId,
    pub resource: ResourceId,
    pub remaining: f32,
    /// Building placed on top (for `requires_building` nodes), set by af_economy.
    pub claimed_by: Option<Entity>,
}

// ---------------------------------------------------------------------------
// Spatial (sim truth; never derive these from Transform)
// ---------------------------------------------------------------------------

/// Ground-plane position of the box centre. Written by af_movement (units)
/// and at spawn.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Position(pub Vec2);

/// Position at the start of the current tick (af_core Prepare). Render
/// interpolates `PrevPosition -> Position` with `Time<Fixed>::overstep_fraction()`.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct PrevPosition(pub Vec2);

/// Yaw in radians. Written by af_movement (units) and at spawn.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Facing(pub f32);

/// Facing at the start of the current tick (af_core Prepare).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct PrevFacing(pub f32);

/// Height of the box's base above the ground (flyers, hover). Visual and
/// for projectile aiming; the sim is 2D. Written at spawn / af_movement.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Altitude(pub f32);

/// Box footprint: half extents on the ground (x along right, y along
/// forward) and full height. Written at spawn.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub half: Vec2,
    pub height: f32,
}

impl Footprint {
    pub fn from_size(size: BoxSize) -> Footprint {
        Footprint { half: Vec2::new(size[0] * 0.5, size[2] * 0.5), height: size[1] }
    }
    pub fn obb(&self, pos: Vec2, yaw: f32) -> Obb {
        Obb::new(pos, self.half, yaw)
    }
}

/// Actual ground velocity over the last tick (m/s). Written by af_movement.
/// Used for projectile leading and animation.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Velocity(pub Vec2);

/// A static obstacle registered in the nav grid with exactly this `obb`.
/// Added at spawn for buildings and blocking nodes. af_movement keeps
/// `Nav` in sync (add on Added<Obstacle>, remove with the same obb when the
/// entity is despawned or the component removed). Gates (`gate_team` set)
/// are registered with `NavGrid::set_gate` instead of `add_obstacle`.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    pub obb: Obb,
    pub gate_team: Option<u8>,
}

/// Movement capability. Written at spawn; speed lives in `Stats::speed`.
#[derive(Component, Clone, Copy, Debug)]
pub struct Mobile {
    pub mobility: Mobility,
    /// Lateral half-width used for path clearance (metres).
    pub radius: f32,
    /// Radians per second.
    pub turn_rate: f32,
    pub flying: bool,
}

// ---------------------------------------------------------------------------
// Health and stats
// ---------------------------------------------------------------------------

/// Hit points. `max` mirrors `Stats::max_hp`. Written by af_combat (damage),
/// af_behavior (repair/heal), af_economy (construction progress, upgrades).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Health {
    pub hp: f32,
    pub max: f32,
}

impl Health {
    pub fn full(max: f32) -> Health {
        Health { hp: max, max }
    }
    pub fn fraction(&self) -> f32 {
        if self.max > 0.0 { (self.hp / self.max).clamp(0.0, 1.0) } else { 0.0 }
    }
}

/// Regenerating energy shield absorbing damage before hp. Written by af_combat.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Shield {
    pub hp: f32,
    pub max: f32,
    pub regen: f32,
    pub delay: f32,
    pub energy_resist: f32,
    /// Seconds since last damage.
    pub since_hit: f32,
}

/// Effective stats after the owner's techs. Written at spawn and by
/// af_economy when a tech changes them (see `stats::refresh_*`).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Stats {
    pub max_hp: f32,
    pub armor: Armor,
    /// m/s (0 for buildings).
    pub speed: f32,
    pub vision: f32,
    /// hp per second.
    pub regen: f32,
    /// Multipliers/adders applied to every weapon of this entity.
    pub damage_mult: f32,
    pub range_add: f32,
    pub cooldown_mult: f32,
    pub shield_max: f32,
    pub tags: TagSet,
}

/// Marks an entity that died this tick. af_combat (or whoever kills it)
/// inserts it and writes `EntityDied`; af_core despawns it in `Cleanup`.
/// Systems should skip entities `With<Dead>`.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Dead;

/// Last thing that damaged this entity. Written by af_combat.
#[derive(Component, Clone, Copy, Debug)]
pub struct LastAttacker {
    pub entity: Option<Entity>,
    pub player: Option<PlayerId>,
    pub pos: Vec2,
    /// SimClock::elapsed at the time.
    pub time: f32,
}

// ---------------------------------------------------------------------------
// Weapons
// ---------------------------------------------------------------------------

/// One mounted weapon. Written by af_combat; read by af_render for animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponSlot {
    pub weapon: WeaponId,
    /// Seconds until the weapon may start another attack.
    pub cooldown: f32,
    /// Seconds left in the current windup (0 = not attacking).
    pub windup: f32,
    /// Target of the current attack.
    pub target: Option<Entity>,
    /// Ground point being aimed at (for turrets and animation).
    pub aim: Vec2,
    /// Turret/aim yaw relative to the body (radians).
    pub aim_yaw: f32,
    /// 0..1 progress of the current attack animation (swing, recoil); 0 idle.
    pub anim: f32,
}

impl WeaponSlot {
    pub fn new(weapon: WeaponId) -> WeaponSlot {
        WeaponSlot { weapon, cooldown: 0.0, windup: 0.0, target: None, aim: Vec2::ZERO, aim_yaw: 0.0, anim: 0.0 }
    }
}

#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Weapons {
    pub slots: Vec<WeaponSlot>,
}

/// What this entity should attack. Written by af_behavior (chasing, orders)
/// and af_combat (auto-acquire for idle/static shooters). af_combat fires
/// at it when in range; removes it when the target is gone.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct AttackTarget {
    pub entity: Entity,
    /// Ordered explicitly by the player (attack even allies/neutrals, never
    /// retarget automatically).
    pub forced: bool,
}

// ---------------------------------------------------------------------------
// Orders and movement intents
// ---------------------------------------------------------------------------

/// High-level orders. Front = current. Written by af_behavior (from
/// commands) and af_economy (builders sent to a placed foundation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    Move { target: Vec2 },
    /// Move, engaging enemies met on the way.
    AttackMove { target: Vec2 },
    Attack { target: Entity },
    /// Gather from a node or a resource-field building (farm).
    Gather { target: Entity },
    /// Carry what is held to the nearest drop site, then resume gathering.
    ReturnCargo,
    /// Construct (or help construct) a foundation.
    Build { site: Entity },
    Repair { target: Entity },
    Patrol { a: Vec2, b: Vec2 },
    Follow { target: Entity },
    /// Stay put; shoot what comes in range; never chase.
    Hold,
}

#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Orders {
    pub queue: VecDeque<Order>,
}

impl Orders {
    pub fn current(&self) -> Option<&Order> {
        self.queue.front()
    }
    /// Replace everything with one order (or append when `queue`).
    pub fn set(&mut self, order: Order, queue: bool) {
        if !queue {
            self.queue.clear();
        }
        self.queue.push_back(order);
    }
}

/// How an idle unit reacts to enemies. Written by af_behavior.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Stance {
    /// Chase enemies that come into vision.
    #[default]
    Aggressive,
    /// Engage enemies in vision but return to post after a short chase.
    Defensive,
    /// Shoot what is in range; never move.
    StandGround,
    /// Never auto-attack (workers default to this).
    Passive,
}

/// Where the unit is trying to go. Written by af_behavior (and af_economy
/// for rally points of fresh units); consumed by af_movement. Bump `seq` (use
/// `MoveGoal::new`, which takes a fresh sequence) to force a repath.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct MoveGoal {
    pub target: Vec2,
    /// Arrive when the unit's centre is within this distance of `target`
    /// (or, with `follow`, when its box is within `arrive` of that entity's box).
    pub arrive: f32,
    /// Track a (moving) entity instead of a point.
    pub follow: Option<Entity>,
    pub seq: u32,
}

impl MoveGoal {
    pub fn point(target: Vec2, arrive: f32) -> MoveGoal {
        MoveGoal { target, arrive, follow: None, seq: crate::next_seq() }
    }
    pub fn entity(e: Entity, target: Vec2, arrive: f32) -> MoveGoal {
        MoveGoal { target, arrive, follow: Some(e), seq: crate::next_seq() }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PathStatus {
    /// No goal.
    #[default]
    Idle,
    /// Waiting for the path service.
    Pending,
    Following,
    /// Within `arrive` of the goal.
    Arrived,
    /// No route, stuck for too long, or the end of a partial path was
    /// reached without getting within `arrive`. af_behavior decides what to do.
    Failed,
}

/// Path-following state. Written only by af_movement; af_behavior reads
/// `status` (and may remove `MoveGoal` to stop).
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct PathState {
    pub waypoints: Vec<Vec2>,
    pub next: usize,
    pub status: PathStatus,
    /// `MoveGoal::seq` this path was computed for.
    pub goal_seq: u32,
    /// The path ends short of the goal (goal unreachable).
    pub partial: bool,
    pub stuck_time: f32,
}

/// Units trained together as a squadron share an id; selecting one selects
/// the squad. Written at spawn by af_economy.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SquadMember {
    pub squad: u32,
}

/// Present on units with nothing to do (no orders, no task). Maintained by
/// af_behavior; used by the UI idle-worker button and the AI.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Idle;

/// What a unit is busy with this tick, for animation (af_render) and
/// anyone else who asks. Written only by af_behavior, at the end of
/// `SimSet::Orders`; every unit spawns with the default `Idle`.
///
/// It names the task, not the movement: walking is read from `Velocity`
/// and the load from `Gatherer`. So a worker walking to its tree or
/// carrying wood home is `Idle` until it stands at the tree, chopping.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Activity {
    /// Nothing below (standing, or walking somewhere).
    #[default]
    Idle,
    /// Harvesting at a node, in the style of its resource
    /// (`ResourceDef::work`), or working a resource-field building
    /// (always `WorkStyle::Farm`).
    Gather(WorkStyle),
    /// Constructing a foundation, at it.
    Build,
    /// Repairing a building or machine, at it.
    Repair,
    /// Mending an ally in reach.
    Heal,
    /// Engaged with a hostile: chasing it, or attacking (the windup is in
    /// `Weapons`).
    Fight,
}

/// A worker's carried load and gathering memory. Written by af_behavior.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Gatherer {
    pub carrying: Option<ResourceId>,
    pub amount: f32,
    /// Effective capacity (def + techs); refreshed with Stats.
    pub capacity: f32,
    /// Node/field currently being worked.
    pub target: Option<Entity>,
    /// Node type last gathered, to find a similar node when one runs out.
    pub last_node_kind: Option<NodeTypeId>,
    pub last_pos: Vec2,
}

// ---------------------------------------------------------------------------
// Buildings
// ---------------------------------------------------------------------------

/// A foundation being built. While present the building does nothing
/// (no production, weapons, drop site, pop). Builders (af_behavior) add
/// `progress` (0..1) and grow hp; af_economy completes it at >= 1.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct UnderConstruction {
    pub progress: f32,
    /// Seconds for one builder (after techs).
    pub build_time: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueKind {
    Unit(UnitTypeId),
    Tech(TechId),
    /// Upgrade this building into the given type.
    Upgrade(BuildingTypeId),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QueueItem {
    pub kind: QueueKind,
    /// What was paid (refunded on cancel).
    pub paid: ResAmounts,
    /// Total seconds needed (after techs).
    pub time: f32,
}

/// Production/research/upgrade queue. Written by af_economy.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct ProductionQueue {
    pub items: VecDeque<QueueItem>,
    /// Seconds spent on the front item.
    pub progress: f32,
    /// Front item is waiting for population room.
    pub pop_blocked: bool,
}

impl ProductionQueue {
    pub const MAX: usize = 10;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RallyTarget {
    Point(Vec2),
    Entity(Entity),
}

/// Where freshly trained units go. Written by af_economy (SetRally).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Rally(pub Option<RallyTarget>);

/// Accepts deposits of these resources (bit per ResourceId). Inactive while
/// UnderConstruction.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct DropSite {
    pub mask: u16,
}

impl DropSite {
    pub fn accepts(&self, r: ResourceId) -> bool {
        self.mask & (1 << r.0) != 0
    }
}

/// A building gathered like a node (farm). af_behavior decrements
/// `remaining` and tracks `users`; when it hits 0 the building is killed.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ResourceField {
    pub resource: ResourceId,
    pub remaining: f32,
    pub slots: u32,
    pub users: u32,
}

/// Wall segment marker (gates also carry `Gate`).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Wall;

/// Gate marker: owner's team passes.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Gate;

/// Passive income per second while complete. Written at spawn/upgrade.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Trickle(pub ResAmounts);

/// The node this building sits on (aether extractor on a vent).
#[derive(Component, Clone, Copy, Debug)]
pub struct ClaimsNode(pub Entity);

// ---------------------------------------------------------------------------
// Visual effects (af_combat writes, af_render draws)
// ---------------------------------------------------------------------------

/// "Draw a box here": projectiles, beams, sparks, explosions, debris. The
/// writer (af_combat) moves/fades/despawns these; af_render only draws them.
/// `dir` is the box's forward (+Z) axis in world space; `size` is the full
/// box size along (right, up, forward).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FxBox {
    pub pos: Vec3,
    pub prev_pos: Vec3,
    pub dir: Vec3,
    pub size: Vec3,
    pub color: [f32; 3],
    pub glow: bool,
    /// 0..1 opacity.
    pub alpha: f32,
}

// ---------------------------------------------------------------------------
// Client-side markers (af_input / af_ui / af_render)
// ---------------------------------------------------------------------------

/// Selected by the local player. Written by af_input (and af_ui buttons).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Selected;

/// A short-lived click marker on the ground (move/attack feedback).
/// af_input spawns it; af_render draws it, counts `time_left` down (real
/// time) and despawns it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ClickPing {
    pub pos: Vec2,
    pub time_left: f32,
    pub color: [f32; 3],
}
