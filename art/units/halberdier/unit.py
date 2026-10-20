"""Halberdier: halberd; a two-handed thrust."""

from lib import fight

PROPS = {"Gear_halberd": "halberd"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("thrust", "Gear_halberd")
CLIPS = fight.clips("halberdier", "thrust", "Gear_halberd")
