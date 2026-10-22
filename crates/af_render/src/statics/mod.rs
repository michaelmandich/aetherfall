//! Buildings and resource nodes drawn with static glTF models instead of
//! boxes (contract: `docs/art/static-models.md`; the pipeline that makes
//! them: `art/static/`).
//!
//! The visual registry's `buildings` and `nodes` ([`crate::registry`]) name
//! a manifest per type; at startup every variant and stump it lists starts
//! loading, and once a model's files have all finished its scenes are
//! flattened into shared meshes and materials ([`library::StaticMesh`]), so
//! all instances batch. A building or node whose type has a ready model is
//! drawn with it in place of its body boxes (foundations keep their pad and
//! scaffold, buildings their weapon boxes and shield shell); until then, or
//! when its model is missing or broken, it is boxes, rebuilt when the model
//! arrives. Selection outlines, bars, fog and memory are drawn as for boxes.
//!
//! Per instance ([`place`]): the model is fitted to the type's box
//! ([`manifest::Fit`]), its variant (and a turn, for `random_yaw`) picked by
//! a hash of its position, its `Team` material tinted with the owner's
//! colour, and a foundation's model sunk by `(1 - progress) x height` so it
//! rises as it is built. Nothing runs per frame for a model at rest: a
//! foundation moves when its progress changes, a node shrinks when its
//! amount (or what the local team knows of it) changes.
//!
//! Trees ([`manifest::StaticManifest::falls`]) do not shrink: when one's
//! wood runs out its model is detached and falls over away from the worker
//! who felled it, then sinks, leaving its stump ([`fall`]).
//!
//! | where | system | what |
//! |---|---|---|
//! | `Startup` | `crate::registry::load_visuals` | registry + manifests, start the glTF loads |
//! | `Update` (any state) | `finish_static_loads` | flatten loaded scenes; the library changes |
//! | `RenderStep::Build` | `attach_buildings`, `attach_nodes` | model instead of boxes; rebuilt on upgrade, completion, a model ready |
//! | `RenderStep::Animate` | `animate_parts`, `shrink_nodes` | a foundation's model rising; nodes shrinking |
//! | `RenderStep::Animate` | `drive_falls`, `sink_stumps` | felled trees falling; their stumps |
//! | `RenderStep::Fog` | `update_fog_memories` | remembered dead buildings and nodes, with their models |
//! | observer (sim tick) | `fell_tree` | a tree's wood ran out: its model starts falling |

pub mod fall;
pub mod library;
pub mod manifest;
pub mod place;

use af_core::prelude::*;
use bevy::prelude::*;

pub use fall::{FallingTree, Remnants, Stump};
pub use library::{MeshPart, MeshState, StaticId, StaticLibrary, StaticMesh, StaticModel, StaticTarget};
pub use manifest::{Fit, MeshSpec, StaticKind, StaticManifest};

use crate::assets::{BoxPart, Painter};
use crate::model::rig::TeamTints;
use place::{base_centre, fit_transform, hashed_yaw};

/// A building or node drawn with a static model (on the sim entity, next
/// to its `BoxVisual`; removed when it goes back to boxes).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct StaticVisual {
    pub model: StaticId,
    pub variant: usize,
    /// The model instance ([`StaticInstance`]) under the visual root.
    pub instance: Entity,
    /// The instance's transform at rest (built) in the visual root's frame.
    pub rest: Transform,
    /// How high its top stands above the ground its entity is drawn on
    /// (metres; a building's banner included): how far a fresh
    /// foundation's model is sunk, so none of it shows at 0 %.
    pub height: f32,
    /// It shrinks as it is gathered (nodes that are not trees).
    pub shrinks: bool,
}

/// The root of a static model instance; its mesh parts are its children.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct StaticInstance;

/// Where `mesh` (a variant of `model`) stands, at rest, on a box of `size`
/// whose base centre is at `pos`: fitted, and turned by the position hash
/// when the model turns at random. Buildings and nodes only stand on
/// buildable ground, which is drawn flat at the ground their entity is
/// drawn on, so the model stands right there.
pub fn rest_transform(model: &StaticModel, mesh: &StaticMesh, size: BoxSize, pos: Vec2) -> Transform {
    let yaw = if model.manifest.random_yaw { hashed_yaw(pos) } else { 0.0 };
    fit_transform(model.manifest.fit, mesh.min, mesh.max, Vec3::from(size), yaw)
}

/// Where a tree's `stump` stands, in the same frame as the tree's `rest`
/// (where its variant `tree` stands): on the same spot, as large and turned
/// as the tree.
pub fn stump_transform(rest: &Transform, tree: &StaticMesh, stump: &StaticMesh) -> Transform {
    let spot = rest.transform_point(base_centre(tree.min, tree.max));
    Transform { translation: spot - rest.rotation * (rest.scale * base_centre(stump.min, stump.max)), ..*rest }
}

/// Spawn an instance of `mesh` at `transform` with `extra` components (a
/// `ChildOf` to hang it under a visual root): a [`StaticInstance`] root and
/// one child per part, sharing the model's meshes and materials. `team`
/// tints the `Team` material with the owner's colour (`None`: left white).
pub(crate) fn spawn_instance(
    commands: &mut Commands,
    painter: &mut Painter,
    tints: &mut TeamTints,
    mesh: &StaticMesh,
    transform: Transform,
    team: Option<Rgb>,
    extra: impl Bundle,
) -> Entity {
    let root = commands.spawn((transform, Visibility::Inherited, BoxPart, StaticInstance, extra)).id();
    for part in &mesh.parts {
        let material = match team.filter(|_| part.team) {
            Some(color) => painter.team_tint(tints, &part.material, color),
            None => part.material.clone(),
        };
        commands.spawn((Mesh3d(part.mesh.clone()), MeshMaterial3d(material), part.transform, Visibility::Inherited, BoxPart, ChildOf(root)));
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(min: Vec3, max: Vec3) -> StaticMesh {
        StaticMesh::new(Vec::new(), min, max)
    }

    #[test]
    fn a_stump_stands_where_its_tree_stood_as_large_and_turned() {
        // A tree off-centre in its own file, fitted, turned and sunk; a stump
        // off-centre in its file too.
        let tree = mesh(Vec3::new(1.0, 0.0, -2.0), Vec3::new(3.0, 5.0, 0.0));
        let stump = mesh(Vec3::new(-0.2, 0.1, 0.4), Vec3::new(0.4, 0.6, 1.0));
        let rest = place::sunk(fit_transform(Fit::Height, tree.min, tree.max, Vec3::new(0.9, 6.0, 0.9), 0.7), 0.25);
        let at = stump_transform(&rest, &tree, &stump);
        assert_eq!((at.rotation, at.scale), (rest.rotation, rest.scale));
        let tree_base = rest.transform_point(base_centre(tree.min, tree.max));
        let stump_base = at.transform_point(base_centre(stump.min, stump.max));
        assert!((tree_base - Vec3::new(0.0, -0.25, 0.0)).length() < 1e-5, "{tree_base}");
        assert!((stump_base - tree_base).length() < 1e-5, "{stump_base} vs {tree_base}");
    }
}
