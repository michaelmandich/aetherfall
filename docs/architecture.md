# Aetherfall architecture

Rust + Bevy 0.19.1. Everything visible is a box. The simulation is a 2D
fixed-timestep model on the ground plane; rendering is a separate layer
that turns sim state into boxes, so "real graphics" later means replacing
`af_render` only.

## Crates

| crate | role | depends on |
|---|---|---|
| `af_data` | content schema, RON loader, validator (`cargo run -p af_data --example check`) | serde, ron |
| `af_geom` | ground-plane geometry (OBB/SAT/rays), `NavGrid`, A* | bevy_math |
| `af_core` | **the contract**: components, resources, messages, sets, rules, spawn, placement, spatial index, test helpers | bevy, af_data, af_geom |
| `af_map` | match setup: terrain generation, resources, players, start spawns, teardown | af_core |
| `af_movement` | obstacle sync into the nav grid, path service, steering, no-overlap collisions | af_core |
| `af_behavior` | unit commands, order state machines, gathering/building/repair, chasing, stances, healing | af_core |
| `af_combat` | target acquisition, all weapon kinds, projectiles/FX, damage/armour/shields, death | af_core |
| `af_economy` | placement, construction, production queues, research/ages/techs, upgrades, trickle, victory | af_core |
| `af_fog` | per-team visibility/exploration | af_core |
| `af_ai` | computer players (only via `PlayerCommand`s) | af_core |
| `af_input` | camera, picking, selection, control groups, commands, placement modes, hotkeys | af_core |
| `af_ui` | skirmish menu, HUD, command card, minimap, notices, game over | af_core |
| `af_render` | box visuals for terrain, entities, weapons, FX, bars, selection, fog, ghosts; animated glTF models for the units listed in `assets/visuals/units.ron` | af_core, serde, ron |
| `aetherfall` | the binary: wiring, CLI, headless harness, screenshot mode | all |

Crates never depend on each other sideways; they communicate only through
`af_core` types. A crate that must share something new adds it to its own
file `crates/af_core/src/ext/<crate>.rs` (never editing other af_core files).

## Simulation model

* `FixedUpdate` at 30 Hz (`SIM_DT`) while `GameState::Playing`, in the chained
  `SimSet` order documented in `crates/af_core/src/sets.rs`.
* Sim truth: `Position(Vec2)`, `Facing(f32)`, `Altitude`, `Footprint` (box half
  extents + height). `Vec2 = (world x, world z)`, yaw 0 faces +Z. Sim code
  never reads `Transform`.
* Everything is an oriented box (`af_geom::Obb`). Boxes never overlap:
  buildings and blocking nodes are static `Obstacle`s rasterised into the
  nav grid; units push out of each other and of obstacles with SAT.
* Players act only through `PlayerCommand` pushed into `CommandInbox`; they
  execute next tick from `TickCommands` (lockstep-friendly).
* Content is data: `Data(Arc<GameData>)`. Effective numbers come from
  `af_core::stats` (techs) and permissions from `af_core::rules`, shared by
  economy, UI and AI.
* Deterministic RNG: `SimRng`. Deaths: insert `Dead` + write `EntityDied`;
  af_core despawns in `Cleanup`.
* What a unit is busy with: `Activity` (af_core `components.rs`), written
  only by af_behavior at the end of `SimSet::Orders`: `Gather(WorkStyle)`
  (at the node or field), `Build`, `Repair` (at the target), `Heal`,
  `Fight`, else `Idle`. It names the task, not the movement: walking is
  `Velocity`, the load is `Gatherer`. Each resource's `work` style in
  `resources.ron` (`Chop`, `Mine`...) says how its nodes are worked;
  fields are always `Farm` (`af_core::ext::behavior::gather_style`).
  Nothing in the sim reads it; it exists for the renderer (and anyone
  else who asks).
* Determinism: sim spawns draw entity ids from a reserved range
  (`af_core::sim_ids`), so client-side spawns (visuals, UI) never shift the
  ids the simulation sees; the same seed replays identically headless or
  windowed (`crates/aetherfall/tests/determinism.rs`).
* Invariants: `crates/aetherfall/tests/invariants.rs` runs AI matches and
  checks every tick that no boxes overlap (> 0.1 m is a hard failure), that
  nothing is stuck, NaN, leaking or mis-counted. Long sweeps:
  `cargo test --release -p aetherfall --test invariants -- --include-ignored`.

## Client model

`Update`: `ClientSet::Pointer` (UI hit-test) -> `Input` -> `Ui` -> `Render`.
Shared client state lives in af_core resources: `CursorWorld`, `InputMode`,
`CameraRig`, `DragSelect`, `LocalPlayer`, and the `Selected` marker.

## Visual models

Units listed in the visual registry are drawn with animated glTF models;
every other unit, and every building, node and effect, stays boxes.

* **Registry:** `assets/visuals/units.ron` maps unit ids to model
  manifests (`{"peasant": "models/peasant/peasant.model.ron"}`).
* **Manifest:** written by the art build next to its glTF file: the model,
  its standing height, and per clip its duration and optional `speed`,
  `hit` and `reach`. The contract (clip names, props, conventions) is
  `docs/art/peasant.md`; the scripted Blender pipeline is
  `art/peasant/README.md`.
* **Loading** (`af_render::model::library`): at startup, from the asset
  folder the content came from (`main.rs` points Bevy's asset root there,
  so no `BEVY_ASSET_ROOT` is needed). A unit is boxes until its model has
  loaded, then rebuilt with it; a missing or broken manifest or model
  leaves its units as boxes with a warning, and a bad clip only loses that
  clip (missing clips fall back: CarryWalk to Walk, work clips to Idle,
  Repair to Build).
* **Drawing:** the model replaces the body, weapon and cargo boxes in the
  unit's visual root, scaled uniformly to the unit's box height; the
  selection outline, bars, shield shell and fog hiding work as for boxes.
  The box footprint stays the collision shape.
* **Clips** (`model::animator`, pure logic): from `Activity`, `Velocity`
  (walk rate matched to ground speed), `Gatherer` cargo, weapon windups
  (Attack lands its `hit` with the sim's) and hit points; one-shots for
  Drop, Spawn, HitReact, Cheer and an occasional IdleFidget.
* **Props** follow the clip being played: the work clip's tool, the load
  while carrying, the weapon while fighting, else the tool of the work
  walked to.
* **Stance** (`model::stance`): the sim stops workers about 0.7 m short of
  their work and never turns them to it, so while a unit works its model
  turns toward the target and steps up until the clip's `reach` lands on
  the target's drawn surface; the outline and bars move with it.
* **Corpses:** a dying unit's model is detached as a corpse (no entity is
  spawned inside the sim tick), plays Death, lies still, then sinks.

To give another unit a model: build it with the art pipeline (its own
`art/<unit>/`, following `docs/art/peasant.md`), then add a line to
`assets/visuals/units.ron`.

Dev flags for checking models in the real game (`--help`): `--select
doing:<activity>` / `carrying:<resource>` with `--follow` and
`--camera SPOT,DIST,TURN` (close-ups down to 5 m), `--shot-at +S` (shots
timed from the pick), and `--no-models`, `--no-vsync` and `--frame-stats S`
(performance).

## Running

```
cargo run -p aetherfall                                   # menu
cargo run -p aetherfall -- --quick --faction ironhold --players 3
cargo run -p aetherfall -- --headless --players 2 --minutes 15 --report-every 60
cargo run -p aetherfall -- --quick --spectate --players 4 --reveal --screenshot shot.png --screenshot-at 30
cargo test --workspace
```

Build output goes to `target/` unless a per-machine `.cargo/config.toml`
says otherwise (see `.cargo/config.example.toml`: faster linking on Windows,
and a build folder outside a synced project folder).
