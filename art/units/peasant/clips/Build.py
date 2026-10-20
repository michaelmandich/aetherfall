"""Build: standing hammer strikes on a foundation in front, waist height.

Staggered stance, knees soft, the left hand steadying the work (IK, so it
stays put), eyes on the spot the hammer lands. One full strike per loop,
starting at the top so the first blow lands soon after the engine starts
the clip:

0.00 cocked by the ear (weight back, chest coiled right, elbow high, the
hammer head back over the shoulder): the hips start down at once and
accelerate all the way into the blow -> 0.10 the arm still hangs back at
the top, then follows (kinetic chain: hips, then chest, then the arm) ->
0.23 the hand leads, the hammer head trails -> 0.33 HIT (the face flat on
the work, chest folded over it, weight forward; body and hammer fastest
here) -> 0.42 rebound (the hammer bounces, the body sinks on through) ->
0.82 lift (weight rocks back, the chest uncoils and rises, the elbow leads
the hammer up) -> 1.20 cocked.

The hammer arm is keyed in FK terms, so every swing stays in the
shoulder's plane and travels along the arm's arc; the wrist cocks back
(towards the thumb) at the top and snaps through to the little-finger
side at the hit, which is what turns a hammer from pointing up to lying
flat on the work.
"""

from lib.clip import FRAME as T
from lib.pose import arm_parts
from lib.stances import relaxed

DURATION = 1.2
LOOP = True
HIT = 10 * T  # the hammer lands (manifest `hit`, e.g. for a knock sound)
PROPS = ("Tool_build",)
SHEET_TIMES = (0.0, 0.1, 0.18, 7 * T, 0.28, HIT, 0.42, 0.62, 0.82, 1.02)

# Where the eyes are on the work, and where the left hand steadies it
# (right, fwd, up) metres in the character's frame.
WORK = (0.06, 0.70, 0.77)
REACH = WORK[1]  # the work surface lies this far ahead (manifest `reach`): the engine steps up to it
STEADY = (-0.07, 0.38, 0.86)

ARM = arm_parts("R")


def stance():
    """Feet, left hand and eyes: the same on every key."""
    p = relaxed()
    p.foot("L", fwd=0.14, out=0.04, turn=-4, knee=4).foot("R", fwd=-0.12, out=0.08, turn=24, knee=6)
    p.reach("L", at=STEADY).wrist("L", flex=-50, twist=-85).elbow("L", out=30)
    p.look(WORK)
    return p


def build(clip):
    hit = stance().hips(up=-0.11, fwd=0.045, turn=-8, bend=12).spine(bend=28, turn=-10, lean=2)
    hit.shoulder("R", fwd=10).arm("R", lift=30, toward=20, twist=20).elbow("R", 75)
    hit.wrist("R", flex=20, tilt=-40, twist=0)

    rebound = stance().hips(up=-0.122, fwd=0.05, turn=-9, bend=13).spine(bend=30, turn=-11, lean=2)
    rebound.shoulder("R", fwd=10).arm("R", lift=35, toward=20, twist=20).elbow("R", 84)
    rebound.wrist("R", flex=16, tilt=-22, twist=4)

    lift = stance().hips(up=-0.08, fwd=0.0, turn=4, bend=8).spine(bend=17, turn=8, lean=-2)
    lift.shoulder("R", up=6, fwd=4).arm("R", lift=50, toward=15, twist=20).elbow("R", 110)
    lift.wrist("R", flex=20, tilt=-30, twist=30)

    top = stance().hips(up=-0.055, fwd=-0.035, turn=11, bend=8).spine(bend=12, turn=18, lean=-4)
    top.shoulder("R", up=14).arm("R", lift=105, toward=20, twist=60).elbow("R", 110)
    top.wrist("R", flex=20, tilt=-30, twist=60)

    hang = stance().hips(up=-0.08, fwd=-0.01, turn=6, bend=11).spine(bend=18, turn=14, lean=-3)
    hang.shoulder("R", up=15).arm("R", lift=110, toward=22, twist=64).elbow("R", 116)
    hang.wrist("R", flex=22, tilt=-30, twist=62)

    # Mid-strike: the upper arm has come down and forward, the elbow is
    # about to open: the hammer head still trails up behind the hand (whip).
    mid = hang.copy().shoulder("R", up=6, fwd=6).arm("R", lift=90, toward=15, twist=40).elbow("R", 110)
    mid.wrist("R", flex=20, tilt=-30, twist=30)

    # The hips and chest start the strike from the top while the arm still
    # hangs back; both accelerate into the blow and land together, then the
    # body decelerates on through it into the rebound.
    clip.key(0.0, top, skip=ARM, interp="QUAD", easing="EASE_IN")
    clip.key(0.0, top, parts=ARM)
    clip.key(0.10, hang, parts=ARM, interp="QUAD", easing="EASE_IN")
    clip.key(7 * T, mid, parts=ARM, interp="LINEAR")
    clip.key(HIT, hit, skip=ARM, interp="QUAD", easing="EASE_OUT")
    clip.key(HIT, hit, parts=ARM)
    clip.key(0.42, rebound)
    clip.key(0.82, lift)
