"""CarryIdle: standing with cargo hugged at the chest.

The load is held between both hands (any Load_*: they share one grip).
The body leans back a little against the weight; one breath per loop
lifts the chest, the load and the shoulders; a small hitch of the load
(re-gripping) near the end of the out-breath.
"""

from lib.stances import CARRY_AT, carrying

DURATION = 2.0
LOOP = True
PROPS = ("Load_wood",)
SHEET_PROPS = ("Load_food", "Load_wood", "Load_stone", "Load_copper", "Load_iron", "Load_silicon",
               "Load_aether", "Load_adamant")


def build(clip):
    hold = carrying()
    r, f, u = CARRY_AT
    breathe = hold.copy().spine(bend=-6.5).shoulder("L", up=2.5).shoulder("R", up=2.5)
    breathe.carry(at=(r, f + 0.005, u + 0.012)).hips(up=-0.02)
    hitch = hold.copy().carry(at=(r, f + 0.01, u + 0.025), pitch=-4).spine(bend=-5.5).hips(up=-0.032)

    clip.key(0.0, hold)
    clip.key(0.8, breathe)
    clip.key(1.45, hold)
    clip.key(1.65, hitch)
