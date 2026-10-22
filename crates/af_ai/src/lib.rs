//! Aetherfall computer opponents: economy, building, research, armies, attacks, defence.
//!
//! Every player whose `Controller` is `Ai` (and who is not defeated) thinks
//! every [`THINK_INTERVAL_TICKS`] ticks, staggered by player index, in
//! `SimSet::Ai`. A think:
//!
//! 1. collects a [`Snapshot`] of the player's own units and buildings and
//!    refreshes the hostile buildings and armies it can see (fog
//!    permitting, through `SeenBy`, so a revealed player's buildings are
//!    seen wherever they stand), its smoothed income and what its army and
//!    its placements actually spent lately;
//! 2. derives a [`Plan`] (home, counts, the age-up step, the army target and
//!    goal, the wave due now, the counter-adjusted army mix, passive income,
//!    and the reserve: the next purchase on the age-up path whole, the rest
//!    [just in time](just_in_time_reserve));
//! 3. runs plain decision functions over an [`AiView`] (worker target,
//!    gatherer split in tiers: age-up, upkeep, army, with piles handing
//!    their gatherers to the bottleneck; drop sites, farm hubs, fields that
//!    wait for their farmers, houses, age-ups, production that grows with
//!    the surplus, vents, towers, walls and their upgrades, training,
//!    research, spending what piles up, waves, retreats, target choice,
//!    hunting, defence) and
//! 4. pushes the resulting `PlayerCommand`s into `CommandInbox`, exactly as
//!    a human player would. Permissions come from `af_core::rules`, prices
//!    from `af_core::stats`, spots from `af_core::placement`, and no spot
//!    closes a ring of buildings on the realm's own ground units (see
//!    [`seals_pocket`]). What the AI weighs of hostile armies, walls and
//!    buildings is what its team sees (the box test of `ext::fog`), but for
//!    the one cheat below.
//!
//! # Waves, saving and finishing
//!
//! * The first wave is due between [`MIN_ATTACK_TIME`] and
//!   [`MIN_ATTACK_TIME`] + [`TURTLE_DELAY`] by the faction's aggression,
//!   later ones [`WAVE_INTERVAL`] after the last left. A due wave waits for
//!   the full [`attack_threshold`], then for less and less: the first down
//!   to [`wave_floor`], later ones down to [`LATER_WAVE_FLOOR_SHARE`] of it
//!   (see [`wave_size`]), so every faction attacks, a little later and
//!   smaller when its army is small, and later waves go in strong enough to
//!   do something. Against the last enemy left, an army well above what it
//!   was seen to field ([`ADVANTAGE`]) goes as soon as the smallest wave is
//!   ready: a beaten realm gets no time to recover.
//! * Army training below the army's goal (half its target, and at least the
//!   smallest wave) or in defence may borrow from the age-up saving, but
//!   never more than a fifth of it, and nothing once the age-up is behind
//!   [`AGE_SCHEDULE`]: a realm under pressure still ages up. A raided realm
//!   with next to no army, and one razing the town of the last enemy left,
//!   spend all of it on soldiers while the age-up keeps to that schedule;
//!   behind it, only a raid in earnest (more than a scout's poke) or a last
//!   enemy that lost every production building (all but dead) still takes
//!   it all, so winners and losers alike keep aging. While the army is
//!   short, a production building the mix trains nothing at (raised in an
//!   earlier age for stand-in units) trains a [`filler_unit`] rather than
//!   standing idle. Workers are trained up to the age's target out of the
//!   saving too, until the saving has all but reached the age-up (or, past
//!   half the target, the age-up is behind schedule).
//! * The first army production building is due once the age-up to the
//!   second age is under way, and from then on whenever none stands: the
//!   cheapest that trains the mix ([`first_production_building`]). It, the
//!   age-up's own building and the workshop of the breakers a razed town
//!   waits for have their price held back from the rest of a think while
//!   they cannot be paid for, so that fields and houses bought as wood
//!   trickles in never keep them waiting.
//! * A wave aims at the best target's nearest known building (production
//!   buildings first, and more so while it presses an attack), else at its
//!   start position, else it hunts: it searches unseen ground near that
//!   start ([`search_spot`]), spot after spot, and turns on any of its
//!   buildings that comes to light. A player who lost every production
//!   building is revealed by af_economy, so its last buildings are always
//!   known; once none of them keeps it in the game either, the AI uses its
//!   one intentional cheat and goes straight for that player's remaining
//!   units wherever they hide ([`Aim::Remnant`], `AiMemory::remnants`),
//!   instead of searching the map for a lone worker until the match ends.
//! * A wave that stands in the goal's town with what defended it beaten
//!   razes it: its [building breakers](is_breaker) (rams, catapults,
//!   bombards, giants, most melee infantry: whatever takes most hp from
//!   buildings per population) strike one building after another, the
//!   town's production first, while the others hunt its workers and guard
//!   them; it does not retreat for being spent while nobody defends, and the
//!   home guard shrinks to reinforce it. Meanwhile the mix asks for the
//!   [best breaker](best_breaker) per resource (more the less the army
//!   fielded could tear the known buildings down in [`RAZE_TIME`]), saves
//!   for it before cheaper units, and builds its workshop first. Later
//!   waves turn on the realm razed lately before a fresh one.
//!
//! # Big maps
//!
//! What the terrain lets units reach comes from af_movement's
//! `NavRegions` (absent in flat-world tests, where everything is reachable
//! by the straight line): remote resources, vents, building spots and the
//! workers sent to them are taken in the workers' region, nearest by the
//! way round; attack targets in the army's (by the class most of the army
//! is: siege on wheels crosses rivers at fords only), and an attack's
//! progress is the route distance left, so a detour round by a ford is no
//! stall. Army units no wheeled or tracked route leads from home to the
//! contested middle are left out of the mix. The army's front faces along
//! the way from home toward the main meteor site (af_map's `MapFeatures`,
//! else `MapInfo`'s crater), and the military radii (defence, home, recall)
//! grow with the map beyond [`TUNED_MAP_SIDE`].
//!
//! Everything is driven by content: the faction's worker, `ai` profile,
//! tags, weapon bonuses and the rules' menus; no unit or building id is
//! hard-coded. Memory between thinks (pending placements, assigned workers,
//! sightings, intel, grudges, income, spending, attack state, searched
//! spots) lives in [`AiBrains`].
//!
//! # Logs
//!
//! Wave launches log at info level under `af_ai::waves`, hunts under
//! `af_ai::hunt`, and once a minute each AI's state (age-up step, army,
//! bank, reserve, gatherer split, shortfalls, mix) at debug level under
//! `af_ai::status`: `RUST_LOG=warn,af_ai::waves=info,af_ai::status=debug`.

mod army;
mod budget;
mod build;
mod economy;
mod lanes;
mod memory;
mod plan;
mod progress;
mod think;
mod view;

#[cfg(test)]
mod tests;

use af_core::ext::economy::{RevealedPlayers, keeps_player_alive};
use af_core::ext::map::MapFeatures;
use af_core::ext::movement::NavRegions;
use af_core::prelude::*;
use bevy::prelude::*;

pub use army::{
    AGE_DECAY, ARMY_SHARE, Aim, BREAKER_RATE, FILLER_WEIGHT, FIRST_WAVE_PRESSURE_TIME, LATER_WAVE_FLOOR_SHARE, MIN_ATTACK_TIME,
    MIN_WAVE_POP, Objective, RAZE_EXTRA_SHARE, RAZE_MEMORY, RAZE_SIEGE_SHARE, RAZE_TIME, SEARCH_MEMORY, SEARCH_STEP, SIEGE_SHARE,
    SURPLUS_KEEP, SURPLUS_PULL, TURTLE_DELAY, WAVE_FLOOR_SHARE, WAVE_INTERVAL, WAVE_PRESSURE_TIME, army_composition, army_mix,
    army_target, attack_threshold, best_breaker, breach_rate, breaking_gap, choose_target, choose_target_for, effectiveness, hits_air,
    filler_unit, is_breaker, is_breaker_vs, min_attack_time, pick_unit, search_spot, should_attack, siege_targets, surplus, surplus_share,
    wanted_unit, wave_due, wave_floor, wave_size,
};
pub use budget::{Budget, SAVE_HORIZON, just_in_time_reserve};
pub use build::{NODE_GAP, SpotQuery, clear_around, find_build_spot, pick_workers};
pub use economy::{
    AGE_UP_HORIZON, AGE_UP_WORKER_SHARE, ARMY_PLAN_GROWTH, ARMY_PLAN_MIN_POP, CLUSTER_RADIUS, DROP_SITE_DISTANCE, DropPlan, FIELD_RING,
    FIELDS_PER_HUB, FieldSpot, GATHER_EFFICIENCY, HOME_RADIUS, PILE_KEEP, PLAN_HORIZON, REMOTE_CLUSTER_RADIUS, REMOTE_EFFICIENCY,
    REMOTE_NODES_WANTED, SourceNode, Sources, Spending, Supply, WORKER_TARGETS, apportion, apportion_capped, army_spending,
    desired_gatherers, drop_site_kind, drop_site_plan, effective_rates, field_kind, field_spot, field_upkeep, find_remote, gather_needs,
    house_kind, needs_house, planned_spending, search_remote, shift_from_piles, worker_target,
};
pub use lanes::{Mover, PINCH_WIDTH, POCKET_WINDOW, WALK_MARGIN, ground_movers, seals_pocket};
pub use memory::{
    AiBrains, AiMemory, Alert, AttackState, EnemyIntel, GRUDGE_TIME, HomeWay, INCOME_WINDOW, INTEL_HALF_LIFE, MAX_REMOTE_CLUSTERS, PendingBuild,
    RemoteSearch, SPEND_WINDOW, SPLIT_WINDOW, SiteWatch, WorkerOrder,
};
pub use plan::{ADVANTAGE, AGE_SCHEDULE, MIN_SEEN, Plan, passive_income};
pub use progress::{
    AgeUpStep, age_up_step, first_production_building, is_tower, max_production, missing_trainer, next_production_building,
    production_targets, tower_kind, tower_target, wall_kind,
};
pub use view::{AiView, ArmyInfo, BuildingInfo, FieldInfo, KnownBuilding, Snapshot, TUNED_MAP_SIDE, WorkerInfo, WorkerTask, collect_snapshot};

/// Ticks between two thinks of one AI (0.5 s at 30 Hz).
pub const THINK_INTERVAL_TICKS: u64 = 15;
/// A placement not seen as a building after this many seconds failed.
pub const PENDING_TIMEOUT: f32 = 6.0;
/// Research, upgrade and similar orders are not repeated within this many seconds.
pub const RESEND_DELAY: f32 = 3.0;
/// A worker is not given a new job within this many seconds of the last one.
pub const REORDER_DELAY: f32 = 4.0;
/// Items the AI keeps in a production queue.
pub const MAX_QUEUE: usize = 2;
/// Alerts kept per player between thinks.
const MAX_ALERTS: usize = 16;

/// Adds the computer opponents: [`AiBrains`] and the think systems in `SimSet::Ai`.
pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AiBrains>()
            .add_systems(OnEnter(GameState::Playing), reset_brains)
            .add_systems(FixedUpdate, (collect_alerts, think).chain().in_set(SimSet::Ai));
    }
}

/// Is it `player`'s turn to think on `tick`?
pub fn think_due(tick: u64, player: PlayerId) -> bool {
    tick % THINK_INTERVAL_TICKS == (player.0 as u64 * 2) % THINK_INTERVAL_TICKS
}

fn reset_brains(mut brains: ResMut<AiBrains>) {
    *brains = AiBrains::default();
}

/// Queue "under attack" reports for AI victims, with the attacker's
/// position when it is still alive.
fn collect_alerts(
    mut reader: MessageReader<UnderAttack>,
    players: Res<Players>,
    clock: Res<SimClock>,
    positions: Query<&Position, Without<Dead>>,
    mut brains: ResMut<AiBrains>,
) {
    for m in reader.read() {
        let is_ai = players.try_get(m.victim_owner).is_some_and(|p| p.controller == Controller::Ai && !p.defeated);
        if !is_ai {
            continue;
        }
        let attacker = m.attacker.and_then(|e| positions.get(e).ok()).map_or(m.pos, |p| p.0);
        let alerts = &mut brains.memory_mut(m.victim_owner).alerts;
        if alerts.len() >= MAX_ALERTS {
            alerts.remove(0);
        }
        alerts.push(Alert { at: m.pos, attacker, attacker_owner: m.attacker_owner, time: clock.elapsed });
    }
}

#[allow(clippy::too_many_arguments)]
fn think(
    data: Res<Data>,
    players: Res<Players>,
    nav: Res<Nav>,
    spatial: Res<SpatialIndex>,
    map: Option<Res<MapInfo>>,
    clock: Res<SimClock>,
    (fog, outcome, revealed): (Res<FogOfWar>, Res<GameOutcome>, Option<Res<RevealedPlayers>>),
    (regions, features): (Option<Res<NavRegions>>, Option<Res<MapFeatures>>),
    mut inbox: ResMut<CommandInbox>,
    mut brains: ResMut<AiBrains>,
    units: view::UnitQuery,
    buildings: view::BuildingQuery,
    (targets, nodes): (view::TargetQuery, view::NodeQuery),
    mut snap: Local<Snapshot>,
    mut out: Local<Vec<PlayerCommand>>,
    mut seen: Local<Vec<f32>>,
) {
    if outcome.ended {
        return;
    }
    let data: &GameData = &data.0;
    let revealed = revealed.as_deref();
    for p in &players.list {
        if p.controller != Controller::Ai || p.defeated || !think_due(clock.tick, p.id) {
            continue;
        }
        collect_snapshot(p.id, data, &units, &buildings, &targets, &nodes, &mut snap);
        let mem = brains.memory_mut(p.id);
        update_sightings(mem, p, &players, &fog, revealed, data, &buildings);
        update_intel(mem, p, &players, &fog, data, &units, clock.elapsed, &mut seen);
        update_remnants(mem, p, &players, revealed, &units);
        let Some(view) = AiView::new(data, &players, p.id, &nav.0, &spatial, map.as_deref(), clock.elapsed, &fog, &snap) else { continue };
        let view = view.with_revealed(revealed).with_regions(regions.as_deref()).with_features(features.as_deref());
        think::run(&view, mem, &mut out);
        inbox.pending.append(&mut out);
    }
}

/// Fold the hostile armies in sight (workers aside) into the intel.
#[allow(clippy::too_many_arguments)]
fn update_intel(
    mem: &mut AiMemory,
    me: &PlayerState,
    players: &Players,
    fog: &FogOfWar,
    data: &GameData,
    units: &view::UnitQuery,
    now: f32,
    seen: &mut Vec<f32>,
) {
    seen.clear();
    seen.resize(data.units.len(), 0.0);
    for (_, unit, owner, pos, _, gatherer, _) in units {
        if gatherer.is_none() && players.hostile(me.id, owner.0) && fog.visible(me.team, pos.0) {
            seen[unit.kind.index()] += data.unit(unit.kind).pop as f32;
        }
    }
    mem.intel.observe(now, seen);
}

/// The AI's one intentional cheat, which ends hunts that could otherwise
/// last the rest of the match. A hostile player still in the game who lost
/// every production building is revealed (see `RevealedPlayers`: all its
/// buildings are seen and so known, see [`update_sightings`]); when none of
/// its buildings that keep it in the game is known either, it holds on only
/// through units, perhaps a single worker hiding in the fog. For such a
/// player, remember where every one of its units is, seen or not, so that
/// a wave goes straight for them (see `army::choose_target_for`) rather
/// than searching the map spot by spot.
fn update_remnants(mem: &mut AiMemory, me: &PlayerState, players: &Players, revealed: Option<&RevealedPlayers>, units: &view::UnitQuery) {
    mem.remnants.clear();
    let Some(revealed) = revealed else { return };
    let beaten: Vec<PlayerId> = revealed
        .list
        .iter()
        .copied()
        .filter(|p| {
            players.hostile(me.id, *p)
                && players.try_get(*p).is_some_and(|s| !s.defeated)
                && !mem.known.iter().any(|k| k.owner == *p)
        })
        .collect();
    if beaten.is_empty() {
        return;
    }
    for (entity, _, owner, pos, ..) in units {
        if beaten.contains(&owner.0) {
            mem.remnants.push((owner.0, entity, pos.0));
        }
    }
    mem.remnants.sort_unstable_by_key(|r| r.1);
}

/// Remember hostile buildings in sight, revealed ones included (see
/// `SeenBy`), that keep their owner in the game: walls, gates and plain
/// fields are not worth a wave (see `ext::economy::keeps_player_alive`).
/// Forget ones seen gone or whose owner is out. "Seen gone" is the test
/// `SeenBy` and af_fog's ghosts use: the team sees any part of the box
/// where the building stood (never just its centre: a siege engine that
/// outranges its sight brings a big building down without ever lighting
/// the middle), or the owner is revealed, so that everyone saw it go.
fn update_sightings(
    mem: &mut AiMemory,
    me: &PlayerState,
    players: &Players,
    fog: &FogOfWar,
    revealed: Option<&RevealedPlayers>,
    data: &GameData,
    buildings: &view::BuildingQuery,
) {
    let in_game = |p: PlayerId| players.try_get(p).is_some_and(|s| !s.defeated);
    for (entity, building, owner, pos, facing, .., seen) in buildings {
        // Before af_fog first tags a new building, fall back on its centre.
        let visible = seen.map_or_else(|| fog.visible(me.team, pos.0), |s| s.visible_to(me.team));
        let worth = || players.try_get(owner.0).is_some_and(|p| keeps_player_alive(data, p.faction, building.kind));
        if visible && players.hostile(me.id, owner.0) && in_game(owner.0) && worth() {
            mem.see(KnownBuilding { entity, owner: owner.0, kind: building.kind, pos: pos.0, facing: facing.0 });
        }
    }
    let seen_gone =
        |k: &KnownBuilding| revealed.is_some_and(|r| r.contains(k.owner)) || af_core::ext::fog::team_sees_box(fog, me.team, &k.obb(data));
    mem.known.retain(|k| in_game(k.owner) && (buildings.get(k.entity).is_ok() || !seen_gone(k)));
}

