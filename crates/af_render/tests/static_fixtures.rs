//! The static art pipeline's fixture models (`tests/fixtures/static/`, a
//! miniature asset folder built by `art/static/`) loaded the way the game
//! loads them: through the registry, the manifests and Bevy's own glTF
//! loader, headless (no GPU; without bevy_pbr's glTF support the parts get
//! a plain material, which these tests do not look at).

use std::path::PathBuf;
use std::time::Duration;

use af_core::prelude::*;
use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};
use af_render::BoxRenderPlugin;
use af_render::model::VisualsRoot;
use af_render::registry::ExtraVisuals;
use af_render::statics::{StaticLibrary, StaticMesh, StaticVisual};
use bevy::gltf::GltfPlugin;
use bevy::mesh::{MeshPlugin, VertexAttributeValues};
use bevy::prelude::*;
use bevy::world_serialization::WorldSerializationPlugin;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/static")
}

/// The real plugin over the fixture folder: its `static.ron` is the
/// registry (laid on top, as `--visuals .` would), every model loading.
fn app() -> App {
    let mut app = sim_app(test_data());
    let assets = AssetPlugin { file_path: fixtures().to_string_lossy().into_owned(), ..default() };
    app.add_plugins((assets, WorldSerializationPlugin, MeshPlugin, GltfPlugin::default())).init_asset::<StandardMaterial>().init_asset::<Image>();
    app.insert_resource(VisualsRoot(fixtures())).insert_resource(ExtraVisuals(PathBuf::from(".")));
    app.add_plugins(BoxRenderPlugin);
    app.finish();
    app.cleanup();
    app
}

/// Run until no model is loading (the loads run on Bevy's IO threads).
fn load(app: &mut App) {
    run_ticks(app, 1);
    for _ in 0..2000 {
        let lib = app.world().resource::<StaticLibrary>();
        if !lib.models().is_empty() && lib.models().iter().all(|m| !m.loading()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
        app.update();
    }
    panic!("the fixture models did not finish loading");
}

/// Bounds of a loaded variant's vertices, placed as the parts are.
fn vertex_bounds(app: &App, mesh: &StaticMesh, skip_team: bool) -> (Vec3, Vec3) {
    let meshes = app.world().resource::<Assets<Mesh>>();
    let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
    for part in mesh.parts.iter().filter(|p| !(skip_team && p.team)) {
        let m = meshes.get(&part.mesh).expect("the mesh is loaded");
        let Some(VertexAttributeValues::Float32x3(points)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("no positions") };
        for p in points {
            let p = part.transform.transform_point(Vec3::from(*p));
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo, hi)
}

#[test]
fn the_fixture_models_load_flatten_and_match_their_manifests() {
    let mut app = app();
    load(&mut app);
    let data = test_data();
    let lib = app.world().resource::<StaticLibrary>().clone();
    let (_, pine) = lib.node(data.node_id("pine").unwrap()).expect("the pine is ready");
    let (_, rock) = lib.node(data.node_id("stone_outcrop").unwrap()).expect("the rock is ready");
    let (_, house) = lib.building(data.building_id("house").unwrap()).expect("the house is ready");
    assert_eq!(pine.variants.len(), 2);
    assert!(pine.variant(0).is_some() && pine.variant(1).is_some() && pine.stump().is_some());
    assert!(pine.manifest.falls() && rock.manifest.shrinks() && house.manifest.banner);
    // The flattened vertices sit where the manifest's bounds say: the engine
    // fits by those bounds, so this is what keeps models in their boxes.
    for (model, index) in [(pine, 0), (pine, 1), (rock, 0)] {
        let mesh = model.variant(index).unwrap();
        let (lo, hi) = vertex_bounds(&app, mesh, false);
        assert!((lo - mesh.min).abs().max_element() < 0.01 && (hi - mesh.max).abs().max_element() < 0.01, "{} {index}: {lo}..{hi} vs {}..{}", model.source, mesh.min, mesh.max);
    }
    let stump = pine.stump().unwrap();
    let (lo, hi) = vertex_bounds(&app, stump, false);
    assert!((lo - stump.min).abs().max_element() < 0.01 && (hi - stump.max).abs().max_element() < 0.01);
    // The house: a team banner (the `Team` material) rising above the
    // bounds the house is fitted by, which leave it out.
    let body = house.variant(0).unwrap();
    assert!(body.has_team(), "the banner's flag uses the Team material");
    let (lo, hi) = vertex_bounds(&app, body, false);
    assert!(lo.y.abs() < 0.01 && hi.y > body.max.y + 0.1, "the banner stands above the roof: {lo}..{hi}");
    let (_, flag_top) = vertex_bounds(&app, &StaticMesh { parts: body.parts.iter().filter(|p| p.team).cloned().collect(), ..body.clone() }, false);
    assert!(flag_top.y > body.max.y, "the flag flies above the roof");
    assert!((body.top - hi.y).abs() < 0.01, "the top counts the banner: {} vs {}", body.top, hi.y);
}

#[test]
fn buildings_and_nodes_draw_the_fixture_models() {
    let mut app = app();
    let data = test_data();
    let players = flat_world(&mut app, 64.0, &[("ironhold", 0), ("dune", 1)]);
    app.world_mut().insert_resource(LocalPlayer(Some(players[0])));
    let (house, pine, rock) = with_commands(&mut app, |c, d, p| {
        (
            spawn_building(c, d, p.get(players[0]), d.building_id("house").unwrap(), Vec2::new(20.0, 10.0), 0.0, true),
            spawn_node(c, d, d.node_id("pine").unwrap(), Vec2::new(12.0, 30.0), 0.0, None),
            spawn_node(c, d, d.node_id("stone_outcrop").unwrap(), Vec2::new(20.0, 30.0), 0.0, None),
        )
    });
    load(&mut app);
    run_ticks(&mut app, 1);
    for e in [house, pine, rock] {
        let drawn = app.world().get::<StaticVisual>(e).unwrap_or_else(|| panic!("{e} is drawn with its model"));
        let parts = app.world().get::<Children>(drawn.instance).expect("parts").len();
        assert!(parts >= 1, "{e}");
    }
    let drawn = *app.world().get::<StaticVisual>(house).unwrap();
    // The house body matches the 4 x 3 x 4 box (its model is 4 x 3 x 4 already).
    let placed = *app.world().get::<Transform>(drawn.instance).unwrap();
    assert!((placed.scale - Vec3::ONE).length() < 0.01, "{placed:?}");
    // A fresh foundation would be buried whole: the banner above the 3 m box too.
    assert!(drawn.height > data.building(data.building_id("house").unwrap()).size[1] + 0.1, "{}", drawn.height);
    // Its flag is tinted with the owner's colour.
    let owner = app.world().resource::<Players>().get(players[0]).color;
    let materials = app.world().resource::<Assets<StandardMaterial>>();
    let tinted = app.world().get::<Children>(drawn.instance).unwrap().iter().filter_map(|p| app.world().get::<MeshMaterial3d<StandardMaterial>>(p)).any(|m| {
        let c = materials.get(&m.0).map(|m| m.base_color.to_srgba());
        c.is_some_and(|c| (c.red - owner[0]).abs() < 0.05 && (c.green - owner[1]).abs() < 0.05 && (c.blue - owner[2]).abs() < 0.05)
    });
    assert!(tinted, "a part in the owner's colour");
}
