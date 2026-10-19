//! The models units are drawn with: read from [`REGISTRY`] and the
//! manifests it names at startup, loaded through the asset server, and
//! turned into what every instance shares once loaded (scene, animation
//! graph, clip table, the team material).

use std::path::{Path, PathBuf};

use af_core::prelude::*;
use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use super::clips::{Clip, ClipTable};
use super::manifest::{ModelManifest, REGISTRY, parse_registry};

/// Index of a model in the [`ModelLibrary`].
pub type ModelId = usize;

/// Name of the material the engine tints with the owner's colour.
pub const TEAM_MATERIAL: &str = "Team";

/// Where the asset folder is when the app does not say ([`VisualsRoot`]):
/// the folder holding the content (`assets/data`), as the game finds it.
pub fn default_assets_dir() -> Option<PathBuf> {
    af_data::find_data_dir().and_then(|d| d.parent().map(Path::to_path_buf))
}

/// The asset folder the registry and manifests are read from. Optional:
/// without it [`default_assets_dir`] is used. It must be the asset
/// server's folder too, since the models load through it.
#[derive(Resource, Clone, Debug)]
pub struct VisualsRoot(pub PathBuf);

/// What one model is made of once loaded; shared by all its instances.
#[derive(Clone, Debug)]
pub struct LoadedModel {
    pub scene: Handle<WorldAsset>,
    pub graph: Handle<AnimationGraph>,
    /// Graph node of each clip the model ships.
    pub nodes: [Option<AnimationNodeIndex>; Clip::COUNT],
    pub clips: ClipTable,
    /// The material the engine tints per owner ([`TEAM_MATERIAL`]).
    pub team_material: Option<Handle<StandardMaterial>>,
}

impl LoadedModel {
    pub fn node(&self, clip: Clip) -> Option<AnimationNodeIndex> {
        self.nodes[clip.index()]
    }
}

#[derive(Clone, Debug)]
pub enum ModelState {
    Loading(Handle<Gltf>),
    Ready(Box<LoadedModel>),
    /// Did not load: its units keep their boxes.
    Failed,
}

#[derive(Clone, Debug)]
pub struct Model {
    /// The manifest's path, for messages.
    pub source: String,
    pub manifest: ModelManifest,
    pub state: ModelState,
}

impl Model {
    pub fn loaded(&self) -> Option<&LoadedModel> {
        match &self.state {
            ModelState::Ready(m) => Some(m),
            _ => None,
        }
    }

    /// The model's display scale on a unit box `height` metres tall.
    pub fn scale_for(&self, height: f32) -> f32 {
        height / self.manifest.height
    }
}

/// Every model and the unit types drawn with them. Inserted at startup
/// (or by a test); changed when a model finishes loading, which rebuilds
/// the visuals of its units.
#[derive(Resource, Clone, Debug, Default)]
pub struct ModelLibrary {
    /// By `UnitTypeId` index.
    by_unit: Vec<Option<ModelId>>,
    models: Vec<Model>,
}

impl ModelLibrary {
    /// Add a model for units of `kind` (one model may serve several types:
    /// models are told apart by `source`).
    pub fn insert(&mut self, kind: UnitTypeId, source: &str, manifest: ModelManifest, state: ModelState) -> ModelId {
        let id = match self.models.iter().position(|m| m.source == source) {
            Some(id) => id,
            None => {
                self.models.push(Model { source: source.to_string(), manifest, state });
                self.models.len() - 1
            }
        };
        if self.by_unit.len() <= kind.index() {
            self.by_unit.resize(kind.index() + 1, None);
        }
        self.by_unit[kind.index()] = Some(id);
        id
    }

    pub fn model(&self, id: ModelId) -> &Model {
        &self.models[id]
    }

    pub fn models(&self) -> &[Model] {
        &self.models
    }

    /// The model units of `kind` are drawn with, once it is loaded.
    pub fn ready(&self, kind: UnitTypeId) -> Option<(ModelId, &Model, &LoadedModel)> {
        let id = (*self.by_unit.get(kind.index())?)?;
        let model = &self.models[id];
        model.loaded().map(|l| (id, model, l))
    }

    fn loading(&self) -> bool {
        self.models.iter().any(|m| matches!(m.state, ModelState::Loading(_)))
    }
}

/// The models the registry asks for, from the manifests that could be read.
#[derive(Debug, Default)]
pub struct ModelSources {
    /// (unit type, manifest path, manifest).
    pub entries: Vec<(UnitTypeId, String, ModelManifest)>,
    /// What was skipped or will be missing, one line each.
    pub warnings: Vec<String>,
}

/// Read the registry text (`None`: there is no registry) and each manifest
/// it names through `read` (asset path -> text).
pub fn read_sources(data: &GameData, registry: Option<&str>, read: impl Fn(&str) -> Result<String, String>) -> ModelSources {
    let mut out = ModelSources::default();
    let Some(text) = registry else { return out };
    let registry = match parse_registry(text) {
        Ok(r) => r,
        Err(e) => {
            out.warnings.push(format!("{REGISTRY}: {e}; every unit is drawn as boxes"));
            return out;
        }
    };
    for (unit, path) in registry {
        let Some(kind) = data.unit_id(&unit) else {
            out.warnings.push(format!("{REGISTRY}: no unit `{unit}`"));
            continue;
        };
        let manifest = read(&path).and_then(|text| ModelManifest::parse(&text));
        match manifest {
            Ok(m) => {
                out.warnings.extend(m.warnings().into_iter().map(|w| format!("{path}: {w}")));
                out.entries.push((kind, path, m));
            }
            Err(e) => out.warnings.push(format!("{path} ({unit}): {e}; drawn as boxes")),
        }
    }
    out
}

/// Startup: read the registry and manifests and start loading the models.
pub(crate) fn load_models(mut commands: Commands, data: Res<Data>, server: Res<AssetServer>, root: Option<Res<VisualsRoot>>) {
    let mut library = ModelLibrary::default();
    let Some(dir) = root.map(|r| r.0.clone()).or_else(default_assets_dir) else {
        warn!("no assets folder found: every unit is drawn as boxes");
        commands.insert_resource(library);
        return;
    };
    let registry = std::fs::read_to_string(dir.join(REGISTRY)).ok();
    let sources = read_sources(&data, registry.as_deref(), |path| std::fs::read_to_string(dir.join(path)).map_err(|e| e.to_string()));
    for w in &sources.warnings {
        warn!("{w}");
    }
    for (kind, source, manifest) in sources.entries {
        let handle = server.load::<Gltf>(manifest.model.clone());
        library.insert(kind, &source, manifest, ModelState::Loading(handle));
    }
    commands.insert_resource(library);
}

/// Finish models whose glTF file (and everything in it) has loaded: the
/// scene, one animation graph with every clip the manifest lists, the
/// lighting fix and the team material.
pub(crate) fn finish_model_loads(
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    worlds: Res<Assets<WorldAsset>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    if !library.loading() {
        return;
    }
    let mut changed = false;
    for model in library.bypass_change_detection().models.iter_mut() {
        let ModelState::Loading(handle) = &model.state else { continue };
        let next = match server.recursive_dependency_load_state(handle) {
            bevy::asset::RecursiveDependencyLoadState::Loaded => {
                match gltfs.get(handle).ok_or_else(|| "loaded but missing".to_string()).and_then(|gltf| finish(model, gltf, &worlds, &mut materials, &mut graphs)) {
                    Ok(loaded) => {
                        info!("{}: model ready", model.source);
                        ModelState::Ready(Box::new(loaded))
                    }
                    Err(e) => {
                        warn!("{}: {e}; drawn as boxes", model.source);
                        ModelState::Failed
                    }
                }
            }
            bevy::asset::RecursiveDependencyLoadState::Failed(e) => {
                warn!("{}: {} failed to load ({e}); drawn as boxes", model.source, model.manifest.model);
                ModelState::Failed
            }
            _ => continue,
        };
        model.state = next;
        changed = true;
    }
    if changed {
        library.set_changed();
    }
}

fn finish(
    model: &Model,
    gltf: &Gltf,
    worlds: &Assets<WorldAsset>,
    materials: &mut Assets<StandardMaterial>,
    graphs: &mut Assets<AnimationGraph>,
) -> Result<LoadedModel, String> {
    let scene = gltf.default_scene.clone().or_else(|| gltf.scenes.first().cloned()).ok_or("the file has no scene")?;
    let mut graph = AnimationGraph::new();
    let mut nodes = [None; Clip::COUNT];
    for clip in Clip::ALL {
        let Some(spec) = model.manifest.clip(clip) else { continue };
        let name = spec.animation_name(clip.name());
        match gltf.named_animations.get(name) {
            Some(handle) => nodes[clip.index()] = Some(graph.add_clip(handle.clone(), 1.0, graph.root)),
            None => warn!("{}: clip {} has no animation `{name}` in {}", model.source, clip.name(), model.manifest.model),
        }
    }
    let clips = ClipTable::from_manifest(&model.manifest, |c| nodes[c.index()].is_some());
    let team_material = fix_materials(worlds.get(&scene).ok_or("its scene did not load")?, materials);
    Ok(LoadedModel { scene, graph: graphs.add(graph), nodes, clips, team_material })
}

/// Light the model like everything else: Meshy exports reuse the colour
/// texture as an emissive map, which makes the whole model glow, so a
/// material with an emissive texture has its emission switched off.
/// Emission without a texture (a glowing prop) is kept. Returns the
/// [`TEAM_MATERIAL`].
fn fix_materials(scene: &WorldAsset, materials: &mut Assets<StandardMaterial>) -> Option<Handle<StandardMaterial>> {
    let mut query = scene.world.try_query::<(&MeshMaterial3d<StandardMaterial>, Option<&GltfMaterialName>)>()?;
    let mut seen = HashSet::new();
    let mut team = None;
    for (material, name) in query.iter(&scene.world) {
        if name.is_some_and(|n| n.0 == TEAM_MATERIAL) {
            team = Some(material.0.clone());
        }
        if !seen.insert(material.0.id()) {
            continue;
        }
        if materials.get(&material.0).is_some_and(|m| m.emissive_texture.is_some())
            && let Some(mut m) = materials.get_mut(&material.0)
        {
            m.emissive = LinearRgba::BLACK;
            m.emissive_texture = None;
        }
    }
    team
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::test_data;
    use bevy::animation::graph::AnimationNodeType;

    const MANIFEST: &str = r#"(model: "models/peasant/peasant.glb", height: 1.65, clips: { "Walk": (duration: 1.08, looping: true, speed: 1.4) })"#;

    #[test]
    fn the_registry_names_the_manifests_to_read() {
        let data = test_data();
        let read = |path: &str| match path {
            "models/peasant/peasant.model.ron" => Ok(MANIFEST.to_string()),
            "models/bad.model.ron" => Ok("(height: 2.0)".to_string()),
            _ => Err("not found".to_string()),
        };
        let registry = r#"{
            "peasant": "models/peasant/peasant.model.ron",
            "spearman": "models/missing.model.ron",
            "militia": "models/bad.model.ron",
            "dragon": "models/peasant/peasant.model.ron",
        }"#;
        let s = read_sources(&data, Some(registry), read);
        assert_eq!(s.entries.len(), 1);
        let (kind, path, manifest) = &s.entries[0];
        assert_eq!(*kind, data.unit_id("peasant").unwrap());
        assert_eq!(path, "models/peasant/peasant.model.ron");
        assert_eq!(manifest.height, 1.65);
        let w = s.warnings.join("\n");
        for needle in ["models/missing.model.ron (spearman): not found", "models/bad.model.ron (militia)", "no unit `dragon`"] {
            assert!(w.contains(needle), "{needle} in:\n{w}");
        }
        assert!(read_sources(&data, None, read).entries.is_empty(), "no registry, no models");
        let broken = read_sources(&data, Some("{"), read);
        assert!(broken.entries.is_empty() && broken.warnings[0].contains(REGISTRY));
    }

    /// A glTF file as the loader leaves it: one scene, named animations.
    fn gltf(scene: Handle<WorldAsset>, animations: &[(&str, Handle<AnimationClip>)]) -> Gltf {
        Gltf {
            scenes: vec![scene.clone()],
            named_scenes: Default::default(),
            meshes: Vec::new(),
            named_meshes: Default::default(),
            materials: Vec::new(),
            named_materials: Default::default(),
            nodes: Vec::new(),
            named_nodes: Default::default(),
            skins: Vec::new(),
            named_skins: Default::default(),
            default_scene: Some(scene),
            animations: animations.iter().map(|(_, h)| h.clone()).collect(),
            named_animations: animations.iter().map(|(n, h)| (Box::from(*n), h.clone())).collect(),
            source: None,
        }
    }

    #[test]
    fn a_loaded_file_becomes_a_graph_of_the_listed_clips() {
        let mut clips = Assets::<AnimationClip>::default();
        let mut worlds = Assets::<WorldAsset>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut graphs = Assets::<AnimationGraph>::default();
        let scene = worlds.add(WorldAsset::new(World::new()));
        let (walking, running, extra) = (clips.add(AnimationClip::default()), clips.add(AnimationClip::default()), clips.add(AnimationClip::default()));
        let file = gltf(scene.clone(), &[("Walking", walking.clone()), ("Running", running), ("Armature|clip0|baselayer", extra)]);
        let manifest = ModelManifest::parse(
            r#"(model: "m.glb", height: 1.65, clips: {
                "Walk": (animation: "Walking", duration: 1.08, looping: true, speed: 1.2),
                "Idle": (duration: 3.0, looping: true),
            })"#,
        )
        .unwrap();
        let model = Model { source: "m.model.ron".into(), manifest, state: ModelState::Loading(Handle::default()) };
        let loaded = finish(&model, &file, &worlds, &mut materials, &mut graphs).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(loaded.scene, scene);
        assert!(loaded.node(Clip::Walk).is_some(), "Walk plays the file's `Walking`");
        assert!(loaded.node(Clip::Idle).is_none(), "listed, but not in the file");
        assert!(loaded.node(Clip::Run).is_none(), "in the file, but not listed");
        assert_eq!(loaded.clips.resolve(Clip::CarryWalk), Some(Clip::Walk));
        assert_eq!(loaded.clips.resolve(Clip::Idle), None);
        let graph = graphs.get(&loaded.graph).expect("graph");
        let node = graph.get(loaded.node(Clip::Walk).unwrap()).expect("node");
        assert!(matches!(&node.node_type, AnimationNodeType::Clip(h) if *h == walking));
        // A file without a scene is no model.
        let empty = Gltf { scenes: Vec::new(), default_scene: None, ..gltf(Handle::default(), &[]) };
        assert!(finish(&model, &empty, &worlds, &mut materials, &mut graphs).is_err());
    }

    #[test]
    fn baked_emission_is_switched_off_but_glowing_props_keep_theirs() {
        let mut materials = Assets::<StandardMaterial>::default();
        let texture = Handle::<Image>::default();
        let skin = materials.add(StandardMaterial { emissive: LinearRgba::WHITE, emissive_texture: Some(texture.clone()), base_color_texture: Some(texture), ..default() });
        let glow = materials.add(StandardMaterial { emissive: LinearRgba::rgb(0.8, 0.4, 1.0), ..default() });
        let team = materials.add(StandardMaterial::default());
        let mut world = World::new();
        world.spawn((MeshMaterial3d(skin.clone()), GltfMaterialName("Material_1".into())));
        world.spawn((MeshMaterial3d(skin.clone()), GltfMaterialName("Material_1".into())));
        world.spawn((MeshMaterial3d(glow.clone()), GltfMaterialName("Aether".into())));
        world.spawn((MeshMaterial3d(team.clone()), GltfMaterialName(TEAM_MATERIAL.into())));
        assert_eq!(fix_materials(&WorldAsset::new(world), &mut materials), Some(team));
        let skin = materials.get(&skin).unwrap();
        assert!(skin.emissive == LinearRgba::BLACK && skin.emissive_texture.is_none() && skin.base_color_texture.is_some());
        assert_eq!(materials.get(&glow).unwrap().emissive, LinearRgba::rgb(0.8, 0.4, 1.0));
        assert_eq!(fix_materials(&WorldAsset::new(World::new()), &mut materials), None, "no meshes, no team material");
    }

    #[test]
    fn the_library_maps_unit_types_to_models_once_loaded() {
        let data = test_data();
        let manifest = ModelManifest::parse(MANIFEST).unwrap();
        let (peasant, militia, spear) = (data.unit_id("peasant").unwrap(), data.unit_id("militia").unwrap(), data.unit_id("spearman").unwrap());
        let mut lib = ModelLibrary::default();
        let a = lib.insert(peasant, "p.model.ron", manifest.clone(), ModelState::Loading(Handle::default()));
        let b = lib.insert(militia, "p.model.ron", manifest.clone(), ModelState::Failed);
        assert_eq!(a, b, "one model per manifest");
        assert_eq!(lib.models().len(), 1);
        assert!(lib.ready(peasant).is_none(), "still loading");
        assert!(lib.ready(spear).is_none(), "no model");
        let clips = ClipTable::from_manifest(&manifest, |_| true);
        let loaded = LoadedModel { scene: Handle::default(), graph: Handle::default(), nodes: [None; Clip::COUNT], clips, team_material: None };
        lib.models[a].state = ModelState::Ready(Box::new(loaded));
        let (id, model, _) = lib.ready(militia).expect("ready");
        assert_eq!(id, a);
        assert!((model.scale_for(1.2) - 1.2 / 1.65).abs() < 1e-6);
    }
}
