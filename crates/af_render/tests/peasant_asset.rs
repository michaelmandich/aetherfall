//! The shipped peasant model (`assets/models/peasant/`, written by
//! `art/peasant/build_peasant.py`) against what the engine expects of it
//! (contract: `docs/art/peasant.md`). The GLB is read with the same `gltf`
//! crate Bevy's loader uses, so a broken export fails here, not in game.

use std::path::PathBuf;

use af_core::testing::test_data;
use af_render::model::manifest::parse_registry;
use af_render::model::props::{Prop, Tool, classify};
use af_render::model::{Clip, ModelManifest};
use bevy::gltf::gltf::Gltf;

const MANIFEST: &str = "models/peasant/peasant.model.ron";

fn asset(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets").join(path)
}

fn manifest() -> ModelManifest {
    let text = std::fs::read_to_string(asset(MANIFEST)).unwrap_or_else(|e| panic!("{MANIFEST}: {e}"));
    ModelManifest::parse(&text).unwrap_or_else(|e| panic!("{MANIFEST}: {e}"))
}

fn model(m: &ModelManifest) -> Gltf {
    let bytes = std::fs::read(asset(&m.model)).unwrap_or_else(|e| panic!("{}: {e}", m.model));
    Gltf::from_slice(&bytes).unwrap_or_else(|e| panic!("{}: {e}", m.model))
}

/// Clips the peasant ships: all of them but the healers' `Heal`.
fn peasant_clips() -> impl Iterator<Item = Clip> {
    Clip::ALL.into_iter().filter(|c| *c != Clip::Heal)
}

#[test]
fn the_registry_names_the_peasant_manifest_and_it_lists_every_clip() {
    let registry = std::fs::read_to_string(asset("visuals/units.ron")).unwrap();
    let registry = parse_registry(&registry).unwrap();
    assert_eq!(registry.get("peasant").map(String::as_str), Some(MANIFEST));

    let m = manifest();
    assert!(m.warnings().is_empty(), "{:#?}", m.warnings());
    assert!((1.5..1.8).contains(&m.height), "height {}", m.height);
    for clip in peasant_clips() {
        let spec = m.clip(clip).unwrap_or_else(|| panic!("{clip:?} missing from {MANIFEST}"));
        assert_eq!(spec.looping, !clip.once(), "{clip:?} loop flag");
        assert_eq!(spec.animation_name(clip.name()), clip.name());
    }
    // Locomotion clips carry the ground speed their feet match, the attack
    // its hit time (the weapon's windup).
    for clip in [Clip::Walk, Clip::Run, Clip::CarryWalk] {
        assert!(m.clip(clip).and_then(|s| s.speed).is_some(), "{clip:?} has no speed");
    }
    let attack = m.clip(Clip::Attack).unwrap();
    assert_eq!((attack.duration, attack.hit), (1.0, Some(0.5)));
}

#[test]
fn the_glb_has_every_clip_at_its_manifest_length() {
    let m = manifest();
    let gltf = model(&m);
    let joints: Vec<_> = gltf.skins().flat_map(|s| s.joints().map(|j| j.index()).collect::<Vec<_>>()).collect();
    assert!(joints.len() >= 23, "{} joints", joints.len());
    for clip in peasant_clips() {
        let spec = m.clip(clip).unwrap();
        let anim = gltf.animations().find(|a| a.name() == Some(clip.name()));
        let anim = anim.unwrap_or_else(|| panic!("no animation {}", clip.name()));
        let end = anim
            .samplers()
            .filter_map(|s| s.input().max().and_then(|v| v.get(0).and_then(|x| x.as_f64())))
            .fold(0.0_f64, f64::max);
        assert!((end - spec.duration as f64).abs() < 1e-3, "{}: {end} s in the file, {} s in the manifest", clip.name(), spec.duration);
        // Every joint is animated, so a clip never inherits a bone from the
        // one before it.
        for j in &joints {
            assert!(anim.channels().any(|c| c.target().node().index() == *j), "{} leaves joint {j} unanimated", clip.name());
        }
    }
}

#[test]
fn the_glb_names_a_prop_for_every_role_and_resource_and_a_team_sash() {
    let data = test_data();
    let gltf = model(&manifest());
    let names: Vec<&str> = gltf.nodes().filter_map(|n| n.name()).collect();
    let props: Vec<Prop> = names.iter().filter_map(|n| classify(n, |id| data.resource_id(id))).collect();
    assert!(!props.contains(&Prop::Unknown), "a prop node the engine cannot place: {names:?}");
    for tool in Tool::ALL {
        assert!(props.contains(&Prop::Tool(tool)), "no Tool_{} node", tool.role());
    }
    for r in data.resource_ids() {
        assert!(props.contains(&Prop::Load(r)), "no Load_{} node", data.resources[r.0 as usize].id);
    }
    assert!(names.contains(&"Team_sash"));
    assert!(gltf.materials().any(|m| m.name() == Some("Team")), "the sash's Team material");
}

#[test]
fn only_the_aether_crystals_glow() {
    let gltf = model(&manifest());
    for m in gltf.materials() {
        let glows = m.emissive_texture().is_some() || m.emissive_factor().iter().any(|c| *c > 0.0);
        let name = m.name().unwrap_or("?");
        assert!(!glows || name == "PropAether", "{name} is emissive");
    }
    assert_eq!(gltf.images().count(), 1, "one texture: the body's base colour");
}
