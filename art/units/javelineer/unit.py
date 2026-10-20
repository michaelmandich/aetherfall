"""Javelineer: javelin and round shield; an overhand throw."""

from lib import fight

PROPS = {"Gear_javelin": "javelin", "Gear_round_shield": "round_shield"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("throw", "Gear_javelin", "Gear_round_shield")
CLIPS = fight.clips("javelineer", "throw", "Gear_javelin", "Gear_round_shield")
