"""Import a source model and turn it into one static mesh in the model frame.

* Import: glTF/GLB, FBX or OBJ. Every importer maps the file's +Z (the
  front, by the contract) to Blender's -Y, which the glTF exporter writes
  back as +Z, so "front" survives the round trip.
* Statue: animations are dropped and every bone goes to its rest pose
  (the file's default node transforms, what a viewer shows with nothing
  playing); the armature deformation and any shape keys are then baked
  into the mesh, and skinning, bones and empties go.
* Join: everything becomes one mesh object with its world transforms
  applied. Cameras, lights and anything without faces are dropped.
* Normalise: turned by `yaw` about the up axis, base on y = 0, centred on
  x/z (bounding box).

Blender is Z-up: glTF (x, y, z) is Blender (x, -z, y). The helpers below
work in Blender axes; `gltf_bounds` converts.
"""

import math
import os

import bmesh
import bpy
import numpy as np
from mathutils import Matrix

from .util import quiet


class ImportError_(RuntimeError):
    pass


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    for coll in (bpy.data.meshes, bpy.data.materials, bpy.data.images, bpy.data.actions):
        for block in list(coll):
            coll.remove(block)


def import_file(path):
    """Import into the (empty) scene; returns the new objects."""
    before = set(bpy.data.objects)
    ext = os.path.splitext(path)[1].lower()
    with quiet():
        _import(path, ext)
    new = [o for o in bpy.data.objects if o not in before]
    if not any(o.type == "MESH" for o in new):
        raise ImportError_(f"{path}: no mesh in the file")
    return new


def _import(path, ext):
    if ext in (".glb", ".gltf"):
        bpy.ops.import_scene.gltf(filepath=path, merge_vertices=True, import_shading="NORMALS",
                                  bone_heuristic="BLENDER", guess_original_bind_pose=False,
                                  disable_bone_shape=True, import_pack_images=True)
    elif ext == ".fbx":
        bpy.ops.import_scene.fbx(filepath=path, use_anim=False, ignore_leaf_bones=True,
                                 automatic_bone_orientation=False, use_custom_normals=True,
                                 use_image_search=True)
    elif ext == ".obj":
        bpy.ops.wm.obj_import(filepath=path, forward_axis="NEGATIVE_Z", up_axis="Y")
    else:
        raise ImportError_(f"{path}: unsupported format {ext}")


def to_statue(objects, name):
    """One mesh object (world transforms applied) from the imported objects:
    rest pose, no animation, no skin. Returns (object, notes)."""
    notes = []
    armatures = [o for o in objects if o.type == "ARMATURE"]
    for o in objects:
        ad = o.animation_data
        if ad:
            if ad.action or ad.nla_tracks:
                notes.append(f"dropped animation on {o.name}")
            for t in list(ad.nla_tracks):
                ad.nla_tracks.remove(t)
            ad.action = None
    for arm in armatures:
        for pb in arm.pose.bones:
            pb.location = (0.0, 0.0, 0.0)
            pb.rotation_quaternion = (1.0, 0.0, 0.0, 0.0)
            pb.rotation_euler = (0.0, 0.0, 0.0)
            pb.scale = (1.0, 1.0, 1.0)
    if armatures:
        notes.append(f"posed {len(armatures)} armature(s) at rest and baked the skin")
    for action in list(bpy.data.actions):
        bpy.data.actions.remove(action)
    bpy.context.view_layer.update()

    dg = bpy.context.evaluated_depsgraph_get()
    baked = []
    for o in objects:
        if o.type != "MESH" or o.hide_render:
            continue
        ev = o.evaluated_get(dg)
        mesh = bpy.data.meshes.new_from_object(ev, preserve_all_data_layers=False, depsgraph=dg)
        if not mesh.polygons:
            bpy.data.meshes.remove(mesh)
            continue
        mesh.transform(o.matrix_world)
        if o.matrix_world.determinant() < 0:
            mesh.flip_normals()
        # Object-linked materials become mesh materials.
        for i, slot in enumerate(o.material_slots):
            if i < len(mesh.materials):
                mesh.materials[i] = slot.material
        baked.append(mesh)
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o, do_unlink=True)
    for m in list(bpy.data.meshes):
        if m not in baked and m.users == 0:
            bpy.data.meshes.remove(m)
    if not baked:
        raise ImportError_("no faces after import")

    joined = _join(baked, name)
    for vg in list(joined.vertex_groups):
        joined.vertex_groups.remove(vg)
    mesh = joined.data
    while len(mesh.uv_layers) > 1:  # textures use the first UV map
        mesh.uv_layers.remove(mesh.uv_layers[-1])
    loose = drop_loose(joined)
    if loose:
        notes.append(f"dropped {loose} loose vertices (not part of any face)")
    return joined, notes


def drop_loose(obj):
    """Delete vertices and edges that belong to no face (the exporter drops
    them, so they must not count in the bounds). Returns how many vertices."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    verts = [v for v in bm.verts if not v.link_faces]
    edges = [e for e in bm.edges if not e.link_faces]
    if verts or edges:
        bmesh.ops.delete(bm, geom=edges, context="EDGES_FACES")
        bmesh.ops.delete(bm, geom=[v for v in verts if v.is_valid], context="VERTS")
        bm.to_mesh(obj.data)
    bm.free()
    return len(verts)


def cut_plate(obj, fraction):
    """Delete the faces lying wholly in the bottom `fraction` of the height:
    a ground plate or mound a generator put under the model (a trunk or
    walls standing on it lose only that much of their foot). Returns how
    many faces went."""
    co = vertices(obj)
    lo, hi = co[:, 2].min(), co[:, 2].max()
    cut = lo + fraction * (hi - lo)
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    faces = [f for f in bm.faces if all(v.co.z < cut for v in f.verts)]
    if faces:
        bmesh.ops.delete(bm, geom=faces, context="FACES_ONLY")
        bm.to_mesh(obj.data)
    bm.free()
    drop_loose(obj)
    return len(faces)


def _join(meshes, name):
    bm = bmesh.new()
    mats = []
    for mesh in meshes:
        remap = []
        for mat in mesh.materials:
            if mat not in mats:
                mats.append(mat)
            remap.append(mats.index(mat))
        if not mesh.materials:
            remap = [None]
        for p in mesh.polygons:
            p.material_index = p.material_index if remap[0] is None else remap[min(p.material_index, len(remap) - 1)]
        uv_names = [uv.name for uv in mesh.uv_layers]
        if uv_names and uv_names[0] != "UVMap":
            mesh.uv_layers[0].name = "UVMap"
        bm.from_mesh(mesh)
    out = bpy.data.meshes.new(name)
    bm.to_mesh(out)
    bm.free()
    # bmesh drops custom normals: carry them over when every part has them.
    if all(m.has_custom_normals for m in meshes):
        normals = np.concatenate([_loop_normals(m) for m in meshes])
        if len(normals) == len(out.loops):
            out.normals_split_custom_set(normals.tolist())
    for mat in mats:
        out.materials.append(mat)
    for m in meshes:
        bpy.data.meshes.remove(m)
    obj = bpy.data.objects.new(name, out)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def _loop_normals(mesh):
    n = np.empty(len(mesh.loops) * 3, np.float32)
    mesh.corner_normals.foreach_get("vector", n)
    return n.reshape(-1, 3)


def vertices(obj):
    mesh = obj.data
    co = np.empty(len(mesh.vertices) * 3, np.float64)
    mesh.vertices.foreach_get("co", co)
    return co.reshape(-1, 3)


def normalise(obj, yaw_deg):
    """Turn by yaw (degrees about up; + turns the front from +Z toward +X),
    then base on the ground and centre on x/z. Applied to the mesh data."""
    mesh = obj.data
    if yaw_deg:
        mesh.transform(Matrix.Rotation(math.radians(yaw_deg), 4, "Z"))
    co = vertices(obj)
    lo, hi = co.min(0), co.max(0)
    mesh.transform(Matrix.Translation((-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -lo[2])))
    mesh.update()


def bounds(obj):
    """(lo, hi) in Blender axes."""
    co = vertices(obj)
    return co.min(0), co.max(0)


def gltf_bounds(lo, hi):
    """Blender-axes (lo, hi) -> glTF-axes (min, max)."""
    return (lo[0], lo[2], -hi[1]), (hi[0], hi[2], -lo[1])


def triangles(obj):
    """Triangles the mesh exports as."""
    mesh = obj.data
    counts = np.empty(len(mesh.polygons), np.int32)
    mesh.polygons.foreach_get("loop_total", counts)
    return int((counts - 2).sum())
