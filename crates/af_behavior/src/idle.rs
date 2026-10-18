//! Units without orders: stances, retaliation, and the `Idle` marker.

use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::prelude::*;

use crate::brain::Brain;
use crate::fight::picked;
use crate::motion::{self, Engagement};
use crate::targeting::{aim, disarm, longest_reach, Seeker, Sight, TargetData};
use crate::tuning::*;

#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Idler {
    entity: Entity,
    owner: &'static Owner,
    pos: &'static Position,
    facing: &'static Facing,
    footprint: &'static Footprint,
    mobile: &'static Mobile,
    stats: &'static Stats,
    weapons: &'static Weapons,
    stance: &'static Stance,
    path: &'static PathState,
    orders: &'static Orders,
    brain: &'static mut Brain,
    goal: Option<&'static mut MoveGoal>,
    attack: Option<&'static AttackTarget>,
    last_attacker: Option<&'static LastAttacker>,
}

impl<'w> IdlerItem<'w, '_> {
    fn seeker(&self, tick: u64) -> Seeker<'w> {
        Seeker {
            entity: self.entity,
            owner: self.owner.0,
            pos: self.pos.0,
            obb: self.footprint.obb(self.pos.0, self.facing.0),
            flying: self.mobile.flying,
            stats: self.stats,
            weapons: self.weapons,
            unreachable: self.brain.unreachable,
            tick,
        }
    }
}

/// Stances of armed units with nothing to do:
/// * Aggressive: engage the best hostile in vision and chase it.
/// * Defensive: the same, but give up past [`DEFENSIVE_LEASH`] from the post
///   and walk back.
/// * StandGround: shoot what is in range, never move.
/// * Passive: never attack.
///
/// Non-passive units also hit back at whoever just hurt them, even while
/// walking to a rally point.
pub(crate) fn run_idle_stances(
    sight: Sight,
    clock: Res<SimClock>,
    mut commands: Commands,
    mut units: Query<Idler, (With<Unit>, Without<Dead>)>,
    targets: Query<TargetData, Without<Dead>>,
) {
    for mut u in &mut units {
        if !u.orders.queue.is_empty() || !u.brain.synced(u.orders) || u.brain.heal_target.is_some() {
            continue;
        }
        let stance = *u.stance;
        if u.weapons.slots.is_empty() || stance == Stance::Passive {
            if u.brain.engaged.take().is_some() {
                disarm(&mut commands, u.entity, u.attack);
                motion::halt(&mut commands, u.entity, &mut u.goal);
            }
            settle(&mut commands, &mut u);
            continue;
        }
        let me = u.seeker(clock.tick);
        let home = *u.brain.home.get_or_insert(u.pos.0);

        if u.brain.returning {
            if u.pos.0.distance(home) <= HOME_ARRIVE || motion::settled(&u.goal, u.path) {
                u.brain.returning = false;
                motion::halt(&mut commands, u.entity, &mut u.goal);
            } else {
                motion::go_to(&mut commands, u.entity, &mut u.goal, home, HOME_ARRIVE * 0.5);
                continue;
            }
        }

        // Keep fighting what we (or af_combat's auto-acquire) engaged.
        let current = picked(u.attack, u.brain.engaged);
        if let Some(t) = current {
            let leash = me.stats.vision * LEASH_VISION_MULT;
            let kept = targets.get(t).ok().and_then(|ti| sight.engageable(&me, &ti, leash)).filter(|en| match stance {
                Stance::StandGround => en.plan.stands(),
                Stance::Defensive => u.pos.0.distance(home) <= DEFENSIVE_LEASH,
                Stance::Aggressive | Stance::Passive => true,
            });
            let stuck = kept.is_some_and(|en| !en.plan.stands() && motion::engagement_failed(&u.goal, u.path, t));
            if let Some(en) = kept.filter(|_| !stuck) {
                engage(&mut commands, &mut u, stance, t, en);
                continue;
            }
            if stuck {
                u.brain.give_up_on(t, clock.tick);
            }
            u.brain.engaged = None;
            disarm(&mut commands, u.entity, u.attack);
            motion::halt(&mut commands, u.entity, &mut u.goal);
            if stance == Stance::Defensive && u.pos.0.distance(home) > HOME_ARRIVE {
                u.brain.returning = true;
                motion::go_to(&mut commands, u.entity, &mut u.goal, home, HOME_ARRIVE * 0.5);
                continue;
            }
            u.brain.home = Some(u.pos.0);
        }

        // Someone else's movement (a fresh unit heading to its rally point)
        // is not given up for a mere sighting, only to hit back.
        let walking = u.goal.is_some() && !motion::settled(&u.goal, u.path);
        settle(&mut commands, &mut u);
        if !u.brain.due(clock.tick, STANCE_TICKS) {
            continue;
        }

        let leash = me.stats.vision * LEASH_VISION_MULT;
        let recent = u.last_attacker.filter(|l| clock.elapsed - l.time <= RETALIATE_WINDOW).and_then(|l| l.entity).filter(|&t| !me.skips(t));
        let retaliate = recent.and_then(|t| {
            let en = targets.get(t).ok().and_then(|ti| sight.engageable(&me, &ti, leash))?;
            (stance != Stance::StandGround || en.plan.stands()).then_some((t, en))
        });
        let found = retaliate.or_else(|| {
            if walking {
                return None;
            }
            let (t, _) = match stance {
                Stance::StandGround => {
                    let radius = longest_reach(&sight.data, me.stats, me.weapons)? + me.obb.bounding_radius();
                    sight.best_hostile(&me, radius, true)?
                }
                _ => sight.best_hostile(&me, me.stats.vision, false)?,
            };
            targets.get(t).ok().and_then(|ti| sight.engageable(&me, &ti, leash)).map(|en| (t, en))
        });
        if let Some((t, en)) = found {
            if walking {
                // The walk is over: the post is where the unit was attacked.
                u.brain.home = Some(u.pos.0);
            }
            engage(&mut commands, &mut u, stance, t, en);
        }
    }
}

/// Fight `target`: chase it (or back off out of minimum range), except on
/// StandGround, which stands and shoots.
fn engage(commands: &mut Commands, u: &mut IdlerItem, stance: Stance, target: Entity, en: Engagement) {
    u.brain.engaged = Some(target);
    aim(commands, u.entity, u.attack, target, false);
    if stance == Stance::StandGround {
        motion::halt(commands, u.entity, &mut u.goal);
    } else {
        motion::pursue(commands, u.entity, &mut u.goal, u.path, u.pos.0, target, &en);
    }
}

/// Drop a goal af_movement has finished with; the unit's post is where it
/// ended up.
fn settle(commands: &mut Commands, u: &mut IdlerItem) {
    if u.goal.is_some() && motion::settled(&u.goal, u.path) {
        motion::halt(commands, u.entity, &mut u.goal);
        u.brain.home = Some(u.pos.0);
    }
}

/// `Idle` = no orders and nothing self-assigned (no chase, no heal, no walk).
#[allow(clippy::type_complexity)]
pub(crate) fn mark_idle(mut commands: Commands, units: Query<(Entity, &Orders, &Brain, Has<MoveGoal>, Has<Idle>), (With<Unit>, Without<Dead>)>) {
    for (entity, orders, brain, moving, idle) in &units {
        let now_idle = orders.queue.is_empty() && !moving && brain.engaged.is_none() && brain.heal_target.is_none() && !brain.returning;
        if now_idle && !idle {
            commands.entity(entity).try_insert(Idle);
        } else if !now_idle && idle {
            commands.entity(entity).try_remove::<Idle>();
        }
    }
}
