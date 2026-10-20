"""Steamplate Thane: ironhold piston hammer; a two-handed overhead smash."""

from lib import fight

PROPS = {"Gear_ironhold_piston_hammer": "ironhold_piston_hammer"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("heavy", "Gear_ironhold_piston_hammer")
CLIPS = fight.clips("ironhold_ironclad", "heavy", "Gear_ironhold_piston_hammer")
