//! Aetherfall UI: skirmish menu, HUD, resource bar, selection panel, command card, minimap, notices, game over.
//!
//! Everything is a Bevy UI node drawn with the default font: wooden panels
//! in metal frames and framed buttons from the painted [`skin`] (flat dark
//! panels until its textures load), with card-game press feedback ([`fx`]).
//!
//! * `ClientSet::Pointer`: [`pointer`] sets `CursorWorld::over_ui` from the
//!   picking hover map, so af_input ignores world clicks on the HUD.
//! * `GameState::Menu`: [`menu`], the skirmish setup screen and its camera.
//! * `GameState::Playing`, `ClientSet::Ui`: the HUD — top bar, idle
//!   workers, notices, selection panel, command card + tooltip, minimap,
//!   drag box, pause and game-over overlays.
//! * [`card`], [`tooltip`] and [`format`] are the pure models behind the
//!   HUD; [`MinimapGeometry`] maps the minimap to the world.
//!
//! The UI acts on the game only through af_core: it pushes
//! `PlayerCommand`s into `CommandInbox` (spawning a click marker,
//! `ClickPing` with a `ClickMark`, for each command given on the
//! minimap), sets `InputMode` for targeted commands and building
//! placement, edits the `Selected` markers, moves `CameraRig::focus`,
//! flips `ShowPaths::on` (the Paths button, which reads af_render's
//! `RoutesShown`), and appends `Notices`.

pub mod card;
pub mod format;
pub mod fx;
mod hud;
pub mod menu;
mod pointer;
pub mod skin;
pub mod theme;
pub mod tooltip;

use af_core::prelude::*;
use bevy::prelude::*;

pub use hud::minimap::{MinimapGeometry, MINIMAP_PX};

/// Menu, HUD and the UI hit-test. Needs `CorePlugin`, and in a real app
/// the UI, picking and input plugins (all in `DefaultPlugins`).
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (pointer::update_over_ui.in_set(ClientSet::Pointer), theme::shade_buttons.in_set(ClientSet::Ui).before(skin::SkinSet)));
        skin::plugin(app);
        menu::plugin(app);
        hud::plugin(app);
    }
}
