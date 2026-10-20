"""Fast numeric self-test of the pose helper, run by every build.

Checks the calibration conventions (directions of the readable angles),
IK reach, two-hand holds and the IK -> FK conversion, so a regression in
rig.py / pose.py / props.py fails the build instead of a contact sheet.
"""

import math

from mathutils import Vector

from .pose import Pose, Solver, fk_numbers
from .props import fist_frame, prop_info
from .rig import ARM, DOWN, FWD, HAND, LEG, UP, at, out_dir


class SelfTestError(AssertionError):
    pass


def _angle(a, b):
    return math.degrees(a.angle(b))


def _dir(fk, a, b):
    return (fk[b][0] - fk[a][0]).normalized()


def run(rig):
    results = []

    def check(name, ok, detail):
        results.append((name, ok, detail))

    solver = Solver(rig)
    # Readable angles point where they say (upper arm direction, both sides).
    for side in ("L", "R"):
        up, fore, hand = ARM[side][1:]
        for lift, toward, want in ((90, 0, FWD), (90, 90, out_dir(side)), (180, 0, UP), (0, 0, DOWN)):
            solver.solve(Pose().arm(side, lift=lift, toward=toward).elbow(side, 0).v)
            fk = {k: (solver.P[k], solver.W[k]) for k in solver.P}
            err = _angle(_dir(fk, up, fore), want)
            check(f"arm {side} lift {lift} toward {toward}", err < 1.0, f"{err:.2f} deg")
        solver.solve(Pose().arm(side, lift=0).elbow(side, 90).v)
        fk = {k: (solver.P[k], solver.W[k]) for k in solver.P}
        err = _angle(_dir(fk, fore, hand), FWD)
        check(f"elbow {side} 90 points forearm forward", err < 1.0, f"{err:.2f} deg")
        thigh, shin, foot, _ = LEG[side]
        solver.solve(Pose().leg(side, lift=90, knee=90).v)
        fk = {k: (solver.P[k], solver.W[k]) for k in solver.P}
        e1, e2 = _angle(_dir(fk, thigh, shin), FWD), _angle(_dir(fk, shin, foot), DOWN)
        check(f"leg {side} lift 90 knee 90", max(e1, e2) < 1.0, f"{e1:.2f}/{e2:.2f} deg")
    # Spine/head turn right and bend forward.
    solver.solve(Pose().spine(turn=40).v)
    f = solver.W["mixamorig:Spine2"] @ rig.canon["mixamorig:Spine2"].transposed() @ FWD
    check("spine turn + faces right", f.x < -0.5, f"forward -> {tuple(round(x, 2) for x in f)}")
    solver.solve(Pose().spine(bend=40).v)
    u = solver.W["mixamorig:Spine2"] @ rig.canon["mixamorig:Spine2"].transposed() @ UP
    check("spine bend + tips forward", u.dot(FWD) > 0.5, f"up -> {tuple(round(x, 2) for x in u)}")

    # IK: wrist reaches a point (well within this body's reach: in front of
    # and below its right shoulder); feet stay where placed.
    shoulder = rig.rest_pos[ARM["R"][1]]
    goal = (-shoulder.x + 0.03, -shoulder.y + 0.30, shoulder.z - 0.25)
    target = at(*goal)
    solver.solve(Pose().reach("R", at=goal).foot("L", fwd=0.2).hips(up=-0.08).v)
    err = (solver.P[HAND["R"]] - target).length
    check("reach R wrist on target", err < 1e-3, f"{err * 1000:.2f} mm")
    rest_ankle = rig.rest_pos[LEG["L"][2]]
    want = rest_ankle + at(0.0, 0.2, 0.0)
    err = (solver.P[LEG["L"][2]] - want).length
    check("foot L IK placement", err < 1e-3, f"{err * 1000:.2f} mm")

    # Two hands on one haft: the left fist sits on the haft line `at` up it.
    chop = prop_info("felling_axe")
    s2 = Solver(rig, {"R": chop})
    pose = Pose().wield(at=(0.15, 0.35, 1.2), pitch=40, yaw=-20).hold("L", at=0.3)
    s2.solve(pose.v)
    tool = s2.hand_frame("R") @ chop.in_hand
    left_fist = (s2.hand_frame("L") @ fist_frame("L")).translation
    on_haft = tool @ Vector((0.0, 0.3, 0.0))
    err = (left_fist - on_haft).length
    check("hold L on haft at 0.3", err < 1e-3 or s2.info.get("arm.L", 0) > 0, f"{err * 1000:.2f} mm")

    # IK -> FK conversion reproduces the arm.
    before = {k: s2.W[k].copy() for k in ARM["R"][1:]}
    fk_pose = Pose(pose.v)
    fk_pose.v.update(fk_numbers(s2, "R", "arm"))
    fk_pose.set("wield.w", 0.0)
    s2.solve(fk_pose.v)
    worst = max(math.degrees(before[k].to_quaternion().rotation_difference(s2.W[k].to_quaternion()).angle)
                for k in before)
    worst = min(worst, 360 - worst)
    check("IK -> FK arm round trip", worst < 0.1, f"{worst:.3f} deg")
    s3 = Solver(rig)
    lp = Pose().foot("R", fwd=0.25, heel=20).hips(up=-0.12, fwd=0.05)
    s3.solve(lp.v)
    before = {k: s3.W[k].copy() for k in LEG["R"][:3]}
    lp.v.update(fk_numbers(s3, "R", "leg"))
    s3.solve(lp.v)
    worst = max(math.degrees(before[k].to_quaternion().rotation_difference(s3.W[k].to_quaternion()).angle)
                for k in before)
    worst = min(worst, 360 - worst)
    check("IK -> FK leg round trip", worst < 0.1, f"{worst:.3f} deg")

    failed = [r for r in results if not r[1]]
    for name, ok, detail in results:
        if not ok:
            print(f"  SELFTEST FAIL {name}: {detail}")
    print(f"selftest: {len(results) - len(failed)}/{len(results)} passed")
    if failed:
        raise SelfTestError(f"{len(failed)} pose helper self-test(s) failed")
    return results

