"""Gatling Crew: gatling; fired from the hip."""

from lib import fight

PROPS = {"Gear_gatling": "gatling"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("hip", "Gear_gatling")
CLIPS = fight.clips("gatling_crew", "hip", "Gear_gatling")
