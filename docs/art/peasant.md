# Peasant model and animation contract

The first real model in the game. This file is the contract between the art
pipeline (`art/units/`, Blender, scripted) and the engine (`af_render`
loading `assets/models/peasant/`). Every other unit model follows it; what
differs for them is in "Other units" at the end.

## Source

- `art/units/peasant/source/peasant_meshy.glb`: the rigged character from Meshy AI
  (one skinned mesh, 18k vertices, one 4K PNG texture, a Mixamo skeleton:
  `mixamorig:Hips`, `Spine`, `Spine1`, `Spine2`, `Neck`, `Head`,
  `Left/RightShoulder`, `Arm`, `ForeArm`, `Hand`, `Left/RightUpLeg`, `Leg`,
  `Foot`, `ToeBase`, plus `headfront`). Skeleton in centimetres under a 0.01
  root scale: the character is about 1.65 m tall, faces +Z, up is +Y.
- It ships only `Walking` (1.08 s) and `Running` (0.71 s) as glTF spans;
  the actual cycles are 31 and 19 frames at 30 fps (`Walk` 1.033 s, `Run`
  0.633 s). Every other clip is authored by `art/units/build_unit.py`
  (shared clips in `art/units/clips/`, the peasant's own in
  `art/units/peasant/clips/`). Because the rig is a standard Mixamo
  skeleton, clips from Mixamo can be read in too (`clip.from_file`, see
  "Mixamo clips" in `art/units/README.md`).

## Build

```bash
"C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b -P art/units/build_unit.py -- --unit peasant
```

It imports the source, cleans the material (no emissive; base colour texture
downscaled to a 1024 px, 256-colour PNG), adds the props below, builds every
clip, and writes the files below (pipeline details, how to add a clip or a
unit: `art/units/README.md`):

- `assets/models/peasant/peasant.glb`: mesh, skeleton, props, all clips.
- `assets/models/peasant/peasant.model.ron`: the manifest the engine reads.
- contact sheets for review under a scratch folder (not in the repo).

## Conventions

- glTF, +Y up, character faces +Z (the game's yaw 0), feet on y = 0 at the
  origin. Every clip is **in place**: no horizontal root/hips travel (the
  game moves the unit). 30 fps.
- The engine scales the model uniformly so its height matches the unit's
  box height (`Footprint.height`; the peasant box is 0.6 x 1.2 x 0.6 m).
- Looping clips loop seamlessly (first pose == last pose).

## Clips (names are exact; the engine selects by name)

| Clip | Loop | Length | Engine plays it when |
|---|---|---|---|
| `Idle` | yes | ~3 s | standing, no work, no cargo |
| `IdleFidget` | no | ~3 s | now and then while idle |
| `Walk` | yes | source | moving without cargo; playback rate scaled to real speed |
| `Run` | yes | source | reserved (fleeing) |
| `CarryWalk` | yes | ~1.1 s | moving with cargo (load held at the chest) |
| `CarryIdle` | yes | ~2 s | standing with cargo |
| `Drop` | no | ~0.7 s | cargo deposited at a drop site |
| `Spawn` | no | ~1 s | just trained (steps out, looks round) |
| `Death` | no | ~1.5 s | killed: collapses, ends lying on the ground (held) |
| `HitReact` | no | ~0.4 s | took damage while not busy |
| `Forage` | yes | ~2 s | gathering food from a bush (picks into the basket) |
| `Farm` | yes | ~2 s | working a farm (hoe strikes) |
| `Chop` | yes | ~1.6 s | gathering wood (two-handed axe swings at a trunk) |
| `Mine` | yes | ~1.5 s | gathering stone, copper, iron (pickaxe swings) |
| `Dig` | yes | ~1.8 s | gathering silicon (shovel scoop and toss) |
| `Chip` | yes | ~1.2 s | gathering aether (crouched, chisel and mallet taps) |
| `Smash` | yes | ~2 s | gathering adamant (heavy overhead sledge) |
| `Build` | yes | ~1.2 s | constructing (hammer strikes, waist height) |
| `Repair` | yes | ~1.4 s | repairing (kneeling hammer) |
| `Attack` | no | 1.0 s | one Work Axe swing; **the hit frame is at 0.5 s** (the weapon's windup) and the clip ends by 1.0 s (cooldown is 1.5 s) |
| `CombatIdle` | yes | ~1.5 s | fighting, between swings |
| `Cheer` | no | ~2 s | own age-up completes |
| `Acknowledge` | no | ~0.8 s | reserved (order acknowledgement) |

## Props (node names are exact; the engine toggles visibility by name)

Low-poly, boxy shapes that match the game's look. Each is a mesh node
parented to a hand bone, posed so the grip fits every clip that uses it.
Hidden unless the engine shows it: every prop is exported **visible** (the
Bevy 0.19 glTF loader ignores node-visibility extensions), so the engine
hides all `Tool_*` and `Load_*` nodes when it spawns the model.

- Tools, one per work role, shown while the unit works (and walks to work)
  in that role: `Tool_forage` (basket, left hand), `Tool_farm` (hoe),
  `Tool_chop` (woodcutter's axe), `Tool_mine` (pickaxe), `Tool_dig`
  (shovel), `Tool_chip` (mallet, right hand) + `Tool_chip_l` (chisel, left
  hand), `Tool_smash` (sledgehammer), `Tool_build` (hammer, also used by
  Repair), `Tool_attack` (the Work Axe, shown for Attack and CombatIdle).
  The engine shows every node whose name starts with `Tool_<role>`.
- Cargo, shown while carrying, one per resource id: `Load_food`,
  `Load_wood`, `Load_stone`, `Load_copper`, `Load_iron`, `Load_silicon`,
  `Load_aether` (glowing violet, like the crystals), `Load_adamant` (a
  charred chunk with gold ore, like the meteors). Each in its resource's
  colours as the game draws the nodes. Held between the hands at the
  chest in `CarryWalk` / `CarryIdle`.
- `Team_sash`: a strap from the right shoulder across the chest to the left
  waist, with its own plain white material named `Team`; the engine tints
  it with its owner's player colour. Always visible. It is a skinned mesh
  (weights copied from the torso), not a bone-parented node, so it follows
  the torso.

## Manifest (`peasant.model.ron`, written by the build)

```ron
(
    model: "models/peasant/peasant.glb",
    height: 1.65,
    clips: {
        "Walk": (duration: 1.0333, looping: true, speed: 1.429), // m/s at rate 1
        "Attack": (duration: 1.0, looping: false, hit: 0.5),
        "Chop": (duration: 1.6, looping: true, hit: 0.9667, reach: 1.3),
        // ... every clip: duration (s), looping, optional speed / hit / reach
    },
)
```

All lengths are the model's own (scale 1, `height` tall, feet at the
origin); the engine scales them with the model.

- `speed`: ground speed (m/s) the feet match at playback rate 1. The
  engine plays Walk, CarryWalk and Run at `unit speed / (speed × scale)`.
- `hit`: seconds into the clip where the blow lands. Attack is started
  with the weapon's windup and played so that `hit` falls on the sim's
  hit.
- `reach`: how far ahead (m) the work lands, from the clip's work point
  (`REACH` in the clip module). The sim stops workers about 0.7 m short of
  what they work on; the engine turns the model toward the work and steps
  it forward (at most to 0.15 m off the target's footprint) until the
  target's drawn surface is `reach` ahead.
- `animation`: the glTF animation's name, when it is not the clip's own.
- The engine decides looping by clip (the one-shots in the table play
  once); a `looping` that disagrees only warns.

## Engine side

- `assets/visuals/units.ron` maps unit ids to manifests
  (`(units: {"peasant": "models/peasant/peasant.model.ron"})`; every
  `*.ron` in `assets/visuals/` is merged, see
  [static-models.md](static-models.md)); units not listed keep their boxes.
- The simulation exposes what a unit is doing through the shared
  `af_core` `Activity` component (written by af_behavior); each resource
  declares its gather style in `assets/data/core/resources.ron`
  (`work: Chop` and so on). Farms always use `Farm`.
- The box footprint stays the collision shape; the model is only visual.
  While a worker works, its model turns toward the work and steps up to
  it (see `reach`); its selection outline and bars move with it.
- The props shown follow the clip being played: the tool of the work clip
  (the load filling up at work stays hidden), the load in CarryWalk,
  CarryIdle and the first half of Drop, the weapon in Attack and
  CombatIdle, and otherwise the tool of the work the unit walks to.

## Other units

Every unit model is built by `art/units/build_unit.py --unit <id>` into
`assets/models/<id>/<id>.glb` and `<id>.model.ron` (same manifest format)
and listed in `assets/visuals/units.ron`. The rules above hold for all of
them, with these differences:

- **Clips.** A unit ships the clips it has; the engine falls back for the
  missing looping ones (`Clip::fallback`: CombatIdle, Walk and the work
  clips to Idle, Run and CarryWalk to Walk, Repair to Build) and skips
  missing one-shots (Attack, HitReact, Death...). Humanoids share Idle,
  IdleFidget, HitReact, Death, Spawn, Cheer, Acknowledge, Walk and Run;
  fighters add their own Attack (with `hit` = the weapon's windup) and
  CombatIdle; healers may add `Heal`.
- **Gear.** A node whose name starts with neither `Tool_` nor `Load_` is
  never hidden. Soldiers' weapons and shields are `Gear_*` nodes
  (`Gear_spear`, `Gear_shield`), held in every clip, since a model also
  replaces the unit's weapon boxes. `Tool_attack` stays the weapon of
  units that work, shown only in Attack and CombatIdle.
- **Team colour.** Always a material named `Team`: the skinned
  `Team_sash` on humanoids (fitted to each body), `Team_pennant` or
  `Team_band` on rigid models, and the face of a round shield.
- **Rigid models** (vehicles, siege, mounts, beasts, flyers): no skin, no
  animation, `clips: {}` in the manifest, `height` = the model's height.
  The engine draws them in their bind pose with no animation player,
  scaled to the box height like any model; they move, fight and die
  without clips, and their corpse sinks its full height (nothing lies
  down). `af_render/tests/models.rs` pins this down.
