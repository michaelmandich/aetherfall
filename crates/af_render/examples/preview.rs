//! Look at the box renderer without the rest of the game: one faction's
//! every unit and building (plus a rival army and every node type) on a
//! small hand-made map with water, cliffs, dunes, marsh and thicket.
//!
//!   cargo run -p af_render --example preview
//!   cargo run -p af_render --example preview -- --faction starborn --rival tide
//!   cargo run -p af_render --example preview -- --fog --screenshot shot.png
//!   cargo run -p af_render --example preview -- --focus 40,100 --distance 35
//!
//! Units repeat their attacks the way combat drives them; flyers circle. Content ids come
//! from the data (nothing here names a unit or building).

use std::path::PathBuf;

use af_core::CorePlugin;
use af_core::prelude::*;
use af_core::testing::{flat_world, with_commands};
use af_render::motion::combat_slot_state;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::WindowResolution;

const MAP: f32 = 160.0;

struct Args {
    faction: String,
    rival: String,
    fog: bool,
    focus: Vec2,
    distance: f32,
    screenshot: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut a = Args { faction: "ironhold".into(), rival: "dune".into(), fog: false, focus: Vec2::new(70.0, 75.0), distance: 70.0, screenshot: None };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| panic!("{arg} needs a value"));
        match arg.as_str() {
            "--faction" => a.faction = value(),
            "--rival" => a.rival = value(),
            "--fog" => a.fog = true,
            "--focus" => {
                let v: Vec<f32> = value().split(',').map(|s| s.trim().parse().expect("--focus x,z")).collect();
                a.focus = Vec2::new(v[0], v[1]);
            }
            "--distance" => a.distance = value().parse().expect("--distance metres"),
            "--screenshot" => a.screenshot = Some(PathBuf::from(value())),
            other => panic!("unknown argument {other}"),
        }
    }
    a
}

#[derive(Resource)]
struct Plan {
    screenshot: Option<PathBuf>,
    frames: u32,
}

fn main() -> AppExit {
    let args = parse_args();
    let data = std::sync::Arc::new(GameData::load_default().unwrap_or_else(|e| panic!("{e}")));
    let mut app = App::new();
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "af_render preview".into(), resolution: WindowResolution::new(1600, 900), ..default() }),
        ..default()
    }));
    app.insert_state(GameState::Playing);
    app.insert_resource(Data(data.clone()));
    app.add_plugins((CorePlugin, af_render::BoxRenderPlugin));
    app.insert_resource(Plan { screenshot: args.screenshot.clone(), frames: 0 });
    app.add_systems(FixedUpdate, (swing_weapons, circle_flyers).in_set(SimSet::Combat));
    app.add_systems(Update, screenshot_when_ready);

    let ids = flat_world(&mut app, MAP, &[(&args.faction, 0), (&args.rival, 1)]);
    app.world_mut().insert_resource(LocalPlayer(Some(ids[0])));
    shape_terrain(&mut app);
    populate(&mut app, ids[0], ids[1]);
    if args.fog {
        app.world_mut().insert_resource(fog_around(Vec2::new(60.0, 70.0), 35.0, 60.0));
    }
    // From where the game's RTS camera looks (af_input's view yaw and pitch).
    let cam = Vec3::new(args.focus.x, 0.0, args.focus.y) + Vec3::new(0.41, 0.81, 0.41).normalize() * args.distance;
    app.world_mut().spawn((Camera3d::default(), Transform::from_translation(cam).looking_at(Vec3::new(args.focus.x, 0.0, args.focus.y), Vec3::Y)));
    app.run()
}

/// Lake with shallows in one corner, a cliff ridge, dunes with marsh, a thicket.
fn shape_terrain(app: &mut App) {
    let data = app.world().resource::<Data>().0.clone();
    let t = |id: &str| data.terrain_id(id).unwrap_or_else(|| panic!("terrain {id}")).0;
    let (deep, shallow, cliff, sand, marsh, thicket) = (t("deep_water"), t("shallow_water"), t("cliff"), t("sand"), t("marsh"), t("thicket"));
    let mut nav = app.world_mut().resource_mut::<Nav>();
    let n = MAP as i32;
    for y in 0..n {
        for x in 0..n {
            let p = Vec2::new(x as f32, y as f32);
            let lake = p.distance(Vec2::new(135.0, 135.0));
            let dune = p.distance(Vec2::new(30.0, 135.0));
            let id = if lake < 18.0 {
                Some(deep)
            } else if lake < 24.0 {
                Some(shallow)
            } else if lake < 28.0 && x < 125 {
                Some(marsh)
            } else if (120..128).contains(&x) && (20..90).contains(&y) {
                Some(cliff)
            } else if dune < 20.0 {
                Some(sand)
            } else if p.distance(Vec2::new(140.0, 50.0)) < 9.0 {
                Some(thicket)
            } else {
                None
            };
            if let Some(id) = id {
                nav.set_terrain(IVec2::new(x, y), id);
            }
        }
    }
}

/// Every building of the faction in rows (a few as foundations), every unit
/// in rows in front of them, a rival army, and one of each node type.
fn populate(app: &mut App, me: PlayerId, rival: PlayerId) {
    let entities = with_commands(app, |c, data, players| {
        let mine = players.get(me);
        let theirs = players.get(rival);
        let faction = data.faction(mine.faction);
        let mut spawned = Vec::new();
        let (mut x, mut z, mut row_depth) = (8.0f32, 92.0f32, 0.0f32);
        for (i, b) in faction.building_ids.iter().enumerate() {
            let def = data.building(*b);
            if def.requires_node_id.is_some() {
                continue;
            }
            let [w, _, d] = def.size;
            if x + w > 118.0 {
                x = 8.0;
                z += row_depth + 3.0;
                row_depth = 0.0;
            }
            let complete = i % 7 != 3;
            spawned.push(spawn_building(c, data, mine, *b, Vec2::new(x + w * 0.5, z + d * 0.5), 0.0, complete));
            x += w + 3.0;
            row_depth = row_depth.max(d);
        }
        let (mut x, mut z) = (8.0f32, 60.0f32);
        for u in &faction.unit_ids {
            let def = data.unit(*u);
            if x + def.size[0] > 110.0 {
                x = 8.0;
                z += 6.0;
            }
            spawned.push(spawn_unit(c, data, mine, *u, Vec2::new(x + def.size[0] * 0.5, z), 0.4));
            x += def.size[0] + 2.2;
        }
        let rival_faction = data.faction(theirs.faction);
        for (i, u) in rival_faction.unit_ids.iter().take(12).enumerate() {
            spawned.push(spawn_unit(c, data, theirs, *u, Vec2::new(20.0 + i as f32 * 4.0, 45.0), std::f32::consts::PI));
        }
        for (i, n) in data.node_ids().enumerate() {
            let p = Vec2::new(10.0 + i as f32 * 8.0, 25.0);
            spawn_node(c, data, n, p, 0.3 * i as f32, None);
            // A small grove of every tree type for scale.
            if data.resource(data.node(n).resource_id).id == "wood" {
                for k in 1..6 {
                    spawn_node(c, data, n, p + Vec2::new((k % 3) as f32 * 1.6, -2.0 * (k / 3 + 1) as f32), 0.0, None);
                }
            }
        }
        spawned
    });
    // Show bars and outlines: damage every third thing, select a few units.
    for (i, e) in entities.iter().enumerate() {
        let mut ent = app.world_mut().entity_mut(*e);
        if i % 3 == 0
            && let Some(mut hp) = ent.get_mut::<Health>()
        {
            hp.hp *= 0.3 + 0.2 * (i % 4) as f32;
        }
        if let Some(mut c) = ent.get_mut::<UnderConstruction>() {
            c.progress = 0.15 + 0.2 * (i % 4) as f32;
        }
        if ent.contains::<Unit>() && i % 5 == 0 {
            ent.insert(Selected);
        }
    }
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
    FogOfWar { enabled: true, cell, width: n, height: n, teams: vec![team, TeamFog::default()] }
}

/// Every weapon repeats its attack (windup, hit, recovery) as af_combat
/// drives it; turrets sweep their aim.
fn swing_weapons(clock: Res<SimClock>, data: Res<Data>, mut q: Query<&mut Weapons>) {
    for (i, mut w) in q.iter_mut().enumerate() {
        for s in &mut w.slots {
            let windup = data.weapon(s.weapon).windup;
            let t = (clock.elapsed + i as f32 * 0.137).rem_euclid(windup + 1.5);
            (s.windup, s.anim) = combat_slot_state(t, windup);
            s.aim_yaw = (clock.elapsed * 0.5 + i as f32).sin() * 1.2;
        }
    }
}

fn circle_flyers(clock: Res<SimClock>, mut q: Query<(&Mobile, &mut Position, &mut Facing)>) {
    for (i, (m, mut p, mut f)) in q.iter_mut().enumerate() {
        if m.flying {
            let a = clock.elapsed * 0.3 + i as f32;
            p.0 = Vec2::new(70.0, 75.0) + Vec2::new(a.cos(), a.sin()) * 20.0;
            f.0 = af_geom::yaw_of(Vec2::new(-a.sin(), a.cos()));
        }
    }
}

fn screenshot_when_ready(mut commands: Commands, mut plan: ResMut<Plan>) {
    plan.frames += 1;
    let Some(path) = plan.screenshot.clone() else { return };
    if plan.frames != 90 {
        return;
    }
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path)).observe(|_c: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
        exit.write(AppExit::Success);
    });
}
