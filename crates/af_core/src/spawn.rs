//! The only way to create units, buildings and resource nodes, so every
//! crate (map setup, production, tests) gets identical component sets.
//! Functions take `&mut Commands`; the entity exists after commands apply.
//!
//! Visual crates add their own components when they see `Added<Unit>` etc.

use af_data::*;
use af_geom::Obb;
use bevy::prelude::*;

use crate::components::*;
use crate::resources::PlayerState;
use crate::spatial::{SpatialIndex, SpatialKind};
use crate::stats;

/// Make a unit. Squad membership is added by the caller (`SquadMember`).
pub fn spawn_unit(commands: &mut Commands, data: &GameData, player: &PlayerState, kind: UnitTypeId, pos: Vec2, facing: f32) -> Entity {
    let def = data.unit(kind);
    let st = stats::unit_stats(data, player, kind);
    let flying = def.is_flying();
    let footprint = Footprint::from_size(def.size);
    let mut e = commands.spawn((
        Name::new(def.name.clone()),
        Unit { kind },
        Owner(player.id),
        (Position(pos), PrevPosition(pos), Facing(facing), PrevFacing(facing)),
        Altitude(def.altitude),
        footprint,
        Velocity::default(),
        Mobile {
            mobility: data.unit_mobility(kind, player.faction),
            radius: footprint.half.x.max(0.2),
            turn_rate: def.turn_rate.to_radians(),
            flying,
        },
        Health::full(st.max_hp),
        Weapons { slots: def.weapon_ids.iter().map(|w| WeaponSlot::new(*w)).collect() },
        Orders::default(),
        if def.gatherer.is_some() { Stance::Passive } else { Stance::Aggressive },
        PathState::default(),
        Idle,
    ));
    if st.shield_max > 0.0 {
        let s = def.shield.as_ref();
        e.insert(Shield {
            hp: st.shield_max,
            max: st.shield_max,
            regen: s.map_or(0.0, |s| s.regen),
            delay: s.map_or(4.0, |s| s.delay),
            energy_resist: s.map_or(0.0, |s| s.energy_resist),
            since_hit: 999.0,
        });
    }
    if def.gatherer.is_some() {
        e.insert(Gatherer {
            carrying: None,
            amount: 0.0,
            capacity: stats::carry_capacity(data, player, kind),
            target: None,
            last_node_kind: None,
            last_pos: pos,
        });
    }
    e.insert(st);
    e.id()
}

/// Make a building. `complete = false` makes a foundation (UnderConstruction,
/// low hp) that builders finish. The caller has already validated placement.
pub fn spawn_building(commands: &mut Commands, data: &GameData, player: &PlayerState, kind: BuildingTypeId, pos: Vec2, facing: f32, complete: bool) -> Entity {
    let def = data.building(kind);
    let st = stats::building_stats(data, player, kind);
    let footprint = Footprint::from_size(def.size);
    let obb = footprint.obb(pos, facing);
    let hp = if complete { st.max_hp } else { (st.max_hp * 0.05).max(1.0) };
    let mut e = commands.spawn((
        Name::new(def.name.clone()),
        Building { kind },
        Owner(player.id),
        (Position(pos), PrevPosition(pos), Facing(facing), PrevFacing(facing)),
        Altitude(0.0),
        footprint,
        Obstacle { obb, gate_team: if def.gate { Some(player.team) } else { None } },
        Health { hp, max: st.max_hp },
        Weapons { slots: def.weapon_ids.iter().map(|w| WeaponSlot::new(*w)).collect() },
        ProductionQueue::default(),
        Rally::default(),
    ));
    if !complete {
        e.insert(UnderConstruction { progress: 0.0, build_time: stats::building_build_time(data, player, kind) });
    }
    if !def.drop_site_ids.is_empty() {
        let mut mask = 0u16;
        for r in &def.drop_site_ids {
            mask |= 1 << r.0;
        }
        e.insert(DropSite { mask });
    }
    if let Some(f) = &def.resource_field {
        e.insert(ResourceField { resource: f.resource_id, remaining: f.amount, slots: f.slots, users: 0 });
    }
    if def.wall {
        e.insert(Wall);
    }
    if def.gate {
        e.insert(Gate);
    }
    if !def.trickle_amounts.is_zero() {
        e.insert(Trickle(def.trickle_amounts));
    }
    if st.shield_max > 0.0 {
        let s = def.shield.as_ref();
        e.insert(Shield {
            hp: if complete { st.shield_max } else { 0.0 },
            max: st.shield_max,
            regen: s.map_or(0.0, |s| s.regen),
            delay: s.map_or(4.0, |s| s.delay),
            energy_resist: s.map_or(0.0, |s| s.energy_resist),
            since_hit: 999.0,
        });
    }
    e.insert(st);
    e.id()
}

/// Make a resource node with its full amount (or `amount` if given).
pub fn spawn_node(commands: &mut Commands, data: &GameData, kind: NodeTypeId, pos: Vec2, facing: f32, amount: Option<f32>) -> Entity {
    let def = data.node(kind);
    let footprint = Footprint::from_size(def.size);
    let mut e = commands.spawn((
        Name::new(def.name.clone()),
        ResourceNode { kind, resource: def.resource_id, remaining: amount.unwrap_or(def.amount), claimed_by: None },
        (Position(pos), PrevPosition(pos), Facing(facing), PrevFacing(facing)),
        Altitude(0.0),
        footprint,
    ));
    if def.blocking {
        e.insert(Obstacle { obb: footprint.obb(pos, facing), gate_team: None });
    }
    e.id()
}

/// Box of an entity from its components.
#[inline]
pub fn obb_of(pos: &Position, facing: &Facing, fp: &Footprint) -> Obb {
    Obb::new(pos.0, fp.half, facing.0)
}

/// A free spot next to a building's footprint to place a freshly trained
/// unit, preferring the side facing `toward`. Uses the nav grid only (static
/// obstacles and terrain): units standing there are not seen, so set fresh
/// units down with [`clear_exit_point`].
pub fn exit_point(nav: &af_geom::NavGrid, building: &Obb, toward: Vec2, mobility: Mobility, radius: f32, team: u8) -> Vec2 {
    let dir = (toward - building.center).normalize_or(af_geom::forward(building.yaw));
    let reach = building.support(dir) + radius + 0.6;
    let want = building.center + dir * reach;
    nav.nearest_open(want, mobility.0, radius, team, 12.0).unwrap_or(want)
}

/// Room a fresh unit keeps from the boxes of units standing where it is set
/// down (metres): they may walk about this far in the tick it appears (the
/// fastest walker covers 0.3 m a tick), so it neither appears on them nor
/// is walked into before the collision solver has seen it.
pub const SPAWN_MARGIN: f32 = 0.3;
/// Room kept from units set down earlier in the same tick (metres): they
/// stand still until their orders reach them next tick.
const SPAWN_SKIN: f32 = 0.05;
/// Gap between a building and the side of a fresh unit facing it (metres).
const SPAWN_GAP: f32 = 0.3;
/// Farthest from its preferred spot a fresh unit is set down (metres).
pub const SPAWN_SEARCH: f32 = 10.0;
/// Spots tried around the preferred one: this far apart close to it...
const SPAWN_STEP_NEAR: f32 = 0.5;
/// ...within this distance (metres), and this far apart beyond.
const SPAWN_NEAR: f32 = 3.0;
const SPAWN_STEP_FAR: f32 = 1.0;

/// A unit about to be set down: its box (half extents and facing) and how
/// it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Newcomer {
    pub half: Vec2,
    pub yaw: f32,
    pub mobility: Mobility,
    pub team: u8,
}

impl Newcomer {
    /// A unit of type `kind` for `player`, facing `yaw`.
    pub fn of(data: &GameData, player: &PlayerState, kind: UnitTypeId, yaw: f32) -> Newcomer {
        Newcomer { half: Footprint::from_size(data.unit(kind).size).half, yaw, mobility: data.unit_mobility(kind, player.faction), team: player.team }
    }

    #[inline]
    fn flying(&self) -> bool {
        self.mobility.0 & af_geom::FLYING != 0
    }

    /// The clearance radius its `Mobile` gets (see [`spawn_unit`]).
    #[inline]
    fn radius(&self) -> f32 {
        self.half.x.max(0.2)
    }
}

/// Offsets of the spots [`clear_spot`] tries, nearest first: a fine lattice
/// near the preferred spot, a coarser one out to [`SPAWN_SEARCH`].
fn spawn_offsets() -> &'static [Vec2] {
    static OFFSETS: std::sync::OnceLock<Vec<Vec2>> = std::sync::OnceLock::new();
    OFFSETS.get_or_init(|| {
        let lattice = |step: f32, keep: &dyn Fn(f32) -> bool| {
            let n = (SPAWN_SEARCH / step).floor() as i32;
            (-n..=n).flat_map(move |y| (-n..=n).map(move |x| Vec2::new(x as f32, y as f32) * step)).filter(|d| keep(d.length())).collect::<Vec<_>>()
        };
        let mut offsets = lattice(SPAWN_STEP_NEAR, &|d| d <= SPAWN_NEAR + 1e-4);
        offsets.extend(lattice(SPAWN_STEP_FAR, &|d| d > SPAWN_NEAR + 1e-4 && d <= SPAWN_SEARCH + 1e-4));
        offsets.sort_by(|p, q| p.length_squared().total_cmp(&q.length_squared()).then(p.x.total_cmp(&q.x)).then(p.y.total_cmp(&q.y)));
        offsets
    })
}

/// The spot nearest `want` (within [`SPAWN_SEARCH`]) where `unit` may
/// appear without overlapping anything: its box inside the map; for ground
/// units, room for it on the nav grid ([`af_geom::NavGrid::clear_at`] with its
/// clearance radius) and its whole box clear of terrain it cannot enter and
/// of every building and blocking node in `spatial`; and its box at least
/// [`SPAWN_MARGIN`] from every unit of its layer (ground or air) in
/// `spatial` (where everything stood when the tick began) and clear of the
/// boxes in `placed` (units set down earlier this tick: `(box, flying)`).
/// `None` when there is no such spot: wait rather than stack units.
pub fn clear_spot(nav: &af_geom::NavGrid, spatial: &SpatialIndex, placed: &[(Obb, bool)], unit: &Newcomer, want: Vec2) -> Option<Vec2> {
    spawn_offsets().iter().map(|d| want + *d).find(|&at| fits_fresh(nav, spatial, placed, unit, at))
}

/// [`clear_spot`] next to `building`, on its side facing `toward` (else its
/// front): the preferred spot is where the unit's box, facing `unit.yaw`,
/// stands [`SPAWN_GAP`] off the building's face.
pub fn clear_exit_point(nav: &af_geom::NavGrid, spatial: &SpatialIndex, placed: &[(Obb, bool)], building: &Obb, toward: Vec2, unit: &Newcomer) -> Option<Vec2> {
    let dir = exit_direction(building, toward);
    let reach = building.support(dir) + Obb::new(Vec2::ZERO, unit.half, unit.yaw).support(-dir) + SPAWN_GAP;
    clear_spot(nav, spatial, placed, unit, building.center + dir * reach)
}

/// The way out of `building` toward `toward` (its front when `toward` is
/// its centre): a fresh unit faces it.
pub fn exit_direction(building: &Obb, toward: Vec2) -> Vec2 {
    (toward - building.center).normalize_or(af_geom::forward(building.yaw))
}

/// May `unit` appear at `at`? See [`clear_spot`].
fn fits_fresh(nav: &af_geom::NavGrid, spatial: &SpatialIndex, placed: &[(Obb, bool)], unit: &Newcomer, at: Vec2) -> bool {
    let me = Obb::new(at, unit.half, unit.yaw);
    let (min, max) = me.aabb();
    let (lo, hi) = (nav.origin(), nav.origin() + nav.world_size());
    if min.x < lo.x || min.y < lo.y || max.x > hi.x || max.y > hi.y {
        return false;
    }
    let flying = unit.flying();
    if !flying && (!nav.clear_at(at, unit.mobility.0, unit.radius(), unit.team) || on_forbidden_terrain(nav, &me, unit.mobility.0)) {
        return false;
    }
    let roomy = Obb { half: unit.half + Vec2::splat(SPAWN_MARGIN), ..me };
    let snug = Obb { half: unit.half + Vec2::splat(SPAWN_SKIN), ..me };
    let mut clear = placed.iter().all(|(o, fly)| *fly != flying || !snug.overlaps(o));
    if clear {
        spatial.for_each_near(at, me.bounding_radius() + SPAWN_MARGIN, |e| {
            clear &= match e.kind {
                SpatialKind::Unit => e.flying != flying || !roomy.overlaps(&e.obb),
                SpatialKind::Building | SpatialKind::Node => flying || !e.blocking || !me.overlaps(&e.obb),
            };
        });
    }
    clear
}

/// Does the box `me` overlap a nav cell whose terrain `mobility` cannot enter?
fn on_forbidden_terrain(nav: &af_geom::NavGrid, me: &Obb, mobility: u16) -> bool {
    let (min, max) = me.aabb();
    let (c0, c1) = (nav.cell_of(min), nav.cell_of(max));
    let half = Vec2::splat(nav.cell_size() * 0.5);
    (c0.y..=c1.y).any(|y| {
        (c0.x..=c1.x).any(|x| {
            let c = IVec2::new(x, y);
            !nav.terrain_passable(c, mobility) && me.overlaps(&Obb::new(nav.cell_center(c), half, 0.0))
        })
    })
}

/// How often production found no room to set a trained unit down and
/// waited instead (see `clear_exit_point`), for tests and reports. Kept by
/// af_economy.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpawnStats {
    /// Trainings set down.
    pub spawned: u64,
    /// Attempts that found no room (the training waited).
    pub waited: u64,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::spatial::SpatialEntry;
    use crate::stats::TagSet;
    use crate::testing::{new_nav, test_data};
    use af_geom::NavGrid;

    /// A peasant-sized ground newcomer facing +Z.
    fn peasant() -> Newcomer {
        Newcomer { half: Vec2::splat(0.3), yaw: 0.0, mobility: Mobility::of(MoveFlag::Foot), team: 0 }
    }

    fn unit_entry(i: u32, obb: Obb, flying: bool) -> SpatialEntry {
        SpatialEntry {
            entity: Entity::from_raw_u32(i).expect("entity"),
            kind: SpatialKind::Unit,
            type_index: 0,
            pos: obb.center,
            obb,
            radius: obb.bounding_radius(),
            owner: Some(PlayerId(0)),
            team: 0,
            flying,
            altitude: 0.0,
            height: 1.0,
            tags: TagSet::default(),
            under_construction: false,
            blocking: false,
        }
    }

    fn building_entry(i: u32, obb: Obb) -> SpatialEntry {
        SpatialEntry { kind: SpatialKind::Building, blocking: true, owner: Some(PlayerId(0)), ..unit_entry(i, obb, false) }
    }

    fn world() -> (Arc<GameData>, NavGrid) {
        let data = test_data();
        let nav = new_nav(&data, 64, 64, data.terrain_id("grass").expect("grass"));
        (data, nav)
    }

    /// Out in the open, the preferred spot itself; a unit standing there
    /// (or within the margin of it) pushes the newcomer to the nearest spot
    /// clear of it by the margin, and one set down earlier this tick does too.
    #[test]
    fn a_fresh_unit_keeps_clear_of_units_standing_there() {
        let (_, nav) = world();
        let unit = peasant();
        let mut spatial = SpatialIndex::new(Vec2::splat(64.0), 8.0);
        let want = Vec2::new(30.0, 30.0);
        assert_eq!(clear_spot(&nav, &spatial, &[], &unit, want), Some(want));

        let standing = Obb::new(want + Vec2::new(0.2, 0.0), Vec2::new(0.45, 0.95), 0.0);
        spatial.insert(unit_entry(1, standing, false));
        let at = clear_spot(&nav, &spatial, &[], &unit, want).expect("room nearby");
        let me = Obb::new(at, unit.half, unit.yaw);
        assert!(me.gap(&standing) >= SPAWN_MARGIN - 1e-4, "{at} gap {}", me.gap(&standing));
        assert!(at.distance(want) <= 1.6, "nearest clear spot: {at}");

        // A flyer overhead is no obstacle to a walker.
        let mut air = SpatialIndex::new(Vec2::splat(64.0), 8.0);
        air.insert(unit_entry(2, Obb::new(want, Vec2::splat(1.0), 0.0), true));
        assert_eq!(clear_spot(&nav, &air, &[], &unit, want), Some(want));

        // A model set down earlier this tick.
        let placed = [(Obb::new(want, unit.half, 0.0), false)];
        let next = clear_spot(&nav, &SpatialIndex::new(Vec2::splat(64.0), 8.0), &placed, &unit, want).expect("room beside it");
        assert!(!Obb::new(next, unit.half, 0.0).overlaps(&placed[0].0) && next.distance(want) <= 1.0 + 1e-4, "{next}");
    }

    /// Next to a building, the unit stands off the face toward the rally;
    /// a crowd there sends it round the side; a building boxed in by units
    /// on every side within the search gives no spot at all.
    #[test]
    fn exits_go_round_crowds_and_wait_when_boxed_in() {
        let (_, mut nav) = world();
        let unit = Newcomer { half: Vec2::new(0.9, 1.6), ..peasant() }; // a ram
        let hall = Obb::new(Vec2::new(32.0, 32.0), Vec2::splat(3.0), 0.0);
        nav.add_obstacle(&hall, 1);
        let mut spatial = SpatialIndex::new(Vec2::splat(64.0), 8.0);
        spatial.insert(building_entry(100, hall));
        let east = Vec2::new(50.0, 32.0);
        let unit = Newcomer { yaw: af_geom::yaw_of(exit_direction(&hall, east)), ..unit };
        let at = clear_exit_point(&nav, &spatial, &[], &hall, east, &unit).expect("open ground");
        let me = Obb::new(at, unit.half, unit.yaw);
        assert!(at.x > 35.0 && (at.y - 32.0).abs() < 0.6 && !me.overlaps(&hall), "east of the hall: {at}");

        // Riders packed in front of the east face.
        let mut i = 1;
        for y in [26.0, 28.0, 30.0, 32.0, 34.0, 36.0, 38.0] {
            for x in [36.0, 37.2, 38.4, 39.6] {
                spatial.insert(unit_entry(i, Obb::new(Vec2::new(x, y), Vec2::new(0.45, 0.95), 0.0), false));
                i += 1;
            }
        }
        let at = clear_exit_point(&nav, &spatial, &[], &hall, east, &unit).expect("room round the crowd");
        let me = Obb::new(at, unit.half, unit.yaw);
        let crowd_gap = spatial.entries.iter().filter(|e| e.kind == SpatialKind::Unit).map(|e| me.gap(&e.obb)).fold(f32::INFINITY, f32::min);
        assert!(crowd_gap >= SPAWN_MARGIN - 1e-4 && !me.overlaps(&hall), "{at}: {crowd_gap}");

        // Wall-to-wall units over the whole search area.
        let mut packed = SpatialIndex::new(Vec2::splat(64.0), 8.0);
        packed.insert(building_entry(100, hall));
        for y in 0..32 {
            for x in 0..32 {
                packed.insert(unit_entry(1000 + y * 32 + x, Obb::new(Vec2::new(16.5 + x as f32, 16.5 + y as f32), Vec2::splat(0.3), 0.0), false));
            }
        }
        assert_eq!(clear_exit_point(&nav, &packed, &[], &hall, east, &unit), None);
    }

    /// Ground units never appear on terrain they cannot enter, nor with
    /// their box over it or off the map.
    #[test]
    fn fresh_units_stay_on_their_ground_and_on_the_map() {
        let (data, mut nav) = world();
        let water = data.terrain_ids().find(|t| data.terrain_mask(*t).0 & Mobility::of(MoveFlag::Foot).0 == 0).expect("terrain foot units cannot enter");
        for y in 0..64 {
            for x in 31..64 {
                nav.set_terrain(IVec2::new(x, y), water.0);
            }
        }
        let unit = peasant();
        let spatial = SpatialIndex::new(Vec2::splat(64.0), 8.0);
        let at = clear_spot(&nav, &spatial, &[], &unit, Vec2::new(31.2, 20.0)).expect("dry land nearby");
        assert!(at.x + unit.half.x <= 31.0 + 1e-4, "{at}");
        let at = clear_spot(&nav, &spatial, &[], &unit, Vec2::new(0.1, 0.1)).expect("inside the map");
        assert!(at.x >= unit.half.x && at.y >= unit.half.y, "{at}");
    }
}
