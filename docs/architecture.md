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
| `af_input` | camera, picking, selection, control groups, commands (each marked where it lands), placement modes, hotkeys, the unit paths toggle | af_core |
| `af_ui` | skirmish menu, multiplayer screens, HUD, command card, minimap, notices, game over, the unit paths button; a painted skin with card-game press effects | af_core |
| `af_render` | the land (height field, water, props, the fog of war, light and camera look); box visuals for entities, weapons, FX, bars, selection, ghosts, click markers and placement slams, the selected units' routes; animated glTF models for the units and static glTF models for the buildings and nodes listed in the visual registry (`assets/visuals/*.ron`) | af_core, serde, ron |
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
* Patrols: consecutive `Order::Patrol` entries at the front of the queue
  (shift-clicked patrol points, each queued from the last one's `b`) are
  one route, `[front.a, front.b, q1.b, ...]`
  (`af_core::ext::behavior::patrol_points`), walked in order and from the
  last point back to the first, forever; a single patrol is the a <-> b
  beat. The leg is af_behavior's `Brain::patrol_leg`, reset with the task
  when the front order changes; points shift-queued onto a running patrol
  join the loop, and the unit carries on to the point it was walking to
  (`Brain::patrol_to`, the leg that still ends there). A patrol never
  pops, so orders queued after the patrols are never reached.
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
  Client-only components (`Selected`, `BoxVisual`, a model marker) move a
  unit to another archetype and change the order queries visit units, so
  no sim system may depend on that order: whatever is first come first
  served (node room, field slots, the path queue, healers' patients),
  whatever sums floats (steering's avoidance pushes, damage, what workers
  take from a node) and whatever breaks ties by insertion (the spatial
  index) visits entities sorted by `Entity`. `tests/determinism.rs`
  marks every second unit with an empty component and checks that five
  minutes of play stay bit-identical; lockstep peers with different
  selections depend on it.
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
* Chokepoints: `crates/aetherfall/tests/chokepoints.rs` orders crowds
  through narrow gaps (cliff gaps, slots between houses, crossing head-on,
  past idle units, behind a ram) the way a player's right-click does, and
  measures how many get through and how fast, failed orders, units left
  stuck and the deepest overlap. The fast tests run every scenario and
  check overlap, determinism and that each crowd gets through as its bar
  asks (everyone through, no order failed, within a time; the ram, whose
  path does not fit its slot, and one unit whose formation slot lies on
  the cliff are the only exceptions), and run them again on twelve
  jittered layouts (every unit moved up to 0.15 m from its spot), where
  the units not through and the orders failed, summed per scenario,
  must stay under a measured ceiling: a change that only suits the
  default layout fails there. The results are sensitive beyond the
  layout too: one more entity in the world (a resource is one) renumbers
  the units and changes them, so the bench passes its layout as a
  parameter and the fast tests read no environment. The table:
  `cargo test -p aetherfall --test chokepoints chokepoint_report -- --ignored --nocapture`
  (`AF_CHOKE_JITTER=<seed>` runs it on that jittered layout).

## Client model

`Update`: `ClientSet::Pointer` (UI hit-test) -> `Input` -> `Ui` -> `Render`.
Shared client state lives in af_core resources: `CursorWorld`, `InputMode`,
`CameraRig`, `DragSelect`, `LocalPlayer`, `ShowPaths` and `RoutesShown`
(below), and the `Selected` marker; click markers are client entities
(`ClickPing` with a `ClickMark`).

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
  `theme::panel_node` and `theme::button` keep the padding a caller asks
  for (8 px and 10 by 5 px only fill in an unset one), which keeps content
  off the frames' metal: the game-over and leave panels pad 24 px, the
  pause banner and the network bar 28 by 10. Every panel and button
  wears it: the menu's Match and Map panels, its Start (train), Spectate
  (command) and Multiplayer (research), the multiplayer screens' panels
  and actions (going ahead: train; Sign up: command; the rest plain) and
  the network bar. Only the water scale's segments stay flat swatches
  (`theme::flat_button`), and the multiplayer text fields keep their flat
  inset look.
* **Plaques:** the button textures carry the hotkey and count plaques in
  their top corners. Only the command card's slots (`SkinPlaques`) show
  them; every other button wears a copy made once the textures load, its
  top 40 texture rows replaced by its bottom ones mirrored
  (`skin::without_plaques`), so "Start" or "Resume" has no empty gems.
* **Command card:** a slot wears its action's family (a build page button
  and its buildings the page's colours, else the action group's), with
  its hotkey and queued count centred on the frame's two corner plaques
  (a count of 10 or more at 11 px, so its two digits stay on the plaque).
  `refresh_card` writes `SkinLook` (usable / soon / not, armed) every
  frame; `skin_looks` turns look, hover and press into the button's tint
  and glow (an armed button pulses gold; the selection's stance, the
  Paths toggle while on and the chosen faction glow steadily, the
  faction in its colour). `theme::shade_buttons` and the card's flat
  colours leave skinned nodes alone.
* **Presses** (`af_ui::fx`, real time, so they play while paused): the
  command card records each press, by mouse or hotkey, in `card_input`.
  Build, train, research and upgrade presses squash the button to 0.9 and
  spring it back past full size (a damped spring, still by 0.35 s), flash
  it gold, sweep a shine across it and throw a ring and sparks from its
  centre (on a root node over the HUD, at most 64 particles alive); other
  orders get a lighter squash and flash; a refused press shakes and
  flashes red. A press that turns the card's page (a build page, More,
  Back) lays the pressed slot out anew, so it plays on the new page
  instead: its buttons appear squashed and spring out with a faint flash,
  a column at a time. Plain buttons squash when clicked. Scale and shake are
  `UiTransform`s, so nothing is laid out again, and nothing runs for a
  button at rest.
* **Checking it:** `cargo run -p af_ui --example ui_preview [-- hud ...]
  --screenshot shot.png`, or the game with `--script` to catch a press
  part-way (`"3 key Q; 3.05 shot a.png; 3.15 shot b.png"`; with
  `--speed 0.5` the shots fall on more frames, as script times are sim
  seconds). The floating hover tip follows the stand-in mouse of a
  scripted run (`--hover LABEL --cursor X,Y`).

### Click markers and the placement slam

**Click markers** (`ClickPing` contract). Every order the local player
gives spawns a client entity (never a unit) carrying a `ClickPing` (point,
colour, life) and a `ClickMark` (`af_core::ext::input`: the `OrderMark`
kind, the entity it targets, whether it was shift-queued), both made by
`ClickMark::ping`. af_input writes them for right-click `Smart` (the kind
guessed from what is pointed at and what the selection can do, as the sim
resolves it per unit; cargo carried to an own drop site that takes it
marks `Gather`; buildings alone mark a rally), targeted commands,
placements (`Place`: the snapped footprint and facing; a wall line marks
its last segment) and af_ui for minimap commands. af_render
(`markers`) dresses each ping with its parts as children, plays it in real
time (also while paused), follows a targeted entity while the local team
sees it, keeps at most 16 alive and despawns the ping when its life runs
out; a ping without a `ClickMark` is drawn as a plain point marker in its
colour. Its arrows come in along the screen's diagonals, each at the same
angle to the camera's line of sight (so none is seen tail-on from the
pitched camera). A marker lies on the drawn world (`Ground::surface`:
the ground, the water surface, a mountain's rock). Its materials
(`Shade::Marker`) are unlit, so the fog the land's shaders darken the
world with never dims them (a click into fog or unexplored black shows
in full), and sort after everything else blended (only a unit route's
stroke, a gizmo, shows across one). af_input and af_map also despawn leftover
pings when a match ends.

**Placement slam.** A foundation of the local player
(`Added<UnderConstruction>`, seen in an `Update` system, never inside the
sim tick) gets a placement slam, a top-level `RenderScene` effect.

### Unit routes

* **Toggle:** `ShowPaths { on, held }` (`af_core::ext::input`): af_input
  initialises it (never reset between matches: a preference), flips `on`
  with P (with a notice) and sets `held` while Shift is down; af_ui's
  "Paths" button under Menu flips `on` and shows it (while on, a gold
  label and the skin's steady gold glow).
* **Overlay** (`af_render::paths`, `RenderStep::Overlay`): `plan_paths`
  turns the local player's own selected units' `Orders`, `PathState`,
  `MoveGoal` and `AttackTarget` into polylines (`plan_route`, pure): the
  current leg from the unit through `waypoints[next..]` to the live goal
  (straight while `Pending`, for flyers and for a goal the waypoints are
  not for yet; stopped with a cross where a partial path ends), then each
  queued order chained on to its target, coloured by order in its click
  marker's colour (`LegKind::mark`, `OrderMark::color`: one palette, so
  a queued order's marker and its leg agree; following a unit is a move,
  as its click marks it). Entity targets of other teams are placed where
  the local team believes they are (the rally flags'
  `believed_rally_point`), a chase after one out of sight is drawn
  straight to where it was seen (with no cross: whether its path is
  blocked depends on where it really is), and enemy units' routes are
  never planned. The
  pathfinder sees the whole map, so a path's turns trace what it goes
  round: only the turns the team has in sight (`in_sight`: the stretch to
  the turn and the cells round it) are drawn, and from the first one it
  does not, the leg runs straight to its goal (nor is a partial path's
  end marked there). Point targets are drawn where the orders put them:
  af_behavior's intake nudges a slot off obstacles with the whole map in
  view too, so one in the fog can stand up to `SLOT_SEARCH` (6 m) off the
  click, as the unit's own walk there shows. The chain stops at an order
  that never ends (a patrol, a hold). A selected unit whose current order
  is a patrol always shows its loop, the leg it walks lit (read off
  `MoveGoal.target`, kept while it fights); with the paths shown, the
  unit's current leg carries that highlight and the loop keeps a plain
  track there. `draw_paths` draws it with Bevy gizmos (registered only
  with `GizmoPlugin`, so headless apps plan without drawing): a few
  pixels wide at any zoom, no entities (nothing is inserted on units),
  terrain-following points on `Ground::surface`, marching dashes and
  chevrons on real time (they run while paused), lifted off the ground
  by a small depth bias (`DEPTH_BIAS`: Bevy scales it with the distance,
  5 to 43 cm over the zoom range, so units standing on a route still
  hide it), and a faint always-on-top copy so routes behind buildings
  still read. Gizmos draw before every blended mesh and write depth (the
  copy at the near plane), so a route's stroke shows over a click
  marker it crosses. Only the
  stretches on screen are cut into pieces (`View`, the camera's view of
  the ground, a little widened), with at most so many dashes and
  chevrons to a stretch, so the cost follows what is on screen, not how
  long the routes are. At most `MAX_ROUTES` (64) units with a route are
  drawn (an idle or holding unit has none), those nearest the centre of
  the view; `RoutesShown` (`af_core::ext::render`)
  tells af_ui, whose Paths button then reads "64 of N routes shown".

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
