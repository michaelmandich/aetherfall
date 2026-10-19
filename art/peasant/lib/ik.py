"""Analytic two-bone IK (shoulder-elbow-wrist, hip-knee-ankle) in world metres."""

import math

from mathutils import Matrix, Vector


def any_perpendicular(d):
    helper = Vector((0.0, 0.0, 1.0)) if abs(d.z) < 0.9 else Vector((0.0, 1.0, 0.0))
    return d.cross(helper).normalized()


def swivel(pole, axis, toward, degrees):
    """Rotate `pole` about `axis` by `degrees`, positive turning it towards
    `toward` (both taken perpendicular to `axis`)."""
    if abs(degrees) < 1e-9:
        return pole
    a = axis.normalized()
    p = pole - a * pole.dot(a)
    t = toward - a * toward.dot(a)
    sign = 1.0
    if p.length > 1e-6 and t.length > 1e-6:
        c = p.cross(t).dot(a)
        sign = 1.0 if c >= 0.0 else -1.0
    return Matrix.Rotation(math.radians(degrees) * sign, 3, a) @ pole


def two_bone(root, target, len_a, len_b, pole):
    """Solve a two-bone chain.

    Returns (mid joint position, end position actually reached, unit
    direction root->target, unit pole direction perpendicular to it,
    reach error in metres). The mid joint bends towards `pole`.
    """
    d = target - root
    dist = d.length
    if dist < 1e-6:
        d = Vector((0.0, 0.0, -1.0))
        dist = 1e-6
    dn = d.normalized()
    max_r = (len_a + len_b) * 0.9995
    min_r = abs(len_a - len_b) + 0.02
    err = 0.0
    if dist > max_r:
        err = dist - max_r
        dist = max_r
    elif dist < min_r:
        err = min_r - dist
        dist = min_r
    cos_a = (len_a * len_a + dist * dist - len_b * len_b) / (2.0 * len_a * dist)
    cos_a = max(-1.0, min(1.0, cos_a))
    sin_a = math.sqrt(max(0.0, 1.0 - cos_a * cos_a))
    n = pole - dn * pole.dot(dn)
    n = n.normalized() if n.length > 1e-6 else any_perpendicular(dn)
    mid = root + (dn * cos_a + n * sin_a) * len_a
    end = root + dn * dist
    return mid, end, dn, n, err
