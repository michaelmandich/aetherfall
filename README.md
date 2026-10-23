# Aetherfall

A real-time strategy game in Rust (Bevy 0.19) in the spirit of Age of Empires /
Age of Mythology, StarCraft and Warhammer, where a medieval world slowly turns
medieval+tech after a star falls and its **Aether** crystals unlock machines,
computers, shields and beams.

**Everything is a box** for now: the map, terrain, buildings, units, weapons,
projectiles. Boxes are placed freely (any position, any angle) but never
overlap. Real graphics come one model at a time and only touch the renderer
crate: the peasant is animated, and any building or resource node takes a
model dropped into `art/static/source/` (see Models below).

## Play

```bash
cargo run -p aetherfall
```

That opens the skirmish menu: pick one of six factions, the number of AI
opponents, teams and fog, then Start. The menu also works from the
keyboard: Left/Right pick the faction, + and - the number of opponents, T
toggles teams, F fog, Enter starts and S spectates. Shortcuts:

```bash
cargo run -p aetherfall -- --quick --faction ironhold --players 3
```

```bash
cargo run -p aetherfall -- --quick --spectate --players 6 --reveal --speed 4
```

`cargo run -p aetherfall -- --help` lists every option. The first build
compiles Bevy (about 5 minutes); later builds are incremental. Build output
goes to `target/` by default. For faster linking on Windows, or to move the
build folder (for example out of a OneDrive- or Dropbox-synced folder), copy
`.cargo/config.example.toml` to `.cargo/config.toml` and edit it; that file
is per machine and git ignores it. On Windows keep the clone (or the build
folder) at a short path: Bevy's build paths are long, and the MSVC linker
fails with LNK1104 once a path passes 260 characters.

In a match the top bar shows your resources, population (used / capacity:
build houses for more), age, clock and game speed. Hover any command card
button for what it costs, how long it takes, what it does, what Shift does
with it and, when it is greyed out, why. While a click is armed (a building
to place, a target to pick) a hint by the cursor says what the click will do
and how to back out; a placement ghost is green where the building fits,
amber where it fits but you cannot pay for it yet, and red with an X where it
cannot go.

Every order you give is marked where it lands, in the order's colour (green
move, red attack, orange attack-move, gold gather, amber build, blue repair,
cyan patrol, pale gold rally), in fog too: four arrows swoop in onto the spot,
or press in on a ring or frame around the unit, building or resource it
targets, which follows it while you can see it. Shift-queued orders leave a
dot (or a faint ring or frame on what they target) that lingers, so a chain of
them reads. A building you place stamps down: its footprint flashes, a
shockwave and dust roll out and chips fly.

### Controls

| Action | Input |
|---|---|
| Pan camera | arrow keys, screen edges, middle-mouse drag; left click or drag on the minimap |
| Zoom | mouse wheel |
| Select | left click; drag a box (units first, else buildings); double-click or Ctrl+click = every one of that type on screen; Shift+click adds or removes |
| Narrow a multi-selection | click a type in the selection panel (Shift+click drops it) |
| Command (move / attack / gather / build / repair / rally) | right click, also on the minimap (Shift queues) |
| Command card buttons | click, or the hotkey printed on the button: Q W E R T / A S D F G / Z X C V B; Shift+train queues 5 |
| Fixed buttons on every building | C the next age-up, V upgrade the building, B rally point (what it trains and researches fills the card from Q) |
| Worker build pages | Z Economy, X Military, C Defence; B or Esc goes back |
| Targeted command (Move, Attack, Patrol, Gather, Repair, Rally) | its button, then left click the target (Move, Attack, Patrol and Rally also on the minimap); Shift keeps the command armed; right click or Esc cancels |
| Patrol route | Patrol (R), then click the far end: the unit walks there and back; Shift+click more points to make one loop it walks round and round (orders queued after a patrol are never reached) |
| Unit paths | P, or the Paths button under Menu: show where the selected units are going (the path being walked, solid with arrows; queued orders chained on, dashed; coloured by order) and say so; hold Shift to see them meanwhile; a selected patrolling unit always shows its loop, the leg it is on lit; with more than 64 of the selected units going somewhere (idle ones have no route), those nearest the centre of the view, and the button says how many |
| Place a building | its button, then left click (Shift keeps placing); R rotates 45°, Shift+R 15°; right click or Esc cancels |
| Place a wall | its button, click the start, click the end (Shift chains the next line from there); segments that do not fit are skipped |
| Cancel a queued item | click it in the selection panel's queue |
| Control groups | Ctrl+0..9 set, Shift+0..9 add, 0..9 recall, press twice to centre; or click the group bar |
| Idle worker | Period or F1, or the Idle workers button |
| Select the main hall | H (again: the next hall), then Q trains a worker |
| Select the whole army | F2 (every unit that is not a worker) |
| Centre on selection / main hall | Space or Home |
| Delete selected | Delete |
| Pause / game speed | Pause or F3 / + and - (0.5x to 8x) |
| Spectating: whose resources to show | Tab / Shift+Tab, or the arrows in the top bar |
| Back out / deselect | Esc, one step per press: a drag, then a pending command or placement, then an open build page, then the selection |
| Leave the match / back to the menu | Menu button (top right) or F10; after a match ends it reopens the results screen |

## The game

* **Five ages:** Hearth, Banner, Starfall (the star falls; aether tech),
  Engine (industry, computers, rifles, mechs), Ascendant (shields, beams,
  hover craft, plasma, titans).
* **Eight resources:** food, wood, stone, copper, iron, silicon, plus the
  made-up **aether** (mid tech) and **adamant** (late tech). Starting areas hold
  the basics; the central **Starfall Crater** holds aether crystals, aether
  vents (claim them with an extractor for passive income) and adamant
  meteors. Every age-up past Banner needs a contested resource.
* **Weapons of every kind**, all boxes: sword swings, spear thrusts, hammer
  smashes, arrows and bullets, arcing catapult and mortar shells, piercing
  cannon and railgun shots, laser beams, dropped bombs, homing rockets,
  flamethrower sprays, shockwave novas and chain lightning.
* **Buildings that upgrade** (palisade → stone → reinforced → force walls;
  watchtower → … → plasma turret; town centre → citadel), production queues
  with costs and build times, squadrons and single units, small troops and
  giants.
* **Terrain that matters:** rivers with fords, marsh, bramble thickets, dunes
  and cliffs. Some factions cross terrain others must walk around.
* **Winning:** a player is out when they have no units left and no complete
  building that matters (walls, gates, plain farms and unfinished
  foundations do not count). A player who loses every building that trains
  units or researches (drop sites like camps and mills do not count) has all
  their remaining buildings revealed to everyone until they finish a new one.
  The last team standing wins.

### Factions

| Faction | Harvest bonus | Terrain trait | Plays like |
|---|---|---|---|
| The Ironhold Thanes | stone, iron, copper | infantry climb cliffs | slow, tough, best walls and artillery, no cavalry or flyers |
| The Sylvan Wardens | wood, food | pass bramble thickets | archers, beasts, living constructs, wisp swarms |
| The Sunspire Caliphate | silicon, aether | full speed on sand | fast riders, shielded infantry, hover skiffs, sand engines |
| The Tidecrown Republic | food, copper | marines wade deep water and marsh | crossbows, greek fire, amphibious tanks, railguns |
| The Khaganate of the Steppe | food | cavalry crosses marsh and dunes | horse archers, hover-bikes, gunship swarms, weak walls |
| The Synod of the Fallen Star | aether, adamant | none: earliest and best flyers | paladins, healers, shields, angels, an archon |

Full design: [docs/design/game-design.md](docs/design/game-design.md) and
one doc per faction in [docs/design/factions/](docs/design/factions/).

## Content is data

All units, buildings, weapons, techs, terrain and factions live in RON files
under [assets/data/](assets/data/). Edit them and validate:

```bash
cargo run -p af_data --example check -- --summary
```

The checker reports unknown references, bad ranges, missing drop sites or
age-ups, and prints every faction's roster per age.

## Models

Models replace boxes one type at a time. Anything without a model is drawn
as boxes, and the box stays every entity's collision shape.

The source models the art builds read (`art/static/source/`,
`art/units/source/`, about 1 GB) are stored with
[Git LFS](https://git-lfs.com): run `git lfs install` once before cloning,
or they arrive as small pointer files. The built models in `assets/` are
ordinary files, so the game runs without them.

### Buildings and resource nodes: dropping a model in

1. **Save the model** under the game's id (the ids are in `assets/data/`):
   `art/static/source/buildings/<id>.glb` (`house.glb`, `town_center.glb`)
   or `art/static/source/nodes/<id>.glb` (`stone_outcrop.glb`,
   `berry_bush.glb`). Variants are `pine_a.glb`, `pine_b.glb`, ... (each
   tree or rock placed gets one); a felled tree's stump is
   `pine_stump.glb`. Meshy's GLB is fine, as are `.gltf`, `.fbx` and
   `.obj`; polygon count, texture size and rigs do not matter. It should
   stand on the ground with its front toward +Z.
2. **Build** (from the repository root):

   ```bash
   "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b -P art/static/build_static.py
   ```

   This reduces, cleans and normalises the model into
   `assets/models/static/`, adds a team banner to buildings, rewrites the
   registry `assets/visuals/static.ron`, and prints the path of a contact
   sheet to look at.
3. **Run the game** (`cargo run -p aetherfall`). That type now draws its
   model, fitted to its box, with the variant and turn picked by position
   and the banner in its owner's colour. Foundations rise out of the
   ground as they are built, ore and rocks shrink as they are gathered,
   and felled trees fall and leave a stump.

A model that faces the wrong way, a banner in a bad spot or a reduction
that hurts is fixed with a line in `art/static/assets.toml`, then a
rebuild. To take a model out, delete its source and build with
`-- --prune`. The pipeline and its options are in
[art/static/README.md](art/static/README.md), the engine contract in
[docs/art/static-models.md](docs/art/static-models.md), and what to model
first in [docs/art/model-queue.md](docs/art/model-queue.md).

### Units

The **peasant** (every faction's worker) is an animated 3D model: 23 clips
(idle, walking, carrying, each kind of gathering, building, repairing,
fighting, dying...), the tool or load in its hands, and a sash in its
owner's colour. Every other unit is still boxes until its model is built.

* One Blender script builds any unit's model from its Meshy source in
  `art/units/source/` (the peasant's is in `art/units/peasant/source/`),
  configured by `art/units/<id>/unit.py`:

  ```bash
  "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b -P art/units/build_unit.py -- --unit peasant
  ```

  Rigged characters (humanoids) share the peasant's clips (idle, walk,
  death...) and add their own attack; their weapons and shields are boxy
  props always in hand (`Gear_*` nodes). Models without a skeleton
  (vehicles, siege, mounts, beasts, flyers) are built as rigid models with
  a team pennant or band and no clips. The spearman (spear, attack and
  combat-idle clips), the battering ram and the scout rider are
  configured and wait for their Meshy sources.
* To add a model for another unit, follow "Adding a unit" in
  [art/units/README.md](art/units/README.md) and the contract in
  [docs/art/peasant.md](docs/art/peasant.md) (clip names, props, manifest),
  then add the unit to `assets/visuals/units.ron`.

### The registry and checking models

* The visual registry, every `*.ron` in `assets/visuals/`, lists what is
  drawn with a model: `units.ron` (hand-written) the units, `static.ron`
  (written by the static build) the buildings and resource nodes, each
  with its manifest. Anything missing from it, or whose model fails to
  load, is drawn as boxes. Nothing in `assets/models/` or
  `assets/visuals/static.ron` is edited by hand.
* `cargo run -p aetherfall -- --no-models` draws everything as boxes.
* Stand-in models for every common building and node (not shipped, and
  git-ignored) show the engine side without real art:

  ```bash
  "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b -P art/static/placeholders.py -- --build
  cargo run -p aetherfall -- --visuals visuals-placeholders
  ```

## Code

See [docs/architecture.md](docs/architecture.md). In short: `af_data`
(content schema), `af_geom` (boxes, navigation grid, pathfinding), `af_core`
(the shared ECS contract), one crate per subsystem (map, movement, behavior,
combat, economy, fog, ai, input, ui, render) and the `aetherfall` binary.
The simulation is a deterministic 30 Hz fixed-step model, separate from
rendering. [docs/bevy-0.19-notes.md](docs/bevy-0.19-notes.md) is a verified
Bevy 0.19 API cheat sheet.

```bash
cargo test --workspace
```

```bash
cargo run -p aetherfall -- --headless --players 2 --minutes 15 --report-every 60
```
