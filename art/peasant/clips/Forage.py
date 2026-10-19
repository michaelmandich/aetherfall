"""Forage: picking berries from a bush in front into a basket (food).

The basket hangs level from the left hand by the left hip (placed with
IK, so it stays upright however the body bends); the right hand does the
picking, twice per loop:

0.00 the hand has just emptied into the basket, held up and in to meet
it -> 0.30 a lean towards the bush -> 0.37 the hand reaches into its upper
right, fingers first, eyes on the hand -> 0.43 it closes on a bunch ->
0.53 pluck: a snap back and up out of the bush (12 cm back, 13 cm up,
clearing its top), the wrist flicking shut, the body rocking back with
the pull -> 0.77 the hand swings back over the basket -> 0.90 a shake to
drop the berries (forearm roll and wrist flex) -> 1.23 a deeper dip,
knees bending, to reach low into the middle of the bush, the basket swung
out of the way -> 1.37 grasp -> 1.47 pluck -> 1.73 back over the basket
-> 1.87 drop -> 2.00.

The right hand is placed with IK (reach) on each key and converted to FK,
so it travels in arcs; its palm is turned by rolling the forearm (`pick`),
so the wrist only bends in its anatomical planes. Feet stay planted; the
weight rocks towards the bush on each reach and back on each pluck.
"""

from lib.clip import FRAME as T
from lib.pose import arm_parts

DURATION = 2.0
LOOP = True
PROPS = ("Tool_forage",)
# The bush's front at picking height (right, fwd, up), metres.
TARGET = (0.12, 0.55, 0.68)
REACH = TARGET[1]  # the work surface lies this far ahead (manifest `reach`): the engine steps up to it
SHEET_TIMES = (0.0, 11 * T, 13 * T, 16 * T, 23 * T, 27 * T, 39 * T, 44 * T, 52 * T, 56 * T)

BEATS = (11 * T, 39 * T)  # the hand closes on a bunch before the pluck
RIGHT = arm_parts("R")


def stance(clip):
    p = clip.pose()
    p.foot("L", fwd=0.04, out=0.06, turn=8, knee=4)
    p.foot("R", fwd=-0.04, out=0.06, turn=12, knee=4)
    return p


def basket(p, at, yaw=0.0):
    """The basket hanging level from the left fist at `at` (root frame, IK:
    wield() poses the left-hand prop when it is the only one), its handle
    bar pointing forward, so it stays upright however the body bends."""
    return p.wield(at=at, pitch=80, yaw=yaw, roll=0)


def pick(clip, body, at, look, flex=0.0, tilt=0.0, roll=0.0):
    """`body` with the right wrist placed at `at` (root frame, IK) and the arm
    converted to FK numbers; then the hand is turned by rolling the forearm
    `roll` degrees (+ palm up, - palm down) and the wrist bent `flex`
    (palm side +) and `tilt` (thumb side +). Rolling the forearm instead of
    twisting the wrist keeps both bends in their anatomical planes."""
    p = clip.fk(body.copy().reach("R", at=at).look(look))  # raises if the hand cannot reach
    forearm = p.get("elbow.R.twist") + p.get("wrist.R.twist")
    return p.elbow("R", twist=forearm + roll).wrist("R", flex=flex, tilt=tilt, twist=0.0)


def build(clip):
    basket_look = (-0.33, 0.32, 0.8)
    over = stance(clip).hips(up=-0.05, fwd=-0.01, turn=-10).spine(bend=21, turn=-20)
    over = basket(over, (-0.29, 0.33, 0.92), yaw=-5)
    drop = pick(clip, over, (-0.17, 0.36, 0.99), basket_look, flex=25, roll=-55)
    shake = pick(clip, over.copy().spine(bend=22), (-0.16, 0.35, 0.975), basket_look, flex=40, roll=-85)

    # Pick 1: high in the right of the bush. Fingers in (wrist back), close
    # on a bunch, then snap back and up out of the bush, the wrist flicking.
    lean = stance(clip).hips(up=-0.15, fwd=0.06, turn=8, bend=17).spine(bend=39, turn=12, lean=3)
    lean = basket(lean, (-0.43, 0.30, 0.82))
    lean.foot("L", heel=6).shoulder("R", fwd=18)
    bush1 = (0.18, 0.66, 0.70)
    reach1 = pick(clip, lean, (0.18, 0.49, 0.74), bush1, flex=-20, roll=-25)
    grasp1 = pick(clip, lean, (0.18, 0.49, 0.72), bush1, flex=10, roll=-25)
    rock1 = lean.copy().hips(up=-0.12, fwd=0.03, bend=13).spine(bend=31)
    rock1.foot("L", heel=3).shoulder("R", fwd=4)
    tug1 = pick(clip, rock1, (0.20, 0.37, 0.85), bush1, flex=45, roll=-40)

    # Pick 2: a deeper dip, low in the middle of the bush.
    dip = stance(clip).hips(up=-0.23, fwd=0.06, turn=4, bend=19).spine(bend=42, turn=4)
    dip = basket(dip, (-0.44, 0.30, 0.72), yaw=5)
    dip.foot("L", knee=12).foot("R", knee=12).shoulder("R", fwd=12)
    bush2 = (0.06, 0.58, 0.55)
    reach2 = pick(clip, dip, (0.06, 0.42, 0.61), bush2, flex=-20, roll=-15)
    grasp2 = pick(clip, dip, (0.06, 0.43, 0.59), bush2, flex=10, roll=-15)
    rock2 = dip.copy().hips(up=-0.20, fwd=0.03, bend=16).spine(bend=37)
    rock2.shoulder("R", fwd=4)
    tug2 = pick(clip, rock2, (0.08, 0.30, 0.72), bush2, flex=45, roll=-35)

    clip.key(0, drop)
    # Reach: the body leans in first, the hand follows a beat behind.
    clip.key(9 * T, reach1, skip=RIGHT)
    clip.key(11 * T, reach1, parts=RIGHT)
    # Pluck: the hand snaps out of the bush (fastest as it starts, easing to
    # a stop clear of the bush); the body holds still until the pull, then
    # rocks back with it on a smooth curve.
    clip.key(13 * T, reach1, skip=RIGHT)
    clip.key(13 * T, grasp1, parts=RIGHT, interp="QUAD", easing="EASE_OUT")
    clip.key(16 * T, tug1)
    clip.key(23 * T, drop)
    clip.key(27 * T, shake)
    clip.key(30 * T, drop, parts=RIGHT)
    clip.key(37 * T, reach2, skip=RIGHT)
    clip.key(39 * T, reach2, parts=RIGHT)
    clip.key(41 * T, reach2, skip=RIGHT)
    clip.key(41 * T, grasp2, parts=RIGHT, interp="QUAD", easing="EASE_OUT")
    clip.key(44 * T, tug2)
    clip.key(52 * T, drop)
    clip.key(56 * T, shake)
