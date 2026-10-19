//! Content definitions as written in `assets/data/**/*.ron`.
//!
//! Every def is referenced from other defs by its string `id` (snake_case).
//! After loading, [`crate::GameData`] resolves those strings into the typed
//! index ids in [`crate::ids`] and fills the `#[serde(skip)]` fields marked
//! "resolved". Content authors never write the resolved fields.
//!
//! Units of measure: distances in world metres (a footman is ~0.8 m wide),
//! times in seconds, speeds in m/s, rates per second, angles in degrees in
//! data (converted where noted).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::*;

/// Maximum number of distinct resources the game supports. Resource amounts
/// are fixed-size arrays indexed by [`ResourceId`].
pub const MAX_RESOURCES: usize = 10;

/// Number of ages. Ages are numbered 1..=MAX_AGE.
pub const MAX_AGE: u8 = 5;

// ---------------------------------------------------------------------------
// Shared value types
// ---------------------------------------------------------------------------

/// An amount of every resource, indexed by [`ResourceId`]. f32 so gathering
/// can accumulate fractions; display with `floor`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResAmounts(pub [f32; MAX_RESOURCES]);

impl ResAmounts {
    pub const ZERO: ResAmounts = ResAmounts([0.0; MAX_RESOURCES]);

    pub fn get(&self, r: ResourceId) -> f32 {
        self.0[r.0 as usize]
    }
    pub fn set(&mut self, r: ResourceId, v: f32) {
        self.0[r.0 as usize] = v;
    }
    pub fn add(&mut self, r: ResourceId, v: f32) {
        self.0[r.0 as usize] += v;
    }
    /// True when every component of `self` is >= the same component of `cost`.
    pub fn covers(&self, cost: &ResAmounts) -> bool {
        self.0.iter().zip(cost.0.iter()).all(|(have, need)| *have + 1e-4 >= *need)
    }
    pub fn add_all(&mut self, other: &ResAmounts) {
        for (a, b) in self.0.iter_mut().zip(other.0.iter()) {
            *a += *b;
        }
    }
    pub fn sub_all(&mut self, other: &ResAmounts) {
        for (a, b) in self.0.iter_mut().zip(other.0.iter()) {
            *a -= *b;
        }
    }
    pub fn scaled(&self, k: f32) -> ResAmounts {
        let mut out = *self;
        for a in out.0.iter_mut() {
            *a *= k;
        }
        out
    }
    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|a| *a == 0.0)
    }
    pub fn total(&self) -> f32 {
        self.0.iter().sum()
    }
}

/// A price, written in RON as a map: `cost: { "food": 50, "wood": 20 }`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "BTreeMap<String, u32>", into = "BTreeMap<String, u32>")]
pub struct Cost {
    /// As authored: resource id -> amount.
    pub entries: BTreeMap<String, u32>,
    /// Resolved: the same amounts indexed by [`ResourceId`].
    pub amounts: ResAmounts,
}

impl From<BTreeMap<String, u32>> for Cost {
    fn from(entries: BTreeMap<String, u32>) -> Self {
        Cost { entries, amounts: ResAmounts::ZERO }
    }
}

impl From<Cost> for BTreeMap<String, u32> {
    fn from(c: Cost) -> Self {
        c.entries
    }
}

/// RGB colour, components 0..=1 in sRGB.
pub type Rgb = [f32; 3];

/// Box dimensions in metres: [width (x), height (y), depth (z)]. Every
/// physical thing in the game is a box; z is the "forward" axis.
pub type BoxSize = [f32; 3];

/// Damage categories. Armour is a fractional resistance per category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DamageType {
    /// Blades, axes, claws.
    Slash,
    /// Arrows, bolts, bullets, spears.
    Pierce,
    /// Hammers, boulders, cannonballs, trampling. Good against buildings.
    Crush,
    /// Fire, explosives, plasma heat.
    Fire,
    /// Lasers, lightning, aether beams. Mostly late ages.
    Energy,
}

/// Fractional damage resistance per [`DamageType`], each in 0.0..=0.9.
/// Damage taken = damage * (1 - resist).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Armor {
    pub slash: f32,
    pub pierce: f32,
    pub crush: f32,
    pub fire: f32,
    pub energy: f32,
}

impl Armor {
    pub fn get(&self, t: DamageType) -> f32 {
        match t {
            DamageType::Slash => self.slash,
            DamageType::Pierce => self.pierce,
            DamageType::Crush => self.crush,
            DamageType::Fire => self.fire,
            DamageType::Energy => self.energy,
        }
    }
    pub fn get_mut(&mut self, t: DamageType) -> &mut f32 {
        match t {
            DamageType::Slash => &mut self.slash,
            DamageType::Pierce => &mut self.pierce,
            DamageType::Crush => &mut self.crush,
            DamageType::Fire => &mut self.fire,
            DamageType::Energy => &mut self.energy,
        }
    }
}

/// Classification tags used for bonus damage, tech selectors, AI and UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tag {
    // --- unit roles ---
    Worker,
    Infantry,
    Cavalry,
    Ranged,
    Melee,
    Siege,
    Mechanical,
    Flying,
    Beast,
    /// Very large unit (giants, titans, walkers).
    Giant,
    Hero,
    Support,
    Scout,
    /// Anti-cavalry polearm/spear troops.
    Spear,
    Light,
    Heavy,
    /// Carries an energy shield.
    Shielded,
    // --- structure roles ---
    Building,
    Wall,
    Gate,
    Tower,
    TownCenter,
    Economic,
    Military,
    House,
    DropSite,
    Wonder,
}

/// How a unit moves, plus the terrain traits factions can grant. A unit's
/// mobility is a bit set of these (see [`crate::Mobility`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MoveFlag {
    // --- locomotion: every unit has exactly one ---
    Foot,
    Mounted,
    Wheeled,
    /// Heavy treads / legs: mechs, tanks.
    Tracked,
    /// Floats a little above ground: crosses water and marsh.
    Hover,
    /// Ignores terrain and ground collision entirely.
    Flying,
    // --- terrain traits: granted by a faction to some locomotions ---
    /// Can wade/swim deep water.
    Amphibious,
    /// Can scale cliffs and rocky highlands.
    Climber,
    /// Moves through thicket/brambles unhindered.
    Forestwalker,
    /// No sand/dune penalty.
    Dunerunner,
    /// No marsh penalty.
    Marshwalker,
}

impl MoveFlag {
    pub fn is_locomotion(self) -> bool {
        matches!(
            self,
            MoveFlag::Foot
                | MoveFlag::Mounted
                | MoveFlag::Wheeled
                | MoveFlag::Tracked
                | MoveFlag::Hover
                | MoveFlag::Flying
        )
    }
}

// ---------------------------------------------------------------------------
// Resources, ages, terrain, resource nodes
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceDef {
    pub id: String,
    pub name: String,
    pub color: Rgb,
    /// Age in which the resource starts to matter (for UI ordering / AI).
    #[serde(default = "one_u8")]
    pub tier: u8,
    #[serde(default)]
    pub description: String,
    /// How a worker gathers it from a resource node (animation and tool).
    /// Never [`WorkStyle::Farm`]: that is how every resource-field
    /// building is worked, whatever it yields.
    #[serde(default)]
    pub work: WorkStyle,
}

/// The way a worker gathers: which work animation it plays and which tool
/// it holds (see `docs/art/peasant.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WorkStyle {
    /// Picking by hand into a basket (berries, fish).
    #[default]
    Forage,
    /// Working a field with a hoe. Reserved for resource-field buildings.
    Farm,
    /// Felling with an axe (trees).
    Chop,
    /// Swinging a pickaxe (stone and ore).
    Mine,
    /// Shovelling (sand).
    Dig,
    /// Crouched chisel-and-mallet work (crystal).
    Chip,
    /// Heavy sledgehammer blows (meteor metal).
    Smash,
}

impl WorkStyle {
    pub const ALL: [WorkStyle; 7] =
        [WorkStyle::Forage, WorkStyle::Farm, WorkStyle::Chop, WorkStyle::Mine, WorkStyle::Dig, WorkStyle::Chip, WorkStyle::Smash];
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgeDef {
    /// 1..=MAX_AGE.
    pub index: u8,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerrainDef {
    pub id: String,
    pub name: String,
    pub color: Rgb,
    /// Visual height of the terrain box top (metres). Ground is 0, water is
    /// slightly negative, cliffs are tall. Purely visual: the simulation is 2D.
    #[serde(default)]
    pub height: f32,
    /// Flags that can enter this terrain. `Flying` can always enter.
    pub passable: Vec<MoveFlag>,
    /// Default speed multiplier for anything that can enter.
    #[serde(default = "one_f32")]
    pub speed: f32,
    /// Per-flag speed overrides; a unit uses the best override it matches,
    /// else `speed`.
    #[serde(default)]
    pub speed_by: Vec<(MoveFlag, f32)>,
    #[serde(default = "true_bool")]
    pub buildable: bool,
}

/// A harvestable thing placed on the map: tree, berry bush, ore vein, crystal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeDef {
    pub id: String,
    pub name: String,
    pub resource: String,
    /// Total amount before depletion.
    pub amount: f32,
    pub size: BoxSize,
    pub color: Rgb,
    /// Whether the node blocks movement and building placement.
    #[serde(default = "true_bool")]
    pub blocking: bool,
    /// Multiplier on worker gather rate at this node.
    #[serde(default = "one_f32")]
    pub gather_mult: f32,
    /// If set, this node cannot be gathered directly; a building of this id
    /// must be placed on top of it (the building then has `trickle` income or
    /// a `resource_field`). Used for contested Aether vents.
    #[serde(default)]
    pub requires_building: Option<String>,

    /// Resolved: `resource`.
    #[serde(skip)]
    pub resource_id: ResourceId,
    /// Resolved: `requires_building`.
    #[serde(skip)]
    pub requires_building_id: Option<BuildingTypeId>,
}

// ---------------------------------------------------------------------------
// Weapons
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum MeleeStyle {
    /// Horizontal sweep across `arc_deg`; can cleave several targets.
    Swing,
    /// Straight jab at one target (spear, lance, rapier).
    Thrust,
    /// Overhead slam; small splash at impact point (hammer, giant fist).
    Smash,
}

/// How a weapon delivers damage. Each kind has a distinct box visual.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WeaponKind {
    /// Close combat. The weapon box animates (swing/thrust/smash) and hits
    /// at the end of `windup`.
    Melee {
        style: MeleeStyle,
        /// Sweep width for Swing (degrees). Ignored by Thrust.
        #[serde(default)]
        arc_deg: f32,
        /// Extra targets a Swing may hit inside the arc.
        #[serde(default)]
        cleave: u32,
    },
    /// Straight, flat projectile that homes on its target's position at
    /// launch time but always hits the locked target (arrows, bullets,
    /// plasma bolts). Simple and reliable.
    Bolt { speed: f32 },
    /// Ballistic lob to the target's *predicted* ground position. Can miss
    /// moving targets; usually has splash (catapult, mortar, grenade).
    Arc {
        speed: f32,
        /// Apex height of the arc as a fraction of the shot distance.
        #[serde(default = "half_f32")]
        apex: f32,
    },
    /// Heavy, fast straight shot to a ground point that damages everything it
    /// passes through, up to `pierce` units, then bursts (cannon, railgun).
    Cannon {
        speed: f32,
        #[serde(default)]
        pierce: u32,
    },
    /// Instant line from muzzle to target, drawn as a stretched glowing box
    /// for `duration` seconds; damage is dealt in `ticks` equal parts over
    /// the duration (laser, aether ray, lightning).
    Beam {
        duration: f32,
        width: f32,
        #[serde(default = "one_u32")]
        ticks: u32,
    },
    /// Dropped from a flyer at its altitude; falls straight down onto the
    /// ground point under the target (bomb, firepot, orbital charge).
    Bomb {
        #[serde(default = "bomb_fall")]
        fall_time: f32,
    },
    /// Guided projectile that steers toward its target (rocket, drone
    /// missile, aether wisp).
    Homing { speed: f32, turn_rate_deg: f32 },
    /// Cone of short-lived particles in front of the unit (flamethrower,
    /// dragon breath, scattergun). Hits everything in the cone.
    Spray { cone_deg: f32, particles: u32 },
    /// Expanding ring around the unit; hits everything within `range`
    /// (shockwave, war-cry, EMP pulse).
    Nova,
    /// Hits the target then jumps to the nearest other enemies.
    Chain { jumps: u32, jump_range: f32 },
}

/// Which kinds of target a weapon may engage.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Targets {
    pub ground: bool,
    pub air: bool,
    pub buildings: bool,
}

impl Default for Targets {
    fn default() -> Self {
        Targets { ground: true, air: false, buildings: true }
    }
}

/// The visible box a unit holds or mounts for a weapon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeaponLook {
    pub size: BoxSize,
    pub color: Rgb,
    /// Mount point relative to the unit centre in unit-local space:
    /// x = right, y = up (from the unit's base), z = forward.
    pub offset: [f32; 3],
}

impl Default for WeaponLook {
    fn default() -> Self {
        WeaponLook { size: [0.08, 0.08, 0.7], color: [0.6, 0.6, 0.65], offset: [0.35, 0.5, 0.2] }
    }
}

/// The visible box for a projectile / beam / particle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectileLook {
    pub size: BoxSize,
    pub color: Rgb,
    /// Render unlit + bright (lasers, plasma, aether).
    pub glow: bool,
}

impl Default for ProjectileLook {
    fn default() -> Self {
        ProjectileLook { size: [0.06, 0.06, 0.5], color: [0.85, 0.8, 0.6], glow: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WeaponDef {
    pub id: String,
    pub name: String,
    pub kind: WeaponKind,
    /// Damage per hit (per projectile for volleys; total over the duration
    /// for beams).
    pub damage: f32,
    pub damage_type: DamageType,
    /// Max engagement range (metres, edge to edge). Melee ~0.5-1.5.
    pub range: f32,
    #[serde(default)]
    pub min_range: f32,
    /// Seconds between attacks.
    pub cooldown: f32,
    /// Seconds from attack start to hit/launch (swing animation, aim).
    #[serde(default)]
    pub windup: f32,
    /// Splash radius (metres) around the impact point. 0 = single target.
    #[serde(default)]
    pub splash: f32,
    #[serde(default)]
    pub targets: Targets,
    /// Damage multipliers against tags, e.g. `[(Cavalry, 2.0), (Building, 0.2)]`.
    /// Multiple matching tags multiply together.
    #[serde(default)]
    pub bonus: Vec<(Tag, f32)>,
    /// Projectiles per attack.
    #[serde(default = "one_u32")]
    pub volley: u32,
    /// Inaccuracy: Arc/Bomb scatter radius in metres; Bolt/Cannon/Spray
    /// angular spread in degrees.
    #[serde(default)]
    pub spread: f32,
    /// Can fire while moving (turrets, horse archers, mechs).
    #[serde(default)]
    pub fire_on_move: bool,
    /// Splash also hurts own/allied units.
    #[serde(default)]
    pub friendly_fire: bool,
    #[serde(default)]
    pub look: WeaponLook,
    #[serde(default)]
    pub projectile: ProjectileLook,
}

// ---------------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GathererDef {
    /// Max carried amount before returning to a drop site.
    pub capacity: f32,
    /// Base gather rate per second, per resource id. Resources not listed
    /// cannot be gathered by this unit.
    pub rates: Vec<(String, f32)>,
    /// Can construct buildings.
    #[serde(default = "true_bool")]
    pub builds: bool,
    /// Repair speed in hp/s (0 = cannot repair).
    #[serde(default)]
    pub repair: f32,

    /// Resolved: `rates`, indexed by ResourceId (0 = cannot gather).
    #[serde(skip)]
    pub rate_by_resource: ResAmounts,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealerDef {
    /// hp per second.
    pub rate: f32,
    pub range: f32,
    /// Heals friendly units having any of these tags.
    pub tags: Vec<Tag>,
}

/// Regenerating energy shield that absorbs damage before hp (late ages).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShieldDef {
    pub hp: f32,
    /// Shield hp per second once regenerating.
    pub regen: f32,
    /// Seconds without taking damage before regen starts.
    #[serde(default = "shield_delay")]
    pub delay: f32,
    /// Fraction of incoming *Energy* damage the shield ignores entirely
    /// (0 = none). Kinetic-shield flavour: slow things pass, fast don't.
    #[serde(default)]
    pub energy_resist: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnitDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Minimum age to train (1..=MAX_AGE).
    pub age: u8,
    pub size: BoxSize,
    /// Accent colour mixed with the owner's faction colour. None = faction colour.
    #[serde(default)]
    pub color: Option<Rgb>,
    pub hp: f32,
    #[serde(default)]
    pub armor: Armor,
    /// m/s on open ground.
    pub speed: f32,
    /// Degrees per second.
    #[serde(default = "turn_rate_default")]
    pub turn_rate: f32,
    /// Exactly one locomotion flag (Foot, Mounted, Wheeled, Tracked, Hover, Flying).
    pub locomotion: MoveFlag,
    /// Altitude for Flying units, float height for Hover (visual + targeting).
    #[serde(default)]
    pub altitude: f32,
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub weapons: Vec<String>,
    pub vision: f32,
    /// Population cost per unit.
    pub pop: u32,
    /// Cost per training (a squad of `squad` units costs this once).
    pub cost: Cost,
    pub train_time: f32,
    /// Building ids that can train this unit.
    pub trained_at: Vec<String>,
    /// Units spawned per training. >1 makes a squadron that selects and
    /// moves together.
    #[serde(default = "one_u32")]
    pub squad: u32,
    #[serde(default)]
    pub gatherer: Option<GathererDef>,
    #[serde(default)]
    pub healer: Option<HealerDef>,
    #[serde(default)]
    pub shield: Option<ShieldDef>,
    /// hp per second.
    #[serde(default)]
    pub regen: f32,

    /// Resolved: `weapons`.
    #[serde(skip)]
    pub weapon_ids: Vec<WeaponId>,
    /// Resolved: `trained_at`.
    #[serde(skip)]
    pub trained_at_ids: Vec<BuildingTypeId>,
}

impl UnitDef {
    pub fn has_tag(&self, t: Tag) -> bool {
        self.tags.contains(&t)
    }
    pub fn is_flying(&self) -> bool {
        self.locomotion == MoveFlag::Flying
    }
}

// ---------------------------------------------------------------------------
// Buildings
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildingUpgrade {
    /// Building id this one becomes (keeps position, hp ratio, queue).
    pub to: String,
    pub cost: Cost,
    pub time: f32,
    /// Resolved: `to`.
    #[serde(skip)]
    pub to_id: BuildingTypeId,
}

/// A building that workers gather from like a node (farms, fish traps).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceFieldDef {
    pub resource: String,
    pub amount: f32,
    /// Max simultaneous gatherers.
    #[serde(default = "one_u32")]
    pub slots: u32,
    #[serde(skip)]
    pub resource_id: ResourceId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildingDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub age: u8,
    pub size: BoxSize,
    #[serde(default)]
    pub color: Option<Rgb>,
    pub hp: f32,
    #[serde(default)]
    pub armor: Armor,
    pub cost: Cost,
    pub build_time: f32,
    pub tags: Vec<Tag>,
    /// Resource ids workers may deposit here.
    #[serde(default)]
    pub drop_site: Vec<String>,
    /// Population capacity provided when complete.
    #[serde(default)]
    pub pop: u32,
    #[serde(default)]
    pub weapons: Vec<String>,
    pub vision: f32,
    #[serde(default)]
    pub upgrade: Option<BuildingUpgrade>,
    /// Placed by click-drag as a line of segments; `size[0]` is the segment
    /// length along the line.
    #[serde(default)]
    pub wall: bool,
    /// Wall segment its owner's (and allies') units can pass through.
    #[serde(default)]
    pub gate: bool,
    /// Units that can garrison inside (reserved; towers/keeps).
    #[serde(default)]
    pub garrison: u32,
    /// Passive income per second while complete: `[("aether", 0.8)]`.
    #[serde(default)]
    pub trickle: Vec<(String, f32)>,
    /// Must be placed on top of a node of this id (and claims it).
    #[serde(default)]
    pub requires_node: Option<String>,
    #[serde(default)]
    pub resource_field: Option<ResourceFieldDef>,
    /// Max number of this building per player.
    #[serde(default)]
    pub limit: Option<u32>,
    /// hp per second (self repair; late tech).
    #[serde(default)]
    pub regen: f32,
    #[serde(default)]
    pub shield: Option<ShieldDef>,

    /// Resolved: `drop_site`.
    #[serde(skip)]
    pub drop_site_ids: Vec<ResourceId>,
    /// Resolved: `weapons`.
    #[serde(skip)]
    pub weapon_ids: Vec<WeaponId>,
    /// Resolved: `trickle`, per second, by resource.
    #[serde(skip)]
    pub trickle_amounts: ResAmounts,
    /// Resolved: `requires_node`.
    #[serde(skip)]
    pub requires_node_id: Option<NodeTypeId>,
}

impl BuildingDef {
    pub fn has_tag(&self, t: Tag) -> bool {
        self.tags.contains(&t)
    }
}

// ---------------------------------------------------------------------------
// Techs
// ---------------------------------------------------------------------------

/// Which of a player's entities a stat effect applies to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Selector {
    AllUnits,
    AllBuildings,
    Unit(String),
    Building(String),
    /// Units *and* buildings carrying this tag.
    Tag(Tag),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Stat {
    Hp,
    Armor(DamageType),
    /// All weapons' damage.
    Damage,
    /// All weapons' range.
    Range,
    /// All weapons' cooldown (use Mul < 1 to attack faster).
    Cooldown,
    Speed,
    Vision,
    TrainTime,
    BuildTime,
    ShieldHp,
    Regen,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Op {
    Add,
    Mul,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    /// Advance the researching player to the next age.
    AdvanceAge,
    Stat {
        target: Selector,
        stat: Stat,
        op: Op,
        value: f32,
    },
    /// Multiply gather rate of a resource for all the player's workers.
    GatherRate { resource: String, mult: f32 },
    /// Add to all the player's workers' carry capacity.
    CarryCapacity { add: f32 },
    /// Multiply all costs of units with this selector (e.g. 0.85).
    CostMult { target: Selector, mult: f32 },
    /// Converts every existing `from` unit to `to` and replaces `from` with
    /// `to` in training menus (AoE-style line upgrade).
    UpgradeUnit { from: String, to: String },
    /// Grants passive income per second.
    Trickle { resource: String, per_sec: f32 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TechDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Minimum age to research (an age-up tech into age N has age N-1).
    pub age: u8,
    pub cost: Cost,
    pub time: f32,
    /// Building id that researches it.
    pub researched_at: String,
    #[serde(default)]
    pub requires: Vec<String>,
    /// Building ids the player must own (complete) to research.
    #[serde(default)]
    pub requires_buildings: Vec<String>,
    pub effects: Vec<Effect>,

    /// Resolved: `researched_at`.
    #[serde(skip)]
    pub researched_at_id: BuildingTypeId,
    /// Resolved: `requires`.
    #[serde(skip)]
    pub requires_ids: Vec<TechId>,
    /// Resolved: `requires_buildings`.
    #[serde(skip)]
    pub requires_building_ids: Vec<BuildingTypeId>,
}

impl TechDef {
    pub fn is_age_up(&self) -> bool {
        self.effects.iter().any(|e| matches!(e, Effect::AdvanceAge))
    }
}

// ---------------------------------------------------------------------------
// Factions
// ---------------------------------------------------------------------------

/// A terrain trait a faction grants to units with the listed locomotions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FactionTrait {
    /// One of Amphibious, Climber, Forestwalker, Dunerunner, Marshwalker.
    pub flag: MoveFlag,
    pub applies_to: Vec<MoveFlag>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StartDef {
    pub resources: Cost,
    pub units: Vec<(String, u32)>,
    /// Pre-built buildings at the start position (first is the main hall).
    pub buildings: Vec<String>,
}

/// Hints for the computer opponent.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AiProfile {
    /// 0 = turtle, 1 = rush.
    pub aggression: f32,
    /// Relative weights for military units the AI prefers (by id).
    pub army: Vec<(String, f32)>,
    /// Resources the AI should prioritise (by id), in order.
    pub eco_focus: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FactionDef {
    pub id: String,
    pub name: String,
    /// One-line flavour, e.g. "Mountain-hold engineers who never retreat".
    pub epithet: String,
    #[serde(default)]
    pub description: String,
    pub color: Rgb,
    #[serde(default)]
    pub traits: Vec<FactionTrait>,
    /// Multipliers on gather rates: `[("stone", 1.25), ("iron", 1.2)]`.
    #[serde(default)]
    pub gather_bonus: Vec<(String, f32)>,
    /// Every unit id this faction may train (common + unique).
    pub units: Vec<String>,
    /// Every building id this faction may build.
    pub buildings: Vec<String>,
    /// Every tech id this faction may research (incl. age-ups).
    pub techs: Vec<String>,
    /// The unit id the AI uses as its worker.
    pub worker: String,
    pub start: StartDef,
    /// Human-readable bonus lines for the UI.
    #[serde(default)]
    pub bonuses: Vec<String>,
    #[serde(default)]
    pub ai: AiProfile,

    /// Resolved: `gather_bonus` (1.0 where not listed).
    #[serde(skip)]
    pub gather_mult: ResAmounts,
    #[serde(skip)]
    pub unit_ids: Vec<UnitTypeId>,
    #[serde(skip)]
    pub building_ids: Vec<BuildingTypeId>,
    #[serde(skip)]
    pub tech_ids: Vec<TechId>,
    #[serde(skip)]
    pub worker_id: UnitTypeId,
}

// ---------------------------------------------------------------------------
// A content file
// ---------------------------------------------------------------------------

/// One `.ron` file. Every field is optional; the loader merges all files.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ContentPack {
    pub resources: Vec<ResourceDef>,
    pub ages: Vec<AgeDef>,
    pub terrain: Vec<TerrainDef>,
    pub nodes: Vec<NodeDef>,
    pub weapons: Vec<WeaponDef>,
    pub units: Vec<UnitDef>,
    pub buildings: Vec<BuildingDef>,
    pub techs: Vec<TechDef>,
    pub factions: Vec<FactionDef>,
}

// --- serde default helpers ---
fn one_u8() -> u8 {
    1
}
fn one_u32() -> u32 {
    1
}
fn one_f32() -> f32 {
    1.0
}
fn half_f32() -> f32 {
    0.5
}
fn true_bool() -> bool {
    true
}
fn turn_rate_default() -> f32 {
    360.0
}
fn bomb_fall() -> f32 {
    0.8
}
fn shield_delay() -> f32 {
    4.0
}
