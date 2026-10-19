//! The shared cube mesh and the material cache, and the [`Painter`] system
//! param every drawing system uses to turn [`Part`]s into entities.

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

/// The 1x1x1 cube every box is drawn with, and one material per [`MatKey`].
/// Inserted at startup; materials are created on first use and reused.
#[derive(Resource)]
pub struct BoxAssets {
    pub cube: Handle<Mesh>,
    materials: HashMap<MatKey, Handle<StandardMaterial>>,
}

impl BoxAssets {
    /// Number of distinct materials created so far.
    pub fn material_count(&self) -> usize {
        self.materials.len()
    }
}

pub(crate) fn init_box_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(BoxAssets { cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)), materials: HashMap::default() });
}

/// How rough every box is: matte, with a broad, dim highlight.
pub const BOX_ROUGHNESS: f32 = 0.85;
/// How much light a box reflects head-on (`StandardMaterial::reflectance`,
/// 0.25 = 1 % against Bevy's default 4 %): low, so dark colours stay
/// saturated instead of greying under the sky light. Static models are
/// drawn with it too (`statics::library::flatten`).
pub const BOX_REFLECTANCE: f32 = 0.25;

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
    StandardMaterial {
        base_color: lit,
        emissive: if glow { Color::srgb(r, g, b).to_linear() * GLOW_STRENGTH } else { LinearRgba::BLACK },
        unlit: key.shade() == Shade::Unlit,
        alpha_mode: if key.is_opaque() { AlphaMode::Opaque } else { AlphaMode::Blend },
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
    fn materials_follow_their_key() {
        let glow = material_for(MatKey::new(Look::glow([0.8, 0.4, 1.0])));
        assert!(glow.emissive.red > 0.0 && !glow.unlit && glow.alpha_mode == AlphaMode::Opaque);
        let ghost = material_for(MatKey::new(Look::unlit([0.2, 1.0, 0.2], 0.4)));
        assert!(ghost.unlit && ghost.alpha_mode == AlphaMode::Blend && ghost.emissive == LinearRgba::BLACK);
        let lit = material_for(MatKey::new(Look::lit([0.5, 0.5, 0.5])));
        assert!(!lit.unlit && lit.emissive == LinearRgba::BLACK);
    }
}
