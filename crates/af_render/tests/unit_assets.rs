//! Every unit model in the registry (`assets/visuals/units.ron`, written by
//! `art/units/build_unit.py`) against what the engine expects of it: a game
//! unit of that id, a manifest without warnings, and a GLB (read with the
//! `gltf` crate Bevy's loader uses) that ships each listed clip at its
//! listed length. The peasant's own contract is `peasant_asset.rs`.

use std::path::PathBuf;

use af_core::testing::test_data;
use af_render::model::{Clip, ModelManifest};
use af_render::registry::parse_registry;
use bevy::gltf::gltf::Gltf;

fn asset(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets").join(path)
}

#[test]
fn every_registered_unit_model_loads_with_its_clips() {
    let data = test_data();
    let registry = parse_registry(&std::fs::read_to_string(asset("visuals/units.ron")).unwrap()).unwrap();
    assert!(registry.units.len() > 1);
    for (unit, path) in &registry.units {
        assert!(data.unit_id(unit).is_some(), "{unit}: no such unit in assets/data");
        let text = std::fs::read_to_string(asset(path)).unwrap_or_else(|e| panic!("{unit}: {path}: {e}"));
        let m = ModelManifest::parse(&text).unwrap_or_else(|e| panic!("{unit}: {path}: {e}"));
        assert!(m.warnings().is_empty(), "{unit}: {:#?}", m.warnings());
        let bytes = std::fs::read(asset(&m.model)).unwrap_or_else(|e| panic!("{unit}: {}: {e}", m.model));
        let gltf = Gltf::from_slice(&bytes).unwrap_or_else(|e| panic!("{unit}: {}: {e}", m.model));
        for (name, spec) in &m.clips {
            let anim = gltf.animations().find(|a| a.name() == Some(spec.animation_name(name)));
            let anim = anim.unwrap_or_else(|| panic!("{unit}: no animation {}", spec.animation_name(name)));
            let end = anim
                .samplers()
                .filter_map(|s| s.input().max().and_then(|v| v.get(0).and_then(|x| x.as_f64())))
                .fold(0.0_f64, f64::max);
            assert!((end - spec.duration as f64).abs() < 1e-3, "{unit} {name}: {end} s in the file, {} s in the manifest", spec.duration);
        }
        // A model that fights has its blow's time.
        if let Some(attack) = m.clip(Clip::Attack) {
            assert!(attack.hit.is_some(), "{unit}: Attack has no hit");
        }
    }
}
