//! Placement checks shared by the preview and the placing click, using the
//! same `check_placement` the economy runs authoritatively, judged from
//! what the local team knows of the map: with the fog on, buildings and
//! nodes it has never seen do not show through the ghost.

use af_core::ext::fog::{FogGhosts, SeenBy};
use af_core::ext::input::{PlacementGhost, PlacementPreview};
use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// What placement validity depends on.
#[derive(SystemParam)]
pub(crate) struct PlacementWorld<'w, 's> {
    data: Res<'w, Data>,
    nav: Res<'w, Nav>,
    spatial: Res<'w, SpatialIndex>,
    players: Res<'w, Players>,
    fog: Res<'w, FogOfWar>,
    ghosts: Option<Res<'w, FogGhosts>>,
    seen: Query<'w, 's, &'static SeenBy>,
}

impl<'w, 's> PlacementWorld<'w, 's> {
    /// Placement context for player `p` (`None` if no such player), judged
    /// from what `p`'s team knows while the fog is on.
    pub fn context(&self, p: PlayerId) -> Option<PlaceCtx<'_, 'w, 's>> {
        let player = self.players.try_get(p)?;
        let known = self.fog.enabled.then(|| {
            let team = self.players.team(p);
            let remembered = self.ghosts.iter().flat_map(|g| g.remembered_by(team)).map(|g| g.footprint.obb(g.pos, g.facing)).collect();
            Known { me: p, team, players: &self.players, seen: &self.seen, remembered }
        });
        Some(PlaceCtx { data: &self.data, nav: &self.nav.0, spatial: &self.spatial, player, known })
    }
}

/// What the local team knows of the map.
struct Known<'a, 'w, 's> {
    me: PlayerId,
    team: u8,
    players: &'a Players,
    seen: &'a Query<'w, 's, &'static SeenBy>,
    /// Footprints of dead buildings and nodes the team still believes stand
    /// (`FogGhosts`).
    remembered: Vec<Obb>,
}

impl Known<'_, '_, '_> {
    /// Own and allied buildings, and whatever the team has seen (`SeenBy`;
    /// a missing one means seen by nobody).
    fn knows(&self, e: &SpatialEntry) -> bool {
        e.owner.is_some_and(|o| self.players.allied(self.me, o)) || self.seen.get(e.entity).is_ok_and(|s| s.seen_by(self.team))
    }
}

pub(crate) struct PlaceCtx<'a, 'w, 's> {
    data: &'a GameData,
    nav: &'a NavGrid,
    spatial: &'a SpatialIndex,
    player: &'a PlayerState,
    /// `None` with the fog off: everything is known.
    known: Option<Known<'a, 'w, 's>>,
}

impl PlaceCtx<'_, '_, '_> {
    pub fn data(&self) -> &GameData {
        self.data
    }

    /// Where `kind` would stand at `pos` (snapped onto its node when it
    /// needs one), or why it cannot, leaving the cost aside. `extra` are
    /// footprints placed earlier in the same batch ([`placed_footprint`]s).
    fn spot(&self, kind: BuildingTypeId, pos: Vec2, facing: f32, extra: &[Obb]) -> Result<Placement, PlacementError> {
        match &self.known {
            None => check_placement(self.data, self.nav, self.spatial, self.player, kind, pos, facing, false, extra),
            Some(k) => {
                let site = Site { data: self.data, nav: self.nav, spatial: self.spatial, player: self.player };
                check_known_placement(&site, kind, pos, facing, extra, |e| k.knows(e), &k.remembered)
            }
        }
    }
}

/// The world a placement is checked in.
#[derive(Clone, Copy)]
struct Site<'a> {
    data: &'a GameData,
    nav: &'a NavGrid,
    spatial: &'a SpatialIndex,
    player: &'a PlayerState,
}

/// [`check_placement`] without the cost, as a team that knows only part of
/// the map judges it, so the preview gives away nothing the fog hides:
/// buildings and nodes failing `knows` neither block the spot nor take a
/// node building's snap, and the nav cells they occupy count by their
/// terrain alone. `remembered` footprints block like live buildings. With
/// `knows` always true and nothing remembered, the answer is
/// `check_placement`'s; af_economy runs that full check when the order
/// arrives.
fn check_known_placement(
    site: &Site,
    kind: BuildingTypeId,
    pos: Vec2,
    facing: f32,
    extra: &[Obb],
    knows: impl Fn(&SpatialEntry) -> bool,
    remembered: &[Obb],
) -> Result<Placement, PlacementError> {
    let Site { data, nav, spatial, player } = *site;
    if !(pos.is_finite() && facing.is_finite()) {
        return Err(PlacementError::OutOfBounds);
    }
    rules::can_build(data, player, kind, false).map_err(PlacementError::Rule)?;
    let (mut pos, mut facing, mut node) = (pos, facing, None);
    if let Some(req) = data.building(kind).requires_node_id {
        let found = spatial.nearest(pos, 4.0, |e| e.kind == SpatialKind::Node && e.type_index == req.0 && knows(e));
        let Some(n) = found else { return Err(PlacementError::NeedsNode) };
        (pos, facing, node) = (n.pos, n.obb.yaw, Some(n.entity));
        let claimed = spatial
            .within(pos, 0.5)
            .iter()
            .any(|e| e.kind == SpatialKind::Building && e.pos.distance(pos) < 0.5 && knows(e));
        if claimed {
            return Err(PlacementError::NodeClaimed);
        }
    }
    let obb = placed_footprint(data, kind, pos, facing);
    let (min, max) = obb.aabb();
    let size = nav.world_size() + nav.origin();
    if min.x < nav.origin().x || min.y < nav.origin().y || max.x > size.x || max.y > size.y {
        return Err(PlacementError::OutOfBounds);
    }
    // Known buildings and nodes block by overlap; unknown obstacles only
    // hide the nav cells they cover. The search reaches a cell further, for
    // an obstacle rasterised into the cell under its centre.
    let mut unknown = Vec::new();
    let mut blocked = false;
    spatial.for_each_near(pos, obb.bounding_radius() + nav.cell_size(), |e| {
        if e.kind == SpatialKind::Unit || Some(e.entity) == node {
            return;
        }
        if !knows(e) {
            if e.blocking {
                unknown.push(e.obb);
            }
        } else if e.obb.overlaps(&obb) {
            blocked = true;
        }
    });
    if !cells_buildable(data, nav, &obb, &unknown) || blocked || extra.iter().chain(remembered).any(|o| o.overlaps(&obb)) {
        return Err(PlacementError::Blocked);
    }
    Ok(Placement { pos, facing, node })
}

/// `NavGrid::area_buildable`, except that cells the obstacles in `unknown`
/// occupy (as the grid rasterises them: centre inside the box, plus the
/// cell under its centre) count as free when their terrain is buildable.
fn cells_buildable(data: &GameData, nav: &NavGrid, obb: &Obb, unknown: &[Obb]) -> bool {
    if unknown.is_empty() {
        return nav.area_buildable(obb);
    }
    let cells = nav.cells_covered(obb, 0.0);
    let (min, max) = obb.aabb();
    let end = nav.origin() + nav.world_size();
    if cells.is_empty() || min.x < nav.origin().x || min.y < nav.origin().y || max.x > end.x || max.y > end.y {
        return false;
    }
    let hidden = |c: IVec2| unknown.iter().any(|o| o.contains(nav.cell_center(c)) || nav.cell_of(o.center) == c);
    cells.iter().all(|&c| {
        let terrain = data.terrain.get(nav.terrain_at(c) as usize).is_some_and(|t| t.buildable);
        let occupied = nav.is_blocked(c) || nav.gate_team(c).is_some();
        terrain && (!occupied || hidden(c))
    })
}

/// The ghost of one building at `pos`: placed where the economy would put
/// it, with the first reason it would refuse. `check_placement` checks the
/// cost last, so a ghost that is only unaffordable still sits at its
/// snapped spot.
pub(crate) fn plan_building(ctx: &PlaceCtx, kind: BuildingTypeId, pos: Vec2, facing: f32) -> PlacementGhost {
    match ctx.spot(kind, pos, facing, &[]) {
        Ok(p) => {
            let error = rules::can_build(ctx.data, ctx.player, kind, true).err().map(PlacementError::Rule);
            PlacementGhost { kind, pos: p.pos, facing: p.facing, error }
        }
        Err(e) => PlacementGhost { kind, pos, facing, error: Some(e) },
    }
}

/// Append the ghosts of a wall line `from` -> `to` to `out`, mirroring how
/// af_economy lays it: segments in order, each checked against the ones
/// accepted before it and paid for as it is placed. The first rule refusal
/// (resources, build limit, age...) stops the line: every later segment
/// whose spot is fine carries it. A blocked segment only leaves a gap and
/// keeps its own reason.
pub(crate) fn plan_wall(ctx: &PlaceCtx, kind: BuildingTypeId, from: Vec2, to: Vec2, out: &mut Vec<PlacementGhost>) {
    let def = ctx.data.building(kind);
    let cost = stats::building_cost(ctx.data, ctx.player, kind);
    let mut left = ctx.player.resources;
    let mut built = ctx.player.buildings_any.get(kind.index()).copied().unwrap_or(0);
    let mut refused: Option<PlacementError> = None;
    let mut accepted: Vec<Obb> = Vec::new();
    for (pos, yaw) in wall_segments(ctx.data, kind, from, to) {
        let ghost = match ctx.spot(kind, pos, yaw, &accepted) {
            Err(e) => {
                if matches!(e, PlacementError::Rule(_)) {
                    refused = refused.or(Some(e));
                }
                PlacementGhost { kind, pos, facing: yaw, error: Some(e) }
            }
            Ok(p) => {
                // The rules that change along the line, in `rules::can_build` order.
                refused = refused.or_else(|| {
                    let reason = if def.limit.is_some_and(|limit| built >= limit) {
                        Some(Reason::LimitReached)
                    } else if !left.covers(&cost) {
                        Some(Reason::CannotAfford)
                    } else {
                        None
                    };
                    reason.map(PlacementError::Rule)
                });
                if refused.is_none() {
                    left.sub_all(&cost);
                    built += 1;
                    accepted.push(placed_footprint(ctx.data, kind, p.pos, p.facing));
                }
                PlacementGhost { kind, pos: p.pos, facing: p.facing, error: refused }
            }
        };
        out.push(ghost);
    }
}

/// Where a wall line of `segments` segments laid from `from` towards `to` ends.
pub(crate) fn wall_end(data: &GameData, kind: BuildingTypeId, from: Vec2, to: Vec2, segments: usize) -> Vec2 {
    let length = data.building(kind).size[0].max(0.5);
    from + (to - from).normalize_or_zero() * length * segments as f32
}

/// Recompute `PlacementPreview` for the current mode and cursor.
pub(crate) fn placement_preview(
    mode: Res<InputMode>,
    cursor: Res<CursorWorld>,
    local: Res<LocalPlayer>,
    world: PlacementWorld,
    mut preview: ResMut<PlacementPreview>,
    mut scratch: Local<Vec<PlacementGhost>>,
) {
    scratch.clear();
    let placing = matches!(*mode, InputMode::Place { .. } | InputMode::PlaceWall { .. });
    if let (true, Some(me), Some(ground)) = (placing, local.0, cursor.ground)
        && let Some(ctx) = world.context(me)
    {
        match *mode {
            InputMode::Place { kind, facing } => scratch.push(plan_building(&ctx, kind, ground, facing)),
            InputMode::PlaceWall { kind, start: None } => scratch.push(plan_building(&ctx, kind, ground, 0.0)),
            InputMode::PlaceWall { kind, start: Some(from) } => plan_wall(&ctx, kind, from, ground, &mut scratch),
            InputMode::Normal | InputMode::Target(_) => {}
        }
    }
    // Swap instead of reallocating, and only flag a change when there is one.
    if preview.ghosts != *scratch {
        std::mem::swap(&mut preview.ghosts, &mut *scratch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_end_follows_the_segments() {
        let data = af_core::testing::test_data();
        let wall = data.buildings.iter().position(|b| b.wall && !b.gate).map(|i| BuildingTypeId(i as u16)).unwrap();
        let length = data.building(wall).size[0];
        let from = Vec2::new(10.0, 10.0);
        let to = Vec2::new(10.0 + length * 3.2, 10.0);
        let n = wall_segments(&data, wall, from, to).len();
        assert_eq!(n, 3);
        let end = wall_end(&data, wall, from, to, n);
        assert!((end - Vec2::new(10.0 + length * 3.0, 10.0)).length() < 1e-4, "{end}");
    }

    /// A 64 m grass map with a strip of water, a building of player 1 at
    /// (30, 30) and a blocking node of the first node type at (42, 30), both
    /// rasterised into the grid as af_movement does and indexed.
    struct Fixture {
        data: std::sync::Arc<GameData>,
        nav: NavGrid,
        spatial: SpatialIndex,
        player: PlayerState,
        house: BuildingTypeId,
        theirs: Entity,
        rock: Entity,
    }

    fn fixture() -> Fixture {
        let data = af_core::testing::test_data();
        let grass = data.terrain_id("grass").unwrap();
        let water = data.terrain_ids().find(|t| !data.terrain_def(*t).buildable).expect("unbuildable terrain");
        let mut nav = af_core::testing::new_nav(&data, 64, 64, grass);
        for y in 0..64 {
            nav.set_terrain(IVec2::new(20, y), water.0);
        }
        let faction = data.faction_id("ironhold").unwrap();
        let player = PlayerState::new(PlayerId(0), "p0".into(), faction, [1.0; 3], 0, Controller::Inert, &data);
        let house = rules::build_menu(&data, &player)
            .into_iter()
            .find(|b| {
                let d = data.building(*b);
                d.has_tag(Tag::House) && !d.wall && d.requires_node_id.is_none()
            })
            .unwrap();
        let mut world = World::new();
        let (theirs, rock) = (world.spawn_empty().id(), world.spawn_empty().id());
        let mut spatial = SpatialIndex::new(Vec2::splat(64.0), 8.0);
        let entry = |entity, kind, type_index, obb: Obb, owner| SpatialEntry {
            entity,
            kind,
            type_index,
            pos: obb.center,
            obb,
            radius: obb.bounding_radius(),
            owner,
            team: owner.map_or(af_geom::NO_TEAM, |o: PlayerId| o.0),
            flying: false,
            altitude: 0.0,
            height: 3.0,
            tags: TagSet::default(),
            under_construction: false,
            blocking: true,
        };
        let building = Obb::from_size(Vec2::splat(30.0), data.building(house).size, 0.3);
        nav.add_obstacle(&building, 1);
        spatial.insert(entry(theirs, SpatialKind::Building, house.0, building, Some(PlayerId(1))));
        let node = Obb::new(Vec2::new(42.0, 30.0), Vec2::splat(1.5), 0.0);
        nav.add_obstacle(&node, 1);
        spatial.insert(entry(rock, SpatialKind::Node, 0, node, None));
        Fixture { data, nav, spatial, player, house, theirs, rock }
    }

    impl Fixture {
        fn site(&self) -> Site<'_> {
            Site { data: &self.data, nav: &self.nav, spatial: &self.spatial, player: &self.player }
        }
    }

    #[test]
    fn knowing_everything_gives_the_economy_s_answer() {
        let f = fixture();
        let mut checked = [0usize; 2];
        for yaw in [0.0, std::f32::consts::FRAC_PI_4, 0.5] {
            for y in 0..32 {
                for x in 0..32 {
                    let pos = Vec2::new(x as f32, y as f32) * 2.0 + Vec2::splat(0.25);
                    let full = check_placement(&f.data, &f.nav, &f.spatial, &f.player, f.house, pos, yaw, false, &[]);
                    let known = check_known_placement(&f.site(), f.house, pos, yaw, &[], |_| true, &[]);
                    assert_eq!(known, full, "at {pos} turned {yaw}");
                    checked[usize::from(full.is_ok())] += 1;
                }
            }
        }
        assert!(checked[0] > 100 && checked[1] > 100, "both outcomes covered: {checked:?}");
    }

    #[test]
    fn unknown_obstacles_free_their_cells_but_not_the_terrain() {
        let f = fixture();
        let site = f.site();
        let on_theirs = Vec2::splat(30.0);
        let full = check_placement(&f.data, &f.nav, &f.spatial, &f.player, f.house, on_theirs, 0.0, false, &[]);
        assert_eq!(full, Err(PlacementError::Blocked), "the economy refuses");
        let unseen = |e: &SpatialEntry| e.entity != f.theirs && e.entity != f.rock;
        assert!(check_known_placement(&site, f.house, on_theirs, 0.0, &[], unseen, &[]).is_ok(), "an unseen building does not show");
        assert!(check_known_placement(&site, f.house, Vec2::new(42.0, 30.0), 0.0, &[], unseen, &[]).is_ok(), "nor an unseen node");
        let only_the_node = |e: &SpatialEntry| e.entity == f.rock;
        assert_eq!(check_known_placement(&site, f.house, Vec2::new(42.0, 30.0), 0.0, &[], only_the_node, &[]), Err(PlacementError::Blocked));
        assert_eq!(check_known_placement(&site, f.house, Vec2::new(20.5, 40.0), 0.0, &[], unseen, &[]), Err(PlacementError::Blocked), "water still counts");
        let remembered = [Obb::from_size(on_theirs, f.data.building(f.house).size, 0.0)];
        assert_eq!(check_known_placement(&site, f.house, on_theirs, 0.0, &[], unseen, &remembered), Err(PlacementError::Blocked), "a remembered footprint blocks");
    }
}
