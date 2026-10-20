"""Attack: one diagonal stroke of the lightblade at an enemy in front.

0.00 ready (CombatIdle's pose) -> 0.06 anticipation (a dip) -> 0.22
wind-up (weight back, chest coiled right, the blade cocked high behind the
head, the guard hand pointing at the target) -> 0.40 HIT (weight driven
onto the front foot, the chest uncoiled, the edge cutting down across the
enemy's chest at full speed) -> 0.52 follow-through down across the body
to the left knee -> 1.00 back to ready.

Built like the peasant's axe stroke: wield() on each key, converted to FK
numbers so the blade travels along the arm's arc; the hips lead the chest
and the chest the arm, and both accelerate from the top of the wind-up
into the hit. The angles came from a search for straight wrists on the
lumen knight's Meshy body.
"""

from lib.clip import FRAME as T
from lib.pose import ARMS
from lib.stances import sword_ready, sword_ready_ik

DURATION = 1.0
LOOP = False
HIT = 12 * T            # the lightblade's windup (assets/data/factions/starborn.ron): 0.4 s
PROPS = ("Gear_sword",)
SHEET_TIMES = (0.0, 0.06, 0.14, 0.22, 0.32, 0.4, 0.52, 0.75, 1.0)


def build(clip):
    ready = sword_ready(clip)

    dip = sword_ready_ik().hips(up=-0.10, turn=14).spine(bend=10)
    dip.wield(at=(0.27, 0.24, 1.18), pitch=20, yaw=25, roll=25)
    dip = clip.fk(dip)

    wind = sword_ready_ik()
    wind.hips(up=-0.085, fwd=-0.045, right=0.02, turn=24, bend=2).spine(bend=-6, turn=24, lean=-7)
    wind.wield(at=(0.32, -0.12, 1.85), pitch=-45, yaw=10, roll=-15).shoulder("R", up=16).elbow("R", out=10)
    wind.arm("L", lift=80, toward=-6, twist=0).elbow("L", 26).wrist("L", flex=0)
    wind = clip.fk(wind)

    hit = sword_ready_ik()
    hit.hips(up=-0.12, fwd=0.075, right=-0.01, turn=-12, bend=12).spine(bend=22, turn=-24, lean=4)
    hit.foot("R", heel=30, turn=24)
    hit.wield_part("Gear_sword", "edge", (0.0, 1.4, 1.15), pitch=70, yaw=0, roll=40)
    hit.shoulder("R", up=0, fwd=14).elbow("R", out=30)
    hit.arm("L", lift=28, toward=150, twist=0).elbow("L", 70).head(nod=8)
    hit = clip.fk(hit)

    follow = sword_ready_ik()
    follow.hips(up=-0.125, fwd=0.08, right=-0.01, turn=-18, bend=14).spine(bend=26, turn=-32, lean=5)
    follow.foot("R", heel=34, turn=24)
    follow.wield_part("Gear_sword", "edge", (-0.25, 1.05, 0.7), pitch=110, yaw=-25, roll=50)
    follow.shoulder("R", fwd=16).elbow("R", out=30)
    follow.arm("L", lift=32, toward=165).elbow("L", 75).head(nod=10)
    follow = clip.fk(follow)

    settle = sword_ready_ik().hips(up=-0.09, fwd=0.02, turn=4).spine(bend=12, turn=-12)
    settle.wield(at=(0.30, 0.32, 1.15), pitch=30, yaw=10, roll=10).elbow("R", out=0)
    settle = clip.fk(settle)

    clip.key(0.0, ready)
    clip.key(0.06, dip)
    # Wind-up: the body arrives first, the arm settles into the cock a beat
    # later. From there both accelerate all the way into the hit.
    clip.key(0.18, wind, skip=ARMS, interp="QUAD", easing="EASE_IN")
    clip.key(0.22, wind, parts=ARMS, interp="QUAD", easing="EASE_IN")
    # The blade lands; body and arm carry on, slowing, into the follow-through.
    clip.key(HIT, hit, interp="QUAD", easing="EASE_OUT")
    clip.key(0.50, follow, skip=ARMS)
    clip.key(0.52, follow, parts=ARMS)
    clip.key(0.75, settle)
    clip.key(1.0, ready)
