"""Hearthguard Pair: ironhold blunderbuss; shouldered and fired."""

from lib import fight

PROPS = {"Gear_ironhold_blunderbuss": "ironhold_blunderbuss"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("gun", "Gear_ironhold_blunderbuss")
CLIPS = fight.clips("ironhold_hearthguard", "gun", "Gear_ironhold_blunderbuss")
