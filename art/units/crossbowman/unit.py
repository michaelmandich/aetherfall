"""Crossbowman: a heavy crossbow, aimed at the shoulder; no shield."""

from lib import stances

PROPS = {"Gear_crossbow": "crossbow"}  # Meshy's crossbow (meshy_props.py); the boxy one until it is there


def CARRY(clip):
    """The crossbow across the chest in both hands, in every shared clip."""
    return stances.crossbow_carry()
