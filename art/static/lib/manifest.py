"""The files the engine reads: per-asset manifests and the registry.

Formats are the contract's (docs/art/static-models.md). Every path is an
asset path: relative to the asset folder, forward slashes.
"""

import os

from .discover import STUMP

KIND_NAME = {"buildings": "Building", "nodes": "Node", "decor": "Decor"}
KINDS = ("buildings", "nodes", "decor")  # the registry's sections, in order
MANIFEST_SUFFIX = ".model.ron"


def asset_path(path, asset_root):
    rel = os.path.relpath(os.path.abspath(path), os.path.abspath(asset_root))
    if rel.startswith(".."):
        raise ValueError(f"{path} is outside the asset folder {asset_root}")
    return rel.replace(os.sep, "/")


def _num(v):
    s = f"{v:.3f}"
    return "0.000" if s == "-0.000" else s


def _vec(v):
    return "(" + ", ".join(_num(x) for x in v) + ")"


def _part(p):
    return f'(model: "{p["model"]}", min: {_vec(p["min"])}, max: {_vec(p["max"])}, triangles: {p["triangles"]})'


def write_manifest(path, kind, ident, fit, random_yaw, variants, stump, banner, size=None):
    """`variants`: list of {model, min, max, triangles} in variant order;
    `stump`: one such dict or None; `size`: decor's drawn size (w, h, d),
    written only when given (buildings and nodes have the game's box)."""
    lines = [
        "// Written by art/static/build_static.py; do not edit by hand.",
        "// Paths are relative to the asset folder. min/max: the model's bounds in",
        "// metres (y up, front +Z) without the team banner; triangles include it.",
        "(",
        f"    kind: {KIND_NAME[kind]},",
        f'    id: "{ident}",',
        f"    fit: {fit},",
        f"    random_yaw: {'true' if random_yaw else 'false'},",
    ]
    if size is not None:
        lines.append(f"    size: {_vec(size)},")
    lines += [
        "    variants: [",
    ]
    lines += [f"        {_part(v)}," for v in variants]
    lines += [
        "    ],",
        f"    stump: {'Some(' + _part(stump) + ')' if stump else 'None'},",
        f"    banner: {'true' if banner else 'false'},",
        ")",
        "",
    ]
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", newline="\n", encoding="utf-8") as f:
        f.write("\n".join(lines))


def find_manifests(out_root):
    """{kind: {id: manifest path}} for every <kind>/<id>/<id>.model.ron under out_root."""
    found = {kind: {} for kind in KINDS}
    for kind in found:
        folder = os.path.join(out_root, kind)
        if not os.path.isdir(folder):
            continue
        for ident in sorted(os.listdir(folder)):
            path = os.path.join(folder, ident, ident + MANIFEST_SUFFIX)
            if os.path.isfile(path):
                found[kind][ident] = path
    return found


def write_registry(path, manifests, asset_root, out_root):
    """The registry (`visuals/static.ron`): building, node and decor ids -> manifest."""
    where = asset_path(out_root, asset_root)
    where = "this folder" if where in ("", ".") else where + "/"
    lines = [
        "// Written by art/static/build_static.py from the manifests under",
        f"// {where}; do not edit by hand.",
        "// Paths are relative to the asset folder.",
        "(",
        "    units: {},",
    ]
    for kind in KINDS:
        entries = manifests.get(kind, {})
        if not entries:
            lines.append(f"    {kind}: {{}},")
            continue
        lines.append(f"    {kind}: {{")
        lines += [f'        "{i}": "{asset_path(p, asset_root)}",' for i, p in sorted(entries.items())]
        lines.append("    },")
    lines += [")", ""]
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w", newline="\n", encoding="utf-8") as f:
        f.write("\n".join(lines))


def output_names(variants, has_stump):
    """Output file stems: the variant names, plus "stump"."""
    names = list(variants)
    if has_stump:
        names.append(STUMP)
    return names
