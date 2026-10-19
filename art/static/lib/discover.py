"""Find the source models and group them into assets by file name.

    source/<buildings|nodes>/<id>.<ext>            one model (variant "a")
    source/<buildings|nodes>/<id>_<variant>.<ext>  variants: pine_a, pine_b...
    source/<buildings|nodes>/<id>_stump.<ext>      a felled tree's stump

Ids contain underscores (`town_center`, `stone_outcrop`), so a name is
split against the game's ids first: `town_center.glb` is the town centre,
`stone_outcrop_b.glb` its variant b. A name that matches no id is split at
its last underscore when what follows looks like a variant (one letter, or
digits), and kept whole otherwise; either way the build warns.
"""

import os
import re
from dataclasses import dataclass, field

EXTENSIONS = (".glb", ".gltf", ".fbx", ".obj")
STUMP = "stump"
DEFAULT_VARIANT = "a"
_VARIANT = re.compile(r"^[a-z0-9]{1,8}$")
_SHORT_VARIANT = re.compile(r"^(?:[a-z]|\d{1,3})$")
_ID = re.compile(r"^[a-z0-9_]+$")


class SourceError(ValueError):
    pass


@dataclass
class Asset:
    kind: str                 # "buildings" | "nodes"
    id: str
    variants: dict = field(default_factory=dict)   # variant name -> source path
    stump: str = None         # source path
    known: bool = True        # the id exists in the game content
    notes: list = field(default_factory=list)

    @property
    def key(self):
        return f"{self.kind}/{self.id}"


def split_name(stem, ids):
    """(id, variant or None, is_stump) for a file stem, or raise SourceError.
    `ids` is the set of known ids of that kind."""
    stem = stem.lower()
    if not _ID.match(stem):
        raise SourceError(f"{stem!r}: use lower-case letters, digits and underscores (<id>[_<variant>])")
    if stem in ids:
        return stem, None, False
    if "_" in stem:
        head, tail = stem.rsplit("_", 1)
        if head in ids:
            if tail == STUMP:
                return head, None, True
            if _VARIANT.match(tail):
                return head, tail, False
        if tail == STUMP:
            return head, None, True
        if _SHORT_VARIANT.match(tail):
            return head, tail, False
    return stem, None, False


def discover(source_root, content):
    """{(kind, id): Asset} for every model under source_root/<kind>/."""
    assets = {}
    for kind in ("buildings", "nodes"):
        folder = os.path.join(source_root, kind)
        if not os.path.isdir(folder):
            continue
        ids = {e.id for e in content.values() if e.kind == kind}
        other = {e.id: e.kind for e in content.values() if e.kind != kind}
        seen = {}
        for name in sorted(os.listdir(folder)):
            path = os.path.join(folder, name)
            stem, ext = os.path.splitext(name)
            if not os.path.isfile(path) or name.startswith((".", "_")):
                continue
            if ext.lower() not in EXTENSIONS:
                if ext.lower() not in (".bin", ".png", ".jpg", ".jpeg", ".mtl", ".tga", ".webp", ".txt", ".md"):
                    print(f"  ! {kind}/{name}: not a model ({', '.join(EXTENSIONS)}); skipped")
                continue
            ident, variant, stump = split_name(stem, ids)
            slot = (ident, STUMP if stump else (variant or DEFAULT_VARIANT))
            if slot in seen:
                raise SourceError(f"{kind}/{name} and {kind}/{seen[slot]} are the same "
                                  f"{'stump' if stump else 'variant ' + slot[1]} of {ident!r}")
            seen[slot] = name
            asset = assets.get((kind, ident))
            if asset is None:
                asset = assets[(kind, ident)] = Asset(kind, ident, known=ident in ids)
                if ident in other:
                    asset.notes.append(f"{ident!r} is a {other[ident][:-1]} in the game, not a {kind[:-1]}")
                elif ident not in ids:
                    asset.notes.append(f"{ident!r} is not a {kind[:-1]} id in assets/data")
            if stump:
                asset.stump = path
            else:
                asset.variants[variant or DEFAULT_VARIANT] = path
    for asset in assets.values():
        if not asset.variants:
            raise SourceError(f"{asset.key}: a stump but no model ({asset.id}.glb or {asset.id}_a.glb)")
        if asset.stump and asset.kind != "nodes":
            raise SourceError(f"{asset.key}: stumps are for trees (nodes)")
    return assets
