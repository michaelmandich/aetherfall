//! Look at the land without the map generator: a hand-painted 256 m map
//! with every kind of terrain (a sea edge with its beach, a river with
//! fords running into the sea, a lake ringed by marsh, a mountain range and
//! a corner massif, a rocky highland, dunes, a bramble thicket in a wood,
//! and a scorched meteor site with its rim), the `MapFeatures` af_map would
//! publish for it, and a few armies, buildings and resource nodes for
//! scale and to check that team colours still read on the ground.
//!
//!   cargo run -p af_render --example landscape
//!   cargo run -p af_render --example landscape -- --shot out/a.png@128,128,66 --shot out/b.png@60,90,150,45
//!   cargo run -p af_render --example landscape -- --site spire --fog --shot out/spire.png@128,128,90
//!
//! A shot is `PATH@X,Z,DIST[,TURN]`: the camera's focus, its distance (the
//! game's view, pitched 54 degrees) and turn (degrees clockwise from above).
//! Without shots the window stays open. `--wait S` sets how many seconds to
//! let models and shaders load before the first shot (default 6).

use std::path::PathBuf;

use af_core::CorePlugin;
use af_core::ext::input::RtsCamera;
use af_core::ext::map::{MapEdge, MapFeatures, MapSize, MeteorSite, MountainRange, ResolvedMapOptions, RiverLine, SiteKind, WaterLevel};
use af_core::prelude::*;
use af_core::testing::{flat_world, with_commands};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::WindowResolution;

const MAP: f32 = 256.0;
/// Where the sea starts (x), before the coast's wobble.
const COAST: f32 = 34.0;
const CENTRE: Vec2 = Vec2::new(140.0, 128.0);
const RIM: f32 = 19.0;

struct Shot {
    path: PathBuf,
    focus: Vec2,
    distance: f32,
    turn: f32,
}

struct Args {
    shots: Vec<Shot>,
    fog: bool,
    site: SiteKind,
    wait: f32,
}

fn parse_args() -> Args {
    let mut a = Args { shots: Vec::new(), fog: false, site: SiteKind::Crater, wait: 6.0 };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| panic!("{arg} needs a value"));
        match arg.as_str() {
            "--shot" => {
                let v = value();
                let (path, at) = v.rsplit_once('@').expect("--shot PATH@X,Z,DIST[,TURN]");
                let n: Vec<f32> = at.split(',').map(|s| s.trim().parse().expect("--shot numbers")).collect();
                assert!(n.len() >= 3, "--shot PATH@X,Z,DIST[,TURN]");
                a.shots.push(Shot { path: PathBuf::from(path), focus: Vec2::new(n[0], n[1]), distance: n[2], turn: n.get(3).copied().unwrap_or(0.0) });
            }
            "--fog" => a.fog = true,
            "--site" => a.site = SiteKind::parse(&value()).expect("--site crater|shattered|lake|scar|spire"),
            "--wait" => a.wait = value().parse().expect("--wait seconds"),
            other => panic!("unknown argument {other}"),
        }
    }
    a
}

#[derive(Resource)]
struct Plan {
    shots: Vec<Shot>,
    wait: f32,
    next: usize,
    /// Real time the current shot's camera was placed.
    placed: Option<f32>,
}

fn main() -> AppExit {
    let args = parse_args();
    let data = std::sync::Arc::new(GameData::load_default().unwrap_or_else(|e| panic!("{e}")));
    let mut app = App::new();
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "af_render landscape".into(), resolution: WindowResolution::new(1600, 900), ..default() }),
        ..default()
    }));
    app.insert_state(GameState::Playing);
    app.insert_resource(Data(data.clone()));
    app.add_plugins((CorePlugin, af_render::BoxRenderPlugin));
    app.add_systems(Update, take_shots);

    let ids = flat_world(&mut app, MAP, &[("ironhold", 0), ("sylvan", 1), ("tide", 2)]);
    app.world_mut().insert_resource(LocalPlayer(Some(ids[0])));
    let features = paint(&mut app, args.site);
    {
        let mut map = app.world_mut().resource_mut::<MapInfo>();
        map.crater_center = CENTRE;
        map.crater_radius = RIM;
    }
    app.world_mut().insert_resource(features);
    populate(&mut app, &ids);
    if args.fog {
        app.world_mut().insert_resource(fog_around(CENTRE + Vec2::new(-10.0, 20.0), 45.0, 85.0));
    }
    let first = args.shots.first().map_or((CENTRE, 90.0, 0.0), |s| (s.focus, s.distance, s.turn));
    app.world_mut().spawn((RtsCamera, Camera3d::default(), view(first.0, first.1, first.2)));
    app.insert_resource(Plan { shots: args.shots, wait: args.wait, next: 0, placed: None });
    app.run()
}

/// The game's RTS view of `focus` from `distance`, turned by `turn` degrees.
fn view(focus: Vec2, distance: f32, turn: f32) -> Transform {
    let f = Vec3::new(focus.x, 0.0, focus.y);
    let back = Quat::from_rotation_y(-turn.to_radians()) * Vec3::new(0.41, 0.81, 0.41).normalize();
    Transform::from_translation(f + back * distance).looking_at(f, Vec3::Y)
}

/// Smooth noise for the painting (only this example's look).
fn wobble(p: Vec2, scale: f32) -> f32 {
    ((p.x * scale).sin() * 1.3 + (p.y * scale * 1.37 + 1.1).sin() + ((p.x + p.y) * scale * 0.71 + 2.3).sin() * 0.7) / 3.0
}

/// The river's centreline: from the north edge, winding south-west into
/// the sea, a point every 0.75 m, and its deep half-width there.
fn river_line() -> (Vec<Vec2>, Vec<f32>) {
    let knots = [Vec2::new(150.0, 262.0), Vec2::new(142.0, 222.0), Vec2::new(112.0, 196.0), Vec2::new(98.0, 160.0), Vec2::new(72.0, 120.0), Vec2::new(40.0, 96.0), Vec2::new(20.0, 88.0)];
    let mut pts = Vec::new();
    for w in knots.windows(2) {
        let n = (w[0].distance(w[1]) / 0.75).ceil() as usize;
        for i in 0..n {
            let t = i as f32 / n as f32;
            let p = w[0].lerp(w[1], t);
            let dir = (w[1] - w[0]).normalize();
            let meander = (pts.len() as f32 * 0.75 / 40.0 * std::f32::consts::TAU).sin() * 3.0;
            pts.push(p + Vec2::new(-dir.y, dir.x) * meander);
        }
    }
    let n = pts.len();
    let half = (0..n).map(|i| 2.4 + 0.9 * i as f32 / n as f32).collect();
    (pts, half)
}

/// Paint every terrain on the map; returns what af_map would publish.
fn paint(app: &mut App, site: SiteKind) -> MapFeatures {
    let data = app.world().resource::<Data>().0.clone();
    let t = |id: &str| data.terrain_id(id).unwrap_or_else(|| panic!("terrain {id}")).0;
    let (pts, half) = river_line();
    // Fords every ~32 m along the river (not in the sea).
    let fords: Vec<usize> = (1..).map(|k| k * 43).take_while(|i| *i < pts.len() - 40).collect();
    let spine = [Vec2::new(176.0, 30.0), Vec2::new(200.0, 62.0), Vec2::new(232.0, 92.0)];
    let mut nav = app.world_mut().resource_mut::<Nav>();
    let n = MAP as i32;
    for z in 0..n {
        for x in 0..n {
            let p = Vec2::new(x as f32 + 0.5, z as f32 + 0.5);
            let coast = COAST + wobble(p, 0.06) * 5.0;
            let mut id = None;
            if p.x < coast - 9.0 {
                id = Some(t("ocean"));
            } else if p.x < coast - 3.0 {
                id = Some(t("shallow_water"));
            } else if p.x < coast {
                id = Some(t("sand"));
            }
            // Mountains: a range with a pass, and a massif in the corner.
            let along = spine.windows(2).map(|s| distance_to_segment(p, s[0], s[1])).fold(f32::MAX, f32::min);
            let range = 7.5 + wobble(p, 0.09) * 2.5;
            let pass = p.distance(Vec2::new(200.0, 62.0)) < 6.0;
            if (along < range && !pass) || p.distance(Vec2::new(256.0, 256.0)) < 34.0 + wobble(p, 0.08) * 6.0 {
                id = Some(t("mountain"));
            }
            // The lake, ringed by marsh.
            let lake = p.distance(Vec2::new(205.0, 185.0)) + wobble(p, 0.15) * 2.0;
            if lake < 10.0 {
                id = Some(t("deep_water"));
            } else if lake < 13.0 {
                id = Some(t("shallow_water"));
            } else if lake < 19.0 && wobble(p, 0.4) > -0.1 {
                id = Some(t("marsh"));
            }
            // A rocky highland: a ring of cliff with a grass top and a ramp.
            let hl = p.distance(Vec2::new(78.0, 205.0));
            let ramp = (p - Vec2::new(78.0, 205.0)).normalize_or_zero().dot(Vec2::new(0.7, -0.7)) > 0.93;
            if (8.0..12.5).contains(&hl) && !ramp {
                id = Some(t("cliff"));
            }
            // Dunes and a wood with a bramble thicket.
            if p.distance(Vec2::new(205.0, 128.0)) + wobble(p, 0.2) * 4.0 < 13.0 {
                id = Some(t("sand"));
            }
            if p.distance(Vec2::new(140.0, 190.0)) + wobble(p, 0.25) * 3.0 < 8.0 {
                id = Some(t("thicket"));
            }
            // The meteor site: a scorched floor inside a cliff rim with gaps.
            let d = p.distance(CENTRE);
            let angle = (p.y - CENTRE.y).atan2(p.x - CENTRE.x);
            let gap = (angle * 3.0 + 0.5).sin() > 0.85;
            if d < RIM - 2.5 + wobble(p, 0.3) * 1.5 {
                id = Some(t("scorched"));
            } else if d < RIM + 0.5 && !gap {
                id = Some(t("cliff"));
            }
            if site == SiteKind::Spire && d < 6.0 {
                id = Some(t("mountain"));
            }
            // The river last: it cuts through everything but the sea.
            let (i, dr) = pts.iter().enumerate().map(|(i, q)| (i, q.distance(p))).fold((0, f32::MAX), |a, b| if b.1 < a.1 { b } else { a });
            if dr < half[i] + 2.0 && id != Some(t("ocean")) && d > RIM + 1.0 {
                let ford = fords.iter().any(|f| (*f as f32 - i as f32).abs() * 0.75 < half[i] + 3.0);
                id = Some(if ford {
                    t("ford")
                } else if dr < half[i] {
                    t("deep_water")
                } else {
                    t("shallow_water")
                });
            }
            if let Some(id) = id {
                nav.set_terrain(IVec2::new(x, z), id);
            }
        }
    }
    MapFeatures {
        options: ResolvedMapOptions { size: MapSize::Medium, water: WaterLevel::Coast, rivers: 1, site },
        land_min: Vec2::new(COAST, 0.0),
        land_max: Vec2::splat(MAP),
        ocean_edges: vec![MapEdge::West],
        sites: vec![MeteorSite { kind: site, center: CENTRE, radius: RIM }],
        rivers: vec![RiverLine { points: pts, half_widths: half, fords }],
        mountains: vec![MountainRange { spine: spine.to_vec(), half_widths: vec![7.5; 3] }],
    }
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Three armies near the river, a few buildings, and resource nodes.
fn populate(app: &mut App, ids: &[PlayerId]) {
    with_commands(app, |c, data, players| {
        for (k, owner) in ids.iter().enumerate() {
            let player = players.get(*owner);
            let faction = data.faction(player.faction);
            let base = Vec2::new(110.0 + k as f32 * 22.0, 150.0);
            for (i, u) in faction.unit_ids.iter().filter(|u| !data.unit(**u).is_flying()).take(10).enumerate() {
                spawn_unit(c, data, player, *u, base + Vec2::new((i % 5) as f32 * 3.0, (i / 5) as f32 * 3.5), 0.6);
            }
            if let Some(b) = faction.building_ids.iter().find(|b| data.building(**b).requires_node_id.is_none()) {
                spawn_building(c, data, player, *b, Vec2::new(105.0 + k as f32 * 30.0, 172.0), 0.0, true);
            }
        }
        let node = |id: &str| data.node_id(id);
        // A wood around the thicket.
        for i in 0..28 {
            let a = i as f32 * 2.4;
            let r = 9.0 + (i % 5) as f32 * 2.2;
            if let Some(n) = node(if i % 3 == 0 { "pine" } else { "oak" }) {
                spawn_node(c, data, n, Vec2::new(140.0, 190.0) + Vec2::new(a.cos(), a.sin()) * r, a, None);
            }
        }
        // The site's crystals, vents and meteors; stone and berries outside.
        for (id, at) in [
            ("aether_crystal", CENTRE + Vec2::new(-8.0, 5.0)),
            ("aether_crystal", CENTRE + Vec2::new(-6.0, 7.0)),
            ("aether_crystal", CENTRE + Vec2::new(9.0, -6.0)),
            ("aether_vent", CENTRE + Vec2::new(4.0, 9.0)),
            ("adamant_meteor", CENTRE + Vec2::new(-9.0, -7.0)),
            ("stone_outcrop", Vec2::new(165.0, 160.0)),
            ("stone_outcrop", Vec2::new(169.0, 157.0)),
            ("berry_bush", Vec2::new(120.0, 120.0)),
            ("berry_bush", Vec2::new(122.0, 117.5)),
            ("iron_vein", Vec2::new(78.0, 205.0)),
        ] {
            if let Some(n) = node(id) {
                spawn_node(c, data, n, at, 0.4, None);
            }
        }
    });
}

/// Team 0 sees a disc and has explored a larger one.
fn fog_around(center: Vec2, sight: f32, explored: f32) -> FogOfWar {
    let cell = 2.0;
    let n = (MAP / cell) as u32;
    let mut team = TeamFog { visible: vec![0; (n * n) as usize], explored: vec![false; (n * n) as usize] };
    for y in 0..n {
        for x in 0..n {
            let d = (Vec2::new(x as f32, y as f32) * cell).distance(center);
            let i = (y * n + x) as usize;
            team.visible[i] = u8::from(d < sight);
            team.explored[i] = d < explored;
        }
    }
    FogOfWar { enabled: true, cell, width: n, height: n, teams: vec![team, TeamFog::default(), TeamFog::default()] }
}

/// For each shot: move the camera, let it settle, save the screen, give the
/// capture a moment; exit after the last.
fn take_shots(mut commands: Commands, time: Res<Time<Real>>, mut plan: ResMut<Plan>, mut camera: Query<&mut Transform, With<RtsCamera>>, mut exit: MessageWriter<AppExit>) {
    let now = time.elapsed_secs();
    if plan.shots.is_empty() || now < plan.wait {
        return;
    }
    let i = plan.next / 2;
    let Some(shot) = plan.shots.get(i) else {
        if now > plan.placed.unwrap_or(0.0) + 1.5 {
            exit.write(AppExit::Success);
        }
        return;
    };
    let since = now - plan.placed.unwrap_or(f32::MIN);
    if plan.next % 2 == 0 {
        // Settle after the last capture, then place the camera.
        if since < 0.4 {
            return;
        }
        if let Ok(mut tf) = camera.single_mut() {
            *tf = view(shot.focus, shot.distance, shot.turn);
        }
    } else {
        if since < 0.6 {
            return;
        }
        let path = shot.path.clone();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        info!("landscape: {}", path.display());
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    plan.next += 1;
    plan.placed = Some(now);
}
