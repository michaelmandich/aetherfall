//! Who is looking at the screen and what they may see and select.

use af_core::ext::fog::{team_sees, team_sees_box, FogBox, SeenBy};
use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// The local player (`None` = spectator) with the fog and teams needed to
/// answer "can I see / select / command this?".
#[derive(SystemParam)]
pub(crate) struct Viewer<'w, 's> {
    local: Res<'w, LocalPlayer>,
    players: Res<'w, Players>,
    fog: Res<'w, FogOfWar>,
    seen: Query<'w, 's, &'static SeenBy>,
}

impl Viewer<'_, '_> {
    /// Entities of `owner` count as "own" for selection: the local player's,
    /// or every player's for a spectator.
    pub fn owns(&self, owner: PlayerId) -> bool {
        self.local.0.is_none_or(|me| me == owner)
    }

    /// Can the viewer see `entity`, the `kind` of thing of `owner` (`None` =
    /// a resource node) whose box is `obb`?
    pub fn sees(&self, entity: Entity, owner: Option<PlayerId>, kind: SpatialKind, obb: &Obb) -> bool {
        sees(self.local.0, &self.players, &self.fog, self.seen.get(entity).ok(), owner, kind, obb)
    }
}

/// Own and allied things are always visible, and spectators see everything.
/// Otherwise the rules follow what the fog shows, as af_render draws it:
/// enemy units while any of their box is in sight; enemy buildings while
/// af_fog's `SeenBy` says the team sees them (any part of the box in sight,
/// or the owner's buildings revealed to all); resource nodes once the team
/// has seen them. Until af_fog has given an entity its `SeenBy`, the same
/// test runs on the box itself (never its bounding rectangle, which reaches
/// past a turned box into cells it does not touch).
pub(crate) fn sees(
    local: Option<PlayerId>,
    players: &Players,
    fog: &FogOfWar,
    seen: Option<&SeenBy>,
    owner: Option<PlayerId>,
    kind: SpatialKind,
    obb: &Obb,
) -> bool {
    let Some(me) = local else { return true };
    let team = players.team(me);
    match owner {
        Some(o) if players.allied(me, o) => true,
        Some(_) if kind == SpatialKind::Unit => team_sees(fog, team, obb.center, obb.half.max_element()),
        Some(_) => seen.map_or_else(|| team_sees_box(fog, team, obb), |s| s.visible_to(team)),
        None => seen.map_or_else(|| box_explored(fog, team, obb), |s| s.seen_by(team)),
    }
}

/// Has `team` explored any fog cell the box `obb` overlaps? Always true
/// with the fog off.
fn box_explored(fog: &FogOfWar, team: u8, obb: &Obb) -> bool {
    if !fog.enabled {
        return true;
    }
    let Some(grid) = fog.teams.get(team as usize) else { return false };
    FogBox::new(obb).any_cell(fog, |i| grid.explored.get(i).copied().unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn players() -> Players {
        let data = af_core::testing::test_data();
        let f = data.faction_id("ironhold").unwrap();
        let p = |i: u8, team: u8| PlayerState::new(PlayerId(i), format!("p{i}"), f, [1.0; 3], team, Controller::Inert, &data);
        Players { list: vec![p(0, 0), p(1, 0), p(2, 1)] }
    }

    /// A 4x4 grid of 1 m cells; `visible` / `explored` are cell indices.
    fn fog_with(visible_cell: Option<usize>, explored_cell: Option<usize>) -> FogOfWar {
        let mut team = TeamFog { visible: vec![0; 16], explored: vec![false; 16] };
        if let Some(i) = visible_cell {
            team.visible[i] = 1;
        }
        if let Some(i) = explored_cell {
            team.explored[i] = true;
        }
        FogOfWar { enabled: true, cell: 1.0, width: 4, height: 4, teams: vec![team.clone(), team] }
    }

    fn square(center: Vec2, half: f32) -> Obb {
        Obb::new(center, Vec2::splat(half), 0.0)
    }

    const ME: Option<PlayerId> = Some(PlayerId(0));
    const ALLY: Option<PlayerId> = Some(PlayerId(1));
    const ENEMY: Option<PlayerId> = Some(PlayerId(2));

    #[test]
    fn allies_always_seen_enemies_only_in_vision() {
        let (players, fog) = (players(), fog_with(Some(0), None));
        let dark = square(Vec2::new(3.5, 3.5), 0.3);
        assert!(sees(ME, &players, &fog, None, ALLY, SpatialKind::Unit, &dark), "ally in the dark");
        assert!(!sees(ME, &players, &fog, None, ENEMY, SpatialKind::Unit, &dark), "enemy in the dark");
        assert!(!sees(ME, &players, &fog, None, ENEMY, SpatialKind::Building, &dark), "enemy building in the dark");
        assert!(sees(ME, &players, &fog, None, ENEMY, SpatialKind::Unit, &square(Vec2::new(0.5, 0.5), 0.3)), "enemy in vision");
    }

    #[test]
    fn enemies_partly_in_sight_are_seen() {
        let (players, fog) = (players(), fog_with(Some(0), None));
        // Centre cells are dark; only cell (0, 0) is lit.
        let unit = square(Vec2::new(1.4, 0.5), 0.5);
        assert!(sees(ME, &players, &fog, None, ENEMY, SpatialKind::Unit, &unit), "the unit's edge is in sight");
        let building = square(Vec2::new(1.5, 1.5), 0.6);
        assert!(sees(ME, &players, &fog, None, ENEMY, SpatialKind::Building, &building), "a corner is in sight");
        // Turned 45 degrees, a footprint just clear of the lit cell pokes a corner into it.
        let straight = square(Vec2::new(1.75, 0.5), 0.55);
        assert!(!sees(ME, &players, &fog, None, ENEMY, SpatialKind::Building, &straight));
        let turned = Obb::new(straight.center, straight.half, std::f32::consts::FRAC_PI_4);
        assert!(sees(ME, &players, &fog, None, ENEMY, SpatialKind::Building, &turned));
        let clear_of_it = square(Vec2::new(2.5, 1.5), 0.4);
        assert!(!sees(ME, &players, &fog, None, ENEMY, SpatialKind::Building, &clear_of_it));
    }

    /// A 10x10 grid of 2 m cells (af_map's fog cell) where only cell
    /// (`x`, `y`) is lit and explored.
    fn fog_lit_cell(x: usize, y: usize) -> FogOfWar {
        let mut team = TeamFog { visible: vec![0; 100], explored: vec![false; 100] };
        team.visible[y * 10 + x] = 1;
        team.explored[y * 10 + x] = true;
        FogOfWar { enabled: true, cell: 2.0, width: 10, height: 10, teams: vec![team.clone(), team] }
    }

    #[test]
    fn turned_footprints_count_by_their_box_not_its_bounding_rectangle() {
        let players = players();
        // A 3 x 1 m palisade segment on a diagonal drag: its bounding
        // rectangle [9.59, 12.41]^2 reaches into the lit cell [12, 14)^2,
        // the box itself stops 0.9 m short of it (af_fog: not seen).
        let fog = fog_lit_cell(6, 6);
        let wall = Obb::new(Vec2::splat(11.0), Vec2::new(1.5, 0.5), std::f32::consts::FRAC_PI_4);
        let (_, max) = wall.aabb();
        assert!(max.x > 12.0 && max.y > 12.0, "the rectangle reaches the lit cell");
        assert!(!af_core::ext::fog::team_sees_box(&fog, 0, &wall), "af_fog does not see it");
        assert!(!sees(ME, &players, &fog, None, ENEMY, SpatialKind::Building, &wall), "so neither may input");
        assert!(!sees(ME, &players, &fog, None, None, SpatialKind::Node, &wall), "nor count it explored");
        // Moved along the diagonal until its side crosses into the cell.
        let closer = Obb { center: Vec2::splat(11.8), ..wall };
        assert!(sees(ME, &players, &fog, None, ENEMY, SpatialKind::Building, &closer));
        assert!(sees(ME, &players, &fog, None, None, SpatialKind::Node, &closer));
    }

    #[test]
    fn seen_by_decides_once_af_fog_has_written_it() {
        use af_core::ext::fog::team_bit;
        let (players, fog) = (players(), fog_with(Some(0), Some(0)));
        let lit = square(Vec2::new(0.5, 0.5), 0.3);
        let dark = square(Vec2::new(3.5, 3.5), 0.3);
        let in_sight = SeenBy { teams: team_bit(0), visible: team_bit(0) };
        let scouted = SeenBy { teams: team_bit(0), visible: 0 };
        let unseen = SeenBy::default();
        // A building of a player revealed to all is in sight in the dark.
        assert!(sees(ME, &players, &fog, Some(&in_sight), ENEMY, SpatialKind::Building, &dark));
        assert!(!sees(ME, &players, &fog, Some(&scouted), ENEMY, SpatialKind::Building, &lit), "out of sight per af_fog");
        assert!(sees(ME, &players, &fog, Some(&scouted), None, SpatialKind::Node, &dark), "nodes once seen");
        assert!(!sees(ME, &players, &fog, Some(&unseen), None, SpatialKind::Node, &lit));
        assert!(sees(ME, &players, &fog, Some(&unseen), ALLY, SpatialKind::Building, &dark), "allies regardless");
    }

    #[test]
    fn nodes_need_exploration_and_spectators_see_all() {
        let (players, fog) = (players(), fog_with(None, Some(5)));
        assert!(sees(ME, &players, &fog, None, None, SpatialKind::Node, &square(Vec2::new(1.5, 1.5), 0.3)));
        assert!(sees(ME, &players, &fog, None, None, SpatialKind::Node, &square(Vec2::new(2.2, 2.2), 0.3)), "partly explored");
        assert!(!sees(ME, &players, &fog, None, None, SpatialKind::Node, &square(Vec2::new(3.5, 3.5), 0.3)));
        assert!(sees(None, &players, &fog, None, ENEMY, SpatialKind::Unit, &square(Vec2::new(3.5, 3.5), 0.3)));
    }

    #[test]
    fn fog_off_shows_everything() {
        let (players, mut fog) = (players(), fog_with(None, None));
        fog.enabled = false;
        let far = square(Vec2::new(3.5, 3.5), 0.3);
        assert!(sees(ME, &players, &fog, None, ENEMY, SpatialKind::Building, &far));
        assert!(sees(ME, &players, &fog, None, None, SpatialKind::Node, &far));
    }
}
