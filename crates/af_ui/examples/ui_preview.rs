//! Shows the UI on its own, without the other game crates, to check the
//! layout by eye:
//!
//! ```text
//! cargo run -p af_ui --example ui_preview                          # skirmish menu
//! cargo run -p af_ui --example ui_preview -- --opponents 5 --map huge --water seas --site shattered
//!                                                                  # the menu, preset
//! cargo run -p af_ui --example ui_preview -- hud                   # HUD, town centre selected
//! cargo run -p af_ui --example ui_preview -- hud --select workers  # or: army, mixed, none
//! cargo run -p af_ui --example ui_preview -- hud --hover Peasant   # hover a button by its label
//! cargo run -p af_ui --example ui_preview -- hud --game-over
//! cargo run -p af_ui --example ui_preview -- hud --world 1024 --painted   # a big painted map
//! ... --screenshot shot.png                                     # save a screenshot and exit
//! ```
//!
//! The HUD world is flat grass (`--world` metres square, 240 by default)
//! with a town centre (with a queue), a few workers, soldiers and an enemy
//! army. `--painted` paints it like a generated map (a sea edge, two rivers
//! with fords, a lake, mountains with a pass, highlands, marsh, thickets, a
//! cratered meteor site) and gives it map features, so the minimap and the
//! match's map notice can be checked. Nothing moves (no simulation crate is
//! loaded) and there are no 3D visuals.

use af_core::ext::input::{ControlGroups, RtsCamera};
use af_core::ext::map::{MapEdge, MapFeatures, MapSize, MeteorSite, ResolvedMapOptions, RiverCount, RiverLine, SiteChoice, SiteKind, WaterLevel};
use af_core::prelude::*;
use af_core::testing::{flat_world, test_data, with_commands};
use af_ui::menu::MenuSettings;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use bevy::window::WindowResolution;

#[derive(Resource)]
struct ScreenshotAt(String, u32);

/// Label of the button the preview keeps hovered.
#[derive(Resource)]
struct HoverLabel(String);

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hud = args.iter().any(|a| a == "hud");

    let mut app = App::new();
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window { title: "Aetherfall UI preview".into(), resolution: WindowResolution::new(1600, 900), ..default() }),
                ..default()
            })
            // The repository's assets (the UI skin), not `crates/af_ui/assets`.
            .set(AssetPlugin { file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(), ..default() }),
    );
    app.insert_state(if hud { GameState::Playing } else { GameState::Menu });
    app.insert_resource(Data(test_data()));
    app.add_plugins((af_core::CorePlugin, af_ui::HudPlugin));
    if !hud {
        match menu_settings(&args) {
            Ok(settings) => {
                app.insert_resource(settings);
            }
            Err(e) => {
                eprintln!("ui_preview: {e}");
                return AppExit::error();
            }
        }
    }
    if hud {
        let side = match arg_value(&args, "--world").map(|v| v.parse::<f32>()) {
            None => 240.0,
            Some(Ok(side)) if (160.0..=1500.0).contains(&side) => side,
            Some(_) => {
                eprintln!("ui_preview: --world wants a side in metres, 160-1500");
                return AppExit::error();
            }
        };
        flat_world(&mut app, side, &[("ironhold", 0), ("dune", 1), ("sylvan", 2)]);
        if args.iter().any(|a| a == "--painted") {
            paint_map(&mut app, side);
        }
        app.insert_resource(LocalPlayer(Some(PlayerId(0))));
        populate(&mut app, arg_value(&args, "--select").as_deref().unwrap_or("tc"));
        app.add_systems(OnEnter(GameState::Playing), |mut commands: Commands| {
            commands.spawn((RtsCamera, Camera3d::default(), Transform::from_xyz(120.0, 60.0, 160.0).looking_at(Vec3::new(120.0, 0.0, 120.0), Vec3::Y)));
        });
        if let Some(label) = arg_value(&args, "--hover") {
            app.insert_resource(HoverLabel(label));
            app.add_systems(Update, hold_hover.in_set(ClientSet::Input));
        }
        if args.iter().any(|a| a == "--game-over") {
            app.insert_resource(GameOutcome { ended: true, winning_team: Some(1), time: 1834.0 });
        }
    }
    if let Some(path) = arg_value(&args, "--screenshot") {
        app.insert_resource(ScreenshotAt(path, 0));
        app.add_systems(Update, screenshot_then_exit);
    }
    app.run()
}

/// The menu's settings from `--opponents N` (1-5) and the map flags
/// (`--map`, `--water`, `--rivers`, `--site`, `--map-size`), as the game's
/// command line takes them.
fn menu_settings(args: &[String]) -> Result<MenuSettings, String> {
    let mut s = MenuSettings::default();
    let bad = |flag: &str, v: &str| format!("{flag}: unknown value `{v}`");
    if let Some(v) = arg_value(args, "--opponents") {
        let n: usize = v.parse().map_err(|_| bad("--opponents", &v))?;
        s.opponents = vec![None; n.clamp(1, af_ui::menu::MAX_OPPONENTS)];
    }
    if let Some(v) = arg_value(args, "--map") {
        s.map.size = MapSize::parse(&v).ok_or_else(|| bad("--map", &v))?;
    }
    if let Some(v) = arg_value(args, "--water") {
        s.map.water = WaterLevel::parse(&v).ok_or_else(|| bad("--water", &v))?;
    }
    if let Some(v) = arg_value(args, "--rivers") {
        s.map.rivers = RiverCount::parse(&v).ok_or_else(|| bad("--rivers", &v))?;
    }
    if let Some(v) = arg_value(args, "--site") {
        s.map.site = SiteChoice::parse(&v).ok_or_else(|| bad("--site", &v))?;
    }
    if let Some(v) = arg_value(args, "--map-size") {
        s.map_metres = v.parse().map_err(|_| bad("--map-size", &v))?;
    }
    Ok(s)
}

/// Paint the flat world like a generated map and publish its features: an
/// ocean edge in the north with shallows and a beach, two rivers (one a
/// tributary) with fords, a lake, a mountain range with a pass, highlands,
/// marsh, thickets and a cratered meteor site in the middle.
fn paint_map(app: &mut App, s: f32) {
    let data = app.world().resource::<Data>().0.clone();
    let world = app.world_mut();
    let mut nav = world.resource_mut::<Nav>();
    let mut paint = |id: &str, points: &[Vec2], half: f32| {
        let t = data.terrain_id(id).unwrap_or_else(|| panic!("no terrain {id}")).0;
        for seg in points.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let (lo, hi) = ((a.min(b) - half).floor().as_ivec2(), (a.max(b) + half).ceil().as_ivec2());
            for y in lo.y..=hi.y {
                for x in lo.x..=hi.x {
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let k = if a == b { 0.0 } else { ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0) };
                    if p.distance(a + (b - a) * k) <= half && nav.in_bounds(IVec2::new(x, y)) {
                        nav.set_terrain(IVec2::new(x, y), t);
                    }
                }
            }
        }
    };
    let at = |x: f32, z: f32| Vec2::new(x * s, z * s);
    // The sea along the north edge (z = s), with a wavy shore.
    let shore: Vec<Vec2> = (0..=40).map(|i| at(i as f32 / 40.0, 0.86 + 0.02 * (i as f32 * 0.9).sin())).collect();
    paint("sand", &shore, 0.04 * s);
    paint("shallow_water", &shore.iter().map(|p| *p + Vec2::new(0.0, 0.03 * s)).collect::<Vec<_>>(), 0.025 * s);
    for k in 0..6 {
        paint("ocean", &shore.iter().map(|p| *p + Vec2::new(0.0, 0.06 * s + k as f32 * 0.02 * s)).collect::<Vec<_>>(), 0.02 * s);
    }
    // Marsh and thickets.
    paint("marsh", &[at(0.12, 0.3), at(0.2, 0.35)], 0.03 * s);
    paint("thicket", &[at(0.7, 0.2), at(0.78, 0.26)], 0.025 * s);
    paint("thicket", &[at(0.3, 0.72), at(0.36, 0.7)], 0.02 * s);
    // Highlands and a mountain range with a pass; an edge massif.
    paint("cliff", &[at(0.6, 0.65), at(0.72, 0.6)], 0.02 * s);
    paint("mountain", &[at(0.08, 0.6), at(0.2, 0.55), at(0.3, 0.58)], 0.022 * s);
    paint("mountain", &[at(0.34, 0.6), at(0.42, 0.66)], 0.022 * s);
    paint("mountain", &[at(1.0, 0.1), at(0.94, 0.04)], 0.06 * s);
    // A lake.
    paint("shallow_water", &[at(0.78, 0.42), at(0.82, 0.44)], 0.045 * s);
    paint("deep_water", &[at(0.78, 0.42), at(0.82, 0.44)], 0.03 * s);
    // Two rivers: one from the west edge to the sea, a tributary from the south.
    let river = [at(0.0, 0.18), at(0.22, 0.24), at(0.38, 0.4), at(0.52, 0.52), at(0.55, 0.7), at(0.5, 0.88)];
    let tributary = [at(0.62, 0.0), at(0.6, 0.18), at(0.66, 0.3), at(0.52, 0.52)];
    for line in [&river[..], &tributary[..]] {
        paint("shallow_water", line, 4.5);
        paint("deep_water", line, 2.5);
    }
    let fords = [river[1].lerp(river[2], 0.5), river[3].lerp(river[4], 0.4), tributary[1].lerp(tributary[2], 0.5)];
    for f in fords {
        paint("ford", &[f, f], 6.0);
    }
    // The meteor site: a cliff rim with gaps round a scorched floor.
    let (centre, rim) = (at(0.5, 0.5), 0.1 * s);
    paint("scorched", &[centre, centre], rim);
    let arc = |from: f32, to: f32| (0..=12).map(|i| centre + Vec2::from_angle(from + (to - from) * i as f32 / 12.0) * rim).collect::<Vec<_>>();
    for (from, to) in [(0.3, 1.8), (2.3, 3.6), (4.1, 5.9)] {
        paint("cliff", &arc(from, to), 1.5);
    }
    let points = |line: &[Vec2]| line.to_vec();
    world.insert_resource(MapFeatures {
        options: ResolvedMapOptions { size: MapSize::Huge, water: WaterLevel::Coast, rivers: 2, site: SiteKind::Crater },
        land_min: Vec2::ZERO,
        land_max: Vec2::new(s, 0.86 * s),
        ocean_edges: vec![MapEdge::North],
        sites: vec![MeteorSite { kind: SiteKind::Crater, center: centre, radius: rim }],
        rivers: vec![RiverLine { points: points(&river), half_widths: vec![2.5; river.len()], fords: vec![1, 3] }],
        mountains: Vec::new(),
    });
}

fn populate(app: &mut App, select: &str) {
    let (tc, workers, army) = with_commands(app, |c, data, players| {
        let me = &players.list[0];
        let tc = spawn_building(c, data, me, data.building_id("town_center").unwrap(), Vec2::new(70.0, 120.0), 0.0, true);
        spawn_building(c, data, me, data.building_id("house").unwrap(), Vec2::new(82.0, 112.0), 0.0, true);
        let mut workers = Vec::new();
        for i in 0..5 {
            let e = spawn_unit(c, data, me, data.unit_id("peasant").unwrap(), Vec2::new(60.0 + i as f32 * 2.0, 130.0), 0.0);
            if i > 2 {
                c.entity(e).remove::<Idle>();
            }
            workers.push(e);
        }
        let mut army = Vec::new();
        for (i, id) in ["spearman", "spearman", "slinger", "scout_rider"].into_iter().enumerate() {
            army.push(spawn_unit(c, data, me, data.unit_id(id).unwrap(), Vec2::new(90.0 + i as f32 * 2.0, 130.0), 0.0));
        }
        let enemy = &players.list[1];
        for i in 0..8 {
            spawn_unit(c, data, enemy, data.unit_id("spearman").unwrap(), Vec2::new(160.0 + (i % 4) as f32 * 2.0, 60.0 + (i / 4) as f32 * 2.0), 0.0);
        }
        for i in 0..12 {
            spawn_node(c, data, data.node_id("oak").unwrap(), Vec2::new(40.0 + (i % 4) as f32 * 1.5, 90.0 + (i / 4) as f32 * 1.5), 0.0, None);
        }
        (tc, workers, army)
    });
    let mut groups = ControlGroups::default();
    groups.set(1, &army);
    groups.set(2, &workers);
    app.insert_resource(groups);
    let selected: Vec<Entity> = match select {
        "workers" => workers,
        "army" => army,
        "mixed" => workers.into_iter().chain(army).chain([tc]).collect(),
        "none" => Vec::new(),
        _ => vec![tc],
    };
    for e in selected {
        app.world_mut().entity_mut(e).insert(Selected);
    }
    let peasant = test_data().unit_id("peasant").unwrap();
    let item = QueueItem { kind: QueueKind::Unit(peasant), paid: ResAmounts::ZERO, time: 14.0 };
    if let Some(mut queue) = app.world_mut().get_mut::<ProductionQueue>(tc) {
        queue.items.extend([item; 3]);
        queue.progress = 6.0;
    }
    let mut notices = app.world_mut().resource_mut::<Notices>();
    notices.push(None, NoticeKind::Info, "The red star grows brighter", None, 0.0);
    notices.push(Some(PlayerId(0)), NoticeKind::Warning, "Not enough wood", None, 0.0);
    notices.push(Some(PlayerId(0)), NoticeKind::Alert, "Your town is under attack", Some(Vec2::new(70.0, 120.0)), 0.0);
}

/// Keeps the labelled button "hovered" (the UI resets it every frame
/// because the real cursor is elsewhere) so its tooltip shows.
fn hold_hover(label: Res<HoverLabel>, labels: Query<(&ChildOf, &Text)>, mut buttons: Query<&mut Interaction, With<Button>>) {
    for (parent, text) in &labels {
        if text.0 == label.0
            && let Ok(mut interaction) = buttons.get_mut(parent.parent())
        {
            *interaction = Interaction::Hovered;
            return;
        }
    }
}

fn screenshot_then_exit(mut commands: Commands, mut at: ResMut<ScreenshotAt>) {
    at.1 += 1;
    if at.1 == 30 {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(at.0.clone())).observe(
            |_captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
    }
}
