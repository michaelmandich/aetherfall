"""Sunspire Guard: dune scimitar; a one-handed stroke."""

from lib import fight

PROPS = {"Gear_dune_scimitar": "dune_scimitar"}  # Meshy props (meshy_props.py), always shown
CARRY = fight.carry("swing", "Gear_dune_scimitar")
CLIPS = fight.clips("dune_sun_guard", "swing", "Gear_dune_scimitar")
