//! The RTS camera: a perspective camera at `CameraRig::distance` from
//! `focus`, pitched down and looking along the fixed rig yaw.

use af_core::ext::input::{CursorOverride, RtsCamera};
use af_core::ext::map::MapFeatures;
use af_core::prelude::*;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::cursor::mouse_position;

/// Vertical field of view of the RTS camera, degrees.
pub const FOV_DEG: f32 = 32.0;
/// Closest zoom of the mouse wheel (metres from the focus). A rig set
/// closer from elsewhere (dev close-ups) stays there until the wheel turns.
pub const MIN_DISTANCE: f32 = 25.0;
/// Farthest zoom of the mouse wheel on maps up to the usual size (metres
/// from the focus); larger maps allow more ([`max_distance`]). A rig set
/// farther from elsewhere (a dev overview) stays there until the wheel turns.
pub const MAX_DISTANCE: f32 = 150.0;
/// Farthest zoom of the mouse wheel on the largest maps.
pub const MAX_DISTANCE_HUGE: f32 = 210.0;
/// Land side (metres) up to which [`MAX_DISTANCE`] holds: the largest
/// Medium map (eight players). Bigger land zooms out further, reaching
/// [`MAX_DISTANCE_HUGE`] at 1.6 times this (Huge).
const USUAL_LAND: f32 = 384.0;
/// The camera draws everything up to this far away (metres): a whole huge
/// map seen from a dev overview, a few kilometres out.
pub const FAR_PLANE: f32 = 8000.0;
/// Zoom a match starts at: a starting base (hall, houses, the nearest
/// resources) fits on screen.
pub const START_DISTANCE: f32 = 66.0;
/// Yaw of the RTS view: looking diagonally along -X -Z, so the camera sits
/// on the +Z side of what it frames and sees two faces of every box, the
/// fronts of buildings (which face +Z at yaw 0) among them.
pub const VIEW_YAW: f32 = 1.25 * std::f32::consts::PI;
/// The cursor scrolls the view within this many logical px of a window edge.
const EDGE_MARGIN_PX: f32 = 6.0;
/// Keyboard / edge pan speed in m/s per metre of camera distance, so a
/// zoomed-out view crosses the screen in about the same time.
const PAN_SPEED_PER_DISTANCE: f32 = 1.1;
/// Distance multiplier per wheel notch towards the ground.
const ZOOM_STEP: f32 = 0.88;
/// A long stall must not throw the camera across the map.
const MAX_FRAME_DT: f32 = 0.1;

/// Set when a match starts; the first frame with a map centres the camera.
#[derive(Resource, Default)]
pub(crate) struct StartFocus(bool);

/// Ground-plane `(forward, right)` of the view for `yaw`: forward points
/// into the screen, right towards the screen's right edge.
pub fn view_axes(yaw: f32) -> (Vec2, Vec2) {
    let f = af_geom::forward(yaw);
    // The camera's local +X (screen right) is forward x up.
    (f, Vec2::new(-f.y, f.x))
}

/// The camera transform for `rig`: `distance` from the focus on the ground,
/// `pitch` below the horizon, looking at the focus along the rig yaw.
pub fn rig_transform(rig: &CameraRig) -> Transform {
    let (forward, _) = view_axes(rig.yaw);
    let focus = Vec3::new(rig.focus.x, 0.0, rig.focus.y);
    let (sin, cos) = rig.pitch.sin_cos();
    let back = Vec3::new(-forward.x * cos, sin, -forward.y * cos);
    Transform::from_translation(focus + back * rig.distance).looking_at(focus, Vec3::Y)
}

/// `p` kept on the map (no clamp before the map exists).
pub(crate) fn clamp_to_map(p: Vec2, map: Option<&MapInfo>) -> Vec2 {
    match map {
        Some(m) if m.size.x > 0.0 && m.size.y > 0.0 => p.clamp(Vec2::ZERO, m.size),
        _ => p,
    }
}

/// Move the camera to look at `p`.
pub(crate) fn focus_on(rig: &mut CameraRig, p: Vec2, map: Option<&MapInfo>) {
    rig.focus = clamp_to_map(p, map);
}

/// Screen-edge scroll direction (x right, y up the screen) for a cursor in
/// a `window`-sized area; each axis is -1, 0 or 1.
pub(crate) fn edge_scroll(cursor: Vec2, window: Vec2, margin: f32) -> Vec2 {
    let axis = |v: f32, size: f32| {
        if v <= margin {
            -1.0
        } else if v >= size - margin {
            1.0
        } else {
            0.0
        }
    };
    // Screen y grows downwards; the top edge pans forward (+y).
    Vec2::new(axis(cursor.x, window.x), -axis(cursor.y, window.y))
}

/// Ground metres covered by one screen pixel at the focus.
fn metres_per_pixel(distance: f32, viewport_height: f32) -> f32 {
    2.0 * distance * (FOV_DEG.to_radians() * 0.5).tan() / viewport_height.max(1.0)
}

/// Focus offset that keeps the ground under the cursor while it moves by
/// `delta` px (middle-mouse drag).
pub(crate) fn grab_offset(rig: &CameraRig, delta: Vec2, viewport_height: f32) -> Vec2 {
    let (forward, right) = view_axes(rig.yaw);
    let mpp = metres_per_pixel(rig.distance, viewport_height);
    // The ground is foreshortened vertically by the pitch.
    -right * delta.x * mpp + forward * delta.y * mpp / rig.pitch.sin().max(0.2)
}

/// A middle-button grab of the ground, followed frame by frame.
#[derive(Default)]
pub(crate) struct Grab {
    active: bool,
    /// Cursor position last frame, `None` while it was outside the window.
    last: Option<Vec2>,
}

impl Grab {
    /// Advance one frame and return how far the cursor dragged (px).
    /// `start`: the button went down over the world; `held`: it is down;
    /// `cursor`: the in-window cursor. Leaving the window pauses the drag,
    /// so coming back elsewhere does not jump the view.
    pub fn step(&mut self, start: bool, held: bool, cursor: Option<Vec2>) -> Vec2 {
        if !held {
            *self = Grab::default();
            return Vec2::ZERO;
        }
        if start {
            self.active = true;
        } else if !self.active {
            return Vec2::ZERO;
        }
        let delta = match (start, self.last, cursor) {
            (false, Some(last), Some(now)) => now - last,
            _ => Vec2::ZERO,
        };
        self.last = cursor;
        delta
    }
}

/// New camera distance after `notches` wheel notches (positive = closer),
/// zooming out at most to `max`.
pub(crate) fn zoomed(distance: f32, notches: f32, max: f32) -> f32 {
    (distance * ZOOM_STEP.powf(notches)).clamp(MIN_DISTANCE, max)
}

/// Farthest wheel zoom on this map: [`MAX_DISTANCE`] on the usual sizes,
/// rising with the land (its size option, or its side past the largest
/// Medium map) to [`MAX_DISTANCE_HUGE`] on Huge maps, so a big map is still
/// easy to look over without the camera feeling different on the others.
pub fn max_distance(map: Option<&MapInfo>, features: Option<&MapFeatures>) -> f32 {
    let land = match (features, map) {
        (Some(f), _) => (f.land_max - f.land_min).max_element(),
        (None, Some(m)) => m.size.max_element(),
        (None, None) => 0.0,
    };
    let factor = features.map_or(1.0, |f| f.options.size.factor()).max(land / USUAL_LAND);
    let huge = af_core::ext::map::MapSize::Huge.factor();
    let t = ((factor - 1.0) / (huge - 1.0)).clamp(0.0, 1.0);
    MAX_DISTANCE + (MAX_DISTANCE_HUGE - MAX_DISTANCE) * t
}

/// Arrow-key direction (x right, y up the screen).
fn arrow_keys(keys: &ButtonInput<KeyCode>) -> Vec2 {
    let axis = |neg: KeyCode, pos: KeyCode| keys.pressed(pos) as i32 as f32 - keys.pressed(neg) as i32 as f32;
    Vec2::new(axis(KeyCode::ArrowLeft, KeyCode::ArrowRight), axis(KeyCode::ArrowDown, KeyCode::ArrowUp))
}

pub(crate) fn spawn_camera(mut commands: Commands, rig: Res<CameraRig>, mut start: ResMut<StartFocus>) {
    commands.spawn((
        Name::new("RTS camera"),
        RtsCamera,
        Camera3d::default(),
        // The projection's depth is infinite and reversed, so a far plane
        // this far costs no precision; it only culls.
        Projection::from(PerspectiveProjection { fov: FOV_DEG.to_radians(), near: 0.5, far: FAR_PLANE, ..default() }),
        rig_transform(&rig),
    ));
    start.0 = true;
}

/// Remove the camera and any click markers still alive when the match ends.
pub(crate) fn despawn_camera(mut commands: Commands, cameras: Query<Entity, With<RtsCamera>>, pings: Query<Entity, With<ClickPing>>) {
    for e in cameras.iter().chain(pings.iter()) {
        commands.entity(e).try_despawn();
    }
}

/// Once per match: look at the local player's start; a spectator looks at
/// the main meteor site (the middle of the land, which the middle of the
/// map is not when a sea edge widens it), else the map centre.
pub(crate) fn focus_match_start(
    mut start: ResMut<StartFocus>,
    local: Res<LocalPlayer>,
    players: Res<Players>,
    map: Option<Res<MapInfo>>,
    features: Option<Res<MapFeatures>>,
    mut rig: ResMut<CameraRig>,
) {
    if !start.0 {
        return;
    }
    let Some(map) = map else { return };
    let middle = features.as_ref().and_then(|f| f.sites.first()).map_or(map.size * 0.5, |s| s.center);
    let target = local.0.and_then(|p| players.try_get(p)).map_or(middle, |p| p.start_pos);
    *rig = CameraRig { distance: START_DISTANCE, yaw: VIEW_YAW, ..CameraRig::default() };
    focus_on(&mut rig, target, Some(&map));
    start.0 = false;
}

/// Pan with arrows, screen edges and middle drag; zoom with the wheel.
pub(crate) fn camera_control(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time<Real>>,
    cursor: Res<CursorWorld>,
    windows: Query<&Window, With<PrimaryWindow>>,
    stand_in: Option<Res<CursorOverride>>,
    map: Option<Res<MapInfo>>,
    features: Option<Res<MapFeatures>>,
    mut rig: ResMut<CameraRig>,
    mut grab: Local<Grab>,
) {
    let window = windows.single().ok();
    // The real mouse only counts while the window has focus; a stand-in always does.
    let stand_in = stand_in.as_deref().filter(|o| o.0.is_some());
    let window_cursor = window.filter(|w| w.focused || stand_in.is_some()).and_then(|w| mouse_position(w, stand_in));
    let dt = time.delta_secs().min(MAX_FRAME_DT);
    let (forward, right) = view_axes(rig.yaw);

    let mut pan = arrow_keys(&keys);
    if let (Some(w), Some(c)) = (window, window_cursor) {
        pan += edge_scroll(c, w.size(), EDGE_MARGIN_PX);
    }
    let pan = pan.clamp_length_max(1.0);
    let mut focus = rig.focus + (right * pan.x + forward * pan.y) * PAN_SPEED_PER_DISTANCE * rig.distance * dt;

    // Middle drag grabs the ground; a drag only starts outside the UI.
    let start = mouse.just_pressed(MouseButton::Middle) && !cursor.over_ui;
    let dragged = grab.step(start, mouse.pressed(MouseButton::Middle), window_cursor);
    if let Some(w) = window {
        focus += grab_offset(&rig, dragged, w.height());
    }

    // A stand-in mouse keeps the real wheel out too.
    let notches = match scroll.unit {
        _ if stand_in.is_some() => 0.0,
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
    };
    let distance = if cursor.over_ui || notches == 0.0 { rig.distance } else { zoomed(rig.distance, notches, max_distance(map.as_deref(), features.as_deref())) };

    let focus = clamp_to_map(focus, map.as_deref());
    if focus != rig.focus || distance != rig.distance {
        rig.focus = focus;
        rig.distance = distance;
    }
}

/// Put the camera where the rig says (only touching it when it moved).
pub(crate) fn sync_camera_transform(rig: Res<CameraRig>, mut cameras: Query<&mut Transform, With<RtsCamera>>) {
    let want = rig_transform(&rig);
    for mut t in &mut cameras {
        t.set_if_neq(want);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig(focus: Vec2, distance: f32) -> CameraRig {
        CameraRig { focus, distance, ..default() }
    }

    #[test]
    fn camera_sits_at_distance_and_looks_at_focus() {
        let r = rig(Vec2::new(40.0, 70.0), 60.0);
        let t = rig_transform(&r);
        let focus = Vec3::new(40.0, 0.0, 70.0);
        assert!((t.translation.distance(focus) - 60.0).abs() < 1e-3);
        let look = (focus - t.translation).normalize();
        assert!(t.forward().dot(look) > 0.9999, "looks at the focus");
        // Pitch below the horizon matches the rig.
        assert!((t.translation.y - 60.0 * r.pitch.sin()).abs() < 1e-3);
    }

    #[test]
    fn view_axes_match_the_camera_basis() {
        let r = rig(Vec2::ZERO, 50.0);
        let t = rig_transform(&r);
        let (forward, right) = view_axes(r.yaw);
        let cam_right = t.right().as_vec3().xz().normalize();
        let cam_forward = t.forward().as_vec3().xz().normalize();
        assert!((cam_right - right).length() < 1e-4, "{cam_right} vs {right}");
        assert!((cam_forward - forward).length() < 1e-4, "{cam_forward} vs {forward}");
    }

    #[test]
    fn focus_is_clamped_to_the_map() {
        let map = MapInfo { size: Vec2::new(100.0, 80.0), ..default() };
        assert_eq!(clamp_to_map(Vec2::new(-5.0, 90.0), Some(&map)), Vec2::new(0.0, 80.0));
        assert_eq!(clamp_to_map(Vec2::new(50.0, 40.0), Some(&map)), Vec2::new(50.0, 40.0));
        assert_eq!(clamp_to_map(Vec2::new(-5.0, 90.0), None), Vec2::new(-5.0, 90.0));
    }

    #[test]
    fn edges_scroll_and_centre_does_not() {
        let w = Vec2::new(1280.0, 720.0);
        assert_eq!(edge_scroll(Vec2::new(640.0, 360.0), w, 6.0), Vec2::ZERO);
        assert_eq!(edge_scroll(Vec2::new(2.0, 360.0), w, 6.0), Vec2::new(-1.0, 0.0));
        assert_eq!(edge_scroll(Vec2::new(1279.0, 1.0), w, 6.0), Vec2::new(1.0, 1.0));
        assert_eq!(edge_scroll(Vec2::new(640.0, 717.0), w, 6.0), Vec2::new(0.0, -1.0));
    }

    #[test]
    fn zoom_is_clamped_and_wheel_up_moves_closer() {
        assert!(zoomed(55.0, 1.0, MAX_DISTANCE) < 55.0);
        assert!(zoomed(55.0, -1.0, MAX_DISTANCE) > 55.0);
        assert_eq!(zoomed(30.0, 10.0, MAX_DISTANCE), MIN_DISTANCE);
        assert_eq!(zoomed(140.0, -10.0, MAX_DISTANCE), MAX_DISTANCE);
        assert_eq!(zoomed(140.0, -10.0, MAX_DISTANCE_HUGE), MAX_DISTANCE_HUGE);
        // From a dev overview far out, the first notch comes back in range.
        assert_eq!(zoomed(900.0, 1.0, MAX_DISTANCE), MAX_DISTANCE);
    }

    /// Map features for a map of `size` with land of side `land`.
    fn features(size: af_core::ext::map::MapSize, land: f32) -> MapFeatures {
        use af_core::ext::map::{ResolvedMapOptions, SiteKind, WaterLevel};
        MapFeatures {
            options: ResolvedMapOptions { size, water: WaterLevel::Lakes, rivers: 1, site: SiteKind::Crater },
            land_min: Vec2::ZERO,
            land_max: Vec2::splat(land),
            ocean_edges: Vec::new(),
            sites: Vec::new(),
            rivers: Vec::new(),
            mountains: Vec::new(),
        }
    }

    #[test]
    fn the_zoom_reaches_further_on_bigger_maps() {
        use af_core::ext::map::MapSize;
        let map = |side: f32| MapInfo { size: Vec2::splat(side), ..default() };
        // Today's maps keep today's limit.
        assert_eq!(max_distance(None, None), MAX_DISTANCE);
        for side in [160.0, 224.0, 288.0, 384.0] {
            assert_eq!(max_distance(Some(&map(side)), None), MAX_DISTANCE, "{side} m");
        }
        // By the size option: Small and Medium as today, Large between, Huge furthest.
        let by = |size: MapSize, land: f32| max_distance(Some(&map(land)), Some(&features(size, land)));
        assert_eq!(by(MapSize::Small, 176.0), MAX_DISTANCE);
        assert_eq!(by(MapSize::Medium, 384.0), MAX_DISTANCE);
        let large = by(MapSize::Large, 288.0);
        assert!(large > MAX_DISTANCE && large < MAX_DISTANCE_HUGE, "{large}");
        assert!((by(MapSize::Huge, 360.0) - MAX_DISTANCE_HUGE).abs() < 1e-3, "two-player Huge");
        assert!((by(MapSize::Huge, 616.0) - MAX_DISTANCE_HUGE).abs() < 1e-3, "eight-player Huge");
        // A side set in metres counts by its land; an ocean band does not.
        assert!((max_distance(Some(&map(1024.0)), None) - MAX_DISTANCE_HUGE).abs() < 1e-3);
        assert!(max_distance(Some(&map(480.0)), None) > MAX_DISTANCE);
        let coast = MapInfo { size: Vec2::splat(224.0 * 1.17), ..default() };
        assert_eq!(max_distance(Some(&coast), Some(&features(MapSize::Medium, 224.0))), MAX_DISTANCE);
    }

    #[test]
    fn grab_follows_the_cursor_and_survives_leaving_the_window() {
        let mut g = Grab::default();
        let at = |x: f32, y: f32| Some(Vec2::new(x, y));
        assert_eq!(g.step(true, true, at(100.0, 100.0)), Vec2::ZERO, "the press itself does not move");
        assert_eq!(g.step(false, true, at(110.0, 96.0)), Vec2::new(10.0, -4.0));
        assert_eq!(g.step(false, true, None), Vec2::ZERO, "outside the window");
        assert_eq!(g.step(false, true, at(600.0, 400.0)), Vec2::ZERO, "re-entering elsewhere does not jump");
        assert_eq!(g.step(false, true, at(605.0, 400.0)), Vec2::new(5.0, 0.0));
        assert_eq!(g.step(false, false, at(700.0, 400.0)), Vec2::ZERO, "released");
        assert_eq!(g.step(false, true, at(720.0, 400.0)), Vec2::ZERO, "held without a fresh press (e.g. pressed over UI)");
    }

    #[test]
    fn grabbing_moves_against_the_drag() {
        let r = rig(Vec2::ZERO, 55.0);
        let (forward, right) = view_axes(r.yaw);
        // Dragging right pulls the world right: the view moves left.
        assert!(grab_offset(&r, Vec2::new(10.0, 0.0), 720.0).dot(right) < 0.0);
        // Dragging down pulls the world down: the view moves forward.
        assert!(grab_offset(&r, Vec2::new(0.0, 10.0), 720.0).dot(forward) > 0.0);
        // One screen height of drag covers the visible ground height at the focus.
        let mpp = metres_per_pixel(55.0, 720.0);
        assert!((mpp * 720.0 - 2.0 * 55.0 * (FOV_DEG.to_radians() / 2.0).tan()).abs() < 1e-3);
    }
}
