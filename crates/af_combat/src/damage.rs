//! Applying damage: armour, shields, hp, deaths, "under attack" alerts,
//! regeneration and death debris (`SimSet::Damage`).

use af_core::prelude::*;
use bevy::prelude::*;

use crate::fx::{owner_tint, FxSpawner};

/// One `UnderAttack` per victim owner per this many seconds...
const ALERT_COOLDOWN: f32 = 5.0;
/// ...within this many metres of an earlier alert.
const ALERT_RADIUS: f32 = 30.0;

/// Recent `UnderAttack` alerts: (victim owner, where, when).
#[derive(Resource, Default)]
pub(crate) struct AlertLog {
    recent: Vec<(PlayerId, Vec2, f32)>,
}

impl AlertLog {
    /// Record an alert unless `owner` had one near `pos` recently.
    fn admit(&mut self, owner: PlayerId, pos: Vec2, now: f32) -> bool {
        let quiet = !self.recent.iter().any(|(p, at, _)| *p == owner && at.distance(pos) <= ALERT_RADIUS);
        if quiet {
            self.recent.push((owner, pos, now));
        }
        quiet
    }

    /// Forget alerts older than the cooldown, and any stamped in the future
    /// (left over from before a `SimClock` reset).
    fn expire(&mut self, now: f32) {
        self.recent.retain(|(_, _, t)| (0.0..ALERT_COOLDOWN).contains(&(now - *t)));
    }
}

/// Split `amount` (after armour) between a shield and hp. The shield
/// absorbs first, except the `energy_resist` fraction of Energy damage
/// that passes straight through. Returns the damage left for hp.
fn absorb(shield: &mut Shield, amount: f32, dtype: DamageType) -> f32 {
    shield.since_hit = 0.0;
    let bypass = if dtype == DamageType::Energy { amount * shield.energy_resist.clamp(0.0, 1.0) } else { 0.0 };
    let absorbed = (amount - bypass).clamp(0.0, shield.hp.max(0.0));
    shield.hp -= absorbed;
    amount - absorbed
}

/// Applies every `DamageMsg`: armour (`stats::mitigate`), then shield, then
/// hp; records `LastAttacker`; writes throttled `UnderAttack`. At hp <= 0 the
/// victim dies exactly once: `Dead`, `EntityDied` (with the killer) and the
/// players' loss/kill counters.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn apply_damage(
    clock: Res<SimClock>,
    mut players: ResMut<Players>,
    mut log: ResMut<AlertLog>,
    mut commands: Commands,
    mut incoming: MessageReader<DamageMsg>,
    mut died: MessageWriter<EntityDied>,
    mut alerts: MessageWriter<UnderAttack>,
    mut victims: Query<
        (&mut Health, Option<&mut Shield>, &Stats, Option<&Owner>, &Position, Option<&mut LastAttacker>, Option<&Unit>, Option<&Building>),
        (Without<Dead>, Or<(With<Unit>, With<Building>)>),
    >,
    positions: Query<&Position>,
) {
    let now = clock.elapsed;
    log.expire(now);
    for msg in incoming.read() {
        if msg.amount <= 0.0 {
            continue;
        }
        let Ok((mut health, shield, stats, owner, pos, last, unit, building)) = victims.get_mut(msg.target) else {
            continue;
        };
        let Some(kind) = unit.map(|u| EntityKind::Unit(u.kind)).or(building.map(|b| EntityKind::Building(b.kind))) else {
            continue;
        };
        if health.hp <= 0.0 {
            // Already killed by an earlier message this tick.
            continue;
        }
        let mut amount = stats::mitigate(msg.amount, msg.dtype, &stats.armor);
        if let Some(mut shield) = shield {
            amount = absorb(&mut shield, amount, msg.dtype);
        }
        health.hp -= amount;

        let from = msg.source.and_then(|s| positions.get(s).ok()).map_or(pos.0, |p| p.0);
        let record = LastAttacker { entity: msg.source, player: msg.source_player, pos: from, time: now };
        match last {
            Some(mut l) => *l = record,
            None => {
                commands.entity(msg.target).try_insert(record);
            }
        }

        let owner = owner.map(|o| o.0);
        if let Some(o) = owner {
            let hostile = msg.source_player.is_none_or(|s| players.hostile(s, o));
            if hostile && log.admit(o, pos.0, now) {
                alerts.write(UnderAttack { victim: msg.target, victim_owner: o, attacker: msg.source, attacker_owner: msg.source_player, pos: pos.0 });
            }
        }

        if health.hp > 0.0 {
            continue;
        }
        health.hp = 0.0;
        commands.entity(msg.target).try_insert(Dead);
        died.write(EntityDied { entity: msg.target, owner, kind, pos: pos.0, killer: msg.source_player });
        let credited = msg.source_player.filter(|k| owner.is_none_or(|o| players.hostile(*k, o)));
        if let Some(p) = owner.and_then(|o| players.list.get_mut(o.index())) {
            match kind {
                EntityKind::Building(_) => p.stats.buildings_lost += 1,
                _ => p.stats.units_lost += 1,
            }
        }
        if let Some(p) = credited.and_then(|k| players.list.get_mut(k.index())) {
            p.stats.kills += 1;
        }
    }
}

/// Hp regeneration (`Stats::regen`) and shield recharge after its delay.
/// Keeps `Shield::max` in step with `Stats::shield_max` (techs).
#[allow(clippy::type_complexity)]
pub(crate) fn regenerate(mut q: Query<(&Stats, &mut Health, Option<&mut Shield>), (Without<Dead>, Without<UnderConstruction>)>) {
    for (stats, mut health, shield) in &mut q {
        if stats.regen > 0.0 && health.hp > 0.0 && health.hp < health.max {
            health.hp = (health.hp + stats.regen * SIM_DT).min(health.max);
        }
        let Some(mut s) = shield else { continue };
        s.since_hit += SIM_DT;
        if s.max != stats.shield_max {
            s.max = stats.shield_max;
            s.hp = s.hp.min(s.max);
        }
        if s.since_hit >= s.delay && s.hp < s.max {
            s.hp = (s.hp + s.regen * SIM_DT).min(s.max);
        }
    }
}

/// Throws chunks in the victim's colours for every unit or building that
/// died this tick (from damage or otherwise); buildings burst bigger.
pub(crate) fn death_debris(
    data: Res<Data>,
    players: Res<Players>,
    mut fx: FxSpawner,
    mut deaths: MessageReader<EntityDied>,
    bodies: Query<(&Position, &Altitude, &Footprint)>,
) {
    for d in deaths.read() {
        let (accent, building) = match d.kind {
            EntityKind::Unit(u) => (data.unit(u).color, false),
            EntityKind::Building(b) => (data.building(b).color, true),
            EntityKind::Node(_) => continue,
        };
        let Ok((pos, alt, fp)) = bodies.get(d.entity) else { continue };
        let color = owner_tint(d.owner.and_then(|o| players.try_get(o)).map(|p| p.color), accent);
        let half = Vec3::new(fp.half.x, fp.height * 0.5, fp.half.y);
        let center = Vec3::new(pos.0.x, alt.0 + half.y, pos.0.y);
        let area = fp.half.x * fp.half.y * 4.0;
        let (count, piece) = if building {
            ((area / 3.0).clamp(10.0, 32.0) as u32, (fp.half.x.min(fp.half.y) * 0.3).clamp(0.3, 1.0))
        } else {
            ((4.0 + area * 3.0).clamp(5.0, 14.0) as u32, (fp.half.x.min(fp.half.y) * 0.5).clamp(0.1, 0.6))
        };
        fx.debris(center, half, color, count, piece);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shield(hp: f32, energy_resist: f32) -> Shield {
        Shield { hp, max: hp, regen: 10.0, delay: 4.0, energy_resist, since_hit: 50.0 }
    }

    #[test]
    fn shield_takes_damage_first_and_spills_the_rest() {
        let mut s = shield(30.0, 0.0);
        assert_eq!(absorb(&mut s, 20.0, DamageType::Slash), 0.0);
        assert_eq!(s.hp, 10.0);
        assert_eq!(s.since_hit, 0.0);
        assert_eq!(absorb(&mut s, 25.0, DamageType::Pierce), 15.0);
        assert_eq!(s.hp, 0.0);
    }

    #[test]
    fn energy_resist_lets_a_fraction_of_energy_through() {
        let mut s = shield(100.0, 0.25);
        assert_eq!(absorb(&mut s, 40.0, DamageType::Energy), 10.0);
        assert_eq!(s.hp, 70.0);
        assert_eq!(absorb(&mut s, 40.0, DamageType::Fire), 0.0);
    }

    #[test]
    fn alerts_are_throttled_per_owner_and_area() {
        let mut log = AlertLog::default();
        let (a, b) = (PlayerId(0), PlayerId(1));
        assert!(log.admit(a, Vec2::ZERO, 0.0));
        assert!(!log.admit(a, Vec2::new(10.0, 0.0), 1.0), "same area");
        assert!(log.admit(a, Vec2::new(50.0, 0.0), 1.0), "another area");
        assert!(log.admit(b, Vec2::ZERO, 1.0), "another owner");
        log.expire(5.5);
        assert!(log.admit(a, Vec2::ZERO, 5.5), "cooled down");
    }

    #[test]
    fn alerts_from_before_a_clock_reset_are_forgotten() {
        let mut log = AlertLog::default();
        let owner = PlayerId(0);
        assert!(log.admit(owner, Vec2::ZERO, 600.0));
        log.expire(0.5);
        assert!(log.admit(owner, Vec2::ZERO, 0.5), "a new match starts at 0 s");
    }
}
