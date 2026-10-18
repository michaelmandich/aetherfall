//! The AI's picture of the world for one player.
//!
//! A [`Snapshot`] lists the player's own workers, army and buildings (and
//! what is left in each resource node); it is collected from the ECS once
//! per think (or built by hand in tests). An
//! [`AiView`] bundles it with the shared resources. Every decision function
//! reads only an `AiView`, so decisions can be tested without the other sim
//! crates.

use af_core::ext::economy::RevealedPlayers;
use af_core::ext::fog::SeenBy;
use af_core::prelude::*;
use bevy::prelude::*;

/// What a worker is doing, as far as the AI cares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorkerTask {
    /// No orders.
    Idle,
    /// Gathering (or carrying back) `resource`, from `target` when known.
    Gather { resource: ResourceId, target: Option<Entity> },
    /// Constructing or repairing `site`.
    Build { site: Entity },
    /// Anything else (moving, fighting); the AI leaves it alone.
    Busy,
}

impl WorkerTask {
    pub fn resource(&self) -> Option<ResourceId> {
        match self {
            WorkerTask::Gather { resource, .. } => Some(*resource),
            _ => None,
        }
    }

    /// The node, field or foundation being worked.
    pub fn target(&self) -> Option<Entity> {
        match self {
            WorkerTask::Gather { target, .. } => *target,
            WorkerTask::Build { site } => Some(*site),
            _ => None,
        }
    }

    /// Free to take a new job (idle or gathering).
    pub fn available(&self) -> bool {
        matches!(self, WorkerTask::Idle | WorkerTask::Gather { .. })
    }
}

/// One of the player's workers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkerInfo {
    pub entity: Entity,
    pub pos: Vec2,
    pub task: WorkerTask,
}

/// A non-worker unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArmyInfo {
    pub entity: Entity,
    pub kind: UnitTypeId,
    pub pos: Vec2,
    /// Population this model takes.
    pub pop: u32,
    pub idle: bool,
}

/// A resource field (farm) building's occupancy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldInfo {
    pub resource: ResourceId,
    pub slots: u32,
    pub users: u32,
}

/// One of the player's buildings (foundations included).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildingInfo {
    pub entity: Entity,
    pub kind: BuildingTypeId,
    pub pos: Vec2,
    pub facing: f32,
    /// False while a foundation.
    pub complete: bool,
    /// Construction progress, 0..=1 (1 when complete).
    pub progress: f32,
    /// `DropSite::mask` (0 = not a drop site).
    pub drop_mask: u16,
    pub field: Option<FieldInfo>,
    /// Where trained units go (`Rally`).
    pub rally: Option<RallyTarget>,
    queue_start: u32,
    queue_len: u32,
}

impl BuildingInfo {
    pub fn new(entity: Entity, kind: BuildingTypeId, pos: Vec2, facing: f32, complete: bool) -> BuildingInfo {
        BuildingInfo {
            entity,
            kind,
            pos,
            facing,
            complete,
            progress: if complete { 1.0 } else { 0.0 },
            drop_mask: 0,
            field: None,
            rally: None,
            queue_start: 0,
            queue_len: 0,
        }
    }

    /// Accepts deposits of `r` (only when complete).
    pub fn accepts(&self, r: ResourceId) -> bool {
        self.complete && self.drop_mask & (1 << r.0) != 0
    }
}

/// A hostile building the AI has seen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnownBuilding {
    pub entity: Entity,
    pub owner: PlayerId,
    pub kind: BuildingTypeId,
    pub pos: Vec2,
    /// Its yaw, so that its box can be tested against the fog (see
    /// [`KnownBuilding::obb`]).
    pub facing: f32,
}

impl KnownBuilding {
    /// The building's box as last seen.
    pub fn obb(&self, data: &GameData) -> Obb {
        Obb::from_size(self.pos, data.building(self.kind).size, self.facing)
    }
}

/// One player's own entities, sorted by entity for deterministic iteration,
/// and what is left in each resource node.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub workers: Vec<WorkerInfo>,
    pub army: Vec<ArmyInfo>,
    pub buildings: Vec<BuildingInfo>,
    queue_items: Vec<QueueKind>,
    /// Remaining amount per resource node, sorted by entity.
    node_amounts: Vec<(Entity, f32)>,
}

impl Snapshot {
    pub fn clear(&mut self) {
        self.workers.clear();
        self.army.clear();
        self.buildings.clear();
        self.queue_items.clear();
        self.node_amounts.clear();
    }

    /// Record what is left in a resource node.
    pub fn add_node_amount(&mut self, node: Entity, remaining: f32) {
        self.node_amounts.push((node, remaining));
    }

    /// What is left in a resource node (None = not recorded).
    pub fn node_remaining(&self, node: Entity) -> Option<f32> {
        self.node_amounts.binary_search_by_key(&node, |n| n.0).ok().map(|i| self.node_amounts[i].1)
    }

    pub fn add_worker(&mut self, w: WorkerInfo) {
        self.workers.push(w);
    }

    pub fn add_army(&mut self, a: ArmyInfo) {
        self.army.push(a);
    }

    /// Add a building with its production queue (front first).
    pub fn add_building(&mut self, mut b: BuildingInfo, queue: impl IntoIterator<Item = QueueKind>) {
        b.queue_start = self.queue_items.len() as u32;
        self.queue_items.extend(queue);
        b.queue_len = self.queue_items.len() as u32 - b.queue_start;
        self.buildings.push(b);
    }

    /// Sort every list by entity. Call once after adding everything.
    pub fn finish(&mut self) {
        self.workers.sort_unstable_by_key(|w| w.entity);
        self.army.sort_unstable_by_key(|a| a.entity);
        self.buildings.sort_unstable_by_key(|b| b.entity);
        self.node_amounts.sort_unstable_by_key(|n| n.0);
    }

    /// The building's production queue, front first.
    pub fn queue(&self, b: &BuildingInfo) -> &[QueueKind] {
        let start = b.queue_start as usize;
        &self.queue_items[start..start + b.queue_len as usize]
    }

    /// The queue holds research or an upgrade (the building is busy with it).
    pub fn queue_busy(&self, b: &BuildingInfo) -> bool {
        self.queue(b).iter().any(|k| !matches!(k, QueueKind::Unit(_)))
    }

    pub fn building(&self, e: Entity) -> Option<&BuildingInfo> {
        self.buildings.binary_search_by_key(&e, |b| b.entity).ok().map(|i| &self.buildings[i])
    }

    pub fn is_worker(&self, e: Entity) -> bool {
        self.workers.binary_search_by_key(&e, |w| w.entity).is_ok()
    }

    pub fn is_army(&self, e: Entity) -> bool {
        self.army.binary_search_by_key(&e, |a| a.entity).is_ok()
    }

    pub fn army_unit(&self, e: Entity) -> Option<&ArmyInfo> {
        self.army.binary_search_by_key(&e, |a| a.entity).ok().map(|i| &self.army[i])
    }
}

/// Everything a decision function may read for one AI player.
#[derive(Clone, Copy)]
pub struct AiView<'a> {
    pub data: &'a GameData,
    pub players: &'a Players,
    pub me: &'a PlayerState,
    pub nav: &'a NavGrid,
    pub spatial: &'a SpatialIndex,
    /// Map size (x, z) in metres.
    pub map_size: Vec2,
    /// Centre of the Starfall Crater (the contested middle of the map).
    pub crater: Vec2,
    /// Sim seconds since the match started.
    pub time: f32,
    /// The fog of war (for searching where enemies may hide).
    pub fog: &'a FogOfWar,
    /// Players every team sees the buildings of, because they lost every
    /// production building (see `RevealedPlayers`; None without af_economy).
    pub revealed: Option<&'a RevealedPlayers>,
    pub snap: &'a Snapshot,
}

impl<'a> AiView<'a> {
    /// `None` when `player` is not in `players`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data: &'a GameData,
        players: &'a Players,
        player: PlayerId,
        nav: &'a NavGrid,
        spatial: &'a SpatialIndex,
        map: Option<&MapInfo>,
        time: f32,
        fog: &'a FogOfWar,
        snap: &'a Snapshot,
    ) -> Option<AiView<'a>> {
        let me = players.try_get(player)?;
        let map_size = map.map_or(nav.world_size(), |m| m.size);
        let crater = map.map_or(map_size * 0.5, |m| m.crater_center);
        Some(AiView { data, players, me, nav, spatial, map_size, crater, time, fog, revealed: None, snap })
    }

    /// The view with af_economy's revealed players (see [`AiView::revealed`]).
    pub fn with_revealed(self, revealed: Option<&'a RevealedPlayers>) -> AiView<'a> {
        AiView { revealed, ..self }
    }

    /// Has `player` lost every production building, so that its remaining
    /// buildings are revealed to everyone (see `RevealedPlayers`)? Such a
    /// realm is all but beaten: it can no longer train or research anything.
    pub fn is_revealed(&self, player: PlayerId) -> bool {
        self.revealed.is_some_and(|r| r.contains(player))
    }

    pub fn faction(&self) -> &'a FactionDef {
        self.data.faction(self.me.faction)
    }

    /// The faction's worker after line upgrades.
    pub fn worker_kind(&self) -> UnitTypeId {
        let base = self.faction().worker_id;
        let up = self.me.mods.upgraded_unit(base);
        if self.faction().unit_ids.contains(&up) { up } else { base }
    }

    /// The faction profile's aggression, 0 (turtle) ..= 1 (rush).
    pub fn aggression(&self) -> f32 {
        self.faction().ai.aggression.clamp(0.0, 1.0)
    }

    /// Buildings of `kind` owned, foundations included.
    pub fn count_any(&self, kind: BuildingTypeId) -> u32 {
        self.me.buildings_any.get(kind.index()).copied().unwrap_or(0)
    }

    /// Complete buildings of `kind` owned.
    pub fn count_owned(&self, kind: BuildingTypeId) -> u32 {
        self.me.buildings_owned.get(kind.index()).copied().unwrap_or(0)
    }

    /// Mobility mask of the faction's worker (for nav queries).
    pub fn ground_mask(&self) -> u16 {
        self.data.unit_mobility(self.worker_kind(), self.me.faction).0
    }

    /// Clamp a point into the map, `margin` metres from the edges.
    pub fn clamp(&self, p: Vec2, margin: f32) -> Vec2 {
        let m = margin.min(self.map_size.min_element() * 0.5);
        p.clamp(Vec2::splat(m), self.map_size - Vec2::splat(m))
    }
}

// ---------------------------------------------------------------------------
// Collecting a snapshot from the ECS
// ---------------------------------------------------------------------------

/// Every live unit.
pub type UnitQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Unit, &'static Owner, &'static Position, Option<&'static Orders>, Option<&'static Gatherer>, Has<Idle>),
    Without<Dead>,
>;

/// Every live building.
pub type BuildingQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Building,
        &'static Owner,
        &'static Position,
        &'static Facing,
        Option<&'static UnderConstruction>,
        Option<&'static ProductionQueue>,
        Option<&'static Rally>,
        Option<&'static ResourceField>,
        Option<&'static DropSite>,
        Option<&'static SeenBy>,
    ),
    Without<Dead>,
>;

/// What a worker may be gathering from: a node or a resource field.
pub type TargetQuery<'w, 's> = Query<'w, 's, (Option<&'static ResourceNode>, Option<&'static ResourceField>), Without<Dead>>;

/// Every live resource node.
pub type NodeQuery<'w, 's> = Query<'w, 's, (Entity, &'static ResourceNode), Without<Dead>>;

/// Fill `snap` with `player`'s units and buildings and the nodes' remaining amounts.
pub fn collect_snapshot(
    player: PlayerId,
    data: &GameData,
    units: &UnitQuery,
    buildings: &BuildingQuery,
    targets: &TargetQuery,
    nodes: &NodeQuery,
    snap: &mut Snapshot,
) {
    snap.clear();
    for (entity, unit, owner, pos, orders, gatherer, idle) in units {
        if owner.0 != player {
            continue;
        }
        match gatherer {
            Some(g) => snap.add_worker(WorkerInfo { entity, pos: pos.0, task: worker_task(data, orders, g, idle, targets) }),
            None => snap.add_army(ArmyInfo {
                entity,
                kind: unit.kind,
                pos: pos.0,
                pop: data.unit(unit.kind).pop,
                idle: idle || orders.is_none_or(|o| o.queue.is_empty()),
            }),
        }
    }
    for (entity, building, owner, pos, facing, site, queue, rally, field, drop, _) in buildings {
        if owner.0 != player {
            continue;
        }
        let mut b = BuildingInfo::new(entity, building.kind, pos.0, facing.0, site.is_none());
        b.progress = site.map_or(1.0, |s| s.progress);
        b.drop_mask = drop.map_or(0, |d| d.mask);
        b.field = field.map(|f| FieldInfo { resource: f.resource, slots: f.slots, users: f.users });
        b.rally = rally.and_then(|r| r.0);
        snap.add_building(b, queue.into_iter().flat_map(|q| q.items.iter().map(|i| i.kind)));
    }
    for (entity, node) in nodes {
        snap.add_node_amount(entity, node.remaining);
    }
    snap.finish();
}

fn worker_task(data: &GameData, orders: Option<&Orders>, g: &Gatherer, idle: bool, targets: &TargetQuery) -> WorkerTask {
    let resource_of = |e: Entity| -> Option<ResourceId> {
        let (node, field) = targets.get(e).ok()?;
        node.map(|n| n.resource).or(field.map(|f| f.resource))
    };
    // Fallbacks for a target that just died: what is carried, then what was last worked.
    let remembered = || g.carrying.or(g.last_node_kind.map(|k| data.node(k).resource_id));
    match orders.and_then(|o| o.current()) {
        Some(Order::Gather { target }) => match resource_of(*target).or_else(remembered) {
            Some(resource) => WorkerTask::Gather { resource, target: Some(*target) },
            None => WorkerTask::Busy,
        },
        Some(Order::ReturnCargo) => match g.target.and_then(resource_of).or_else(remembered) {
            Some(resource) => WorkerTask::Gather { resource, target: g.target },
            None => WorkerTask::Busy,
        },
        Some(Order::Build { site }) | Some(Order::Repair { target: site }) => WorkerTask::Build { site: *site },
        Some(_) => WorkerTask::Busy,
        None if idle => WorkerTask::Idle,
        // No order but still bound to a node: some behaviour code keeps the
        // gathering loop in `Gatherer` alone.
        None => match g.target.and_then(|t| resource_of(t).map(|r| (t, r))) {
            Some((t, resource)) => WorkerTask::Gather { resource, target: Some(t) },
            None => WorkerTask::Idle,
        },
    }
}
