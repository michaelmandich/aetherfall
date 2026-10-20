"""Rifle Squad: rifle; shouldered and fired."""

from lib import fight

PROPS = {"Gear_rifle": "rifle"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("gun", "Gear_rifle")
CLIPS = fight.clips("rifle_squad", "gun", "Gear_rifle")
