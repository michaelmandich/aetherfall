# The Khaganate of the Endless Steppe (`steppe`)

> *Every herder a rider, every rider a bow.*

Data: `assets/data/factions/steppe.ron`. Validate with
`cargo run -q -p af_data --example check -- --only factions/steppe.ron --summary`.
Shared rules and the common pool: `docs/design/game-design.md`.

| | |
|---|---|
| Colour | Khagan crimson `(0.78, 0.13, 0.17)`: war-banner red, kept clear of the greens, blues, sands and violets of the other five |
| Traits | Marshwalker and Dunerunner for Mounted and Wheeled: riders and wagons (siege, flak wagons, the War Ordu) cross marsh and dunes at full speed |
| Gather bonus | food x1.2; wood, stone, copper, iron, silicon, aether and adamant x1.08 |
| Worker | `peasant` (all eight resources), `engineer` from the Engine Age |
| Start | 300 food, 300 wood, 150 stone, 100 copper; 5 peasants, 1 scout rider, a `town_center` |
| Roster | 33 trainable units: 25 from the common pool (23 soldiers + 2 workers) and 8 uniques |
| Excluded | 18 of the 43 common units (42%; 16 of the 41 a faction may take, 39%, since the `shock_trooper` and `titan` benchmarks are in no roster), 15 of 36 common buildings, 9 of 53 common techs |
| Uniques | 8 units, 5 buildings (4 ids plus one upgrade tier), 8 techs |

---

## 1. Identity and the fallen star

The clans of the grass have one god, the Eternal Blue Sky, and one law: the Sky
chooses the Khagan. For years the red star swelled every autumn, and the shamans
said the Sky was drawing a bow. When it fell, they said the arrow had been loosed,
and that whoever pulled the arrowhead out of the earth would rule everything under
the Sky.

The Synod's monks walked to the crater. The clans rode. They arrived a season
later and have contested it ever since. Nothing they build is meant to stay put:
the Khagan's palace is a tent, the forts are rings of wagons, the walls are stakes
hammered in at dawn and abandoned at dusk.

The star changed them unevenly, as it changed everyone:

- **Starfall:** shamans who hear the aether fly hawks of starlight, the old
  falconry turned strange. The old fire-lance, a bamboo tube of powder, is packed
  with aether salt and becomes a cavalry weapon that sprays flame.
- **Engine:** captured foundries turn out rocket arrows that steer themselves,
  iron war-wagons with crank gatlings, and biplanes that drop firebombs. The riders
  stay riders.
- **Ascendant:** star-steeds with aether thrusters in their barding, repulsor bikes
  that skim rivers, gunships built like saddles instead of cathedrals, and finally
  the **War Ordu**: the Khagan's capital on a great aether-driven wagon bed, the
  ger-cart of the old khans grown into a palace that never has to stop moving.

The Khaganate stays "knights who found a spaceship": the jet lancer is still a
lancer on a horse, and the War Ordu is still a wagon with drums and archers on its
galleries.

---

## 2. Playstyle and win conditions

**The Khaganate wins by being where the enemy is not.** It has the fastest army in
every age, the weakest static defence, and thin late siege. It does not out-fight
fortresses; it out-runs them, starves them and kills them when they come out.

- **Hearth: harass.** Outriders are the first horse archers in the game. Two or
  three of them circle enemy berry bushes and woodlines from minute three, shooting
  on the move. Spearmen and archers keep them honest.
- **Banner: raid and claim.** Keshig Guard (cheap fast heavy horse) and horse
  archers hunt workers and siege. Felt Camps puts a mining camp on the crater rim
  in half the time; a Wagon Laager next to it protects the first aether.
- **Starfall: hold the crater.** The Golden Tent goes up instead of a castle. Fire
  lancers burn infantry blobs off the extractors; sky shamans fly spirit hawks and
  keep the riders alive. Kurultai makes every rider 10% cheaper and 20% faster to train.
- **Engine: kite.** Rocket riders shoot guided salvos on the move and punish
  machines and gyrocopters; rifle squads and gatling crews hold the extractors;
  flak wagons cover the sky; biplane bombers firebomb packed rifle squads and the
  buildings behind them. Gun Laagers replace the wagon bows.
- **Ascendant: the horde takes wing.** Skyrider bands cross water and marsh, jet
  lancers run down artillery and shooters, Sky Horde gunships come a little cheaper
  and faster, and the War Ordu rolls with the army as its healing heart, through
  the marsh lanes at full speed.

**Win conditions**, in the order the AI should pursue them:

1. Deny the crater: kill extractors and crater mining camps, hold the vents with
   laagers. Every late-age unit needs aether and adamant.
2. Kill workers. Fire-on-move units make every undefended resource a target.
3. Break the enemy army in the open, away from its towers, using the marsh and
   dune patches that slow every other faction's riders.
4. Only then siege: rams, bombards, rocket batteries, biplane bombers and gunships
   against a base whose economy is already gone. The siege wagons can come through
   a marsh lane that every other faction's wheels must go around.

---

## 3. Economy

- **Gather bonus.** Food x1.2 (herds and hunting), every other resource x1.08:
  "slightly better at everything", as the framework intends for the Steppe.
- **Felt Camps** (Hearth, lumber camp): lumber camps, mining camps, mills, farms
  and houses build twice as fast. Cheap expansion and a fast crater rim camp.
- **Great Herds** (Banner, mill): +0.6 food per second, about one extra worker on
  food. The Steppe have no temple and no Star Tithe, so this is their trickle.
- **Kurultai** (Starfall, Golden Tent): cavalry costs 10% less and trains 20% faster.
  It replaces Conscription, which lives at the castle the Steppe do not build.
- **Less stone.** No stone walls, no towers, no castle: Steppe stone goes into the
  town centre, citadel, manors and the odd laager. Stone workers can move to copper
  and iron sooner than any other faction's.
- **Drop sites:** `town_center`/`citadel` (all), `lumber_camp` (wood), `mill` (food),
  `mining_camp` (stone, copper, iron, silicon, aether, adamant), `aether_extractor`
  (aether; it also trickles 0.6 aether and 0.06 adamant per second, the only
  renewable adamant, which is why win condition 1 is the vents). All eight resources
  are covered, and the peasant gathers all eight.
- **AI eco focus:** food, wood, copper, aether, iron, silicon, adamant, stone.

---

## 4. Terrain traits

Marshwalker and Dunerunner both apply to **Mounted** and **Wheeled**:

| terrain | Mounted (others) | Mounted (Steppe) | Wheeled (others) | Wheeled (Steppe) | Steppe Foot |
|---|---|---|---|---|---|
| grass | x1 | x1 | x1 | x1 | x1 |
| sand (dunes) | x0.7 | **x1** | x0.7 (Sunspire x1) | **x1** | x0.9 |
| marsh | x0.6 | **x1** | impassable | **x1** | x0.6 |
| shallow water | x0.55 | x0.55 | impassable | impassable | x0.55 |
| deep water, thicket, cliff | impassable | impassable | impassable | impassable | thicket x0.5 |

The Steppe's wheels are the rams, mangonels, ballistae, bombards, rocket batteries,
flak wagons and the War Ordu.

How to use it:

- **The crater ring.** The design doc puts marsh and thicket patches outside the
  crater's cliff rim. Steppe riders take the marsh lanes at full speed while the
  enemy's knights and lancers wade at 60%. Flank through the marsh, never the thicket.
- **Silicon dunes.** Silicon sits on sand in the mid ring. Steppe riders raid enemy
  silicon at full speed and escape the same way; enemy cavalry chasing them crawls at 70%.
- **Wagons through the marsh.** Marsh stops every other faction's wheels. Steppe
  siege wagons and flak wagons roll through it at full speed, so a bombard train
  or a rocket battery can come out of a marsh lane nobody walled, and the War Ordu
  keeps pace with its riders through marsh and dunes. Shallow and deep water still
  stop every Steppe wheel.
- **Hover covers water.** The Ascendant skyriders are Hover, so the traits do not
  apply to them, but they cross shallows, marsh and deep water (x0.8).
- Steppe infantry has no trait. It stays home or on the crater rim.

---

## 5. Roster by age

Each age's units are the common pieces the Steppe keep plus their uniques
(**bold**). Squads count once.

### Hearth (6)

| unit | source | role |
|---|---|---|
| `peasant` | common | worker, all eight resources |
| `scout_rider` | common | scout, hunts slingers and archers |
| `spearman` | common | anti-cavalry line; upgrades to pikeman |
| `slinger` | common (squad 3) | cheap anti-infantry, batters palisades |
| `archer` | common | foot archer; stays an archer (no crossbow line) |
| **`steppe_outrider`** | unique | the first horse archer: shoots on the move, harasses workers; becomes a horse archer with Composite Bows |

### Banner (7)

| unit | source | role |
|---|---|---|
| `pikeman` | common | anti-cavalry line; upgrades to halberdier |
| `javelineer` | common | hunts enemy archers and horse archers |
| `horse_archer` | common | main ranged cavalry from here on |
| `battering_ram` | common | wall breaker |
| `mangonel` | common | anti-blob siege |
| `ballista` | common | anti-siege |
| **`steppe_keshig`** | unique | heavy shock cavalry in place of the knight: faster, cheaper, cleaves |

### Starfall (7)

| unit | source | role |
|---|---|---|
| `halberdier` | common | anti-cavalry and anti-machine line |
| `arquebusier` | common | trained directly at the archery range |
| `grenadier` | common (squad 2) | blob and gate breaker; Thunder Powder +15% |
| `aether_lancer` | common | heavy lancer, the Steppe's anti-machine rider into the Engine Age |
| `bombard` | common | the Steppe's main wall breaker for the rest of the game |
| **`steppe_fire_lancer`** | unique | aether fire-lance (Spray cone) that burns clumped infantry while riding through them |
| **`steppe_sky_shaman`** | unique | spirit-hawk support rider; heals one rider at a time |

### Engine (8)

| unit | source | role |
|---|---|---|
| `engineer` | common | late worker |
| `rifle_squad` | common (squad 4) | long-range line infantry, shoots air |
| `gatling_crew` | common | holds the extractors against infantry |
| `rocket_battery` | common | long-range siege; Sky-Fire Rockets +15% |
| `flak_wagon` | common | anti-air escort |
| `gyrocopter` | common (squad 2) | fast air raiders |
| `biplane_bomber` | common | the Steppe's Bomb: 3 m firebombs on packed infantry and buildings; at 8 m/s the fastest Engine flyer |
| **`steppe_rocket_rider`** | unique | guided rocket salvos on the move; hunts machines and flyers |

### Ascendant (5)

| unit | source | role |
|---|---|---|
| `hover_tank` | common | main battle hover tank |
| `gunship` | common | the "gunship swarm"; Sky Horde makes it 5% cheaper, 15% faster to train and 10% faster in the air |
| **`steppe_skyrider`** | unique (squad 3) | repulsor-bike riders: fast, shoot on the move, cross water |
| **`steppe_jet_lancer`** | unique | fastest heavy rider; runs down shooters and artillery |
| **`steppe_war_ordu`** | unique | Giant: the capital on wheels; rolls through marsh and dunes, heals cavalry, galleries shoot ground and air, drums shock what climbs aboard |

Buildings by age: Hearth `town_center`, `house`, `lumber_camp`, `mining_camp`,
`mill`, `farm`, `barracks`, `archery_range`, **`steppe_palisade`**,
**`steppe_palisade_gate`**; Banner `stable`, `siege_workshop`, `forge`,
**`steppe_laager`**; Starfall `citadel`, `manor`, `aether_extractor`,
`aether_foundry`, **`steppe_golden_tent`**; Engine `engine_works`, `laboratory`,
`airfield`, `anti_air_battery`, **`steppe_gun_laager`**; Ascendant `starport`,
`shield_generator`.

The age path is complete: `town_center` (to_banner, to_starfall), `forge` (gate of
Starfall), `citadel` (to_engine, to_ascendant), `aether_foundry` (gate of Engine),
`laboratory` (gate of Ascendant).

---

## 6. Unique units

Stats per model. dps is before armour. Value columns compare with the age medians
in game-design.md section 8 (Hearth 0.057 / 0.73, Banner 0.054 / 0.76, Starfall
0.067 / 0.81, Engine 0.074 / 0.95, Ascendant 0.095 / 1.45 for dps/cost and ehp/cost).

| id | name | age | tags | cost | pop | hp (+shield) | armour % | speed | weapon | dps | range | dps/cost (x med) | ehp/cost (x med) | trained at |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `steppe_outrider` | Outrider | 1 | Cavalry, Ranged, Light | 60F 50W | 2 | 75 | S5 P10 | 6.0 | `steppe_horn_bow` Bolt, fire on move | 4.5 | 7 | 0.041 (0.72) | 0.68 (0.93) | archery_range, stable |
| `steppe_keshig` | Keshig Guard | 2 | Cavalry, Melee, Heavy | 110F 20Cu 50Fe | 2 | 160 | S30 P25 C10 | 6.0 | `steppe_guard_sabre` Swing 70 deg, cleave 1 | 10.0 | 0.9 | 0.056 (1.03) | 0.89 (1.17) | stable, golden tent |
| `steppe_fire_lancer` | Fire Lancer | 3 | Cavalry, Melee, Light | 100F 40W 50Fe 30Ae | 2 | 150 | S20 P20 C10 F25 | 6.2 | `steppe_fire_lance` Spray 35 deg, fire on move | 10.9 per target (14.2 vs infantry) | 2.5 | 0.050 (0.74) per target | 0.68 (0.84) | stable, golden tent |
| `steppe_sky_shaman` | Sky Shaman | 3 | Cavalry, Ranged, Support, Light | 80F 60Cu 40Ae | 2 | 110 | S5 P10 E20 | 5.8 | `steppe_spirit_hawks` Homing, ground and air; heals one Cavalry or Beast 2/s | 5.0 (6.0 vs Infantry) | 9 | support | 0.61 (0.75) | golden tent |
| `steppe_rocket_rider` | Rocket Rider | 4 | Cavalry, Ranged, Light | 120F 40W 60Cu 40Fe 40Si 20Ae | 2 | 240 | S15 P25 C5 F15 | 6.0 | `steppe_rocket_arrows` Homing x2, fire on move | 25.0 | 11 | 0.078 (1.06) | 0.75 (0.79) | stable, golden tent |
| `steppe_skyrider` | Skyrider Band (squad 3) | 5 | Cavalry, Ranged, Light, Shielded | 180F 80Fe 120Si 190Ae 60Ad | 2 each | 200+60 | S15 P20 C10 F15 E15 | 7.5 hover | `steppe_bike_carbine` Bolt, fire on move | 20 (60 squad) | 9 | 0.095 (1.00) | 1.24 (0.85) | starport, golden tent |
| `steppe_jet_lancer` | Jet Lancer | 5 | Cavalry, Melee, Heavy, Shielded | 180F 80Fe 160Ae 60Ad | 2 | 420+120 | S40 P35 C20 F25 E25 | 7.0 | `steppe_star_lance` Thrust, Energy | 40.0 | 1.5 | 0.083 (0.88) | 1.13 (0.78) | stable, golden tent |
| `steppe_war_ordu` | War Ordu | 5 | Mechanical, Giant, Heavy, Hero | 400F 250Fe 250Si 600Ae 400Ad | 10 | 4000 | S55 P55 C35 F35 E25 | 3.6 wheeled (marsh and sand x1) | `steppe_ordu_galleries` Bolt x8 (ground+air), `steppe_ordu_drums` Nova; heals one Cavalry 6/s | 70.5 | 12 / 5 | 0.037 (titan 0.039) | 2.11 (titan 2.31) | golden tent |

Design notes:

- **Outrider.** About the value of the common horse archer (0.044 / 0.73): ranged
  cavalry pays for firing on the move. It out-ranges nothing (7 m against the archer's 8) and
  dies to spearmen and javelins. `steppe_composite_bows` converts it to the horse
  archer: equal pop (2) and a per-model cost ratio of 150 / 110 = 1.36, inside the 1.6 rule.
- **Keshig Guard.** A knight that trades armour and 20 cost for speed and a cleaving
  sabre. Even with men-at-arms and knights, beats crossbows, loses hard to pikes.
- **Fire Lancer.** Below the band against one target on purpose. The Spray cone hits
  every model in front of it, so against squads and clumps it deals two to four
  times that. Short reach, light armour and the cavalry tag make halberds its hard counter.
- **Sky Shaman.** A support rider and the horde's falconer: slow hawks of starlight
  that steer onto one target, rider or flyer (5 dps, 9 m), and 2 hp/s healing on one
  cavalry model at a time. Fragile and aether-hungry. The hawks are deliberately not
  a chain: chain lightning belongs to other factions.
- **Rocket Rider.** At the Engine median for damage and below it for toughness. Homing
  rockets with x1.4 against Mechanical, and they hit flyers. Rifles (12 m) and
  landship cannon (13 m) out-range it until Sky-Fire Rockets.
- **Skyrider Band.** At the Ascendant dps median, below on ehp. Hover lets it cross
  water and marsh. It carries Cavalry and Shielded tags: pikes, halberds and EMP find it.
- **Jet Lancer.** No machine bonus on purpose. It runs down shooters (x1.3 vs Ranged)
  and siege (x1.5), and loses to hover tanks and storm walkers.
- **War Ordu.** A giant priced like the titan per cost but built for a different job:
  faster (3.6 against 2.0), rolling through marsh and dunes at full speed with the
  riders, healing them, and shooting air. It is a wagon-palace, not a ship: Wheeled,
  no shield (hp 4000, the top of the giant band, in place of the shield), and it
  cannot ford water. Its 5.0 x 6.0 m footprint is at the giant width cap and fits
  the 9 m Stake Gate. It loses badly to a titan (0.36) and to storm walkers (railgun
  x1.5 vs Giant). Its drums carry x1.5 against buildings, so it can finish a breached wall.

### Unique weapons

| id | kind | damage | cd s | volley | range | splash | hits | bonus | look / projectile |
|---|---|---|---|---|---|---|---|---|---|
| `steppe_horn_bow` | Bolt 22 | 5 Pierce | 1.1 | 1 | 7 | 0 | GAB | Spear x1.5, Building x0.2 | short horn bow; arrow |
| `steppe_guard_sabre` | Melee Swing 70 deg, cleave 1 | 13 Slash | 1.3 | 1 | 0.9 | 0 | GB | Ranged x1.3, Siege x1.5 | curved sabre |
| `steppe_fire_lance` | Spray 35 deg, 8 particles | 12 Fire | 1.1 | 1 | 2.5 | 0 | GB | Infantry x1.3, Building x1.2 | 2.2 m lance-tube; glowing flame cubes |
| `steppe_spirit_hawks` | Homing 18, 240 deg/s | 10 Energy | 2.0 | 1 | 9 | 0 | GAB | Infantry x1.2, Building x0.2 | hawk on the gauntlet; glowing pale-blue hawk |
| `steppe_rocket_arrows` | Homing 26, 160 deg/s | 20 Fire | 1.6 | 2 | 11 | 0.5 | GAB | Mechanical x1.4, Building x0.5 | saddle launcher; rocket |
| `steppe_bike_carbine` | Bolt 50 | 14 Fire | 0.7 | 1 | 9 | 0 | GAB | Building x0.25 | carbine; glowing sky-fire bolt |
| `steppe_star_lance` | Melee Thrust | 60 Energy | 1.5 | 1 | 1.5 | 0 | GB | Ranged x1.3, Siege x1.5 | 3 m glowing lance |
| `steppe_ordu_galleries` | Bolt 45 | 10 Fire | 1.5 | 8 | 12 | 0 | GAB | Building x0.3 | roof gallery; glowing plasma arrows |
| `steppe_ordu_drums` | Nova | 60 Crush | 3.5 | 1 | 5 | 0 | GB | Infantry x1.2, Cavalry x1.2, Building x1.5 | war drum on the rear deck; shock ring |
| `steppe_tent_archers` | Bolt 26 | 7 Pierce | 2.0 | 4 | 11 | 0 | GAB | Building x0.2 | roof archers (Golden Tent) |
| `steppe_laager_bows` | Bolt 24 | 6 Pierce | 2.0 | 3 | 10 | 0 | GAB | Building x0.2 | wagon archers (Wagon Laager) |
| `steppe_laager_gatlings` | Bolt 60 | 5 Pierce | 1.2 | 6 | 11 | 0 | GAB | Infantry x1.3, Building x0.2 | crank gatling (Gun Laager) |

Weapon kinds the Steppe field: Bolt, Arc (slingers, grenadiers, mangonels),
Cannon (ballista, bombard), Melee Swing and Thrust and Smash (ram), Spray, Homing
(rocket riders, sky shamans, rocket batteries, gunships), Beam (hover tank), Nova
(War Ordu drums, shield generator EMP) and Bomb (biplane bomber). The uncommon ones
(Spray, Nova, and Homing on a rider) are on uniques. Chain is the one kind they do not
field: chain lightning is other factions' signature.

---

## 7. Unique buildings

| id | name | age | size m | cost | hp | armour % | role |
|---|---|---|---|---|---|---|---|
| `steppe_palisade` | Stake Line | 1 | 3 x 2.2 x 1.0 | 6W | 220 | S30 P80 | wall segment; **no upgrade** |
| `steppe_palisade_gate` | Stake Gate | 1 | 9 x 2.4 x 1.2 | 25W | 380 | S30 P80 | gate; no upgrade |
| `steppe_laager` | Wagon Laager | 2 | 6 x 2.4 x 6 | 120W 40S 50Cu, 35 s | 1100 | S50 P80 C5 E20 | ring of war-wagons, 3 archers (9 dps, 10 m, hits air), garrison 6; Tower tag; upgrades to Gun Laager (100Fe 50Si, 40 s) |
| `steppe_gun_laager` | Gun Laager | 4 | 6 x 2.8 x 6 | 120W 60S 50Cu 100Fe 50Si, 50 s | 2200 | S65 P85 C20 F30 E25 | crank gatlings (25 dps, x1.3 vs Infantry, 11 m, hits air); Tower tag |
| `steppe_golden_tent` | Golden Tent | 3 | 11 x 8 x 11 | 350W 100S 150Cu 100Fe 50Ae, 120 s | 3200 | S55 P80 C10 F5 E20 | **limit 1**; +10 pop; roof archers (14 dps); garrison 15; trains Keshig, Fire Lancer, Sky Shaman, Rocket Rider, Skyrider, Jet Lancer, War Ordu; researches Kurultai, Thunder Powder, Sky-Fire Rockets, Eternal Blue Sky |

- **Stake Line instead of stone.** Rule 2 of the framework: listing `palisade_wall`
  would pull in the whole stone-to-force chain, so the Steppe have their own segment
  with no upgrade. It is cheaper and faster than the common palisade (6 wood, 4 s),
  lower and weaker (2.2 m, 220 hp). It funnels and delays; it never holds. It keeps
  the common wall footprint rules (game-design.md section 10, rule 10): segments
  1.0 m thick so the line stays closed on the 1 m nav grid, and a gate 9 m long and
  1.2 m thick, wide enough for the War Ordu and any siege wagon.
- **Wagon Laager instead of towers.** The Steppe have no tower chain. The laager is
  cheaper than a guard tower, goes up in 35 s, and is the thing they drop next to a
  crater mining camp or an extractor. It burns (no fire armour) and its arrows are
  weaker than a guard tower's (9 dps against 12). The Engine-age Gun Laager is the
  Steppe's last static defence. In the Ascendant they rely on the `shield_generator`,
  the `anti_air_battery` and their army.
- **Golden Tent instead of the castle.** Cheaper in stone (100 against 650), faster to
  raise (120 s against 200), far weaker (3200 hp, almost no fire armour), and one per
  realm. It holds the Steppe's elite production and their faction research. Killing it
  is the best single blow against the Khaganate, and the Steppe player must guard it.

---

## 8. Unique techs

| id | name | age | at | cost | time s | effects |
|---|---|---|---|---|---|---|
| `steppe_felt_camps` | Felt Camps | 1 | lumber_camp | 75F 75W | 25 | lumber camp, mining camp, mill, farm, house build time x0.5 |
| `steppe_composite_bows` | Composite Bows | 2 | stable | 150F 100W 50Cu | 40 | `steppe_outrider` -> `horse_archer`; horse archer range +0.5 |
| `steppe_great_herds` | Great Herds | 2 | mill | 200F 150W | 50 | +0.6 food/s |
| `steppe_kurultai` | Kurultai | 3 | steppe_golden_tent | 400F 200Cu 50Ae | 60 | Cavalry train time x0.8; Cavalry cost x0.9 |
| `steppe_thunder_powder` | Thunder Powder | 3 | steppe_golden_tent | 250F 100Cu 100Fe 50Ae | 50 | fire lancer damage x1.2 and range +0.5; grenadier damage x1.15 |
| `steppe_sky_fire_rockets` | Sky-Fire Rockets | 4 | steppe_golden_tent | 300F 150Fe 100Si 50Ae | 55 | rocket rider range +1, damage x1.1; rocket battery damage x1.15 |
| `steppe_sky_horde` | Sky Horde | 5 | starport | 300Si 250Ae 100Ad | 60 | gunship cost x0.95 (550 -> 523), train time x0.85, speed x1.1 |
| `steppe_eternal_blue_sky` | Eternal Blue Sky | 5 | steppe_golden_tent | 500F 300Ae 200Ad | 80 | **signature:** all Cavalry speed x1.1, damage x1.1, shield hp x1.25 |

Common techs the Steppe keep (44): every age-up; all gather, carry and mining techs;
the whole forge line (forging, fletching, scale and plate for infantry and cavalry
through star-forged arms and adamant weave); masonry and architecture; pike_drill and
halberd_drill; husbandry; ballistics; aether_infusion; analytical_engine,
targeting_computers, punch_card_logistics, assembly_line; deflector_shields,
adamant_plating, fusion_cores.

Sky Horde is held to a 5% discount on purpose. Every common counter must still win an
even-cost fight against the Steppe gunship: at x0.95 the starfighter scores 1.01, the
shield trooper 1.02 and the flak wagon 1.13 against it (section 10). At x0.9 the first
two would lose (0.91, 0.92), and at the earlier x0.8 all three would (0.72, 0.73, 0.80).
The tech pays in tempo and speed, not in price.

---

## 9. What is excluded, and why

**Units (18 of 43 common units, 42%).** Two of the 18 are the `shock_trooper` and
`titan` benchmarks that no faction lists, so of the 41 units a faction may take the
Steppe leave out 16 (39%), inside the brief's 25-40% exclusion band. The 23 soldiers and
2 workers kept fall inside the brief's 18-26 band; the 8 uniques come on top.

| excluded | why |
|---|---|
| `militia` | No village levies: every herder rides. Outriders fill the cheap Hearth slot. |
| `man_at_arms`, `shock_trooper`, `shield_trooper` | No heavy foot in any age. The Steppe's heavy troops are on horses. |
| `crossbowman` | Composite bows instead. No crossbow line, so `crossbow_drill` and `hand_cannon_drill` go too; arquebusiers are trained directly and archers stay archers. |
| `knight` | Replaced by the Keshig Guard. |
| `monk` | No temple. Sky shamans ride with the horde and heal riders, not foot. |
| `clockwork_automaton`, `steam_tank`, `mech_walker`, `plasma_artillery`, `storm_walker`, `titan` | **Nothing Tracked.** The Khagan will not wait for anything that crawls. The War Ordu takes the titan's place. |
| `war_balloon` | The one slow flyer (3.5 m/s). Steppe air is fast: gyrocopters and biplane bombers, then gunships. |
| `starfighter`, `drone_swarm` | Air superiority wings and drone swarms are the Synod's sky. The horde takes wing in its own way: gyrocopters, biplanes, then Sky Horde gunships and skyriders. |
| `star_bomber` | A cathedral of a bomber; Sky Horde gunships are built like saddles. Leaving it out also keeps the Steppe's late siege thin, which is their weakness against a turtle. |
| `trebuchet` | Too slow to haul across the grass; bombards and rockets break walls. |

**Buildings (15 of 36 common).** `castle` (Golden Tent instead), `temple` (no
priests, no monks), all four wall tiers and gates (Stake Line instead), and the
whole five-tier tower chain (Wagon Laager and Gun Laager instead).

**Techs (9 of 53 common).** `militia_drill`, `steam_harness` (no militia or
men-at-arms), `crossbow_drill`, `hand_cannon_drill` (no crossbowmen),
`herbal_medicine`, `star_tithe` (no temple), `conscription` (no castle; Kurultai
instead), `ferroconcrete`, `force_lattice` (they never build in stone; stake lines
and laagers stay fragile all game).

**How the combos change over time** compared with the other factions:

| age | the Steppe have early or extra | the Steppe lack |
|---|---|---|
| Hearth | horse archers (outriders) | militia, any tower |
| Banner | Keshig (fast heavy horse), laagers anywhere, siege wagons that cross marsh | knights, men-at-arms, crossbows, monks, stone walls, guard towers |
| Starfall | fire lancers, sky shamans, the Golden Tent | castle, automatons, balloons, trebuchets, bombard towers |
| Engine | rocket riders, gun laagers, biplane bombers | landships, mech walkers, shock troopers, reinforced walls, gun turrets |
| Ascendant | skyriders, jet lancers, the War Ordu, Sky Horde gunships | titans, storm walkers, plasma artillery, shield troopers, starfighters, drone swarms, star bombers, force walls, plasma turrets |

---

## 10. Counters and weaknesses

Equal-cost brawl scores (Lanchester square law, range and kiting ignored, as in
game-design.md 5.5; above 1 means A wins). Kiting units do better in play than here.

| A | B | A's score | A dps into B | B dps into A |
|---|---|---|---|---|
| `steppe_outrider` | `archer` | 0.94 | 4.1 | 3.4 |
| `steppe_outrider` | `spearman` | 0.42 | 6.8 | 9.9 |
| `scout_rider` | `steppe_outrider` | 2.68 | 5.5 | 3.9 |
| `javelineer` | `steppe_outrider` | 3.58 | 10.4 | 3.2 |
| `steppe_keshig` | `crossbowman` | 1.91 | 12.3 | 6.6 |
| `steppe_keshig` | `knight` | 0.89 | 6.5 | 8.0 |
| `steppe_keshig` | `man_at_arms` | 0.97 | 7.0 | 5.8 |
| `steppe_keshig` | `pikeman` | 0.44 | 8.5 | 12.0 |
| `steppe_fire_lancer` | `arquebusier` | 1.02 (single target) | 14.2 | 10.5 |
| `halberdier` | `steppe_fire_lancer` | 2.77 | 22.9 | 14.2 |
| `steppe_rocket_rider` | `gyrocopter` | 1.57 | 35.0 | 21.0 |
| `steppe_rocket_rider` | `steam_tank` | 1.09 | 24.5 | 23.8 |
| `steppe_rocket_rider` | `mech_walker` | 0.75 | 24.5 | 26.1 |
| `rifle_squad` | `steppe_rocket_rider` | 1.33 | 45.0 | 22.5 |
| `steppe_skyrider` | `drone_swarm` | 1.27 | 60.0 | 31.9 |
| `steppe_skyrider` | `gunship` | 1.05 | 42.0 | 29.8 |
| `steppe_skyrider` | `shield_trooper` | 0.92 | 45.0 | 31.2 |
| `hover_tank` | `steppe_skyrider` | 1.31 | 36.8 | 39.0 |
| `steppe_jet_lancer` | `shield_trooper` | 1.08 | 39.0 | 27.5 |
| `steppe_jet_lancer` | `storm_walker` | 0.86 | 32.0 | 41.7 |
| `steppe_jet_lancer` | `hover_tank` | 0.74 | 28.0 | 32.5 |
| `halberdier` | `steppe_jet_lancer` | 0.98 | 17.1 | 40.0 |
| `steppe_war_ordu` | `shield_trooper` | 0.86 | 56.5 | 23.8 |
| `steppe_war_ordu` | `titan` | 0.36 | 35.2 | 120.8 |
| `storm_walker` | `steppe_war_ordu` | 2.54 | 61.5 | 45.8 |
| `starfighter` | `gunship` with Sky Horde (523) | 1.01 | 27.4 | 65.6 |
| `shield_trooper` | `gunship` with Sky Horde (523) | 1.02 | 25.7 | 26.2 |
| `flak_wagon` | `gunship` with Sky Horde (523) | 1.13 | 50.4 | 39.4 |

**What beats the Khaganate**

- **Polearm walls.** Almost the whole army carries the Cavalry tag, so pikes (x3),
  halberds (x2.5) and spears (x2.75) are the answer in every age. The Steppe must
  shoot pike blocks from range (archers, slingers, arquebusiers, rifles, mangonels)
  before the riders go in.
- **Javelineers** against outriders and horse archers.
- **Rifle squads and landship guns** out-range rocket riders until Sky-Fire Rockets.
- **Hover tanks and storm walkers** beat jet lancers. **EMP** (shield generator,
  x2 vs Shielded) eats skyriders and jet lancer shields, and its x2.5 against
  Mechanical still finds the War Ordu.
- **Starfighters, shield troopers and flak** still beat Sky Horde gunships at equal
  cost (1.01, 1.02, 1.13): the tech buys tempo, not a new counter triangle.
- **Water lines.** Every Steppe wheel, the War Ordu included, stops at shallow water.
  A base behind a river or lake keeps out their siege trains and the Ordu; only
  skyriders, riders wading the shallows and the air get across.
- **Turtling with siege.** The Steppe have no stone walls and no tower chain, and
  their late siege is thin (rams, bombards, rocket batteries, biplane bombers,
  gunships; no trebuchet, landship, plasma artillery or star bomber). A force wall
  takes a bombard about three minutes per segment. Against a turtle they must win by
  starving it of the crater, not by breaking in.
- **A determined push into their base.** Stake lines fall in seconds and laagers burn.
  The Steppe answer is to counter-raid, not to hold.
- **Killing the Golden Tent.** One per realm, weak to fire: it halts shamans, the
  War Ordu and the faction research until it is rebuilt.
- **No heavy infantry.** Nothing in the Steppe army holds a choke point against
  heavy foot except halberdiers and, later, gatling crews and rifle squads.

**What the Khaganate beats**

- Anything slow in the open: siege, artillery and shooters caught away from their walls.
- Undefended economies: fire-on-move units make every worker a target.
- Crater control: the fastest rim camp (Felt Camps), laagers in 35 s, and full speed
  through the marsh lanes around the crater.
- Defences that lean on marsh: Steppe rams, bombards, rocket batteries and the War
  Ordu roll through marsh that stops every other faction's wheels.

---

## 11. AI notes

- **Profile:** aggression 0.8 (raider). Army weights: horse archer 1.2, outrider
  1.0, rocket rider 1.0, skyrider 1.0, keshig 0.8, jet lancer 0.8, fire lancer 0.7,
  gunship 0.6, rocket battery 0.4, biplane bomber 0.4, battering ram 0.3, bombard 0.3.
  The siege weights are low on purpose: enough to finish a walled base, not to lead
  with siege.
- **Opening:** 5 peasants on berries and wood, Felt Camps early, two outriders from
  the archery range by minute three to harass the nearest enemy's food. Spearmen
  only if enemy cavalry shows.
- **Banner:** stable, Composite Bows, horse archers and keshig. Mining camp plus a
  Wagon Laager on the crater rim; gather the 100 aether for Starfall early.
- **Starfall:** Golden Tent before anything else military, then Kurultai. Extractors
  on vents, each with a laager. Fire lancers against infantry blobs, two shamans
  per ten riders.
- **Engine:** rocket riders and horse archers as the mobile army; rifle squads,
  gatling crews and flak wagons on the extractors; upgrade laagers to Gun Laagers.
  Biplane bombers on packed infantry and on walls the bombards and rockets have
  opened; route siege wagons through marsh lanes when the path allows.
- **Ascendant:** skyriders and jet lancers, Sky Horde then gunships, one War Ordu
  kept inside the cavalry blob. Pull damaged riders back to the Ordu or a shaman
  instead of retreating home. Path the Ordu around water; it cannot ford it.
- **Targeting:** prefer workers, extractors, crater mining camps and siege. Avoid
  engaging pike or halberd blocks in melee; shoot them first. Never fight under
  enemy towers; pull the enemy out onto marsh and dunes, where its riders slow and
  the Steppe's do not.
- **Defence:** do not wall in. Stake lines only to funnel raiders into the town
  centre's arrows; a shield generator and flak battery in the Ascendant.

---

## 12. Notes for the engine

- `steppe_composite_bows` uses `UpgradeUnit` from `steppe_outrider` (trained at
  archery range and stable) to `horse_archer` (trained at the stable). Under section
  11's rule, the archery range then offers horse archers. That is intended; if
  the engine prefers to keep units at their own `trained_at` buildings, dropping the
  outrider button from the archery range is fine too.
- `steppe_golden_tent` has `limit: 1`.
- `steppe_sky_shaman` and `steppe_war_ordu` both heal and fight, one target at a
  time under the section 11 healer rule. The shaman heals Cavalry and Beast, the Ordu
  Cavalry only.
- `steppe_spirit_hawks` is a slow Homing projectile (18 m/s, 240 deg/s) that may
  target flyers; the hawk box on the shaman's gauntlet is its look.
- `steppe_fire_lance` is a Spray with `fire_on_move: true`: the rider is meant to
  ride through a blob while spraying.
- The laagers are wide and low (6 x 6 m) and carry the Tower tag, so tower techs
  (fletching, rifled barrels, ballistics, targeting computers) apply to them. Their
  weapon looks sit on the wagon roofs.
- Both Steppe traits apply to Mounted **and Wheeled**, so the common siege wagons,
  flak wagons and the War Ordu enter marsh (via Marshwalker) and cross sand at x1.
  Wheeled still cannot enter shallow or deep water.
- The War Ordu is Wheeled at 5.0 x 5.0 x 6.0 m: at the 5.0 m giant width cap (k = 2,
  a clear 5 x 5 block of cells) and 6 m long. Its gallery and drum looks sit on the
  5 m roof. The 9 m Stake Gate lets it through at any angle.
- Stake Line segments are 3.0 x 1.0 m and the Stake Gate 9.0 x 1.2 m, matching the
  common wall and gate footprints so the line stays closed on the nav grid.
- Skyriders are Hover. The Steppe traits do not apply to them, and they do not need
  them.
