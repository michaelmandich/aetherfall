//! Effective stats: def values + a player's tech modifiers.
//!
//! af_economy applies a researched tech by calling
//! [`PlayerMods::apply_effect`] for each effect, then refreshing the Stats of
//! the player's existing entities with [`unit_stats`] / [`building_stats`]
//! (keeping hp fraction). Spawning uses the same functions, so new and old
//! entities always agree.

use af_data::*;

use crate::resources::PlayerState;

/// Bit set of [`Tag`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TagSet(pub u64);

impl TagSet {
    pub fn from_tags(tags: &[Tag]) -> TagSet {
        let mut s = 0u64;
        for t in tags {
            s |= 1u64 << (*t as u32);
        }
        TagSet(s)
    }
    #[inline]
    pub fn contains(&self, t: Tag) -> bool {
        self.0 & (1u64 << (t as u32)) != 0
    }
    #[inline]
    pub fn insert(&mut self, t: Tag) {
        self.0 |= 1u64 << (t as u32);
    }
}

/// Damage multiplier of `weapon` against something with `tags`.
pub fn bonus_multiplier(data: &GameData, weapon: WeaponId, tags: TagSet) -> f32 {
    let mut m = 1.0;
    for (tag, mult) in &data.weapon(weapon).bonus {
        if tags.contains(*tag) {
            m *= *mult;
        }
    }
    m
}

/// A selector with ids resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    AllUnits,
    AllBuildings,
    Unit(UnitTypeId),
    Building(BuildingTypeId),
    Tag(Tag),
}

impl Target {
    pub fn resolve(data: &GameData, sel: &Selector) -> Option<Target> {
        Some(match sel {
            Selector::AllUnits => Target::AllUnits,
            Selector::AllBuildings => Target::AllBuildings,
            Selector::Unit(u) => Target::Unit(data.unit_id(u)?),
            Selector::Building(b) => Target::Building(data.building_id(b)?),
            Selector::Tag(t) => Target::Tag(*t),
        })
    }
    pub fn matches_unit(&self, data: &GameData, u: UnitTypeId) -> bool {
        match self {
            Target::AllUnits => true,
            Target::AllBuildings | Target::Building(_) => false,
            Target::Unit(x) => *x == u,
            Target::Tag(t) => data.unit(u).has_tag(*t),
        }
    }
    pub fn matches_building(&self, data: &GameData, b: BuildingTypeId) -> bool {
        match self {
            Target::AllBuildings => true,
            Target::AllUnits | Target::Unit(_) => false,
            Target::Building(x) => *x == b,
            Target::Tag(t) => data.building(b).has_tag(*t),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatMod {
    pub target: Target,
    pub stat: Stat,
    pub op: Op,
    pub value: f32,
}

/// Everything a player's researched techs changed.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerMods {
    /// Multiplies the faction gather rates (starts at 1.0).
    pub gather_mult: ResAmounts,
    pub carry_add: f32,
    /// Passive income per second from techs (buildings' trickle is separate).
    pub trickle: ResAmounts,
    pub stat_mods: Vec<StatMod>,
    pub cost_mods: Vec<(Target, f32)>,
    /// (from, to) unit line upgrades, in research order.
    pub unit_upgrades: Vec<(UnitTypeId, UnitTypeId)>,
}

impl Default for PlayerMods {
    fn default() -> Self {
        PlayerMods {
            gather_mult: ResAmounts([1.0; MAX_RESOURCES]),
            carry_add: 0.0,
            trickle: ResAmounts::ZERO,
            stat_mods: Vec::new(),
            cost_mods: Vec::new(),
            unit_upgrades: Vec::new(),
        }
    }
}

impl PlayerMods {
    /// Record one tech effect. `AdvanceAge` is handled by the caller
    /// (af_economy bumps `PlayerState::age`).
    pub fn apply_effect(&mut self, data: &GameData, e: &Effect) {
        match e {
            Effect::AdvanceAge => {}
            Effect::Stat { target, stat, op, value } => {
                if let Some(t) = Target::resolve(data, target) {
                    self.stat_mods.push(StatMod { target: t, stat: *stat, op: *op, value: *value });
                }
            }
            Effect::GatherRate { resource, mult } => {
                if let Some(r) = data.resource_id(resource) {
                    let v = self.gather_mult.get(r) * mult;
                    self.gather_mult.set(r, v);
                }
            }
            Effect::CarryCapacity { add } => self.carry_add += add,
            Effect::CostMult { target, mult } => {
                if let Some(t) = Target::resolve(data, target) {
                    self.cost_mods.push((t, *mult));
                }
            }
            Effect::UpgradeUnit { from, to } => {
                if let (Some(f), Some(t)) = (data.unit_id(from), data.unit_id(to)) {
                    self.unit_upgrades.push((f, t));
                }
            }
            Effect::Trickle { resource, per_sec } => {
                if let Some(r) = data.resource_id(resource) {
                    self.trickle.add(r, *per_sec);
                }
            }
        }
    }

    /// Follow line upgrades: militia -> man_at_arms -> ... .
    pub fn upgraded_unit(&self, mut u: UnitTypeId) -> UnitTypeId {
        for _ in 0..16 {
            match self.unit_upgrades.iter().find(|(f, _)| *f == u) {
                Some((_, t)) => u = *t,
                None => break,
            }
        }
        u
    }

    /// `base` with every matching Add applied, then every matching Mul.
    fn stat(&self, base: f32, stat: Stat, matches: impl Fn(&Target) -> bool) -> f32 {
        let mut v = base;
        for m in &self.stat_mods {
            if m.stat == stat && m.op == Op::Add && matches(&m.target) {
                v += m.value;
            }
        }
        for m in &self.stat_mods {
            if m.stat == stat && m.op == Op::Mul && matches(&m.target) {
                v *= m.value;
            }
        }
        v
    }
}

fn armor_with(mods: &PlayerMods, base: &Armor, matches: &impl Fn(&Target) -> bool) -> Armor {
    let mut a = *base;
    for t in [DamageType::Slash, DamageType::Pierce, DamageType::Crush, DamageType::Fire, DamageType::Energy] {
        let v = mods.stat(base.get(t), Stat::Armor(t), matches);
        *a.get_mut(t) = v.clamp(0.0, 0.9);
    }
    a
}

/// Effective stats of a unit type for a player.
pub fn unit_stats(data: &GameData, player: &PlayerState, u: UnitTypeId) -> crate::components::Stats {
    let def = data.unit(u);
    let mods = &player.mods;
    let m = |t: &Target| t.matches_unit(data, u);
    crate::components::Stats {
        max_hp: mods.stat(def.hp, Stat::Hp, m).max(1.0),
        armor: armor_with(mods, &def.armor, &m),
        speed: mods.stat(def.speed, Stat::Speed, m).max(0.0),
        vision: mods.stat(def.vision, Stat::Vision, m).max(1.0),
        regen: mods.stat(def.regen, Stat::Regen, m).max(0.0),
        damage_mult: mods.stat(1.0, Stat::Damage, m).max(0.0),
        range_add: mods.stat(0.0, Stat::Range, m),
        cooldown_mult: mods.stat(1.0, Stat::Cooldown, m).max(0.05),
        shield_max: mods.stat(def.shield.as_ref().map_or(0.0, |s| s.hp), Stat::ShieldHp, m).max(0.0),
        tags: TagSet::from_tags(&def.tags),
    }
}

/// Effective stats of a building type for a player.
pub fn building_stats(data: &GameData, player: &PlayerState, b: BuildingTypeId) -> crate::components::Stats {
    let def = data.building(b);
    let mods = &player.mods;
    let m = |t: &Target| t.matches_building(data, b);
    crate::components::Stats {
        max_hp: mods.stat(def.hp, Stat::Hp, m).max(1.0),
        armor: armor_with(mods, &def.armor, &m),
        speed: 0.0,
        vision: mods.stat(def.vision, Stat::Vision, m).max(1.0),
        regen: mods.stat(def.regen, Stat::Regen, m).max(0.0),
        damage_mult: mods.stat(1.0, Stat::Damage, m).max(0.0),
        range_add: mods.stat(0.0, Stat::Range, m),
        cooldown_mult: mods.stat(1.0, Stat::Cooldown, m).max(0.05),
        shield_max: mods.stat(def.shield.as_ref().map_or(0.0, |s| s.hp), Stat::ShieldHp, m).max(0.0),
        tags: TagSet::from_tags(&def.tags),
    }
}

/// Price of a unit for a player (cost techs applied).
pub fn unit_cost(data: &GameData, player: &PlayerState, u: UnitTypeId) -> ResAmounts {
    let mut c = data.unit(u).cost.amounts;
    for (t, mult) in &player.mods.cost_mods {
        if t.matches_unit(data, u) {
            c = c.scaled(*mult);
        }
    }
    c
}

pub fn building_cost(data: &GameData, player: &PlayerState, b: BuildingTypeId) -> ResAmounts {
    let mut c = data.building(b).cost.amounts;
    for (t, mult) in &player.mods.cost_mods {
        if t.matches_building(data, b) {
            c = c.scaled(*mult);
        }
    }
    c
}

pub fn unit_train_time(data: &GameData, player: &PlayerState, u: UnitTypeId) -> f32 {
    player.mods.stat(data.unit(u).train_time, Stat::TrainTime, |t| t.matches_unit(data, u)).max(0.5)
}

pub fn building_build_time(data: &GameData, player: &PlayerState, b: BuildingTypeId) -> f32 {
    player.mods.stat(data.building(b).build_time, Stat::BuildTime, |t| t.matches_building(data, b)).max(0.5)
}

/// Gather rate per second of a worker type for a resource (0 = cannot).
pub fn gather_rate(data: &GameData, player: &PlayerState, u: UnitTypeId, r: ResourceId) -> f32 {
    data.gather_rate(u, player.faction, r) * player.mods.gather_mult.get(r)
}

pub fn carry_capacity(data: &GameData, player: &PlayerState, u: UnitTypeId) -> f32 {
    data.unit(u).gatherer.as_ref().map_or(0.0, |g| g.capacity + player.mods.carry_add)
}

/// Effective range of a weapon on an entity with `stats`.
pub fn weapon_range(data: &GameData, stats: &crate::components::Stats, w: WeaponId) -> f32 {
    (data.weapon(w).range + stats.range_add).max(0.1)
}

/// Effective cooldown of a weapon on an entity with `stats`.
pub fn weapon_cooldown(data: &GameData, stats: &crate::components::Stats, w: WeaponId) -> f32 {
    (data.weapon(w).cooldown * stats.cooldown_mult).max(0.05)
}

/// Damage after armour (and before shields) of one hit.
pub fn mitigate(amount: f32, dtype: DamageType, armor: &Armor) -> f32 {
    (amount * (1.0 - armor.get(dtype).clamp(0.0, 0.9))).max(0.0)
}
