"""HitReact: a quick flinch of the upper body; Idle to Idle.

The engine plays it over calm states (Idle, Walk and the carry states)
when the unit loses health, so it starts and ends exactly on Idle's first
pose (`stances.relaxed()`), at rest. The feet stay planted, so over Walk
or a carry state it should only drive the spine, neck and head (and the
arms only when the hands are empty); over Idle it plays whole.

Body (pelvis, spine, head): 0.00 Idle -> 0.07 the blow: the chest snaps
back and twists away, the head jerks back and aside, the pelvis is knocked
back -> 0.16 a hold at the recoil -> 0.26 rebound past upright (chest dips
forward, head down) -> 0.40 Idle.

Arms, keyed on their own a beat behind: the hands lag where they hung
while the chest snaps away (0.07), are flung out and back by the recoil,
the shoulders hunched (0.17), hang there (0.24), swing back through, just
past the thighs (0.33), and settle (0.40).
"""

from lib.pose import ARMS
from lib.stances import relaxed

DURATION = 0.4
LOOP = False
SHEET_TIMES = (0.0, 0.03, 0.07, 0.1, 0.13, 0.17, 0.24, 0.29, 0.33, 0.4)


def build(clip):
    rest = relaxed()

    struck = rest.copy().hips(fwd=-0.045, up=-0.045, bend=-6, turn=6).spine(bend=-16, turn=14, lean=-5)
    struck.head(nod=-22, turn=18, tilt=-10)
    recoil = struck.copy().hips(fwd=-0.05, up=-0.055).spine(bend=-13, turn=12).head(nod=-16, turn=15)
    rebound = rest.copy().hips(fwd=0.01, up=-0.04, turn=-2).spine(bend=7, turn=-3).head(nod=10, turn=-4)

    # The hands hang back where they were while the chest snaps away, then
    # the recoil flings them out and back, the left (the side the blow came
    # from) further, wrists trailing; they hang there a moment, then fall
    # back through, just past the thighs.
    lag = struck.copy().arm("L", lift=20, toward=140).arm("R", lift=12, toward=105)
    flung = recoil.copy().shoulder("L", up=12).shoulder("R", up=10)
    flung.arm("L", lift=46, toward=135).elbow("L", 25).arm("R", lift=40, toward=140).elbow("R", 28)
    flung.wrist("L", flex=-25).wrist("R", flex=-20)
    hang = flung.copy().shoulder("L", up=6).shoulder("R", up=5)
    hang.arm("L", lift=34).arm("R", lift=31).wrist("L", flex=-10).wrist("R", flex=-8)
    through = rebound.copy().arm("L", lift=10, toward=55).elbow("L", 22).arm("R", lift=10, toward=45)
    through.elbow("R", 22).wrist("L", flex=6).wrist("R", flex=6)

    # The body snaps away at once and decelerates into the recoil
    # (`interp` shapes the segment that leaves a key).
    clip.key(0.0, rest, skip=ARMS, interp="QUAD", easing="EASE_OUT")
    clip.key(0.07, struck, skip=ARMS)
    clip.key(0.16, recoil, skip=ARMS)
    clip.key(0.26, rebound, skip=ARMS)
    clip.key(0.40, rest, skip=ARMS)

    # The arms counter the snap (the same easing, so the hands hang back
    # rather than being thrown forward by the chest), then swing on smooth
    # curves.
    clip.key(0.0, rest, parts=ARMS, interp="QUAD", easing="EASE_OUT")
    clip.key(0.07, lag, parts=ARMS)
    clip.key(0.17, flung, parts=ARMS)
    clip.key(0.24, hang, parts=ARMS)
    clip.key(0.33, through, parts=ARMS)
    clip.key(0.40, rest, parts=ARMS)
