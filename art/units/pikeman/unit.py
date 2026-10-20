"""Pikeman: pike; a two-handed thrust."""

from lib import fight

PROPS = {"Gear_pike": "pike"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("thrust", "Gear_pike")
CLIPS = fight.clips("pikeman", "thrust", "Gear_pike")
