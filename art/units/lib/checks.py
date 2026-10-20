"""Automatic checks and measurements on built clips (printed by the build).

From the solved frames (`measure`):

* loop seam: first vs last frame (looping clips must match exactly);
* in place: the pelvis's horizontal travel;
* feet: how far a planted foot slides (work clips) or, for locomotion, the
  ground speed: the planted foot's backward travel under the in-place body;
* IK: how far any IK target was out of reach;
* wrists: the bend of both hands against their forearms on every frame,
  FK and IK arms alike, split into flexion (palm side +) and deviation
  (thumb side +), against human ranges;
* strike: the speed of the held tool's striking part (props.STRIKES) as it
  arrives at HIT, against the swing's peak (a blow must not slow down
  before it lands);
* hitches: a bone (or the tool's striking part) that slows almost to a stop
  mid-motion and carries on the same way: a velocity reset between keys
  (a key that restarts from rest, a body key that stops dead). Turning
  points (the motion reverses) are not hitches, and neither are the
  pauses a clip declares in BEATS.

From the skinned body mesh (`mesh_profile`):

* ground: the lowest body point per frame (nothing below the floor);
* arms: how close the forearm and hand axes come to the torso and head
  surface (negative: the axis is inside, i.e. the arm passes through);
* props: how deep a held prop goes into the body, away from the fists.

Lines starting with "  !" are warnings: look at the sheet, then fix the
clip or accept it knowingly.
"""

import math

import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from .clip import assign
from .props import prop_info
from .rig import ARM, CHEST, HAND, HEAD, HEADFRONT, HIPS, NECK, SIDES, SPINE, mat4
from .source import FPS

CONTACT = 0.01  # a sole this close to the floor (metres) is planted

# Wrist ranges (degrees) beyond which a pose reads as broken.
WRIST_RADIAL = 25.0
WRIST_ULNAR = 40.0
WRIST_TOTAL = 65.0

STRIKE_KEEP = 0.6  # the strike must arrive with at least this share of its peak speed
STRIKE_WINDOW = 8  # frames before HIT searched for the swing's peak

HITCH_BONES = (HIPS, CHEST, HEAD) + tuple(ARM[s][i] for s in SIDES for i in (1, 2, 3))
HITCH_BONE_SPEED = 4.0  # deg/frame on both sides of a dip for it to count
HITCH_POINT_SPEED = 0.05  # m/frame (1.5 m/s), for the tool's striking part
HITCH_DIP = 0.45  # a dip slows below this share of the speed on both sides
HITCH_WINDOW = 3  # frames searched on each side of a dip
BEAT_SLACK = 2  # frames: a dip this close to a declared BEAT is deliberate

# The forearm and hand axes are sampled here (fractions from the elbow to
# the wrist, and metres along the hand to the middle of the fist).
FOREARM_SAMPLES = (0.3, 0.6, 0.9)
HAND_SAMPLES = (0.0, 0.085)
CORE_BONES = {HIPS, NECK, HEAD, HEADFRONT, *SPINE}
CLEAR_RANGE = 0.25  # metres: clearances beyond this are not reported
FIST_CLEARANCE = 0.12  # metres around each fist where a held prop may pass through the hand
PROP_RANGE = 0.15  # metres: deeper than this inside the body is not searched
PROP_SINK = 0.01  # metres a prop may sink into the body (cloth, skin) before it is a warning
PARITY_RAYS = tuple(Vector(d).normalized() for d in ((0, 0, 1), (0.3, 0.9, 0.1), (-0.7, 0.2, -0.6),
                                                     (0.9, -0.3, 0.2), (-0.2, -0.9, 0.3)))


def measure(clip):
    rig = clip.ctx.rig
    fk = [rig.fk(b) for b in clip.basis]
    out = {"name": clip.name, "frames": clip.frames, "duration": clip.duration, "loop": clip.loop}
    if clip.loop:
        a, b = clip.basis[0], clip.basis[-1]
        out["seam_deg"] = max(math.degrees(a[n][1].rotation_difference(b[n][1]).angle) for n in a)
        out["seam_m"] = max((a[n][0] - b[n][0]).length for n in a) * rig.scale
    hips = [f[HIPS][0] for f in fk]
    out["hips_travel"] = max(math.hypot(p.x, p.y) for p in hips)
    feet = {}
    for s in SIDES:
        soles = [rig.sole_points(f, s) for f in fk]
        lows = [min(p.z for p in pts) for pts in soles]
        centres = [sum(pts, pts[0] * 0) / len(pts) for pts in soles]
        planted = [z <= CONTACT for z in lows]
        runs = _runs(planted, clip.loop)
        slide, travel = 0.0, []
        for run in runs:
            c0 = centres[run[0]]
            slide = max(slide, max(math.hypot(centres[i].x - c0.x, centres[i].y - c0.y) for i in run))
            if len(run) > 2:
                # Ground speed: the planted foot's backward travel under the body.
                travel.append(((centres[run[-1]].y - c0.y) * FPS / (len(run) - 1), len(run) - 1))
        feet[s] = dict(ground=min(lows), contact=sum(planted), slide=slide, travel=travel)
    out["feet"] = feet
    if clip.locomotion:
        runs = [t for s in SIDES for t in feet[s]["travel"]]
        weight = sum(n for _, n in runs)
        out["speed"] = sum(v * n for v, n in runs) / weight if weight else None
        out["speed_by_foot"] = {s: [round(v, 2) for v, _ in feet[s]["travel"]] for s in SIDES}
    errs = {}
    for info in clip.info:
        for k, v in info.items():
            if not k.startswith("wrist"):
                errs[k] = max(errs.get(k, 0.0), v)
    out["ik"] = errs
    out["wrists"] = {s: _wrist_range([wrist_bend(f, s) for f in fk]) for s in SIDES}
    tracks = _strike_tracks(clip, fk)
    out["strike"] = [_strike(clip, name, part, pts) for name, part, pts in tracks] if clip.hit is not None else []
    out["hitches"] = _all_hitches(clip, fk, tracks)
    out["beats"] = [round(t * FPS) for t in clip.beats]
    return out


def _runs(flags, loop):
    """Index runs of consecutive True flags (joined across the seam for loops,
    where the last frame repeats the first)."""
    n = len(flags) - 1 if loop else len(flags)
    runs, cur = [], []
    for i in range(n):
        if flags[i]:
            cur.append(i)
        elif cur:
            runs.append(cur)
            cur = []
    if cur:
        if loop and runs and runs[0][0] == 0:
            runs[0] = cur + [n + i for i in runs[0]]
        else:
            runs.append(cur)
    return [[i % n for i in r] for r in runs]


# -- wrists -------------------------------------------------------------------


def wrist_bend(fk, side):
    """(flex, deviation, total) degrees of a hand against its forearm in an
    FK result: flex + bends to the palm side, deviation + to the thumb side
    (radial), - to the little finger (ulnar); total is the angle between the
    two axes (twist excluded)."""
    fore, hand = fk[ARM[side][2]][1], fk[ARM[side][3]][1]
    v = hand.transposed() @ fore.col[1]   # the forearm's axis in hand space
    palm = 1.0 if side == "R" else -1.0   # the palm faces hand +X (right) / -X (left); the thumb -Z
    flex = math.degrees(math.atan2(-palm * v.x, v.y))
    deviation = math.degrees(math.atan2(v.z, v.y))
    total = math.degrees(fore.col[1].angle(hand.col[1]))
    return flex, deviation, total


def _wrist_range(bends):
    worst = max(range(len(bends)), key=lambda i: bends[i][2])
    return dict(flex=(min(b[0] for b in bends), max(b[0] for b in bends)),
                deviation=(min(b[1] for b in bends), max(b[1] for b in bends)),
                total=bends[worst][2], frame=worst)


# -- the tool's striking part -------------------------------------------------


def _strike_tracks(clip, fk):
    """(prop, part, world positions per frame) of each held tool's striking part."""
    tracks = []
    for name in clip.props:
        prop = prop_info(name)
        if prop.strike is None:
            continue
        bone, local = HAND[prop.hand], prop.in_hand @ prop.part(prop.strike)
        tracks.append((name, prop.strike, [mat4(*f[bone]) @ local for f in fk]))
    return tracks


def _strike(clip, name, part, pts):
    hit = round(clip.hit * FPS)
    speed = [0.0] + [(pts[i] - pts[i - 1]).length * FPS for i in range(1, len(pts))]
    window = range(max(1, hit - STRIKE_WINDOW), hit + 1)
    peak = max(window, key=lambda i: speed[i])
    return dict(prop=name, part=part, hit=hit, arrive=speed[hit] if hit < len(speed) else 0.0,
                peak=speed[peak], peak_frame=peak)


# -- hitches ------------------------------------------------------------------


def _all_hitches(clip, fk, tracks):
    found = []
    for bone in HITCH_BONES:
        vel = [None] + [_rotvec(fk[i - 1][bone][1], fk[i][bone][1]) for i in range(1, len(fk))]
        found += [(bone.replace("mixamorig:", ""), "deg/f") + h for h in hitches(vel, HITCH_BONE_SPEED, clip.loop)]
    for name, part, pts in tracks:
        vel = [None] + [pts[i] - pts[i - 1] for i in range(1, len(pts))]
        found += [(f"{name} {part}", "m/f") + h for h in hitches(vel, HITCH_POINT_SPEED, clip.loop)]
    return sorted(found, key=lambda h: h[2])


def _rotvec(a, b):
    """World rotation from `a` to `b` as a rotation vector (degrees)."""
    q = (b @ a.transposed()).to_quaternion()
    if q.w < 0.0:
        q.negate()
    return q.axis * math.degrees(q.angle)


def hitches(vel, threshold, loop):
    """Dips in a motion: frames where the speed falls below HITCH_DIP of
    the speed on both sides (each at least `threshold`) while the motion
    carries on in the same direction (within 60 degrees). `vel[i]` is the
    velocity from frame i-1 to i (vel[0] unused). Returns (frame, speed
    before, speed at the dip, speed after) tuples."""
    n = len(vel) - 1
    if n < 2 * HITCH_WINDOW + 1:
        return []

    def at(i):
        if loop:
            return vel[(i - 1) % n + 1]
        return vel[i] if 1 <= i <= n else None

    out = []
    first, last = (1, n) if loop else (1 + HITCH_WINDOW, n - HITCH_WINDOW)
    for i in range(first, last + 1):
        v = at(i)
        s = v.length
        if s > at(i - 1).length or s > at(i + 1).length:
            continue
        vb = max((at(i - k) for k in range(1, HITCH_WINDOW + 1)), key=lambda x: x.length)
        va = max((at(i + k) for k in range(1, HITCH_WINDOW + 1)), key=lambda x: x.length)
        sb, sa = vb.length, va.length
        if min(sb, sa) < threshold or s > HITCH_DIP * min(sb, sa):
            continue
        if vb.dot(va) < 0.5 * sb * sa:
            continue  # a turning point, not a stall
        out.append(((i - 1) % n + 1 if loop else i, sb, s, sa))
    return out


# -- the skinned mesh -----------------------------------------------------------


class BodyProbe:
    """The body mesh's torso and head faces (every vertex dominated by a
    pelvis, spine, neck or head bone), for arm clearance tests."""

    def __init__(self, body):
        self.body = body
        groups = {g.index: g.name for g in body.vertex_groups}
        dom = []
        for v in body.data.vertices:
            g = max(v.groups, key=lambda g: g.weight, default=None)
            dom.append(groups[g.group] if g else "")
        self.dominant = dom
        self.polys = [tuple(p.vertices) for p in body.data.polygons]
        self.core = [p for p in self.polys if all(dom[i] in CORE_BONES for i in p)]

    def world_verts(self):
        ev = self.body.evaluated_get(bpy.context.evaluated_depsgraph_get())
        n = len(ev.data.vertices)
        co = np.empty(n * 3, np.float32)
        ev.data.vertices.foreach_get("co", co)
        m = np.array(ev.matrix_world)
        return co.reshape(n, 3) @ m[:3, :3].T + m[:3, 3]


def _arm_samples(fk, side):
    _, _, fore, hand = ARM[side]
    elbow, wrist = fk[fore][0], fk[hand][0]
    pts = [("ForeArm", elbow.lerp(wrist, t)) for t in FOREARM_SAMPLES]
    return pts + [("Hand", wrist + fk[hand][1].col[1] * d) for d in HAND_SAMPLES]


def _inside(tree, p):
    """Is `p` inside the torso/head surface? That surface is open (at the
    neck, shoulders and hips), so a single ray can escape through a hole:
    count crossings along several rays and take a clear majority."""
    votes = 0
    for d in PARITY_RAYS:
        crossings, o = 0, p.copy()
        for _ in range(64):
            hit, _, _, _ = tree.ray_cast(o, d, 3.0)
            if hit is None:
                break
            crossings += 1
            o = hit + d * 1e-4
        votes += crossings % 2
    return votes >= len(PARITY_RAYS) - 1


PROP_SAMPLES = 2000  # a modelled prop (Meshy's) is sampled at about this many of its vertices


def _prop_samples(obj, per_edge=6):
    """Points along every edge of a prop's boxes, in the prop's local space
    (a modelled prop, with many more edges: a spread of its vertices)."""
    me = obj.data
    if len(me.edges) * (per_edge + 1) > 4 * PROP_SAMPLES:
        step = max(1, len(me.vertices) // PROP_SAMPLES)
        return [me.vertices[i].co.copy() for i in range(0, len(me.vertices), step)]
    pts = []
    for e in me.edges:
        a, b = (me.vertices[i].co for i in e.vertices)
        pts += [a.lerp(b, k / per_edge) for k in range(per_edge + 1)]
    return pts


def mesh_profile(clip, arm, probe, prop_objs):
    """Per frame, with the clip's action on the armature: the body mesh's
    lowest point (world z); the forearm/hand axes' signed clearance from
    the torso and head surface; and how deep any held prop goes into the
    body (away from the fists, which the hafts pass through: the hands have
    no fingers). Returns a dict for `report`."""
    scene = bpy.context.scene
    rig = clip.ctx.rig
    assign(arm, clip.action)
    # Every prop the clip may show (SHEET_PROPS: other loads on the same grip),
    # while it is shown (PROPS_UNTIL: the engine hides it after that).
    names = dict.fromkeys(tuple(clip.props) + tuple(getattr(clip.module, "SHEET_PROPS", ())))
    shown_until = getattr(clip.module, "PROPS_UNTIL", None)
    held = {name: (prop_objs[name], _prop_samples(prop_objs[name])) for name in names}
    lows, where = [], []
    clear = {s: (math.inf, None, None) for s in SIDES}
    sunk = (0.0, None, None)
    for f in range(clip.frames + 1):
        scene.frame_set(f)
        w = probe.world_verts()
        i = int(w[:, 2].argmin())
        lows.append(float(w[i, 2]))
        where.append(i)
        verts = w.tolist()
        core = BVHTree.FromPolygons(verts, probe.core)
        fk = rig.fk(clip.basis[f])
        for s in SIDES:
            for part, p in _arm_samples(fk, s):
                loc, normal, idx, dist = core.find_nearest(p, CLEAR_RANGE)
                if loc is None:
                    continue
                inside = (p - loc).dot(normal) < 0.0 and _inside(core, p)
                signed = -dist if inside else dist
                if signed < clear[s][0]:
                    region = probe.dominant[probe.core[idx][0]].replace("mixamorig:", "")
                    clear[s] = (signed, f, f"{part} by {region}")
        if not held or (shown_until is not None and f > shown_until * FPS + 1e-6):
            continue
        whole = BVHTree.FromPolygons(verts, probe.polys)
        fists = [fk[HAND[s]][0] + fk[HAND[s]][1].col[1] * HAND_SAMPLES[-1] for s in SIDES]
        for name, (obj, local) in held.items():
            m = obj.matrix_world
            for q in local:
                p = m @ q
                if min((p - c).length for c in fists) < FIST_CLEARANCE:
                    continue
                loc, normal, idx, dist = whole.find_nearest(p, PROP_RANGE)
                if loc is None or dist <= sunk[0] or (p - loc).dot(normal) >= 0.0 or not _inside(whole, p):
                    continue
                region = probe.dominant[probe.polys[idx][0]].replace("mixamorig:", "")
                sunk = (dist, f, f"{name} in {region}")
    worst = int(np.argmin(lows))
    groups = {g.index: g.name for g in probe.body.vertex_groups}
    v = probe.body.data.vertices[where[worst]]
    bone = groups[max(v.groups, key=lambda g: g.weight).group] if v.groups else "?"
    return dict(lows=lows, worst=(worst, bone.replace("mixamorig:", "")), clearance=clear,
                props_in_body=sunk if held else None)


# -- report ---------------------------------------------------------------------


def report(m, mesh=None):
    lines = [f"[{m['name']}] {m['frames']} frames, {m['duration']:.4f}s, {'loop' if m['loop'] else 'once'}"]

    def warn(text):
        lines.append("  ! " + text)

    if m["loop"]:
        lines.append(f"  seam: {m['seam_deg']:.4f} deg, {m['seam_m'] * 1000:.2f} mm")
    lines.append(f"  pelvis horizontal offset max: {m['hips_travel'] * 100:.1f} cm")
    for s, f in m["feet"].items():
        lines.append(f"  foot {s}: lowest sole {f['ground'] * 100:.1f} cm, planted {f['contact']} frames, "
                     f"slide {f['slide'] * 100:.1f} cm")
    if "speed" in m:
        lines.append(f"  ground speed: {m['speed']:.3f} m/s (stride {m['speed'] * m['duration']:.3f} m per cycle;"
                     f" per contact {m['speed_by_foot']})" if m["speed"] else "  ground speed: n/a")
    if m["ik"]:
        lines.append("  ik reach error: " + ", ".join(f"{k} {v * 100:.1f} cm" for k, v in sorted(m["ik"].items())))
    for s, w in m["wrists"].items():
        lines.append(f"  wrist {s}: flex {w['flex'][0]:.0f}..{w['flex'][1]:.0f}, deviation {w['deviation'][0]:.0f}"
                     f"..{w['deviation'][1]:.0f}, worst {w['total']:.0f} deg (frame {w['frame']})")
        past = [f"{what} {value:.0f} deg (range {limit:.0f})" for what, value, limit in
                (("radial", w["deviation"][1], WRIST_RADIAL), ("ulnar", -w["deviation"][0], WRIST_ULNAR),
                 ("bend", w["total"], WRIST_TOTAL)) if value > limit]
        if past:
            warn(f"wrist {s} bends past the human range: " + ", ".join(past))
    for st in m["strike"]:
        lines.append(f"  strike {st['prop']} {st['part']}: {st['arrive']:.1f} m/s into HIT (frame {st['hit']}),"
                     f" swing peak {st['peak']:.1f} m/s (frame {st['peak_frame']})")
        if st["arrive"] < STRIKE_KEEP * st["peak"]:
            warn(f"the blow slows before it lands ({st['arrive'] / max(st['peak'], 1e-9):.0%} of its peak)")
    beats = [h for h in m["hitches"] if any(abs(h[2] - b) <= BEAT_SLACK for b in m["beats"])]
    if beats:
        frames = ", ".join(str(f) for f in sorted({h[2] for h in beats}))
        lines.append(f"  deliberate beats (BEATS) around frame {frames}: {len(beats)} pausing bone(s)")
    hitches = [h for h in m["hitches"] if h not in beats]
    for what, unit, f, sb, s, sa in hitches[:4]:
        scale = 1.0 if unit == "deg/f" else 100.0
        u = unit if unit == "deg/f" else "cm/f"
        warn(f"hitch: {what} {sb * scale:.1f} -> {s * scale:.1f} -> {sa * scale:.1f} {u} around frame {f}")
    if len(hitches) > 4:
        lines.append(f"    (+{len(hitches) - 4} more hitches)")
    if mesh is not None:
        lows, (worst, bone) = mesh["lows"], mesh["worst"]
        lines.append(f"  body lowest point: min {min(lows) * 100:.1f} cm (frame {worst}, {bone}), "
                     f"first {lows[0] * 100:.1f} cm, last {lows[-1] * 100:.1f} cm")
        for s, (d, f, where) in mesh["clearance"].items():
            if f is None:
                lines.append(f"  arm {s} axis vs torso/head: always more than {CLEAR_RANGE * 100:.0f} cm away")
                continue
            lines.append(f"  arm {s} axis vs torso/head: min {d * 100:.1f} cm (frame {f}, {where})")
            if d < 0.0:
                warn(f"arm {s} passes into the body ({-d * 100:.1f} cm)")
        if mesh["props_in_body"] is not None:
            d, f, where = mesh["props_in_body"]
            if f is None:
                lines.append("  props in the body: none (away from the fists)")
            else:
                lines.append(f"  props in the body: max {d * 100:.1f} cm (frame {f}, {where})")
                if d > PROP_SINK:
                    warn(f"a prop goes {d * 100:.1f} cm into the body")
    return "\n".join(lines)
