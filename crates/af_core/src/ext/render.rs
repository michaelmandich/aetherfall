//! Shared items added by af_render (other crates may use them).
//! Only af_render edits this file. Never modify items in other af_core files.

#[allow(unused_imports)]
use crate::prelude::*;
#[allow(unused_imports)]
use bevy::prelude::*;

/// The fog-of-war texture af_render keeps in step with [`FogOfWar`] for the
/// local team, about ten times a second: one RGBA pixel per fog cell, image
/// row `y` = fog row `y` (world +Z grows with the row index, so row 0 is the
/// map's z = 0 edge). Colour is black; alpha is 0 in sight, 128 explored,
/// 255 unexplored. Inserted by af_render while fog is shown (a player view
/// with fog enabled); absent for spectators and fog-off matches. af_render's
/// terrain, water and prop shaders darken the world with it; af_ui may draw
/// it over the minimap (flip vertically if the minimap puts +Z up).
/// Client builds only (af_core's `client` feature): the server has no images.
#[cfg(feature = "client")]
#[derive(Resource, Clone, Debug)]
pub struct FogTexture(pub Handle<Image>);

/// How far from y = 0 [`ground_height`] may put the visual ground.
///
/// The rest of the game measures heights from y = 0: af_core's
/// `SpatialIndex` stores `Altitude` as the base height of the columns
/// af_input hovers units through and af_combat aims at (`Altitude` +
/// offset). af_input's cursor ray meets the drawn land through
/// [`TerrainSurface`] while a map exists (the y = 0 plane without one),
/// but units, buildings and their columns still sit near y = 0. So
/// af_render keeps every surface that something stands on within this
/// distance of y = 0, and a box drawn on it still overlaps the column the
/// sim picks and shoots at (for an infantry box, the ray through its
/// centre passes through the column).
/// Climbable highlands are therefore drawn this high, not at their
/// `TerrainDef::height`, until those crates measure heights from
/// [`ground_height`]. Real relief (mountains, water beds) only happens
/// where no ground unit stands.
pub const MAX_GROUND_OFFSET: f32 = 0.6;

/// Height of the drawn water surface over every water and ford cell (lakes,
/// rivers, the sea). Markers on water lie on it.
pub const WATER_LEVEL: f32 = -0.12;

/// The highest point af_render draws terrain at (mountain peaks): below
/// every flyer's altitude (12 m and up), so aircraft never fly through rock.
pub const MAX_TERRAIN_HEIGHT: f32 = 10.5;

/// Water terrain (shallows, deep water, fords, the sea): drawn as a bed
/// below [`WATER_LEVEL`] under the water surface, since its visual height
/// is below that surface.
pub fn is_water_terrain(def: &TerrainDef) -> bool {
    def.height < WATER_LEVEL - 0.03
}

/// Barrier terrain no ground unit enters at all (mountains): drawn as
/// relief rising from its edge.
pub fn is_barrier_terrain(def: &TerrainDef) -> bool {
    !is_water_terrain(def) && def.passable.is_empty()
}

/// Height of the visual ground at `p` that ground units, resource nodes,
/// buildings and markers stand on (af_render draws them there; flyers keep
/// their absolute `Altitude`). Terrain height is purely visual, so this is
/// the renderer's rule, shared so aiming (af_combat) and picking (af_input)
/// can match what is drawn.
///
/// The ground is drawn as a mesh with a vertex at every cell corner, each
/// at [`corner_ground`], and every cell split into two triangles along its
/// (min x, min z) to (max x, max z) diagonal. This interpolates on those
/// triangles, so it is exactly the drawn surface of every cell units walk
/// on: flat inside an area of one terrain, sloping only inside the edge
/// cells of raised or sunk land (a highland's scarp, a marsh's rim, a
/// river bank). Always within [`MAX_GROUND_OFFSET`] of 0.
pub fn ground_height(data: &GameData, nav: &NavGrid, p: Vec2) -> f32 {
    let g = (p - nav.origin()) / nav.cell_size();
    let base = g.floor();
    let (fx, fz) = (g.x - base.x, g.y - base.y);
    let c = base.as_ivec2();
    let h = |dx: i32, dz: i32| corner_ground(data, nav, c + IVec2::new(dx, dz));
    let (h00, h11) = (h(0, 0), h(1, 1));
    if fx >= fz {
        let h10 = h(1, 0);
        h00 + (h10 - h00) * fx + (h11 - h10) * fz
    } else {
        let h01 = h(0, 1);
        h00 + (h11 - h01) * fx + (h01 - h00) * fz
    }
}

/// The ground at cell corner `c` (the corner at the min x / min z of cell
/// `c`; it touches cells `c - (1, 1)` to `c`): of the land cells touching
/// it, the ground ([`terrain_ground`]) nearest y = 0, so flat grass stays
/// exactly flat and a highland or marsh slopes inside its own edge cells;
/// with no land around it, the same of the water cells (a unit wading in
/// sinks gradually); else (deep inside a mountain range) 0. Off the map
/// counts as what af_render draws just beyond the edge ([`off_map_terrain`]):
/// the water of the nearest map cell, else grass.
pub fn corner_ground(data: &GameData, nav: &NavGrid, c: IVec2) -> f32 {
    let (mut land, mut water) = (None::<f32>, None::<f32>);
    for d in [IVec2::new(-1, -1), IVec2::new(0, -1), IVec2::new(-1, 0), IVec2::ZERO] {
        let Some(def) = data.terrain.get(off_map_terrain(data, nav, c + d) as usize) else { continue };
        let slot = if is_water_terrain(def) {
            &mut water
        } else if is_barrier_terrain(def) {
            continue;
        } else {
            &mut land
        };
        let g = terrain_ground(def);
        *slot = Some(slot.map_or(g, |h| nearer_zero(h, g)));
    }
    land.or(water).unwrap_or(0.0)
}

/// The terrain of cell `c`, on the map or just off it: off the map, the
/// terrain of the nearest map cell when that is water (a river or the sea
/// carries on past the edge, and af_render draws its first ring of off-map
/// cells so), else grass (terrain 0, what `NavGrid::terrain_at` reads out
/// of bounds).
pub fn off_map_terrain(data: &GameData, nav: &NavGrid, c: IVec2) -> u8 {
    if nav.in_bounds(c) || nav.width() == 0 || nav.height() == 0 {
        return nav.terrain_at(c);
    }
    let edge = c.clamp(IVec2::ZERO, IVec2::new(nav.width() as i32 - 1, nav.height() as i32 - 1));
    let t = nav.terrain_at(edge);
    if data.terrain.get(t as usize).is_some_and(is_water_terrain) { t } else { nav.terrain_at(c) }
}

/// Whichever of two heights is nearer y = 0 (the higher on a tie).
pub fn nearer_zero(a: f32, b: f32) -> f32 {
    if a.abs() < b.abs() || (a.abs() == b.abs() && a > b) { a } else { b }
}

/// The ground height one terrain type stands at, inside an area of it:
///
/// * climbable terrain (its `passable` admits `Climber`: rocky highlands):
///   its top, so climbers walk on the rock;
/// * terrain below 0 (water, marsh): its surface (units wade);
/// * anything else (grass, dunes, thickets, mountains): 0, so units wade
///   through brambles rather than walk on them;
///
/// always within [`MAX_GROUND_OFFSET`] of 0.
pub fn terrain_ground(def: &TerrainDef) -> f32 {
    let surface = if def.passable.contains(&MoveFlag::Climber) { def.height } else { def.height.min(0.0) };
    surface.clamp(-MAX_GROUND_OFFSET, MAX_GROUND_OFFSET)
}

/// The drawn terrain of the current map, published by af_render once it has
/// built it (removed when the match ends): the top of what is drawn at
/// every cell corner of the map (the ground, a mountain's rock, or the water
/// surface over a water bed). For things that must sit on or hit the drawn
/// world where it rises above the ground units walk on: markers on
/// mountains and water, and af_input's cursor ray
/// ([`TerrainSurface::raycast`], falling back to the y = 0 plane while
/// there is no map), so clicks land on the rock and water that are drawn.
/// Unit columns and aiming still work from `Altitude` near y = 0 (see
/// [`MAX_GROUND_OFFSET`]).
#[derive(Resource, Clone, Debug, Default)]
pub struct TerrainSurface {
    /// World position of corner (0, 0).
    pub origin: Vec2,
    /// Distance between corners (the nav cell size).
    pub cell: f32,
    /// Corners per row (cells + 1) and rows of corners.
    pub width: u32,
    pub height: u32,
    /// Top of the drawn world at each corner, row-major from z = origin.
    pub top: Vec<f32>,
    /// How each cell (row-major, `(width - 1) * (height - 1)`) is split into
    /// its two drawn triangles: true along its (max x, min z) - (min x,
    /// max z) diagonal, false along (min, min) - (max, max) as every cell
    /// units walk on is ([`ground_height`]). Empty: all along the latter.
    pub split: Vec<bool>,
}

impl TerrainSurface {
    /// The drawn top at `p`, interpolated on the drawn triangles of its
    /// cell; 0 off the map.
    pub fn top_at(&self, p: Vec2) -> f32 {
        if self.width < 2 || self.height < 2 || self.top.len() < (self.width * self.height) as usize {
            return 0.0;
        }
        let g = (p - self.origin) / self.cell.max(1e-6);
        let max = Vec2::new((self.width - 1) as f32, (self.height - 1) as f32);
        if g.x < 0.0 || g.y < 0.0 || g.x > max.x || g.y > max.y {
            return 0.0;
        }
        let x0 = (g.x.floor() as u32).min(self.width - 2);
        let z0 = (g.y.floor() as u32).min(self.height - 2);
        let (fx, fz) = (g.x - x0 as f32, g.y - z0 as f32);
        let at = |x: u32, z: u32| self.top[(z * self.width + x) as usize];
        let (h00, h10, h01, h11) = (at(x0, z0), at(x0 + 1, z0), at(x0, z0 + 1), at(x0 + 1, z0 + 1));
        let split = self.split.get((z0 * (self.width - 1) + x0) as usize).copied().unwrap_or(false);
        match (split, fx >= fz, fx + fz <= 1.0) {
            // Along (min, min) - (max, max), as `ground_height`.
            (false, true, _) => h00 + (h10 - h00) * fx + (h11 - h10) * fz,
            (false, false, _) => h00 + (h11 - h01) * fx + (h01 - h00) * fz,
            // Along (max, min) - (min, max).
            (true, _, true) => h00 + (h10 - h00) * fx + (h01 - h00) * fz,
            (true, _, false) => h11 + (h01 - h11) * (1.0 - fx) + (h10 - h11) * (1.0 - fz),
        }
    }

    /// Where a ray from `origin` along `dir` first meets the drawn top within
    /// `max_distance`, if it does (steps of a quarter cell, then bisection).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_distance: f32) -> Option<Vec3> {
        let dir = dir.try_normalize()?;
        let step = (self.cell * 0.25).max(0.05);
        let above = |t: f32| {
            let p = origin + dir * t;
            p.y - self.top_at(p.xz())
        };
        if above(0.0) < 0.0 {
            return Some(origin);
        }
        let mut t0 = 0.0;
        let mut t = step;
        while t <= max_distance {
            if above(t) < 0.0 {
                let (mut lo, mut hi) = (t0, t);
                for _ in 0..12 {
                    let mid = (lo + hi) * 0.5;
                    if above(mid) < 0.0 { hi = mid } else { lo = mid }
                }
                return Some(origin + dir * hi);
            }
            t0 = t;
            t += step;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{new_nav, test_data};

    #[test]
    fn grass_is_flat_and_highland_edges_slope_inside_the_highland() {
        let data = test_data();
        let grass = data.terrain_id("grass").unwrap();
        let cliff = data.terrain_id("cliff").unwrap();
        let highland = terrain_ground(data.terrain_def(cliff));
        let mut nav = new_nav(&data, 20, 20, grass);
        for y in 0..20 {
            for x in 10..20 {
                nav.set_terrain(IVec2::new(x, y), cliff.0);
            }
        }
        for i in 0..200 {
            let p = Vec2::new((i % 20) as f32 * 0.49, (i / 20) as f32 * 1.9 + 0.05);
            assert_eq!(ground_height(&data, &nav, p), 0.0, "grass at {p} stays at 0");
        }
        assert!((ground_height(&data, &nav, Vec2::new(14.3, 6.6)) - highland).abs() < 1e-6, "on the plateau");
        let edge = ground_height(&data, &nav, Vec2::new(10.5, 5.0));
        assert!(edge > 0.0 && edge < highland, "the scarp is inside the highland's edge cell: {edge}");
        assert_eq!(corner_ground(&data, &nav, IVec2::new(10, 4)), 0.0, "a corner touching grass stays on the grass");
    }

    #[test]
    fn water_and_mountain_corners() {
        let data = test_data();
        let grass = data.terrain_id("grass").unwrap();
        let deep = data.terrain_id("deep_water").unwrap();
        let mountain = data.terrain_id("mountain").unwrap();
        assert!(is_water_terrain(data.terrain_def(deep)) && !is_barrier_terrain(data.terrain_def(deep)));
        assert!(is_barrier_terrain(data.terrain_def(mountain)) && !is_water_terrain(data.terrain_def(mountain)));
        for t in ["grass", "sand", "marsh", "thicket", "cliff", "scorched"] {
            let def = data.terrain_def(data.terrain_id(t).unwrap());
            assert!(!is_water_terrain(def) && !is_barrier_terrain(def), "{t} is land");
        }
        for t in ["shallow_water", "ford", "ocean"] {
            assert!(is_water_terrain(data.terrain_def(data.terrain_id(t).unwrap())), "{t} is water");
        }
        let mut nav = new_nav(&data, 12, 12, grass);
        for y in 0..12 {
            for x in 6..12 {
                nav.set_terrain(IVec2::new(x, y), if x < 9 { deep.0 } else { mountain.0 });
            }
        }
        let wet = terrain_ground(data.terrain_def(deep));
        assert_eq!(corner_ground(&data, &nav, IVec2::new(7, 5)), wet, "inside the water: its ground");
        assert_eq!(corner_ground(&data, &nav, IVec2::new(6, 5)), 0.0, "the shore stays on the land");
        assert_eq!(corner_ground(&data, &nav, IVec2::new(9, 5)), wet, "water before mountain");
        assert_eq!(corner_ground(&data, &nav, IVec2::new(11, 5)), 0.0, "inside the range");
        for i in 0..144 {
            let g = ground_height(&data, &nav, Vec2::new((i % 12) as f32 + 0.3, (i / 12) as f32 + 0.6));
            assert!(g.abs() <= MAX_GROUND_OFFSET, "{g}");
        }
    }

    #[test]
    fn water_carries_on_past_the_map_edge() {
        // A river runs off the south and north edges: units wading in its
        // last row stay at its ground instead of rising toward the grass
        // `terrain_at` reads off the map.
        let data = test_data();
        let grass = data.terrain_id("grass").unwrap();
        let deep = data.terrain_id("deep_water").unwrap();
        let shallow = data.terrain_id("shallow_water").unwrap();
        let mut nav = new_nav(&data, 12, 12, grass);
        for y in 0..12 {
            for x in 3..8 {
                nav.set_terrain(IVec2::new(x, y), if (4..7).contains(&x) { deep.0 } else { shallow.0 });
            }
        }
        let wet = terrain_ground(data.terrain_def(deep));
        assert_eq!(off_map_terrain(&data, &nav, IVec2::new(5, -1)), deep.0, "beyond the river: the river");
        assert_eq!(off_map_terrain(&data, &nav, IVec2::new(1, -3)), 0, "beyond the grass: grass");
        assert_eq!(off_map_terrain(&data, &nav, IVec2::new(5, 4)), deep.0, "on the map: the map");
        for c in [IVec2::new(5, 0), IVec2::new(6, 12)] {
            assert_eq!(corner_ground(&data, &nav, c), wet, "border corner {c} is in the water");
        }
        for p in [Vec2::new(5.5, 0.1), Vec2::new(5.5, 11.9), Vec2::new(5.9, 11.97)] {
            assert!((ground_height(&data, &nav, p) - wet).abs() < 1e-6, "wading at {p}: {}", ground_height(&data, &nav, p));
        }
        assert_eq!(corner_ground(&data, &nav, IVec2::new(1, 0)), 0.0, "the grass edge stays flat");
        assert_eq!(corner_ground(&data, &nav, IVec2::new(3, 0)), 0.0, "the bank stays on the land");
    }

    #[test]
    fn surface_follows_the_drawn_triangles() {
        // A cell creased along one diagonal: the drawn top at its centre is
        // that diagonal's, not the average of its corners.
        let top = vec![6.0, 8.0, 8.0, 6.0];
        let along = TerrainSurface { origin: Vec2::ZERO, cell: 1.0, width: 2, height: 2, top: top.clone(), split: vec![false] };
        let across = TerrainSurface { split: vec![true], ..along.clone() };
        assert_eq!(along.top_at(Vec2::splat(0.5)), 6.0, "a gully along (min, min) - (max, max)");
        assert_eq!(across.top_at(Vec2::splat(0.5)), 8.0, "a ridge along the other diagonal");
        assert!((across.top_at(Vec2::new(0.8, 0.7)) - 7.0).abs() < 1e-6);
        assert!((along.top_at(Vec2::new(0.8, 0.7)) - 6.2).abs() < 1e-6);
        // The corners are the corners either way, and no split reads as
        // the main diagonal (as land is drawn).
        for (p, h) in [(Vec2::ZERO, 6.0), (Vec2::X, 8.0), (Vec2::Y, 8.0), (Vec2::ONE, 6.0)] {
            assert_eq!(along.top_at(p), h);
            assert_eq!(across.top_at(p), h);
        }
        let unsplit = TerrainSurface { split: Vec::new(), ..along.clone() };
        assert_eq!(unsplit.top_at(Vec2::new(0.3, 0.6)), along.top_at(Vec2::new(0.3, 0.6)));
    }

    #[test]
    fn surface_samples_and_raycasts() {
        let mut top = vec![0.0; 11 * 11];
        top[5 * 11 + 5] = 4.0;
        let s = TerrainSurface { origin: Vec2::ZERO, cell: 1.0, width: 11, height: 11, top, split: Vec::new() };
        assert_eq!(s.top_at(Vec2::new(5.0, 5.0)), 4.0);
        assert_eq!(s.top_at(Vec2::new(5.5, 5.0)), 2.0);
        assert_eq!(s.top_at(Vec2::new(-3.0, 5.0)), 0.0, "off the map");
        // Straight down onto the peak, and a slanted ray onto the flat.
        let hit = s.raycast(Vec3::new(5.0, 20.0, 5.0), Vec3::NEG_Y, 100.0).unwrap();
        assert!((hit.y - 4.0).abs() < 0.01, "{hit}");
        let hit = s.raycast(Vec3::new(1.0, 10.0, 1.0), Vec3::new(0.1, -1.0, 0.1), 100.0).unwrap();
        assert!(hit.y.abs() < 0.01 && (hit.x - 2.0).abs() < 0.01, "{hit}");
        assert!(s.raycast(Vec3::new(1.0, 10.0, 1.0), Vec3::Y, 100.0).is_none());
    }
}
