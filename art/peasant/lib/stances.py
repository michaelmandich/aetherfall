"""Shared starting poses, so clips that the engine blends between agree.

Each function returns a fresh `Pose`; modify the copy in the clip.
"""

from .pose import Pose

# The target of a melee swing: 1.6 m in front, chest height.
ENEMY = (0.0, 1.6, 1.15)


def relaxed():
    """Standing at ease (Idle's first frame)."""
    p = Pose().hips(up=-0.025).elbow("L", 14).elbow("R", 14)
    p.arm("L", lift=7, toward=70).arm("R", lift=7, toward=70)
    return p.foot("L", out=0.02, turn=4).foot("R", out=0.02, turn=6)


def combat_ready_ik():
    """Fighting stance with the Work Axe (Tool_attack) in the right hand:
    left foot forward, knees bent, the axe raised by the right shoulder
    (head up, outside the head's silhouette, the wrist straight), left hand
    up as a guard, eyes on the enemy. The axe is placed with wield() (IK)."""
    p = Pose()
    p.foot("L", fwd=0.15, out=0.03, turn=-5, knee=8)
    p.foot("R", fwd=-0.13, out=0.07, turn=28, knee=10)
    p.hips(up=-0.08, fwd=0.0, turn=12, bend=6)
    p.spine(bend=8, turn=-8, lean=0)
    p.look(ENEMY)
    p.wield(at=(0.30, 0.20, 1.20), pitch=-5, yaw=20, roll=-5)
    p.shoulder("R", up=4).elbow("R", out=5)
    p.arm("L", lift=42, toward=8, twist=10).elbow("L", 95).wrist("L", flex=10)
    return p


def combat_ready(clip):
    """combat_ready_ik() with the axe arm converted to FK numbers, so swings
    keyed from it travel along arcs. `clip` must hold Tool_attack."""
    return clip.fk(combat_ready_ik())


# Load held between both hands, in the chest frame (metres from Spine2).
CARRY_AT = (0.0, 0.29, -0.15)


def carrying():
    """Standing with cargo hugged at the chest (CarryIdle's first frame)."""
    p = relaxed().carry(at=CARRY_AT, pitch=0.0)
    p.spine(bend=-5).hips(fwd=0.015).elbow("L", out=10).elbow("R", out=10)
    return p
