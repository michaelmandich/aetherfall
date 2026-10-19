"""CombatIdle: fighting, between swings, Work Axe raised.

A springy guard: the body sinks and rises once per loop, the axe arm
follows a beat later (overlap) and the axe head dips and rocks with it,
the guard hand bobs, the eyes stay on the enemy.
"""

from lib.pose import ARMS
from lib.stances import combat_ready, combat_ready_ik

DURATION = 1.5
LOOP = True
PROPS = ("Tool_attack",)


def build(clip):
    ready = combat_ready(clip)
    low = combat_ready_ik().hips(up=-0.13, fwd=0.03, turn=8).spine(bend=14, turn=-5)
    low.wield(at=(0.30, 0.23, 1.11), pitch=4, yaw=24, roll=-10)
    low = clip.fk(low)
    low.arm("L", lift=33).elbow("L", 106)

    clip.key(0.0, ready)
    clip.key(0.7, low, skip=ARMS)
    clip.key(0.84, low, parts=ARMS)
