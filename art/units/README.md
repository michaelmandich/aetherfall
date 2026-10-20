# Unit models

A scripted Blender build turns each unit's Meshy model into what the game
loads: `assets/models/<id>/<id>.glb` and its manifest `<id>.model.ron`.
There are two kinds of unit:

- **Humanoid** (the default): a character rigged by Meshy (Mixamo bone
  names), generated empty-handed in an A-pose. The build keeps its
  skeleton, puts its weapon or tools in its hands (Meshy models of their
  own, or boxy props), adds a team sash, and bakes the clips: the shared ones in `clips/` (idle, walk, death...)
  plus the unit's own in `<id>/clips/` (its attack). The peasant was the
  first; the spearman is set up and waits for its source.
- **Rigid**: anything without a skeleton (vehicles, siege, mounts, beasts,
  flyers). The build cleans and decimates it, adds a team pennant or band,
  and writes a manifest with no clips; the engine draws it as it is.

The contract with the engine (clip names, props, manifest, conventions) is
[docs/art/peasant.md](../../docs/art/peasant.md). Nothing in
`assets/models/<id>/` is edited by hand: change a script or a config here
and rebuild.

## Adding a unit

The id is the game's unit id (`assets/data/core/units.ron`,
`assets/data/factions/*.ron`). Its folder `art/units/<id>/` holds
`unit.py` (the settings; [lib/config.py](lib/config.py) documents every
one, and a misspelt one stops the build) and, for a humanoid, `clips/`.

### A humanoid

1. **Source.** The generation session saves Meshy's files under
   `art/units/source/`:

   | file | from Meshy | needed |
   |---|---|---|
   | `<id>.glb` | the rigged character (rigging `rigged_character_glb_url`) | yes |
   | `<id>_walking.glb`, `<id>_running.glb` | the rigging's basic animations (`walking_armature_glb_url`...) | no |
   | `props/<prop>.glb` | a weapon or shield, a model of its own (see "Meshy props") | no: a boxy prop stands in |
   | `<id>_<Clip>.glb` | an Animation API clip (3 credits each: ask before spending) | no |

   Walk and Run come from the source's own `Walking`/`Running` actions if
   it has them, else from `<id>_walking.glb`/`<id>_running.glb`, else from
   the peasant's walk, retargeted (`lib/retarget.py`). The build prints
   which.
2. **Config.** `art/units/<id>/unit.py`, for example the spearman's:

   ```python
   """Spearman: a bronze-tipped spear in both hands; no shield."""

   from lib import stances

   PROPS = {"Gear_spear": "spear"}  # node name -> a kind from lib/props.py KINDS


   def CARRY(clip):
       """The spear sloped forward in the right hand, in every shared clip."""
       return stances.spear_carry()
   ```

   `SOURCE` defaults to `source/<id>.glb`; the team sash's points come from
   the skeleton (`SASH` overrides them). See "Props" for `Gear_` against
   `Tool_attack`, and "Shared clips and the carry layer" for `CARRY`.
3. **Weapon.** Name a Meshy prop of `art/units/meshy_props.py` (its model
   is used once it is in `source/props/`, its boxy stand-in until then;
   see "Meshy props"), or a boxy kind of `lib/props.py` (spear, bow,
   crossbow, sling, round_shield, work_axe...; see "Props").
4. **Clips.** The unit gets every shared clip. Fighters need their own
   `Attack` and `CombatIdle` in `<id>/clips/` (start from the spearman's):
   `HIT` is the weapon's `windup` in `assets/data/core/weapons.ron`, and the
   clip should end well inside the weapon's `cooldown`. A unit clip with a
   shared clip's name replaces it.
5. **Build** and look at the sheets and the printed checks (see "Checks"):

   ```bash
   "$BLENDER" -b -P art/units/build_unit.py -- --unit spearman --sheets <dir>
   ```

6. **Register** it in `assets/visuals/units.ron`
   (`"spearman": "models/spearman/spearman.model.ron"`) and run the game.
   Until then its units keep their boxes.

### A rigid unit

1. Save the model as `art/units/source/<id>.glb` (`.gltf`, `.fbx` and `.obj`
   work too). It should face +Z and stand on the ground; anything else is
   one setting away.
2. `art/units/<id>/unit.py`:

   ```python
   """Battering ram: a roofed ram on wheels; rigid, no clips."""

   KIND = "rigid"
   MARKER = "pennant"  # or "band" (round the body), or None
   # YAW = 180          when it faces -Z;  MARKER_AT = (right, fwd, up) / a height fraction
   # TRIANGLES = 5000   CUT_PLATE = 0.05   TEXTURE = 1024
   ```

3. Build, look at the sheet (`<dir>/<id>.png`: the model turned four ways,
   from the game's camera too) and read where the marker went; set `YAW`,
   `MARKER_AT`, `CUT_PLATE` as needed.
4. Register it as above. The engine scales it so its height matches the
   unit's box height and plays nothing (bind pose, no animation player);
   the marker takes the owner's colour.

`battering_ram` and `scout_rider` are configured this way and wait for
their sources. A bob or sway for rigid units can come later as a
manifest field the engine applies.

### Trying a stand-in

`--source` builds a unit from any file, and `--props DIR` takes Meshy prop
models from DIR; with `--out` to a scratch folder nothing in the
repository changes. For example the spearman on the peasant's body, and a
"ram" made of the lumber camp:

```bash
"$BLENDER" -b -P art/units/build_unit.py -- --unit spearman --source art/units/peasant/source/peasant_meshy.glb --out <dir> --sheets <dir>/sheets
"$BLENDER" -b -P art/units/build_unit.py -- --unit battering_ram --source art/static/source/buildings/lumber_camp.glb --out <dir> --sheets <dir>/sheets
```

Never commit stand-in output or register it.

## Commands

Run these from the repository root with Blender 4.4:

```bash
BLENDER="C:/Program Files/Blender Foundation/Blender 4.4/blender.exe"
"$BLENDER" -b -P art/units/build_unit.py -- --list                            # the configured units
"$BLENDER" -b -P art/units/build_unit.py -- --unit peasant                    # every clip -> assets/models/peasant/
"$BLENDER" -b -P art/units/build_unit.py -- --unit peasant --sheets <dir>     # ... and a contact sheet per clip
"$BLENDER" -b -P art/units/build_unit.py -- --unit peasant --only Chop --sheets <dir>   # iterate on one clip
"$BLENDER" -b -P art/units/build_unit.py -- --unit all                        # every configured unit
```

| option | effect |
|---|---|
| `--unit ID` | The unit to build (`a,b` for several, `all` for every one). Builds every clip and writes `<ID>.glb` and `<ID>.model.ron` to `assets/models/<ID>/`. With several units, a unit whose source has not arrived is skipped, and one failing does not stop the others (the run then exits non-zero). |
| `--only A,B` | Builds only these clips and the clips they depend on. Nothing is exported unless `--out` is also given, so a partial build cannot overwrite the asset. |
| `--out DIR` | Exports to DIR instead (the files are named by id, so units can share it). |
| `--source PATH` | Builds from this file instead of the unit's `SOURCE` (one unit only). |
| `--props DIR` | Looks for Meshy prop models (`<prop>.glb`) in DIR instead of `art/units/source/props/`. |
| `--sheets DIR` | Writes one contact sheet per clip to `DIR/<Clip>.png` (`DIR/<ID>/` for several units; a rigid unit's is `DIR/<ID>.png`). |
| `--calibrate DIR` | Writes the pose-helper calibration sheets (`calib_*.png`) on this unit's body. |
| `--check-retarget` | Also checks that clips read from other files come through exactly (see "Mixamo clips"). |
| `--mixamo-fbx PATH` | Writes the character as an FBX for uploading to Mixamo, then stops. |
| `--no-export` | Builds and checks only. |
| `--list` | Lists the configured units and stops. |

The peasant's full build takes about 15 s (mostly the mesh checks and the
export), or about 45 s with sheets for every clip; a rigid unit takes a
few seconds. Keep sheets and other scratch output outside the repository.
The peasant's GLB is about 2.6 MB:

| part | size |
|---|---|
| texture | 0.75 MB |
| mesh | 1.1 MB |
| animation | 0.4 MB |
| glTF JSON | 0.4 MB |

### What a humanoid build does

1. **Source.** `lib/source.py` imports the source and keeps the skeleton
   exactly: the bone names, the rest transforms and the 0.01 root scale.
   It checks that every bone `lib/rig.py` needs is there and joins the
   skinned meshes into one body. It then cleans the material:
   - no emissive;
   - metallic 0;
   - the 4K base colour downscaled to `TEXTURE` px (1024) and re-encoded as
     a 256-colour PNG (`lib/png.py`).

   It also reads the source's `Walking` and `Running` clips onto the 30 fps
   grid.
2. **Rig.** `lib/rig.py` sets up the character frame, the rest data, FK in
   metres and samples of the foot soles. `lib/selftest.py` then checks the
   pose helper numerically (19 checks); a failure stops the build.
3. **Props.** `lib/props.py` builds the unit's `PROPS` (boxes, or a Meshy
   model imported, cleaned, decimated and turned into the prop frame),
   parents them to the hand bones and builds the skinned `Team_sash`. The
   build prints which props are Meshy models and which are stand-ins.
4. **Clips.** `lib/config.py` gathers the clip modules: the inherited
   shared ones (wrapped by the carry layer when the unit holds gear) and
   the unit's own. Every module is a clip, and its file name is the clip's
   name. Its `build(clip)` keys poses, and `lib/clip.py` turns those into
   frames:
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
   - the `Team` material and the sash are there;
   - the textures are PNGs within `TEXTURE` px;
   - nothing is emissive except the aether crystals.

   Finally it writes the manifest.
7. **Sheets.** `lib/sheets.py` renders the contact sheets: front, side,
   three-quarter, and the in-game view (54° pitch at the game's pixel scale,
   enlarged 4x).

A rigid build (`lib/rigid.py`) runs the static model pipeline's own steps
(`art/static/lib`, read-only from here): import any format and bake it
into one statue, turn it by `YAW`, base and centre it, clean the
materials, shrink the textures, decimate to `TRIANGLES`. It then adds the
marker, exports, checks the GLB (static, no emission, the marker's `Team`
material, the triangle budget) and writes the manifest with no clips.

## Layout

```
art/units/
  build_unit.py        entry point (headless)
  meshy_props.py       how each Meshy weapon / shield model is turned into the prop frame
  source/              Meshy sources: <id>.glb, <id>_walking.glb..., props/<prop>.glb (written by the generation session)
  clips/<Clip>.py      shared clips: Idle, IdleFidget, HitReact, Death, Spawn, Cheer, Acknowledge, Walk, Run
  lib/
    config.py          a unit's unit.py, read and checked; the clip modules it builds; the carry layer
    source.py          import, cleanup, source clips on the 30 fps grid
    png.py             compact PNG writer (RGB or palette), used for the texture
    rig.py             bone names, character frame, rest data, FK, sole samples
    pose.py            THE POSE HELPER: Pose (readable numbers), Solver (numbers -> bones, IK)
    ik.py              two-bone IK
    clip.py            Clip (the authoring API), Context (one build), source resampling
    stances.py         shared start poses: relaxed, combat ready (axe, spear), carrying
    props.py           the prop library (tools, weapons, loads), grips, named parts, Team_sash
    retarget.py        clips from other files (Mixamo FBX, Meshy GLB) onto the rig
    checks.py          per-clip measurements and warnings
    selftest.py        numeric checks of the pose helper (fails the build)
    calibrate.py       calibration sheets
    sheets.py          contact sheets (clips; a rigid model turned four ways)
    export.py          GLB export and verification, manifest, FBX for Mixamo
    rigid.py           rigid units: the static pipeline's steps, pennant / band, manifest
    static.py          imports art/static/lib (read-only) for rigid units and Meshy props
  peasant/
    unit.py            the worker: every tool and load, the Work Axe, its sash points
    source/            peasant_meshy.glb, the untouched Meshy source (and any Mixamo downloads)
    clips/<Clip>.py    its own clips: the work clips, carrying, Attack, CombatIdle
  spearman/            unit.py + clips/Attack.py, CombatIdle.py
  battering_ram/       unit.py (rigid, pennant)
  scout_rider/         unit.py (rigid, band)
```

## Shared clips and the carry layer

Every humanoid inherits the shared clips in `clips/` (`INHERIT` limits
them). None of them holds a prop, so they suit any body; the engine plays
them for every unit (contract: "Clips"). What a unit lacks, the engine
falls back from (a missing `CombatIdle` plays `Idle`) or skips (a missing
`Attack` or `HitReact`).

A unit whose hands are never empty (a spearman's spear, an archer's bow, a
shield) lists that gear in `PROPS` as `Gear_*` nodes and gives a `CARRY`
pose that holds it. Each inherited clip `X` is then built twice: as shared
(`X:free`, not exported), and as `X` itself, which takes every bone from
`X:free` except the arms the carry pose uses, whose numbers are constant.
The arm angles are measured from the chest, so the gear rides the body:
it breathes in Idle, bobs in Walk and falls in Death, while the other arm
swings freely. `CARRY` may place the gear with `wield()` and a second hand
with `hold()`; the build converts the wielding arm to FK. `HELD` (default:
the `Gear_*` props) and `CARRY_SKIP` (clips that keep their own arms)
adjust it. Check the carried clips' sheets for the gear crossing the legs
(`props in the body`).

## Adding or changing a clip

The engine selects clips by name, and the names it knows are listed in the
contract. A clip every unit should have goes in `clips/<Name>.py`; one unit's
own in `<id>/clips/<Name>.py`. To change one, edit its module. Then
iterate:

```bash
"$BLENDER" -b -P art/units/build_unit.py -- --unit peasant --only Mine --sheets <dir>
```

Look at the sheet and read the printed checks, then adjust and rebuild. No
other file needs to change: the build finds the module, exports the action
and adds the clip to the manifest. A shared clip changes every unit:
rebuild them all (`--unit all`).

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

A module whose file name starts with `_` is not a clip.

### The authoring API (`clip`)

| call | what it does |
|---|---|
| `clip.pose()` | A neutral standing `Pose` (`lib.stances` has shared start poses). |
| `clip.key(t, pose, parts=, skip=, interp=, easing=)` | Keys every pose number at `t` seconds, or only some of them. `parts`/`skip` take prefixes: `"arm"`, `"arm.R"`, `"wield"`. `interp` shapes the segment that *leaves* this key: `BEZIER` (default, auto-clamped), `LINEAR`, `CONSTANT`, or `QUAD`/`CUBIC`/`SINE`/`BACK`... with `easing`. |
| `clip.fk(pose, arms=("R",), legs=(), switch=True)` | Re-expresses limbs placed with IK as FK numbers, so they swing along arcs between keys. It raises if an IK target was out of reach. With `switch=False` the FK numbers are filled in but the limb stays on IK. |
| `clip.solve(pose)` | Returns the solved pose. Use `.part_point("Tool_chip_l", "butt")`, `.prop_frame(name)` or `.hand_frame(side)` to aim one thing at another. |
| `clip.base("Walk", bones="lower")` | Takes these bones from another built clip, time-stretched (`CarryWalk` = Walk's legs under a posed upper body). With `DURATION = None` the clip takes that clip's length. |
| `clip.from_source("Walking")` | Uses a clip of the Meshy source as it is, in place, with the feet put on the floor (from the unit's walking file or the peasant's when the source has none). |
| `clip.from_file("peasant/source/mixamo/X.fbx", action=None)` | The same for a clip from another file (path relative to `art/units/`), retargeted onto the rig (see "Mixamo clips"). |
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
     hand on purpose: the spearman's thrust drives the spear through the
     front hand).
  3. `clip.fk()` the right arm.

  The hands then cannot drift apart.
- **Wrists stay in range.** A tool's grip fixes the hand's rotation, so the
  wrist takes whatever the tool's yaw, pitch and roll leave over. Choose
  those for the wrist as well as for where the tool points. The `wrist`
  check reports flexion and deviation against human ranges. For a new
  weapon it pays to search: solve a grid of yaw/pitch/roll/hold values with
  `lib.pose.Solver` and keep the ones with the straightest wrists
  (`checks.wrist_bend`); the spear stances in `lib/stances.py` came from
  such a search.
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
character; the `chest` frame follows Spine2. The numbers are in metres, so
clips tuned on one body suit another of about the same height (the
engine scales every model to its unit's box afterwards); check the sheets
of a much taller or shorter one.

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
| `wield_part(prop, part, at, yaw, pitch, roll)` | Wields so that a named part (`bit`, `edge`, `tip`, `face`, `butt`... from the prop's kind) lands at `at`. |
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
round trips. `--calibrate DIR` renders labelled test poses on the unit's
body, with every prop of the library (`calib_weapons` shows the soldiers'
holds). Re-render them after touching `rig.py`, `pose.py` or the grip
constants in `props.py`.

## Props (`lib/props.py`)

The library `KINDS` holds every design; a unit's `PROPS` maps the node
names its GLB gets to kinds. The engine knows props by node name:

- `Tool_<role>` and `Load_<resource>` are shown and hidden by what the unit
  does: a work tool while it works or walks to work, a load while it
  carries, `Tool_attack` only in Attack and CombatIdle (the peasant's Work
  Axe).
- Every other node is always shown. Name a soldier's weapon and shield
  `Gear_*` (`Gear_spear`, `Gear_shield`), so a model never walks around
  empty-handed (a model replaces the unit's weapon boxes too).

| kind | hand | what | named parts |
|---|---|---|---|
| `work_axe` | R | Work Axe, one-handed, 0.62 m | `bit` |
| `felling_axe` | R | felling axe, 0.93 m | `bit` |
| `hoe` | R | hoe, 1.3 m (broom grip, left hand palm up) | `edge` |
| `pickaxe` | R | pickaxe, 0.87 m | `tip` |
| `shovel` | R | shovel with a cross grip, 1.2 m | `tip` |
| `mallet` + `chisel` | R + L | mallet + chisel | `face`; `butt`, `edge` |
| `sledgehammer` | R | sledgehammer, 0.93 m | `face` |
| `hammer` | R | hammer (Build and Repair) | `face` |
| `basket` | L | basket, hanging from the fist | |
| `load_food` ... `load_adamant` | R | cargo, one shared two-hand grip (`load_aether` glows) | |
| `spear` | R | bronze-tipped spear, 2.15 m, gripped 0.7 m up from the butt | `tip`, `butt` |
| `bow` | L | short bow, 1.1 m; back toward +X, string toward the archer | `string`, `tip` |
| `crossbow` | R | crossbow, spanned and loaded; tiller along the haft axis | `bolt`, `butt` |
| `sling` | R | two cords and a pouch, hanging off the fingers | `pouch` |
| `round_shield` | L | 0.62 m centre-grip shield, face in the team's colour | `boss` |

The peasant's table is `props.WORKER` (`Tool_attack` = `work_axe`,
`Tool_chop` = `felling_axe` ..., `Load_<resource>` = `load_<resource>`).
The boxy weapons and the shield stand in for the Meshy models until those
arrive (next section).

- **Hafted props.** The prop frame's origin is the main hand's grip just
  above the butt. +Y runs up the haft and +X is the striking direction.
- **Grip.** A power grip holds the haft obliquely across the palm
  (`GRIP_OBLIQUITY`). The hands have no finger bones, so hafts pass through
  the palm; this cannot be seen from the RTS camera.
- **A new kind.** Write its boxes in a builder (`Boxes().haft(...)`,
  `.box(...)`, `.bar(...)` between two points, `.disc(...)`), add a `Kind`
  to `KINDS` with its hand, mount and named parts, add a pose to
  `calibrate.py` and render `--calibrate` to check the grip. Materials
  come from `_PALETTE` (`Team` is tinted by the engine).
- **Visibility.** Every prop is exported visible, because Bevy 0.19's glTF
  loader ignores node visibility. The engine hides all `Tool_*` and
  `Load_*` nodes when it spawns the model.

### Meshy props

Weapons and shields are generated by Meshy as models of their own, so the
characters can be rigged empty-handed: `art/units/source/props/<prop>.glb`,
about 3k triangles, textured, at any scale and orientation.
`art/units/meshy_props.py` has an entry per prop that turns its model into
the prop frame (the file documents every key):

```python
"spear": dict(like="spear", length=2.15, grip=0.33),
# a shield standing upright, facing the viewer:
# "round_shield": dict(like="round_shield", length=0.62, grip=(0.0, 0.5, 0.5),
#                      grip_offset=(-0.04, 0.0, 0.0), haft="+Y", strike="+Z"),
```

- `like`: the boxy kind it replaces, which stands in until the file arrives
  and gives the hand, the mount and the striking part's name, so clips work
  with either.
- `haft` / `strike`: which of the file's axes (glTF's, +Y up) runs up the
  haft to the head and toward the striking side. Left out, the build takes
  the longest axis for the haft and the longer of the others for the
  strike, and prints what it chose.
- `length`: metres along the haft (the file's scale is arbitrary).
- `grip`: a fraction of the length from the butt; the hand goes to the
  middle of the haft there (where the mesh's edges cross that height). Or
  `(x, y, z)` fractions of the oriented bounding box, plus `grip_offset` in
  metres: a shield's grip is behind the middle of its back.
- `parts`: named points for `wield_part()`; `tip` and `butt` are always
  there.

The model is cleaned like a static model (base colour only, no emission,
metallic 0), its texture shrunk to 512 px, decimated to 1500 triangles,
and its materials named after the node (`Gear_spear`, `Gear_spear_1`).
A stand-in made from the boxy spear, its haft turned onto glTF +X and
scaled 37 times (`--props`), comes out on the boxy one to within its 1 %
length difference, and the spearman's checks read the same with either. When a
real model arrives, build a unit that holds it with `--sheets` and look at
the hands: set `haft`, `strike` or `grip` if the build guessed wrong.

## Team colour

Every unit shows its owner's colour on a material named `Team` (plain
white; the engine tints it):

- **Humanoids: `Team_sash`**, a strap from the right shoulder across the
  chest to the left waist. It is fitted to the body's outline in the plane
  through two points: the unit's `SASH` points, or points found from the
  skeleton (`props.sash_anchors`: just inside the right shoulder joint and
  over the left hip, which reproduce the peasant's hand-placed points to
  1 mm; the build prints them). It is laid flat over the clothes' folds and
  skinned to the torso bones only, so an arm never drags it. `SASH = None`
  leaves it out (a unit whose shield face shows the colour, say).
- **Rigid units: `Team_pennant`** (a pole 0.5 of the model's height with a
  flag streaming back; on `MARKER_AT` or the highest point of the back
  third) **or `Team_band`** (a strip round the model's outline at a fraction
  of its height, fitted by rays like the sash).

## Checks

Every humanoid build prints one block per clip. Lines starting with `!`
are warnings: look at the sheet, then either fix the clip or accept the
warning knowingly.

| line | meaning |
|---|---|
| `seam` | Loops: first frame against last. Must be 0. |
| `pelvis horizontal offset` | How far the pelvis leaves the spot (clips are in place; Death, Spawn and a lunge travel on purpose). |
| `foot L/R` | Lowest sole point, planted frames, and how far a planted foot slides (must be about 0 in work clips). |
| `ground speed` | Locomotion: the planted foot's speed under the body, written to the manifest as `speed`. |
| `ik reach error` | How far any IK target was out of reach. |
| `wrist L/R` | Flexion (palm side +) and deviation (thumb side +) over the clip, against 25° radial, 40° ulnar and a 65° total bend. |
| `strike` | The prop's striking part (its kind's `strike`): its speed into HIT against the swing's peak. Warns below 60 %. |
| `hitch` | A bone or tool that nearly stops mid-motion and carries on the same way (a velocity reset). Turning points and declared `BEATS` are not hitches. |
| `body lowest point` | Nothing below the floor. |
| `arm L/R axis vs torso/head` | Closest approach of the forearm and hand axes to the torso and head. Negative means the arm passes through. |
| `props in the body` | How deep a held prop goes into the body, away from the fists. Warns above 1 cm. |

Accepted warnings in the current set:
- peasant Dig: the right wrist at the end of the toss (40° radial).
- peasant Drop: both wrists at the floor set-down (43° radial, for about 0.1 s).
- peasant Farm: the left, palm-up hand at the top of the lift (43° ulnar).
- peasant Run: the source's own right wrist (39° radial).
- spearman Attack (on the peasant's body as a stand-in): the left forearm
  grazes the chest by 0.7 cm at full extension. Re-check on the real body.

## Mixamo clips

The skeleton is a standard Mixamo rig (`mixamorig:` bone names), so clips
from mixamo.com can stand in for authored ones or add clips the engine knows
but a set lacks. `clip.from_file()` reads them:

1. Write the character to an FBX for Mixamo (body and skeleton in the rest
   pose, texture embedded):

   ```bash
   "$BLENDER" -b -P art/units/build_unit.py -- --unit peasant --mixamo-fbx <dir>/peasant_for_mixamo.fbx
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

   Save it as `art/units/<id>/source/mixamo/<Name>.fbx`.
4. Make the clip module with the engine's clip name:

   ```python
   """Cheer: a Mixamo "Victory" clip."""
   DURATION = None   # the file's length (or a whole number of frames to stretch it to)
   LOOP = False


   def build(clip):
       clip.from_file("peasant/source/mixamo/Victory.fbx")
   ```

5. Build it with `--only Cheer --sheets <dir> --check-retarget` and read the
   checks.

Meshy's Animation API clips (`art/units/source/<id>_<Clip>.glb`, 3 credits
each: ask before spending) read the same way:
`clip.from_file("source/spearman_Attack.glb")`. The file's one-frame
bind-pose stub (`Armature|clip0|baselayer`) is ignored.

`lib/retarget.py` carries each bone's motion across as a world-space
rotation from its rest pose. That makes it independent of the file's axes,
scale and bone rolls, which `--check-retarget` proves with a round trip
through FBX using different bone axes. It is exact when the two rest poses
agree, which is why step 2 uploads this very skeleton. It samples the
action over its rotation keys (Blender's glTF importer starts a Meshy clip
on frame 1). The peasant's Meshy walk read this way onto the spearman
gives the same 31-frame cycle as the peasant's own (1.45 m/s against 1.43);
no real Mixamo download has been through it yet.

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
  repeating the first (the peasant's Walking: 31 frames = 1.033 s; Running:
  19 = 0.633 s), and the hips translation at 24 fps over a slightly
  different span. `lib/source.py` samples every channel by phase over its
  own period.
- **Source clips in place.** The source cycles are already in place. The
  build centres the pelvis and lifts or lowers it per frame so the lowest
  sole point sits on the floor (the source sank 3-5 cm).
- **Viewing the GLB in Blender.** Re-importing the GLB into Blender shows
  bone-parented props metres away. That comes from Blender's importer, not
  the file. `lib/export.py` checks prop placement from the file's own node
  tree, and `crates/af_render/tests/peasant_asset.rs` reads the GLB with the
  same `gltf` crate Bevy uses.
- **A-pose.** The characters are rigged in an A-pose. The clips do not
  depend on the rest pose (every angle is measured from a calibrated
  neutral), but `--mixamo-fbx` uploads it and retargeted clips align to it.
- **New sources.** The first real file of a kind (a rigged soldier, a
  `<id>_walking.glb`) deserves a look at its sheets: the build was proven
  on the peasant's source and stand-ins made from it.
