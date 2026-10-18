//! Effect boxes (`FxBox`, written by af_combat) and click pings.

use af_core::prelude::*;
use bevy::prelude::*;

use crate::assets::{Painter, no_shadows};
use crate::entities::Ground;
use crate::look::{Look, MatKey, Shade};
use crate::motion::rotation_for_dir;

/// Side of a fresh click ping (metres).
const PING_SIZE: f32 = 0.9;
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

/// Present once a ping has its mesh; `total` is its starting lifetime.
#[derive(Component, Clone, Copy, Debug)]
pub struct PingDrawn {
    total: f32,
}

/// Scale of a ping with `time_left` of `total` seconds: a flat square that
/// shrinks to nothing.
pub fn ping_scale(time_left: f32, total: f32) -> Vec3 {
    let f = if total > 0.0 { (time_left / total).clamp(0.0, 1.0) } else { 0.0 };
    Vec3::new(PING_SIZE * f, 0.03, PING_SIZE * f)
}

pub(crate) fn attach_pings(mut commands: Commands, mut painter: Painter, ground: Ground, pings: Query<(Entity, &ClickPing), Without<PingDrawn>>) {
    for (e, ping) in &pings {
        let total = ping.time_left.max(0.01);
        commands.entity(e).try_insert((
            Mesh3d(painter.cube()),
            MeshMaterial3d(painter.material(Look::unlit(ping.color, 0.9))),
            Transform::from_xyz(ping.pos.x, ground.surface(ping.pos) + 0.08, ping.pos.y)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4))
                .with_scale(ping_scale(ping.time_left, total)),
            Visibility::Inherited,
            no_shadows(),
            PingDrawn { total },
        ));
    }
}

/// Count pings down in real time (they keep shrinking while paused) and
/// despawn them when they run out.
pub(crate) fn tick_pings(mut commands: Commands, time: Res<Time<Real>>, mut pings: Query<(Entity, &mut ClickPing, &PingDrawn, &mut Transform)>) {
    let dt = time.delta_secs();
    for (e, mut ping, drawn, mut tf) in &mut pings {
        ping.time_left -= dt;
        if ping.time_left <= 0.0 {
            commands.entity(e).try_despawn();
        } else {
            tf.scale = ping_scale(ping.time_left, drawn.total);
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

    #[test]
    fn pings_shrink_to_nothing() {
        assert_eq!(ping_scale(1.0, 1.0).x, PING_SIZE);
        assert!((ping_scale(0.5, 1.0).x - PING_SIZE * 0.5).abs() < 1e-6);
        assert_eq!(ping_scale(0.0, 1.0).x, 0.0);
        assert_eq!(ping_scale(-1.0, 1.0).x, 0.0);
    }
}
