//! Shared items added by af_movement (other crates may use them).
//! Only af_movement edits this file. Never modify items in other af_core files.

use std::sync::{Arc, Mutex};

use af_geom::{CoarseRoute, DistanceField, RegionId, RegionMap, TerrainSnapshot, GOAL_SEARCH_RADIUS, START_SEARCH_RADIUS};
#[allow(unused_imports)]
use crate::prelude::*;
#[allow(unused_imports)]
use bevy::prelude::*;

/// What ground movers can reach on this map, and how far it is by the way
/// round (`docs/design/maps-and-terrain.md`, section 7): connected regions
/// of the terrain and a coarse route graph per mover class (the terrain
/// its mobility enters and its clearance), see `af_geom::regions`.
/// Obstacles, gates and teams are left out: a region is what the terrain
/// allows for the whole match, so a target in another region of the mover
/// can never be reached, while one in the same region usually can (unless
/// buildings close the way).
///
/// af_movement inserts it for each map (in `OnEnter(GameState::Playing)`
/// after af_map, and again first thing in `SimSet::Pathing` whenever the
/// terrain changed) and its path service answers goals out of a mover's
/// region from it without a search. Everyone else reads it. The map of a
/// mover class is built the first time anyone asks for it (a few
/// milliseconds on a 768 m map), as a pure function of the terrain, so it
/// does not matter which system asks first.
///
/// Absent in tests that only set up `af_core::testing::flat_world`: read
/// it as `Option<Res<NavRegions>>`. The free functions [`reachable`] and
/// [`route_distance`] take that option and treat a missing one as "all
/// reachable, by the straight line".
#[derive(Resource)]
pub struct NavRegions {
    terrain: Arc<TerrainSnapshot>,
    /// Region maps built so far, one per mover class.
    maps: Mutex<Vec<Arc<RegionMap>>>,
}

impl NavRegions {
    /// For the terrain of `nav` as it stands.
    pub fn new(nav: &NavGrid) -> NavRegions {
        NavRegions { terrain: Arc::new(nav.terrain_snapshot()), maps: Mutex::new(Vec::new()) }
    }

    /// The terrain stamp (`NavGrid::terrain_stamp`) it was made for.
    pub fn stamp(&self) -> u64 {
        self.terrain.stamp()
    }

    /// The region map of movers of `mobility` and half-width `radius`
    /// (`Mobile::radius`), built on first use; `None` for flyers, which go
    /// everywhere.
    pub fn map(&self, mobility: Mobility, radius: f32) -> Option<Arc<RegionMap>> {
        if mobility.is_flying() {
            return None;
        }
        let class = self.terrain.class(mobility.0, radius);
        let mut maps = self.maps.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = maps.iter().find(|m| m.class() == class) {
            return Some(m.clone());
        }
        let built = Arc::new(RegionMap::build(&self.terrain, mobility.0, radius));
        maps.push(built.clone());
        Some(built)
    }

    /// The region a mover of this class standing at `p` belongs to (that
    /// of the open ground nearest it, within the few metres the path service
    /// looks round a mover that was shoved somewhere tight): `None` for
    /// flyers, `Some(0)` when there is no open ground about.
    pub fn region(&self, p: Vec2, mobility: Mobility, radius: f32) -> Option<RegionId> {
        self.map(mobility, radius).map(|m| m.region_near(p, START_SEARCH_RADIUS))
    }

    /// Can a mover of this class at `from` get to `to`? When `to` stands on
    /// ground the class may use, exactly when both are in one region; else
    /// (a node in a forest, a spot in the water, a unit standing where this
    /// one cannot) when some ground of `from`'s region lies within `near`
    /// metres of it. Flyers reach everything, and so does a mover with no
    /// open ground about (the path service sorts that out).
    pub fn reachable(&self, from: Vec2, to: Vec2, mobility: Mobility, radius: f32, near: f32) -> bool {
        self.from(from, mobility, radius).reaches(to, near)
    }

    /// What a mover of this class standing at `from` can reach: for testing
    /// many targets at the cost of one lookup.
    pub fn from(&self, from: Vec2, mobility: Mobility, radius: f32) -> ReachFrom {
        let map = self.map(mobility, radius);
        let region = map.as_ref().map_or(0, |m| m.region_near(from, START_SEARCH_RADIUS));
        ReachFrom { map, region, from }
    }

    /// Are `a` and `b` in one region for this class? True for flyers and
    /// where either has no open ground about.
    pub fn same_region(&self, a: Vec2, b: Vec2, mobility: Mobility, radius: f32) -> bool {
        let Some(m) = self.map(mobility, radius) else { return true };
        let (ra, rb) = (m.region_near(a, START_SEARCH_RADIUS), m.region_near(b, GOAL_SEARCH_RADIUS));
        ra == 0 || rb == 0 || ra == rb
    }

    /// Route distance (metres) from `from` to `to` for this class: the
    /// straight line when it stays on ground the class may use, else the
    /// length of the coarse route round the terrain (obstacles left out).
    /// `None` when `to` is out of reach (see [`NavRegions::reachable`]);
    /// the straight distance for flyers and where either end has no open
    /// ground about. Costs a coarse search when the line is not open:
    /// rank many targets from one place with [`NavRegions::field`].
    pub fn route_distance(&self, from: Vec2, to: Vec2, mobility: Mobility, radius: f32) -> Option<f32> {
        let reach = self.from(from, mobility, radius);
        if reach.region == 0 {
            return Some(from.distance(to));
        }
        reach.route_distance(to)
    }

    /// The coarse route from `from` to `to` for this class (see
    /// `RegionMap::route`); `None` for flyers and when out of reach.
    pub fn route(&self, from: Vec2, to: Vec2, mobility: Mobility, radius: f32) -> Option<CoarseRoute> {
        let m = self.map(mobility, radius)?;
        let (a, b) = (m.open_near(from, START_SEARCH_RADIUS)?, m.open_near(to, GOAL_SEARCH_RADIUS)?);
        m.route(a, b)
    }

    /// The point a mover of this class at `from` can reach that is nearest
    /// `to` (`to` itself when reachable); `None` for flyers and a mover
    /// with no open ground about.
    pub fn nearest_reachable(&self, from: Vec2, to: Vec2, mobility: Mobility, radius: f32) -> Option<Vec2> {
        let m = self.map(mobility, radius)?;
        m.nearest_in(m.region_near(from, START_SEARCH_RADIUS), to)
    }

    /// Route distances from `from` to everywhere in its region up to
    /// `max` metres, for this class: one coarse search, then each target
    /// is a lookup. `None` for flyers and a mover with no open ground about.
    pub fn field(&self, from: Vec2, mobility: Mobility, radius: f32, max: f32) -> Option<RouteField> {
        let map = self.map(mobility, radius)?;
        let start = map.open_near(from, START_SEARCH_RADIUS)?;
        let field = map.field(start, max)?;
        Some(RouteField { map, field, from })
    }
}

/// What a mover of one class at one place can reach (see
/// [`NavRegions::from`]; [`reach_from`] makes one with or without regions).
#[derive(Clone, Debug)]
pub struct ReachFrom {
    map: Option<Arc<RegionMap>>,
    /// The mover's region (0: nowhere open about, or no map).
    region: RegionId,
    from: Vec2,
}

impl ReachFrom {
    /// Reaches everything, by the straight line (no regions, or a flyer).
    pub fn anything(from: Vec2) -> ReachFrom {
        ReachFrom { map: None, region: 0, from }
    }

    /// See [`NavRegions::reachable`].
    pub fn reaches(&self, to: Vec2, near: f32) -> bool {
        let Some(m) = self.map.as_deref().filter(|_| self.region != 0) else { return true };
        match m.region_at(to) {
            0 => m.touches(self.region, to, near),
            r => r == self.region,
        }
    }

    /// See [`NavRegions::route_distance`].
    pub fn route_distance(&self, to: Vec2) -> Option<f32> {
        let Some(m) = self.map.as_deref().filter(|_| self.region != 0) else { return Some(self.from.distance(to)) };
        let (Some(a), Some(b)) = (m.open_near(self.from, START_SEARCH_RADIUS), m.open_near(to, GOAL_SEARCH_RADIUS)) else {
            return Some(self.from.distance(to));
        };
        m.distance(a, b).map(|d| d + self.from.distance(a) + to.distance(b))
    }
}

/// What a mover of this class at `from` can reach, with regions or (all of
/// it, by the straight line) without.
pub fn reach_from(regions: Option<&NavRegions>, from: Vec2, mobility: Mobility, radius: f32) -> ReachFrom {
    match regions {
        Some(r) => r.from(from, mobility, radius),
        None => ReachFrom::anything(from),
    }
}

/// Route distances from one point (see [`NavRegions::field`]).
#[derive(Clone, Debug)]
pub struct RouteField {
    map: Arc<RegionMap>,
    field: DistanceField,
    from: Vec2,
}

impl RouteField {
    /// Where it measures from.
    pub fn from(&self) -> Vec2 {
        self.from
    }

    /// Terrain stamp of the map it was made on (see [`NavRegions::stamp`]).
    pub fn stamp(&self) -> u64 {
        self.map.stamp()
    }

    /// Route distance (metres, the coarse estimate: a few percent over the
    /// way a mover walks) to `p`, or to the open ground nearest it within
    /// `near` metres; `None` when out of reach or past the field's limit.
    pub fn get(&self, p: Vec2, near: f32) -> Option<f32> {
        let q = self.map.open_near(p, near)?;
        self.field.get(&self.map, q).map(|d| d + p.distance(q))
    }
}

/// [`NavRegions::reachable`] when there are regions; everything is
/// reachable without them.
pub fn reachable(regions: Option<&NavRegions>, from: Vec2, to: Vec2, mobility: Mobility, radius: f32, near: f32) -> bool {
    regions.is_none_or(|r| r.reachable(from, to, mobility, radius, near))
}

/// [`NavRegions::route_distance`] when there are regions; the straight
/// distance without them.
pub fn route_distance(regions: Option<&NavRegions>, from: Vec2, to: Vec2, mobility: Mobility, radius: f32) -> Option<f32> {
    match regions {
        Some(r) => r.route_distance(from, to, mobility, radius),
        None => Some(from.distance(to)),
    }
}

/// What the collision solver's final word on pairs did since the match
/// started: the bookkeeping behind the no-overlap rule, for tests, tuning
/// and the headless report. Written by af_movement at the end of every
/// tick's collision solve (cheap counters, no per-unit state); nothing in
/// the simulation reads it.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct CollisionStats {
    /// Ticks solved.
    pub ticks: u64,
    /// Unit-ticks a unit was held back toward where it started the tick
    /// (wholly or part of the way), so that no pair ended deeper than it
    /// began: the crowd stood still there.
    pub held: u64,
    /// Pairs the final word parted, pair-ticks (pushed apart, a turn undone,
    /// a unit set down nearby or sent back, eased apart part of the way).
    pub parted: u64,
    /// Unit-ticks a unit was set down on a free spot rather than pushed: the
    /// last resort, a visible jump.
    pub relocated: u64,
    /// Farthest the final word moved one unit in one tick (metres, from
    /// where the solver passes left it; units it only took back toward
    /// where they began the tick, held or sent back, aside), and when (sim
    /// seconds).
    pub max_correction: f32,
    pub max_correction_at: f32,
    /// Farthest one relocation set a unit down (metres).
    pub max_relocation: f32,
    /// Pair-ticks left deeper than the final tolerance (every one no deeper
    /// than the pair began the tick).
    pub left: u64,
    /// Unit-ticks of units that began the tick overlapping another deeper
    /// than the final tolerance (trained onto it, a foundation laid on
    /// them): how long such overlaps last.
    pub overlapped: u64,
    /// Unit-ticks a ground unit ended farther than [`Self::JUMP`] from where
    /// it began the tick (the fastest walker covers about 0.3 m), by what
    /// moved it last: the final word on pairs (but for taking it back toward
    /// where it began), the obstacles (pushed or set down out of a new
    /// foundation), or the solver passes alone.
    pub jumps_final: u64,
    pub jumps_eject: u64,
    pub jumps_passes: u64,
    /// Farthest a unit the final word on pairs moved (as for
    /// [`Self::max_correction`]) ended from where it began its tick
    /// (metres): the jump the player sees, walk and shoves included.
    pub max_final_move: f32,
}

impl CollisionStats {
    /// A one-tick move past this is a visible jump (metres).
    pub const JUMP: f32 = 0.4;

    /// One line for reports.
    pub fn describe(&self) -> String {
        format!(
            "{} ticks: {} unit-ticks held back, {} parted, {} set down (farthest {:.2} m), largest correction {:.2} m at t={:.1}s, largest one-tick move of a unit it moved {:.2} m, {} pair-ticks left too deep, {} unit-ticks overlapped at tick start; jumps >{} m by the final word {}, out of foundations {}, by the passes {}",
            self.ticks,
            self.held,
            self.parted,
            self.relocated,
            self.max_relocation,
            self.max_correction,
            self.max_correction_at,
            self.max_final_move,
            self.left,
            self.overlapped,
            Self::JUMP,
            self.jumps_final,
            self.jumps_eject,
            self.jumps_passes
        )
    }

    /// Add another match's counts (the maxima are kept).
    pub fn add(&mut self, other: &CollisionStats) {
        self.ticks += other.ticks;
        self.held += other.held;
        self.parted += other.parted;
        self.relocated += other.relocated;
        self.left += other.left;
        self.overlapped += other.overlapped;
        self.jumps_final += other.jumps_final;
        self.jumps_eject += other.jumps_eject;
        self.jumps_passes += other.jumps_passes;
        self.max_relocation = self.max_relocation.max(other.max_relocation);
        self.max_final_move = self.max_final_move.max(other.max_final_move);
        if other.max_correction > self.max_correction {
            (self.max_correction, self.max_correction_at) = (other.max_correction, other.max_correction_at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{new_nav, test_data};

    /// A 128 m grass map with a 12 m river down x = 58..70, crossed by a
    /// 7 m band of `crossing` centred on y = 110 (none when `None`).
    fn river(crossing: Option<&str>) -> NavGrid {
        let data = test_data();
        let id = |n: &str| data.terrain_id(n).unwrap_or_else(|| panic!("no terrain `{n}`"));
        let mut nav = new_nav(&data, 128, 128, id("grass"));
        for y in 0..128 {
            for x in 58..70 {
                let t = match crossing {
                    Some(c) if (y - 110i32).abs() < 4 => id(c),
                    _ => id("deep_water"),
                };
                nav.set_terrain(IVec2::new(x, y), t.0);
            }
        }
        nav
    }

    /// Reach, route distance and the nearest reachable point, per mover
    /// class: feet and wheels cross at a ford, only feet in the shallows,
    /// nobody without either, flyers always.
    #[test]
    fn regions_answer_reach_and_route_distance_per_class() {
        let (foot, wheels, air) = (Mobility::of(MoveFlag::Foot), Mobility::of(MoveFlag::Wheeled), Mobility::of(MoveFlag::Flying));
        let (west, east) = (Vec2::new(30.5, 30.5), Vec2::new(100.5, 30.5));
        let ford = NavRegions::new(&river(Some("ford")));
        for m in [foot, wheels] {
            assert!(ford.reachable(west, east, m, 0.4, 1.0) && ford.same_region(west, east, m, 0.4));
            let d = ford.route_distance(west, east, m, 0.4).expect("by the ford");
            assert!(d > 150.0 && d < 190.0, "{d}");
        }
        let shallows = NavRegions::new(&river(Some("shallow_water")));
        assert!(shallows.reachable(west, east, foot, 0.4, 1.0));
        assert!(!shallows.reachable(west, east, wheels, 0.4, 1.0), "wheels never enter shallows");
        let none = NavRegions::new(&river(None));
        assert!(!none.reachable(west, east, foot, 0.4, 1.0) && !none.same_region(west, east, foot, 0.4));
        assert_eq!(none.route_distance(west, east, foot, 0.4), None);
        let bank = none.nearest_reachable(west, east, foot, 0.4).expect("the near bank");
        assert!((bank - Vec2::new(57.5, 30.5)).length() < 0.01, "{bank}");
        // A spot in the water: reachable when the bank is near enough.
        let in_water = Vec2::new(59.5, 30.5);
        assert!(none.reachable(west, in_water, foot, 0.4, 3.0) && !none.reachable(west, Vec2::new(64.0, 30.5), foot, 0.4, 3.0));
        // Flyers go everywhere, by the straight line.
        assert!(none.reachable(west, east, air, 0.4, 1.0));
        assert_eq!(none.route_distance(west, east, air, 0.4), Some(70.0));
        assert!(none.map(air, 0.4).is_none());
        // Without regions everything is reachable by the straight line.
        assert!(reachable(None, west, east, foot, 0.4, 1.0));
        assert_eq!(route_distance(None, west, east, foot, 0.4), Some(70.0));
        assert_eq!(route_distance(Some(&none), west, east, foot, 0.4), None);
    }

    /// A field ranks targets by the way round, like route distances do,
    /// and one region map serves every mobility entering the same terrain.
    #[test]
    fn fields_and_shared_maps() {
        let regions = NavRegions::new(&river(Some("ford")));
        let foot = Mobility::of(MoveFlag::Foot);
        let field = regions.field(Vec2::new(30.5, 30.5), foot, 0.4, f32::INFINITY).expect("a field");
        let across = field.get(Vec2::new(80.5, 30.5), 2.0).expect("reachable");
        let along = field.get(Vec2::new(30.5, 110.5), 2.0).expect("reachable");
        assert!(across > along, "across the river ({across}) is farther than 80 m up this bank ({along})");
        assert_eq!(field.get(Vec2::new(64.0, 30.5), 2.0), None, "mid-river");
        let a = regions.map(foot, 0.4).expect("a map");
        let b = regions.map(foot.with(MoveFlag::Dunerunner), 0.3).expect("a map");
        assert!(Arc::ptr_eq(&a, &b), "a trait that only changes speeds enters the same terrain");
        assert_eq!(regions.stamp(), a.stamp());
    }
}
