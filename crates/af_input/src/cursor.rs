//! Picking: what the mouse points at on the ground and among the boxes.

use af_core::ext::input::{CursorOverride, RtsCamera};
use af_core::ext::render::{TerrainSurface, MAX_GROUND_OFFSET, MAX_TERRAIN_HEIGHT};
use af_core::prelude::*;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::{clamp_to_map, rig_transform};
use crate::viewer::Viewer;

/// Small ground units within this many metres of the cursor's ground point
/// count as hovered even when the ray misses their box.
const PICK_SLACK: f32 = 0.5;
/// The drawn terrain lies between these heights: af_render draws peaks up
/// to `MAX_TERRAIN_HEIGHT`, water at its surface, and the lowest ground
/// (marsh) `MAX_GROUND_OFFSET` below 0. The margins put the ray's first
/// and last samples clear of it.
const SURFACE_TOP: f32 = MAX_TERRAIN_HEIGHT + 0.5;
const SURFACE_BOTTOM: f32 = -MAX_GROUND_OFFSET - 0.5;
/// A ray meeting the drawn terrain higher than this meets rock (ground
/// units never stand above `MAX_GROUND_OFFSET`), which hides what is
/// behind it.
const ROCK_ABOVE: f32 = MAX_GROUND_OFFSET + 0.1;

/// Screen <-> world conversions for the RTS camera as the rig places it this
/// frame (the camera's `GlobalTransform` is only refreshed in PostUpdate).
pub(crate) struct Projector<'a> {
    camera: &'a Camera,
    view: GlobalTransform,
}

impl<'a> Projector<'a> {
    pub fn new(camera: &'a Camera, rig: &CameraRig) -> Projector<'a> {
        Projector { camera, view: GlobalTransform::from(rig_transform(rig)) }
    }

    /// Logical-px position of a world point, if it is in front of the camera.
    pub fn to_screen(&self, world: Vec3) -> Option<Vec2> {
        self.camera.world_to_viewport(&self.view, world).ok()
    }

    /// Screen position of the middle of an entity's box.
    pub fn box_centre(&self, pos: Vec2, altitude: f32, height: f32) -> Option<Vec2> {
        self.to_screen(Vec3::new(pos.x, altitude + height * 0.5, pos.y))
    }

    /// The camera's viewport in logical px.
    pub fn viewport(&self) -> Option<Rect> {
        self.camera.logical_viewport_rect()
    }

    pub fn ray(&self, screen: Vec2) -> Option<Ray3d> {
        self.camera.viewport_to_world(&self.view, screen).ok()
    }
}

/// The mouse position in logical window px: the [`CursorOverride`] while it
/// holds one, else wherever the real mouse is (`None` outside the window).
pub(crate) fn mouse_position(window: &Window, stand_in: Option<&CursorOverride>) -> Option<Vec2> {
    stand_in.and_then(|o| o.0).or_else(|| window.cursor_position())
}

/// Fill `CursorWorld` (screen, ground, hovered) from the mouse. Leaves it
/// untouched without a window or camera (headless).
pub(crate) fn update_cursor(
    windows: Query<&Window, With<PrimaryWindow>>,
    stand_in: Option<Res<CursorOverride>>,
    cameras: Query<&Camera, With<RtsCamera>>,
    rig: Res<CameraRig>,
    map: Option<Res<MapInfo>>,
    surface: Option<Res<TerrainSurface>>,
    spatial: Res<SpatialIndex>,
    viewer: Viewer,
    alive: Query<(), Without<Dead>>,
    mut cursor: ResMut<CursorWorld>,
) {
    let (Ok(window), Ok(camera)) = (windows.single(), cameras.single()) else { return };
    cursor.screen = mouse_position(window, stand_in.as_deref());
    let projector = Projector::new(camera, &rig);
    let ray = cursor.screen.filter(|_| !cursor.over_ui).and_then(|s| projector.ray(s));
    let Some(ray) = ray else {
        cursor.ground = None;
        cursor.hovered = None;
        return;
    };
    let hit = ground_hit(ray, surface.as_deref());
    let ground = hit.map(|p| clamp_to_map(p.xz(), map.as_deref()));
    // Rock the ray meets first hides every box behind it.
    let reach = hit.filter(|p| p.y > ROCK_ABOVE).map(|p| p.distance(ray.origin));
    cursor.ground = ground;
    cursor.hovered = pick(&spatial, ray, ground, reach, |e| alive.contains(e.entity) && viewer.sees(e.entity, e.owner, e.kind, &e.obb));
}

/// Where the ray meets the drawn world: the top of the drawn terrain while
/// af_render publishes it (mountains stand up to `MAX_TERRAIN_HEIGHT`),
/// else (headless, or no hit) the plane y = 0. Only the stretch of the ray
/// between [`SURFACE_TOP`] and [`SURFACE_BOTTOM`] is searched, so a ray
/// from far above costs no more than one from close by.
pub(crate) fn ground_hit(ray: Ray3d, surface: Option<&TerrainSurface>) -> Option<Vec3> {
    let plane = || ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y));
    let (origin, dir) = (ray.origin, *ray.direction);
    let Some(surface) = surface.filter(|_| dir.y < -1e-4) else { return plane() };
    let top = ((SURFACE_TOP - origin.y) / dir.y).max(0.0);
    let bottom = (SURFACE_BOTTOM - origin.y) / dir.y;
    if bottom <= top {
        return plane();
    }
    surface.raycast(origin + dir * top, dir, bottom - top).or_else(plane)
}

/// The visible entity whose box the ray hits first, within `reach` along
/// the ray when rock stands in the way; failing that, the nearest ground
/// unit within [`PICK_SLACK`] of the ground point.
pub(crate) fn pick(spatial: &SpatialIndex, ray: Ray3d, ground: Option<Vec2>, reach: Option<f32>, mut visible: impl FnMut(&SpatialEntry) -> bool) -> Option<Entity> {
    let (origin, dir) = (ray.origin, *ray.direction);
    // The box hit first is the nearest one: when it is behind the rock, so
    // is every other.
    let in_reach = |e: &SpatialEntry| reach.is_none_or(|r| e.obb.ray_hit(origin, dir, e.altitude, e.altitude + e.height).is_some_and(|t| t <= r));
    if let Some(hit) = spatial.pick(origin, dir, &mut visible).filter(in_reach) {
        return Some(hit.entity);
    }
    let g = ground?;
    spatial
        .nearest(g, PICK_SLACK, |e| e.kind == SpatialKind::Unit && !e.flying && visible(e))
        .map(|e| e.entity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(entity: Entity, kind: SpatialKind, pos: Vec2, half: f32, height: f32) -> SpatialEntry {
        let obb = Obb::new(pos, Vec2::splat(half), 0.0);
        SpatialEntry {
            entity,
            kind,
            type_index: 0,
            pos,
            obb,
            radius: obb.bounding_radius(),
            owner: None,
            team: af_geom::NO_TEAM,
            flying: false,
            altitude: 0.0,
            height,
            tags: TagSet::default(),
            under_construction: false,
            blocking: kind != SpatialKind::Unit,
        }
    }

    fn down_at(p: Vec2) -> Ray3d {
        // Looking down at 45 degrees towards +Z onto `p`.
        let dir = Vec3::new(0.0, -1.0, 1.0).normalize();
        Ray3d::new(Vec3::new(p.x, 10.0, p.y - 10.0), Dir3::new(dir).unwrap())
    }

    #[test]
    fn ray_hits_the_nearest_box_and_respects_the_filter() {
        let mut world = World::new();
        let (tall, small) = (world.spawn_empty().id(), world.spawn_empty().id());
        let mut index = SpatialIndex::new(Vec2::splat(64.0), 8.0);
        // A tall building in front of a unit along the ray.
        index.insert(entry(tall, SpatialKind::Building, Vec2::new(10.0, 8.0), 1.0, 6.0));
        index.insert(entry(small, SpatialKind::Unit, Vec2::new(10.0, 10.0), 0.4, 1.0));
        let ray = down_at(Vec2::new(10.0, 10.0));
        assert_eq!(pick(&index, ray, Some(Vec2::new(10.0, 10.0)), None, |_| true), Some(tall));
        assert_eq!(pick(&index, ray, Some(Vec2::new(10.0, 10.0)), None, |e| e.entity != tall), Some(small));
        assert_eq!(pick(&index, ray, Some(Vec2::new(10.0, 10.0)), None, |_| false), None);
    }

    #[test]
    fn near_misses_still_pick_small_units() {
        let mut world = World::new();
        let unit = world.spawn_empty().id();
        let mut index = SpatialIndex::new(Vec2::splat(64.0), 8.0);
        index.insert(entry(unit, SpatialKind::Unit, Vec2::new(20.0, 20.0), 0.3, 1.0));
        let ground = Vec2::new(20.6, 20.0);
        assert_eq!(pick(&index, down_at(ground), Some(ground), None, |_| true), Some(unit));
        let far = Vec2::new(22.0, 20.0);
        assert_eq!(pick(&index, down_at(far), Some(far), None, |_| true), None);
    }

    /// Flat ground at y = 0, with a block of rock `height` high over the
    /// cells x 20-23 (every z).
    fn ridge(height: f32) -> TerrainSurface {
        let (width, rows) = (41, 41);
        let top = (0..width * rows).map(|i| if (20..=24).contains(&(i % width)) { height } else { 0.0 }).collect();
        TerrainSurface { origin: Vec2::ZERO, cell: 1.0, width, height: rows, top, split: Vec::new() }
    }

    #[test]
    fn the_ground_point_is_on_the_drawn_terrain() {
        let rock = ridge(8.0);
        // Down at 45 degrees towards +Z: the y = 0 plane is 10 m ahead.
        let along = |x: f32| Ray3d::new(Vec3::new(x, 10.0, 0.0), Dir3::new(Vec3::new(0.0, -1.0, 1.0)).unwrap());
        let plane = ground_hit(along(5.0), None).unwrap();
        assert!(plane.distance(Vec3::new(5.0, 0.0, 10.0)) < 1e-4, "headless: the plane: {plane}");
        let flat = ground_hit(along(5.0), Some(&rock)).unwrap();
        assert!(flat.distance(Vec3::new(5.0, 0.0, 10.0)) < 0.01, "flat ground as before: {flat}");
        let top = ground_hit(along(22.0), Some(&rock)).unwrap();
        assert!(top.distance(Vec3::new(22.0, 8.0, 2.0)) < 0.01, "on the rock, not 8 m beyond it: {top}");
        // A shallow ray towards +X meets the ridge's near face (its edge
        // cell slopes from 0 at x = 19 to 8 m at x = 20), not the plane 28 m on.
        let shallow = Ray3d::new(Vec3::new(0.0, 12.0, 5.0), Dir3::new(Vec3::new(1.0, -0.25, 0.0)).unwrap());
        assert!((ground_hit(shallow, None).unwrap().x - 48.0).abs() < 1e-3);
        let face = ground_hit(shallow, Some(&rock)).unwrap();
        assert!((face.x - 164.0 / 8.25).abs() < 0.01 && face.y > 6.0, "{face}");
        // Rays that never come down still miss; from far above they still hit.
        assert_eq!(ground_hit(Ray3d::new(Vec3::new(5.0, 3.0, 5.0), Dir3::Y), Some(&rock)), None);
        let high = Ray3d::new(Vec3::new(22.0, 4000.0, 3.0), Dir3::NEG_Y);
        assert!(ground_hit(high, Some(&rock)).unwrap().distance(Vec3::new(22.0, 8.0, 3.0)) < 0.01);
    }

    #[test]
    fn rock_hides_the_boxes_behind_it() {
        let mut world = World::new();
        let (behind, front) = (world.spawn_empty().id(), world.spawn_empty().id());
        let mut index = SpatialIndex::new(Vec2::splat(64.0), 8.0);
        index.insert(entry(behind, SpatialKind::Unit, Vec2::new(10.0, 10.0), 0.4, 1.5));
        let ray = down_at(Vec2::new(10.0, 10.0));
        // Rock 8 m before the unit along the ray (and the ground point on it).
        let rock = ray.get_point(8.0);
        assert_eq!(pick(&index, ray, Some(rock.xz()), Some(8.0), |_| true), None);
        assert_eq!(pick(&index, ray, Some(Vec2::new(10.0, 10.0)), None, |_| true), Some(behind), "no rock in the way");
        // A box in front of the rock is still picked.
        index.insert(entry(front, SpatialKind::Building, Vec2::new(10.0, 5.0), 1.0, 9.0));
        assert_eq!(pick(&index, ray, Some(rock.xz()), Some(8.0), |_| true), Some(front));
    }
}
