"""Slinger Band: sling; swung and whipped forward."""

from lib import fight

PROPS = {"Gear_sling": "sling"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("sling", "Gear_sling")
CLIPS = fight.clips("slinger", "sling", "Gear_sling")
