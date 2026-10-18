//! Army decisions: what to train, how big the army should be, when to
//! attack, where.

use af_core::ext::economy::is_production_building;
use af_core::ext::fog::{team_explored, team_sees};
use af_core::prelude::*;
use bevy::prelude::*;

use crate::budget::Budget;
use crate::memory::{AiMemory, EnemyIntel};
use crate::view::{AiView, KnownBuilding};

/// The first wave is due after this many sim seconds for an all-out
/// rusher; a turtle's comes up to [`TURTLE_DELAY`] later.
pub const MIN_ATTACK_TIME: f32 = 360.0;
/// How much later than [`MIN_ATTACK_TIME`] a pure turtle (aggression 0)
/// is due to attack first; in between, by aggression.
pub const TURTLE_DELAY: f32 = 210.0;
/// After a wave leaves, the next is due this many seconds later.
pub const WAVE_INTERVAL: f32 = 180.0;
/// Once a wave is due, the army it waits for shrinks over this many
/// seconds from the full [`attack_threshold`] (the first wave over
/// [`FIRST_WAVE_PRESSURE_TIME`]) ...
pub const WAVE_PRESSURE_TIME: f32 = 150.0;
/// The first wave shrinks faster: a realm that was never seen to attack
/// gets nothing done, and waiting lets the neighbours strike first.
pub const FIRST_WAVE_PRESSURE_TIME: f32 = 60.0;
/// ... down to this share of it (the first wave) ...
pub const WAVE_FLOOR_SHARE: f32 = 0.4;
/// ... while later waves first wait for this share of it (once the realm
/// has shown it attacks, a wave that goes in small only feeds the enemy, and
/// an army that keeps its units grows), and reach the first wave's floor
/// only after as long again ...
pub const LATER_WAVE_FLOOR_SHARE: f32 = 0.75;
/// ... and never below this population.
pub const MIN_WAVE_POP: u32 = 10;

/// Share of the population the army should hold, by age (1..=5).
pub const ARMY_SHARE: [f32; 5] = [0.25, 0.33, 0.42, 0.5, 0.55];
/// Each age a unit lags behind the player's multiplies its weight by this:
/// newer lines take over from older ones.
pub const AGE_DECAY: f32 = 0.45;
/// Weight of units added because nothing preferred can be trained yet.
pub const FILLER_WEIGHT: f32 = 0.5;
/// What is spendable beyond the reserve and this much more counts as
/// surplus (see [`surplus`]).
pub const SURPLUS_KEEP: f32 = 200.0;
/// A unit paid entirely out of surplus counts as this much population
/// further below its share of the mix (see [`pick_unit`]).
pub const SURPLUS_PULL: f32 = 4.0;
/// Below this much remembered hostile army, the mix is not adjusted.
const MIN_INTEL_POP: f32 = 4.0;
/// A unit's weight is scaled by its effectiveness against what was seen,
/// relative to the mix's average, clamped to this range.
const COUNTER_RANGE: (f32, f32) = (0.3, 2.5);
/// Hostile flyers above this share of their army call for anti-air.
const AIR_ALERT: f32 = 0.08;
/// Share of the mix held by [building breakers](is_breaker) once hostile
/// fortifications (or many hostile buildings) are known ...
pub const SIEGE_SHARE: f32 = 0.15;
/// ... and while a wave razes a town, and for [`RAZE_MEMORY`] after: an
/// army of archers takes many minutes over a citadel that a few rams fell
/// in seconds.
pub const RAZE_SIEGE_SHARE: f32 = 0.35;
/// Seconds after razing during which the mix keeps [`RAZE_SIEGE_SHARE`].
pub const RAZE_MEMORY: f32 = 180.0;
/// While razing, the best breaker takes up to this much more of the mix
/// (see [`breaking_gap`]) ...
pub const RAZE_EXTRA_SHARE: f32 = 0.4;
/// ... when the army fielded cannot tear the known buildings down within
/// this many seconds: archers that count as breakers (they barely do) must
/// not keep the rams away.
pub const RAZE_TIME: f32 = 90.0;
/// A unit whose weapons take at least this much hp per second per
/// population from a building (bonus and armour applied) is a building
/// breaker: rams, catapults, bombards, giants, most melee infantry; not
/// archers or riflemen, whose missiles barely scratch stone.
pub const BREAKER_RATE: f32 = 3.0;
/// Known hostile towers that call for siege ...
const FORTS_FOR_SIEGE: usize = 2;
/// ... and so do this many known hostile buildings of any kind: soldiers
/// alone take long to raze a town.
const BUILDINGS_FOR_SIEGE: usize = 6;

/// Army population wanted alongside `workers` in `age`, so that the army
/// holds [`ARMY_SHARE`] of the population.
pub fn army_target(age: u8, workers: u32) -> u32 {
    let share = ARMY_SHARE[(age.clamp(1, MAX_AGE) - 1) as usize];
    (workers as f32 * share / (1.0 - share)).round() as u32
}

/// Faction units (not workers) trainable at some faction building now, in data order.
fn trainable_army(data: &GameData, me: &PlayerState) -> Vec<UnitTypeId> {
    let faction = data.faction(me.faction);
    let trainable = |u: UnitTypeId| faction.building_ids.iter().any(|b| rules::can_train(data, me, *b, u, false).is_ok());
    faction.unit_ids.iter().copied().filter(|u| data.unit(*u).gatherer.is_none() && trainable(*u)).collect()
}

/// The faction's preferred army for the player's current age: `ai.army`
/// weights, each unit mapped through researched line upgrades and faded by
/// [`AGE_DECAY`] per age it lags behind, keeping only units
/// [`rules::can_train`] allows at some faction building in this age.
/// Falls back to every trainable non-worker unit (weight 1) when the
/// profile names none that are available yet, and adds what a buildable
/// production building trains (weight [`FILLER_WEIGHT`]) when nothing in
/// the mix can be trained outside the main hall yet.
pub fn army_composition(data: &GameData, me: &PlayerState) -> Vec<(UnitTypeId, f32)> {
    let faction = data.faction(me.faction);
    let trainable = trainable_army(data, me);
    let mut comp: Vec<(UnitTypeId, f32)> = Vec::new();
    for (id, w) in &faction.ai.army {
        let Some(base) = data.unit_id(id) else { continue };
        let up = me.mods.upgraded_unit(base);
        let u = if faction.unit_ids.contains(&up) { up } else { base };
        if *w > 0.0 && trainable.contains(&u) {
            let lag = me.age.saturating_sub(data.unit(u).age) as i32;
            add_weight(&mut comp, u, *w * AGE_DECAY.powi(lag));
        }
    }
    if comp.is_empty() {
        for u in &trainable {
            if !data.unit(*u).has_tag(Tag::Scout) {
                add_weight(&mut comp, *u, 1.0);
            }
        }
    }
    // An army only the main hall (busy with workers) or a later age's
    // building could train is no army: add what the buildings of this age
    // can turn out.
    let buildable_trainer = |u: UnitTypeId| {
        faction.building_ids.iter().any(|b| {
            !data.building(*b).has_tag(Tag::TownCenter)
                && rules::can_build(data, me, *b, false).is_ok()
                && rules::can_train(data, me, *b, u, false).is_ok()
        })
    };
    if !comp.iter().any(|(u, _)| buildable_trainer(*u)) {
        for u in trainable {
            if !data.unit(u).has_tag(Tag::Scout) && buildable_trainer(u) {
                add_weight(&mut comp, u, FILLER_WEIGHT);
            }
        }
    }
    comp
}

/// The army to train: [`army_composition`] with each armed unit scaled by
/// how well it fights the hostile armies seen lately, plus the best anti-air
/// unit when enemy flyers outnumber what the mix can shoot at, and the best
/// [building breaker](best_breaker) up to [`SIEGE_SHARE`] once hostile
/// towers (or many hostile buildings) are known, up to [`RAZE_SIEGE_SHARE`]
/// against every known building while `razing` (see `AiMemory::razing_lately`),
/// and further (see [`RAZE_EXTRA_SHARE`]) while the army `fielded`
/// (population alive and queued, by unit type) lags behind.
pub fn army_mix(
    data: &GameData,
    me: &PlayerState,
    intel: &EnemyIntel,
    known: &[KnownBuilding],
    razing: bool,
    fielded: &[u32],
) -> Vec<(UnitTypeId, f32)> {
    let mut comp = army_composition(data, me);
    if comp.is_empty() {
        return comp;
    }
    let candidates: Vec<UnitTypeId> =
        trainable_army(data, me).into_iter().filter(|u| armed(data, *u) && !data.unit(*u).has_tag(Tag::Scout)).collect();
    let share_of = |comp: &[(UnitTypeId, f32)], f: &dyn Fn(UnitTypeId) -> bool| {
        let total: f32 = comp.iter().map(|c| c.1).sum();
        comp.iter().filter(|(u, _)| f(*u)).map(|c| c.1).sum::<f32>() / total.max(1e-6)
    };
    let enemy = intel.total();
    if enemy >= MIN_INTEL_POP {
        let eff: Vec<Option<f32>> = comp.iter().map(|(u, _)| armed(data, *u).then(|| effectiveness(data, *u, intel))).collect();
        let (sum, weight) =
            comp.iter().zip(&eff).filter_map(|((_, w), e)| e.map(|e| (e * w, *w))).fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
        if sum > 0.0 {
            let mean = sum / weight;
            for (c, e) in comp.iter_mut().zip(eff) {
                if let Some(e) = e {
                    c.1 *= (e / mean).clamp(COUNTER_RANGE.0, COUNTER_RANGE.1);
                }
            }
        }
        let flying = |u: UnitTypeId| data.unit(u).is_flying();
        let air = intel.pop_where(flying) / enemy;
        let covered = share_of(&comp, &|u| hits_air(data, u));
        if air > AIR_ALERT && covered < air {
            let vs_air = |u: UnitTypeId| effectiveness_where(data, u, intel, flying);
            let best = candidates
                .iter()
                .copied()
                .filter(|u| hits_air(data, *u))
                .max_by(|a, b| vs_air(*a).total_cmp(&vs_air(*b)).then(b.cmp(a)));
            if let Some(u) = best {
                let total: f32 = comp.iter().map(|c| c.1).sum();
                add_weight(&mut comp, u, total * (air - covered));
            }
        }
    }
    let forts = siege_targets(data, known, razing);
    let want = if razing { RAZE_SIEGE_SHARE } else { SIEGE_SHARE };
    let have = share_of(&comp, &|u| is_breaker_vs(data, u, &forts));
    let best = if me.age >= 2 { best_breaker(data, me, &candidates, &forts) } else { None };
    if let Some(u) = best {
        let total: f32 = comp.iter().map(|c| c.1).sum();
        // What makes breakers `want` of the mix ...
        let topped = if have < want { total * (want - have) / (1.0 - want) } else { 0.0 };
        // ... and, while razing, up to [`RAZE_EXTRA_SHARE`] more of the
        // best breaker, the further the army fielded falls short of tearing
        // the known buildings down within [`RAZE_TIME`].
        let extra = if razing { RAZE_EXTRA_SHARE * breaking_gap(data, fielded, known, &forts) } else { 0.0 };
        let w = topped + (total + topped) * extra / (1.0 - extra);
        if w > 0.0 {
            add_weight(&mut comp, u, w);
        }
    }
    comp
}

/// How far (0..=1) the army `fielded` (population by unit type) falls short
/// of the building damage that tears every `known` building down within
/// [`RAZE_TIME`], against the building types `kinds` on average.
pub fn breaking_gap(data: &GameData, fielded: &[u32], known: &[KnownBuilding], kinds: &[BuildingTypeId]) -> f32 {
    if kinds.is_empty() {
        return 0.0;
    }
    let need: f32 = known.iter().map(|k| data.building(k.kind).hp).sum::<f32>() / RAZE_TIME;
    let have: f32 = fielded
        .iter()
        .enumerate()
        .filter(|(_, p)| **p > 0)
        .map(|(i, p)| {
            let u = UnitTypeId(i as u16);
            let models = *p as f32 / data.unit(u).pop.max(1) as f32;
            models * kinds.iter().map(|b| siege_power(data, u, *b)).sum::<f32>() / kinds.len() as f32
        })
        .sum();
    if need <= 0.0 { 0.0 } else { (1.0 - have / need).clamp(0.0, 1.0) }
}

/// Building hp per second per population `u` takes from a building of type
/// `b` (see [`BREAKER_RATE`]).
pub fn breach_rate(data: &GameData, u: UnitTypeId, b: BuildingTypeId) -> f32 {
    siege_power(data, u, b) / data.unit(u).pop.max(1) as f32
}

/// Is `u` a building breaker against `b` (see [`BREAKER_RATE`])?
pub fn is_breaker(data: &GameData, u: UnitTypeId, b: BuildingTypeId) -> bool {
    breach_rate(data, u, b) >= BREAKER_RATE
}

/// The hostile building types the army's breakers are chosen against: the
/// known towers once `FORTS_FOR_SIEGE` are known, else every known
/// building type once `BUILDINGS_FOR_SIEGE` are known, or any while
/// `razing`; none (no breakers called for) otherwise.
pub fn siege_targets(data: &GameData, known: &[KnownBuilding], razing: bool) -> Vec<BuildingTypeId> {
    let towers: Vec<BuildingTypeId> = known.iter().filter(|k| data.building(k.kind).has_tag(Tag::Tower)).map(|k| k.kind).collect();
    if towers.len() >= FORTS_FOR_SIEGE {
        towers
    } else if known.len() >= BUILDINGS_FOR_SIEGE || razing {
        known.iter().map(|k| k.kind).collect()
    } else {
        Vec::new()
    }
}

/// Is `u` a building breaker against the building types `kinds` on average?
pub fn is_breaker_vs(data: &GameData, u: UnitTypeId, kinds: &[BuildingTypeId]) -> bool {
    !kinds.is_empty() && kinds.iter().map(|b| breach_rate(data, u, *b)).sum::<f32>() >= BREAKER_RATE * kinds.len() as f32
}

/// The building breaker among `candidates` that takes the most building hp
/// per second per resource spent from the building types `kinds` (a ram
/// before a trebuchet: two rams cost about one and hit harder).
pub fn best_breaker(data: &GameData, me: &PlayerState, candidates: &[UnitTypeId], kinds: &[BuildingTypeId]) -> Option<UnitTypeId> {
    let value = |u: UnitTypeId| {
        let d = data.unit(u);
        let power: f32 = kinds.iter().map(|b| siege_power(data, u, *b)).sum();
        power * d.squad.max(1) as f32 / stats::unit_cost(data, me, u).total().max(1.0)
    };
    candidates
        .iter()
        .copied()
        .filter(|u| is_breaker_vs(data, *u, kinds))
        .max_by(|a, b| value(*a).total_cmp(&value(*b)).then(b.cmp(a)))
}

fn armed(data: &GameData, u: UnitTypeId) -> bool {
    !data.unit(u).weapon_ids.is_empty()
}

/// Can any weapon of `u` shoot flyers?
pub fn hits_air(data: &GameData, u: UnitTypeId) -> bool {
    data.unit(u).weapon_ids.iter().any(|w| data.weapon(*w).targets.air)
}

/// Damage multiplier of `u`'s best weapon against a `target` unit: 0 when no
/// weapon may engage it, else the bonus against its tags times what its
/// armour lets through.
fn hit_factor(data: &GameData, u: UnitTypeId, target: UnitTypeId) -> f32 {
    let t = data.unit(target);
    let tags = stats::TagSet::from_tags(&t.tags);
    data.unit(u)
        .weapon_ids
        .iter()
        .filter_map(|id| {
            let w = data.weapon(*id);
            let engages = if t.is_flying() { w.targets.air } else { w.targets.ground };
            engages.then(|| stats::bonus_multiplier(data, *id, tags) * (1.0 - t.armor.get(w.damage_type).clamp(0.0, 0.9)))
        })
        .fold(0.0, f32::max)
}

/// How well `u` fights the remembered hostile army: its best weapon's
/// damage multiplier (bonus against the target's tags times what its armour
/// lets through; 0 for targets no weapon may engage), averaged by population.
pub fn effectiveness(data: &GameData, u: UnitTypeId, intel: &EnemyIntel) -> f32 {
    effectiveness_where(data, u, intel, |_| true)
}

/// [`effectiveness`] against the remembered hostile unit types passing `f`.
fn effectiveness_where(data: &GameData, u: UnitTypeId, intel: &EnemyIntel, f: impl Fn(UnitTypeId) -> bool) -> f32 {
    let (mut sum, mut pop) = (0.0, 0.0);
    for (i, p) in intel.pop.iter().enumerate().take(data.units.len()) {
        let t = UnitTypeId(i as u16);
        if *p > 0.0 && f(t) {
            sum += p * hit_factor(data, u, t);
            pop += p;
        }
    }
    if pop > 0.0 { sum / pop } else { 0.0 }
}

/// Damage per second `u`'s best building-capable weapon deals to a building
/// of type `b` (bonus and armour applied).
fn siege_power(data: &GameData, u: UnitTypeId, b: BuildingTypeId) -> f32 {
    let def = data.building(b);
    let tags = stats::TagSet::from_tags(&def.tags);
    data.unit(u)
        .weapon_ids
        .iter()
        .filter(|w| data.weapon(**w).targets.buildings)
        .map(|w| {
            let wd = data.weapon(*w);
            let through = 1.0 - def.armor.get(wd.damage_type).clamp(0.0, 0.9);
            wd.damage * wd.volley.max(1) as f32 * stats::bonus_multiplier(data, *w, tags) * through / wd.cooldown.max(0.1)
        })
        .fold(0.0, f32::max)
}

fn add_weight(comp: &mut Vec<(UnitTypeId, f32)>, u: UnitTypeId, w: f32) {
    match comp.iter_mut().find(|(k, _)| *k == u) {
        Some(e) => e.1 += w,
        None => comp.push((u, w)),
    }
}

/// Army population at which an attack is launched: `10 + 8 * age`, scaled
/// from x1.2 (aggression 0) down to x0.7 (aggression 1).
pub fn attack_threshold(age: u8, aggression: f32) -> u32 {
    let base = 10.0 + 8.0 * age.clamp(1, MAX_AGE) as f32;
    (base * (1.2 - 0.5 * aggression.clamp(0.0, 1.0))).round() as u32
}

/// When the first wave is due: six minutes for an all-out rusher, up to
/// three and a half minutes later for a turtle.
pub fn min_attack_time(aggression: f32) -> f32 {
    MIN_ATTACK_TIME + (1.0 - aggression.clamp(0.0, 1.0)) * TURTLE_DELAY
}

/// When the next wave is due: the faction's first-attack time, and
/// [`WAVE_INTERVAL`] after the last wave left (`last_wave`).
pub fn wave_due(aggression: f32, last_wave: Option<f32>) -> f32 {
    let first = min_attack_time(aggression);
    last_wave.map_or(first, |t| first.max(t + WAVE_INTERVAL))
}

/// Army population a wave due at `due` waits for at `time` (`u32::MAX`
/// before it is due): the full [`attack_threshold`] when it falls due,
/// shrinking over [`WAVE_PRESSURE_TIME`] to [`LATER_WAVE_FLOOR_SHARE`] of it
/// and over as long again to the [`wave_floor`] (the `first` wave straight
/// to the floor over [`FIRST_WAVE_PRESSURE_TIME`]), so an army that cannot
/// reach the full size still attacks, a little later and smaller, and waves
/// keep coming.
pub fn wave_size(time: f32, due: f32, age: u8, aggression: f32, first: bool) -> u32 {
    if time < due {
        return u32::MAX;
    }
    let full = attack_threshold(age, aggression) as f32;
    let share = if first {
        let late = ((time - due) / FIRST_WAVE_PRESSURE_TIME).clamp(0.0, 1.0);
        1.0 - (1.0 - WAVE_FLOOR_SHARE) * late
    } else {
        let late = ((time - due) / WAVE_PRESSURE_TIME).clamp(0.0, 2.0);
        if late <= 1.0 {
            1.0 - (1.0 - LATER_WAVE_FLOOR_SHARE) * late
        } else {
            LATER_WAVE_FLOOR_SHARE - (LATER_WAVE_FLOOR_SHARE - WAVE_FLOOR_SHARE) * (late - 1.0)
        }
    };
    ((full * share).round() as u32).max(MIN_WAVE_POP)
}

/// The smallest wave [`wave_size`] ever waits for in `age` (the first).
pub fn wave_floor(age: u8, aggression: f32) -> u32 {
    ((attack_threshold(age, aggression) as f32 * WAVE_FLOOR_SHARE).round() as u32).max(MIN_WAVE_POP)
}

/// Launch a wave with `gathered_pop` of army at the rally point at `time`?
pub fn should_attack(time: f32, gathered_pop: u32, age: u8, aggression: f32, last_wave: Option<f32>) -> bool {
    gathered_pop >= wave_size(time, wave_due(aggression, last_wave), age, aggression, last_wave.is_none())
}

/// What a budget may spend of each resource without touching the reserve
/// or the last [`SURPLUS_KEEP`] of it: what is piling up.
pub fn surplus(budget: &Budget) -> ResAmounts {
    let mut out = ResAmounts::ZERO;
    for i in 0..MAX_RESOURCES {
        out.0[i] = (budget.have.0[i] - budget.reserve.0[i] - SURPLUS_KEEP).max(0.0);
    }
    out
}

/// Share (0..=1) of `cost` that `surplus` covers.
pub fn surplus_share(cost: &ResAmounts, surplus: &ResAmounts) -> f32 {
    let total = cost.total();
    if total <= 0.0 {
        return 0.0;
    }
    cost.0.iter().zip(surplus.0.iter()).map(|(c, s)| c.max(0.0).min(*s)).sum::<f32>() / total
}

/// The unit the mix wants most at a building of type `at`: the composition
/// unit furthest below its share of the army (by population) that the
/// building may train and `pop_room` takes, whatever it costs, with how far
/// below its share it is (population; negative when above). `pop_by_kind`
/// = existing + queued pop per unit type.
pub fn wanted_unit(
    view: &AiView,
    comp: &[(UnitTypeId, f32)],
    pop_by_kind: &[u32],
    at: BuildingTypeId,
    pop_room: u32,
) -> Option<(UnitTypeId, f32)> {
    let (data, me) = (view.data, view.me);
    let total_w: f32 = comp.iter().map(|(_, w)| w).sum();
    if total_w <= 0.0 {
        return None;
    }
    let total_pop: u32 = comp.iter().map(|(u, _)| pop_by_kind.get(u.index()).copied().unwrap_or(0)).sum();
    let mut best: Option<(UnitTypeId, f32)> = None;
    for (u, w) in comp {
        let d = data.unit(*u);
        if d.pop * d.squad > pop_room || rules::can_train(data, me, at, *u, false).is_err() {
            continue;
        }
        let have = pop_by_kind.get(u.index()).copied().unwrap_or(0) as f32;
        let deficit = w / total_w * (total_pop + d.pop * d.squad) as f32 - have;
        if best.is_none_or(|(_, b)| deficit > b) {
            best = Some((*u, deficit));
        }
    }
    best
}

/// Pick the unit to queue at a building of type `at`: the composition
/// unit furthest below its share of the army (by population) that the
/// budget and population room allow, pulled up to [`SURPLUS_PULL`]
/// population further by how much of its price is [`surplus`], so
/// that what piles up (aether, iron...) is spent first. `pop_by_kind` =
/// existing + queued pop per unit type.
pub fn pick_unit(
    view: &AiView,
    comp: &[(UnitTypeId, f32)],
    pop_by_kind: &[u32],
    at: BuildingTypeId,
    budget: &Budget,
    reserve_share: f32,
    pop_room: u32,
) -> Option<UnitTypeId> {
    let (data, me) = (view.data, view.me);
    let total_w: f32 = comp.iter().map(|(_, w)| w).sum();
    if total_w <= 0.0 {
        return None;
    }
    let total_pop: u32 = comp.iter().map(|(u, _)| pop_by_kind.get(u.index()).copied().unwrap_or(0)).sum();
    let piled = surplus(budget);
    let mut best: Option<(f32, UnitTypeId)> = None;
    for (u, w) in comp {
        if rules::can_train(data, me, at, *u, false).is_err() {
            continue;
        }
        let d = data.unit(*u);
        let cost = stats::unit_cost(data, me, *u);
        if d.pop * d.squad > pop_room || !budget.affords(&cost, reserve_share) {
            continue;
        }
        let have = pop_by_kind.get(u.index()).copied().unwrap_or(0) as f32;
        let deficit = w / total_w * (total_pop + d.pop * d.squad) as f32 - have;
        let score = deficit + SURPLUS_PULL * surplus_share(&cost, &piled);
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, *u));
        }
    }
    best.map(|(_, u)| u)
}

/// What a building of type `at` that trains nothing of the mix turns out
/// for a short army (see `Think::train_army`): the cheapest armed unit
/// (scouts aside) it may train that the budget pays for keeping
/// `reserve_share` of the reserve and that `pop_room` takes.
pub fn filler_unit(view: &AiView, at: BuildingTypeId, budget: &Budget, reserve_share: f32, pop_room: u32) -> Option<UnitTypeId> {
    let (data, me) = (view.data, view.me);
    data.faction(me.faction)
        .unit_ids
        .iter()
        .copied()
        .filter(|u| {
            let d = data.unit(*u);
            d.gatherer.is_none() && !d.has_tag(Tag::Scout) && armed(data, *u) && d.pop * d.squad <= pop_room
        })
        .filter(|u| rules::can_train(data, me, at, *u, false).is_ok())
        .map(|u| (u, stats::unit_cost(data, me, u)))
        .filter(|(_, cost)| budget.affords(cost, reserve_share))
        .min_by(|a, b| a.1.total().total_cmp(&b.1.total()).then(a.0.cmp(&b.0)))
        .map(|(u, _)| u)
}

/// What an attack is aimed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aim {
    /// A known hostile building (revealed ones included): strike it there.
    Building(Entity),
    /// The player's start position, not visited yet: it is marked visited
    /// on arrival.
    Start,
    /// A spot where the player may hide, once none of its buildings is
    /// known and its start was found empty (see [`search_spot`]): it is
    /// marked searched on arrival and the hunt moves on.
    Search,
    /// One of the last units of a player that has no building left that
    /// keeps it in the game, aimed at wherever it is, seen or not: the AI's
    /// one intentional cheat (see `AiMemory::remnants`). The goal follows
    /// the unit; when it dies the hunt turns on the next.
    Remnant(Entity),
}

/// Where an attack is headed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Objective {
    pub player: PlayerId,
    pub aim: Aim,
    pub pos: Vec2,
}

impl Objective {
    /// The known building aimed at, if any.
    pub fn building(&self) -> Option<Entity> {
        match self.aim {
            Aim::Building(e) => Some(e),
            Aim::Start | Aim::Search | Aim::Remnant(_) => None,
        }
    }
}

/// A player who attacked us lately looks this much nearer as a target.
const GRUDGE_PULL: f32 = 0.7;
/// Each of the latest waves in a row aimed at a player makes it look this
/// much farther (as a share of the distance), so waves spread over neighbours.
const REPEAT_PUSH: f32 = 0.4;
/// The player already under attack looks this much nearer (finish the job).
const STAY_PULL: f32 = 0.6;
/// The player whose town a wave razed lately looks this much nearer to the
/// next waves, and earlier waves at it do not push them away: a crippled
/// realm is finished before a fresh one is attacked.
const FINISH_PULL: f32 = 0.5;
/// A blind search looks this much farther (metres) than a known target.
const SEARCH_DETOUR: f32 = 60.0;
/// A production building (see `ext::economy::is_production_building`)
/// looks this much nearer than other buildings of its owner: razing them
/// stops the reinforcements and reveals the rest.
const PRODUCTION_PULL: f32 = 0.6;
/// ... and this much nearer while an attack on its owner goes on (the
/// `current` target): a wave in the town tears down what trains the
/// defenders before the houses and camps.
const PRESS_PRODUCTION_PULL: f32 = 0.3;

/// Spacing of the grid of spots a hunt may search (metres).
pub const SEARCH_STEP: f32 = 24.0;
/// A searched spot is left alone for this long (seconds).
pub const SEARCH_MEMORY: f32 = 240.0;
/// A spot the team sees within this radius has nothing hidden to find.
const SEARCH_SIGHT: f32 = 6.0;
/// Spots the team has explored count this much farther (metres) than
/// unexplored ones: what was never seen hides more than what was.
const EXPLORED_PENALTY: f32 = 80.0;
/// How far from a grid spot its walkable stand-in may be (metres).
const SEARCH_SNAP: f32 = 8.0;

/// Where to send a new wave from `from`: see [`choose_target_for`].
pub fn choose_target(view: &AiView, mem: &AiMemory, from: Vec2) -> Option<Objective> {
    choose_target_for(view, mem, from, None)
}

/// Where to attack from `from`: the best hostile player's nearest known
/// building (production buildings first, see `PRODUCTION_PULL`; revealed
/// buildings are known), else, when it has no building left that keeps it
/// in the game, its unit nearest `from` wherever that is (the AI's one
/// cheat, see `AiMemory::remnants`), else that player's start position
/// unless already visited, else a [spot to search](search_spot) for it.
/// Players are ranked
/// by distance, pulled nearer by a recent attack on us (revenge), by being
/// the `current` target (finish what was started) or the one razed lately
/// (finish it off), and pushed away by the latest waves in a row aimed at
/// them (so waves spread over the neighbours, a realm being finished off
/// aside) and by having to be searched for.
/// Goals the army recently could not reach are skipped.
pub fn choose_target_for(view: &AiView, mem: &AiMemory, from: Vec2, current: Option<PlayerId>) -> Option<Objective> {
    let me = view.me.id;
    let mut best: Option<(f32, Objective)> = None;
    for p in view.players.iter() {
        if p.defeated || !view.players.hostile(me, p.id) {
            continue;
        }
        let production_pull = if current == Some(p.id) { PRESS_PRODUCTION_PULL } else { PRODUCTION_PULL };
        let reach = |k: &KnownBuilding| {
            let pull = if is_production_building(view.data, p.faction, k.kind) { production_pull } else { 1.0 };
            k.pos.distance(from) * pull
        };
        let known =
            mem.known.iter().filter(|k| k.owner == p.id && !mem.is_unreachable(k.pos)).min_by(|a, b| reach(a).total_cmp(&reach(b)));
        let remnant = mem
            .remnants
            .iter()
            .filter(|r| r.0 == p.id && !mem.is_unreachable(r.2) && !mem.lost_prey.iter().any(|l| l.0 == r.1))
            .min_by(|a, b| a.2.distance_squared(from).total_cmp(&b.2.distance_squared(from)).then(a.1.cmp(&b.1)));
        let candidate = match (known, remnant) {
            (Some(k), _) => Objective { player: p.id, aim: Aim::Building(k.entity), pos: k.pos },
            (None, Some(&(_, unit, pos))) => Objective { player: p.id, aim: Aim::Remnant(unit), pos },
            (None, None) if !mem.visited.contains(&p.id) && !mem.is_unreachable(p.start_pos) => {
                Objective { player: p.id, aim: Aim::Start, pos: p.start_pos }
            }
            (None, None) => match search_spot(view, mem, p, from) {
                Some(pos) => Objective { player: p.id, aim: Aim::Search, pos },
                None => continue,
            },
        };
        let mut d = candidate.pos.distance(from);
        if candidate.aim == Aim::Search {
            d += SEARCH_DETOUR;
        }
        if mem.holds_grudge(p.id, view.time) {
            d *= GRUDGE_PULL;
        }
        if current == Some(p.id) {
            d *= STAY_PULL;
        } else if mem.finishing(p.id, view.time) {
            d *= FINISH_PULL;
        } else {
            let repeats = mem.target_history.iter().rev().take_while(|t| **t == p.id).count();
            d *= 1.0 + REPEAT_PUSH * repeats as f32;
        }
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, c)| c)
}

/// Where to look for `target`, none of whose buildings is known and whose
/// start position was found empty: the best spot of a [`SEARCH_STEP`] grid
/// over the map that the team does not see now, was not searched lately
/// (see `AiMemory::searched`) and can be walked to, preferring spots near
/// the target's start, then spots never explored, then spots near `from`.
/// `None` when nothing is left to search (or the fog is off: then nothing
/// can hide).
pub fn search_spot(view: &AiView, mem: &AiMemory, target: &PlayerState, from: Vec2) -> Option<Vec2> {
    let (fog, team) = (view.fog, view.me.team);
    if !fog.enabled {
        return None;
    }
    let cells = (view.map_size / SEARCH_STEP).floor().max(Vec2::ONE);
    let mut spots: Vec<(f32, Vec2)> = Vec::new();
    for y in 0..cells.y as u32 {
        for x in 0..cells.x as u32 {
            let spot = (Vec2::new(x as f32, y as f32) + 0.5) * (view.map_size / cells);
            if mem.is_searched(spot) || mem.is_unreachable(spot) || team_sees(fog, team, spot, SEARCH_SIGHT) {
                continue;
            }
            let explored = team_explored(fog, team, spot, 0.0);
            let score = spot.distance(target.start_pos) + 0.5 * spot.distance(from) + if explored { EXPLORED_PENALTY } else { 0.0 };
            spots.push((score, spot));
        }
    }
    spots.sort_by(|a, b| a.0.total_cmp(&b.0));
    // The nav lookup is the costly part: only until a walkable spot turns up.
    // Goals are remembered where they were walked to, so check that point too.
    let mask = view.ground_mask();
    spots
        .into_iter()
        .filter_map(|(_, spot)| view.nav.nearest_open(spot, mask, 1.0, team, SEARCH_SNAP))
        .find(|p| !mem.is_unreachable(*p) && !mem.is_searched(*p))
}
