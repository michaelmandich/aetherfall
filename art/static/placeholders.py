"""Placeholder models: low-poly stand-ins for every common building and node.

Run headless from the repository root:

    "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b \
        -P art/static/placeholders.py -- [options]

Options (after the `--`):
    (none)          write every placeholder source to art/static/placeholders/source/
    --only a,b      only these sources (file stems: pine_a, house...)
    --build         then build them like real models into assets/models/_placeholders/,
                    registry assets/visuals-placeholders/static.ron (both git-ignored),
                    with the settings in placeholders/assets.toml (not art/static/assets.toml);
                    run the game with `--visuals visuals-placeholders` to see them
    --sheets DIR    contact sheets for --build (default: a temp folder)
    --fixtures      instead: write the tiny test models and build them into
                    crates/af_render/tests/fixtures/static/ (the engine's tests),
                    with the contract's default settings

The sources are committed (they are small); the built outputs are not.
Every recipe models at the game's box size (assets/data), in metres,
Blender axes: Z up, the front toward -Y (glTF +Z). Shapes and colours come
from lib/shapes.py; randomness is seeded per recipe, so the sources are
reproducible.
"""

import argparse
import math
import os
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

from mathutils import Vector  # noqa: E402

from lib import gltf, scene  # noqa: E402
from lib.shapes import Model, shade  # noqa: E402

SOURCE = os.path.join(HERE, "placeholders", "source")
# The placeholder build's own settings: art/static/assets.toml is for real
# models and is not read here (see the file).
SETTINGS = os.path.join(HERE, "placeholders", "assets.toml")
FIXTURES = os.path.join(REPO, "crates", "af_render", "tests", "fixtures", "static")

# --- palette (sRGB) ----------------------------------------------------------
BARK = "#5e412a"
WOOD_CUT = "#d2ac72"
WOOD_RING = "#a8794a"
PINE = "#356b3c"
PINE_LIGHT = "#467f4b"
PINE_UNDER = "#24462b"
OAK = "#4d7f2c"
OAK_LIGHT = "#68973a"
PALM_LEAF = "#5f9a36"
PALM_TRUNK = "#93714a"
BUSH = "#46803a"
BUSH_LIGHT = "#5a9645"
BERRY = "#c0223f"
BERRY_DARK = "#7b2a7f"
STONE = "#b3aea4"
STONE_DARK = "#99948b"
COPPER_ROCK = "#857868"
COPPER = "#d27a3c"
COPPER_LIGHT = "#eb9a55"
PATINA = "#4f9e85"
IRON_ROCK = "#686c76"
RUST = "#a14d2b"
IRON = "#3d424c"
SAND = "#ecdfb2"
QUARTZ = "#f3f5f1"
QUARTZ_BLUE = "#c4dbe6"
AETHER = "#9256ec"
AETHER_LIGHT = "#cfa8ff"
AETHER_DARK = "#5a2fa0"
CRATER_ROCK = "#574f61"
METEOR = "#4a4252"
GOLD = "#e6bd4c"
GOLD_LIGHT = "#fff0a6"
RUBBLE = "#6e5d4d"
PLASTER = "#e6dac0"
TIMBER = "#5a3a22"
DOOR = "#4a2e1b"
WINDOW = "#2b2d38"
SHUTTER = "#8a5d34"
THATCH = "#c49448"
ROOF_TILE = "#a24c33"
STONE_WALL = "#a09a8e"
STONE_BASE = "#878177"
SOIL = "#6e4a2d"
SOIL_DARK = "#573a23"
CROP = "#7fae3c"
GRASS_EDGE = "#6b8a3a"
WHEAT = "#d4b84e"
DIRT = "#8a6a48"
GRAVEL = "#867d70"
PLANK = "#9a7349"
METAL = "#5b5f66"
SLATE = "#5f6672"
VENT_FLOOR = "#3d3348"


# ===========================================================================
# Shared parts
# ===========================================================================

def stump_top(m, radius, height, sides, at=(0.0, 0.0)):
    """The cut face of a trunk: light wood with a darker ring."""
    x, y = at
    m.prism(sides, radius * 0.98, 0.012, at=(x, y, height), colour=WOOD_RING, cap_top=True)
    m.prism(sides, radius * 0.62, 0.012, at=(x, y, height + 0.012), colour=WOOD_CUT, cap_top=True)


def roots(m, radius, count, colour=BARK, length=0.42, height=0.28):
    """Root flares round a trunk's foot: wedges sloping to the ground."""
    for i in range(count):
        a = math.tau * (i + m.rng.uniform(-0.15, 0.15)) / count
        d = Vector((math.cos(a), math.sin(a), 0.0))
        side = Vector((-d.y, d.x, 0.0)) * radius * 0.42
        base = d * radius * 0.75
        tip = d * (radius + length * m.rng.uniform(0.8, 1.2))
        top = d * radius * 0.85 + Vector((0, 0, height * m.rng.uniform(0.8, 1.2)))
        verts = [base - side, base + side, tip, top]
        m.poly(verts, [(0, 2, 3), (2, 1, 3), (0, 3, 1)], colour=colour, vary=0.06)


def stump(m, radius, height, sides=8, flares=4):
    m.prism(sides, radius, height, top=radius * 0.94, colour=BARK, cap_top=False, vary=0.08)
    stump_top(m, radius * 0.94, height, sides)
    roots(m, radius, flares, length=0.6 * radius, height=0.45 * height)


def star_cone(m, z, radius, height, points, colour, under, droop=0.22, inner=0.7, rot=0.0):
    """A pine tier: a cone whose rim is a drooping star (points out and
    down), with a darker, slightly hollow underside."""
    n = points * 2
    rim = []
    for i in range(n):
        a = math.tau * i / n + math.radians(rot)
        out = i % 2 == 0
        r = radius * (1.0 if out else inner) * (1 + m.rng.uniform(-0.07, 0.07))
        rim.append((math.cos(a) * r, math.sin(a) * r, z - (droop if out else 0.0) + m.rng.uniform(-0.04, 0.04)))
    apex = (m.rng.uniform(-0.04, 0.04) * radius, m.rng.uniform(-0.04, 0.04) * radius, z + height)
    m.poly(rim + [apex], [(i, (i + 1) % n, n) for i in range(n)], colour=colour, vary=0.09)
    hollow = (0.0, 0.0, z + 0.22 * height)
    m.poly(rim + [hollow], [((i + 1) % n, i, n) for i in range(n)], colour=under, vary=0.05)


def crystal(m, radius, height, tip, at, colour, light, tilt=(0.0, 0.0), rot=0.0, sides=6):
    """A hexagonal crystal: a prism body and a pointed tip in a lighter shade."""
    mat = m.place(at, rot, tilt)
    ring_lo = [(math.cos(math.tau * i / sides) * radius, math.sin(math.tau * i / sides) * radius, 0.0)
               for i in range(sides)]
    ring_hi = [(x * 0.92, y * 0.92, height) for x, y, _ in ring_lo]
    body = [(i, (i + 1) % sides, sides + (i + 1) % sides, sides + i) for i in range(sides)]
    m._add(ring_lo + ring_hi, body, mat, colour, 0.12)
    m._add(ring_hi + [(0.0, 0.0, height + tip)], [(i, (i + 1) % sides, sides) for i in range(sides)], mat, light, 0.08)


def surface(centre, radii, a_deg, e_deg, depth=0.92):
    """A point on an ellipsoid (a rock's surface), a little inside it."""
    a, e = math.radians(a_deg), math.radians(e_deg)
    return Vector((centre[0] + radii[0] * math.cos(e) * math.cos(a) * depth,
                   centre[1] + radii[1] * math.cos(e) * math.sin(a) * depth,
                   centre[2] + radii[2] * math.sin(e) * depth))


def boulder(m, x, y, r, scale, colour, jitter=0.2, subdiv=2, vary=0.08, sink=0.35):
    """A rock resting on the ground; returns (centre, radii) for placing ore on it."""
    cz = r * scale[2] * (1.0 - sink)
    m.blob(r, at=(x, y, cz), scale=scale, colour=colour, jitter=jitter, subdiv=subdiv, vary=vary,
           flat_bottom=-cz, rot=m.rng.uniform(0, 360))
    return (x, y, cz), (r * scale[0], r * scale[1], r * scale[2])


def vein(m, rock, a0, a1, e, colours, size=0.13, count=5):
    """A streak of ore nuggets along an arc over a rock's surface."""
    centre, radii = rock
    for k in range(count):
        t = k / max(1, count - 1)
        p = surface(centre, radii, a0 + (a1 - a0) * t, e + m.rng.uniform(-8, 8), depth=0.93)
        s = size * m.rng.uniform(0.75, 1.25)
        m.blob(s, at=tuple(p), colour=colours[k % len(colours)], scale=(1.5, 1.0, 0.9), jitter=0.2, subdiv=1,
               rot=a0 + (a1 - a0) * t + 90, vary=0.08)


def log(m, p0, p1, radius, sides=7):
    """A felled log: bark and two cut ends."""
    p0, p1 = Vector(p0), Vector(p1)
    d = (p1 - p0).normalized()
    m.segment(p0, p1, radius, colour=BARK, sides=sides, vary=0.08, spin=m.rng.uniform(0, 60))
    for end, sign in ((p0, -1), (p1, 1)):
        m.segment(end, end + d * sign * 0.012, radius * 0.86, colour=WOOD_CUT, sides=sides)


def lean_to(m, x0, x1, y0, y1, z_front, z_back, post=0.08, roof=PLANK):
    """An open shed: four posts and a sloping plank roof (higher at +y, the back)."""
    for x in (x0, x1):
        for y, z in ((y0, z_front), (y1, z_back)):
            m.segment((x, y, 0.0), (x, y, z), post, colour=TIMBER)
    m.segment((x0, y0, z_front - 0.1), (x1, y0, z_front - 0.1), post * 0.9, colour=TIMBER)
    m.segment((x0, y1, z_back - 0.1), (x1, y1, z_back - 0.1), post * 0.9, colour=TIMBER)
    over = 0.25
    dy, dz = y1 - y0, z_back - z_front
    k = over / math.hypot(dy, dz)
    a = (x0 - over, y0 - dy * k, z_front - dz * k)
    b = (x1 + over, y0 - dy * k, z_front - dz * k)
    c = (x1 + over, y1 + dy * k, z_back + dz * k)
    d = (x0 - over, y1 + dy * k, z_back + dz * k)
    t = 0.09
    verts = [a, b, c, d] + [(v[0], v[1], v[2] - t) for v in (a, b, c, d)]
    faces = [(0, 1, 2, 3), (7, 6, 5, 4), (0, 4, 5, 1), (1, 5, 6, 2), (2, 6, 7, 3), (3, 7, 4, 0)]
    m.poly(verts, faces, colour=roof, vary=0.08)
    # plank seams: thin battens across the slope
    for i in range(1, 5):
        x = x0 - over + (x1 - x0 + 2 * over) * i / 5
        m.segment((x, a[1], a[2] + 0.01), (x, d[1], d[2] + 0.01), 0.035, colour=shade(roof, 0.75))


# ===========================================================================
# Trees (fit Height: the box is the trunk; canopies spread past it)
# ===========================================================================

RECIPES = {}


def recipe(kind, name, seed):
    def register(fn):
        RECIPES[(kind, name)] = (fn, seed)
        return fn
    return register


def pine(m, tiers, lean=0.0, trunk=2.2):
    mark = m.mark()
    m.prism(7, 0.22, trunk, top=0.12, colour=BARK, cap_top=False, vary=0.08)
    roots(m, 0.2, 3, length=0.25, height=0.18)
    for i, (z, r, h) in enumerate(tiers):
        star_cone(m, z, r, h, 8 if r > 0.8 else 6, PINE if i % 2 == 0 else PINE_LIGHT, PINE_UNDER,
                  droop=0.16 + 0.1 * r, rot=m.rng.uniform(0, 45))
    if lean:
        m.transform_since(mark, lambda v: Vector((v.x + lean * v.z * v.z / 36.0, v.y, v.z)))


@recipe("nodes", "pine_a", 11)
def pine_a(m):
    pine(m, [(1.0, 1.5, 2.1), (2.05, 1.26, 1.9), (3.0, 1.0, 1.75), (3.9, 0.74, 1.5), (4.7, 0.45, 1.3)])


@recipe("nodes", "pine_b", 12)
def pine_b(m):
    pine(m, [(1.35, 1.2, 1.9), (2.25, 1.05, 1.7), (3.05, 0.88, 1.6), (3.8, 0.7, 1.45), (4.5, 0.5, 1.25),
             (5.1, 0.3, 0.9)], trunk=2.6)


@recipe("nodes", "pine_c", 13)
def pine_c(m):
    pine(m, [(1.5, 1.65, 2.2), (2.75, 1.3, 2.0), (3.85, 0.92, 1.85), (4.85, 0.5, 1.15)], lean=0.35, trunk=2.6)


@recipe("nodes", "pine_stump", 14)
def pine_stump(m):
    stump(m, 0.24, 0.5, sides=7, flares=3)


def oak_tree(m, trunk_h, lobes, branches, lean=(0.0, 0.0)):
    m.prism(8, 0.33, trunk_h, top=0.22, colour=BARK, cap_top=False, vary=0.08, tilt=lean)
    roots(m, 0.3, 5, length=0.35, height=0.25)
    top = Vector((math.sin(math.radians(lean[1])) * trunk_h, -math.sin(math.radians(lean[0])) * trunk_h, trunk_h))
    for (dx, dy, dz) in branches:
        start = top * 0.8
        m.segment(start, start + Vector((dx, dy, dz)), 0.13, top=0.07, colour=BARK, sides=5)
    for x, y, z, r, flat in lobes:
        m.blob(r, at=(x, y, z), scale=(1.0, 1.0, flat), colour=OAK_LIGHT if z > trunk_h + 0.9 else OAK,
               jitter=0.12, subdiv=2, vary=0.09, rot=m.rng.uniform(0, 360))


@recipe("nodes", "oak_a", 21)
def oak_a(m):
    oak_tree(m, 2.5, [(0.0, 0.0, 3.55, 1.35, 0.85), (0.95, 0.25, 3.05, 0.95, 0.85), (-0.9, -0.3, 3.15, 1.0, 0.85),
                      (0.2, -0.85, 3.25, 0.9, 0.85), (-0.25, 0.85, 3.35, 0.9, 0.85), (0.1, 0.1, 4.25, 0.85, 0.8)],
             [(0.9, 0.3, 0.9), (-0.8, -0.4, 0.8), (0.1, 0.9, 1.0)])


@recipe("nodes", "oak_b", 22)
def oak_b(m):
    oak_tree(m, 2.2, [(0.6, 0.0, 3.1, 1.15, 0.75), (-0.75, 0.2, 3.0, 1.1, 0.75), (0.0, -0.75, 3.2, 1.0, 0.75),
                      (0.0, 0.8, 3.15, 1.0, 0.75), (0.0, 0.0, 3.85, 1.0, 0.75)],
             [(1.0, 0.0, 0.8), (-1.0, 0.2, 0.8), (0.0, -0.8, 0.9)], lean=(0.0, 4.0))


@recipe("nodes", "oak_c", 23)
def oak_c(m):
    oak_tree(m, 2.9, [(0.0, 0.0, 3.85, 1.15, 0.95), (0.7, -0.2, 3.45, 0.85, 0.9), (-0.65, 0.25, 3.5, 0.85, 0.9),
                      (0.1, 0.1, 4.45, 0.65, 0.9)],
             [(0.7, 0.2, 0.9), (-0.6, -0.3, 0.8)], lean=(-3.0, 0.0))


@recipe("nodes", "oak_stump", 24)
def oak_stump(m):
    stump(m, 0.33, 0.45, sides=8, flares=5)


def palm_tree(m, height, bend, fronds, phase):
    pts = [Vector((bend * (i / 7) ** 2, 0.15 * bend * math.sin(i / 7 * 3.0), height * i / 7)) for i in range(8)]
    for i in range(7):
        r0, r1 = 0.21 - 0.012 * i, 0.21 - 0.012 * (i + 1)
        m.segment(pts[i], pts[i + 1] + (pts[i + 1] - pts[i]) * 0.04, r0, top=r1 * 1.12, sides=6, spin=i * 17,
                  colour=PALM_TRUNK if i % 2 == 0 else shade(PALM_TRUNK, 0.82))
    crown = pts[-1]
    m.blob(0.26, at=tuple(crown + Vector((0, 0, 0.05))), colour=shade(PALM_LEAF, 0.7), subdiv=1, jitter=0.1)
    for k in range(3):
        a = math.tau * k / 3 + phase
        m.blob(0.13, at=tuple(crown + Vector((math.cos(a) * 0.2, math.sin(a) * 0.2, -0.22))), colour="#6b4a2a",
               subdiv=1, jitter=0.1)
    for k in range(fronds):
        a = math.tau * k / fronds + phase + m.rng.uniform(-0.15, 0.15)
        d = Vector((math.cos(a), math.sin(a), 0.0))
        side = Vector((-d.y, d.x, 0.0))
        length = m.rng.uniform(2.0, 2.5)
        rise, droop = 0.55, 1.75 * m.rng.uniform(0.85, 1.15)
        spine = []
        for j in range(5):
            t = j / 4
            spine.append(crown + d * length * t + Vector((0, 0, rise * t - droop * t * t)))
        widths = (0.06, 0.34, 0.38, 0.26, 0.03)
        verts, top_faces, under_faces = [], [], []
        for j, (p, w) in enumerate(zip(spine, widths)):
            down = Vector((0, 0, -0.32 * w))
            verts += [p - side * w + down, p + Vector((0, 0, 0.03)), p + side * w + down]
        for j in range(4):
            a0, b0 = 3 * j, 3 * (j + 1)
            top_faces += [(a0, b0, b0 + 1, a0 + 1), (a0 + 1, b0 + 1, b0 + 2, a0 + 2)]
            under_faces += [(a0 + 1, b0 + 1, b0, a0), (a0 + 2, b0 + 2, b0 + 1, a0 + 1)]
        m.poly(verts, top_faces, colour=PALM_LEAF if k % 2 else shade(PALM_LEAF, 1.12), vary=0.08)
        m.poly([v + Vector((0, 0, -0.015)) for v in verts], under_faces, colour=shade(PALM_LEAF, 0.7))


@recipe("nodes", "palm_a", 31)
def palm_a(m):
    palm_tree(m, 4.95, 0.9, 7, 0.2)


@recipe("nodes", "palm_b", 32)
def palm_b(m):
    palm_tree(m, 5.0, -0.55, 8, 0.6)


@recipe("nodes", "palm_stump", 33)
def palm_stump(m):
    m.prism(6, 0.21, 0.45, top=0.19, colour=PALM_TRUNK, cap_top=False, vary=0.08)
    m.prism(6, 0.205, 0.08, at=(0, 0, 0.18), colour=shade(PALM_TRUNK, 0.8), cap_top=False)
    stump_top(m, 0.19, 0.45, 6)


# ===========================================================================
# Other nodes (fit Footprint: one scale, the wider side matches the box)
# ===========================================================================

@recipe("nodes", "berry_bush", 41)
def berry_bush(m):
    lobes = [(0.0, 0.0, 0.42, 0.56, (1.1, 1.0, 0.85)), (0.42, 0.15, 0.33, 0.42, (1, 1, 0.9)),
             (-0.4, 0.2, 0.32, 0.4, (1, 1, 0.9)), (0.05, -0.42, 0.31, 0.4, (1, 1, 0.9)),
             (0.0, 0.42, 0.33, 0.38, (1, 1, 0.9))]
    for x, y, z, r, sc in lobes:
        m.blob(r, at=(x, y, z), scale=sc, colour=BUSH_LIGHT if z > 0.4 else BUSH, jitter=0.13, subdiv=2, vary=0.09,
               flat_bottom=-z, rot=m.rng.uniform(0, 360))
    for k in range(14):
        a = 360 * k / 14 + m.rng.uniform(-10, 10)
        e = m.rng.uniform(15, 65) if k % 4 else 80
        p = surface((0.0, 0.0, 0.3), (0.72, 0.68, 0.55), a, e, depth=1.0)
        colour = BERRY if k % 4 else BERRY_DARK
        for _ in range(3):
            o = Vector((m.rng.uniform(-0.08, 0.08), m.rng.uniform(-0.08, 0.08), m.rng.uniform(-0.03, 0.05)))
            m.blob(0.085, at=tuple(p + o), colour=colour, subdiv=1, jitter=0.05)


@recipe("nodes", "stone_outcrop", 51)
def stone_outcrop(m):
    boulder(m, 0.25, 0.15, 0.95, (1.15, 1.0, 0.95), STONE)
    boulder(m, -0.7, -0.25, 0.7, (1.0, 1.1, 0.95), STONE_DARK)
    boulder(m, 0.1, -0.75, 0.55, (1.1, 0.9, 0.8), STONE)
    boulder(m, -0.45, 0.75, 0.5, (1.0, 1.0, 0.85), shade(STONE, 0.93))
    for x, y, r in ((1.05, -0.55, 0.22), (-1.1, 0.35, 0.2), (0.85, 0.85, 0.18), (-0.15, -1.1, 0.16)):
        boulder(m, x, y, r, (1.2, 1.0, 0.7), STONE_DARK, subdiv=1, jitter=0.15)


def ore_rocks(m, rock_colour, veins, nuggets, height_scale=0.95):
    a = boulder(m, 0.2, 0.1, 0.85, (1.2, 1.0, height_scale), rock_colour)
    b = boulder(m, -0.65, -0.2, 0.62, (1.0, 1.1, height_scale), shade(rock_colour, 0.92))
    c = boulder(m, 0.05, -0.7, 0.45, (1.1, 0.9, 0.85), shade(rock_colour, 1.06))
    for x, y, r in ((0.95, -0.5, 0.2), (-1.0, 0.45, 0.18), (0.7, 0.8, 0.17)):
        boulder(m, x, y, r, (1.2, 1.0, 0.7), shade(rock_colour, 0.9), subdiv=1, jitter=0.15)
    vein(m, a, -70, 50, 38, veins, size=0.18, count=7)
    vein(m, a, 110, 210, 18, veins, size=0.15, count=5)
    vein(m, b, 140, 270, 32, veins, size=0.15, count=6)
    vein(m, c, -160, -50, 25, veins, size=0.13, count=5)
    for rock, ang, e, size in ((a, 80, 62, 0.22), (b, 300, 55, 0.18), (c, 0, 50, 0.15), (a, 200, 55, 0.18)):
        p = surface(*rock, ang, e, depth=0.95)
        m.blob(size, at=tuple(p), colour=nuggets, scale=(1.2, 1.0, 0.9), subdiv=1, jitter=0.2)


@recipe("nodes", "copper_vein", 61)
def copper_vein(m):
    ore_rocks(m, COPPER_ROCK, (COPPER, COPPER_LIGHT, COPPER), PATINA, 0.9)


@recipe("nodes", "iron_vein", 62)
def iron_vein(m):
    ore_rocks(m, IRON_ROCK, (RUST, shade(RUST, 1.2), IRON), IRON)


@recipe("nodes", "silicon_deposit", 63)
def silicon_deposit(m):
    m.blob(1.2, at=(0.05, 0.0, 0.0), scale=(1.0, 0.95, 0.42), colour=SAND, jitter=0.07, subdiv=2, vary=0.05,
           flat_bottom=0.0)
    m.blob(0.7, at=(-0.65, 0.55, 0.0), scale=(1.0, 1.0, 0.5), colour=shade(SAND, 0.96), jitter=0.08, subdiv=2,
           vary=0.05, flat_bottom=0.0)
    for x, y, r, h, tx, ty in ((0.15, -0.15, 0.17, 0.42, 8, -12), (0.4, 0.1, 0.13, 0.32, -18, 10),
                               (-0.15, 0.3, 0.12, 0.3, 14, 18), (-0.7, 0.5, 0.12, 0.28, -10, -16),
                               (0.65, -0.5, 0.11, 0.22, 20, 8), (-0.45, -0.4, 0.11, 0.24, -16, 14),
                               (0.0, -0.65, 0.1, 0.2, 22, -6), (0.75, 0.45, 0.1, 0.2, -12, 20)):
        z = 0.42 * math.sqrt(max(0.0, 1 - (x * x + y * y) / 1.3)) * 0.9
        crystal(m, r, h, r * 1.1, (x, y, z), QUARTZ if r > 0.115 else QUARTZ_BLUE, "#ffffff", tilt=(tx, ty))
    for _ in range(10):
        a, d = m.rng.uniform(0, math.tau), m.rng.uniform(0.2, 0.95)
        x, y = math.cos(a) * d * 1.1, math.sin(a) * d * 1.0
        z = 0.48 * math.sqrt(max(0.0, 1 - d * d)) - 0.03
        m.blob(0.06, at=(x, y, z), colour=QUARTZ, subdiv=1, jitter=0.2)


@recipe("nodes", "aether_crystal", 71)
def aether_crystal(m):
    boulder(m, 0.0, 0.0, 0.5, (1.25, 1.15, 0.6), CRATER_ROCK, subdiv=2)
    boulder(m, 0.38, -0.3, 0.26, (1.0, 1.0, 0.7), shade(CRATER_ROCK, 1.1), subdiv=1)
    crystal(m, 0.3, 1.95, 0.55, (0.0, 0.03, 0.15), AETHER, AETHER_LIGHT, tilt=(-3, 4), rot=10)
    for x, y, r, h, tx, ty, rot in ((0.26, -0.1, 0.19, 1.25, 8, 17, 30), (-0.26, 0.1, 0.2, 1.4, -5, -18, 5),
                                    (0.05, -0.3, 0.16, 1.0, 19, 3, 20), (-0.05, 0.3, 0.15, 0.9, -20, -5, 40),
                                    (0.3, 0.28, 0.12, 0.62, -16, 20, 0), (-0.32, -0.26, 0.12, 0.58, 18, -20, 15)):
        crystal(m, r, h, r * 1.7, (x, y, 0.12), AETHER if h > 0.8 else AETHER_DARK, AETHER_LIGHT, tilt=(tx, ty),
                rot=rot)


@recipe("nodes", "aether_vent", 72)
def aether_vent(m):
    m.prism(12, 1.42, 0.06, colour=VENT_FLOOR, cap_top=True, vary=0.06)
    for k in range(14):
        a = math.tau * k / 14 + m.rng.uniform(-0.08, 0.08)
        r = m.rng.uniform(1.22, 1.38)
        boulder(m, math.cos(a) * r, math.sin(a) * r, m.rng.uniform(0.18, 0.26), (1.4, 1.0, 0.8), CRATER_ROCK,
                subdiv=1, jitter=0.18, sink=0.4)
    # the glowing pool and the cracks running out of it
    m.prism(9, 0.78, 0.04, at=(0, 0, 0.06), top=0.74, colour=AETHER, cap_top=True, vary=0.08)
    m.prism(9, 0.46, 0.05, at=(0, 0, 0.06), top=0.42, colour=AETHER_LIGHT, cap_top=True, rot=20)
    for k in range(7):
        a = math.tau * k / 7 + 0.3
        pts = []
        for j in range(4):
            r = 0.6 + 0.2 * j
            aa = a + m.rng.uniform(-0.18, 0.18)
            pts.append(Vector((math.cos(aa) * r, math.sin(aa) * r, 0)))
        for j, (p0, p1) in enumerate(zip(pts, pts[1:])):
            d = (p1 - p0).normalized()
            w0, w1 = 0.1 * (1 - j / 3), 0.1 * (1 - (j + 1) / 3) + 0.015
            z = Vector((0, 0, 0.075))
            s0, s1 = Vector((-d.y, d.x, 0)) * w0, Vector((-d.y, d.x, 0)) * w1
            m.poly([p0 - s0 + z, p1 - s1 + z, p1 + s1 + z, p0 + s0 + z], [(0, 1, 2, 3)], colour=AETHER_LIGHT)
    for x, y, h, tx, ty in ((0.05, 0.0, 0.34, 0, 0), (0.17, 0.1, 0.22, 15, 25), (-0.14, 0.06, 0.25, -10, -25),
                            (0.0, -0.18, 0.2, 25, 0)):
        crystal(m, 0.07, h, 0.09, (x, y, 0.1), AETHER, AETHER_LIGHT, tilt=(tx, ty))


@recipe("nodes", "adamant_meteor", 73)
def adamant_meteor(m):
    for k in range(12):
        a = math.tau * k / 12 + m.rng.uniform(-0.1, 0.1)
        r = m.rng.uniform(1.15, 1.35)
        boulder(m, math.cos(a) * r, math.sin(a) * r, m.rng.uniform(0.3, 0.42), (1.3, 1.0, 0.7), RUBBLE,
                subdiv=1, jitter=0.18, sink=0.4)
    rock = boulder(m, 0.0, 0.05, 0.95, (1.15, 1.0, 0.95), METEOR, jitter=0.24, sink=0.25, vary=0.12)
    vein(m, rock, -40, 60, 20, (GOLD, shade(GOLD, 0.85)), size=0.14, count=6)
    vein(m, rock, 150, 250, 35, (GOLD, shade(GOLD, 0.85)), size=0.13, count=5)
    for ang, e, h, r in ((20, 42, 0.75, 0.17), (95, 30, 0.6, 0.15), (160, 55, 0.8, 0.17), (230, 25, 0.55, 0.14),
                         (300, 45, 0.7, 0.16), (340, 72, 0.5, 0.14), (60, 70, 0.55, 0.14), (200, 75, 0.45, 0.13)):
        p = surface(*rock, ang, e, depth=0.88)
        normal = (p - Vector(rock[0])).normalized()
        tilt_x = -math.degrees(math.asin(max(-1.0, min(1.0, normal.y))))
        tilt_y = math.degrees(math.asin(max(-1.0, min(1.0, normal.x))))
        crystal(m, r, h, r * 1.6, tuple(p), GOLD, GOLD_LIGHT, tilt=(tilt_x * 0.8, tilt_y * 0.8), rot=ang)
    for k in range(5):
        a = math.tau * k / 5 + 0.4
        m.blob(0.12, at=(math.cos(a) * 1.0, math.sin(a) * 1.0, 0.12), colour=GOLD, subdiv=1, jitter=0.2)


# ===========================================================================
# Buildings (fit Box: scaled per axis to the box; the build adds the banner)
# ===========================================================================

def timber_walls(m, w, d, z0, h, posts=(0.0,), band=True, braces=True):
    """Dark timber framing on a plastered block w x d from z0 to z0 + h."""
    t = 0.09
    x, y = w / 2 + 0.02, d / 2 + 0.02
    for sx in (-1, 1):
        for sy in (-1, 1):
            m.segment((sx * x, sy * y, z0), (sx * x, sy * y, z0 + h), t, colour=TIMBER)
    for z in ((z0 + 0.05, z0 + h * 0.5, z0 + h - 0.05) if band else (z0 + 0.05, z0 + h - 0.05)):
        m.segment((-x, -y, z), (x, -y, z), t * 0.8, colour=TIMBER)
        m.segment((-x, y, z), (x, y, z), t * 0.8, colour=TIMBER)
        m.segment((-x, -y, z), (-x, y, z), t * 0.8, colour=TIMBER)
        m.segment((x, -y, z), (x, y, z), t * 0.8, colour=TIMBER)
    for px in posts:
        for sy in (-1, 1):
            m.segment((px * x, sy * y, z0), (px * x, sy * y, z0 + h), t * 0.8, colour=TIMBER)
    if braces:
        for sx in (-1, 1):
            m.segment((sx * x, -y * 0.9, z0 + 0.1), (sx * x, -y * 0.2, z0 + h - 0.1), t * 0.7, colour=TIMBER)
            m.segment((sx * x, y * 0.9, z0 + 0.1), (sx * x, y * 0.2, z0 + h - 0.1), t * 0.7, colour=TIMBER)


def window(m, x, y, z, face="front", w=0.5, h=0.55):
    """A dark window with light shutters on a wall (front = -Y, or the X sides)."""
    if face == "front":
        m.box((w, 0.08, h), at=(x, y, z), colour=WINDOW)
        for s in (-1, 1):
            m.box((w * 0.45, 0.06, h * 1.02), at=(x + s * w * 0.75, y - 0.01, z), colour=SHUTTER)
    else:
        m.box((0.08, w, h), at=(x, y, z), colour=WINDOW)
        for s in (-1, 1):
            m.box((0.06, w * 0.45, h * 1.02), at=(x + (0.01 if x > 0 else -0.01), y + s * w * 0.75, z), colour=SHUTTER)


@recipe("buildings", "house", 81)
def house(m):
    """A timber-framed cottage, gable to the front, thatched."""
    m.box((3.7, 3.7, 0.3), colour=STONE_BASE, vary=0.06)
    m.box((3.4, 3.4, 1.6), at=(0, 0, 0.3), colour=PLASTER, vary=0.03)
    timber_walls(m, 3.4, 3.4, 0.3, 1.6)
    m.box((0.82, 0.12, 1.28), at=(-0.55, -1.7, 0.3), colour=DOOR)
    m.box((1.0, 0.1, 0.12), at=(-0.55, -1.73, 1.58), colour=TIMBER)
    window(m, 0.75, -1.72, 0.95)
    window(m, 1.72, -0.5, 0.95, face="side")
    window(m, -1.72, 0.3, 0.95, face="side")
    window(m, 0.0, -1.72, 2.25, w=0.4, h=0.42)  # loft window in the gable
    m.gable_roof(3.4, 3.4, 1.12, at=(0, 0, 1.9), colour=THATCH, overhang=0.28, thick=0.16, gable_colour=PLASTER,
                 vary=0.07, rot=90, rows=3)
    m.segment((0.0, -1.98, 3.0), (0.0, 1.98, 3.0), 0.09, colour=shade(THATCH, 0.72), sides=4)
    m.box((0.45, 0.45, 1.2), at=(0.85, 0.8, 1.85), colour=STONE_WALL, vary=0.05)
    m.box((0.52, 0.52, 0.08), at=(0.85, 0.8, 2.97), colour=STONE_BASE)
    for z, dy in ((0.12, 0.0), (0.12, 0.22), (0.31, 0.11)):  # firewood by the side wall
        log(m, (1.84, -1.2 + dy, z), (1.84, -0.35 + dy, z), 0.1)


@recipe("buildings", "town_center", 82)
def town_center(m):
    """A stone and timber hall with a tiled roof, a porch and a corner tower."""
    m.box((9.0, 9.0, 0.3), colour=STONE_BASE, vary=0.05)
    m.box((7.2, 6.4, 2.3), at=(0, 0.2, 0.3), colour=STONE_WALL, vary=0.06)
    m.box((7.4, 6.6, 1.5), at=(0, 0.2, 2.6), colour=PLASTER, vary=0.03)
    timber_walls(m, 7.4, 6.6, 2.6, 1.5, posts=(-0.5, 0.0, 0.5), braces=False)
    m.box((7.6, 6.8, 0.14), at=(0, 0.2, 2.55), colour=TIMBER)
    m.gable_roof(7.4, 6.6, 1.9, at=(0, 0.2, 4.1), colour=ROOF_TILE, overhang=0.4, thick=0.18,
                 gable_colour=PLASTER, vary=0.06, rows=4)
    m.segment((-4.1, 0.2, 6.0), (4.1, 0.2, 6.0), 0.12, colour=shade(ROOF_TILE, 0.7))
    # dormers on the front slope
    for x in (-2.2, 2.2):
        m.box((0.9, 1.1, 0.95), at=(x, -2.25, 4.15), colour=PLASTER)
        m.box((0.5, 0.08, 0.5), at=(x, -2.82, 4.35), colour=WINDOW)
        m.gable_roof(1.1, 0.9, 0.45, at=(x, -2.25, 5.1), colour=ROOF_TILE, overhang=0.12, thick=0.1, rot=90,
                     gable_colour=PLASTER)
    # doors and porch
    m.box((1.7, 0.14, 1.95), at=(0, -3.0, 0.3), colour=DOOR)
    m.box((0.08, 0.16, 1.95), at=(0, -3.04, 0.3), colour=TIMBER)
    for sx in (-1, 1):
        m.segment((sx * 1.1, -3.95, 0.3), (sx * 1.1, -3.95, 2.45), 0.1, colour=TIMBER)
    m.gable_roof(1.0, 2.2, 0.55, at=(0, -3.55, 2.45), colour=ROOF_TILE, overhang=0.15, thick=0.12, rot=90,
                 vary=0.06)
    # windows
    for x in (-2.4, 2.4):
        m.box((0.6, 0.1, 0.95), at=(x, -3.02, 0.95), colour=WINDOW)
    for x in (-2.5, -1.25, 1.25, 2.5):
        window(m, x, -3.14, 3.0, w=0.45, h=0.6)
    for y in (-1.6, 0.0, 1.6):
        window(m, 3.72, y + 0.2, 3.0, face="side", w=0.45, h=0.6)
        window(m, -3.72, y + 0.2, 3.0, face="side", w=0.45, h=0.6)
    # corner tower at the back right, with a pyramid roof
    m.box((2.5, 2.5, 4.7), at=(3.15, 3.15, 0.3), colour=STONE_WALL, vary=0.06)
    for sx in (-1, 1):
        for sy in (-1, 1):
            m.box((0.5, 0.5, 0.35), at=(3.15 + sx * 1.0, 3.15 + sy * 1.0, 5.0), colour=STONE_WALL)
    m.prism(4, 1.55, 1.05, at=(3.15, 3.15, 4.95), top=0, rot=45, colour=ROOF_TILE, cap_bottom=True, vary=0.05)
    m.box((0.45, 0.1, 0.8), at=(3.15, 1.88, 3.4), colour=WINDOW)
    m.box((0.1, 0.45, 0.8), at=(4.42, 3.15, 3.4), colour=WINDOW)
    # supplies by the door
    for k in range(3):
        m.prism(8, 0.28, 0.6, at=(-3.6 + 0.62 * k, -3.95, 0.3), colour=PLANK, vary=0.08)
    for k in range(2):
        m.box((0.55, 0.55, 0.5), at=(2.6 + 0.62 * k, -3.95, 0.3), colour=shade(PLANK, 0.85), rot=8 * k)


@recipe("buildings", "farm", 83)
def farm(m):
    """A tilled field: crop rows (green and ripe), a grass edge."""
    m.box((6.0, 6.0, 0.06), colour=SOIL, vary=0.04)
    rows = 9
    for i in range(rows):
        y = -2.55 + 5.1 * i / (rows - 1)
        colour = WHEAT if i >= 5 else CROP
        m.segment((-2.75, y, 0.05), (2.75, y, 0.05), 0.17, colour=SOIL_DARK, sides=3, spin=90)
        m.segment((-2.6, y, 0.13), (2.6, y, 0.13), 0.1, colour=colour, sides=4, vary=0.1)
        for j in range(11):
            x = -2.45 + 4.9 * j / 10 + m.rng.uniform(-0.08, 0.08)
            h = m.rng.uniform(0.12, 0.2)
            m.prism(4, 0.13, h, at=(x, y + m.rng.uniform(-0.03, 0.03), 0.15), top=0, rot=m.rng.uniform(0, 90),
                    colour=shade(colour, 1.08), vary=0.12)
    for x0, y0, x1, y1 in ((-2.96, -2.96, 2.96, -2.96), (-2.96, 2.96, 2.96, 2.96), (-2.96, -2.96, -2.96, 2.96),
                           (2.96, -2.96, 2.96, 2.96)):
        m.segment((x0, y0, 0.06), (x1, y1, 0.06), 0.07, colour=GRASS_EDGE, sides=4)


@recipe("buildings", "lumber_camp", 84)
def lumber_camp(m):
    """A woodcutters' yard: a plank shed, a big log pile, a chopping block and a sawhorse."""
    m.box((3.9, 3.9, 0.05), colour=DIRT, vary=0.05)
    for x in (-1.75, 0.55):
        for y in (0.45, 1.7):
            m.segment((x, y, 0.05), (x, y, 1.95), 0.08, colour=TIMBER)
    m.gable_roof(2.3, 1.25, 0.8, at=(-0.6, 1.08, 1.95), colour=shade(PLANK, 0.8), overhang=0.22, thick=0.1,
                 rows=2, vary=0.06)
    for k in range(5):  # sawn planks under the roof
        m.box((2.0, 0.42, 0.07), at=(-0.6, 1.3, 0.05 + 0.075 * k), colour=WOOD_CUT, rot=m.rng.uniform(-2, 2))
        m.box((2.0, 0.42, 0.07), at=(-0.6, 0.78, 0.05 + 0.075 * k), colour=shade(WOOD_CUT, 0.92),
              rot=m.rng.uniform(-2, 2))
    # log pile, front right: 4 + 3 + 2 + 1, cut ends toward the camera side (+X)
    for row, n in enumerate((4, 3, 2, 1)):
        for k in range(n):
            y = -1.75 + 0.34 * k + 0.17 * row
            z = 0.22 + 0.29 * row
            log(m, (0.15, y, z), (1.85, y, z), 0.17)
    # chopping block with an axe in it
    m.prism(8, 0.27, 0.48, at=(-1.2, -1.15, 0.05), colour=BARK, cap_top=False, vary=0.08)
    stump_top(m, 0.27, 0.53, 8, at=(-1.2, -1.15))
    m.segment((-1.2, -1.15, 0.58), (-0.82, -1.38, 1.15), 0.035, colour=PLANK, sides=4)
    m.box((0.1, 0.26, 0.17), at=(-1.14, -1.18, 0.5), colour=METAL)
    for k in range(4):  # split firewood
        a = m.rng.uniform(0, 360)
        m.box((0.3, 0.09, 0.09), at=(-0.85 + 0.12 * k, -0.62 - 0.08 * k, 0.05), rot=a, colour=WOOD_CUT)
    # sawhorse with a log
    for x in (-1.55, -0.7):
        m.segment((x, -0.3, 0.05), (x, 0.0, 0.75), 0.04, colour=TIMBER)
        m.segment((x, 0.3, 0.05), (x, 0.0, 0.75), 0.04, colour=TIMBER)
    log(m, (-1.85, 0.0, 0.86), (-0.35, 0.0, 0.86), 0.14)


@recipe("buildings", "mining_camp", 85)
def mining_camp(m):
    """A miners' yard: a slate shed, a winch frame, a cart on rails and ore heaps."""
    m.box((3.9, 3.9, 0.05), colour=GRAVEL, vary=0.06)
    lean_to(m, -1.75, 0.5, 0.45, 1.75, 2.15, 2.6, roof=SLATE)
    for k in range(3):  # crates under the shed
        m.box((0.5, 0.5, 0.45), at=(-1.3 + 0.6 * k, 1.25, 0.05), colour=shade(PLANK, 0.9), rot=6 * k)
    # winch frame (an A-frame with a wheel and a bucket) at the back right
    for y in (0.45, 1.55):
        m.segment((0.95, y, 0.05), (1.45, 1.0, 2.95), 0.07, colour=TIMBER)
        m.segment((1.85, y, 0.05), (1.45, 1.0, 2.95), 0.07, colour=TIMBER)
    m.segment((1.45, 1.0, 2.88), (1.45, -0.15, 2.88), 0.07, colour=TIMBER)
    m.prism(10, 0.24, 0.07, at=(1.45, -0.05, 2.62), tilt=(90, 0), colour=METAL)
    m.segment((1.45, -0.1, 2.55), (1.45, -0.1, 1.15), 0.015, colour="#3a3026", sides=3)
    m.prism(8, 0.2, 0.3, at=(1.45, -0.1, 0.85), top=0.24, colour=PLANK, cap_top=False)
    # mine cart on rails, front left
    for x in (-1.35, -0.75):
        m.box((0.07, 2.2, 0.05), at=(x, -0.95, 0.05), colour=METAL)
    for k in range(6):
        m.box((0.95, 0.14, 0.05), at=(-1.05, -1.9 + 0.38 * k, 0.04), colour=TIMBER)
    m.box((0.9, 0.7, 0.45), at=(-1.05, -1.15, 0.28), colour=shade(PLANK, 0.75), vary=0.05)
    m.box((0.95, 0.75, 0.06), at=(-1.05, -1.15, 0.7), colour=METAL)
    for sx in (-1, 1):
        for sy in (-1, 1):
            m.prism(8, 0.15, 0.06, at=(-1.05 + sx * 0.48, -1.15 + sy * 0.22, 0.2), tilt=(0, 90), colour=METAL)
    for k, colour in enumerate((COPPER, STONE, RUST, STONE, COPPER, RUST)):
        m.blob(0.16, at=(-1.3 + 0.1 * k, -1.15 + m.rng.uniform(-0.18, 0.18), 0.78), colour=colour, subdiv=1,
               jitter=0.2)
    # ore heaps, front right: stone, copper, iron
    for (cx, cy, r), colour in (((0.85, -1.3, 0.55), STONE), ((1.45, -0.35, 0.45), COPPER),
                                ((0.3, -0.35, 0.42), RUST)):
        m.blob(r, at=(cx, cy, 0.05), scale=(1.0, 1.0, 0.62), colour=shade(colour, 0.9), subdiv=2, jitter=0.15,
               flat_bottom=0.0, vary=0.08)
        for k in range(8):
            a, d = m.rng.uniform(0, math.tau), m.rng.uniform(0.0, 0.75)
            z = 0.05 + r * 0.62 * math.sqrt(max(0.0, 1 - d * d)) - 0.04
            m.blob(m.rng.uniform(0.1, 0.16), at=(cx + math.cos(a) * d * r, cy + math.sin(a) * d * r, z),
                   colour=colour if k % 3 else shade(colour, 1.15), subdiv=1, jitter=0.2)
    # pickaxe leaning on the shed post
    m.segment((-1.7, 0.2, 0.05), (-1.62, 0.4, 1.0), 0.03, colour=PLANK, sides=4)
    m.segment((-1.62, 0.22, 1.0), (-1.62, 0.6, 0.95), 0.035, colour=METAL, sides=4)


# ===========================================================================
# Fixtures: the tiniest models that still have every feature the engine
# reads (variants, a stump, a banner, a texture). A few KB each.
# ===========================================================================

def fixture_pine(m, tiers):
    m.prism(5, 0.13, 1.3, colour=BARK, cap_top=False)
    z, r, h = 0.9, 1.0, 2.6
    for i in range(tiers):
        m.prism(6, r, h, at=(0, 0, z), colour=PINE if i % 2 == 0 else PINE_LIGHT, top=0, cap_bottom=True, rot=30 * i)
        z, r, h = z + h * 0.55, r * 0.72, h * 0.85


def fx_pine_a(m):
    fixture_pine(m, 2)


def fx_pine_b(m):
    fixture_pine(m, 3)


def fx_pine_stump(m):
    m.prism(6, 0.17, 0.35, colour=BARK)
    m.prism(6, 0.15, 0.001, at=(0, 0, 0.35), colour=WOOD_CUT)


def fx_stone_outcrop(m):
    m.blob(0.9, at=(0.25, 0.1, 0.45), scale=(1.2, 1.0, 0.85), colour=STONE, flat_bottom=-0.45, jitter=0.15)
    m.blob(0.6, at=(-0.7, -0.2, 0.3), scale=(1.0, 1.1, 0.9), colour=STONE_DARK, flat_bottom=-0.3, jitter=0.15)


def fx_house(m):
    m.box((3.4, 3.4, 1.9), colour=PLASTER)
    m.box((0.8, 0.1, 1.3), at=(0, -1.72, 0), colour=TIMBER)
    m.gable_roof(3.4, 3.4, 1.1, at=(0, 0, 1.9), colour=ROOF_TILE, overhang=0.3, gable_colour=PLASTER)


FIXTURE_RECIPES = {
    ("nodes", "pine_a"): fx_pine_a,
    ("nodes", "pine_b"): fx_pine_b,
    ("nodes", "pine_stump"): fx_pine_stump,
    ("nodes", "stone_outcrop"): fx_stone_outcrop,
    ("buildings", "house"): fx_house,
}


# ===========================================================================

def write_sources(recipes, out_dir, only=None):
    """Model every recipe and export it as `<out_dir>/<kind>/<name>.glb`."""
    written = []
    for (kind, name), spec in sorted(recipes.items()):
        if only and name not in only:
            continue
        fn, seed = spec if isinstance(spec, tuple) else (spec, 0)
        scene.reset()
        m = Model(name, seed=seed)
        fn(m)
        obj = m.finish()
        path = os.path.join(out_dir, kind, name + ".glb")
        gltf.export(path, [obj])
        written.append(path)
        print(f"  {kind}/{name}.glb  {scene.triangles(obj)} tris  {os.path.getsize(path) / 1e3:.1f} kB")
    return written


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    ap = argparse.ArgumentParser(prog="placeholders.py")
    ap.add_argument("--only", default=None)
    ap.add_argument("--build", action="store_true")
    ap.add_argument("--sheets", default=None)
    ap.add_argument("--fixtures", action="store_true")
    return ap.parse_args(argv)


def main():
    args = parse_args()
    import build_static
    if args.fixtures:
        tmp = tempfile.mkdtemp(prefix="af_static_fixtures_")
        try:
            write_sources(FIXTURE_RECIPES, tmp)
            # No settings file: the contract's defaults, which the engine's
            # tests assume, whatever art/static/assets.toml says.
            failed = build_static.run(build_static.parse_args([
                "--source", tmp, "--out-root", FIXTURES, "--asset-root", FIXTURES,
                "--registry", os.path.join(FIXTURES, "static.ron"), "--settings", os.devnull, "--no-sheets"]))
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
        sys.exit(1 if failed else 0)
    only = {n.strip() for n in args.only.split(",")} if args.only else None
    if only:
        unknown = only - {name for _, name in RECIPES}
        if unknown:
            raise SystemExit(f"--only: no recipe for {sorted(unknown)}")
    print(f"writing placeholder sources to {SOURCE}")
    write_sources(RECIPES, SOURCE, only)
    if args.build:
        argv = ["--source", SOURCE, "--out-root", os.path.join(REPO, "assets", "models", "_placeholders"),
                "--registry", os.path.join(REPO, "assets", "visuals-placeholders", "static.ron"),
                "--settings", SETTINGS]
        if args.sheets:
            argv += ["--sheets", args.sheets]
        sys.exit(1 if build_static.run(build_static.parse_args(argv)) else 0)


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception:
        import traceback
        traceback.print_exc()
        sys.exit(1)
