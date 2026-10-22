//! Decor placement on real generated maps (af_map's generator and the real
//! content), with every decor model there is (`Catalogue::full`): the
//! rules hold on every kind of map, the plan is the same every time and
//! its size stays bounded, and the land gets dressed where it should be.
//! `cargo test -p af_render --test decor -- --nocapture` prints what each
//! map gets.

use std::sync::Arc;

use af_core::ext::map::{MapFeatures, MapOptions, MapSize, RiverCount, SiteChoice, SiteKind, WaterLevel};
use af_core::prelude::*;
use af_core::testing::{flat_world, run_ticks, sim_app, test_data};
use af_render::BoxRenderPlugin;
use af_render::statics::{MeshPart, MeshState, StaticLibrary, StaticManifest, StaticMesh, StaticTarget};
use af_render::terrain::decor::{Catalogue, DecorKind, MAX_DECOR, plan, violations};
use af_render::terrain::field::{Field, FieldSource};
use af_render::terrain::{DecorPart, SMALL_DECOR_RANGE, TerrainChunk};
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

fn setup(data: &GameData, n: usize, seed: u64) -> MatchSetup {
    let players = (0..n)
        .map(|i| PlayerSetup { name: format!("P{i}"), faction: FactionId((i % data.factions.len()) as u8), team: i as u8, controller: Controller::Ai, color: None })
        .collect();
    MatchSetup { seed, players, ..MatchSetup::default() }
}

/// A generated map's field and features.
fn land(data: &GameData, players: usize, seed: u64, size: MapSize, water: WaterLevel, site: SiteKind) -> (Field, MapFeatures) {
    let options = MapOptions { size, water, rivers: RiverCount::Two, site: SiteChoice::Kind(site) };
    let map = af_map::generate_with(data, &setup(data, players, seed), &options);
    let nav = map.nav(data);
    let crater = (map.crater_radius > 0.0).then_some((map.crater_center, map.crater_radius));
    let field = Field::build(&FieldSource { data, nav: &nav, seed, features: Some(&map.features), crater });
    (field, map.features)
}

fn counts(placed: &[af_render::terrain::decor::Placed]) -> [usize; DecorKind::COUNT] {
    let mut c = [0; DecorKind::COUNT];
    for p in placed {
        c[p.kind.index()] += 1;
    }
    c
}

#[test]
fn every_kind_of_map_is_dressed_by_the_rules() {
    let data = test_data();
    let cat = Catalogue::full();
    let cases = [
        (4, 7, MapSize::Large, WaterLevel::Lakes, SiteKind::Crater),
        (2, 3, MapSize::Medium, WaterLevel::Coast, SiteKind::Shattered),
        (6, 11, MapSize::Large, WaterLevel::Seas, SiteKind::Lake),
        (3, 5, MapSize::Medium, WaterLevel::Ponds, SiteKind::Scar),
        (4, 9, MapSize::Large, WaterLevel::Coast, SiteKind::Spire),
        (8, 2, MapSize::Huge, WaterLevel::Seas, SiteKind::Crater),
    ];
    for (players, seed, size, water, site) in cases {
        let (field, features) = land(&data, players, seed, size, water, site);
        let placed = plan(&field, Some(&features), &cat);
        let c = counts(&placed);
        let at = format!("{players} players, seed {seed}, {} {} {:?}", size.name(), water.name(), site);
        let list: Vec<String> = DecorKind::ALL.iter().filter(|k| c[k.index()] > 0).map(|k| format!("{} {}", k.id(), c[k.index()])).collect();
        println!("{at}: {} pieces: {}", placed.len(), list.join(", "));
        let broken = violations(&field, &cat, &placed);
        assert!(broken.is_empty(), "{at}: {} broken rules, first: {:#?}", broken.len(), &broken[..broken.len().min(12)]);
        assert!(placed.len() <= MAX_DECOR, "{at}: {}", placed.len());
        // What every such map shows.
        if !features.mountains.is_empty() {
            assert!(c[DecorKind::MountainPeak.index()] > 0, "{at}: mountains without a peak");
        }
        if matches!(site, SiteKind::Crater | SiteKind::Shattered | SiteKind::Scar | SiteKind::Lake) {
            // The rims are dressed most of the way round (the main site's
            // circumference over pieces of at most 11 m, its gaps left out).
            let main = features.sites[0];
            let want = (std::f32::consts::TAU * main.radius / 11.0 * 0.5) as usize;
            assert!(c[DecorKind::CraterRim.index()] >= want, "{at}: {} rim pieces round a {} m rim (want {want})", c[DecorKind::CraterRim.index()], main.radius);
        }
        if site == SiteKind::Spire {
            assert!(c[DecorKind::AetherMonolith.index()] >= 2, "{at}: the Starspire's monoliths");
        }
        if matches!(water, WaterLevel::Coast | WaterLevel::Seas) {
            assert!(c[DecorKind::BeachRocks.index()] + c[DecorKind::SeaStack.index()] > 0, "{at}: a coast without its rocks");
        }
        assert!(c[DecorKind::StoneBridge.index()] + c[DecorKind::WoodenFootbridge.index()] > 0, "{at}: no bridge over the skirt's rivers");
        assert!(c[DecorKind::Reeds.index()] > 0, "{at}: no reeds");
        // Deterministic.
        assert_eq!(plan(&field, Some(&features), &cat), placed, "{at}");
    }
}

/// A decor manifest as the static build writes it, its variants unit boxes.
fn decor_manifest(id: &str, variants: &[&str]) -> StaticManifest {
    let list: Vec<String> = variants.iter().map(|v| format!(r#"(model: "models/static/decor/{id}/{v}.glb", min: (-1.0, 0.0, -1.0), max: (1.0, 2.0, 1.0))"#)).collect();
    let text = format!(r#"(kind: Decor, id: "{id}", fit: Height, random_yaw: true, variants: [{}])"#, list.join(", "));
    StaticManifest::parse(&text).unwrap_or_else(|e| panic!("{e}")).0
}

#[test]
fn the_land_waits_for_its_decor_then_dresses_itself_headless() {
    let data = test_data();
    let mut app = sim_app(data.clone());
    app.add_plugins(AssetPlugin::default()).init_asset::<Mesh>().init_asset::<StandardMaterial>().init_asset::<Image>();
    let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(Cuboid::default());
    let rock = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
    let ready = || MeshState::Ready(Arc::new(StaticMesh::new(vec![MeshPart { mesh: mesh.clone(), material: rock.clone(), transform: Transform::default(), team: false }], Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 2.0, 1.0))));
    let mut lib = StaticLibrary::default();
    for (kind, variants) in [(DecorKind::MountainPeak, ["a", "c"].as_slice()), (DecorKind::Bramble, ["a"].as_slice())] {
        let m = decor_manifest(kind.id(), variants);
        let states = m.variants.iter().map(|_| ready()).collect();
        lib.insert(StaticTarget::Decor(kind), &format!("{}.model.ron", kind.id()), m, states, None);
    }
    // The reeds are still loading.
    let reeds = lib.insert(StaticTarget::Decor(DecorKind::Reeds), "reeds.model.ron", decor_manifest("reeds", &["a"]), vec![MeshState::Loading(Handle::default())], None);
    app.insert_resource(lib);
    app.add_plugins(BoxRenderPlugin);
    flat_world(&mut app, 64.0, &[("ironhold", 0), ("dune", 1)]);
    {
        // A mountain block, a thicket and marsh with a pond in it.
        let t = |id: &str| data.terrain_id(id).unwrap().0;
        let mut nav = app.world_mut().resource_mut::<Nav>();
        for z in 0..64 {
            for x in 0..64 {
                let p = Vec2::new(x as f32 + 0.5, z as f32 + 0.5);
                let id = if (34..58).contains(&x) && (14..40).contains(&z) {
                    Some(t("mountain"))
                } else if p.distance(Vec2::new(14.0, 50.0)) < 7.0 {
                    Some(t("thicket"))
                } else if p.distance(Vec2::new(14.0, 18.0)) < 4.0 {
                    Some(t("shallow_water"))
                } else if p.distance(Vec2::new(14.0, 18.0)) < 9.0 {
                    Some(t("marsh"))
                } else {
                    None
                };
                if let Some(id) = id {
                    nav.set_terrain(IVec2::new(x, z), id);
                }
            }
        }
    }
    let parts = |app: &mut App| app.world_mut().query::<(&DecorPart, &Transform, Has<NotShadowCaster>, Option<&VisibilityRange>)>().iter(app.world()).map(|(d, t, flat, range)| (d.0, *t, flat, range.cloned())).collect::<Vec<_>>();
    run_ticks(&mut app, 3);
    let chunks = app.world_mut().query::<&TerrainChunk>().iter(app.world()).count();
    assert_eq!(chunks, 0, "the land waits while a decor model is loading");
    // The reeds arrive: the land is built, dressed with all three.
    app.world_mut().resource_mut::<StaticLibrary>().model_mut(reeds).variants[0] = ready();
    run_ticks(&mut app, 2);
    assert!(app.world_mut().query::<&TerrainChunk>().iter(app.world()).count() > 0);
    let drawn = parts(&mut app);
    let count = |k: DecorKind| drawn.iter().filter(|p| p.0 == k).count();
    assert!(count(DecorKind::MountainPeak) > 0 && count(DecorKind::Bramble) > 0 && count(DecorKind::Reeds) > 0, "{:?}", DecorKind::ALL.map(|k| (k, count(k))));
    let on_block = drawn.iter().filter(|p| p.0 == DecorKind::MountainPeak && (34.0..58.0).contains(&p.1.translation.x) && (14.0..40.0).contains(&p.1.translation.z)).count();
    assert!(on_block > 0, "a peak crowns the painted range");
    for (kind, transform, flat, range) in &drawn {
        assert!(transform.is_finite());
        if *kind == DecorKind::MountainPeak {
            assert!(!flat && range.is_none(), "peaks cast shadows and are drawn from afar");
            let p = transform.translation;
            let on_map = (0.0..64.0).contains(&p.x) && (0.0..64.0).contains(&p.z);
            assert!(!on_map || (p.x > 30.0 && p.z > 10.0 && p.z < 44.0), "on the mountain block (or the skirt's): {p}");
        } else {
            assert!(*flat && *range == Some(VisibilityRange::abrupt(0.0, SMALL_DECOR_RANGE)), "small scenery: no shadow, not from the far overview");
        }
    }
    // Without a render stack the parts keep their model's own material.
    let materials: Vec<Handle<StandardMaterial>> = app.world_mut().query_filtered::<&MeshMaterial3d<StandardMaterial>, With<DecorPart>>().iter(app.world()).map(|m| m.0.clone()).collect();
    assert!(!materials.is_empty() && materials.iter().all(|m| *m == rock));
    // Built once: nothing changes on later frames.
    run_ticks(&mut app, 3);
    assert_eq!(parts(&mut app).len(), drawn.len());
}

