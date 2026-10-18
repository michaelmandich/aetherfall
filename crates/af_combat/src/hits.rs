//! Turning hits into `DamageMsg`s: bonus multipliers, splash with falloff,
//! and who a blast may touch.

use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::body::{Arena, Body, Layer};
use crate::fx::FxSpawner;

/// A zero-splash shell that lands without a direct hit still strikes a box
/// this close to the landing point (it may graze the target's edge).
const DIRECT_HIT_TOLERANCE: f32 = 0.3;

/// What a hit carries from its shooter.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Payload {
    pub weapon: WeaponId,
    pub source: Entity,
    /// What the shot was fired at (may be an ally under a forced attack).
    pub target: Entity,
    pub player: PlayerId,
    pub team: u8,
    /// `WeaponDef::damage x Stats::damage_mult` (one part of it for beams),
    /// before bonus multipliers and armour.
    pub damage: f32,
}

impl Payload {
    /// May an area effect (splash, pierce, cone, ring) of this hit touch `b`?
    /// Always the target it was fired at, even an ally or a neutral under a
    /// forced attack; otherwise never the shooter, and allies only with
    /// `friendly_fire`.
    pub fn affects(&self, def: &WeaponDef, b: &Body) -> bool {
        b.entity == self.target || (b.entity != self.source && b.owner.is_some() && (def.friendly_fire || b.team != self.team))
    }
}

/// Splash damage factor at edge distance `dist` from the impact: 100% at the
/// centre, falling linearly to 50% at the edge of `radius`.
pub(crate) fn splash_falloff(dist: f32, radius: f32) -> f32 {
    if radius <= 0.0 { 1.0 } else { 1.0 - 0.5 * (dist / radius).clamp(0.0, 1.0) }
}

/// Everything a weapon system writes: damage, gameplay randomness, effects.
#[derive(SystemParam)]
pub(crate) struct Effects<'w, 's> {
    damage: MessageWriter<'w, DamageMsg>,
    /// Gameplay randomness (scatter, spread that decides hits).
    pub rng: ResMut<'w, SimRng>,
    pub fx: FxSpawner<'w, 's>,
}

impl Effects<'_, '_> {
    /// Deal `scale` of the payload to `victim` (bonus multipliers applied).
    pub fn hit(&mut self, arena: &Arena, p: &Payload, victim: &Body, scale: f32) {
        let data = arena.data();
        let amount = p.damage * stats::bonus_multiplier(data, p.weapon, victim.tags) * scale;
        if amount > 0.0 {
            self.damage.write(DamageMsg {
                target: victim.entity,
                amount,
                dtype: data.weapon(p.weapon).damage_type,
                source: Some(p.source),
                source_player: Some(p.player),
            });
        }
    }

    /// A hit landing at `point` on `layer`: `direct` (if any) takes full
    /// damage; with splash, everything else within the radius that the hit
    /// [affects](Payload::affects) and the weapon's `Targets` allow takes
    /// falloff damage. Without splash and without a direct victim, the box
    /// at the point (if any) is struck. `exclude` were already hit.
    pub fn impact(&mut self, arena: &Arena, p: &Payload, point: Vec2, layer: Layer, direct: Option<&Body>, exclude: &[Entity]) {
        let def = arena.data().weapon(p.weapon);
        if let Some(d) = direct {
            self.hit(arena, p, d, 1.0);
        }
        let skip = |b: &Body| {
            direct.is_some_and(|d| d.entity == b.entity)
                || exclude.contains(&b.entity)
                || b.layer() != layer
                || !b.class().allowed_by(&def.targets)
                || !p.affects(def, b)
        };
        if def.splash > 0.0 {
            arena.for_each_near(point, def.splash, |b, dist| {
                if !skip(&b) {
                    self.hit(arena, p, &b, splash_falloff(dist, def.splash));
                }
            });
        } else if direct.is_none() {
            let mut best: Option<(f32, u64, Body)> = None;
            arena.for_each_near(point, DIRECT_HIT_TOLERANCE, |b, dist| {
                let key = (dist, b.entity.to_bits());
                if !skip(&b) && best.is_none_or(|(d, e, _)| key < (d, e)) {
                    best = Some((dist, b.entity.to_bits(), b));
                }
            });
            if let Some((_, _, b)) = best {
                self.hit(arena, p, &b, 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falloff_is_linear_to_half_at_the_edge() {
        assert_eq!(splash_falloff(0.0, 2.0), 1.0);
        assert!((splash_falloff(1.0, 2.0) - 0.75).abs() < 1e-6);
        assert_eq!(splash_falloff(2.0, 2.0), 0.5);
        assert_eq!(splash_falloff(5.0, 2.0), 0.5);
        assert_eq!(splash_falloff(0.3, 0.0), 1.0);
    }
}
