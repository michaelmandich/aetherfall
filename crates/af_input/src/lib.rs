//! Aetherfall player input: RTS camera, picking, selection, control groups, commands, placement modes, hotkeys.
//!
//! Everything runs in `Update`, `ClientSet::Input`, while `GameState::Playing`,
//! chained in this order (so each step sees the previous one's result):
//!
//! 1. camera: once per match, frame the start position with the start view
//!    ([`START_DISTANCE`], [`VIEW_YAW`]); pan/zoom the `CameraRig` (arrows,
//!    screen edges, middle drag, wheel), drive the
//!    [`RtsCamera`](af_core::ext::input::RtsCamera) transform from it;
//! 2. cursor: ray through the mouse (or its stand-in,
//!    [`CursorOverride`](af_core::ext::input::CursorOverride)) ->
//!    `CursorWorld` (ground point, hovered entity the local team can see);
//! 3. hotkeys: Esc, R, control groups, idle worker, main hall (H), army (F2), centre,
//!    Delete, pause, speed;
//! 4. selection: click / shift / ctrl / double-click / drag-box (`DragSelect`);
//! 5. orders: right-click `Smart`, then clicks that complete an `InputMode`;
//! 6. placement preview (`PlacementPreview`) and pruning of selected
//!    entities that went out of sight.
//!
//! All timing uses `Time<Real>`, so the camera and selection keep working
//! while the simulation is paused. Players act only through `PlayerCommand`s
//! pushed into `CommandInbox`; spectators (`LocalPlayer(None)`) can look and
//! select but issue nothing.

// Bevy systems receive their data as (often many, often generic) parameters.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod camera;
mod cursor;
mod hotkeys;
mod orders;
mod placement;
mod selection;
mod viewer;

use af_core::ext::input::{ControlGroups, PlacementPreview};
use af_core::prelude::*;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;

pub use camera::{rig_transform, view_axes, FOV_DEG, MAX_DISTANCE, MIN_DISTANCE, START_DISTANCE, VIEW_YAW};

pub struct RtsInputPlugin;

impl Plugin for RtsInputPlugin {
    fn build(&self, app: &mut App) {
        // InputPlugin registers these too (init is idempotent); doing it here
        // keeps the plugin usable in headless apps and tests without it.
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<ControlGroups>()
            .init_resource::<PlacementPreview>()
            .init_resource::<camera::StartFocus>()
            .init_resource::<selection::Gesture>()
            .add_systems(OnEnter(GameState::Playing), camera::spawn_camera)
            .add_systems(OnExit(GameState::Playing), (camera::despawn_camera, reset_client_state))
            .add_systems(
                Update,
                (
                    camera::focus_match_start.run_if(match_ready),
                    camera::camera_control,
                    camera::sync_camera_transform,
                    (
                        cursor::update_cursor,
                        hotkeys::escape,
                        hotkeys::rotate_placement,
                        hotkeys::control_groups,
                        hotkeys::select_idle_worker,
                        hotkeys::select_hall,
                        hotkeys::select_army,
                        hotkeys::centre_camera,
                        hotkeys::delete_selected,
                        hotkeys::game_speed,
                        selection::selection_gesture,
                        orders::smart_command,
                        orders::mode_clicks,
                        placement::placement_preview,
                        selection::prune_selection,
                    )
                        .chain()
                        .run_if(match_ready),
                )
                    .chain()
                    .in_set(ClientSet::Input)
                    .run_if(in_state(GameState::Playing)),
            )
            // Re-apply the rig after ClientSet::Ui (the minimap writes `focus`)
            // so the rendered view never lags a frame behind.
            .add_systems(
                PostUpdate,
                camera::sync_camera_transform
                    .before(TransformSystems::Propagate)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

/// A match is set up: players and the nav grid exist (af_map inserts them
/// on entering `Playing`).
fn match_ready(players: Option<Res<Players>>, nav: Option<Res<Nav>>) -> bool {
    players.is_some() && nav.is_some()
}

/// Leave no client state behind for the next match: modes, drags, groups,
/// previews, and a paused or sped-up virtual clock.
fn reset_client_state(
    mut mode: ResMut<InputMode>,
    mut drag: ResMut<DragSelect>,
    mut cursor: ResMut<CursorWorld>,
    mut groups: ResMut<ControlGroups>,
    mut preview: ResMut<PlacementPreview>,
    mut gesture: ResMut<selection::Gesture>,
    mut time: ResMut<Time<Virtual>>,
) {
    *mode = InputMode::Normal;
    drag.0 = None;
    cursor.ground = None;
    cursor.hovered = None;
    groups.clear();
    preview.ghosts.clear();
    *gesture = selection::Gesture::default();
    time.unpause();
    time.set_relative_speed(1.0);
}
