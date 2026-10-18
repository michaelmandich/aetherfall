//! Aetherfall combat: target acquisition, weapons (melee, bolts, arcs,
//! cannons, beams, bombs, homing, spray, nova, chain), projectiles, damage,
//! shields, death.
//!
//! Systems (FixedUpdate, chained within each set):
//!
//! | set | systems |
//! |---|---|
//! | `SimSet::Combat` | validate `AttackTarget` -> pick a target per weapon slot -> aim (body facing as far as there is room to turn, turret `aim_yaw`) -> weapon cycle (cooldown, windup, fire) |
//! | `SimSet::Projectiles` | bolts, homing missiles, lobbed shells and bombs, cannon shots, beams -> `DamageMsg`; cosmetic FX |
//! | `SimSet::Damage` | apply `DamageMsg` (armour, shields, hp, deaths, alerts), regeneration, death debris |
//! | `OnEnter(GameState::Playing)` | reset per-match state (alert throttling, effect randomness) |
//!
//! Combat never moves units and never writes `AttackTarget` (except to drop
//! an invalid one); af_behavior owns chasing. Every projectile, beam and
//! effect is an entity with an `FxBox` that af_render draws.

mod body;
mod damage;
mod firing;
mod fx;
mod hits;
mod projectile;
mod targeting;
mod turning;

use af_core::prelude::*;
use bevy::prelude::*;

/// Adds the combat systems to `SimSet::Combat`, `SimSet::Projectiles` and
/// `SimSet::Damage`. Needs `af_core::CorePlugin`.
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<fx::FxRng>()
            .init_resource::<fx::FxCount>()
            .init_resource::<damage::AlertLog>()
            .add_systems(OnEnter(GameState::Playing), reset_match_state)
            .add_systems(
                FixedUpdate,
                (
                    targeting::validate_attack_targets,
                    targeting::acquire_targets,
                    targeting::aim_weapons,
                    firing::cycle_weapons,
                )
                    .chain()
                    .in_set(SimSet::Combat),
            )
            .add_systems(
                FixedUpdate,
                (
                    projectile::fly_bolts,
                    projectile::fly_homing,
                    projectile::fly_ballistic,
                    projectile::fly_cannon_shots,
                    projectile::update_beams,
                    fx::animate_fx,
                )
                    .chain()
                    .in_set(SimSet::Projectiles),
            )
            .add_systems(
                FixedUpdate,
                (damage::apply_damage, damage::regenerate, damage::death_debris).chain().in_set(SimSet::Damage),
            );
    }
}

/// Every match starts with fresh combat state, as af_map resets `SimClock`
/// and `SimRng` on the same transition: no alert throttling carried over
/// from the last match, and the same effect randomness for the same seed.
fn reset_match_state(mut commands: Commands) {
    commands.insert_resource(fx::FxRng::default());
    commands.insert_resource(fx::FxCount::default());
    commands.insert_resource(damage::AlertLog::default());
}
