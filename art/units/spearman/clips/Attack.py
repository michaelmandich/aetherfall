"""Attack: one two-handed spear thrust at an enemy in front.

0.00 ready (CombatIdle's pose) -> 0.08 anticipation (a dip) -> 0.26 draw
(weight back, chest turned further right, the spear pulled back through
the left hand, which slides up the haft) -> 0.40 HIT (the hips drive
forward onto the front foot, the rear heel lifts, the chest uncoils and
the right hand drives the spear through the left, which slides back down
the haft: the point arrives at the enemy's chest at full speed) -> 0.50
full extension -> 0.90 back to ready.

The spear is placed with wield() on each key and converted to FK numbers,
so the right arm travels along arcs; the left hand holds the haft (IK) at a
keyed distance (hold `at`: the slide). Body keys lead the arm keys (kinetic
chain), and both accelerate from the draw into the hit. The angles came
from a search for straight wrists on the spearman's Meshy body.
"""

from lib.clip import FRAME as T
from lib.pose import ARMS
from lib.stances import spear_ready, spear_ready_ik

DURATION = 27 * T
LOOP = False
HIT = 12 * T           # the Bronze Spear's windup (assets/data/core/weapons.ron): 0.4 s
PROPS = ("Gear_spear",)
SHEET_TIMES = (0.0, 0.08, 0.17, 0.26, 0.33, 0.4, 0.5, 0.7, 0.9)


def build(clip):
    ready = spear_ready(clip)

    dip = spear_ready_ik().hips(up=-0.12, turn=23).spine(bend=8, turn=19)
    dip.wield(at=(0.21, -0.07, 0.92), pitch=81, yaw=-6, roll=-30).hold("L", at=0.43, roll=-45)
    dip = clip.fk(dip)

    draw = spear_ready_ik()
    draw.hips(up=-0.10, fwd=-0.04, turn=21, bend=4).spine(bend=4, turn=17.5, lean=-3)
    draw.wield(at=(0.21, -0.20, 0.94), pitch=82, yaw=-5, roll=-30).hold("L", at=0.52, roll=-45)
    draw = clip.fk(draw)

    hit = spear_ready_ik()
    hit.hips(up=-0.13, fwd=0.12, turn=3, bend=12).spine(bend=12, turn=2.5)
    hit.foot("R", heel=30, turn=35)
    hit.wield_part("Gear_spear", "tip", (0.0, 1.62, 1.15), pitch=80, yaw=-10, roll=0).hold("L", at=0.28, roll=-45)
    hit.head(nod=6)
    hit = clip.fk(hit)

    reach = spear_ready_ik()
    reach.hips(up=-0.135, fwd=0.14, turn=1, bend=13).spine(bend=13, turn=1)
    reach.foot("R", heel=32, turn=35)
    reach.wield_part("Gear_spear", "tip", (0.0, 1.68, 1.13), pitch=81, yaw=-9, roll=2).hold("L", at=0.25, roll=-45)
    reach.head(nod=8)
    reach = clip.fk(reach)

    back = spear_ready_ik().hips(up=-0.10, fwd=0.04, turn=12).spine(bend=8, turn=10)
    back.wield(at=(0.21, 0.06, 0.92), pitch=80, yaw=-6, roll=-15).hold("L", at=0.34, roll=-45)
    back = clip.fk(back)

    clip.key(0.0, ready)
    clip.key(0.08, dip)
    # The draw: the body arrives first, the arms a beat later; from there
    # both accelerate all the way into the hit.
    clip.key(0.22, draw, skip=ARMS, interp="QUAD", easing="EASE_IN")
    clip.key(0.26, draw, parts=ARMS, interp="QUAD", easing="EASE_IN")
    # The point lands; body and arms carry on, slowing, to full extension.
    clip.key(HIT, hit, interp="QUAD", easing="EASE_OUT")
    clip.key(0.50, reach)
    clip.key(0.70, back)
    clip.key(DURATION, ready)
