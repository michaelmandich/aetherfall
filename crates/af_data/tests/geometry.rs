//! The real content in `assets/data/` fits the engine's navigation geometry
//! (game-design.md section 10 rule 10 and section 11): every ground unit a
//! faction fields stays inside its clearance class, and walks through that
//! faction's own gates set in a closed wall line, at any angle.
//!
//! The grid is af_geom's, set up the way the game sets it up:
//! * 1 m cells (af_map `mapgen`);
//! * a 0.3 m walk margin (af_movement `obstacles::WALK_MARGIN`);
//! * wall segments added as obstacles and gates marked for their owner's
//!   team (af_movement `obstacles::apply`);
//! * a mover's clearance radius is half its width (af_core `spawn_unit`:
//!   `Mobile.radius = footprint.half.x`), so A* needs a clear
//!   `(2k + 1)^2` block of cells with `k = ceil(width / 2 - 0.5)`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use af_data::*;
use af_geom::{NavGrid, Obb, PathRequest};
use bevy_math::Vec2;

/// af_movement `obstacles::WALK_MARGIN`.
const WALK_MARGIN: f32 = 0.3;
/// Cell side of the game's nav grid (af_map).
const CELL: f32 = 1.0;
/// A worker's half-width: the narrowest mover, for the "line is closed" checks.
const WORKER_RADIUS: f32 = 0.3;
/// Widest a non-giant ground unit may be (k = 1), and a giant (k = 2).
const MAX_WIDTH: f32 = 2.9;
const MAX_GIANT_WIDTH: f32 = 5.0;

fn data() -> GameData {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
    GameData::load_dir(&dir).unwrap_or_else(|e| panic!("{e}"))
}

/// Ground units on some faction's roster (benchmark units nobody fields are left out).
fn fielded_ground_units(d: &GameData) -> Vec<(FactionId, UnitTypeId)> {
    let mut out = Vec::new();
    for f in d.faction_ids() {
        for u in &d.faction(f).unit_ids {
            if !d.unit(*u).is_flying() {
                out.push((f, *u));
            }
        }
    }
    out
}

#[test]
fn ground_units_stay_inside_their_clearance_class() {
    let d = data();
    let grid = NavGrid::new(4, 4, CELL, Vec2::ZERO);
    let mut bad = Vec::new();
    for (f, u) in fielded_ground_units(&d) {
        let def = d.unit(u);
        let width = def.size[0];
        let (cap, k_cap) = if def.has_tag(Tag::Giant) { (MAX_GIANT_WIDTH, 2) } else { (MAX_WIDTH, 1) };
        let k = grid.clearance_cells(width * 0.5);
        if width > cap || k > k_cap {
            bad.push(format!("{} ({}): {width} m wide (k = {k}), cap {cap} m (k = {k_cap})", def.id, d.faction(f).id));
        }
    }
    assert!(bad.is_empty(), "ground units too wide for the nav grid's gates and gaps:\n  {}", bad.join("\n  "));
}

/// A gate of footprint `gate` (x = length, z = depth) at `angle_deg` and
/// sub-cell `offset` in the middle of a straight, closed line of `wall`
/// segments laid flush against both its ends, across the whole grid.
/// Returns the grid, the gate centre and the line's normal.
fn gate_line(gate: [f32; 2], wall: [f32; 2], angle_deg: f32, offset: Vec2, gate_solid: bool) -> (NavGrid, Vec2, Vec2) {
    const SIDE: u32 = 96;
    let mut nav = NavGrid::new(SIDE, SIDE, CELL, Vec2::ZERO);
    nav.set_terrain_info(0, u16::MAX, true);
    nav.set_walk_margin(WALK_MARGIN);
    let a = angle_deg.to_radians();
    let dir = Vec2::new(a.sin(), a.cos());
    // The yaw whose `right` axis (the box's x / length) runs along the line.
    let yaw = (-dir.y).atan2(dir.x);
    let center = Vec2::splat(SIDE as f32 * 0.5) + offset;
    let gate_box = Obb::new(center, Vec2::new(gate[0] * 0.5, gate[1] * 0.5), yaw);
    if gate_solid {
        nav.add_obstacle(&gate_box, 1);
    } else {
        nav.set_gate(&gate_box, Some(0));
    }
    let segments = (SIDE as f32 / wall[0]).ceil() as usize;
    for i in 0..segments {
        let along = gate[0] * 0.5 + wall[0] * (i as f32 + 0.5);
        for side in [-1.0f32, 1.0] {
            nav.add_obstacle(&Obb::new(center + dir * along * side, Vec2::new(wall[0] * 0.5, wall[1] * 0.5), yaw), 1);
        }
    }
    (nav, center, Vec2::new(dir.y, -dir.x))
}

fn crosses(nav: &mut NavGrid, center: Vec2, normal: Vec2, mobility: u16, radius: f32, team: u8) -> bool {
    nav.find_path(&PathRequest {
        start: center - normal * 15.0,
        goal: center + normal * 15.0,
        mobility,
        radius,
        team,
        max_expansions: 400_000,
        reach: None,
    })
    .complete
}

#[test]
fn own_gates_let_every_ground_unit_through_at_any_angle() {
    let d = data();
    let offsets = [Vec2::ZERO, Vec2::new(0.5, 0.5), Vec2::new(0.25, 0.75), Vec2::new(0.75, 0.2)];
    let grid = NavGrid::new(4, 4, CELL, Vec2::ZERO);
    // (gate xz, wall xz, k) -> the first layout that failed, if any.
    let mut seen: HashMap<[u32; 5], Option<String>> = HashMap::new();
    let mut reported = HashSet::new();
    let mut bad = Vec::new();
    let mut checked = 0;
    for f in d.faction_ids() {
        let fd = d.faction(f);
        let walls: Vec<&BuildingDef> = fd.building_ids.iter().map(|b| d.building(*b)).filter(|b| b.wall && !b.gate).collect();
        let gates: Vec<&BuildingDef> = fd.building_ids.iter().map(|b| d.building(*b)).filter(|b| b.gate).collect();
        // The widest ground unit decides: a clear (2k+1)^2 block holds every
        // narrower class's block too.
        let Some(widest) = fd.unit_ids.iter().map(|u| d.unit(*u)).filter(|u| !u.is_flying()).max_by(|a, b| a.size[0].total_cmp(&b.size[0])) else {
            continue;
        };
        let uid = d.unit_id(&widest.id).expect("listed unit");
        let mobility = d.unit_mobility(uid, f).0;
        let radius = widest.size[0] * 0.5;
        let k = grid.clearance_cells(radius);
        for g in &gates {
            assert!(!walls.is_empty(), "faction `{}` builds gate `{}` but no wall", fd.id, g.id);
            for w in &walls {
                let key = [g.size[0].to_bits(), g.size[2].to_bits(), w.size[0].to_bits(), w.size[2].to_bits(), k as u32];
                let result = seen.entry(key).or_insert_with(|| {
                    for angle in (0..=90).step_by(5) {
                        for off in offsets {
                            checked += 1;
                            let (gate, wall) = ([g.size[0], g.size[2]], [w.size[0], w.size[2]]);
                            let (mut nav, c, n) = gate_line(gate, wall, angle as f32, off, false);
                            // The line holds: no worker of another team gets through the gate...
                            assert!(!crosses(&mut nav, c, n, mobility, WORKER_RADIUS, 1), "an enemy worker crosses `{}` + `{}` at {angle} deg {off}", g.id, w.id);
                            // ...nor anyone past the line with the gate shut.
                            let (mut shut, c, n) = gate_line(gate, wall, angle as f32, off, true);
                            assert!(!crosses(&mut shut, c, n, mobility, WORKER_RADIUS, 0), "the closed line `{}` + `{}` leaks at {angle} deg {off}", g.id, w.id);
                            if !crosses(&mut nav, c, n, mobility, radius, 0) {
                                return Some(format!("first fails at {angle} deg, gate centre offset {off}"));
                            }
                        }
                    }
                    None
                });
                // One line per faction and footprint pair (every tier of a chain shares one).
                if let Some(why) = result
                    && reported.insert((f, key))
                {
                    bad.push(format!(
                        "{}: `{}` ({} m wide, k = {k}) cannot pass its own `{}` ({} m) in a `{}` line: {why}",
                        fd.id, widest.id, widest.size[0], g.id, g.size[0], w.id
                    ));
                }
            }
        }
    }
    assert!(checked > 0, "no gate layouts were checked");
    assert!(bad.is_empty(), "own-team gates that trap their owner's ground units:\n  {}", bad.join("\n  "));
}
