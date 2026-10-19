//! A model instance on a unit: spawning it, and wiring it up once its glTF
//! scene is in the world (animation player, props, team colour).

use af_core::prelude::*;
use bevy::animation::AnimationTargetId;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use super::animator::Animator;
use super::library::{ModelId, ModelLibrary};
use super::props::{Prop, Shown, Tool, classify};
use super::stance::Lean;
use crate::entities::owner_color;
use crate::look::MatKey;

/// Weapon slots whose windups start attack clips.
pub const TRACKED_SLOTS: usize = 4;

/// The entity holding a model's scene (a `WorldAssetRoot`), under a unit's
/// visual root; on its own once it is a [`Corpse`]. Scaled to the unit's
/// box height. Hidden until [`Rig`]ged, so its bind pose never shows.
#[derive(Component, Clone, Copy, Debug)]
pub struct ModelRoot {
    /// The unit it draws (gone once it is a corpse).
    pub unit: Entity,
    pub model: ModelId,
}

/// The scene instance under a [`ModelRoot`] has spawned.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneReady;

/// What drives a model instance, found once its scene is in the world.
#[derive(Component, Clone, Debug, Default)]
pub struct Rig {
    /// The entity with the `AnimationPlayer` (none: a model without bones).
    pub player: Option<Entity>,
    /// Every tool and load node.
    pub props: Vec<(Prop, Entity)>,
    /// Every animated node with its transform as spawned: the bind pose,
    /// put back when the model has no clip to play (not even Idle).
    pub bind_pose: Vec<(Entity, Transform)>,
}

/// A unit drawn with a model (on the sim entity, next to `BoxVisual`).
#[derive(Component, Clone, Debug)]
pub struct ModelUnit {
    pub model: ModelId,
    /// Its [`ModelRoot`].
    pub root: Entity,
    pub(crate) anim: Animator,
    /// Props as last applied (`None`: not yet).
    pub(crate) shown: Option<Shown>,
    /// The tool for the work the front order leads to.
    pub(crate) heading: Option<Tool>,
    /// The model's pose relative to the unit: turned toward its work and
    /// stepped up to it, else upright (see [`super::stance`]).
    pub(crate) lean: Lean,
    /// `WeaponSlot::windup` of the tracked slots last frame.
    pub(crate) windups: [f32; TRACKED_SLOTS],
    /// Trained, waiting for the rig to play Spawn.
    pub(crate) spawned: bool,
    /// Its owner aged up, waiting for the rig to cheer.
    pub(crate) cheer: bool,
}

impl ModelUnit {
    pub fn new(unit: Entity, model: ModelId, root: Entity) -> ModelUnit {
        ModelUnit {
            model,
            root,
            anim: Animator::new(unit.index_u32()),
            shown: None,
            heading: None,
            lean: Lean::default(),
            windups: [0.0; TRACKED_SLOTS],
            spawned: false,
            cheer: false,
        }
    }

    /// The animation state (for inspection).
    pub fn animator(&self) -> &Animator {
        &self.anim
    }

    /// Where the model is drawn relative to its unit (the unit's frame:
    /// x right, z forward; metres): stepped up to its work, else 0. The
    /// selection outline and bars follow it.
    pub fn body_offset(&self) -> Vec3 {
        let o = self.lean.offset();
        Vec3::new(o.x, 0.0, o.y)
    }
}

/// A dead unit's model: plays Death, lies still, sinks and goes
/// ([`super::animator::corpse_drop`]).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Corpse {
    pub age: f32,
    /// Height it lies at.
    pub rest_y: f32,
    pub(crate) started: bool,
}

/// Per-owner copies of team materials, by (material, quantised colour).
#[derive(Resource, Default)]
pub struct TeamTints {
    tints: HashMap<(AssetId<StandardMaterial>, MatKey), Handle<StandardMaterial>>,
}

impl TeamTints {
    /// `base` coloured `color` (made once per base and colour).
    pub fn tint(&mut self, base: &Handle<StandardMaterial>, color: Rgb, materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
        let key = (base.id(), MatKey::new(crate::look::Look::lit(color)));
        if let Some(h) = self.tints.get(&key) {
            return h.clone();
        }
        let [r, g, b] = key.1.color();
        let mut material = materials.get(base).cloned().unwrap_or_default();
        material.base_color = Color::srgb(r, g, b);
        let handle = materials.add(material);
        self.tints.insert(key, handle.clone());
        handle
    }
}

/// Mark model roots whose scene has spawned (rigged in `rig_models`).
pub(crate) fn mark_scene_ready(ready: On<WorldInstanceReady>, roots: Query<(), With<ModelRoot>>, mut commands: Commands) {
    if roots.contains(ready.entity) {
        commands.entity(ready.entity).try_insert(SceneReady);
    }
}

/// Wire up freshly spawned model scenes: give the animation player the
/// model's graph, hide every prop, tint the team material, then show it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn rig_models(
    mut commands: Commands,
    library: Res<ModelLibrary>,
    data: Res<Data>,
    players: Option<Res<Players>>,
    mut tints: ResMut<TeamTints>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<(Entity, &ModelRoot, Has<Corpse>), (With<SceneReady>, Without<Rig>)>,
    children: Query<&Children>,
    nodes: Query<(Option<&Name>, Has<AnimationPlayer>, Option<&MeshMaterial3d<StandardMaterial>>, Option<&Transform>, Has<AnimationTargetId>)>,
    owners: Query<&Owner>,
) {
    for (root, model_root, corpse) in &roots {
        let Some(loaded) = library.model(model_root.model).loaded() else { continue };
        let color = owners.get(model_root.unit).ok().zip(players.as_deref()).map(|(o, p)| owner_color(p, o.0));
        let mut rig = Rig::default();
        for e in children.iter_descendants(root) {
            let Ok((name, has_player, material, transform, animated)) = nodes.get(e) else { continue };
            if has_player && rig.player.is_none() {
                rig.player = Some(e);
            }
            if let Some(t) = transform.filter(|_| animated) {
                rig.bind_pose.push((e, *t));
            }
            if let Some(prop) = name.and_then(|n| classify(n.as_str(), |id| data.resource_id(id))) {
                rig.props.push((prop, e));
                commands.entity(e).try_insert(Visibility::Hidden);
            }
            if let (Some(material), Some(team), Some(color)) = (material, &loaded.team_material, color)
                && material.0 == *team
            {
                commands.entity(e).try_insert(MeshMaterial3d(tints.tint(team, color, &mut materials)));
            }
        }
        if let Some(player) = rig.player {
            commands.entity(player).try_insert((AnimationGraphHandle(loaded.graph.clone()), AnimationTransitions::new()));
        }
        let mut e = commands.entity(root);
        e.try_insert(rig);
        if !corpse {
            e.try_insert(Visibility::Inherited);
        }
    }
}

/// When a modelled unit dies its model is kept as a [`Corpse`]: it leaves
/// the unit (which af_core despawns at the end of the tick) where it was
/// last drawn. This runs inside the sim tick, so it only moves an existing
/// client entity and spawns nothing (see `af_core::sim_ids`).
pub(crate) fn keep_corpse(
    dead: On<Add, Dead>,
    mut commands: Commands,
    units: Query<&ModelUnit>,
    roots: Query<(&GlobalTransform, &InheritedVisibility)>,
) {
    let Ok(unit) = units.get(dead.entity) else { return };
    let Ok((at, seen)) = roots.get(unit.root) else { return };
    let transform = at.compute_transform();
    let visibility = if seen.get() { Visibility::Inherited } else { Visibility::Hidden };
    commands.entity(unit.root).try_remove::<ChildOf>().try_insert((
        transform,
        visibility,
        Corpse { age: 0.0, rest_y: transform.translation.y, started: false },
        crate::scene::RenderScene,
    ));
}
