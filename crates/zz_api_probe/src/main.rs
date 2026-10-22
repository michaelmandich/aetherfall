//! zz_api_probe: a compile-verified probe of the Bevy 0.19.1 API surface the RTS uses.
//!
//! Every snippet in `docs/bevy-0.19-notes.md` is copied from this file.
//!
//! * `cargo check -p zz_api_probe`            -> proves the API names/signatures.
//! * `cargo run   -p zz_api_probe -- verify`  -> runs the headless runtime checks
//!   (fixed-step counting, despawn recursion, error severity, ...) and prints results.
//! * `cargo test  -p zz_api_probe`            -> same checks as unit tests.
//! * `cargo run   -p zz_api_probe`            -> opens a window, takes a screenshot of the
//!   primary window after ~60 frames, writes it to `zz_api_probe_screenshot.png`, then exits.

mod custom_material;

use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

// Everything not listed below comes from `bevy::prelude::*`.
use bevy::asset::RenderAssetUsages;
use bevy::camera::ScalingMode;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::error::{BevyError, Severity};
use bevy::ecs::system::RunSystemOnce;
use bevy::image::ImageSampler;
use bevy::input::common_conditions::input_just_pressed;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::light::NotShadowCaster;
use bevy::log::{Level, LogPlugin};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use bevy::state::app::StatesPlugin;
use bevy::time::common_conditions::on_timer;
use bevy::time::TimeUpdateStrategy;
use bevy::window::{CursorOptions, PresentMode, PrimaryWindow, WindowResolution};
use bevy::camera::Hdr;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::render::batching::NoAutomaticBatching;

// ============================================================================
// Components, resources, states, sets, messages, events
// ============================================================================

#[derive(Component, Debug, Clone, Copy, PartialEq)]
struct Health(i32);

#[derive(Component, Debug, Clone, Copy, Default)]
struct Velocity(Vec3);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Team(u8);

#[derive(Component)]
struct Selected;

/// Required components: spawning `Unit` alone also inserts Transform, Visibility,
/// Health(100), Velocity::default() and a Name (unless the bundle already has them).
#[derive(Component, Default)]
#[require(Transform, Visibility, Health(100), Velocity, Name = Name::new("Unit"))]
struct Unit;

#[derive(Component)]
struct GoldText;

#[derive(Component)]
struct SelectionBox;

#[derive(Component)]
struct FogPlane;

#[derive(Resource, Default, Debug)]
struct Score(u32);

#[derive(Resource)]
struct FogImage(Handle<Image>);

#[derive(Resource, Default)]
struct DragStart(Option<Vec2>);

#[derive(Resource, Default)]
struct FixedCount(u32);

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
enum GameState {
    #[default]
    Loading,
    InGame,
    Paused,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SimSet {
    Input,
    Orders,
    Movement,
    Combat,
    Cleanup,
}

/// Buffered message (0.17+ name for the old `Event` + `EventReader/EventWriter`).
#[derive(Message, Debug, Clone)]
struct DamageMsg {
    target: Entity,
    amount: i32,
}

/// Global observer event: triggered with `commands.trigger(..)`, handled by `app.add_observer(..)`.
#[derive(Event, Debug, Clone)]
struct GameOver {
    winner: u8,
}

/// Entity-targeted observer event. The field named `entity` is the target.
/// Runs global observers AND observers attached to that entity with `.observe(..)`.
#[derive(EntityEvent, Debug, Clone)]
struct Attacked {
    entity: Entity,
    damage: i32,
}

/// Propagating entity event: bubbles up the ChildOf hierarchy until `propagate(false)`.
#[derive(EntityEvent, Debug, Clone)]
#[entity_event(propagate)]
struct Hit {
    entity: Entity,
}

// ============================================================================
// main
// ============================================================================

fn main() -> AppExit {
    if std::env::args().any(|a| a == "verify") {
        run_headless_checks();
        return AppExit::Success;
    }
    windowed_app().run()
}

fn windowed_app() -> App {
    let mut app = App::new();
    // Fallback for errors returned by systems/commands/observers that carry no explicit
    // severity. Default is `match_severity`, which PANICS on unclassified errors.
    // May be called only once per App; call it before adding plugins.
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "zz_api_probe".into(),
                    // Physical pixels, u32 (not f32 logical like older Bevy).
                    resolution: WindowResolution::new(1280, 720),
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                ..default()
            })
            .set(LogPlugin {
                level: Level::INFO,
                filter: "wgpu=error,naga=warn".into(),
                ..default()
            })
            // `.set()` returned a PluginGroupBuilder, so `.disable()` is available here.
            // On a bare group use `DefaultPlugins.build().disable::<X>()`.
            .disable::<bevy::audio::AudioPlugin>(),
    )
    // Mesh picking is NOT part of DefaultPlugins; add it to get Pointer<..> events on meshes.
    .add_plugins(MeshPickingPlugin)
    .add_plugins((SimPlugin, ProbeRenderPlugin, custom_material::CustomMaterialPlugin));
    app
}

/// A `PluginGroup` of our own.
struct ProbePlugins;

impl PluginGroup for ProbePlugins {
    fn build(self) -> bevy::app::PluginGroupBuilder {
        bevy::app::PluginGroupBuilder::start::<Self>()
            .add(SimPlugin)
            .add(ProbeRenderPlugin)
    }
}

// ============================================================================
// Simulation plugin: pure ECS, runs headless
// ============================================================================

struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .add_message::<DamageMsg>()
            .init_resource::<Score>()
            .init_resource::<FixedCount>()
            .insert_resource(Time::<Fixed>::from_hz(20.0))
            .configure_sets(
                FixedUpdate,
                (
                    SimSet::Input,
                    SimSet::Orders,
                    SimSet::Movement,
                    SimSet::Combat,
                    SimSet::Cleanup,
                )
                    .chain()
                    .run_if(in_state(GameState::InGame)),
            )
            .add_systems(Startup, spawn_sim_entities)
            .add_systems(
                Update,
                finish_loading.run_if(in_state(GameState::Loading)),
            )
            .add_systems(OnEnter(GameState::InGame), on_enter_ingame)
            .add_systems(OnExit(GameState::InGame), on_exit_ingame)
            .add_systems(FixedPreUpdate, count_fixed_ticks)
            .add_systems(
                FixedUpdate,
                (
                    deal_damage.in_set(SimSet::Combat),
                    apply_damage.in_set(SimSet::Combat).after(deal_damage),
                    movement.in_set(SimSet::Movement),
                    check_victory.in_set(SimSet::Cleanup),
                ),
            )
            .add_systems(
                Update,
                (
                    log_score.run_if(on_timer(Duration::from_secs(5))),
                    query_showcase,
                    sorted_and_many,
                    fallible_system,
                    single_and_populated,
                    paramset_showcase,
                ),
            )
            .add_systems(PostUpdate, changed_health_report)
            .add_observer(on_unit_added)
            .add_observer(on_unit_removed)
            .add_observer(on_game_over)
            // Observers accept run conditions too (0.19).
            .add_observer(on_any_attacked.run_if(in_state(GameState::InGame)));
    }
}

fn spawn_sim_entities(mut commands: Commands) {
    // Tuples are bundles. Unit's #[require] fills in Transform/Visibility/Health/Velocity/Name.
    commands.spawn((Unit, Team(0), Transform::from_xyz(-4.0, 0.0, 0.0), Selected));
    commands.spawn((
        Unit,
        Team(1),
        Name::new("Grunt"),
        Transform::from_xyz(4.0, 0.0, 0.0),
        Velocity(Vec3::new(-0.5, 0.0, 0.0)),
    ));

    // Hierarchy with the children! macro (children get ChildOf(parent) automatically).
    let barracks = commands
        .spawn((
            Name::new("Barracks"),
            Team(0),
            Transform::from_xyz(0.0, 0.0, -6.0),
            Visibility::default(),
            children![
                (Name::new("Flag"), Transform::from_xyz(0.0, 3.0, 0.0)),
                (Name::new("Door"), Transform::from_xyz(0.0, 0.0, 1.0)),
            ],
        ))
        // Entity-scoped observer: only runs for Attacked events targeting this entity.
        .observe(|attacked: On<Attacked>, mut q: Query<&mut Health>| {
            if let Ok(mut hp) = q.get_mut(attacked.entity) {
                hp.0 -= attacked.damage;
            }
        })
        .insert(Health(500))
        .id();

    // with_children closure form, and explicit ChildOf.
    commands.entity(barracks).with_children(|parent| {
        parent.spawn((Name::new("Window"), Transform::from_xyz(1.0, 1.0, 1.0)));
    });
    commands.spawn((Name::new("Chimney"), Transform::default(), ChildOf(barracks)));
    let banner = commands.spawn((Name::new("Banner"), Transform::default())).id();
    commands.entity(barracks).add_child(banner);

    // Trigger an EntityEvent: runs entity observer above + global `on_any_attacked`.
    commands.trigger(Attacked {
        entity: barracks,
        damage: 5,
    });
    // Same, constructing the event from the EntityCommands' entity:
    commands
        .entity(barracks)
        .trigger(|entity| Attacked { entity, damage: 1 });
}

fn finish_loading(mut next: ResMut<NextState<GameState>>) {
    next.set(GameState::InGame);
}

fn on_enter_ingame(mut score: ResMut<Score>) {
    score.0 += 1;
    info!("entered InGame");
}

fn on_exit_ingame() {
    info!("left InGame");
}

fn count_fixed_ticks(mut count: ResMut<FixedCount>) {
    count.0 += 1;
}

fn movement(time: Res<Time>, mut q: Query<(&mut Transform, &Velocity), With<Unit>>) {
    // Inside FixedUpdate, `Res<Time>` is the fixed clock: delta_secs() == timestep.
    let dt = time.delta_secs();
    for (mut transform, velocity) in &mut q {
        transform.translation += velocity.0 * dt;
    }
}

fn deal_damage(mut writer: MessageWriter<DamageMsg>, q: Query<(Entity, &Team), With<Unit>>) {
    for (entity, team) in &q {
        if team.0 == 1 {
            writer.write(DamageMsg {
                target: entity,
                amount: 1,
            });
        }
    }
}

fn apply_damage(
    mut reader: MessageReader<DamageMsg>,
    mut q: Query<&mut Health>,
    mut commands: Commands,
) {
    for msg in reader.read() {
        let Ok(mut hp) = q.get_mut(msg.target) else {
            continue;
        };
        hp.0 -= msg.amount;
        if hp.0 <= 0 {
            // despawn() is recursive over Children. try_despawn() = no warning if already gone.
            commands.entity(msg.target).try_despawn();
        }
    }
}

fn check_victory(q: Query<&Team, With<Unit>>, mut commands: Commands, mut done: Local<bool>) {
    if *done {
        return;
    }
    let team1_alive = q.iter().any(|t| t.0 == 1);
    if !team1_alive {
        *done = true;
        commands.trigger(GameOver { winner: 0 });
    }
}

fn on_game_over(game_over: On<GameOver>, mut next: ResMut<NextState<GameState>>) {
    info!("winner: {}", game_over.winner);
    next.set(GameState::Paused);
}

fn on_any_attacked(attacked: On<Attacked>) {
    debug!("{} took {} damage", attacked.entity, attacked.damage);
}

// Component lifecycle observers: Add / Insert / Discard (was Replace) / Remove / Despawn.
fn on_unit_added(add: On<Add, Unit>, names: Query<&Name>, mut score: ResMut<Score>) {
    if let Ok(name) = names.get(add.entity) {
        debug!("unit added: {name}");
    }
    score.0 += 10;
}

fn on_unit_removed(remove: On<Remove, Unit>, teams: Query<&Team>) {
    // `Remove` runs before the component is gone, so its data is still readable.
    if let Ok(team) = teams.get(remove.entity) {
        debug!("unit of team {} removed", team.0);
    }
}

fn log_score(score: Res<Score>) {
    info!("score = {}", score.0);
}

fn query_showcase(
    units: Query<(Entity, &Transform, Option<&Name>, Has<Selected>), With<Unit>>,
    changed: Query<Entity, (With<Unit>, Or<(Added<Health>, Changed<Health>)>)>,
    not_units: Query<&Name, (Without<Unit>, With<Children>)>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
) {
    for (entity, transform, name, is_selected) in &units {
        let _ = (entity, transform.translation, name.map(|n| n.as_str()), is_selected);
        let _ = units.contains(entity);
    }
    for entity in &changed {
        let _ = entity;
    }
    for name in &not_units {
        let _ = name;
    }
    for child_of in &parents {
        let _parent: Entity = child_of.parent();
    }
    for kids in &children {
        for child in kids.iter() {
            let _: Entity = child;
        }
    }
}

fn sorted_and_many(
    teams: Query<(Entity, &Team)>,
    selected: Query<Entity, With<Selected>>,
    names: Query<&Name>,
) {
    // QueryIter::sort / sort_by take the sort-key lens type as a turbofish.
    let by_team: Vec<(Entity, &Team)> = teams.iter().sort::<&Team>().collect();
    let by_team_desc: Vec<(Entity, &Team)> = teams
        .iter()
        .sort_by::<&Team>(|a, b| b.cmp(a))
        .collect();
    let _ = (by_team, by_team_desc);
    // iter_many: fetch query items for an iterator of entities (skips non-matching).
    for name in names.iter_many(selected.iter()) {
        let _ = name;
    }
}

/// Systems may return `Result`; `?` converts any std::error::Error into BevyError.
fn fallible_system(units: Query<&Health, With<Unit>>, all: Query<Entity, With<Unit>>) -> Result {
    let Some(first) = all.iter().next() else {
        return Ok(());
    };
    let hp = units.get(first)?; // QueryEntityError -> BevyError
    if hp.0 < -1000 {
        // Explicit severity: logged as a warning by the default handler instead of panicking.
        return Err(BevyError::warning("absurd health"));
    }
    // single() is a Result now (get_single() is gone).
    let _maybe = all.single().with_severity(Severity::Debug);
    Ok(())
}

/// `Single` skips the system (silently) unless exactly one entity matches.
/// `Option<Single<..>>` runs with None for zero matches (still skips for >1).
/// `Populated` skips unless at least one entity matches.
fn single_and_populated(
    selected: Single<(Entity, &Transform), With<Selected>>,
    maybe_cam: Option<Single<&Camera>>,
    units: Populated<&Unit>,
) {
    let (entity, transform) = *selected;
    let _ = (entity, transform, maybe_cam.is_some(), units.iter().count());
}

fn paramset_showcase(
    mut set: ParamSet<(
        Query<&mut Transform, With<Unit>>,
        Query<&Transform, With<Name>>,
    )>,
) {
    for mut t in set.p0().iter_mut() {
        t.translation.y = 0.0;
    }
    let count = set.p1().iter().count();
    let _ = count;
}

fn changed_health_report(q: Query<(&Name, &Health), Changed<Health>>) {
    for (name, hp) in &q {
        trace!("{name}: {}", hp.0);
    }
}

// ============================================================================
// Render / UI / input plugin (needs DefaultPlugins)
// ============================================================================

struct ProbeRenderPlugin;

impl Plugin for ProbeRenderPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.12)))
            // Ambient light: resource GlobalAmbientLight (per-camera override: AmbientLight component).
            .insert_resource(GlobalAmbientLight {
                color: Color::WHITE,
                brightness: 400.0,
                ..default()
            })
            .init_resource::<DragStart>()
            .add_systems(Startup, (setup_scene, setup_ui, setup_fog).chain())
            .add_systems(
                Update,
                (
                    camera_controls,
                    ground_click.run_if(pointer_not_over_ui),
                    selection_box,
                    button_feedback,
                    update_gold_text,
                    update_fog.run_if(resource_exists::<FogImage>),
                    draw_gizmos,
                    world_to_screen,
                    toggle_pause.run_if(input_just_pressed(KeyCode::Escape)),
                    keyboard_showcase,
                    screenshot_then_exit,
                ),
            )
            // Compile-only coverage of more APIs (never runs).
            .add_systems(Update, (misc_api_probe, misc_writer).run_if(never))
            .add_observer(|inserted: On<Insert, Health>| {
                trace!("Health (re)inserted on {}", inserted.entity);
            })
            .add_observer(|discarded: On<Discard, Health>| {
                trace!("Health about to be replaced/removed on {}", discarded.entity);
            })
            .add_observer(|despawned: On<Despawn, Unit>| {
                trace!("unit {} despawning", despawned.entity);
            })
            .add_observer(|mut hit: On<Hit>| {
                hit.propagate(false); // stop bubbling to the parent
            })
            // Ordering: .chain(), .before(), .after(), .in_set(), and a set ordered vs a system.
            .configure_sets(Update, UiInputSet.before(issue_orders))
            .add_systems(
                Update,
                (
                    (read_input, issue_orders, after_orders).chain(),
                    ui_hotkeys.in_set(UiInputSet),
                    late_orders.after(issue_orders).before(after_orders),
                ),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct UiInputSet;

fn read_input() {}
fn issue_orders() {}
fn after_orders() {}
fn ui_hotkeys() {}
fn late_orders() {}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Camera: Camera3d requires Camera + Projection (perspective by default).
    commands.spawn((
        Camera3d::default(),
        Projection::from(PerspectiveProjection {
            fov: 45f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            ..default()
        }),
        Transform::from_xyz(0.0, 30.0, 22.0).looking_at(Vec3::ZERO, Vec3::Y),
        Msaa::Sample4,               // the default
        Tonemapping::TonyMcMapface,  // the default
    ));

    // Orthographic alternative (not spawned): use ..default_3d() for 3D cameras.
    let _ortho = Projection::from(OrthographicProjection {
        scaling_mode: ScalingMode::FixedVertical {
            viewport_height: 40.0,
        },
        scale: 1.0,
        ..OrthographicProjection::default_3d()
    });

    // Directional light with shadows (field is `shadow_maps_enabled`, not `shadows_enabled`).
    commands.spawn((
        DirectionalLight {
            illuminance: light_consts::lux::OVERCAST_DAY * 5.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(30.0, 50.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Ground: meshes.add() accepts anything Into<Mesh> (primitives and mesh builders).
    commands.spawn((
        Name::new("Ground"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(64.0, 64.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.25, 0.4, 0.2))),
        Transform::default(),
    ));

    // Shared handles => automatic batching/instancing of identical mesh+material.
    let cube = meshes.add(Cuboid::new(1.0, 2.0, 1.0));
    let _also_a_mesh: Mesh = Mesh::from(Cuboid::new(1.0, 1.0, 1.0));
    let red = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.2, 0.2),
        emissive: LinearRgba::rgb(0.2, 0.0, 0.0),
        perceptual_roughness: 0.8,
        ..default()
    });
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgba(0.3, 0.5, 1.0, 0.4),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    for i in 0..5 {
        commands.spawn((
            Unit,
            Team(1),
            Mesh3d(cube.clone()),
            MeshMaterial3d(red.clone()),
            Transform::from_xyz(i as f32 * 2.0 - 4.0, 1.0, 6.0)
                .with_scale(Vec3::splat(0.8))
                .with_rotation(Quat::from_rotation_y(0.3 * i as f32)),
        ));
    }
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(1.0))),
        MeshMaterial3d(glass),
        Transform::from_xyz(0.0, 1.0, 0.0),
        NotShadowCaster,
        Visibility::Visible, // Inherited (default) | Hidden | Visible
    ));

    // Color helpers (traits Mix / Luminance / Alpha are in the prelude).
    let base = Color::srgb(0.5, 0.4, 0.3);
    let _mixed = base.mix(&Color::WHITE, 0.25);
    let _light = base.lighter(0.1);
    let _dark = base.darker(0.1);
    let _faded = base.with_alpha(0.5);
    let _lin: LinearRgba = base.to_linear();
    let _hex = Color::srgb_u8(200, 180, 40);
}

fn setup_ui(mut commands: Commands, images: Res<Assets<Image>>) {
    let _ = images;
    // Full-screen root. Pickable::IGNORE so this transparent layout node does not
    // count as "UI under the cursor" (every Node is pickable and blocks by default).
    commands.spawn((
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Stretch,
            ..default()
        },
        Pickable::IGNORE,
        children![
            // Top resource bar.
            (
                Node {
                    width: percent(100),
                    height: px(32),
                    padding: UiRect::axes(px(8), px(4)),
                    column_gap: px(12),
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                children![(
                    Text::new("Gold: "),
                    TextFont::from_font_size(18.0),
                    TextColor(Color::WHITE),
                    children![(
                        TextSpan::new("100"),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(Color::srgb(1.0, 0.85, 0.2)),
                        GoldText,
                    )],
                )],
            ),
            // Bottom command panel.
            (
                Node {
                    height: px(120),
                    margin: UiRect::all(px(4)),
                    padding: UiRect::all(px(6)),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(6)),
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                    row_gap: px(6),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.1, 0.08, 0.05, 0.9)),
                BorderColor::all(Color::srgb(0.6, 0.5, 0.3)),
                children![
                    (
                        Button,
                        Node {
                            width: px(64),
                            height: px(64),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.25, 0.25, 0.25)),
                        children![Text::new("Build")],
                    ),
                    (
                        Node {
                            display: Display::None, // collapsed: takes no layout space
                            ..default()
                        },
                        Text::new("hidden"),
                    ),
                    (
                        Node::default(),
                        Text::new("invisible but laid out"),
                        Visibility::Hidden,
                    ),
                ],
            ),
        ],
    ));

    // Drag-select rectangle as an absolutely positioned UI node (screen space).
    commands.spawn((
        SelectionBox,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: px(0),
            height: px(0),
            border: UiRect::all(px(1)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.2, 1.0, 0.2, 0.1)),
        BorderColor::all(Color::srgb(0.2, 1.0, 0.2)),
        Visibility::Hidden,
        GlobalZIndex(100),
        Pickable::IGNORE,
    ));

    // Button with a picking observer (alternative to polling Interaction).
    commands
        .spawn((
            Button,
            Node {
                position_type: PositionType::Absolute,
                right: px(10),
                top: px(40),
                padding: UiRect::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.3, 0.2, 0.2)),
            ZIndex(1),
            children![Text::new("Quit")],
        ))
        .observe(|_click: On<Pointer<Click>>, mut exit: MessageWriter<AppExit>| {
            exit.write(AppExit::Success);
        });
}

const FOG_W: u32 = 64;
const FOG_H: u32 = 64;

fn setup_fog(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut image = Image::new_fill(
        Extent3d {
            width: FOG_W,
            height: FOG_H,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 200],
        TextureFormat::Rgba8UnormSrgb,
        // Keep MAIN_WORLD so the CPU copy stays mutable after upload.
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    let handle = images.add(image);

    // Fog overlay: unlit, alpha-blended, textured flat cuboid.
    commands.spawn((
        FogPlane,
        Mesh3d(meshes.add(Cuboid::new(64.0, 0.01, 64.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(handle.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.05, 0.0),
        NotShadowCaster,
    ));

    // Same image as a UI minimap.
    commands.spawn((
        ImageNode::new(handle.clone()),
        Node {
            position_type: PositionType::Absolute,
            right: px(10),
            bottom: px(140),
            width: px(160),
            height: px(160),
            ..default()
        },
        GlobalZIndex(10),
    ));
    commands.insert_resource(FogImage(handle));
}

fn update_fog(
    fog: Res<FogImage>,
    mut images: ResMut<Assets<Image>>,
    units: Query<&Transform, With<Unit>>,
) {
    // get_mut returns Option<AssetMut<Image>> (marks Modified only if actually mutated).
    let Some(mut image) = images.get_mut(&fog.0) else {
        return;
    };
    // `data` is Option<Vec<u8>>, tightly packed RGBA8 rows.
    let Some(data) = image.data.as_mut() else {
        return;
    };
    for px_rgba in data.chunks_exact_mut(4) {
        px_rgba[3] = 200;
    }
    for t in &units {
        let cell = ((t.translation.xz() + Vec2::splat(32.0)) / 64.0 * FOG_W as f32).as_uvec2();
        if cell.x < FOG_W && cell.y < FOG_H {
            let i = ((cell.y * FOG_W + cell.x) * 4) as usize;
            data[i + 3] = 0;
        }
    }
    // Per-pixel helper alternative:
    let _ = image.set_color_at(0, 0, Color::srgba(0.0, 0.0, 0.0, 0.8));
}

fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    motion: Res<AccumulatedMouseMotion>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>, // real time so the camera moves while the sim is paused
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let (mut transform, mut projection) = camera.into_inner();
    let mut pan = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        pan.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        pan.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        pan.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        pan.x += 1.0;
    }
    let speed = if keys.pressed(KeyCode::ShiftLeft) { 40.0 } else { 20.0 };
    transform.translation += pan.extend(0.0).xzy() * speed * time.delta_secs();

    if mouse.pressed(MouseButton::Middle) {
        transform.translation.x -= motion.delta.x * 0.05;
        transform.translation.z -= motion.delta.y * 0.05;
    }

    // Wheel: Line units (1.0 per notch) on most mice, Pixel units on touchpads.
    let notches = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
    };
    match &mut *projection {
        Projection::Perspective(p) => p.fov = (p.fov - notches * 0.05).clamp(0.3, 1.2),
        Projection::Orthographic(o) => o.scale = (o.scale - notches * 0.1).max(0.1),
        _ => {}
    }
}

/// Run condition: true when the mouse pointer is NOT over any UI node.
fn pointer_not_over_ui(hover_map: Res<HoverMap>, ui_nodes: Query<(), With<Node>>) -> bool {
    !hover_map
        .get(&PointerId::Mouse)
        .is_some_and(|hits| hits.keys().any(|e| ui_nodes.contains(*e)))
}

/// Cheaper alternative: only sees nodes that have an Interaction component (Buttons, or
/// nodes you add `Interaction` to).
#[allow(dead_code)]
fn pointer_over_interactive_ui(q: Query<&Interaction>) -> bool {
    q.iter().any(|i| *i != Interaction::None)
}

fn ground_click(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut selected: Query<&mut Velocity, With<Selected>>,
    selected_tf: Query<&Transform, With<Selected>>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return; // cursor outside the window
    };
    let (camera, camera_gt) = *camera;
    // viewport_to_world returns Result<Ray3d, ViewportConversionError>.
    let Ok(ray) = camera.viewport_to_world(camera_gt, cursor) else {
        return;
    };
    let Some(distance) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)) else {
        return;
    };
    let target = ray.get_point(distance);
    let _same = ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y));
    for (mut v, t) in selected.iter_mut().zip(selected_tf.iter()) {
        v.0 = (target - t.translation).normalize_or_zero() * 3.0;
    }
}

fn selection_box(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut drag: ResMut<DragStart>,
    rect: Single<(&mut Node, &mut Visibility), With<SelectionBox>>,
) {
    let (mut node, mut vis) = rect.into_inner();
    let cursor = window.cursor_position();
    if mouse.just_pressed(MouseButton::Left) {
        drag.0 = cursor;
    }
    if mouse.just_released(MouseButton::Left) {
        drag.0 = None;
        *vis = Visibility::Hidden;
        return;
    }
    if let (Some(start), Some(now)) = (drag.0, cursor) {
        let min = start.min(now);
        let size = (start - now).abs();
        node.left = px(min.x);
        node.top = px(min.y);
        node.width = Val::Px(size.x);
        node.height = px(size.y);
        *vis = Visibility::Visible;
    }
}

fn button_feedback(
    mut q: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, mut bg) in &mut q {
        bg.0 = match *interaction {
            Interaction::Pressed => Color::srgb(0.35, 0.75, 0.35),
            Interaction::Hovered => Color::srgb(0.35, 0.35, 0.35),
            Interaction::None => Color::srgb(0.25, 0.25, 0.25),
        };
    }
}

fn update_gold_text(score: Res<Score>, mut q: Query<&mut TextSpan, With<GoldText>>) {
    if !score.is_changed() {
        return;
    }
    for mut span in &mut q {
        **span = score.0.to_string();
    }
}

fn draw_gizmos(mut gizmos: Gizmos, selected: Query<&Transform, With<Selected>>) {
    // Gizmo circles/rects are drawn in the local XY plane; rotate onto the XZ ground.
    let flat = Quat::from_rotation_x(FRAC_PI_2);
    for t in &selected {
        gizmos.circle(
            Isometry3d::new(t.translation + Vec3::Y * 0.05, flat),
            0.9,
            Color::srgb(0.2, 1.0, 0.2),
        );
    }
    gizmos.line(Vec3::ZERO, Vec3::X * 5.0, Color::srgb(1.0, 0.0, 0.0));
    gizmos.rect(
        Isometry3d::new(Vec3::Y * 0.02, flat),
        Vec2::splat(10.0),
        Color::WHITE,
    );
    gizmos.grid(
        Isometry3d::new(Vec3::Y * 0.01, flat),
        UVec2::splat(16),
        Vec2::splat(4.0),
        Color::srgba(1.0, 1.0, 1.0, 0.1),
    );
}

fn world_to_screen(
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    units: Query<&GlobalTransform, With<Unit>>,
) {
    let (camera, camera_gt) = *camera;
    for gt in &units {
        // Result<Vec2, ViewportConversionError>, logical pixels, origin top-left.
        if let Ok(screen) = camera.world_to_viewport(camera_gt, gt.translation()) {
            let _ = screen;
        }
    }
}

fn toggle_pause(
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    match state.get() {
        GameState::InGame => {
            next.set(GameState::Paused);
            virtual_time.pause();
        }
        GameState::Paused => {
            next.set(GameState::InGame);
            virtual_time.unpause();
        }
        GameState::Loading => {}
    }
}

fn keyboard_showcase(keys: Res<ButtonInput<KeyCode>>, mut virtual_time: ResMut<Time<Virtual>>) {
    let _ = keys.just_pressed(KeyCode::Digit1)
        || keys.pressed(KeyCode::ControlLeft)
        || keys.just_released(KeyCode::Space)
        || keys.just_pressed(KeyCode::F1)
        || keys.just_pressed(KeyCode::Delete)
        || keys.just_pressed(KeyCode::Tab);
    if keys.just_pressed(KeyCode::Equal) {
        virtual_time.set_relative_speed(2.0);
    }
    if keys.just_pressed(KeyCode::Minus) {
        virtual_time.set_relative_speed(1.0);
    }
}

fn screenshot_then_exit(mut commands: Commands, mut frame: Local<u32>) {
    *frame += 1;
    if *frame == 60 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("zz_api_probe_screenshot.png"))
            // ScreenshotCaptured is an EntityEvent on the Screenshot entity.
            .observe(|_captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success); // AppExit is a Message
            });
    }
}

fn never() -> bool {
    false
}

/// Compile-only coverage of APIs mentioned in the cheat sheet that the game systems above
/// don't otherwise touch. Registered with `.run_if(never)`.
#[allow(clippy::too_many_arguments)]
fn misc_api_probe(
    mut commands: Commands,
    mut next: ResMut<NextState<GameState>>,
    mut damage: MessageMutator<DamageMsg>,
    mut images: ResMut<Assets<Image>>,
    fog: Option<Res<FogImage>>,
    asset_server: Res<AssetServer>,
    cameras: Query<Entity, With<Camera3d>>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
) {
    // `next.set_if_neq(..)` would hit DetectChangesMut::set_if_neq on ResMut and not compile;
    // deref explicitly to reach NextState::set_if_neq (no transition if already InGame).
    (*next).set_if_neq(GameState::InGame);
    next.reset();
    for msg in damage.read() {
        msg.amount += 1; // MessageMutator: read + mutate (+ write) the same message type
    }
    commands.write_message(DamageMsg {
        target: Entity::PLACEHOLDER,
        amount: 0,
    });
    let e = commands
        .spawn((
            Name::new("tmp"),
            DespawnOnExit(GameState::InGame), // was StateScoped
            NoAutomaticBatching,
        ))
        .id();
    commands.entity(e).despawn_children(); // was despawn_descendants
    commands.entity(e).despawn_related::<Children>();
    commands.entity(e).trigger(|entity| Hit { entity });
    commands.delayed().secs(1.0).spawn(Name::new("spawned one second later"));
    // GLTF scenes: SceneRoot was renamed WorldAssetRoot in 0.19.
    commands.spawn(WorldAssetRoot(asset_server.load("models/castle.gltf#Scene0")));
    for camera in &cameras {
        commands.entity(camera).insert(Hdr); // HDR is a marker component now
        commands.spawn((Node::default(), UiTargetCamera(camera)));
    }
    if let Some(fog) = fog
        && let Some(raw) = images.get_mut_untracked(&fog.0)
    {
        let _ = raw.width(); // &mut Image without change detection
    }
    for options in &cursor {
        let _ = (options.visible, options.grab_mode);
    }
    let mut map: HashMap<Entity, u32> = HashMap::default();
    let mut set: HashSet<Entity> = HashSet::default();
    map.insert(e, 1);
    set.insert(e);
    let _ = (TextLayout::justify(Justify::Center), TextLayout::no_wrap());
    let _font: Font = Font::from_bytes(Vec::new()); // ONE argument in 0.19.1
    let _vt = Time::<Virtual>::from_max_delta(Duration::from_secs(1));
    let _err = AppExit::error();
}

fn misc_writer(mut writer: MessageWriter<DamageMsg>) {
    writer.write_batch(Vec::new());
}

// ============================================================================
// Math probe
// ============================================================================

fn math_probe() -> f32 {
    let v3 = Vec3::new(1.0, 2.0, 3.0);
    let ground: Vec2 = v3.xz(); // (1, 3)
    let back: Vec3 = ground.extend(0.0); // (1, 3, 0)  (extend adds z)
    let world = Vec3::new(ground.x, 0.0, ground.y);
    let dir: Dir3 = Dir3::new(world).unwrap_or(Dir3::NEG_Z);
    let rot = Rot2::degrees(90.0);
    let rotated: Vec2 = rot * Vec2::X;
    let iso = Isometry3d::new(Vec3::ZERO, Quat::from_rotation_y(FRAC_PI_2));
    let moved: Vec3 = iso * Vec3::X;
    let angle = ops::atan2(rotated.y, rotated.x);
    let s = ops::sin(angle) + angle.sin(); // both fine; ops:: is the portable/deterministic-ish path
    let q = Quat::from_rotation_y(FRAC_PI_2);
    let _ = (back, dir, moved, q * Vec3::X, Vec2::from_angle(0.5), v3.distance(world));
    s
}

/// Bevy re-exports no RNG. A tiny deterministic xorshift for sim code (no `rand` dep).
#[derive(Resource, Clone, Copy)]
struct SimRng(u64);

impl SimRng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }
    fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
}

// ============================================================================
// Headless runtime checks (also used by `cargo test`)
// ============================================================================

fn run_headless_checks() {
    check_fixed_timesteps();
    check_manual_duration_and_max_delta();
    check_virtual_speed_and_pause();
    check_states_and_sim_plugin();
    check_messages_from_test();
    check_despawn_recursive();
    check_insert_on_dead_entity_panics();
    check_single_skips();
    check_transform_propagation();
    check_assets_headless();
    check_observers();
    let _ = math_probe();
    let mut rng = SimRng(0x9E37_79B9_7F4A_7C15);
    let _ = (rng.next_f32(), rng.next_u32());
    let _plugins = ProbePlugins; // type-checks the PluginGroup impl
    println!("ALL HEADLESS CHECKS PASSED");
}

fn fixed_counting_app(strategy: TimeUpdateStrategy, hz: f64) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(Time::<Fixed>::from_hz(hz))
        .insert_resource(strategy)
        .init_resource::<FixedCount>()
        .add_systems(FixedUpdate, count_fixed_ticks);
    app
}

fn fixed_count(app: &App) -> u32 {
    app.world().resource::<FixedCount>().0
}

fn check_fixed_timesteps() {
    let mut app = fixed_counting_app(TimeUpdateStrategy::FixedTimesteps(1), 20.0);
    let mut per_update = Vec::new();
    for _ in 0..6 {
        let before = fixed_count(&app);
        app.update();
        per_update.push(fixed_count(&app) - before);
    }
    println!("FixedTimesteps(1) @20Hz, fixed runs per app.update(): {per_update:?}");
    assert_eq!(per_update, vec![0, 1, 1, 1, 1, 1], "first update only primes the clock");

    let fixed = app.world().resource::<Time<Fixed>>();
    println!(
        "  Time<Fixed>: timestep={:?} elapsed={:?} overstep_fraction={}",
        fixed.timestep(),
        fixed.elapsed(),
        fixed.overstep_fraction()
    );

    // FixedTimesteps(n) above Time<Virtual>::max_delta (250 ms) is clamped!
    let mut app = fixed_counting_app(TimeUpdateStrategy::FixedTimesteps(10), 20.0);
    app.update();
    let before = fixed_count(&app);
    app.update();
    let clamped = fixed_count(&app) - before;
    println!("FixedTimesteps(10) @20Hz (500 ms > 250 ms max_delta): {clamped} runs");
    assert_eq!(clamped, 5);

    // Raise max_delta to fast-forward many steps per update.
    let mut app = fixed_counting_app(TimeUpdateStrategy::FixedTimesteps(10), 20.0);
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(Duration::from_secs(10));
    app.update();
    let before = fixed_count(&app);
    app.update();
    let ff = fixed_count(&app) - before;
    println!("FixedTimesteps(10) with max_delta=10s: {ff} runs");
    assert_eq!(ff, 10);
}

fn check_manual_duration_and_max_delta() {
    let step = Time::<Fixed>::from_hz(20.0).timestep();
    let mut app = fixed_counting_app(TimeUpdateStrategy::ManualDuration(step), 20.0);
    let mut per_update = Vec::new();
    for _ in 0..4 {
        let before = fixed_count(&app);
        app.update();
        per_update.push(fixed_count(&app) - before);
    }
    println!("ManualDuration(timestep) @20Hz: {per_update:?}");
    assert_eq!(per_update, vec![0, 1, 1, 1]);

    // Update-time delta in Update matches the manual duration.
    let delta = app.world().resource::<Time>().delta();
    println!("  Res<Time> delta in main world after update: {delta:?}");
    assert_eq!(delta, step);
}

fn check_virtual_speed_and_pause() {
    let mut app = fixed_counting_app(TimeUpdateStrategy::FixedTimesteps(1), 20.0);
    app.update();
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_relative_speed(2.0);
    let before = fixed_count(&app);
    app.update();
    let fast = fixed_count(&app) - before;
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    let before = fixed_count(&app);
    app.update();
    let paused = fixed_count(&app) - before;
    println!("relative_speed 2.0 -> {fast} fixed runs/update; paused -> {paused}");
    assert_eq!(fast, 2);
    assert_eq!(paused, 0);
}

fn sim_app() -> App {
    let mut app = App::new();
    // MinimalPlugins has no TransformPlugin, StatesPlugin, AssetPlugin or InputPlugin.
    app.add_plugins((MinimalPlugins, StatesPlugin, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(1))
        .add_plugins(SimPlugin);
    app
}

fn check_states_and_sim_plugin() {
    let mut app = sim_app();
    app.update(); // Startup + Loading -> NextState(InGame) queued
    app.update(); // StateTransition runs (after PreUpdate): OnEnter(InGame)
    let state = *app.world().resource::<State<GameState>>().get();
    println!("state after 2 updates: {state:?}");
    assert_eq!(state, GameState::InGame);
    for _ in 0..400 {
        app.update();
    }
    let state = *app.world().resource::<State<GameState>>().get();
    let score = app.world().resource::<Score>().0;
    println!("state after sim ran: {state:?}, score {score}");
    assert_eq!(state, GameState::Paused, "GameOver observer fired");
}

fn check_messages_from_test() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_message::<DamageMsg>();
    let target = app.world_mut().spawn(Health(10)).id();
    app.add_systems(Update, apply_damage);
    // Writing a message from a test (or any exclusive context):
    app.world_mut().write_message(DamageMsg { target, amount: 3 });
    app.update();
    let hp = app.world().get::<Health>(target).unwrap().0;
    println!("message from test applied: hp {hp}");
    assert_eq!(hp, 7);

    // run_system_once needs `use bevy::ecs::system::RunSystemOnce;` and returns Result.
    let count = app
        .world_mut()
        .run_system_once(|q: Query<&Health>| q.iter().count())
        .expect("system ran");
    assert_eq!(count, 1);
    let total: i32 = app
        .world_mut()
        .run_system_once(|q: Query<&Health>| q.iter().map(|h| h.0).sum::<i32>())
        .unwrap();
    assert_eq!(total, 7);
}

fn check_despawn_recursive() {
    let mut world = World::new();
    let parent = world
        .spawn((Name::new("parent"), children![Name::new("child")]))
        .id();
    let child = world.get::<Children>(parent).unwrap()[0];
    world.entity_mut(parent).despawn();
    let child_alive = world.get_entity(child).is_ok();
    println!("child alive after parent.despawn(): {child_alive}");
    assert!(!child_alive, "despawn is recursive over Children");
}

fn check_insert_on_dead_entity_panics() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let e = app.world_mut().spawn_empty().id();
    app.world_mut().despawn(e);

    app.add_systems(Update, move |mut commands: Commands| {
        commands.entity(e).try_insert(Health(1)); // silently ignored
        commands.entity(e).try_despawn(); // silently ignored
    });
    app.update();
    println!("try_insert / try_despawn on dead entity: no panic");

    let mut app2 = App::new();
    app2.add_plugins(MinimalPlugins);
    let e2 = app2.world_mut().spawn_empty().id();
    app2.world_mut().despawn(e2);
    app2.add_systems(Update, move |mut commands: Commands| {
        commands.entity(e2).insert(Health(1));
    });
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app2.update()));
    std::panic::set_hook(prev_hook);
    println!("insert() on dead entity panicked (default handler): {}", result.is_err());
    assert!(result.is_err());

    // With a non-panicking fallback handler the same command only logs.
    let mut app3 = App::new();
    app3.set_error_handler(bevy::ecs::error::warn);
    app3.add_plugins(MinimalPlugins);
    let e3 = app3.world_mut().spawn_empty().id();
    app3.world_mut().despawn(e3);
    app3.add_systems(Update, move |mut commands: Commands| {
        commands.entity(e3).insert(Health(1));
    });
    app3.add_systems(Update, |_s: Res<Score>| {}); // missing resource: warn, not panic
    app3.update();
    println!("with set_error_handler(warn): insert on dead entity + missing Res only warn");
}

fn check_single_skips() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<FixedCount>()
        .add_systems(
            Update,
            |_s: Single<&Selected>, mut c: ResMut<FixedCount>| c.0 += 1,
        );
    app.update(); // zero matches: skipped, no panic
    assert_eq!(app.world().resource::<FixedCount>().0, 0);
    app.world_mut().spawn(Selected);
    app.update();
    assert_eq!(app.world().resource::<FixedCount>().0, 1);
    println!("Single<..> skips silently with 0 matches, runs with 1");

    // A missing Res<T> is NOT a silent skip: it panics under the default handler.
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_systems(Update, |_s: Res<Score>| {});
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.update()));
    std::panic::set_hook(prev_hook);
    println!("missing Res<T> panics by default: {}", result.is_err());
    assert!(result.is_err());
}

fn check_transform_propagation() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin));
    let parent = app
        .world_mut()
        .spawn((
            Transform::from_xyz(10.0, 0.0, 0.0),
            children![Transform::from_xyz(1.0, 0.0, 0.0)],
        ))
        .id();
    app.update();
    let child = app.world().get::<Children>(parent).unwrap()[0];
    let gt = app.world().get::<GlobalTransform>(child).unwrap().translation();
    println!("child GlobalTransform with TransformPlugin: {gt}");
    assert_eq!(gt, Vec3::new(11.0, 0.0, 0.0));
}

fn check_assets_headless() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<Image>();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(1.0, 1.0, 1.0));
    app.update();
    let has = app.world().resource::<Assets<Mesh>>().get(&handle).is_some();
    println!("Assets<Mesh> headless (AssetPlugin + init_asset): {has}");
    assert!(has);
}

fn check_observers() {
    let mut world = World::new();
    world.init_resource::<Score>();
    world.add_observer(on_unit_added);
    world.spawn(Unit);
    assert_eq!(world.resource::<Score>().0, 10, "On<Add, Unit> ran on spawn");

    let e = world.spawn(Health(10)).id();
    world.entity_mut(e).observe(|a: On<Attacked>, mut q: Query<&mut Health>| {
        q.get_mut(a.entity).unwrap().0 -= a.damage;
    });
    world.trigger(Attacked {
        entity: e,
        damage: 4,
    });
    assert_eq!(world.get::<Health>(e).unwrap().0, 6);
    let unit = world
        .query_filtered::<Entity, With<Unit>>()
        .single(&world)
        .unwrap();
    let has_name = world.get::<Name>(unit).is_some();
    assert!(has_name, "required Name component inserted");
    println!("observers + required components OK");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_timesteps_run_once_per_update() {
        check_fixed_timesteps();
    }

    #[test]
    fn states_and_sim() {
        check_states_and_sim_plugin();
    }

    #[test]
    fn messages_and_run_system_once() {
        check_messages_from_test();
    }

    #[test]
    fn all_headless() {
        run_headless_checks();
    }
}
