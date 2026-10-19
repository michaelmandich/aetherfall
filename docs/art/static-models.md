# Static models: buildings and resource nodes

Contract between the static art pipeline (`art/static/`, headless Blender)
and the engine (`af_render`). Static models have no skeleton and no
animation; the engine moves them (construction rising, a felled tree
falling, ore shrinking). Units use the animated pipeline instead
(`docs/art/peasant.md`).

## Dropping a model in (the whole workflow)

1. Save the model (`.glb`, `.gltf`, `.fbx` or `.obj`; Meshy's GLB is fine,
   polygon count and texture size do not matter) as
   - `art/static/source/buildings/<building id>.glb`, or
   - `art/static/source/nodes/<node id>.glb`,
   using the game's id (`town_center`, `house`, `pine`, `stone_outcrop`...).
   Variants: `pine_a.glb`, `pine_b.glb`, `pine_c.glb` (the engine picks one
   per tree). A felled-tree stump: `pine_stump.glb` (optional).
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
  2000, buildings 6000, `town_center` and `citadel` 10000). Where collapse
  stalls on non-manifold edges (fronds sharing a stem, planks meeting a
  rail), those edges are split and collapse runs again.
- Strips emissive, downsizes textures (nodes 512 px, buildings 1024 px),
  keeps alpha-cutout leaves working.
- Buildings: adds a team banner (a pole and flag, node `Team_banner`,
  material `Team`, plain white) at the top front-left corner unless
  `banner = false` or an anchor is given; the engine tints it with the
  owner's faction colour.
- Writes, per asset:
  - `assets/models/static/<buildings|nodes>/<id>/<variant>.glb` (and
    `stump.glb` when given),
  - `assets/models/static/<buildings|nodes>/<id>/<id>.model.ron`, the manifest,
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
buildings `fit = "Box"`, `random_yaw = false`.

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
    kind: Node,                       // Node | Building
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

Registry (`assets/visuals/*.ron`, all files merged; `static.ron` is
generated, `units.ron` is hand-written):

```ron
(
    units: { "peasant": "models/peasant/peasant.model.ron" },
    buildings: { "house": "models/static/buildings/house/house.model.ron" },
    nodes: { "pine": "models/static/nodes/pine/pine.model.ron" },
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
- Construction: the model rises out of the ground with build progress (it is
  drawn sunk by `(1 - progress) x height`; the opaque terrain hides the
  buried part), over the foundation outline.
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
- Construction buries the whole model at 0 %, the banner included, so the
  flag rises first; the foundation's pad and scaffold stay. A foundation
  drawn from fog memory rises as far as it was seen.
- Weapon boxes and shield shells stay boxes on a modelled building, and
  placement ghosts stay boxes.
- Trees (a stump in the manifest, or `pine`, `oak`, `palm`) keep their
  size as they are gathered. A tree falls only where the local team sees
  it (else it is simply gone, as boxes are); at most 32 fall at once and
  200 stumps stand (the oldest sink early).
- Dead buildings and nodes remembered in the fog are drawn with their
  models too.

## Placeholders

`art/static/placeholders.py` writes simple stand-in sources (tree variants
and stump, rock, crystal, house, town centre, farm) to
`art/static/placeholders/source/`; building those with
`--source art/static/placeholders/source --out-root assets/models/_placeholders
--registry assets/visuals-placeholders/static.ron` gives a full test set.
Both output folders are git-ignored. Run the game with
`--visuals visuals-placeholders` to see them (the folder is loaded on top of
`assets/visuals/`). Tiny placeholder GLBs for the engine's tests live in
`crates/af_render/tests/fixtures/static/`.
