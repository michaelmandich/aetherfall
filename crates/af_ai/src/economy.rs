//! Economy decisions: worker count, where workers gather, drop sites,
//! fields and houses.

use af_core::ext::movement::RouteField;
use af_core::prelude::*;
use bevy::prelude::*;

use crate::memory::{MAX_REMOTE_CLUSTERS, PendingBuild};
use crate::plan::Plan;
use crate::progress::{cost_total, next_production_building};
use crate::view::AiView;

/// Workers to keep, by age (1..=5). Halls keep training up to the target
/// while saving for the age-up, so the economy never stands still.
pub const WORKER_TARGETS: [u32; 5] = [28, 46, 62, 72, 80];
/// Nodes within this distance of the main hall count as "home" sources.
pub const HOME_RADIUS: f32 = 50.0;
/// A worked cluster farther than this from the edge of every drop site
/// gets its own; nodes closer than this are "served". Short, because a
/// worker carries little and walks every load back.
pub const DROP_SITE_DISTANCE: f32 = 12.0;
/// Nodes within this distance of each other form one cluster.
pub const CLUSTER_RADIUS: f32 = 10.0;
/// A remote cluster is the nodes within this distance of the node found.
pub const REMOTE_CLUSTER_RADIUS: f32 = CLUSTER_RADIUS * 1.5;
/// Remote clusters are added until they hold this many nodes.
pub const REMOTE_NODES_WANTED: u32 = 8;
/// A cluster must hold at least this much to deserve a drop site.
pub const MIN_CLUSTER_AMOUNT: f32 = 300.0;
/// Clusters examined per resource when planning a drop site, nearest home first.
const MAX_CLUSTER_SEEDS: usize = 32;
/// Workers one gatherable node comfortably takes (for farm planning).
pub const WORKERS_PER_NODE: u32 = 2;
/// A node counts as in a worker's reach when its ground comes this close to
/// it (metres; nodes stand on open ground, so this only matters for odd ones).
const NODE_REACH: f32 = 2.0;

/// Base demand of the first `eco_focus` resource; later ones get less.
const FOCUS_BASE: f32 = 300.0;
const FOCUS_STEP: f32 = 0.12;
/// Focus demand halves once this much of a resource is stockpiled.
const STOCKPILE_SOFT_CAP: f32 = 400.0;
/// Army population the economy always plans to pay for next.
pub const ARMY_PLAN_MIN_POP: u32 = 8;
/// The army is planned to spend up to this many times what it spent lately
/// (room to grow), within what its production can turn out.
pub const ARMY_PLAN_GROWTH: f32 = 1.5;
/// Gathering plans for the spending of this many seconds ahead.
pub const PLAN_HORIZON: f32 = 120.0;
/// The age-up path gets gatherers to collect what it lacks within this
/// many seconds ...
pub const AGE_UP_HORIZON: f32 = 150.0;

/// ... from at most this share of the workers.
pub const AGE_UP_WORKER_SHARE: f32 = 0.5;
/// Share of the nominal gather rate a gatherer achieves near home, walking
/// to the drop site included ...
pub const GATHER_EFFICIENCY: f32 = 0.6;
/// ... and at a remote cluster (longer walks, crowded, contested).
pub const REMOTE_EFFICIENCY: f32 = 0.35;

/// Workers to keep in `age` (see [`WORKER_TARGETS`]).
pub fn worker_target(age: u8) -> u32 {
    WORKER_TARGETS[(age.clamp(1, MAX_AGE) - 1) as usize]
}

// ---------------------------------------------------------------------------
// Housing
// ---------------------------------------------------------------------------

/// The cheapest (per pop) building that only provides housing.
pub fn house_kind(data: &GameData, me: &PlayerState) -> Option<BuildingTypeId> {
    let per_pop = |b: BuildingTypeId| cost_total(data, me, b) / data.building(b).pop.max(1) as f32;
    rules::build_menu(data, me)
        .into_iter()
        .filter(|b| {
            let d = data.building(*b);
            d.pop > 0
                && !d.has_tag(Tag::TownCenter)
                && d.requires_node_id.is_none()
                && data.trainable_at(*b, me.faction).is_empty()
                && rules::can_build(data, me, *b, false).is_ok()
        })
        .min_by(|a, b| per_pop(*a).total_cmp(&per_pop(*b)))
}

/// Population room the AI keeps free: 6 plus 2 per production building.
pub fn wanted_room(production: u32) -> u32 {
    6 + 2 * production
}

/// Build housing now? `queued_pop` = pop of units in production queues;
/// `pending_pop` = housing in foundations and pending placements.
pub fn needs_house(me: &PlayerState, queued_pop: u32, pending_pop: u32, production: u32) -> bool {
    let cap = me.pop_cap + pending_pop;
    if cap >= POP_HARD_CAP {
        return false;
    }
    cap.saturating_sub(me.pop_used + queued_pop) < wanted_room(production)
}

// ---------------------------------------------------------------------------
// Drop sites and fields
// ---------------------------------------------------------------------------

/// The cheapest buildable drop site accepting `r` (no town centres, no
/// buildings that must sit on a node).
pub fn drop_site_kind(data: &GameData, me: &PlayerState, r: ResourceId) -> Option<BuildingTypeId> {
    rules::build_menu(data, me)
        .into_iter()
        .filter(|b| {
            let d = data.building(*b);
            d.drop_site_ids.contains(&r)
                && !d.has_tag(Tag::TownCenter)
                && d.requires_node_id.is_none()
                && rules::can_build(data, me, *b, false).is_ok()
        })
        .min_by(|a, b| cost_total(data, me, *a).total_cmp(&cost_total(data, me, *b)))
}

/// The cheapest (per worker slot) buildable resource field producing `r`.
pub fn field_kind(data: &GameData, me: &PlayerState, r: ResourceId) -> Option<BuildingTypeId> {
    let per_slot =
        |b: BuildingTypeId| cost_total(data, me, b) / data.building(b).resource_field.as_ref().map_or(1, |f| f.slots.max(1)) as f32;
    rules::build_menu(data, me)
        .into_iter()
        .filter(|b| {
            let d = data.building(*b);
            d.resource_field.as_ref().is_some_and(|f| f.resource_id == r && f.slots > 0)
                && d.requires_node_id.is_none()
                && rules::can_build(data, me, *b, false).is_ok()
        })
        .min_by(|a, b| per_slot(*a).total_cmp(&per_slot(*b)))
}

/// Fields packed round one drop site before another drop site becomes the next hub.
pub const FIELDS_PER_HUB: u32 = 8;
/// Fields within this distance of a drop site's edge belong to its ring.
pub const FIELD_RING: f32 = 14.0;

/// Where the next field of a resource goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldSpot {
    /// In the ring round the drop site at `pos` (footprint radius `radius`).
    Around { pos: Vec2, radius: f32 },
    /// Every drop site is ringed with fields: build this drop site first.
    NewHub(BuildingTypeId),
}

/// Fields go next to a drop site, where gatherers walk least: the drop site
/// for `r` (built, foundation or pending) with the fewest fields in its
/// ring, until each holds [`FIELDS_PER_HUB`]; then a new drop site (when
/// one can be built) becomes the next hub. `None` without any drop site.
pub fn field_spot(view: &AiView, r: ResourceId, pending: &[PendingBuild]) -> Option<FieldSpot> {
    let data = view.data;
    let radius = |k: BuildingTypeId| {
        let s = data.building(k).size;
        Vec2::new(s[0], s[2]).length() * 0.5
    };
    let is_field = |k: BuildingTypeId| data.building(k).resource_field.as_ref().is_some_and(|f| f.resource_id == r);
    let is_hub = |k: BuildingTypeId| data.building(k).drop_site_ids.contains(&r);
    let all = || view.snap.buildings.iter().map(|b| (b.kind, b.pos)).chain(pending.iter().map(|p| (p.kind, p.pos)));
    let fields: Vec<Vec2> = all().filter(|(k, _)| is_field(*k)).map(|(_, p)| p).collect();
    let (hub, crowd) = all()
        .filter(|(k, _)| is_hub(*k))
        .map(|(k, p)| {
            let reach = radius(k) + FIELD_RING;
            ((p, radius(k)), fields.iter().filter(|f| f.distance(p) <= reach).count() as u32)
        })
        .min_by_key(|(_, crowd)| *crowd)?;
    if crowd >= FIELDS_PER_HUB
        && let Some(kind) = drop_site_kind(data, view.me, r)
    {
        return Some(FieldSpot::NewHub(kind));
    }
    Some(FieldSpot::Around { pos: hub.0, radius: hub.1 })
}

/// Distance from `pos` to the edge of the nearest own drop site for `r`,
/// counting foundations and pending placements (planned sites count as present).
pub fn drop_distance(view: &AiView, pending: &[PendingBuild], r: ResourceId, pos: Vec2) -> f32 {
    let data = view.data;
    let accepts = |kind: BuildingTypeId| data.building(kind).drop_site_ids.contains(&r);
    // Edge distance, with the footprint as a circle of its smaller half-side.
    let half = |kind: BuildingTypeId| data.building(kind).size[0].min(data.building(kind).size[2]) * 0.5;
    let built = view.snap.buildings.iter().filter(|b| accepts(b.kind)).map(|b| (b.kind, b.pos));
    let planned = pending.iter().filter(|p| accepts(p.kind)).map(|p| (p.kind, p.pos));
    built.chain(planned).map(|(k, p)| (p.distance(pos) - half(k)).max(0.0)).fold(f32::INFINITY, f32::min)
}

/// A gatherable node a worker can be sent to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceNode {
    pub entity: Entity,
    pub kind: NodeTypeId,
    pub pos: Vec2,
    pub resource: ResourceId,
    /// What is left in it.
    pub remaining: f32,
    /// Within [`HOME_RADIUS`] of the main hall (else part of a remote cluster).
    pub home: bool,
}

/// Gatherable nodes near home, plus one remote cluster for each resource
/// with none near home.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    pub nodes: Vec<SourceNode>,
}

impl Sources {
    /// Gatherable nodes within [`HOME_RADIUS`] of `home`, except those `skip` rejects.
    pub fn home(view: &AiView, home: Vec2, skip: impl Fn(Entity) -> bool) -> Sources {
        let mut sources = Sources::default();
        sources.push_near(view, home, HOME_RADIUS, None, &skip);
        sources
    }

    /// Add the cluster of `r` nodes around a remote node at `pos` (see [`find_remote`]).
    pub fn add_remote(&mut self, view: &AiView, r: ResourceId, pos: Vec2, skip: impl Fn(Entity) -> bool) {
        self.push_near(view, pos, REMOTE_CLUSTER_RADIUS, Some(r), &skip);
    }

    /// Add gatherable nodes (of `only`, if given) within `radius` of `center`
    /// that are not listed yet and that a worker at `center` can get to over
    /// the terrain: not the trees across water no ford crosses. Those over a
    /// forded river are listed too, and a worker sent to one walks round by
    /// the ford (rivers keep out of the start areas, so few are near home).
    fn push_near(&mut self, view: &AiView, center: Vec2, radius: f32, only: Option<ResourceId>, skip: &impl Fn(Entity) -> bool) {
        let home = only.is_none();
        let listed = self.nodes.len();
        let reach = view.reach_from(view.worker_kind(), center);
        view.spatial.for_each_near(center, radius, |e| {
            if e.kind != SpatialKind::Node || e.pos.distance(center) > radius || skip(e.entity) || !reach.reaches(e.pos, NODE_REACH) {
                return;
            }
            if self.nodes[..listed].iter().any(|n| n.entity == e.entity) {
                return;
            }
            let kind = NodeTypeId(e.type_index);
            let def = view.data.node(kind);
            if def.requires_building_id.is_none() && only.is_none_or(|r| r == def.resource_id) {
                let remaining = view.snap.node_remaining(e.entity).unwrap_or(def.amount);
                self.nodes.push(SourceNode { entity: e.entity, kind, pos: e.pos, resource: def.resource_id, remaining, home });
            }
        });
    }

    pub fn of(&self, r: ResourceId) -> impl Iterator<Item = &SourceNode> + '_ {
        self.nodes.iter().filter(move |n| n.resource == r)
    }

    pub fn has(&self, r: ResourceId) -> bool {
        self.of(r).next().is_some()
    }

    pub fn home_count(&self, r: ResourceId) -> u32 {
        self.of(r).filter(|n| n.home).count() as u32
    }

    pub fn remote_count(&self, r: ResourceId) -> u32 {
        self.of(r).filter(|n| !n.home).count() as u32
    }
}

/// The nearest gatherable node of `r` beyond [`HOME_RADIUS`] of `from` and
/// outside the clusters around `known`, among those the workers can get to:
/// nearest by their way round as `field` measures it (route distances from
/// `from`, see `AiView::nearest_by_route`), else in a straight line.
pub fn find_remote(view: &AiView, from: Vec2, r: ResourceId, known: &[Vec2], field: Option<&RouteField>) -> Option<Vec2> {
    let clear = |p: Vec2| known.iter().all(|k| k.distance(p) > REMOTE_CLUSTER_RADIUS + CLUSTER_RADIUS);
    view.nearest_by_route(view.worker_kind(), from, field, |e| {
        e.kind == SpatialKind::Node && e.pos.distance(from) > HOME_RADIUS && clear(e.pos) && {
            let def = view.data.node(NodeTypeId(e.type_index));
            def.resource_id == r && def.requires_building_id.is_none()
        }
    })
    .map(|e| e.pos)
}

/// The remote clusters of `r` nearest `from` (see [`find_remote`]): more
/// of them, up to [`MAX_REMOTE_CLUSTERS`], until they hold
/// [`REMOTE_NODES_WANTED`] nodes between them.
pub fn search_remote(view: &AiView, from: Vec2, r: ResourceId, field: Option<&RouteField>) -> [Option<Vec2>; MAX_REMOTE_CLUSTERS] {
    let mut found = [None; MAX_REMOTE_CLUSTERS];
    let mut known = Vec::with_capacity(MAX_REMOTE_CLUSTERS);
    let mut nodes = 0;
    for slot in found.iter_mut() {
        if nodes >= REMOTE_NODES_WANTED {
            break;
        }
        let Some(pos) = find_remote(view, from, r, &known, field) else { break };
        view.spatial.for_each_near(pos, REMOTE_CLUSTER_RADIUS, |e| {
            let of_r = e.kind == SpatialKind::Node && view.data.node(NodeTypeId(e.type_index)).resource_id == r;
            if of_r && e.pos.distance(pos) <= REMOTE_CLUSTER_RADIUS {
                nodes += 1;
            }
        });
        known.push(pos);
        *slot = Some(pos);
    }
    found
}

/// Where to put the next drop site.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropPlan {
    pub kind: BuildingTypeId,
    /// Centre of the cluster it serves.
    pub anchor: Vec2,
    pub resource: ResourceId,
}

/// A drop site for the most wanted resource whose gatherers work a cluster
/// farther than [`DROP_SITE_DISTANCE`] from every drop site for it (or,
/// while nobody gathers it and no node of it is served yet, for its
/// worthwhile cluster nearest home). A cluster's worth is what its nodes
/// still hold. `desired` = gatherers wanted per resource; `worked` = nodes
/// with gatherers.
pub fn drop_site_plan(
    view: &AiView,
    home: Vec2,
    sources: &Sources,
    desired: &[u32; MAX_RESOURCES],
    pending: &[PendingBuild],
    worked: impl Fn(Entity) -> bool,
) -> Option<DropPlan> {
    let mut order: Vec<ResourceId> = view.data.resource_ids().filter(|r| desired[r.index()] >= 2).collect();
    order.sort_by_key(|r| (std::cmp::Reverse(desired[r.index()]), r.0));
    let mut candidates: Vec<&SourceNode> = Vec::new();
    for r in order {
        let Some(kind) = drop_site_kind(view.data, view.me, r) else { continue };
        let (mut any_worked, mut any_served) = (false, false);
        candidates.clear();
        for n in sources.of(r) {
            let served = drop_distance(view, pending, r, n.pos) <= DROP_SITE_DISTANCE;
            let busy = worked(n.entity);
            any_worked |= busy;
            any_served |= served;
            if !served && busy {
                candidates.push(n);
            }
        }
        if !any_worked && !any_served {
            candidates.extend(sources.of(r));
        }
        candidates.sort_by(|a, b| a.pos.distance_squared(home).total_cmp(&b.pos.distance_squared(home)).then(a.entity.cmp(&b.entity)));
        // Each candidate costs a pass over the resource's nodes; the nearest few decide.
        for seed in candidates.iter().take(MAX_CLUSTER_SEEDS) {
            let (mut sum, mut amount, mut n) = (Vec2::ZERO, 0.0, 0.0);
            for c in sources.of(r).filter(|c| c.pos.distance(seed.pos) <= CLUSTER_RADIUS) {
                sum += c.pos;
                amount += c.remaining;
                n += 1.0;
            }
            if amount >= MIN_CLUSTER_AMOUNT {
                return Some(DropPlan { kind, anchor: sum / n, resource: r });
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Gatherer distribution
// ---------------------------------------------------------------------------

/// What the army will spend over [`PLAN_HORIZON`]: the composition's average
/// price per population, times [`ARMY_PLAN_GROWTH`] times the population
/// its actual spending lately bought, but no more than the complete army
/// production buildings turn out and at least [`ARMY_PLAN_MIN_POP`].
/// Planning for the production capacity alone would gather for an army the
/// budget (the age-up saving) does not let it buy, and leave the gathered
/// resources idle. Returns the cost and the population it buys.
pub fn army_spending(view: &AiView, plan: &Plan) -> (ResAmounts, u32) {
    let (data, me) = (view.data, view.me);
    let total_w: f32 = plan.comp.iter().map(|c| c.1).sum();
    if total_w <= 0.0 {
        return (ResAmounts::ZERO, 0);
    }
    let (mut per_pop, mut secs_per_pop) = (ResAmounts::ZERO, 0.0);
    for (u, w) in &plan.comp {
        let d = data.unit(*u);
        let pop = (d.pop * d.squad).max(1) as f32;
        let share = w / total_w;
        per_pop.add_all(&stats::unit_cost(data, me, *u).scaled(share / pop));
        secs_per_pop += share * stats::unit_train_time(data, me, *u) / pop;
    }
    let capacity = plan.production as f32 * PLAN_HORIZON / secs_per_pop.max(1.0);
    let bought = plan.army_spend.total() * PLAN_HORIZON / per_pop.total().max(1.0);
    let pop = ((bought * ARMY_PLAN_GROWTH).min(capacity).round() as u32).max(ARMY_PLAN_MIN_POP);
    (per_pop.scaled(pop as f32), pop)
}

/// Fields (farms) worn out over [`PLAN_HORIZON`] by their current users,
/// priced as the replacement fields.
pub fn field_upkeep(view: &AiView, plan: &Plan) -> ResAmounts {
    let (data, me) = (view.data, view.me);
    let mut upkeep = ResAmounts::ZERO;
    for r in data.resource_ids() {
        let Some(kind) = field_kind(data, me, r) else { continue };
        let fields = view.snap.buildings.iter().filter_map(|b| b.field.filter(|f| b.complete && f.resource == r));
        let users: u32 = fields.map(|f| f.users).sum();
        let amount = data.building(kind).resource_field.as_ref().map_or(0.0, |f| f.amount);
        if users == 0 || amount <= 0.0 {
            continue;
        }
        let worn = users as f32 * stats::gather_rate(data, me, plan.worker_kind, r) * PLAN_HORIZON / amount;
        upkeep.add_all(&stats::building_cost(data, me, kind).scaled(worn));
    }
    upkeep
}

/// What the plans besides the age-up spend over [`PLAN_HORIZON`], split
/// into upkeep, which gathering serves first, and the army.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spending {
    /// The workers the halls can train, housing for all the growth, the next
    /// production building, worn-out fields and recent purchase shortfalls.
    pub upkeep: ResAmounts,
    /// What the army production can turn out (see [`army_spending`]).
    pub army: ResAmounts,
}

impl Spending {
    pub fn total(&self) -> ResAmounts {
        let mut t = self.upkeep;
        t.add_all(&self.army);
        t
    }
}

/// What the plans besides the age-up spend over [`PLAN_HORIZON`]. The
/// buildings are priced as the housing, the next production building and
/// the worn-out fields foreseen, or as what placements actually spent
/// lately if that is more for a resource (fields for new farmers, drop
/// sites by new clusters, towers: much of the wood goes there).
pub fn planned_spending(view: &AiView, plan: &Plan) -> Spending {
    let (data, me) = (view.data, view.me);
    let mut upkeep = ResAmounts::ZERO;
    let missing = plan.worker_target.saturating_sub(plan.workers + plan.workers_queued);
    let halls = view.snap.buildings.iter().filter(|b| b.complete && plan.trains_worker[b.kind.index()]).count().max(1);
    let per_hall = (PLAN_HORIZON / stats::unit_train_time(data, me, plan.worker_kind)).floor() as u32;
    let workers = missing.min(halls as u32 * per_hall.max(1));
    upkeep.add_all(&stats::unit_cost(data, me, plan.worker_kind).scaled(workers as f32));
    let (army, army_pop) = army_spending(view, plan);
    let mut buildings = field_upkeep(view, plan);
    if let Some(h) = house_kind(data, me) {
        let growth = (workers + army_pop + plan.queued_pop).saturating_sub(me.pop_room()).min(POP_HARD_CAP.saturating_sub(me.pop_cap));
        let houses = growth.div_ceil(data.building(h).pop.max(1));
        buildings.add_all(&stats::building_cost(data, me, h).scaled(houses as f32));
    }
    if let Some(b) = next_production_building(view, &plan.comp, &[]) {
        buildings.add_all(&stats::building_cost(data, me, b));
    }
    let lately = plan.build_spend.scaled(PLAN_HORIZON);
    for (b, l) in buildings.0.iter_mut().zip(lately.0.iter()) {
        *b = b.max(*l);
    }
    upkeep.add_all(&buildings);
    upkeep.add_all(&plan.shortfall);
    Spending { upkeep, army }
}

/// Where each resource can be gathered, as far as planning gatherers goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Supply {
    /// Most gatherers the known sources employ (0 = nothing to gather).
    pub capacity: [u32; MAX_RESOURCES],
    /// Gathered near home (else at a remote cluster, after a long walk).
    pub near: [bool; MAX_RESOURCES],
}

impl Supply {
    /// Gatherers one node employs usefully.
    pub const PER_NODE: u32 = 3;
    /// Capacity of a resource that fields (farms) can always extend.
    pub const UNBOUNDED: u32 = 1000;

    /// The supply of the resources in `sources`, plus `fields` (resources
    /// that fields produce or could produce: near home and unbounded).
    pub fn new(sources: &Sources, fields: &[bool; MAX_RESOURCES]) -> Supply {
        let mut supply = Supply { capacity: [0; MAX_RESOURCES], near: [false; MAX_RESOURCES] };
        for n in &sources.nodes {
            let i = n.resource.index();
            supply.capacity[i] = supply.capacity[i].saturating_add(Supply::PER_NODE);
            supply.near[i] |= n.home;
        }
        for (i, field) in fields.iter().enumerate() {
            if *field {
                supply.capacity[i] = Supply::UNBOUNDED;
                supply.near[i] = true;
            }
        }
        supply
    }
}

/// What one gatherer of each resource is counted on to collect per second:
/// the nominal rate times [`GATHER_EFFICIENCY`] near home or
/// [`REMOTE_EFFICIENCY`] at a remote cluster. 0 = cannot gather.
pub fn effective_rates(view: &AiView, plan: &Plan, supply: &Supply) -> [f32; MAX_RESOURCES] {
    let mut rates = [0.0; MAX_RESOURCES];
    for r in view.data.resource_ids() {
        let efficiency = if supply.near[r.index()] { GATHER_EFFICIENCY } else { REMOTE_EFFICIENCY };
        rates[r.index()] = stats::gather_rate(view.data, view.me, plan.worker_kind, r) * efficiency;
    }
    rates
}

/// Gatherers to collect what `cost` lacks beyond `stock` and what the
/// `passive` income (per second) brings in within `horizon` seconds at
/// `rates`: at most `room` in all and `per_room` per resource. What the
/// stock covers is taken out of it.
#[allow(clippy::too_many_arguments)]
fn gatherers_for(
    cost: &ResAmounts,
    stock: &mut ResAmounts,
    passive: &ResAmounts,
    rates: &[f32; MAX_RESOURCES],
    horizon: f32,
    room: u32,
    per_room: &[u32; MAX_RESOURCES],
) -> [u32; MAX_RESOURCES] {
    let mut exact = [0.0; MAX_RESOURCES];
    for i in 0..MAX_RESOURCES {
        let lacking = (cost.0[i] - stock.0[i] - passive.0[i].max(0.0) * horizon).max(0.0);
        stock.0[i] = (stock.0[i] - cost.0[i]).max(0.0);
        if rates[i] > 0.0 && per_room[i] > 0 {
            exact[i] = (lacking / (rates[i] * horizon)).min(per_room[i] as f32);
        }
    }
    let whole = exact.map(|x| x.ceil() as u32);
    if whole.iter().sum::<u32>() <= room { whole } else { apportion_capped(&exact, room, per_room) }
}

/// Amount of each resource wanted once the age-up path and the upkeep are
/// served from the stockpile (`stock` = what is left of it after them): the
/// army's spending beyond it, plus a standing demand in the faction's
/// `eco_focus` order for what the plans or the age-ups use, until the stock
/// covers them (half of the following age-up included). What the passive
/// income brings in over [`PLAN_HORIZON`] counts as stock.
pub fn gather_needs(view: &AiView, plan: &Plan, spending: &Spending, stock: &ResAmounts) -> [f32; MAX_RESOURCES] {
    let (data, me) = (view.data, view.me);
    let mut stock = *stock;
    stock.add_all(&plan.passive.scaled(PLAN_HORIZON));
    let stock = &stock;
    let mut need = [0.0; MAX_RESOURCES];
    for r in data.resource_ids() {
        need[r.index()] = (spending.army.get(r) - stock.get(r)).max(0.0);
    }
    let planned = spending.total();
    let after_next = data.age_up_tech(me.faction, me.age + 1).map_or(ResAmounts::ZERO, |t| data.tech(t).cost.amounts);
    let focus = &view.faction().ai.eco_focus;
    let ranked: Vec<ResourceId> =
        if focus.is_empty() { data.resource_ids().collect() } else { focus.iter().filter_map(|id| data.resource_id(id)).collect() };
    for (i, r) in ranked.iter().enumerate() {
        let wanted = planned.get(*r) + 0.5 * after_next.get(*r);
        let used = wanted > 0.0 || plan.age_cost.get(*r) > 0.0;
        if data.resource(*r).tier > me.age || !used || stock.get(*r) >= wanted {
            continue;
        }
        let base = FOCUS_BASE * (1.0 - FOCUS_STEP * i as f32).max(0.2);
        need[r.index()] += base / (1.0 + stock.get(*r) / STOCKPILE_SOFT_CAP);
    }
    need
}

/// Split `total` into whole numbers proportional to `weights` (largest
/// remainder; ties go to the lower index).
pub fn apportion(weights: &[f32; MAX_RESOURCES], total: u32) -> [u32; MAX_RESOURCES] {
    let mut out = [0u32; MAX_RESOURCES];
    let sum: f32 = weights.iter().filter(|w| **w > 0.0).sum();
    if sum <= 0.0 || total == 0 {
        return out;
    }
    let mut rest = [f32::NEG_INFINITY; MAX_RESOURCES];
    let mut given = 0;
    for i in 0..MAX_RESOURCES {
        if weights[i] > 0.0 {
            let exact = weights[i] / sum * total as f32;
            out[i] = exact.floor() as u32;
            rest[i] = exact - out[i] as f32;
            given += out[i];
        }
    }
    while given < total {
        let mut best = 0;
        for i in 1..MAX_RESOURCES {
            if rest[i] > rest[best] {
                best = i;
            }
        }
        out[best] += 1;
        rest[best] = f32::NEG_INFINITY;
        given += 1;
    }
    out
}

/// [`apportion`], but no resource gets more than `caps` allows; what a
/// capped resource cannot take goes to the others, in proportion.
pub fn apportion_capped(weights: &[f32; MAX_RESOURCES], total: u32, caps: &[u32; MAX_RESOURCES]) -> [u32; MAX_RESOURCES] {
    let mut out = [0u32; MAX_RESOURCES];
    let mut open: [f32; MAX_RESOURCES] = std::array::from_fn(|i| if caps[i] > 0 { weights[i] } else { 0.0 });
    let mut left = total;
    while left > 0 && open.iter().any(|w| *w > 0.0) {
        let share = apportion(&open, left);
        left = 0;
        for i in 0..MAX_RESOURCES {
            let take = share[i].min(caps[i] - out[i]);
            out[i] += take;
            left += share[i] - take;
            if out[i] >= caps[i] {
                open[i] = 0.0;
            }
        }
    }
    out
}

/// Stock beyond the reserve and this much more counts as piling up (see
/// [`desired_gatherers`]).
pub const PILE_KEEP: f32 = 200.0;

/// Gatherers wanted per resource among `total` workers, in three tiers: the
/// age-up path's lacking resources within [`AGE_UP_HORIZON`] (from at most
/// [`AGE_UP_WORKER_SHARE`] of the workers), then the upkeep within
/// [`PLAN_HORIZON`], then the rest split by [`gather_needs`] over what a
/// gatherer collects. Passive income is counted on throughout, and no
/// resource gets more than its sources employ. Finally a resource piling up
/// (more than [`PILE_KEEP`] beyond the reserve) gives up the gatherers whose
/// work the pile already holds over [`PLAN_HORIZON`] to what the age-up
/// still lacks (or else to the rest of the plan): plans can overshoot (an
/// army waiting on the saving's food does not spend its iron), and a pile is
/// work the bottleneck did not get.
pub fn desired_gatherers(view: &AiView, plan: &Plan, supply: &Supply, total: u32) -> [u32; MAX_RESOURCES] {
    let rates = effective_rates(view, plan, supply);
    let spending = planned_spending(view, plan);
    let mut stock = view.me.resources;
    let mut room = supply.capacity;
    let mut out = [0u32; MAX_RESOURCES];
    let give = |out: &mut [u32; MAX_RESOURCES], room: &mut [u32; MAX_RESOURCES], add: [u32; MAX_RESOURCES]| {
        for i in 0..MAX_RESOURCES {
            out[i] += add[i];
            room[i] = room[i].saturating_sub(add[i]);
        }
    };
    let passive = plan.passive;
    let age_room = (total as f32 * AGE_UP_WORKER_SHARE).floor() as u32;
    let age = gatherers_for(&plan.age_cost, &mut stock, &passive, &rates, AGE_UP_HORIZON, age_room, &room);
    give(&mut out, &mut room, age);
    let left = total.saturating_sub(out.iter().sum());
    let upkeep = gatherers_for(&spending.upkeep, &mut stock, &passive, &rates, PLAN_HORIZON, left, &room);
    give(&mut out, &mut room, upkeep);
    let left = total.saturating_sub(out.iter().sum());
    let need = gather_needs(view, plan, &spending, &stock);
    let weights: [f32; MAX_RESOURCES] = std::array::from_fn(|i| if rates[i] > 0.0 { need[i] / rates[i] } else { 0.0 });
    let rest = apportion_capped(&weights, left, &room);
    give(&mut out, &mut room, rest);
    shift_from_piles(&view.me.resources, &plan.reserve, &plan.age_cost, &rates, &weights, &mut out, &mut room);
    out
}

/// Move the gatherers (`out`, with `room` left at each resource's sources)
/// of resources piling up in `have` beyond `reserve` (see
/// [`desired_gatherers`]) to what `age_cost` still lacks, by what each lacks
/// per gatherer at `rates`, or else by `weights`.
pub fn shift_from_piles(
    have: &ResAmounts,
    reserve: &ResAmounts,
    age_cost: &ResAmounts,
    rates: &[f32; MAX_RESOURCES],
    weights: &[f32; MAX_RESOURCES],
    out: &mut [u32; MAX_RESOURCES],
    room: &mut [u32; MAX_RESOURCES],
) {
    let mut piling = [false; MAX_RESOURCES];
    let mut freed = 0;
    for i in 0..MAX_RESOURCES {
        let pile = have.0[i] - reserve.0[i] - PILE_KEEP;
        if pile > 0.0 && rates[i] > 0.0 {
            piling[i] = true;
            let cut = ((pile / (rates[i] * PLAN_HORIZON)).floor() as u32).min(out[i]);
            out[i] -= cut;
            room[i] += cut;
            freed += cut;
        }
    }
    if freed == 0 {
        return;
    }
    let lacking: [f32; MAX_RESOURCES] = std::array::from_fn(|i| {
        let short = (age_cost.0[i] - have.0[i]).max(0.0);
        if piling[i] || rates[i] <= 0.0 { 0.0 } else { short / rates[i] }
    });
    let to: [f32; MAX_RESOURCES] =
        if lacking.iter().any(|w| *w > 0.0) { lacking } else { std::array::from_fn(|i| if piling[i] { 0.0 } else { weights[i] }) };
    let moved = apportion_capped(&to, freed, room);
    for i in 0..MAX_RESOURCES {
        out[i] += moved[i];
        room[i] = room[i].saturating_sub(moved[i]);
    }
}
