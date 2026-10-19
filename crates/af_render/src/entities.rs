//! Units, buildings and resource nodes as boxes.
//!
//! A sim entity gets `Transform` + `Visibility` and one child "visual root"
//! holding its boxes; [`BoxVisual`] on the sim entity remembers the root and
//! the parts that animate. When what the visual was built for changes
//! (unit/building upgrade, construction finished) the root is despawned and
//! rebuilt. Despawning the sim entity takes everything with it.
//!
//! Buildings are drawn from their [`BuildingView`] (live state, or how the
//! local team last saw an enemy building that is out of sight). Units whose
//! type has a model are drawn with it instead of body boxes (see
//! [`crate::model`]); buildings and nodes whose type has a static model
//! likewise (see [`crate::statics`]).

use af_core::ext::render::ground_height;
use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use af_core::ext::fog::SeenBy;

use crate::assets::{Painter, PartQuery, node};
use crate::fog::{BuildingView, Viewer, node_in_sight};
use crate::look::Look;
use crate::model::rig::TeamTints;
use crate::model::{ModelLibrary, ModelRoot, ModelUnit};
use crate::motion::{WeaponAnim, attack_phase, ground_transform, lerp_pos, lerp_yaw};
use crate::parts::{Part, SHIELD, SHIELD_ALPHA, UnitTraits, WeaponMount, building_layout, node_layout, node_scale, progress_transform, shadow_y, unit_layout};
use crate::statics::place::{construction_sink, sunk};
use crate::statics::{StaticLibrary, StaticVisual, rest_transform, spawn_instance};

/// Neutral grey for an owner missing from `Players`.
const NO_OWNER: Rgb = [0.6, 0.6, 0.6];
/// Seconds a shield stays bright after a hit.
const SHIELD_FLASH: f32 = 0.35;

/// What a visual was built for; a mismatch rebuilds it (unit and building
/// upgrades, finished construction, a shield granted by a tech, a model
/// that finished loading).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualSig {
    /// `model`: drawn with its type's model rather than boxes.
    Unit { kind: UnitTypeId, shield: bool, model: bool },
    /// `model`: drawn with its type's static model rather than body boxes.
    Building { kind: BuildingTypeId, foundation: bool, shield: bool, model: bool },
    /// `model`: drawn with its type's static model rather than boxes.
    Node { kind: NodeTypeId, model: bool },
}

/// An animated weapon pivot.
#[derive(Clone, Copy, Debug)]
pub struct WeaponPart {
    pub pivot: Entity,
    /// Index into `Weapons::slots`.
    pub slot: usize,
    /// Pivot position at rest.
    pub rest: Vec3,
    pub anim: WeaponAnim,
}

/// Render state on a sim entity (unit, building or node).
#[derive(Component, Debug)]
pub struct BoxVisual {
    /// Child holding every box; despawned on rebuild.
    pub root: Entity,
    pub sig: VisualSig,
    pub weapons: Vec<WeaponPart>,
    pub shield: Option<Entity>,
    pub cargo: Option<Entity>,
    pub shadow: Option<Entity>,
    /// Foundation's built-so-far box and its full-height layout (boxes
    /// only: a static model rises instead, see [`StaticVisual`]).
    pub progress: Option<(Entity, Part)>,
}

/// The visual ground under a point (see `af_core::ext::render::ground_height`);
/// 0 before the map exists.
#[derive(SystemParam)]
pub struct Ground<'w> {
    data: Res<'w, Data>,
    nav: Option<Res<'w, Nav>>,
}

impl Ground<'_> {
    pub fn at(&self, p: Vec2) -> f32 {
        self.nav.as_ref().map_or(0.0, |nav| ground_height(&self.data, nav, p))
    }

    /// Where flat markers (selection outlines, flyer shadows, click pings,
    /// rally flags) lie: on the drawn terrain, which can stand a little
    /// proud of the ground units walk on (sand), so it never hides them.
    pub fn surface(&self, p: Vec2) -> f32 {
        let Some(nav) = self.nav.as_ref() else { return 0.0 };
        let drawn = self.data.terrain.get(nav.terrain_at(nav.cell_of(p)) as usize).map_or(0.0, crate::terrain::flat_top);
        drawn.max(self.at(p))
    }

    /// Where a unit's base is drawn: flyers at their altitude, everything
    /// else that high above the ground.
    pub fn unit_base(&self, p: Vec2, altitude: f32, mobile: Option<&Mobile>) -> f32 {
        if mobile.is_some_and(|m| m.flying) { altitude } else { self.at(p) + altitude }
    }
}

/// The owner's colour (grey if the slot is unknown).
pub fn owner_color(players: &Players, owner: PlayerId) -> Rgb {
    players.try_get(owner).map_or(NO_OWNER, |p| p.color)
}

/// Spawns visual roots and their parts.
struct Builder<'a, 'w, 's, 'p> {
    commands: &'a mut Commands<'w, 's>,
    painter: &'a mut Painter<'p>,
}

impl Builder<'_, '_, '_, '_> {
    /// A fresh visual root under `entity` (replacing `old`).
    fn root(&mut self, entity: Entity, old: Option<&BoxVisual>, transform: Transform) -> Entity {
        if let Some(v) = old {
            self.commands.entity(v.root).try_despawn();
        }
        self.commands.spawn((node(transform, Visibility::Inherited), ChildOf(entity))).id()
    }

    fn parts(&mut self, root: Entity, parts: &[Part]) {
        for p in parts {
            self.painter.spawn_child(self.commands, root, p);
        }
    }

    fn part(&mut self, root: Entity, part: Option<Part>, hidden: bool) -> Option<Entity> {
        let part = part?;
        let e = self.painter.spawn_child(self.commands, root, &part);
        if hidden {
            self.commands.entity(e).insert(Visibility::Hidden);
        }
        Some(e)
    }

    /// A foundation's built part at `progress` of its full layout.
    fn progress(&mut self, root: Entity, full: Part, progress: f32) -> (Entity, Part) {
        let t = progress_transform(&full, progress);
        let built = Part { center: t.translation, size: t.scale, ..full };
        (self.painter.spawn_child(self.commands, root, &built), full)
    }

    fn weapons(&mut self, root: Entity, mounts: &[WeaponMount]) -> Vec<WeaponPart> {
        mounts
            .iter()
            .map(|m| {
                let pivot = self.commands.spawn((node(Transform::from_translation(m.pivot), Visibility::Inherited), ChildOf(root))).id();
                self.painter.spawn_child(self.commands, pivot, &m.part);
                WeaponPart { pivot, slot: m.slot, rest: m.pivot, anim: m.anim }
            })
            .collect()
    }
}

/// What [`attach_units`] builds a unit's visuals from.
#[derive(QueryData)]
pub(crate) struct UnitLook {
    entity: Entity,
    unit: &'static Unit,
    owner: &'static Owner,
    pose: (&'static PrevPosition, &'static Position, &'static PrevFacing, &'static Facing),
    altitude: &'static Altitude,
    mobile: Option<&'static Mobile>,
    footprint: &'static Footprint,
    shield: Has<Shield>,
    gatherer: Has<Gatherer>,
    visual: Option<&'static BoxVisual>,
}

/// Build unit visuals, and rebuild them after an upgrade, a granted shield,
/// or once the unit's model has loaded (every unit is looked at then).
///
/// A unit whose type has a loaded model gets the model's scene (a
/// [`ModelRoot`], scaled to its box height) instead of its body, weapon and
/// cargo boxes; it keeps its shield shell and flyer shadow.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn attach_units(
    mut commands: Commands,
    mut painter: Painter,
    data: Res<Data>,
    players: Res<Players>,
    fixed: Res<Time<Fixed>>,
    ground: Ground,
    models: Option<Res<ModelLibrary>>,
    fresh: Query<UnitLook, (Or<(Without<BoxVisual>, Changed<Unit>, Added<Shield>)>, Without<Dead>)>,
    all: Query<UnitLook, Without<Dead>>,
) {
    let t = fixed.overstep_fraction();
    let everyone = models.as_ref().is_some_and(|m| m.is_changed());
    let models = models.as_deref();
    let mut build = |u: UnitLookItem| {
        let (e, kind, shield) = (u.entity, u.unit.kind, u.shield);
        let model = models.and_then(|m| m.ready(kind));
        let sig = VisualSig::Unit { kind, shield, model: model.is_some() };
        let old = u.visual;
        if old.is_some_and(|v| v.sig == sig) {
            return;
        }
        let def = data.unit(kind);
        let traits = UnitTraits { gatherer: u.gatherer, shield, altitude: u.altitude.0 };
        let layout = unit_layout(&data, def, owner_color(&players, u.owner.0), traits);
        let mut b = Builder { commands: &mut commands, painter: &mut painter };
        let root = b.root(e, old, Transform::IDENTITY);
        let (weapons, cargo) = match model {
            Some((id, m, loaded)) => {
                let scale = Vec3::splat(m.scale_for(u.footprint.height));
                let scene = (WorldAssetRoot(loaded.scene.clone()), Transform::from_scale(scale), Visibility::Hidden);
                let model_root = b.commands.spawn((scene, ModelRoot { unit: e, model: id }, ChildOf(root))).id();
                b.commands.entity(e).try_insert(ModelUnit::new(e, id, model_root));
                (Vec::new(), None)
            }
            None => {
                b.parts(root, &layout.parts);
                if old.is_some() {
                    b.commands.entity(e).try_remove::<ModelUnit>();
                }
                (b.weapons(root, &layout.weapons), b.part(root, layout.cargo, true))
            }
        };
        let visual =
            BoxVisual { root, sig, weapons, shield: b.part(root, layout.shield, true), cargo, shadow: b.part(root, layout.shadow, false), progress: None };
        let mut ec = commands.entity(e);
        if old.is_none() {
            let (pp, p, pf, f) = u.pose;
            let at = lerp_pos(pp.0, p.0, t);
            ec.try_insert((ground_transform(at, ground.unit_base(at, u.altitude.0, u.mobile), lerp_yaw(pf.0, f.0, t)), Visibility::Inherited));
        }
        ec.try_insert(visual);
    };
    if everyone {
        all.iter().for_each(&mut build);
    } else {
        fresh.iter().for_each(&mut build);
    }
}

/// Build building visuals, and rebuild them when the building the local
/// team sees is upgraded or completed, or its type's model is ready.
///
/// A building whose type has a ready static model gets an instance of it
/// instead of its body boxes (a foundation keeps its pad and scaffold, and
/// its model is sunk by how much is left to build); weapons and the shield
/// shell stay boxes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn attach_buildings(
    mut commands: Commands,
    mut painter: Painter,
    mut tints: ResMut<TeamTints>,
    data: Res<Data>,
    players: Res<Players>,
    ground: Ground,
    statics: Option<Res<StaticLibrary>>,
    buildings: Query<(Entity, &BuildingView, &Owner, &Position, &Facing, Has<Shield>, Option<&BoxVisual>), Without<Dead>>,
) {
    let statics = statics.as_deref();
    for (e, view, owner, pos, facing, shield, old) in &buildings {
        let model = statics.and_then(|s| s.building(view.kind)).and_then(|(id, m)| m.variant_at(pos.0).map(|(variant, mesh)| (id, m, variant, mesh)));
        let sig = VisualSig::Building { kind: view.kind, foundation: view.construction.is_some(), shield, model: model.is_some() };
        if old.is_some_and(|v| v.sig == sig) {
            continue;
        }
        let def = data.building(view.kind);
        let color = owner_color(&players, owner.0);
        let progress = view.construction;
        let layout = building_layout(&data, def, color, progress, shield);
        let mut b = Builder { commands: &mut commands, painter: &mut painter };
        let root = b.root(e, old, Transform::IDENTITY);
        let weapons = b.weapons(root, &layout.weapons);
        let mut progress_box = None;
        match model {
            Some((id, m, variant, mesh)) => {
                // A foundation's pad and scaffold (all its layout but the
                // built-so-far box) stay; the model takes that box's place.
                if progress.is_some() {
                    b.parts(root, &layout.parts);
                }
                // Buried whole at first, the banner too.
                let rest = rest_transform(m, mesh, def.size, pos.0);
                let height = rest.scale.y * mesh.full_height();
                let at = sunk(rest, construction_sink(progress.unwrap_or(1.0), height));
                let instance = spawn_instance(b.commands, b.painter, &mut tints, mesh, at, Some(color), ChildOf(root));
                b.commands.entity(e).try_insert(StaticVisual { model: id, variant, instance, rest, height, shrinks: false });
            }
            None => {
                b.parts(root, &layout.parts);
                progress_box = layout.progress.zip(progress).map(|(full, p)| b.progress(root, full, p));
                if old.is_some() {
                    b.commands.entity(e).try_remove::<StaticVisual>();
                }
            }
        }
        let visual = BoxVisual { root, sig, weapons, shield: b.part(root, layout.shield, true), cargo: None, shadow: None, progress: progress_box };
        let mut ec = commands.entity(e);
        if old.is_none() {
            ec.try_insert((ground_transform(pos.0, ground.at(pos.0), facing.0), Visibility::Inherited));
        }
        ec.try_insert(visual);
    }
}

/// What [`attach_nodes`] builds a node's visuals from.
#[derive(QueryData)]
pub(crate) struct NodeLook {
    entity: Entity,
    node: &'static ResourceNode,
    pos: &'static Position,
    facing: &'static Facing,
    visual: Option<&'static BoxVisual>,
}

/// Build resource node visuals (nodes never change kind), and rebuild them
/// once their type's static model is ready (every node is looked at then).
/// A tree drawn with a model keeps its full size as it is gathered (it
/// falls when felled); every other node shrinks ([`shrink_nodes`]).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn attach_nodes(
    mut commands: Commands,
    mut painter: Painter,
    mut tints: ResMut<TeamTints>,
    data: Res<Data>,
    ground: Ground,
    statics: Option<Res<StaticLibrary>>,
    fresh: Query<NodeLook, (Without<BoxVisual>, Without<Dead>)>,
    all: Query<NodeLook, Without<Dead>>,
) {
    let everyone = statics.as_ref().is_some_and(|s| s.is_changed());
    let statics = statics.as_deref();
    let mut build = |n: NodeLookItem| {
        let (e, kind, old) = (n.entity, n.node.kind, n.visual);
        let model = statics.and_then(|s| s.node(kind)).and_then(|(id, m)| m.variant_at(n.pos.0).map(|(variant, mesh)| (id, m, variant, mesh)));
        let sig = VisualSig::Node { kind, model: model.is_some() };
        if old.is_some_and(|v| v.sig == sig) {
            return;
        }
        let def = data.node(kind);
        let shrinks = model.is_none_or(|(_, m, ..)| m.manifest.shrinks());
        let scale = if shrinks { node_scale(n.node.remaining, def.amount) } else { 1.0 };
        let mut b = Builder { commands: &mut commands, painter: &mut painter };
        let root = b.root(e, old, Transform::from_scale(Vec3::splat(scale)));
        match model {
            Some((id, m, variant, mesh)) => {
                let rest = rest_transform(m, mesh, def.size, n.pos.0);
                let instance = spawn_instance(b.commands, b.painter, &mut tints, mesh, rest, None, ChildOf(root));
                let height = rest.scale.y * mesh.size().y;
                b.commands.entity(e).try_insert(StaticVisual { model: id, variant, instance, rest, height, shrinks });
            }
            None => {
                b.parts(root, &node_layout(&data, def));
                if old.is_some() {
                    b.commands.entity(e).try_remove::<StaticVisual>();
                }
            }
        }
        let visual = BoxVisual { root, sig, weapons: Vec::new(), shield: None, cargo: None, shadow: None, progress: None };
        let mut ec = commands.entity(e);
        if old.is_none() {
            ec.try_insert((ground_transform(n.pos.0, ground.at(n.pos.0), n.facing.0), Visibility::Inherited));
        }
        ec.try_insert(visual);
    };
    if everyone {
        all.iter().for_each(&mut build);
    } else {
        fresh.iter().for_each(&mut build);
    }
}

/// Interpolate unit transforms between the last two sim ticks.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_units(
    fixed: Res<Time<Fixed>>,
    ground: Ground,
    mut units: Query<(&PrevPosition, &Position, &PrevFacing, &Facing, &Altitude, Option<&Mobile>, &mut Transform), (With<Unit>, With<BoxVisual>)>,
) {
    let t = fixed.overstep_fraction();
    for (pp, p, pf, f, alt, mobile, mut tf) in &mut units {
        let at = lerp_pos(pp.0, p.0, t);
        tf.set_if_neq(ground_transform(at, ground.unit_base(at, alt.0, mobile), lerp_yaw(pf.0, f.0, t)));
    }
}

/// Buildings and nodes are static; follow them only if the sim ever moves one.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_static(
    ground: Ground,
    mut q: Query<(&Position, &Facing, &mut Transform), (With<BoxVisual>, Without<Unit>, Or<(Changed<Position>, Changed<Facing>)>)>,
) {
    for (pos, facing, mut tf) in &mut q {
        tf.set_if_neq(ground_transform(pos.0, ground.at(pos.0), facing.0));
    }
}

/// Weapon swings/recoil/turret aim, shields, cargo, flyer shadows and
/// foundation progress (a box growing, or a model rising). A building drawn
/// from memory keeps its weapons as last seen and shows no shield.
#[allow(clippy::type_complexity)]
pub(crate) fn animate_parts(
    mut painter: Painter,
    data: Res<Data>,
    ground: Ground,
    actors: Query<(&BoxVisual, &Position, &Transform, Option<&Weapons>, Option<&Shield>, Option<&Gatherer>, Option<&BuildingView>, Option<&StaticVisual>), With<Owner>>,
    mut parts: PartQuery,
) {
    for (visual, pos, body, weapons, shield, gatherer, view, model) in &actors {
        let live = view.is_none_or(|v| v.live);
        if let Some(weapons) = weapons.filter(|_| live) {
            for w in &visual.weapons {
                let Some(slot) = weapons.slots.get(w.slot) else { continue };
                let Ok((mut tf, _, _)) = parts.get_mut(w.pivot) else { continue };
                let (offset, rotation) = w.anim.pose(attack_phase(slot.windup, slot.anim), slot.aim_yaw);
                tf.set_if_neq(Transform { translation: w.rest + offset, rotation, scale: Vec3::ONE });
            }
        }
        if let Some(e) = visual.shield {
            let look = shield.filter(|s| live && s.hp > 0.0).map(|s| {
                let flash = (1.0 - s.since_hit / SHIELD_FLASH).clamp(0.0, 1.0);
                Look::unlit(SHIELD, SHIELD_ALPHA.0 + (SHIELD_ALPHA.1 - SHIELD_ALPHA.0) * flash)
            });
            show(&mut painter, &mut parts, e, look);
        }
        if let (Some(e), Some(g)) = (visual.cargo, gatherer) {
            let load = g.carrying.filter(|_| g.amount > 0.0).map(|r| Look::lit(data.resource(r).color));
            show(&mut painter, &mut parts, e, load);
        }
        if let Some(e) = visual.shadow
            && let Ok((mut tf, _, _)) = parts.get_mut(e)
        {
            let y = shadow_y(ground.surface(pos.0), body.translation.y);
            if tf.translation.y != y {
                tf.translation.y = y;
            }
        }
        if let (Some((e, full)), Some(progress)) = (visual.progress, view.and_then(|v| v.construction))
            && let Ok((mut tf, _, _)) = parts.get_mut(e)
        {
            tf.set_if_neq(progress_transform(&full, progress));
        }
        if let (Some(m), Some(progress)) = (model, view.and_then(|v| v.construction))
            && let Ok((mut tf, _, _)) = parts.get_mut(m.instance)
        {
            tf.set_if_neq(sunk(m.rest, construction_sink(progress, m.height)));
        }
    }
}

/// Show `entity` with `look`, or hide it for `None`.
fn show(painter: &mut Painter, parts: &mut PartQuery, entity: Entity, look: Option<Look>) {
    let Ok((_, mut vis, mat)) = parts.get_mut(entity) else { return };
    match look {
        Some(look) => {
            vis.set_if_neq(Visibility::Inherited);
            if let Some(mut mat) = mat {
                let handle = painter.material(look);
                if mat.0 != handle {
                    mat.0 = handle;
                }
            }
        }
        None => {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
}

/// What [`shrink_nodes`] reads of a node.
type Shrinking<'a> = (&'a ResourceNode, Option<&'a SeenBy>, &'a BoxVisual, Option<&'a StaticVisual>);

/// Nodes shrink a little as they are gathered; the local team sees the size
/// a node had when it was last in sight. Trees drawn with a model keep
/// their size (they fall when felled). Only nodes whose amount, sighting or
/// visuals changed are looked at (every node when the local player does).
#[allow(clippy::type_complexity)]
pub(crate) fn shrink_nodes(
    data: Res<Data>,
    viewer: Viewer,
    changed: Query<Shrinking, Or<(Changed<ResourceNode>, Changed<SeenBy>, Changed<BoxVisual>)>>,
    all: Query<Shrinking>,
    mut roots: PartQuery,
) {
    let team = viewer.team();
    let mut shrink = |(n, seen, visual, model): Shrinking| {
        if !node_in_sight(team, seen) || model.is_some_and(|m| !m.shrinks) {
            return;
        }
        if let Ok((mut tf, _, _)) = roots.get_mut(visual.root) {
            let s = Vec3::splat(node_scale(n.remaining, data.node(n.kind).amount));
            if tf.scale != s {
                tf.scale = s;
            }
        }
    };
    if viewer.local_changed() {
        all.iter().for_each(&mut shrink);
    } else {
        changed.iter().for_each(&mut shrink);
    }
}
