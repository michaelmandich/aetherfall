# Aetherfall architecture

Rust + Bevy 0.19.1. The simulation is a 2D fixed-timestep model on the
ground plane; rendering is a separate layer (`af_render`) that turns sim
state into pictures: the land as a height field painted by its own
shaders, units, buildings and nodes as glTF models where they have one and
boxes where they do not, effects and markers as boxes.

## Crates

| crate | role | depends on |
|---|---|---|
| `af_data` | content schema, RON loader, validator (`cargo run -p af_data --example check`) | serde, ron |
| `af_geom` | ground-plane geometry (OBB/SAT/rays), `NavGrid`, A*, terrain regions and coarse routes | bevy_math |
| `af_core` | **the contract**: components, resources, messages, sets, rules, spawn, placement, spatial index, test helpers | bevy, af_data, af_geom |
| `af_map` | match setup: terrain generation, resources, players, start spawns, teardown | af_core |
| `af_movement` | obstacle sync into the nav grid, terrain regions (`NavRegions`), path service, steering, no-overlap collisions | af_core |
| `af_behavior` | unit commands, order state machines, gathering/building/repair, chasing, stances, healing | af_core |
| `af_combat` | target acquisition, all weapon kinds, projectiles/FX, damage/armour/shields, death | af_core |
| `af_economy` | placement, construction, production queues, research/ages/techs, upgrades, trickle, victory | af_core |
| `af_fog` | per-team visibility/exploration | af_core |
| `af_ai` | computer players (only via `PlayerCommand`s) | af_core |
| `af_input` | camera, picking, selection, control groups, commands, placement modes, hotkeys | af_core |
| `af_ui` | skirmish menu, HUD, command card, minimap, notices, game over | af_core |
| `af_render` | the land (height field, water, props, the fog of war, light and camera look); box visuals for entities, weapons, FX, bars, selection, ghosts; animated glTF models for the units and static glTF models for the buildings and nodes listed in the visual registry (`assets/visuals/*.ron`) | af_core, serde, ron |
| `af_sim` | the simulation wired together: every sim plugin in one call, the headless runner, skirmish setup, match reports, the `SimSet` profiler, the referee app | af_core and every sim crate |
| `af_net` | multiplayer client: wire protocol (shared with the server), TCP framing, the lockstep client plugin, the account-service client | af_core, serde, postcard, ureq |
| `aetherfall` | the game binary: wiring, CLI, screenshot and dev tooling | all |
| `af_server` | the multiplayer server binary: casual rooms relayed in lockstep, ranked matches refereed with `af_sim`, Elo, Supabase accounts (`docs/multiplayer.md`) | af_sim, af_net |

Crates never depend on each other sideways; they communicate only through
`af_core` types. A crate that must share something new adds it to its own
file `crates/af_core/src/ext/<crate>.rs` (never editing other af_core files).
The multiplayer UI is an example: af_ui reads `af_core::ext::net::NetSession`
and pushes `NetAction`s; af_net (which af_ui never sees) does the talking.

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
  execute next tick from `TickCommands`. Online, the server's turns are what
  reaches the inbox (`af_net::client`), and the local player's commands
  leave it for the server instead; see `docs/multiplayer.md`.
* `MatchSetup::rule` picks the victory rule: `LastStanding` (a player is out
  with nothing left) or `TownHalls` (a team is out with no main hall left;
  ranked 2v2).
* Content is data: `Data(Arc<GameData>)`. Effective numbers come from
  `af_core::stats` (techs) and permissions from `af_core::rules`, shared by
  economy, UI and AI.
* Deterministic RNG: `SimRng`. Deaths: insert `Dead` + write `EntityDied`;
  af_core despawns in `Cleanup`.
* Deterministic math: sim code never calls `f32::sin`, `cos`, `tan`,
  `atan2`, `powf` or `Vec2::from_angle` (the platform's libm differs in the
  last bit); it uses `af_geom::dmath`, so a Windows, Mac and Raspberry Pi
  simulation of the same match agree bit for bit. Square roots, rounding
  and arithmetic are exact everywhere. `af_core::hash::sim_hash` digests a
  match for the lockstep desync check.
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
  ids the simulation sees. Nor does the order a query visits entities in
  matter, which is table order and changes whenever the client puts a
  component on a sim entity (`Selected`, `BoxVisual`, a model marker, the
  `FxDrawn` of every effect): wherever one entity's turn reaches another's
  (shared nodes, field slots, stocks and hit points, float sums, `SimRng`
  draws, spawns, despawns, messages, notices, the path queue, the first come
  taking a place), the system goes in entity index order
  (`af_core::by_entity_index`: sorted once a tick into a buffer kept in a
  `Local`; fresh ids are handed out in spawn order, so this is mostly the
  order a headless run's tables hold anyway, older first), and every
  spatial index cell lists its entries by entity index
  (`SpatialIndex::sort_cells`), so searches and their ties do not depend on
  table order either. The systems that do: `collect_requests` (path
  queue), `steer` (avoidance sums), `sync_obstacles` (registry slots),
  gathering, building and repair, healing, the fight orders' notices,
  `cycle_weapons`, every projectile system, `animate_fx` (despawns),
  `advance_queues`, `complete_construction` and `trickle_income`; the
  collision solver, `aim_weapons`, Cleanup and the AI snapshot already
  did. New sim code that lets one entity's work reach another's must do the
  same. So the same seed replays identically headless or windowed, and
  `--select` changes nothing (`crates/aetherfall/tests/determinism.rs`:
  `client_markers_on_sim_entities_do_not_change_the_match` tags every sim
  entity from tick 90 on as a client does, every one with one empty marker
  and every other one of each kind with another, and checks every unit,
  building, node and stock bit for bit, entity ids included, after five
  sim minutes; `--release -- --ignored` adds a four-player match with
  fighting, twelve minutes). Cost: about 0.02 ms per tick (some 5 %) in
  the ten-minute six-player match of `tests/profile.rs` on the
  single-threaded executor; on the default one it is lost in run-to-run
  noise.
* Pathing on big maps (`docs/design/maps-and-terrain.md`, section 7): a
  single A* search stops after 24,000 cells, so a river or a range across
  the map is handled a level up. af_geom labels the connected regions of
  the terrain per mover class (the terrain its mobility enters x its
  clearance; obstacles and teams left out, so they hold all match) and
  builds a coarse graph over them, one node per connected part of each
  8 x 8 cell block (`af_geom::regions`). af_movement publishes them as
  `af_core::ext::movement::NavRegions`: made on entering a match (the
  start units' classes built at once) and again whenever the grid's
  terrain stamp changes; any other class's map is built on first use, a
  pure function of the terrain, whoever asks. The path service answers a
  goal out of the mover's region at once with the reachable point nearest
  it (no search); plans a long way that no straight line covers on the
  coarse graph and, when the route bends round the terrain, searches it
  leg by leg (64 m of route a leg; the next leg is searched from the end
  of the path while the unit walks its last 24 m, so it never stands
  waiting); measures progress by the way left (path plus route beyond);
  and serves the players' queues in turn, one request each round.
  af_behavior's automatic choices (drop sites, next nodes, fields,
  formation slots, patients, hostiles to chase) and af_ai's spots,
  workers, targets and attack progress read the same regions and route
  distances; without `NavRegions` (flat-world tests) everything is
  reachable by the straight line.
* Invariants: `crates/aetherfall/tests/invariants.rs` runs AI matches and
  checks every tick that no boxes overlap (> 0.1 m is a hard failure), that
  nothing is stuck (by progress along its path), NaN, leaking or
  mis-counted. Long sweeps:
  `cargo test --release -p aetherfall --test invariants -- --include-ignored`
  (`sweep_big_maps`: six to eight players on 512-768 m maps); `AF_SWEEP`
  names matches and their map options (see `sweep_custom`).

## Client model

`Update`: `ClientSet::Pointer` (UI hit-test) -> `Input` -> `Ui` -> `Render`.
Shared client state lives in af_core resources: `CursorWorld`, `InputMode`,
`CameraRig`, `DragSelect`, `LocalPlayer`, and the `Selected` marker.

## The land

`af_render::terrain` draws the map from its terrain grid, once per match
(design: `docs/design/maps-and-terrain.md` section 6; look:
`docs/art/style-guide.md`, "Landscape").

* **Field** (`terrain::field`): what every cell looks like and the height
  of every cell corner, over the map and a 64 m off-map skirt that carries
  the edge on (sea beyond a sea edge, rivers winding on along their lines
  and narrowing, other water and mountains straight out, else meadow,
  woodland and a frame of mountains; the first ring beyond the edge always
  carries its water on, as `off_map_terrain` assumes). Every cell units
  walk on is drawn exactly at `af_core::ext::render::ground_height`
  (corner heights, interpolated on the drawn triangles); relief only rises in
  mountains (a sharp crest, at most 10.5 m, below every flyer) and sinks in
  water beds. A pure function of the map: the nav terrain, the seed and
  `MapFeatures` when af_map publishes them (river lines give the water its
  flow; without them rivers are still).
* **Meshes** (`terrain::mesh`): chunks of 64 x 64 cells, normals from the
  whole field so chunks meet without seams; flat chunks cast no shadows.
  An opaque water surface at `WATER_LEVEL` covers water cells and reaches
  under the banks; its depth look comes from per-vertex bed depth.
* **Shaders** (`terrain/*.wgsl`, embedded in the crate and imported from
  `common.wgsl`): `StandardMaterial` extended, so sun, shadows and haze
  stay Bevy's. The ground blends the cells' covers with noise-warped
  weights from two class images and paints each procedurally; it also
  paints the water's rim on the bank, so shores curve. The water streams
  ripples and foam down rivers in their own downstream/across coordinates.
  All three draw the fog of war from `FogTexture` (and dark beyond the
  map while it is shown, though the water still mirrors the sky) and grey
  the skirt; spectators and fog-off matches see everything. The meadow
  is painted from large to small (swathes, meadows, field-like plots,
  patches) with soft fine detail. Animation reads `globals.time` with periods
  that divide an hour; materials change only when the fog binding does.
* **Props** (`terrain::props`): brambles, reeds, scree (also down the
  meteor sites' rims), meteor rock and the Starspire (a cluster of
  pointed crystal prisms, its own shadow-casting mesh), from cell hashes,
  only on unbuildable ground, merged per chunk.
* **Decor** (`terrain::decor`): scenery models from the registry's `decor`
  section (mountain peaks, ridges and foothills, crater rims, the
  Starspire's monoliths, reeds, river rocks, lily pads, brambles, sea
  stacks and cliffs, and off the map bridges, ruins and backdrops), placed
  once per match by `decor::plan`, a pure function of the field and
  `MapFeatures` (rules: `docs/art/static-models.md`, "Decor"): never a sim
  entity, never on buildable ground, only low pieces where units walk,
  landmarks only beyond the edge. The land waits (at most 5 s) for decor
  models still loading. Each part is an entity sharing its model's mesh
  and a fog-aware copy of its material (`DecorMaterial`, `decor.wgsl`), so
  they batch; big pieces cast shadows, small ones do not and are culled
  past 340 m. Where brambles or reeds have models, a share of the
  procedural ones stays. With no decor models the land is as before.
* **Published:** `TerrainSurface` (the drawn top of the map: ground, rock
  or water surface, with each cell's triangle split, so `top_at` lies on
  the drawn triangles) for markers on mountains and water, and for
  af_input's cursor ray, so clicks land on the rock and water that are
  drawn.
* **Headless** apps (no `PbrPlugin`) get the same chunk entities with
  plain `StandardMaterial`s, made once per match.
* **Look of the light** (`scene`): a warm afternoon sun, sky-tinted
  ambient light, a hazy sky with distance fog matched to it, colour grading
  and soft bloom over glows only, put on the RTS camera by an observer;
  `fit_haze` scales the haze with the camera's distance from its focus
  whenever it moves, so far zooms never turn milky.

## Visual models

Units listed in the visual registry are drawn with animated glTF models,
buildings and resource nodes listed there with static ones; everything
else (and every effect) stays boxes.

* **Registry** (`af_render::registry`): every `*.ron` in `assets/visuals/`,
  merged in file-name order, each with optional sections
  `(units: {...}, buildings: {...}, nodes: {...}, decor: {...})` mapping
  game ids (decor: the ids the land places) to manifests. `units.ron` is hand-written; `static.ron` is written by the
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
whose source was deleted). Engine: `af_render::statics` (decor models load
through the same library; the land places them, see "The land").

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
  colour. The model stands where its entity does: buildings and nodes only
  stand on buildable ground, which is drawn flat at the ground units walk
  on.
  Outlines, bars, fog, fog memory (live and of dead entities) and rally
  flags work as for boxes; placement ghosts stay boxes.
* **Construction:** the foundation's pad and scaffold stay, and the model
  is drawn sunk by `(1 - progress) x height`, rising as it is built
  (`height`: how high its top stands, banner included,
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
