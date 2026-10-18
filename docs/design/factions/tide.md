# The Tidecrown Republic (`tide`)

*Lagoon merchants whose marines walk out of the sea.*

Data: `assets/data/factions/tide.ron`. Framework: `docs/design/game-design.md`.
Validate with `cargo run -q -p af_data --example check -- --only factions/tide.ron --summary`.

| | |
|---|---|
| Colour | bright lagoon cyan `(0.0, 0.85, 0.85)`: Tidecrown troops stand in water, so the team colour is kept well away from shallows `(0.35, 0.6, 0.72)` |
| Traits | **Amphibious** for Foot, Mounted and Tracked (deep water x0.7, shallows x0.9); **Marshwalker** for Foot (marsh x1.0) |
| Gather bonus | food x1.15, copper x1.25 |
| Worker | `peasant` (later `engineer`) |
| Start | 300 food, 300 wood, 150 stone, 100 copper; 5 peasants, 1 scout rider; a `town_center` |
| Roster | 27 common units (25 combat + 2 workers; 16 of 43 left out, 37%) and 8 uniques: 35 trainable |
| Buildings | 34 common (no `stable`, no `airfield`) and 3 uniques |
| Techs | 48 common (5 left out) and 9 uniques |

---

## 1. Identity and the fallen star

The Tidecrown is a republic of merchant houses, shipwrights and marines on a lagoon
of a hundred islands. They elect a Doge, who wears a crown of coral and copper wire:
the Tidecrown. Their navy is old, their banks are older, and their guard is made of
northern axemen who came south for the pay and stayed for the oath.

Their navigators steer by the stars. So they were the first to notice that the new
red star appeared on no chart, and the first to work out roughly where it would
fall. When it fell, the Republic did not wait for roads. Its galleys went up the
rivers and its marines waded the marshes around the crater rim while other armies
were still cutting a track through the forest. The Arsenal, their walled shipyard,
had kept the recipe for liquid fire for two hundred years; aether thinned it until it
sprayed like water. Later the Arsenal riveted iron onto crab legs, put brass divers on
the lake bed, and finally floated the Doge's gilded state barge, the Bucentaur, on
adamant repulsors.

The look on the battlefield: cyan tabards, red-cloaked Varangians, bronze siphons,
brass diving helmets, verdigris ironclads and one gold ship. Knights who found a
spaceship, and sailed it.

## 2. Playstyle and win conditions

A defensive economy with an amphibious edge. The Republic gathers well, walls up
early, and fights where the map is wet.

- **Hearth and Banner: shield the boom.** Marines, spears and archers hold the
  start. A Harbour puts two peasants to fishing off its quays, doubles as the
  food/wood/copper drop site and starts the Drift Nets trickle. In the Banner Age,
  crossbows behind pikes (with Pavises) are the line; Varangians are the quick
  axemen sent at heavy foot and knights (x1.5 vs Heavy); stradiots swim rivers to
  raid workers.
- **Starfall: take the crater by the marsh.** The crater sits inside a cliff rim
  with three or four gaps, with marsh and thicket patches outside it (design doc
  4.2). Tidecrown infantry cross that marsh at full speed and wade lakes, so they
  reach the gaps first and can switch between them faster than the enemy. Siphon
  towers at the extractor and siphoneers in the gaps make the crater expensive to
  retake.
- **Engine: come out of the water.** Brass divers, ironclad crawlers and landships
  cross deep water that other armies' tracked machines cannot. Lakes and rivers that
  protect an enemy's flank are roads for the Republic.
- **Ascendant: the fleet.** Hover tanks and the Bucentaur glide over the same water,
  phaser marines wade ashore, star bombers carry the Arsenal's fire over the walls,
  and Marriage of the Sea speeds the hover fleet up while the sea pays tribute.

**Win conditions.** (1) Hold more aether vents than the enemy from the Starfall Age,
using marsh approaches and siphon towers. (2) Starve the enemy with amphibious raids
on mills, mining camps and extractors across water they thought was safe. (3) A
late push of Bucentaur, hover tanks and phaser marines behind walls that never fell.

**Losing looks like:** caught on dry open ground by heavy cavalry or horse archers
with no knights to answer; bombed from the air in the Engine Age with no flyers of
their own; a slow Banner Age because nothing but a mangonel breaks stone walls.

## 3. Economy

| Lever | What it does | Numbers |
|---|---|---|
| `gather_bonus` | fishing and the copper trade | peasant food 0.7 -> 0.81/s, copper 0.5 -> 0.63/s |
| `tide_harbor` | fishing quays, and one building that accepts food, wood and copper | 200 wood, 1700 hp, up to three; `resource_field` of 3000 food for 2 peasants (0.81/s each with the food bonus and no walk to a drop site, so about half an hour of fishing, 20 minutes with the food techs); once the shoals are fished out it stays standing as a drop site and barracks; replaces a lumber camp + mill pair at the shore, and trains troops |
| `tide_drift_nets` (Hearth) | fishing smacks offshore | +0.3 food/s (18/min, about 0.4 of a worker) for 50F 150W; pays back in ~11 min |
| `tide_levant_convoys` (Banner) | trade galleys | +0.25 copper/s and +0.15 iron/s (24/min) for 350; pays back in ~15 min |
| `tide_murano_glass` (Starfall) | lagoon glassworks | quartz sand x1.2 before the 200-silicon Engine age-up |

The bonuses pay for the roster: siphoneers, divers, ironclads, phaser marines,
Copper Sheathing and the Bucentaur all spend copper heavily, so the 25% copper
bonus and the convoys are what make them affordable. Food x1.15, the Harbour's
fishing quays and the nets carry the food in the age-ups (500, 600, 600 and 800)
and in the marine-heavy army. No other faction fishes; the x1.15 applies to the quays
as to berries and farms.

Drop sites: town centre and citadel (all), lumber camp (wood), mill (food),
mining camp (stone, ores, silicon, aether, adamant), aether extractor (aether),
harbour (food, wood, copper; also fished as a food field). Houses upgrade to manors
as usual.

## 4. Terrain traits in play

| Terrain | Most armies | Tidecrown foot | Tidecrown mounted | Tidecrown tracked | Notes |
|---|---|---|---|---|---|
| Shallows | foot/mounted x0.55, tracked x0.6, wheeled cannot | x0.9 | x0.9 | x0.9 | fords are no obstacle |
| Deep water | only hover (x0.8) | **x0.7** | **x0.7** | **x0.7** | marines, stradiots, divers, automatons, landships and the ironclad cross lakes |
| Marsh | foot/mounted/tracked x0.6, wheeled cannot | **x1.0** | x0.6 | x0.6 | the crater's outer marsh is home ground for the foot |
| Thicket | foot x0.5 | x0.5 | cannot | cannot | no help |
| Cliff | climbers only | cannot | cannot | cannot | no help |

- Every Foot unit gets both traits, so this covers marines, Varangians, crossbows,
  pikes, siphoneers, rifles, divers and phaser marines. Workers wade too: ore on a
  lake island or wood across a river is Tidecrown ground.
- **Amphibious also reaches Tracked machines**: the clockwork automaton (Starfall),
  the steam landship and the **Ironclad Crawler** (Engine) and plasma artillery
  (Ascendant) wade lakes at x0.7 and fords at x0.9. Marshwalker does not reach them
  (marsh stays x0.6), and like every machine they never enter thickets. The
  ironclad is the machine built for it: the amphibious tank whose torpedoes and
  casemate gun hit shore works from the water's edge.
- The trait does not reach wheeled machines (mangonels, ballistae, bombards, rocket
  batteries, flak wagons). A Tidecrown army that wades a lake leaves its wheeled
  guns on the far bank. Plan crossings for infantry, divers and tracked machines, and
  send the wheeled guns the long way.
- Hover (hover tanks, the Bucentaur) crosses everything wet at x0.8 in the
  Ascendant Age, which lets the whole late fleet follow the marines.

## 5. Roster by age

U = unique. Costs per training; dps per model before armour.

**Hearth**

| id | name | role | hp | dps | cost | trained at |
|---|---|---|---|---|---|---|
| `peasant` | Peasant | worker, all 8 resources | 30 | 2 | 50F | town_center, citadel |
| `scout_rider` | Scout Rider | scout and raider (swims) | 75 | 3.8 | 80F | town_center, citadel |
| `spearman` | Spearman | anti-cavalry; becomes pikeman | 50 | 4 | 45F 25W | barracks |
| `slinger` | Slinger Band (3) | cheap skirmish squad vs infantry and palisades | 28 | 2.2 | 90F 15W | archery_range |
| `archer` | Archer | ranged; becomes crossbowman | 40 | 3.8 | 30F 45W | archery_range |
| `tide_marine` **U** | Marine Band (3) | melee squad that replaces militia; bucklers vs arrows | 40 | 3.6 | 120F 40W | barracks, tide_harbor |

**Banner**

| id | name | role | hp | dps | cost | trained at |
|---|---|---|---|---|---|---|
| `pikeman` | Pikeman | the anti-cavalry line; Tide's main answer to horse | 80 | 5.3 | 50F 35W 15Cu | barracks |
| `crossbowman` | Crossbowman | ranged backbone (Pavises), shoots air | 70 | 7 | 35F 55W 30Cu | archery_range |
| `javelineer` | Javelineer | hunts archers and horse archers, which Tide cannot chase | 75 | 4.6 | 45F 60W | archery_range |
| `monk` | Monk | healer | 50 | 0 | 60F 60Cu | temple |
| `mangonel` | Mangonel | anti-blob; the only Banner wall-breaker | 180 | 6 | 160W 100Cu | siege_workshop |
| `ballista` | Ballista | anti-siege | 160 | 7.6 | 150W 70Cu | siege_workshop |
| `tide_stradiot` **U** | Stradiot | amphibious light raider; the only cavalry after the scout | 120 | 7.5 | 90F 40W 30Cu | tide_harbor |
| `tide_varangian` **U** | Varangian Guard | fast armour-breaker that replaces men-at-arms; overhead axe x1.5 vs Heavy, x1.25 vs machines, no cleave | 100 | 9.4 | 65F 20W 60Fe | barracks, castle |

**Starfall**

| id | name | role | hp | dps | cost | trained at |
|---|---|---|---|---|---|---|
| `halberdier` | Halberdier | anti-cavalry and anti-clockwork line | 120 | 11.4 | 70F 40W 30Fe 10Ae | barracks |
| `arquebusier` | Arquebusier | ranged line (Pavises carry over) | 105 | 13.1 | 60F 40W 45Fe 15Ae | archery_range |
| `grenadier` | Grenadier Pair (2) | anti-blob, burns gates | 95 | 6.5 | 90F 60Cu 40Fe 30Ae | barracks, castle |
| `clockwork_automaton` | Clockwork Automaton | arrow-proof bruiser | 320 | 16.2 | 110Cu 100Fe 40Ae | aether_foundry |
| `bombard` | Bombard | wall and tower breaker | 220 | 13.8 | 120W 50Cu 130Fe 20Ae | siege_workshop |
| `tide_siphoneer` **U** | Fire Siphoneers (2) | 4.5 m cone of liquid fire vs infantry, rams and gates | 100 | 7.2 | 90F 30W 80Cu 30Ae | tide_arsenal, castle |

**Engine**

| id | name | role | hp | dps | cost | trained at |
|---|---|---|---|---|---|---|
| `engineer` | Engineer | late worker | 90 | 2 | 60F 20Fe 20Si | town_center, citadel |
| `rifle_squad` | Rifle Squad (4) | ranged backbone and anti-air | 160 | 15 | 260F 100Cu 170Fe 80Si 30Ae | barracks |
| `gatling_crew` | Gatling Crew | anti-infantry | 260 | 25 | 110F 50Cu 100Fe 40Si 20Ae | barracks |
| `steam_tank` | Steam Landship | land armour and building killer | 800 | 37 | 100W 100Cu 260Fe 100Si 40Ae | engine_works |
| `rocket_battery` | Rocket Battery | long-range siege | 260 | 14.2 | 100W 70Cu 130Fe 80Si 20Ae | engine_works |
| `flak_wagon` | Flak Wagon | mobile anti-air; Tide has no flyers yet | 300 | 24 | 80W 60Cu 120Fe 60Si 20Ae | engine_works |
| `tide_diver` **U** | Brass Diver | amphibious anti-machine; harpoon arcs to a second target | 260 | 18.2 | 100F 90Cu 50Fe 40Si 20Ae | tide_harbor |
| `tide_ironclad` **U** | Ironclad Crawler | the amphibious tank: casemate gun and torpedoes | 760 | 34.1 | 60W 160Cu 240Fe 100Si 40Ae | tide_arsenal |

**Ascendant**

| id | name | role | hp | dps | cost | trained at |
|---|---|---|---|---|---|---|
| `shield_trooper` | Shield Trooper | shielded ranged line, anti-air | 300+120 | 36.7 | 150F 70Fe 110Ae 40Ad | barracks |
| `hover_tank` | Hover Tank | armour that crosses water (Marriage of the Sea) | 900+150 | 43.3 | 130Fe 150Si 220Ae 120Ad | engine_works, starport |
| `gunship` | Gunship | the Republic's first flyer; hunts hover tanks | 700+100 | 35 | 100Fe 160Si 190Ae 100Ad | starport |
| `star_bomber` | Star Bomber | the Republic's only Bomb: 5 m aether charges, x2.5 vs buildings, flattens blobs and walls | 650+100 | 30 | 100Fe 160Si 270Ae 120Ad | starport |
| `plasma_artillery` | Plasma Artillery | fortress breaker | 450+80 | 20 | 100Fe 150Si 250Ae 60Ad | engine_works |
| `tide_phaser_marine` **U** | Phaser Marines (3) | anti-armour squad; beams x1.3 vs machines, shoots air | 230+60 | 22.9 | 240F 150Cu 80Fe 190Ae 60Ad | tide_harbor, barracks |
| `tide_bucentaur` **U** | Bucentaur | giant hover flagship: railguns vs armour, phasers vs air | 3800+500 | 85.3 | 200Cu 350Fe 300Si 650Ae 500Ad | tide_arsenal |

## 6. Unique units

| id | age | look | weapon(s) | kind | notes |
|---|---|---|---|---|---|
| `tide_marine` | Hearth | 0.6x1.3x0.6, squad of 3 | `tide_boarding_axe` 5 Slash / 1.4 s | Melee Swing | pierce armour 0.15 (bucklers), crush 0; Boarding Drill +25% hp |
| `tide_stradiot` | Banner | 0.9x1.7x1.9 | `tide_stradiot_lance` 9 Pierce / 1.2 s, x1.5 vs Ranged and Siege | Melee Thrust | 6.2 m/s, swims; no barding techs, so stays light |
| `tide_varangian` | Banner | 0.75x1.5x0.75, red cloak | `tide_varangian_axe` 17 Slash / 1.8 s, splash 0.5, x1.5 vs Heavy, x1.25 vs Mechanical | Melee Smash | 2.9 m/s; light mail (slash 0.25, pierce 0.15, no shield); one target per blow, unlike the Ironhold Thane Guard's cleaving swing; Varangian Oath +20% hp, +10% damage |
| `tide_siphoneer` | Starfall | 0.7x1.4x0.7, bronze, pair | `tide_siphon` 9 Fire / 1.25 s, cone 35 deg, x1.25 Infantry, x1.5 Siege, x1.25 Building | **Spray** | range 4.5, a fixed cone (range techs do not lengthen it; Liquid Fire adds damage and rate), fire armour 0.35 |
| `tide_diver` | Engine | 0.8x1.6x0.8, brass | `tide_galvanic_harpoon` 40 Energy / 2.2 s, 1 jump of 2.5 m, x1.5 Mechanical | **Chain** | energy armour 0: the copper suit conducts |
| `tide_ironclad` | Engine | 2.8x2.4x3.8, verdigris | `tide_casemate_gun` 70 Crush Cannon, 12 m; `tide_torpedo` 110 Fire Homing, 15 m, min 3 | Cannon + **Homing** | Tracked: wades deep water through the Amphibious trait, no thickets; vision 15 so it spots its own torpedo shots; no anti-air; 2.4 m/s |
| `tide_phaser_marine` | Ascendant | 0.7x1.5x0.7, squad of 3 | `tide_phaser` 32 Energy beam / 1.4 s, x1.3 Mechanical, hits air | Beam | 60 shield per model; light armour |
| `tide_bucentaur` | Ascendant | 5x6x6 gilded hull, hover | `tide_broadside_rail` 2x130 Pierce, pierce 4, 17 m; `tide_phaser_battery` 22 Energy / 0.8 s, x1.75 Flying | Cannon + Beam | Giant, Hero; pop 10; turn rate 40; vision 17; railguns do not fire on the move |

Spray, Chain and Homing each have one or two users in the common pool; the Republic
adds a second Spray user (and the Siphon Tower), a second Chain user and a third
Homing user. Its Bomb comes from the common `star_bomber` at the starport, the
Arsenal's liquid fire grown into a 5 m aether charge.

**Value bands** (section 8 of the design doc; ehp = hp + shield; squads counted whole):

| unique | dps/cost | ehp/cost | age median (dps / ehp) | reading |
|---|---|---|---|---|
| `tide_marine` | 0.067 | 0.75 | 0.057 / 0.73 | the militia's numbers (0.067 / 0.76) with better pierce and worse crush armour |
| `tide_stradiot` | 0.047 | 0.75 | 0.054 / 0.76 | below both; pays for swimming raids |
| `tide_varangian` | 0.065 | 0.69 | 0.054 / 0.76 | above on dps, below on ehp (man-at-arms 0.067 / 0.76); the x1.5 vs Heavy is paid for with light armour, no cleave and a 145 price |
| `tide_siphoneer` | 0.063 | 0.87 | 0.067 / 0.81 | below on dps, above on ehp; the cone's extra targets on single-model blobs are not counted |
| `tide_diver` | 0.061 | 0.87 | 0.074 / 0.95 | below both; makes it back on machines (x1.5) and the chain |
| `tide_ironclad` | 0.057 | 1.27 | 0.074 / 0.95 | below on dps, above on ehp (landship 0.062 / 1.33); pays with speed and no anti-air |
| `tide_phaser_marine` | 0.095 | 1.21 | 0.095 / 1.45 | median dps, below on ehp |
| `tide_bucentaur` | 0.043 | 2.15 | titan 0.039 / 2.31 | a giant priced on the titan; less of both, more speed (3.2 vs 2.0) and water |

**Stat-only matchups** (Lanchester square law at equal cost, same method as design
doc 5.5: no range, kiting or micro; chain, cleave, cone and pierce reach count only
against squads):

| A | B | A's score (>1 wins) | A dps into B | B dps into A |
|---|---|---|---|---|
| `tide_marine` | `spearman` | 1.30 | 3.2 | 3.4 |
| `militia` | `tide_marine` | 1.12 | 3.3 | 3.2 |
| `man_at_arms` | `tide_marine` | 1.37 | 7.9 | 2.5 |
| `tide_stradiot` | `crossbowman` | 1.65 | 9.6 | 5.6 |
| `tide_stradiot` | `pikeman` | 0.31 | 6.8 | 12.8 |
| `knight` | `tide_stradiot` | 1.57 | 8.6 | 5.2 |
| `tide_varangian` | `pikeman` | 1.05 | 8.0 | 4.5 |
| `tide_varangian` | `man_at_arms` | 1.24 | 9.9 | 6.2 |
| `tide_varangian` | `knight` | 1.07 | 9.2 | 9.1 |
| `tide_varangian` | `ironhold_thane_guard` | 1.12 | 9.2 | 7.5 |
| `crossbowman` | `tide_varangian` | 0.85 | 7.4 | 9.0 |
| `tide_siphoneer` | `arquebusier` | 1.41 | 9.0 | 11.8 |
| `tide_siphoneer` | `rifle_squad` | 2.18 | 24.3 | 13.5 |
| `tide_siphoneer` | `halberdier` | 0.62 | 9.0 | 20.6 |
| `knight` | `tide_siphoneer` | 1.04 | 12.5 | 7.2 |
| `tide_diver` | `mech_walker` | 1.11 | 24.5 | 22.0 |
| `tide_diver` | `steam_tank` | 0.95 | 24.5 | 33.5 |
| `gatling_crew` | `tide_diver` | 1.18 | 24.4 | 18.2 |
| `shock_trooper` | `tide_diver` | 1.10 | 19.4 | 16.4 |
| `tide_ironclad` | `steam_tank` | 1.01 | 30.6 | 28.7 |
| `tide_ironclad` | `rifle_squad` | 3.13 | 65.8 | 7.1 |
| `shock_trooper` | `tide_ironclad` | 1.46 | 31.6 | 27.3 |
| `tide_phaser_marine` | `hover_tank` | 1.26 | 20.8 | 30.3 |
| `tide_phaser_marine` | `shield_trooper` | 0.83 | 17.1 | 33.7 |
| `tide_phaser_marine` | `storm_walker` | 0.46 | 23.8 | 125.3 |
| `tide_bucentaur` | `shield_trooper` | 1.14 | 65.7 | 20.2 |
| `tide_bucentaur` | `titan` | 0.72 | 61.6 | 104.2 |
| `tide_bucentaur` | `storm_walker` | 0.63 | 60.8 | 49.1 |
| `gunship` | `tide_bucentaur` | 1.54 | 24.1 | 38.5 |
| `tide_bucentaur` | `star_bomber` | 1.41 | 38.5 | 16.5 |
| `star_bomber` | `tide_phaser_marine` | 1.28 | 28.8 | 23.8 |

Ranged columns understate the ranged side (crossbows and arquebuses kite Varangians
and siphoneers; horse archers kite everything here). The Varangian scores 1.07-1.24
against the heavies at equal cost and no better than 1.05 against pikes: it is a
fast specialist, not a line. Against the Thane Guard the row counts one target; a
packed thane line cleaves two Varangians per swing and wins (0.56). The star bomber
row counts its 5 m charge on all three phaser marines; spread out, they win.

The siphoneer row against halberdiers counts one target per cone; into a packed block of halberdiers the cone
reaches two or three, which turns it around, but halberdiers that close in on a
thin line of siphoneers win.

## 7. Unique buildings

| id | age | size | hp | cost | role |
|---|---|---|---|---|---|
| `tide_harbor` | Hearth | 10x5x8 | 1700 | 200W | up to three; fishing quays (`resource_field` 3000 food, 2 peasants); drop site for food, wood and copper; trains marines, stradiots, divers, phaser marines; researches Drift Nets, Boarding Drill, Levant Convoys, Murano Glassworks |
| `tide_arsenal` | Starfall | 12x7x12 | 2400 | 250W 150S 150Cu 50Fe | production of siphoneers, ironclads and the Bucentaur; researches Liquid Fire, Copper Sheathing, Marriage of the Sea |
| `tide_siphon_tower` | Starfall | 3.5x6x3.5 | 1600 | 175S 100Cu 20Ae | wall and gate defence: `tide_tower_siphon` 18 Fire / 1.5 s in a 60 deg cone, 6 m, x1.25 Infantry, x1.75 Siege; no air, no buildings |

- The **Harbour** is the Republic's first unique decision: 200 wood buys fishing
  for two peasants (3000 food, the worth of 1.7 farms: 15 food per wood against a
  farm's 30, because the rest of the price buys three drop sites and a second barracks
  for marines), and the building outlives its fishing. Three at most, so the quays are
  a head start on food, not a replacement for farms. It is wooden and big, so it is
  also a juicy raid target.
- The **Siphon Tower** is the "thing that defends walls" that the Republic adds to
  the common tower chain. It shares the 3.5 m footprint of the tower chain but does
  not upgrade. It burns a battering ram (320 hp, no fire armour) in about 15 s on its
  own (12 s with Liquid Fire). Every Tower-tag tech applies to it, but only the
  damage and hp parts matter: the +1 range of ballistics and targeting computers
  does not lengthen its 6 m cone (design doc section 11). Like the bombard tower it cannot
  hit flyers, so it pairs with guard towers and, later, flak batteries.
- The **Arsenal** is where the strange half of the arc lives: it unlocks in the
  Starfall Age with the age's first unique and ends with the Bucentaur.

## 8. Techs

**Unique techs (9)**

| id | name | age | at | cost | time s | effects |
|---|---|---|---|---|---|---|
| `tide_drift_nets` | Drift Nets | Hearth | tide_harbor | 50F 150W | 35 | +0.3 food/s |
| `tide_boarding_drill` | Boarding Drill | Banner | tide_harbor | 150F 40Fe | 40 | `tide_marine` hp x1.25, slash armour +0.1 |
| `tide_levant_convoys` | Levant Convoys | Banner | tide_harbor | 150F 150W 50Cu | 50 | +0.25 copper/s, +0.15 iron/s |
| `tide_pavises` | Pavises | Banner | archery_range | 125F 125W | 40 | `crossbowman` and `arquebusier` pierce armour +0.1, slash +0.05 |
| `tide_murano_glass` | Murano Glassworks | Starfall | tide_harbor | 200F 200W 100Cu | 50 | silicon gather x1.2 |
| `tide_liquid_fire` | Liquid Fire | Starfall | tide_arsenal | 200F 150Cu 50Ae | 50 | `tide_siphoneer` and `tide_siphon_tower` damage x1.15, cooldown x0.9 (dps x1.28); no range, since range does not lengthen a Spray cone |
| `tide_varangian_oath` | Varangian Oath | Starfall | castle | 250F 125Fe 30Ae | 50 | `tide_varangian` hp x1.2, damage x1.1 |
| `tide_copper_sheathing` | Copper Sheathing | Engine | tide_arsenal | 300Cu 100Fe 75Si | 55 | Mechanical hp x1.1, fire and energy armour +0.05 |
| `tide_marriage_of_the_sea` | Marriage of the Sea (signature) | Ascendant | tide_arsenal | 500F 300Ae 200Ad | 80 | `hover_tank` and `tide_bucentaur` speed x1.15, damage x1.1; the sea's tribute: +0.5 food/s and +0.3 copper/s |

Marriage of the Sea pays out as tribute (a food and copper trickle) rather than
army-wide regeneration: Starborn's The Star Answers and the Sylvan regrowth techs
already own healing over time, and the Republic is the faction of trade.

**No unique line upgrades.** Every candidate pair broke the rule for `UpgradeUnit`
(pop-neutral, per-model cost ratio 1.6 or less): a marine model costs 53, a
Varangian 145 (ratio 2.7). So, as rule 3 asks, the better unit is trained-only and
the old one gets a stat drill: Boarding Drill for marines, Varangian Oath for the
guard. The common infantry lines (`pike_drill`, `halberd_drill`, `crossbow_drill`,
`hand_cannon_drill`) are all kept, and Pavises follow the crossbow line into
arquebusiers.

**Common techs left out (5):** `husbandry` (no stable), `scale_barding` and
`plate_barding` (light horse only; the Republic never armours its horses),
`militia_drill` (no militia), `steam_harness` (no men-at-arms). The other 48,
including all four age-ups, are kept.

## 9. What the Republic does not get, and why

| left out | why | what covers the role |
|---|---|---|
| `militia` | marines are the Republic's levy | `tide_marine` |
| `man_at_arms` | the Doge's guard is Varangian | `tide_varangian` |
| `knight`, `aether_lancer` | a merchant republic keeps no stud farms or knightly class | pikes and halberds stop horse; stradiots raid |
| `horse_archer` | a steppe art | javelineers and crossbows |
| `battering_ram` | the Arsenal burns gates instead of knocking | grenadiers, siphoneers, bombards |
| `trebuchet` | the Arsenal casts guns, not counterweights | bombards, later rockets and plasma artillery |
| `war_balloon`, `gyrocopter`, `biplane_bomber` | sea people, not sky people: no flyers before the Ascendant Age | crossbows, arquebuses, rifles, flak wagons, flak batteries and gun turrets shoot flyers down |
| `starfighter`, `drone_swarm` | even late, the Republic flies only gunships and star bombers | `gunship`, `star_bomber`, phaser battery on the Bucentaur |
| `shock_trooper` | heavy Engine-age foot is the brass diver | `tide_diver`, Varangians |
| `mech_walker` | the Arsenal's walker is the crab-legged ironclad | `tide_ironclad` |
| `storm_walker`, `titan` | the Republic's capital ship is the Bucentaur | `tide_bucentaur` |
| building `stable` | no stud farms; scouts come from the town centre, stradiots off the Harbour's transports | `town_center`, `tide_harbor` |
| building `airfield` | no early flight | `starport` in the Ascendant Age |

The result over time: strong infantry and walls throughout, early liquid fire,
amphibious reach from the first marine (and from the first tracked machine), no
heavy cavalry ever, and no air until the very end, when gunships and star bombers
leave the starport. The Republic gets every wall and tower tier and the castle: it is the
faction that wants to sit behind stone and send boats round the side.

## 10. Counters and weaknesses

**Faction-level weaknesses**

- **Cavalry on open ground.** No knights or lancers to counter-charge, and no horse
  archers. Horse archers and heavy cavalry are answered only by pikes, halberds,
  javelineers and crossbows standing in the right place. A Tidecrown army caught
  moving on dry land is in trouble.
- **The sky.** No flyers until gunships and star bombers in the Ascendant Age. War balloons, gyros
  and bombers must be shot down from the ground, so flak batteries and gun turrets
  belong inside every Engine-age base.
- **Banner-age sieges.** With no ram or trebuchet, the mangonel is the only Banner
  answer to stone walls. The Republic should not plan to break a walled enemy before
  the Starfall Age (bombards, grenadiers, siphoneers).
- **Split armies.** Only foot, mounted, tracked machines and (late) hover cross
  deep water. Mangonels, ballistae, bombards, rockets and flak wagons go the long way
  round, and tracked machines still slog through marsh at x0.6.

**What beats the uniques**

| unique | beaten by |
|---|---|
| `tide_marine` | men-at-arms and Varangian-style heavies, clubs and maces (crush armour 0), mangonels |
| `tide_stradiot` | spears, pikes, halberds, knights; guard towers |
| `tide_varangian` | crossbows, arquebuses and rifles (x1.25-1.35 vs Heavy, pierce armour 0.15), kiting; pike and spear lines (no Heavy tag, so no axe bonus); a packed Thane Guard line that cleaves |
| `tide_siphoneer` | anything that out-ranges 4.5 m: archers, guns, horse archers, artillery; javelineers (x2.5 vs Ranged); cavalry charges |
| `tide_diver` | gatlings, shock troopers, any energy weapon (aether lances, tesla, lasers: energy armour 0) |
| `tide_ironclad` | shock troopers, aether lancers, hover tanks, bombers and gunships (no anti-air), halberds in numbers |
| `tide_phaser_marine` | storm walkers (tesla chains through the squad), gatlings, shield troopers, plasma artillery |
| `tide_bucentaur` | gunships and starfighters, massed hover tanks, storm-walker railguns (x1.5 vs Giant), EMP (x2.5 Mechanical, x2 Shielded) |

**What the Republic beats:** armies that rely on rivers and lakes for flank cover;
slow siege that has to walk to its walls past siphon towers; infantry blobs that
meet liquid fire; machine-heavy late armies (divers, phasers, railguns).

## 11. AI notes

`ai` in the data: aggression 0.45, army weights `tide_marine` 0.7, `crossbowman` 1.0,
`tide_varangian` 0.8, `arquebusier` 1.0, `bombard` 0.4, `tide_siphoneer` 0.6,
`rifle_squad` 1.0, `tide_ironclad` 0.7, `tide_phaser_marine` 1.0, `tide_bucentaur`
0.3; eco focus food, wood, copper, iron, aether, silicon, adamant, stone. The AI
reads weights by unit id, so `arquebusier` has its own weight (Hand Cannon Drill
removes the crossbowman from the menu), and `bombard` gives the Starfall army a
Siege unit before the Engine-age ironclad.

Behaviour hints for whoever writes the AI:

- Build the Harbour as the second military building, put two peasants on its
  fishing quays and research Drift Nets in the Hearth Age; research Levant Convoys
  early in the Banner Age.
- Keep a pike or halberd screen with every crossbow/arquebus group. Without
  cavalry, the AI must never let its ranged units stand alone.
- Prefer routes through deep water and marsh when the army is foot-only. Tracked
  machines may take the deep-water route but not the marsh one (x0.6). Leave wheeled
  units on the land route, or keep them home.
- From the Starfall Age, approach the crater over its marsh, put an extractor on a
  vent, and ring it with one or two siphon towers.
- Build flak batteries and gun turrets in the Engine Age; the AI has no flyers to
  answer air raids until the starport.
- Raid with stradiots and divers across water toward mills, mining camps and
  extractors that the enemy's land army cannot reach quickly.
- Train the Bucentaur late and escort it with phaser marines and shield troopers;
  pull it back from gunships.

## 12. Notes for the engine

- **Amphibious covers Tracked.** The faction grants Amphibious to Foot, Mounted and
  Tracked, so `tide_ironclad` (Tracked), `steam_tank`, `clockwork_automaton` and
  `plasma_artillery` enter deep water (x0.7) and take the Amphibious x0.9 in shallows
  over Tracked's x0.6 (best override wins). Marshwalker stays Foot-only and no
  tracked machine enters thickets. The ironclad is 2.8 m wide (k = 1 on the nav grid,
  as for any tank). `docs/design/game-design.md` section 7 still lists the tide
  Amphibious trait as Foot and Mounted only.
- **`tide_harbor` placement and fishing.** The description says "best built on a
  shore". Nothing enforces it, and nothing should need to: maps without water must
  still allow it, and its `resource_field` (3000 food, 2 slots, three harbours at
  most) works anywhere, like a farm. A fished-out harbour keeps its drop site, queue
  and research (af_behavior's gather code removes only the field). A later rule could
  give it a bonus when it touches shallow water. It is a food drop site too, so its
  fishers never walk.
- **Spray damage** (`tide_siphon`, `tide_tower_siphon`) is read the same way as the
  common `flame_projector`: `damage` applies to each target in the cone per attack.
  The cone length is the authored `range` and no Tidecrown tech changes it. The
  common range techs still add acquisition reach (siphoneers are Ranged: fletching
  +0.5, targeting computers +1; the siphon tower is a Tower: ballistics and targeting
  computers +1 each). The engine should only fire a Spray at a target inside its
  cone, or these units will waste shots just beyond 4.5 m and 6 m.
- **Chain damage** (`tide_galvanic_harpoon`) is assumed to deal full damage on the
  jump, as the design doc assumes for the tesla coil.
- **`tide_bucentaur`** is a Hover ground target (design doc 5.3), a 5 x 6 x 6 m box
  (5 m wide, the giant cap, so k = 2 and a 5-cell lane; its turning sweep is a 3.9 m
  radius) with a turn rate of 40, and carries `Hero` like the titan. Its broadside
  railguns do not fire on the move; its phaser battery does. The railgun look
  (offset x 2.6) sits half outside the hull on purpose so the broadside is visible.
- **`tide_varangian_axe`** is a Smash with a 0.5 m splash at the impact point; in a
  packed line it may graze the target's neighbour. The matchups count one target.
- **`tide_siphon_tower`** is a Tower that cannot hit air, like the bombard tower.
