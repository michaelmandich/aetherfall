"""Star Druid: sylvan starcall; raised and pointed at the target."""

from lib import fight

PROPS = {"Gear_sylvan_starcall": "sylvan_starcall"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("cast", "Gear_sylvan_starcall")
CLIPS = dict(fight.clips("sylvan_druid", "cast", "Gear_sylvan_starcall"), **fight.heal("Gear_sylvan_starcall"))
