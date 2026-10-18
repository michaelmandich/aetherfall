//! Progression decisions: the age-up path, research choices, military
//! production buildings, aether vents and towers. All read only the view.

use af_core::prelude::*;
use af_core::stats::Target;
use bevy::prelude::*;

use crate::memory::PendingBuild;
use crate::view::{AiView, BuildingInfo};

/// Military production buildings wanted per type, by age (1..=5). The most
/// important type gets this many, the others one fewer (at least one).
pub const PRODUCTION_PER_AGE: [u32; 5] = [1, 2, 2, 3, 3];

/// Towers near the main hall for defensive (aggression < 0.5) factions, by age.
pub const TOWERS_PER_AGE: [u32; 5] = [0, 1, 2, 3, 4];
/// Towers for aggressive factions, by age: one against raids, later two.
pub const RAID_TOWERS_PER_AGE: [u32; 5] = [0, 0, 1, 1, 2];

/// Most army production buildings the AI keeps in `age`, counting those
/// added to spend a surplus.
pub fn max_production(age: u8) -> u32 {
    2 + 2 * age.clamp(1, MAX_AGE) as u32
}

/// The next step toward the next age.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AgeUpStep {
    /// Final age, or the faction has no further age-up.
    Done,
    /// The age-up is being researched.
    InProgress,
    /// Research `tech` (the age-up or a prerequisite) at `building` now: it is affordable.
    Research { tech: TechId, building: Entity },
    /// Upgrade `building` into the building that researches the age-up: affordable now.
    Upgrade { building: Entity },
    /// Construct this missing building first.
    Build(BuildingTypeId),
    /// A requirement is on its way (foundation, queued upgrade or research).
    Wait,
    /// Only resources are missing: save up `cost`.
    Save { cost: ResAmounts },
    /// The faction's content does not allow the next age from here.
    Blocked,
}

impl AgeUpStep {
    /// Something to do now, rather than wait or save.
    fn actionable(&self) -> bool {
        matches!(self, AgeUpStep::Research { .. } | AgeUpStep::Upgrade { .. } | AgeUpStep::Build(_))
    }
}

/// Prerequisite chains (techs needing techs, buildings upgraded from
/// buildings) are followed at most this deep.
const MAX_DEPTH: u32 = 8;

fn researching(me: &PlayerState, tech: TechId) -> bool {
    me.researching.get(tech.index()).copied().unwrap_or(false)
}

/// What to do next to reach the next age. Checks the age-up tech's
/// `requires_buildings`, prerequisite techs (and theirs, recursively), and
/// the building that researches it (upgrading into it when that is how it
/// is obtained), and affordability against the player's stockpile.
pub fn age_up_step(view: &AiView, pending: &[PendingBuild]) -> AgeUpStep {
    let (data, me) = (view.data, view.me);
    let Some(tech) = data.age_up_tech(me.faction, me.age) else { return AgeUpStep::Done };
    if researching(me, tech) {
        return AgeUpStep::InProgress;
    }
    tech_step(view, tech, pending, 0)
}

/// The next step toward researching `tech`. Requirements are independent,
/// so the first one that can be acted on now wins (an upgrade does not wait
/// for another building's foundation); otherwise the first one held up
/// (saving or waiting) is reported. A requirement that can never be met
/// blocks the tech.
fn tech_step(view: &AiView, tech: TechId, pending: &[PendingBuild], depth: u32) -> AgeUpStep {
    let me = view.me;
    let def = view.data.tech(tech);
    let requirements = def
        .requires_building_ids
        .iter()
        .map(|b| ensure_building(view, *b, pending, 0))
        .chain(def.requires_ids.iter().filter(|t| !me.has_tech(**t)).map(|t| Some(prerequisite_step(view, *t, pending, depth))))
        .chain(std::iter::once_with(|| ensure_building(view, def.researched_at_id, pending, 0)));
    let mut held = None;
    for step in requirements.flatten() {
        match step {
            AgeUpStep::Blocked => return AgeUpStep::Blocked,
            s if s.actionable() => return s,
            s => {
                held.get_or_insert(s);
            }
        }
    }
    held.unwrap_or_else(|| research_at(view, tech, def.researched_at_id))
}

/// The step toward a prerequisite tech; its research under way means waiting.
fn prerequisite_step(view: &AiView, tech: TechId, pending: &[PendingBuild], depth: u32) -> AgeUpStep {
    if researching(view.me, tech) {
        return AgeUpStep::Wait;
    }
    if depth >= MAX_DEPTH {
        return AgeUpStep::Blocked;
    }
    match tech_step(view, tech, pending, depth + 1) {
        AgeUpStep::InProgress => AgeUpStep::Wait,
        s => s,
    }
}

fn research_at(view: &AiView, tech: TechId, at: BuildingTypeId) -> AgeUpStep {
    let Some(host) = least_busy(view, at) else { return AgeUpStep::Wait };
    match rules::can_research(view.data, view.me, at, tech, true) {
        Ok(()) => AgeUpStep::Research { tech, building: host.entity },
        Err(Reason::CannotAfford) => AgeUpStep::Save { cost: view.data.tech(tech).cost.amounts },
        Err(Reason::InProgress) => AgeUpStep::InProgress,
        Err(Reason::MissingBuilding(_) | Reason::MissingTech(_)) => AgeUpStep::Wait,
        Err(_) => AgeUpStep::Blocked,
    }
}

/// `None` when a complete `kind` is owned; otherwise the step that gets
/// one: wait for one under way, upgrade an owned building into it, build it
/// from the build menu, or first get the building it is upgraded from (a
/// type only reachable by upgrade cannot be placed directly, by a human or
/// the AI).
fn ensure_building(view: &AiView, kind: BuildingTypeId, pending: &[PendingBuild], depth: u32) -> Option<AgeUpStep> {
    let (data, me) = (view.data, view.me);
    if view.count_owned(kind) > 0 {
        return None;
    }
    if view.count_any(kind) > 0 || pending.iter().any(|p| p.kind == kind) {
        return Some(AgeUpStep::Wait);
    }
    let upgrades_to = |b: BuildingTypeId| data.building(b).upgrade.as_ref().is_some_and(|u| u.to_id == kind);
    if let Some(src) = view.snap.buildings.iter().find(|b| b.complete && upgrades_to(b.kind)) {
        if view.snap.queue(src).iter().any(|q| matches!(q, QueueKind::Upgrade(_))) {
            return Some(AgeUpStep::Wait);
        }
        return Some(match rules::can_upgrade(data, me, src.kind, true) {
            Ok(_) => AgeUpStep::Upgrade { building: src.entity },
            Err(Reason::CannotAfford) => {
                AgeUpStep::Save { cost: data.building(src.kind).upgrade.as_ref().map_or(ResAmounts::ZERO, |u| u.cost.amounts) }
            }
            Err(_) => AgeUpStep::Blocked,
        });
    }
    if rules::build_menu(data, me).contains(&kind) {
        return Some(match rules::can_build(data, me, kind, false) {
            Ok(()) => AgeUpStep::Build(kind),
            Err(_) => AgeUpStep::Blocked,
        });
    }
    let from = view.faction().building_ids.iter().copied().find(|b| upgrades_to(*b));
    Some(match from {
        // A source exists but not complete in the snapshot yet: it is on its way.
        Some(from) if depth < MAX_DEPTH => ensure_building(view, from, pending, depth + 1).unwrap_or(AgeUpStep::Wait),
        _ => AgeUpStep::Blocked,
    })
}

/// Everything the age-up path still costs: the tech, missing prerequisite
/// techs (recursively) and buildings (or the upgrades into them).
pub fn age_path_cost(view: &AiView) -> ResAmounts {
    let (data, me) = (view.data, view.me);
    let mut total = ResAmounts::ZERO;
    let Some(tech) = data.age_up_tech(me.faction, me.age) else { return total };
    if researching(me, tech) {
        return total;
    }
    add_tech_cost(view, tech, &mut Vec::new(), &mut total, 0);
    total
}

/// Add the price of `tech` and of what it still lacks. `seen` = buildings
/// already counted (several techs may need the same one).
fn add_tech_cost(view: &AiView, tech: TechId, seen: &mut Vec<BuildingTypeId>, total: &mut ResAmounts, depth: u32) {
    let def = view.data.tech(tech);
    total.add_all(&def.cost.amounts);
    for b in def.requires_building_ids.iter().chain(std::iter::once(&def.researched_at_id)) {
        add_building_cost(view, *b, seen, total, 0);
    }
    for t in &def.requires_ids {
        if depth < MAX_DEPTH && !view.me.has_tech(*t) && !researching(view.me, *t) {
            add_tech_cost(view, *t, seen, total, depth + 1);
        }
    }
}

/// Add what getting a `kind` still costs: nothing when one exists, else the
/// upgrade of an owned building into it, its price from the build menu, or
/// the upgrade plus what the building it comes from costs.
fn add_building_cost(view: &AiView, kind: BuildingTypeId, seen: &mut Vec<BuildingTypeId>, total: &mut ResAmounts, depth: u32) {
    let (data, me) = (view.data, view.me);
    if view.count_any(kind) > 0 || seen.contains(&kind) || depth > MAX_DEPTH {
        return;
    }
    seen.push(kind);
    let upgrade_into = |b: BuildingTypeId| data.building(b).upgrade.as_ref().filter(|u| u.to_id == kind);
    if let Some(u) = view.snap.buildings.iter().find_map(|b| upgrade_into(b.kind)) {
        total.add_all(&u.cost.amounts);
    } else if rules::build_menu(data, me).contains(&kind) {
        total.add_all(&stats::building_cost(data, me, kind));
    } else if let Some((from, u)) = view.faction().building_ids.iter().find_map(|b| upgrade_into(*b).map(|u| (*b, u))) {
        total.add_all(&u.cost.amounts);
        add_building_cost(view, from, seen, total, depth + 1);
    }
}

/// The complete own building of `kind` with the shortest queue.
pub fn least_busy<'a>(view: &AiView<'a>, kind: BuildingTypeId) -> Option<&'a BuildingInfo> {
    view.snap.buildings.iter().filter(|b| b.complete && b.kind == kind).min_by_key(|b| view.snap.queue(b).len())
}

// ---------------------------------------------------------------------------
// Tech classification
// ---------------------------------------------------------------------------

/// Gather-rate, carry-capacity and income techs.
pub fn is_eco_tech(def: &TechDef) -> bool {
    !def.is_age_up()
        && def.effects.iter().any(|e| matches!(e, Effect::GatherRate { .. } | Effect::CarryCapacity { .. } | Effect::Trickle { .. }))
}

/// Does any effect of `def` improve units of type `u`?
pub fn tech_helps_unit(data: &GameData, def: &TechDef, u: UnitTypeId) -> bool {
    def.effects.iter().any(|e| match e {
        Effect::Stat { target, .. } | Effect::CostMult { target, .. } => {
            Target::resolve(data, target).is_some_and(|t| t.matches_unit(data, u))
        }
        Effect::UpgradeUnit { from, .. } => data.unit_id(from) == Some(u),
        _ => false,
    })
}

/// Is an economy tech worth it now? `gathering` = workers per resource.
pub fn eco_tech_useful(data: &GameData, def: &TechDef, gathering: &[u32; MAX_RESOURCES], workers: u32) -> bool {
    def.effects.iter().any(|e| match e {
        Effect::GatherRate { resource, .. } => data.resource_id(resource).is_some_and(|r| gathering[r.index()] >= 2),
        Effect::CarryCapacity { .. } => workers >= 10,
        Effect::Trickle { .. } => true,
        _ => false,
    })
}

// ---------------------------------------------------------------------------
// Buildings to construct
// ---------------------------------------------------------------------------

/// Military production building types to have, with how many of each and
/// their priority (sum of composition weights they serve), most important
/// first. Town centres are never counted here.
pub fn production_targets(view: &AiView, comp: &[(UnitTypeId, f32)]) -> Vec<(BuildingTypeId, u32, f32)> {
    let (data, me) = (view.data, view.me);
    let menu = rules::build_menu(data, me);
    let mut weights: Vec<(BuildingTypeId, f32)> = Vec::new();
    for (u, w) in comp {
        // Count a type already owned, else the cheapest one that can be built.
        let owned = trainers(view, *u).find(|b| view.count_any(*b) > 0);
        let chosen = owned.or_else(|| {
            trainers(view, *u)
                .filter(|b| {
                    menu.contains(b) && rules::can_build(data, me, *b, false).is_ok() && data.building(*b).requires_node_id.is_none()
                })
                .min_by(|a, b| cost_total(data, me, *a).total_cmp(&cost_total(data, me, *b)))
        });
        let Some(b) = chosen else { continue };
        match weights.iter_mut().find(|(k, _)| *k == b) {
            Some(e) => e.1 += *w,
            None => weights.push((b, *w)),
        }
    }
    weights.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let per = PRODUCTION_PER_AGE[(me.age.clamp(1, 5) - 1) as usize];
    weights.iter().enumerate().map(|(i, (b, w))| (*b, if i == 0 { per } else { per.saturating_sub(1).max(1) }, *w)).collect()
}

/// Faction building types (not town centres) where `u` may be trained.
fn trainers<'a>(view: &AiView<'a>, u: UnitTypeId) -> impl Iterator<Item = BuildingTypeId> + 'a {
    let (data, me) = (view.data, view.me);
    view.faction()
        .building_ids
        .iter()
        .copied()
        .filter(move |b| !data.building(*b).has_tag(Tag::TownCenter) && rules::can_train(data, me, *b, u, false).is_ok())
}

/// The cheapest building type on the build menu that trains `u`, when no
/// building that does is owned, laid out or placed (`pending`).
pub fn missing_trainer(view: &AiView, u: UnitTypeId, pending: &[PendingBuild]) -> Option<BuildingTypeId> {
    let (data, me) = (view.data, view.me);
    if trainers(view, u).any(|b| view.count_any(b) > 0 || pending.iter().any(|p| p.kind == b)) {
        return None;
    }
    let menu = rules::build_menu(data, me);
    trainers(view, u)
        .filter(|b| menu.contains(b) && data.building(*b).requires_node_id.is_none() && rules::can_build(data, me, *b, false).is_ok())
        .min_by(|a, b| cost_total(data, me, *a).total_cmp(&cost_total(data, me, *b)))
}

/// The next military production building to construct (from the build
/// menu), if any is missing.
pub fn next_production_building(view: &AiView, comp: &[(UnitTypeId, f32)], pending: &[PendingBuild]) -> Option<BuildingTypeId> {
    let menu = rules::build_menu(view.data, view.me);
    production_targets(view, comp).into_iter().find_map(|(b, want, _)| {
        let have = view.count_any(b) + pending.iter().filter(|p| p.kind == b).count() as u32;
        (have < want && menu.contains(&b) && rules::can_build(view.data, view.me, b, false).is_ok()).then_some(b)
    })
}

/// The first military production building to construct: of the types the
/// mix is trained at (see [`production_targets`]), the cheapest on the build
/// menu, so that the first soldiers come as soon as possible; the mix's
/// most important type follows (see [`next_production_building`]).
pub fn first_production_building(view: &AiView, comp: &[(UnitTypeId, f32)]) -> Option<BuildingTypeId> {
    let (data, me) = (view.data, view.me);
    let menu = rules::build_menu(data, me);
    production_targets(view, comp)
        .into_iter()
        .map(|(b, ..)| b)
        .filter(|b| menu.contains(b) && data.building(*b).requires_node_id.is_none() && rules::can_build(data, me, *b, false).is_ok())
        .min_by(|a, b| cost_total(data, me, *a).total_cmp(&cost_total(data, me, *b)).then(a.cmp(b)))
}

/// Total of a building's price (for "cheapest" choices).
pub fn cost_total(data: &GameData, me: &PlayerState, b: BuildingTypeId) -> f32 {
    stats::building_cost(data, me, b).total()
}

/// The cheapest buildable defensive tower (see [`is_tower`]).
pub fn tower_kind(data: &GameData, me: &PlayerState) -> Option<BuildingTypeId> {
    rules::build_menu(data, me)
        .into_iter()
        .filter(|b| {
            is_tower(data, me, *b) && data.building(*b).requires_node_id.is_none() && rules::can_build(data, me, *b, false).is_ok()
        })
        .min_by(|a, b| cost_total(data, me, *a).total_cmp(&cost_total(data, me, *b)))
}

/// How many towers a faction with `aggression` wants in `age`.
pub fn tower_target(age: u8, aggression: f32) -> u32 {
    let i = (age.clamp(1, 5) - 1) as usize;
    if aggression >= 0.5 { RAID_TOWERS_PER_AGE[i] } else { TOWERS_PER_AGE[i] }
}

/// A defensive tower: Tower-tagged, armed, training nothing (a castle that
/// trains units is a production building, not a tower).
pub fn is_tower(data: &GameData, me: &PlayerState, kind: BuildingTypeId) -> bool {
    let d = data.building(kind);
    d.has_tag(Tag::Tower) && !d.has_tag(Tag::TownCenter) && !d.weapon_ids.is_empty() && data.trainable_at(kind, me.faction).is_empty()
}

/// The cheapest buildable plain wall segment (not a gate).
pub fn wall_kind(data: &GameData, me: &PlayerState) -> Option<BuildingTypeId> {
    rules::build_menu(data, me)
        .into_iter()
        .filter(|b| {
            let d = data.building(*b);
            d.wall && !d.gate && rules::can_build(data, me, *b, false).is_ok()
        })
        .min_by(|a, b| cost_total(data, me, *a).total_cmp(&cost_total(data, me, *b)))
}

/// Buildings that must sit on a node (aether extractors), with that node type.
pub fn node_claimers(data: &GameData, me: &PlayerState) -> Vec<(BuildingTypeId, NodeTypeId)> {
    rules::build_menu(data, me)
        .into_iter()
        .filter_map(|b| data.building(b).requires_node_id.map(|n| (b, n)))
        .filter(|(b, _)| rules::can_build(data, me, *b, false).is_ok())
        .collect()
}

/// How many vent claims the AI keeps in `age`.
pub fn vent_target(age: u8) -> u32 {
    age.saturating_sub(1) as u32
}

/// Nearest node of `kind` to `from` with no building on it, skipping nodes
/// within a metre of an `avoid` point (claims that failed there before).
pub fn nearest_free_node(view: &AiView, kind: NodeTypeId, from: Vec2, avoid: &[Vec2]) -> Option<Vec2> {
    let spatial = view.spatial;
    spatial
        .nearest_anywhere(from, |e| {
            e.kind == SpatialKind::Node && e.type_index == kind.0 && !avoid.iter().any(|a| a.distance_squared(e.pos) < 1.0) && {
                let mut claimed = false;
                spatial.for_each_near(e.pos, 0.5, |o| claimed |= o.kind == SpatialKind::Building && o.pos.distance(e.pos) < 0.5);
                !claimed
            }
        })
        .map(|e| e.pos)
}
