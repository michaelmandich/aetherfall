# Static models: buildings and resource nodes

A scripted Blender build turns any model of a building or a resource node
(a Meshy GLB, an FBX, an OBJ...) into what the game loads: one cleaned,
reduced, normalised GLB per variant, a manifest per asset and the registry
`assets/visuals/static.ron`. The contract with the engine (formats, fit
modes, engine behaviour) is
[docs/art/static-models.md](../../docs/art/static-models.md); what to model
first is [docs/art/model-queue.md](../../docs/art/model-queue.md).

Nothing in `assets/models/static/` or `assets/visuals/static.ron` is edited
by hand: change the source or `assets.toml` and rebuild.

## Dropping a model in

1. Save the model under the game's id (the ids are in `assets/data/`):

   | file | what |
   |---|---|
   | `source/buildings/<id>.glb` | a building: `house.glb`, `town_center.glb` |
   | `source/nodes/<id>.glb` | a node: `stone_outcrop.glb`, `berry_bush.glb` |
   | `source/nodes/<id>_a.glb`, `_b`, `_c`... | variants: the engine picks one per tree or rock |
   | `source/nodes/<id>_stump.glb` | what a felled tree leaves (optional) |

   `.glb`, `.gltf`, `.fbx` and `.obj` all work. Polygon count, texture size,
   rigs and animations do not matter. The model should stand on the ground
   facing +Z (its door or front toward +Z), roughly in the proportions of
   the game's box (`size` in `assets/data`); the build does the rest.
2. Build (from the repository root):

   ```bash
   "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b -P art/static/build_static.py
   ```

3. Look at the contact sheet it prints the path of (see "Contact sheets").
   If the model faces the wrong way, the banner is in a bad spot or the
   reduction hurts, add a line to `assets.toml` and build again.
4. Run the game. Anything with a model shows it; everything else keeps its
   boxes.

## Commands

```bash
BLENDER="C:/Program Files/Blender Foundation/Blender 4.4/blender.exe"
"$BLENDER" -b -P art/static/build_static.py                          # every source -> assets/
"$BLENDER" -b -P art/static/build_static.py -- --only pine,house     # just these (the registry keeps the rest)
"$BLENDER" -b -P art/static/build_static.py -- --selftest            # check the pipeline itself
"$BLENDER" -b -P art/static/placeholders.py -- --build               # the placeholder set (git-ignored)
"$BLENDER" -b -P art/static/placeholders.py -- --fixtures            # the engine's test models
```

`build_static.py` options (after the `--`):

| option | effect |
|---|---|
| (none) | Builds every source in `source/` into `assets/models/static/` and rewrites `assets/visuals/static.ron`. |
| `--only a,b` | Builds only these ids. The registry still lists every asset already built. |
| `--source DIR` | Sources from `DIR/buildings/` and `DIR/nodes/` instead. |
| `--out-root DIR` | Models to `DIR/<buildings\|nodes>/<id>/` instead. |
| `--registry PATH` | The registry to write. |
| `--asset-root DIR` | The folder asset paths are relative to (default `assets/`); `--out-root` must be inside it. |
| `--settings PATH` | Another `assets.toml`. |
| `--sheets DIR` | Contact sheets go to `DIR/<kind>_<id>.png` (default: `aetherfall-static-sheets` in the system temp folder). |
| `--no-sheets` | No contact sheets. |
| `--prune` | Delete built assets whose source is gone (and folders left empty). Without it the build lists them and leaves them (and their registry lines) alone. |
| `--data DIR` | The game content, for ids and boxes (default `assets/data`). |
| `--selftest` | Builds synthetic sources with every awkward feature into a temporary folder and checks the results (see "Self-test"). |

A build takes a few seconds per model plus about a second per contact
sheet; the 22 MB rigged Meshy peasant takes 3 s. Each asset is built in a
temporary folder and moved into place only when every part has passed its
checks, so a failed build leaves the previous output as it was; the run
then exits non-zero and names the failures.

## What the build does

Per variant (and stump), in order:

1. **Find** (`lib/discover.py`, `lib/content.py`). The file name is split
   against the game's ids (`stone_outcrop_b` is variant `b` of
   `stone_outcrop`). An id the game does not have gets a warning and is
   built anyway, with the defaults of its folder.
2. **Import** (`lib/scene.py`). Animations are dropped and every bone goes
   to its rest pose; the skin deformation and shape keys are baked into the
   mesh (a rigged model becomes a statue). Every mesh is joined into one,
   with its transforms applied; cameras, lights, empties and loose vertices
   go.
3. **Normalise.** Turned by `yaw`, then based on y = 0 and centred on x/z
   by its bounding box. A variant named in `cut_plate` then loses the faces
   lying wholly in that bottom fraction of its height (a ground plate or
   mound), and is based and centred again.
4. **Materials** (`lib/materials.py`). Per material: the base colour
   texture is kept and every other texture (normal, metallic-roughness,
   occlusion, emissive) goes; emission off; metallic 0 (sources often leave
   glTF's default of 1: dark metal); no transmission, coat, sheen or IOR
   extensions. An alpha the texture really uses becomes a cutout (glTF
   `MASK`, cutoff 0.5), blended alpha included, and an alpha from a
   separate opacity map is copied into the base colour texture; an alpha
   that is opaque everywhere is dropped. Textures are downscaled to the
   asset's size, multiplied by the variant's `tint` if it has one, and
   re-encoded (`lib/png.py`: a 256-colour palette for opaque textures, RGBA
   with a binary alpha and bled edges for cutouts). Materials that draw the
   same are merged.
5. **Reduce** (`lib/reduce.py`). Over budget only: coincident vertices are
   welded (exports split the surface at every UV seam and hard edge, and
   collapse would shred it), then Blender's Decimate (Collapse) runs to the
   budget, with the triangles the banner needs kept free. Collapse never
   merges a non-manifold edge (three or more faces on it: fronds sharing a
   stem, planks meeting a rail), so on models with many it stalls above
   the budget (Meshy's farm at 10,414 of 5,940, its palms at twice theirs);
   then those edges are split and collapse runs again. This is only a
   fallback: the split sheets reduce independently and can open hairline
   cracks (a budget above the stall avoids it, as the farm's does). The
   result gets
   smooth normals with edges over 42° kept sharp, and is based and centred
   again.
6. **Banner** (`lib/banner.py`, buildings). A pole (`Banner_pole`, dark
   wood) and a flag and finial (`Team`, plain white), node `Team_banner`,
   about 40 triangles. It is sized in game metres from the box height (a 3
   m house gets a 1.55 m pole and a 0.96 x 0.62 m flag) and divided by the
   scale the engine will fit the model with, per axis, so it comes out
   undistorted whatever the source's size. It stands on the anchor from
   `assets.toml`, or on the first high point (at least 45 % of the height)
   found by rays dropped from the top front-left corner (-X, +Z: the left
   of the default camera's view) toward the centre; the flag flies toward
   +X and faces the front. The build prints where it went.
7. **Export and check** (`lib/gltf.py`). One GLB per part, then read back:
   no skin or animation, no node transforms, nothing emissive or metallic,
   no blending, no texture but base colour, PNG textures within the size,
   `Team_banner` with the `Team` material exactly when there should be one,
   and the triangle count within the budget.
8. **Manifest and registry** (`lib/manifest.py`). `min`/`max` come from the
   exported file: the model's bounds without the banner (so a building
   fits its box by its walls and roof, and the flag may rise above the
   box); `triangles` counts everything in the file, the banner included.
   The registry is rebuilt from every manifest under `--out-root`.
9. **Contact sheet** (`lib/sheets.py`).

A source with no suffix (`house.glb`) becomes variant `a`
(`buildings/house/a.glb`); `house.glb` and `house_a.glb` together are an
error.

## Settings and budgets

`assets.toml` documents every key and default. In short:

| | fit | random yaw | triangles | texture |
|---|---|---|---|---|
| trees (nodes that give wood: pine, oak, palm) | Height | yes | 3000 | 512 px |
| other nodes | Footprint | yes | 2000 | 512 px |
| buildings | Box | no | 6000 | 1024 px |
| `town_center`, `citadel` | Box | no | 10000 | 1024 px |
| stumps | (the tree's) | (the tree's) | 300 (`stump_triangles`) | 512 px |

Keys: `fit`, `triangles`, `stump_triangles`, `random_yaw`, `yaw`, `banner`
(`false`, or `[x, y, z]`: the pole's foot in metres in the model frame after
yaw and centring, before fitting), `texture`, `cut_plate` (`{ c = 0.08 }`:
cut away the ground plate or mound under variant `c`, the faces wholly in
that bottom fraction of its height) and `tint` (`{ a = [0.7, 0.75, 0.55] }`:
multiply variant `a`'s base colour textures, in sRGB values, to bring it in
line with the other variants). Unknown keys and wrong types stop the
build.

Why these budgets: about 900 trees are on a map at once, so a tree at 3000
triangles is 2.7 M triangles for the forests; buildings are a few dozen.
Trees started at 1500, but Meshy's canopies (about 8000 triangles as made)
then broke into big flat facets that merged a forest into one sheet; at
3000 each tree reads on its own. Measured on the six-player map (2,107
buildings and nodes drawn as models, no vsync, RTX 4070 Ti): 2.3 ms a
frame at 1500 and at 3000 alike, zoomed in or out. Placeholder sizes for
comparison: pines 143-191 triangles, oaks 383-559, a house 816, the town
centre 1172.

## Contact sheets

One PNG per asset, rendered from the GLB files just written (what the
engine loads), each part scaled the way the engine fits it to the game's
box (the stump at its tree's scale), the team material tinted blue:

| row | view |
|---|---|
| 1 | front, orthographic, from +Z; the label gives triangles and size |
| 2 | side, orthographic, from +X |
| 3 | three-quarter, from the front-left, above |
| 4 | the game's camera at its start zoom (66 m) at the game's pixel scale on a 1080p screen: what a player sees (a big building that does not fit the tile is zoomed out, and labelled so) |
| strip | "in game": nodes as a patch of nine (the variants mixed, turned at random when `random_yaw` is on), buildings as three copies at 40 %, 75 % and 100 % build progress tinted red, blue and green |

The magenta wireframe is the game's box. Trees overflow it sideways (the
box is the trunk); buildings fill it; rocks fill its width. The strip is
enlarged 2x or 3x when that still fits, or zoomed out when even 1x does not;
its label says which.

## Placeholders and fixtures

`placeholders.py` models low-poly stand-ins for the commonly seen things
(model queue batches 1-3 and the base buildings) and writes them as
sources to `placeholders/source/` (committed: 1.3 MB). `--build` builds
them like real models into `assets/models/_placeholders/` with the registry
`assets/visuals-placeholders/static.ron` (both git-ignored); run the game
with `--visuals visuals-placeholders` to see them. The placeholder build
reads its own settings, `placeholders/assets.toml` (banner spots for the
two camps), and never `assets.toml`, so settings made for real models do
not move the stand-ins; the fixtures are built with no settings at all.

| asset | variants | triangles | size (m, before fitting) |
|---|---|---|---|
| pine | a, b, c + stump | 167, 191, 143; stump 61 | 2.3-3.3 wide, 6.0 tall |
| oak | a, b, c + stump | 559, 479, 383; stump 75 | 3.1-3.8 wide, 4.6-5.1 tall |
| palm | a, b + stump | 444, 476; stump 56 | 4.5-4.6 wide, 5.3 tall |
| berry_bush | a | 1240 | 1.66 x 0.97 x 1.59 |
| stone_outcrop | a | 400 | 2.89 x 1.52 x 2.57 |
| copper_vein | a | 840 | 2.45 x 1.38 x 2.20 |
| iron_vein | a | 840 | 2.73 x 1.41 x 2.16 |
| silicon_deposit (quartz sand) | a | 504 | 2.54 x 0.96 x 2.35 |
| aether_crystal | a | 226 | 1.68 x 2.64 x 1.35 |
| aether_vent | a | 478 | 3.08 x 0.53 x 2.96 |
| adamant_meteor | a | 784 | 3.23 x 2.26 x 3.14 |
| house | a | 816 (banner included) | 3.96 x 3.06 x 3.96 |
| town_center | a | 1172 | 9.00 x 6.08 x 9.00 |
| farm | a | 674 | 6.02 x 0.38 x 6.02 |
| lumber_camp | a | 1242 | 3.92 x 2.75 x 3.90 |
| mining_camp | a | 1402 | 3.95 x 2.96 x 4.04 |

Each recipe is a function in `placeholders.py`, modelled at the game's box
size with the helpers in `lib/shapes.py` (boxes, prisms, lumpy rocks,
beams, roofs). Every model has one material whose base colour is a small
palette texture (4 x 4 texels per colour, nearest filtering), so it draws
in one part like a Meshy model and goes through the same texture path.
Randomness is seeded per recipe: the sources rebuild identically.

`--fixtures` builds the tiny versions in the same file (`fx_*`: a pine with
two variants and a stump, a rock, a house with its banner; 4-8 KB each)
into `crates/af_render/tests/fixtures/static/`, a miniature asset folder
for the engine's tests (see the README there).

## Self-test

`build_static.py -- --selftest` writes synthetic sources to a temporary
folder and builds them for real: a skinned, animated column posed away from
rest, with an emissive, metallic material whose 2048 px texture has blended
alpha, a duplicate of that material on a second mesh, flat shading (a
split surface) and 11,640 triangles; an OBJ stump with loose vertices; an
unknown id; an FBX house facing -Z (`yaw = 180`); a banner at an anchor and
`banner = false`; 40 tufts of blades sharing their stems, where collapse
stalls (`triangles = 1500`, so the split fallback must run); and two
columns standing on a plate, off the origin, with `cut_plate` naming one
of them; the oak also gets `tint = { a = [0.5, 1.0, 1.0] }`. It then
checks 56 things in the output (rest pose, budget, welding, merged
materials, cutout alpha with an RGBA texture, 512 px, the tint, no
emission, front turned to +Z, banner placement, the stalled collapse
reaching its budget, the plate cut from one variant only and the model
based and centred again, bounds, manifests and registry parse), plus 2 on
an FBX-style material whose alpha comes from a separate opacity map, and
fails if any is wrong. Run it after changing anything in `lib/`.

## Rehearsal: the Meshy peasant as a statue

The heaviest real source in the repository, `art/units/peasant/source/
peasant_meshy.glb` (22 MB, skinned and animated, 10,319 triangles in
18,149 split vertices, a 4096 px texture, an emissive material), built as
node `statue`:

| | source | built |
|---|---|---|
| triangles | 10,319 | 1,997 (budget 2000) |
| vertices before reduction | 18,149 | welded to 5,128 |
| texture | 4096 x 4096 | 512 x 512, 207 kB palette PNG |
| file | 22 MB | 358 kB |
| pose | rest (arms down), animation dropped | the same, 1.65 m, base on y = 0 |
| emission | yes | none |

Without the weld step the same reduction shreds it into slivers; with it,
the silhouette, face and clothing hold up in a close-up.

## Layout

```
art/static/
  build_static.py      entry point (headless)
  placeholders.py      placeholder and fixture models (recipes) and their build
  assets.toml          per-asset settings (every key and default documented)
  source/              YOUR MODELS: buildings/<id>.glb, nodes/<id>[_<variant>].glb, nodes/<id>_stump.glb
  placeholders/source/ the placeholder sources (generated by placeholders.py, committed)
  placeholders/assets.toml  settings for the placeholder build only
  lib/
    content.py         game ids, boxes and resources from assets/data (a small RON reader)
    discover.py        file names -> assets, variants, stumps
    settings.py        defaults and assets.toml
    scene.py           import, statue (rest pose, baked skin), join, normalise
    materials.py       material cleanup, alpha cutout, texture downscale, merging
    png.py             compact PNG writer (palette or RGBA cutout)
    reduce.py          weld and decimate to the budget
    banner.py          the team banner
    gltf.py            GLB export, read-back and checks
    manifest.py        manifests and the registry
    build.py           one asset, end to end
    sheets.py          contact sheets
    shapes.py          low-poly modelling helpers (placeholders)
    selftest.py        the pipeline's self-test
    util.py            quiet() for Blender's console output
```

## Troubleshooting

| symptom | cause and fix |
|---|---|
| `! 'x' is not a building id` | The file name is not a game id (typo, or a node in `buildings/`). The model is built, but the game never asks for it. Rename the file. |
| The model faces sideways or backwards | The source does not face +Z. Set `yaw` (source faces -Z: 180; +X: -90; -X: 90). |
| Squashed or stretched building | `Box` fits each axis to the box separately. A source in very different proportions from the box is distorted; remodel it closer, or use `fit = "Footprint"`. |
| Sunk, floating or too small in game | Something below or around the model counts in its bounds: a ground plate, a shadow plane, a stray part. Delete it from the source. |
| A tree or rock too small | `Footprint` matches the wider horizontal side: wide branches make the whole thing smaller. Use `fit = "Height"`. |
| Blotchy texture after reduction | Raise `triangles` (or `texture`) for that id and compare on the sheet. |
| Leaves solid, or holes in leaves | Alpha is cut at 0.5. A texture whose alpha is 0.5 or more everywhere counts as opaque; one with soft edges loses the faint parts. |
| Banner inside a wall or floating | Copy the position the build printed into `assets.toml` as `banner = [x, y, z]`, adjust, rebuild. `banner = false` removes it. |
| `GLB check failed` | The export did not come out as the engine needs (the message says what). The previous output is untouched. |
| `assets.toml [...] unknown setting` | A typo; the message lists the valid keys. |
| An asset stays after its source was deleted | The build lists it; delete the folder or build with `--prune`. |
| Two files for the same variant | `house.glb` and `house_a.glb` are both variant `a`: rename one. |
