//! Grid pathfinding on [`NavGrid`]: A* with mover clearance and terrain
//! costs, exact line-of-sight checks, string-pull smoothing, nearest open
//! cell, and a small cache that lets a group ordered to one place share one
//! search.
//!
//! * Clearance tests cost O(1) once [`NavGrid::prepare`] has built the
//!   summed-area tables for the mover's class (see `clearance.rs`);
//!   `find_path` prepares them itself.
//! * Step costs are `distance * terrain cost` with per-mobility cost tables
//!   ([`NavGrid::set_terrain_cost`]; 1.0 when none is registered). The
//!   heuristic is octile distance times the table's cheapest cost (so it
//!   stays admissible), with a small greedy weight.
//! * A goal whose cell is blocked (a tree being gathered, a building being
//!   built or attacked) is reached at the first open cell found that touches
//!   an obstruction and is about as close to the goal as the open cell facing
//!   the start. So the search neither floods the map for a cell it can never
//!   enter nor insists on one particular cell (which may sit in a pocket).
//! * A request walking up to an entity ([`PathRequest::reach`]) is done at
//!   the first open cell from which that entity's box is within reach, so a
//!   worker sent to a tree behind two others goes round to an open side
//!   instead of stopping at the cell facing it across a slot it cannot pass.
//! * Only searches that ended on their own are cached; one cut short by its
//!   expansion budget is flagged `truncated`, and a bigger budget may do better.
//! * [`NavGrid::find_path_sliced`] spreads one search over several calls (the
//!   path service gives each tick a fixed budget): a search out of its
//!   slice is set aside with its open list and goes on, with the same
//!   result, at the next call for the same request.
//! * A request may name cells to go round (other movers in the way of one
//!   that got stuck): stepping on one costs [`AVOID_COST`] times as much, and
//!   such a search is neither cached nor answered from the cache.
//! * A clear cell whose four axis neighbours are all closed is a *pocket*:
//!   no step of the search leads out of it (diagonals may not cut corners),
//!   yet a mover can stand in it, e.g. a worker squeezed between a tree and
//!   a new wall. A mover starting in a pocket first walks straight to the
//!   nearest roomy cell (clear and no pocket), a goal in a pocket counts as
//!   blocked, and `nearest_open` never picks one.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::fmt;

use bevy_math::{IVec2, Vec2};

use crate::clearance::{ClearQuery, Clearance, MAX_TABLE_CLEARANCE};
use crate::grid::{NavGrid, FLYING};
use crate::obb::{Frame, Obb};

/// Greedy weight on the heuristic: routes are at most 25% longer than optimal
/// (far less after smoothing) and cluttered searches expand ~30% fewer cells.
const HEURISTIC_WEIGHT: f32 = 1.25;
/// Searches remembered for reuse by nearby movers heading to the same cell.
const RECENT_PATHS: usize = 32;
/// A cached search is reused for starts within this distance (metres).
const REUSE_RADIUS: f32 = 6.0;
/// How many leading waypoints of a cached path a reusing mover may join at.
const REUSE_JOIN: usize = 3;
/// Fallback radius (metres) for an open cell around a blocked goal.
const GOAL_SEARCH_RADIUS: f32 = 8.0;
/// A blocked goal accepts open cells up to this many cells farther from it
/// than the open cell facing the start.
const GOAL_ACCEPT_SLACK: f32 = 0.75;
/// The open cell facing the start stands for a blocked goal unless an open
/// cell around the goal is this many cells closer to it.
const FACING_PREFERENCE: f32 = 1.5;
/// Radius (metres) searched for an open cell when the mover starts somewhere tight.
const START_SEARCH_RADIUS: f32 = 6.0;
/// Step-cost factor on a cell a request asked to go round (another mover in
/// the way): a detour up to this many times as long is taken instead.
pub const AVOID_COST: f32 = 4.0;

/// Per-grid pathfinding state: search scratch, clearance tables, terrain
/// cost tables and recently computed paths. Lives inside the `NavGrid`.
#[derive(Clone, Default)]
pub struct PathCache {
    search: Search,
    /// A search `find_path_sliced` set aside; it owns `search` until dropped.
    paused: Option<PausedSearch>,
    /// Per cell: the stamp of the latest request that asked to go round it.
    avoid: Vec<u32>,
    /// Latest stamp handed out.
    avoid_stamp: u32,
    /// Stamp of the request being planned or searched (0 = none).
    active_avoid: u32,
    clearance: Clearance,
    costs: Vec<CostTable>,
    recent: VecDeque<RecentPath>,
    tag: u64,
}

impl fmt::Debug for PathCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PathCache")
            .field("clearance_layers", &self.clearance.layer_count())
            .field("cost_tables", &self.costs.len())
            .field("recent_paths", &self.recent.len())
            .field("paused", &self.paused.as_ref().map(|p| p.token))
            .field("tag", &self.tag)
            .finish()
    }
}

/// Generation-stamped A* buffers (never cleared between searches). One
/// record per cell keeps each visit to a single cache line.
#[derive(Clone, Default)]
struct Search {
    cells: Vec<CellState>,
    generation: u32,
    open: BinaryHeap<Node>,
}

#[derive(Clone, Copy, Default)]
struct CellState {
    g: f32,
    parent: u32,
    /// Search run that last touched `g`/`parent` (stale otherwise).
    seen: u32,
    /// Search run that closed the cell.
    closed: u32,
}

impl Search {
    /// Start a new search over `n` cells; returns its run id.
    fn begin(&mut self, n: usize) -> u32 {
        if self.cells.len() != n {
            self.cells = vec![CellState::default(); n];
            self.generation = 0;
        }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.cells.iter_mut().for_each(|c| *c = CellState::default());
            self.generation = 1;
        }
        self.open.clear();
        self.generation
    }
}

/// Step-cost multipliers by terrain id for movers with one mobility mask.
#[derive(Clone)]
struct CostTable {
    mobility: u16,
    cost: [f32; 256],
    /// Cheapest entry (keeps the heuristic admissible).
    min: f32,
}

#[derive(Clone)]
struct RecentPath {
    mobility: u16,
    clearance: i32,
    team: u8,
    version: u64,
    start: Vec2,
    goal_cell: IVec2,
    waypoints: Vec<Vec2>,
    complete: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct PathRequest {
    pub start: Vec2,
    pub goal: Vec2,
    /// Mobility bit mask of the mover (see af_data::Mobility).
    pub mobility: u16,
    /// Lateral half-width of the mover in metres (clearance).
    pub radius: f32,
    /// Mover's team (for gates); `grid::NO_TEAM` passes no gates.
    pub team: u8,
    /// Search budget; the best partial path is returned when exhausted.
    pub max_expansions: u32,
    /// Walking up to an entity at `goal`: its box, and how far from it (to
    /// a cell centre) the mover can do what it came for. Any open cell that
    /// near ends the search. `None`: a plain point goal.
    pub reach: Option<(Obb, f32)>,
}

#[derive(Clone, Debug, Default)]
pub struct PathResult {
    /// World waypoints after `start`, smoothed. The last one is `goal` when
    /// `complete`, else the reachable point closest to the goal. Empty when
    /// nothing is reachable.
    pub waypoints: Vec<Vec2>,
    /// The goal cell itself was reached.
    pub complete: bool,
    /// The search stopped at `max_expansions`: the path ends wherever it had
    /// got to, and a bigger budget may get closer or complete it.
    pub truncated: bool,
    pub expansions: u32,
}

impl PathResult {
    /// A single straight walk to `to`.
    fn straight(to: Vec2, complete: bool) -> PathResult {
        PathResult { waypoints: vec![to], complete, ..PathResult::default() }
    }
}

/// What a search heads for.
#[derive(Clone, Copy)]
struct Goal {
    /// Cell of the (map-clamped) goal point.
    cell: IVec2,
    point: Vec2,
    /// The goal cell is clear for the mover: only reaching it completes the path.
    open: bool,
    /// Blocked goal: a clear cell touching an obstruction whose centre lies
    /// within this squared distance of `point` also ends the search.
    accept_sq: Option<f32>,
    /// Walking up to an entity: a clear cell whose centre lies this close to
    /// its box ends the search (see [`PathRequest::reach`]).
    reach: Option<(Frame, f32)>,
    /// Where a straight walk would end: the goal itself, else the open cell
    /// facing the start (its centre).
    near: IVec2,
    near_point: Vec2,
}

impl Goal {
    #[inline]
    fn accepts(&self, g: &NavGrid, c: IVec2, touches_obstruction: bool) -> bool {
        c == self.cell
            || self.within_reach(g.cell_center(c))
            || (touches_obstruction && self.accept_sq.is_some_and(|a| g.cell_center(c).distance_squared(self.point) <= a))
    }

    #[inline]
    fn within_reach(&self, p: Vec2) -> bool {
        self.reach.is_some_and(|(b, r)| b.distance_to_point(p) <= r)
    }

    /// May a mover walk straight to `near_point` and be done? Always for a
    /// point goal; walking up to an entity, only when it is within reach there.
    fn near_will_do(&self) -> bool {
        self.open || self.reach.is_none() || self.within_reach(self.near_point)
    }
}

/// How a sliced search ([`NavGrid::find_path_sliced`]) stands after a call.
#[derive(Clone, Debug)]
pub enum PathStep {
    /// Finished (or answered without a search) after `spent` expansions in
    /// this call.
    Done { result: PathResult, spent: u32 },
    /// Out of this call's slice, all `spent`: call again with the same
    /// token to go on.
    Paused { spent: u32 },
}

/// What [`NavGrid::plan`] made of a request.
enum Plan {
    /// Answered without a search.
    Answer(PathResult),
    /// Needs a search from `start` toward `goal`.
    Search { goal: Goal, start: IVec2 },
}

/// Progress of one A* run over the `Search` buffers.
#[derive(Clone, Copy)]
struct Run {
    /// `Search` generation of the run.
    id: u32,
    /// Start cell index.
    start: u32,
    /// (octile distance to the goal, cell index) of the expanded cell
    /// closest to the goal so far.
    best: (f32, u32),
    expansions: u32,
    /// Expansions in the latest slice.
    slice_expansions: u32,
    h_scale: f32,
}

/// A search set aside by `find_path_sliced`, for the next call with its token.
#[derive(Clone)]
struct PausedSearch {
    token: u64,
    /// Grid version it was searching (any change drops it).
    version: u64,
    req: PathRequest,
    goal: Goal,
    run: Run,
    /// Its `avoid` stamp (0 = none).
    avoid: u32,
}

/// Outcome of one A* run.
struct Found {
    /// Cells from the start to the accepted cell (else to the expanded cell
    /// closest to the goal).
    cells: Vec<IVec2>,
    reached: bool,
    truncated: bool,
    expansions: u32,
}

#[derive(Copy, Clone, PartialEq)]
struct Node {
    f: f32,
    idx: u32,
}
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.partial_cmp(&self.f).unwrap_or(Ordering::Equal).then_with(|| o.idx.cmp(&self.idx))
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

#[inline]
fn octile(a: IVec2, b: IVec2) -> f32 {
    let dx = (a.x - b.x).abs() as f32;
    let dy = (a.y - b.y).abs() as f32;
    let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
    hi + (std::f32::consts::SQRT_2 - 1.0) * lo
}

/// Axis steps; diagonals combine two of them (indices into this table).
const AXES: [IVec2; 4] = [IVec2::new(1, 0), IVec2::new(-1, 0), IVec2::new(0, 1), IVec2::new(0, -1)];
const DIAGONALS: [(usize, usize); 4] = [(0, 2), (0, 3), (1, 2), (1, 3)];

/// One mover class (mobility, clearance, team) bound to a grid: clearance
/// through the tables when fresh, else per cell; terrain costs.
struct Walker<'a> {
    grid: &'a NavGrid,
    query: Option<ClearQuery<'a>>,
    mobility: u16,
    team: u8,
    k: i32,
    costs: Option<&'a CostTable>,
    /// Cells to go round: (per-cell stamps, this request's stamp).
    avoid: Option<(&'a [u32], u32)>,
}

impl<'a> Walker<'a> {
    fn new(grid: &'a NavGrid, mobility: u16, radius: f32, team: u8) -> Walker<'a> {
        let k = grid.clearance_cells(radius);
        let query = if k <= MAX_TABLE_CLEARANCE { grid.cache.clearance.query(grid, mobility, team) } else { None };
        let stamp = grid.cache.active_avoid;
        let avoid = (stamp != 0).then_some((grid.cache.avoid.as_slice(), stamp));
        Walker { grid, query, mobility, team, k, costs: grid.cache.costs.iter().find(|t| t.mobility == mobility), avoid }
    }

    #[inline]
    fn clear(&self, c: IVec2) -> bool {
        match &self.query {
            // A single cell is cheaper to test directly than through the tables.
            _ if self.k == 0 => self.grid.passable(c, self.mobility, self.team),
            Some(q) => q.square_clear(c, self.k),
            None => self.grid.square_passable(c, self.k, self.mobility, self.team),
        }
    }

    #[inline]
    fn cost(&self, c: IVec2) -> f32 {
        let terrain = match self.costs {
            Some(t) => t.cost[self.grid.terrain_at(c) as usize],
            None => 1.0,
        };
        match self.avoid {
            Some((stamps, stamp)) if stamps.get(self.grid.index(c)) == Some(&stamp) => terrain * AVOID_COST,
            _ => terrain,
        }
    }

    fn min_cost(&self) -> f32 {
        self.costs.map_or(1.0, |t| t.min)
    }

    /// Steps cost more than their length somewhere (terrain costs, cells to
    /// go round): straight shortcuts must not cross dearer cells.
    fn weighted(&self) -> bool {
        self.costs.is_some() || self.avoid.is_some()
    }

    /// Clear, and no pocket: some axis neighbour is clear too, so a search
    /// can step out of it.
    fn roomy(&self, c: IVec2) -> bool {
        self.clear(c) && AXES.iter().any(|step| {
            let n = c + *step;
            self.grid.in_bounds(n) && self.clear(n)
        })
    }

    /// Every cell the segment crosses is clear (and, with `max_cost`, no
    /// dearer than that).
    fn line(&self, a: Vec2, b: Vec2, max_cost: Option<f32>) -> bool {
        self.grid.traverse(a, b, |c| self.clear(c) && max_cost.is_none_or(|m| self.cost(c) <= m + 1e-4))
    }
}

impl NavGrid {
    /// Cells of clearance needed around the centre cell for `radius`.
    #[inline]
    pub fn clearance_cells(&self, radius: f32) -> i32 {
        ((radius / self.cell) - 0.5).ceil().max(0.0) as i32
    }

    /// Bring the clearance tables for movers of `mobility` on `team` up to
    /// date, so `&self` queries (`clear_cell`, `line_clear`, `nearest_open`)
    /// cost O(1) per cell until the grid changes again. Cheap when nothing
    /// changed. `find_path` calls it itself.
    pub fn prepare(&mut self, mobility: u16, team: u8) {
        if mobility & FLYING != 0 {
            return;
        }
        let mut cl = std::mem::take(&mut self.cache.clearance);
        cl.refresh(self);
        cl.ensure(self, mobility, team);
        self.cache.clearance = cl;
    }

    /// Register the A* step-cost multiplier for movers whose mobility mask is
    /// exactly `mobility` on terrain id `terrain` (e.g. `1 / speed`). Unset
    /// entries cost 1.0. Costs steer route choice only; passability is
    /// unchanged.
    pub fn set_terrain_cost(&mut self, terrain: u8, mobility: u16, cost: f32) {
        let cost = if cost.is_finite() { cost.clamp(0.05, 100.0) } else { 1.0 };
        let table = match self.cache.costs.iter().position(|t| t.mobility == mobility) {
            Some(i) => &mut self.cache.costs[i],
            None => {
                self.cache.costs.push(CostTable { mobility, cost: [1.0; 256], min: 1.0 });
                self.cache.costs.last_mut().expect("just pushed")
            }
        };
        table.cost[terrain as usize] = cost;
        table.min = table.cost.iter().copied().fold(f32::INFINITY, f32::min);
        // Cached routes were costed with the old table.
        self.cache.recent.retain(|r| r.mobility != mobility);
    }

    /// Step-cost multiplier for `mobility` on `terrain` (1.0 when unset).
    pub fn terrain_cost(&self, terrain: u8, mobility: u16) -> f32 {
        self.cache.costs.iter().find(|t| t.mobility == mobility).map_or(1.0, |t| t.cost[terrain as usize])
    }

    /// Whether a cost table is registered for exactly this mobility mask.
    pub fn has_terrain_costs(&self, mobility: u16) -> bool {
        self.cache.costs.iter().any(|t| t.mobility == mobility)
    }

    /// An opaque stamp for code that mirrors outside state into this grid:
    /// af_movement stamps the grid it registered obstacles into, so it notices
    /// when a new map replaces it. 0 on a fresh grid; cloning copies it.
    pub fn sync_tag(&self) -> u64 {
        self.cache.tag
    }

    pub fn set_sync_tag(&mut self, tag: u64) {
        self.cache.tag = tag;
    }

    /// Every cell of the square of half-size `k` around `c` is passable.
    fn square_passable(&self, c: IVec2, k: i32, mobility: u16, team: u8) -> bool {
        (-k..=k).all(|dy| (-k..=k).all(|dx| self.passable(c + IVec2::new(dx, dy), mobility, team)))
    }

    /// Can a mover of `radius` stand with its centre in cell `c`?
    pub fn clear_cell(&self, c: IVec2, mobility: u16, radius: f32, team: u8) -> bool {
        if mobility & FLYING != 0 {
            return self.in_bounds(c);
        }
        Walker::new(self, mobility, radius, team).clear(c)
    }

    pub fn clear_at(&self, p: Vec2, mobility: u16, radius: f32, team: u8) -> bool {
        self.clear_cell(self.cell_of(p), mobility, radius, team)
    }

    /// Straight walk from `a` to `b` stays on clear cells (every cell the
    /// segment crosses is checked).
    pub fn line_clear(&self, a: Vec2, b: Vec2, mobility: u16, radius: f32, team: u8) -> bool {
        if mobility & FLYING != 0 {
            return true;
        }
        Walker::new(self, mobility, radius, team).line(a, b, None)
    }

    /// Visit, in order, every cell the segment `a`-`b` passes through
    /// (Amanatides-Woo traversal). Stops and returns false as soon as `f` does.
    pub fn traverse(&self, a: Vec2, b: Vec2, mut f: impl FnMut(IVec2) -> bool) -> bool {
        let pa = (a - self.origin) / self.cell;
        let pb = (b - self.origin) / self.cell;
        let mut c = IVec2::new(pa.x.floor() as i32, pa.y.floor() as i32);
        let end = IVec2::new(pb.x.floor() as i32, pb.y.floor() as i32);
        if !f(c) {
            return false;
        }
        let d = pb - pa;
        let step = IVec2::new(if d.x > 0.0 { 1 } else { -1 }, if d.y > 0.0 { 1 } else { -1 });
        let axis = |p: f32, cell: i32, s: i32, dv: f32| -> (f32, f32) {
            if dv.abs() < 1e-9 {
                (f32::INFINITY, f32::INFINITY)
            } else {
                let boundary = if s > 0 { cell as f32 + 1.0 } else { cell as f32 };
                ((boundary - p) / dv, 1.0 / dv.abs())
            }
        };
        let (mut tx, dtx) = axis(pa.x, c.x, step.x, d.x);
        let (mut ty, dty) = axis(pa.y, c.y, step.y, d.y);
        let steps = (end.x - c.x).abs() + (end.y - c.y).abs();
        for _ in 0..steps {
            if tx < ty {
                c.x += step.x;
                tx += dtx;
            } else {
                c.y += step.y;
                ty += dty;
            }
            if !f(c) {
                return false;
            }
        }
        true
    }

    /// Nearest point (cell centre) within `max_dist` where the mover fits.
    pub fn nearest_open(&self, p: Vec2, mobility: u16, radius: f32, team: u8, max_dist: f32) -> Option<Vec2> {
        if mobility & FLYING != 0 {
            return Some(self.clamp_point(p, 0.01));
        }
        let w = Walker::new(self, mobility, radius, team);
        nearest_clear(&w, p, max_dist)
    }

    /// A* from `req.start` to `req.goal`.
    pub fn find_path(&mut self, req: &PathRequest) -> PathResult {
        match self.search(req, u32::MAX, None, &[]) {
            PathStep::Done { result, .. } => result,
            PathStep::Paused { .. } => unreachable!("an unsliced search runs to its end"),
        }
    }

    /// [`find_path`](Self::find_path) spread over several calls: a search
    /// still short of its end (and of `req.max_expansions`) after `slice`
    /// more expansions is set aside and `Paused`, and the next call with the
    /// same `token` goes on with it where it stopped, with another slice. A
    /// call with another token, or a change to the grid, drops it (only one
    /// search is set aside at a time). The token names the request (the
    /// mover and its goal); `req` is only read when the search starts.
    ///
    /// A path rather goes round the `avoid` cells (other movers in the way
    /// of one that got stuck): stepping on one costs [`AVOID_COST`] times as
    /// much. Such a search is neither shared with nor taken from others.
    pub fn find_path_sliced(&mut self, req: &PathRequest, slice: u32, token: u64, avoid: &[IVec2]) -> PathStep {
        self.search(req, slice.max(1), Some(token), avoid)
    }

    /// The answer to `req` when it needs no search: a flyer, a mover already
    /// at the goal, a clear straight line, or a recent search by a nearby
    /// mover to the same cell. `None`: only `find_path` can answer it. A
    /// straight line crossing `avoid` cells (see
    /// [`find_path_sliced`](Self::find_path_sliced)) does not count.
    pub fn quick_path(&mut self, req: &PathRequest, avoid: &[IVec2]) -> Option<PathResult> {
        if req.mobility & FLYING != 0 {
            return Some(PathResult::straight(req.goal, true));
        }
        self.prepare(req.mobility, req.team);
        self.cache.active_avoid = self.mark_avoid(avoid);
        match self.plan(req) {
            Plan::Answer(result) => Some(result),
            Plan::Search { .. } => None,
        }
    }

    fn search(&mut self, req: &PathRequest, slice: u32, token: Option<u64>, avoid: &[IVec2]) -> PathStep {
        if req.mobility & FLYING != 0 {
            return PathStep::Done { result: PathResult::straight(req.goal, true), spent: 0 };
        }
        let resumes = self.cache.paused.as_ref().is_some_and(|p| token == Some(p.token) && p.version == self.version);
        let resumed = if resumes { self.cache.paused.take() } else { None };
        let (req, goal, mut run, mut search) = match resumed {
            Some(p) => {
                self.cache.active_avoid = p.avoid;
                (p.req, p.goal, p.run, std::mem::take(&mut self.cache.search))
            }
            None => {
                self.prepare(req.mobility, req.team);
                self.cache.active_avoid = self.mark_avoid(avoid);
                match self.plan(req) {
                    Plan::Answer(result) => return PathStep::Done { result, spent: 0 },
                    Plan::Search { goal, start } => {
                        // Its buffers are about to be reused.
                        self.cache.paused = None;
                        let mut search = std::mem::take(&mut self.cache.search);
                        let run = self.begin_astar(&mut search, start, &goal, req);
                        (*req, goal, run, search)
                    }
                }
            }
        };
        let w = Walker::new(self, req.mobility, req.radius, req.team);
        let step = self.astar(&w, &mut search, &mut run, &goal, req.max_expansions, slice).map(|found| self.finish_search(&w, &req, &goal, found));
        self.cache.search = search;
        let spent = run.slice_expansions;
        match step {
            Some(result) => {
                // A cut search answers nobody else, nor a retry with more
                // budget; nor does one that went round other movers.
                if !result.truncated && self.cache.active_avoid == 0 {
                    self.remember(&req, &result);
                }
                PathStep::Done { result, spent }
            }
            None => {
                let avoid = self.cache.active_avoid;
                self.cache.paused = Some(PausedSearch { token: token.unwrap_or_default(), version: self.version, req, goal, run, avoid });
                PathStep::Paused { spent }
            }
        }
    }

    /// Stamp `cells` for the request about to be planned; returns its stamp
    /// (0 when there is nothing to go round).
    fn mark_avoid(&mut self, cells: &[IVec2]) -> u32 {
        if cells.is_empty() {
            return 0;
        }
        let n = (self.width * self.height) as usize;
        let cache = &mut self.cache;
        if cache.avoid.len() != n {
            cache.avoid = vec![0; n];
            cache.avoid_stamp = 0;
        }
        cache.avoid_stamp = cache.avoid_stamp.wrapping_add(1);
        if cache.avoid_stamp == 0 {
            cache.avoid.iter_mut().for_each(|s| *s = 0);
            cache.avoid_stamp = 1;
        }
        for &c in cells {
            if self.in_bounds(c) {
                let i = self.index(c);
                self.cache.avoid[i] = self.cache.avoid_stamp;
            }
        }
        self.cache.avoid_stamp
    }

    /// Everything short of a search: a mover already at the goal, a clear
    /// straight line, a recent search to join; else what the search heads
    /// for and where it starts. Expects the clearance tables prepared.
    fn plan(&self, req: &PathRequest) -> Plan {
        let w = Walker::new(self, req.mobility, req.radius, req.team);
        let mut start = self.cell_of(req.start);
        if !w.roomy(start) {
            match nearest_clear(&w, req.start, START_SEARCH_RADIUS) {
                Some(p) => start = self.cell_of(p),
                None => return Plan::Answer(PathResult::default()),
            }
        }
        let goal = self.goal_for(&w, req);
        if goal.accepts(self, start, self.touches_obstruction(&w, start)) {
            let end = if goal.open { req.goal } else { self.cell_center(start) };
            return Plan::Answer(PathResult::straight(end, goal.open));
        }
        // Direct line (no dearer than its ends when costs apply)? Skip the search.
        let direct_cost = w.weighted().then(|| w.cost(self.cell_of(req.start)).max(w.cost(goal.near)));
        if goal.near_will_do() && w.clear(goal.near) && w.line(req.start, goal.near_point, direct_cost) {
            return Plan::Answer(PathResult::straight(goal.near_point, goal.open));
        }
        if let Some(reused) = self.reuse(&w, req, goal.cell).filter(|_| self.cache.active_avoid == 0) {
            return Plan::Answer(reused);
        }
        Plan::Search { goal, start }
    }

    /// The path a finished search found.
    fn finish_search(&self, w: &Walker, req: &PathRequest, goal: &Goal, found: Found) -> PathResult {
        // Complete only on the goal cell itself (a cell within reach of an
        // entity ends the search, but the requester judges that reach).
        let complete = found.reached && goal.open && found.cells.last() == Some(&goal.cell);
        let end_point = match found.cells.last() {
            Some(_) if complete => req.goal,
            Some(c) => self.cell_center(*c),
            None => goal.near_point,
        };
        let waypoints = smooth(w, req.start, &found.cells, end_point);
        PathResult { waypoints, complete, truncated: found.truncated, expansions: found.expansions }
    }

    /// The goal of `req` for this mover class: the goal cell when the mover
    /// fits there, else an acceptance radius around it (see [`Goal`]).
    fn goal_for(&self, w: &Walker, req: &PathRequest) -> Goal {
        let point = self.clamp_point(req.goal, 0.01);
        let cell = self.cell_of(point);
        // An entity no open cell is within reach of (a tree deep in a
        // forest) is searched for like a blocked point: the search must not
        // flood the map for a cell that does not exist.
        let reach = req.reach.map(|(b, r)| (b.frame(), r)).filter(|(b, r)| self.open_cell_within(w, b, *r));
        if w.roomy(cell) {
            return Goal { cell, point, open: true, accept_sq: None, reach, near: cell, near_point: req.goal };
        }
        match self.open_cell_near_goal(w, point, req.start) {
            // An entity's box decides where it is within reach; no guessing
            // from the cells around it.
            Some(near) if reach.is_some() => Goal { cell, point, open: false, accept_sq: None, reach, near, near_point: self.cell_center(near) },
            Some(near) => {
                let accept = self.cell_center(near).distance(point) + GOAL_ACCEPT_SLACK * self.cell;
                Goal { cell, point, open: false, accept_sq: Some(accept * accept), reach, near, near_point: self.cell_center(near) }
            }
            // Nothing open nearby: search toward the goal for the closest reachable cell.
            None => Goal { cell, point, open: false, accept_sq: None, reach, near: cell, near_point: self.cell_center(cell) },
        }
    }

    /// Is some roomy cell's centre within `reach` of the box `b`?
    fn open_cell_within(&self, w: &Walker, b: &Frame, reach: f32) -> bool {
        let (min, max) = b.aabb();
        let (c0, c1) = (self.cell_of(min - Vec2::splat(reach)), self.cell_of(max + Vec2::splat(reach)));
        (c0.y..=c1.y).any(|y| {
            (c0.x..=c1.x).any(|x| {
                let c = IVec2::new(x, y);
                self.in_bounds(c) && b.distance_to_point(self.cell_center(c)) <= reach && w.roomy(c)
            })
        })
    }

    /// First roomy cell walking from a blocked goal back toward the start
    /// (the near side of a building, the near shore of a lake), unless the
    /// nearest roomy cell around the goal is clearly closer to it (the line
    /// back crosses a ring of trees around a clearing): then that one.
    fn open_cell_near_goal(&self, w: &Walker, goal: Vec2, start: Vec2) -> Option<IVec2> {
        let mut facing = None;
        self.traverse(goal, start, |c| {
            if w.roomy(c) {
                facing = Some(c);
                false
            } else {
                true
            }
        });
        let nearest = nearest_clear(w, goal, GOAL_SEARCH_RADIUS).map(|p| self.cell_of(p));
        let dist = |c: IVec2| self.cell_center(c).distance(goal);
        match (facing, nearest) {
            (Some(f), Some(n)) if dist(f) > dist(n) + FACING_PREFERENCE * self.cell => Some(n),
            (Some(f), _) => Some(f),
            (None, n) => n,
        }
    }

    /// Some axis neighbour of `c` is no place for the mover (or off the map).
    fn touches_obstruction(&self, w: &Walker, c: IVec2) -> bool {
        AXES.iter().any(|step| {
            let n = c + *step;
            !(self.in_bounds(n) && w.clear(n))
        })
    }

    /// Join a recent search for the same mover class and goal cell that
    /// started nearby, if the grid has not changed since.
    fn reuse(&self, w: &Walker, req: &PathRequest, goal: IVec2) -> Option<PathResult> {
        let k = w.k;
        let cached = self.cache.recent.iter().rev().find(|r| {
            r.version == self.version
                && r.mobility == req.mobility
                && r.clearance == k
                && r.team == req.team
                && r.goal_cell == goal
                && r.start.distance(req.start) <= REUSE_RADIUS
        })?;
        let n = cached.waypoints.len();
        let join = (0..n.min(REUSE_JOIN)).rev().find(|i| w.line(req.start, cached.waypoints[*i], None))?;
        let mut waypoints = cached.waypoints[join..].to_vec();
        if cached.complete {
            let before = if waypoints.len() >= 2 { waypoints[waypoints.len() - 2] } else { req.start };
            if !w.line(before, req.goal, None) {
                return None;
            }
            if let Some(last) = waypoints.last_mut() {
                *last = req.goal;
            }
        }
        Some(PathResult { waypoints, complete: cached.complete, ..PathResult::default() })
    }

    fn remember(&mut self, req: &PathRequest, result: &PathResult) {
        if result.waypoints.is_empty() {
            return;
        }
        if self.cache.recent.len() >= RECENT_PATHS {
            self.cache.recent.pop_front();
        }
        let goal_cell = self.cell_of(self.clamp_point(req.goal, 0.01));
        self.cache.recent.push_back(RecentPath {
            mobility: req.mobility,
            clearance: self.clearance_cells(req.radius),
            team: req.team,
            version: self.version,
            start: req.start,
            goal_cell,
            waypoints: result.waypoints.clone(),
            complete: result.complete,
        });
    }

    /// Start an A* run from `start` toward `goal` over the `s` buffers.
    fn begin_astar(&self, s: &mut Search, start: IVec2, goal: &Goal, req: &PathRequest) -> Run {
        let id = s.begin((self.width * self.height) as usize);
        let h_scale = Walker::new(self, req.mobility, req.radius, req.team).min_cost() * HEURISTIC_WEIGHT;
        let si = (start.y * self.width as i32 + start.x) as u32;
        s.cells[si as usize] = CellState { g: 0.0, parent: u32::MAX, seen: id, closed: 0 };
        s.open.push(Node { f: octile(start, goal.cell) * h_scale, idx: si });
        Run { id, start: si, best: (octile(start, goal.cell), si), expansions: 0, slice_expansions: 0, h_scale }
    }

    /// Go on with the A* `run` over clear cells until the goal accepts a
    /// cell, nothing reachable is left or `max_expansions` are spent in all
    /// (`Some`: the cells found), or until `slice` more are spent first
    /// (`None`: paused; the buffers keep the run for the next slice).
    fn astar(&self, w: &Walker, s: &mut Search, run: &mut Run, goal: &Goal, max_expansions: u32, slice: u32) -> Option<Found> {
        let width = self.width as i32;
        let to_idx = |c: IVec2| (c.y * width + c.x) as u32;
        let to_cell = |i: u32| IVec2::new(i as i32 % width, i as i32 / width);
        let (id, h_scale, target) = (run.id, run.h_scale, goal.cell);
        let slice_end = run.expansions.saturating_add(slice);
        run.slice_expansions = 0;
        let (mut reached, mut truncated) = (false, false);

        while let Some(&Node { idx, .. }) = s.open.peek() {
            if s.cells[idx as usize].closed == id {
                s.open.pop();
                continue;
            }
            if run.expansions >= slice_end {
                // Out of this slice: the node stays queued for the next one.
                return None;
            }
            s.open.pop();
            s.cells[idx as usize].closed = id;
            let c = to_cell(idx);
            // Axis neighbours first; their clearance doubles as the
            // no-corner-cutting test of the diagonals.
            let mut axis_clear = [false; 4];
            for (i, step) in AXES.iter().enumerate() {
                let nc = c + *step;
                axis_clear[i] = self.in_bounds(nc) && w.clear(nc);
            }
            if goal.accepts(self, c, axis_clear.contains(&false)) {
                reached = true;
                run.best = (0.0, idx);
                break;
            }
            let h = octile(c, target);
            if h < run.best.0 {
                run.best = (h, idx);
            }
            run.expansions += 1;
            run.slice_expansions += 1;
            if run.expansions >= max_expansions {
                truncated = true;
                break;
            }
            let gc = s.cells[idx as usize].g;
            let mut relax = |nc: IVec2, len: f32| {
                let ni = to_idx(nc) as usize;
                let cell = &mut s.cells[ni];
                if cell.closed == id {
                    return;
                }
                let ng = gc + len * w.cost(nc);
                if cell.seen != id || ng < cell.g {
                    *cell = CellState { g: ng, parent: idx, seen: id, closed: cell.closed };
                    s.open.push(Node { f: ng + octile(nc, target) * h_scale, idx: ni as u32 });
                }
            };
            for (i, step) in AXES.iter().enumerate() {
                if axis_clear[i] {
                    relax(c + *step, 1.0);
                }
            }
            for (a, b) in DIAGONALS {
                let nc = c + AXES[a] + AXES[b];
                if axis_clear[a] && axis_clear[b] && w.clear(nc) {
                    relax(nc, std::f32::consts::SQRT_2);
                }
            }
        }

        let mut cells = Vec::new();
        let mut cur = run.best.1;
        while cur != u32::MAX {
            cells.push(to_cell(cur));
            if cur == run.start {
                break;
            }
            cur = s.cells[cur as usize].parent;
        }
        cells.reverse();
        Some(Found { cells, reached, truncated, expansions: run.expansions })
    }
}

/// Nearest roomy cell centre to `p` within `max_dist` (`p` itself when its
/// cell is roomy), searching square rings outward.
fn nearest_clear(w: &Walker, p: Vec2, max_dist: f32) -> Option<Vec2> {
    let g = w.grid;
    let c = g.cell_of(p);
    if w.roomy(c) {
        return Some(p);
    }
    let max_r = (max_dist / g.cell).ceil() as i32;
    for r in 1..=max_r {
        let mut best: Option<(f32, IVec2)> = None;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue;
                }
                let cc = c + IVec2::new(dx, dy);
                if w.roomy(cc) {
                    let d = g.cell_center(cc).distance_squared(p);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, cc));
                    }
                }
            }
        }
        if let Some((_, cc)) = best {
            return Some(g.cell_center(cc));
        }
    }
    None
}

/// String-pull a cell path into few waypoints: from each anchor, jump to the
/// farthest following turn point still in straight-line reach. With terrain
/// costs, a shortcut may not cross terrain dearer than the stretch it replaces.
fn smooth(w: &Walker, start: Vec2, cells: &[IVec2], end_point: Vec2) -> Vec<Vec2> {
    if cells.is_empty() {
        return Vec::new();
    }
    let g = w.grid;
    // Turn points: indices where the step direction changes, plus the ends.
    let mut turns: Vec<usize> = Vec::with_capacity(cells.len().min(64));
    for i in 0..cells.len() {
        let keep = i == 0 || i + 1 == cells.len() || (cells[i] - cells[i - 1]) != (cells[i + 1] - cells[i]);
        if keep {
            turns.push(i);
        }
    }
    let point = |i: usize| if i + 1 == cells.len() { end_point } else { g.cell_center(cells[i]) };
    let with_costs = w.weighted();
    let mut out = Vec::new();
    let mut anchor = start;
    let mut anchor_cell = 0usize;
    let mut t = 0usize;
    while t < turns.len() {
        // Dearest terrain on the cell path from the anchor to the candidate.
        let mut stretch_max = w.cost(cells[anchor_cell]);
        let mut scanned = anchor_cell;
        let mut grow = |upto: usize, stretch_max: &mut f32| {
            while scanned < upto {
                scanned += 1;
                *stretch_max = stretch_max.max(w.cost(cells[scanned]));
            }
        };
        grow(turns[t], &mut stretch_max);
        let mut j = t;
        while j + 1 < turns.len() {
            let mut m = stretch_max;
            grow(turns[j + 1], &mut m);
            if !w.line(anchor, point(turns[j + 1]), with_costs.then_some(m)) {
                break;
            }
            stretch_max = m;
            j += 1;
        }
        let p = point(turns[j]);
        out.push(p);
        anchor = p;
        anchor_cell = turns[j];
        t = j + 1;
    }
    // Drop a leading waypoint that is the start cell itself.
    if out.len() > 1 && out[0].distance(start) < g.cell * 0.5 {
        out.remove(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::NO_TEAM;

    fn grid() -> NavGrid {
        let mut g = NavGrid::new(40, 40, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b1, true);
        g
    }

    fn req(start: Vec2, goal: Vec2) -> PathRequest {
        PathRequest { start, goal, mobility: 1, radius: 0.4, team: 0, max_expansions: 50_000, reach: None }
    }

    /// Every segment of the path (from `start`) is walkable.
    fn assert_walkable(g: &NavGrid, start: Vec2, r: &PathResult, radius: f32) {
        let mut prev = start;
        for w in &r.waypoints {
            assert!(g.line_clear(prev, *w, 1, radius, 0) || prev == start, "segment {prev} -> {w} blocked: {r:?}");
            prev = *w;
        }
    }

    #[test]
    fn straight_line() {
        let mut g = grid();
        let r = g.find_path(&PathRequest { max_expansions: 10_000, ..req(Vec2::new(2.5, 2.5), Vec2::new(30.5, 2.5)) });
        assert!(r.complete);
        assert_eq!(r.waypoints, vec![Vec2::new(30.5, 2.5)]);
    }

    #[test]
    fn around_a_wall() {
        let mut g = grid();
        // Wall from y=0 to y=30 at x=20.
        g.add_obstacle(&Obb::new(Vec2::new(20.0, 15.0), Vec2::new(0.5, 15.0), 0.0), 1);
        let r = g.find_path(&PathRequest { team: NO_TEAM, ..req(Vec2::new(5.5, 5.5), Vec2::new(35.5, 5.5)) });
        assert!(r.complete, "{r:?}");
        assert!(r.waypoints.len() >= 2);
        for w in &r.waypoints {
            assert!(g.passable_at(*w, 1, NO_TEAM));
        }
        // Must go around the top end (y > 30).
        assert!(r.waypoints.iter().any(|w| w.y > 29.0));
        assert_walkable(&g, Vec2::new(5.5, 5.5), &r, 0.4);
    }

    #[test]
    fn unreachable_goal_gives_closest() {
        let mut g = grid();
        // Box the goal in.
        g.add_obstacle(&Obb::new(Vec2::new(30.0, 30.0), Vec2::new(4.0, 4.0), 0.0), 1);
        let r = g.find_path(&req(Vec2::new(2.5, 2.5), Vec2::new(30.0, 30.0)));
        assert!(!r.complete);
        let last = *r.waypoints.last().unwrap();
        assert!(last.distance(Vec2::new(30.0, 30.0)) < 7.0, "{last:?}");
    }

    /// Closes the four axis neighbours of cell (10, 10), leaving it a
    /// one-cell pocket that only diagonals touch.
    fn pocket_grid() -> NavGrid {
        let mut g = grid();
        for c in [IVec2::new(9, 10), IVec2::new(11, 10), IVec2::new(10, 9), IVec2::new(10, 11)] {
            g.add_obstacle(&Obb::new(g.cell_center(c), Vec2::splat(0.3), 0.0), 1);
        }
        g
    }

    /// A mover standing in a pocket (a worker squeezed in beside a tree and
    /// a new wall) walks out to the nearest roomy cell and on to its goal,
    /// instead of getting a path that never leaves the pocket.
    #[test]
    fn a_mover_in_a_pocket_walks_out() {
        let mut g = pocket_grid();
        let start = Vec2::new(10.5, 10.5);
        assert!(g.clear_at(start, 1, 0.4, 0), "the pocket itself is clear");
        let r = g.find_path(&req(start, Vec2::new(30.5, 25.5)));
        assert!(r.complete, "{r:?}");
        assert!(r.waypoints[0].distance(start) < 2.0, "first steps out of the pocket: {r:?}");
        // Nor is a pocket offered as open ground.
        let open = g.nearest_open(start, 1, 0.4, 0, 5.0).expect("open ground");
        assert!(open.distance(start) > 1.0 && g.clear_at(open, 1, 0.4, 0), "{open}");
    }

    /// A goal in a pocket counts as blocked: the path ends beside it
    /// without flooding the map for a cell no step leads into.
    #[test]
    fn a_goal_in_a_pocket_ends_beside_it() {
        let mut g = pocket_grid();
        let r = g.find_path(&req(Vec2::new(30.5, 25.5), Vec2::new(10.5, 10.5)));
        let last = *r.waypoints.last().expect("a path");
        assert!(!r.complete && !r.truncated && r.expansions < 400, "{r:?}");
        assert!(last.distance(Vec2::new(10.5, 10.5)) < 2.0, "{last}");
    }

    /// Walking up to an entity, the search ends at the first open cell from
    /// which its box is within reach: for an entity standing on open ground
    /// well short of its centre, for one that blocks its cells on the side
    /// the search gets to first.
    #[test]
    fn walking_up_to_an_entity_ends_within_reach_of_its_box() {
        let mut g = grid();
        let start = Vec2::new(5.5, 20.5);
        let reach = 0.3 + 0.4 + 0.5;
        // A wall between start and target, so a real search is needed.
        g.add_obstacle(&Obb::new(Vec2::new(15.0, 20.0), Vec2::new(0.5, 8.0), 0.0), 1);
        // On open ground (a unit): done within reach, not at its centre.
        let unit = Obb::new(Vec2::new(30.5, 20.5), Vec2::splat(0.3), 0.0);
        let r = g.find_path(&PathRequest { reach: Some((unit, reach)), ..req(start, unit.center) });
        let last = *r.waypoints.last().expect("a path");
        assert!(!r.complete && unit.distance_to_point(last) <= reach && last.distance(unit.center) > 0.4, "{r:?}");
        assert_walkable(&g, start, &r, 0.4);
        // Blocking its cells (a building).
        let hall = Obb::new(Vec2::new(30.0, 20.0), Vec2::splat(3.0), 0.4);
        g.add_obstacle(&hall, 1);
        let r = g.find_path(&PathRequest { reach: Some((hall, reach)), ..req(start, hall.center) });
        let last = *r.waypoints.last().expect("a path");
        assert!(!r.complete && !r.truncated && hall.distance_to_point(last) <= reach, "{r:?}");
        assert_walkable(&g, start, &r, 0.4);
    }

    /// Walking up to an entity no open cell is within reach of (a tree in
    /// the middle of a forest), the search ends beside the forest as for a
    /// blocked point instead of flooding the map for a cell that does not
    /// exist.
    #[test]
    fn walking_up_to_an_enclosed_entity_does_not_flood_the_map() {
        let mut g = NavGrid::new(120, 120, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b1, true);
        for y in 55..=65 {
            for x in 55..=65 {
                g.add_obstacle(&Obb::new(g.cell_center(IVec2::new(x, y)), Vec2::splat(0.45), 0.0), 1);
            }
        }
        let tree = Obb::new(g.cell_center(IVec2::new(60, 60)), Vec2::splat(0.45), 0.0);
        let r = g.find_path(&PathRequest { reach: Some((tree, 1.2)), ..req(Vec2::new(20.5, 60.5), tree.center) });
        assert!(!r.complete && !r.truncated && r.expansions < 400, "{r:?}");
        assert!(r.waypoints.last().is_some_and(|p| p.distance(tree.center) < 8.0), "{r:?}");
    }

    #[test]
    fn nearest_open_finds_free_cell() {
        let mut g = grid();
        g.add_obstacle(&Obb::new(Vec2::new(10.0, 10.0), Vec2::new(2.0, 2.0), 0.0), 1);
        let p = g.nearest_open(Vec2::new(10.0, 10.0), 1, 0.4, 0, 10.0).unwrap();
        assert!(g.passable_at(p, 1, 0));
    }

    /// A goal inside an obstacle ends right beside it, without exhausting the
    /// search.
    #[test]
    fn blocked_goal_ends_beside_it() {
        let mut g = grid();
        let block = Obb::new(Vec2::new(30.0, 20.0), Vec2::new(4.0, 4.0), 0.0);
        g.add_obstacle(&block, 1);
        // A wall between start and goal so a real search is needed.
        g.add_obstacle(&Obb::new(Vec2::new(18.0, 20.0), Vec2::new(0.5, 8.0), 0.0), 1);
        let r = g.find_path(&req(Vec2::new(5.5, 20.5), Vec2::new(30.0, 20.0)));
        let last = *r.waypoints.last().unwrap();
        assert!(!r.complete && !r.truncated, "{r:?}");
        assert!(g.clear_at(last, 1, 0.4, 0), "ends on open ground: {last:?}");
        assert!(block.distance_to_point(last) <= 1.0, "ends beside the block: {last:?}");
        assert!(r.expansions < 2_000, "no flood fill: {}", r.expansions);
        assert_walkable(&g, Vec2::new(5.5, 20.5), &r, 0.4);
    }

    /// A blocked goal whose open cell facing the start is an enclosed pocket
    /// (a clearing inside a ring of trees) is reached from its open side.
    #[test]
    fn blocked_goal_beside_a_pocket_is_reached_from_its_open_side() {
        let mut g = NavGrid::new(200, 200, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b1, true);
        let tree = |g: &mut NavGrid, x: i32, y: i32| g.add_obstacle(&Obb::new(g.cell_center(IVec2::new(x, y)), Vec2::splat(0.45), 0.0), 1);
        for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            tree(&mut g, 100 + dx, 100 + dy);
        }
        tree(&mut g, 102, 100);
        let goal = g.cell_center(IVec2::new(102, 100));
        let r = g.find_path(&PathRequest { max_expansions: 24_000, ..req(Vec2::new(20.5, 100.5), goal) });
        let last = *r.waypoints.last().unwrap();
        assert!(!r.truncated && r.expansions < 2_000, "no flood fill: {r:?}");
        assert!(last.distance(goal) < 2.5, "ends next to the tree: {last:?}");
        assert_ne!(g.cell_of(last), IVec2::new(100, 100), "not the pocket");
        assert_walkable(&g, Vec2::new(20.5, 100.5), &r, 0.4);
    }

    /// A search cut by its budget is flagged and never handed to a retry with
    /// a bigger budget, nor to a groupmate.
    #[test]
    fn searches_cut_by_their_budget_are_not_reused() {
        let mut g = NavGrid::new(128, 128, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b1, true);
        g.add_obstacle(&Obb::new(Vec2::new(64.0, 55.0), Vec2::new(0.5, 55.0), 0.0), 1);
        let (start, goal) = (Vec2::new(20.5, 20.5), Vec2::new(100.5, 20.5));
        let cut = g.find_path(&PathRequest { max_expansions: 2_000, ..req(start, goal) });
        assert!(cut.truncated && !cut.complete && cut.expansions == 2_000, "{cut:?}");
        let groupmate = g.find_path(&PathRequest { max_expansions: 24_000, ..req(Vec2::new(21.5, 21.5), goal) });
        assert!(groupmate.complete && !groupmate.truncated && groupmate.expansions > 0, "searched afresh: {groupmate:?}");
        // Whole searches are shared.
        let retry = g.find_path(&PathRequest { max_expansions: 24_000, ..req(start, goal) });
        assert!(retry.complete && !retry.truncated && retry.expansions == 0, "reused the whole search: {retry:?}");
        assert_walkable(&g, start, &retry, 0.4);
    }

    /// Runs a sliced search to its end, `slice` expansions per call; returns
    /// the result and how many calls it took.
    fn sliced(g: &mut NavGrid, r: &PathRequest, slice: u32, token: u64) -> (PathResult, u32) {
        for calls in 1..1000 {
            match g.find_path_sliced(r, slice, token, &[]) {
                PathStep::Done { result, spent } => {
                    assert!(spent <= slice);
                    return (result, calls);
                }
                PathStep::Paused { spent } => assert_eq!(spent, slice, "a paused slice uses all of itself"),
            }
        }
        panic!("the search never ended");
    }

    /// A wall a search must walk round, with nothing to reuse.
    fn walled() -> (NavGrid, PathRequest) {
        let mut g = NavGrid::new(128, 128, 1.0, Vec2::ZERO);
        g.set_terrain_info(0, 0b1, true);
        g.add_obstacle(&Obb::new(Vec2::new(64.0, 55.0), Vec2::new(0.5, 55.0), 0.0), 1);
        (g, PathRequest { max_expansions: 24_000, ..req(Vec2::new(20.5, 20.5), Vec2::new(100.5, 20.5)) })
    }

    /// A search spread over many small slices finds exactly what one call
    /// finds, with the same expansions in all.
    #[test]
    fn a_sliced_search_finds_what_one_call_finds() {
        let (mut whole_grid, r) = walled();
        let whole = whole_grid.find_path(&r);
        assert!(whole.complete && whole.expansions > 3_000, "{whole:?}");
        let (mut g, _) = walled();
        let (sliced, calls) = sliced(&mut g, &r, 1_000, 7);
        assert!(calls > 3, "{calls} calls");
        assert_eq!((sliced.waypoints, sliced.complete, sliced.expansions), (whole.waypoints, whole.complete, whole.expansions));
    }

    /// A search set aside is dropped by a search for another request or by
    /// a change to the grid: the next call for it starts afresh.
    #[test]
    fn a_paused_search_is_dropped_by_another_search_or_a_grid_change() {
        let (mut g, r) = walled();
        let whole = g.clone().find_path(&r).expansions;
        assert!(matches!(g.find_path_sliced(&r, 1_000, 1, &[]), PathStep::Paused { .. }));
        // Another request that needs a search takes the buffers.
        let other = PathRequest { max_expansions: 24_000, ..req(Vec2::new(30.5, 90.5), Vec2::new(90.5, 90.5)) };
        assert!(matches!(g.find_path_sliced(&other, 100_000, 2, &[]), PathStep::Done { .. }));
        let (_, calls) = sliced(&mut g, &r, 1_000, 1);
        assert_eq!(calls, whole.div_ceil(1_000), "started afresh");
        // A grid change in the middle.
        let (mut g, r) = walled();
        assert!(matches!(g.find_path_sliced(&r, 1_000, 1, &[]), PathStep::Paused { .. }));
        g.add_obstacle(&Obb::new(Vec2::new(5.0, 120.0), Vec2::splat(0.5), 0.0), 1);
        let (result, calls) = sliced(&mut g, &r, 1_000, 1);
        assert!(result.complete && calls >= whole.div_ceil(1_000), "started afresh: {calls} calls");
        // A request answered without a search leaves a paused one alone.
        let (mut g, r) = walled();
        assert!(matches!(g.find_path_sliced(&r, 1_000, 1, &[]), PathStep::Paused { .. }));
        assert!(matches!(g.find_path_sliced(&req(Vec2::new(2.5, 2.5), Vec2::new(10.5, 2.5)), 1_000, 3, &[]), PathStep::Done { .. }));
        let (_, calls) = sliced(&mut g, &r, 1_000, 1);
        assert_eq!(calls, whole.div_ceil(1_000) - 1, "went on where it stopped");
    }

    /// Movers in the way are gone round when there is room, walked through
    /// when they block the only way, and never make a route for others.
    #[test]
    fn a_path_goes_round_movers_in_the_way() {
        let mut g = grid();
        let (start, goal) = (Vec2::new(5.5, 20.5), Vec2::new(30.5, 20.5));
        // Two movers standing across the straight line.
        let blockers = [IVec2::new(15, 20), IVec2::new(15, 21), IVec2::new(15, 19)];
        assert!(g.quick_path(&req(start, goal), &blockers).is_none(), "the straight line crosses them");
        let PathStep::Done { result: round, .. } = g.find_path_sliced(&req(start, goal), 10_000, 1, &blockers) else { panic!("unsliced") };
        assert!(round.complete && round.waypoints.len() >= 2, "{round:?}");
        let mut prev = start;
        for w in &round.waypoints {
            g.traverse(prev, *w, |c| {
                assert!(!blockers.contains(&c), "walked through {c}: {round:?}");
                true
            });
            prev = *w;
        }
        assert_eq!(g.quick_path(&req(start, goal), &[]).map(|r| r.waypoints), Some(vec![goal]), "nobody else goes round them");
        // A wall with a one-cell gap the movers stand in: through them all the same.
        g.add_obstacle(&Obb::new(Vec2::new(20.5, 10.0), Vec2::new(0.5, 10.0), 0.0), 1);
        g.add_obstacle(&Obb::new(Vec2::new(20.5, 31.0), Vec2::new(0.5, 9.0), 0.0), 1);
        let in_gap = [IVec2::new(20, 20), IVec2::new(19, 20)];
        let PathStep::Done { result: through, .. } = g.find_path_sliced(&req(start, goal), 10_000, 2, &in_gap) else { panic!("unsliced") };
        assert!(through.complete, "{through:?}");
    }

    /// `quick_path` answers whatever needs no search, and nothing else.
    #[test]
    fn quick_paths_need_no_search() {
        let mut g = grid();
        let open = g.quick_path(&req(Vec2::new(2.5, 2.5), Vec2::new(30.5, 2.5)), &[]).expect("straight line");
        assert!(open.complete && open.waypoints == vec![Vec2::new(30.5, 2.5)]);
        let flyer = g.quick_path(&PathRequest { mobility: FLYING, ..req(Vec2::new(2.5, 2.5), Vec2::new(30.5, 30.5)) }, &[]);
        assert!(flyer.is_some_and(|r| r.complete));
        g.add_obstacle(&Obb::new(Vec2::new(20.0, 20.0), Vec2::new(1.0, 12.0), 0.0), 1);
        let goal = Vec2::new(35.5, 20.5);
        assert!(g.quick_path(&req(Vec2::new(5.5, 20.5), goal), &[]).is_none(), "behind a wall");
        assert!(g.find_path(&req(Vec2::new(5.5, 20.5), goal)).complete);
        let groupmate = g.quick_path(&req(Vec2::new(6.5, 21.5), goal), &[]).expect("joins the search just made");
        assert!(groupmate.complete);
    }

    #[test]
    fn terrain_costs_route_around_slow_ground() {
        let mut g = grid();
        g.set_terrain_info(1, 0b1, false);
        // A slow band crossing the direct line, with a cheap detour around its end.
        for y in 10..30 {
            for x in 18..22 {
                g.set_terrain(IVec2::new(x, y), 1);
            }
        }
        let start = Vec2::new(5.5, 20.5);
        let goal = Vec2::new(35.5, 20.5);
        let through = g.find_path(&req(start, goal));
        assert_eq!(through.waypoints, vec![goal], "uniform costs: straight through");
        g.set_terrain_cost(1, 1, 8.0);
        let around = g.find_path(&req(start, goal));
        assert!(around.complete);
        let mut prev = start;
        for w in &around.waypoints {
            g.traverse(prev, *w, |c| {
                assert_ne!(g.terrain_at(c), 1, "crossed slow ground at {c}: {around:?}");
                true
            });
            prev = *w;
        }
        // Another mobility is unaffected.
        assert_eq!(g.terrain_cost(1, 2), 1.0);
    }

    #[test]
    fn clearance_keeps_wide_movers_out_of_narrow_gaps() {
        let mut g = grid();
        // Wall with a 2-cell gap at y = 20..22.
        g.add_obstacle(&Obb::new(Vec2::new(20.5, 10.0), Vec2::new(0.5, 10.0), 0.0), 1);
        g.add_obstacle(&Obb::new(Vec2::new(20.5, 31.0), Vec2::new(0.5, 9.0), 0.0), 1);
        let small = g.find_path(&PathRequest { radius: 0.4, ..req(Vec2::new(5.5, 21.0), Vec2::new(35.5, 21.0)) });
        assert!(small.complete);
        let wide = g.find_path(&PathRequest { radius: 1.4, ..req(Vec2::new(5.5, 21.0), Vec2::new(35.5, 21.0)) });
        assert!(!wide.complete, "k = 1 needs a 3-cell gap: {wide:?}");
    }

    #[test]
    fn group_requests_reuse_one_search() {
        let mut g = grid();
        g.add_obstacle(&Obb::new(Vec2::new(20.0, 20.0), Vec2::new(1.0, 12.0), 0.0), 1);
        let goal = Vec2::new(35.5, 20.5);
        let first = g.find_path(&req(Vec2::new(5.5, 20.5), goal));
        assert!(first.complete && first.expansions > 0);
        let second = g.find_path(&req(Vec2::new(6.5, 21.5), Vec2::new(35.7, 20.2)));
        assert!(second.complete);
        assert_eq!(second.expansions, 0, "reused the first search");
        assert_eq!(*second.waypoints.last().unwrap(), Vec2::new(35.7, 20.2));
        assert_walkable(&g, Vec2::new(6.5, 21.5), &second, 0.4);
        // A grid change invalidates the cache.
        g.add_obstacle(&Obb::new(Vec2::new(2.0, 2.0), Vec2::splat(0.5), 0.0), 1);
        let third = g.find_path(&req(Vec2::new(6.5, 21.5), goal));
        assert!(third.expansions > 0);
    }

    #[test]
    fn traversal_visits_every_crossed_cell() {
        let g = grid();
        let mut cells = Vec::new();
        g.traverse(Vec2::new(0.5, 0.5), Vec2::new(3.5, 1.5), |c| {
            cells.push(c);
            true
        });
        assert_eq!(cells.first(), Some(&IVec2::new(0, 0)));
        assert_eq!(cells.last(), Some(&IVec2::new(3, 1)));
        for pair in cells.windows(2) {
            let d = pair[1] - pair[0];
            assert_eq!(d.x.abs() + d.y.abs(), 1, "4-connected: {cells:?}");
        }
    }

    #[test]
    fn diagonal_line_cannot_slip_between_corner_blocks() {
        let mut g = grid();
        g.add_obstacle(&Obb::new(Vec2::new(10.5, 11.5), Vec2::splat(0.5), 0.0), 1);
        g.add_obstacle(&Obb::new(Vec2::new(11.5, 10.5), Vec2::splat(0.5), 0.0), 1);
        assert!(!g.line_clear(Vec2::new(10.4, 10.4), Vec2::new(11.6, 11.6), 1, 0.3, 0));
    }

    #[test]
    fn stale_tables_fall_back_to_cell_checks() {
        let mut g = grid();
        g.prepare(1, 0);
        // Changed after prepare: &self queries must still see the obstacle.
        g.add_obstacle(&Obb::new(Vec2::new(10.5, 10.5), Vec2::splat(0.5), 0.0), 1);
        assert!(!g.clear_cell(IVec2::new(10, 10), 1, 0.4, 0));
        assert!(!g.line_clear(Vec2::new(5.5, 10.5), Vec2::new(15.5, 10.5), 1, 0.4, 0));
    }

    #[test]
    fn sync_tag_survives_clone() {
        let mut g = grid();
        assert_eq!(g.sync_tag(), 0);
        g.set_sync_tag(7);
        assert_eq!(g.clone().sync_tag(), 7);
    }
}
