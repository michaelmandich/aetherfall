"""Contact sheets: every clip from fixed cameras at several times, one PNG.

Rows: front (orthographic), right side (orthographic), three-quarter
(perspective, front-right, from above) and "rts": the in-game view (pitch
54 deg, the game's pixel scale at 40 m zoom for a 1.2 m unit), rendered at
that tiny size and enlarged 4x so it shows what a player actually sees.
Columns: the clip's SHEET_TIMES, or evenly spaced times (one-shot clips
also get their last frame and HIT time). Workbench engine, textured,
with shadows on a 25 cm checker ground so sliding feet show.
"""

import math
import os

import bpy
import numpy as np
from mathutils import Vector

from .clip import assign
from .source import FPS

TILE_W, TILE_H = 256, 336
RTS_SCALE = 4
COLUMNS = 8
VIEWS = ("front", "side", "tq", "rts")

# Game camera (crates/af_input/src/camera.rs): FOV 32 deg on the viewport
# height, pitch 0.95 rad. A 1.2 m unit seen from 40 m on a 1080 px screen.
GAME_PITCH = 0.95
GAME_MPP = 2.0 * 40.0 * math.tan(math.radians(16.0)) / 1080.0


def _checker_image(size=512, squares=16):
    img = bpy.data.images.new("_checker", size, size)
    idx = np.indices((size, size)) // (size // squares)
    on = ((idx[0] + idx[1]) % 2).astype(np.float32)
    px = np.empty((size, size, 4), np.float32)
    shade = 0.62 + 0.10 * on
    px[..., 0] = shade
    px[..., 1] = shade
    px[..., 2] = shade * 0.98
    px[..., 3] = 1.0
    img.pixels.foreach_set(px.ravel())
    return img


class SheetRig:
    """Ground, light and cameras for contact sheets (built once per run)."""

    def __init__(self, scene):
        self.scene = scene
        scene.render.engine = "BLENDER_WORKBENCH"
        scene.view_settings.view_transform = "Standard"
        scene.view_settings.look = "None"
        sh = scene.display.shading
        sh.light = "STUDIO"
        sh.color_type = "TEXTURE"
        sh.show_shadows = True
        sh.shadow_intensity = 0.45
        sh.show_cavity = True
        sh.cavity_type = "WORLD"
        scene.display.light_direction = (0.45, -0.35, 0.82)
        scene.render.film_transparent = False
        if scene.world is None:
            scene.world = bpy.data.worlds.new("SheetWorld")
        scene.world.color = (0.80, 0.82, 0.85)
        scene.render.image_settings.file_format = "PNG"
        scene.render.image_settings.color_mode = "RGBA"
        r = scene.render
        r.use_stamp = False
        for attr in dir(r):
            if attr.startswith("use_stamp_"):
                try:
                    setattr(r, attr, False)
                except (AttributeError, TypeError):
                    pass
        r.stamp_font_size = 14
        r.stamp_background = (0.0, 0.0, 0.0, 0.6)

        bpy.ops.mesh.primitive_plane_add(size=8.0, location=(0.0, 0.0, -0.002))
        self.ground = bpy.context.active_object
        self.ground.name = "_sheet_ground"
        mat = bpy.data.materials.new("_sheet_ground")
        mat.use_nodes = True
        tex = mat.node_tree.nodes.new("ShaderNodeTexImage")
        tex.image = _checker_image()
        tex.interpolation = "Closest"
        bsdf = mat.node_tree.nodes["Principled BSDF"]
        mat.node_tree.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
        self.ground.data.materials.append(mat)
        self.cams = {}
        for v in VIEWS:
            cam = bpy.data.cameras.new("_cam_" + v)
            obj = bpy.data.objects.new("_cam_" + v, cam)
            scene.collection.objects.link(obj)
            self.cams[v] = obj

    def frame_cameras(self, lo, hi):
        """Aim the cameras at a world bounding box (fixed for a whole clip)."""
        center = (lo + hi) / 2
        corners = [Vector((x, y, z)) for x in (lo.x, hi.x) for y in (lo.y, hi.y) for z in (lo.z, hi.z)]
        aspect = TILE_W / TILE_H
        for v, obj in self.cams.items():
            cam = obj.data
            if v == "front":
                d = Vector((0.0, 1.0, -0.14))
            elif v == "side":
                d = Vector((1.0, 0.0, -0.14))
            elif v == "tq":
                d = Vector((0.62, 0.62, -0.48))
            else:
                yaw = math.radians(30.0)
                d = Vector((-math.sin(yaw) * math.cos(GAME_PITCH), math.cos(yaw) * math.cos(GAME_PITCH),
                            -math.sin(GAME_PITCH)))
            d.normalize()
            rot = d.to_track_quat("-Z", "Y").to_matrix()
            right, up = rot.col[0], rot.col[1]
            xs = [(c - center).dot(right) for c in corners]
            ys = [(c - center).dot(up) for c in corners]
            w_need, h_need = max(xs) - min(xs), max(ys) - min(ys)
            mid = center + right * (max(xs) + min(xs)) / 2 + up * (max(ys) + min(ys)) / 2
            if v in ("front", "side"):
                cam.type = "ORTHO"
                cam.ortho_scale = max(h_need, w_need / aspect) * 1.12
                obj.location = mid - d * 10.0
            elif v == "tq":
                cam.type = "PERSP"
                cam.sensor_fit = "VERTICAL"
                cam.lens = 50.0
                fov = 2 * math.atan(cam.sensor_height / 2 / cam.lens)
                size = max(h_need, w_need / aspect) * 1.12
                obj.location = mid - d * (size / 2 / math.tan(fov / 2))
            else:
                cam.type = "PERSP"
                cam.sensor_fit = "VERTICAL"
                dist = 53.0
                height = (TILE_H / RTS_SCALE) * GAME_MPP * (1.59 / 1.2)
                cam.lens = cam.sensor_height / 2 / (height / 2 / dist)
                obj.location = center - d * dist
            cam.clip_start = 0.05
            cam.clip_end = 200.0
            obj.rotation_mode = "QUATERNION"
            obj.rotation_quaternion = rot.to_quaternion()


def sheet_times(clip):
    if clip.sheet_times:
        return list(clip.sheet_times)
    if clip.loop:
        return [clip.duration * i / COLUMNS for i in range(COLUMNS)]
    times = [clip.duration * i / (COLUMNS - 1) for i in range(COLUMNS)]
    if clip.hit is not None and all(abs(t - clip.hit) > 1e-6 for t in times):
        nearest = min(range(1, COLUMNS - 1), key=lambda i: abs(times[i] - clip.hit))
        times[nearest] = clip.hit
    return times


def _set_time(scene, t):
    f = t * FPS
    fi = int(math.floor(f + 1e-6))
    scene.frame_set(fi, subframe=max(0.0, f - fi))


def _bbox(objs, depsgraph):
    lo = Vector((1e9, 1e9, 1e9))
    hi = -lo
    for obj in objs:
        ev = obj.evaluated_get(depsgraph)
        if obj.type == "MESH" and ev.data.vertices:
            n = len(ev.data.vertices)
            co = np.empty(n * 3, np.float32)
            ev.data.vertices.foreach_get("co", co)
            co = co.reshape(n, 3)
            m = np.array(ev.matrix_world)
            w = co @ m[:3, :3].T + m[:3, 3]
            lo = Vector(np.minimum(np.array(lo), w.min(0)))
            hi = Vector(np.maximum(np.array(hi), w.max(0)))
    return lo, hi


def render_sheet(rig_obj, body, props, clip, out_png, scratch, sheet_rig):
    """Render one clip's contact sheet to `out_png`."""
    # SHEET_PROPS (optional) cycles props across the columns instead of
    # PROPS, e.g. every load in a carry clip (they share one grip).
    cycle = list(getattr(clip.module, "SHEET_PROPS", ()) or ())
    columns = []
    for i, t in enumerate(sheet_times(clip)):
        shown = {cycle[i % len(cycle)]} if cycle else set(clip.props)
        columns.append((clip.action, t, shown, f"{clip.name} {t:.2f}s"))
    render_columns(rig_obj, body, props, columns, out_png, scratch, sheet_rig)


def render_columns(rig_obj, body, props, columns, out_png, scratch, sheet_rig, bbox=None):
    """Render columns of (action, time, prop names shown, label) into one sheet.
    The cameras are framed once on everything the columns show (or `bbox`)."""
    scene = sheet_rig.scene

    def show(names):
        for name, obj in props.items():
            obj.hide_render = name not in names

    def pose(action, t):
        assign(rig_obj, action)
        _set_time(scene, t)

    lo, hi = Vector((1e9,) * 3), Vector((-1e9,) * 3)
    for action, t, shown, _ in columns:
        pose(action, t)
        a, b = _bbox([body] + [props[n] for n in shown], bpy.context.evaluated_depsgraph_get())
        lo = Vector(np.minimum(np.array(lo), np.array(a)))
        hi = Vector(np.maximum(np.array(hi), np.array(b)))
    lo.z = min(lo.z, 0.0)
    if bbox is not None:
        lo, hi = bbox
    sheet_rig.frame_cameras(lo, hi)

    os.makedirs(scratch, exist_ok=True)
    tiles = {v: [] for v in VIEWS}
    stem = os.path.splitext(os.path.basename(out_png))[0]
    for i, (action, t, shown, label) in enumerate(columns):
        pose(action, t)
        show(shown)
        for v in VIEWS:
            scene.camera = sheet_rig.cams[v]
            r = scene.render
            if v == "rts":
                r.resolution_x, r.resolution_y = TILE_W // RTS_SCALE, TILE_H // RTS_SCALE
                r.use_stamp = False
            else:
                r.resolution_x, r.resolution_y = TILE_W, TILE_H
                r.use_stamp = v == "front"
                r.use_stamp_note = True
                r.stamp_note_text = label
            r.resolution_percentage = 100
            path = os.path.join(scratch, f"{stem}_{i}_{v}.png")
            r.filepath = path
            bpy.ops.render.render(write_still=True)
            tiles[v].append(path)
    show(set())
    _tile(tiles, out_png)
    for paths in tiles.values():
        for p in paths:
            try:
                os.remove(p)
            except OSError:
                pass


def _load_px(path, scale=1):
    img = bpy.data.images.load(path)
    w, h = img.size
    px = np.array(img.pixels[:], np.float32).reshape(h, w, 4)
    bpy.data.images.remove(img)
    if scale > 1:
        px = np.repeat(np.repeat(px, scale, axis=0), scale, axis=1)
    return px


def _tile(tiles, out_png):
    rows = [v for v in VIEWS if tiles[v]]
    cols = len(tiles[rows[0]])
    sheet = np.ones((len(rows) * TILE_H, cols * TILE_W, 4), np.float32)
    for r_i, v in enumerate(rows):
        for c_i, path in enumerate(tiles[v]):
            px = _load_px(path, RTS_SCALE if v == "rts" else 1)
            px = px[:TILE_H, :TILE_W]
            y0 = (len(rows) - 1 - r_i) * TILE_H
            sheet[y0:y0 + px.shape[0], c_i * TILE_W:c_i * TILE_W + px.shape[1]] = px
    # Thin separators between tiles.
    sheet[:, ::TILE_W, :3] = 0.25
    sheet[::TILE_H, :, :3] = 0.25
    img = bpy.data.images.new("_sheet", sheet.shape[1], sheet.shape[0], alpha=True)
    img.pixels.foreach_set(sheet.ravel())
    img.filepath_raw = out_png
    img.file_format = "PNG"
    img.save()
    bpy.data.images.remove(img)
