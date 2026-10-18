//! Shared items added by af_map (other crates may use them).
//! Only af_map edits this file. Never modify items in other af_core files.

use bevy::prelude::*;

/// The set in which af_map builds the match world (map, players, every match
/// resource and the start entities) in `OnEnter(GameState::Playing)`, and
/// tears it down again in `OnExit(GameState::Playing)`.
///
/// * An `OnEnter(GameState::Playing)` system that needs `Players`, `Nav`,
///   `MapInfo` or the start entities runs `.after(MapSetupSet)`; Bevy
///   applies af_map's commands in between.
/// * An `OnExit(GameState::Playing)` system that still needs them runs
///   `.before(MapSetupSet)`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapSetupSet;
