"""Repair: kneeling on the right knee, hammering the work low in front.

Right knee and right toes on the ground, left foot planted forward, left
hand braced on the left knee, eyes on the work: a sill at shin height
(0.22 m) in front of the left foot. The pelvis stays put (it sits on the
kneeling leg); the strikes come from the chest and the arm. Two blows per
loop, a full blow and a light tap ("BANG ... tap"), both landing the
hammer face on the same spot, starting at the top so the first blow lands
soon after the engine starts the clip:

0.00 full wind-up (chest risen and coiled right, hammer cocked over the
shoulder) -> 0.10 hang -> 0.27 the hand leads, the head trails -> 0.37
BANG (chest folded over the work) -> 0.47 rebound -> 0.67 a short lift
from the elbow and wrist, the chest barely rising -> 0.83 tap (the same
contact pose as the blow) -> 0.93 rebound -> 1.17 lifting into the
wind-up again.

Keys sit on whole 30 fps frames, so the contacts are sampled exactly. The
hammer arm is keyed in FK terms so swings stay in the shoulder's plane
(see Build); the wrist snaps from cocked to flat at each blow.
"""

from lib.clip import FRAME as T
from lib.pose import arm_parts
from lib.stances import relaxed

DURATION = 1.4
LOOP = True
HIT = 11 * T  # the full blow lands (manifest `hit`)
TAP = 25 * T  # the light tap lands on the same spot
PROPS = ("Tool_build",)
SHEET_TIMES = (0.0, 3 * T, 6 * T, 8 * T, HIT, 14 * T, 20 * T, TAP, 28 * T, 35 * T)

# Where the hammer face meets the work, and the left knee the hand rests
# on, (right, fwd, up) metres in the character's frame.
WORK = (0.12, 0.62, 0.22)
REACH = WORK[1]  # the work surface lies this far ahead (manifest `reach`): the engine steps up to it
KNEE_REST = (-0.16, 0.27, 0.60)

ARM = arm_parts("R")


def kneel():
    """Down on the right knee, left foot forward, left hand on the left knee."""
    p = relaxed().hips(up=-0.462, fwd=-0.05)
    p.foot("R", fwd=-0.48, lift=-0.010, heel=70, knee=-80).toe("R", bend=20)
    p.foot("L", fwd=0.38, out=0.04, turn=5)
    p.reach("L", at=KNEE_REST).wrist("L", flex=25, twist=-70).elbow("L", out=40)
    p.look(WORK)
    return p


def body_hit():
    return kneel().hips(bend=14, turn=-4).spine(bend=40, turn=-10, lean=2).shoulder("R", fwd=10)


def body_top():
    return kneel().hips(bend=6, turn=4).spine(bend=16, turn=18, lean=-4).shoulder("R", up=14)


def contact():
    """The hammer face flat on the work: both blows land in this pose."""
    return body_hit().arm("R", lift=50, toward=20, twist=20).elbow("R", 50).wrist("R", flex=20, tilt=-25, twist=0)


def build(clip):
    bang = contact()
    rebound = body_hit().hips(bend=15).spine(bend=42).arm("R", lift=54, toward=20, twist=20).elbow("R", 62)
    rebound.wrist("R", flex=18, tilt=-10, twist=2)

    # The tap: a short lift from the elbow and wrist, the chest barely rises,
    # then down onto the same spot as the blow.
    tap_up = body_hit().hips(bend=13, turn=-3).spine(bend=34, turn=-8, lean=1).shoulder("R", fwd=8)
    tap_up.arm("R", lift=58, toward=12, twist=24).elbow("R", 84).wrist("R", flex=20, tilt=-14, twist=0)
    tap = contact()
    tap_rebound = body_hit().hips(bend=15).spine(bend=41).arm("R", lift=52, toward=20, twist=20).elbow("R", 58)
    tap_rebound.wrist("R", flex=18, tilt=-12, twist=2)

    # The full blow: chest rises and coils right, elbow leads the hammer up.
    lift = kneel().hips(bend=9, turn=1).spine(bend=26, turn=6, lean=-2).shoulder("R", up=6, fwd=4)
    lift.arm("R", lift=75, toward=12, twist=35).elbow("R", 100).wrist("R", flex=20, tilt=-15, twist=15)
    top = body_top().arm("R", lift=105, toward=10, twist=45).elbow("R", 90).wrist("R", flex=20, tilt=-15, twist=30)
    hang = body_top().hips(bend=8, turn=2).spine(bend=20, turn=15, lean=-3).shoulder("R", up=15)
    hang.arm("R", lift=110, toward=10, twist=48).elbow("R", 96).wrist("R", flex=22, tilt=-15, twist=32)
    # Mid-strike: the hand leads, the hammer head trails up behind it.
    mid = hang.copy().shoulder("R", up=6, fwd=6).arm("R", lift=105, toward=15, twist=20).elbow("R", 80)
    mid.wrist("R", flex=0, tilt=0, twist=15)

    # The chest starts down from the top at once while the arm still hangs
    # back, then the arm whips; both accelerate into the blow and land
    # together, and the chest decelerates on through it into the rebound.
    clip.key(0.0, top, skip=ARM, interp="QUAD", easing="EASE_IN")
    clip.key(0.0, top, parts=ARM)
    clip.key(3 * T, hang, parts=ARM, interp="QUAD", easing="EASE_IN")
    clip.key(8 * T, mid, parts=ARM, interp="LINEAR")
    clip.key(HIT, bang, skip=ARM, interp="QUAD", easing="EASE_OUT")
    clip.key(HIT, bang, parts=ARM)
    clip.key(14 * T, rebound)
    clip.key(20 * T, tap_up, interp="QUAD", easing="EASE_IN")
    clip.key(TAP, tap)
    clip.key(28 * T, tap_rebound)
    clip.key(35 * T, lift)
