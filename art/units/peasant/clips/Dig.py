"""Dig: shovelling sand from a pile in front and tossing it aside (silicon).

0.00 aim: the blade drawn back and up, 20 cm short of the foot of the
pile along the shaft, weight back on the left foot -> 0.23 HIT: a lunge,
the hips driving forward and the rear heel coming up, the blade driven
along the shaft into the sand, fastest as it goes in -> 0.37 pushed in
deeper, easing to a stop, knees and back bent -> 0.57 the top hand pushes
down and the blade levers up full of sand -> 0.77 the body straightens
and turns left, the loaded blade carried round -> 1.00 swung out to the
left -> 1.07 the toss: a flick of both arms snaps the blade up and over,
the chest twisted hard left -> 1.20 the follow-through runs out, arms
extended -> 1.50 swinging back round to the front, the body unwinding
with it -> 1.67 back over the pile, blade lowered -> 1.80 drawn back
and up into the aim again.

Grip: the right hand on the top of the shaft by the cross grip, the left
further down, palm up under it (lib/props.py). The blade's upper face is
its scoop. The shovel is placed with IK on every key: in the sand each key
places the blade tip itself (`tip_at`), so the thrust runs straight along
the shaft; in the carry and the toss the shovel turns about the hands, so
the blade sweeps an arc. (FK numbers would flip branch in the toss, where
the right forearm rolls right over.) The top hand is kept out in front of
the right hip, clear of the body. Feet planted; the head follows the
blade.
"""

from lib.clip import FRAME as T
from lib.props import prop_info
from lib.rig import at, from_world

DURATION = 1.8
LOOP = True
HIT = 7 * T
PROPS = ("Tool_dig",)
# Where the blade tip enters the sand pile (right, fwd, up), metres.
TARGET = (0.0, 1.10, 0.10)
REACH = TARGET[1]  # the work surface lies this far ahead (manifest `reach`): the engine steps up to it
THRUST = 136.0     # the shaft's pitch along the thrust (46 degrees below level)
THRUST_YAW = -6.0  # turned a little, so the top hand is out by the right hip
SHEET_TIMES = (0.0, 4 * T, HIT, 11 * T, 17 * T, 23 * T, 30 * T, 32 * T, 36 * T, 45 * T, 50 * T)
BEATS = (29 * T,)  # the loaded blade slows as it comes round, gathering for the flick

LOWER = dict(at=0.3, roll=180.0)  # the left hand: 30 cm down the shaft, palm up
PUSH = dict(at=0.22, roll=180.0)  # ... choked up to 22 cm for the push into the sand
TOSS = (-0.9, 0.7, 0.9)           # where the eyes follow the sand
SHOVEL = prop_info("Tool_dig")


def stance(clip):
    p = clip.pose()
    p.foot("L", fwd=0.14, out=0.06, turn=2, knee=6)
    p.foot("R", fwd=-0.12, out=0.08, turn=20, knee=6)
    return p


def along(depth, pitch=THRUST, yaw=THRUST_YAW):
    """The point `depth` metres along the shaft past TARGET (negative: short
    of it), for a shovel at this pitch and yaw."""
    axis = SHOVEL.orientation(yaw=yaw, pitch=pitch).col[1]
    return from_world(at(*TARGET) + axis * depth)


def tip_at(p, tip, pitch=THRUST, yaw=THRUST_YAW):
    """wield() the shovel so its blade tip is at `tip` (root frame), the left
    hand choked up for the push and the right elbow out (wrist straight)."""
    return p.wield_part(SHOVEL, "tip", tip, pitch=pitch, yaw=yaw).hold("L", **PUSH).elbow("R", out=40)


def build(clip):
    # Drawn back: the tip 20 cm short of the pile along the shaft, weight
    # back on the left foot.
    aim = stance(clip).hips(up=-0.11, fwd=-0.08, right=0.01, bend=4).spine(bend=18)
    tip_at(aim, along(-0.20)).look((0.0, 1.0, 0.0))

    # The lunge drives it in: hips forward, rear heel up.
    thrust = stance(clip).hips(up=-0.17, fwd=0.05, right=-0.02, bend=14).spine(bend=33)
    thrust.foot("R", heel=14)
    tip_at(thrust, along(0.02)).look(TARGET)

    deep = stance(clip).hips(up=-0.21, fwd=0.07, right=-0.03, bend=17).spine(bend=39)
    deep.foot("R", heel=18)
    tip_at(deep, along(0.10)).look(TARGET)

    # The top hand pushes down: the shaft flattens about the pile's lip and
    # the blade levers up, full.
    lever = stance(clip).hips(up=-0.17, fwd=0.0, right=-0.02, bend=11).spine(bend=28)
    lever.foot("R", heel=8)
    tip_at(lever, (TARGET[0] - 0.01, TARGET[1] + 0.08, 0.22), pitch=124).look((0.0, 1.1, 0.2))

    # Carry and toss: the shovel turns about the hands; the elbows are held
    # out (`elbow(out=...)`) so the forearms clear the belly and chest.
    lift = stance(clip).hips(up=-0.10, right=-0.01, turn=-10, bend=6).spine(bend=14, turn=-15)
    lift.wield(at=(0.10, 0.16, 1.06), pitch=110, yaw=-25, roll=-10).hold("L", **LOWER).look((-0.4, 1.1, 0.6))
    lift.elbow("R", out=20).elbow("L", out=30)

    swing = stance(clip).hips(up=-0.08, right=-0.03, turn=-25, bend=4).spine(bend=10, turn=-35)
    swing.foot("R", heel=12)
    swing.wield(at=(0.05, 0.18, 1.10), pitch=100, yaw=-65, roll=-20).hold("L", **LOWER).look(TOSS)
    swing.elbow("R", out=50).elbow("L", out=40)

    toss = stance(clip).hips(up=-0.06, right=-0.04, turn=-28, bend=2).spine(bend=6, turn=-42, lean=-5)
    toss.foot("R", heel=16)
    toss.wield(at=(0.05, 0.18, 1.10), pitch=72, yaw=-82, roll=-40).hold("L", **LOWER).look(TOSS)
    toss.elbow("R", out=50).elbow("L", out=40)

    # Follow-through: the flick carries on, easing out, arms extended.
    after = stance(clip).hips(up=-0.055, right=-0.045, turn=-30, bend=1).spine(bend=4, turn=-46, lean=-7)
    after.foot("R", heel=18)
    after.wield(at=(0.04, 0.175, 1.12), pitch=62, yaw=-90, roll=-48).hold("L", **LOWER).look(TOSS)
    after.elbow("R", out=35).elbow("L", out=40)

    # Swinging back round to the front, the body unwinding with it.
    ret = stance(clip).hips(up=-0.10, right=-0.03, turn=-18, bend=5).spine(bend=14, turn=-24)
    ret.wield(at=(0.06, 0.17, 1.10), pitch=100, yaw=-50, roll=-25).hold("L", **LOWER).look((-0.3, 1.0, 0.3))
    ret.elbow("R", out=50).elbow("L", out=40)

    # Back over the pile, blade lowered and a little ahead of the aim (which
    # then draws it back and up: the anticipation of the next thrust).
    back = stance(clip).hips(up=-0.15, fwd=-0.01, turn=-6, bend=10).spine(bend=28, turn=-6)
    back.wield(at=(0.10, 0.22, 1.02), pitch=128, yaw=-10, roll=0).hold("L", **LOWER).look((0.0, 1.0, 0.0))
    back.elbow("R", out=40).elbow("L", out=30)

    # The thrust: accelerating all the way into the sand, then easing out.
    clip.key(0, aim, interp="QUAD", easing="EASE_IN")
    clip.key(HIT, thrust, interp="QUAD", easing="EASE_OUT")
    clip.key(11 * T, deep)
    clip.key(17 * T, lever)
    clip.key(23 * T, lift)
    # The carry and toss on one smooth curve: fastest through the flick,
    # running out over the follow-through.
    clip.key(30 * T, swing)
    clip.key(32 * T, toss)
    clip.key(36 * T, after)
    clip.key(45 * T, ret)
    clip.key(50 * T, back)
