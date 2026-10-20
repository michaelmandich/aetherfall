"""Shield Trooper: plasma carbine and adamant shield; fired one-handed."""

from lib import fight

PROPS = {"Gear_plasma_carbine": "plasma_carbine", "Gear_adamant_shield": "adamant_shield"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("gun1h", "Gear_plasma_carbine", "Gear_adamant_shield")
CLIPS = fight.clips("shield_trooper", "gun1h", "Gear_plasma_carbine", "Gear_adamant_shield")
