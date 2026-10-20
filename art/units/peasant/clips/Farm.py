"""Farm: working a field with a hoe, two strokes per loop.

Each stroke (1 s, times for the first): 0.00 the blade is in the soil at
the end of the last draw, the hands by the right hip -> 0.13 the body
rises and rocks back first, easing the blade out of the soil -> 0.43 the
hoe is lifted high, blade above head height in front, the body arching
-> 0.53 the hips start down while the blade still hangs at the top ->
0.67 the blade is chopped down into the soil 1.1 m ahead, knees and back
bending into it, fastest as it lands -> 0.77 it bites 4 cm deep while the
body sinks on through the blow -> 1.00 the blade is drawn 20 cm back
through the soil towards the feet (still 4 cm deep), the body
straightening and the hands coming back beside the right hip (clear of
the body) -> the next lift. The second stroke (1.00-2.00)
is a little lower and lands a hand's width to the left, the body turned
with it, so the loop does not read as one stroke repeated.

Broom grip (see lib/props.py): the right hand at the top of the shaft, the
left further down, palm up under it. The lift and chop are FK (arcs); IK
holds the blade in the soil from the bite to the end of the draw (each
IK key places the blade edge itself, `edge_at`). Feet planted; the head
watches the blade.
"""

import math

from lib.clip import FRAME as T
from lib.pose import ARMS

DURATION = 2.0
LOOP = True
PROPS = ("Tool_farm",)
# Where the first stroke's blade edge meets the soil (right, fwd, up). Two
# strokes per loop, so no single HIT goes to the manifest.
TARGET = (0.0, 1.12, 0.0)
REACH = TARGET[1]  # the work surface lies this far ahead (manifest `reach`): the engine steps up to it
BITE = 0.04       # how deep the edge sinks at the bite and stays through the draw (m)
DRAW = 0.20       # how far the blade is drawn back towards the feet (m)
SHEET_TIMES = (0.0, 4 * T, 13 * T, 16 * T, 19 * T, 20 * T, 23 * T, 26 * T, 43 * T, 50 * T)

GRIP = dict(at=0.2, roll=180.0)  # the left hand: 20 cm down the shaft, palm up


def stance(clip):
    p = clip.pose()
    p.foot("L", fwd=0.12, out=0.05, turn=4, knee=6)
    p.foot("R", fwd=-0.10, out=0.07, turn=18, knee=6)
    return p


def edge_at(p, edge, pitch, yaw):
    """wield() the hoe so its blade edge is at `edge` (root frame), the haft
    pitched `pitch` degrees from upright (90 = level, more = the blade
    lower than the hands) and turned `yaw` (right +)."""
    return p.wield_part("Tool_farm", "edge", edge, pitch=pitch, yaw=yaw).hold("L", **GRIP)


def _swung(point, yaw):
    """`point` turned `yaw` degrees (right +) about the vertical through the
    root, so the second stroke's blade path is the first one's, turned."""
    a = math.radians(yaw)
    r, f = point[0], point[1]
    return (r * math.cos(a) + f * math.sin(a), -r * math.sin(a) + f * math.cos(a), point[2])


def stroke(clip, yaw, turn, lift):
    """The poses of one stroke. `yaw` swings the blade's line left (-) or
    right, `turn` the body with it; `lift` scales how high the blade goes."""
    land = _swung(TARGET, yaw)
    sunk = (land[0], land[1], -BITE)
    near = _swung((TARGET[0], TARGET[1] - DRAW, -BITE + 0.005), yaw)

    raised = stance(clip).hips(up=-0.035, fwd=-0.02, bend=0, turn=turn).spine(bend=-5, turn=turn)
    grip = _swung((0.0, 0.14 + 0.09 * lift, 1.12 + 0.16 * lift), yaw)
    raised.wield(at=grip, pitch=75 - 30 * lift, yaw=yaw).hold("L", **GRIP)
    raised.shoulder("L", up=6 * lift).shoulder("R", up=6 * lift).look((land[0], 1.0, 0.0))
    raised = clip.fk(raised)

    # The blade lands at the surface, fastest; the body bends into it.
    strike = stance(clip).hips(up=-0.12, fwd=0.02, bend=12, turn=turn).spine(bend=30, turn=turn)
    edge_at(strike, land, pitch=128, yaw=yaw - 4).look(land)
    strike.shoulder("L", fwd=8).shoulder("R", fwd=8).elbow("R", out=40)
    # The edge sinks in while the body sinks on through the blow.
    bite = stance(clip).hips(up=-0.15, fwd=0.025, bend=15, turn=turn).spine(bend=35, turn=turn)
    edge_at(bite, sunk, pitch=127, yaw=yaw - 4).look(land)
    bite.shoulder("L", fwd=8).shoulder("R", fwd=8).elbow("R", out=40)
    # Drawn back through the soil: the haft steepens, the hands come back
    # beside the right hip (outside the body), the body straightens.
    drawn = stance(clip).hips(up=-0.13, fwd=-0.03, bend=8, turn=turn).spine(bend=20, turn=turn)
    edge_at(drawn, near, pitch=136, yaw=yaw - 10).look((near[0], near[1], 0.0))
    drawn.shoulder("L", fwd=4).shoulder("R", fwd=4).elbow("R", out=50)
    # Half way through the draw, the edge still at depth (IK numbers alone
    # would let it dip between the bite and the end of the draw).
    mid = bite.lerp(drawn, 0.5)
    edge_at(mid, tuple((a + b) / 2 for a, b in zip(sunk, near)), pitch=131.5, yaw=yaw - 7)
    return dict(raised=raised, strike=clip.fk(strike, switch=False), bite=clip.fk(bite, switch=False),
                mid=clip.fk(mid, switch=False), drawn=clip.fk(drawn, switch=False))


def key_stroke(clip, start, prev, cur):
    """Key `cur` (a stroke()) starting `start` frames in, rising out of the
    previous stroke's draw `prev`."""
    # The body rises first (the hands follow on their own curve), easing
    # the blade out of the soil.
    clip.key(start * T, prev["drawn"])
    clip.key((start + 4) * T, prev["drawn"].lerp(cur["raised"], 0.3), skip=ARMS)
    clip.key((start + 13) * T, cur["raised"])
    # The top: the arms hang there while the body starts down; then both
    # accelerate into the soil and land together.
    clip.key((start + 13) * T, cur["raised"], skip=ARMS, interp="QUAD", easing="EASE_IN")
    clip.key((start + 16) * T, cur["raised"], parts=ARMS, interp="QUAD", easing="EASE_IN")
    clip.key((start + 16) * T, cur["raised"], parts=("wield.w",), interp="CONSTANT")
    # Impact: the blade stops in the soil (IK from here), the body decelerates
    # through it into the bite.
    clip.key((start + 20) * T, cur["strike"], skip=ARMS, interp="CUBIC", easing="EASE_OUT")
    clip.key((start + 20) * T, cur["strike"], parts=ARMS, interp="SINE", easing="EASE_IN_OUT")
    clip.key((start + 23) * T, cur["bite"])
    clip.key((start + 26) * T, cur["mid"])


def build(clip):
    first = stroke(clip, yaw=0.0, turn=0.0, lift=1.0)
    second = stroke(clip, yaw=-7.0, turn=-5.0, lift=0.8)
    # The loop starts where the second stroke's draw ends (blade in the soil).
    key_stroke(clip, 0, second, first)
    key_stroke(clip, 30, first, second)
