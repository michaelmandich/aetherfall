"""Brass Diver: tide galvanic harpoon; shouldered and fired."""

from lib import fight

PROPS = {"Gear_tide_galvanic_harpoon": "tide_galvanic_harpoon"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("gun", "Gear_tide_galvanic_harpoon")
CLIPS = fight.clips("tide_diver", "gun", "Gear_tide_galvanic_harpoon")
