//! Shared resources (ECS singletons). Note: in Bevy 0.19 a `Resource` is
//! also a `Component`; never derive both on one type.

use std::sync::Arc;

use af_data::*;
use af_geom::NavGrid;
use bevy::prelude::*;

use crate::commands::PlayerCommand;
use crate::components::PlayerId;
use crate::stats::PlayerMods;

/// All game content. Inserted by the app at startup (before the menu).
#[derive(Resource, Clone, Deref)]
pub struct Data(pub Arc<GameData>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    /// This machine's player.
    Human,
    Ai,
    /// Slot that does nothing (testing).
    Inert,
}

/// Running counters for the score screen and tests.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerStats {
    pub gathered: ResAmounts,
    pub units_trained: u32,
    pub units_lost: u32,
    pub buildings_built: u32,
    pub buildings_lost: u32,
    pub kills: u32,
    pub techs: u32,
}

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub id: PlayerId,
    pub name: String,
    pub faction: FactionId,
    pub color: [f32; 3],
    /// Players on the same team are allies (share vision, pass gates).
    pub team: u8,
    pub controller: Controller,
    pub resources: ResAmounts,
    /// Recounted every tick by af_core (Prepare).
    pub pop_used: u32,
    /// Recounted every tick by af_core (Prepare); capped at [`POP_HARD_CAP`].
    pub pop_cap: u32,
    /// 1..=MAX_AGE. Written by af_economy.
    pub age: u8,
    /// By TechId. Written by af_economy.
    pub researched: Vec<bool>,
    /// Techs queued somewhere (cannot be queued twice). Written by af_economy.
    pub researching: Vec<bool>,
    pub mods: PlayerMods,
    /// Complete buildings owned, by BuildingTypeId (recounted in Prepare).
    pub buildings_owned: Vec<u32>,
    /// Buildings owned including foundations, by BuildingTypeId (Prepare).
    pub buildings_any: Vec<u32>,
    /// Units owned, by UnitTypeId (Prepare).
    pub units_owned: Vec<u32>,
    pub start_pos: Vec2,
    /// Written by af_economy (victory check).
    pub defeated: bool,
    pub stats: PlayerStats,
}

/// Hard population ceiling regardless of houses.
pub const POP_HARD_CAP: u32 = 300;

impl PlayerState {
    pub fn new(id: PlayerId, name: String, faction: FactionId, color: [f32; 3], team: u8, controller: Controller, data: &GameData) -> PlayerState {
        PlayerState {
            id,
            name,
            faction,
            color,
            team,
            controller,
            resources: data.faction(faction).start.resources.amounts,
            pop_used: 0,
            pop_cap: 0,
            age: 1,
            researched: vec![false; data.techs.len()],
            researching: vec![false; data.techs.len()],
            mods: PlayerMods::default(),
            buildings_owned: vec![0; data.buildings.len()],
            buildings_any: vec![0; data.buildings.len()],
            units_owned: vec![0; data.units.len()],
            start_pos: Vec2::ZERO,
            defeated: false,
            stats: PlayerStats::default(),
        }
    }
    pub fn has_tech(&self, t: TechId) -> bool {
        self.researched.get(t.index()).copied().unwrap_or(false)
    }
    pub fn can_afford(&self, cost: &ResAmounts) -> bool {
        self.resources.covers(cost)
    }
    /// Deduct `cost` if affordable.
    pub fn pay(&mut self, cost: &ResAmounts) -> bool {
        if self.resources.covers(cost) {
            self.resources.sub_all(cost);
            true
        } else {
            false
        }
    }
    pub fn refund(&mut self, amount: &ResAmounts) {
        self.resources.add_all(amount);
    }
    pub fn pop_room(&self) -> u32 {
        self.pop_cap.saturating_sub(self.pop_used)
    }
}

/// Every player in the match, indexed by `PlayerId`. Inserted by af_map.
#[derive(Resource, Clone, Debug, Default)]
pub struct Players {
    pub list: Vec<PlayerState>,
}

impl Players {
    pub fn get(&self, p: PlayerId) -> &PlayerState {
        &self.list[p.index()]
    }
    pub fn get_mut(&mut self, p: PlayerId) -> &mut PlayerState {
        &mut self.list[p.index()]
    }
    pub fn try_get(&self, p: PlayerId) -> Option<&PlayerState> {
        self.list.get(p.index())
    }
    pub fn team(&self, p: PlayerId) -> u8 {
        self.list.get(p.index()).map_or(af_geom::NO_TEAM, |s| s.team)
    }
    pub fn allied(&self, a: PlayerId, b: PlayerId) -> bool {
        a == b || self.team(a) == self.team(b)
    }
    /// `a` and `b` are both real players on different teams.
    pub fn hostile(&self, a: PlayerId, b: PlayerId) -> bool {
        !self.allied(a, b)
    }
    pub fn iter(&self) -> impl Iterator<Item = &PlayerState> {
        self.list.iter()
    }
}

/// The local human's player slot; `None` = spectating.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct LocalPlayer(pub Option<PlayerId>);

/// Static description of the generated map. Inserted by af_map.
#[derive(Resource, Clone, Debug, Default)]
pub struct MapInfo {
    /// World size in metres (x, z); the map spans 0..size.
    pub size: Vec2,
    pub seed: u64,
    /// Start position per player index.
    pub start_positions: Vec<Vec2>,
    /// Centre and radius of the Starfall Crater (contested aether/adamant).
    pub crater_center: Vec2,
    pub crater_radius: f32,
}

/// The navigation grid (terrain + obstacles). Inserted by af_map;
/// obstacles kept in sync by af_movement.
#[derive(Resource, Clone, Debug, Deref, DerefMut)]
pub struct Nav(pub NavGrid);

/// Simulation clock. Advanced by af_core in Prepare.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SimClock {
    pub tick: u64,
    /// Sim seconds since the match started.
    pub elapsed: f32,
}

/// Commands from any source (UI/input, AI, tests, network later). Drained
/// into [`TickCommands`] at the start of each tick (Prepare), so a command
/// pushed during tick N executes in tick N+1 for everyone.
#[derive(Resource, Clone, Debug, Default)]
pub struct CommandInbox {
    pub pending: Vec<PlayerCommand>,
}

impl CommandInbox {
    pub fn push(&mut self, c: PlayerCommand) {
        self.pending.push(c);
    }
}

/// The commands executing this tick (read in SimSet::Commands).
#[derive(Resource, Clone, Debug, Default)]
pub struct TickCommands {
    pub list: Vec<PlayerCommand>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Warning,
    /// Under attack, building destroyed, etc.
    Alert,
}

#[derive(Clone, Debug)]
pub struct Notice {
    /// None = everyone.
    pub player: Option<PlayerId>,
    pub text: String,
    pub kind: NoticeKind,
    pub pos: Option<Vec2>,
    /// SimClock::elapsed when posted.
    pub time: f32,
}

/// Player-facing messages ("Not enough wood", "Your town is under attack").
/// Anyone appends; af_ui shows the local player's recent ones.
#[derive(Resource, Clone, Debug, Default)]
pub struct Notices {
    pub list: Vec<Notice>,
}

impl Notices {
    pub fn push(&mut self, player: Option<PlayerId>, kind: NoticeKind, text: impl Into<String>, pos: Option<Vec2>, time: f32) {
        self.list.push(Notice { player, text: text.into(), kind, pos, time });
        if self.list.len() > 200 {
            self.list.drain(0..100);
        }
    }
}

#[derive(Clone, Debug)]
pub struct PlayerSetup {
    pub name: String,
    pub faction: FactionId,
    pub team: u8,
    pub controller: Controller,
    /// None = the faction colour (duplicates get shifted by af_map).
    pub color: Option<[f32; 3]>,
}

/// How to build a match. Inserted before entering `GameState::Playing`
/// (by the menu, CLI or a test); af_map reads it OnEnter(Playing).
#[derive(Resource, Clone, Debug)]
pub struct MatchSetup {
    pub seed: u64,
    /// Side of the land square in metres (Coast and Seas add ocean bands
    /// beyond it); 0 = from `MapOptions::size` and the player count.
    pub map_size: f32,
    pub players: Vec<PlayerSetup>,
    /// Index into `players` of the local human, or None to spectate.
    pub local_player: Option<usize>,
    pub fog: bool,
    pub reveal_map: bool,
    /// Start every player in this age (debug).
    pub start_age: u8,
    /// Multiplies starting resources (debug / quick tests).
    pub start_resources_mult: f32,
}

impl Default for MatchSetup {
    fn default() -> Self {
        MatchSetup {
            seed: 1,
            map_size: 0.0,
            players: Vec::new(),
            local_player: Some(0),
            fog: true,
            reveal_map: false,
            start_age: 1,
            start_resources_mult: 1.0,
        }
    }
}

/// Set by af_economy when the match is decided.
#[derive(Resource, Clone, Debug, Default)]
pub struct GameOutcome {
    pub ended: bool,
    pub winning_team: Option<u8>,
    pub time: f32,
}

/// Fog-of-war grids per team. Computed by af_fog (Vision set); read by
/// af_render/af_ui/af_input (hide enemies, minimap) and optionally the AI.
#[derive(Resource, Clone, Debug)]
pub struct FogOfWar {
    pub enabled: bool,
    /// Cell size in metres.
    pub cell: f32,
    pub width: u32,
    pub height: u32,
    /// Indexed by team id. `visible[i] > 0` = seen this update.
    pub teams: Vec<TeamFog>,
}

#[derive(Clone, Debug, Default)]
pub struct TeamFog {
    pub visible: Vec<u8>,
    pub explored: Vec<bool>,
}

impl Default for FogOfWar {
    fn default() -> Self {
        FogOfWar { enabled: false, cell: 2.0, width: 0, height: 0, teams: Vec::new() }
    }
}

impl FogOfWar {
    #[inline]
    fn idx(&self, p: Vec2) -> Option<usize> {
        let x = (p.x / self.cell).floor() as i32;
        let y = (p.y / self.cell).floor() as i32;
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            None
        } else {
            Some((y as u32 * self.width + x as u32) as usize)
        }
    }
    /// Is `p` currently visible to `team`? Always true with fog disabled.
    pub fn visible(&self, team: u8, p: Vec2) -> bool {
        if !self.enabled {
            return true;
        }
        match (self.teams.get(team as usize), self.idx(p)) {
            (Some(t), Some(i)) => t.visible.get(i).is_some_and(|v| *v > 0),
            _ => false,
        }
    }
    pub fn explored(&self, team: u8, p: Vec2) -> bool {
        if !self.enabled {
            return true;
        }
        match (self.teams.get(team as usize), self.idx(p)) {
            (Some(t), Some(i)) => t.explored.get(i).copied().unwrap_or(false),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Client-side shared state (af_input, af_ui, af_render)
// ---------------------------------------------------------------------------

/// What a left click in the world means right now. Written by af_ui
/// (command-card buttons) and af_input (hotkeys, completing/cancelling).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub enum InputMode {
    #[default]
    Normal,
    /// Placing a building; `facing` rotates with the R key (af_input).
    Place { kind: BuildingTypeId, facing: f32 },
    /// Placing a wall line; `start` is set after the first click.
    PlaceWall { kind: BuildingTypeId, start: Option<Vec2> },
    /// Waiting for a target click for a command button.
    Target(TargetAction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetAction {
    Move,
    AttackMove,
    Attack,
    Patrol,
    Rally,
    Gather,
    Repair,
}

/// Where the mouse points this frame. Written by af_ui (`over_ui`, in
/// ClientSet::Pointer) and af_input (`ground`, `hovered`, in ClientSet::Input).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CursorWorld {
    pub ground: Option<Vec2>,
    pub hovered: Option<Entity>,
    pub over_ui: bool,
    /// Cursor position in logical window pixels (as `Window::cursor_position`
    /// reports), if inside the window. Written by af_input.
    pub screen: Option<Vec2>,
}

/// The RTS camera's target. af_input owns the camera; af_ui's minimap
/// writes `focus` to jump.
#[derive(Resource, Clone, Copy, Debug)]
pub struct CameraRig {
    pub focus: Vec2,
    /// Distance from focus to camera (zoom).
    pub distance: f32,
    pub yaw: f32,
    /// Pitch below horizontal (radians).
    pub pitch: f32,
}

impl Default for CameraRig {
    fn default() -> Self {
        CameraRig { focus: Vec2::new(50.0, 50.0), distance: 55.0, yaw: std::f32::consts::FRAC_PI_4, pitch: 0.95 }
    }
}

/// Drag-select rectangle (corners in logical window pixels) while dragging.
/// af_input writes it; af_ui draws it.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct DragSelect(pub Option<(Vec2, Vec2)>);
