"""Rigid units: a model without a skeleton (vehicles, siege, mounts, beasts,
flyers), drawn as it is, with a team-colour marker and no clips.

The model goes through the static model pipeline's own steps (art/static/lib,
read-only from here; docs/art/static-models.md):

1. import any .glb/.gltf/.fbx/.obj; a rig or animation in it is posed at
   rest and baked into the mesh (a statue), and every mesh is joined;
2. turned by YAW to face +Z, based on the ground and centred (its
   bounding box), and a ground plate cut away if CUT_PLATE says so;
3. materials cleaned (base colour texture only, no emission, metallic 0,
   cutout alpha) and textures shrunk to TEXTURE px;
4. decimated to TRIANGLES (marker included);

then gets its marker (`Team_pennant` or `Team_band`, material `Team`,
which the engine tints with the owner's colour) and is exported, checked
and written with a manifest that has no clips. The engine draws such a
model in its bind pose (no animation player), scaled so its height matches
the unit's box height.

Markers, sized from the model's height H so they come out the same in the
game whatever the source's scale:
* pennant: a pole 0.5 H tall with a flag streaming back (+Y in Blender,
  -Z in glTF), standing on MARKER_AT or on the highest point of the back
  third of the model, near its middle;
* band: a strip 0.07 H wide round the model's outline at MARKER_AT (a
  fraction of H, default 0.5), fitted by rays like the humanoid's sash.
"""

import math
import os
import tempfile

import bmesh
import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from . import export, gamedata, props
from .rig import at
from .static import static_lib

PENNANT_POLE = 0.5           # pole length above its foot, of the model's height
PENNANT_SINK = 0.02          # how far the pole goes into the model
PENNANT_FLAG = (0.30, 0.17)  # flag length (streaming back) and height
BACK_THIRD = 1.0 / 3.0       # the automatic pennant stands in this rear share...
MIDDLE = 0.25                # ...within this share of the width either side of the middle
GRID = 9                     # rays per side searched for the pennant's foot
BAND_WIDTH = 0.07
BAND_THICK = 0.01
BAND_LIFT = 0.006
BAND_SEGMENTS = 48
BAND_LINES = 3
# Triangles kept free for the marker: 4 boxes; a 4-sided tube.
MARKER_TRIANGLES = {"pennant": 48, "band": 8 * BAND_SEGMENTS, None: 0}


class RigidError(RuntimeError):
    pass


def build(unit, src, out_dir, sheets_dir, model_path, writer):
    scene, materials, reduce, gltf = (static_lib(n) for n in ("scene", "materials", "reduce", "gltf"))
    if not os.path.isfile(src):
        raise RigidError(f"no source file {src}")
    scene.reset()
    obj, notes = scene.to_statue(scene.import_file(src), unit.id)
    scene.normalise(obj, unit.yaw)
    if unit.cut_plate:
        cut = scene.cut_plate(obj, unit.cut_plate)
        if not cut:
            raise RigidError(f"{unit.id}: CUT_PLATE found nothing in the bottom {unit.cut_plate:.0%} to cut")
        notes.append(f"cut its ground plate: {cut} faces")
        scene.normalise(obj, 0.0)
    obj.data.name = unit.id

    found = [m for m in obj.data.materials if m is not None]
    if not found:
        obj.data.materials.append(materials.flat_material(f"{unit.id}_default", (0.6, 0.6, 0.6)))
    for mat in found:
        if mat.name == materials.TEAM:  # the marker's: the engine must not tint the model itself
            mat.name = f"{unit.id}_{mat.name}"
        materials.clean(mat, notes)
    materials.shrink_textures(obj, unit.texture, unit.id, notes)
    materials.merge_duplicates(obj, notes)
    source_tris, tris = reduce.reduce(obj, unit.triangles - MARKER_TRIANGLES[unit.marker], notes)
    if tris != source_tris:
        scene.drop_loose(obj)
        scene.normalise(obj, 0.0)
    lo, hi = (Vector(v) for v in scene.bounds(obj))
    height = hi.z - lo.z
    if height <= 0 or min(hi.x - lo.x, hi.y - lo.y) <= 0:
        raise RigidError(f"{os.path.basename(src)} is flat or empty")

    shown, note = drawn_height(unit, hi - lo)
    if note:
        notes.append(note)

    objects, marker = [obj], None
    if unit.marker == "pennant":
        marker, note = pennant(obj, lo, hi, unit.marker_at)
    elif unit.marker == "band":
        marker, note = band(obj, lo, hi, 0.5 if unit.marker_at is None else unit.marker_at)
    if marker is not None:
        notes.append(note)
        objects.append(marker)
    print(f"{unit.id}: {source_tris} -> {tris} triangles, {hi.x - lo.x:.2f} x {height:.2f} x {hi.y - lo.y:.2f} m "
          f"(w x h x d)")
    for n in notes:
        print(f"  {n}")

    if out_dir:
        glb = os.path.join(out_dir, f"{unit.id}.glb")
        gltf.export(glb, objects)
        s = gltf.verify(glb, unit.id, marker.name if marker else None, unit.texture)
        total = sum(n["triangles"] for n in s["nodes"].values())
        if total > unit.triangles:
            raise RigidError(f"{glb}: {total} triangles, over the budget of {unit.triangles}")
        export.write_manifest(os.path.join(out_dir, f"{unit.id}.model.ron"), model_path, shown, [], writer)
        print(f"\nwrote {glb} ({s['bytes'] / 1e3:.0f} kB, {total} triangles)")
        print("  images:", ", ".join(f"{i['name']} {i['size'][0]}x{i['size'][1]} {i['bytes'] / 1e3:.0f} kB"
                                     for i in s["images"]) or "none")
    if sheets_dir:
        from . import sheets
        out = os.path.join(sheets_dir, f"{unit.id}.png")
        sheets.render_model(objects, out, tempfile.mkdtemp(prefix="af_tiles_"))  # short: Windows paths stop at 260
        print("sheet", out)


def drawn_height(unit, size):
    """The manifest's `height`, which the engine scales to the unit's box
    height: the model's own height, or more when FIT lists the box's width
    or depth and the model, scaled to the box height, would overrun them (it
    is then drawn lower than its box, at the largest size that fits every
    listed dimension). `size` is the model's (width, depth, height) in
    Blender axes. Returns (height, a note on its size in game or None)."""
    model = {"width": size.x, "depth": size.y, "height": size.z}
    try:
        bw, bh, bd = (float(v) for v in gamedata.unit(unit.id)["size"])
    except (KeyError, TypeError, ValueError):
        if unit.fit != ("height",):
            raise RigidError(f"{unit.id}: FIT needs the unit's box, and assets/data has no size for it")
        return model["height"], None
    box = {"width": bw, "height": bh, "depth": bd}
    scale = min(box[d] / model[d] for d in unit.fit)
    game = " x ".join(f"{model[d] * scale:.2f}" for d in ("width", "height", "depth"))
    note = f"in game {game} m (w x h x d) in its {bw:g} x {bh:g} x {bd:g} m box"
    if unit.fit == ("height",):
        return model["height"], note
    return bh / scale, (f"{note}: FIT {', '.join(unit.fit)} draws it at {model['height'] * scale / bh:.0%} "
                        f"of the box height (manifest height {bh / scale:.3f}, the model's {model['height']:.3f})")


def _bvh(obj):
    return BVHTree.FromObject(obj, bpy.context.evaluated_depsgraph_get())


def pennant(obj, lo, hi, anchor=None):
    """`Team_pennant`: a pole with a flag streaming back. Returns (object, note)."""
    h = hi.z - lo.z
    if anchor is not None:
        foot = Vector(tuple(x + 0.0 for x in at(*anchor)))
        note = f"pennant at {tuple(anchor)} (right, fwd, up) m"
    else:
        bvh = _bvh(obj)
        cx, half = (lo.x + hi.x) / 2, (hi.x - lo.x) / 2 * MIDDLE
        best = None
        for i in range(GRID):
            for j in range(GRID):
                x = cx - half + 2 * half * i / (GRID - 1)
                y = hi.y - (hi.y - lo.y) * BACK_THIRD * j / (GRID - 1)
                hit, *_ = bvh.ray_cast(Vector((x, y, hi.z + 1.0)), Vector((0.0, 0.0, -1.0)))
                if hit is not None and (best is None or hit.z > best.z + 1e-6):
                    best = hit
        if best is None:
            raise RigidError("pennant: no surface in the back third to stand on; set MARKER_AT")
        foot = best
        note = (f"pennant at ({-foot.x:.2f}, {-foot.y:.2f}, {foot.z:.2f}) m (right, fwd, up): the highest point "
                f"of the back third (set MARKER_AT to move it)")
    pole, r = PENNANT_POLE * h, max(0.012, 0.012 * h)
    length, tall = PENNANT_FLAG[0] * h, PENNANT_FLAG[1] * h
    b = props.Boxes()
    base = foot.z - PENNANT_SINK * h
    b.box("PropWood", (foot.x, foot.y, (base + foot.z + pole) / 2), (2 * r, 2 * r, foot.z + pole - base))
    b.box("Team", (foot.x, foot.y, foot.z + pole + 1.5 * r), (3 * r, 3 * r, 3 * r), (0.0, 0.0, 45.0))   # finial
    top = foot.z + pole - 0.5 * r
    b.box("Team", (foot.x, foot.y + r + 0.32 * length, top - tall / 2), (0.6 * r, 0.64 * length, tall))
    b.box("Team", (foot.x, foot.y + r + 0.80 * length, top - 0.3 * tall), (0.6 * r, 0.36 * length, 0.6 * tall))
    return b.to_object("Team_pennant"), note


def band(obj, lo, hi, fraction):
    """`Team_band`: a strip round the model's outline at `fraction` of its
    height. Returns (object, note)."""
    if not 0.0 < fraction < 1.0:
        raise RigidError(f"band: MARKER_AT must be a fraction of the height (0..1), not {fraction}")
    h = hi.z - lo.z
    z = lo.z + fraction * h
    width, thick, lift = BAND_WIDTH * h, BAND_THICK * h, BAND_LIFT * h
    centre = Vector(((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, z))
    reach = (hi - lo).length + 1.0
    bvh = _bvh(obj)
    n = BAND_SEGMENTS
    dirs = [Vector((math.cos(2 * math.pi * i / n), math.sin(2 * math.pi * i / n), 0.0)) for i in range(n)]
    r = [None] * n
    for k in range(BAND_LINES):
        dz = width * (k / (BAND_LINES - 1) - 0.5)
        for i, d in enumerate(dirs):
            c = centre + Vector((0.0, 0.0, dz))
            hit, *_ = bvh.ray_cast(c + d * reach, -d, reach)
            if hit is not None:
                r[i] = max(r[i] or 0.0, (hit - c).dot(d))
    missing = sum(x is None for x in r)
    if missing > n // 2:
        raise RigidError(f"band: the model is mostly open at {fraction:.2f} of its height; set MARKER_AT")
    for i in range(n):  # gaps (between legs, through a frame): span the neighbours
        if r[i] is None:
            a = next(r[(i - j) % n] for j in range(1, n) if r[(i - j) % n] is not None)
            b = next(r[(i + j) % n] for j in range(1, n) if r[(i + j) % n] is not None)
            r[i] = max(a, b)
    wide = [max(r[(i + j) % n] for j in (-1, 0, 1)) for i in range(n)]
    r = [max(a, (wide[(i - 1) % n] + 2 * wide[i] + wide[(i + 1) % n]) / 4) + lift for i, a in enumerate(r)]
    bm = bmesh.new()
    rings = []
    for d, ri in zip(dirs, r):
        rings.append([bm.verts.new(centre + d * (ri + t) + Vector((0.0, 0.0, s * width / 2)))
                      for t, s in ((0.0, -1), (thick, -1), (thick, 1), (0.0, 1))])
    for i in range(n):
        r0, r1 = rings[i], rings[(i + 1) % n]
        for k in range(4):
            bm.faces.new((r0[k], r0[(k + 1) % 4], r1[(k + 1) % 4], r1[k])).smooth = False
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    mesh = bpy.data.meshes.new("Team_band")
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(props.material("Team"))
    ob = bpy.data.objects.new("Team_band", mesh)
    bpy.context.scene.collection.objects.link(ob)
    return ob, f"band at {fraction:.2f} of the height ({z:.2f} m), {missing} of {n} rays through gaps"
