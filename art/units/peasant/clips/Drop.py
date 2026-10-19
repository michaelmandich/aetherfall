"""Drop: puts the carried load down at a drop site, ends in Idle, hands empty.

Starts exactly on CarryIdle's first pose (`stances.carrying()`) and ends
exactly on Idle's (`stances.relaxed()`). In the gather loop the unit walks
off at once, and the engine cuts the clip at af_render's `DROP_HOLD`
(0.3 s), hiding the load then (otherwise it hides it at half the clip,
0.35 s), so the load is down, level, by 0.23 s:

0.00 carrying -> 0.07 the hips start to hinge and the load swings out
from the belly -> 0.12 half way down, ahead of the knees -> 0.23 down: a
deep hip hinge over bent, spread knees, the load set level on the ground
in front of them, eyes on it -> 0.33 the body settles onto it (the load
stays put) -> 0.42 the hands let go and open outwards -> 0.55
straightening up, arms swinging back down -> 0.70 Idle.

There is no anticipation hitch: the engine's 0.2 s crossfade into the
clip would swallow it, and the set-down needs every frame up to 0.23 s.
The load is placed in the character's root frame (not the chest's), so it
lands exactly on the floor, level, and stays where it was put while the
body settles. The arms hold it with IK (carry); on the settle key they
are re-expressed as FK numbers (`Clip.fk`), so the release and the swing
back to Idle's hanging arms travel along arcs.
"""

import math

from lib.clip import FRAME as T
from lib.props import Z_AXIS
from lib.rig import from_world
from lib.stances import carrying, relaxed

DURATION = 0.7
LOOP = False
PROPS = ("Load_wood",)
# The engine hides the load by 0.35 s at the latest (at af_render's
# DROP_HOLD, 0.3 s, or half the clip); the build's checks stop there.
PROPS_UNTIL = 0.35
# One load per sheet column (they share one grip). The sheet keeps showing
# it after the release.
SHEET_PROPS = ("Load_wood", "Load_food", "Load_stone", "Load_aether", "Load_iron", "Load_silicon",
               "Load_copper", "Load_adamant")
SHEET_TIMES = (0.0, 0.07, 0.13, 0.2, 0.23, 0.3, 0.42, 0.5, 0.6, 0.7)

# Where the load is set down (its centre), in the root frame: level, its
# bottom on the floor (the loads' undersides are 0.10-0.11 m below their
# centres), its near end well in front of the knees.
SET_DOWN = (0.0, 0.50, 0.108)
# The load's path on the way down (centre, root frame): out from the belly,
# then down ahead of the knees.
SWING_OUT = (0.0, 0.43, 0.94)
LOWERING = (0.0, 0.47, 0.60)
LOOK_AT = (0.0, 0.55, 0.0)
CONTACT = 7 * T    # the load touches the floor
SWITCH = 10 * T    # IK -> FK (the load is hidden by now)
BEATS = (SWITCH,)  # the hands rest on the set-down load until they let go
HANDS = ("L", "R")


def in_root_frame(clip, pose):
    """`pose` with its load placed by root-frame numbers instead of
    chest-frame ones (the placement itself is unchanged), so that keys from
    it to the root-frame set-down interpolate within one frame."""
    solved = clip.solve(pose)
    m = solved.prop_pose(solved.held_prop(PROPS[0]))
    _, fwd, up = from_world(m.to_3x3() @ Z_AXIS)  # the load's up axis
    return pose.copy().carry(at=from_world(m.translation), pitch=math.degrees(math.atan2(fwd, up)), frame="root")


def build(clip):
    hold = in_root_frame(clip, carrying())

    # A deep hip hinge over bent, spread knees: the shoulders come out over
    # the load, so the arms reach down in front of the knees, not between
    # them, and the wrists stay nearly straight.
    down = hold.copy().hips(up=-0.40, fwd=-0.10, bend=65).spine(bend=50).head(nod=4)
    down.shoulder("L", fwd=30).shoulder("R", fwd=30)
    down.foot("L", knee=14).foot("R", knee=14)
    down.carry(at=SET_DOWN, pitch=0.0, frame="root").look(LOOK_AT)
    # The body settles a touch onto the set-down load; the load stays put.
    settle = down.copy().hips(up=-0.41, bend=66).spine(bend=51)
    settle_ik = clip.fk(settle, arms=HANDS, switch=False)  # still on IK, FK numbers filled in
    settle_fk = clip.fk(settle, arms=HANDS)                 # the same pose on FK: the hands can let go
    # The load's path down, keyed on its own (the body interpolates
    # straight from `hold` to `down`).
    swing_out = hold.copy().carry(at=SWING_OUT, pitch=0.0, frame="root")
    lowering = down.copy().carry(at=LOWERING, pitch=0.0, frame="root")

    # Let go: the hands open outwards off the sides of the load.
    let_go = settle_fk.copy().hips(up=-0.39).spine(bend=36)
    for s in HANDS:
        let_go.arm(s, toward=settle_fk.get(f"arm.{s}.toward") + 6).elbow(s, settle_fk.get(f"elbow.{s}.bend") - 12)
        let_go.wrist(s, flex=0, tilt=0)

    rise = relaxed().hips(up=-0.12, fwd=-0.02, bend=8).spine(bend=14).look(LOOK_AT, weight=0.4)
    rise.arm("L", lift=24, toward=40).arm("R", lift=24, toward=40).elbow("L", 28).elbow("R", 28)

    clip.key(0.0, hold)
    clip.key(2 * T, swing_out, parts=("wield",))
    clip.key(0.12, lowering, parts=("wield",))
    clip.key(CONTACT, down)
    clip.key(0.30, settle_ik)
    clip.key(SWITCH, settle_fk)  # IK -> FK over one frame, on the same pose
    clip.key(0.42, let_go)
    clip.key(0.55, rise)
    clip.key(0.70, relaxed())
