"""Shock Trooper: shock maul; a two-handed overhead smash."""

from lib import fight

PROPS = {"Gear_shock_maul": "shock_maul"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("heavy", "Gear_shock_maul")
CLIPS = fight.clips("shock_trooper", "heavy", "Gear_shock_maul")
