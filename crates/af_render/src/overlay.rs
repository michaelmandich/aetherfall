//! Status overlays: health/shield/progress bars, selection outlines and
//! rally markers (whose fog rules [`crate::paths`] shares for the entities
//! units have orders on).

use af_core::ext::fog::{SeenBy, team_sees_box};
use af_core::ext::input::RtsCamera;
use af_core::prelude::*;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::assets::{BoxPart, Painter, PartQuery, no_shadows, node};
use crate::entities::{BoxVisual, Ground};
use crate::fog::{BuildingView, viewing_team};
use crate::look::{Look, health_color};
use crate::model::ModelUnit;
use crate::parts::{BarSpec, Part, min_line_width, outline_boxes, segment_transform};
use crate::scene::RenderScene;

const BAR_BACK: Rgb = [0.05, 0.05, 0.06];
const BAR_SHIELD: Rgb = [0.35, 0.65, 1.0];
const BAR_PROGRESS: Rgb = [0.95, 0.9, 0.7];
const OWN: Rgb = [0.25, 0.95, 0.35];
const HOSTILE: Rgb = [0.95, 0.2, 0.15];
const NEUTRAL: Rgb = [0.95, 0.85, 0.2];
/// Outline opacity when selected and when only hovered.
const OUTLINE_ALPHA: (f32, f32) = (0.9, 0.35);

/// Bar rows, bottom to top.
const ROW_PROGRESS: f32 = -1.0;
const ROW_HEALTH: f32 = 0.0;
const ROW_SHIELD: f32 = 1.0;

/// Outline colour for an entity owned by `owner` as seen by `local`:
/// green own, red hostile, yellow anything else (allies, nodes, spectating).
pub fn relation_color(players: &Players, local: Option<PlayerId>, owner: Option<PlayerId>) -> Rgb {
    match (local, owner) {
        (Some(l), Some(o)) if l == o => OWN,
        (Some(l), Some(o)) if players.hostile(l, o) => HOSTILE,
        _ => NEUTRAL,
    }
}

/// The bar boxes of one entity (children of the sim entity, created the
/// first time it needs bars and hidden when it does not).
#[derive(Component, Clone, Copy, Debug)]
pub struct BarParts {
    /// Turns to face the camera; hidden while no bar is needed.
    pub root: Entity,
    /// (background, fill) per row: progress, health, shield.
    pub rows: [(Entity, Entity); 3],
}

fn spawn_bars(commands: &mut Commands, painter: &mut Painter, parent: Entity, spec: &BarSpec) -> BarParts {
    let root = commands.spawn((node(spec.root(), Visibility::Hidden), ChildOf(parent))).id();
    let back = painter.material(Look::unlit(BAR_BACK, 0.85));
    let mut row = |r: f32, color: Rgb| {
        let fill = painter.material(Look::unlit(color, 1.0));
        let mesh = painter.cube();
        let mut bar = |material: Handle<StandardMaterial>, tf: Transform| {
            commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material), tf, Visibility::Inherited, BoxPart, no_shadows(), ChildOf(root))).id()
        };
        (bar(back.clone(), spec.background(r)), bar(fill, spec.fill(r, 1.0)))
    };
    let rows = [row(ROW_PROGRESS, BAR_PROGRESS), row(ROW_HEALTH, health_color(1.0)), row(ROW_SHIELD, BAR_SHIELD)];
    BarParts { root, rows }
}

/// The 3D cameras, marking af_input's RTS camera ([`rts_camera`]).
pub(crate) type Cameras<'w, 's> = Query<'w, 's, (&'static GlobalTransform, Has<RtsCamera>), With<Camera3d>>;

/// Where the view is from: af_input's RTS camera, else any 3D camera;
/// `None` when there is none (headless).
pub(crate) fn rts_camera<'a>(cameras: &'a Cameras) -> Option<&'a GlobalTransform> {
    let camera = cameras.iter().find(|(_, rts)| *rts).or_else(|| cameras.iter().next());
    camera.map(|(g, _)| g)
}

/// The world rotation bars face: the camera's ([`rts_camera`]), or one
/// derived from the rig when there is none (headless).
fn camera_rotation(cameras: &Cameras, rig: &CameraRig) -> Quat {
    rts_camera(cameras).map_or_else(|| Quat::from_rotation_y(rig.yaw) * Quat::from_rotation_x(-rig.pitch), |g| g.rotation())
}

/// Bars for entities that are damaged, selected, hovered or under
/// construction. Buildings show what the local team knows of them
/// ([`BuildingView`]): no shield bar for one drawn from memory.
#[allow(clippy::type_complexity)]
pub(crate) fn update_bars(
    mut commands: Commands,
    mut painter: Painter,
    cursor: Res<CursorWorld>,
    rig: Res<CameraRig>,
    camera: Cameras,
    actors: Query<(Entity, &Health, Option<&Shield>, Option<&BuildingView>, &Footprint, &Transform, Has<Selected>, Option<&BarParts>, Option<&ModelUnit>), With<BoxVisual>>,
    mut parts: PartQuery,
) {
    let cam = camera_rotation(&camera, &rig);
    let min_height = min_line_width(rig.distance);
    for (e, health, shield, view, fp, tf, selected, bars, model) in &actors {
        let shield = shield.filter(|s| s.max > 0.0).map(|s| (s.hp / s.max).clamp(0.0, 1.0));
        let (hp, progress, shield, fp) = match view {
            Some(v) => (v.hp, v.construction, shield.filter(|_| v.live), &v.footprint),
            None => (health.fraction(), None, shield, fp),
        };
        let need = selected || cursor.hovered == Some(e) || hp < 0.999 || progress.is_some();
        let Some(bars) = bars else {
            if need {
                let spawned = spawn_bars(&mut commands, &mut painter, e, &BarSpec::for_footprint(fp, min_height));
                commands.entity(e).try_insert(spawned);
            }
            continue;
        };
        if let Ok((_, mut vis, _)) = parts.get_mut(bars.root) {
            vis.set_if_neq(if need { Visibility::Inherited } else { Visibility::Hidden });
        }
        if !need {
            continue;
        }
        let spec = BarSpec::for_footprint(fp, min_height);
        if let Ok((mut root, _, _)) = parts.get_mut(bars.root) {
            // Straight above the entity (its model, where it is drawn off
            // the unit), facing the camera whatever way the entity is turned.
            let at = spec.root().translation + model.map_or(Vec3::ZERO, ModelUnit::body_offset);
            root.set_if_neq(Transform { translation: at, rotation: tf.rotation.inverse() * cam, ..spec.root() });
        }
        let rows = [
            (ROW_PROGRESS, progress.map(|p| (p, BAR_PROGRESS))),
            (ROW_HEALTH, Some((hp, health_color(hp)))),
            (ROW_SHIELD, shield.map(|f| (f, BAR_SHIELD))),
        ];
        for ((row, value), (bg, fill)) in rows.into_iter().zip(bars.rows) {
            set_bar(&mut painter, &mut parts, (bg, fill), &spec, row, value);
        }
    }
}

fn set_bar(painter: &mut Painter, parts: &mut PartQuery, (bg, fill): (Entity, Entity), spec: &BarSpec, row: f32, value: Option<(f32, Rgb)>) {
    let vis = if value.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    if let Ok((mut tf, mut v, _)) = parts.get_mut(bg) {
        v.set_if_neq(vis);
        tf.set_if_neq(spec.background(row));
    }
    let Ok((mut tf, mut v, mat)) = parts.get_mut(fill) else { return };
    v.set_if_neq(vis);
    let Some((fraction, color)) = value else { return };
    tf.set_if_neq(spec.fill(row, fraction));
    if let Some(mut mat) = mat {
        let handle = painter.material(Look::unlit(color, 1.0));
        if mat.0 != handle {
            mat.0 = handle;
        }
    }
}

/// The four outline boxes around an entity's footprint (children of the
/// sim entity, created on first selection/hover).
#[derive(Component, Clone, Copy, Debug)]
pub struct OutlineParts {
    /// Parent of the four sides, kept on the ground under the entity.
    pub root: Entity,
    sides: [Entity; 4],
    /// Footprint half extents the sides are laid around.
    pub half: Vec2,
    /// Least side width they were laid out with (follows the zoom).
    width: f32,
    look: Option<Look>,
}

/// Local height of the outline boxes above the root.
const OUTLINE_Y: f32 = 0.06;

/// Outline boxes around `fp`, `drop` below the entity's base (so a flyer's
/// outline lies on the ground under it).
fn spawn_outline(commands: &mut Commands, painter: &mut Painter, parent: Entity, fp: &Footprint, drop: f32, width: f32) -> OutlineParts {
    let root = commands.spawn((node(Transform::from_xyz(0.0, -drop, 0.0), Visibility::Hidden), ChildOf(parent))).id();
    let sides = outline_boxes(fp.half, OUTLINE_Y, width).map(|(center, size)| {
        let part = Part::new(center, size, Look::unlit(NEUTRAL, OUTLINE_ALPHA.0)).no_shadow();
        painter.spawn_child(commands, root, &part)
    });
    OutlineParts { root, sides, half: fp.half, width, look: None }
}

/// Selection outlines (strong) and the hovered entity's outline (faint),
/// around the footprint as it is now (upgrades change it) or, for a
/// building drawn from memory, as the local team last saw it.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn update_outlines(
    mut commands: Commands,
    mut painter: Painter,
    cursor: Res<CursorWorld>,
    rig: Res<CameraRig>,
    local: Res<LocalPlayer>,
    players: Res<Players>,
    ground: Ground,
    mut marked: Query<
        (Entity, Option<&Owner>, &Position, &Transform, &Footprint, Option<&BuildingView>, Has<Selected>, &mut OutlineParts, Option<&ModelUnit>),
        With<BoxVisual>,
    >,
    unmarked: Query<(&Footprint, Option<&BuildingView>, &Position, &Transform), (With<BoxVisual>, Without<OutlineParts>)>,
    selected_new: Query<Entity, (With<Selected>, With<BoxVisual>, Without<OutlineParts>)>,
    mut parts: PartQuery,
) {
    // First selection or hover of an entity: create its outline (shown next
    // frame). Once per entity, even when it is hovered as it is selected.
    let hovered = cursor.hovered.filter(|h| !selected_new.contains(*h));
    let width = min_line_width(rig.distance);
    for e in selected_new.iter().chain(hovered) {
        if let Ok((fp, view, pos, tf)) = unmarked.get(e) {
            let fp = view.map_or(fp, |v| &v.footprint);
            let outline = spawn_outline(&mut commands, &mut painter, e, fp, tf.translation.y - ground.surface(pos.0), width);
            commands.entity(e).try_insert(outline);
        }
    }
    for (e, owner, pos, body, fp, view, selected, mut outline, model) in &mut marked {
        let half = view.map_or(fp, |v| &v.footprint).half;
        // Re-laid when the footprint changes or the zoom moves the least width by a tenth.
        if half != outline.half || (outline.width - width).abs() > width * 0.1 {
            outline.half = half;
            outline.width = width;
            for (side, (center, size)) in outline.sides.into_iter().zip(outline_boxes(half, OUTLINE_Y, width)) {
                if let Ok((mut tf, _, _)) = parts.get_mut(side) {
                    *tf = Transform::from_translation(center).with_scale(size);
                }
            }
        }
        let alpha = if selected {
            Some(OUTLINE_ALPHA.0)
        } else if cursor.hovered == Some(e) {
            Some(OUTLINE_ALPHA.1)
        } else {
            None
        };
        let look = alpha.map(|a| Look::unlit(relation_color(&players, local.0, owner.map(|o| o.0)), a));
        if let Ok((mut tf, mut vis, _)) = parts.get_mut(outline.root) {
            vis.set_if_neq(if look.is_some() { Visibility::Inherited } else { Visibility::Hidden });
            // Flyers: the outline stays on the ground under them. A model
            // drawn off its unit (stepped up to its work) takes it along.
            let at = model.map_or(Vec3::ZERO, ModelUnit::body_offset).with_y(ground.surface(pos.0) - body.translation.y);
            tf.set_if_neq(Transform { translation: at, ..*tf });
        }
        let Some(look) = look.filter(|l| outline.look != Some(*l)) else { continue };
        outline.look = Some(look);
        let handle = painter.material(look);
        for side in outline.sides {
            if let Ok((_, _, Some(mut mat))) = parts.get_mut(side) {
                mat.0 = handle.clone();
            }
        }
    }
}

/// Marker on rally flags and lines.
#[derive(Component)]
pub struct RallyPart;

#[derive(Clone, Copy, Debug)]
struct RallyMarker {
    flag: Entity,
    line: Entity,
}

/// What the viewing team last knew of an entity rally target it does not
/// own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sighting {
    /// Where the team last saw it.
    pos: Vec2,
    /// It was in sight the last frame it was looked up.
    in_sight: bool,
}

/// Rally markers, reused frame to frame, and what the local team knows of
/// the entities its rallies follow. Client-only bookkeeping, so hashing
/// order does not matter.
#[derive(Resource, Default)]
pub struct RallyPool {
    markers: Vec<RallyMarker>,
    /// Entity rally targets of other teams, as the viewing team last saw them.
    sightings: HashMap<Entity, Sighting>,
    /// The viewing team `sightings` belong to.
    team: Option<u8>,
    /// Scratch: where this frame draws each entity rally of the local
    /// player's buildings (`None`: not drawn).
    points: HashMap<Entity, Option<Vec2>>,
}

#[allow(clippy::type_complexity)]
pub(crate) type RallyTargets<'w, 's> =
    Query<'w, 's, (&'static Position, Option<&'static Owner>, Option<&'static Footprint>, Option<&'static Facing>, Option<&'static SeenBy>, Has<Unit>)>;

/// Where the viewing `team` (`None`: nothing hidden) believes the entity
/// rally target `t` is, keeping `sightings` current. The live position for
/// the team's own entities; for others, only what the team has seen: a
/// unit's position while some of its box is in sight, else where it was
/// last seen (also after it dies out of sight, which the team cannot know);
/// a building or node's (they never move) once the team has seen it.
pub(crate) fn believed_rally_point(t: Entity, team: Option<u8>, fog: &FogOfWar, players: &Players, targets: &RallyTargets, sightings: &mut HashMap<Entity, Sighting>) -> Option<Vec2> {
    let Ok((pos, owner, fp, facing, seen, is_unit)) = targets.get(t) else {
        return match sightings.get(&t) {
            Some(s) if !s.in_sight => Some(s.pos),
            _ => {
                // Gone in sight (or nothing hidden): the flag goes too.
                sightings.remove(&t);
                None
            }
        };
    };
    let Some(team) = team.filter(|tm| owner.is_none_or(|o| players.team(o.0) != *tm)) else {
        return Some(pos.0);
    };
    let (in_sight, known) = if is_unit {
        let half = fp.map_or(Vec2::ZERO, |f| f.half);
        let s = team_sees_box(fog, team, &Obb::new(pos.0, half, facing.map_or(0.0, |f| f.0)));
        (s, s)
    } else {
        (seen.is_some_and(|s| s.visible_to(team)), seen.is_some_and(|s| s.seen_by(team)))
    };
    if known {
        sightings.insert(t, Sighting { pos: pos.0, in_sight });
    } else if let Some(s) = sightings.get_mut(&t) {
        s.in_sight = false;
    }
    sightings.get(&t).map(|s| s.pos)
}

fn spawn_rally(commands: &mut Commands, painter: &mut Painter, color: Rgb) -> RallyMarker {
    let flag = commands.spawn((Transform::default(), Visibility::Hidden, RallyPart, RenderScene)).id();
    let cloth = Look::unlit(color, 1.0);
    for part in [
        Part::column(0.0, 0.0, 0.07, 0.07, 0.0, 1.8, Look::lit([0.35, 0.28, 0.2])),
        Part::column(0.335, 0.0, 0.6, 0.05, 1.4, 1.78, cloth),
        Part::column(0.0, 0.0, 0.5, 0.5, 0.0, 0.04, cloth).no_shadow(),
    ] {
        painter.spawn_child(commands, flag, &part);
    }
    let line = commands
        .spawn((
            Mesh3d(painter.cube()),
            MeshMaterial3d(painter.material(Look::unlit(color, 0.7))),
            Transform::default(),
            Visibility::Hidden,
            no_shadows(),
            RallyPart,
            RenderScene,
        ))
        .id();
    RallyMarker { flag, line }
}

/// A flag at the rally point of each selected own building, with a line to
/// it. A rally that follows an entity of another team shows only what the
/// local team has seen of it ([`believed_rally_point`]), so the flag never
/// tracks an enemy through the fog.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn update_rally(
    mut commands: Commands,
    mut painter: Painter,
    mut pool: ResMut<RallyPool>,
    local: Res<LocalPlayer>,
    players: Res<Players>,
    fog: Res<FogOfWar>,
    ground: Ground,
    buildings: Query<(&Position, &Owner, &Rally, Has<Selected>), With<Building>>,
    targets: RallyTargets,
    mut markers: Query<(&mut Transform, &mut Visibility), With<RallyPart>>,
) {
    let color = local.0.and_then(|p| players.try_get(p)).map_or(NEUTRAL, |p| p.color);
    let team = viewing_team(&fog, &local, &players);
    let pool = &mut *pool;
    if pool.team != team {
        pool.sightings.clear();
        pool.team = team;
    }
    // Every entity rally of the local player's buildings, selected or not,
    // so one selected later shows what the team last saw.
    pool.points.clear();
    for (_, owner, rally, _) in &buildings {
        let Some(RallyTarget::Entity(t)) = rally.0 else { continue };
        if Some(owner.0) != local.0 || pool.points.contains_key(&t) {
            continue;
        }
        let point = believed_rally_point(t, team, &fog, &players, &targets, &mut pool.sightings);
        pool.points.insert(t, point);
    }
    let points = &pool.points;
    pool.sightings.retain(|t, _| points.contains_key(t));
    let mut used = 0;
    for (pos, owner, rally, selected) in &buildings {
        if !selected || Some(owner.0) != local.0 {
            continue;
        }
        let point = match rally.0 {
            Some(RallyTarget::Point(p)) => p,
            Some(RallyTarget::Entity(t)) => match pool.points.get(&t) {
                Some(Some(p)) => *p,
                _ => continue,
            },
            None => continue,
        };
        if used == pool.markers.len() {
            let marker = spawn_rally(&mut commands, &mut painter, color);
            pool.markers.push(marker);
        }
        let m = pool.markers[used];
        used += 1;
        let (from, to) = (ground.surface(pos.0), ground.surface(point));
        let line = segment_transform(Vec3::new(pos.0.x, from + 0.12, pos.0.y), Vec3::new(point.x, to + 0.12, point.y), 0.08, 0.03);
        for (e, tf) in [(m.flag, Transform::from_xyz(point.x, to, point.y)), (m.line, line)] {
            if let Ok((mut t, mut vis)) = markers.get_mut(e) {
                t.set_if_neq(tf);
                vis.set_if_neq(Visibility::Inherited);
            }
        }
    }
    for m in &pool.markers[used..] {
        for e in [m.flag, m.line] {
            if let Ok((_, mut vis)) = markers.get_mut(e) {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_colours_follow_relations() {
        let data = af_core::testing::test_data();
        let mut app = af_core::testing::sim_app(data);
        let ids = af_core::testing::flat_world(&mut app, 64.0, &[("ironhold", 0), ("dune", 1), ("tide", 0)]);
        let players = app.world().resource::<Players>().clone();
        assert_eq!(relation_color(&players, Some(ids[0]), Some(ids[0])), OWN);
        assert_eq!(relation_color(&players, Some(ids[0]), Some(ids[1])), HOSTILE);
        assert_eq!(relation_color(&players, Some(ids[0]), Some(ids[2])), NEUTRAL, "allies");
        assert_eq!(relation_color(&players, Some(ids[0]), None), NEUTRAL, "nodes");
        assert_eq!(relation_color(&players, None, Some(ids[1])), NEUTRAL, "spectating");
    }
}
