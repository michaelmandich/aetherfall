# Peasant art pipeline

A scripted Blender build turns the rigged Meshy character in
`source/peasant_meshy.glb` into the game's peasant: the cleaned mesh and
skeleton, boxy props, a team sash and 23 animation clips. The contract with
the engine (clip names, props, manifest, conventions) is
[docs/art/peasant.md](../../docs/art/peasant.md).

Nothing in `assets/models/peasant/` is edited by hand. To change it, change
a script here and rebuild.

## Rebuilding

Run these from the repository root with Blender 4.4:

```bash
BLENDER="C:/Program Files/Blender Foundation/Blender 4.4/blender.exe"
"$BLENDER" -b -P art/peasant/build_peasant.py                         # every clip -> assets/models/peasant/
"$BLENDER" -b -P art/peasant/build_peasant.py -- --sheets <dir>        # ... and a contact sheet per clip
"$BLENDER" -b -P art/peasant/build_peasant.py -- --only Chop --sheets <dir>   # iterate on one clip
```

| option | effect |
|---|---|
| (none) | Builds every clip and writes `peasant.glb` and `peasant.model.ron` to `assets/models/peasant/`. |
| `--only A,B` | Builds only these clips and the clips they depend on. Nothing is exported unless `--out` is also given, so a partial build cannot overwrite the asset. |
| `--out DIR` | Exports to DIR instead. |
| `--sheets DIR` | Writes one contact sheet per clip to `DIR/<Clip>.png`. |
| `--calibrate DIR` | Writes the pose-helper calibration sheets (`calib_*.png`). |
| `--check-retarget` | Also checks that clips read from other files come through exactly (see "Mixamo clips"). |
| `--mixamo-fbx PATH` | Writes the character as an FBX for uploading to Mixamo, then stops. |
| `--no-export` | Builds and checks only. |

A full build takes about 15 s (mostly the mesh checks and the export), or
about 45 s with sheets for every clip. Keep sheets and other scratch output
outside the repository. The GLB is about 2.6 MB:

| part | size |
|---|---|
| texture | 0.75 MB |
| mesh | 1.1 MB |
| animation | 0.4 MB |
| glTF JSON | 0.4 MB |

### What a build does

1. **Source.** `lib/source.py` imports the source and keeps the skeleton
   exactly: the bone names, the rest transforms and the 0.01 root scale. It
   then cleans the material:
   - no emissive;
   - metallic 0;
   - the 4K base colour downscaled to a 1024 px, 256-colour PNG
     (`lib/png.py`).

   It also reads the source's `Walking` and `Running` clips onto the 30 fps
   grid.
2. **Rig.** `lib/rig.py` sets up the character frame, the rest data, FK in
   metres and samples of the foot soles. `lib/selftest.py` then checks the
   pose helper numerically (19 checks); a failure stops the build.
3. **Props.** `lib/props.py` builds the `Tool_*` and `Load_*` boxes,
   parents them to the hand bones and builds the skinned `Team_sash`.
4. **Clips.** Every module in `clips/` is a clip, and its file name is the
   clip's name. Its `build(clip)` keys poses, and `lib/clip.py` turns those
   into frames:
   - the pose numbers are interpolated with Blender F-curves (cyclic for
     loops);
   - every frame is solved by `lib/pose.py`, with IK where the clip asks
     for it;
   - the frames are baked into an action.
5. **Checks.** `lib/checks.py` prints measurements for every clip. See
   "Checks" below.
6. **Export.** `lib/export.py` writes the GLB, then reads it back to verify
   it:
   - every clip is present with its duration;
   - every prop node sits where it was in Blender;
   - the texture is one 1024 px PNG;
   - nothing is emissive except the aether crystals.

   Finally it writes the manifest.
7. **Sheets.** `lib/sheets.py` renders the contact sheets: front, side,
   three-quarter, and the in-game view (54° pitch at the game's pixel scale,
   enlarged 4x).

## Layout

```
art/peasant/
  build_peasant.py     entry point (headless)
  source/              peasant_meshy.glb, the untouched Meshy source (and any Mixamo downloads)
  lib/
    source.py          import, cleanup, source clips on the 30 fps grid
    png.py             compact PNG writer (RGB or palette), used for the texture
    rig.py             bone names, character frame, rest data, FK, sole samples
    pose.py            THE POSE HELPER: Pose (readable numbers), Solver (numbers -> bones, IK)
    ik.py              two-bone IK
    clip.py            Clip (the authoring API), Context (one build), source resampling
    stances.py         shared start poses: relaxed, combat ready, carrying
    props.py           Tool_* / Load_* boxes, grips, named tool parts, Team_sash
    retarget.py        clips from other files (Mixamo FBX) onto this rig
    checks.py          per-clip measurements and warnings
    selftest.py        numeric checks of the pose helper (fails the build)
    calibrate.py       calibration sheets
    sheets.py          contact sheets
    export.py          GLB export and verification, manifest, FBX for Mixamo
  clips/<Clip>.py      one module per clip; the file name is the clip name
```

## Adding or changing a clip

The engine selects clips by name, and the names it knows are listed in the
contract. To add one, create `clips/<Name>.py`. To change one, edit its
module. Then iterate:

```bash
"$BLENDER" -b -P art/peasant/build_peasant.py -- --only Mine --sheets <dir>
```

Look at the sheet and read the printed checks, then adjust and rebuild. No
other file needs to change: the build finds the module, exports the action
and adds the clip to the manifest.

### The module

```python
"""Mine: overhead pickaxe swings at a rock in front."""   # describe the beats with their times

from lib.clip import FRAME as T        # one 30 fps frame: key on whole frames
from lib.pose import ARMS

DURATION = 1.5          # seconds, a whole number of frames; None = the source clip's length
LOOP = True
HIT = 24 * T            # optional: impact time, written to the manifest
PROPS = ("Tool_mine",)  # what the hands hold (wield/hold use it; sheets show it)
TARGET = (0.0, 1.1, 0.42)   # where the pick's tip lands (right, fwd, up), metres
REACH = TARGET[1]       # optional: how far ahead the work lands, written to the manifest
# optional: LOCOMOTION = True   ground speed measured and written to the manifest
#           SHEET_TIMES = (...) columns of the contact sheet
#           SHEET_PROPS = (...) one prop per column instead of PROPS (all loads share a grip)
#           PROPS_UNTIL = 0.35  the engine hides the props after this; checks stop there
#           BEATS = (0.7,)      deliberate pauses mid-motion; not reported as hitches


def build(clip):
    feet = clip.pose().foot("L", fwd=0.06, out=0.09, turn=6).foot("R", fwd=-0.06, out=0.09, turn=16)
    top = feet.copy().hips(up=-0.02, bend=-4).spine(bend=-12).look(TARGET)
    top.wield(at=(-0.04, 0.17, 1.63), pitch=-50).hold("L", at=0.18, roll=-20)
    hit = feet.copy().hips(up=-0.14, fwd=0.03, bend=12).spine(bend=30).look(TARGET)
    hit.wield_part("Tool_mine", "tip", TARGET, pitch=85).hold("L", at=0.10)

    top, hit = clip.fk(top), clip.fk(hit)       # IK-placed arm -> FK numbers: swings follow arcs
    clip.key(0, hit)                            # loops key every number at t=0
    clip.key(15 * T, top, parts=ARMS)           # the pick arrives at the top first...
    clip.key(17 * T, top, skip=ARMS, interp="QUAD", easing="EASE_IN")   # ...the body starts down
    clip.key(19 * T, top, parts=ARMS, interp="QUAD", easing="EASE_IN")  # then the arms follow
    clip.key(HIT, hit)                          # body and pick fastest as they land
```

### The authoring API (`clip`)

| call | what it does |
|---|---|
| `clip.pose()` | A neutral standing `Pose` (`lib.stances` has shared start poses). |
| `clip.key(t, pose, parts=, skip=, interp=, easing=)` | Keys every pose number at `t` seconds, or only some of them. `parts`/`skip` take prefixes: `"arm"`, `"arm.R"`, `"wield"`. `interp` shapes the segment that *leaves* this key: `BEZIER` (default, auto-clamped), `LINEAR`, `CONSTANT`, or `QUAD`/`CUBIC`/`SINE`/`BACK`... with `easing`. |
| `clip.fk(pose, arms=("R",), legs=(), switch=True)` | Re-expresses limbs placed with IK as FK numbers, so they swing along arcs between keys. It raises if an IK target was out of reach. With `switch=False` the FK numbers are filled in but the limb stays on IK. |
| `clip.solve(pose)` | Returns the solved pose. Use `.part_point("Tool_chip_l", "butt")`, `.prop_frame(name)` or `.hand_frame(side)` to aim one thing at another. |
| `clip.base("Walk", bones="lower")` | Takes these bones from another built clip, time-stretched (`CarryWalk` = Walk's legs under a posed upper body). |
| `clip.from_source("Walking")` | Uses a clip of the Meshy source as it is, in place, with the feet put on the floor. |
| `clip.from_file("mixamo/X.fbx", action=None)` | The same for a clip from another file, retargeted onto the rig (see "Mixamo clips"). |
| `ARMS`, `FEET`, `arm_parts("R")` | Groups of pose numbers (`lib.pose`) for `parts=` and `skip=`. |

### What makes a clip read from 40 m

- **Impacts accelerate into HIT.** Key the segment before the impact with
  `interp="QUAD", easing="EASE_IN"`, and key the body at HIT too, with an
  ease-out into a follow-through key. An auto-clamped Bezier stops at every
  extreme, so a body key placed a few frames before the hit makes the blow
  visibly slow down before it lands. The `strike` check measures this.
- **Kinetic chain.** Key the body a few frames ahead of the arms
  (`skip=ARMS` / `parts=ARMS`): hips first, then the chest, then the arm,
  then the tool.
- **No velocity resets.** A key that starts an `EASE_IN` from rest in the
  middle of a motion makes a hitch. Start eases only at turning points; the
  `hitch` check flags the rest.
- **Planted feet.** Work clips keep both feet on `foot()` (IK), so they
  cannot slide (`slide` must read 0).
- **Two-handed tools.**
  1. `wield()` the tool, or `wield_part()` its working point onto the target.
  2. `hold("L", at=...)` with a constant `at` (key `at` only to slide the
     hand on purpose).
  3. `clip.fk()` the right arm.

  The hands then cannot drift apart.
- **Wrists stay in range.** A tool's grip fixes the hand's rotation, so the
  wrist takes whatever the tool's yaw, pitch and roll leave over. Choose
  those for the wrist as well as for where the tool points. The `wrist`
  check reports flexion and deviation against human ranges.
- **Loops.** Key every number at `t=0`; the build repeats it at the end, and
  the seam must read 0.
- **IK to FK.** To switch exactly on one frame, key `wield.w` with
  `interp="CONSTANT"` on a pose made with `clip.fk(..., switch=False)`. Mine
  shows the pattern.

## The pose helper (`lib/pose.py`)

The module docstring is the reference. Every call sets readable numbers on
a `Pose` and returns it, so calls chain; `None` keeps the current value.
Positions are `(right, fwd, up)` in metres in the character's own frame, and
angles are in degrees. The `root` frame is the ground point under the
character; the `chest` frame follows Spine2.

| call | what it does |
|---|---|
| `hips(right, fwd, up, bend, lean, turn)` | Pelvis offset from rest (m) and tilt/turn. Bend + tips forward, lean + tilts right, turn + faces right. |
| `spine(bend, lean, turn, steady)` | Torso bend, lean and twist, spread over the three spine bones. `steady` cancels the pelvis's own turn and lean. |
| `head(nod, tilt, turn)` / `look(at, weight)` | Head angles (nod + looks down), or aim the face at a point. |
| `shoulder(side, up, fwd)` | Clavicle shrug and roll forward. |
| `arm(side, lift, toward, twist)` | Upper arm: `lift` from hanging (90 level, 180 straight up) in the plane heading `toward` (0 forward, 90 out to that side, -90 across). Measured from the chest, so a shrug does not swing it. |
| `elbow(side, bend, out, twist)` | Elbow flexion. `out` swings an IK elbow outward round the shoulder-wrist line. |
| `wrist(side, flex, tilt, twist)` | Palm-side bend, thumb-side bend, and palm turn (split between forearm and hand). |
| `reach(side, at, frame)` | IK: puts the wrist at a point. |
| `wield(at, yaw, pitch, roll, frame)` / `unwield()` | IK: poses the held prop itself (its grip point and haft direction). |
| `wield_part(prop, part, at, yaw, pitch, roll)` | Wields so that a named part (`props.PARTS`: `bit`, `edge`, `tip`, `face`, `butt`) lands at `at`. |
| `hold(side, at, roll)` / `release(side)` | The other hand grips the prop `at` metres up its haft, turned `roll` about it. |
| `carry(at, ...)` | A load between both hands (all loads share one grip). |
| `foot(side, out, fwd, lift, turn, heel, knee)` | IK foot placement as offsets from the rest stance: `heel` lifts the heel onto the ball of the foot, `knee` swings the knee out. |
| `leg(side, lift, toward, twist, knee, ankle)`, `ankle(...)`, `toe(...)` | FK leg and foot. Keying `foot()` after `leg()` blends back to IK. |
| `copy()`, `set(name, v)`, `get(name)`, `lerp(other, t)`, `mirror()` | The pose as numbers. |
| `Prop.angles(haft, strike)`, `Prop.orientation(yaw, pitch, roll)` | Convert between wield angles and world directions, for example to lay a tool on a swing plane. Get a `Prop` from `props.prop_info(name)`. |

**Calibration.** Zero for every angle is a calibrated neutral built in
`rig.py`:
- the spine, head, clavicles, pelvis and feet are at their rest pose;
- the arms and legs hang straight, with the elbow and knee hinges on the
  left-right axis;
- the hands are in line with the forearm, palm against the thigh.

`selftest.py` enforces every sign numerically, including exact IK-to-FK
round trips. `--calibrate DIR` renders labelled test poses. Re-render them
after touching `rig.py`, `pose.py` or the grip constants in `props.py`.

## Props (`lib/props.py`)

| node | hand | what | named parts |
|---|---|---|---|
| `Tool_attack` | R | Work Axe, one-handed, 0.62 m | `bit` |
| `Tool_chop` | R | felling axe, 0.93 m | `bit` |
| `Tool_farm` | R | hoe, 1.3 m (broom grip, left hand palm up) | `edge` |
| `Tool_mine` | R | pickaxe, 0.87 m | `tip` |
| `Tool_dig` | R | shovel with a cross grip, 1.2 m | `tip` |
| `Tool_chip` + `Tool_chip_l` | R + L | mallet + chisel | `face`; `butt`, `edge` |
| `Tool_smash` | R | sledgehammer, 0.93 m | `face` |
| `Tool_build` | R | hammer (Build and Repair) | `face` |
| `Tool_forage` | L | basket, hanging from the fist | |
| `Load_food`, `Load_wood`, `Load_stone`, `Load_copper`, `Load_iron`, `Load_silicon`, `Load_aether` (glows), `Load_adamant` | R | cargo, one shared two-hand grip | |
| `Team_sash` | skinned | strap from the right shoulder to the left waist, material `Team` (white; the engine tints it) | |

- **Hafted tools.** The prop frame's origin is the right hand's grip just
  above the butt. +Y runs up the haft and +X is the striking direction.
- **Grip.** A power grip holds the haft obliquely across the palm
  (`GRIP_OBLIQUITY`). The hands have no finger bones, so hafts pass through
  the palm; this cannot be seen from the RTS camera.
- **Sash.** The sash is fitted to the body's outline in the plane through
  `SASH_SHOULDER` and `SASH_HIP`. It is laid flat over the tunic's folds and
  skinned to the torso bones only, so an arm never drags it.
- **Visibility.** Every prop is exported visible, because Bevy 0.19's glTF
  loader ignores node visibility. The engine hides all `Tool_*` and
  `Load_*` nodes when it spawns the model.

## Checks

Every build prints one block per clip. Lines starting with `!` are warnings:
look at the sheet, then either fix the clip or accept the warning knowingly.

| line | meaning |
|---|---|
| `seam` | Loops: first frame against last. Must be 0. |
| `pelvis horizontal offset` | How far the pelvis leaves the spot (clips are in place; Death and Spawn travel on purpose). |
| `foot L/R` | Lowest sole point, planted frames, and how far a planted foot slides (must be about 0 in work clips). |
| `ground speed` | Locomotion: the planted foot's speed under the body, written to the manifest as `speed`. |
| `ik reach error` | How far any IK target was out of reach. |
| `wrist L/R` | Flexion (palm side +) and deviation (thumb side +) over the clip, against 25° radial, 40° ulnar and a 65° total bend. |
| `strike` | The tool's striking part (`props.STRIKES`): its speed into HIT against the swing's peak. Warns below 60 %. |
| `hitch` | A bone or tool that nearly stops mid-motion and carries on the same way (a velocity reset). Turning points and declared `BEATS` are not hitches. |
| `body lowest point` | Nothing below the floor. |
| `arm L/R axis vs torso/head` | Closest approach of the forearm and hand axes to the torso and head. Negative means the arm passes through. |
| `props in the body` | How deep a held prop goes into the body, away from the fists. Warns above 1 cm. |

Accepted warnings in the current set:
- Dig: the right wrist at the end of the toss (40° radial).
- Drop: both wrists at the floor set-down (43° radial, for about 0.1 s).
- Farm: the left, palm-up hand at the top of the lift (43° ulnar).
- Run: the source's own right wrist (39° radial).

## Mixamo clips

The skeleton is a standard Mixamo rig (`mixamorig:` bone names), so clips
from mixamo.com can stand in for authored ones or add clips the engine knows
but this set lacks. `clip.from_file()` reads them:

1. Write the character to an FBX for Mixamo (body and skeleton in the rest
   pose, texture embedded):

   ```bash
   "$BLENDER" -b -P art/peasant/build_peasant.py -- --mixamo-fbx <dir>/peasant_for_mixamo.fbx
   ```

2. On mixamo.com, upload it (Upload Character). Mixamo should recognise the
   existing `mixamorig` skeleton and animate it as it is, so its clips come
   back on this exact rest pose. If it re-rigs the character instead, the
   build aligns the rest poses (see below).
3. Pick an animation. For locomotion, tick "In Place" where it is offered
   (the build removes root travel anyway). Download it with these settings:
   - Format: FBX Binary
   - Skin: Without Skin
   - Frames per Second: 30
   - Keyframe Reduction: none

   Save it as `art/peasant/source/mixamo/<Name>.fbx`.
4. Make the clip module with the engine's clip name:

   ```python
   """Cheer: a Mixamo "Victory" clip."""
   DURATION = None   # the file's length (or a whole number of frames to stretch it to)
   LOOP = False


   def build(clip):
       clip.from_file("mixamo/Victory.fbx")
   ```

5. Build it with `--only Cheer --sheets <dir> --check-retarget` and read the
   checks.

`lib/retarget.py` carries each bone's motion across as a world-space
rotation from its rest pose. That makes it independent of the FBX's axes,
scale and bone rolls, which `--check-retarget` proves with a round trip
through FBX using different bone axes. It is exact when the two rest poses
agree, which is why step 2 uploads this very skeleton. The test needs no
Mixamo file; no real Mixamo download has been through it yet.

If the rest poses differ, for example a clip from Mixamo's own T-pose
characters (this rig rests with its arms hanging), each bone is aligned
first by where its limb points and the plane it bends in. The build prints
a note when this happens. In a test with the arms' rest raised 40°, bones
point exactly where the file's do, and rotations are within 3°. A one-shot
clip keeps its first and last frames as they are, and a loop whose last
frame repeats its first is cut to one cycle. The pelvis travel is scaled by
the ratio of the pelvis heights, then removed (in place), and the feet are
put on the floor. Mixamo clips know nothing of the props: list `PROPS` only
to show them on the sheet. A Mixamo clip can still be a base layer:
`clip.base("Walk")` works with any clip.

## Source notes

- **Source timing.** Meshy writes rotations at 30 fps, with the last sample
  repeating the first (Walking: 31 frames = 1.033 s; Running: 19 = 0.633 s),
  and the hips translation at 24 fps over a slightly different span.
  `lib/source.py` samples every channel by phase over its own period.
- **Source clips in place.** The source cycles are already in place. The
  build centres the pelvis and lifts or lowers it per frame so the lowest
  sole point sits on the floor (the source sank 3-5 cm).
- **Viewing the GLB in Blender.** Re-importing the GLB into Blender shows
  bone-parented props metres away. That comes from Blender's importer, not
  the file. `lib/export.py` checks prop placement from the file's own node
  tree, and `crates/af_render/tests/peasant_asset.rs` reads the GLB with the
  same `gltf` crate Bevy uses.
