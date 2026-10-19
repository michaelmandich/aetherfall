"""Spawn: just trained, steps out of the doorway, looks round, ends in Idle.

The clip starts a step behind the unit's spot (ducking under the lintel)
and two steps bring the body forward onto it, so the unit seems to walk
out of the building it appeared from; it ends exactly on Idle's first
pose (`stances.relaxed()`), at rest. A unit with a rally point sets off at
once, and the engine lets only the first `SPAWN_HOLD` (0.4 s, af_render)
play before Walk takes over, so both steps land inside it:

0.00 stooped, a stride back, weight on the right foot -> 0.13 the left
foot swings forward (right arm swings forward, chest rises) -> 0.24 left
foot down -> 0.31 the right foot comes through -> 0.38 right foot down
beside it -> 0.40 arrived, upright -> 0.56 a look to the left -> 0.78 and
to the right -> 1.00 Idle.
"""

from lib.pose import ARMS, FEET
from lib.stances import relaxed

DURATION = 1.0
LOOP = False
SHEET_TIMES = (0.0, 0.07, 0.13, 0.2, 0.24, 0.31, 0.38, 0.4, 0.56, 0.78, 1.0)

BACK = 0.36  # how far behind its spot the unit starts (m)
# Step timing (s): each foot lifts, peaks mid-swing, lands.
L_LIFT, L_PEAK, L_DOWN = 0.04, 0.13, 0.24
R_PEAK, R_DOWN = 0.31, 0.38
ARRIVE = 0.40  # af_render SPAWN_HOLD: Walk may take over from here
ARM_LAG = 0.03  # the arm swings trail the steps (overlap)


def build(clip):
    rest = relaxed()
    out_l, turn_l = rest.get("foot.L.out"), rest.get("foot.L.turn")
    out_r, turn_r = rest.get("foot.R.out"), rest.get("foot.R.turn")

    # Body (pelvis, spine, head, arms), keyed apart from the feet.
    duck = rest.copy().hips(fwd=-BACK + 0.02, up=-0.06, right=0.025, bend=8).spine(bend=16).head(nod=12)
    duck.arm("L", lift=10, toward=160).arm("R", lift=13, toward=40).elbow("R", 24)
    stride = rest.copy().hips(fwd=-BACK * 0.55, up=-0.035, right=0.035, turn=-6, bend=4).spine(bend=6, turn=5)
    stride.head(nod=0).arm("L", lift=22, toward=170).arm("R", lift=26, toward=8).elbow("R", 32).elbow("L", 20)
    land = rest.copy().hips(fwd=-BACK * 0.3, up=-0.06, right=-0.02, turn=3).spine(bend=3)
    land.arm("L", lift=15, toward=130).arm("R", lift=12, toward=30)
    through = rest.copy().hips(fwd=-0.06, up=-0.03, right=-0.035, turn=6).spine(turn=-5)
    through.arm("L", lift=16, toward=20).elbow("L", 28).arm("R", lift=15, toward=140).elbow("R", 18)
    arrive = rest.copy().hips(fwd=0.005, up=-0.035).head(turn=-10)
    look_l = rest.copy().hips(right=0.012, turn=-6).spine(turn=-16).head(turn=-45, nod=-4, tilt=-3)
    look_r = rest.copy().hips(right=-0.012, turn=6).spine(turn=16).head(turn=42, nod=-3, tilt=3)

    for t, p in ((0.0, duck), (L_PEAK, stride), (L_DOWN, land), (R_PEAK, through), (ARRIVE, arrive)):
        clip.key(t, p, skip=FEET + ARMS)
        clip.key(t + ARM_LAG if t else 0.0, p, parts=ARMS)
    clip.key(0.56, look_l, skip=FEET)
    # Settle into Idle at rest, so the crossfade into it is smooth.
    clip.key(0.78, look_r, skip=FEET, interp="SINE", easing="EASE_IN_OUT")
    clip.key(1.0, rest, skip=FEET)

    # Feet: each planted (constant) until it lifts, then swung to Idle's
    # stance and planted there.
    feet = rest.copy()
    feet.foot("L", fwd=-BACK + 0.04).foot("R", fwd=-BACK - 0.04)
    clip.key(0.0, feet, parts=FEET)
    clip.key(L_LIFT, feet, parts=FEET)
    swing_l = feet.copy().foot("L", fwd=-BACK * 0.45, lift=0.11, heel=10)
    clip.key(L_PEAK, swing_l, parts=("foot.L",))
    down_l = feet.copy().foot("L", fwd=0.0, out=out_l, turn=turn_l, lift=0.0, heel=0.0)
    clip.key(L_DOWN, down_l, parts=("foot.L",))
    # The right heel peels off as the weight goes forward, then it swings.
    clip.key(L_DOWN, down_l.copy().foot("R", heel=22), parts=("foot.R",))
    swing_r = down_l.copy().foot("R", fwd=-BACK * 0.4, lift=0.10, heel=12)
    clip.key(R_PEAK, swing_r, parts=("foot.R",))
    down_r = down_l.copy().foot("R", fwd=0.0, out=out_r, turn=turn_r, lift=0.0, heel=0.0)
    clip.key(R_DOWN, down_r, parts=("foot.R",))
    clip.key(1.0, rest, parts=FEET)
