//! Keeping the base walkable: a new building never closes a ring.
//!
//! Buildings keep a gap from each other (see `SpotQuery::gap`), but no gap
//! the AI keeps is wide enough for every mover. A mover stands only on nav
//! cells whose centres clear every obstacle by the walk margin, and one
//! wider than a cell needs a whole square of them
//! (`NavGrid::clearance_cells`): a ram, a bombard or a clockwork automaton
//! needs a lane of three such cells, about 2.6 to 3.6 m between two
//! buildings; a worker needs 0.95 m (af_movement closes narrower slots). So
//! a ring of buildings, however its gaps look, can close for the wide
//! movers, and whatever stands inside then (a unit trained at a production
//! building whose exit lies within, one that walked in before the ring
//! closed) is trapped for the rest of the match.
//!
//! [`seals_pocket`] guards every spot the AI places on: with the new
//! footprint, no ground that any kind of the realm's ground units could
//! leave before (reach the edge of a window round the spot) may become
//! enclosed. The spiral search then simply tries its next spot, so rings
//! stay open where it matters.

use af_core::prelude::*;
use bevy::prelude::*;

use crate::view::AiView;

/// af_movement's walk margin: movers only use nav cells whose centre is
/// farther than this from every obstacle (metres). The nav grid reports it
/// once af_movement registered the map's obstacles; tests without
/// af_movement get it from here.
pub const WALK_MARGIN: f32 = 0.3;
/// af_movement closes slots between two obstacles narrower than this
/// (metres) to every mover, workers included.
pub const PINCH_WIDTH: f32 = 0.95;
/// A pocket is looked for this far (metres) round the new footprint: rings
/// up to about this size are kept open.
pub const POCKET_WINDOW: f32 = 28.0;

/// How one kind of the realm's ground units moves on the nav grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mover {
    /// `NavGrid::clearance_cells` of its radius.
    pub clearance: i32,
    /// Its mobility mask.
    pub mobility: u16,
}

/// The distinct movers among the faction's ground units, its worker
/// included, sorted.
pub fn ground_movers(view: &AiView) -> Vec<Mover> {
    let (data, me) = (view.data, view.me);
    let faction = view.faction();
    let mut out: Vec<Mover> = Vec::new();
    for u in std::iter::once(faction.worker_id).chain(faction.unit_ids.iter().copied()) {
        let def = data.unit(u);
        if def.is_flying() {
            continue;
        }
        // As `spawn_unit` sets `Mobile::radius`.
        let radius = Footprint::from_size(def.size).half.x.max(0.2);
        let m = Mover { clearance: view.nav.clearance_cells(radius), mobility: data.unit_mobility(u, me.faction).0 };
        if m.mobility & af_geom::FLYING == 0 && !out.contains(&m) {
            out.push(m);
        }
    }
    out.sort_unstable();
    out
}

/// Would putting the footprints `new` down (with the pending footprints
/// `extra`, which stand either way) enclose ground that one of `movers`
/// can leave now? "Leave" means reach the edge of a window [`POCKET_WINDOW`]
/// round `new` (map edges aside); obstacles are what the nav grid holds
/// (terrain, buildings, blocking nodes, other teams' gates) plus every
/// building and blocking node in the spatial index and `extra`, each
/// closing the cells within the walk margin of its box (for workers, half
/// [`PINCH_WIDTH`]: slots too narrow for them are closed too). Ground that
/// was enclosed already does not count: only pockets the new footprints
/// close.
pub fn seals_pocket(view: &AiView, movers: &[Mover], new: &[Obb], extra: &[Obb]) -> bool {
    let nav = view.nav;
    let Some(pad) = movers.iter().map(|m| m.clearance.max(0)).max() else { return false };
    let Some(first) = new.first() else { return false };
    if nav.width() == 0 || nav.height() == 0 {
        return false;
    }
    let (mut min, mut max) = first.aabb();
    for o in &new[1..] {
        let (a, b) = o.aabb();
        min = min.min(a);
        max = max.max(b);
    }
    let grid = IVec2::new(nav.width() as i32, nav.height() as i32);
    let reach = Vec2::splat(POCKET_WINDOW);
    let (raw_lo, raw_hi) = (nav.cell_of(min - reach), nav.cell_of(max + reach));
    let (lo, hi) = (raw_lo.max(IVec2::ZERO), raw_hi.min(grid - IVec2::ONE));
    if lo.x > hi.x || lo.y > hi.y {
        return false;
    }
    let win = Window {
        lo,
        size: hi - lo + IVec2::ONE,
        pad,
        // Open ground lies beyond these sides (west, east, south, north);
        // the map's edge is no way out.
        open: [raw_lo.x > 0, raw_hi.x < grid.x - 1, raw_lo.y > 0, raw_hi.y < grid.y - 1],
    };
    if !win.open.iter().any(|o| *o) {
        return false;
    }
    // The boxes standing either way: the spatial index's (own gates let
    // own units through) and the pending footprints.
    let team = view.me.team;
    let mut standing: Vec<Obb> = extra.to_vec();
    let centre = (min + max) * 0.5;
    let radius = ((max - min) * 0.5 + reach + Vec2::splat((pad + 2) as f32 * nav.cell_size())).length();
    view.spatial.for_each_near(centre, radius, |e| match e.kind {
        SpatialKind::Building if !(e.team == team && view.data.building(BuildingTypeId(e.type_index)).gate) => standing.push(e.obb),
        SpatialKind::Node if e.blocking => standing.push(e.obb),
        _ => {}
    });
    let walk = nav.walk_margin().max(WALK_MARGIN);
    let pinch = walk.max(PINCH_WIDTH * 0.5);
    let mut terrain: Vec<(u16, Vec<bool>)> = Vec::new();
    let mut rasters: Vec<(bool, Vec<bool>, Vec<bool>)> = Vec::new();
    for m in movers {
        if !terrain.iter().any(|(mob, _)| *mob == m.mobility) {
            terrain.push((m.mobility, win.passable(nav, m.mobility, team)));
        }
        let narrow = m.clearance == 0;
        if !rasters.iter().any(|(n, ..)| *n == narrow) {
            let margin = if narrow { pinch } else { walk };
            rasters.push((narrow, win.raster(nav, &standing, margin), win.raster(nav, new, margin)));
        }
        let passable = &terrain.iter().find(|(mob, _)| *mob == m.mobility).expect("just added").1;
        let (_, standing, placed) = rasters.iter().find(|(n, ..)| *n == narrow).expect("just added");
        let before: Vec<bool> = passable.iter().zip(standing).map(|(p, s)| !p || *s).collect();
        let after: Vec<bool> = before.iter().zip(placed).map(|(b, n)| *b || *n).collect();
        let (clear_before, clear_after) = (win.clear(&before, m.clearance), win.clear(&after, m.clearance));
        let (out_before, out_after) = (win.flood(&clear_before), win.flood(&clear_after));
        if (0..clear_after.len()).any(|i| clear_after[i] && out_before[i] && !out_after[i]) {
            return true;
        }
    }
    false
}

/// The nav cells round a placement: the window `lo..lo + size`, and an
/// image of it padded by `pad` cells on every side (so that the clearance
/// square of every window cell lies in the image).
struct Window {
    lo: IVec2,
    size: IVec2,
    pad: i32,
    open: [bool; 4],
}

impl Window {
    fn image_size(&self) -> IVec2 {
        self.size + IVec2::splat(2 * self.pad)
    }

    fn image_cell(&self, i: usize) -> IVec2 {
        let w = self.image_size().x as usize;
        self.lo - IVec2::splat(self.pad) + IVec2::new((i % w) as i32, (i / w) as i32)
    }

    /// Per image cell: can a mover of `mobility` on `team` stand there,
    /// as far as the nav grid knows (off the grid: no)?
    fn passable(&self, nav: &NavGrid, mobility: u16, team: u8) -> Vec<bool> {
        let s = self.image_size();
        (0..(s.x * s.y) as usize).map(|i| nav.passable(self.image_cell(i), mobility, team)).collect()
    }

    /// Per image cell: does its centre lie within `margin` of one of `boxes`?
    fn raster(&self, nav: &NavGrid, boxes: &[Obb], margin: f32) -> Vec<bool> {
        let s = self.image_size();
        let origin = self.lo - IVec2::splat(self.pad);
        let mut out = vec![false; (s.x * s.y) as usize];
        for b in boxes {
            let (min, max) = b.aabb();
            let c0 = (nav.cell_of(min - Vec2::splat(margin)) - origin).max(IVec2::ZERO);
            let c1 = (nav.cell_of(max + Vec2::splat(margin)) - origin).min(s - IVec2::ONE);
            for y in c0.y..=c1.y {
                for x in c0.x..=c1.x {
                    let i = (y * s.x + x) as usize;
                    if !out[i] && b.distance_to_point(nav.cell_center(origin + IVec2::new(x, y))) <= margin {
                        out[i] = true;
                    }
                }
            }
        }
        out
    }

    /// Per window cell: is the square of half-size `k` round it free of
    /// `blocked` image cells (a mover of clearance `k` may stand there)?
    fn clear(&self, blocked: &[bool], k: i32) -> Vec<bool> {
        let s = self.image_size();
        let (w, h) = (s.x as usize, s.y as usize);
        let stride = w + 1;
        let mut sat = vec![0u32; stride * (h + 1)];
        for y in 0..h {
            let mut row = 0;
            for x in 0..w {
                row += u32::from(blocked[y * w + x]);
                sat[(y + 1) * stride + x + 1] = sat[y * stride + x + 1] + row;
            }
        }
        let k = k.clamp(0, self.pad) as usize;
        let pad = self.pad as usize;
        let mut out = vec![false; (self.size.x * self.size.y) as usize];
        for wy in 0..self.size.y as usize {
            for wx in 0..self.size.x as usize {
                let (x0, y0, x1, y1) = (wx + pad - k, wy + pad - k, wx + pad + k + 1, wy + pad + k + 1);
                let sum = sat[y1 * stride + x1] + sat[y0 * stride + x0] - sat[y0 * stride + x1] - sat[y1 * stride + x0];
                out[wy * self.size.x as usize + wx] = sum == 0;
            }
        }
        out
    }

    /// Per window cell: can a mover get there over `clear` cells (no
    /// corner cutting: four neighbours) from the window's open sides?
    fn flood(&self, clear: &[bool]) -> Vec<bool> {
        let (w, h) = (self.size.x as usize, self.size.y as usize);
        let mut seen = vec![false; w * h];
        let mut stack = Vec::new();
        let seed = |x: usize, y: usize, seen: &mut Vec<bool>, stack: &mut Vec<usize>| {
            let i = y * w + x;
            if clear[i] && !seen[i] {
                seen[i] = true;
                stack.push(i);
            }
        };
        let [west, east, south, north] = self.open;
        for y in 0..h {
            if west {
                seed(0, y, &mut seen, &mut stack);
            }
            if east {
                seed(w - 1, y, &mut seen, &mut stack);
            }
        }
        for x in 0..w {
            if south {
                seed(x, 0, &mut seen, &mut stack);
            }
            if north {
                seed(x, h - 1, &mut seen, &mut stack);
            }
        }
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            let mut visit = |n: usize| {
                if clear[n] && !seen[n] {
                    seen[n] = true;
                    stack.push(n);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        seen
    }
}
