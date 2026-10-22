//! Where to put a building and who builds it.

use std::f32::consts::TAU;

use af_core::prelude::*;
use bevy::prelude::*;

use crate::lanes::{Mover, ground_movers, seals_pocket};
use crate::view::{AiView, Snapshot, WorkerInfo, WorkerTask};

/// Free space kept between a new building and blocking nodes (metres).
pub const NODE_GAP: f32 = 0.8;

/// A spiral search for a building spot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpotQuery {
    pub kind: BuildingTypeId,
    /// Centre of the search.
    pub anchor: Vec2,
    /// Preferred direction from the anchor; tried first, then alternately
    /// further round on both sides.
    pub toward: Vec2,
    /// Ring radii searched (centre-to-centre).
    pub min_r: f32,
    pub max_r: f32,
    /// Free space kept around the footprint against other buildings, pending
    /// footprints and the map edge, so units can walk between buildings.
    /// Narrower than what wide movers need: a spot that would close a ring
    /// on them is skipped regardless (see [`seals_pocket`]).
    pub gap: f32,
}

/// Spiral outward from the anchor and return the first spot where
/// [`check_placement`] accepts the building, the [`clear_around`] gap
/// holds and the footprint encloses no ground the realm's ground units
/// could leave before ([`seals_pocket`]): their own buildings must never
/// trap them. Placements are axis-aligned (facing 0) unless snapped onto a
/// node. `extra` = footprints placed but not yet in the spatial index;
/// `avoid` = points (with radius) to skip, e.g. spots that failed before.
pub fn find_build_spot(view: &AiView, q: &SpotQuery, extra: &[Obb], avoid: &[(Vec2, f32)]) -> Option<Placement> {
    let movers = ground_movers(view);
    let def = view.data.building(q.kind);
    let width = def.size[0].max(def.size[2]);
    let ring_step = (width * 0.5 + q.gap * 0.5).max(1.5);
    let arc_step = width + q.gap;
    let toward = q.toward.normalize_or(Vec2::Y);
    let mut r = q.min_r.max(0.0);
    while r <= q.max_r + 1e-3 {
        let n = if r < 0.5 { 1 } else { ((TAU * r) / arc_step).ceil().max(6.0) as i32 };
        let dtheta = TAU / n as f32;
        for k in 0..n {
            // 0, +1, -1, +2, -2, ... steps round from the preferred direction.
            let j = if k % 2 == 1 { (k + 1) / 2 } else { -(k / 2) };
            let pos = q.anchor + af_geom::dmath::from_angle(j as f32 * dtheta).rotate(toward) * r;
            if avoid.iter().any(|(p, rad)| p.distance_squared(pos) < rad * rad) {
                continue;
            }
            if let Some(p) = try_spot(view, q, pos, extra, &movers) {
                return Some(p);
            }
        }
        r += ring_step;
    }
    None
}

fn try_spot(view: &AiView, q: &SpotQuery, pos: Vec2, extra: &[Obb], movers: &[Mover]) -> Option<Placement> {
    let p = check_placement(view.data, view.nav, view.spatial, view.me, q.kind, pos, 0.0, false, extra).ok()?;
    let obb = Obb::from_size(p.pos, view.data.building(q.kind).size, p.facing);
    (clear_around(view, &obb, q.gap, p.node, extra) && !seals_pocket(view, movers, &[obb], extra)).then_some(p)
}

/// Is there `gap` metres of free space between `obb` and every building,
/// pending footprint and the map edge, and [`NODE_GAP`] to blocking nodes?
/// `ignore` = a node the building is placed on.
pub fn clear_around(view: &AiView, obb: &Obb, gap: f32, ignore: Option<Entity>, extra: &[Obb]) -> bool {
    let grown = Obb { half: obb.half + Vec2::splat(gap), ..*obb };
    let (min, max) = grown.aabb();
    if min.x < 0.0 || min.y < 0.0 || max.x > view.map_size.x || max.y > view.map_size.y {
        return false;
    }
    if extra.iter().any(|o| grown.overlaps(o)) {
        return false;
    }
    let near_nodes = Obb { half: obb.half + Vec2::splat(NODE_GAP), ..*obb };
    let mut ok = true;
    view.spatial.for_each_near(obb.center, grown.bounding_radius(), |e| {
        if !ok || Some(e.entity) == ignore {
            return;
        }
        ok = match e.kind {
            SpatialKind::Building => !grown.overlaps(&e.obb),
            SpatialKind::Node => ignore.is_some() || !e.blocking || !near_nodes.overlaps(&e.obb),
            SpatialKind::Unit => true,
        };
    });
    ok
}

/// An idle worker counts as this many metres closer than a busy one.
const IDLE_BONUS: f32 = 15.0;

/// Up to `n` workers to send to `at`, nearest first (idle ones favoured),
/// skipping `taken`. Appends to `out`.
pub fn pick_workers(snap: &Snapshot, at: Vec2, n: usize, taken: &[Entity], out: &mut Vec<Entity>) {
    let start = out.len();
    let score = |w: &WorkerInfo| w.pos.distance(at) - if w.task == WorkerTask::Idle { IDLE_BONUS } else { 0.0 };
    for _ in 0..n {
        let best = snap
            .workers
            .iter()
            .filter(|w| w.task.available() && !taken.contains(&w.entity) && !out[start..].contains(&w.entity))
            .min_by(|a, b| score(a).total_cmp(&score(b)));
        match best {
            Some(w) => out.push(w.entity),
            None => break,
        }
    }
}

/// How many builders a building deserves: more for long builds.
pub fn builders_for(data: &GameData, kind: BuildingTypeId) -> usize {
    let t = data.building(kind).build_time;
    if t >= 90.0 {
        3
    } else if t >= 45.0 {
        2
    } else {
        1
    }
}
