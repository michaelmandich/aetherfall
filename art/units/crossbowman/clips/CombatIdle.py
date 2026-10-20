"""CombatIdle: crossbow lowered at the chest, between shots.

The body sinks and rises once per loop, the arms follow a beat later and
the crossbow's head dips with them; the eyes stay on the target.
"""

from lib.pose import ARMS
from lib.stances import crossbow_ready, crossbow_ready_ik

DURATION = 1.5
LOOP = True
PROPS = ("Gear_crossbow",)


def build(clip):
    ready = crossbow_ready(clip)
    low = crossbow_ready_ik().hips(up=-0.10, fwd=0.02, turn=17).spine(bend=7, turn=7)
    low.wield(at=(0.12, 0.23, 1.19), pitch=118, yaw=-10, roll=180)
    low = clip.fk(low)

    clip.key(0.0, ready)
    clip.key(0.7, low, skip=ARMS)
    clip.key(0.84, low, parts=ARMS)
