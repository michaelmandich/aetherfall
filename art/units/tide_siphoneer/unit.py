"""Fire Siphoneers: tide siphon; fired from the hip."""

from lib import fight

PROPS = {"Gear_tide_siphon": "tide_siphon"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("hip", "Gear_tide_siphon")
CLIPS = fight.clips("tide_siphoneer", "hip", "Gear_tide_siphon")
