//! Facts derived once per think from the view: where home is, how many
//! workers and soldiers there are, the age-up step and what to save for.

use af_core::prelude::af_geom::GOAL_SEARCH_RADIUS;
use af_core::prelude::*;
use bevy::prelude::*;

use crate::army::{army_mix, army_target, wave_due, wave_floor, wave_size};
use crate::budget::just_in_time_reserve;
use crate::economy::worker_target;
use crate::memory::AiMemory;
use crate::progress::{AgeUpStep, age_path_cost, age_up_step};
use crate::view::AiView;

/// Against the last enemy left, a due wave (after the first) goes as soon
/// as the smallest wave (see [`wave_floor`]) is ready once the army is this
/// many times the hostile army seen lately: a beaten realm gets no time to
/// recover ...
pub const ADVANTAGE: f32 = 1.6;
/// ... provided at least this much hostile army population was seen.
pub const MIN_SEEN: f32 = 4.0;
/// The army gathers this far in front of the main hall (toward the crater).
pub const RALLY_DISTANCE: f32 = 20.0;
/// The front faces the point this far (metres) along the way from home to
/// the main meteor site: past the rally point, round what bends the way.
pub const FRONT_LOOKAHEAD: f32 = 30.0;
/// Seconds an age (1..=4) may take before its age-up is behind schedule:
/// from then on investments and the army leave its saving alone (see
/// `Think::invest_share` and `Think::mil_share`). Tight on purpose, so that
/// the ages keep coming under pressure (about Banner by 5 min, Starfall by
/// 13, Engine by 23 and Ascendant by 33).
pub const AGE_SCHEDULE: [f32; 4] = [270.0, 340.0, 430.0, 480.0];

/// What one think knows about its own side before deciding anything.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The main hall (oldest complete town centre).
    pub hall: Option<Entity>,
    /// Main hall position (or the start position without one).
    pub home: Vec2,
    /// Radius of the main hall's footprint (0 without one).
    pub hall_radius: f32,
    /// Unit direction from home toward the middle of the map: along the way
    /// there (see [`front_from`]).
    pub front: Vec2,
    /// Army gathering point.
    pub rally: Vec2,
    pub worker_kind: UnitTypeId,
    pub workers: u32,
    pub workers_queued: u32,
    pub worker_target: u32,
    /// Population of the army (non-workers) alive.
    pub army_pop: u32,
    /// Army population wanted alongside the worker target (see [`army_target`]).
    pub army_target: u32,
    /// Army population (alive and queued) that training works toward out of
    /// the saving for the age-up: half the army target, and from the second
    /// age on at least the smallest wave ([`wave_floor`]), so that the waves
    /// keep coming.
    pub army_goal: u32,
    /// Population of army units waiting in production queues.
    pub army_queued: u32,
    /// Army population the next wave waits for now (`u32::MAX` while it is
    /// not due; see [`wave_size`] and [`ADVANTAGE`]).
    pub wave_size: u32,
    /// Population of every unit waiting in production queues.
    pub queued_pop: u32,
    /// Complete army production buildings (worker halls not counted).
    pub production: u32,
    /// Army to train now: the faction's preference for this age, adjusted
    /// to counter what the enemy fields, with building breakers to raze
    /// what it holds (see [`army_mix`]).
    pub comp: Vec<(UnitTypeId, f32)>,
    pub age_up: AgeUpStep,
    /// Recent purchase shortfalls (see `AiMemory::shortfall`).
    pub shortfall: ResAmounts,
    /// What the rest of the age-up path costs.
    pub age_cost: ResAmounts,
    /// Resources set aside for the age-up (see [`just_in_time_reserve`]).
    pub reserve: ResAmounts,
    /// Smoothed income per second.
    pub income: ResAmounts,
    /// What army training and building placements actually spent lately,
    /// per second (smoothed).
    pub army_spend: ResAmounts,
    pub build_spend: ResAmounts,
    /// Passive income per second: complete buildings' trickle (extractors on
    /// vents, shrines, deep mines) plus the techs' (see [`passive_income`]).
    pub passive: ResAmounts,
    /// The current age has lasted longer than [`AGE_SCHEDULE`] allows while
    /// its age-up is still to be bought.
    pub behind_schedule: bool,
    /// By BuildingTypeId: trains army units / trains the worker.
    pub trains_army: Vec<bool>,
    pub trains_worker: Vec<bool>,
    /// By UnitTypeId: army population alive plus queued.
    pub pop_by_kind: Vec<u32>,
}

impl Plan {
    /// Derive the plan for this think from the view and memory.
    pub fn new(view: &AiView, mem: &AiMemory) -> Plan {
        let (data, me, snap) = (view.data, view.me, view.snap);
        let worker_kind = view.worker_kind();

        let hall = snap
            .buildings
            .iter()
            .find(|b| b.complete && data.building(b.kind).has_tag(Tag::TownCenter))
            .or_else(|| snap.buildings.iter().find(|b| data.building(b.kind).has_tag(Tag::TownCenter)));
        let home = hall.map(|b| b.pos).unwrap_or_else(|| {
            if snap.buildings.is_empty() {
                me.start_pos
            } else {
                snap.buildings.iter().map(|b| b.pos).sum::<Vec2>() / snap.buildings.len() as f32
            }
        });
        let hall_radius = hall.map_or(0.0, |b| {
            let s = data.building(b.kind).size;
            Vec2::new(s[0], s[2]).length() * 0.5
        });
        let front = front_from(view, home);
        let mut wanted = view.clamp(home + front * RALLY_DISTANCE, 4.0);
        if !view.worker_reaches(home, wanted)
            && let Some(near) = view.regions.and_then(|r| {
                let (mobility, radius) = view.unit_class(worker_kind);
                r.nearest_reachable(home, wanted, mobility, radius)
            })
        {
            // Over the water from home: gather on the near bank.
            wanted = near;
        }
        let rally = view.nav.nearest_open(wanted, view.ground_mask(), 1.0, me.team, 15.0).unwrap_or(wanted);

        let mut trains_army = vec![false; data.buildings.len()];
        let mut trains_worker = vec![false; data.buildings.len()];
        for b in &view.faction().building_ids {
            for u in rules::train_menu(data, me, *b) {
                if u == worker_kind {
                    trains_worker[b.index()] = true;
                } else if data.unit(u).gatherer.is_none() {
                    trains_army[b.index()] = true;
                }
            }
        }

        let mut pop_by_kind = vec![0u32; data.units.len()];
        let mut army_pop = 0;
        for a in &snap.army {
            army_pop += a.pop;
            pop_by_kind[a.kind.index()] += a.pop;
        }
        let (mut queued_pop, mut workers_queued, mut production, mut army_queued) = (0, 0, 0, 0);
        for b in &snap.buildings {
            if b.complete && trains_army[b.kind.index()] && !trains_worker[b.kind.index()] {
                production += 1;
            }
            for q in snap.queue(b) {
                if let QueueKind::Unit(u) = q {
                    let d = data.unit(*u);
                    queued_pop += d.pop * d.squad;
                    if d.gatherer.is_some() {
                        workers_queued += d.squad;
                    } else {
                        pop_by_kind[u.index()] += d.pop * d.squad;
                        army_queued += d.pop * d.squad;
                    }
                }
            }
        }

        let age_up = age_up_step(view, &mem.pending);
        let age_cost = age_path_cost(view);
        // The next purchase on the path is set aside whole, unless it is the
        // last one; what the rest of the path needs is set aside just in
        // time for its slowest resource. The step is released again when
        // paid (see Think::age_up).
        let reserve = match age_up {
            AgeUpStep::Done | AgeUpStep::InProgress | AgeUpStep::Blocked => ResAmounts::ZERO,
            _ => {
                let step = step_cost(view, age_up, &age_cost);
                let mut rest = age_cost;
                let mut left = me.resources;
                for i in 0..MAX_RESOURCES {
                    rest.0[i] = (rest.0[i] - step.0[i]).max(0.0);
                    left.0[i] = (left.0[i] - step.0[i]).max(0.0);
                }
                let mut reserve = step;
                reserve.add_all(&just_in_time_reserve(&rest, &left, &mem.income));
                reserve
            }
        };
        let worker_target = worker_target(me.age);
        let army_target = army_target(me.age, worker_target);
        let aggression = view.aggression();
        let army_goal = if me.age >= 2 { (army_target / 2).max(wave_floor(me.age, aggression)) } else { army_target / 2 };
        let mut wave_size = wave_size(view.time, wave_due(aggression, mem.last_wave), me.age, aggression, mem.last_wave.is_none());
        let seen = mem.intel.total();
        let last_enemy = view.players.iter().filter(|p| !p.defeated && view.players.hostile(me.id, p.id)).count() == 1;
        if last_enemy && wave_size != u32::MAX && mem.last_wave.is_some() && seen >= MIN_SEEN && army_pop as f32 >= ADVANTAGE * seen {
            wave_size = wave_size.min(wave_floor(me.age, aggression));
        }
        let pending = !matches!(age_up, AgeUpStep::Done | AgeUpStep::InProgress | AgeUpStep::Blocked);
        let allowed = AGE_SCHEDULE.get(me.age.saturating_sub(1) as usize).copied().unwrap_or(f32::INFINITY);
        let behind_schedule = pending && mem.age_duration(view.time) > allowed;

        Plan {
            hall: hall.map(|b| b.entity),
            home,
            hall_radius,
            front,
            rally,
            worker_kind,
            workers: snap.workers.len() as u32,
            workers_queued,
            worker_target,
            army_pop,
            army_target,
            army_goal,
            army_queued,
            wave_size,
            queued_pop,
            production,
            comp: reachable_mix(view, home, army_mix(data, me, &mem.intel, &mem.known, mem.razing_lately(view.time), &pop_by_kind)),
            age_up,
            age_cost,
            shortfall: mem.shortfall,
            reserve,
            income: mem.income,
            army_spend: mem.army_spend,
            build_spend: mem.build_spend,
            passive: passive_income(view),
            behind_schedule,
            trains_army,
            trains_worker,
            pop_by_kind,
        }
    }
}

/// Unit direction from `home` toward the main meteor site (the contested
/// middle), along the way a worker walks there: toward the point
/// [`FRONT_LOOKAHEAD`] down the coarse route, so a base whose way to the
/// middle bends round a river or a range faces where its army will go,
/// not the water. Straight at the site without regions or a route.
pub fn front_from(view: &AiView, home: Vec2) -> Vec2 {
    let toward = (view.crater - home).normalize_or(Vec2::Y);
    let Some(regions) = view.regions else { return toward };
    let (mobility, radius) = view.unit_class(view.worker_kind());
    let Some(route) = regions.route(home, view.crater, mobility, radius) else { return toward };
    let ahead = route.point_at(FRONT_LOOKAHEAD.min(route.length()));
    (ahead - home).normalize_or(toward)
}

/// The army mix less the units no route leads for from `home` to the main
/// meteor site (siege on wheels where only shallows cross a river, giants
/// too wide for every pass), unless that would leave nothing. Flyers always
/// get there; without regions everything does.
pub(crate) fn reachable_mix(view: &AiView, home: Vec2, comp: Vec<(UnitTypeId, f32)>) -> Vec<(UnitTypeId, f32)> {
    if view.regions.is_none() {
        return comp;
    }
    let near = view.features.and_then(|f| f.sites.first()).map_or(GOAL_SEARCH_RADIUS, |s| s.radius.max(GOAL_SEARCH_RADIUS));
    let kept: Vec<(UnitTypeId, f32)> = comp
        .iter()
        .copied()
        .filter(|(u, _)| view.data.unit(*u).locomotion == MoveFlag::Flying || view.reaches(*u, home, view.crater, near))
        .collect();
    if kept.is_empty() { comp } else { kept }
}

/// Passive income per second: the trickle of every complete own building
/// plus the player's tech trickle. Gatherers are only planned for what it
/// does not bring in.
pub fn passive_income(view: &AiView) -> ResAmounts {
    let mut income = view.me.mods.trickle;
    for b in view.snap.buildings.iter().filter(|b| b.complete) {
        income.add_all(&view.data.building(b.kind).trickle_amounts);
    }
    income
}

/// What the next purchase on the age-up path costs: the step to take now,
/// or the one being saved for unless it is all that is left of the path
/// (`age_cost`), which the just-in-time reserve covers. Nothing while waiting.
fn step_cost(view: &AiView, step: AgeUpStep, age_cost: &ResAmounts) -> ResAmounts {
    let (data, me) = (view.data, view.me);
    match step {
        AgeUpStep::Save { cost } if cost != *age_cost => cost,
        AgeUpStep::Research { tech, .. } => data.tech(tech).cost.amounts,
        AgeUpStep::Upgrade { building } => view
            .snap
            .building(building)
            .and_then(|b| data.building(b.kind).upgrade.as_ref())
            .map_or(ResAmounts::ZERO, |u| u.cost.amounts),
        AgeUpStep::Build(kind) => stats::building_cost(data, me, kind),
        _ => ResAmounts::ZERO,
    }
}
