//! Headless fog-of-war behaviour against the real content.

use std::f32::consts::{SQRT_2, TAU};

use af_core::ext::economy::RevealedPlayers;
use af_core::ext::fog::*;
use af_core::prelude::*;
use af_core::testing::*;
use af_fog::{FogPlugin, SIGHT_SLACK_CELLS, VISION_INTERVAL};
use bevy::prelude::*;

const MAP: f32 = 128.0;
const CELL: f32 = 2.0;

/// A flat map with fog on, set up the way af_map does it.
fn fog_app(players: &[(&str, u8)]) -> (App, Vec<PlayerId>) {
    let mut app = sim_app(test_data());
    app.add_plugins(FogPlugin);
    let ids = flat_world(&mut app, MAP, players);
    let slots = players.iter().map(|(_, team)| *team as usize + 1).max().unwrap_or(0);
    app.insert_resource(fog_for_map(Vec2::splat(MAP), CELL, slots, true));
    (app, ids)
}

/// `player`'s worker at `pos` with exactly `vision` metres of sight.
fn scout(app: &mut App, player: PlayerId, pos: Vec2, vision: f32) -> Entity {
    let unit = with_commands(app, |commands, data, players| {
        let p = players.get(player);
        spawn_unit(commands, data, p, data.faction(p.faction).worker_id, pos, 0.0)
    });
    app.world_mut().get_mut::<Stats>(unit).expect("spawned").vision = vision;
    unit
}

/// `player`'s main hall (the faction's first start building) at `pos`.
fn hall(app: &mut App, player: PlayerId, pos: Vec2, complete: bool) -> Entity {
    with_commands(app, |commands, data, players| {
        let p = players.get(player);
        let id = data.faction(p.faction).start.buildings.first().expect("a start building");
        let kind = data.building_id(id).expect("known building");
        spawn_building(commands, data, p, kind, pos, 0.0, complete)
    })
}

fn oak(app: &mut App, pos: Vec2) -> Entity {
    with_commands(app, |commands, data, _| spawn_node(commands, data, data.node_id("oak").expect("oak"), pos, 0.0, None))
}

fn fog(app: &App) -> &FogOfWar {
    app.world().resource::<FogOfWar>()
}

fn seen(app: &App, e: Entity) -> SeenBy {
    *app.world().get::<SeenBy>(e).expect("SeenBy inserted")
}

fn last_seen(app: &App, e: Entity) -> LastSeen {
    app.world().get::<LastSeen>(e).expect("LastSeen inserted").clone()
}

fn ghosts(app: &App) -> &[FogGhost] {
    &app.world().resource::<FogGhosts>().list
}

fn tick(app: &App) -> u64 {
    app.world().resource::<SimClock>().tick
}

fn move_to(app: &mut App, e: Entity, pos: Vec2) {
    app.world_mut().get_mut::<Position>(e).expect("alive").0 = pos;
}

/// How far `e` sees from its centre: its vision beyond its box.
fn reach(app: &App, e: Entity) -> f32 {
    let w = app.world();
    sight_radius(w.get::<Stats>(e).expect("stats").vision, w.get::<Footprint>(e))
}

/// Past this distance from a viewer's centre no point is visible: a lit
/// cell has a point within `reach + slack`, and a cell is at most one
/// diagonal across.
fn beyond(reach: f32) -> f32 {
    reach + SIGHT_SLACK_CELLS * CELL + CELL * SQRT_2
}

fn half(app: &App, e: Entity) -> Vec2 {
    app.world().get::<Footprint>(e).expect("footprint").half
}

/// Kill a building or node the way af_combat (or a gatherer emptying it) does.
fn kill(app: &mut App, e: Entity) {
    let world = app.world_mut();
    let target = world.entity(e);
    let pos = target.get::<Position>().expect("alive").0;
    let owner = target.get::<Owner>().map(|o| o.0);
    let kind = match (target.get::<Building>(), target.get::<ResourceNode>()) {
        (Some(b), _) => EntityKind::Building(b.kind),
        (None, Some(n)) => EntityKind::Node(n.kind),
        (None, None) => panic!("only buildings and nodes are killed here"),
    };
    world.entity_mut(e).insert(Dead);
    world.write_message(EntityDied { entity: e, owner, kind, pos, killer: None });
}

fn cell_centres(fog: &FogOfWar) -> impl Iterator<Item = Vec2> + '_ {
    (0..fog.height).flat_map(move |y| (0..fog.width).map(move |x| (Vec2::new(x as f32, y as f32) + 0.5) * fog.cell))
}

/// `team` sees every cell holding a point within `reach` of `at`, and no
/// cell whose nearest point is farther than `reach` plus the slack.
fn assert_sight(fog: &FogOfWar, team: u8, at: Vec2, reach: f32) {
    let mut lit = 0;
    for y in 0..fog.height {
        for x in 0..fog.width {
            let min = Vec2::new(x as f32, y as f32) * fog.cell;
            let d = at.distance(at.clamp(min, min + fog.cell));
            let visible = fog.visible(team, min + fog.cell * 0.5);
            assert!(visible || d > reach - 1e-3, "from {at}: cell ({x}, {y}), {d} m away, should be visible");
            assert!(!visible || d <= reach + SIGHT_SLACK_CELLS * fog.cell, "from {at}: cell ({x}, {y}), {d} m away, should be dark");
            lit += usize::from(visible);
        }
    }
    assert!(lit > 0);
}

#[test]
fn a_unit_reveals_cells_within_its_vision_and_none_beyond() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    // A cell centre, then spots near each corner of that cell and in between:
    // sight is measured from where the unit really stands.
    let spots = [(41.0, 61.0), (40.02, 60.02), (41.98, 61.98), (40.01, 61.99), (41.99, 60.01), (41.37, 60.6)].map(Vec2::from);
    let unit = scout(&mut app, p[0], spots[0], 10.0);
    run_ticks(&mut app, 1);

    let r = reach(&app, unit);
    assert_sight(fog(&app), 0, spots[0], r);
    for c in cell_centres(fog(&app)) {
        assert_eq!(fog(&app).explored(0, c), fog(&app).visible(0, c), "explored follows visible at {c}");
        assert!(!fog(&app).visible(1, c) && !fog(&app).explored(1, c), "the enemy sees nothing at {c}");
    }
    for at in &spots[1..] {
        move_to(&mut app, unit, *at);
        run_ticks(&mut app, VISION_INTERVAL as u32);
        assert_sight(fog(&app), 0, *at, r);
    }
}

#[test]
fn a_box_within_vision_of_the_viewers_box_is_seen_in_every_direction() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let (at, vision) = (Vec2::new(60.02, 60.02), 10.0);
    let unit = scout(&mut app, p[0], at, vision);
    run_ticks(&mut app, 1);

    let viewer = half(&app, unit);
    let far = beyond(reach(&app, unit));
    let fog = fog(&app);
    // An infantryman, a town-centre-sized box and a long one.
    for target in [Vec2::splat(0.35), Vec2::splat(4.5), Vec2::new(1.5, 0.5)] {
        for i in 0..72 {
            let dir = Vec2::from_angle(i as f32 * TAU / 72.0);
            // Both boxes are axis-aligned, so their gap is the length of
            // the per-axis separations.
            let at_gap = |gap: f32| {
                let sep = dir * gap;
                Obb::new(at + sep.signum() * (sep.abs() + viewer + target), target, 0.0)
            };
            let near = at_gap(vision - 0.01);
            assert!(team_sees_box(fog, 0, &near), "{target} box {vision} m off the viewer towards {dir}");
            assert!(!team_sees_box(fog, 1, &near), "the enemy does not see it");
            assert!(!team_sees_box(fog, 0, &at_gap(far + 0.01)), "{target} box out of sight towards {dir}");
            if target.x > 4.0 {
                assert!(!fog.visible(0, near.center), "a point test on a big box's centre misses it");
            }
        }
    }
}

#[test]
fn allies_share_vision_and_enemies_do_not() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("sylvan", 0), ("dune", 1)]);
    let (a, b, c) = (Vec2::new(25.0, 25.0), Vec2::new(101.0, 101.0), Vec2::new(25.0, 101.0));
    scout(&mut app, p[0], a, 8.0);
    scout(&mut app, p[1], b, 8.0);
    scout(&mut app, p[2], c, 8.0);
    run_ticks(&mut app, 1);

    let fog = fog(&app);
    assert!(fog.visible(0, a) && fog.visible(0, b), "team 0 sees through both allies");
    assert!(!fog.visible(0, c), "team 0 does not see the enemy's area");
    assert!(fog.visible(1, c) && !fog.visible(1, a) && !fog.visible(1, b), "team 1 sees only its own unit's area");
}

#[test]
fn explored_ground_stays_explored_after_the_unit_leaves() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let (a, b) = (Vec2::new(21.0, 21.0), Vec2::new(105.0, 105.0));
    let unit = scout(&mut app, p[0], a, 8.0);
    run_ticks(&mut app, 1);
    move_to(&mut app, unit, b);
    run_ticks(&mut app, VISION_INTERVAL as u32);

    let fog = fog(&app);
    assert!(!fog.visible(0, a) && fog.explored(0, a), "left behind: explored but dark");
    assert!(fog.visible(0, b) && fog.explored(0, b));
    assert!(!fog.explored(0, Vec2::new(21.0, 105.0)), "never visited");
}

#[test]
fn vision_refreshes_on_the_first_tick_then_every_interval() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let (a, b) = (Vec2::new(21.0, 21.0), Vec2::new(105.0, 105.0));
    let unit = scout(&mut app, p[0], a, 8.0);
    run_ticks(&mut app, 1);
    assert!(fog(&app).visible(0, a), "the first tick computes vision");

    move_to(&mut app, unit, b);
    run_ticks(&mut app, VISION_INTERVAL as u32 - 1);
    assert!(fog(&app).visible(0, a) && !fog(&app).visible(0, b), "no update between intervals");
    run_ticks(&mut app, 1);
    assert!(!fog(&app).visible(0, a) && fog(&app).visible(0, b), "updated on the interval");
}

#[test]
fn dead_units_give_no_vision_and_foundations_do() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let (a, site) = (Vec2::new(21.0, 21.0), Vec2::new(85.0, 85.0));
    let corpse = scout(&mut app, p[0], a, 8.0);
    app.world_mut().entity_mut(corpse).insert(Dead);
    let foundation = hall(&mut app, p[0], site, false);
    run_ticks(&mut app, 1);

    let r = reach(&app, foundation);
    let fog = fog(&app);
    assert!(!fog.visible(0, a), "a dead unit sees nothing");
    assert!(fog.visible(0, site), "a foundation sees");
    // Sight is measured from the walls: the hall sees `vision` beyond its
    // box, corners included.
    for i in 0..16 {
        let dir = Vec2::from_angle(i as f32 * TAU / 16.0);
        assert!(fog.visible(0, site + dir * (r - 0.05)), "{dir}");
        assert!(!fog.visible(0, site + dir * (beyond(r) + 0.05)), "{dir}");
    }
}

#[test]
fn disabled_fog_reports_everything_visible_and_does_no_work() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    app.world_mut().resource_mut::<FogOfWar>().enabled = false;
    scout(&mut app, p[0], Vec2::new(21.0, 21.0), 8.0);
    let enemy_hall = hall(&mut app, p[1], Vec2::new(100.0, 100.0), true);
    run_ticks(&mut app, 1);

    let fog = fog(&app);
    assert!(fog.visible(0, Vec2::new(100.0, 100.0)) && fog.explored(1, Vec2::new(3.0, 120.0)));
    assert!(team_sees(fog, 0, Vec2::new(100.0, 100.0), 0.0) && team_sees(fog, 9, Vec2::ZERO, 0.0));
    assert!(fog.teams.iter().all(|t| t.visible.iter().all(|v| *v == 0)), "grids are not computed");
    assert_eq!(seen(&app, enemy_hall), SeenBy { teams: SeenBy::ALL, visible: SeenBy::ALL });
}

#[test]
fn a_revealed_map_is_visible_and_explored_everywhere() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    app.insert_resource(MatchSetup { reveal_map: true, ..default() });
    let enemy_hall = hall(&mut app, p[1], Vec2::new(100.0, 100.0), true);
    run_ticks(&mut app, 1 + VISION_INTERVAL as u32);

    let fog = fog(&app);
    for c in cell_centres(fog) {
        assert!(fog.visible(0, c) && fog.visible(1, c) && fog.explored(0, c) && fog.explored(1, c), "{c}");
    }
    assert_eq!(seen(&app, enemy_hall), SeenBy { teams: SeenBy::ALL, visible: SeenBy::ALL });
}

#[test]
fn a_fog_without_dimensions_is_sized_from_the_map() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 3)]);
    app.insert_resource(FogOfWar { enabled: true, ..default() });
    let at = Vec2::new(21.0, 21.0);
    scout(&mut app, p[1], at, 8.0);
    run_ticks(&mut app, 1);

    let fog = fog(&app);
    let side = (MAP / fog.cell).ceil() as u32;
    assert_eq!((fog.width, fog.height, fog.teams.len()), (side, side, 4));
    assert!(fog.visible(3, at) && !fog.visible(0, at));
}

#[test]
fn seen_by_is_set_once_a_building_is_seen_and_kept_after() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let site = Vec2::new(100.0, 100.0);
    let enemy_hall = hall(&mut app, p[1], site, true);
    let home = Vec2::new(20.0, 20.0);
    let unit = scout(&mut app, p[0], home, 8.0);
    let tree = oak(&mut app, home + Vec2::new(4.0, 0.0));
    run_ticks(&mut app, 1);

    assert_eq!(seen(&app, enemy_hall), SeenBy { teams: team_bit(1), visible: team_bit(1) }, "only its owner sees it");
    assert_eq!(seen(&app, tree), SeenBy { teams: team_bit(0), visible: team_bit(0) });

    let lookout = site + Vec2::new(half(&app, enemy_hall).x + 5.0, 0.0);
    move_to(&mut app, unit, lookout);
    run_ticks(&mut app, VISION_INTERVAL as u32);
    let s = seen(&app, enemy_hall);
    assert!(s.seen_by(0) && s.visible_to(0), "scouted: {s:?}");

    move_to(&mut app, unit, home);
    run_ticks(&mut app, VISION_INTERVAL as u32);
    let s = seen(&app, enemy_hall);
    assert!(s.seen_by(0) && !s.visible_to(0), "remembered but out of sight: {s:?}");
    assert!(s.seen_by(1) && s.visible_to(1), "its owner still sees it");
}

#[test]
fn every_team_sees_the_buildings_of_a_revealed_player() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1), ("tide", 2)]);
    let enemy_hall = hall(&mut app, p[1], Vec2::new(100.0, 100.0), true);
    let enemy_worker = scout(&mut app, p[1], Vec2::new(100.0, 80.0), 8.0);
    let tree = oak(&mut app, Vec2::new(100.0, 120.0));
    scout(&mut app, p[0], Vec2::new(20.0, 20.0), 8.0);
    scout(&mut app, p[2], Vec2::new(20.0, 110.0), 8.0);
    run_ticks(&mut app, 1);
    assert_eq!(seen(&app, enemy_hall), SeenBy { teams: team_bit(1), visible: team_bit(1) });

    app.insert_resource(RevealedPlayers { list: vec![p[1]] });
    run_ticks(&mut app, VISION_INTERVAL as u32);
    let all = team_bit(0) | team_bit(1) | team_bit(2);
    assert_eq!(seen(&app, enemy_hall), SeenBy { teams: all, visible: all }, "revealed to every team");
    assert!(last_seen(&app, enemy_hall).by(0).is_some() && last_seen(&app, enemy_hall).by(2).is_some(), "and remembered as seen");
    assert!(!seen(&app, tree).seen_by(0), "nodes are nobody's: not revealed");
    let worker_pos = app.world().get::<Position>(enemy_worker).expect("alive").0;
    assert!(!fog(&app).visible(0, worker_pos), "only buildings are revealed, not the ground");

    // Destroyed while revealed, it is seen going: no ghost.
    kill(&mut app, enemy_hall);
    run_ticks(&mut app, 1);
    assert!(ghosts(&app).is_empty(), "{:?}", ghosts(&app));
}

#[test]
fn a_reveal_lifts_when_the_player_is_hidden_again() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let enemy_hall = hall(&mut app, p[1], Vec2::new(100.0, 100.0), true);
    scout(&mut app, p[0], Vec2::new(20.0, 20.0), 8.0);
    app.insert_resource(RevealedPlayers { list: vec![p[1]] });
    run_ticks(&mut app, 1);
    assert!(seen(&app, enemy_hall).visible_to(0));
    app.world_mut().resource_mut::<RevealedPlayers>().set(p[1], false);
    run_ticks(&mut app, VISION_INTERVAL as u32);
    let s = seen(&app, enemy_hall);
    assert!(s.seen_by(0) && !s.visible_to(0), "remembered, no longer in sight: {s:?}");
}

/// An enemy hall at (100, 100) with a tree beside it, both seen by team 0's
/// scout, which then walked home out of sight.
struct Scouted {
    hall: Entity,
    tree: Entity,
    scout: Entity,
    lookout: Vec2,
    home: Vec2,
    /// `SimClock::tick` of the first vision update; later ones follow every
    /// `VISION_INTERVAL` ticks.
    first_update: u64,
}

impl Scouted {
    /// Did the latest tick run a vision update?
    fn updated_this_tick(&self, app: &App) -> bool {
        (tick(app) - self.first_update).is_multiple_of(VISION_INTERVAL)
    }
}

fn scout_and_leave(app: &mut App, p: &[PlayerId]) -> Scouted {
    let site = Vec2::new(100.0, 100.0);
    let hall = hall(app, p[1], site, true);
    let lookout = site + Vec2::new(half(app, hall).x + 5.0, 0.0);
    let tree = oak(app, lookout + Vec2::new(0.0, 4.0));
    let scout = scout(app, p[0], lookout, 8.0);
    run_ticks(app, 1);
    let first_update = tick(app);
    let home = Vec2::new(20.0, 20.0);
    move_to(app, scout, home);
    run_ticks(app, VISION_INTERVAL as u32);
    assert!(seen(app, hall).seen_by(0) && !seen(app, hall).visible_to(0));
    assert!(seen(app, tree).seen_by(0) && !seen(app, tree).visible_to(0));
    Scouted { hall, tree, scout, lookout, home, first_update }
}

#[test]
fn buildings_and_nodes_destroyed_out_of_sight_leave_ghosts_until_rescouted() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let s = scout_and_leave(&mut app, &p);
    let hall_kind = app.world().get::<Building>(s.hall).expect("hall").kind;
    let hall_pos = app.world().get::<Position>(s.hall).expect("hall").0;
    let tree_kind = app.world().get::<ResourceNode>(s.tree).expect("tree").kind;

    kill(&mut app, s.hall);
    kill(&mut app, s.tree);
    run_ticks(&mut app, 1);
    assert!(app.world().get_entity(s.hall).is_err(), "despawned in Cleanup");
    assert_eq!(ghosts(&app).len(), 2);
    let (hall, tree) = (ghosts(&app)[0], ghosts(&app)[1]);
    assert_eq!((hall.entity, hall.kind, hall.owner, hall.pos), (s.hall, EntityKind::Building(hall_kind), Some(p[1]), hall_pos));
    assert_eq!((tree.entity, tree.kind, tree.owner), (s.tree, EntityKind::Node(tree_kind), None));
    // Team 1's hall was watching both when they fell.
    assert_eq!((hall.teams, tree.teams), (team_bit(0), team_bit(0)));
    assert_eq!(app.world().resource::<FogGhosts>().remembered_by(0).count(), 2);

    move_to(&mut app, s.scout, s.lookout);
    run_ticks(&mut app, VISION_INTERVAL as u32);
    assert!(ghosts(&app).is_empty(), "looking again dispels the ghosts");
}

#[test]
fn a_building_destroyed_in_plain_sight_leaves_no_ghost() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let s = scout_and_leave(&mut app, &p);
    // A tree the scout passes at home, then leaves out of sight: it dies
    // with the hall and must leave a ghost, so the check below cannot pass
    // just because no ghost was made at all.
    let home_tree = oak(&mut app, s.home + Vec2::new(0.0, 4.0));
    run_ticks(&mut app, VISION_INTERVAL as u32);
    move_to(&mut app, s.scout, s.lookout);
    run_ticks(&mut app, VISION_INTERVAL as u32);
    assert!(seen(&app, s.hall).visible_to(0), "the scout is watching the hall");
    assert!(seen(&app, home_tree).seen_by(0) && !seen(&app, home_tree).visible_to(0));

    kill(&mut app, s.hall);
    kill(&mut app, home_tree);
    run_ticks(&mut app, 1);
    // No vision update ran this tick, so nothing could have dispelled a
    // ghost made by mistake.
    assert!(!s.updated_this_tick(&app));
    let remembered: Vec<Entity> = ghosts(&app).iter().map(|g| g.entity).collect();
    assert_eq!(remembered, vec![home_tree], "only the unwatched tree is remembered");
}

#[test]
fn a_building_is_remembered_as_it_was_last_seen() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let site = Vec2::new(100.0, 100.0);
    let foundation = hall(&mut app, p[1], site, false);
    let lookout = site + Vec2::new(half(&app, foundation).x + 5.0, 0.0);
    let home = Vec2::new(20.0, 20.0);
    let unit = scout(&mut app, p[0], lookout, 8.0);
    run_ticks(&mut app, 1);

    let started = app.world().get::<Building>(foundation).expect("building").kind;
    let sighting = *last_seen(&app, foundation).by(0).expect("team 0 saw the foundation");
    let fresh_hp = app.world().get::<Health>(foundation).expect("health").fraction();
    assert_eq!(sighting, BuildingSighting { kind: started, construction: Some(0.0), hp: fresh_hp });
    assert_eq!(last_seen(&app, foundation).by(1), None, "its owner's team is not recorded");

    // Out of sight it is finished, upgraded and damaged; team 0 still
    // remembers the fresh foundation.
    move_to(&mut app, unit, home);
    run_ticks(&mut app, VISION_INTERVAL as u32);
    let upgraded = test_data().building_ids().find(|k| *k != started).expect("a second building type");
    let rebuild = |app: &mut App, kind: BuildingTypeId, hp: f32| {
        let mut e = app.world_mut().entity_mut(foundation);
        e.remove::<UnderConstruction>();
        e.get_mut::<Building>().expect("building").kind = kind;
        let mut health = e.get_mut::<Health>().expect("health");
        health.hp = health.max * hp;
    };
    rebuild(&mut app, upgraded, 0.5);
    run_ticks(&mut app, VISION_INTERVAL as u32);
    assert_eq!(last_seen(&app, foundation).by(0), Some(&sighting));

    // Looking again refreshes the memory.
    move_to(&mut app, unit, lookout);
    run_ticks(&mut app, VISION_INTERVAL as u32);
    let now = BuildingSighting { kind: upgraded, construction: None, hp: 0.5 };
    assert_eq!(last_seen(&app, foundation).by(0), Some(&now));

    // Changed again and destroyed out of sight: the ghost is what team 0 saw.
    move_to(&mut app, unit, home);
    run_ticks(&mut app, VISION_INTERVAL as u32);
    rebuild(&mut app, started, 0.9);
    kill(&mut app, foundation);
    run_ticks(&mut app, 1);
    let ghosts = ghosts(&app);
    assert_eq!(ghosts.len(), 1);
    assert_eq!((ghosts[0].kind, ghosts[0].teams), (EntityKind::Building(upgraded), team_bit(0)));
}

#[test]
fn ghosts_written_by_other_systems_are_dispelled_too() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let unit = scout(&mut app, p[0], Vec2::new(20.0, 20.0), 8.0);
    run_ticks(&mut app, 1);
    let spot = Vec2::new(70.0, 70.0);
    let oak_kind = test_data().node_id("oak").expect("oak");
    app.world_mut().resource_mut::<FogGhosts>().list.push(FogGhost {
        entity: Entity::from_raw_u32(4242).expect("valid index"),
        kind: EntityKind::Node(oak_kind),
        owner: None,
        pos: spot,
        facing: 0.3,
        footprint: Footprint { half: Vec2::splat(1.0), height: 4.0 },
        teams: team_bit(0),
    });
    run_ticks(&mut app, VISION_INTERVAL as u32);
    assert_eq!(ghosts(&app).len(), 1, "out of sight: still believed");

    move_to(&mut app, unit, spot + Vec2::new(5.0, 0.0));
    run_ticks(&mut app, VISION_INTERVAL as u32);
    assert!(ghosts(&app).is_empty(), "seen again: forgotten");
}

#[test]
fn fog_memory_is_forgotten_when_the_match_ends() {
    let (mut app, p) = fog_app(&[("ironhold", 0), ("dune", 1)]);
    let s = scout_and_leave(&mut app, &p);
    kill(&mut app, s.hall);
    run_ticks(&mut app, 1);
    assert_eq!(ghosts(&app).len(), 1);

    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Menu);
    app.update();
    assert!(ghosts(&app).is_empty());
}
