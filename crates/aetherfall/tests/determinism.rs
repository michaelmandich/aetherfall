//! The simulation replays identically whatever drives it.
//!
//! * No two `FixedUpdate` systems that touch the same data run in an
//!   unspecified order, so the multi-threaded executor cannot change a
//!   match (Bevy's ambiguity detection, applied to the whole sim).
//! * How many sim ticks run per frame does not matter: a windowed game runs
//!   one or two per frame, the headless harness thirty.
//! * What the client puts on sim entities does not matter: a marker
//!   component moves an entity to another archetype, which changes the
//!   order queries visit entities in, and no sim system depends on that
//!   order.
//!
//! `matches.rs` checks that the same seed replays identically.

use std::collections::HashSet;

use aetherfall::*;
use af_core::prelude::*;
use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
use bevy::prelude::*;

fn two_player_match(ticks_per_update: u32) -> App {
    let data = af_core::testing::test_data();
    let opts = SkirmishOptions { seed: 4, players: 2, factions: vec!["ironhold".into(), "dune".into()], human: false, ..Default::default() };
    headless_app(data.clone(), skirmish_setup(&data, &opts).expect("setup"), ticks_per_update)
}

/// Bevy's report of every pair of `FixedUpdate` systems with conflicting
/// data access and no ordering between them (empty when there are none).
fn fixed_update_ambiguities() -> String {
    let mut app = two_player_match(1);
    let world = app.world_mut();
    let mut schedule = world.resource_mut::<Schedules>().remove(FixedUpdate).expect("FixedUpdate");
    schedule.set_build_settings(ScheduleBuildSettings { ambiguity_detection: LogLevel::Error, ..default() });
    match schedule.initialize(world) {
        Ok(_) => String::new(),
        Err(e) => e.to_string(schedule.graph(), world),
    }
}

#[test]
fn sim_systems_touching_the_same_data_are_ordered() {
    let report = fixed_update_ambiguities();
    assert!(report.is_empty(), "{report}");
}

/// A unit as (entity, type, position x, position z, facing, hp), bit for bit.
type UnitBits = (u64, u16, u32, u32, u32, u32);
/// A building as (entity, type, hp, construction progress), bit for bit.
type BuildingBits = (u64, u16, u32, u32);

/// The match, bit for bit: every unit (entity, type, position, facing, hp),
/// every building (entity, type, hp, construction progress), every resource
/// node (entity, what is left) and every player's resources.
#[derive(Debug, PartialEq)]
struct MatchState {
    units: Vec<UnitBits>,
    buildings: Vec<BuildingBits>,
    nodes: Vec<(u64, u32)>,
    resources: Vec<[u32; MAX_RESOURCES]>,
}

impl MatchState {
    /// Where `other` differs, briefly (how much of each list agrees).
    fn differences(&self, other: &MatchState) -> String {
        fn agree<T: PartialEq>(a: &[T], b: &[T]) -> String {
            format!("{} of {}/{} agree", a.iter().zip(b).filter(|(x, y)| x == y).count(), a.len(), b.len())
        }
        format!(
            "units: {}; buildings: {}; nodes: {}; resources: {}",
            agree(&self.units, &other.units),
            agree(&self.buildings, &other.buildings),
            agree(&self.nodes, &other.nodes),
            agree(&self.resources, &other.resources)
        )
    }
}

fn state(app: &mut App) -> MatchState {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(Entity, &Unit, &Position, &Facing, &Health), Without<Dead>>();
    let unit = |(e, u, p, f, h): (Entity, &Unit, &Position, &Facing, &Health)| {
        (e.to_bits(), u.kind.0, p.0.x.to_bits(), p.0.y.to_bits(), f.0.to_bits(), h.hp.to_bits())
    };
    let mut units: Vec<_> = q.iter(world).map(unit).collect();
    units.sort_unstable();
    let mut q = world.query_filtered::<(Entity, &Building, &Health, Option<&UnderConstruction>), Without<Dead>>();
    let mut buildings: Vec<_> =
        q.iter(world).map(|(e, b, h, c)| (e.to_bits(), b.kind.0, h.hp.to_bits(), c.map_or(1.0, |c| c.progress).to_bits())).collect();
    buildings.sort_unstable();
    let mut q = world.query_filtered::<(Entity, &ResourceNode), Without<Dead>>();
    let mut nodes: Vec<_> = q.iter(world).map(|(e, n)| (e.to_bits(), n.remaining.to_bits())).collect();
    nodes.sort_unstable();
    let resources = world.resource::<Players>().list.iter().map(|p| p.resources.0.map(f32::to_bits)).collect();
    MatchState { units, buildings, nodes, resources }
}

#[test]
fn ticks_per_frame_do_not_change_the_match() {
    let ticks = 60 * SIM_HZ as u64;
    let mut states = Vec::new();
    for ticks_per_update in [1, 30] {
        let mut app = two_player_match(ticks_per_update);
        run_headless(&mut app, ticks, 0, |_| {});
        assert_eq!(app.world().resource::<SimClock>().tick, ticks);
        states.push(state(&mut app));
    }
    assert!(states[0] == states[1], "one tick per frame and thirty diverge: {}", states[0].differences(&states[1]));
}

/// Stands in for what a windowed client puts on every sim entity it draws
/// (a visual's marker), and on some (`Selected`): empty, but it moves the
/// entity to another archetype.
#[derive(Component)]
struct Drawn;

/// Stands in for `Selected`: put on every other entity.
#[derive(Component)]
struct Picked;

/// Every sim entity: what a client draws.
type SimEntity = Or<(With<Unit>, With<Building>, With<ResourceNode>, With<FxBox>)>;

/// What a client does between ticks: tag the sim entities (units,
/// buildings, nodes, projectiles and effects) it has not seen before, in
/// entity order: every one with [`Drawn`], every other one of each kind
/// with [`Picked`] (`picked` says whether the next one of a kind gets it).
fn tag_like_a_client(world: &mut World, seen: &mut HashSet<Entity>, picked: &mut [bool; 4]) {
    let kind = |unit: bool, building: bool, node: bool| if unit { 0 } else if building { 1 } else if node { 2 } else { 3 };
    let mut q = world.query_filtered::<(Entity, Has<Unit>, Has<Building>, Has<ResourceNode>), SimEntity>();
    let mut fresh: Vec<(Entity, usize)> =
        q.iter(world).filter(|(e, ..)| !seen.contains(e)).map(|(e, unit, building, node)| (e, kind(unit, building, node))).collect();
    fresh.sort_unstable();
    for (e, kind) in fresh {
        seen.insert(e);
        let mut entity = world.entity_mut(e);
        entity.insert(Drawn);
        if picked[kind] {
            entity.insert(Picked);
        }
        picked[kind] = !picked[kind];
    }
}

/// Run the match `new` makes twice for `ticks` ticks, one tick per frame:
/// untouched, and with sim entities tagged like a client tags them from
/// tick 90 on, and assert they end exactly alike, entity ids included.
/// Returns the untouched run.
fn untouched_and_tagged(new: impl Fn() -> App, ticks: u64) -> App {
    let mut plain = new();
    run_headless(&mut plain, ticks, 0, |_| {});
    let mut tagged = new();
    let (mut seen, mut picked) = (HashSet::new(), [true; 4]);
    run_headless(&mut tagged, ticks, 1, |app| {
        if app.world().resource::<SimClock>().tick >= 90 {
            tag_like_a_client(app.world_mut(), &mut seen, &mut picked);
        }
    });
    assert_eq!(tagged.world().resource::<SimClock>().tick, ticks);
    let world = tagged.world_mut();
    let mut q = world.query_filtered::<(), (With<Unit>, With<Picked>)>();
    let picked_units = q.iter(world).count();
    assert!(picked_units > 10, "the client tags units: {picked_units}");
    let (a, b) = (state(&mut plain), state(&mut tagged));
    assert!(a == b, "client markers changed the match: {}", a.differences(&b));
    plain
}

/// The regression: marking every other unit at tick 90 changed the match
/// within four minutes (sim systems visited units in table order: workers
/// sharing a tree took from it in that order, so it held a different
/// amount; likewise the spatial index's cells, steering's sums, the path
/// queue, healers, repairers, weapons, projectiles, production and
/// trickle income). Tagging every sim entity the way a windowed client
/// does must leave five sim minutes of a match exactly as they were.
#[test]
fn client_markers_on_sim_entities_do_not_change_the_match() {
    let _ = untouched_and_tagged(|| two_player_match(1), 5 * 60 * SIM_HZ as u64);
}

/// The same for a long four-player match with plenty of fighting (shots,
/// projectiles, splash, deaths, debris): `cargo test --release -p
/// aetherfall --test determinism -- --ignored`.
#[test]
#[ignore = "long: cargo test --release -p aetherfall --test determinism -- --ignored"]
fn client_markers_do_not_change_a_long_match_with_fighting() {
    let four_players = || {
        let data = af_core::testing::test_data();
        let factions = ["sylvan", "tide", "steppe", "starborn"].map(String::from).to_vec();
        let opts = SkirmishOptions { seed: 9, players: 4, factions, human: false, ..Default::default() };
        headless_app(data.clone(), skirmish_setup(&data, &opts).expect("setup"), 1)
    };
    let plain = untouched_and_tagged(four_players, 12 * 60 * SIM_HZ as u64);
    let kills: u32 = plain.world().resource::<Players>().list.iter().map(|p| p.stats.kills).sum();
    assert!(kills > 20, "the match saw fighting: {kills} kills");
}

/// A unit as seen by the digest: type, position, facing and hit points, bit
/// for bit, keyed by entity so a divergence names the unit.
type UnitDigest = (Entity, u16, u32, u32, u32, u32, u32);

/// Every live unit (sorted by entity) and every player's resources, bit for
/// bit.
fn digest(app: &mut App) -> (Vec<UnitDigest>, Vec<[u32; MAX_RESOURCES]>) {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(Entity, &Unit, &Position, &Facing, Option<&Health>, Option<&Gatherer>), Without<Dead>>();
    let mut units: Vec<UnitDigest> = q
        .iter(world)
        .map(|(e, u, p, f, h, g)| (e, u.kind.0, p.0.x.to_bits(), p.0.y.to_bits(), f.0.to_bits(), h.map_or(0, |h| h.hp.to_bits()), g.map_or(0, |g| g.amount.to_bits())))
        .collect();
    units.sort_unstable_by_key(|d| d.0);
    let resources = world.resource::<Players>().list.iter().map(|p| p.resources.0.map(f32::to_bits)).collect();
    (units, resources)
}

/// A client-only marker like `Selected`, `BoxVisual` or a model marker: an
/// empty component the simulation never reads. Adding it moves the entity
/// to another archetype, which changes the order Bevy queries visit units.
#[derive(Component)]
struct Marker;

/// Marks every second unit (sorted by entity) with [`Marker`].
fn mark_every_second_unit(app: &mut App) -> usize {
    let world = app.world_mut();
    let mut q = world.query_filtered::<Entity, With<Unit>>();
    let mut units: Vec<Entity> = q.iter(world).collect();
    units.sort_unstable();
    let odd: Vec<Entity> = units.iter().copied().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, e)| e).collect();
    for e in &odd {
        world.entity_mut(*e).insert(Marker);
    }
    odd.len()
}

/// One digest entry as text.
fn unit_line(d: &UnitDigest) -> String {
    format!(
        "{:?} kind {} pos ({}, {}) facing {} hp {} cargo {}",
        d.0,
        d.1,
        f32::from_bits(d.2),
        f32::from_bits(d.3),
        f32::from_bits(d.4),
        f32::from_bits(d.5),
        f32::from_bits(d.6)
    )
}

/// Where a unit started the tick and what it is up to (goal, path, order,
/// cargo), so a divergence shows whether the intent or the movement differs.
fn describe(world: &World, e: Entity) -> String {
    let Ok(entity) = world.get_entity(e) else { return "gone".into() };
    let prev = entity.get::<PrevPosition>().map(|p| (p.0.x, p.0.y));
    let prev_facing = entity.get::<PrevFacing>().map(|f| f.0);
    let vel = entity.get::<Velocity>().map(|v| (v.0.x, v.0.y));
    let goal = entity.get::<MoveGoal>().map(|g| format!("({}, {}) arrive {} follow {:?} seq {}", g.target.x, g.target.y, g.arrive, g.follow, g.seq));
    let path = entity
        .get::<PathState>()
        .map(|p| format!("{:?} next {}/{} partial {} seq {} waypoints {:?}", p.status, p.next, p.waypoints.len(), p.partial, p.goal_seq, p.waypoints));
    let order = entity.get::<Orders>().and_then(|o| o.current().cloned());
    let cargo = entity.get::<Gatherer>().map(|g| format!("{:?} {} target {:?}", g.carrying, g.amount, g.target));
    format!(
        "      tick start {prev:?} facing {prev_facing:?} velocity {vel:?}\n      goal {goal:?}\n      path {path:?}\n      order {order:?} cargo {cargo:?}"
    )
}

/// Describes the first tick two matches differ: the resources, then every
/// unit present in one only or differing between them, with its state.
fn divergence_report(plain: &App, marked: &App, a: &(Vec<UnitDigest>, Vec<[u32; MAX_RESOURCES]>), b: &(Vec<UnitDigest>, Vec<[u32; MAX_RESOURCES]>)) -> String {
    let mut report = String::new();
    if a.1 != b.1 {
        report.push_str(&format!("  resources: plain  {:?}\n             marked {:?}\n", a.1, b.1));
    }
    let (mut i, mut j) = (0, 0);
    while i < a.0.len() || j < b.0.len() {
        match (a.0.get(i), b.0.get(j)) {
            (Some(x), Some(y)) if x.0 == y.0 => {
                if x != y {
                    report.push_str(&format!("  plain:  {}\n{}\n", unit_line(x), describe(plain.world(), x.0)));
                    report.push_str(&format!("  marked: {}\n{}\n", unit_line(y), describe(marked.world(), y.0)));
                }
                i += 1;
                j += 1;
            }
            (Some(x), Some(y)) if x.0 < y.0 => {
                report.push_str(&format!("  only in plain:  {}\n", unit_line(x)));
                i += 1;
            }
            (Some(x), None) => {
                report.push_str(&format!("  only in plain:  {}\n", unit_line(x)));
                i += 1;
            }
            (_, Some(y)) => {
                report.push_str(&format!("  only in marked: {}\n", unit_line(y)));
                j += 1;
            }
            (None, None) => break,
        }
    }
    report
}

/// Client-only components (`Selected`, visual markers) are added between
/// ticks and move units to other archetypes. That must not change the match:
/// in lockstep multiplayer every peer has a different selection, and a
/// windowed match must replay like its headless twin.
#[test]
fn client_only_components_do_not_change_the_match() {
    const MARK_AT: u64 = 90;
    let ticks = 5 * 60 * SIM_HZ as u64;
    let mut plain = two_player_match(1);
    let mut marked = two_player_match(1);
    // Prime the clocks (enters Playing), like `run_headless`.
    plain.update();
    marked.update();
    let mut marked_count = 0;
    loop {
        plain.update();
        marked.update();
        let tick = plain.world().resource::<SimClock>().tick;
        assert_eq!(tick, marked.world().resource::<SimClock>().tick, "the two matches stepped differently");
        let a = digest(&mut plain);
        let b = digest(&mut marked);
        if a != b {
            let report = divergence_report(&plain, &marked, &a, &b);
            panic!("the matches diverge at tick {tick} (marked {marked_count} units at tick {MARK_AT})\n{report}");
        }
        if tick == MARK_AT {
            marked_count = mark_every_second_unit(&mut marked);
            assert!(marked_count > 0, "no units to mark");
        }
        if tick >= ticks || plain.world().resource::<GameOutcome>().ended {
            break;
        }
    }
    assert!(marked_count > 0, "the marker was never added");
}
