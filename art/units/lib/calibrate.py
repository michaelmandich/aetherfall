"""Calibration sheets for the pose helper: one labelled test pose per column.

`build_unit.py --unit <id> -- --calibrate DIR` renders these on that unit's
body; each label says what the pose should show, so a wrong sign or axis is
visible at a glance. Re-run it after touching rig.py / pose.py / props.py
grip constants. Props are named by kind (lib/props.py KINDS), so every
unit's calibration shows the whole library.
"""

import os
from types import SimpleNamespace

from mathutils import Vector

from .clip import FRAME, Clip
from .rig import HAND, SIDES
from .sheets import render_columns


SHEETS = {
    "calib_arms": [
        ("neutral", (), lambda p: p),
        ("R arm fwd 90", (), lambda p: p.arm("R", lift=90, toward=0).elbow("R", 0)),
        ("R arm out 90", (), lambda p: p.arm("R", lift=90, toward=90).elbow("R", 0)),
        ("R arm up 180", (), lambda p: p.arm("R", lift=180, toward=0).elbow("R", 0)),
        ("R across -90 lift 60", (), lambda p: p.arm("R", lift=60, toward=-90).elbow("R", 0)),
        ("R elbow 90", (), lambda p: p.arm("R", lift=0).elbow("R", 90)),
        ("R elbow 90 twist +45", (), lambda p: p.arm("R", lift=0, twist=45).elbow("R", 90)),
        ("R fwd 90 elbow 90", (), lambda p: p.arm("R", lift=90, toward=0).elbow("R", 90)),
    ],
    "calib_hands": [
        ("R e90 flex +60", (), lambda p: p.arm("R", lift=0).elbow("R", 90).wrist("R", flex=60)),
        ("R e90 tilt +30", (), lambda p: p.arm("R", lift=0).elbow("R", 90).wrist("R", tilt=30)),
        ("R e90 twist +90", (), lambda p: p.arm("R", lift=0).elbow("R", 90).wrist("R", twist=90)),
        ("L e90 flex +60", (), lambda p: p.arm("L", lift=0).elbow("L", 90).wrist("L", flex=60)),
        ("L e90 twist +90", (), lambda p: p.arm("L", lift=0).elbow("L", 90).wrist("L", twist=90)),
        ("L shoulder up 25", (), lambda p: p.shoulder("L", up=25)),
        ("R reach f.45 u1.2", (), lambda p: p.reach("R", at=(0.1, 0.45, 1.2))),
        ("L reach chest", (), lambda p: p.reach("L", at=(-0.05, 0.35, -0.15), frame="chest")),
    ],
    "calib_body": [
        ("spine bend 40", (), lambda p: p.spine(bend=40)),
        ("spine turn R 45", (), lambda p: p.spine(turn=45)),
        ("spine lean R 25", (), lambda p: p.spine(lean=25)),
        ("head turn R 60 nod 20", (), lambda p: p.head(turn=60, nod=20)),
        ("hips bend 30 up-.1", (), lambda p: p.hips(bend=30, up=-0.1)),
        ("L fwd .3, R heel 35", (), lambda p: p.foot("L", fwd=0.3).foot("R", fwd=-0.15, heel=35).hips(up=-0.06)),
        ("FK R leg 90 knee 90", (), lambda p: p.leg("R", lift=90, knee=90)),
        ("squat up -.35", (), lambda p: p.hips(up=-0.35, fwd=-0.05).spine(bend=25).foot("L", out=0.08, turn=20)
         .foot("R", out=0.08, turn=20)),
    ],
    "calib_props": [
        ("chop hanging", ("felling_axe",), lambda p: p),
        ("chop 2-hand level", ("felling_axe",), lambda p: p.wield(at=(0.0, 0.32, 1.0), pitch=70).hold("L", at=0.12)),
        ("chop 2-hand up", ("felling_axe",), lambda p: p.wield(at=(0.05, 0.15, 1.35), pitch=-30)
         .hold("L", at=0.25).spine(turn=25, bend=-5).hips(turn=20)),
        ("hammer e90", ("hammer",), lambda p: p.arm("R", lift=10).elbow("R", 90)),
        ("basket L", ("basket",), lambda p: p.arm("L", lift=20, toward=10).elbow("L", 70)),
        ("chisel+mallet", ("mallet", "chisel"), lambda p: p.arm("R", lift=30).elbow("R", 100)
         .arm("L", lift=20).elbow("L", 80)),
        ("carry wood", ("load_wood",), lambda p: p.carry(at=(0.0, 0.29, -0.15))),
        ("carry food", ("load_food",), lambda p: p.carry(at=(0.0, 0.29, -0.15))),
    ],
    # Close-ups of the hands (framed on the wrists): grips and palm directions.
    "calib_grips": [
        ("chop 2-hand level", ("felling_axe",), lambda p: p.wield(at=(0.0, 0.32, 1.0), pitch=70).hold("L", at=0.12)),
        ("axe e90", ("work_axe",), lambda p: p.arm("R", lift=10).elbow("R", 90)),
        ("R e90 twist +90 palm up", (), lambda p: p.arm("R", lift=0).elbow("R", 90).wrist("R", twist=90)),
        ("shovel 2-hand", ("shovel",), lambda p: p.spine(bend=30).hips(up=-0.1, bend=10)
         .wield(at=(0.1, 0.2, 1.1), pitch=125).hold("L", at=0.3, roll=180)),
        ("carry wood", ("load_wood",), lambda p: p.carry(at=(0.0, 0.29, -0.15))),
        ("basket L e70", ("basket",), lambda p: p.arm("L", lift=20, toward=10).elbow("L", 70)),
        ("chisel+mallet", ("mallet", "chisel"), lambda p: p.arm("R", lift=30).elbow("R", 100)
         .arm("L", lift=20).elbow("L", 80)),
        ("hoe 2-hand", ("hoe",), lambda p: p.spine(bend=30).hips(up=-0.1, bend=10)
         .wield(at=(0.1, 0.2, 1.1), pitch=125).hold("L", at=0.3, roll=180)),
    ],
    # The soldiers' weapons in their basic holds.
    "calib_weapons": [
        ("spear upright R e90", ("spear",), lambda p: p.arm("R", lift=10).elbow("R", 90)),
        ("spear 2-hand level", ("spear",), lambda p: p.wield(at=(0.12, 0.25, 1.0), pitch=90, roll=90)
         .hold("L", at=0.55)),
        ("bow L hanging", ("bow",), lambda p: p.arm("L", lift=8, toward=70).elbow("L", 14)),
        ("bow L at aim", ("bow",), lambda p: p.arm("L", lift=88, toward=10).elbow("L", 5)),
        ("crossbow 2-hand level", ("crossbow",), lambda p: p.wield(at=(0.1, 0.30, 1.1), pitch=90)
         .hold("L", at=0.25, roll=180)),
        ("sling R hanging", ("sling",), lambda p: p.arm("R", lift=8, toward=70).elbow("R", 14)),
        ("round shield L guard", ("round_shield",), lambda p: p.arm("L", lift=50, toward=20).elbow("L", 90)),
        ("spear + shield", ("spear", "round_shield"), lambda p: p.arm("R", lift=10).elbow("R", 90)
         .arm("L", lift=50, toward=20).elbow("L", 90)),
    ],
}
CLOSE_UP = {"calib_grips"}


def prop_names():
    """Every prop the calibration sheets show (library kinds)."""
    return sorted({n for poses in SHEETS.values() for _, names, _ in poses for n in names})


def _single_pose_clip(ctx, name, prop_names, fn):
    module = SimpleNamespace(DURATION=FRAME, LOOP=False, PROPS=tuple(prop_names),
                             build=lambda clip: clip.key(0.0, fn(clip.pose())))
    clip = Clip(ctx, name, module)
    module.build(clip)
    clip.evaluate()
    clip.bake(ctx.arm)
    return clip


def render(ctx, arm, body, prop_objs, out_dir, scratch, sheet_rig):
    outs = []
    for sheet, poses in SHEETS.items():
        columns, clips = [], []
        for i, (label, prop_names, fn) in enumerate(poses):
            clip = _single_pose_clip(ctx, f"_{sheet}_{i}", prop_names, fn)
            clips.append(clip)
            columns.append((clip.action, 0.0, set(prop_names), label))
            info = {k: round(v, 3) for f in clip.info for k, v in f.items()}
            if info:
                print(f"  {sheet} '{label}': {info}")
        bbox = None
        if sheet in CLOSE_UP:
            wrists = [ctx.rig.fk(c.basis[0])[HAND[s]][0] for c in clips for s in SIDES]
            bbox = (Vector([min(w[i] for w in wrists) - 0.25 for i in range(3)]),
                    Vector([max(w[i] for w in wrists) + 0.25 for i in range(3)]))
        out = os.path.join(out_dir, sheet + ".png")
        render_columns(arm, body, prop_objs, columns, out, scratch, sheet_rig, bbox)
        outs.append(out)
    return outs
