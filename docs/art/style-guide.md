# Aetherfall art style guide

Every generated model follows this guide. It is written for two readers: people
judging concept art and models, and the Meshy tool (`tools/meshy/meshy.py`),
which builds every prompt from the layers in `art/meshy/style.json` (section 10).
Change the layers there and every later concept follows; models already made
keep the style they were made with.

Decided with the project owner on 2026-10-19:

| Question | Decision |
|---|---|
| Shared units and buildings | One model per id, faction-neutral. Team colour comes from the pipeline (sash or marker on units, banner on buildings). |
| Overall look | Grounded realism, matching the peasant and Meshy batch 1. |
| Proportions | Slightly heroic. |
| Colour | Neutral materials, a small faction accent on faction units only. |
| Late-age tech | Gold, grey and dark shadowy metal, rivets, verdigris bronze, thin glowing lines. Amber glow in the Starfall and Engine Ages, blue in the Ascendant. |
| Magic | As restrained as the tech, in each faction's own metals and glow colour. |
| Wear | Lived-in. |

## 1. The picture we are making

A real-time strategy battlefield seen from about 40 m, pitched 54 degrees down.
A soldier is about 1.4 m tall and fills a few dozen pixels, and a town centre
fills a few hundred. Everything is judged at that distance first and in
close-up second.

The look is **grounded realism with a hand-made warmth**. Batch 1 sets it: the
house and the great hall have chunky, slightly soft forms, thick thatch, timber
with visible grain, and stones with rounded edges. Materials are believable
and the colours muted, but shapes are simplified and a little generous, like a
well-made miniature rather than a photograph. It is not cartoon (no outlines, no
flat cel colour, no big heads) and not gritty photorealism (no fine noise, no
photographic dirt).

**Readability rules:**

- **Silhouette first.** Every unit's role reads from its outline: a spear's
  length, a crossbow's stock, a tower's height, a walker's legs.
- **Few, large details.** Prefer three big shapes to thirty small ones. Rivets,
  runes and trim are welcome at the scale of a hand, not a fingernail.
- **Value contrast between parts.** Dark armour over light cloth, a pale
  thatch over dark timber, so the forms separate when small.
- **Glow is an accent, never a light show.** Glowing lines and cores stay under
  about 5 % of a model's surface.

## 2. Ages: materials, metals and glow

Each age keeps everything before it and adds a layer. A late unit is still made
by smiths and carpenters: tech grows out of craft, it does not replace it.

| Age | Materials added | Metals | Glow |
|---|---|---|---|
| 1 Hearth | timber, wattle and daub, straw thatch, fieldstone, undyed wool and linen, leather, fur | bronze, rough dark iron | none |
| 2 Banner | dressed stone, slate and shingle roofs, mail, padded gambesons, steel plate pieces | forged steel, iron | none |
| 3 Starfall | the first aether: violet crystals set in fittings, carved stone housings, glass | brass, gold trim, verdigris bronze, dark iron with rivets | thin warm amber lines |
| 4 Engine | pipes, gauges, boilers, cogs, soot, riveted plate, canvas and rubberised leather | blackened iron, gunmetal grey, verdigris bronze, gold trim | thin warm amber lines, small amber cores |
| 5 Ascendant | polished plates, smooth forged shells over the old frames, shadowy recesses | dark polished metal, gold, cool grey, still riveted and hand-forged | thin blue lines and small blue cores |

- **Aether crystal itself is violet**, in every age: the raw resource, the
  crystals set into machines, the cargo. Machines that run on it glow amber
  (Starfall, Engine) or blue (Ascendant) along their lines.
- **"Shadowy"** means the late ages are darker overall: dark metal and deep
  recesses, lit by gold trim and the thin glow, never black on black.
- **Verdigris bronze** is aged copper and bronze with a soft green patina. It is
  the signature metal of the Starfall and Engine Ages, alongside gold and dark
  iron.

## 3. Magic by faction

Faith, nature magic and sorcery match the tech in restraint: shiny metal in
the faction's own colour and a thin glow, never auras, smoke or particles baked
into a model. Machines use the age glow (section 2); magic uses the faction's
glow below. Where a unit is both (a Synod hover machine of faith), the faction
glow wins.

| Faction | Magic metals and materials | Magic glow |
|---|---|---|
| Synod of the Fallen Star (`starborn`) | white enamel, polished gold, pale crystal | pale gold starlight |
| Sylvan Wardens (`sylvan`) | living wood and bark, bronze, moss, river stone | soft green |
| Sunspire Caliphate (`dune`) | sun-fused glass, polished brass, sandstone, white linen | amber sun-glow |
| Ironhold Thanes (`ironhold`) | blackened iron, carved stone, gold runes inlaid | molten orange in the runes |
| Tidecrown Republic (`tide`) | copper, verdigris, coral, mother-of-pearl | sea-teal |
| Khaganate of the Steppe (`steppe`) | horn, silver, lacquered leather, felt | white-gold sky-fire |

## 4. Factions: identity for faction units and buildings

Shared (common-pool) units and buildings show **no faction at all**. Faction
units and buildings carry their culture in shapes and materials, and their
accent colour only on small areas (trim, a sash, a cap, a cloak lining). Team
colour comes on top from the pipeline, so no large area of a model is strongly
coloured.

| Faction | Culture and shapes | Accent colour |
|---|---|---|
| Ironhold Thanes | Norse and Germanic mountain clans, dwarves in spirit though human: broad, stocky, heavy plate, bearded faces, rock-cut halls, round shields, anvils and forges | soot black and oxblood red, gold runes |
| Sylvan Wardens | Celtic forest clans: druids, rangers and beast-tamers; antlers, torcs, bark and living wood, standing stones; never stone keeps | moss green and bark brown |
| Sunspire Caliphate | desert scholars and glass-wrights (Abbasid and Persian): domes, arches, observatories, veils, turbans, curved blades, mirrors and lenses | white linen and saffron, polished brass |
| Tidecrown Republic | lagoon merchant republic (Venetian and Byzantine) with a Varangian guard: galley and arsenal shapes, lamellar, coral crowns, siphons | teal-cyan and copper; Varangians in red cloaks |
| Khaganate of the Steppe | Mongol and Turkic horse clans: felt tents, wagons, composite bows, lamellar, fur-trimmed hats, horse tails on standards | indigo felt and silver |
| Synod of the Fallen Star | monastic star-cult: paladins, monks and winged warrior-monks; cathedral and reliquary shapes, star motifs | white and gold |

## 5. Colour and team colour

- **Base palette:** undyed wool (oatmeal, grey, brown), leather browns, iron
  and steel greys, timber, stone, thatch.
- **Player colour is added by the pipeline:** a sash or marker on every unit,
  a banner on every building, tinted in game. Models therefore carry **no
  flags, banners, heraldry, tabards in strong colours, text or logos**.
- **Saturated colour** appears only as the faction accent (small areas) and in
  glow.

## 6. Units

**One model per unit type, one figure per model.** Squads, bands, pairs,
wings and swarms are several units of the same type in game, so a "Militia
Band" prompt describes one militiaman.

**Proportions: slightly heroic.** Real anatomy, but weapons, shields, helmets
and hands are about 1.2 to 1.3 times life size, and shoulders a little broad.
Heads stay normal size. A bow is longer than in life and a halberd's blade
bigger, so the role reads from far away.

**Who they are:** adults of mixed ages and builds; follow the unit's
description where it says who the unit is (the Sand Mage and the Sky Shaman
are women). Faces are plain and weathered, not glamorous.

By body type:

| Type | Units (examples) | How they are made |
|---|---|---|
| Humanoid, rigged | spearman, crossbowman, man-at-arms, monk, rifle squad, lumen knight, varangian | Standing in an **A-pose** with **empty open hands**, feet flat. Everything worn stays on the body: helmet, armour, belt, quiver, scabbard, pack, slung shield. **Weapons and held shields are separate prop models** (kind `props`), attached to the hand by the pipeline, because Meshy's auto-rig weights a held spear or bow badly. |
| Mounted | scout rider, knight, horse archer, elk rider, camel rider | One rigid model: rider seated on the mount, both standing still and symmetrical, mount's four legs planted, rider's weapon held upright and part of the mesh. |
| Beasts | war hounds, war bear, rootbreaker, thornback | One rigid model in a natural standing pose, four feet planted, head forward. |
| Vehicles, siege, mechs | battering ram, ballista, bombard, steam landship, mech walker, hover tank | One rigid model at rest: wheels, tracks or feet on the ground (hover craft just above it), weapon facing forward. |
| Flyers | war balloon, gyrocopter, ornithopter, seraph, gunship, starfighter | One rigid model in level flight: wings, rotors or envelope spread, nothing touching the ground. |
| Giants | elder treant, forgefather, archon, titan | One model, upright, imposing; treant and forgefather can be rigged later if they read as bipeds. |

The model faces the viewer (+Z), stands on the ground at the origin, and is
scaled in game to its box height, so proportions matter more than size.

## 7. Weapon and shield props

Small separate models, one per distinct weapon: a spear, a pike, a crossbow,
a sling, a halberd, an arquebus, a rifle, a round shield and so on. Each is
shown alone and whole, lying flat in three-quarter view, in the same
materials and age rules as its unit. Heroic size applies (section 6). No hands,
straps to nowhere or stands.

## 8. Buildings

- **One building per model,** standing on flat ground, footprint matching its
  box (square or rectangular as the data says). No base plate, no ground
  around it, no people, no animals, no flags or text.
- **Chunky massing:** a clear roof shape, thick walls and a few large
  features (doors, chimneys, turrets, crystals) that read from above.
- **Upgrade chains share their DNA.** A palisade, stone wall, reinforced wall
  and force wall are the same line built better; watchtower to plasma turret
  keeps one tower silhouette growing; town centre to citadel keeps the great
  hall at its heart.
- **Walls and gates** are straight segments with flat, plain ends so segments
  join seamlessly: no towers or posts at the ends. Gates are a wall segment
  with one large gate in the middle.
- **Shared buildings** follow batch 1's Iron Age to medieval village and grow
  through the ages per section 2. **Faction buildings** follow section 4.

## 9. Resource nodes

Natural objects with no base plate, coloured so the resource reads from far
away: grey stone; copper ore in green-orange veins; iron in rust-red veins;
pale quartz sand for silicon; berry bushes with red berries; violet aether
crystals; the adamant meteor charred black with gold ore; the aether vent a
scorched crater rim with violet light deep inside.

## 9a. Landscape

The land is where the picture departs from the objects' muted palette: a
temperate European landscape in the spirit of Civilization VI, **lush
greens, deep blues, grey rocky mountains with some variation, rivers a
bright blue with white blended in**. It is painted procedurally by
af_render's terrain shaders (`crates/af_render/src/terrain/*.wgsl`; no
textures), so these colours are the shaders' palette:

| Surface | sRGB | Notes |
|---|---|---|
| Meadow | `#5E9A3A` | base; shade `#3F7A2E`, sunlit `#7DB348`, dry accents `#A5A957`; wildflower specks white, yellow, pale blue (never red: berries) |
| Forest floor (thickets, off-map woods) | `#3B5A2A` | moss `#4E6E2E`, leaf litter `#6B5434` |
| Dirt | `#8A6A48` | |
| Sand | dunes `#E3D3A2`, beaches `#D8C38E` | wind ripples, pebbles, grass tufts at meadow edges |
| Marsh | `#556B3A` | puddles of dark water `#2F4A4A` |
| Rock (highlands, mountains) | `#A4A6A8` / `#7E8184` / `#55585C` | lichen `#B9BE8C`; lighter on ridges, darker in gullies |
| Snow | `#EEF3F8` shading to `#C9D6E6` | gentle slopes above about 7 m |
| Meteor site floor | `#3A3436` / `#241F22` | cracked basalt; aether veins `#BF73FF`, the only glow on the land |
| Shallows, fords | `#6FC0EA` | the shores' clear turquoise |
| River | `#3C9BE0` | foam `#F2F8FB` streaking downstream, white water over fords |
| Lake | `#1F5BA8` | |
| Sea | `#173E7A` to `#0F2A57` | swell and surf on beaches |
| Sky and haze | `#A1BFDB` | the distance fog and the skirt's far edge fade into it |

Rules:

- **Team colour still reads.** Grass and water stay a step below the team
  colours in saturation: Sylvan green, Tide cyan and Ironhold blue must
  stand out on meadow and river. Nodes keep their colour code (section 9)
  against every ground.
- **Mountains are not highlands.** Impassable mountains are tall grey peaks
  (at most 10.5 m, below every flyer) with snow on top and green at the
  foot; climbable rocky highlands are a low (0.6 m) plateau of grey rock
  with grass and lichen on top and dark scarps. Never draw a mountain low or
  a highland tall.
- **Decoration never looks gatherable.** Brambles, reeds, scree and meteor
  rock grow only on unbuildable ground and avoid the node colour code: no
  loose round grey boulder piles (stone), no red berries, no bright violet
  crystals (the Starspire is pale), no gold in charred rock, no scorched
  rings, no free-standing trees (every tree is a wood node).
- **Off the map** the land carries on, greyed and darkened, fading into the
  haze; never a frame or a void.
- **The fog of war** is drawn into the land: out of sight darkened and
  greyed, unexplored near black.
- **Scenery models** made later (mountain pieces, crater rims, reeds, ...)
  follow this palette rather than the muted object rule, may carry snow, and
  must blend into the ground instead of standing on a plate.

## 10. How prompts are built (Meshy)

Every asset goes through **concept art first, then a model built from the
approved concept**:

1. **Concept** (image to image, `nano-banana-pro`, 2 candidates per asset).
   Style references are sent with every request: batch 1 renders for
   buildings and nodes, a peasant render for units, and from phase 3 on the
   approved phase 1 concepts. The prompt is built from `style.json`, in order:
   - **framing:** how the picture is set up: a character front view in an
     A-pose on a plain grey background, or an object in three-quarter view;
   - **Subject:** the asset's own description from `requests.json`;
   - **faction** layer (faction assets only), then the faction's **magic**
     layer (assets marked `magic`), then the **age** layer. Magic assets
     leave the age out, since its machine glow would fight the magic glow;
     their description says what era they belong to;
   - **kind** layer (unit, prop, building or node rules);
   - **look:** the global look.
2. **Pick.** A person chooses one candidate per asset (`meshy.py pick`).
3. **Model** (image to 3D from the picked concept, `meshy-6`, 2k texture,
   lighting removed, remeshed to the kind's triangle budget), then **rig**
   for humanoids (Meshy auto-rig), then install where the pipelines read it.

Asset descriptions in `requests.json` say **what** the thing is (body,
gear, materials, silhouette); the layers say **how** it looks. Keep
descriptions free of style words so the layers stay in charge.

## 11. Never

- Text, letters, numbers, logos, heraldry, flags or banners on any model.
- A base plate, plinth, ground patch or display stand.
- More than one figure or object in a model (no groups, no crew of three).
- People or animals around buildings.
- Weapons held by rigged humanoids (they are separate props).
- Baked lighting, cast shadows or ambient occlusion painted into textures.
- Glow covering large areas, particle effects, smoke, auras.
- Cartoon proportions, outlines or flat cel shading; anime faces.
- Strong colour on large areas (team colour must read).

## 12. Generation plan

| Phase | What | Credits (estimate) |
|---|---|---|
| 1 | Concept art for pilot batch 2 (15 assets x 2 candidates) | ~270 |
| 2 | Models of batch 2 (15 image-to-3D, 3 rigs) | ~470 |
| 3 | Concept art for everything else (~180 assets x 2) | ~3,240 |
| 4 | Models of everything else (~180, ~36 rigs) | ~5,580 |

Phases 3 and 4 run only if phases 1 and 2 look right. Batch 2 tests each
choice in this guide at least once: ages 1 to 5, a rigged humanoid, a rider,
a vehicle, mechs, a hover craft, a giant, props, a building, a tower, a wall,
and two factions' magic:

| # | Asset | Kind | Why it is in the pilot |
|---|---|---|---|
| 1 | spearman | unit (rigged) | the most common soldier; Hearth materials; heroic proportions |
| 2 | crossbowman | unit (rigged) | Banner mail and steel |
| 3 | scout_rider | unit (mounted) | rider and horse as one rigid model |
| 4 | battering_ram | unit (vehicle) | timber siege engine |
| 5 | clockwork_automaton | unit (mech) | Starfall: brass, verdigris, violet crystal heart, amber lines |
| 6 | mech_walker | unit (mech) | Engine: blackened iron, rivets, gold trim, amber |
| 7 | hover_tank | unit (hover) | Ascendant: dark metal, gold, blue glow |
| 8 | starborn_lumen_knight | unit (rigged) | Synod magic: white enamel, gold, pale gold starlight |
| 9 | sylvan_elder_treant | unit (giant) | Sylvan nature magic, green glow, a creature |
| 10 | short_spear | prop | proves the prop approach (the spearman's spear) |
| 11 | crossbow | prop | a complex prop (the crossbowman's crossbow) |
| 12 | barracks | building | shared Hearth building next to batch 1 |
| 13 | watchtower | building | start of the tower chain |
| 14 | stone_wall | building | wall segment that must tile |
| 15 | aether_foundry | building | Starfall industry: carved stone, brass, crystals, amber |
