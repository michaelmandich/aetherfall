"""Chip: crouched at an aether crystal, chisel in the left hand, mallet taps.

A low crouch (right knee forward and down, on the ball of the right foot)
with the chisel held to the crystal's face at 0.6 m, edge forward-down,
butt up and to the right, towards the mallet. Two taps per loop, a light
one and a hard one: 0.00 the mallet cocked a hand's width up -> 0.17 a
light tap on the chisel's butt, a small dip and nod -> a small rebound ->
0.53 the mallet swung right up to head height, 40 cm above the butt, the
elbow up, the body rising, the head tilting to check the cut -> 0.63 a
beat at the top while the body starts down -> 0.77 the hard tap: the body
dips into it and the head nods on contact -> rebound -> 1.00 the chisel
edged along to a new spot -> 1.20.

The mallet face meets the chisel's butt exactly on each tap: the butt is
found by solving the pose, and the mallet is placed so its face lands
there striking along the chisel (`mallet_at`); raised keys draw it back
up that line and swing its head up and back about the grip (the forearm
turns with it, so the wrist stays straight). The mallet arm is converted
to FK so the swings between keys are arcs. The left hand is IK (planted on
the work), its wrist nearly straight: the chisel's angle comes from the
elbow (held out) and the forearm. Feet planted.
"""

from lib.clip import FRAME as T
from lib.pose import ARMS
from lib.props import X_AXIS, Y_AXIS, prop_info
from lib.rig import at, from_world, rot

DURATION = 1.2
LOOP = True
PROPS = ("Tool_chip", "Tool_chip_l")
# Where the chisel's edge meets the crystal (right, fwd, up), metres. Two
# taps per loop, so no single HIT goes to the manifest.
TARGET = (-0.08, 0.55, 0.62)
REACH = TARGET[1]  # the work surface lies this far ahead (manifest `reach`): the engine steps up to it
TAPS = (5 * T, 23 * T)
SHEET_TIMES = (0.0, TAPS[0], 8 * T, 16 * T, 19 * T, TAPS[1], 26 * T, 30 * T)

CHISEL_WRIST = dict(flex=0, tilt=15, twist=0)  # with the elbow out: edge forward-down, butt up-right
CHISEL_ELBOW = 30                              # the left elbow swung out (degrees)
HAFT_UP = at(-0.4, 0.9, 0.1)                   # the mallet's haft leans this way (wrist straight)
MALLET = prop_info("Tool_chip")


def crouch(clip, up=0.0, fwd=0.0, bend=0.0, nod=0.0):
    """The crouch, bobbed `up` metres and bent `bend` degrees more, the head
    nodding `nod` degrees beyond looking at the work."""
    p = clip.pose()
    p.foot("L", fwd=0.10, out=0.05, turn=8)
    p.foot("R", fwd=-0.14, out=0.06, turn=15, heel=35)
    p.hips(up=-0.40 + up, fwd=-0.10 + fwd, bend=22 + bend).spine(bend=24 + bend)
    return p.look(TARGET).head(nod=nod)


def chisel(p, shift=0.0, push=0.0):
    """The chisel held to the crystal; `shift` moves it right, `push` into the cut."""
    p.reach("L", at=(-0.08 + shift, 0.46 + push, 0.70)).wrist("L", **CHISEL_WRIST)
    return p.elbow("L", out=CHISEL_ELBOW)


def mallet_at(clip, p, lift=0.0, cock=0.0, elbow=30.0):
    """Wield the mallet against the chisel's butt in pose `p` (its face on
    the butt, striking along the chisel), or raised from there: lifted
    `lift` metres back up the chisel's line with its head swung `cock`
    degrees up and back about the grip (the arm placed by IK to match, the
    elbow swung `elbow` degrees out). Returns the pose with the mallet arm
    as FK numbers."""
    solved = clip.solve(p)
    axis = (solved.prop_frame("Tool_chip_l").to_3x3() @ Y_AXIS).normalized()  # chisel edge -> butt
    strike = -axis
    haft = (HAFT_UP - strike * HAFT_UP.dot(strike)).normalized()
    yaw, pitch, roll = MALLET.angles(haft, strike)
    grip = MALLET.grip_for("face", solved.part_point("Tool_chip_l", "butt") + axis * lift, yaw, pitch, roll)
    if cock:
        orient = rot(strike.cross(haft), cock) @ MALLET.orientation(yaw, pitch, roll)
        yaw, pitch, roll = MALLET.angles(orient @ Y_AXIS, orient @ X_AXIS)
    p.wield(at=from_world(grip), yaw=yaw, pitch=pitch, roll=roll).elbow("R", out=elbow)
    return clip.fk(p)


def build(clip):
    # The light tap.
    cocked = mallet_at(clip, chisel(crouch(clip)), lift=0.12, cock=45, elbow=0)
    tap1 = mallet_at(clip, chisel(crouch(clip, up=-0.02, bend=3, nod=6), push=0.004))
    bounce1 = mallet_at(clip, chisel(crouch(clip, up=-0.01, bend=1, nod=2)), lift=0.06, cock=20)

    # The hard tap: the mallet swung up to head height, the body rising.
    high = crouch(clip, up=0.03, bend=-4).head(tilt=-8, nod=-6)
    high = mallet_at(clip, chisel(high), lift=0.40, cock=90, elbow=0)
    hang = crouch(clip, up=0.025, bend=-3).head(tilt=-6, nod=-4)
    hang = mallet_at(clip, chisel(hang), lift=0.42, cock=95)
    tap2 = mallet_at(clip, chisel(crouch(clip, up=-0.05, bend=6, nod=9), push=0.008))
    bounce2 = mallet_at(clip, chisel(crouch(clip, up=-0.035, bend=5, nod=6)), lift=0.08, cock=8)
    # The chisel edged along to a new spot.
    edge_on = crouch(clip).head(turn=-6)
    edge_on = mallet_at(clip, chisel(edge_on, shift=0.012), lift=0.12, cock=45, elbow=0)

    clip.key(0, cocked, interp="QUAD", easing="EASE_IN")
    clip.key(TAPS[0], tap1, interp="QUAD", easing="EASE_OUT")
    clip.key(8 * T, bounce1)
    clip.key(16 * T, high)
    # The body drops from the top of its rise straight into the hard tap;
    # the mallet hangs a beat longer at the top, then both land together.
    clip.key(16 * T, high, skip=ARMS, interp="QUAD", easing="EASE_IN")
    clip.key(19 * T, hang, parts=ARMS, interp="QUAD", easing="EASE_IN")
    clip.key(TAPS[1], tap2, interp="QUAD", easing="EASE_OUT")
    clip.key(26 * T, bounce2)
    clip.key(30 * T, edge_on)
