"""Cheer: both fists thrown up, pumped three times; Idle to Idle.

Played when the player's own age-up completes, over Idle, so it starts
and ends exactly on Idle's first pose (`stances.relaxed()`), at rest.
Feet stay planted (the heels lift on each throw).

0.00 Idle -> 0.20 gather (knees dip, chest curls, fists down by the hips)
-> 0.45 THROW: up on the toes, both fists high in a V, chest open, face
up -> 0.62 pump down (upper arms level out to the sides, fists up by the
ears: a "W") -> 0.80 up -> 0.97 down -> 1.18 up, highest -> 1.55 the
arms come down out to the sides, still pleased -> 2.00 Idle.
"""

from lib.pose import ARMS
from lib.stances import relaxed

DURATION = 2.0
LOOP = False
SHEET_TIMES = (0.0, 0.2, 0.45, 0.62, 0.8, 0.97, 1.18, 1.4, 1.65, 2.0)


def fists(p, lift, toward, elbow, shrug, twist=10):
    """Both arms alike (mirrored), hands balled into fists."""
    for s in ("L", "R"):
        p.arm(s, lift=lift, toward=toward, twist=twist).elbow(s, elbow).wrist(s, flex=35, tilt=10)
        p.shoulder(s, up=shrug)
    return p


def build(clip):
    rest = relaxed()

    gather = rest.copy().hips(up=-0.09, fwd=0.01, bend=6).spine(bend=12).head(nod=16)
    gather.foot("L", knee=6).foot("R", knee=6)
    fists(gather, lift=16, toward=40, elbow=70, shrug=-2)

    up = rest.copy().hips(up=0.005).spine(bend=-12).head(nod=-20)
    up.foot("L", heel=18).foot("R", heel=18)
    fists(up, lift=150, toward=72, elbow=14, shrug=16)

    pump = rest.copy().hips(up=-0.045).spine(bend=-4).head(nod=-8)
    pump.foot("L", heel=4).foot("R", heel=4)
    fists(pump, lift=95, toward=82, elbow=90, shrug=6, twist=80)

    high = up.copy().spine(bend=-15).head(nod=-24).hips(up=0.01)
    high.foot("L", heel=20).foot("R", heel=20)
    fists(high, lift=157, toward=68, elbow=10, shrug=18)

    settle = rest.copy().hips(up=-0.035).spine(bend=-3).head(nod=-4, turn=8)
    fists(settle, lift=42, toward=80, elbow=50, shrug=2, twist=20)

    clip.key(0.0, rest)
    # Body first, the arms snap up a beat behind and overshoot a little
    # (`interp` shapes the segment that leaves a key).
    clip.key(0.20, gather, skip=ARMS)
    clip.key(0.20, gather, parts=ARMS, interp="BACK", easing="EASE_OUT")
    clip.key(0.42, up, skip=ARMS)
    clip.key(0.45, up, parts=ARMS)
    clip.key(0.62, pump)
    clip.key(0.80, up)
    clip.key(0.97, pump)
    clip.key(1.18, high)
    clip.key(1.55, settle)
    clip.key(2.0, rest)
