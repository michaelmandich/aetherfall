"""Thane Guard: ironhold dane axe; a two-handed overhead smash."""

from lib import fight

PROPS = {"Gear_ironhold_dane_axe": "ironhold_dane_axe"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("heavy", "Gear_ironhold_dane_axe")
CLIPS = fight.clips("ironhold_thane_guard", "heavy", "Gear_ironhold_dane_axe")
