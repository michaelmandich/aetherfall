"""Clips from other files: Mixamo FBX downloads, or any glTF / FBX whose
armature uses the peasant's Mixamo bone names (`mixamorig:Hips`, ...; any
`<prefix>:` before the name, such as Mixamo's `mixamorig1:`, matches).

    clip.from_file("mixamo/Waving.fbx")      # in a clip module's build()

(paths are relative to art/peasant/source/). The file is imported into the
build's scene, each of its actions is sampled frame by frame, and every
bone's motion is carried over as a *world-space rotation from its rest
pose*: the peasant's bone turns from its own rest exactly as the file's bone
turned from the file's rest. That is independent of the file's axis
convention, scale and bone rolls (FBX importers change them). It is exact
when the two rest poses agree, as they do when the clip was made on this
skeleton (upload the peasant to Mixamo, see art/peasant/README.md). When
they differ (a T-pose file, this rig's arms-down rest), each bone is first
aligned by two directions: where its limb points and the plane it bends in
(`_reference`), and the build says so. Bones then point where the file's
point; a twist about a straight limb's own axis that only the rest poses
disagree on cannot be recovered. A test with the arms' rest raised 40 deg
reproduces a clip to within 3 deg.

The pelvis's travel is scaled by the ratio of the two pelvis heights. The
result plugs into `Clip.from_source`'s pipeline (in place, feet put on the
floor), like the Meshy source's own Walking and Running.
"""

import math
import os

import bpy
from mathutils import Matrix, Vector

from .rig import HIPS

REST_TOLERANCE = 5.0  # degrees: rest poses further apart than this are reported
LOOP_SEAM = 0.5       # degrees: a looping action whose last frame matches its first repeats it


class RetargetError(RuntimeError):
    pass


def import_clips(path, rig, loop):
    """{action name: RetargetedClip} for every action in the file at `path`."""
    before = {k: set(getattr(bpy.data, k)) for k in _DATA}
    ext = os.path.splitext(path)[1].lower()
    scene = bpy.context.scene
    fps = scene.render.fps, scene.render.fps_base
    frame = scene.frame_current
    try:
        if ext == ".fbx":
            bpy.ops.import_scene.fbx(filepath=path, use_anim=True, ignore_leaf_bones=False,  # keep the hands
                                     automatic_bone_orientation=False)
        elif ext in (".glb", ".gltf"):
            bpy.ops.import_scene.gltf(filepath=path, bone_heuristic="BLENDER", guess_original_bind_pose=False)
        else:
            raise RetargetError(f"{path}: only .fbx, .glb and .gltf files are read")
        new = [o for o in bpy.data.objects if o not in before["objects"]]
        armatures = [o for o in new if o.type == "ARMATURE" and _HIPS in _names(o)]
        if len(armatures) != 1:
            raise RetargetError(f"{path}: expected one armature with a {HIPS} bone, found {len(armatures)}")
        src = armatures[0]
        actions = [a for a in bpy.data.actions if a not in before["actions"]]
        if not actions:
            raise RetargetError(f"{path}: no animation")
        names, align = _match(path, src, rig)
        return {a.name: _sample(src, a, rig, names, align, loop) for a in actions}
    finally:
        scene.render.fps, scene.render.fps_base = fps
        scene.frame_set(frame)
        for kind in _DATA:
            coll = getattr(bpy.data, kind)
            for block in [b for b in coll if b not in before[kind]]:
                coll.remove(block)


_DATA = ("objects", "armatures", "meshes", "materials", "images", "actions", "cameras", "lights")
_HIPS = HIPS.split(":")[-1]


def _names(src):
    """The file's bone names by their name without the `<prefix>:`."""
    return {b.name.split(":")[-1]: b.name for b in src.data.bones}


def _rest(src):
    """World rest rotation and head position of every bone of `src`."""
    mw = src.matrix_world
    rot = mw.to_3x3().normalized()
    return {b.name: (rot @ b.matrix_local.to_3x3().normalized(), mw @ b.head_local) for b in src.data.bones}


def _match(path, src, rig):
    """({rig bone: file bone}, {rig bone: alignment}) for the bones they
    share. An alignment is the world rotation that takes the rig's rest pose
    of a bone onto the file's (identity when the rest poses agree), found
    from two directions per bone (`_reference`): where its limb points and
    the plane it bends in. A bone with no child takes its parent's."""
    rest, by_name = _rest(src), _names(src)
    names = {n: by_name[n.split(":")[-1]] for n in rig.order if n.split(":")[-1] in by_name}
    ours = {n: rig.rest_pos[n] for n in names}
    theirs = {n: rest[f][1] for n, f in names.items()}
    align, worst = {}, (0.0, None)
    for name in names:
        a, b = _reference(rig, names, name, ours), _reference(rig, names, name, theirs)
        if a is None or b is None:
            parent = rig.parent[name]
            align[name] = align[parent].copy() if parent in align else Matrix.Identity(3)
            continue
        if len(a) == 2 and len(b) == 2:
            align[name] = _basis(*b) @ _basis(*a).transposed()
        else:
            align[name] = a[0].rotation_difference(b[0]).to_matrix()
        angle = math.degrees(align[name].to_quaternion().angle)
        if angle > worst[0]:
            worst = (angle, name)
    if worst[0] > REST_TOLERANCE:
        print(f"{os.path.basename(path)}: its rest pose differs from the peasant's (up to {worst[0]:.0f} deg, "
              f"{worst[1]}); bones are aligned by direction")
    return names, align


def _reference(rig, names, name, pos):
    """One or two directions that pin a bone's rest orientation: to its
    child, and to its second child, or else from its child on to the
    grandchild, or else from its parent (the plane the limb bends in).
    None for a bone with no child."""
    kids = [c for c in rig.order if rig.parent.get(c) == name and c in names]
    if not kids:
        return None
    dirs = [pos[kids[0]] - pos[name]]
    grand = [c for c in rig.order if rig.parent.get(c) == kids[0] and c in names]
    parent = rig.parent[name]
    if len(kids) > 1:
        dirs.append(pos[kids[1]] - pos[name])
    elif grand:
        dirs.append(pos[grand[0]] - pos[kids[0]])
    elif parent in names:
        dirs.append(pos[name] - pos[parent])
    if min(d.length for d in dirs) < 1e-6:
        return None
    if len(dirs) == 2 and dirs[0].normalized().cross(dirs[1].normalized()).length < 1e-3:
        dirs = dirs[:1]  # a straight limb: the bend plane is undefined
    return dirs


def _basis(a, b):
    """Orthonormal frame (columns) from a direction and a second one."""
    x = a.normalized()
    y = (b - x * b.dot(x)).normalized()
    return Matrix((x, y, x.cross(y))).transposed()


def _sample(src, action, rig, names, align, loop):
    from .clip import assign

    scene = bpy.context.scene
    assign(src, action)
    first, last = (int(round(x)) for x in action.frame_range)
    rest = _rest(src)
    src_rot = src.matrix_world.to_3x3().normalized()
    hips_height = rest[names[HIPS]][1].z - min(p.z for _, p in rest.values())
    scale = (rig.rest_pos[HIPS].z - min(p.z for p in rig.rest_pos.values())) / max(hips_height, 1e-6)
    frames = []
    for f in range(first, last + 1):
        scene.frame_set(f)
        world = {}
        for name, theirs in names.items():
            pb = src.pose.bones[theirs]
            delta = (src_rot @ pb.matrix.to_3x3().normalized()) @ rest[theirs][0].transposed()
            world[name] = delta @ align[name] @ rig.rest_rot[name]
        basis = {}
        for name in names:
            p = rig.parent[name]
            w_parent = world.get(p, rig.rest_rot[p]) if p else None
            basis[name] = (Vector(), rig.basis_rot(name, world[name], w_parent))
        moved = (src.matrix_world @ src.pose.bones[names[HIPS]].head) - rest[names[HIPS]][1]
        basis[HIPS] = (rig.root_loc(rig.rest_pos[HIPS] + moved * scale), basis[HIPS][1])
        frames.append(basis)
    assign(src, None)
    return RetargetedClip(action.name, frames, scene.render.fps / scene.render.fps_base, loop)


class RetargetedClip:
    """Frames of a retargeted action, sampled by phase like source.SourceClip."""

    def __init__(self, name, frames, fps, loop):
        self.name = name
        if loop and len(frames) > 2 and _same(frames[0], frames[-1]):
            frames = frames[:-1]  # the last frame repeats the first: one cycle
        self._frames = frames
        self.loop = loop
        span = len(frames) if loop else len(frames) - 1
        self.frames = max(1, round(span * 30.0 / fps))  # on the build's 30 fps grid
        self.duration = self.frames / 30.0

    def sample(self, phase):
        n = len(self._frames)
        span = n if self.loop else n - 1
        x = (phase % 1.0 if self.loop else max(0.0, min(1.0, phase))) * span
        i = min(int(x), span - 1) if span > 0 else 0
        t = x - i
        a, b = self._frames[i % n], self._frames[(i + 1) % n]
        return {name: (la.lerp(b[name][0], t), qa.slerp(b[name][1], t)) for name, (la, qa) in a.items()}


def _same(a, b):
    return all(math.degrees(a[n][1].rotation_difference(b[n][1]).angle) < LOOP_SEAM for n in a)


def round_trip(ctx, arm, body, name):
    """Export a built clip to FBX with deliberately different bone axes,
    read it back through `import_clips` and return the worst rotation
    difference (degrees) and pelvis offset (metres) against the original:
    both must be ~0 (build_peasant.py --check-retarget)."""
    import tempfile

    from .clip import _source_frames, assign

    clip = ctx.built(name)
    if clip.action is None:
        clip.bake(arm)
    scene = bpy.context.scene
    keep = scene.frame_start, scene.frame_end, [o for o in scene.objects if o.select_get()]
    assign(arm, clip.action)
    scene.frame_start, scene.frame_end = 0, clip.frames
    for o in scene.objects:
        o.select_set(o in (arm, body))
    try:
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, name + ".fbx")
            bpy.ops.export_scene.fbx(filepath=path, use_selection=True, object_types={"ARMATURE", "MESH"},
                                     add_leaf_bones=False, bake_anim=True, bake_anim_use_all_actions=False,
                                     bake_anim_use_nla_strips=False, bake_anim_force_startend_keying=True,
                                     bake_anim_simplify_factor=0.0, primary_bone_axis="X", secondary_bone_axis="Y")
            assign(arm, None)
            back = next(iter(import_clips(path, ctx.rig, clip.loop).values()))
    finally:
        assign(arm, None)
        scene.frame_start, scene.frame_end = keep[0], keep[1]
        for o in scene.objects:
            o.select_set(o in keep[2])
    frames = _source_frames(ctx.rig, back, False, clip.frames, clip.loop)
    if len(frames) != len(clip.basis):
        raise RetargetError(f"{name}: {len(frames)} frames came back, {len(clip.basis)} went out")
    rot = loc = 0.0
    for a, b in zip(frames, clip.basis):
        for bone, (la, qa) in a.items():
            lb, qb = b[bone]
            angle = math.degrees(qa.rotation_difference(qb).angle)
            rot = max(rot, min(angle, 360.0 - angle))
            loc = max(loc, (la - lb).length * ctx.rig.scale)
    return rot, loc
