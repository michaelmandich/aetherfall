"""The game's units and weapons, read from assets/data/**/*.ron.

Clips take their timing from the content: an Attack lands on the weapon's
`windup` (the engine plays it so `hit` falls on the sim's hit). The files
are read with the static pipeline's small RON reader (art/static/lib/
content.py `parse`), so a new field in the content never breaks the build.
Plain Python: usable in Blender and outside it.
"""

import importlib.util
import os

UNITS_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))   # art/units
REPO = os.path.dirname(os.path.dirname(UNITS_DIR))
DATA = os.path.join(REPO, "assets", "data")
_CONTENT = os.path.join(REPO, "art", "static", "lib", "content.py")
_CACHE = {}


def _parse():
    spec = importlib.util.spec_from_file_location("af_content_reader", _CONTENT)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.parse


def _load():
    if not _CACHE:
        parse = _parse()
        units, weapons = {}, {}
        for root, _, files in os.walk(DATA):
            for name in sorted(files):
                if not name.endswith(".ron"):
                    continue
                with open(os.path.join(root, name), encoding="utf-8") as f:
                    doc = parse(f.read(), name)
                if not isinstance(doc, dict):
                    continue
                for u in doc.get("units", ()) or ():
                    if isinstance(u, dict) and isinstance(u.get("id"), str):
                        units[u["id"]] = u
                for w in doc.get("weapons", ()) or ():
                    if isinstance(w, dict) and isinstance(w.get("id"), str):
                        weapons[w["id"]] = w
        _CACHE["units"], _CACHE["weapons"] = units, weapons
    return _CACHE["units"], _CACHE["weapons"]


def unit(unit_id):
    """The unit's content entry (a dict: size, weapons, tags, healer...)."""
    units, _ = _load()
    if unit_id not in units:
        raise KeyError(f"no unit {unit_id!r} in {DATA}")
    return units[unit_id]


def weapon(weapon_id):
    _, weapons = _load()
    if weapon_id not in weapons:
        raise KeyError(f"no weapon {weapon_id!r} in {DATA}")
    return weapons[weapon_id]


def main_weapon(unit_id):
    """The unit's first weapon's id, or None for a unit without one."""
    ws = unit(unit_id).get("weapons") or []
    return ws[0] if ws else None


def windup(unit_id):
    """Seconds from the start of a swing to its hit: the Attack clip's HIT."""
    wid = main_weapon(unit_id)
    if wid is None:
        raise KeyError(f"{unit_id} has no weapon (no Attack clip)")
    return float(weapon(wid)["windup"])


def cooldown(unit_id):
    return float(weapon(main_weapon(unit_id))["cooldown"])
