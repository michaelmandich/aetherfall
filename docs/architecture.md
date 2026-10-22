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
| `af_render` | box visuals for terrain, entities, weapons, FX, bars, selection, fog, ghosts; animated glTF models for the units and static glTF models for the buildings and nodes listed in the visual registry (`assets/visuals/*.ron`) | af_core, serde, ron |
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
  Known gap (found 2026-10-19): some sim system depends on the order in
  which it visits units, and adding a client-only component to a unit
  (`Selected`, `BoxVisual`, a model marker) moves it to another archetype
  and changes that order. In a headless test, marking every other unit
  with an empty component at tick 90 changes the match within four
  minutes; marking nodes or buildings does not. So a windowed match can
  drift from its headless replay (and `--select` changes a match). The
  determinism tests do not cover it yet.
* Invariants: `crates/aetherfall/tests/invariants.rs` runs AI matches and
  checks every tick that no boxes overlap (> 0.1 m is a hard failure), that
  nothing is stuck, NaN, leaking or mis-counted. Long sweeps:
  `cargo test --release -p aetherfall --test invariants -- --include-ignored`.

## Client model

`Update`: `ClientSet::Pointer` (UI hit-test) -> `Input` -> `Ui` -> `Render`.
Shared client state lives in af_core resources: `CursorWorld`, `InputMode`,
`CameraRig`, `DragSelect`, `LocalPlayer`, and the `Selected` marker.

### UI skin and press feedback

The menu and HUD are Bevy UI nodes dressed in a painted, worn-ornate
medieval skin (`af_ui::skin`); one look serves every age for now.

* **Textures** (`assets/ui/`, painted at 2x: corners are drawn at half
  size): `panel_wood.png` (tiled planks behind every panel),
  `panel_frame.png` / `bar_frame.png` (9-sliced metal frames for boxes and
  for strips under about 80 px tall), `button_<family>.png` for the ten
  button families (`ButtonSkin`: command, stance, economy, military,
  defence, train, research, upgrade, nav, plain), `glow.png` (hover, armed
  and press light), and the press effects' `burst_ring.png`, `spark.png`
  and `shine.png`. `UiSkin::load` reads a folder of the asset root, so a
  per-age skin later is another folder with the same file names.
* **Applying it:** spawn functions only add markers (`SkinPanel::Panel` /
  `Bar`, `SkinButton(family)`, `SkinGlow` for the faction cards). The
  textures load in the plugin's `build` (the first state's `OnEnter`
  runs before `Startup`); once all are in, `apply_skin` gives each marked
  node its image and decoration children (a frame under the panel's
  content, a glow over a button's wood and under its label, the shine of
  a command card slot), all `Pickable::IGNORE`, and clears the node's flat
  colours. Until then, or for good when a texture is missing (the headless
  tests have no PNG loader), the flat `theme` colours stay.
* **Command card:** a slot wears its action's family (a build page button
  and its buildings the page's colours, else the action group's), with
  its hotkey and queued count on the frame's two corner plaques.
  `refresh_card` writes `SkinLook` (usable / soon / not, armed) every
  frame; `skin_looks` turns look, hover and press into the button's tint
  and glow (an armed button pulses gold; the selection's stance glows
  steadily, the chosen faction in its colour). `theme::shade_buttons` and the card's flat colours leave
  skinned nodes alone.
* **Presses** (`af_ui::fx`, real time, so they play while paused): the
  command card records each press, by mouse or hotkey, in `card_input`.
  Build, train, research and upgrade presses squash the button to 0.9 and
  spring it back past full size (a damped spring, still by 0.35 s), flash
  it gold, sweep a shine across it and throw a ring and sparks from its
  centre (on a root node over the HUD, at most 64 particles alive); other
  orders get a lighter squash and flash; a refused press shakes and
  flashes red. Plain buttons squash when clicked. Scale and shake are
  `UiTransform`s, so nothing is laid out again, and nothing runs for a
  button at rest.
* **Checking it:** `cargo run -p af_ui --example ui_preview [-- hud ...]
  --screenshot shot.png`, or the game with `--script` to catch a press
  part-way (`"3 key Q; 3.05 shot a.png; 3.15 shot b.png"`).

## Visual models

Units listed in the visual registry are drawn with animated glTF models,
buildings and resource nodes listed there with static ones; everything
else (and every effect) stays boxes.

* **Registry** (`af_render::registry`): every `*.ron` in `assets/visuals/`,
  merged in file-name order, each with optional sections
  `(units: {...}, buildings: {...}, nodes: {...})` mapping game ids to
  manifests. `units.ron` is hand-written; `static.ron` is written by the
  static art build. An id listed twice keeps its first entry and a file
  that does not parse is skipped, both with a warning. `--visuals DIR`
  merges `assets/DIR/*.ron` on top (its entries win), to try models
  without touching the shipped registry.

### Animated unit models

* **Manifest:** written by the art build next to its glTF file: the model,
  its standing height, and per clip its duration and optional `speed`,
  `hit` and `reach`. The contract (clip names, props, conventions) is
  `docs/art/peasant.md`; the scripted Blender pipeline is
  `art/units/README.md`.
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

To give another unit a model: configure it in the art pipeline
(`art/units/<unit>/unit.py`; "Adding a unit" in `art/units/README.md`),
build it with `art/units/build_unit.py --unit <unit>`, then add a line to
`assets/visuals/units.ron`. A unit model without bones or clips (a
vehicle, siege engine or beast built rigid) is drawn as it is.

### Static building and node models

Contract: `docs/art/static-models.md`; pipeline: `art/static/` (drop a GLB
into `art/static/source/`, run the build; `-- --prune` takes out models
whose source was deleted). Engine: `af_render::statics`.

* **Manifest** (`statics::manifest`): kind, id, `fit`, `random_yaw`, the
  variants (each a GLB with its bounds) and an optional stump; settings
  left out take the contract's defaults. A bad variant or stump only loses
  itself.
* **Loading** (`statics::library`): every variant and stump loads through
  the asset server; once a model's files have all finished, each scene is
  flattened into its mesh primitives (node transforms multiplied out,
  baked emission switched off, the boxes' low reflectance applied so
  models and boxes sit in the same light, the `Team` material marked),
  shared by every instance so Bevy batches them. A building or node is boxes until
  its model is ready, then rebuilt; a model with no loadable variant
  leaves its type as boxes, with a warning.
* **Drawing:** the model replaces the body boxes in the visual root
  (weapon boxes and the shield shell stay), fitted to the type's box by
  the manifest's bounds (`Box` per axis, `Footprint`, `Height`). The
  variant, and for `random_yaw` a turn, come from a hash of the position
  (`statics::place`): stable between runs, the same for every player,
  independent of the simulation. The `Team` material takes the owner's
  colour. The model stands on the highest drawn flat terrain that its box
  footprint overlaps (`entities::Ground::model_lift`: sand is drawn 5 cm
  above the ground units walk on), while the entity stays on that ground.
  Outlines, bars, fog, fog memory (live and of dead entities) and rally
  flags work as for boxes; placement ghosts stay boxes.
* **Construction:** the foundation's pad and scaffold stay, and the model
  is drawn sunk by `(1 - progress) x height`, rising as it is built
  (`height`: how high its top stands, banner and lift onto sand included,
  so nothing shows at 0 %; the opaque terrain hides the buried part); a
  foundation drawn from fog memory rises as far as it was seen. Completion
  and upgrades rebuild the visual with the new type's model, or boxes if
  it has none.
* **Depletion:** non-tree nodes shrink toward 70 % as before. Trees
  (manifests with a stump, or `pine`, `oak`, `palm`) keep their size; when
  one's wood runs out where the local team sees it, an observer detaches
  its model (no spawn inside the sim tick), which falls away from the
  worker gathering it (else a hashed direction) over 1.2 s, lies, sinks
  and goes; its stump stands 90 s, then sinks (`statics::fall`; at most
  32 falling trees and 200 stumps).
* **Cost:** nothing runs per frame for a model at rest: foundations move
  when their progress changes, nodes shrink only when their amount or
  sighting changes, and only the oldest standing stump is looked at.
  Measured with the placeholder set (6 AI players at x8, about 2,300
  buildings and nodes drawn as models, no vsync, RTX 4070 Ti): 3.4 ms a
  frame against 2.4 ms with `--no-models`, at the start zoom and fully
  zoomed out alike.

Dev flags for checking models in the real game (`--help`): `--select
doing:<activity>` / `carrying:<resource>` with `--follow` and
`--camera SPOT,DIST,TURN` (close-ups down to 5 m), `--shot-at +S` (shots
timed from the pick), `--visuals DIR` (extra registry files, such as the
art pipeline's placeholders: `--visuals visuals-placeholders`), and
`--no-models`, `--no-vsync` and `--frame-stats S` (performance; it also
counts the units and the buildings and nodes drawn as models).

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
