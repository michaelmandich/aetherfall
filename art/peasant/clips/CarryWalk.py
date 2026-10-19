"""CarryWalk: walking with cargo hugged at the chest.

Legs and pelvis are Walk's (slowed to a heavier 1.1 s stride); the upper
body is posed: leaning back a little, the chest held steady against the
pelvis's twist so the load stays square, arms holding the load (IK).
"""

from lib.stances import CARRY_AT, carrying

DURATION = 1.1
LOOP = True
LOCOMOTION = True
PROPS = ("Load_wood",)
SHEET_PROPS = ("Load_food", "Load_wood", "Load_stone", "Load_copper", "Load_iron", "Load_silicon",
               "Load_aether", "Load_adamant")


def build(clip):
    clip.base("Walk", bones="lower")
    r, f, u = CARRY_AT
    hold = carrying().spine(bend=-4, steady=0.8).look((0.0, 4.0, 1.2), weight=0.6)
    # The load settles a touch just after each footfall (Walk's pelvis is
    # lowest at 23 % and 72 % of the cycle), so it lags the body.
    settle = hold.copy().carry(at=(r, f, u - 0.014)).spine(bend=-3)
    clip.key(0.0, hold)
    clip.key(0.33, settle)
    clip.key(0.60, hold)
    clip.key(0.88, settle)
