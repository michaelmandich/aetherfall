//! Fog of war as seen by the local team, on top of af_fog's model:
//!
//! * units of other teams are drawn while any part of their box is in
//!   sight ([`team_sees_box`]);
//! * buildings and nodes once the team has ever seen them ([`SeenBy`],
//!   AoE-style memory; a missing `SeenBy` means nobody has yet);
//! * a scouted enemy building out of sight as the team last saw it
//!   ([`LastSeen`], through [`BuildingView`]), and a node at the size it
//!   last had in sight, so upgrades, construction, damage and gathering
//!   inside the fog stay hidden;
//! * buildings and nodes that died out of sight stay drawn from
//!   [`FogGhosts`] until the team looks again;
//! * the land itself darkened by the terrain, water and prop shaders from
//!   a texture of `FogOfWar` ([`FogTexture`]: near black unexplored,
//!   darkened and greyed explored, clear in sight).
//!
//! Spectators and fog-off matches see everything.

use af_core::ext::fog::{BuildingSighting, FogGhosts, LastSeen, SeenBy, team_sees_box};
use af_core::ext::render::FogTexture;
use af_core::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::image::ImageSampler;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::assets::Painter;
use crate::entities::{BoxVisual, Ground, owner_color};
use crate::fx::FxDrawn;
use crate::model::rig::TeamTints;
use crate::motion::ground_transform;
use crate::parts::{building_layout, node_layout};
use crate::scene::RenderScene;
use crate::statics::{StaticLibrary, rest_transform, spawn_instance};

/// Seconds between fog texture refreshes.
const OVERLAY_PERIOD: f32 = 0.1;

/// The team whose sight hides things, or `None` when everything is shown
/// (spectating, or fog disabled).
pub fn viewing_team(fog: &FogOfWar, local: &LocalPlayer, players: &Players) -> Option<u8> {
    if !fog.enabled {
        return None;
    }
    local.0.map(|p| players.team(p))
}

/// The local team's point of view (see [`viewing_team`]).
#[derive(SystemParam)]
pub struct Viewer<'w> {
    fog: Res<'w, FogOfWar>,
    local: Res<'w, LocalPlayer>,
    players: Option<Res<'w, Players>>,
}

impl Viewer<'_> {
    /// The team whose sight hides things, or `None` when everything is shown.
    pub fn team(&self) -> Option<u8> {
        viewing_team(&self.fog, &self.local, self.players.as_deref()?)
    }

    /// Team of a player (`None` before `Players` exists).
    pub fn team_of(&self, player: PlayerId) -> Option<u8> {
        self.players.as_ref().map(|p| p.team(player))
    }

    /// The local player changed since the system last ran (so may the
    /// team whose sight hides things).
    pub fn local_changed(&self) -> bool {
        self.local.is_changed()
    }
}

/// How the local team sees a building: its live state, or af_fog's memory
/// of it while it is an enemy building out of sight. Kept on every
/// building by [`view_buildings`]; the systems that draw buildings, their
/// bars and outlines read this instead of the live `Building`,
/// `UnderConstruction`, `Health` and `Footprint`.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BuildingView {
    pub kind: BuildingTypeId,
    /// Build progress (0..1) while a foundation.
    pub construction: Option<f32>,
    /// Health fraction.
    pub hp: f32,
    pub footprint: Footprint,
    /// Drawn from live state. Memory also hides what only live state shows
    /// (shield shell, weapon aim).
    pub live: bool,
}

/// What `team` (`None`: everything shown) believes about a building of
/// `owner_team` whose live state is `live`: the live state while it is the
/// team's own or in sight, else af_fog's latest sighting (the live state
/// again if af_fog has none, e.g. it has not run yet). The flag is true
/// for the live state.
pub fn believed(team: Option<u8>, owner_team: u8, live: BuildingSighting, seen: Option<&SeenBy>, last: Option<&LastSeen>) -> (BuildingSighting, bool) {
    let memory = team.filter(|t| *t != owner_team && !seen.is_some_and(|s| s.visible_to(*t))).and_then(|t| last?.by(t));
    match memory {
        Some(sighting) => (*sighting, false),
        None => (live, true),
    }
}

/// Keep every building's [`BuildingView`] current for the local team.
#[allow(clippy::type_complexity)]
pub(crate) fn view_buildings(
    mut commands: Commands,
    data: Res<Data>,
    viewer: Viewer,
    mut buildings: Query<
        (Entity, &Building, &Owner, Option<&UnderConstruction>, Option<&Health>, &Footprint, Option<&SeenBy>, Option<&LastSeen>, Option<&mut BuildingView>),
        Without<Dead>,
    >,
) {
    let team = viewer.team();
    for (e, building, owner, construction, health, footprint, seen, last, view) in &mut buildings {
        let live = BuildingSighting { kind: building.kind, construction: construction.map(|c| c.progress), hp: health.map_or(1.0, Health::fraction) };
        let owner_team = viewer.team_of(owner.0).unwrap_or(af_geom::NO_TEAM);
        let (sighting, is_live) = believed(team, owner_team, live, seen, last);
        let footprint = if is_live { *footprint } else { Footprint::from_size(data.building(sighting.kind).size) };
        let next = BuildingView { kind: sighting.kind, construction: sighting.construction, hp: sighting.hp, footprint, live: is_live };
        match view {
            Some(mut view) => {
                view.set_if_neq(next);
            }
            None => {
                commands.entity(e).try_insert(next);
            }
        }
    }
}

/// Does the local team see this node right now (or is nothing hidden)?
pub fn node_in_sight(team: Option<u8>, seen: Option<&SeenBy>) -> bool {
    team.is_none_or(|t| seen.is_some_and(|s| s.visible_to(t)))
}

/// Overlay opacity of one fog cell.
pub fn fog_alpha(visible: bool, explored: bool) -> u8 {
    if visible {
        0
    } else if explored {
        128
    } else {
        255
    }
}

fn visibility(shown: bool) -> Visibility {
    if shown { Visibility::Inherited } else { Visibility::Hidden }
}

/// Units of other teams are drawn only while some of their box (as it
/// stands and faces in the sim) is in sight.
#[allow(clippy::type_complexity)]
pub(crate) fn fog_units(
    fog: Res<FogOfWar>,
    local: Res<LocalPlayer>,
    players: Res<Players>,
    mut units: Query<(&Owner, &Position, &Facing, &Footprint, &mut Visibility), (With<Unit>, With<BoxVisual>)>,
) {
    let team = viewing_team(&fog, &local, &players);
    for (owner, pos, facing, fp, mut vis) in &mut units {
        let shown = team.is_none_or(|t| players.team(owner.0) == t || team_sees_box(&fog, t, &fp.obb(pos.0, facing.0)));
        vis.set_if_neq(visibility(shown));
    }
}

/// Buildings of other teams and resource nodes are drawn once seen.
#[allow(clippy::type_complexity)]
pub(crate) fn fog_structures(
    fog: Res<FogOfWar>,
    local: Res<LocalPlayer>,
    players: Res<Players>,
    mut q: Query<(Option<&Owner>, Option<&SeenBy>, &mut Visibility), (Or<(With<Building>, With<ResourceNode>)>, With<BoxVisual>)>,
) {
    let team = viewing_team(&fog, &local, &players);
    for (owner, seen, mut vis) in &mut q {
        let shown = team.is_none_or(|t| owner.is_some_and(|o| players.team(o.0) == t) || seen.is_some_and(|s| s.seen_by(t)));
        vis.set_if_neq(visibility(shown));
    }
}

/// Effects are drawn only where the local team can see.
pub(crate) fn fog_fx(fog: Res<FogOfWar>, local: Res<LocalPlayer>, players: Res<Players>, mut fx: Query<(&FxBox, &mut Visibility), With<FxDrawn>>) {
    let team = viewing_team(&fog, &local, &players);
    for (b, mut vis) in &mut fx {
        let shown = team.is_none_or(|t| fog.visible(t, b.pos.xz()));
        vis.set_if_neq(visibility(shown));
    }
}

/// Marker on the boxes of a remembered dead building or node.
#[derive(Component, Clone, Copy, Debug)]
pub struct FogMemory;

/// Drawn [`FogGhosts`]: dead entity -> visual root, and the team they were
/// drawn for. Client-only bookkeeping, so hashing order does not matter.
#[derive(Resource, Default)]
pub struct MemoryPool {
    drawn: HashMap<Entity, Entity>,
    team: Option<u8>,
    /// Scratch: the dead entities the team still remembers.
    wanted: HashSet<Entity>,
}

impl MemoryPool {
    pub fn reset(&mut self) {
        *self = MemoryPool::default();
    }

    /// Number of remembered dead buildings/nodes being drawn.
    pub fn len(&self) -> usize {
        self.drawn.len()
    }

    pub fn is_empty(&self) -> bool {
        self.drawn.is_empty()
    }
}

/// Keep drawing buildings and nodes that died out of the local team's
/// sight, as the team last saw them (with their type's static model when it
/// is ready), until it looks again.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_fog_memories(
    mut commands: Commands,
    mut painter: Painter,
    mut tints: ResMut<TeamTints>,
    data: Res<Data>,
    players: Res<Players>,
    fog: Res<FogOfWar>,
    local: Res<LocalPlayer>,
    ghosts: Res<FogGhosts>,
    statics: Option<Res<StaticLibrary>>,
    ground: Ground,
    mut pool: ResMut<MemoryPool>,
) {
    let team = viewing_team(&fog, &local, &players);
    if !ghosts.is_changed() && pool.team == team {
        return;
    }
    pool.team = team;
    let MemoryPool { drawn, wanted, .. } = &mut *pool;
    wanted.clear();
    if let Some(t) = team {
        wanted.extend(ghosts.remembered_by(t).map(|g| g.entity));
    }
    drawn.retain(|dead, root| {
        let keep = wanted.contains(dead);
        if !keep {
            commands.entity(*root).try_despawn();
        }
        keep
    });
    let Some(team) = team else { return };
    let statics = statics.as_deref();
    for g in ghosts.remembered_by(team) {
        if drawn.contains_key(&g.entity) {
            continue;
        }
        let owner = g.owner.map(|o| owner_color(&players, o));
        let (size, model, parts) = match g.kind {
            EntityKind::Building(kind) => {
                let def = data.building(kind);
                (def.size, statics.and_then(|s| s.building(kind)), building_layout(&data, def, owner.unwrap_or([0.6; 3]), None, false).parts)
            }
            EntityKind::Node(kind) => (data.node(kind).size, statics.and_then(|s| s.node(kind)), node_layout(&data, data.node(kind))),
            EntityKind::Unit(_) => continue,
        };
        let root = commands.spawn((ground_transform(g.pos, ground.at(g.pos), g.facing), Visibility::Inherited, FogMemory, RenderScene)).id();
        match model.and_then(|(_, m)| m.variant_at(g.pos).map(|(_, mesh)| (m, mesh))) {
            Some((m, mesh)) => {
                spawn_instance(&mut commands, &mut painter, &mut tints, mesh, rest_transform(m, mesh, size, g.pos), owner, ChildOf(root));
            }
            None => {
                for p in &parts {
                    painter.spawn_child(&mut commands, root, p);
                }
            }
        }
        drawn.insert(g.entity, root);
    }
}

/// The fog-of-war texture ([`FogTexture`]) and when it was last written.
/// The terrain, water and prop shaders darken the world with it
/// (`terrain::bind_fog`); there is no overlay plane, which could not lie on
/// mountains.
#[derive(Resource, Default)]
pub struct FogOverlay {
    image: Option<Handle<Image>>,
    dims: (u32, u32),
    since: f32,
    dirty: bool,
}

impl FogOverlay {
    pub fn reset(&mut self) {
        *self = FogOverlay::default();
    }
}

fn fog_image(width: u32, height: u32) -> Image {
    let mut image = Image::new_fill(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        // MAIN_WORLD keeps the CPU copy so it can be rewritten.
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

/// Write one team's fog into RGBA pixels (only alpha changes).
pub fn write_fog_pixels(pixels: &mut [u8], team: &TeamFog) {
    for (i, px) in pixels.chunks_exact_mut(4).enumerate() {
        let visible = team.visible.get(i).is_some_and(|v| *v > 0);
        let explored = team.explored.get(i).copied().unwrap_or(false);
        px[3] = fog_alpha(visible, explored);
    }
}

/// Keep the fog-of-war texture in step with `FogOfWar` about 10 times a
/// second while the local team's fog is shown (none for spectators and
/// fog-off matches).
pub(crate) fn update_fog_overlay(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    fog: Res<FogOfWar>,
    local: Res<LocalPlayer>,
    players: Res<Players>,
    time: Res<Time<Real>>,
    mut overlay: ResMut<FogOverlay>,
) {
    let team = viewing_team(&fog, &local, &players).filter(|_| fog.width > 0 && fog.height > 0);
    let Some(team) = team else {
        // Nothing hidden: drop the shared texture.
        if overlay.image.is_some() {
            commands.remove_resource::<FogTexture>();
            overlay.reset();
        }
        return;
    };
    if fog.is_changed() {
        overlay.dirty = true;
    }
    if overlay.dims != (fog.width, fog.height) || overlay.image.is_none() {
        let image = images.add(fog_image(fog.width, fog.height));
        commands.insert_resource(FogTexture(image.clone()));
        *overlay = FogOverlay { image: Some(image), dims: (fog.width, fog.height), since: OVERLAY_PERIOD, dirty: true };
    }
    overlay.since += time.delta_secs();
    if !overlay.dirty || overlay.since < OVERLAY_PERIOD {
        return;
    }
    let Some(team_fog) = fog.teams.get(team as usize) else { return };
    let Some(mut image) = overlay.image.as_ref().and_then(|h| images.get_mut(h)) else { return };
    let Some(pixels) = image.data.as_mut() else { return };
    write_fog_pixels(pixels, team_fog);
    overlay.since = 0.0;
    overlay.dirty = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_alpha_levels() {
        assert_eq!(fog_alpha(true, true), 0);
        assert_eq!(fog_alpha(true, false), 0, "in sight is clear even if exploration lags");
        assert_eq!(fog_alpha(false, true), 128);
        assert_eq!(fog_alpha(false, false), 255);
    }

    #[test]
    fn pixels_follow_the_team_grid() {
        let team = TeamFog { visible: vec![1, 0, 0, 2], explored: vec![true, true, false, true] };
        let mut pixels = vec![9u8; 16];
        write_fog_pixels(&mut pixels, &team);
        let alphas: Vec<u8> = pixels.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alphas, vec![0, 128, 255, 0]);
        assert_eq!(pixels[0], 9, "colour channels untouched");
    }
}
