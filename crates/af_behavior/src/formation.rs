//! Group move formations: rows of boxes centred on the target point,
//! facing from the group's centre towards it.

use bevy::math::ops;
use bevy::prelude::*;

use crate::tuning::FORMATION_GAP;

/// A unit taking part in a group move.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Member {
    pub entity: Entity,
    /// Where it starts from (its position, or the end of its queued path).
    pub from: Vec2,
    /// Footprint half extents (x across, y along the direction of travel).
    pub half: Vec2,
}

/// One destination per member. Units keep their place in the group: the
/// ones in front take the front row and left stays left, so paths do not
/// cross. Rows are about twice as wide as the block is deep, and each row
/// is packed by the members' own widths.
pub(crate) fn arrange(members: &[Member], target: Vec2) -> Vec<(Entity, Vec2)> {
    match members {
        [] => return Vec::new(),
        [only] => return vec![(only.entity, target)],
        _ => {}
    }
    let centre = members.iter().map(|m| m.from).sum::<Vec2>() / members.len() as f32;
    let (ahead, across) = axes(target - centre);

    let mut order: Vec<&Member> = members.iter().collect();
    order.sort_by(|a, b| b.from.dot(ahead).total_cmp(&a.from.dot(ahead)).then(a.entity.cmp(&b.entity)));
    let cols = (ops::sqrt(2.0 * members.len() as f32).ceil() as usize).clamp(1, members.len());
    let mut rows: Vec<Vec<&Member>> = order.chunks(cols).map(|r| r.to_vec()).collect();
    for row in &mut rows {
        row.sort_by(|a, b| a.from.dot(across).total_cmp(&b.from.dot(across)).then(a.entity.cmp(&b.entity)));
    }

    let depth = |row: &[&Member]| row.iter().map(|m| m.half.y * 2.0).fold(0.0, f32::max);
    let total_depth = rows.iter().map(|r| depth(r)).sum::<f32>() + FORMATION_GAP * (rows.len() - 1) as f32;
    let mut out = Vec::with_capacity(members.len());
    // Distance from the block's front edge to the current row's front edge.
    let mut behind = 0.0;
    for row in &rows {
        let row_depth = depth(row);
        let along = total_depth * 0.5 - behind - row_depth * 0.5;
        let width = row.iter().map(|m| m.half.x * 2.0).sum::<f32>() + FORMATION_GAP * (row.len() - 1) as f32;
        let mut left = -width * 0.5;
        for m in row {
            let side = left + m.half.x;
            out.push((m.entity, target + across * side + ahead * along));
            left += m.half.x * 2.0 + FORMATION_GAP;
        }
        behind += row_depth + FORMATION_GAP;
    }
    out
}

/// Unit vectors along and to the right of `direction` (+Z when it is zero),
/// with af_geom's conventions. Plain vector maths rather than a round trip
/// through a yaw: no trigonometry, so lockstep peers agree bit for bit.
fn axes(direction: Vec2) -> (Vec2, Vec2) {
    let ahead = direction.try_normalize().unwrap_or(Vec2::Y);
    (ahead, Vec2::new(ahead.y, -ahead.x))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn members(n: u32, half: Vec2) -> Vec<Member> {
        (0..n)
            .map(|i| Member { entity: Entity::from_raw_u32(i + 1).unwrap(), from: Vec2::new(i as f32 * 0.3, 0.0), half })
            .collect()
    }

    #[test]
    fn axes_follow_af_geom_conventions() {
        use af_core::prelude::af_geom::{forward, right, yaw_of};
        for dir in [Vec2::X, Vec2::Y, Vec2::new(-3.0, 1.0), Vec2::new(0.5, -2.0)] {
            let (ahead, across) = axes(dir);
            let yaw = yaw_of(dir);
            assert!(ahead.distance(forward(yaw)) < 1e-5 && across.distance(right(yaw)) < 1e-5, "{dir}");
        }
        assert_eq!(axes(Vec2::ZERO), (Vec2::Y, Vec2::X));
    }

    #[test]
    fn single_unit_goes_to_the_point() {
        let m = members(1, Vec2::splat(0.3));
        assert_eq!(arrange(&m, Vec2::new(10.0, 10.0)), vec![(m[0].entity, Vec2::new(10.0, 10.0))]);
    }

    #[test]
    fn slots_do_not_overlap_and_centre_on_the_target() {
        let half = Vec2::new(0.35, 0.35);
        let m = members(12, half);
        let target = Vec2::new(40.0, 30.0);
        let slots = arrange(&m, target);
        assert_eq!(slots.len(), 12);
        for (i, (_, a)) in slots.iter().enumerate() {
            for (_, b) in &slots[i + 1..] {
                assert!(a.distance(*b) >= half.x * 2.0 + FORMATION_GAP - 1e-3, "{a} {b}");
            }
        }
        let centre = slots.iter().map(|(_, p)| *p).sum::<Vec2>() / slots.len() as f32;
        assert!(centre.distance(target) < 1.0, "{centre}");
    }

    #[test]
    fn rows_are_wider_than_deep_and_face_the_target() {
        let m = members(8, Vec2::new(0.3, 0.3));
        // Group sits at the origin; target straight ahead along +Z.
        let slots = arrange(&m, Vec2::new(0.0, 50.0));
        let xs: Vec<f32> = slots.iter().map(|(_, p)| p.x).collect();
        let zs: Vec<f32> = slots.iter().map(|(_, p)| p.y).collect();
        let spread = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread(&xs) > spread(&zs), "x spread {} z spread {}", spread(&xs), spread(&zs));
    }
}
