//! Shared items added by af_economy (other crates may use them).
//! Only af_economy edits this file. Never modify items in other af_core files.

use bevy::prelude::*;

use crate::prelude::*;

/// af_economy's half of the `PlayerCommand` intake (placement, production
/// queues, rallies), inside `SimSet::Commands`. Order after it to see this
/// tick's foundations in [`PlacedSites`].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EconomyIntake;

/// Foundations placed this tick, in placement order, as (index of the
/// placing command in `TickCommands::list`, site). Filled in
/// [`EconomyIntake`], which also gives each command's builders their Build
/// orders, so the tick's later unit commands reach them afterwards, as in
/// inbox order (builders an earlier command of the tick also ordered get
/// theirs in `SimSet::Economy`, once those commands are applied).
#[derive(Resource, Clone, Debug, Default)]
pub struct PlacedSites {
    pub list: Vec<(usize, Entity)>,
}

/// Players whose buildings every other team sees, StarCraft style: a player
/// still in the game who no longer owns a complete
/// [production building](is_production_building) (every production, main
/// and research building is lost) has all of its remaining buildings
/// revealed, so the match can be finished. The reveal lifts once such a
/// building is complete again.
///
/// Written by af_economy with the victory check (once per sim second, after
/// a short grace period) and reset when a match starts. af_fog shows the
/// revealed buildings through `SeenBy` (every team sees them, as if in
/// sight), which is what the renderer, the minimap and the AI read.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct RevealedPlayers {
    /// Sorted by player, without duplicates.
    pub list: Vec<PlayerId>,
}

impl RevealedPlayers {
    /// Are `player`'s buildings revealed?
    pub fn contains(&self, player: PlayerId) -> bool {
        self.list.binary_search(&player).is_ok()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Reveal (`true`) or hide `player`'s buildings; returns whether that
    /// changed anything.
    pub fn set(&mut self, player: PlayerId, revealed: bool) -> bool {
        match (self.list.binary_search(&player), revealed) {
            (Err(i), true) => self.list.insert(i, player),
            (Ok(i), false) => {
                self.list.remove(i);
            }
            _ => return false,
        }
        true
    }
}

/// Can a complete building of `kind` train units or research anything for
/// `faction` (ages not considered)?
pub fn trains_or_researches(data: &GameData, faction: FactionId, kind: BuildingTypeId) -> bool {
    !data.trainable_at(kind, faction).is_empty() || !data.researchable_at(kind, faction).is_empty()
}

/// Is `kind` a production, main or research building for `faction`: one
/// that trains units (town centres included), or researches without being
/// an economic drop site? Camps and mills research gather techs, but they
/// are where workers unload, not what a realm is run from. Losing the last
/// complete one reveals a player (see [`RevealedPlayers`]).
pub fn is_production_building(data: &GameData, faction: FactionId, kind: BuildingTypeId) -> bool {
    let def = data.building(kind);
    !data.trainable_at(kind, faction).is_empty() || (def.drop_site_ids.is_empty() && !data.researchable_at(kind, faction).is_empty())
}

/// Does a complete building of `kind` keep a `faction` player in the game?
///
/// The defeat rule: a player with no living unit and no complete building
/// that matters is defeated. Walls and gates do not matter, nor do pure
/// resource fields, whose only role is to be gathered (a farm): a field
/// that also trains, researches, takes deposits or shoots (a harbour)
/// matters. Foundations never matter: with no unit left to build them they
/// can never be finished.
pub fn keeps_player_alive(data: &GameData, faction: FactionId, kind: BuildingTypeId) -> bool {
    let def = data.building(kind);
    if def.wall || def.gate {
        return false;
    }
    let pure_field =
        def.resource_field.is_some() && def.drop_site_ids.is_empty() && def.weapon_ids.is_empty() && !trains_or_researches(data, faction, kind);
    !pure_field
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revealed_players_stay_sorted() {
        let mut r = RevealedPlayers::default();
        assert!(r.set(PlayerId(3), true));
        assert!(r.set(PlayerId(1), true));
        assert!(!r.set(PlayerId(3), true), "already revealed");
        assert_eq!(r.list, vec![PlayerId(1), PlayerId(3)]);
        assert!(r.contains(PlayerId(3)) && !r.contains(PlayerId(2)));
        assert!(r.set(PlayerId(1), false));
        assert!(!r.set(PlayerId(1), false), "already hidden");
        assert_eq!(r.list, vec![PlayerId(3)]);
    }

    #[test]
    fn walls_gates_and_pure_fields_do_not_keep_a_player_alive() {
        let data = crate::testing::test_data();
        let tide = data.faction_id("tide").expect("tide");
        let alive = |id: &str| keeps_player_alive(&data, tide, data.building_id(id).unwrap_or_else(|| panic!("{id}")));
        for id in ["palisade_wall", "palisade_gate", "farm"] {
            assert!(!alive(id), "{id}");
        }
        // A harbour is a field too, but it also trains, researches and takes deposits.
        for id in ["tide_harbor", "town_center", "house", "watchtower", "mill"] {
            assert!(alive(id), "{id}");
        }
        let production = |id: &str| is_production_building(&data, tide, data.building_id(id).unwrap());
        for id in ["town_center", "barracks", "tide_harbor", "forge", "laboratory"] {
            assert!(production(id), "{id}");
        }
        // The mill and the camps research, but they are drop sites.
        for id in ["house", "farm", "watchtower", "mill", "lumber_camp", "mining_camp", "palisade_wall"] {
            assert!(!production(id), "{id}");
        }
        assert!(trains_or_researches(&data, tide, data.building_id("mill").unwrap()));
    }
}
