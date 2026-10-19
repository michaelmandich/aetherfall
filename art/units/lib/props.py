"""Props: boxy tools, weapons and cargo parented to the hands, and the team sash.

The library (`KINDS`) holds every design; a unit names the ones it carries
in its config (`PROPS`: node name -> kind, `use()`), and `prop_info()`
answers for those names or for a kind by its own name. Every prop is built
from boxes in its own *prop frame* and parented to a hand bone. Two kinds:

* hafted (``kind="haft"``: tools, weapons, the bow, the shield's grip): the
  prop frame's origin is the hand's grip, just above the butt end; +Y runs
  along the haft towards the head, +X is the striking direction (blade
  edge, hammer face, hoe blade; the way a bow shoots), +Z = X x Y. The other hand grips the same haft `at` metres
  further up (`Pose.hold`; slide it by keying `at`).
* cargo (``kind="load"``): the origin is the load's centre; held upright at
  the chest its axes are the character's (+X left, +Y back, +Z up) and the
  hands press its left and right faces (`Pose.carry`).

The *fist frame* ties a prop frame to a hand bone: with the hand hanging
(palm against the thigh), the fist's +X (striking direction) points along
the fingers, its +Y (haft) out of the thumb side, so a hafted tool held at
the side hangs with its haft pointing forward. Both hands use the same
fist frame, so two hands on one haft hold it like a bat (palms opposite).
The FIST_* offsets, GRIP_OBLIQUITY and the per-tool mount rolls were tuned
by calibration renders and IK searches (README "Calibration"): with a
mount of -90 the hoe and shovel lie along the forearm in a broom grip, the
lower hand palm-up under the shaft (hold(roll=180)).

Visibility: all props are exported visible. The Bevy 0.19 glTF loader
ignores node-visibility extensions (KHR_node_visibility), so the engine
hides every `Tool_*` and `Load_*` node when it spawns the model and shows
the ones it needs; `Gear_*` nodes (a soldier's weapon, a shield) and
`Team_sash` always stay visible.
"""

import math
import os

import bmesh
import bpy
from mathutils import Euler, Matrix, Vector

from .rig import ARM, DOWN, HAND, HEAD, HEADFRONT, HIPS, LEFT, NECK, SIDES, SPINE, UP, frame, from_world, mat4, rot

X_AXIS = Vector((1.0, 0.0, 0.0))  # prop frame: striking direction
Y_AXIS = Vector((0.0, 1.0, 0.0))  # prop frame: along the haft, towards the head
Z_AXIS = Vector((0.0, 0.0, 1.0))

# Hand-local (metres) centre of a gripping fist: along the fingers (+Y) and
# out of the palm. The palm faces local +X on the right hand, -X on the left.
FIST_ALONG = 0.085
FIST_PALM = 0.030
FIST_THUMB = -0.012
PALM_SIGN = {"R": 1.0, "L": -1.0}
# A power grip holds a handle obliquely across the palm (heel of the hand to
# the index knuckle): the haft leaves the thumb side tilted this many degrees
# towards the fingertips, which keeps wrists straighter in every swing.
GRIP_OBLIQUITY = 25.0

# Load grip: hands on the load's side faces, fingers pointing forward-down.
LOAD_HALF_WIDTH = 0.165
LOAD_FINGER_DROP = 20.0  # degrees below horizontal: steeper bends the wrists past their range
                         # while carrying, shallower makes the floor set-down (Drop) unreachable


def fist_frame(side, oblique=GRIP_OBLIQUITY):
    """4x4: the fist frame expressed in the hand bone's frame (metres).
    Axes (hand-local): +Y (haft) out of the thumb side (-Z) tilted `oblique`
    degrees towards the fingers (+Y); +X (striking direction) along the
    fingers; +Z = X x Y, away from the palm on both hands' -X."""
    a = math.radians(oblique)
    haft = Vector((0.0, math.sin(a), -math.cos(a)))
    strike = Vector((0.0, math.cos(a), math.sin(a)))
    m = frame(strike, haft).to_4x4()
    m.translation = Vector((PALM_SIGN[side] * FIST_PALM, FIST_ALONG, FIST_THUMB))
    return m


def _load_hand(side):
    """The hand bone frame relative to a carried load's frame."""
    x = -LOAD_HALF_WIDTH if side == "R" else LOAD_HALF_WIDTH
    drop = math.radians(LOAD_FINGER_DROP)
    fingers = Vector((0.0, -math.cos(drop), -math.sin(drop)))  # fist +X (forward, down)
    fist_z = Vector((-1.0, 0.0, 0.0))  # fist +Z = -hand X; palms face the load
    fist_y = fist_z.cross(fingers)
    fist = frame(fingers, fist_y).to_4x4()
    fist.translation = Vector((x, 0.0, 0.0))
    return fist @ fist_frame(side, oblique=0.0).inverted()  # flat hands: no power grip


class Prop:
    """A prop's grip data (the solver's view of it). `mount` (degrees) turns a
    hafted tool about its haft inside the fist, chosen so that the hands are
    comfortable when wield(roll=0) shows the tool the natural way round.
    `parts` names points in the prop frame (`PARTS`); `strike` is the one
    that does the work (checked for speed at the clip's HIT)."""

    def __init__(self, name, hand, kind, neutral, mount=0.0, parts=None, strike=None):
        self.name = name
        self.hand = hand
        self.kind = kind
        self.neutral = neutral
        self.parts = {k: Vector(v) for k, v in (parts or {}).items()}
        self.strike = strike
        if kind == "load":
            self.in_hand = _load_hand(hand).inverted()
            other = "L" if hand == "R" else "R"
            self._other = _load_hand(other)
        else:
            self.in_hand = fist_frame(hand) @ rot(Vector((0.0, 1.0, 0.0)), mount).to_4x4()
            self._mount = mount
            self._other = None

    def grip(self, along=0.0, roll=0.0):
        """The other hand's bone frame relative to this prop's frame."""
        if self.kind == "load":
            return self._other.copy()
        other = "L" if self.hand == "R" else "R"
        m = Matrix.Translation(Vector((0.0, along, 0.0))) @ rot(Y_AXIS, roll - self._mount).to_4x4()
        return m @ fist_frame(other).inverted()

    # -- placing the prop by its own geometry (used by Pose.wield_part) -----

    def part(self, part):
        """A point in the prop frame: a name from `PARTS` or (x, y, z) metres."""
        if isinstance(part, str):
            if part not in self.parts:
                raise KeyError(f"{self.name} has no part {part!r}; it has {sorted(self.parts)}")
            return self.parts[part].copy()
        return Vector(part)

    def orientation(self, yaw=0.0, pitch=0.0, roll=0.0):
        """World rotation of the prop frame for wield(yaw, pitch, roll) in the
        root frame: the neutral (haft up, striking face forward; a load
        upright) pitched forward, turned right, rolled about the haft."""
        return rot(DOWN, yaw) @ rot(LEFT, pitch) @ self.neutral @ rot(Y_AXIS, roll)

    def angles(self, haft, strike):
        """(yaw, pitch, roll) for wield() that point the prop's +Y along the
        world direction `haft` and its +X (striking direction) as close to
        `strike` as the haft allows."""
        haft = Vector(haft).normalized()
        pitch = math.degrees(math.acos(max(-1.0, min(1.0, haft.dot(UP)))))
        r, f, _ = from_world(haft)
        yaw = math.degrees(math.atan2(r, f))
        base = self.orientation(yaw, pitch, 0.0)
        x0, z0 = base @ X_AXIS, base @ Z_AXIS
        strike = Vector(strike)
        return yaw, pitch, math.degrees(math.atan2(-strike.dot(z0), strike.dot(x0)))

    def grip_for(self, part, at, yaw=0.0, pitch=0.0, roll=0.0):
        """The grip position (world vector) that puts `part` at the world
        point `at` with the prop at these wield() angles."""
        return Vector(at) - self.orientation(yaw, pitch, roll) @ self.part(part)


# ---------------------------------------------------------------------------
# Materials and boxes

_PALETTE = {
    # name: (base colour, roughness, metallic, emission colour, emission strength)
    "PropWood": ((0.42, 0.27, 0.14), 0.85, 0.0, None, 0.0),
    "PropWoodLight": ((0.70, 0.55, 0.36), 0.85, 0.0, None, 0.0),
    "PropIron": ((0.36, 0.37, 0.39), 0.55, 0.0, None, 0.0),
    "PropSteel": ((0.66, 0.68, 0.70), 0.45, 0.0, None, 0.0),
    "PropRope": ((0.62, 0.52, 0.34), 0.9, 0.0, None, 0.0),
    "PropWicker": ((0.66, 0.50, 0.26), 0.9, 0.0, None, 0.0),
    "PropSack": ((0.74, 0.63, 0.44), 0.95, 0.0, None, 0.0),
    "PropSand": ((0.88, 0.81, 0.60), 0.95, 0.0, None, 0.0),
    "PropBerry": ((0.70, 0.08, 0.12), 0.6, 0.0, None, 0.0),
    "PropLeaf": ((0.25, 0.52, 0.18), 0.8, 0.0, None, 0.0),
    "PropStone": ((0.56, 0.56, 0.54), 0.9, 0.0, None, 0.0),
    "PropCopper": ((0.76, 0.42, 0.20), 0.5, 0.0, None, 0.0),
    "PropPatina": ((0.30, 0.62, 0.52), 0.7, 0.0, None, 0.0),
    "PropOre": ((0.26, 0.26, 0.28), 0.7, 0.0, None, 0.0),
    "PropRust": ((0.50, 0.27, 0.15), 0.8, 0.0, None, 0.0),
    # Aether and adamant in the colours the game draws their nodes in
    # (assets/data/core/nodes.ron, af_render parts.rs: the violet crystal,
    # the charred meteor with its gold ore), so a load reads as what it was
    # cut from. The game's colours are sRGB; these are their linear values
    # (sRGB (0.75, 0.45, 1.0), (0.32, 0.28, 0.21) and (0.95, 0.85, 0.45)).
    "PropAether": ((0.52, 0.17, 1.00), 0.3, 0.0, (0.52, 0.17, 1.00), 4.0),
    "PropAdamant": ((0.084, 0.064, 0.036), 0.6, 0.0, None, 0.0),
    "PropAdamantOre": ((0.89, 0.69, 0.17), 0.35, 0.0, None, 0.0),
    # The bronze of the spear's look in assets/data/core/weapons.ron (sRGB
    # (0.8, 0.55, 0.25)), in linear values.
    "PropBronze": ((0.61, 0.27, 0.047), 0.45, 0.0, None, 0.0),
    "PropString": ((0.85, 0.80, 0.66), 0.9, 0.0, None, 0.0),
    "PropLeather": ((0.24, 0.13, 0.06), 0.85, 0.0, None, 0.0),
    "Team": ((1.0, 1.0, 1.0), 0.75, 0.0, None, 0.0),
}


def material(name):
    """Get or create one of the palette's materials (plain Principled BSDF)."""
    mat = bpy.data.materials.get(name)
    if mat is not None:
        return mat
    color, rough, metal, emission, strength = _PALETTE[name]
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Roughness"].default_value = rough
    bsdf.inputs["Metallic"].default_value = metal
    if emission:
        bsdf.inputs["Emission Color"].default_value = (*emission, 1.0)
        bsdf.inputs["Emission Strength"].default_value = strength
    mat.diffuse_color = (*color, 1.0)
    mat.use_backface_culling = True  # closed boxes: exported single-sided
    return mat


def emissive_materials():
    """Names of the palette materials that are meant to glow."""
    return {name for name, spec in _PALETTE.items() if spec[3] is not None}


class Boxes:
    """Accumulates axis boxes (optionally rotated) into one bmesh."""

    def __init__(self):
        self.bm = bmesh.new()
        self.materials = []

    def box(self, mat, center, size, rot_deg=(0.0, 0.0, 0.0)):
        if mat not in self.materials:
            self.materials.append(mat)
        idx = self.materials.index(mat)
        m = (Matrix.Translation(Vector(center)) @ Euler([math.radians(a) for a in rot_deg]).to_matrix().to_4x4()
             @ Matrix.Diagonal((*size, 1.0)))
        res = bmesh.ops.create_cube(self.bm, size=1.0, matrix=m)
        faces = {f for v in res["verts"] for f in v.link_faces}
        for f in faces:
            f.material_index = idx
            f.smooth = False
        return self

    def haft(self, y0, y1, thick=0.034, mat="PropWood"):
        return self.box(mat, (0.0, (y0 + y1) / 2, 0.0), (thick, y1 - y0, thick))

    def disc(self, mat, center, radius, depth, segments=12):
        """A flat prism of `segments` sides, its axis along the prop's X."""
        if mat not in self.materials:
            self.materials.append(mat)
        idx = self.materials.index(mat)
        m = Matrix.Translation(Vector(center)) @ Matrix.Rotation(math.radians(90.0), 4, "Y")
        res = bmesh.ops.create_cone(self.bm, cap_ends=True, cap_tris=False, segments=segments, radius1=radius,
                                    radius2=radius, depth=depth, matrix=m)
        for f in {f for v in res["verts"] for f in v.link_faces}:
            f.material_index = idx
            f.smooth = False
        return self

    def bar(self, mat, a, b, width, depth):
        """A box from point `a` to point `b` (its length along a -> b), `width`
        across in the plane of a, b and the prop's Z, `depth` along Z (for a
        bar in the X-Y plane; otherwise the box's own Z is as near the
        prop's Z as the bar allows)."""
        a, b = Vector(a), Vector(b)
        y = (b - a).normalized()
        x = Z_AXIS.cross(y) if abs(y.dot(Z_AXIS)) < 0.999 else X_AXIS.copy()
        m = frame(x, y).to_4x4()
        m.translation = (a + b) / 2
        if mat not in self.materials:
            self.materials.append(mat)
        idx = self.materials.index(mat)
        res = bmesh.ops.create_cube(self.bm, size=1.0, matrix=m @ Matrix.Diagonal((width, (b - a).length, depth, 1.0)))
        for f in {f for v in res["verts"] for f in v.link_faces}:
            f.material_index = idx
            f.smooth = False
        return self

    def to_object(self, name):
        me = bpy.data.meshes.new(name)
        self.bm.to_mesh(me)
        self.bm.free()
        for m in self.materials:
            me.materials.append(material(m))
        obj = bpy.data.objects.new(name, me)
        bpy.context.scene.collection.objects.link(obj)
        return obj


# ---------------------------------------------------------------------------
# The prop set (sizes in metres for a 1.6 m character)


def _work_axe():
    b = Boxes().haft(-0.06, 0.56, 0.032)
    b.box("PropIron", (0.075, 0.50, 0.0), (0.11, 0.075, 0.022))     # cheek
    b.box("PropSteel", (0.14, 0.50, 0.0), (0.03, 0.12, 0.018))      # bit (edge, +X)
    b.box("PropIron", (-0.012, 0.50, 0.0), (0.035, 0.07, 0.04))    # poll
    return b


def _felling_axe():
    b = Boxes().haft(-0.07, 0.86, 0.036)
    b.box("PropIron", (0.09, 0.79, 0.0), (0.13, 0.09, 0.026))
    b.box("PropSteel", (0.17, 0.79, 0.0), (0.035, 0.16, 0.02))
    b.box("PropIron", (-0.015, 0.79, 0.0), (0.045, 0.085, 0.048))
    return b


def _hoe():
    b = Boxes().haft(-0.07, 1.22, 0.032)
    b.box("PropIron", (0.02, 1.22, 0.0), (0.06, 0.06, 0.05))        # socket
    b.box("PropIron", (0.10, 1.235, 0.0), (0.16, 0.012, 0.15))     # blade plate, edge at +X
    return b


def _pickaxe():
    b = Boxes().haft(-0.07, 0.80, 0.036)
    b.box("PropIron", (0.0, 0.74, 0.0), (0.09, 0.08, 0.06))         # eye
    b.box("PropIron", (0.15, 0.735, 0.0), (0.22, 0.05, 0.045))      # front pick
    b.box("PropSteel", (0.29, 0.725, 0.0), (0.07, 0.03, 0.03), (0.0, 0.0, -8.0))
    b.box("PropIron", (-0.14, 0.735, 0.0), (0.20, 0.05, 0.045))     # back adze
    b.box("PropSteel", (-0.27, 0.73, 0.0), (0.06, 0.03, 0.05))
    return b


def _shovel():
    b = Boxes().haft(-0.07, 0.86, 0.034)
    b.box("PropWood", (0.0, -0.075, 0.0), (0.04, 0.03, 0.13))       # cross grip at the top
    b.box("PropIron", (0.0, 0.87, 0.0), (0.05, 0.06, 0.06))         # socket
    b.box("PropIron", (0.012, 1.03, 0.0), (0.014, 0.26, 0.22), (0.0, 0.0, 8.0))  # blade, scoops +X
    return b


def _mallet():
    b = Boxes().haft(-0.07, 0.22, 0.03)
    b.box("PropWoodLight", (0.0, 0.26, 0.0), (0.13, 0.085, 0.085))
    return b


def _chisel():
    b = Boxes()
    b.box("PropWood", (0.0, 0.03, 0.0), (0.028, 0.10, 0.028))       # handle in the fist
    b.box("PropSteel", (0.0, -0.07, 0.0), (0.018, 0.12, 0.012))     # shank, edge below the fist
    b.box("PropSteel", (0.0, -0.135, 0.0), (0.022, 0.02, 0.006))
    return b


def _sledge():
    b = Boxes().haft(-0.07, 0.86, 0.038)
    b.box("PropIron", (0.0, 0.88, 0.0), (0.24, 0.10, 0.10))
    b.box("PropSteel", (0.125, 0.88, 0.0), (0.012, 0.09, 0.09))
    b.box("PropSteel", (-0.125, 0.88, 0.0), (0.012, 0.09, 0.09))
    return b


def _hammer():
    b = Boxes().haft(-0.06, 0.27, 0.028)
    b.box("PropIron", (0.01, 0.27, 0.0), (0.10, 0.035, 0.035))
    b.box("PropSteel", (0.065, 0.27, 0.0), (0.012, 0.04, 0.04))
    b.box("PropIron", (-0.06, 0.255, 0.0), (0.04, 0.02, 0.025), (0.0, 0.0, -25.0))   # claw
    return b


def _basket():
    # Held by its handle in the left fist; hangs on the fist's +X (fingers).
    b = Boxes()
    b.box("PropWicker", (0.19, 0.0, 0.0), (0.20, 0.30, 0.22))
    b.box("PropWood", (0.09, 0.0, 0.0), (0.02, 0.32, 0.235))        # rim
    b.box("PropWicker", (0.03, 0.14, 0.0), (0.12, 0.02, 0.02))      # handle uprights
    b.box("PropWicker", (0.03, -0.14, 0.0), (0.12, 0.02, 0.02))
    b.box("PropWicker", (-0.025, 0.0, 0.0), (0.02, 0.30, 0.02))     # handle bar (in the fist)
    b.box("PropBerry", (0.085, 0.06, 0.04), (0.04, 0.04, 0.04))
    b.box("PropBerry", (0.085, -0.05, -0.05), (0.04, 0.04, 0.04))
    b.box("PropLeaf", (0.085, -0.02, 0.06), (0.03, 0.06, 0.04))
    return b


# Loads: centred on the origin, width (X) under 2 * LOAD_HALF_WIDTH.
def _load_food():
    b = Boxes()
    b.box("PropWicker", (0.0, 0.0, -0.03), (0.27, 0.24, 0.16))
    b.box("PropBerry", (0.06, 0.04, 0.07), (0.07, 0.07, 0.07), (0, 0, 20))
    b.box("PropBerry", (-0.06, -0.04, 0.075), (0.07, 0.07, 0.07), (0, 0, -15))
    b.box("PropLeaf", (-0.05, 0.06, 0.07), (0.08, 0.06, 0.05), (0, 0, 30))
    b.box("PropBerry", (0.02, -0.06, 0.09), (0.06, 0.06, 0.06), (0, 0, 45))
    return b


def _load_wood():
    # Three 40 cm logs, held behind their middle (they reach further forward
    # than back, so the near ends stop short of the belly).
    length, ahead = 0.40, 0.05
    b = Boxes()
    for x, z in ((-0.065, -0.05), (0.065, -0.05), (0.0, 0.055)):
        b.box("PropWood", (x, -ahead, z), (0.115, length, 0.105))
        for y in (-length / 2 - 0.002, length / 2 + 0.002):
            b.box("PropWoodLight", (x, y - ahead, z), (0.095, 0.006, 0.085))
    return b


def _load_stone():
    return Boxes().box("PropStone", (0.0, 0.0, 0.0), (0.27, 0.22, 0.2)).box(
        "PropStone", (0.03, -0.02, 0.11), (0.16, 0.14, 0.05), (0, 0, 15))


def _ore(main, accent):
    b = Boxes()
    b.box(main, (0.0, 0.0, -0.01), (0.25, 0.2, 0.17), (8, 0, 12))
    b.box(main, (-0.05, 0.02, 0.09), (0.12, 0.12, 0.1), (0, 20, 30))
    b.box(accent, (0.07, -0.05, 0.075), (0.09, 0.08, 0.07), (15, 0, -20))
    return b


def _load_silicon():
    b = Boxes()
    b.box("PropSand", (0.0, 0.0, -0.01), (0.27, 0.22, 0.24))
    b.box("PropRope", (0.0, 0.0, 0.125), (0.10, 0.08, 0.03))
    b.box("PropSand", (0.0, 0.0, 0.155), (0.06, 0.05, 0.04))
    return b


def _load_aether():
    b = Boxes()
    b.box("PropStone", (0.0, 0.0, -0.06), (0.24, 0.2, 0.08))
    for x, y, h, rx, ry in ((0.0, 0.0, 0.26, 0, 0), (0.07, 0.03, 0.18, 0, 22), (-0.07, -0.02, 0.2, 0, -25),
                            (0.02, -0.06, 0.15, 25, 0), (-0.02, 0.07, 0.16, -22, 0)):
        b.box("PropAether", (x, y, -0.03 + h / 2), (0.055, 0.055, h), (rx, ry, 45))
    return b


def _load_adamant():
    b = Boxes()
    b.box("PropAdamant", (0.0, 0.0, 0.0), (0.24, 0.2, 0.18), (0, 0, 10))
    b.box("PropAdamantOre", (0.0, 0.0, 0.095), (0.15, 0.12, 0.02), (0, 0, 10))
    b.box("PropAdamantOre", (0.05, 0.05, 0.11), (0.1, 0.09, 0.06), (10, 0, -20))
    return b


# ---------------------------------------------------------------------------
# Weapons (the soldiers'). Sizes for a 1.65 m figure, from the game's weapon
# boxes (assets/data/core/weapons.ron `look`, scaled from the 1.4 m unit box).


def _spear():
    # A 2.15 m bronze-tipped spear, gripped 0.7 m above the butt: the hand
    # that holds it alone (upright at the side) is the rear hand of a thrust.
    b = Boxes().haft(-0.70, 1.20, 0.034)
    b.box("PropBronze", (0.0, -0.715, 0.0), (0.042, 0.03, 0.042))    # butt cap
    b.box("PropBronze", (0.0, 1.215, 0.0), (0.046, 0.06, 0.046))     # socket
    b.box("PropBronze", (0.0, 1.31, 0.0), (0.075, 0.14, 0.016))      # leaf blade, flat in the strike plane
    b.box("PropBronze", (0.0, 1.405, 0.0), (0.038, 0.07, 0.013))     # point
    return b


def _bow():
    # A 1.1 m short bow in the left fist. The back faces +X (the way the
    # arrow flies when the bow arm points at the target); the limbs sweep
    # back toward the archer and the string runs between their tips.
    b = Boxes()
    b.box("PropWood", (0.0, 0.0, 0.0), (0.042, 0.13, 0.036))         # grip
    b.box("PropLeather", (0.0, 0.0, 0.0), (0.046, 0.07, 0.04))       # grip wrap
    for sign in (1.0, -1.0):
        pts = ((0.0, 0.06), (-0.035, 0.30), (-0.095, 0.50), (-0.13, 0.56))
        for (x0, y0), (x1, y1), t in zip(pts, pts[1:], (0.03, 0.024, 0.018)):
            b.bar("PropWoodLight", (x0, sign * y0, 0.0), (x1, sign * y1, 0.0), t, 0.026)
    b.bar("PropString", (-0.125, 0.555, 0.0), (-0.125, -0.555, 0.0), 0.006, 0.006)
    return b


def _crossbow():
    # Gripped by the tiller behind the trigger; the tiller runs forward
    # (+Y) to the prod, which spans the prop's Z; the bolt lies on the +X
    # side, spanned and loaded.
    b = Boxes()
    b.box("PropWood", (0.0, 0.03, 0.0), (0.05, 0.62, 0.045))          # tiller, butt at -0.28
    b.box("PropWood", (-0.012, -0.22, 0.0), (0.085, 0.14, 0.05))      # stock end, deeper
    b.box("PropIron", (-0.035, 0.03, 0.0), (0.03, 0.05, 0.012))       # trigger lever
    for sign in (1.0, -1.0):
        b.bar("PropIron", (0.0, 0.36, 0.0), (0.0, 0.31, sign * 0.30), 0.03, 0.025)    # prod
        b.bar("PropString", (0.012, 0.31, sign * 0.29), (0.012, 0.10, 0.0), 0.005, 0.005)
    b.box("PropIron", (0.0, 0.42, 0.0), (0.012, 0.08, 0.07))           # stirrup
    b.box("PropWood", (0.032, 0.25, 0.0), (0.014, 0.30, 0.014))        # bolt
    b.box("PropIron", (0.032, 0.405, 0.0), (0.02, 0.03, 0.02))         # bolt head
    return b


def _sling():
    # Two cords from the fist, hanging along the fingers (+X), and the
    # pouch with its stone. Whirled, it stays straight: from the RTS camera
    # it is a short line off the hand.
    b = Boxes()
    for z in (-0.012, 0.012):
        b.box("PropRope", (0.22, 0.0, z), (0.40, 0.008, 0.008))
    b.box("PropLeather", (0.445, 0.0, 0.0), (0.06, 0.045, 0.05))
    b.box("PropStone", (0.45, 0.0, 0.0), (0.042, 0.042, 0.042))
    return b


def _round_shield():
    # A 0.62 m centre-grip shield in the left fist: the face is square to
    # the forearm (+X), in the team's colour, with an iron boss.
    b = Boxes()
    b.box("PropWood", (0.0, 0.0, 0.0), (0.03, 0.14, 0.03))     # the grip bar, in the fist
    b.disc("PropWood", (0.055, 0.0, 0.0), 0.31, 0.024)          # planks and rim
    b.disc("Team", (0.069, 0.0, 0.0), 0.27, 0.006)              # painted face
    b.disc("PropIron", (0.08, 0.0, 0.0), 0.07, 0.03, 8)         # boss
    return b


# ---------------------------------------------------------------------------
# The prop library. Each kind is a design: its boxes, the hand that holds
# it, how it sits in the fist (`mount`, degrees about the haft) and its
# named points. A unit names its props in its config (`PROPS`: node name
# -> kind); the engine knows props by node name (docs/art/peasant.md):
# `Tool_<role>` and `Load_<resource>` nodes are shown and hidden by what
# the unit does, every other node (`Gear_spear`, `Gear_shield`) always
# shows.


class Kind:
    def __init__(self, builder, hand, mount=0.0, parts=None, strike=None, load=False):
        self.builder = builder
        self.hand = hand
        self.mount = mount
        self.parts = parts or {}
        self.strike = strike
        self.load = load

    def make(self, name):
        """The prop's object `name`, its mesh in the prop frame."""
        return self.builder().to_object(name)


LOAD_HAND = "R"

# Named points of the props, in the prop frame (metres), measured off the
# boxes above: clips aim these at the work (`Pose.wield_part`), and the
# build's checks report the speed of the striking one (`strike`) at HIT.
KINDS = {
    "work_axe": Kind(_work_axe, "R", parts={"bit": (0.155, 0.50, 0.0)}, strike="bit"),   # centre of the edge
    "felling_axe": Kind(_felling_axe, "R", parts={"bit": (0.1875, 0.79, 0.0)}, strike="bit"),
    # Broom-like grip: the hoe and the shovel lie along the forearm.
    "hoe": Kind(_hoe, "R", -90.0, parts={"edge": (0.18, 1.235, 0.0)}, strike="edge"),   # the blade's edge
    "pickaxe": Kind(_pickaxe, "R", parts={"tip": (0.325, 0.72, 0.0)}, strike="tip"),    # the front pick's point
    "shovel": Kind(_shovel, "R", -90.0, parts={"tip": (0.0, 1.16, 0.0)}, strike="tip"),  # the blade's tip
    "mallet": Kind(_mallet, "R", parts={"face": (0.065, 0.26, 0.0)}, strike="face"),
    # Chisel: the struck end and the cutting end.
    "chisel": Kind(_chisel, "L", parts={"butt": (0.0, 0.08, 0.0), "edge": (0.0, -0.145, 0.0)}),
    "sledgehammer": Kind(_sledge, "R", parts={"face": (0.131, 0.88, 0.0)}, strike="face"),
    "hammer": Kind(_hammer, "R", parts={"face": (0.071, 0.27, 0.0)}, strike="face"),
    "basket": Kind(_basket, "L"),
    "load_food": Kind(_load_food, LOAD_HAND, load=True),
    "load_wood": Kind(_load_wood, LOAD_HAND, load=True),
    "load_stone": Kind(_load_stone, LOAD_HAND, load=True),
    "load_copper": Kind(lambda: _ore("PropCopper", "PropPatina"), LOAD_HAND, load=True),
    "load_iron": Kind(lambda: _ore("PropOre", "PropRust"), LOAD_HAND, load=True),
    "load_silicon": Kind(_load_silicon, LOAD_HAND, load=True),
    "load_aether": Kind(_load_aether, LOAD_HAND, load=True),
    "load_adamant": Kind(_load_adamant, LOAD_HAND, load=True),
    "spear": Kind(_spear, "R", parts={"tip": (0.0, 1.44, 0.0), "butt": (0.0, -0.73, 0.0)}, strike="tip"),
    "bow": Kind(_bow, "L", parts={"string": (-0.125, 0.0, 0.0), "tip": (-0.13, 0.56, 0.0)}),
    "crossbow": Kind(_crossbow, "R", parts={"bolt": (0.032, 0.42, 0.0), "butt": (0.0, -0.28, 0.0)}),
    "sling": Kind(_sling, "R", parts={"pouch": (0.45, 0.0, 0.0)}, strike="pouch"),
    "round_shield": Kind(_round_shield, "L", parts={"boss": (0.10, 0.0, 0.0)}),
}

# The peasant's props: a tool per work role, the Work Axe, and the cargo.
WORKER = {
    "Tool_attack": "work_axe",
    "Tool_chop": "felling_axe",
    "Tool_farm": "hoe",
    "Tool_mine": "pickaxe",
    "Tool_dig": "shovel",
    "Tool_chip": "mallet",
    "Tool_chip_l": "chisel",
    "Tool_smash": "sledgehammer",
    "Tool_build": "hammer",
    "Tool_forage": "basket",
    "Load_food": "load_food",
    "Load_wood": "load_wood",
    "Load_stone": "load_stone",
    "Load_copper": "load_copper",
    "Load_iron": "load_iron",
    "Load_silicon": "load_silicon",
    "Load_aether": "load_aether",
    "Load_adamant": "load_adamant",
}

# The props of the unit being built: node name -> Kind (`use`).
_TABLE = {}


def use(table, props_dir=None):
    """Make `table` the props of the unit being built. It maps node names to
    prop ids: a Meshy prop of art/units/meshy_props.py, built from its file
    in `props_dir` (default art/units/source/props/) when that is there and
    from its boxy stand-in until then, or a boxy kind of `KINDS`. Returns
    one line per Meshy prop saying which it got."""
    specs = mesh_specs()
    props_dir = props_dir or MESH_DIR
    resolved, notes = {}, []
    for name, pid in table.items():
        if pid in specs:
            path = mesh_file(props_dir, pid)
            if path:
                resolved[name] = MeshKind(pid, path, specs[pid])
                notes.append(f"{name}: Meshy prop {pid} from {path}")
            else:
                resolved[name] = KINDS[specs[pid]["like"]]
                notes.append(f"{name}: no {pid} model in {props_dir} yet: the boxy {specs[pid]['like']} stands in")
        elif pid in KINDS:
            resolved[name] = KINDS[pid]
        else:
            raise KeyError(f"prop {name}: no prop {pid!r} (Meshy props: {sorted(specs)}; kinds: {sorted(KINDS)})")
    _TABLE.clear()
    _TABLE.update(resolved)
    return notes


def kind_of(name):
    """The Kind of a prop of the unit being built, or a kind by its own name."""
    if name in _TABLE:
        return _TABLE[name]
    if name in KINDS:
        return KINDS[name]
    raise KeyError(f"unknown prop {name!r} (this unit has {sorted(_TABLE)})")


def prop_info(name):
    """The solver's `Prop` for a prop name (no Blender objects needed, but a
    Meshy prop's named parts are known once it is built): a node of the unit
    being built, or a library kind by its own name."""
    k = kind_of(name)
    if k.load:
        return Prop(name, k.hand, "load", Matrix.Identity(3))
    return Prop(name, k.hand, "haft", frame(Vector((0, -1, 0)), Vector((0, 0, 1))), k.mount,
                parts=k.parts, strike=k.strike)


def all_prop_names():
    return list(_TABLE)


def build_props(arm_obj, rig, names=None):
    """Create the unit's prop objects (or those in `names`: nodes of the
    unit or kinds), each parented to its hand bone."""
    objs = {}
    rest = rig.fk({})
    for name in all_prop_names() if names is None else names:
        obj = kind_of(name).make(name)
        info = prop_info(name)
        bone = HAND[info.hand]
        pos, w = rest[bone]
        world = mat4(pos, w) @ info.in_hand
        obj.parent = arm_obj
        obj.parent_type = "BONE"
        obj.parent_bone = bone
        obj.matrix_parent_inverse = Matrix.Identity(4)
        bpy.context.view_layer.update()
        obj.matrix_world = world
        objs[name] = obj
    bpy.context.view_layer.update()
    return objs


# ---------------------------------------------------------------------------
# Meshy props: weapons and shields generated as models of their own
# (art/units/source/props/<id>.glb, about 3k triangles, textured), at any
# scale and orientation. Each has a spec in art/units/meshy_props.py that
# turns it into the prop frame: which of the file's axes (as glTF writes
# them, +Y up) runs up the haft to the head (`haft`) and which toward the
# striking side (`strike`), how long it is in metres along the haft
# (`length`; the file's own scale means nothing), and where the main hand
# grips (`grip`: a fraction of the length from the butt, on the middle of
# the haft there; or (x, y, z) fractions of the oriented bounding box,
# x strike, y haft, z side). Then it is cleaned, decimated and textured
# like a static model (art/static/lib) and held like the boxy kind it
# replaces (`like`: hand, mount, the striking part's name).

UNITS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))   # art/units
MESH_DIR = os.path.join(UNITS, "source", "props")
MESH_SPECS = os.path.join(UNITS, "meshy_props.py")
MESH_EXTENSIONS = (".glb", ".gltf", ".fbx", ".obj")
SPEC_KEYS = {"like", "length", "grip", "haft", "strike", "grip_offset", "roll", "parts", "hand", "mount",
             "triangles", "texture"}
AXES = {"+X": (1.0, 0.0, 0.0), "-X": (-1.0, 0.0, 0.0), "+Y": (0.0, 0.0, 1.0), "-Y": (0.0, 0.0, -1.0),
        "+Z": (0.0, -1.0, 0.0), "-Z": (0.0, 1.0, 0.0)}   # glTF axis -> Blender (glTF +Z is Blender -Y)
MESH_TRIANGLES = 1500   # a weapon at RTS distance needs no more
MESH_TEXTURE = 512
_SPECS = None


def mesh_specs():
    """{prop id: spec} from art/units/meshy_props.py, checked."""
    global _SPECS
    if _SPECS is None:
        import importlib.util
        spec = importlib.util.spec_from_file_location("meshy_props", MESH_SPECS)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        _SPECS = dict(mod.PROPS)
        for pid, s in _SPECS.items():
            _check_spec(pid, s)
    return _SPECS


def _check_spec(pid, s):
    where = f"{MESH_SPECS}: {pid}"
    unknown = sorted(set(s) - SPEC_KEYS)
    if unknown:
        raise KeyError(f"{where}: unknown keys {unknown} (known: {sorted(SPEC_KEYS)})")
    if s.get("like") not in KINDS:
        raise KeyError(f"{where}: `like` must name a boxy kind ({sorted(KINDS)})")
    if not (isinstance(s.get("length"), (int, float)) and s["length"] > 0):
        raise KeyError(f"{where}: `length` (metres along the haft) is required")
    grip = s.get("grip")
    if not (isinstance(grip, (int, float)) and 0.0 <= grip <= 1.0 or isinstance(grip, (tuple, list)) and len(grip) == 3):
        raise KeyError(f"{where}: `grip` must be a fraction of the length from the butt or (x, y, z) box fractions")
    for key in ("haft", "strike"):
        if s.get(key) is not None and s[key] not in AXES:
            raise KeyError(f"{where}: `{key}` must be one of {sorted(AXES)} or None (automatic)")
    if s.get("haft") and s.get("strike") and s["haft"][1] == s["strike"][1]:
        raise KeyError(f"{where}: `haft` and `strike` must be different axes")


def mesh_file(folder, pid):
    for ext in MESH_EXTENSIONS:
        path = os.path.join(folder, pid + ext)
        if os.path.isfile(path):
            return path
    return None


class MeshKind(Kind):
    """A prop built from a Meshy model by its spec (see above). Its named
    parts are measured off the model when it is made."""

    def __init__(self, pid, path, spec):
        like = KINDS[spec["like"]]
        super().__init__(None, spec.get("hand", like.hand), spec.get("mount", like.mount), strike=like.strike)
        self.id = pid
        self.path = path
        self.spec = spec

    def make(self, name):
        from .static import static_lib

        notes = []
        obj = _import_mesh(self.path, name, static_lib("scene"))
        materials = static_lib("materials")
        found = [m for m in obj.data.materials if m is not None]
        if not found:
            obj.data.materials.append(materials.flat_material(name, (0.5, 0.5, 0.5)))
        for i, mat in enumerate(found):
            mat.name = name if i == 0 else f"{name}_{i}"
            materials.clean(mat, notes)
        materials.shrink_textures(obj, self.spec.get("texture", MESH_TEXTURE), name, notes)
        materials.merge_duplicates(obj, notes)
        before, after = static_lib("reduce").reduce(obj, self.spec.get("triangles", MESH_TRIANGLES), notes)
        self.parts, how = _normalise(obj, self.spec)
        if self.strike and self.strike not in self.parts:
            raise KeyError(f"{MESH_SPECS}: {self.id} needs a part {self.strike!r} (the {self.spec['like']}'s "
                           f"striking part) in `parts`")
        print(f"{name}: {os.path.basename(self.path)}, {before} -> {after} triangles, {how}")
        for n in notes:
            print(f"  {n}")
        return obj


def _import_mesh(path, name, scene_lib):
    """The file's meshes, baked and joined into one object `name` in the
    current scene (nothing else in it is touched)."""
    before = {k: set(getattr(bpy.data, k)) for k in ("objects", "meshes", "actions")}
    objs = scene_lib.import_file(path)
    for a in set(bpy.data.actions) - before["actions"]:
        bpy.data.actions.remove(a)
    bpy.context.view_layer.update()
    dg = bpy.context.evaluated_depsgraph_get()
    baked = []
    for o in objs:
        if o.type != "MESH" or o.hide_render:
            continue
        me = bpy.data.meshes.new_from_object(o.evaluated_get(dg), preserve_all_data_layers=False, depsgraph=dg)
        if not me.polygons:
            bpy.data.meshes.remove(me)
            continue
        me.transform(o.matrix_world)
        if o.matrix_world.determinant() < 0:
            me.flip_normals()
        for i, slot in enumerate(o.material_slots):
            if i < len(me.materials):
                me.materials[i] = slot.material
        baked.append(me)
    for o in objs:
        bpy.data.objects.remove(o, do_unlink=True)
    for m in set(bpy.data.meshes) - before["meshes"] - set(baked):
        if m.users == 0:
            bpy.data.meshes.remove(m)
    if not baked:
        raise ValueError(f"{path}: no faces in the file")
    obj = scene_lib._join(baked, name)
    while len(obj.data.uv_layers) > 1:  # textures use the first UV map
        obj.data.uv_layers.remove(obj.data.uv_layers[-1])
    return obj


def _normalise(obj, spec):
    """Move `obj`'s mesh into the prop frame by `spec`. Returns ({part: point
    in the prop frame}, a line saying what was done)."""
    import numpy as np

    me = obj.data
    co = np.empty(len(me.vertices) * 3)
    me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    size = co.max(0) - co.min(0)
    order = [int(i) for i in np.argsort(-size)]   # Blender axes, longest first
    names = {0: "X", 1: "-Z", 2: "Y"}               # Blender axis -> the glTF axis it shows
    haft = Vector(AXES[spec["haft"]]) if spec.get("haft") else Vector(np.eye(3)[order[0]])
    if spec.get("strike"):
        strike = Vector(AXES[spec["strike"]])
    else:
        rest = [i for i in order if abs(haft[i]) < 0.5]
        strike = Vector(np.eye(3)[rest[0]])
    axes = frame(strike, haft)                      # columns: prop X, Y, Z in Blender axes
    q = co @ np.array(axes)                         # vertices in prop axes, file units
    y0, y1 = q[:, 1].min(), q[:, 1].max()
    if y1 - y0 <= 0:
        raise ValueError(f"{obj.name}: flat along its haft axis")
    scale = spec["length"] / (y1 - y0)
    q = (q - [0.0, y0, 0.0]) * scale                # metres, butt at y = 0
    length = spec["length"]
    lo, hi = q.min(0), q.max(0)

    edges = np.empty(len(me.edges) * 2, np.int64)
    me.edges.foreach_get("vertices", edges)
    a, b = q[edges[0::2]], q[edges[1::2]]

    def on_haft(f):
        """The middle of the haft's cross-section `f` of the way from the
        butt: where the mesh's edges cross that plane (a shaft may be one
        long box with no vertex along it)."""
        y = min(max(f * length, 1e-6), length - 1e-6)
        cross = (a[:, 1] - y) * (b[:, 1] - y) <= 0.0
        da = b[cross, 1] - a[cross, 1]
        t = np.where(np.abs(da) > 1e-12, (y - a[cross, 1]) / np.where(np.abs(da) > 1e-12, da, 1.0), 0.0)
        cut = a[cross] + (b[cross] - a[cross]) * t[:, None]
        if len(cut) < 3:
            raise ValueError(f"{obj.name}: nothing to grip at {f:.2f} of its length")
        mid = (cut.min(0) + cut.max(0)) / 2
        return np.array([mid[0], y, mid[2]])

    def point(p):
        return on_haft(p) if isinstance(p, (int, float)) else lo + np.array(p) * (hi - lo)

    grip = point(spec["grip"]) + np.array(spec.get("grip_offset", (0.0, 0.0, 0.0)))
    turn = rot(Y_AXIS, spec.get("roll", 0.0))
    m = turn.to_4x4() @ Matrix.Translation(Vector(-grip)) @ Matrix.Diagonal((scale, scale, scale, 1.0)) \
        @ Matrix.Translation((0.0, -y0, 0.0)) @ axes.transposed().to_4x4()
    me.transform(m)
    me.update()
    parts = {"tip": 1.0, "butt": 0.0}
    parts.update(spec.get("parts", {}))
    out = {k: tuple(turn @ Vector(point(p) - grip)) for k, p in parts.items()}

    def axis_name(v):
        i = max(range(3), key=lambda k: abs(v[k]))
        n = names[i]
        sign = 1 if (v[i] > 0) != n.startswith("-") else -1
        return ("+" if sign > 0 else "-") + n[-1]

    how = (f"haft {axis_name(haft)}{'' if spec.get('haft') else ' (automatic: the longest)'}, strike "
           f"{axis_name(strike)}{'' if spec.get('strike') else ' (automatic)'}, scaled x{scale:.4g} to "
           f"{length} m, grip {spec['grip']}")
    return out, how
# ---------------------------------------------------------------------------
# Team sash: a strap from the right shoulder across the chest to the left
# hip and round the back, fitted to the body by ray casts and skinned with
# weights copied from the nearest body vertices. The strap is the body's
# outline in the plane through two points inside the body and the
# front-back axis: one under the top of the right shoulder, outside the
# neck and the hair, so the strap runs over the shoulder, not up the side
# of the neck; one at the left hip. A unit's config may give them
# (`SASH`); otherwise `sash_anchors` finds them from the skeleton.

# Where `sash_anchors` puts the two points, measured on the peasant (whose
# hand-placed points they reproduce to 1 mm): the shoulder point 89 % of
# the way out to the right shoulder joint and 75 % of the way up from it to
# the neck; the hip point 86.5 % of that half-width out to the left and
# 42 % of the way up from the pelvis joint to the spine's first joint.
SASH_SHOULDER_OUT = 0.89
SASH_SHOULDER_UP = 0.75
SASH_HIP_OUT = 0.865
SASH_HIP_UP = 0.42
SASH_WIDTH = 0.085  # wide enough to read as team colour from the RTS camera
SASH_THICK = 0.008
SASH_LIFT = 0.008
SASH_SEGMENTS = 96
SASH_LINES = 5  # rays across the width per sample
SASH_REACH = 1  # rays each way the strap bridges over
SASH_SMOOTH = (1, 2, 1)  # weights of the smoothing round the loop
SASH_BONES = {HIPS, *SPINE, NECK, ARM["L"][0], ARM["R"][0]}  # skinned to: pelvis, spine, neck, clavicles
SASH_OFF = {HEAD, HEADFRONT, *(ARM[s][i] for s in SIDES for i in (1, 2, 3))}  # not fitted over: head, arms


def sash_anchors(rig):
    """(shoulder, hip) world points for the sash, from the rest skeleton."""
    p = rig.rest_pos
    r_arm, l_arm = p[ARM["R"][1]], p[ARM["L"][1]]
    half = (l_arm.x - r_arm.x) / 2
    mid_y = (p[SPINE[0]].y + p[SPINE[2]].y) / 2
    shoulder = Vector((-SASH_SHOULDER_OUT * half, mid_y, r_arm.z + SASH_SHOULDER_UP * (p[NECK].z - r_arm.z)))
    hip = Vector((SASH_HIP_OUT * half, mid_y, p[HIPS].z + SASH_HIP_UP * (p[SPINE[0]].z - p[HIPS].z)))
    return shoulder, hip


def build_sash(body, arm_obj, shoulder, hip, width=SASH_WIDTH):
    """The skinned `Team_sash` through the world points `shoulder` and
    `hip` (`sash_anchors`, or a unit's own)."""
    from mathutils.bvhtree import BVHTree
    from mathutils.kdtree import KDTree

    mw = body.matrix_world
    me = body.data
    verts = [mw @ v.co for v in me.vertices]
    groups = {g.index: g.name for g in body.vertex_groups}

    def dominant(v):
        g = max(v.groups, key=lambda g: g.weight, default=None)
        return groups[g.group] if g else None

    # The strap lies on the body's outer surface, leaving out the arms (the
    # hands hang by the hips in the rest pose) and the head and hair, and is
    # skinned to the torso's bones only, so an arm never drags it along.
    dom = [dominant(v) for v in me.vertices]
    polys = [tuple(p.vertices) for p in me.polygons if not any(dom[i] in SASH_OFF for i in p.vertices)]
    bvh = BVHTree.FromPolygons(verts, polys)
    torso = [d in SASH_BONES for d in dom]

    shoulder, hip = Vector(shoulder), Vector(hip)
    center = (shoulder + hip) / 2
    u = (hip - shoulder).normalized()
    v = Vector((0.0, 1.0, 0.0))
    normal = u.cross(v).normalized()

    def ray(i):
        theta = 2 * math.pi * i / SASH_SEGMENTS
        return u * math.cos(theta) + v * math.sin(theta)

    # The body's outline round the plane, sampled along SASH_LINES lines
    # across the strap's width as a distance along each ray. The strap takes
    # the outermost line, widened over SASH_REACH rays each way and smoothed
    # round the loop (never inwards), so it lies flat across its width and
    # bridges the tunic's folds instead of following them.
    n = SASH_SEGMENTS
    offsets = [width * (k / (SASH_LINES - 1) - 0.5) for k in range(SASH_LINES)]
    r = [None] * n
    for off in offsets:
        c = center + normal * off
        for i in range(n):
            d = ray(i)
            hit, _, _, _ = bvh.ray_cast(c + d * 0.6, -d, 1.0)  # through an arm hole: no hit
            if hit is not None:
                r[i] = max(r[i] or 0.0, (hit - c).dot(d))
    if None in r:
        raise RuntimeError(f"sash: no body surface under ray {r.index(None)} of {n}")
    reach = [max(r[(i + j) % n] for j in range(-SASH_REACH, SASH_REACH + 1)) for i in range(n)]
    half = len(SASH_SMOOTH) // 2
    smooth = [sum(k * reach[(i + j) % n] for j, k in zip(range(-half, half + 1), SASH_SMOOTH)) / sum(SASH_SMOOTH)
              for i in range(n)]
    r = [max(a, b) + SASH_LIFT for a, b in zip(r, smooth)]
    rings = [[tuple(center + normal * off + ray(i) * (r[i] + t) for t in (0.0, SASH_THICK))
              for off in (offsets[0], offsets[-1])] for i in range(n)]

    bm = bmesh.new()
    vs = []
    for ring in rings:
        (a_in, a_out), (b_in, b_out) = ring
        vs.append([bm.verts.new(q) for q in (a_in, a_out, b_out, b_in)])
    for i in range(SASH_SEGMENTS):
        r0, r1 = vs[i], vs[(i + 1) % SASH_SEGMENTS]
        for k in range(4):
            k1 = (k + 1) % 4
            f = bm.faces.new((r0[k], r0[k1], r1[k1], r1[k]))
            f.smooth = False
    bm.normal_update()
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)

    # Skin weights: inverse-distance blend of the nearest torso vertices'
    # weights on the torso bones.
    torso_ix = [i for i, t in enumerate(torso) if t]
    kd = KDTree(len(torso_ix))
    for i in torso_ix:
        kd.insert(verts[i], i)
    kd.balance()
    weights = []
    for bv in bm.verts:
        acc = {}
        for _, idx, dist in kd.find_n(bv.co, 8):
            wgt = 1.0 / max(dist, 1e-4)
            for g in me.vertices[idx].groups:
                if groups[g.group] in SASH_BONES:
                    acc[g.group] = acc.get(g.group, 0.0) + wgt * g.weight
        top = sorted(acc.items(), key=lambda kv: -kv[1])[:4]
        total = sum(x for _, x in top) or 1.0
        weights.append([(groups[gi], x / total) for gi, x in top])

    # Store in the armature's local space (centimetres), like the body.
    to_local = arm_obj.matrix_world.inverted()
    for bv in bm.verts:
        bv.co = to_local @ bv.co
    mesh = bpy.data.meshes.new("Team_sash")
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(material("Team"))
    obj = bpy.data.objects.new("Team_sash", mesh)
    bpy.context.scene.collection.objects.link(obj)
    obj.parent = arm_obj
    obj.matrix_parent_inverse = Matrix.Identity(4)
    obj.matrix_basis = Matrix.Identity(4)
    for i, wl in enumerate(weights):
        for gname, x in wl:
            vg = obj.vertex_groups.get(gname) or obj.vertex_groups.new(name=gname)
            vg.add([i], x, "REPLACE")
    mod = obj.modifiers.new("Armature", "ARMATURE")
    mod.object = arm_obj
    return obj
