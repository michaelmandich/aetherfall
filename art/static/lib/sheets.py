"""Contact sheets: every part of an asset as the game will draw it.

The sheet imports the GLB files just written (so it shows what the engine
loads, not the Blender scene) and scales each one as the engine fits it to
the game's box (the stump at its tree's scale). The team material is tinted
blue.

    columns  the variants, then the stump
    rows     front (orthographic, from +Z), side (orthographic, from +X),
             three-quarter (perspective, from the front-left, above),
             rts (the game's camera at its start zoom, 66 m, at the game's
             pixel scale on a 1080 px screen: what a player sees; zoomed
             out, and labelled so, when a big building does not fit)
    bottom   "in game": nodes as a patch of nine (the variants mixed, turned
             at random when random_yaw is on), decor as its variants side
             by side at the size the game draws them, buildings as three copies, rising out
             of the ground at 40 %, 75 % and 100 % build progress and
             tinted red, blue and green, all from the game's camera
             (enlarged 2x or 3x when that still fits, zoomed out when even
             1x does not; the label says)

The box the engine fits to is drawn as a magenta wireframe on the first
three rows. Workbench render, flat studio light with shadows, 1 m checker
ground.
"""

import math
import os
import random

import bpy
import numpy as np
from mathutils import Matrix, Vector

from .util import quiet

TILE_W, TILE_H = 260, 300
MIN_COLUMNS = 4
VIEWS = ("front", "side", "tq", "rts")

# Game camera (crates/af_input/src/camera.rs): FOV 32 deg (vertical), pitch
# 0.95 rad, looking along -X -Z (glTF) from the +X +Z side, start zoom 66 m.
GAME_FOV = math.radians(32.0)
GAME_PITCH = 0.95
GAME_DISTANCE = 66.0
SCREEN_H = 1080
GAME_MPP = 2.0 * GAME_DISTANCE * math.tan(GAME_FOV / 2) / SCREEN_H

TEAM_COLOURS = ((0.85, 0.16, 0.12), (0.16, 0.36, 0.85), (0.20, 0.62, 0.22))
BOX_COLOUR = (0.95, 0.10, 0.80)


def _game_dir():
    """View direction of the game camera in Blender axes (Z up, glTF +Z = -Y)."""
    c, s = math.cos(GAME_PITCH), math.sin(GAME_PITCH)
    return Vector((-c / math.sqrt(2), c / math.sqrt(2), -s)).normalized()


def _setup_scene():
    from . import scene as sc
    sc.reset()
    scn = bpy.context.scene
    scn.render.engine = "BLENDER_WORKBENCH"
    scn.view_settings.view_transform = "Standard"
    scn.view_settings.exposure = 0.35  # closer to the game's brightness
    sh = scn.display.shading
    sh.light = "STUDIO"
    sh.color_type = "TEXTURE"
    sh.show_shadows = True
    sh.shadow_intensity = 0.45
    sh.show_cavity = True
    sh.cavity_type = "WORLD"
    scn.display.light_direction = (0.45, -0.35, 0.82)
    scn.world = bpy.data.worlds.new("SheetWorld")
    scn.world.color = (0.80, 0.82, 0.85)
    scn.render.image_settings.file_format = "PNG"
    scn.render.image_settings.color_mode = "RGBA"
    scn.render.film_transparent = False
    scn.render.resolution_percentage = 100
    scn.render.use_stamp = False
    for attr in dir(scn.render):
        if attr.startswith("use_stamp_"):
            try:
                setattr(scn.render, attr, False)
            except (AttributeError, TypeError):
                pass
    scn.render.stamp_font_size = 13
    scn.render.stamp_background = (0.0, 0.0, 0.0, 0.55)
    cam = bpy.data.objects.new("_cam", bpy.data.cameras.new("_cam"))
    scn.collection.objects.link(cam)
    scn.camera = cam
    return scn, cam


def _flat(name, colour):
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*colour, 1.0)
    return mat


def _ground(size):
    img = bpy.data.images.new("_checker", 2, 2)
    a, b = (0.60, 0.62, 0.58, 1.0), (0.68, 0.70, 0.66, 1.0)
    img.pixels.foreach_set(np.array([a, b, b, a], np.float32).ravel())
    mesh = bpy.data.meshes.new("_ground")
    h = size / 2
    mesh.from_pydata([(-h, -h, -0.003), (h, -h, -0.003), (h, h, -0.003), (-h, h, -0.003)], [], [(0, 1, 2, 3)])
    uv = mesh.uv_layers.new()
    for i, (u, v) in enumerate(((0, 0), (1, 0), (1, 1), (0, 1))):
        uv.data[i].uv = (u * size / 2, v * size / 2)  # one checker cell = 1 m
    mat = bpy.data.materials.new("_ground")
    mat.use_nodes = True
    tex = mat.node_tree.nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.interpolation = "Closest"
    mat.node_tree.nodes.active = tex
    mesh.materials.append(mat)
    obj = bpy.data.objects.new("_ground", mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def _import(path, name):
    """Import a GLB under an empty named `name`; returns (root, meshes)."""
    before = set(bpy.data.objects)
    with quiet():
        bpy.ops.import_scene.gltf(filepath=path, import_shading="NORMALS")
    new = [o for o in bpy.data.objects if o not in before]
    root = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(root)
    for o in new:
        if o.parent is None:
            o.parent = root
    for o in new:
        for slot in o.material_slots:
            m = slot.material
            if m is None or not m.use_nodes:
                continue
            bsdf = next((n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
            if bsdf is not None and not bsdf.inputs["Base Color"].links:
                m.diffuse_color = tuple(bsdf.inputs["Base Color"].default_value)
    return root, [o for o in new if o.type == "MESH"]


def _tint(meshes, colour, suffix):
    """Give these meshes their own copy of the Team material, coloured."""
    for o in meshes:
        for slot in o.material_slots:
            if slot.material is not None and slot.material.name.startswith("Team"):
                m = slot.material.copy()
                m.name = "Team_" + suffix
                m.diffuse_color = (*colour, 1.0)
                slot.link = "OBJECT"
                slot.material = m


def _box_wire(box, thickness):
    w, h, d = box
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0, 0, h / 2))
    cube = bpy.context.active_object
    cube.scale = (w, d, h)
    bpy.ops.object.transform_apply(scale=True)
    mod = cube.modifiers.new("wire", "WIREFRAME")
    mod.thickness = thickness
    mod.use_replace = True
    cube.data.materials.append(_flat("_box", BOX_COLOUR))
    cube.name = "_box"
    return cube


def _world_bbox(objs):
    lo, hi = np.full(3, np.inf), np.full(3, -np.inf)
    dg = bpy.context.evaluated_depsgraph_get()
    for o in objs:
        ev = o.evaluated_get(dg)
        n = len(ev.data.vertices)
        if not n:
            continue
        co = np.empty(n * 3, np.float32)
        ev.data.vertices.foreach_get("co", co)
        m = np.array(ev.matrix_world)
        w = co.reshape(n, 3) @ m[:3, :3].T + m[:3, 3]
        lo, hi = np.minimum(lo, w.min(0)), np.maximum(hi, w.max(0))
    return Vector(lo), Vector(hi)


def _aim(cam, direction, lo, hi, w_px, h_px, mode, mpp=None):
    """Point the camera along `direction` at the box lo..hi. mode: ORTHO
    (fit), PERSP (fit, 50 mm), GAME (the game's FOV at a fixed
    metres-per-pixel `mpp`)."""
    d = direction.normalized()
    rot = d.to_track_quat("-Z", "Y").to_matrix()
    right, up = rot.col[0], rot.col[1]
    centre = (lo + hi) / 2
    corners = [Vector((x, y, z)) for x in (lo.x, hi.x) for y in (lo.y, hi.y) for z in (lo.z, hi.z)]
    xs = [(c - centre).dot(right) for c in corners]
    ys = [(c - centre).dot(up) for c in corners]
    mid = centre + right * (max(xs) + min(xs)) / 2 + up * (max(ys) + min(ys)) / 2
    need = max(max(ys) - min(ys), (max(xs) - min(xs)) * h_px / w_px) * 1.12
    data = cam.data
    data.sensor_fit = "VERTICAL"
    data.clip_start, data.clip_end = 0.05, 500.0
    if mode == "ORTHO":
        data.type = "ORTHO"
        data.ortho_scale = need * max(1.0, w_px / h_px)
        cam.location = mid - d * 50.0
    elif mode == "PERSP":
        data.type = "PERSP"
        data.lens = 50.0
        fov = 2 * math.atan(data.sensor_height / 2 / data.lens)
        cam.location = mid - d * (need / 2 / math.tan(fov / 2) + (hi - lo).length / 2)
    else:
        data.type = "PERSP"
        dist = GAME_DISTANCE
        data.lens = data.sensor_height / 2 / (h_px * mpp / 2 / dist)
        cam.location = mid - d * dist
    cam.rotation_mode = "QUATERNION"
    cam.rotation_quaternion = rot.to_quaternion()


def _render(scn, path, w, h, label=None):
    r = scn.render
    r.resolution_x, r.resolution_y = w, h
    r.use_stamp = bool(label)
    r.use_stamp_note = bool(label)
    r.stamp_note_text = label or ""
    r.filepath = path
    with quiet():
        bpy.ops.render.render(write_still=True)
    img = bpy.data.images.load(path)
    px = np.array(img.pixels[:], np.float32).reshape(img.size[1], img.size[0], 4)
    bpy.data.images.remove(img)
    os.remove(path)
    return px


def _enlarge(px, k):
    return np.repeat(np.repeat(px, k, axis=0), k, axis=1) if k > 1 else px


def _fit_scale(fit, size, box):
    from .build import fit_scale
    return fit_scale(fit, size, box)


def render_asset(asset, s, entry, parts, out_png):
    """parts: [(name, glb path, built dict with min/max/triangles)]."""
    scn, cam = _setup_scene()
    variants = [p for p in parts if p[0] != "stump"]
    first = variants[0][2]
    first_size = [b - a for a, b in zip(first["min"], first["max"])]
    box = entry.size if entry and entry.size else tuple(first_size)
    tree_scale = _fit_scale(s.fit, first_size, box)

    roots = []
    for name, path, info in parts:
        size = [b - a for a, b in zip(info["min"], info["max"])]
        gx, gy, gz = tree_scale if name == "stump" else _fit_scale(s.fit, size, box)
        root, meshes = _import(path, f"_part_{name}")
        root.scale = (gx, gz, gy)  # glTF (x, y, z) -> Blender (x, z, y)
        _tint(meshes, TEAM_COLOURS[1], "blue")
        roots.append((name, info, root, meshes))
    _ground(400.0)
    wire = _box_wire(box, 0.012 * max(box))

    scratch = os.path.join(os.path.dirname(out_png), "_tiles")
    os.makedirs(scratch, exist_ok=True)
    every = [o for _, _, _, ms in roots for o in ms]
    lo, hi = _world_bbox(every + [wire])
    lo.z = 0.0
    columns = []
    for name, info, root, meshes in roots:
        for _, _, other, ms in roots:
            for o in ms:
                o.hide_render = other is not root
        tiles = []
        tris = info["triangles"]
        size = [b - a for a, b in zip(info["min"], info["max"])]
        label = f"{asset.id} {name}: {tris} tris, {size[0]:.2f}x{size[1]:.2f}x{size[2]:.2f} m"
        for v in VIEWS:
            wire.hide_render = v == "rts"
            if v == "front":
                _aim(cam, Vector((0, 1, -0.14)), lo, hi, TILE_W, TILE_H, "ORTHO")
            elif v == "side":
                _aim(cam, Vector((-1, 0, -0.14)), lo, hi, TILE_W, TILE_H, "ORTHO")
            elif v == "tq":
                _aim(cam, Vector((0.62, 0.62, -0.48)), lo, hi, TILE_W, TILE_H, "PERSP")
            path = os.path.join(scratch, f"{asset.id}_{name}_{v}.png")
            if v == "rts":
                k, rw, rh = _game_view(cam, lo, hi, TILE_W, TILE_H, (1, 0.75, 0.5, 0.35))
                px = _render(scn, path, rw, rh, None if k == 1 else f"zoomed out to {k:.0%}")
            else:
                px = _render(scn, path, TILE_W, TILE_H, label if v == "front" else None)
            tiles.append(px)
        columns.append(tiles)
    wire.hide_render = True
    for _, _, _, ms in roots:
        for o in ms:
            o.hide_render = True

    ncols = max(MIN_COLUMNS, len(columns))
    group_w, group_h = ncols * TILE_W, 2 * TILE_H
    group = _group(asset, s, roots, box, scn, cam, scratch, group_w, group_h)

    sheet = np.ones((len(VIEWS) * TILE_H + group_h, ncols * TILE_W, 4), np.float32)
    sheet[..., :3] = 0.9
    for c, tiles in enumerate(columns):
        for r, px in enumerate(tiles):
            y0 = group_h + (len(VIEWS) - 1 - r) * TILE_H  # Blender images are bottom-up
            sheet[y0:y0 + TILE_H, c * TILE_W:(c + 1) * TILE_W] = px[:TILE_H, :TILE_W]
    sheet[:group_h, :group.shape[1]] = group[:group_h]
    sheet[:, ::TILE_W, :3] = 0.25
    sheet[group_h::TILE_H, :, :3] = 0.25
    img = bpy.data.images.new("_sheet", sheet.shape[1], sheet.shape[0], alpha=True)
    img.pixels.foreach_set(sheet.ravel())
    os.makedirs(os.path.dirname(out_png), exist_ok=True)
    img.filepath_raw = out_png
    img.file_format = "PNG"
    img.save()
    bpy.data.images.remove(img)
    try:
        os.rmdir(scratch)
    except OSError:
        pass


def _group(asset, s, roots, box, scn, cam, scratch, w, h):
    """The "in game" strip: a patch of nodes or three buildings under construction."""
    rng = random.Random(asset.id)
    copies = []
    variants = [r for r in roots if r[0] != "stump"]
    if asset.kind == "nodes":
        step = max(box[0], box[2]) * 2.0 + (1.6 if box[1] > 3 else 0.6)
        order = [i % len(variants) for i in range(9)]  # every variant shows, evenly mixed
        rng.shuffle(order)
        n = 0
        for gy in range(3):
            for gx in range(3):
                root = variants[order[n]][2]
                n += 1
                yaw = rng.uniform(0, 360) if s.random_yaw else 0.0
                at = Vector(((gx - 1) * step + rng.uniform(-0.25, 0.25) * step,
                             (gy - 1) * step + rng.uniform(-0.25, 0.25) * step, 0.0))
                copies.append(_instance(root, at, yaw, 0.0))
        label = f"in game: nine {asset.id}, variants mixed" + (", turned at random" if s.random_yaw else "")
    elif asset.kind == "decor":
        # Every variant once, side by side at its drawn size (the game
        # scales each placement further: mountains to the range, rims to
        # the crater, bridges to the river).
        step = box[0] * 1.15 + 0.6
        for i, (_, _, root, _) in enumerate(variants):
            at = Vector(((i - (len(variants) - 1) / 2) * step, (i % 2) * box[2] * 0.3, 0.0))
            copies.append(_instance(root, at, 0.0, 0.0))
        label = f"in game: {asset.id}, every variant at its drawn size"
    else:
        root = variants[0][2]
        height = box[1]
        step = max(box[0], box[2]) * 1.35
        for i, (progress, colour) in enumerate(zip((0.4, 0.75, 1.0), TEAM_COLOURS)):
            at = Vector(((i - 1) * step, (i - 1) * step * -0.15, 0.0))
            copies.append(_instance(root, at, 0.0, -(1.0 - progress) * height, colour, f"c{i}"))
        label = f"in game: {asset.id} at 40 %, 75 %, 100 % built"
    objs = [o for c in copies for o in c]
    lo, hi = _world_bbox(objs)
    lo.z = 0.0
    k, rw, rh = _game_view(cam, lo, hi, w, h, (3, 2, 1, 0.75, 0.5, 0.35, 0.25))
    scale = f"{k}x" if k >= 1 else f"zoomed out to {k:.0%}"
    scn.render.stamp_font_size = max(8, int(13 // max(k, 1)))
    px = _render(scn, os.path.join(scratch, f"{asset.id}_group.png"), rw, rh, f"{label} ({scale})")
    scn.render.stamp_font_size = 13
    for o in objs:
        bpy.data.objects.remove(o, do_unlink=True)
    out = np.ones((h, w, 4), np.float32)
    big = _enlarge(px, int(k)) if k >= 1 else px
    out[:big.shape[0], :big.shape[1]] = big[:h, :w]
    return out


def _game_view(cam, lo, hi, w, h, scales):
    """Aim the game camera at lo..hi for a w x h tile at the first of
    `scales` that fits: k >= 1 renders w/k x h/k at the game's pixel scale
    (to be enlarged k times), k < 1 renders w x h zoomed out to k.
    Returns (k, render width, render height)."""
    for k in scales:
        rw, rh = (w // k, h // k) if k >= 1 else (w, h)
        _aim(cam, _game_dir(), lo, hi, rw, rh, "GAME", GAME_MPP if k >= 1 else GAME_MPP / k)
        if _fits(cam, lo, hi, rw, rh):
            break
    return k, int(rw), int(rh)


def _fits(cam, lo, hi, w, h):
    from bpy_extras.object_utils import world_to_camera_view
    bpy.context.view_layer.update()
    scn = bpy.context.scene
    scn.render.resolution_x, scn.render.resolution_y = w, h
    for x in (lo.x, hi.x):
        for y in (lo.y, hi.y):
            for z in (lo.z, hi.z):
                p = world_to_camera_view(scn, cam, Vector((x, y, z)))
                if not (0.02 <= p.x <= 0.98 and 0.02 <= p.y <= 0.9):
                    return False
    return True


def _instance(root, at, yaw_deg, sink, colour=None, suffix=""):
    out = []
    place = Matrix.Translation(at + Vector((0, 0, sink))) @ Matrix.Rotation(math.radians(yaw_deg), 4, "Z")
    for child in root.children:
        if child.type != "MESH":
            continue
        o = child.copy()
        o.parent = None
        o.matrix_world = place @ child.matrix_world
        o.hide_render = False
        bpy.context.scene.collection.objects.link(o)
        if colour is not None:
            _tint([o], colour, suffix)
        out.append(o)
    return out
