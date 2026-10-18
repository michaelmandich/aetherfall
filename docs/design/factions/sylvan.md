# The Sylvan Wardens (`sylvan`)

*Forest kingdoms that grow their armies.*

Data: `assets/data/factions/sylvan.ron`. Framework: `docs/design/game-design.md`.
Validate with
`cargo run -q -p af_data --example check -- --only factions/sylvan.ron --summary`.

| Item | Value |
|---|---|
| Colour | `(0.2, 0.82, 0.32)`, a bright warden green. It is lighter and more saturated than grassland `(0.36, 0.55, 0.27)` and thicket `(0.2, 0.36, 0.16)`, so it stands out on the map, and it is far enough from blue-green that it will not be confused with a sea faction's teal. |
| Trait | Forestwalker for Foot and Mounted: crosses bramble thickets at x1.1 |
| Gather bonus | wood x1.25, food x1.15 |
| Worker | `peasant`, used all game (no engineer) |
| Start | 300 food, 300 wood, 150 stone, 100 copper; 5 peasants, 1 scout rider; a `town_center` |
| Roster | 28 units: the peasant, 16 common combat units and 11 uniques |
| Buildings | 26: 22 common and 4 unique |
| Techs | 49: 40 common and 9 unique, including their own Engine age-up |

---

## 1. Identity and the fallen star

The Wardens are the clans of the deep forest: druids who keep the standing stones,
rangers who never miss, and the beast-tamers of the hound kennels and bear pits.
They are Celtic in spirit, hold land by oath to the old trees, and have never built
a stone keep.

When the red star swelled in the Banner Age, the druids read it as a seed rather than
a sign. When it fell, they did not carry its crystals to a furnace. They planted them
at the roots of their elder oaks. The forest drank the light and woke up:

- **Starfall.** The first Heartwood Groves rise. Druids call down forked starlight,
  and war bears grow bark over their hides.
- **Engine.** Other realms light boilers. The Wardens instead "Wake the Wildwood":
  thornbacks and root-skulled rootbreakers walk out of the brambles, and rangers grow
  crystal staves that throw starlight in beams.
- **Ascendant.** Wisps born in the crystal canopy fly over the walls, white stags grow
  antlers of living crystal, and the eldest oaks get up and walk to war as Elder Treants.

The faction plays "medieval, then strange" the green way: their uniques are grown,
tamed or carved, never forged. Their late game is crystal and living wood,
fighting beside the rifle and the shield trooper they do borrow from the common pool.

---

## 2. Playstyle and win conditions

**Fast, slippery, regrowing.** The Wardens have the best wood and forage economy, the
cheapest early army (bows and beasts cost food and wood, not iron) and the map's
thickets as private roads. They are poor at holding a fixed line and poor against fire.

| Phase | Plan |
|---|---|
| Hearth | Boom on wood and berries. War hounds run through the brambles to kill enemy workers and archers. Thornwalls go up around the woodlines for almost nothing. |
| Banner | Longbowmen behind pikes out-range every Banner hand missile (11 m against crossbows' 9 and javelins' 8), and elk riders sweep the flanks. Ironbark Hedges turn the thornwalls into a real, regrowing barrier. Send workers to the crater early: the food bonus pays for the 800-food Starfall advance. |
| Starfall | Build a Heartwood Grove at once. It gates the Engine Age and trains Star Druids. War bears are fast cleaving bruisers for raiding the crater's extractors; druids heal them. |
| Engine | Crystal Rangers are the answer to landships and walkers, and to flyers. Thornbacks hold choke points against infantry, and rootbreakers break the reinforced walls that timber siege cannot. Crystal Song makes peasants viable on the crater. |
| Ascendant | Wisp swarms harass over walls and seed-bomb workers and blobs. Starstag riders hunt artillery and siege at the gallop. Elder Treants are the siege and the tank; shield troopers and Crystal Rangers cover them against fire and machines. |

**Win conditions**

1. **Crater raids.** Win the centre through mobility rather than by holding it. The
   crater's outer marsh and thicket patches (game-design 4.2) let Wardens infantry,
   beasts and elk reach the extractors from directions others cannot. Kill
   extractors and mining camps, fall back into the brambles, and repeat.
2. **Mid-game timing.** Banner and Starfall are the Wardens' strongest ages per
   resource (longbows, elk, bears, druids). Push before the enemy's Engine Age.
3. **Living siege.** If the game goes long, rootbreakers (Engine) and then treants
   (Ascendant), escorted by Crystal Rangers, break fortresses that the Wardens' timber
   siege cannot.

**They lose when** the enemy turtles behind stone and fire until the Engine Age,
then rolls landships and flamers through regrowing hedges and burning beasts.

---

## 3. Economy

| Lever | Effect |
|---|---|
| `gather_bonus` | wood x1.25, food x1.15 (berries, farms, and game brought to the Beast Lodge) |
| Beast Lodge | also a **food drop site**, so hunters and foragers at far berry patches drop there |
| Peasants all game | no engineer, so the late crater economy is weaker until Crystal Song |
| `sylvan_crystal_song` (Engine, Grove) | aether x1.25, adamant x1.3, carry +2 |
| Thornwalls | 10 wood a segment and no stone at all; Ironbark is wood and copper |
| Stone needs | town centre/citadel (400 for the citadel), towers, temple, aether extractors and the Grove only, so stone matters less than for other factions, but it is never zero |

Peasant rates with common and Sylvan techs, against an engineer with the common techs:

| resource | Sylvan peasant, all techs | engineer, common techs | note |
|---|---|---|---|
| wood | 0.75 x1.25 x1.15 x1.15 x1.2 = 1.49/s | 0.75 x1.15 x1.15 x1.2 = 1.19/s | the Wardens' speciality |
| food | 0.7 x1.15 x1.1 x1.15 x1.15 x1.2 = 1.41/s | 0.7 x1.1 x1.15 x1.15 x1.2 = 1.22/s | |
| aether | 0.3 x1.25 x1.1 x1.25 = 0.52/s | 0.4 x1.25 x1.1 = 0.55/s | about 6% short |
| adamant | 0.18 x1.3 x1.3 = 0.30/s | 0.26 x1.3 = 0.34/s | about 10% short |
| carry | 10 +3 +4 +2 = 19 | 14 +3 +4 = 21 | |

Workers drop at `town_center`/`citadel` (everything), `lumber_camp` (wood), `mill` and
`sylvan_beast_lodge` (food), `mining_camp` (stone, ores, silicon, aether, adamant) and
`aether_extractor` (aether). Every one of the eight resources has a drop site.

---

## 4. Terrain: Forestwalker

The trait applies to every Foot and Mounted unit: infantry, peasants, hounds, war
bears, thornbacks, rootbreakers, treants (all Foot) and elk riders, starstag riders and
scouts (Mounted). In thickets,
other Foot units move at x0.5 and nothing else can enter; the Wardens move at **x1.1**.

- **Ambush lanes.** Thickets sit inside forests and outside the crater rim. A Wardens
  army can wait inside one, untouchable by cavalry and machines, then strike out.
- **Raids.** Hounds and elk use bramble patches as shortcuts that horse, wheels and
  tracks cannot follow. A thicket between two bases is a Sylvan road.
- **Escape.** Retreat into a thicket and pursuing cavalry stops dead while infantry
  crawls. Thornbacks and treants can stand inside the brambles they came from.
- **Limits.** No help on cliffs, deep water or marsh (marsh at x0.6 as normal). Wisps
  fly. Thickets are unbuildable, so they cannot be walled.

---

## 5. Roster per age

**C** = common pool, **U** = unique. The checker's summary lists the same ages.

| Age | Unit | C/U | Role |
|---|---|---|---|
| Hearth | `peasant` | C | worker for the whole game |
| Hearth | `scout_rider` | C | scout and raider (trained at the town centre and citadel; the one horse the Wardens keep) |
| Hearth | `militia` | C | cheap melee squad of woodsmen |
| Hearth | `spearman` | C | anti-cavalry, anti-beast |
| Hearth | `slinger` | C | squad skirmisher, palisade-breaker |
| Hearth | `archer` | C | early bow; stays trainable beside the longbowman as the bow that shoots flyers |
| Hearth | `sylvan_hound_pack` | U | **War Hounds**: 3-hound squad, fast, kills archers and workers |
| Banner | `pikeman` | C | anti-cavalry line (Pike Drill converts spearmen) |
| Banner | `javelineer` | C | anti-missile skirmisher; shoots flyers |
| Banner | `monk` | C | healer |
| Banner | `battering_ram` | C | wall-breaker (a felled oak) |
| Banner | `mangonel` | C | anti-blob siege |
| Banner | `ballista` | C | anti-siege |
| Banner | `sylvan_longbowman` | U | **Longbowman**: arcing 11 m volleys; ground only |
| Banner | `sylvan_elk_rider` | U | **Elk Rider**: cleaving shock cavalry, no iron |
| Starfall | `halberdier` | C | anti-cavalry, anti-beast, anti-clockwork |
| Starfall | `grenadier` | C | aether grenade pair: anti-blob, anti-gate |
| Starfall | `trebuchet` | C | long-range timber siege |
| Starfall | `sylvan_druid` | U | **Star Druid**: healer with chain starlight |
| Starfall | `sylvan_great_bear` | U | **War Bear**: fast cleaving bruiser |
| Engine | `rifle_squad` | C | the one firearm: rangers take up the long rifle |
| Engine | `sylvan_thornback` | U | **Thornback**: living construct, spray of thorns against infantry |
| Engine | `sylvan_lightwarden` | U | **Crystal Ranger**: crystal beam ranger; anti-machine and anti-air |
| Engine | `sylvan_rootbreaker` | U | **Rootbreaker**: root-skulled boar beast; the Engine wall-breaker |
| Ascendant | `shield_trooper` | C | shielded plasma line infantry |
| Ascendant | `sylvan_starstag_rider` | U | **Starstag Rider**: crystal-antlered beam cavalry that fires on the move; hunts artillery |
| Ascendant | `sylvan_wisp_swarm` | U | **Wisp Swarm**: 6 flying wisps with homing motes and seedburst bombs |
| Ascendant | `sylvan_elder_treant` | U | **Elder Treant**: giant siege-tank |

Trainable units per age (worker excluded): Hearth 6, Banner 8, Starfall 5, Engine 4,
Ascendant 4; 27 in all, plus the peasant. Every age has at least two, and every age
has a unique. The roster stays smaller than the other factions' (34-35) because the
Wardens take few common machines; 11 uniques fill the gap.

### 5.1 Unique units

Value bands use game-design section 8: dps = damage x volley / cooldown, ehp = hp
(no shields here), per total training cost, squads counted whole. Medians are the
common pool's line units.

| id | age | box m | squad | pop/model | cost | hp | armour % | speed | weapon (kind) | dps/model | range | dps/cost (x median) | ehp/cost (x median) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `sylvan_hound_pack` | 1 | 0.5 x 0.7 x 1.0 | 3 | 1 | 100F 20W | 30 | S5 P5 | 4.4 | `sylvan_hound_bite` (Melee Thrust) | 2.9 | 0.5 | 0.073 (1.28x) | 0.75 (1.03x) |
| `sylvan_longbowman` | 2 | 0.7 x 1.45 x 0.7 | 1 | 1 | 40F 70W | 65 | S5 P10 | 2.9 | `sylvan_yew_longbow` (Arc, volley 2) | 6.8 | 11 | 0.062 (1.15x) | 0.59 (0.78x) |
| `sylvan_elk_rider` | 2 | 0.9 x 2.0 x 1.9 | 1 | 2 | 100F 50W 30Cu | 160 | S20 P20 C5 | 5.8 | `sylvan_antler_sweep` (Melee Swing, cleave 1) | 8.8 | 1.0 | 0.049 (0.91x) | 0.89 (1.17x) |
| `sylvan_druid` | 3 | 0.65 x 1.5 x 0.65 | 1 | 1 | 70F 30W 40Cu 30Ae | 110 | P10 E20 | 2.6 | `sylvan_starcall` (Chain, 3 jumps); heals 4/s at 7 m | 5.0 | 8 | 0.029 (support) | 0.65 (0.80x) |
| `sylvan_great_bear` | 3 | 1.4 x 1.7 x 2.3 | 1 | 3 | 150F 50W 40Cu 30Ae | 320 | S30 P35 C10 E5 | 3.6 | `sylvan_bear_claws` (Melee Swing, cleave 2) | 13.8 | 0.9 | 0.051 (0.76x) | 1.19 (1.46x) |
| `sylvan_thornback` | 4 | 2.2 x 2.2 x 3.0 | 1 | 4 | 80F 120W 150Fe 100Si 40Ae | 650, regen 2 | S45 P55 C20 E15 | 2.8 | `sylvan_thorn_spray` (Spray 50 deg) | 12.2 per target | 6 | 0.025 single target (0.34x) | 1.33 (1.40x) |
| `sylvan_lightwarden` (Crystal Ranger) | 4 | 0.7 x 1.5 x 0.7 | 1 | 1 | 75F 40W 65Fe 50Si 20Ae | 200 | S10 P20 F5 E20 | 2.9 | `sylvan_crystal_lens` (Crystal Stave, Beam) | 18.8 | 12 | 0.075 (1.01x) | 0.80 (0.84x) |
| `sylvan_rootbreaker` | 4 | 2.4 x 2.0 x 3.2 | 1 | 4 | 140F 120W 80Fe 100Si 80Ae | 700 | S35 P45 C15 E10 | 2.4 | `sylvan_root_ram` (Melee Smash, splash 0.5) | 20 (60 vs buildings) | 0.8 | 0.038 (siege) | 1.35 (1.42x) |
| `sylvan_starstag_rider` | 5 | 1.0 x 2.2 x 2.0 | 1 | 2 | 140F 100W 130Ae 50Ad | 420 | S25 P30 C10 E25 | 6.0 | `sylvan_antler_ray` (Beam, fires on the move) | 37.1 | 11 | 0.088 (0.93x) | 1.00 (0.69x) |
| `sylvan_wisp_swarm` | 5 | 0.45 cube, alt 12 | 6 | 1 | 80F 60Si 180Ae 30Ad | 75 | E20 | 8.5 (Flying) | `sylvan_wisp_mote` (Homing) + `sylvan_seedburst` (Bomb, splash 1.2) | 6.6 (5.0 + 1.6) | 7 / 1 | 0.113 (1.19x) | 1.29 (0.89x) |
| `sylvan_elder_treant` | 5 | 3.6 x 9.5 x 3.6 | 1 | 10 | 300W 100Fe 200Si 550Ae 350Ad | 3600, regen 5 | S50 P60 C25 F10 E30 | 2.2 | `sylvan_treant_fists` (Smash, splash 1.5) + `sylvan_rootquake` (Nova 5 m) | 46.5 | 1.5 / 5 | 0.031 (giant) | 2.40 (giant) |

Requirements from the brief, and where each is met:

| Requirement | Met by |
|---|---|
| A squad | `sylvan_hound_pack` (3) and `sylvan_wisp_swarm` (6) |
| A big unit | `sylvan_elder_treant` (Giant, pop 10, hp 3600). The thornback and rootbreaker are large but deliberately not Giant (rule 6). |
| A readable silhouette | The treant is a tall, narrow trunk (3.6 x 9.5 x 3.6), so it does not read as the Starborn archon's broad, squat block. |
| Weapon kinds and styles uncommon in the pool | Arc longbow (Arc is otherwise only slings and siege); Chain (otherwise only the storm walker's tesla coil); Spray (otherwise only the landship's flamer); Homing on a living flyer (otherwise rockets and missile pods); Bomb from a living flyer (`sylvan_seedburst`; otherwise balloons, biplanes and star bombers); Nova on a unit (otherwise only the titan's stomp); a Beam that fires from the saddle (`sylvan_antler_ray`) |
| Uniques in four or more ages | All five: Hearth 1, Banner 2, Starfall 2, Engine 3, Ascendant 3 |

**Unit notes**

- **War Hounds.** Bites ignore buildings, which keeps them off walls. They get x1.5
  against Ranged and x1.25 against Worker. As Beasts they take x2 from spears, pikes
  and halberds. They are Hearth harassment that falls off once Banner polearms arrive.
- **Longbowman.** The arc means volleys land where the target is predicted to be, so
  slow blobs get hit and fast riders dodge. The arc cannot hit flyers (game-design
  5.3). Two arrows a shot, splash 0.35, x1.5 against Spear. It is trained, never
  converted: archers stay in the archery range menu as the Wardens' bow that shoots
  flyers. Seasoned Yew (`sylvan_yew_longbows`, Banner) adds +1 range and +1 vision
  (12 m, 12.5 with Fletching). Starshot Arrows (Starfall) adds +35% damage and +40%
  hp, which keeps it at Starfall value: 9.2 dps and 91 hp for 110.
- **Elk Rider.** Antlers sweep in a 70 degree arc and cleave one extra target. Costs
  no iron, and is lighter-armoured than a knight (S20 P20 against S35 P30). Tagged
  Cavalry only, not Beast as well, so polearm bonuses do not double.
- **Star Druid.** Heals Infantry, Cavalry and Beast at 4 hp/s (a monk heals 3).
  Starcall is Energy damage that jumps to three more targets within 3.5 m, x1.3
  against Mechanical. Fragile and slow.
- **War Bear.** Cleaves two extra targets in a 100 degree arc, and moves at 3.6 m/s,
  fast for a heavy. Its hp per cost is high (1.46x the median) and paid for by below-
  median dps, the Beast tag (x2 from polearms), Heavy (crossbows, arquebuses and
  rifles bonus) and no fire armour.
- **Thornback.** Sprays thorns in a 50 degree cone while moving, x1.4 against
  Infantry, so a rifle squad in the cone takes four hits. Single-target dps is very
  low and its 6 m range is outranged by rifles (12) and landships (13). Fire armour 0.
- **Crystal Ranger** (`sylvan_lightwarden`). A ranger with a living crystal grown on a
  yew stave (the Crystal Stave, `sylvan_crystal_lens`). The Wardens' anti-armour and
  anti-air: 30 Energy, x1.75 against Mechanical, 12 m, shoots air. The only common
  answers to landships and walkers the Wardens have are halberds and rifles, and both
  trade badly, so this unit carries the role (an even trade with a steam landship,
  below). Its crystal is grown, not ground: the lens is the Starborn's signature.
- **Rootbreaker.** The Engine-age wall-breaker the timber siege line lacks. A boar
  the star swelled to wagon size, with a ram of living root over its skull: 60 Crush
  every 3 s, x3 against Building, 0.5 m splash. One rootbreaker fells a stone wall
  segment in about 29 s, a reinforced wall in 83 s, a force wall in 147 s and a stone
  gate in 43 s (a battering ram takes 35, 100, 177 and 52). Against troops it does
  only 20 dps. Beast, so polearms take x2 (halberdiers win 1.70 at equal cost), and
  fire armour is 0.
- **Starstag Rider.** A white stag with antlers of living crystal, ridden by a
  warden. The Antler Ray is a 52 Energy beam every 1.4 s at 11 m that fires on the
  move and hits air, x1.5 against Siege, so it runs down plasma artillery, rocket
  batteries and trebuchets (3.53 against plasma artillery at equal cost). No iron in
  its kit. Below the Ascendant median on both axes and frail for its pop: shield
  troopers (1.70), hover tanks (1.43) and halberdiers that catch it (1.29) win a
  stand-up fight, so it must keep moving.
- **Wisp Swarm.** Six wisps at 12 m altitude, the drone layer above every box,
  firing slow homing motes (6 Energy, 16 m/s, 300 deg/s turn) at ground and air. As
  they pass over a target they drop a Seedburst (Bomb, 16 Pierce, 1.2 m splash,
  every 10 s, x1.2 against Infantry and x0.5 against Building): the Wardens' one
  bomb, for workers, blobs and archers rather than walls. Tagged Flying, Beast,
  Ranged and Light: living creatures, so Mechanical bonuses skip them, but flak (x2
  Flying), EMP (x1.5 Flying), tesla jumps and splash shred the squad. Javelins (x2.5
  Ranged) also hurt them.
- **Elder Treant.** A tall, narrow trunk of a box (3.6 x 9.5 x 3.6), so its
  silhouette is a walking tree rather than a block. Oaken Fists are x3 against
  Building (a stone wall segment in about 16 s, much like the titan) with a 1.5 m
  splash. Rootquake is a 5 m Nova every 5 s. Regen 5 hp/s. Its weaknesses: Beast and
  Giant together take x3 from pikes and halberds, fire armour is only 0.1, it cannot
  hit air, and at 2.2 m/s it is easily kited. It costs about 60% of a titan. With
  Barkskin and The Wildwood Ascends it reaches 4554 hp, 7.5 hp/s regen and 1380 cost
  (3.30 hp per cost); a titan with the common Mechanical techs reaches 7920 hp for
  2210 (3.58).

---

## 6. Buildings

### 6.1 Unique buildings

| id | age | box m | cost | hp | role |
|---|---|---|---|---|---|
| `sylvan_beast_lodge` | Hearth | 9 x 4.5 x 8 | 175W | 1500 | Replaces the stable. Trains war hounds, elk riders, war bears and starstag riders; **food drop site**; researches Pack Tactics |
| `sylvan_heart_grove` | Starfall | 10 x 8 x 10 | 250W 100S 50Cu 50Ae | 2200, regen 1.5 | Replaces the aether foundry. Trains druids, thornbacks, rootbreakers, starstag riders, wisp swarms, Elder Treants. Gates `sylvan_wake_the_wildwood` (the Engine Age). Researches Barkskin, Crystal Song, Crystal Bramble, The Wildwood Ascends |
| `sylvan_thornwall` | Hearth | 3 x 2.8 x 1.0 segment | 10W | 350, regen 1 | Living wall. Replaces the whole palisade-to-force-wall chain |
| `sylvan_thorn_gate` | Hearth | 9 x 3 x 1.2 | 35W | 550, regen 1.5 | Gate in the hedge; 9 m long like every common gate, so treants fit through |

Starstag riders train at both the Lodge and the Grove: they are elk-line cavalry, and
the Grove already trains five other uniques.

### 6.2 Walls: living hedges (the deliberate alternative)

The Wardens never build stone walls. A thornwall has **no upgrade chain**. Instead,
two techs strengthen every segment in place, and the hedge regrows constantly:

| Level | Tech (where) | hp | armour % | regen | Compare (common chain) |
|---|---|---|---|---|---|
| Hearth | - | 350 | S35 P80 E10 | 1/s | palisade 250 |
| Banner | `sylvan_ironbark` (lumber camp) | 1050 | S60 P80 C10 E10 | 3/s | stone wall 1500 |
| Engine | `sylvan_crystal_bramble` (Grove) | 2310 | S60 P80 C10 F30 E30 | 8/s | reinforced 3500 |
| Ascendant | + common `ferroconcrete`, `force_lattice` | 3190 | as above | 10/s | force wall 4800 + 500 shield |

Gates follow the same multipliers (550, then 1650, then 3630). Masonry and
Architecture also apply. The hedges are about two thirds of the common walls' hp but
cost no stone and regrow, so small raids and stray fire never wear them down, while
real siege still goes through. The segment is 1.0 m deep and the gate 9.0 x 1.2 m
from the start (game-design rule 10), so nothing thickens, nothing clips, and
anything up to 5 m wide, the treant included, passes the gate.

Seconds for one attacker to fell one segment, regen counted, no other techs:

| attacker | weapon | thornwall s | + Ironbark s | + Crystal Bramble s |
|---|---|---|---|---|
| `battering_ram` | ram_head | 4 | 24 | 60 |
| `mangonel` | mangonel_shot | 30 | 130 | 810 |
| `bombard` | bombard_shot | 6 | 20 | 58 |
| `trebuchet` | trebuchet_stone | 0 (one shot) | 10 | 30 |
| `grenadier` (one model) | grenade | 37 | 153 | never (regen wins) |
| `war_balloon` | fire_bomb | 10 | 45 | 235 |
| `steam_tank` | tank_cannon | 6 | 24 | 60 |
| `biplane_bomber` | aerial_bomb | 0 (one bomb) | 21 | 77 |
| `plasma_artillery` | plasma_mortar | 0 (one shot) | 12 | 54 |
| `titan` | star_cannon | 0 (one shot) | 5 | 20 |
| `man_at_arms` | arming_sword | 78 | 3139 | never |
| `sylvan_elder_treant` | sylvan_treant_fists | 2 | 10 | 24 |

### 6.3 Common buildings taken

`town_center`, `citadel`, `house`, `manor`, `lumber_camp`, `mining_camp`, `mill`,
`farm`, `aether_extractor`, `barracks`, `archery_range`, `siege_workshop`, `forge`,
`temple`, `laboratory`, the full tower chain (`watchtower`, `guard_tower`,
`bombard_tower`, `gun_turret`, `plasma_turret`), `anti_air_battery`,
`shield_generator`.

The tower chain is how the Wardens defend their hedges: a watchtower in Hearth, then
crossbow loops, a bombard, an autocannon and a plasma spire. Inside the base, the
citadel's keep arrows, flak batteries (Engine) and shield generators (Ascendant)
cover the interior against flyers and drops.

---

## 7. Techs

### 7.1 Unique techs

| id | age | at | cost | time s | requires | effects |
|---|---|---|---|---|---|---|
| `sylvan_yew_longbows` (Seasoned Yew) | Banner | archery_range | 125F 150W | 40 | - | `sylvan_longbowman` range +1, vision +1 (no conversion: archers stay as anti-air) |
| `sylvan_pack_tactics` | Banner | sylvan_beast_lodge | 150F 75W | 35 | - | Beast and Cavalry speed x1.1; Beast vision +2 (replaces Husbandry) |
| `sylvan_ironbark` | Banner | lumber_camp | 100F 200W 75Cu | 45 | - | thornwall and gate hp x3, slash +0.25, crush +0.1, regen +2 |
| `sylvan_starshot_arrows` | Starfall | archery_range | 200F 150W 40Ae | 50 | - | `sylvan_longbowman` damage x1.35, hp x1.4 |
| `sylvan_barkskin` | Starfall | sylvan_heart_grove | 250F 150W 50Ae | 50 | - | Infantry and Beast hp x1.1; Infantry regen +0.5, Beast regen +1 (replaces Plate Armour) |
| `sylvan_wake_the_wildwood` | Starfall | citadel | 600F 150W 150Fe 200Si 200Ae | 120 | owns sylvan_heart_grove | **advance to the Engine Age** (replaces `to_engine`) |
| `sylvan_crystal_song` | Engine | sylvan_heart_grove | 300F 200W 100Ae | 55 | - | aether gather x1.25, adamant x1.3, carry +2 (replaces the engineer) |
| `sylvan_crystal_bramble` | Engine | sylvan_heart_grove | 300W 150Si 150Ae | 60 | sylvan_ironbark | thornwall and gate hp x2.2, fire +0.3, energy +0.2, regen +5 |
| `sylvan_wildwood_ascends` | Ascendant | sylvan_heart_grove | 400F 400W 300Ae 150Ad | 80 | sylvan_barkskin | **signature**: Beast hp x1.15, regen +1.5, fire armour +0.15; Elder Treants cost x0.92 |

`sylvan_wake_the_wildwood` matches `to_engine`'s price (600F 250Fe 200Si 200Ae,
1250 in all) but trades 100 iron for 150 wood (1300 in all), and its building gate is
the Grove instead of the Aether Foundry. Since it is researched at the `citadel`,
the Wardens must still raise the citadel, as framework section 3.1 intends.

The Wildwood Ascends is held to +1.5 regen and an 8% treant discount. With Barkskin,
beasts then regrow 2.5 hp/s (about 2.6% a second on a 95-hp wisp), and a fully grown
treant (4554 hp for 1380, 3.30 hp per cost) stays below a titan with the common
Mechanical techs (3.58).

### 7.2 Age path

| Advance | Tech | At | Gate |
|---|---|---|---|
| Hearth -> Banner | `to_banner` | town_center | - |
| Banner -> Starfall | `to_starfall` | town_center | owns `forge` |
| Starfall -> Engine | `sylvan_wake_the_wildwood` | citadel | owns `sylvan_heart_grove` |
| Engine -> Ascendant | `to_ascendant` | citadel | owns `laboratory` |

### 7.3 Common techs taken (40)

Hearth: `to_banner`, `double_bit_axe`, `horse_collar`, `stone_mining`, `prospecting`.
Banner: `to_starfall`, `bow_saw`, `heavy_plow`, `wheelbarrow`, `forging`, `fletching`,
`scale_mail`, `scale_barding`, `masonry`, `militia_drill`, `pike_drill`,
`herbal_medicine`. Starfall: `crop_rotation`, `handcart`, `deep_shafts`,
`aether_resonance`, `aether_tempering`, `rifled_barrels`, `plate_barding`,
`architecture`, `halberd_drill`, `star_tithe`, `ballistics`. Engine: `to_ascendant`,
`steam_harvesters`, `adamant_cutting`, `steam_hammers`, `smokeless_powder`,
`ferroconcrete`, `analytical_engine`, `targeting_computers`, `punch_card_logistics`.
Ascendant: `star_forged_arms`, `force_lattice`, `deflector_shields`.

Every `requires` chain is complete inside this list, and every research building is
in the faction's buildings.

---

## 8. What is excluded, and why

| Common pool | Used | Excluded | Excluded % |
|---|---|---|---|
| Units (43) | 17 | 26 | 60% |
| Buildings (36) | 22 | 14 | 39% |
| Techs (53) | 40 | 13 | 25% |
| **Whole pool (132)** | **79** | **53** | **40%** |

**Arithmetic note.** The unit share is above the 25-40% band on purpose: 11 uniques
stand in for whole common lines (heavy armour, horses, crossbows and guns, machines,
early flyers; 8.1). Counting the whole common pool (units, buildings and techs), the
Wardens exclude 40%.

### 8.1 Excluded units (26)

| Gap | Excluded | Why, and what fills it |
|---|---|---|
| **No heavy armour** | `man_at_arms`, `knight`, `aether_lancer`, `shock_trooper` | Wardens fight in leather and bark. Beasts are their heavy line (war bears, thornbacks, treants), and Barkskin gives regen in place of plate. Heavy infantry win straight fights against them. |
| **No horse army** | `horse_archer` (and the knight and aether lancer above); the `stable` | Elk replace horses: one shock rider (`sylvan_elk_rider`), and no mounted archer. The scout rider (trained at the town centre) is the only horse. |
| **No crossbows or hand-guns** | `crossbowman`, `arquebusier`, `gatling_crew` | The yew longbow line instead, then crystal staves. The rifle squad is the one firearm, a hunter's long gun that arrives in the Engine Age. |
| **No machines** | `clockwork_automaton`, `steam_tank`, `mech_walker`, `rocket_battery`, `flak_wagon`, `hover_tank`, `plasma_artillery`, `storm_walker`, `titan`; `aether_foundry`, `engine_works` | Living constructs and beasts instead: thornbacks, rootbreakers, wisps, starstags and treants. The Wardens have no ground vehicle at all after the timber siege engines. |
| **No flyers until the Ascendant** | `war_balloon`, `gyrocopter`, `biplane_bomber`, `gunship`, `starfighter`, `drone_swarm`, `star_bomber`; `airfield`, `starport` | Wisp swarms in the Ascendant Age are their only air, and their seedbursts their only bombs. Until then, enemy flyers are answered from the ground: archers, javelins, druids, rifles, Crystal Rangers, towers and flak batteries. |
| **No powder siege** | `bombard` | Their siege is all timber (ram, mangonel, ballista, trebuchet), plus bears and grenadiers, then rootbreakers (Engine) and the treant. |
| **No engineer** | `engineer` | Peasants all game, with Crystal Song. Repair is slow (3 hp/s), but the hedges regrow on their own. |

### 8.2 Excluded buildings (14)

`stable` (the Beast Lodge replaces it), `castle` (the Wardens build no keeps), `aether_foundry` (the
Heartwood Grove), `engine_works`, `airfield`, `starport` (no machines or early flyers),
and the eight common wall and gate tiers `palisade_wall`, `palisade_gate`, `stone_wall`,
`stone_gate`, `reinforced_wall`, `reinforced_gate`, `force_wall`, `force_gate`
(thornwalls instead).

### 8.3 Excluded techs (13)

`to_engine` (replaced by `sylvan_wake_the_wildwood`), `crossbow_drill`,
`hand_cannon_drill` (no crossbow line), `husbandry` (stable; Pack Tactics instead),
`plate_armor`, `ballistic_weave`, `adamant_weave` (the infantry armour line; Barkskin and
The Wildwood Ascends instead), `steam_harness` (no men-at-arms), `conscription`
(castle), `aether_infusion` (foundry), `assembly_line`, `adamant_plating` (engine
works), `fusion_cores` (starport).

What this leaves: Wardens infantry top out at scale mail (+5% slash and pierce)
and make it up with hp, regen and healing. The cavalry still get both barding techs.

---

## 9. Counters and weaknesses

Equal-cost Lanchester scores as in game-design 5.5 (above 1 means A wins). Same
method: no techs; cleave, chain, spray and nova count extra models only against
squads; flak splash reaches about 2.2 drones or wisps.

| A | B | A's score | A dps into B | B dps into A |
|---|---|---|---|---|
| `sylvan_hound_pack` | `archer` | 2.88 | 4.4 | 3.6 |
| `sylvan_hound_pack` | `spearman` | 0.63 | 2.6 | 7.6 |
| `sylvan_hound_pack` | `militia` | 0.97 | 2.6 | 3.3 |
| `sylvan_longbowman` | `pikeman` | 1.29 | 9.2 | 4.8 |
| `sylvan_longbowman` | `crossbowman` | 1.02 | 5.8 | 6.3 |
| `man_at_arms` | `sylvan_longbowman` | 1.75 | 7.9 | 5.1 |
| `javelineer` | `sylvan_longbowman` | 2.76 | 10.4 | 4.8 |
| `sylvan_elk_rider` | `crossbowman` | 1.90 | 10.4 | 5.6 |
| `pikeman` | `sylvan_elk_rider` | 2.77 | 12.8 | 7.5 |
| `knight` | `sylvan_elk_rider` | 1.37 | 8.6 | 5.7 |
| `sylvan_great_bear` | `arquebusier` | 1.38 | 14.2 | 11.1 |
| `halberdier` | `sylvan_great_bear` | 1.54 | 16.0 | 12.6 |
| `aether_lancer` | `sylvan_great_bear` | 1.43 | 16.6 | 8.2 |
| `clockwork_automaton` | `sylvan_great_bear` | 1.64 | 14.6 | 8.9 |
| `sylvan_thornback` | `rifle_squad` | 2.44 | 51.3 | 9.1 |
| `shock_trooper` | `sylvan_thornback` | 1.72 | 19.4 | 11.1 |
| `steam_tank` | `sylvan_thornback` | 6.14 | 32.0 | 4.3 |
| `sylvan_lightwarden` | `steam_tank` | 1.01 | 29.5 | 42.1 |
| `sylvan_lightwarden` | `gyrocopter` | 1.88 | 32.8 | 11.2 |
| `gatling_crew` | `sylvan_lightwarden` | 1.27 | 30.0 | 18.8 |
| `shock_trooper` | `sylvan_lightwarden` | 1.20 | 24.3 | 16.9 |
| `rifle_squad` | `sylvan_lightwarden` | 1.25 | 12.0 | 18.8 |
| `sylvan_lightwarden` | `storm_walker` | 0.88 | 26.2 | 45.6 |
| `sylvan_lightwarden` | `gunship` | 0.96 | 26.2 | 33.2 |
| `halberdier` | `sylvan_rootbreaker` | 1.70 | 14.9 | 18.0 |
| `rifle_squad` | `sylvan_rootbreaker` | 0.90 | 11.1 | 30.0 |
| `sylvan_starstag_rider` | `plasma_artillery` | 3.53 | 50.1 | 20.0 |
| `shield_trooper` | `sylvan_starstag_rider` | 1.70 | 36.7 | 27.9 |
| `hover_tank` | `sylvan_starstag_rider` | 1.43 | 32.5 | 26.0 |
| `halberdier` | `sylvan_starstag_rider` | 1.29 | 21.4 | 37.1 |
| `shield_trooper` | `sylvan_wisp_swarm` | 1.02 | 36.7 | 5.0 |
| `flak_wagon` | `sylvan_wisp_swarm` | 2.11 | 105.6 | 5.9 |
| `starfighter` | `sylvan_wisp_swarm` | 1.77 | 27.4 | 5.0 |
| `sylvan_wisp_swarm` | `hover_tank` | A cannot be hurt | 4.1 | 0 |
| `shield_trooper` | `sylvan_elder_treant` | 1.64 | 33.0 | 38.6 |
| `halberdier` | `sylvan_elder_treant` | 1.31 | 17.1 | 43.5 |
| `storm_walker` | `sylvan_elder_treant` | 1.68 | 38.6 | 30.2 |
| `hover_tank` | `sylvan_elder_treant` | 2.14 | 37.9 | 30.2 |
| `archer` | `war_balloon` | 0.63 | 3.4 | 12.0 |
| `javelineer` | `war_balloon` | 0.66 | 4.1 | 12.0 |

`sylvan_lightwarden` is the Crystal Ranger. Bombs never hit flyers, so the wisps'
seedburst counts only against ground targets; its 1.2 m splash reaches about 2.2
models of a squad, the root ram's 0.5 m about 1.5. The two balloon rows ignore range:
archers and javelineers shoot from 8 m at a 3.5 m/s balloon that must fly over them
to bomb, so they do better in play, but they are the thin end of the Wardens' air
defence.

**Strengths**

- Economy: the best wood and forage income, a food drop at every Beast Lodge, and
  walls that need no stone.
- Banner reach: longbows at 11 m (12 with Seasoned Yew) out-range every Banner
  infantry and cavalry missile. Elk raid at 5.8 m/s.
- Mobility: thickets are private terrain, so ambush and retreat are always on.
- Sustain: druids, Barkskin, regen on every beast, hedges that regrow.
- The Crystal Ranger trades evenly with landships and beats gyrocopters, so the
  Wardens are not helpless against machines. Starstag riders run down artillery.

**Weaknesses (by design)**

- **Fire.** Every beast, construct and hedge has 0 fire armour (the treant 0.1 until
  The Wildwood Ascends). Grenadiers, war balloons, flamers, rockets, bombers and
  plasma are the best tools against the Wardens.
- **Polearms.** Beasts take x2 from spears, pikes and halberds; the treant takes x3
  (Beast and Giant). A halberd wall stops the bear and treant core.
- **Heavy infantry.** Men-at-arms and shock troopers beat longbows, elk and
  Crystal Rangers head-on, and the Wardens have no heavy infantry of their own to meet them.
- **Splash and gatlings.** Light, low-armour infantry dies to mangonels and gatlings.
- **Weak walls and no castle.** Hedges are about two thirds of common wall hp. The
  Wardens cannot out-turtle anyone.
- **Thin late ground army.** No tanks, walkers or artillery. The late siege is
  beasts that must walk up to the wall (rootbreakers, treants), machines kite them,
  and late heavy armour is answered only by the Crystal Ranger.
- **Thin Starfall anti-air.** Longbowmen cannot shoot up and there are no crossbows
  or arquebuses, so Starfall air defence is archers (kept for this), javelineers,
  druids, guard towers and keep arrows. Upgrading guard towers to bombard towers
  gives up their air fire. War balloons stay a real threat until rifles and Crystal
  Rangers arrive.
- **Crater economy.** Without engineers, aether and adamant come in about 11%
  slower even after Crystal Song.

---

## 10. AI notes

```
aggression: 0.55
army: sylvan_hound_pack 0.5, sylvan_longbowman 1.0, sylvan_elk_rider 0.7,
      sylvan_great_bear 0.8, sylvan_druid 0.35, mangonel 0.3, trebuchet 0.3,
      sylvan_lightwarden 1.0, sylvan_rootbreaker 0.4, sylvan_starstag_rider 0.7,
      sylvan_wisp_swarm 0.6, sylvan_elder_treant 0.3
eco_focus: wood, food, aether, copper, stone, iron, silicon, adamant
```

- **All eight resources.** Stone pays for the citadel upgrade (200; the Engine
  age-up is proclaimed there), towers, the temple, the Grove and the extractors.
  Adamant pays for `to_ascendant` (200), treants (350), wisps and starstags. Copper
  comes before stone because elk, Ironbark, druids, bears and timber siege spend it
  from the Banner Age on, while stone is mostly one Starfall lump.
- **Siege in the army.** Mangonels against blobs and trebuchets against towers and
  walls in Banner and Starfall, rootbreakers from the Engine Age, treants last.

- **Raider, not turtle.** Aggression 0.55: harass with hounds in Hearth and elk in Banner.
  Path raids through thickets (Forestwalker) and retreat into them when chased.
- **Hedges early, towers on chokes.** Wall the woodlines and the base with thornwalls
  (cheap), research Ironbark in Banner, and put towers at the gates.
- **Grove first in Starfall.** It gates the Engine Age and every late unique. Then Barkskin.
- **Counter-picks from the common pool**, which the weights alone do not express:
  - pikemen and halberdiers against cavalry and giants;
  - javelineers against archers and horse archers;
  - grenadiers against blobs;
  - rifles against flyers in Engine;
  - shield troopers as the Ascendant line.
- **Crystal Rangers scale with enemy machines.** Raise their share whenever the enemy
  fields tanks, walkers or flyers; raise starstags against artillery and siege.
- **Keep druids behind beasts.** Keep beasts away from enemy fire weapons: pull bears
  back from grenadiers and flamers.
- **Treants and rootbreakers need an escort** of Crystal Rangers or shield troopers
  against machines that kite them. Use them to break walls, not to chase.

---

## 11. Notes for the engine

- **Stat Mul stacking.** The hedge numbers assume repeated `Stat ... op: Mul` effects
  multiply: x3 then x2.2 gives x6.6, so 350 hp becomes 2310. If the engine adds
  multipliers instead, `sylvan_crystal_bramble` should be re-tuned.
- **Selectors target buildings.** `Selector::Building("sylvan_thornwall")` stat
  effects must apply to existing and future segments, including `Regen`.
- **Arc weapons on infantry** (`sylvan_yew_longbow`) need the splash at the impact
  point to resolve the hit, since an Arc is not homing. Splash is 0.35 m per arrow, with
  volley 2 and spread 0.6.
- **Healer with a weapon.** The Star Druid has both `healer` and a weapon; it should
  heal injured allies in range first and fight otherwise. It mends one target at a
  time, lowest hp fraction first (game-design 11), and its 4 hp/s is priced that way.
- **Living flyers.** `sylvan_wisp_swarm` is Flying + Beast. Flying-tag bonuses (flak,
  EMP) apply, and Mechanical ones do not. It flies at 12 m like the drone swarm, above
  every box (the Grove is 8 m, the treant 9.5 m).
- **Two weapons of different reach.** The wisps fire motes from 7 m and drop the
  seedburst only from 1 m, when they pass over a target; it is `fire_on_move: true` so
  they can bomb in passing without stopping. An attack-move should keep them on the
  motes; an attack order on a ground unit or building may fly them over it.
- **Wall footprint.** Thornwall segments are 3.0 x 1.0 m and thorn gates 9.0 x 1.2 m,
  the common footprints (game-design 11), and they never upgrade in place.
- **Fire on the move.** `sylvan_thorn_spray` (like the landship flamer) and
  `sylvan_antler_ray` (like the laser lance) are `fire_on_move: true`.
