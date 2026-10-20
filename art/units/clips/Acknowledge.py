"""Acknowledge: a knuckle to the brow and a nod ("aye"); Idle to Idle.

Reserved for order acknowledgements. Starts and ends exactly on Idle's
first pose (`stances.relaxed()`), at rest, feet planted.

0.00 Idle -> 0.10 the elbow folds -> 0.22 the right fist is at the brow,
elbow out to the side (the salute triangle reads from far), the chest
straightens -> 0.34 the nod: head and chest bob forward, a small knee dip
-> 0.46 back up, the hand flicks off the brow out to the side -> 0.62 the
arm lowering, still bent -> 0.80 Idle.
"""

from lib.stances import relaxed

DURATION = 0.8
LOOP = False
SHEET_TIMES = (0.0, 0.1, 0.22, 0.28, 0.34, 0.46, 0.56, 0.68, 0.8)

# The right wrist by the right brow, in the chest frame (metres from Spine2).
BROW = (0.16, 0.15, 0.25)


def build(clip):
    rest = relaxed()

    salute = rest.copy().hips(up=-0.02).spine(bend=-3).head(nod=-4)
    salute.shoulder("R", up=6).reach("R", at=BROW, frame="chest").elbow("R", out=55).wrist("R", flex=40, tilt=15)
    salute = clip.fk(salute)
    lift, toward, bend = salute.get("arm.R.lift"), salute.get("arm.R.toward"), salute.get("elbow.R.bend")

    # The hand travels up close to the body (elbow folds first), not out
    # on a straight arm.
    rising = rest.copy().hips(up=-0.015).arm("R", lift=lift * 0.4, toward=toward, twist=0).elbow("R", bend)
    rising.wrist("R", flex=25, tilt=8)

    nod = salute.copy().hips(up=-0.05, bend=4).spine(bend=10).head(nod=24)
    nod.arm("R", lift=lift - 6).elbow("R", bend + 6)
    away = salute.copy().hips(up=-0.03).spine(bend=2).head(nod=4)
    away.arm("R", lift=lift - 12, toward=toward + 28).elbow("R", bend - 22).wrist("R", flex=15, tilt=5)
    lowering = rest.copy().hips(up=-0.03).arm("R", lift=lift * 0.3, toward=toward + 20, twist=0).elbow("R", bend * 0.7)
    lowering.wrist("R", flex=8)

    # Ease out of Idle and back into it, so both crossfades are smooth.
    clip.key(0.0, rest, interp="SINE", easing="EASE_IN")
    clip.key(0.10, rising)
    clip.key(0.22, salute)
    clip.key(0.34, nod)
    clip.key(0.46, away)
    clip.key(0.62, lowering, interp="SINE", easing="EASE_OUT")
    clip.key(0.80, rest)
