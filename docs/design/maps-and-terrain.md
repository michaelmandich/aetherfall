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

`MatchSetup::map_size` (metres) still overrides the size when it is not 0: it
is then the land side, rounded to whole metres and clamped to 160-1024 m,
and the map plays as (feature counts) and is published as the size whose
land side for the player count is nearest (`af_map::size_for_side`).

The random choices (river count, site kind) come from a stream of their own
(`af_map::resolve_map_options(options, seed)`), so they never depend on
anything else the generator draws; the ocean edges are drawn from the map
stream.

### 1.1 Sizes

The land is a square of side `L = base(players) x factor`, rounded to a whole
multiple of 8 m and clamped to 160-1024 m (`MapSize::land_side`). `base` is
`map_size_for(players)` (224 m for two players up to 384 m for eight).
Starts sit on a ring of 36 % of `L` round its centre, but at least 30 m
inside the land square's edges (so on Small lands and 160 m custom maps the
ring is a little tighter).

| size | factor | 2 players | 4 players | 8 players |
|---|---|---|---|---|
| Small | 0.8 | 176 m | 232 m | 304 m |
| Medium | 1.0 | 224 m | 288 m | 384 m |
| Large | 1.3 | 288 m | 376 m | 496 m |
| Huge | 1.6 | 360 m | 464 m | 616 m |

Ocean edges (section 2.3) add their bands outside this land square, so the
playable land is the same size at every water level.

## 2. Terrain types

Four new terrain types are appended to `assets/data/core/terrain.ron`
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
  ocean coast, or, for a second river, a point on the middle two thirds of
  the first river (a tributary; 40 % of second rivers, and any second river
  that finds no way across the land on its own). Rivers never cross.
* It may run in any direction: its ends are where a line in a random
  direction, passing near the land's centre, meets the map edge. It is routed
  with A* on a 4 m grid: start areas (plus 4 m), meteor sites, contested
  spots, start aether spots, every start's own resources and its front yard
  (its corridor within the start area) are closed, with the river's own
  reach (deep core and banks) on top; cells within 20 % of the land side
  from a land edge cost more the nearer the edge, and the outermost 12 m are
  closed, except within 20 m of the river's own ends; the start corridors
  beyond the start areas are open (they become fords); value noise adds cost
  so the course wanders; a course keeps a few metres more than it must from
  what is closed; a second river pays extra near the first. At most 20 % of
  the course's middle (beyond 20 % of the land side from either end) may lie
  within 12 % of the land side from a land edge.
* **Crowded maps.** When the starts, spots and site close off the whole
  interior (2-4 players on Small and Medium lands, 5-8 players on Small),
  the router relaxes in steps: the outskirts of start areas open at a price
  (never a start's clear disc, resources or front yard), then the margins
  shrink, then the river narrows to two thirds of its width and may cut a
  corner of the map. If no step finds a course across the interior, the
  course furthest from the edges is used.
* The course is smoothed (Chaikin), resampled every 0.75 m and given a gentle
  meander (2-5 m, wavelength 35-60 m) wherever that keeps it off closed
  cells. Its deep core is 2-3 m half-wide (1.2-1.8 m narrowed), widening by a
  third toward the mouth, with 1.5-2.5 m of shallow banks.
* **Fords** sit at most 28-36 m apart along the river, at most about 16 m
  from where it enters the map and 26 m from where it leaves it, and wherever
  it crosses a start's corridor; a tributary is a ford for its last stretch
  into the river it joins. Each ford is `ford` terrain across the full width
  for `deep + 6` m either side of its centre (at least 12 m of ford where the
  river is deepest); its banks are reserved (4 m beyond the water), and a
  strip as wide as the ford and 10 m deep beyond each bank is kept free of
  trees, thicket and marsh, so no mountain, wood or marsh closes its ways
  out.
* Marsh grows near rivers and lakes on non-reserved grass (less at `Dry` and
  `Ponds`), never along the sea, by a mountain or on a lane (section 2.4).

### 2.2 Lakes

| water | lakes |
|---|---|
| Dry | none |
| Ponds | 1-2 small (radius 6-9 m) |
| Lakes | 2-4 (radius 8-15 m) |
| Coast | 1-2 (radius 8-13 m) |
| Seas | 1-3 (radius 8-13 m) |

Lands bigger than Medium for their player count get more lakes (the count
times the square root of the area ratio) and bigger ones (the radius times
its fourth root). Lakes are noisy blobs (deep middle, shallow shore), placed
with `Gen::feature_spot` so they keep clear of everything reserved, of
rivers and (by 10 m) of mountains.

### 2.3 Coasts

`Coast` turns one map edge into ocean; `Seas` turns two (adjacent or
opposite, from the seed). An ocean band is about 17 % of `L` deep and lies
beyond the land square, so the map grows (one band or two adjacent bands:
side `L + B`; two opposite bands: side `L + 2B`; the map stays square, so
the land rectangle can be wider than `L`; the land square is centred in it).
The coastline wobbles with noise into bays and headlands between 2 % and
62 % of the band beyond the land square, only outward from the land square,
never into it, and it is pushed further out wherever reserved ground (a
start area reaching past the square on a small land) would come within its
beach, so every start area keeps its full template on land. From the shore
out: 2-4 m of sand beach on land (not on reserved cells), 3-8 m of
shallows, then `ocean` out to the map edge. Rivers may end in the ocean. The
start ring, the meteor sites and the wilds are laid out on the land square,
whose centre is the layout centre. Like Age of Empires coastal maps, some
starts have the sea at their back and some do not.

### 2.4 Mountains

Mountains are the map's big obstacle: long grey ranges that read as ranges,
big massifs, and massifs framing the edges and filling the dead corners.
They take about 2-3 % of the land on Small maps, about 5 % on Medium, 5.5 %
on Large and 7 % on Huge.

| size | ranges | big single massifs | edge and corner massifs |
|---|---|---|---|
| Small | 1 | 0-1 (one map in three) | 1-2 |
| Medium | 2-3 | 0-1 | 2-3 |
| Large | 2-3 | 1 | 2-3 |
| Huge | 3-4 | 1-2 | 3-4 |

* **Ranges.** A range is a spine of rock 30-56 % of the land side long
  (22-40 % on Small): about 85-160 m on a Medium land for four, 115-210 m
  on Large and 140-260 m on Huge (up to 345 m on Huge for eight). Of eight
  candidate spines, each grown from the roomiest of six random spots
  between the main site and the land edge, the one with the most rock is
  painted, a third more welcome when it stands between neighbouring starts;
  a crowded map gets it shorter and thinner, round by round (down to 24 m),
  before leaving it out. A spine starts along the heading with the longest
  straight run of room (most ranges prefer to run outward from the middle,
  so between neighbours they make flanks and lanes) and grows both ways
  2 m at a time along a gentle bend (up to about 60 degrees end to end) and
  a slow wave, steering round whatever is in the way.
* **Shape.** The half-width is 8.5-10.5 m (7-9 m on Small), swelling to
  peaks of about 14 m and dipping to saddles of about 6 m along the spine
  (two waves, 28-46 m and 10-16 m long), tapering over the last 20 m of each
  end and never wider than the room. The outline is ragged (a fine wobble
  and coarse lumps, 0.8-1.2 of the half-width). One to three spurs (one more
  on ranges over 120 m), 14-30 m long, branch off away from the passes, and
  two to five foothill knobs bulge from the flanks. Half-widths of 9 m and
  more let the renderer raise real peaks (section 6.1).
* **Passes.** Ranges of 64 m and more keep a pass 12-16 m wide, two passes
  from 200 m. A pass goes where it makes the most useful lane: where the
  range stands between neighbouring starts, at a saddle, with open ground on
  both sides (within the middle 40 % of the range, or 20-42 % and 58-80 %
  for two). The rock narrows to 70 % over 8 m toward it, and the pass and
  4 m beyond the rock either side are reserved, so nothing closes it.
* **Massifs.** A big single massif is a main peak of 13-19 m radius (times
  the square root of the land side over 300 m, 0.8-1.6) with two to four
  lesser peaks round its flanks. An edge massif goes in a corner where two
  land edges meet (three times in five, each corner once; a main peak of
  7-10 % of the land side round the corner and one or two lesser peaks
  along each edge) or against a land edge (a main peak of 14-24 m, times the
  same root, with up to two lesser peaks along the edge either side), partly
  off the map, which the renderer's skirt carries on.
* **Keeping the play open.** Mountains never take reserved cells (start
  areas, corridors, spots, sites, fords, passes) and keep a metre from them;
  they keep 7 m from the lanes from every start to its two contested spots
  and from every spot to the main site (so no range favours one neighbour's
  way to a spot), 10 m beyond every spot's reserved ground, and 10 m (room
  for the widest ground unit) from water, other rock, dunes and, except edge
  massifs, the map edge. Highlands, ridges and lakes placed after them keep
  the same 10 m, and trees, thicket and marsh keep 9 m from mountains and
  4.5 m from the lanes' axes, so a wood or a marsh never closes what the
  mountains leave open and every spot keeps an open way in.
* Highland ridges (`cliff`) and dunes still appear: 1-2 cliff features (one
  more on Large, two more on Huge; a map with no room for any gets one small
  highland closer to the main site) and 1-2 dune regions (one more on Large
  and Huge), as do forests with thickets (about half the scattered forest
  clusters grow beside another, so they gather into woods). The dunes are
  placed before the mountains, the highlands and ridges after them, in the
  room that is left. A highland's ramp opens roughly toward the centre,
  onto the most open ground of a few such directions, with a 5 m apron below
  its foot kept free of trees and thicket.
* The painting order: coasts, meteor sites, rivers, dunes, mountains,
  highlands and ridges, lakes, marsh, the main site's surroundings, the
  start areas.

## 3. Meteor sites

The star fell in one of five ways. The main site is always at the layout
centre, so every start is the same distance from it. Every kind keeps the
design budget: 8-12 aether crystals, 1-3 vents and 2-4 adamant meteors in the
site(s), plus the contested spots between neighbours. Site floors are
`scorched`; rims are `cliff`.

| kind | name | what it is |
|---|---|---|
| `Crater` | The Starfall Crater | a cliff rim (11 % of `L`) with a ramp gap toward every start and 1-2 extra, everything on the floor |
| `Shattered` | The Shattered Fall | a smaller central crater (7 % of `L`, at least 18 m and 2.5 m per start plus 7) with a gap toward every start and half the nodes (1-2 vents, 1-2 meteors, 4-6 crystals); the rest fell at the contested spots, each now a small rimmed crater (4.5 %, at least 12 m) with a gap toward each of its two neighbouring starts, holding its usual vent (at its centre), meteor and veins plus 2 crystals |
| `Lake` | The Crater Lake | the crater (12 %, at least 24 m) is flooded: from the rim in, a 6 m dry shore of scar, shallows, deep water, shallows and a central island of scar (42 % of the rim radius); the vents and meteors stand on the island (on the shore when it is full), the crystals on the shore and the island, and a ford causeway 11 m wide runs from every rim gap to the island; hover and amphibious units cross anywhere |
| `Scar` | The Skyfall Scar | the star skidded: three overlapping basins in a line through the centre (main 8 %, at least 19 m; two 6 %, at least 14 m, at 13 % of `L` or far enough to overlap the main basin), walled by cliff with a gap toward every start and at both ends; for two players the line runs across the axis between them, for four along a bisector, so every start lies as near a side basin as every other, and each side basin holds the same share of the nodes (a third of each kind, rounded down), the main basin the rest. The side basins shrink toward the centre until they keep out of the start areas (on crowded maps, the clear discs) and the starts' aether clusters. With three or five to eight players (where side basins would lie by only some starts), and when no line allows them, the scar is a single basin the size of the Starfall Crater with its extra gaps at the line's ends |
| `Spire` | The Starspire | no rim: an impassable crystal spire (a `mountain` massif of radius 4 %, at least 6 m, drawn as a giant aether crystal) at the centre, with the nodes on an open ring of scar round it (10 %, at least 12 m wide); armies meet from every side |

Rim gaps are 11 m wide and causeways 11 m: a 7 x 7 cell clearance square
(the titan) needs about 10 m across a diagonal way. The way from each start
gap (and, at the Scar with up to four players, from both ends, down the
furrow) to the site's open hub is kept free of nodes 5 m either side, as are
the start corridors; every other gap keeps its mouth clear, and every gap
has an 8 m apron outside it kept free of trees, thicket and marsh. Nodes
that units walk over (vents) only keep off a way's axis. The main site's
band of marsh and bramble keeps 10 m beyond every contested spot's reserved
ground. A start's aether cluster
stops 5 m short of the main site along its corridor (the shared template
uses the site's widest reach toward any start), and the contested spots are
pushed out along their bisectors to keep 3 m outside the main site.

`MapInfo::crater_center` and `crater_radius` describe the main site (its
centre is the spire itself at the Starspire); every site is in
`MapFeatures::sites`.

## 4. Connectivity and fairness

* The start corridors (start to main site, 5 m half-width, reserved to 6 m)
  stay reserved: no river except as a ford (and none at all within the start
  area), no lake, mountain, thicket, marsh or tree.
* The lanes from every start to its two contested spots and from every spot
  to the main site are kept open too, though not reserved: mountains keep
  7 m from their axes, and trees, thicket and marsh 4.5 m. Mountains also
  keep 10 m beyond every spot's reserved ground, and trees, thicket and
  marsh 9 m from every mountain (section 2.4).
* After generation, `connect.rs` checks that every start front (within 4 m),
  every contested spot (within 9 m of its vent: its open ground inside its
  ring of veins), every other meteor site (the Scar's side basins, within
  90 % of their radius; the Shattered Fall's spot craters are the spots) and
  the Crater Lake's shore are reachable from the main site's hub for each
  ground class at its widest unit: Foot and Mounted at radius 1.0 (3 x 3
  cells), Wheeled at radius 2.5 (5 x 5: the war ordu, the sand engine) and
  Tracked at radius 3.0 (7 x 7: the titan); and every highland top on foot.
  Fords, crater gaps, causeways and mountain passes are therefore at least
  10 m wide. Anything cut off gets a route carved as a last resort, along
  the cheapest way from open ground near the place to ground already
  reached, keeping the class's radius from buildings and layout nodes:
  thicket, marsh and rock become grass, water a ford; ocean is never carved
  and no layout node (start resources, spots, sites, highland ore) is ever
  deleted, only scattered trees and dune nodes. Carved routes are listed in
  `GeneratedMap::carved` (rare: 12 maps of the full sweep's 4200 need one,
  for a highland ramp or a contested spot hemmed in by woods); what stays
  cut off is listed in `GeneratedMap::cut_off`, which tests assert is
  empty.
* Every player gets the same start template (rotated), as today; each
  template node goes in at every start or at none, so a coast, the map edge
  or a crowded neighbour never leaves one start short. Rivers, lakes, coasts
  and mountains are not mirrored: like Age of Empires random maps, the wilds
  differ, the starts and the main site do not.

## 5. What af_map publishes

Besides today's resources, af_map inserts `af_core::ext::map::MapFeatures`
in `OnEnter(Playing)` (and removes it on exit):

* `options`: the resolved `MapOptions` as played (no `Random` left; the
  size a `map_size` override plays as, and the number of rivers actually
  laid, should one find no course);
* `land_min`, `land_max`: the land rectangle (the map minus ocean bands;
  every coast lies outside it);
* `ocean_edges`: which map edges are ocean (west, east, south, north);
* `sites`: every meteor site (kind, centre, radius), main site first; the
  Shattered Fall's spot craters follow in spot order, the Skyfall Scar's
  side basins follow as sites of their own;
* `rivers`: each river's centreline (every 0.75 m, starting a few metres
  beyond the map edge at its source and ending beyond the edge, in the sea
  or on the river it joins), deep half-width per point and its ford centres
  (indices into the points), source first;
* `mountains`: each range's spine and half-widths (for drawing): a range is
  listed as its spine between its passes plus each spur, then each foothill
  knob as one point and its radius; a massif as each of its peaks (one point
  and its radius). The Starspire's spire is not listed (it is the main
  site's).

`GeneratedMap` carries the same `MapFeatures`, so tests and the headless
harness see it. `cargo run -p af_map --example map_png -- --help` draws maps
(or contact sheets over seeds and options) to PNG for review.

## 6. The look

The terrain stops being boxes. Everything here is af_render only; the
simulation is still 2D and nothing in it reads heights.

### 6.1 Ground

* A **height field** with a vertex every metre (cell corners), built once per
  match into chunks of 64 x 64 cells (1 m each), each one mesh with one terrain material.
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
  * grass: a deep, lush green painted from large to small: broad darker
    swathes (a hundred metres and more), lighter yellow-green and olive
    meadows between them, now and then a field-like plot of pasture, hay
    or clover with faint stripes, lusher hollows and sunny rises a view
    across, dry spots; fine detail (clumps, specks of wildflowers) kept
    soft, since the game is seen from 40-66 m; richer and darker near
    water;
  * sand: warm pale sand with ripples; beaches lighter;
  * rock (highlands, mountains): grey with variation and strata, darker
    crevices, green at the foot, snow on gentle slopes above about 7 m;
  * a meteor site's rim (its `cliff`): a raised lip of heaved, scorched
    rock, big blocks split by dark joints along its crest, screes of
    angular stones down its flanks, a shaded ring at its feet; the lip's
    swell is in the shading only (bumped normals), the cells units climb
    on stay at their ground;
  * marsh: dark wet olive with puddles;
  * scar: dark basalt with slowly pulsing aether veins (emissive);
  * thicket ground: dark leaf litter (the brambles are props, 6.3).
* Palette (sRGB): meadow #3E702D with deep swathes #2B5224, yellow-green
  meadows #548230, olive #617338 and dry accents #8F8C54 (fields: pasture
  #578033, hay #807D45, clover #38612A); forest floor #3B5A2A; dirt
  #8A6A48; beach sand #D8C38E, dune sand #E3D3A2; marsh #556B3A with dark
  water #2F4A4A; rock #A4A6A8 / #7E8184 / #55585C with lichen #B9BE8C;
  site rims #8F8A85 / #66625F / #3D393B; snow #EEF3F8 shading to #C9D6E6;
  scar #3A3436 / #241F22 with aether violet #BF73FF in the veins.
  Grass and water stay a step below the team colours in saturation (Sylvan
  green, Tide cyan and Ironhold blue must still read on them).
* Fog of war is drawn by the terrain, water and prop shaders from
  `FogTexture` (linear filtered): unexplored near black, explored darkened
  and desaturated. The flat overlay plane goes, since it cannot sit over
  mountains.
* Around the map a 60-80 m **skirt** of out-of-bounds land continues the
  terrain (meadow, distant forest seen from above as a soft green canopy,
  and mountains; ocean beyond ocean edges) darkened and desaturated so it
  reads as off the map. Rivers carry on along their own lines, winding on
  and narrowing into the distance rather than running out straight; the
  first ring of cells beyond the edge always continues the edge's water,
  which is what the ground units walk on assumes there
  (`af_core::ext::render::off_map_terrain`). The frame boxes go.

### 6.2 Water

One water surface at `y = -0.12` over every water and ford cell (with a cell
of overlap under the banks, so the rising bank cuts the shoreline), drawn
opaque (the depth look is faked from per-vertex depth, so the many
alpha-blended overlays never sort against it) with a water material:

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
* charred, ore-less meteor fragments in the meteor sites, screes of
  angular stones down the sites' rims and jagged spurs along their crests;
* the Starspire for `Spire`: a giant dormant crystal, long six-sided
  prisms with pointed tips leaning out from a common base (three tall
  ones at its heart, up to 10.2 m, a ring of shorter ones and slimmer ones
  fanning out low, and smaller clusters breaking through toward the edge
  of its impassable footing), pale smoky white-violet with darker facets; only
  their feet glow a soft violet, well under a twentieth of the surface. It
  casts its shadow. (A modelled spire may replace it later.)

A prop must never look like something a worker can gather (the style
guide's node colour code): no loose grey boulder piles (stone), no red
berries (berry bush), no bright violet crystals (aether), no gold-veined
charred rock (adamant), no scorched rings (vent), no free-standing trees
(every tree is a wood node).

**Decor models** (`af_render::terrain::decor`; the static build's `decor`
kind, from Tripo) dress the land beside the procedural props, by the same
principles: placed from the map alone (the field and `MapFeatures`, hashed
from positions and the seed), never a sim entity, never on buildable
ground, and where units walk only low things they wade through (reeds,
river rocks, lily pads, brambles, ferns, shrubs, beach rocks; the rim arcs
on the climbable rims):

* mountains read as craggy ranges: peaks on the height field's summits
  (scaled to the local height and width, sunk into the field, which stays
  the base; snow on the tallest; never above 10.4 m), ridges along the
  ranges' spines, foothills at their ends and feet;
* crater rim arcs along every rimmed site's rim, facing in, never in a gap;
  monoliths join the Starspire on its footing; star cores at its foot and
  in the Crater Lake; impact scars and meteor shards on the rims' free
  cells;
* reeds on shores and in marsh, river rocks in fords, lily pads on still
  lake water, brambles over thickets (a share of the procedural brambles
  and reeds stays), ferns, shrubs and mossy boulders at the edges of
  thickets, highlands, mountains and woods;
* the coast: sea stacks offshore, beach rocks at the waterline, sea cliffs
  along the skirt's coast, wrecks and driftwood there;
* landmarks units would walk through stand only in the skirt: bridges over
  its rivers, standing stones, ruins and shrines on its meadows, fallen
  logs at its woods, and distant mountains and forest in its outer ring.

Each rule uses whichever of its models exist; with none the land looks as
it did before them. The full table: `docs/art/static-models.md`, "Decor".

### 6.4 Light

A brighter, warmer afternoon: a warm sun, sky-tinted ambient light, a light
hazy blue sky colour with distance fog matched to it, a touch of extra
saturation in the colour grading and a soft bloom so aether glows. The
haze scales with the camera's distance from what it looks at: it starts
just beyond the focus and only softens the top of the view, the far
horizon and the skirt by the same amount at every play zoom (25-210 m),
and thins out from a few hundred metres up, so the dev overview from
kilometres away stays clear and saturated.

### 6.5 How it is built

* Height field and meshes are a pure function of the map (`Nav` terrain,
  `MapInfo`, `MapFeatures` when present), built once per match, in chunks of
  64 x 64 cells with normals taken from the whole field so chunk seams match.
  Flat chunks cast no shadows; mountain chunks do.
* `TerrainMaterial` and `WaterMaterial` are
  `ExtendedMaterial<StandardMaterial, _>` with WGSL shaders embedded in the
  af_render crate (`embedded_asset!`), so they load from any asset root.
  Their material plugins are added only when the render stack exists;
  headless test apps get the same chunk entities with a vertex-coloured
  `StandardMaterial`. Animation reads `globals.time` (periods divide 3600 s);
  no material changes per frame.
* Ground-bound things keep standing where they stand today (walkable ground
  is unchanged); on water, markers sit on the water surface; flyer shadows,
  pings and rally flags sit on the drawn surface.

## 7. Pathing on big maps

A river or mountain range that spans the map forces detours of hundreds of
metres, which the path service cannot find today (each A* search stops after
24,000 cells: a ford more than about 110 m off the straight line is never
found, and the unit stops on the near bank). So:

* **Regions.** af_movement labels connected regions of the terrain once per
  map for each mover class it meets (admitted terrain set and clearance
  level; obstacles and teams ignored), and publishes them read-only in
  `af_core::ext::movement` (`NavRegions`). A path request whose goal lies in
  another region is answered at once, with the reachable point nearest the
  goal and no search.
* **Coarse routes.** Long requests are planned first on a coarse terrain
  graph (static per match, per mover class), then followed leg by leg with
  the existing obstacle-aware sliced A*, each leg well inside its cap. A
  truncated search ends at its best g + h frontier cell, not at the bank
  facing the goal. One long request can no longer hold the head of the
  queue for many ticks.
* **Consumers.** af_behavior and af_ai skip targets in another region
  (drop sites, replacement nodes, attack targets, remote resources, vents,
  build spots) and prefer route distance where they rank by distance; the
  AI measures attack progress by route distance left and scales its
  military radii with the map. The invariant tests measure stuck and
  stalled units by path progress, not straight-line distance.

## 8. Testing

* af_map (`crates/af_map/tests/generate.rs`): a sample of every size x
  water level x site kind, with 2-8 players and one or two rivers cycling
  through it: the reachability above for every ground class, checked by the
  test's own flood as well as the generator's report, and carving on at most
  one map in fifty; the requested river count, each river's course away from
  its ends at least 8 m inside the land edges, fords no more than 36 m apart
  and no deep river stretch over 40 m, and no tree, thicket or marsh in a
  ford's ways out; starts and start discs on clear
  grass; ocean present exactly at `Coast`/`Seas` and never on the land;
  mountains present on Medium and larger, on 3.5-9 % of the land on
  average there (0.5-4.5 % on Small), with a range at least 9 m half-wide
  on every such map, never in a start area, a corridor, by a contested spot
  or across a lane, and no tree, thicket or marsh at their feet; a pass
  stays open and reserved right through its range; dunes, a ridge or highland,
  marsh, thicket and mostly grass on every map; node budgets per site kind;
  mirrored start resources; no overlaps; determinism; a 384 m map for 8
  under 1 s and a Huge one with seas under 4 s. `--ignored` runs the full
  sweep (every combination, three seeds, 4200 maps: everything reachable,
  the rivers asked for, carving tallied by cause) and a diagnostic listing every
  problem over a wide sample.
* af_render: walkable cells drawn at `ground_height`; mountains under 10.5 m;
  every water bed below the water surface; chunks cover the map once; decor
  (`crates/af_render/tests/decor.rs`, over generated maps of every site
  kind and water level) keeps its rules (above), is deterministic and
  bounded, and dresses rims, ranges, shores and the skirt's rivers.
