//! The manifest of a static model (`<id>.model.ron`, written by the static
//! art build; contract: `docs/art/static-models.md`): what it is, how the
//! engine fits it to the game's box (decor: to the size it is drawn at),
//! and its variants (each a glTF file with its bounds) and optional stump.

use bevy::prelude::*;
use ron::extensions::Extensions;
use serde::Deserialize;

/// What a static model is drawn for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub enum StaticKind {
    Node,
    Building,
    /// Landscape scenery: not a game entity; placed from the terrain
    /// (`crate::terrain::decor`).
    Decor,
}

impl StaticKind {
    /// The registry section that lists models of this kind.
    pub fn section(self) -> &'static str {
        match self {
            StaticKind::Node => "nodes",
            StaticKind::Building => "buildings",
            StaticKind::Decor => "decor",
        }
    }
}

/// How the engine sizes a model to the game's box (`w x h x d`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub enum Fit {
    /// Scaled per axis to exactly the box.
    Box,
    /// One scale: the model's larger horizontal side matches the box's.
    Footprint,
    /// One scale: the model's height matches the box's.
    Height,
}

/// Nodes that are trees by default: they fit by height and fall when felled.
pub const TREE_IDS: [&str; 3] = ["pine", "oak", "palm"];

/// The contract's default fit for a model of `kind` and `id`.
pub fn default_fit(kind: StaticKind, id: &str) -> Fit {
    match kind {
        StaticKind::Building => Fit::Box,
        StaticKind::Node if TREE_IDS.contains(&id) => Fit::Height,
        StaticKind::Node | StaticKind::Decor => Fit::Footprint,
    }
}

/// The contract's default for `random_yaw`: nodes and decor turn,
/// buildings do not.
pub fn default_random_yaw(kind: StaticKind) -> bool {
    kind != StaticKind::Building
}

/// One glTF file of a model and the bounds of its geometry (metres, as the
/// file stands: base at y = 0, centred on x/z, front toward +Z).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct MeshSpec {
    /// The glTF file (asset path).
    pub model: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Triangle count after reduction (informational).
    #[serde(default)]
    pub triangles: u32,
}

impl MeshSpec {
    pub fn min(&self) -> Vec3 {
        Vec3::from(self.min)
    }

    pub fn max(&self) -> Vec3 {
        Vec3::from(self.max)
    }

    /// Width, height and depth of the bounds.
    pub fn size(&self) -> Vec3 {
        self.max() - self.min()
    }

    fn check(&self) -> Result<(), String> {
        if self.model.trim().is_empty() {
            return Err("`model` is empty".into());
        }
        if !(self.min().is_finite() && self.max().is_finite()) {
            return Err(format!("bounds {:?}..{:?} are not finite", self.min, self.max));
        }
        if self.size().min_element() < MIN_EXTENT {
            return Err(format!("bounds {:?}..{:?} are empty on some axis", self.min, self.max));
        }
        Ok(())
    }
}

/// The smallest extent (metres) bounds may have on any axis.
const MIN_EXTENT: f32 = 1e-3;

/// The manifest as written; optional settings take the contract's defaults.
#[derive(Deserialize)]
struct RawManifest {
    kind: StaticKind,
    id: String,
    #[serde(default)]
    fit: Option<Fit>,
    #[serde(default)]
    random_yaw: Option<bool>,
    #[serde(default)]
    size: Option<[f32; 3]>,
    variants: Vec<MeshSpec>,
    #[serde(default)]
    stump: Option<MeshSpec>,
    #[serde(default)]
    banner: bool,
}

/// A checked manifest, its settings resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct StaticManifest {
    pub kind: StaticKind,
    /// The game id it was built for (`pine`, `house`...).
    pub id: String,
    pub fit: Fit,
    /// Each instance turns by a position hash (visual only).
    pub random_yaw: bool,
    /// At least one.
    pub variants: Vec<MeshSpec>,
    /// What a felled tree leaves.
    pub stump: Option<MeshSpec>,
    /// The model has a `Team_banner` (buildings).
    pub banner: bool,
    /// Decor: the size (w, h, d metres) it is drawn at before a
    /// placement's own scaling; `None` leaves it to the engine's default
    /// for its id. Buildings and nodes are sized by the game's box.
    pub size: Option<Vec3>,
}

impl StaticManifest {
    /// Parse and check a manifest. Errors make the model unusable; the
    /// warnings list what was dropped (a broken variant or stump).
    pub fn parse(text: &str) -> Result<(StaticManifest, Vec<String>), String> {
        let options = ron::Options::default().with_default_extension(Extensions::IMPLICIT_SOME);
        let raw: RawManifest = options.from_str(text).map_err(|e| e.to_string())?;
        if raw.id.trim().is_empty() {
            return Err("`id` is empty".into());
        }
        let mut warnings = Vec::new();
        let mut variants = Vec::with_capacity(raw.variants.len());
        for (i, v) in raw.variants.into_iter().enumerate() {
            match v.check() {
                Ok(()) => variants.push(v),
                Err(e) => warnings.push(format!("variant {i} ({}): {e} (unused)", v.model)),
            }
        }
        if variants.is_empty() {
            let why = if warnings.is_empty() { "no variants".to_string() } else { format!("no usable variant: {}", warnings.join("; ")) };
            return Err(why);
        }
        let stump = raw.stump.and_then(|s| match s.check() {
            Ok(()) => Some(s),
            Err(e) => {
                warnings.push(format!("stump ({}): {e} (unused)", s.model));
                None
            }
        });
        if stump.is_some() && raw.kind != StaticKind::Node {
            warnings.push(format!("a {} model has no use for a stump (ignored)", raw.kind.section()));
        }
        let size = raw.size.map(Vec3::from).filter(|s| {
            let ok = s.is_finite() && s.min_element() > 0.0;
            if !ok {
                warnings.push(format!("size {s} is not a size (ignored)"));
            }
            ok
        });
        if size.is_some() && raw.kind != StaticKind::Decor {
            warnings.push("only decor has a drawn size; the game's box sizes the rest (ignored)".into());
        }
        let manifest = StaticManifest {
            fit: raw.fit.unwrap_or_else(|| default_fit(raw.kind, &raw.id)),
            random_yaw: raw.random_yaw.unwrap_or_else(|| default_random_yaw(raw.kind)),
            stump: stump.filter(|_| raw.kind == StaticKind::Node),
            size: size.filter(|_| raw.kind == StaticKind::Decor),
            kind: raw.kind,
            id: raw.id,
            variants,
            banner: raw.banner,
        };
        Ok((manifest, warnings))
    }

    /// A tree: it falls over when its wood runs out (and does not shrink as
    /// it is gathered). Nodes with a stump, and the contract's tree ids.
    pub fn falls(&self) -> bool {
        self.kind == StaticKind::Node && (self.stump.is_some() || TREE_IDS.contains(&self.id.as_str()))
    }

    /// It shrinks toward 70 % as it is gathered (nodes that do not fall).
    pub fn shrinks(&self) -> bool {
        self.kind == StaticKind::Node && !self.falls()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The contract's example (`docs/art/static-models.md`), stump bounds filled in.
    const PINE: &str = r#"
        (
            kind: Node,                       // Node | Building
            id: "pine",
            fit: Height,
            random_yaw: true,
            variants: [
                (model: "models/static/nodes/pine/a.glb", min: (-1.4, 0.0, -1.3), max: (1.4, 6.3, 1.3), triangles: 1480),
                (model: "models/static/nodes/pine/b.glb", min: (-1.2, 0.0, -1.2), max: (1.2, 5.8, 1.2), triangles: 1502),
            ],
            stump: Some((model: "models/static/nodes/pine/stump.glb", min: (-0.3, 0.0, -0.3), max: (0.3, 0.5, 0.3), triangles: 120)),
            banner: false,
        )
    "#;

    #[test]
    fn the_contract_example_parses() {
        let (m, warnings) = StaticManifest::parse(PINE).unwrap_or_else(|e| panic!("{e}"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!((m.kind, m.id.as_str(), m.fit, m.random_yaw, m.banner), (StaticKind::Node, "pine", Fit::Height, true, false));
        assert_eq!(m.variants.len(), 2);
        assert_eq!(m.variants[0].model, "models/static/nodes/pine/a.glb");
        assert_eq!(m.variants[0].min(), Vec3::new(-1.4, 0.0, -1.3));
        assert_eq!(m.variants[1].size(), Vec3::new(2.4, 5.8, 2.4));
        assert_eq!(m.variants[1].triangles, 1502);
        assert_eq!(m.stump.as_ref().map(|s| s.model.as_str()), Some("models/static/nodes/pine/stump.glb"));
        assert!(m.falls() && !m.shrinks());
    }

    #[test]
    fn settings_default_per_the_contract() {
        let parse = |kind: &str, id: &str| {
            let text = format!(r#"(kind: {kind}, id: "{id}", variants: [(model: "m.glb", min: (-1.0, 0.0, -1.0), max: (1.0, 2.0, 1.0))])"#);
            StaticManifest::parse(&text).unwrap_or_else(|e| panic!("{e}")).0
        };
        let oak = parse("Node", "oak");
        assert_eq!((oak.fit, oak.random_yaw, oak.stump.is_none()), (Fit::Height, true, true));
        assert!(oak.falls(), "a tree without a stump still falls");
        let rock = parse("Node", "stone_outcrop");
        assert_eq!((rock.fit, rock.random_yaw), (Fit::Footprint, true));
        assert!(!rock.falls() && rock.shrinks());
        let house = parse("Building", "house");
        assert_eq!((house.fit, house.random_yaw, house.banner), (Fit::Box, false, false));
        assert!(!house.falls() && !house.shrinks());
        // Explicit settings win, and a stump makes any node a faller.
        let (bush, _) = StaticManifest::parse(
            r#"(kind: Node, id: "berry_bush", fit: Box, random_yaw: false, banner: true,
                variants: [(model: "m.glb", min: (-1.0, 0.0, -1.0), max: (1.0, 2.0, 1.0))],
                stump: (model: "s.glb", min: (-0.2, 0.0, -0.2), max: (0.2, 0.3, 0.2)))"#,
        )
        .unwrap();
        assert_eq!((bush.fit, bush.random_yaw, bush.banner), (Fit::Box, false, true));
        assert!(bush.falls(), "a stump, written bare (implicit Some)");
    }

    #[test]
    fn broken_variants_and_stumps_cost_only_themselves() {
        let (m, warnings) = StaticManifest::parse(
            r#"(kind: Node, id: "pine", variants: [
                (model: "a.glb", min: (-1.0, 0.0, -1.0), max: (1.0, 6.0, 1.0)),
                (model: "", min: (-1.0, 0.0, -1.0), max: (1.0, 6.0, 1.0)),
                (model: "flat.glb", min: (-1.0, 0.0, -1.0), max: (1.0, 0.0, 1.0)),
                (model: "inside_out.glb", min: (1.0, 0.0, 1.0), max: (-1.0, 6.0, -1.0)),
            ], stump: Some((model: "s.glb", min: (0.0, 0.0, 0.0), max: (0.0, 0.0, 0.0))))"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.variants.iter().map(|v| v.model.as_str()).collect::<Vec<_>>(), ["a.glb"]);
        assert!(m.stump.is_none());
        let w = warnings.join("\n");
        for needle in ["variant 1 (): `model` is empty", "variant 2 (flat.glb): bounds", "variant 3 (inside_out.glb)", "stump (s.glb)"] {
            assert!(w.contains(needle), "{needle} in:\n{w}");
        }
        assert!(m.falls(), "still a tree: pine");
    }

    #[test]
    fn unusable_manifests_are_rejected_with_a_reason() {
        let err = |text: &str| StaticManifest::parse(text).err().unwrap_or_else(|| panic!("accepted: {text}"));
        assert!(err(r#"(kind: Node, id: "pine", variants: [])"#).contains("no variants"));
        assert!(err(r#"(kind: Node, id: "pine", variants: [(model: "", min: (0.0, 0.0, 0.0), max: (1.0, 1.0, 1.0))])"#).contains("no usable variant"));
        assert!(err(r#"(kind: Node, id: "", variants: [(model: "a.glb", min: (0.0, 0.0, 0.0), max: (1.0, 1.0, 1.0))])"#).contains("id"));
        assert!(err(r#"(kind: Tree, id: "pine", variants: [(model: "a.glb", min: (0.0, 0.0, 0.0), max: (1.0, 1.0, 1.0))])"#).contains("Tree"));
        assert!(err(r#"(kind: Node, id: "pine", fit: Squash, variants: [(model: "a.glb", min: (0.0, 0.0, 0.0), max: (1.0, 1.0, 1.0))])"#).contains("Squash"));
        assert!(err(r#"(kind: Node, id: "pine")"#).contains("variants"));
        err("(");
    }

    #[test]
    fn decor_has_its_drawn_size_and_turns_by_default() {
        // As the static build writes it (art/static/lib/manifest.py).
        let (m, warnings) = StaticManifest::parse(
            r#"(
                kind: Decor,
                id: "mountain_peak",
                fit: Height,
                random_yaw: true,
                size: (13.000, 8.500, 13.000),
                variants: [
                    (model: "models/static/decor/mountain_peak/a.glb", min: (-6.0, 0.0, -6.5), max: (6.0, 9.0, 6.5), triangles: 7990),
                ],
                stump: None,
                banner: false,
            )"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!((m.kind, m.fit, m.size), (StaticKind::Decor, Fit::Height, Some(Vec3::new(13.0, 8.5, 13.0))));
        assert!(!m.falls() && !m.shrinks(), "decor is no node");
        assert_eq!(StaticKind::Decor.section(), "decor");
        let (bare, _) = StaticManifest::parse(r#"(kind: Decor, id: "reeds", variants: [(model: "r.glb", min: (-0.5, 0.0, -0.5), max: (0.5, 1.4, 0.5))])"#).unwrap();
        assert_eq!((bare.fit, bare.random_yaw, bare.size), (Fit::Footprint, true, None));
        // A size on anything else, or a broken one, is dropped with a warning.
        let (house, warnings) = StaticManifest::parse(r#"(kind: Building, id: "house", size: (1.0, 1.0, 1.0), variants: [(model: "h.glb", min: (-1.0, 0.0, -1.0), max: (1.0, 1.0, 1.0))])"#).unwrap();
        assert!(house.size.is_none() && warnings.iter().any(|w| w.contains("only decor")), "{warnings:?}");
        let (flat, warnings) = StaticManifest::parse(r#"(kind: Decor, id: "reeds", size: (1.0, 0.0, 1.0), variants: [(model: "r.glb", min: (-0.5, 0.0, -0.5), max: (0.5, 1.4, 0.5))])"#).unwrap();
        assert!(flat.size.is_none() && warnings.len() == 1, "{warnings:?}");
    }

    #[test]
    fn a_building_never_keeps_a_stump_and_extra_fields_are_tolerated() {
        let (m, warnings) = StaticManifest::parse(
            r#"(kind: Building, id: "house", variants: [(model: "h.glb", min: (-2.0, 0.0, -2.0), max: (2.0, 3.0, 2.0))],
                stump: (model: "s.glb", min: (-0.2, 0.0, -0.2), max: (0.2, 0.3, 0.2)), banner: true, source: "art/static/source/buildings/house.glb")"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(m.stump.is_none() && m.banner && !m.falls());
        assert!(warnings.iter().any(|w| w.contains("stump")), "{warnings:?}");
    }
}
