# Model queue: what to replace with models next

Rule: the more often something is on screen in a real game, the sooner it
gets a model. Measured with the asset census (14 AI matches x 25 minutes,
every faction; rerun any time):

```bash
cargo run --release -p aetherfall --example asset_census
```

"On map" is how many exist at an average moment; "games" is the share of
matches it appears in.

| # | Thing | On map | Games | Box (w x h x d, m) |
|---|---|---|---|---|
| 1 | Pine (node) | ~467 | 100% | 0.9 x 6.0 x 0.9 |
| 2 | Oak (node) | ~440 | 100% | 1.0 x 5.0 x 1.0 |
| 3 | Peasant (unit) | ~83 | 100% | done |
| 4 | Farm | ~31 | 100% | 6.0 x 0.2 x 6.0 |
| 5 | Palm (node) | ~20 | 100% | 0.8 x 5.5 x 0.8 |
| 6 | Aether Crystal (node) | ~18 | 100% | 1.4 x 2.6 x 1.4 |
| 7 | House | ~13 | 100% | 4.0 x 3.0 x 4.0 |
| 8 | Iron Vein (node) | ~12 | 100% | 2.2 x 1.4 x 2.2 |
| 9 | Quartz Sand (node, silicon) | ~11 | 100% | 2.4 x 0.9 x 2.4 |
| 10 | Copper Vein (node) | ~11 | 100% | 2.2 x 1.3 x 2.2 |
| 11 | Stone Outcrop (node) | ~6 | 100% | 2.4 x 1.6 x 2.4 |
| 12 | Lumber Camp | ~6 | 100% | 4.0 x 3.0 x 4.0 |
| 13 | Adamant Meteor (node) | ~5 | 100% | 3.0 x 2.2 x 3.0 |
| 14 | Mining Camp | ~5 | 100% | 4.0 x 3.0 x 4.0 |
| 15 | Aether Vent (node) | ~4 | 100% | 3.0 x 0.4 x 3.0 |
| 16 | Berry Bush (node) | ~4 | 100% | 1.4 x 1.0 x 1.4 |
| 17 | Town Centre | ~2 | 100% | 9.0 x 6.0 x 9.0 |
| 18 | Archery Range | ~2 | 100% | 8.0 x 5.0 x 8.0 |
| 19 | Forge | ~2 | 100% | 7.0 x 5.0 x 7.0 |
| 20 | Barracks | ~2 | 86% | 8.0 x 5.0 x 8.0 |
| 21 | Spearman | ~2 | 71% | 0.7 x 1.4 x 0.7 |
| 22 | Crossbowman | ~2 | 71% | 0.7 x 1.4 x 0.7 |
| 23 | Archer | ~1.5 | 57% | 0.7 x 1.4 x 0.7 |
| 24 | Siege Workshop | ~1 | 100% | 10.0 x 6.0 x 10.0 |
| 25 | Scout Rider | ~1 | 100% | 0.8 x 1.6 x 1.7 |
| 26 | Battering Ram | ~1 | 93% | 1.8 x 1.6 x 3.2 |

Faction-unique units and late-age things each appear in under half the
games and come after all of these.

**Status:** every unit in `assets/data` now has a model
(`assets/visuals/units.ron`, 95 units). The last eleven, late-game units
whose Meshy concepts were picked but never modelled, were made in Tripo
Studio instead ([art/tripo/studio_imports.json](../../art/tripo/studio_imports.json),
`art/units/README.md` "Tripo Studio sources").

## Batches

1. **Trees: pine, oak, palm.** Nine out of ten objects on the map are trees.
   Two or three variants of each make a forest look natural.
2. **Every base: farm, house, town centre, lumber camp, mining camp.** The
   town centre is only ~2 per map, but it is the biggest thing in every
   base and the first thing the player looks at.
3. **Resources:** stone, copper and iron (one rock set with differently
   coloured veins can cover all three), quartz sand, berry bush; then the
   crater set that gives the game its identity: aether crystal, aether
   vent, adamant meteor.
4. **Military buildings everyone builds:** barracks, archery range, forge,
   siege workshop, watchtower (and its upgrades), stone wall segment.
5. **Common units:** spearman, crossbowman, archer, slinger. Humans on a
   Mixamo rig reuse most of the peasant's animation set (walk, idle, hit,
   death); only their weapon attacks are new. Then scout rider (needs a
   horse, which is not a Mixamo rig) and the battering ram (a vehicle: no
   skeleton, just a swinging log).

## What to generate (Meshy or similar)

- **Buildings and nodes: no rig, no animation.** One GLB each, base on the
  ground, centred, facing +Z (the front/door toward +Z). Proportions should
  match the box above; the pipeline scales it to fit the box exactly.
  Polygon count does not matter: the pipeline reduces it (trees especially:
  hundreds are on screen).
- **Units: rigged like the peasant** (Meshy's auto-rig gives the Mixamo
  skeleton the pipeline already understands).
- **Same art style every time** (the peasant's), so models sit together.
- **Team colour:** buildings get a faction-coloured banner added by the
  pipeline, so the source model needs none.
