//! Shared items added by af_ui (other crates may use them).
//! Only af_ui edits this file. Never modify items in other af_core files.

#[allow(unused_imports)]
use crate::prelude::*;
#[allow(unused_imports)]
use bevy::prelude::*;

/// Whether the command card shows a sub-page (a build page), so that Esc
/// steps back out of it before it clears the selection. af_ui sets `open`
/// every frame it lays the card out; af_input's Esc, finding no drag or
/// mode to cancel and a page open, sets `close` instead of deselecting;
/// af_ui then returns to the main card and clears `close`. Absent (or
/// default) means no page is open.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardPage {
    pub open: bool,
    pub close: bool,
}

/// Hides the whole in-match HUD (panels, minimap, notices, overlays) while
/// present and `true`, so a screenshot shows only the world (the dev flag
/// `--no-hud`). The HUD still runs underneath; setting it back to `false`
/// shows it again. Absent means shown.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HudHidden(pub bool);
