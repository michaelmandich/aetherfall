//! Cosmetic effect boxes: sparks, bursts, rings, beam segments, spray
//! particles, smoke and debris. af_render only draws `FxBox`; this module
//! moves, fades and despawns them.

use std::f32::consts::TAU;

use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::body::lift;

/// Above this many live cosmetic boxes new ones are skipped (projectiles,
/// which carry gameplay, are always spawned).
const FX_SOFT_CAP: usize = 4000;

/// Debris colour when the owner is unknown.
const NEUTRAL: Rgb = [0.5, 0.5, 0.5];

/// Randomness for visuals only, kept apart from `SimRng` so that adding or
/// removing a spark never changes gameplay.
#[derive(Resource)]
pub(crate) struct FxRng(SimRng);

impl Default for FxRng {
    fn default() -> Self {
        FxRng(SimRng::new(0xF1A5_4B0C))
    }
}

/// Live cosmetic boxes, counted each tick by [`animate_fx`].
#[derive(Resource, Default)]
pub(crate) struct FxCount(usize);

/// Lifetime and motion of a cosmetic box.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Fx {
    age: f32,
    life: f32,
    vel: Vec3,
    gravity: f32,
    /// Size change per second (bursts grow, sparks shrink).
    grow: Vec3,
    /// Opacity at birth; fades towards 0 over the life.
    alpha: f32,
    /// Comes to rest on the ground instead of falling through it.
    lands: bool,
}

impl Fx {
    fn new(life: f32) -> Fx {
        Fx { age: 0.0, life, vel: Vec3::ZERO, gravity: 0.0, grow: Vec3::ZERO, alpha: 1.0, lands: false }
    }
}

/// A fresh box (prev_pos = pos, so it does not streak on its first frame).
pub(crate) fn fx_box(pos: Vec3, dir: Vec3, size: Vec3, color: Rgb, glow: bool) -> FxBox {
    FxBox { pos, prev_pos: pos, dir: dir.normalize_or(Vec3::Z), size, color, glow, alpha: 1.0 }
}

/// Mix the owner's colour with a def's accent colour, as entities read.
pub(crate) fn owner_tint(player: Option<Rgb>, accent: Option<Rgb>) -> Rgb {
    let p = player.unwrap_or(NEUTRAL);
    match accent {
        Some(a) => [(p[0] + a[0]) * 0.5, (p[1] + a[1]) * 0.5, (p[2] + a[2]) * 0.5],
        None => p,
    }
}

/// Spawns effect boxes (and gives systems their `Commands`).
#[derive(SystemParam)]
pub(crate) struct FxSpawner<'w, 's> {
    pub commands: Commands<'w, 's>,
    rng: ResMut<'w, FxRng>,
    count: Res<'w, FxCount>,
}

impl FxSpawner<'_, '_> {
    /// Visual-only random number in `[lo, hi)`.
    pub fn rand(&mut self, lo: f32, hi: f32) -> f32 {
        self.rng.0.range(lo, hi)
    }

    /// A projectile's own box: always spawned, owned by the caller's flight
    /// component (moved and despawned by the projectile systems).
    pub fn projectile(&mut self, b: FxBox, flight: impl Bundle) -> Entity {
        self.commands.spawn((b, flight, DespawnOnExit(GameState::Playing))).id()
    }

    fn spawn(&mut self, b: FxBox, fx: Fx) {
        if self.count.0 < FX_SOFT_CAP {
            self.commands.spawn((b, fx, DespawnOnExit(GameState::Playing)));
        }
    }

    fn random_dir(&mut self, up_bias: f32) -> Vec3 {
        let a = self.rand(0.0, TAU);
        let up = self.rand(0.1, 1.0) + up_bias;
        Vec3::new(a.cos(), up, a.sin()).normalize_or(Vec3::Y)
    }

    /// Small fast boxes flung from a hit point.
    pub fn sparks(&mut self, at: Vec3, color: Rgb, glow: bool, count: u32, speed: f32) {
        for _ in 0..count {
            let dir = self.random_dir(0.2);
            let s = self.rand(0.05, 0.1);
            let mut fx = Fx::new(self.rand(0.15, 0.3));
            fx.vel = dir * speed * self.rand(0.5, 1.0);
            fx.gravity = 9.0;
            fx.grow = Vec3::splat(-s * 2.0);
            self.spawn(fx_box(at, dir, Vec3::new(s, s, s * 2.5), color, glow), fx);
        }
    }

    /// A flat flash on the ground that grows to `radius` and fades, plus
    /// a few sparks: shell, bomb and smash impacts.
    pub fn burst(&mut self, at: Vec2, radius: f32, color: Rgb, glow: bool) {
        let r = radius.max(0.3);
        let mut fx = Fx::new(0.35);
        fx.grow = Vec3::new(r * 4.0, r * 1.2, r * 4.0);
        // Rise at half the growth so the bottom stays on the ground.
        fx.vel = Vec3::Y * fx.grow.y * 0.5;
        fx.alpha = 0.8;
        let start = Vec3::new(r * 0.6, r * 0.3, r * 0.6);
        self.spawn(fx_box(lift(at, start.y * 0.5), Vec3::Z, start, color, glow), fx);
        self.sparks(lift(at, 0.2), color, glow, 3 + (r * 2.0) as u32, 3.0 + r * 2.0);
    }

    /// A short-lived stretched box from `a` to `b` (chain lightning arcs).
    pub fn segment(&mut self, a: Vec3, b: Vec3, width: f32, color: Rgb, glow: bool, life: f32) {
        let len = a.distance(b).max(0.01);
        let mut fx = Fx::new(life);
        fx.grow = Vec3::new(-width * 0.5 / life, -width * 0.5 / life, 0.0);
        self.spawn(fx_box((a + b) * 0.5, b - a, Vec3::new(width, width, len), color, glow), fx);
    }

    /// Particles for a Spray: `count` boxes fanned within `half_angle` of
    /// `dir` that fly out to `reach` and fade.
    pub fn spray(&mut self, origin: Vec3, dir: Vec2, half_angle: f32, reach: f32, look: &ProjectileLook, count: u32) {
        const LIFE: f32 = 0.35;
        for _ in 0..count {
            let a = self.rand(-half_angle, half_angle);
            let d2 = Vec2::from_angle(a).rotate(dir);
            let d = Vec3::new(d2.x, self.rand(-0.05, 0.15), d2.y).normalize_or(Vec3::Z);
            let mut fx = Fx::new(LIFE * self.rand(0.8, 1.0));
            fx.vel = d * reach / LIFE;
            let s = Vec3::from_array(look.size);
            fx.grow = s * 2.0;
            self.spawn(fx_box(origin, d, s, look.color, look.glow), fx);
        }
    }

    /// A flat ring of boxes around `center` expanding from radius `from` to
    /// `to` (Nova).
    pub fn ring(&mut self, center: Vec2, height: f32, from: f32, to: f32, look: &ProjectileLook) {
        const LIFE: f32 = 0.4;
        let n = ((TAU * to / 0.8).ceil() as u32).clamp(12, 40);
        let speed = (to - from).max(0.1) / LIFE;
        let size = Vec3::from_array(look.size);
        for i in 0..n {
            let a = i as f32 / n as f32 * TAU;
            let out = Vec2::from_angle(a);
            let mut fx = Fx::new(LIFE);
            fx.vel = lift(out, 0.0) * speed;
            // Tangential length keeps pace with the circumference.
            fx.grow = Vec3::new(0.0, 0.0, TAU * speed / n as f32);
            let tangent = lift(out.perp(), 0.0);
            let pos = lift(center + out * from, height + size.y * 0.5);
            self.spawn(fx_box(pos, tangent, Vec3::new(size.x, size.y, size.z.max(TAU * from / n as f32)), look.color, look.glow), fx);
        }
    }

    /// A grey smoke puff left behind a missile.
    pub fn puff(&mut self, at: Vec3) {
        let mut fx = Fx::new(0.5);
        fx.vel = Vec3::Y * 0.4;
        fx.grow = Vec3::splat(0.4);
        fx.alpha = 0.5;
        self.spawn(fx_box(at, Vec3::Z, Vec3::splat(0.12), [0.55, 0.55, 0.55], false), fx);
    }

    /// Chunks of a destroyed entity in its colour: `count` boxes of about
    /// `piece` metres thrown out of a box of half extents `half` at `center`.
    pub fn debris(&mut self, center: Vec3, half: Vec3, color: Rgb, count: u32, piece: f32) {
        for _ in 0..count {
            let offset = Vec3::new(self.rand(-1.0, 1.0), self.rand(-1.0, 1.0), self.rand(-1.0, 1.0)) * half;
            let dir = self.random_dir(0.6);
            let s = piece * self.rand(0.5, 1.2);
            let mut fx = Fx::new(self.rand(1.6, 2.6));
            fx.vel = dir * self.rand(2.0, 5.0) * (1.0 + piece);
            fx.gravity = 12.0;
            fx.lands = true;
            let size = Vec3::new(s, s * self.rand(0.5, 1.0), s * self.rand(0.7, 1.4));
            self.spawn(fx_box(center + offset, dir, size, color, false), fx);
        }
        // A dust cloud for anything sizeable.
        if half.x.max(half.z) > 1.0 {
            let mut fx = Fx::new(1.2);
            fx.grow = Vec3::new(half.x * 1.5, half.y, half.z * 1.5);
            fx.vel = Vec3::Y * fx.grow.y * 0.5;
            fx.alpha = 0.6;
            let base = lift(Vec2::new(center.x, center.z), half.y * 0.5);
            self.spawn(fx_box(base, Vec3::Z, Vec3::new(half.x * 2.0, half.y, half.z * 2.0), [0.6, 0.56, 0.5], false), fx);
        }
    }
}

/// Moves, grows, fades and despawns cosmetic boxes.
pub(crate) fn animate_fx(mut commands: Commands, mut count: ResMut<FxCount>, mut q: Query<(Entity, &mut FxBox, &mut Fx)>) {
    let mut live = 0;
    for (e, mut b, mut fx) in &mut q {
        fx.age += SIM_DT;
        if fx.age >= fx.life {
            commands.entity(e).try_despawn();
            continue;
        }
        live += 1;
        fx.vel.y -= fx.gravity * SIM_DT;
        b.pos += fx.vel * SIM_DT;
        if fx.lands && b.pos.y < b.size.y * 0.5 {
            b.pos.y = b.size.y * 0.5;
            fx.vel = Vec3::ZERO;
        }
        b.size = (b.size + fx.grow * SIM_DT).max(Vec3::splat(0.01));
        let k = fx.age / fx.life;
        b.alpha = fx.alpha * (1.0 - k * k);
    }
    count.0 = live;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_mixes_accent_with_owner() {
        assert_eq!(owner_tint(Some([1.0, 0.0, 0.0]), Some([0.0, 0.0, 1.0])), [0.5, 0.0, 0.5]);
        assert_eq!(owner_tint(Some([0.2, 0.3, 0.4]), None), [0.2, 0.3, 0.4]);
        assert_eq!(owner_tint(None, None), NEUTRAL);
    }

    #[test]
    fn fx_box_never_has_a_zero_axis() {
        let b = fx_box(Vec3::ONE, Vec3::ZERO, Vec3::ONE, [1.0; 3], false);
        assert_eq!(b.dir, Vec3::Z);
        assert_eq!(b.prev_pos, b.pos);
    }
}
