//! Shared items added by af_combat (other crates may use them).
//! Only af_combat edits this file. Never modify items in other af_core files.
//!
//! These are the rules af_combat fires by, so that af_behavior (how close to
//! chase), af_ai (what can fight what) and af_ui (tooltips) agree with it.

use bevy::math::Vec2;

use crate::prelude::*;

/// What kind of thing a target is, for a weapon's [`Targets`] filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetClass {
    /// A unit on the ground (hover units included).
    Ground,
    /// A flying unit.
    Air,
    /// Any building, foundations included.
    Building,
}

impl TargetClass {
    pub fn of(is_building: bool, flying: bool) -> TargetClass {
        if is_building {
            TargetClass::Building
        } else if flying {
            TargetClass::Air
        } else {
            TargetClass::Ground
        }
    }

    /// May a weapon with these `Targets` engage this class?
    pub fn allowed_by(self, targets: &Targets) -> bool {
        match self {
            TargetClass::Ground => targets.ground,
            TargetClass::Air => targets.air,
            TargetClass::Building => targets.buildings,
        }
    }
}

/// Exact edge-to-edge distance between two boxes on the ground plane (0 when
/// they touch or overlap). Every weapon reach is measured with this.
///
/// [`Obb::gap`] takes the largest separation over the four box axes, which
/// under-reads boxes offset diagonally by up to a factor of 1/sqrt(2): a
/// weapon checked with it engages up to 41% beyond its range.
pub fn edge_distance(a: &Obb, b: &Obb) -> f32 {
    let (fa, fb) = (Frame::of(a), Frame::of(b));
    let d = fb.center - fa.center;
    let separated = [fa.right, fa.forward, fb.right, fb.forward]
        .into_iter()
        .any(|axis| d.dot(axis).abs() >= fa.support(axis) + fb.support(axis));
    if !separated {
        return 0.0;
    }
    // Two disjoint convex polygons are closest at a corner of one of them.
    let a_to_b = fa.corners().into_iter().map(|c| fb.distance_squared(c));
    let b_to_a = fb.corners().into_iter().map(|c| fa.distance_squared(c));
    a_to_b.chain(b_to_a).fold(f32::MAX, f32::min).sqrt()
}

/// An [`Obb`] with its axes worked out once (no trigonometry per query).
struct Frame {
    center: Vec2,
    half: Vec2,
    right: Vec2,
    forward: Vec2,
}

impl Frame {
    fn of(b: &Obb) -> Frame {
        let (right, forward) = b.axes();
        Frame { center: b.center, half: b.half, right, forward }
    }

    fn support(&self, axis: Vec2) -> f32 {
        axis.dot(self.right).abs() * self.half.x + axis.dot(self.forward).abs() * self.half.y
    }

    fn corners(&self) -> [Vec2; 4] {
        let (r, f, c) = (self.right * self.half.x, self.forward * self.half.y, self.center);
        [c - r - f, c + r - f, c + r + f, c - r + f]
    }

    fn distance_squared(&self, p: Vec2) -> f32 {
        let d = p - self.center;
        Vec2::new((d.dot(self.right).abs() - self.half.x).max(0.0), (d.dot(self.forward).abs() - self.half.y).max(0.0)).length_squared()
    }
}

/// Reach of weapon `w` on an entity with `stats`, edge to edge: the tech
/// modified [`stats::weapon_range`], for every weapon kind. This is how far
/// a weapon acquires targets and how close af_behavior chases; the damage
/// area of a Nova or Spray is [`area_reach`] instead.
pub fn weapon_reach(data: &GameData, stats: &Stats, w: WeaponId) -> f32 {
    stats::weapon_range(data, stats, w)
}

/// Radius of a Nova's ring and length of a Spray's cone: the def's own
/// `range`, which `Stat::Range` techs do not grow (design doc, section 11).
pub fn area_reach(def: &WeaponDef) -> f32 {
    def.range.max(0.1)
}

/// Is `target` inside weapon `w`'s firing band (`min_range ..= reach`),
/// measured with [`edge_distance`]?
pub fn in_weapon_range(data: &GameData, stats: &Stats, w: WeaponId, shooter: &Obb, target: &Obb) -> bool {
    let gap = edge_distance(shooter, target);
    gap >= data.weapon(w).min_range && gap <= weapon_reach(data, stats, w)
}

/// The longest reach among `weapons` that may engage a target of `class`,
/// or `None` when none can (e.g. melee against a flyer). af_behavior chases
/// until the [`edge_distance`] to the target is below this.
pub fn attack_reach(data: &GameData, stats: &Stats, weapons: &Weapons, class: TargetClass) -> Option<f32> {
    weapons
        .slots
        .iter()
        .filter(|s| class.allowed_by(&data.weapon(s.weapon).targets))
        .map(|s| weapon_reach(data, stats, s.weapon))
        .reduce(f32::max)
}
