//! `CursorWorld::over_ui` for the input layer: true while the mouse is over
//! any blocking UI node. Transparent layout roots carry `Pickable::IGNORE`
//! so they never count. A `CursorOverride` stands in for a mouse over the
//! world, so it never counts as over the HUD.

use af_core::ext::input::CursorOverride;
use af_core::prelude::*;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;

pub(crate) fn update_over_ui(
    hover: Option<Res<HoverMap>>,
    stand_in: Option<Res<CursorOverride>>,
    nodes: Query<(), With<Node>>,
    mut cursor: ResMut<CursorWorld>,
) {
    let virtual_mouse = stand_in.is_some_and(|o| o.0.is_some());
    let over = !virtual_mouse && hover.is_some_and(|h| h.get(&PointerId::Mouse).is_some_and(|hits| hits.keys().any(|e| nodes.contains(*e))));
    if cursor.over_ui != over {
        cursor.over_ui = over;
    }
}
