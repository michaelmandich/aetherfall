"""Militia Band: club; a one-handed stroke."""

from lib import fight

PROPS = {"Gear_club": "club"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("swing", "Gear_club")
CLIPS = fight.clips("militia", "swing", "Gear_club")
