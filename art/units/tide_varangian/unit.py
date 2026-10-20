"""Varangian Guard: tide varangian axe; a two-handed overhead smash."""

from lib import fight

PROPS = {"Gear_tide_varangian_axe": "tide_varangian_axe"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("heavy", "Gear_tide_varangian_axe")
CLIPS = fight.clips("tide_varangian", "heavy", "Gear_tide_varangian_axe")
