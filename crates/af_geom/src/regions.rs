//! Connected regions of the terrain per mover class, and a coarse graph
//! over them that plans long routes in a few hundred steps.
//!
//! A *mover class* ([`RegionClass`]) is the set of terrain ids a mover may
//! enter (from its mobility mask) and its clearance `k` (see
//! [`NavGrid::clearance_cells`](crate::NavGrid::clearance_cells)). A cell is
//! *open* for the class when the whole `(2k+1)^2` square around it lies on
//! the map and on terrain the class enters. Obstacles, gates and teams are
//! ignored, so a [`RegionMap`] holds for as long as the terrain does (the
//! whole match: terrain never changes in play).
//!
//! * **Regions**: open cells joined by axis steps, as A* walks them
//!   (diagonals never cut corners, so they join nothing new). A goal in
//!   another region than the mover is out of reach however long one
//!   searches; [`RegionMap::nearest_in`] finds the reachable point nearest
//!   it instead.
//! * **Coarse graph**: the map is cut into [`BLOCK`] x [`BLOCK`] cell
//!   blocks. Each connected part of a block's open cells is a node,
//!   standing at its cell nearest the part's centroid, linked to the parts
//!   of the neighbouring blocks it touches (and to a diagonal neighbour's
//!   across a block corner when all four cells round the corner are open),
//!   at the distance between the two. [`RegionMap::route`] runs A* over
//!   it and pulls the result tight with straight lines over open cells: a
//!   route round a river to a ford 300 m off the straight line costs a few
//!   thousand coarse steps where a cell search would take 140,000.
//!
//! Everything is a pure function of the terrain snapshot (scan orders and
//! index tie-breaks only), so every run and every player sees the same
//! regions and routes.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use bevy_math::{IVec2, Vec2};

use crate::grid::{traverse_cells, FLYING};

/// Side of a coarse block, in cells.
pub const BLOCK: i32 = 8;

/// A region: 1, 2, ... in the order of their first coarse node (block by
/// block, row by row); 0 = no region (a closed cell, or off the map).
pub type RegionId = u32;

/// The terrain of a grid as it stood when taken
/// ([`NavGrid::terrain_snapshot`](crate::NavGrid::terrain_snapshot)): what
/// region maps are built from.
#[derive(Clone, Debug)]
pub struct TerrainSnapshot {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) cell: f32,
    pub(crate) origin: Vec2,
    pub(crate) terrain: Vec<u8>,
    pub(crate) masks: [u16; 256],
    pub(crate) stamp: u64,
}

impl TerrainSnapshot {
    /// The grid's [`terrain_stamp`](crate::NavGrid::terrain_stamp) when taken.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Cells of clearance needed around the centre cell for `radius` (as
    /// [`NavGrid::clearance_cells`](crate::NavGrid::clearance_cells)).
    pub fn clearance_cells(&self, radius: f32) -> i32 {
        ((radius / self.cell) - 0.5).ceil().max(0.0) as i32
    }

    /// The class of movers of `mobility` and half-width `radius`.
    pub fn class(&self, mobility: u16, radius: f32) -> RegionClass {
        let mut admits = [0u64; 4];
        for (t, mask) in self.masks.iter().enumerate() {
            if mobility & FLYING != 0 || mask & mobility != 0 {
                admits[t / 64] |= 1 << (t % 64);
            }
        }
        RegionClass { admits, k: self.clearance_cells(radius) }
    }
}

/// A mover class: the terrain ids it enters and its clearance in cells.
/// Movers of different mobilities that enter the same terrain (Foot and
/// Foot with a trait that only changes speeds) share one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RegionClass {
    admits: [u64; 4],
    k: i32,
}

impl RegionClass {
    /// Clearance in cells.
    pub fn clearance(&self) -> i32 {
        self.k
    }

    #[inline]
    fn admits(&self, terrain: u8) -> bool {
        let t = terrain as usize;
        self.admits[t / 64] & (1 << (t % 64)) != 0
    }
}

/// One connected part of a block's open cells.
#[derive(Clone, Copy, Debug)]
struct CoarseNode {
    /// Its cell nearest the part's centroid.
    rep: IVec2,
    region: RegionId,
    block: u32,
}

/// A link from one coarse node to a neighbouring one.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Link {
    to: u32,
    /// Metres between the two representative cells (straight: a route
    /// is pulled tight afterwards, and the detour through `exit` and
    /// `enter` would mostly make distances longer than the way is).
    len: f32,
    /// Where the two parts touch: the cell on this side and the cell on the other.
    exit: IVec2,
    enter: IVec2,
}

/// Regions and the coarse graph of one mover class (see the module docs).
#[derive(Clone, Debug)]
pub struct RegionMap {
    class: RegionClass,
    stamp: u64,
    width: i32,
    height: i32,
    cell: f32,
    origin: Vec2,
    /// Blocks per row.
    blocks_w: i32,
    /// Per cell: 0 = closed, else 1 + the index of its part within its block.
    part: Vec<u8>,
    /// Per block: index of its first node (its parts in order); one more at the end.
    first: Vec<u32>,
    nodes: Vec<CoarseNode>,
    /// Links of node `i`: `links[link_first[i]..link_first[i + 1]]`.
    link_first: Vec<u32>,
    links: Vec<Link>,
    /// Nodes of region `r`: `region_nodes[region_first[r - 1]..region_first[r]]`.
    region_first: Vec<u32>,
    region_nodes: Vec<u32>,
}

/// A coarse route: straight legs over open cells (for the class) from
/// where it starts to where it ends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoarseRoute {
    /// The start, the turns, the end.
    pub points: Vec<Vec2>,
    /// Distance along the route at each point (`cum[0] = 0`).
    pub cum: Vec<f32>,
    /// Coarse nodes A* expanded to find it.
    pub expansions: u32,
}

impl CoarseRoute {
    /// Total length (metres).
    pub fn length(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    pub fn end(&self) -> Vec2 {
        self.points.last().copied().unwrap_or_default()
    }

    /// Where `p` stands along the route: the leg it is nearest (searched
    /// from leg `from` on, so a mover that went on never maps back onto an
    /// earlier leg it happens to pass near) and the distance along the
    /// route of its projection onto that leg. Returns (leg, along, how
    /// far `p` is off the route).
    pub fn locate(&self, p: Vec2, from: usize) -> (usize, f32, f32) {
        if self.points.len() < 2 {
            return (0, 0.0, self.points.first().map_or(0.0, |q| q.distance(p)));
        }
        let mut best = (from.min(self.points.len() - 2), 0.0, f32::INFINITY);
        for i in from.min(self.points.len() - 2)..self.points.len() - 1 {
            let (a, b) = (self.points[i], self.points[i + 1]);
            let ab = b - a;
            let len = ab.length();
            let t = if len > 1e-6 { ((p - a).dot(ab) / (len * len)).clamp(0.0, 1.0) } else { 0.0 };
            let off = p.distance(a + ab * t);
            if off < best.2 {
                best = (i, self.cum[i] + t * len, off);
            }
        }
        best
    }

    /// The point `along` metres down the route (clamped to its ends).
    pub fn point_at(&self, along: f32) -> Vec2 {
        let Some(&first) = self.points.first() else { return Vec2::ZERO };
        if along <= 0.0 {
            return first;
        }
        for i in 0..self.points.len().saturating_sub(1) {
            if along <= self.cum[i + 1] {
                let len = self.cum[i + 1] - self.cum[i];
                let t = if len > 1e-6 { (along - self.cum[i]) / len } else { 1.0 };
                return self.points[i].lerp(self.points[i + 1], t);
            }
        }
        self.end()
    }
}

/// The next stretch of a route to search (see [`RegionMap::next_leg`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leg {
    /// Where to search to (on open ground for the class).
    pub target: Vec2,
    /// `target` is the route's end.
    pub last: bool,
    /// Route length left beyond `target` (0 on the last leg).
    pub beyond: f32,
    /// The route leg the mover was found on: pass it back as `from` next
    /// time, so it is never mapped back onto a leg it already walked.
    pub segment: usize,
    /// How far the mover stands off the route (metres).
    pub off: f32,
}

/// Route distances from one point to everywhere in its region (see
/// [`RegionMap::field`]): many targets ranked with one search.
#[derive(Clone, Debug)]
pub struct DistanceField {
    stamp: u64,
    class: RegionClass,
    from: Vec2,
    region: RegionId,
    /// Per coarse node: route distance to its representative cell (infinite
    /// when out of reach or past the field's limit).
    g: Vec<f32>,
}

impl DistanceField {
    /// Where it measures from.
    pub fn from(&self) -> Vec2 {
        self.from
    }

    pub fn region(&self) -> RegionId {
        self.region
    }

    /// Terrain stamp of the map it was computed on.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// Route distance (metres, coarse) to `p`: `None` when `p` is closed
    /// ground, in another region or past the field's limit. `map` must be
    /// the map the field was computed on.
    pub fn get(&self, map: &RegionMap, p: Vec2) -> Option<f32> {
        debug_assert!(map.class == self.class && map.stamp == self.stamp, "a field read through another map");
        let n = map.node_at(map.cell_of(p))?;
        let g = *self.g.get(n as usize)?;
        g.is_finite().then(|| g + map.rep_point(n).distance(p))
    }
}

/// Open-list entry of the coarse searches (a min-heap on `f`, ties by node).
#[derive(Clone, Copy, PartialEq)]
struct Open {
    f: f32,
    node: u32,
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.partial_cmp(&self.f).unwrap_or(Ordering::Equal).then_with(|| o.node.cmp(&self.node))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

const AXES: [IVec2; 4] = [IVec2::new(1, 0), IVec2::new(-1, 0), IVec2::new(0, 1), IVec2::new(0, -1)];

impl RegionMap {
    /// Label the regions and build the coarse graph of movers of
    /// `mobility` and half-width `radius` on terrain `t`. About a pass and a
    /// half over the cells: a few milliseconds for a 768 m map.
    pub fn build(t: &TerrainSnapshot, mobility: u16, radius: f32) -> RegionMap {
        let class = t.class(mobility, radius);
        let (w, h) = (t.width as i32, t.height as i32);
        let n = (w * h) as usize;
        let open = open_cells(t, &class);

        // Parts of each block, as coarse nodes.
        let blocks_w = (w + BLOCK - 1) / BLOCK;
        let blocks_h = (h + BLOCK - 1) / BLOCK;
        let mut part = vec![0u8; n];
        let mut first = Vec::with_capacity((blocks_w * blocks_h) as usize + 1);
        let mut nodes = Vec::new();
        let mut stack: Vec<IVec2> = Vec::new();
        let mut members: Vec<IVec2> = Vec::new();
        for by in 0..blocks_h {
            for bx in 0..blocks_w {
                let block = (by * blocks_w + bx) as u32;
                first.push(nodes.len() as u32);
                let (x0, y0) = (bx * BLOCK, by * BLOCK);
                let (x1, y1) = ((x0 + BLOCK).min(w), (y0 + BLOCK).min(h));
                let mut parts = 0u8;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = (y * w + x) as usize;
                        if !open[i] || part[i] != 0 {
                            continue;
                        }
                        parts += 1;
                        part[i] = parts;
                        members.clear();
                        stack.push(IVec2::new(x, y));
                        while let Some(c) = stack.pop() {
                            members.push(c);
                            for step in AXES {
                                let nc = c + step;
                                if nc.x < x0 || nc.y < y0 || nc.x >= x1 || nc.y >= y1 {
                                    continue;
                                }
                                let ni = (nc.y * w + nc.x) as usize;
                                if open[ni] && part[ni] == 0 {
                                    part[ni] = parts;
                                    stack.push(nc);
                                }
                            }
                        }
                        let centroid = members.iter().map(|c| c.as_vec2()).sum::<Vec2>() / members.len() as f32;
                        let rep = members
                            .iter()
                            .copied()
                            .min_by(|a, b| {
                                a.as_vec2().distance_squared(centroid).total_cmp(&b.as_vec2().distance_squared(centroid)).then((a.y, a.x).cmp(&(b.y, b.x)))
                            })
                            .unwrap_or(IVec2::new(x, y));
                        nodes.push(CoarseNode { rep, region: 0, block });
                    }
                }
            }
        }
        first.push(nodes.len() as u32);
        let mut map = RegionMap {
            class,
            stamp: t.stamp,
            width: w,
            height: h,
            cell: t.cell,
            origin: t.origin,
            blocks_w,
            part,
            first,
            nodes,
            link_first: Vec::new(),
            links: Vec::new(),
            region_first: Vec::new(),
            region_nodes: Vec::new(),
        };

        // Links across block borders and corners, each through the middle
        // one of the cell pairs where the two parts touch: along a border,
        // a run of touching pairs of the same two parts is one entry.
        let mut touching: Vec<(u32, u32, IVec2, IVec2)> = Vec::new();
        for x in (BLOCK..w).step_by(BLOCK as usize) {
            push_runs(&map, (0..h).map(|y| (IVec2::new(x - 1, y), IVec2::new(x, y))), &mut touching);
        }
        for y in (BLOCK..h).step_by(BLOCK as usize) {
            push_runs(&map, (0..w).map(|x| (IVec2::new(x, y - 1), IVec2::new(x, y))), &mut touching);
        }
        for y in (BLOCK..h).step_by(BLOCK as usize) {
            for x in (BLOCK..w).step_by(BLOCK as usize) {
                let (sw, ne, nw, se) = (IVec2::new(x - 1, y - 1), IVec2::new(x, y), IVec2::new(x - 1, y), IVec2::new(x, y - 1));
                if [sw, ne, nw, se].iter().all(|c| map.open_cell(*c)) {
                    push_runs(&map, [(sw, ne)].into_iter(), &mut touching);
                    push_runs(&map, [(nw, se)].into_iter(), &mut touching);
                }
            }
        }
        // Stable: each pair's runs stay in the order found.
        touching.sort_by_key(|t| (t.0, t.1));
        let mut pairs: Vec<(u32, u32, IVec2, IVec2)> = Vec::new();
        for group in touching.chunk_by(|x, y| (x.0, x.1) == (y.0, y.1)) {
            pairs.push(group[group.len() / 2]);
        }
        let count = map.nodes.len();
        let mut degree = vec![0u32; count + 1];
        for &(a, b, ..) in &pairs {
            degree[a as usize] += 1;
            degree[b as usize] += 1;
        }
        let mut link_first = Vec::with_capacity(count + 1);
        let mut acc = 0u32;
        for d in &degree[..count] {
            link_first.push(acc);
            acc += d;
        }
        link_first.push(acc);
        let mut fill = link_first.clone();
        let mut links = vec![Link { to: 0, len: 0.0, exit: IVec2::ZERO, enter: IVec2::ZERO }; acc as usize];
        for &(a, b, ca, cb) in &pairs {
            let len = map.rep_point(a).distance(map.rep_point(b));
            links[fill[a as usize] as usize] = Link { to: b, len, exit: ca, enter: cb };
            fill[a as usize] += 1;
            links[fill[b as usize] as usize] = Link { to: a, len, exit: cb, enter: ca };
            fill[b as usize] += 1;
        }
        map.link_first = link_first;
        map.links = links;

        // Regions: linked nodes, numbered in node order.
        let mut root: Vec<u32> = (0..count as u32).collect();
        fn find(root: &mut [u32], mut i: u32) -> u32 {
            while root[i as usize] != i {
                root[i as usize] = root[root[i as usize] as usize];
                i = root[i as usize];
            }
            i
        }
        for &(a, b, ..) in &pairs {
            let (ra, rb) = (find(&mut root, a), find(&mut root, b));
            if ra != rb {
                // The smaller index roots the merged set.
                let (lo, hi) = (ra.min(rb), ra.max(rb));
                root[hi as usize] = lo;
            }
        }
        let mut id_of = vec![0u32; count];
        let mut regions = 0u32;
        for i in 0..count {
            let r = find(&mut root, i as u32) as usize;
            if id_of[r] == 0 {
                regions += 1;
                id_of[r] = regions;
            }
            map.nodes[i].region = id_of[r];
        }
        let mut sizes = vec![0u32; regions as usize + 1];
        for node in &map.nodes {
            sizes[node.region as usize] += 1;
        }
        let mut region_first = Vec::with_capacity(regions as usize + 1);
        let mut acc = 0u32;
        region_first.push(0);
        for s in &sizes[1..] {
            acc += s;
            region_first.push(acc);
        }
        let mut fill = region_first.clone();
        let mut region_nodes = vec![0u32; count];
        for (i, node) in map.nodes.iter().enumerate() {
            let r = node.region as usize - 1;
            region_nodes[fill[r] as usize] = i as u32;
            fill[r] += 1;
        }
        map.region_first = region_first;
        map.region_nodes = region_nodes;
        map
    }

    /// The class it was built for.
    pub fn class(&self) -> RegionClass {
        self.class
    }

    /// Terrain stamp of the snapshot it was built from.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// Number of regions.
    pub fn region_count(&self) -> u32 {
        self.region_first.len().saturating_sub(1) as u32
    }

    /// Number of coarse nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    #[inline]
    fn cell_of(&self, p: Vec2) -> IVec2 {
        let l = (p - self.origin) / self.cell;
        IVec2::new(l.x.floor() as i32, l.y.floor() as i32)
    }

    #[inline]
    fn cell_center(&self, c: IVec2) -> Vec2 {
        self.origin + (c.as_vec2() + Vec2::splat(0.5)) * self.cell
    }

    #[inline]
    fn in_bounds(&self, c: IVec2) -> bool {
        c.x >= 0 && c.y >= 0 && c.x < self.width && c.y < self.height
    }

    /// The coarse node (block part) of cell `c`; `None` when closed or off the map.
    #[inline]
    fn node_at(&self, c: IVec2) -> Option<u32> {
        if !self.in_bounds(c) {
            return None;
        }
        let p = self.part[(c.y * self.width + c.x) as usize];
        if p == 0 {
            return None;
        }
        let block = (c.y / BLOCK) * self.blocks_w + c.x / BLOCK;
        Some(self.first[block as usize] + p as u32 - 1)
    }

    #[inline]
    fn rep_point(&self, n: u32) -> Vec2 {
        self.cell_center(self.nodes[n as usize].rep)
    }

    /// Is cell `c` open for the class?
    #[inline]
    pub fn open_cell(&self, c: IVec2) -> bool {
        self.in_bounds(c) && self.part[(c.y * self.width + c.x) as usize] != 0
    }

    /// Is the cell under `p` open for the class?
    pub fn open_at(&self, p: Vec2) -> bool {
        self.open_cell(self.cell_of(p))
    }

    /// The region of the cell under `p` (0 when it is closed or off the map).
    pub fn region_at(&self, p: Vec2) -> RegionId {
        self.node_at(self.cell_of(p)).map_or(0, |n| self.nodes[n as usize].region)
    }

    /// The open cell nearest `p` within `max_dist` metres (`p`'s own cell
    /// when open), searching square rings outward: its centre (or `p`
    /// itself when its cell is open).
    pub fn open_near(&self, p: Vec2, max_dist: f32) -> Option<Vec2> {
        let c = self.cell_of(p);
        if self.open_cell(c) {
            return Some(p);
        }
        let max_r = (max_dist / self.cell).ceil() as i32;
        for r in 1..=max_r {
            let mut best: Option<(f32, IVec2)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let cc = c + IVec2::new(dx, dy);
                    if self.open_cell(cc) {
                        let d = self.cell_center(cc).distance_squared(p);
                        if best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, cc));
                        }
                    }
                }
            }
            if let Some((d, cc)) = best
                && d <= max_dist * max_dist + 1e-3
            {
                return Some(self.cell_center(cc));
            }
        }
        None
    }

    /// The region of `p`, or of the open cell nearest it within `max_dist`
    /// (0 when there is none): where a mover standing at `p` belongs,
    /// although a shove may have put it on a cell it could not choose.
    pub fn region_near(&self, p: Vec2, max_dist: f32) -> RegionId {
        self.open_near(p, max_dist).map_or(0, |q| self.region_at(q))
    }

    /// Does some open cell of `region` have its centre within `radius` of
    /// `p`? (Can a mover of the region get that close to `p`?)
    pub fn touches(&self, region: RegionId, p: Vec2, radius: f32) -> bool {
        if region == 0 {
            return false;
        }
        if self.region_at(p) == region {
            return true;
        }
        let (c0, c1) = (self.cell_of(p - Vec2::splat(radius)), self.cell_of(p + Vec2::splat(radius)));
        let r2 = radius * radius;
        for y in c0.y.max(0)..=c1.y.min(self.height - 1) {
            for x in c0.x.max(0)..=c1.x.min(self.width - 1) {
                let c = IVec2::new(x, y);
                if self.cell_center(c).distance_squared(p) <= r2 && self.node_at(c).is_some_and(|n| self.nodes[n as usize].region == region) {
                    return true;
                }
            }
        }
        false
    }

    /// The point of `region` nearest `p`: `p` itself when its cell belongs
    /// to it, else the centre of its nearest cell (`None` for no region).
    /// Coarse blocks are visited nearest first and the search stops once
    /// no block left can hold anything nearer, so it costs about the cells
    /// of the few blocks round the answer.
    pub fn nearest_in(&self, region: RegionId, p: Vec2) -> Option<Vec2> {
        if region == 0 || region > self.region_count() {
            return None;
        }
        if self.region_at(p) == region {
            return Some(p);
        }
        let list = &self.region_nodes[self.region_first[region as usize - 1] as usize..self.region_first[region as usize] as usize];
        let size = self.cell * BLOCK as f32;
        let mut candidates: Vec<(f32, u32)> = list
            .iter()
            .map(|&n| {
                let b = self.nodes[n as usize].block as i32;
                let lo = self.origin + IVec2::new(b % self.blocks_w, b / self.blocks_w).as_vec2() * size;
                let hi = lo + Vec2::splat(size);
                let gap = (lo - p).max(p - hi).max(Vec2::ZERO);
                (gap.length(), n)
            })
            .collect();
        candidates.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut best: Option<(f32, IVec2)> = None;
        for (bound, n) in candidates {
            if best.is_some_and(|(d, _)| bound * bound > d) {
                break;
            }
            let node = self.nodes[n as usize];
            let local = (n - self.first[node.block as usize] + 1) as u8;
            let (bx, by) = (node.block as i32 % self.blocks_w, node.block as i32 / self.blocks_w);
            for y in by * BLOCK..((by + 1) * BLOCK).min(self.height) {
                for x in bx * BLOCK..((bx + 1) * BLOCK).min(self.width) {
                    let c = IVec2::new(x, y);
                    if self.part[(y * self.width + x) as usize] != local {
                        continue;
                    }
                    let d = self.cell_center(c).distance_squared(p);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, c));
                    }
                }
            }
        }
        best.map(|(_, c)| self.cell_center(c))
    }

    /// The first cell of `region` on the way from `to` back to `from` (its
    /// centre): the bank facing `to` across water or rock, as seen from
    /// `from`. Costs a walk along the line; `None` when the line never
    /// enters the region.
    pub fn facing(&self, region: RegionId, from: Vec2, to: Vec2) -> Option<Vec2> {
        let mut found = None;
        traverse_cells(self.origin, self.cell, to, from, |c| {
            if self.node_at(c).is_some_and(|n| self.nodes[n as usize].region == region) {
                found = Some(self.cell_center(c));
                false
            } else {
                true
            }
        });
        found
    }

    /// Every cell the segment `a`-`b` crosses is open for the class.
    pub fn line_open(&self, a: Vec2, b: Vec2) -> bool {
        traverse_cells(self.origin, self.cell, a, b, |c| self.open_cell(c))
    }

    /// The coarse route from `from` to `to` (both on open cells of one
    /// region, else `None`): A* over the coarse graph, pulled tight with
    /// straight lines over open cells. Obstacles are not looked at: the
    /// route says which way round the terrain to go, and a cell search
    /// walks each leg of it.
    pub fn route(&self, from: Vec2, to: Vec2) -> Option<CoarseRoute> {
        let s = self.node_at(self.cell_of(from))?;
        let t = self.node_at(self.cell_of(to))?;
        if self.nodes[s as usize].region != self.nodes[t as usize].region {
            return None;
        }
        let mut expansions = 0;
        let mut points = vec![from];
        if s != t && !self.line_open(from, to) {
            points.extend(self.astar(s, t, from, to, &mut expansions)?);
        }
        points.push(to);
        let points = self.pull(&points);
        let mut cum = Vec::with_capacity(points.len());
        let mut acc = 0.0;
        cum.push(0.0);
        for pair in points.windows(2) {
            acc += pair[0].distance(pair[1]);
            cum.push(acc);
        }
        Some(CoarseRoute { points, cum, expansions })
    }

    /// The next leg of `route` to search for a mover at `p` (found on the
    /// route from leg `from` on): about `leg` metres down the route from
    /// where it stands, ending at the farthest turn of the route between a
    /// quarter and one and a quarter legs on when there is one (a corner:
    /// the legs on either side are straight lines), else `leg` metres on;
    /// the route's end once that is within a leg and a half.
    pub fn next_leg(&self, route: &CoarseRoute, p: Vec2, from: usize, leg: f32) -> Leg {
        let (segment, along, off) = route.locate(p, from);
        let length = route.length();
        if along + leg * 1.5 >= length {
            return Leg { target: route.end(), last: true, beyond: 0.0, segment, off };
        }
        let turn = (segment + 1..route.points.len() - 1).rev().find(|&i| route.cum[i] > along + leg * 0.25 && route.cum[i] <= along + leg * 1.25);
        let (at, mut target) = match turn {
            Some(i) => (route.cum[i], route.points[i]),
            None => (along + leg, route.point_at(along + leg)),
        };
        if !self.open_at(target)
            && let Some(q) = self.open_near(target, BLOCK as f32 * self.cell)
        {
            target = q;
        }
        Leg { target, last: false, beyond: length - at, segment, off }
    }

    /// Route distance (metres) from `from` to `to`: the straight line when
    /// it stays on open cells, else the coarse route's length; `None` when
    /// either end is closed or they lie in different regions.
    pub fn distance(&self, from: Vec2, to: Vec2) -> Option<f32> {
        let s = self.node_at(self.cell_of(from))?;
        let t = self.node_at(self.cell_of(to))?;
        if self.nodes[s as usize].region != self.nodes[t as usize].region {
            return None;
        }
        if self.line_open(from, to) {
            return Some(from.distance(to));
        }
        self.route(from, to).map(|r| r.length())
    }

    /// Route distances from `from` (on an open cell) to every coarse node
    /// of its region up to `max` metres (Dijkstra over the coarse graph):
    /// for ranking many targets by route distance at the cost of one search.
    pub fn field(&self, from: Vec2, max: f32) -> Option<DistanceField> {
        let s = self.node_at(self.cell_of(from))?;
        let mut g = vec![f32::INFINITY; self.nodes.len()];
        let mut closed = vec![false; self.nodes.len()];
        let mut open = BinaryHeap::new();
        g[s as usize] = from.distance(self.rep_point(s));
        open.push(Open { f: g[s as usize], node: s });
        while let Some(Open { node, .. }) = open.pop() {
            if std::mem::replace(&mut closed[node as usize], true) {
                continue;
            }
            let gn = g[node as usize];
            for link in self.links_of(node) {
                let ng = gn + link.len;
                if ng <= max && ng < g[link.to as usize] {
                    g[link.to as usize] = ng;
                    open.push(Open { f: ng, node: link.to });
                }
            }
        }
        Some(DistanceField { stamp: self.stamp, class: self.class, from, region: self.nodes[s as usize].region, g })
    }

    #[inline]
    fn links_of(&self, n: u32) -> &[Link] {
        &self.links[self.link_first[n as usize] as usize..self.link_first[n as usize + 1] as usize]
    }

    /// The points of the shortest chain of coarse nodes from `s` to `t`:
    /// each node's representative cell and, between two nodes, the cells
    /// where they touch; `None` when not linked.
    fn astar(&self, s: u32, t: u32, from: Vec2, to: Vec2, expansions: &mut u32) -> Option<Vec<Vec2>> {
        let n = self.nodes.len();
        let mut g = vec![f32::INFINITY; n];
        // The link (index into `links`) each node was reached by.
        let mut came = vec![u32::MAX; n];
        let mut parent = vec![u32::MAX; n];
        let mut closed = vec![false; n];
        let mut open = BinaryHeap::new();
        g[s as usize] = from.distance(self.rep_point(s));
        open.push(Open { f: g[s as usize] + self.rep_point(s).distance(to), node: s });
        let mut found = false;
        while let Some(Open { node, .. }) = open.pop() {
            if std::mem::replace(&mut closed[node as usize], true) {
                continue;
            }
            *expansions += 1;
            if node == t {
                found = true;
                break;
            }
            let gn = g[node as usize];
            let base = self.link_first[node as usize];
            for (i, link) in self.links_of(node).iter().enumerate() {
                let m = link.to as usize;
                if closed[m] {
                    continue;
                }
                let ng = gn + link.len;
                if ng < g[m] {
                    g[m] = ng;
                    came[m] = base + i as u32;
                    parent[m] = node;
                    open.push(Open { f: ng + self.rep_point(link.to).distance(to), node: link.to });
                }
            }
        }
        if !found {
            return None;
        }
        let mut points = vec![self.rep_point(t)];
        let mut cur = t;
        while cur != s {
            let link = self.links[came[cur as usize] as usize];
            points.push(self.cell_center(link.enter));
            points.push(self.cell_center(link.exit));
            cur = parent[cur as usize];
            points.push(self.rep_point(cur));
        }
        points.reverse();
        Some(points)
    }

    /// Pull a chain of points tight: from each anchor, on to the farthest
    /// following point still in a straight line over open cells.
    fn pull(&self, pts: &[Vec2]) -> Vec<Vec2> {
        let mut out = Vec::with_capacity(pts.len().min(16));
        let Some(&start) = pts.first() else { return out };
        out.push(start);
        let mut anchor = start;
        let mut i = 1;
        while i < pts.len() {
            let mut j = i;
            while j + 1 < pts.len() && self.line_open(anchor, pts[j + 1]) {
                j += 1;
            }
            out.push(pts[j]);
            anchor = pts[j];
            i = j + 1;
        }
        out.dedup();
        out
    }
}

/// For each run of consecutive cell pairs (one cell either side of a block
/// border) whose cells belong to the same two parts, the middle pair, as
/// (lower node, higher node, its cell, the other's cell).
fn push_runs(map: &RegionMap, cells: impl Iterator<Item = (IVec2, IVec2)>, out: &mut Vec<(u32, u32, IVec2, IVec2)>) {
    let mut run: Vec<(u32, u32, IVec2, IVec2)> = Vec::new();
    let flush = |run: &mut Vec<(u32, u32, IVec2, IVec2)>, out: &mut Vec<(u32, u32, IVec2, IVec2)>| {
        if !run.is_empty() {
            out.push(run[run.len() / 2]);
            run.clear();
        }
    };
    for (ca, cb) in cells {
        let pair = match (map.node_at(ca), map.node_at(cb)) {
            (Some(a), Some(b)) if a < b => Some((a, b, ca, cb)),
            (Some(a), Some(b)) => Some((b, a, cb, ca)),
            _ => None,
        };
        match pair {
            Some(p) if run.last().is_some_and(|r| (r.0, r.1) == (p.0, p.1)) => run.push(p),
            Some(p) => {
                flush(&mut run, out);
                run.push(p);
            }
            None => flush(&mut run, out),
        }
    }
    flush(&mut run, out);
}

/// Open cells of `class` on `t`: the whole `(2k+1)^2` square round the
/// cell on the map and on terrain the class enters.
fn open_cells(t: &TerrainSnapshot, class: &RegionClass) -> Vec<bool> {
    let (w, h, k) = (t.width as i32, t.height as i32, class.k);
    let n = (w * h) as usize;
    let mut open: Vec<bool> = t.terrain.iter().map(|&id| class.admits(id)).collect();
    if k == 0 {
        return open;
    }
    // Summed-area table of closed cells.
    let stride = (w + 1) as usize;
    let mut sat = vec![0u32; stride * (h as usize + 1)];
    for y in 0..h as usize {
        let mut row = 0u32;
        for x in 0..w as usize {
            row += u32::from(!open[y * w as usize + x]);
            sat[(y + 1) * stride + x + 1] = sat[y * stride + x + 1] + row;
        }
    }
    let rect = |x0: i32, y0: i32, x1: i32, y1: i32| {
        let (x0, y0, x1, y1) = (x0 as usize, y0 as usize, x1 as usize, y1 as usize);
        (sat[(y1 + 1) * stride + x1 + 1] + sat[y0 * stride + x0]) - (sat[y0 * stride + x1 + 1] + sat[(y1 + 1) * stride + x0])
    };
    let mut squared = vec![false; n];
    for y in k..h - k {
        for x in k..w - k {
            squared[(y * w + x) as usize] = rect(x - k, y - k, x + k, y + k) == 0;
        }
    }
    open = squared;
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::NavGrid;
    use crate::obb::Obb;

    const GRASS: u8 = 0;
    const DEEP: u8 = 1;
    const FORD: u8 = 2;
    const ROCK: u8 = 3;
    const FOOT: u16 = 1;
    const WHEELS: u16 = 2;

    /// Grass (everyone), deep water (nobody), a ford (everyone), rock (nobody).
    fn grid(n: u32) -> NavGrid {
        let mut g = NavGrid::new(n, n, 1.0, Vec2::ZERO);
        g.set_terrain_info(GRASS, FOOT | WHEELS, true);
        g.set_terrain_info(DEEP, 0, false);
        g.set_terrain_info(FORD, FOOT | WHEELS, false);
        g.set_terrain_info(ROCK, 0, false);
        g
    }

    /// A river 12 m wide down x = cx - 6 .. cx + 6, with 8 m fords centred on `fords` (y).
    fn river_x(g: &mut NavGrid, cx: i32, fords: &[i32]) {
        for y in 0..g.height() as i32 {
            for x in cx - 6..cx + 6 {
                let t = if fords.iter().any(|f| (y - f).abs() < 4) { FORD } else { DEEP };
                g.set_terrain(IVec2::new(x, y), t);
            }
        }
    }

    /// The same across, along z = cy - 6 .. cy + 6, fords centred on `fords` (x).
    fn river_y(g: &mut NavGrid, cy: i32, fords: &[i32]) {
        for x in 0..g.width() as i32 {
            for y in cy - 6..cy + 6 {
                let t = if fords.iter().any(|f| (x - f).abs() < 4) { FORD } else { DEEP };
                g.set_terrain(IVec2::new(x, y), t);
            }
        }
    }

    fn map(g: &NavGrid, mobility: u16, radius: f32) -> RegionMap {
        RegionMap::build(&g.terrain_snapshot(), mobility, radius)
    }

    /// Every pair of open cells a few metres apart agrees with a flood fill:
    /// same region exactly when one reaches the other by axis steps.
    #[test]
    fn regions_match_a_flood_fill() {
        let mut g = grid(70);
        river_x(&mut g, 35, &[10]);
        // A rock wall closing a pocket in the corner, and a 2-cell gap in another.
        for i in 0..20 {
            g.set_terrain(IVec2::new(i, 55), ROCK);
            g.set_terrain(IVec2::new(55, 50 + i.min(19)), ROCK);
        }
        for y in 55..70 {
            g.set_terrain(IVec2::new(20, y), ROCK);
        }
        for k_radius in [0.4f32, 1.4, 2.4] {
            let m = map(&g, FOOT, k_radius);
            let k = g.clearance_cells(k_radius);
            let open = |c: IVec2| (-k..=k).all(|dy| (-k..=k).all(|dx| g.terrain_passable(c + IVec2::new(dx, dy), FOOT)));
            // Flood from each open cell's region representative.
            let mut label = vec![0u32; 70 * 70];
            let mut next = 0;
            for y in 0..70 {
                for x in 0..70 {
                    let c = IVec2::new(x, y);
                    if !open(c) || label[(y * 70 + x) as usize] != 0 {
                        continue;
                    }
                    next += 1;
                    let mut stack = vec![c];
                    label[(y * 70 + x) as usize] = next;
                    while let Some(c) = stack.pop() {
                        for s in AXES {
                            let nc = c + s;
                            if g.in_bounds(nc) && open(nc) && label[(nc.y * 70 + nc.x) as usize] == 0 {
                                label[(nc.y * 70 + nc.x) as usize] = next;
                                stack.push(nc);
                            }
                        }
                    }
                }
            }
            assert_eq!(m.region_count(), next, "radius {k_radius}");
            for y in 0..70 {
                for x in 0..70 {
                    let c = IVec2::new(x, y);
                    assert_eq!(m.open_cell(c), open(c), "{c} radius {k_radius}");
                    // Labels agree up to renaming: compare against a fixed partner cell.
                    for other in [IVec2::new(3, 3), IVec2::new(60, 3), IVec2::new(5, 65), IVec2::new(66, 66)] {
                        if open(c) && open(other) {
                            let same = label[(y * 70 + x) as usize] == label[(other.y * 70 + other.x) as usize];
                            assert_eq!(m.region_at(g.cell_center(c)) == m.region_at(g.cell_center(other)), same, "{c} vs {other} radius {k_radius}");
                        }
                    }
                }
            }
        }
    }

    /// A river with no ford splits the map; a ford joins it again, but not
    /// for a mover too wide for it; a terrain the class may not enter (the
    /// ford, for a mover without it) splits it too.
    #[test]
    fn rivers_split_and_fords_join() {
        let mut g = grid(96);
        river_x(&mut g, 48, &[]);
        let m = map(&g, FOOT, 0.4);
        let (west, east) = (Vec2::new(20.5, 40.5), Vec2::new(75.5, 40.5));
        assert_eq!(m.region_count(), 2);
        assert_ne!(m.region_at(west), m.region_at(east));
        assert_eq!(m.distance(west, east), None);
        river_x(&mut g, 48, &[80]);
        let m = map(&g, FOOT, 0.4);
        assert_eq!(m.region_count(), 1);
        assert!(m.distance(west, east).is_some_and(|d| d > 75.0), "round by the ford");
        // An 8 m ford takes clearance 3 (7 cells) but not 4 (9 cells).
        assert_eq!(map(&g, FOOT, 3.0).region_count(), 1);
        assert_eq!(map(&g, FOOT, 4.0).region_count(), 2);
        // Wheels do not enter a ford that only feet do.
        g.set_terrain_info(FORD, FOOT, false);
        assert_eq!(map(&g, WHEELS, 0.4).region_count(), 2);
        assert_eq!(map(&g, FOOT, 0.4).region_count(), 1);
    }

    /// Obstacles never split regions: they come and go during a match.
    #[test]
    fn obstacles_do_not_count() {
        let mut g = grid(64);
        g.add_obstacle(&Obb::new(Vec2::new(32.0, 32.0), Vec2::new(0.5, 32.0), 0.0), 1);
        assert_eq!(map(&g, FOOT, 0.4).region_count(), 1);
    }

    /// The point of a region nearest a goal across the water is on the
    /// near bank facing it; one inside the region is itself.
    #[test]
    fn nearest_in_finds_the_facing_bank() {
        let mut g = grid(96);
        river_x(&mut g, 48, &[]);
        let m = map(&g, FOOT, 0.4);
        let west = m.region_at(Vec2::new(20.5, 40.5));
        let p = m.nearest_in(west, Vec2::new(75.5, 40.3)).expect("a bank");
        assert!((p - Vec2::new(41.5, 40.5)).length() < 0.01, "{p}");
        assert_eq!(m.facing(west, Vec2::new(20.5, 40.5), Vec2::new(75.5, 40.5)), Some(Vec2::new(41.5, 40.5)), "the facing bank");
        assert_eq!(m.facing(west, Vec2::new(80.5, 40.5), Vec2::new(75.5, 40.5)), None, "a line that never enters the region");
        assert_eq!(m.nearest_in(west, Vec2::new(10.2, 3.3)), Some(Vec2::new(10.2, 3.3)));
        // Brute force over a few goals.
        for goal in [Vec2::new(60.0, 5.0), Vec2::new(95.0, 95.0), Vec2::new(47.0, 47.0)] {
            let p = m.nearest_in(west, goal).expect("a point");
            let best = (0..96 * 96)
                .map(|i| IVec2::new(i % 96, i / 96))
                .filter(|c| m.region_at(g.cell_center(*c)) == west)
                .map(|c| g.cell_center(c).distance(goal))
                .fold(f32::INFINITY, f32::min);
            assert!((p.distance(goal) - best).abs() < 1e-3, "{goal}: {p} at {} vs {best}", p.distance(goal));
        }
    }

    /// The coarse route round a river by a ford far off the straight line
    /// crosses at the ford, stays on open cells and is about as long as
    /// the way round; finding it takes a few hundred coarse steps.
    #[test]
    fn a_route_goes_round_by_the_ford() {
        let mut g = grid(256);
        river_x(&mut g, 128, &[228]);
        let m = map(&g, FOOT, 0.4);
        let (from, to) = (Vec2::new(88.5, 128.5), Vec2::new(168.5, 128.5));
        let r = m.route(from, to).expect("a route");
        assert_eq!((r.points[0], r.end()), (from, to));
        for pair in r.points.windows(2) {
            assert!(m.line_open(pair[0], pair[1]), "{r:?}");
        }
        assert!(r.points.iter().any(|p| p.y > 210.0), "by the ford: {r:?}");
        let round = 2.0 * Vec2::new(34.0, 100.0).length() + 12.0;
        assert!(r.length() > round * 0.95 && r.length() < round * 1.15, "{} vs {round}", r.length());
        assert!(r.expansions < 1_500, "{} coarse steps", r.expansions);
        // Distance: the straight line where it is open, the route where not.
        assert_eq!(m.distance(from, Vec2::new(88.5, 20.5)), Some(108.0));
        assert_eq!(m.distance(from, to), Some(r.length()));
    }

    /// Locating a point along a route and walking a distance down it.
    #[test]
    fn locate_and_point_at() {
        let r = CoarseRoute { points: vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)], cum: vec![0.0, 10.0, 20.0], expansions: 0 };
        assert_eq!(r.point_at(5.0), Vec2::new(5.0, 0.0));
        assert_eq!(r.point_at(15.0), Vec2::new(10.0, 5.0));
        assert_eq!(r.point_at(50.0), Vec2::new(10.0, 10.0));
        let (leg, along, off) = r.locate(Vec2::new(11.0, 4.0), 0);
        assert!(leg == 1 && (along - 14.0).abs() < 1e-4 && (off - 1.0).abs() < 1e-4, "{leg} {along} {off}");
        let (leg, along, off) = r.locate(Vec2::new(3.0, 1.0), 1);
        assert!(leg == 1 && (along - 11.0).abs() < 1e-4 && (off - 7.0).abs() < 1e-4, "never back onto an earlier leg: {leg} {along} {off}");
    }

    /// A distance field ranks like the route distances it stands for.
    #[test]
    fn a_field_measures_route_distances() {
        let mut g = grid(128);
        river_x(&mut g, 64, &[110]);
        let m = map(&g, FOOT, 0.4);
        let home = Vec2::new(30.5, 30.5);
        let f = m.field(home, f32::INFINITY).expect("a field");
        let (near_across, far_same_side) = (Vec2::new(90.5, 30.5), Vec2::new(30.5, 120.5));
        let across = f.get(&m, near_across).expect("reachable");
        let same = f.get(&m, far_same_side).expect("reachable");
        assert!(across > same, "across the river ({across}) is farther than 90 m up this bank ({same})");
        let exact = m.distance(home, near_across).expect("a route");
        assert!(across >= exact - 1.0 && across < exact * 1.15, "field {across} vs route {exact}");
        assert_eq!(f.get(&m, Vec2::new(64.0, 30.5)), None, "in the river");
        let short = m.field(home, 40.0).expect("a field");
        assert_eq!(short.get(&m, far_same_side), None, "past the limit");
    }

    /// Building is deterministic, and two mobilities entering the same
    /// terrain share a class.
    #[test]
    fn classes_and_determinism() {
        let mut g = grid(64);
        river_x(&mut g, 32, &[50]);
        let t = g.terrain_snapshot();
        assert_eq!(t.class(FOOT, 0.4), t.class(WHEELS, 0.3));
        assert_ne!(t.class(FOOT, 0.4), t.class(FOOT, 1.4));
        let (a, b) = (RegionMap::build(&t, FOOT, 0.4), RegionMap::build(&t, FOOT, 0.4));
        assert_eq!(a.part, b.part);
        assert_eq!(a.links, b.links);
        assert_eq!(a.route(Vec2::new(5.5, 5.5), Vec2::new(60.5, 5.5)), b.route(Vec2::new(5.5, 5.5), Vec2::new(60.5, 5.5)));
        // The stamp follows the terrain.
        let before = g.terrain_stamp();
        g.add_obstacle(&Obb::new(Vec2::new(5.0, 5.0), Vec2::splat(1.0), 0.0), 1);
        assert_eq!(g.terrain_stamp(), before, "obstacles are not terrain");
        g.set_terrain(IVec2::new(1, 1), ROCK);
        assert_ne!(g.terrain_stamp(), before);
        assert_eq!(g.clone().terrain_stamp(), g.terrain_stamp());
    }

    /// Two rivers crossing in a plus, each with one ford: every quadrant is
    /// joined (each ford on a river arm links two), routes cross at the
    /// fords, and with the ford of one arm gone a quadrant pair splits off.
    #[test]
    fn two_crossing_rivers_with_fords() {
        let mut g = grid(160);
        // The arms: x-river fords at y = 30 (south arm) and y = 130 (north
        // arm); y-river fords at x = 30 (west arm) and x = 130 (east arm).
        river_x(&mut g, 80, &[30, 130]);
        river_y(&mut g, 80, &[30, 130]);
        let m = map(&g, FOOT, 0.4);
        assert_eq!(m.region_count(), 1);
        let (sw, ne) = (Vec2::new(40.5, 40.5), Vec2::new(120.5, 120.5));
        let r = m.route(sw, ne).expect("a route");
        assert!(r.length() > sw.distance(ne) * 1.2, "round by two fords: {r:?}");
        river_x(&mut g, 80, &[130]);
        river_y(&mut g, 80, &[130]);
        let m = map(&g, FOOT, 0.4);
        // South-west is cut off: no ford on the south or west arm.
        assert_eq!(m.region_count(), 2);
        assert_eq!(m.route(sw, ne), None);
    }
}
