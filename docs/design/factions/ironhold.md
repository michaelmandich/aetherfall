# The Ironhold Thanes (`ironhold`)

*"Mountain-hold clans who never give ground."*

Data: `assets/data/factions/ironhold.ron`. Shared rules and the common pool are in
[`../game-design.md`](../game-design.md). Validate with:

```
cargo run -q -p af_data --example check -- --only factions/ironhold.ron --summary
```

| Item | Value |
|---|---|
| Colour | cobalt blue `(0.16, 0.34, 0.86)`. Cobalt is a miner's ore and blue-and-white is the hold banner. It is far from the colours already taken: Dune amber, Steppe crimson, Starborn violet and Sylvan green. |
| Terrain trait | **Climber** for Foot: every Ironhold infantry unit, monk and worker scales cliffs (rocky highland, x0.6 speed) |
| Gather bonus | stone x1.25, iron x1.2, copper x1.15 |
| Worker | common `peasant` (Hearth), common `engineer` (Engine) |
| Start | 300 food, 300 wood, 150 stone, 100 copper; 5 peasants, 1 scout rider; one `town_center` |
| Roster | 34 trainable units: 26 from the common pool (2 workers + 24 others; 17 of 43 excluded = 40%) and 8 uniques |
| Buildings | 36: 32 common (4 excluded) and 4 unique |
| Techs | 55: 46 common (7 excluded) and 9 unique |
| AI | aggression 0.3 (turtle, then a late push) |

---

## 1. Identity and lore

The Thanes are the hill-folk of the high holds: Norse and Germanic mountain clans of
miners, smiths and engineers. Their halls are cut into the rock, and their thanes
swear on the anvil, not on a book. They are human, but in spirit they are the
dwarves of the old songs.

They were the last realm to look at the sky and the first to put the star to work.
When the star fell, the holds did what they have always done with anything that comes
out of the ground: they dug it up, cut runes into it and fed it to the forge. Aether
dust ground into rune-grooves guides a cannonball to its target or carries lightning
along an axe. Aether-fired bellows drive steam hammers, and steam drives the piston
armour of the Steamplate Thanes. In the Ascendant Age the holds beat adamant on a meteor they
call the Star Anvil. They build a walking mountain-forge, the Forgefather, and armour
their best warriors in plate that turns plasma.

Some things they never take from the star. They do not fly and they ride only hill
ponies, and no hold warrior wears a force field: shields are for stone and hulls.
Force walls, plasma turrets, the storm walker and the plasma artillery carry shields
(and `deflector_shields` strengthens those); the Runeguard and every other hold
soldier trust plate. Theirs is the "knights who found a
spaceship" theme with the knights still in charge: rune-axes and blunderbusses beside
steam walkers, under banners older than any of it.

## 2. Playstyle and win conditions

**Slow, unbreakable and long-ranged.** The Thanes are the best turtle in the game and
the best at sieges. Their infantry are slow (2.2-2.5 m/s) and very tough. Their walls
go up faster and stand longer than anyone's, and from the Starfall Age their guns
out-range everyone's.

| Phase | Plan |
|---|---|
| Hearth | Boom on stone and copper. Research **Stonewrights** early and wall the base with palisades and watchtowers; Stonewrights gives Thane walls a quarter more hp and extra crush armour. Delver Bands, spearmen and slingers hold the gaps. Send workers up cliffs to the highland ore. |
| Banner | Build two or three **Deep Delvings**: ore drop sites that also pay a small income costing no population. Raise a **Thanes' Hall** and field Thane Guard, pikes and crossbows. Turn the palisades to stone. Send workers to the crater with a mining camp on the rim. |
| Starfall | Claim aether vents and plant a **Rune Bastion** beside each extractor. A bastion is the Thanes' answer to "hold the centre". Field Hearthguard with halberdiers in front. **Rune Cannons** win the artillery duel. |
| Engine | **Steamplate Thanes** and rifle squads hold the line, steam landships and mech walkers add armour, and **Breachers** walk up to enemy walls. Bastions become **Thunder Bastions**. **Deep Bellows** covers the silicon shortfall. |
| Ascendant | **Runeguard** with the **Forgefather** behind them, storm walkers for anti-armour, and plasma artillery. **Unbreaking Holds** makes the home base and the crater forts close to permanent. |

**Win conditions**

1. **Fortified crater.** Take the vents with workers who climb the cliff rim, then
   hold them behind stone walls and bastions until the opponent runs out of aether.
2. **Siege grind.** Push slowly behind rune cannons and trebuchets, which out-range
   towers, then Breachers and the Forgefather, which break walls. The enemy has to come
   out and fight tough infantry on the Thanes' terms.
3. **Highland flank.** Climbing infantry cross cliffs that block every other ground
   army except flyers. They can raid a base from a plateau, or seize the ore on top
   and wall the ramps.

**Losing pattern.** Out-manoeuvred in the open. Cavalry, horse archers, hover craft
and flyers pick off slow columns and raid lightly guarded mines. The Thanes cannot
chase them; they have to make the enemy come to them.

## 3. Economy

| Lever | Detail |
|---|---|
| Gather bonus | stone x1.25, iron x1.2, copper x1.15. Stone feeds the walls and bastions, iron the heavy infantry, copper the early guns and siege. |
| **Deep Delving** (`ironhold_deep_mine`, Banner, limit 3) | 150 wood, 125 stone. First a drop site for stone, copper and iron. It also trickles 0.1 stone, 0.06 copper and 0.06 iron per second (0.22/s), which takes about 21 minutes to repay the building. Three of them (0.66/s) equal about one quarry worker and cost no population. |
| **Ore Lifts** (Banner) | 400 resources for a global +0.12 iron, +0.1 copper, +0.1 stone per second (0.32/s), also about 21 minutes to repay |
| **Banner trickle total** | about 1.0/s with three Delvings and Ore Lifts. That is still the largest Banner trickle in the game, about 1.4x the Tide's (0.7/s) and 1.6x the Steppe's (0.6/s). It pays off only in a long game, which suits a turtle. |
| **Deep Bellows** (Engine) | +0.2 silicon, +0.1 iron per second: quartz seams under the mountain. The Thanes have no sand trait, and their wheeled siege crawls on dunes, so this softens their Engine-age silicon squeeze. |
| Weak spots | **No aether or adamant bonus.** Like everyone, the Thanes must take the crater. Their advantage is holding it once they have it. `star_tithe` (temple) is the fallback. Food and wood are average. |
| Iron-built late army | Since 2026-10 the common Engine and Ascendant pool spends aether where it used to spend iron (framework 4.1 and 10.5). The Thanes' own late uniques do not: the Steamplate Thane, Breacher, Runeguard and Forgefather keep their iron-heavy prices, so the iron bonus, the Delvings and Ore Lifts still pay for the Thanes' endgame while the crater's aether goes further for everyone else. |
| Climber workers | Peasants and engineers walk straight up the cliffs to highland ore and over the crater rim, without using the ramps. |

Recommended AI economic order: food, wood, stone, iron, copper, then aether, silicon
and adamant as the ages demand them.

## 4. Terrain trait: Climber

The common terrain table gives `cliff` (rocky highland) a speed of x0.6, and only
Climbers and flyers can enter it. The Thanes grant Climber to Foot, so it covers
every infantry unit, the monk, both workers, the Steamplate Thane and the Runeguard.

- **Highland ore.** The generator puts ore on cliff tops reachable by ramps. Thane
  workers ignore the ramps, so they get to it first, and a wall across the ramps
  shuts everyone else out.
- **The crater rim.** The Starfall Crater is ringed by a cliff rim with three or four
  gaps. Other ground armies have to funnel through the gaps; Thane infantry come over
  the rim anywhere.
- **Flanks and escapes.** Infantry can retreat up a cliff where cavalry and tanks
  cannot follow. Ranged infantry (crossbows, arquebusiers, Hearthguard, Runeguard) on
  a plateau edge can only be answered by flyers, other climbers or artillery.
- **What does not climb:** siege, machines (Tracked), the scout rider (Mounted) and the
  Forgefather. Armies split at cliffs, so plan the meeting point.

## 5. Roster by age

Common units are in plain text, uniques in **bold**.

**Hearth Age** (5)

| id | role |
|---|---|
| `peasant` | worker; gathers all eight resources, climbs |
| `scout_rider` | the only horse the holds keep: scouting and picking off siege |
| `spearman` | anti-cavalry line |
| `slinger` | three-shepherd ranged squad; the Thanes' only Hearth missile troop |
| **`ironhold_delver_band`** | militia replacement: three sturdy miners; beat spearmen, dig out palisades |

**Banner Age** (8)

| id | role |
|---|---|
| `man_at_arms` | standard heavy swordsman |
| `pikeman` | anti-cavalry (`pike_drill` from spearmen) |
| `crossbowman` | anti-armour missile troop; shoots air; trained directly (no archers) |
| `monk` | healer; climbs with the infantry |
| `battering_ram` | gate-breaker |
| `mangonel` | anti-blob artillery |
| `ballista` | anti-siege |
| **`ironhold_thane_guard`** | elite, pop-efficient heavy axeman; cleaves |

**Starfall Age** (8)

| id | role |
|---|---|
| `halberdier` | anti-cavalry, anti-giant, anti-clockwork (`halberd_drill`) |
| `arquebusier` | heavy missile troop, trained directly |
| `grenadier` | anti-blob, burns timber |
| `clockwork_automaton` | arrow-proof brass brawler |
| `bombard` | the wall-breaker |
| `trebuchet` | out-ranges every tower |
| **`ironhold_hearthguard`** | blunderbuss pair: a short-range cone that shreds infantry |
| **`ironhold_rune_cannon`** | guided field artillery: counter-battery and anti-machine at 18 m |

**Engine Age** (9)

| id | role |
|---|---|
| `engineer` | late worker, fast repair |
| `rifle_squad` | long-range backbone; shoots air |
| `gatling_crew` | anti-infantry, anti-air |
| `steam_tank` | armour |
| `mech_walker` | armour that shoots air |
| `rocket_battery` | long-range siege |
| `flak_wagon` | the main anti-air (the Thanes have no fighters) |
| **`ironhold_ironclad`** (Steamplate Thane) | steam power armour: a super-heavy melee anchor that climbs |
| **`ironhold_breacher`** | walking siege drill that bores through walls |

**Ascendant Age** (4)

| id | role |
|---|---|
| `plasma_artillery` | fortress-breaker |
| `storm_walker` | anti-armour and anti-giant; tesla hits air |
| **`ironhold_runeguard`** | adamant-plated lightning infantry; the bolt jumps to one more target; hits air |
| **`ironhold_forgefather`** | titan walker: line-piercing forge cannon and quake hammer; cannot hit air |

### 5.1 Unique units

dps is per model, before armour. `dps/cost` and `ehp/cost` count the whole squad,
using the method of section 8 of the framework. The bands are the common pool's
min-max and median for that age.

| id | age | squad | pop | cost | hp | armour % | speed | weapon (kind) | dps | range | dps/cost (band, median) | ehp/cost (band, median) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `ironhold_delver_band` | 1 | 3 | 1 | 115F 30W 15Cu | 42 | S10 P10 C5 | 2.5 | `ironhold_mattock` (Melee Smash, Building x1.5) | 3.3 | 0.7 | 0.062 (0.048-0.067, 0.057) | 0.79 (0.60-0.94, 0.73) |
| `ironhold_thane_guard` | 2 | 1 | 1 | 65F 30Cu 60Fe | 125 | S35 P30 C15 F5 | 2.4 | `ironhold_dane_axe` (Melee Swing 80 deg, cleave 1) | 10.0 | 0.9 | 0.065 (0.044-0.067, 0.054) | 0.81 (0.58-0.90, 0.76) |
| `ironhold_hearthguard` | 3 | 2 | 1 | 120F 40W 90Fe 30Ae | 115 | S20 P25 C10 F15 | 2.4 | `ironhold_blunderbuss` (**Spray** 35 deg, Infantry x1.2) | 10.8 per target in the cone (13.0 with Scattershot) | 5 | 0.077 (0.059-0.082, 0.067) | 0.82 (0.66-1.19, 0.81) |
| `ironhold_rune_cannon` | 3 | 1 | 3 | 120W 80Cu 140Fe 40Ae | 280 | S30 P65 C10 F10 | 1.4 | `ironhold_rune_shot` (**Homing**, Siege x1.75, Mech x1.3, Building x2) | 11.4 | 18 (min 4) | 0.030 (siege; bombard 0.043) | 0.74 (bombard 0.69) |
| `ironhold_ironclad` (Steamplate Thane) | 4 | 1 | 2 | 150F 40Cu 180Fe 70Si | 400 | S45 P45 C25 F30 E10 | 2.2 | `ironhold_piston_hammer` (Melee Smash, splash 1, Mech x1.5) | 29.4 | 1.0 | 0.067 (0.042-0.094, 0.074) | 0.91 (0.67-1.43, 0.95) |
| `ironhold_breacher` | 4 | 1 | 4 | 100W 80Cu 320Fe 120Si | 850 | S65 P65 C30 F35 E10 | 2.2 | `ironhold_breaching_drill` (Melee Thrust, Building x3.5) | 25 (87.5 vs buildings) | 0.8 | 0.040 (siege) | 1.37 (siege) |
| `ironhold_runeguard` | 5 | 1 | 1 | 140F 120Fe 90Ae 50Ad | 480 | S40 P40 C25 F25 E30 | 2.5 | `ironhold_thunder_rune` (**Chain**, 1 jump of 3 m, cooldown 1.8, hits air) | 25 (50 when the bolt jumps) | 8 | 0.063 (0.058-0.099, 0.095) | 1.20 (1.11-1.89, 1.45) |
| `ironhold_forgefather` | 5 | 1 | 12 | 300S 800Fe 300Si 500Ae 700Ad | 6400 | S70 P70 C55 F55 E35 | 1.8 | `ironhold_forge_cannon` (Cannon, pierce 2, no burst, Building x3) + `ironhold_quake_hammer` (**Nova**, 4 m) | 48.9 + 26.7 | 17 / 4 | 0.029 (titan 0.039) | 2.46 (titan 2.31) |

**Notes on value**

- Every unique is inside its age's bands. Each one that sits above a median pays for
  it with speed, range or a hard counter:
  - Delvers, Thane Guard and Hearthguard are slower than the common line.
  - Hearthguard have 5 m range where other missile troops have 8-12.
  - The Breacher is melee only.
  - The Forgefather cannot hit flyers.
- **Thane Guard vs Man-at-Arms:** the guard edges them at equal cost (1.11 on the
  stat-only score) and needs one pop where men-at-arms need about 1.3 for the same
  strength. Its cleave and armour are worth more in a packed line than in a
  duel.
- **Runeguard vs Shield Trooper:** the Runeguard loses a single-target duel (0.58)
  and wins once its bolt has a second model to jump to (1.16). Its single-target
  dps/cost sits on the band floor (0.063, the median divided by 1.5); the jump
  doubles it against squads and packed lines, and it hits air. It is an anti-squad,
  anti-drone line unit, not a duelist.
- **Forgefather vs Titan:** the Forgefather loses the duel (0.62) and has a quarter
  less single-target dps/cost. In return it has more hp and fire and crush armour,
  its slug drives through two models in a line, and it matches the titan against
  buildings (187 raw dps against 189, Building x3 on the cannon). Its quake hammer
  uses the titan stomp's damage and 4 m radius. It cannot shoot air, and that is the
  main way to beat it.
- **Weapon kinds rare in the common pool:**
  - Spray: Hearthguard. The pool's only other Spray is the landship flamer.
  - Chain: Runeguard (1 jump) and the Thunder Bastion (4 jumps). The pool's only
    other Chain is the tesla coil.
  - Homing: Rune Cannon and the Rune Bastion.
  - Nova: the Forgefather's hammer.
  - Bomb: the bastions' murder holes, the only Bomb in the game dropped from a
    building. The Thanes never fly, so their Bomb falls from the parapet.
  - The Thane Guard is the pool's second cleaving swing, after the halberd.

## 6. Unique buildings

| id | age | cost | build s | hp | armour % | role |
|---|---|---|---|---|---|---|
| `ironhold_mead_hall` (Thanes' Hall) | 2 | 200W 150S 50Cu | 65 | 2200 | S65 P85 C15 F25 E25 | trains Thane Guard, Hearthguard, Steamplate Thane, Runeguard; researches Oathsworn, Scattershot, Steam-Forged Plate, Star Anvil |
| `ironhold_deep_mine` (Deep Delving) | 2 | 150W 125S | 60 | 1300 | S65 P85 C15 F40 E25 | drop site for stone, copper and iron; trickles 0.1 stone, 0.06 copper, 0.06 iron per second; limit 3; researches Ore Lifts and Deep Bellows |
| `ironhold_rune_bastion` (Rune Bastion) | 3 | 450S 120Fe 40Ae | 120 | 3200 | S70 P85 C25 F35 E25 | 6 x 7 x 6 wall-line strongpoint; `ironhold_bastion_rune_shot` (Homing, 13 m, Siege x1.75, 14 dps) + `ironhold_bastion_bolts` (3 bolts, 12 m, hits air, 14.4 dps) + `ironhold_murder_holes` (**Bomb**, 1.5 m, 60 Crush every 4 s, splash 1.5, Siege x2, Infantry x1.2: 15 dps, 30 against rams and Breachers); garrison 10; upgrades to Thunder Bastion (150Fe 100Si 100Ae, 60 s) |
| `ironhold_thunder_bastion` (Thunder Bastion) | 4 | 450S 270Fe 100Si 140Ae | 150 | 4500 | S75 P85 C30 F45 E35 | the same footprint and height with a rune-spire on the roof: rune shot + `ironhold_thunder_runes` (Chain, 4 jumps, 12 m, hits air, 18 dps per target) + the same murder holes |

- **The bastion beside the tower chain.** Both carry the `Tower` tag, so
  `ferroconcrete`, `ballistics`, `targeting_computers`, Runic Gunnery and Unbreaking
  Holds all apply. The bastion is bigger, costs more stone, and has no gun-turret or
  plasma step. Its Engine form is the chain-lightning anti-blob, anti-air tower. Use
  bastions at the crater extractors and at gates, and the five-tier tower chain along
  the wall.
- The bastion keeps its 6 x 6 footprint and its 7 m height through the upgrade, so
  every roof weapon sits on the same roof.
- **Murder holes** are a thing that defends walls: rocks dropped through the
  parapet onto whatever stands at the bastion's foot. They reach only 1.5 m from the
  bastion's edge, so they punish rams, Breachers and sappers working on the bastion
  or the wall segment beside it, and nothing further out. They hit ground units
  only, not buildings or flyers, and do not hurt friendly units.
- The Thanes' Hall is Banner-age, so it fills the gap before the castle (Starfall).
  The castle also trains Hearthguard, along with the common grenadier and trebuchet.

## 7. Unique techs

| id | age | at | cost | time s | requires | effects |
|---|---|---|---|---|---|---|
| `ironhold_stonewrights` | 1 | `mining_camp` | 100F 100W 50S | 30 | - | walls build time x0.7, towers x0.75; walls hp x1.25 and crush armour +0.1 |
| `ironhold_ore_lifts` | 2 | `ironhold_deep_mine` | 200F 150W 50Cu | 45 | - | trickle +0.12 iron, +0.1 copper, +0.1 stone /s |
| `ironhold_oathsworn` | 2 | `ironhold_mead_hall` | 200F 100Fe | 45 | - | Thane Guard hp x1.2, pierce armour +0.05; Delver Band hp x1.2 |
| `ironhold_runic_gunnery` | 3 | `siege_workshop` | 200W 150Fe 60Ae | 50 | `ballistics` | Siege damage x1.1, Tower damage x1.1, Rune Cannon range +1 |
| `ironhold_scattershot` | 3 | `ironhold_mead_hall` | 250F 100Fe 30Ae | 45 | - | Hearthguard damage x1.2 |
| `ironhold_steam_forged_plate` | 4 | `ironhold_mead_hall` | 300F 250Fe 75Si | 55 | - | Steamplate Thane hp x1.15, speed x1.1; Thane Guard and Hearthguard crush armour +0.1 |
| `ironhold_deep_bellows` | 4 | `ironhold_deep_mine` | 300F 100Cu 150Fe | 55 | `ironhold_ore_lifts` | trickle +0.2 silicon, +0.1 iron /s |
| **`ironhold_unbreaking_holds`** (signature) | 5 | `castle` | 600S 300Fe 250Ae 150Ad | 80 | `ironhold_stonewrights` | all buildings hp x1.15 and energy armour +0.1; walls and towers regen +4 hp/s |
| `ironhold_star_anvil` | 5 | `ironhold_mead_hall` | 300F 200Fe 200Ae 100Ad | 70 | - | Runeguard hp x1.15, energy armour +0.1; Steamplate Thane energy armour +0.15; Forgefather regen +4 |

- **Stonewrights makes Thane walls the best from the Hearth Age on.** With +25% hp
  and +0.1 crush armour, a siege engine needs about 1.4x as long to bring down a
  Thane palisade, stone wall or reinforced wall (stone wall: 1875 hp at 25% crush
  armour instead of 1500 at 15%). Everyone else who takes the common wall chain gets
  only the shared masonry, architecture, ferroconcrete and force lattice, which the
  Thanes take as well.
- **Scattershot is damage only.** Under the framework's rule a `Stat Range` tech
  does not lengthen a Spray cone, so a +1 range would have let the Hearthguard
  acquire targets its 5 m cone cannot reach. The tech gives +20% damage instead.
- **No `UpgradeUnit` among the uniques.** Each unique pair would break rule 3: the
  Thane Guard costs 155 against the Steamplate Thane's 440 and needs a different pop, and the
  Hearthguard is a squad. So, following rule 3, every elite is trained only, and the
  older units get stat drills instead: Oathsworn, Scattershot and Steam-Forged Plate.
  These keep Thane Guard and Hearthguard useful into the Engine Age.
- The common line upgrades kept are `pike_drill` (spearman -> pikeman) and
  `halberd_drill` (pikeman -> halberdier). There are no archers, so there is no
  `crossbow_drill`. `hand_cannon_drill` requires `crossbow_drill`, so it is excluded
  too: crossbowmen and arquebusiers are trained side by side.
- **Unbreaking Holds** is researched at the castle. It is the one exception to the
  "castle trains, others research" habit, and it ties the signature tech to the keep.
  With `force_lattice` and `masonry`/`architecture` it pushes Thane walls far past
  anyone else's. Engine note: walls and towers would regenerate +6 hp/s, and the
  framework already clamps armour at 0.9.

## 8. What is excluded and why

**Common units excluded (17 of 43, 40%)**

| Excluded | Why |
|---|---|
| `militia` | replaced by the Delver Band |
| `archer` | the holds never took to the bow; Hearth missiles are slings, then crossbows |
| `javelineer` | a light, fast skirmisher; nothing about the Thanes is light or fast |
| `knight`, `horse_archer`, `aether_lancer` | **weak cavalry**: hill ponies only. Only the scout rider remains, and it is trained at the town centre, so there is no stable. |
| `shock_trooper` | replaced by the Steamplate Thane, the Thanes' own steam plate |
| `war_balloon`, `gyrocopter`, `biplane_bomber` | **no flyers in any age.** The Thanes never take to the air; air is answered from the ground (crossbows, rifles, gatlings, flak, mech walkers, towers, the Thunder Bastion and the Runeguard). |
| `gunship`, `starfighter`, `star_bomber`, `drone_swarm`, `hover_tank` | no star-craft and no hover (all but the hover tank come from the starport, which they do not build). The Ascendant Age is the Thanes' plate, lightning and the Forgefather; their big blasts come from plasma artillery, not from the air. |
| `shield_trooper` | no hold warrior wears a force field: replaced by the Runeguard |
| `titan` | replaced by their own titan, the Forgefather |

**Common buildings excluded (4 of 36)**: `stable` (no cavalry to train), `airfield`
and `starport` (no flyers or star-craft), `shield_generator` (no force-field domes;
the Thunder Bastion and Unbreaking Holds are their interior defence instead).

**Common techs excluded (7 of 53)**:
- `militia_drill`: there is no militia.
- `crossbow_drill`: there are no archers.
- `hand_cannon_drill`: it requires `crossbow_drill`.
- `husbandry`, `scale_barding`, `plate_barding`: weak cavalry by design; scouts stay
  unarmoured.
- `fusion_cores`: it is researched at the starport.

**What the Thanes kept that others may not:** the full wall and gate chains to force
walls, the full five-tier tower chain, every common siege engine, the castle,
clockwork automatons, steam landships, mech walkers, storm walkers and plasma
artillery.

## 9. Counters and weaknesses

Stat-only Lanchester scores at equal cost, using the framework's method (section
5.5); above 1 means the first unit wins. Range, kiting and micro are ignored.

| A | B | A's score | A dps into B | B dps into A |
|---|---|---|---|---|
| `ironhold_delver_band` | `spearman` | 1.34 | 3.3 | 3.6 |
| `ironhold_delver_band` | `militia` | 1.02 | 3.3 | 3.2 |
| `slinger` | `ironhold_delver_band` | 0.98 | 2.5 | 3.3 |
| `scout_rider` | `ironhold_delver_band` | 0.82 | 3.5 | 3.3 |
| `ironhold_thane_guard` | `pikeman` | 1.48 | 8.5 | 3.7 |
| `ironhold_thane_guard` | `man_at_arms` | 1.11 | 7.0 | 5.4 |
| `ironhold_thane_guard` | `knight` | 1.00 | 6.5 | 7.5 |
| `crossbowman` | `ironhold_thane_guard` | 0.60 | 6.1 | 9.5 |
| `ironhold_hearthguard` | `halberdier` | 1.17 | 9.8 | 9.1 |
| `ironhold_hearthguard` | `arquebusier` | 1.24 | 11.0 | 12.8 |
| `aether_lancer` | `ironhold_hearthguard` | 1.32 | 17.5 | 7.0 |
| `ironhold_rune_cannon` | `bombard` | 0.97 | 20.0 | 18.7 |
| `ironhold_ironclad` | `shock_trooper` | 1.00 | 23.5 | 18.2 |
| `ironhold_ironclad` | `rifle_squad` | 0.87 | 29.4 | 11.1 |
| `ironhold_ironclad` | `gatling_crew` | 1.16 | 29.4 | 20.6 |
| `ironhold_ironclad` | `halberdier` | 1.63 | 26.5 | 6.3 |
| `steam_tank` | `ironhold_ironclad` | 1.18 | 31.4 | 28.7 |
| `aether_lancer` | `ironhold_ironclad` | 1.01 | 15.8 | 23.5 |
| `shock_trooper` | `ironhold_breacher` | 2.05 | 34.0 | 20.0 |
| `steam_tank` | `ironhold_breacher` | 1.47 | 29.7 | 20.3 |
| `ironhold_breacher` | `rifle_squad` | 1.25 | 25.0 | 7.1 |
| `ironhold_runeguard` | `shield_trooper` | 0.58 (1.16 with a jump) | 18.8 | 31.6 |
| `ironhold_runeguard` | `tide_phaser_marine` | 0.65 (1.30 with a jump) | 17.5 | 16.0 |
| `hover_tank` | `ironhold_runeguard` | 1.32 | 30.3 | 21.0 |
| `storm_walker` | `ironhold_runeguard` | 1.22 | 41.8 | 24.0 |
| `ironhold_forgefather` | `shield_trooper` | 1.23 | 65.8 | 16.5 |
| `ironhold_forgefather` | `titan` | 0.62 | 59.2 | 101.2 |
| `storm_walker` | `ironhold_forgefather` | 1.70 | 43.8 | 57.1 |
| `hover_tank` | `ironhold_forgefather` | 2.67 | 52.8 | 57.1 |

Notes:
- **Hearthguard** figures are per target. The cone hits every model in 35 degrees at
  5 m, so against a blob they are worth two or three times the listed value. Against
  anything that keeps out of 5 m they are worth nothing.
- Against packed infantry or squads, the **Runeguard**'s bolt jumps to a second
  model within 3 m, which doubles its output. The "with a jump" scores count that.
- The **Forgefather**'s slug drives through two models in a line and its 4 m hammer
  hits everything around it, so against packed infantry it does better than the
  single-target scores above. It has no burst at the end of the slug's path.
- **Steamplate Thane** scores use its own armour once: B's dps into it is already
  after its 45% slash and pierce armour, and hp counts plain hp. It loses to rifles
  (0.87) and landships (0.85) and beats gatlings stat-only (1.16), but gatling crews
  get their 10 m of free fire as it closes, so they still wear it down in play.

**How to beat the Thanes**

| Weakness | Exploit |
|---|---|
| Slow armies (2.2-2.5 m/s infantry, 1.4 siege) | kite with horse archers, hover tanks and gunships; raid elsewhere while they march |
| No cavalry to chase with | raid Deep Delvings, outlying farms and crater miners with fast units |
| No air force | flyers are only ever shot at, never challenged. Bombers and gunships force the Thanes to buy flak; a Forgefather with no air cover dies to gunships. |
| Short-range Hearthguard and Runeguard | out-range them with rifles, arquebuses, artillery and shield troopers |
| Heavy infantry is still infantry | gatlings (x1.5 vs Infantry), crossbows and arquebuses (x1.25-1.3 vs Heavy), rifles (x1.35 vs Heavy), aether and laser beams (low energy armour before the Star Anvil) |
| Melee-only Breacher | screen it out with halberds and shock mauls, or kill it from range with ballistae and cannon |
| Wheeled siege on dunes | rune cannons and bombards crawl at x0.7 on sand; fight them on dunes |
| No aether or adamant bonus | contest the crater early, before the bastions go up |

**What the Thanes beat**

| Their strength | Victim |
|---|---|
| Fortified lines | any army that has to come to them: infantry blobs into Hearthguard and grenadiers, siege into Rune Cannons and bastions |
| Artillery reach | towers and static defences, out-ranged by trebuchets (22 m) and rune cannons (18 m) |
| Climbing | ground armies relying on cliff chokepoints, and crater defences that only cover the rim gaps |
| Toughness | early pressure: by the time the enemy arrives, Stonewrights walls and watchtowers are already up |

## 10. AI notes

| Field | Value | Reason |
|---|---|---|
| `aggression` | 0.3 | Turtle: wall early, boom, then push in the Engine and Ascendant ages |
| `army` | Delver Band 0.6, spearman 0.4, slinger 0.4, Thane Guard 0.8, crossbowman 0.5, Hearthguard 0.9, Rune Cannon 0.5, rifle squad 0.7, Steamplate Thane 1.0, Runeguard 1.0, Forgefather 0.3 | Every age has a melee anchor and a missile troop, and from the Starfall Age artillery too; the Hearth entries give the turtle defenders while it walls up. The AI keeps one Forgefather, not a stack. |
| `eco_focus` | food, wood, stone, iron, copper, aether, silicon, adamant | wood second, because houses, camps, farms and the early buildings all need it; then stone and iron ride the gather bonus |

Behaviour hints for whoever writes the AI:

1. **Walls.** Research `ironhold_stonewrights` before the first wall. Wall the base
   with palisades in the Hearth Age and upgrade the line to stone in the Banner Age.
2. **Deep Delvings.** Build all three by the late Banner Age, near the town centre and
   inside the walls.
3. **Bastions.** Place a Rune Bastion next to each `aether_extractor` and at main
   gates. Never send the army out of bastion range on defence.
4. **Escorts and anti-air.** Pair every artillery group (rune cannons, trebuchets,
   bombards) with a melee screen. Build flak wagons as soon as the enemy shows flyers;
   the Thanes have no other air answer that moves.
5. **Climbing routes.** Use cliff paths for infantry raids and crater approaches; the
   pathfinder must grant Climber to Foot units for this faction.
6. **Forgefather escort.** Escort the Forgefather with flak wagons, gatlings or
   Runeguard. On its own it is helpless against air.

## 11. Notes for the engine and the lead

- **Chain jumps.** The `Chain` jumps on the Runeguard (1) and Thunder Bastion (4)
  are assumed to deal full damage per jump, the same as the framework's tesla coil.
  If the engine adds falloff, raise `ironhold_thunder_rune` damage by about 10%.
- **Bombs from a building.** `ironhold_murder_holes` is a `Bomb` on a building, and
  schema.rs describes Bomb as dropped from a flyer at its altitude. For a building
  the engine should use the weapon look's `offset` y (7.1 m, the parapet) as the
  drop height and drop onto the ground point under the target, which is at most
  1.5 m from the bastion's edge. `fall_time` 0.5 s is set for that short fall.
- **Spray damage.** Spray damage is per target in the cone (as for `flame_projector`),
  not per particle. `particles: 8` is visual. No Thane tech changes the blunderbuss
  range, since `Stat Range` would not lengthen the cone.
- **Homing min range.** `ironhold_rune_shot` uses `min_range` 4 with a Homing kind,
  so the engine should honour `min_range` for every projectile kind, not just Arc and
  Cannon.
- **Deep Delving placement.** It needs no node and sits on open buildable ground. Its
  trickle is the "mine into the mountain" fantasy without a new node type.
- **Unit looks.** Every unique weapon look sits partly outside its host box, and roof
  weapons sit on the 7 m bastion roof (audited the same way as the lead's
  `lookcheck.py`).
