"""Lumen knight (Starborn): white-enamelled plate and a one-handed blade of solid light."""

from lib import stances

# Meshy's lightblade once it is generated (meshy_props.py); a glowing boxy sword until then.
PROPS = {"Gear_sword": "starborn_lightblade"}


def CARRY(clip):
    """The blade upright before the right shoulder, in every shared clip."""
    return stances.sword_carry()
