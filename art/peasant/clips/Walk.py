"""Walk: the source "Walking" cycle, made in place and resampled to 30 fps.

Moving without cargo; the engine scales the playback rate to the unit's
real speed using the manifest's measured `speed`.
"""

DURATION = None  # the source cycle: 31 frames
LOOP = True
LOCOMOTION = True


def build(clip):
    clip.from_source("Walking")
