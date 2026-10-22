//! Click markers and the placement slam: in-world feedback for the local
//! player's orders.
//!
//! **Click markers.** af_input and af_ui spawn a `ClickPing`, with a
//! `ClickMark` naming the order, for every order the local player gives
//! (`af_core::ext::input`). [`attach_markers`] gives each new ping its parts
//! as children and [`animate_markers`] plays them over the ping's life in
//! real time (they keep playing while the game is paused), then despawns
//! the ping:
//!
//! * on a point, four arrows high above the spot swoop down and in,
//!   pointing down at it and speeding up; where they meet, a ground ring
//!   contracts and a flash pops;
//! * on an entity (a unit attacked or followed, a node gathered, a
//!   foundation built, ...), the arrows press in on a ring (units) or a
//!   frame (buildings, nodes) hugging its footprint, which pulses twice
//!   and follows it while the local team sees it;
//! * a building placed flashes its footprint on the ground;
//! * shift-queued orders are smaller and quicker and leave a dot that
//!   lingers, so a chain of them reads.
//!
//! Colours are the order's (`OrderMark::color`, also the ping's). Arrows,
//! rings and flashes grow with the camera distance so they read at any
//! zoom ([`marker_size`]); frames and hugging rings keep the footprint's
//! size and a readable line width. At most [`MAX_MARKERS`] play at once: a
//! burst of clicks drops the oldest.
//!
//! **Placement slam.** When a foundation of the local player appears, a
//! stamp ([`SLAM_SECS`]): its footprint flashes, a shockwave the shape of
//! the footprint runs outward, dust puffs roll out and rise, and chips of
//! wood and stone are thrown up and fall back spinning. Each slam is a
//! top-level `RenderScene` entity; at most [`MAX_SLAMS`] play at once.
//!
//! Every fading layer switches material only when its quantised
//! [`MatKey`] changes, so nothing is created per frame. The curves are
//! pure functions ([`marker_pose`], [`arrow_transform`], the `slam_*`
//! functions), unit-tested without an app.

use std::f32::consts::{FRAC_PI_4, TAU};

use af_core::ext::input::{ClickMark, OrderMark};
use af_core::prelude::*;
use bevy::prelude::*;

use crate::assets::{BoxPart, Painter, PartQuery, RING_INNER};
use crate::entities::Ground;
use crate::look::{Look, MatKey, Shade, hash32, lighten};
use crate::motion::rotation_for_dir;
use crate::parts::{frame_boxes, min_line_width};
use crate::scene::RenderScene;

/// Most click markers playing at once; a burst of clicks drops the oldest.
pub const MAX_MARKERS: usize = 16;
/// Camera distance (metres) at which arrows, rings and flashes have their
/// design size ([`marker_size`]).
const DESIGN_DISTANCE: f32 = 36.0;
/// Size of a queued marker against an ordinary one.
const QUEUED_SIZE: f32 = 0.7;
/// Height of a marker above the drawn ground.
const LIFT: f32 = 0.05;
/// Arrow length (metres at size 1).
const ARROW_LENGTH: f32 = 0.9;
/// How steeply the arrows dive (radians below the horizontal, about 57 degrees).
const DIVE_ANGLE: f32 = 1.0;
/// How far the arrows travel along their own axis (metres at size 1).
const DIVE: f32 = 2.4;
/// Height of a landed arrow's tip above the ground (metres at size 1).
const TIP_Y: f32 = 0.12;
/// How far from a point the arrow tips land (metres at size 1).
const POINT_REACH: f32 = 0.22;
/// Share of the landing (land..settle) the arrows take to fade after landing.
const ARROW_FADE: f32 = 0.3;
/// Gap between a hugged footprint and its frame or ring (metres).
const HUG_GAP: f32 = 0.25;
/// How far a hugging frame swells at the top of a pulse (metres).
const FRAME_SWELL: f32 = 0.3;
/// How much a hugging ring swells at the top of a pulse (share of its radius).
const RING_SWELL: f32 = 0.14;
/// The contracting ring's radius as it appears and as it ends (size 1).
const RING_FROM: f32 = 1.3;
const RING_TO: f32 = 0.3;
/// The flash's radius as it pops and as it fades (size 1).
const FLASH_FROM: f32 = 0.15;
const FLASH_TO: f32 = 0.8;
/// Share of the landing (land..settle) the flash lasts.
const FLASH_SHARE: f32 = 0.35;
/// Radius of a queued marker's lingering dot (size 1).
const DOT: f32 = 0.16;
/// Where the four arrows come from, in the marker's frame.
const SIDES: [Vec3; 4] = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z];

/// What a marker hugs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hug {
    /// A spot on the ground.
    Point,
    /// A unit: a ring of this radius around it.
    Ring(f32),
    /// A building, node or placement: a frame around these half extents
    /// (the marker is turned with the footprint).
    Frame(Vec2),
}

/// How a marker looks, fixed when it is made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkerSpec {
    pub hug: Hug,
    /// Arrows dive in (everything but a placement).
    pub arrows: bool,
    pub queued: bool,
    /// Scale of the arrows, the contracting ring, the flash and the dot
    /// ([`marker_size`]).
    pub size: f32,
    /// Thinnest line drawn (`parts::min_line_width`).
    pub line: f32,
}

/// Parts of one kind at one moment: their transforms in the marker's frame
/// and their shared opacity (0 = hidden).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer<const N: usize> {
    pub transforms: [Transform; N],
    pub alpha: f32,
}

/// A marker at one moment of its life. Which layers are present depends
/// only on its [`MarkerSpec`]; a layer not showing yet (or any more) has
/// alpha 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MarkerPose {
    pub arrows: Option<Layer<4>>,
    /// A point's contracting ring, or the ring hugging a unit.
    pub ring: Option<Layer<1>>,
    pub flash: Option<Layer<1>>,
    /// A queued point's lingering dot.
    pub dot: Option<Layer<1>>,
    /// The frame hugging a building, node or placement.
    pub frame: Option<Layer<4>>,
}

/// When, as fractions of a marker's life, its arrows land and its landing
/// (ring, flash) and pulses are over.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Timeline {
    land: f32,
    settle: f32,
}

impl Timeline {
    /// Ordinary markers land at 0.36 s and play to the end of their
    /// `ClickMark::SECS`; queued ones land at 0.28 s and settle by 0.6 s,
    /// then only their dot lingers (to `ClickMark::QUEUED_SECS`). A ping
    /// with another life plays the same fractions of it.
    fn of(queued: bool) -> Timeline {
        let (life, land, settle) = if queued { (ClickMark::QUEUED_SECS, 0.28, 0.6) } else { (ClickMark::SECS, 0.36, ClickMark::SECS) };
        Timeline { land: land / life, settle: settle / life }
    }
}

/// Size of arrows, rings and flashes with the camera `camera_distance`
/// metres from its focus: about the same on screen at any zoom, a little
/// smaller close up.
pub fn marker_size(camera_distance: f32) -> f32 {
    (camera_distance / DESIGN_DISTANCE).clamp(0.8, 3.5)
}

/// Slow start, fast finish (0..1).
fn ease_in(u: f32) -> f32 {
    u * u
}

/// Fast start, slow finish (0..1).
fn ease_out(u: f32) -> f32 {
    1.0 - (1.0 - u) * (1.0 - u)
}

/// 0 below `a`, 1 above `b`, smooth between.
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Rises 0 -> 1 -> 0 twice over `f` = 0..1: a hug's two pulses.
pub fn pulses(f: f32) -> f32 {
    let s = (TAU * f.clamp(0.0, 1.0)).sin();
    s * s
}

/// The arrow coming from `side` (a unit vector in the ground plane) at `u`
/// of its dive (0: start, 1: landed). It points down and in,
/// [`DIVE_ANGLE`] below the horizontal, and lands with its tip `reach`
/// metres out from the centre, just above the ground; it starts [`DIVE`]
/// back along its own axis (up and out) and speeds up as it comes.
/// `size` scales the arrow and its dive.
pub fn arrow_transform(side: Vec3, reach: f32, u: f32, size: f32) -> Transform {
    let dir = -side * DIVE_ANGLE.cos() - Vec3::Y * DIVE_ANGLE.sin();
    let landed = side * reach + Vec3::Y * TIP_Y * size;
    let tip = landed - dir * DIVE * size * (1.0 - ease_in(u.clamp(0.0, 1.0)));
    let length = ARROW_LENGTH * size;
    // The arrow mesh is one metre long with its tip at +0.5 along +Z.
    Transform::from_translation(tip - dir * length * 0.5).with_rotation(rotation_for_dir(dir)).with_scale(Vec3::splat(length))
}

/// Side width of a hugging frame: like a selection outline's, never
/// thinner than `line`.
fn frame_width(half: Vec2, line: f32) -> f32 {
    (half.min_element() * 0.12).clamp(0.08, 0.22).max(line)
}

/// A hugging ring's drawn radius: never so small that its band is
/// thinner than `line`.
fn hug_radius(radius: f32, line: f32) -> f32 {
    radius.max(line / (1.0 - RING_INNER))
}

/// How far out from the centre the arrow from `side` lands.
fn reach(spec: &MarkerSpec, side: Vec3) -> f32 {
    match spec.hug {
        Hug::Point => POINT_REACH * spec.size,
        Hug::Ring(r) => hug_radius(r, spec.line),
        Hug::Frame(half) => {
            let along = if side.x != 0.0 { half.x } else { half.y };
            along + HUG_GAP + frame_width(half, spec.line)
        }
    }
}

/// A flat shape (ring, disc) of `radius` at local height `y`.
fn flat(radius: f32, y: f32) -> Transform {
    Transform::from_xyz(0.0, y, 0.0).with_scale(Vec3::splat(radius.max(0.001)))
}

/// A hug's opacity at `f` = 0..1 of its pulses: brightest at each pulse,
/// fading out over the last part.
fn hug_alpha(f: f32) -> f32 {
    (0.5 + 0.5 * pulses(f)) * (1.0 - smoothstep(0.7, 1.0, f))
}

/// The marker described by `spec` at `t` = 0..1 of its life.
pub fn marker_pose(spec: &MarkerSpec, t: f32) -> MarkerPose {
    let t = t.clamp(0.0, 1.0);
    let tl = Timeline::of(spec.queued);
    let size = spec.size;
    let landing = tl.settle - tl.land;
    // 0..1 through the landing (ring, flash); negative before it.
    let w = (t - tl.land) / landing;
    let mut pose = MarkerPose::default();
    if spec.arrows {
        let (u, alpha, shrink) = if t < tl.land {
            let u = t / tl.land;
            (u, (u / 0.2).min(1.0), 1.0)
        } else {
            let v = (w / ARROW_FADE).min(1.0);
            (1.0, 1.0 - v, 1.0 - 0.4 * v)
        };
        pose.arrows = Some(Layer { transforms: SIDES.map(|side| arrow_transform(side, reach(spec, side), u, size * shrink)), alpha });
    }
    let hug = (t / tl.settle).min(1.0);
    match spec.hug {
        Hug::Point => {
            let landed = (0.0..1.0).contains(&w);
            let r = RING_FROM + (RING_TO - RING_FROM) * ease_out(w.clamp(0.0, 1.0));
            pose.ring = Some(Layer { transforms: [flat(r * size, 0.02)], alpha: if landed { 0.9 * (1.0 - w) } else { 0.0 } });
            let f = w / FLASH_SHARE;
            let r = FLASH_FROM + (FLASH_TO - FLASH_FROM) * ease_out(f.clamp(0.0, 1.0));
            let alpha = if (0.0..1.0).contains(&f) { 0.9 * (1.0 - f) } else { 0.0 };
            pose.flash = Some(Layer { transforms: [flat(r * size, 0.04)], alpha });
            if spec.queued {
                let grown = ((t - tl.land) / 0.05).clamp(0.0, 1.0);
                let alpha = if t >= tl.land { 0.9 * (1.0 - smoothstep(0.75, 1.0, t)) } else { 0.0 };
                pose.dot = Some(Layer { transforms: [flat(DOT * size * grown, 0.06)], alpha });
            }
        }
        Hug::Ring(r) => {
            let r = hug_radius(r, spec.line) * (1.0 + RING_SWELL * pulses(hug));
            pose.ring = Some(Layer { transforms: [flat(r, 0.02)], alpha: hug_alpha(hug) });
        }
        Hug::Frame(half) => {
            let boxes = frame_boxes(half, HUG_GAP + FRAME_SWELL * pulses(hug), frame_width(half, spec.line), 0.03);
            pose.frame = Some(Layer { transforms: boxes.map(|(c, s)| Transform::from_translation(c).with_scale(s)), alpha: hug_alpha(hug) });
        }
    }
    pose
}

/// Present on a click marker (its ping) once it has its parts.
#[derive(Component, Debug)]
pub struct Marker {
    spec: MarkerSpec,
    /// Seconds it lives in all (its ping's starting `time_left`).
    life: f32,
    color: Rgb,
    /// The entity it hugs and follows; dropped once gone or out of sight.
    target: Option<Entity>,
    arrows: Option<[Entity; 4]>,
    ring: Option<Entity>,
    flash: Option<Entity>,
    dot: Option<Entity>,
    frame: Option<[Entity; 4]>,
    /// The material each layer is drawn with: arrows, ring, flash, dot, frame.
    keys: [Option<MatKey>; 5],
}

impl Marker {
    /// Seconds it has played.
    fn age(&self, ping: &ClickPing) -> f32 {
        self.life - ping.time_left
    }
}

/// What a marker for `mark` hugs and which way it is turned, given what
/// its target is (`footprint`, `facing`, whether it is a unit).
fn marker_hug(data: &GameData, mark: &ClickMark, target: Option<(&Footprint, Option<&Facing>, bool)>, view_yaw: f32) -> (Hug, bool, f32) {
    match (mark.kind, target) {
        (OrderMark::Place { kind, facing }, _) => (Hug::Frame(Footprint::from_size(data.building(kind).size).half), false, facing),
        (_, Some((fp, _, true))) => (Hug::Ring(fp.half.length() + HUG_GAP), true, view_yaw),
        (_, Some((fp, facing, false))) => (Hug::Frame(fp.half), true, facing.map_or(0.0, |f| f.0)),
        (_, None) => (Hug::Point, true, view_yaw),
    }
}

/// Whether a layer at `alpha` shows at all (it rounds to a visible
/// material).
fn shows(alpha: f32) -> bool {
    alpha * MatKey::ALPHA_LEVELS >= 0.5
}

/// The material for `look` when its key differs from the one `key` says is
/// in use (remembering the new one); `None` when nothing needs to change.
fn refresh(painter: &mut Painter, key: &mut Option<MatKey>, look: Look) -> Option<Handle<StandardMaterial>> {
    let k = MatKey::new(look);
    if *key == Some(k) {
        return None;
    }
    *key = Some(k);
    Some(painter.material(look))
}

/// Lay out parts drawn with `look` at their transforms, all hidden while
/// `look` is invisible; `key` is the layer's material in use.
fn draw(painter: &mut Painter, parts: &mut PartQuery, look: Look, key: &mut Option<MatKey>, items: impl IntoIterator<Item = (Entity, Transform)>) {
    let shown = shows(look.alpha);
    let material = if shown { refresh(painter, key, look) } else { None };
    for (id, place) in items {
        let Ok((mut tf, mut vis, mat)) = parts.get_mut(id) else { continue };
        vis.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
        if shown {
            tf.set_if_neq(place);
            if let (Some(h), Some(mut m)) = (&material, mat) {
                m.0 = h.clone();
            }
        }
    }
}

/// Spawn a part of `mesh` under `root` drawn with `look`, hidden (with a
/// visible material it swaps on the first frame it shows) while `look` is
/// invisible.
fn spawn_part(commands: &mut Commands, painter: &mut Painter, root: Entity, mesh: &Handle<Mesh>, look: Look, tf: Transform) -> Entity {
    let shown = shows(look.alpha);
    let e = painter.spawn_shape(commands, root, mesh.clone(), if shown { look } else { Look { alpha: 1.0, ..look } }, tf);
    if !shown {
        commands.entity(e).insert(Visibility::Hidden);
    }
    e
}

/// The material key a layer starts with: its look's, if it shows.
fn first_key(look: Look) -> Option<MatKey> {
    shows(look.alpha).then(|| MatKey::new(look))
}

/// Spawn one layer of `mesh` parts under `root` at their first transforms.
fn spawn_layer<const N: usize>(commands: &mut Commands, painter: &mut Painter, root: Entity, mesh: &Handle<Mesh>, color: Rgb, layer: &Layer<N>, key: &mut Option<MatKey>) -> [Entity; N] {
    let look = Look::unlit(color, layer.alpha);
    *key = first_key(look);
    layer.transforms.map(|tf| spawn_part(commands, painter, root, mesh, look, tf))
}

/// A marker's parts, spawned as children of its ping `root`.
fn spawn_marker(commands: &mut Commands, painter: &mut Painter, root: Entity, spec: MarkerSpec, life: f32, color: Rgb, target: Option<Entity>) -> Marker {
    let pose = marker_pose(&spec, 0.0);
    let (arrow, ring, disc, cube) = (painter.arrow(), painter.ring(), painter.disc(), painter.cube());
    let mut keys = [None; 5];
    let [ka, kr, kf, kd, kfr] = &mut keys;
    let arrows = pose.arrows.map(|l| spawn_layer(commands, painter, root, &arrow, arrow_color(color), &l, ka));
    let ring = pose.ring.map(|l| spawn_layer(commands, painter, root, &ring, color, &l, kr)[0]);
    let flash = pose.flash.map(|l| spawn_layer(commands, painter, root, &disc, flash_color(color), &l, kf)[0]);
    let dot = pose.dot.map(|l| spawn_layer(commands, painter, root, &disc, color, &l, kd)[0]);
    let frame = pose.frame.map(|l| spawn_layer(commands, painter, root, &cube, color, &l, kfr));
    Marker { spec, life, color, target, arrows, ring, flash, dot, frame, keys }
}

/// The arrows: the marker's colour a little lighter, so they stand out
/// from the ground (green arrows on grass).
fn arrow_color(color: Rgb) -> Rgb {
    lighten(color, 0.12)
}

/// The flash where a point marker's arrows meet: its colour, near white.
fn flash_color(color: Rgb) -> Rgb {
    lighten(color, 0.6)
}

/// Dress each new click marker (`ClickPing`) with its parts, making room
/// first: past [`MAX_MARKERS`] the oldest markers go.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn attach_markers(
    mut commands: Commands,
    mut painter: Painter,
    ground: Ground,
    data: Res<Data>,
    rig: Res<CameraRig>,
    fresh: Query<(Entity, &ClickPing, Option<&ClickMark>), Without<Marker>>,
    playing: Query<(Entity, &ClickPing, &Marker)>,
    targets: Query<(&Position, &Footprint, Option<&Facing>, Has<Unit>)>,
) {
    if fresh.is_empty() {
        return;
    }
    let over = (playing.iter().count() + fresh.iter().count()).saturating_sub(MAX_MARKERS);
    if over > 0 {
        let mut oldest: Vec<(f32, Entity)> = playing.iter().map(|(e, ping, m)| (m.age(ping), e)).collect();
        oldest.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, e) in oldest.into_iter().take(over) {
            commands.entity(e).try_despawn();
        }
    }
    let size = marker_size(rig.distance);
    let line = min_line_width(rig.distance);
    // Arrows come in diagonally across the view, so none hides behind another.
    let view_yaw = rig.yaw + FRAC_PI_4;
    for (n, (e, ping, mark)) in fresh.iter().enumerate() {
        if n >= MAX_MARKERS {
            commands.entity(e).try_despawn();
            continue;
        }
        let mark = mark.copied().unwrap_or(ClickMark::new(OrderMark::Move, None, false));
        let target = mark.target.and_then(|t| targets.get(t).ok().map(|found| (t, found)));
        let (hug, arrows, yaw) = marker_hug(&data, &mark, target.map(|(_, (_, fp, facing, unit))| (fp, facing, unit)), view_yaw);
        // Only an entity hug follows its target (a placement has none).
        let follow = target.filter(|_| arrows && hug != Hug::Point);
        let centre = follow.map_or(ping.pos, |(_, (pos, ..))| pos.0);
        let spec = MarkerSpec { hug, arrows, queued: mark.queued, size: size * if mark.queued { QUEUED_SIZE } else { 1.0 }, line };
        let marker = spawn_marker(&mut commands, &mut painter, e, spec, ping.time_left.max(0.01), ping.color, follow.map(|(t, _)| t));
        commands.entity(e).try_insert((
            Transform::from_xyz(centre.x, ground.surface(centre) + LIFT, centre.y).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::Inherited,
            marker,
        ));
    }
}

/// Play the click markers in real time (they run on while paused), keep
/// entity hugs on their target while the local team sees it, and despawn
/// each ping when its time is up.
#[allow(clippy::type_complexity)]
pub(crate) fn animate_markers(
    mut commands: Commands,
    mut painter: Painter,
    time: Res<Time<Real>>,
    ground: Ground,
    mut markers: Query<(Entity, &mut ClickPing, &mut Marker, &mut Transform), Without<BoxPart>>,
    targets: Query<(&Transform, Option<&Visibility>), (Without<Marker>, Without<BoxPart>)>,
    mut parts: PartQuery,
) {
    let dt = time.delta_secs();
    for (e, mut ping, mut marker, mut tf) in &mut markers {
        ping.time_left -= dt;
        if ping.time_left <= 0.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        let m = &mut *marker;
        if let Some(t) = m.target {
            match targets.get(t) {
                Ok((at, vis)) if vis.is_none_or(|v| *v != Visibility::Hidden) => {
                    let p = at.translation.xz();
                    tf.translation = Vec3::new(p.x, ground.surface(p) + LIFT, p.y);
                }
                // Gone, or hidden by the fog: stay where it was last seen.
                _ => m.target = None,
            }
        }
        let pose = marker_pose(&m.spec, m.age(&ping) / m.life);
        let color = m.color;
        let [ka, kr, kf, kd, kfr] = &mut m.keys;
        if let (Some(ids), Some(l)) = (m.arrows, pose.arrows) {
            draw(&mut painter, &mut parts, Look::unlit(arrow_color(color), l.alpha), ka, ids.into_iter().zip(l.transforms));
        }
        if let (Some(id), Some(l)) = (m.ring, pose.ring) {
            draw(&mut painter, &mut parts, Look::unlit(color, l.alpha), kr, [(id, l.transforms[0])]);
        }
        if let (Some(id), Some(l)) = (m.flash, pose.flash) {
            draw(&mut painter, &mut parts, Look::unlit(flash_color(color), l.alpha), kf, [(id, l.transforms[0])]);
        }
        if let (Some(id), Some(l)) = (m.dot, pose.dot) {
            draw(&mut painter, &mut parts, Look::unlit(color, l.alpha), kd, [(id, l.transforms[0])]);
        }
        if let (Some(ids), Some(l)) = (m.frame, pose.frame) {
            draw(&mut painter, &mut parts, Look::unlit(color, l.alpha), kfr, ids.into_iter().zip(l.transforms));
        }
    }
}

// ---------------------------------------------------------------------------
// Placement slam
// ---------------------------------------------------------------------------

/// Seconds a placement slam plays.
pub const SLAM_SECS: f32 = 0.9;
/// Most placement slams playing at once (a wall line lays many
/// foundations at once); past it the oldest go.
pub const MAX_SLAMS: usize = 24;
/// Seconds the shockwave takes to run out and fade.
const WAVE_SECS: f32 = 0.55;
/// Pull on the chips (m/s²): a little more than the real thing, so they
/// land within the slam.
const SLAM_GRAVITY: f32 = 20.0;
/// One dust puff per this many metres of the footprint's edge.
const PUFF_SPACING: f32 = 1.4;
/// One chip per this many metres of the footprint's edge.
const CHIP_SPACING: f32 = 2.5;
const DUST: Rgb = [0.47, 0.38, 0.27];
const SHOCKWAVE: Rgb = [0.96, 0.89, 0.72];
const WOOD: Rgb = [0.55, 0.37, 0.2];
const STONE: Rgb = [0.63, 0.61, 0.57];

/// How a placement slam looks, fixed when it starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlamSpec {
    /// Half extents of the foundation's footprint (the slam is turned with it).
    pub half: Vec2,
    /// Scale of the shockwave's run, the dust and the chips: at least 1,
    /// growing when zoomed out ([`marker_size`]).
    pub size: f32,
    /// Thinnest line drawn (`parts::min_line_width`).
    pub line: f32,
    /// Varies the dust and chips from one slam to the next.
    pub seed: u32,
}

impl SlamSpec {
    fn perimeter(&self) -> f32 {
        4.0 * (self.half.x + self.half.y)
    }

    /// Number of dust puffs: by the footprint's edge, 8 to 20.
    pub fn puffs(&self) -> usize {
        ((self.perimeter() / PUFF_SPACING).round() as usize).clamp(8, 20)
    }

    /// Number of chips: by the footprint's edge, 4 to 10.
    pub fn chips(&self) -> usize {
        ((self.perimeter() / CHIP_SPACING).round() as usize).clamp(4, 10)
    }
}

/// A number in 0..1 from `seed` and `n` (client-side variety only).
fn rand(seed: u32, n: u32) -> f32 {
    (hash32(seed ^ n.wrapping_mul(0x9e37_79b9)) >> 8) as f32 / (1u32 << 24) as f32
}

/// The point `s` metres round the edge of a footprint with half extents
/// `half` (local x, z; starting at the front right corner), and the
/// outward direction there.
pub fn edge_point(half: Vec2, s: f32) -> (Vec2, Vec2) {
    let (w, d) = (2.0 * half.x, 2.0 * half.y);
    let s = s.rem_euclid(2.0 * (w + d));
    if s < w {
        (Vec2::new(half.x - s, half.y), Vec2::Y)
    } else if s < w + d {
        (Vec2::new(-half.x, half.y - (s - w)), Vec2::NEG_X)
    } else if s < 2.0 * w + d {
        (Vec2::new(-half.x + (s - w - d), -half.y), Vec2::NEG_Y)
    } else {
        (Vec2::new(half.x, -half.y + (s - 2.0 * w - d)), Vec2::X)
    }
}

/// The shockwave `age` seconds into a slam: a frame the shape of the
/// footprint running outward from its edge, thinning, and its opacity.
pub fn slam_wave(spec: &SlamSpec, age: f32) -> ([Transform; 4], f32) {
    let w = (age / WAVE_SECS).clamp(0.0, 1.0);
    let run = (1.5 + 0.15 * spec.half.max_element()) * spec.size;
    let width = (0.45 - 0.35 * w).max(spec.line);
    let boxes = frame_boxes(spec.half, 0.05 + run * ease_out(w), width, 0.05);
    (boxes.map(|(c, s)| Transform::from_translation(c).with_scale(s)), 0.85 * (1.0 - w).powf(1.3))
}

/// Dust puff `i` of `n` `age` seconds into a slam: it starts on the
/// footprint's edge and rolls outward, growing and rising.
pub fn slam_puff(spec: &SlamSpec, i: usize, n: usize, age: f32) -> Transform {
    let w = ease_out((age / SLAM_SECS).clamp(0.0, 1.0));
    let jitter = rand(spec.seed, 100 + i as u32);
    let (at, out) = edge_point(spec.half, (i as f32 + 0.25 + 0.5 * jitter) / n.max(1) as f32 * spec.perimeter());
    let s = spec.size;
    let dist = (0.15 + (1.2 + 0.6 * jitter) * w) * s;
    let size = (0.25 + 0.5 * w) * s;
    let p = at + out * dist;
    let roll = Quat::from_axis_angle(Vec3::new(-out.y, 0.0, out.x), dist / size);
    Transform::from_xyz(p.x, size * 0.5 + 0.7 * w * s, p.y).with_rotation(roll * Quat::from_rotation_y(jitter * TAU)).with_scale(Vec3::splat(size))
}

/// The dust's opacity `age` seconds into a slam.
pub fn dust_alpha(age: f32) -> f32 {
    let f = (age / SLAM_SECS).clamp(0.0, 1.0);
    0.85 * (age / 0.05).clamp(0.0, 1.0) * (1.0 - f).powf(1.3)
}

/// Whether chip `i` is a wood splinter (else a stone chip).
pub fn chip_is_wood(spec: &SlamSpec, i: usize) -> bool {
    rand(spec.seed, 2000 + i as u32) < 0.6
}

/// Chip `i` `age` seconds into a slam: thrown up and out from near the
/// footprint's edge, spinning, it falls back and lies where it lands.
pub fn slam_chip(spec: &SlamSpec, i: usize, age: f32) -> Transform {
    let r = |k: u32| rand(spec.seed, 1000 + 16 * i as u32 + k);
    let (at, out) = edge_point(spec.half * 0.8, r(0) * spec.perimeter());
    let dir = Vec2::from_angle((r(1) - 0.5) * 1.2).rotate(out);
    let speed = (1.4 + 1.8 * r(2)) * spec.size.sqrt();
    let vel = Vec3::new(dir.x * speed, 4.5 + 2.0 * r(3), dir.y * speed);
    let axis = Vec3::new(r(4) - 0.5, r(5) - 0.5, r(6) - 0.5).normalize_or(Vec3::X);
    let size = (0.28 + 0.17 * r(7)) * spec.size;
    let shape = if chip_is_wood(spec, i) { Vec3::new(0.5, 0.5, 1.6) } else { Vec3::new(1.0, 0.8, 1.0) } * size;
    let (from, rest, a) = (Vec3::new(at.x, 0.25, at.y), shape.y * 0.5, 0.5 * SLAM_GRAVITY);
    // It lands when from.y + vy t - a t^2 = rest.
    let landed = (vel.y + (vel.y * vel.y + 4.0 * a * (from.y - rest).max(0.0)).sqrt()) / (2.0 * a);
    let t = age.clamp(0.0, landed);
    let p = from + vel * t - Vec3::Y * a * t * t;
    Transform::from_translation(p).with_rotation(Quat::from_axis_angle(axis, (6.0 + 8.0 * r(8)) * t)).with_scale(shape)
}

/// The chips' opacity `age` seconds into a slam: solid, fading at the end.
pub fn chip_alpha(age: f32) -> f32 {
    1.0 - smoothstep(0.65 * SLAM_SECS, SLAM_SECS, age)
}

/// A placement slam playing (a top-level scene entity at the foundation).
#[derive(Component, Debug)]
pub struct Slam {
    spec: SlamSpec,
    age: f32,
    wave: [Entity; 4],
    dust: Vec<Entity>,
    chips: Vec<Entity>,
    /// Materials in use: shockwave, dust, wood chips, stone chips.
    keys: [Option<MatKey>; 4],
}

/// The looks of a slam's layers at `age`: shockwave, dust, wood, stone.
fn slam_looks(spec: &SlamSpec, age: f32) -> [Look; 4] {
    let chips = chip_alpha(age);
    [
        Look::unlit(SHOCKWAVE, slam_wave(spec, age).1),
        Look { color: DUST, alpha: dust_alpha(age), shade: Shade::Lit },
        Look { color: WOOD, alpha: chips, shade: Shade::Lit },
        Look { color: STONE, alpha: chips, shade: Shade::Lit },
    ]
}

/// A slam for each foundation of the local player that appeared since the
/// last frame (never inside the sim tick: an `Added` filter in `Update`).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn spawn_slams(
    mut commands: Commands,
    mut painter: Painter,
    ground: Ground,
    local: Res<LocalPlayer>,
    rig: Res<CameraRig>,
    founded: Query<(&Position, &Facing, &Footprint, &Owner), (With<Building>, Added<UnderConstruction>)>,
    playing: Query<(Entity, &Slam)>,
) {
    let Some(me) = local.0 else { return };
    let fresh: Vec<_> = founded.iter().filter(|(.., owner)| owner.0 == me).take(MAX_SLAMS).collect();
    if fresh.is_empty() {
        return;
    }
    let over = (playing.iter().count() + fresh.len()).saturating_sub(MAX_SLAMS);
    if over > 0 {
        let mut oldest: Vec<(f32, Entity)> = playing.iter().map(|(e, s)| (s.age, e)).collect();
        oldest.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, e) in oldest.into_iter().take(over) {
            commands.entity(e).try_despawn();
        }
    }
    let cube = painter.cube();
    for (pos, facing, fp, _) in fresh {
        let p = pos.0;
        let spec = SlamSpec {
            half: fp.half,
            size: marker_size(rig.distance).max(1.0),
            line: min_line_width(rig.distance),
            seed: hash32(p.x.to_bits() ^ p.y.to_bits().rotate_left(16)),
        };
        let root = commands
            .spawn((Transform::from_xyz(p.x, ground.surface(p), p.y).with_rotation(Quat::from_rotation_y(facing.0)), Visibility::Inherited, RenderScene))
            .id();
        let looks = slam_looks(&spec, 0.0);
        let keys = looks.map(first_key);
        let (wave, _) = slam_wave(&spec, 0.0);
        let wave = wave.map(|tf| spawn_part(&mut commands, &mut painter, root, &cube, looks[0], tf));
        let n = spec.puffs();
        let dust = (0..n).map(|i| spawn_part(&mut commands, &mut painter, root, &cube, looks[1], slam_puff(&spec, i, n, 0.0))).collect();
        let chips = (0..spec.chips())
            .map(|i| {
                let look = if chip_is_wood(&spec, i) { looks[2] } else { looks[3] };
                spawn_part(&mut commands, &mut painter, root, &cube, look, slam_chip(&spec, i, 0.0))
            })
            .collect();
        commands.entity(root).insert(Slam { spec, age: 0.0, wave, dust, chips, keys });
    }
}

/// Play the placement slams in real time and despawn each when it is over.
pub(crate) fn animate_slams(mut commands: Commands, mut painter: Painter, time: Res<Time<Real>>, mut slams: Query<(Entity, &mut Slam)>, mut parts: PartQuery) {
    let dt = time.delta_secs();
    for (e, mut slam) in &mut slams {
        slam.age += dt;
        if slam.age >= SLAM_SECS {
            commands.entity(e).try_despawn();
            continue;
        }
        let s = &mut *slam;
        let (spec, age) = (s.spec, s.age);
        let looks = slam_looks(&spec, age);
        let [kw, kd, kwood, kstone] = &mut s.keys;
        let (wave, _) = slam_wave(&spec, age);
        draw(&mut painter, &mut parts, looks[0], kw, s.wave.into_iter().zip(wave));
        let n = s.dust.len();
        draw(&mut painter, &mut parts, looks[1], kd, s.dust.iter().enumerate().map(|(i, &id)| (id, slam_puff(&spec, i, n, age))));
        let chips = &s.chips;
        let of_kind = |wood: bool| chips.iter().enumerate().filter(move |(i, _)| chip_is_wood(&spec, *i) == wood).map(move |(i, &id)| (id, slam_chip(&spec, i, age)));
        draw(&mut painter, &mut parts, looks[2], kwood, of_kind(true));
        draw(&mut painter, &mut parts, looks[3], kstone, of_kind(false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(queued: bool) -> MarkerSpec {
        MarkerSpec { hug: Hug::Point, arrows: true, queued, size: if queued { QUEUED_SIZE } else { 1.0 }, line: 0.1 }
    }

    /// Where an arrow's tip is (the mesh tip is +0.5 along its +Z).
    fn tip(tf: &Transform) -> Vec3 {
        tf.transform_point(Vec3::Z * 0.5)
    }

    #[test]
    fn arrows_swoop_down_and_in_speeding_up() {
        for side in SIDES {
            let steps: Vec<Vec3> = (0..=10).map(|i| tip(&arrow_transform(side, 0.3, i as f32 / 10.0, 1.0))).collect();
            for pair in steps.windows(2) {
                assert!(pair[1].y < pair[0].y, "falls: {pair:?}");
                assert!(pair[1].xz().length() < pair[0].xz().length(), "closes in: {pair:?}");
            }
            let first = (steps[1] - steps[0]).length();
            let last = (steps[10] - steps[9]).length();
            assert!(last > first * 3.0, "eases in: {first} then {last}");
            assert!(steps[0].y > 1.5, "starts high above the spot: {}", steps[0].y);
            assert!((steps[10].xz() - side.xz() * 0.3).length() < 1e-4 && (steps[10].y - TIP_Y).abs() < 1e-4, "lands at its reach: {}", steps[10]);
            let forward = arrow_transform(side, 0.3, 0.5, 1.0).rotation * Vec3::Z;
            assert!(forward.y < -0.7 && forward.dot(-side) > 0.4, "points down and in: {forward}");
        }
        let big = arrow_transform(Vec3::X, 0.3, 0.0, 2.0);
        assert!((big.scale.x - 2.0 * ARROW_LENGTH).abs() < 1e-5 && tip(&big).y > tip(&arrow_transform(Vec3::X, 0.3, 0.0, 1.0)).y);
    }

    #[test]
    fn a_point_marker_dives_lands_and_fades() {
        let spec = point(false);
        let tl = Timeline::of(false);
        let at = |t: f32| marker_pose(&spec, t);
        assert_eq!(at(0.0).arrows.unwrap().alpha, 0.0, "the arrows fade in");
        assert!(at(0.0).frame.is_none() && at(0.0).dot.is_none(), "a point has no frame, and no dot unless queued");
        let diving = at(tl.land * 0.6);
        assert_eq!(diving.arrows.unwrap().alpha, 1.0);
        assert_eq!((diving.ring.unwrap().alpha, diving.flash.unwrap().alpha), (0.0, 0.0), "nothing on the ground before they land");
        // All four meet at the centre.
        let landed = at(tl.land + 0.001).arrows.unwrap();
        for tf in landed.transforms {
            assert!(tip(&tf).xz().length() <= POINT_REACH + 1e-3, "{}", tip(&tf));
        }
        // Then the ring contracts as it fades, and the flash pops and goes.
        let ring = |t: f32| at(t).ring.unwrap();
        let (r1, r2) = (ring(tl.land + 0.05), ring(tl.land + 0.3));
        assert!(r1.alpha > r2.alpha && r2.alpha > 0.0);
        assert!(r1.transforms[0].scale.x > r2.transforms[0].scale.x, "contracts");
        assert!(at(tl.land + 0.02).flash.unwrap().alpha > 0.5);
        assert_eq!(at(0.98).flash.unwrap().alpha, 0.0);
        assert!(at(tl.land + 0.25).arrows.unwrap().alpha == 0.0, "the arrows vanish once landed");
        let end = at(1.0);
        assert!(end.ring.unwrap().alpha < 0.01 && end.flash.unwrap().alpha == 0.0);
        // Rings and flashes grow with the marker size.
        let big = marker_pose(&MarkerSpec { size: 2.0, ..spec }, tl.land + 0.05).ring.unwrap();
        assert!((big.transforms[0].scale.x - 2.0 * r1.transforms[0].scale.x).abs() < 1e-4);
    }

    #[test]
    fn queued_markers_are_smaller_quicker_and_leave_a_dot() {
        let (normal, queued) = (point(false), point(true));
        let (tn, tq) = (Timeline::of(false), Timeline::of(true));
        assert!(tq.land * ClickMark::QUEUED_SECS < tn.land * ClickMark::SECS, "they land sooner");
        let scale = |spec: &MarkerSpec| marker_pose(spec, 0.1).arrows.unwrap().transforms[0].scale.x;
        assert!(scale(&queued) < scale(&normal));
        // The ring is gone long before the dot.
        let late = marker_pose(&queued, 0.7);
        assert_eq!(late.ring.unwrap().alpha, 0.0);
        assert!(late.dot.unwrap().alpha > 0.5);
        assert_eq!(marker_pose(&queued, 0.1).dot.unwrap().alpha, 0.0, "no dot before the arrows land");
        assert!(marker_pose(&queued, 1.0).dot.unwrap().alpha < 0.01);
    }

    /// Times over `t` = 0..1 at which `f` peaks.
    fn peaks(f: impl Fn(f32) -> f32) -> usize {
        let v: Vec<f32> = (0..=200).map(|i| f(i as f32 / 200.0)).collect();
        v.windows(3).filter(|w| w[1] > w[0] && w[1] >= w[2]).count()
    }

    #[test]
    fn hugs_pulse_twice_around_the_footprint() {
        let half = Vec2::new(2.0, 1.5);
        let frame = MarkerSpec { hug: Hug::Frame(half), arrows: true, queued: false, size: 1.0, line: 0.05 };
        let outer = |t: f32| marker_pose(&frame, t).frame.unwrap().transforms[3].translation.x;
        assert_eq!(peaks(outer), 2, "swells twice");
        assert!(outer(0.0) > half.x, "outside the footprint");
        assert!(marker_pose(&frame, 1.0).frame.unwrap().alpha < 0.01, "and fades");
        // The arrows land on the frame's outer edge (between two pulses),
        // on each side's middle.
        let pose = marker_pose(&frame, Timeline::of(false).land);
        let edge = pose.frame.unwrap().transforms[3].translation.x + pose.frame.unwrap().transforms[3].scale.x * 0.5;
        let arrow_x = tip(&pose.arrows.unwrap().transforms[0]).x;
        assert!((arrow_x - edge).abs() < 0.02, "{arrow_x} vs {edge}");
        assert!((tip(&pose.arrows.unwrap().transforms[2]).z - (half.y + HUG_GAP + frame_width(half, 0.05))).abs() < 1e-4);
        // A unit's ring pulses too, and keeps a readable band zoomed out.
        let ring = MarkerSpec { hug: Hug::Ring(0.6), ..frame };
        assert_eq!(peaks(|t| marker_pose(&ring, t).ring.unwrap().transforms[0].scale.x), 2);
        let far = MarkerSpec { line: 0.24, ..ring };
        assert!(marker_pose(&far, 0.0).ring.unwrap().transforms[0].scale.x * (1.0 - RING_INNER) >= 0.24 - 1e-5);
        // A placement only flashes its footprint.
        let place = MarkerSpec { arrows: false, ..frame };
        assert!(marker_pose(&place, 0.3).arrows.is_none() && marker_pose(&place, 0.3).frame.is_some());
    }

    #[test]
    fn marker_kinds_hug_what_they_target() {
        let data = af_core::testing::test_data();
        let house = data.building_id("house").unwrap();
        let fp = Footprint { half: Vec2::new(0.4, 0.3), height: 1.0 };
        let mark = |kind| ClickMark::new(kind, None, false);
        let (hug, arrows, yaw) = marker_hug(&data, &mark(OrderMark::Place { kind: house, facing: 0.5 }), None, 2.0);
        assert_eq!((hug, arrows, yaw), (Hug::Frame(Footprint::from_size(data.building(house).size).half), false, 0.5));
        assert_eq!(marker_hug(&data, &mark(OrderMark::Attack), Some((&fp, None, true)), 2.0), (Hug::Ring(0.5 + HUG_GAP), true, 2.0));
        assert_eq!(marker_hug(&data, &mark(OrderMark::Gather), Some((&fp, Some(&Facing(1.0)), false)), 2.0), (Hug::Frame(fp.half), true, 1.0));
        assert_eq!(marker_hug(&data, &mark(OrderMark::Move), None, 2.0), (Hug::Point, true, 2.0));
    }

    #[test]
    fn markers_stay_readable_when_zoomed_out() {
        assert!(marker_size(150.0) > marker_size(66.0) && marker_size(66.0) > marker_size(25.0));
        assert_eq!(marker_size(1000.0), 3.5);
        assert_eq!(marker_size(1.0), 0.8);
    }

    #[test]
    fn edge_points_walk_round_the_footprint() {
        let half = Vec2::new(2.0, 1.0);
        for i in 0..48 {
            let (p, out) = edge_point(half, i as f32 * 0.25);
            let on_edge = (p.x.abs() - half.x).abs() < 1e-4 || (p.y.abs() - half.y).abs() < 1e-4;
            assert!(on_edge && p.x.abs() <= half.x + 1e-4 && p.y.abs() <= half.y + 1e-4, "{p}");
            assert!(p.dot(out) > 0.0 && (out.length() - 1.0).abs() < 1e-5, "outward at {p}: {out}");
        }
    }

    #[test]
    fn the_slam_runs_out_from_the_footprint_and_settles() {
        let spec = SlamSpec { half: Vec2::new(2.0, 1.5), size: 1.0, line: 0.05, seed: 7 };
        // The shockwave runs outward and fades.
        let reach = |age: f32| slam_wave(&spec, age).0[3].translation.x;
        assert!(reach(0.0) < spec.half.x + 0.5 && reach(0.3) > reach(0.1) && reach(WAVE_SECS) > spec.half.x + 1.5);
        assert!(slam_wave(&spec, 0.0).1 > 0.8 && slam_wave(&spec, WAVE_SECS).1 < 1e-4);
        // Dust rolls outward and rises, then fades.
        let n = spec.puffs();
        for i in 0..n {
            let (a, b) = (slam_puff(&spec, i, n, 0.05), slam_puff(&spec, i, n, 0.6));
            let out = |tf: &Transform| (tf.translation.x.abs() - spec.half.x).max(tf.translation.z.abs() - spec.half.y);
            assert!(out(&b) > out(&a) + 0.5 && b.translation.y > a.translation.y && b.scale.x > a.scale.x, "puff {i}: {a:?} -> {b:?}");
        }
        assert!(dust_alpha(0.1) > 0.4 && dust_alpha(SLAM_SECS) < 1e-4);
        // Chips fly up, fall back, and lie still where they land.
        for i in 0..spec.chips() {
            let y = |age: f32| slam_chip(&spec, i, age).translation.y;
            assert!(y(0.2) > y(0.0) + 0.3, "chip {i} is thrown up");
            let rest = slam_chip(&spec, i, SLAM_SECS);
            assert!((rest.translation.y - rest.scale.y * 0.5).abs() < 1e-3, "chip {i} lies on the ground: {rest:?}");
            assert_eq!(slam_chip(&spec, i, SLAM_SECS), slam_chip(&spec, i, SLAM_SECS + 1.0), "and stays");
        }
        assert_eq!(chip_alpha(0.3), 1.0);
        assert!(chip_alpha(SLAM_SECS) < 1e-4);
        // Bigger footprints throw more dust and chips, within bounds.
        let small = SlamSpec { half: Vec2::splat(0.5), ..spec };
        let big = SlamSpec { half: Vec2::splat(6.0), ..spec };
        assert_eq!((small.puffs(), small.chips()), (8, 4));
        assert!(spec.puffs() > small.puffs() && big.puffs() == 20 && big.chips() == 10);
    }
}
