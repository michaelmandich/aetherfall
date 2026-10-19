"""Build the peasant model: source -> cleaned rig + props + every clip -> GLB.

Run headless from the repository root:

    "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b \
        -P art/peasant/build_peasant.py -- [options]

Options (after the `--`):
    --only Walk,Attack   build (and render) only these clips (plus what they
                         depend on). Nothing is exported unless --out is given.
    --out DIR            where peasant.glb and peasant.model.ron go
                         (default assets/models/peasant)
    --sheets DIR         render a contact sheet per clip into DIR
    --calibrate DIR      render the pose-helper calibration sheets into DIR
    --check-retarget     also check that clips read from other files
                         (Mixamo FBX, lib/retarget.py) come through exactly
    --mixamo-fbx PATH    write the character (body + skeleton, rest pose, no
                         props) as an FBX to upload to Mixamo, and stop
    --no-export          build and check only

Every module in art/peasant/clips/ is a clip (see lib/clip.py for the
module contract); adding a clip is adding a file there.
"""

import argparse
import importlib.util
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
from lib.clip import ClipError, Context  # noqa: E402
from lib.rig import Rig  # noqa: E402

SOURCE = os.path.join(HERE, "source", "peasant_meshy.glb")
DEFAULT_OUT = os.path.join(REPO, "assets", "models", "peasant")
MODEL_PATH = "models/peasant/peasant.glb"  # as the engine's asset server sees it


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    ap = argparse.ArgumentParser(prog="build_peasant.py")
    ap.add_argument("--only", default=None)
    ap.add_argument("--out", default=None)
    ap.add_argument("--sheets", default=None)
    ap.add_argument("--calibrate", default=None)
    ap.add_argument("--check-retarget", action="store_true")
    ap.add_argument("--mixamo-fbx", default=None)
    ap.add_argument("--no-export", action="store_true")
    return ap.parse_args(argv)


def discover_clips():
    clips_dir = os.path.join(HERE, "clips")
    modules = {}
    for fname in sorted(os.listdir(clips_dir)):
        if not fname.endswith(".py") or fname.startswith("_"):
            continue
        name = fname[:-3]
        spec = importlib.util.spec_from_file_location(f"peasant_clips.{name}", os.path.join(clips_dir, fname))
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        for attr in ("DURATION", "LOOP", "build"):
            if not hasattr(mod, attr):
                raise ClipError(f"clips/{fname} has no {attr}")
        modules[name] = mod
    return modules


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


def main():
    args = parse_args()
    t0 = time.time()
    arm, body, source_clips = source.load(SOURCE)
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
    props.build_sash(body, arm)

    modules = discover_clips()
    if args.only:
        wanted = [n.strip() for n in args.only.split(",") if n.strip()]
        unknown = [n for n in wanted if n not in modules]
        if unknown:
            raise SystemExit(f"unknown clips: {unknown}; have {sorted(modules)}")
    else:
        wanted = sorted(modules)
    ctx = Context(rig, arm, source_clips, modules)
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
        for name in ("Cheer", "Chop"):  # a one-shot clip and a loop
            rot, loc = retarget.round_trip(ctx, arm, body, name)
            print(f"retarget round trip via FBX, {name}: worst rotation {rot:.4f} deg, pelvis {loc * 1000:.2f} mm")
            if rot > 0.05 or loc > 0.001:
                raise SystemExit("retarget round trip failed")
    arm.animation_data.action = None
    for pb in arm.pose.bones:
        pb.location = (0, 0, 0)
        pb.rotation_quaternion = (1, 0, 0, 0)
    bpy.context.view_layer.update()

    out_dir = args.out or (None if args.only else DEFAULT_OUT)
    if out_dir and not args.no_export:
        # Only the requested clips go into the file.
        for clip in ctx.clips.values():
            if clip not in clips:
                bpy.data.actions.remove(clip.action)
        glb = os.path.join(out_dir, "peasant.glb")
        rest = {n: tuple(sum((o.matrix_world @ v.co for v in o.data.vertices), Vector()) / len(o.data.vertices))
                for n, o in prop_objs.items()}
        export.export_glb(glb)
        s = export.verify(glb, clips, props.all_prop_names(), rig.order, props.emissive_materials(), rest,
                          texture_size=source.TEXTURE_SIZE)
        export.write_manifest(os.path.join(out_dir, "peasant.model.ron"), MODEL_PATH, hi - lo, clips)
        print(f"\nwrote {glb} ({os.path.getsize(glb) / 1e6:.2f} MB)")
        for name, a in sorted(s["animations"].items()):
            print(f"  animation {name:12s} {a['duration']:.4f}s  {a['channels']} channels on {a['nodes']} nodes")
        print("  images:", ", ".join(f"{i['name']} {i['mime']} {i['size'][0]}x{i['size'][1]} {i['bytes'] / 1e3:.0f} kB"
                                     for i in s["images"]))
    elif args.only:
        print("\n--only without --out: nothing exported")

    if args.sheets or args.calibrate:
        rig_scene = sheets.SheetRig(bpy.context.scene)
        scratch = os.path.join(args.sheets or args.calibrate, "_tiles")
        if args.sheets:
            for clip in clips:
                out = os.path.join(args.sheets, f"{clip.name}.png")
                sheets.render_sheet(arm, body, prop_objs, clip, out, scratch, rig_scene)
                print("sheet", out)
        if args.calibrate:
            from lib import calibrate
            for out in calibrate.render(ctx, arm, body, prop_objs, args.calibrate, scratch, rig_scene):
                print("calibration", out)
        try:
            os.rmdir(scratch)
        except OSError:
            pass
    print(f"\ndone in {time.time() - t0:.1f}s")


if __name__ == "__main__":
    try:
        main()
    except Exception:
        import traceback
        traceback.print_exc()
        sys.exit(1)
