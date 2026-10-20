"""Mine: overhead pickaxe swings at a rock in front (stone, copper, iron).

The right hand holds the butt, the left slides along the haft:

0.00 the pick has just popped free, held low in front -> 0.27 it rises in
front of the face, haft upright -> 0.50 cocked over the head (hands above
the forehead), the pick head behind the shoulders,
chest arched, up on the toes -> 0.57 the body turns over and starts down,
hips first, while the pick still hangs -> 0.63 the arms follow, both
accelerating all the way -> 0.80 HIT: the pick drives down into the top
of the rock at full speed, the left hand slid down to the right, knees
sunk, back bent over the stroke -> 0.90 the tip sticks while the body
sinks on through the blow -> 1.17 the haft is levered down and back, the
body sitting back against it (the tip stays in the rock) -> 1.33 the tip
pops up out of it -> 1.50.

The swing is FK (arcs); IK takes over on the impact frame and holds the
tip in the rock until it pops free, then hands back to FK over the last
few frames. Feet stay planted (IK); the head
watches the strike point throughout.
"""

from lib.clip import FRAME as T
from lib.pose import ARMS

DURATION = 1.5
LOOP = True
HIT = 24 * T
PROPS = ("Tool_mine",)
# Where the pick's tip strikes the rock (right, fwd, up), metres.
TARGET = (0.0, 1.1, 0.42)
REACH = TARGET[1]  # the work surface lies this far ahead (manifest `reach`): the engine steps up to it
SHEET_TIMES = (0.0, 8 * T, 15 * T, 18 * T, 21 * T, HIT, 27 * T, 35 * T, 40 * T)


def stance(clip):
    p = clip.pose()
    p.foot("L", fwd=0.06, out=0.09, turn=6, knee=6)
    p.foot("R", fwd=-0.06, out=0.09, turn=16, knee=6)
    p.hips(up=-0.05).spine(bend=6)
    return p.look(TARGET)


def build(clip):
    free = stance(clip)
    free.hips(up=-0.11, fwd=-0.02, bend=8).spine(bend=20)
    free.wield(at=(0.0, 0.36, 0.80), pitch=65, roll=0).hold("L", at=0.12, roll=-20)
    free = clip.fk(free)

    # Rising in front of the face: the haft upright, outside the head, the
    # left hand turning over the haft as it slides up (wrist straight).
    lift = stance(clip)
    lift.hips(up=-0.04, fwd=-0.01, bend=2).spine(bend=0, turn=-6)
    lift.wield(at=(-0.03, 0.34, 1.40), pitch=-5, yaw=-4, roll=0).hold("L", at=0.20, roll=-40)
    lift = clip.fk(lift)

    # Cocked over the head: the hands above the forehead, in front of the
    # head's line, so neither forearm crosses the head (and the left elbow,
    # which IK swings round when the hands pass over the head, stays steady).
    top = stance(clip)
    top.hips(up=-0.02, fwd=-0.035, bend=-4, turn=-6).spine(bend=-12, turn=-6)
    top.foot("L", heel=4).foot("R", heel=4)
    top.wield(at=(-0.04, 0.17, 1.63), pitch=-50, yaw=-6, roll=0).hold("L", at=0.18, roll=-20)
    top.shoulder("L", up=12).shoulder("R", up=12).elbow("R", out=20)
    top = clip.fk(top)

    hit = stance(clip)
    hit.hips(up=-0.14, fwd=0.03, bend=12).spine(bend=30, turn=2)
    hit.wield_part("Tool_mine", "tip", TARGET, pitch=85).hold("L", at=0.10)
    hit.shoulder("L", fwd=10).shoulder("R", fwd=10)
    hit = clip.fk(hit, switch=False)

    # The tip sticks while the body sinks on through the blow.
    shudder = stance(clip)
    shudder.hips(up=-0.16, fwd=0.03, bend=14).spine(bend=34, turn=2)
    shudder.wield_part("Tool_mine", "tip", TARGET, pitch=85).hold("L", at=0.10)
    shudder.shoulder("L", fwd=10).shoulder("R", fwd=10)
    shudder = clip.fk(shudder, switch=False)

    # Lever: the tip stays in the rock, the hands pull down and back.
    pry = stance(clip)
    pry.hips(up=-0.20, fwd=-0.015, bend=12).spine(bend=31, turn=-2)
    pry.foot("L", heel=6)
    pry.wield_part("Tool_mine", "tip", TARGET, pitch=82).hold("L", at=0.10)
    pry = clip.fk(pry, switch=False)

    # The tip pops up out of the rock.
    pop = stance(clip)
    pop.hips(up=-0.17, fwd=-0.03, bend=10).spine(bend=26)
    pop.wield(at=(0.0, 0.35, 0.72), pitch=72, roll=0).hold("L", at=0.11)
    pop = clip.fk(pop, switch=False)

    clip.key(0, free)
    clip.key(8 * T, lift)
    # The top: the pick arrives first and hangs there while the body, still
    # coiling up, turns over and starts down (hips first). Body and arms then
    # accelerate on one curve each, from rest, and land together.
    clip.key(15 * T, top, parts=ARMS)
    clip.key(17 * T, top, skip=ARMS, interp="QUAD", easing="EASE_IN")
    clip.key(19 * T, top, parts=ARMS, interp="QUAD", easing="EASE_IN")
    # FK through the swing; IK (wield.w = 1) from the impact frame on.
    clip.key(19 * T, top, parts=("wield.w",), interp="CONSTANT")
    # Impact: the tip stops in the rock; the body decelerates on through it.
    clip.key(HIT, hit, skip=ARMS, interp="CUBIC", easing="EASE_OUT")
    clip.key(HIT, hit, parts=ARMS, interp="SINE", easing="EASE_IN_OUT")
    clip.key(27 * T, shudder)
    clip.key(35 * T, pry)
    # Still on IK while the tip pops free (a straight pull up out of the
    # rock), then FK from the next frame on: `pop` carries both, so the
    # switch is exact and the pick swings on up along the arm's arc.
    clip.key(35 * T, pry, parts=("wield.w",), interp="CONSTANT")
    clip.key(40 * T, pop)
    clip.key(40 * T, pop, parts=("wield.w",), interp="CONSTANT")
    clip.key(41 * T, free, parts=("wield.w",))
