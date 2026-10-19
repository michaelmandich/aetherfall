"""The peasant armature: bone names, the character frame, rest data and FK.

Frames
------
Blender world coordinates (the armature object is never rotated):

    +X = the character's LEFT    -Y = FORWARD    +Z = UP

(the glTF exporter turns this into the contract's +Y up, facing +Z).
Everything public in `lib` takes positions as ``(right, fwd, up)`` in metres
relative to the character's ground origin, so callers never see Blender's
axes; `at()` converts.

The armature keeps the source's 0.01 object scale (the skeleton is in
centimetres, exactly as Meshy/Mixamo ship it), so pose-bone locations are in
centimetres. `Rig` hides that: its FK works in metres in world space.

Bone conventions (Mixamo): every bone's local +Y points at its child joint;
on arms and legs local +X is the hinge axis of the elbow / knee (flexion is a
rotation about -X on arms and +X on legs). The pose helper (`pose.py`) builds
on that.
"""

import math

from mathutils import Matrix, Quaternion, Vector

PREFIX = "mixamorig:"
HIPS = PREFIX + "Hips"
SPINE = (PREFIX + "Spine", PREFIX + "Spine1", PREFIX + "Spine2")
CHEST = SPINE[2]
NECK = PREFIX + "Neck"
HEAD = PREFIX + "Head"
HEADFRONT = "headfront"
SIDES = ("L", "R")
_SIDE_NAME = {"L": "Left", "R": "Right"}


def side_bone(part, side):
    """`side_bone("Hand", "R")` -> "mixamorig:RightHand"."""
    return f"{PREFIX}{_SIDE_NAME[side]}{part}"


# Per side: (clavicle, upper arm, forearm, hand) and (thigh, shin, foot, toes).
ARM = {s: tuple(side_bone(p, s) for p in ("Shoulder", "Arm", "ForeArm", "Hand")) for s in SIDES}
LEG = {s: tuple(side_bone(p, s) for p in ("UpLeg", "Leg", "Foot", "ToeBase")) for s in SIDES}
HAND = {s: ARM[s][3] for s in SIDES}

GROUPS = {
    "hips": (HIPS,),
    "spine": SPINE,
    "head": (NECK, HEAD, HEADFRONT),
    "arms": ARM["L"] + ARM["R"],
    "legs": LEG["L"] + LEG["R"],
}
GROUPS["lower"] = GROUPS["hips"] + GROUPS["legs"]
GROUPS["upper"] = GROUPS["spine"] + GROUPS["head"] + GROUPS["arms"]
GROUPS["all"] = GROUPS["lower"] + GROUPS["upper"]

# Character directions in world coordinates.
FWD = Vector((0.0, -1.0, 0.0))
BACK = -FWD
UP = Vector((0.0, 0.0, 1.0))
DOWN = -UP
LEFT = Vector((1.0, 0.0, 0.0))
RIGHT = -LEFT


def out_dir(side):
    """Unit vector pointing away from the midline on that side."""
    return LEFT.copy() if side == "L" else RIGHT.copy()


def side_sign(side):
    """+1 for the left side, -1 for the right (mirrors rotations about Z)."""
    return 1.0 if side == "L" else -1.0


def at(right=0.0, fwd=0.0, up=0.0):
    """Character-frame offset (right, fwd, up) in metres -> world vector."""
    return Vector((-right, -fwd, up))


def from_world(v):
    """World vector -> (right, fwd, up) tuple."""
    return (-v.x, -v.y, v.z)


def rot(axis, degrees):
    """3x3 rotation of `degrees` about `axis` (right-hand rule)."""
    axis = Vector(axis)
    if axis.length < 1e-9 or abs(degrees) < 1e-12:
        return Matrix.Identity(3)
    return Matrix.Rotation(math.radians(degrees), 3, axis.normalized())


def swing(from_dir, to_dir, degrees):
    """Rotation that turns `from_dir` towards `to_dir` by `degrees`."""
    axis = Vector(from_dir).cross(Vector(to_dir))
    return rot(axis, degrees)


def frame(x_axis, y_axis):
    """Rotation matrix whose columns are (x, y, x cross y), orthonormalised on y."""
    y = Vector(y_axis).normalized()
    x = Vector(x_axis)
    x = (x - y * x.dot(y)).normalized()
    z = x.cross(y)
    m = Matrix((x, y, z)).transposed()
    return m


def mat4(pos, rot3):
    """4x4 transform from a position and a 3x3 rotation."""
    m = rot3.to_4x4()
    m.translation = pos
    return m


def ortho(m):
    """Nearest proper rotation for a 3x3 that drifted (via quaternion)."""
    return m.to_quaternion().normalized().to_matrix()


class Rig:
    """Rest data of the armature and forward kinematics in metres.

    `basis` dicts map bone name -> (location, Quaternion) exactly as Blender
    pose bones store them (location in the bone's rest frame, armature units).
    FK results map bone name -> (head position in world metres, world 3x3).
    """

    def __init__(self, obj):
        self.obj = obj
        bones = obj.data.bones
        mw = obj.matrix_world
        self.scale = mw.to_scale()[0]
        self.order = []
        pending = [b for b in bones if b.parent is None]
        while pending:
            b = pending.pop(0)
            self.order.append(b.name)
            pending.extend(b.children)
        self.parent = {b.name: (b.parent.name if b.parent else None) for b in bones}
        self.rest_rot = {b.name: b.matrix_local.to_3x3().normalized() for b in bones}
        self.rest_pos = {b.name: mw @ b.head_local for b in bones}
        self.offset = {}
        self.rel_rot = {}
        for name in self.order:
            p = self.parent[name]
            if p is None:
                continue
            rp = self.rest_rot[p]
            self.offset[name] = rp.transposed() @ (self.rest_pos[name] - self.rest_pos[p])
            self.rel_rot[name] = rp.transposed() @ self.rest_rot[name]
        self.root = self.order[0]
        self.canon = self._canonical_frames()
        # Segment lengths (metres) used by IK.
        self.arm_len = {s: (self.dist(ARM[s][1], ARM[s][2]), self.dist(ARM[s][2], ARM[s][3])) for s in SIDES}
        self.leg_len = {s: (self.dist(LEG[s][0], LEG[s][1]), self.dist(LEG[s][1], LEG[s][2])) for s in SIDES}

    # -- rest geometry ---------------------------------------------------

    def dist(self, a, b):
        return (self.rest_pos[a] - self.rest_pos[b]).length

    def _canonical_frames(self):
        """The calibrated zero pose every anatomical angle is measured from.

        Spine, neck, head, clavicles, hips and feet: the rest pose (an upright
        natural posture). Arms (upper arm, forearm, hand) and legs (thigh,
        shin): hanging straight down with the hinge axis (local +X) along the
        character's left-right axis, so the elbow bends the forearm straight
        forward and the knee bends the shin straight back. Hands: in line
        with the forearm, palm facing the thigh.
        """
        straight = frame(LEFT, DOWN)
        canon = {}
        for name in self.order:
            canon[name] = self.rest_rot[name].copy()
        for s in SIDES:
            for name in ARM[s][1:] + LEG[s][:2]:
                canon[name] = straight.copy()
        return canon

    # -- conversions -----------------------------------------------------

    def basis_rot(self, name, w, w_parent):
        """World rotation of a bone (given its parent's) -> pose-bone quaternion."""
        r = self.rest_rot[name]
        p = self.parent[name]
        if p is None:
            m = r.transposed() @ w
        else:
            m = r.transposed() @ self.rest_rot[p] @ w_parent.transposed() @ w
        return ortho(m).to_quaternion()

    def anat(self, name, a, w_parent, ref=None):
        """Anatomical rotation `a` (character axes, relative to the calibrated
        zero of the parent, or of bone `ref` posed at `w_parent`) -> the
        bone's world rotation."""
        c = self.canon[name]
        p = ref or self.parent[name]
        if p is None:
            return a @ c
        return w_parent @ self.canon[p].transposed() @ a @ c

    # -- soles -------------------------------------------------------------

    def attach_soles(self, body, per_foot=160):
        """Sample the body's sole vertices (rest z < 3 cm) per foot, each rigidly
        on its dominant bone (Foot or ToeBase), for ground contact checks."""
        groups = {g.index: g.name for g in body.vertex_groups}
        mw = body.matrix_world
        self.soles = {}
        for s in SIDES:
            foot, toe = LEG[s][2], LEG[s][3]
            pts = []
            for v in body.data.vertices:
                co = mw @ v.co
                if co.z > 0.03:
                    continue
                best = max(v.groups, key=lambda g: g.weight, default=None)
                if best is None or groups[best.group] not in (foot, toe):
                    continue
                bone = groups[best.group]
                local = self.rest_rot[bone].transposed() @ (co - self.rest_pos[bone])
                pts.append((bone, local))
            step = max(1, len(pts) // per_foot)
            self.soles[s] = pts[::step]

    def sole_points(self, fk, side):
        """World positions of a foot's sole samples for an FK result."""
        return [fk[b][0] + fk[b][1] @ local for b, local in self.soles[side]]

    def root_loc(self, pos):
        """World position of the root bone's head -> pose-bone location."""
        d = (pos - self.rest_pos[self.root]) / self.scale
        return self.rest_rot[self.root].transposed() @ d

    def child_pos(self, name, pos_parent, w_parent):
        return pos_parent + w_parent @ self.offset[name]

    def fk(self, basis):
        """Pose-bone basis -> {bone: (world head position, world rotation)}."""
        out = {}
        for name in self.order:
            loc, q = basis.get(name, (Vector(), Quaternion()))
            qm = q.to_matrix()
            p = self.parent[name]
            if p is None:
                r = self.rest_rot[name]
                pos = self.rest_pos[name] + (r @ Vector(loc)) * self.scale
                w = r @ qm
            else:
                pp, wp = out[p]
                rel = self.rel_rot[name]
                pos = pp + wp @ (self.offset[name] + (rel @ Vector(loc)) * self.scale)
                w = wp @ rel @ qm
            out[name] = (pos, w)
        return out
