"""Self-test of the static pipeline: synthetic sources with every awkward
feature, built for real, and the outputs checked.

    "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b \
        -P art/static/build_static.py -- --selftest

Sources (written to a temporary folder, with their own assets.toml):

    nodes/oak_a.glb      a skinned, animated column whose armature is posed
                         and animated away from rest; an emissive material
                         with a 2048 px base colour whose alpha cuts half
                         of it away (blended); a duplicate of that material
                         on a second mesh; 11,640 triangles (flat shaded)
    nodes/oak_stump.obj  an OBJ stump
    nodes/mystery.glb    an id the game does not have
    nodes/pine_a.glb     a column on a wide, thin ground plate, both off the
                         origin (assets.toml: cut_plate = { a = 0.02 })
    nodes/pine_b.glb     the same, untouched (the setting names variant a)
    nodes/palm.glb       40 tufts of blades sharing their stems: hundreds of
                         non-manifold edges, where plain collapse stalls
                         above its budget (assets.toml: triangles = 1500),
                         as on Meshy's palms
    buildings/house.fbx  an FBX house facing -Z with a chimney at its front
                         (assets.toml: yaw = 180), default banner
    buildings/farm.glb   banner at a given anchor
    buildings/mill.glb   banner = false
    decor/reeds_a.glb    decor with a drawn size from assets.toml
                         (size = [1.0, 2.0, 1.0])
    decor/mountain_ridge.glb  a long piece lying along Z: turned to lie along X
    decor/odd_rock.glb   an id the game does not place

Every check that fails is listed; the run fails if any does.
"""

import math
import os
import shutil
import tempfile

import bmesh
import bpy
import numpy as np

from . import content, gltf, materials, scene, settings
from .util import quiet


def _mesh_obj(name, bm):
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def _column(name, height=2.0, size=0.4, cuts=60):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bmesh.ops.scale(bm, vec=(size, size, height), verts=bm.verts)
    bmesh.ops.translate(bm, vec=(0, 0, height / 2), verts=bm.verts)
    bmesh.ops.subdivide_edges(bm, edges=bm.edges, cuts=cuts // 2, use_grid_fill=True)
    bmesh.ops.triangulate(bm, faces=bm.faces)
    bm.verts.ensure_lookup_table()
    uv = bm.loops.layers.uv.new("UVMap")
    for f in bm.faces:
        for loop in f.loops:
            loop[uv].uv = (loop.vert.co.x / size + 0.5, loop.vert.co.z / height)
    for v in bm.verts:  # a rounded, uneven trunk, so decimation moves its extremes
        r = 1.0 + 0.15 * math.sin(7.0 * v.co.z) * math.cos(3.0 * math.atan2(v.co.y, v.co.x))
        v.co.x, v.co.y = v.co.x * r, v.co.y * r
    return _mesh_obj(name, bm)


def _leaf_material(name):
    """Emissive, metallic, with a 2048 px texture whose alpha is 0 on the left half (blended)."""
    img = bpy.data.images.new(name + "_tex", 2048, 2048, alpha=True)
    px = np.ones((2048, 2048, 4), np.float32)
    px[..., 0], px[..., 1], px[..., 2] = 0.2, 0.6, 0.2
    px[:, :1024, 3] = 0.0
    img.pixels.foreach_set(px.ravel())
    img.pack()
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(tex.outputs["Alpha"], bsdf.inputs["Alpha"])
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Emission Color"])
    bsdf.inputs["Emission Strength"].default_value = 2.0
    bsdf.inputs["Metallic"].default_value = 1.0
    return mat


def _rigged_oak(path):
    scene.reset()
    col = _column("trunk")
    leaf = _leaf_material("Leaves")
    col.data.materials.append(leaf)
    twin = _column("twin", height=0.5, size=0.3, cuts=4)
    twin.location = (0.6, 0, 0)
    twin.data.materials.append(leaf.copy())  # "Leaves.001": same look, other name
    arm_data = bpy.data.armatures.new("rig")
    arm = bpy.data.objects.new("rig", arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="EDIT")
    lower = arm_data.edit_bones.new("lower")
    lower.head, lower.tail = (0, 0, 0), (0, 0, 1)
    upper = arm_data.edit_bones.new("upper")
    upper.head, upper.tail = (0, 0, 1), (0, 0, 2)
    upper.parent = lower
    bpy.ops.object.mode_set(mode="OBJECT")
    for obj in (col, twin):
        obj.parent = arm
        mod = obj.modifiers.new("rig", "ARMATURE")
        mod.object = arm
        g_lo, g_hi = obj.vertex_groups.new(name="lower"), obj.vertex_groups.new(name="upper")
        for v in obj.data.vertices:
            (g_hi if v.co.z > 1.0 else g_lo).add([v.index], 1.0, "REPLACE")
    pb = arm.pose.bones["upper"]
    pb.rotation_mode = "XYZ"
    arm.animation_data_create()
    arm.animation_data.action = bpy.data.actions.new("Sway")
    for frame, angle in ((1, 0.0), (20, 90.0)):
        pb.rotation_euler = (math.radians(angle), 0, 0)
        pb.keyframe_insert("rotation_euler", frame=frame)
    bpy.context.scene.frame_set(20)  # posed (and animated) away from rest
    _export_all(path, skins=True)


def _export_all(path, skins=False):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    ext = os.path.splitext(path)[1]
    for o in bpy.context.scene.objects:
        o.select_set(True)
    with quiet():
        if ext == ".glb":
            bpy.ops.export_scene.gltf(filepath=path, export_format="GLB", export_skins=skins,
                                      export_animations=skins, export_apply=False)
        elif ext == ".fbx":
            bpy.ops.export_scene.fbx(filepath=path, use_selection=False, axis_forward="-Z", axis_up="Y")
        else:
            bpy.ops.wm.obj_export(filepath=path, forward_axis="NEGATIVE_Z", up_axis="Y", export_materials=False)


def _tufts(path, n=40, pages=6, cuts=8, height=0.6, width=0.25):
    """`n` tufts whose blades share one stem: every stem edge has `pages`
    faces, so collapse alone stalls at about 1900 of 30720 triangles."""
    scene.reset()
    bm = bmesh.new()
    for k in range(n):
        ox, oy = (k % 8) * 0.7, (k // 8) * 0.7
        stem = [bm.verts.new((ox, oy, height * i / cuts)) for i in range(cuts + 1)]
        for p in range(pages):
            a = 2 * math.pi * p / pages + 0.3 * k
            cols = [stem]
            for j in range(1, cuts + 1):
                r = width * j / cuts
                cols.append([bm.verts.new((ox + r * math.cos(a), oy + r * math.sin(a),
                                           height * i / cuts + 0.05 * math.sin(9 * r))) for i in range(cuts + 1)])
            for j in range(cuts):
                for i in range(cuts):
                    bm.faces.new((cols[j][i], cols[j + 1][i], cols[j + 1][i + 1], cols[j][i + 1]))
    bmesh.ops.triangulate(bm, faces=bm.faces)
    obj = _mesh_obj("palm", bm)
    obj.data.materials.append(materials.flat_material("Frond", (0.3, 0.6, 0.2)))
    _export_all(path)


def _simple(path, parts):
    """`parts`: [(size xyz, centre xyz)] boxes in glTF axes (y up, +Z front)."""
    scene.reset()
    bm = bmesh.new()
    for (sx, sy, sz), (cx, cy, cz) in parts:
        geom = bmesh.ops.create_cube(bm, size=1.0)
        verts = geom["verts"]
        bmesh.ops.scale(bm, vec=(sx, sz, sy), verts=verts)        # glTF -> Blender axes
        bmesh.ops.translate(bm, vec=(cx, -cz, cy), verts=verts)
    obj = _mesh_obj(os.path.splitext(os.path.basename(path))[0], bm)
    obj.data.materials.append(materials.flat_material("Wall", (0.8, 0.7, 0.5)))
    _export_all(path)


def _pixels_of(path):
    """(h, w, 4) float pixels of a PNG file, as stored (sRGB values)."""
    img = bpy.data.images.load(path)
    w, h = img.size
    px = np.empty(w * h * 4, np.float32)
    img.pixels.foreach_get(px)
    bpy.data.images.remove(img)
    return px.reshape(h, w, 4)


def _positions(path, node):
    js, blob = gltf.read(path)
    n = next(n for n in js["nodes"] if n.get("name") == node)
    out = []
    for prim in js["meshes"][n["mesh"]]["primitives"]:
        acc = js["accessors"][prim["attributes"]["POSITION"]]
        view = js["bufferViews"][acc["bufferView"]]
        start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
        out.append(np.frombuffer(blob, np.float32, acc["count"] * 3, start).reshape(-1, 3))
    return np.concatenate(out)


def _opacity_map_check(check):
    """An FBX-style material: colour from one image, alpha from another
    image's colour output. Cleaned, it must read its cutout from one RGBA
    base colour texture."""
    scene.reset()
    mat = bpy.data.materials.new("Opacity")
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    colour, opacity = bpy.data.images.new("colour", 64, 64), bpy.data.images.new("opacity", 32, 32)
    colour.pixels.foreach_set(np.tile(np.array([0.8, 0.1, 0.1, 1.0], np.float32), 64 * 64))
    mask = np.ones((32, 32, 4), np.float32)
    mask[:, :16, :3] = 0.0  # left half transparent
    opacity.pixels.foreach_set(mask.ravel())
    for img, socket, out in ((colour, "Base Color", "Color"), (opacity, "Alpha", "Color")):
        node = nt.nodes.new("ShaderNodeTexImage")
        node.image = img
        nt.links.new(node.outputs[out], bsdf.inputs[socket])
    notes = []
    base = materials.clean(mat, notes)
    link = bsdf.inputs["Alpha"].links[0] if bsdf.inputs["Alpha"].links else None
    fed = link is not None and link.from_node.type == "MATH" and link.from_node.inputs[0].links         and link.from_node.inputs[0].links[0].from_node == base         and link.from_node.inputs[0].links[0].from_socket.name == "Alpha"
    check(materials.uses_cutout(mat) and fed, "a separate opacity map: the cutout reads the base colour texture's alpha")
    px = np.empty(64 * 64 * 4, np.float32)
    base.image.pixels.foreach_get(px)
    px = px.reshape(64, 64, 4)
    check(px[:, :32, 3].max() == 0.0 and px[:, 32:, 3].min() == 1.0 and abs(px[0, 40, 0] - 0.8) < 0.01,
          "a separate opacity map: copied into the base colour texture's alpha, colour kept")


def run(build_run, parse_args):
    """Write the sources, build them with `build_run(parse_args(argv))`, check."""
    tmp = tempfile.mkdtemp(prefix="af_static_selftest_")
    problems = []

    def check(ok, what):
        print(f"  {'ok  ' if ok else 'FAIL'} {what}")
        if not ok:
            problems.append(what)

    try:
        src, out = os.path.join(tmp, "source"), os.path.join(tmp, "assets")
        _rigged_oak(os.path.join(src, "nodes", "oak_a.glb"))
        scene.reset()
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=10, radius1=0.3, radius2=0.28, depth=0.5)
        stray = bm.verts.new((3.0, 0.0, 0.5))  # loose geometry (OBJ keeps it; glTF would not)
        bm.edges.new((stray, bm.verts.new((3.0, 0.2, 0.5))))
        _mesh_obj("stump", bm)
        _export_all(os.path.join(src, "nodes", "oak_stump.obj"))
        _simple(os.path.join(src, "nodes", "mystery.glb"), [((1, 1, 1), (0, 0.5, 0))])
        _tufts(os.path.join(src, "nodes", "palm.glb"))
        # A column standing on a plate, both off the origin: the cut must
        # take the plate of variant a only, and a must be based and centred
        # again afterwards.
        plated = [((0.4, 2.0, 0.4), (0.3, 1.02, -0.2)), ((2.0, 0.02, 2.0), (0, 0.01, 0))]
        _simple(os.path.join(src, "nodes", "pine_a.glb"), plated)
        _simple(os.path.join(src, "nodes", "pine_b.glb"), plated)
        # House facing -Z: its chimney is at the front, so at -Z in the source.
        _simple(os.path.join(src, "buildings", "house.fbx"),
                [((2, 2, 2), (0, 1, 0)), ((0.3, 1.0, 0.3), (0, 2.5, -0.7))])
        _simple(os.path.join(src, "buildings", "farm.glb"), [((6, 0.3, 6), (0, 0.15, 0))])
        _simple(os.path.join(src, "buildings", "mill.glb"), [((3, 3, 3), (0, 1.5, 0))])
        _simple(os.path.join(src, "decor", "reeds_a.glb"), [((0.5, 1.5, 0.5), (0, 0.75, 0))])
        _simple(os.path.join(src, "decor", "mountain_ridge.glb"), [((2, 1, 8), (0, 0.5, 0)), ((1, 2, 1), (0, 1, 3))])
        _simple(os.path.join(src, "decor", "odd_rock.glb"), [((1, 1, 1), (0, 0.5, 0))])
        toml = os.path.join(tmp, "assets.toml")
        with open(toml, "w", encoding="utf-8") as f:
            f.write("[house]\nyaw = 180\n\n[farm]\nbanner = [1.0, 0.3, -2.0]\n\n[mill]\nbanner = false\n"
                    "\n[pine]\ncut_plate = { a = 0.02 }\n\n[palm]\ntriangles = 1500\n"
                    "\n[oak]\ntint = { a = [0.5, 1.0, 1.0] }\n\n[reeds]\nsize = [1.0, 2.0, 1.0]\n")

        failed = build_run(parse_args([
            "--source", src, "--out-root", os.path.join(out, "models", "static"), "--asset-root", out,
            "--registry", os.path.join(out, "visuals", "static.ron"), "--settings", toml, "--no-sheets"]))
        print("\nchecks:")
        check(failed == 0, "every asset built")
        reg = content.parse(open(os.path.join(out, "visuals", "static.ron"), encoding="utf-8").read())
        check(reg.get("units") == {} and sorted(reg["buildings"]) == ["farm", "house", "mill"]
              and sorted(reg["nodes"]) == ["mystery", "oak", "palm", "pine"]
              and sorted(reg.get("decor", {})) == ["mountain_ridge", "odd_rock", "reeds"], f"registry lists every asset ({reg})")
        man = {}
        for kind in ("buildings", "nodes", "decor"):
            for ident, rel in reg.get(kind, {}).items():
                m = content.parse(open(os.path.join(out, rel), encoding="utf-8").read())
                man[ident] = m
                for part in m["variants"] + ([m["stump"]] if m["stump"] not in (None, "None") else []):
                    check(os.path.isfile(os.path.join(out, part["model"])), f"{ident}: {part['model']} exists")
                    check(abs(part["min"][1]) < 1e-3, f"{ident}: base on y = 0 ({part['min'][1]})")
                    cx, cz = (part["min"][0] + part["max"][0]) / 2, (part["min"][2] + part["max"][2]) / 2
                    check(abs(cx) < 1e-3 and abs(cz) < 1e-3, f"{ident}: centred on x/z ({cx:.4f}, {cz:.4f})")

        oak = man["oak"]
        check(oak["kind"] == "Node" and oak["fit"] == "Height" and oak["random_yaw"] == "true"
              and oak["banner"] == "false", "oak: tree defaults (Node, Height, random_yaw, no banner)")
        a = oak["variants"][0]
        check(a["triangles"] <= settings.TREE_TRIANGLES,
              f"oak: within the tree budget ({a['triangles']} <= {settings.TREE_TRIANGLES})")
        h = a["max"][1] - a["min"][1]
        check(abs(h - 2.0) < 0.05, f"oak: rest pose, not the animated or posed one (height {h:.3f}, want 2.0)")

        check(oak["stump"] not in (None, "None") and oak["stump"]["triangles"] <= 300, "oak: OBJ stump built")
        w = oak["stump"]["max"][0] - oak["stump"]["min"][0]
        check(w < 0.7, f"oak: the stump's loose vertices dropped, not counted in its bounds (width {w:.2f})")
        glb = os.path.join(out, a["model"])
        s = gltf.summarize(glb)
        check(s["skins"] == 0 and s["animations"] == 0, "oak: no skin, no animation")
        verts = len(_positions(glb, "oak"))
        check(verts < 1.5 * a["triangles"], f"oak: the flat-shaded (split) source was welded and decimated as one "
              f"surface, not shredded ({verts} vertices for {a['triangles']} triangles)")
        check(len(s["materials"]) == 1, f"oak: duplicate materials merged ({[m['name'] for m in s['materials']]})")
        mat = s["materials"][0]
        check(mat["alpha"] == "MASK", f"oak: blended alpha became a cutout ({mat['alpha']})")
        check(not mat["emissive"] and mat["metallic"] == 0, "oak: not emissive, not metallic")
        sizes = [i["size"] for i in s["images"]]
        check(sizes == [(512, 512)], f"oak: texture downsized to 512 px ({sizes})")
        js, blob = gltf.read(glb)
        img = js["images"][0]
        view = js["bufferViews"][img["bufferView"]]
        png = blob[view.get("byteOffset", 0):view.get("byteOffset", 0) + view["byteLength"]]
        check(png[25] == 6, f"oak: the cutout texture keeps its alpha (PNG colour type {png[25]})")
        tex = os.path.join(tmp, "oak_tint.png")
        with open(tex, "wb") as f:
            f.write(png)
        px = _pixels_of(tex)
        r, g = px[px[..., 3] > 0.5][:, :2].mean(0)
        check(abs(r - 0.1) < 0.02 and abs(g - 0.6) < 0.02,
              f"oak: tint [0.5, 1.0, 1.0] halved the leaves' red only (r {r:.3f} g {g:.3f}, want 0.1 0.6)")

        check(man["mystery"]["fit"] == "Footprint", "mystery: unknown node id built with node defaults")
        pine = man.get("pine", {"variants": []})
        check(len(pine["variants"]) == 2, "pine: both variants built")
        for v, (want_w, want_h, what) in zip(pine["variants"], [(0.4, 2.0, "the column alone (plate cut)"),
                                                               (2.0, 2.02, "column and plate (no cut_plate)")]):
            w, h = v["max"][0] - v["min"][0], v["max"][1] - v["min"][1]
            check(abs(w - want_w) < 0.02 and abs(h - want_h) < 0.02,
                  f"pine: {v['model'].rsplit('/', 1)[-1]} is {what} ({w:.2f} x {h:.2f}, want {want_w:.2f} x {want_h:.2f})")
        check("palm" in man, "palm: built")
        palm = man.get("palm", {"variants": [{"triangles": -1}]})["variants"][0]
        check(palm["triangles"] <= 1500, f"palm: blades sharing stems (non-manifold) decimated within its "
              f"budget ({palm['triangles']} <= 1500)")
        check(palm["triangles"] >= 1000, f"palm: not decimated far below its budget ({palm['triangles']})")

        house = man["house"]
        check(house["kind"] == "Building" and house["fit"] == "Box" and house["banner"] == "true",
              "house: building defaults with a banner")
        glb = os.path.join(out, house["variants"][0]["model"])
        s = gltf.summarize(glb)
        check("Team_banner" in s["nodes"] and "Team" in s["nodes"]["Team_banner"]["materials"],
              "house: Team_banner node with the Team material")
        pts = _positions(glb, "house")
        top = pts[pts[:, 1] > pts[:, 1].max() - 0.3]
        check(top[:, 2].mean() > 0.3, f"house: yaw 180 turned the front (chimney) to +Z (z {top[:, 2].mean():.2f})")
        banner = s["nodes"]["Team_banner"]
        bx = (banner["min"][0] + banner["max"][0]) / 2
        check(banner["min"][0] < 0 and banner["min"][2] > 0 and bx < 0,
              f"house: default banner at the front-left (-X, +Z): {banner['min']}..{banner['max']}")
        check(house["variants"][0]["triangles"] <= 6000, "house: within the building budget")

        farm = man["farm"]
        s = gltf.summarize(os.path.join(out, farm["variants"][0]["model"]))
        b = s["nodes"]["Team_banner"]
        check(b["min"][0] < 1.0 < b["max"][0] and b["min"][2] < -2.0 < b["max"][2] and abs(b["min"][1] - 0.3) < 0.2,
              f"farm: banner foot on its anchor (1.0, 0.3, -2.0): {b['min']}..{b['max']}")
        mill = man["mill"]
        s = gltf.summarize(os.path.join(out, mill["variants"][0]["model"]))
        check(mill["banner"] == "false" and "Team_banner" not in s["nodes"] and not any(
            m["name"] == "Team" for m in s["materials"]), "mill: banner = false leaves no banner")
        reeds, ridge, odd = man.get("reeds", {}), man.get("mountain_ridge", {}), man.get("odd_rock", {})
        check(reeds.get("kind") == "Decor" and reeds.get("fit") == "Height" and reeds.get("size") == [1.0, 2.0, 1.0]
              and reeds.get("banner") == "false", f"reeds: decor with the size from assets.toml ({reeds})")
        r = ridge.get("variants", [{"min": [0, 0, 0], "max": [0, 0, 0]}])[0]
        w, d = r["max"][0] - r["min"][0], r["max"][2] - r["min"][2]
        check(abs(w - 8.0) < 0.05 and abs(d - 2.0) < 0.05,
              f"mountain_ridge: lying along Z, turned to lie along X ({w:.2f} x {d:.2f}, want 8 x 2)")
        check(ridge.get("size") == list(settings.DECOR["mountain_ridge"][1]) and r.get("triangles", 0)
              <= settings.DECOR["mountain_ridge"][2], "mountain_ridge: its default size and budget")
        check(odd.get("kind") == "Decor" and "size" not in odd, "odd_rock: an unknown decor id built, with no size")
        try:
            settings.resolve("nodes", "pine", None, {"pine": {"size": (1.0, 1.0, 1.0)}})
            check(False, "size on a node is refused")
        except settings.SettingsError:
            check(True, "size on a node is refused")
        _opacity_map_check(check)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    print(f"\nself-test: {'FAILED: ' + '; '.join(problems) if problems else 'all checks passed'}")
    return len(problems)

