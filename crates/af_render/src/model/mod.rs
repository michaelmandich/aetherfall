//! Units drawn with animated glTF models instead of boxes.
//!
//! The visual registry's `units` ([`crate::registry`], hand-written in
//! `assets/visuals/units.ron`) names a manifest per unit type (contract:
//! `docs/art/peasant.md`); at startup `crate::registry::load_visuals` reads
//! them and loads each model through the asset server. A unit whose type has a
//! loaded model gets the model's scene in its visual root instead of its
//! body boxes, scaled to its box height and yawed with it like the boxes;
//! its selection outline, bars, shield shell and fog hiding stay as they
//! are. Until a model has loaded (or when it is missing or fails to load)
//! its units are boxes; they are rebuilt when it arrives.
//!
//! Per frame (`RenderStep::Animate`) each modelled unit's [`Animator`]
//! picks its clip from the shared `Activity`, its velocity, cargo, weapon
//! windups and hit points, crossfading through `AnimationTransitions`, and
//! the props (`Tool_*`, `Load_*` nodes) it holds are toggled. A worker's
//! model turns toward what it works on and steps up to it ([`stance`]).
//! A unit that dies leaves its model behind as a [`rig::Corpse`].
//!
//! | where | system | what |
//! |---|---|---|
//! | `Startup` | `crate::registry::load_visuals` | registry + manifests, start the glTF loads |
//! | `Update` (any state) | `finish_model_loads` | graph, clip table, lighting fix, team material |
//! | observer | `mark_scene_ready` | a model scene has spawned |
//! | `RenderStep::Build` | `rig_models` (after `attach_units`) | animation player, props hidden, team tint |
//! | `RenderStep::Animate` | `drive_models`, `drive_corpses` | clips, rates, props, the stance at work; corpses |
//! | observer (sim tick) | `keep_corpse` | a modelled unit died: its model stays as a corpse |

pub mod animator;
pub mod clips;
pub(crate) mod drive;
pub mod library;
pub mod manifest;
pub mod props;
pub mod rig;
pub mod stance;

pub use animator::Animator;
pub use clips::{Clip, ClipInfo, ClipTable};
pub use library::{LoadedModel, ModelId, ModelLibrary, ModelState, VisualsRoot};
pub use manifest::{ClipSpec, ModelManifest};
pub use rig::{Corpse, ModelRoot, ModelUnit, Rig, SceneReady};
