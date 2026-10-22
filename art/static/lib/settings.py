"""Per-asset settings: the defaults, overridden by art/static/assets.toml.

    [town_center]
    triangles = 10000
    banner = [3.2, 6.5, 3.2]   # pole foot (m, model frame before fitting), or false
    yaw = 180                  # the source faces -Z

    [reeds]
    size = [1.2, 1.4, 1.2]     # decor: the size the game draws it at (m)

Unknown keys and wrong types are errors, so a typo cannot silently do
nothing. The defaults are in `defaults` (buildings and nodes) and `DECOR`
(scenery), and documented in assets.toml.
"""

import tomllib
from dataclasses import dataclass, replace

FITS = ("Box", "Footprint", "Height")

# Triangle budgets (whole model, banner included) and texture sizes.
TREE_TRIANGLES = 3000   # 1500 left Meshy canopies in big flat facets; 3000 cost no measurable frame time
NODE_TRIANGLES = 2000
BUILDING_TRIANGLES = 6000
BIG_BUILDING_TRIANGLES = 10000
BIG_BUILDINGS = ("town_center", "citadel")
STUMP_TRIANGLES = 300
MAX_PLATE = 0.25    # cut_plate never takes more than this of a model's height
MAX_TINT = 2.0      # tint multiplies each channel by at most this
NODE_TEXTURE = 512
BUILDING_TEXTURE = 1024
DECOR_TEXTURE = 512        # small scenery; the big pieces (mountains, rims...) 1024

# Decor (landscape scenery, not a game entity; the engine places it from
# the terrain, docs/art/static-models.md "Decor"): per id, how the engine
# sizes it (fit), the size it is drawn at before the placement's own
# scaling (w, h, d metres; the front toward +Z, long pieces lying along
# X), its triangle budget and texture size. The engine falls back to the
# same sizes (`af_render::terrain::decor::DecorKind::size`) for a manifest
# without one. Any other id in source/decor/ is built with DECOR_DEFAULT
# (and a warning), and the game never places it.
DECOR = {
    # Mountains: the summits, spines and feet of the ranges (scaled to the land).
    "mountain_peak": ("Height", (13.0, 8.5, 13.0), 8000, 1024),
    "mountain_ridge": ("Footprint", (22.0, 6.0, 8.0), 6000, 1024),
    "mountain_foothill": ("Footprint", (10.0, 3.0, 8.0), 4000, 1024),
    # Meteor sites.
    "crater_rim": ("Footprint", (10.0, 2.5, 4.0), 5000, 1024),
    "star_core": ("Footprint", (6.0, 3.5, 6.0), 5000, 1024),
    "aether_monolith": ("Height", (2.6, 8.0, 2.6), 3500, 1024),
    "impact_scar": ("Footprint", (5.0, 1.0, 4.0), 2000, 512),
    "meteor_shard": ("Height", (1.4, 1.8, 1.2), 1000, 512),
    # Water and wet ground.
    "reeds": ("Height", (1.2, 1.5, 1.2), 1500, 512),
    "river_rocks": ("Footprint", (2.0, 0.45, 1.4), 800, 512),
    "lily_pads": ("Footprint", (2.4, 0.15, 2.4), 600, 512),
    # Coasts.
    "sea_cliff": ("Footprint", (24.0, 7.0, 7.0), 5000, 1024),
    "sea_stack": ("Height", (4.5, 8.0, 4.5), 4000, 1024),
    "beach_rocks": ("Footprint", (4.0, 0.8, 2.6), 1000, 512),
    # Thickets, the woods' edges, rough ground.
    "bramble": ("Footprint", (2.2, 1.3, 2.2), 1500, 512),
    "mossy_boulder": ("Footprint", (1.8, 1.2, 1.5), 1000, 512),
    "shrub": ("Footprint", (1.6, 1.3, 1.6), 1500, 512),
    "fern": ("Footprint", (1.2, 0.8, 1.2), 1200, 512),
    # Landmarks: off the map (in the skirt), where nothing walks.
    "standing_stones": ("Footprint", (5.0, 3.2, 3.0), 1500, 512),
    "ruined_watchtower": ("Height", (5.0, 7.5, 5.0), 3000, 1024),
    "wayside_shrine": ("Height", (1.4, 2.4, 1.2), 1500, 512),
    "fallen_log": ("Footprint", (5.0, 0.8, 1.2), 1000, 512),
    "driftwood": ("Footprint", (3.5, 0.5, 1.2), 800, 512),
    "shipwreck": ("Footprint", (12.0, 5.0, 5.0), 3000, 1024),
    "stone_bridge": ("Footprint", (12.0, 3.0, 3.4), 2500, 1024),
    "wooden_footbridge": ("Footprint", (10.0, 1.8, 2.0), 2000, 512),
    "backdrop_mountains": ("Footprint", (60.0, 12.0, 14.0), 4000, 1024),
    "backdrop_forest": ("Footprint", (42.0, 7.0, 10.0), 3000, 512),
}
DECOR_DEFAULT = ("Footprint", None, 1500, DECOR_TEXTURE)
# A decor size this much wider than deep is a long piece: the build turns
# a model whose longer side lies along Z a quarter, so it lies along X.
LONG_DECOR = 1.4


@dataclass(frozen=True)
class Settings:
    fit: str
    triangles: int
    random_yaw: bool
    yaw: float = 0.0              # degrees about +Y, applied to the source (+ turns +Z toward +X)
    banner: object = None         # buildings: None = automatic anchor, False = none, (x, y, z) = anchor
    texture: int = NODE_TEXTURE   # largest texture side, px
    stump_triangles: int = STUMP_TRIANGLES
    cut_plate: tuple = ()         # ((variant or "*", fraction of the height), ...): ground plates cut away
    tint: tuple = ()              # ((variant or "*", (r, g, b)), ...): base colour textures multiplied
    size: tuple = None            # decor: (w, h, d) metres the engine draws it at (None: the model's own)

    @property
    def long(self):
        """A long decor piece (a ridge, a rim arc, a bridge): its longer
        horizontal side should lie along X."""
        return self.size is not None and self.size[0] > LONG_DECOR * self.size[2]

    def plate_cut(self, variant):
        """How deep (fraction of the height) to cut this variant's ground
        plate, or None to leave it whole."""
        return _for_variant(self.cut_plate, variant)

    def tint_for(self, variant):
        """(r, g, b) to multiply this variant's base colour textures by, or None."""
        return _for_variant(self.tint, variant)

    def variants_named(self):
        """Every variant a per-variant setting names ("*" left out)."""
        return {v for table in (self.cut_plate, self.tint) for v, _ in table if v != "*"}


def _for_variant(table, variant):
    values = dict(table)
    return values.get(variant, values.get("*"))


def defaults(kind, ident, entry=None):
    """The settings an asset gets when assets.toml says nothing."""
    if kind == "decor":
        fit, size, tris, texture = DECOR.get(ident, DECOR_DEFAULT)
        return Settings(fit=fit, triangles=tris, random_yaw=True, banner=False, texture=texture, size=size)
    if kind == "buildings":
        tris = BIG_BUILDING_TRIANGLES if ident in BIG_BUILDINGS else BUILDING_TRIANGLES
        return Settings(fit="Box", triangles=tris, random_yaw=False, banner=None, texture=BUILDING_TEXTURE)
    if entry is not None and entry.is_tree:
        return Settings(fit="Height", triangles=TREE_TRIANGLES, random_yaw=True, banner=False)
    return Settings(fit="Footprint", triangles=NODE_TRIANGLES, random_yaw=True, banner=False)


class SettingsError(ValueError):
    pass


def _check(ident, key, value):
    where = f"assets.toml [{ident}] {key}"
    if key == "fit":
        if value not in FITS:
            raise SettingsError(f"{where} = {value!r}: one of {', '.join(FITS)}")
        return value
    if key in ("triangles", "stump_triangles", "texture"):
        if not isinstance(value, int) or isinstance(value, bool) or value < (16 if key == "texture" else 12):
            raise SettingsError(f"{where} = {value!r}: a whole number{' of px' if key == 'texture' else ''}")
        return value
    if key == "random_yaw":
        if not isinstance(value, bool):
            raise SettingsError(f"{where} = {value!r}: true or false")
        return value
    if key == "yaw":
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise SettingsError(f"{where} = {value!r}: degrees")
        return float(value)
    if key == "cut_plate":
        if (isinstance(value, dict) and value and all(
                isinstance(k, str) and isinstance(v, (int, float)) and not isinstance(v, bool) and 0 < v <= MAX_PLATE
                for k, v in value.items())):
            return tuple(sorted((k, float(v)) for k, v in value.items()))
        raise SettingsError(f"{where} = {value!r}: variant = depth, as a fraction of the height "
                            f"(0 to {MAX_PLATE}): {{ c = 0.08 }}, or {{ \"*\" = 0.08 }} for every variant")
    if key == "tint":
        def rgb(v):
            return (isinstance(v, list) and len(v) == 3 and all(
                isinstance(c, (int, float)) and not isinstance(c, bool) and 0 <= c <= MAX_TINT for c in v))
        if isinstance(value, dict) and value and all(isinstance(k, str) and rgb(v) for k, v in value.items()):
            return tuple(sorted((k, tuple(float(c) for c in v)) for k, v in value.items()))
        raise SettingsError(f"{where} = {value!r}: variant = [r, g, b] multipliers (0 to {MAX_TINT}): "
                            f"{{ a = [0.7, 0.75, 0.55] }}, or {{ \"*\" = [...] }} for every variant")
    if key == "size":
        if (isinstance(value, list) and len(value) == 3 and all(
                isinstance(v, (int, float)) and not isinstance(v, bool) and v > 0 for v in value)):
            return tuple(float(v) for v in value)
        raise SettingsError(f"{where} = {value!r}: [w, h, d] in metres, each above 0")
    if key == "banner":
        if value is True:
            return None
        if value is False:
            return False
        if (isinstance(value, list) and len(value) == 3
                and all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in value)):
            return tuple(float(v) for v in value)
        raise SettingsError(f"{where} = {value!r}: false, true or [x, y, z]")
    raise SettingsError(f"{where}: unknown setting (fit, triangles, stump_triangles, random_yaw, yaw, banner, texture, cut_plate, tint, size)")


def load(path):
    """{id: {key: checked value}} from assets.toml (empty if it is missing)."""
    try:
        with open(path, "rb") as f:
            raw = tomllib.load(f)
    except FileNotFoundError:
        return {}
    except tomllib.TOMLDecodeError as e:
        raise SettingsError(f"{path}: {e}") from e
    out = {}
    for ident, table in raw.items():
        if not isinstance(table, dict):
            raise SettingsError(f"assets.toml: `{ident} = ...` must be a table: [{ident}]")
        out[ident] = {k: _check(ident, k, v) for k, v in table.items()}
    return out


def resolve(kind, ident, entry, overrides):
    """Defaults for this asset with its assets.toml table applied."""
    s = defaults(kind, ident, entry)
    table = dict(overrides.get(ident, {}))
    if kind != "buildings" and table.get("banner", False) is not False:
        raise SettingsError(f"assets.toml [{ident}] banner: only buildings carry a banner")
    if kind != "nodes" and "stump_triangles" in table:
        raise SettingsError(f"assets.toml [{ident}] stump_triangles: only nodes have stumps")
    if kind != "decor" and "size" in table:
        raise SettingsError(f"assets.toml [{ident}] size: only decor has a drawn size (the game's box sizes the rest)")
    return replace(s, **table)
