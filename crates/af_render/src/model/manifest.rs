//! The manifest of an animated unit model and how to play it (the contract
//! is `docs/art/peasant.md`): `<name>.model.ron`, written by the art build,
//! names the glTF file, the model's height and every clip it ships. Which
//! units have one is the visual registry's business ([`crate::registry`]).
//!
//! Paths are asset paths, relative to the `assets/` folder.

use std::collections::BTreeMap;

use ron::extensions::Extensions;
use serde::Deserialize;

use super::clips::Clip;

/// One model and its clips, as the art build describes it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ModelManifest {
    /// The glTF file (asset path).
    pub model: String,
    /// The model's standing height at scale 1 (metres). The engine scales it
    /// uniformly so this matches the unit's box height.
    pub height: f32,
    /// Engine clip name (see [`Clip`]) -> what the file ships for it.
    #[serde(default)]
    pub clips: BTreeMap<String, ClipSpec>,
}

/// One clip of a manifest.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ClipSpec {
    /// The glTF animation's name, when it is not the clip's own.
    #[serde(default)]
    pub animation: Option<String>,
    /// Seconds.
    pub duration: f32,
    pub looping: bool,
    /// Ground speed (m/s, at the model's own `height`) that the clip's feet
    /// match at playback rate 1. Walk cycles.
    #[serde(default)]
    pub speed: Option<f32>,
    /// Seconds into the clip where the blow lands. Attack.
    #[serde(default)]
    pub hit: Option<f32>,
    /// How far ahead the work lands (metres, at the model's own `height`):
    /// the engine steps the model up until its target's surface is there.
    /// Work clips.
    #[serde(default)]
    pub reach: Option<f32>,
}

impl ClipSpec {
    /// The glTF animation this clip plays.
    pub fn animation_name<'a>(&'a self, clip: &'a str) -> &'a str {
        self.animation.as_deref().unwrap_or(clip)
    }
}

fn ron_options() -> ron::Options {
    // Optional fields may be written bare: `speed: 1.4`.
    ron::Options::default().with_default_extension(Extensions::IMPLICIT_SOME)
}

impl ModelManifest {
    /// Parse and check a manifest. Errors make the whole model unusable;
    /// see [`ModelManifest::warnings`] for what only loses a clip.
    pub fn parse(text: &str) -> Result<ModelManifest, String> {
        let m: ModelManifest = ron_options().from_str(text).map_err(|e| e.to_string())?;
        if m.model.trim().is_empty() {
            return Err("`model` is empty".into());
        }
        if !(m.height.is_finite() && m.height > 0.0) {
            return Err(format!("`height` must be > 0, got {}", m.height));
        }
        Ok(m)
    }

    /// Problems that cost a clip (or are ignored), one line each: clip
    /// names the engine does not know, bad timings, and loop flags that
    /// disagree with how the engine plays the clip.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (name, spec) in &self.clips {
            let Some(clip) = Clip::from_name(name) else {
                out.push(format!("clip `{name}`: no such engine clip (ignored)"));
                continue;
            };
            if let Err(e) = spec.check() {
                out.push(format!("clip `{name}`: {e} (clip unused)"));
                continue;
            }
            if spec.looping == clip.once() {
                let how = if clip.once() { "plays it once" } else { "loops it" };
                out.push(format!("clip `{name}`: looping is {}, but the engine always {how}", spec.looping));
            }
        }
        out
    }

    /// The usable spec of `clip`, if the manifest lists one.
    pub fn clip(&self, clip: Clip) -> Option<&ClipSpec> {
        self.clips.get(clip.name()).filter(|s| s.check().is_ok())
    }
}

impl ClipSpec {
    fn check(&self) -> Result<(), String> {
        if !(self.duration.is_finite() && self.duration > 0.0) {
            return Err(format!("duration must be > 0, got {}", self.duration));
        }
        if let Some(s) = self.speed.filter(|s| !(s.is_finite() && *s > 0.0)) {
            return Err(format!("speed must be > 0, got {s}"));
        }
        if let Some(h) = self.hit.filter(|h| !(0.0..=self.duration).contains(h)) {
            return Err(format!("hit {h} lies outside the clip (0..={})", self.duration));
        }
        if let Some(r) = self.reach.filter(|r| !(r.is_finite() && *r > 0.0)) {
            return Err(format!("reach must be > 0, got {r}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEASANT: &str = r#"
        (
            model: "models/peasant/peasant.glb",
            height: 1.65,
            clips: {
                "Walk": (duration: 1.08, looping: true, speed: 1.4), // m/s at rate 1
                "Attack": (duration: 1.0, looping: false, hit: 0.5),
                "Idle": (duration: 3.0, looping: true),
                "Death": (duration: 1.5, looping: false),
                "Chop": (duration: 1.6, looping: true, hit: 0.9667, reach: 1.3),
            },
        )
    "#;

    #[test]
    fn the_contract_example_parses() {
        let m = ModelManifest::parse(PEASANT).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.model, "models/peasant/peasant.glb");
        assert_eq!(m.height, 1.65);
        let walk = m.clip(Clip::Walk).expect("walk");
        assert_eq!((walk.duration, walk.looping, walk.speed, walk.hit), (1.08, true, Some(1.4), None));
        assert_eq!(walk.animation_name("Walk"), "Walk");
        assert_eq!(m.clip(Clip::Attack).and_then(|a| a.hit), Some(0.5));
        assert_eq!(m.clip(Clip::Chop).and_then(|c| c.reach), Some(1.3));
        assert!(m.clip(Clip::Mine).is_none());
        assert!(m.warnings().is_empty(), "{:?}", m.warnings());
    }

    #[test]
    fn a_clip_may_name_its_gltf_animation_and_options_may_be_explicit() {
        let m = ModelManifest::parse(r#"(model: "m.glb", height: 1.65, clips: { "Walk": (animation: Some("Walking"), duration: 1.08, looping: true, speed: Some(1.2)) })"#)
            .unwrap_or_else(|e| panic!("{e}"));
        let walk = m.clip(Clip::Walk).unwrap();
        assert_eq!(walk.animation_name("Walk"), "Walking");
        assert_eq!(walk.speed, Some(1.2));
    }

    #[test]
    fn broken_manifests_are_rejected_with_a_reason() {
        assert!(ModelManifest::parse("(height: 1.0)").unwrap_err().contains("model"));
        assert!(ModelManifest::parse(r#"(model: "", height: 1.0)"#).unwrap_err().contains("model"));
        assert!(ModelManifest::parse(r#"(model: "m.glb", height: 0.0)"#).unwrap_err().contains("height"));
        assert!(ModelManifest::parse(r#"(model: "m.glb", height: 1.0, clips: { "Walk": (duration: 1.0) })"#).is_err(), "looping is required");
    }

    #[test]
    fn bad_clips_cost_only_themselves() {
        let m = ModelManifest::parse(
            r#"(model: "m.glb", height: 1.6, clips: {
                "Idle": (duration: 2.0, looping: true),
                "Chopp": (duration: 1.6, looping: true),
                "Attack": (duration: 1.0, looping: false, hit: 1.5),
                "Walk": (duration: 0.0, looping: true),
                "Cheer": (duration: 2.0, looping: true),
                "Mine": (duration: 1.5, looping: true, reach: -1.0),
            })"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(m.clip(Clip::Idle).is_some());
        assert!(m.clip(Clip::Attack).is_none(), "hit outside the clip");
        assert!(m.clip(Clip::Walk).is_none(), "zero duration");
        assert!(m.clip(Clip::Cheer).is_some(), "a wrong loop flag is only a warning");
        assert!(m.clip(Clip::Mine).is_none(), "negative reach");
        let w = m.warnings().join("\n");
        for needle in ["`Chopp`: no such engine clip", "`Attack`: hit 1.5", "`Walk`: duration", "`Cheer`: looping is true, but the engine always plays it once", "`Mine`: reach"] {
            assert!(w.contains(needle), "{needle} in:\n{w}");
        }
    }
}
