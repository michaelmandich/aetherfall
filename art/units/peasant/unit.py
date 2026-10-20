"""Peasant: the worker. Every shared clip, the work clips, cargo and the Work Axe."""

from lib import props

SOURCE = "peasant/source/peasant_meshy.glb"
# A tool per work role, the Work Axe (Tool_attack: shown only while
# fighting) and the cargo: all shown and hidden by the engine.
PROPS = dict(props.WORKER)
# Placed by hand on this body before the points could come from the
# skeleton; props.sash_anchors() reproduces them to 1 mm.
SASH = {"shoulder": (0.165, 0.0, 1.33), "hip": (-0.16, 0.0, 0.99)}
