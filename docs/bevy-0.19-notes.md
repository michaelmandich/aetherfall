# Bevy 0.19.1 API cheat sheet (compile-verified)

Every code snippet below is copied from `crates/zz_api_probe/src/main.rs`, except section 15,
which quotes Bevy's own source. Snippets are lightly trimmed (`/* .. */`). The probe compiles
against bevy 0.19.1 with zero errors and zero warnings (`cargo check -p zz_api_probe`), and
`cargo test -p zz_api_probe` passes.
Runtime claims marked **[verified at runtime]** come from
`cargo run -p zz_api_probe -- verify` (headless checks, all asserting) and from one
windowed run that rendered the scene, saved a screenshot and exited.

If your memory of Bevy is from 0.14–0.16, assume it is wrong until checked against this
page. The full rename list is at the end.

**Source references.** `$REG` = `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f`.
Paths like `bevy_ecs-0.19.1/src/...` are under `$REG`. Examples are under
`$REG/bevy-0.19.1/examples/`, and the 0.18→0.19 migration guides are under
`$REG/bevy-0.19.1/_release-content/migration-guides/`.

---

## 0. Imports that are NOT in `bevy::prelude`

The prelude covers most of the API. The things you will still need to import:

```rust
use bevy::asset::RenderAssetUsages;
use bevy::camera::ScalingMode;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::error::{BevyError, Severity};          // also bevy::ecs::error::{warn, panic, ignore, ...}
use bevy::ecs::system::RunSystemOnce;                  // world.run_system_once(..)
use bevy::image::ImageSampler;
use bevy::input::common_conditions::input_just_pressed;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::light::NotShadowCaster;
use bevy::log::{Level, LogPlugin};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use bevy::state::app::StatesPlugin;
use bevy::time::common_conditions::on_timer;
use bevy::time::TimeUpdateStrategy;
use bevy::window::{CursorOptions, PresentMode, PrimaryWindow, WindowResolution};
use bevy::camera::Hdr;
use bevy::platform::collections::{HashMap, HashSet};  // NOT bevy::utils::HashMap
use bevy::render::batching::NoAutomaticBatching;
```

The prelude does include `Pickable`, `Pointer<Click>`, `MeshPickingPlugin`, `GlobalAmbientLight`,
`light_consts`, `AlphaMode`, `Msaa`, `Projection`, `TextSpan`, `FontSize`, `px`/`percent`,
`Add`/`Remove`/`Insert`/`Discard`/`Despawn`, `On`, `Message*`, `Single`, `Populated`,
`TransformPlugin`, `AssetPlugin`, and `ops`.

---

## 1. App, Plugin, PluginGroup, schedules, sets, run conditions

```rust
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
                (SimSet::Input, SimSet::Orders, SimSet::Movement, SimSet::Combat, SimSet::Cleanup)
                    .chain()
                    .run_if(in_state(GameState::InGame)),
            )
            .add_systems(Startup, spawn_sim_entities)
            .add_systems(Update, finish_loading.run_if(in_state(GameState::Loading)))
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
            .add_systems(Update, log_score.run_if(on_timer(Duration::from_secs(5))))
            .add_systems(PostUpdate, changed_health_report)
            .add_observer(on_unit_added)
            // Observers accept run conditions too (0.19).
            .add_observer(on_any_attacked.run_if(in_state(GameState::InGame)));
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SimSet { Input, Orders, Movement, Combat, Cleanup }

// Ordering: .chain(), .before(), .after(), .in_set(), and a set ordered vs a system.
app.configure_sets(Update, UiInputSet.before(issue_orders))
    .add_systems(Update, (
        (read_input, issue_orders, after_orders).chain(),
        ui_hotkeys.in_set(UiInputSet),
        late_orders.after(issue_orders).before(after_orders),
    ));

// Other run conditions used in the probe:
update_fog.run_if(resource_exists::<FogImage>)
toggle_pause.run_if(input_just_pressed(KeyCode::Escape))
ground_click.run_if(pointer_not_over_ui)     // any fn(..) -> bool system works as a condition

// Your own PluginGroup:
struct ProbePlugins;
impl PluginGroup for ProbePlugins {
    fn build(self) -> bevy::app::PluginGroupBuilder {
        bevy::app::PluginGroupBuilder::start::<Self>().add(SimPlugin).add(ProbeRenderPlugin)
    }
}
```

* Schedules in the prelude: `PreStartup, Startup, PostStartup, First, PreUpdate, StateTransition`,
  `RunFixedMainLoop, FixedFirst, FixedPreUpdate, FixedUpdate, FixedPostUpdate, FixedLast`,
  `Update, PostUpdate, Last` (`bevy_app-0.19.1/src/lib.rs` prelude).
* Source: `bevy_app-0.19.1/src/app.rs` (`add_systems` :321, `configure_sets` :402,
  `add_message` :427, `add_observer` :1474, `set_error_handler` :1508).
  Conditions: `bevy_ecs-0.19.1/src/schedule/condition.rs` (`resource_exists` :730),
  `bevy_time-0.19.1/src/common_conditions.rs:36` (`on_timer`), `bevy_state-0.19.1/src/condition.rs:103`
  (`in_state`).
* **Gotcha:** `on_timer` lives in `bevy::time::common_conditions` and `input_just_pressed` lives in
  `bevy::input::common_conditions`. Neither is in the prelude. `in_state`, `resource_exists`,
  `run_once`, `not`, `on_message`, and `any_with_component` are in the prelude.

---

## 2. States

```rust
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
enum GameState { #[default] Loading, InGame, Paused }

app.init_state::<GameState>();                         // needs StatesPlugin (in DefaultPlugins)
app.add_systems(OnEnter(GameState::InGame), on_enter_ingame);
fn finish_loading(mut next: ResMut<NextState<GameState>>) { next.set(GameState::InGame); }
fn toggle_pause(state: Res<State<GameState>>, mut next: ResMut<NextState<GameState>>, ..) {
    match state.get() { GameState::InGame => next.set(GameState::Paused), .. }
}

// Headless: MinimalPlugins does NOT include StatesPlugin.
app.add_plugins((MinimalPlugins, StatesPlugin, TransformPlugin));
```

* **[verified at runtime]** With `MinimalPlugins + StatesPlugin`, `next.set()` in `Update` of
  frame 1 makes `State` equal `InGame` after frame 2. The transition runs in the
  `StateTransition` schedule, which runs after `PreUpdate`.
* Source: `bevy_state-0.19.1/src/app.rs:330` (`StatesPlugin`), `:96` (`init_state` warns
  "StatesPlugin is not installed" and then panics on the missing `StateTransition` schedule);
  `bevy_state-0.19.1/src/state/resources.rs:181-215` (`NextState`).
* **Gotcha (compile error found by the probe):** `next.set_if_neq(GameState::InGame)` on a
  `ResMut<NextState<_>>` resolves to the change-detection trait method
  `DetectChangesMut::set_if_neq` (which expects a whole `NextState`) and **does not compile**.
  Deref explicitly:
  ```rust
  // `next.set_if_neq(..)` would hit DetectChangesMut::set_if_neq on ResMut and not compile;
  // deref explicitly to reach NextState::set_if_neq (no transition if already InGame).
  (*next).set_if_neq(GameState::InGame);
  next.reset();
  ```
* **Gotcha:** `NextState` is an enum (`Unchanged | Pending(S) | PendingIfNeq(S)`). Use
  `.set(s)`, `(*next).set_if_neq(s)` or `.reset()`; do not assign `NextState(Some(..))`. In 0.19, `set()` to the
  *same* state re-runs `OnExit`/`OnEnter` and triggers `DespawnOnExit`/`DespawnOnEnter`. Use
  `set_if_neq` to avoid that (migration guide `despawn_on_same_state_transitions.md`).
  `StateScoped` is now `DespawnOnExit`.

---

## 3. Buffered messages vs observer events

Since 0.17 the old `Event` has been split in two:

| Want | 0.19 API |
|---|---|
| Buffered, pull-based queue read by systems | `#[derive(Message)]`, `app.add_message::<M>()`, `MessageWriter<M>::write`, `MessageReader<M>::read`, `MessageMutator<M>` |
| Push-based, run observers immediately | `#[derive(Event)]` + `commands.trigger(e)` + `app.add_observer(fn(On<E>, ..))` |
| Push-based, targets one entity | `#[derive(EntityEvent)]` (field `entity: Entity` or `#[event_target]`), plus `.observe(..)` on the entity |
| Component lifecycle | `On<Add, T>`, `On<Insert, T>`, `On<Discard, T>`, `On<Remove, T>`, `On<Despawn, T>` |

```rust
#[derive(Message, Debug, Clone)]
struct DamageMsg { target: Entity, amount: i32 }

fn deal_damage(mut writer: MessageWriter<DamageMsg>, q: Query<(Entity, &Team), With<Unit>>) {
    for (entity, team) in &q {
        if team.0 == 1 { writer.write(DamageMsg { target: entity, amount: 1 }); }
    }
}
fn apply_damage(mut reader: MessageReader<DamageMsg>, mut q: Query<&mut Health>, mut commands: Commands) {
    for msg in reader.read() {
        let Ok(mut hp) = q.get_mut(msg.target) else { continue };
        hp.0 -= msg.amount;
        if hp.0 <= 0 { commands.entity(msg.target).try_despawn(); }
    }
}

#[derive(Event, Debug, Clone)]
struct GameOver { winner: u8 }
commands.trigger(GameOver { winner: 0 });
fn on_game_over(game_over: On<GameOver>, mut next: ResMut<NextState<GameState>>) {
    info!("winner: {}", game_over.winner);      // On<E> derefs to E
    next.set(GameState::Paused);
}

#[derive(EntityEvent, Debug, Clone)]
struct Attacked { entity: Entity, damage: i32 }   // `entity` = event target
commands.spawn((Name::new("Barracks"), /*..*/))
    .observe(|attacked: On<Attacked>, mut q: Query<&mut Health>| {   // entity-scoped observer
        if let Ok(mut hp) = q.get_mut(attacked.entity) { hp.0 -= attacked.damage; }
    });
commands.trigger(Attacked { entity: barracks, damage: 5 });           // runs entity + global observers
commands.entity(barracks).trigger(|entity| Attacked { entity, damage: 1 });

fn on_unit_added(add: On<Add, Unit>, names: Query<&Name>, mut score: ResMut<Score>) {
    if let Ok(name) = names.get(add.entity) { debug!("unit added: {name}"); }
    score.0 += 10;
}
fn on_unit_removed(remove: On<Remove, Unit>, teams: Query<&Team>) {
    // `Remove` runs before the component is gone, so its data is still readable.
    if let Ok(team) = teams.get(remove.entity) { debug!("unit of team {} removed", team.0); }
}

// Other lifecycle observers:
.add_observer(|inserted: On<Insert, Health>| { trace!("Health (re)inserted on {}", inserted.entity); })
.add_observer(|discarded: On<Discard, Health>| { trace!("Health about to be replaced/removed on {}", discarded.entity); })
.add_observer(|despawned: On<Despawn, Unit>| { trace!("unit {} despawning", despawned.entity); })

/// Propagating entity event: bubbles up the ChildOf hierarchy until `propagate(false)`.
#[derive(EntityEvent, Debug, Clone)]
#[entity_event(propagate)]
struct Hit { entity: Entity }
.add_observer(|mut hit: On<Hit>| { hit.propagate(false); /* stop bubbling to the parent */ })
commands.entity(e).trigger(|entity| Hit { entity });

// Read + mutate the same message type in one system (Reader + Writer of one type conflict):
fn misc_api_probe(mut damage: MessageMutator<DamageMsg>, mut commands: Commands, /* .. */) {
    for msg in damage.read() { msg.amount += 1; }
    commands.write_message(DamageMsg { target: Entity::PLACEHOLDER, amount: 0 });
}
fn misc_writer(mut writer: MessageWriter<DamageMsg>) { writer.write_batch(Vec::new()); }

// From a test / exclusive context:
app.world_mut().write_message(DamageMsg { target, amount: 3 });
world.trigger(Attacked { entity: e, damage: 4 });
```

* **[verified at runtime]** `On<Add, Unit>` fires on `world.spawn(Unit)`. An entity observer
  attached with `world.entity_mut(e).observe(..)` runs on `world.trigger(Attacked{entity: e,..})`.
  A message written with `world.write_message` is read by a system on the next `app.update()`.
* Source: `examples/ecs/message.rs`, `examples/ecs/observers.rs`, `examples/ecs/observer_propagation.rs`;
  `bevy_ecs-0.19.1/src/lifecycle.rs:337-395` (`Add`, `Insert`, `Discard`, `Remove`, `Despawn`, each with
  a pub `entity` field); `bevy_ecs-0.19.1/src/event/mod.rs:100-200` (EntityEvent derive rules);
  `bevy_ecs-0.19.1/src/system/commands/mod.rs:1169` (`Commands::trigger`), `:2071` (`observe`),
  `:1217` (`Commands::write_message`).
* **Gotchas:**
  * `Trigger<E>` became `On<E>`, and `trigger.target()` became `event.entity`.
  * `OnAdd`/`OnRemove` became `Add`/`Remove`.
  * `Replace` was renamed `Discard` in 0.19, and `#[component(on_replace=..)]` became `on_discard`.
  * Propagation: `#[derive(EntityEvent)] #[entity_event(propagate)]`, and stop it with `event.propagate(false)`.
  * Messages are double-buffered and survive one frame boundary. With `TimePlugin`, the buffer swap
    waits until a fixed tick has run (`bevy_time-0.19.1/src/lib.rs:95-98`), so `FixedUpdate` readers
    don't miss messages written in `Update`.
  * Input events (`MouseWheel`, `KeyboardInput`, `CursorMoved`) and `AppExit` are **Messages**, so
    read them with `MessageReader`.

---

## 4. Components, resources, required components, commands, hierarchy

```rust
#[derive(Component, Debug, Clone, Copy, PartialEq)]
struct Health(i32);

/// Spawning `Unit` alone also inserts Transform, Visibility, Health(100),
/// Velocity::default() and a Name (unless the bundle already has them).
#[derive(Component, Default)]
#[require(Transform, Visibility, Health(100), Velocity, Name = Name::new("Unit"))]
struct Unit;

#[derive(Resource, Default, Debug)]
struct Score(u32);

fn check_victory(q: Query<&Team, With<Unit>>, mut commands: Commands, mut done: Local<bool>) { .. }

// Tuples are bundles.
commands.spawn((Unit, Team(0), Transform::from_xyz(-4.0, 0.0, 0.0), Selected));

// Hierarchy
let barracks = commands.spawn((
        Name::new("Barracks"), Team(0), Transform::from_xyz(0.0, 0.0, -6.0), Visibility::default(),
        children![
            (Name::new("Flag"), Transform::from_xyz(0.0, 3.0, 0.0)),
            (Name::new("Door"), Transform::from_xyz(0.0, 0.0, 1.0)),
        ],
    ))
    .insert(Health(500))
    .id();
commands.entity(barracks).with_children(|parent| {
    parent.spawn((Name::new("Window"), Transform::from_xyz(1.0, 1.0, 1.0)));
});
commands.spawn((Name::new("Chimney"), Transform::default(), ChildOf(barracks)));
commands.entity(barracks).add_child(banner);
let _parent: Entity = child_of.parent();               // ChildOf(pub Entity)
for child in children.iter() { let _: Entity = child; } // &Children iterates Entity

commands.entity(e).insert(Health(1));     // PANICS (default handler) if e is dead
commands.entity(e).try_insert(Health(1)); // silently ignored if e is dead
commands.entity(e).remove::<Selected>();
commands.entity(e).despawn();             // recursive over Children; warns if already dead
commands.entity(e).try_despawn();         // recursive, silent if already dead
commands.entity(e).despawn_children();    // was despawn_descendants
commands.entity(e).despawn_related::<Children>();
commands.spawn((Name::new("tmp"), DespawnOnExit(GameState::InGame), NoAutomaticBatching)); // was StateScoped
commands.delayed().secs(1.0).spawn(Name::new("spawned one second later"));            // new in 0.19
let mut map: HashMap<Entity, u32> = HashMap::default();   // bevy::platform::collections
```

* **[verified at runtime]**
  * `despawn()` on a parent also despawns its `children![..]`.
  * `try_insert`/`try_despawn` on a dead entity do nothing.
  * `insert()` on a dead entity **panics** under the default error handler, and only logs once
    `app.set_error_handler(bevy::ecs::error::warn)` is set.
* Source: `bevy_ecs-0.19.1/src/system/commands/mod.rs:1435` (`insert`), `:1626` (`try_insert`),
  `:1906` (`despawn`, whose doc says "recursively despawn Children"), `:1921` (`try_despawn`);
  `bevy_ecs-0.19.1/src/hierarchy.rs:107` (`ChildOf`), `:152` (`Children`), `:519` (`children!`);
  `bevy_ecs-0.19.1/src/component/mod.rs:105-280` (`#[require]` syntax).
* **Gotchas:**
  * `despawn_recursive()` is gone because `despawn()` is now recursive. `despawn_descendants()`
    is now `despawn_children()` / `despawn_related::<Children>()`.
  * `Parent` became `ChildOf`, and `set_parent(p)` became `insert(ChildOf(p))`.
  * In 0.19 `#[derive(Resource)]` also implements `Component`, so you can no longer derive both on
    one type. Broad queries such as `Query<Entity>` or `Query<EntityMut>` now also match
    resource entities; filter them with `Without<IsResource>` if that matters (migration guide
    `resources_as_components.md`).

---

## 5. Queries, Single, Populated, ParamSet, fallible systems

```rust
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
}

fn sorted_and_many(teams: Query<(Entity, &Team)>, selected: Query<Entity, With<Selected>>, names: Query<&Name>) {
    // QueryIter::sort / sort_by take the sort-key lens type as a turbofish.
    let by_team: Vec<(Entity, &Team)> = teams.iter().sort::<&Team>().collect();
    let by_team_desc: Vec<(Entity, &Team)> = teams.iter().sort_by::<&Team>(|a, b| b.cmp(a)).collect();
    // iter_many: fetch query items for an iterator of entities (skips non-matching).
    for name in names.iter_many(selected.iter()) { let _ = name; }
}

/// Systems may return `Result`; `?` converts any std::error::Error into BevyError.
fn fallible_system(units: Query<&Health, With<Unit>>, all: Query<Entity, With<Unit>>) -> Result {
    let Some(first) = all.iter().next() else { return Ok(()); };
    let hp = units.get(first)?;                       // QueryEntityError -> BevyError
    if hp.0 < -1000 {
        // Explicit severity: logged as a warning by the default handler instead of panicking.
        return Err(BevyError::warning("absurd health"));
    }
    let _maybe = all.single().with_severity(Severity::Debug); // single() returns Result
    Ok(())
}

fn single_and_populated(
    selected: Single<(Entity, &Transform), With<Selected>>,  // skip system unless exactly 1
    maybe_cam: Option<Single<&Camera>>,                       // None if 0; still skips if >1
    units: Populated<&Unit>,                                  // skip system unless >= 1
) {
    let (entity, transform) = *selected;
}
let (mut transform, mut projection) = camera.into_inner();   // Single<(&mut A, &mut B)>

fn paramset_showcase(mut set: ParamSet<(Query<&mut Transform, With<Unit>>, Query<&Transform, With<Name>>)>) {
    for mut t in set.p0().iter_mut() { t.translation.y = 0.0; }
    let count = set.p1().iter().count();
}
```

* `query.get(e)` / `get_mut(e)` return `Result<_, QueryEntityError>`. `single()` /
  `single_mut()` return `Result<_, QuerySingleError>`. `contains(e) -> bool`.
* **[verified at runtime]** A system with `Single<&Selected>` and 0 matches is skipped silently, and runs
  once a match exists. A system with a missing `Res<T>` **panics** under the default handler.
* **Error handling default:** an unhandled error goes to `FallbackErrorHandler`, whose default is
  `match_severity`. That handler panics for any error with no explicit severity: "errors without an
  assigned severity use `Severity::Panic`" (`bevy_ecs-0.19.1/src/error/bevy_error.rs:20`,
  `error/handler.rs:115-140`). Change it once per app, before plugins:
  `app.set_error_handler(bevy::ecs::error::warn);`.
* Source: `bevy_ecs-0.19.1/src/system/query.rs` (`get` :1577, `get_mut` :1713, `single` :2097,
  `single_mut` :2126, `contains` :2242, `iter_many` :891; `Single` :2850, `Populated` :2892);
  `bevy_ecs-0.19.1/src/query/iter.rs:576` (`sort`), `:719` (`sort_by`);
  `bevy_ecs-0.19.1/src/system/system_param.rs:377-380` (Single uses "skipped" validation errors),
  `:712` (Res uses "invalid"); `examples/ecs/fallible_params.rs`, `examples/ecs/error_handling.rs`.
* **Gotchas:**
  * `get_single()`/`get_single_mut()` are gone. `single()` is the fallible one.
  * `sort_by` needs a turbofish naming the lens, e.g. `sort_by::<&Team>`.
  * Error logs show `<Enable the debug feature to see the name>` instead of system names unless
    bevy's `debug` cargo feature is on.

---

## 6. Time, fixed timestep, virtual time, headless fast-forward

```rust
app.insert_resource(Time::<Fixed>::from_hz(20.0));   // default is 64 Hz (15.625 ms)

fn movement(time: Res<Time>, mut q: Query<(&mut Transform, &Velocity), With<Unit>>) {
    // Inside FixedUpdate, `Res<Time>` is the fixed clock: delta_secs() == timestep.
    let dt = time.delta_secs();
    for (mut transform, velocity) in &mut q { transform.translation += velocity.0 * dt; }
}

let fixed = app.world().resource::<Time<Fixed>>();
fixed.timestep(); fixed.elapsed(); fixed.overstep_fraction();   // f32 in [0,1) for interpolation

virtual_time.pause(); virtual_time.unpause(); virtual_time.set_relative_speed(2.0);
// camera code that must keep working while paused: Res<Time<Real>>

// Headless deterministic stepping: each app.update() == exactly one fixed tick.
fn fixed_counting_app(strategy: TimeUpdateStrategy, hz: f64) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(Time::<Fixed>::from_hz(hz))
        .insert_resource(strategy)
        .init_resource::<FixedCount>()
        .add_systems(FixedUpdate, count_fixed_ticks);
    app
}
let mut app = fixed_counting_app(TimeUpdateStrategy::FixedTimesteps(1), 20.0);
// Fast-forward N ticks per update: raise the virtual clamp first.
app.world_mut().resource_mut::<Time<Virtual>>().set_max_delta(Duration::from_secs(10));
```

**[verified at runtime]** (`check_fixed_timesteps`, `check_manual_duration_and_max_delta`, `check_virtual_speed_and_pause`):

| Setup (20 Hz fixed) | Fixed runs per `app.update()` |
|---|---|
| `TimeUpdateStrategy::FixedTimesteps(1)` | `[0, 1, 1, 1, 1, 1]`. The **first update runs 0**, because it only primes the clock |
| `TimeUpdateStrategy::ManualDuration(timestep)` | `[0, 1, 1, 1]`. `Res<Time>.delta()` in `Update` is then exactly 50 ms |
| `FixedTimesteps(10)` (500 ms) with the default `max_delta` | **5** (clamped to `Time<Virtual>` max_delta = 250 ms) |
| `FixedTimesteps(10)` + `set_max_delta(10 s)` | 10 |
| `FixedTimesteps(1)` + `set_relative_speed(2.0)` | 2 |
| `FixedTimesteps(1)` + `pause()` | 0 |

* Source: `bevy_time-0.19.1/src/lib.rs:107-123` (`TimeUpdateStrategy`: `Automatic`, `ManualInstant`,
  `ManualDuration`, `FixedTimesteps(u32)`), `:181` (FixedTimesteps = timestep × n into `Time<Real>`);
  `real.rs:99-110` (the first update only records the instant, so delta is 0);
  `virt.rs:86` (`DEFAULT_MAX_DELTA` = 250 ms); `fixed.rs:76` (64 Hz default), `:105` (`from_hz`),
  `:205` (`overstep_fraction`), `:243` (`run_fixed_main_schedule`).
* **Gotchas:**
  * `delta_seconds()`/`elapsed_seconds()` were renamed `delta_secs()`/`elapsed_secs()`.
  * Real time flows into virtual time (clamped, scaled), and virtual time feeds the fixed accumulator.
    So pausing virtual time stops `FixedUpdate`, and the virtual `max_delta` caps catch-up ticks.
  * New in 0.19: `commands.delayed().secs(1.0).spawn(..)` (`DelayedCommandsExt` in the prelude).

---

## 7. Headless apps and tests

```rust
fn sim_app() -> App {
    let mut app = App::new();
    // MinimalPlugins has no TransformPlugin, StatesPlugin, AssetPlugin or InputPlugin.
    app.add_plugins((MinimalPlugins, StatesPlugin, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(1))
        .add_plugins(SimPlugin);
    app
}

// Assets<T> headless:
app.add_plugins((MinimalPlugins, AssetPlugin::default()))
    .init_asset::<Mesh>()
    .init_asset::<Image>();
let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(Cuboid::new(1.0, 1.0, 1.0));

// Test helpers
let target = app.world_mut().spawn(Health(10)).id();
app.world_mut().write_message(DamageMsg { target, amount: 3 });
app.update();
let hp = app.world().get::<Health>(target).unwrap().0;
let state = *app.world().resource::<State<GameState>>().get();
// run_system_once needs `use bevy::ecs::system::RunSystemOnce;` and returns Result.
let count = app.world_mut().run_system_once(|q: Query<&Health>| q.iter().count()).expect("system ran");
let total: i32 = app.world_mut().run_system_once(|q: Query<&Health>| q.iter().map(|h| h.0).sum::<i32>()).unwrap();
// Bare World works for ECS-only tests:
let mut world = World::new();
let parent = world.spawn((Name::new("parent"), children![Name::new("child")])).id();
world.entity_mut(parent).despawn();
let unit = world.query_filtered::<Entity, With<Unit>>().single(&world).unwrap();
```

* `MinimalPlugins` = `TaskPoolPlugin, FrameCountPlugin, TimePlugin, ScheduleRunnerPlugin`
  (`bevy_internal-0.19.1/src/default_plugins.rs`).
  * Transform propagation needs `TransformPlugin` **[verified: child GlobalTransform = (11,0,0)]**.
  * States need `StatesPlugin`.
  * `Assets<T>` needs `AssetPlugin` + `init_asset::<T>()` **[verified]**.
  * `ButtonInput<KeyCode>` needs `InputPlugin`.
  * Calling `app.update()` yourself ignores the `ScheduleRunnerPlugin` loop.
* `cargo test -p zz_api_probe` runs the same checks as `#[test]`s.
* **Gotcha:** `world.run_system_once` returns `Result<Out, RunSystemError>`, and the trait must be
  imported. `World::send_event` is now `World::write_message`.

---

## 8. 3D rendering: meshes, materials, lights, colors, transforms

```rust
commands.spawn((
    Name::new("Ground"),
    Mesh3d(meshes.add(Plane3d::default().mesh().size(64.0, 64.0))),
    MeshMaterial3d(materials.add(Color::srgb(0.25, 0.4, 0.2))),   // Color: Into<StandardMaterial>
    Transform::default(),
));
// Shared handles => automatic batching/instancing of identical mesh+material.
let cube = meshes.add(Cuboid::new(1.0, 2.0, 1.0));             // primitives are Into<Mesh>
let _also_a_mesh: Mesh = Mesh::from(Cuboid::new(1.0, 1.0, 1.0));
let red = materials.add(StandardMaterial {
    base_color: Color::srgb(0.8, 0.2, 0.2),
    emissive: LinearRgba::rgb(0.2, 0.0, 0.0),                  // emissive is LinearRgba, not Color
    perceptual_roughness: 0.8,
    ..default()
});
let glass = materials.add(StandardMaterial {
    base_color: Color::srgba(0.3, 0.5, 1.0, 0.4),
    alpha_mode: AlphaMode::Blend,
    unlit: true,
    ..default()
});
commands.spawn((
    Unit, Team(1), Mesh3d(cube.clone()), MeshMaterial3d(red.clone()),
    Transform::from_xyz(i as f32 * 2.0 - 4.0, 1.0, 6.0)
        .with_scale(Vec3::splat(0.8))
        .with_rotation(Quat::from_rotation_y(0.3 * i as f32)),
));
commands.spawn((Mesh3d(meshes.add(Sphere::new(1.0))), MeshMaterial3d(glass),
    Transform::from_xyz(0.0, 1.0, 0.0), NotShadowCaster,
    Visibility::Visible, /* Inherited (default) | Hidden | Visible */));

// Directional light with shadows (field is `shadow_maps_enabled`, not `shadows_enabled`).
commands.spawn((
    DirectionalLight {
        illuminance: light_consts::lux::OVERCAST_DAY * 5.0,
        shadow_maps_enabled: true,
        ..default()
    },
    Transform::from_xyz(30.0, 50.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
));
// Ambient light: resource GlobalAmbientLight (per-camera override: AmbientLight component).
app.insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 400.0, ..default() });

// Color helpers (traits Mix / Luminance / Alpha are in the prelude).
let base = Color::srgb(0.5, 0.4, 0.3);
let _mixed = base.mix(&Color::WHITE, 0.25);
let _light = base.lighter(0.1);
let _dark = base.darker(0.1);
let _faded = base.with_alpha(0.5);
let _lin: LinearRgba = base.to_linear();
let _hex = Color::srgb_u8(200, 180, 40);
```

* `StandardMaterial` fields: `base_color: Color`, `base_color_texture: Option<Handle<Image>>`,
  `emissive: LinearRgba`, `perceptual_roughness`, `metallic`, `unlit`, `alpha_mode: AlphaMode`,
  `double_sided`, `cull_mode`, `fog_enabled`, `depth_bias`, `uv_transform`
  (`bevy_pbr-0.19.1/src/pbr_material.rs:34-783`). `AlphaMode` is at `bevy_material-0.19.1/src/alpha.rs:9`.
* Lights: `bevy_light-0.19.1/src/directional_light.rs:73-128` (`illuminance` default
  `AMBIENT_DAYLIGHT` = 10 000 lux, `shadow_maps_enabled`, `contact_shadows_enabled`,
  `shadow_depth_bias`, `shadow_normal_bias`); `ambient_light.rs:12` (`AmbientLight` is a
  **component** that `#[require(Camera)]`) and `:62` (`GlobalAmbientLight` **resource**, brightness 80);
  `lib.rs:264` (`NotShadowCaster`, not in the prelude).
* Batching: entities that share the same `Mesh3d` handle and `MeshMaterial3d` handle are batched
  automatically (with GPU preprocessing on desktop: log line "GPU preprocessing is fully
  supported"). Opt out per entity with `bevy::render::batching::NoAutomaticBatching`
  (`bevy_render-0.19.1/src/batching/mod.rs:23`). Make one handle and clone it, rather than calling
  `materials.add` per unit.
* **Gotchas:**
  * Bundles (`PbrBundle`, ...) are gone. Spawn `Mesh3d(h)` + `MeshMaterial3d(h)` + `Transform`.
  * `shadows_enabled` was renamed `shadow_maps_enabled` (0.19, migration guide `contact_shadows.md`).
  * The `AmbientLight` resource is now `GlobalAmbientLight`.
  * `Color::rgb` is now `Color::srgb`.
  * `Assets::get_mut` returns `Option<AssetMut<A>>`, so bind it `let Some(mut m) = ..`.

---

## 9. Camera, projection, picking rays

```rust
commands.spawn((
    Camera3d::default(),                       // requires Camera + Projection (perspective)
    Projection::from(PerspectiveProjection { fov: 45f32.to_radians(), near: 0.1, far: 1000.0, ..default() }),
    Transform::from_xyz(0.0, 30.0, 22.0).looking_at(Vec3::ZERO, Vec3::Y),
    Msaa::Sample4,               // the default
    Tonemapping::TonyMcMapface,  // the default
));
let _ortho = Projection::from(OrthographicProjection {
    scaling_mode: ScalingMode::FixedVertical { viewport_height: 40.0 },
    scale: 1.0,
    ..OrthographicProjection::default_3d()    // NOT ::default() for 3D
});
app.insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.12)));

fn ground_click(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    /* .. */
) {
    if !mouse.just_pressed(MouseButton::Right) { return; }
    let Some(cursor) = window.cursor_position() else { return; };  // cursor outside the window
    let (camera, camera_gt) = *camera;
    // viewport_to_world returns Result<Ray3d, ViewportConversionError>.
    let Ok(ray) = camera.viewport_to_world(camera_gt, cursor) else { return; };
    let Some(distance) = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)) else { return; };
    let target = ray.get_point(distance);
    let _same = ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y));
}
// Result<Vec2, ViewportConversionError>, logical pixels, origin top-left.
if let Ok(screen) = camera.world_to_viewport(camera_gt, gt.translation()) { .. }
// Zoom:
match &mut *projection {
    Projection::Perspective(p) => p.fov = (p.fov - notches * 0.05).clamp(0.3, 1.2),
    Projection::Orthographic(o) => o.scale = (o.scale - notches * 0.1).max(0.1),
    _ => {}
}
```

* Source: `bevy_camera-0.19.1/src/camera.rs:579` (`world_to_viewport`), `:647`
  (`viewport_to_world`), both returning `Result`; `components.rs:24` (`Camera3d` requires
  `Camera, Projection`); `projection.rs:216` (`enum Projection {Perspective, Orthographic, Custom}`),
  `:283` (`PerspectiveProjection{fov, aspect_ratio, near, far, ..}`), `:523` (`ScalingMode`:
  `WindowSize | Fixed | AutoMin | AutoMax | FixedVertical{viewport_height}`), `:580`
  (`OrthographicProjection{near, far, viewport_origin, scaling_mode, scale, area}`), `:782`
  (`default_3d`); `bevy_math-0.19.1/src/ray.rs:97` (`get_point`), `:105` (`intersect_plane ->
  Option<f32>`, which only hits in front of the ray origin).
* Defaults that matter: `Msaa::Sample4` is a **per-camera component**
  (`bevy_render-0.19.1/src/view/mod.rs:240`). `Tonemapping::TonyMcMapface`
  (`bevy_core_pipeline-0.19.1/src/tonemapping/mod.rs:157`). HDR is the `bevy::camera::Hdr` marker
  component (moved to bevy_camera in 0.19); `Camera { hdr: true }` no longer exists:
  `commands.entity(camera).insert(Hdr);`. UI on a specific camera:
  `commands.spawn((Node::default(), UiTargetCamera(camera)));`.
* **Gotchas:**
  * `viewport_to_world` returns `Result`, not `Option`, so use `.ok()` / `let Ok(..)`.
  * `PerspectiveProjection` is not a component. Wrap it as `Projection::from(..)` or
    `Projection::Perspective(..)`.
  * `Camera3dBundle` is gone.

---

## 10. Input and window

```rust
fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    motion: Res<AccumulatedMouseMotion>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>, // real time so the camera moves while the sim is paused
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let (mut transform, mut projection) = camera.into_inner();
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) { /* .. */ }
    let speed = if keys.pressed(KeyCode::ShiftLeft) { 40.0 } else { 20.0 };
    if mouse.pressed(MouseButton::Middle) {
        transform.translation.x -= motion.delta.x * 0.05;
    }
    // Wheel: Line units (1.0 per notch) on most mice, Pixel units on touchpads.
    let notches = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
    };
}
keys.just_pressed(KeyCode::Digit1) || keys.pressed(KeyCode::ControlLeft) || keys.just_released(KeyCode::Space)
    || keys.just_pressed(KeyCode::F1) || keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Tab);
// also KeyCode::Escape, KeyCode::Equal, KeyCode::Minus, KeyCode::Backquote, KeyCode::Enter

// Window
fn selection_box(window: Single<&Window, With<PrimaryWindow>>, /* .. */) {
    let cursor: Option<Vec2> = window.cursor_position();   // logical px, origin top-left
}
window.width(); window.height(); window.size(); window.scale_factor(); window.physical_width();

App::new().add_plugins(
    DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "zz_api_probe".into(),
                resolution: WindowResolution::new(1280, 720),
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        })
        .set(LogPlugin { level: Level::INFO, filter: "wgpu=error,naga=warn".into(), ..default() })
        // `.set()` returned a PluginGroupBuilder, so `.disable()` is available here.
        // On a bare group use `DefaultPlugins.build().disable::<X>()`.
        .disable::<bevy::audio::AudioPlugin>(),
);
```

* Source: `bevy_input-0.19.1/src/button_input.rs:149-272` (`pressed`, `just_pressed`,
  `just_released`, `any_pressed`, `get_just_pressed`, ...); `keyboard.rs` (`KeyCode::KeyA`, `Digit1`,
  `ShiftLeft`, `ControlLeft`, `Escape`, `Space`, `F1`, `Delete`, `Tab`); `mouse.rs:218`
  (`AccumulatedMouseMotion{delta}`), `:239` (`AccumulatedMouseScroll{unit, delta}`), `:129`
  (`MouseScrollUnit`), `:151` (`SCROLL_UNIT_CONVERSION_FACTOR = 100.`);
  `bevy_window-0.19.1/src/window.rs:562` (`width`), `:620` (`cursor_position`), `:923`
  (`WindowResolution::new(u32, u32)`), `:1044` (`From<(u32,u32)>`); `bevy_window/src/lib.rs:64`
  (`WindowPlugin{primary_window, primary_cursor_options, exit_condition, close_when_requested}`);
  `bevy_app-0.19.1/src/plugin_group.rs:211` (`PluginGroup::set`), `:501` (`disable`);
  `bevy_log-0.19.1/src/lib.rs:218` (`LogPlugin{filter, level, custom_layer, ..}`).
* **Gotchas:**
  * `PrimaryWindow` is **not** in the prelude (`bevy::window::PrimaryWindow`).
  * `WindowResolution::new` takes `u32`. At window creation bevy_winit treats the size as **logical**
    (`bevy_winit-0.19.1/src/winit_windows.rs:114`). **[verified]** `new(1280, 720)` produced a
    1600×900 physical window and screenshot on a 125% DPI display.
  * The scroll resource is reset every frame.
  * Cursor grab/visibility moved to a separate `CursorOptions` component
    (`WindowPlugin::primary_cursor_options`): `Query<&CursorOptions, With<PrimaryWindow>>` →
    `options.visible`, `options.grab_mode`.

---

## 11. UI

```rust
// Full-screen root. Pickable::IGNORE so this transparent layout node does not
// count as "UI under the cursor" (every Node is pickable and blocks by default).
commands.spawn((
    Node {
        width: percent(100), height: percent(100),
        flex_direction: FlexDirection::Column,
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Stretch,
        ..default()
    },
    Pickable::IGNORE,
    children![
        (   // Top resource bar.
            Node { width: percent(100), height: px(32), padding: UiRect::axes(px(8), px(4)),
                   column_gap: px(12), align_items: AlignItems::Center, ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            children![(
                Text::new("Gold: "),
                TextFont::from_font_size(18.0),
                TextColor(Color::WHITE),
                children![(                                   // multi-part text = TextSpan children
                    TextSpan::new("100"),
                    TextFont { font_size: FontSize::Px(18.0), ..default() },
                    TextColor(Color::srgb(1.0, 0.85, 0.2)),
                    GoldText,
                )],
            )],
        ),
        (   // Bottom command panel.
            Node { height: px(120), margin: UiRect::all(px(4)), padding: UiRect::all(px(6)),
                   border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(6)),
                   flex_direction: FlexDirection::Row, column_gap: px(6), row_gap: px(6), ..default() },
            BackgroundColor(Color::srgba(0.1, 0.08, 0.05, 0.9)),
            BorderColor::all(Color::srgb(0.6, 0.5, 0.3)),
            children![
                (Button, Node { width: px(64), height: px(64), justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center, ..default() },
                 BackgroundColor(Color::srgb(0.25, 0.25, 0.25)), children![Text::new("Build")]),
                (Node { display: Display::None, ..default() }, Text::new("hidden")),   // no layout space
                (Node::default(), Text::new("invisible but laid out"), Visibility::Hidden),
            ],
        ),
    ],
));

// Absolute positioning + z order
Node { position_type: PositionType::Absolute, right: px(10), bottom: px(140),
       width: px(160), height: px(160), ..default() }, GlobalZIndex(10)   // or ZIndex(1) (sibling-local)

fn button_feedback(mut q: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<Button>)>) {
    for (interaction, mut bg) in &mut q {
        bg.0 = match *interaction {
            Interaction::Pressed => Color::srgb(0.35, 0.75, 0.35),
            Interaction::Hovered => Color::srgb(0.35, 0.35, 0.35),
            Interaction::None => Color::srgb(0.25, 0.25, 0.25),
        };
    }
}
fn update_gold_text(score: Res<Score>, mut q: Query<&mut TextSpan, With<GoldText>>) {
    if !score.is_changed() { return; }
    for mut span in &mut q { **span = score.0.to_string(); }
}
// Click via picking observer instead of polling:
commands.spawn((Button, Node { /*..*/ }, children![Text::new("Quit")]))
    .observe(|_click: On<Pointer<Click>>, mut exit: MessageWriter<AppExit>| { exit.write(AppExit::Success); });
```

### Do not issue world commands when the cursor is over UI (recommended 0.19 way)

```rust
/// Run condition: true when the mouse pointer is NOT over any UI node.
fn pointer_not_over_ui(hover_map: Res<HoverMap>, ui_nodes: Query<(), With<Node>>) -> bool {
    !hover_map
        .get(&PointerId::Mouse)
        .is_some_and(|hits| hits.keys().any(|e| ui_nodes.contains(*e)))
}
app.add_systems(Update, ground_click.run_if(pointer_not_over_ui));

/// Cheaper alternative: only sees nodes that have an Interaction component (Buttons, or
/// nodes you add `Interaction` to).
fn pointer_over_interactive_ui(q: Query<&Interaction>) -> bool {
    q.iter().any(|i| *i != Interaction::None)
}
```

Why `HoverMap`: the UI picking backend (`UiPickingPlugin`, which `UiPlugin` adds when the
default `ui_picking` feature is on, `bevy_ui-0.19.1/src/lib.rs:178-179`, and which feeds
`DefaultPickingPlugins`) hit-tests **every** `Node`. By default a node blocks hits on whatever is under it,
and the `HoverMap` keeps only the topmost blocking hits for each pointer. So "any hovered entity
has `Node`" means the cursor is over UI. Mark full-screen layout containers, the drag box and
anything purely decorative with `Pickable::IGNORE`, or the whole screen counts as UI.
`FocusPolicy` is ignored by picking (`bevy_ui-0.19.1/src/picking_backend.rs:1-13`). `HoverMap` is
updated in `PreUpdate`, so it is current for `Update` systems.

* Source: `bevy_ui-0.19.1/src/ui_node.rs:499-818` (`Node` fields: `display, box_sizing, position_type,
  overflow, left/right/top/bottom, width/height, min_*/max_*, aspect_ratio, align_items,
  justify_items, align_self, justify_self, align_content, justify_content, direction, margin,
  padding, border, border_radius, flex_direction, flex_wrap, flex_grow, flex_shrink, flex_basis,
  row_gap, column_gap, grid_*`), `:2229` (`BackgroundColor(pub Color)`), `:2256` (`BorderColor{top,
  right, bottom, left}` + `BorderColor::all`), `:2526` (`BorderRadius` struct, **not a component**),
  `:2440/2450` (`ZIndex`, `GlobalZIndex`); `geometry.rs:541/558/563/568` (`px`, `percent`, `vw`,
  `vh`), `:690/807` (`UiRect::all`, `UiRect::axes`); `focus.rs` (`Interaction`, updated by
  `ui_focus_system`); `widget/button.rs:8` (`Button` requires `Node, FocusPolicy::Block,
  Interaction`); `widget/image.rs:74` (`ImageNode::new(handle)`); `bevy_picking-0.19.1/src/hover.rs:60`
  (`HoverMap`), `lib.rs:198-245` (`Pickable`, `Pickable::IGNORE`); `bevy_text-0.19.1/src/text.rs:376`
  (`TextFont{font: FontSource, font_size: FontSize, weight, ..}`), `:487` (`FontSize::Px|Vw|Vh|VMin|VMax|Rem`),
  `:193` (`TextSpan`); `examples/ui/widgets/button.rs`.
* **Default font:** `default_font` is part of the default features (`default` → `2d`/`3d`/`ui` →
  `default_platform` → `default_font`, `bevy-0.19.1/Cargo.toml:2742-2774`). It embeds
  `FiraMono-subset.ttf` as the default `Handle<Font>` (`bevy_text-0.19.1/src/lib.rs:81-82,141-146`).
  **[verified]** `Text::new(..)` renders with no font loaded. It is a subset, so ship a real
  font for anything beyond basic Latin.
* **Gotchas:**
  * `Style` is merged into `Node`, `NodeBundle`/`TextBundle`/`ButtonBundle` are gone, and `UiImage`
    is now `ImageNode`.
  * `TextFont.font_size` is `FontSize::Px(f32)` (0.19), though `TextFont::from_font_size(18.0)`
    accepts `f32`. `TextFont.font` is a `FontSource`, so pass `asset_server.load(..).into()` (0.19).
  * `Font::from_bytes(bytes)` takes **one** argument in 0.19.1 (`bevy_text-0.19.1/src/font.rs:38`).
    The migration guide `rename_font_from_bytes.md` shows a two-argument form; that guide is
    stale. Also `TextLayout::justify(Justify::Center)` and `TextLayout::no_wrap()` (the
    `new_with_` prefix is gone).
  * `Text::from_section` / `TextStyle` are gone. Use `Text` + `TextFont` + `TextColor` + `TextSpan` children.
  * `BorderColor(c)` is now `BorderColor::all(c)`.
  * The `BorderRadius` component is now the `Node::border_radius` field.
  * `px(8)`/`percent(100)` helpers exist (taking ints or floats), and `Val::Px(8.0)` still works.

---

## 12. Dynamic textures (fog of war, minimap)

```rust
let mut image = Image::new_fill(
    Extent3d { width: FOG_W, height: FOG_H, depth_or_array_layers: 1 },
    TextureDimension::D2,
    &[0, 0, 0, 200],
    TextureFormat::Rgba8UnormSrgb,
    // Keep MAIN_WORLD so the CPU copy stays mutable after upload.
    RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
);
image.sampler = ImageSampler::nearest();
let handle = images.add(image);
commands.spawn((                                   // fog overlay: unlit, alpha-blended, textured flat cuboid
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
commands.spawn((ImageNode::new(handle.clone()), Node { /* minimap rect */ ..default() }, GlobalZIndex(10)));

fn update_fog(fog: Res<FogImage>, mut images: ResMut<Assets<Image>>, units: Query<&Transform, With<Unit>>) {
    // get_mut returns Option<AssetMut<Image>> (marks Modified only if actually mutated).
    let Some(mut image) = images.get_mut(&fog.0) else { return; };
    // `data` is Option<Vec<u8>>, tightly packed RGBA8 rows.
    let Some(data) = image.data.as_mut() else { return; };
    for px_rgba in data.chunks_exact_mut(4) { px_rgba[3] = 200; }
    // .. write data[(y * W + x) * 4 + 3] = 0 for revealed cells ..
    let _ = image.set_color_at(0, 0, Color::srgba(0.0, 0.0, 0.0, 0.8)); // per-pixel helper (Result)
}
```

* **[verified at runtime, windowed]** The same image updates both the 3D overlay and the UI minimap
  every frame. On the cuboid's top face, u runs along +X and v along +Z (row `y` of the image maps
  to world +Z).
* Source: `bevy_image-0.19.1/src/image.rs:613-645` (`Image{data: Option<Vec<u8>>, sampler,
  texture_descriptor, asset_usage, ..}`), `:693` (`ImageSampler::nearest`), `:1185` (`new_fill`);
  `bevy_asset-0.19.1/src/assets.rs:440` (`get_mut -> Option<AssetMut<A>>`),
  `render_asset.rs:32` (`RenderAssetUsages`); `examples/2d/cpu_draw.rs`.
* **Gotchas:**
  * `image.data` is an `Option`.
  * `pixel_bytes(_mut)` / `pixel_data_offset` return `Result` in 0.19.
  * `RenderAssetUsages` lives in `bevy::asset`, not `bevy::render::render_asset`.
  * Each `get_mut` that actually writes re-uploads the whole texture, so keep fog textures small.
  * `images.get_mut_untracked(&h)` returns a plain `&mut Image` with no `Modified` event, so the GPU
    copy is not refreshed.

---

## 13. Gizmos

```rust
fn draw_gizmos(mut gizmos: Gizmos, selected: Query<&Transform, With<Selected>>) {
    // Gizmo circles/rects are drawn in the local XY plane; rotate onto the XZ ground.
    let flat = Quat::from_rotation_x(FRAC_PI_2);
    for t in &selected {
        gizmos.circle(Isometry3d::new(t.translation + Vec3::Y * 0.05, flat), 0.9, Color::srgb(0.2, 1.0, 0.2));
    }
    gizmos.line(Vec3::ZERO, Vec3::X * 5.0, Color::srgb(1.0, 0.0, 0.0));
    gizmos.rect(Isometry3d::new(Vec3::Y * 0.02, flat), Vec2::splat(10.0), Color::WHITE);
    gizmos.grid(Isometry3d::new(Vec3::Y * 0.01, flat), UVec2::splat(16), Vec2::splat(4.0), Color::srgba(1.0, 1.0, 1.0, 0.1));
}
```

For a screen-space drag rectangle, use an absolutely positioned UI `Node`, not gizmos:

```rust
commands.spawn((
    SelectionBox,
    Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: px(0), height: px(0),
           border: UiRect::all(px(1)), ..default() },
    BackgroundColor(Color::srgba(0.2, 1.0, 0.2, 0.1)),
    BorderColor::all(Color::srgb(0.2, 1.0, 0.2)),
    Visibility::Hidden, GlobalZIndex(100), Pickable::IGNORE,
));
// each frame while dragging:
let min = start.min(now); let size = (start - now).abs();
node.left = px(min.x); node.top = px(min.y); node.width = Val::Px(size.x); node.height = px(size.y);
```

* Source: `bevy_gizmos-0.19.1/src/gizmos.rs:408` (`line`), `:615` (`rect(Isometry3d, Vec2, color)`),
  `:726`/`:902` (`line_2d`, `rect_2d`); `circles.rs:128` (`circle(Isometry3d, radius, color)`,
  where identity orientation is the XY plane); `grid.rs:217`.
* **Gotcha:** the old signatures `circle(pos, normal, r, c)` and `rect(pos, rot, size, c)` are
  gone. Everything takes `impl Into<Isometry3d>`, and a bare `Vec3` converts too.
  **[verified, windowed]** The circle, rect and grid render flat on the ground.

---

## 14. Screenshot, then exit

```rust
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
fn main() -> AppExit { windowed_app().run() }   // AppExit: Termination
```

* **[verified at runtime]** It logs "Screenshot saved to zz_api_probe_screenshot.png" (a PNG at
  physical resolution) and the process exits 0. `save_to_disk` writes synchronously inside the
  observer. Capture is asynchronous, though: wait several frames after startup, because the
  screenshot arrives a frame or two after the `Screenshot` entity is spawned.
* Source: `bevy_render-0.19.1/src/view/window/screenshot.rs:49` (`ScreenshotCaptured`), `:80`
  (`Screenshot(RenderTarget)`), `:98` (`primary_window`), `:134` (`save_to_disk`);
  `bevy_app-0.19.1/src/app.rs:1554-1560` (`#[derive(Message)] enum AppExit {Success, Error(NonZero<u8>)}`);
  `examples/window/screenshot.rs`.
* **Gotchas:**
  * `ScreenshotManager` is gone.
  * `EventWriter<AppExit>::send` is now `MessageWriter<AppExit>::write`.
  * Window close/exit systems moved to `Last` in 0.19.

---

## 15. Assets root

`bevy_asset-0.19.1/src/io/file/mod.rs:19-29`:

```rust
pub(crate) fn get_base_path() -> PathBuf {
    if let Ok(manifest_dir) = env::var("BEVY_ASSET_ROOT") {          // line 20
        PathBuf::from(manifest_dir)
    } else if let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") { // line 22
        PathBuf::from(manifest_dir)
    } else {
        env::current_exe() /* parent dir */
    }
}
```

The asset folder is `<base>/<AssetPlugin::file_path>` and `file_path` defaults to `"assets"`.
`CARGO_MANIFEST_DIR` under `cargo run -p aetherfall` is `crates/aetherfall`, not the workspace
root, which is why `.cargo/config.example.toml` sets `BEVY_ASSET_ROOT = "."` (relative to the workspace root) for machines that build outside the project.
A shipped exe with neither variable set looks next to the executable.

---

## 16. Math

```rust
let v3 = Vec3::new(1.0, 2.0, 3.0);
let ground: Vec2 = v3.xz();               // (1, 3)
let back: Vec3 = ground.extend(0.0);      // (1, 3, 0)  (extend adds z)
let world = Vec3::new(ground.x, 0.0, ground.y);
let dir: Dir3 = Dir3::new(world).unwrap_or(Dir3::NEG_Z);
let rot = Rot2::degrees(90.0);
let rotated: Vec2 = rot * Vec2::X;
let iso = Isometry3d::new(Vec3::ZERO, Quat::from_rotation_y(FRAC_PI_2));
let moved: Vec3 = iso * Vec3::X;
let angle = ops::atan2(rotated.y, rotated.x);
let s = ops::sin(angle) + angle.sin();    // both fine
// camera pan: map screen-ish Vec2 to XZ: pan.extend(0.0).xzy()
```

* `ops::{sin, cos, atan2, sqrt, powf, ..}` (`bevy_math-0.19.1/src/ops.rs:22`) forward to the std
  `f32` methods by default, so plain `f32::sin` is equivalent and fine. Enabling bevy's `libm` cargo
  feature (`bevy-0.19.1/Cargo.toml:2809` → `bevy_math/libm`, glam/libm) switches `ops::*` and glam
  to `libm`, which has specified precision. That is the switch to flip if you need
  cross-platform-identical float results for a lockstep sim. `f32::sin` called directly always
  uses std, so in deterministic sim code call `ops::sin`, not `x.sin()`.
* All of `Vec2/3`, `Quat`, `Dir2/3`, `Rot2`, `Isometry2d/3d`, `Ray3d`, `InfinitePlane3d`, `Cuboid`,
  `Plane3d`, `Sphere` and `ops` are in the prelude.

---

## 17. Random numbers

Bevy 0.19 does **not** re-export an RNG crate. `bevy_math`'s `rand` feature only adds the
`ShapeSample`/`FromRng` traits, which still need a `rand::Rng` from your own `rand` dependency.
Examples use `chacha20` + `rand 0.10`, and in 0.10 `Rng` was renamed `RngExt` and `RngCore` renamed
`Rng` (migration guide `rand_glam_uuid_update.md`). Without adding a dependency, use a tiny seeded
generator, which is also what a deterministic sim wants:

```rust
#[derive(Resource, Clone, Copy)]
struct SimRng(u64);
impl SimRng {
    fn next_u32(&mut self) -> u32 { let mut x = self.0; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.0 = x; (x >> 32) as u32 }
    fn next_f32(&mut self) -> f32 { (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32 }
}
```

`bevy::platform::collections::HashMap` uses a fixed seed hasher (`FixedHasher`/foldhash,
`bevy_platform-0.19.1/src/hash.rs:18-23`). Iteration order is therefore reproducible run to run
for the same inserts, but sort before anything that must be order-independent.

---

## Renamed/removed since 0.14–0.16 (everything hit while building the probe)

| Old (0.14–0.16 era) | Bevy 0.19.1 |
|---|---|
| `#[derive(Event)]` for buffered events | `#[derive(Message)]` |
| `EventReader<T>` / `EventWriter<T>` / `Events<T>` | `MessageReader<T>` / `MessageWriter<T>` / `Messages<T>` (+ `MessageMutator<T>`) |
| `writer.send(e)` | `writer.write(m)` (`write_default()`, `write_batch()`) |
| `app.add_event::<T>()` | `app.add_message::<T>()` |
| `world.send_event(e)` | `world.write_message(m)`, `commands.write_message(m)` |
| `Trigger<E>` observer param, `trigger.target()`/`entity()` | `On<E>`, `event.entity` (EntityEvent field) |
| `#[derive(Event)]` + `commands.trigger_targets(e, entity)` | `#[derive(EntityEvent)] struct E { entity: Entity, .. }` + `commands.trigger(E{entity,..})` or `entity_commands.trigger(|entity| E{..})` |
| `OnAdd` / `OnInsert` / `OnReplace` / `OnRemove` / `OnDespawn` | `Add` / `Insert` / **`Discard`** (0.19) / `Remove` / `Despawn` |
| `#[component(on_replace = ..)]`, `ComponentHooks::on_replace` | `on_discard` (0.19) |
| `EventWriter<AppExit>` + `AppExit::Success` | `MessageWriter<AppExit>` + `.write(AppExit::Success)` |
| `despawn_recursive()` | `despawn()` (recursive by default) |
| `despawn_descendants()` | `despawn_children()` / `despawn_related::<Children>()` |
| `Parent` component, `parent.get()` | `ChildOf(pub Entity)`, `child_of.parent()` |
| `set_parent(p)` / `remove_parent()` | `insert(ChildOf(p))` / `remove::<ChildOf>()` |
| `with_children` only | also `children![..]` macro, `with_child`, `related!` |
| `query.get_single()` / `get_single_mut()` (Result) and panicking `single()` | `single()` / `single_mut()` return `Result` |
| `Commands::add(cmd)` | `Commands::queue(cmd)` |
| `impl Command<Result>` / `HandleError` | `impl Command { type Out = Result; }` (0.19) |
| `DefaultErrorHandler` / `default_error_handler` | `FallbackErrorHandler` / `app.set_error_handler(..)` (0.19) |
| `#[derive(Component, Resource)]` on one type | not allowed: `Resource` now implies `Component` (0.19) |
| `init_non_send_resource` / `insert_non_send_resource` | `init_non_send` / `insert_non_send` (0.19, deprecated aliases) |
| `bevy::utils::HashMap` / `HashSet` | `bevy::platform::collections::{HashMap, HashSet}` |
| `StateScoped(S)` | `DespawnOnExit(S)` (also `DespawnOnEnter`) |
| `NextState(Some(s))` tuple (≤0.13) | `NextState` enum: `.set(s)` / `(*next).set_if_neq(s)` / `.reset()` |
| `time.delta_seconds()` / `elapsed_seconds()` | `time.delta_secs()` / `elapsed_secs()` |
| (no equivalent) | `TimeUpdateStrategy::FixedTimesteps(n)`, `commands.delayed()` |
| `PbrBundle`, `MaterialMeshBundle`, `Handle<Mesh>` as component | `Mesh3d(handle)` + `MeshMaterial3d(handle)` (required components) |
| `Camera3dBundle` / `Camera2dBundle` | `Camera3d` / `Camera2d` components |
| `OrthographicProjection`/`PerspectiveProjection` inserted as components (via bundles) | only `Projection` is a component: `Projection::from(PerspectiveProjection{..})` |
| `OrthographicProjection::default()` for 3D | `OrthographicProjection::default_3d()` |
| `Msaa` resource | `Msaa` component on the camera |
| `Camera { hdr: true }` / `bevy::render::view::Hdr` | `bevy::camera::Hdr` marker component (0.19 move) |
| `camera.viewport_to_world(..) -> Option<Ray3d>` | `-> Result<Ray3d, ViewportConversionError>` (same for `world_to_viewport`) |
| `ray.intersect_plane(o, Plane3d)` (≤0.13) | `ray.intersect_plane(o, InfinitePlane3d::new(Vec3::Y))` (+ `plane_intersection_point`) |
| `AmbientLight` resource | `GlobalAmbientLight` resource; `AmbientLight` is a per-camera component |
| `shadows_enabled` on Point/Spot/DirectionalLight | `shadow_maps_enabled` (0.19) |
| `Color::rgb/rgba` (≤0.13) | `Color::srgb/srgba` (+ `srgb_u8`, `linear_rgb`, `hsl`) |
| `Assets::get_mut -> Option<&mut A>` | `Option<AssetMut<A>>` (0.19); `get_mut_untracked` for raw `&mut` |
| `Image::pixel_bytes -> Option` | `-> Result<_, TextureAccessError>` (0.19) |
| `bevy::render::render_asset::RenderAssetUsages` | `bevy::asset::RenderAssetUsages` |
| `ScreenshotManager::save_screenshot_to_disk` | `commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path))` |
| `Gizmos::circle(pos, normal, r, c)`, `rect(pos, rot, size, c)` | `circle(Isometry3d, r, c)`, `rect(Isometry3d, size, c)` |
| `MouseWheel` event-sum by hand | `Res<AccumulatedMouseScroll>`, `Res<AccumulatedMouseMotion>` |
| `WindowResolution::new(f32, f32)` | `WindowResolution::new(u32, u32)` (treated as logical size at creation) |
| `Window.cursor.visible/grab_mode` | separate `CursorOptions` component / `WindowPlugin::primary_cursor_options` |
| `NodeBundle`, `Style`, `ButtonBundle`, `ImageBundle`, `TextBundle` | `Node` (Style merged in), `Button`, `ImageNode`, `Text` |
| `UiImage` | `ImageNode` |
| `Text::from_section(s, TextStyle{..})`, `TextSection` | `Text::new(s)` + `TextFont` + `TextColor`; extra parts = `TextSpan` children |
| `TextFont { font: Handle<Font>, font_size: f32 }` | `font: FontSource` (`handle.into()`), `font_size: FontSize::Px(f32)` (0.19) |
| `TextLayout::new_with_justify/linebreak/no_wrap` | `TextLayout::justify/linebreak/no_wrap` (0.19) |
| `Font::try_from_bytes(bytes)` | `Font::from_bytes(bytes)`, one arg in 0.19.1 (the migration guide's 2-arg form is stale) |
| `BorderColor(Color)` | `BorderColor { top, right, bottom, left }`, `BorderColor::all(c)` |
| `BorderRadius` component | `Node::border_radius` field (`BorderRadius` is now a plain struct) |
| `Val::Px(x)` only | also `px(x)`, `percent(x)`, `vw(x)`, `vh(x)` helpers |
| `TargetCamera` | `UiTargetCamera` |
| `InputFocus.0` | `input_focus.get()/set()/clear()` (0.19) |
| `SceneRoot`, `Scene`, `DynamicScene`, `ScenePlugin` (old bevy_scene) | `WorldAssetRoot`, `WorldAsset`, `DynamicWorld`, `WorldSerializationPlugin` (0.19; GLTF: `WorldAssetRoot(asset_server.load("x.gltf#Scene0"))`) |
| `UiWidgetsPlugins`/`InputDispatchPlugin` added manually | now in `DefaultPlugins` (0.19) |
| `3d` feature implied `ui` | `ui` is a separate feature (still in `default`) (0.19) |
| `bevy_picking` (third-party `bevy_mod_picking`) | upstream `bevy::picking`; `MeshPickingPlugin` must be added manually; UI picking is on by default |
| `rand::Rng` / `RngCore` (rand 0.8/0.9) | rand 0.10: `RngExt` / `Rng` (if you add rand yourself) |

---

## 15 most surprising differences (summary)

1. Buffered events are now **Messages** (`MessageReader/Writer`, `write`, `add_message`), and
   `Event` means observer-only. `AppExit` and all input events are Messages.
2. Observers take `On<E>`. Lifecycle events are `Add/Insert/Discard/Remove/Despawn`, and
   `Replace` became `Discard` in 0.19. An `EntityEvent` targets its `entity` field.
3. Under the default error handler, a command on a dead entity (`insert`) or a missing `Res<T>`
   **panics**. Use `try_insert`/`try_despawn` or `app.set_error_handler(warn)`.
4. `despawn()` is recursive, `despawn_recursive` is gone, and `Parent` is now `ChildOf`
   (`.parent()`).
5. `single()` returns `Result`. `get_single` is gone. `Single<..>`/`Populated<..>` params
   *silently skip* the system.
6. `TimeUpdateStrategy::FixedTimesteps(n)` gives exact ticks per update. The **first**
   `app.update()` runs 0 fixed ticks, and catch-up is capped by `Time<Virtual>` max_delta
   (250 ms), so 10 steps/update at 20 Hz gives 5 unless you raise `max_delta`.
7. Every UI `Node` is pickable and blocks by default. Use `HoverMap` + `Query<(), With<Node>>` to
   gate world clicks, and put `Pickable::IGNORE` on transparent layout roots. `MeshPickingPlugin`
   is not in `DefaultPlugins`.
8. `BorderRadius` is a `Node` field, `BorderColor` is per-side (`::all`), and
   `font_size: FontSize::Px(..)`.
9. `shadows_enabled` was renamed `shadow_maps_enabled`, the ambient light resource is
   `GlobalAmbientLight`, and `AmbientLight` is a camera component.
10. `viewport_to_world`/`world_to_viewport` return `Result`. Ray-plane uses `InfinitePlane3d`.
11. `Assets::get_mut` returns `AssetMut` (needs `let Some(mut ..)`), and `Image::data` is
    `Option<Vec<u8>>`.
12. `WindowResolution::new` takes `u32` but is treated as logical size, so on a 125% display
    1280×720 becomes a 1600×900 physical window/screenshot.
13. `PrimaryWindow`, `on_timer`, `input_just_pressed`, `NotShadowCaster`, `StatesPlugin` and
    `RunSystemOnce` are **not** in the prelude, and Bevy re-exports no RNG.
14. `#[derive(Resource)]` implies `Component` (0.19), so you can't derive both, and broad
    `Query<Entity>` now also sees resource entities.
15. `next.set_if_neq(s)` on `ResMut<NextState<S>>` does not compile (it hits
    `DetectChangesMut::set_if_neq`), so write `(*next).set_if_neq(s)`. Also,
    `Font::from_bytes` takes one argument despite what the migration guide says.
