"""Man-at-Arms: arming sword and heater shield; a one-handed stroke."""

from lib import fight

PROPS = {"Gear_arming_sword": "arming_sword", "Gear_heater_shield": "heater_shield"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("swing", "Gear_arming_sword", "Gear_heater_shield")
CLIPS = fight.clips("man_at_arms", "swing", "Gear_arming_sword", "Gear_heater_shield")
