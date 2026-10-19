"""Low-poly modelling helpers for the placeholder models (placeholders.py).

A `Model` collects faces with a colour each. `finish()` turns it into one
mesh object with one material whose base colour is a small palette atlas
(4x4 texels per colour, nearest filtering; every face's UVs sit on the
centre of its colour's cell). That is how low-poly game art is usually
textured: one material and one draw per model, like a Meshy model, and it
goes through the same texture path in the build.

Units are metres, Blender axes (Z up, the front faces -Y, which is glTF
+Z). Colours are sRGB in 0..1. Shapes are flat-shaded. Everything is
deterministic: randomness comes from `Model.rng`, seeded by the caller.
"""

import math
import os
import random
import tempfile

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

from . import png

CELL = 4  # texels per palette colour (each side)
SHADES = (-1.0, -0.5, 0.0, 0.5, 1.0)  # per-face variation steps (times `vary`)


def srgb(hex_or_tuple):
    if isinstance(hex_or_tuple, str):
        h = hex_or_tuple.lstrip("#")
        return tuple(int(h[i:i + 2], 16) / 255 for i in (0, 2, 4))
    return tuple(hex_or_tuple)


def shade(colour, f):
    """Lighter (f > 1) or darker (f < 1) version of an sRGB colour."""
    return tuple(min(1.0, max(0.0, c * f)) for c in srgb(colour))


def mix(a, b, t):
    a, b = srgb(a), srgb(b)
    return tuple(x + (y - x) * t for x, y in zip(a, b))


class Model:
    def __init__(self, name, seed=0):
        self.name = name
        self.bm = bmesh.new()
        self.colours = []          # palette, sRGB tuples
        self.face_colour = {}      # face -> palette index
        self.rng = random.Random(seed)

    # --- palette ----------------------------------------------------------
    def _colour(self, colour):
        c = tuple(round(v, 3) for v in srgb(colour))
        if c not in self.colours:
            self.colours.append(c)
        return self.colours.index(c)

    def paint(self, faces, colour, vary=0.0):
        """Give faces a colour; `vary` (0..0.2) makes each face a little
        lighter or darker at random (one of five shades, so the palette
        stays small), which reads as texture from afar."""
        for f in faces:
            c = colour if not vary else shade(colour, 1.0 + vary * self.rng.choice(SHADES))
            self.face_colour[f] = self._colour(c)
        return faces

    # --- primitives (each returns its faces) --------------------------------
    def _add(self, verts, faces, matrix, colour, vary, closed=False):
        """Faces wound counter-clockwise seen from outside (the engine culls
        back faces). `closed` shapes are re-wound outward by Blender."""
        bm = self.bm
        vs = [bm.verts.new(matrix @ Vector(v)) for v in verts]
        out = []
        for f in faces:
            try:
                out.append(bm.faces.new([vs[i] for i in f]))
            except ValueError:  # degenerate (repeated vertex)
                pass
        if closed and out:
            bmesh.ops.recalc_face_normals(bm, faces=out)
        return self.paint(out, colour, vary)

    def box(self, size, at=(0, 0, 0), colour="#888888", rot=0.0, vary=0.0, tilt=(0.0, 0.0), base=True):
        """A box `size` (x, y, z); `at` is the centre of its bottom face
        (base=True) or of the box. `rot` turns it about Z (degrees),
        `tilt` about X then Y."""
        sx, sy, sz = size
        z0 = 0.0 if base else -sz / 2
        verts = [(x * sx / 2, y * sy / 2, z0 + z * sz) for z in (0, 1) for y in (-1, 1) for x in (-1, 1)]
        faces = [(0, 2, 3, 1), (4, 5, 7, 6), (0, 1, 5, 4), (2, 6, 7, 3), (0, 4, 6, 2), (1, 3, 7, 5)]
        return self._add(verts, faces, self.place(at, rot, tilt), colour, vary, closed=True)

    def prism(self, sides, radius, height, at=(0, 0, 0), colour="#888888", top=None, rot=0.0, tilt=(0.0, 0.0),
              vary=0.0, cap_top=True, cap_bottom=False, tip=None, squash=(1.0, 1.0), jitter=0.0):
        """A `sides`-gon prism / frustum standing on `at`: `top` = top radius
        (None = same; 0 = a cone point). `tip` adds a point this far above
        the top (crystals). `squash` scales x, y. `jitter` moves vertices
        at random (fraction of the radius) for an organic look."""
        top = radius if top is None else top
        verts, faces = [], []
        j = lambda: self.rng.uniform(-jitter, jitter) * radius if jitter else 0.0  # noqa: E731
        for k, (r, z) in enumerate(((radius, 0.0), (top, height))):
            for i in range(sides):
                a = math.tau * i / sides
                verts.append((math.cos(a) * r * squash[0] + j(), math.sin(a) * r * squash[1] + j(),
                              z + (j() if k else 0.0)))
        for i in range(sides):
            n = (i + 1) % sides
            if top > 0:
                faces.append((i, n, sides + n, sides + i))
            else:
                faces.append((i, n, sides + i))
        if top > 0 and tip is not None:
            verts.append((0.0, 0.0, height + tip))
            c = len(verts) - 1
            faces += [(sides + i, sides + (i + 1) % sides, c) for i in range(sides)]
        elif top > 0 and cap_top:
            faces.append(tuple(range(sides, 2 * sides)))
        if cap_bottom:
            faces.append(tuple(reversed(range(sides))))
        if top == 0:  # cone: collapse the top ring into one apex
            verts = verts[:sides] + [(0.0, 0.0, height)]
            faces = [(i, (i + 1) % sides, sides) for i in range(sides)]
            if cap_bottom:
                faces.append(tuple(reversed(range(sides))))
        return self._add(verts, faces, self.place(at, rot, tilt), colour, vary)

    def blob(self, radius, at=(0, 0, 0), colour="#888888", scale=(1, 1, 1), jitter=0.12, subdiv=1, rot=0.0,
             tilt=(0.0, 0.0), vary=0.0, flat_bottom=None):
        """A lumpy low-poly sphere (icosphere: `subdiv` 1 = 20 faces, 2 = 80), centred
        on `at`. `flat_bottom` cuts it flat at that height (relative to
        the centre) so it sits on the ground."""
        tmp = bmesh.new()
        bmesh.ops.create_icosphere(tmp, subdivisions=subdiv, radius=radius)
        verts = []
        for v in tmp.verts:
            p = v.co * (1.0 + self.rng.uniform(-jitter, jitter))
            p = Vector((p.x * scale[0], p.y * scale[1], p.z * scale[2]))
            if flat_bottom is not None:
                p.z = max(p.z, flat_bottom)
            verts.append(tuple(p))
        index = {v: i for i, v in enumerate(tmp.verts)}
        faces = [tuple(index[v] for v in f.verts) for f in tmp.faces]
        tmp.free()
        return self._add(verts, faces, self.place(at, rot, tilt), colour, vary, closed=True)

    def poly(self, verts, faces, at=(0, 0, 0), colour="#888888", rot=0.0, tilt=(0.0, 0.0), vary=0.0):
        return self._add(verts, faces, self.place(at, rot, tilt), colour, vary)

    def gable_roof(self, width, depth, rise, at=(0, 0, 0), colour="#884433", overhang=0.25, thick=0.12,
                   rot=0.0, gable_colour=None, vary=0.0, rows=0):
        """A gable roof whose ridge runs along X (turn it with `rot`), eaves
        at `at` z. The gable ends (triangles) take `gable_colour` (walls)
        when given. `rows` adds that many courses of tiles / thatch per
        slope (thin darker battens), which read as texture from afar."""
        w, d = width / 2 + overhang, depth / 2 + overhang
        t = thick
        verts = [(-w, -d, 0), (w, -d, 0), (w, 0, rise), (-w, 0, rise), (-w, d, 0), (w, d, 0),
                 (-w, -d, -t), (w, -d, -t), (w, d, -t), (-w, d, -t), (-w, 0, rise - t * 1.4), (w, 0, rise - t * 1.4)]
        faces = [(0, 1, 2, 3), (3, 2, 5, 4),                 # top slopes
                 (6, 10, 11, 7), (10, 9, 8, 11),             # undersides
                 (0, 6, 7, 1), (4, 5, 8, 9),                 # eave edges
                 (0, 3, 10, 6), (3, 4, 9, 10),               # left end slab
                 (1, 7, 11, 2), (2, 11, 8, 5)]               # right end slab
        out = self._add(verts, faces, self.place(at, rot), colour, vary, closed=True)
        if gable_colour is not None:
            gw, gd = width / 2, depth / 2
            tri = [(-gw, -gd, -t), (-gw, gd, -t), (-gw, 0, rise - t * 1.4), (gw, -gd, -t), (gw, 0, rise - t * 1.4),
                   (gw, gd, -t)]
            out += self._add(tri, [(0, 2, 1), (3, 5, 4)], self.place(at, rot), gable_colour, 0.0)
        if rows:
            m = self.place(at, rot)
            dark = shade(colour, 0.78)
            for i in range(1, rows + 1):
                t = i / (rows + 1)
                for sy in (-1, 1):
                    y, z = sy * d * (1 - t), rise * t
                    p0, p1 = m @ Vector((-w, y, z + 0.01)), m @ Vector((w, y, z + 0.01))
                    out += self.segment(p0, p1, 0.035 + 0.01 * thick, colour=dark, sides=3, spin=90 if sy < 0 else -90)
        return out

    def segment(self, p0, p1, radius, colour="#888888", sides=4, vary=0.0, top=None, spin=45.0):
        """A prism from point p0 to p1 (beams, posts, branches, rails);
        `top` = radius at p1. Square beams by default (sides=4)."""
        p0, p1 = Vector(p0), Vector(p1)
        d = p1 - p0
        q = d.to_track_quat("Z", "Y")
        m = Matrix.Translation(p0) @ q.to_matrix().to_4x4() @ Matrix.Rotation(math.radians(spin), 4, "Z")
        top = radius if top is None else top
        verts = []
        for r, z in ((radius, 0.0), (top, d.length)):
            for i in range(sides):
                a = math.tau * i / sides
                verts.append((math.cos(a) * r, math.sin(a) * r, z))
        faces = [(i, (i + 1) % sides, sides + (i + 1) % sides, sides + i) for i in range(sides)]
        faces += [tuple(range(sides, 2 * sides)), tuple(reversed(range(sides)))]
        return self._add(verts, faces, m, colour, vary, closed=True)

    def mark(self):
        """A bookmark for `transform_since`."""
        return len(self.bm.verts)

    def transform_since(self, mark, fn):
        """Move every vertex added after `mark`: `fn(Vector) -> Vector`
        (leaning a tree, bending a frond)."""
        self.bm.verts.ensure_lookup_table()
        for v in self.bm.verts[mark:]:
            v.co = fn(v.co.copy())

    @staticmethod
    def place(at=(0, 0, 0), rot=0.0, tilt=(0.0, 0.0)):
        return (Matrix.Translation(Vector(at)) @ Matrix.Rotation(math.radians(rot), 4, "Z")
                @ Matrix.Rotation(math.radians(tilt[1]), 4, "Y") @ Matrix.Rotation(math.radians(tilt[0]), 4, "X"))

    # --- output -----------------------------------------------------------
    def finish(self):
        """The mesh object (flat shaded, palette atlas material)."""
        bm = self.bm
        n = max(1, len(self.colours))
        cols = math.ceil(math.sqrt(n))
        size = cols * CELL
        atlas = np.zeros((size, size, 3), np.uint8)
        for i, c in enumerate(self.colours):
            r, q = divmod(i, cols)
            atlas[r * CELL:(r + 1) * CELL, q * CELL:(q + 1) * CELL] = np.round(np.array(c) * 255)
        uv = bm.loops.layers.uv.new("UVMap")
        for f in bm.faces:
            i = self.face_colour.get(f, 0)
            r, q = divmod(i, cols)
            u, v = (q + 0.5) / cols, 1.0 - (r + 0.5) / cols  # UV origin is bottom-left; row 0 is the top
            f.smooth = False
            for loop in f.loops:
                loop[uv].uv = (u, v)
        mesh = bpy.data.meshes.new(self.name)
        bm.to_mesh(mesh)
        bm.free()
        mesh.materials.append(_atlas_material(self.name, atlas))
        obj = bpy.data.objects.new(self.name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        return obj


def _atlas_material(name, atlas):
    data, _ = png.encode_rgb(atlas, 0)
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, name + "_palette.png")
        with open(path, "wb") as f:
            f.write(data)
        img = bpy.data.images.load(path)
        img.pack()
    img.name = name + "_palette"
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.interpolation = "Closest"
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = 0.85
    bsdf.inputs["Metallic"].default_value = 0.0
    nt.nodes.active = tex
    return mat
