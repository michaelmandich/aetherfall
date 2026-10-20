"""Shared starting poses, so clips that the engine blends between agree
(and units with the same weapon start from the same stance).

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


# -- the two-handed spear (the spearman's `Gear_spear`: Meshy's short_spear,
# or the boxy spear standing in) --
# Angles and points from a search for straight wrists (README "Wrists stay
# in range") on the spearman's Meshy body (1.70 m, A-pose rig).


def spear_carry():
    """Standing and walking: the spear in the right fist by the hip, sloped
    25 degrees forward and turned out so its butt clears the thigh, the
    forearm level (straight wrist). The CARRY pose of
    the shared clips: only its right arm is used."""
    return Pose().wield(at=(0.26, 0.20, 1.14), pitch=25, yaw=-20, roll=-15)


def spear_ready_ik():
    """Fighting stance with the spear levelled at the enemy, 10 degrees up:
    left foot forward, knees bent, body turned right so the left shoulder
    leads, right hand at the hip, left hand 0.4 m up the haft, eyes on the
    enemy. The spear is placed with wield() (IK)."""
    p = Pose()
    p.foot("L", fwd=0.17, out=0.03, turn=-5, knee=8)
    p.foot("R", fwd=-0.15, out=0.08, turn=35, knee=10)
    p.hips(up=-0.09, turn=21, bend=6).spine(bend=6, turn=17.5)
    p.look(ENEMY)
    return p.wield(at=(0.21, -0.04, 0.92), pitch=80, yaw=-6, roll=-30).hold("L", at=0.4, roll=-45)


def spear_ready(clip):
    """spear_ready_ik() with the right arm converted to FK numbers."""
    return clip.fk(spear_ready_ik())


# -- the crossbow (the crossbowman's `Gear_crossbow`: Meshy's crossbow, or
# the boxy one standing in), searched on the crossbowman's Meshy body --

# Where a ranged unit aims: far ahead at chest height.
FAR = (0.0, 8.0, 1.3)


def crossbow_carry():
    """Standing and walking: the crossbow across the chest in both hands,
    pointing down to the front-left (right hand on the grip, left under the
    fore-end). Held upright by the shoulder its prod swept through the head,
    and held at the side it hit the legs."""
    return Pose().wield(at=(0.06, 0.24, 1.12), pitch=135, yaw=-35, roll=180).hold("L", at=0.12, roll=90)


def crossbow_ready_ik():
    """Fighting between shots: left foot forward, knees soft, the crossbow
    in both hands at the chest, its head lowered 25 degrees (right hand on
    the grip, left under the fore-end), eyes on the target. IK."""
    p = Pose()
    p.foot("L", fwd=0.17, out=0.03, turn=-5, knee=8)
    p.foot("R", fwd=-0.15, out=0.08, turn=35, knee=10)
    p.hips(up=-0.07, turn=18, bend=4).spine(bend=4, turn=8)
    p.look(FAR)
    return p.wield(at=(0.12, 0.22, 1.22), pitch=115, yaw=-10, roll=180).hold("L", at=0.2, roll=60)


def crossbow_aim_ik():
    """crossbow_ready_ik() raised to the shoulder and levelled at the target
    (canted 30 degrees, which keeps the right wrist straight)."""
    p = crossbow_ready_ik().hips(up=-0.05, turn=20, bend=2).spine(bend=-2, turn=10)
    return p.wield(at=(0.14, 0.24, 1.42), pitch=90, yaw=0, roll=150).hold("L", at=0.14, roll=90)


def crossbow_ready(clip):
    """crossbow_ready_ik() with the right arm converted to FK numbers."""
    return clip.fk(crossbow_ready_ik())


# -- the one-handed sword (the lumen knight's `Gear_sword`: the Starborn
# lightblade, its boxy stand-in until Meshy's arrives), searched on the
# lumen knight's Meshy body (1.80 m) --


def sword_carry():
    """Standing and walking: the blade upright before the right shoulder,
    sloped 25 degrees forward and a little out, the forearm level (straight
    wrist)."""
    return Pose().wield(at=(0.24, 0.30, 1.20), pitch=25, yaw=-20, roll=-15)


def sword_ready_ik():
    """Fighting stance: left foot forward, knees bent, the blade raised
    forward by the right shoulder, left hand up as a guard, eyes on the
    enemy. IK."""
    p = Pose()
    p.foot("L", fwd=0.15, out=0.03, turn=-5, knee=8)
    p.foot("R", fwd=-0.13, out=0.07, turn=28, knee=10)
    p.hips(up=-0.08, turn=12, bend=6).spine(bend=8, turn=-8)
    p.look(ENEMY)
    p.arm("L", lift=42, toward=8, twist=10).elbow("L", 95).wrist("L", flex=10)
    return p.wield(at=(0.26, 0.26, 1.25), pitch=15, yaw=25, roll=30)


def sword_ready(clip):
    """sword_ready_ik() with the sword arm converted to FK numbers."""
    return clip.fk(sword_ready_ik())
