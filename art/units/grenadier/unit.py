"""Grenadier Pair: grenade; an overhand throw."""

from lib import fight

PROPS = {"Gear_grenade": "grenade"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("throw", "Gear_grenade")
CLIPS = fight.clips("grenadier", "throw", "Gear_grenade")
