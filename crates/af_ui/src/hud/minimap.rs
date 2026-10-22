//! Minimap (bottom left): terrain, resource nodes, units and buildings in
//! player colours, fog, and the camera's view. Under fog it hides what the
//! world view (af_render) hides: enemy units unless some of their box is
//! in sight, buildings and nodes until scouted; remembered dead ones stay
//! drawn as their ghosts. Redrawn into a dynamic image four times a
//! second. Left click/drag moves the camera; right click sends the
//! selection there (or cancels a pending mode); a pending target command
//! (Move, Attack, Patrol, Rally) completes on the left-clicked point.

use af_core::ext::fog::{team_sees, FogGhosts, SeenBy};
use af_core::ext::input::RtsCamera;
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

/// Side of the minimap image in pixels (and logical UI px).
pub const MINIMAP_PX: u32 = 200;
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
        Extent3d { width: MINIMAP_PX, height: MINIMAP_PX, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &OFF_MAP,
        TextureFormat::Rgba8UnormSrgb,
        // Keep the CPU copy: it is rewritten on every redraw.
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    let handle = images.add(image);
    let len = (MINIMAP_PX * MINIMAP_PX * 4) as usize;
    commands.insert_resource(Minimap { image: handle.clone(), terrain: vec![0; len], terrain_for: None, frame: vec![0; len], since_redraw: REDRAW, dragging: false });
    handle
}

pub(super) fn spawn(bottom: &mut ChildSpawnerCommands, image: Handle<Image>) {
    bottom.spawn((theme::panel_node(Node { padding: UiRect::all(px(4)), flex_shrink: 0.0, ..default() }), theme::panel_colors(), crate::skin::SkinPanel::Panel)).with_child((
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
            }
        }
    }
}

/// An RGBA pixel buffer of `MINIMAP_PX` square.
struct Canvas<'a>(&'a mut [u8]);

impl Canvas<'_> {
    fn put(&mut self, x: i32, y: i32, c: [u8; 4]) {
        let n = MINIMAP_PX as i32;
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

    fn line(&mut self, a: Vec2, b: Vec2, c: [u8; 4]) {
        let d = b - a;
        // Far-off view corners would make huge loops; the image is small.
        let steps = d.abs().max_element().ceil().clamp(1.0, 4.0 * MINIMAP_PX as f32) as i32;
        for i in 0..=steps {
            let p = a + d * (i as f32 / steps as f32);
            self.put(p.x.floor() as i32, p.y.floor() as i32, c);
        }
    }
}

fn rgba(c: [f32; 3]) -> [u8; 4] {
    [(c[0].clamp(0.0, 1.0) * 255.0) as u8, (c[1].clamp(0.0, 1.0) * 255.0) as u8, (c[2].clamp(0.0, 1.0) * 255.0) as u8, 255]
}

fn draw_terrain(data: &GameData, nav: &NavGrid, geo: &MinimapGeometry, out: &mut [u8]) {
    let colors: Vec<[u8; 4]> = data.terrain.iter().map(|t| rgba(t.color)).collect();
    for (i, px) in out.chunks_exact_mut(4).enumerate() {
        let (x, y) = (i as u32 % MINIMAP_PX, i as u32 / MINIMAP_PX);
        let w = geo.to_world(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
        let c = if w.cmplt(Vec2::ZERO).any() || w.cmpge(geo.world).any() {
            OFF_MAP
        } else {
            colors.get(nav.terrain_at(nav.cell_of(w)) as usize).copied().unwrap_or(OFF_MAP)
        };
        px.copy_from_slice(&c);
    }
}

fn apply_fog(fog: &FogOfWar, team: u8, geo: &MinimapGeometry, out: &mut [u8]) {
    for (i, px) in out.chunks_exact_mut(4).enumerate() {
        let w = geo.to_world(Vec2::new((i as u32 % MINIMAP_PX) as f32 + 0.5, (i as u32 / MINIMAP_PX) as f32 + 0.5));
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
/// at least 2x2 pixels so they read as boxes, nodes at their size.
fn draw_mark(canvas: &mut Canvas, geo: &MinimapGeometry, kind: SpatialKind, obb: &Obb, color: [u8; 4]) {
    let (min, max) = obb.aabb();
    let half = match kind {
        SpatialKind::Unit => Vec2::splat(1.0),
        SpatialKind::Building => ((max - min) * 0.5 * geo.scale()).max(Vec2::splat(1.0)),
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
    let geo = MinimapGeometry::new(map.size, MINIMAP_PX);
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
