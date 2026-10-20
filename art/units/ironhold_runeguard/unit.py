"""Runeguard: ironhold thunder rune; raised and pointed at the target."""

from lib import fight

PROPS = {"Gear_ironhold_thunder_rune": "ironhold_thunder_rune"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("cast", "Gear_ironhold_thunder_rune")
CLIPS = fight.clips("ironhold_runeguard", "cast", "Gear_ironhold_thunder_rune")
