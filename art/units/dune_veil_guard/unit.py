"""Veiled Blades: dune khanjar; a one-handed stroke."""

from lib import fight

PROPS = {"Gear_dune_khanjar": "dune_khanjar"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("swing", "Gear_dune_khanjar")
CLIPS = fight.clips("dune_veil_guard", "swing", "Gear_dune_khanjar")
