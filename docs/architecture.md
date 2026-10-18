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
| `af_render` | box visuals for terrain, entities, weapons, FX, bars, selection, fog, ghosts | af_core |
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
