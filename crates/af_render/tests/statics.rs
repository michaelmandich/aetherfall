//! Buildings and nodes drawn with static models, headless: the real plugin
//! and content, and a static library built by hand (cube meshes standing in
//! for the loaded glTF files; `static_fixtures.rs` loads real ones).

use af_core::ext::fog::{BuildingSighting, FogGhost, FogGhosts, LastSeen, SeenBy, team_bit};
use af_core::prelude::*;
use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};
use af_render::BoxRenderPlugin;
use af_render::assets::BoxAssets;
use af_render::entities::{BoxVisual, VisualSig};
use af_render::fog::FogMemory;
use af_render::parts::node_scale;
use af_render::scene::RenderScene;
use af_render::statics::fall::{MAX_FALLING, MAX_STUMPS, Sinking};
use af_render::statics::place::{FALL_TIME, FALLEN_HOLD, FALLEN_SINK, STUMP_HOLD, STUMP_SINK, construction_sink, fit_transform, hashed_yaw, variant_index};
use af_render::statics::{
    FallingTree, Fit, MeshPart, MeshState, Remnants, StaticId, StaticInstance, StaticLibrary, StaticManifest, StaticMesh, StaticTarget, StaticVisual, Stump,
};
use bevy::prelude::*;
use std::sync::Arc;

const HOUSE: &str = r#"(kind: Building, id: "house", banner: true, variants: [(model: "house.glb", min: (-1.0, 0.0, -1.0), max: (1.0, 1.5, 1.0))])"#;
const MANOR: &str = r#"(kind: Building, id: "manor", banner: true, variants: [(model: "manor.glb", min: (-2.0, 0.0, -1.0), max: (2.0, 4.0, 1.0))])"#;
const PINE: &str = r#"(kind: Node, id: "pine", variants: [
    (model: "a.glb", min: (-1.0, 0.0, -1.0), max: (1.0, 5.0, 1.0)),
    (model: "b.glb", min: (-1.2, 0.0, -1.2), max: (1.2, 6.0, 1.2)),
    (model: "c.glb", min: (-0.8, 0.0, -0.8), max: (0.8, 4.0, 0.8)),
], stump: (model: "stump.glb", min: (-0.3, 0.0, -0.3), max: (0.3, 0.5, 0.3)))"#;
const ROCK: &str = r#"(kind: Node, id: "stone_outcrop", variants: [(model: "rock.glb", min: (-2.0, 0.0, -1.0), max: (2.0, 1.0, 1.0))])"#;

/// What the stand-in models are made of: one mesh and two materials, as a
/// loaded glTF file shares them between instances.
#[derive(Clone)]
struct Kit {
    mesh: Handle<Mesh>,
    stone: Handle<StandardMaterial>,
    team: Handle<StandardMaterial>,
}

impl Kit {
    fn new(app: &mut App) -> Kit {
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(Sphere::default());
        let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
        Kit { mesh, stone: materials.add(StandardMaterial::from(Color::srgb(0.5, 0.5, 0.5))), team: materials.add(StandardMaterial::from(Color::WHITE)) }
    }

    /// A loaded file: a body, plus a team-coloured flag when `banner`.
    fn file(&self, min: Vec3, max: Vec3, banner: bool) -> MeshState {
        let mut parts = vec![MeshPart { mesh: self.mesh.clone(), material: self.stone.clone(), transform: Transform::default(), team: false }];
        if banner {
            parts.push(MeshPart { mesh: self.mesh.clone(), material: self.team.clone(), transform: Transform::from_xyz(-0.8, 1.6, 0.8), team: true });
        }
        MeshState::Ready(Arc::new(StaticMesh::new(parts, min, max)))
    }
}

fn manifest(text: &str) -> StaticManifest {
    StaticManifest::parse(text).unwrap_or_else(|e| panic!("{e}")).0
}

/// A library with the four models, every file loaded (`ready`) or loading.
fn library(kit: &Kit, ready: bool) -> StaticLibrary {
    let data = test_data();
    let mut lib = StaticLibrary::default();
    for (target, text) in [
        (StaticTarget::Building(data.building_id("house").unwrap()), HOUSE),
        (StaticTarget::Building(data.building_id("manor").unwrap()), MANOR),
        (StaticTarget::Node(data.node_id("pine").unwrap()), PINE),
        (StaticTarget::Node(data.node_id("stone_outcrop").unwrap()), ROCK),
    ] {
        let m = manifest(text);
        let state = |spec: &af_render::statics::MeshSpec| if ready { kit.file(spec.min(), spec.max(), m.banner) } else { MeshState::Loading(Handle::default()) };
        let variants = m.variants.iter().map(state).collect();
        let stump = m.stump.as_ref().map(|s| if ready { kit.file(s.min(), s.max(), false) } else { MeshState::Loading(Handle::default()) });
        lib.insert(target, &format!("{}.model.ron", m.id), m.clone(), variants, stump);
    }
    lib
}

struct Test {
    app: App,
    kit: Kit,
    players: Vec<PlayerId>,
    house: Entity,
    site: Entity,
    barracks: Entity,
    pine: Entity,
    rock: Entity,
    oak: Entity,
}

const PINE_AT: Vec2 = Vec2::new(12.0, 30.0);

fn setup(ready: bool) -> Test {
    let data = test_data();
    let mut app = sim_app(data.clone());
    app.add_plugins(AssetPlugin::default()).init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>();
    let kit = Kit::new(&mut app);
    app.insert_resource(library(&kit, ready));
    app.add_plugins(BoxRenderPlugin);
    let players = flat_world(&mut app, 64.0, &[("ironhold", 0), ("dune", 1)]);
    app.world_mut().insert_resource(LocalPlayer(Some(players[0])));
    let id = |s: &str| data.building_id(s).unwrap();
    let node = |s: &str| data.node_id(s).unwrap();
    let (house, site, barracks, pine, rock, oak) = with_commands(&mut app, |c, d, p| {
        let own = p.get(players[0]);
        (
            spawn_building(c, d, own, id("house"), Vec2::new(20.0, 10.0), 0.0, true),
            spawn_building(c, d, own, id("house"), Vec2::new(30.0, 10.0), 0.0, false),
            spawn_building(c, d, own, id("barracks"), Vec2::new(10.0, 50.0), 0.0, true),
            spawn_node(c, d, node("pine"), PINE_AT, 0.0, None),
            spawn_node(c, d, node("stone_outcrop"), Vec2::new(20.0, 30.0), 0.0, None),
            spawn_node(c, d, node("oak"), Vec2::new(28.0, 30.0), 0.0, None),
        )
    });
    run_ticks(&mut app, 2);
    Test { app, kit, players, house, site, barracks, pine, rock, oak }
}

fn visual(app: &App, e: Entity) -> &BoxVisual {
    app.world().get::<BoxVisual>(e).expect("entity has visuals")
}

fn drawn(app: &App, e: Entity) -> StaticVisual {
    *app.world().get::<StaticVisual>(e).expect("drawn with a model")
}

fn tf(app: &App, e: Entity) -> Transform {
    *app.world().get::<Transform>(e).expect("a transform")
}

fn children(app: &App, e: Entity) -> Vec<Entity> {
    app.world().get::<Children>(e).map(|c| c.iter().collect()).unwrap_or_default()
}

/// Boxes (the shared cube) anywhere under `root`.
fn cubes_under(app: &App, root: Entity) -> usize {
    let cube = app.world().resource::<BoxAssets>().cube.clone();
    let mut stack = vec![root];
    let mut n = 0;
    while let Some(e) = stack.pop() {
        n += app.world().get::<Mesh3d>(e).is_some_and(|m| m.0 == cube) as usize;
        stack.extend(children(app, e));
    }
    n
}

fn size(kind: &str) -> Vec3 {
    let data = test_data();
    Vec3::from(data.building_id(kind).map(|b| data.building(b).size).unwrap_or_else(|| data.node(data.node_id(kind).unwrap()).size))
}

fn library_model(app: &App, id: StaticId) -> String {
    app.world().resource::<StaticLibrary>().model(id).manifest.id.clone()
}

#[test]
fn a_ready_model_replaces_the_body_boxes() {
    let t = setup(true);
    let app = &t.app;
    let house = drawn(app, t.house);
    let root = visual(app, t.house).root;
    assert!(matches!(visual(app, t.house).sig, VisualSig::Building { model: true, foundation: false, .. }));
    assert_eq!(cubes_under(app, root), 0, "no body boxes");
    assert_eq!(app.world().get::<ChildOf>(house.instance).map(ChildOf::parent), Some(root));
    assert!(app.world().get::<StaticInstance>(house.instance).is_some());
    // Box fit: the 2 x 1.5 x 2 model on the 4 x 3 x 4 house box, not turned.
    let placed = tf(app, house.instance);
    assert_eq!(placed, fit_transform(Fit::Box, Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 1.5, 1.0), size("house"), 0.0));
    assert!((placed.scale - Vec3::splat(2.0)).length() < 1e-5 && placed.rotation == Quat::IDENTITY);
    assert_eq!(house.height, 3.0);
    // The body keeps the model's own (shared) material; the banner takes the owner's colour.
    let parts = children(app, house.instance);
    assert_eq!(parts.len(), 2);
    let material = |e: Entity| app.world().get::<MeshMaterial3d<StandardMaterial>>(e).unwrap().0.clone();
    let (body, flag) = (material(parts[0]), material(parts[1]));
    assert_eq!(body, t.kit.stone, "shared, so instances batch");
    assert_ne!(flag, t.kit.team);
    let owner = app.world().resource::<Players>().get(t.players[0]).color;
    let tint = app.world().resource::<Assets<StandardMaterial>>().get(&flag).unwrap().base_color.to_srgba();
    assert!((tint.red - owner[0]).abs() < 0.05 && (tint.green - owner[1]).abs() < 0.05 && (tint.blue - owner[2]).abs() < 0.05, "{tint:?} vs {owner:?}");
    assert!(parts.iter().all(|p| app.world().get::<Mesh3d>(*p).unwrap().0 == t.kit.mesh));

    // Nodes: the pine fits by height and turns by its position; the rock by its footprint.
    let pine = drawn(app, t.pine);
    assert_eq!(pine.variant, variant_index(PINE_AT, 3));
    let spec = &manifest(PINE).variants[pine.variant];
    let want = fit_transform(Fit::Height, spec.min(), spec.max(), size("pine"), hashed_yaw(PINE_AT));
    assert_eq!(tf(app, pine.instance), want);
    assert!((pine.height - 6.0).abs() < 1e-4, "as tall as the pine's box");
    assert!(!pine.shrinks, "trees fall instead");
    let rock = drawn(app, t.rock);
    assert!((tf(app, rock.instance).scale - Vec3::splat(2.4 / 4.0)).length() < 1e-5, "the 4 m side on the 2.4 m box");
    assert!(rock.shrinks);
    assert_eq!(children(app, rock.instance).len(), 1, "nodes have no banner");

    // No model: boxes, as before.
    for e in [t.barracks, t.oak] {
        assert!(app.world().get::<StaticVisual>(e).is_none());
        assert!(cubes_under(app, visual(app, e).root) >= 2, "{e} keeps its boxes");
    }
    assert!(matches!(visual(app, t.oak).sig, VisualSig::Node { model: false, .. }));
    assert!(matches!(visual(app, t.barracks).sig, VisualSig::Building { model: false, .. }));
}

#[test]
fn boxes_until_the_model_is_ready_then_rebuilt() {
    let mut t = setup(false);
    let roots: Vec<Entity> = [t.house, t.pine].iter().map(|e| visual(&t.app, *e).root).collect();
    for (e, root) in [t.house, t.pine].into_iter().zip(&roots) {
        assert!(t.app.world().get::<StaticVisual>(e).is_none(), "still loading: boxes");
        assert!(cubes_under(&t.app, *root) >= 2);
    }
    let ready = library(&t.kit, true);
    t.app.insert_resource(ready);
    run_ticks(&mut t.app, 1);
    for (e, root) in [t.house, t.pine].into_iter().zip(roots) {
        assert!(t.app.world().get_entity(root).is_err(), "the boxes are gone");
        assert!(t.app.world().get::<StaticVisual>(e).is_some());
    }
    assert!(matches!(visual(&t.app, t.pine).sig, VisualSig::Node { model: true, .. }));
}

#[test]
fn a_foundation_rises_out_of_the_ground_as_it_is_built() {
    let mut t = setup(true);
    let app = &mut t.app;
    let site = drawn(app, t.site);
    let root = visual(app, t.site).root;
    assert!(matches!(visual(app, t.site).sig, VisualSig::Building { foundation: true, model: true, .. }));
    assert!(visual(app, t.site).progress.is_none(), "no built-so-far box: the model rises instead");
    assert_eq!(cubes_under(app, root), 2, "the pad and the scaffold stay");
    assert!((tf(app, site.instance).translation.y + 3.0).abs() < 1e-5, "just started: all 3 m underground");
    app.world_mut().get_mut::<UnderConstruction>(t.site).unwrap().progress = 0.6;
    run_ticks(app, 1);
    let y = tf(app, site.instance).translation.y;
    assert!((y + construction_sink(0.6, 3.0)).abs() < 1e-5, "60% built: 1.2 m still underground, at {y}");
    assert_eq!(tf(app, site.instance).scale, site.rest.scale, "it rises, it does not stretch");
    app.world_mut().entity_mut(t.site).remove::<UnderConstruction>();
    run_ticks(app, 1);
    let done = drawn(app, t.site);
    assert!(matches!(visual(app, t.site).sig, VisualSig::Building { foundation: false, model: true, .. }));
    assert!(app.world().get_entity(root).is_err(), "the scaffold went with the old root");
    assert_eq!(tf(app, done.instance), done.rest, "at rest");
    assert_eq!(cubes_under(app, visual(app, t.site).root), 0);
}

#[test]
fn models_stand_on_the_sand_their_footprint_overlaps() {
    let mut t = setup(true);
    let app = &mut t.app;
    let data = test_data();
    let sand = data.terrain_id("sand").unwrap();
    let top = af_render::terrain::flat_top(data.terrain_def(sand));
    assert!(top > 0.0, "sand is drawn proud of the ground units walk on");
    {
        let mut nav = app.world_mut().resource_mut::<Nav>();
        for y in 0..64 {
            for x in 41..64 {
                nav.set_terrain(IVec2::new(x, y), sand.0);
            }
        }
    }
    let house = data.building_id("house").unwrap();
    let pine = data.node_id("pine").unwrap();
    let own = t.players[0];
    // A 4 m house centred on grass with its east metre on sand; one whose
    // footprint only touches the sand's edge; a foundation and a pine on sand.
    let (straddling, touching, site, tree) = with_commands(app, |c, d, p| {
        (
            spawn_building(c, d, p.get(own), house, Vec2::new(40.0, 20.0), 0.0, true),
            spawn_building(c, d, p.get(own), house, Vec2::new(39.0, 30.0), 0.0, true),
            spawn_building(c, d, p.get(own), house, Vec2::new(50.0, 20.0), 0.0, false),
            spawn_node(c, d, pine, Vec2::new(50.0, 40.0), 0.0, None),
        )
    });
    run_ticks(app, 2);
    let fitted = |m: &StaticVisual| {
        let model = app.world().resource::<StaticLibrary>().model(m.model).clone();
        let mesh = model.variant(m.variant).unwrap().clone();
        let yaw = if model.manifest.random_yaw { hashed_yaw(tf(app, tree).translation.xz()) } else { 0.0 };
        let target = if model.manifest.id == "pine" { size("pine") } else { size("house") };
        (fit_transform(model.manifest.fit, mesh.min, mesh.max, target, yaw), mesh.full_height())
    };
    for e in [straddling, touching, site, tree] {
        assert_eq!(tf(app, e).translation.y, 0.0, "the entity stays on the ground units walk on");
    }
    let lifted = |m: &StaticVisual| m.rest.translation.y - fitted(m).0.translation.y;
    let on_edge = drawn(app, straddling);
    assert!((lifted(&on_edge) - top).abs() < 1e-5, "up on the sand its east side stands in: {}", lifted(&on_edge));
    assert_eq!(tf(app, on_edge.instance), on_edge.rest);
    assert!(lifted(&drawn(app, touching)).abs() < 1e-6, "a footprint flush with the sand stays on the grass");
    let pine_model = drawn(app, tree);
    assert!((lifted(&pine_model) - top).abs() < 1e-5);
    // A foundation on sand starts with its top level with the ground, so
    // none of it (the banner's tip included) shows before building starts.
    let foundation = drawn(app, site);
    let (fit, full) = fitted(&foundation);
    assert!((foundation.height - (top + fit.scale.y * full)).abs() < 1e-5, "{}", foundation.height);
    let start = tf(app, foundation.instance);
    assert!((start.translation.y - (foundation.rest.translation.y - foundation.height)).abs() < 1e-5);
    app.world_mut().entity_mut(site).remove::<UnderConstruction>();
    run_ticks(app, 1);
    let built = drawn(app, site);
    assert_eq!(built.rest, foundation.rest);
    assert_eq!(tf(app, built.instance), built.rest, "built: on the sand");
}

#[test]
fn an_enemy_foundation_in_the_fog_rises_as_far_as_it_was_seen() {
    let mut t = setup(true);
    let app = &mut t.app;
    let data = test_data();
    app.world_mut().insert_resource(FogOfWar { enabled: true, cell: 2.0, width: 32, height: 32, teams: vec![TeamFog::default(); 2] });
    let enemy = t.players[1];
    let site = with_commands(app, |c, d, p| spawn_building(c, d, p.get(enemy), data.building_id("house").unwrap(), Vec2::new(45.0, 20.0), 0.0, false));
    let mut last = LastSeen::default();
    last.record(team_bit(0), BuildingSighting { kind: data.building_id("house").unwrap(), construction: Some(0.2), hp: 0.5 });
    app.world_mut().entity_mut(site).insert((SeenBy { teams: team_bit(0), visible: 0 }, last));
    app.world_mut().get_mut::<UnderConstruction>(site).unwrap().progress = 0.9;
    run_ticks(app, 2);
    let m = drawn(app, site);
    let y = tf(app, m.instance).translation.y;
    assert!((y + construction_sink(0.2, 3.0)).abs() < 1e-5, "20% built as last seen, not 90%: {y}");
    // The enemy's banner is in the enemy's colour.
    let flag = children(app, m.instance)[1];
    let tint = app.world().get::<MeshMaterial3d<StandardMaterial>>(flag).unwrap().0.clone();
    let color = app.world().resource::<Players>().get(enemy).color;
    let c = app.world().resource::<Assets<StandardMaterial>>().get(&tint).unwrap().base_color.to_srgba();
    assert!((c.red - color[0]).abs() < 0.05 && (c.blue - color[2]).abs() < 0.05);
}

#[test]
fn upgrades_swap_the_model_or_go_back_to_boxes() {
    let mut t = setup(true);
    let app = &mut t.app;
    let data = test_data();
    let before = drawn(app, t.house);
    app.world_mut().get_mut::<Building>(t.house).unwrap().kind = data.building_id("manor").unwrap();
    run_ticks(app, 1);
    let after = drawn(app, t.house);
    assert_eq!(library_model(app, after.model), "manor");
    assert!(app.world().get_entity(before.instance).is_err(), "the house model is gone");
    let want = fit_transform(Fit::Box, Vec3::new(-2.0, 0.0, -1.0), Vec3::new(2.0, 4.0, 1.0), size("manor"), 0.0);
    assert_eq!(tf(app, after.instance), want);
    // A type without a model: boxes again.
    app.world_mut().get_mut::<Building>(t.house).unwrap().kind = data.building_id("barracks").unwrap();
    run_ticks(app, 1);
    assert!(app.world().get::<StaticVisual>(t.house).is_none());
    assert!(matches!(visual(app, t.house).sig, VisualSig::Building { model: false, .. }));
    assert!(cubes_under(app, visual(app, t.house).root) >= 2);
}

#[test]
fn rocks_shrink_as_they_are_gathered_and_trees_keep_their_size() {
    let mut t = setup(true);
    let app = &mut t.app;
    let data = test_data();
    let root_scale = |app: &App, e: Entity| tf(app, visual(app, e).root).scale.x;
    for e in [t.rock, t.pine, t.oak] {
        let amount = data.node(app.world().get::<ResourceNode>(e).unwrap().kind).amount;
        app.world_mut().get_mut::<ResourceNode>(e).unwrap().remaining = amount * 0.1;
    }
    run_ticks(app, 1);
    assert!((root_scale(app, t.rock) - node_scale(0.1, 1.0)).abs() < 1e-5, "the rock model shrinks");
    assert!((root_scale(app, t.oak) - node_scale(0.1, 1.0)).abs() < 1e-5, "box trees still shrink");
    assert_eq!(root_scale(app, t.pine), 1.0, "a modelled tree stands until it falls");
}

/// Put a worker next to `tree`, gathering it.
fn feller(app: &mut App, owner: PlayerId, tree: Entity, at: Vec2) -> Entity {
    let worker = with_commands(app, |c, d, p| spawn_unit(c, d, p.get(owner), d.unit_id("peasant").unwrap(), at, 0.0));
    run_ticks(app, 1);
    let mut orders = app.world().get::<Orders>(worker).unwrap().clone();
    orders.set(Order::Gather { target: tree }, false);
    app.world_mut().entity_mut(worker).insert(orders);
    worker
}

fn ticks(seconds: f32) -> u32 {
    (seconds * SIM_HZ as f32).ceil() as u32
}

#[test]
fn a_felled_tree_falls_away_from_its_feller_sinks_and_leaves_a_stump() {
    let mut t = setup(true);
    let app = &mut t.app;
    feller(app, t.players[0], t.pine, PINE_AT - Vec2::new(1.5, 0.0));
    let m = drawn(app, t.pine);
    let model = app.world().resource::<StaticLibrary>().model(m.model).clone();
    let top = model.variant(m.variant).unwrap().max.y;
    app.world_mut().entity_mut(t.pine).insert(Dead);
    run_ticks(app, 1);
    assert!(app.world().get_entity(t.pine).is_err(), "the node is gone");
    let fall = *app.world().get::<FallingTree>(m.instance).expect("its model falls");
    assert!(app.world().get::<ChildOf>(m.instance).is_none() && app.world().get::<RenderScene>(m.instance).is_some());
    assert!((fall.dir - Vec2::X).length() < 1e-5, "away from the worker on its west: {}", fall.dir);
    assert_eq!(app.world().resource::<Remnants>().falling(), 1);
    // The stump is planted where the tree stood, as large as the tree.
    let stumps: Vec<(Entity, Transform)> = app.world_mut().query_filtered::<(Entity, &Transform), With<Stump>>().iter(app.world()).map(|(e, t)| (e, *t)).collect();
    assert_eq!(stumps.len(), 1);
    let (stump, at) = stumps[0];
    assert!((at.translation.xz() - PINE_AT).length() < 1e-4 && (at.scale - m.rest.scale).length() < 1e-5, "{at:?}");
    assert_eq!(app.world().resource::<Remnants>().stumps(), 1);

    run_ticks(app, ticks(FALL_TIME));
    let down = tf(app, m.instance).transform_point(Vec3::Y * top);
    assert!(down.x > PINE_AT.x + 5.0 && down.y < 1.0, "on the ground east of the stump: {down}");
    run_ticks(app, ticks(FALLEN_HOLD + FALLEN_SINK));
    assert!(app.world().get_entity(m.instance).is_err(), "sunk and gone");
    assert_eq!(app.world().resource::<Remnants>().falling(), 0);
    assert!(app.world().get_entity(stump).is_ok(), "the stump stays");

    run_ticks(app, ticks(STUMP_HOLD - FALL_TIME - FALLEN_HOLD - FALLEN_SINK - 1.0));
    assert!(app.world().get::<Sinking>(stump).is_none(), "standing for a while");
    run_ticks(app, ticks(1.0 + STUMP_SINK * 0.5));
    assert!(tf(app, stump).translation.y < at.translation.y - 0.05, "sinking");
    run_ticks(app, ticks(STUMP_SINK * 0.5 + 0.1));
    assert!(app.world().get_entity(stump).is_err(), "gone");
    assert_eq!(app.world().resource::<Remnants>().stumps(), 0);
}

#[test]
fn trees_felled_out_of_sight_or_without_a_model_just_go() {
    let mut t = setup(true);
    let app = &mut t.app;
    app.world_mut().insert_resource(FogOfWar { enabled: true, cell: 2.0, width: 32, height: 32, teams: vec![TeamFog::default(); 2] });
    app.world_mut().entity_mut(t.pine).insert(SeenBy { teams: team_bit(0), visible: 0 });
    app.world_mut().entity_mut(t.oak).insert(SeenBy::default());
    run_ticks(app, 1);
    let instance = drawn(app, t.pine).instance;
    app.world_mut().entity_mut(t.pine).insert(Dead);
    app.world_mut().entity_mut(t.oak).insert(Dead);
    run_ticks(app, 2);
    assert!(app.world().get_entity(instance).is_err(), "nobody saw it fall");
    assert_eq!(app.world_mut().query::<&FallingTree>().iter(app.world()).count(), 0);
    assert_eq!(app.world_mut().query::<&Stump>().iter(app.world()).count(), 0);
}

#[test]
fn falling_trees_and_stumps_are_capped() {
    let mut t = setup(true);
    let app = &mut t.app;
    let pine = test_data().node_id("pine").unwrap();
    let forest: Vec<Entity> = with_commands(app, |c, d, _| (0..MAX_FALLING + 3).map(|i| spawn_node(c, d, pine, Vec2::new(4.0 + (i % 12) as f32 * 4.5, 36.0 + (i / 12) as f32 * 4.5), 0.0, None)).collect());
    run_ticks(app, 2);
    for e in &forest {
        app.world_mut().entity_mut(*e).insert(Dead);
    }
    run_ticks(app, 1);
    let falling = app.world_mut().query::<&FallingTree>().iter(app.world()).count();
    assert_eq!(falling, MAX_FALLING, "the oldest falls are cut short");
    assert_eq!(app.world().resource::<Remnants>().falling(), MAX_FALLING);
    // Past the stump cap, the oldest stumps sink early.
    let now = app.world().resource::<Time>().elapsed_secs();
    let extra: Vec<Entity> = (0..MAX_STUMPS).map(|_| app.world_mut().spawn((Transform::default(), Stump { rest_y: 0.0, depth: 0.5 })).id()).collect();
    for e in &extra {
        app.world_mut().resource_mut::<Remnants>().plant(*e, now);
    }
    run_ticks(app, 1);
    assert_eq!(app.world().resource::<Remnants>().stumps(), MAX_STUMPS);
    let sinking = app.world_mut().query_filtered::<(), (With<Stump>, With<Sinking>)>().iter(app.world()).count();
    assert_eq!(sinking, MAX_FALLING, "the felled trees' stumps were planted first");
}

#[test]
fn remembered_dead_buildings_and_trees_keep_their_models() {
    let mut t = setup(true);
    let app = &mut t.app;
    let data = test_data();
    app.world_mut().insert_resource(FogOfWar { enabled: true, cell: 2.0, width: 32, height: 32, teams: vec![TeamFog::default(); 2] });
    app.init_resource::<FogGhosts>();
    let ghost = |raw: u32, kind: EntityKind, owner: Option<PlayerId>, pos: Vec2| FogGhost {
        entity: Entity::from_raw_u32(raw).unwrap(),
        kind,
        owner,
        pos,
        facing: 0.0,
        footprint: Footprint { half: Vec2::splat(1.0), height: 3.0 },
        teams: team_bit(0),
    };
    let pos = Vec2::new(50.0, 40.0);
    {
        let mut ghosts = app.world_mut().resource_mut::<FogGhosts>();
        ghosts.list.push(ghost(12345, EntityKind::Building(data.building_id("house").unwrap()), Some(PlayerId(1)), Vec2::new(50.0, 50.0)));
        ghosts.list.push(ghost(12346, EntityKind::Node(data.node_id("pine").unwrap()), None, pos));
        ghosts.list.push(ghost(12347, EntityKind::Node(data.node_id("oak").unwrap()), None, Vec2::new(40.0, 40.0)));
    }
    run_ticks(app, 1);
    let memories: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<FogMemory>>().iter(app.world()).collect();
    assert_eq!(memories.len(), 3);
    let modelled = memories.iter().filter(|m| children(app, **m).iter().any(|c| app.world().get::<StaticInstance>(*c).is_some())).count();
    assert_eq!(modelled, 2, "the house and the pine; the oak is boxes");
    // The remembered pine looks as it did standing: same variant and turn.
    let pine = memories.iter().find(|m| (tf(app, **m).translation.xz() - pos).length() < 1e-4).unwrap();
    let instance = children(app, *pine)[0];
    let spec = &manifest(PINE).variants[variant_index(pos, 3)];
    assert_eq!(tf(app, instance), fit_transform(Fit::Height, spec.min(), spec.max(), size("pine"), hashed_yaw(pos)));
}
