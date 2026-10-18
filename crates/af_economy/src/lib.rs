//! Aetherfall economy and rules: placement, construction, production queues,
//! research, ages, techs, upgrades, trickle income and victory.
//!
//! Everything here is authoritative simulation running in `FixedUpdate`:
//!
//! | set | systems |
//! |---|---|
//! | `EconomyIntake` (in `SimSet::Commands`) | one pass over the tick's commands in inbox order: `PlaceBuilding`/`PlaceWall` (and their builders' Build orders), `Train`/`Research`/`UpgradeBuilding`/`CancelQueue`, `SetRally`/`Smart` on buildings |
//! | `SimSet::Economy` (chained) | Build orders for the builders an earlier command of the tick also ordered (see `builders.rs`), construction completion, production queues (units, techs, upgrades), building upgrades, stat refresh after research, trickle income, defeat, reveal and victory (see `victory.rs`) |
//!
//! Two observers keep derived state honest whatever removes a building:
//! a claimed node is released when its `ClaimsNode` building goes away, and
//! research queued in a lost building can be queued again elsewhere.
//!
//! Permissions and numbers always come from `af_core::rules` and
//! `af_core::stats`, so the UI and the AI see exactly what this crate enforces.

// Bevy systems take many parameters and wide queries by design.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]

use af_core::ext::economy::{EconomyIntake, PlacedSites, RevealedPlayers};
use af_core::prelude::*;
use bevy::prelude::*;

mod builders;
mod construction;
mod income;
mod intake;
mod lifecycle;
mod production;
mod refresh;
mod victory;

/// Adds the economy systems to `EconomyIntake` and `SimSet::Economy`.
pub struct EconomyPlugin;

impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<production::UpgradeFinished>()
            .init_resource::<af_core::spawn::SpawnStats>()
            .init_resource::<PlacedSites>()
            .init_resource::<RevealedPlayers>()
            .add_systems(OnEnter(GameState::Playing), victory::reset_reveals)
            .add_systems(OnExit(GameState::Playing), victory::reset_reveals)
            .configure_sets(FixedUpdate, EconomyIntake.in_set(SimSet::Commands))
            .add_systems(FixedUpdate, intake::apply_commands.in_set(EconomyIntake))
            .add_systems(
                FixedUpdate,
                (
                    builders::assign_builders,
                    construction::complete_construction,
                    production::advance_queues,
                    production::apply_building_upgrades,
                    refresh::refresh_researched,
                    income::trickle_income,
                    victory::check_victory,
                )
                    .chain()
                    .in_set(SimSet::Economy),
            )
            .add_observer(lifecycle::release_claim)
            .add_observer(lifecycle::forget_lost_research);
    }
}

/// Where the player clicked when rallying a building onto an entity: fresh
/// units head there once that entity is gone. Only read while the
/// building's `Rally` targets an entity.
#[derive(Component, Clone, Copy, Debug)]
struct RallyFallback(Vec2);

/// Post a warning to one player.
fn warn(notices: &mut Notices, clock: &SimClock, player: PlayerId, text: impl Into<String>, pos: Option<Vec2>) {
    notices.push(Some(player), NoticeKind::Warning, text, pos, clock.elapsed);
}
