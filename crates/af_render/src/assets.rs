//! The shared cube mesh (and the few marker shapes), the material cache,
//! and the [`Painter`] system param every drawing system uses to turn
//! [`Part`]s into entities.

use bevy::ecs::system::SystemParam;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use af_core::prelude::Rgb;

use crate::entities::BoxVisual;
use crate::look::{Look, MatKey, Shade};
use crate::model::rig::TeamTints;
use crate::parts::Part;

/// `Shade::Glow` boxes light themselves: their lit (base) colour is dimmed
/// to `GLOW_BASE` of itself (linear) and their colour times
/// `GLOW_STRENGTH` is emitted, so shaded faces glow too and the colour
/// stays saturated instead of washing out to pastel in the sun.
const GLOW_BASE: f32 = 0.45;
const GLOW_STRENGTH: f32 = 1.1;

/// The 1x1x1 cube every box is drawn with, the click markers' shapes
/// ([`crate::markers`]), and one material per [`MatKey`]. Inserted at
/// startup; materials are created on first use and reused.
#[derive(Resource)]
pub struct BoxAssets {
    pub cube: Handle<Mesh>,
    /// An arrow one metre long pointing along +Z: a square shaft and a
    /// four-sided head ([`arrow_mesh`]).
    pub arrow: Handle<Mesh>,
    /// A flat ring of outer radius 1 lying in the ground plane, facing up.
    pub ring: Handle<Mesh>,
    /// A flat disc of radius 1 lying in the ground plane, facing up.
    pub disc: Handle<Mesh>,
    /// A ball one metre across: unlit, it reads as a soft round blot from
    /// any side (the placement slam's dust).
    pub puff: Handle<Mesh>,
    materials: HashMap<MatKey, Handle<StandardMaterial>>,
}

impl BoxAssets {
    /// Number of distinct materials created so far.
    pub fn material_count(&self) -> usize {
        self.materials.len()
    }
}

/// Inner radius of [`BoxAssets::ring`] (its outer radius is 1).
pub const RING_INNER: f32 = 0.8;

pub(crate) fn init_box_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    // 2D shapes lie in the XY plane facing +Z; turned to face up (+Y).
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    commands.insert_resource(BoxAssets {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        arrow: meshes.add(arrow_mesh()),
        ring: meshes.add(Mesh::from(Annulus::new(RING_INNER, 1.0).mesh().resolution(40)).rotated_by(flat)),
        disc: meshes.add(Mesh::from(Circle::new(1.0).mesh().resolution(32)).rotated_by(flat)),
        puff: meshes.add(Sphere::new(0.5).mesh().uv(16, 10)),
        materials: HashMap::default(),
    });
}

/// Share of an arrow's length taken by its head.
const ARROW_HEAD: f32 = 0.42;

/// The click markers' arrow, one metre long along +Z (tail at -0.5, tip at
/// +0.5): a square shaft and a four-sided pyramid head twice as wide, so it
/// reads as an arrow from any side.
pub fn arrow_mesh() -> Mesh {
    let shaft_len = 1.0 - ARROW_HEAD;
    let mut arrow = Mesh::from(Cuboid::new(0.16, 0.16, shaft_len)).translated_by(Vec3::Z * (-0.5 + shaft_len * 0.5));
    // The cone points along +Y; turn it to +Z, a square base edge-on.
    let head = Mesh::from(Cone { radius: 0.34, height: ARROW_HEAD }.mesh().resolution(4))
        .rotated_by(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_y(std::f32::consts::FRAC_PI_4))
        .translated_by(Vec3::Z * (0.5 - ARROW_HEAD * 0.5));
    if let Err(e) = arrow.merge(&head) {
        warn!("click marker arrow drawn without its head: {e}");
    }
    arrow
}

/// How rough every box is: matte, with a broad, dim highlight.
pub const BOX_ROUGHNESS: f32 = 0.85;
/// How much light a box reflects head-on (`StandardMaterial::reflectance`,
/// 0.25 = 1 % against Bevy's default 4 %): low, so dark colours stay
/// saturated instead of greying under the sky light. Static models are
/// drawn with it too (`statics::library::flatten`).
pub const BOX_REFLECTANCE: f32 = 0.25;

/// How much nearer than they are [`Shade::Marker`] materials sort among
/// blended things (metres): more than any two things in view can differ
/// by (the camera sees 2 km at most), so they are drawn after the rest,
/// the fog overlay included. It nudges their depth only by a hair (Bevy
/// also applies it as a constant depth-buffer bias).
pub const MARKER_SORT_BIAS: f32 = 2000.0;

/// The `StandardMaterial` for a key.
pub fn material_for(key: MatKey) -> StandardMaterial {
    let [r, g, b] = key.color();
    let alpha = key.alpha();
    let color = Color::srgba(r, g, b, alpha);
    let glow = key.shade() == Shade::Glow;
    let lit = if glow {
        let l = color.to_linear();
        Color::from(LinearRgba::new(l.red * GLOW_BASE, l.green * GLOW_BASE, l.blue * GLOW_BASE, l.alpha))
    } else {
        color
    };
    let marker = key.shade() == Shade::Marker;
    StandardMaterial {
        base_color: lit,
        emissive: if glow { Color::srgb(r, g, b).to_linear() * GLOW_STRENGTH } else { LinearRgba::BLACK },
        unlit: matches!(key.shade(), Shade::Unlit | Shade::Marker),
        // Markers blend even when solid, so they sort with (after) the
        // blended fog overlay rather than being drawn before it.
        alpha_mode: if key.is_opaque() && !marker { AlphaMode::Opaque } else { AlphaMode::Blend },
        depth_bias: if marker { MARKER_SORT_BIAS } else { 0.0 },
        perceptual_roughness: BOX_ROUGHNESS,
        reflectance: BOX_REFLECTANCE,
        ..default()
    }
}

/// Marks every entity af_render spawns as part of a sim entity's visuals
/// (boxes, pivots, roots), so queries over parts and over sim entities stay
/// disjoint.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct BoxPart;

/// Mutable access to af_render's own boxes and nodes (never sim entities).
pub type PartQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut Visibility, Option<&'static mut MeshMaterial3d<StandardMaterial>>),
    (With<BoxPart>, Without<BoxVisual>),
>;

/// Shared mesh + cached materials. Never creates a material for a key it
/// has seen before.
#[derive(SystemParam)]
pub struct Painter<'w> {
    assets: ResMut<'w, BoxAssets>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
}

impl Painter<'_> {
    pub fn cube(&self) -> Handle<Mesh> {
        self.assets.cube.clone()
    }

    pub fn arrow(&self) -> Handle<Mesh> {
        self.assets.arrow.clone()
    }

    pub fn ring(&self) -> Handle<Mesh> {
        self.assets.ring.clone()
    }

    pub fn disc(&self) -> Handle<Mesh> {
        self.assets.disc.clone()
    }

    pub fn puff(&self) -> Handle<Mesh> {
        self.assets.puff.clone()
    }

    /// The cached material for a look.
    pub fn material(&mut self, look: Look) -> Handle<StandardMaterial> {
        let key = MatKey::new(look);
        if let Some(h) = self.assets.materials.get(&key) {
            return h.clone();
        }
        let handle = self.materials.add(material_for(key));
        self.assets.materials.insert(key, handle.clone());
        handle
    }

    /// `base` (a model's team material) in the owner's `color`, made once
    /// per material and colour.
    pub fn team_tint(&mut self, tints: &mut TeamTints, base: &Handle<StandardMaterial>, color: Rgb) -> Handle<StandardMaterial> {
        tints.tint(base, color, &mut self.materials)
    }

    /// The mesh, material and local transform of a part.
    pub fn bundle(&mut self, part: &Part) -> impl Bundle + use<> {
        (Mesh3d(self.cube()), MeshMaterial3d(self.material(part.look)), part.transform(), Visibility::Inherited, BoxPart)
    }

    /// Spawn `part` as a child of `parent`.
    pub fn spawn_child(&mut self, commands: &mut Commands, parent: Entity, part: &Part) -> Entity {
        let bundle = self.bundle(part);
        let mut e = commands.spawn((bundle, ChildOf(parent)));
        if !part.shadow {
            e.insert(no_shadows());
        }
        e.id()
    }

    /// Spawn a shadowless `mesh` drawn with `look` at `transform` as a
    /// child of `parent` (marker arrows, rings and discs).
    pub fn spawn_shape(&mut self, commands: &mut Commands, parent: Entity, mesh: Handle<Mesh>, look: Look, transform: Transform) -> Entity {
        let material = self.material(look);
        commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), transform, Visibility::Inherited, BoxPart, no_shadows(), ChildOf(parent))).id()
    }

    /// Spawn `part` as a top-level box with `extra` components.
    pub fn spawn_root(&mut self, commands: &mut Commands, part: &Part, extra: impl Bundle) -> Entity {
        let bundle = self.bundle(part);
        let mut e = commands.spawn((bundle, extra));
        if !part.shadow {
            e.insert(NotShadowCaster);
        }
        e.id()
    }
}

/// Components for boxes that neither cast nor receive shadows (markers,
/// bars, translucent shells).
pub fn no_shadows() -> (NotShadowCaster, NotShadowReceiver) {
    (NotShadowCaster, NotShadowReceiver)
}

/// An empty transform node (visual roots, weapon pivots, bar roots).
pub fn node(transform: Transform, visibility: Visibility) -> impl Bundle {
    (transform, visibility, BoxPart)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrow_has_a_shaft_and_a_head_pointing_forward() {
        let arrow = arrow_mesh();
        let cube = Mesh::from(Cuboid::new(1.0, 1.0, 1.0)).count_vertices();
        assert!(arrow.count_vertices() > cube, "the head was merged onto the shaft");
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) = arrow.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("positions") };
        let (min, max) = points.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p))));
        assert!((min.z + 0.5).abs() < 1e-4 && (max.z - 0.5).abs() < 1e-4, "one metre along +Z: {min} {max}");
        // The widest points are the head's, near the front.
        let widest = points.iter().map(|p| Vec3::from(*p)).max_by(|a, b| a.x.abs().total_cmp(&b.x.abs())).unwrap();
        assert!(widest.z > 0.0 && widest.x.abs() > 0.16, "{widest}");
    }

    #[test]
    fn materials_follow_their_key() {
        let glow = material_for(MatKey::new(Look::glow([0.8, 0.4, 1.0])));
        assert!(glow.emissive.red > 0.0 && !glow.unlit && glow.alpha_mode == AlphaMode::Opaque);
        let ghost = material_for(MatKey::new(Look::unlit([0.2, 1.0, 0.2], 0.4)));
        assert!(ghost.unlit && ghost.alpha_mode == AlphaMode::Blend && ghost.emissive == LinearRgba::BLACK);
        let lit = material_for(MatKey::new(Look::lit([0.5, 0.5, 0.5])));
        assert!(!lit.unlit && lit.emissive == LinearRgba::BLACK && lit.depth_bias == 0.0);
        // Click markers blend even when solid, and sort after everything
        // else blended (the fog overlay).
        for alpha in [0.4, 1.0] {
            let marker = material_for(MatKey::new(Look::marker([0.3, 1.0, 0.35], alpha)));
            assert!(marker.unlit && marker.alpha_mode == AlphaMode::Blend && marker.depth_bias >= 1000.0, "{alpha}");
        }
        assert_eq!(ghost.depth_bias, 0.0);
    }
}
