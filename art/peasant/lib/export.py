"""GLB export, the engine manifest, and a check of what was written."""

import json
import os
import struct

import bpy

PNG_MAGIC = bytes((0x89, 0x50, 0x4E, 0x47))


def export_glb(path):
    """Export the scene's character (armature, body, sash, props, all clips)."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        export_yup=True,
        export_apply=False,
        export_texcoords=True,
        export_normals=True,
        export_materials="EXPORT",
        export_image_format="AUTO",
        export_cameras=False,
        export_lights=False,
        export_extras=False,
        export_skins=True,
        export_def_bones=False,
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_force_sampling=True,
        export_frame_step=1,
        export_anim_slide_to_zero=False,
        export_optimize_animation_size=True,
        export_optimize_animation_keep_anim_armature=True,
        export_reset_pose_bones=True,
        export_rest_position_armature=True,
        export_morph=False,
    )


def export_fbx_for_mixamo(path, arm, body):
    """The character for mixamo.com: skinned body and skeleton in the rest
    pose, no props or animation. Mixamo keeps an uploaded `mixamorig`
    skeleton, so its clips come back on exactly this rest pose
    (lib/retarget.py checks that)."""
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    for o in bpy.context.scene.objects:
        o.select_set(o in (arm, body))
    bpy.context.view_layer.objects.active = arm
    bpy.ops.export_scene.fbx(filepath=path, use_selection=True, object_types={"ARMATURE", "MESH"},
                             add_leaf_bones=False, bake_anim=False, path_mode="COPY", embed_textures=True)


def _glb_chunks(path):
    """(JSON dict, binary chunk bytes) of a GLB file."""
    with open(path, "rb") as f:
        data = f.read()
    if struct.unpack_from("<I", data, 0)[0] != 0x46546C67:  # "glTF"
        raise ValueError(f"{path} is not a GLB")
    json_len = struct.unpack_from("<I", data, 12)[0]
    js = json.loads(data[20:20 + json_len])
    off = 20 + json_len
    bin_len = struct.unpack_from("<I", data, off)[0]
    return js, data[off + 8:off + 8 + bin_len]


def _node_matrix(n):
    import numpy as np
    if "matrix" in n:
        return np.array(n["matrix"], np.float64).reshape(4, 4).T
    x, y, z, w = n.get("rotation", (0.0, 0.0, 0.0, 1.0))
    r = np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                  [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                  [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])
    m = np.eye(4)
    m[:3, :3] = r @ np.diag(n.get("scale", (1.0, 1.0, 1.0)))
    m[:3, 3] = n.get("translation", (0.0, 0.0, 0.0))
    return m


def rest_centroids(path):
    """Rest-pose world centroid (Blender axes, metres) of every unskinned mesh
    node, computed from the GLB's own node tree and vertex buffers."""
    import numpy as np
    js, buf = _glb_chunks(path)
    parent = {c: i for i, n in enumerate(js["nodes"]) for c in n.get("children", [])}
    out = {}
    for i, n in enumerate(js["nodes"]):
        if "mesh" not in n or "skin" in n:
            continue
        m, j = _node_matrix(n), i
        while j in parent:
            j = parent[j]
            m = _node_matrix(js["nodes"][j]) @ m
        pts = []
        for prim in js["meshes"][n["mesh"]]["primitives"]:
            acc = js["accessors"][prim["attributes"]["POSITION"]]
            view = js["bufferViews"][acc["bufferView"]]
            start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
            pts.append(np.frombuffer(buf, np.float32, acc["count"] * 3, start).reshape(-1, 3))
        p = np.concatenate(pts) @ m[:3, :3].T + m[:3, 3]
        c = p.mean(0)
        out[n["name"]] = (float(c[0]), float(-c[2]), float(c[1]))  # glTF Y-up -> Blender Z-up
    return out


def summarize(path):
    """What the engine will see: animations (name, duration, channels),
    named nodes, materials and images."""
    js, blob = _glb_chunks(path)
    acc = js["accessors"]
    anims = {}
    for a in js.get("animations", []):
        dur = max(acc[s["input"]]["max"][0] for s in a["samplers"])
        start = min(acc[s["input"]]["min"][0] for s in a["samplers"])
        targets = {(c["target"]["node"], c["target"]["path"]) for c in a["channels"]}
        anims[a["name"]] = dict(duration=dur, start=start, channels=len(a["channels"]),
                                nodes=len({n for n, _ in targets}))
    nodes = {n.get("name"): n for n in js["nodes"]}
    joints = set()
    for skin in js.get("skins", []):
        joints |= {js["nodes"][j]["name"] for j in skin["joints"]}
    materials = []
    for m in js.get("materials", []):
        materials.append(dict(name=m.get("name"), emissive=m.get("emissiveFactor"),
                              emissive_tex="emissiveTexture" in m, ext=sorted(m.get("extensions", {}))))
    images = []
    for i in js.get("images", []):
        view = js["bufferViews"][i["bufferView"]]
        data = blob[view.get("byteOffset", 0):view.get("byteOffset", 0) + view["byteLength"]]
        size = struct.unpack(">II", data[16:24]) if data.startswith(PNG_MAGIC) else None  # IHDR width, height
        images.append(dict(name=i.get("name"), mime=i.get("mimeType"), bytes=len(data), size=size))
    return dict(animations=anims, nodes=nodes, joints=joints, materials=materials, images=images,
                skins=len(js.get("skins", [])))


def verify(path, clips, prop_names, bone_names, glowing=(), prop_rest=None, texture_size=None):
    """Raise if the GLB does not match the build: every clip present with its
    duration, every prop / bone node present by name, nothing emissive but
    the `glowing` materials, one PNG texture of `texture_size` px, and
    (given `prop_rest`, name -> rest centroid) every prop where Blender had
    it in the rest pose."""
    s = summarize(path)
    problems = []
    if prop_rest:
        got = rest_centroids(path)
        for name, want in prop_rest.items():
            if name in got:
                err = sum((a - b) ** 2 for a, b in zip(got[name], want)) ** 0.5
                if err > 1e-3:
                    problems.append(f"{name} exported {err * 100:.1f} cm away from its rest placement")
    for clip in clips:
        a = s["animations"].get(clip.name)
        if a is None:
            problems.append(f"animation {clip.name} missing")
            continue
        if abs(a["duration"] - clip.duration) > 1e-3 or abs(a["start"]) > 1e-6:
            problems.append(f"{clip.name}: exported {a['start']:.4f}..{a['duration']:.4f}s, built {clip.duration:.4f}s")
        if a["nodes"] < len(bone_names):
            problems.append(f"{clip.name}: animates {a['nodes']} nodes, expected all {len(bone_names)} bones")
    extra = set(s["animations"]) - {c.name for c in clips}
    if extra:
        problems.append(f"unexpected animations {sorted(extra)}")
    for name in list(prop_names) + ["Team_sash"]:
        if name not in s["nodes"]:
            problems.append(f"node {name} missing")
    missing = set(bone_names) - s["joints"]
    if missing:
        problems.append(f"joints missing: {sorted(missing)}")
    team = [m for m in s["materials"] if m["name"] == "Team"]
    if not team:
        problems.append("material Team missing")
    for m in s["materials"]:
        glows = m["emissive_tex"] or (m["emissive"] and any(m["emissive"]))
        if glows and m["name"] not in glowing:
            problems.append(f"material {m['name']} is emissive")
    if texture_size is not None:
        sizes = [i["size"] for i in s["images"]]
        if sizes != [(texture_size, texture_size)]:
            problems.append(f"textures {sizes}, expected one {texture_size} px PNG")
    if problems:
        raise RuntimeError("GLB check failed:\n  " + "\n  ".join(problems))
    return s


def write_manifest(path, model, height, clips):
    """peasant.model.ron, in the contract's format (docs/art/peasant.md)."""
    lines = [
        "// Written by art/peasant/build_peasant.py; do not edit by hand.",
        "// Clip durations in seconds; speed = ground speed in m/s at playback",
        "// rate 1 (model scale 1); hit = impact time in seconds; reach = how far",
        "// ahead the work lands in metres (model scale 1).",
        "(",
        f'    model: "{model}",',
        f"    height: {height:.3f},",
        "    clips: {",
    ]
    for c in sorted(clips, key=lambda c: c.name):
        fields = [f"duration: {c.duration:.4f}", f"looping: {'true' if c.loop else 'false'}"]
        if c.speed is not None:
            fields.append(f"speed: {c.speed:.3f}")
        if c.hit is not None:
            fields.append(f"hit: {c.hit:.4f}")
        if c.reach is not None:
            fields.append(f"reach: {c.reach:.3f}")
        lines.append(f'        "{c.name}": ({", ".join(fields)}),')
    lines += ["    },", ")", ""]
    with open(path, "w", newline="\n", encoding="utf-8") as f:
        f.write("\n".join(lines))
