//! Aetherfall shared ECS contract.
//!
//! Every gameplay crate depends on this one and talks to the others only
//! through what is defined here:
//!
//! * [`sets`] — schedules, `SimSet`/`ClientSet` ordering, `GameState`, `SIM_DT`.
//! * [`components`] — entity data (each documents which crate writes it).
//! * [`resources`] — singletons: `Data`, `Players`, `Nav`, `MapInfo`, fog, client state.
//! * [`commands`] — `PlayerCommand`: how humans and AI act.
//! * [`messages`] — buffered events: deaths, damage, completions.
//! * [`stats`] — tech-modified stats, costs, times.
//! * [`rules`] — can train / research / build / upgrade (shared by UI, AI, economy).
//! * [`spawn`] — create units/buildings/nodes consistently.
//! * [`placement`] — building placement validity and wall segmentation.
//! * [`spatial`] — per-tick spatial index.
//! * [`rng`] — deterministic RNG.
//! * [`sim_ids`] — entity ids for sim spawns that client spawns cannot shift
//!   (sim code orders and staggers by `Entity`).
//! * [`hash`] — a digest of the match state, for lockstep desync checks.
//!
//! Apps should call `app.set_error_handler(bevy::ecs::error::warn)` before
//! adding plugins, and sim code should prefer `try_insert`/`try_despawn` on
//! entities that may have died.

pub mod commands;
pub mod components;
pub mod ext;
pub mod hash;
pub mod messages;
pub mod placement;
pub mod resources;
pub mod rng;
pub mod rules;
pub mod sets;
pub mod sim_ids;
pub mod spatial;
pub mod spawn;
pub mod stats;
pub mod testing;

use std::sync::atomic::{AtomicU32, Ordering};

use bevy::prelude::*;

pub mod prelude {
    pub use af_data;
    pub use af_geom;
    pub use crate::commands::*;
    pub use crate::components::*;
    pub use crate::messages::*;
    pub use crate::placement::{check_placement, placed_footprint, wall_segments, Placement, PlacementError, FLUSH_TOLERANCE};
    pub use crate::resources::*;
    pub use crate::rng::SimRng;
    pub use crate::rules::{self, Reason};
    pub use crate::sets::*;
    pub use crate::spatial::{SpatialEntry, SpatialIndex, SpatialKind};
    pub use crate::spawn::{exit_point, obb_of, spawn_building, spawn_node, spawn_unit};
    pub use crate::stats::{self, PlayerMods, TagSet};
    pub use af_data::*;
    pub use af_geom::{NavGrid, Obb};
}

use prelude::*;

static SEQ: AtomicU32 = AtomicU32::new(1);

/// A fresh sequence number (MoveGoal::seq etc.).
pub fn next_seq() -> u32 {
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Registers the shared resources, messages and set ordering, and runs the
/// Prepare and Cleanup sets. Add it in every app (headless or windowed).
/// The app must also have states (DefaultPlugins or StatesPlugin) and call
/// `init_state::<GameState>()` or `insert_state(..)` — CorePlugin does
/// `init_state` only if no state exists yet.
pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<State<GameState>>() {
            app.init_state::<GameState>();
        }
        sim_ids::plugin(app);
        app.insert_resource(Time::<Fixed>::from_hz(SIM_HZ))
            .init_resource::<CommandInbox>()
            .init_resource::<TickCommands>()
            .init_resource::<SimClock>()
            .init_resource::<SimRng>()
            .init_resource::<Notices>()
            .init_resource::<SpatialIndex>()
            .init_resource::<FogOfWar>()
            .init_resource::<GameOutcome>()
            .init_resource::<LocalPlayer>()
            .init_resource::<CursorWorld>()
            .init_resource::<InputMode>()
            .init_resource::<CameraRig>()
            .init_resource::<DragSelect>()
            .add_message::<EntityDied>()
            .add_message::<DamageMsg>()
            .add_message::<UnitSpawned>()
            .add_message::<BuildingPlaced>()
            .add_message::<BuildingCompleted>()
            .add_message::<TechResearched>()
            .add_message::<AgeAdvanced>()
            .add_message::<UnderAttack>()
            .configure_sets(
                FixedUpdate,
                (
                    SimSet::Prepare,
                    SimSet::Ai,
                    SimSet::Commands,
                    SimSet::Orders,
                    SimSet::Economy,
                    SimSet::Pathing,
                    SimSet::Movement,
                    SimSet::Combat,
                    SimSet::Projectiles,
                    SimSet::Damage,
                    SimSet::Vision,
                    SimSet::Cleanup,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing).and_then(resource_exists::<Players>).and_then(resource_exists::<Nav>)),
            )
            .configure_sets(Update, (ClientSet::Pointer, ClientSet::Input, ClientSet::Ui, ClientSet::Render).chain())
            .add_systems(
                FixedUpdate,
                (tick_clock, store_prev, drain_commands, recount_players, rebuild_spatial).chain().in_set(SimSet::Prepare),
            )
            .add_systems(FixedUpdate, despawn_dead.in_set(SimSet::Cleanup));
    }
}

fn tick_clock(mut clock: ResMut<SimClock>) {
    clock.tick += 1;
    clock.elapsed += SIM_DT;
}

fn store_prev(
    mut q: Query<(&Position, &mut PrevPosition, &Facing, &mut PrevFacing), Without<Building>>,
    mut fx: Query<&mut FxBox>,
) {
    for (p, mut pp, f, mut pf) in &mut q {
        if pp.0 != p.0 {
            pp.0 = p.0;
        }
        if pf.0 != f.0 {
            pf.0 = f.0;
        }
    }
    for mut b in &mut fx {
        b.prev_pos = b.pos;
    }
}

fn drain_commands(mut inbox: ResMut<CommandInbox>, mut tick: ResMut<TickCommands>) {
    tick.list.clear();
    tick.list.append(&mut inbox.pending);
}

fn recount_players(
    data: Res<Data>,
    mut players: ResMut<Players>,
    units: Query<(&Unit, &Owner), Without<Dead>>,
    buildings: Query<(&Building, &Owner, Has<UnderConstruction>), Without<Dead>>,
) {
    for p in players.list.iter_mut() {
        p.pop_used = 0;
        p.pop_cap = 0;
        p.units_owned.iter_mut().for_each(|c| *c = 0);
        p.buildings_owned.iter_mut().for_each(|c| *c = 0);
        p.buildings_any.iter_mut().for_each(|c| *c = 0);
    }
    for (u, o) in &units {
        if let Some(p) = players.list.get_mut(o.0.index()) {
            p.pop_used += data.unit(u.kind).pop;
            p.units_owned[u.kind.index()] += 1;
        }
    }
    for (b, o, building) in &buildings {
        if let Some(p) = players.list.get_mut(o.0.index()) {
            p.buildings_any[b.kind.index()] += 1;
            if !building {
                p.buildings_owned[b.kind.index()] += 1;
                p.pop_cap += data.building(b.kind).pop;
            }
        }
    }
    for p in players.list.iter_mut() {
        p.pop_cap = p.pop_cap.min(POP_HARD_CAP);
    }
}

#[allow(clippy::type_complexity)]
fn rebuild_spatial(
    data: Res<Data>,
    players: Res<Players>,
    map: Option<Res<MapInfo>>,
    mut index: ResMut<SpatialIndex>,
    units: Query<(Entity, &Unit, &Owner, &Position, &Facing, &Footprint, &Altitude, &Stats, &Mobile), Without<Dead>>,
    buildings: Query<(Entity, &Building, &Owner, &Position, &Facing, &Footprint, &Stats, Has<UnderConstruction>), Without<Dead>>,
    nodes: Query<(Entity, &ResourceNode, &Position, &Facing, &Footprint, Has<Obstacle>), Without<Dead>>,
) {
    if let Some(map) = map
        && map.is_changed()
    {
        index.reset(map.size);
    }
    index.clear();
    // In entity order, not table order (which client-only components on a
    // unit change): `SpatialIndex::nearest` breaks ties by insertion order.
    let mut unit_rows: Vec<_> = units.iter().collect();
    unit_rows.sort_unstable_by_key(|row| row.0);
    for (e, u, o, p, f, fp, alt, st, mob) in unit_rows {
        let obb = fp.obb(p.0, f.0);
        index.insert(SpatialEntry {
            entity: e,
            kind: SpatialKind::Unit,
            type_index: u.kind.0,
            pos: p.0,
            obb,
            radius: obb.bounding_radius(),
            owner: Some(o.0),
            team: players.team(o.0),
            flying: mob.flying,
            altitude: alt.0,
            height: fp.height,
            tags: st.tags,
            under_construction: false,
            blocking: false,
        });
    }
    let mut building_rows: Vec<_> = buildings.iter().collect();
    building_rows.sort_unstable_by_key(|row| row.0);
    for (e, b, o, p, f, fp, st, building) in building_rows {
        let obb = fp.obb(p.0, f.0);
        index.insert(SpatialEntry {
            entity: e,
            kind: SpatialKind::Building,
            type_index: b.kind.0,
            pos: p.0,
            obb,
            radius: obb.bounding_radius(),
            owner: Some(o.0),
            team: players.team(o.0),
            flying: false,
            altitude: 0.0,
            height: fp.height,
            tags: st.tags,
            under_construction: building,
            blocking: true,
        });
    }
    let mut node_rows: Vec<_> = nodes.iter().collect();
    node_rows.sort_unstable_by_key(|row| row.0);
    for (e, n, p, f, fp, blocking) in node_rows {
        let obb = fp.obb(p.0, f.0);
        index.insert(SpatialEntry {
            entity: e,
            kind: SpatialKind::Node,
            type_index: n.kind.0,
            pos: p.0,
            obb,
            radius: obb.bounding_radius(),
            owner: None,
            team: af_geom::NO_TEAM,
            flying: false,
            altitude: 0.0,
            height: fp.height,
            tags: TagSet::default(),
            under_construction: false,
            blocking,
        });
    }
    let _ = &data;
}

/// In entity order, not table order (which client inserts on sim entities
/// change): the order ids are freed in decides which ids are reused.
fn despawn_dead(mut commands: Commands, dead: Query<Entity, With<Dead>>) {
    let mut doomed: Vec<Entity> = dead.iter().collect();
    doomed.sort_unstable();
    for e in doomed {
        commands.entity(e).try_despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flying_bit_matches() {
        assert_eq!(Mobility::of(MoveFlag::Flying).0, af_geom::FLYING);
    }

    #[test]
    fn tagset_roundtrip() {
        let t = TagSet::from_tags(&[Tag::Infantry, Tag::Wonder]);
        assert!(t.contains(Tag::Infantry) && t.contains(Tag::Wonder) && !t.contains(Tag::Cavalry));
    }

    #[test]
    fn wall_segments_line_up() {
        // Uses the real content: palisade_wall must exist.
        let data = GameData::load_dir(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/data")).unwrap();
        let w = data.building_id("palisade_wall").unwrap();
        let segs = placement::wall_segments(&data, w, Vec2::ZERO, Vec2::new(30.0, 0.0));
        let len = data.building(w).size[0];
        assert_eq!(segs.len(), (30.0 / len).round() as usize);
        // yaw puts the box's right axis along +X.
        let r = af_geom::right(segs[0].1);
        assert!((r - Vec2::X).length() < 1e-4, "{r:?}");
    }
}
