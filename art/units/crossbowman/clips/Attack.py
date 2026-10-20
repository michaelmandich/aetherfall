"""Attack: raise the crossbow, aim and shoot.

0.00 lowered (CombatIdle's pose) -> 0.30 raised to the shoulder and
levelled at the target, the body first, the arms a beat later, slowing as
they arrive -> held still, aiming -> 0.50 HIT: the bolt is loosed -> 0.57
recoil (the head kicks up, the shoulders rock back) -> 0.75 recovered to
the aim -> 1.10 lowered again.

The crossbow is placed with wield() on each key and converted to FK
numbers; the left hand holds the fore-end (IK). The angles came from a
search for straight wrists on the crossbowman's Meshy body.
"""

from lib.clip import FRAME as T
from lib.pose import ARMS
from lib.stances import crossbow_aim_ik, crossbow_ready

DURATION = 33 * T
LOOP = False
HIT = 15 * T            # the crossbow's windup (assets/data/core/weapons.ron): 0.5 s
PROPS = ("Gear_crossbow",)
BEATS = (0.4,)          # the held aim before the shot
SHEET_TIMES = (0.0, 0.12, 0.22, 0.3, 0.42, 0.5, 0.57, 0.75, 1.1)


def build(clip):
    ready = crossbow_ready(clip)
    aim = clip.fk(crossbow_aim_ik())

    steady = crossbow_aim_ik().hips(up=-0.055)
    steady.wield(at=(0.14, 0.24, 1.415), pitch=90, yaw=0, roll=150)
    steady = clip.fk(steady)

    kick = crossbow_aim_ik().hips(up=-0.05, fwd=-0.03, turn=20, bend=0).spine(bend=-5, turn=12, lean=-2)
    kick.wield(at=(0.14, 0.20, 1.45), pitch=86, yaw=0, roll=160).hold("L", at=0.16, roll=90)
    kick.shoulder("R", up=4).head(nod=-3)
    kick = clip.fk(kick)

    clip.key(0.0, ready)
    # Raising: the body arrives first, the arms a beat later, slowing into the aim.
    clip.key(0.26, aim, skip=ARMS, interp="SINE", easing="EASE_OUT")
    clip.key(0.30, aim, parts=ARMS, interp="SINE", easing="EASE_OUT")
    clip.key(0.42, steady)
    # The shot: the kick is sudden, the recovery slow.
    clip.key(HIT, aim, interp="CUBIC", easing="EASE_OUT")
    clip.key(0.57, kick)
    clip.key(0.75, aim)
    clip.key(DURATION, ready)
