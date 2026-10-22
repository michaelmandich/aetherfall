# Aetherfall: game design and shared framework

Working title **Aetherfall**. A real-time strategy game in Rust (Bevy 0.19) for an
eventual Windows release on Steam. Everything on screen is a box for now: the map,
every building, unit, weapon and projectile has a size and a colour, and nothing more.

This document is the lead designer's framework. It defines the world, the age arc,
the economy, the combat and defence models, and the **common pool** of content that
all six factions draw from. Faction designers pick subsets of the pool and add their
own uniques; section 10 lists the rules they must follow.

Content lives in `assets/data/**/*.ron` and is validated by the checker
(`cargo run -q -p af_data --example check -- --summary`). The schema is
`crates/af_data/src/schema.rs`. The shared files are `assets/data/core/*.ron`:
8 resources, 5 ages, 7 terrain types, 11 node types, 53 weapons, 43 units, 36 buildings, 53 techs. Every number in the tables below is read from those files.

## Factions at a glance

Each faction has its own file, `assets/data/factions/<id>.ron`, and its own design doc,
linked below. With the core files they make 123 weapons, 95 units, 58 buildings and
105 techs (52 unique units, 22 unique buildings, 52 unique techs, 70 unique weapons).

| Faction | Epithet | Colour | Harvest bonus | Terrain trait | Signature units | Weaknesses |
|---|---|---|---|---|---|---|
| [The Ironhold Thanes](factions/ironhold.md) (`ironhold`) | Mountain-hold clans who never give ground | cobalt blue `(0.16, 0.34, 0.86)` | stone x1.25, iron x1.2, copper x1.15 | Climber (Foot): infantry and workers scale cliffs | Thane Guard, Rune Cannon, Steamplate Thane, Runeguard, Forgefather | slow; no cavalry beyond scouts; no flyers or hover craft; short-range late infantry |
| [The Sylvan Wardens](factions/sylvan.md) (`sylvan`) | Forest kingdoms that grow their armies | warden green `(0.2, 0.82, 0.32)` | wood x1.25, food x1.15 | Forestwalker (Foot, Mounted): cross bramble thickets | Longbowman, War Bear, Crystal Ranger, Wisp Swarm, Elder Treant | fire (beasts and hedges have no fire armour); polearms; hedge walls and no castle; no tanks or artillery; thin Starfall anti-air |
| [The Sunspire Caliphate](factions/dune.md) (`dune`) | Sand-mages and glass-wrights of the deep desert | sun-saffron `(0.96, 0.62, 0.12)` | silicon x1.25, aether x1.2 | Dunerunner (Foot, Mounted, Wheeled): full speed on sand | Camel Rider, Sunglass Engine, Veiled Blades, Sandskiff, Great Sand-Engine | no castles, temples or knights; heavy cavalry and men-at-arms in the Banner Age; ornithopters are its only flyers, so gunships go unanswered late; EMP strips the veils |
| [The Tidecrown Republic](factions/tide.md) (`tide`) | Lagoon merchants whose marines walk out of the sea | lagoon cyan `(0.0, 0.85, 0.85)` | food x1.15, copper x1.25 | Amphibious (Foot, Mounted, Tracked); Marshwalker (Foot) | Marine Band, Fire Siphoneers, Ironclad Crawler, Phaser Marines, Bucentaur | cavalry on open ground; no flyers before the Ascendant Age; mangonel is its only Banner siege; wheeled siege cannot follow across water |
| [The Khaganate of the Endless Steppe](factions/steppe.md) (`steppe`) | Every herder a rider, every rider a bow | Khagan crimson `(0.78, 0.13, 0.17)` | food x1.2, everything else x1.08 | Marshwalker and Dunerunner (Mounted, Wheeled) | Outrider, Keshig Guard, Rocket Rider, Jet Lancer, War Ordu | polearms (nearly all Cavalry); stake lines only, no stone walls or towers; thin late siege; wheels stop at water; one Golden Tent |
| [The Synod of the Fallen Star](factions/starborn.md) (`starborn`) | They saw it fall first, and have never looked away | Byzantine magenta `(0.85, 0.2, 0.65)` | aether x1.2, adamant x1.25 | none: earliest and best flyers and hover craft instead | Star Kite, Paladin, Reliquary Ark, Orbital Lance, Archon | needs the crater's aether; no terrain trait; no skirmishers or mangonels early; thin Engine-age ground army; energy armour, EMP and flak blunt it |

---

## 1. Pillars

1. **Knights who found a spaceship.** The world is earthy and medieval. Technology
   arrives unevenly from a fallen star, so a pikeman, a steam landship and a hover
   tank can share a battlefield, all of them under heraldic banners. Never pure sci-fi.
2. **Hold the centre.** Every base is self-sufficient in the Hearth Age. In the
   Banner Age the first workers walk to the crater for aether (the Starfall age-up
   costs 100), and from the Starfall Age on progress needs aether and later adamant,
   which exist only in the central Starfall Crater (adamant also at a few contested
   impact sites). Armies fight over ground, not over menus.
3. **Readable boxes.** A unit's silhouette (box size), colour, held weapon box and
   projectile box tell the player what it is and what it beats. Big guys are big.
4. **Free-form space.** No grid and no hexes. Buildings and units are boxes placed
   anywhere they do not overlap. Walls are dragged as lines of segments.
5. **Counters, not stat races.** Bonus multipliers and armour types create a clear
   rock-paper-scissors in every age. A cheaper unit used well beats a dearer one used badly.
6. **Asymmetric but shared.** Six factions share one economy, one age arc and one
   common pool, then differ through gather bonuses, terrain traits, which common
   pieces they get, and their uniques. Civilization-style variety in Age of Empires
   style play, with Warhammer-style army flavour and StarCraft-style control.

---

## 2. The fallen star

For as long as the songs remember, the land was a patchwork of clans: hill-folk
miners, forest wardens, desert caliphs, sea merchants, horse lords and a small
order of star-watching monks. In the **Hearth Age** they farmed, felled oak and
fought with bronze.

In the **Banner Age** the lords raised stone and iron, and the astronomers began to
argue about a new red star that grew brighter every autumn. Scouts brought back
violet crystals from the empty heartland where its light fell hardest. The monks of
the Synod called it a sign and went to meet it.

Then the star fell. It broke over the heartland and left the **Starfall Crater**:
a bowl of glowing **aether** crystal, vents that breathe light, and meteors of
**adamant**, a star-metal no smith can scratch. Aether stores light and will. It
makes powder burn hotter, makes clockwork move on its own, lifts balloons without
fire. Every realm turned toward the crater.

What followed was not an orderly march of progress. In the **Engine Age** aether-fired
boilers drove the forges and thinking engines of brass and quartz (computers, and
the sand that makes them) computed firing tables for knights with rifles. In the
**Ascendant Age** the crystals answered back: personal shields, beams of starlight,
craft that hover and fly, adamant titans. The heralds still sound trumpets over the
plasma turrets.

Nobody knows what the star *was*. Whoever controls the most of it will find out.

---

## 3. The age arc

Ages are numbered 1-5 and their ids are fixed. Each advance is a tech with the
`AdvanceAge` effect; age-ups for ages 1 and 2 are researched at the **Town Centre**,
for ages 3 and 4 at the **Citadel** (the Town Centre's Starfall upgrade; see 3.1).

| # | id | Name | Feel | New resource pressure | Advance (tech, where, cost, gate) |
|---|---|---|---|---|---|
| 1 | `hearth` | Hearth Age | Timber halls, palisades, bronze spears, slings, short bows | food, wood, some stone and copper | `to_banner` at town_center: 500 food, 100 wood, 90 s |
| 2 | `banner` | Banner Age | Stone walls, iron mail, knights, crossbows, rams, mangonels; first crystals found | iron; aether crystals from the crater | `to_starfall` at town_center: 600 food, 150 copper, **100 aether**, 110 s; must own a `forge` |
| 3 | `starfall` | Starfall Age | The star has fallen: aether-powder arquebuses and bombards, clockwork automatons, aether lancers, war balloons, castles, citadels | aether (vents and extractors), stone for castles | `to_engine` at citadel: 600 food, 250 iron, **200 silicon**, 200 aether, 120 s; must own an `aether_foundry` |
| 4 | `engine` | Engine Age | Industry and thinking engines: rifle squads, gatlings, steam landships, mech walkers, rockets, gyrocopters, biplanes, flak, laboratories | silicon, iron, aether for powder and boilers; adamant stockpiling begins | `to_ascendant` at citadel: 800 food, 300 silicon, 450 aether, **200 adamant**, 150 s; must own a `laboratory` |
| 5 | `ascendant` | Ascendant Age | Stellar tech: shield troopers, hover tanks, gunships, starfighters, drone swarms, plasma artillery, storm walkers, star bombers dropping aether charges (the biggest blasts in the game), faction giants, force walls | adamant and aether | - |

### 3.1 Why the Citadel proclaims the late ages

A tech has exactly one `researched_at` building, and a `BuildingUpgrade` replaces
the building id. If the Town Centre researched every age-up, upgrading it to a
Citadel would strand the player. So `town_center` researches `to_banner` and
`to_starfall`, its upgrade to `citadel` unlocks in the Starfall Age, and the citadel
researches `to_engine` and `to_ascendant`. Raising a citadel is therefore part of
the price of the Engine Age (150 wood, 200 stone, 100 iron, 75 aether, 75 s as an
upgrade). Other Town Centre research is deliberately limited to the age-ups for the
same reason. Both train workers, scouts and engineers.

---

## 4. Economy

### 4.1 Resources and gatherers

Eight resources, in this fixed order (the UI bar uses it). A worker gathers at its
base rate per second, carries 10 (peasant) or 14 (engineer) before walking to a drop
site, and every rate is multiplied by the faction's `gather_bonus` and by gather techs.

| id | name | matters from age | peasant /s | engineer /s | found | use |
|---|---|---|---|---|---|---|
| `food` | Food | 1 | 0.7 | 0.7 | berry bushes and farms at home; hunting/fishing via faction bonuses | Grain, berries, herds and fish. Feeds every army in every age. |
| `wood` | Wood | 1 | 0.75 | 0.75 | oak/pine forests around every base, palms on sand | Felled from oak, pine and palm. Halls, bows, siege frames, palisades. |
| `stone` | Stone | 1 | 0.5 | 0.55 | 2-3 outcrops per base and nowhere else | Quarried from outcrops. Walls, towers, castles and town centres. |
| `copper` | Copper | 1 | 0.5 | 0.55 | 2-3 veins per base, more at the contested spots and on highlands | The first metal: bronze spear-heads, then gears, boilers and wiring. |
| `iron` | Iron | 2 | 0.45 | 0.5 | 2-3 veins per base, more at the contested spots and on highlands | Steel for blades and plate, later hulls, rails and engine blocks. |
| `silicon` | Silicon | 3 | 0.4 | 0.55 | 2-3 quartz sand deposits per base (about 38 m out), more in the dune regions | Quartz sand from the dunes. Lenses, glass and the thinking engines of the Engine Age. |
| `aether` | Aether | 2 | 0.3 | 0.4 | 3 crystals per base toward the crater; crystals and vents in the Starfall Crater, vents at the contested spots | Glowing crystal from the fallen star. It stores light and will; every machine past the Starfall Age drinks it. |
| `adamant` | Adamant | 4 | 0.18 | 0.26 | meteors in the crater and at the contested spots | Star-metal from the meteor shards: light, unbreakable, mythic. Hulls, shields and titans. |

- The **peasant** gathers all eight from the Hearth Age, so nothing is locked behind
  a special worker. The **engineer** (Engine Age, 100 cost) is the late worker:
  better at silicon, aether and adamant, bigger loads, fast repair (10 hp/s vs 3).
- Drop sites: `town_center` and `citadel` take everything; `lumber_camp` wood;
  `mill` food; `mining_camp` stone, copper, iron, silicon, aether and adamant (build
  one on the crater rim); `aether_extractor` takes aether.
- Farms are buildings with a `resource_field` (1800 food, 1 worker) for 60 wood: thirty
  times their price in food and about an hour of one farmer (35-40 minutes with every
  food tech), so a farm is sown about once a game and reseeding costs one wood worker
  per 25-30 farmers rather than one per nine or ten. Berry bushes (about 2100 food per
  start) carry the Hearth Age and run out early in the Banner Age; farms are the food
  engine from then on. The first ring of farms is still a real wood bill in the Banner
  Age (about 1100 wood for 18 farms), but after that the wood goes into houses, camps,
  production buildings, bows and siege instead of reseeding.
- Faction specialities are `gather_bonus` multipliers (guide: 1.1-1.3 on two or three
  resources). Steppe's "slightly better at everything" is 1.05-1.1 across the board.

Share of the cost of each age's content (units + buildings + techs) by resource,
which is what drives where workers must go:

| content of age | food | wood | stone | copper | iron | silicon | aether | adamant |
|---|---|---|---|---|---|---|---|---|
| Hearth | 39% | 51% | 10% | - | - | - | - | - |
| Banner | 40% | 30% | 9% | 17% | 3% | - | 1% | - |
| Starfall | 29% | 20% | 17% | 10% | 17% | 1% | 6% | - |
| Engine | 22% | 10% | 9% | 7% | 25% | 17% | 9% | 1% |
| Ascendant | 7% | - | 4% | - | 14% | 21% | 35% | 19% |

The Engine and Ascendant rows lean on aether on purpose: the extractors pour it in from
the Starfall Age on (0.6 per second each), so Engine machines, towers and the computer
techs take 20-100 of it in place of iron, and Ascendant content asks for more aether than
iron. Iron is the metal that runs out first (2-3 veins at home, the rest contested). The
Ironhold Thanes' late uniques are the deliberate exception: they stay iron-heavy, which is
what the Thanes' iron bonus and ore trickles are for.

### 4.2 Map layout and resource nodes

The map generator (engine side) places these node ids. The land is 224-384 m across on
the default Medium size (2 to 8 players), 176-304 m on Small, 288-496 m on Large and
360-616 m on Huge; coastal maps add a band of open sea beyond it on one or two sides.
[maps-and-terrain.md](maps-and-terrain.md) has the options, rivers, mountains and the five
meteor sites in full.

| id | name | resource | amount | box m | how |
|---|---|---|---|---|---|
| `oak` | Oak | wood | 130 | 1 x 5 x 1 | gather |
| `pine` | Pine | wood | 120 | 0.9 x 6 x 0.9 | gather |
| `palm` | Palm | wood | 100 | 0.8 x 5.5 x 0.8 | gather |
| `berry_bush` | Berry Bush | food | 300 | 1.4 x 1 x 1.4 | gather |
| `stone_outcrop` | Stone Outcrop | stone | 1000 | 2.4 x 1.6 x 2.4 | gather |
| `copper_vein` | Copper Vein | copper | 800 | 2.2 x 1.3 x 2.2 | gather |
| `iron_vein` | Iron Vein | iron | 900 | 2.2 x 1.4 x 2.2 | gather |
| `silicon_deposit` | Quartz Sand | silicon | 800 | 2.4 x 0.9 x 2.4 | gather |
| `aether_crystal` | Aether Crystal | aether | 400 | 1.4 x 2.6 x 1.4 | gather |
| `aether_vent` | Aether Vent | aether | 6000 | 3 x 0.4 x 3 | needs `aether_extractor` |
| `adamant_meteor` | Adamant Meteor | adamant | 1600 | 3 x 2.2 x 3 | gather |

What the generator (`crates/af_map/src/mapgen`) places, and what that means per player.
Node amounts are sized to these counts:

| Zone | What sits there | Per player |
|---|---|---|
| **Start area** (mirrored for every player) | 6-8 berry bushes 12 m from the town centre; 2-3 stone outcrops, 2-3 copper veins and 2-3 iron veins 18-22 m out on the flanks; a band of 40-80 oaks and pines 20-28 m behind; a dune patch with 2-3 quartz sand deposits and palms about 38 m out; 3 aether crystals 45-55 m out toward the meteor site, a few metres short of it (closer on Small lands) | about 2100 food in berries, 2000-3000 stone (the only stone on the map), 1600-2400 copper, 1800-2700 iron, 1600-2400 silicon, 1200 aether, 5000-10000 wood |
| **Wilds** | 1-2 dune regions with 2-3 quartz sand deposits and palms each (one more on Large and Huge); 1-2 cliff features (one more on Large, two on Huge), each highland topped by 1 iron and 1 copper vein (ramps, or Climbers); impassable grey mountain ranges (about 85-160 m long on Medium, up to 260 m on Huge, with one or two 12-16 m passes), big single massifs and massifs on the edges and in the corners, about 5 % of the land (more on bigger maps, less on Small); scattered forests, half of them with bramble thickets; one or two rivers across the land with fords at most 36 m apart; ponds or lakes by the water setting; marsh by rivers and lakes; open sea beyond one or two edges on coastal maps | shared: a few more silicon, iron and copper deposits |
| **Contested spots** (one between each pair of neighbouring starts; two in a 1v1) | 1 `aether_vent`, 1 `adamant_meteor`, 2 iron veins and 1-2 copper veins: small, open, hard to wall (at the Shattered Fall each is a small rimmed crater with 2 aether crystals as well) | shared with one neighbour |
| **The meteor site** (centre) | 8-12 `aether_crystal`, 1-3 `aether_vent`, 2-4 `adamant_meteor` on a scar floor. The Starfall Crater: a cliff rim (11% of the land side) with gaps; the Shattered Fall: a smaller crater with half of that, the rest at the spots; the Crater Lake: a flooded crater with the vents and meteors on an island reached by ford causeways; the Skyfall Scar: three basins in a line; the Starspire: an impassable spire on an open ring | shared by everyone; with the contested spots about 2000-4800 adamant per player (8 to 2 players) |

### 4.3 How aether and adamant flow

- **Banner Age: crystals.** Each start has 3 aether crystals (400 each) on the way to
  the crater, and the crater floor holds 8-12 more. A peasant gathers 0.3/s raw, about
  0.24/s with a mining camp beside the crystals, so the 100 aether for `to_starfall` is
  three workers for two and a half minutes. Scouting and skirmishing in the centre
  start here.
- **Starfall Age: vents.** An `aether_extractor` (150 wood, 100 stone, 100 copper)
  caps an `aether_vent` and trickles 0.6 aether/s (36 per minute, about two and a half
  crystal workers) plus a seep of 0.06 adamant/s (3.6 per minute, about 220 an hour)
  for as long as it stands. It repays its price in under ten minutes. It is also an
  aether drop site. Extractors are the "claim the centre" mechanic: they are what
  armies attack and defend. Vents hold 6000.
- **Engine Age: silicon and adamant.** Silicon comes from the player's own quartz sand
  (about 38 m out, 1600-2400 per start) and the dune regions; adamant from meteors
  (0.18/s raw, about 0.16/s worked from a camp, 1600 each) in the crater and at the
  contested spots, and from the extractors' seep. The 200 adamant for `to_ascendant` is
  about five peasants for four minutes, or fewer engineers with `adamant_cutting`.
  Aether keeps flowing in from the extractors (one per age past Banner is a normal
  holding), so the Engine Age spends it too: 20-50 on every machine and rifle squad,
  50-100 on the Engine Works, laboratory, airfield, turrets and computer techs, and 450
  on `to_ascendant`.
- **Ascendant Age: everything costs adamant and aether.** The crater decides the
  endgame. The crater meteors and the contested spots hold roughly 2000-4800 adamant
  per player for the whole game, against an Ascendant age where 19% of all content
  cost is adamant (the age-up 200, a starport 100, the late techs 50-200 each, a
  faction giant several hundred) and 35% is aether (more than iron on every common
  Ascendant unit). Once the meteors are mined out, the extractor seep is the only
  renewable adamant, so holding the crater still matters after the meteors are gone;
  aether, by contrast, barely runs out (6000 per vent).
- `star_tithe` (temple) gives a small aether trickle for players locked out of the centre.

### 4.4 Population and start

| Item | Value |
|---|---|
| Start | 300 food, 300 wood, 150 stone, 100 copper; 5 peasants + 1 scout rider; one town centre |
| Housing | house +10, manor +20, town centre +15, citadel +20, castle +10 |
| Population cap | 300 (engine) |
| Pop per model | workers and infantry 1; cavalry and gatling crews 2; siege 3 (plasma artillery 4); flyers 1 (drones) to 5 (gunships, star bombers); tanks and mechs 3-6; titan 12 |
| Squads | `cost` is paid once per training; `pop` is per model (a rifle squad of 4 takes 4 pop) |

### 4.5 Pacing model

Targets for a competent player in a 1v1: one town centre training workers without a
break, houses on time, the economy upgrades bought as they come, and a sixth of the
income (Hearth, Banner) to a third (Starfall, Engine) going into army and towers. The
computer players are held to their own, later band (measured below).

| Age reached | Banner | Starfall | Engine | Ascendant |
|---|---|---|---|---|
| target, competent player | 5-7 min | 11-14 min | 18-22 min | 24-28 min |
| model (below) | 6:35 | 13:10 | 20:25 | 25:45 |
| target, computer players | about 6 | 13-15 | 20-24 | 28-34 |

**Worker income.** A worker fills its load at the raw rate `r`, walks to the drop site
and back, and unloads. With carry `C` and a trip time `T`, its income is
`e = C / (C / r + T)`. A camp, mill or town centre beside the work (5-6 m each way at
2.4 m/s plus about 3 s of stopping and turning) makes `T` about 8 s:

| | food | wood | stone, copper | iron | silicon | aether | adamant |
|---|---|---|---|---|---|---|---|
| peasant raw `r` /s | 0.7 | 0.75 | 0.5 | 0.45 | 0.4 | 0.3 | 0.18 |
| `e` /s (C 10, T 8 s) | 0.45 | 0.47 | 0.36 | 0.33 | 0.30 | 0.24 | 0.16 |
| `e` /s with Wheelbarrow and Handcart (C 17, speed x1.21) | 0.55 | 0.58 | 0.42 | 0.38 | 0.35 | 0.27 | 0.17 |

The headless AI measures 0.37-0.46/s per worker over its first ten minutes, which is
`T` of 8-10 s. Faction bonuses and gather techs multiply `r`.

**Budget per phase.** A phase runs from one age-up click to the next, so it includes the
previous age-up's research, during which the town centre trains no workers. Rounded
figures from a one-second-step model of the rules above: workers at 0.45 per second
with a tenth of their time spent building, the town centre training one every 14 s up
to the phase's worker count whenever it is not researching or upgrading, the army's
share taken off the top, and the age-up clicked as soon as the phase's purchases and
the age-up are paid for:

| Phase | Clicks | Workers | Gathered | Main spending | Next age |
|---|---|---|---|---|---|
| Hearth | 0:00-5:05 | 5 -> 24 | 1900, plus the 850 start stock | 19 workers 950, `to_banner` 600, lumber camp and mill 200, two houses 100, two eco techs 300, scout and spears 300 | Banner at 6:35 (90 s research) |
| Banner | 5:05-11:20 | 24 -> 40 | 4700 | 16 workers 800, farms 1100 (the berries run out around minute 7), forge and two mining camps 350, eco techs 900, `to_starfall` 850, army 700 | Starfall at 13:10 (110 s) |
| Starfall | 11:20-18:25 | 40 -> 52 | 7700 | citadel upgrade 525 (75 s of town centre time), aether foundry and a camp on the quartz sand 550, one extractor 350, eco techs 1800, `to_engine` 1250, workers 600, army 2600 | Engine at 20:25 (120 s) |
| Engine | 18:25-23:15 | 52 -> 62 | 6500 | laboratory and a camp at a meteor 650, eco techs 1100, `to_ascendant` 1750, workers 500, army 2500 | Ascendant at 25:45 (150 s) |

With the prices before 2026-10-17 (`to_starfall` 950, citadel upgrade 800 and 90 s,
`to_engine` 1700 and 130 s, `to_ascendant` 2300 and 160 s) the same model puts the
Starfall Age at 13:15, the Engine Age at 21:40 and the Ascendant at 27:35.

Hearth arithmetic, by hand: 300 s at an average of 14.5 workers is 4350 worker-seconds;
take a tenth off for building and multiply by 0.45-0.48 (the axe and collar techs come
early) for 1800-1900 gathered. With the start stock that is about 2700 against about
2450 of spending. Its food (about 1800, less the 300 at the start) comes from the berry
bushes alone (7 x 300 = 2100), so the first farms wait for the Banner Age.

Sensitivity: with `T` = 6 s the ages fall at about 6:20, 12:30, 19:15 and 24:15; with
`T` = 10-12 s at about 7:10, 14:40, 23:10 and 29:20. A player who walks every load home
is one or two minutes late per age.

**The computer players**, measured over 30 headless 1v1s on fixed seeds (every faction
pairing twice, 42-minute games, a report every 15 s). Times are the median over the
players who reached the age, with how many of the 60 did; players who are beaten early
never get there, which is most of the shortfall in the counts:

| 1v1, computer players | Banner | Starfall | Engine | Ascendant |
|---|---|---|---|---|
| AI at the start of round 3, old prices | 6:15 (60) | 14:15 (59) | 26:00 (37) | 34:00 (14) |
| AI at the start of round 3, new prices | 6:15 (60) | 14:00 (59) | 22:30 (43) | 31:37 (26) |
| round-3 AI, build of 22:28, old prices | 6:22 (60) | 15:15 (59) | 25:15 (31) | 33:15 (12) |
| round-3 AI, build of 22:28, new prices | 6:22 (60) | 14:30 (59) | 22:45 (37) | 30:22 (18) |
| round-3 AI, build of 22:40, old prices | 6:52 (60) | 15:45 (57) | 26:22 (30) | 34:30 (13) |
| round-3 AI, build of 22:40, new prices | 6:45 (60) | 15:30 (55) | 25:00 (33) | 34:15 (13) |

("Old prices" are those before 2026-10-17; the round-3 AI was still being tuned while
these were taken, which is why its own baseline moves between builds.) The late age-ups
and the citadel are where the time went. With the old prices the citadel waited about
five minutes into the Starfall Age for 200 wood and 400 stone, and the computer players
then needed another six to seven minutes, research included, for the 1000- and
1400-food age-ups, with food, the resource every purchase wants, near zero. Leaning the
late age-ups on the age's own resources (iron, silicon, aether, adamant), halving the
citadel's stone and taking 100 food off `to_starfall` cut the Starfall-to-Engine phase
by 1:15 to 3:00 (to 8-9 minutes) in every build measured, and brought the Engine Age
1.5-3.5 minutes earlier. The aether the computer players hold in the Engine Age fell by a
sixth to a third (641 to 475, 537 to 453, 527 to 361 in the three builds). In the
Ascendant Age they still bank aether, silicon, iron and adamant alike (500-1500 aether)
while food sits near zero and the Engine Works and Starport are seldom built: that is
for the AI to fix, since no price can make it spend what it cannot use.

What the content numbers are set for:

- **Food and wood.** Berries (7 x 300) carry the Hearth Age. A farm costs 60 wood and
  holds 1800 food: thirty times its price and about an hour of one farmer, so the switch
  to farms costs about 1100 wood in the Banner Age and almost nothing after that. Wood is
  the one resource every age spends (houses, camps, farms, production buildings, bows and
  siege frames), so a woodcutter is a little faster than a farmer (0.47 against 0.45 per
  second), as in Age of Empires.
- **Deposits.** Each start holds 2-3 outcrops, veins and quartz deposits of 800-1000
  (section 4.2). That carries a normal game into the Engine Age before anyone must leave
  home for stone, copper or iron; stone exists nowhere else, and iron is the metal that
  runs short first, which is why the late ages spend aether where they used to spend iron.
- **Slow resources.** Raw rates fall from 0.75 (wood) and 0.7 (food) to 0.18
  (adamant), so an hour of a worker buys about 1600 food but about 570 adamant. Each
  age-up asks for a few minutes of a handful of workers, or of the extractors, on its
  rare resource: 100 aether is three crystal workers for two and a half minutes, 200
  aether one extractor for five and a half, 450 aether two extractors for six, and 200
  adamant five meteor workers for four minutes.
- **Age-ups.** Research takes 90, 110, 120 and 150 s, and the town centre trains nothing
  meanwhile. `to_starfall` is 600F 150Cu 100Ae. The late prices (`to_engine` 600F 250Fe
  200Si 200Ae; `to_ascendant` 800F 300Si 450Ae 200Ad) ask for less food than an army
  does and more of the late resources, so the Engine and Ascendant ages land about 7
  and 5 minutes after the age before for a competent player, with the citadel upgrade
  (150W 200S 100Fe 75Ae, 75 s), foundry and laboratory paid on the way.
- **Trickles.** An aether extractor (350) repays itself in under ten minutes at 0.6
  aether/s. The faction trickles (Ironhold delvings, Starborn wayshrines, the Tide and
  Steppe trade techs) are smaller, from a fraction of a worker to about one and a half
  workers each, and are bought for the long game.

---

## 5. Combat model

### 5.1 Damage

```
damage taken = weapon.damage
             x product of weapon.bonus multipliers whose tag the target has
             x (1 - target.armor[damage type])        armour is 0.0 .. 0.9
dps          = damage x volley / cooldown
```

Shields (`ShieldDef`) absorb damage before hp and regenerate after `delay` seconds
without being hit. Techs add to armour; the engine should clamp armour to 0.9 after
techs. `energy_resist` on shields is left at 0 everywhere until its semantics are
settled.

| Damage type | Delivered by | Typical armour |
|---|---|---|
| Slash | swords, axes, halberds, cavalry sabres | 0-15% light, 30-40% heavy infantry and knights, 60-80% machines and buildings |
| Pierce | arrows, bolts, bullets, spears, pikes, lances, ballista and rail slugs | 0-15% light, 20-35% heavy, 55-85% siege, machines and buildings |
| Crush | clubs, mauls, sling stones, catapults, cannon, autocannon, stomps | low on almost everything; 10-50% on stone and later buildings, tanks and titans. The building-breaking type |
| Fire | grenades, firepots, bombs, rockets, flamers, flak, plasma | 0-25% on troops, 30-50% on machines, 0-10% timber, 30-50% stone and later walls |
| Energy | aether lances, tesla, lasers, beams, EMP | 0-40%; almost nothing has much, which is why late weapons hurt |

### 5.2 Bonus conventions

| Counter | Multiplier | Where |
|---|---|---|
| Polearms vs cavalry | x2.5-3 (also x2 vs Beast, x1.5 vs Giant) | `short_spear`, `pike`, `halberd` |
| Missile troops vs unshielded polearms | x1.5 vs Spear | `short_bow`, `crossbow`, `arquebus` |
| Skirmishers vs missile troops | x2.5 vs Ranged | `javelin` |
| Siege vs buildings | x2-4 vs Building | ram x4, mangonel x2, bombard x3.5, trebuchet x4, bombs and aether charges x2.5, plasma mortar x3.5 |
| Anti-siege, anti-giant | x2.5 vs Siege; x1.5 vs Giant | `ballista_bolt`, `railgun`, `star_cannon` |
| Anti-air | air-only targeting, x2 vs Flying, x1.5 vs Heavy (so flak also answers gunships and star bombers) | `flak_cannon`, `battery_flak` |
| Energy / beams vs machines | x1.25-1.6 vs Mechanical | `aether_lance`, `laser_lance`, `turret_plasma`, `tesla_coil` |
| Mauls and EMP vs machines | x2 / x2.5 vs Mechanical; EMP x2 vs Shielded | `shock_maul`, `halberd`, `emp_pulse` |
| Fire vs infantry and buildings | x1.2-1.5 | `grenade`, `fire_bomb`, `flame_projector`, `gatling` (x1.5 vs Infantry) |
| Missiles vs buildings | x0.2-0.25 | every bow, gun and tower missile |

### 5.3 Targeting rule

Straight-firing weapons (Bolt, Beam, Homing, Chain) may be given `air: true`;
lobbed and ground-bound ones (Arc, Cannon, Bomb, Melee) never hit flyers. In the
common pool every bow, crossbow and gun can shoot up (StarCraft-style), siege and
melee cannot, and flak shoots only flyers. Every tower hits air except the bombard
tower. Hover units are ground targets.

### 5.4 Weapon kinds as boxes

| Kind | What the player sees | Common examples |
|---|---|---|
| Melee Swing | the held weapon box sweeps horizontally through `arc_deg`; may `cleave` extra targets | work axe, arming sword, halberd |
| Melee Thrust | the weapon box slides forward and back | spear, pike, war lance, aether lance |
| Melee Smash | the box lifts and slams; small splash at the impact point | cudgel, ram, clockwork fists, shock maul |
| Bolt | a thin box flies straight and always hits the locked target | arrow 0.04x0.04x0.6, bullet tracer 0.04x0.04x0.3, glowing plasma bolt |
| Arc | a chunky box lobbed on a parabola to the predicted ground point; can miss | sling stone, mangonel rock, grenade, trebuchet boulder, plasma ball |
| Cannon | a dense box or slug, fast and flat to a ground point, punching through `pierce` units, then bursting | ballista bolt, bombard ball, landship shell, railgun slug |
| Beam | a box stretched from muzzle to target for `duration`, glowing | laser lance, pulse laser, star cannon, plasma lance turret |
| Bomb | a box released at the flyer's altitude falls straight down | firepots (2.5 m blast), aerial bombs (3 m), aether charges (5 m, the largest blast in the game) |
| Homing | an elongated box that steers | rocket salvo, missile pods |
| Spray | a cone of small glowing cubes | flame projector |
| Nova | a flat ring box expanding to `range` around the source | titan stomp, EMP pulse |
| Chain | short glowing segments jumping target to target | tesla coil |

Every weapon has a `look` (the box held or mounted: sword, bow, pike, barrel,
emitter, offset given for its main user) and, where it fires something, a
`projectile` (`glow: true` for energy, plasma and flame).

### 5.5 Counters per age

| Age | Core triangle | Specialists and answers |
|---|---|---|
| Hearth | spearman > scout rider > archer/slinger > spearman | militia bands beat spearmen in melee; slingers soften militia and palisades; the town centre's arrows punish early raids |
| Banner | pikeman > knight/horse archer > crossbowman > pikeman | men-at-arms beat pikes and skirmishers; javelineers hunt archers and horse archers; rams break walls but die to swords; ballistae kill siege; mangonels punish blobs |
| Starfall | halberdier > aether lancer > arquebusier > halberdier | automatons shrug off bullets but lose to lancers and halberds; grenadiers and war balloons punish blobs and burn timber; bombards and trebuchets out-range towers; crossbows and guns shoot balloons down |
| Engine | landship/mech walker > rifle squad and gatling crew; rifle squad (x1.35 vs Heavy) > aether lancer; aether lancer (x1.6 vs Mechanical) > landship/mech walker. Gatlings beat rifles up close, rifles out-range gatlings | each faction's own heavy foot (see the faction docs) and halberds (x2 vs Mechanical) back the lancers against machines; flak wagons and batteries shred gyros and bombers; mech walkers hunt flyers and trade evenly with landships; biplane bombers punish packed rifle squads; rockets, landship cannon and bombers level buildings |
| Ascendant | storm walker > shield trooper; hover tank > storm walker; gunship > hover tank (which cannot shoot up) and storm walker; shield trooper > gunship; starfighter > gunship and drones | shield troopers shoot air and beat drones and star bombers; flak (x1.5 vs Heavy), tesla and EMP delete drone swarms and bring down gunships and star bombers; star bombers' aether charges flatten blobs and fortresses; the railgun (x1.5 vs Giant) and laser lance (x1.25 vs Giant) answer the faction giants, polearms only slow one; plasma artillery breaks fortresses |

Stat-only scores below use Lanchester's square law at equal cost. Range, kiting and
micro are ignored, so ranged units do better in play than here. Against squads,
chain jumps, novas, sprays, cleaves and splash count the extra models they reach.
Above 1 means A wins an even-cost brawl. Rows with `shock_trooper` or `titan` score the
two benchmarks that no faction trains (section 9.1).

| A | B | A's score (>1 wins) | A dps into B | B dps into A |
|---|---|---|---|---|
| `spearman` | `scout_rider` | 2.35 | 9.3 | 3.5 |
| `scout_rider` | `archer` | 2.65 | 5.8 | 3.2 |
| `archer` | `spearman` | 1.23 | 5.6 | 3.6 |
| `militia` | `spearman` | 1.31 | 3.3 | 3.8 |
| `scout_rider` | `slinger` | 1.60 | 5.8 | 2.2 |
| `slinger` | `militia` | 1.00 | 2.7 | 3.3 |
| `archer` | `militia` | 0.56 | 3.6 | 3.3 |
| `pikeman` | `knight` | 2.06 | 11.2 | 9.6 |
| `knight` | `crossbowman` | 1.79 | 11.8 | 6.1 |
| `crossbowman` | `pikeman` | 1.27 | 9.5 | 4.5 |
| `man_at_arms` | `pikeman` | 1.35 | 7.1 | 4 |
| `javelineer` | `crossbowman` | 2.80 | 9.8 | 4.9 |
| `javelineer` | `horse_archer` | 2.75 | 9.2 | 4.7 |
| `horse_archer` | `man_at_arms` | 0.54 | 5 | 7.5 |
| `knight` | `man_at_arms` | 1.10 | 8 | 5.4 |
| `crossbowman` | `man_at_arms` | 0.66 | 6.6 | 7.9 |
| `pikeman` | `horse_archer` | 3.49 | 12.8 | 6 |
| `halberdier` | `aether_lancer` | 1.68 | 17.1 | 17.5 |
| `aether_lancer` | `arquebusier` | 1.20 | 17.5 | 11.1 |
| `arquebusier` | `halberdier` | 1.10 | 14.7 | 10.3 |
| `aether_lancer` | `clockwork_automaton` | 1.52 | 28 | 13 |
| `halberdier` | `clockwork_automaton` | 1.23 | 14.9 | 14.6 |
| `arquebusier` | `clockwork_automaton` | 0.54 | 9.4 | 16.2 |
| `arquebusier` | `war_balloon` | 1.13 | 11.8 | 12 |
| `clockwork_automaton` | `arquebusier` | 1.86 | 16.2 | 9.4 |
| `arquebusier` | `aether_lancer` | 0.83 | 11.1 | 17.5 |
| `shock_trooper` | `mech_walker` | 1.41 | 34 | 22 |
| `rifle_squad` | `shock_trooper` | 0.85 | 13.2 | 36.4 |
| `gatling_crew` | `rifle_squad` | 1.09 | 28.1 | 10.5 |
| `steam_tank` | `rifle_squad` | 5.20 | 103.6 | 7.1 |
| `shock_trooper` | `steam_tank` | 1.10 | 31.6 | 34.4 |
| `flak_wagon` | `gyrocopter` | 6.49 | 96 | 7.7 |
| `flak_wagon` | `biplane_bomber` | 4.98 | 48 | 18 |
| `gyrocopter` | `rifle_squad` | 0.70 | 10.5 | 12 |
| `mech_walker` | `gyrocopter` | 4.79 | 51.6 | 6.3 |
| `rifle_squad` | `gyrocopter` | 1.43 | 12 | 10.5 |
| `gatling_crew` | `shock_trooper` | 0.96 | 24.4 | 24.3 |
| `mech_walker` | `steam_tank` | 0.97 | 22.3 | 30.3 |
| `rocket_battery` | `steam_tank` | 0.20 | 11.9 | 43.2 |
| `steam_tank` | `shock_trooper` | 0.91 | 34.4 | 31.6 |
| `steam_tank` | `gatling_crew` | 4.12 | 41.2 | 8.8 |
| `mech_walker` | `rifle_squad` | 2.11 | 41.2 | 9.1 |
| `rifle_squad` | `aether_lancer` | 1.51 | 13.2 | 17.5 |
| `aether_lancer` | `steam_tank` | 1.10 | 25.2 | 32 |
| `aether_lancer` | `mech_walker` | 1.22 | 25.2 | 22 |
| `biplane_bomber` | `rifle_squad` | 1.42 | 72 | 11.2 |
| `gatling_crew` | `biplane_bomber` | 1.90 | 18.8 | 18 |
| `hover_tank` | `storm_walker` | 1.64 | 52 | 41.2 |
| `shield_trooper` | `drone_swarm` | 1.38 | 36.7 | 4.7 |
| `starfighter` | `gunship` | 1.12 | 27.4 | 65.6 |
| `starfighter` | `drone_swarm` | 2.23 | 34.3 | 6.2 |
| `storm_walker` | `shield_trooper` | 1.29 | 45.1 | 23.8 |
| `hover_tank` | `shield_trooper` | 1.21 | 32.5 | 23.8 |
| `shield_trooper` | `hover_tank` | 0.82 | 23.8 | 32.5 |
| `gunship` | `hover_tank` | A cannot be hurt | 28.4 | 0 |
| `halberdier` | `titan` | 0.59 | 10.3 | 104.4 |
| `storm_walker` | `titan` | 1.13 | 43.8 | 91.5 |
| `shield_trooper` | `titan` | 0.77 | 18.3 | 82.8 |
| `gunship` | `shield_trooper` | 0.88 | 26.2 | 25.7 |
| `shield_trooper` | `gunship` | 1.13 | 25.7 | 26.2 |
| `flak_wagon` | `drone_swarm` | 2.51 | 105.6 | 6.2 |
| `storm_walker` | `drone_swarm` | 2.25 | 93.8 | 5 |
| `gunship` | `storm_walker` | 2.39 | 28.4 | 15 |
| `flak_wagon` | `gunship` | 1.26 | 50.4 | 39.4 |
| `flak_wagon` | `star_bomber` | 2.73 | 50.4 | 27 |
| `shield_trooper` | `star_bomber` | 1.64 | 25.7 | 27 |
| `starfighter` | `star_bomber` | A cannot be hurt | 27.4 | 0 |
| `star_bomber` | `rifle_squad` | 3.03 | 129.6 | 12.2 |
| `star_bomber` | `shield_trooper` | 0.61 | 27 | 25.7 |

---

## 6. Walls and defence

**Walls** are dragged as a line of segments (`wall: true`, `size[0]` is the segment
length). **Gates** are wall segments their owner's units pass through. Each tier
upgrades in place (`BuildingUpgrade` keeps position and hp ratio), so a palisade
line becomes a stone line without re-placing it. Every tier has the same footprint,
so an upgrade never clips a neighbour: wall segments are 3.0 m long and 1.0 m thick,
gates 9.0 m long (three segments) and 1.2 m thick. The depth keeps the line closed on
the 1 m nav grid and the 9 m gate lets anything up to 5 m wide through at any angle,
giants included (section 11):

| Tier | Wall | Gate | Age | Segment hp | Idea |
|---|---|---|---|---|---|
| 1 | `palisade_wall` | `palisade_gate` | Hearth | 250 | cheap stakes, burns |
| 2 | `stone_wall` | `stone_gate` | Banner | 1500 | the real wall; arrows bounce |
| 3 | `reinforced_wall` | `reinforced_gate` | Engine | 3500 | ferro-concrete and plate against shells |
| 4 | `force_wall` | `force_gate` | Ascendant | 4800 + 500 shield | adamant pylons and a barrier field; self-repairs |

**Things that defend walls** stand on or right behind the line: the tower chain
(`watchtower` -> `guard_tower` -> `bombard_tower` -> `gun_turret` -> `plasma_turret`,
one 3.5 x 3.5 m building upgraded in place each age), and the bombard tower in particular,
which is the wall-adjacent bombard that answers siege outside the wall.

**Things that defend the interior**: the `castle` (keep arrows plus a bombard,
garrison 20), the `citadel` (keep arrows), `anti_air_battery` against flyers that
ignore walls, `gun_turret` and `plasma_turret` (both hit air), and the
`shield_generator` (3000-hp regenerating shield, EMP pulse against machines and
drones).

**Breaking in** changes by age: rams (Banner), bombards, trebuchets, grenadiers and
war balloons (Starfall), landships, rockets and biplanes (Engine), plasma
artillery, star bombers and the faction giants (Ascendant). Flyers bypass walls entirely, which is why AA
belongs inside. Seconds for one attacker to bring down one segment (no techs,
shields included):

| attacker | weapon | palisade_wall s | stone_wall s | reinforced_wall s | force_wall s | stone_gate s |
|---|---|---|---|---|---|---|
| `battering_ram` | ram_head | 5 | 35 | 100 | 177 | 52 |
| `mangonel` | mangonel_shot | 21 | 147 | 417 | 736 | 216 |
| `bombard` | bombard_shot | 5 | 36 | 103 | 182 | 53 |
| `trebuchet` | trebuchet_stone | 3 | 25 | 69 | 123 | 36 |
| `war_balloon` | fire_bomb | 10 | 120 | 280 | 385 | 176 |
| `steam_tank` | tank_cannon | 5 | 35 | 100 | 177 | 52 |
| `rocket_battery` | rocket_salvo | 12 | 141 | 328 | 452 | 206 |
| `biplane_bomber` | aerial_bomb | 5 | 60 | 140 | 193 | 88 |
| `plasma_artillery` | plasma_mortar | 4 | 43 | 100 | 138 | 63 |
| `star_bomber` | aether_charge | 3 | 40 | 93 | 128 | 59 |
| `titan` (benchmark) | star_cannon | 2 | 15 | 35 | 61 | 22 |
| `man_at_arms` | arming_sword | 43 | 600 | 1680 | 3180 | 880 |

The table counts one segment. Splash reaches the neighbours too: an aether charge's
5 m blast covers about three segments of a line per bomb, the 2.5-3 m firepots and
aerial bombs about two.

### 6.1 Victory, defeat and reveal

- **Defeat:** a player is out when they own no living unit and no complete
  building that keeps them alive. Walls, gates, pure resource fields (a building
  whose only role is a resource field: no training, research, drop site or
  weapons, such as a farm) and unfinished foundations do not count. A harbour
  does count, because it is also a drop site and trains troops. The rule is
  `keeps_player_alive` in `crates/af_core/src/ext/economy.rs`.
- **Reveal:** a player who is still in the game but owns no complete production
  building (one that trains units, or researches without being a drop site, so
  camps and mills do not count) has every remaining building revealed to all
  teams until a new production building is complete. Everyone gets an Alert
  notice when it happens. This is what lets an attacker find and finish a
  hidden last building.
- **Victory:** when every undefeated player is on one team, that team wins. A
  one-player sandbox never ends.

---

## 7. Terrain and mobility

The simulation is 2D; terrain height is visual only. A unit's mobility is its
single locomotion flag plus any terrain traits its faction grants that locomotion.
Flying ignores terrain and ground collision. Water, fords, the sea, marsh,
thicket, cliff and mountains are not buildable.

| terrain | name | height m | build | Foot | Mounted | Wheeled | Tracked | Hover | Flying | Amphibious (Foot+) | Climber (Foot+) | Forestwalker (Foot+) | Dunerunner (Foot+) | Marshwalker (Foot+) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `grass` | Grassland | 0 | yes | x1 | x1 | x1 | x1 | x1 | x1 |  |  |  |  |  |
| `sand` | Dunes | 0.05 | yes | x0.9 | x0.7 | x0.7 | x0.85 | x1 | x1 |  |  |  | x1 |  |
| `shallow_water` | Shallows | -0.25 | no | x0.55 | x0.55 | - | x0.6 | x0.8 | x1 | x0.9 |  |  |  |  |
| `deep_water` | Deep Water | -0.6 | no | - | - | - | - | x0.8 | x1 | x0.7 |  |  |  |  |
| `marsh` | Marsh | -0.08 | no | x0.6 | x0.6 | - | x0.6 | x0.8 | x1 |  |  |  |  | x1 |
| `thicket` | Bramble Thicket | 1.2 | no | x0.5 | - | - | - | - | x1 |  |  | x1.1 |  |  |
| `cliff` | Rocky Highland | 5 | no | - | - | - | - | - | x1 |  | x0.6 |  |  |  |
| `mountain` | Mountains | 9 | no | - | - | - | - | - | x1 |  | - |  |  |  |
| `ford` | Ford | -0.2 | no | x0.7 | x0.7 | x0.6 | x0.7 | x0.9 | x1 | x0.9 |  |  |  |  |
| `scorched` | Starfall Scar | 0 | yes | x1 | x1 | x1 | x1 | x1 | x1 |  |  |  |  |  |
| `ocean` | Open Sea | -0.6 | no | - | - | - | - | x0.8 | x1 | x0.7 |  |  |  |  |

Cells show the speed multiplier ("-" impassable). Trait columns show a Foot unit
holding that trait. Mountains are the true obstacle: only flyers cross them, with a
pass through every long range. Fords cross every river at most 36 m apart and are the
one way wheeled machines get over a river. The scar is the burnt floor of the meteor
sites; the open sea lies beyond the coasts of coastal maps. Faction traits:

| Faction | Trait | Applies to | Effect |
|---|---|---|---|
| ironhold | Climber | Foot | infantry scale cliffs (x0.6) and take highland ore and flanks |
| sylvan | Forestwalker | Foot, Mounted | pass thickets at x1.1: ambushes from the brambles |
| dune | Dunerunner | Foot, Mounted, Wheeled | no sand penalty |
| tide | Amphibious; Marshwalker | Foot and Mounted; Foot | marines wade deep water (x0.7) and shallows (x0.9); marsh at full speed on foot |
| steppe | Marshwalker; Dunerunner | Mounted; Mounted | horse lords ignore marsh and sand |
| starborn | none | - | instead: the earliest and best flyers and hover units |

Hover units cross shallows, deep water, the sea and marsh at x0.8 (full speed on
grass and sand) but cannot enter thickets, cliffs or mountains. Tracked machines wade
shallows and marsh slowly; wheeled machines need dry land or a ford.

---

## 8. Balance baselines

Targets the pool was built to (the faction designers should hit the same bands):

| Age | Line infantry hp | dps | cost | Notes |
|---|---|---|---|---|
| Hearth | 40-60 | 4-6 | 60-90 | squads are lighter per model and cheaper per model |
| Banner | 70-110 | 7-11 | 100-160 | knights 150-180 hp, ~10 dps, ~200 |
| Starfall | 100-170 | 12-18 | 150-260 incl. 10-40 aether | |
| Engine | 160-320 | 18-30 | iron and silicon, plus 20-50 aether | tanks 600-900 hp |
| Ascendant | 250-450 (+50-150 shield) | 30-50 | aether and adamant | giants 2500-4000 hp, titans 4500-6500 |

| Category | Values |
|---|---|
| Speeds (m/s) | workers 2.4-2.6; infantry 2.5-3.2 (gatling crew 2.2); cavalry 5.6-6.2; siege 1.4-2.4; tanks 3-4; mechs 2-3; hover 5-7; flyers 6-9 (war balloon 3.5, deliberately slow) |
| Ranges (m, edge to edge) | melee 0.5-1.5; bows 8-10; crossbow 9; arquebus 10; rifles 12; landship cannon 13; laser lance 14; railgun 16; siege/artillery 14-22 with min range; towers 10-13; flak 14 |
| Vision (m) | 8-16 for ground units (rams 6); scouts, flyers and anti-air 14-18. Direct-fire units see at least as far as they shoot (rifle squad 12, landship 13, hover tank 14, storm walker 16), and the range techs add the same vision; siege and artillery out-range their sight and need a spotter |
| Buildings hp | house 550, barracks 1600, town centre 2600 (citadel 4200), castle 5000; palisade 250, stone wall 1500, reinforced 3500, force 4800; towers 800-2500 |
| Box scale (m) | worker 0.6x1.2x0.6, infantry 0.7x1.4x0.7, cavalry 0.9x1.7x1.9, siege 1.8-2.9 wide, tanks/mechs 2.6-2.9, titan 6x12x5; house 4x3x4, barracks 8x5x8, town centre 9x6x9 (citadel 9x10x9), wall segment 3 long x 1.0 thick, gate 9 long x 1.2 thick, tower 3.5 wide and 8-9.5 tall, castle 12x10x12; upgrades keep x and z and change only height |
| Width limits (m) | ground units other than giants under 3.0 wide (2.9 at most); giants at most 5.0 wide. See section 11 for the nav-grid reason |
| Altitude (m) | flyers 12-14 (drones and war balloons 12, gyrocopters 12.5, biplanes and gunships 13, star bombers 13.5, starfighters 14), above the tallest box in the game (12 m); hover 0.5-1.0 |

Measured value bands of the common pool (line combat units: no workers, support,
siege, bombers or giants; squads counted whole; with an even count the median is the
upper of the two middle values):

| age | line units | dps/cost range | median | ehp/cost range | median |
|---|---|---|---|---|---|
| Hearth | 5 | 0.048 - 0.067 | 0.057 | 0.60 - 0.94 | 0.73 |
| Banner | 6 | 0.044 - 0.067 | 0.054 | 0.58 - 0.90 | 0.76 |
| Starfall | 5 | 0.059 - 0.082 | 0.067 | 0.66 - 1.19 | 0.81 |
| Engine | 7 | 0.056 - 0.094 | 0.074 | 0.80 - 1.43 | 0.95 |
| Ascendant | 6 | 0.058 - 0.099 | 0.095 | 1.11 - 1.89 | 1.45 |

Strength per pop and per cost of the same units, as `sqrt(dps x ehp)` of the whole
squad (Lanchester strength grows with dps times hp) divided by pop or by cost:

| age | line units | strength per pop: range | median | per cost: range | median |
|---|---|---|---|---|---|
| Hearth | 5 | 8 - 14 | 11 | 0.173 - 0.225 | 0.206 |
| Banner | 6 | 14 - 28 | 22 | 0.177 - 0.225 | 0.207 |
| Starfall | 5 | 24 - 37 | 30 | 0.225 - 0.267 | 0.233 |
| Engine | 7 | 25 - 76 | 35 | 0.250 - 0.306 | 0.264 |
| Ascendant | 6 | 21 - 124 | 43 | 0.304 - 0.349 | 0.335 |

Each age is clearly stronger per pop than the one before (x2, then x1.2-1.4), and an
Ascendant army is about four times a Hearth army of the same population, not ten.
Per cost the ages climb more slowly, so the late resources, not the population cap,
are what limit a late army. Within an age, infantry is dense in strength per pop
(the shield trooper at 124, the benchmark shock trooper at 76) and swarms are thin (the
drone swarm at 21, six models of one pop each): infantry is pop-efficient and
resource-hungry, swarms and machines the reverse.

Bombers (units whose only weapon is a Bomb) are priced like siege: the biplane
bomber (0.042 dps/cost, 0.67 ehp/cost) and the star bomber (0.046, 1.15) are paid
for by their 3 m and 5 m blasts and x2.5 against buildings, which raw dps does not show.

Rules of thumb: within an age, dps/cost and ehp/cost (hp plus shield, per cost)
stay within a factor of about 1.5 of the median. Units above the median on one axis
sit below it on the other, or pay with range, speed or a hard counter. Ranged units
sit 20-40% below melee on raw efficiency and make it back through range. Siege and
giants are priced on what they do to buildings and armies, not on raw dps.
Deliberate exception: the `javelineer` (4.6 dps) sits below the Banner dps band and
below both medians; it is a counter unit priced on its x2.5 against Ranged, its 30%
pierce armour and its speed, not a line unit.

---

## 9. The common pool

Faction designers choose from these. Ids are fixed; tables are grouped by the age
the item unlocks.

### 9.1 Units (43)

Costs are per training (a squad costs this once). `dps/model` sums all weapons
before armour; `dps/cost` and `ehp/cost` count the whole squad.

**Hearth Age**

| id | name | tags | squad | pop/model | cost (per training) | train s | hp/model | armour % | speed | moves | weapons | dps/model | range | trained at | dps/cost | ehp/cost |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `peasant` | Peasant | Worker, Infantry, Light |  | 1 | 50F | 14 | 30 | P5 | 2.4 | Foot | work_axe (gathers) | 2 | 0.6 | town_center, citadel | 0.040 | 0.60 |
| `scout_rider` | Scout Rider | Cavalry, Scout, Light, Melee |  | 2 | 80F | 22 | 75 | S10 P15 | 6.2 | Mounted | riding_sword | 3.8 | 0.8 | town_center, citadel, stable | 0.048 | 0.94 |
| `militia` | Militia Band | Infantry, Melee, Light | 3 | 1 | 120F 30W | 24 | 38 | S10 P5 | 2.7 | Foot | club | 3.3 | 0.7 | barracks | 0.067 | 0.76 |
| `spearman` | Spearman | Infantry, Melee, Spear, Light |  | 1 | 45F 25W | 20 | 50 | S10 | 2.8 | Foot | short_spear | 4 | 1 | barracks | 0.057 | 0.71 |
| `slinger` | Slinger Band | Infantry, Ranged, Light | 3 | 1 | 95F 20W | 24 | 28 | P5 | 3 | Foot | sling | 2.2 | 7 | archery_range | 0.058 | 0.73 |
| `archer` | Archer | Infantry, Ranged, Light |  | 1 | 30F 45W | 22 | 45 | P10 | 2.9 | Foot | short_bow | 3.8 | 8 | archery_range | 0.050 | 0.60 |

**Banner Age**

| id | name | tags | squad | pop/model | cost (per training) | train s | hp/model | armour % | speed | moves | weapons | dps/model | range | trained at | dps/cost | ehp/cost |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `man_at_arms` | Man-at-Arms | Infantry, Melee, Heavy |  | 1 | 70F 55Fe | 26 | 95 | S30 P25 C10 | 2.7 | Foot | arming_sword | 8.3 | 0.8 | barracks | 0.067 | 0.76 |
| `pikeman` | Pikeman | Infantry, Melee, Spear |  | 1 | 50F 35W 15Cu | 24 | 80 | S15 P10 | 2.7 | Foot | pike | 5.3 | 1.5 | barracks | 0.053 | 0.80 |
| `crossbowman` | Crossbowman | Infantry, Ranged |  | 1 | 35F 55W 30Cu | 26 | 70 | S5 P15 | 2.8 | Foot | crossbow | 7 | 9 | archery_range | 0.058 | 0.58 |
| `javelineer` | Javelineer | Infantry, Ranged, Light |  | 1 | 45F 60W | 22 | 75 | S5 P30 | 3 | Foot | javelin | 4.6 | 8 | archery_range | 0.044 | 0.71 |
| `knight` | Knight | Cavalry, Melee, Heavy |  | 2 | 120F 80Fe | 32 | 180 | S35 P30 C15 | 5.6 | Mounted | war_lance | 10.7 | 1.2 | stable, castle | 0.054 | 0.90 |
| `horse_archer` | Horse Archer | Cavalry, Ranged, Light |  | 2 | 60F 55W 35Cu | 30 | 110 | S10 P20 | 6 | Mounted | horse_bow | 6.7 | 8 | stable | 0.044 | 0.73 |
| `monk` | Monk | Infantry, Support |  | 1 | 60F 60Cu | 30 | 50 | - | 2.6 | Foot |  (heals one target 3/s) | 0 | 0 | temple | 0.000 | 0.42 |
| `battering_ram` | Battering Ram | Siege, Heavy |  | 3 | 160W 60Cu | 35 | 320 | S30 P85 C10 | 1.5 | Wheeled | ram_head | 12.5 | 0.5 | siege_workshop | 0.057 | 1.45 |
| `mangonel` | Mangonel | Siege |  | 3 | 160W 100Cu | 40 | 180 | S20 P60 | 1.4 | Wheeled | mangonel_shot | 6 | 15 | siege_workshop | 0.023 | 0.69 |
| `ballista` | Ballista | Siege |  | 3 | 150W 70Cu | 38 | 160 | S20 P55 | 1.5 | Wheeled | ballista_bolt | 7.6 | 14 | siege_workshop | 0.034 | 0.73 |

**Starfall Age**

| id | name | tags | squad | pop/model | cost (per training) | train s | hp/model | armour % | speed | moves | weapons | dps/model | range | trained at | dps/cost | ehp/cost |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `halberdier` | Halberdier | Infantry, Melee, Spear |  | 1 | 70F 40W 30Fe 10Ae | 28 | 120 | S20 P25 C10 | 2.7 | Foot | halberd | 11.4 | 1.4 | barracks | 0.076 | 0.80 |
| `arquebusier` | Arquebusier | Infantry, Ranged |  | 1 | 60F 40W 45Fe 15Ae | 30 | 105 | S10 P15 | 2.6 | Foot | arquebus | 13.1 | 10 | archery_range | 0.082 | 0.66 |
| `grenadier` | Grenadier Pair | Infantry, Ranged | 2 | 1 | 90F 60Cu 40Fe 30Ae | 34 | 95 | S10 P15 F20 | 2.7 | Foot | grenade | 6.5 | 7 | barracks, castle | 0.059 | 0.86 |
| `aether_lancer` | Aether Lancer | Cavalry, Melee, Heavy |  | 2 | 130F 90Fe 40Ae | 36 | 210 | S40 P35 C20 E15 | 6 | Mounted | aether_lance | 17.5 | 1.4 | stable, aether_foundry | 0.067 | 0.81 |
| `clockwork_automaton` | Clockwork Automaton | Mechanical, Melee, Heavy |  | 3 | 120Cu 110Fe 40Ae | 40 | 320 | S35 P45 C15 F20 | 2.2 | Tracked | clockwork_fists | 16.2 | 0.8 | aether_foundry | 0.060 | 1.19 |
| `war_balloon` | War Balloon | Flying, Siege |  | 3 | 140W 90Cu 40Ae | 45 | 260 | S20 P10 | 3.5 | Flying | fire_bomb | 10 | 1 | aether_foundry | 0.037 | 0.96 |
| `bombard` | Bombard | Siege, Heavy |  | 3 | 120W 50Cu 130Fe 20Ae | 45 | 220 | S30 P60 | 1.4 | Wheeled | bombard_shot | 13.8 | 16 | siege_workshop | 0.043 | 0.69 |
| `trebuchet` | Trebuchet | Siege, Heavy |  | 3 | 260W 60S 120Cu | 55 | 250 | S20 P70 | 1.4 | Wheeled | trebuchet_stone | 18 | 22 | siege_workshop, castle | 0.041 | 0.57 |

**Engine Age**

| id | name | tags | squad | pop/model | cost (per training) | train s | hp/model | armour % | speed | moves | weapons | dps/model | range | trained at | dps/cost | ehp/cost |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `engineer` | Engineer | Worker, Infantry, Support |  | 1 | 60F 20Fe 20Si | 18 | 90 | S15 P20 | 2.6 | Foot | work_axe (gathers) | 2 | 0.6 | town_center, citadel | 0.020 | 0.90 |
| `rifle_squad` | Rifle Squad | Infantry, Ranged | 4 | 1 | 260F 100Cu 170Fe 80Si 30Ae | 40 | 160 | S15 P25 F10 | 2.8 | Foot | rifle | 15 | 12 | barracks | 0.094 | 1.00 |
| `gatling_crew` | Gatling Crew | Infantry, Ranged |  | 2 | 110F 50Cu 100Fe 40Si 20Ae | 36 | 260 | S20 P30 F10 | 2.2 | Foot | gatling | 25 | 10 | barracks | 0.078 | 0.81 |
| `shock_trooper` | Shock Trooper (benchmark) | Infantry, Melee, Heavy |  | 1 | 120F 130Fe 50Si | 32 | 240 | S40 P35 C20 F20 E10 | 2.6 | Foot | shock_maul | 24.3 | 0.9 | barracks | 0.081 | 0.80 |
| `steam_tank` | Steam Landship | Mechanical, Heavy |  | 5 | 100W 100Cu 260Fe 100Si 40Ae | 55 | 800 | S70 P65 C35 F30 E10 | 3.2 | Tracked | tank_cannon, flame_projector | 37 | 13 | engine_works | 0.062 | 1.33 |
| `mech_walker` | Mech Walker | Mechanical, Heavy |  | 4 | 80Cu 240Fe 120Si 50Ae | 50 | 700 | S60 P55 C30 F30 E10 | 2.6 | Tracked | autocannon | 27.5 | 11 | engine_works | 0.056 | 1.43 |
| `rocket_battery` | Rocket Battery | Siege, Mechanical |  | 3 | 100W 70Cu 130Fe 80Si 20Ae | 45 | 260 | S20 P40 | 2 | Wheeled | rocket_salvo | 14.2 | 20 | engine_works | 0.036 | 0.65 |
| `flak_wagon` | Flak Wagon | Mechanical, Light |  | 3 | 80W 60Cu 120Fe 60Si 20Ae | 40 | 300 | S30 P45 F10 | 3 | Wheeled | flak_cannon | 24 | 14 | engine_works | 0.071 | 0.88 |
| `gyrocopter` | Gyrocopter Pair | Flying, Mechanical, Light | 2 | 2 | 120W 60Cu 100Fe 80Si 20Ae | 40 | 180 | S20 P20 | 7 | Flying | gyro_guns | 14 | 8 | airfield | 0.074 | 0.95 |
| `biplane_bomber` | Biplane Bomber | Flying, Mechanical |  | 3 | 160W 80Cu 110Fe 100Si 30Ae | 50 | 320 | S20 P25 | 8 | Flying | aerial_bomb | 20 | 1 | airfield | 0.042 | 0.67 |

**Ascendant Age**

| id | name | tags | squad | pop/model | cost (per training) | train s | hp/model | armour % | speed | moves | weapons | dps/model | range | trained at | dps/cost | ehp/cost |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `shield_trooper` | Shield Trooper | Infantry, Ranged, Heavy, Shielded |  | 1 | 150F 70Fe 110Ae 40Ad | 34 | 300+120sh | S30 P35 C20 F25 E25 | 2.8 | Foot | plasma_carbine | 36.7 | 10 | barracks | 0.099 | 1.14 |
| `hover_tank` | Hover Tank | Mechanical, Heavy, Shielded |  | 5 | 130Fe 150Si 220Ae 120Ad | 50 | 900+150sh | S65 P60 C35 F35 E30 | 5.5 | Hover | laser_lance | 43.3 | 14 | engine_works, starport | 0.070 | 1.69 |
| `gunship` | Gunship | Flying, Mechanical, Heavy, Shielded |  | 5 | 100Fe 160Si 190Ae 100Ad | 55 | 700+100sh | S40 P40 C30 F30 E20 | 6.5 | Flying | missile_pod | 35 | 13 | starport | 0.064 | 1.45 |
| `starfighter` | Starfighter Wing | Flying, Mechanical, Light, Shielded | 3 | 2 | 80Fe 150Si 220Ae 90Ad | 45 | 180+50sh | P20 | 9 | Flying | pulse_laser | 17.1 | 9 | starport | 0.095 | 1.28 |
| `plasma_artillery` | Plasma Artillery | Siege, Mechanical, Shielded |  | 4 | 100Fe 150Si 250Ae 60Ad | 55 | 450+80sh | S30 P50 C20 F20 E10 | 2.4 | Tracked | plasma_mortar | 20 | 22 | engine_works | 0.036 | 0.95 |
| `drone_swarm` | Drone Swarm | Flying, Mechanical, Light | 6 | 1 | 50Fe 150Si 150Ae 30Ad | 40 | 70 | - | 8 | Flying | drone_zapper | 6.2 | 6 | starport | 0.099 | 1.11 |
| `star_bomber` | Star Bomber | Flying, Mechanical, Heavy, Shielded |  | 5 | 100Fe 160Si 270Ae 120Ad | 60 | 650+100sh | S40 P40 C30 F30 E20 | 6 | Flying | aether_charge | 30 | 1 | starport | 0.046 | 1.15 |
| `storm_walker` | Storm Walker | Mechanical, Heavy, Shielded |  | 6 | 200Fe 200Si 300Ae 200Ad | 70 | 1500+200sh | S60 P60 C35 F35 E20 | 2.6 | Tracked | railgun, tesla_coil | 52.5 | 16 | engine_works | 0.058 | 1.89 |
| `titan` | Titan (benchmark) | Mechanical, Giant, Heavy, Hero |  | 12 | 500Fe 400Si 900Ae 800Ad | 120 | 6000 | S70 P70 C50 F50 E35 | 2 | Tracked | star_cannon, titan_stomp | 102 | 16 | engine_works | 0.039 | 2.31 |

Line upgrades (AoE style, `UpgradeUnit`) exist only on four infantry pairs:
spearman -> pikeman -> halberdier and archer -> crossbowman -> arquebusier
(`pike_drill`, `halberd_drill`, `crossbow_drill`, `hand_cannon_drill`). Each pair
has equal pop, a per-model cost ratio of 1.6 or less and at most 15 aether of
difference, so converting a cheap army never undercuts the market. Everything else
is trained, not converted: `militia_drill` and `steam_harness` toughen militia and
men-at-arms in place, and shock troopers, hover tanks and storm walkers are only
trained.

**Benchmarks and unclaimed units.** `shock_trooper` and `titan` stay in the pool but
no faction lists them: every faction fields its own heavy Engine foot and its own giant,
and the faction docs score those against these two. Their weapons (`shock_maul`,
`star_cannon`, `titan_stomp`) are therefore unused in play. `star_bomber` is new and
is not yet in any roster; it is proposed for tide, steppe, dune and starborn (for Tide
it is a Bomb carrier that keeps "flyers only in the Ascendant"). Every other common
unit ships in at least one faction.

### 9.2 Buildings (36)

23 core buildings plus 13 defensive tiers: four wall tiers, each with a gate, and
the five-tier tower chain.

**Hearth Age**

| id | name | cost | build s | hp | armour % | role | weapons | trains | techs | upgrade |
|---|---|---|---|---|---|---|---|---|---|---|
| `town_center` | Town Centre | 300W 200S | 150 | 2600 | S60 P80 C10 F10 E20 | drop: all; +15 pop | tc_arrows | peasant, scout_rider, engineer | 2 | -> `citadel` (150W 200S 100Fe 75Ae, 75s) |
| `house` | House | 50W | 25 | 550 | S50 P80 E20 | +10 pop | - | - | - | -> `manor` (75W 50S, 30s) |
| `lumber_camp` | Lumber Camp | 100W | 25 | 600 | S50 P80 E20 | drop: wood | - | - | 2 | - |
| `mining_camp` | Mining Camp | 100W | 25 | 600 | S50 P80 E20 | drop: stone, copper, iron, silicon, aether, adamant | - | - | 7 | - |
| `mill` | Mill | 100W | 30 | 650 | S50 P80 E20 | drop: food | - | - | 6 | - |
| `farm` | Farm | 60W | 15 | 200 | S50 P80 E20 | field 1800 food | - | - | - | - |
| `barracks` | Barracks | 175W | 50 | 1600 | S50 P80 E20 | - | - | militia, spearman, man_at_arms, pikeman, halberdier, grenadier, rifle_squad, gatling_crew, shock_trooper, shield_trooper | 4 | - |
| `archery_range` | Archery Range | 175W | 50 | 1500 | S50 P80 E20 | - | - | slinger, archer, crossbowman, javelineer, arquebusier | 2 | - |
| `palisade_wall` | Palisade | 8W | 6 | 250 | S30 P80 | wall segment | - | - | - | -> `stone_wall` (8S, 15s) |
| `palisade_gate` | Palisade Gate | 30W | 15 | 400 | S30 P80 | gate | - | - | - | -> `stone_gate` (25S, 20s) |
| `watchtower` | Watchtower | 50W 125S | 60 | 800 | S50 P80 E20 | - | longbow | - | - | -> `guard_tower` (50W 100S 50Cu, 40s) |

**Banner Age**

| id | name | cost | build s | hp | armour % | role | weapons | trains | techs | upgrade |
|---|---|---|---|---|---|---|---|---|---|---|
| `stable` | Stable | 175W 25Cu | 50 | 1600 | S50 P80 E20 | - | - | scout_rider, knight, horse_archer, aether_lancer | 1 | - |
| `siege_workshop` | Siege Workshop | 200W 75Cu | 60 | 1700 | S50 P80 E20 | - | - | battering_ram, mangonel, ballista, bombard, trebuchet | 1 | - |
| `forge` | Forge | 150W 50S 50Cu | 45 | 1500 | S65 P85 C15 F30 E25 | - | - | - | 13 | - |
| `temple` | Temple | 100W 200S 50Cu | 60 | 1800 | S65 P85 C15 F30 E25 | - | - | monk | 2 | - |
| `stone_wall` | Stone Wall | 12S | 10 | 1500 | S70 P85 C15 F50 E30 | wall segment | - | - | - | -> `reinforced_wall` (15S 10Fe, 20s) |
| `stone_gate` | Stone Gate | 40S | 25 | 2200 | S70 P85 C15 F50 E30 | gate | - | - | - | -> `reinforced_gate` (40S 25Fe, 25s) |
| `guard_tower` | Guard Tower | 60W 200S 50Cu | 70 | 1300 | S65 P85 C15 F30 E25 | - | tower_crossbow | - | - | -> `bombard_tower` (150S 100Fe 30Ae, 50s) |

**Starfall Age**

| id | name | cost | build s | hp | armour % | role | weapons | trains | techs | upgrade |
|---|---|---|---|---|---|---|---|---|---|---|
| `citadel` | Citadel | 400W 650S 150Fe 50Ae | 220 | 4200 | S65 P85 C20 F15 E20 | drop: all; +20 pop | keep_arrows | peasant, scout_rider, engineer | 2 | - |
| `manor` | Manor | 110W 60S | 40 | 900 | S65 P85 C15 F30 E25 | +20 pop | - | - | - | - |
| `aether_extractor` | Aether Extractor | 150W 100S 100Cu | 45 | 1200 | S65 P85 C15 F30 E25 | drop: aether; trickle 0.6 aether/s and 0.06 adamant/s; on `aether_vent` | - | - | - | - |
| `castle` | Castle | 200W 650S 100Fe | 200 | 5000 | S70 P85 C25 F20 E25 | +10 pop | keep_arrows, tower_bombard | knight, grenadier, trebuchet | 1 | - |
| `aether_foundry` | Aether Foundry | 200W 100S 100Fe 50Ae | 70 | 2000 | S65 P85 C15 F30 E25 | - | - | aether_lancer, clockwork_automaton, war_balloon | 1 | - |
| `bombard_tower` | Bombard Tower | 300S 100Fe 30Ae | 80 | 1800 | S65 P85 C15 F30 E25 | - | tower_bombard | - | - | -> `gun_turret` (100S 100Fe 75Si 50Ae, 50s) |

**Engine Age**

| id | name | cost | build s | hp | armour % | role | weapons | trains | techs | upgrade |
|---|---|---|---|---|---|---|---|---|---|---|
| `engine_works` | Engine Works | 200W 200S 200Fe 100Si 50Ae | 80 | 2600 | S70 P85 C25 F40 E30 | - | - | steam_tank, mech_walker, rocket_battery, flak_wagon, hover_tank, plasma_artillery, storm_walker, titan | 2 | - |
| `laboratory` | Laboratory | 150S 100Cu 100Fe 150Si 50Ae | 70 | 1800 | S70 P85 C25 F40 E30 | - | - | - | 6 | - |
| `airfield` | Airfield | 200W 150Fe 100Si 50Ae | 70 | 2200 | S70 P85 C25 F40 E30 | - | - | gyrocopter, biplane_bomber | - | - |
| `anti_air_battery` | Flak Battery | 100S 120Fe 50Si 30Ae | 45 | 1400 | S70 P85 C25 F40 E30 | - | battery_flak | - | - | - |
| `reinforced_wall` | Reinforced Wall | 20S 10Fe | 14 | 3500 | S75 P85 C30 F50 E30 | wall segment | - | - | - | -> `force_wall` (6Si 10Ae 4Ad, 20s) |
| `reinforced_gate` | Reinforced Gate | 50S 25Fe | 30 | 4500 | S75 P85 C30 F50 E30 | gate | - | - | - | -> `force_gate` (15Si 25Ae 10Ad, 25s) |
| `gun_turret` | Gun Turret | 250S 150Fe 75Si 50Ae | 80 | 2200 | S70 P85 C25 F40 E30 | - | turret_autocannon | - | - | -> `plasma_turret` (50Fe 100Si 150Ae 40Ad, 60s) |

**Ascendant Age**

| id | name | cost | build s | hp | armour % | role | weapons | trains | techs | upgrade |
|---|---|---|---|---|---|---|---|---|---|---|
| `starport` | Starport | 200S 200Fe 200Si 250Ae 100Ad | 90 | 3200 | S75 P85 C30 F45 E35 | regen 2/s | - | hover_tank, gunship, starfighter, drone_swarm, star_bomber | 1 | - |
| `shield_generator` | Shield Generator | 150S 200Si 250Ae 100Ad | 60 | 1500 | S75 P85 C30 F45 E35 | shield 3000 | emp_pulse | - | - | - |
| `force_wall` | Force Wall | 15S 8Si 10Ae 4Ad | 14 | 4800 | S80 P85 C40 F45 E40 | wall segment; shield 500; regen 3/s | - | - | - | - |
| `force_gate` | Force Gate | 40S 15Si 25Ae 10Ad | 30 | 5000 | S80 P85 C40 F45 E40 | gate; shield 600; regen 3/s | - | - | - | - |
| `plasma_turret` | Plasma Turret | 200S 150Fe 100Si 150Ae 40Ad | 80 | 2500 | S75 P85 C30 F45 E35 | shield 400 | turret_plasma | - | - | - |

Upgrade chains: `town_center` -> `citadel`; `house` -> `manor`;
`palisade_wall` -> `stone_wall` -> `reinforced_wall` -> `force_wall`;
`palisade_gate` -> `stone_gate` -> `reinforced_gate` -> `force_gate`;
`watchtower` -> `guard_tower` -> `bombard_tower` -> `gun_turret` -> `plasma_turret`.

### 9.3 Techs (53)

**Researchable from the Hearth Age** (5)

| id | name | at | cost | time s | requires | effects |
|---|---|---|---|---|---|---|
| `to_banner` | Raise the Banners | `town_center` | 500F 100W | 90 | - | advance age |
| `double_bit_axe` | Double-Bit Axe | `lumber_camp` | 100F 50W | 25 | - | wood gather x1.15 |
| `horse_collar` | Horse Collar | `mill` | 75F 75W | 25 | - | food gather x1.1 |
| `stone_mining` | Stone Mining | `mining_camp` | 100F 75W | 25 | - | stone gather x1.15 |
| `prospecting` | Prospecting | `mining_camp` | 100F 100W | 30 | - | copper gather x1.15; iron gather x1.15 |

**Researchable from the Banner Age** (14)

| id | name | at | cost | time s | requires | effects |
|---|---|---|---|---|---|---|
| `to_starfall` | Seek the Fallen Star | `town_center` | 600F 150Cu 100Ae | 110 | owns forge | advance age |
| `bow_saw` | Bow Saw | `lumber_camp` | 150F 100W | 35 | double_bit_axe | wood gather x1.15 |
| `heavy_plow` | Heavy Plow | `mill` | 125F 125W | 35 | horse_collar | food gather x1.15 |
| `wheelbarrow` | Wheelbarrow | `mill` | 175F 50W | 40 | - | carry +3; Worker speed x1.1 |
| `forging` | Forging | `forge` | 150F 75Cu | 40 | - | Melee damage x1.1 |
| `fletching` | Fletching | `forge` | 100F 75W 50Cu | 40 | - | Ranged damage x1.1; Ranged range +0.5; Ranged vision +0.5; Tower damage x1.1 |
| `scale_mail` | Scale Mail | `forge` | 125F 75Cu | 40 | - | Infantry slash armour +0.05; Infantry pierce armour +0.05 |
| `scale_barding` | Scale Barding | `forge` | 150F 75Cu | 40 | - | Cavalry slash armour +0.05; Cavalry pierce armour +0.05 |
| `masonry` | Masonry | `mining_camp` | 150W 150S | 45 | - | all buildings hp x1.1; all buildings crush armour +0.05 |
| `militia_drill` | Militia Drill | `barracks` | 150F 50Fe | 40 | - | unit militia hp x1.25; unit militia slash armour +0.1 |
| `pike_drill` | Pike Drill | `barracks` | 150F 100W | 40 | - | `spearman` -> `pikeman` |
| `crossbow_drill` | Crossbows | `archery_range` | 125F 100W 50Cu | 40 | - | `archer` -> `crossbowman` |
| `husbandry` | Husbandry | `stable` | 150F 50W | 35 | - | Cavalry speed x1.1 |
| `herbal_medicine` | Herbal Medicine | `temple` | 150F 50Cu | 40 | - | Infantry regen +0.3; Cavalry regen +0.3 |

**Researchable from the Starfall Age** (16)

| id | name | at | cost | time s | requires | effects |
|---|---|---|---|---|---|---|
| `to_engine` | Kindle the Engines | `citadel` | 600F 250Fe 200Si 200Ae | 120 | owns aether_foundry | advance age |
| `crop_rotation` | Crop Rotation | `mill` | 250F 200W | 45 | heavy_plow | food gather x1.15 |
| `handcart` | Handcart | `mill` | 300F 150W 50Fe | 50 | wheelbarrow | carry +4; Worker speed x1.1 |
| `deep_shafts` | Deep Shafts | `mining_camp` | 200F 200W 50Fe | 45 | prospecting | stone gather x1.15; copper gather x1.15; iron gather x1.15; silicon gather x1.25 |
| `aether_resonance` | Aether Resonance | `mining_camp` | 200F 150Cu 50Ae | 45 | - | aether gather x1.25 |
| `aether_tempering` | Aether Tempering | `forge` | 250F 150Fe 40Ae | 50 | forging | Melee damage x1.1 |
| `rifled_barrels` | Rifled Barrels | `forge` | 200F 100W 150Fe | 50 | fletching | Ranged damage x1.1; Tower damage x1.1 |
| `plate_armor` | Plate Armour | `forge` | 250F 200Fe | 50 | scale_mail | Infantry slash armour +0.05; Infantry pierce armour +0.05; Infantry crush armour +0.05 |
| `plate_barding` | Plate Barding | `forge` | 275F 200Fe | 50 | scale_barding | Cavalry slash armour +0.05; Cavalry pierce armour +0.05; Cavalry crush armour +0.05 |
| `architecture` | Architecture | `mining_camp` | 250W 300S | 55 | masonry | all buildings hp x1.1; all buildings build time x0.9 |
| `halberd_drill` | Halberds | `barracks` | 250F 100Fe 25Ae | 50 | pike_drill | `pikeman` -> `halberdier` |
| `hand_cannon_drill` | Hand Cannons | `archery_range` | 250F 150Fe 50Ae | 50 | crossbow_drill | `crossbowman` -> `arquebusier` |
| `star_tithe` | Star Tithe | `temple` | 300F 150S 100Cu | 60 | - | +0.15 aether/s; +0.5 food/s |
| `ballistics` | Ballistics | `siege_workshop` | 300W 150Cu 100Fe | 50 | - | Siege range +1; Tower range +1; Tower vision +1 |
| `conscription` | Conscription | `castle` | 400F 200Cu | 60 | - | Infantry train time x0.8; Cavalry train time x0.8 |
| `aether_infusion` | Aether Infusion | `aether_foundry` | 200Cu 100Fe 100Ae | 50 | - | Mechanical damage x1.15; Mechanical hp x1.1 |

**Researchable from the Engine Age** (12)

| id | name | at | cost | time s | requires | effects |
|---|---|---|---|---|---|---|
| `to_ascendant` | Ascend | `citadel` | 800F 300Si 450Ae 200Ad | 150 | owns laboratory | advance age |
| `steam_harvesters` | Steam Harvesters | `mill` | 300F 150Fe 75Si | 55 | crop_rotation | food gather x1.2; wood gather x1.2 |
| `adamant_cutting` | Adamant Cutting | `mining_camp` | 300F 150Fe 100Ae | 55 | - | adamant gather x1.3; aether gather x1.1 |
| `steam_hammers` | Steam Hammers | `forge` | 300F 200Fe 50Si 100Ae | 60 | aether_tempering | Melee damage x1.1 |
| `smokeless_powder` | Smokeless Powder | `forge` | 250F 200Fe 75Si 100Ae | 60 | rifled_barrels | Ranged damage x1.1; Ranged range +0.5; Ranged vision +0.5 |
| `ballistic_weave` | Ballistic Weave | `forge` | 350F 250Fe 75Si | 60 | plate_armor | Infantry pierce armour +0.05; Infantry fire armour +0.05; Cavalry pierce armour +0.05 |
| `ferroconcrete` | Ferro-Concrete | `laboratory` | 400S 200Fe 100Si | 60 | architecture | Wall hp x1.2; Tower hp x1.2; all buildings crush armour +0.05; all buildings fire armour +0.05 |
| `steam_harness` | Steam Harness | `barracks` | 250F 150Fe 50Si | 50 | - | unit man_at_arms hp x1.25; unit man_at_arms crush armour +0.1 |
| `analytical_engine` | Analytical Engine | `laboratory` | 250F 150Cu 150Si 50Ae | 60 | - | all units train time x0.85 |
| `targeting_computers` | Targeting Computers | `laboratory` | 150Cu 100Fe 200Si 50Ae | 60 | - | Ranged range +1; Mechanical range +1; Tower range +1; Ranged vision +1; Mechanical vision +1; Tower vision +1 |
| `punch_card_logistics` | Punch-Card Logistics | `laboratory` | 300W 150S 100Si | 50 | - | all buildings build time x0.8 |
| `assembly_line` | Assembly Line | `engine_works` | 200W 250Fe 150Si 50Ae | 60 | - | Mechanical cost x0.85 |

**Researchable from the Ascendant Age** (6)

| id | name | at | cost | time s | requires | effects |
|---|---|---|---|---|---|---|
| `star_forged_arms` | Star-Forged Arms | `forge` | 400F 200Ae 100Ad | 70 | steam_hammers, smokeless_powder | all units damage x1.1 |
| `adamant_weave` | Adamant Weave | `forge` | 400F 150Ae 150Ad | 70 | ballistic_weave | Infantry hp x1.1; Cavalry hp x1.1; Infantry energy armour +0.1; Cavalry energy armour +0.1 |
| `force_lattice` | Force Lattice | `laboratory` | 300Si 300Ae 100Ad | 70 | ferroconcrete | all buildings regen +2; Wall hp x1.15; all buildings shield hp x1.25 |
| `deflector_shields` | Deflector Shields | `laboratory` | 300Si 250Ae 50Ad | 60 | - | Shielded shield hp x1.35 |
| `adamant_plating` | Adamant Plating | `engine_works` | 200Fe 250Ae 200Ad | 70 | - | Mechanical hp x1.2; Mechanical pierce armour +0.05; Mechanical crush armour +0.05 |
| `fusion_cores` | Fusion Cores | `starport` | 250Si 300Ae 100Ad | 70 | - | Mechanical speed x1.1; Mechanical regen +2 |

### 9.4 Weapons (53)

`hits`: G ground units, A air, B buildings.

**Hearth Age**

| id | name | kind | damage | cd s | dps | range m | splash | hits | bonus | used by |
|---|---|---|---|---|---|---|---|---|---|---|
| `work_axe` | Work Axe | Melee Swing | 3 Slash | 1.5 | 2 | 0.6 | 0 | GB | - | peasant, engineer |
| `club` | Cudgel | Melee Smash | 5 Crush | 1.5 | 3.3 | 0.7 | 0 | GB | - | militia |
| `short_spear` | Bronze Spear | Melee Thrust | 6 Pierce | 1.5 | 4 | 1 | 0 | GB | Cavalry x2.75, Beast x2, Building x0.3 | spearman |
| `sling` | Sling | Arc | 4 Crush | 1.8 | 2.2 | 7 | 0 | GB | Infantry x1.2, Building x1.5 | slinger |
| `short_bow` | Short Bow | Bolt | 6 Pierce | 1.6 | 3.8 | 8 | 0 | GAB | Spear x1.5, Building x0.2 | archer |
| `riding_sword` | Riding Sword | Melee Swing | 5 Slash | 1.3 | 3.8 | 0.8 | 0 | GB | Ranged x1.5, Siege x1.5 | scout_rider |
| `tc_arrows` | Hall Archers | Bolt | 5 x3 Pierce | 2 | 7.5 | 10 | 0 | GA | - | town_center |
| `longbow` | Longbow Volley | Bolt | 7 x2 Pierce | 2 | 7 | 10 | 0 | GAB | Building x0.2 | watchtower |

**Banner Age**

| id | name | kind | damage | cd s | dps | range m | splash | hits | bonus | used by |
|---|---|---|---|---|---|---|---|---|---|---|
| `arming_sword` | Arming Sword | Melee Swing | 10 Slash | 1.2 | 8.3 | 0.8 | 0 | GB | - | man_at_arms |
| `pike` | Pike | Melee Thrust | 8 Pierce | 1.5 | 5.3 | 1.5 | 0 | GB | Cavalry x3, Beast x2, Giant x1.5, Building x0.3 | pikeman |
| `crossbow` | Crossbow | Bolt | 14 Pierce | 2 | 7 | 9 | 0 | GAB | Heavy x1.25, Spear x1.5, Building x0.2 | crossbowman |
| `javelin` | Javelins | Bolt | 6 Pierce | 1.3 | 4.6 | 8 | 0 | GAB | Ranged x2.5, Building x0.2 | javelineer |
| `war_lance` | War Lance | Melee Thrust | 15 Pierce | 1.4 | 10.7 | 1.2 | 0 | GB | Ranged x1.3, Siege x1.5 | knight |
| `horse_bow` | Horse Bow | Bolt | 8 Pierce | 1.2 | 6.7 | 8 | 0 | GAB | Building x0.2 | horse_archer |
| `ram_head` | Iron Ram | Melee Smash | 50 Crush | 4 | 12.5 | 0.5 | 0 | B | Building x4 | battering_ram |
| `mangonel_shot` | Mangonel Stone | Arc | 30 Crush | 5 | 6 | 15 (min 3) | 1.6 | GB | Building x2, Infantry x1.25 | mangonel |
| `ballista_bolt` | Ballista Bolt | Cannon (pierce 3) | 34 Pierce | 4.5 | 7.6 | 14 (min 2) | 0 | GB | Siege x2.5, Giant x1.5, Heavy x1.25, Building x0.5 | ballista |
| `tower_crossbow` | Tower Crossbows | Bolt | 12 x2 Pierce | 2 | 12 | 11 | 0 | GAB | Building x0.2 | guard_tower |

**Starfall Age**

| id | name | kind | damage | cd s | dps | range m | splash | hits | bonus | used by |
|---|---|---|---|---|---|---|---|---|---|---|
| `halberd` | Halberd | Melee Swing (cleave 1) | 16 Slash | 1.4 | 11.4 | 1.4 | 0 | GB | Cavalry x2.5, Beast x2, Giant x1.5, Mechanical x2 | halberdier |
| `arquebus` | Arquebus | Bolt | 34 Pierce | 2.6 | 13.1 | 10 | 0 | GAB | Heavy x1.3, Spear x1.5, Building x0.25 | arquebusier |
| `grenade` | Aether Grenade | Arc | 22 Fire | 3.4 | 6.5 | 7 | 1.6 | GB | Infantry x1.2, Building x1.5 | grenadier |
| `aether_lance` | Aether Lance | Melee Thrust | 28 Energy | 1.6 | 17.5 | 1.4 | 0 | GB | Mechanical x1.6 | aether_lancer |
| `clockwork_fists` | Clockwork Fists | Melee Smash | 26 Crush | 1.6 | 16.2 | 0.8 | 0.6 | GB | Building x1.5 | clockwork_automaton |
| `fire_bomb` | Firepot Drop | Bomb | 50 Fire | 5 | 10 | 1 | 2.5 | GB | Building x2.5, Infantry x1.2 | war_balloon |
| `bombard_shot` | Bombard | Cannon | 90 Crush | 6.5 | 13.8 | 16 (min 3) | 1 | GB | Building x3.5, Siege x1.5 | bombard |
| `trebuchet_stone` | Trebuchet | Arc | 180 Crush | 10 | 18 | 22 (min 7) | 1.8 | GB | Building x4, Infantry x0.35, Cavalry x0.35 | trebuchet |
| `tower_bombard` | Tower Bombard | Cannon (pierce 1) | 60 Crush | 4 | 15 | 12 | 1 | GB | Siege x1.5, Building x1.5 | castle, bombard_tower |
| `keep_arrows` | Keep Battlements | Bolt | 9 x5 Pierce | 2 | 22.5 | 12 | 0 | GAB | Building x0.2 | citadel, castle |

**Engine Age**

| id | name | kind | damage | cd s | dps | range m | splash | hits | bonus | used by |
|---|---|---|---|---|---|---|---|---|---|---|
| `rifle` | Rifle | Bolt | 24 Pierce | 1.6 | 15 | 12 | 0 | GAB | Heavy x1.35, Building x0.25 | rifle_squad |
| `gatling` | Gatling Gun | Bolt | 5 x6 Pierce | 1.2 | 25 | 10 | 0 | GAB | Infantry x1.5, Building x0.2 | gatling_crew |
| `shock_maul` | Shock Maul | Melee Smash | 34 Crush | 1.4 | 24.3 | 0.9 | 0.5 | GB | Mechanical x2, Building x1.25 | shock_trooper (benchmark) |
| `tank_cannon` | Landship Cannon | Cannon (pierce 1) | 75 Crush | 3 | 25 | 13 | 1.2 | GB | Building x2, Mechanical x1.25 | steam_tank |
| `rocket_salvo` | Rocket Salvo | Homing | 32 x4 Fire | 9 | 14.2 | 20 (min 5) | 1.6 | GB | Building x1.5, Mechanical x1.2 | rocket_battery |
| `flak_cannon` | Flak Cannon | Bolt | 18 x2 Fire | 1.5 | 24 | 14 | 1.2 | A | Flying x2, Heavy x1.5 | flak_wagon |
| `gyro_guns` | Gyro Guns | Bolt | 7 x2 Pierce | 1 | 14 | 8 | 0 | GAB | Building x0.25 | gyrocopter |
| `aerial_bomb` | Aerial Bombs | Bomb | 140 Fire | 7 | 20 | 1 | 3 | GB | Building x2.5 | biplane_bomber |
| `autocannon` | Autocannon | Bolt | 22 x2 Crush | 1.6 | 27.5 | 11 | 0.5 | GAB | Mechanical x1.25 | mech_walker |
| `turret_autocannon` | Turret Autocannon | Bolt | 22 x2 Crush | 1.6 | 27.5 | 11 | 0.5 | GAB | Mechanical x1.25 | gun_turret |
| `battery_flak` | Emplaced Flak | Bolt | 18 x2 Fire | 1.5 | 24 | 14 | 1.2 | A | Flying x2, Heavy x1.5 | anti_air_battery |
| `flame_projector` | Flame Projector | Spray | 12 Fire | 1 | 12 | 5 | 0 | GB | Infantry x1.5, Building x1.5 | steam_tank |
| `tesla_coil` | Tesla Coil | Chain (4 jumps) | 30 Energy | 2 | 15 | 8 | 0 | GAB | Infantry x1.3, Mechanical x1.25 | storm_walker |

**Ascendant Age**

| id | name | kind | damage | cd s | dps | range m | splash | hits | bonus | used by |
|---|---|---|---|---|---|---|---|---|---|---|
| `plasma_carbine` | Plasma Carbine | Bolt | 22 Fire | 0.6 | 36.7 | 10 | 0 | GAB | Infantry x1.15 | shield_trooper |
| `laser_lance` | Laser Lance | Beam | 130 Energy | 3 | 43.3 | 14 | 0 | GB | Mechanical x1.5, Giant x1.25 | hover_tank |
| `missile_pod` | Missile Pods | Homing | 28 x2 Fire | 1.6 | 35 | 13 | 0.5 | GAB | Mechanical x1.25 | gunship |
| `pulse_laser` | Pulse Laser | Beam | 12 Energy | 0.7 | 17.1 | 9 | 0 | GAB | Flying x2 | starfighter |
| `plasma_mortar` | Plasma Mortar | Arc | 120 Fire | 6 | 20 | 22 (min 6) | 3.5 | GB | Building x3.5 | plasma_artillery |
| `drone_zapper` | Drone Zapper | Beam | 5 Energy | 0.8 | 6.2 | 6 | 0 | GAB | - | drone_swarm |
| `aether_charge` | Aether Charge | Bomb | 300 Fire | 10 | 30 | 1 | 5 | GB | Building x2.5, Infantry x1.2 | star_bomber |
| `star_cannon` | Star Cannon | Beam | 360 Energy | 5 | 72 | 16 | 0 | GAB | Building x2, Giant x1.5, Mechanical x1.25 | titan (benchmark) |
| `titan_stomp` | Titan Stomp | Nova | 120 Crush | 4 | 30 | 4 | 0 | GB | Infantry x1.2, Building x1.5 | titan (benchmark) |
| `railgun` | Railgun | Cannon (pierce 4) | 150 Pierce | 4 | 37.5 | 16 | 0 | GB | Mechanical x1.5, Giant x1.5, Heavy x1.25 | storm_walker |
| `emp_pulse` | EMP Pulse | Nova | 40 Energy | 3 | 13.3 | 8 | 0 | GA | Mechanical x2.5, Flying x1.5, Shielded x2 | shield_generator |
| `turret_plasma` | Plasma Lance Turret | Beam | 80 Energy | 1.8 | 44.4 | 13 | 0 | GAB | Mechanical x1.3 | plasma_turret |

---

## 10. Rules for faction designers

Your file is `assets/data/factions/<id>.ron`; validate with
`cargo run -q -p af_data --example check -- --only factions/<id>.ron --summary`.

**The checker enforces:** every listed unit is trained at a building you list;
every listed tech is researched at a building you list and its `requires` are
listed; any building you list that upgrades must have its upgrade target listed;
age-ups exist for ages 1-4; your worker gathers only what some listed building
accepts; your first start building is a TownCenter.

**You must also (not checked):**

1. **Keep the age path walkable.** The common age-ups need `town_center`,
   `citadel`, `forge` (gate of `to_starfall`), `aether_foundry` (gate of
   `to_engine`) and `laboratory` (gate of `to_ascendant`). List all five, or write
   your own age-up techs (prefixed ids) and list those instead.
2. **Take whole chains.** Listing `palisade_wall` pulls in `stone_wall`,
   `reinforced_wall` and `force_wall`; the same for gates, the five towers,
   `house`/`manor` and `town_center`/`citadel`. A faction that should stop short (the
   Khaganate has palisades only) defines its own segment without an upgrade, e.g.
   `steppe_palisade`, `steppe_palisade_gate`, and does not list the common walls.
3. **Line upgrades need both ends.** `pike_drill` needs `spearman` and `pikeman`;
   `halberd_drill` needs `pikeman` and `halberdier`; `crossbow_drill` needs `archer`
   and `crossbowman`; `hand_cannon_drill` needs `crossbowman` and `arquebusier`.
   `militia_drill` needs `militia` and `steam_harness` needs `man_at_arms`. A unique
   `UpgradeUnit` must be pop-neutral with a per-model cost ratio of 1.6 or less;
   otherwise make the better unit trained-only and give the old one a stat drill.
4. **Name uniques with your prefix** (`ironhold_thane_guard`, `dune_sandskiff`).
   Train them at a common building (the `castle` is the natural home of elite
   uniques, the `aether_foundry`, `engine_works` and `starport` for machines) or at a
   prefixed unique building. Uniques may reuse common weapons or define prefixed ones.
5. **Price by the bands in section 8.** Compute dps/cost and ehp/cost for every
   unique; stay within about 1.5x of the age median and give anything above it a
   clear weakness. Starfall units carry 10-40 aether; Engine units iron and silicon
   plus 20-50 aether (the powder and the boilers burn it); Ascendant units aether
   and adamant, with more aether than iron. A faction whose identity is a metal may
   keep its own uniques on that metal (the Ironhold Thanes' late uniques stay
   iron-heavy, see their doc).
6. **Tag carefully; tags are the combat API.** Every combat unit carries one of
   Infantry, Cavalry, Mechanical, Siege, Beast. Infantry, cavalry and beasts carry
   Melee or Ranged (a winged soldier is still infantry). Machines and siege do not,
   flyers among them: Ranged/Melee techs (`fletching`, `forging` and their
   successors) and anti-Ranged bonuses (`javelin`, `riding_sword`, `war_lance`) skip
   them, and Mechanical techs cover them. Polearm troops carry Spear. Armoured troops
   carry Heavy. Anything with a `shield` carries Shielded. Every flyer carries Flying
   (bonuses read tags, not locomotion). Giant is only for giants and titans (pop 8 or
   more, or hp 2500 or more), never walkers, tanks or siege, whatever their size.
   Faction techs that target `Tag(...)` will then pick up uniques automatically.
7. **Missiles that shoot up** must be straight-firing kinds (section 5.3). Give
   every weapon a `look`, and a `projectile` when it fires something.
8. **Gather bonuses** 1.1-1.3 on specialities; nothing above 1.5. Start everyone at
   300 food, 300 wood, 150 stone, 100 copper with 5 workers, 1 scout and a
   `town_center`, unless the bonus is the start.
9. **Use every age.** Give each faction something new to want in all five ages,
   and at least one unique in the Starfall Age or later, so the arc reads as
   "medieval, then strange".
10. **Fit the engine's geometry and sight rules** (reasons in section 11):
    - ground units other than giants are under 3.0 m wide (2.9 at most); giants are
      at most 5.0 m wide;
    - wall segments are at least 1.0 m thick and gates at least 1.2 m thick and
      9.0 m long, in every tier, and an upgrade keeps the footprint;
    - flyers fly at `altitude` 12 or higher, above the tallest box in the game;
    - a direct-fire unit's `vision` is at least its longest weapon range, and a tech
      that adds range also adds the same vision (siege and artillery excepted);
    - `Stat Range` does not grow Nova radii or Spray cones, so price those areas as
      fixed;
    - a healer mends one target at a time; price `rate` as single-target healing.

---

## 11. Notes for the engine

Assumptions the content makes, collected so engine work can honour or revisit them:

- Squads: `cost` per training, `pop` per model; models select and move together.
- `UpgradeUnit` converts per model and swaps the unit in every training menu where
  `from` appeared. The common pool uses it only on four infantry pairs of equal pop
  (see 9.1), so a conversion never changes population.
- Building upgrades keep x and z of the footprint and change only height, so an
  in-place upgrade never clips a flush neighbour. This includes every wall and gate
  tier: walls 3.0 x 1.0 m, gates 9.0 x 1.2 m from palisade to force field.
- Walls, gates and widths assume the af_geom nav grid with 1 m cells, where a
  footprint blocks the cells whose centres it covers (`cells_covered`, no inflate)
  and A* needs `k = ceil(width / 2 - 0.5)` clear cells on every side of a mover.
  - A line thinner than one cell leaves gaps on most angles, so every wall and gate
    tier is at least 1.0 m thick (gates 1.2 m).
  - A unit up to 2.9 m wide has k = 1 and needs a clear 3 x 3 block of cells; up to
    5.0 m wide it has k = 2 and needs a clear 5 x 5 block. Wider than 5.0 m is k = 3
    (7 x 7), which no gate passes, so no ground unit is wider than 5.0 m.
  - The block is axis-aligned, so a diagonal gate must be much longer than 5 cells:
    with flush walls and the 0.3 m walk margin, k = 2 movers cannot cross a 6 m gate
    laid at about 20-70 degrees to the grid (7.5 m still fails near 45 degrees), and
    cross 8 m and longer gates at every angle.
    Gates are 9 m long (three wall segments), which keeps a margin over that, and
    k = 1 units pass at any angle. `crates/af_data/tests/geometry.rs` checks every
    faction's widest ground unit against its own gates with the real nav grid.
  - Non-giant ground units stay at or below 2.9 m wide, which also keeps them at
    k = 1 if the movement code adds a collision skin.
- Armour may exceed 0.9 after stacked techs; clamp at 0.9.
- `aether_extractor` must be placed on an `aether_vent` (`requires_node`). The vent
  is flat and non-blocking (the extractor sits on top); other buildings should not be
  allowed to cover a vent. The extractor is a drop site and trickles 0.6 aether/s and
  0.06 adamant/s (the only renewable adamant once the meteors are mined out).
- Healers (`HealerDef`): a healer restores `rate` hp/s to one friendly unit in `range`
  that has one of its `tags`, choosing the lowest hp fraction first. No target is
  healed by more than 3 healers at once. Every healer's `rate` is priced as
  single-target healing; area heals would need every rate divided by about 4.
- `Stat Range` techs change weapon reach for target acquisition, but they do not
  change the radius of a Nova or the cone length of a Spray, whose `range` is an
  area priced as fixed (+1 m on a 4-6 m nova is +40-60% area). A tech that should
  grow a radius needs its own effect.
- Direct-fire units see at least as far as they shoot, and every common range tech
  adds matching vision (`fletching` and `smokeless_powder` +0.5 for Ranged;
  `targeting_computers` +1 for Ranged, Mechanical and Tower; `ballistics` +1 for
  Tower). Siege and artillery deliberately out-range their own sight and need a
  spotter.
- Flyers: `altitude` is the height of the bottom of the flyer's box above the ground.
  Flyers ignore ground collision, so every flyer flies at 12 m or higher, above the
  tallest box in the game (the titan, 12 m; buildings reach 11 m and faction giants
  11 m), and never visibly passes through a building or giant. The renderer should
  draw a ground marker or shadow under each flyer so the player can see where it is;
  bombs fall from the flyer's altitude in `fall_time`.
- A building's `shield` protects that building. The `shield_generator` is designed
  so that projecting its shield as a dome (radius about 12 m) over nearby buildings
  is a natural later feature.
- Melee range +1 from `targeting_computers` (it targets `Tag(Mechanical)`) also
  reaches clockwork automatons; harmless.
- `garrison` values are set on town centres, citadels, castles and early towers
  for when garrisoning exists.
- Weapon `look.offset` is read as the centre of the look box (x right, y up from
  the host's base, z forward). Every look sits at least partly outside its host's
  box so the player can see it; the one exception is the castle's `tower_bombard`,
  which sits inside the keep, so the castle reads by its `keep_arrows` on the roof.
  Roof-mounted building weapons sit on the roof. Where a unit and a building would
  share a weapon, the building gets its own copy with a roof offset
  (`turret_autocannon`, `battery_flak`).
- The war balloon is intentionally slow (3.5 m/s) for a flyer.
