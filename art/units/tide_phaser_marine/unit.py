"""Phaser Marines: tide phaser; shouldered and fired."""

from lib import fight

PROPS = {"Gear_tide_phaser": "tide_phaser"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("gun", "Gear_tide_phaser")
CLIPS = fight.clips("tide_phaser_marine", "gun", "Gear_tide_phaser")
