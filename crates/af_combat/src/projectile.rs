//! Projectiles and beams: launched by the weapon cycle (`SimSet::Combat`),
//! moved and resolved into damage here (`SimSet::Projectiles`). Each one is
//! an entity with its `FxBox` (what af_render draws), the shooter's
//! [`Payload`] and one flight component.
//!
//! Each flight system moves its projectiles in entity index order, not table
//! order (which af_render's components on every `FxBox` change): their
//! damage, effects and despawns come in turn.

use std::f32::consts::PI;

use af_core::prelude::*;
use bevy::prelude::*;

use crate::body::{ground, lift, muzzle_point, Arena, Body, Layer};
use crate::fx::{fx_box, FxSpawner};
use crate::hits::{Effects, Payload};

/// Shortest flight of a lobbed shell (a shot at point-blank still arcs).
const MIN_FLIGHT: f32 = 0.1;
/// A bolt chasing a target faster than itself gives up after this long.
const BOLT_MAX_LIFE: f32 = 4.0;
/// Missiles leave the launcher pitched up by this much.
const LAUNCH_PITCH: f32 = 10.0 * PI / 180.0;
/// A missile strikes when it passes this close to its target's box.
const HOMING_HIT_MARGIN: f32 = 0.3;
/// Seconds between smoke puffs behind a missile.
const PUFF_EVERY: f32 = 0.08;
/// Half-width of a cannon shot when testing what it passes through.
const SHELL_RADIUS: f32 = 0.1;
/// Beams stay visible at least this long, and fade over it at the end.
const BEAM_FADE: f32 = 0.08;
const EPS: f32 = 1e-4;

/// Arrow, bullet, plasma bolt: re-aimed at its locked target every tick, so
/// it always hits if the target lives. It really flies straight (`at`);
/// `swerve` (from the volley spread) only bows the drawn path sideways, so
/// visual randomness never changes when it lands.
#[derive(Component, Debug)]
pub(crate) struct BoltFlight {
    target: Entity,
    aim: Vec3,
    at: Vec3,
    speed: f32,
    swerve: Vec3,
    start_dist: f32,
    life: f32,
}

impl BoltFlight {
    /// Moves the bolt one tick straight toward its aim; true once it is there.
    fn advance(&mut self) -> bool {
        let step = self.speed * SIM_DT;
        let to_aim = self.aim - self.at;
        let dist = to_aim.length();
        if dist <= step {
            self.at = self.aim;
            return true;
        }
        self.at += to_aim / dist * step;
        false
    }

    /// Where the bolt is drawn: its true position bowed by the swerve, which
    /// is zero at both ends of the flight.
    fn drawn(&self) -> Vec3 {
        let s = (1.0 - self.at.distance(self.aim) / self.start_dist).clamp(0.0, 1.0);
        self.at + self.swerve * s * (1.0 - s)
    }
}

/// Rocket or missile steering toward its target at a limited turn rate.
/// Without a target it flies on to the last aim point and bursts there, on
/// the layer the target was on (an air burst does not reach the ground).
#[derive(Component, Debug)]
pub(crate) struct HomingFlight {
    target: Option<Entity>,
    aim: Vec3,
    layer: Layer,
    dir: Vec3,
    speed: f32,
    turn_rate: f32,
    life: f32,
    puff_in: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Path {
    /// Parabola peaking `height` metres above the straight line (Arc).
    Lob { height: f32 },
    /// Accelerating fall onto the landing point (Bomb).
    Drop,
}

/// A shell or bomb on a fixed path to a ground point; it lands where it was
/// sent, whatever the target did meanwhile.
#[derive(Component, Debug)]
pub(crate) struct BallisticFlight {
    from: Vec3,
    to: Vec3,
    t: f32,
    duration: f32,
    path: Path,
}

impl BallisticFlight {
    /// Position and direction of travel at progress `s` in 0..=1.
    fn at(&self, s: f32) -> (Vec3, Vec3) {
        match self.path {
            Path::Lob { height } => {
                let pos = self.from.lerp(self.to, s) + Vec3::Y * 4.0 * height * s * (1.0 - s);
                let tangent = (self.to - self.from) + Vec3::Y * 4.0 * height * (1.0 - 2.0 * s);
                (pos, tangent)
            }
            Path::Drop => {
                let flat = ground(self.from).lerp(ground(self.to), s);
                let fall = self.to.y - self.from.y;
                let tangent = lift(ground(self.to) - ground(self.from), 2.0 * s * fall);
                (lift(flat, self.from.y + fall * s * s), tangent)
            }
        }
    }
}

/// Heavy flat shot to a ground point: damages up to `pierce_left` enemies it
/// passes through, stops in the next one, and bursts.
#[derive(Component, Debug)]
pub(crate) struct CannonFlight {
    at: Vec2,
    dir: Vec2,
    speed: f32,
    travelled: f32,
    distance: f32,
    y_from: f32,
    y_to: f32,
    pierce_left: u32,
    pierced: Vec<Entity>,
}

/// A beam held from its shooter's muzzle to the target for its duration,
/// dealing one part of its damage every `interval`. The entity's `Payload`
/// carries one part.
#[derive(Component, Debug)]
pub(crate) struct BeamLink {
    target: Option<Entity>,
    mount: [f32; 3],
    end: Vec3,
    parts_left: u32,
    interval: f32,
    until_part: f32,
    life: f32,
    width: f32,
}

/// Where a target moving at a constant `velocity` from `pos` will be when a
/// shell fired from `from` at ground `speed` arrives.
pub(crate) fn lead_point(from: Vec2, pos: Vec2, velocity: Vec2, speed: f32) -> Vec2 {
    let mut p = pos;
    for _ in 0..2 {
        p = pos + velocity * (from.distance(p) / speed.max(0.1));
    }
    p
}

fn look_size(look: &ProjectileLook) -> Vec3 {
    Vec3::from_array(look.size)
}

pub(crate) fn launch_bolt(out: &mut Effects, p: Payload, look: &ProjectileLook, from: Vec3, target: &Body, speed: f32, spread_deg: f32) {
    let aim = target.aim_point();
    let to = aim - from;
    let dist = to.length().max(0.01);
    let side = Vec3::new(-to.z, 0.0, to.x).normalize_or(Vec3::X);
    // Leaves the muzzle at the spread angle: d(drawn offset)/d(distance) is
    // |swerve| / dist at the start of the bow.
    let swerve = side * dist * af_geom::dmath::tan(out.fx.rand(-0.5, 0.5) * spread_deg.to_radians());
    let life = BOLT_MAX_LIFE.max(2.0 * dist / speed.max(0.1));
    let flight = BoltFlight { target: target.entity, aim, at: from, speed, swerve, start_dist: dist, life };
    out.fx.projectile(fx_box(from, to + swerve, look_size(look), look.color, look.glow), (p, flight));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn launch_homing(
    out: &mut Effects,
    p: Payload,
    look: &ProjectileLook,
    from: Vec3,
    target: &Body,
    speed: f32,
    turn_rate_deg: f32,
    spread_deg: f32,
    reach: f32,
) {
    let aim = target.aim_point();
    let yaw_off = out.rng.range(-0.5, 0.5) * spread_deg.to_radians();
    let dir = (Quat::from_rotation_y(yaw_off) * (aim - from).normalize_or(Vec3::Z)).rotate_towards(Vec3::Y, LAUNCH_PITCH);
    let flight = HomingFlight {
        target: Some(target.entity),
        aim,
        layer: target.layer(),
        dir,
        speed,
        turn_rate: turn_rate_deg.to_radians(),
        life: (3.0 * reach / speed.max(0.1)).clamp(1.5, 10.0),
        puff_in: PUFF_EVERY,
    };
    out.fx.projectile(fx_box(from, dir, look_size(look), look.color, look.glow), (p, flight));
}

/// Lob a shell (Arc) from `from` to the ground point `landing`.
pub(crate) fn launch_lob(out: &mut Effects, p: Payload, look: &ProjectileLook, from: Vec3, landing: Vec2, speed: f32, apex: f32) {
    let flat = ground(from).distance(landing);
    let flight = BallisticFlight {
        from,
        to: lift(landing, 0.0),
        t: 0.0,
        duration: (flat / speed.max(0.1)).max(MIN_FLIGHT),
        path: Path::Lob { height: apex * flat },
    };
    let (_, dir) = flight.at(0.0);
    out.fx.projectile(fx_box(from, dir, look_size(look), look.color, look.glow), (p, flight));
}

/// Release a bomb at `from`; it falls onto the ground point `landing`.
pub(crate) fn launch_bomb(out: &mut Effects, p: Payload, look: &ProjectileLook, from: Vec3, landing: Vec2, fall_time: f32) {
    let flight = BallisticFlight { from, to: lift(landing, 0.0), t: 0.0, duration: fall_time.max(MIN_FLIGHT), path: Path::Drop };
    out.fx.projectile(fx_box(from, Vec3::NEG_Y, look_size(look), look.color, look.glow), (p, flight));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn launch_cannon(out: &mut Effects, p: Payload, look: &ProjectileLook, from: Vec3, target: &Body, speed: f32, pierce: u32, spread_deg: f32) {
    let start = ground(from);
    let to = target.pos - start;
    let off = out.rng.range(-0.5, 0.5) * spread_deg.to_radians();
    let dir = af_geom::dmath::from_angle(off).rotate(to.normalize_or(af_geom::forward(0.0)));
    let distance = to.length().max(0.01);
    let y_to = target.aim_point().y;
    let flight = CannonFlight {
        at: start,
        dir,
        speed,
        travelled: 0.0,
        distance,
        y_from: from.y,
        y_to,
        pierce_left: pierce,
        pierced: Vec::with_capacity(pierce as usize),
    };
    let heading = lift(dir, (y_to - from.y) / distance);
    out.fx.projectile(fx_box(from, heading, look_size(look), look.color, look.glow), (p, flight));
}

/// Open a beam from `from` (the weapon mounted at `mount` on the shooter)
/// to `target`. `part` is the payload of one of its `ticks` damage parts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn start_beam(out: &mut Effects, part: Payload, look: &ProjectileLook, from: Vec3, mount: [f32; 3], target: &Body, duration: f32, width: f32, ticks: u32) {
    let ticks = ticks.max(1);
    let end = target.aim_point();
    let link = BeamLink {
        target: Some(target.entity),
        mount,
        end,
        parts_left: ticks,
        interval: duration / ticks as f32,
        until_part: 0.0,
        life: duration.max(BEAM_FADE),
        width,
    };
    let b = fx_box((from + end) * 0.5, end - from, Vec3::new(width, width, from.distance(end).max(0.01)), look.color, look.glow);
    out.fx.projectile(b, (part, link));
}

/// The flash of a hit: a ground burst for splash on the ground, else sparks.
fn impact_fx(fx: &mut FxSpawner, def: &WeaponDef, at: Vec3, layer: Layer) {
    let look = &def.projectile;
    if layer == Layer::Ground && def.splash > 0.0 {
        fx.burst(ground(at), def.splash, look.color, look.glow);
    } else {
        fx.sparks(at, look.color, look.glow, if def.splash > 0.0 { 6 } else { 2 }, 2.5);
    }
}

pub(crate) fn fly_bolts(
    arena: Arena,
    mut out: Effects,
    mut order: Local<Vec<Entity>>,
    mut bolts: Query<(Entity, &Payload, &mut BoltFlight, &mut FxBox)>,
) {
    af_core::by_entity_index(&mut order, bolts.iter().map(|(e, ..)| e));
    for &e in order.iter() {
        let Ok((e, p, mut f, mut b)) = bolts.get_mut(e) else { continue };
        let target = arena.body(f.target);
        if let Some(t) = &target {
            f.aim = t.aim_point();
        }
        f.life -= SIM_DT;
        if f.advance() {
            if let Some(t) = &target {
                out.impact(&arena, p, t.pos, t.layer(), Some(t), &[]);
                impact_fx(&mut out.fx, arena.data().weapon(p.weapon), f.aim, t.layer());
            }
            out.fx.commands.entity(e).try_despawn();
            continue;
        }
        if f.life <= 0.0 {
            out.fx.commands.entity(e).try_despawn();
            continue;
        }
        let drawn = f.drawn();
        b.dir = (drawn - b.pos).normalize_or(b.dir);
        b.pos = drawn;
    }
}

pub(crate) fn fly_homing(
    arena: Arena,
    mut out: Effects,
    mut order: Local<Vec<Entity>>,
    mut missiles: Query<(Entity, &Payload, &mut HomingFlight, &mut FxBox)>,
) {
    af_core::by_entity_index(&mut order, missiles.iter().map(|(e, ..)| e));
    for &e in order.iter() {
        let Ok((e, p, mut f, mut b)) = missiles.get_mut(e) else { continue };
        let target = f.target.and_then(|t| arena.body(t));
        match &target {
            Some(t) => {
                f.aim = t.aim_point();
                f.layer = t.layer();
            }
            None => f.target = None,
        }
        let def = arena.data().weapon(p.weapon);
        let step = f.speed * SIM_DT;
        if let Some(t) = target.filter(|t| t.contains(b.pos, HOMING_HIT_MARGIN) || b.pos.distance(f.aim) <= step) {
            out.impact(&arena, p, t.pos, t.layer(), Some(&t), &[]);
            impact_fx(&mut out.fx, def, b.pos, t.layer());
            out.fx.commands.entity(e).try_despawn();
            continue;
        }
        let burst = if b.pos.y <= 0.0 {
            Some((Layer::Ground, lift(ground(b.pos), 0.0)))
        } else if target.is_none() && b.pos.distance(f.aim) <= step {
            Some((f.layer, b.pos))
        } else {
            None
        };
        if let Some((layer, at)) = burst {
            out.impact(&arena, p, ground(at), layer, None, &[]);
            impact_fx(&mut out.fx, def, at, layer);
            out.fx.commands.entity(e).try_despawn();
            continue;
        }
        f.life -= SIM_DT;
        if f.life <= 0.0 {
            out.fx.sparks(b.pos, def.projectile.color, def.projectile.glow, 3, 2.0);
            out.fx.commands.entity(e).try_despawn();
            continue;
        }
        let desired = (f.aim - b.pos).normalize_or(f.dir);
        let turn = f.turn_rate * SIM_DT;
        f.dir = f.dir.rotate_towards(desired, turn).normalize_or(desired);
        b.pos += f.dir * step;
        b.dir = f.dir;
        f.puff_in -= SIM_DT;
        if f.puff_in <= 0.0 {
            f.puff_in += PUFF_EVERY;
            out.fx.puff(b.pos - f.dir * b.size.z * 0.5);
        }
    }
}

pub(crate) fn fly_ballistic(
    arena: Arena,
    mut out: Effects,
    mut order: Local<Vec<Entity>>,
    mut shells: Query<(Entity, &Payload, &mut BallisticFlight, &mut FxBox)>,
) {
    af_core::by_entity_index(&mut order, shells.iter().map(|(e, ..)| e));
    for &e in order.iter() {
        let Ok((e, p, mut f, mut b)) = shells.get_mut(e) else { continue };
        f.t += SIM_DT;
        let s = (f.t / f.duration).min(1.0);
        let (pos, dir) = f.at(s);
        b.pos = pos;
        b.dir = dir.normalize_or(Vec3::NEG_Y);
        if s >= 1.0 {
            let at = ground(f.to);
            out.impact(&arena, p, at, Layer::Ground, None, &[]);
            impact_fx(&mut out.fx, arena.data().weapon(p.weapon), f.to, Layer::Ground);
            out.fx.commands.entity(e).try_despawn();
        }
    }
}

pub(crate) fn fly_cannon_shots(
    arena: Arena,
    mut out: Effects,
    mut crossings: Local<Vec<(f32, Body)>>,
    mut order: Local<Vec<Entity>>,
    mut shots: Query<(Entity, &Payload, &mut CannonFlight, &mut FxBox)>,
) {
    af_core::by_entity_index(&mut order, shots.iter().map(|(e, ..)| e));
    for &e in order.iter() {
        let Ok((e, p, mut f, mut b)) = shots.get_mut(e) else { continue };
        let def = arena.data().weapon(p.weapon);
        let step = (f.speed * SIM_DT).min(f.distance - f.travelled).max(0.0);
        let (a, dir) = (f.at, f.dir);
        let end = a + dir * step;
        crossings.clear();
        arena.for_each_near((a + end) * 0.5, step * 0.5 + SHELL_RADIUS, |body, _| {
            if body.layer() == Layer::Ground
                && body.class().allowed_by(&def.targets)
                && p.affects(def, &body)
                && !f.pierced.contains(&body.entity)
                && body.obb.segment_distance(a, end) <= SHELL_RADIUS
            {
                let along = ((body.pos - a).dot(dir) - body.obb.support(dir)).clamp(0.0, step);
                crossings.push((along, body));
            }
        });
        crossings.sort_by(|x, y| x.0.total_cmp(&y.0).then_with(|| x.1.entity.cmp(&y.1.entity)));
        let mut stop = None;
        for (along, body) in crossings.iter() {
            if f.pierce_left == 0 {
                stop = Some((a + dir * *along, *body));
                break;
            }
            f.pierce_left -= 1;
            f.pierced.push(body.entity);
            out.hit(&arena, p, body, 1.0);
            out.fx.sparks(body.aim_point(), def.projectile.color, def.projectile.glow, 3, 3.0);
        }
        if let Some((point, body)) = stop {
            out.impact(&arena, p, point, Layer::Ground, Some(&body), &f.pierced);
            impact_fx(&mut out.fx, def, lift(point, b.pos.y), Layer::Ground);
            out.fx.commands.entity(e).try_despawn();
            continue;
        }
        f.at = end;
        f.travelled += step;
        let s = (f.travelled / f.distance).min(1.0);
        b.pos = lift(end, f.y_from + (f.y_to - f.y_from) * s);
        if f.travelled >= f.distance - EPS {
            out.impact(&arena, p, end, Layer::Ground, None, &f.pierced);
            impact_fx(&mut out.fx, def, lift(end, 0.0), Layer::Ground);
            out.fx.commands.entity(e).try_despawn();
        }
    }
}

pub(crate) fn update_beams(
    arena: Arena,
    mut out: Effects,
    mut order: Local<Vec<Entity>>,
    mut beams: Query<(Entity, &Payload, &mut BeamLink, &mut FxBox)>,
) {
    af_core::by_entity_index(&mut order, beams.iter().map(|(e, ..)| e));
    for &e in order.iter() {
        let Ok((e, p, mut beam, mut b)) = beams.get_mut(e) else { continue };
        let Some(source) = arena.body(p.source) else {
            out.fx.commands.entity(e).try_despawn();
            continue;
        };
        let start = muzzle_point(source.pos, source.obb.yaw, source.altitude, beam.mount);
        let target = beam.target.and_then(|t| arena.body(t));
        match &target {
            Some(t) => beam.end = t.aim_point(),
            None => {
                beam.target = None;
                beam.parts_left = 0;
                beam.life = beam.life.min(BEAM_FADE);
            }
        }
        if let Some(t) = &target {
            let look = &arena.data().weapon(p.weapon).projectile;
            while beam.parts_left > 0 && beam.until_part <= EPS {
                out.impact(&arena, p, t.pos, t.layer(), Some(t), &[]);
                out.fx.sparks(beam.end, look.color, look.glow, 2, 2.0);
                beam.parts_left -= 1;
                beam.until_part += beam.interval;
            }
        }
        b.pos = (start + beam.end) * 0.5;
        b.dir = (beam.end - start).normalize_or(Vec3::Z);
        b.size = Vec3::new(beam.width, beam.width, start.distance(beam.end).max(0.01));
        b.alpha = (beam.life / BEAM_FADE).clamp(0.0, 1.0);
        beam.until_part -= SIM_DT;
        beam.life -= SIM_DT;
        if beam.life <= 0.0 && beam.parts_left == 0 {
            out.fx.commands.entity(e).try_despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lead_point_predicts_a_moving_target() {
        // Target 10 m away running sideways at 2 m/s; shell at 10 m/s.
        let p = lead_point(Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 2.0), 10.0);
        let t = p.length() / 10.0;
        assert!((p - Vec2::new(10.0, 2.0 * t)).length() < 0.05, "{p:?}");
        // A still target is hit where it stands.
        assert_eq!(lead_point(Vec2::ZERO, Vec2::new(5.0, 5.0), Vec2::ZERO, 10.0), Vec2::new(5.0, 5.0));
    }

    #[test]
    fn bolt_swerve_bows_the_drawn_path_but_not_the_arrival() {
        let bolt = |swerve: Vec3| BoltFlight {
            target: Entity::PLACEHOLDER,
            aim: Vec3::new(12.3, 1.0, 0.0),
            at: Vec3::new(0.0, 1.0, 0.0),
            speed: 20.0,
            swerve,
            start_dist: 12.3,
            life: 4.0,
        };
        let ticks_to_land = |mut f: BoltFlight| {
            let mut widest: f32 = 0.0;
            let ticks = (1..100).find(|_| {
                let landed = f.advance();
                widest = widest.max(f.drawn().z.abs());
                landed
            });
            (ticks, widest)
        };
        let (straight, no_bow) = ticks_to_land(bolt(Vec3::ZERO));
        let (swerved, bow) = ticks_to_land(bolt(Vec3::new(0.0, 0.0, 2.0)));
        assert_eq!(straight, Some(19), "12.3 m at 20 m/s");
        assert_eq!(swerved, straight, "the swerve is visual only");
        assert_eq!(no_bow, 0.0);
        assert!(bow > 0.4 && bow <= 0.5, "bowed sideways by up to a quarter of the swerve: {bow}");
        let mut landed = bolt(Vec3::new(0.0, 0.0, 2.0));
        while !landed.advance() {}
        assert_eq!(landed.drawn(), landed.aim, "drawn on target when it lands");
    }

    #[test]
    fn lob_starts_and_lands_on_its_endpoints_and_peaks_midway() {
        let f = BallisticFlight { from: Vec3::new(0.0, 1.0, 0.0), to: Vec3::new(10.0, 0.0, 0.0), t: 0.0, duration: 1.0, path: Path::Lob { height: 3.0 } };
        assert_eq!(f.at(0.0).0, f.from);
        assert_eq!(f.at(1.0).0, f.to);
        assert!((f.at(0.5).0.y - 3.5).abs() < 1e-5);
        assert!(f.at(0.0).1.y > 0.0 && f.at(1.0).1.y < 0.0, "rises then falls");
    }

    #[test]
    fn bomb_drop_accelerates_onto_the_landing_point() {
        let f = BallisticFlight { from: Vec3::new(0.0, 12.0, 0.0), to: Vec3::new(1.0, 0.0, 0.0), t: 0.0, duration: 1.0, path: Path::Drop };
        assert_eq!(f.at(1.0).0, f.to);
        let first = 12.0 - f.at(0.5).0.y;
        let second = f.at(0.5).0.y - f.at(1.0).0.y;
        assert!(second > first, "falls faster in the second half");
    }
}
