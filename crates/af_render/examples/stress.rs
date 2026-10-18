//! Frame-time check for the renderer at the game's budget: a 384 m map,
//! 1000 moving units from every faction, 3000 resource nodes and 150
//! buildings, with bars and outlines on part of them. Runs uncapped (no
//! vsync) for a few seconds, prints the average and worst frame time, the
//! entity / material / terrain box counts, and exits.
//!
//!   cargo run -p af_render --example stress --release

use std::time::Duration;

use af_core::CorePlugin;
use af_core::prelude::*;
use af_core::testing::{flat_world, with_commands};
use af_render::assets::BoxAssets;
use af_render::motion::combat_slot_state;
use af_render::terrain::TerrainBox;
use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};

const MAP: f32 = 384.0;
const UNITS: usize = 1000;
const NODES: usize = 3000;
const BUILDINGS: usize = 150;
/// Frames measured after a warm-up.
const WARMUP: u32 = 120;
const MEASURE: u32 = 300;

fn main() -> AppExit {
    let data = std::sync::Arc::new(GameData::load_default().unwrap_or_else(|e| panic!("{e}")));
    let mut app = App::new();
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "af_render stress".into(),
            resolution: WindowResolution::new(1600, 900),
            present_mode: PresentMode::AutoNoVsync,
            ..default()
        }),
        ..default()
    }));
    app.insert_state(GameState::Playing);
    app.insert_resource(Data(data.clone()));
    app.add_plugins((CorePlugin, af_render::BoxRenderPlugin));
    app.add_systems(FixedUpdate, wander.in_set(SimSet::Movement));
    app.add_systems(Update, measure);

    let factions: Vec<(String, u8)> = data.factions.iter().enumerate().map(|(i, f)| (f.id.clone(), i as u8)).collect();
    let slots: Vec<(&str, u8)> = factions.iter().map(|(f, t)| (f.as_str(), *t)).collect();
    let ids = flat_world(&mut app, MAP, &slots);
    app.world_mut().insert_resource(LocalPlayer(Some(ids[0])));
    carve_terrain(&mut app);
    populate(&mut app, &ids);
    let focus = Vec3::new(MAP * 0.5, 0.0, MAP * 0.5);
    app.world_mut()
        .spawn((Camera3d::default(), Transform::from_translation(focus + Vec3::new(0.41, 0.81, 0.41).normalize() * 160.0).looking_at(focus, Vec3::Y)));
    app.run()
}

/// Lakes, cliff ridges and dune fields with ragged edges.
fn carve_terrain(app: &mut App) {
    let data = app.world().resource::<Data>().0.clone();
    let t = |id: &str| data.terrain_id(id).unwrap_or_else(|| panic!("terrain {id}")).0;
    let kinds = [t("deep_water"), t("cliff"), t("sand"), t("marsh"), t("thicket")];
    let shallow = t("shallow_water");
    let mut rng = SimRng::new(11);
    let blobs: Vec<(Vec2, f32, u8)> =
        (0..30).map(|i| (Vec2::new(rng.range(0.0, MAP), rng.range(0.0, MAP)), rng.range(6.0, 18.0), kinds[i % kinds.len()])).collect();
    let mut nav = app.world_mut().resource_mut::<Nav>();
    for y in 0..MAP as i32 {
        for x in 0..MAP as i32 {
            let p = Vec2::new(x as f32, y as f32);
            for (c, r, kind) in &blobs {
                // Ragged at a 4 m scale, like the map generator's value noise.
                let d = p.distance(*c) + (((x / 4) * 7 + (y / 4) * 13) % 5) as f32 * 0.8;
                if d < *r {
                    nav.set_terrain(IVec2::new(x, y), if *kind == kinds[0] && d > r - 3.0 { shallow } else { *kind });
                }
            }
        }
    }
}

fn populate(app: &mut App, ids: &[PlayerId]) {
    let spawned = with_commands(app, |c, data, players| {
        let mut rng = SimRng::new(5);
        let mut out = Vec::new();
        for i in 0..UNITS {
            let p = players.get(ids[i % ids.len()]);
            let units = &data.faction(p.faction).unit_ids;
            let u = units[(i / ids.len()) % units.len()];
            out.push(spawn_unit(c, data, p, u, Vec2::new(rng.range(20.0, MAP - 20.0), rng.range(20.0, MAP - 20.0)), rng.range(0.0, std::f32::consts::TAU)));
        }
        for i in 0..BUILDINGS {
            let p = players.get(ids[i % ids.len()]);
            let b = data.faction(p.faction).building_ids[i % 7];
            out.push(spawn_building(c, data, p, b, Vec2::new(rng.range(20.0, MAP - 20.0), rng.range(20.0, MAP - 20.0)), 0.0, i % 5 != 0));
        }
        let nodes: Vec<NodeTypeId> = data.node_ids().collect();
        for i in 0..NODES {
            spawn_node(
                c,
                data,
                nodes[i % nodes.len()],
                Vec2::new(rng.range(4.0, MAP - 4.0), rng.range(4.0, MAP - 4.0)),
                rng.range(0.0, std::f32::consts::TAU),
                None,
            );
        }
        out
    });
    for (i, e) in spawned.into_iter().enumerate() {
        let mut ent = app.world_mut().entity_mut(e);
        if i % 4 == 0
            && let Some(mut hp) = ent.get_mut::<Health>()
        {
            hp.hp *= 0.5;
        }
        if i % 10 == 0 {
            ent.insert(Selected);
        }
    }
}

/// Every unit walks a slow circle and keeps attacking.
fn wander(clock: Res<SimClock>, data: Res<Data>, mut q: Query<(&mut Position, &mut Facing, &mut Weapons), With<Unit>>) {
    for (i, (mut p, mut f, mut w)) in q.iter_mut().enumerate() {
        let a = clock.elapsed * 0.4 + i as f32;
        let dir = Vec2::new(a.cos(), a.sin());
        p.0 += dir * 2.0 * SIM_DT;
        f.0 = af_geom::yaw_of(dir);
        for s in &mut w.slots {
            let windup = data.weapon(s.weapon).windup;
            let t = (clock.elapsed + i as f32 * 0.1).rem_euclid(windup + 1.2);
            (s.windup, s.anim) = combat_slot_state(t, windup);
        }
    }
}

#[derive(Default)]
struct Stats {
    frames: u32,
    total: Duration,
    worst: Duration,
}

fn measure(
    time: Res<Time<Real>>,
    mut stats: Local<Stats>,
    assets: Res<BoxAssets>,
    terrain: Query<(), With<TerrainBox>>,
    all: Query<Entity>,
    mut exit: MessageWriter<AppExit>,
) {
    stats.frames += 1;
    if stats.frames <= WARMUP {
        return;
    }
    let dt = time.delta();
    stats.total += dt;
    stats.worst = stats.worst.max(dt);
    if stats.frames == WARMUP + MEASURE {
        let avg = stats.total.as_secs_f64() * 1000.0 / MEASURE as f64;
        println!(
            "stress: {UNITS} units, {NODES} nodes, {BUILDINGS} buildings | avg {avg:.2} ms ({:.0} fps), worst {:.2} ms | {} entities, {} materials, {} terrain boxes",
            1000.0 / avg,
            stats.worst.as_secs_f64() * 1000.0,
            all.iter().count(),
            assets.material_count(),
            terrain.iter().count(),
        );
        exit.write(AppExit::Success);
    }
}
