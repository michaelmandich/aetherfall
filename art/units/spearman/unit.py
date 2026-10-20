"""Spearman: a bronze-tipped spear in both hands; no shield."""

from lib import stances

# SOURCE: source/spearman.glb (the default), the rigged Meshy character.
PROPS = {"Gear_spear": "short_spear"}  # Gear_: always shown, so it is never empty-handed


def CARRY(clip):
    """The spear sloped forward in the right hand, in every shared clip."""
    return stances.spear_carry()
