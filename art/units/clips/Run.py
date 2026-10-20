"""Run: the source "Running" cycle, made in place and resampled to 30 fps.

Reserved (fleeing).
"""

DURATION = None  # the source cycle: 19 frames
LOOP = True
LOCOMOTION = True


def build(clip):
    clip.from_source("Running")
