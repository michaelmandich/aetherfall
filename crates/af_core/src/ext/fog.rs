//! Shared items added by af_fog (other crates may use them).
//! Only af_fog edits this file. Never modify items in other af_core files.
//!
//! af_fog keeps [`FogOfWar`] current in `SimSet::Vision` (every few ticks)
//! and maintains the memory a renderer and the AI need on top of it:
//!
//! * [`SeenBy`] on every building and resource node: which teams have ever
//!   seen it (AoE-style: keep drawing an enemy building once scouted) and
//!   which see it right now.
//! * [`LastSeen`] on every building: how each other team last saw it (type,
//!   construction, damage), so a remembered building is drawn as it was,
//!   not as it is now.
//! * [`FogGhosts`]: buildings and nodes that died while a team that had seen
//!   them was not looking. That team keeps "seeing" the ghost until it looks
//!   at the spot again.
//! * [`team_sees_box`] / [`team_sees`] / [`team_explored`]: area queries
//!   against the grids (target acquisition, hiding enemy units, minimap
//!   pings, AI scouting).
//!
//! # Sight convention
//!
//! Weapon ranges are edge to edge, and the content gives direct-fire units
//! at least as much `vision` as range. So sight is measured from the
//! viewer's box: a viewer lights every fog cell holding a point within
//! [`sight_radius`] of its centre, which covers everything within `vision`
//! of its box in every direction. To ask "can team T see this target", test
//! the target's box ([`team_sees_box`], or [`SeenBy::visible_to`] for
//! buildings and nodes), never just its centre: a point test misses big
//! targets whose near side is in sight.
//!
//! Team ids index the masks as bits, so only teams `0..MAX_FOG_TEAMS` have
//! vision; entities on other teams see nothing.

use crate::prelude::*;
use bevy::prelude::*;

/// Number of teams the fog tracks (team ids `0..MAX_FOG_TEAMS`).
pub const MAX_FOG_TEAMS: u8 = 32;

/// The bit of `team` in a team mask (0 for teams the fog does not track).
#[inline]
pub fn team_bit(team: u8) -> u32 {
    1u32.checked_shl(team as u32).unwrap_or(0)
}

/// The team ids whose bits are set in `mask`, lowest first.
pub fn team_ids(mut mask: u32) -> impl Iterator<Item = u8> {
    std::iter::from_fn(move || {
        (mask != 0).then(|| {
            let team = mask.trailing_zeros() as u8;
            mask &= mask - 1;
            team
        })
    })
}

/// How far from its centre a viewer with `Stats::vision` `vision` sees:
/// `vision` beyond its box in every direction, i.e. `vision` plus the box's
/// bounding radius (see the module docs). Negative or NaN inputs count as 0.
pub fn sight_radius(vision: f32, footprint: Option<&Footprint>) -> f32 {
    vision.max(0.0) + footprint.map_or(0.0, |f| f.half.max(Vec2::ZERO).length())
}

/// Fog memory of a building or resource node. Inserted and updated by
/// af_fog on every `Building` and `ResourceNode` (absent until the first
/// vision update after spawn: treat a missing component as "seen by nobody").
///
/// With fog disabled or the map revealed both masks are [`SeenBy::ALL`].
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeenBy {
    /// Bit `t` set: team `t` has seen this entity at least once. Never
    /// cleared, so a renderer keeps drawing scouted enemy buildings.
    pub teams: u32,
    /// Bit `t` set: team `t` saw part of the entity's box in the latest
    /// vision update (the [`team_sees_box`] test), or the entity is a
    /// building of a player whose buildings are revealed to everyone
    /// (`ext::economy::RevealedPlayers`).
    pub visible: u32,
}

impl SeenBy {
    /// Every team.
    pub const ALL: u32 = u32::MAX;

    /// Has `team` ever seen this entity?
    #[inline]
    pub fn seen_by(&self, team: u8) -> bool {
        self.teams & team_bit(team) != 0
    }

    /// Does `team` see this entity right now?
    #[inline]
    pub fn visible_to(&self, team: u8) -> bool {
        self.visible & team_bit(team) != 0
    }
}

/// How a team saw a building.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildingSighting {
    pub kind: BuildingTypeId,
    /// `UnderConstruction::progress` (0..1) while it was a foundation.
    pub construction: Option<f32>,
    /// `Health::fraction` (1 without `Health`).
    pub hp: f32,
}

/// What other teams remember of a building: for each team that has seen
/// it, the building as of the latest vision update in which that team saw
/// it. Inserted with [`SeenBy`] and updated by af_fog on every `Building`.
/// The owner's team is not recorded (it always sees its own buildings).
///
/// While `!SeenBy::visible_to(team)`, draw (or reason about) the building
/// from `by(team)` instead of its live `Building`/`Health`/
/// `UnderConstruction`, so upgrades, finished foundations and damage inside
/// the fog stay hidden. Not maintained with fog disabled or the map
/// revealed, where every team sees everything live.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct LastSeen {
    /// One entry per team, sorted by team.
    entries: Vec<(u8, BuildingSighting)>,
}

impl LastSeen {
    /// How `team` last saw the building (None if it never has, or owns it).
    pub fn by(&self, team: u8) -> Option<&BuildingSighting> {
        let i = self.entries.binary_search_by_key(&team, |(t, _)| *t).ok()?;
        Some(&self.entries[i].1)
    }

    /// Does every team in `teams` already remember exactly `sighting`?
    pub fn is_current(&self, teams: u32, sighting: &BuildingSighting) -> bool {
        team_ids(teams).all(|t| self.by(t) == Some(sighting))
    }

    /// Every team in `teams` sees the building as `sighting` now.
    pub fn record(&mut self, teams: u32, sighting: BuildingSighting) {
        for team in team_ids(teams) {
            match self.entries.binary_search_by_key(&team, |(t, _)| *t) {
                Ok(i) => self.entries[i].1 = sighting,
                Err(i) => self.entries.insert(i, (team, sighting)),
            }
        }
    }
}

/// A building or node that died while out of sight of a team that had seen
/// it. Its entity is gone; draw it from these fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogGhost {
    /// The dead entity (already despawned; for matching only).
    pub entity: Entity,
    /// For a building, its type as these teams last saw it ([`LastSeen`]);
    /// teams that remember different types get separate ghosts.
    pub kind: EntityKind,
    pub owner: Option<PlayerId>,
    pub pos: Vec2,
    pub facing: f32,
    pub footprint: Footprint,
    /// Teams that still remember it. A team's bit clears once the team sees
    /// any part of its box again; the ghost is dropped at 0.
    pub teams: u32,
}

impl FogGhost {
    /// Does `team` still believe this entity stands?
    #[inline]
    pub fn remembered_by(&self, team: u8) -> bool {
        self.teams & team_bit(team) != 0
    }
}

/// Remembered dead buildings and nodes, oldest first. Written by af_fog
/// (`SimSet::Vision`), cleared when a match starts or ends.
#[derive(Resource, Clone, Debug, Default)]
pub struct FogGhosts {
    pub list: Vec<FogGhost>,
}

impl FogGhosts {
    /// Ghosts `team` still believes in.
    pub fn remembered_by(&self, team: u8) -> impl Iterator<Item = &FogGhost> {
        self.list.iter().filter(move |g| g.remembered_by(team))
    }
}

/// A fresh, all-dark [`FogOfWar`] covering a map of `map_size` metres with
/// `cell`-metre cells. `team_slots` is the highest team id + 1 (grids are
/// indexed by team id). Intended for af_map at match start.
pub fn fog_for_map(map_size: Vec2, cell: f32, team_slots: usize, enabled: bool) -> FogOfWar {
    let cell = if cell > 0.0 { cell } else { FogOfWar::default().cell };
    let width = (map_size.x / cell).ceil().max(0.0) as u32;
    let height = (map_size.y / cell).ceil().max(0.0) as u32;
    let n = width as usize * height as usize;
    FogOfWar {
        enabled,
        cell,
        width,
        height,
        teams: vec![TeamFog { visible: vec![0; n], explored: vec![false; n] }; team_slots],
    }
}

/// The fog cells covering the world rectangle `min..=max`, clipped to the
/// grid, as inclusive `(first, last)` cell coordinates; `None` when the
/// rectangle lies entirely outside the grid.
pub fn fog_cell_rect(fog: &FogOfWar, min: Vec2, max: Vec2) -> Option<(UVec2, UVec2)> {
    if fog.width == 0 || fog.height == 0 || fog.cell.is_nan() || fog.cell <= 0.0 {
        return None;
    }
    let lo = (min / fog.cell).floor();
    let hi = (max / fog.cell).floor();
    let size = Vec2::new(fog.width as f32, fog.height as f32);
    // Written so that NaN coordinates fall through to None.
    if !(hi.x >= 0.0 && hi.y >= 0.0 && lo.x < size.x && lo.y < size.y && lo.x <= hi.x && lo.y <= hi.y) {
        return None;
    }
    let lo = lo.max(Vec2::ZERO);
    let hi = hi.min(size - Vec2::ONE);
    Some((UVec2::new(lo.x as u32, lo.y as u32), UVec2::new(hi.x as u32, hi.y as u32)))
}

/// A box prepared for repeated fog lookups (axes and bounds computed once):
/// it finds the fog cells the box overlaps or touches. af_fog caches one per
/// building and node; [`team_sees_box`] builds one per call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogBox {
    center: Vec2,
    half: Vec2,
    right: Vec2,
    forward: Vec2,
    min: Vec2,
    max: Vec2,
}

impl FogBox {
    pub fn new(obb: &Obb) -> FogBox {
        let (right, forward) = obb.axes();
        let half = obb.half.max(Vec2::ZERO);
        let ext = right.abs() * half.x + forward.abs() * half.y;
        FogBox { center: obb.center, half, right, forward, min: obb.center - ext, max: obb.center + ext }
    }

    /// Calls `hit` with the row-major index of every cell of `fog` the box
    /// overlaps or touches (clipped to the grid) until `hit` returns true;
    /// returns whether it did.
    pub fn any_cell(&self, fog: &FogOfWar, mut hit: impl FnMut(usize) -> bool) -> bool {
        let Some((lo, hi)) = fog_cell_rect(fog, self.min, self.max) else { return false };
        // Separating axes: every cell in the AABB's range already overlaps
        // the box on the world axes, so only the box's own axes can
        // separate a cell from it.
        let spread = Vec2::new(self.right.x.abs() + self.right.y.abs(), self.forward.x.abs() + self.forward.y.abs());
        let limit = self.half + spread * (fog.cell * 0.5);
        for y in lo.y..=hi.y {
            let row = y as usize * fog.width as usize;
            for x in lo.x..=hi.x {
                let d = (Vec2::new(x as f32, y as f32) + 0.5) * fog.cell - self.center;
                if d.dot(self.right).abs() <= limit.x && d.dot(self.forward).abs() <= limit.y && hit(row + x as usize) {
                    return true;
                }
            }
        }
        false
    }
}

/// Does `team` currently see any part of the box `obb`? This is the test
/// for targets (see the module docs): a unit whose vision is at least its
/// edge-to-edge range sees every box it can shoot. Always true with fog
/// disabled.
pub fn team_sees_box(fog: &FogOfWar, team: u8, obb: &Obb) -> bool {
    if !fog.enabled {
        return true;
    }
    let Some(t) = fog.teams.get(team as usize) else { return false };
    FogBox::new(obb).any_cell(fog, |i| t.visible.get(i).is_some_and(|v| *v > 0))
}

/// Does `team` currently see any part of the disc (`pos`, `radius`)? For a
/// box prefer [`team_sees_box`]. Always true with fog disabled.
pub fn team_sees(fog: &FogOfWar, team: u8, pos: Vec2, radius: f32) -> bool {
    if !fog.enabled {
        return true;
    }
    let Some(t) = fog.teams.get(team as usize) else { return false };
    any_cell_near(fog, pos, radius, |i| t.visible.get(i).is_some_and(|v| *v > 0))
}

/// Has `team` ever explored any part of the disc (`pos`, `radius`)?
/// Always true with fog disabled.
pub fn team_explored(fog: &FogOfWar, team: u8, pos: Vec2, radius: f32) -> bool {
    if !fog.enabled {
        return true;
    }
    let Some(t) = fog.teams.get(team as usize) else { return false };
    any_cell_near(fog, pos, radius, |i| t.explored.get(i).copied().unwrap_or(false))
}

/// Does `hit` accept any cell (by row-major index) whose square touches the
/// disc (`pos`, `radius`)?
fn any_cell_near(fog: &FogOfWar, pos: Vec2, radius: f32, mut hit: impl FnMut(usize) -> bool) -> bool {
    let r = radius.max(0.0);
    let Some((lo, hi)) = fog_cell_rect(fog, pos - Vec2::splat(r), pos + Vec2::splat(r)) else { return false };
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let cell_min = Vec2::new(x as f32, y as f32) * fog.cell;
            let nearest = pos.max(cell_min).min(cell_min + Vec2::splat(fog.cell));
            if nearest.distance_squared(pos) <= r * r && hit((y * fog.width + x) as usize) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(cells: &[(u32, u32)]) -> FogOfWar {
        let mut fog = fog_for_map(Vec2::splat(20.0), 2.0, 2, true);
        for &(x, y) in cells {
            let i = (y * fog.width + x) as usize;
            fog.teams[0].visible[i] = 1;
            fog.teams[0].explored[i] = true;
        }
        fog
    }

    #[test]
    fn fog_for_map_sizes_grids() {
        let fog = fog_for_map(Vec2::new(21.0, 10.0), 2.0, 3, true);
        assert_eq!((fog.width, fog.height, fog.teams.len()), (11, 5, 3));
        assert!(fog.teams.iter().all(|t| t.visible.len() == 55 && t.explored.len() == 55));
    }

    #[test]
    fn team_sees_tests_cells_touching_the_disc() {
        let fog = lit(&[(5, 5)]); // world [10, 12) x [10, 12)
        assert!(team_sees(&fog, 0, Vec2::new(11.0, 11.0), 0.0));
        assert!(!team_sees(&fog, 0, Vec2::new(13.0, 11.0), 0.5));
        assert!(team_sees(&fog, 0, Vec2::new(13.0, 11.0), 1.01));
        // The cell's corner is sqrt(2) away from (13, 13).
        assert!(!team_sees(&fog, 0, Vec2::new(13.0, 13.0), 1.4));
        assert!(team_sees(&fog, 0, Vec2::new(13.0, 13.0), 1.42));
        assert!(!team_sees(&fog, 1, Vec2::new(11.0, 11.0), 3.0));
        assert!(!team_sees(&fog, 7, Vec2::new(11.0, 11.0), 3.0));
        assert!(team_explored(&fog, 0, Vec2::new(11.0, 11.0), 0.0));
    }

    #[test]
    fn disabled_fog_sees_everything() {
        let mut fog = lit(&[]);
        fog.enabled = false;
        assert!(team_sees(&fog, 9, Vec2::new(-50.0, 3.0), 0.0));
        assert!(team_explored(&fog, 0, Vec2::new(5.0, 5.0), 0.0));
    }

    #[test]
    fn cell_rect_clips_to_the_grid() {
        let fog = lit(&[]);
        assert_eq!(fog_cell_rect(&fog, Vec2::new(-5.0, 3.0), Vec2::new(4.1, 99.0)), Some((UVec2::new(0, 1), UVec2::new(2, 9))));
        assert_eq!(fog_cell_rect(&fog, Vec2::new(21.0, 0.0), Vec2::new(30.0, 5.0)), None);
        assert_eq!(fog_cell_rect(&fog, Vec2::splat(f32::NAN), Vec2::splat(3.0)), None);
    }

    #[test]
    fn team_sees_box_tests_the_cells_the_box_touches() {
        let fog = lit(&[(5, 5)]); // world [10, 12) x [10, 12)
        let square = |center: Vec2, half: f32, yaw: f32| Obb::new(center, Vec2::splat(half), yaw);
        assert!(team_sees_box(&fog, 0, &square(Vec2::new(16.0, 11.0), 4.5, 0.0)), "a big box whose near side is lit");
        assert!(!fog.visible(0, Vec2::new(16.0, 11.0)), "while its centre is dark");
        assert!(!team_sees_box(&fog, 0, &square(Vec2::new(16.0, 11.0), 3.9, 0.0)));
        assert!(!team_sees_box(&fog, 1, &square(Vec2::new(11.0, 11.0), 1.0, 0.0)));
        // Rotated 45 degrees, the box's AABB covers the lit cell but the box
        // itself stops short of it.
        let diamond = square(Vec2::new(14.5, 14.5), 2.0, std::f32::consts::FRAC_PI_4);
        let (min, _) = diamond.aabb();
        assert!(min.x < 12.0 && min.y < 12.0, "the AABB reaches the lit cell");
        assert!(!team_sees_box(&fog, 0, &diamond));
        let diamond = square(Vec2::new(13.3, 13.3), 2.0, std::f32::consts::FRAC_PI_4);
        assert!(team_sees_box(&fog, 0, &diamond), "its side crosses the lit corner");
        let mut off = fog.clone();
        off.enabled = false;
        assert!(team_sees_box(&off, 9, &square(Vec2::splat(-50.0), 1.0, 0.0)));
    }

    #[test]
    fn fog_box_visits_each_touched_cell_once() {
        let fog = lit(&[]);
        let mut cells = Vec::new();
        // [9, 13] x [9, 11]: x touches cells 4..=6, y cells 4..=5.
        FogBox::new(&Obb::new(Vec2::new(11.0, 10.0), Vec2::new(2.0, 1.0), 0.0)).any_cell(&fog, |i| {
            cells.push(i);
            false
        });
        let expected: Vec<usize> = [4, 5].iter().flat_map(|y| (4..=6).map(move |x| y * 10 + x)).collect();
        assert_eq!(cells, expected);
    }

    #[test]
    fn sight_radius_covers_vision_beyond_the_box() {
        let footprint = Footprint { half: Vec2::new(3.0, 4.0), height: 1.0 };
        assert_eq!(sight_radius(10.0, Some(&footprint)), 15.0);
        assert_eq!(sight_radius(10.0, None), 10.0);
        assert_eq!(sight_radius(f32::NAN, None), 0.0);
        assert_eq!(sight_radius(-3.0, Some(&Footprint { half: Vec2::splat(f32::NAN), height: 1.0 })), 0.0);
    }

    #[test]
    fn last_seen_keeps_one_sighting_per_team() {
        let data = crate::testing::test_data();
        let mut kinds = data.building_ids();
        let (a, b) = (kinds.next().expect("a building"), kinds.next().expect("two buildings"));
        let foundation = BuildingSighting { kind: a, construction: Some(0.25), hp: 0.3 };
        let upgraded = BuildingSighting { kind: b, construction: None, hp: 1.0 };
        let mut last = LastSeen::default();
        assert_eq!(last.by(0), None);
        assert!(last.is_current(0, &foundation), "nothing to record for no team");
        last.record(team_bit(4) | team_bit(1), foundation);
        assert!(last.is_current(team_bit(1) | team_bit(4), &foundation));
        assert!(!last.is_current(team_bit(1) | team_bit(2), &foundation));
        last.record(team_bit(4), upgraded);
        assert_eq!((last.by(1), last.by(4), last.by(0)), (Some(&foundation), Some(&upgraded), None));
    }

    #[test]
    fn team_bits() {
        assert_eq!(team_bit(0), 1);
        assert_eq!(team_bit(31), 1 << 31);
        assert_eq!(team_bit(32), 0);
        assert_eq!(team_bit(af_geom::NO_TEAM), 0);
        assert_eq!(team_ids(0b1010_0001).collect::<Vec<_>>(), vec![0, 5, 7]);
        assert_eq!(team_ids(u32::MAX).count(), 32);
        assert_eq!(team_ids(0).count(), 0);
        let s = SeenBy { teams: 0b101, visible: 0b100 };
        assert!(s.seen_by(0) && !s.seen_by(1) && s.seen_by(2) && !s.visible_to(0) && s.visible_to(2));
    }
}
