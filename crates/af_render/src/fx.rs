//! Effect boxes (`FxBox`, written by af_combat). Click markers are drawn by
//! [`crate::markers`].

use af_core::prelude::*;
use bevy::prelude::*;

use crate::assets::{Painter, no_shadows};
use crate::look::{Look, MatKey, Shade};
use crate::motion::rotation_for_dir;

/// Effects are drawn at least this many metres thick per metre of camera
/// distance (about 3 px at any zoom), so arrows, bullets and sparks stay
/// visible when the view is zoomed out. Only the drawing grows; the sim
/// keeps the content's sizes.
const FX_MIN_SIZE_PER_DISTANCE: f32 = 0.0018;
/// The thinnest an effect is ever drawn, close up.
const FX_MIN_SIZE: f32 = 0.05;

/// Present once an `FxBox` has its mesh; remembers the material key in use.
#[derive(Component, Clone, Copy, Debug)]
pub struct FxDrawn {
    key: MatKey,
}

/// How an effect box is drawn: glowing effects are unlit (bright, no shading).
pub fn fx_look(fx: &FxBox) -> Look {
    Look { color: fx.color, alpha: fx.alpha, shade: if fx.glow { Shade::Unlit } else { Shade::Lit } }
}

/// The smallest an effect box is drawn along any axis with the camera
/// `camera_distance` metres from its focus.
pub fn fx_min_size(camera_distance: f32) -> f32 {
    (camera_distance * FX_MIN_SIZE_PER_DISTANCE).max(FX_MIN_SIZE)
}

/// Transform of an effect box at tick fraction `t`, no thinner than `min_size`.
pub fn fx_transform(fx: &FxBox, t: f32, min_size: f32) -> Transform {
    Transform::from_translation(fx.prev_pos.lerp(fx.pos, t.clamp(0.0, 1.0))).with_rotation(rotation_for_dir(fx.dir)).with_scale(fx.size.max(Vec3::splat(min_size)))
}

pub(crate) fn attach_fx(mut commands: Commands, mut painter: Painter, fixed: Res<Time<Fixed>>, rig: Res<CameraRig>, fx: Query<(Entity, &FxBox), Without<FxDrawn>>) {
    let (t, min_size) = (fixed.overstep_fraction(), fx_min_size(rig.distance));
    for (e, b) in &fx {
        let look = fx_look(b);
        commands.entity(e).try_insert((
            Mesh3d(painter.cube()),
            MeshMaterial3d(painter.material(look)),
            fx_transform(b, t, min_size),
            Visibility::Inherited,
            no_shadows(),
            FxDrawn { key: MatKey::new(look) },
        ));
    }
}

pub(crate) fn sync_fx(
    mut painter: Painter,
    fixed: Res<Time<Fixed>>,
    rig: Res<CameraRig>,
    mut fx: Query<(&FxBox, &mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut FxDrawn)>,
) {
    let (t, min_size) = (fixed.overstep_fraction(), fx_min_size(rig.distance));
    for (b, mut tf, mut mat, mut drawn) in &mut fx {
        tf.set_if_neq(fx_transform(b, t, min_size));
        let look = fx_look(b);
        let key = MatKey::new(look);
        if key != drawn.key {
            drawn.key = key;
            mat.0 = painter.material(look);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_interpolate_and_face_their_direction() {
        let fx = FxBox {
            pos: Vec3::new(10.0, 2.0, 0.0),
            prev_pos: Vec3::new(8.0, 2.0, 0.0),
            dir: Vec3::X,
            size: Vec3::new(0.05, 0.05, 0.6),
            color: [1.0, 0.5, 0.2],
            glow: true,
            alpha: 1.0,
        };
        let t = fx_transform(&fx, 0.5, 0.01);
        assert_eq!(t.translation, Vec3::new(9.0, 2.0, 0.0));
        assert!((t.rotation * Vec3::Z - Vec3::X).length() < 1e-4);
        assert_eq!(t.scale, fx.size);
        assert_eq!(fx_look(&fx).shade, Shade::Unlit);
        assert_eq!(fx_look(&FxBox { glow: false, ..fx }).shade, Shade::Lit);
    }

    #[test]
    fn thin_effects_stay_visible_when_zoomed_out() {
        let bolt = FxBox { pos: Vec3::ZERO, prev_pos: Vec3::ZERO, dir: Vec3::Z, size: Vec3::new(0.04, 0.04, 0.5), color: [0.6; 3], glow: false, alpha: 1.0 };
        assert!(fx_min_size(150.0) > fx_min_size(60.0), "thicker the further away");
        assert_eq!(fx_min_size(1.0), FX_MIN_SIZE);
        let far = fx_transform(&bolt, 0.0, fx_min_size(120.0));
        assert!((far.scale.x - fx_min_size(120.0)).abs() < 1e-6 && far.scale.x == far.scale.y);
        assert_eq!(far.scale.z, 0.5, "length kept");
    }
}
