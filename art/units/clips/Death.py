"""Death: struck down; buckles, sits back hard and ends lying on the ground.

0.00 standing -> 0.12 the blow snaps the chest and head back, arms fling
-> 0.42 the knees give (feet planted), the body folds -> 0.64 the seat
hits the ground, feet skid forward (still IK, so nothing goes through the
floor) -> 0.71 a beat on the seat while the legs let go (IK -> FK at the
same angles) -> 0.92 the back hits the ground, legs lift -> 1.02
the head lands, arms flop out -> a small rebound -> 1.5 limp (the engine
holds the last frame).
"""

from lib.clip import ClipError
from lib.stances import relaxed

DURATION = 1.5
LOOP = False
SHEET_TIMES = (0.0, 0.12, 0.3, 0.42, 0.54, 0.64, 0.71, 0.82, 0.92, 1.02, 1.5)
BEATS = (0.71,)  # the beat on the seat before the fall
LEGS = ("L", "R")


def build(clip):
    stand = relaxed()

    struck = stand.copy().hips(fwd=-0.03, up=-0.035, bend=-4).spine(bend=-14).head(nod=-18)
    struck.arm("L", lift=38, toward=115).elbow("L", 35).arm("R", lift=44, toward=105).elbow("R", 30)
    struck.shoulder("L", up=8).shoulder("R", up=8)

    buckle = stand.copy().hips(fwd=-0.08, up=-0.30, bend=10).spine(bend=24, lean=-6).head(nod=28, tilt=-8)
    buckle.foot("L", knee=6).foot("R", knee=8)
    buckle.arm("L", lift=22, toward=45).elbow("L", 30).arm("R", lift=26, toward=40).elbow("R", 25)
    buckle = clip.fk(buckle, arms=(), legs=LEGS, switch=False)

    for draw in (1.0, 0.94, 0.88, 0.82, 0.76):   # a short-legged body sits with its feet drawn in
        seat = stand.copy().hips(fwd=-0.22 * draw, up=-0.70, bend=-18, lean=-4).spine(bend=10, lean=-4).head(nod=14)
        seat.foot("L", fwd=0.42 * draw, out=0.03, turn=6, knee=2).foot("R", fwd=0.38 * draw, out=0.05, turn=8, knee=4)
        seat.arm("L", lift=36, toward=100).elbow("L", 30).arm("R", lift=42, toward=105).elbow("R", 32)
        try:
            seat = clip.fk(seat, arms=(), legs=LEGS, switch=False)
            break
        except ClipError:
            if draw == 0.76:
                raise
    # A beat on the seat: the legs let go (FK, same angles) before the fall.
    slump = seat.copy().hips(up=-0.71, fwd=-0.225).spine(bend=8).head(nod=10)
    for s in LEGS:
        slump.set(f"leg.{s}.ik", 0.0)

    down = seat.copy().hips(fwd=-0.27, up=-0.735, bend=-84, lean=-3).spine(bend=-4).head(nod=-12)
    down.leg("L", lift=55, toward=8, twist=10, knee=62).leg("R", lift=48, toward=12, twist=12, knee=55)
    down.ankle("L", flex=10).ankle("R", flex=8)
    down.arm("L", lift=62, toward=100).elbow("L", 25).arm("R", lift=66, toward=105).elbow("R", 30)

    land = down.copy().hips(up=-0.745, bend=-90).spine(bend=-6).head(nod=-6, turn=12)
    land.leg("L", lift=10, knee=20).leg("R", lift=6, knee=16)
    land.arm("L", lift=58, toward=112).elbow("L", 18).arm("R", lift=62, toward=116).elbow("R", 22)

    rebound = land.copy().spine(bend=-2).head(nod=-2, turn=16)
    rebound.leg("L", lift=14, knee=26).leg("R", lift=9, knee=20)

    limp = land.copy().head(nod=-5, turn=26, tilt=6)
    limp.leg("L", lift=6, toward=12, twist=14, knee=16).leg("R", lift=4, toward=14, twist=18, knee=12)
    limp.ankle("L", flex=14, turn=10).ankle("R", flex=12, turn=12)
    limp.arm("L", lift=60, toward=114).elbow("L", 22).arm("R", lift=64, toward=118).elbow("R", 26)

    clip.key(0.0, stand)
    clip.key(0.12, struck, interp="QUAD", easing="EASE_OUT")
    clip.key(0.42, buckle, interp="QUAD", easing="EASE_IN")
    clip.key(0.64, seat)
    clip.key(0.71, slump, interp="QUAD", easing="EASE_IN")
    clip.key(0.92, down, interp="QUAD", easing="EASE_OUT")
    clip.key(1.02, land)
    clip.key(1.14, rebound)
    clip.key(1.5, limp)
