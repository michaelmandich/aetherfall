"""CombatIdle: spear levelled at the enemy, between thrusts.

A springy guard: the body sinks and rises once per loop, the arms follow a
beat later (overlap) and the spear point dips with them, the eyes stay on
the enemy.
"""

from lib.pose import ARMS
from lib.stances import spear_ready, spear_ready_ik

DURATION = 1.5
LOOP = True
PROPS = ("Gear_spear",)


def build(clip):
    ready = spear_ready(clip)
    low = spear_ready_ik().hips(up=-0.13, fwd=0.02, turn=19).spine(bend=10, turn=16)
    low.wield(at=(0.21, -0.02, 0.89), pitch=78, yaw=-7, roll=-35)
    low = clip.fk(low)

    clip.key(0.0, ready)
    clip.key(0.7, low, skip=ARMS)
    clip.key(0.84, low, parts=ARMS)
