//! Aetherfall UI: skirmish menu, HUD, resource bar, selection panel, command card, minimap, notices, game over.
//!
//! Everything is a Bevy UI node (boxes, fittingly) drawn with the default
//! font on dark translucent panels.
//!
//! * `ClientSet::Pointer`: [`pointer`] sets `CursorWorld::over_ui` from the
//!   picking hover map, so af_input ignores world clicks on the HUD.
//! * `GameState::Menu`: [`menu`], the skirmish setup screen and its camera,
//!   and [`multiplayer`], the server, room and ranked screens.
//! * `GameState::Playing`, `ClientSet::Ui`: the HUD — top bar, idle
//!   workers, notices, selection panel, command card + tooltip, minimap,
//!   drag box, pause and game-over overlays.
//! * [`card`], [`tooltip`] and [`format`] are the pure models behind the
//!   HUD; [`MinimapGeometry`] maps the minimap to the world.
//!
//! The UI acts on the game only through af_core: it pushes
//! `PlayerCommand`s into `CommandInbox`, sets `InputMode` for targeted
//! commands and building placement, edits the `Selected` markers, moves
//! `CameraRig::focus`, and appends `Notices`.

pub mod card;
pub mod format;
mod hud;
pub mod menu;
pub mod multiplayer;
mod pointer;
pub mod theme;
pub mod tooltip;

use af_core::prelude::*;
use bevy::prelude::*;

pub use hud::minimap::{MinimapGeometry, MINIMAP_PX, MINIMAP_RES};

/// Menu, HUD and the UI hit-test. Needs `CorePlugin`, and in a real app
/// the UI, picking and input plugins (all in `DefaultPlugins`).
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (pointer::update_over_ui.in_set(ClientSet::Pointer), theme::shade_buttons.in_set(ClientSet::Ui)));
        menu::plugin(app);
        multiplayer::plugin(app);
        hud::plugin(app);
    }
}
