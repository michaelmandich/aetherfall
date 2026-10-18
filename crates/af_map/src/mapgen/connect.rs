//! Connectivity guarantee: every start (and so every other start) and every
//! highland top is reachable on foot from the crater floor, through cells
//! wide enough for most units.

use std::collections::VecDeque;

use af_core::prelude::*;
use bevy::prelude::*;

use super::{terrain_nav, Gen};

/// Clearance radius checked: NavGrid needs the 3x3 block around each step
/// clear, which every non-giant ground unit fits through.
pub(crate) const CONNECT_RADIUS: f32 = 1.0;
/// Half-width of a corridor carved when something is cut off.
const CARVE_HALF_WIDTH: f32 = 3.0;
/// Distance in front of the hall used as each start's reachability point.
const FRONT_GAP: f32 = 2.5;
/// Carving rounds before giving up on a place that stays cut off.
const MAX_CARVES: usize = 3;

/// Cells reachable on foot from `from` (4-neighbour flood over cells where a
/// mover of [`CONNECT_RADIUS`] fits). Empty when `from` itself is blocked.
pub(crate) fn reachable(nav: &NavGrid, from: Vec2, mobility: u16) -> Vec<bool> {
    let (w, h) = (nav.width() as i32, nav.height() as i32);
    let mut seen = vec![false; (w * h) as usize];
    let Some(start) = nav.nearest_open(from, mobility, CONNECT_RADIUS, af_geom::NO_TEAM, 6.0) else { return seen };
    let start = nav.cell_of(start);
    let mut queue = VecDeque::from([start]);
    seen[nav.index(start)] = true;
    while let Some(c) = queue.pop_front() {
        for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let n = c + d;
            if nav.in_bounds(n) && !seen[nav.index(n)] && nav.clear_cell(n, mobility, CONNECT_RADIUS, af_geom::NO_TEAM) {
                seen[nav.index(n)] = true;
                queue.push_back(n);
            }
        }
    }
    seen
}

impl Gen<'_> {
    /// Check that every start and every highland top is reachable on foot
    /// from the crater centre; carve each cut-off place's route open (land
    /// through cliffs, fords through deep water, no trees or rocks) and check
    /// again. Returns how many places are still cut off after
    /// [`MAX_CARVES`] rounds (0 normally; warns otherwise).
    pub(super) fn ensure_connectivity(&mut self) -> usize {
        let mut cut_off = self.cut_off_routes();
        for _ in 0..MAX_CARVES {
            if cut_off.is_empty() {
                break;
            }
            for route in &cut_off {
                for leg in route.windows(2) {
                    self.carve(leg[0], leg[1]);
                }
            }
            cut_off = self.cut_off_routes();
        }
        if !cut_off.is_empty() {
            warn!("map gen: {} place(s) still cut off from the crater after carving", cut_off.len());
        }
        cut_off.len()
    }

    /// The carve route (the place first, the crater centre last) of every
    /// start front and highland top not reachable on foot from the crater.
    /// A highland's route runs out through its ramp, so carving it never
    /// breaches the cliff ring.
    pub(super) fn cut_off_routes(&self) -> Vec<Vec<Vec2>> {
        let nav = self.obstacle_nav();
        let seen = reachable(&nav, self.layout.center, Mobility::of(MoveFlag::Foot).0);
        let center = self.layout.center;
        let starts = self.start_fronts().into_iter().map(|p| vec![p, center]);
        let highlands = self.highlands.iter().map(|h| vec![h.ramp_top(), h.ramp_foot(), center]);
        starts
            .chain(highlands)
            .filter(|route| !seen.get(nav.index(nav.cell_of(route[0]))).copied().unwrap_or(false))
            .collect()
    }

    /// A point just in front of each start's hall, toward the centre.
    fn start_fronts(&self) -> Vec<Vec2> {
        (0..self.layout.starts.len())
            .map(|i| {
                let frame = self.layout.frame(i);
                let reach = self.hall_or_point(i).support(frame.dir(Vec2::X));
                frame.point(Vec2::X * (reach + FRONT_GAP))
            })
            .collect()
    }

    /// The terrain grid with every building and blocking node as an obstacle,
    /// as af_movement will see it on the first tick.
    fn obstacle_nav(&self) -> NavGrid {
        let mut nav = terrain_nav(self.data, self.w as u32, self.w as u32, &self.terrain);
        for b in &self.buildings {
            nav.add_obstacle(&Obb::from_size(b.pos, self.data.building(b.kind).size, b.yaw), 1);
        }
        for n in &self.nodes {
            let def = self.data.node(n.kind);
            if def.blocking {
                nav.add_obstacle(&Obb::from_size(n.pos, def.size, n.yaw), 1);
            }
        }
        nav
    }

    fn carve(&mut self, a: Vec2, b: Vec2) {
        let foot = Mobility::of(MoveFlag::Foot);
        let (deep, shallow, grass) = (self.ids.deep, self.ids.shallow, self.ids.grass);
        let walkable: Vec<bool> = self.data.terrain_ids().map(|t| self.data.terrain_passable(t, foot)).collect();
        let passable = |t: TerrainId| walkable.get(t.index()).copied().unwrap_or(false);
        let ford = if passable(shallow) { shallow } else { grass };
        self.paint_disc((a + b) * 0.5, a.distance(b) * 0.5 + CARVE_HALF_WIDTH, |cell| {
            let on_path = af_geom::point_segment_distance(cell.center, a, b) <= CARVE_HALF_WIDTH;
            (on_path && !passable(cell.terrain)).then_some(if cell.terrain == deep { ford } else { grass })
        });
        let data = self.data;
        self.nodes.retain(|n| {
            let def = data.node(n.kind);
            !def.blocking || Obb::from_size(n.pos, def.size, n.yaw).segment_distance(a, b) > CARVE_HALF_WIDTH + 0.5
        });
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use af_core::testing::test_data;

    use super::*;
    use crate::mapgen::{polar, BuildingPlacement};
    use crate::setup::default_setup;

    #[test]
    fn a_walled_in_start_is_carved_free() {
        let data = test_data();
        let mut g = Gen::bare(&data, 2, 224.0, 5);
        g.place_halls(&default_setup(&data));
        let start = g.layout.starts[0];
        let cliff = g.ids.cliff;
        g.paint_disc(start, 14.0, |cell| ((cell.center.distance(start) - 12.0).abs() <= 1.5).then_some(cliff));
        assert_eq!(g.cut_off_routes().len(), 1, "the cliff ring cuts start 0 off");
        assert_eq!(g.ensure_connectivity(), 0);
        assert!(g.cut_off_routes().is_empty());
    }

    #[test]
    fn a_start_that_carving_cannot_free_is_reported() {
        let data = test_data();
        let mut g = Gen::bare(&data, 2, 224.0, 6);
        g.place_halls(&default_setup(&data));
        // Carving clears terrain, trees and rocks, never buildings.
        let (start, kind) = (g.layout.starts[1], g.buildings[0].kind);
        for k in 0..24 {
            let pos = start + polar(k as f32 * TAU / 24.0) * 16.0;
            g.buildings.push(BuildingPlacement { player: 1, kind, pos, yaw: 0.0 });
        }
        assert_eq!(g.ensure_connectivity(), 1, "start 1 is still walled in after the last carve");
    }
}
