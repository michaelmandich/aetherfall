//! Picking: what the mouse points at on the ground and among the boxes.

use af_core::ext::input::{CursorOverride, RtsCamera};
use af_core::prelude::*;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::{clamp_to_map, rig_transform};
use crate::viewer::Viewer;

/// Small ground units within this many metres of the cursor's ground point
/// count as hovered even when the ray misses their box.
const PICK_SLACK: f32 = 0.5;

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
    let ground = ray
        .plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))
        .map(|p| clamp_to_map(p.xz(), map.as_deref()));
    cursor.ground = ground;
    cursor.hovered = pick(&spatial, ray, ground, |e| alive.contains(e.entity) && viewer.sees(e.entity, e.owner, e.kind, &e.obb));
}

/// The visible entity whose box the ray hits first; failing that, the
/// nearest ground unit within [`PICK_SLACK`] of the ground point.
pub(crate) fn pick(spatial: &SpatialIndex, ray: Ray3d, ground: Option<Vec2>, mut visible: impl FnMut(&SpatialEntry) -> bool) -> Option<Entity> {
    if let Some(hit) = spatial.pick(ray.origin, *ray.direction, &mut visible) {
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
        assert_eq!(pick(&index, ray, Some(Vec2::new(10.0, 10.0)), |_| true), Some(tall));
        assert_eq!(pick(&index, ray, Some(Vec2::new(10.0, 10.0)), |e| e.entity != tall), Some(small));
        assert_eq!(pick(&index, ray, Some(Vec2::new(10.0, 10.0)), |_| false), None);
    }

    #[test]
    fn near_misses_still_pick_small_units() {
        let mut world = World::new();
        let unit = world.spawn_empty().id();
        let mut index = SpatialIndex::new(Vec2::splat(64.0), 8.0);
        index.insert(entry(unit, SpatialKind::Unit, Vec2::new(20.0, 20.0), 0.3, 1.0));
        let ground = Vec2::new(20.6, 20.0);
        assert_eq!(pick(&index, down_at(ground), Some(ground), |_| true), Some(unit));
        let far = Vec2::new(22.0, 20.0);
        assert_eq!(pick(&index, down_at(far), Some(far), |_| true), None);
    }
}
