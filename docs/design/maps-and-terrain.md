# Maps and terrain: generation options and the terrain look

Status: design for the map and graphics overhaul (October 2026). It extends
`docs/design/game-design.md` sections 4.2 (map layout) and 7 (terrain); where
they disagree, this document wins for maps and terrain drawing.

Goals, from the owner:

* every map has a river that is not on the very edge and can run in any
  direction, and up to two rivers;
* several map sizes;
* different types of meteor site;
* natural, truly impassable obstacles such as mountains;
* a sliding scale of water, from no extra water through lakes to some map
  edges being ocean;
* a lush, vibrant, temperate European look in the spirit of Civilization VI:
  lush greens, deep blues, grey rocky mountains with variation, bright blue
  rivers with white blended in.

## 1. Map options

`af_core::ext::map::MapOptions` (a resource next to `MatchSetup`; the menu and
the CLI insert it, af_map reads it in `OnEnter(Playing)`, a missing resource
means `MapOptions::default()`). Every "random" choice is resolved from the
match seed, and the resolved options are published in `MapFeatures` (section
5) so the HUD and the renderer can show them.

| option | values (default first) | menu | CLI |
|---|---|---|---|
| size | `Medium`, `Small`, `Large`, `Huge` | Map size | `--map small\|medium\|large\|huge` |
| water | `Lakes` (2), `Dry` (0), `Ponds` (1), `Coast` (3), `Seas` (4) | Water (a 0-4 slider) | `--water 0..4` |
| rivers | `Random` (1 or 2), `One`, `Two` | Rivers | `--rivers 1\|2\|random` |
| site | `Random`, `Crater`, `Shattered`, `Lake`, `Scar`, `Spire` | Meteor site | `--site <name>` |

`MatchSetup::map_size` (metres) still overrides the size when it is not 0.

### 1.1 Sizes

The land is a square of side `L = base(players) x factor`, rounded to a whole
multiple of 8 m and clamped to 160-1024 m. `base` is today's
`map_size_for(players)` (224 m for two players up to 384 m for eight).

| size | factor | 2 players | 4 players | 8 players |
|---|---|---|---|---|
| Small | 0.8 | 176 m | 232 m | 304 m |
| Medium | 1.0 | 224 m | 288 m | 384 m |
| Large | 1.3 | 288 m | 376 m | 496 m |
| Huge | 1.6 | 360 m | 464 m | 616 m |

Ocean edges (section 2.3) add their bands outside this land square, so the
playable land is the same size at every water level.

## 2. Terrain types

Three new terrain types are appended to `assets/data/core/terrain.ron`
(append only: the numeric ids are list positions):

| terrain | name | height m | build | Foot | Mounted | Wheeled | Tracked | Hover | Amphibious | Climber |
|---|---|---|---|---|---|---|---|---|---|---|
| `mountain` | Mountains | 9 (visual) | no | - | - | - | - | - | - | - |
| `ford` | Ford | -0.2 | no | x0.7 | x0.7 | x0.6 | x0.7 | x0.9 | x0.9 | |
| `scorched` | Starfall Scar | 0 | yes | x1 | x1 | x1 | x1 | x1 | | |
| `ocean` | Open Sea | -0.6 | no | - | - | - | - | x0.8 | x0.7 | - |

(Flyers cross everything. `ocean` plays like `deep_water`; it is its own
type so lakes and rivers stay small bodies, marsh never lines a coast and the
renderer can draw swell and depth.)

* **Mountains** are the true obstacle: only flyers cross them. They never
  cover a start area, a corridor, a contested spot or a meteor site, and the
  connectivity pass (section 4) guarantees every start still reaches every
  meteor site for every ground locomotion. They are drawn as tall rock peaks,
  at most 10.5 m high so flyers (12 m and up) never pass through them.
* **Fords** are where every ground unit, wheeled siege included, crosses a
  river. Shallow river banks stay impassable to wheeled machines, so a
  river that crosses the whole map is crossed by wheeled units only at fords.
* **Starfall Scar** is the burnt, glassy floor of a meteor site. It plays
  like grass; it exists so the sites read at a glance (dark basalt with
  glowing aether veins).
* The climbable **Rocky Highland** (`cliff`: ridges and highlands, Ironhold
  infantry walk over them) stays, now drawn as a rocky plateau.

## 2.1 Rivers

Every map has one or two rivers (`MapOptions::rivers`).

* A river runs from a **source** to a **mouth**. A source is a point on a land
  edge of the map (never an ocean edge). A mouth is another land edge, an
  ocean coast, or, for a second river, a point on the first river (a
  tributary; 40 % of second rivers).
* It may run in any direction. It is routed with A* on a 4 m grid over the
  land: start areas (plus 4 m), meteor sites, contested spots and start
  aether spots are closed; cells within 12 % of the map side from a land edge
  cost extra except near the river's own ends, so a river never hugs an
  edge; the start corridors are open (they become fords); value noise adds
  cost so the course wanders; a second river pays extra near the first.
* The course is smoothed (Chaikin), resampled every 0.75 m and given a gentle
  meander (2-5 m, wavelength 35-60 m) wherever that keeps it off closed
  cells. Its deep core is 2-3 m half-wide, widening by a third toward the
  mouth, with 1.5-2.5 m of shallow banks.
* **Fords** sit at most 28-36 m apart along the river and wherever it crosses
  a start's corridor; each ford is `ford` terrain across the full width for
  `deep + 3` m either side of its centre.
* Marsh grows near rivers on non-reserved grass as today (less at `Dry`).

### 2.2 Lakes

| water | lakes |
|---|---|
| Dry | none |
| Ponds | 1-2 small (radius 6-9 m) |
| Lakes | 2-4 (radius 8-15 m) |
| Coast | 1-2 (radius 8-13 m) |
| Seas | 1-3 (radius 8-13 m) |

Lakes are noisy blobs (deep middle, shallow shore), placed with
`Gen::feature_spot` so they keep clear of everything reserved and of rivers.

### 2.3 Coasts

`Coast` turns one map edge into ocean; `Seas` turns two (adjacent or
opposite, from the seed). An ocean band is about 17 % of `L` deep and lies
beyond the land square, so the map grows (one band or two adjacent bands:
side `L + B`; two opposite bands: side `L + 2B`; the map stays square, so
the land rectangle can be wider than `L`). The coastline wobbles with noise
into bays and headlands, but only outward from the land square, never into
it, so every start area keeps its full template on land. From the shore out:
2-4 m of sand beach on land (not on reserved cells), 3-8 m of shallows, then
`ocean` out to the map edge. Rivers may end in the ocean. The start ring, the
meteor sites and the wilds are laid out on the land square, whose centre is
the layout centre. Like Age of Empires coastal maps, some starts have the sea
at their back and some do not.

### 2.4 Mountains

Mountain ranges are spines (a random walk of 2-4 gently turning segments,
30-90 m long) painted as `mountain` out to a half-width of 5-10 m that varies
along the spine with noise, so peaks, saddles and spurs read naturally.

| size | ranges | single massifs | edge massifs |
|---|---|---|---|
| Small | 1 | 0-1 | 0-1 |
| Medium | 1-2 | 1 | 1-2 |
| Large | 2-3 | 1-2 | 1-2 |
| Huge | 3-4 | 1-2 | 2-3 |

* Ranges and massifs go where `Gen::room` allows (shrinking before giving
  up), between the meteor sites and the land edge, never on reserved cells.
  Long ranges (over 60 m) keep one 8-10 m pass.
* Edge massifs sit against a land edge or in a corner (partly off the map),
  framing the map and using its dead corners.
* Highland ridges (`cliff`) and dunes still appear (1-2 and 1-2), as do
  forests with thickets.

## 3. Meteor sites

The star fell in one of five ways. The main site is always at the layout
centre, so every start is the same distance from it. Every kind keeps the
design budget: 8-12 aether crystals, 1-3 vents and 2-4 adamant meteors in the
site(s), plus the contested spots between neighbours. Site floors are
`scorched`.

| kind | name | what it is |
|---|---|---|
| `Crater` | The Starfall Crater | today's crater: a cliff rim (11 % of `L`) with a ramp gap toward every start and 1-2 extra, everything on the floor |
| `Shattered` | The Shattered Fall | a smaller central crater (7 %) with half the nodes; the rest fell at the contested spots, each now a small rimmed crater (4.5 %, two gaps) holding its usual vent, meteor and veins plus 2 crystals |
| `Lake` | The Crater Lake | the crater (12 %) is flooded: deep water inside the rim with a shallow inner shore, a central island of scar holding the vents and meteors, crystals on the shore and the island, and a ford causeway from every rim gap to the island; hover and amphibious units cross anywhere |
| `Scar` | The Skyfall Scar | the star skidded: three overlapping basins in a line through the centre (main 8 %, two 6 % at plus and minus 13 % of `L`), walled by cliff along both sides with gaps toward the starts, crystals strewn along the furrow; for two players the line runs across the axis between them |
| `Spire` | The Starspire | no rim: an impassable crystal spire (a `mountain` massif of radius 4 % drawn as a giant aether crystal) at the centre, with the nodes on an open ring of scar around it; armies meet from every side |

`MapInfo::crater_center` and `crater_radius` describe the main site; every
site is in `MapFeatures::sites`.

## 4. Connectivity and fairness

* The start corridors (start to main site, 4 m half-width) stay reserved: no
  river except as a ford, no lake, mountain, thicket, marsh or tree.
* After generation, `connect.rs` checks that every start front, every
  contested spot and every meteor site is reachable from the main site for
  each ground class at its widest unit: Foot and Mounted at radius 1.0
  (3 x 3 cells), Wheeled at radius 2.5 (5 x 5: the war ordu, the sand engine)
  and Tracked at radius 3.0 (7 x 7: the titan); and every highland top on
  foot. Fords, crater gaps, causeways and mountain passes are therefore at
  least 8 m wide. Anything cut off gets a route carved as a last resort:
  thicket becomes grass, water a ford, mountain a pass; ocean is never carved
  and no template node is ever deleted. The pass returns a diagnostics list
  that tests assert is empty.
* Every player gets the same start template (rotated), as today. Rivers,
  lakes, coasts and mountains are not mirrored: like Age of Empires random
  maps, the wilds differ, the starts and the main site do not.

## 5. What af_map publishes

Besides today's resources, af_map inserts `af_core::ext::map::MapFeatures`
in `OnEnter(Playing)` (and removes it on exit):

* `options`: the resolved `MapOptions` (no `Random` left);
* `land`: the land rectangle (the map minus ocean bands);
* `ocean_edges`: which map edges are ocean (west, east, south, north);
* `sites`: every meteor site (kind, centre, radius), main site first;
* `rivers`: each river's centreline (every 0.75 m), deep half-width per point
  and its ford centres (distance along the line), source first;
* `mountains`: each range's spine and half-widths (for drawing).

`GeneratedMap` carries the same `MapFeatures`, so tests and the headless
harness see it.

## 6. The look

The terrain stops being boxes. Everything here is af_render only; the
simulation is still 2D and nothing in it reads heights.

### 6.1 Ground

* A **height field** with a vertex every metre (cell corners), built once per
  match into chunks of 32 x 32 m, each one mesh with one terrain material.
  Every walkable cell is drawn at exactly `ground_height` (units, buildings
  and markers stand where they always did); banks, beds and slopes happen
  inside unwalkable cells:
  * water beds fall with distance from the shore: fords -0.32 m, shallows
    -0.5 to -0.8 m, deep water -1.4 to -2.5 m, ocean to -4 m;
  * mountains rise with distance into the range, shaped by ridged noise, to
    7-10.5 m peaks;
  * rocky highlands are a 0.6 m plateau with steep rocky edges;
  * marsh is boggy ground with shallow dips where water shows through.
* A **terrain material** (`ExtendedMaterial<StandardMaterial, _>`, so PBR
  light and shadows stay) reads a per-cell terrain-id texture, blends the
  four nearest cells with noise-warped weights (organic borders, no
  staircase), and paints each terrain procedurally in world space:
  * grass: lush green with lighter meadow and darker patches and fine
    speckles of wildflowers; richer and darker near water;
  * sand: warm pale sand with ripples; beaches lighter;
  * rock (highlands, mountains): grey with variation and strata, darker
    crevices, green at the foot, snow on gentle slopes above about 7 m;
  * marsh: dark wet olive with puddles;
  * scar: dark basalt with slowly pulsing aether veins (emissive);
  * thicket ground: dark leaf litter (the brambles are props, 6.3).
* Palette (sRGB): meadow #5E9A3A with shade #3F7A2E, sunlit #7DB348 and
  dry accents #A5A957; forest floor #3B5A2A; dirt #8A6A48; beach sand
  #D8C38E, dune sand #E3D3A2; marsh #556B3A with dark water #2F4A4A; rock
  #A4A6A8 / #7E8184 / #55585C with lichen #B9BE8C; snow #EEF3F8 shading to
  #C9D6E6; scar #3A3436 / #241F22 with aether violet #BF73FF in the veins.
  Grass and water stay a step below the team colours in saturation (Sylvan
  green, Tide cyan and Ironhold blue must still read on them).
* Fog of war is drawn by the terrain, water and prop shaders from
  `FogTexture` (linear filtered): unexplored near black, explored darkened
  and desaturated. The flat overlay plane goes, since it cannot sit over
  mountains.
* Around the map a 60-80 m **skirt** of out-of-bounds land continues the
  terrain (meadow, forest-coloured ground and mountains; ocean beyond ocean
  edges) darkened and desaturated so it reads as off the map. The frame
  boxes go.

### 6.2 Water

One water surface at `y = -0.12` over every water and ford cell (with a cell
of overlap under the banks), drawn with a water material:

* colour by depth: clear turquoise over fords and shallows (#6FC0EA) to deep
  blue in lakes (#1F5BA8) and the ocean (#173E7A to #0F2A57); rivers a
  brighter azure (#3C9BE0) with white (#F2F8FB) blended in;
* animated normals (scrolling noise), sun glint and Fresnel;
* white foam at shorelines (by depth), in streaks that flow downstream on
  rivers (direction from `MapFeatures::rivers`) and as broken white water
  over fords;
* gentle swell and surf lines on the ocean.

### 6.3 Props

Props are client-side decoration placed from a hash of the position (stable,
the same for every player, never touching the simulation), merged into one
mesh per chunk and material, and only on unbuildable ground so a building is
never placed on top of one:

* brambles: dense dark thorny bushes over every thicket cell (no berries);
* reeds and rushes along shores and in marsh;
* scree and rock spurs grown out of mountain feet and highland edges;
* charred, ore-less meteor fragments in the meteor sites, and the giant
  pale crystal spire for `Spire`.

A prop must never look like something a worker can gather (the style
guide's node colour code): no loose grey boulder piles (stone), no red
berries (berry bush), no bright violet crystals (aether), no gold-veined
charred rock (adamant), no scorched rings (vent), no free-standing trees
(every tree is a wood node).

### 6.4 Light

A brighter, warmer afternoon: a warm sun, sky-tinted ambient light, a light
hazy blue sky colour with distance fog matched to it, a touch of extra
saturation in the colour grading and a soft bloom so aether glows.

## 7. Testing

* af_map: for 2-8 players, every size, water level, river count and site
  kind and several seeds: the reachability above for every ground
  locomotion; the requested river count, each river's interior well away
  from the edge, fords no more than 36 m apart; starts and start discs on
  clear grass; ocean present exactly at `Coast`/`Seas`; mountains present on
  Medium and larger; node budgets per site kind; determinism.
* af_render: walkable cells drawn at `ground_height`; mountains under 10.5 m;
  every water bed below the water surface; chunks cover the map once.
