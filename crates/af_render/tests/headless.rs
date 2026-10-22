//! The renderer driven headless: real plugin, real content, no GPU.

use af_core::ext::fog::{BuildingSighting, FogGhost, FogGhosts, LastSeen, SeenBy, team_bit};
use af_core::ext::input::{PlacementGhost, PlacementPreview, ShowPaths};
use af_core::ext::render::{MAX_GROUND_OFFSET, RoutesShown, terrain_ground};
use af_core::prelude::*;
use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};
use af_render::BoxRenderPlugin;
use af_render::assets::BoxAssets;
use af_render::entities::{BoxVisual, VisualSig};
use af_render::fog::{FogMemory, MemoryPool};
use af_render::overlay::{BarParts, OutlineParts};
use af_render::parts::{BarSpec, min_line_width, node_scale, outline_boxes, progress_transform};
use af_render::paths::{LegKind, MAX_ROUTES, PathOverlay};
use af_render::scene::RenderScene;
use af_render::terrain::{TerrainBox, terrain_top};
use bevy::prelude::*;

struct World {
    app: App,
    own_unit: Entity,
    enemy_unit: Entity,
    house: Entity,
    foundation: Entity,
    tree: Entity,
}

const MAP: f32 = 64.0;

fn setup() -> World {
    let data = test_data();
    let mut app = sim_app(data.clone());
    app.add_plugins(AssetPlugin::default()).init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>();
    app.add_plugins(BoxRenderPlugin);
    let ids = flat_world(&mut app, MAP, &[("ironhold", 0), ("dune", 1)]);
    app.world_mut().insert_resource(LocalPlayer(Some(ids[0])));
    let peasant = data.unit_id("peasant").unwrap();
    let house = data.building_id("house").unwrap();
    let barracks = data.building_id("barracks").unwrap();
    let oak = data.node_id("oak").unwrap();
    let (own_unit, enemy_unit, house, foundation, tree) = with_commands(&mut app, |c, d, p| {
        (
            spawn_unit(c, d, p.get(ids[0]), peasant, Vec2::new(10.0, 10.0), 0.0),
            spawn_unit(c, d, p.get(ids[1]), peasant, Vec2::new(50.0, 50.0), 1.0),
            spawn_building(c, d, p.get(ids[0]), house, Vec2::new(20.0, 10.0), 0.0, true),
            spawn_building(c, d, p.get(ids[0]), barracks, Vec2::new(10.0, 30.0), 0.0, false),
            spawn_node(c, d, oak, Vec2::new(30.0, 30.0), 0.0, None),
        )
    });
    run_ticks(&mut app, 2);
    World { app, own_unit, enemy_unit, house, foundation, tree }
}

fn visual(app: &App, e: Entity) -> &BoxVisual {
    app.world().get::<BoxVisual>(e).expect("entity has visuals")
}

fn box_count(app: &mut App, root: Entity) -> usize {
    let mut q = app.world_mut().query::<&Children>();
    let mut stack = vec![root];
    let mut n = 0;
    while let Some(e) = stack.pop() {
        n += 1;
        if let Ok(children) = q.get(app.world(), e) {
            stack.extend(children.iter());
        }
    }
    n - 1
}

#[test]
fn builds_scene_terrain_and_entity_visuals() {
    let mut w = setup();
    let app = &mut w.app;
    let terrain = app.world_mut().query_filtered::<(), With<TerrainBox>>().iter(app.world()).count();
    // 64 m of grass in 32-cell patches = 4, plus the slab and the frame.
    assert_eq!(terrain, 4 + 1 + 4);
    let lights = app.world_mut().query_filtered::<(), (With<DirectionalLight>, With<RenderScene>)>().iter(app.world()).count();
    assert_eq!(lights, 1);
    for e in [w.own_unit, w.house, w.foundation, w.tree] {
        let root = visual(app, e).root;
        assert!(box_count(app, root) >= 2, "{e} has boxes");
        assert!(app.world().get::<Transform>(e).is_some());
    }
    let tf = app.world().get::<Transform>(w.own_unit).unwrap();
    assert_eq!(tf.translation, Vec3::new(10.0, 0.0, 10.0));
    let peasant = app.world().get::<Unit>(w.own_unit).unwrap().kind;
    assert_eq!(visual(app, w.own_unit).sig, VisualSig::Unit { kind: peasant, shield: false, model: false });
    assert_eq!(visual(app, w.own_unit).weapons.len(), 1, "the work axe");
    assert!(visual(app, w.foundation).progress.is_some());
}

#[test]
fn units_follow_the_sim_and_rebuild_on_upgrade() {
    let mut w = setup();
    let app = &mut w.app;
    app.world_mut().get_mut::<Position>(w.own_unit).unwrap().0 = Vec2::new(12.0, 11.0);
    run_ticks(app, 1);
    // Prepare copied the new position into PrevPosition: no interpolation left.
    let tf = *app.world().get::<Transform>(w.own_unit).unwrap();
    assert!((tf.translation - Vec3::new(12.0, 0.0, 11.0)).length() < 1e-4, "{tf:?}");

    let data = test_data();
    let militia = data.unit_id("militia").unwrap();
    let old_root = visual(app, w.own_unit).root;
    app.world_mut().get_mut::<Unit>(w.own_unit).unwrap().kind = militia;
    run_ticks(app, 1);
    assert_eq!(visual(app, w.own_unit).sig, VisualSig::Unit { kind: militia, shield: false, model: false });
    assert!(app.world().get_entity(old_root).is_err(), "old visual despawned");

    // A tech grants a shield: the unit gets a shell, shown while it holds.
    let shield = Shield { hp: 20.0, max: 20.0, regen: 1.0, delay: 4.0, energy_resist: 0.0, since_hit: 999.0 };
    app.world_mut().entity_mut(w.own_unit).insert(shield);
    run_ticks(app, 2);
    let shell = visual(app, w.own_unit).shield.expect("shield shell");
    assert_eq!(*app.world().get::<Visibility>(shell).unwrap(), Visibility::Inherited);
    app.world_mut().get_mut::<Shield>(w.own_unit).unwrap().hp = 0.0;
    run_ticks(app, 1);
    assert_eq!(*app.world().get::<Visibility>(shell).unwrap(), Visibility::Hidden, "broken shields vanish");
}

#[test]
fn foundations_grow_and_complete() {
    let mut w = setup();
    let app = &mut w.app;
    let (part, _) = visual(app, w.foundation).progress.unwrap();
    let low = app.world().get::<Transform>(part).unwrap().scale.y;
    app.world_mut().get_mut::<UnderConstruction>(w.foundation).unwrap().progress = 0.6;
    run_ticks(app, 1);
    let high = app.world().get::<Transform>(part).unwrap().scale.y;
    assert!(high > low * 5.0, "{low} -> {high}");
    let foundation_root = visual(app, w.foundation).root;
    app.world_mut().entity_mut(w.foundation).remove::<UnderConstruction>();
    run_ticks(app, 1);
    let v = visual(app, w.foundation);
    assert!(v.progress.is_none());
    assert!(matches!(v.sig, VisualSig::Building { foundation: false, .. }));
    assert!(app.world().get_entity(foundation_root).is_err(), "scaffold replaced by the finished building");
}

#[test]
fn fog_hides_what_the_local_team_has_not_seen() {
    let mut w = setup();
    let app = &mut w.app;
    let cells = (MAP / 2.0) as u32;
    let mut team0 = TeamFog { visible: vec![0; (cells * cells) as usize], explored: vec![false; (cells * cells) as usize] };
    // Team 0 sees the west half of the map.
    for y in 0..cells {
        for x in 0..cells / 2 {
            team0.visible[(y * cells + x) as usize] = 1;
            team0.explored[(y * cells + x) as usize] = true;
        }
    }
    app.world_mut().insert_resource(FogOfWar { enabled: true, cell: 2.0, width: cells, height: cells, teams: vec![team0, TeamFog::default()] });
    run_ticks(app, 2);
    let vis = |app: &App, e: Entity| *app.world().get::<Visibility>(e).unwrap();
    assert_eq!(vis(app, w.own_unit), Visibility::Inherited);
    assert_eq!(vis(app, w.house), Visibility::Inherited, "own buildings always show");
    assert_eq!(vis(app, w.enemy_unit), Visibility::Hidden, "enemy out of sight");
    assert_eq!(vis(app, w.tree), Visibility::Hidden, "no SeenBy yet: seen by nobody");
    // af_fog marks the tree as seen by team 0.
    app.world_mut().entity_mut(w.tree).insert(SeenBy { teams: team_bit(0), visible: 0 });
    // The enemy's box edge crosses into sight (x = 32 is the boundary).
    let edge = Vec2::new(32.2, 50.0);
    app.world_mut().get_mut::<Position>(w.enemy_unit).unwrap().0 = edge;
    app.world_mut().get_mut::<PrevPosition>(w.enemy_unit).unwrap().0 = edge;
    run_ticks(app, 1);
    assert_eq!(vis(app, w.tree), Visibility::Inherited);
    assert_eq!(vis(app, w.enemy_unit), Visibility::Inherited, "part of its box is in sight");
    // Spectators see everything.
    app.world_mut().get_mut::<Position>(w.enemy_unit).unwrap().0 = Vec2::new(50.0, 50.0);
    app.world_mut().entity_mut(w.tree).remove::<SeenBy>();
    app.world_mut().insert_resource(LocalPlayer(None));
    run_ticks(app, 1);
    assert_eq!(vis(app, w.enemy_unit), Visibility::Inherited);
    assert_eq!(vis(app, w.tree), Visibility::Inherited);
}

#[test]
fn remembered_dead_buildings_stay_drawn() {
    let mut w = setup();
    let app = &mut w.app;
    app.world_mut().insert_resource(FogOfWar { enabled: true, cell: 2.0, width: 32, height: 32, teams: vec![TeamFog::default(); 2] });
    app.init_resource::<FogGhosts>();
    let house = test_data().building_id("house").unwrap();
    let ghost = |teams| FogGhost {
        entity: Entity::from_raw_u32(12345).unwrap(),
        kind: EntityKind::Building(house),
        owner: Some(PlayerId(1)),
        pos: Vec2::new(50.0, 50.0),
        facing: 0.0,
        footprint: Footprint { half: Vec2::splat(2.0), height: 3.0 },
        teams,
    };
    app.world_mut().resource_mut::<FogGhosts>().list.push(ghost(team_bit(0) | team_bit(1)));
    run_ticks(app, 1);
    assert_eq!(app.world().resource::<MemoryPool>().len(), 1);
    let memories = app.world_mut().query_filtered::<&Children, With<FogMemory>>().iter(app.world()).map(|c| c.len()).collect::<Vec<_>>();
    assert_eq!(memories.len(), 1);
    assert!(memories[0] >= 2, "drawn with the building's boxes");
    // Team 0 looked again: forgotten (team 1 still remembers, but it is not ours).
    app.world_mut().resource_mut::<FogGhosts>().list[0] = ghost(team_bit(1));
    run_ticks(app, 1);
    assert!(app.world().resource::<MemoryPool>().is_empty());
    assert_eq!(app.world_mut().query_filtered::<(), With<FogMemory>>().iter(app.world()).count(), 0);
}

#[test]
fn bars_outlines_pings_and_effects() {
    let mut w = setup();
    let app = &mut w.app;
    app.world_mut().get_mut::<Health>(w.house).unwrap().hp *= 0.5;
    app.world_mut().entity_mut(w.own_unit).insert(Selected);
    let ping = app.world_mut().spawn(ClickPing { pos: Vec2::new(5.0, 5.0), time_left: 0.2, color: [0.2, 1.0, 0.2] }).id();
    let fx = app
        .world_mut()
        .spawn(FxBox {
            pos: Vec3::new(5.0, 1.0, 5.0),
            prev_pos: Vec3::new(5.0, 1.0, 5.0),
            dir: Vec3::X,
            size: Vec3::new(0.05, 0.05, 0.6),
            color: [1.0, 0.8, 0.3],
            glow: true,
            alpha: 1.0,
        })
        .id();
    run_ticks(app, 2);
    assert!(app.world().get::<BarParts>(w.house).is_some(), "damaged building gets bars");
    assert!(app.world().get::<BarParts>(w.own_unit).is_some(), "selected unit gets bars");
    assert!(app.world().get::<BarParts>(w.enemy_unit).is_none(), "healthy, unselected: none");
    assert!(app.world().get::<OutlineParts>(w.own_unit).is_some());
    assert!(app.world().get::<Mesh3d>(fx).is_some(), "effects get a mesh");
    assert!(app.world().get::<Mesh3d>(ping).is_some());
    // 0.2 s at 1/30 s per update.
    run_ticks(app, 8);
    assert!(app.world().get_entity(ping).is_err(), "ping expired");

    // A selected own building with a rally point shows a flag there.
    app.world_mut().entity_mut(w.house).insert((Selected, Rally(Some(RallyTarget::Point(Vec2::new(25.0, 20.0))))));
    run_ticks(app, 2);
    let mut flags = app.world_mut().query_filtered::<(&Transform, &Visibility), (With<af_render::overlay::RallyPart>, With<Children>)>();
    let shown: Vec<Vec3> = flags.iter(app.world()).filter(|(_, v)| **v == Visibility::Inherited).map(|(t, _)| t.translation).collect();
    assert_eq!(shown, vec![Vec3::new(25.0, 0.0, 20.0)]);
}

#[test]
fn materials_are_cached_not_created_per_frame() {
    let mut w = setup();
    let app = &mut w.app;
    app.world_mut().entity_mut(w.own_unit).insert(Selected);
    app.world_mut().get_mut::<Health>(w.own_unit).unwrap().hp *= 0.5;
    run_ticks(app, 3);
    let before = app.world().resource::<BoxAssets>().material_count();
    let materials_before = app.world().resource::<Assets<StandardMaterial>>().len();
    run_ticks(app, 20);
    assert_eq!(app.world().resource::<BoxAssets>().material_count(), before);
    assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), materials_before);
}

#[test]
fn placement_ghosts_follow_the_preview() {
    let mut w = setup();
    let app = &mut w.app;
    let house = test_data().building_id("house").unwrap();
    let spot = |x: f32| PlacementGhost { kind: house, pos: Vec2::new(x, 12.0), facing: 0.0, error: None };
    app.world_mut().insert_resource(PlacementPreview { ghosts: vec![spot(40.0), spot(46.0)] });
    run_ticks(app, 2);
    let roots = |app: &mut App| {
        let mut q = app.world_mut().query_filtered::<(&Transform, &Visibility), (With<af_render::ghost::GhostPart>, Without<ChildOf>)>();
        let mut v: Vec<(Vec3, Visibility)> = q.iter(app.world()).map(|(t, v)| (t.translation, *v)).collect();
        v.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
        v
    };
    assert_eq!(roots(app), vec![(Vec3::new(40.0, 0.0, 12.0), Visibility::Inherited), (Vec3::new(46.0, 0.0, 12.0), Visibility::Inherited)]);
    app.world_mut().insert_resource(PlacementPreview { ghosts: vec![spot(30.0)] });
    run_ticks(app, 1);
    let shown: Vec<Vec3> = roots(app).into_iter().filter(|(_, v)| *v == Visibility::Inherited).map(|(t, _)| t).collect();
    assert_eq!(shown, vec![Vec3::new(30.0, 0.0, 12.0)], "pooled, extra ghosts hidden");
    app.world_mut().insert_resource(PlacementPreview::default());
    run_ticks(app, 1);
    assert!(roots(app).iter().all(|(_, v)| *v == Visibility::Hidden));
}

#[test]
fn leaving_the_match_clears_the_scene() {
    let mut w = setup();
    let app = &mut w.app;
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Menu);
    app.update();
    let scene = app.world_mut().query_filtered::<(), With<RenderScene>>().iter(app.world()).count();
    assert_eq!(scene, 0);
}

#[test]
fn ground_units_stand_on_highlands_where_the_sim_picks_them() {
    let mut w = setup();
    let app = &mut w.app;
    let data = test_data();
    let cliff = data.terrain_id("cliff").unwrap();
    let highland = terrain_ground(data.terrain_def(cliff));
    assert_eq!(highland, terrain_top(data.terrain_def(cliff)), "the ground units stand on is the drawn top");
    assert!(highland > 0.0 && highland <= MAX_GROUND_OFFSET);
    {
        let mut nav = app.world_mut().resource_mut::<Nav>();
        for y in 0..20 {
            for x in 40..60 {
                nav.set_terrain(IVec2::new(x, y), cliff.0);
            }
        }
    }
    let peasant = data.unit_id("peasant").unwrap();
    let flyer = UnitTypeId(data.units.iter().position(|u| u.is_flying()).expect("content has a flyer") as u16);
    let climber_at = Vec2::new(45.0, 5.0);
    let flyer_at = Vec2::new(55.0, 15.0);
    let (climber, plane) = with_commands(app, |c, d, p| {
        (spawn_unit(c, d, p.get(PlayerId(0)), peasant, climber_at, 0.0), spawn_unit(c, d, p.get(PlayerId(0)), flyer, flyer_at, 0.0))
    });
    run_ticks(app, 2);
    let y = |app: &App, e: Entity| app.world().get::<Transform>(e).unwrap().translation.y;
    assert_eq!(y(app, climber), highland, "on the highland, not inside it");
    // af_input picks with a ray against the SpatialIndex columns, which
    // stand at Altitude on y = 0: a click on the drawn body still finds it.
    let pitch = CameraRig::default().pitch;
    let to_camera = Vec3::new(-pitch.cos() * 0.6, pitch.sin(), -pitch.cos() * 0.8);
    let center = Vec3::new(climber_at.x, highland + data.unit(peasant).size[1] * 0.5, climber_at.y);
    let picked = app.world().resource::<SpatialIndex>().pick(center + to_camera * 60.0, -to_camera, |e| e.kind == SpatialKind::Unit);
    assert_eq!(picked.map(|e| e.entity), Some(climber));
    let altitude = data.unit(flyer).altitude;
    assert_eq!(y(app, plane), altitude, "flyers ignore the ground");
    let shadow = visual(app, plane).shadow.expect("flyers cast a ground shadow");
    let local = app.world().get::<Transform>(shadow).unwrap().translation.y;
    assert!((altitude + local - (highland + 0.04)).abs() < 1e-4, "shadow lies on the highland: {local}");
}

#[test]
fn enemy_buildings_and_nodes_out_of_sight_look_as_last_seen() {
    let mut w = setup();
    let app = &mut w.app;
    let data = test_data();
    let barracks = data.building_id("barracks").unwrap();
    // Fog on; team 0 sees nothing right now.
    app.world_mut().insert_resource(FogOfWar { enabled: true, cell: 2.0, width: 32, height: 32, teams: vec![TeamFog::default(); 2] });
    let site = with_commands(app, |c, d, p| spawn_building(c, d, p.get(PlayerId(1)), barracks, Vec2::new(45.0, 20.0), 0.0, false));
    // af_fog: team 0 scouted the site at 20% built and half health.
    let mut last = LastSeen::default();
    last.record(team_bit(0), BuildingSighting { kind: barracks, construction: Some(0.2), hp: 0.5 });
    app.world_mut().entity_mut(site).insert((SeenBy { teams: team_bit(0), visible: 0 }, last, Selected));
    app.world_mut().get_mut::<UnderConstruction>(site).unwrap().progress = 0.9;
    run_ticks(app, 2);
    let v = visual(app, site);
    let root = v.root;
    assert!(matches!(v.sig, VisualSig::Building { foundation: true, .. }));
    let (part, full) = v.progress.expect("drawn as the foundation it was");
    let built = app.world().get::<Transform>(part).unwrap().scale.y;
    assert!((built - progress_transform(&full, 0.2).scale.y).abs() < 1e-4, "still 20% built: {built}");
    let bars = *app.world().get::<BarParts>(site).expect("selected: bars");
    let width = BarSpec::for_footprint(&Footprint::from_size(data.building(barracks).size), 0.0).width;
    let fill = |app: &App, row: usize| app.world().get::<Transform>(bars.rows[row].1).unwrap().scale.x / width;
    assert!((fill(app, 0) - 0.2).abs() < 1e-3, "progress bar as last seen");
    assert!((fill(app, 1) - 0.5).abs() < 1e-3, "health bar as last seen");
    // Finished inside the fog: still the remembered foundation.
    app.world_mut().entity_mut(site).remove::<UnderConstruction>();
    run_ticks(app, 1);
    assert_eq!(visual(app, site).root, root, "no rebuild out of sight");
    // In sight again: drawn as it is.
    app.world_mut().get_mut::<SeenBy>(site).unwrap().visible = team_bit(0);
    run_ticks(app, 1);
    assert!(matches!(visual(app, site).sig, VisualSig::Building { foundation: false, .. }));
    assert!(app.world().get_entity(root).is_err(), "the foundation was replaced");

    // A scouted node gathered out of sight keeps its size until seen again.
    app.world_mut().entity_mut(w.tree).insert(SeenBy { teams: team_bit(0), visible: 0 });
    let amount = data.node(app.world().get::<ResourceNode>(w.tree).unwrap().kind).amount;
    app.world_mut().get_mut::<ResourceNode>(w.tree).unwrap().remaining = amount * 0.1;
    run_ticks(app, 1);
    let scale = |app: &App| app.world().get::<Transform>(visual(app, w.tree).root).unwrap().scale.x;
    assert_eq!(scale(app), 1.0);
    app.world_mut().get_mut::<SeenBy>(w.tree).unwrap().visible = team_bit(0);
    run_ticks(app, 1);
    assert!((scale(app) - node_scale(amount * 0.1, amount)).abs() < 1e-5);
}

#[test]
fn outlines_are_made_once_and_follow_the_footprint() {
    let mut w = setup();
    let app = &mut w.app;
    // Selected in the same frame it is first hovered.
    app.world_mut().entity_mut(w.own_unit).insert(Selected);
    app.world_mut().resource_mut::<CursorWorld>().hovered = Some(w.own_unit);
    run_ticks(app, 2);
    let outline = *app.world().get::<OutlineParts>(w.own_unit).unwrap();
    let ours = [visual(app, w.own_unit).root, app.world().get::<BarParts>(w.own_unit).unwrap().root, outline.root];
    let children: Vec<Entity> = app.world().get::<Children>(w.own_unit).unwrap().iter().collect();
    assert_eq!(children.len(), 3, "visual, bars and one outline: {children:?}");
    assert!(ours.iter().all(|e| children.contains(e)));
    // An upgrade changes the footprint: the outline follows.
    let half = app.world().get::<Footprint>(w.own_unit).unwrap().half * 1.8;
    app.world_mut().get_mut::<Footprint>(w.own_unit).unwrap().half = half;
    run_ticks(app, 1);
    assert_eq!(app.world().get::<OutlineParts>(w.own_unit).unwrap().half, half);
    let sides: Vec<Transform> = app.world().get::<Children>(outline.root).unwrap().iter().map(|e| *app.world().get::<Transform>(e).unwrap()).collect();
    let width = min_line_width(app.world().resource::<CameraRig>().distance);
    for (center, size) in outline_boxes(half, 0.0, width) {
        let laid = sides.iter().any(|t| (t.translation.xz() - center.xz()).length() < 1e-4 && (t.scale - size).length() < 1e-4);
        assert!(laid, "a side at {center:?} sized {size:?} in {sides:?}");
    }
}

/// Fog in which team 0 sees exactly `cells` (2 m cells, 32 x 32).
fn fog_seeing(cells: impl IntoIterator<Item = (u32, u32)>) -> FogOfWar {
    let n = 32 * 32;
    let mut team0 = TeamFog { visible: vec![0; n], explored: vec![false; n] };
    for (x, y) in cells {
        team0.visible[(y * 32 + x) as usize] = 1;
        team0.explored[(y * 32 + x) as usize] = true;
    }
    FogOfWar { enabled: true, cell: 2.0, width: 32, height: 32, teams: vec![team0, TeamFog::default()] }
}

/// Put a unit somewhere with no interpolation left, run a tick.
fn place(app: &mut App, e: Entity, at: Vec2, yaw: f32) {
    let mut unit = app.world_mut().entity_mut(e);
    unit.get_mut::<Position>().unwrap().0 = at;
    unit.get_mut::<PrevPosition>().unwrap().0 = at;
    unit.get_mut::<Facing>().unwrap().0 = yaw;
    unit.get_mut::<PrevFacing>().unwrap().0 = yaw;
    run_ticks(app, 1);
}

#[test]
fn enemy_units_show_while_their_box_is_in_sight() {
    let mut w = setup();
    let app = &mut w.app;
    // Only the cell [42, 44) x [40, 42) is in sight.
    app.world_mut().insert_resource(fog_seeing([(21, 20)]));
    let ram = test_data().unit_id("battering_ram").unwrap();
    let ram = with_commands(app, |c, d, p| spawn_unit(c, d, p.get(PlayerId(1)), ram, Vec2::new(40.0, 30.0), 0.0));
    run_ticks(app, 1);
    assert_eq!(app.world().get::<Footprint>(ram).unwrap().half, Vec2::new(0.9, 1.6), "a long box");
    let vis = |app: &App| *app.world().get::<Visibility>(ram).unwrap();
    // Its long side ends at x = 41.7, short of the cell; a disc of its half
    // length (1.6 m) would reach 0.4 m into it.
    place(app, ram, Vec2::new(40.8, 41.0), 0.0);
    assert_eq!(vis(app), Visibility::Hidden, "no lit cell touches its box");
    // Only a corner reaches into the cell, 1.72 m from the centre.
    place(app, ram, Vec2::new(41.15, 38.5), 0.0);
    assert_eq!(vis(app), Visibility::Inherited, "a corner is in sight");
    // Lengthwise its end reaches the cell; turned crosswise it stops 0.5 m short.
    place(app, ram, Vec2::new(43.0, 38.6), 0.0);
    assert_eq!(vis(app), Visibility::Inherited);
    place(app, ram, Vec2::new(43.0, 38.6), std::f32::consts::FRAC_PI_2);
    assert_eq!(vis(app), Visibility::Hidden, "the box turns with the unit");
}

#[test]
fn rally_flags_on_enemy_units_show_what_the_team_saw() {
    let mut w = setup();
    let app = &mut w.app;
    // Team 0 sees the west half of the map (x < 32).
    app.world_mut().insert_resource(fog_seeing((0..32).flat_map(|y| (0..16).map(move |x| (x, y)))));
    let flags = |app: &mut App| -> Vec<Vec3> {
        let mut q = app.world_mut().query_filtered::<(&Transform, &Visibility), (With<af_render::overlay::RallyPart>, With<Children>)>();
        q.iter(app.world()).filter(|(_, v)| **v == Visibility::Inherited).map(|(t, _)| t.translation).collect()
    };
    let enemy = w.enemy_unit;
    // Rallied onto the enemy while it is in sight (the house not selected yet).
    place(app, enemy, Vec2::new(30.0, 50.0), 0.0);
    app.world_mut().entity_mut(w.house).insert(Rally(Some(RallyTarget::Entity(enemy))));
    run_ticks(app, 1);
    // It walks into the fog; then the house is selected (the flag is made
    // the first frame, shown the next).
    place(app, enemy, Vec2::new(50.0, 50.0), 0.0);
    app.world_mut().entity_mut(w.house).insert(Selected);
    run_ticks(app, 2);
    assert_eq!(flags(app), vec![Vec3::new(30.0, 0.0, 50.0)], "where the team last saw it, not where it is");
    place(app, enemy, Vec2::new(55.0, 20.0), 0.0);
    assert_eq!(flags(app), vec![Vec3::new(30.0, 0.0, 50.0)], "its moves in the fog stay hidden");
    // Back in sight: followed again.
    place(app, enemy, Vec2::new(20.0, 40.0), 0.0);
    assert_eq!(flags(app), vec![Vec3::new(20.0, 0.0, 40.0)]);
    // Out of sight again and killed there: the team cannot know.
    place(app, enemy, Vec2::new(50.0, 40.0), 0.0);
    app.world_mut().despawn(enemy);
    run_ticks(app, 1);
    assert_eq!(flags(app), vec![Vec3::new(20.0, 0.0, 40.0)], "a death in the fog is not given away");

    // Another enemy, killed in sight: its flag goes.
    let peasant = test_data().unit_id("peasant").unwrap();
    let second = with_commands(app, |c, d, p| spawn_unit(c, d, p.get(PlayerId(1)), peasant, Vec2::new(28.0, 20.0), 0.0));
    app.world_mut().entity_mut(w.house).insert(Rally(Some(RallyTarget::Entity(second))));
    run_ticks(app, 1);
    assert_eq!(flags(app), vec![Vec3::new(28.0, 0.0, 20.0)]);
    app.world_mut().despawn(second);
    run_ticks(app, 1);
    assert_eq!(flags(app), vec![], "seen to die");

    // Own units are always followed, and with fog off so is everything.
    let third = with_commands(app, |c, d, p| spawn_unit(c, d, p.get(PlayerId(1)), peasant, Vec2::new(50.0, 30.0), 0.0));
    app.world_mut().entity_mut(w.house).insert(Rally(Some(RallyTarget::Entity(w.own_unit))));
    place(app, w.own_unit, Vec2::new(12.0, 14.0), 0.0);
    assert_eq!(flags(app), vec![Vec3::new(12.0, 0.0, 14.0)]);
    app.world_mut().entity_mut(w.house).insert(Rally(Some(RallyTarget::Entity(third))));
    run_ticks(app, 1);
    assert_eq!(flags(app), vec![], "never seen: no flag");
    app.world_mut().resource_mut::<FogOfWar>().enabled = false;
    run_ticks(app, 1);
    assert_eq!(flags(app), vec![Vec3::new(50.0, 0.0, 30.0)]);
}

/// Give a unit orders and a goal straight to `goal` (af_behavior and
/// af_movement are absent: nothing changes them).
fn send(app: &mut App, e: Entity, orders: &[Order], goal: Vec2) {
    let mut unit = app.world_mut().entity_mut(e);
    unit.insert((Orders { queue: orders.iter().copied().collect() }, MoveGoal::point(goal, 0.35), Selected));
}

/// The planned legs, as (points, kind).
fn legs(app: &App) -> Vec<(Vec<Vec2>, LegKind)> {
    let plan = &app.world().resource::<PathOverlay>().plan;
    plan.legs.iter().map(|l| (plan.leg_points(l).to_vec(), l.kind)).collect()
}

#[test]
fn routes_show_only_own_units_and_what_the_team_saw() {
    let mut w = setup();
    let app = &mut w.app;
    app.world_mut().insert_resource(ShowPaths { on: true, held: false });
    // Team 0 sees the west half of the map (x < 32).
    app.world_mut().insert_resource(fog_seeing((0..32).flat_map(|y| (0..16).map(move |x| (x, y)))));
    let (me, enemy) = (w.own_unit, w.enemy_unit);
    place(app, enemy, Vec2::new(30.0, 50.0), 0.0);
    // Ours: a move, then an attack on the enemy queued behind it. Theirs,
    // selected too (for its stats): a move of its own.
    send(app, me, &[Order::Move { target: Vec2::new(20.0, 20.0) }, Order::Attack { target: enemy }], Vec2::new(20.0, 20.0));
    send(app, enemy, &[Order::Move { target: Vec2::new(60.0, 60.0) }], Vec2::new(60.0, 60.0));
    run_ticks(app, 1);
    let mine = vec![(vec![Vec2::new(10.0, 10.0), Vec2::new(20.0, 20.0)], LegKind::Move), (vec![Vec2::new(20.0, 20.0), Vec2::new(30.0, 50.0)], LegKind::Attack)];
    assert_eq!(legs(app), mine, "the enemy's route is never planned");
    // It walks into the fog: the attack leg ends where it was last seen.
    place(app, enemy, Vec2::new(50.0, 50.0), 0.0);
    assert_eq!(legs(app), mine, "its moves in the fog stay hidden");
    // Fog off: everything is known.
    app.world_mut().resource_mut::<FogOfWar>().enabled = false;
    run_ticks(app, 1);
    assert_eq!(legs(app)[1].0, vec![Vec2::new(20.0, 20.0), Vec2::new(50.0, 50.0)]);
    // Paths hidden: nothing (no patrol among them).
    app.world_mut().resource_mut::<ShowPaths>().on = false;
    run_ticks(app, 1);
    let overlay = app.world().resource::<PathOverlay>();
    assert!(overlay.plan.legs.is_empty() && overlay.plan.loops.is_empty());
    // Shift held shows them meanwhile.
    app.world_mut().resource_mut::<ShowPaths>().held = true;
    run_ticks(app, 1);
    assert_eq!(legs(app).len(), 2);
}

#[test]
fn a_selected_patrol_always_shows_its_loop() {
    let mut w = setup();
    let app = &mut w.app;
    let (a, b, c) = (Vec2::new(10.0, 10.0), Vec2::new(20.0, 10.0), Vec2::new(20.0, 20.0));
    // Walking from b to c (af_behavior points the goal at the leg's end).
    send(app, w.own_unit, &[Order::Patrol { a, b }, Order::Patrol { a: b, b: c }], c);
    send(app, w.enemy_unit, &[Order::Patrol { a: Vec2::new(50.0, 50.0), b: Vec2::new(40.0, 50.0) }], Vec2::new(40.0, 50.0));
    run_ticks(app, 1);
    let overlay = app.world().resource::<PathOverlay>();
    assert!(overlay.plan.legs.is_empty(), "paths are off (no ShowPaths at all here)");
    assert_eq!(overlay.plan.loops.len(), 1, "only our own patrol");
    let route = &overlay.plan.loops[0];
    assert_eq!(overlay.plan.loop_points(route), &[a, b, c]);
    assert_eq!((route.active, route.walking), (true, Some(1)));
    // Off fighting (its goal on a foe): the leg it was on stays lit.
    app.world_mut().entity_mut(w.own_unit).insert(MoveGoal::entity(w.enemy_unit, Vec2::new(50.0, 50.0), 1.0));
    run_ticks(app, 1);
    assert_eq!(app.world().resource::<PathOverlay>().plan.loops[0].walking, Some(1));
    // Deselected: gone.
    app.world_mut().entity_mut(w.own_unit).remove::<Selected>();
    run_ticks(app, 1);
    assert!(app.world().resource::<PathOverlay>().plan.loops.is_empty());
}

/// The pathfinder knows the whole map, so a path round an enemy building
/// the team has not seen would trace it: the route is drawn through the
/// turns in sight, then straight on to the goal.
#[test]
fn routes_never_trace_what_the_fog_hides() {
    let mut w = setup();
    let app = &mut w.app;
    app.world_mut().insert_resource(ShowPaths { on: true, held: false });
    // Team 0 sees the west half of the map (x < 32); an enemy house stands
    // in the east, unseen, between the unit and its goal.
    app.world_mut().insert_resource(fog_seeing((0..32).flat_map(|y| (0..16).map(move |x| (x, y)))));
    with_commands(app, |c, d, p| spawn_building(c, d, &p.list[1], d.building_id("house").unwrap(), Vec2::new(40.0, 20.0), 0.0, true));
    let (from, to) = (Vec2::new(20.0, 20.0), Vec2::new(56.0, 20.0));
    place(app, w.own_unit, from, 0.0);
    send(app, w.own_unit, &[Order::Move { target: to }], to);
    let seq = app.world().get::<MoveGoal>(w.own_unit).unwrap().seq;
    let round = vec![Vec2::new(28.0, 20.0), Vec2::new(36.0, 14.0), Vec2::new(44.0, 14.0), to];
    let path = PathState { waypoints: round.clone(), next: 0, status: PathStatus::Following, goal_seq: seq, ..default() };
    app.world_mut().entity_mut(w.own_unit).insert(path);
    run_ticks(app, 1);
    assert_eq!(legs(app), vec![(vec![from, round[0], to], LegKind::Move)], "the turns round the house are left out");
    // Fog off: the whole path.
    app.world_mut().resource_mut::<FogOfWar>().enabled = false;
    run_ticks(app, 1);
    assert_eq!(legs(app), vec![([vec![from], round].concat(), LegKind::Move)]);
}

/// Past [`MAX_ROUTES`] selected units, those nearest the centre of the view
/// are drawn, and the HUD is told how many.
#[test]
fn a_big_selection_draws_the_routes_nearest_the_view() {
    let mut w = setup();
    let app = &mut w.app;
    app.world_mut().insert_resource(ShowPaths { on: true, held: false });
    app.world_mut().resource_mut::<CameraRig>().focus = Vec2::new(0.0, 40.0);
    let peasant = app.world().resource::<Data>().unit_id("peasant").unwrap();
    let at = |i: usize| Vec2::new(2.0 + 0.8 * i as f32, 40.0);
    let army: Vec<Entity> = (0..70).map(|i| with_commands(app, |c, d, p| spawn_unit(c, d, &p.list[0], peasant, at(i), 0.0))).collect();
    run_ticks(app, 1);
    for (i, e) in army.iter().enumerate() {
        let goal = at(i) + Vec2::new(0.0, 10.0);
        send(app, *e, &[Order::Move { target: goal }], goal);
    }
    run_ticks(app, 1);
    let starts: Vec<Vec2> = legs(app).iter().map(|(points, _)| points[0]).collect();
    assert_eq!(starts.len(), MAX_ROUTES);
    assert!((0..MAX_ROUTES).all(|i| starts.contains(&at(i))), "the nearest {MAX_ROUTES}: {starts:?}");
    assert_eq!(*app.world().resource::<RoutesShown>(), RoutesShown { drawn: MAX_ROUTES, wanted: 70 });
}
