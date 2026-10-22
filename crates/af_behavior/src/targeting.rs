//! Who can attack what, and picking hostiles to engage.
//!
//! Reach and firing bands are af_combat's own rules (`af_core::ext::combat`),
//! so a unit never stands where af_combat would not fire: the same reach,
//! the same distance measure (`edge_distance`), and nothing fires at a
//! target inside its minimum range.
//!
//! A ground unit only takes on, and keeps chasing, hostiles it can get
//! within weapon reach of over the terrain (see `reach`): not a camp across
//! the river that only a long detour by the ford leads to, nor anything on
//! an island. What its weapons already reach is always fair game.

use af_core::ext::combat::{edge_distance, in_weapon_range, weapon_reach, TargetClass};
use af_core::ext::movement::NavRegions;
use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::brain::Unreachable;
use crate::motion::{Engagement, Plan};
use crate::reach::Reach;
use crate::tuning::*;

/// The best-scoring hostiles a chase is checked for detours, in order.
const DETOUR_CHECKS: usize = 3;

/// Longest reach of any of these weapons; `None` when unarmed.
pub(crate) fn longest_reach(data: &GameData, stats: &Stats, weapons: &Weapons) -> Option<f32> {
    weapons.slots.iter().map(|s| weapon_reach(data, stats, s.weapon)).reduce(f32::max)
}

/// Can one of these weapons fire at a target of `class` from here?
fn can_fire(data: &GameData, stats: &Stats, weapons: &Weapons, class: TargetClass, me: &Obb, target: &Obb) -> bool {
    weapons
        .slots
        .iter()
        .any(|s| class.allowed_by(&data.weapon(s.weapon).targets) && in_weapon_range(data, stats, s.weapon, me, target))
}

/// What these weapons can do about a target of `class` at `pos`, from box
/// `me`; `None` when none can hit that class (a ground-only spear against a
/// flyer).
///
/// Out of every band, the unit closes in to the longest reach that falls
/// short of the gap (the band it gets to first), or steps back when it is
/// inside the minimum range of every weapon that could hit.
pub(crate) fn assess(data: &GameData, stats: &Stats, weapons: &Weapons, class: TargetClass, me: &Obb, target: &Obb, pos: Vec2) -> Option<Engagement> {
    let gap = edge_distance(me, target);
    let mut armed = false;
    let mut firing: Option<f32> = None;
    // (reach, min_range) of the longest-reaching weapon short of the gap.
    let mut short: Option<(f32, f32)> = None;
    let mut nearest_min = f32::MAX;
    for s in &weapons.slots {
        let def = data.weapon(s.weapon);
        if !class.allowed_by(&def.targets) {
            continue;
        }
        armed = true;
        let reach = weapon_reach(data, stats, s.weapon);
        if in_weapon_range(data, stats, s.weapon, me, target) {
            firing = Some(firing.map_or(reach, |r| r.max(reach)));
        } else if reach < gap {
            if short.is_none_or(|(r, _)| reach > r) {
                short = Some((reach, def.min_range));
            }
        } else {
            // In reach but too close for this weapon.
            nearest_min = nearest_min.min(def.min_range);
        }
    }
    if !armed {
        return None;
    }
    let plan = match (firing, short) {
        (Some(reach), _) => Plan::Stand { reach },
        (None, Some((reach, min))) => {
            // Aim well inside the reach, but never inside the minimum range.
            let wanted = (reach * CHASE_FRACTION).max((min + reach) * 0.5);
            // af_movement measures arrival with the same exact `Obb::gap`.
            Plan::Close { arrive: wanted }
        }
        (None, None) => Plan::BackOff { min: nearest_min },
    };
    Some(Engagement { pos, gap, plan })
}

/// Can these weapons hurt something of `class` at all?
fn threatens(data: &GameData, weapons: &Weapons, class: TargetClass) -> bool {
    weapons.slots.iter().any(|s| class.allowed_by(&data.weapon(s.weapon).targets))
}

/// Where a (potential) target is and what it is.
#[derive(QueryData)]
pub(crate) struct TargetData {
    pub entity: Entity,
    pub pos: &'static Position,
    pub facing: &'static Facing,
    pub footprint: &'static Footprint,
    pub owner: Option<&'static Owner>,
    pub mobile: Option<&'static Mobile>,
    pub building: Has<Building>,
    /// A foundation: its weapons do not fire yet.
    pub foundation: Has<UnderConstruction>,
    /// Has hit points (resource nodes do not).
    pub mortal: Has<Health>,
}

impl TargetDataItem<'_, '_> {
    pub(crate) fn obb(&self) -> Obb {
        self.footprint.obb(self.pos.0, self.facing.0)
    }

    pub(crate) fn class(&self) -> TargetClass {
        TargetClass::of(self.building, self.mobile.is_some_and(|m| m.flying))
    }

    pub(crate) fn owner(&self) -> Option<PlayerId> {
        self.owner.map(|o| o.0)
    }
}

/// The unit choosing a target.
pub(crate) struct Seeker<'a> {
    pub entity: Entity,
    pub owner: PlayerId,
    pub pos: Vec2,
    pub obb: Obb,
    pub flying: bool,
    pub mobile: &'a Mobile,
    pub stats: &'a Stats,
    pub weapons: &'a Weapons,
    /// Hostiles we could not reach lately, as of `tick`.
    pub unreachable: Unreachable,
    pub tick: u64,
}

impl Seeker<'_> {
    fn class(&self) -> TargetClass {
        TargetClass::of(false, self.flying)
    }

    /// Is `target` one automatic target choice skips for now?
    pub(crate) fn skips(&self, target: Entity) -> bool {
        self.unreachable.skips(target, self.tick)
    }
}

/// Everything needed to judge hostiles: diplomacy, fog, the spatial index
/// and what the candidates are armed with.
#[derive(SystemParam)]
pub(crate) struct Sight<'w, 's> {
    pub data: Res<'w, Data>,
    pub players: Res<'w, Players>,
    fog: Res<'w, FogOfWar>,
    pub spatial: Res<'w, SpatialIndex>,
    regions: Option<Res<'w, NavRegions>>,
    armament: Query<'w, 's, &'static Weapons, Without<Dead>>,
}

impl Sight<'_, '_> {
    pub(crate) fn team(&self, player: PlayerId) -> u8 {
        self.players.team(player)
    }

    /// Where a ground seeker can get to over the terrain (`None` for flyers).
    fn reach(&self, me: &Seeker) -> Option<Reach> {
        (!me.flying).then(|| Reach::of(self.regions.as_deref(), me.pos, me.mobile))
    }

    fn hostile(&self, me: PlayerId, other: Option<PlayerId>) -> bool {
        other.is_some_and(|o| self.players.hostile(me, o))
    }

    /// Distance-like cost of a target: lower is better. Enemies that can
    /// fight back come first, then harmless units, then buildings. A
    /// foundation counts as unarmed, as it does for af_combat.
    fn penalty(&self, me: &Seeker, target: Entity, building: bool, foundation: bool) -> f32 {
        let armed = !foundation && self.armament.get(target).is_ok_and(|w| threatens(&self.data, w, me.class()));
        match (building, armed) {
            (false, true) => 0.0,
            (false, false) => PENALTY_HARMLESS_UNIT,
            (true, true) => PENALTY_ARMED_BUILDING,
            (true, false) => PENALTY_HARMLESS_BUILDING,
        }
    }

    /// The best hostile to engage on our own initiative: within `radius`
    /// (box edge to our centre) or, with `in_range`, only what one of our
    /// weapons can already fire at. Walls and hostiles we could not reach
    /// lately are never picked automatically, nor (by a ground unit, unless
    /// in range) ones it cannot get within weapon reach of over the terrain,
    /// or only by a long detour (see [`CHASE_DETOUR`]). Returns the entity
    /// and its score.
    pub(crate) fn best_hostile(&self, me: &Seeker, radius: f32, in_range: bool) -> Option<(Entity, f32)> {
        let team = self.team(me.owner);
        let reach = if in_range { None } else { self.reach(me) };
        let weapon = longest_reach(&self.data, me.stats, me.weapons).unwrap_or(0.0);
        let mut found: Vec<(f32, Entity, Option<Vec2>)> = Vec::new();
        self.spatial.for_each_near(me.pos, radius, |e| {
            if e.kind == SpatialKind::Node
                || e.entity == me.entity
                || !self.hostile(me.owner, e.owner)
                || e.tags.contains(Tag::Wall)
                || me.skips(e.entity)
            {
                return;
            }
            let class = TargetClass::of(e.kind == SpatialKind::Building, e.flying);
            let near = if in_range {
                can_fire(&self.data, me.stats, me.weapons, class, &me.obb, &e.obb)
            } else {
                threatens(&self.data, me.weapons, class) && e.obb.distance_to_point(me.pos) <= radius
            };
            if !near || !self.fog.visible(team, e.pos) || reach.as_ref().is_some_and(|r| !r.within(&e.obb, weapon)) {
                return;
            }
            let gap = edge_distance(&me.obb, &e.obb);
            let score = gap + self.penalty(me, e.entity, e.kind == SpatialKind::Building, e.under_construction);
            // Out of weapon reach: the chase's way is checked below.
            found.push((score, e.entity, (gap > weapon).then_some(e.pos)));
        });
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let direct = |chase: Option<Vec2>| chase.zip(reach.as_ref()).is_none_or(|(p, r)| !r.detour(p));
        found.into_iter().take(DETOUR_CHECKS).find(|f| direct(f.2)).map(|(s, e, _)| (e, s))
    }

    /// How our weapons stand against `target`, whoever it belongs to (a
    /// forced attack); `None` when none can hit it.
    pub(crate) fn assess(&self, me: &Seeker, target: &TargetDataItem) -> Option<Engagement> {
        assess(&self.data, me.stats, me.weapons, target.class(), &me.obb, &target.obb(), target.pos.0)
    }

    /// Is `target` still a hostile worth engaging automatically, no farther
    /// than `leash`, in sight and, when a ground unit would have to chase
    /// it, within its weapon reach of ground it can get to?
    pub(crate) fn engageable(&self, me: &Seeker, target: &TargetDataItem, leash: f32) -> Option<Engagement> {
        if !target.mortal || !self.hostile(me.owner, target.owner()) || !self.fog.visible(self.team(me.owner), target.pos.0) {
            return None;
        }
        let en = self.assess(me, target).filter(|en| en.gap <= leash)?;
        if !en.plan.stands()
            && let Some(reach) = self.reach(me)
            && !reach.within(&target.obb(), longest_reach(&self.data, me.stats, me.weapons).unwrap_or(0.0))
        {
            return None;
        }
        Some(en)
    }

    /// Score of a target already engaged (see [`Sight::best_hostile`]).
    pub(crate) fn score(&self, me: &Seeker, target: &TargetDataItem, gap: f32) -> f32 {
        gap + self.penalty(me, target.entity, target.building, target.foundation)
    }
}

/// Point `AttackTarget` at `target` unless it already does.
pub(crate) fn aim(commands: &mut Commands, entity: Entity, current: Option<&AttackTarget>, target: Entity, forced: bool) {
    let wanted = AttackTarget { entity: target, forced };
    if current != Some(&wanted) {
        commands.entity(entity).try_insert(wanted);
    }
}

/// Drop `AttackTarget` if there is one.
pub(crate) fn disarm(commands: &mut Commands, entity: Entity, current: Option<&AttackTarget>) {
    if current.is_some() {
        commands.entity(entity).try_remove::<AttackTarget>();
    }
}
