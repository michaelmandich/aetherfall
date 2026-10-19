"""Chop: two-handed felling-axe swings at a trunk in front (gathering wood).

The right hand holds the butt of the haft, so the axe is swung from the
LEFT shoulder (the left hand is the sliding one, as for a woodcutter whose
fixed hand is at the knob). Each blow is a diagonal downward cut into the
trunk's front-left face, the edge leading all the way down:

0.00 the axe, out of the cut, is already rising and swings up and back
past the left side of the face (one smooth backswing, no stop) -> 0.50
wind-up: weight back on the left foot, hips and chest coiled hard left,
the axe cocked high behind the left shoulder, the left hand slid up the
haft -> 0.57 the hips and chest start to unwind while the axe still hangs
at the top -> 0.67 the arms follow, body and arms accelerating all the
way down -> 0.97 HIT: the weight driven onto the right foot, knees sunk,
chest turned square, the left hand slid down to the right, the bit
cutting down and in at waist height at full speed -> 1.07 the axe sticks
while the body sinks on through the blow -> 1.23 the bit is backed out of
the cut -> 1.40 clear of the trunk, turning up into the next backswing.

Every key on the swing is placed on one swing plane (`swing`): the haft
along the plane's radius, the edge along its tangent, so the edge points
where the bit travels (within about 30 degrees all the way down). The
keys are placed with IK (wield/hold) and the axe arm is converted to FK,
so the swing travels in arcs while the left hand stays on the haft; from
the impact until the wrench the bit is held in the cut by IK. Feet stay
planted (IK); the head watches the cut.
"""

import math

from lib.clip import FRAME as T
from lib.pose import ARMS
from lib.props import prop_info
from lib.rig import UP, at, from_world

DURATION = 1.6
LOOP = True
HIT = 29 * T
PROPS = ("Tool_chop",)
# The trunk: a 32 cm log standing 1.4 m ahead, a little to the right (its
# centre (right, fwd), metres), an arm and an axe's length away. The bit
# cuts into its front-left face at waist height, travelling CUT_YAW degrees
# right of forward and CUT_DOWN degrees below the horizontal (a diagonal
# downward cut); the haft lies across the face, perpendicular to that,
# tilted HAFT_RISE degrees head-up.
TRUNK = (0.15, 1.38)
TRUNK_RADIUS = 0.16
CUT_YAW = 60.0
CUT_DOWN = 30.0
HAFT_RISE = 15.0
# Where the bit meets the trunk (right, fwd, up).
TARGET = (round(TRUNK[0] - TRUNK_RADIUS * math.sin(math.radians(CUT_YAW)), 3),
          round(TRUNK[1] - TRUNK_RADIUS * math.cos(math.radians(CUT_YAW)), 3), 1.0)
REACH = TARGET[1]  # the work surface lies this far ahead (manifest `reach`): the engine steps up to it
SHEET_TIMES = (0.0, 8 * T, 15 * T, 20 * T, 23 * T, 25 * T, 27 * T, HIT, 32 * T, 37 * T, 42 * T)
AXE = prop_info("Tool_chop")


def stance(clip):
    p = clip.pose()
    p.foot("L", fwd=-0.10, out=0.07, turn=14, knee=6)
    p.foot("R", fwd=0.12, out=0.05, turn=-4, knee=4)
    p.hips(up=-0.05, turn=-4).spine(bend=6)
    return p.look(TARGET)


def swing(theta):
    """(haft, edge): world directions of the axe `theta` degrees back up the
    swing from the impact (0). The haft lies along the swing plane's radius,
    the edge along its tangent, pointing the way the bit travels downwards."""
    y, a, b = math.radians(CUT_YAW), math.radians(CUT_DOWN), math.radians(HAFT_RISE)
    cut = at(math.sin(y) * math.cos(a), math.cos(y) * math.cos(a), -math.sin(a))
    across = UP.cross(cut).normalized()   # horizontal, forward-left
    lift = cut.cross(across)              # up the plane
    haft0 = across * math.cos(b) + lift * math.sin(b)
    t = math.radians(theta)
    return haft0 * math.cos(t) - cut * math.sin(t), haft0 * math.sin(t) + cut * math.cos(t)


def axe(p, theta, hold, grip=None, bit=None, roll=0.0):
    """Wield the axe `theta` degrees back up the swing, the left hand `hold`
    metres up the haft and turned `roll` degrees about it: by the right
    hand's grip (root frame), or so that the edge is at `bit`."""
    yaw, pitch, turn = AXE.angles(*swing(theta))
    if bit is not None:
        p.wield_part(AXE, "bit", bit, yaw=yaw, pitch=pitch, roll=turn)
    else:
        p.wield(at=grip, yaw=yaw, pitch=pitch, roll=turn)
    return p.hold("L", at=hold, roll=roll)


def into_cut(depth):
    """A point `depth` metres along the cut from TARGET (negative: backed out)."""
    _, edge = swing(0.0)
    return from_world(at(*TARGET) + edge * depth)


def build(clip):
    # Out of the cut and already rising: hands in front, the axe swinging
    # up and back on the swing plane.
    free = stance(clip)
    free.hips(up=-0.055, right=0.0, bend=5).spine(bend=8, turn=-4)
    free.elbow("L", out=-30).elbow("R", out=-30)
    free = clip.fk(axe(free, 45, hold=0.18, grip=(0.16, 0.36, 1.08), roll=-10))

    # Cocked behind the left shoulder, the left hand slid up the haft and
    # turned over it (wrist straight), the body coiled.
    wind = stance(clip)
    wind.hips(up=-0.035, right=-0.05, fwd=-0.02, turn=-30, bend=-2).spine(bend=-8, turn=-40, lean=-10)
    wind.foot("R", heel=10)
    wind.shoulder("L", up=10).shoulder("R", up=8, fwd=10).elbow("L", out=-20).elbow("R", out=-30)
    wind = clip.fk(axe(wind, 120, hold=0.30, grip=(-0.28, 0.06, 1.40), roll=-40))

    # Arms reaching down the cut, the elbows in (wrists straight).
    hit = stance(clip)
    hit.hips(up=-0.10, right=0.04, fwd=0.03, turn=14, bend=10).spine(bend=18, turn=16, lean=4)
    hit.foot("L", heel=22)
    hit.shoulder("R", fwd=12).shoulder("L", fwd=8).elbow("L", out=-40).elbow("R", out=-40)
    hit = clip.fk(axe(hit, 0, hold=0.11, bit=TARGET), switch=False)

    # The axe sticks (a centimetre deeper); the body sinks on through the blow.
    shudder = stance(clip)
    shudder.hips(up=-0.115, right=0.045, fwd=0.035, turn=14, bend=12).spine(bend=22, turn=16, lean=4)
    shudder.foot("L", heel=22)
    shudder.shoulder("R", fwd=12).shoulder("L", fwd=8).elbow("L", out=-40).elbow("R", out=-40)
    shudder = clip.fk(axe(shudder, 0, hold=0.11, bit=into_cut(0.01)), switch=False)

    # Wrench it out: the bit backed out along the cut, the arms still
    # reaching (the forearms clear of the belly).
    wrench = stance(clip)
    wrench.hips(up=-0.10, right=0.04, fwd=0.03, turn=10, bend=10).spine(bend=20, turn=10, lean=2)
    wrench.foot("L", heel=12).shoulder("L", fwd=8).elbow("L", out=-40).elbow("R", out=-40)
    wrench = clip.fk(axe(wrench, 4, hold=0.12, bit=into_cut(-0.08)))

    # Clear of the trunk and turning up into the next backswing.
    recover = stance(clip)
    recover.hips(up=-0.07, right=0.02, fwd=0.0, turn=4, bend=6).spine(bend=12, turn=-2, lean=1)
    recover.elbow("L", out=-40).elbow("R", out=-40)
    recover = clip.fk(axe(recover, 25, hold=0.16, grip=(0.18, 0.42, 1.00)))

    # One backswing from the start to the top (no key between: the axe
    # keeps moving).
    clip.key(0, free)
    # Wind-up: the axe arrives at the top first and hangs there while the
    # body, still coiling, turns over and starts to unwind (hips lead).
    # Body and arms then accelerate on one curve each, from rest, and land
    # together.
    clip.key(15 * T, wind, parts=ARMS)
    clip.key(17 * T, wind, skip=ARMS, interp="QUAD", easing="EASE_IN")
    clip.key(20 * T, wind, parts=ARMS, interp="QUAD", easing="EASE_IN")
    clip.key(20 * T, wind, parts=("wield.w",), interp="CONSTANT")
    # Impact: the bit stops in the wood (IK holds it there until the
    # wrench, then hands back to FK); the body decelerates on through the
    # blow.
    clip.key(HIT, hit, skip=ARMS, interp="CUBIC", easing="EASE_OUT")
    clip.key(HIT, hit, parts=ARMS, interp="SINE", easing="EASE_IN_OUT")
    clip.key(32 * T, shudder)
    clip.key(37 * T, wrench)
    clip.key(42 * T, recover)
