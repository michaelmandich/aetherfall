//! Who each weapon shoots at, and turning to face it.
//!
//! af_behavior owns chasing and writes `AttackTarget`; this module only
//! validates it, picks a target per weapon slot (the ordered one when that
//! weapon can hit it, else the best hostile in reach when auto-fire is
//! allowed) and aims: body facing for fixed weapons, `aim_yaw` for turrets.
//! Weapons without `fire_on_move` only fire while their host stands still.

use std::f32::consts::PI;

use af_core::ext::combat::{in_weapon_range, weapon_reach, TargetClass};
use af_core::ext::fog::team_sees_box;
use af_core::prelude::*;
use bevy::prelude::*;

use crate::body::{Arena, Body, BodyKind};
use crate::turning::Room;

/// A fixed weapon fires when its host faces the target within this angle.
const FIRE_ARC: f32 = PI / 6.0;
/// How fast turrets and building weapons swing (radians per second).
const TURRET_TURN_RATE: f32 = 240.0 * PI / 180.0;
/// Auto-acquisition rescans every this many ticks (staggered by entity).
const SCAN_EVERY: u64 = 4;

/// How a weapon is pointed at its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Aim {
    /// Fixed to the body: the unit turns to face the target.
    Body,
    /// Swivels on its mount (`WeaponSlot::aim_yaw`): turrets, `fire_on_move`
    /// weapons and every building weapon.
    Turret,
    /// Needs no aim: Nova rings and bombs dropped straight down.
    Free,
}

pub(crate) fn aim_mode(def: &WeaponDef, on_building: bool) -> Aim {
    match def.kind {
        WeaponKind::Nova | WeaponKind::Bomb { .. } => Aim::Free,
        _ if on_building || def.fire_on_move => Aim::Turret,
        _ => Aim::Body,
    }
}

/// Is a weapon with this aim mode pointed at a target lying along world
/// yaw `want`?
pub(crate) fn aimed(mode: Aim, facing: f32, aim_yaw: f32, want: f32) -> bool {
    match mode {
        Aim::Free => true,
        Aim::Body => af_geom::wrap_angle(want - facing).abs() <= FIRE_ARC,
        Aim::Turret => af_geom::wrap_angle(want - facing - aim_yaw).abs() <= FIRE_ARC,
    }
}

/// Is the unit under way? Judged by intent, as af_movement judges it: it has
/// a goal it is waiting on a path for or following. Measured `Velocity` would
/// also count collision push-outs, so a jostled unit could never turn or fire.
pub(crate) fn under_way(goal: Option<&MoveGoal>, path: Option<&PathState>) -> bool {
    goal.is_some() && path.is_some_and(|p| matches!(p.status, PathStatus::Pending | PathStatus::Following))
}

/// Drops `AttackTarget`s whose target is gone, dead, not damageable, or no
/// longer hostile (unless the attack was forced).
pub(crate) fn validate_attack_targets(
    mut commands: Commands,
    arena: Arena,
    attackers: Query<(Entity, &Owner, &AttackTarget), Without<Dead>>,
) {
    for (entity, owner, attack) in &attackers {
        let team = arena.players.team(owner.0);
        let keep = attack.entity != entity
            && arena.body(attack.entity).is_some_and(|t| attack.forced || t.hostile_to(team));
        if !keep {
            commands.entity(entity).try_remove::<AttackTarget>();
        }
    }
}

/// Target priority for auto-fire, lower first: units that can shoot back,
/// other units, armed buildings, other buildings, walls and gates.
fn priority(data: &GameData, b: &Body, shooter: TargetClass) -> u8 {
    let threatens = |weapons: &[WeaponId]| weapons.iter().any(|w| shooter.allowed_by(&data.weapon(*w).targets));
    match b.kind {
        BodyKind::Unit(u) => u8::from(!threatens(&data.unit(u).weapon_ids)),
        BodyKind::Building(_) if b.tags.contains(Tag::Wall) || b.tags.contains(Tag::Gate) => 4,
        BodyKind::Building(k) if !b.under_construction && threatens(&data.building(k).weapon_ids) => 2,
        BodyKind::Building(_) => 3,
    }
}

/// What a shooter looks like to the acquisition logic.
struct Shooter {
    obb: Obb,
    team: u8,
    class: TargetClass,
}

/// The best visible auto-fire target for one weapon: lowest priority class,
/// then nearest; `current` is kept when nothing of a better class is in
/// reach, so equal targets do not flicker. Visibility tests the target's
/// box, as the fog contract requires (af_core::ext::fog): a big building
/// whose near wall is in sight counts as seen though its centre is dark.
fn best_target(arena: &Arena, fog: &FogOfWar, me: &Shooter, def: &WeaponDef, reach: f32, current: Option<Body>) -> Option<Body> {
    let current = current.filter(|c| team_sees_box(fog, me.team, &c.obb));
    let mut best: Option<((u8, f32, u64), Body)> = None;
    arena.for_each_in_reach(&me.obb, reach, |b, gap| {
        // The fog test last: it walks cells, the others are cheap.
        if !b.hostile_to(me.team) || gap < def.min_range || !b.class().allowed_by(&def.targets) || !team_sees_box(fog, me.team, &b.obb) {
            return;
        }
        let key = (priority(arena.data(), &b, me.class), gap, b.entity.to_bits());
        if best.is_none_or(|(k, _)| key < k) {
            best = Some((key, b));
        }
    });
    match (current, best) {
        (Some(c), Some((key, _))) if priority(arena.data(), &c, me.class) <= key.0 => Some(c),
        (_, Some((_, b))) => Some(b),
        (c, None) => c,
    }
}

/// Chooses each weapon slot's target for this tick (`WeaponSlot::target`
/// and `aim`). An attack already winding up keeps its target, unless its
/// host sets off with a weapon that cannot fire on the move: that cancels it.
#[allow(clippy::type_complexity)]
pub(crate) fn acquire_targets(
    arena: Arena,
    fog: Res<FogOfWar>,
    clock: Res<SimClock>,
    mut shooters: Query<
        (
            Entity,
            &Owner,
            &Position,
            &Facing,
            &Footprint,
            &Stats,
            &mut Weapons,
            Option<&AttackTarget>,
            Option<&MoveGoal>,
            Option<&PathState>,
            Option<&Mobile>,
            Option<&Stance>,
            Option<&Orders>,
            Has<Building>,
            Has<UnderConstruction>,
        ),
        Without<Dead>,
    >,
) {
    for (entity, owner, pos, facing, fp, stats, mut weapons, attack, goal, path, mobile, stance, orders, on_building, constructing) in &mut shooters {
        if weapons.slots.is_empty() {
            continue;
        }
        if constructing {
            // Foundations never fire.
            for slot in weapons.slots.iter_mut().filter(|s| s.target.is_some() || s.windup > 0.0) {
                slot.target = None;
                slot.windup = 0.0;
            }
            continue;
        }
        let me = Shooter {
            obb: fp.obb(pos.0, facing.0),
            team: arena.players.team(owner.0),
            class: TargetClass::of(on_building, mobile.is_some_and(|m| m.flying)),
        };
        let ordered = attack.and_then(|a| arena.body(a.entity));
        let moving = !on_building && under_way(goal, path);
        // A plain move order means "go there", not "fight on the way".
        let may_auto = on_building
            || (stance.is_some_and(|s| *s != Stance::Passive) && !matches!(orders.and_then(Orders::current), Some(Order::Move { .. })));
        let scan = (u64::from(entity.index_u32()) + clock.tick).is_multiple_of(SCAN_EVERY);

        for slot in weapons.slots.iter_mut() {
            let w = slot.weapon;
            let def = arena.data().weapon(w);
            let fire_now = !moving || def.fire_on_move;
            if slot.windup > 0.0 {
                if !fire_now {
                    // Like a dropped target: the attack is wasted and may
                    // start over as soon as the unit stops.
                    *slot = WeaponSlot { target: None, windup: 0.0, cooldown: 0.0, anim: 0.0, ..*slot };
                } else if slot.target.and_then(|t| arena.body(t)).is_none() {
                    slot.target = None;
                }
                continue;
            }
            let reach = weapon_reach(arena.data(), stats, w);
            let can_hit = |b: &Body| b.class().allowed_by(&def.targets) && in_weapon_range(arena.data(), stats, w, &me.obb, &b.obb);
            let chosen = if !fire_now {
                None
            } else if let Some(t) = ordered.filter(|t| can_hit(t)) {
                Some(t)
            } else if may_auto {
                let current = slot.target.and_then(|t| arena.body(t)).filter(|t| t.hostile_to(me.team) && can_hit(t));
                if scan { best_target(&arena, &fog, &me, def, reach, current) } else { current }
            } else {
                None
            };
            let target = chosen.map(|t| t.entity);
            if slot.target != target {
                slot.target = target;
            }
            if let Some(t) = chosen {
                slot.aim = t.pos;
            }
        }
    }
}

/// What a shooter's target looks like to aiming: where it is, whether it flies, whether it is a building.
type AimTargetData = (&'static Position, Option<&'static Mobile>, Has<Building>);

type ShooterData = (
    Entity,
    &'static Owner,
    &'static Position,
    &'static mut Facing,
    &'static Footprint,
    &'static mut Weapons,
    Option<&'static Mobile>,
    Option<&'static AttackTarget>,
    Option<&'static MoveGoal>,
    Option<&'static PathState>,
    Has<Building>,
);

/// Turns standing units to face their fixed weapons' target (or, with none
/// in reach, the target they were ordered to attack, so they are ready when
/// it closes in), as far as there is room to turn (see [`Room`]), then
/// swings turrets (`aim_yaw`, relative to the body) toward theirs; idle
/// turrets return to the front.
pub(crate) fn aim_weapons(
    data: Res<Data>,
    room: Room,
    mut turns: Local<Vec<(Entity, f32)>>,
    mut shooters: Query<ShooterData, (Without<Dead>, Without<UnderConstruction>)>,
    targets: Query<AimTargetData, (With<Health>, Without<Dead>)>,
) {
    let slot_yaw = |pos: Vec2, slot: &WeaponSlot| slot.target.and_then(|t| targets.get(t).ok()).map(|(p, ..)| af_geom::yaw_of(p.0 - pos));

    // The turns standing units want, each judged against where everyone
    // stands now, then made.
    turns.clear();
    for (entity, _, pos, facing, _, weapons, mobile, attack, goal, path, on_building) in &shooters {
        let Some(step) = mobile.filter(|_| !on_building && !under_way(goal, path)).map(|m| m.turn_rate * SIM_DT) else { continue };
        let ordered = attack.and_then(|a| targets.get(a.entity).ok()).map(|(p, m, b)| (af_geom::yaw_of(p.0 - pos.0), TargetClass::of(b, m.is_some_and(|m| m.flying))));
        let fixed = || weapons.slots.iter().filter(|s| aim_mode(data.weapon(s.weapon), false) == Aim::Body);
        let want = fixed().find_map(|s| slot_yaw(pos.0, s)).or_else(|| {
            let (yaw, class) = ordered?;
            fixed().any(|s| class.allowed_by(&data.weapon(s.weapon).targets)).then_some(yaw)
        });
        if let Some(want) = want {
            let yaw = af_geom::turn_toward(facing.0, want, step);
            if yaw != facing.0 {
                turns.push((entity, yaw));
            }
        }
    }
    // Each turn is judged against the turns already settled (in entity
    // order), so two neighbours turning into each other in the same tick
    // cannot each take the whole slack and together cut twice as deep.
    turns.sort_unstable_by_key(|t| t.0);
    for i in 0..turns.len() {
        let (entity, want) = turns[i];
        let Ok((_, owner, pos, facing, footprint, _, mobile, ..)) = shooters.get(entity) else { continue };
        if mobile.is_some_and(|m| m.flying) {
            continue;
        }
        let settled = &turns[..i];
        let unit_box = |e: Entity| {
            let (_, _, p, f, fp, ..) = shooters.get(e).ok().filter(|s| s.6.is_some_and(|m| !m.flying))?;
            let yaw = settled.binary_search_by_key(&e, |t| t.0).map_or(f.0, |j| settled[j].1);
            Some(fp.obb(p.0, yaw))
        };
        turns[i].1 = room.turn(entity, owner.0, footprint.obb(pos.0, facing.0), want, unit_box);
    }

    for (entity, _, pos, mut facing, _, mut weapons, _, _, _, _, on_building) in &mut shooters {
        if let Ok(i) = turns.binary_search_by_key(&entity, |t| t.0)
            && turns[i].1 != facing.0
        {
            facing.0 = turns[i].1;
        }
        let body = facing.0;
        for slot in weapons.slots.iter_mut() {
            if aim_mode(data.weapon(slot.weapon), on_building) != Aim::Turret {
                continue;
            }
            let rel = slot_yaw(pos.0, slot).map_or(0.0, |w| af_geom::wrap_angle(w - body));
            let swung = af_geom::turn_toward(slot.aim_yaw, rel, TURRET_TURN_RATE * SIM_DT);
            if swung != slot.aim_yaw {
                slot.aim_yaw = swung;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_weapons_need_the_body_to_face_the_target() {
        assert!(aimed(Aim::Body, 0.0, 0.0, 0.4));
        assert!(!aimed(Aim::Body, 0.0, 0.0, 0.7));
        // Wraps through PI.
        assert!(aimed(Aim::Body, 3.1, 0.0, -3.1));
    }

    #[test]
    fn turrets_aim_relative_to_the_body() {
        assert!(aimed(Aim::Turret, 1.0, 1.5, 2.5));
        assert!(!aimed(Aim::Turret, 1.0, 0.0, 2.5));
        assert!(aimed(Aim::Free, 0.0, 0.0, 3.0));
    }

    #[test]
    fn under_way_follows_movement_intent_not_speed() {
        let goal = MoveGoal::point(Vec2::new(10.0, 0.0), 0.5);
        let path = |status| PathState { status, goal_seq: goal.seq, ..PathState::default() };
        assert!(!under_way(None, Some(&PathState::default())), "no goal: standing, however it is pushed");
        assert!(under_way(Some(&goal), Some(&path(PathStatus::Following))));
        assert!(under_way(Some(&goal), Some(&path(PathStatus::Pending))));
        for status in [PathStatus::Idle, PathStatus::Arrived, PathStatus::Failed] {
            assert!(!under_way(Some(&goal), Some(&path(status))), "{status:?}");
        }
        assert!(!under_way(Some(&goal), None), "nothing can move it without path state");
    }
}
