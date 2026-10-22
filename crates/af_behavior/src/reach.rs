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

/// Rings of cells round a formation slot searched for the unit's region
/// before the region-wide search (see [`Reach::nearest`]): a slot in a
/// river or a few metres off a shore never needs that.
const SLOT_RINGS: i32 = 16;

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
    /// get there, else the centre of the nearest cell of its region), for a
    /// formation slot across water from it. `grid` is the grid the regions
    /// were built on: the few metres round `p` are looked through first, so
    /// a slot in a river costs about the cells to the bank, and only one far
    /// out on a lake or the sea the region-wide search.
    pub(crate) fn nearest(&self, p: Vec2, grid: &NavGrid) -> Vec2 {
        match self.knows() {
            Some(map) if map.region_at(p) != self.region => {
                near_cell_in(map, grid, self.region, p).or_else(|| map.nearest_in(self.region, p)).unwrap_or(p)
            }
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

/// The centre of the cell of `region` nearest `p` (whose own cell is not
/// one) within [`SLOT_RINGS`] rings of cells of `grid` round it: square
/// rings outward until none left can hold a nearer cell. `None` when that
/// is not settled within the rings.
fn near_cell_in(map: &RegionMap, grid: &NavGrid, region: RegionId, p: Vec2) -> Option<Vec2> {
    let c = grid.cell_of(p);
    let mut best: Option<(f32, Vec2)> = None;
    for r in 1..=SLOT_RINGS + 1 {
        // Every cell of ring r has its centre at least r - 1/2 cells from `p`.
        let bound = (r as f32 - 0.5) * grid.cell_size();
        if let Some((d, q)) = best
            && d <= bound * bound
        {
            return Some(q);
        }
        if r > SLOT_RINGS {
            break;
        }
        for dy in -r..=r {
            // Along the ring: every cell of its top and bottom rows, the two
            // end cells of the rows between.
            let step = if dy.abs() == r { 1 } else { 2 * r };
            for dx in (-r..=r).step_by(step as usize) {
                let q = grid.cell_center(c + IVec2::new(dx, dy));
                let d = q.distance_squared(p);
                if map.region_at(q) == region && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, q));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    use super::*;

    /// A side x side grid of land (Foot) whose east half from x = `shore`
    /// on is deep water, its regions, and a walker on the land.
    fn lake_shore(side: u32, shore: i32) -> (NavGrid, NavRegions, Mobile) {
        let walker = Mobile { mobility: Mobility::of(MoveFlag::Foot), radius: 0.3, turn_rate: 6.0, flying: false };
        let mut grid = NavGrid::new(side, side, 1.0, Vec2::ZERO);
        grid.set_terrain_info(0, walker.mobility.0, true);
        grid.set_terrain_info(1, 0, false);
        for y in 0..side as i32 {
            for x in shore..side as i32 {
                grid.set_terrain(IVec2::new(x, y), 1);
            }
        }
        let regions = NavRegions::new(&grid);
        (grid, regions, walker)
    }

    /// A formation slot in the water lands on the cell of the shore nearest
    /// it, as the region-wide search finds it; on land it stays put.
    #[test]
    fn a_slot_in_the_water_lands_on_the_nearest_shore() {
        let (grid, regions, walker) = lake_shore(256, 128);
        let reach = Reach::of(Some(&regions), Vec2::new(60.0, 60.0), &walker);
        let map = regions.map(walker.mobility, walker.radius).expect("a map");
        let region = map.region_at(Vec2::new(60.0, 60.0));
        // In the water just off the shore, a few metres out, and far out
        // (past the rings: the region-wide search answers).
        let slots = [Vec2::new(128.2, 40.7), Vec2::new(131.9, 100.1), Vec2::new(143.5, 12.25), Vec2::new(200.0, 70.0), Vec2::new(250.0, 0.4)];
        for (i, slot) in slots.into_iter().enumerate() {
            let q = reach.nearest(slot, &grid);
            let full = map.nearest_in(region, slot).expect("a shore");
            assert_eq!(map.region_at(q), region, "slot {i} on the walker's ground: {q}");
            assert!((q.distance(slot) - full.distance(slot)).abs() < 1e-4, "slot {i}: {q} is no nearer than {full}");
        }
        assert_eq!(reach.nearest(Vec2::new(90.3, 33.3), &grid), Vec2::new(90.3, 33.3));
    }

    /// On a big map, slots off a shore cost a look round each, not a walk
    /// over the whole region (thousands of coarse nodes) per slot: an order
    /// to a lake shore must not stall the tick it is given in. The bound is
    /// absolute, since `RegionMap::nearest_in` itself no longer sorts the
    /// region (it is checked here only as the reference answer's cost).
    #[test]
    fn slots_off_a_shore_on_a_big_map_skip_the_region_wide_search() {
        let (grid, regions, walker) = lake_shore(1024, 512);
        let reach = Reach::of(Some(&regions), Vec2::new(300.0, 500.0), &walker);
        let map = regions.map(walker.mobility, walker.radius).expect("a map");
        let region = map.region_at(Vec2::new(300.0, 500.0));
        // An 80-unit group: half its slots in the water, up to 8 m out.
        let slots: Vec<Vec2> = (0..80).map(|i| Vec2::new(512.3 + (i % 8) as f32, 480.6 + (i / 8) as f32 * 2.1)).collect();
        let time = |f: &dyn Fn(Vec2) -> Vec2| {
            (0..3)
                .map(|_| {
                    let start = Instant::now();
                    for &s in &slots {
                        black_box(f(black_box(s)));
                    }
                    start.elapsed()
                })
                .min()
                .unwrap_or(Duration::ZERO)
        };
        let bounded = time(&|s| reach.nearest(s, &grid));
        let full = time(&|s| map.nearest_in(region, s).expect("a shore"));
        // 80 slots well inside a millisecond each way (a sort of the region
        // per slot took about 2 ms a slot on this map).
        assert!(bounded < Duration::from_millis(5), "{bounded:?} for the slots (region-wide answers: {full:?})");
        assert!(full < Duration::from_millis(20), "{full:?} for the region-wide answers");
    }
}
