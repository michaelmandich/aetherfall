//! What a unit can get to over the terrain, for automatic choices: the
//! regions of af_movement's `NavRegions` (`docs/design/maps-and-terrain.md`,
//! section 7). A target out of the unit's region can never be walked up to
//! (nor reached by a weapon from a bank too far off), so drop sites,
//! replacement nodes, fields, formation slots, patients and hostiles there
//! are left alone instead of walked at until the path fails; in the same
//! region the way may still be long (round by a ford), which ranking by
//! route distance and the chase's detour check see.
//!
//! Everything counts as in reach for flyers, without `NavRegions` (tests
//! that set up a flat world only) and for a unit with no open ground about.

use std::sync::Arc;

use af_core::ext::movement::NavRegions;
use af_core::prelude::af_geom::{RegionId, RegionMap, GOAL_SEARCH_RADIUS, START_SEARCH_RADIUS};
use af_core::prelude::*;
use bevy::prelude::*;

use crate::tuning::{CHASE_DETOUR, CHASE_DETOUR_SLACK, CHASE_FRACTION};

/// A unit's place in the regions of its mover class.
#[derive(Clone)]
pub(crate) struct Reach {
    map: Option<Arc<RegionMap>>,
    region: RegionId,
    pos: Vec2,
    /// `Mobile::radius`.
    radius: f32,
}

impl Reach {
    /// The reach of a unit at `pos` that moves as `mobile` does.
    pub(crate) fn of(regions: Option<&NavRegions>, pos: Vec2, mobile: &Mobile) -> Reach {
        let map = if mobile.flying { None } else { regions.and_then(|r| r.map(mobile.mobility, mobile.radius)) };
        let region = map.as_ref().map_or(0, |m| m.region_near(pos, START_SEARCH_RADIUS));
        Reach { map, region, pos, radius: mobile.radius }
    }

    /// Terrain stamp of the regions it reads (0 without any): what
    /// `Unreachable::sync` keeps its for-good entries to.
    pub(crate) fn stamp(&self) -> u64 {
        self.map.as_ref().map_or(0, |m| m.stamp())
    }

    /// Does it know anything (a ground unit on open ground, with regions)?
    fn knows(&self) -> Option<&RegionMap> {
        self.map.as_deref().filter(|_| self.region != 0)
    }

    /// Can the unit get within `reach` of the box `obb`? When the box
    /// stands in the unit's region, yes; else (across water or rock, or on
    /// ground the unit's class cannot use) when the unit's region comes
    /// within `reach` of it, looked for on the line from the box back to
    /// the unit: an archer shoots across a stream, a worker does not chop
    /// across it.
    pub(crate) fn within(&self, obb: &Obb, reach: f32) -> bool {
        let Some(map) = self.knows() else { return true };
        if map.region_at(obb.center) == self.region {
            return true;
        }
        map.facing(self.region, self.pos, obb.center).is_some_and(|p| obb.distance_to_point(p) <= reach)
    }

    /// Route distance from the unit to `p` (the straight line when it stays
    /// on ground the unit may use, else the coarse route's length); `None`
    /// when `p` is out of reach. The straight distance when nothing is known.
    pub(crate) fn route_distance(&self, p: Vec2) -> Option<f32> {
        let Some(map) = self.knows() else { return Some(self.pos.distance(p)) };
        let (Some(a), Some(b)) = (map.open_near(self.pos, START_SEARCH_RADIUS), map.open_near(p, GOAL_SEARCH_RADIUS)) else {
            return Some(self.pos.distance(p));
        };
        map.distance(a, b).map(|d| d + self.pos.distance(a) + p.distance(b))
    }

    /// How much longer than the straight line the way to `p` is: 0 when the
    /// line is clear, nothing is known, or `p` lies out of the unit's
    /// region (whatever `within` lets it work there is worked from the near
    /// side). Costs a coarse search only when the line is blocked.
    pub(crate) fn way_round(&self, p: Vec2) -> f32 {
        let straight = self.pos.distance(p);
        self.route_distance(p).map_or(0.0, |d| (d - straight).max(0.0))
    }

    /// Is the way to the box `obb`, in the unit's region, a detour a chase
    /// should not take: more than [`CHASE_DETOUR`] times the straight
    /// distance (plus [`CHASE_DETOUR_SLACK`])? Not when the straight line
    /// leads over ground the unit may use to where the chase ends: the
    /// first point [`CHASE_FRACTION`] of the weapon's `reach` plus the
    /// unit's radius from the box (af_movement ends a chaser's walk at the
    /// first cell that near). However long the way round to the box itself,
    /// an archer shoots across a forded river from its bank. (Across water
    /// with no crossing `within` says whether a weapon reaches it from the
    /// near bank.) A clear line costs a walk along it; only a blocked one
    /// costs a coarse search.
    pub(crate) fn detour(&self, obb: &Obb, reach: f32) -> bool {
        let Some(map) = self.knows() else { return false };
        let p = obb.center;
        if map.region_at(p) != self.region || map.line_open(self.pos, arrival(obb, self.pos, reach * CHASE_FRACTION + self.radius)) {
            return false;
        }
        let straight = self.pos.distance(p);
        self.route_distance(p).is_none_or(|d| d > straight * CHASE_DETOUR + CHASE_DETOUR_SLACK)
    }

    /// Where the unit can stand nearest `p` (the point itself when it can
    /// get there), for a formation slot across water from it.
    pub(crate) fn nearest(&self, p: Vec2) -> Vec2 {
        match self.knows() {
            Some(map) if map.region_at(p) != self.region => map.nearest_in(self.region, p).unwrap_or(p),
            _ => p,
        }
    }
}

/// The first point on the straight way from `from` to the centre of `obb`
/// within `close` of the box (`from` itself when it is).
fn arrival(obb: &Obb, from: Vec2, close: f32) -> Vec2 {
    if obb.distance_to_point(from) <= close {
        return from;
    }
    // The distance to a box only shrinks on the way to its centre: halve
    // the stretch where it falls to `close` down to a fraction of a mm.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..16 {
        let mid = (lo + hi) * 0.5;
        if obb.distance_to_point(from.lerp(obb.center, mid)) <= close {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    from.lerp(obb.center, hi)
}
