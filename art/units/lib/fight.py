"""Fight clips by weapon family: a unit's Attack and CombatIdle, and the
CARRY pose that holds its weapon (and shield) in the shared clips.

A unit's config names its family and props:

    from lib import fight
    PROPS = {"Gear_pike": "pike"}
    CARRY = fight.carry("thrust", "Gear_pike")
    CLIPS = fight.clips("pikeman", "thrust", "Gear_pike")

Families (the poses of each were searched for straight wrists on a real
Meshy body with a real Meshy weapon, then scaled to every other body by
its height):

    thrust   two-handed polearm thrust (pike, halberd), the spearman's
    swing    one-handed stroke (sword, axe, club, dagger, flail), the lumen
             knight's; with a shield on the left forearm, or a guard hand
    heavy    two-handed overhead smash (great axe, maul, mattock)
    bow      draw to the cheek and loose (the bow in the left hand)
    gun      shouldered and fired (arquebus, rifle, blunderbuss, beam gun),
             like the crossbowman's crossbow
    hip      fired from the hip with both hands (gatling, siphon)
    gun1h    fired one-handed, a shield on the left arm (carbine)
    throw    overhand throw (javelin, grenade), with an optional shield
    sling    the sling swung up behind the head and whipped forward
    cast     staff, lens or rune weapon raised and pointed at the target;
             with weapon=None, both hands (a mage's spell)

HIT is the unit's weapon windup (assets/data, lib/gamedata.py), on a
whole frame, and the clip ends well inside the weapon's cooldown. The hand
paths are fixed and the weapon's own geometry decides where its head goes,
so a shorter or longer weapon of the family keeps the same swing.
`heal()` is the healers' Heal loop.
"""

from types import SimpleNamespace

from . import gamedata, props
from .clip import FRAME, ClipError
from .pose import ARMS, Pose
from .rig import ARM, at, from_world

FPS = 30


def _frames(t):
    return round(t * FPS) * FRAME


def _fk(clip, pose, arms=("R",)):
    """clip.fk, except that a hand target out of this body's reach (short
    arms for its height) is drawn in toward the shoulder a few per cent at
    a time until the hand gets there. A body that reaches keeps the pose
    exactly as written."""
    for _ in range(12):
        try:
            return clip.fk(pose, arms=arms)
        except ClipError as e:
            side = next((s for s in arms if f"arm.{s} misses" in str(e)), None)
            solver = clip.solve(pose)
            key = "wield" if side == solver.wield_side and pose.get("wield.w") > 0.0 else f"reach.{side}"
            if side is None or pose.get(f"{key}.chest") > 0.0:
                raise
            shoulder = from_world(solver.P[ARM[side][1]])
            pose = pose.copy()
            for axis, c in zip(("right", "fwd", "up"), shoulder):
                pose.set(f"{key}.{axis}", c + 0.96 * (pose.get(f"{key}.{axis}") - c))
    return clip.fk(pose, arms=arms)


def _k(clip, ref):
    """The body's height against the height the family was searched on."""
    return getattr(clip.ctx.rig, "height", ref) / ref


def _sc(k, p):
    return tuple(k * x for x in p)


def _feet(p, k, lead_fwd=0.15, back_fwd=-0.13, back_out=0.07, back_turn=28):
    p.foot("L", fwd=lead_fwd * k, out=0.03 * k, turn=-5, knee=8)
    p.foot("R", fwd=back_fwd * k, out=back_out * k, turn=back_turn, knee=10)
    return p


def _grip_for(kind, part, target, yaw, pitch, roll):
    """Where (right, fwd, up) the main hand goes so that `part` of the boxy
    `kind` lands on `target`: the hand path the family was searched with."""
    g = props.prop_info(kind).grip_for(part, at(*target), yaw, pitch, roll)
    return from_world(g)


def _module(doc, duration, loop, held, build, hit=None, sheet=None, beats=()):
    m = SimpleNamespace(__doc__=doc, DURATION=duration, LOOP=loop, PROPS=tuple(h for h in held if h), build=build)
    if hit is not None:
        m.HIT = hit
    if sheet:
        m.SHEET_TIMES = tuple(sheet)
    if beats:
        m.BEATS = tuple(beats)
    return m


def _idle(name, ready, low, held):
    """CombatIdle: a springy guard, the body sinking once per loop and the
    arms following a beat later."""
    def build(clip):
        clip.key(0.0, ready(clip))
        lo = low(clip)
        clip.key(0.7, lo, skip=ARMS)
        clip.key(0.84, lo, parts=ARMS)
    return _module(f"CombatIdle: {name}, between blows.", 1.5, True, held, build)


# -- thrust: two-handed polearm (the spearman's thrust, lib/stances.py) -------

THRUST_REF = 1.70
# The spearman's hit and full extension, as hand positions for the boxy spear's tip.
_THRUST_HIT = _grip_for("spear", "tip", (0.0, 1.62, 1.15), -10, 80, 0)
_THRUST_REACH = _grip_for("spear", "tip", (0.0, 1.68, 1.13), -9, 81, 2)


def _thrust_ready_ik(k):
    p = _feet(Pose(), k, 0.17, -0.15, 0.08, 35)
    p.hips(up=-0.09 * k, turn=21, bend=6).spine(bend=6, turn=17.5).look(_sc(k, (0.0, 1.6, 1.15)))
    return p.wield(at=_sc(k, (0.21, -0.04, 0.92)), pitch=80, yaw=-6, roll=-30).hold("L", at=0.4, roll=-45)


def _thrust(unit, weapon, shield, hit):
    def ready(clip):
        return _fk(clip, _thrust_ready_ik(_k(clip, THRUST_REF)))

    def low(clip):
        k = _k(clip, THRUST_REF)
        p = _thrust_ready_ik(k).hips(up=-0.13 * k, fwd=0.02 * k, turn=19).spine(bend=10, turn=16)
        return _fk(clip, p.wield(at=_sc(k, (0.21, -0.02, 0.89)), pitch=78, yaw=-7, roll=-35))

    end = _frames(hit + 0.5)

    def attack(clip):
        k = _k(clip, THRUST_REF)
        dip = _thrust_ready_ik(k).hips(up=-0.12 * k, turn=23).spine(bend=8, turn=19)
        dip.wield(at=_sc(k, (0.21, -0.07, 0.92)), pitch=81, yaw=-6, roll=-30).hold("L", at=0.43, roll=-45)
        draw = _thrust_ready_ik(k).hips(up=-0.10 * k, fwd=-0.04 * k, turn=21, bend=4).spine(bend=4, turn=17.5, lean=-3)
        draw.wield(at=_sc(k, (0.21, -0.20, 0.94)), pitch=82, yaw=-5, roll=-30).hold("L", at=0.52, roll=-45)
        strike = _thrust_ready_ik(k).hips(up=-0.13 * k, fwd=0.12 * k, turn=3, bend=12).spine(bend=12, turn=2.5)
        strike.foot("R", heel=30, turn=35).head(nod=6)
        strike.wield(at=_sc(k, _THRUST_HIT), pitch=80, yaw=-10, roll=0).hold("L", at=0.28, roll=-45)
        reach = _thrust_ready_ik(k).hips(up=-0.135 * k, fwd=0.14 * k, turn=1, bend=13).spine(bend=13, turn=1)
        reach.foot("R", heel=32, turn=35).head(nod=8)
        reach.wield(at=_sc(k, _THRUST_REACH), pitch=81, yaw=-9, roll=2).hold("L", at=0.25, roll=-45)
        back = _thrust_ready_ik(k).hips(up=-0.10 * k, fwd=0.04 * k, turn=12).spine(bend=8, turn=10)
        back.wield(at=_sc(k, (0.21, 0.06, 0.92)), pitch=80, yaw=-6, roll=-15).hold("L", at=0.34, roll=-45)
        clip.key(0.0, ready(clip))
        clip.key(0.2 * hit, _fk(clip, dip))
        clip.key(0.55 * hit, _fk(clip, draw), skip=ARMS, interp="QUAD", easing="EASE_IN")
        clip.key(0.65 * hit, _fk(clip, draw), parts=ARMS, interp="QUAD", easing="EASE_IN")
        clip.key(hit, _fk(clip, strike), interp="QUAD", easing="EASE_OUT")
        clip.key(hit + 0.1, _fk(clip, reach))
        clip.key(hit + 0.3, _fk(clip, back))
        clip.key(end, ready(clip))

    doc = ("Attack: one two-handed thrust: a dip, the draw (the shaft pulled back through the front hand), "
           "the lunge (HIT) and full extension, then back to the guard.")
    sheet = (0.0, 0.2 * hit, 0.65 * hit, 0.85 * hit, hit, hit + 0.1, hit + 0.3, end)
    return {"Attack": _module(doc, end, False, (weapon, shield), attack, hit, sheet),
            "CombatIdle": _idle("polearm levelled at the enemy", ready, low, (weapon, shield))}


def _thrust_carry(weapon, shield):
    def carry(clip):
        k = _k(clip, THRUST_REF)
        return Pose().wield(at=_sc(k, (0.26, 0.20, 1.14)), pitch=25, yaw=-20, roll=-15)
    return carry


# -- swing: one-handed stroke (the lumen knight's, lib/stances.py) -------------

SWING_REF = 1.80
_SWING_HIT = _grip_for("lightblade", "edge", (0.0, 1.4, 1.15), 0, 70, 40)
_SWING_FOLLOW = _grip_for("lightblade", "edge", (-0.25, 1.05, 0.7), -25, 110, 50)


def _guard_arm(p, shield, lift=42, toward=8, elbow=95):
    """The left arm up as a guard: a shield on it faces the enemy."""
    return p.arm("L", lift=lift, toward=toward, twist=10).elbow("L", elbow).wrist("L", flex=0 if shield else 10)


def _swing_ready_ik(k, shield):
    p = _feet(Pose(), k).hips(up=-0.08 * k, turn=12, bend=6).spine(bend=8, turn=-8).look(_sc(k, (0.0, 1.6, 1.15)))
    _guard_arm(p, shield)
    return p.wield(at=_sc(k, (0.26, 0.26, 1.25)), pitch=15, yaw=25, roll=30)


def _swing(unit, weapon, shield, hit):
    def ready(clip):
        return _fk(clip, _swing_ready_ik(_k(clip, SWING_REF), shield))

    def low(clip):
        k = _k(clip, SWING_REF)
        p = _swing_ready_ik(k, shield).hips(up=-0.13 * k, fwd=0.03 * k, turn=8).spine(bend=14, turn=-5)
        p = _fk(clip, p.wield(at=_sc(k, (0.27, 0.27, 1.17)), pitch=20, yaw=25, roll=25))
        return p.arm("L", lift=33 if not shield else 40).elbow("L", 106 if not shield else 95)

    end = _frames(hit + 0.6)

    def attack(clip):
        k = _k(clip, SWING_REF)
        dip = _swing_ready_ik(k, shield).hips(up=-0.10 * k, turn=14).spine(bend=10)
        dip.wield(at=_sc(k, (0.27, 0.24, 1.18)), pitch=20, yaw=25, roll=25)
        wind = _swing_ready_ik(k, shield)
        wind.hips(up=-0.085 * k, fwd=-0.045 * k, right=0.02 * k, turn=24, bend=2).spine(bend=-6, turn=24, lean=-7)
        wind.wield(at=_sc(k, (0.32, -0.12, 1.85)), pitch=-45, yaw=10, roll=-15).shoulder("R", up=16).elbow("R", out=10)
        if not shield:
            wind.arm("L", lift=80, toward=-6, twist=0).elbow("L", 26).wrist("L", flex=0)
        strike = _swing_ready_ik(k, shield)
        strike.hips(up=-0.12 * k, fwd=0.075 * k, right=-0.01 * k, turn=-12, bend=12).spine(bend=22, turn=-24, lean=4)
        strike.foot("R", heel=30, turn=24).head(nod=8)
        strike.wield(at=_sc(k, _SWING_HIT), pitch=70, yaw=0, roll=40).shoulder("R", up=0, fwd=14).elbow("R", out=30)
        if not shield:
            strike.arm("L", lift=28, toward=150, twist=0).elbow("L", 70)
        follow = _swing_ready_ik(k, shield)
        follow.hips(up=-0.125 * k, fwd=0.08 * k, right=-0.01 * k, turn=-18, bend=14).spine(bend=26, turn=-32, lean=5)
        follow.foot("R", heel=34, turn=24).head(nod=10)
        follow.wield(at=_sc(k, _SWING_FOLLOW), pitch=110, yaw=-25, roll=50).shoulder("R", fwd=16).elbow("R", out=30)
        if not shield:
            follow.arm("L", lift=32, toward=165).elbow("L", 75)
        settle = _swing_ready_ik(k, shield).hips(up=-0.09 * k, fwd=0.02 * k, turn=4).spine(bend=12, turn=-12)
        settle.wield(at=_sc(k, (0.30, 0.32, 1.15)), pitch=30, yaw=10, roll=10).elbow("R", out=0)
        clip.key(0.0, ready(clip))
        clip.key(0.15 * hit, _fk(clip, dip))
        clip.key(0.45 * hit, _fk(clip, wind), skip=ARMS, interp="QUAD", easing="EASE_IN")
        clip.key(0.55 * hit, _fk(clip, wind), parts=ARMS, interp="QUAD", easing="EASE_IN")
        clip.key(hit, _fk(clip, strike), interp="QUAD", easing="EASE_OUT")
        clip.key(hit + 0.10, _fk(clip, follow), skip=ARMS)
        clip.key(hit + 0.12, _fk(clip, follow), parts=ARMS)
        clip.key(hit + 0.35, _fk(clip, settle))
        clip.key(end, ready(clip))

    doc = ("Attack: one diagonal stroke: a dip, the wind-up behind the head, the cut down across the enemy's "
           "chest (HIT), the follow-through to the left knee, back to the guard"
           + (", the shield held up throughout." if shield else "."))
    sheet = (0.0, 0.15 * hit, 0.55 * hit, 0.8 * hit, hit, hit + 0.12, hit + 0.35, end)
    return {"Attack": _module(doc, end, False, (weapon, shield), attack, hit, sheet),
            "CombatIdle": _idle("weapon raised" + (", shield up" if shield else ""), ready, low, (weapon, shield))}


def _swing_carry(weapon, shield):
    def carry(clip):
        k = _k(clip, SWING_REF)
        p = Pose().wield(at=_sc(k, (0.24, 0.30, 1.20)), pitch=25, yaw=-20, roll=-15)
        if shield:
            _shield_side(p)
        return p
    return carry


def _shield_side(p):
    """Walking and standing: the shield at the left side, facing forward."""
    return p.arm("L", lift=18, toward=35).elbow("L", 75)


# The rest were searched on bodies of their own (tide_varangian, archer,
# arquebusier, gatling_crew, shield_trooper, javelineer,
# starborn_lightbearer), every position for a 1.70 m figure.
REF = 1.70


def _upright_carry(k):
    """A weapon held upright before the right shoulder, sloped forward (the
    spearman's carry): heavy weapons, javelins, grenades, slings."""
    return Pose().wield(at=_sc(k, (0.26, 0.20, 1.14)), pitch=25, yaw=-20, roll=-15)


def _attack_doc(what, shield):
    return f"Attack: {what}" + (", the shield held up throughout." if shield else ".")


# -- heavy: two-handed overhead smash ------------------------------------------


def _heavy_ready_ik(k):
    p = _feet(Pose(), k).hips(up=-0.09 * k, turn=15, bend=6).spine(bend=6, turn=5).look(_sc(k, (0.0, 1.6, 1.15)))
    return p.wield(at=_sc(k, (0.2, 0.3, 1.1)), pitch=10, yaw=10, roll=30).hold("L", at=0.4, roll=-90)


def _heavy(unit, weapon, shield, hit):
    def ready(clip):
        return _fk(clip, _heavy_ready_ik(_k(clip, REF)))

    def low(clip):
        k = _k(clip, REF)
        p = _heavy_ready_ik(k).hips(up=-0.13 * k, fwd=0.03 * k, turn=13).spine(bend=12, turn=4)
        return _fk(clip, p.wield(at=_sc(k, (0.2, 0.31, 1.04)), pitch=18, yaw=10, roll=30))

    end = _frames(hit + 0.65)

    def attack(clip):
        k = _k(clip, REF)
        wind = _heavy_ready_ik(k).hips(up=-0.07 * k, fwd=-0.04 * k, turn=24, bend=0).spine(bend=-8, turn=22, lean=-6)
        wind.wield(at=_sc(k, (0.2, 0.0, 1.5)), pitch=-70, yaw=40, roll=30).hold("L", at=0.3, roll=-90)
        strike = _heavy_ready_ik(k).hips(up=-0.14 * k, fwd=0.08 * k, turn=-5, bend=18).spine(bend=25, turn=-10)
        strike.foot("R", heel=25, turn=28).head(nod=8)
        strike.wield(at=_sc(k, (0.05, 0.35, 0.95)), pitch=70, yaw=-10, roll=30).hold("L", at=0.4, roll=-90)
        follow = _heavy_ready_ik(k).hips(up=-0.16 * k, fwd=0.09 * k, turn=-8, bend=22).spine(bend=30, turn=-12)
        follow.foot("R", heel=28, turn=28).head(nod=10)
        follow.wield(at=_sc(k, (0.03, 0.33, 0.82)), pitch=88, yaw=-10, roll=30).hold("L", at=0.4, roll=-90)
        clip.key(0.0, ready(clip))
        clip.key(0.5 * hit, _fk(clip, wind), skip=ARMS, interp="QUAD", easing="EASE_IN")
        clip.key(0.6 * hit, _fk(clip, wind), parts=ARMS, interp="QUAD", easing="EASE_IN")
        clip.key(hit, _fk(clip, strike), interp="QUAD", easing="EASE_OUT")
        clip.key(hit + 0.12, _fk(clip, follow))
        clip.key(end, ready(clip))

    doc = _attack_doc("one two-handed overhead blow: the weapon heaved back over the right shoulder, "
                      "brought down in front at full speed (HIT) and driven on, then raised to the guard", None)
    sheet = (0.0, 0.3 * hit, 0.6 * hit, 0.85 * hit, hit, hit + 0.12, hit + 0.35, end)
    return {"Attack": _module(doc, end, False, (weapon,), attack, hit, sheet),
            "CombatIdle": _idle("two-handed weapon raised", ready, low, (weapon,))}


# -- bow: draw to the cheek and loose (the bow in the left hand) ---------------


def _bow_ready_ik(k):
    p = _feet(Pose(), k).hips(up=-0.07 * k, turn=18, bend=4).spine(bend=4, turn=8).look(_sc(k, (0.0, 8.0, 1.3)))
    p.wield(at=_sc(k, (-0.12, 0.25, 1.15)), pitch=40, yaw=10, roll=-30)
    return p.reach("R", at=_sc(k, (0.02, 0.07, 1.20)))


def _bow_aim_ik(k):
    p = _feet(Pose(), k).hips(up=-0.05 * k, turn=42, bend=2).spine(bend=-2, turn=28).look(_sc(k, (0.0, 8.0, 1.4)))
    p.wield(at=_sc(k, (-0.04, 0.66, 1.38)), pitch=0, yaw=-15, roll=-30)
    return p.reach("R", at=_sc(k, (0.0, 0.05, 1.48)))


def _bow(unit, weapon, shield, hit):
    def ready(clip):
        return _fk(clip, _bow_ready_ik(_k(clip, REF)), arms=("L", "R"))

    def low(clip):
        k = _k(clip, REF)
        p = _bow_ready_ik(k).hips(up=-0.10 * k, turn=17).spine(bend=7, turn=7)
        p.wield(at=_sc(k, (-0.12, 0.25, 1.11)), pitch=44, yaw=10, roll=-30).reach("R", at=_sc(k, (0.02, 0.07, 1.16)))
        return _fk(clip, p, arms=("L", "R"))

    end = _frames(hit + 0.55)

    def attack(clip):
        k = _k(clip, REF)
        aim = _fk(clip, _bow_aim_ik(k), arms=("L", "R"))
        loose = _bow_aim_ik(k).reach("R", at=_sc(k, (0.06, -0.06, 1.47)))   # the string hand flies back
        loose = _fk(clip, loose, arms=("L", "R"))
        clip.key(0.0, ready(clip))
        clip.key(0.45 * hit, aim, skip=ARMS, interp="SINE", easing="EASE_OUT")
        clip.key(0.6 * hit, aim, parts=ARMS, interp="SINE", easing="EASE_OUT")
        clip.key(hit, aim, interp="CUBIC", easing="EASE_OUT")
        clip.key(hit + 0.1, loose)
        clip.key(hit + 0.25, aim)
        clip.key(end, ready(clip))

    doc = ("Attack: raise the bow at the target side-on and draw the string to the cheek, hold, loose (HIT: "
           "the string hand flies back), lower the bow again.")
    sheet = (0.0, 0.3 * hit, 0.6 * hit, hit, hit + 0.1, hit + 0.25, end)
    return {"Attack": _module(doc, end, False, (weapon,), attack, hit, sheet, beats=(0.8 * hit,)),
            "CombatIdle": _idle("bow lowered, an arrow on the string", ready, low, (weapon,))}


def _bow_carry(weapon, shield):
    def carry(clip):
        return Pose().wield(at=_sc(_k(clip, REF), (-0.24, 0.24, 1.1)), pitch=30, yaw=0, roll=0)
    return carry


# -- gun: shouldered and fired (the crossbowman's way, any stock length) -------


def _stock(weapon):
    """How much further forward the trigger hand sits than on the crossbow
    the stances were first made for: half the extra stock length."""
    butt = props.prop_info(weapon).part("butt")
    return 0.5 * max(0.0, -butt.y - 0.27)


def _gun_aim_ik(k, d):
    p = _feet(Pose(), k).hips(up=-0.05 * k, turn=20, bend=2).spine(bend=-2, turn=12).look(_sc(k, (0.0, 8.0, 1.4)))
    return p.wield(at=(0.12 * k, 0.12 * k + d, 1.42 * k), pitch=90, yaw=-6, roll=170).hold("L", at=0.22, roll=60)


def _gun_ready_ik(k, d):
    p = _feet(Pose(), k).hips(up=-0.07 * k, turn=18, bend=4).spine(bend=4, turn=8).look(_sc(k, (0.0, 8.0, 1.3)))
    return p.wield(at=(0.12 * k, 0.18 * k + d, 1.2 * k), pitch=120, yaw=-15, roll=180).hold("L", at=0.2, roll=60)


def _gun(unit, weapon, shield, hit):
    def ready(clip):
        return _fk(clip, _gun_ready_ik(_k(clip, REF), _stock(weapon)))

    def low(clip):
        k, d = _k(clip, REF), _stock(weapon)
        p = _gun_ready_ik(k, d).hips(up=-0.10 * k, fwd=0.02 * k, turn=17).spine(bend=7, turn=7)
        return _fk(clip, p.wield(at=(0.12 * k, 0.19 * k + d, 1.17 * k), pitch=123, yaw=-15, roll=180))

    end = _frames(hit + 0.6)

    def attack(clip):
        k, d = _k(clip, REF), _stock(weapon)
        aim = _fk(clip, _gun_aim_ik(k, d))
        kick = _gun_aim_ik(k, d).hips(up=-0.05 * k, fwd=-0.03 * k, turn=20, bend=0).spine(bend=-5, turn=14, lean=-2)
        kick.wield(at=(0.12 * k, 0.08 * k + d, 1.45 * k), pitch=84, yaw=-6, roll=170).shoulder("R", up=4).head(nod=-3)
        # Halfway up and halfway down placed by IK: straight between the two
        # ends, the arm angles would swing the muzzle up past the head.
        between = _fk(clip, _gun_ready_ik(k, d).lerp(_gun_aim_ik(k, d), 0.5))
        clip.key(0.0, ready(clip))
        clip.key(0.3 * hit, between, parts=ARMS)
        clip.key(0.5 * hit, aim, skip=ARMS, interp="SINE", easing="EASE_OUT")
        clip.key(0.6 * hit, aim, parts=ARMS, interp="SINE", easing="EASE_OUT")
        clip.key(hit, aim, interp="CUBIC", easing="EASE_OUT")
        clip.key(hit + 0.07, _fk(clip, kick))
        clip.key(hit + 0.25, aim)
        clip.key(0.5 * (hit + 0.25 + end), between, parts=ARMS)
        clip.key(end, ready(clip))

    doc = ("Attack: shoulder the gun and level it at the target, hold the aim, fire (HIT) with a kick "
           "(the muzzle up, the shoulders back), lower it again.")
    sheet = (0.0, 0.3 * hit, 0.6 * hit, hit, hit + 0.07, hit + 0.25, end)
    return {"Attack": _module(doc, end, False, (weapon,), attack, hit, sheet, beats=(0.8 * hit,)),
            "CombatIdle": _idle("gun lowered at the chest", ready, low, (weapon,))}


def _gun_carry(weapon, shield):
    def carry(clip):
        k = _k(clip, REF)
        return Pose().wield(at=_sc(k, (0.06, 0.24, 1.12)), pitch=135, yaw=-35, roll=180).hold("L", at=0.12, roll=90)
    return carry


# -- hip: fired from the hip with both hands -----------------------------------


def _hip_ik(k):
    p = _feet(Pose(), k).hips(up=-0.08 * k, turn=10, bend=4).spine(bend=4, turn=5).look(_sc(k, (0.0, 8.0, 1.3)))
    return p.wield(at=_sc(k, (0.12, 0.15, 1.1)), pitch=100, yaw=0, roll=90).hold("L", at=0.25, roll=180)


def _hip(unit, weapon, shield, hit):
    def ready(clip):
        return _fk(clip, _hip_ik(_k(clip, REF)))

    def low(clip):
        k = _k(clip, REF)
        p = _hip_ik(k).hips(up=-0.11 * k, fwd=0.02 * k, turn=9).spine(bend=7, turn=4)
        return _fk(clip, p.wield(at=_sc(k, (0.12, 0.16, 1.07)), pitch=102, yaw=0, roll=90))

    end = _frames(hit + 0.45)

    def attack(clip):
        k = _k(clip, REF)
        brace = _hip_ik(k).hips(up=-0.11 * k, fwd=0.03 * k, turn=8, bend=8).spine(bend=8, turn=3)
        kick = _hip_ik(k).hips(up=-0.09 * k, fwd=-0.03 * k, turn=11, bend=2).spine(bend=0, turn=6, lean=-2)
        kick.wield(at=_sc(k, (0.12, 0.11, 1.12)), pitch=95, yaw=0, roll=90)
        clip.key(0.0, ready(clip))
        clip.key(0.6 * hit, _fk(clip, brace), interp="SINE", easing="EASE_OUT")
        clip.key(hit, _fk(clip, brace), interp="CUBIC", easing="EASE_OUT")
        clip.key(hit + 0.06, _fk(clip, kick))
        clip.key(hit + 0.2, _fk(clip, brace))
        clip.key(end, ready(clip))

    doc = "Attack: brace, fire from the hip (HIT) with a kick, settle."
    sheet = (0.0, 0.6 * hit, hit, hit + 0.06, hit + 0.2, end)
    return {"Attack": _module(doc, end, False, (weapon,), attack, hit, sheet),
            "CombatIdle": _idle("weapon at the hip", ready, low, (weapon,))}


def _hip_carry(weapon, shield):
    def carry(clip):
        k = _k(clip, REF)
        return Pose().wield(at=_sc(k, (0.12, 0.15, 1.1)), pitch=100, yaw=0, roll=90).hold("L", at=0.25, roll=180)
    return carry


# -- gun1h: fired one-handed, a shield on the left arm --------------------------


def _gun1h_ik(k, pitch=95, at=(0.18, 0.4, 1.4)):
    p = _feet(Pose(), k).hips(up=-0.06 * k, turn=15, bend=2).spine(bend=0, turn=5).look(_sc(k, (0.0, 8.0, 1.4)))
    _guard_arm(p, True, lift=22, toward=15, elbow=95)   # a tall shield: knees to eyes
    return p.wield(at=_sc(k, at), pitch=pitch, yaw=10, roll=150)


def _gun1h(unit, weapon, shield, hit):
    def ready(clip):
        return _fk(clip, _gun1h_ik(_k(clip, REF), 115, (0.2, 0.32, 1.25)))

    def low(clip):
        k = _k(clip, REF)
        return _fk(clip, _gun1h_ik(k, 118, (0.2, 0.32, 1.21)).hips(up=-0.09 * k).spine(bend=4))

    end = _frames(max(hit + 0.4, 0.5))

    def attack(clip):
        k = _k(clip, REF)
        aim = _fk(clip, _gun1h_ik(k))
        kick = _fk(clip, _gun1h_ik(k, 88, (0.18, 0.36, 1.43)).hips(fwd=-0.02 * k))
        clip.key(0.0, ready(clip))
        clip.key(0.7 * hit, aim, interp="SINE", easing="EASE_OUT")
        clip.key(hit, aim, interp="CUBIC", easing="EASE_OUT")
        clip.key(hit + 0.06, kick)
        clip.key(hit + 0.18, aim)
        clip.key(end, ready(clip))

    doc = _attack_doc("raise the gun one-handed over the shield's rim, fire (HIT) with a kick, lower it", shield)
    sheet = (0.0, 0.7 * hit, hit, hit + 0.06, hit + 0.18, end)
    return {"Attack": _module(doc, end, False, (weapon, shield), attack, hit, sheet),
            "CombatIdle": _idle("gun ready behind the shield", ready, low, (weapon, shield))}


def _gun1h_carry(weapon, shield):
    def carry(clip):
        k = _k(clip, REF)
        p = Pose().wield(at=_sc(k, (0.24, 0.30, 1.20)), pitch=25, yaw=-20, roll=-15)
        return _shield_side(p) if shield else p
    return carry


# -- throw: overhand (javelin, grenade; the sling its own way) -----------------

# Where the throwing hand is and how the weapon lies in it, (right, fwd, up)
# and (pitch, yaw, roll), at each beat. A javelin or grenade is held
# level over the shoulder and thrown along its length; a sling hangs from
# the fist, is swung up behind the head and whips forward over it.
_HANDS = {
    "throw": dict(ready=((0.3, 0.05, 1.55), (90, -10, -180)), low=((0.3, 0.04, 1.5), (92, -10, -180)),
                  wind=((0.3, -0.2, 1.5), (90, 30, -180)), release=((0.15, 0.45, 1.4), (100, -10, -225)),
                  follow=((0.05, 0.4, 1.15), (115, -15, -225))),
    "sling": dict(ready=((0.3, 0.0, 1.05), (160, -20, -135)), low=((0.3, 0.0, 1.01), (162, -20, -135)),
                  wind=((0.35, -0.3, 1.45), (-60, -30, -45)), release=((0.1, 0.45, 1.5), (30, 20, 90)),
                  follow=((0.1, 0.5, 1.2), (110, -20, 135))),
}


def _hand(p, k, hands, beat):
    pos, (pitch, yaw, roll) = hands[beat]
    return p.wield(at=_sc(k, pos), pitch=pitch, yaw=yaw, roll=roll)


def _throw_ready_ik(k, shield, hands):
    p = _feet(Pose(), k).hips(up=-0.07 * k, turn=15, bend=4).spine(bend=2, turn=10).look(_sc(k, (0.0, 6.0, 1.3)))
    if shield:
        _guard_arm(p, True)
    else:
        p.arm("L", lift=55, toward=5).elbow("L", 30)   # the free hand points at the target
    return _hand(p, k, hands, "ready")


def _throw(unit, weapon, shield, hit, how="throw"):
    hands = _HANDS[how]

    def ready(clip):
        return _fk(clip, _throw_ready_ik(_k(clip, REF), shield, hands))

    def low(clip):
        k = _k(clip, REF)
        p = _throw_ready_ik(k, shield, hands).hips(up=-0.10 * k, turn=14).spine(bend=5, turn=9)
        return _fk(clip, _hand(p, k, hands, "low"))

    end = _frames(hit + 0.5)

    def attack(clip):
        k = _k(clip, REF)
        wind = _throw_ready_ik(k, shield, hands).hips(up=-0.07 * k, fwd=-0.04 * k, turn=35, bend=0)
        _hand(wind.spine(bend=-8, turn=30, lean=-6), k, hands, "wind")
        release = _throw_ready_ik(k, shield, hands).hips(up=-0.1 * k, fwd=0.08 * k, turn=-10, bend=12)
        _hand(release.spine(bend=15, turn=-20, lean=4).foot("R", heel=25, turn=28), k, hands, "release")
        follow = _throw_ready_ik(k, shield, hands).hips(up=-0.12 * k, fwd=0.09 * k, turn=-16, bend=16)
        _hand(follow.spine(bend=22, turn=-26, lean=5).foot("R", heel=30, turn=28), k, hands, "follow")
        clip.key(0.0, ready(clip))
        clip.key(0.5 * hit, _fk(clip, wind), skip=ARMS, interp="QUAD", easing="EASE_IN")
        clip.key(0.6 * hit, _fk(clip, wind), parts=ARMS, interp="QUAD", easing="EASE_IN")
        clip.key(hit, _fk(clip, release), interp="QUAD", easing="EASE_OUT")
        clip.key(hit + 0.12, _fk(clip, follow))
        clip.key(end, ready(clip))

    if how == "sling":
        what = "one sling cast: the sling swung up behind the head, whipped forward over it (HIT: the release) and down"
        idle = "the sling hanging from the hand"
    else:
        what = ("one overhand throw: the arm cocked back with the body turned away, whipped forward past the head "
                "(HIT: the release) and through")
        idle = "weapon cocked by the shoulder"
    sheet = (0.0, 0.3 * hit, 0.6 * hit, 0.85 * hit, hit, hit + 0.12, end)
    return {"Attack": _module(_attack_doc(what, shield), end, False, (weapon, shield), attack, hit, sheet),
            "CombatIdle": _idle(idle + (", shield up" if shield else ""), ready, low, (weapon, shield))}


def _throw_carry(weapon, shield):
    def carry(clip):
        p = _upright_carry(_k(clip, REF))
        return _shield_side(p) if shield else p
    return carry


def _sling_carry(weapon, shield):
    def carry(clip):
        return _hand(Pose(), _k(clip, REF), _HANDS["sling"], "ready")
    return carry


# -- cast: a staff, lens or rune weapon raised and pointed (or bare hands) -----

_PALMS = dict(flex=-45)   # wrist bent back: the palm faces the target


def _cast_ready_ik(k, weapon):
    p = _feet(Pose(), k).hips(up=-0.07 * k, turn=10, bend=4).spine(bend=4).look(_sc(k, (0.0, 6.0, 1.3)))
    if weapon:
        p.arm("L", lift=14, toward=40).elbow("L", 35)
        return p.wield(at=_sc(k, (0.22, 0.25, 1.15)), pitch=10, yaw=-20, roll=-15)
    return p.reach("R", at=_sc(k, (0.15, 0.3, 1.2))).reach("L", at=_sc(k, (-0.15, 0.3, 1.2)))


def _cast(unit, weapon, shield, hit):
    arms = ("R",) if weapon else ("R", "L")

    def ready(clip):
        return _fk(clip, _cast_ready_ik(_k(clip, REF), weapon), arms=arms)

    def low(clip):
        k = _k(clip, REF)
        p = _cast_ready_ik(k, weapon).hips(up=-0.10 * k).spine(bend=7)
        if weapon:
            p.wield(at=_sc(k, (0.22, 0.25, 1.11)), pitch=12, yaw=-20, roll=-15)
        return _fk(clip, p, arms=arms)

    end = _frames(hit + 0.6)

    def attack(clip):
        k = _k(clip, REF)
        rise = _cast_ready_ik(k, weapon).hips(up=-0.05 * k, fwd=-0.02 * k, bend=0).spine(bend=-4)
        point = _cast_ready_ik(k, weapon).hips(up=-0.08 * k, fwd=0.05 * k, bend=8).spine(bend=4, turn=-5)
        if weapon:
            rise.wield(at=_sc(k, (0.15, 0.25, 1.5)), pitch=0, yaw=-20, roll=15)
            point.wield(at=_sc(k, (0.15, 0.35, 1.5)), pitch=15, yaw=-20, roll=15)
            point.arm("L", lift=80, toward=15).elbow("L", 10).wrist("L", **_PALMS)
        else:
            rise.reach("R", at=_sc(k, (0.2, 0.25, 1.6))).reach("L", at=_sc(k, (-0.2, 0.25, 1.6)))
            point.reach("R", at=_sc(k, (0.18, 0.55, 1.4))).reach("L", at=_sc(k, (-0.18, 0.55, 1.4)))
            point.wrist("R", **_PALMS).wrist("L", **_PALMS)
        clip.key(0.0, ready(clip))
        clip.key(0.6 * hit, _fk(clip, rise, arms=arms), interp="SINE", easing="EASE_IN")
        clip.key(hit, _fk(clip, point, arms=arms), interp="CUBIC", easing="EASE_OUT")
        clip.key(hit + 0.25, _fk(clip, point, arms=arms))
        clip.key(end, ready(clip))

    what = ("raise the " + ("weapon high" if weapon else "hands") + ", then point it at the target (HIT: "
            "the spell goes) and hold a moment, back to the guard")
    sheet = (0.0, 0.3 * hit, 0.6 * hit, hit, hit + 0.25, end)
    return {"Attack": _module(_attack_doc(what, None), end, False, (weapon,), attack, hit, sheet,
                              beats=(hit + 0.12,)),
            "CombatIdle": _idle("ready to cast", ready, low, (weapon,))}


def _cast_carry(weapon, shield):
    def carry(clip):
        return Pose().wield(at=_sc(_k(clip, REF), (0.22, 0.25, 1.15)), pitch=10, yaw=-20, roll=-15)
    return carry


def heal(weapon=None):
    """{"Heal": module}: mending an ally, a loop: hands raised in blessing
    toward it (with a staff, the free hand), swaying gently."""
    arms = ("L",) if weapon else ("R", "L")

    def pose(k, up):
        p = _feet(Pose(), k, 0.08, -0.06, 0.07, 15).hips(up=-0.05 * k).spine(bend=6).look(_sc(k, (0.0, 2.0, 0.9)))
        p.reach("L", at=_sc(k, (-0.18, 0.35, up))).wrist("L", **_PALMS)
        if weapon:
            p.wield(at=_sc(k, (0.22, 0.25, 1.15)), pitch=10, yaw=-20, roll=-15)
        else:
            p.reach("R", at=_sc(k, (0.18, 0.35, up))).wrist("R", **_PALMS)
        return p

    def build(clip):
        k = _k(clip, REF)
        a = _fk(clip, pose(k, 1.32), arms=arms + (("R",) if weapon else ()))
        b = pose(k, 1.40).hips(right=0.02 * k, up=-0.04 * k).spine(bend=4, lean=-2)
        b = _fk(clip, b, arms=arms + (("R",) if weapon else ()))
        clip.key(0.0, a)
        clip.key(1.0, b)

    return {"Heal": _module("Heal: hands raised in blessing toward the ally, swaying gently (a loop).", 2.0, True,
                            (weapon,), build)}


# -- the families ---------------------------------------------------------------

FAMILIES = {
    "thrust": (_thrust, _thrust_carry),
    "swing": (_swing, _swing_carry),
    "heavy": (_heavy, lambda weapon, shield: lambda clip: _upright_carry(_k(clip, REF))),
    "bow": (_bow, _bow_carry),
    "gun": (_gun, _gun_carry),
    "hip": (_hip, _hip_carry),
    "gun1h": (_gun1h, _gun1h_carry),
    "throw": (_throw, _throw_carry),
    "sling": (lambda unit, weapon, shield, hit: _throw(unit, weapon, shield, hit, "sling"), _sling_carry),
    "cast": (_cast, _cast_carry),
}


def clips(unit, family, weapon=None, shield=None):
    """{"Attack": module, "CombatIdle": module} for `unit` (its weapon's
    windup times the hit) fighting as `family` with these prop nodes."""
    if family not in FAMILIES:
        raise KeyError(f"no fight family {family!r} (have {sorted(FAMILIES)})")
    hit = _frames(gamedata.windup(unit))
    return FAMILIES[family][0](unit, weapon, shield, hit)


def carry(family, weapon=None, shield=None):
    """The CARRY pose function for a unit of `family` holding these props."""
    return FAMILIES[family][1](weapon, shield)
