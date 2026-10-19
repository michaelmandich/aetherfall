"""The team banner on buildings: a pole and a flag, node `Team_banner`.

The flag and the pole's finial use the material `Team`, plain white, which
the engine tints with the owner's colour; the pole is dark wood
(`Banner_pole`). 38 triangles.

Size: designed in game metres from the building's box height (a 3 m house
gets a 1.55 m pole and a 0.96 x 0.62 m flag, a 6 m town centre a 2.3 m
pole), then divided by the scale the engine will fit the model with, per
axis, so it comes out undistorted in the game whatever the source's size
and proportions.

Placement: the pole's foot goes on the anchor (`banner = [x, y, z]` in
assets.toml: model frame before fitting, glTF axes, y up, +Z front), or by
default on the model near its top front-left corner as the game's camera
sees it (-X, +Z): rays dropped from above, from that corner toward the
centre, and the first that lands high on the model (a roof, not a step)
wins. The flag flies toward +X and faces the front, so the camera sees it.
"""

import math

import bmesh
import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from .materials import TEAM, flat_material

NODE = "Team_banner"
POLE_MATERIAL = "Banner_pole"
POLE_COLOUR = (0.20, 0.13, 0.07)
INSET = 0.14          # default anchor: this fraction of the size in from the corner
SINK = 0.08           # game metres the pole's foot goes into the roof


def dims(box_height):
    """(pole length, pole radius, flag width, flag height) in game metres."""
    pole = min(3.4, max(1.2, 0.25 * box_height + 0.8))
    return pole, 0.04 + 0.012 * pole, 0.62 * pole, 0.4 * pole


STEPS = 12      # rays from the corner to the centre
HIGH = 0.45     # a hit this high (fraction of the model's height) is "on top"


def auto_anchor(obj, lo, hi):
    """Pole foot (Blender axes) near the top front-left corner, on the model:
    the first ray from the corner toward the centre that lands high on the
    model (a roof, not a step); else the highest landing; else the ground
    at the corner. Returns (foot, moved off the corner)."""
    bvh = BVHTree.FromObject(obj, bpy.context.evaluated_depsgraph_get())
    size = Vector(hi) - Vector(lo)
    corner = Vector((lo[0] + INSET * size.x, lo[1] + INSET * size.y))
    centre = Vector(((lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2))
    top = hi[2] + 1.0 + size.z
    hits = []
    for k in range(STEPS):
        p = corner.lerp(centre, k / STEPS)
        hit, *_ = bvh.ray_cast(Vector((p.x, p.y, top)), Vector((0.0, 0.0, -1.0)))
        if hit is None:
            continue
        foot = Vector((p.x, p.y, hit.z))
        if hit.z >= lo[2] + HIGH * size.z:
            return foot, k > 0
        hits.append((hit.z, -k, foot))
    if hits:
        _, k, foot = max(hits)
        return foot, k != 0
    return Vector((corner.x, corner.y, 0.0)), False


def build(obj, lo, hi, model_to_game, box_height, anchor=None):
    """Add the banner object for model `obj` (normalised, Blender axes).
    `model_to_game` = per-axis (x, y, z) Blender-axes scale the engine will
    apply. Returns (banner object, foot (Blender axes), note)."""
    note = None
    if anchor is None:
        foot, moved = auto_anchor(obj, lo, hi)
        if moved:
            note = (f"banner at ({foot.x:.2f}, {foot.z:.2f}, {-foot.y:.2f}) m: the first high point in from "
                    f"the front-left corner (set `banner = [x, y, z]` to move it)")
    else:
        foot = Vector((anchor[0], -anchor[2], anchor[1]))  # glTF -> Blender
    sx, sy, sz = model_to_game
    pole, radius, flag_w, flag_h = dims(box_height)

    bm = bmesh.new()
    team_layer = []  # face indices that use the Team material

    def to_model(x, y, z):  # game metres relative to the foot -> model units
        return Vector((foot.x + x / sx, foot.y + y / sy, foot.z + z / sz))

    # Pole: a hexagonal prism from SINK below the foot to the top.
    ring_lo, ring_hi = [], []
    for i in range(6):
        a = math.tau * i / 6
        c, s = math.cos(a) * radius, math.sin(a) * radius
        ring_lo.append(bm.verts.new(to_model(c, s, -SINK)))
        ring_hi.append(bm.verts.new(to_model(c, s, pole)))
    for i in range(6):
        j = (i + 1) % 6
        bm.faces.new((ring_lo[i], ring_lo[j], ring_hi[j], ring_hi[i]))
    # Finial: a small cone (team colour) on top.
    tip = bm.verts.new(to_model(0.0, 0.0, pole + 3.2 * radius))
    for i in range(6):
        j = (i + 1) % 6
        team_layer.append(bm.faces.new((ring_hi[i], ring_hi[j], tip)))

    # Flag: a thin slab hanging from the top, toward +X, with one fold.
    top = pole - 0.4 * radius
    xs = (radius * 0.6, radius * 0.6 + flag_w * 0.55, radius * 0.6 + flag_w)
    fold = (0.0, 0.07 * flag_w, -0.02 * flag_w)  # toward the front (-Y Blender = +Z glTF)
    t = 0.022
    sections = []
    for x, f in zip(xs, fold):
        sag = 0.06 * flag_h * (x / flag_w)  # the free edge droops a little
        sec = [bm.verts.new(to_model(x, -f + dy, z)) for dy, z in
               ((-t, top - sag), (t, top - sag), (t, top - flag_h - sag), (-t, top - flag_h - sag))]
        sections.append(sec)
    for a, b in zip(sections, sections[1:]):
        for i in range(4):
            j = (i + 1) % 4
            team_layer.append(bm.faces.new((a[i], b[i], b[j], a[j])))
    team_layer.append(bm.faces.new(tuple(reversed(sections[0]))))
    team_layer.append(bm.faces.new(tuple(sections[-1])))

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    team = set(team_layer)
    for f in bm.faces:
        f.material_index = 1 if f in team else 0
        f.smooth = False
    mesh = bpy.data.meshes.new(NODE)
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(bpy.data.materials.get(POLE_MATERIAL) or flat_material(POLE_MATERIAL, POLE_COLOUR, 0.85))
    mesh.materials.append(bpy.data.materials.get(TEAM) or flat_material(TEAM, (1.0, 1.0, 1.0), 0.8))
    ob = bpy.data.objects.new(NODE, mesh)
    bpy.context.scene.collection.objects.link(ob)
    return ob, foot, note
