"""A unit's configuration (art/units/<id>/unit.py), read and checked, and the
clip modules it builds.

A unit's folder holds `unit.py` (the settings below, UPPERCASE module
names; anything else UPPERCASE is an error, so a typo cannot silently do
nothing) and, for humanoids, `clips/` with the unit's own clip modules.

Humanoid units (KIND = "humanoid", the default): a rigged Meshy character.

    SOURCE = "source/spearman.glb"   # the rigged GLB, relative to art/units/
                                     # (default source/<id>.glb)
    PROPS = {"Gear_spear": "spear"}  # node name -> prop id: a Meshy prop of
                                     # art/units/meshy_props.py (its model in
                                     # source/props/, a boxy stand-in until
                                     # then) or a boxy kind of lib/props.py
                                     # KINDS; Tool_*/Load_* nodes are shown by
                                     # what the unit does, Gear_* always
    HELD = ("Gear_spear",)           # props in hand in every inherited clip
                                     # (default: the Gear_* props)
    def CARRY(clip): ...             # the pose that holds HELD in inherited
                                     # clips (a Pose; wield() is fine, the
                                     # build converts it to FK)
    CARRY_SKIP = ("Cheer",)          # inherited clips that keep their own arms
    INHERIT = ("Idle", ...)          # shared clips (art/units/clips/) to use
                                     # (default: all of them); a clip in the
                                     # unit's own clips/ replaces a shared one
    SASH = {"shoulder": (0.165, 0.0, 1.33), "hip": (-0.16, 0.0, 0.99), "width": 0.085}
                                     # the team sash's points (right, fwd, up
                                     # in metres; default: from the skeleton,
                                     # props.sash_anchors), or None for none
    SOURCE_CLIPS = {"Walking": [("source/x.glb", None)]}
                                     # where Walking / Running come from when
                                     # the source GLB has no such action
                                     # (default: source/<id>_walking.glb, then
                                     # the peasant's, retargeted)
    TEXTURE = 1024                   # base colour texture size, px
    TEXTURE_NAME, BODY_MATERIAL      # names in the GLB (default
                                     # <id>_basecolor, <Id>Body)

Rigid units (KIND = "rigid"): any model without a skeleton (vehicles,
siege, mounts, beasts, flyers), drawn as it is, with no clips.

    SOURCE = "source/ram.glb"        # .glb/.gltf/.fbx/.obj (default source/<id>.glb)
    YAW = 180                        # degrees to turn it so it faces +Z
    TRIANGLES = 5000                 # budget; decimated down to it
    TEXTURE = 1024
    CUT_PLATE = 0.05                 # cut a ground plate: the faces wholly in
                                     # this bottom fraction of the height
    MARKER = "pennant"               # team colour: "pennant" (a pole and flag),
                                     # "band" (a strip round the body) or None
    MARKER_AT = (0.2, -0.4, 1.1)     # pennant: where its pole stands (right,
                                     # fwd, up metres after normalising;
                                     # default: the highest point near the
                                     # back); band: its height as a fraction
                                     # of the model's (default 0.5)
"""

import importlib.util
import os
from types import SimpleNamespace

from . import props
from .clip import ROOT, ClipError
from .rig import ARM, GROUPS, SIDES

SHARED_CLIPS = os.path.join(ROOT, "clips")
# The source Walking/Running of the peasant, retargeted onto a unit whose
# own source and files have none.
FALLBACK_SOURCE = "peasant/source/peasant_meshy.glb"

HUMANOID = {"KIND", "SOURCE", "PROPS", "HELD", "CARRY", "CARRY_SKIP", "INHERIT", "SASH", "SOURCE_CLIPS",
            "TEXTURE", "TEXTURE_NAME", "BODY_MATERIAL"}
RIGID = {"KIND", "SOURCE", "YAW", "TRIANGLES", "TEXTURE", "CUT_PLATE", "MARKER", "MARKER_AT"}
MARKERS = ("pennant", "band", None)


class UnitError(RuntimeError):
    pass


def unit_ids():
    """Every configured unit (a folder of art/units with a unit.py)."""
    return sorted(d for d in os.listdir(ROOT) if os.path.isfile(os.path.join(ROOT, d, "unit.py")))


def _load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _camel(unit_id):
    return "".join(w.capitalize() for w in unit_id.split("_"))


class Unit:
    """One unit's settings, with every default filled in."""

    def __init__(self, unit_id, props_dir=None):
        """`props_dir`: where Meshy prop models are looked for (default
        art/units/source/props/)."""
        self.id = unit_id
        self.dir = os.path.join(ROOT, unit_id)
        path = os.path.join(self.dir, "unit.py")
        if not os.path.isfile(path):
            raise UnitError(f"no unit {unit_id!r}: {path} does not exist (configured: {', '.join(unit_ids())})")
        mod = _load_module(f"unit_{unit_id}", path)
        given = {k for k in vars(mod) if k.isupper()}
        self.kind = getattr(mod, "KIND", "humanoid")
        if self.kind not in ("humanoid", "rigid"):
            raise UnitError(f"{path}: KIND must be \"humanoid\" or \"rigid\", not {self.kind!r}")
        allowed = HUMANOID if self.kind == "humanoid" else RIGID
        unknown = sorted(given - allowed)
        if unknown:
            raise UnitError(f"{path}: unknown settings {unknown} for a {self.kind} unit (known: {sorted(allowed)})")
        get = lambda k, d=None: getattr(mod, k, d)  # noqa: E731
        self.source = os.path.normpath(os.path.join(ROOT, get("SOURCE", f"source/{unit_id}.glb")))
        self.texture = int(get("TEXTURE", 1024))
        self.doc = (mod.__doc__ or "").strip().split("\n")[0]
        if self.kind == "rigid":
            self.yaw = float(get("YAW", 0.0))
            self.triangles = int(get("TRIANGLES", 5000))
            self.cut_plate = get("CUT_PLATE")
            self.marker = get("MARKER", "pennant")
            if self.marker not in MARKERS:
                raise UnitError(f"{path}: MARKER must be one of {MARKERS}, not {self.marker!r}")
            self.marker_at = get("MARKER_AT")
            return
        self.props = dict(get("PROPS", {}))
        try:
            self.prop_notes = props.use(self.props, props_dir)
        except KeyError as e:
            raise UnitError(f"{path}: PROPS: {e.args[0]}") from None
        self.held = tuple(get("HELD", tuple(n for n in self.props if n.startswith("Gear_"))))
        for name in self.held:
            if name not in self.props:
                raise UnitError(f"{path}: HELD names {name}, which is not in PROPS")
        hands = [props.prop_info(n).hand for n in self.held]
        if len(set(hands)) != len(hands):
            raise UnitError(f"{path}: HELD puts two props in one hand")
        self.carry = get("CARRY")
        if self.held and self.carry is None:
            raise UnitError(f"{path}: HELD props need a CARRY pose to hold them in the shared clips")
        self.carry_skip = tuple(get("CARRY_SKIP", ()))
        self.inherit = get("INHERIT")
        sash = get("SASH", {})
        if sash is not None:
            unknown = sorted(set(sash) - {"shoulder", "hip", "width"})
            if unknown or (("shoulder" in sash) != ("hip" in sash)):
                raise UnitError(f"{path}: SASH takes shoulder and hip (both or neither) and width; got {sorted(sash)}")
        self.sash = sash
        self.source_clips = get("SOURCE_CLIPS") or {
            name: [(f"source/{unit_id}_{name.lower()}.glb", None), (FALLBACK_SOURCE, name)]
            for name in ("Walking", "Running")
        }
        self.texture_name = get("TEXTURE_NAME", f"{unit_id}_basecolor")
        self.body_material = get("BODY_MATERIAL", f"{_camel(unit_id)}Body")

    # -- clips ---------------------------------------------------------------

    def clip_modules(self):
        """{clip name: module}: the inherited shared clips (wrapped to hold the
        HELD props, see `carried`; the clip as shared is then `<name>:free`,
        built but not exported) and the unit's own (which win)."""
        shared = _modules(SHARED_CLIPS, "shared_clips")
        names = sorted(shared) if self.inherit is None else list(self.inherit)
        unknown = [n for n in names if n not in shared]
        if unknown:
            raise UnitError(f"{self.id}: INHERIT names {unknown}, which art/units/clips/ lacks (it has {sorted(shared)})")
        own = _modules(os.path.join(self.dir, "clips"), f"{self.id}_clips")
        modules = {}
        for name in names:
            if name in own:
                continue
            if self.held and name not in self.carry_skip:
                free = f"{name}:free"
                modules[free] = shared[name]
                modules[name] = carried(self, name, free, shared[name])
            else:
                modules[name] = shared[name]
        modules.update(own)
        return modules

    def carried_sides(self, pose):
        """The arms the carry pose takes over: the hands holding HELD props
        and any hand it puts on one (hold) or anywhere (reach)."""
        sides = {props.prop_info(n).hand for n in self.held}
        sides |= {s for s in SIDES if pose.get(f"hold.{s}.w") > 0.0 or pose.get(f"reach.{s}.w") > 0.0}
        return sorted(sides)


def carried(unit, name, free, module):
    """A shared clip with the HELD props in hand: the clip as it is (built as
    `free`) under the arms of the unit's CARRY pose, which ride on the chest
    (FK arm angles are measured from it), so the body still breathes,
    walks and falls as the shared clip does."""

    hands = {props.prop_info(n).hand for n in unit.held}
    wield_side = "R" if "R" in hands else "L"

    def build(clip):
        pose = unit.carry(clip)
        sides = unit.carried_sides(pose)
        ik = [s for s in sides if pose.get(f"reach.{s}.w") > 0.0 or (pose.get("wield.w") > 0.0 and s == wield_side)]
        if ik:
            pose = clip.fk(pose, arms=tuple(ik))
        mask = tuple(b for b in GROUPS["all"] if b not in {ARM[s][i] for s in sides for i in (1, 2, 3)})
        clip.base(free, bones=mask)
        clip.key(0.0, pose)

    doc = (module.__doc__ or name).strip().split("\n")[0]
    return SimpleNamespace(
        __doc__=f"{doc} (holding {', '.join(unit.held)})",
        DURATION=None, LOOP=module.LOOP, PROPS=unit.held,
        **{k: getattr(module, k) for k in ("LOCOMOTION", "HIT", "REACH", "BEATS", "SHEET_TIMES")
           if hasattr(module, k)},
        build=build,
    )


def _modules(folder, package):
    """{name: module} of every clip module (a .py not starting with _) in `folder`."""
    out = {}
    if not os.path.isdir(folder):
        return out
    for fname in sorted(os.listdir(folder)):
        if not fname.endswith(".py") or fname.startswith("_"):
            continue
        name = fname[:-3]
        mod = _load_module(f"{package}.{name}", os.path.join(folder, fname))
        for attr in ("DURATION", "LOOP", "build"):
            if not hasattr(mod, attr):
                raise ClipError(f"{os.path.join(folder, fname)} has no {attr}")
        out[name] = mod
    return out

