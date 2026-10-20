"""Lightbearer: starborn sunlens; raised and pointed at the target."""

from lib import fight

PROPS = {"Gear_starborn_sunlens": "starborn_sunlens"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("cast", "Gear_starborn_sunlens")
CLIPS = fight.clips("starborn_lightbearer", "cast", "Gear_starborn_sunlens")
