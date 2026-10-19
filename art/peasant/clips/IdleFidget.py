"""IdleFidget: a stretch, a scratch of the head, a look round; back to Idle.

Played now and then over Idle, so it starts and ends exactly on Idle's
first pose (`stances.relaxed()`), easing out of it and settling back into
it at rest, for clean crossfades both ways.

0.00 Idle -> 0.25 gather (a small dip, shoulders round, head drops)
-> 0.70 the stretch: up on the toes, both arms straight up, back arched,
face to the sky -> 1.00 a sway to the left at full stretch -> 1.16 the
left arm sweeps down out to the side -> 1.32 the right hand is on the
back of the head, the forearm folded up behind it from the side ->
1.42..1.86 three scratches (the hand rubs, the head tilts into it) ->
2.05 the hand comes down beside the shoulder, elbow still bent -> 2.10
the head turns left, looking round, while the arm unfolds to hang (2.36)
-> 2.48 and right -> 2.78 front again -> 3.00 Idle.
"""

from lib.stances import relaxed

DURATION = 3.0
LOOP = False
SHEET_TIMES = (0.0, 0.25, 0.7, 1.0, 1.16, 1.32, 1.5, 1.66, 1.96, 2.1, 2.3, 2.48, 3.0)


def build(clip):
    rest = relaxed()

    gather = rest.copy().hips(up=-0.05, fwd=0.01).spine(bend=8).head(nod=14)
    gather.shoulder("L", fwd=8).shoulder("R", fwd=8)
    gather.arm("L", lift=14, toward=30).arm("R", lift=14, toward=30).elbow("L", 30).elbow("R", 30)

    stretch = rest.copy().hips(up=0.0, fwd=-0.01).spine(bend=-14).head(nod=-26)
    stretch.foot("L", heel=14).foot("R", heel=14)
    stretch.shoulder("L", up=16).shoulder("R", up=16)
    stretch.arm("L", lift=168, toward=20, twist=20).arm("R", lift=168, toward=20, twist=20)
    stretch.elbow("L", 12).elbow("R", 12).wrist("L", flex=-25).wrist("R", flex=-25)

    sway = stretch.copy().hips(right=0.03, lean=3).spine(lean=-12, bend=-10).head(nod=-18, tilt=-8)
    sway.foot("L", heel=8).foot("R", heel=8)
    sway.arm("L", lift=172, toward=6).arm("R", lift=165, toward=10)

    scratch = rest.copy().hips(right=0.02, lean=-2).spine(lean=3, bend=2).head(tilt=12, nod=8)
    # The upper arm out to the side, the forearm folded up behind the head
    # (it clears the hair by 4 cm or more), only the fingers on the back of
    # the head.
    scratch.shoulder("R", up=8).arm("R", lift=140, toward=85, twist=125).elbow("R", 120).wrist("R", flex=40)
    rub = scratch.copy().arm("R", twist=121).elbow("R", 123).wrist("R", flex=50)
    lean_in = scratch.copy().head(tilt=17, nod=10, turn=4)

    # The hand comes down from the head close to the shoulder, elbow still
    # bent, then the arm unfolds to hang.
    lower = scratch.copy().shoulder("R", up=3).arm("R", lift=50, toward=62, twist=65).elbow("R", 128)
    lower.wrist("R", flex=10)
    look_left = rest.copy().hips(right=0.015, turn=-4).spine(turn=-10).head(turn=-40, nod=2, tilt=-3)
    look_left.arm("R", lift=14, toward=65, twist=35).elbow("R", 40)
    look_right = rest.copy().hips(right=-0.015, turn=4).spine(turn=10).head(turn=40, nod=-2, tilt=3)

    clip.key(0.0, rest)
    clip.key(0.25, gather)
    clip.key(0.70, stretch)
    clip.key(1.00, sway)
    # The left arm comes down out to the side in a wide arc, not forward.
    clip.key(1.16, sway.copy().arm("L", lift=100, toward=85, twist=10).elbow("L", 14).wrist("L", flex=-10),
             parts=("arm.L", "elbow.L", "wrist.L"))
    clip.key(1.32, scratch)
    # Three rubs: the forearm and hand rock while the head leans into them.
    hand = ("arm.R", "wrist.R", "elbow.R")
    for t in (1.42, 1.58, 1.74):
        clip.key(t, rub, parts=hand)
        clip.key(t + 0.08, scratch, parts=hand)
    clip.key(1.66, lean_in, skip=hand)
    clip.key(1.86, scratch, skip=hand)
    r_arm = ("shoulder.R", "arm.R", "elbow.R", "wrist.R")
    clip.key(2.05, lower, parts=r_arm)
    clip.key(2.10, look_left, skip=r_arm)
    clip.key(2.36, look_left, parts=r_arm)
    clip.key(2.48, look_right)
    clip.key(2.78, rest.copy().head(turn=5), parts=("head", "spine", "hips"))
    clip.key(2.78, rest, skip=("head", "spine", "hips"))
    clip.key(3.0, rest)
