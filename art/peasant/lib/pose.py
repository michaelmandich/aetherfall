"""Pose helper: whole-body poses in calibrated, readable terms.

A `Pose` is a flat set of named numbers (angles in degrees, distances in
metres). Every number has a neutral default, so `Pose()` is a relaxed
standing pose; the methods below change only what they are given (None
keeps the current value) and return the pose, so calls chain. Clips key
poses at times (`Clip.key`); the numbers are interpolated with smooth curves
and `solve()` turns each frame's numbers into bone rotations, running IK
where asked. Because the numbers (not the bone quaternions) are
interpolated, swings move along arcs and IK constraints (planted feet,
both hands on a haft) hold on every frame, not only on keys.

Frame and signs (the character's own frame, wherever it faces):

* positions are ``(right, fwd, up)`` metres; "out" means away from the
  body's midline on that side, "in" towards it;
* `side` is "L" or "R" (the character's left / right);
* zero for every angle is the calibrated neutral below, verified by renders
  (see art/peasant/README.md "Calibration").

Root and torso
--------------
hips(right, fwd, up, bend, lean, turn)
    Pelvis offset from its rest position (m; `up` -0.02 by default so the
    knees are soft) and rotation. bend + tips the pelvis forward, lean +
    tilts it to the right, turn + faces it to the right. Rotating the pelvis
    carries the whole upper body; IK feet stay planted.
spine(bend, lean, turn, steady)
    Bend forward +, lean right +, turn (twist) right +, spread over Spine,
    Spine1, Spine2 (35/35/30 %), measured from the pelvis. steady (0..1)
    cancels that share of the pelvis's own turn and lean, so the chest keeps
    facing the character's forward while the pelvis swings (e.g. a walk's
    hip twist under a carried load).
head(nod, tilt, turn)
    nod + looks down, tilt + drops the right ear, turn + looks right
    (neck 40 %, head 60 %), measured from the chest.
look(at=(right, fwd, up), weight=1) / look(None)
    Aim the face at a point (root frame); head() angles are added on top.
shoulder(side, up, fwd)
    Clavicle: shrug up +, roll forward + (degrees).

Arms (FK)
---------
arm(side, lift, toward, twist)
    Upper arm, measured from the chest (a shrug does not swing it). lift =
    degrees away from hanging straight down (90 = horizontal, 180 =
    straight up) in the vertical plane that heads `toward` degrees: 0
    forward, 90 straight out to that side, -90 across the chest, 180 back.
    twist + rotates the arm outward (external rotation: a bent forearm
    swings out). Example, "raise the right arm forward 60 degrees":
    arm("R", lift=60, toward=0).
elbow(side, bend, out, twist)
    bend = elbow flexion (0 straight, 90 right angle, max ~145). `out` is
    for IK only: swings the elbow outward/upward around the
    shoulder-wrist line (degrees). twist: extra forearm roll on top of
    wrist(twist) (normally 0; Clip.fk() uses it to reproduce IK exactly).
wrist(side, flex, tilt, twist)
    flex + bends the palm side in, tilt + bends towards the thumb, twist +
    turns the palm up/forward (supination; split between forearm and hand).

Arms (IK)
---------
reach(side, at=(right, fwd, up), frame="root"|"chest", weight=1)
    Put the wrist at a point; the wrist() angles still shape the hand.
wield(at=(right, fwd, up), yaw, pitch, roll, frame="root"|"chest", weight=1)
    Pose the prop held in the hand that carries it (the clip's PROPS)
    instead of the arm: its grip point goes to `at`. A hafted tool's
    neutral is haft straight up, striking face forward; pitch + tips the
    head forward/down (90 = haft level, pointing forward), yaw + turns it to
    the right, roll + spins it about the haft. A load's neutral is upright,
    facing forward. The holding arm is solved to match; unwield() hands that
    arm back to its FK numbers.
wield_part(prop, part, at, yaw, pitch, roll)
    wield() by a point of the prop instead of the grip: `part` ("bit",
    "edge", "tip", "face", "butt" from props.PARTS, or (x, y, z) in the
    prop frame) goes to `at` (root frame), e.g. a pick's tip onto the rock.
    `Prop.angles(haft, strike)` gives the yaw/pitch/roll that point a haft
    and its striking face along two world directions (a swing plane).
hold(side, at, roll, weight=1) / release(side)
    The other hand grips the prop held by the opposite hand: on a haft, `at`
    metres along it from the main grip (+ towards the head, - towards the
    butt) and `roll` degrees around it; on a load, its fixed side grip.
    Key a constant `at` through a motion and the hands cannot drift apart.
carry(at, yaw, pitch, roll, frame="chest")
    wield() + hold("L"): a load held between both hands.

Legs
----
foot(side, out, fwd, lift, turn, heel, knee)   (IK, feet planted)
    Foot placement as an offset from the rest stance: out + widens, fwd +
    steps forward, lift raises the whole foot, turn + points the toes out,
    heel + lifts the heel pivoting on the ball of the foot; knee + swings
    the knee outward. The default is the rest stance (IK on).
leg(side, lift, toward, twist, knee, ankle)    (FK; switches the leg to FK)
    Thigh like arm() (lift 90 = thigh level forward), knee = flexion,
    ankle + = toes up. Keying foot() after leg() blends back to IK.
ankle(side, flex, turn, roll)                  (FK foot)
    flex + toes up, turn + toes out, roll + onto the outer edge.
toe(side, bend)
    Toes up +.

Misc
----
copy(), set(name, value), get(name); lerp(other, t) is the pose t of the
way to another; mirror() swaps left and right (it does not move props
between hands). ARMS, FEET and arm_parts(side) name groups of numbers for
Clip.key(parts=/skip=). `Clip.fk(pose)` (lib/clip.py)
re-expresses IK-placed limbs as the FK numbers above, so keys placed
precisely with wield/hold/foot interpolate along arcs; `fk_numbers()`
below does the conversion and `selftest.py` checks it is exact.
"""

import math

from mathutils import Euler, Matrix, Vector

from . import ik
from .props import Prop, prop_info
from .rig import (ARM, BACK, CHEST, DOWN, FWD, HEAD, HEADFRONT, HIPS, LEFT, LEG, NECK, RIGHT, SIDES,
                  SPINE, UP, at, frame, from_world, mat4, out_dir, rot, side_sign, swing)

# Groups of pose numbers for Clip.key(parts=...) / key(skip=...), e.g. key
# the body a few frames ahead of the arms (a kinetic chain).
ARMS = ("shoulder", "arm", "elbow", "wrist", "reach", "wield", "hold")  # both arms, FK and IK
FEET = ("foot", "knee.L.out", "knee.R.out", "leg.L.ik", "leg.R.ik")     # IK foot placement


def arm_parts(side):
    """One arm's numbers (FK angles and its reach/hold targets; not wield)."""
    return tuple(f"{p}.{side}" for p in ("shoulder", "arm", "elbow", "wrist", "reach", "hold"))


DEFAULTS = {}


def _param(name, value):
    DEFAULTS[name] = float(value)


for _k, _v in dict(right=0.0, fwd=0.0, up=-0.02, bend=0.0, lean=0.0, turn=0.0).items():
    _param("hips." + _k, _v)
for _k in ("bend", "lean", "turn", "steady"):
    _param("spine." + _k, 0.0)
for _k in ("nod", "tilt", "turn"):
    _param("head." + _k, 0.0)
for _k, _v in dict(w=0.0, right=0.0, fwd=3.0, up=1.4).items():
    _param("look." + _k, _v)
for _k, _v in dict(w=0.0, right=0.0, fwd=0.35, up=1.0, yaw=0.0, pitch=0.0, roll=0.0, chest=0.0).items():
    _param("wield." + _k, _v)
for _s in SIDES:
    _param(f"shoulder.{_s}.up", 0.0)
    _param(f"shoulder.{_s}.fwd", 0.0)
    _param(f"arm.{_s}.lift", 6.0)
    _param(f"arm.{_s}.toward", 75.0)
    _param(f"arm.{_s}.twist", 0.0)
    _param(f"elbow.{_s}.bend", 12.0)
    _param(f"elbow.{_s}.out", 0.0)
    _param(f"elbow.{_s}.twist", 0.0)
    _param(f"wrist.{_s}.flex", 0.0)
    _param(f"wrist.{_s}.tilt", 0.0)
    _param(f"wrist.{_s}.twist", 0.0)
    for _k, _v in dict(w=0.0, right=0.0, fwd=0.3, up=1.0, chest=0.0).items():
        _param(f"reach.{_s}.{_k}", _v)
    for _k, _v in dict(w=0.0, at=0.3, roll=0.0).items():
        _param(f"hold.{_s}.{_k}", _v)
    _param(f"leg.{_s}.ik", 1.0)
    for _k in ("out", "fwd", "lift", "turn", "heel"):
        _param(f"foot.{_s}.{_k}", 0.0)
    _param(f"knee.{_s}.out", 0.0)
    for _k in ("lift", "toward", "twist"):
        _param(f"leg.{_s}.{_k}", 0.0)
    _param(f"knee.{_s}.bend", 0.0)
    _param(f"ankle.{_s}.flex", 0.0)
    _param(f"ankle.{_s}.turn", 0.0)
    _param(f"ankle.{_s}.roll", 0.0)
    _param(f"toe.{_s}.bend", 0.0)

# Parameters whose sign flips when the pose is mirrored left <-> right.
_MIRROR_NEGATE = {"hips.right", "hips.lean", "hips.turn", "spine.lean", "spine.turn", "head.tilt",
                  "head.turn", "look.right", "wield.right", "wield.yaw", "wield.roll",
                  "reach.L.right", "reach.R.right"}

SPINE_SHARE = (0.35, 0.35, 0.30)
HEAD_SHARE = (0.4, 0.6)


def _world_point(p):
    """(right, fwd, up) -> world vector (for methods whose argument is `at`)."""
    return at(*p)


def _frame_name(frame_):
    if frame_ not in ("root", "chest"):
        raise ValueError(f"frame must be 'root' or 'chest', not {frame_!r}")
    return 1.0 if frame_ == "chest" else 0.0


class Pose:
    """See the module docstring for every call."""

    def __init__(self, values=None):
        self.v = dict(DEFAULTS)
        if values:
            for k, x in values.items():
                self.set(k, x)

    # -- generic ----------------------------------------------------------

    def copy(self):
        return Pose(self.v)

    def set(self, name, value):
        if name not in DEFAULTS:
            raise KeyError(f"unknown pose parameter {name!r}")
        self.v[name] = float(value)
        return self

    def get(self, name):
        return self.v[name]

    def lerp(self, other, t):
        """A new pose `t` of the way from this one to `other` (every number)."""
        return Pose({k: x + (other.v[k] - x) * t for k, x in self.v.items()})

    def _put(self, prefix, **kw):
        for k, x in kw.items():
            if x is not None:
                self.set(f"{prefix}.{k}", x)
        return self

    def mirror(self):
        """Swap left and right (for symmetric cycles)."""
        out = {}
        for k, x in self.v.items():
            parts = k.split(".")
            if len(parts) == 3 and parts[1] in SIDES:
                parts[1] = "R" if parts[1] == "L" else "L"
            out[".".join(parts)] = x
        for k in _MIRROR_NEGATE:
            out[k] = -out[k]
        self.v = out
        return self

    # -- root, torso, head -----------------------------------------------

    def hips(self, right=None, fwd=None, up=None, bend=None, lean=None, turn=None):
        return self._put("hips", right=right, fwd=fwd, up=up, bend=bend, lean=lean, turn=turn)

    def spine(self, bend=None, lean=None, turn=None, steady=None):
        return self._put("spine", bend=bend, lean=lean, turn=turn, steady=steady)

    def head(self, nod=None, tilt=None, turn=None):
        return self._put("head", nod=nod, tilt=tilt, turn=turn)

    def look(self, at=None, weight=1.0):
        if at is None:
            return self.set("look.w", 0.0)
        r, f, u = at
        return self._put("look", w=weight, right=r, fwd=f, up=u)

    def shoulder(self, side, up=None, fwd=None):
        return self._put(f"shoulder.{side}", up=up, fwd=fwd)

    # -- arms ---------------------------------------------------------------

    def arm(self, side, lift=None, toward=None, twist=None):
        return self._put(f"arm.{side}", lift=lift, toward=toward, twist=twist)

    def elbow(self, side, bend=None, out=None, twist=None):
        return self._put(f"elbow.{side}", bend=bend, out=out, twist=twist)

    def wrist(self, side, flex=None, tilt=None, twist=None):
        return self._put(f"wrist.{side}", flex=flex, tilt=tilt, twist=twist)

    def reach(self, side, at=None, frame="root", weight=1.0):
        if at is None:
            return self.set(f"reach.{side}.w", 0.0)
        r, f, u = at
        return self._put(f"reach.{side}", w=weight, right=r, fwd=f, up=u, chest=_frame_name(frame))

    def wield(self, at=None, yaw=None, pitch=None, roll=None, frame=None, weight=1.0):
        if at is not None:
            r, f, u = at
            self._put("wield", right=r, fwd=f, up=u)
        if frame is not None:
            self.set("wield.chest", _frame_name(frame))
        return self._put("wield", w=weight, yaw=yaw, pitch=pitch, roll=roll)

    def wield_part(self, prop, part, at, yaw=0.0, pitch=0.0, roll=0.0):
        """wield() so that a point of the prop (a name from props.PARTS, e.g.
        "bit", "tip", "edge", "face", or (x, y, z) in the prop frame) is at
        `at` (root frame) with the prop at these angles."""
        info = prop if isinstance(prop, Prop) else prop_info(prop)
        grip = info.grip_for(part, _world_point(at), yaw, pitch, roll)
        return self.wield(at=from_world(grip), yaw=yaw, pitch=pitch, roll=roll, frame="root")

    def unwield(self):
        return self.set("wield.w", 0.0)

    def hold(self, side, at=None, roll=None, weight=1.0):
        return self._put(f"hold.{side}", w=weight, at=at, roll=roll)

    def release(self, side):
        return self.set(f"hold.{side}.w", 0.0)

    def carry(self, at=None, yaw=None, pitch=None, roll=None, frame="chest", weight=1.0):
        self.wield(at=at, yaw=yaw, pitch=pitch, roll=roll, frame=frame, weight=weight)
        return self.hold("L", weight=weight)

    # -- legs ---------------------------------------------------------------

    def foot(self, side, out=None, fwd=None, lift=None, turn=None, heel=None, knee=None):
        self.set(f"leg.{side}.ik", 1.0)
        if knee is not None:
            self.set(f"knee.{side}.out", knee)
        return self._put(f"foot.{side}", out=out, fwd=fwd, lift=lift, turn=turn, heel=heel)

    def ankle(self, side, flex=None, turn=None, roll=None):
        return self._put(f"ankle.{side}", flex=flex, turn=turn, roll=roll)

    def leg(self, side, lift=None, toward=None, twist=None, knee=None, ankle=None):
        self.set(f"leg.{side}.ik", 0.0)
        if knee is not None:
            self.set(f"knee.{side}.bend", knee)
        if ankle is not None:
            self.set(f"ankle.{side}.flex", ankle)
        return self._put(f"leg.{side}", lift=lift, toward=toward, twist=twist)

    def toe(self, side, bend=None):
        return self._put(f"toe.{side}", bend=bend)


# ---------------------------------------------------------------------------
# Solver


def body_rot(bend, lean, turn):
    """Bend forward +, lean right +, turn right + (applied in that order)."""
    return rot(DOWN, turn) @ rot(FWD, lean) @ rot(LEFT, bend)


def limb_swing(side, lift, toward):
    """Swing a limb hanging straight down by `lift` in the plane heading `toward`."""
    t = math.radians(toward)
    heading = FWD * math.cos(t) + out_dir(side) * math.sin(t)
    return swing(DOWN, heading, lift)


def _foot_basis(side):
    """Axes of ankle(): flex about RIGHT, roll about FWD (mirrored), turn about UP (mirrored)."""
    sign = side_sign(side)
    return Matrix((RIGHT, FWD * sign, UP * sign)).transposed()


def ankle_rot(side, flex, turn, roll):
    b = _foot_basis(side)
    e = Euler((math.radians(flex), math.radians(-roll), math.radians(turn)), "ZYX").to_matrix()
    return b @ e @ b.transposed()


def _hand_basis(side):
    sign = side_sign(side)
    return Matrix((BACK * sign, RIGHT, UP * sign)).transposed()


def _slerp(a, b, t):
    if t >= 1.0:
        return b
    if t <= 0.0:
        return a
    return a.to_quaternion().slerp(b.to_quaternion(), t).to_matrix()


class Solver:
    """Turns evaluated pose numbers into pose-bone transforms for one rig.

    `held` maps "L"/"R" to the `props.Prop` that hand carries in this clip
    (used by wield / hold / carry). `base` (bone -> (loc, quat)) supplies the
    bones listed in `mask` from another clip instead of the pose numbers.
    """

    def __init__(self, rig, held=None):
        self.rig = rig
        self.held = dict(held or {})
        self.wield_side = "R" if "R" in self.held else ("L" if "L" in self.held else None)
        r = rig
        self._eye_rest = r.rest_pos[HEADFRONT] - r.rest_pos[CHEST]
        self._foot_fwd = {}
        for s in SIDES:
            foot, toe = LEG[s][2], LEG[s][3]
            d = r.rest_pos[toe] - r.rest_pos[foot]
            d.z = 0.0
            self._foot_fwd[s] = d.normalized()

    # -- helpers ----------------------------------------------------------

    def _put(self, name, w, pos=None):
        r = self.rig
        p = r.parent[name]
        self.W[name] = w
        if pos is None:
            pos = r.rest_pos[name] if p is None else r.child_pos(name, self.P[p], self.W[p])
        self.P[name] = pos

    def _anat(self, name, a):
        p = self.rig.parent[name]
        self._put(name, self.rig.anat(name, a, self.W[p] if p else None))

    def _from_base(self, name):
        r = self.rig
        loc, q = self.base[name]
        loc = Vector(loc)
        p = r.parent[name]
        if p is None:
            rr = r.rest_rot[name]
            self.P[name] = r.rest_pos[name] + (rr @ loc) * r.scale
            self.W[name] = rr @ q.to_matrix()
        else:
            rel = r.rel_rot[name]
            self.P[name] = self.P[p] + self.W[p] @ (r.offset[name] + (rel @ loc) * r.scale)
            self.W[name] = self.W[p] @ rel @ q.to_matrix()

    def _driven(self, name):
        return self.base is not None and name in self.mask

    def chest_frame(self):
        """(origin, rotation) of the chest frame: Spine2's head, turned by the
        chest's deviation from its calibrated neutral."""
        dev = self.W[CHEST] @ self.rig.canon[CHEST].transposed()
        return self.P[CHEST], dev

    def _place(self, right, fwd, up, chest_w):
        p_root = at(right, fwd, up)
        if chest_w <= 0.0:
            return p_root, Matrix.Identity(3)
        origin, dev = self.chest_frame()
        p_chest = origin + dev @ at(right, fwd, up)
        if chest_w >= 1.0:
            return p_chest, dev
        return p_root.lerp(p_chest, chest_w), _slerp(Matrix.Identity(3), dev, chest_w)

    # -- solve ------------------------------------------------------------

    def solve(self, v, base=None, mask=()):
        """Returns (basis, info): basis maps every bone to (location, quaternion);
        info holds IK reach errors (metres) per chain."""
        r = self.rig
        self.v, self.base, self.mask = v, base, set(mask)
        self.P, self.W = {}, {}
        self.info = {}

        # Pelvis.
        if self._driven(HIPS):
            self._from_base(HIPS)
        else:
            a = body_rot(v["hips.bend"], v["hips.lean"], v["hips.turn"])
            self._put(HIPS, r.anat(HIPS, a, None), r.rest_pos[HIPS] + at(v["hips.right"], v["hips.fwd"], v["hips.up"]))

        # Spine (optionally cancelling the pelvis's turn and lean).
        bend, lean, turn = v["spine.bend"], v["spine.lean"], v["spine.turn"]
        steady = v["spine.steady"]
        if steady > 0.0:
            dev = self.W[HIPS] @ r.canon[HIPS].transposed()
            f, u = dev @ FWD, dev @ UP
            turn -= steady * math.degrees(math.atan2(-f.x, -f.y))
            lean -= steady * math.degrees(math.atan2(-u.x, u.z))
        for name, share in zip(SPINE, SPINE_SHARE):
            if self._driven(name):
                self._from_base(name)
            else:
                self._anat(name, body_rot(share * bend, share * lean, share * turn))

        # Neck and head (with look-at).
        nod, tilt, turn = v["head.nod"], v["head.tilt"], v["head.turn"]
        lw = v["look.w"]
        if lw > 0.0:
            origin, dev = self.chest_frame()
            eye = origin + dev @ self._eye_rest
            d = dev.transposed() @ (at(v["look.right"], v["look.fwd"], v["look.up"]) - eye)
            rr, ff, uu = from_world(d)
            turn += lw * math.degrees(math.atan2(rr, ff))
            nod += lw * -math.degrees(math.atan2(uu, math.hypot(rr, ff)))
        for name, share in zip((NECK, HEAD), HEAD_SHARE):
            if self._driven(name):
                self._from_base(name)
            else:
                self._anat(name, body_rot(share * nod, share * tilt, share * turn))
        self._anat(HEADFRONT, Matrix.Identity(3))

        # Clavicles.
        for s in SIDES:
            clav = ARM[s][0]
            if self._driven(clav):
                self._from_base(clav)
            else:
                o = out_dir(s)
                self._anat(clav, swing(o, UP, v[f"shoulder.{s}.up"]) @ swing(o, FWD, v[f"shoulder.{s}.fwd"]))

        for s in SIDES:
            self._leg(s)
        self._arms()

        basis = {}
        for name in r.order:
            if self._driven(name):
                basis[name] = self.base[name]
                continue
            p = r.parent[name]
            q = r.basis_rot(name, self.W[name], self.W[p] if p else None)
            loc = r.root_loc(self.P[name]) if p is None else Vector()
            basis[name] = (loc, q)
        return basis, self.info

    # -- legs -------------------------------------------------------------

    def _leg(self, s):
        r, v = self.rig, self.v
        thigh, shin, foot, toe = LEG[s]
        if self._driven(thigh):
            for name in LEG[s]:
                self._from_base(name)
            return
        sign = side_sign(s)
        a_thigh = limb_swing(s, v[f"leg.{s}.lift"], v[f"leg.{s}.toward"]) @ rot(UP * sign, v[f"leg.{s}.twist"])
        a_shin = rot(LEFT, v[f"knee.{s}.bend"])
        a_foot = ankle_rot(s, v[f"ankle.{s}.flex"], v[f"ankle.{s}.turn"], v[f"ankle.{s}.roll"])
        w = max(0.0, min(1.0, v[f"leg.{s}.ik"]))
        if w <= 0.0:
            self._anat(thigh, a_thigh)
            self._anat(shin, a_shin)
            self._anat(foot, a_foot)
            self._anat(toe, rot(RIGHT, v[f"toe.{s}.bend"]))
            return

        # Foot placement: offsets from the rest stance, pivoting on the ball.
        o = out_dir(s)
        turn = rot(UP, sign * v[f"foot.{s}.turn"])
        ank0, ball0 = r.rest_pos[foot], r.rest_pos[toe]
        ground0 = Vector((ank0.x, ank0.y, 0.0))
        place = ground0 + o * v[f"foot.{s}.out"] + FWD * v[f"foot.{s}.fwd"]
        ball = place + turn @ (ball0 - ground0) + UP * v[f"foot.{s}.lift"]
        heel = rot(turn @ LEFT, v[f"foot.{s}.heel"])
        ankle = ball + heel @ turn @ (ank0 - ball0)
        w_foot_ik = heel @ turn @ r.rest_rot[foot]
        w_toe_ik = rot(turn @ RIGHT, v[f"toe.{s}.bend"]) @ turn @ r.rest_rot[toe]

        hip = r.child_pos(thigh, self.P[HIPS], self.W[HIPS])
        # Knee over the toes; the upward part only matters when the leg
        # reaches forward (sitting, kneeling): the knee then points up.
        pole = turn @ self._foot_fwd[s] + UP * 0.6
        dn0 = (ankle - hip).normalized()
        pole = ik.swivel(pole, dn0, o, v[f"knee.{s}.out"])
        la, lb = r.leg_len[s]
        knee, end, dn, n, err = ik.two_bone(hip, ankle, la, lb, pole)
        self.info[f"leg.{s}"] = err
        hinge = n.cross(dn)
        w_thigh_ik = frame(hinge, knee - hip)
        w_shin_ik = frame(hinge, end - knee)

        self._put(thigh, _slerp(r.anat(thigh, a_thigh, self.W[HIPS]), w_thigh_ik, w))
        self._put(shin, _slerp(r.anat(shin, a_shin, self.W[thigh]), w_shin_ik, w))
        self._put(foot, _slerp(r.anat(foot, a_foot, self.W[shin]), w_foot_ik, w))
        w_toe_fk = r.anat(toe, rot(RIGHT, v[f"toe.{s}.bend"]), self.W[foot])
        self._put(toe, _slerp(w_toe_fk, w_toe_ik, w))

    # -- arms -------------------------------------------------------------

    def _arm_fk(self, s):
        v = self.v
        sign = side_sign(s)
        tw = v[f"wrist.{s}.twist"]
        a_up = limb_swing(s, v[f"arm.{s}.lift"], v[f"arm.{s}.toward"]) @ rot(UP * sign, v[f"arm.{s}.twist"])
        a_fore = rot(RIGHT, v[f"elbow.{s}.bend"]) @ rot(UP * sign, 0.5 * tw + v[f"elbow.{s}.twist"])
        a_hand = (rot(BACK * sign, v[f"wrist.{s}.flex"]) @ rot(RIGHT, v[f"wrist.{s}.tilt"])
                  @ rot(UP * sign, 0.5 * tw))
        return a_up, a_fore, a_hand

    def hand_frame(self, s):
        """World 4x4 of a hand bone (wrist position, hand rotation)."""
        hand = ARM[s][3]
        return mat4(self.P[hand], self.W[hand])

    def prop_pose(self, prop):
        """World 4x4 of the wielded prop from the wield.* numbers."""
        v = self.v
        pos, frot = self._place(v["wield.right"], v["wield.fwd"], v["wield.up"], v["wield.chest"])
        return mat4(pos, frot @ prop.orientation(v["wield.yaw"], v["wield.pitch"], v["wield.roll"]))

    def held_prop(self, name):
        """The `props.Prop` of a prop this solver's clip holds, by name."""
        for prop in self.held.values():
            if prop.name == name:
                return prop
        raise KeyError(f"{name} is not held (held: {sorted(p.name for p in self.held.values())})")

    def prop_frame(self, name):
        """World 4x4 of a held prop where its hand actually is (after solve)."""
        prop = self.held_prop(name)
        return self.hand_frame(prop.hand) @ prop.in_hand

    def part_point(self, name, part):
        """World position of a held prop's part (props.PARTS) after solve."""
        return self.prop_frame(name) @ self.held_prop(name).part(part)

    def _arms(self):
        v = self.v
        # The hand that carries a prop is solved first; the other may grip it.
        # wield() poses the right-hand prop (or the left-hand one if that is
        # all there is); that hand is solved first so the other can grip it.
        wield_side = self.wield_side
        order = ["L", "R"] if wield_side == "L" else ["R", "L"]
        for s in order:
            target, hand_rot, weight = None, None, 0.0
            other = "L" if s == "R" else "R"
            if s == wield_side and v["wield.w"] > 0.0:
                prop = self.held[s]
                m = self.prop_pose(prop) @ prop.in_hand.inverted()
                target, hand_rot, weight = m.translation, m.to_3x3().normalized(), v["wield.w"]
            elif v[f"hold.{s}.w"] > 0.0 and other in self.held:
                prop = self.held[other]
                m = self.hand_frame(other) @ prop.in_hand @ prop.grip(v[f"hold.{s}.at"], v[f"hold.{s}.roll"])
                target, hand_rot, weight = m.translation, m.to_3x3().normalized(), v[f"hold.{s}.w"]
            elif v[f"reach.{s}.w"] > 0.0:
                target, _ = self._place(v[f"reach.{s}.right"], v[f"reach.{s}.fwd"], v[f"reach.{s}.up"],
                                        v[f"reach.{s}.chest"])
                weight = v[f"reach.{s}.w"]
            self._arm(s, target, hand_rot, min(1.0, weight))

    def _arm(self, s, target, hand_rot, weight):
        r = self.rig
        clav, upper, fore, hand = ARM[s]
        if self._driven(upper):
            for name in (upper, fore, hand):
                self._from_base(name)
            return
        a_up, a_fore, a_hand = self._arm_fk(s)
        # Upper-arm angles are measured from the chest, so shrugging or rolling
        # the clavicle moves the shoulder joint without swinging the arm.
        w_up_fk = r.anat(upper, a_up, self.W[CHEST], ref=CHEST)
        if target is None or weight <= 0.0:
            self._put(upper, w_up_fk)
            self._anat(fore, a_fore)
            self._anat(hand, a_hand)
            return
        shoulder = r.child_pos(upper, self.P[clav], self.W[clav])
        _, dev = self.chest_frame()
        o = out_dir(s)
        pole = dev @ (DOWN + BACK * 0.5 + o * 0.5).normalized()
        dn0 = (target - shoulder)
        dn0 = dn0.normalized() if dn0.length > 1e-6 else DOWN.copy()
        pole = ik.swivel(pole, dn0, dev @ (o + UP * 0.5), self.v[f"elbow.{s}.out"])
        la, lb = r.arm_len[s]
        elbow, end, dn, n, err = ik.two_bone(shoulder, target, la, lb, pole)
        self.info[f"arm.{s}"] = err
        hinge = dn.cross(n)
        w_up_ik = frame(hinge, elbow - shoulder)
        w_fore_ik = frame(hinge, end - elbow)
        if hand_rot is not None:
            rel = (w_fore_ik.transposed() @ hand_rot).to_quaternion()
            twist = 2.0 * math.atan2(rel.y, rel.w)
            if twist > math.pi:
                twist -= 2.0 * math.pi
            elif twist < -math.pi:
                twist += 2.0 * math.pi
            w_fore_ik = w_fore_ik @ rot(Vector((0.0, 1.0, 0.0)), math.degrees(0.5 * twist))
            # Wrist bend (the hand's axis against the forearm's; twist excluded).
            self.info[f"wrist.{s}"] = math.degrees(w_fore_ik.col[1].angle(hand_rot.col[1]))
        self._put(upper, _slerp(w_up_fk, w_up_ik, weight))
        self._put(fore, _slerp(r.anat(fore, a_fore, self.W[upper]), w_fore_ik, weight))
        w_hand_fk = r.anat(hand, a_hand, self.W[fore])
        if hand_rot is not None:
            self._put(hand, _slerp(w_hand_fk, hand_rot, weight))
        else:
            self._put(hand, w_hand_fk)


# ---------------------------------------------------------------------------
# IK -> FK: read a solved limb back as the FK numbers that reproduce it.


def _twist_about(m, axis):
    """Angle (degrees) of rotation `m` about `axis` (swing-twist split)."""
    q = m.to_quaternion()
    a = Vector(axis).normalized()
    proj = Vector((q.x, q.y, q.z)).dot(a)
    return math.degrees(2.0 * math.atan2(proj, q.w))


def _limb_swing_angles(side, a):
    """(lift, toward, twist) of an anatomical rotation built by
    limb_swing(side, lift, toward) @ rot(UP * side_sign, twist)."""
    d = a @ DOWN
    lift = math.degrees(math.acos(max(-1.0, min(1.0, d.dot(DOWN)))))
    h = Vector((d.x, d.y, 0.0))
    toward = 0.0
    if h.length > 1e-6:
        h.normalize()
        toward = math.degrees(math.atan2(h.dot(out_dir(side)), h.dot(FWD)))
    rest = limb_swing(side, lift, toward).transposed() @ a
    twist = _twist_about(rest, UP * side_sign(side))
    return lift, toward, twist


def fk_numbers(solver, side, limb):
    """The FK pose numbers that reproduce `solver`'s last solution of one
    limb ("arm" or "leg"), e.g. after an IK solve."""
    r, W = solver.rig, solver.W
    sign = side_sign(side)
    out = {}
    if limb == "arm":
        _, upper, fore, hand = ARM[side]
        a_up = r.canon[CHEST] @ W[CHEST].transposed() @ W[upper] @ r.canon[upper].transposed()
        lift, toward, twist = _limb_swing_angles(side, a_up)
        a_fore = r.canon[upper] @ W[upper].transposed() @ W[fore] @ r.canon[fore].transposed()
        d = a_fore @ DOWN
        bend = math.degrees(math.atan2(d.dot(FWD), d.dot(DOWN)))
        fore_twist = _twist_about(rot(RIGHT, bend).transposed() @ a_fore, UP * sign)
        a_hand = r.canon[fore] @ W[fore].transposed() @ W[hand] @ r.canon[hand].transposed()
        basis = _hand_basis(side)
        flex, tilt, hand_twist = (math.degrees(x) for x in (basis.transposed() @ a_hand @ basis).to_euler("ZYX"))
        # FK splits wrist.twist evenly between forearm and hand; elbow.twist
        # carries the forearm's remainder.
        out.update({f"arm.{side}.lift": lift, f"arm.{side}.toward": toward, f"arm.{side}.twist": twist,
                    f"elbow.{side}.bend": bend, f"elbow.{side}.twist": fore_twist - hand_twist,
                    f"wrist.{side}.flex": flex, f"wrist.{side}.tilt": tilt,
                    f"wrist.{side}.twist": 2.0 * hand_twist})
    else:
        thigh, shin, foot, toe = LEG[side]
        a_th = r.canon[HIPS] @ W[HIPS].transposed() @ W[thigh] @ r.canon[thigh].transposed()
        lift, toward, twist = _limb_swing_angles(side, a_th)
        a_sh = r.canon[thigh] @ W[thigh].transposed() @ W[shin] @ r.canon[shin].transposed()
        d = a_sh @ DOWN
        knee = math.degrees(math.atan2(d.dot(BACK), d.dot(DOWN)))
        a_ft = r.canon[shin] @ W[shin].transposed() @ W[foot] @ r.canon[foot].transposed()
        fb = _foot_basis(side)
        flex, neg_roll, turn = (math.degrees(x) for x in (fb.transposed() @ a_ft @ fb).to_euler("ZYX"))
        a_toe = r.canon[foot] @ W[foot].transposed() @ W[toe] @ r.canon[toe].transposed()
        out.update({f"leg.{side}.lift": lift, f"leg.{side}.toward": toward, f"leg.{side}.twist": twist,
                    f"knee.{side}.bend": knee, f"ankle.{side}.flex": flex, f"ankle.{side}.turn": turn,
                    f"ankle.{side}.roll": -neg_roll, f"toe.{side}.bend": _twist_about(a_toe, RIGHT),
                    f"leg.{side}.ik": 0.0})
    return out
