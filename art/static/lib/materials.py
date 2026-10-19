"""Material cleanup for the game: what Bevy's glTF loader gets.

Per material:
* a Principled BSDF (built from the viewport colour if the importer made
  none);
* the base colour texture is kept, downscaled to the asset's size and
  re-encoded (lib/png.py); every other texture goes: normal,
  metallic-roughness, occlusion, emissive. At RTS distance they cost memory
  and draw time and show nothing;
* no emission; metallic 0 (sources often leave the glTF default of 1, which
  renders as dark metal); roughness 0.8 where it came from a texture; no
  transmission, coat, sheen, specular or IOR extensions;
* alpha: a texture alpha that is really used becomes a cutout (glTF MASK,
  cutoff 0.5) and its texture keeps an alpha channel; blended alpha is
  turned into a cutout too (RTS foliage must not need sorting); an alpha
  read from a separate opacity map is copied into the base colour
  texture's alpha (one RGBA texture); an alpha that is everywhere opaque is
  dropped.

Then materials that are the same (same texture pixels, colour, alpha,
roughness, sidedness) are merged, so the model draws in as few parts as
possible.
"""

import hashlib
import os
import tempfile

import bpy
import numpy as np

from . import png

CUTOFF = 0.5
TEXTURE_COLOURS = 256   # palette size for opaque textures (lib/png.py)
DEFAULT_ROUGHNESS = 0.8

TEAM = "Team"


def principled(mat):
    if not mat.use_nodes:
        mat.use_nodes = True
    nt = mat.node_tree
    bsdf = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
    out = next((n for n in nt.nodes if n.type == "OUTPUT_MATERIAL" and n.is_active_output), None) \
        or next((n for n in nt.nodes if n.type == "OUTPUT_MATERIAL"), None)
    if bsdf is None:
        colour = tuple(mat.diffuse_color)
        for n in list(nt.nodes):
            nt.nodes.remove(n)
        bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
        bsdf.inputs["Base Color"].default_value = colour
        out = None
    if out is None:
        out = nt.nodes.new("ShaderNodeOutputMaterial")
    for link in list(out.inputs["Surface"].links):
        nt.links.remove(link)
    nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    return bsdf


def _upstream_image(socket):
    """(image texture node, name of its output used) feeding `socket`,
    through any chain of nodes; (None, None) if there is none."""
    seen = set()
    stack = [(link.from_node, link.from_socket.name) for link in socket.links]
    while stack:
        node, out = stack.pop()
        if node.name in seen:
            continue
        seen.add(node.name)
        if node.type == "TEX_IMAGE" and node.image is not None:
            return node, out
        for inp in node.inputs:
            stack.extend((link.from_node, link.from_socket.name) for link in inp.links)
    return None, None


def _unlink(nt, socket):
    for link in list(socket.links):
        nt.links.remove(link)


def _set(bsdf, name, value):
    if name in bsdf.inputs:
        bsdf.inputs[name].default_value = value


def clean(mat, notes):
    """Clean one material in place. Returns the base colour image node (or None)."""
    bsdf = principled(mat)
    nt = mat.node_tree
    base, _ = _upstream_image(bsdf.inputs["Base Color"])
    emissive = bool(bsdf.inputs["Emission Color"].links) or (
        bsdf.inputs["Emission Strength"].default_value > 0
        and any(c > 0 for c in bsdf.inputs["Emission Color"].default_value[:3]))
    if emissive:
        notes.append(f"{mat.name}: emission removed")
    for name in ("Emission Color", "Emission Strength", "Normal", "Coat Normal", "Metallic", "Specular IOR Level",
                 "Specular Tint", "IOR", "Transmission Weight", "Coat Weight", "Sheen Weight",
                 "Subsurface Weight", "Anisotropic"):
        if name in bsdf.inputs:
            _unlink(nt, bsdf.inputs[name])
    if bsdf.inputs["Roughness"].links:
        _unlink(nt, bsdf.inputs["Roughness"])
        _set(bsdf, "Roughness", DEFAULT_ROUGHNESS)
    _set(bsdf, "Emission Color", (0.0, 0.0, 0.0, 1.0))
    _set(bsdf, "Emission Strength", 0.0)
    _set(bsdf, "Metallic", 0.0)
    _set(bsdf, "Specular IOR Level", 0.5)
    _set(bsdf, "Specular Tint", (1.0, 1.0, 1.0, 1.0))
    _set(bsdf, "IOR", 1.5)
    for name in ("Transmission Weight", "Coat Weight", "Sheen Weight", "Subsurface Weight", "Anisotropic"):
        _set(bsdf, name, 0.0)

    base = _alpha(mat, bsdf, base, notes)

    # Drop everything that no longer feeds the output (the occlusion group,
    # unused textures), so the exporter cannot pick it up.
    keep = set()
    out = next(n for n in nt.nodes if n.type == "OUTPUT_MATERIAL" and n.is_active_output)
    stack = [out]
    while stack:
        node = stack.pop()
        if node.name in keep:
            continue
        keep.add(node.name)
        for inp in node.inputs:
            stack.extend(link.from_node for link in inp.links)
    for n in list(nt.nodes):
        if n.name not in keep:
            nt.nodes.remove(n)
    if base is not None:
        nt.nodes.active = base
    return base


def _alpha(mat, bsdf, base, notes):
    """Make the material opaque or a clean cutout: the base colour image's
    own alpha -> Round -> Alpha (glTF MASK, cutoff 0.5). An alpha read from
    another image (an FBX opacity map), or from a colour output, is copied
    into the base colour image's alpha first, so the model keeps one RGBA
    texture. Returns the base colour image node (new if one was made)."""
    nt = mat.node_tree
    alpha = bsdf.inputs["Alpha"]
    if not alpha.links:
        if alpha.default_value < 1.0:
            notes.append(f"{mat.name}: alpha {alpha.default_value:.2f} made opaque")
            alpha.default_value = 1.0
        return base
    src, out = _upstream_image(alpha)
    if src is None:
        _unlink(nt, alpha)
        alpha.default_value = 1.0
        notes.append(f"{mat.name}: alpha not from a texture; made opaque")
        return base
    px = _pixels(src.image)
    a = px[..., 3] if out == "Alpha" else px[..., :3].mean(axis=2)
    if (a >= CUTOFF).all():
        _unlink(nt, alpha)
        alpha.default_value = 1.0
        return base
    clipped = alpha.links[0].from_node
    if not (clipped.type == "MATH" and clipped.operation in ("ROUND", "SUBTRACT", "GREATER_THAN", "LESS_THAN")):
        notes.append(f"{mat.name}: blended alpha made a cutout ({CUTOFF})")
    if base is None or src != base or out != "Alpha":
        base = _with_alpha(mat, bsdf, base, a, notes)
    _unlink(nt, alpha)
    rnd = nt.nodes.new("ShaderNodeMath")
    rnd.operation = "ROUND"
    nt.links.new(base.outputs["Alpha"], rnd.inputs[0])
    nt.links.new(rnd.outputs["Value"], alpha)
    mat["af_cutout"] = True
    return base


def _with_alpha(mat, bsdf, base, a, notes):
    """A new base colour image: the old one's colour (or the flat base
    colour) with alpha `a` (h, w, top row first) resampled into it."""
    nt = mat.node_tree
    if base is not None:
        rgba = _pixels(base.image).copy()
    else:
        colour = np.array(bsdf.inputs["Base Color"].default_value, np.float32)
        rgba = np.broadcast_to(colour, a.shape + (4,)).copy()
    h, w = rgba.shape[:2]
    rows = (np.arange(h) * a.shape[0] // h)[:, None]
    cols = (np.arange(w) * a.shape[1] // w)[None, :]
    rgba[..., 3] = a[rows, cols]
    img = bpy.data.images.new(f"{mat.name}_rgba", w, h, alpha=True)
    img.pixels.foreach_set(np.ascontiguousarray(rgba[::-1]).ravel())
    img.pack()
    node = nt.nodes.new("ShaderNodeTexImage")
    node.image = img
    if base is None:
        nt.links.new(node.outputs["Color"], bsdf.inputs["Base Color"])
    else:
        for link in list(base.outputs["Color"].links):
            nt.links.new(node.outputs["Color"], link.to_socket)
        node.interpolation = base.interpolation
    notes.append(f"{mat.name}: alpha from a separate map copied into the base colour texture")
    return node


def uses_cutout(mat):
    return bool(mat.get("af_cutout"))


def _pixels(img):
    """(h, w, 4) float32, top row first."""
    w, h = img.size
    px = np.empty(w * h * 4, np.float32)
    img.pixels.foreach_get(px)
    return px.reshape(h, w, 4)[::-1]


def shrink_textures(obj, limit, name, notes, tint=None):
    """Downscale (keeping the aspect) and re-encode every base colour image,
    multiplying its colour (sRGB values) by `tint` when given. Images shared
    by several materials are done once. Returns how many images there were."""
    done = {}
    for mat in (m for m in obj.data.materials if m is not None):
        for node in (n for n in mat.node_tree.nodes if n.type == "TEX_IMAGE" and n.image is not None):
            img = node.image
            key = (img.name, uses_cutout(mat))
            if key not in done:
                done[key] = _shrink(img, limit, f"{name}_{len(done)}", uses_cutout(mat), notes, tint)
            node.image = done[key]
    for img in list(bpy.data.images):
        if img.users == 0:
            bpy.data.images.remove(img)
    return len(done)


def _shrink(img, limit, new_name, cutout, notes, tint=None):
    w, h = img.size
    if w == 0 or h == 0:
        raise RuntimeError(f"texture {img.name} could not be read ({img.filepath})")
    if max(w, h) > limit:
        f = limit / max(w, h)
        nw, nh = max(1, round(w * f)), max(1, round(h * f))
        notes.append(f"texture {img.name} {w}x{h} -> {nw}x{nh}")
        img = img.copy()
        img.scale(nw, nh)
        w, h = nw, nh
    px = _pixels(img)
    if tint is not None:
        px = px.copy()
        px[..., :3] *= np.asarray(tint, np.float32)
    px = np.round(np.clip(px, 0.0, 1.0) * 255.0).astype(np.uint8)
    if cutout:
        data, _ = png.encode_rgba_cutout(px, CUTOFF)
    else:
        data, _ = png.encode_rgb(px[..., :3], TEXTURE_COLOURS)
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, new_name + ".png")  # the exporter names the image after the file
        with open(path, "wb") as f:
            f.write(data)
        small = bpy.data.images.load(path)
        small.pack()
    small.name = new_name
    small.colorspace_settings.name = "sRGB"
    small.alpha_mode = "STRAIGHT" if cutout else "NONE"
    small["af_bytes"] = len(data)
    return small


def signature(mat):
    """What makes two cleaned materials draw the same."""
    nt = mat.node_tree
    bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
    node = next((n for n in nt.nodes if n.type == "TEX_IMAGE" and n.image is not None), None)
    if node is not None:
        px = np.empty(node.image.size[0] * node.image.size[1] * 4, np.float32)
        node.image.pixels.foreach_get(px)
        tex = hashlib.sha1(np.round(px * 255).astype(np.uint8).tobytes()).hexdigest()
    else:
        tex = None
    colour = bsdf.inputs["Base Color"]
    base = None if colour.links else tuple(round(c, 3) for c in colour.default_value)
    return (tex, base, uses_cutout(mat), round(bsdf.inputs["Roughness"].default_value, 2),
            mat.use_backface_culling, tuple(sorted(n.bl_idname for n in nt.nodes)))


def merge_duplicates(obj, notes):
    """Merge materials that draw the same; drop unused slots."""
    mesh = obj.data
    mats = list(mesh.materials)
    if not mats:
        return
    sigs = [signature(m) if m is not None else None for m in mats]
    first = {}
    remap = []
    for i, s in enumerate(sigs):
        remap.append(first.setdefault(s, i))
    idx = np.empty(len(mesh.polygons), np.int32)
    mesh.polygons.foreach_get("material_index", idx)
    idx = np.array(remap, np.int32)[np.clip(idx, 0, len(remap) - 1)]
    used = sorted(set(idx.tolist()))
    new_index = {old: new for new, old in enumerate(used)}
    idx = np.array([new_index[i] for i in idx], np.int32) if len(idx) else idx
    keep = [mats[i] for i in used]
    merged = len(mats) - len(keep)
    mesh.materials.clear()
    for m in keep:
        mesh.materials.append(m)
    mesh.polygons.foreach_set("material_index", idx)
    mesh.update()
    if merged:
        notes.append(f"{merged} duplicate or unused material slot(s) merged")


def flat_material(name, colour, roughness=0.8):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*colour, 1.0)
    bsdf.inputs["Roughness"].default_value = roughness
    bsdf.inputs["Metallic"].default_value = 0.0
    mat.diffuse_color = (*colour, 1.0)
    return mat
