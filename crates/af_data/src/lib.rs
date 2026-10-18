//! Aetherfall content: the data-driven definitions of everything in the game.
//!
//! * [`schema`] — the serde types authored in `assets/data/**/*.ron`.
//! * [`ids`] — typed indices used at runtime (`UnitTypeId`, `WeaponId`, ...).
//! * [`GameData`] — all content merged, resolved and validated.
//!
//! Load with [`GameData::load_default`] (finds `assets/data` from
//! `BEVY_ASSET_ROOT`, the working directory or the executable's directory) or
//! [`GameData::load_dir`].

pub mod ids;
mod load;
mod resolve;
pub mod schema;

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub use ids::*;
pub use load::find_data_dir;
pub use schema::*;

/// All content errors found while loading/validating, one line each.
#[derive(Debug, Clone)]
pub struct DataErrors(pub Vec<String>);

impl fmt::Display for DataErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} content error(s):", self.0.len())?;
        for e in &self.0 {
            writeln!(f, "  - {e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DataErrors {}

/// All game content, merged from every content file, with every string
/// reference resolved to a typed id and validated.
#[derive(Debug, Clone, Default)]
pub struct GameData {
    pub resources: Vec<ResourceDef>,
    /// Sorted by `index` (ages[0] is age 1).
    pub ages: Vec<AgeDef>,
    pub terrain: Vec<TerrainDef>,
    pub nodes: Vec<NodeDef>,
    pub weapons: Vec<WeaponDef>,
    pub units: Vec<UnitDef>,
    pub buildings: Vec<BuildingDef>,
    pub techs: Vec<TechDef>,
    pub factions: Vec<FactionDef>,

    /// Where each def came from ("<file>"), keyed by "category:id".
    pub sources: HashMap<String, String>,

    pub(crate) resource_ix: HashMap<String, ResourceId>,
    pub(crate) terrain_ix: HashMap<String, TerrainId>,
    pub(crate) node_ix: HashMap<String, NodeTypeId>,
    pub(crate) weapon_ix: HashMap<String, WeaponId>,
    pub(crate) unit_ix: HashMap<String, UnitTypeId>,
    pub(crate) building_ix: HashMap<String, BuildingTypeId>,
    pub(crate) tech_ix: HashMap<String, TechId>,
    pub(crate) faction_ix: HashMap<String, FactionId>,

    /// Mobility flags admitted by each terrain (Flying always included).
    pub(crate) terrain_masks: Vec<Mobility>,
    /// Units trainable at each building (all factions), by BuildingTypeId.
    pub(crate) trains_at: Vec<Vec<UnitTypeId>>,
    /// Techs researchable at each building (all factions), by BuildingTypeId.
    pub(crate) researches_at: Vec<Vec<TechId>>,
}

macro_rules! accessors {
    ($get:ident, $lookup:ident, $field:ident, $ix:ident, $id:ty, $def:ty) => {
        #[inline]
        pub fn $get(&self, id: $id) -> &$def {
            &self.$field[id.index()]
        }
        pub fn $lookup(&self, name: &str) -> Option<$id> {
            self.$ix.get(name).copied()
        }
    };
}

impl GameData {
    /// Load from the default data directory (see [`find_data_dir`]).
    pub fn load_default() -> Result<GameData, DataErrors> {
        let dir = find_data_dir().ok_or_else(|| {
            DataErrors(vec![
                "could not find an `assets/data` directory (set BEVY_ASSET_ROOT to the project root)".into(),
            ])
        })?;
        GameData::load_dir(&dir)
    }

    /// Load every `*.ron` under `dir` (recursively, in path order), merge and validate.
    pub fn load_dir(dir: &Path) -> Result<GameData, DataErrors> {
        let packs = load::read_packs(dir)?;
        GameData::from_packs(packs)
    }

    /// Merge and validate already-parsed packs. `source` names are used in errors.
    pub fn from_packs(packs: Vec<(String, ContentPack)>) -> Result<GameData, DataErrors> {
        resolve::build(packs)
    }

    /// Read and parse every `*.ron` under `dir` without merging, as
    /// (path relative to `dir` with `/` separators, pack). For tools.
    pub fn read_packs(dir: &Path) -> Result<Vec<(String, ContentPack)>, DataErrors> {
        load::read_packs(dir)
    }

    /// Parse one RON string as a pack (for tests and tools).
    pub fn parse_pack(source: &str, text: &str) -> Result<ContentPack, DataErrors> {
        load::parse_pack(source, text)
    }

    accessors!(resource, resource_id, resources, resource_ix, ResourceId, ResourceDef);
    accessors!(terrain_def, terrain_id, terrain, terrain_ix, TerrainId, TerrainDef);
    accessors!(node, node_id, nodes, node_ix, NodeTypeId, NodeDef);
    accessors!(weapon, weapon_id, weapons, weapon_ix, WeaponId, WeaponDef);
    accessors!(unit, unit_id, units, unit_ix, UnitTypeId, UnitDef);
    accessors!(building, building_id, buildings, building_ix, BuildingTypeId, BuildingDef);
    accessors!(tech, tech_id, techs, tech_ix, TechId, TechDef);
    accessors!(faction, faction_id, factions, faction_ix, FactionId, FactionDef);

    /// Age def by 1-based index.
    pub fn age(&self, index: u8) -> &AgeDef {
        &self.ages[(index.clamp(1, schema::MAX_AGE) - 1) as usize]
    }

    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    pub fn resource_ids(&self) -> impl Iterator<Item = ResourceId> + '_ {
        (0..self.resources.len()).map(|i| ResourceId(i as u8))
    }
    pub fn unit_ids(&self) -> impl Iterator<Item = UnitTypeId> + '_ {
        (0..self.units.len()).map(|i| UnitTypeId(i as u16))
    }
    pub fn building_ids(&self) -> impl Iterator<Item = BuildingTypeId> + '_ {
        (0..self.buildings.len()).map(|i| BuildingTypeId(i as u16))
    }
    pub fn tech_ids(&self) -> impl Iterator<Item = TechId> + '_ {
        (0..self.techs.len()).map(|i| TechId(i as u16))
    }
    pub fn faction_ids(&self) -> impl Iterator<Item = FactionId> + '_ {
        (0..self.factions.len()).map(|i| FactionId(i as u8))
    }
    pub fn node_ids(&self) -> impl Iterator<Item = NodeTypeId> + '_ {
        (0..self.nodes.len()).map(|i| NodeTypeId(i as u16))
    }
    pub fn terrain_ids(&self) -> impl Iterator<Item = TerrainId> + '_ {
        (0..self.terrain.len()).map(|i| TerrainId(i as u8))
    }

    /// The mobility of a unit type when owned by `faction`: its locomotion
    /// flag plus every faction trait that applies to that locomotion.
    pub fn unit_mobility(&self, unit: UnitTypeId, faction: FactionId) -> Mobility {
        let u = self.unit(unit);
        let mut m = Mobility::of(u.locomotion);
        for t in &self.faction(faction).traits {
            if t.applies_to.contains(&u.locomotion) {
                m = m.with(t.flag);
            }
        }
        m
    }

    /// Flags admitted by a terrain (always includes Flying).
    pub fn terrain_mask(&self, t: TerrainId) -> Mobility {
        self.terrain_masks[t.index()]
    }

    pub fn terrain_passable(&self, t: TerrainId, m: Mobility) -> bool {
        m.is_flying() || self.terrain_masks[t.index()].intersects(m)
    }

    /// Speed multiplier for mobility `m` on terrain `t` (1.0 for flyers).
    pub fn terrain_speed(&self, t: TerrainId, m: Mobility) -> f32 {
        if m.is_flying() {
            return 1.0;
        }
        let def = &self.terrain[t.index()];
        let mut best: Option<f32> = None;
        for (flag, mult) in &def.speed_by {
            if m.contains(*flag) {
                best = Some(best.map_or(*mult, |b: f32| b.max(*mult)));
            }
        }
        best.unwrap_or(def.speed)
    }

    /// Units `faction` can train at `building` (ignores age and tech gates).
    pub fn trainable_at(&self, building: BuildingTypeId, faction: FactionId) -> Vec<UnitTypeId> {
        let f = self.faction(faction);
        self.trains_at[building.index()]
            .iter()
            .copied()
            .filter(|u| f.unit_ids.contains(u))
            .collect()
    }

    /// Techs `faction` can research at `building` (ignores age and prerequisites).
    pub fn researchable_at(&self, building: BuildingTypeId, faction: FactionId) -> Vec<TechId> {
        let f = self.faction(faction);
        self.researches_at[building.index()]
            .iter()
            .copied()
            .filter(|t| f.tech_ids.contains(t))
            .collect()
    }

    /// Buildings `faction` may construct.
    pub fn buildable_by(&self, faction: FactionId) -> &[BuildingTypeId] {
        &self.faction(faction).building_ids
    }

    /// The age-up tech `faction` researches to go from `from_age` to the next.
    pub fn age_up_tech(&self, faction: FactionId, from_age: u8) -> Option<TechId> {
        self.faction(faction)
            .tech_ids
            .iter()
            .copied()
            .find(|t| {
                let d = self.tech(*t);
                d.age == from_age && d.is_age_up()
            })
    }

    /// Effective gather rate for a worker of `unit` type in `faction` for `res`
    /// (before techs). 0 if the unit cannot gather it.
    pub fn gather_rate(&self, unit: UnitTypeId, faction: FactionId, res: ResourceId) -> f32 {
        match &self.unit(unit).gatherer {
            Some(g) => g.rate_by_resource.get(res) * self.faction(faction).gather_mult.get(res),
            None => 0.0,
        }
    }

    /// Damage multiplier of `weapon` against something with `tags`.
    pub fn bonus_multiplier(&self, weapon: WeaponId, tags: &[Tag]) -> f32 {
        let mut m = 1.0;
        for (tag, mult) in &self.weapon(weapon).bonus {
            if tags.contains(tag) {
                m *= *mult;
            }
        }
        m
    }

    /// Human-readable overview of the content (counts per faction and age).
    pub fn summary(&self) -> String {
        let mut s = String::new();
        use std::fmt::Write;
        let _ = writeln!(
            s,
            "{} resources, {} ages, {} terrain, {} nodes, {} weapons, {} units, {} buildings, {} techs, {} factions",
            self.resources.len(),
            self.ages.len(),
            self.terrain.len(),
            self.nodes.len(),
            self.weapons.len(),
            self.units.len(),
            self.buildings.len(),
            self.techs.len(),
            self.factions.len()
        );
        for f in &self.factions {
            let _ = writeln!(s, "\n== {} — {}", f.name, f.epithet);
            for age in 1..=schema::MAX_AGE {
                let units: Vec<&str> = f
                    .unit_ids
                    .iter()
                    .map(|u| self.unit(*u))
                    .filter(|u| u.age == age)
                    .map(|u| u.name.as_str())
                    .collect();
                let buildings: Vec<&str> = f
                    .building_ids
                    .iter()
                    .map(|b| self.building(*b))
                    .filter(|b| b.age == age)
                    .map(|b| b.name.as_str())
                    .collect();
                let _ = writeln!(
                    s,
                    "  {} | units: {} | buildings: {}",
                    self.age(age).name,
                    units.join(", "),
                    buildings.join(", ")
                );
            }
        }
        s
    }
}

/// Convenience for tools/tests: the data dir as a path.
pub fn data_dir_or(fallback: impl Into<PathBuf>) -> PathBuf {
    find_data_dir().unwrap_or_else(|| fallback.into())
}
