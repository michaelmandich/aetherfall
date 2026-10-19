"""Attack: one overhead Work Axe swing at an enemy in front.

0.00 ready (CombatIdle's pose) -> 0.08 anticipation (a dip, the axe sinks)
-> 0.30 wind-up (weight back, chest coiled right, axe cocked high behind
the head, the wrist cocked back, guard hand pointing at the target) ->
0.50 HIT (weight driven onto the front foot, chest uncoiled, the arm
reaching forward and down, the bit cutting down into the enemy's chest at
full speed) -> 0.64 follow-through down across the body to the left knee
-> 1.00 back to ready.

The axe is placed with wield() on each key and converted to FK numbers,
so the swing travels along the arm's arc. Every key keeps the wrist in its
working range (the haft leaves the fist at the angle a real grip gives it,
so the pitch, yaw and roll of each key were chosen for a straight wrist,
not only for where the axe points). The hips lead the chest and the chest
leads the arm (kinetic chain), and body and arm both accelerate from the
top of the wind-up into the hit, so the blow is fastest as it lands.
"""

from lib.pose import ARMS
from lib.stances import combat_ready, combat_ready_ik

DURATION = 1.0
LOOP = False
HIT = 0.5
PROPS = ("Tool_attack",)
SHEET_TIMES = (0.0, 0.08, 0.2, 0.3, 0.42, 0.5, 0.64, 0.82, 1.0)


def build(clip):
    ready = combat_ready(clip)

    dip = combat_ready_ik().hips(up=-0.10, turn=14).spine(bend=10)
    dip.wield(at=(0.31, 0.22, 1.12), pitch=0, yaw=22, roll=-5)
    dip = clip.fk(dip)

    wind = combat_ready_ik()
    wind.hips(up=-0.085, fwd=-0.045, right=0.02, turn=24, bend=2)
    wind.spine(bend=-6, turn=24, lean=-7)
    wind.wield(at=(0.27, -0.08, 1.68), pitch=-50, yaw=18, roll=0).shoulder("R", up=16).elbow("R", out=10)
    wind.arm("L", lift=80, toward=-6, twist=0).elbow("L", 26).wrist("L", flex=0)
    wind = clip.fk(wind)

    hit = combat_ready_ik()
    hit.hips(up=-0.12, fwd=0.075, right=-0.01, turn=-12, bend=12)
    hit.spine(bend=22, turn=-24, lean=4)
    hit.foot("R", heel=30, turn=24)
    hit.wield(at=(0.08, 0.55, 1.05), pitch=55, yaw=-6, roll=30).shoulder("R", up=0, fwd=14).elbow("R", out=30)
    hit.arm("L", lift=28, toward=150, twist=0).elbow("L", 70)
    hit.head(nod=8)
    hit = clip.fk(hit)

    follow = combat_ready_ik()
    follow.hips(up=-0.125, fwd=0.08, right=-0.01, turn=-18, bend=14).spine(bend=26, turn=-32, lean=5)
    follow.foot("R", heel=34, turn=24)
    follow.wield(at=(0.0, 0.35, 0.85), pitch=120, yaw=-20, roll=60).shoulder("R", fwd=16).elbow("R", out=30)
    follow.arm("L", lift=32, toward=165).elbow("L", 75).head(nod=10)
    follow = clip.fk(follow)

    settle = combat_ready_ik().hips(up=-0.09, fwd=0.02, turn=4).spine(bend=12, turn=-12)
    settle.wield(at=(0.30, 0.32, 1.08), pitch=20, yaw=0, roll=-5).elbow("R", out=0)
    settle = clip.fk(settle)

    clip.key(0.0, ready)
    clip.key(0.08, dip)
    # Wind-up: the body arrives first, the arm settles into the cock a beat
    # later. From there both accelerate all the way into the hit.
    clip.key(0.26, wind, skip=ARMS, interp="QUAD", easing="EASE_IN")
    clip.key(0.30, wind, parts=ARMS, interp="QUAD", easing="EASE_IN")
    # The blow lands; body and arm carry on, slowing, into the follow-through.
    clip.key(HIT, hit, interp="QUAD", easing="EASE_OUT")
    clip.key(0.62, follow, skip=ARMS)
    clip.key(0.64, follow, parts=ARMS)
    clip.key(0.82, settle)
    clip.key(1.0, ready)
