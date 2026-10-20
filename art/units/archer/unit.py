"""Archer: short bow; draw and loose."""

from lib import fight

PROPS = {"Gear_short_bow": "short_bow"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("bow", "Gear_short_bow")
CLIPS = fight.clips("archer", "bow", "Gear_short_bow")
