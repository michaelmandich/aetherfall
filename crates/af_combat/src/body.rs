//! The battlefield as weapons see it: snapshots of damageable entities and
//! proximity queries over them.

use af_core::ext::combat::{edge_distance, TargetClass};
use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// How far a unit may have moved since the spatial index was rebuilt at the
/// start of the tick. Proximity searches widen by this so that the precise
/// test against the current box never misses a mover.
const MOVE_MARGIN: f32 = 0.5;

/// Components read to describe something that can be hit.
pub(crate) type BodyData = (
    Entity,
    &'static Position,
    &'static Facing,
    &'static Footprint,
    &'static Altitude,
    Option<&'static Owner>,
    Option<&'static Stats>,
    Option<&'static Velocity>,
    Option<&'static Mobile>,
    Option<&'static Unit>,
    Option<&'static Building>,
    Has<UnderConstruction>,
);

/// Damageable, alive units and buildings (nodes have no Health).
pub(crate) type BodyFilter = (With<Health>, Without<Dead>, Or<(With<Unit>, With<Building>)>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyKind {
    Unit(UnitTypeId),
    Building(BuildingTypeId),
}

/// Splash and pierce stay on one layer: a flak burst at altitude does not
/// reach the ground under it, and a shell landing does not reach flyers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layer {
    Ground,
    Air,
}

/// A snapshot of a damageable unit or building, as of now in this tick.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Body {
    pub entity: Entity,
    pub kind: BodyKind,
    pub pos: Vec2,
    pub obb: Obb,
    pub altitude: f32,
    pub height: f32,
    pub velocity: Vec2,
    pub flying: bool,
    pub under_construction: bool,
    pub owner: Option<PlayerId>,
    /// Owner's team (`af_geom::NO_TEAM` without an owner).
    pub team: u8,
    pub tags: TagSet,
}

impl Body {
    pub fn is_building(&self) -> bool {
        matches!(self.kind, BodyKind::Building(_))
    }

    pub fn class(&self) -> TargetClass {
        TargetClass::of(self.is_building(), self.flying)
    }

    pub fn layer(&self) -> Layer {
        if self.flying { Layer::Air } else { Layer::Ground }
    }

    /// Centre of the box in 3D: where weapons aim.
    pub fn aim_point(&self) -> Vec3 {
        lift(self.pos, self.altitude + self.height * 0.5)
    }

    /// Does the 3D point lie inside the box grown by `margin`?
    pub fn contains(&self, p: Vec3, margin: f32) -> bool {
        p.y >= self.altitude - margin
            && p.y <= self.altitude + self.height + margin
            && self.obb.distance_to_point(ground(p)) <= margin
    }

    /// Is this an enemy of `team` (owned, on another team)?
    pub fn hostile_to(&self, team: u8) -> bool {
        self.owner.is_some() && self.team != team
    }
}

/// World point at ground position `p` and height `y`.
#[inline]
pub(crate) fn lift(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

/// Ground-plane position of a world point.
#[inline]
pub(crate) fn ground(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.z)
}

/// Where a weapon's shots leave its host: the `WeaponLook::offset` mount
/// point rotated by the host's facing, at the host's altitude.
pub(crate) fn muzzle_point(pos: Vec2, facing: f32, altitude: f32, offset: [f32; 3]) -> Vec3 {
    let g = pos + af_geom::right(facing) * offset[0] + af_geom::forward(facing) * offset[2];
    lift(g, altitude + offset[1])
}

/// Read access to every damageable body plus the content and players.
#[derive(SystemParam)]
pub(crate) struct Arena<'w, 's> {
    pub data: Res<'w, Data>,
    pub players: Res<'w, Players>,
    spatial: Res<'w, SpatialIndex>,
    bodies: Query<'w, 's, BodyData, BodyFilter>,
}

impl Arena<'_, '_> {
    pub fn data(&self) -> &GameData {
        &self.data
    }

    /// The live body of `e`, if it is an alive unit or building.
    pub fn body(&self, e: Entity) -> Option<Body> {
        let (entity, pos, facing, fp, alt, owner, stats, vel, mobile, unit, building, constructing) = self.bodies.get(e).ok()?;
        let kind = match (unit, building) {
            (Some(u), _) => BodyKind::Unit(u.kind),
            (None, Some(b)) => BodyKind::Building(b.kind),
            (None, None) => return None,
        };
        let owner = owner.map(|o| o.0);
        Some(Body {
            entity,
            kind,
            pos: pos.0,
            obb: fp.obb(pos.0, facing.0),
            altitude: alt.0,
            height: fp.height,
            velocity: vel.map_or(Vec2::ZERO, |v| v.0),
            flying: mobile.is_some_and(|m| m.flying),
            under_construction: constructing,
            owner,
            team: owner.map_or(af_geom::NO_TEAM, |o| self.players.team(o)),
            tags: stats.map_or(TagSet::default(), |s| s.tags),
        })
    }

    /// Visit every body whose box is within `radius` of `point`, with that
    /// edge distance (0 inside the box).
    pub fn for_each_near(&self, point: Vec2, radius: f32, mut f: impl FnMut(Body, f32)) {
        self.spatial.for_each_near(point, radius + MOVE_MARGIN, |e| {
            if e.kind == SpatialKind::Node {
                return;
            }
            if let Some(b) = self.body(e.entity) {
                let d = b.obb.distance_to_point(point);
                if d <= radius {
                    f(b, d);
                }
            }
        });
    }

    /// Visit every body whose box is within `reach` of the box `from`, with
    /// the exact edge-to-edge distance ([`edge_distance`]).
    pub fn for_each_in_reach(&self, from: &Obb, reach: f32, mut f: impl FnMut(Body, f32)) {
        self.spatial.for_each_near(from.center, reach + from.bounding_radius() + MOVE_MARGIN, |e| {
            if e.kind == SpatialKind::Node {
                return;
            }
            if let Some(b) = self.body(e.entity) {
                let gap = edge_distance(from, &b.obb);
                if gap <= reach {
                    f(b, gap);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn muzzle_offset_follows_facing() {
        // Facing +X (yaw 90 degrees): forward is +X, right is -Z.
        let m = muzzle_point(Vec2::new(10.0, 10.0), std::f32::consts::FRAC_PI_2, 2.0, [0.5, 1.0, 2.0]);
        assert!((m - Vec3::new(12.0, 3.0, 9.5)).length() < 1e-4, "{m:?}");
    }

    #[test]
    fn edge_distance_matches_obb_gap() {
        let half = Vec2::splat(1.0);
        let a = Obb::new(Vec2::ZERO, half, 0.0);
        // Side by side: both measures agree.
        let beside = Obb::new(Vec2::new(5.0, 0.5), half, 0.0);
        assert!((edge_distance(&a, &beside) - 3.0).abs() < 1e-5);
        assert!((a.gap(&beside) - 3.0).abs() < 1e-5);
        // Corner to corner: the true distance is 3 * sqrt(2), not 3.
        let diagonal = Obb::new(Vec2::new(5.0, 5.0), half, 0.0);
        assert!((edge_distance(&a, &diagonal) - 3.0 * std::f32::consts::SQRT_2).abs() < 1e-4);
        assert!((a.gap(&diagonal) - edge_distance(&a, &diagonal)).abs() < 1e-5);
        // A box turned 45 degrees: its corner points at `a`'s face.
        let turned = Obb::new(Vec2::new(4.0, 0.0), half, std::f32::consts::FRAC_PI_4);
        assert!((edge_distance(&a, &turned) - (3.0 - std::f32::consts::SQRT_2)).abs() < 1e-4);
        assert!((edge_distance(&turned, &a) - edge_distance(&a, &turned)).abs() < 1e-6, "symmetric");
    }

    #[test]
    fn edge_distance_is_zero_for_touching_and_crossing_boxes() {
        let a = Obb::new(Vec2::ZERO, Vec2::new(3.0, 0.2), 0.0);
        let touching = Obb::new(Vec2::new(0.0, 1.2), Vec2::splat(1.0), 0.0);
        assert!(edge_distance(&a, &touching) < 1e-5);
        // A plus sign: no corner of either lies inside the other.
        let crossing = Obb::new(Vec2::ZERO, Vec2::new(3.0, 0.2), std::f32::consts::FRAC_PI_2);
        assert_eq!(edge_distance(&a, &crossing), 0.0);
    }

    #[test]
    fn ground_lift_roundtrip() {
        let p = Vec2::new(3.0, -4.0);
        assert_eq!(ground(lift(p, 7.0)), p);
    }
}
