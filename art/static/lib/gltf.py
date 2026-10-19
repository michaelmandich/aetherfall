"""GLB export, and a read-back of what was written (what Bevy will load)."""

import json
import os
import struct

import bpy
import numpy as np

from .util import quiet

PNG_MAGIC = b"\x89PNG"


def export(path, objects):
    """Export exactly `objects` (static meshes, transforms applied) as GLB."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    for o in bpy.context.scene.objects:
        o.select_set(o in objects)
    bpy.context.view_layer.objects.active = objects[0]
    with quiet():
        _export(path)


def _export(path):
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_apply=True,
        export_texcoords=True,
        export_normals=True,
        export_tangents=False,
        export_materials="EXPORT",
        export_image_format="AUTO",
        export_cameras=False,
        export_lights=False,
        export_extras=False,
        export_skins=False,
        export_animations=False,
        export_morph=False,
        export_draco_mesh_compression_enable=False,
    )


def read(path):
    """(JSON dict, binary chunk) of a GLB."""
    with open(path, "rb") as f:
        data = f.read()
    if struct.unpack_from("<I", data, 0)[0] != 0x46546C67:  # "glTF"
        raise ValueError(f"{path} is not a GLB")
    json_len = struct.unpack_from("<I", data, 12)[0]
    js = json.loads(data[20:20 + json_len])
    off = 20 + json_len
    if off >= len(data):
        return js, b""
    bin_len = struct.unpack_from("<I", data, off)[0]
    return js, data[off + 8:off + 8 + bin_len]


def summarize(path):
    """Nodes, per-node triangles and bounds (glTF axes), materials, images."""
    js, blob = read(path)
    acc = js.get("accessors", [])
    nodes = {}
    for n in js.get("nodes", []):
        if "mesh" not in n:
            nodes[n.get("name")] = dict(triangles=0, min=None, max=None, materials=[], transform=_has_transform(n))
            continue
        tris, lo, hi, mats = 0, np.full(3, np.inf), np.full(3, -np.inf), []
        for prim in js["meshes"][n["mesh"]]["primitives"]:
            if prim.get("mode", 4) != 4:
                raise ValueError(f"{path}: node {n.get('name')} has non-triangle primitives")
            count = acc[prim["indices"]]["count"] if "indices" in prim else acc[prim["attributes"]["POSITION"]]["count"]
            tris += count // 3
            pos = acc[prim["attributes"]["POSITION"]]
            lo = np.minimum(lo, pos["min"])
            hi = np.maximum(hi, pos["max"])
            if "material" in prim:
                mats.append(js["materials"][prim["material"]].get("name"))
        nodes[n.get("name")] = dict(triangles=tris, min=tuple(lo), max=tuple(hi), materials=mats,
                                    transform=_has_transform(n))
    materials = []
    for m in js.get("materials", []):
        pbr = m.get("pbrMetallicRoughness", {})
        materials.append(dict(
            name=m.get("name"),
            emissive=bool(m.get("emissiveTexture")) or any(m.get("emissiveFactor", (0, 0, 0))),
            alpha=m.get("alphaMode", "OPAQUE"),
            textures=sorted(k for k in ("normalTexture", "occlusionTexture", "emissiveTexture") if k in m)
            + (["metallicRoughnessTexture"] if "metallicRoughnessTexture" in pbr else []),
            base_texture="baseColorTexture" in pbr,
            metallic=pbr.get("metallicFactor", 1.0),
            extensions=sorted(m.get("extensions", {})),
        ))
    images = []
    for i in js.get("images", []):
        view = js["bufferViews"][i["bufferView"]]
        data = blob[view.get("byteOffset", 0):view.get("byteOffset", 0) + view["byteLength"]]
        size = struct.unpack(">II", data[16:24]) if data.startswith(PNG_MAGIC) else None
        images.append(dict(name=i.get("name"), mime=i.get("mimeType"), bytes=len(data), size=size))
    return dict(nodes=nodes, materials=materials, images=images, skins=len(js.get("skins", [])),
                animations=len(js.get("animations", [])), bytes=os.path.getsize(path),
                extensions=sorted(js.get("extensionsUsed", [])))


def _has_transform(n):
    return any(k in n for k in ("matrix", "rotation", "scale")) or any(abs(v) > 1e-6 for v in n.get("translation", ()))


def verify(path, model_node, banner_node, texture_limit):
    """Raise unless the GLB is what the engine expects. Returns the summary."""
    s = summarize(path)
    problems = []
    if s["skins"] or s["animations"]:
        problems.append(f"{s['skins']} skin(s), {s['animations']} animation(s): must be static")
    if model_node not in s["nodes"]:
        problems.append(f"node {model_node} missing (have {sorted(s['nodes'])})")
    if banner_node:
        b = s["nodes"].get(banner_node)
        if b is None:
            problems.append(f"node {banner_node} missing")
        elif "Team" not in b["materials"]:
            problems.append(f"{banner_node} does not use the Team material")
    elif "Team_banner" in s["nodes"]:
        problems.append("unexpected Team_banner node")
    for name, n in s["nodes"].items():
        if n["transform"]:
            problems.append(f"node {name} has a transform (the build applies them)")
    for m in s["materials"]:
        if m["emissive"]:
            problems.append(f"material {m['name']} is emissive")
        if m["textures"]:
            problems.append(f"material {m['name']} has {m['textures']}")
        if m["metallic"] > 0:
            problems.append(f"material {m['name']} is metallic")
        if m["alpha"] == "BLEND":
            problems.append(f"material {m['name']} blends (alpha must be a cutout)")
    for i in s["images"]:
        if i["mime"] != "image/png" or i["size"] is None:
            problems.append(f"image {i['name']} is {i['mime']}, not PNG")
        elif max(i["size"]) > texture_limit:
            problems.append(f"image {i['name']} is {i['size'][0]}x{i['size'][1]}, over {texture_limit} px")
    if problems:
        raise RuntimeError(f"{path}: GLB check failed:\n  " + "\n  ".join(problems))
    return s
