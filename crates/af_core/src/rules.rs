//! Shared game rules: what a player may train / research / build / upgrade
//! right now and why not. Used by af_economy (authoritative), af_ui
//! (button state + tooltip) and af_ai (decisions) so they always agree.

use af_data::*;

use crate::resources::{PlayerState, POP_HARD_CAP};
use crate::stats;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Not part of this faction.
    NotInFaction,
    /// Needs this age.
    Age(u8),
    MissingTech(TechId),
    MissingBuilding(BuildingTypeId),
    AlreadyResearched,
    InProgress,
    LimitReached,
    CannotAfford,
    /// Training would exceed population capacity.
    PopCapped,
    /// Replaced by a line upgrade (the newer unit is offered instead).
    Superseded,
    /// The building cannot do this (wrong building, no upgrade, ...).
    WrongBuilding,
    UnderConstruction,
}

impl Reason {
    pub fn describe(&self, data: &GameData) -> String {
        match self {
            Reason::NotInFaction => "Not available to your faction".into(),
            Reason::Age(a) => format!("Requires the {}", data.age(*a).name),
            Reason::MissingTech(t) => format!("Requires {}", data.tech(*t).name),
            Reason::MissingBuilding(b) => format!("Requires a {}", data.building(*b).name),
            Reason::AlreadyResearched => "Already researched".into(),
            Reason::InProgress => "Already being researched".into(),
            Reason::LimitReached => "Build limit reached".into(),
            Reason::CannotAfford => "Not enough resources".into(),
            Reason::PopCapped => "Need more houses".into(),
            Reason::Superseded => "Upgraded".into(),
            Reason::WrongBuilding => "Not here".into(),
            Reason::UnderConstruction => "Still under construction".into(),
        }
    }
    /// Reasons that only depend on resources/pop (button shown but greyed).
    pub fn is_soft(&self) -> bool {
        matches!(self, Reason::CannotAfford | Reason::PopCapped)
    }
}

/// Units the building type offers this player (age not yet reached
/// included, superseded excluded), in data order.
pub fn train_menu(data: &GameData, p: &PlayerState, building: BuildingTypeId) -> Vec<UnitTypeId> {
    let mut out = Vec::new();
    for u in data.trainable_at(building, p.faction) {
        let up = p.mods.upgraded_unit(u);
        let shown = if up != u && data.faction(p.faction).unit_ids.contains(&up) { up } else { u };
        if !out.contains(&shown) {
            out.push(shown);
        }
    }
    out
}

/// May `p` queue `unit` at a complete building of type `building`?
/// (`check_cost` = also require resources and pop room.)
pub fn can_train(data: &GameData, p: &PlayerState, building: BuildingTypeId, unit: UnitTypeId, check_cost: bool) -> Result<(), Reason> {
    let f = data.faction(p.faction);
    let def = data.unit(unit);
    if !f.unit_ids.contains(&unit) {
        return Err(Reason::NotInFaction);
    }
    if !def.trained_at_ids.contains(&building) {
        // Line upgrades are trained where the original was.
        let origin_ok = p.mods.unit_upgrades.iter().any(|(from, to)| *to == unit && data.unit(*from).trained_at_ids.contains(&building));
        if !origin_ok {
            return Err(Reason::WrongBuilding);
        }
    }
    if p.mods.upgraded_unit(unit) != unit {
        return Err(Reason::Superseded);
    }
    if p.age < def.age {
        return Err(Reason::Age(def.age));
    }
    if check_cost {
        if !p.can_afford(&stats::unit_cost(data, p, unit)) {
            return Err(Reason::CannotAfford);
        }
        if p.pop_used + def.pop * def.squad > p.pop_cap.min(POP_HARD_CAP) {
            return Err(Reason::PopCapped);
        }
    }
    Ok(())
}

/// Techs the building type offers this player, excluding researched ones.
pub fn research_menu(data: &GameData, p: &PlayerState, building: BuildingTypeId) -> Vec<TechId> {
    data.researchable_at(building, p.faction).into_iter().filter(|t| !p.has_tech(*t)).collect()
}

pub fn can_research(data: &GameData, p: &PlayerState, building: BuildingTypeId, tech: TechId, check_cost: bool) -> Result<(), Reason> {
    let f = data.faction(p.faction);
    let def = data.tech(tech);
    if !f.tech_ids.contains(&tech) {
        return Err(Reason::NotInFaction);
    }
    if def.researched_at_id != building {
        return Err(Reason::WrongBuilding);
    }
    if p.has_tech(tech) {
        return Err(Reason::AlreadyResearched);
    }
    if p.researching.get(tech.index()).copied().unwrap_or(false) {
        return Err(Reason::InProgress);
    }
    if def.is_age_up() {
        // Age-ups are only offered in exactly their age.
        if p.age != def.age {
            return Err(if p.age < def.age { Reason::Age(def.age) } else { Reason::AlreadyResearched });
        }
        // Only one age-up at a time.
        if f.tech_ids.iter().any(|t| data.tech(*t).is_age_up() && p.researching[t.index()]) {
            return Err(Reason::InProgress);
        }
    } else if p.age < def.age {
        return Err(Reason::Age(def.age));
    }
    for r in &def.requires_ids {
        if !p.has_tech(*r) {
            return Err(Reason::MissingTech(*r));
        }
    }
    for b in &def.requires_building_ids {
        if p.buildings_owned.get(b.index()).copied().unwrap_or(0) == 0 {
            return Err(Reason::MissingBuilding(*b));
        }
    }
    if check_cost && !p.can_afford(&def.cost.amounts) {
        return Err(Reason::CannotAfford);
    }
    Ok(())
}

/// Buildings this faction can place (the "build" menu), in data order.
pub fn build_menu(data: &GameData, p: &PlayerState) -> Vec<BuildingTypeId> {
    let f = data.faction(p.faction);
    // Hide buildings that are only reachable by upgrading another one
    // (e.g. stone_wall comes from palisade_wall) unless nothing upgrades to
    // them within the faction's list.
    f.building_ids
        .iter()
        .copied()
        .filter(|b| {
            !f.building_ids.iter().any(|o| data.building(*o).upgrade.as_ref().is_some_and(|u| u.to_id == *b))
                || data.building(*b).wall
        })
        .collect()
}

pub fn can_build(data: &GameData, p: &PlayerState, building: BuildingTypeId, check_cost: bool) -> Result<(), Reason> {
    let f = data.faction(p.faction);
    let def = data.building(building);
    if !f.building_ids.contains(&building) {
        return Err(Reason::NotInFaction);
    }
    if p.age < def.age {
        return Err(Reason::Age(def.age));
    }
    if let Some(limit) = def.limit
        && p.buildings_any.get(building.index()).copied().unwrap_or(0) >= limit
    {
        return Err(Reason::LimitReached);
    }
    if check_cost && !p.can_afford(&stats::building_cost(data, p, building)) {
        return Err(Reason::CannotAfford);
    }
    Ok(())
}

/// May a complete building of type `from` be upgraded now? Returns the target type.
pub fn can_upgrade(data: &GameData, p: &PlayerState, from: BuildingTypeId, check_cost: bool) -> Result<BuildingTypeId, Reason> {
    let def = data.building(from);
    let Some(up) = &def.upgrade else { return Err(Reason::WrongBuilding) };
    let to = data.building(up.to_id);
    if !data.faction(p.faction).building_ids.contains(&up.to_id) {
        return Err(Reason::NotInFaction);
    }
    if p.age < to.age {
        return Err(Reason::Age(to.age));
    }
    if check_cost && !p.can_afford(&up.cost.amounts) {
        return Err(Reason::CannotAfford);
    }
    Ok(up.to_id)
}
