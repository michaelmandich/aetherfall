"""Longbowman: sylvan yew longbow; draw and loose."""

from lib import fight

PROPS = {"Gear_sylvan_yew_longbow": "sylvan_yew_longbow"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("bow", "Gear_sylvan_yew_longbow")
CLIPS = fight.clips("sylvan_longbowman", "bow", "Gear_sylvan_yew_longbow")
