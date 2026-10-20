"""Build a unit's model: its Meshy source -> GLB + manifest for the game.

Run headless from the repository root:

    "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b \
        -P art/units/build_unit.py -- --unit peasant [options]

Options (after the `--`):
    --unit ID[,ID...]    the unit(s) to build: art/units/<ID>/unit.py; `all`
                         builds every configured unit
    --only Walk,Attack   build (and render) only these clips (plus what they
                         depend on). Nothing is exported unless --out is given.
    --out DIR            where <ID>.glb and <ID>.model.ron go
                         (default assets/models/<ID>/)
    --source PATH        build from this file instead of the unit's SOURCE
                         (a stand-in, or a new download to try; use --out)
    --props DIR          look for Meshy prop models (art/units/meshy_props.py)
                         in DIR instead of art/units/source/props/
    --sheets DIR         render a contact sheet per clip into DIR (into
                         DIR/<ID>/ when building several units)
    --calibrate DIR      render the pose-helper calibration sheets into DIR
    --check-retarget     also check that clips read from other files
                         (Mixamo FBX, lib/retarget.py) come through exactly
    --mixamo-fbx PATH    write the character (body + skeleton, rest pose, no
                         props) as an FBX to upload to Mixamo, and stop
    --no-export          build and check only
    --list               list the configured units and stop

A unit is configured by art/units/<ID>/unit.py (lib/config.py documents every
setting). Humanoid units get the shared clips in art/units/clips/ plus
their own in art/units/<ID>/clips/ (see lib/clip.py for the module
contract); rigid units are built by lib/rigid.py. How to add a unit:
art/units/README.md.
"""

import argparse
import os
import sys
import time

sys.dont_write_bytecode = True  # keep __pycache__ out of the repository
HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import bpy  # noqa: E402
from mathutils import Vector  # noqa: E402

from lib import checks, export, props, selftest, sheets, source  # noqa: E402
from lib.clip import Context  # noqa: E402
from lib.rig import Rig, at  # noqa: E402
from lib.config import Unit, unit_ids  # noqa: E402


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    ap = argparse.ArgumentParser(prog="build_unit.py")
    ap.add_argument("--unit", default=None)
    ap.add_argument("--only", default=None)
    ap.add_argument("--out", default=None)
    ap.add_argument("--source", default=None)
    ap.add_argument("--props", default=None)
    ap.add_argument("--sheets", default=None)
    ap.add_argument("--calibrate", default=None)
    ap.add_argument("--check-retarget", action="store_true")
    ap.add_argument("--mixamo-fbx", default=None)
    ap.add_argument("--no-export", action="store_true")
    ap.add_argument("--list", action="store_true")
    return ap.parse_args(argv)


def model_path(unit_id):
    """The GLB's asset path, as the engine's asset server sees it."""
    return f"models/{unit_id}/{unit_id}.glb"


def writer(unit_id):
    return f"art/units/build_unit.py --unit {unit_id}"


def _box_centre(obj):
    """World bounding-box centre of a mesh object's vertices."""
    pts = [obj.matrix_world @ v.co for v in obj.data.vertices]
    return tuple((min(p[i] for p in pts) + max(p[i] for p in pts)) / 2 for i in range(3))


def body_height(body):
    import numpy as np
    dg = bpy.context.evaluated_depsgraph_get()
    ev = body.evaluated_get(dg)
    n = len(ev.data.vertices)
    co = np.empty(n * 3, np.float32)
    ev.data.vertices.foreach_get("co", co)
    m = np.array(ev.matrix_world)
    z = co.reshape(n, 3) @ m[2, :3] + m[2, 3]
    return float(z.min()), float(z.max())


def _point(p):
    """(right, fwd, up) -> world vector (no negative zeros: the peasant's
    sash is fitted exactly as before)."""
    v = at(*p)
    return Vector(tuple(x + 0.0 for x in v))


def sash_points(unit, rig):
    if "shoulder" in unit.sash:
        return _point(unit.sash["shoulder"]), _point(unit.sash["hip"])
    return props.sash_anchors(rig)


def build_humanoid(unit, args, src, out_dir, sheets_dir):
    t0 = time.time()
    for note in unit.prop_notes:
        print(note)
    arm, body, source_clips, notes = source.load(src, unit.texture_name, unit.body_material, unit.texture)
    for note in notes:
        print(note)
    source_files = unit.source_files(src)
    for name in notes.retarget:  # the source's own clip, read by retargeting
        source_files[name] = [(src, name)] + source_files.get(name, [])
    if args.mixamo_fbx:
        export.export_fbx_for_mixamo(args.mixamo_fbx, arm, body)
        print(f"wrote {args.mixamo_fbx}")
        return
    rig = Rig(arm)
    rig.attach_soles(body)
    selftest.run(rig)
    lo, hi = body_height(body)
    print(f"rest pose: body z {lo:.4f} .. {hi:.4f} m (height {hi - lo:.3f} m)")
    prop_objs = props.build_props(arm, rig)
    markers = []
    if unit.sash is not None:
        shoulder, hip = sash_points(unit, rig)
        if "shoulder" not in unit.sash:
            print(f"sash from the skeleton: shoulder ({-shoulder.x:.3f}, {-shoulder.y:.3f}, {shoulder.z:.3f}), "
                  f"hip ({-hip.x:.3f}, {-hip.y:.3f}, {hip.z:.3f}) m (right, fwd, up)")
        props.build_sash(body, arm, shoulder, hip, unit.sash.get("width", props.SASH_WIDTH))
        markers.append("Team_sash")

    modules = unit.clip_modules()
    public = sorted(n for n in modules if ":" not in n)
    if args.only:
        wanted = [n.strip() for n in args.only.split(",") if n.strip()]
        unknown = [n for n in wanted if n not in public]
        if unknown:
            raise SystemExit(f"{unit.id}: unknown clips {unknown}; have {public}")
    else:
        wanted = public
    ctx = Context(rig, arm, source_clips, modules, source_files=source_files)
    for name in wanted:
        ctx.built(name)
    clips = [ctx.clips[n] for n in wanted]
    for clip in ctx.clips.values():
        clip.bake(arm)

    print()
    probe = checks.BodyProbe(body)
    for clip in clips:
        m = checks.measure(clip)
        if clip.locomotion:
            clip.speed = m.get("speed")
        print(checks.report(m, checks.mesh_profile(clip, arm, probe, prop_objs)))
    if args.check_retarget:
        from lib import retarget
        for name in retarget_probes(ctx):
            rot, loc = retarget.round_trip(ctx, arm, body, name)
            print(f"retarget round trip via FBX, {name}: worst rotation {rot:.4f} deg, pelvis {loc * 1000:.2f} mm")
            if rot > 0.05 or loc > 0.001:
                raise SystemExit("retarget round trip failed")
    arm.animation_data.action = None
    for pb in arm.pose.bones:
        pb.location = (0, 0, 0)
        pb.rotation_quaternion = (1, 0, 0, 0)
    bpy.context.view_layer.update()

    if out_dir and not args.no_export:
        # Only the requested clips go into the file.
        for clip in ctx.clips.values():
            if clip not in clips:
                bpy.data.actions.remove(clip.action)
        glb = os.path.join(out_dir, f"{unit.id}.glb")
        rest = {n: _box_centre(o) for n, o in prop_objs.items()}
        export.export_glb(glb)
        s = export.verify(glb, clips, props.all_prop_names(), rig.order, props.emissive_materials(), rest,
                          texture_size=unit.texture, markers=markers)
        export.write_manifest(os.path.join(out_dir, f"{unit.id}.model.ron"), model_path(unit.id), hi - lo, clips,
                              writer(unit.id))
        print(f"\nwrote {glb} ({os.path.getsize(glb) / 1e6:.2f} MB)")
        for name, a in sorted(s["animations"].items()):
            print(f"  animation {name:12s} {a['duration']:.4f}s  {a['channels']} channels on {a['nodes']} nodes")
        print("  images:", ", ".join(f"{i['name']} {i['mime']} {i['size'][0]}x{i['size'][1]} {i['bytes'] / 1e3:.0f} kB"
                                     for i in s["images"]))
    elif args.only:
        print("\n--only without --out: nothing exported")

    if sheets_dir or args.calibrate:
        rig_scene = sheets.SheetRig(bpy.context.scene)
        scratch = os.path.join(sheets_dir or args.calibrate, "_tiles")
        if sheets_dir:
            for clip in clips:
                out = os.path.join(sheets_dir, f"{clip.name}.png")
                sheets.render_sheet(arm, body, prop_objs, clip, out, scratch, rig_scene)
                print("sheet", out)
        if args.calibrate:
            from lib import calibrate
            shown = dict(prop_objs)
            shown.update(props.build_props(arm, rig, [n for n in calibrate.prop_names() if n not in prop_objs]))
            for out in calibrate.render(ctx, arm, body, shown, args.calibrate, scratch, rig_scene):
                print("calibration", out)
        try:
            os.rmdir(scratch)
        except OSError:
            pass
    print(f"\n{unit.id} done in {time.time() - t0:.1f}s")


def retarget_probes(ctx):
    """A one-shot clip and a loop to send through the FBX round trip: Cheer
    and Chop where the unit has them, else its first of each kind that is
    keyed (not read from a file)."""
    keyed = [c for c in ctx.clips.values() if c._source is None and ":" not in c.name]
    once = "Cheer" if "Cheer" in ctx.clips else next((c.name for c in keyed if not c.loop), None)
    loop = "Chop" if "Chop" in ctx.clips else next((c.name for c in keyed if c.loop), None)
    return [n for n in (once, loop) if n]


def main():
    args = parse_args()
    if args.list:
        for uid in unit_ids():
            u = Unit(uid)
            print(f"{uid:16s} {u.kind:9s} {os.path.relpath(u.source, REPO)}  {u.doc}")
        return
    if not args.unit:
        raise SystemExit(f"--unit is required (configured: {', '.join(unit_ids())})")
    ids = unit_ids() if args.unit == "all" else [u.strip() for u in args.unit.split(",") if u.strip()]
    if args.source and len(ids) != 1:
        raise SystemExit("--source needs exactly one --unit")
    failed = []
    for uid in ids:
        try:
            unit = Unit(uid, args.props and os.path.abspath(args.props))
            src = os.path.abspath(args.source) if args.source else unit.source
            if len(ids) > 1 and not os.path.isfile(src):
                print(f"=== {uid}: no source yet ({os.path.relpath(src, REPO)}), skipped")
                continue
            out_dir = args.out or (None if args.only else os.path.join(REPO, "assets", "models", uid))
            sheets_dir = args.sheets and (os.path.join(args.sheets, uid) if len(ids) > 1 else args.sheets)
            print(f"=== {uid} ({unit.kind}) from {src}")
            if unit.kind == "rigid":
                from lib import rigid
                rigid.build(unit, src, None if args.no_export else out_dir, sheets_dir, model_path(uid), writer(uid))
            else:
                build_humanoid(unit, args, src, out_dir, sheets_dir)
        except Exception as e:  # one unit's failure does not stop the others
            if len(ids) == 1:
                raise
            print(f"\n{uid} FAILED: {type(e).__name__}: {e}")
            failed.append(uid)
    if failed:
        raise SystemExit(f"failed: {', '.join(failed)}")


if __name__ == "__main__":
    try:
        main()
    except Exception:
        import traceback
        traceback.print_exc()
        sys.exit(1)
