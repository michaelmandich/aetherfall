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

use crate::tuning::{CHASE_DETOUR, CHASE_DETOUR_SLACK};

/// A unit's place in the regions of its mover class.
#[derive(Clone)]
pub(crate) struct Reach {
    map: Option<Arc<RegionMap>>,
    region: RegionId,
    pos: Vec2,
}

impl Reach {
    /// The reach of a unit at `pos` that moves as `mobile` does.
    pub(crate) fn of(regions: Option<&NavRegions>, pos: Vec2, mobile: &Mobile) -> Reach {
        let map = if mobile.flying { None } else { regions.and_then(|r| r.map(mobile.mobility, mobile.radius)) };
        let region = map.as_ref().map_or(0, |m| m.region_near(pos, START_SEARCH_RADIUS));
        Reach { map, region, pos }
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

    /// Is the way to `p`, in the unit's region, a detour a chase should not
    /// take: more than [`CHASE_DETOUR`] times the straight distance (plus
    /// [`CHASE_DETOUR_SLACK`])? (Across water `within` says whether a
    /// weapon reaches it from the near bank.) A clear line over the terrain
    /// costs a walk along it; only a blocked one costs a coarse search.
    pub(crate) fn detour(&self, p: Vec2) -> bool {
        let Some(map) = self.knows() else { return false };
        if map.region_at(p) != self.region || map.line_open(self.pos, p) {
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
