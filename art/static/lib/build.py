"""One asset: every variant (and the stump) -> GLB files and the manifest.

The asset is built into a temporary folder and moved into place only when
every part has passed its checks, so a failure leaves the previous output
untouched.
"""

import os
import shutil
import tempfile

import bpy

from . import banner, gltf, materials, reduce, scene
from .discover import STUMP
from .manifest import MANIFEST_SUFFIX, asset_path, write_manifest

BANNER_RESERVE = 60  # triangles kept free for the team banner


class BuildError(RuntimeError):
    pass


def has_banner(kind, s):
    return kind == "buildings" and s.banner is not False


def fit_scale(fit, model_size, box):
    """Per-axis scale (glTF axes) the engine applies: model metres -> game metres."""
    mx, my, mz = (max(v, 1e-6) for v in model_size)
    w, h, d = box
    if fit == "Box":
        return (w / mx, h / my, d / mz)
    if fit == "Footprint":
        s = max(w, d) / max(mx, mz)
    else:
        s = h / my
    return (s, s, s)


def build_part(src, ident, part, s, kind, entry, out_path, is_stump=False):
    """Import, clean, reduce (and add the banner to) one model; export it to
    out_path. Returns {min, max, triangles, notes, ...} (glTF axes)."""
    scene.reset()
    objs = scene.import_file(src)
    obj, notes = scene.to_statue(objs, ident)
    scene.normalise(obj, s.yaw)
    depth = None if is_stump else s.plate_cut(part)
    if depth is not None:
        cut = scene.cut_plate(obj, depth)
        if not cut:
            raise BuildError(f"{os.path.basename(src)}: cut_plate found nothing in the bottom {depth:.0%} to cut")
        notes.append(f"cut its ground plate: {cut} faces in the bottom {depth:.0%}")
        scene.normalise(obj, 0.0)
    obj.data.name = f"{ident}_{part}"

    found = [m for m in obj.data.materials if m is not None]
    if not found:
        obj.data.materials.append(materials.flat_material(f"{ident}_default", (0.6, 0.6, 0.6)))
    for mat in found:
        if mat.name == materials.TEAM:  # the banner's: the engine must not tint the model itself
            mat.name = f"{ident}_{mat.name}"
        materials.clean(mat, notes)
    tint = None if is_stump else s.tint_for(part)
    images = materials.shrink_textures(obj, s.texture, f"{ident}_{part}", notes, tint)
    if tint is not None:
        if not images:
            raise BuildError(f"{os.path.basename(src)}: tint has no texture to tint")
        notes.append(f"tinted by ({tint[0]:g}, {tint[1]:g}, {tint[2]:g})")
    materials.merge_duplicates(obj, notes)

    with_banner = has_banner(kind, s) and not is_stump
    budget = s.stump_triangles if is_stump else s.triangles
    source_tris, after = reduce.reduce(obj, budget - (BANNER_RESERVE if with_banner else 0), notes)
    if after != source_tris:  # collapsing edges moves vertices: base and centre again
        scene.drop_loose(obj)
        scene.normalise(obj, 0.0)

    lo, hi = scene.bounds(obj)
    gmin, gmax = scene.gltf_bounds(lo, hi)
    size = [b - a for a, b in zip(gmin, gmax)]
    if min(size[0], size[2]) <= 0 or size[1] <= 0:
        raise BuildError(f"{os.path.basename(src)} is flat or empty (size {size})")
    box = entry.size if entry and entry.size else tuple(size)
    objects = [obj]
    if with_banner:
        gx, gy, gz = fit_scale(s.fit, size, box)
        flag, foot, note = banner.build(obj, lo, hi, (gx, gz, gy), box[1], s.banner)
        notes.append(note or f"banner at ({foot.x:.2f}, {foot.z:.2f}, {-foot.y:.2f}) m")
        objects.append(flag)

    gltf.export(out_path, objects)
    summary = gltf.verify(out_path, ident, banner.NODE if with_banner else None, s.texture)
    model = summary["nodes"][ident]
    tris = sum(n["triangles"] for n in summary["nodes"].values())
    if tris > budget:
        raise BuildError(f"{os.path.basename(out_path)}: {tris} triangles, over the budget of {budget}")
    return dict(min=model["min"], max=model["max"], triangles=tris, source_triangles=source_tris,
                notes=notes, bytes=summary["bytes"], images=summary["images"],
                materials=[m["name"] for m in summary["materials"]])


def build_asset(asset, s, entry, out_root, asset_root, sheets_dir=None):
    """Build every part of `asset` into out_root/<kind>/<id>/ and write its manifest."""
    final_dir = os.path.join(out_root, asset.kind, asset.id)
    tmp_root = tempfile.mkdtemp(prefix=f"af_static_{asset.id}_")
    try:
        tmp_dir = os.path.join(tmp_root, asset.id)
        os.makedirs(tmp_dir)
        parts = [(name, path, False) for name, path in sorted(asset.variants.items())]
        unknown = sorted(s.variants_named() - set(asset.variants))
        if unknown:
            raise BuildError(f"assets.toml [{asset.id}]: no variant {', '.join(unknown)} "
                             f"(have {', '.join(sorted(asset.variants))})")
        if asset.stump:
            parts.append((STUMP, asset.stump, True))
        built = {}
        for name, src, is_stump in parts:
            out = os.path.join(tmp_dir, f"{name}.glb")
            r = build_part(src, asset.id, name, s, asset.kind, entry, out, is_stump)
            r["model"] = asset_path(os.path.join(final_dir, f"{name}.glb"), asset_root)
            built[name] = r
            size = [b - a for a, b in zip(r["min"], r["max"])]
            tex = ", ".join(f"texture {i['size'][0]}x{i['size'][1]} px {i['bytes'] / 1e3:.1f} kB"
                            for i in r["images"]) or "no texture"
            print(f"  {name:6s} {os.path.basename(src):28s} {r['source_triangles']:>7} -> {r['triangles']:>5} tris  "
                  f"{size[0]:.2f} x {size[1]:.2f} x {size[2]:.2f} m  {r['bytes'] / 1e3:.0f} kB, {tex}")
            for note in r["notes"]:
                print(f"         {note}")
        variants = [built[n] for n, _, stump in parts if not stump]
        stump = built.get(STUMP)
        write_manifest(os.path.join(tmp_dir, asset.id + MANIFEST_SUFFIX), asset.kind, asset.id, s.fit,
                       s.random_yaw, variants, stump, has_banner(asset.kind, s))
        if sheets_dir:
            from . import sheets
            sheet = os.path.join(sheets_dir, f"{asset.kind}_{asset.id}.png")
            files = [(n, os.path.join(tmp_dir, f"{n}.glb"), built[n]) for n, _, _ in parts]
            sheets.render_asset(asset, s, entry, files, sheet)
            print(f"  sheet {sheet}")
        if os.path.isdir(final_dir):
            shutil.rmtree(final_dir)
        os.makedirs(os.path.dirname(final_dir), exist_ok=True)
        shutil.move(tmp_dir, final_dir)
    finally:
        shutil.rmtree(tmp_root, ignore_errors=True)
        for img in list(bpy.data.images):
            if img.users == 0:
                bpy.data.images.remove(img)
