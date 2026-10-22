//! Attack, AttackMove, Patrol and Hold orders.

use af_core::ext::behavior::patrol_points;
use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::prelude::*;

use crate::brain::{Brain, Phase};
use crate::motion;
use crate::notice::notify;
use crate::targeting::{aim, disarm, longest_reach, Seeker, Sight, TargetData};
use crate::tuning::*;

#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Fighter {
    entity: Entity,
    owner: &'static Owner,
    pos: &'static Position,
    facing: &'static Facing,
    footprint: &'static Footprint,
    mobile: &'static Mobile,
    stats: &'static Stats,
    weapons: &'static Weapons,
    path: &'static PathState,
    orders: &'static mut Orders,
    brain: &'static mut Brain,
    goal: Option<&'static mut MoveGoal>,
    attack: Option<&'static AttackTarget>,
}

impl<'w> FighterItem<'w, '_> {
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

/// Units that have somewhere to walk to (a `MoveGoal`).
type Walking<'w, 's> = Query<'w, 's, Has<MoveGoal>, (With<Unit>, Without<Dead>)>;

/// Read-only lookups shared by the order handlers.
struct Ctx<'a> {
    sight: &'a Sight<'a, 'a>,
    targets: &'a Query<'a, 'a, TargetData, Without<Dead>>,
    walking: &'a Walking<'a, 'a>,
    tick: u64,
    now: f32,
}

pub(crate) fn run_fight_orders(
    sight: Sight,
    clock: Res<SimClock>,
    mut notices: ResMut<Notices>,
    mut commands: Commands,
    mut units: Query<Fighter, (With<Unit>, Without<Dead>)>,
    targets: Query<TargetData, Without<Dead>>,
    walking: Walking,
) {
    let ctx = Ctx { sight: &sight, targets: &targets, walking: &walking, tick: clock.tick, now: clock.elapsed };
    for mut u in &mut units {
        if !u.brain.synced(&u.orders) {
            continue;
        }
        match u.orders.current().copied() {
            Some(Order::Attack { target }) => attack(&ctx, &mut commands, &mut notices, &mut u, target),
            Some(Order::AttackMove { target }) => {
                if advance(&ctx, &mut commands, &mut u, target) {
                    u.orders.queue.pop_front();
                }
            }
            Some(Order::Patrol { b, .. }) => {
                // The route: this patrol and the ones queued straight behind
                // it, as a loop (two points: back and forth).
                let leg = patrol_leg(&u.orders, u.brain.patrol_leg, u.brain.patrol_to);
                let points = patrol_points(&u.orders.queue).count();
                let next = (leg + 1) % points;
                let leg_end = patrol_points(&u.orders.queue).nth(next).unwrap_or(b);
                (u.brain.patrol_leg, u.brain.patrol_to) = (leg, Some(leg_end));
                if advance(&ctx, &mut commands, &mut u, leg_end) {
                    (u.brain.patrol_leg, u.brain.patrol_to) = (next, None);
                }
            }
            Some(Order::Hold) => hold(&ctx, &mut commands, &mut u),
            _ => {}
        }
    }
}

/// The patrol leg to walk along the route at the front of `orders`
/// ([`patrol_points`]): `leg`, unless points shift-queued since have moved
/// `to`, the point it led to, along the route; then the leg that still
/// ends there (the first one on from `leg`), so the unit keeps on its way.
fn patrol_leg(orders: &Orders, leg: usize, to: Option<Vec2>) -> usize {
    let n = patrol_points(&orders.queue).count().max(1);
    let point = |i: usize| patrol_points(&orders.queue).nth(i % n);
    let leg = leg % n;
    match to {
        Some(to) if point(leg + 1) != Some(to) => (leg + 1..=leg + n).find(|&i| point(i) == Some(to)).map_or(leg, |i| (i - 1) % n),
        _ => leg,
    }
}

/// Forced attack: get into the firing band of a weapon that can hit the
/// target (chasing, or backing off out of minimum range), then stand and
/// let af_combat fire. Done when the target is gone.
fn attack(ctx: &Ctx, commands: &mut Commands, notices: &mut Notices, u: &mut FighterItem, target: Entity) {
    let Some(t) = ctx.targets.get(target).ok().filter(|t| t.mortal) else {
        u.orders.queue.pop_front();
        return;
    };
    let Some(en) = ctx.sight.assess(&u.seeker(ctx.tick), &t) else {
        notify(notices, u.owner.0, NoticeKind::Warning, "Cannot attack that target", Some(t.pos.0), ctx.now);
        u.orders.queue.pop_front();
        return;
    };
    // Failures count in a row: getting into the band ends a run of them.
    if en.plan.stands() {
        u.brain.failures = 0;
    } else if motion::engagement_failed(&u.goal, u.path, target) {
        u.brain.failures += 1;
        if u.brain.failures > MAX_PATH_FAILURES {
            u.orders.queue.pop_front();
            return;
        }
    }
    aim(commands, u.entity, u.attack, target, true);
    motion::pursue(commands, u.entity, &mut u.goal, u.path, u.pos.0, target, &en);
}

/// Attack-move towards `dest`: engage hostiles seen on the way (rescanning
/// every few ticks, preferring ones that fight back), resume afterwards.
/// A healer mending an ally that stands (fighting, or waiting) stays by it
/// rather than walking on alone; one walking with the army mends it on the
/// way. Returns true once at `dest` with nothing engaged.
fn advance(ctx: &Ctx, commands: &mut Commands, u: &mut FighterItem, dest: Vec2) -> bool {
    let leash = u.stats.vision * LEASH_VISION_MULT;
    let mut current = None;
    if let Some(t) = picked(u.attack, u.brain.engaged) {
        let me = u.seeker(ctx.tick);
        current = ctx.targets.get(t).ok().and_then(|ti| ctx.sight.engageable(&me, &ti, leash).map(|en| (t, en, ctx.sight.score(&me, &ti, en.gap))));
        if current.is_some_and(|(_, en, _)| !en.plan.stands() && motion::engagement_failed(&u.goal, u.path, t)) {
            u.brain.give_up_on(t, ctx.tick);
            current = None;
        }
        u.brain.engaged = current.map(|(t, _, _)| t);
        if current.is_none() && u.attack.is_some_and(|a| a.entity == t) {
            disarm(commands, u.entity, u.attack);
        }
    }
    if u.brain.phase == Phase::Fresh || u.brain.due(ctx.tick, SCAN_TICKS) {
        u.brain.phase = Phase::Approach;
        let me = u.seeker(ctx.tick);
        if let Some((candidate, score)) = ctx.sight.best_hostile(&me, u.stats.vision, false) {
            let better = current.is_none_or(|(t, _, s)| candidate != t && score + RETARGET_MARGIN < s);
            if better {
                let en = ctx.targets.get(candidate).ok().and_then(|ti| ctx.sight.engageable(&me, &ti, leash));
                if let Some(en) = en {
                    current = Some((candidate, en, score));
                    u.brain.engaged = Some(candidate);
                }
            }
        }
    }
    if let Some((t, en, _)) = current {
        aim(commands, u.entity, u.attack, t, false);
        motion::pursue(commands, u.entity, &mut u.goal, u.path, u.pos.0, t, &en);
        return false;
    }
    // heal.rs lets the patient go once it is mended or out of reach.
    if u.brain.heal_target.is_some_and(|p| ctx.walking.get(p).is_ok_and(|walking| !walking)) {
        motion::halt(commands, u.entity, &mut u.goal);
        return false;
    }
    motion::go_to(commands, u.entity, &mut u.goal, dest, MOVE_ARRIVE);
    u.pos.0.distance(dest) <= MOVE_ARRIVE || motion::settled(&u.goal, u.path)
}

/// The hostile currently engaged on the unit's own initiative. A target
/// af_combat switched to wins over our older pick, so the two never fight
/// over `AttackTarget`.
pub(crate) fn picked(attack: Option<&AttackTarget>, engaged: Option<Entity>) -> Option<Entity> {
    attack.filter(|a| !a.forced).map(|a| a.entity).or(engaged)
}

/// Hold position: never move; shoot the best hostile a weapon can already
/// fire at.
fn hold(ctx: &Ctx, commands: &mut Commands, u: &mut FighterItem) {
    motion::halt(commands, u.entity, &mut u.goal);
    let Some(radius) = longest_reach(&ctx.sight.data, u.stats, u.weapons) else { return };
    let me = u.seeker(ctx.tick);
    let in_range =
        |t: Entity| ctx.targets.get(t).ok().and_then(|ti| ctx.sight.engageable(&me, &ti, f32::MAX)).is_some_and(|en| en.plan.stands());
    // Keep what we (or af_combat) picked while it stays in range.
    let current = picked(u.attack, u.brain.engaged).filter(|&t| in_range(t));
    let fresh = u.brain.phase == Phase::Fresh;
    u.brain.phase = Phase::Work;
    let target = match current {
        Some(t) => Some(t),
        None if fresh || u.brain.due(ctx.tick, SCAN_TICKS) => {
            ctx.sight.best_hostile(&me, radius + me.obb.bounding_radius(), true).map(|(t, _)| t)
        }
        None => None,
    };
    u.brain.engaged = target;
    match target {
        Some(t) => aim(commands, u.entity, u.attack, t, false),
        None => disarm(commands, u.entity, u.attack),
    }
}
