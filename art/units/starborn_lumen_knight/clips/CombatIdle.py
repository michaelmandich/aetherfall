"""CombatIdle: fighting, between strokes, the lightblade raised.

A springy guard: the body sinks and rises once per loop, the sword arm
follows a beat later and the blade dips and rocks with it, the guard hand
bobs, the eyes stay on the enemy.
"""

from lib.pose import ARMS
from lib.stances import sword_ready, sword_ready_ik

DURATION = 1.5
LOOP = True
PROPS = ("Gear_sword",)


def build(clip):
    ready = sword_ready(clip)
    low = sword_ready_ik().hips(up=-0.13, fwd=0.03, turn=8).spine(bend=14, turn=-5)
    low.wield(at=(0.27, 0.27, 1.17), pitch=20, yaw=25, roll=25)
    low = clip.fk(low)
    low.arm("L", lift=33).elbow("L", 106)

    clip.key(0.0, ready)
    clip.key(0.7, low, skip=ARMS)
    clip.key(0.84, low, parts=ARMS)
