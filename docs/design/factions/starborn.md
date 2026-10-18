# The Synod of the Fallen Star (`starborn`)

> *They saw it fall first, and have never looked away.*

Data: `assets/data/factions/starborn.ron`. Framework and common pool:
`docs/design/game-design.md`. Every number below is read from those files.

| Field | Value |
|---|---|
| Colour | `(0.85, 0.2, 0.65)`: Byzantine magenta, the imperial purple-red of the Synod's vestments. It stays clear of the violet aether crystals and vents in the crater it holds (CIE76 deltaE 40 and 47) and of Ironhold blue and Khaganate red (62 and 63) |
| Terrain traits | none (instead: the earliest and best flyers and hover craft) |
| Gather bonus | aether x1.2, adamant x1.25 |
| Worker | `peasant` (and the common `engineer` from the Engine Age) |
| Start | 300 food, 300 wood, 150 stone, 100 copper; 5 peasants, 1 scout rider; a `town_center` |
| Roster | 35 trainable units: 2 workers, 24 common military, 9 unique |
| Unique buildings | `starborn_wayshrine`, `starborn_aerie`, `starborn_sanctum` |
| Unique techs | 8 |

---

## 1. Identity and lore

The Synod began as a handful of star-watching monks on a bare hill, keeping a
calendar of the sky. For a hundred years before the fall they charted the red star
that rose each autumn, and they preached that it was coming *to* them. When it broke
over the heartland they were the only realm that marched toward the light instead
of away from it, and their brothers walked into the crater while the glass was still
hot.

What they carried out made them. A sliver of crystal sewn into a kite lifted a lay
brother into the air, and the Synod was flying before anyone else had seen a war
balloon. A shard set in a reliquary floated a hand's breadth off the ground, and
the first hover craft was a shrine. Lenses ground from quartz and crystal turned
star-light into a beam, so the order never needed the smoke and powder other realms
bought their way into the Starfall Age with. The Rule of Light, written in the first
winter after the fall, forbids powder in the hands of the faithful; their engines and
towers may burn it. So the Synod fields no arquebus, grenade, bombard, rocket rack,
rifle, gatling or hand-dropped bomb, while its mech walkers, gyrocopters, castles,
bombard towers and gun turrets fire powder as any realm's do.

The Synod is Templar and Byzantine in look: purple and gold, white enamel, gilded
plate, censers, icons, hymns. It is an army of paladins, monks and barefoot zealots
that grows lightbearers, lumen knights with blades of solid light, seraphs on brass
wings, orbital lances that call a pillar of fire from the sky and, at the end, the
**Archon**: the star given a body. They are still knights. They have found the
spaceship, and they pray to it.

Weaknesses, in-world: the order is small, devout and expensive. Every miracle costs
aether, so a Synod cut off from the crater starves. Its heralds have no gift for
mountain paths or bramble ways. And the faith that refused powder also refused the
cheap, massed firepower that powder buys.

## 2. Playstyle and win conditions

**Style.** Elite, sustained and aether-hungry. A Synod army is smaller per resource
than its rivals' but heals itself (monks, paladins, the Reliquary Ark, the Archon),
wears shields from the Engine Age on, and kills with Energy damage that ignores
most armour. Its eyes are in the sky: Star Kites from the Banner Age, Aether Sails
in the Starfall Age, seraph hosts in the Engine Age, the full starport in the last.

**How the Synod wins**

1. **Own the crater early.** Wayshrines on the road to the centre give drop sites
   and a trickle; kites spot the crater from the Banner Age. Take the vents with
   aether extractors in the Starfall Age and hold them with a healed, shielded ball
   (halberdiers, lightbearers, paladins, an ark) that does not bleed out.
2. **Win the sky.** From the Starfall Age (Aether Sails) the Synod has the fastest,
   toughest flyers in every age. Seraphs shred gyrocopters and bombers; star kites,
   war balloons and, in the last age, star bombers go where walls do not.
3. **Out-tech, then out-last.** With +20% aether and +25% adamant the Synod reaches
   the Ascendant Age first if the crater holds. Hover tanks, orbital lances and the
   Archon then end the game: the lances break fortresses from 22 m, the Archon holds
   any infantry fight and crosses lakes to do it.

**How it loses.** Starved of aether, swarmed early by cheap massed archers and
skirmishers, out-ranged on the ground by rifles and artillery, or caught by EMP and
flak late.

## 3. Economy

### 3.1 Gathering

| Resource | Peasant /s (base -> Synod) | With Synod techs | Notes |
|---|---|---|---|
| aether | 0.3 -> 0.36 | 0.45 with Aether Resonance; 0.50 with Adamant Cutting as well | engineer 0.4 -> 0.48, 0.66 with both |
| adamant | 0.18 -> 0.23 | 0.25 with Star Charts; 0.32 with Adamant Cutting as well | engineer 0.26 -> 0.33, 0.46 with both |
| everything else | common rates | common techs | no food, wood, stone or metal bonus |

The Synod has no bonus on food, wood, stone or metal, and every unique costs aether.
Its economy is normal at home and excellent in the crater: it wants more workers
in the centre than any other faction, which is exactly where they are in danger.

Its Banner-age aether pace comes from wayshrines, not from a gather tech: shrines
on the crater road shorten every crystal worker's walk and add their trickle.
Star Charts pays off later instead, in adamant and in star-kite scouting, so it no
longer mirrors the Caliphate's Star Reckoning.

### 3.2 Trickles (passive income)

| Source | Aether /s | Adamant /s | From |
|---|---|---|---|
| `starborn_wayshrine` (up to 5, 75W 75S each) | 0.05 each, 0.25 for five | - | Banner |
| `aether_extractor` on a vent | 0.5 each | 0.04 each | Starfall |
| `star_tithe` (temple) | 0.15 | - | Starfall |
| `starborn_meteor_pilgrimage` (wayshrine) | 0.1 | 0.06 | Engine |

Five wayshrines are worth roughly one crystal worker. They are a floor that keeps
the Synod teching when it is pushed out of the centre, not a substitute for it: the
100 aether for `to_starfall` takes nearly 7 minutes from five shrines alone, so a
Synod player still walks two or three workers to the crater in the Banner Age.

### 3.3 Drop sites

All eight resources: `town_center` / `citadel`. Wood: `lumber_camp`. Food: `mill`.
Stone, copper, iron, silicon, aether, adamant: `mining_camp`. Aether: `aether_extractor`.
Aether and adamant: `starborn_wayshrine` (cheap, stone-built, five allowed: plant one
at each contested meteor and on the crater rim).

### 3.4 Age path

`to_banner`, `to_starfall` at the town centre (needs a `forge`); `to_engine`,
`to_ascendant` at the citadel (needs an `aether_foundry`, then a `laboratory`). All
five gate buildings are in the Synod's list.

## 4. Terrain: no trait, so it flies and floats

The Synod gets no terrain trait. Cliffs and bramble thickets stop its foot and horse
like anyone's, and it is the only faction with nothing that climbs, wades deep water
or ignores sand and marsh on foot. Instead it owns the air and the water surface
earlier than anyone:

| Age | What gets it past terrain | How |
|---|---|---|
| Banner | `starborn_star_kite` | a flyer one age before any common flyer: scouts cliffs, highlands and the crater |
| Starfall | `starborn_reliquary_ark` (Hover), `war_balloon`, `starborn_aether_sails` | hover two ages before the common hover tank: heals an army across shallows, marsh and deep water at x0.8 |
| Engine | `starborn_seraph`, `gyrocopter` | flying infantry and gyro raids over walls and cliffs |
| Ascendant | `hover_tank`, `starborn_orbital_lance` (Hover), `starborn_archon` (Hover), the starport flyers (the star bomber among them) | a hover siege line, a giant that crosses lakes, and bombs from the sky again |

Hover cannot enter thickets or cliffs, so a Synod army still has to go round the
crater's cliff rim through its gaps. The Ironhold Climbers and Sylvan Forestwalkers
can use exactly those obstacles against it; the answer is to see them coming (kites)
and to bring the fight over water or into the open.

## 5. Roster

Roles in brackets. Unique units are in **bold**.

| Age | Common (24 military + 2 workers) | Unique (9) |
|---|---|---|
| Hearth | `peasant` (worker), `scout_rider` (scout, raider), `militia` (cheap melee), `spearman` (anti-cavalry), `archer` (ranged) | - |
| Banner | `man_at_arms` (line melee), `pikeman` (anti-cavalry), `crossbowman` (ranged, anti-armour), `knight` (heavy cavalry), `monk` (healer), `battering_ram` (wall breaker), `ballista` (anti-siege) | **`starborn_zealot`** (fast shock squad, anti-spear), **`starborn_star_kite`** (flying scout and harasser) |
| Starfall | `halberdier` (anti-cavalry, anti-machine), `aether_lancer` (shock cavalry), `clockwork_automaton` (armoured brawler), `war_balloon` (flying siege), `trebuchet` (long siege) | **`starborn_lightbearer`** (beam ranged infantry, replaces the arquebusier), **`starborn_paladin`** (healing heavy cavalry), **`starborn_reliquary_ark`** (hover healer) |
| Engine | `engineer` (worker), `mech_walker` (anti-armour, anti-air), `gyrocopter` (air raider) | **`starborn_lumen_knight`** (shielded beam-blade infantry), **`starborn_seraph`** (flying anti-air squad) |
| Ascendant | `shield_trooper` (shielded ranged line), `hover_tank` (anti-armour), `gunship` (air heavy), `starfighter` (air superiority), `drone_swarm` (the Synod calls them cherubim), `star_bomber` (heavy air siege) | **`starborn_orbital_lance`** (beam artillery), **`starborn_archon`** (hovering healer giant) |

Two line upgrades carry the Synod's ranged infantry from bow to beam:
`archer` -> `crossbowman` (`crossbow_drill`) -> `starborn_lightbearer`
(`starborn_rite_of_the_lens`). The spear line is the common one: `spearman` ->
`pikeman` -> `halberdier`.

### 5.1 Unique units

Costs are per training; dps/cost and ehp/cost count the whole squad and compare
with the age medians of the common pool (design doc section 8).

| id | name | age | tags | squad | pop/model | cost | train s | hp (+shield) | armour % | speed | moves | dps/model | range | trained at | dps/cost (median) | ehp/cost (median) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `starborn_zealot` | Zealot Band | 2 | Infantry, Melee, Light | 3 | 1 | 200F 50W 50Fe | 30 | 65 | S5 | 3.2 | Foot | 6.7 | 0.8 | barracks | 0.067 (0.054) | 0.65 (0.76) |
| `starborn_star_kite` | Star Kite | 2 | Infantry, Ranged, Flying, Light, Scout | 1 | 1 | 60F 80W 15Ae | 30 | 70 | - | 6.5 | Flying 12 m | 3.5 | 1 | starborn_aerie | 0.023 | 0.45 |
| `starborn_lightbearer` | Lightbearer | 3 | Infantry, Ranged | 1 | 1 | 60F 50W 25Cu 10Si 15Ae | 30 | 105 | S5 P15 | 2.7 | Foot | 12 | 9 | archery_range | 0.075 (0.067) | 0.66 (0.81) |
| `starborn_paladin` | Paladin | 3 | Cavalry, Melee, Heavy, Support | 1 | 2 | 150F 20Cu 90Fe 40Ae | 40 | 240 | S40 P35 C15 E20 | 5.4 | Mounted | 18.8 + heals 2/s | 1 | castle, starborn_sanctum | 0.062 (0.067) | 0.80 (0.81) |
| `starborn_reliquary_ark` | Reliquary Ark | 3 | Mechanical, Support | 1 | 3 | 80W 100Cu 60Fe 40Ae | 45 | 340 | S40 P50 C10 F30 | 4.0 | Hover | 4.8 + heals 4/s | 4 | temple, starborn_sanctum | 0.017 | 1.21 |
| `starborn_lumen_knight` | Lumen Knight | 4 | Infantry, Melee, Heavy, Shielded | 1 | 1 | 130F 110Fe 50Si 40Ae | 34 | 220 (+60) | S35 P30 C20 F15 E25 | 2.8 | Foot | 20 | 1 | barracks, castle | 0.061 (0.074) | 0.85 (0.95) |
| `starborn_seraph` | Seraph Host | 4 | Infantry, Ranged, Flying, Shielded | 3 | 1 | 160F 110Fe 90Si 90Ae | 42 | 110 (+30) | S15 P15 | 7.5 | Flying 12.5 m | 10 | 8 | starborn_aerie | 0.067 (0.074) | 0.93 (0.95) |
| `starborn_orbital_lance` | Orbital Lance | 5 | Siege, Mechanical, Shielded | 1 | 4 | 100Fe 160Si 270Ae 70Ad | 60 | 420 (+80) | S30 P45 C20 F20 E15 | 2.8 | Hover | 25 | 22 (min 7) | starborn_sanctum | 0.042 | 0.83 |
| `starborn_archon` | Archon | 5 | Mechanical, Giant, Heavy, Hero, Shielded, Support | 1 | 10 | 250Fe 250Si 600Ae 400Ad | 100 | 3400 (+400), regen 3 | S60 P60 C40 F40 E40 | 2.4 | Hover | 48 single target (104 on a group) + heals 4/s | 12 | starborn_sanctum | 0.032 (0.069 on a group) | 2.53 |

Pricing notes:

- **Zealot Band** sits 24% above the Banner dps median and well below on ehp: no
  armour at all against pierce. Fervour (+20% hp, 0.5 hp/s regen) lifts it to 0.78
  ehp/cost.
- **Star Kite** is priced on flight and 18 m vision, not dps. Star Charts takes it
  to 23 m vision and 7.2 m/s (+2 as a Scout, +3 and x1.1 speed as a kite).
  Its 15 aether is the only aether in the Synod's Banner roster: they found the star
  first.
- **Lightbearer** costs exactly an arquebusier (160) with 8% less dps and 1 m less
  range, but deals Energy (almost nothing resists it before the Ascendant Age) with
  x1.25 against machines. Prism Focus in the Engine Age takes it to 15 dps, 131 hp and
  10 m range with 11 m vision (0.094 dps/cost, 0.82 ehp/cost): a rifleman's
  efficiency with a Starfall unit's frailty.
- **Paladin** is a little under both Starfall medians on paper; its 2 hp/s
  single-target heal on infantry and cavalry (two-thirds of a monk's 3) is what it
  is paid for.
- **Lumen Knight** is 18% under the Engine dps median (20 dps) and 10% under on ehp,
  so one on one it loses narrowly to the shock-trooper benchmark (0.89) and trades
  evenly with a mech walker. Its cleave doubles its damage on packed infantry and its
  Energy blade is barely resisted, so it beats rifle squads that let it close (1.16).
  It is a specialist, not a strict upgrade on the shock trooper.
- **Seraph Host** is 10% under the Engine dps median, but its motes deal Energy, home,
  fire on the move and do x1.5 against flyers.
- **Orbital Lance** is priced like plasma artillery (600 vs 560) and trades splash for
  reach against single big targets (see 7.2).
- **Archon** costs far less than the titan (1500 against 2600). Against one target it
  deals 48 dps, 0.032 per cost, under the titan's 0.039. Its chain reaches three
  targets (two jumps), so against a group it deals 84 dps from the chain plus 20 for
  each trooper in Judgement's 5 m ring: about 0.069 per cost counting the jumps and
  one ring hit, 1.8 times the titan. That crowd damage, the heal and the hover hull
  are what it is paid for. Heavy single targets beat it at equal cost (hover tank
  2.35, gunship 1.84, storm walker 1.73).

### 5.2 Unique weapons

`hits`: G ground units, A air, B buildings. Every weapon has a `look`; every one
that fires something has a glowing `projectile`.

| id | name | kind | damage | cd s | dps | range m | splash | hits | bonus | used by |
|---|---|---|---|---|---|---|---|---|---|---|
| `starborn_scourge` | Iron Scourge | Melee Swing (90 deg, cleave 1) | 8 Slash | 1.2 | 6.7 | 0.8 | 0 | GB | Spear x1.3 | zealot |
| `starborn_censer_drop` | Censer Drop | Bomb (fall 0.8 s) | 14 Fire | 4 | 3.5 | 1 | 1.2 | GB | Building x1.5 | star kite |
| `starborn_sunlens` | Sunlens | Beam (0.3 s, 3 ticks) | 24 Energy | 2 | 12 | 9 | 0 | GAB | Mechanical x1.25, Building x0.25 | lightbearer |
| `starborn_sun_hammer` | Sun Hammer | Melee Smash | 30 Crush | 1.6 | 18.8 | 1 | 0.6 | GB | Mechanical x1.25, Building x0.6 | paladin |
| `starborn_radiant_hymn` | Radiant Hymn | Nova | 12 Energy | 2.5 | 4.8 | 4 | ring | G | - | reliquary ark |
| `starborn_lightblade` | Lightblade | Melee Swing (80 deg, cleave 1) | 28 Energy | 1.4 | 20 | 1 | 0 | GB | Mechanical x1.25 | lumen knight |
| `starborn_seraph_motes` | Seraph Motes | Homing (fires on the move) | 12 Energy | 1.2 | 10 | 8 | 0 | GAB | Flying x1.5, Building x0.25 | seraph |
| `starborn_orbital_beam` | Orbital Lance | Beam (1 s, 5 ticks; 2 s windup) | 200 Energy | 8 | 25 | 22 (min 7) | 0 | GB | Building x2.5, Giant x1.5, Mechanical x1.2 | orbital lance |
| `starborn_archon_wrath` | Archon's Wrath | Chain (2 jumps, 4.5 m) | 70 Energy | 2.5 | 28 per target, 3 targets | 12 | 0 | GAB | Infantry x1.2 | archon |
| `starborn_judgement` | Judgement | Nova | 80 Energy | 4 | 20 | 5 | ring | GB | Infantry x1.2, Building x1.5 | archon |
| `starborn_sanctum_ray` | Sanctum Ray | Beam (0.4 s, 4 ticks) | 40 Energy | 2.5 | 16 | 12 | 0 | GAB | Building x0.25 | sanctum |

Flavour of kinds: the Synod fires beams from the Starfall Age (the common pool's
first beam is Ascendant), drops bombs from the Banner Age (common: Starfall), has a
Nova in the Starfall Age (the ark; common: Ascendant) and beam artillery, which the
common pool lacks. It uses every weapon kind except Spray and has almost no Pierce
after the crossbow: its damage is Energy.

### 5.3 Unique buildings

| id | name | age | cost | build s | hp | armour % | role | weapons | trains | techs |
|---|---|---|---|---|---|---|---|---|---|---|
| `starborn_wayshrine` | Wayshrine | 2 | 75W 75S | 35 | 700 | S60 P85 C10 F25 E20 | drop: aether, adamant; trickle 0.05 aether/s; limit 5; vision 12 | - | - | Star Charts, Meteor Pilgrimage |
| `starborn_aerie` | Star Aerie | 2 | 200W 75S 50Cu | 55 | 1500 | S50 P80 C5 F10 E20 | flyer production; vision 14 | - | star kite, seraph host | Aether Sails |
| `starborn_sanctum` | Sanctum of the Star | 3 | 250W 450S 100Fe 100Ae | 160 | 3600 + 600 shield | S70 P85 C20 F30 E35 | elite production and interior defence; limit 2; garrison 10 | starborn_sanctum_ray | paladin, reliquary ark, orbital lance, archon | The Star Answers |

Box sizes: wayshrine 3 x 4 x 3, aerie 7 x 11 x 7 (a tall roost), sanctum 12 x 11 x 12.
The sanctum is tagged Tower, so tower techs (fletching, ballistics, targeting
computers, ferro-concrete) improve its ray and hull. Its standing shield arrives two
ages before any common building shield.

### 5.4 Unique techs

| id | name | age | at | cost | time s | requires | effects |
|---|---|---|---|---|---|---|---|
| `starborn_star_charts` | Star Charts | 2 | wayshrine | 100F 100W 50Cu | 35 | - | adamant gather x1.1; Scout vision +2; star kite vision +3 more and speed x1.1 |
| `starborn_fervour` | Fervour | 2 | barracks | 150F 50Cu | 35 | - | zealot hp x1.2, regen +0.5 |
| `starborn_rite_of_the_lens` | Rite of the Lens | 3 | archery_range | 250F 75Si 50Ae | 50 | crossbow_drill | `crossbowman` -> `starborn_lightbearer` |
| `starborn_aether_sails` | Aether Sails | 3 | aerie | 200W 100Cu 60Ae | 45 | - | Flying speed x1.15, hp x1.1, vision +2 |
| `starborn_holy_orders` | Oath of the Order | 3 | castle | 300F 150Fe 40Ae | 50 | - | paladin hp x1.15; Cavalry energy armour +0.1 |
| `starborn_prism_focus` | Prism Focus | 4 | archery_range | 300F 150Si 75Ae | 55 | - | lightbearer damage x1.25, hp x1.25, range +1, vision +1 |
| `starborn_meteor_pilgrimage` | Meteor Pilgrimage | 4 | wayshrine | 300F 100Fe 100Ae | 55 | - | +0.06 adamant/s, +0.1 aether/s |
| `starborn_the_star_answers` | The Star Answers | 5 | sanctum | 500F 300Ae 150Ad | 80 | - | every unit regen +1 hp/s; Shielded shield hp x1.2; Flying damage x1.1 |

`starborn_rite_of_the_lens` follows rule 3: pop 1 to pop 1, per-model cost 120 to 160
(ratio 1.33), 15 aether of difference. `The Star Answers` is the signature: an army
that never stops healing, under brighter shields, with a sky that hits harder.

Star Charts builds on the kites rather than on aether: the Banner-age aether pace
comes from wayshrines (section 3.1), and the adamant bonus pays off from the Engine
Age.

Common techs: 52 of the 53, all except `hand_cannon_drill` (no arquebusiers).

## 6. What the Synod does not get, and why

17 of the 43 common units are excluded (40%). Each gap is a decision. "Powder" below
means the Rule of Light: no powder in the hands of the faithful (their engines and
towers may burn it).

| Excluded | Age | Why | What fills the role |
|---|---|---|---|
| `slinger` | Hearth | the order's levies train at the monastery butts, not the sheepfold | `archer`, `militia` |
| `javelineer` | Banner | no skirmisher tradition | knights and zealots run archers down; kites spot them |
| `horse_archer` | Banner | the order's knights fight at the lance; no steppe horsemanship | `knight`, `scout_rider` |
| `mangonel` | Banner | the Synod lobs nothing early | rams and ballistae; censer kites burn roofs |
| `arquebusier` | Starfall | powder: a hand gun | `starborn_lightbearer` (and the Rite of the Lens) |
| `grenadier` | Starfall | powder: thrown by hand | the ark's hymn and the balloon's firepots for blobs |
| `bombard` | Starfall | powder: a gun loaded and fired by its crew | `trebuchet`, `war_balloon` |
| `rifle_squad` | Engine | powder: a hand gun | lightbearers with Prism Focus, seraphs |
| `gatling_crew` | Engine | powder: a hand-cranked gun | lumen knights (cleave), seraphs |
| `shock_trooper` | Engine | the Synod armours its knights in light, not steam | `starborn_lumen_knight` |
| `steam_tank` | Engine | smoke-belching landships are profane | `mech_walker`; hover tanks later |
| `rocket_battery` | Engine | powder: rockets lit by hand | `trebuchet`, `war_balloon`; orbital lances and star bombers later |
| `flak_wagon` | Engine | the Synod fights flyers with flyers | seraphs, starfighters, `anti_air_battery` at home |
| `biplane_bomber` | Engine | powder: its bombs are dropped by hand | star kites' censers and the balloon's firepots; `star_bomber` in the Ascendant Age |
| `plasma_artillery` | Ascendant | | `starborn_orbital_lance` |
| `storm_walker` | Ascendant | no war-walkers taller than a mech walker | `hover_tank`, the Archon |
| `titan` | Ascendant | the Synod builds one kind of colossus | `starborn_archon` |

What that does to the faction over time:

- **Hearth and Banner** play conventionally but with no slingers, skirmishers, horse
  archers or mangonels, so cheap massed missile troops are the Synod's early problem.
  Its answers are mobility and sustain: zealots and knights close the distance, monks
  heal, kites see everything.
- **Starfall** is where the Synod turns strange early: beams, a hover shrine,
  paladins, the first building shield, while everyone else is lighting fuses.
- **Engine** is its thinnest ground age: no rifles, gatlings, landships, rockets or
  biplane bombers. Lumen knights, prism lightbearers and mech walkers hold the ground;
  the Synod wins the Engine Age in the air (seraphs and gyrocopters with Aether Sails).
- **Ascendant** is its strongest: the full starport (star bombers included), shield
  troopers, hover tanks, orbital lances and the Archon, all drinking the aether it is
  best at gathering.

Every common building is available (the Synod builds walls well and takes all four
wall tiers, the whole tower chain and the shield generator).

## 7. Counters and weaknesses

### 7.1 Matchups

Lanchester square law at equal cost, as in the design doc section 5.5 (above 1: A
wins). Ranged units do better in play than here; healing and shields' regeneration
are ignored, which understates the Synod.

| A | B | A's score | A dps into B | B dps into A |
|---|---|---|---|---|
| `starborn_zealot` | `spearman` | 1.24 | 7.8 | 4.0 |
| `starborn_zealot` | `pikeman` | 1.12 | 7.4 | 5.3 |
| `starborn_zealot` | `crossbowman` | 1.21 | 6.3 | 7.0 |
| `starborn_zealot` | `militia` | 1.54 | 12.0 | 3.3 |
| `man_at_arms` | `starborn_zealot` | 1.59 | 7.9 | 4.7 |
| `knight` | `starborn_zealot` | 1.71 | 10.7 | 4.3 |
| `archer` | `starborn_star_kite` | 2.94 | 3.8 | 3.5 |
| `javelineer` | `starborn_star_kite` | 7.70 | 11.5 | 3.5 |
| `starborn_lightbearer` | `arquebusier` | 1.08 | 12.0 | 11.1 |
| `starborn_lightbearer` | `halberdier` | 0.85 | 12.0 | 10.9 |
| `aether_lancer` | `starborn_lightbearer` | 1.30 | 17.5 | 10.2 |
| `clockwork_automaton` | `starborn_lightbearer` | 1.16 | 16.2 | 15.0 |
| `starborn_paladin` | `arquebusier` | 1.10 | 18.8 | 11.1 |
| `starborn_paladin` | `aether_lancer` | 0.92 | 15.0 | 14.0 |
| `halberdier` | `starborn_paladin` | 2.03 | 17.1 | 16.9 |
| `starborn_lumen_knight` | `shock_trooper` | 0.89 | 18.0 | 19.4 |
| `starborn_lumen_knight` | `mech_walker` | 0.90 | 22.5 | 22.0 |
| `starborn_lumen_knight` | `rifle_squad` | 1.16 | 40.0 | 14.2 |
| `steam_tank` | `starborn_lumen_knight` | 1.36 | 35.3 | 22.5 |
| `gatling_crew` | `starborn_lumen_knight` | 1.30 | 26.2 | 20.0 |
| `starborn_seraph` | `gyrocopter` | 1.57 | 15.0 | 11.9 |
| `starborn_seraph` | `biplane_bomber` | B cannot hit A | 15.0 | 0 |
| `starborn_seraph` | `star_bomber` | B cannot hit A | 12.0 | 0 |
| `rifle_squad` | `starborn_seraph` | 1.28 | 12.8 | 10.0 |
| `mech_walker` | `starborn_seraph` | 2.86 | 55.0 | 9.0 |
| `flak_wagon` | `starborn_seraph` | 4.00 | 96.0 | 10.0 |
| `starborn_archon` | `shield_trooper` | 1.08 | 43.2 | 22.0 |
| `starborn_archon` | `shield_trooper` (packed: chain and ring each reach 3) | 3.24 | 129.6 | 22.0 |
| `starborn_archon` | `drone_swarm` | 2.17 | 84.0 | 3.8 |
| `hover_tank` | `starborn_archon` | 2.35 | 48.8 | 33.6 |
| `storm_walker` | `starborn_archon` | 1.73 | 53.4 | 38.4 |
| `titan` | `starborn_archon` | 1.67 | 99.0 | 31.2 |
| `gunship` | `starborn_archon` | 1.84 | 26.2 | 22.4 |

Rows against squads count the extra models a cleave, splash or chain reaches (zealot
and lumen-knight cleave, flak and autocannon splash on seraphs, the Archon's three
chain targets on drones). Against
single infantry the Archon is only slightly ahead (1.08); packed together they let its
chain and its Judgement ring reach three at once, and it wins clearly (3.24). Against
heavy single targets it loses at equal cost: it is a crowd-breaker and a healer, not a
duellist.

### 7.2 Breaking walls

Seconds for one attacker to bring down one segment (no techs, shields included):

| attacker | palisade | stone wall | reinforced wall | force wall | house |
|---|---|---|---|---|---|
| `starborn_orbital_lance` | 4 | 34 | 80 | 141 | 11 |
| `plasma_artillery` (common, for comparison) | 4 | 43 | 100 | 138 | 8 |
| `starborn_archon` (Judgement only) | 8 | 71 | 167 | 294 | 23 |
| `starborn_paladin` | 22 | 157 | 444 | 785 | 49 |
| `starborn_star_kite` | 48 | 571 | 1333 | 1835 | 105 |
| `star_bomber` (common) | 3 | 40 | 93 | 128 | 7 |

Earlier siege is common: `battering_ram` (Banner), `trebuchet` and `war_balloon`
(Starfall). The Synod has no Engine-age bomber (no `biplane_bomber`, see section 6);
the trebuchet, balloons and kites carry it to the Ascendant Age, where the star
bomber's aether charges join the orbital lance.

### 7.3 Per unique: beats / loses to

| Unique | Beats | Loses to |
|---|---|---|
| Zealot Band | spearmen, pikemen, militia, crossbowmen that let it close; workers | men-at-arms, knights, mangonels and grenades (splash on a squad with no armour), towers |
| Star Kite | unprotected workers and roofs; fog of war | archers, crossbows, javelineers, the town centre's arrows |
| Lightbearer | armour of every kind, clockwork, mechs, flyers | aether lancers, knights, automatons in melee; arquebusiers out-range it by 1 m |
| Paladin | gunners, automaton-less armies, attrition fights (heals) | halberdiers and pikes, massed aether lancers |
| Reliquary Ark | wars of attrition; crossing water | halberds (x2 vs Mechanical), aether lances, focus fire; it barely fights |
| Lumen Knight | rifle squads up close, packed infantry (cleave), clockwork; even with shock troopers and mech walkers | landships, gatling crews, kiting rifles, EMP (x2 vs Shielded) |
| Seraph Host | gyrocopters, bombers, gunships, drones | flak (x2 vs Flying), mech walkers, massed rifles, starfighters |
| Orbital Lance | walls, towers, titans, storm walkers at 22 m | anything that closes inside 7 m, flyers (it cannot shoot up), swarms (no splash) |
| Archon | infantry blobs, drone swarms, long fights (heals, regen) | hover tanks, storm walkers, titans, gunships, EMP (x2.5 Mechanical x2 Shielded) |

### 7.4 Faction-level weaknesses

1. **Aether dependence.** Every unique past the Banner Age costs aether (15 to 500).
   A Synod locked out of the crater keeps only its wayshrine trickle and stalls.
2. **No terrain trait.** Climbers, Forestwalkers and Amphibious marines can use
   cliffs, thickets and lakes as walls against it. Hover and flyers compensate, at a
   price.
3. **Weak against massed cheap missiles early.** No skirmishers, no horse archers, no
   mangonels: an archer-and-slinger mass is answered only by closing distance.
4. **A thin Engine-age ground army.** No rifles, gatlings, landships or rockets. It
   must win the Engine Age in the air or reach the Ascendant Age quickly.
5. **Energy everywhere.** Nearly all its late damage is Energy, so `adamant_weave` (+10%
   energy armour on infantry and cavalry) and energy-armoured hulls blunt it more
   than they blunt others.
6. **EMP and flak.** Its army is unusually Shielded, Mechanical and Flying, which is
   exactly what `emp_pulse` (x2.5 Mechanical, x2 Shielded, x1.5 Flying) and flak
   punish.

## 8. AI notes

| Field | Value | Meaning |
|---|---|---|
| `aggression` | 0.4 | a crater-holder, not a rusher: take the centre, then push with an elite ball |
| `army` | spearman 0.5, archer 0.5, zealot 0.8, lightbearer 1.0, halberdier 0.6, paladin 0.5, trebuchet 0.3, lumen knight 1.0, seraph 0.8, hover tank 0.7, orbital lance 0.4, archon 0.3 | a spear-and-bow guard in the Hearth Age; then a front line of zealots and halberds, lightbearers behind, paladins and seraphs as the mobile arm; trebuchets and later orbital lances to break walls; one Archon late |
| `eco_focus` | food, wood, aether, copper, stone, iron, silicon, adamant | aether third, ahead of every metal; then copper for `to_starfall` (150), monks, crossbows and arks; stone for the citadel (400), castle (650) and sanctum (450); then iron, silicon and adamant for the elite army and the late ages |

Build-order hints for whoever writes the AI:

- **Hearth.** Standard boom; a spearman/archer guard; go Banner early.
- **Banner.** One wayshrine toward the crater at once (drop site and trickle); one
  aerie and one or two kites as scouts, then Star Charts; zealots, crossbowmen and
  monks; a forge for the Starfall gate. Two or three workers on crater crystals.
- **Starfall.** Extractors on the vents, defended. Rite of the Lens, then a castle or
  a sanctum for paladins; one Reliquary Ark per army group; Aether Sails before any
  balloon raid.
- **Engine.** Lumen knights plus prism lightbearers on the ground, seraphs over them;
  an airfield for gyrocopters to raid the enemy's extractors. Meteor Pilgrimage.
- **Ascendant.** Starport (hover tanks, starfighters, star bombers against packed
  armies and fortresses); orbital lances to break the enemy fortress; the Archon when
  the adamant allows. The Star Answers.
- Keep flyers away from flak batteries; pull shielded units out of an EMP's 8 m ring.

## 9. Notes for the engine

- **Healers that fight.** `starborn_paladin` and `starborn_archon` have both weapons
  and a `healer`. They should heal while not attacking (or between swings); neither
  heals Mechanical units, so the Archon does not heal itself (it has `regen: 3`).
  Like every healer (design doc section 11) they mend one target at a time, lowest
  hp fraction first, and every `rate` here is priced as single-target healing.
- **The Rite of the Lens** replaces `crossbowman` with `starborn_lightbearer` in the
  archery range's menu, where the lightbearer already appears from the Starfall Age.
  The menu should show it once.
- **Flying infantry.** `starborn_star_kite` and `starborn_seraph` carry both Infantry
  and Flying (design rule 6: a winged soldier is still infantry). Infantry techs and
  Infantry bonuses apply to them; melee and Arc/Cannon/Bomb weapons cannot reach them.
  Both fly above every box in the game, as design rule 10 requires: the kite at 12 m,
  the seraphs at 12.5 m.
- **Hover giant.** `starborn_archon` is a Hover giant (like the Tidecrown Bucentaur);
  it crosses shallows, marsh and deep water at x0.8 and cannot enter thickets or
  cliffs.
- **Beam artillery.** `starborn_orbital_beam` is a Beam with a `min_range` and a 2 s
  windup. Drawing it from the emitter on the lance's back is fine; a later visual can
  bring it down from the sky onto the target.
- **Building shield from the Starfall Age.** `starborn_sanctum` has a `shield`
  (600, regen 12 after 6 s) and is tagged Shielded, so `deflector_shields` and
  `force_lattice` raise it.
- **Look boxes.** Every unique look sits at least partly outside its host. The
  Archon's halo (`starborn_judgement`) rests on top of its head; the star kite's
  censer hangs below the kite.
