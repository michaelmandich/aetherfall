//! The path service (`SimSet::Pathing`, after obstacle sync).
//!
//! * New goals (`MoveGoal::seq` differs from `PathState::goal_seq`) and
//!   repaths of following units (stuck, path cut by a newly placed obstacle,
//!   followed entity moved away, end of a leg reached) are answered on the
//!   spot when they need no search: flyers (they fly straight), clear
//!   straight lines, goals a groupmate's fresh search already leads to (the
//!   same goal cell), and goals near the end of a route found lately from
//!   near the mover (see [`PathQueue::share`]); the line checks of offering
//!   routes are charged to the tick's search budget (at most half of it
//!   before the queue is served, the rest waiting in line to be offered
//!   them there).
//! * Regions (`docs/design/maps-and-terrain.md`, section 7): a ground goal
//!   out of the mover's region of the terrain (see
//!   `af_core::ext::movement::NavRegions`) is swapped for the reachable
//!   point nearest it, with no search to find that out; the path leads
//!   there and is partial, so the unit walks to the bank facing the goal
//!   and af_behavior hears that it failed.
//! * Long ways: a goal farther than [`LONG_WAY`] that no straight line
//!   reaches is planned on the coarse route graph of the mover's class
//!   first. A route that bends round the terrain (a river crossed at a
//!   ford, a range at its pass) is then searched leg by leg, about
//!   [`LEG_LENGTH`] of it at a time, each leg well within the search cap:
//!   the path ends at the end of the leg (partial, more to come), and once
//!   less than [`LEG_AHEAD`] of it is left, the next leg is searched from
//!   its end and added to it while the unit walks on. Groupmates share
//!   coarse routes as they share paths, and the coarse search is charged
//!   to the budget too.
//! * The rest join the queue: new goals as `Pending`, repaths keeping their
//!   current path until the new one is ready. The queue holds a line per
//!   player, served in turn, one request each round, under a per-tick A*
//!   expansion budget: a mass order spreads over a few ticks, nobody
//!   starves, and one player's long searches never hold up another's
//!   workers. Each request is first offered the routes of searches served
//!   before it, so a group ordered to one place (each unit to its own spot
//!   of a formation) costs a few searches, not one per unit. A search the
//!   tick's budget cuts short is set aside (`NavGrid::find_path_sliced`)
//!   and goes on where it stopped first thing next tick, so a long search
//!   costs several short ticks instead of one long one.
//! * A repath asked for because the unit got stuck goes round the ground
//!   units near it where it can (the cells they stand on cost more): a
//!   worker held up by another at work in a gap, or a column wading into a
//!   blob, takes another way when there is one, and walks on through them
//!   when there is none.

use std::collections::VecDeque;
use std::sync::Arc;

use af_core::ext::movement::NavRegions;
use af_core::prelude::af_geom::{CoarseRoute, PathRequest, PathStep, RegionClass, RegionMap, GOAL_SEARCH_RADIUS, START_SEARCH_RADIUS};
use af_core::prelude::*;
use bevy::prelude::*;

use crate::mover::{Aim, FollowTarget, Mover};
use crate::obstacles::ObstacleRegistry;

/// A* expansions allowed per tick across all requests (a hard cap: each
/// search may use only what is left of it, and goes on next tick when that
/// runs out). About 1.5 ms of searching.
const TICK_EXPANSION_BUDGET: u32 = 8_000;
/// Expansion cap of one search (one leg of a long way), over however many
/// ticks it takes; a longer search returns its best partial path and the
/// unit repaths when it gets there.
const MAX_EXPANSIONS_PER_PATH: u32 = 24_000;
/// No request is taken up with less budget than this left in the tick.
const MIN_SEARCH_BUDGET: u32 = 500;
/// Budget charged per request on top of its expansions (line checks, smoothing).
const REQUEST_OVERHEAD: u32 = 50;
/// Most of the tick's budget that offering new requests routes to share
/// may spend before the queue is served (the queue keeps the rest).
const SHARE_ON_ARRIVAL_BUDGET: u32 = TICK_EXPANSION_BUDGET / 2;
/// A mover that got stuck goes round the other ground units this close to it.
const AVOID_RADIUS: f32 = 8.0;
/// ...and keeps its centre this much farther than its half-width from their boxes (metres).
const AVOID_MARGIN: f32 = 0.2;
/// Repath when the followed entity moved this far from where the path led.
const FOLLOW_REPATH_DISTANCE: f32 = 1.5;
/// Minimum seconds between follow-target repaths of one unit.
const REPATH_COOLDOWN: f32 = 0.25;
/// A path that falls short of its aim (a search cut by its budget, a leg
/// that ends short of the leg's end) is only taken while the way left from
/// its end shrinks by this much each time; else the unit gives up.
const TRUNCATED_MIN_GAIN: f32 = 1.0;
/// Walking up to an entity, a path may end this much farther from its box
/// than `arrive` plus the unit's radius (half a nav cell: the path ends on
/// a cell centre); the last stretch is walked straight at the box.
const FOLLOW_REACH_SLACK: f32 = 0.5;
/// Routes kept for sharing (see [`PathQueue::share`]).
const SHARED_ROUTES: usize = 32;
/// A route is shared with a mover starting this close to where it started...
const SHARE_START_RADIUS: f32 = 6.0;
/// ...and heading this close to where it led (metres).
const SHARE_GOAL_RADIUS: f32 = 8.0;
/// How many leading waypoints of a shared route a mover may join at.
const SHARE_JOIN: usize = 3;
/// Most routes tried for one request (the newest that qualify).
const SHARE_TRIES: usize = 4;
/// What a line check costs against the path service's budget, in A*
/// expansions per nav cell it crosses (it tests one clearance per cell,
/// several times cheaper than expanding a cell).
const LINE_CHECK_CELL_COST: f32 = 0.25;
/// A goal farther than this (metres) that no straight line reaches is
/// planned on the coarse route graph first.
const LONG_WAY: f32 = 48.0;
/// Length of route one leg of a long way covers (metres). A leg over open
/// ground is a straight line or a search of a few hundred cells; one
/// through a forest a few thousand.
const LEG_LENGTH: f32 = 64.0;
/// The next leg is searched once this much of the path is left (metres:
/// several seconds of walking, while the unit walks on).
const LEG_AHEAD: f32 = 24.0;
/// A leg path ending this close to the leg's end got there (metres; a leg
/// end inside a tree or a building is reached beside it).
const LEG_REACHED: f32 = GOAL_SEARCH_RADIUS;
/// A unit this far off its coarse route (metres; pushed, or chasing a goal
/// that ran) gets a new one.
const OFF_ROUTE: f32 = 32.0;
/// Coarse routes kept for sharing (see [`PathQueue::shared_route`]).
const SHARED_COARSE: usize = 16;
/// A coarse route serves a mover starting this close to where it started...
const COARSE_SHARE_START: f32 = 24.0;
/// ...and a way ending this close to where it leads (metres). A unit's own
/// route is kept as long as its goal stays this close to the route's end.
const COARSE_SHARE_GOAL: f32 = 16.0;
/// What a coarse search costs against the budget, in A* expansions per
/// coarse node it expands (measured: about what a cell expansion costs;
/// each looks at up to eight links instead of eight cells' clearance).
const COARSE_EXPANSION_COST: u32 = 1;

/// Budget charged for checking the line from `a` to `b` (see
/// [`LINE_CHECK_CELL_COST`]): the walk visits one cell per cell boundary it
/// crosses on either axis.
fn line_check_cost(nav: &NavGrid, a: Vec2, b: Vec2) -> u32 {
    let cells = (b - a).abs().element_sum() / nav.cell_size() + 1.0;
    1 + (cells * LINE_CHECK_CELL_COST) as u32
}

/// Units waiting for a path, a line per player, and recent routes to share.
#[derive(Resource, Debug, Default)]
pub(crate) struct PathQueue {
    /// Waiting units by owner (player index), each line oldest first.
    lines: Vec<VecDeque<Entity>>,
    /// The line served last: the lines take turns.
    turn: usize,
    /// A unit whose search the last tick's budget cut short (or whose look
    /// for a route to share spent it): first in line next tick.
    resume: Option<Entity>,
    routes: VecDeque<Route>,
    /// Coarse routes planned lately, for groupmates.
    coarse: VecDeque<SharedCoarse>,
    /// Budget (in A* expansions) spent this tick on offering new requests
    /// routes to share, before the queue is served: the queue gets what is
    /// left of the tick's budget.
    spent: u32,
    /// `NavGrid::sync_tag` of the grid `routes` were found on: a new map
    /// (a new grid, whose version count starts over) drops them.
    grid: u64,
    /// Cells expanded by searches, and coarse nodes by route planning,
    /// since the app started (for tests and measurements).
    #[cfg_attr(not(test), allow(dead_code))]
    expanded: u64,
    #[cfg_attr(not(test), allow(dead_code))]
    coarse_expanded: u64,
}

/// A complete route to a point, found by a search or shared from one.
#[derive(Clone, Debug)]
struct Route {
    mobility: u16,
    /// Clearance class (`NavGrid::clearance_cells` of the radius).
    clearance: i32,
    team: u8,
    /// `NavGrid::sync_tag` and `NavGrid::version` of the grid it was found
    /// on (any grid change retires it). The version alone does not name a
    /// grid: every new map's grid counts its versions from 1 again.
    grid: u64,
    version: u64,
    start: Vec2,
    goal: Vec2,
    /// Waypoints after `start`, the last one `goal`.
    waypoints: Vec<Vec2>,
}

/// A coarse route planned lately, for groupmates going the same way.
#[derive(Clone, Debug)]
struct SharedCoarse {
    class: RegionClass,
    /// Terrain stamp of the region map it was planned on.
    stamp: u64,
    from: Vec2,
    to: Vec2,
    route: Arc<CoarseRoute>,
}

impl PathQueue {
    fn push(&mut self, e: Entity, owner: PlayerId, mover: &mut Mover) {
        if !mover.queued {
            mover.queued = true;
            let i = owner.index();
            if self.lines.len() <= i {
                self.lines.resize_with(i + 1, VecDeque::new);
            }
            self.lines[i].push_back(e);
        }
    }

    /// The next unit to serve: the one set aside last tick, else the
    /// front of the next line after the one served last.
    fn pop(&mut self) -> Option<Entity> {
        if let Some(e) = self.resume.take() {
            return Some(e);
        }
        let n = self.lines.len();
        for step in 1..=n {
            let i = (self.turn + step) % n;
            if let Some(e) = self.lines[i].pop_front() {
                self.turn = i;
                return Some(e);
            }
        }
        None
    }

    /// Units waiting, set aside included.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lines.iter().map(VecDeque::len).sum::<usize>() + usize::from(self.resume.is_some())
    }

    /// A recent route that serves `ask` without a search: one found on this
    /// grid as it is now (see [`Route::grid`]), of the same mover class,
    /// that started within [`SHARE_START_RADIUS`] and led within
    /// [`SHARE_GOAL_RADIUS`] of its goal, joined in a straight line at one of
    /// its first waypoints and left in a straight line at its last turn (or
    /// at its end) for the goal itself. So a group ordered to one
    /// place, each to its own spot of a formation, walks the route of one
    /// search instead of searching each its own. Point goals of ground
    /// movers only; the grid must be prepared for the mover's class. Adds
    /// what its line checks cost, in A* expansions, to `cost`.
    fn share(&self, nav: &NavGrid, ask: &Ask, cost: &mut u32) -> Option<af_geom::PathResult> {
        if ask.follow.is_some() || ask.mob.flying {
            return None;
        }
        let (mobility, radius, team) = (ask.mob.mobility.0, ask.mob.radius, ask.team);
        let clearance = nav.clearance_cells(radius);
        let spent = std::cell::Cell::new(0u32);
        let line = |a: Vec2, b: Vec2| {
            spent.set(spent.get() + line_check_cost(nav, a, b));
            nav.line_clear(a, b, mobility, radius, team)
        };
        let near = |r: &&Route| {
            let alike = r.grid == nav.sync_tag() && r.version == nav.version() && r.mobility == mobility && r.clearance == clearance && r.team == team;
            alike && r.start.distance(ask.pos) <= SHARE_START_RADIUS && r.goal.distance(ask.target) <= SHARE_GOAL_RADIUS
        };
        // The newest few near routes only: each costs a handful of line checks.
        let shared = self.routes.iter().rev().filter(near).take(SHARE_TRIES).find_map(|r| {
            let n = r.waypoints.len();
            let join = (0..n.min(SHARE_JOIN)).rev().find(|&i| line(ask.pos, r.waypoints[i]))?;
            // The route up to its last turn, then straight on to the goal;
            // else all of it and on from its end.
            let turns = &r.waypoints[join..n - 1];
            let last_turn = turns.last().copied().unwrap_or(ask.pos);
            let mut waypoints = if line(last_turn, ask.target) {
                turns.to_vec()
            } else if line(r.goal, ask.target) {
                r.waypoints[join..].to_vec()
            } else {
                return None;
            };
            waypoints.push(ask.target);
            waypoints.dedup();
            Some(af_geom::PathResult { waypoints, complete: true, ..Default::default() })
        });
        *cost += spent.get();
        shared
    }

    /// Forget the match: its waiting units (despawned with it), whose turn
    /// it was, how many lines it had, and its routes. The next match is
    /// served as in a fresh game (as a replay of it, or a lockstep peer that
    /// just started, serves it): the lines take turns from the first again.
    /// The expansion counts run on.
    fn start_over(&mut self) {
        self.lines.clear();
        self.turn = 0;
        self.resume = None;
        self.routes.clear();
        self.coarse.clear();
        self.spent = 0;
        self.grid = 0;
    }

    /// Drop the routes of another grid (the previous match's map), so a new
    /// map starts with none, as in a fresh game.
    fn follow_grid(&mut self, nav: &NavGrid) {
        if self.grid != nav.sync_tag() {
            self.grid = nav.sync_tag();
            self.routes.clear();
            self.coarse.clear();
        }
    }

    /// Keep a complete route to `ask`'s point goal for sharing.
    fn remember(&mut self, nav: &NavGrid, ask: &Ask, waypoints: &[Vec2]) {
        if ask.follow.is_some() || ask.mob.flying || waypoints.is_empty() {
            return;
        }
        if self.routes.len() >= SHARED_ROUTES {
            self.routes.pop_front();
        }
        self.routes.push_back(Route {
            mobility: ask.mob.mobility.0,
            clearance: nav.clearance_cells(ask.mob.radius),
            team: ask.team,
            grid: nav.sync_tag(),
            version: nav.version(),
            start: ask.pos,
            goal: ask.target,
            waypoints: waypoints.to_vec(),
        });
    }

    /// A coarse route planned lately on `map` from within
    /// [`COARSE_SHARE_START`] of `from` to within [`COARSE_SHARE_GOAL`] of
    /// `to` (the newest such).
    fn shared_route(&self, map: &RegionMap, from: Vec2, to: Vec2) -> Option<Arc<CoarseRoute>> {
        let (class, stamp) = (map.class(), map.stamp());
        self.coarse
            .iter()
            .rev()
            .find(|c| c.class == class && c.stamp == stamp && c.from.distance(from) <= COARSE_SHARE_START && c.to.distance(to) <= COARSE_SHARE_GOAL)
            .map(|c| c.route.clone())
    }

    fn remember_route(&mut self, map: &RegionMap, from: Vec2, to: Vec2, route: Arc<CoarseRoute>) {
        if self.coarse.len() >= SHARED_COARSE {
            self.coarse.pop_front();
        }
        self.coarse.push_back(SharedCoarse { class: map.class(), stamp: map.stamp(), from, to, route });
    }
}

/// Where a goal currently is: the followed entity's centre, else the target point.
pub(crate) fn goal_point(goal: &MoveGoal, follow: Option<&FollowTarget>) -> Option<Vec2> {
    match goal.follow {
        Some(_) => follow.map(|f| f.pos),
        None => Some(goal.target),
    }
}

/// Look up followed entities once per tick (positions at the start of movement).
pub(crate) fn refresh_follow_targets(
    mut movers: Query<(&MoveGoal, &mut Mover), Without<Dead>>,
    targets: Query<(&Position, &Facing, &Footprint), Without<Dead>>,
) {
    for (goal, mut mover) in &mut movers {
        let follow = goal.follow.and_then(|e| targets.get(e).ok()).map(|(p, f, fp)| FollowTarget { pos: p.0, obb: fp.obb(p.0, f.0) });
        if mover.follow != follow {
            mover.follow = follow;
        }
    }
}

/// Flag following units whose remaining path crosses an obstacle placed this tick.
pub(crate) fn invalidate_blocked_paths(
    reg: Res<ObstacleRegistry>,
    mut nav: ResMut<Nav>,
    players: Res<Players>,
    mut units: Query<(&Position, &Mobile, &Owner, &PathState, &mut Mover), Without<Dead>>,
) {
    if reg.fresh.is_empty() {
        return;
    }
    // Only path caches change here: not a grid change for anyone watching `Nav`.
    let nav = nav.bypass_change_detection();
    for (pos, mob, owner, path, mut mover) in &mut units {
        if mob.flying || path.status != PathStatus::Following || mover.wants_repath || path.next >= path.waypoints.len() {
            continue;
        }
        let pad = Vec2::splat(mob.radius + 1.0);
        let mut from = pos.0;
        let mut blocked = false;
        for &to in &path.waypoints[path.next..] {
            let (lo, hi) = (from.min(to) - pad, from.max(to) + pad);
            if reg.fresh.iter().any(|(min, max)| min.x <= hi.x && max.x >= lo.x && min.y <= hi.y && max.y >= lo.y) {
                let team = players.team(owner.0);
                nav.prepare(mob.mobility.0, team);
                if !nav.line_clear(from, to, mob.mobility.0, mob.radius, team) {
                    blocked = true;
                    break;
                }
            }
            from = to;
        }
        if blocked {
            mover.wants_repath = true;
        }
    }
}

/// One search request of a unit this tick.
struct Ask<'a> {
    goal: &'a MoveGoal,
    mob: &'a Mobile,
    /// Where the search starts: the unit, or the end of its path when the
    /// search is for the next leg.
    pos: Vec2,
    team: u8,
    /// Where the search heads: the goal (see [`goal_point`]), the
    /// reachable point nearest it, or the end of a leg.
    target: Vec2,
    /// The followed entity's box, when the search walks up to it.
    follow: Option<Obb>,
}

impl Ask<'_> {
    /// How far from the followed box a path may end (see [`FOLLOW_REACH_SLACK`]).
    fn follow_reach(&self) -> f32 {
        self.goal.arrive + self.mob.radius + FOLLOW_REACH_SLACK
    }

    /// How near the goal a unit must be able to get for it to count as in
    /// reach when the goal stands on ground the unit's class cannot use (a
    /// tree, a spot in the water): within reach of the followed box, or
    /// where the path search would accept an open cell for a blocked point.
    fn near(&self) -> f32 {
        match self.follow {
            Some(b) => b.bounding_radius() + self.follow_reach(),
            None => GOAL_SEARCH_RADIUS.max(self.goal.arrive),
        }
    }

    /// The same request heading for `aim` from `from`.
    fn toward(&self, aim: &Aim, from: Vec2) -> Ask<'_> {
        Ask { goal: self.goal, mob: self.mob, pos: from, team: self.team, target: aim.goal, follow: self.follow.filter(|_| aim.reach) }
    }

    /// The grid request (registers the mover's terrain costs on first use).
    fn request(&self, nav: &mut NavGrid, data: &GameData, max_expansions: u32) -> PathRequest {
        if !self.mob.flying {
            register_terrain_costs(nav, data, self.mob.mobility);
        }
        PathRequest {
            start: self.pos,
            goal: self.target,
            mobility: self.mob.mobility.0,
            radius: self.mob.radius,
            team: self.team,
            max_expansions,
            reach: self.follow.map(|b| (b, self.follow_reach())),
        }
    }
}

/// The region map of a ground mover's class (none for flyers, or without
/// `NavRegions`).
fn class_map(regions: Option<&NavRegions>, mob: &Mobile) -> Option<Arc<RegionMap>> {
    if mob.flying {
        return None;
    }
    regions?.map(mob.mobility, mob.radius)
}

/// Where the way to `ask`'s goal ends for the unit (at `ask.pos`): the goal
/// itself, unless the regions say it is out of the unit's reach; then the
/// reachable point nearest it, found without a search.
fn way_end(map: Option<&RegionMap>, ask: &Ask) -> Aim {
    let to_goal = Aim { goal: ask.target, reach: ask.follow.is_some(), last: true, unreachable: false, beyond: 0.0, extends: false };
    let Some(map) = map else { return to_goal };
    let from = map.region_near(ask.pos, START_SEARCH_RADIUS);
    if from == 0 {
        // Nowhere open about: the search sorts it out.
        return to_goal;
    }
    let reachable = match map.region_at(ask.target) {
        0 => map.touches(from, ask.target, ask.near()),
        r => r == from,
    };
    if reachable {
        return to_goal;
    }
    let goal = map.nearest_in(from, ask.target).unwrap_or(ask.pos);
    Aim { goal, reach: false, unreachable: true, ..to_goal }
}

/// What a search from `from` on the way to `last` (where the way ends)
/// heads for: `last` itself on a short way, or when the coarse route is
/// straight (obstacles are the search's business); else the end of the
/// next leg of the route (see [`RegionMap::next_leg`]). Plans the route,
/// or takes the unit's own or a groupmate's, and charges a coarse search
/// to `spent`.
fn leg_aim(queue: &mut PathQueue, map: &RegionMap, mover: &mut Mover, from: Vec2, last: Aim, spent: &mut u32) -> Aim {
    if from.distance(last.goal) <= LONG_WAY {
        return last;
    }
    let Some(route) = route_for(queue, map, mover, from, last.goal, spent) else { return last };
    if route.points.len() <= 2 || route.length() <= LEG_LENGTH * 1.5 {
        return last;
    }
    let leg = map.next_leg(&route, from, mover.route_segment, LEG_LENGTH);
    mover.route_segment = leg.segment;
    if leg.last {
        return last;
    }
    Aim { goal: leg.target, reach: false, last: false, unreachable: false, beyond: leg.beyond, extends: false }
}

/// The coarse route from `from` to `to`: the unit's own while it still
/// leads there and the unit is near it, else a groupmate's planned lately,
/// else a new one (its coarse search charged to `spent`). `None` when the
/// ends have no open ground about or lie apart (`way_end` keeps the latter
/// from happening).
fn route_for(queue: &mut PathQueue, map: &RegionMap, mover: &mut Mover, from: Vec2, to: Vec2, spent: &mut u32) -> Option<Arc<CoarseRoute>> {
    if let Some(route) = mover.route.as_ref().filter(|r| mover.route_stamp == map.stamp() && r.end().distance(to) <= COARSE_SHARE_GOAL)
        && route.locate(from, mover.route_segment).2 <= OFF_ROUTE
    {
        return Some(route.clone());
    }
    let route = match queue.shared_route(map, from, to) {
        Some(r) => r,
        None => {
            let (a, b) = (map.open_near(from, START_SEARCH_RADIUS)?, map.open_near(to, GOAL_SEARCH_RADIUS)?);
            let planned = map.route(a, b)?;
            *spent += planned.expansions * COARSE_EXPANSION_COST;
            queue.coarse_expanded += u64::from(planned.expansions);
            let r = Arc::new(planned);
            queue.remember_route(map, from, to, r.clone());
            r
        }
    };
    mover.route = Some(route.clone());
    mover.route_stamp = map.stamp();
    mover.route_segment = 0;
    Some(route)
}

type RequestQuery<'a> = (Entity, &'a Position, &'a Facing, &'a Mobile, &'a Owner, Option<&'a MoveGoal>, &'a mut PathState, &'a mut Mover);

/// Turn goal changes and repath wishes into paths, on the spot when no
/// search is needed, else through the queue; ask for the next leg of a long
/// way in time; drop state of units whose goal was removed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_requests(
    mut queue: ResMut<PathQueue>,
    mut nav: ResMut<Nav>,
    data: Res<Data>,
    players: Res<Players>,
    spatial: Res<SpatialIndex>,
    regions: Option<Res<NavRegions>>,
    mut avoid: Local<Vec<IVec2>>,
    mut units: Query<RequestQuery, Without<Dead>>,
) {
    // Only path caches change here: not a grid change for anyone watching `Nav`.
    let nav = nav.bypass_change_detection();
    queue.spent = 0;
    queue.follow_grid(nav);
    for (e, pos, facing, mob, owner, goal, mut path, mut mover) in &mut units {
        let Some(goal) = goal else {
            if path.status != PathStatus::Idle || path.goal_seq != 0 || !path.waypoints.is_empty() {
                // Seq 0 = no goal, so re-inserting the same MoveGoal paths again.
                *path = PathState::default();
                mover.wants_repath = false;
            }
            continue;
        };
        let new_goal = goal.seq != path.goal_seq;
        if new_goal {
            *path = PathState { goal_seq: goal.seq, status: PathStatus::Pending, ..PathState::default() };
            mover.reset_goal(pos.0, facing.0);
        } else {
            match path.status {
                PathStatus::Following => {
                    // Flyers steer at the live goal; only ground paths go stale.
                    let target = goal_point(goal, mover.follow.as_ref());
                    let moved_target = !mob.flying && target.is_some_and(|p| p.distance(mover.path_goal) > FOLLOW_REPATH_DISTANCE);
                    if moved_target && mover.repath_cooldown <= 0.0 {
                        mover.wants_repath = true;
                    }
                    if !mover.wants_repath {
                        // Near the end of a leg: ask for the next one.
                        if mover.on_leg && !mover.extend && !mover.queued && mover.way_left(pos.0, &path) - mover.beyond < LEG_AHEAD {
                            mover.extend = true;
                            queue.push(e, owner.0, &mut mover);
                        }
                        continue;
                    }
                }
                PathStatus::Pending => {}
                PathStatus::Idle | PathStatus::Arrived | PathStatus::Failed => continue,
            }
            if mover.queued {
                // Already waiting in line.
                continue;
            }
        }
        if let Some(target) = goal_point(goal, mover.follow.as_ref()) {
            let follow = goal.follow.and(mover.follow).map(|f| f.obb);
            let to_goal = Ask { goal, mob, pos: pos.0, team: players.team(owner.0), target, follow };
            let map = class_map(regions.as_deref(), mob);
            let last = way_end(map.as_deref(), &to_goal);
            (mover.way_end, mover.unreachable) = (last.goal, last.unreachable);
            let ask = to_goal.toward(&last, pos.0);
            let req = ask.request(nav, &data, 0);
            avoid.clear();
            if mover.stuck_repath {
                movers_in_the_way(&spatial, nav, e, pos.0, mob.radius, goal.follow, &mut avoid);
            }
            if let Some(result) = nav.quick_path(&req, &avoid) {
                apply_path(&ask, &last, target, result, &mut path, &mut mover);
                continue;
            }
            // Offer it a recent route while this tick's share of the budget
            // lasts; past that it waits in line, where it is offered again.
            if avoid.is_empty() && queue.spent < SHARE_ON_ARRIVAL_BUDGET {
                let mut checks = 0;
                let shared = queue.share(nav, &ask, &mut checks);
                queue.spent += checks;
                if let Some(result) = shared {
                    queue.spent += REQUEST_OVERHEAD;
                    queue.remember(nav, &ask, &result.waypoints);
                    apply_path(&ask, &last, target, result, &mut path, &mut mover);
                    continue;
                }
            }
        }
        queue.push(e, owner.0, &mut mover);
    }
}

type ServiceQuery<'a> = (&'a Position, &'a Mobile, &'a Owner, Option<&'a MoveGoal>, &'a mut PathState, &'a mut Mover);

/// Serve queued requests, the players' lines in turn, until this tick's
/// budget is spent.
#[allow(clippy::too_many_arguments)]
pub(crate) fn service_paths(
    mut queue: ResMut<PathQueue>,
    mut nav: ResMut<Nav>,
    data: Res<Data>,
    players: Res<Players>,
    spatial: Res<SpatialIndex>,
    regions: Option<Res<NavRegions>>,
    mut avoid: Local<Vec<IVec2>>,
    mut units: Query<ServiceQuery, Without<Dead>>,
) {
    // Only path caches change here: not a grid change for anyone watching `Nav`.
    let nav = nav.bypass_change_detection();
    // What offering new requests a route already spent of the tick.
    let mut spent = queue.spent;
    while TICK_EXPANSION_BUDGET.saturating_sub(spent) >= MIN_SEARCH_BUDGET {
        let Some(e) = queue.pop() else { break };
        let Ok((pos, mob, owner, goal, mut path, mut mover)) = units.get_mut(e) else { continue };
        mover.queued = false;
        let Some(goal) = goal else { continue };
        // Skip entries answered meanwhile (a newer goal got a quick path) or
        // of units no longer going anywhere.
        let full = path.status == PathStatus::Pending || (path.status == PathStatus::Following && mover.wants_repath);
        let extension = !full && path.status == PathStatus::Following && mover.extend && mover.on_leg;
        if !full && !extension {
            continue;
        }
        let Some(target) = goal_point(goal, mover.follow.as_ref()) else {
            path.status = PathStatus::Failed;
            continue;
        };
        let follow = goal.follow.and(mover.follow).map(|f| f.obb);
        let to_goal = Ask { goal, mob, pos: pos.0, team: players.team(owner.0), target, follow };
        let map = class_map(regions.as_deref(), mob);
        // A search set aside last tick goes on with the aim it had.
        let resumed = mover.aim.filter(|a| a.extends == extension);
        let from = if extension { path.waypoints.last().copied().unwrap_or(pos.0) } else { pos.0 };
        let aim = match resumed {
            Some(aim) => aim,
            None => {
                let last = if extension {
                    Aim { goal: mover.way_end, reach: follow.is_some() && !mover.unreachable, last: true, unreachable: mover.unreachable, beyond: 0.0, extends: false }
                } else {
                    let last = way_end(map.as_deref(), &to_goal);
                    (mover.way_end, mover.unreachable) = (last.goal, last.unreachable);
                    last
                };
                let aim = match map.as_deref() {
                    Some(map) if !mob.flying => leg_aim(&mut queue, map, &mut mover, from, last, &mut spent),
                    _ => last,
                };
                mover.searches += 1;
                Aim { extends: extension, ..aim }
            }
        };
        let ask = to_goal.toward(&aim, from);
        let req = ask.request(nav, &data, MAX_EXPANSIONS_PER_PATH);
        avoid.clear();
        if mover.stuck_repath && !extension {
            movers_in_the_way(&spatial, nav, e, pos.0, mob.radius, goal.follow, &mut avoid);
        } else if !mob.flying && resumed.is_none() {
            // A leg along open ground needs no search (the goal itself was
            // tried when the request came in) ...
            if !aim.last || aim.extends {
                spent += line_check_cost(nav, from, aim.goal);
                if let Some(result) = nav.quick_path(&req, &[]) {
                    spent += REQUEST_OVERHEAD;
                    apply_path(&ask, &aim, target, result, &mut path, &mut mover);
                    continue;
                }
            }
            // ... and a groupmate served earlier in the line may have found the way.
            nav.prepare(mob.mobility.0, ask.team);
            let mut checks = 0;
            let shared = queue.share(nav, &ask, &mut checks);
            spent += checks;
            if let Some(result) = shared {
                spent += REQUEST_OVERHEAD;
                queue.remember(nav, &ask, &result.waypoints);
                apply_path(&ask, &aim, target, result, &mut path, &mut mover);
                continue;
            }
        }
        let budget = TICK_EXPANSION_BUDGET.saturating_sub(spent);
        if budget < MIN_SEARCH_BUDGET {
            // Planning and the look for a route to share spent the tick:
            // first in line next tick, with this aim.
            mover.aim = Some(aim);
            mover.queued = true;
            queue.resume = Some(e);
            break;
        }
        match nav.find_path_sliced(&req, budget, search_token(e, goal.seq, mover.searches), &avoid) {
            PathStep::Done { result, spent: used } => {
                spent += used + REQUEST_OVERHEAD;
                queue.expanded += u64::from(used);
                if result.complete && !result.truncated && avoid.is_empty() {
                    queue.remember(nav, &ask, &result.waypoints);
                }
                apply_path(&ask, &aim, target, result, &mut path, &mut mover);
            }
            PathStep::Paused { spent: used } => {
                queue.expanded += u64::from(used);
                // Out of this tick: first in line next tick, where it goes on.
                mover.aim = Some(aim);
                mover.queued = true;
                queue.resume = Some(e);
                break;
            }
        }
    }
}

/// The cells other ground units near `me` stand on, for a mover that got
/// stuck to go round (`out`, cleared first): a
/// worker held up behind one at work in a gap, or a column in a crowd that
/// blocks its way, takes another way if there is one (see
/// `NavGrid::find_path_sliced`). The followed entity is not in the way.
fn movers_in_the_way(spatial: &SpatialIndex, nav: &NavGrid, me: Entity, pos: Vec2, radius: f32, follow: Option<Entity>, out: &mut Vec<IVec2>) {
    out.clear();
    spatial.for_each_near(pos, AVOID_RADIUS, |e| {
        if e.kind == SpatialKind::Unit && !e.flying && e.entity != me && Some(e.entity) != follow {
            // Every cell where the mover's centre would bring its box against it.
            out.extend(nav.cells_covered(&e.obb, radius + AVOID_MARGIN));
        }
    });
}

/// Names a mover's search for its current goal (and which search for it
/// this is: each leg is a search of its own), so a search the path service
/// set aside is only taken up again for that same request.
fn search_token(e: Entity, seq: u32, search: u32) -> u64 {
    e.to_bits().wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ u64::from(seq) ^ (u64::from(search) << 32)
}

/// Install a computed path for `aim` (on the way to the goal at `real`), or
/// add the next leg to the path.
fn apply_path(ask: &Ask, aim: &Aim, real: Vec2, result: af_geom::PathResult, path: &mut PathState, mover: &mut Mover) {
    mover.aim = None;
    if aim.extends {
        return extend_path(ask, aim, real, result, path, mover);
    }
    mover.wants_repath = false;
    mover.extend = false;
    mover.repath_cooldown = REPATH_COOLDOWN;
    // A path taken because the unit was stuck, or on along the same way,
    // goes on with the progress count: the same route again must not reset
    // it.
    let stuck = std::mem::take(&mut mover.stuck_repath);
    let continuing = std::mem::take(&mut mover.continuing);
    if !continuing && !stuck {
        mover.restart_progress();
    }
    let Some(&end) = result.waypoints.last() else {
        // Nowhere to stand near the start: a repath keeps the old route.
        if path.status == PathStatus::Pending {
            path.status = PathStatus::Failed;
        }
        return;
    };
    if !gains(ask, aim, &result, end, mover) {
        path.status = PathStatus::Failed;
        return;
    }
    // On along the same way, the next leg's path takes the place of what
    // the route said was left from the end of the last (and of the stretch
    // that leg fell short by): see `Mover::rebase_progress`.
    let before = continuing.then(|| mover.way_left(ask.pos, path));
    path.waypoints = result.waypoints;
    path.next = 0;
    path.partial = partial(ask, aim, real, result.complete, end);
    path.status = PathStatus::Following;
    mover.truncated = result.truncated || !aim.last;
    mover.on_leg = !aim.last;
    mover.beyond = aim.beyond;
    mover.path_start = ask.pos;
    mover.path_goal = real;
    if let Some(before) = before {
        let after = mover.way_left(ask.pos, path);
        mover.rebase_progress(after - before);
    }
}

/// Add the next leg (searched from the end of the path) to the path. A leg
/// that gets nowhere is dropped: the unit walks to the end of its path and
/// asks afresh from there. The leg's path takes the place of the route's
/// stretch it covers in the way left (see `Mover::rebase_progress`).
fn extend_path(ask: &Ask, aim: &Aim, real: Vec2, result: af_geom::PathResult, path: &mut PathState, mover: &mut Mover) {
    mover.extend = false;
    let Some(&end) = result.waypoints.last() else {
        mover.on_leg = false;
        return;
    };
    if !gains(ask, aim, &result, end, mover) {
        mover.on_leg = false;
        return;
    }
    let joined = path.waypoints.len();
    path.waypoints.extend(result.waypoints.iter().copied().filter(|w| w.distance(ask.pos) > 1e-3));
    let added: f32 = path.waypoints[joined..]
        .iter()
        .scan(ask.pos, |at, &w| {
            let step = at.distance(w);
            *at = w;
            Some(step)
        })
        .sum();
    mover.rebase_progress(added + aim.beyond - mover.beyond);
    path.partial = partial(ask, aim, real, result.complete, end);
    mover.truncated = result.truncated || !aim.last;
    mover.on_leg = !aim.last;
    mover.beyond = aim.beyond;
}

/// Is a path ending at `end` worth taking? Always when it got to its aim;
/// one that falls short (cut by the search budget, or a leg that ends
/// short of the leg's end) only when the way left from its end is
/// [`TRUNCATED_MIN_GAIN`] shorter than from the last such path's: a unit
/// does not walk round in circles for ever.
fn gains(_ask: &Ask, aim: &Aim, result: &af_geom::PathResult, end: Vec2, mover: &mut Mover) -> bool {
    let short = result.truncated || (!aim.last && end.distance(aim.goal) > LEG_REACHED);
    if !short {
        return true;
    }
    let remaining = end.distance(aim.goal) + aim.beyond;
    if remaining > mover.best_remaining - TRUNCATED_MIN_GAIN {
        return false;
    }
    mover.best_remaining = remaining;
    true
}

/// Does a path for `aim` ending at `end` stop short of what the goal (at
/// `real`) asks? Every leg but the last does; the last when the unit cannot
/// fulfil the goal from its end: it is too far from the followed box, or
/// from the goal point (the reachable point nearest a goal out of the
/// unit's region is not the goal).
fn partial(ask: &Ask, aim: &Aim, real: Vec2, complete: bool, end: Vec2) -> bool {
    if !aim.last {
        return true;
    }
    let reach = match ask.follow {
        Some(b) => b.distance_to_point(end) <= ask.follow_reach() + 1e-3,
        None => end.distance(real) <= ask.goal.arrive + 0.05,
    };
    !reach && (!complete || aim.unreachable)
}

/// Register A* step costs (1 / terrain speed) for a mobility, once per grid.
fn register_terrain_costs(nav: &mut NavGrid, data: &GameData, mobility: Mobility) {
    if nav.has_terrain_costs(mobility.0) {
        return;
    }
    for t in data.terrain_ids() {
        let speed = data.terrain_speed(t, mobility);
        nav.set_terrain_cost(t.0, mobility.0, if speed > 1e-3 { 1.0 / speed } else { 1.0 });
    }
}

/// Keep `NavRegions` for the map's terrain: (re)made whenever the grid's
/// terrain stamp moves on (a new map). On entering a match it also builds
/// the region maps of the start units' classes, so the first ticks do not
/// pay for them.
pub(crate) fn sync_regions(mut commands: Commands, nav: Option<Res<Nav>>, regions: Option<Res<NavRegions>>) {
    let Some(nav) = nav else { return };
    if regions.is_none_or(|r| r.stamp() != nav.terrain_stamp()) {
        commands.insert_resource(NavRegions::new(&nav.0));
    }
}

/// `OnEnter(GameState::Playing)`, after af_map: regions for the new map,
/// with the start units' classes built.
pub(crate) fn prepare_regions(mut commands: Commands, nav: Option<Res<Nav>>, units: Query<&Mobile>) {
    let Some(nav) = nav else { return };
    let regions = NavRegions::new(&nav.0);
    for mob in &units {
        if !mob.flying {
            regions.map(mob.mobility, mob.radius);
        }
    }
    commands.insert_resource(regions);
}

/// `OnExit(GameState::Playing)`: the map's regions go with it.
pub(crate) fn drop_regions(mut commands: Commands) {
    commands.remove_resource::<NavRegions>();
}

/// `OnEnter` and `OnExit(GameState::Playing)`: the path queue starts over
/// with every match (see [`PathQueue::start_over`]).
pub(crate) fn reset_queue(mut queue: ResMut<PathQueue>) {
    queue.start_over();
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};

    fn status(app: &App, e: Entity) -> PathStatus {
        app.world().get::<PathState>(e).expect("path").status
    }

    fn queued(app: &App) -> usize {
        app.world().resource::<PathQueue>().len()
    }

    /// Every unit follows a path that leads all the way to its goal.
    fn assert_full_paths(app: &App, units: &[Entity]) {
        for &e in units {
            let path = app.world().get::<PathState>(e).expect("path");
            assert!(path.status == PathStatus::Following && !path.partial, "{e}: {:?}, partial {}, {:?}", path.status, path.partial, path.waypoints.last());
        }
    }

    /// A mass order is spread over several ticks under the budget, in order,
    /// and every unit gets a path to its goal (searches cut by the end of a
    /// tick are run again with the full budget, never handed back cut).
    #[test]
    fn budget_spreads_a_mass_order_and_serves_everyone() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 128.0, &[("ironhold", 0)]);
        // A long wall so every request needs a real search; distinct goal
        // cells so no search can be shared.
        let wall = data.building_ids().find(|b| data.building(*b).wall && !data.building(*b).gate).expect("a wall");
        for i in 0..40 {
            let at = Vec2::new(64.0, 4.0 + i as f32 * 3.0);
            with_commands(&mut app, |c, data, players| spawn_building(c, data, players.get(p[0]), wall, at, std::f32::consts::FRAC_PI_2, true));
        }
        let foot = data.unit_ids().find(|u| data.unit(*u).locomotion == MoveFlag::Foot).expect("a foot unit");
        let mut units = Vec::new();
        for i in 0..240 {
            let at = Vec2::new(10.0 + (i % 20) as f32 * 1.5, 10.0 + (i / 20) as f32 * 2.0);
            let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), foot, at, 0.0));
            app.world_mut().entity_mut(e).insert(MoveGoal::point(Vec2::new(100.0 + (i % 12) as f32 * 2.0, 20.0 + (i / 12) as f32 * 3.0), 0.5));
            units.push(e);
        }
        run_ticks(&mut app, 1);
        assert!(queued(&app) > 0, "not everything fits in one tick");
        // FIFO: whoever is still waiting was queued after everyone served.
        let first_waiting = units.iter().position(|e| status(&app, *e) == PathStatus::Pending).expect("someone waits");
        assert!(units[first_waiting..].iter().all(|e| status(&app, *e) == PathStatus::Pending));
        for _ in 0..300 {
            run_ticks(&mut app, 1);
            if queued(&app) == 0 {
                break;
            }
        }
        assert_eq!(queued(&app), 0);
        assert_full_paths(&app, &units);
    }

    /// Offering new requests routes to share is charged to the tick's search
    /// budget: a mass order past a long wall, given right after one unit
    /// found the way, is answered from that unit's route without searching,
    /// at most half the budget's worth of it on arrival and the rest from
    /// the queue under what is left; everyone gets a full path.
    #[test]
    fn sharing_routes_on_arrival_is_budgeted() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 128.0, &[("ironhold", 0)]);
        let wall = data.building_ids().find(|b| data.building(*b).wall && !data.building(*b).gate).expect("a wall");
        for i in 0..40 {
            let at = Vec2::new(64.0, 4.0 + i as f32 * 3.0);
            with_commands(&mut app, |c, data, players| spawn_building(c, data, players.get(p[0]), wall, at, std::f32::consts::FRAC_PI_2, true));
        }
        let foot = data.unit_ids().find(|u| data.unit(*u).locomotion == MoveFlag::Foot).expect("a foot unit");
        let (start, goal) = (Vec2::new(20.0, 60.0), Vec2::new(100.0, 60.0));
        let scout = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), foot, start + Vec2::new(0.0, -6.0), 0.0));
        app.world_mut().entity_mut(scout).insert(MoveGoal::point(goal, 0.5));
        for _ in 0..30 {
            run_ticks(&mut app, 1);
            if status(&app, scout) == PathStatus::Following {
                break;
            }
        }
        assert_full_paths(&app, &[scout]);
        let units: Vec<Entity> = (0..100)
            .map(|i| {
                let at = start + Vec2::new((i % 10) as f32 - 4.5, (i / 10) as f32 - 4.5) * 0.9;
                let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), foot, at, 0.0));
                app.world_mut().entity_mut(e).insert(MoveGoal::point(goal + Vec2::new((i % 10) as f32, (i / 10) as f32) * 0.5, 0.5));
                e
            })
            .collect();
        run_ticks(&mut app, 1);
        let spent = app.world().resource::<PathQueue>().spent;
        let answered = units.iter().filter(|e| status(&app, **e) == PathStatus::Following).count();
        assert!(answered > 10 && queued(&app) > 10, "some shared on arrival ({answered}), the rest queued ({})", queued(&app));
        assert!((SHARE_ON_ARRIVAL_BUDGET..SHARE_ON_ARRIVAL_BUDGET + 500).contains(&spent), "{spent}");
        for _ in 0..60 {
            run_ticks(&mut app, 1);
            if queued(&app) == 0 {
                break;
            }
        }
        assert_eq!(queued(&app), 0);
        assert_full_paths(&app, &units);
    }

    /// Time the path service of a forest maze, where every request needs a
    /// search, serving the requests of units walking from `starts` to
    /// `goals`: (ticks to serve everyone, mean, p90 and worst ms per tick).
    /// Everyone gets a path to their goal.
    fn serve_through_a_forest(starts: &[Vec2], goals: &[Vec2]) -> (usize, f64, f64, f64) {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 160.0, &[("ironhold", 0)]);
        let tree = data.node_ids().filter(|n| data.node(*n).blocking).min_by(|a, b| data.node(*a).size[0].total_cmp(&data.node(*b).size[0])).expect("a tree");
        // Four tree lines across the map, each with one gap, alternating ends.
        let mut trees = 0;
        for (x, gap) in [(50, 20), (62, 140), (74, 30), (86, 130)] {
            for y in (0..160).filter(|y| !(gap..gap + 4).contains(y)) {
                let at = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                with_commands(&mut app, |c, data, _| spawn_node(c, data, tree, at, 0.0, None));
                trees += 1;
            }
        }
        assert!(trees > 600);
        let foot = data.unit_ids().find(|u| data.unit(*u).locomotion == MoveFlag::Foot).expect("a foot unit");
        let units: Vec<Entity> = starts
            .iter()
            .zip(goals)
            .map(|(&at, &goal)| {
                let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), foot, at, 0.0));
                app.world_mut().entity_mut(e).insert(MoveGoal::point(goal, 0.5));
                e
            })
            .collect();
        run_ticks(&mut app, 1);
        assert!(queued(&app) + 10 > units.len(), "the forest needs searches ({} of {} queued)", queued(&app), units.len());

        // Serve the queue tick by tick, timing the service alone.
        let mut service = Schedule::default();
        service.add_systems(service_paths);
        let mut ticks: Vec<f64> = Vec::new();
        while queued(&app) > 0 && ticks.len() < 500 {
            let t = Instant::now();
            service.run(app.world_mut());
            ticks.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        assert_eq!(queued(&app), 0);
        assert_full_paths(&app, &units);
        let runs = ticks.len();
        let mean = ticks.iter().sum::<f64>() / runs as f64;
        ticks.sort_by(f64::total_cmp);
        // The 90th percentile, not the very worst tick: a test machine busy
        // with other work preempts a tick now and then.
        (runs, mean, ticks[runs * 9 / 10], ticks[runs - 1])
    }

    /// A mass order through the forest: 100 units in a block sent to a
    /// formation beyond it share a few searches' routes, so the order is
    /// served within a second (161 ticks when each unit searched), and no
    /// tick of the service runs over its time budget.
    #[test]
    fn path_service_through_a_forest_stays_within_budget() {
        let starts: Vec<Vec2> = (0..100).map(|i| Vec2::new(10.0 + (i % 10) as f32 * 3.0, 40.0 + (i / 10) as f32 * 8.0)).collect();
        let goals: Vec<Vec2> = (0..100).map(|i| Vec2::new(110.0 + (i % 10) as f32 * 4.0, 20.0 + (i / 10) as f32 * 12.0)).collect();
        let (runs, mean, p90, worst) = serve_through_a_forest(&starts, &goals);
        println!("path service through a forest: {runs} ticks for a mass order of 100, {mean:.2} ms per tick, p90 {p90:.2} ms, worst {worst:.2} ms");
        assert!(runs <= 30, "groupmates share routes ({runs} ticks)");
        assert!(mean < 4.0 && p90 < 6.0, "{mean:.2} ms per tick of the path service, p90 {p90:.2} ms");
    }

    /// Requests that cannot share (every goal far from every other) are
    /// spread over ticks by the budget, each tick within its time.
    #[test]
    fn scattered_searches_are_spread_over_ticks() {
        let starts: Vec<Vec2> = (0..60).map(|i| Vec2::new(10.0 + (i % 6) as f32 * 4.0, 10.0 + (i / 6) as f32 * 14.0)).collect();
        let goals: Vec<Vec2> = (0..60).map(|i| Vec2::new(95.0 + (i % 6) as f32 * 12.0, 8.0 + (i / 6) as f32 * 16.0)).collect();
        let (runs, mean, p90, worst) = serve_through_a_forest(&starts, &goals);
        println!("path service through a forest: {runs} ticks for 60 scattered searches, {mean:.2} ms per tick, p90 {p90:.2} ms, worst {worst:.2} ms");
        assert!(runs > 20, "the budget spread the searches over ticks ({runs})");
        assert!(mean < 4.0 && p90 < 6.0, "{mean:.2} ms per tick of the path service, p90 {p90:.2} ms");
    }

    /// A 64 m grass grid with a wall along x = 32 open only at y 4..8.
    fn walled_grid() -> NavGrid {
        let data = test_data();
        let mut nav = af_core::testing::new_nav(&data, 64, 64, data.terrain_id("grass").expect("grass"));
        nav.add_obstacle(&Obb::new(Vec2::new(32.5, 36.0), Vec2::new(0.5, 28.0), 0.0), 1);
        nav
    }

    fn walker() -> Mobile {
        Mobile { mobility: Mobility::of(MoveFlag::Foot), radius: 0.3, turn_rate: 6.0, flying: false }
    }

    /// Every leg of `waypoints` from `start` is walkable.
    fn walkable(nav: &NavGrid, start: Vec2, waypoints: &[Vec2], mob: &Mobile) -> bool {
        let mut from = start;
        waypoints.iter().all(|&to| {
            let clear = nav.line_clear(from, to, mob.mobility.0, mob.radius, 0);
            from = to;
            clear
        })
    }

    /// A route found through the gap is shared with a groupmate starting
    /// near it and heading near its goal: a walkable path through the same
    /// gap, ending on the groupmate's own goal. Not with a mover starting
    /// or heading too far off, of another team or size, walking up to an
    /// entity, or once the grid has changed.
    #[test]
    fn groupmates_share_a_route_through_a_gap() {
        let mut nav = walled_grid();
        let mob = walker();
        nav.prepare(mob.mobility.0, 0);
        let (start, goal) = (Vec2::new(20.0, 30.0), Vec2::new(44.0, 30.0));
        let found = nav.find_path(&PathRequest { start, goal, mobility: mob.mobility.0, radius: mob.radius, team: 0, max_expansions: 50_000, reach: None });
        assert!(found.complete && found.waypoints.len() > 1, "{found:?}");
        let mut queue = PathQueue::default();
        let order = MoveGoal::point(goal, 0.5);
        queue.remember(&nav, &Ask { goal: &order, mob: &mob, pos: start, team: 0, target: goal, follow: None }, &found.waypoints);

        let ask = |pos: Vec2, target: Vec2, team: u8, mob: &Mobile| {
            let order = MoveGoal::point(target, 0.5);
            let ask = Ask { goal: &order, mob, pos, team, target, follow: None };
            queue.share(&nav, &ask, &mut 0)
        };
        let (mate, spot) = (Vec2::new(18.5, 33.0), Vec2::new(47.0, 27.0));
        let shared = ask(mate, spot, 0, &mob).expect("shared");
        assert!(shared.complete && shared.waypoints.last() == Some(&spot), "{shared:?}");
        assert!(walkable(&nav, mate, &shared.waypoints, &mob), "{shared:?}");
        assert!(shared.waypoints.iter().any(|w| w.x > 31.0 && w.x < 34.0 && w.y < 9.0), "through the gap: {shared:?}");

        assert!(ask(Vec2::new(10.0, 30.0), spot, 0, &mob).is_none(), "started too far off");
        assert!(ask(mate, Vec2::new(44.0, 50.0), 0, &mob).is_none(), "heading too far off");
        assert!(ask(mate, spot, 1, &mob).is_none(), "another team");
        let rider = Mobile { radius: 1.2, ..mob };
        assert!(ask(mate, spot, 0, &rider).is_none(), "another clearance");
        let target = Obb::new(spot, Vec2::splat(1.0), 0.0);
        let to_entity = Ask { goal: &order, mob: &mob, pos: mate, team: 0, target: spot, follow: Some(target) };
        assert!(queue.share(&nav, &to_entity, &mut 0).is_none(), "walking up to an entity");
        nav.add_obstacle(&Obb::new(Vec2::new(5.0, 5.0), Vec2::splat(1.0), 0.0), 1);
        nav.prepare(mob.mobility.0, 0);
        assert!(queue.share(&nav, &Ask { goal: &order, mob: &mob, pos: mate, team: 0, target: spot, follow: None }, &mut 0).is_none(), "the grid changed");
    }

    /// A shared route is left at its last turn when the goal is in sight
    /// from there, else at its end, and refused when neither sees the goal.
    #[test]
    fn a_shared_route_ends_on_the_goal_only_in_sight_of_it() {
        let mut nav = walled_grid();
        let mob = walker();
        // A short wall east of the gap, between the route's end and a spot behind it.
        nav.add_obstacle(&Obb::new(Vec2::new(46.0, 20.0), Vec2::new(4.0, 0.5), 0.0), 1);
        nav.prepare(mob.mobility.0, 0);
        let (start, goal) = (Vec2::new(20.0, 30.0), Vec2::new(44.0, 24.0));
        let found = nav.find_path(&PathRequest { start, goal, mobility: mob.mobility.0, radius: mob.radius, team: 0, max_expansions: 50_000, reach: None });
        assert!(found.complete, "{found:?}");
        let mut queue = PathQueue::default();
        let order = MoveGoal::point(goal, 0.5);
        queue.remember(&nav, &Ask { goal: &order, mob: &mob, pos: start, team: 0, target: goal, follow: None }, &found.waypoints);
        let share = |target: Vec2| queue.share(&nav, &Ask { goal: &order, mob: &mob, pos: start, team: 0, target, follow: None }, &mut 0);
        let near = share(Vec2::new(41.0, 26.0)).expect("in sight of the last turn");
        assert!(walkable(&nav, start, &near.waypoints, &mob), "{near:?}");
        assert!(share(Vec2::new(46.0, 17.0)).is_none(), "behind the wall from both the last turn and the end");
    }

    /// A route belongs to the grid it was found on. The next map's grid
    /// counts its versions from 1 again, so it passes through the version
    /// numbers of the last map's routes: a route found through the gap of
    /// the last map is not shared on a map walled off all along, although
    /// the version, the mover class and the start and goal all match (the
    /// leg through the old gap, never checked again, crosses the new wall).
    #[test]
    fn routes_of_another_map_are_never_shared() {
        let data = test_data();
        let mut nav = walled_grid();
        nav.set_sync_tag(1);
        let mob = walker();
        nav.prepare(mob.mobility.0, 0);
        let (start, goal) = (Vec2::new(20.0, 30.0), Vec2::new(44.0, 30.0));
        let found = nav.find_path(&PathRequest { start, goal, mobility: mob.mobility.0, radius: mob.radius, team: 0, max_expansions: 50_000, reach: None });
        assert!(found.complete, "{found:?}");
        let mut queue = PathQueue::default();
        let order = MoveGoal::point(goal, 0.5);
        let ask = Ask { goal: &order, mob: &mob, pos: start, team: 0, target: goal, follow: None };
        queue.remember(&nav, &ask, &found.waypoints);

        // The next match's map: as big, walled off along x = 32, as many changes in.
        let grass = data.terrain_id("grass").expect("grass");
        let mut next = af_core::testing::new_nav(&data, 64, 64, grass);
        next.add_obstacle(&Obb::new(Vec2::new(32.5, 32.0), Vec2::new(0.5, 32.0), 0.0), 1);
        while next.version() < nav.version() {
            next.set_terrain(IVec2::ZERO, grass.0);
        }
        assert_eq!(next.version(), nav.version());
        next.set_sync_tag(2);
        next.prepare(mob.mobility.0, 0);
        assert!(!walkable(&next, start, &found.waypoints, &mob), "the old route crosses the new wall");
        assert!(queue.share(&next, &ask, &mut 0).is_none(), "a route of the last map is shared on the next");
        // The check that refuses it is the grid's, not the line checks':
        // stamped as the old grid, the new one is handed the old route.
        next.set_sync_tag(1);
        let stale = queue.share(&next, &ask, &mut 0).expect("shared on what looks like the old grid");
        assert!(!walkable(&next, start, &stale.waypoints, &mob), "{stale:?}");
    }

    /// Played through the plugin: a unit of a new match, on a map walled off
    /// all along, is never handed the route a unit of the last match found
    /// through that map's gap, even once the new grid reaches the version
    /// the route was found at. It searches, and walks up to the wall.
    #[test]
    fn a_new_map_starts_without_the_last_maps_routes() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 128.0, &[("ironhold", 0)]);
        let grass = data.terrain_id("grass").expect("grass");
        let wall = data.building_ids().find(|b| data.building(*b).wall && !data.building(*b).gate).expect("a wall");
        let foot = data.unit_ids().find(|u| data.unit(*u).locomotion == MoveFlag::Foot).expect("a foot unit");
        let (start, goal) = (Vec2::new(20.0, 60.0), Vec2::new(100.0, 60.0));
        let send = |app: &mut App| {
            let e = with_commands(app, |c, data, players| spawn_unit(c, data, players.get(p[0]), foot, start, 0.0));
            app.world_mut().entity_mut(e).insert(MoveGoal::point(goal, 0.5));
            for _ in 0..60 {
                run_ticks(app, 1);
                if status(app, e) != PathStatus::Pending {
                    break;
                }
            }
            e
        };

        // The last match: a wall along x = 64 with a gap at the north end,
        // and some terrain changes, so its grid's version is ahead of the next one's.
        let mut last_map: Vec<Entity> = (0..38)
            .map(|i| {
                let at = Vec2::new(64.0, 4.0 + i as f32 * 3.0);
                with_commands(&mut app, |c, data, players| spawn_building(c, data, players.get(p[0]), wall, at, std::f32::consts::FRAC_PI_2, true))
            })
            .collect();
        run_ticks(&mut app, 1);
        for _ in 0..100 {
            app.world_mut().resource_mut::<Nav>().set_terrain(IVec2::ZERO, grass.0);
        }
        let scout = send(&mut app);
        assert_full_paths(&app, &[scout]);
        let version = app.world().resource::<Nav>().version();
        last_map.push(scout);

        // Torn down as af_map does it: everything despawned, a fresh grid
        // in. The next map is walled off all along x = 64.
        for e in last_map {
            app.world_mut().entity_mut(e).despawn();
        }
        run_ticks(&mut app, 1);
        app.world_mut().insert_resource(Nav(af_core::testing::new_nav(&data, 127, 128, grass)));
        for i in 0..43 {
            let at = Vec2::new(64.0, 1.5 + i as f32 * 3.0);
            with_commands(&mut app, |c, data, players| spawn_building(c, data, players.get(p[0]), wall, at, std::f32::consts::FRAC_PI_2, true));
        }
        run_ticks(&mut app, 1);
        assert!(app.world().resource::<PathQueue>().routes.is_empty(), "the new map starts with no routes, as in a fresh game");
        {
            let mut nav = app.world_mut().resource_mut::<Nav>();
            assert!(nav.version() <= version, "the next grid starts behind the last one");
            while nav.version() < version {
                nav.set_terrain(IVec2::ZERO, grass.0);
            }
        }
        let walker = send(&mut app);
        let path = app.world().get::<PathState>(walker).expect("path").clone();
        let mob = *app.world().get::<Mobile>(walker).expect("mobile");
        let nav = app.world().resource::<Nav>();
        assert!(walkable(nav, start, &path.waypoints, &mob), "a path through the new wall: {:?}", path.waypoints);
        assert!(path.status == PathStatus::Following && path.partial, "up to the wall: {:?}, partial {}", path.status, path.partial);
    }

    // --- big maps: regions, coarse routes and legs -----------------------------

    /// Paint a 12 m river down x = cx - 6 .. cx + 6 over the whole map,
    /// crossed by 7 m bands (centred on y) of the named terrains.
    fn paint_river(app: &mut App, cx: i32, crossings: &[(i32, &str)]) {
        let data = app.world().resource::<Data>().0.clone();
        let id = |name: &str| data.terrain_id(name).unwrap_or_else(|| panic!("no terrain `{name}`")).0;
        let deep = id("deep_water");
        let crossings: Vec<(i32, u8)> = crossings.iter().map(|(y, t)| (*y, id(t))).collect();
        let mut nav = app.world_mut().resource_mut::<Nav>();
        for y in 0..nav.height() as i32 {
            for x in cx - 6..cx + 6 {
                let t = crossings.iter().find(|(c, _)| (y - c).abs() < 4).map_or(deep, |c| c.1);
                nav.set_terrain(IVec2::new(x, y), t);
            }
        }
    }

    fn unit_of(data: &GameData, locomotion: MoveFlag) -> UnitTypeId {
        data.unit_ids()
            .filter(|u| data.unit(*u).locomotion == locomotion)
            .max_by(|a, b| data.unit(*a).speed.total_cmp(&data.unit(*b).speed).then(b.cmp(a)))
            .expect("a unit of that locomotion")
    }

    /// Walk `e` until its path service is done with the goal (or `ticks`
    /// run out); returns its status, the ticks taken and every position it
    /// stood on.
    fn walk(app: &mut App, e: Entity, ticks: u32) -> (PathStatus, u32, Vec<Vec2>) {
        let mut trail = Vec::new();
        for t in 1..=ticks {
            run_ticks(app, 1);
            trail.push(app.world().get::<Position>(e).expect("position").0);
            let s = status(app, e);
            if matches!(s, PathStatus::Arrived | PathStatus::Failed) {
                return (s, t, trail);
            }
        }
        (status(app, e), ticks, trail)
    }

    fn expanded(app: &App) -> (u64, u64) {
        let q = app.world().resource::<PathQueue>();
        (q.expanded, q.coarse_expanded)
    }

    /// The case that stranded units on big maps: a 768 m map, a river
    /// across it and its only ford 300 m off the straight line. A rider
    /// ordered across gets a path at once, walks up the bank, crosses at
    /// the ford and arrives, the next leg always found before it needs it,
    /// for a few hundred cell expansions in all (a single search of the
    /// whole way takes 138,000, and one cut at 24,000 ends on the near bank).
    #[test]
    fn a_unit_crosses_a_river_by_a_ford_far_off_the_line() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 768.0, &[("ironhold", 0)]);
        paint_river(&mut app, 384, &[(684, "ford")]);
        let rider = unit_of(&data, MoveFlag::Mounted);
        let (start, goal) = (Vec2::new(344.5, 384.5), Vec2::new(424.5, 384.5));
        let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), rider, start, 0.0));
        app.world_mut().entity_mut(e).insert(MoveGoal::point(goal, 0.5));
        run_ticks(&mut app, 2);
        let first = app.world().get::<PathState>(e).expect("path").clone();
        assert_eq!(first.status, PathStatus::Following, "a path at once: {first:?}");
        let mut waits = 0;
        let mut ticks = 0;
        let mut trail = Vec::new();
        let (mut done, mut status_now) = (false, PathStatus::Following);
        while !done && ticks < 20_000 {
            let (s, t, mut part) = walk(&mut app, e, 30);
            ticks += t;
            trail.append(&mut part);
            status_now = s;
            done = matches!(s, PathStatus::Arrived | PathStatus::Failed);
            let mover = app.world().get::<Mover>(e).expect("mover");
            waits += u32::from(mover.wants_repath);
        }
        let (cells, coarse) = expanded(&app);
        let walked: f32 = trail.windows(2).map(|w| w[0].distance(w[1])).sum();
        println!("ford 300 m off the line, 768 m map: arrived after {ticks} ticks, {walked:.0} m walked, {cells} cell and {coarse} coarse expansions, stood waiting for a leg {waits} times");
        assert_eq!(status_now, PathStatus::Arrived, "{:?}", app.world().get::<PathState>(e));
        assert!(trail.iter().any(|q| q.y > 676.0 && q.y < 692.0 && (378.0..390.0).contains(&q.x)), "crossed at the ford");
        assert!(walked < 760.0, "about the way round ({walked:.0} m; 690 m by the ford)");
        assert!(cells < 5_000 && coarse < 5_000, "{cells} cell and {coarse} coarse expansions");
        assert_eq!(waits, 0, "the next leg came before the unit needed it");
    }

    /// A goal across a river with no ford costs no search: the unit walks
    /// to the bank facing it and fails there.
    #[test]
    fn a_goal_out_of_reach_costs_no_search() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 256.0, &[("ironhold", 0)]);
        paint_river(&mut app, 128, &[]);
        let foot = unit_of(&data, MoveFlag::Foot);
        let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), foot, Vec2::new(98.5, 128.5), 0.0));
        app.world_mut().entity_mut(e).insert(MoveGoal::point(Vec2::new(170.5, 128.5), 0.5));
        let (s, _, trail) = walk(&mut app, e, 1_200);
        let end = *trail.last().expect("walked");
        assert_eq!(s, PathStatus::Failed);
        assert!(end.x > 119.0 && end.x < 122.5, "on the near bank: {end}");
        assert_eq!(expanded(&app), (0, 0), "no search");
    }

    /// Wheels cross only at fords: feet wade a shallow crossing on the
    /// straight line, a siege engine goes round by the ford instead and
    /// never sets a wheel in the shallows.
    #[test]
    fn wheels_cross_at_the_ford_feet_in_the_shallows() {
        let data = test_data();
        let shallow = data.terrain_id("shallow_water").expect("shallow water");
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 256.0, &[("ironhold", 0)]);
        paint_river(&mut app, 128, &[(128, "shallow_water"), (220, "ford")]);
        let (start, goal) = (Vec2::new(98.5, 128.5), Vec2::new(158.5, 128.5));
        for (locomotion, by_ford) in [(MoveFlag::Foot, false), (MoveFlag::Wheeled, true)] {
            let kind = unit_of(&data, locomotion);
            let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), kind, start, 0.0));
            app.world_mut().entity_mut(e).insert(MoveGoal::point(goal, 0.5));
            let (s, _, trail) = walk(&mut app, e, 12_000);
            assert_eq!(s, PathStatus::Arrived, "{locomotion:?}");
            assert_eq!(trail.iter().any(|q| q.y > 200.0), by_ford, "{locomotion:?} by the ford");
            let nav = app.world().resource::<Nav>();
            if by_ford {
                assert!(trail.iter().all(|q| TerrainId(nav.terrain_at(nav.cell_of(*q))) != shallow), "wheels in the shallows");
            }
            app.world_mut().entity_mut(e).despawn();
        }
    }

    /// One player's long queue does not hold up another's request: the
    /// lines take turns.
    #[test]
    fn the_players_lines_take_turns() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 160.0, &[("ironhold", 0), ("ironhold", 1)]);
        forest(&mut app, &data);
        let foot = unit_of(&data, MoveFlag::Foot);
        for i in 0..60 {
            let at = Vec2::new(10.0 + (i % 6) as f32 * 4.0, 10.0 + (i / 6) as f32 * 14.0);
            let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), foot, at, 0.0));
            app.world_mut().entity_mut(e).insert(MoveGoal::point(Vec2::new(95.0 + (i % 6) as f32 * 12.0, 8.0 + (i / 6) as f32 * 16.0), 0.5));
        }
        run_ticks(&mut app, 1);
        assert!(queued(&app) > 30, "a long line ({})", queued(&app));
        let late = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[1]), foot, Vec2::new(20.0, 80.0), 0.0));
        app.world_mut().entity_mut(late).insert(MoveGoal::point(Vec2::new(140.0, 80.0), 0.5));
        run_ticks(&mut app, 3);
        assert_eq!(status(&app, late), PathStatus::Following, "served within a few ticks with {} waiting ahead", queued(&app));
    }

    /// Four tree lines across a 160 m map, each with one gap, alternating ends.
    fn forest(app: &mut App, data: &GameData) {
        let tree = data.node_ids().filter(|n| data.node(*n).blocking).min_by(|a, b| data.node(*a).size[0].total_cmp(&data.node(*b).size[0])).expect("a tree");
        for (x, gap) in [(50, 20), (62, 140), (74, 30), (86, 130)] {
            for y in (0..160).filter(|y| !(gap..gap + 4).contains(y)) {
                let at = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                with_commands(app, |c, data, _| spawn_node(c, data, tree, at, 0.0, None));
            }
        }
    }

    /// The path service on a 768 m map with a river across it, crossed at
    /// two fords far from where most units go, and a mass order of 60
    /// riders scattered up and down the west bank to spots on the east
    /// bank: every request gets its first leg within a few dozen ticks, no
    /// tick of the service runs over its time budget, and the counts stay
    /// small.
    #[test]
    fn path_service_on_a_big_map_with_a_river_stays_within_budget() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 768.0, &[("ironhold", 0)]);
        paint_river(&mut app, 384, &[(120, "ford"), (650, "ford")]);
        let rider = unit_of(&data, MoveFlag::Mounted);
        let units: Vec<Entity> = (0..60)
            .map(|i| {
                let at = Vec2::new(250.0 + (i % 6) as f32 * 15.0, 200.0 + (i / 6) as f32 * 40.0);
                let goal = Vec2::new(440.0 + (i % 5) as f32 * 40.0, 650.0 - (i / 5) as f32 * 45.0);
                let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), rider, at, 0.0));
                app.world_mut().entity_mut(e).insert(MoveGoal::point(goal, 0.5));
                e
            })
            .collect();
        // The first tick also builds the riders' region map.
        run_ticks(&mut app, 1);
        let mut service = Schedule::default();
        service.add_systems(service_paths);
        let mut times: Vec<f64> = Vec::new();
        while queued(&app) > 0 && times.len() < 200 {
            let t = Instant::now();
            service.run(app.world_mut());
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        assert_eq!(queued(&app), 0);
        for &e in &units {
            assert_eq!(status(&app, e), PathStatus::Following);
        }
        let runs = times.len();
        let mean = times.iter().sum::<f64>() / runs as f64;
        times.sort_by(f64::total_cmp);
        let (p90, worst) = (times[runs * 9 / 10], times[runs - 1]);
        let (cells, coarse) = expanded(&app);
        println!("path service on a 768 m map with a river: {runs} ticks for 60 riders, {mean:.2} ms per tick, p90 {p90:.2} ms, worst {worst:.2} ms; {cells} cell and {coarse} coarse expansions");
        assert!(runs <= 40, "{runs} ticks");
        assert!(mean < 4.0 && p90 < 6.0, "{mean:.2} ms per tick of the path service, p90 {p90:.2} ms");
    }

    /// Paint `terrain` over every cell `inside` says.
    fn paint(app: &mut App, terrain: &str, inside: impl Fn(i32, i32) -> bool) {
        let data = app.world().resource::<Data>().0.clone();
        let t = data.terrain_id(terrain).unwrap_or_else(|| panic!("no terrain `{terrain}`")).0;
        let mut nav = app.world_mut().resource_mut::<Nav>();
        for y in 0..nav.height() as i32 {
            for x in 0..nav.width() as i32 {
                if inside(x, y) {
                    nav.set_terrain(IVec2::new(x, y), t);
                }
            }
        }
    }

    /// A mountain range across a 512 m map with one 5 m pass 300 m off
    /// the line: a rider crosses at the pass, and a giant too wide for it
    /// is told at once that it cannot get there.
    #[test]
    fn a_range_is_crossed_at_its_pass() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 512.0, &[("ironhold", 0)]);
        paint(&mut app, "mountain", |x, y| (250..266).contains(&y) && !(400..405).contains(&x));
        let rider = unit_of(&data, MoveFlag::Mounted);
        let (start, goal) = (Vec2::new(100.5, 200.5), Vec2::new(100.5, 320.5));
        let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), rider, start, 0.0));
        app.world_mut().entity_mut(e).insert(MoveGoal::point(goal, 0.5));
        let (s, ticks, trail) = walk(&mut app, e, 12_000);
        let (cells, coarse) = expanded(&app);
        println!("a range with one pass 300 m off the line, 512 m map: {ticks} ticks, {cells} cell and {coarse} coarse expansions");
        assert_eq!(s, PathStatus::Arrived);
        assert!(trail.iter().any(|q| (399.0..406.0).contains(&q.x) && (250.0..266.0).contains(&q.y)), "through the pass");
        assert!(cells < 5_000, "{cells} cell expansions");
        // Wider than the pass: no search, and it fails at the foot of the range.
        let giant = data
            .unit_ids()
            .filter(|u| data.unit(*u).locomotion != MoveFlag::Flying)
            .max_by(|a, b| data.unit(*a).size[0].total_cmp(&data.unit(*b).size[0]).then(b.cmp(a)))
            .filter(|u| data.unit(*u).size[0] * 0.5 > 2.5);
        if let Some(giant) = giant {
            let before = expanded(&app).0;
            let g = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), giant, start, 0.0));
            app.world_mut().entity_mut(g).insert(MoveGoal::point(goal, 0.5));
            let (s, ..) = walk(&mut app, g, 3_000);
            assert_eq!(s, PathStatus::Failed);
            assert_eq!(expanded(&app).0, before, "no search");
        }
    }

    /// Two rivers crossing in a plus, one ford on each arm: a rider goes
    /// from one quadrant to the opposite one through two fords.
    #[test]
    fn two_crossing_rivers_are_crossed_at_their_fords() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 512.0, &[("ironhold", 0)]);
        paint(&mut app, "deep_water", |x, y| (250..262).contains(&x) || (250..262).contains(&y));
        paint(&mut app, "ford", |x, y| ((250..262).contains(&x) && ((60..67).contains(&y) || (450..457).contains(&y))) || ((250..262).contains(&y) && ((60..67).contains(&x) || (450..457).contains(&x))));
        let rider = unit_of(&data, MoveFlag::Mounted);
        let (start, goal) = (Vec2::new(200.5, 200.5), Vec2::new(312.5, 312.5));
        let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), rider, start, 0.0));
        app.world_mut().entity_mut(e).insert(MoveGoal::point(goal, 0.5));
        let (s, ticks, trail) = walk(&mut app, e, 12_000);
        let (cells, coarse) = expanded(&app);
        println!("two crossing rivers, 512 m map: {ticks} ticks, {cells} cell and {coarse} coarse expansions");
        assert_eq!(s, PathStatus::Arrived);
        let nav = app.world().resource::<Nav>();
        let deep = data.terrain_id("deep_water").expect("deep water");
        assert!(trail.iter().all(|q| TerrainId(nav.terrain_at(nav.cell_of(*q))) != deep), "never in deep water");
        assert!(cells < 5_000, "{cells} cell expansions");
    }

    /// The slowest wheels on a long way whose second leg runs into a wall
    /// the coarse route does not see (it leaves obstacles out): the leg's
    /// search goes round by the wall's far end, some 130 m longer than the
    /// route's stretch, a minute and a half of driving. The engine drives
    /// round, crosses at the ford and arrives; the longer way is no lack of
    /// progress.
    #[test]
    fn a_leg_round_a_wall_the_route_does_not_see_is_progress() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let p = flat_world(&mut app, 256.0, &[("ironhold", 0)]);
        paint_river(&mut app, 128, &[(220, "ford")]);
        // Across the way up the west bank to the ford, from 80 m west of
        // the river to the river itself.
        let wall = data.building_ids().find(|b| data.building(*b).wall && !data.building(*b).gate).expect("a wall");
        for i in 0..28 {
            let at = Vec2::new(40.0 + i as f32 * 3.0, 196.0);
            with_commands(&mut app, |c, data, players| spawn_building(c, data, players.get(p[0]), wall, at, 0.0, true));
        }
        let engine = data
            .unit_ids()
            .filter(|u| data.unit(*u).locomotion == MoveFlag::Wheeled)
            .min_by(|a, b| data.unit(*a).speed.total_cmp(&data.unit(*b).speed).then(a.cmp(b)))
            .expect("an engine");
        assert!(data.unit(engine).speed < 2.0, "a slow engine");
        let (start, goal) = (Vec2::new(98.5, 128.5), Vec2::new(158.5, 128.5));
        let e = with_commands(&mut app, |c, data, players| spawn_unit(c, data, players.get(p[0]), engine, start, 0.0));
        app.world_mut().entity_mut(e).insert(MoveGoal::point(goal, 0.5));
        let (s, ticks, trail) = walk(&mut app, e, 20_000);
        let walked: f32 = trail.windows(2).map(|w| w[0].distance(w[1])).sum();
        println!("wheels round a wall across a leg: {s:?} after {ticks} ticks, {walked:.0} m walked");
        assert_eq!(s, PathStatus::Arrived, "{:?} at {:?}", app.world().get::<PathState>(e), trail.last());
        assert!(trail.iter().any(|q| q.x < 40.0 && q.y > 190.0 && q.y < 202.0), "round the wall's far end");
    }

    /// At the end of a leg, the path to the next one replaces what the
    /// route said was left (the route's straight stretch): a way round
    /// obstacles longer than the stretch moves the best mark of the
    /// progress count up by what it adds, so the unit does not have to
    /// walk that much before anything counts as progress again. So does a
    /// leg added to the path while the unit walks on.
    #[test]
    fn the_next_leg_moves_the_progress_mark_by_what_it_adds() {
        let mob = walker();
        let (pos, goal) = (Vec2::new(10.0, 10.0), Vec2::new(200.0, 10.0));
        let order = MoveGoal::point(goal, 0.5);
        // The last leg ended here: 100 m of route left beyond it.
        let mut path = PathState { waypoints: vec![pos], next: 1, status: PathStatus::Following, goal_seq: 1, partial: true, stuck_time: 0.0 };
        let mut mover = Mover { on_leg: true, truncated: true, beyond: 100.0, progress_best: 100.0, wants_repath: true, continuing: true, ..Mover::default() };
        assert_eq!(mover.way_left(pos, &path), 100.0);
        // The next leg ends 20 m down the route (80 m beyond it), but its
        // path goes 10 m aside and back: 2 x sqrt(10^2 + 10^2) = 28.3 m.
        let aim = Aim { goal: pos + Vec2::new(20.0, 0.0), reach: false, last: false, unreachable: false, beyond: 80.0, extends: false };
        let ask = Ask { goal: &order, mob: &mob, pos, team: 0, target: aim.goal, follow: None };
        let result = af_geom::PathResult { waypoints: vec![pos + Vec2::new(10.0, 10.0), aim.goal], complete: true, truncated: false, expansions: 50 };
        apply_path(&ask, &aim, goal, result, &mut path, &mut mover);
        assert_eq!(path.status, PathStatus::Following);
        let left = mover.way_left(pos, &path);
        assert!((left - 108.28).abs() < 0.01, "{left}");
        assert!(left <= mover.progress_best + 1e-3, "{left} m left, best mark {}", mover.progress_best);
        assert!(mover.progress_best < 108.29, "no more than the leg added: {}", mover.progress_best);

        // A leg added to the path while the unit, 15 m short of its end,
        // walks on.
        let end = pos + Vec2::new(20.0, 0.0);
        let unit = pos + Vec2::new(5.0, 0.0);
        let mut mover = Mover { on_leg: true, truncated: true, beyond: 80.0, progress_best: 95.0, extend: true, ..Mover::default() };
        let mut path = PathState { waypoints: vec![end], next: 0, status: PathStatus::Following, goal_seq: 1, partial: true, stuck_time: 0.0 };
        let before = mover.way_left(unit, &path);
        let aim = Aim { goal: end + Vec2::new(20.0, 0.0), extends: true, beyond: 60.0, ..aim };
        let ask = Ask { pos: end, target: aim.goal, ..ask };
        let result = af_geom::PathResult { waypoints: vec![end + Vec2::new(10.0, 10.0), aim.goal], complete: true, truncated: false, expansions: 50 };
        apply_path(&ask, &aim, goal, result, &mut path, &mut mover);
        let after = mover.way_left(unit, &path);
        assert!((after - before - 8.28).abs() < 0.01, "{before} -> {after}");
        assert!((mover.progress_best - 103.28).abs() < 0.01, "the mark moved by what the leg added: {}", mover.progress_best);
    }

    /// The forest map for `players`; each of `orders` (owner, start, goal)
    /// sends a foot unit. Returns them in order.
    fn forest_match(app: &mut App, players: &[(&str, u8)], orders: &[(usize, Vec2, Vec2)]) -> Vec<Entity> {
        let data = app.world().resource::<Data>().0.clone();
        let p = flat_world(app, 160.0, players);
        forest(app, &data);
        let foot = unit_of(&data, MoveFlag::Foot);
        orders
            .iter()
            .map(|&(owner, at, goal)| {
                let e = with_commands(app, |c, data, players| spawn_unit(c, data, players.get(p[owner]), foot, at, 0.0));
                app.world_mut().entity_mut(e).insert(MoveGoal::point(goal, 0.5));
                e
            })
            .collect()
    }

    /// Leave the match as the game does: everything of it despawned, then
    /// out of `Playing`. A test world keeps Bevy's ids, which hand the
    /// freed slots out again last freed first; the slots are taken up here,
    /// so the next match's entities are numbered in the order they are
    /// spawned, as in a fresh app (in a real match af_core's sim ids see to
    /// that).
    fn leave_match(app: &mut App) {
        let world = app.world_mut();
        let doomed: Vec<Entity> = world.query_filtered::<Entity, Or<(With<Unit>, With<Building>, With<ResourceNode>)>>().iter(world).collect();
        for e in doomed {
            world.despawn(e);
        }
        world.flush();
        while world.spawn_empty().id().generation() != bevy::ecs::entity::EntityGeneration::FIRST {}
        world.resource_mut::<NextState<GameState>>().set(GameState::Menu);
        app.update();
    }

    /// Every unit's path status, position and facing, bit for bit, after
    /// each of `ticks` ticks.
    fn trace(app: &mut App, units: &[Entity], ticks: u32) -> Vec<Vec<(PathStatus, [u32; 3])>> {
        (0..ticks)
            .map(|_| {
                run_ticks(app, 1);
                units
                    .iter()
                    .map(|&e| {
                        let (pos, facing) = (app.world().get::<Position>(e).expect("position").0, app.world().get::<Facing>(e).expect("facing").0);
                        (status(app, e), [pos.x.to_bits(), pos.y.to_bits(), facing.to_bits()])
                    })
                    .collect()
            })
            .collect()
    }

    /// A match played in an app that played one before goes as it does in
    /// a fresh app (as a headless replay of it, or a lockstep peer that
    /// just started, plays it): the path queue starts over, so the players'
    /// lines take turns from the first again, whatever the last match's
    /// player count, whose turn it was and who was still waiting when it
    /// ended.
    #[test]
    fn a_second_match_in_one_app_replays_as_in_a_fresh_one() {
        let data = test_data();
        // Two players each send six units across the forest, scattered so
        // that no two share a search: more searching than a tick's budget.
        let orders: Vec<(usize, Vec2, Vec2)> = (0..12)
            .map(|i| {
                let (owner, j) = (i % 2, (i / 2) as f32);
                (owner, Vec2::new(10.0 + owner as f32 * 20.0, 10.0 + j * 28.0), Vec2::new(110.0 + owner as f32 * 30.0, 150.0 - j * 28.0))
            })
            .collect();
        let two = [("ironhold", 0), ("ironhold", 1)];

        let mut fresh = sim_app(data.clone());
        fresh.add_plugins(crate::MovementPlugin);
        let units = forest_match(&mut fresh, &two, &orders);
        let first = trace(&mut fresh, &units, 90);

        // The last match: three players, the third sending a crowd across
        // the forest, some of it still in line when the match ends.
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        let crowd: Vec<(usize, Vec2, Vec2)> =
            (0..30).map(|i| (2, Vec2::new(10.0 + (i % 6) as f32 * 4.0, 10.0 + (i / 6) as f32 * 30.0), Vec2::new(100.0 + (i % 6) as f32 * 9.0, 8.0 + (i / 6) as f32 * 30.0))).collect();
        forest_match(&mut app, &[("ironhold", 0), ("ironhold", 1), ("ironhold", 2)], &crowd);
        run_ticks(&mut app, 2);
        assert!(queued(&app) > 0, "the last match ends with units waiting");
        leave_match(&mut app);
        assert_eq!(queued(&app), 0, "nobody of the last match waits in line");
        let units = forest_match(&mut app, &two, &orders);
        app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Playing);
        let second = trace(&mut app, &units, 90);
        let agree = first.iter().zip(&second).take_while(|(a, b)| a == b).count();
        assert!(first == second, "the second match diverges from a fresh one at tick {}", agree + 1);
    }
}
