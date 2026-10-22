//! The static models buildings, nodes and the land's decor are drawn with:
//! read from the visual registry's `buildings`, `nodes` and `decor` and the
//! manifests they name at startup, each variant (and stump) loaded through
//! the asset server and, once loaded, flattened into the meshes and
//! materials every instance shares ([`StaticMesh`]).

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use af_core::prelude::*;
use bevy::asset::RecursiveDependencyLoadState;
use bevy::camera::primitives::Aabb;
use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;

use super::manifest::{MeshSpec, StaticKind, StaticManifest};
use crate::assets::BOX_REFLECTANCE;
use crate::model::library::{TEAM_MATERIAL, fix_materials};
use crate::registry::{Entry, Registry};
use crate::terrain::decor::{Catalogue, DecorKind, DecorModel, DecorVariant};

/// Index of a model in the [`StaticLibrary`].
pub type StaticId = usize;

/// One mesh primitive of a static model: its mesh and material (shared by
/// every instance, so Bevy batches them) and where it sits in the model.
#[derive(Clone, Debug)]
pub struct MeshPart {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub transform: Transform,
    /// Uses the `Team` material: tinted with the owner's colour.
    pub team: bool,
}

/// A loaded variant (or stump): its parts and the bounds the manifest
/// gives for them (which leave a building's banner out).
#[derive(Clone, Debug)]
pub struct StaticMesh {
    pub parts: Vec<MeshPart>,
    pub min: Vec3,
    pub max: Vec3,
    /// The highest point of any part, the banner's included (at least
    /// `max.y`): how deep a fresh foundation is buried.
    pub top: f32,
}

impl StaticMesh {
    /// Parts within bounds `min..max` (nothing above them).
    pub fn new(parts: Vec<MeshPart>, min: Vec3, max: Vec3) -> StaticMesh {
        StaticMesh { parts, min, max, top: max.y }
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    /// From its base to its [`StaticMesh::top`], at scale 1.
    pub fn full_height(&self) -> f32 {
        self.top.max(self.max.y) - self.min.y
    }

    /// Some part takes the owner's colour.
    pub fn has_team(&self) -> bool {
        self.parts.iter().any(|p| p.team)
    }
}

/// One glTF file of a model.
#[derive(Clone, Debug)]
pub enum MeshState {
    Loading(Handle<Gltf>),
    Ready(Arc<StaticMesh>),
    /// Did not load (or holds nothing usable): never drawn.
    Failed,
}

impl MeshState {
    pub fn ready(&self) -> Option<&StaticMesh> {
        match self {
            MeshState::Ready(m) => Some(m),
            _ => None,
        }
    }
}

/// A model and the state of each of its files.
#[derive(Clone, Debug)]
pub struct StaticModel {
    /// The manifest's path, for messages.
    pub source: String,
    pub manifest: StaticManifest,
    /// One per manifest variant, in order.
    pub variants: Vec<MeshState>,
    pub stump: Option<MeshState>,
}

impl StaticModel {
    fn files(&mut self) -> impl Iterator<Item = (&MeshSpec, &mut MeshState)> {
        let stump = self.manifest.stump.as_ref().zip(self.stump.as_mut());
        self.manifest.variants.iter().zip(self.variants.iter_mut()).chain(stump)
    }

    /// Some file is still loading.
    pub fn loading(&self) -> bool {
        self.variants.iter().chain(&self.stump).any(|s| matches!(s, MeshState::Loading(_)))
    }

    /// Drawn: every file has finished (so the variant a position picks
    /// never changes later) and some variant loaded.
    pub fn ready(&self) -> bool {
        !self.loading() && self.variants.iter().any(|v| v.ready().is_some())
    }

    /// The variant drawn at `pos` (see [`super::place::variant_index`]); a
    /// variant that failed to load gives way to the next one that did.
    pub fn variant_at(&self, pos: Vec2) -> Option<(usize, &StaticMesh)> {
        if !self.ready() {
            return None;
        }
        let n = self.variants.len();
        let first = super::place::variant_index(pos, n);
        (0..n).map(|i| (first + i) % n).find_map(|i| self.variants[i].ready().map(|m| (i, m)))
    }

    pub fn variant(&self, index: usize) -> Option<&StaticMesh> {
        self.variants.get(index).and_then(MeshState::ready)
    }

    /// The loaded stump, if the manifest has one.
    pub fn stump(&self) -> Option<&StaticMesh> {
        self.stump.as_ref().and_then(MeshState::ready)
    }
}

/// Every static model and the building and node types (and the decor)
/// drawn with them. Inserted at startup (or by a test); changed when a
/// model finishes loading, which rebuilds the visuals of its buildings and
/// nodes.
#[derive(Resource, Clone, Debug, Default)]
pub struct StaticLibrary {
    /// By `BuildingTypeId` index.
    buildings: Vec<Option<StaticId>>,
    /// By `NodeTypeId` index.
    nodes: Vec<Option<StaticId>>,
    /// By `DecorKind` index.
    decor: Vec<Option<StaticId>>,
    models: Vec<StaticModel>,
}

/// What a static model draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticTarget {
    Building(BuildingTypeId),
    Node(NodeTypeId),
    Decor(DecorKind),
}

impl StaticLibrary {
    /// Add a model for `target` (one model may serve several types: models
    /// are told apart by `source`), with the state of each of its variants
    /// (in manifest order) and of its stump.
    pub fn insert(&mut self, target: StaticTarget, source: &str, manifest: StaticManifest, variants: Vec<MeshState>, stump: Option<MeshState>) -> StaticId {
        let id = match self.models.iter().position(|m| m.source == source) {
            Some(id) => id,
            None => {
                self.models.push(StaticModel { source: source.to_string(), manifest, variants, stump });
                self.models.len() - 1
            }
        };
        let (slots, index) = match target {
            StaticTarget::Building(k) => (&mut self.buildings, k.index()),
            StaticTarget::Node(k) => (&mut self.nodes, k.index()),
            StaticTarget::Decor(k) => (&mut self.decor, k.index()),
        };
        if slots.len() <= index {
            slots.resize(index + 1, None);
        }
        slots[index] = Some(id);
        id
    }

    pub fn model(&self, id: StaticId) -> &StaticModel {
        &self.models[id]
    }

    pub fn models(&self) -> &[StaticModel] {
        &self.models
    }

    /// Mutable access to a model (tests, tools).
    pub fn model_mut(&mut self, id: StaticId) -> &mut StaticModel {
        &mut self.models[id]
    }

    fn ready_in(&self, slots: &[Option<StaticId>], index: usize) -> Option<(StaticId, &StaticModel)> {
        let id = (*slots.get(index)?)?;
        let model = &self.models[id];
        model.ready().then_some((id, model))
    }

    /// The model buildings of `kind` are drawn with, once it is ready.
    pub fn building(&self, kind: BuildingTypeId) -> Option<(StaticId, &StaticModel)> {
        self.ready_in(&self.buildings, kind.index())
    }

    /// The model nodes of `kind` are drawn with, once it is ready.
    pub fn node(&self, kind: NodeTypeId) -> Option<(StaticId, &StaticModel)> {
        self.ready_in(&self.nodes, kind.index())
    }

    /// The decor model of `kind`, once it is ready.
    pub fn decor(&self, kind: DecorKind) -> Option<(StaticId, &StaticModel)> {
        self.ready_in(&self.decor, kind.index())
    }

    /// Some decor model is still loading (the land waits for them, so the
    /// decor is placed once, from every model there is).
    pub fn decor_loading(&self) -> bool {
        self.decor.iter().flatten().any(|id| self.models[*id].loading())
    }

    /// The decor there is to place: every ready decor model with its loaded
    /// variants (named after their files: `a.glb` is `a`), and for each
    /// kind the model and, per catalogue variant, its manifest variant.
    pub fn decor_catalogue(&self) -> (Catalogue, Vec<(DecorKind, StaticId, Vec<usize>)>) {
        let mut catalogue = Catalogue::default();
        let mut models = Vec::new();
        for kind in DecorKind::ALL {
            let Some((id, model)) = self.decor(kind) else { continue };
            let mut variants = Vec::new();
            let mut index = Vec::new();
            for (i, spec) in model.manifest.variants.iter().enumerate() {
                let Some(mesh) = model.variant(i) else { continue };
                let name = Path::new(&spec.model).file_stem().map_or_else(|| i.to_string(), |s| s.to_string_lossy().into_owned());
                variants.push(DecorVariant { name, min: mesh.min, max: mesh.max });
                index.push(i);
            }
            let size = model.manifest.size.unwrap_or_else(|| kind.size());
            catalogue.insert(kind, DecorModel { fit: model.manifest.fit, size, variants });
            models.push((kind, id, index));
        }
        (catalogue, models)
    }

    fn loading(&self) -> bool {
        self.models.iter().any(StaticModel::loading)
    }
}

/// The models the registry asks for, from the manifests that could be read.
#[derive(Debug, Default)]
pub struct StaticSources {
    /// (what it draws, manifest path, manifest).
    pub entries: Vec<(StaticTarget, String, StaticManifest)>,
    /// What was skipped or will be missing, one line each.
    pub warnings: Vec<String>,
}

/// Read the manifest of each building, node and decor model the registry
/// lists through `read` (asset path -> text), checking it against the
/// content (decor: the kinds the land places) and its registry section.
pub fn read_sources(data: &GameData, registry: &Registry, read: impl Fn(&str) -> Result<String, String>) -> StaticSources {
    let mut out = StaticSources::default();
    let sections: [(&BTreeMap<String, Entry>, StaticKind); 3] = [(&registry.buildings, StaticKind::Building), (&registry.nodes, StaticKind::Node), (&registry.decor, StaticKind::Decor)];
    for (section, kind) in sections {
        for (id, entry) in section {
            let target = match kind {
                StaticKind::Building => data.building_id(id).map(StaticTarget::Building),
                StaticKind::Node => data.node_id(id).map(StaticTarget::Node),
                StaticKind::Decor => DecorKind::parse(id).map(StaticTarget::Decor),
            };
            let Some(target) = target else {
                let what = match kind {
                    StaticKind::Node => "resource node",
                    StaticKind::Building => "building",
                    StaticKind::Decor => "decor the land places",
                };
                out.warnings.push(format!("{}: no {what} `{id}`", entry.file));
                continue;
            };
            let path = &entry.manifest;
            let parsed = read(path).and_then(|text| StaticManifest::parse(&text));
            match parsed {
                Ok((m, _)) if m.kind != kind => {
                    let fallback = if kind == StaticKind::Decor { "not placed" } else { "drawn as boxes" };
                    out.warnings.push(format!("{path} ({id}): a {:?} model listed under `{}`; {fallback}", m.kind, kind.section()));
                }
                Ok((m, warnings)) => {
                    out.warnings.extend(warnings.into_iter().map(|w| format!("{path}: {w}")));
                    out.entries.push((target, path.clone(), m));
                }
                Err(e) => out.warnings.push(format!("{path} ({id}): {e}; {}", if kind == StaticKind::Decor { "not placed" } else { "drawn as boxes" })),
            }
        }
    }
    out
}

/// The library of every static model the registry lists, their glTF files
/// loading from the asset folder `dir` (warnings are logged).
pub(crate) fn load_static_models(data: &GameData, registry: &Registry, dir: &Path, server: &AssetServer) -> StaticLibrary {
    let sources = read_sources(data, registry, |path| std::fs::read_to_string(dir.join(path)).map_err(|e| e.to_string()));
    for w in &sources.warnings {
        warn!("{w}");
    }
    let mut library = StaticLibrary::default();
    for (target, source, manifest) in sources.entries {
        let load = |spec: &MeshSpec| MeshState::Loading(server.load::<Gltf>(spec.model.clone()));
        let variants = manifest.variants.iter().map(load).collect();
        let stump = manifest.stump.as_ref().map(load);
        library.insert(target, &source, manifest, variants, stump);
    }
    library
}

/// Finish the files whose glTF (and everything in it) has loaded: flatten
/// their scenes into shared parts. Marks the library changed when a model
/// becomes ready or fails, which rebuilds the visuals that use it.
pub(crate) fn finish_static_loads(
    mut library: ResMut<StaticLibrary>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    worlds: Res<Assets<WorldAsset>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !library.loading() {
        return;
    }
    let mut changed = false;
    for model in library.bypass_change_detection().models.iter_mut() {
        let source = model.source.clone();
        let mut finished = false;
        for (spec, state) in model.files() {
            let MeshState::Loading(handle) = state else { continue };
            let next = match server.recursive_dependency_load_state(&*handle) {
                RecursiveDependencyLoadState::Loaded => {
                    let scene = gltfs.get(&*handle).and_then(|g| g.default_scene.clone().or_else(|| g.scenes.first().cloned()));
                    match scene.as_ref().and_then(|s| worlds.get(s)).ok_or_else(|| "the file has no scene".to_string()).and_then(|w| flatten(w, spec, &mut materials)) {
                        Ok((mesh, warnings)) => {
                            for w in warnings {
                                warn!("{source}: {}: {w}", spec.model);
                            }
                            MeshState::Ready(Arc::new(mesh))
                        }
                        Err(e) => {
                            warn!("{source}: {}: {e}; not drawn", spec.model);
                            MeshState::Failed
                        }
                    }
                }
                RecursiveDependencyLoadState::Failed(e) => {
                    warn!("{source}: {} failed to load ({e}); not drawn", spec.model);
                    MeshState::Failed
                }
                _ => continue,
            };
            *state = next;
            finished = true;
        }
        changed |= finished;
        if finished && !model.loading() {
            match model.ready() {
                true => info!("{source}: static model ready"),
                false => warn!("{source}: no variant loaded; drawn as boxes"),
            }
        }
    }
    if changed {
        library.set_changed();
    }
}

/// The node holding a building's team banner (a pole and a flag).
pub const BANNER_NODE: &str = "Team_banner";

/// How far (metres, plus a share of the size) the meshes' real bounds
/// (without the banner) may stray from the manifest's before it is
/// reported.
const BOUNDS_SLACK: (f32, f32) = (0.05, 0.05);

/// Flatten a loaded glTF scene into its mesh primitives, each with its
/// transform in the model (the scene's node transforms multiplied out), its
/// material (emission baked into the colour texture switched off, see
/// `fix_materials`, and the boxes' reflectance, see [`shade_like_boxes`])
/// and whether it is the `Team` material. Warns when the
/// meshes' bounds (the [`BANNER_NODE`] left out, as the manifest leaves it
/// out) disagree with the manifest's `spec`, which the engine fits by.
pub fn flatten(scene: &WorldAsset, spec: &MeshSpec, materials: &mut Assets<StandardMaterial>) -> Result<(StaticMesh, Vec<String>), String> {
    let world = &scene.world;
    let mut warnings = Vec::new();
    fix_materials(scene, materials);
    shade_like_boxes(scene, materials);
    let Some(mut meshes) = world.try_query::<(Entity, &Mesh3d)>() else {
        return Err("the file has no meshes".into());
    };
    let mut parts = Vec::new();
    let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
    let mut top = f32::MIN;
    let mut fallback: Option<Handle<StandardMaterial>> = None;
    for (e, mesh) in meshes.iter(world) {
        let chain = lineage(world, e);
        let transform = model_transform(world, &chain);
        // The banner may stand above the bounds the model is fitted by.
        let banner = chain.iter().any(|n| world.get::<Name>(*n).is_some_and(|n| n.as_str() == BANNER_NODE));
        let material = match world.get::<MeshMaterial3d<StandardMaterial>>(e) {
            Some(m) => m.0.clone(),
            // Only without bevy_pbr's glTF support (headless tools).
            None => fallback.get_or_insert_with(|| materials.add(StandardMaterial::default())).clone(),
        };
        let team = world.get::<GltfMaterialName>(e).is_some_and(|n| n.0 == TEAM_MATERIAL);
        if world.get::<SkinnedMesh>(e).is_some() && !warnings.iter().any(|w: &String| w.contains("skinned")) {
            warnings.push("a skinned mesh is drawn in its bind pose (the build should apply the pose)".into());
        }
        if let Some(aabb) = world.get::<Aabb>(e) {
            let (min, max) = (Vec3::from(aabb.min()), Vec3::from(aabb.max()));
            for i in 0..8 {
                let corner = Vec3::new(if i & 1 == 0 { min.x } else { max.x }, if i & 2 == 0 { min.y } else { max.y }, if i & 4 == 0 { min.z } else { max.z });
                let p = transform.transform_point(corner);
                top = top.max(p.y);
                if !banner {
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
        }
        parts.push(MeshPart { mesh: mesh.0.clone(), material, transform, team });
    }
    if parts.is_empty() {
        return Err("the file has no meshes".into());
    }
    let (min, max) = (spec.min(), spec.max());
    if lo.cmple(hi).all() {
        let slack = BOUNDS_SLACK.0 + BOUNDS_SLACK.1 * spec.size().max_element();
        if (lo - min).abs().max_element() > slack || (hi - max).abs().max_element() > slack {
            warnings.push(format!("the meshes span {lo:.2}..{hi:.2}, the manifest says {min:.2}..{max:.2} (fitted by the manifest)"));
        }
    }
    Ok((StaticMesh { parts, min, max, top: top.max(max.y) }, warnings))
}

/// Give every material of a scene the boxes' low reflectance
/// ([`BOX_REFLECTANCE`]), so models sit among boxes in the same light: the
/// default (4 %) greys dark colours (foliage, bark, rock) under the sky
/// light. The build keeps only base colour textures, so nothing else of the
/// source's surface is lost.
fn shade_like_boxes(scene: &WorldAsset, materials: &mut Assets<StandardMaterial>) {
    let Some(mut used) = scene.world.try_query::<&MeshMaterial3d<StandardMaterial>>() else { return };
    let ids: HashSet<AssetId<StandardMaterial>> = used.iter(&scene.world).map(|m| m.0.id()).collect();
    for id in ids {
        if let Some(mut m) = materials.get_mut(id) {
            m.reflectance = BOX_REFLECTANCE;
        }
    }
}

/// Scene entity `e` and its ancestors, root last.
fn lineage(world: &World, e: Entity) -> Vec<Entity> {
    let mut chain = vec![e];
    while let Some(parent) = world.get::<ChildOf>(chain[chain.len() - 1]).map(ChildOf::parent) {
        chain.push(parent);
    }
    chain
}

/// The transform of the scene entity whose [`lineage`] is `chain`
/// relative to the scene root: its own and every ancestor's multiplied out.
fn model_transform(world: &World, chain: &[Entity]) -> Transform {
    let global = chain.iter().rev().filter_map(|node| world.get::<Transform>(*node)).fold(GlobalTransform::IDENTITY, |g, t| g.mul_transform(*t));
    global.compute_transform()
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::test_data;
    use crate::statics::manifest::Fit;

    fn entry(manifest: &str, file: &str) -> Entry {
        Entry { manifest: manifest.to_string(), file: file.to_string() }
    }

    const PINE: &str = r#"(kind: Node, id: "pine", variants: [
        (model: "models/static/nodes/pine/a.glb", min: (-1.4, 0.0, -1.3), max: (1.4, 6.3, 1.3)),
        (model: "models/static/nodes/pine/b.glb", min: (-1.2, 0.0, -1.2), max: (1.2, 5.8, 1.2), triangles: 1502),
    ], stump: Some((model: "models/static/nodes/pine/stump.glb", min: (-0.3, 0.0, -0.3), max: (0.3, 0.5, 0.3))))"#;
    const HOUSE: &str = r#"(kind: Building, id: "house", banner: true, variants: [(model: "h.glb", min: (-2.0, 0.0, -2.0), max: (2.0, 3.0, 2.0))])"#;

    #[test]
    fn the_registry_names_the_manifests_to_read() {
        let data = test_data();
        let read = |path: &str| match path {
            "pine.model.ron" => Ok(PINE.to_string()),
            "house.model.ron" => Ok(HOUSE.to_string()),
            "bad.model.ron" => Ok("(kind: Node)".to_string()),
            _ => Err("not found".to_string()),
        };
        let mut registry = Registry::default();
        registry.nodes.insert("pine".into(), entry("pine.model.ron", "visuals/static.ron"));
        registry.nodes.insert("oak".into(), entry("missing.model.ron", "visuals/static.ron"));
        registry.nodes.insert("palm".into(), entry("bad.model.ron", "visuals/static.ron"));
        registry.nodes.insert("baobab".into(), entry("pine.model.ron", "visuals/static.ron"));
        registry.nodes.insert("berry_bush".into(), entry("house.model.ron", "visuals/static.ron"));
        registry.buildings.insert("house".into(), entry("house.model.ron", "visuals/static.ron"));
        registry.buildings.insert("skyscraper".into(), entry("house.model.ron", "visuals/static.ron"));
        let s = read_sources(&data, &registry, read);
        let targets: Vec<_> = s.entries.iter().map(|(t, p, m)| (*t, p.as_str(), m.id.as_str())).collect();
        assert_eq!(
            targets,
            [(StaticTarget::Building(data.building_id("house").unwrap()), "house.model.ron", "house"), (StaticTarget::Node(data.node_id("pine").unwrap()), "pine.model.ron", "pine")]
        );
        let w = s.warnings.join("\n");
        for needle in [
            "missing.model.ron (oak): not found",
            "bad.model.ron (palm)",
            "visuals/static.ron: no resource node `baobab`",
            "visuals/static.ron: no building `skyscraper`",
            "house.model.ron (berry_bush): a Building model listed under `nodes`",
        ] {
            assert!(w.contains(needle), "{needle} in:\n{w}");
        }
    }

    #[test]
    fn decor_comes_from_its_own_section_and_becomes_a_catalogue() {
        let data = test_data();
        const PEAK: &str = r#"(kind: Decor, id: "mountain_peak", fit: Height, size: (13.0, 8.5, 13.0), variants: [
            (model: "models/static/decor/mountain_peak/a.glb", min: (-6.0, 0.0, -6.0), max: (6.0, 9.0, 6.0)),
            (model: "models/static/decor/mountain_peak/c.glb", min: (-5.0, 0.0, -5.0), max: (5.0, 8.0, 5.0)),
        ])"#;
        let read = |path: &str| match path {
            "peak.model.ron" => Ok(PEAK.to_string()),
            "pine.model.ron" => Ok(PINE.to_string()),
            _ => Err("not found".to_string()),
        };
        let mut registry = Registry::default();
        registry.decor.insert("mountain_peak".into(), entry("peak.model.ron", "visuals/static.ron"));
        registry.decor.insert("volcano".into(), entry("peak.model.ron", "visuals/static.ron"));
        registry.decor.insert("reeds".into(), entry("pine.model.ron", "visuals/static.ron"));
        let s = read_sources(&data, &registry, read);
        assert_eq!(s.entries.iter().map(|(t, _, _)| *t).collect::<Vec<_>>(), [StaticTarget::Decor(DecorKind::MountainPeak)]);
        let w = s.warnings.join("\n");
        for needle in ["visuals/static.ron: no decor the land places `volcano`", "pine.model.ron (reeds): a Node model listed under `decor`; not placed"] {
            assert!(w.contains(needle), "{needle} in:\n{w}");
        }
        // Loaded (variant c failed): the catalogue has what there is, named by file.
        let (_, path, m) = s.entries.into_iter().next().unwrap();
        let mut lib = StaticLibrary::default();
        let id = lib.insert(StaticTarget::Decor(DecorKind::MountainPeak), &path, m.clone(), vec![MeshState::Loading(Handle::default()), MeshState::Loading(Handle::default())], None);
        assert!(lib.decor_loading() && lib.decor_catalogue().0.is_empty());
        lib.model_mut(id).variants = vec![MeshState::Failed, mesh(Vec3::new(-5.0, 0.0, -5.0), Vec3::new(5.0, 8.0, 5.0))];
        assert!(!lib.decor_loading());
        let (catalogue, models) = lib.decor_catalogue();
        let peak = catalogue.get(DecorKind::MountainPeak).expect("ready");
        assert_eq!(peak.variants.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["c"]);
        assert_eq!((peak.fit, peak.size), (Fit::Height, Vec3::new(13.0, 8.5, 13.0)));
        assert_eq!(models, [(DecorKind::MountainPeak, id, vec![1])], "catalogue variant 0 is manifest variant 1");
        assert!(lib.node(data.node_id("pine").unwrap()).is_none(), "decor serves no node");
    }

    fn manifest(text: &str) -> StaticManifest {
        StaticManifest::parse(text).unwrap_or_else(|e| panic!("{e}")).0
    }

    fn mesh(min: Vec3, max: Vec3) -> MeshState {
        MeshState::Ready(Arc::new(StaticMesh::new(Vec::new(), min, max)))
    }

    #[test]
    fn a_model_is_ready_once_every_file_has_finished() {
        let data = test_data();
        let (pine, oak) = (data.node_id("pine").unwrap(), data.node_id("oak").unwrap());
        let m = manifest(PINE);
        let mut lib = StaticLibrary::default();
        let loading = || MeshState::Loading(Handle::default());
        let id = lib.insert(StaticTarget::Node(pine), "pine.model.ron", m.clone(), vec![loading(), loading()], Some(loading()));
        assert_eq!(lib.insert(StaticTarget::Node(oak), "pine.model.ron", m.clone(), Vec::new(), None), id, "one model per manifest");
        assert!(lib.node(pine).is_none() && lib.loading());
        let model = lib.model_mut(id);
        model.variants[0] = mesh(Vec3::splat(-1.0), Vec3::splat(1.0));
        assert!(!model.ready(), "variant b and the stump are still loading");
        model.variants[1] = MeshState::Failed;
        model.stump = Some(MeshState::Failed);
        assert!(model.ready() && model.stump().is_none());
        let (got, model) = lib.node(oak).expect("ready for every type it serves");
        assert_eq!(got, id);
        // Wherever variant b would be drawn, a gives way.
        for x in 0..20 {
            assert_eq!(model.variant_at(Vec2::new(x as f32 * 3.1, 7.0)).map(|(i, _)| i), Some(0));
        }
        assert!(lib.building(data.building_id("house").unwrap()).is_none());
        // Nothing loaded: never ready.
        let mut none = StaticLibrary::default();
        none.insert(StaticTarget::Node(pine), "p", m, vec![MeshState::Failed, MeshState::Failed], None);
        assert!(none.node(pine).is_none() && !none.loading());
    }

    #[test]
    fn variants_are_picked_by_position() {
        let m = manifest(PINE);
        let model = StaticModel { source: "p".into(), manifest: m, variants: vec![mesh(Vec3::ZERO, Vec3::ONE), mesh(Vec3::ZERO, Vec3::splat(2.0))], stump: None };
        let mut seen = [false; 2];
        for x in 0..30 {
            let p = Vec2::new(x as f32 * 1.3, 4.0);
            let (i, _) = model.variant_at(p).unwrap();
            assert_eq!(i, crate::statics::place::variant_index(p, 2));
            seen[i] = true;
        }
        assert_eq!(seen, [true; 2]);
        assert_eq!(model.manifest.fit, Fit::Height);
    }

    #[test]
    fn a_scene_flattens_into_shared_parts_with_their_transforms() {
        let mut materials = Assets::<StandardMaterial>::default();
        let mut meshes = Assets::<Mesh>::default();
        let bark = materials.add(StandardMaterial::default());
        let team = materials.add(StandardMaterial::default());
        let glowing = materials.add(StandardMaterial { emissive: LinearRgba::WHITE, emissive_texture: Some(Handle::default()), ..default() });
        let (trunk, crown, flag) = (meshes.add(Cuboid::default()), meshes.add(Sphere::default()), meshes.add(Cuboid::default()));
        let mut world = World::new();
        let root = world.spawn(Transform::default()).id();
        let node = world.spawn((Transform::from_xyz(0.0, 1.0, 0.0).with_scale(Vec3::splat(2.0)), ChildOf(root))).id();
        world.spawn((Mesh3d(trunk.clone()), MeshMaterial3d(bark.clone()), Transform::default(), Aabb::from_min_max(Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5)), ChildOf(node)));
        world.spawn((Mesh3d(crown.clone()), MeshMaterial3d(glowing.clone()), Transform::from_xyz(0.0, 1.0, 0.0), ChildOf(node)));
        let banner = world.spawn((Name::new(BANNER_NODE), Transform::from_xyz(0.5, 0.0, 0.5), ChildOf(root))).id();
        let above = Aabb::from_min_max(Vec3::new(-0.1, 0.0, -0.1), Vec3::new(0.1, 3.5, 0.1));
        world.spawn((Mesh3d(flag), MeshMaterial3d(team.clone()), GltfMaterialName(TEAM_MATERIAL.into()), Transform::default(), above, ChildOf(banner)));
        world.spawn((Mesh3d(meshes.add(Cuboid::default())), Transform::default(), ChildOf(root)));
        let spec = MeshSpec { model: "m.glb".into(), min: [-1.0, 0.0, -1.0], max: [1.0, 2.0, 1.0], triangles: 0 };
        let (flat, warnings) = flatten(&WorldAsset::new(world), &spec, &mut materials).unwrap_or_else(|e| panic!("{e}"));
        assert!(warnings.is_empty(), "the trunk's bounds match; the banner may rise above them: {warnings:?}");
        assert!((flat.top - 3.5).abs() < 1e-5 && (flat.full_height() - 3.5).abs() < 1e-5, "the banner's top counts: {}", flat.top);
        assert_eq!(flat.parts.len(), 4);
        assert_eq!((flat.min, flat.max), (spec.min(), spec.max()));
        let part = |m: &Handle<Mesh>| flat.parts.iter().find(|p| p.mesh == *m).unwrap();
        assert_eq!(part(&trunk).transform, Transform::from_xyz(0.0, 1.0, 0.0).with_scale(Vec3::splat(2.0)));
        assert_eq!(part(&trunk).material, bark);
        let crown_at = part(&crown).transform.translation;
        assert!((crown_at - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-5, "the node's scale applies to the child's offset: {crown_at}");
        assert!(flat.parts.iter().filter(|p| p.team).count() == 1 && flat.has_team());
        assert!(!part(&trunk).team && !part(&crown).team);
        assert!(materials.get(&glowing).unwrap().emissive_texture.is_none(), "baked emission switched off");
        for m in [&bark, &glowing, &team] {
            assert_eq!(materials.get(m).unwrap().reflectance, BOX_REFLECTANCE, "shaded like the boxes");
        }
        assert!(flat.parts.iter().filter(|p| ![bark.clone(), glowing.clone(), team.clone()].contains(&p.material)).count() == 1, "a mesh without a material gets a plain one");
    }

    #[test]
    fn scenes_without_meshes_fail_and_stray_bounds_are_reported() {
        let mut materials = Assets::<StandardMaterial>::default();
        let spec = MeshSpec { model: "m.glb".into(), min: [-1.0, 0.0, -1.0], max: [1.0, 2.0, 1.0], triangles: 0 };
        assert!(flatten(&WorldAsset::new(World::new()), &spec, &mut materials).is_err());
        let mut world = World::new();
        world.spawn(Transform::default());
        assert!(flatten(&WorldAsset::new(world), &spec, &mut materials).is_err(), "nodes but no meshes");
        let mut world = World::new();
        world.spawn((Mesh3d(Handle::default()), Transform::default(), Aabb::from_min_max(Vec3::new(-3.0, 0.0, -1.0), Vec3::new(3.0, 2.0, 1.0))));
        let (_, warnings) = flatten(&WorldAsset::new(world), &spec, &mut materials).unwrap();
        assert!(warnings.len() == 1 && warnings[0].contains("the manifest says"), "{warnings:?}");
    }
}
