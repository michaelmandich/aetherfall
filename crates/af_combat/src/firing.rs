//! The weapon cycle and what each weapon kind does when it fires.
//!
//! Per slot: `cooldown` counts down from the start of an attack (so an
//! attack every `cooldown` seconds, the design's dps); an attack winds up for
//! `WeaponDef::windup` seconds (`anim` 0 -> 1 for af_render), then fires if
//! the target is still alive and in reach. Melee, Spray, Nova and Chain deal
//! their damage on the spot; the other kinds launch projectiles or a beam.

use af_core::ext::combat::{area_reach, edge_distance, weapon_reach};
use af_core::prelude::*;
use bevy::prelude::*;

use crate::body::{ground, lift, muzzle_point, Arena, Body};
use crate::hits::{Effects, Payload};
use crate::projectile::{launch_bolt, launch_bomb, launch_cannon, launch_homing, launch_lob, lead_point, start_beam};
use crate::targeting::{aim_mode, aimed};

/// A released attack still lands if the target strayed this far past reach
/// during the windup.
const RELEASE_LEEWAY: f32 = 0.25;
/// `anim` falls back from 1 to 0 at this rate after an attack (recoil).
const ANIM_RECOVERY: f32 = 4.0;
/// Seconds a chain lightning arc stays visible.
const CHAIN_ARC_LIFE: f32 = 0.2;
const EPS: f32 = 1e-4;

/// The firing entity, as the fire functions need it.
struct Shooter<'a> {
    entity: Entity,
    player: PlayerId,
    team: u8,
    pos: Vec2,
    facing: f32,
    altitude: f32,
    obb: Obb,
    stats: &'a Stats,
}

/// Reused buffers (no per-attack allocation).
#[derive(Default)]
pub(crate) struct Scratch {
    nearby: Vec<(f32, Body)>,
    chained: Vec<Entity>,
}

/// Advances every weapon slot: cooldown, windup, and firing.
#[allow(clippy::type_complexity)]
pub(crate) fn cycle_weapons(
    arena: Arena,
    mut out: Effects,
    mut scratch: Local<Scratch>,
    mut shooters: Query<
        (Entity, &Owner, &Position, &Facing, &Altitude, &Footprint, &Stats, &mut Weapons, Has<Building>),
        (Without<Dead>, Without<UnderConstruction>),
    >,
) {
    for (entity, owner, pos, facing, alt, fp, stats, mut weapons, on_building) in &mut shooters {
        if weapons.slots.is_empty() {
            continue;
        }
        let shooter = Shooter {
            entity,
            player: owner.0,
            team: arena.players.team(owner.0),
            pos: pos.0,
            facing: facing.0,
            altitude: alt.0,
            obb: fp.obb(pos.0, facing.0),
            stats,
        };
        for slot in weapons.slots.iter_mut() {
            step_slot(&arena, &mut out, &mut scratch, &shooter, slot, on_building);
        }
    }
}

fn step_slot(arena: &Arena, out: &mut Effects, scratch: &mut Scratch, shooter: &Shooter, slot: &mut WeaponSlot, on_building: bool) {
    let def = arena.data().weapon(slot.weapon);
    slot.cooldown = (slot.cooldown - SIM_DT).max(0.0);

    if slot.windup > 0.0 {
        slot.windup -= SIM_DT;
        if slot.windup > EPS {
            slot.anim = 1.0 - slot.windup / def.windup.max(EPS);
            return;
        }
        slot.windup = 0.0;
        let reach = weapon_reach(arena.data(), shooter.stats, slot.weapon) + RELEASE_LEEWAY;
        match slot.target.and_then(|t| arena.body(t)).filter(|t| edge_distance(&shooter.obb, &t.obb) <= reach) {
            Some(target) => {
                fire(arena, out, scratch, shooter, slot.weapon, &target);
                slot.anim = 1.0;
            }
            None => {
                // The target died or escaped: the swing is wasted, try again.
                slot.cooldown = 0.0;
                slot.anim = 0.0;
            }
        }
        return;
    }

    if slot.anim > 0.0 {
        slot.anim = (slot.anim - SIM_DT * ANIM_RECOVERY).max(0.0);
    }
    if slot.cooldown > EPS {
        return;
    }
    let Some(target) = slot.target.and_then(|t| arena.body(t)) else { return };
    let want = af_geom::yaw_of(target.pos - shooter.pos);
    if !aimed(aim_mode(def, on_building), shooter.facing, slot.aim_yaw, want) {
        return;
    }
    slot.cooldown = stats::weapon_cooldown(arena.data(), shooter.stats, slot.weapon);
    if def.windup > EPS {
        slot.windup = def.windup;
        slot.anim = 0.0;
    } else {
        fire(arena, out, scratch, shooter, slot.weapon, &target);
        slot.anim = 1.0;
    }
}

/// Release one attack of `weapon` at `target`.
fn fire(arena: &Arena, out: &mut Effects, scratch: &mut Scratch, shooter: &Shooter, weapon: WeaponId, target: &Body) {
    let def = arena.data().weapon(weapon);
    let p = Payload {
        weapon,
        source: shooter.entity,
        target: target.entity,
        player: shooter.player,
        team: shooter.team,
        damage: def.damage * shooter.stats.damage_mult,
    };
    let muzzle = muzzle_point(shooter.pos, shooter.facing, shooter.altitude, def.look.offset);
    let look = &def.projectile;
    match def.kind {
        WeaponKind::Melee { style, arc_deg, cleave } => melee(arena, out, scratch, shooter, def, &p, target, style, arc_deg, cleave),
        WeaponKind::Bolt { speed } => {
            for _ in 0..def.volley {
                launch_bolt(out, p, look, muzzle, target, speed, def.spread);
            }
        }
        WeaponKind::Arc { speed, apex } => {
            let lead = lead_point(ground(muzzle), target.pos, target.velocity, speed);
            for _ in 0..def.volley {
                let landing = lead + out.rng.in_disc(def.spread);
                launch_lob(out, p, look, muzzle, landing, speed, apex);
            }
        }
        WeaponKind::Cannon { speed, pierce } => {
            for _ in 0..def.volley {
                launch_cannon(out, p, look, muzzle, target, speed, pierce, def.spread);
            }
        }
        WeaponKind::Beam { duration, width, ticks } => {
            let part = Payload { damage: p.damage / ticks.max(1) as f32, ..p };
            for _ in 0..def.volley {
                start_beam(out, part, look, muzzle, def.look.offset, target, duration, width, ticks);
            }
        }
        WeaponKind::Bomb { fall_time } => {
            for _ in 0..def.volley {
                let landing = target.pos + out.rng.in_disc(def.spread);
                launch_bomb(out, p, look, muzzle, landing, fall_time);
            }
        }
        WeaponKind::Homing { speed, turn_rate_deg } => {
            let reach = weapon_reach(arena.data(), shooter.stats, weapon);
            for _ in 0..def.volley {
                launch_homing(out, p, look, muzzle, target, speed, turn_rate_deg, def.spread, reach);
            }
        }
        WeaponKind::Spray { cone_deg, particles } => spray(arena, out, shooter, def, &p, target, muzzle, cone_deg, particles),
        WeaponKind::Nova => nova(arena, out, shooter, def, &p, target),
        WeaponKind::Chain { jumps, jump_range } => chain(arena, out, &mut scratch.chained, def, &p, target, muzzle, jumps, jump_range),
    }
}

/// Does any part of `b`'s box lie within `half` radians of world yaw `yaw`
/// as seen from `origin`?
fn within_cone(origin: Vec2, yaw: f32, half: f32, b: &Body) -> bool {
    [b.obb.closest_point(origin), b.pos].into_iter().any(|p| {
        let d = p - origin;
        d.length_squared() < 1e-6 || af_geom::wrap_angle(af_geom::yaw_of(d) - yaw).abs() <= half
    })
}

/// Swing hits the target and up to `cleave` other enemies inside its arc;
/// Thrust hits the target alone; Smash hits the target and splashes around
/// the point of the blow.
#[allow(clippy::too_many_arguments)]
fn melee(
    arena: &Arena,
    out: &mut Effects,
    scratch: &mut Scratch,
    shooter: &Shooter,
    def: &WeaponDef,
    p: &Payload,
    target: &Body,
    style: MeleeStyle,
    arc_deg: f32,
    cleave: u32,
) {
    let contact = target.obb.closest_point(shooter.pos);
    let spark_height = target.altitude + target.height * 0.5;
    match style {
        MeleeStyle::Thrust => out.hit(arena, p, target, 1.0),
        MeleeStyle::Swing => {
            out.hit(arena, p, target, 1.0);
            let reach = weapon_reach(arena.data(), shooter.stats, p.weapon) + RELEASE_LEEWAY;
            let half = arc_deg.to_radians() * 0.5;
            let nearby = &mut scratch.nearby;
            nearby.clear();
            arena.for_each_in_reach(&shooter.obb, reach, |b, gap| {
                if b.entity != target.entity
                    && b.hostile_to(p.team)
                    && b.class().allowed_by(&def.targets)
                    && within_cone(shooter.pos, shooter.facing, half, &b)
                {
                    nearby.push((gap, b));
                }
            });
            nearby.sort_by(|x, y| x.0.total_cmp(&y.0).then_with(|| x.1.entity.cmp(&y.1.entity)));
            for (_, b) in nearby.iter().take(cleave as usize) {
                out.hit(arena, p, b, 1.0);
                out.fx.sparks(b.aim_point(), def.projectile.color, def.projectile.glow, 2, 2.5);
            }
        }
        MeleeStyle::Smash => {
            out.impact(arena, p, contact, target.layer(), Some(target), &[]);
            if def.splash > 0.0 {
                out.fx.burst(contact, def.splash, def.projectile.color, def.projectile.glow);
            }
        }
    }
    out.fx.sparks(lift(contact, spark_height), def.projectile.color, def.projectile.glow, 3, 2.5);
}

/// How far around the shooter an area attack must look: its fixed area, or
/// further when range techs let it aim at a target beyond that area.
fn area_search(arena: &Arena, shooter: &Shooter, weapon: WeaponId, area: f32) -> f32 {
    weapon_reach(arena.data(), shooter.stats, weapon).max(area) + RELEASE_LEEWAY
}

/// The target and every enemy inside the cone and its fixed length take
/// one hit; particles fly out.
#[allow(clippy::too_many_arguments)]
fn spray(arena: &Arena, out: &mut Effects, shooter: &Shooter, def: &WeaponDef, p: &Payload, target: &Body, muzzle: Vec3, cone_deg: f32, particles: u32) {
    let area = area_reach(def);
    let dir = (target.pos - shooter.pos).normalize_or(af_geom::forward(shooter.facing));
    let yaw = af_geom::yaw_of(dir);
    let half = cone_deg.to_radians() * 0.5;
    arena.for_each_in_reach(&shooter.obb, area_search(arena, shooter, p.weapon, area), |b, gap| {
        let caught = b.entity == target.entity
            || (gap <= area && p.affects(def, &b) && b.class().allowed_by(&def.targets) && within_cone(shooter.pos, yaw, half, &b));
        if caught {
            out.hit(arena, p, &b, 1.0);
        }
    });
    out.fx.spray(muzzle, dir, half, area, &def.projectile, particles);
}

/// The target and every enemy within the fixed radius of the shooter's box
/// take one hit; a ring of boxes expands to the edge of the area.
fn nova(arena: &Arena, out: &mut Effects, shooter: &Shooter, def: &WeaponDef, p: &Payload, target: &Body) {
    let area = area_reach(def);
    arena.for_each_in_reach(&shooter.obb, area_search(arena, shooter, p.weapon, area), |b, gap| {
        if b.entity == target.entity || (gap <= area && p.affects(def, &b) && b.class().allowed_by(&def.targets)) {
            out.hit(arena, p, &b, 1.0);
        }
    });
    let edge = shooter.obb.half.max_element();
    out.fx.ring(shooter.pos, shooter.altitude, edge, edge + area, &def.projectile);
}

/// Hit the target, then jump to the nearest enemy not yet hit within
/// `jump_range` (edge to edge) of the last one, up to `jumps` times, every
/// jump dealing the full hit: the schema documents no falloff and the
/// content is priced on full damage per jump.
#[allow(clippy::too_many_arguments)]
fn chain(arena: &Arena, out: &mut Effects, chained: &mut Vec<Entity>, def: &WeaponDef, p: &Payload, target: &Body, muzzle: Vec3, jumps: u32, jump_range: f32) {
    let look = &def.projectile;
    let width = look.size[0];
    chained.clear();
    chained.push(target.entity);
    out.hit(arena, p, target, 1.0);
    out.fx.segment(muzzle, target.aim_point(), width, look.color, look.glow, CHAIN_ARC_LIFE);
    let mut prev = *target;
    for _ in 0..jumps {
        let mut next: Option<((f32, u64), Body)> = None;
        arena.for_each_in_reach(&prev.obb, jump_range, |b, gap| {
            let key = (gap, b.entity.to_bits());
            if b.entity != p.source
                && !chained.contains(&b.entity)
                && b.hostile_to(p.team)
                && b.class().allowed_by(&def.targets)
                && next.is_none_or(|(k, _)| key < k)
            {
                next = Some((key, b));
            }
        });
        let Some((_, b)) = next else { break };
        out.hit(arena, p, &b, 1.0);
        out.fx.segment(prev.aim_point(), b.aim_point(), width, look.color, look.glow, CHAIN_ARC_LIFE);
        chained.push(b.entity);
        prev = b;
    }
}
