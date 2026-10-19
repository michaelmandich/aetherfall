"""Triangle budget: weld, then collapse decimation down to the budget.

* Weld first. Exported meshes (Meshy's among them) are split into separate
  vertices wherever a UV seam or a hard edge runs, so the surface is many
  disconnected patches; collapse cannot merge across the cracks and shreds
  the model into slivers. Welding coincident vertices (WELD of the model's
  diagonal) makes it one surface again; UVs stay per face corner, so the
  seams survive.
* Then Blender's Decimate (Collapse): it merges the edges that change the
  shape least first, so silhouettes hold (check them on the contact sheet),
  and interpolates UVs. The ratio is corrected until the result fits, since
  collapse lands near, not exactly on, the ratio asked for.
* Collapse never merges a non-manifold edge (one shared by three or more
  faces: fronds or blades sharing a stem, planks meeting a fence rail), so
  a model with many of them stalls above its budget (Meshy's farm, palms
  and some oaks). Then those edges are split, each face fan becoming its
  own sheet with a boundary that collapse can work along, and collapse
  runs again. Only as a fallback: the sheets are reduced independently, so
  hairline cracks can open along the split.
* A decimated mesh gets fresh normals: smooth, with edges sharper than
  SHARP_ANGLE kept hard, which suits organic models (trees, rocks) and
  buildings (walls, roofs) alike.

A model already within its budget is left exactly as it is.
"""

import math

import bmesh
import bpy

from .scene import triangles, vertices

SHARP_ANGLE = math.radians(42.0)
WELD = 2e-5      # weld distance, as a fraction of the bounding-box diagonal
ATTEMPTS = 6


def reduce(obj, budget, notes):
    """Weld and decimate `obj` in place to at most `budget` triangles.
    Returns (triangles before, triangles after)."""
    before = triangles(obj)
    if before <= budget:
        return before, before
    welded = _weld_and_triangulate(obj)
    if welded:
        notes.append(f"welded {welded} split vertices before decimating")
    after = _collapse(obj, budget)
    if after > budget:
        split = _split_non_manifold(obj)
        if split:
            notes.append(f"collapse stalled at {after} triangles; split {split} non-manifold edges")
            after = _collapse(obj, budget)
    if after > budget:
        raise RuntimeError(f"could not decimate {obj.name} to {budget} triangles (got {after})")
    mesh = obj.data
    if mesh.has_custom_normals:  # the decimated surface needs its own
        _clear_custom_normals(obj)
    mesh.shade_smooth()
    mesh.set_sharp_from_angle(angle=SHARP_ANGLE)
    notes.append(f"decimated {before} -> {after} triangles")
    return before, triangles(obj)


def _collapse(obj, budget):
    """Collapse-decimate `obj` to at most `budget` triangles, correcting the
    ratio until it fits, and apply it. When collapse cannot get there, the
    mesh is left as it was and the closest count is returned."""
    ratio = budget / triangles(obj)
    best = triangles(obj)
    for _ in range(ATTEMPTS):
        mod = obj.modifiers.new("Decimate", "DECIMATE")
        mod.decimate_type = "COLLAPSE"
        mod.ratio = ratio
        mod.use_collapse_triangulate = True
        dg = bpy.context.evaluated_depsgraph_get()
        ev = obj.evaluated_get(dg)
        after = sum(len(p.vertices) - 2 for p in ev.data.polygons)
        if after <= budget:
            _apply(obj, mod)
            return after
        obj.modifiers.remove(mod)
        best = min(best, after)
        ratio *= budget / after * 0.995
    return best


def _split_non_manifold(obj):
    """Split every edge shared by three or more faces. Returns how many."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    edges = [e for e in bm.edges if len(e.link_faces) > 2]
    if edges:
        bmesh.ops.split_edges(bm, edges=edges)
        bm.to_mesh(obj.data)
    bm.free()
    return len(edges)


def _weld_and_triangulate(obj):
    """Merge coincident vertices and triangulate. Returns how many merged."""
    co = vertices(obj)
    diagonal = float(((co.max(0) - co.min(0)) ** 2).sum() ** 0.5) if len(co) else 0.0
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    n = len(bm.verts)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=max(1e-7, WELD * diagonal))
    welded = n - len(bm.verts)
    bmesh.ops.triangulate(bm, faces=bm.faces, quad_method="BEAUTY", ngon_method="BEAUTY")
    bm.to_mesh(obj.data)
    bm.free()
    return welded


def _apply(obj, mod):
    with bpy.context.temp_override(object=obj, active_object=obj, selected_objects=[obj]):
        bpy.ops.object.modifier_apply(modifier=mod.name)


def _clear_custom_normals(obj):
    with bpy.context.temp_override(object=obj, active_object=obj, selected_objects=[obj]):
        try:
            bpy.ops.mesh.customdata_custom_splitnormals_clear()
        except RuntimeError:
            pass
