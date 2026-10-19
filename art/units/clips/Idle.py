"""Idle: standing at ease, no work, no cargo.

One slow breath and one weight shift per loop: the pelvis drifts over the
right foot and back (the free hip drops, the shoulders counter-tilt), the
chest lifts on the in-breath, the head glances right then left a beat
ahead of the body. Feet stay planted (IK).
"""

from lib.stances import relaxed

DURATION = 3.0
LOOP = True


def build(clip):
    stand = relaxed()

    # Weight over the right leg: pelvis right, left hip drops, chest counters.
    right = stand.copy().hips(right=0.045, lean=-4.0, turn=-3, up=-0.024)
    right.spine(lean=4.5, bend=-2.5).shoulder("L", up=-1).shoulder("R", up=3)
    right.arm("L", lift=10).elbow("R", 18).head(tilt=-2.5, turn=16, nod=-3)

    back = stand.copy().hips(right=0.01, lean=-0.5, up=-0.032).spine(bend=1.5)
    back.head(turn=-13, nod=3).elbow("L", 18)

    clip.key(0.0, stand)
    clip.key(1.1, right, skip=("head",))
    clip.key(2.2, back, skip=("head",))
    clip.key(0.85, right, parts=("head",))
    clip.key(1.95, back, parts=("head",))
