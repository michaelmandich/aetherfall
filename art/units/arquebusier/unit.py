"""Arquebusier: arquebus; shouldered and fired."""

from lib import fight

PROPS = {"Gear_arquebus": "arquebus"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("gun", "Gear_arquebus")
CLIPS = fight.clips("arquebusier", "gun", "Gear_arquebus")
