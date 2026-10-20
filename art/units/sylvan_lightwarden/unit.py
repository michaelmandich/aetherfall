"""Crystal Ranger: sylvan crystal lens; raised and pointed at the target."""

from lib import fight

PROPS = {"Gear_sylvan_crystal_lens": "sylvan_crystal_lens"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("cast", "Gear_sylvan_crystal_lens")
CLIPS = fight.clips("sylvan_lightwarden", "cast", "Gear_sylvan_crystal_lens")
