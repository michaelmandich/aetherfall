"""Marine Band: tide boarding axe and tide buckler shield; a one-handed stroke."""

from lib import fight

PROPS = {"Gear_tide_boarding_axe": "tide_boarding_axe", "Gear_tide_buckler_shield": "tide_buckler_shield"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("swing", "Gear_tide_boarding_axe", "Gear_tide_buckler_shield")
CLIPS = fight.clips("tide_marine", "swing", "Gear_tide_boarding_axe", "Gear_tide_buckler_shield")
