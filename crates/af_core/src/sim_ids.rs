//! Entity ids for the simulation that only the simulation decides.
//!
//! Sim code orders, staggers and breaks ties by `Entity`: the collision
//! solver sorts its bodies by entity, brains and target scans stagger by the
//! entity index, equal targets and splash victims are told apart by the
//! entity bits, the AI snapshot is sorted by entity. Bevy hands every spawn
//! its id from one world-wide allocator, and the client spawns and despawns
//! entities between ticks (visual roots and parts, health bars, outlines,
//! ghosts, click pings, UI). Left alone, the ids later sim spawns get would
//! depend on frame pacing, the mouse and the selection, and so would the
//! match: windowed games would not replay and lockstep peers would drift.
//!
//! So the simulation gets ids of its own:
//!
//! * The first match setup reserves the index range [`SIM_ID_START`] ..
//!   `SIM_ID_START + SIM_ID_COUNT` from the world's allocator, which keeps
//!   every other index for the client.
//! * While the simulation can spawn, af_core installs a *sim allocator* in
//!   the world that hands out ids from that range only: every `FixedMain`
//!   tick ([`SimTickOpen`] .. [`SimTickClose`]), af_map's setup
//!   (`OnEnter(GameState::Playing)` around `MapSetupSet`) and the teardown
//!   on leaving `Playing` (see below). Outside those *windows* the client's
//!   allocator is back.
//! * Each window starts from a fresh sim allocator built from af_core's own
//!   record of the range (the ids freed so far, sorted, then the next
//!   never-used index), so nothing of how the previous window went (despawn
//!   order, Bevy's batching of freed ids) reaches the next one, and the ids
//!   depend only on what the simulation spawned and despawned.
//! * Client entities despawned inside a window (the visual children of a
//!   unit that dies) go back to the client allocator when it closes.
//!
//! Sim code needs nothing special: spawn with `Commands` in `FixedUpdate`
//! (or in af_map's setup) as before. A sim entity spawned outside a window
//! (a test harness, a dev tool) works too; it just gets a client id.
//!
//! On leaving `Playing`, af_core despawns every `Unit`, `Building`,
//! `ResourceNode` and `FxBox` itself, in entity order and inside a window,
//! before any `OnExit` system or `DespawnOnExit` runs (so the range's ids are
//! all free again for the next match); af_map's teardown then only removes
//! the match resources (and click pings).
//!
//! An app whose world was not built by a match setup (the test harness's
//! `flat_world`, which inserts `Players` itself) never reserves the range and
//! keeps Bevy's ids, as before.

use std::ops::Range;

use bevy::ecs::entity::{EntityAllocator, EntityGeneration, EntityIndex};
use bevy::ecs::schedule::{ScheduleLabel, SingleThreadedExecutor};
use bevy::prelude::*;
use bevy::state::state::{StateTransition, StateTransitionEvent, StateTransitionSystems};

use crate::components::{Building, FxBox, Position, ResourceNode, Unit};
use crate::ext::map::MapSetupSet;
use crate::resources::Players;
use crate::sets::GameState;

/// First index of the simulation's id range. Fixed, so a headless match and
/// a windowed one give their entities the very same ids.
pub const SIM_ID_START: u32 = 1 << 15;
/// Size of the simulation's id range: far more entities than a match ever
/// has alive at once (units, buildings, nodes, projectiles and effects).
pub const SIM_ID_COUNT: u32 = 1 << 16;
/// If [`SIM_ID_START`] is already taken (a client that spawned that many
/// entities before the first match), the range starts at the next multiple
/// of this instead: same index residues, so the same staggers.
const FALLBACK_ALIGN: u32 = 1 << 16;
/// Freed ids lent to a tick's sim allocator (any more it spawns come from the
/// never-used part of the range).
const LEND: usize = 512;
/// Windows open only while at least this many never-used ids remain: one
/// tick never spawns that many.
const FRESH_RESERVE: u32 = 8192;
/// How far past the last used fresh id a window close looks for more.
const SCAN_SLACK: u32 = 64;

/// Runs first in every `FixedMain` tick: installs the sim allocator.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimTickOpen;

/// Runs last in every `FixedMain` tick: puts the client allocator back.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimTickClose;

/// The simulation's id range and its window state. Owned by af_core.
#[derive(Resource, Default)]
pub struct SimEntities {
    pool: Option<Pool>,
    /// The range could not be reserved or ran out: the sim uses Bevy's ids.
    failed: bool,
    window: Option<Window>,
    /// Entities despawned while the window is open (sim entities and the
    /// client children that went with them), sorted out on close.
    freed: Vec<Entity>,
}

struct Pool {
    start: u32,
    /// Indices `start .. start + used` have been handed out at least once.
    used: u32,
    /// Ids the sim freed, ready to hand out again; the next one out is last.
    stock: Vec<Entity>,
    /// Per index of the range: the entity the sim allocator gave it to and
    /// that was alive when its window closed (`Entity::PLACEHOLDER` if none).
    /// Only that entity's despawn frees the id for us again.
    live: Vec<Entity>,
    /// Per index of the range: the client allocator got it back (despawned
    /// outside a window), so it is no longer ours.
    lost: Vec<bool>,
    /// Per index of the range: handed out by the open window (scratch).
    fresh_out: Vec<bool>,
}

impl Pool {
    fn new(start: u32) -> Pool {
        let n = SIM_ID_COUNT as usize;
        Pool { start, used: 0, stock: Vec::new(), live: vec![Entity::PLACEHOLDER; n], lost: vec![false; n], fresh_out: vec![false; n] }
    }

    fn slot(&self, index: u32) -> Option<usize> {
        index.checked_sub(self.start).filter(|i| *i < SIM_ID_COUNT).map(|i| i as usize)
    }
}

struct Window {
    /// The client allocator, parked while the sim allocator is installed.
    client: EntityAllocator,
    /// Stock lent to the sim allocator.
    lent: Vec<Entity>,
    /// `Pool::used` when the window opened.
    used: u32,
}

impl SimEntities {
    /// The simulation's index range, once a match setup has reserved it.
    pub fn range(&self) -> Option<Range<u32>> {
        if self.failed {
            return None;
        }
        self.pool.as_ref().map(|p| p.start..p.start + SIM_ID_COUNT)
    }

    /// Is the sim allocator installed right now?
    pub fn is_open(&self) -> bool {
        self.window.is_some()
    }

    /// How many indices of the range have ever been handed out.
    pub fn used(&self) -> u32 {
        self.pool.as_ref().map_or(0, |p| p.used)
    }

    /// Freed ids waiting to be handed out again.
    pub fn free_ids(&self) -> usize {
        self.pool.as_ref().map_or(0, |p| p.stock.len())
    }

    /// Indices of the range the client allocator took back.
    pub fn lost_ids(&self) -> usize {
        self.pool.as_ref().map_or(0, |p| p.lost.iter().filter(|l| **l).count())
    }
}

/// Registers the resource, the windows and the despawn bookkeeping.
pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<SimEntities>();
    if let Some(mut order) = app.world_mut().get_resource_mut::<bevy::app::FixedMainScheduleOrder>() {
        order.insert_before(bevy::app::FixedFirst, SimTickOpen);
        order.insert_after(bevy::app::FixedLast, SimTickClose);
    }
    app.add_systems(SimTickOpen, open_tick)
        .add_systems(SimTickClose, close_window)
        .add_systems(OnEnter(GameState::Playing), (open_setup.before(MapSetupSet), close_window.after(MapSetupSet)))
        // A deterministic system order for the setup: client systems in this
        // schedule run before or after the window, never inside it.
        .edit_schedule(OnEnter(GameState::Playing), |s| {
            s.set_executor(SingleThreadedExecutor::new());
        })
        .add_systems(
            StateTransition,
            tear_down
                .run_if(leaving_play)
                .after(StateTransitionSystems::DependentTransitions)
                .before(StateTransitionSystems::ExitSchedules),
        )
        .add_observer(note_despawn::<Position>)
        .add_observer(note_despawn::<FxBox>)
        .add_observer(note_despawn::<ChildOf>);
}

/// Every despawn of a sim entity or a child entity: inside a window, keep it
/// for the close; outside, a sim id the client allocator now holds is lost.
fn note_despawn<C: Component>(despawn: On<Despawn, C>, sims: Option<ResMut<SimEntities>>) {
    let Some(mut sims) = sims else { return };
    let e = despawn.entity;
    if sims.window.is_some() {
        sims.freed.push(e);
        return;
    }
    let Some(pool) = sims.pool.as_ref() else { return };
    if let Some(i) = pool.slot(e.index_u32())
        && pool.live[i] == e
    {
        let pool = sims.pool.as_mut().expect("checked");
        pool.live[i] = Entity::PLACEHOLDER;
        pool.lost[i] = true;
    }
}

fn open_tick(world: &mut World) {
    let playing = world.get_resource::<State<GameState>>().is_some_and(|s| *s.get() == GameState::Playing);
    if playing && world.resource::<SimEntities>().pool.is_some() {
        open(world, LEND);
    }
}

/// af_map is about to build the match world: open a window for it (the
/// first one reserves the range). A world a test harness built itself
/// (`Players` already there) keeps Bevy's ids.
fn open_setup(world: &mut World) {
    if !world.contains_resource::<Players>() {
        open(world, usize::MAX);
    }
}

fn close_window(world: &mut World) {
    close(world);
}

/// Did this frame's state transition leave `Playing`?
fn leaving_play(mut transitions: MessageReader<StateTransitionEvent<GameState>>) -> bool {
    transitions.read().fold(false, |leaving, t| leaving | (t.exited == Some(GameState::Playing)))
}

/// Despawn the match's sim entities in entity order inside a window, so
/// their ids are free for the next match and the next match's ids do not
/// depend on the order the teardown happened to find them in.
fn tear_down(world: &mut World) {
    if world.resource::<SimEntities>().pool.is_none() || !open(world, 0) {
        return;
    }
    let mut q = world.query_filtered::<Entity, Or<(With<Unit>, With<Building>, With<ResourceNode>, With<FxBox>)>>();
    let mut doomed: Vec<Entity> = q.iter(world).collect();
    doomed.sort_unstable();
    for e in doomed {
        world.despawn(e);
    }
    world.flush();
    close(world);
}

/// Install a sim allocator lending it up to `lend` freed ids. False when sim
/// ids are unavailable (the world keeps the client allocator).
fn open(world: &mut World, lend: usize) -> bool {
    {
        let sims = world.resource::<SimEntities>();
        if sims.failed || sims.window.is_some() {
            return false;
        }
    }
    if world.resource::<SimEntities>().pool.is_none() {
        match reserve(world) {
            Some(start) => world.resource_mut::<SimEntities>().pool = Some(Pool::new(start)),
            None => {
                warn!("could not reserve entity ids {SIM_ID_START}.. for the simulation; sim ids now depend on client spawns");
                world.resource_mut::<SimEntities>().failed = true;
                return false;
            }
        }
    }
    world.resource_scope(|world, mut sims: Mut<SimEntities>| {
        let sims = &mut *sims;
        let pool = sims.pool.as_mut().expect("reserved above");
        if SIM_ID_COUNT - pool.used.min(SIM_ID_COUNT) < FRESH_RESERVE {
            warn!("the simulation's entity id range is used up; sim ids now depend on client spawns");
            sims.failed = true;
            return false;
        }
        let n = pool.stock.len().min(lend);
        let lent = pool.stock.split_off(pool.stock.len() - n);
        let mut sim = EntityAllocator::default();
        // Fresh ids continue the range: skip the indices below it (the
        // iterator only holds the skipped range; forgetting it is free).
        std::mem::forget(sim.alloc_many(pool.start + pool.used));
        // The last one lent comes out first.
        sim.free_many(&lent);
        std::mem::swap(world.entity_allocator_mut(), &mut sim);
        sims.freed.clear();
        sims.window = Some(Window { client: sim, lent, used: pool.used });
        true
    })
}

/// Put the client allocator back and record what the window did: which of
/// our ids are now live, which are free again (sorted, for the next
/// window), and which client ids were freed into the sim allocator (handed
/// back to the client allocator).
fn close(world: &mut World) {
    world.resource_scope(|world, mut sims: Mut<SimEntities>| {
        let sims = &mut *sims;
        let Some(mut window) = sims.window.take() else { return };
        std::mem::swap(world.entity_allocator_mut(), &mut window.client);
        // `window.client` is now the sim allocator; everything it holds is
        // accounted for below.
        drop(window.client);
        let pool = sims.pool.as_mut().expect("a window needs the range");
        let entities = world.entities();
        let index_of = |i: u32| EntityIndex::from_raw_u32(i).expect("entity index");
        let used = |i: u32| entities.is_index_spawned(index_of(i)) || entities.resolve_from_index(index_of(i)).generation() != EntityGeneration::FIRST;

        // What the sim allocator handed out: lent ids, then fresh ones in
        // order (skipping over a few never spawned).
        let mut ours: Vec<u32> = window.lent.iter().map(|e| e.index_u32()).collect();
        let first_fresh = pool.start + window.used;
        let end = pool.start + SIM_ID_COUNT;
        let (mut last_used, mut idle, mut i) = (None, 0, first_fresh);
        while idle < SCAN_SLACK && i < end {
            if used(i) {
                last_used = Some(i);
                idle = 0;
            } else {
                idle += 1;
            }
            i += 1;
        }
        if let Some(last) = last_used {
            ours.extend(first_fresh..=last);
            pool.used = last + 1 - pool.start;
            if last + 1 == end {
                // The sim allocator may have gone on into client indices.
                error!("the simulation spawned past its entity id range; sim ids now depend on client spawns");
                sims.failed = true;
            }
        }
        for &i in &ours {
            let slot = pool.slot(i).expect("in range");
            pool.fresh_out[slot] = true;
        }
        // Entities despawned in the window: ours if the window handed out
        // their id or they are the live entity we gave it to before; anything
        // else (a client entity, also one at an index of the range the client
        // allocator took back) goes back to the client allocator.
        let mut give_back = Vec::new();
        for e in sims.freed.drain(..) {
            let i = e.index_u32();
            match pool.slot(i) {
                Some(slot) if pool.fresh_out[slot] || pool.live[slot] == e => ours.push(i),
                slot => {
                    if !entities.is_index_spawned(e.index()) {
                        give_back.push(entities.resolve_from_index(e.index()));
                    }
                    if let Some(slot) = slot {
                        pool.lost[slot] = true;
                    }
                }
            }
        }
        ours.sort_unstable();
        ours.dedup();
        let mut free_again = Vec::new();
        for i in ours {
            let slot = pool.slot(i).expect("in range");
            pool.fresh_out[slot] = false;
            let id = entities.resolve_from_index(index_of(i));
            if entities.is_index_spawned(index_of(i)) {
                pool.live[slot] = id;
            } else {
                pool.live[slot] = Entity::PLACEHOLDER;
                free_again.push(id);
            }
        }
        // Lowest index out first next time.
        free_again.reverse();
        pool.stock.extend(free_again);
        give_back.sort_unstable();
        give_back.dedup();
        if !give_back.is_empty() {
            world.entity_allocator_mut().free_many(&give_back);
        }
    });
}

/// Reserve the range from the world's allocator: [`SIM_ID_START`], else the
/// first aligned start above every index in use. `None` if neither works.
fn reserve(world: &mut World) -> Option<u32> {
    let len = world.entities().len();
    let fallback = len.max(SIM_ID_START + 1).div_ceil(FALLBACK_ALIGN).checked_mul(FALLBACK_ALIGN)?;
    [SIM_ID_START, fallback].into_iter().find(|start| try_reserve(world, *start))
}

/// Take every index of `start .. start + SIM_ID_COUNT` from the allocator
/// (they must all be untouched: never spawned), giving back whatever else it
/// hands out on the way.
fn try_reserve(world: &mut World, start: u32) -> bool {
    let Some(end) = start.checked_add(SIM_ID_COUNT) else { return false };
    let entities = world.entities();
    let untouched = (start..end).all(|i| {
        let index = EntityIndex::from_raw_u32(i).expect("entity index");
        !entities.is_index_spawned(index) && entities.resolve_from_index(index).generation() == EntityGeneration::FIRST
    });
    if !untouched {
        return false;
    }
    // Free ids come first, then fresh ones in order: this many allocations
    // reach the end of the range unless part of it is taken.
    let budget = 2 * end as usize + entities.len() as usize;
    let alloc = world.entity_allocator_mut();
    let mut ours = Vec::with_capacity(SIM_ID_COUNT as usize);
    let mut spare = Vec::new();
    for _ in 0..budget {
        let e = alloc.alloc();
        if (start..end).contains(&e.index_u32()) {
            ours.push(e);
            if ours.len() == SIM_ID_COUNT as usize {
                break;
            }
        } else {
            spare.push(e);
        }
    }
    let whole = ours.len() == SIM_ID_COUNT as usize;
    if !whole {
        spare.append(&mut ours);
    }
    alloc.free_many(&spare);
    whole
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::prelude::*;
    use crate::testing::test_data;
    use crate::CorePlugin;

    /// When a fake sim entity dies (tick).
    #[derive(Component)]
    struct Life(u64);

    /// A sim entity no despawn bookkeeping watches (no `Position`, no
    /// `FxBox`), like a resource the setup inserts.
    #[derive(Component)]
    struct Prop;

    /// Entity bits of every sim spawn, in order.
    #[derive(Resource, Default)]
    struct Log(Vec<u64>);

    /// Which client churn an app runs between ticks.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Client {
        None,
        /// Plain spawns and despawns, bursts, and visual children on sim
        /// entities (which die with them, inside a tick).
        Churn,
    }

    /// Like af_map: builds the match world in `MapSetupSet`.
    fn fake_setup(mut commands: Commands, mut log: ResMut<Log>) {
        commands.insert_resource(Players { list: Vec::new() });
        commands.insert_resource(Nav(NavGrid::new(8, 8, 1.0, Vec2::ZERO)));
        for i in 0..300u64 {
            let e = commands.spawn((Unit { kind: UnitTypeId(0) }, Position(Vec2::new(i as f32, 0.0)), Life(40 + (i * 7919) % 400))).id();
            log.0.push(e.to_bits());
        }
        let prop = commands.spawn(Prop).id();
        log.0.push(prop.to_bits());
    }

    /// Like af_map's teardown.
    fn fake_teardown(mut commands: Commands) {
        commands.remove_resource::<Players>();
        commands.remove_resource::<Nav>();
    }

    /// A sim that spawns and despawns every tick (units that die through
    /// `Dead`, effects despawned directly, bursts of effects), deciding in
    /// entity order so table order does not matter.
    #[allow(clippy::type_complexity)]
    fn churn(
        mut commands: Commands,
        clock: Res<SimClock>,
        mut log: ResMut<Log>,
        units: Query<(Entity, &Life), (With<Unit>, Without<Dead>)>,
        fx: Query<(Entity, &Life), With<FxBox>>,
    ) {
        let t = clock.tick;
        let mut dying: Vec<Entity> = units.iter().filter(|(_, l)| l.0 <= t).map(|(e, _)| e).collect();
        dying.sort_unstable();
        for e in dying {
            commands.entity(e).insert(Dead);
        }
        let mut faded: Vec<Entity> = fx.iter().filter(|(_, l)| l.0 <= t).map(|(e, _)| e).collect();
        faded.sort_unstable();
        for e in faded {
            commands.entity(e).despawn();
        }
        let fx_box = FxBox { pos: Vec3::ZERO, prev_pos: Vec3::ZERO, dir: Vec3::Z, size: Vec3::ONE, color: [1.0; 3], glow: false, alpha: 1.0 };
        let sparks = if t.is_multiple_of(50) { 150 } else { 3 };
        for k in 0..sparks {
            let e = commands.spawn((fx_box, Life(t + 1 + (t + k) % 5))).id();
            log.0.push(e.to_bits());
        }
        if t.is_multiple_of(5) {
            let e = commands.spawn((Unit { kind: UnitTypeId(0) }, Position(Vec2::ZERO), Life(t + 30 + (t * 31) % 170))).id();
            log.0.push(e.to_bits());
        }
    }

    /// What a renderer and an input layer do between ticks.
    fn client_churn(mut commands: Commands, mut frame: Local<u64>, mut mine: Local<Vec<(Entity, u64)>>, units: Query<Entity, With<Unit>>) {
        *frame += 1;
        let f = *frame;
        for _ in 0..f % 5 {
            let e = commands.spawn_empty().id();
            mine.push((e, f + 3));
        }
        mine.retain(|(e, until)| {
            let keep = *until > f;
            if !keep {
                commands.entity(*e).despawn();
            }
            keep
        });
        if f.is_multiple_of(17) {
            let burst: Vec<Entity> = (0..200).map(|_| commands.spawn_empty().id()).collect();
            for e in burst {
                commands.entity(e).despawn();
            }
        }
        let mut sim: Vec<Entity> = units.iter().collect();
        sim.sort_unstable();
        if !sim.is_empty() {
            let parent = sim[(f as usize * 7) % sim.len()];
            commands.spawn(ChildOf(parent)).with_child(());
        }
    }

    fn fake_match(ticks_per_update: u32, client: Client, onenter_client: Option<bool>) -> App {
        let mut app = App::new();
        app.set_error_handler(bevy::ecs::error::warn);
        app.add_plugins((MinimalPlugins, StatesPlugin));
        app.insert_state(GameState::Playing);
        app.insert_resource(TimeUpdateStrategy::FixedTimesteps(ticks_per_update));
        app.insert_resource(Data(test_data()));
        app.init_resource::<Log>();
        // A client `OnEnter(Playing)` system (camera, HUD), added before or
        // after af_core.
        let spawn_hud = |mut commands: Commands| {
            for _ in 0..100 {
                commands.spawn_empty();
            }
        };
        if onenter_client == Some(true) {
            app.add_systems(OnEnter(GameState::Playing), spawn_hud);
        }
        app.add_plugins(CorePlugin);
        if onenter_client == Some(false) {
            app.add_systems(OnEnter(GameState::Playing), spawn_hud);
        }
        app.add_systems(OnEnter(GameState::Playing), fake_setup.in_set(MapSetupSet))
            .add_systems(OnExit(GameState::Playing), fake_teardown.in_set(MapSetupSet))
            .add_systems(FixedUpdate, churn.in_set(SimSet::Economy));
        if client == Client::Churn {
            app.add_systems(Update, client_churn);
        }
        app.world_mut().resource_mut::<Time<Virtual>>().set_max_delta(Duration::from_secs(10));
        app
    }

    /// Run until `ticks` sim ticks have passed: the spawn log, a separator,
    /// and the sim entities alive at the end.
    fn run(app: &mut App, ticks: u64) -> Vec<u64> {
        app.update();
        while app.world().resource::<SimClock>().tick < ticks {
            app.update();
        }
        let world = app.world_mut();
        let mut alive: Vec<u64> = world.query_filtered::<Entity, Or<(With<Unit>, With<FxBox>)>>().iter(world).map(Entity::to_bits).collect();
        alive.sort_unstable();
        let mut log = std::mem::take(&mut world.resource_mut::<Log>().0);
        log.push(u64::MAX);
        log.extend(alive);
        log
    }

    fn agreeing(a: &[u64], b: &[u64]) -> usize {
        a.iter().zip(b).take_while(|(x, y)| x == y).count()
    }

    /// The regression: client entities spawned and despawned between ticks,
    /// and how many ticks run per frame, change nothing about the ids the
    /// simulation's spawns get (so nothing that sorts or staggers by them).
    #[test]
    fn client_spawns_and_frame_pacing_do_not_change_sim_ids() {
        let ticks = 600;
        let quiet = run(&mut fake_match(1, Client::None, None), ticks);
        let busy = run(&mut fake_match(1, Client::Churn, None), ticks);
        let paced = run(&mut fake_match(2, Client::Churn, None), ticks);
        assert!(quiet.len() > 3000, "the fake sim spawns: {}", quiet.len());
        assert!(quiet == busy, "client churn moved sim ids: {} of {} agree", agreeing(&quiet, &busy), quiet.len());
        assert!(quiet == paced, "two ticks per frame moved sim ids: {} of {} agree", agreeing(&quiet, &paced), quiet.len());
    }

    /// Sim ids come from the reserved range, client ids never do, and the
    /// client allocator is back between ticks.
    #[test]
    fn sim_spawns_use_the_range_and_client_spawns_do_not() {
        let mut app = fake_match(1, Client::Churn, None);
        let log = run(&mut app, 200);
        let range = app.world().resource::<SimEntities>().range().expect("the setup reserved the range");
        assert_eq!(range, SIM_ID_START..SIM_ID_START + SIM_ID_COUNT);
        assert!(log.iter().filter(|b| **b != u64::MAX).all(|b| range.contains(&Entity::from_bits(*b).index_u32())));
        assert!(!app.world().resource::<SimEntities>().is_open());
        let client = app.world_mut().spawn_empty().id();
        assert!(!range.contains(&client.index_u32()), "{client}");
        assert_eq!(app.world().resource::<SimEntities>().lost_ids(), 0);
    }

    /// Client systems in `OnEnter(Playing)` (camera, HUD, scene), added
    /// before or after af_core, do not change the setup's ids.
    #[test]
    fn client_setup_systems_stay_out_of_the_setup_window() {
        let quiet = run(&mut fake_match(1, Client::None, None), 30);
        for before_core in [false, true] {
            let hud = run(&mut fake_match(1, Client::None, Some(before_core)), 30);
            assert!(quiet == hud, "a client OnEnter system (added before af_core: {before_core}) moved the setup's ids: {} agree", agreeing(&quiet, &hud));
        }
    }

    /// The visual children of a sim entity that dies inside a tick go back
    /// to the client allocator (they are its next ids), not into the sim's.
    #[test]
    fn client_children_of_dead_sim_entities_return_to_the_client() {
        let mut app = fake_match(1, Client::None, None);
        run(&mut app, 3);
        let world = app.world_mut();
        let unit = {
            let mut q = world.query_filtered::<Entity, With<Unit>>();
            let mut v: Vec<Entity> = q.iter(world).collect();
            v.sort_unstable();
            v[0]
        };
        let children: Vec<Entity> = (0..300).map(|_| world.spawn(ChildOf(unit)).id()).collect();
        world.entity_mut(unit).insert(Dead);
        app.update();
        let world = app.world_mut();
        assert!(world.get_entity(unit).is_err(), "Cleanup despawned the unit");
        assert!(children.iter().all(|c| world.get_entity(*c).is_err()), "and its children");
        let mut freed: Vec<u32> = children.iter().map(|c| c.index_u32()).collect();
        freed.sort_unstable();
        let mut next: Vec<u32> = (0..300).map(|_| world.spawn_empty().id().index_u32()).collect();
        next.sort_unstable();
        assert_eq!(next, freed, "the client allocator reuses the children's ids");
        let range = world.resource::<SimEntities>().range().unwrap();
        assert!(freed.iter().all(|i| !range.contains(i)));
    }

    /// Leaving `Playing` frees the whole match inside a window, so a second
    /// match reuses the ids instead of eating into the range, and the same
    /// history gives the same ids again.
    #[test]
    fn a_rematch_reuses_the_range_and_replays() {
        let rematch = || {
            let mut app = fake_match(1, Client::Churn, None);
            let first = run(&mut app, 300);
            let used = app.world().resource::<SimEntities>().used();
            app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Menu);
            app.update();
            let world = app.world_mut();
            assert_eq!(world.query_filtered::<Entity, Or<(With<Unit>, With<FxBox>)>>().iter(world).count(), 0);
            let sims = world.resource::<SimEntities>();
            let stock: std::collections::HashSet<u32> = sims.pool.as_ref().unwrap().stock.iter().map(|e| e.index_u32()).collect();
            let gone = |b: &&u64| **b != u64::MAX && world.get_entity(Entity::from_bits(**b)).is_err();
            let missing = first.iter().filter(gone).map(|b| Entity::from_bits(*b).index_u32()).filter(|i| !stock.contains(i)).count();
            assert_eq!(missing, 0, "every id the match's entities had is free again");
            // The setup's prop and new resources (`Players`, `Nav`) hold ids too.
            assert!(sims.free_ids() as u32 + 3 >= used, "{} free of {used}", sims.free_ids());
            assert_eq!(sims.lost_ids(), 0);
            app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Playing);
            let second = run(&mut app, 300);
            let after = app.world().resource::<SimEntities>().used();
            assert!(after <= used + SCAN_SLACK, "the rematch reused the range: {used} -> {after}");
            (first, second)
        };
        let a = rematch();
        let b = rematch();
        assert!(a == b, "the same history gives the same ids");
    }

    /// A sim entity despawned between ticks gives its id to the client
    /// allocator. When a client entity later gets that index and dies inside
    /// a tick (the visual child of a dying unit), the id goes back to the
    /// client, not into the sim's free ids (whose generations the client
    /// would then decide): for a watched sim entity (a unit) and for one
    /// nothing watches (like a resource the setup inserted).
    #[test]
    fn range_ids_the_client_took_back_stay_with_the_client() {
        for watched in [true, false] {
            client_took_back(watched);
        }
    }

    fn client_took_back(watched: bool) {
        let mut app = fake_match(1, Client::None, None);
        run(&mut app, 3);
        let world = app.world_mut();
        let mut units: Vec<Entity> = world.query_filtered::<Entity, With<Unit>>().iter(world).collect();
        units.sort_unstable();
        let prop = world.query_filtered::<Entity, With<Prop>>().single(world).expect("the setup's prop");
        let range = world.resource::<SimEntities>().range().unwrap();
        assert!(range.contains(&prop.index_u32()));
        let (gone, parent) = if watched { (units[0], units[1]) } else { (prop, units[0]) };
        world.despawn(gone);
        assert_eq!(world.resource::<SimEntities>().lost_ids(), usize::from(watched));
        // Make the client allocator hand the index out again.
        let filler: Vec<Entity> = (0..300).map(|_| world.spawn_empty().id()).collect();
        for e in filler {
            world.despawn(e);
        }
        let mut child = None;
        for _ in 0..1000 {
            let e = world.spawn(ChildOf(parent)).id();
            if e.index() == gone.index() {
                child = Some(e);
                break;
            }
        }
        let child = child.expect("the client reused the index");
        world.entity_mut(parent).insert(Dead);
        app.update();
        let world = app.world_mut();
        assert!(world.get_entity(child).is_err(), "the child died with its parent");
        let sims = world.resource::<SimEntities>();
        let stock = &sims.pool.as_ref().unwrap().stock;
        assert!(stock.iter().all(|e| e.index() != gone.index()), "not adopted as a sim id");
        assert!(stock.iter().any(|e| e.index() == parent.index()), "the parent's id is free again");
        let mut again = false;
        for _ in 0..1000 {
            again |= world.spawn_empty().id().index() == gone.index();
        }
        assert!(again, "the client allocator has the index back");
    }

    /// A world a test harness built itself keeps Bevy's ids: no range.
    #[test]
    fn harness_worlds_keep_bevys_ids() {
        let mut app = crate::testing::sim_app(test_data());
        crate::testing::flat_world(&mut app, 64.0, &[("ironhold", 0)]);
        crate::testing::run_ticks(&mut app, 5);
        assert_eq!(app.world().resource::<SimEntities>().range(), None);
        assert!(!app.world().resource::<SimEntities>().is_open());
    }
}
