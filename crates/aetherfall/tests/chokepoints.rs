//! Chokepoints: crowds ordered through narrow gaps the way a player orders
//! them, measured. The bench for gridlock in channels.
//!
//! Every scenario runs the real simulation a player's orders go through
//! (af_core, af_movement, af_behavior, af_combat, af_economy) on a flat
//! map with a wall across it, a cliff band or a row of houses, and one gap.
//! Units stand in blocks on one side (or both) and get a single
//! `CommandKind::Move` or `AttackMove` per group through the `CommandInbox`,
//! as a right-click sends it: af_behavior spreads them over formation
//! slots, af_movement paths, steers and collides them, and an order whose
//! path fails pops and leaves the unit idle where it stands. Positions are
//! fixed and each scenario runs for a fixed simulated time.
//!
//! What it measures per scenario: units through the gap of all ordered;
//! units whose order ended by arriving, and by failing; arrivals short of
//! the gap (counted as arrived by touching an arrived groupmate before
//! getting through); seconds after the order until half, nine tenths and
//! all were through; units stuck at the end (not through and moved less
//! than a metre over the last ten seconds); the deepest overlap of two
//! ground units at the end of any tick; and the collision solver's
//! counters ([`CollisionStats`]) over the run.
//!
//! The fast tests check that no tick of any scenario ends with boxes
//! overlapping past the game's rule, that every scenario's crowd gets
//! through on the default layout as its bar asks ([`BARS`]: everyone
//! through, no order failed, in good time, but for two units that
//! cannot), and that two runs agree bit for bit. The report is a
//! measurement, so it is `#[ignore]`d:
//!
//! ```text
//! cargo test -p aetherfall --test chokepoints chokepoint_report -- --ignored --nocapture
//! ```
//!
//! `AF_CHOKE=<name>[,<name>...]` runs only those scenarios, and
//! `AF_CHOKE_TRACE=<name>|all` also prints, for those, a line a second
//! (through, in the gap, failed, stuck, the solver's holds) and a map of
//! the gap every five seconds: `>` `<` walking east or west (`!` stuck
//! for a second or more), `a` arrived through, `s` arrived short of it,
//! `x` failed, `i` an idle bystander, `#` cliff, `H` house. With the trace,
//! `AF_CHOKE_UNITS=<name>|all` also lists every second each unit within
//! two metres of the gap: where it stands and faces, its velocity, path
//! status, next waypoint and stuck time, and its order.
//! `AF_CHOKE_JITTER=<seed>` moves every unit up to [`JITTER`] from its spot,
//! the same way for the same seed: a variant of each scenario, to tell a
//! change that helps from one that happens to suit the exact layout.

use std::f32::consts::FRAC_PI_2;
use std::fmt::Write as _;

use af_core::ext::movement::CollisionStats;
use af_core::prelude::*;
use af_core::testing::{command, flat_world, run_ticks, sim_app, test_data, with_commands};
use bevy::prelude::*;

/// Side of the square map (metres).
const MAP: f32 = 96.0;
/// The faction every player plays: no movement traits, so its foot units
/// do not climb the cliffs.
const FACTION: &str = "starborn";
/// Unit types, from the common pool.
const WORKER: &str = "peasant";
const SPEAR: &str = "spearman";
const HEAVY: &str = "man_at_arms";
const RIDER: &str = "knight";
const RAM: &str = "battering_ram";
/// Deepest overlap of two ground units a tick may end with (the game's
/// rule, which `invariants.rs` holds every match to).
const MAX_OVERLAP: f32 = 0.1;
/// A unit is through once its centre is this far past the wall (metres).
const PAST: f32 = 0.5;
/// Stuck at the end: not through and moved less than this (metres)...
const STUCK_MOVE: f32 = 1.0;
/// ...over this many final seconds.
const STUCK_SECS: usize = 10;
/// Farthest `AF_CHOKE_JITTER` moves a unit from its spot, along x and along
/// z (metres).
const JITTER: f32 = 0.15;

// ---------------------------------------------------------------------------
// The map: a wall across x with one gap
// ---------------------------------------------------------------------------

/// The wall and its gap.
#[derive(Clone, Copy, Debug)]
enum Wall {
    /// Cliff cells over x `x0..x1`, but for `cells` cells from y = `y0`.
    Cliff { x0: i32, x1: i32, y0: i32, cells: i32 },
    /// Rows of houses, `deep` houses long from x = 41, two on either side
    /// of a slot `width` metres wide centred on y = `centre` (an alley
    /// through a town), in a cliff band that runs on from the outer houses
    /// (they overlap it, so nothing leaks round them).
    Houses { centre: f32, width: f32, deep: usize },
}

/// Where the gap is: the wall's west and east faces and the gap's middle.
#[derive(Clone, Copy, Debug)]
struct Gap {
    entry: f32,
    exit: f32,
    centre: f32,
}

impl Gap {
    /// Is `p` inside the gap (between the faces, near its middle)?
    fn holds(&self, p: Vec2) -> bool {
        p.x > self.entry && p.x < self.exit && (p.y - self.centre).abs() < 3.0
    }
}

/// West face of the houses (metres).
const HOUSES_X: f32 = 41.0;

fn build_wall(app: &mut App, owner: PlayerId, wall: Wall) -> Gap {
    match wall {
        Wall::Cliff { x0, x1, y0, cells } => {
            paint(app, "cliff", x0..x1, 0..y0);
            paint(app, "cliff", x0..x1, y0 + cells..MAP as i32);
            Gap { entry: x0 as f32, exit: x1 as f32, centre: y0 as f32 + cells as f32 * 0.5 }
        }
        Wall::Houses { centre, width, deep } => {
            let data = test_data();
            let house = data.building_id("house").expect("house");
            let side = data.building(house).size[2];
            let (lo, hi) = (centre - width * 0.5, centre + width * 0.5);
            let (entry, exit) = (HOUSES_X, HOUSES_X + side * deep as f32);
            for along in 0..deep {
                let x = entry + side * (along as f32 + 0.5);
                for k in 0..2 {
                    let k = k as f32;
                    spawn_building_at(app, owner, house, Vec2::new(x, lo - side * (k + 0.5)));
                    spawn_building_at(app, owner, house, Vec2::new(x, hi + side * (k + 0.5)));
                }
            }
            paint(app, "cliff", entry as i32..exit as i32, 0..(lo - 1.5 * side) as i32);
            paint(app, "cliff", entry as i32..exit as i32, (hi + 1.5 * side).ceil() as i32..MAP as i32);
            Gap { entry, exit, centre }
        }
    }
}

/// Paint terrain `id` over the cells x0..x1, y0..y1 (exclusive ends).
fn paint(app: &mut App, id: &str, x: std::ops::Range<i32>, y: std::ops::Range<i32>) {
    let t = app.world().resource::<Data>().terrain_id(id).expect("terrain id");
    let mut nav = app.world_mut().resource_mut::<Nav>();
    for cy in y {
        for cx in x.clone() {
            nav.set_terrain(IVec2::new(cx, cy), t.0);
        }
    }
}

fn spawn_building_at(app: &mut App, player: PlayerId, kind: BuildingTypeId, pos: Vec2) -> Entity {
    with_commands(app, |c, data, players| spawn_building(c, data, players.get(player), kind, pos, 0.0, true))
}

fn spawn(app: &mut App, player: PlayerId, id: &str, pos: Vec2, facing: f32) -> Entity {
    let pos = pos + jitter(pos);
    with_commands(app, |c, data, players| {
        let kind = data.unit_id(id).unwrap_or_else(|| panic!("unit {id}"));
        spawn_unit(c, data, players.get(player), kind, pos, facing)
    })
}

/// How far `AF_CHOKE_JITTER` moves a unit spawned at `pos`: up to
/// [`JITTER`] each way, a hash of the seed and the spot (zero without it).
fn jitter(pos: Vec2) -> Vec2 {
    let Some(seed) = std::env::var("AF_CHOKE_JITTER").ok().and_then(|v| v.parse::<u32>().ok()) else { return Vec2::ZERO };
    let hash = |salt: u32| {
        let mut x = seed.wrapping_mul(0x9E37_79B1) ^ pos.x.to_bits().wrapping_mul(0x85EB_CA6B) ^ pos.y.to_bits().wrapping_mul(0xC2B2_AE35) ^ salt;
        x ^= x >> 15;
        x = x.wrapping_mul(0x2C1B_3C6D);
        x ^= x >> 13;
        (x % 1001) as f32 / 500.0 - 1.0
    };
    Vec2::new(hash(1), hash(2)) * JITTER
}

/// Size (width across, length along) of a unit type.
fn size_of(id: &str) -> Vec2 {
    let data = test_data();
    let def = data.unit(data.unit_id(id).unwrap_or_else(|| panic!("unit {id}")));
    Vec2::new(def.size[0], def.size[2])
}

/// A block of `n` units of `player` standing `cols` abreast (along y,
/// centred on `centre_y`), rank after rank away from the front rank at
/// x = `front`, all facing east or west; unit `i` is of type
/// `kinds[i % kinds.len()]`. Spaced for the largest type with a little
/// room, as a group stands after a move.
#[derive(Clone, Copy)]
struct Block<'a> {
    player: PlayerId,
    kinds: &'a [&'a str],
    n: usize,
    cols: usize,
    front: f32,
    centre_y: f32,
    east: bool,
}

impl Block<'_> {
    /// Type, position and facing of unit `i`.
    fn spot(&self, i: usize) -> (&str, Vec2, f32) {
        let biggest = self.kinds.iter().map(|k| size_of(k)).fold(Vec2::ZERO, Vec2::max);
        let (across, deep) = (biggest.x + 0.4, biggest.y + 0.5);
        let (rank, col) = ((i / self.cols) as f32, (i % self.cols) as f32);
        let back = if self.east { -1.0 } else { 1.0 };
        let at = Vec2::new(self.front + back * rank * deep, self.centre_y + (col - (self.cols - 1) as f32 * 0.5) * across);
        let facing = if self.east { FRAC_PI_2 } else { -FRAC_PI_2 };
        (self.kinds[i % self.kinds.len()], at, facing)
    }
}

/// Spawn the blocks one unit of each in turn (the first of every block,
/// then the second...), so their units' ages interleave as they do in a
/// match where both sides trained over time: of two units meeting head-on
/// the younger gives way, and age then favours neither block.
fn spawn_blocks(app: &mut App, blocks: &[Block]) -> Vec<Vec<Entity>> {
    let mut out: Vec<Vec<Entity>> = blocks.iter().map(|b| Vec::with_capacity(b.n)).collect();
    for i in 0..blocks.iter().map(|b| b.n).max().unwrap_or(0) {
        for (b, units) in blocks.iter().zip(&mut out) {
            if i < b.n {
                let (kind, at, facing) = b.spot(i);
                units.push(spawn(app, b.player, kind, at, facing));
            }
        }
    }
    out
}

fn block(app: &mut App, b: Block) -> Vec<Entity> {
    spawn_blocks(app, &[b]).remove(0)
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

/// Units ordered together, and where they count as through.
struct Group {
    units: Vec<Entity>,
    /// Through once past x = `line`, eastward or westward.
    line: f32,
    east: bool,
}

impl Group {
    fn east(units: Vec<Entity>, gap: &Gap) -> Group {
        Group { units, line: gap.exit + PAST, east: true }
    }
    fn west(units: Vec<Entity>, gap: &Gap) -> Group {
        Group { units, line: gap.entry - PAST, east: false }
    }
    fn through(&self, p: Vec2) -> bool {
        if self.east { p.x > self.line } else { p.x < self.line }
    }
}

/// A scenario ready to run: the world with everyone standing, the orders
/// to issue at time zero, the groups measured, and the bystanders.
struct Setup {
    app: App,
    gap: Gap,
    orders: Vec<PlayerCommand>,
    groups: Vec<Group>,
    idle: Vec<Entity>,
    secs: u32,
}

/// A world with one player of [`FACTION`] per entry of `teams` (its team),
/// and the sim plugins.
fn world(teams: &[u8]) -> (App, Vec<PlayerId>) {
    let mut app = sim_app(test_data());
    app.add_plugins((af_movement::MovementPlugin, af_behavior::BehaviorPlugin, af_combat::CombatPlugin, af_economy::EconomyPlugin));
    let setup: Vec<(&str, u8)> = teams.iter().map(|t| (FACTION, *t)).collect();
    let players = flat_world(&mut app, MAP, &setup);
    (app, players)
}

fn order(player: PlayerId, units: &[Entity], kind: CommandKind) -> PlayerCommand {
    PlayerCommand::new(player, units.to_vec(), kind)
}

/// 40 workers and infantry through a gap one cell wide in a cliff band six
/// metres thick, to a point 15 m past it.
fn one_way_cliff_gap_1() -> Setup {
    let (mut app, p) = world(&[0]);
    let gap = build_wall(&mut app, p[0], Wall::Cliff { x0: 40, x1: 46, y0: 48, cells: 1 });
    let units = block(&mut app, Block { player: p[0], kinds: &[WORKER, SPEAR, WORKER, HEAVY], n: 40, cols: 8, front: 34.0, centre_y: gap.centre, east: true });
    let orders = vec![order(p[0], &units, CommandKind::Move(Vec2::new(gap.exit + 15.0, gap.centre)))];
    Setup { app, gap, orders, groups: vec![Group::east(units, &gap)], idle: Vec::new(), secs: 60 }
}

/// 30 infantry and 10 knights (every fourth unit) through a slot between
/// two houses 1.2 m wide, to a point 15 m past it: just wider than a
/// knight, too narrow for two infantry abreast, and placed as a player
/// places houses, off the nav grid (its one open cell column runs 0.1 m
/// off its middle). A slot two infantry wide (1.4 m and more) flows.
fn one_way_building_gap() -> Setup {
    let (mut app, p) = world(&[0]);
    let gap = build_wall(&mut app, p[0], Wall::Houses { centre: 48.4, width: 1.2, deep: 1 });
    let units = block(&mut app, Block { player: p[0], kinds: &[SPEAR, HEAVY, SPEAR, RIDER], n: 40, cols: 8, front: 35.0, centre_y: gap.centre, east: true });
    let orders = vec![order(p[0], &units, CommandKind::Move(Vec2::new(gap.exit + 15.0, gap.centre)))];
    Setup { app, gap, orders, groups: vec![Group::east(units, &gap)], idle: Vec::new(), secs: 60 }
}

/// 20 against 20 of two hostile players (both set Passive, so it measures
/// movement, not a fight) crossing each other through a channel two cells
/// wide and ten long.
fn head_on_gap_2_enemies() -> Setup {
    let (mut app, p) = world(&[0, 1]);
    let gap = build_wall(&mut app, p[0], Wall::Cliff { x0: 40, x1: 50, y0: 48, cells: 2 });
    let blocks = [
        Block { player: p[0], kinds: &[SPEAR, WORKER], n: 20, cols: 5, front: 34.0, centre_y: gap.centre, east: true },
        Block { player: p[1], kinds: &[SPEAR, WORKER], n: 20, cols: 5, front: 56.0, centre_y: gap.centre, east: false },
    ];
    let [east, west]: [Vec<Entity>; 2] = spawn_blocks(&mut app, &blocks).try_into().expect("two blocks");
    let orders = vec![
        order(p[0], &east, CommandKind::SetStance(Stance::Passive)),
        order(p[1], &west, CommandKind::SetStance(Stance::Passive)),
        order(p[0], &east, CommandKind::Move(Vec2::new(gap.exit + 16.0, gap.centre))),
        order(p[1], &west, CommandKind::Move(Vec2::new(gap.entry - 16.0, gap.centre))),
    ];
    Setup { app, gap, orders, groups: vec![Group::east(east, &gap), Group::west(west, &gap)], idle: Vec::new(), secs: 60 }
}

/// 12 against 12 of one player crossing each other through a gap one cell wide.
fn head_on_gap_1_allies() -> Setup {
    let (mut app, p) = world(&[0]);
    let gap = build_wall(&mut app, p[0], Wall::Cliff { x0: 40, x1: 46, y0: 48, cells: 1 });
    let blocks = [
        Block { player: p[0], kinds: &[SPEAR, WORKER], n: 12, cols: 4, front: 34.0, centre_y: gap.centre, east: true },
        Block { player: p[0], kinds: &[WORKER, SPEAR], n: 12, cols: 4, front: 52.0, centre_y: gap.centre, east: false },
    ];
    let [east, west]: [Vec<Entity>; 2] = spawn_blocks(&mut app, &blocks).try_into().expect("two blocks");
    let orders = vec![
        order(p[0], &east, CommandKind::Move(Vec2::new(gap.exit + 15.0, gap.centre))),
        order(p[0], &west, CommandKind::Move(Vec2::new(gap.entry - 15.0, gap.centre))),
    ];
    Setup { app, gap, orders, groups: vec![Group::east(east, &gap), Group::west(west, &gap)], idle: Vec::new(), secs: 60 }
}

/// 60 units (a fifth of them knights) in three ranks spread 40 m wide,
/// their line centred 20 m to the side of a gap two cells wide,
/// attack-moving to a point past it: the crowd comes along the cliff face
/// and packs into the mouth from one side.
fn wide_funnel() -> Setup {
    let (mut app, p) = world(&[0]);
    let gap = build_wall(&mut app, p[0], Wall::Cliff { x0: 40, x1: 46, y0: 48, cells: 2 });
    let mut units = Vec::new();
    for i in 0..60 {
        let (rank, col) = ((i / 20) as f32, (i % 20) as f32);
        let kind = [SPEAR, HEAVY, RIDER, WORKER, SPEAR][i % 5];
        units.push(spawn(&mut app, p[0], kind, Vec2::new(34.0 - rank * 2.5, gap.centre + 20.0 + (col - 9.5) * 2.0), FRAC_PI_2));
    }
    let orders = vec![order(p[0], &units, CommandKind::AttackMove(Vec2::new(gap.exit + 15.0, gap.centre)))];
    Setup { app, gap, orders, groups: vec![Group::east(units, &gap)], idle: Vec::new(), secs: 60 }
}

/// Eight idle knights left standing two abreast in a corridor two cells
/// wide and eight long (four inside, four in and beyond its exit), and 30
/// workers and infantry of the same player ordered through it.
fn idle_plug() -> Setup {
    let (mut app, p) = world(&[0]);
    let gap = build_wall(&mut app, p[0], Wall::Cliff { x0: 40, x1: 48, y0: 48, cells: 2 });
    let (lo, hi) = (gap.centre - 0.5, gap.centre + 0.5);
    let parked = [(42.0, lo), (42.0, hi), (45.0, lo), (45.0, hi), (48.6, lo), (48.6, hi), (51.0, gap.centre - 1.0), (51.0, gap.centre + 1.0)];
    let idle: Vec<Entity> = parked.iter().map(|&(x, y)| spawn(&mut app, p[0], RIDER, Vec2::new(x, y), FRAC_PI_2)).collect();
    let units = block(&mut app, Block { player: p[0], kinds: &[WORKER, SPEAR], n: 30, cols: 6, front: 34.0, centre_y: gap.centre, east: true });
    let orders = vec![order(p[0], &units, CommandKind::Move(Vec2::new(gap.exit + 15.0, gap.centre)))];
    Setup { app, gap, orders, groups: vec![Group::east(units, &gap)], idle, secs: 60 }
}

/// 40 units ordered to a point 2 m past the exit of a gap two cells wide:
/// the formation's slots lie on the cliff about the exit and in the gap.
fn goal_just_past_exit() -> Setup {
    let (mut app, p) = world(&[0]);
    let gap = build_wall(&mut app, p[0], Wall::Cliff { x0: 40, x1: 46, y0: 48, cells: 2 });
    let units = block(&mut app, Block { player: p[0], kinds: &[WORKER, SPEAR, WORKER, HEAVY], n: 40, cols: 8, front: 34.0, centre_y: gap.centre, east: true });
    let orders = vec![order(p[0], &units, CommandKind::Move(Vec2::new(gap.exit + 2.0, gap.centre)))];
    Setup { app, gap, orders, groups: vec![Group::east(units, &gap)], idle: Vec::new(), secs: 60 }
}

/// A battering ram (1.8 m wide) at the head of 20 workers and infantry and
/// six knights, attack-moving together through a 2.5 m slot between two
/// houses (which the ram's path clearance does not fit through).
fn mixed_sizes() -> Setup {
    let (mut app, p) = world(&[0]);
    let gap = build_wall(&mut app, p[0], Wall::Houses { centre: 49.0, width: 2.5, deep: 1 });
    let mut units = vec![spawn(&mut app, p[0], RAM, Vec2::new(35.0, gap.centre), FRAC_PI_2)];
    units.extend(block(&mut app, Block { player: p[0], kinds: &[WORKER, SPEAR], n: 20, cols: 5, front: 32.0, centre_y: gap.centre, east: true }));
    units.extend(block(&mut app, Block { player: p[0], kinds: &[RIDER], n: 6, cols: 3, front: 25.5, centre_y: gap.centre, east: true }));
    let orders = vec![order(p[0], &units, CommandKind::AttackMove(Vec2::new(gap.exit + 15.0, gap.centre)))];
    Setup { app, gap, orders, groups: vec![Group::east(units, &gap)], idle: Vec::new(), secs: 60 }
}

/// A scenario's name and how to set it up.
type Scenario = (&'static str, fn() -> Setup);

/// Every scenario, by name.
const SCENARIOS: [Scenario; 8] = [
    ("one_way_cliff_gap_1", one_way_cliff_gap_1),
    ("one_way_building_gap", one_way_building_gap),
    ("head_on_gap_2_enemies", head_on_gap_2_enemies),
    ("head_on_gap_1_allies", head_on_gap_1_allies),
    ("wide_funnel", wide_funnel),
    ("idle_plug", idle_plug),
    ("goal_just_past_exit", goal_just_past_exit),
    ("mixed_sizes", mixed_sizes),
];

fn scenario(name: &str) -> fn() -> Setup {
    SCENARIOS.iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("no scenario {name}")).1
}

// ---------------------------------------------------------------------------
// Running and measuring
// ---------------------------------------------------------------------------

/// How a unit's order ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ended {
    Walking,
    Arrived,
    Failed,
}

/// One ordered unit, followed tick by tick.
#[derive(Clone, Debug)]
struct Track {
    entity: Entity,
    group: usize,
    /// Its order has reached it (the command runs the tick after it is sent).
    ordered: bool,
    /// The path status af_behavior saw this tick (the last tick's movement).
    status: PathStatus,
    ended: Ended,
    through_at: Option<f32>,
    /// Position at the end of every whole second since the order.
    trail: Vec<Vec2>,
}

/// What one run of a scenario came to.
#[derive(Clone, Debug)]
struct Outcome {
    name: &'static str,
    total: usize,
    through: usize,
    arrived: usize,
    failed: usize,
    /// Arrived but not through.
    short: usize,
    /// Seconds after the order until 50 %, 90 % and all were through.
    t50: Option<f32>,
    t90: Option<f32>,
    t100: Option<f32>,
    /// When each unit that got through did (seconds after the order),
    /// earliest first.
    times: Vec<f32>,
    stuck: usize,
    max_overlap: f32,
    max_overlap_at: f32,
    /// The solver's counters over the run (from the order on).
    stats: CollisionStats,
    /// Every unit's position and facing at the end, bit for bit, in spawn order.
    end: Vec<(u32, u32, u32)>,
}

/// Status of `e`'s current goal as af_behavior reads it (a path computed
/// for an older goal counts as pending).
fn goal_status(world: &World, e: Entity) -> PathStatus {
    let Some(path) = world.get::<PathState>(e) else { return PathStatus::Idle };
    match world.get::<MoveGoal>(e) {
        None => PathStatus::Idle,
        Some(g) if g.seq == path.goal_seq => path.status,
        Some(_) => PathStatus::Pending,
    }
}

fn has_order(world: &World, e: Entity) -> bool {
    world.get::<Orders>(e).is_some_and(|o| o.current().is_some())
}

/// Every ground unit's box.
fn ground_boxes(app: &mut App) -> Vec<(Entity, Obb, f32)> {
    let w = app.world_mut();
    let mut q = w.query_filtered::<(Entity, &Position, &Facing, &Footprint, &Mobile), (With<Unit>, Without<Dead>)>();
    let mut boxes: Vec<(Entity, Obb, f32)> = q.iter(w).filter(|(.., m)| !m.flying).map(|(e, p, f, fp, _)| (e, obb_of(p, f, fp), fp.half.length())).collect();
    boxes.sort_unstable_by_key(|b| b.0);
    boxes
}

/// Deepest overlap of two ground units right now.
fn deepest_overlap(app: &mut App) -> f32 {
    let boxes = ground_boxes(app);
    let mut worst = 0.0f32;
    for (i, (_, a, ra)) in boxes.iter().enumerate() {
        for (_, b, rb) in &boxes[i + 1..] {
            if a.center.distance_squared(b.center) < (ra + rb) * (ra + rb)
                && let Some(m) = a.mtv(b)
            {
                worst = worst.max(m.length());
            }
        }
    }
    worst
}

/// Is `name` listed in the environment variable `var` (or is it `all`)?
fn listed(var: &str, name: &str) -> bool {
    std::env::var(var).is_ok_and(|v| v.split(',').any(|s| s.trim() == name || s.trim() == "all"))
}

/// Run a scenario for `secs` simulated seconds after the order (its own
/// length when `None`) and measure it.
fn run(name: &'static str, secs: Option<u32>) -> Outcome {
    let Setup { mut app, gap, orders, groups, idle, secs: own } = scenario(name)();
    let secs = secs.unwrap_or(own);
    let trace = listed("AF_CHOKE_TRACE", name);
    // Everyone stands for a moment first (movement state attached, the
    // obstacles registered), as an army does before the player clicks.
    run_ticks(&mut app, 3);
    let before = app.world().resource::<CollisionStats>().clone();
    for c in orders {
        command(&mut app, c);
    }
    let mut tracks: Vec<Track> = groups
        .iter()
        .enumerate()
        .flat_map(|(g, group)| {
            group.units.iter().map(move |&entity| Track { entity, group: g, ordered: false, status: PathStatus::Idle, ended: Ended::Walking, through_at: None, trail: Vec::new() })
        })
        .collect();
    let total = tracks.len();
    let (mut max_overlap, mut max_overlap_at) = (0.0f32, 0.0f32);
    let mut last_held = before.held;
    let ticks = secs * SIM_HZ as u32;
    for tick in 1..=ticks {
        run_ticks(&mut app, 1);
        let now = tick as f32 / SIM_HZ as f32;
        let world = app.world();
        for t in &mut tracks {
            let pos = world.get::<Position>(t.entity).expect("position").0;
            let ordered = has_order(world, t.entity);
            if t.ordered && !ordered && t.ended == Ended::Walking {
                // The order popped this tick, on what af_behavior saw: the
                // status our last look found.
                t.ended = if t.status == PathStatus::Failed { Ended::Failed } else { Ended::Arrived };
            }
            t.ordered |= ordered;
            t.status = goal_status(world, t.entity);
            if t.through_at.is_none() && groups[t.group].through(pos) {
                t.through_at = Some(now);
            }
            if tick % SIM_HZ as u32 == 0 {
                t.trail.push(pos);
            }
        }
        let overlap = deepest_overlap(&mut app);
        if overlap > max_overlap {
            (max_overlap, max_overlap_at) = (overlap, now);
        }
        if trace && tick % SIM_HZ as u32 == 0 {
            let held = app.world().resource::<CollisionStats>().held;
            println!("{}", trace_line(&app, &tracks, &gap, now, held - last_held));
            last_held = held;
            if tick % (5 * SIM_HZ as u32) == 0 {
                println!("{}", gap_map(&app, &tracks, &groups, &idle, &gap));
            }
            if listed("AF_CHOKE_UNITS", name) {
                println!("{}", mouth_units(&app, &tracks, &idle, &gap));
            }
        }
    }

    let mut times: Vec<f32> = tracks.iter().filter_map(|t| t.through_at).collect();
    times.sort_by(f32::total_cmp);
    let time_for = |fraction: f32| {
        let need = ((total as f32 * fraction).ceil() as usize).max(1);
        times.get(need - 1).copied()
    };
    let stuck = tracks
        .iter()
        .filter(|t| t.through_at.is_none() && t.trail.len() > STUCK_SECS && t.trail[t.trail.len() - 1].distance(t.trail[t.trail.len() - 1 - STUCK_SECS]) < STUCK_MOVE)
        .count();
    let after = app.world().resource::<CollisionStats>().clone();
    let stats = CollisionStats {
        ticks: after.ticks - before.ticks,
        held: after.held - before.held,
        parted: after.parted - before.parted,
        relocated: after.relocated - before.relocated,
        left: after.left - before.left,
        overlapped: after.overlapped - before.overlapped,
        jumps_final: after.jumps_final - before.jumps_final,
        jumps_eject: after.jumps_eject - before.jumps_eject,
        jumps_passes: after.jumps_passes - before.jumps_passes,
        ..after
    };
    let world = app.world();
    let end = tracks
        .iter()
        .map(|t| t.entity)
        .chain(idle.iter().copied())
        .map(|e| {
            let (p, f) = (world.get::<Position>(e).expect("position").0, world.get::<Facing>(e).expect("facing").0);
            (p.x.to_bits(), p.y.to_bits(), f.to_bits())
        })
        .collect();
    if trace {
        println!("{name} at the end:\n{}", gap_map(&app, &tracks, &groups, &idle, &gap));
    }
    Outcome {
        name,
        total,
        through: times.len(),
        arrived: tracks.iter().filter(|t| t.ended == Ended::Arrived).count(),
        failed: tracks.iter().filter(|t| t.ended == Ended::Failed).count(),
        short: tracks.iter().filter(|t| t.ended == Ended::Arrived && t.through_at.is_none()).count(),
        t50: time_for(0.5),
        t90: time_for(0.9),
        t100: time_for(1.0),
        times,
        stuck,
        max_overlap,
        max_overlap_at,
        stats,
        end,
    }
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

fn secs(t: Option<f32>) -> String {
    t.map_or("-".to_string(), |t| format!("{t:.1}"))
}

fn table(outcomes: &[Outcome]) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "| scenario | through | arrived | failed | short | t50 s | t90 s | t100 s | stuck | max overlap m | held | parted | relocated | left | jumps |");
    let _ = writeln!(s, "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for o in outcomes {
        let st = &o.stats;
        let _ = writeln!(
            s,
            "| {} | {}/{} | {} | {} | {} | {} | {} | {} | {} | {:.3} (t={:.1}) | {} | {} | {} | {} | {} |",
            o.name,
            o.through,
            o.total,
            o.arrived,
            o.failed,
            o.short,
            secs(o.t50),
            secs(o.t90),
            secs(o.t100),
            o.stuck,
            o.max_overlap,
            o.max_overlap_at,
            st.held,
            st.parted,
            st.relocated,
            st.left,
            st.jumps_final + st.jumps_eject + st.jumps_passes,
        );
    }
    s
}

/// One trace line: through, in the gap, failed so far, units stuck for a
/// second or more (`PathState::stuck_time`), units walking, and how many
/// unit-ticks the solver held back over the last second.
fn trace_line(app: &App, tracks: &[Track], gap: &Gap, now: f32, held: u64) -> String {
    let world = app.world();
    let pos = |t: &Track| world.get::<Position>(t.entity).expect("position").0;
    let through = tracks.iter().filter(|t| t.through_at.is_some()).count();
    let in_gap = tracks.iter().filter(|t| gap.holds(pos(t))).count();
    let failed = tracks.iter().filter(|t| t.ended == Ended::Failed).count();
    let walking = tracks.iter().filter(|t| t.ended == Ended::Walking).count();
    let stuck = tracks.iter().filter(|t| world.get::<PathState>(t.entity).is_some_and(|p| p.stuck_time >= 1.0)).count();
    format!("t={now:>4.0}s through {through:>2}/{} in gap {in_gap:>2} walking {walking:>2} failed {failed:>2} stuck {stuck:>2} held {held:>4}/s", tracks.len())
}

/// Every unit within two metres of the gap: where it stands and faces,
/// how it moves, its order, its path's progress and stuck time.
fn mouth_units(app: &App, tracks: &[Track], idle: &[Entity], gap: &Gap) -> String {
    let world = app.world();
    let data = world.resource::<Data>();
    let mut s = String::new();
    for e in tracks.iter().map(|t| t.entity).chain(idle.iter().copied()) {
        let p = world.get::<Position>(e).expect("position").0;
        if p.x < gap.entry - 2.0 || p.x > gap.exit + 2.0 || (p.y - gap.centre).abs() > 3.0 {
            continue;
        }
        let kind = &data.unit(world.get::<Unit>(e).expect("unit").kind).id;
        let facing = world.get::<Facing>(e).expect("facing").0.to_degrees();
        let v = world.get::<Velocity>(e).map_or(Vec2::ZERO, |v| v.0);
        let path = world.get::<PathState>(e).expect("path");
        let next = path.waypoints.get(path.next).map_or("-".to_string(), |w| format!("({:.1},{:.1})", w.x, w.y));
        let order = world.get::<Orders>(e).and_then(|o| o.current().copied());
        let _ = writeln!(
            s,
            "  {e} {kind:<12} at ({:.2},{:.2}) facing {facing:>4.0} v ({:.2},{:.2}) {:?} next {next} of {} stuck {:.1} {order:?}",
            p.x,
            p.y,
            v.x,
            v.y,
            goal_status(world, e),
            path.waypoints.len(),
            path.stuck_time
        );
    }
    s
}

/// The gap and 10 m either side at half-metre resolution (see the module docs).
fn gap_map(app: &App, tracks: &[Track], groups: &[Group], idle: &[Entity], gap: &Gap) -> String {
    const RES: f32 = 0.5;
    let world = app.world();
    let nav = world.resource::<Nav>();
    let data = world.resource::<Data>();
    let cliff = data.terrain_id("cliff").expect("cliff");
    let (x0, x1, y0, y1) = (gap.entry - 10.0, gap.exit + 10.0, gap.centre - 6.0, gap.centre + 6.0);
    let (cols, rows) = (((x1 - x0) / RES) as usize, ((y1 - y0) / RES) as usize);
    let mut grid = vec![vec![' '; cols]; rows];
    for (r, row) in grid.iter_mut().enumerate() {
        for (c, ch) in row.iter_mut().enumerate() {
            let p = Vec2::new(x0 + (c as f32 + 0.5) * RES, y0 + (r as f32 + 0.5) * RES);
            if nav.terrain_at(nav.cell_of(p)) == cliff.0 {
                *ch = '#';
            } else if nav.is_walk_blocked(nav.cell_of(p)) {
                *ch = '.';
            }
        }
    }
    let mut houses = world.try_query_filtered::<(&Position, &Facing, &Footprint), With<Building>>().expect("building query");
    for (p, f, fp) in houses.iter(world) {
        let obb = obb_of(p, f, fp);
        for (r, row) in grid.iter_mut().enumerate() {
            for (c, ch) in row.iter_mut().enumerate() {
                let at = Vec2::new(x0 + (c as f32 + 0.5) * RES, y0 + (r as f32 + 0.5) * RES);
                if obb.contains(at) {
                    *ch = 'H';
                }
            }
        }
    }
    let mut mark = |p: Vec2, ch: char| {
        let (c, r) = (((p.x - x0) / RES).floor(), ((p.y - y0) / RES).floor());
        if c >= 0.0 && r >= 0.0 && (c as usize) < cols && (r as usize) < rows {
            grid[r as usize][c as usize] = ch;
        }
    };
    for &e in idle {
        mark(world.get::<Position>(e).expect("position").0, 'i');
    }
    for t in tracks {
        let p = world.get::<Position>(t.entity).expect("position").0;
        let stuck = world.get::<PathState>(t.entity).is_some_and(|s| s.stuck_time >= 1.0);
        let ch = match t.ended {
            Ended::Failed => 'x',
            Ended::Arrived if t.through_at.is_some() => 'a',
            Ended::Arrived => 's',
            Ended::Walking if stuck => '!',
            Ended::Walking if groups[t.group].east => '>',
            Ended::Walking => '<',
        };
        mark(p, ch);
    }
    let mut s = format!("x {x0:.0}..{x1:.0} left to right, y {y1:.0} (top) to {y0:.0}\n");
    for row in grid.iter().rev() {
        s.push('|');
        s.extend(row.iter());
        s.push_str("|\n");
    }
    s
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Every scenario (or those `AF_CHOKE` lists), as a markdown table.
#[test]
#[ignore = "measurement: cargo test -p aetherfall --test chokepoints chokepoint_report -- --ignored --nocapture"]
fn chokepoint_report() {
    let only = std::env::var("AF_CHOKE").ok();
    let outcomes: Vec<Outcome> = SCENARIOS
        .iter()
        .filter(|(name, _)| only.as_deref().is_none_or(|_| listed("AF_CHOKE", name)))
        .map(|(name, _)| {
            let started = std::time::Instant::now();
            let o = run(name, None);
            eprintln!("{name}: {:.1} s wall", started.elapsed().as_secs_f32());
            o
        })
        .collect();
    println!("\n{}", table(&outcomes));
}

/// In every scenario (crowds pressing into gaps one cell wide, two hostile
/// crowds crossing in a channel, a ram, infantry and knights squeezing
/// through a slot between houses...), no tick ends with two units
/// overlapping past the game's rule.
#[test]
fn chokepoint_crowds_never_overlap() {
    for (name, _) in SCENARIOS {
        let o = run(name, None);
        assert!(o.max_overlap <= MAX_OVERLAP, "{name}: units overlapped by {:.3} m at t={:.1}s\n{}", o.max_overlap, o.max_overlap_at, table(std::slice::from_ref(&o)));
    }
}

/// What a scenario must come to on its default layout: at least `through`
/// units through, at most `failed` orders failed and `short` units arrived
/// short of the gap, and the first `through` of them through within `secs`
/// seconds of the order.
struct Bar {
    name: &'static str,
    through: usize,
    failed: usize,
    short: usize,
    secs: f32,
}

/// Every scenario's bar: everyone through and no order failed, but for one
/// unit of the crowd ordered just past the exit, whose formation slot lies
/// on the cliff beside it (it stops at the mouth), and the ram, whose path
/// clearance does not fit its slot (its order fails where it stands). The
/// times leave about a third over what the bench measures. Before the
/// collision solver let crowds file through (slipping round corners of the
/// terrain, waiting their turn in a queue, a file going on giving way to
/// the column it met), 3 of 40 got through the cliff gap, 11 of 40 across
/// the channel, 9 of 24 across the one-cell gap, and 106 orders failed
/// over the bench.
const BARS: [Bar; 8] = [
    Bar { name: "one_way_cliff_gap_1", through: 40, failed: 0, short: 0, secs: 32.0 },
    Bar { name: "one_way_building_gap", through: 40, failed: 0, short: 0, secs: 28.0 },
    Bar { name: "head_on_gap_2_enemies", through: 40, failed: 0, short: 0, secs: 52.0 },
    Bar { name: "head_on_gap_1_allies", through: 24, failed: 0, short: 0, secs: 25.0 },
    Bar { name: "wide_funnel", through: 60, failed: 0, short: 0, secs: 45.0 },
    Bar { name: "idle_plug", through: 30, failed: 0, short: 0, secs: 16.0 },
    Bar { name: "goal_just_past_exit", through: 39, failed: 0, short: 1, secs: 20.0 },
    Bar { name: "mixed_sizes", through: 26, failed: 1, short: 0, secs: 16.0 },
];

/// Every scenario's crowd gets through as its bar ([`BARS`]) asks: the
/// crowds that used to gridlock in gaps and channels file through.
#[test]
fn chokepoint_crowds_get_through() {
    let mut missed = Vec::new();
    let mut outcomes = Vec::new();
    for (name, _) in SCENARIOS {
        let bar = BARS.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no bar for {name}"));
        let o = run(name, None);
        let within = o.times.get(bar.through.saturating_sub(1)).copied();
        if o.through < bar.through || o.failed > bar.failed || o.short > bar.short || within.is_none_or(|t| t > bar.secs) {
            missed.push(format!(
                "{name}: {}/{} through (bar {}), {} failed (bar {}), {} short (bar {}), first {} through at {} s (bar {:.0} s)",
                o.through,
                o.total,
                bar.through,
                o.failed,
                bar.failed,
                o.short,
                bar.short,
                bar.through,
                secs(within),
                bar.secs
            ));
        }
        outcomes.push(o);
    }
    assert!(missed.is_empty(), "{}\n\n{}", missed.join("\n"), table(&outcomes));
}

/// The same scenario run twice ends with every unit in the same place,
/// facing the same way, bit for bit.
#[test]
fn chokepoint_runs_are_deterministic() {
    for name in ["one_way_cliff_gap_1", "head_on_gap_1_allies"] {
        let (a, b) = (run(name, Some(30)), run(name, Some(30)));
        assert!(a.end == b.end, "{name}: two runs diverged");
    }
}
