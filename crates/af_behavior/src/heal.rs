//! Healers mend injured allies: idle healers seek them out within vision
//! (those they can get within healing range of over the terrain, see
//! `reach`); healers holding, following, attacking, attack-moving or
//! patrolling mend whoever is already in range.

use af_core::ext::movement::NavRegions;
use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::brain::Brain;
use crate::motion::{self, Engagement, Plan};
use crate::reach::Reach;
use crate::tuning::*;

#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Medic {
    entity: Entity,
    unit: &'static Unit,
    owner: &'static Owner,
    pos: &'static Position,
    facing: &'static Facing,
    footprint: &'static Footprint,
    mobile: &'static Mobile,
    stats: &'static Stats,
    path: &'static PathState,
    orders: &'static Orders,
    brain: &'static mut Brain,
    goal: Option<&'static mut MoveGoal>,
}

#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Patient {
    health: &'static mut Health,
    pos: &'static Position,
    facing: &'static Facing,
    footprint: &'static Footprint,
    owner: &'static Owner,
}

/// One target at a time, lowest hit-point fraction first, and never more
/// than [`MAX_HEALERS_PER_TARGET`] healers on one patient. Healers go in
/// entity index order, not table order (which client inserts on sim entities
/// change): the first come take the room on a patient, and healers of one
/// patient add to its hit points one after another.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_healers(
    data: Res<Data>,
    players: Res<Players>,
    spatial: Res<SpatialIndex>,
    regions: Option<Res<NavRegions>>,
    clock: Res<SimClock>,
    mut commands: Commands,
    mut medics: Query<Medic, (With<Unit>, Without<Dead>)>,
    mut patients: Query<Patient, (With<Unit>, Without<Dead>)>,
    mut load: Local<HashMap<Entity, u8>>,
    mut order: Local<Vec<Entity>>,
) {
    load.clear();
    order.clear();
    for m in &medics {
        if let Some(t) = m.brain.heal_target {
            *load.entry(t).or_default() += 1;
        }
        if data.unit(m.unit.kind).healer.is_some() {
            order.push(m.entity);
        }
    }
    order.sort_unstable_by_key(|e| e.index_u32());
    for &e in order.iter() {
        let Ok(mut m) = medics.get_mut(e) else { continue };
        let Some(heal) = &data.unit(m.unit.kind).healer else { continue };
        if !m.brain.synced(m.orders) {
            continue;
        }
        m.brain.mending = false;
        // Idle healers roam to patients. Holding, following, fighting,
        // attack-moving and patrolling ones mend whoever is in reach without
        // leaving their order (fight.rs keeps steering them, and an
        // attack-moving healer waits by a patient that stands). A plain Move
        // only moves.
        let roam = match m.orders.current() {
            None => true,
            Some(Order::Hold | Order::Follow { .. } | Order::Attack { .. } | Order::AttackMove { .. } | Order::Patrol { .. }) => false,
            Some(_) => {
                release(&mut load, m.brain.heal_target.take());
                continue;
            }
        };
        let me = m.owner.0;
        let obb = m.footprint.obb(m.pos.0, m.facing.0);
        let reach = heal.range;
        let leash = m.stats.vision * LEASH_VISION_MULT;

        if let Some(t) = m.brain.heal_target {
            let gap = patients
                .get(t)
                .ok()
                .filter(|p| p.health.hp < p.health.max && players.allied(me, p.owner.0))
                .map(|p| obb.gap(&p.footprint.obb(p.pos.0, p.facing.0)));
            // A patient af_movement cannot get to is left alone for a while.
            let unreachable = roam && gap.is_some_and(|g| g > reach) && motion::engagement_failed(&m.goal, m.path, t);
            if unreachable {
                m.brain.unreachable.remember(t, clock.tick);
            }
            let keep = !unreachable && gap.is_some_and(|g| g <= if roam { leash } else { reach });
            if !keep {
                release(&mut load, m.brain.heal_target.take());
                if roam {
                    motion::halt(&mut commands, m.entity, &mut m.goal);
                }
            }
        }

        if m.brain.heal_target.is_none() && m.brain.due(clock.tick, STANCE_TICKS) {
            let tags = TagSet::from_tags(&heal.tags);
            let radius = if roam { m.stats.vision } else { reach + obb.bounding_radius() };
            let avoid = m.brain.unreachable;
            let ground = (roam && !m.mobile.flying).then(|| Reach::of(regions.as_deref(), m.pos.0, m.mobile));
            let mut best: Option<(f32, f32, Entity)> = None;
            spatial.for_each_near(m.pos.0, radius, |e| {
                let eligible = e.kind == SpatialKind::Unit
                    && e.entity != m.entity
                    && e.tags.0 & tags.0 != 0
                    && e.owner.is_some_and(|o| players.allied(me, o))
                    && load.get(&e.entity).copied().unwrap_or(0) < MAX_HEALERS_PER_TARGET
                    && !avoid.skips(e.entity, clock.tick)
                    && ground.as_ref().is_none_or(|g| g.within(&e.obb, reach));
                if !eligible {
                    return;
                }
                let dist = if roam { e.obb.distance_to_point(m.pos.0) } else { obb.gap(&e.obb) };
                if dist > if roam { radius } else { reach } {
                    return;
                }
                let Ok(p) = patients.get(e.entity) else { return };
                let fraction = p.health.fraction();
                if fraction < 1.0 && best.is_none_or(|(f, d, b)| (fraction, dist, e.entity) < (f, d, b)) {
                    best = Some((fraction, dist, e.entity));
                }
            });
            if let Some((_, _, t)) = best {
                m.brain.heal_target = Some(t);
                *load.entry(t).or_default() += 1;
            }
        }

        let Some(t) = m.brain.heal_target else { continue };
        let Ok(mut p) = patients.get_mut(t) else { continue };
        let gap = obb.gap(&p.footprint.obb(p.pos.0, p.facing.0));
        let in_reach = if roam {
            let en = Engagement { pos: p.pos.0, gap, plan: Plan::within(gap, reach) };
            motion::pursue(&mut commands, m.entity, &mut m.goal, m.path, m.pos.0, t, &en)
        } else {
            gap <= reach
        };
        if in_reach {
            p.health.hp = (p.health.hp + heal.rate * SIM_DT).min(p.health.max);
            m.brain.mending = true;
        }
    }
}

fn release(load: &mut HashMap<Entity, u8>, target: Option<Entity>) {
    if let Some(n) = target.and_then(|t| load.get_mut(&t)) {
        *n = n.saturating_sub(1);
    }
}
