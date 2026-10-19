"""Props: boxy tools and cargo parented to the hands, and the team sash.

Every prop is built from boxes in its own *prop frame* and parented to a hand
bone. Two kinds:

* hafted tools (``kind="haft"``): the prop frame's origin is the main
  (right) hand's grip, just above the butt end; +Y runs along the haft
  towards the head, +X is the striking direction (blade edge, hammer face,
  hoe blade), +Z = X x Y. The other hand grips the same haft `at` metres
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
the ones it needs; `Team_sash` stays visible.
"""

import math

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


TOOLS = {
    # name: (builder, hand, mount roll in degrees)
    "Tool_attack": (_work_axe, "R", 0.0),
    "Tool_chop": (_felling_axe, "R", 0.0),
    "Tool_farm": (_hoe, "R", -90.0),     # broom-like grip: hoe and shovel lie along the forearm
    "Tool_mine": (_pickaxe, "R", 0.0),
    "Tool_dig": (_shovel, "R", -90.0),
    "Tool_chip": (_mallet, "R", 0.0),
    "Tool_chip_l": (_chisel, "L", 0.0),
    "Tool_smash": (_sledge, "R", 0.0),
    "Tool_build": (_hammer, "R", 0.0),
    "Tool_forage": (_basket, "L", 0.0),
}

LOADS = {
    "Load_food": _load_food,
    "Load_wood": _load_wood,
    "Load_stone": _load_stone,
    "Load_copper": lambda: _ore("PropCopper", "PropPatina"),
    "Load_iron": lambda: _ore("PropOre", "PropRust"),
    "Load_silicon": _load_silicon,
    "Load_aether": _load_aether,
    "Load_adamant": _load_adamant,
}
LOAD_HAND = "R"

# Named points of the tools, in the prop frame (metres), measured off the
# boxes above: clips aim these at the work (`Pose.wield_part`), and the
# build's checks report the speed of the striking one (`STRIKES`) at HIT.
PARTS = {
    "Tool_attack": {"bit": (0.155, 0.50, 0.0)},     # centre of the axe's edge
    "Tool_chop": {"bit": (0.1875, 0.79, 0.0)},
    "Tool_farm": {"edge": (0.18, 1.235, 0.0)},      # the hoe blade's edge
    "Tool_mine": {"tip": (0.325, 0.72, 0.0)},       # the front pick's point
    "Tool_dig": {"tip": (0.0, 1.16, 0.0)},          # the shovel blade's tip
    "Tool_chip": {"face": (0.065, 0.26, 0.0)},      # the mallet's striking face
    "Tool_chip_l": {"butt": (0.0, 0.08, 0.0), "edge": (0.0, -0.145, 0.0)},  # chisel: struck end, cutting end
    "Tool_smash": {"face": (0.131, 0.88, 0.0)},
    "Tool_build": {"face": (0.071, 0.27, 0.0)},
}
STRIKES = {"Tool_attack": "bit", "Tool_chop": "bit", "Tool_farm": "edge", "Tool_mine": "tip", "Tool_dig": "tip",
           "Tool_chip": "face", "Tool_smash": "face", "Tool_build": "face"}


def prop_info(name):
    """The solver's `Prop` for a prop name (no Blender objects needed)."""
    if name in TOOLS:
        _, hand, mount = TOOLS[name]
        return Prop(name, hand, "haft", frame(Vector((0, -1, 0)), Vector((0, 0, 1))), mount,
                    parts=PARTS.get(name), strike=STRIKES.get(name))
    if name in LOADS:
        return Prop(name, LOAD_HAND, "load", Matrix.Identity(3))
    raise KeyError(f"unknown prop {name!r}")


def all_prop_names():
    return list(TOOLS) + list(LOADS)


def build_props(arm_obj, rig):
    """Create every Tool_* / Load_* object, parented to its hand bone."""
    objs = {}
    rest = rig.fk({})
    for name in all_prop_names():
        info = prop_info(name)
        builder = TOOLS[name][0] if name in TOOLS else LOADS[name]
        obj = builder().to_object(name)
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
# Team sash: a strap from the right shoulder across the chest to the left
# hip and round the back, fitted to the body by ray casts and skinned with
# weights copied from the nearest body vertices. The strap is the body's
# outline in the plane through these two points (inside the body, world
# axes: +X left) and the front-back axis. The shoulder point lies under the
# top of the right shoulder, outside the neck and the hair (both within
# 12 cm of the midline), so the strap runs over the shoulder, not up the
# side of the neck.

SASH_SHOULDER = (-0.165, 0.0, 1.33)
SASH_HIP = (0.16, 0.0, 0.99)
SASH_WIDTH = 0.085  # wide enough to read as team colour from the RTS camera
SASH_THICK = 0.008
SASH_LIFT = 0.008
SASH_SEGMENTS = 96
SASH_LINES = 5  # rays across the width per sample
SASH_REACH = 1  # rays each way the strap bridges over
SASH_SMOOTH = (1, 2, 1)  # weights of the smoothing round the loop
SASH_BONES = {HIPS, *SPINE, NECK, ARM["L"][0], ARM["R"][0]}  # skinned to: pelvis, spine, neck, clavicles
SASH_OFF = {HEAD, HEADFRONT, *(ARM[s][i] for s in SIDES for i in (1, 2, 3))}  # not fitted over: head, arms


def build_sash(body, arm_obj):
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

    shoulder, hip = Vector(SASH_SHOULDER), Vector(SASH_HIP)
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
    offsets = [SASH_WIDTH * (k / (SASH_LINES - 1) - 0.5) for k in range(SASH_LINES)]
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
