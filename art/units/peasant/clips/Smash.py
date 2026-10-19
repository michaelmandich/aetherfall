"""Smash: a heavy overhead sledgehammer blow onto a meteor (gathering adamant).

0.00 the hammer rests on the meteor after the last blow, the body bent
over it -> 0.27 a dip, and the left hand slides up the haft to heave the
head off the rock -> 0.67 the hammer rises in front of the chest, well
forward of the face, head upright, the left hand sliding back down ->
1.00 high overhead: arms up (hands above the forehead), back arched, up
on the balls of the feet, the hammer head far above and behind -> 1.13
a hang at the top while the body turns over and starts down -> 1.40 HIT:
the whole body drops into the blow (deep knee and hip bend, the left hand
slid down to the right), body and hammer fastest as the face lands
square on the meteor's top -> the body sinks on through the blow while
the head rebounds a little -> 1.67 the head settles back on the rock.

Feet stay planted (IK). The swing is FK (arcs); IK holds the hammer on the
rock from the impact to the start of the next heave.
"""

from lib.clip import FRAME as T
from lib.pose import ARMS

DURATION = 2.0
LOOP = True
HIT = 42 * T
PROPS = ("Tool_smash",)
# Where the hammer face meets the meteor's top (right, fwd, up), metres.
TARGET = (0.0, 1.25, 0.5)
REACH = TARGET[1]  # the work surface lies this far ahead (manifest `reach`): the engine steps up to it
SHEET_TIMES = (0.0, 8 * T, 20 * T, 30 * T, 34 * T, 38 * T, 40 * T, HIT, 45 * T, 50 * T)


def stance(clip):
    p = clip.pose()
    p.foot("L", fwd=0.08, out=0.10, turn=8, knee=8)
    p.foot("R", fwd=-0.08, out=0.10, turn=14, knee=8)
    return p.look(TARGET)


def on_rock(clip, up, bend, spine, hold=0.10, lift=0.0):
    """Bent over the meteor with the hammer face on it (IK), `lift` metres up."""
    p = stance(clip)
    p.hips(up=up, fwd=0.03, bend=bend).spine(bend=spine)
    p.shoulder("L", fwd=10).shoulder("R", fwd=10)
    r, f, u = TARGET
    p.wield_part("Tool_smash", "face", (r, f, u + lift), pitch=85).hold("L", at=hold)
    return clip.fk(p, switch=False)


def build(clip):
    rest = on_rock(clip, up=-0.20, bend=16, spine=39)

    # Dip and choke up: the left hand slides up towards the head, turning
    # over the haft as it goes (keeps the wrist straight).
    heave = stance(clip)
    heave.hips(up=-0.21, fwd=0.0, bend=10).spine(bend=28)
    heave.wield(at=(0.08, 0.35, 0.82), pitch=40, roll=0).hold("L", at=0.32, roll=-40)
    heave = clip.fk(heave)

    # Rising in front of the chest, well forward of the face, the left hand
    # sliding back down before the haft passes the face.
    raise_ = stance(clip)
    raise_.hips(up=-0.08, fwd=-0.01, bend=4).spine(bend=6)
    raise_.wield(at=(0.06, 0.42, 1.15), pitch=-5, roll=0).hold("L", at=0.22)
    raise_ = clip.fk(raise_)

    # High overhead: the hands above the forehead (in front of the head's
    # line, so neither arm presses on the head), the head far back.
    top = stance(clip)
    top.hips(up=-0.03, fwd=-0.03, bend=-4).spine(bend=-12)
    top.foot("L", heel=8).foot("R", heel=8)
    top.shoulder("L", up=14).shoulder("R", up=14)
    top.wield(at=(0.0, 0.25, 1.60), pitch=-30, roll=0).hold("L", at=0.16)
    top = clip.fk(top)

    hit = on_rock(clip, up=-0.20, bend=16, spine=40)
    # After the blow: the body sinks on through it while the head rebounds a
    # little off the rock, then both settle.
    bounce = on_rock(clip, up=-0.22, bend=18, spine=45, lift=0.06)
    settle = on_rock(clip, up=-0.21, bend=17, spine=42)

    clip.key(0, rest)
    clip.key(8 * T, heave)
    clip.key(20 * T, raise_)
    clip.key(30 * T, top)
    # The top: the hammer hangs there while the body turns over and starts
    # down; body and arms then accelerate on one curve each, from rest, and
    # land together.
    clip.key(30 * T, top, skip=ARMS, interp="CUBIC", easing="EASE_IN")
    clip.key(34 * T, top, parts=ARMS, interp="CUBIC", easing="EASE_IN")
    clip.key(34 * T, top, parts=("wield.w",), interp="CONSTANT")
    # Impact: the hammer stops on the rock (IK from here) and bounces; the
    # body decelerates on through the blow.
    clip.key(HIT, hit, skip=ARMS, interp="CUBIC", easing="EASE_OUT")
    clip.key(HIT, hit, parts=ARMS, interp="QUAD", easing="EASE_OUT")
    clip.key(45 * T, bounce, interp="QUAD", easing="EASE_IN")
    clip.key(50 * T, settle)
