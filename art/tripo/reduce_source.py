"""Pre-reduce a Tripo Studio download into a unit source (headless Blender).

Tripo Studio's H3.1 exports are about 1.9 million triangles and 60 MB each,
twelve times the Meshy sources beside them in art/units/source/ (about
24k triangles, 5 MB). The unit build decimates every rigid model to a few
thousand triangles anyway, so the stored source only has to be far finer
than that: this script welds and collapse-decimates the model to
`--triangles` (default 200k) with the static pipeline's own reduce step
(art/static/lib/reduce.py), keeps its orientation, scale and materials
exactly as Tripo made them (base colour, normal and metallic-roughness
maps, 2k JPEGs, written back as they were) and writes a GLB. The untouched
downloads stay outside the repository; art/tripo/studio_imports.json says
where each source came from.

    "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b -P art/tripo/reduce_source.py -- \
        --in C:/Users/meman/Downloads/dune_sandskiff.glb --out art/units/source/dune_sandskiff.glb
"""

import argparse
import os
import sys
import time

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(os.path.dirname(HERE), "units"))

from lib.static import static_lib  # noqa: E402  (art/units/lib/static.py)


def main(argv):
    ap = argparse.ArgumentParser(prog="reduce_source.py")
    ap.add_argument("--in", dest="src", required=True, help="the Tripo GLB")
    ap.add_argument("--out", required=True, help="the reduced GLB to write")
    ap.add_argument("--triangles", type=int, default=200_000)
    args = ap.parse_args(argv)
    scene, reduce, gltf = (static_lib(n) for n in ("scene", "reduce", "gltf"))

    t0 = time.time()
    scene.reset()
    objs = scene.import_file(args.src)
    meshes = [o for o in objs if o.type == "MESH"]
    if len(meshes) != 1:
        raise SystemExit(f"{args.src}: expected one mesh, found {len(meshes)}")
    obj = meshes[0]
    with bpy.context.temp_override(object=obj, active_object=obj, selected_objects=[obj]):
        bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    notes = []
    before, after = reduce.reduce(obj, args.triangles, notes)
    images = sorted((i.name, i.size[0], i.size[1], i.file_format) for i in bpy.data.images if i.size[0])
    gltf.export(os.path.abspath(args.out), [obj])
    js, _ = gltf.read(os.path.abspath(args.out))
    print(f"{os.path.basename(args.src)}: {before} -> {after} triangles in {time.time() - t0:.0f} s; "
          f"wrote {args.out} ({os.path.getsize(args.out) / 1e6:.1f} MB)")
    for n in notes:
        print("  ", n)
    for name, w, h, fmt in images:
        print(f"   image {name} {w}x{h} {fmt}")
    for im in js.get("images", []):
        bv = js["bufferViews"][im["bufferView"]]
        print(f"   written {im.get('name')} {im.get('mimeType')} {bv['byteLength'] / 1e6:.2f} MB")


if __name__ == "__main__":
    main(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])
