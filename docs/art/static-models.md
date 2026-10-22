# Static models: buildings, resource nodes and decor

Contract between the static art pipeline (`art/static/`, headless Blender)
and the engine (`af_render`). Static models have no skeleton and no
animation; the engine moves them (construction rising, a felled tree
falling, ore shrinking). Units use the animated pipeline instead
(`docs/art/peasant.md`). Decor (landscape scenery: mountain pieces, crater
rims, reeds, sea cliffs, bridges...) goes through the same build but is no
game entity: the engine places it over the land (see "Decor").

## Dropping a model in (the whole workflow)

1. Save the model (`.glb`, `.gltf`, `.fbx` or `.obj`; Meshy's GLB is fine,
   polygon count and texture size do not matter) as
   - `art/static/source/buildings/<building id>.glb`, or
   - `art/static/source/nodes/<node id>.glb`, or
   - `art/static/source/decor/<decor id>_<variant>.glb`,
   using the game's id (`town_center`, `house`, `pine`, `stone_outcrop`...;
   decor: `mountain_peak`, `reeds`... in `art/static/lib/settings.py`
   `DECOR`). Variants: `pine_a.glb`, `pine_b.glb`, `pine_c.glb` (the engine
   picks one per tree). A felled-tree stump: `pine_stump.glb` (optional).
2. Run the build:
   ```bash
   "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b -P art/static/build_static.py
   ```
3. Run the game. Anything with a model shows it; everything else keeps its
   boxes.

Optional per-asset settings go in `art/static/assets.toml` (defaults below);
most models need none.

## What the build does

- Validates the id against the game content (warns on unknown ids).
- Imports, applies any armature pose and drops animations (a rigged model
  works as a statue), joins meshes.
- Normalises: base on y = 0, centred on x/z, front facing +Z (a
  `yaw` setting turns a model authored facing another way).
- Reduces polygons to a triangle budget (defaults: trees 3000, other nodes
  2000, buildings 6000, `town_center` and `citadel` 10000; decor per id:
  mountain pieces 4000-8000, meteor-site pieces 3500-5000, small scenery
  600-2000). Where collapse stalls on non-manifold edges (fronds sharing a
  stem, planks meeting a rail), those edges are split and collapse runs
  again.
- Strips emissive, downsizes textures (nodes and small decor 512 px,
  buildings and big decor 1024 px), keeps alpha-cutout leaves working.
- Decor: a long piece whose longer side lies along Z is turned a quarter
  to lie along X.
- Buildings: adds a team banner (a pole and flag, node `Team_banner`,
  material `Team`, plain white) at the top front-left corner unless
  `banner = false` or an anchor is given; the engine tints it with the
  owner's faction colour.
- Writes, per asset:
  - `assets/models/static/<buildings|nodes|decor>/<id>/<variant>.glb` (and
    `stump.glb` when given),
  - `assets/models/static/<buildings|nodes|decor>/<id>/<id>.model.ron`, the
    manifest,
- and the generated registry `assets/visuals/static.ron`.
- Renders a contact sheet per asset for review (scratch folder, not the repo).

## Settings (`art/static/assets.toml`)

```toml
[pine]
fit = "Height"        # Height | Footprint | Box
triangles = 3000
random_yaw = true     # nodes: each one turned at random (visual only)

[town_center]
triangles = 10000
banner = [3.2, 6.5, 3.2]   # anchor (m, before fitting), or false
yaw = 180                  # the source faces -Z
```

`cut_plate = { c = 0.08 }` cuts away a ground plate or mound a generator
put under variant `c`: the faces lying wholly in the bottom 8 % of its
height (`"*"` for every variant; at most 0.25). The build stops if a
listed variant has nothing there. `tint = { a = [0.7, 0.75, 0.55] }`
multiplies variant `a`'s base colour textures (sRGB values) to bring it in
line with the other variants.

Defaults: trees (`pine`, `oak`, `palm`) `fit = "Height"` and
`random_yaw = true`; other nodes `fit = "Footprint"` and `random_yaw = true`;
buildings `fit = "Box"`, `random_yaw = false`; decor per id
(`settings.DECOR`: fit, `size`, triangles, texture), `random_yaw = true`.
`size = [w, h, d]` (decor only) sets the size the game draws it at.

## Fit (how the engine sizes a model to the game's box)

- `Box`: scaled per axis to exactly the box (buildings: the model matches
  the collision footprint, so nothing pokes out or leaves gaps).
- `Footprint`: one scale so the model's larger horizontal side matches the
  box's; height follows (rocks, bushes, crystals).
- `Height`: one scale so the model's height matches the box height (trees:
  the box is the trunk, the canopy may spread past it).

## Manifest (`<id>.model.ron`, written by the build)

```ron
(
    kind: Node,                       // Node | Building | Decor
    id: "pine",
    fit: Height,
    random_yaw: true,
    variants: [
        (model: "models/static/nodes/pine/a.glb", min: (-1.4, 0.0, -1.3), max: (1.4, 6.3, 1.3), triangles: 1480),
        (model: "models/static/nodes/pine/b.glb", min: (-1.2, 0.0, -1.2), max: (1.2, 5.8, 1.2), triangles: 1502),
    ],
    stump: Some((model: "models/static/nodes/pine/stump.glb", min: (...), max: (...), triangles: 120)),
    banner: false,                    // buildings: true when Team_banner exists
)
```

A decor manifest has `kind: Decor` and, after `random_yaw`, its drawn size
`size: (13.000, 8.500, 13.000)` (w, h, d metres; without it the engine uses
its id's default, `DecorKind::size`). Buildings' and nodes' manifests are
unchanged.

Registry (`assets/visuals/*.ron`, all files merged; `static.ron` is
generated, `units.ron` is hand-written; every section is optional):

```ron
(
    units: { "peasant": "models/peasant/peasant.model.ron" },
    buildings: { "house": "models/static/buildings/house/house.model.ron" },
    nodes: { "pine": "models/static/nodes/pine/pine.model.ron" },
    decor: { "reeds": "models/static/decor/reeds/reeds.model.ron" },
)
```

## Engine behaviour

- A building or node with a ready model draws it instead of its boxes;
  missing or failed models keep the boxes. Selection outlines, health and
  progress bars and fog work as before.
- Variant: picked per entity from a hash of its position (stable between
  runs, the same for every player, independent of the simulation).
- `random_yaw`: the model turns by a position hash (visual only; the box and
  collisions do not turn).
- Ground: the model stands on the drawn terrain. Sand is drawn 5 cm above
  the ground units walk on, so a model whose footprint overlaps any sand
  stands on the sand, and its paving or field is never buried.
- Construction: the model rises out of the ground with build progress (it is
  drawn sunk by `(1 - progress) x height`, `height` being how high its top
  stands, banner included; the opaque terrain hides the buried part), over
  the foundation outline. A flat model with a tall banner (the farm) shows
  little but the flag until late in the build.
- Upgrades (house -> manor, wall tiers, tower tiers): the model swaps when
  the building's type changes.
- Depletion: ore, crystal and bush models shrink toward 70 % as the node
  empties. A tree whose wood runs out falls over (about 1.2 s, away from the
  worker who felled it), sinks, and leaves its stump (when the manifest has
  one), which sinks after about 90 s.
- Enemy buildings in fog are drawn from memory, as before.
- Team banner: the `Team` material is tinted with the owner's colour.

Engine details (`af_render::registry`, `af_render::statics`):

- Registry files merge in file-name order: an id listed twice keeps its
  first entry and a file that does not parse is skipped (both warn). The
  folder given to `--visuals` is merged the same way, then laid on top
  (its entries win).
- A model is drawn once all its files (variants and stump) have finished
  loading; a variant that failed gives way to the next one that loaded,
  and a model with none keeps its type's boxes.
- Materials are drawn as the build wrote them (base colour texture,
  roughness, alpha cutout), with the boxes' low reflectance (1 % head-on,
  `af_render::assets::BOX_REFLECTANCE`) so models and boxes sit in the
  same light; baked emission is switched off.
- Fitting uses the manifest's `min`/`max`. The engine warns when the
  meshes (the `Team_banner` node left out, as the bounds leave it out)
  stray from them by more than 5 cm plus 5 % of the model's size.
- A model stands where its entity does: buildings and nodes only stand on
  buildable ground, which the terrain draws flat at the ground units walk
  on (sand and dunes are paint, not a raised top), so picking and aiming
  are unchanged. Fog memory and felled trees stand on the same spot.
- Construction buries the whole model at 0 %, the banner included, so the
  flag rises first; the foundation's pad and
  scaffold stay. A foundation drawn from fog memory rises as far as it was
  seen.
- Weapon boxes and shield shells stay boxes on a modelled building, and
  placement ghosts stay boxes.
- Trees (a stump in the manifest, or `pine`, `oak`, `palm`) keep their
  size as they are gathered. A tree falls only where the local team sees
  it (else it is simply gone, as boxes are); at most 32 fall at once and
  200 stumps stand (the oldest sink early).
- Dead buildings and nodes remembered in the fog are drawn with their
  models too.

## Decor

Scenery models that dress the land (`af_render::terrain::decor`). Never a
game entity: nothing in the simulation knows them, units walk on the ground
as before, and where each stands is a pure function of the map (the drawn
terrain and `MapFeatures`), hashed from positions and the seed, so every
player sees the same land. Each placement rule draws from whatever variants
of its model exist; a model that is missing (or fails to load) places
nothing, so with no decor at all the land looks as it did before.

| id (variants) | where the engine puts it |
|---|---|
| `mountain_peak` (a, b snowy; c bare; d green flanks) | on the summits of the ranges and massifs (the height field's local maxima over mountain cells, the map's and the skirt's), as wide as the rock round the summit allows, rising 0.9-2.4 m above the field's own crest, sunk into it, never above 10.4 m on the map (flyers fly at 12 m); snow on those topping 9.3 m, bare rock from 7.6 m, green flanks below |
| `mountain_ridge` (a; b snow dust; c grassy) | along each range's spine (`MapFeatures::mountains`) every 10 m between the peaks, laid along the spine, as wide as the range there, just above its crest; shorter or slimmer where it bends or narrows |
| `mountain_foothill` (a, b, c) | where a range tapers out (carrying its line on) and along the feet of the ranges, lying along the rock's edge; on mountain cells only |
| `crater_rim` (a short, b long, c crowned) | arcs along the rims of the Crater, Shattered, Lake and Scar sites, their steep inner face (front, +Z) toward the site's centre, each wholly on rim cells (never in a gap, never over the floor or the ground outside), 1.4-2.7 m above the rim (climbers cross it) |
| `aether_monolith` | three to five standing round the procedural Starspire on its impassable footing (they join it rather than replace it: the heart stays the game's own crystal cluster) |
| `star_core` (a sunk, b split) | half buried at the Starspire's foot; half sunk in the Crater Lake's deep ring |
| `impact_scar`, `meteor_shard` (a, b) | sparse, on the rim and footing cells the arcs left free (the rims' torn ends by the gaps, the Starspire's footing) |
| `reeds` (a, b, c) | in marsh and along the shallows of rivers and lakes (not the sea), over the map and the skirt; a share of the procedural reeds stays between them |
| `river_rocks` (a, b, c) | in fords and river shallows, just breaking the surface |
| `lily_pads` | on still lake water 1-6 m from the shore, gathered in coves |
| `bramble` (a, b, c) | over the thickets (a share of the procedural brambles stays under them) |
| `fern` (a, b), `shrub` (a gorse, b hazel), `mossy_boulder` (a, b, c) | ferns in thickets and marsh; shrubs on highland and thicket edges; boulders at the feet of mountains; all three at the skirt's wood edges |
| `sea_stack` (a, b) | in the sea 6-18 m off the shore (a few on the map, more in the skirt) |
| `beach_rocks` | in the sea's shallows off the beach (never on the sand, which is buildable), and at the skirt's shore |
| `sea_cliff` (a, b, c) | along the skirt's coast, its sheer face (+Z) to the sea |
| `shipwreck` | in the skirt's sea near its coast; at most one far out in the map's own sea |
| `driftwood` | on the skirt's shore |
| `stone_bridge`, `wooden_footbridge` | over each river where it runs on across the skirt, 14-30 m beyond the edge, spanning bank to bank (stone where the river is wide) |
| `standing_stones` (a, b), `ruined_watchtower`, `wayside_shrine` | on the skirt's open meadows, well apart (a shrine also by a bridge's end) |
| `fallen_log` (a, b) | at the skirt's wood edges |
| `backdrop_mountains` (a, b), `backdrop_forest` (a, b) | in the skirt's outer ring along every side not at sea, facing the map, fading into the haze |

The rules every placement keeps (tested on every kind of generated map,
`crates/af_render/tests/decor.rs`, through `decor::violations`): no
footprint covers buildable ground; where units walk only low pieces stand
(reeds, river rocks, lily pads, brambles, ferns, shrubs, beach rocks, and
on the climbable rims the rim pieces); the landmarks (stones, ruins,
shrines, logs, driftwood, bridges, cliffs, backdrops) stand only beyond the
map's edge; nothing rises above 10.4 m on the map; at most 9000 pieces.
Nothing may look gatherable (the style guide's node colour code).

Conventions for the models: the front (+Z) is a rim arc's inner face, a
cliff's sea face, a backdrop's side toward the map; long pieces lie along
X (the build turns them; `yaw = 180` turns one that faces the wrong way);
no ground plate.

Engine:

- The land is built once the decor models have finished loading (at most
  5 s after the map exists; then without the stragglers), once per match.
- Each placed model's parts are entities sharing the model's meshes, so
  Bevy batches them; nothing runs per frame for them. Big pieces cast
  shadows; small scenery casts none and is not drawn from further than
  340 m (past every play zoom; only the dev overview leaves it out).
- Each of a model's materials gets one fog-aware copy per match
  (`DecorMaterial`: its `StandardMaterial`, texture and cutout kept, with
  the land's fog of war: unexplored near black, out of sight darkened,
  greyed off the map; spectators see all).
- Cost, measured on a Huge map (721 m, 8 players, seas: 1,031 pieces), with
  stand-ins at their triangle budgets (`placeholders.py --heavy`), the game
  paused, no vsync, median frame (RTX 4070 Ti, the machine otherwise
  busy): 4.7 ms without decor, 5.0-5.3 ms with it at the start zoom; 4.5
  against 4.9 ms over the main site; 4.8 against 4.85 ms at the dev
  overview (the small scenery culled). Planning and spawning it take 38 ms
  of the land's 295 ms build.

## Placeholders

`art/static/placeholders.py` writes simple stand-in sources (tree variants
and stump, rock, crystal, house, town centre, farm, and every decor model)
to `art/static/placeholders/source/` (decor in `decor/`, not committed);
building those with
`--source art/static/placeholders/source --out-root assets/models/_placeholders
--registry assets/visuals-placeholders/static.ron` gives a full test set.
Both output folders are git-ignored. Run the game with
`--visuals visuals-placeholders` to see them (the folder is loaded on top of
`assets/visuals/`). Tiny placeholder GLBs for the engine's tests live in
`crates/af_render/tests/fixtures/static/`.
