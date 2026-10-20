"""Delver Band: ironhold mattock; a two-handed overhead smash."""

from lib import fight

PROPS = {"Gear_ironhold_mattock": "ironhold_mattock"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("heavy", "Gear_ironhold_mattock")
CLIPS = fight.clips("ironhold_delver_band", "heavy", "Gear_ironhold_mattock")
