//! Per-team visibility: recompute `FogOfWar` from every viewer's sight disc.

use af_core::ext::fog::{sight_radius, team_bit, FogBox, MAX_FOG_TEAMS};
use af_core::prelude::*;
use bevy::prelude::*;

use crate::grid::{stamp_disc, DiscCache};

/// Ticks between vision updates (10 Hz at 30 ticks/s).
pub const VISION_INTERVAL: u64 = 3;

/// What the latest vision update did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FogMode {
    /// No update has run yet.
    #[default]
    Pending,
    /// `FogOfWar::enabled` is false: everything counts as visible.
    Off,
    /// The match was set up with `reveal_map`: every cell is lit.
    Revealed,
    /// Normal fog: the grids hold this update's visibility.
    Live,
}

/// af_fog's working state between updates.
#[derive(Resource, Debug, Default)]
pub(crate) struct Vision {
    pub mode: FogMode,
    /// `SimClock::tick` of the latest update.
    pub last_tick: Option<u64>,
    /// Teams that have at least one player.
    pub team_mask: u32,
    /// Live mode: per cell, the teams seeing it (bit per team), row-major
    /// like the `FogOfWar` grids. Lets one lookup per cell answer "who sees
    /// this box" for every team at once.
    pub cell_teams: Vec<u32>,
    discs: DiscCache,
}

impl Vision {
    /// Teams currently seeing any part of `area` (the `team_sees_box` test
    /// for all teams at once).
    pub fn teams_seeing(&self, fog: &FogOfWar, area: &FogBox) -> u32 {
        let mut seen = 0;
        area.any_cell(fog, |i| {
            seen |= self.cell_teams.get(i).copied().unwrap_or(0);
            seen & self.team_mask == self.team_mask
        });
        seen
    }
}

/// Run condition: on the first tick, then every [`VISION_INTERVAL`] ticks
/// (and immediately when the clock restarts for a new match).
pub(crate) fn vision_due(clock: Res<SimClock>, vision: Res<Vision>) -> bool {
    vision.last_tick.is_none_or(|last| clock.tick < last || clock.tick >= last + VISION_INTERVAL)
}

/// Recompute every team's `visible` grid (clear, then stamp a disc per
/// viewer) and fold it into `explored`.
///
/// A viewer is any live entity with an owner and stats: units, buildings
/// and foundations. It lights every cell holding a point within
/// `sight_radius` of its real position (its vision beyond its box, the
/// edge-to-edge convention of weapon ranges), so units see what they shoot.
pub(crate) fn update_grids(
    clock: Res<SimClock>,
    players: Res<Players>,
    setup: Option<Res<MatchSetup>>,
    map: Option<Res<MapInfo>>,
    mut fog: ResMut<FogOfWar>,
    mut vision: ResMut<Vision>,
    viewers: Query<(&Owner, &Position, &Stats, Option<&Footprint>), Without<Dead>>,
) {
    let vision = &mut *vision;
    vision.last_tick = Some(clock.tick);
    vision.team_mask = players.iter().fold(0, |m, p| m | team_bit(p.team));
    if !fog.enabled {
        vision.mode = FogMode::Off;
        return;
    }
    if setup.is_some_and(|s| s.reveal_map) {
        // Fill once; refill only if someone (af_map) replaced the grids.
        if vision.mode != FogMode::Revealed || fog.is_changed() {
            vision.mode = FogMode::Revealed;
            reveal_all(&mut fog, map.as_deref(), &players);
        }
        return;
    }
    vision.mode = FogMode::Live;
    let fog = &mut *fog;
    if !fit_grids(fog, map.as_deref(), &players) {
        vision.cell_teams.clear();
        return;
    }
    for team in &mut fog.teams {
        team.visible.fill(0);
    }

    let mut stamped = 0u32;
    for (owner, pos, stats, footprint) in &viewers {
        let team = players.team(owner.0);
        if team >= MAX_FOG_TEAMS || !pos.0.is_finite() {
            continue;
        }
        let Some(grid) = fog.teams.get_mut(team as usize) else { continue };
        let at = pos.0 / fog.cell;
        let cell = at.floor();
        let disc = vision.discs.disc(sight_radius(stats.vision, footprint) / fog.cell, at - cell);
        stamp_disc(&mut grid.visible, fog.width, fog.height, cell.x as i32, cell.y as i32, disc);
        stamped |= team_bit(team);
    }

    vision.cell_teams.clear();
    vision.cell_teams.resize(fog.width as usize * fog.height as usize, 0);
    for (t, grid) in fog.teams.iter_mut().enumerate().take(MAX_FOG_TEAMS as usize) {
        let bit = 1u32 << t;
        if stamped & bit == 0 {
            continue;
        }
        // Two single-purpose loops: each vectorises, a fused one does not.
        for (explored, visible) in grid.explored.iter_mut().zip(&grid.visible) {
            *explored |= *visible != 0;
        }
        for (teams, visible) in vision.cell_teams.iter_mut().zip(&grid.visible) {
            *teams |= bit * u32::from(*visible != 0);
        }
    }
}

/// Light every cell for every team (reveal_map).
fn reveal_all(fog: &mut FogOfWar, map: Option<&MapInfo>, players: &Players) {
    if fit_grids(fog, map, players) {
        for team in &mut fog.teams {
            team.visible.fill(1);
            team.explored.fill(true);
        }
    }
}

/// Make the grids usable: derive missing dimensions from the map, add a
/// slot for every player's team, and reallocate grids of the wrong length
/// (af_map normally sets all of this up; this keeps a partial setup
/// working). Returns false when there is no grid to work on.
fn fit_grids(fog: &mut FogOfWar, map: Option<&MapInfo>, players: &Players) -> bool {
    if !(fog.cell.is_finite() && fog.cell > 0.0) {
        warn_once!("fog of war has an invalid cell size ({}); vision is not computed", fog.cell);
        return false;
    }
    if fog.width == 0 || fog.height == 0 {
        let Some(map) = map else { return false };
        fog.width = (map.size.x / fog.cell).ceil().max(0.0) as u32;
        fog.height = (map.size.y / fog.cell).ceil().max(0.0) as u32;
        if fog.width == 0 || fog.height == 0 {
            return false;
        }
    }
    let slots = players.iter().filter(|p| p.team < MAX_FOG_TEAMS).map(|p| p.team as usize + 1).max().unwrap_or(0);
    if fog.teams.len() < slots {
        fog.teams.resize_with(slots, TeamFog::default);
    }
    let cells = fog.width as usize * fog.height as usize;
    for team in &mut fog.teams {
        if team.visible.len() != cells {
            team.visible = vec![0; cells];
        }
        if team.explored.len() != cells {
            team.explored = vec![false; cells];
        }
    }
    true
}
