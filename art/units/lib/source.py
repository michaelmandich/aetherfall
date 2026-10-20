"""Import a unit's rigged Meshy/Mixamo source GLB, clean it up, and read its clips.

* The skeleton is kept exactly: same bones, names, hierarchy, rest
  transforms and the 0.01 root scale (joint inverse binds stay valid and
  Mixamo clips retarget by name). Only the Blender-side bone *lengths*,
  which the importer sets 100x too long and which glTF never stores, are
  fixed so the rig reads in Blender's viewport and bone-parented props
  behave.
* Body: every mesh skinned to the armature, joined into one (Meshy ships
  one). The skeleton must have every bone `lib/rig.py` names: bones in
  Meshy's rigging-API naming are renamed to the Mixamo names first
  (`rig.canonical_names`), and arm and hand bones rolled half a turn from
  the Mixamo convention (local X toward the character's left) are rolled
  back, so the pose helper's elbow hinge and palm conventions hold. Neither
  moves the mesh. The source's own Walking/Running are then read from the
  file by retargeting (`SourceClip` values are relative to the old roll).
* Material: emissive texture and factor removed, base colour downscaled to
  the unit's texture size and re-encoded as a compact PNG (lib/png.py), metallic 0 /
  plain roughness (the source leaves the glTF defaults, metallic 1, which
  renders as dark metal), specular reset.
* Source clips are resampled onto the 30 fps grid. Meshy writes rotations at
  30 fps with the last sample repeating the first, and the hips translation
  at 24 fps over a slightly different span; every channel is sampled by
  phase over its own period, so the result is one seamless cycle.
"""

import math
import os
import tempfile

import bpy
import numpy as np
from mathutils import Quaternion, Vector

from . import png

FPS = 30
TEXTURE_SIZE = 1024
# Palette size of the base colour PNG (0 = lossless RGB, about twice the
# size). 256 colours: mean error 1.3/255 on the painted texels, 99 % under
# 8/255, invisible at RTS distance; the GLB is about 1 MB smaller.
TEXTURE_COLOURS = 256

# Lengths (armature units, cm) for bones whose child does not continue the
# bone's +Y axis; the others get the distance to that child.
_LEAF_LENGTH = {"Hand": 15.0, "ToeBase": 8.0, "Head": 18.0, "headfront": 5.0, "Hips": 10.0}


class SourceError(RuntimeError):
    pass


SOURCE_CLIPS = ("Walking", "Running")  # the source's own actions read as clips


def load(path, texture_name, body_material, texture_size=TEXTURE_SIZE):
    """Fresh scene with the cleaned source. Returns (armature, body, clips,
    notes): `clips` maps the names in SOURCE_CLIPS the file has to
    `SourceClip`s; `notes` says what was renamed or rolled, and
    `notes.retarget` lists the source clips to read by retargeting instead."""
    from .rig import GROUPS

    if not os.path.isfile(path):
        raise SourceError(f"no source file {path}")
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.fps = FPS
    scene.render.fps_base = 1.0
    bpy.ops.import_scene.gltf(filepath=path, disable_bone_shape=True, bone_heuristic="BLENDER",
                              guess_original_bind_pose=False)
    armatures = [o for o in scene.objects if o.type == "ARMATURE"]
    if len(armatures) != 1:
        raise SourceError(f"{path}: expected one armature, found {len(armatures)} (an unrigged model is a "
                          f"rigid unit: KIND = \"rigid\" in its unit.py)")
    arm = armatures[0]
    notes = Notes()
    renamed = _canonical_bones(arm)
    if renamed:
        spine = [old for old, new in renamed.items() if new.split(":")[-1].startswith("Spine")]
        notes.append(f"renamed {len(renamed)} bones to the Mixamo names (spine from the hips: {' > '.join(spine)})")
    missing = [n for n in GROUPS["all"] if n not in arm.data.bones]
    if missing:
        raise SourceError(f"{path}: the skeleton lacks {missing} (lib/rig.py expects Meshy's Mixamo skeleton)")
    meshes = [o for o in scene.objects if o.type == "MESH"
              and any(m.type == "ARMATURE" and m.object == arm for m in o.modifiers)]
    if not meshes:
        raise SourceError(f"{path}: no mesh skinned to the armature")
    body = _join(meshes)
    for o in list(scene.objects):
        if o not in (arm, body):
            bpy.data.objects.remove(o, do_unlink=True)
    _fix_bone_lengths(arm)
    rolled = _roll_arms(arm)
    if rolled:
        notes.append(f"rolled {len(rolled)} arm and hand bones half a turn to the Mixamo convention")
    _clean_material(body, body_material, texture_name, texture_size)
    clips = {}
    for action in bpy.data.actions:
        if action.name in SOURCE_CLIPS:
            if rolled:
                notes.retarget.append(action.name)
            else:
                clips[action.name] = SourceClip(action)
    ad = arm.animation_data
    if ad:
        for t in list(ad.nla_tracks):
            ad.nla_tracks.remove(t)
        ad.action = None
    for action in list(bpy.data.actions):
        bpy.data.actions.remove(action)
    for pb in arm.pose.bones:
        pb.rotation_mode = "QUATERNION"
        pb.location = (0, 0, 0)
        pb.rotation_quaternion = (1, 0, 0, 0)
        pb.scale = (1, 1, 1)
    bpy.context.view_layer.update()
    return arm, body, clips, notes


class Notes(list):
    """Lines saying what `load` changed; `retarget`: source clips to read
    from the file by retargeting."""

    def __init__(self):
        super().__init__()
        self.retarget = []


def _canonical_bones(arm):
    """Rename the bones to the rig's names (`rig.canonical_names`); Blender
    renames the vertex groups with them. Returns {old: new}."""
    from .rig import canonical_names

    names = canonical_names({b.name: b.parent.name if b.parent else None for b in arm.data.bones})
    renames = {old: new for old, new in names.items() if old != new}
    for old, new in renames.items():  # the new names are prefixed: they never clash with old ones
        arm.data.bones[old].name = new
    return renames


def _roll_arms(arm):
    """Roll back half a turn the arm and hand bones whose local X points to
    the character's right. Returns their names."""
    from .rig import ARM, LEFT, SIDES

    rot = arm.matrix_world.to_3x3()
    turn = [n for s in SIDES for n in ARM[s][1:]
            if (rot @ arm.data.bones[n].matrix_local.to_3x3()).col[0].dot(LEFT) < 0.0]
    if turn:
        bpy.context.view_layer.objects.active = arm
        bpy.ops.object.mode_set(mode="EDIT")
        for n in turn:
            arm.data.edit_bones[n].roll += math.pi
        bpy.ops.object.mode_set(mode="OBJECT")
    return turn


def _join(meshes):
    """One body object from the skinned meshes (the first, if only one)."""
    if len(meshes) == 1:
        return meshes[0]
    with bpy.context.temp_override(active_object=meshes[0], selected_editable_objects=meshes):
        bpy.ops.object.join()
    return meshes[0]


def _fix_bone_lengths(arm):
    before = {b.name: b.matrix_local.copy() for b in arm.data.bones}
    bpy.context.view_layer.objects.active = arm
    arm.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    for eb in arm.data.edit_bones:
        axis = (eb.tail - eb.head).normalized()
        best = None
        for c in eb.children:
            d = c.head - eb.head
            if d.length > 1e-4 and d.normalized().dot(axis) > 0.98:
                best = d.dot(axis) if best is None else min(best, d.dot(axis))
        if best is None:
            key = eb.name.replace("mixamorig:", "").replace("Left", "").replace("Right", "")
            best = _LEAF_LENGTH.get(key, 10.0)
        eb.length = best
    bpy.ops.object.mode_set(mode="OBJECT")
    for b in arm.data.bones:
        m0, m1 = before[b.name], b.matrix_local
        turn = m0.to_quaternion().rotation_difference(m1.to_quaternion()).angle
        turn = min(turn, 2 * math.pi - turn)
        move = (m0.translation - m1.translation).length
        if turn > 1e-4 or move > 0.01:  # radians / centimetres (edit-mode float noise is ~1e-5)
            raise RuntimeError(f"bone {b.name} rest changed ({turn} rad, {move} cm) while fixing its length")


def _clean_material(body, body_material, texture_name, texture_size):
    for mat in body.data.materials:
        mat.name = body_material
        nt = mat.node_tree
        bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
        base_links = bsdf.inputs["Base Color"].links
        base_node = base_links[0].from_node if base_links else None
        for link in list(bsdf.inputs["Emission Color"].links):
            nt.links.remove(link)
        bsdf.inputs["Emission Color"].default_value = (0.0, 0.0, 0.0, 1.0)
        bsdf.inputs["Emission Strength"].default_value = 0.0
        bsdf.inputs["Metallic"].default_value = 0.0
        bsdf.inputs["Roughness"].default_value = 0.8
        bsdf.inputs["Specular IOR Level"].default_value = 0.5
        bsdf.inputs["Specular Tint"].default_value = (1.0, 1.0, 1.0, 1.0)
        bsdf.inputs["IOR"].default_value = 1.5
        for n in list(nt.nodes):
            if n.type == "TEX_IMAGE" and (base_node is None or n.name != base_node.name):
                nt.nodes.remove(n)
        if base_node is not None:
            nt.nodes.active = base_node  # what Workbench's texture mode displays
        if base_node is not None and base_node.image is not None:
            base_node.image = _small_texture(base_node.image, texture_name, texture_size)


def _small_texture(img, name, size):
    """The base colour at `size` px as a compact packed PNG (lib/png.py:
    RGB, TEXTURE_COLOURS palette), which the glTF exporter embeds as-is.
    Checks that the packed file decodes to exactly the pixels encoded."""
    if max(img.size) > size:
        img.scale(size, size)
    w, h = img.size
    px = np.empty(w * h * 4, np.float32)
    img.pixels.foreach_get(px)
    rgb = np.round(px.reshape(h, w, 4)[::-1, :, :3] * 255.0).astype(np.uint8)  # Blender stores rows bottom-up
    data, decoded = png.encode(rgb, TEXTURE_COLOURS)
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, name + ".png")  # the exporter names the glTF image after the file
        with open(path, "wb") as f:
            f.write(data)
        small = bpy.data.images.load(path)
        small.pack()
    small.name = name
    small.colorspace_settings.name = img.colorspace_settings.name
    bpy.data.images.remove(img)
    back = np.empty(w * h * 4, np.float32)
    small.pixels.foreach_get(back)
    back = np.round(back.reshape(h, w, 4)[::-1, :, :3] * 255.0).astype(np.uint8)
    if not np.array_equal(back, decoded):
        raise RuntimeError("the packed base colour PNG does not decode to the encoded pixels")
    return small


class SourceClip:
    """A source action's channels, sampled by phase onto a 30 fps cycle."""

    def __init__(self, action):
        self.name = action.name
        self.channels = {}  # (bone, prop) -> list of per-index value lists
        n_rot = 0
        for fc in action.fcurves:
            bone = fc.data_path.split('"')[1]
            prop = fc.data_path.rsplit(".", 1)[1]
            vals = [k.co[1] for k in fc.keyframe_points]
            self.channels.setdefault((bone, prop), {})[fc.array_index] = vals
            if prop == "rotation_quaternion":
                n_rot = max(n_rot, len(vals))
        # Rotations repeat their first sample at the end: n_rot - 1 frames.
        self.frames = n_rot - 1
        self.duration = self.frames / FPS
        self.periodic = {}
        for key, comps in self.channels.items():
            arrays = [comps[i] for i in sorted(comps)]
            self.periodic[key] = _period_samples(arrays)

    def sample(self, phase):
        """Pose-bone basis at a phase in [0, 1): {bone: (location, quaternion)}."""
        out = {}
        bones = {b for b, _ in self.channels}
        for bone in bones:
            loc = self._channel(bone, "location", phase, 3) or [0.0, 0.0, 0.0]
            q = self._channel(bone, "rotation_quaternion", phase, 4) or [1.0, 0.0, 0.0, 0.0]
            out[bone] = (Vector(loc), Quaternion(q).normalized())
        return out

    def _channel(self, bone, prop, phase, n):
        samples = self.periodic.get((bone, prop))
        if samples is None:
            return None
        return [_periodic_cubic(samples[i], phase) for i in range(n)]


def _period_samples(arrays):
    """Trim the trailing samples that repeat the first one (the loop closure)."""
    count = len(arrays[0])
    last_diff = 0
    for i in range(1, count):
        if any(abs(a[i] - a[0]) > 1e-6 for a in arrays):
            last_diff = i
    period = last_diff + 1 if last_diff else 1
    return [a[:period] for a in arrays]


def _periodic_cubic(samples, phase):
    """Catmull-Rom through evenly spaced periodic samples at phase in [0, 1)."""
    n = len(samples)
    if n == 1:
        return samples[0]
    x = (phase % 1.0) * n
    i = int(x)
    t = x - i
    p0, p1, p2, p3 = (samples[(i + k) % n] for k in (-1, 0, 1, 2))
    if t < 1e-9:
        return p1
    return 0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t
                  + (-p0 + 3 * p1 - 3 * p2 + p3) * t * t * t)
