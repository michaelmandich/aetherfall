# The Sunspire Caliphate (`dune`)

*"Sand-mages and glass-wrights of the deep desert."*

| Item | Value |
|---|---|
| Faction id | `dune` |
| Data file | `assets/data/factions/dune.ron` |
| Colour | sun-saffron `(0.96, 0.62, 0.12)`: a warm orange-gold, no grey |
| Trait | **Dunerunner** for Foot, Mounted and Wheeled |
| Gather bonus | silicon x1.25, aether x1.2 |
| Worker | common `peasant` (gathers all 8 resources); `engineer` from the Engine Age |
| Start | 300 food, 300 wood, 150 stone, 100 copper; 5 peasants, 1 scout rider; a town centre |
| Roster | 34 trainable units: 24 common combat units, 2 common workers, 8 uniques |
| Buildings | 37: 34 common, 3 unique |
| Techs | 52: 43 common (age-ups included), 9 unique |

Validated with
`cargo run -q -p af_data --example check -- --only factions/dune.ron --summary`
(result at the end of this document).

---

## 1. Identity

The Sunspire is a tower of sun-fused glass in the deep desert, half palace and half
observatory. Its astronomers, the Calculists, measured the red star for a
generation. The Synod of the Fallen Star treated it as a sign. The Sunspire treated
it as a problem in arithmetic, and solved it: the year of the fall, the season, and
roughly where the light would land. When the star broke over the heartland, the
Synod reached it first, but the Caliph's caravans were the ones that knew where the
shards would scatter.

The desert had always been their workshop. Glass-wrights fused quartz sand with
mirrors and sunlight, and the sand-mages of the House of Wisdom could make dunes
move with chant and bell. Aether answered both arts. It holds light, so a mirror
lit with aether burns at night as well as noon. It answers resonance, so a tuned
bell can lift a skiff or turn sand into a scouring storm. Most of all it can be
spun into a *veil*: a humming lattice of aether-glass around a soldier that turns
fast missiles but lets a slow blade through.

They distrust one thing: an engine that thinks for a man. Their thinking engines
are people. The Calculist Viziers do in their heads what other realms do with brass
fire-control engines and punch cards, and no Caliph has allowed a drone to fly over
the Sunspire. Clockwork is permitted, because an automaton is wound, not reasoning.

**Flavour in one line:** scimitars, camels and lamellar under a saffron banner;
then burning mirrors, sonic skiffs and bullet-turning veils; never a machine that
thinks.

### Visual language (boxes)

| Thing | Box read |
|---|---|
| Units | faction saffron mixed with sand, gold and pale-brass accents |
| Camel rider | taller than a horseman (2.0 m), long scimitar box held high |
| Sunglass engine / tower | a big flat square mirror box on a cart or on the roof; the beam is a gold glowing box |
| Sand mage | thin staff box; her Nova is a sand-coloured flat ring |
| Veiled Blades | slim duellists, short dagger box; shield shimmer from the engine |
| Sandskiff | low flat hull with a brass horn on top; the shot is a thin glowing ring that flies forward |
| Great Sand-Engine | 4.5 x 6 x 6 m crawler with a brass bell on top and a glass-furnace maw poking out in front |

---

## 2. Playstyle and win conditions

**Reckon, claim, outrun.** The Caliphate is a tempo and manoeuvre faction. It is
average in the Hearth Age, gathers the crater's aether fastest in the Banner Age, and
feeds its own silicon at home from the Starfall Age, so it reaches the Engine Age
early without having to win the fight for the mid-ring dunes. Its army is light,
fast and specialised: cheap cleaving swordsmen, camels that ruin cavalry, mirrors
that burn siege, mages that punish crowds. It wins by choosing where to fight.

| Win condition | How |
|---|---|
| **Tech race** | Star Reckoning and the aether bonus speed the Starfall age-up; Quartz Sifters and the silicon bonus cover the Engine age-up (200 silicon) at home. Calculist Viziers then speed up all training by 15%. |
| **Own the sand around the crater** | Dunerunner keeps foot, camels, horse archers, aether lancers, bombards, mirror engines and the sand-engine at full speed on dunes, where enemy cavalry and carts crawl at x0.7. Fight there. |
| **Burn their siege** | Sunglass Engines and Towers take x2 against Siege. A push of bombards and trebuchets into a Dune base melts. |
| **Late raid and crush** | Sandskiffs (7 m/s hover, crosses water and marsh) raid crater workers and kill artillery; the Great Sand-Engine walks an army of shield troopers into the enemy base and flattens infantry and walls. |

**Where it loses:** straight slugfests against heavy infantry and knights (no
men-at-arms, no knights, no healers), turtling (no castle, no late wall techs),
and the late sky (no Ascendant flyers at all).

### Rough build arc

| Age | Your focus |
|---|---|
| Hearth | Standard boom. One or two camel riders from the town centre punish enemy scouts and archers. |
| Banner | House of Wisdom early, **Star Reckoning**, then workers to the crater crystals. Sunspire Guards plus pikes and crossbows. Camel Studs if cavalry matters. |
| Starfall | Two to four **Quartz Sifters** at home, citadel, aether foundry. Extractors in the crater, held by halberds, arquebusiers and Sunglass Towers on the rim. Sand mages behind the guards. |
| Engine | Veiled Blades and rifles as the line, mech walkers and rockets as armour, flak against air. Ornithopters bomb extractors and mining camps. **Calculist Viziers.** |
| Ascendant | Sandskiff raids, shield troopers and storm walkers as the core, one Great Sand-Engine for the base push. **The Great Lens** if the game goes long. |

---

## 3. Economy

| Resource | Caliphate rate | Notes |
|---|---|---|
| food, wood, stone, copper, iron | baseline | no bonus; the desert is not rich in timber |
| silicon | **x1.25** (peasant 0.50/s, engineer 0.69/s) | plus **Glass Kilns** x1.2 and common Deep Shafts x1.25: a peasant reaches 0.75/s |
| aether | **x1.2** (peasant 0.36/s, engineer 0.48/s) | plus **Star Reckoning** x1.15 from the Banner Age and Aether Resonance x1.25: a peasant reaches 0.52/s |
| adamant | baseline | the crater fight still decides the endgame |

**The Quartz Sifter** (Starfall, 150W 50S 75Cu) is a resource field like a farm:
800 silicon, two workers at a time, up to four per player. Workers drop at a mining
camp or town centre as usual. Two peasants on one sifter make 1.0 silicon/s, so
(counting gathering time only) a single sifter pays the Engine age-up's 200 silicon
in about three and a half minutes and two sifters in under two. The Caliphate can still take the mid-ring dunes (it walks there at full
speed), but it does not *have* to.

**Aether.** The 100 aether for `to_starfall` takes three crystal workers about 80 s
with Star Reckoning, against about 111 s for a faction without bonuses (gathering
time only). From the Starfall Age the Caliphate fights for vents like everyone
else. Late on, **The Great Lens** adds 0.4 aether/s and 0.3 silicon/s that no
enemy can raid.

**What the economy lacks:** no Star Tithe (no temple) and no healer, so losses are
paid in food and iron, not recovered. There is no wood bonus, and wood-hungry
Banner siege (rams, ballistae) competes with the House of Wisdom.

---

## 4. Terrain trait: Dunerunner

Speed multipliers on `sand` (from `core/terrain.ron`):

| Locomotion | Everyone else | Caliphate |
|---|---|---|
| Foot | x0.9 | **x1.0** |
| Mounted | x0.7 | **x1.0** |
| Wheeled | x0.7 | **x1.0** |
| Tracked | x0.85 | x0.85 (no trait) |
| Hover | x1.0 | x1.0 |

- **Who benefits:** peasants on the quartz-sand deposits; camel riders, horse
  archers and aether lancers; rams, ballistae, bombards, Sunglass Engines, rocket
  batteries, flak wagons; the wheeled Great Sand-Engine.
- **Who does not:** the tracked common machines (clockwork automaton, mech walker,
  storm walker, plasma artillery). The Caliphate prefers wheels and hover partly for
  this reason, which is why its giant rolls rather than walks.
- **Use it:** pick fights on the dune patches of the mid ring and around the crater.
  An enemy knight charge across sand arrives at 70% speed, and Caliphate horse
  archers can kite there indefinitely. Retreat over dunes; pursuers fall behind.
- **The cost:** wheels need dry land. The Great Sand-Engine, the mirror engines and
  every wagon cannot enter shallows, marsh or thickets, so a river with only fords
  blocks them. Sandskiffs (hover) are the Caliphate's way over water and marsh.

---

## 5. Roster by age

[U] marks a unique. Roles are the job the unit does for this faction.

### Hearth Age (6)

| id | name | role |
|---|---|---|
| `peasant` | Peasant | worker: all 8 resources |
| `scout_rider` | Scout Rider | scouting; finds the crater and the dunes |
| `spearman` | Spearman | anti-cavalry line |
| `slinger` | Slinger Band | squad skirmisher; bruises infantry and palisades |
| `archer` | Archer | ranged line, shoots air |
| `dune_camel_rider` [U] | Camel Rider | anti-cavalry raider: beats scouts, archers and horse archers; dies to spears |

### Banner Age (7)

| id | name | role |
|---|---|---|
| `pikeman` | Pikeman | anti-cavalry, anti-beast (via Pike Drill) |
| `crossbowman` | Crossbowman | anti-armour ranged, shoots air |
| `javelineer` | Javelineer | hunts archers and horse archers |
| `horse_archer` | Horse Archer | kiting cavalry; best on the dunes |
| `battering_ram` | Battering Ram | the Banner wall-breaker |
| `ballista` | Ballista | anti-siege, anti-big |
| `dune_sun_guard` [U] | Sunspire Guard | cheap fast melee line that cleaves two; beats pikes, crossbows and squads |

### Starfall Age (8)

| id | name | role |
|---|---|---|
| `halberdier` | Halberdier | polearm line; anti-cavalry, anti-clockwork |
| `arquebusier` | Arquebusier | heavy ranged line |
| `grenadier` | Grenadier Pair | anti-blob, anti-gate |
| `aether_lancer` | Aether Lancer | the Caliphate's only heavy cavalry; hunts gunners and machines |
| `clockwork_automaton` | Clockwork Automaton | bullet sponge |
| `bombard` | Bombard | wall-breaker |
| `dune_mirror_engine` [U] | Sunglass Engine | burns siege (x2), balloons and timber at 14 m; shoots air |
| `dune_sand_mage` [U] | Sand Mage | 3 m Nova caster right behind the melee line; shreds crowds and squads |

### Engine Age (7)

| id | name | role |
|---|---|---|
| `engineer` | Engineer | late worker; silicon and aether bonuses apply |
| `rifle_squad` | Rifle Squad | ranged line, shoots air |
| `mech_walker` | Mech Walker | the Caliphate's armour; also hunts flyers |
| `rocket_battery` | Rocket Battery | long-range siege |
| `flak_wagon` | Flak Wagon | anti-air (the Caliphate needs it) |
| `dune_ornithopter` [U] | Ornithopter Flight | squad of 2 light bombers: raids camps, extractors and blobs |
| `dune_veil_guard` [U] | Veiled Blades | squad of 3 shielded duellists: beats gunlines and enemy shields |

### Ascendant Age (6)

| id | name | role |
|---|---|---|
| `shield_trooper` | Shield Trooper | ranged line; the Caliphate's main anti-air infantry |
| `hover_tank` | Hover Tank | heavy armour over water and sand |
| `plasma_artillery` | Plasma Artillery | fortress-breaker |
| `storm_walker` | Storm Walker | anti-armour, anti-giant; tesla shoots air |
| `dune_sandskiff` [U] | Sandskiff | fast hover raider; sonic cannon pierces a file of 3 |
| `dune_sand_engine` [U] | Great Sand-Engine | giant: crushes infantry blobs and walls; blind to the sky |

### Counting

- **Common units:** 26 of 43 kept (24 combat units plus `peasant` and `engineer`);
  17 excluded. That is 41% of the 41 combat units, or 40% of all 43. Leaving out the
  two benchmarks that no faction trains (`shock_trooper`, `titan`), it is 15 of 39
  (38%).
- **Common buildings:** 34 of 36 kept (no `temple`, no `castle`).
- **Common techs:** 43 of 53 kept.
- Across the whole common pool (132 entries) the Caliphate leaves out 29 (22%).
  Most of the variety is in the army, where it shows.

---

## 6. The uniques

Bands are the common-pool medians from `game-design.md` section 8. dps/cost and
ehp/cost count the whole squad; ehp is hp plus shield.

| id | age | tags | squad | pop/model | cost (per training) | hp/model | armour % | speed | moves | weapon | dps/model | range | trained at | dps/cost (age median) | ehp/cost (age median) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `dune_camel_rider` | Hearth | Cavalry, Melee, Light | 1 | 2 | 70F 30W (100) | 90 | S10 P10 | 5.6 | Mounted | camel scimitar: 6 Slash swing, x1.75 Cavalry, x1.5 Beast | 4.3 | 0.9 | town_center, citadel, stable | 0.043 (0.057) | 0.90 (0.73) |
| `dune_sun_guard` | Banner | Infantry, Melee | 1 | 1 | 60F 25W 35Fe (120) | 90 | S20 P15 C5 | 3.0 | Foot | scimitar: 8 Slash swing 90°, cleave 1 | 8.0 | 0.8 | barracks | 0.067 (0.054) | 0.75 (0.76) |
| `dune_mirror_engine` | Starfall | Siege | 1 | 3 | 120W 60Cu 60Fe 30Si 30Ae (300) | 200 | S20 P50 F30 | 1.6 | Wheeled | sunglass mirror: 70 Fire beam, x2 Siege, x1.5 Building, hits air | 14.0 | 14 | siege_workshop | 0.047 (bombard 0.043) | 0.67 (bombard 0.69) |
| `dune_sand_mage` | Starfall | Infantry, Ranged, Light | 1 | 1 | 70F 40Cu 40Si 40Ae (190) | 110 | S10 P10 F10 E10 | 2.8 | Foot | sandstorm: 14 Slash Nova r3 every 3 s, x1.2 Infantry, ground units only | 4.7 per target | 3 | dune_house_of_wisdom | 0.025 single, 0.098 at her radius (4 targets) (0.067) | 0.58 (0.81) |
| `dune_ornithopter` | Engine | Flying, Mechanical, Light | 2 | 2 | 120W 60Cu 100Fe 100Si 20Ae (400) | 170 | S15 P15 | 7.5 | Flying (12.5 m) | glassfire bombs: 55 Fire Bomb, splash 2, x2 Building, x1.2 Infantry | 11.0 | 1 | airfield | 0.055 (0.074) | 0.85 (0.95) |
| `dune_veil_guard` | Engine | Infantry, Melee, Shielded | 3 | 1 | 260F 60Cu 240Fe 100Si 30Ae (690) | 150 + 60 shield | S15 P35 C5 F5 | 3.1 | Foot | veil khanjar: 19 Pierce thrust, x1.5 Shielded, x1.2 Ranged | 21.1 | 0.7 | barracks | 0.092 (0.074) | 0.91 (0.95) |
| `dune_sandskiff` | Ascendant | Mechanical, Light, Shielded | 1 | 3 | 80Fe 120Si 160Ae 60Ad (420) | 450 + 100 shield | S30 P30 C15 F20 E15 | 7.0 | Hover | sonic cannon: 60 Crush Cannon, pierce 3, x1.25 Infantry, x0.6 Building, fires on the move | 30.0 | 12 | starport, engine_works | 0.071 (0.095) | 1.31 (1.45) |
| `dune_sand_engine` | Ascendant | Mechanical, Giant, Heavy | 1 | 10 | 280Fe 300Si 570Ae 400Ad (1550) | 3600, regen 3/s | S60 P60 C40 F50 E25 | 2.2 | Wheeled | glass torrent (50 Fire Spray, 40° cone, r6) + sonic roar (80 Crush Nova, r4.5) | 45.0 | 6 / 4.5 | engine_works | 0.029 single (titan 0.039) | 2.32 (titan 2.31) |

Every unique stays within about 1.5x of its age median on both axes. Area weapons
are priced at their radius, which is fixed: `Stat Range` never grows a Nova or a
Spray (`game-design.md` section 10, rule 10). The Sunglass Engine and the
sand-engine are compared with their common equivalents (bombard, titan), as
section 8 prices siege and giants.

- **Sand Mage:** a 3 m ring is 28 m². In a melee press about half the ring is her
  own side, at roughly one enemy model per 6.4 m² of ring, so she reaches about four
  models: 18.7 dps, 0.098 per cost (1.46x the median), against 0.58 ehp per cost
  (0.72x). The storm pulses every 3 s, so a lone target takes only 4.7 dps.
- **Great Sand-Engine:** 45 single-target dps (0.029 per cost, three quarters of the
  titan's 0.039). Counted as per-target dps times area, its output is about 1590: the
  roar's 20 dps over a 4.5 m ring (63.6 m²) is about 1270, and the torrent's 25 dps
  over a 40° x 6 m cone (12.6 m²) is about 310. The titan's stomp is 30 dps over a
  4 m ring (50.3 m²), about 1510, on top of 72 single-target dps from its star
  cannon. So per cost the sand-engine is the stronger crowd-breaker and the titan
  the far stronger duellist.

Like the sand mage, two more uniques sit above the median on one axis and below it
on the other:

- **Sunspire Guard:** dps above (0.067 vs 0.054) because it is cheap and cleaves;
  ehp just below (0.75 vs 0.76) because it wears lamellar, not mail, and has no `Heavy`
  tag. Men-at-arms and knights beat it.
- **Veiled Blades:** dps above (0.092 vs 0.074); ehp just below (0.91 vs 0.95).
  The veil plus 35% pierce armour makes them strong against guns, while 5% crush and
  fire armour leave them open to mauls, flamers, tesla and EMP (x2 vs Shielded).

The others pay with something else. The camel rider has high ehp but low dps and
dies to spears. The sand mage only pays with three or more enemies inside her 3 m
ring, so she has to stand right behind the melee line. The ornithopter cannot touch
other flyers. The sandskiff has no anti-air and light
armour. The sand-engine has no anti-air, a 4.5-6 m reach, and wheels.

### Unique weapons (10)

| id | kind | damage | cd s | dps | range | splash | hits | bonus | used by |
|---|---|---|---|---|---|---|---|---|---|
| `dune_camel_scimitar` | Melee Swing 70° | 6 Slash | 1.4 | 4.3 | 0.9 | - | GB | Cavalry x1.75, Beast x1.5 | camel rider |
| `dune_scimitar` | Melee Swing 90°, cleave 1 | 8 Slash | 1.0 | 8.0 | 0.8 | - | GB | - | Sunspire Guard |
| `dune_sunglass_beam` | Beam 1.0 s, 5 ticks | 70 Fire | 5.0 | 14.0 | 14 | - | GAB | Siege x2, Building x1.5 | Sunglass Engine |
| `dune_sandstorm` | Nova | 14 Slash | 3.0 | 4.7 per target | 3 | ring | G | Infantry x1.2 | Sand Mage |
| `dune_glass_bomb` | Bomb (0.9 s fall) | 55 Fire | 5.0 | 11.0 | 1 | 2.0 | GB | Building x2, Infantry x1.2 | Ornithopter |
| `dune_khanjar` | Melee Thrust | 19 Pierce | 0.9 | 21.1 | 0.7 | - | GB | Shielded x1.5, Ranged x1.2 | Veiled Blades |
| `dune_sonic_cannon` | Cannon, pierce 3 | 60 Crush | 2.0 | 30.0 | 12 | - | GB | Infantry x1.25, Building x0.6 | Sandskiff |
| `dune_glass_torrent` | Spray 40°, 16 particles | 50 Fire | 2.0 | 25.0 | 6 | cone | GB | Infantry x1.25, Building x1.25 | Great Sand-Engine |
| `dune_sonic_roar` | Nova | 80 Crush | 4.0 | 20.0 per target | 4.5 | ring | GB | Infantry x1.3, Building x1.5 | Great Sand-Engine |
| `dune_tower_mirror` | Beam 0.8 s, 4 ticks | 45 Fire | 3.0 | 15.0 | 13 | - | GAB | Siege x2, Mechanical x1.25 | Sunglass Tower |

Weapon kinds that are rare in the common pool, used here for flavour: a **Beam in
the Starfall Age** (the common pool has none before the Ascendant), two **Novas**
outside the titan and shield generator, a second **Spray**, and a **Cannon that
fires sound** (crush, pierce 3). Every weapon has a look box, and every one that
fires something has a projectile box; the light and sound weapons glow.

---

## 7. Unique buildings (3)

| id | name | age | cost | build s | hp | armour % | size m | role |
|---|---|---|---|---|---|---|---|---|
| `dune_house_of_wisdom` | House of Wisdom | Banner | 150W 150S 75Cu | 60 | 1800 | S65 P85 C15 F30 E25 | 8 x 8 x 8 | observatory and academy: trains `dune_sand_mage`; researches Star Reckoning, Parabolic Mirrors, Calculist Viziers, Sonic Resonators; vision 14 |
| `dune_quartz_sifter` | Quartz Sifter | Starfall | 150W 50S 75Cu | 30 | 500 | S50 P80 E20 | 6 x 1.2 x 6 | resource field: 800 silicon, 2 workers; limit 4 per player |
| `dune_mirror_tower` | Sunglass Tower | Starfall | 100W 200S 50Fe 40Si 30Ae | 75 | 1500 | S65 P85 C15 F40 E25 | 3.5 x 9 x 3.5 | tower: 45 Fire beam every 3 s at 13 m, x2 Siege, x1.25 Mechanical, hits air (15 dps) |

The Sunglass Tower stands alone (no upgrade chain) beside the common tower chain,
which the Caliphate keeps whole. It carries the `Tower` tag, so Fletching, Rifled
Barrels and Ballistics improve it (the Caliphate has no Targeting Computers). With
Parabolic Mirrors, The Great Lens and those tower techs it reaches about 27 dps at
17.5 m; every range step brings the same vision, so it sees 18.5 m. Its mirror sits
on the 9 m roof and tops out at 10.8 m, under every flyer.

### Walls and defence

The Caliphate builds the common walls, gates and the full tower chain
(`palisade_*` -> `stone_*` -> `reinforced_*` -> `force_*`, `watchtower` ->
`plasma_turret`), but it has **no castle** and **no Ferro-Concrete or Force
Lattice**. Its walls match everyone else's up to the Starfall Age and fall behind
from the Engine Age. It defends with things that shoot rather than things that
endure:

| Layer | What |
|---|---|
| Wall line | stone walls, the tower chain, Sunglass Towers on the line to burn siege |
| Interior | citadel arrows, Sunglass Towers, flak batteries, shield generator |
| Crater claim | Sunglass Towers on the crater rim next to the extractors; an extractor with two towers beside it costs an enemy ram or bombard push dearly |

---

## 8. Unique techs (9)

| id | name | age | at | cost | time s | requires | effects |
|---|---|---|---|---|---|---|---|
| `dune_star_reckoning` | Star Reckoning | Banner | `dune_house_of_wisdom` | 150F 100W 50Cu | 40 | - | aether gather x1.15; Scout vision +3 |
| `dune_camel_breeding` | Camel Studs | Banner | `stable` | 175F 50W 50Cu | 40 | - | camel rider hp x1.25, damage x1.2 |
| `dune_glass_kilns` | Glass Kilns | Starfall | `mining_camp` | 200F 150W 50Cu | 45 | - | silicon gather x1.2 |
| `dune_aether_steel` | Aether Steel | Starfall | `barracks` | 200F 100Fe 30Ae | 45 | - | Sunspire Guard damage x1.15, hp x1.2, pierce armour +0.05 |
| `dune_burning_glass` | Parabolic Mirrors | Starfall | `dune_house_of_wisdom` | 150W 100Cu 100Si 40Ae | 50 | - | Sunglass Engine and Tower damage x1.15, range +1.5; Tower vision +1.5 |
| `dune_calculists` | Calculist Viziers | Engine | `dune_house_of_wisdom` | 350F 150Cu 100Si 50Ae | 60 | - | all units train time x0.85; Siege range +1 |
| `dune_veil_discipline` | Veil Discipline | Engine | `barracks` | 250F 100Fe 75Si 50Ae | 50 | - | Veiled Blades shield hp x1.35, speed x1.1 |
| `dune_sonic_resonance` | Sonic Resonators | Ascendant | `dune_house_of_wisdom` | 200Si 200Ae 75Ad | 60 | - | Sand Mage, Sandskiff and Great Sand-Engine damage x1.2 |
| `dune_great_lens` | The Great Lens (signature) | Ascendant | `citadel` | 400S 300Si 300Ae 150Ad | 90 | `dune_burning_glass` | +0.4 aether/s, +0.3 silicon/s; Sunglass Engine and Tower damage x1.3, range +2; Tower vision +2 |

- **Economy perks:** Star Reckoning, Glass Kilns, The Great Lens.
- **Unit upgrades:** Camel Studs, Aether Steel and Veil Discipline are stat
  drills, not `UpgradeUnit` conversions. No unique pair in this roster is
  pop-neutral within a 1.6 per-model cost ratio. Camel to aether lancer is 2.6, and
  Sunspire Guard to Veiled Blades jumps two ages. So, following design rule 3, the
  better unit is trained-only and the older one gets a drill that keeps it useful
  for one more age.
- **Calculist Viziers** replaces the common `analytical_engine` (the same 15%
  training speed, plus Siege +1 range) and costs aether and food instead of extra
  copper and silicon: the human computers have to eat.
- **No Targeting Computers.** Brass fire-control engines that pick their own targets
  are what the Calculists' edict forbids. Calculist Viziers' Siege +1 range is the
  substitute, so Caliphate rifles, machines and towers do without the Engine Age
  +1 range that other armies get.
- **The Great Lens** is researched at the citadel, which is the Sunspire itself.
  It is safe there: the citadel has no upgrade, so nothing can strand the research.

The common line upgrades that are kept all have both ends listed: Pike Drill
(spearman, pikeman), Halberds (pikeman, halberdier), Crossbows (archer,
crossbowman), Hand Cannons (crossbowman, arquebusier).

---

## 9. What is excluded and why

### Units (17 of 43)

| Excluded | Why | What fills the gap |
|---|---|---|
| `militia` | No village levies; the first soldiers are paid spears and camel riders. | spearman, camel rider |
| `man_at_arms` | No heavy mail in the desert heat. | Sunspire Guard (lighter, faster, cleaves) |
| `knight` | No heavy lancers until aether. Camels, not destriers. | camel rider (Hearth), aether lancer (Starfall) |
| `monk` | Scholars, not priests: no healers at all. | shields and regen techs only |
| `mangonel`, `trebuchet` | No timber for great throwing frames. | ram and ballista (Banner), bombard and Sunglass Engine (Starfall) |
| `war_balloon` | The Caliphate does not fly until it has engines. | ornithopters (Engine) |
| `steam_tank` | Landships bog in the dunes. | mech walker, rockets, halberds, aether lancers |
| `gatling_crew` | A 2.2 m/s gun crew cannot keep pace with a tempo army, and the Sunspire answers gunlines with blades, not more guns. | rifle squads at range; Veiled Blades (x1.2 Ranged, bullet-turning veils) against enemy gunlines |
| `shock_trooper` | The Sunspire's elite infantry duels, it does not smash. | Veiled Blades |
| `gyrocopter`, `biplane_bomber` | Replaced by its own flyer. | Ornithopter Flight |
| `gunship`, `starfighter`, `star_bomber` | The late sky belongs to others; the Caliphate goes over the ground instead. Its bombs come from ornithopters, its big blasts from the sand-engine's roar. | sandskiff, Great Sand-Engine; anti-air from shield troopers, storm walkers, flak, plasma turrets |
| `drone_swarm` | The Calculists' edict: no engine may think for a man. | Calculist Viziers (people, not machines) |
| `titan` | Its own giant. | Great Sand-Engine |

**Combos over time this creates:** no heavy cavalry until the Starfall Age, then
only aether lancers. No healers ever. Siege is ram-and-ballista only in the Banner
Age: rams have to reach the wall (35 s per stone segment) and nothing breaks walls
from range until the bombard. Fire beams and bombards follow in the Starfall Age.
The Engine gunline is rifles only, with no gatlings. Flyers come in the Engine Age
as bombers only, with no fighters and no star bombers ever. There is no landship
and no titan; the giant it does have is anti-army, not anti-giant.

### Buildings (2 of 36)

| Excluded | Why |
|---|---|
| `temple` | No monks, so no Herbal Medicine or Star Tithe. The House of Wisdom is the Caliphate's "temple". |
| `castle` | The Caliphate builds observatories, not keeps. No garrisoned keep, no castle bombard, no Conscription. Interior defence is towers, Sunglass Towers and the citadel. |

### Techs (10 of 53)

| Excluded | Why |
|---|---|
| `militia_drill`, `steam_harness` | their units (militia, man-at-arms) are not in the roster |
| `herbal_medicine`, `star_tithe` | researched at the temple |
| `conscription` | researched at the castle |
| `plate_barding` | a light-cavalry tradition; camels do not carry plate (Scale Barding is kept) |
| `analytical_engine` | replaced by `dune_calculists` |
| `targeting_computers` | brass fire-control engines are machines that think, which the Calculists' edict forbids; `dune_calculists` gives Siege +1 range instead |
| `ferroconcrete`, `force_lattice` | walls of sandstone and glass: no late wall-hp techs, so a deliberate late defensive weakness |

---

## 10. Counters and weaknesses

Lanchester square-law scores at equal cost, computed like section 5.5 of
`game-design.md` (stat only, no range or micro; above 1 means A wins). "(xN)"
means the AoE weapon is counted as reaching N models.

| A | B | A's score | A dps into B | B dps into A |
|---|---|---|---|---|
| `dune_camel_rider` | `scout_rider` | 1.50 | 6.8 | 3.5 |
| `dune_camel_rider` | `archer` | 1.43 | 4.3 | 3.4 |
| `dune_camel_rider` | `horse_archer` | 2.07 | 6.8 | 6.0 |
| `spearman` | `dune_camel_rider` | 2.91 | 9.9 | 3.9 |
| `dune_camel_rider` | `slinger` | 0.91 | 4.3 | 2.2 |
| `dune_sun_guard` | `pikeman` | 1.17 | 6.8 | 4.5 |
| `dune_sun_guard` | `crossbowman` | 1.64 | 7.6 | 6.0 |
| `dune_sun_guard` (x2, cleave) | `militia` | 1.87 | 14.4 | 3.2 |
| `knight` | `dune_sun_guard` | 1.26 | 9.1 | 5.2 |
| `dune_sun_guard` | `man_at_arms` | 0.86 | 5.6 | 6.7 |
| `dune_mirror_engine` | `bombard` | 1.39 | 28.0 | 20.8 |
| `dune_mirror_engine` | `ballista` | 1.99 | 28.0 | 9.4 |
| `aether_lancer` | `dune_mirror_engine` | 1.75 | 17.5 | 14.0 |
| `dune_sand_mage` (x4) | `arquebusier` | 1.27 | 20.2 | 11.8 |
| `dune_sand_mage` (x4) | `halberdier` | 1.00 | 17.9 | 10.3 |
| `dune_sand_mage` (alone) | `halberdier` | 0.25 | 4.5 | 10.3 |
| `javelineer` | `dune_sand_mage` | 4.36 | 10.4 | 5.3 |
| `aether_lancer` | `dune_sand_mage` | 5.73 | 15.7 | 2.8 |
| `dune_veil_guard` | `rifle_squad` | 1.24 | 19.0 | 9.8 |
| `gatling_crew` | `dune_veil_guard` | 0.88 | 24.4 | 17.7 |
| `dune_veil_guard` | `shock_trooper` | 0.89 | 13.7 | 23.1 |
| `mech_walker` | `dune_veil_guard` | 2.02 | 26.1 | 9.5 |
| `steam_tank` | `dune_veil_guard` | 3.09 | 40.8 | 7.4 |
| `dune_ornithopter` (x2, splash) | `rifle_squad` | 1.27 | 23.8 | 12.7 |
| `rifle_squad` | `dune_ornithopter` | 1.58 | 12.7 | 11.9 |
| `flak_wagon` | `dune_ornithopter` | 2.96 | 48.0 | 9.9 |
| `gyrocopter` | `dune_ornithopter` | A cannot be hurt | 11.9 | 0 |
| `dune_sandskiff` | `plasma_artillery` | 2.77 | 24.0 | 16.0 |
| `dune_sandskiff` (x2, pierce) | `shield_trooper` | 2.08 | 60.0 | 29.3 |
| `dune_sandskiff` | `shield_trooper` | 1.04 | 30.0 | 29.3 |
| `hover_tank` | `dune_sandskiff` | 2.48 | 55.2 | 19.5 |
| `storm_walker` | `dune_sandskiff` | 1.91 | 55.3 | 19.5 |
| `gunship` | `dune_sandskiff` | A cannot be hurt | 35.0 | 0 |
| `dune_sand_engine` (x4) | `shield_trooper` | 4.72 | 177.0 | 18.3 |
| `dune_sand_engine` | `shield_trooper` | 1.18 | 44.2 | 18.3 |
| `halberdier` | `dune_sand_engine` | 0.89 | 13.7 | 54.7 |
| `storm_walker` | `dune_sand_engine` | 2.69 | 56.2 | 29.3 |
| `titan` | `dune_sand_engine` | 3.14 | 119.2 | 22.5 |
| `hover_tank` | `dune_sand_engine` | 3.80 | 60.9 | 29.3 |
| `gunship` | `dune_sand_engine` | A cannot be hurt | 21.9 | 0 |

### By age

| Age | Caliphate strengths | Answers to the Caliphate |
|---|---|---|
| Hearth | Camel riders win the scout war and punish lone archers and horse archers. | Spearmen (2.91), slinger bands. Without militia, the early melee line is thin. |
| Banner | Sunspire Guards beat pikes and crossbows and cleave through squads; Star Reckoning speeds up the crater aether. | Knights and men-at-arms out-fight the guards; heavy cavalry has no answer except pikes and camels. With no mangonel or trebuchet, Banner walls fall only to rams that walk right up to them. |
| Starfall | Sunglass Engines and Towers burn siege pushes; sand mages ruin crowded melees; home silicon. | Aether lancers ride down mirrors and mages (1.75, 5.73); javelineers and arquebusiers pick mages off at range. |
| Engine | Veiled Blades beat rifle and gatling lines; ornithopters raid the crater; Calculist Viziers train everything 15% faster. | Shock troopers, landships and mech walkers beat the Blades; EMP (x2 Shielded) and tesla burn veils; flak and fighters own the ornithopters. |
| Ascendant | Sandskiffs kill artillery and crater workers and cross water; the Great Sand-Engine crushes infantry and walls (36 s per stone segment). | **Flyers.** The Caliphate has none. Gunships and starfighters cannot be hit by skiffs or the sand-engine. Hover tanks and storm walkers beat both uniques head-on; railguns and ballistae take x1.5 against the Giant. |

### Siege: seconds for one training to bring down one segment (no techs, shields included)

| attacker | palisade_wall | stone_wall | reinforced_wall | force_wall | stone_gate | town_center |
|---|---|---|---|---|---|---|
| `battering_ram` (common) | 5 | 35 | 100 | 177 | 52 | - |
| `bombard` (common) | 5 | 36 | 103 | 182 | 53 | - |
| `dune_mirror_engine` | 12 | 143 | 333 | 459 | 210 | 138 |
| `dune_ornithopter` (flight of 2) | 6 | 68 | 159 | 219 | 100 | 66 |
| `dune_sandskiff` | 14 | 98 | 278 | 491 | 144 | 160 |
| `dune_sand_engine` | 4 | 36 | 96 | 151 | 53 | 47 |

Mirrors burn timber, houses and siege, not stone. Skiffs are raiders, not breachers
(x0.6 against buildings). The sand-engine's roar is a Nova, so it damages every
segment within 4.5 m at once.

### Matchup notes (provisional; the other factions are being written now)

- **Ironhold:** slow infantry and artillery on rock. Fight them on sand and burn
  their siege; never assault their walls before bombards and the sand-engine.
- **Sylvan:** thickets stop every Caliphate wheel. Keep fights in the open; sand
  mages answer ambushing squads.
- **Tide:** water lanes. Sandskiffs and hover tanks are the only way to follow
  marines across deep water.
- **Steppe:** their horses also run the dunes, so the terrain edge is gone; camel
  riders (x1.75 against Cavalry) and pikes become the plan.
- **Starborn:** the worst late matchup, since Starborn own the sky. Stack shield
  troopers, flak and plasma turrets, and win before the Ascendant Age if possible.

---

## 11. AI notes

`ai` in the RON: aggression **0.6** (mobile, raids early, will not turtle),
eco_focus `food, wood, aether, silicon, iron, copper, adamant, stone`.

| Army weight | Unit | When |
|---|---|---|
| 0.6 | `dune_camel_rider` | Hearth and Banner: anti-scout, anti-horse-archer raids |
| 1.0 | `dune_sun_guard` | Banner and Starfall melee line |
| 0.8 | `arquebusier` | Starfall ranged line |
| 0.3 | `battering_ram` | Banner: the only wall-breaker before the bombard |
| 0.4 | `bombard` | Starfall and Engine: walls and towers from range |
| 0.4 | `dune_mirror_engine` | Starfall and Engine: escorts the bombards and burns enemy siege |
| 0.5 | `dune_sand_mage` | with every melee blob from the Starfall Age, never alone |
| 1.0 | `dune_veil_guard` | Engine melee line |
| 0.7 | `rifle_squad` | Engine ranged line |
| 1.0 | `dune_sandskiff` | Ascendant raider and artillery hunter |
| 0.3 | `dune_sand_engine` | one at a time, for the base push |

Behaviour hints for whoever writes the AI:

1. Build the House of Wisdom early in the Banner Age and research Star Reckoning
   before the first workers walk to the crater.
2. Build Quartz Sifters (2-4) in the Starfall Age ahead of `to_engine`; take the
   mid-ring dunes only when they are safe.
3. Prefer to engage on sand when the enemy army is mostly mounted or wheeled, and
   retreat across dunes.
4. Escort bombards with Sunglass Engines and make enemy siege their first target.
   Put Sunglass Towers next to crater extractors.
5. Walk sand mages in right behind the Sunspire Guards; their Nova reaches only 3 m.
6. Raid with sandskiffs (crater workers, artillery), and pull them back from hover
   tanks and storm walkers.
7. Path the Great Sand-Engine over dry land only (no shallows, marsh or thicket),
   and keep shield troopers with it for anti-air.
8. Against an air-heavy enemy, switch weight to flak wagons, mech walkers, shield
   troopers and flak batteries one age earlier than usual.

---

## 12. Notes for the engine

- **Dunerunner** is granted to Foot, Mounted and Wheeled (`traits` in the RON).
  Tracked and Hover are untouched.
- **Nova targets:** `dune_sandstorm` hits ground units only (`buildings: false`);
  `dune_sonic_roar` hits ground units and buildings. Both assume a Nova hits enemies
  only (neither sets `friendly_fire`).
- **The Great Sand-Engine is Wheeled.** Its hull is 4.5 m wide and 6 m long (inside
  the 5 m giant width limit), it turns at 45°/s, and its pathing must avoid shallows,
  marsh and thickets. Its `look` boxes are a roof bell (top at 7.3 m) and a front maw
  that pokes 1.1 m out of the hull.
- **Fixed areas.** The sandstorm (3 m), roar (4.5 m) and torrent (40° x 6 m) are
  priced as fixed areas. No Dune tech changes them; Sonic Resonators adds damage only.
- **Heights.** Ornithopters fly at 12.5 m like gyrocopters. The Sunglass Tower is 9 m
  tall with its mirror on the roof (9.0-10.8 m), so no Dune box reaches a flyer.
- **Quartz Sifter:** a `resource_field` building like a farm (silicon 800, 2 slots,
  `limit: 4`). It is not a drop site, so its workers walk to a mining camp or town
  centre.
- **Veils:** the Veiled Blades use an ordinary `ShieldDef` with `energy_resist: 0`.
  The "slow blade passes the veil" idea lives in the khanjar's x1.5 against
  `Shielded`. If kinetic-shield semantics are settled later (fast missiles blocked,
  melee passing), the veil is the natural first user, and its numbers would need a
  new pass.
- **The Great Lens** adds a permanent `Trickle`, so it does not depend on the crater.
- **The camel rider trains at the town centre** in the Hearth Age, like the scout
  rider. Its 2.0 m height makes camels read differently from horses.

---

## 13. Validation

```
cargo run -q -p af_data --example check -- --only factions/dune.ron --summary
OK: 9 files: core/ages.ron, core/buildings.ron, core/nodes.ron, core/resources.ron, core/techs.ron, core/terrain.ron, core/units.ron, core/weapons.ron, factions/dune.ron
8 resources, 5 ages, 7 terrain, 11 nodes, 63 weapons, 51 units, 39 buildings, 62 techs, 1 factions
```

Checked by hand as well (the checker does not catch these):

- The age path can be walked: `town_center` (to_banner, to_starfall), `forge` (gate
  of to_starfall), `citadel` (to_engine, to_ascendant), `aether_foundry` (gate of
  to_engine), `laboratory` (gate of to_ascendant) are all listed.
- Whole chains: the town_center/citadel, house/manor, palisade/stone/reinforced/force
  wall and gate, and the five-tower chain are all listed.
- Drop sites for all 8 resources: `town_center`/`citadel` take everything;
  `lumber_camp`, `mill`, `mining_camp` and `aether_extractor` as usual.
- Every unique unit trains at a listed building; every unique tech is researched at
  a listed building.
