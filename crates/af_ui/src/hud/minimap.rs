//! Minimap (bottom left): terrain, resource nodes, units and buildings in
//! player colours, fog, and the camera's view. Under fog it hides what the
//! world view (af_render) hides: enemy units unless some of their box is
//! in sight, buildings and nodes until scouted; remembered dead ones stay
//! drawn as their ghosts. Redrawn into a dynamic image four times a
//! second. Left click/drag moves the camera; right click sends the
//! selection there (or cancels a pending mode); a pending target command
//! (Move, Attack, Patrol, Rally) completes on the left-clicked point. Both
//! orders leave a click marker at their point in the world.
//!
//! The terrain is drawn once per map at twice the shown size and filtered
//! down. Each texel weighs every terrain cell under it, water, fords and
//! barriers above open ground ([`terrain_weight`]), so a river stays one
//! unbroken line with its fords on it even where a texel spans metres of
//! ground, and a pass or a ramp gap stays open.

use af_core::ext::fog::{team_sees, FogGhosts, SeenBy};
use af_core::ext::input::{ClickMark, OrderMark, RtsCamera};
use af_core::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::RelativeCursorPosition;

use super::overlays::GameOverState;
use super::{HudSet, SelectionInfo};
use crate::theme;

/// Side of the minimap on screen, in logical UI px.
pub const MINIMAP_PX: u32 = 200;
/// Texels per logical px: the image is drawn at twice the size it is shown
/// and filtered down, so coasts and rivers keep their shape (and a
/// high-DPI screen gets every texel).
const SUPERSAMPLE: u32 = 2;
/// Side of the minimap image in texels.
pub const MINIMAP_RES: u32 = MINIMAP_PX * SUPERSAMPLE;
/// Seconds of real time between redraws.
const REDRAW: f32 = 0.25;
const OFF_MAP: [u8; 4] = [8, 9, 12, 255];
const UNEXPLORED: [u8; 4] = [10, 10, 12, 255];
const VIEW_BOX: [u8; 4] = [255, 255, 255, 255];
/// Brightness kept where the viewer has explored but does not see now.
const FOGGED: f32 = 0.55;

/// Maps world ground points (x, z) to minimap pixels: the longer map side
/// fills the image, x runs right and z runs down (row 0 is z = 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinimapGeometry {
    pub world: Vec2,
    pub px: u32,
}

impl MinimapGeometry {
    /// Geometry for a map of `world` metres drawn into `px` square pixels.
    pub fn new(world: Vec2, px: u32) -> MinimapGeometry {
        MinimapGeometry { world: world.max(Vec2::ONE), px }
    }

    /// Pixels per metre.
    pub fn scale(&self) -> f32 {
        self.px as f32 / self.world.max_element()
    }

    /// Top-left of the map inside the image (non-square maps are centred).
    fn offset(&self) -> Vec2 {
        (Vec2::splat(self.px as f32) - self.world * self.scale()) * 0.5
    }

    /// Continuous pixel coordinates of a world point.
    pub fn to_pixel(&self, p: Vec2) -> Vec2 {
        self.offset() + p * self.scale()
    }

    /// The pixel containing a world point, if it is inside the image.
    pub fn pixel(&self, p: Vec2) -> Option<UVec2> {
        let q = self.to_pixel(p).floor();
        let n = self.px as f32;
        (q.x >= 0.0 && q.y >= 0.0 && q.x < n && q.y < n).then(|| q.as_uvec2())
    }

    /// World point at continuous pixel coordinates.
    pub fn to_world(&self, pixel: Vec2) -> Vec2 {
        (pixel - self.offset()) / self.scale()
    }

    /// World point under a `RelativeCursorPosition::normalized` value
    /// (centre origin, +-0.5 at the edges, y down), clamped onto the map.
    pub fn from_normalized(&self, n: Vec2) -> Vec2 {
        self.to_world((n + 0.5) * self.px as f32).clamp(Vec2::ZERO, self.world)
    }
}

/// The minimap's image and drawing state.
#[derive(Resource)]
pub(crate) struct Minimap {
    pub image: Handle<Image>,
    /// Terrain layer (RGBA) of the map it was drawn for.
    terrain: Vec<u8>,
    terrain_for: Option<(Vec2, u64)>,
    /// Scratch frame composed before upload.
    frame: Vec<u8>,
    since_redraw: f32,
    /// A left drag that started on the minimap is steering the camera.
    dragging: bool,
}

#[derive(Component)]
struct MinimapView;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, (minimap_input, draw_minimap).chain().in_set(HudSet::Panels));
}

/// Creates the minimap image and state; returns the image for the view.
pub(super) fn create(commands: &mut Commands, images: &mut Assets<Image>) -> Handle<Image> {
    let mut image = Image::new_fill(
        Extent3d { width: MINIMAP_RES, height: MINIMAP_RES, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &OFF_MAP,
        TextureFormat::Rgba8UnormSrgb,
        // Keep the CPU copy: it is rewritten on every redraw.
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    // Filtered: the image is shown at half its size (or less).
    image.sampler = ImageSampler::linear();
    let handle = images.add(image);
    let len = (MINIMAP_RES * MINIMAP_RES * 4) as usize;
    commands.insert_resource(Minimap { image: handle.clone(), terrain: vec![0; len], terrain_for: None, frame: vec![0; len], since_redraw: REDRAW, dragging: false });
    handle
}

pub(super) fn spawn(bottom: &mut ChildSpawnerCommands, image: Handle<Image>) {
    // The panel's usual padding keeps the map off its frame's metal band.
    bottom.spawn((theme::panel_node(Node { flex_shrink: 0.0, ..default() }), theme::panel_colors(), crate::skin::SkinPanel::Panel)).with_child((
        MinimapView,
        ImageNode::new(image),
        Node { width: px(MINIMAP_PX as f32), height: px(MINIMAP_PX as f32), ..default() },
        RelativeCursorPosition::default(),
    ));
}

/// The command a pending target mode becomes when completed on the minimap.
fn command_at(t: TargetAction, point: Vec2) -> Option<CommandKind> {
    Some(match t {
        TargetAction::Move => CommandKind::Move(point),
        TargetAction::AttackMove | TargetAction::Attack => CommandKind::AttackMove(point),
        TargetAction::Patrol => CommandKind::Patrol(point),
        TargetAction::Rally => CommandKind::SetRally { point, target: None },
        // These need an entity under the cursor, which the minimap cannot pick.
        TargetAction::Gather | TargetAction::Repair => return None,
    })
}

/// Whether the mouse is on the minimap itself. `RelativeCursorPosition`
/// is pure geometry and ignores UI drawn above the minimap (the game-over
/// screen); the picking hover map leaves out whatever such UI blocks.
fn over_minimap(hover: Option<&HoverMap>, view: Entity) -> bool {
    hover.and_then(|h| h.get(&PointerId::Mouse)).is_some_and(|hits| hits.contains_key(&view))
}

#[allow(clippy::too_many_arguments)]
fn minimap_input(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Query<(Entity, &RelativeCursorPosition), With<MinimapView>>,
    hover: Option<Res<HoverMap>>,
    game_over: Res<GameOverState>,
    map: Option<Res<MapInfo>>,
    sel: Res<SelectionInfo>,
    local: Res<LocalPlayer>,
    mut minimap: ResMut<Minimap>,
    mut rig: ResMut<CameraRig>,
    mut mode: ResMut<InputMode>,
    mut inbox: ResMut<CommandInbox>,
    mut commands: Commands,
) {
    // The game-over screen covers the minimap and also ends a drag in progress.
    let active = view.single().ok().zip(map).filter(|_| !game_over.blocking());
    let Some(((view, rel), map)) = active else {
        minimap.dragging = false;
        return;
    };
    let geo = MinimapGeometry::new(map.size, MINIMAP_PX);
    let point = rel.normalized.map(|n| geo.from_normalized(n));
    let over = over_minimap(hover.as_deref(), view);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if over && mouse.just_pressed(MouseButton::Left) {
        let pending = if let InputMode::Target(t) = *mode { Some(t) } else { None };
        match (pending, local.0, point) {
            (Some(t), Some(me), Some(p)) => {
                let entities = if t == TargetAction::Rally { sel.buildings.clone() } else { sel.own_units.clone() };
                if let Some(kind) = command_at(t, p) {
                    inbox.push(PlayerCommand::new(me, entities, kind).queued(shift));
                    commands.spawn(ClickMark::new(OrderMark::of_target(t), None, shift).ping(p));
                }
                if !shift {
                    *mode = InputMode::Normal;
                }
            }
            _ => {
                // Jump on the press itself: a tap may be released in the same frame.
                minimap.dragging = true;
                if let Some(p) = point {
                    rig.focus = p;
                }
            }
        }
    }
    if minimap.dragging
        && mouse.pressed(MouseButton::Left)
        && let Some(p) = point
    {
        rig.focus = p;
    }
    // Only after the press is handled: a press and release in one frame must
    // not leave a drag armed for the next press anywhere on screen.
    if !mouse.pressed(MouseButton::Left) {
        minimap.dragging = false;
    }
    if over && mouse.just_pressed(MouseButton::Right) {
        // As in the world, a right click first cancels a pending mode.
        if *mode != InputMode::Normal {
            *mode = InputMode::Normal;
        } else if let (Some(me), Some(p)) = (local.0, point) {
            let entities: Vec<Entity> = sel.own_units.iter().chain(&sel.buildings).copied().collect();
            if !entities.is_empty() {
                inbox.push(PlayerCommand::new(me, entities, CommandKind::Smart { point: p, target: None }).queued(shift));
                let mark = if sel.own_units.is_empty() { OrderMark::Rally } else { OrderMark::Move };
                commands.spawn(ClickMark::new(mark, None, shift).ping(p));
            }
        }
    }
}

/// An RGBA pixel buffer of `MINIMAP_RES` square.
struct Canvas<'a>(&'a mut [u8]);

impl Canvas<'_> {
    fn put(&mut self, x: i32, y: i32, c: [u8; 4]) {
        let n = MINIMAP_RES as i32;
        if (0..n).contains(&x) && (0..n).contains(&y) {
            let i = ((y * n + x) * 4) as usize;
            self.0[i..i + 4].copy_from_slice(&c);
        }
    }

    /// Filled rectangle covering pixels `min..=max` (continuous coordinates).
    fn rect(&mut self, min: Vec2, max: Vec2, c: [u8; 4]) {
        let (x0, y0) = (min.x.floor() as i32, min.y.floor() as i32);
        let (x1, y1) = (max.x.floor() as i32, max.y.floor() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.put(x, y, c);
            }
        }
    }

    /// A line one logical px wide (`SUPERSAMPLE` texels).
    fn line(&mut self, a: Vec2, b: Vec2, c: [u8; 4]) {
        let d = b - a;
        // Far-off view corners would make huge loops; the image is small.
        let steps = d.abs().max_element().ceil().clamp(1.0, 4.0 * MINIMAP_RES as f32) as i32;
        let w = SUPERSAMPLE as i32;
        for i in 0..=steps {
            let p = a + d * (i as f32 / steps as f32);
            let (x, y) = (p.x.floor() as i32 - w / 2, p.y.floor() as i32 - w / 2);
            for dy in 0..w {
                for dx in 0..w {
                    self.put(x + dx, y + dy, c);
                }
            }
        }
    }
}

fn rgba(c: [f32; 3]) -> [u8; 4] {
    [(c[0].clamp(0.0, 1.0) * 255.0) as u8, (c[1].clamp(0.0, 1.0) * 255.0) as u8, (c[2].clamp(0.0, 1.0) * 255.0) as u8, 255]
}

/// How strongly a terrain claims a texel that covers some of it. Rivers,
/// shores, fords and barriers are thin lines on a big map, and a texel can
/// span several metres: they outweigh open ground, so a river or a range
/// never breaks up (water over a quarter of a grass texel takes it), and a
/// ford, the way across, outweighs the water round it. Mountains and
/// highlands weigh less than water, so a pass or a ramp gap a couple of
/// texels wide still shows open.
fn terrain_weight(id: &str) -> f32 {
    match id {
        "ford" => 8.0,
        "ocean" | "deep_water" | "shallow_water" => 3.0,
        "mountain" | "cliff" => 1.5,
        _ => 1.0,
    }
}

/// The terrain layer: each texel takes the terrain that weighs most over
/// the ground it covers (cells weighed by how much of them it covers and
/// by [`terrain_weight`]; ties go to the heavier terrain, then the lower
/// id), in its `TerrainDef::color`.
fn draw_terrain(data: &GameData, nav: &NavGrid, geo: &MinimapGeometry, out: &mut [u8]) {
    let colors: Vec<[u8; 4]> = data.terrain.iter().map(|t| rgba(t.color)).collect();
    let mut weights = [1.0f32; 256];
    for (w, t) in weights.iter_mut().zip(&data.terrain) {
        *w = terrain_weight(&t.id);
    }
    let mut scores = [0.0f32; 256];
    let mut touched: Vec<u8> = Vec::new();
    let (origin, cell) = (nav.origin(), nav.cell_size().max(1e-3));
    for (i, px) in out.chunks_exact_mut(4).enumerate() {
        let (x, y) = (i as u32 % geo.px, i as u32 / geo.px);
        // The texel's ground, clipped to the map, in cells.
        let lo = geo.to_world(Vec2::new(x as f32, y as f32)).max(Vec2::ZERO);
        let hi = geo.to_world(Vec2::new(x as f32 + 1.0, y as f32 + 1.0)).min(geo.world);
        if lo.cmpge(hi).any() {
            px.copy_from_slice(&OFF_MAP);
            continue;
        }
        let (lo, hi) = ((lo - origin) / cell, (hi - origin) / cell);
        for cy in lo.y.floor() as i32..hi.y.ceil() as i32 {
            let span_y = hi.y.min(cy as f32 + 1.0) - lo.y.max(cy as f32);
            for cx in lo.x.floor() as i32..hi.x.ceil() as i32 {
                let span_x = hi.x.min(cx as f32 + 1.0) - lo.x.max(cx as f32);
                let t = nav.terrain_at(IVec2::new(cx, cy)) as usize;
                if !touched.contains(&(t as u8)) {
                    touched.push(t as u8);
                }
                scores[t] += span_x.max(0.0) * span_y.max(0.0) * weights[t];
            }
        }
        let best = touched.iter().map(|t| *t as usize).max_by(|&a, &b| scores[a].total_cmp(&scores[b]).then(weights[a].total_cmp(&weights[b])).then(b.cmp(&a)));
        px.copy_from_slice(&best.and_then(|t| colors.get(t).copied()).unwrap_or(OFF_MAP));
        for t in touched.drain(..) {
            scores[t as usize] = 0.0;
        }
    }
}

fn apply_fog(fog: &FogOfWar, team: u8, geo: &MinimapGeometry, out: &mut [u8]) {
    for (i, px) in out.chunks_exact_mut(4).enumerate() {
        let w = geo.to_world(Vec2::new((i as u32 % geo.px) as f32 + 0.5, (i as u32 / geo.px) as f32 + 0.5));
        if !fog.explored(team, w) {
            px.copy_from_slice(&UNEXPLORED);
        } else if !fog.visible(team, w) {
            for c in &mut px[..3] {
                *c = (*c as f32 * FOGGED) as u8;
            }
        }
    }
}

/// Ground points under the four corners of the RTS camera's view.
fn view_corners(camera: &Camera, transform: &GlobalTransform) -> Option<[Vec2; 4]> {
    let size = camera.logical_viewport_size()?;
    let mut out = [Vec2::ZERO; 4];
    for (o, corner) in out.iter_mut().zip([Vec2::ZERO, Vec2::new(size.x, 0.0), size, Vec2::new(0.0, size.y)]) {
        let ray = camera.viewport_to_world(transform, corner).ok()?;
        *o = match ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)) {
            Some(d) => ray.get_point(d).xz(),
            // Above the horizon: far along the ground in the ray's direction.
            None => ray.origin.xz() + ray.direction.xz().normalize_or_zero() * 1000.0,
        };
    }
    Some(out)
}

/// Draws one entity's mark: units as 2x2 dots, buildings at their size but
/// at least 2x2 logical px so they read as boxes, nodes at their size.
fn draw_mark(canvas: &mut Canvas, geo: &MinimapGeometry, kind: SpatialKind, obb: &Obb, color: [u8; 4]) {
    let (min, max) = obb.aabb();
    let dot = Vec2::splat(SUPERSAMPLE as f32);
    let half = match kind {
        SpatialKind::Unit => dot,
        SpatialKind::Building => ((max - min) * 0.5 * geo.scale()).max(dot),
        SpatialKind::Node => (max - min) * 0.5 * geo.scale(),
    };
    let c = geo.to_pixel(obb.center);
    canvas.rect(c - half, c + half - Vec2::splat(0.01), color);
}

/// Whether `team` sees this entity on the minimap, by af_render's rules:
/// its own team's always; other units while some of their box is in sight;
/// buildings and nodes once the team has seen them ([`SeenBy`]).
fn shown_to(team: u8, e: &SpatialEntry, fog: &FogOfWar, seen: &Query<&SeenBy>) -> bool {
    e.team == team
        || match e.kind {
            SpatialKind::Unit => team_sees(fog, team, e.pos, e.obb.half.max_element()),
            SpatialKind::Building | SpatialKind::Node => seen.get(e.entity).is_ok_and(|s| s.seen_by(team)),
        }
}

#[allow(clippy::too_many_arguments)]
fn draw_minimap(
    time: Res<Time<Real>>,
    data: Res<Data>,
    map: Option<Res<MapInfo>>,
    nav: Option<Res<Nav>>,
    players: Option<Res<Players>>,
    local: Res<LocalPlayer>,
    fog: Res<FogOfWar>,
    ghosts: Option<Res<FogGhosts>>,
    spatial: Res<SpatialIndex>,
    seen: Query<&SeenBy>,
    rig: Res<CameraRig>,
    cameras: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    mut minimap: ResMut<Minimap>,
    mut images: ResMut<Assets<Image>>,
) {
    minimap.since_redraw += time.delta_secs();
    let (Some(map), Some(nav), Some(players)) = (map, nav, players) else { return };
    if minimap.since_redraw < REDRAW {
        return;
    }
    minimap.since_redraw = 0.0;
    let geo = MinimapGeometry::new(map.size, MINIMAP_RES);
    let mm = &mut *minimap;
    if mm.terrain_for != Some((map.size, map.seed)) {
        draw_terrain(&data, &nav, &geo, &mut mm.terrain);
        mm.terrain_for = Some((map.size, map.seed));
    }
    mm.frame.copy_from_slice(&mm.terrain);
    // The team whose fog hides things: none for spectators or without fog.
    let team = local.0.filter(|_| fog.enabled).map(|p| players.team(p));
    let owner_color = |owner: Option<PlayerId>| owner.and_then(|o| players.try_get(o)).map_or(OFF_MAP, |p| rgba(p.color));
    let mut canvas = Canvas(&mut mm.frame);
    if let Some(t) = team {
        apply_fog(&fog, t, &geo, canvas.0);
        // Dead buildings and nodes the team has not looked at again.
        for g in ghosts.iter().flat_map(|g| g.remembered_by(t)) {
            let (kind, color) = match g.kind {
                EntityKind::Node(n) => (SpatialKind::Node, rgba(data.node(n).color)),
                EntityKind::Building(_) => (SpatialKind::Building, owner_color(g.owner)),
                EntityKind::Unit(_) => continue,
            };
            draw_mark(&mut canvas, &geo, kind, &Obb::new(g.pos, g.footprint.half, g.facing), color);
        }
    }
    for pass in [SpatialKind::Node, SpatialKind::Building, SpatialKind::Unit] {
        for e in spatial.entries.iter().filter(|e| e.kind == pass) {
            if team.is_some_and(|t| !shown_to(t, e, &fog, &seen)) {
                continue;
            }
            let color = match e.owner {
                Some(_) => owner_color(e.owner),
                None => rgba(data.node(NodeTypeId(e.type_index)).color),
            };
            draw_mark(&mut canvas, &geo, pass, &e.obb, color);
        }
    }
    let corners = cameras.iter().find_map(|(cam, gt)| view_corners(cam, gt)).unwrap_or_else(|| {
        let h = Vec2::splat(rig.distance * 0.5);
        [rig.focus - h, rig.focus + Vec2::new(h.x, -h.y), rig.focus + h, rig.focus + Vec2::new(-h.x, h.y)]
    });
    for i in 0..4 {
        canvas.line(geo.to_pixel(corners[i]), geo.to_pixel(corners[(i + 1) % 4]), VIEW_BOX);
    }
    if let Some(mut image) = images.get_mut(&mm.image)
        && let Some(pixels) = image.data.as_mut()
    {
        pixels.copy_from_slice(&mm.frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::{new_nav, test_data};

    /// Paint terrain `id` on every cell within `half` metres of the
    /// polyline through `points`.
    fn paint(data: &GameData, nav: &mut NavGrid, id: &str, points: &[Vec2], half: f32) {
        let t = data.terrain_id(id).unwrap().0;
        for seg in points.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let (lo, hi) = ((a.min(b) - half).floor().as_ivec2(), (a.max(b) + half).ceil().as_ivec2());
            for y in lo.y..=hi.y {
                for x in lo.x..=hi.x {
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let k = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                    if p.distance(a + (b - a) * k) <= half && nav.in_bounds(IVec2::new(x, y)) {
                        nav.set_terrain(IVec2::new(x, y), t);
                    }
                }
            }
        }
    }

    /// The terrain layer's texels for an image `px` texels square, as RGBA,
    /// row by row; `nearest` draws each texel as the one cell under its
    /// centre instead (the old way, to show what the weighing fixes).
    fn terrain_texels(data: &GameData, nav: &NavGrid, px: u32, nearest: bool) -> (MinimapGeometry, Vec<[u8; 4]>) {
        let geo = MinimapGeometry::new(nav.world_size(), px);
        let mut out = vec![0; (px * px * 4) as usize];
        draw_terrain(data, nav, &geo, &mut out);
        let mut texels: Vec<[u8; 4]> = out.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        if nearest {
            for (i, t) in texels.iter_mut().enumerate() {
                let centre = geo.to_world(Vec2::new((i as u32 % px) as f32 + 0.5, (i as u32 / px) as f32 + 0.5));
                *t = rgba(data.terrain[nav.terrain_at(nav.cell_of(centre)) as usize].color);
            }
        }
        (geo, texels)
    }

    fn color_of(data: &GameData, id: &str) -> [u8; 4] {
        rgba(data.terrain_def(data.terrain_id(id).unwrap()).color)
    }

    /// The texel at `p` of an image drawn with `geo`.
    fn texel(texels: &[[u8; 4]], geo: &MinimapGeometry, p: IVec2) -> [u8; 4] {
        texels[(p.y * geo.px as i32 + p.x) as usize]
    }

    /// Whether texels passing `keep` join `from` to `to` (8-neighbours).
    fn joined(texels: &[[u8; 4]], geo: &MinimapGeometry, from: UVec2, to: UVec2, keep: impl Fn([u8; 4]) -> bool) -> bool {
        let n = geo.px as i32;
        let mut seen = vec![false; texels.len()];
        let mut stack = vec![from.as_ivec2()];
        while let Some(p) = stack.pop() {
            if p.cmplt(IVec2::ZERO).any() || p.cmpge(IVec2::splat(n)).any() || seen[(p.y * n + p.x) as usize] || !keep(texel(texels, geo, p)) {
                continue;
            }
            if p == to.as_ivec2() {
                return true;
            }
            seen[(p.y * n + p.x) as usize] = true;
            for d in [IVec2::X, IVec2::Y, IVec2::NEG_X, IVec2::NEG_Y, IVec2::ONE, IVec2::NEG_ONE, IVec2::new(1, -1), IVec2::new(-1, 1)] {
                stack.push(p + d);
            }
        }
        false
    }

    /// A map `side` metres square: a river 7 m wide (the narrowest the
    /// generator makes) winding across it with two fords, and a range with
    /// a 9 m pass. Returns the map, the river's course, the fords and where
    /// the range runs (its z, a point on it, and the pass's x).
    #[allow(clippy::type_complexity)]
    fn river_and_range(data: &GameData, side: u32) -> (NavGrid, [Vec2; 4], [Vec2; 2], (f32, Vec2, f32)) {
        let s = side as f32;
        let mut nav = new_nav(data, side, side, data.terrain_id("grass").unwrap());
        let river = [Vec2::new(0.0, 0.25 * s), Vec2::new(0.3 * s, 0.5 * s), Vec2::new(0.55 * s, 0.38 * s), Vec2::new(s, 0.66 * s)];
        paint(data, &mut nav, "shallow_water", &river, 3.5);
        paint(data, &mut nav, "deep_water", &river, 2.0);
        // Fords: across the full width, 10 m along the river.
        let fords = [river[1] + (river[2] - river[1]) * 0.5, river[2] + (river[3] - river[2]) * 0.3];
        for (f, seg) in fords.iter().zip([(river[1], river[2]), (river[2], river[3])]) {
            let along = (seg.1 - seg.0).normalize() * 5.0;
            paint(data, &mut nav, "ford", &[*f - along, *f + along], 3.6);
        }
        let (z, x0, x1, pass) = (0.85 * s, 0.2 * s, 0.8 * s, 0.5 * s);
        paint(data, &mut nav, "mountain", &[Vec2::new(x0, z), Vec2::new(pass - 4.5 - 8.0, z)], 8.0);
        paint(data, &mut nav, "mountain", &[Vec2::new(pass + 4.5 + 8.0, z), Vec2::new(x1, z)], 8.0);
        (nav, river, fords, (z, Vec2::new(0.3 * s, z), pass))
    }

    /// On the largest maps a texel spans metres of ground (3.4 m at the
    /// minimap's resolution, 6.9 m even at half of it): the river still
    /// draws as one line with its fords on it, and the pass stays open.
    #[test]
    fn rivers_fords_and_passes_survive_the_biggest_maps() {
        let data = test_data();
        let water = [color_of(&data, "deep_water"), color_of(&data, "shallow_water"), color_of(&data, "ford")];
        let (ford, mountain, grass) = (color_of(&data, "ford"), color_of(&data, "mountain"), color_of(&data, "grass"));
        for (side, px) in [(1024, MINIMAP_RES), (1372, MINIMAP_RES), (1372, MINIMAP_PX)] {
            let (nav, river, fords, (z, on_range, pass)) = river_and_range(&data, side);
            let (geo, texels) = terrain_texels(&data, &nav, px, false);
            let at = |p: Vec2| geo.pixel(p).unwrap();
            let what = format!("{side} m in {px} texels");
            // A water texel by each end of the river (in the edge column).
            let end = |p: Vec2, texels: &[[u8; 4]]| {
                let p = at(p).as_ivec2();
                (-1..=1).map(|dy| p + IVec2::new(0, dy)).find(|q| water.contains(&texel(texels, &geo, *q))).map(|q| q.as_uvec2())
            };
            let ends = (end(river[0], &texels).expect("water at the source"), end(river[3] - Vec2::X, &texels).expect("water at the mouth"));
            assert!(joined(&texels, &geo, ends.0, ends.1, |c| water.contains(&c)), "{what}: the river is unbroken");
            for f in fords {
                let p = at(f).as_ivec2();
                let mut near = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| p + IVec2::new(dx, dy)));
                assert!(near.any(|q| texel(&texels, &geo, q) == ford), "{what}: the ford at {f} shows");
            }
            assert_eq!(texel(&texels, &geo, at(on_range).as_ivec2()), mountain, "{what}: the range draws");
            let (north, south) = (at(Vec2::new(pass, z - 14.0)), at(Vec2::new(pass, z + 14.0)));
            assert!(joined(&texels, &geo, north, south, |c| c == grass), "{what}: the pass stays open");
        }
    }

    /// Water narrower than a texel still draws unbroken, where the cell
    /// under each texel's centre alone (the old way) leaves gaps.
    #[test]
    fn water_thinner_than_a_texel_stays_whole() {
        let data = test_data();
        let side = 1372;
        let s = side as f32;
        let mut nav = new_nav(&data, side, side, data.terrain_id("grass").unwrap());
        let channel = [Vec2::new(0.0, 0.08 * s), Vec2::new(s, 0.15 * s)];
        paint(&data, &mut nav, "deep_water", &channel, 1.2);
        let deep = color_of(&data, "deep_water");
        let (geo, texels) = terrain_texels(&data, &nav, MINIMAP_RES, false);
        assert!(1.0 / geo.scale() > 2.4 * 1.4, "a texel spans {} m", 1.0 / geo.scale());
        let column_end = |x: u32, texels: &[[u8; 4]]| (0..geo.px).map(|y| UVec2::new(x, y)).find(|p| texel(texels, &geo, p.as_ivec2()) == deep);
        let ends = (column_end(0, &texels).unwrap(), column_end(geo.px - 1, &texels).unwrap());
        assert!(joined(&texels, &geo, ends.0, ends.1, |c| c == deep), "the channel is unbroken");
        let (_, nearest) = terrain_texels(&data, &nav, MINIMAP_RES, true);
        let gaps = (0..geo.px).filter(|x| column_end(*x, &nearest).is_none()).count();
        assert!(gaps > 0, "the old way leaves columns without the channel");
    }

    /// Small maps still draw cell for cell: a texel inside one cell has
    /// that cell's colour, and the new terrains have theirs.
    #[test]
    fn small_maps_draw_the_cell_under_each_texel() {
        let data = test_data();
        let mut nav = new_nav(&data, 128, 128, data.terrain_id("grass").unwrap());
        paint(&data, &mut nav, "marsh", &[Vec2::new(10.0, 10.0), Vec2::new(60.0, 90.0)], 1.0);
        paint(&data, &mut nav, "ocean", &[Vec2::new(100.0, 0.0), Vec2::new(100.0, 128.0)], 6.0);
        paint(&data, &mut nav, "mountain", &[Vec2::new(20.0, 100.0), Vec2::new(60.0, 110.0)], 5.0);
        paint(&data, &mut nav, "scorched", &[Vec2::new(64.0, 64.0), Vec2::new(64.0, 64.5)], 8.0);
        let (geo, texels) = terrain_texels(&data, &nav, MINIMAP_RES, false);
        let colors: Vec<[u8; 4]> = data.terrain.iter().map(|t| rgba(t.color)).collect();
        let mut checked = 0;
        for (i, c) in texels.iter().enumerate() {
            let p = Vec2::new((i as u32 % MINIMAP_RES) as f32, (i as u32 / MINIMAP_RES) as f32);
            let (lo, hi) = (geo.to_world(p).floor(), (geo.to_world(p + Vec2::ONE) - 1e-4).floor());
            if lo == hi {
                assert_eq!(*c, colors[nav.terrain_at(lo.as_ivec2()) as usize], "texel {p}");
                checked += 1;
            }
        }
        assert!(checked > texels.len() / 2, "{checked}");
        for (id, at) in [("ocean", Vec2::new(100.0, 64.0)), ("mountain", Vec2::new(40.0, 105.0)), ("scorched", Vec2::new(64.0, 64.0))] {
            let px = geo.pixel(at).unwrap();
            assert_eq!(texels[(px.y * MINIMAP_RES + px.x) as usize], color_of(&data, id), "{id}");
        }
    }

    #[test]
    fn square_map_fills_the_image() {
        let g = MinimapGeometry::new(Vec2::splat(400.0), 200);
        assert_eq!(g.scale(), 0.5);
        assert_eq!(g.pixel(Vec2::ZERO), Some(UVec2::ZERO));
        assert_eq!(g.pixel(Vec2::new(399.9, 399.9)), Some(UVec2::splat(199)));
        assert_eq!(g.pixel(Vec2::new(400.0, 10.0)), None);
        assert_eq!(g.pixel(Vec2::new(-1.0, 10.0)), None);
        // Row index grows with world z.
        assert_eq!(g.pixel(Vec2::new(10.0, 300.0)), Some(UVec2::new(5, 150)));
        assert_eq!(g.to_world(Vec2::new(100.0, 50.0)), Vec2::new(200.0, 100.0));
    }

    #[test]
    fn non_square_maps_are_centred() {
        let g = MinimapGeometry::new(Vec2::new(400.0, 200.0), 200);
        assert_eq!(g.pixel(Vec2::ZERO), Some(UVec2::new(0, 50)));
        assert_eq!(g.pixel(Vec2::new(399.0, 199.0)), Some(UVec2::new(199, 149)));
        // Just off the map is the letterbox margin; further off leaves the image.
        assert_eq!(g.pixel(Vec2::new(0.0, -1.0)), Some(UVec2::new(0, 49)));
        assert_eq!(g.pixel(Vec2::new(0.0, -101.0)), None);
    }

    #[test]
    fn cursor_position_maps_to_world_and_clamps() {
        let g = MinimapGeometry::new(Vec2::splat(300.0), 200);
        assert_eq!(g.from_normalized(Vec2::ZERO), Vec2::splat(150.0));
        assert_eq!(g.from_normalized(Vec2::new(-0.5, -0.5)), Vec2::ZERO);
        assert_eq!(g.from_normalized(Vec2::new(0.25, -0.25)), Vec2::new(225.0, 75.0));
        // Dragging past the edge stays on the map.
        assert_eq!(g.from_normalized(Vec2::new(0.9, -0.7)), Vec2::new(300.0, 0.0));
    }

    #[test]
    fn world_pixel_round_trip() {
        let g = MinimapGeometry::new(Vec2::new(256.0, 320.0), 200);
        for p in [Vec2::new(10.0, 20.0), Vec2::new(250.0, 300.0), Vec2::new(128.0, 160.0)] {
            assert!((g.to_world(g.to_pixel(p)) - p).length() < 1e-3);
        }
    }

    #[test]
    fn minimap_targets_complete_point_commands() {
        let p = Vec2::new(3.0, 4.0);
        assert_eq!(command_at(TargetAction::Move, p), Some(CommandKind::Move(p)));
        assert_eq!(command_at(TargetAction::Attack, p), Some(CommandKind::AttackMove(p)));
        assert_eq!(command_at(TargetAction::Rally, p), Some(CommandKind::SetRally { point: p, target: None }));
        assert_eq!(command_at(TargetAction::Gather, p), None);
    }
}
