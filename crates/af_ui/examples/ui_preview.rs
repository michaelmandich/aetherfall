//! Shows the UI on its own, without the other game crates, to check the
//! layout by eye:
//!
//! ```text
//! cargo run -p af_ui --example ui_preview                          # skirmish menu
//! cargo run -p af_ui --example ui_preview -- hud                   # HUD, town centre selected
//! cargo run -p af_ui --example ui_preview -- hud --select workers  # or: army, mixed, none
//! cargo run -p af_ui --example ui_preview -- hud --hover Peasant   # hover a button by its label
//! cargo run -p af_ui --example ui_preview -- hud --game-over
//! ... --screenshot shot.png                                     # save a screenshot and exit
//! ```
//!
//! The HUD world is flat grass with a town centre (with a queue), a few
//! workers, soldiers and an enemy army. Nothing moves (no simulation crate
//! is loaded) and there are no 3D visuals.

use af_core::ext::input::{ControlGroups, RtsCamera};
use af_core::prelude::*;
use af_core::testing::{flat_world, test_data, with_commands};
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
    if hud {
        flat_world(&mut app, 240.0, &[("ironhold", 0), ("dune", 1), ("sylvan", 2)]);
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
