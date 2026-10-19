"""Clips: keyed poses -> a baked 30 fps Blender action.

A clip module (art/peasant/clips/<Name>.py) declares

    DURATION = 1.0          # seconds (a whole number of 30 fps frames);
                            # None = take it from the source clip
    LOOP = False
    HIT = 0.5               # optional: the impact time written to the manifest
    REACH = 1.1             # optional: how far ahead (metres) the work lands,
                            # written to the manifest: the engine steps the
                            # model up until its target's surface is there
    LOCOMOTION = False      # optional: measure ground speed for the manifest
    PROPS = ("Tool_attack",)  # optional: props this clip holds (the solver
                            # uses them for wield/hold/carry; sheets show them)
    SHEET_TIMES = (...)     # optional: times for the contact sheet columns
    SHEET_PROPS = (...)     # optional: props shown one per sheet column
                            # instead of PROPS (e.g. every load on one grip)
    PROPS_UNTIL = 0.35      # optional: the engine hides the props after this
                            # (checks stop testing them there)
    BEATS = (0.7,)          # optional: deliberate pauses mid-motion (a beat
                            # on the seat); the checks do not call them hitches

    def build(clip): ...    # key poses on `clip`

and `build` uses this API:

    p = clip.pose()                   # neutral standing Pose (lib.pose)
    clip.key(t, p)                    # key every pose number at t seconds
    clip.key(t, p, parts=("arm.R", "elbow.R"))   # key only some numbers
    clip.key(t, p, skip=("head",))    # key all but some (offset timing between
                                      # body parts = overlap / follow-through)
    clip.key(t, p, interp="LINEAR")   # interpolation out of this key: BEZIER
                                      # (default, auto-clamped), LINEAR,
                                      # CONSTANT, or an easing family (SINE,
                                      # QUAD, CUBIC, BACK, ...) with
                                      # easing="EASE_IN"/"EASE_OUT"/"EASE_IN_OUT"
    p2 = clip.fk(p)                   # IK-placed limbs -> FK numbers (arcs)
    s = clip.solve(p)                 # where things end up, e.g.
    s.part_point("Tool_chip_l", "butt")   #   a held prop's part (world)
    clip.base("Walk", bones="lower")  # take these bones from another built
                                      # clip (time-stretched to this length)
    clip.from_source("Walking")       # use a source clip as-is, in place
    clip.from_file("mixamo/Wave.fbx") # or a clip from another file on this
                                      # skeleton, retargeted (lib/retarget.py)

Key times are seconds; `FRAME` (1/30 s) puts keys on whole frames, so
impacts are sampled exactly (`HIT = 29 * FRAME`).

Pose numbers, not bone rotations, are interpolated (with Blender's own
F-curves, cyclic for loops), then every frame is solved and baked. Looping
clips must key every number at t=0; the t=0 value is repeated at the end so
the first and last frames are identical and tangents match across the seam.
"""

import os

import bpy
from mathutils import Quaternion, Vector

from .pose import Pose, Solver, fk_numbers
from .props import prop_info
from .rig import GROUPS, HIPS
from .source import FPS

INTERPOLATIONS = {"BEZIER", "LINEAR", "CONSTANT", "SINE", "QUAD", "CUBIC", "QUART", "QUINT", "EXPO",
                  "CIRC", "BACK", "BOUNCE", "ELASTIC"}

SOURCE_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "source")
FRAME = 1.0 / FPS  # one frame in seconds: `from lib.clip import FRAME as T`, key at `29 * T`
REACH_TOLERANCE = 0.01  # metres an IK limb may fall short before Clip.fk() refuses it


class ClipError(RuntimeError):
    pass


class Clip:
    def __init__(self, ctx, name, module):
        self.ctx = ctx
        self.name = name
        self.module = module
        self.loop = bool(getattr(module, "LOOP"))
        self.hit = getattr(module, "HIT", None)
        self.reach = getattr(module, "REACH", None)
        self.locomotion = bool(getattr(module, "LOCOMOTION", False))
        self.props = tuple(getattr(module, "PROPS", ()))
        self.sheet_times = getattr(module, "SHEET_TIMES", None)
        self.beats = tuple(getattr(module, "BEATS", ()))
        duration = getattr(module, "DURATION")
        self.duration = None
        self.frames = None
        if duration is not None:
            self._set_duration(duration)
        self.keys = {}
        self._base = None
        self._source = None
        self.basis = None       # per frame: {bone: (location, quaternion)}
        self.info = []          # per frame: solver diagnostics
        self.action = None
        self.speed = None

    def _set_duration(self, seconds):
        frames = round(seconds * FPS)
        if frames < 1 or abs(frames / FPS - seconds) > 1e-6:
            raise ClipError(f"{self.name}: DURATION {seconds} is not a whole number of {FPS} fps frames")
        self.frames = frames
        self.duration = frames / FPS

    # -- authoring API -----------------------------------------------------

    def pose(self):
        return Pose()

    def key(self, t, pose, parts=None, skip=None, interp="BEZIER", easing="AUTO"):
        if self._source is not None:
            raise ClipError(f"{self.name}: a source clip cannot also be keyed")
        if interp not in INTERPOLATIONS:
            raise ClipError(f"{self.name}: unknown interpolation {interp!r}")
        if t < -1e-9 or t > self.duration + 1e-9:
            raise ClipError(f"{self.name}: key at {t}s is outside 0..{self.duration}s")
        frame = t * FPS
        matched = False
        def match(name, prefixes):
            return any(name == p or name.startswith(p + ".") for p in prefixes)

        for name, value in pose.v.items():
            if parts and not match(name, parts):
                continue
            if skip and match(name, skip):
                continue
            matched = True
            keys = self.keys.setdefault(name, [])
            keys[:] = [k for k in keys if abs(k[0] - frame) > 1e-6]
            keys.append((frame, value, interp, easing))
        if parts and not matched:
            raise ClipError(f"{self.name}: parts {parts} match no pose numbers")

    def solve(self, pose):
        """Solve `pose` with this clip's props in hand and return the solver,
        to read where things ended up: `solver.hand_frame(side)`,
        `solver.prop_frame(name)`, `solver.part_point(name, part)` (world),
        `solver.info` (IK reach errors in metres, wrist bends in degrees)."""
        solver = Solver(self.ctx.rig, self._held())
        solver.solve(pose.v)
        return solver

    def fk(self, pose, arms=("R",), legs=(), switch=True, tolerance=REACH_TOLERANCE):
        """A copy of `pose` with the listed limbs re-expressed as FK numbers.

        Place a limb or a tool precisely with IK (wield, reach, hold, foot),
        then convert: FK numbers interpolate along arcs, IK targets along
        straight lines. A left hand holding the right hand's tool keeps its
        hold() unless "L" is listed. switch=False only fills in the FK
        numbers and leaves the limbs on IK: key that on the last IK key
        before an IK -> FK blend so both sides of the blend agree.
        Raises ClipError if a converted limb's IK target is more than
        `tolerance` metres out of reach (the FK pose would silently miss
        it): move the target or lean the body in."""
        solver = self.solve(pose)
        for chain in [f"arm.{s}" for s in arms] + [f"leg.{s}" for s in legs]:
            miss = solver.info.get(chain, 0.0)
            if miss > tolerance:
                raise ClipError(f"{self.name}: {chain} misses its IK target by {miss * 100:.1f} cm")
        out = pose.copy()
        for s in arms:
            out.v.update(fk_numbers(solver, s, "arm"))
            if switch:
                out.set(f"reach.{s}.w", 0.0)
                out.set(f"hold.{s}.w", 0.0)
                if s == solver.wield_side:
                    out.set("wield.w", 0.0)
        for s in legs:
            ik_weight = out.get(f"leg.{s}.ik")
            out.v.update(fk_numbers(solver, s, "leg"))
            if not switch:
                out.set(f"leg.{s}.ik", ik_weight)
        return out

    def _held(self):
        held = {}
        for name in self.props:
            info = prop_info(name)
            if info.hand in held:
                raise ClipError(f"{self.name}: two props in the {info.hand} hand")
            held[info.hand] = info
        return held

    def base(self, clip_name, bones="lower"):
        names = GROUPS[bones] if isinstance(bones, str) else tuple(bones)
        self._base = (self.ctx.built(clip_name), tuple(names))

    def from_source(self, action_name, in_place=True):
        """Play a clip of the Meshy source as it is (made in place, feet on
        the floor): "Walking" or "Running"."""
        self._use_source(self.ctx.source_clips[action_name], in_place)

    def from_file(self, path, action=None, in_place=True):
        """Play an action from another file (a Mixamo FBX download, or any
        glTF/FBX on this skeleton), retargeted onto the rig (lib/retarget.py).
        `path` is relative to art/peasant/source/; `action` picks one when
        the file has several."""
        actions = self.ctx.imported(path, self.loop)
        if action is None and len(actions) != 1:
            raise ClipError(f"{self.name}: {path} has actions {sorted(actions)}; name one")
        name = action or next(iter(actions))
        if name not in actions:
            raise ClipError(f"{self.name}: {path} has no action {name!r} (it has {sorted(actions)})")
        self._use_source(actions[name], in_place)

    def _use_source(self, src, in_place):
        if self.duration is None:
            self._set_duration(src.duration)
        self._source = (src, in_place)

    # -- evaluation ----------------------------------------------------------

    def evaluate(self):
        rig = self.ctx.rig
        if self._source is not None:
            self.basis = _source_frames(rig, *self._source, self.frames, self.loop)
            self.info = [{} for _ in self.basis]
            return
        if not self.keys:
            raise ClipError(f"{self.name}: build() keyed nothing")
        solver = Solver(rig, self._held())
        curves = self._curves()
        base_clip, mask = self._base if self._base else (None, ())
        self.basis, self.info = [], []
        for f in range(self.frames + 1):
            v = {name: (c.evaluate(f) if hasattr(c, "evaluate") else c) for name, c in curves.items()}
            base = base_clip.sample(f / self.frames) if base_clip else None
            basis, info = solver.solve(v, base, mask)
            self.basis.append(basis)
            self.info.append(info)
        self.ctx.drop_curves()

    def _curves(self):
        act = self.ctx.scratch_action()
        out = {}
        end = float(self.frames)
        for name, keys in sorted(self.keys.items()):
            keys = sorted(keys)
            if self.loop:
                if abs(keys[0][0]) > 1e-6:
                    raise ClipError(f"{self.name}: looping clip must key {name!r} at t=0")
                if abs(keys[-1][0] - end) < 1e-6:
                    if abs(keys[-1][1] - keys[0][1]) > 1e-6:
                        raise ClipError(f"{self.name}: {name!r} differs at t=0 and t={self.duration}")
                    keys = keys[:-1]
                keys = keys + [(end, keys[0][1], keys[0][2], keys[0][3])]
            values = {k[1] for k in keys}
            if len(values) == 1:
                out[name] = keys[0][1]
                continue
            fc = act.fcurves.new(f'["{name}"]')
            for frame, value, interp, easing in keys:
                kp = fc.keyframe_points.insert(frame, value, options={"FAST"})
                kp.interpolation = interp
                kp.easing = easing
            if self.loop:
                fc.modifiers.new("CYCLES")
            fc.update()
            out[name] = fc
        return out

    def sample(self, phase):
        """Basis at a phase in [0, 1] (interpolating between baked frames)."""
        x = max(0.0, min(1.0, phase)) * self.frames
        i = min(int(x), self.frames - 1)
        t = x - i
        a, b = self.basis[i], self.basis[i + 1]
        out = {}
        for name, (la, qa) in a.items():
            lb, qb = b[name]
            out[name] = (la.lerp(lb, t), qa.slerp(qb, t))
        return out

    # -- baking ----------------------------------------------------------------

    def bake(self, arm):
        """Write the baked frames into a Blender action (and NLA track)."""
        act = bpy.data.actions.new(self.name)
        act.use_fake_user = True
        act.use_frame_range = True
        act.frame_start = 0
        act.frame_end = self.frames
        prev = {}
        series = {}
        for f, basis in enumerate(self.basis):
            for name, (loc, q) in basis.items():
                q = Quaternion(q).normalized()
                if name in prev and prev[name].dot(q) < 0.0:
                    q = -q
                prev[name] = q
                s = series.setdefault(name, ([], []))
                s[0].append(Vector(loc))
                s[1].append(q)
        for name, (locs, quats) in series.items():
            group = name
            for idx in range(3):
                _write_curve(act, f'pose.bones["{name}"].location', idx, [l[idx] for l in locs], group)
            for idx in range(4):
                _write_curve(act, f'pose.bones["{name}"].rotation_quaternion', idx, [q[idx] for q in quats], group)
        ad = arm.animation_data or arm.animation_data_create()
        track = ad.nla_tracks.new()
        track.name = self.name
        track.strips.new(self.name, 0, act)
        track.mute = True
        self.action = act
        return act


def assign(obj, action):
    """Make `action` the object's active action (Blender 4.4 also needs its slot)."""
    ad = obj.animation_data or obj.animation_data_create()
    ad.action = action
    if action is not None and ad.action_slot is None and len(action.slots):
        ad.action_slot = action.slots[0]


def _write_curve(act, path, index, values, group):
    fc = act.fcurves.new(path, index=index, action_group=group)
    fc.keyframe_points.add(len(values))
    co = []
    for f, x in enumerate(values):
        co.extend((float(f), float(x)))
    fc.keyframe_points.foreach_set("co", co)
    for kp in fc.keyframe_points:
        kp.interpolation = "LINEAR"
    fc.update()


GROUND_CONTACT = 0.03  # a sole within this of the floor counts as touching it
GROUND_SINK = 0.003    # how far smoothing may let a sole dip below the floor


def _source_frames(rig, src, in_place, frames, loop=True):
    """Resample a source clip at 30 fps, centre the hips horizontally and put
    the feet on the floor.

    The source cycles drift a few centimetres through the floor. On every
    frame where a sole is near the floor the pelvis is raised or lowered so
    the lowest sole point is exactly on it; airborne frames (running) blend
    the correction between their neighbours; the result is smoothed. Loops
    wrap round (the last frame repeats the first); one-shot clips do not.
    """
    count = frames if loop else frames + 1
    out = [src.sample(f / frames) for f in range(count)]
    for basis in out:
        for name in rig.order:
            basis.setdefault(name, (Vector(), Quaternion()))
    rest = rig.rest_rot[HIPS]
    world = [rest @ basis[HIPS][0] for basis in out]
    if in_place:
        mean_x = sum(w.x for w in world) / count
        mean_y = sum(w.y for w in world) / count
        world = [Vector((w.x - mean_x, w.y - mean_y, w.z)) for w in world]
        for basis, w in zip(out, world):
            basis[HIPS] = (rest.transposed() @ w, basis[HIPS][1])
    lows = []
    for basis in out:
        fk = rig.fk(basis)
        lows.append(min(p.z for s in ("L", "R") for p in rig.sole_points(fk, s)))
    contact = [i for i, low in enumerate(lows) if low < GROUND_CONTACT]
    if contact:
        dz = [0.0] * count
        for i in range(count):
            if i in contact:
                dz[i] = -lows[i]
                continue
            before = [c for c in contact if c < i]
            after = [c for c in contact if c > i]
            if loop:
                prev = before[-1] if before else contact[-1] - count
                nxt = after[0] if after else contact[0] + count
            else:  # one-shot: hold the nearest contact's correction past the ends
                prev = before[-1] if before else after[0]
                nxt = after[0] if after else before[-1]
            t = (i - prev) / (nxt - prev) if nxt != prev else 0.0
            dz[i] = (1 - t) * -lows[prev % count] + t * -lows[nxt % count]

        def at(i):
            return dz[i % count] if loop else dz[max(0, min(count - 1, i))]

        dz = [0.25 * at(i - 1) + 0.5 * at(i) + 0.25 * at(i + 1) for i in range(count)]
        # Smoothing must not push a sole back through the floor.
        dz = [max(d, -low - GROUND_SINK) for d, low in zip(dz, lows)]
        for basis, w, d in zip(out, world, dz):
            w = Vector((w.x, w.y, w.z + d / rig.scale))
            basis[HIPS] = (rest.transposed() @ w, basis[HIPS][1])
    if loop:
        out.append({k: (Vector(l), Quaternion(q)) for k, (l, q) in out[0].items()})
    return out


class Context:
    """Shared state for one build: the rig, source clips and built clips."""

    def __init__(self, rig, arm, source_clips, modules, source_dir=SOURCE_DIR):
        self.rig = rig
        self.arm = arm
        self.source_clips = source_clips
        self.modules = modules
        self.source_dir = source_dir
        self.clips = {}
        self._building = []
        self._scratch = None
        self._imported = {}

    def imported(self, path, loop):
        """{action: RetargetedClip} of a file under source_dir (read once)."""
        key = (path, loop)
        if key not in self._imported:
            from .retarget import import_clips
            full = os.path.join(self.source_dir, path)
            if not os.path.isfile(full):
                raise ClipError(f"no file {full}")
            self._imported[key] = import_clips(full, self.rig, loop)
        return self._imported[key]

    def built(self, name):
        if name in self.clips:
            return self.clips[name]
        if name in self._building:
            raise ClipError(f"clip dependency cycle: {' -> '.join(self._building + [name])}")
        if name not in self.modules:
            raise ClipError(f"no clip module named {name!r}")
        self._building.append(name)
        clip = Clip(self, name, self.modules[name])
        self.modules[name].build(clip)
        clip.evaluate()
        self._building.pop()
        self.clips[name] = clip
        return clip

    def scratch_action(self):
        if self._scratch is None:
            self._scratch = bpy.data.actions.new("_pose_numbers")
        return self._scratch

    def drop_curves(self):
        if self._scratch is not None:
            bpy.data.actions.remove(self._scratch)
            self._scratch = None
