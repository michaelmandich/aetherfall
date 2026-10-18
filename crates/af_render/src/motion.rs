//! Pure motion helpers: tick interpolation, effect orientation and weapon
//! animation poses.

use std::f32::consts::{PI, TAU};

use af_core::prelude::*;
use bevy::prelude::*;

/// Ground position between the start (`prev`) and end (`cur`) of the
/// current sim tick, `t` = `Time<Fixed>::overstep_fraction()`.
#[inline]
pub fn lerp_pos(prev: Vec2, cur: Vec2, t: f32) -> Vec2 {
    prev.lerp(cur, t.clamp(0.0, 1.0))
}

/// Yaw between `prev` and `cur` along the shorter way round.
#[inline]
pub fn lerp_yaw(prev: f32, cur: f32, t: f32) -> f32 {
    af_geom::wrap_angle(prev + af_geom::wrap_angle(cur - prev) * t.clamp(0.0, 1.0))
}

/// World transform of a sim entity box root: ground position, base height and yaw.
#[inline]
pub fn ground_transform(pos: Vec2, base_y: f32, yaw: f32) -> Transform {
    Transform::from_xyz(pos.x, base_y, pos.y).with_rotation(Quat::from_rotation_y(yaw))
}

/// Rotation that points a box's +Z axis along `dir`, keeping its +Y as close
/// to world up as possible. Identity for a zero direction.
pub fn rotation_for_dir(dir: Vec3) -> Quat {
    if dir.length_squared() < 1e-12 {
        return Quat::IDENTITY;
    }
    Transform::IDENTITY.aligned_by(Vec3::Z, dir, Vec3::Y, Vec3::Y).rotation
}

/// Phase at which an attack releases (the hit, the shot): see [`attack_phase`].
pub const RELEASE: f32 = 0.5;
/// Rate at which af_combat lets `WeaponSlot::anim` fall from 1 back to 0
/// after an attack, per second.
pub const ANIM_RECOVERY: f32 = 4.0;

/// Attack phase (0..1) of a weapon slot as af_combat drives it: `anim`
/// rises 0 -> 1 while the slot winds up (`windup` seconds left > 0), is 1
/// at release, and falls back to 0 afterwards. Phases 0..[`RELEASE`] are
/// the windup, [`RELEASE`] the moment the attack lands, and RELEASE..1 the
/// recovery, so a pose plays once per attack; 0 and 1 are rest.
pub fn attack_phase(windup: f32, anim: f32) -> f32 {
    let anim = anim.clamp(0.0, 1.0);
    if windup > 0.0 {
        RELEASE * anim
    } else if anim > 0.0 {
        1.0 - (1.0 - RELEASE) * anim
    } else {
        0.0
    }
}

/// The `(windup, anim)` af_combat leaves on a slot `t` seconds after it
/// starts an attack with `windup` seconds of windup (see [`attack_phase`]).
/// Lets previews drive weapons the way combat does.
pub fn combat_slot_state(t: f32, windup: f32) -> (f32, f32) {
    if t < windup {
        (windup - t, t / windup)
    } else {
        (0.0, (1.0 - (t - windup) * ANIM_RECOVERY).max(0.0))
    }
}

/// How a mounted weapon box moves during an attack (from the weapon kind).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WeaponAnim {
    /// Horizontal sweep through `arc` radians around the grip.
    Swing { arc: f32 },
    /// Slides forward by `reach` metres and back.
    Thrust { reach: f32 },
    /// Lifts tip-up, then slams down past level.
    Smash,
    /// Ranged: kicks back by `kick` metres when it fires. `turret` weapons
    /// also yaw by the slot's `aim_yaw`.
    Recoil { kick: f32, turret: bool },
}

impl WeaponAnim {
    /// The animation for a weapon def mounted on a building (`on_building`)
    /// or a unit. Turrets swivel the way af_combat aims them: every
    /// building weapon and every `fire_on_move` weapon, except novas and
    /// bombs, which need no aim.
    pub fn of(def: &WeaponDef, on_building: bool) -> WeaponAnim {
        let length = def.look.size[2].max(0.05);
        match &def.kind {
            WeaponKind::Melee { style: MeleeStyle::Swing, arc_deg, .. } => {
                let deg = if *arc_deg > 1.0 { *arc_deg } else { 60.0 };
                WeaponAnim::Swing { arc: deg.to_radians() }
            }
            WeaponKind::Melee { style: MeleeStyle::Thrust, .. } => WeaponAnim::Thrust { reach: (length * 0.4).clamp(0.1, 1.2) },
            WeaponKind::Melee { style: MeleeStyle::Smash, .. } => WeaponAnim::Smash,
            WeaponKind::Nova | WeaponKind::Bomb { .. } => WeaponAnim::Recoil { kick: (length * 0.2).clamp(0.04, 0.4), turret: false },
            _ => WeaponAnim::Recoil { kick: (length * 0.2).clamp(0.04, 0.4), turret: on_building || def.fire_on_move },
        }
    }

    /// Swing and smash rotate about the grip (the box's rear end); the
    /// others move the box about its centre.
    pub fn pivots_at_grip(&self) -> bool {
        matches!(self, WeaponAnim::Swing { .. } | WeaponAnim::Smash)
    }

    /// Pivot pose (offset from the rest position, rotation) at attack
    /// `phase` (from [`attack_phase`]) with turret aim `aim_yaw`.
    pub fn pose(&self, phase: f32, aim_yaw: f32) -> (Vec3, Quat) {
        let p = phase.clamp(0.0, 1.0);
        let active = p > 0.0 && p < 1.0;
        match *self {
            // Back to one side during the windup, across the front at the
            // hit, out to the other side and back during the recovery.
            WeaponAnim::Swing { arc } => {
                let a = if active { 0.5 * arc * (TAU * p).sin() } else { 0.0 };
                (Vec3::ZERO, Quat::from_rotation_y(a))
            }
            // Fully extended at the hit.
            WeaponAnim::Thrust { reach } => {
                let z = if active { reach * (PI * p).sin() } else { 0.0 };
                (Vec3::Z * z, Quat::IDENTITY)
            }
            WeaponAnim::Smash => (Vec3::ZERO, Quat::from_rotation_x(smash_pitch(p))),
            // Kicks back at the shot and eases forward during the recovery.
            WeaponAnim::Recoil { kick, turret } => {
                let yaw = if turret { aim_yaw } else { 0.0 };
                let rot = Quat::from_rotation_y(yaw);
                let back = if (RELEASE..1.0).contains(&p) { kick * (1.0 - p) / (1.0 - RELEASE) } else { 0.0 };
                (rot * (Vec3::NEG_Z * back), rot)
            }
        }
    }
}

/// Pitch of a smash at phase `p`: a slow lift to 70 degrees tip-up, a fast
/// slam to 25 degrees below level that bottoms out at the hit, then back
/// to rest during the recovery.
fn smash_pitch(p: f32) -> f32 {
    const LIFT: f32 = -70.0 * PI / 180.0;
    const SLAM: f32 = 25.0 * PI / 180.0;
    /// The lift ends and the slam starts here.
    const TOP: f32 = 0.7 * RELEASE;
    if p <= 0.0 || p >= 1.0 {
        0.0
    } else if p < TOP {
        LIFT * (p / TOP)
    } else if p < RELEASE {
        LIFT + (SLAM - LIFT) * ((p - TOP) / (RELEASE - TOP))
    } else {
        SLAM * (1.0 - (p - RELEASE) / (1.0 - RELEASE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn position_interpolation() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(10.0, -4.0);
        assert_eq!(lerp_pos(a, b, 0.0), a);
        assert_eq!(lerp_pos(a, b, 1.0), b);
        assert_eq!(lerp_pos(a, b, 0.5), Vec2::new(5.0, -2.0));
        assert_eq!(lerp_pos(a, b, 1.5), b, "clamped");
    }

    #[test]
    fn yaw_interpolation_takes_the_short_way() {
        assert!((lerp_yaw(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
        // 3.0 -> -3.0 crosses PI, not zero.
        let mid = lerp_yaw(3.0, -3.0, 0.5);
        assert!(mid.abs() > 3.0, "{mid}");
        assert!((lerp_yaw(3.0, -3.0, 1.0) - (-3.0)).abs() < 1e-5);
    }

    #[test]
    fn ground_transform_matches_the_sim_convention() {
        // Yaw PI/2 faces +X in the sim; the box's local +Z must point there too.
        let t = ground_transform(Vec2::new(3.0, 4.0), 12.0, FRAC_PI_2);
        assert_eq!(t.translation, Vec3::new(3.0, 12.0, 4.0));
        let fwd = af_geom::forward(FRAC_PI_2);
        assert!(near(t.rotation * Vec3::Z, Vec3::new(fwd.x, 0.0, fwd.y)));
        let right = af_geom::right(FRAC_PI_2);
        assert!(near(t.rotation * Vec3::X, Vec3::new(right.x, 0.0, right.y)));
    }

    #[test]
    fn effect_rotation_points_z_along_dir() {
        for dir in [Vec3::Z, Vec3::X, Vec3::new(1.0, 0.5, -2.0), Vec3::NEG_Y, Vec3::Y] {
            let q = rotation_for_dir(dir);
            assert!(near(q * Vec3::Z, dir.normalize()), "{dir:?}");
        }
        assert_eq!(rotation_for_dir(Vec3::ZERO), Quat::IDENTITY);
        // Level shots keep the box upright.
        let q = rotation_for_dir(Vec3::new(1.0, 0.0, 1.0));
        assert!(near(q * Vec3::Y, Vec3::Y));
    }

    #[test]
    fn idle_weapons_rest() {
        for anim in [WeaponAnim::Swing { arc: 1.0 }, WeaponAnim::Thrust { reach: 0.5 }, WeaponAnim::Smash, WeaponAnim::Recoil { kick: 0.2, turret: false }] {
            for phase in [0.0, 1.0, attack_phase(0.0, 0.0)] {
                let (off, rot) = anim.pose(phase, 0.0);
                assert!(near(off, Vec3::ZERO) && rot.angle_between(Quat::IDENTITY) < 1e-5, "{anim:?} at {phase}");
            }
        }
    }

    #[test]
    fn swing_crosses_the_arc_and_hits_in_front() {
        let arc = 60f32.to_radians();
        let s = WeaponAnim::Swing { arc };
        let (_, a) = s.pose(0.25, 0.0);
        let (_, b) = s.pose(0.75, 0.0);
        let side_a = (a * Vec3::Z).x;
        let side_b = (b * Vec3::Z).x;
        assert!(side_a * side_b < 0.0, "sweeps from one side to the other");
        assert!((a.angle_between(Quat::IDENTITY) - arc / 2.0).abs() < 1e-4);
        let (_, hit) = s.pose(RELEASE, 0.0);
        assert!(near(hit * Vec3::Z, Vec3::Z), "points at the target when it lands");
    }

    #[test]
    fn thrust_recoil_and_smash_peak_at_the_hit() {
        let (off, _) = WeaponAnim::Thrust { reach: 0.5 }.pose(RELEASE, 0.0);
        assert!(near(off, Vec3::Z * 0.5));
        let recoil = WeaponAnim::Recoil { kick: 0.2, turret: false };
        assert!(near(recoil.pose(RELEASE, 0.0).0, Vec3::NEG_Z * 0.2), "full kick at the shot");
        assert!(near(recoil.pose(0.3, 0.0).0, Vec3::ZERO), "no kick before the shot");
        assert!(near(recoil.pose(0.75, 0.0).0, Vec3::NEG_Z * 0.1));
        // Smash lifts the tip during the windup and is slammed below level at the hit.
        let (_, lift) = WeaponAnim::Smash.pose(0.3, 0.0);
        assert!((lift * Vec3::Z).y > 0.5);
        let (_, slam) = WeaponAnim::Smash.pose(RELEASE, 0.0);
        assert!((slam * Vec3::Z).y < -0.2);
    }

    /// `(windup, anim)` per tick for one attack, computed the way
    /// af_combat's `step_slot` does (windup counts down from `windup`; the
    /// attack fires when it runs out; `anim` then falls at ANIM_RECOVERY),
    /// from the tick the attack starts until the slot is idle again.
    fn combat_ticks(windup: f32) -> (Vec<(f32, f32)>, usize) {
        const EPS: f32 = 1e-4;
        let (mut left, mut anim) = if windup > EPS { (windup, 0.0) } else { (0.0, 1.0) };
        let mut ticks = vec![(left, anim)];
        let mut fired = if windup > EPS { usize::MAX } else { 0 };
        while left > 0.0 || anim > 0.0 {
            if left > 0.0 {
                left -= SIM_DT;
                if left > EPS {
                    anim = 1.0 - left / windup;
                } else {
                    left = 0.0;
                    anim = 1.0;
                    fired = ticks.len();
                }
            } else {
                anim = (anim - SIM_DT * ANIM_RECOVERY).max(0.0);
            }
            ticks.push((left, anim));
        }
        (ticks, fired)
    }

    #[test]
    fn poses_play_once_per_attack_and_land_at_the_hit() {
        for windup in [0.0, 0.2, 0.6] {
            let (ticks, fired) = combat_ticks(windup);
            let phases: Vec<f32> = ticks.iter().map(|(w, a)| attack_phase(*w, *a)).collect();
            assert!(phases.windows(2).all(|p| p[1] >= p[0] || p[1] == 0.0), "phase only moves forward: {phases:?}");
            assert_eq!(phases[fired], RELEASE, "windup {windup}: the attack lands at the release phase");
            assert!(phases[..fired].iter().all(|p| *p < RELEASE));
            assert_eq!(*phases.last().unwrap(), 0.0, "back at rest");
            // A thrust is fully out exactly when the hit lands, and goes out and back once.
            let reach: Vec<f32> = phases.iter().map(|p| WeaponAnim::Thrust { reach: 1.0 }.pose(*p, 0.0).0.z).collect();
            let peak = reach.iter().cloned().fold(0.0, f32::max);
            assert_eq!(reach[fired], peak);
            assert!(reach[..=fired].windows(2).all(|r| r[1] >= r[0]) && reach[fired..].windows(2).all(|r| r[1] <= r[0]), "{reach:?}");
            // Recoil kicks at the shot, never before it.
            let kick: Vec<f32> = phases.iter().map(|p| -WeaponAnim::Recoil { kick: 0.2, turret: false }.pose(*p, 0.0).0.z).collect();
            assert!(kick[..fired].iter().all(|k| *k == 0.0) && (kick[fired] - 0.2).abs() < 1e-6, "{kick:?}");
            // The preview helper reproduces the same slot states.
            for (i, (w, a)) in ticks.iter().enumerate() {
                let (pw, pa) = combat_slot_state(i as f32 * SIM_DT, windup);
                assert!((pw - w).abs() < 1e-3 && (pa - a).abs() < 1e-3, "tick {i}: ({pw}, {pa}) vs ({w}, {a})");
            }
        }
    }

    #[test]
    fn turrets_follow_aim_yaw() {
        let (_, rot) = WeaponAnim::Recoil { kick: 0.1, turret: true }.pose(0.0, FRAC_PI_2);
        assert!(near(rot * Vec3::Z, Vec3::X));
        let (_, rot) = WeaponAnim::Recoil { kick: 0.1, turret: false }.pose(0.0, FRAC_PI_2);
        assert!(near(rot * Vec3::Z, Vec3::Z), "body-fixed weapons ignore aim_yaw");
    }

    #[test]
    fn anims_come_from_weapon_kinds() {
        let data = af_core::testing::test_data();
        let mut seen = [false; 4];
        for w in &data.weapons {
            for on_building in [false, true] {
                let anim = WeaponAnim::of(w, on_building);
                match (&w.kind, anim) {
                    (WeaponKind::Melee { style: MeleeStyle::Swing, .. }, WeaponAnim::Swing { arc }) => {
                        assert!(arc > 0.0);
                        seen[0] = true;
                    }
                    (WeaponKind::Melee { style: MeleeStyle::Thrust, .. }, WeaponAnim::Thrust { .. }) => seen[1] = true,
                    (WeaponKind::Melee { style: MeleeStyle::Smash, .. }, WeaponAnim::Smash) => seen[2] = true,
                    (WeaponKind::Melee { .. }, other) => panic!("{} melee mapped to {other:?}", w.id),
                    (WeaponKind::Nova | WeaponKind::Bomb { .. }, WeaponAnim::Recoil { turret, .. }) => assert!(!turret, "{} needs no aim", w.id),
                    (_, WeaponAnim::Recoil { turret, .. }) => {
                        // af_combat aims every building weapon and every fire-on-move weapon as a turret.
                        assert_eq!(turret, on_building || w.fire_on_move, "{} on a building: {on_building}", w.id);
                        seen[3] = true;
                    }
                    (_, other) => panic!("{} ranged mapped to {other:?}", w.id),
                }
            }
        }
        assert_eq!(seen, [true; 4], "the content uses every animation");
    }
}
