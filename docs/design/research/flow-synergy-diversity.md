# Flow, synergies and faction diversity

Research proposal, 22 October 2026. Nothing in this document has been applied to the game.

It brings together five research audits:

- game flow;
- synergies and counters;
- faction diversity;
- ideas from other RTS games;
- data from AI-versus-AI matches.

Each audit's claims were checked against the files at commit `1cda6dc`:

- `assets/data/core/*.ron` and `assets/data/factions/*.ron`;
- `crates/af_ai` and `crates/af_core`;
- the map contract in `docs/design/maps-and-terrain.md`.

Where the audits disagreed, the files decided. Those calls are marked **Resolved**.

**Evidence used**

- **Sweep A**: 45 headless 1v1s, every pairing of factions on seeds 7, 11 and 23, with a 42-minute cap. It used the release build of 2026-10-19 and the current data. The command was `aetherfall --headless --players 2 --factions A,B --minutes 42 --report-every 60 --seed N`.
- **Sweep B**: 45 more 1v1s on seeds 11-75, with a 40-minute cap. These ran through a harness that reads the game's own event messages. The same harness also ran:
  - 6 mirror matches;
  - 5 team and free-for-all games;
  - 3 peaceful six-player controls, with everyone on one team;
  - three data experiments that each changed a single file: `xp_food`, `xp_ram` and `xp_star`.
- **Combat scores** come in two forms:
  - "stat" is the stat-only Lanchester score from game-design.md 5.5;
  - "duel" is a range-aware duel at equal cost. The shorter-ranged side walks in and nobody kites. A positive score means the first-named unit wins.

  Spray, chain and splash weapons count only one target in both, so they are undercounted.
- The raw logs and scripts are outside the repo, in the research session's scratchpad.

All win records are AI against AI. Section 6 explains why they measure the early game more than the factions' designs.

---

## 1. Executive summary

The combat, the age arc and the faction fantasies are sound. The content is rich: 95 units and 105 techs across five ages. The trouble is that most of it is never played.

These are the seven most important problems, in order:

1. **Food is the only resource that matters.**
   - Almost everything costs food: workers, most soldiers, 56% of the common Banner tech prices, and 2,500 food on the age-up path.
   - Meanwhile copper, stone, silicon, aether and adamant pile up unspent.
   - In both sweeps, food was what held up the next age in nearly every long wait.
   - The three factions with a food bonus (Sylvan, Tide, Steppe) went 59-24. The three without one went 18-53.
   - Their bonuses on stone, iron, aether and adamant only add to stockpiles nobody can spend.
2. **The late game rarely happens.**
   - The median 1v1 ends at about 27 minutes.
   - In 53 of 77 decided games, the loser was still in the Starfall Age.
   - Only 46% of players reached the Engine Age, and 11% reached the Ascendant.
   - Across both sweeps, not one hover tank, gunship or storm walker was fielded, and only one faction giant appeared.
3. **The town centre is the bottleneck of the age arc.**
   - It researches every age-up and the citadel upgrade. That takes 545 seconds of the first 26 minutes, and it trains no workers while it does.
   - The Starfall Age adds the citadel, the foundry, the first extractor and 15 techs on top. It is the longest age.
4. **The AI plays half the game.**
   - It never finishes a mill, so the food techs never come.
   - It never builds castles, temples, engine works, airfields, starports or the factions' own economy buildings.
   - It researches almost none of the 52 unique techs.
   - Late in the game, it fills the population cap with Hearth and Banner units.

   Until this is fixed, AI games cannot tell us whether a faction is balanced.
5. **The six factions are one faction with six skins.**
   - They share one worker, one start, one age path (Sylvan's own Engine age-up, `sylvan_wake_the_wildwood`, aside) and one set of building and unit roles.
   - Twenty of the 52 unique techs are "+x% to our named unit".
   - Several promises have nothing behind them in the rules: Dune's fast riders, Tide's navy and Steppe's "every herder a rider".
   - Two more exist but are not distinctive: Starborn's healers (five of the six factions heal) and Ironhold's best walls (`ironhold_stonewrights` and `ironhold_unbreaking_holds` buff the common wall chain that Dune, Tide and Starborn also build).
6. **Some counters the design documents promise do not exist in the numbers.**
   - Fire does not burn beasts.
   - Nothing beats Banner heavy foot.
   - The aether lancer loses its edge over arquebusiers once range counts.
   - No melee unit beats a landship.
   - Shields have no counter that can move.
7. **The map offers nothing to fight over between home and the crater, and defenders have no tools.**
   - Garrisoning is written into the data but not built.
   - Expansion is all or nothing: you either hold your home or walk to the centre.

**Recommended direction**

1. **Unblock the economy.** Take food off the late prices and the age-ups. Fix the AI's mill, farm building and research budget. Both halves are needed: the `xp_food` experiment re-priced only the age-ups. The Banner and Starfall ages came about 40 seconds earlier and the Engine Age 77 seconds earlier, but food stayed short and Sylvan went 15-0.
2. **Shorten the age arc by freeing the town centre.**
   - Research the Engine and Ascendant age-ups at the building that already gates them (`aether_foundry`, `laboratory`), and `to_starfall` at the Oath buildings once the Oaths land (F2, section 4.2).
   - Make the citadel an optional fortress.
   - Cut research times to 90, 90, 100 and 120 seconds.
3. **Make factions play differently, using systems the engine already runs:**
   - a choice of two "Starfall Oaths" per faction: age-ups that each carry a bonus, like Age of Mythology's minor gods;
   - economy buildings that must sit on a faction node: Tide harbours on fishing shoals, Starborn shrines on star shards, and Ironhold mines on deep seams;
   - starts and age paths that differ by faction.

   After that, add three engine features: garrisons, auras, and a limit of one giant per player.
4. **Patch the counter web** with about a dozen data edits.
5. **Use the map overhaul to add a middle game:**
   - a forward resource pocket for each player;
   - meteor sites that differ in what they hold;
   - the faction nodes.
6. **Only then rebalance faction strength,** using the headless sweep as a regression test (F9).

The quick wins are in section 7, Tier 1, and are data-only.

---

## 2. Game flow

### 2.1 Intended against measured

| | Banner | Starfall | Engine | Ascendant | Match end |
|---|---|---|---|---|---|
| Doc target, competent player (game-design.md 4.5) | 5-7 | 11-14 | 18-22 | 24-28 | not set |
| Doc one-second model | 6:35 | 13:10 | 20:25 | 25:45 | - |
| Doc target, computer players | about 6 | 13-15 | 20-24 | 28-34 | - |
| AI, peaceful six-player controls (sweep B) | about 5:30 | about 14:00 | 21:30-22:30 | 28:30-32:30 | - |
| AI 1v1, sweep A (median; players who got there, of 90) | 6:00 (90) | 15:00 (83) | 26:00 (44) | 37:30 (10) | median 27:24; 38 of 45 decided |
| AI 1v1, sweep B | 5:42 | 14:32 | 24:59 (38) | 9 of 90 got there | median 26:28; 39 of 45 decided |

What this shows:

- **Without war, the AI is close to the targets.** It reaches the Engine Age at about 22 minutes and the Ascendant at about 30.
- **With war, most games end before the late ages.** Each AI age lasts 9-11.5 minutes, against 5-7 in the model.
- **The Banner Age is the longest wait.** In sweep B it lasted 8:50, from 5:42 to 14:32. The AI spent 70-73% of its Banner reports saving for `to_starfall`.
- **Losers die early.** In 53 of the 77 decided games, the loser was still in the Starfall Age. All 7 games in sweep A that ended in the Banner Age were won by Steppe or Sylvan.
- **Siege is not the cause.** Experiment `xp_ram` cut the ram's building bonus from x4 to x2.5, and match length barely moved: the median went from 26:28 to 27:09.

### 2.2 Where the time goes

**The town centre researches everything.**
- `to_banner` (90 s) and `to_starfall` (110 s) are researched at `town_center`.
- `to_engine` (120 s) and `to_ascendant` (150 s) are researched at `citadel`.
- The citadel comes from a 75-second town-centre upgrade costing 150W 200S 100Fe 75Ae (`buildings.ron`).

Together, these lock the only worker producer for 545 seconds in the first 26 minutes. That is enough time to train 39 peasants at 14 seconds each.

Doc section 3.1 explains why it was built this way: a tech has only one `researched_at` building, so if the town centre researched every age-up, upgrading it to a citadel would strand the player.

**The Starfall Age is overloaded.** It has to pay for:
- the citadel upgrade (525 resources);
- the `aether_foundry` (450), which `to_engine` requires;
- the first `aether_extractor` (350);
- 15 common techs worth 7,090 in total, of which 3,125 is food;
- `to_engine` itself (1,250).

The AI's citadels finished between 18:00 and 25:00. That is 3-10 minutes after the AI reached the Starfall Age.

**Food is a tax on the whole arc.**

| | Today | Proposed (F2 + F3) |
|---|---|---|
| Mandatory age path (age-ups, forge, citadel upgrade, foundry, laboratory) | 6,225 resources | 5,150 |
| of which food | 2,500 (93 farmer-minutes at 0.45/s) | 1,550 (57 farmer-minutes) |
| Age-up research time | 470 s | 400 s |
| Town-centre time locked by the path | 545 s | 180 s (`to_banner` and `to_starfall`, which F2 keeps at the town centre until the Oaths land); 90 s after that |

Common techs (age-ups aside) charge a large share in food (`techs.ron`):

| Age | Food share of common tech prices |
|---|---|
| Hearth | 56% |
| Banner | 56% (1,700 of 3,025) |
| Starfall | 44% |
| Engine | 31% |

On top of that, every peasant costs 50 food, and so do most soldiers.

What the sweeps measured:
- **Sweep A:** food was the scarcest age-up resource in 109 of 117 long waits.
- **Sweep B:** while saving for `to_banner`, `to_starfall` or `to_ascendant`, food was below the age-up price in 96-100% of samples. While saving for `to_engine`, it was below in 50-72%.
- **What piled up instead:**
  - Banner Age: copper 156-255, stone 99-180, aether 76-115.
  - Starfall Age: stone 151-226, silicon 109-225.
  - Engine Age: silicon 243-602, aether 338-624, adamant 122-530.

**Part of this is the AI's fault** (section 6.2):
- It never completes a mill. As a result, `horse_collar`, `heavy_plow`, `crop_rotation`, `wheelbarrow`, `handcart` and `steam_harvesters` are never bought.
- It allows only as many food gatherers as it has farms. `fields()` in `crates/af_ai/src/think.rs` builds at most two farms at a time, and only once every existing farm is worked.

**Map size now moves the clock.**
- The map contract adds sizes from 176 m to 360 m for two players.
- Start crystals sit `to_center - crater_outer - 5` metres from the town centre, clamped to 20-55 m (`crates/af_map/src/mapgen/starts.rs`).
- So the Banner Age's walk to the aether, and with it the `to_starfall` clock, will change with map size, unless the crystals sit at a fixed distance.

### 2.3 Proposals

**F1. Set the targets.** Add a match-length target to game-design.md 4.5. Suggested:

- A 1v1 between competent players is usually decided in the Engine Age, at 25-35 minutes.
- The Ascendant Age is reached in about a third of 1v1s and in most team games.

Suggested reach targets, in minutes:

| | Banner | Starfall | Engine | Ascendant |
|---|---|---|---|---|
| Competent player | 5-6 | 11-13 | 17-20 | 23-26 |
| AI, peaceful controls | about 6 | about 13 | about 20 | about 27 |

**F2. Free the town centre.** This is a data-only change:

- `age_up_step` in `crates/af_ai/src/progress.rs:66-98` already follows `researched_at` and `requires_buildings` for any tech, so the AI adapts with no new code.
- The command card in `af_ui` pins one age-up per building, so age-ups at different buildings all show.

| Tech | Researched at | Requires buildings | Time |
|---|---|---|---|
| `to_starfall` | stays at `town_center` until the Oaths land (section 4.2); then each faction gets two Oaths at two different buildings, and the common tech stays only as the default | `forge` | 110 → 90 s |
| `to_engine` | `citadel` → `aether_foundry` | `[aether_foundry]` → `[]` | 120 → 100 s |
| `to_ascendant` | `citadel` → `laboratory` | `[laboratory]` → `[]` | 150 → 120 s |
| `sylvan_wake_the_wildwood` | `citadel` → `sylvan_heart_grove` | `[sylvan_heart_grove]` → `[]` | 120 → 100 s |

What changes along with it:
- **The citadel becomes an optional fortress.** Its upgrade price drops from 150W 200S 100Fe 75Ae to 150W 150S 75Fe 50Ae. It keeps its 4,200 hp, `keep_arrows`, +5 population over the town centre, and garrison 20.
- **`dune_great_lens` moves** from `citadel` to `dune_house_of_wisdom`, since the citadel is no longer guaranteed.
- **Rewrite** doc 3.1 and the `town_center` and `citadel` descriptions.

Estimated effect, using the doc's one-second model:

| | Today (model) | With F2 + F3 | Where the time comes from |
|---|---|---|---|
| Engine Age | 20:25 | about 19:05 | 29 s from not paying the citadel's 525 resources; 40 s from shorter research; about 11 s from cheaper age-ups |
| Ascendant Age | 25:45 | about 23:40 | the Engine gain, plus shorter research and a cheaper `to_ascendant` |

These figures assume the freed town-centre time is not used for extra workers. A player who keeps training workers gains more. The flow audit's upper estimate is about 18:00 for the Engine Age and 22:30 for the Ascendant.

For the AI, the gain should be larger, because its citadel arrived 3-10 minutes into the Starfall Age.

**Resolved:** the genre audit kept `requires_buildings: ["citadel"]` on the late age-ups. That would keep the citadel wait the sweeps measured, so the requirement is dropped here.

**F3. Move food onto the resources that pile up.** The rule: from the Starfall Age on, food is at most about a third of any common price. What comes off food moves to:
- copper or stone in the Starfall Age;
- silicon in the Engine Age;
- aether or copper in the Ascendant Age.

| id | Now | Proposed |
|---|---|---|
| `to_banner` | 500F 100W | 400F 100W 100S. Tested in `xp_food`. It spends the 150 starting stone, which otherwise sits untouched for the first 5:40. |
| `to_starfall`, and every Oath | 600F 150Cu 100Ae | 400F 100S 200Cu 100Ae. Tested in `xp_food`. |
| `to_engine` | 600F 250Fe 200Si 200Ae | 350F 250Fe 250Si 200Ae |
| `to_ascendant` | 800F 300Si 450Ae 200Ad | 400F 400Si 450Ae 200Ad |
| `sylvan_wake_the_wildwood` | 600F 150W 150Fe 200Si 200Ae | 350F 150W 150Fe 250Si 200Ae |
| `plate_armor` / `plate_barding` | 250F 200Fe / 275F 200Fe | 125F 100Cu 200Fe for both |
| `aether_tempering` | 250F 150Fe 40Ae | 125F 100Cu 150Fe 40Ae |
| `rifled_barrels` | 200F 100W 150Fe | 100F 100Cu 150Fe |
| `halberd_drill` / `hand_cannon_drill` | 250F plus metals | 150F, the rest unchanged |
| `crop_rotation` / `handcart` | 250F 200W / 300F 150W 50Fe | 100F 250W / 150F 200W 50Fe |
| `steam_hammers` / `smokeless_powder` | 300F 200Fe 50Si 100Ae / 250F 200Fe 75Si 100Ae | 150F 200Fe 125Si 100Ae / 125F 200Fe 150Si 100Ae |
| `ballistic_weave` / `steam_harvesters` | 350F 250Fe 75Si / 300F 150Fe 75Si | 175F 250Fe 175Si / 150F 150Fe 150Si |
| `rifle_squad` | 260F 100Cu 170Fe 80Si 30Ae | 150F 140Cu 170Fe 150Si 30Ae. Same 640 total. It was fielded in 3 of 45 sweep A games and never in sweep B. |
| Faction techs and late faction units | | See the faction sections: `starborn_zealot`, `dune_veil_guard`, `tide_phaser_marine`, `ironhold_oathsworn` and others. |

With the prices in this table, food in the Starfall common tech prices falls from 3,125 to 2,125: the eight Starfall rows above lose 1,000 food between them. The table does not yet meet its own one-third rule everywhere:

- `halberd_drill` (150F of 275) and `hand_cannon_drill` (150F of 350) are still above a third;
- four Starfall techs are not in the table: `deep_shafts` (200F of 450), `aether_resonance` (200F of 400), `star_tithe` (300F of 550) and `conscription` (400F of 600).

Applying the rule to those six as well, at the same totals, brings Starfall food to about 1,600.

**F4. Give copper a late job.** This is optional and data-only.

Copper is banked all game (a mean of 145 in the Engine Age), and Tide's x1.25 copper bonus has nothing late to buy.

The change: Engine machines pay copper instead of wood, keeping the same totals. This also cuts each of them from five resources to four.

| Unit | Wood + copper now | Copper after |
|---|---|---|
| `steam_tank` | 100W 100Cu | 200Cu |
| `rocket_battery` | 100W 70Cu | 170Cu |
| `flak_wagon` | 80W 60Cu | 140Cu |
| `gyrocopter` | 120W 60Cu | 180Cu |
| `biplane_bomber` | 160W 80Cu | 240Cu |

The alternative, merging copper into iron, is an open question (section 8).

**F5. Give defenders and losing players tools.**

- **Garrisons** (Tier 3).
  - The data already sets garrison sizes, among them: `town_center` 15, `citadel` 20, `castle` 20, the early towers 5, `ironhold_rune_bastion` and `ironhold_thunder_bastion` 10, `steppe_golden_tent` 15, `steppe_laager` and `steppe_gun_laager` 6, `tide_siphon_tower` 3, and `starborn_sanctum` 10.
  - Only `crates/af_data/src/schema.rs` reads the field, which it marks "reserved".
  - Proposed behaviour: garrisoned infantry and workers are safe, and each adds one arrow to the building's first Bolt weapon.
- **Start crystals at a fixed 34 m from the town centre,** on every map size (`starts.rs`).
  - 34 m is inside `START_AREA_RADIUS` (46 m), so a watchtower can cover the Banner Age's aether.
  - Pacing stops depending on map size.
  - Keep three crystals of 400 each.

  **Resolved:** the genre audit wanted home aether cut to 2 x 150, so that crater fights start in the Starfall Age. Games are already decided too early, so that cut waits until F2, F3 and the AI fixes have been measured.
- **`star_tithe` for every faction.**
  - Research it at `mining_camp` instead of `temple`, and list it for all six factions. Dune and Steppe have no temple today.
  - Add `Trickle(adamant, 0.02)`.

  It becomes the route to the Ascendant for a player locked out of the crater.
- **A defeat rule for remnants.**
  - After 120 s with no town centre, citadel, castle or production building, a player's drop sites and houses stop keeping them alive. (Walls and gates already do not count today.)
  - This changes `keeps_player_alive` in `crates/af_core/src/ext/economy.rs:90`.
  - Five of sweep B's 6 undecided games were a beaten player with 0-1 units, living on a mining camp or houses.
- **Meteor showers** (Tier 3). From 18:00, every 5 minutes, 1-2 `adamant_meteor` nodes of 800 land in open wilds, at least 40 m from every start. Players are warned 30 s ahead.

**F6. Stage expansion on the new maps.** This is for the map work in `docs/design/maps-and-terrain.md`.

Today, on a Medium 1v1, each contested spot is about 96 m from both starts. Nothing worth taking lies on your own side of the map (`mapgen/mod.rs`, `nodes.rs`).

- **A forward pocket for each player.**
  - Place it 55-65 m from its own start, at least 100 m from any enemy start, and away from the crater.
  - It holds 2 `iron_vein`, 1 `copper_vein`, 1 `stone_outcrop` and 2 `silicon_deposit`, with no aether or adamant.

  Expansion then runs in stages: home, then the pocket (Banner and Starfall ages), then the contested spots (Starfall and Engine), then the crater.
- **Meteor node types,** on top of the five site layouts the contract already defines:
  - `rich_meteor`: `gather_mult` 1.5 and 900 adamant, placed on the most open contested spot. This is StarCraft's "gold base".
  - `meteor_shards`: six nodes of 250, in the open between starts. They are fast to mine and easy to raid.
  - `adamant_meteor`, as now.
  - Later, a `molten_meteor` that needs an `adamant_drill` built on it (the vent pattern, applied to adamant).

  Keep each player's adamant budget at 2,000-4,800 (doc 4.3).
- **Scale pockets and contested spots with map size,** not only with the number of players.

**F7. Small fixes that unclog the middle game.**

- **Place late towers directly.**
  - `build_menu` (`crates/af_core/src/rules.rs:149-162`) hides every building that is an upgrade target, walls and gates excepted.
  - So a gun turret beside a new extractor costs a watchtower plus three upgrades: 100W 475S 50Cu 200Fe 75Si 80Ae and 200 s.
  - Its own price is 250S 150Fe 75Si 50Ae and 80 s.
  - Fix: offer the highest tier the player's age allows, at that tier's own price.
- **Trim late stone,** which exists only at home:
  - `castle` 650S → 450S. Also make the castle a drop site for all eight resources, so it can serve as a forward base.
  - `architecture` 300S → 150S.
  - `ferroconcrete` 400S → 200S.
- **Revive techs nobody uses:**
  - `steam_harness`: age 4 → 3, priced 175F 100Fe 25Ae.
  - `conscription`: researched at `barracks` instead of `castle`.
  - `militia_drill`: age 1, priced 100F 25Cu.
  - `punch_card_logistics`: merge into `analytical_engine` by adding BuildTime x0.85.

**F8. Fix the AI economy first.** These fixes are the first item of Tier 2 (section 7). Without them, the content changes above cannot be measured.

**F9. Turn the sweep into a regression test.** Script the 45-game 1v1 sweep (15 pairings x 3 seeds) and the peaceful controls. The test passes when:

- in the peaceful controls, the Engine Age comes at 19-21 minutes and the Ascendant at 24-27;
- in the 1v1 sweep:
  - at least 60% of players reach the Engine Age;
  - at least 25% reach the Ascendant;
  - the median game ends at 25-35 minutes;
  - no more than 10% of games are undecided at 42 minutes;
  - over 90 games (two seed sets), every faction wins 35-65%;
  - every unit in a faction's `ai.army` is trained in at least half the games where its age is reached.

---

## 3. Synergies and counters

### 3.1 Holes in the counter web

Scores below are stat / duel at equal cost (see the evidence note at the top). Every change is a data edit.

The AI picks its army using the same bonus-times-armour factor (`hit_factor`, `crates/af_ai/src/army.rs:302`), so it adapts to these changes on its own.

**C1. Nothing beats Banner heavy foot.**
- Evidence: the `crossbow` has a Heavy bonus of x1.25. Crossbow against `ironhold_thane_guard` scores 0.60 / -0.12. The Thane Guard and `tide_varangian` each beat 15 of the 17 Banner units.
- Change: raise the `crossbow` Heavy bonus from 1.25 to 1.5.
- Result:
  - crossbow against Thane Guard: 0.72 / +0.49;
  - against Varangian: 1.02 / +0.72;
  - against `man_at_arms`: 0.80 / +0.56;
  - the knight still beats the crossbow (-0.38), and knight against Thane Guard is even at 1.00.

  That gives a cycle: heavy foot beats pikes, pikes beat knights, knights beat crossbows, crossbows beat heavy foot.
- **Resolved:** the synergy audit also nerfed the Thane Guard (hp 125 → 115, pierce armour 0.30 → 0.20) and `dune_sun_guard` (pierce 0.15 → 0.10). Re-run for this document, the crossbow change alone is enough. Ironhold (7-17-6) and Dune (6-17-7) are two of the three weakest factions (Starborn, 5-19-6, is last), and `dune_sun_guard` has no Heavy tag anyway. Leave both units alone until the AI fixes land.

**C2. The Starfall triangle breaks once range counts.**
- Evidence: `aether_lance` has only a Mechanical x1.6 bonus. The lancer against `arquebusier` scores 1.20 stat but -0.28 in the duel.
- Change: add (Ranged, 1.3) to `aether_lance`, as the knight's `war_lance` already has.
- Result: 1.56 / +0.35. The halberdier still beats the lancer, 1.68 / +0.63.

**C3. No melee unit beats a landship.**
- Evidence: `tank_cannon` (75 crush damage, 13 m range) has no `min_range`, so it keeps firing at point-blank range. Duel scores against the landship: `aether_lancer` -0.31, `ironhold_ironclad` -0.70, `starborn_lumen_knight` -0.73.
- Change:
  - give `tank_cannon` a `min_range` of 3.0;
  - `ironhold_piston_hammer`: Mechanical bonus 1.5 → 2.0;
  - `starborn_lightblade`: Mechanical bonus 1.25 → 1.75.
- Result: lancer +0.70, Steamplate +0.54, Lumen Knight +0.50. The tank still beats rifles (+0.48) and gatlings (+0.81).

**C4. Fire does not burn beasts,** even though doc "Factions at a glance" names fire as Sylvan's weakness.
- Evidence: only `short_spear`, `pike`, `halberd` and `dune_camel_scimitar` have any bonus against Beast. Against `sylvan_great_bear`, both the siphoneer and the grenadier score -0.16.
- Change: add (Beast, 1.5) to `grenade`, `fire_bomb`, `flame_projector`, `tide_siphon`, `tide_tower_siphon`, `steppe_fire_lance`, `dune_glass_bomb` and `dune_glass_torrent`.
- Result: siphoneer 1.31 / +0.59, grenadier 1.22 / +0.61, fire lancer 1.05 / +0.32. The +0.15 fire armour from `sylvan_wildwood_ascends` then becomes a real answer.

**C5. Shields have no counter that can move.**
- Evidence: only `emp_pulse`, which sits on a building, and `dune_khanjar` have a bonus against Shielded units. 17 units are Shielded: 7 common (`shield_trooper`, `hover_tank`, `gunship`, `starfighter`, `plasma_artillery`, `star_bomber`, `storm_walker`) and 10 faction units; Ironhold and Sylvan have none. The paladin's description says it "breaks shields", but its numbers do not.
- Change:
  - `starborn_sun_hammer`: add (Shielded, 1.5), and raise its Mechanical bonus from 1.25 to 1.6;
  - `ironhold_thunder_rune`, `tide_galvanic_harpoon` and `tesla_coil`: add (Shielded, 1.5);
  - `sylvan_crystal_lens`: add (Shielded, 1.25).
- Result: paladin against automaton 1.12 / +0.35. Runeguard against `shield_trooper` 0.87, and against the jet lancer +0.32.

**C6. The storm walker's tesla coil never reaches its target.**
- Evidence: `tesla_coil` reaches 8 m. `plasma_carbine` fires from 10 m and `tide_phaser` from 11 m. Storm walker against shield trooper scores -0.04.
- Change: raise the `tesla_coil` range from 8 to 11. The storm walker's vision is 16, so it can see that far.
- Result: +0.57. The hover tank still beats the storm walker (+0.75).

**C7. Wall-breakers out-fight infantry.**
- Evidence: `ironhold_breaching_drill` and `sylvan_root_ram` can hit units. The Breacher scores 2.49 against gatlings, and the halberdier loses to it (-0.34).
- Change: add (Infantry, 0.4) to both weapons.
- Result: the halberdier beats the Breacher (+0.74), and the Rootbreaker loses to gatlings. Siege needs an escort again.

**C8. Four outliers.**

| Unit | Problem | Change | Result |
|---|---|---|---|
| `tide_siphoneer` | beats 15 of 19 Starfall units | hp 100 → 85; `tide_siphon` Infantry bonus 1.25 → 1.1 | the arquebusier beats it (+0.57) |
| `tide_phaser_marine` | beats 19 of 21 Ascendant units | `tide_phaser` Mechanical bonus 1.3 → 1.1 | the hover tank wins (the marines score -0.14) |
| `sylvan_wisp_swarm` | beats 18 of 21; tagged Beast, not Mechanical, so it skips the Mechanical bonuses that hit drones (`emp_pulse` x2.5, `tesla_coil` x1.25); flak's x2 against Flying still applies | energy armour 0.2 → 0; cost 350 → 370 | wisps against drones 0.90 |
| `dune_sand_mage` | beats 1 of 19 | reworked in section 4.5 | |

### 3.2 Synergies inside an army

- **S1. Healers that heal their own army.**
  - `starborn_reliquary_ark` heals [Infantry, Cavalry, Beast]. But the Synod has no beasts, and its late army is mostly machines. Change the list to [Infantry, Cavalry, Mechanical].
  - `starborn_archon`: add Mechanical to its healing list, and raise its heal rate from 4 to 6.
  - `steppe_sky_shaman`: drop Beast, since Steppe has none.

  "Faith mends the engines" then becomes the Synod's synergy with hover tanks and orbital lances.
- **S2. No friendly fire against the line it supports.**
  - `dune_glass_bomb`: set `friendly_fire` to false, and cut splash from 2.0 to 1.6. Dune's only flyer then stops resetting the shields of its own Veiled Blades: any hit resets shield regeneration (`absorb()` in `crates/af_combat/src/damage.rs`).
  - `steppe_thunder_powder`: replace its grenadier +15% damage with `Stat(Unit("steppe_fire_lancer"), Armor(Fire), Add, 0.15)`.
- **S3. Beasts get an armour line.**
  - `herbal_medicine`: add `Stat(Tag(Beast), Regen, Add, 0.3)`.
  - New tech `sylvan_bark_barding`: age 2, researched at `sylvan_beast_lodge`, costs 150F 75Cu, gives Beast slash and pierce armour +0.05.

  This pairs with C4: beasts burn, but they are no longer paper against steel.
- **S4. Tidy up the tags.**
  - Remove Melee from `clockwork_automaton`. Rule 10.6 says machines carry neither Melee nor Ranged, and today it gets both tech lines.
  - Retag `tide_siphoneer`, `ironhold_hearthguard` and `sylvan_thornback` from Ranged to Melee. They spray at 4.5-6 m: they take the javelin's x2.5 bonus against Ranged, but gain nothing from range techs.
  - Add to rule 10.6: "Ranged means a reach of 7 m or more."
- **S5. Remove duplicate units.**
  - Drop `man_at_arms` and `steam_harness` from `ironhold.ron`. The Thane Guard beats the man-at-arms 1.11 per cost and is already the Thanes' heavy foot.
  - The paladin becomes the shield-breaking support knight (C5), with its heal rate raised from 2 to 4 and its range from 5 to 6.
- **S6. Make unit lines readable.** Three roles run through every age:
  - Line: the Spear-tagged polearms, then `shield_trooper`.
  - Shot: archer → crossbowman → arquebusier → rifle squad.
  - Shock: scout → knight → aether lancer → landship or walker → hover tank.

  To support this:
  - Add `lance_drill`: age 3, researched at `stable`, costs 300F 200Fe 50Ae, `UpgradeUnit` from knight to aether_lancer. It keeps population the same, and the cost ratio is 1.3 (rule 10.3).
  - Generate a "strong against / weak against" line on every tooltip from the bonus tags.
- **S7. Auras** (Tier 3) are what makes synergy visible on the battlefield. Examples: a paladin that restores the shield regeneration of units near it, or a sky shaman that adds speed to nearby cavalry. See section 7.

### 3.3 Terrain and the new maps

- **Already handled by the map contract.**
  - The synergy audit's biggest risk was that rivers would strand every Wheeled unit: most siege (ram, mangonel, ballista, bombard, trebuchet, rocket battery, `ironhold_rune_cannon`, `dune_mirror_engine`), the flak wagon, `steppe_war_ordu` and `dune_sand_engine`. (Not all siege is wheeled: `plasma_artillery` and `ironhold_breacher` are Tracked, `starborn_orbital_lance` hovers and the war balloon flies.)
  - The new `ford` terrain lets Wheeled units cross at x0.6 speed (`terrain.ron`).
  - Connectivity is checked for each kind of movement, including Wheeled units at radius 2.5.
  - `mountain` is a separate terrain from the climbable `cliff`, so Ironhold's climbing infantry do not walk over every mountain.

  Keep those tests (contract section 8).
- **Still worth adding to the map tests:**
  - every player has at least one highland with ore, one thicket forest, one marsh stretch and one dune region within 80 m;
  - rivers keep deep, unfordable stretches between the fords, so Tide's Amphibious trait keeps its value.
- **Later** (Tier 3): combat fields on `TerrainDef`, so each faction's terrain trait also matters in combat:
  - `cover`: thicket gives +0.15 pierce armour;
  - `conceal`: units in a thicket are unseen beyond 4 m;
  - `high_ground`: Ranged units on a cliff get +1.5 range and +3 vision.

---

## 4. Faction diversity

### 4.1 Comparison

| | Ironhold | Sylvan | Dune | Tide | Steppe | Starborn |
|---|---|---|---|---|---|---|
| 1v1 record, both sweeps (W-L-D of 30) | 7-17-6 | 25-3-2 | 6-17-7 | 18-9-3 | 16-12-2 | 5-19-6 |
| Gather bonus | stone 1.25, iron 1.2, copper 1.15 | wood 1.25, food 1.15 | silicon 1.25, aether 1.2 | food 1.15, copper 1.25 | food 1.2, the rest 1.08 | aether 1.2, adamant 1.25 |
| Units: common + unique | 26 + 8 | 17 + 11 | 26 + 8 | 27 + 8 | 25 + 8 | 26 + 9 |
| What the data backs | no stable; its own infantry at 2.2-2.5 m/s; climbing; bastions; longest guns | beasts; longbows; hedges; no machines | mirrors; veils; quartz sifters | marines wade; liquid fire; copper trade | Hearth-age horse archers; stake lines; no stone walls or towers | first flyer (Banner Age); hover ark; Sanctum |
| Promised but not backed, or not distinctive | best walls (Stonewrights and Unbreaking Holds buff the common chain); an ore economy in the highlands (each highland holds just 1 iron and 1 copper vein, game-design.md 4.2) | hunting; "grows its armies" | fast riders; a tech race | anything naval | every herder a rider; "forges on wagons" (the capital rolls only as the Ascendant `steppe_war_ordu`) | healers (5 factions heal) |
| What the AI fields most (sweep B) | spearman 450, crossbowman 215, slinger 207, Thane Guard 203 | 552 hound packs, longbowman 216, elk rider 184 | Sun Guard 408, camel 150, arquebusier 142 | crossbowman 559, 504 marine models, Varangian 365 | outrider 406, horse archer 97 | spearman 560, archer 288 |
| Signature to add | deep mines claim seams; an iron age path | living towers; fire really burns | House of Wisdom age-ups; fast camels | harbours on shoals; rivers as roads | the Khagan's tent proclaims; a riding economy | shrines on star shards; pilgrims feed the Synod |

The six also share much of one structure:

- each has an elite hall: `ironhold_mead_hall`, `sylvan_heart_grove`, `dune_house_of_wisdom`, `tide_arsenal`, `steppe_golden_tent` or `starborn_sanctum`;
- each has a melee unique in the Banner Age: `ironhold_thane_guard`, `sylvan_elk_rider`, `dune_sun_guard`, `tide_varangian`, `steppe_keshig` and `starborn_zealot`. Only the Thane Guard, Varangian and Keshig are tagged Heavy;
- three have Heavy-tagged infantry in the Engine Age: `ironhold_ironclad`, `tide_diver` and `starborn_lumen_knight`. Dune's `dune_veil_guard` is elite shielded infantry without the Heavy tag, and Sylvan and Steppe have none;
- each has a giant costing 10-12 population;
- each has an economy extra, but they differ in kind:
  - buildings that trickle: `ironhold_deep_mine` and `starborn_wayshrine`;
  - resource fields: `dune_quartz_sifter` and `tide_harbor`;
  - a trickle tech: `steppe_great_herds`;
  - a gather tech: `sylvan_crystal_song`.

  All six also have the common `aether_extractor`.

All six use `peasant` as their worker, and all start with 300F 300W 150S 100Cu, 5 peasants and 1 scout. Building overlap between pairs of factions reaches 0.81 (Dune and Starborn, and Tide and Starborn).

The proposals below give each faction one system nobody else has. They use only what the engine already runs:

- node claims: a building with `requires_node` and a `trickle` drawn from the node, as the extractor does. Only the node's own resource is drawn from it; the building's other trickles flow on after the node is empty (`crates/af_economy/src/income.rs`), as the extractor's adamant seep does;
- a different start per faction (`StartDef`);
- where the age-up is researched, and its price;
- a choice between age-ups.

### 4.2 The shared tool: Starfall Oaths

Each faction replaces `to_starfall` with two age-up techs. The two:

- are researched at two different buildings, each one buildable in the Banner Age;
- cost the same (the F3 price, 400F 100S 200Cu 100Ae, 90 s), and both require a `forge`;
- each carry one bonus worth a tech costing 300-500 resources: `effects: [AdvanceAge, <bonus>]`.

This works today with no code:

- `can_research` (`crates/af_core/src/rules.rs`) offers an age-up only in exactly its own age, and only one at a time. So the first Oath to finish locks the other out.
- Every effect other than AdvanceAge applies when the tech completes (`af_economy/src/production.rs`).
- The command card pins one age-up per building, so the two Oaths show at their two buildings.
- The AI takes the first age-up listed (`age_up_tech`, `crates/af_data/src/lib.rs:223`). List the AI's preferred Oath first. That order also makes the AI build the faction's own buildings.
- Later, an `AiProfile.oaths` field could let the AI choose according to its army mix.

| Faction | Oath listed first (the AI's pick) | Second Oath |
|---|---|---|
| Ironhold | `ironhold_oath_of_the_deep` at `ironhold_deep_mine`: `Trickle(iron, 0.15)`, `Trickle(stone, 0.1)` | `ironhold_oath_of_the_anvil` at `forge`: `Stat(Tag(Infantry), Hp, Mul, 1.1)` |
| Sylvan | `sylvan_oath_of_the_hunt` at `sylvan_beast_lodge`: `Stat(Tag(Beast), Speed, Mul, 1.1)` | `sylvan_oath_of_the_bough` at `lumber_camp`: thornwall and thorn gate Hp x1.5, Regen +1 |
| Dune | `dune_oath_of_the_lens` at `dune_house_of_wisdom` (72 s, see 4.5): `dune_mirror_engine` and `dune_mirror_tower` Damage x1.15 | `dune_oath_of_the_caravan` at `stable`: `Stat(Tag(Cavalry), Speed, Mul, 1.1)` |
| Tide | `tide_oath_of_the_convoy` at `tide_harbor`: `Trickle(copper, 0.2)`, `Trickle(food, 0.2)` | `tide_oath_of_the_marines` at `barracks`: `Stat(Tag(Infantry), Armor(Pierce), Add, 0.05)` |
| Steppe | `steppe_oath_of_the_herds` at `mill`: `Trickle(food, 0.4)` | `steppe_oath_of_the_horde` at `stable`: `CostMult(Tag(Cavalry), 0.92)` |
| Starborn | `starborn_oath_of_pilgrims` at `temple`: `Trickle(aether, 0.15)`, `Trickle(food, 0.3)` | `starborn_oath_of_wings` at `starborn_aerie`: `Stat(Tag(Flying), Speed, Mul, 1.15)` |

The Oaths listed first make the AI build buildings it builds today in only 0-3 of about 25 games:

- Ironhold's deep mine;
- Tide's harbour;
- Dune's House of Wisdom;
- Steppe's mill;
- Starborn's temple.

One bonus needs replacing: `starborn_oath_of_wings` repeats the flyer speed of the existing `starborn_aether_sails` (also `Stat(Tag(Flying), Speed, Mul, 1.15)`, at the same aerie). Aether Sails also gives flyers +10% hp and +2 vision, so give the Oath something else, for example `CostMult(Unit("starborn_star_kite"), 0.85)` and the same for `starborn_seraph`.

If this plays well, use the same template for the Engine Age advance.

### 4.3 Ironhold, the Thanes (7-17-6)

**Today.** The slow, unbreakable mountain hold. The data backs most of it:

- no `stable`, and its own infantry at 2.2-2.5 m/s (the common foot soldiers it lists move at 2.6-3.0, the gatling crew at 2.2);
- the Climber trait, which lets infantry and workers walk over cliffs;
- `ironhold_rune_bastion` (garrison 10) and `ironhold_rune_cannon`;
- an iron-heavy giant: `ironhold_forgefather` costs 800 iron;
- in sweep B, its towers dealt the most damage of any faction's.

**What blurs it.**

- "Best walls" amounts to two techs on the common wall chain, which Dune, Tide and Starborn also build:
  - `ironhold_stonewrights` (Hearth Age): walls +25% hp and +10% crush armour;
  - `ironhold_unbreaking_holds` (Ascendant Age, at the castle): walls and towers regenerate 4 hp/s.
- `ironhold_deep_mine` trickles 0.1 stone, 0.06 copper and 0.06 iron wherever it is placed, and it was never built. The AI takes the cheapest drop site (`drop_site_kind`, `crates/af_ai/src/economy.rs:104`): the mining camp costs 100W, against the deep mine's 150W 125S.
- None of its 9 unique techs was ever researched.
- It has no food bonus, and the resources it does get a bonus on pile up (stone banks of 180-218).
- In play it is a spearman and crossbow army like any other.

**Changes**

1. **Deep mines claim seams** (Tier 2).
   - New node `deep_seam`: iron, 3,000, `blocking: false`, `requires_building: "ironhold_deep_mine"`. The map generator places one or two per player at the foot of a highland or mountain, within 60 m of the start, plus one on each highland.
   - `ironhold_deep_mine` gets `requires_node: "deep_seam"` and a trickle of `[("iron", 0.3), ("stone", 0.08)]`. The iron is drawn from the seam; the stone is not, and keeps flowing after the seam is empty (see 4.1). Its cost drops from 150W 125S to 125W 75S, and its limit rises from 3 to 4. It stays a drop site.

   This ties the climbing trait to the highland ore. Because `ironhold_oath_of_the_deep` requires a deep mine, the AI builds one.
2. **An iron age path** (Tier 1). Two faction age-ups replace the common ones:
   - `ironhold_to_engine`: 350F 400Fe 250Si 100Ae, researched at `aether_foundry`, 100 s.
   - `ironhold_to_ascendant`: 400F 400Fe 300Si 250Ae 150Ad, researched at `laboratory`, 120 s.

   Together they save 300 aether and 50 adamant compared with the common pair, so the Thanes can reach the late ages without winning the crater. Doc 4.1 already says the Thanes stay iron-heavy.
3. **Stone from the start** (Tier 1).
   - Starting resources: 300F 200W 300S 100Cu.
   - Drop `man_at_arms` and `steam_harness` (S5).
   - Price the unique techs out of food:
     - `ironhold_oathsworn`: 200F 100Fe → 150S 100Fe;
     - `ironhold_stonewrights`: 100F 100W 50S → 100W 100S.
4. **Later.**
   - Once garrisons exist, each soldier garrisoned in a bastion adds bolts to `ironhold_bastion_bolts` and `ironhold_thunder_runes`.
   - Optional drystone walls from the Hearth Age would make "best walls" true: `ironhold_drystone_wall`, 10S, 600 hp, upgrades to `stone_wall`. This needs art.

### 4.4 Sylvan, the Wardens (25-3-2)

**Today.** The most distinct faction: only 17 of its 28 units (61%) come from the common pool, against 74-77% for the other five. Its unit overlap with each other faction is 0.24-0.29 (Jaccard); Starborn's is as low (0.24-0.28) because it takes a different slice of the common pool. It has:

- beasts from the Hearth Age, longbows, and hedges that regrow;
- no machines and no engineer;
- the Forestwalker trait, which lets its units cross bramble thickets.

It is also the strongest faction in both sweeps. The files show three reasons:

- **The cheapest army in the resource everyone lacks.** Wood x1.25 and food x1.15 pay for an army priced mostly in wood: `sylvan_longbowman` costs 40F 70W.
- **Cheap cavalry that razes buildings.** Elk riders dealt 106k damage to buildings, second only to rams.
  - The elk rider's `sylvan_antler_sweep` is a Slash weapon with no Building multiplier. Against a timber building (slash armour 0.5) it does about 4.4 dps.
  - That is normal for slash cavalry rather than unique: `steppe_keshig`'s `steppe_guard_sabre` does about 5.0. Lances do less because they deal Pierce, which buildings armour 0.8-0.85 against: the knight's `war_lance` does about 2.1.
  - Among melee weapons, only `short_spear` and `pike` carry Building 0.3.
  - The damage total reflects how early and how many elk riders Sylvan fields (184 trained in sweep B) as much as the weapon.
- **The earliest army.** Hound packs from the Hearth-age lodge give Sylvan an army of 5 population at 5:00 and 20 at 8:00. The other factions have 2 and 11-15.

**What blurs it.**

- It lists the whole common stone tower chain, from `watchtower` to `plasma_turret`, although `sylvan.md` says the Wardens never built a stone keep.
- The beast lodge's description says "hunters drop food here", but the map has nothing to hunt.
- It shares healing with four other factions.

**Changes**

1. **Tune it down** (Tier 1):
   - `sylvan_antler_sweep`: add (Building, 0.3). This singles out Sylvan; if the aim is "cavalry should not raze", give `steppe_guard_sabre` and `riding_sword` the same;
   - wood gather bonus: 1.25 → 1.15;
   - `sylvan_starcall`: `Chain(jumps: 3)` → 2;
   - C4 (fire burns beasts).

   Target: about 8-6 over 15 games.
2. **Living towers** (Tier 1 data, plus three tree-tower models).
   - Remove `watchtower`, `guard_tower`, `bombard_tower`, `gun_turret` and `plasma_turret` from `sylvan.buildings`.
   - Add a tower chain on the same 3.5 x 3.5 footprint, with fire armour 0 throughout. These towers burn and regrow, and they cost no stone.

   | Tower | Age | Cost | hp | Regen | Weapon |
   |---|---|---|---|---|---|
   | `sylvan_warden_oak` | 1 | 150W 30Cu | 800 | 2 | `longbow` |
   | `sylvan_elder_oak` | 2 | upgrade 100W 60Cu | 1,300 | 3 | `tower_crossbow` |
   | `sylvan_star_oak` | 4 | upgrade 150W 100Si 80Ae | 2,200 | 5 | `sylvan_crystal_lens` |
3. **The Oaths of the hunt and the bough** (section 4.2).
4. **A beast line** (Tier 1).
   - S3: Beast regeneration from `herbal_medicine`, and the new `sylvan_bark_barding`.
   - Price `sylvan_barkskin` out of food: 250F 150W 50Ae → 250W 100Cu 50Ae.
   - Optional, so that the forest really "grows its armies": `sylvan_dire_wolves` (Banner Age, at the beast lodge, 150F 75W). It is an `UpgradeUnit` from `sylvan_hound_pack` to a new `sylvan_dire_wolf_pack`: 3 wolves of 50 hp each, a cost ratio of 1.33, the same population, and the hound model reused.

### 4.5 Dune, the Sunspire Caliphate (6-17-7)

**Today.** Mirrors (`dune_mirror_engine`, `dune_mirror_tower`), `dune_quartz_sifter`, the shielded `dune_veil_guard`, and `dune_sandskiff`.

**What blurs it.**

- **The "fast camel riders" are the slowest light cavalry in the game.** `dune_camel_rider` runs at 5.6 m/s. `scout_rider` and `tide_stradiot` run at 6.2, and `steppe_outrider` at 6.0.
- **The "tech race" is a copy.** It amounts to `dune_calculists`, which is the common `analytical_engine` plus siege range. Dune also uses the common age-ups.
- **The academy is almost never built.** Dune has one Hearth-age unique. The House of Wisdom was built once in about 25 games, so the sand mage and four Dune techs are almost never available.
- **The sand mage does not work.** It beats 1 of 19 Starfall units, with 0.025 damage per second per unit of cost against an age median of 0.067. Only 9 were trained across all games.
- **The Veiled Blades fight against their own side.** They are paid mostly in food and iron (260F and 240Fe of 690). Dune has no bonus on either: its bonuses are on silicon and aether only. Dune's own glass bombs also reset their shields.

**Changes**

1. **The academy proclaims the ages** (Tier 1).
   - `dune_to_starfall` (requires `forge`, 72 s), `dune_to_engine` (requires `aether_foundry`, 80 s) and `dune_to_ascendant` (requires `laboratory`, 95 s) are all researched at `dune_house_of_wisdom`, at the F3 prices.
   - Research there is 20% faster than with the common age-ups.
   - When the Oaths land, `dune_to_starfall` becomes `dune_oath_of_the_lens`.

   The Caliphate ages fastest, and its 1,800-hp House of Wisdom becomes a landmark: destroy it and the next age stalls. The AI builds the House through `age_up_step`.
2. **Fast riders** (Tier 1).
   - `dune_camel_rider`: speed 5.6 → 6.4, hp 90 → 80.
   - `dune_camel_breeding` adds `Stat(Unit("dune_camel_rider"), Speed, Mul, 1.1)`.
   - Starting units: `[("peasant", 5), ("dune_camel_rider", 1)]` instead of the scout.
3. **A sand mage that works** (Tier 1).
   - `dune_sandstorm` becomes a thrown storm: `Arc(speed: 10, apex: 0.4)`, range 8, 18 slash damage, splash 2.0, cooldown 3.0, (Infantry, 1.3), `friendly_fire: false`.
   - Cost: 70F 40Cu 40Si 40Ae → 60F 30Cu 30Si 30Ae.

   Against a single arquebusier its stat score rises from 0.32 to 0.44 with the new weapon at today's price (190), and to about 0.71 with the price cut to 150 as well. The splash also hits 2-3 more models in a squad or blob. Check it against the 1.5x band of rule 10.5 before shipping.
4. **Veils that turn bullets** (Tier 1).
   - `dune_veil_guard`: pierce armour 0.35 → 0.50, speed 3.1 → 3.4.
   - Cost: 260F 60Cu 240Fe 100Si 30Ae → 140F 60Cu 150Fe 240Si 100Ae. That is the same 690 in total, moved onto the resources Dune has bonuses for.

   With S2 (glass bombs without friendly fire), ornithopters and Veiled Blades become a combination that works together.

### 4.6 Tide, the Tidecrown Republic (18-9-3)

**Today.** The Amphibious and Marshwalker traits, `tide_siphoneer` and `tide_siphon_tower`, and copper trickles from `tide_levant_convoys` and `tide_marriage_of_the_sea`.

**What blurs it.**

- **Nothing is naval.**
  - `tide_harbor` is a 3,000-food field that works on dry land.
  - It was built 0 times, because the AI's `field_kind` picks the farm: 60W per worker slot against the harbour's 100. (`drop_site_kind` likewise picks the 100W mill over the 200W harbour.)
  - Amphibious only matters on water: it opens deep water (x0.7) and speeds up shallows and fords (x0.9, against x0.55-0.7 for other foot).
- **It turtles like Ironhold.** It lists `castle`, `conscription` and every wall and tower tier. Its buildings overlap Ironhold's by 0.78 (Jaccard: 32 shared of 41 distinct).
- **One tech is a copy.** `tide_murano_glass` has the same effect as `dune_glass_kilns` (silicon gathered 20% faster); only the price and the building differ.
- **It plays as a crossbow army.** The crossbowman was its most-trained unit, 559 times, and the top damage dealer against units in the game.

**Changes**

1. **Harbours on fishing shoals** (Tier 2).
   - New node `fishing_shoal`: food, 2,500, `blocking: false`, `requires_building: "tide_harbor"`.
   - The map generator gives every start a small pond, 5-6 m across, 30-40 m out on the side away from the crater, with two shoals in it.
   - It also places shoals along rivers (about one per 40 m of bank), lakes and coasts. The new water slider then scales Tide's economy.
   - `tide_harbor` gets `requires_node: "fishing_shoal"` and a trickle of `[("food", 0.6), ("copper", 0.1)]`, and loses its `resource_field`. The food is drawn from the shoal; the copper keeps flowing after the shoal is fished out (see 4.1). Its cost drops from 200W to 150W. It keeps its drop sites, its training and its limit of 3.
2. **Rivers are roads** (Tier 1). In `terrain.ron`, Amphibious speed changes:
   - `shallow_water` and `ford`: 0.9 → 1.1;
   - `deep_water` and `ocean`: 0.7 → 0.9.

   Only Tide has the trait, so its marines move faster in river shallows and at fords than over grass, and at 0.9 through the deep core.
3. **Stop turtling** (Tier 1).
   - Remove `castle` and `conscription` from `tide`.
   - Train `tide_varangian` at `["barracks"]` only, and `tide_siphoneer` at `["tide_arsenal"]` only.
   - Research `tide_varangian_oath` at `tide_arsenal`.
   - Delete `tide_murano_glass`.
   - `tide_boarding_drill`: 150F 40Fe → 100Cu 60Fe.
   - `tide_phaser_marine`: 240F 150Cu 80Fe 190Ae 60Ad → 120F 270Cu 80Fe 190Ae 60Ad, which gives copper a late use. Apply C8 at the same time.
4. **Later** (Tier 3): a `Naval` movement type and a Banner-age `tide_war_galley` trained at the harbour, now that maps have lakes and coasts.

### 4.7 Steppe, the Khaganate (16-12-2)

**Today.** Horse archers from the Hearth Age (`steppe_outrider`: 406 trained, the best early raider), stake lines and wagon laagers, no stone walls or towers, and the Marshwalker and Dunerunner traits on mounted and wheeled units.

**What blurs it.**

- **Most of the army walks.** Steppe trains 10 types of foot unit, and its workers walk.
- **The faction description says the clans "carry their forges on wagons and, in the end, their capital too". Until the end, nothing rolls.**
  - The forges are the common, static `forge`. The capital on wheels is `steppe_war_ordu`, an Ascendant giant trained at the tent.
  - Before that, the Khagan's seat is `steppe_golden_tent`, a static building with a limit of 1, and it was built 3 times in 25 games.
  - So `steppe_sky_shaman` and `steppe_war_ordu` were never trained, and `steppe_kurultai` and `steppe_thunder_powder` were rarely researched.
- **Its techs repeat common ones.** `steppe_kurultai` is close to `conscription`. `steppe_great_herds` is a generic food trickle, researched at the mill the AI never finishes.

**Changes**

1. **The Khagan proclaims the Engine Age** (Tier 1).
   - `steppe_to_engine`, at the F3 price, is researched at `steppe_golden_tent` and requires `aether_foundry`.
   - The AI then builds the tent in every game, which unlocks the shaman, the Ordu and the horde's techs.
   - The tent costs 750 (350W 100S 150Cu 100Fe 50Ae), about 225 more than the 525 citadel upgrade that today's path requires. Under F2 no faction pays for the citadel, so this is a real extra cost for Steppe, paid back by the units and techs the tent unlocks.
2. **An economy that rides** (Tier 1).
   - `steppe_felt_camps` adds `Stat(Tag(Worker), Speed, Mul, 1.15)`.
   - Long trips to the crater gain the most: a crater aether worker with no nearby drop site rises from about 0.133 to about 0.143 aether per second (+7%). Only the walking part of the trip speeds up: at 0.133/s a 10-load trip is about 75 s, of which about 42 s is walking, so x1.15 speed saves about 5.5 s.
   - List `steppe_oath_of_the_herds` first, which also gets the AI's mill built.
   - Later, a mounted worker, `steppe_herder`. This needs a model.
3. **Cut the foot** (Tier 1).
   - Remove `arquebusier`, `gatling_crew`, `slinger` and `grenadier` from `steppe.units`. Horse archers and rocket riders become the gunline.
   - Apply S2 to `steppe_thunder_powder`.
   - `steppe_kurultai` becomes `CostMult(Tag(Cavalry), 0.9)` plus `Stat(Tag(Cavalry), Speed, Mul, 1.08)`, so Steppe's speed edge starts in the Starfall Age.
   - Check Steppe's answers to infantry with a headless sweep before shipping.
4. **Later** (Tier 3): a moving capital. Either units that act as drop sites (`UnitDef.drop_site`, for `steppe_war_ordu` and a Banner-age `steppe_supply_wagon`), or a Golden Tent that packs up into a wagon.

### 4.8 Starborn, the Synod (5-19-6)

**Today.** The first flyer (`starborn_star_kite`, Banner Age). The only faction with `war_balloon`, `starfighter`, `drone_swarm` and `knight`. Lightbearers, and a shielded Sanctum.

**What blurs it.**

- **It is built from the common pool.** It lists all 36 common buildings and 52 of the 53 common techs, and shares 0.81 of its buildings with Dune and with Tide.
- **Its signatures arrive late or not at all.**
  - It has nothing of its own in the Hearth Age.
  - Four other factions also heal.
  - Its wayshrine trickles a token 0.05 aether per second, and the shrine was built 0 times.
- **Its bonuses pay off too late.**
  - Its bonuses are on aether and adamant. It gathered an average of 531 aether and 25 adamant before being defeated, at a median of about 24 minutes.
  - It also gathered the least food: 6,696, against 8,600-10,700 for the others.
- **Its army is the common Hearth army.** It trained mostly spearmen (560) and archers (288). Its Banner infantry unique, `starborn_zealot`, costs 200F 50W 50Fe, the most food-hungry Banner unit in the game.
- **Changing the AI's choices alone did not fix it.** Changing only its AI army weights (`xp_star`) gave 1-13-1, so the problem is in the content.

**Changes**

1. **Shrines on star shards from the Hearth Age** (Tier 2).
   - New node `star_shard`: aether, 1,200, `requires_building: "starborn_wayshrine"`. The map generator places 2 beside each start's crystals and 1 at each contested spot.
   - `starborn_wayshrine`: age 2 → 1, `requires_node: "star_shard"`, a trickle of `[("aether", 0.15)]` drawn from the shard, and cost 75W 75S → 100W. It stays an aether and adamant drop site, with a limit of 5.

   The Synod then banks its Starfall aether without walking to the crater. If the node is not wanted, a data-only fallback: age 1, 100W, and a trickle of 0.08 aether placed anywhere.
2. **Food for the Synod** (Tier 1).
   - `starborn_zealot`: 200F 50W 50Fe → 120F 40W 40Fe.
   - `starborn_fervour`: 150F 50Cu → 100Cu 40Ae.
   - List `starborn_oath_of_pilgrims` first. The AI then builds the temple, which unlocks `star_tithe`, `herbal_medicine`, the monk and the Reliquary Ark.
   - Add crossbowman at 0.8 to `ai.army`, so that `starborn_rite_of_the_lens` has crossbowmen to convert.
   - If Starborn still wins under 35% after re-measuring, add a food gather bonus of x1.1.
3. **Faith mends the engines** (Tier 1): S1, and the shield-breaking paladin (C5).
4. **The Sanctum is the keep** (Tier 1).
   - Remove `castle` and `conscription` from `starborn`.
   - Research `starborn_holy_orders` at `starborn_sanctum`.
   - Later (Tier 3): a wayshrine aura (radius 10, Regen +1 for infantry and cavalry) and a Sanctum shield dome over buildings within 12 m. Doc section 11 anticipates the same 12 m dome for the common `shield_generator`, so one engine feature serves both.

### 4.9 The Ascendant Age looks the same for everyone

Every faction ends with a giant. Four factions field each of `hover_tank` (Dune, Tide, Steppe, Starborn), `shield_trooper` (Sylvan, Dune, Tide, Starborn) and the `starport` (Dune, Tide, Steppe, Starborn); Dune, Tide and Starborn have all three.

- Remove `hover_tank` from Dune and Steppe. Their hover units are `dune_sandskiff` and `steppe_skyrider`.
- Remove `shield_trooper` from Tide. `tide_phaser_marine` is its shielded line.
- Before shipping these two removals, re-check Dune's anti-air and Steppe's anti-armour.
- Add `limit: Option<u32>` to `UnitDef`, and set it to 1 on all six giants.
- Tag `sylvan_elder_treant` and `dune_sand_engine` as Hero, like the other four giants.

---

## 5. Ideas from other RTS games that fit

The genre audit collected the sources below. They were not re-checked for this document. Figures taken from fan wikis are only rough orders of magnitude.

| Idea | Where it comes from | How it fits Aetherfall | Section |
|---|---|---|---|
| A choice of age-ups, each with its own bonus | AoE4 landmarks; AoM minor gods ([AoE4](https://en.wikipedia.org/wiki/Age_of_Empires_IV), [AoM](https://en.wikipedia.org/wiki/Age_of_Mythology)) | Starfall Oaths, data-only | 4.2 |
| Aging up without stopping worker production | AoE4 Abbasid House of Wisdom ([civ page](https://www.ageofempires.com/games/age-of-empires-iv/civilizations/abbasid-dynasty/)) | late age-ups off the town centre; Dune's academy | F2, 4.5 |
| A distinct economic rule per civilisation | AoE4 Rus, Mongol Ovoo, Abbasid Golden Age; AoM's favour, earned differently by each culture ([Rus](https://www.ageofempires.com/games/age-of-empires-iv/civilizations/rus/)) | buildings that claim faction nodes: seams, shoals, shards | 4.3, 4.6, 4.8 |
| Rich, exposed expansions | StarCraft II gold bases ([Liquipedia](https://liquipedia.net/starcraft2/Resources)) | `rich_meteor`, `meteor_shards` | F6 |
| Objectives that pay out and can win the game | AoE4 sacred sites, AoE2 relics, SC2 Xel'Naga towers, CoH victory points ([AoE forum](https://forums.ageofempires.com/t/you-dont-have-to-hold-on-to-sacred-sites/273581), [Blizzard map elements](https://news.blizzard.com/en-us/article/4546768/game-guide-map-elements), [CoH](https://en.wikipedia.org/wiki/Company_of_Heroes_(video_game))) | Star Altars: capture points in the crater, with an optional "hold them all" victory | Tier 3 |
| Auras, and buildings that fight back harder when garrisoned | WC3 auras; AoE4 English Network of Castles ([Liquipedia](https://liquipedia.net/warcraft/Aura), [English](https://www.ageofempires.com/games/age-of-empires-iv/civilizations/english/)) | garrisons and auras | F5, Tier 3 |
| Winning by destroying landmarks instead of hunting the last hut | AoE4 ([wiki](https://ageofempires.fandom.com/wiki/Landmark)) | the remnant defeat rule | F5 |
| Few, readable units per age | Dustin Browder on StarCraft II rosters ([Kotaku](https://kotaku.com/the-sacrifices-of-starcraft-ii-made-in-the-name-of-spor-5777029)) | three roles; `lance_drill` | S6 |
| Considered and deferred: neutral guardians at resource sites; an extractor upgrade with claim costs that rise per extractor; a cheap "trash" levy and salvage from wrecks; active powers | WC3 creeps; Beyond All Reason and Northgard; AoE2 trash units; AoM god powers ([creeps](https://warcraft.wiki.gg/wiki/Creeps), [BAR](https://www.beyondallreason.info/guide/in-depth-look-at-economy), [trash units](https://forums.ageofempires.com/t/kamandaran-and-the-trash-unit-system/67179), [Retold](https://progameguides.com/age-of-mythology-retold/all-changes-in-age-of-mythology-retold/)) | worth revisiting once matches regularly reach the Engine Age | section 8 |

---

## 6. What the AI matches show

### 6.1 Records

The two sweeps agree that Sylvan leads, and that factions with a food bonus beat those without. They disagree a lot on the middle of the table:

| Faction | Sweep A (W-L-D) | Sweep B (W-L-D) |
|---|---|---|
| Dune | 0-11-4 | 6-6-3 |
| Steppe | 10-5-0 | 6-7-2 |
| Starborn | 4-7-4 | 1-12-2 |

**Resolved:** the flow audit judged faction strength from sweep A alone. Combined over 30 games each, the records are:

| | Ironhold | Sylvan | Dune | Tide | Steppe | Starborn |
|---|---|---|---|---|---|---|
| Combined record (W-L-D) | 7-17-6 | 25-3-2 | 6-17-7 | 18-9-3 | 16-12-2 | 5-19-6 |

The factions with a food bonus went 59-24-7. The factions without one went 18-53-19.

Sylvan also did well in the team and free-for-all games:

- it won a four-player and a six-player free-for-all;
- it was on the winning 3v3 team;
- it led the other six-player game at 242 population.

Treat a difference of 3 wins either way as noise. Only these conclusions are robust:

- Sylvan is too strong.
- Food decides how strong a faction is.
- The AI's early game decides matches.

### 6.2 AI weakness or content problem

| Symptom | What the AI does wrong (`crates/af_ai`) | What the content does wrong | Fix |
|---|---|---|---|
| Food short at every age-up while everything else piles up | It never completes a mill: 0 in 111 matches. When `fields()` fails to place a new mill hub, it falls back to placing farms round the town centre. Food gatherers are capped at its number of farms, and `fields()` adds at most two farms at a time. | 2,500 food on the age path; 56% food in Banner tech prices; food in nearly every unit | AI: place a mill once 6 farms exist, ignoring the economy reserve, and allow up to 4 farms in progress while food is what the age-up lacks. Content: F3. |
| Faction strength follows food bonuses; the other bonuses are wasted | It never builds faction economy buildings, because `drop_site_kind` and `field_kind` pick the cheapest option | The bonus resources have nothing to buy until late | AI: rank drop sites by cost minus 10 minutes of trickle income. Content: F3, F4, node claims, Oaths. |
| Unique techs almost never researched (Ironhold: 0 of 9) | `military_techs` never touches the age-up reserve, which always holds food (`think/military.rs:269`); `tech_helps_unit` ignores wall, tower and trickle effects | 35 of the 52 unique techs cost 150-500 food (9 cost none, 7 cost 50-125) | AI: spend what the reserve holds beyond the age-up's needs, and value building and trickle effects. Content: price faction techs out of food (section 4). |
| Late units almost never trained: rifle squads in 3 of 45 sweep A games and none in sweep B (F3); one faction giant across both sweeps (section 1). The audits' counts differed here and the logs are not in the repo. | It never builds `engine_works`, `airfield` or `starport`, even when `ai.army` lists units they train. It fills the 300-population cap with Hearth and Banner units (its status log shows "army 170/49": 170 army population against a target of 49). | Late units cost food (`rifle_squad`: 260F) | AI: build the buildings that train the units in `ai.army`, and at the population cap retire units more than one age old. Content: F3. |
| Matches end in the Banner and Starfall ages | First attacks are timed by `MIN_ATTACK_TIME` (360 s) + `TURTLE_DELAY` (210 s) x (1 - aggression); `ARMY_SHARE` keeps 0.25-0.55 of the population as army | No garrisons, distant crystals, a slow age arc (`xp_ram` shows siege is not the cause) | Content and engine: F2, F5. The AI settings matter less. |
| 14% of 1v1s (13 of 90) undecided at 40-42 minutes | It does not hunt down revealed remnants | `keeps_player_alive` counts houses and camps | The F5 defeat rule; the AI sends every unit at a revealed remnant |
| Starborn last | Changing its AI weights did not help (`xp_star`: 1-13-1) | A food-heavy zealot, bonuses that only pay late, nothing of its own in the Hearth Age | Section 4.8 |

### 6.3 What the experiments show

| Experiment | What changed | Result |
|---|---|---|
| `xp_food` | food taken off the age-ups only | Banner and Starfall came about 40 s earlier and the Engine Age 77 s earlier: Banner 5:42 → 5:01, Starfall 14:32 → 13:56, Engine 24:59 → 23:42. 43 of 90 players reached the Engine Age instead of 38, and 12 the Ascendant instead of 9. In the peaceful controls the Ascendant came at 28:05 instead of 30:32. Match length did not change (26:28 → 27:06), food stayed short, and Sylvan went 15-0. |
| `xp_ram` | `ram_head` Building bonus 4.0 → 2.5 | Almost no change: games ended at 27:09, and 42 of 90 players reached the Engine Age. Siege is not why games end early. |
| `xp_star` | Starborn's `ai.army` weights | 1-13-1. Starborn's weakness is in its content. |

Taken together:

- Price changes help, but the AI economy and the town-centre lock are bigger levers.
- Any price change that helps the economy also helps Sylvan, unless Sylvan is tuned down in the same pass.

### 6.4 Caveats

- Unit counts are per model. The hound pack, zealot, marine and delver band are squads of 3.
- Damage is counted before armour.
- Win records are AI against AI, and the AI never builds half the buildings.
- Do not tune factions on AI records until the AI items in Tier 2 have landed and the sweep has been run again.

### 6.5 Corrections to the audits

- `ARMY_SHARE` in `crates/af_ai/src/army.rs:39` has five entries, one per age: 0.25, 0.33, 0.42, 0.5, 0.55. One audit listed four.
- There are 52 unique techs (game-design.md and the faction files), not 46.
- `sylvan_wake_the_wildwood` costs 600F 150W 150Fe 200Si 200Ae. Compared with `to_engine`, it swaps 100 iron for 150 wood; it is not "the same price plus 150 wood".
- `dune_sun_guard` is not tagged Heavy, so the crossbow change (C1) does not affect it.
- The risk of rivers stranding wheeled units is already handled by `maps-and-terrain.md` (section 3.3).
- Node-claiming faction buildings are not fully data-only, as the diversity audit assumed. `claim_nodes` (`crates/af_ai/src/think.rs`) caps every `requires_node` building at `vent_target(age) = age - 1`, which allows none in the Hearth Age. Each of these buildings needs its own target (Tier 2.4).
- `drop_site_kind` and `field_kind` (`crates/af_ai/src/economy.rs`) skip every building with `requires_node`. Once the deep mine and the harbour claim nodes, the "rank drop sites by trickle value" fix in 6.2 no longer reaches them; only `claim_nodes` builds them.

---

## 7. Prioritised roadmap

In the tables below, "Art" means the change needs a new model or a re-skin of an existing one.

### Tier 1: data-only changes

| # | Change | Files |
|---|---|---|
| 1.1 | Late age-ups off the town centre; research times 90/90/100/120 s; the citadel made optional and cheaper (F2) | `assets/data/core/techs.ron`, `assets/data/core/buildings.ron`, `assets/data/factions/sylvan.ron`, `assets/data/factions/dune.ron` (`dune_great_lens`); sections 3.1 and 4.5 of `docs/design/game-design.md` |
| 1.2 | Food re-pricing of the age-ups, the Starfall and Engine techs and `rifle_squad` (F3) | `techs.ron`, `units.ron` |
| 1.3 | Counter fixes C1-C8 | `assets/data/core/weapons.ron`, `units.ron`, `factions/*.ron` |
| 1.4 | Synergy fixes S1-S5: healer tags, friendly fire, beast armour, tag tidying, duplicate units | `units.ron`, `techs.ron`, `factions/*.ron`; rule 10.6 in `game-design.md` |
| 1.5 | Sylvan tuned down; food relief for Starborn; faction techs priced out of food | `factions/sylvan.ron`, `factions/starborn.ron`, `factions/ironhold.ron`, `factions/tide.ron`, `factions/dune.ron` |
| 1.6 | Faction age paths and starts: Ironhold's iron age-ups and stone start; Dune's House of Wisdom age-ups and camel start; Steppe's tent age-up | `factions/ironhold.ron`, `factions/dune.ron`, `factions/steppe.ron` |
| 1.7 | Pruning faction rosters: Tide and Starborn drop the castle and `conscription`; Steppe drops 4 foot units; Tide drops `tide_murano_glass`; Ironhold drops `man_at_arms` | `factions/*.ron` |
| 1.8 | Copper's late job (F4); stone trims and the unused techs (F7) | `units.ron`, `techs.ron`, `buildings.ron` |
| 1.9 | Amphibious speeds on water (Tide, "rivers are roads") | `assets/data/core/terrain.ron` (coordinate with the map work) |
| 1.10 | Living towers for Sylvan (art: 3 tree towers); optional `sylvan_dire_wolf_pack` (reuses the hound model) | `factions/sylvan.ron`, `assets/visuals/` |

### Tier 2: changes that need some code

| # | Change | Files |
|---|---|---|
| 2.1 | **AI economy, first.** Build a mill hub and allow more farms in progress; rank drop sites by trickle value; a research budget that can spend beyond the food reserve; build the buildings that train `ai.army` units; retire obsolete units at the population cap | `crates/af_ai/src/think.rs` (`fields`, `eco_techs`), `crates/af_ai/src/economy.rs` (`drop_site_kind`, `field_kind`, `field_spot`), `crates/af_ai/src/think/military.rs`, `crates/af_ai/src/progress.rs` (`production_targets`, `missing_trainer`, `tech_helps_unit`), `crates/af_ai/src/army.rs` |
| 2.2 | The pacing regression sweep, with the F9 pass criteria | a script beside the `aetherfall` binary, or a test in its crate; doc 4.5 |
| 2.3 | Starfall Oaths for all six factions (data). Optionally, an `AiProfile.oaths` field so the AI chooses by its army mix. | `factions/*.ron`; optionally `crates/af_data/src/schema.rs`, `crates/af_data/src/lib.rs` (`age_up_tech`), `crates/af_ai/src/progress.rs` |
| 2.4 | Faction nodes and claims: `deep_seam`, `fishing_shoal` (with a pond at every start) and `star_shard`. Each `requires_node` building gets its own claim target instead of `vent_target(age)`, including claims in the Hearth Age. Art: three node models. | `assets/data/core/nodes.ron`, `factions/ironhold.ron`, `factions/tide.ron`, `factions/starborn.ron`, `crates/af_map/src/mapgen/nodes.rs`, `crates/af_map/src/mapgen/starts.rs`, `crates/af_ai/src/progress.rs` (`vent_target`), `crates/af_ai/src/think.rs` (`claim_nodes`) |
| 2.5 | Map staging: start crystals at a fixed 34 m; a forward pocket per player; `rich_meteor` and `meteor_shards`; pockets and contested spots scaled with map size. Art: two node variants. | `crates/af_map/src/mapgen/{mod,nodes,starts}.rs`, `nodes.ron`, `docs/design/maps-and-terrain.md` (with the map team) |
| 2.6 | The remnant defeat rule; the AI hunts revealed remnants | `crates/af_core/src/ext/economy.rs` (`keeps_player_alive`) and the victory system; `crates/af_ai/src/army.rs` |
| 2.7 | Place the highest tower tier the age allows, at its own price | `crates/af_core/src/rules.rs` (`build_menu`, `can_build`), the build UI in `af_ui`; doc sections 6 and 9.2 |
| 2.8 | `UnitDef.limit` (giants limited to 1); "strong against / weak against" tooltips | `crates/af_data/src/schema.rs`, `crates/af_core/src/rules.rs`, the `af_data` checker, `af_ui` tooltips |

### Tier 3: larger new mechanics

| # | Change | Files |
|---|---|---|
| 3.1 | **Garrisons**, using the existing `garrison` values: units enter and leave, are protected, each adds one arrow, and the AI uses them | `crates/af_core`, `crates/af_behavior`, `crates/af_combat`, `crates/af_ui`, `crates/af_ai` |
| 3.2 | **Auras**: `AuraDef { radius, target, stat, op, value }` on units and buildings. First ones: wayshrine regeneration (Starborn), paladin shield regeneration, sky shaman cavalry speed, rune bastion infantry pierce armour, druid beast damage, and range for Dune's mirror network | `schema.rs`, stat application in `af_core` and `af_combat`, a ring on the ground in `af_render` |
| 3.3 | **Meteor showers** from 18:00 (F5) | `crates/af_map` or `crates/af_economy`, and a HUD alert |
| 3.4 | **Star Altars**: neutral capture points in the crater and at contested spots. Each pays 0.2 aether per second and gives 20 m of vision. Optional victory for holding every altar for 8:00. | `crates/af_core`, `crates/af_fog`, the victory check, objective goals in `crates/af_ai` |
| 3.5 | **Faction engine features**: Steppe units that act as drop sites (or a tent that packs up); a `Naval` movement type and galley for Tide; the Sanctum's shield dome; `Stat::ResearchTime`. Art: supply wagon, galley, mounted herder. | `schema.rs`, `af_movement`, `af_economy`, `af_combat`, `factions/*.ron` |
| 3.6 | **Terrain combat fields**: cover, concealment and high ground | `TerrainDef` in `schema.rs`, `terrain.ron`, `af_combat`, `af_fog` |

**Order of work**

1. Do 1.1-1.3 and 2.1 together.
2. Measure again with 2.2.
3. Do the faction work (1.5-1.7, 2.3-2.4) and the map items (2.5) alongside the map overhaul.
4. Then Tier 3, starting with garrisons.

---

## 8. Open questions for the owner

1. **Match length.** How long should a 1v1 last, and how long a game of 4-8 players? Is the Ascendant meant to be a normal endgame (the proposed 23-26 minutes for a competent player), or a rare reward?
2. **The citadel.** May the citadel stop proclaiming the late ages (doc 3.1), with the age-ups moving to the foundry, the laboratory and the faction halls?
3. **Oaths.** Do you want a choice of age-ups (Oaths)? If so, only at the Starfall Age advance to begin with, or at every advance?
4. **Copper.** Keep copper as a separate resource with a late "wiring" job (F4), or merge it into iron and have seven resources?
5. **Early adamant.** Is mining adamant before the Engine Age intended? Peasants can mine meteors from minute one, and the pacing model ignores it.
6. **Other ways to win.** Should there be a Star Altar hold victory, or should destroying the enemy (with the remnant rule) stay the only way to win?
7. **How different should factions be?** Today 74-77% of the units in five factions' rosters come from the common pool (Sylvan: 61%). Is a target like "no more than 60% shared" what you want?
8. **Art budget for new units and buildings.**
   - New models: living oak towers, three faction nodes, meteor variants, a Steppe mounted worker or supply wagon, and a Tide galley.
   - Re-skins of existing models: dire wolves from the hounds.
9. **Shields.** Should shields be countered weapon by weapon (C5), or by one global rule: set `energy_resist` to 0.5 on personal shields, so energy beams pass half their damage through?
10. **Friendly fire.** Is friendly fire on every bomb and grenade meant to be a universal rule? If so, should a friend's splash still reset shield regeneration?
11. **Home aether.** Should the home crystals stay at 3 x 400? Or, once the age arc is faster, should they be cut (the genre audit's 2 x 150) so the first crater fights come in the Starfall Age?
12. **Active powers.** Do faction powers that reshape terrain (a flood, a bramble wall, a rockfall) fit pillar 5, "counters, not stat races"? Or should faction identity stay passive?

---

## Fact-check

A second pass on 22 October 2026 checked this document against the files at `1cda6dc`:

- `assets/data/core/*.ron` and `assets/data/factions/*.ron`;
- `docs/design/game-design.md`, `docs/design/maps-and-terrain.md` and `docs/design/factions/*.md`;
- the code locations it cites in `af_ai`, `af_core`, `af_data`, `af_economy`, `af_combat` and `af_map`.

It also recomputed a sample of the stat scores with the game-design.md 5.5 method. These match: C1's 0.60, 0.72, 1.02, 0.80 and 1.00; C2; C4's 1.31, 1.22 and 1.05; C5's 1.12; C7's 2.49; S5's 1.11; and the sand mage's 0.32.

**Corrected**

- **Section 1, 6.3 (`xp_food`):** "every age came about 40 s earlier" became Banner and Starfall about 40 s and Engine 77 s (24:59 → 23:42).
- **Section 1:** Starborn's healers and Ironhold's walls are now listed as "not distinctive", not "nothing behind them".
- **Section 1:** Sylvan's own Engine age-up is noted as the exception to "one age path".
- **2.2 table:** with F2, town-centre time is 180 s until the Oaths land, not 90 s. F2 keeps `to_starfall` at the town centre until then.
- **F3:** the table's prices cut Starfall food from 3,125 to 2,125, not to about 1,600. Reaching 1,600 also needs the one-third rule on `deep_shafts`, `aether_resonance`, `star_tithe` and `conscription`, and on the two drills, which the table leaves above a third.
- **F5:** walls and gates already do not keep a player alive (`keeps_player_alive`), so they were removed from the remnant rule. The garrison list now includes the bastion, laagers and siphon tower.
- **F7:** `build_menu` exempts walls and gates from hiding upgrade targets.
- **C1:** Starborn (5-19-6), not Ironhold or Dune, is the weakest faction.
- **C5:** there are 17 Shielded units, not about 20.
- **C8:** wisps still take flak's x2 against Flying. Being Beast-tagged only spares them the Mechanical bonuses of EMP and tesla.
- **3.3:** not all siege is wheeled.
- **4.1:**
  - The "shared structure" list was wrong in three places:
    - only three Banner melee uniques are Heavy;
    - only three factions have Heavy Engine infantry;
    - only Ironhold and Starborn have a building that trickles.
  - The Steppe promise is "forges on wagons". The capital does roll, as the Ascendant `steppe_war_ordu`.
  - The table now says "its own infantry" and "no stone walls or towers".
- **4.1, 4.3, 4.6:** a node claim draws only the node's own resource. The building's other trickles flow on (`income.rs`).
- **4.2:** `starborn_oath_of_wings` duplicated `starborn_aether_sails`; a replacement bonus is suggested.
- **4.3:** "best walls" also includes `ironhold_unbreaking_holds`.
- **4.4:**
  - "Most distinct" now rests on Sylvan's 61% common units. Starborn's unit overlap is as low as Sylvan's.
  - The elk rider premise was wrong: most melee weapons have no Building multiplier, and only spear and pike do. The Keshig's sabre razes buildings about as well. The nerf is kept, with that caveat.
- **4.5:**
  - Veiled Blades are paid mostly in food and iron, and Dune lacks bonuses on six resources, not one.
  - The new sand mage scores 0.44 only at today's price; at the proposed 150 it scores about 0.71.
- **4.6:**
  - Amphibious also matters in shallows and fords.
  - The 0.78 Tide-Ironhold figure is a Jaccard overlap.
  - Murano Glass matches Glass Kilns in effect only.
  - "Faster than over grass" holds in shallows and fords, not in the deep core.
- **4.7:**
  - The faction description was misquoted.
  - The Golden Tent costs 750, not "about the same" as the 525 citadel upgrade.
  - Felt Camps worker speed gives about 0.143 aether/s, not 0.15.
- **4.8:**
  - The zealot is Starborn's Banner infantry unique; the star kite is the other Banner unique.
  - Doc section 11 anticipates the dome for the `shield_generator`, not the Sanctum.
- **4.9:** the three late pieces are each fielded by a different set of four factions.
- **6.2:**
  - 35 of 52 unique techs cost 150-500 food, and 9 cost none.
  - Undecided games are 13 of 90 (14%).
  - The rifle squad and giant counts contradicted F3 and section 1. They now point to those counts and are flagged unresolved.
- **6.5:** a new note says `drop_site_kind` and `field_kind` skip `requires_node` buildings.
- **Section 8, question 7:** 74-77% of units are common, not 74-79%.

**Checked and correct.** Every unit, building, tech and weapon id, cost, stat and roster count the document uses, apart from the items above:

- the totals: 95 units, 105 techs, 52 unique techs and 36 common buildings;
- the age-path sums (6,225 and 5,150 in total; 2,500 and 1,550 of food) and the food shares by age (56%, 56%, 44%, 31%);
- the 545 s of town-centre time;
- the gun-turret chain sum and the F4 copper totals;
- the code references.

**Not verifiable from the repo.** Every sweep figure: win records, timings, counts trained, damage totals and stockpiles. The logs are outside the repository. The records are internally consistent: 77 wins against 77 losses, 13 draws counted twice, and 30 games per faction.
