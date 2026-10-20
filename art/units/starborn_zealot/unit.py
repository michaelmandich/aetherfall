"""Zealot Band: starborn scourge; a one-handed stroke."""

from lib import fight

PROPS = {"Gear_starborn_scourge": "starborn_scourge"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("swing", "Gear_starborn_scourge")
CLIPS = fight.clips("starborn_zealot", "swing", "Gear_starborn_scourge")
