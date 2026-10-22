//! Decision tests on a flat world with only af_core and (where noted) the
//! AI plugin: no movement, behaviour or economy crates, so nothing the AI
//! orders is executed; tests inspect the decisions and commands directly.

use std::sync::Arc;

use af_core::ext::economy::RevealedPlayers;
use af_core::ext::map::MapFeatures;
use af_core::ext::movement::NavRegions;
use af_core::spawn;
use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};
use bevy::ecs::system::RunSystemOnce;

use super::*;
use crate::economy::{FIELDS_PER_HUB, FieldSpot, Supply, apportion_capped, field_spot};
use crate::plan::passive_income;
use crate::progress::cost_total;
use crate::view::{BuildingQuery, NodeQuery, TargetQuery, UnitQuery};

const MAP: f32 = 200.0;

struct Match {
    app: App,
    data: Arc<GameData>,
    players: Vec<PlayerId>,
}

impl Match {
    fn new(factions: &[&str]) -> Match {
        Match::with_data(test_data(), factions)
    }

    fn with_data(data: Arc<GameData>, factions: &[&str]) -> Match {
        let mut app = sim_app(data.clone());
        let teams: Vec<(&str, u8)> = factions.iter().enumerate().map(|(i, f)| (*f, i as u8)).collect();
        let players = flat_world(&mut app, MAP, &teams);
        Match { app, data, players }
    }

    fn with_ai_plugin(mut self) -> Match {
        self.app.add_plugins(AiPlugin);
        self
    }

    fn p(&self, i: usize) -> PlayerId {
        self.players[i]
    }

    fn start(&self, i: usize) -> Vec2 {
        self.player(i).start_pos
    }

    fn player(&self, i: usize) -> &PlayerState {
        self.app.world().resource::<Players>().get(self.players[i])
    }

    fn edit_player(&mut self, i: usize, f: impl FnOnce(&mut PlayerState)) {
        let p = self.players[i];
        f(self.app.world_mut().resource_mut::<Players>().get_mut(p));
    }

    fn building_id(&self, id: &str) -> BuildingTypeId {
        self.data.building_id(id).unwrap_or_else(|| panic!("no building {id}"))
    }

    fn unit_id(&self, id: &str) -> UnitTypeId {
        self.data.unit_id(id).unwrap_or_else(|| panic!("no unit {id}"))
    }

    fn building(&mut self, i: usize, kind: BuildingTypeId, pos: Vec2, complete: bool) -> Entity {
        let p = self.players[i];
        with_commands(&mut self.app, |c, data, players| spawn::spawn_building(c, data, players.get(p), kind, pos, 0.0, complete))
    }

    fn unit(&mut self, i: usize, kind: UnitTypeId, pos: Vec2) -> Entity {
        let p = self.players[i];
        with_commands(&mut self.app, |c, data, players| spawn::spawn_unit(c, data, players.get(p), kind, pos, 0.0))
    }

    fn workers(&mut self, i: usize, n: usize, around: Vec2) -> Vec<Entity> {
        let kind = self.data.faction(self.player(i).faction).worker_id;
        (0..n).map(|k| self.unit(i, kind, around + Vec2::new(k as f32 * 1.2 - 3.0, -7.0))).collect()
    }

    fn node(&mut self, id: &str, pos: Vec2) -> Entity {
        self.node_holding(id, pos, None)
    }

    /// A node holding `amount` (None = its full amount).
    fn node_holding(&mut self, id: &str, pos: Vec2, amount: Option<f32>) -> Entity {
        let kind = self.data.node_id(id).unwrap_or_else(|| panic!("no node {id}"));
        with_commands(&mut self.app, |c, data, _| spawn::spawn_node(c, data, kind, pos, 0.0, amount))
    }

    /// `n` army units of `kind` in rows of six eastward and northward from
    /// `from`, 2.5 m apart.
    fn army(&mut self, i: usize, kind: &str, from: Vec2, n: usize) -> Vec<Entity> {
        let kind = self.unit_id(kind);
        (0..n).map(|k| self.unit(i, kind, from + Vec2::new((k % 6) as f32 * 2.5, (k / 6) as f32 * 2.5))).collect()
    }

    /// The faction's main hall type for player `i`.
    fn hall_kind(&self, i: usize) -> BuildingTypeId {
        let f = self.data.faction(self.player(i).faction);
        self.data.building_id(&f.start.buildings[0]).expect("start building")
    }

    /// A complete hostile building of player `i` at `pos`, as `mem` would remember it.
    fn known(&mut self, i: usize, pos: Vec2) -> KnownBuilding {
        let kind = self.hall_kind(i);
        let entity = self.building(i, kind, pos, true);
        KnownBuilding { entity, owner: self.p(i), kind, pos, facing: 0.0 }
    }

    fn rally(&mut self, i: usize) -> Vec2 {
        let snap = self.snapshot(i);
        let view = self.view(i, &snap);
        Plan::new(&view, &AiMemory::default()).rally
    }

    fn age_up_step(&mut self, i: usize) -> AgeUpStep {
        let snap = self.snapshot(i);
        let view = self.view(i, &snap);
        age_up_step(&view, &[])
    }

    /// A complete main hall at the player's start (the faction's first start building).
    fn hall(&mut self, i: usize) -> Entity {
        let (kind, at) = (self.hall_kind(i), self.start(i));
        self.building(i, kind, at, true)
    }

    /// One sim tick: applies spawns to the spatial index and player counts.
    fn tick(&mut self) {
        run_ticks(&mut self.app, 1);
    }

    fn set_time(&mut self, secs: f32) {
        self.app.world_mut().resource_mut::<SimClock>().elapsed = secs;
    }

    fn snapshot(&mut self, i: usize) -> Snapshot {
        self.app.world_mut().run_system_once_with(collect_for, self.players[i]).expect("snapshot system")
    }

    fn view<'a>(&'a self, i: usize, snap: &'a Snapshot) -> AiView<'a> {
        let w = self.app.world();
        AiView::new(
            &w.resource::<Data>().0,
            w.resource::<Players>(),
            self.players[i],
            &w.resource::<Nav>().0,
            w.resource::<SpatialIndex>(),
            w.get_resource::<MapInfo>(),
            w.resource::<SimClock>().elapsed,
            w.resource::<FogOfWar>(),
            snap,
        )
        .expect("player exists")
        .with_revealed(w.get_resource::<RevealedPlayers>())
        .with_regions(w.get_resource::<NavRegions>())
        .with_features(w.get_resource::<MapFeatures>())
    }

    /// Run one think for player `i` with `mem`, returning its commands.
    fn think(&mut self, i: usize, mem: &mut AiMemory) -> Vec<PlayerCommand> {
        let snap = self.snapshot(i);
        let view = self.view(i, &snap);
        let mut out = Vec::new();
        think::run(&view, mem, &mut out);
        out
    }
}

fn collect_for(
    In(p): In<PlayerId>,
    data: Res<Data>,
    units: UnitQuery,
    buildings: BuildingQuery,
    targets: TargetQuery,
    nodes: NodeQuery,
) -> Snapshot {
    let mut snap = Snapshot::default();
    collect_snapshot(p, &data.0, &units, &buildings, &targets, &nodes, &mut snap);
    snap
}

fn amounts(data: &GameData, v: &[(&str, f32)]) -> ResAmounts {
    let mut a = ResAmounts::ZERO;
    for (id, x) in v {
        a.set(data.resource_id(id).expect("resource"), *x);
    }
    a
}

fn trains(cmds: &[PlayerCommand], unit: UnitTypeId) -> Vec<&PlayerCommand> {
    cmds.iter().filter(|c| matches!(c.kind, CommandKind::Train { unit: u, .. } if u == unit)).collect()
}

fn placements(cmds: &[PlayerCommand], kind: BuildingTypeId) -> usize {
    cmds.iter().filter(|c| matches!(c.kind, CommandKind::PlaceBuilding { kind: k, .. } if k == kind)).count()
}

/// Where `kind` was placed (first placement).
fn placed_at(cmds: &[PlayerCommand], kind: BuildingTypeId) -> Option<Vec2> {
    cmds.iter().find_map(|c| match c.kind {
        CommandKind::PlaceBuilding { kind: k, pos, .. } if k == kind => Some(pos),
        _ => None,
    })
}

/// The units of the attack-move ordered at (about) `pos`, sorted.
fn attack_moved_to(cmds: &[PlayerCommand], pos: Vec2) -> Option<Vec<Entity>> {
    cmds.iter().find(|c| matches!(c.kind, CommandKind::AttackMove(p) if p.distance(pos) < 1.0)).map(|c| sorted(&c.entities))
}

fn sorted(entities: &[Entity]) -> Vec<Entity> {
    let mut v = entities.to_vec();
    v.sort();
    v
}

/// Memory of an attack by `group` on `goal`, launched at `since`.
fn attacking(group: &[Entity], goal: Objective, since: f32) -> AiMemory {
    let mut mem = AiMemory { attack: AttackState::attacking(goal, since, since), ..default() };
    for u in group {
        mem.add_attacker(*u);
    }
    mem
}

// ---------------------------------------------------------------------------
// Workers and housing
// ---------------------------------------------------------------------------

#[test]
fn worker_target_grows_with_age() {
    for age in 1..=5u8 {
        assert_eq!(worker_target(age), WORKER_TARGETS[age as usize - 1]);
        assert!(age == 1 || worker_target(age) > worker_target(age - 1), "more workers every age");
    }
    assert_eq!(worker_target(0), WORKER_TARGETS[0], "clamped below");
    assert_eq!(worker_target(9), WORKER_TARGETS[4], "clamped above");
}

#[test]
fn trains_workers_until_the_target() {
    let mut m = Match::new(&["ironhold", "dune"]);
    let hall = m.hall(0);
    m.workers(0, 5, m.start(0));
    m.tick();
    let worker = m.data.faction(m.player(0).faction).worker_id;
    let cmds = m.think(0, &mut AiMemory::default());
    let t = trains(&cmds, worker);
    assert_eq!(t.len(), 1, "one worker queued per think at the single hall: {cmds:?}");
    assert_eq!(t[0].entities, vec![hall]);
    assert_eq!(t[0].player, m.p(0));

    // At the target, no more workers.
    m.edit_player(0, |p| p.resources = ResAmounts([5000.0; MAX_RESOURCES]));
    let missing = worker_target(1) - 5;
    m.workers(0, missing as usize, m.start(0) + Vec2::new(0.0, -14.0));
    // Housing so pop room is not what stops training.
    let house = m.building_id("house");
    for k in 0..3 {
        m.building(0, house, m.start(0) + Vec2::new(-25.0, -15.0 + k as f32 * 6.0), true);
    }
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(trains(&cmds, worker).is_empty(), "{} workers is the age-1 target: {cmds:?}", worker_target(1));
}

#[test]
fn house_rule_keeps_room_until_the_cap() {
    let data = test_data();
    let f = data.faction_id("ironhold").unwrap();
    let mut p = PlayerState::new(PlayerId(0), "p".into(), f, [1.0; 3], 0, Controller::Ai, &data);
    p.pop_cap = 15;
    p.pop_used = 8;
    assert!(needs_house(&p, 2, 0, 0), "5 free < 6 wanted");
    assert!(!needs_house(&p, 0, 0, 0), "7 free is enough without production");
    assert!(needs_house(&p, 0, 0, 1), "each production building wants 2 more room");
    assert!(!needs_house(&p, 2, 10, 0), "a house under way counts");
    p.pop_cap = POP_HARD_CAP;
    p.pop_used = POP_HARD_CAP - 1;
    assert!(!needs_house(&p, 0, 0, 3), "never past the hard cap");
    assert_eq!(house_kind(&data, &p), data.building_id("house"), "cheapest housing per pop");
}

#[test]
fn plans_a_house_before_pop_cap() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    m.workers(0, 12, m.start(0));
    m.tick();
    let house = m.building_id("house");
    let mut mem = AiMemory::default();
    let cmds = m.think(0, &mut mem);
    assert_eq!(placements(&cmds, house), 1, "12/15 pop needs a house: {cmds:?}");
    let (pos, builders) = cmds
        .iter()
        .find_map(|c| match c.kind {
            CommandKind::PlaceBuilding { pos, .. } => Some((pos, c.entities.len())),
            _ => None,
        })
        .unwrap();
    assert!(builders >= 1, "a builder is sent");
    assert!(pos.distance(m.start(0)) < 50.0, "house near the hall at {pos}");
    // Remembered as pending: the next think does not place another.
    m.set_time(0.5);
    let again = m.think(0, &mut mem);
    assert_eq!(placements(&again, house), 0, "no duplicate placement: {again:?}");

    // Plenty of room: no house.
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    m.workers(0, 5, m.start(0));
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert_eq!(placements(&cmds, house), 0, "6/15 pop does not need a house yet: {cmds:?}");
}

#[test]
fn idle_workers_are_sent_to_gather() {
    let mut m = Match::new(&["sylvan", "tide"]);
    m.hall(0);
    let home = m.start(0);
    let mut nodes = Vec::new();
    for k in 0..6 {
        nodes.push(m.node("berry_bush", home + Vec2::new(-12.0, -8.0 + k as f32 * 2.5)));
        nodes.push(m.node("oak", home + Vec2::new(14.0 + (k % 2) as f32 * 2.0, -6.0 + k as f32 * 2.0)));
    }
    let workers = m.workers(0, 6, home);
    m.tick();
    let mut mem = AiMemory::default();
    let cmds = m.think(0, &mut mem);
    let gathers: Vec<_> = cmds.iter().filter(|c| matches!(c.kind, CommandKind::Gather(_))).collect();
    assert_eq!(gathers.len(), workers.len(), "every idle worker gets a job: {cmds:?}");
    let food = m.data.resource_id("food").unwrap();
    let mut food_workers = 0;
    for c in &gathers {
        let CommandKind::Gather(t) = c.kind else { unreachable!() };
        assert!(nodes.contains(&t), "gathers from a real node");
        if m.app.world().get::<ResourceNode>(t).is_some_and(|n| n.resource == food) {
            food_workers += 1;
        }
    }
    assert!(food_workers >= 2, "food is the main need early on ({food_workers} on food)");
    // Not re-ordered on the next think, although nothing executed them.
    m.set_time(0.5);
    let again = m.think(0, &mut mem);
    assert!(!again.iter().any(|c| matches!(c.kind, CommandKind::Gather(_))), "no repeated gather orders: {again:?}");
}

#[test]
fn apportion_splits_by_weight() {
    let mut w = [0.0; MAX_RESOURCES];
    w[0] = 3.0;
    w[1] = 1.0;
    let a = apportion(&w, 8);
    assert_eq!((a[0], a[1]), (6, 2));
    assert_eq!(a.iter().sum::<u32>(), 8);
    w[2] = 1.0;
    let a = apportion(&w, 7);
    assert_eq!(a.iter().sum::<u32>(), 7);
    assert_eq!(apportion(&[0.0; MAX_RESOURCES], 5), [0; MAX_RESOURCES]);
}

// ---------------------------------------------------------------------------
// Building spots and drop sites
// ---------------------------------------------------------------------------

#[test]
fn build_spot_near_hall_is_valid() {
    let mut m = Match::new(&["ironhold", "dune"]);
    let hall = m.hall(0);
    let home = m.start(0);
    // A tree line in the preferred direction forces the search around it.
    for k in 0..8 {
        m.node("pine", home + Vec2::new(12.0 + k as f32, 0.0));
    }
    m.tick();
    let barracks = m.building_id("barracks");
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let q = SpotQuery { kind: barracks, anchor: home, toward: Vec2::X, min_r: 10.0, max_r: 40.0, gap: 2.5 };
    let spot = find_build_spot(&view, &q, &[], &[]).expect("a spot on open grass");
    let d = spot.pos.distance(home);
    assert!((9.0..=41.0).contains(&d), "within the searched rings: {d}");
    let ok = check_placement(view.data, view.nav, view.spatial, view.me, barracks, spot.pos, spot.facing, false, &[]);
    assert_eq!(ok.map(|p| p.pos), Ok(spot.pos), "af_core accepts the spot");
    let obb = Obb::from_size(spot.pos, view.data.building(barracks).size, spot.facing);
    let hall_obb = view.spatial.entries.iter().find(|e| e.entity == hall).unwrap().obb;
    assert!(obb.gap(&hall_obb) >= 2.4, "keeps the walking gap to the hall");
    assert!(clear_around(&view, &obb, 2.5, None, &[]));

    // A footprint already placed this tick is avoided.
    let second = find_build_spot(&view, &q, &[obb], &[]).expect("a second spot");
    let obb2 = Obb::from_size(second.pos, view.data.building(barracks).size, second.facing);
    assert!(!obb2.overlaps(&obb) && obb2.gap(&obb) >= 2.4, "no overlap with the pending footprint");
}

/// Two barracks 4 m apart with a third behind them leave a bay whose
/// mouth a ram (a mover of clearance 1) passes. A house laid across the
/// mouth with the usual gap would close the bay on it: workers still slip
/// out through the 1.6 m slits, a ram does not. The spot search skips that
/// spot, and the bay stays open, by the real path finder on a nav grid
/// that holds the buildings as af_movement registers them.
#[test]
fn a_new_building_never_closes_a_bay_on_wide_movers() {
    use af_geom::{NO_TEAM, PathRequest};
    let mut m = Match::new(&["ironhold", "dune"]);
    let barracks = m.building_id("barracks");
    let house = m.building_id("house");
    let bay = Vec2::new(100.0, 60.0);
    let walls = [bay + Vec2::new(-6.0, 0.0), bay + Vec2::new(6.0, 0.0), bay + Vec2::new(0.0, -10.0)];
    for at in walls {
        m.building(0, barracks, at, true);
    }
    m.tick();
    let (barracks_size, house_size) = (m.data.building(barracks).size, m.data.building(house).size);
    {
        let mut nav = m.app.world_mut().resource_mut::<Nav>();
        nav.0.set_walk_margin(WALK_MARGIN);
        for at in walls {
            nav.0.add_obstacle(&Obb::from_size(at, barracks_size, 0.0), 1);
        }
    }
    // Right across the mouth (the barracks end at y = 64), 1.6 m off.
    let across = bay + Vec2::new(0.0, 7.6);
    let house_box = move |at: Vec2| Obb::from_size(at, house_size, 0.0);
    let (spot, ram) = {
        let snap = m.snapshot(0);
        let view = m.view(0, &snap);
        let movers = ground_movers(&view);
        let ram = *movers.iter().find(|mv| mv.clearance == 1).expect("ironhold fields movers of clearance 1");
        assert!(movers.iter().any(|mv| mv.clearance == 0), "and workers");
        assert!(clear_around(&view, &house_box(across), 1.5, None, &[]), "the gap alone allows the spot");
        assert!(seals_pocket(&view, &movers, &[house_box(across)], &[]), "but it closes the bay");
        assert!(!seals_pocket(&view, &[Mover { clearance: 0, ..ram }], &[house_box(across)], &[]), "on wide movers only");
        let q = SpotQuery { kind: house, anchor: across, toward: Vec2::Y, min_r: 0.0, max_r: 12.0, gap: 1.5 };
        (find_build_spot(&view, &q, &[], &[]).expect("a spot further out"), ram)
    };
    let mut nav = m.app.world_mut().resource_mut::<Nav>();
    let nav = &mut nav.0;
    nav.add_obstacle(&house_box(spot.pos), 1);
    let way_out = |nav: &mut NavGrid, radius: f32| {
        let req = PathRequest {
            start: bay,
            goal: bay + Vec2::new(0.0, 40.0),
            mobility: ram.mobility,
            radius,
            team: NO_TEAM,
            max_expansions: 200_000,
            reach: None,
        };
        nav.find_path(&req).complete
    };
    assert_eq!(nav.clearance_cells(0.9), 1);
    assert!(way_out(nav, 0.9), "a ram in the bay still gets out (house at {})", spot.pos);
    assert!(spot.pos.distance(across) > 1.0, "not across the mouth: {}", spot.pos);
    // Where the gap alone would have put it, the ram is shut in.
    nav.add_obstacle(&house_box(spot.pos), -1);
    nav.add_obstacle(&house_box(across), 1);
    assert!(!way_out(nav, 0.9), "the house across the mouth shuts it in");
    assert!(way_out(nav, 0.3), "while a worker slips out");
}

#[test]
fn drop_site_for_a_far_cluster() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let home = m.start(0);
    let far = home + Vec2::new(-35.0, 4.0);
    for k in 0..6 {
        m.node("oak", far + Vec2::new((k % 3) as f32 * 2.0, (k / 3) as f32 * 2.0));
    }
    m.tick();
    let wood = m.data.resource_id("wood").unwrap();
    let mut desired = [0; MAX_RESOURCES];
    desired[wood.index()] = 4;
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let sources = Sources::home(&view, home, |_| false);
    let plan = drop_site_plan(&view, home, &sources, &desired, &[], |_| false).expect("a lumber site for trees 35 m out");
    assert_eq!(Some(plan.kind), view.data.building_id("lumber_camp"), "the cheapest wood drop site");
    assert_eq!(plan.resource, wood);
    assert!(plan.anchor.distance(far + Vec2::new(2.0, 1.0)) < 2.0, "anchored on the cluster: {}", plan.anchor);
    // Not when nobody needs wood.
    assert_eq!(drop_site_plan(&view, home, &sources, &[0; MAX_RESOURCES], &[], |_| false), None);
    // Not when one is already planned there.
    let lumber = plan.kind;
    let pending = [PendingBuild {
        kind: lumber,
        pos: plan.anchor + Vec2::new(4.0, 0.0),
        obb: Obb::from_size(plan.anchor, view.data.building(lumber).size, 0.0),
        time: 0.0,
    }];
    assert_eq!(drop_site_plan(&view, home, &sources, &desired, &pending, |_| false), None);
}

#[test]
fn no_drop_site_for_a_near_cluster() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let home = m.start(0);
    for k in 0..6 {
        m.node("oak", home + Vec2::new(10.0 + (k % 3) as f32 * 2.0, (k / 3) as f32 * 2.0));
    }
    m.tick();
    let mut desired = [0; MAX_RESOURCES];
    desired[m.data.resource_id("wood").unwrap().index()] = 4;
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let sources = Sources::home(&view, home, |_| false);
    assert_eq!(drop_site_plan(&view, home, &sources, &desired, &[], |_| false), None, "the hall is close enough");
}

#[test]
fn drop_site_follows_the_gatherers() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let home = m.start(0);
    // A grove by the hall (served by it) and a forest 35 m out.
    let near: Vec<Entity> = (0..6).map(|k| m.node("oak", home + Vec2::new(8.0 + (k % 3) as f32 * 2.0, 8.0 + (k / 3) as f32 * 2.0))).collect();
    let far_at = home + Vec2::new(-35.0, 4.0);
    let far: Vec<Entity> = (0..6).map(|k| m.node("oak", far_at + Vec2::new((k % 3) as f32 * 2.0, (k / 3) as f32 * 2.0))).collect();
    m.tick();
    let mut desired = [0; MAX_RESOURCES];
    desired[m.data.resource_id("wood").unwrap().index()] = 4;
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let sources = Sources::home(&view, home, |_| false);
    let plan = drop_site_plan(&view, home, &sources, &desired, &[], |e| far.contains(&e)).expect("a site for the worked far forest");
    assert!(plan.anchor.distance(far_at + Vec2::new(2.0, 1.0)) < 2.0, "anchored on the worked cluster: {}", plan.anchor);
    assert_eq!(drop_site_plan(&view, home, &sources, &desired, &[], |e| near.contains(&e)), None, "the grove is served");
    assert_eq!(drop_site_plan(&view, home, &sources, &desired, &[], |_| false), None, "nobody works the far forest yet");
}

#[test]
fn no_drop_site_for_a_cluster_nearly_mined_out() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let home = m.start(0);
    let far = home + Vec2::new(-35.0, 4.0);
    for k in 0..6 {
        m.node_holding("oak", far + Vec2::new((k % 3) as f32 * 2.0, (k / 3) as f32 * 2.0), Some(20.0));
    }
    m.tick();
    let mut desired = [0; MAX_RESOURCES];
    desired[m.data.resource_id("wood").unwrap().index()] = 4;
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let sources = Sources::home(&view, home, |_| false);
    assert!(sources.nodes.iter().all(|n| n.remaining == 20.0), "what is left is known");
    assert_eq!(drop_site_plan(&view, home, &sources, &desired, &[], |_| true), None, "120 wood left is not worth a camp");
}

#[test]
fn claims_the_next_vent_when_one_fails() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let home = m.start(0);
    let data = m.data.clone();
    let to_4 = data.age_up_tech(m.player(0).faction, 3).unwrap();
    for b in data.tech(to_4).requires_building_ids.clone() {
        m.building(0, b, home + Vec2::new(0.0, -25.0), true);
    }
    m.workers(0, 10, home);
    let (near, far) = (home + Vec2::new(10.0, 30.0), home + Vec2::new(10.0, 50.0));
    m.node("aether_vent", near);
    m.node("aether_vent", far);
    let aether = data.resource_id("aether").unwrap();
    let rich = |p: &mut PlayerState| {
        p.resources = ResAmounts([5000.0; MAX_RESOURCES]);
        p.resources.set(aether, 0.0);
    };
    m.edit_player(0, |p| {
        p.age = 3;
        rich(p);
    });
    m.tick();
    let extractor = m.building_id("aether_extractor");
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(placed_at(&cmds, extractor).is_some_and(|p| p.distance(near) < 0.5), "the nearest vent first: {cmds:?}");

    // Something is in the way at the nearest vent: claim the next one, and remember the failure.
    let house = m.building_id("house");
    let obb = Obb::from_size(near, data.building(house).size, 0.0);
    let mut mem = AiMemory { pending: vec![PendingBuild { kind: house, pos: near, obb, time: 0.0 }], ..default() };
    let cmds = m.think(0, &mut mem);
    assert!(placed_at(&cmds, extractor).is_some_and(|p| p.distance(far) < 0.5), "the next vent: {cmds:?}");
    assert!(mem.bad_spots.iter().any(|s| s.0 == extractor && s.1.distance(near) < 0.5));
    // A vent that failed lately is skipped outright.
    let mut mem = AiMemory { bad_spots: vec![(extractor, near, 60.0)], ..default() };
    let cmds = m.think(0, &mut mem);
    assert!(placed_at(&cmds, extractor).is_some_and(|p| p.distance(far) < 0.5), "{cmds:?}");
    // With aether piling up already, no more vents are claimed.
    m.edit_player(0, |p| p.resources.set(aether, 2000.0));
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(placed_at(&cmds, extractor).is_none(), "aether is piling up: {cmds:?}");
}

#[test]
fn stuck_foundations_are_deleted() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let home = m.start(0);
    m.workers(0, 8, home);
    let house = m.building_id("house");
    let stuck = m.building(0, house, home + Vec2::new(20.0, -15.0), false);
    let growing = m.building(0, house, home + Vec2::new(20.0, 15.0), false);
    m.tick();
    let mut mem = AiMemory::default();
    let deletes = |cmds: &[PlayerCommand]| -> Vec<Entity> {
        cmds.iter().filter(|c| c.kind == CommandKind::Delete).flat_map(|c| c.entities.clone()).collect()
    };
    let first = m.think(0, &mut mem);
    assert!(first.iter().any(|c| c.kind == CommandKind::Build(stuck)), "a builder is sent: {first:?}");
    for t in [1.0, 40.0] {
        m.set_time(t);
        if t == 40.0 {
            m.app.world_mut().get_mut::<UnderConstruction>(growing).unwrap().progress = 0.3;
        }
        let cmds = m.think(0, &mut mem);
        assert!(deletes(&cmds).is_empty(), "too early at {t} s: {cmds:?}");
    }
    m.set_time(62.0);
    let cmds = m.think(0, &mut mem);
    assert_eq!(deletes(&cmds), vec![stuck], "a minute without progress: {cmds:?}");
    assert!(mem.bad_spots.iter().any(|s| s.0 == house && s.1.distance(home + Vec2::new(20.0, -15.0)) < 0.5), "the spot is avoided");
}

#[test]
fn workers_are_trained_out_of_the_saving_up_to_the_target() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let data = m.data.clone();
    let worker = data.faction(m.player(0).faction).worker_id;
    let food = data.resource_id("food").unwrap();
    let price = stats::unit_cost(&data, m.player(0), worker).get(food);
    // Less food than the age-up needs: all of it is saving, and a worker
    // is bought out of it anyway.
    m.edit_player(0, |p| p.resources = amounts(&data, &[("food", 2.0 * price)]));
    let house = m.building_id("house");
    for k in 0..3 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -10.0 + k as f32 * 6.0), true);
    }
    m.workers(0, worker_target(1) as usize - 1, m.start(0));
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert_eq!(trains(&cmds, worker).len(), 1, "one short of the target: one more: {cmds:?}");
    m.workers(0, 1, m.start(0) + Vec2::new(0.0, -3.0));
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(trains(&cmds, worker).is_empty(), "at the target: no more: {cmds:?}");
}

#[test]
fn a_failed_gather_target_is_counted_once() {
    let mut m = Match::new(&["sylvan", "tide"]);
    m.hall(0);
    let berry = m.node("berry_bush", m.start(0) + Vec2::new(-12.0, 0.0));
    let w = m.workers(0, 1, m.start(0))[0];
    // Broke, so no placement claims the worker as a builder in between.
    m.edit_player(0, |p| p.resources = ResAmounts::ZERO);
    m.tick();
    let mut mem = AiMemory::default();
    mem.worker_orders.insert(w, WorkerOrder { target: berry, time: 5.0 });
    for t in [10.0, 15.0] {
        m.set_time(t);
        m.think(0, &mut mem);
    }
    assert_eq!(mem.bad_targets.iter().filter(|b| b.0 == berry).count(), 1, "{:?}", mem.bad_targets);
}

#[test]
fn farms_when_food_nodes_run_out() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    m.workers(0, 8, m.start(0));
    m.tick();
    // No berries anywhere: food has to come from fields.
    let cmds = m.think(0, &mut AiMemory::default());
    let farm = m.building_id("farm");
    assert_eq!(placements(&cmds, farm), 1, "a farm is planned: {cmds:?}");
}

// ---------------------------------------------------------------------------
// Ages
// ---------------------------------------------------------------------------

#[test]
fn age_up_respects_affordability_and_required_buildings() {
    let mut m = Match::new(&["ironhold", "dune"]);
    let hall = m.hall(0);
    m.tick();
    let data = m.data.clone();
    let faction = m.player(0).faction;
    let step = |m: &mut Match| {
        let snap = m.snapshot(0);
        let view = m.view(0, &snap);
        age_up_step(&view, &[])
    };

    // Age 1: only resources matter.
    let to_2 = data.age_up_tech(faction, 1).unwrap();
    m.edit_player(0, |p| p.resources = ResAmounts::ZERO);
    assert_eq!(step(&mut m), AgeUpStep::Save { cost: data.tech(to_2).cost.amounts });
    m.edit_player(0, |p| p.resources = data.tech(to_2).cost.amounts);
    assert_eq!(step(&mut m), AgeUpStep::Research { tech: to_2, building: hall });
    m.edit_player(0, |p| p.researching[to_2.index()] = true);
    assert_eq!(step(&mut m), AgeUpStep::InProgress);

    // Age 2: the required building comes first, even when rich.
    let to_3 = data.age_up_tech(faction, 2).unwrap();
    let required = data.tech(to_3).requires_building_ids[0];
    m.edit_player(0, |p| {
        p.age = 2;
        p.researched[to_2.index()] = true;
        p.researching[to_2.index()] = false;
        p.resources = ResAmounts([5000.0; MAX_RESOURCES]);
    });
    assert_eq!(step(&mut m), AgeUpStep::Build(required));
    let spot = m.start(0) + Vec2::new(-20.0, 0.0);
    let site = m.building(0, required, spot, false);
    m.tick();
    assert_eq!(step(&mut m), AgeUpStep::Wait, "foundation under way");
    m.app.world_mut().entity_mut(site).despawn();
    m.building(0, required, spot, true);
    m.tick();
    m.edit_player(0, |p| p.resources = ResAmounts::ZERO);
    assert_eq!(step(&mut m), AgeUpStep::Save { cost: data.tech(to_3).cost.amounts });
    m.edit_player(0, |p| p.resources = data.tech(to_3).cost.amounts);
    assert_eq!(step(&mut m), AgeUpStep::Research { tech: to_3, building: hall });

    // Age 3: the next age-up is researched in the hall's upgrade.
    let to_4 = data.age_up_tech(faction, 3).unwrap();
    for b in data.tech(to_4).requires_building_ids.clone() {
        m.building(0, b, m.start(0) + Vec2::new(0.0, -25.0), true);
    }
    m.edit_player(0, |p| {
        p.age = 3;
        p.researched[to_3.index()] = true;
        p.resources = ResAmounts::ZERO;
    });
    m.tick();
    let upgrade = data.building(data.building_id("town_center").unwrap()).upgrade.clone().unwrap();
    assert_eq!(data.tech(to_4).researched_at_id, upgrade.to_id);
    assert_eq!(step(&mut m), AgeUpStep::Save { cost: upgrade.cost.amounts });
    m.edit_player(0, |p| p.resources = upgrade.cost.amounts);
    assert_eq!(step(&mut m), AgeUpStep::Upgrade { building: hall });
}

#[test]
fn age_up_follows_faction_specific_requirements() {
    // The Sylvan Engine-age tech needs their own grove, not the common foundry.
    let mut m = Match::new(&["sylvan", "dune"]);
    m.hall(0);
    let data = m.data.clone();
    let faction = m.player(0).faction;
    let tech = data.age_up_tech(faction, 3).unwrap();
    m.edit_player(0, |p| {
        p.age = 3;
        p.resources = ResAmounts([5000.0; MAX_RESOURCES]);
    });
    m.tick();
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    assert_eq!(age_up_step(&view, &[]), AgeUpStep::Build(data.tech(tech).requires_building_ids[0]));
}

#[test]
fn age_up_is_researched_when_affordable() {
    let mut m = Match::new(&["dune", "tide"]);
    let hall = m.hall(0);
    m.workers(0, 6, m.start(0));
    m.tick();
    let data = m.data.clone();
    let tech = data.age_up_tech(m.player(0).faction, 1).unwrap();
    let mut rich = data.tech(tech).cost.amounts;
    rich.add_all(&amounts(&data, &[("wood", 100.0)]));
    m.edit_player(0, |p| p.resources = rich);
    let cmds = m.think(0, &mut AiMemory::default());
    let research: Vec<_> = cmds.iter().filter(|c| c.kind == CommandKind::Research { tech }).collect();
    assert_eq!(research.len(), 1, "{cmds:?}");
    assert_eq!(research[0].entities, vec![hall]);
    // The age-up is paid first: no worker is queued with the food it needs.
    let worker = data.faction(m.player(0).faction).worker_id;
    assert!(trains(&cmds, worker).is_empty(), "{cmds:?}");

    // Rich enough for both: the hall is busy with the age-up from this think
    // on, so no worker is queued behind it.
    m.edit_player(0, |p| p.resources = ResAmounts([5000.0; MAX_RESOURCES]));
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(cmds.iter().any(|c| c.kind == CommandKind::Research { tech } && c.entities == vec![hall]), "{cmds:?}");
    assert!(!cmds.iter().any(|c| matches!(c.kind, CommandKind::Train { .. }) && c.entities.contains(&hall)), "{cmds:?}");
}

#[test]
fn age_up_upgrades_the_hall_while_a_required_building_goes_up() {
    let mut m = Match::new(&["ironhold", "dune"]);
    let hall = m.hall(0);
    let data = m.data.clone();
    let to_4 = data.age_up_tech(m.player(0).faction, 3).unwrap();
    let required = data.tech(to_4).requires_building_ids[0];
    m.building(0, required, m.start(0) + Vec2::new(0.0, -25.0), false);
    let upgrade = data.building(m.hall_kind(0)).upgrade.clone().unwrap();
    m.edit_player(0, |p| {
        p.age = 3;
        p.resources = upgrade.cost.amounts;
    });
    m.tick();
    assert_eq!(m.age_up_step(0), AgeUpStep::Upgrade { building: hall }, "the 90 s upgrade need not wait for the foundation");
}

#[test]
fn age_up_follows_prerequisite_chains() {
    // Content where the Starfall age-up needs heavy_plow, which needs horse_collar (both at the mill).
    let mut data = (*test_data()).clone();
    let tech = |d: &GameData, id: &str| d.tech_id(id).unwrap_or_else(|| panic!("no tech {id}"));
    let (to_3, plow, collar) = (tech(&data, "to_starfall"), tech(&data, "heavy_plow"), tech(&data, "horse_collar"));
    data.techs[to_3.index()].requires_ids.push(plow);
    let mut m = Match::with_data(Arc::new(data), &["ironhold", "dune"]);
    let data = m.data.clone();
    let mill = m.building_id("mill");
    assert_eq!(data.tech(collar).researched_at_id, mill);
    m.hall(0);
    for b in data.tech(to_3).requires_building_ids.clone() {
        m.building(0, b, m.start(0) + Vec2::new(0.0, -25.0), true);
    }
    m.edit_player(0, |p| {
        p.age = 2;
        p.resources = ResAmounts([5000.0; MAX_RESOURCES]);
    });
    m.tick();
    assert_eq!(m.age_up_step(0), AgeUpStep::Build(mill), "the prerequisite's prerequisite needs a mill");
    let snap = m.snapshot(0);
    let mut expected = data.tech(to_3).cost.amounts;
    for t in [plow, collar] {
        expected.add_all(&data.tech(t).cost.amounts);
    }
    expected.add_all(&stats::building_cost(&data, m.player(0), mill));
    assert_eq!(progress::age_path_cost(&m.view(0, &snap)), expected, "the whole chain is saved for");

    let at = m.building(0, mill, m.start(0) + Vec2::new(25.0, 0.0), true);
    m.tick();
    assert_eq!(m.age_up_step(0), AgeUpStep::Research { tech: collar, building: at });
    m.edit_player(0, |p| p.researched[collar.index()] = true);
    assert_eq!(m.age_up_step(0), AgeUpStep::Research { tech: plow, building: at });
    m.edit_player(0, |p| p.researching[plow.index()] = true);
    assert_eq!(m.age_up_step(0), AgeUpStep::Wait);
}

#[test]
fn upgrade_only_buildings_are_never_placed() {
    let mut m = Match::new(&["ironhold", "dune"]);
    let data = m.data.clone();
    let to_4 = data.age_up_tech(m.player(0).faction, 3).unwrap();
    for b in data.tech(to_4).requires_building_ids.clone() {
        m.building(0, b, m.start(0) + Vec2::new(0.0, -25.0), true);
    }
    m.edit_player(0, |p| {
        p.age = 3;
        p.resources = ResAmounts([5000.0; MAX_RESOURCES]);
    });
    // The age-up is researched in an upgraded hall: with no hall at all, build a hall first ...
    let hall = m.hall_kind(0);
    m.tick();
    assert_eq!(m.age_up_step(0), AgeUpStep::Build(hall));
    // ... and with only a foundation, wait for it rather than place the upgrade directly.
    m.building(0, hall, m.start(0), false);
    m.tick();
    assert_eq!(m.age_up_step(0), AgeUpStep::Wait);
}

// ---------------------------------------------------------------------------
// Army
// ---------------------------------------------------------------------------

#[test]
fn army_composition_only_uses_trainable_units_for_every_faction() {
    let data = test_data();
    for f in data.faction_ids() {
        let fdef = data.faction(f);
        let mut p = PlayerState::new(PlayerId(0), "p".into(), f, [1.0; 3], 0, Controller::Ai, &data);
        for age in 1..=MAX_AGE {
            p.age = age;
            let comp = army_composition(&data, &p);
            assert!(!comp.is_empty(), "{}: no army in age {age}", fdef.id);
            for (u, w) in &comp {
                let d = data.unit(*u);
                assert!(*w > 0.0);
                assert!(d.gatherer.is_none(), "{}: worker {} in the army", fdef.id, d.id);
                assert!(d.age <= age, "{}: {} is from age {}", fdef.id, d.id, d.age);
                assert!(
                    fdef.building_ids.iter().any(|b| rules::can_train(&data, &p, *b, *u, false).is_ok()),
                    "{}: {} cannot be trained in age {age}",
                    fdef.id,
                    d.id
                );
            }
        }
    }
}

#[test]
fn army_composition_follows_line_upgrades() {
    let data = test_data();
    let f = data.faction_id("ironhold").unwrap();
    let mut p = PlayerState::new(PlayerId(0), "p".into(), f, [1.0; 3], 0, Controller::Ai, &data);
    p.age = 2;
    let (spear, pike) = (data.unit_id("spearman").unwrap(), data.unit_id("pikeman").unwrap());
    assert!(army_composition(&data, &p).iter().any(|(u, _)| *u == spear));
    p.mods.unit_upgrades.push((spear, pike));
    let comp = army_composition(&data, &p);
    assert!(comp.iter().any(|(u, _)| *u == pike), "the upgraded line is trained: {comp:?}");
    assert!(!comp.iter().any(|(u, _)| *u == spear), "the superseded unit is not");
}

#[test]
fn attack_threshold_logic() {
    assert_eq!(attack_threshold(1, 0.5), 17);
    assert_eq!(attack_threshold(3, 0.5), 32);
    assert!(attack_threshold(2, 0.9) < attack_threshold(2, 0.5), "rushers attack smaller");
    assert!(attack_threshold(2, 0.1) > attack_threshold(2, 0.5), "turtles attack bigger");
    assert!(attack_threshold(4, 0.5) > attack_threshold(2, 0.5), "later ages need more");
    // The first wave is due between six minutes (all-out rush) and ten (turtle).
    assert_eq!(min_attack_time(1.0), MIN_ATTACK_TIME);
    assert!(min_attack_time(0.2) > min_attack_time(0.8) && min_attack_time(0.8) > MIN_ATTACK_TIME);
    assert!(min_attack_time(0.0) <= 600.0, "even a turtle is due by ten minutes");
    let big = attack_threshold(1, 0.5) + 10;
    let earliest = min_attack_time(0.5);
    assert!(!should_attack(earliest - 1.0, big, 1, 0.5, None), "never before the first-attack time");
    assert!(should_attack(earliest + 1.0, big, 1, 0.5, None));
    assert!(!should_attack(earliest + 1.0, attack_threshold(1, 0.5) - 1, 1, 0.5, None), "army too small");
}

/// A due wave waits for the full threshold at first, then for less and
/// less, down to a floor: the first soon, later ones over a while at a
/// higher share first; the next one is due an interval after it left.
#[test]
fn waves_come_regularly_and_shrink_when_late() {
    for aggression in [0.0, 0.3, 0.6, 1.0] {
        let due = wave_due(aggression, None);
        assert_eq!(due, min_attack_time(aggression));
        let full = attack_threshold(2, aggression);
        assert_eq!(wave_size(due - 1.0, due, 2, aggression, false), u32::MAX, "not due yet");
        assert_eq!(wave_size(due, due, 2, aggression, false), full);
        let mut last = full;
        for step in 1..=10 {
            let size = wave_size(due + step as f32 * WAVE_PRESSURE_TIME / 5.0, due, 2, aggression, false);
            assert!(size <= last, "never grows while waiting");
            last = size;
        }
        let floor = ((full as f32 * WAVE_FLOOR_SHARE).round() as u32).max(MIN_WAVE_POP);
        let later_floor = ((full as f32 * LATER_WAVE_FLOOR_SHARE).round() as u32).max(MIN_WAVE_POP);
        assert_eq!(last, floor, "aggression {aggression}: down to the floor");
        assert_eq!(wave_size(due + WAVE_PRESSURE_TIME, due, 2, aggression, false), later_floor, "the later waves' share first");
        assert!(later_floor > floor && later_floor < full);
        // The first wave shrinks to the floor faster: even a turtle with only
        // the floor launches by eleven minutes.
        assert_eq!(wave_size(due + FIRST_WAVE_PRESSURE_TIME, due, 2, aggression, true), floor);
        assert!(wave_size(due + FIRST_WAVE_PRESSURE_TIME, due, 2, aggression, false) > floor);
        assert!(due + FIRST_WAVE_PRESSURE_TIME <= 660.0, "aggression {aggression}: floor reached at {}", due + FIRST_WAVE_PRESSURE_TIME);
    }
    assert_eq!(wave_due(0.5, Some(100.0)), min_attack_time(0.5), "not before the first-attack time");
    assert_eq!(wave_due(0.5, Some(900.0)), 900.0 + WAVE_INTERVAL);
    let later = wave_size(WAVE_PRESSURE_TIME, 0.0, 2, 0.5, false);
    assert!(should_attack(900.0 + WAVE_INTERVAL + WAVE_PRESSURE_TIME, later, 2, 0.5, Some(900.0)));
    assert!(!should_attack(900.0 + WAVE_INTERVAL + WAVE_PRESSURE_TIME, later - 1, 2, 0.5, Some(900.0)));
    assert!(!should_attack(900.0 + WAVE_INTERVAL - 1.0, 200, 2, 0.5, Some(900.0)), "the next wave is not due yet");
}

/// A gathered army too small for a full wave still launches once the wave
/// has been due long enough, and the next wave waits its interval.
#[test]
fn a_late_wave_launches_smaller() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    m.hall(1);
    m.edit_player(0, |p| p.age = 2);
    m.tick();
    let rally = m.rally(0);
    let aggression = m.data.faction(m.player(0).faction).ai.aggression;
    let floor = wave_size(1e6, 0.0, 2, aggression, true);
    let army = m.army(0, "spearman", rally + Vec2::new(-6.0, -3.0), floor as usize);
    m.tick();
    let due = min_attack_time(aggression);
    let mut mem = AiMemory::default();
    m.set_time(due + 1.0);
    let cmds = m.think(0, &mut mem);
    assert_eq!(mem.attack, AttackState::Gathering, "a full wave is wanted at first: {cmds:?}");
    m.set_time(due + FIRST_WAVE_PRESSURE_TIME + 1.0);
    let cmds = m.think(0, &mut mem);
    assert_eq!(attack_moved_to(&cmds, m.start(1)), Some(sorted(&army)), "late: the smaller army goes: {cmds:?}");
    assert_eq!(mem.last_wave, Some(due + FIRST_WAVE_PRESSURE_TIME + 1.0));
}

/// Raided with next to no army, a realm trains out of its whole age-up
/// saving while the age-up keeps to its schedule, and behind it only
/// against a raid in earnest (a scout's poke takes nothing); with a quarter
/// of its army target, defence borrows from the saving but never all of it.
#[test]
fn defence_spends_the_whole_saving_only_with_next_to_no_army() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let data = m.data.clone();
    let barracks = m.building_id("barracks");
    m.building(0, barracks, m.start(0) + Vec2::new(-20.0, 15.0), true);
    m.workers(0, worker_target(2) as usize, m.start(0));
    let house = m.building_id("house");
    for k in 0..8 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -20.0 + k as f32 * 6.0), true);
    }
    let forge = m.building_id("forge");
    m.building(0, forge, m.start(0) + Vec2::new(-25.0, -15.0), true);
    let to_3 = data.age_up_tech(m.player(0).faction, 2).unwrap();
    let saving = data.tech(to_3).cost.amounts;
    m.edit_player(0, |p| p.age = 2);
    m.tick();
    let trained =
        |cmds: &[PlayerCommand]| cmds.iter().any(|c| matches!(c.kind, CommandKind::Train { unit, .. } if data.unit(unit).gatherer.is_none()));
    let raided = |m: &Match| AiMemory {
        alerts: vec![Alert { at: m.start(0), attacker: m.start(0) + Vec2::new(5.0, 0.0), attacker_owner: Some(m.p(1)), time: 0.0 }],
        ..default()
    };
    // The age-up waits for aether, and with no income measured all of its
    // food is saving; wood is free.
    let (food, wood, aether) = (data.resource_id("food").unwrap(), data.resource_id("wood").unwrap(), data.resource_id("aether").unwrap());
    let bank = |food_share: f32| {
        let mut bank = saving;
        bank.set(aether, 0.0);
        bank.set(food, saving.get(food) * food_share);
        bank.set(wood, 100.0);
        bank
    };
    m.edit_player(0, |p| p.resources = bank(1.0));
    m.tick();
    let cmds = m.think(0, &mut raided(&m));
    assert!(trained(&cmds), "under attack with no army: train from the saving: {cmds:?}");
    // With little more than the floor of the saving in the bank, a realm
    // with no army still trains ...
    m.edit_player(0, |p| p.resources = bank(0.27));
    m.tick();
    let cmds = m.think(0, &mut raided(&m));
    assert!(trained(&cmds), "no army: the whole saving goes: {cmds:?}");
    // Once the age-up is behind schedule a scout's poke takes no saving, but
    // a raid in earnest still does: the town itself is at stake.
    let late = |m: &Match| {
        let mut mem = raided(m);
        mem.age_seen = Some((2, -AGE_SCHEDULE[1] - 1.0));
        mem
    };
    m.army(1, "scout_rider", m.start(0) + Vec2::new(5.0, 0.0), 1);
    m.tick();
    let cmds = m.think(0, &mut late(&m));
    assert!(!trained(&cmds), "a poke behind schedule: the saving stays: {cmds:?}");
    m.army(1, "spearman", m.start(0) + Vec2::new(6.0, 3.0), 8);
    m.tick();
    let cmds = m.think(0, &mut late(&m));
    assert!(trained(&cmds), "a raid in earnest: the whole saving goes: {cmds:?}");
    // ... but with a quarter of its army target, defence stops short of it.
    m.army(0, "spearman", m.start(0) + Vec2::new(-12.0, 20.0), (army_target(2, worker_target(2)) / 4) as usize + 1);
    m.tick();
    let cmds = m.think(0, &mut raided(&m));
    assert!(!trained(&cmds), "the floor of the saving stays: {cmds:?}");
}

// ---------------------------------------------------------------------------
// Finishing games: hunting a player whose buildings are not known
// ---------------------------------------------------------------------------

/// A fog in which team 0 has explored (and sees) only the disc of `radius`
/// round `around`.
fn fog_explored_round(m: &mut Match, around: Vec2, radius: f32) {
    let mut fog = af_core::ext::fog::fog_for_map(Vec2::splat(MAP), 2.0, 3, true);
    for y in 0..fog.height {
        for x in 0..fog.width {
            let centre = (Vec2::new(x as f32, y as f32) + 0.5) * fog.cell;
            if centre.distance(around) <= radius {
                let i = (y * fog.width + x) as usize;
                fog.teams[0].visible[i] = 1;
                fog.teams[0].explored[i] = true;
            }
        }
    }
    m.app.insert_resource(fog);
}

/// A fog in which team 0 sees (and has explored) only the cells whose
/// centres lie in one of the discs (centre, radius).
fn fog_lit(m: &mut Match, discs: &[(Vec2, f32)]) {
    let mut fog = af_core::ext::fog::fog_for_map(Vec2::splat(MAP), 2.0, 3, true);
    for y in 0..fog.height {
        for x in 0..fog.width {
            let centre = (Vec2::new(x as f32, y as f32) + 0.5) * fog.cell;
            if discs.iter().any(|(at, r)| centre.distance(*at) <= *r) {
                let i = (y * fog.width + x) as usize;
                fog.teams[0].visible[i] = 1;
                fog.teams[0].explored[i] = true;
            }
        }
    }
    m.app.insert_resource(fog);
}

/// With nothing of a player known and its start found empty, the hunt
/// searches unseen ground near that start, then moves on to the next spot;
/// with the fog off nothing can hide, so nothing is searched.
#[test]
fn a_hunt_searches_unseen_ground_near_the_enemy_start() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let home = m.start(0);
    m.tick();
    let mut mem = AiMemory::default();
    mem.visited.push(m.p(1));
    {
        let snap = m.snapshot(0);
        let view = m.view(0, &snap);
        assert_eq!(choose_target(&view, &mem, home), None, "fog off: nothing to search for");
    }
    fog_explored_round(&mut m, home, 40.0);
    // Built (out of sight) before looking, so the view can be held.
    let b = m.known(1, m.start(1) + Vec2::new(-60.0, 10.0));
    m.tick();
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let goal = choose_target(&view, &mem, home).expect("a spot to search");
    assert_eq!((goal.player, goal.aim), (m.p(1), Aim::Search));
    assert!(goal.pos.distance(home) > 40.0, "not where we already see");
    assert!(goal.pos.distance(m.start(1)) < SEARCH_STEP * 1.5, "next to the enemy start: {} vs {}", goal.pos, m.start(1));
    mem.note_searched(goal.pos, 0.0);
    let next = choose_target(&view, &mem, goal.pos).expect("another spot");
    assert_eq!(next.aim, Aim::Search);
    assert!(next.pos.distance(goal.pos) >= SEARCH_STEP * 0.9, "a new spot: {} after {}", next.pos, goal.pos);
    // A known building always beats a blind search.
    mem.see(b);
    assert_eq!(choose_target(&view, &mem, home).map(|g| g.aim), Some(Aim::Building(b.entity)));
}

/// A wave that reaches the spot it searched marks it and moves on; one
/// hunting blind turns on a building of its prey as soon as one is known.
#[test]
fn a_hunting_wave_moves_on_and_turns_on_what_it_finds() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let home = m.start(0);
    fog_explored_round(&mut m, home, 40.0);
    let spot = m.start(1) + Vec2::new(10.0, 10.0);
    let group = m.army(0, "steppe_outrider", spot + Vec2::new(-3.0, 0.0), 6);
    m.tick();
    m.set_time(900.0);
    let goal = Objective { player: m.p(1), aim: Aim::Search, pos: spot };
    let mut mem = attacking(&group, goal, 850.0);
    mem.wave_pop = 12;
    mem.visited.push(m.p(1));
    let cmds = m.think(0, &mut mem);
    assert!(mem.is_searched(spot), "arrived: the spot is searched");
    let AttackState::Attacking { goal: next, .. } = mem.attack else { panic!("still hunting: {cmds:?}") };
    assert_eq!(next.aim, Aim::Search);
    assert!(next.pos.distance(spot) > SEARCH_STEP * 0.5, "on to the next spot");
    assert!(attack_moved_to(&cmds, next.pos).is_some(), "{cmds:?}");

    // Far from the spot, a building of the prey comes to light.
    let far = m.start(0) + Vec2::new(0.0, -30.0);
    let mut mem = attacking(&group, Objective { player: m.p(1), aim: Aim::Search, pos: far }, 850.0);
    mem.wave_pop = 12;
    let house = m.known(1, m.start(1) + Vec2::new(-30.0, 0.0));
    mem.see(house);
    let cmds = m.think(0, &mut mem);
    assert!(matches!(mem.attack, AttackState::Attacking { goal, .. } if goal.aim == Aim::Building(house.entity)), "{cmds:?}");
}

/// Buildings every team sees (a revealed player's, through `SeenBy`) are
/// remembered as targets, wherever they stand.
#[test]
fn revealed_buildings_become_known_targets() {
    use af_core::ext::fog::{SeenBy, team_bit};
    let mut m = Match::new(&["steppe", "ironhold"]).with_ai_plugin();
    m.edit_player(0, |p| p.controller = Controller::Ai);
    m.hall(0);
    m.app.insert_resource(af_core::ext::fog::fog_for_map(Vec2::splat(MAP), 2.0, 2, true));
    let house = m.building_id("house");
    let hidden = m.building(1, house, m.start(1), true);
    let revealed = m.building(1, house, m.start(1) + Vec2::new(10.0, 0.0), true);
    // A farm in plain sight is not worth a wave: it keeps nobody in the game.
    let farm = m.building_id("farm");
    let field = m.building(1, farm, m.start(1) + Vec2::new(-12.0, 0.0), true);
    let all = team_bit(0) | team_bit(1);
    m.app.world_mut().entity_mut(hidden).insert(SeenBy { teams: team_bit(1), visible: team_bit(1) });
    m.app.world_mut().entity_mut(revealed).insert(SeenBy { teams: all, visible: all });
    m.app.world_mut().entity_mut(field).insert(SeenBy { teams: all, visible: all });
    for _ in 0..THINK_INTERVAL_TICKS + 1 {
        m.tick();
    }
    let mem = m.app.world().resource::<AiBrains>().memory(m.p(0)).expect("thought").clone();
    assert!(mem.knows(revealed), "seen by everyone: known");
    assert!(!mem.knows(hidden), "out of sight: not known");
    assert!(!mem.knows(field), "a field is no target");
}

/// A known building that falls while only its near side is in sight (a
/// siege engine that outranges its sight, say) is forgotten: the team saw
/// it go though its centre stayed dark. One that falls wholly out of sight
/// is still believed in, and one of a revealed player is seen to go
/// wherever it stood.
#[test]
fn a_building_seen_falling_is_forgotten_though_its_centre_is_dark() {
    use af_core::ext::fog::team_sees_box;
    let mut m = Match::new(&["steppe", "ironhold"]).with_ai_plugin();
    m.edit_player(0, |p| p.controller = Controller::Ai);
    m.hall(0);
    let barracks = m.building_id("barracks");
    let near_side = m.start(1);
    let unseen = m.start(1) + Vec2::new(0.0, 30.0);
    let a = m.building(1, barracks, near_side, true);
    let b = m.building(1, barracks, unseen, true);
    // Team 0 sees a strip along the west wall of `a` (8 m wide), not its centre.
    fog_lit(&mut m, &[(near_side + Vec2::new(-6.0, 0.0), 3.0)]);
    m.tick();
    let ka = KnownBuilding { entity: a, owner: m.p(1), kind: barracks, pos: near_side, facing: 0.0 };
    let kb = KnownBuilding { entity: b, owner: m.p(1), kind: barracks, pos: unseen, facing: 0.0 };
    {
        let fog = m.app.world().resource::<FogOfWar>();
        assert!(!fog.visible(0, near_side) && team_sees_box(fog, 0, &ka.obb(&m.data)), "near side lit, centre dark");
        assert!(!team_sees_box(fog, 0, &kb.obb(&m.data)), "wholly out of sight");
    }
    let p0 = m.p(0);
    {
        let mut brains = m.app.world_mut().resource_mut::<AiBrains>();
        let mem = brains.memory_mut(p0);
        mem.see(ka);
        mem.see(kb);
    }
    m.app.world_mut().despawn(a);
    m.app.world_mut().despawn(b);
    for _ in 0..THINK_INTERVAL_TICKS + 1 {
        m.tick();
    }
    let mem = m.app.world().resource::<AiBrains>().memory(p0).expect("thought").clone();
    assert!(!mem.knows(a), "seen gone: forgotten");
    assert!(mem.knows(b), "fell out of sight: still believed in");
    // Once its owner is revealed, everyone sees its buildings, and so sees them go.
    m.app.insert_resource(RevealedPlayers { list: vec![m.p(1)] });
    for _ in 0..THINK_INTERVAL_TICKS + 1 {
        m.tick();
    }
    let mem = m.app.world().resource::<AiBrains>().memory(p0).expect("thought").clone();
    assert!(!mem.knows(b), "a revealed realm's building seen gone: forgotten");
}

#[test]
fn army_target_grows_with_age() {
    for age in 1..=5u8 {
        let share = army_target(age, worker_target(age)) as f32 / (army_target(age, worker_target(age)) + worker_target(age)) as f32;
        assert!((share - ARMY_SHARE[age as usize - 1]).abs() < 0.02, "age {age}: army share {share}");
    }
    const { assert!(ARMY_SHARE[0] <= 0.25 && ARMY_SHARE[4] >= 0.5, "a quarter early, half or more late") };
}

/// An army at the rally point, big enough, after four minutes: attack-move
/// at the enemy's base.
#[test]
fn launches_an_attack_at_the_nearest_enemy() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    m.hall(0);
    m.hall(1);
    m.tick();
    let rally = {
        let snap = m.snapshot(0);
        let view = m.view(0, &snap);
        Plan::new(&view, &AiMemory::default()).rally
    };
    let rider = m.unit_id("steppe_outrider");
    let need = attack_threshold(1, m.data.faction(m.player(0).faction).ai.aggression);
    let pop = m.data.unit(rider).pop;
    let army: Vec<Entity> =
        (0..need.div_ceil(pop)).map(|k| m.unit(0, rider, rally + Vec2::new((k % 6) as f32 * 2.5, (k / 6) as f32 * 2.5))).collect();
    m.tick();

    let mut mem = AiMemory::default();
    m.set_time(100.0);
    let early = m.think(0, &mut mem);
    assert!(!early.iter().any(|c| matches!(c.kind, CommandKind::AttackMove(_)) && c.entities.len() == army.len()), "too early: {early:?}");
    assert_eq!(mem.attack, AttackState::Gathering);

    m.set_time(min_attack_time(m.data.faction(m.player(0).faction).ai.aggression) + 10.0);
    let cmds = m.think(0, &mut mem);
    let enemy = m.start(1);
    let attack = cmds
        .iter()
        .find(|c| matches!(c.kind, CommandKind::AttackMove(p) if p.distance(enemy) < 1.0))
        .unwrap_or_else(|| panic!("attack-move at the enemy start: {cmds:?}"));
    let mut sent = attack.entities.clone();
    sent.sort();
    let mut all = army.clone();
    all.sort();
    assert_eq!(sent, all, "the whole gathered army goes");
    assert!(matches!(mem.attack, AttackState::Attacking { goal, .. } if goal.player == m.p(1)));
}

/// Only the army gathered at the rally point counts toward (and goes on) an attack.
#[test]
fn launches_only_with_the_army_at_the_rally_point() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    m.hall(0);
    m.hall(1);
    m.tick();
    let rally = m.rally(0);
    let rider = m.unit_id("steppe_outrider");
    let need = attack_threshold(1, m.data.faction(m.player(0).faction).ai.aggression);
    let n = need.div_ceil(m.data.unit(rider).pop) as usize;
    let mut gathered = m.army(0, "steppe_outrider", rally + Vec2::new(-6.0, 0.0), n - 1);
    // Enough to pass the threshold, but still far from the rally point.
    let stragglers = m.army(0, "steppe_outrider", m.start(0) + Vec2::new(10.0, 50.0), 3);
    m.tick();
    let mut mem = AiMemory::default();
    let earliest = min_attack_time(m.data.faction(m.player(0).faction).ai.aggression);
    m.set_time(earliest + 10.0);
    let cmds = m.think(0, &mut mem);
    assert_eq!(mem.attack, AttackState::Gathering, "{cmds:?}");
    assert_eq!(attack_moved_to(&cmds, rally), Some(sorted(&stragglers)), "stragglers walk to the rally point");

    gathered.push(m.unit(0, rider, rally + Vec2::new(0.0, 3.0)));
    m.tick();
    m.set_time(earliest + 20.0);
    let cmds = m.think(0, &mut mem);
    assert_eq!(attack_moved_to(&cmds, m.start(1)), Some(sorted(&gathered)), "the gathered army goes, stragglers stay: {cmds:?}");
}

/// The nearest hostile player's nearest known building, else the nearest
/// unvisited start; goals given up as unreachable are skipped.
#[test]
fn attack_target_is_the_nearest_known_building_or_start() {
    let mut m = Match::new(&["steppe", "ironhold", "dune"]);
    let home = m.start(0);
    // Enemy A (player 1) starts nearer than enemy B (player 2) ...
    m.edit_player(1, |p| p.start_pos = Vec2::new(110.0, 140.0));
    // ... but B has buildings nearer still.
    let b_near = m.known(2, home + Vec2::new(-25.0, -15.0));
    let b_far = m.known(2, home + Vec2::new(-40.0, -40.0));
    m.tick();
    let mut mem = AiMemory::default();
    mem.see(b_far);
    mem.see(b_near);
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let goal = |mem: &AiMemory| choose_target(&view, mem, home).expect("a target");
    assert_eq!(goal(&mem), Objective { player: m.p(2), aim: Aim::Building(b_near.entity), pos: b_near.pos });
    mem.unreachable.push((b_near.pos, 1e9));
    assert_eq!(goal(&mem).building(), Some(b_far.entity), "the next building of that player");
    mem.unreachable.push((b_far.pos, 1e9));
    assert_eq!(goal(&mem), Objective { player: m.p(1), aim: Aim::Start, pos: Vec2::new(110.0, 140.0) }, "the nearer start");
    mem.visited.push(m.p(1));
    assert_eq!(goal(&mem).pos, m.start(2), "A was visited: B's start");
}

#[test]
fn retargets_when_the_goal_is_gone() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let home = m.start(0);
    let first = m.known(1, m.start(1));
    let second = m.known(1, m.start(1) + Vec2::new(0.0, -30.0));
    let group = m.army(0, "steppe_outrider", home + Vec2::new(-40.0, 0.0), 8);
    m.tick();
    m.set_time(300.0);
    let mut mem = attacking(&group, Objective { player: m.p(1), aim: Aim::Building(first.entity), pos: first.pos }, 295.0);
    mem.see(second);
    // `first` was destroyed (or seen gone): only `second` is still known.
    let cmds = m.think(0, &mut mem);
    assert_eq!(attack_moved_to(&cmds, second.pos), Some(sorted(&group)), "{cmds:?}");
    assert!(matches!(mem.attack, AttackState::Attacking { goal, .. } if goal.building() == Some(second.entity)));
}

#[test]
fn retargets_when_the_goal_owner_is_defeated() {
    let mut m = Match::new(&["steppe", "ironhold", "dune"]);
    let group = m.army(0, "steppe_outrider", m.start(0) + Vec2::new(-30.0, 0.0), 8);
    m.tick();
    m.set_time(300.0);
    let mut mem = attacking(&group, Objective { player: m.p(1), aim: Aim::Start, pos: m.start(1) }, 295.0);
    m.edit_player(1, |p| p.defeated = true);
    let cmds = m.think(0, &mut mem);
    assert_eq!(attack_moved_to(&cmds, m.start(2)), Some(sorted(&group)), "on to the remaining enemy: {cmds:?}");
    assert!(matches!(mem.attack, AttackState::Attacking { goal, .. } if goal.player == m.p(2)));
}

#[test]
fn retreats_when_the_attack_is_spent() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let survivor = m.army(0, "steppe_outrider", m.start(0) + Vec2::new(-60.0, 0.0), 1);
    m.tick();
    let rally = m.rally(0);
    m.set_time(300.0);
    let mut mem = attacking(&survivor, Objective { player: m.p(1), aim: Aim::Start, pos: m.start(1) }, 250.0);
    mem.wave_pop = 20;
    let cmds = m.think(0, &mut mem);
    assert!(cmds.iter().any(|c| c.kind == CommandKind::Move(rally) && c.entities == survivor), "back to the rally point: {cmds:?}");
    assert_eq!(mem.attack, AttackState::Gathering);
    assert!(mem.attackers.is_empty() && mem.next_attack_after > 300.0);
}

/// An attack that stops getting closer to its goal breaches a hostile wall
/// at its front, or else gives the goal up and moves on.
#[test]
fn a_stalled_attack_breaches_the_wall_or_moves_on() {
    let setup = |wall: bool| {
        let mut m = Match::new(&["steppe", "ironhold", "dune"]);
        let front = Vec2::new(110.0, 130.0);
        let group = m.army(0, "steppe_outrider", front, 6);
        let wall = wall.then(|| {
            let kind = m.building_id("palisade_wall");
            m.building(1, kind, front + Vec2::new(-5.0, 3.0), true)
        });
        m.tick();
        m.set_time(300.0);
        let goal = Objective { player: m.p(1), aim: Aim::Start, pos: m.start(1) };
        let best_dist = front.distance(goal.pos);
        let mut mem = attacking(&group, goal, 240.0);
        mem.attack = AttackState::Attacking { goal, since: 240.0, best_dist, progress_at: 250.0, razing: false };
        (m, group, wall, mem)
    };

    let (mut m, group, wall, mut mem) = setup(true);
    let wall = wall.unwrap();
    let cmds = m.think(0, &mut mem);
    let breach = cmds.iter().find(|c| c.kind == CommandKind::Attack(wall)).unwrap_or_else(|| panic!("attack the wall: {cmds:?}"));
    assert_eq!(sorted(&breach.entities), sorted(&group));
    assert!(matches!(mem.attack, AttackState::Attacking { progress_at, .. } if progress_at == 300.0), "time to break through");

    let (mut m, group, _, mut mem) = setup(false);
    let cmds = m.think(0, &mut mem);
    assert!(mem.is_unreachable(m.start(1)), "the goal is given up for a while");
    assert_eq!(attack_moved_to(&cmds, m.start(2)), Some(sorted(&group)), "on to the next target: {cmds:?}");
}

/// A stalled wave breaches only a wall its team sees: a wall in the fog is
/// no more known to the AI than to a human, so the goal is given up instead.
#[test]
fn a_stalled_attack_breaches_only_a_wall_in_sight() {
    for in_sight in [false, true] {
        let mut m = Match::new(&["steppe", "ironhold", "dune"]);
        let front = Vec2::new(110.0, 130.0);
        let group = m.army(0, "steppe_outrider", front, 6);
        let kind = m.building_id("palisade_wall");
        let wall_at = front + Vec2::new(-12.0, 3.0);
        let wall = m.building(1, kind, wall_at, true);
        // The riders see round themselves; the wall stands 10 m beyond that ...
        let mut discs: Vec<(Vec2, f32)> = (0..6).map(|k| (front + Vec2::new(k as f32 * 2.5, 0.0), 3.0)).collect();
        // ... unless someone lights it.
        if in_sight {
            discs.push((wall_at, 3.0));
        }
        fog_lit(&mut m, &discs);
        m.tick();
        m.set_time(300.0);
        let goal = Objective { player: m.p(1), aim: Aim::Start, pos: m.start(1) };
        let mut mem = attacking(&group, goal, 240.0);
        mem.attack = AttackState::Attacking { goal, since: 240.0, best_dist: front.distance(goal.pos), progress_at: 250.0, razing: false };
        let cmds = m.think(0, &mut mem);
        let breach = cmds.iter().any(|c| c.kind == CommandKind::Attack(wall));
        assert_eq!(breach, in_sight, "in sight {in_sight}: {cmds:?}");
        assert_eq!(mem.is_unreachable(m.start(1)), !in_sight, "a goal behind an unseen wall is given up");
    }
}

/// Rally orders follow the buildings' real rally points, without spamming.
#[test]
fn rallies_production_buildings_once() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let barracks = m.building_id("barracks");
    let set = m.building(0, barracks, m.start(0) + Vec2::new(-20.0, 15.0), true);
    let unset = m.building(0, barracks, m.start(0) + Vec2::new(-20.0, -15.0), true);
    m.tick();
    let rally = m.rally(0);
    m.app.world_mut().entity_mut(set).insert(Rally(Some(RallyTarget::Point(rally))));
    let rallies = |cmds: &[PlayerCommand]| -> Vec<Vec<Entity>> {
        cmds.iter().filter(|c| matches!(c.kind, CommandKind::SetRally { .. })).map(|c| c.entities.clone()).collect()
    };
    let mut mem = AiMemory::default();
    assert_eq!(rallies(&m.think(0, &mut mem)), vec![vec![unset]], "only the building not rallied yet");
    m.set_time(1.0);
    assert!(rallies(&m.think(0, &mut mem)).is_empty(), "not repeated at once");
    m.set_time(12.0);
    assert_eq!(rallies(&m.think(0, &mut mem)), vec![vec![unset]], "repeated later while it still does not hold");
}

/// The plugin turns an UnderAttack message into a defence order.
#[test]
fn defends_buildings_under_attack() {
    let mut m = Match::new(&["tide", "starborn"]).with_ai_plugin();
    m.edit_player(0, |p| p.controller = Controller::Ai);
    let hall = m.hall(0);
    let spear = m.unit_id("spearman");
    let guards: Vec<Entity> = (0..3).map(|k| m.unit(0, spear, m.start(0) + Vec2::new(-10.0, k as f32 * 2.0))).collect();
    let raider_at = m.start(0) + Vec2::new(9.0, 3.0);
    let raider = m.unit(1, spear, raider_at);
    m.tick();
    let hit = m.start(0) + Vec2::new(4.5, 0.0);
    let alert = UnderAttack { victim: hall, victim_owner: m.p(0), attacker: Some(raider), attacker_owner: Some(m.p(1)), pos: hit };
    m.app.world_mut().write_message(alert);
    let mut defended = false;
    for _ in 0..THINK_INTERVAL_TICKS + 1 {
        m.tick();
        let inbox = m.app.world().resource::<CommandInbox>();
        defended |=
            inbox.pending.iter().any(|c| c.kind == CommandKind::AttackMove(raider_at) && guards.iter().all(|g| c.entities.contains(g)));
    }
    assert!(defended, "idle guards attack-move at the attacker");
}

/// Only AI players think, each on its own staggered tick.
#[test]
fn plugin_commands_only_for_ai_players() {
    let mut m = Match::new(&["ironhold", "dune", "steppe"]).with_ai_plugin();
    m.edit_player(0, |p| p.controller = Controller::Ai);
    m.edit_player(2, |p| p.controller = Controller::Ai);
    for i in 0..3 {
        m.hall(i);
        m.workers(i, 5, m.start(i));
    }
    let mut seen: Vec<(u64, PlayerId)> = Vec::new();
    for _ in 0..2 * THINK_INTERVAL_TICKS {
        m.tick();
        let tick = m.app.world().resource::<SimClock>().tick;
        for c in &m.app.world().resource::<CommandInbox>().pending {
            seen.push((tick, c.player));
        }
    }
    assert!(seen.iter().any(|(_, p)| *p == m.p(0)) && seen.iter().any(|(_, p)| *p == m.p(2)), "both AIs act: {seen:?}");
    assert!(seen.iter().all(|(_, p)| *p != m.p(1)), "the inert player never acts");
    for (tick, p) in &seen {
        assert!(think_due(*tick, *p), "player {} acted off its slot at tick {tick}", p.0);
    }
}

// ---------------------------------------------------------------------------
// Spending, gathering plans and counters
// ---------------------------------------------------------------------------

#[test]
fn intel_keeps_recent_peaks_and_fades() {
    let mut intel = EnemyIntel::default();
    intel.observe(0.0, &[0.0, 10.0, 4.0]);
    assert_eq!(intel.total(), 14.0);
    // Fewer in sight a moment later: the peak is remembered.
    intel.observe(1.0, &[0.0, 2.0, 0.0]);
    assert!(intel.pop[1] > 9.9 && intel.pop[2] > 3.9, "{:?}", intel.pop);
    // One half-life later, half of it is left.
    intel.observe(1.0 + memory::INTEL_HALF_LIFE, &[0.0, 0.0, 0.0]);
    assert!((intel.pop[1] - 5.0).abs() < 0.1, "{:?}", intel.pop);
    assert!((intel.pop_where(|u| u.0 == 2) - 2.0).abs() < 0.1);
}

#[test]
fn income_and_split_are_smoothed() {
    let mut mem = AiMemory::default();
    let mut gathered = ResAmounts::ZERO;
    mem.observe_income(0.0, &gathered);
    for t in 1..=600 {
        gathered.0[0] += 5.0; // 5 food a second
        mem.observe_income(t as f32, &gathered);
    }
    assert!((mem.income.0[0] - 5.0).abs() < 0.05, "{:?}", mem.income);
    // A sudden swing in wanted gatherers only moves the split gradually.
    let mut wanted = [0u32; MAX_RESOURCES];
    wanted[0] = 20;
    assert_eq!(mem.smooth_split(0.0, &wanted)[0], 20.0, "the first split is taken as is");
    wanted[0] = 0;
    wanted[1] = 20;
    let split = mem.smooth_split(2.0, &wanted);
    assert!(split[0] > 15.0 && split[1] < 5.0, "{split:?}");
    let split = mem.smooth_split(2.0 + memory::SPLIT_WINDOW, &wanted);
    assert_eq!((split[0], split[1]), (0.0, 20.0), "settled after a window");
}

#[test]
fn apportion_capped_moves_the_overflow() {
    let mut w = [0.0; MAX_RESOURCES];
    w[0] = 1.0;
    w[1] = 1.0;
    w[2] = 2.0;
    let mut caps = [u32::MAX; MAX_RESOURCES];
    caps[2] = 3;
    let a = apportion_capped(&w, 12, &caps);
    assert_eq!(a[2], 3, "capped: {a:?}");
    assert_eq!(a.iter().sum::<u32>(), 12, "the rest goes elsewhere: {a:?}");
    assert!(a[0].abs_diff(a[1]) <= 1, "evenly: {a:?}");
    // Everything capped: some workers are left over.
    let a = apportion_capped(&w, 12, &[1; MAX_RESOURCES]);
    assert_eq!(a.iter().sum::<u32>(), 3);
}

/// A view-level check of the gathering tiers: the age-up's lacking resource
/// gets gatherers first, and a remote resource never more than its nodes take.
#[test]
fn gatherers_serve_the_age_up_first_within_what_the_sources_take() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let data = m.data.clone();
    m.workers(0, 40, m.start(0));
    m.edit_player(0, |p| {
        p.age = 4;
        p.resources = ResAmounts::ZERO;
    });
    m.tick();
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let mem = AiMemory::default();
    let plan = Plan::new(&view, &mem);
    let adamant = data.resource_id("adamant").unwrap();
    let food = data.resource_id("food").unwrap();
    assert!(plan.age_cost.get(adamant) > 0.0, "the last age-up needs adamant");
    let mut supply = Supply { capacity: [Supply::UNBOUNDED; MAX_RESOURCES], near: [true; MAX_RESOURCES] };
    // Two remote meteors: room for six gatherers, walked to from afar.
    supply.capacity[adamant.index()] = 2 * Supply::PER_NODE;
    supply.near[adamant.index()] = false;
    let wanted = desired_gatherers(&view, &plan, &supply, 40);
    assert!((1..=6).contains(&wanted[adamant.index()]), "some, capped by the meteors: {wanted:?}");
    assert!(wanted[food.index()] >= 8, "food for the age-up and the workers: {wanted:?}");
    assert!(wanted.iter().sum::<u32>() <= 40);
    // Nothing to gather adamant from: nobody is sent.
    supply.capacity[adamant.index()] = 0;
    assert_eq!(desired_gatherers(&view, &plan, &supply, 40)[adamant.index()], 0);
}

#[test]
fn fields_ring_a_drop_site_then_a_new_hub() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let home = m.start(0);
    let farm = m.building_id("farm");
    let mill = m.building_id("mill");
    let food = m.data.resource_id("food").unwrap();
    m.tick();
    let spot = |m: &mut Match, pending: &[PendingBuild]| {
        let snap = m.snapshot(0);
        let view = m.view(0, &snap);
        field_spot(&view, food, pending)
    };
    assert!(matches!(spot(&mut m, &[]), Some(FieldSpot::Around { pos, .. }) if pos == home), "around the hall first");
    for k in 0..FIELDS_PER_HUB {
        let at = home + Vec2::from_angle(k as f32 / FIELDS_PER_HUB as f32 * std::f32::consts::TAU) * 12.0;
        m.building(0, farm, at, true);
    }
    m.tick();
    assert_eq!(spot(&mut m, &[]), Some(FieldSpot::NewHub(mill)), "the hall is ringed: a mill next");
    let at = home + Vec2::new(-30.0, 0.0);
    let pending = [PendingBuild { kind: mill, pos: at, obb: Obb::from_size(at, m.data.building(mill).size, 0.0), time: 0.0 }];
    assert!(matches!(spot(&mut m, &pending), Some(FieldSpot::Around { pos, .. }) if pos == at), "the new mill is the next hub");
}

/// Units with bonuses against what the enemy fields gain weight; flyers
/// call for anti-air, known towers for siege.
#[test]
fn army_mix_counters_what_the_enemy_fields() {
    let data = test_data();
    let unit = |id: &str| data.unit_id(id).unwrap_or_else(|| panic!("no unit {id}"));
    let f = data.faction_id("ironhold").unwrap();
    let mut p = PlayerState::new(PlayerId(0), "p".into(), f, [1.0; 3], 0, Controller::Ai, &data);
    p.age = 2;
    let share = |comp: &[(UnitTypeId, f32)], u: UnitTypeId| {
        comp.iter().find(|c| c.0 == u).map_or(0.0, |c| c.1) / comp.iter().map(|c| c.1).sum::<f32>()
    };
    let spear = unit("spearman");
    let calm = army_mix(&data, &p, &EnemyIntel::default(), &[], false, &[]);
    assert_eq!(calm, army_composition(&data, &p), "nothing seen: the faction's preference");
    let mut cavalry = EnemyIntel::default();
    let mut seen = vec![0.0; data.units.len()];
    seen[unit("horse_archer").index()] = 20.0;
    cavalry.observe(0.0, &seen);
    let vs_cavalry = army_mix(&data, &p, &cavalry, &[], false, &[]);
    assert!(share(&vs_cavalry, spear) > share(&calm, spear) * 1.2, "spears against riders: {vs_cavalry:?}");

    // Only flyers seen: whoever has anti-air makes it at least half the mix.
    let mut flyers = EnemyIntel::default();
    let mut seen = vec![0.0; data.units.len()];
    seen[unit("gyrocopter").index()] = 20.0;
    flyers.observe(0.0, &seen);
    for fid in data.faction_ids() {
        let mut p = PlayerState::new(PlayerId(0), "p".into(), fid, [1.0; 3], 0, Controller::Ai, &data);
        p.age = 4;
        let mix = army_mix(&data, &p, &flyers, &[], false, &[]);
        let total: f32 = mix.iter().map(|c| c.1).sum();
        let air: f32 = mix.iter().filter(|(u, _)| hits_air(&data, *u)).map(|c| c.1).sum();
        assert!(air / total >= 0.5, "{}: anti-air share {}: {mix:?}", data.faction(fid).id, air / total);
    }

    // Towers known: siege joins the mix.
    let tower = data.building_id("guard_tower").unwrap();
    let known: Vec<KnownBuilding> = (0..2)
        .map(|k| KnownBuilding { entity: Entity::from_raw_u32(100 + k).unwrap(), owner: PlayerId(1), kind: tower, pos: Vec2::ZERO, facing: 0.0 })
        .collect();
    let f = data.faction_id("sylvan").unwrap();
    let mut p = PlayerState::new(PlayerId(0), "p".into(), f, [1.0; 3], 0, Controller::Ai, &data);
    p.age = 3;
    let mix = army_mix(&data, &p, &EnemyIntel::default(), &known, false, &[]);
    let siege: f32 = mix.iter().filter(|(u, _)| data.unit(*u).has_tag(Tag::Siege)).map(|c| c.1).sum::<f32>()
        / mix.iter().map(|c| c.1).sum::<f32>();
    assert!(siege >= 0.14, "siege share {siege}: {mix:?}");
}

#[test]
fn newer_lines_outweigh_older_ones() {
    let data = test_data();
    let f = data.faction_id("ironhold").unwrap();
    let mut p = PlayerState::new(PlayerId(0), "p".into(), f, [1.0; 3], 0, Controller::Ai, &data);
    p.age = 3;
    let comp = army_composition(&data, &p);
    let weight_by_age = |age: u8| comp.iter().filter(|(u, _)| data.unit(*u).age == age).map(|c| c.1).sum::<f32>();
    assert!(weight_by_age(3) > weight_by_age(1), "{comp:?}");
}

/// Revenge pulls an attack toward whoever hit us; repeated waves at one
/// neighbour push the next toward another.
#[test]
fn attack_targets_follow_grudges_and_spread() {
    let mut m = Match::new(&["steppe", "ironhold", "dune"]);
    let home = m.start(0);
    // Both enemies as far from home.
    let d = 80.0;
    m.edit_player(1, |p| p.start_pos = home + Vec2::new(d, 0.0));
    m.edit_player(2, |p| p.start_pos = home + Vec2::new(0.0, d * 1.05));
    m.tick();
    m.set_time(600.0);
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let mut mem = AiMemory::default();
    let goal = |mem: &AiMemory| choose_target(&view, mem, home).expect("a target").player;
    assert_eq!(goal(&mem), m.p(1), "the nearer one");
    mem.note_target(m.p(1));
    assert_eq!(goal(&mem), m.p(2), "after a wave at the first, the other neighbour");
    mem.note_grudge(m.p(1), 590.0);
    assert_eq!(goal(&mem), m.p(1), "revenge outweighs spreading");
    mem.grudges.clear();
    mem.note_target(m.p(2));
    mem.note_target(m.p(2));
    assert_eq!(goal(&mem), m.p(1), "two waves at the second: back to the first");
    // Mid-attack, the player being attacked stays the target.
    let mem = AiMemory { target_history: vec![m.p(2), m.p(2), m.p(2)], ..default() };
    assert_eq!(choose_target_for(&view, &mem, home, Some(m.p(2))).unwrap().player, m.p(2));
}

/// While saving for an upgrade on the age-up path, the upgrade is set aside
/// whole; the final age-up tech only just in time.
#[test]
fn intermediate_age_up_steps_are_saved_for_whole() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let data = m.data.clone();
    let to_4 = data.age_up_tech(m.player(0).faction, 3).unwrap();
    for b in data.tech(to_4).requires_building_ids.clone() {
        m.building(0, b, m.start(0) + Vec2::new(0.0, -25.0), true);
    }
    m.edit_player(0, |p| {
        p.age = 3;
        p.resources = ResAmounts::ZERO;
    });
    m.tick();
    let upgrade = data.building(m.hall_kind(0)).upgrade.clone().unwrap().cost.amounts;
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    // Plenty of income: just in time alone would keep almost nothing.
    let mem = AiMemory { income: ResAmounts([50.0; MAX_RESOURCES]), ..default() };
    let plan = Plan::new(&view, &mem);
    assert_eq!(plan.age_up, AgeUpStep::Save { cost: upgrade });
    for i in 0..MAX_RESOURCES {
        assert!(plan.reserve.0[i] >= upgrade.0[i], "the upgrade is kept whole: {:?}", plan.reserve);
    }
    assert!(plan.reserve.total() < plan.age_cost.total(), "the tech after it only just in time");
}

/// A short army may borrow from the age-up saving; a big enough one may not.
#[test]
fn a_short_army_borrows_from_the_saving() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let data = m.data.clone();
    let barracks = m.building_id("barracks");
    m.building(0, barracks, m.start(0) + Vec2::new(-20.0, 15.0), true);
    m.workers(0, worker_target(1) as usize, m.start(0));
    let house = m.building_id("house");
    for k in 0..4 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -15.0 + k as f32 * 6.0), true);
    }
    // Just short of the age-up's food: all of it is saving (with wood to spare).
    let to_2 = data.age_up_tech(m.player(0).faction, 1).unwrap();
    let mut cost = data.tech(to_2).cost.amounts;
    cost.0[0] -= 1.0;
    cost.0[1] += 2000.0;
    m.edit_player(0, |p| p.resources = cost);
    m.tick();
    let trained = |cmds: &[PlayerCommand]| {
        cmds.iter().any(|c| matches!(c.kind, CommandKind::Train { unit, .. } if data.unit(unit).gatherer.is_none()))
    };
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(trained(&cmds), "no army at all: train some: {cmds:?}");
    m.army(0, "spearman", m.start(0) + Vec2::new(-10.0, 25.0), 12);
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(!trained(&cmds), "enough army: keep saving: {cmds:?}");
}

/// When every production queue is full and the bank overflows, another
/// production building goes up.
#[test]
fn a_surplus_buys_more_production() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let barracks = m.building_id("barracks");
    let range = m.building_id("archery_range");
    let hall = m.building_id("ironhold_mead_hall");
    let spear = m.unit_id("spearman");
    m.workers(0, 30, m.start(0));
    let mut production = Vec::new();
    for (k, kind) in [hall, hall, range, barracks].into_iter().enumerate() {
        production.push(m.building(0, kind, m.start(0) + Vec2::new(-30.0 + 15.0 * k as f32, 30.0), true));
    }
    let house = m.building_id("house");
    for k in 0..6 {
        m.building(0, house, m.start(0) + Vec2::new(30.0, -20.0 + k as f32 * 6.0), true);
    }
    for b in &production {
        m.app.world_mut().get_mut::<ProductionQueue>(*b).unwrap().items.extend(
            [QueueItem { kind: QueueKind::Unit(spear), paid: ResAmounts::ZERO, time: 30.0 }; MAX_QUEUE],
        );
    }
    m.edit_player(0, |p| {
        p.age = 2;
        p.resources = ResAmounts([3000.0; MAX_RESOURCES]);
    });
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    let placed: Vec<BuildingTypeId> = cmds
        .iter()
        .filter_map(|c| match c.kind {
            CommandKind::PlaceBuilding { kind, .. } if [barracks, range, hall].contains(&kind) => Some(kind),
            _ => None,
        })
        .collect();
    assert_eq!(placed.len(), 1, "one more production building: {cmds:?}");
    // Room left in a queue: no more production.
    m.app.world_mut().get_mut::<ProductionQueue>(production[0]).unwrap().items.clear();
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(!cmds.iter().any(|c| matches!(c.kind, CommandKind::PlaceBuilding { kind, .. } if [barracks, range, hall].contains(&kind))));
}

/// A raid that outnumbers the army at home recalls the attack.
#[test]
fn an_outnumbering_raid_recalls_the_attack() {
    let mut m = Match::new(&["tide", "steppe"]);
    m.hall(0);
    let far = m.start(1) + Vec2::new(-10.0, 0.0);
    let group = m.army(0, "spearman", far, 10);
    let raid_at = m.start(0) + Vec2::new(8.0, 4.0);
    let raiders = m.army(1, "steppe_outrider", raid_at, 8);
    m.tick();
    m.set_time(600.0);
    let mut mem = attacking(&group, Objective { player: m.p(1), aim: Aim::Start, pos: m.start(1) }, 560.0);
    mem.alerts.push(Alert { at: m.start(0), attacker: raid_at, attacker_owner: Some(m.p(1)), time: 599.0 });
    let cmds = m.think(0, &mut mem);
    assert_eq!(mem.attack, AttackState::Gathering, "{cmds:?}");
    assert_eq!(attack_moved_to(&cmds, raid_at), Some(sorted(&group)), "the attack comes home: {cmds:?}");
    assert!(mem.holds_grudge(m.p(1), 600.0), "and the raider is remembered");
    let _ = raiders;
}

/// An attack group that lost half of what was sent and faces a stronger
/// force falls back.
#[test]
fn a_losing_wave_falls_back() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    // On the way, short of the enemy base, meeting its army.
    let at = m.start(1) + Vec2::new(40.0, 0.0);
    let group = m.army(0, "steppe_outrider", at, 6);
    m.army(1, "spearman", at + Vec2::new(-4.0, -6.0), 24);
    m.tick();
    let rally = m.rally(0);
    m.set_time(600.0);
    let mut mem = attacking(&group, Objective { player: m.p(1), aim: Aim::Start, pos: m.start(1) }, 590.0);
    mem.wave_pop = 30;
    let cmds = m.think(0, &mut mem);
    assert_eq!(mem.attack, AttackState::Gathering, "{cmds:?}");
    assert!(cmds.iter().any(|c| c.kind == CommandKind::Move(rally)), "back to the rally point: {cmds:?}");
    // Without the losses, it fights on.
    let mut mem = attacking(&group, Objective { player: m.p(1), aim: Aim::Start, pos: m.start(1) }, 590.0);
    mem.wave_pop = 12;
    let cmds = m.think(0, &mut mem);
    assert!(matches!(mem.attack, AttackState::Attacking { .. }), "{:?} {cmds:?}", mem.attack);
}

/// Only hostile army the team sees is weighed: a wave down to less than
/// half of what it sent fights on against a stronger army standing in the
/// fog round it, and falls back once that army comes into sight.
#[test]
fn a_wave_weighs_only_the_hostile_army_in_sight() {
    for in_sight in [false, true] {
        let mut m = Match::new(&["steppe", "ironhold"]);
        let at = m.start(1) + Vec2::new(40.0, 0.0);
        let group = m.army(0, "steppe_outrider", at, 6);
        // 24 spearmen 10-19 m south of the riders' centre: within the
        // battle radius, beyond what the riders light here.
        let foes_from = at + Vec2::new(0.0, -18.0);
        m.army(1, "spearman", foes_from, 24);
        let mut discs: Vec<(Vec2, f32)> = (0..6).map(|k| (at + Vec2::new(k as f32 * 2.5, 0.0), 4.0)).collect();
        if in_sight {
            discs.push((foes_from + Vec2::new(6.0, 4.0), 12.0));
        }
        fog_lit(&mut m, &discs);
        m.tick();
        m.set_time(600.0);
        let mut mem = attacking(&group, Objective { player: m.p(1), aim: Aim::Start, pos: m.start(1) }, 590.0);
        mem.wave_pop = 30;
        let cmds = m.think(0, &mut mem);
        assert_eq!(mem.attack == AttackState::Gathering, in_sight, "in sight {in_sight}: {:?} {cmds:?}", mem.attack);
    }
}

/// A defensive faction walls its approach in two wings once a tower
/// stands, and upgrades its tower when the age allows, but only with an
/// army of at least half its target.
#[test]
fn defensive_factions_wall_and_upgrade() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let data = m.data.clone();
    assert!(data.faction(m.player(0).faction).ai.aggression < 0.5);
    m.workers(0, 20, m.start(0));
    let watchtower = m.building_id("watchtower");
    let tower = m.building(0, watchtower, m.start(0) + Vec2::new(14.0, 14.0), true);
    m.edit_player(0, |p| {
        p.age = 2;
        p.resources = ResAmounts([5000.0; MAX_RESOURCES]);
    });
    m.tick();
    let fortifies = |cmds: &[PlayerCommand]| {
        cmds.iter().any(|c| matches!(c.kind, CommandKind::PlaceWall { .. } | CommandKind::UpgradeBuilding))
    };
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(!fortifies(&cmds), "no army: no walls or upgrades yet: {cmds:?}");
    m.army(0, "spearman", m.start(0) + Vec2::new(-12.0, 20.0), 14);
    m.tick();
    let mut mem = AiMemory::default();
    let cmds = m.think(0, &mut mem);
    let walls: Vec<(Vec2, Vec2)> = cmds
        .iter()
        .filter_map(|c| match c.kind {
            CommandKind::PlaceWall { from, to, .. } => Some((from, to)),
            _ => None,
        })
        .collect();
    assert_eq!(walls.len(), 2, "two wings: {cmds:?}");
    let gap = walls[0].1.distance(walls[1].0);
    assert!(gap > 5.0, "an opening between the wings ({gap} m)");
    assert!(cmds.iter().any(|c| c.kind == CommandKind::UpgradeBuilding && c.entities == vec![tower]), "the tower is upgraded: {cmds:?}");
    // Not repeated at once.
    m.set_time(1.0);
    let again = m.think(0, &mut mem);
    assert!(!again.iter().any(|c| matches!(c.kind, CommandKind::PlaceWall { .. })), "{again:?}");
}

/// A wall wing laid 1.6 m in front of a bay of barracks would close the bay
/// on wide movers: the wing goes out without the segments across the
/// bay's mouth, and a ram in the bay still gets out, by the real path
/// finder on a nav grid that holds what af_economy would lay.
#[test]
fn a_wall_never_closes_a_bay_on_wide_movers() {
    use af_geom::{NO_TEAM, PathRequest};
    let scene = || {
        let mut m = Match::new(&["ironhold", "dune"]);
        m.hall(0);
        m.workers(0, 20, m.start(0));
        let watchtower = m.building_id("watchtower");
        m.building(0, watchtower, m.start(0) + Vec2::new(14.0, 14.0), true);
        m.edit_player(0, |p| {
            p.age = 2;
            p.resources = ResAmounts([5000.0; MAX_RESOURCES]);
        });
        m.army(0, "spearman", m.start(0) + Vec2::new(-12.0, 20.0), 14);
        m
    };
    let wings = |cmds: &[PlayerCommand]| -> Vec<(BuildingTypeId, Vec2, Vec2)> {
        cmds.iter()
            .filter_map(|c| match c.kind {
                CommandKind::PlaceWall { kind, from, to } => Some((kind, from, to)),
                _ => None,
            })
            .collect()
    };
    // Where the wings go on open ground.
    let mut m = scene();
    m.tick();
    let open = wings(&m.think(0, &mut AiMemory::default()));
    assert_eq!(open.len(), 2, "two wings: {open:?}");
    let (_, from, to) = open[0];
    let dir = (to - from).normalize();
    assert!(dir.x.abs() < 1e-3, "a wing across an approach from the east: {dir}");
    let mid = (from + to) * 0.5;
    let inward = if (m.start(0) - mid).dot(dir.perp()) > 0.0 { dir.perp() } else { -dir.perp() };
    // The bay: two barracks 4 m apart whose fronts stand 1.6 m off the
    // wall's face, a third 2 m behind them.
    let barracks = m.building_id("barracks");
    let front = 0.5 + 1.6 + 4.0;
    let bay = [mid + inward * front - dir * 6.0, mid + inward * front + dir * 6.0, mid + inward * (front + 10.0)];
    let mut m = scene();
    for at in bay {
        m.building(0, barracks, at, true);
    }
    m.tick();
    let laid = wings(&m.think(0, &mut AiMemory::default()));
    // What af_economy would lay, registered as af_movement does.
    let snap = m.snapshot(0);
    let mut boxes: Vec<Obb> = Vec::new();
    {
        let view = m.view(0, &snap);
        let mut placed: Vec<Obb> = Vec::new();
        for (kind, from, to) in &laid {
            for (pos, yaw) in wall_segments(view.data, *kind, *from, *to) {
                if check_placement(view.data, view.nav, view.spatial, view.me, *kind, pos, yaw, false, &placed).is_ok() {
                    placed.push(placed_footprint(view.data, *kind, pos, yaw));
                    boxes.push(Obb::from_size(pos, view.data.building(*kind).size, yaw));
                }
            }
        }
        boxes.extend(view.spatial.entries.iter().filter(|e| e.kind == SpatialKind::Building).map(|e| e.obb));
    }
    let ram = {
        let view = m.view(0, &snap);
        *ground_movers(&view).iter().find(|mv| mv.clearance == 1).expect("ironhold fields movers of clearance 1")
    };
    let mut nav = m.app.world_mut().resource_mut::<Nav>();
    let nav = &mut nav.0;
    nav.set_walk_margin(WALK_MARGIN);
    for b in &boxes {
        nav.add_obstacle(b, 1);
    }
    let req = PathRequest {
        start: mid + inward * (front + 1.0),
        goal: mid - inward * 20.0,
        mobility: ram.mobility,
        radius: 0.9,
        team: NO_TEAM,
        max_expansions: 200_000,
        reach: None,
    };
    assert!(nav.find_path(&req).complete, "a ram in the bay still gets out: {laid:?}");
    assert!(laid.len() > 2, "a wing is split round the bay's mouth: {laid:?}");
}

// ---------------------------------------------------------------------------
// Whole-game progression against a stand-in economy
// ---------------------------------------------------------------------------

/// A crude stand-in for af_behavior/af_economy so whole-game progression
/// can be checked here: commands are validated with the shared rules and
/// paid; production queues, research, upgrades and construction take their
/// real times; gatherers earn their rate without walking; nothing fights.
struct StandInEconomy;

impl Plugin for StandInEconomy {
    fn build(&self, app: &mut App) {
        app.init_resource::<Executed>()
            .add_systems(FixedUpdate, (execute, gather_income, produce, construct).chain().in_set(SimSet::Commands));
    }
}

/// What the stand-in economy carried out.
#[derive(Resource, Default, Debug)]
struct Executed {
    placed: Vec<BuildingTypeId>,
    trained: u32,
    researched: Vec<TechId>,
    upgraded: u32,
    attack_moves: u32,
}

#[allow(clippy::too_many_arguments)]
fn execute(
    mut commands: Commands,
    tick: Res<TickCommands>,
    data: Res<Data>,
    nav: Res<Nav>,
    spatial: Res<SpatialIndex>,
    mut players: ResMut<Players>,
    mut done: ResMut<Executed>,
    mut orders: Query<&mut Orders>,
    mut buildings: Query<(&Building, &mut ProductionQueue), Without<UnderConstruction>>,
) {
    let data: &GameData = &data.0;
    let mut placed: Vec<Obb> = Vec::new();
    for c in &tick.list {
        let p = c.player;
        let queue_at = |b: Entity, buildings: &mut Query<(&Building, &mut ProductionQueue), Without<UnderConstruction>>| {
            buildings.get(b).ok().map(|(building, q)| (building.kind, q.items.len()))
        };
        match &c.kind {
            CommandKind::PlaceBuilding { kind, pos, facing } => {
                let player = players.get(p).clone();
                let Ok(pl) = check_placement(data, &nav.0, &spatial, &player, *kind, *pos, *facing, true, &placed) else { continue };
                players.get_mut(p).pay(&stats::building_cost(data, &player, *kind));
                placed.push(Obb::from_size(pl.pos, data.building(*kind).size, pl.facing));
                spawn::spawn_building(&mut commands, data, &player, *kind, pl.pos, pl.facing, false);
                done.placed.push(*kind);
            }
            CommandKind::Train { unit, .. } => {
                let Some((kind, len)) = c.entities.first().and_then(|b| queue_at(*b, &mut buildings)) else { continue };
                let player = players.get_mut(p);
                if len >= ProductionQueue::MAX || rules::can_train(data, player, kind, *unit, true).is_err() {
                    continue;
                }
                let paid = stats::unit_cost(data, player, *unit);
                player.pay(&paid);
                let time = stats::unit_train_time(data, player, *unit);
                if let Ok((_, mut q)) = buildings.get_mut(c.entities[0]) {
                    q.items.push_back(QueueItem { kind: QueueKind::Unit(*unit), paid, time });
                }
            }
            CommandKind::Research { tech } => {
                let Some((kind, _)) = c.entities.first().and_then(|b| queue_at(*b, &mut buildings)) else { continue };
                let player = players.get_mut(p);
                if rules::can_research(data, player, kind, *tech, true).is_err() {
                    continue;
                }
                let def = data.tech(*tech);
                player.pay(&def.cost.amounts);
                player.researching[tech.index()] = true;
                if let Ok((_, mut q)) = buildings.get_mut(c.entities[0]) {
                    q.items.push_back(QueueItem { kind: QueueKind::Tech(*tech), paid: def.cost.amounts, time: def.time });
                }
            }
            CommandKind::UpgradeBuilding => {
                for b in &c.entities {
                    let Ok((building, mut q)) = buildings.get_mut(*b) else { continue };
                    let player = players.get_mut(p);
                    let Ok(to) = rules::can_upgrade(data, player, building.kind, true) else { continue };
                    let Some(up) = data.building(building.kind).upgrade.as_ref() else { continue };
                    player.pay(&up.cost.amounts);
                    q.items.push_back(QueueItem { kind: QueueKind::Upgrade(to), paid: up.cost.amounts, time: up.time });
                }
            }
            CommandKind::Gather(target) => {
                for u in &c.entities {
                    if let Ok(mut o) = orders.get_mut(*u) {
                        o.set(Order::Gather { target: *target }, false);
                        commands.entity(*u).remove::<Idle>();
                    }
                }
            }
            CommandKind::AttackMove(point) | CommandKind::Move(point) => {
                done.attack_moves += u32::from(matches!(c.kind, CommandKind::AttackMove(_)));
                for u in &c.entities {
                    if let Ok(mut o) = orders.get_mut(*u) {
                        o.set(Order::Move { target: *point }, false);
                        commands.entity(*u).remove::<Idle>();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Gatherers earn their rate every tick wherever they are; nodes deplete,
/// and a worker whose node is gone goes idle.
fn gather_income(
    mut commands: Commands,
    data: Res<Data>,
    mut players: ResMut<Players>,
    mut workers: Query<(Entity, &Unit, &Owner, &mut Orders), With<Gatherer>>,
    mut nodes: Query<&mut ResourceNode>,
    mut fields: Query<&mut ResourceField>,
) {
    for (e, unit, owner, mut orders) in &mut workers {
        let Some(Order::Gather { target }) = orders.current().copied() else { continue };
        let resource = nodes.get(target).map(|n| n.resource).or(fields.get(target).map(|f| f.resource));
        let Ok(r) = resource else {
            orders.queue.clear();
            commands.entity(e).insert(Idle);
            continue;
        };
        let player = players.get_mut(owner.0);
        let amount = stats::gather_rate(&data.0, player, unit.kind, r) * SIM_DT;
        player.resources.add(r, amount);
        let left = if let Ok(mut n) = nodes.get_mut(target) {
            n.remaining -= amount;
            n.remaining
        } else if let Ok(mut f) = fields.get_mut(target) {
            f.remaining -= amount;
            f.remaining
        } else {
            1.0
        };
        if left <= 0.0 {
            commands.entity(target).try_despawn();
        }
    }
}

/// Advance the front item of every production queue.
fn produce(
    mut commands: Commands,
    data: Res<Data>,
    mut players: ResMut<Players>,
    mut done: ResMut<Executed>,
    mut buildings: Query<(&mut Building, &Owner, &Position, &mut ProductionQueue), Without<UnderConstruction>>,
) {
    let data: &GameData = &data.0;
    for (mut building, owner, pos, mut q) in &mut buildings {
        let Some(front) = q.items.front().copied() else { continue };
        let player = players.get_mut(owner.0);
        if let QueueKind::Unit(u) = front.kind {
            let d = data.unit(u);
            q.pop_blocked = player.pop_used + d.pop * d.squad > player.pop_cap;
            if q.pop_blocked {
                continue;
            }
        }
        q.progress += SIM_DT;
        if q.progress < front.time {
            continue;
        }
        q.progress = 0.0;
        q.items.pop_front();
        match front.kind {
            QueueKind::Unit(u) => {
                for k in 0..data.unit(u).squad {
                    spawn::spawn_unit(&mut commands, data, player, u, pos.0 + Vec2::new(k as f32, -7.0), 0.0);
                }
                // Count the squad now so the next queue this tick sees it.
                player.pop_used += data.unit(u).pop * data.unit(u).squad;
                done.trained += 1;
            }
            QueueKind::Tech(t) => {
                player.researching[t.index()] = false;
                player.researched[t.index()] = true;
                for e in &data.tech(t).effects {
                    if matches!(e, Effect::AdvanceAge) {
                        player.age += 1;
                    }
                    player.mods.apply_effect(data, e);
                }
                done.researched.push(t);
            }
            QueueKind::Upgrade(to) => {
                building.kind = to;
                done.upgraded += 1;
            }
        }
    }
}

/// Foundations finish after their build time, builders or not.
fn construct(mut commands: Commands, mut sites: Query<(Entity, &mut UnderConstruction, &mut Health)>) {
    for (e, mut site, mut hp) in &mut sites {
        site.progress += SIM_DT / site.build_time.max(0.5);
        if site.progress >= 1.0 {
            hp.hp = hp.max;
            commands.entity(e).remove::<UnderConstruction>();
        }
    }
}

/// How a stand-in game went for the AI player.
struct Played {
    age: u8,
    /// Sim second each age (index) was reached.
    reached: [u32; MAX_AGE as usize + 1],
    military_buildings: usize,
    upgraded: u32,
    researched: usize,
    trained: u32,
    attack_moves: u32,
    summary: String,
}

/// Play `faction` (as an AI against an idle opponent) for up to `minutes`
/// against the stand-in economy, on a map with every resource in reach.
fn play_stand_in(faction: &str, minutes: f32) -> Played {
    let mut m = Match::new(&[faction, "dune"]).with_ai_plugin();
    m.app.add_plugins(StandInEconomy);
    m.edit_player(0, |p| p.controller = Controller::Ai);
    m.hall(0);
    m.hall(1);
    let home = m.start(0);
    let centre = Vec2::splat(MAP * 0.5);
    let ring = |n: usize, at: Vec2, r: f32| (0..n).map(move |k| at + Vec2::from_angle(k as f32 / n as f32 * std::f32::consts::TAU) * r);
    for p in ring(8, home + Vec2::new(-14.0, 10.0), 4.0) {
        m.node("berry_bush", p);
    }
    // Two forests near home and one farther out.
    for (corner, n) in [(Vec2::new(-34.0, -22.0), 48), (Vec2::new(14.0, -40.0), 48), (Vec2::new(-70.0, 30.0), 64)] {
        for k in 0..n {
            m.node("oak", home + corner + Vec2::new((k % 8) as f32 * 1.8, -((k / 8) as f32) * 1.8));
        }
    }
    for (row, id) in ["stone_outcrop", "copper_vein", "iron_vein"].iter().enumerate() {
        for k in 0..6 {
            m.node(id, home + Vec2::new(8.0 + k as f32 * 3.2, 24.0 + row as f32 * 3.2));
        }
    }
    // A richer ore field in the middle ring.
    for (row, id) in ["stone_outcrop", "copper_vein", "iron_vein"].iter().enumerate() {
        for k in 0..8 {
            m.node(id, centre + Vec2::new(-20.0 + k as f32 * 3.2, 45.0 + row as f32 * 3.2));
        }
    }
    for p in ring(6, home + Vec2::new(-10.0, 45.0), 4.0) {
        m.node("silicon_deposit", p);
    }
    for p in ring(10, centre, 9.0) {
        m.node("aether_crystal", p);
    }
    for p in ring(3, centre, 16.0) {
        m.node("aether_vent", p);
    }
    for p in ring(3, centre, 3.5) {
        m.node("adamant_meteor", p);
    }
    let start = m.data.faction(m.player(0).faction).start.units.clone();
    for (id, n) in start {
        let kind = m.unit_id(&id);
        for k in 0..n {
            m.unit(0, kind, home + Vec2::new(k as f32 * 1.5 - 3.0, -8.0));
        }
    }
    m.tick();

    let mut reached = [0u32; MAX_AGE as usize + 1];
    for t in 0..(minutes * 60.0 * SIM_HZ as f32) as u32 {
        m.tick();
        let age = m.player(0).age as usize;
        if reached[age] == 0 {
            reached[age] = t / SIM_HZ as u32;
        }
        if age == MAX_AGE as usize {
            break;
        }
    }
    let p = m.player(0).clone();
    let done = m.app.world().resource::<Executed>();
    let mut built: Vec<&str> = done.placed.iter().map(|b| m.data.building(*b).id.as_str()).collect();
    built.sort();
    built.dedup();
    let summary = format!(
        "{faction}: age {} (reached at {:?} s), pop {}/{}, trained {}, built [{}], researched {}, upgrades {}, attack orders {}",
        p.age,
        &reached[2..],
        p.pop_used,
        p.pop_cap,
        done.trained,
        built.join(", "),
        done.researched.len(),
        done.upgraded,
        done.attack_moves
    );
    Played {
        age: p.age,
        reached,
        military_buildings: done.placed.iter().filter(|b| m.data.building(**b).has_tag(Tag::Military)).count(),
        upgraded: done.upgraded,
        researched: done.researched.len(),
        trained: done.trained,
        attack_moves: done.attack_moves,
        summary,
    }
}

/// Against the stand-in economy, an AI goes from the start position through
/// every age, builds an economy and an army, and attacks.
#[test]
fn ai_plays_through_the_ages_against_a_stand_in_economy() {
    let g = play_stand_in("ironhold", 40.0);
    let s = &g.summary;
    assert_eq!(g.age, MAX_AGE, "the AI should reach the last age: {s}");
    assert!(g.reached[2] > 0 && g.reached[2] < 600, "Banner Age within ten minutes: {s}");
    assert!(g.upgraded >= 1, "the hall is upgraded for the late age-ups: {s}");
    assert!(g.researched > 8, "economy and military research happen: {s}");
    assert!(g.military_buildings >= 3, "military production is built: {s}");
    assert!(g.trained > 40, "workers and soldiers are trained: {s}");
    assert!(g.attack_moves > 0, "the army is sent out: {s}");
}

/// Every faction's content leads its AI up the ages (slow: run with
/// `cargo test -p af_ai --release -- --ignored`).
#[test]
#[ignore]
fn every_faction_plays_through_the_ages_against_a_stand_in_economy() {
    let data = test_data();
    let mut failures = Vec::new();
    for f in &data.factions {
        let g = play_stand_in(&f.id, 40.0);
        println!("{}", g.summary);
        if g.age < MAX_AGE || g.military_buildings < 2 || g.attack_moves == 0 {
            failures.push(g.summary);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// ---------------------------------------------------------------------------
// Spending what is gathered
// ---------------------------------------------------------------------------

/// A resource piling up beyond the reserve gives its gatherers to what the
/// age-up lacks; without anything lacking, to the rest of the plan.
#[test]
fn piles_hand_their_gatherers_to_the_bottleneck() {
    let data = test_data();
    let index = |id: &str| data.resource_id(id).unwrap().index();
    let (food, wood, iron) = (index("food"), index("wood"), index("iron"));
    let rates = [0.4; MAX_RESOURCES];
    let mut weights = [0.0; MAX_RESOURCES];
    weights[wood] = 1.0;
    let mut have = ResAmounts::ZERO;
    have.0[iron] = PILE_KEEP + 0.4 * PLAN_HORIZON * 3.5; // three gatherers' worth over the horizon, and a half
    let mut age_cost = ResAmounts::ZERO;
    age_cost.0[food] = 700.0;
    let shifted = |reserve: &ResAmounts, age_cost: &ResAmounts| {
        let mut out = [0u32; MAX_RESOURCES];
        out[iron] = 5;
        out[food] = 4;
        let mut room = [100u32; MAX_RESOURCES];
        shift_from_piles(&have, reserve, age_cost, &rates, &weights, &mut out, &mut room);
        out
    };
    let out = shifted(&ResAmounts::ZERO, &age_cost);
    assert_eq!((out[iron], out[food]), (2, 7), "three of five iron gatherers go to the age-up's food");
    let out = shifted(&ResAmounts::ZERO, &ResAmounts::ZERO);
    assert_eq!((out[iron], out[wood], out[food]), (2, 3, 4), "nothing lacking: the rest of the plan takes them");
    let out = shifted(&have, &age_cost);
    assert_eq!((out[iron], out[food]), (5, 4), "the stock is saving, not a pile");
}

/// The army is planned to spend what it spent lately (with room to grow),
/// within what its production turns out, and never less than a floor.
#[test]
fn army_spending_follows_what_the_army_spent() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let barracks = m.building_id("barracks");
    for k in 0..4 {
        m.building(0, barracks, m.start(0) + Vec2::new(-30.0 + 12.0 * k as f32, 20.0), true);
    }
    m.tick();
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let pop_planned = |spend: f32| {
        let mut mem = AiMemory::default();
        mem.army_spend.0[0] = spend;
        army_spending(&view, &Plan::new(&view, &mem)).1
    };
    assert_eq!(pop_planned(0.0), ARMY_PLAN_MIN_POP, "nothing spent lately: the floor");
    let some = pop_planned(6.0);
    assert!(some > ARMY_PLAN_MIN_POP, "spending lately raises the plan: {some}");
    let capacity = pop_planned(1e6);
    assert!(capacity > some && pop_planned(2e6) == capacity, "capped by what the barracks turn out: {capacity}");
}

/// Of two units equally wanted, the one paid out of what piles up is
/// trained.
#[test]
fn surplus_pulls_the_unit_choice() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    m.edit_player(0, |p| p.age = 2);
    m.tick();
    let data = m.data.clone();
    let (man, spear) = (m.unit_id("man_at_arms"), m.unit_id("spearman"));
    let barracks = m.building_id("barracks");
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let comp = vec![(man, 1.0), (spear, 1.0)];
    let pop = vec![0u32; data.units.len()];
    let mut have = ResAmounts::ZERO;
    for (r, x) in [("food", 1000.0), ("wood", 1000.0), ("iron", 1000.0)] {
        have.set(data.resource_id(r).unwrap(), x);
    }
    // Wood and iron pile up alike: the even mix decides (the first listed).
    let pick = |budget: &Budget| pick_unit(&view, &comp, &pop, barracks, budget, 1.0, 10);
    let mut reserve = ResAmounts::ZERO;
    reserve.set(data.resource_id("food").unwrap(), 900.0);
    let even = pick(&Budget::new(have, reserve));
    // Iron no longer piles up: the spearman, paid in food and wood, is pulled ahead.
    let mut tight = reserve;
    tight.set(data.resource_id("iron").unwrap(), 950.0);
    assert_eq!(pick(&Budget::new(have, tight)), Some(spear));
    // Wood no longer piles up: the man-at-arms (food and iron) is.
    let mut tight = reserve;
    tight.set(data.resource_id("wood").unwrap(), 950.0);
    assert_eq!(pick(&Budget::new(have, tight)), Some(man));
    assert!(even.is_some());
    assert!(surplus_share(&data.unit(man).cost.amounts, &surplus(&Budget::new(have, ResAmounts::ZERO))) > 0.99);
}

/// A field is only built when the fields standing are taken and a farmer
/// would work the new one soon: no more than two gatherers ahead of those
/// on food.
#[test]
fn fields_wait_for_their_farmers() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let data = m.data.clone();
    let farm = m.building_id("farm");
    let mill = m.building_id("mill");
    m.building(0, mill, m.start(0) + Vec2::new(-15.0, 0.0), true);
    m.workers(0, 10, m.start(0));
    m.edit_player(0, |p| p.resources = ResAmounts([2000.0; MAX_RESOURCES]));
    m.tick();
    let food = data.resource_id("food").unwrap().index();
    // The smoothed gatherer split decides how many farmers are wanted.
    let wanting = |n: f32| {
        let mut split = [0.0; MAX_RESOURCES];
        split[food] = n;
        AiMemory { split: Some((split, 1e9)), ..default() }
    };
    let cmds = m.think(0, &mut wanting(1.0));
    assert_eq!(placements(&cmds, farm), 1, "no food nodes, one farmer wanted: a field: {cmds:?}");
    m.building(0, farm, m.start(0) + Vec2::new(-15.0, 9.0), true);
    m.tick();
    let cmds = m.think(0, &mut wanting(1.0));
    assert_eq!(placements(&cmds, farm), 0, "the field standing takes the one farmer: {cmds:?}");
    let cmds = m.think(0, &mut wanting(6.0));
    assert_eq!(placements(&cmds, farm), 1, "more farmers wanted: one more field, not five: {cmds:?}");
}

/// When the army is short of its goal and nothing it wants is affordable,
/// what the wanted unit lacks is remembered, so gathering turns to it.
#[test]
fn a_short_army_remembers_what_it_lacks() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let data = m.data.clone();
    let hall = m.building_id("ironhold_mead_hall");
    m.building(0, hall, m.start(0) + Vec2::new(-20.0, 15.0), true);
    let house = m.building_id("house");
    for k in 0..4 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -10.0 + k as f32 * 6.0), true);
    }
    m.workers(0, worker_target(2) as usize, m.start(0));
    let iron = data.resource_id("iron").unwrap();
    m.edit_player(0, |p| {
        p.age = 2;
        p.resources = ResAmounts([3000.0; MAX_RESOURCES]);
        p.resources.set(iron, 0.0);
    });
    m.tick();
    let mut mem = AiMemory::default();
    m.think(0, &mut mem);
    assert!(mem.shortfall.get(iron) > 0.0, "the thane guards wait on iron: {:?}", mem.shortfall);
}

/// Gatherers are only planned for what passive income does not bring in.
#[test]
fn passive_income_needs_no_gatherers() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let data = m.data.clone();
    let extractor = m.building_id("aether_extractor");
    m.tick();
    let aether = data.resource_id("aether").unwrap();
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    assert_eq!(passive_income(&view).get(aether), 0.0);
    for k in 0..3 {
        m.building(0, extractor, m.start(0) + Vec2::new(-30.0 + 8.0 * k as f32, 30.0), true);
    }
    m.tick();
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let trickle = data.building(extractor).trickle_amounts.get(aether);
    assert!((passive_income(&view).get(aether) - 3.0 * trickle).abs() < 1e-4);
}

// ---------------------------------------------------------------------------
// Finishing games: razing a beaten town
// ---------------------------------------------------------------------------

/// Player 0 (steppe) stands at player 1's barracks with `rams` battering
/// rams and `riders` outriders, attacking it since 300 s (now 360 s); player
/// 1 has a worker at the barracks. Returns the barracks as known, the rams,
/// the riders and the worker.
fn raze_scene(m: &mut Match, rams: usize, riders: usize) -> (KnownBuilding, Vec<Entity>, Vec<Entity>, Entity) {
    let barracks = m.building_id("barracks");
    let at = m.start(1);
    let entity = m.building(1, barracks, at, true);
    let known = KnownBuilding { entity, owner: m.p(1), kind: barracks, pos: at, facing: 0.0 };
    let rams = m.army(0, "battering_ram", at + Vec2::new(-12.0, -6.0), rams);
    let riders = m.army(0, "steppe_outrider", at + Vec2::new(-16.0, 8.0), riders);
    let worker = m.workers(1, 1, at + Vec2::new(14.0, 3.0))[0];
    m.tick();
    m.set_time(360.0);
    (known, rams, riders, worker)
}

/// Memory of player 0 attacking `known` with `group` since 300 s.
fn raze_memory(known: KnownBuilding, group: &[Entity], razing: bool) -> AiMemory {
    let goal = Objective { player: known.owner, aim: Aim::Building(known.entity), pos: known.pos };
    let mut mem = attacking(group, goal, 300.0);
    mem.attack = AttackState::Attacking { goal, since: 300.0, best_dist: 5.0, progress_at: 350.0, razing };
    mem.wave_pop = 18;
    mem.see(known);
    mem
}

/// The units of the first `kind` command, sorted.
fn ordered(cmds: &[PlayerCommand], kind: impl Fn(&CommandKind) -> bool) -> Option<Vec<Entity>> {
    cmds.iter().find(|c| kind(&c.kind)).map(|c| sorted(&c.entities))
}

/// A wave that beat the defenders tears the town down: the breakers strike
/// the goal, the others hunt the workers in sight.
#[test]
fn a_wave_in_a_beaten_town_razes_it() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let (known, rams, riders, worker) = raze_scene(&mut m, 2, 6);
    let worker_at = m.app.world().get::<Position>(worker).unwrap().0;
    let group: Vec<Entity> = rams.iter().chain(&riders).copied().collect();
    let mut mem = raze_memory(known, &group, false);
    let cmds = m.think(0, &mut mem);
    assert!(mem.attack.razing(), "nobody defends: razing: {:?}", mem.attack);
    assert_eq!(ordered(&cmds, |k| *k == CommandKind::Attack(known.entity)), Some(sorted(&rams)), "the rams strike: {cmds:?}");
    assert_eq!(ordered(&cmds, |k| matches!(k, CommandKind::AttackMove(p) if p.distance(worker_at) < 0.5)), Some(sorted(&riders)), "the riders hunt the worker: {cmds:?}");
    assert_eq!(mem.raze_focus, Some(known.entity));
    assert_eq!(mem.raze_victim, Some(m.p(1)));
    assert!(mem.razing_lately(361.0) && mem.finishing(m.p(1), 361.0));
    // Sent once per building: the next think leaves the rams at it.
    m.set_time(361.0);
    let cmds = m.think(0, &mut mem);
    assert!(!cmds.iter().any(|c| c.kind == CommandKind::Attack(known.entity)), "not re-sent: {cmds:?}");
}

/// With no worker in sight, the soldiers that cannot tear the building
/// down wait beside it, on their side, out of the rams' way.
#[test]
fn escorts_wait_beside_the_breakers() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let (known, rams, riders, worker) = raze_scene(&mut m, 2, 6);
    m.app.world_mut().despawn(worker);
    m.tick();
    let group: Vec<Entity> = rams.iter().chain(&riders).copied().collect();
    let mut mem = raze_memory(known, &group, false);
    let cmds = m.think(0, &mut mem);
    let spot = cmds
        .iter()
        .find_map(|c| match c.kind {
            CommandKind::AttackMove(p) if sorted(&c.entities) == sorted(&riders) => Some(p),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the riders move: {cmds:?}"));
    let size = m.data.building(known.kind).size;
    let reach = Vec2::new(size[0], size[2]).length() * 0.5;
    let off = spot.distance(known.pos);
    assert!(off > reach + 1.0 && off < reach + 10.0, "beside the barracks, not at it: {off} ({reach})");
    assert!(spot.x < known.pos.x, "on the riders' side: {spot} vs {}", known.pos);
}

/// Razing does not start against a defending army, and stops when one shows
/// up: everyone turns on it.
#[test]
fn razing_waits_for_the_defenders_to_be_beaten() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let (known, rams, riders, _) = raze_scene(&mut m, 2, 6);
    m.army(1, "spearman", known.pos + Vec2::new(-6.0, 0.0), 10);
    m.tick();
    let group: Vec<Entity> = rams.iter().chain(&riders).copied().collect();
    let mut mem = raze_memory(known, &group, false);
    m.think(0, &mut mem);
    assert!(!mem.attack.razing(), "ten spearmen hold the town: {:?}", mem.attack);
    let mut mem = raze_memory(known, &group, true);
    mem.raze_focus = Some(known.entity);
    let cmds = m.think(0, &mut mem);
    assert!(!mem.attack.razing() && mem.raze_focus.is_none(), "{:?}", mem.attack);
    assert_eq!(ordered(&cmds, |k| *k == CommandKind::AttackMove(known.pos)), Some(sorted(&group)), "all fight: {cmds:?}");
}

/// A wave whittled down to a ram goes on razing a town nobody defends,
/// instead of retreating as spent.
#[test]
fn a_spent_wave_goes_on_razing_an_undefended_town() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let (known, rams, _, _) = raze_scene(&mut m, 1, 0);
    let mut mem = raze_memory(known, &rams, true);
    mem.wave_pop = 40;
    m.think(0, &mut mem);
    assert!(mem.attack.razing(), "still razing: {:?}", mem.attack);
    assert_eq!(mem.attackers, sorted(&rams));
}

/// When the goal falls, razing goes on at the town's next building,
/// production first, without walking everyone over.
#[test]
fn razing_moves_on_to_the_next_production_building() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let (known, rams, riders, _) = raze_scene(&mut m, 2, 4);
    let house = m.building_id("house");
    let near = m.building(1, house, known.pos + Vec2::new(-10.0, 10.0), true);
    let barracks = m.building_id("barracks");
    let next = m.building(1, barracks, known.pos + Vec2::new(-18.0, 16.0), true);
    m.tick();
    let group: Vec<Entity> = rams.iter().chain(&riders).copied().collect();
    let mut mem = raze_memory(known, &group, true);
    for (entity, kind, pos) in [(near, house, known.pos + Vec2::new(-10.0, 10.0)), (next, barracks, known.pos + Vec2::new(-18.0, 16.0))] {
        mem.see(KnownBuilding { entity, owner: m.p(1), kind, pos, facing: 0.0 });
    }
    // The goal fell.
    mem.known.retain(|k| k.entity != known.entity);
    let cmds = m.think(0, &mut mem);
    assert!(matches!(mem.attack, AttackState::Attacking { goal, razing: true, .. } if goal.building() == Some(next)), "{:?}", mem.attack);
    assert_eq!(ordered(&cmds, |k| *k == CommandKind::Attack(next)), Some(sorted(&rams)), "the rams go straight on: {cmds:?}");
}

/// Breakers that would crowd round one building spread over its
/// neighbours: a house takes as many rams as fit along its walls.
#[test]
fn breakers_spread_over_the_town() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let house = m.building_id("house");
    let barracks = m.building_id("barracks");
    let at = m.start(1);
    let goal = KnownBuilding { entity: m.building(1, house, at, true), owner: m.p(1), kind: house, pos: at, facing: 0.0 };
    let next_at = at + Vec2::new(14.0, 0.0);
    let next = KnownBuilding { entity: m.building(1, barracks, next_at, true), owner: m.p(1), kind: barracks, pos: next_at, facing: 0.0 };
    let rams = m.army(0, "battering_ram", at + Vec2::new(-14.0, -8.0), 8);
    m.tick();
    m.set_time(360.0);
    let mut mem = raze_memory(goal, &rams, true);
    mem.see(next);
    let cmds = m.think(0, &mut mem);
    let at_goal = ordered(&cmds, |k| *k == CommandKind::Attack(goal.entity)).unwrap_or_default();
    let at_next = ordered(&cmds, |k| *k == CommandKind::Attack(next.entity)).unwrap_or_default();
    let width = m.data.unit(m.unit_id("battering_ram")).size[0];
    let fit = (2.0 * (m.data.building(house).size[0] + m.data.building(house).size[2]) / (width + 1.2)).floor() as usize;
    assert_eq!((at_goal.len(), at_next.len()), (fit, rams.len() - fit), "{cmds:?}");
    assert!(at_goal.iter().all(|r| mem.raze_target(*r) == Some(goal.entity)));
}

/// Breakers are judged by the building damage they deal per population,
/// and chosen by what they deal per resource.
#[test]
fn breakers_are_chosen_from_the_weapon_data() {
    let data = test_data();
    let unit = |id: &str| data.unit_id(id).unwrap_or_else(|| panic!("no unit {id}"));
    let hall = data.building_id("town_center").unwrap();
    for id in ["battering_ram", "trebuchet", "bombard", "man_at_arms", "sylvan_elder_treant"] {
        assert!(is_breaker(&data, unit(id), hall), "{id}");
    }
    for id in ["archer", "crossbowman", "sylvan_longbowman", "horse_archer", "sylvan_hound_pack"] {
        assert!(!is_breaker(&data, unit(id), hall), "{id}");
    }
    assert!(breach_rate(&data, unit("battering_ram"), hall) > 3.0 * breach_rate(&data, unit("man_at_arms"), hall));
    let f = data.faction_id("sylvan").unwrap();
    let mut p = PlayerState::new(PlayerId(0), "p".into(), f, [1.0; 3], 0, Controller::Ai, &data);
    p.age = 3;
    let candidates = [unit("battering_ram"), unit("trebuchet"), unit("mangonel"), unit("sylvan_longbowman"), unit("sylvan_great_bear")];
    assert_eq!(best_breaker(&data, &p, &candidates, &[hall]), Some(unit("battering_ram")), "two rams cost about a trebuchet");
    assert_eq!(best_breaker(&data, &p, &[unit("sylvan_longbowman")], &[hall]), None);
}

/// While razing, the mix asks for the best breaker, the more the less the
/// army fielded could tear the known buildings down.
#[test]
fn razing_calls_for_breakers_until_the_army_can_tear_the_town_down() {
    let data = test_data();
    let ram = data.unit_id("battering_ram").unwrap();
    let f = data.faction_id("sylvan").unwrap();
    let mut p = PlayerState::new(PlayerId(0), "p".into(), f, [1.0; 3], 0, Controller::Ai, &data);
    p.age = 2;
    let kinds = ["town_center", "barracks", "house"];
    let known: Vec<KnownBuilding> = kinds
        .iter()
        .enumerate()
        .map(|(i, id)| KnownBuilding {
            entity: Entity::from_raw_u32(200 + i as u32).unwrap(),
            owner: PlayerId(1),
            kind: data.building_id(id).unwrap(),
            pos: Vec2::ZERO,
            facing: 0.0,
        })
        .collect();
    let weight = |mix: &[(UnitTypeId, f32)]| mix.iter().find(|c| c.0 == ram).map_or(0.0, |c| c.1) / mix.iter().map(|c| c.1).sum::<f32>();
    let calm = army_mix(&data, &p, &EnemyIntel::default(), &known, false, &[]);
    assert_eq!(weight(&calm), 0.0, "three buildings known, no towers, not razing: no breakers called for: {calm:?}");
    let razing = army_mix(&data, &p, &EnemyIntel::default(), &known, true, &[]);
    let targets = siege_targets(&data, &known, true);
    let breakers: f32 = razing.iter().filter(|(u, _)| is_breaker_vs(&data, *u, &targets)).map(|c| c.1).sum::<f32>()
        / razing.iter().map(|c| c.1).sum::<f32>();
    assert!(breakers >= RAZE_SIEGE_SHARE + 0.1, "breakers {breakers}: {razing:?}");
    assert!(weight(&razing) > 0.3, "the ram: {razing:?}");
    // Rams enough in the field: only the base share.
    let mut fielded = vec![0u32; data.units.len()];
    fielded[ram.index()] = 3 * 20;
    assert_eq!(breaking_gap(&data, &fielded, &known, &targets), 0.0);
    assert_eq!(breaking_gap(&data, &[], &known, &targets), 1.0);
    let enough = army_mix(&data, &p, &EnemyIntel::default(), &known, true, &fielded);
    assert!(weight(&enough) < weight(&razing) && weight(&enough) > 0.0, "{enough:?}");
}

/// A realm with barracks and a siege workshop, researching the next age
/// (nothing saved), with what pays for a spearman but not quite for a ram.
fn hold_scene() -> Match {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    m.hall(1);
    for (id, at) in [("barracks", Vec2::new(-20.0, 15.0)), ("siege_workshop", Vec2::new(-20.0, -15.0))] {
        let kind = m.building_id(id);
        m.building(0, kind, m.start(0) + at, true);
    }
    let house = m.building_id("house");
    for k in 0..4 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -15.0 + k as f32 * 6.0), true);
    }
    m.workers(0, worker_target(2) as usize, m.start(0));
    let data = m.data.clone();
    let to_3 = data.age_up_tech(m.player(0).faction, 2).expect("an age-up");
    m.edit_player(0, |p| {
        p.age = 2;
        p.researching[to_3.index()] = true;
        p.resources = amounts(&data, &[("food", 2000.0), ("wood", 150.0), ("copper", 60.0)]);
    });
    m.tick();
    m
}

/// While razing, the breaker the mix wants is saved for: the wood the ram
/// lacks is not spent on spearmen.
#[test]
fn razing_saves_for_the_breakers() {
    let mut m = hold_scene();
    let data = m.data.clone();
    let trained = |cmds: &[PlayerCommand]| cmds.iter().filter(|c| matches!(c.kind, CommandKind::Train { unit, .. } if data.unit(unit).gatherer.is_none())).count();
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(trained(&cmds) > 0, "not razing: spearmen: {cmds:?}");
    let barracks = m.building_id("barracks");
    let known = KnownBuilding { entity: m.building(1, barracks, m.start(1), true), owner: m.p(1), kind: barracks, pos: m.start(1), facing: 0.0 };
    m.tick();
    let mut mem = AiMemory { last_razing: Some(0.0), raze_victim: Some(m.p(1)), ..default() };
    mem.see(known);
    let cmds = m.think(0, &mut mem);
    assert_eq!(trained(&cmds), 0, "razing: the wood waits for the ram: {cmds:?}");
    assert!(mem.shortfall.get(data.resource_id("wood").unwrap()) > 0.0, "and wood is asked for");
}

/// While razing, a missing workshop for the breakers comes first; with one
/// standing (or laid out), nothing is missing.
#[test]
fn razing_builds_the_breakers_workshop_first() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    m.hall(0);
    let (workshop, ram) = (m.building_id("siege_workshop"), m.unit_id("battering_ram"));
    let house = m.building_id("house");
    for k in 0..4 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -15.0 + k as f32 * 6.0), true);
    }
    m.workers(0, worker_target(2) as usize, m.start(0));
    let data = m.data.clone();
    let to_3 = data.age_up_tech(m.player(0).faction, 2).expect("an age-up");
    m.edit_player(0, |p| {
        p.age = 2;
        p.researching[to_3.index()] = true;
        p.resources = amounts(&data, &[("food", 600.0), ("wood", 600.0), ("stone", 200.0), ("copper", 200.0)]);
    });
    let barracks = m.building_id("barracks");
    let known = KnownBuilding { entity: m.building(1, barracks, m.start(1), true), owner: m.p(1), kind: barracks, pos: m.start(1), facing: 0.0 };
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert_eq!(placements(&cmds, workshop), 0, "not razing: the mix's main production first: {cmds:?}");
    let mut mem = AiMemory { last_razing: Some(0.0), raze_victim: Some(m.p(1)), ..default() };
    mem.see(known);
    let cmds = m.think(0, &mut mem);
    assert_eq!(placements(&cmds, workshop), 1, "razing: the rams' workshop: {cmds:?}");
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    assert_eq!(missing_trainer(&view, ram, &[]), Some(workshop));
    assert_eq!(missing_trainer(&view, ram, &mem.pending), None, "laid out: nothing missing");
}

/// The realm razed lately is finished off before a nearer, fresh one, and
/// the earlier waves at it do not push the next one away.
#[test]
fn the_realm_razed_lately_is_finished_first() {
    let mut m = Match::new(&["steppe", "ironhold", "dune"]);
    let home = m.start(0);
    let near = m.known(1, home + Vec2::new(50.0, 0.0));
    let far = m.known(2, home + Vec2::new(0.0, 80.0));
    m.tick();
    m.set_time(600.0);
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let mut mem = AiMemory::default();
    mem.see(near);
    mem.see(far);
    assert_eq!(choose_target(&view, &mem, home).unwrap().player, m.p(1), "the nearer one");
    mem.last_razing = Some(590.0);
    mem.raze_victim = Some(m.p(2));
    mem.target_history = vec![m.p(2); 3];
    assert_eq!(choose_target(&view, &mem, home).unwrap().player, m.p(2), "finish the razed one");
    mem.last_razing = Some(600.0 - RAZE_MEMORY - 1.0);
    assert_eq!(choose_target(&view, &mem, home).unwrap().player, m.p(1), "long ago: spread again");
}

/// Against the last enemy, an army well above what it was seen to field
/// goes with the smallest wave; not before the first wave, not without a
/// sighting, not in a free-for-all.
#[test]
fn an_advantage_against_the_last_enemy_is_pressed() {
    let wave = |factions: &[&str], seen: f32, last_wave: Option<f32>| {
        let mut m = Match::new(factions);
        m.hall(0);
        m.edit_player(0, |p| p.age = 2);
        m.army(0, "steppe_outrider", m.start(0) + Vec2::new(10.0, 10.0), 12);
        m.tick();
        m.set_time(2000.0);
        let mut mem = AiMemory { last_wave, ..default() };
        let mut sighting = vec![0.0; m.data.units.len()];
        sighting[m.unit_id("spearman").index()] = seen;
        mem.intel.observe(2000.0, &sighting);
        let snap = m.snapshot(0);
        let view = m.view(0, &snap);
        Plan::new(&view, &mem).wave_size
    };
    let aggression = test_data().faction(test_data().faction_id("steppe").unwrap()).ai.aggression;
    let floor = wave_floor(2, aggression);
    let full = wave_size(2000.0, 2000.0 - 1.0, 2, aggression, false);
    assert!(full > floor);
    assert_eq!(wave(&["steppe", "ironhold"], 8.0, Some(1990.0 - WAVE_INTERVAL)), floor, "24 pop against 8 seen: go");
    assert_eq!(wave(&["steppe", "ironhold"], 20.0, Some(1990.0 - WAVE_INTERVAL)), full, "not enough of an edge");
    assert_eq!(wave(&["steppe", "ironhold"], 0.0, Some(1990.0 - WAVE_INTERVAL)), full, "nothing seen");
    assert_eq!(wave(&["steppe", "ironhold", "dune"], 8.0, Some(1990.0 - WAVE_INTERVAL)), full, "another enemy is left");
}

/// Workers stop eating into the saving once it holds most of the age-up.
#[test]
fn workers_wait_for_a_nearly_complete_age_up() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let house = m.building_id("house");
    for k in 0..4 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -15.0 + k as f32 * 6.0), true);
    }
    let data = m.data.clone();
    m.workers(0, 20, m.start(0));
    let to_2 = data.age_up_tech(m.player(0).faction, 1).unwrap();
    let cost = data.tech(to_2).cost.amounts;
    let worker = m.data.faction(m.player(0).faction).worker_id;
    for (share, expect) in [(0.5, true), (0.9, false)] {
        m.edit_player(0, |p| p.resources = cost.scaled(share));
        m.tick();
        let cmds = m.think(0, &mut AiMemory::default());
        assert_eq!(!trains(&cmds, worker).is_empty(), expect, "saving at {share}: {cmds:?}");
    }
}

// ---------------------------------------------------------------------------
// The first army building
// ---------------------------------------------------------------------------

/// Army production buildings placed by `cmds` (town centres aside).
fn army_buildings_placed(m: &Match, cmds: &[PlayerCommand]) -> Vec<BuildingTypeId> {
    let faction = m.player(0).faction;
    cmds.iter()
        .filter_map(|c| match c.kind {
            CommandKind::PlaceBuilding { kind, .. } => Some(kind),
            _ => None,
        })
        .filter(|k| !m.data.building(*k).has_tag(Tag::TownCenter) && !m.data.trainable_at(*k, faction).is_empty())
        .collect()
}

/// Once the age-up is being researched the first army building is due,
/// however few workers took the realm there, and so it is from the second
/// age on whenever none stands: the cheapest building that trains the mix,
/// with its price held back from the farms that would otherwise take the
/// wood as it trickles in and keep it from ever being affordable.
#[test]
fn the_first_army_building_is_saved_for_before_farms() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let house = m.building_id("house");
    for k in 0..3 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -10.0 + k as f32 * 6.0), true);
    }
    // Twelve workers: too few for an army building while saving for the age-up.
    m.workers(0, 12, m.start(0));
    let data = m.data.clone();
    let (farm, wood) = (m.building_id("farm"), data.resource_id("wood").unwrap());
    let to_2 = data.age_up_tech(m.player(0).faction, 1).unwrap();
    let bank = |w: f32| amounts(&data, &[("food", 200.0), ("wood", w)]);
    // Saving for the age-up: no army building yet, and the wood the saving
    // can spare buys a farm.
    m.edit_player(0, |p| p.resources = bank(140.0));
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert_eq!(placements(&cmds, farm), 1, "{cmds:?}");
    assert!(army_buildings_placed(&m, &cmds).is_empty(), "{cmds:?}");
    // The age-up under way: the wood waits for the army building ...
    m.edit_player(0, |p| p.researching[to_2.index()] = true);
    m.tick();
    let mut mem = AiMemory::default();
    let cmds = m.think(0, &mut mem);
    assert_eq!(placements(&cmds, farm), 0, "the wood is held for the army building: {cmds:?}");
    assert!(army_buildings_placed(&m, &cmds).is_empty(), "not affordable yet: {cmds:?}");
    assert!(mem.shortfall.get(wood) > 0.0, "and wood is asked for");
    // ... which goes up as soon as it is affordable: the cheapest that trains the mix.
    m.edit_player(0, |p| p.resources = bank(200.0));
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    let placed = army_buildings_placed(&m, &cmds);
    assert_eq!(placed.len(), 1, "{cmds:?}");
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let comp = Plan::new(&view, &AiMemory::default()).comp;
    let cheapest = production_targets(&view, &comp).iter().map(|t| cost_total(&data, view.me, t.0)).fold(f32::INFINITY, f32::min);
    assert_eq!(cost_total(&data, view.me, placed[0]), cheapest, "the cheapest first");
    assert_eq!(first_production_building(&view, &comp), Some(placed[0]));
    // From the second age on it is due whenever none stands (the forge the
    // next age-up needs stands, so that only the army building waits).
    m.edit_player(0, |p| {
        p.researching[to_2.index()] = false;
        p.age = 2;
        p.resources = bank(140.0);
    });
    let forge = m.building_id("forge");
    m.building(0, forge, m.start(0) + Vec2::new(-22.0, -15.0), true);
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert_eq!(placements(&cmds, farm), 0, "second age: the wood waits for the army building: {cmds:?}");
    // With one standing, the farm is bought again.
    m.building(0, placed[0], m.start(0) + Vec2::new(-22.0, 15.0), true);
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert_eq!(placements(&cmds, farm), 1, "an army building stands: {cmds:?}");
}

/// The building the next age-up needs has its price held back while it
/// cannot be paid for: the farms that may borrow half of the saving would
/// otherwise take the wood as it trickles in and keep it from ever being
/// affordable.
#[test]
fn the_age_ups_building_is_saved_for_before_farms() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let house = m.building_id("house");
    for k in 0..3 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -10.0 + k as f32 * 6.0), true);
    }
    let barracks = m.building_id("barracks");
    m.building(0, barracks, m.start(0) + Vec2::new(-22.0, 15.0), true);
    m.workers(0, 12, m.start(0));
    let data = m.data.clone();
    let (farm, forge, wood) = (m.building_id("farm"), m.building_id("forge"), data.resource_id("wood").unwrap());
    let bank = |w: f32| amounts(&data, &[("food", 200.0), ("wood", w), ("stone", 50.0), ("copper", 50.0)]);
    m.edit_player(0, |p| {
        p.age = 2;
        p.resources = bank(140.0);
    });
    m.tick();
    assert_eq!(m.age_up_step(0), AgeUpStep::Build(forge));
    let mut mem = AiMemory::default();
    let cmds = m.think(0, &mut mem);
    assert_eq!(placements(&cmds, farm), 0, "the wood waits for the forge: {cmds:?}");
    assert_eq!(placements(&cmds, forge), 0, "not affordable yet: {cmds:?}");
    assert!(mem.shortfall.get(wood) > 0.0, "and wood is asked for");
    m.edit_player(0, |p| p.resources = bank(150.0));
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert_eq!(placements(&cmds, forge), 1, "affordable: it goes up: {cmds:?}");
}

/// A realm in the second age that has lost every army building raises one
/// first, even while the building its next age-up needs (the forge) cannot
/// be paid for: that building's price is not held back then, or the wood
/// would idle while the realm waits for stone it is not gathering.
#[test]
fn a_lost_army_building_is_rebuilt_before_the_age_ups_building() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let house = m.building_id("house");
    for k in 0..3 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -10.0 + k as f32 * 6.0), true);
    }
    m.workers(0, 12, m.start(0));
    let data = m.data.clone();
    let forge = m.building_id("forge");
    // Wood enough for an army building, no stone or copper for the forge.
    m.edit_player(0, |p| {
        p.age = 2;
        p.resources = amounts(&data, &[("food", 200.0), ("wood", 200.0)]);
    });
    m.tick();
    assert_eq!(m.age_up_step(0), AgeUpStep::Build(forge));
    let cmds = m.think(0, &mut AiMemory::default());
    assert_eq!(placements(&cmds, forge), 0, "the forge is not affordable: {cmds:?}");
    assert_eq!(army_buildings_placed(&m, &cmds).len(), 1, "an army building goes up: {cmds:?}");
}

/// While the army is short, a production building the mix trains nothing
/// at (an archery range raised for the first age's stand-ins) trains what
/// it can rather than standing idle; one whose mix unit cannot be paid for
/// (an iron unit with no iron) waits for it; at its goal the army buys
/// nothing outside the mix.
#[test]
fn a_short_army_fills_in_where_the_mix_trains_nothing() {
    let mut m = Match::new(&["dune", "tide"]);
    m.hall(0);
    let (barracks, range) = (m.building_id("barracks"), m.building_id("archery_range"));
    let b = m.building(0, barracks, m.start(0) + Vec2::new(-22.0, 15.0), true);
    let r = m.building(0, range, m.start(0) + Vec2::new(-22.0, -15.0), true);
    let house = m.building_id("house");
    for k in 0..6 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -15.0 + k as f32 * 6.0), true);
    }
    m.workers(0, worker_target(2) as usize, m.start(0));
    let data = m.data.clone();
    let to_3 = data.age_up_tech(m.player(0).faction, 2).expect("an age-up");
    // Researching the next age (nothing saved), with no iron.
    m.edit_player(0, |p| {
        p.age = 2;
        p.researching[to_3.index()] = true;
        p.resources = amounts(&data, &[("food", 400.0), ("wood", 400.0)]);
    });
    m.tick();
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let plan = Plan::new(&view, &AiMemory::default());
    let at_barracks = wanted_unit(&view, &plan.comp, &plan.pop_by_kind, barracks, 10).expect("the mix trains at the barracks").0;
    assert!(stats::unit_cost(&data, view.me, at_barracks).get(data.resource_id("iron").unwrap()) > 0.0, "an iron unit");
    assert_eq!(wanted_unit(&view, &plan.comp, &plan.pop_by_kind, range, 10), None, "nothing of the mix at the range");
    let comp: Vec<UnitTypeId> = plan.comp.iter().map(|c| c.0).collect();
    let trained_at = |cmds: &[PlayerCommand], at: Entity| {
        cmds.iter().find_map(|c| match c.kind {
            CommandKind::Train { unit, .. } if c.entities == [at] => Some(unit),
            _ => None,
        })
    };
    let mut mem = AiMemory::default();
    let cmds = m.think(0, &mut mem);
    let u = trained_at(&cmds, r).unwrap_or_else(|| panic!("a filler at the range: {cmds:?}"));
    assert!(!comp.contains(&u) && data.unit(u).gatherer.is_none(), "{u:?}");
    assert_eq!(trained_at(&cmds, b), None, "the barracks waits for its iron unit: {cmds:?}");
    assert!(mem.shortfall.get(data.resource_id("iron").unwrap()) > 0.0, "and iron is asked for");
    // At its goal, the army buys nothing outside the mix.
    m.army(0, "spearman", m.start(0) + Vec2::new(-10.0, 25.0), plan.army_goal as usize + 1);
    m.tick();
    let cmds = m.think(0, &mut AiMemory::default());
    assert!(trained_at(&cmds, b).is_none() && trained_at(&cmds, r).is_none(), "{cmds:?}");
}

/// While razing the town of the last enemy, the army spends the whole
/// saving only as long as the age-up keeps to its schedule; behind it the
/// saving is kept and the next age comes, unless that enemy has lost every
/// production building (it is all but dead).
#[test]
fn razing_spends_the_saving_only_on_schedule_or_on_the_beaten() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    m.hall(1);
    for (id, at) in [("barracks", Vec2::new(-22.0, 15.0)), ("forge", Vec2::new(-22.0, -15.0))] {
        let kind = m.building_id(id);
        m.building(0, kind, m.start(0) + at, true);
    }
    let house = m.building_id("house");
    for k in 0..6 {
        m.building(0, house, m.start(0) + Vec2::new(25.0, -15.0 + k as f32 * 6.0), true);
    }
    m.workers(0, worker_target(2) as usize, m.start(0));
    // Above the army's goal, so that only all-in training touches the saving.
    m.army(0, "spearman", m.start(0) + Vec2::new(-10.0, 25.0), 14);
    let data = m.data.clone();
    // Everything the next age-up needs but food, all of which is saving.
    m.edit_player(0, |p| {
        p.age = 2;
        p.resources = amounts(&data, &[("food", 500.0), ("wood", 600.0), ("copper", 300.0), ("aether", 100.0)]);
    });
    let barracks = m.building_id("barracks");
    let known = KnownBuilding { entity: m.building(1, barracks, m.start(1), true), owner: m.p(1), kind: barracks, pos: m.start(1), facing: 0.0 };
    m.tick();
    let trained = |cmds: &[PlayerCommand]| cmds.iter().any(|c| matches!(c.kind, CommandKind::Train { unit, .. } if data.unit(unit).gatherer.is_none()));
    let victim = m.p(1);
    let razing = |behind: bool| {
        let mut mem = AiMemory { last_razing: Some(0.0), raze_victim: Some(victim), ..default() };
        mem.see(known);
        if behind {
            mem.age_seen = Some((2, -AGE_SCHEDULE[1] - 1.0));
        }
        mem
    };
    let cmds = m.think(0, &mut razing(false));
    assert!(trained(&cmds), "on schedule: the saving goes into finishing the town: {cmds:?}");
    let cmds = m.think(0, &mut razing(true));
    assert!(!trained(&cmds), "behind schedule: the saving waits for the next age: {cmds:?}");
    m.app.insert_resource(RevealedPlayers { list: vec![m.p(1)] });
    let cmds = m.think(0, &mut razing(true));
    assert!(trained(&cmds), "the enemy lost every production building: finish it: {cmds:?}");
}

// ---------------------------------------------------------------------------
// Finishing a realm with nothing left but units
// ---------------------------------------------------------------------------

/// A player with no building left that keeps it in the game is hunted down
/// through its units, wherever they hide (the AI's one cheat, see
/// `AiMemory::remnants`), rather than searched for spot by spot; a known
/// building still comes first.
#[test]
fn a_realm_with_nothing_left_but_units_is_hunted_through_them() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let home = m.start(0);
    fog_explored_round(&mut m, home, 40.0);
    let hideout = Vec2::new(MAP - 12.0, MAP - 12.0);
    let prey = m.unit(1, m.unit_id("peasant"), hideout);
    let house = m.known(1, m.start(1) + Vec2::new(-40.0, 20.0));
    m.tick();
    let snap = m.snapshot(0);
    let view = m.view(0, &snap);
    let mut mem = AiMemory::default();
    mem.visited.push(m.p(1));
    assert_eq!(choose_target(&view, &mem, home).map(|g| g.aim), Some(Aim::Search), "blind: search");
    mem.remnants = vec![(m.p(1), prey, hideout)];
    let goal = choose_target(&view, &mem, home).expect("a goal");
    assert_eq!((goal.player, goal.aim, goal.pos), (m.p(1), Aim::Remnant(prey), hideout), "straight for the hidden unit");
    mem.visited.clear();
    assert_eq!(choose_target(&view, &mem, home).map(|g| g.aim), Some(Aim::Remnant(prey)), "no detour by its empty start");
    mem.see(house);
    assert_eq!(choose_target(&view, &mem, home).map(|g| g.aim), Some(Aim::Building(house.entity)), "a known building first");
}

/// A wave after a remnant unit follows it wherever it runs, and turns on
/// the next one once it is dead.
#[test]
fn a_remnant_hunt_follows_its_prey_and_moves_on() {
    let mut m = Match::new(&["steppe", "ironhold"]);
    let home = m.start(0);
    fog_explored_round(&mut m, home, 40.0);
    let peasant = m.unit_id("peasant");
    let (first_at, second_at) = (m.start(1) + Vec2::new(-30.0, 20.0), m.start(1) + Vec2::new(20.0, -30.0));
    let (prey, next) = (m.unit(1, peasant, first_at), m.unit(1, peasant, second_at));
    let group = m.army(0, "steppe_outrider", home + Vec2::new(10.0, 10.0), 6);
    m.tick();
    m.set_time(900.0);
    let goal = Objective { player: m.p(1), aim: Aim::Remnant(prey), pos: first_at };
    let hunting = |since: f32| {
        let mut mem = attacking(&group, goal, since);
        mem.wave_pop = 12;
        mem
    };
    let fled = first_at + Vec2::new(0.0, -25.0);
    let mut remnants = vec![(m.p(1), prey, fled), (m.p(1), next, second_at)];
    remnants.sort_by_key(|r| r.1);
    let mut mem = hunting(890.0);
    mem.remnants = remnants.clone();
    let cmds = m.think(0, &mut mem);
    let AttackState::Attacking { goal: now_at, .. } = mem.attack else { panic!("still hunting: {cmds:?}") };
    assert_eq!((now_at.aim, now_at.pos), (Aim::Remnant(prey), fled), "after the prey: {cmds:?}");
    assert_eq!(attack_moved_to(&cmds, fled), Some(sorted(&group)), "{cmds:?}");
    // The prey is dead: on to the next.
    mem.remnants = vec![(m.p(1), next, second_at)];
    m.set_time(901.0);
    let cmds = m.think(0, &mut mem);
    let AttackState::Attacking { goal: now_at, .. } = mem.attack else { panic!("still hunting: {cmds:?}") };
    assert_eq!(now_at.aim, Aim::Remnant(next), "{cmds:?}");
    assert!(attack_moved_to(&cmds, second_at).is_some(), "{cmds:?}");
    // A prey the group has not closed in on for a minute is given up for the others.
    let mut mem = hunting(840.0);
    mem.remnants = remnants;
    m.set_time(900.0);
    m.think(0, &mut mem);
    assert!(mem.lost_prey.iter().any(|l| l.0 == prey), "{:?}", mem.lost_prey);
    let AttackState::Attacking { goal: now_at, .. } = mem.attack else { panic!("still hunting") };
    assert_eq!(now_at.aim, Aim::Remnant(next));
}

/// Remnants are tracked only for a hostile realm that lost every production
/// building (revealed) and has no building known that keeps it in the game.
#[test]
fn remnants_are_tracked_only_for_realms_with_nothing_left() {
    use af_core::ext::fog::{SeenBy, team_bit};
    let mut m = Match::new(&["steppe", "ironhold"]).with_ai_plugin();
    m.edit_player(0, |p| p.controller = Controller::Ai);
    m.hall(0);
    m.app.insert_resource(af_core::ext::fog::fog_for_map(Vec2::splat(MAP), 2.0, 2, true));
    let hideout = Vec2::new(MAP - 12.0, MAP - 12.0);
    let prey = m.unit(1, m.unit_id("peasant"), hideout);
    let remnants = |m: &mut Match| {
        for _ in 0..THINK_INTERVAL_TICKS {
            m.tick();
        }
        m.app.world().resource::<AiBrains>().memory(m.p(0)).expect("thought").remnants.clone()
    };
    assert!(remnants(&mut m).is_empty(), "not revealed: it may still have buildings in the fog");
    m.app.insert_resource(RevealedPlayers { list: vec![m.p(1)] });
    let found = remnants(&mut m);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].0, found[0].1), (m.p(1), prey));
    assert!(found[0].2.distance(hideout) < 3.0, "where it hides: {found:?}");
    // A revealed house keeps it in the game: an ordinary target, no cheat.
    let house = m.building_id("house");
    let b = m.building(1, house, m.start(1), true);
    let all = team_bit(0) | team_bit(1);
    m.app.world_mut().entity_mut(b).insert(SeenBy { teams: all, visible: all });
    assert!(remnants(&mut m).is_empty(), "a building that keeps it in the game is known");
}

// ---------------------------------------------------------------------------
// Big maps: regions, routes and the map's size
// ---------------------------------------------------------------------------

impl Match {
    /// Paint deep water over x in `x` the whole height of the map, crossed
    /// by `crossing` (a terrain id) over y in its range, and publish the
    /// regions as af_movement does.
    fn river(&mut self, x: std::ops::Range<i32>, crossing: Option<(&str, std::ops::Range<i32>)>) {
        let id = |n: &str| self.data.terrain_id(n).unwrap_or_else(|| panic!("no terrain {n}")).0;
        let deep = id("deep_water");
        let crossing = crossing.map(|(t, ys)| (id(t), ys));
        let world = self.app.world_mut();
        let mut nav = world.resource_mut::<Nav>();
        for y in 0..nav.height() as i32 {
            for xx in x.clone() {
                let t = match &crossing {
                    Some((t, ys)) if ys.contains(&y) => *t,
                    _ => deep,
                };
                nav.set_terrain(IVec2::new(xx, y), t);
            }
        }
        let regions = NavRegions::new(&nav);
        world.insert_resource(regions);
    }
}

/// Player 0 of two starts at (160, 100), the crater is at (100, 100): a
/// river between them at x = 125..131 with a ford far north (y 170..177)
/// bends the way to the middle, and the army's front faces along it.
#[test]
fn the_front_faces_along_the_way_to_the_middle() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    m.workers(0, 3, m.start(0));
    m.tick();
    let straight = {
        let snap = m.snapshot(0);
        Plan::new(&m.view(0, &snap), &AiMemory::default()).front
    };
    assert!(straight.distance(Vec2::new(-1.0, 0.0)) < 1e-3, "straight at the crater without regions: {straight}");
    m.river(125..131, Some(("ford", 170..177)));
    let snap = m.snapshot(0);
    let plan = Plan::new(&m.view(0, &snap), &AiMemory::default());
    assert!(plan.front.y > 0.5, "toward the ford, up the bank: {}", plan.front);
    assert!(plan.rally.x > 131.0, "the rally point stays on the home bank: {}", plan.rally);
}

/// A remote wood cluster across the river is passed over for a farther
/// one the workers can walk to.
#[test]
fn remote_resources_across_the_river_are_passed_over() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let across = Vec2::new(104.0, 100.0);
    let this_side = Vec2::new(160.0, 32.0);
    m.node("oak", across);
    m.node("oak", this_side);
    m.tick();
    let wood = m.data.resource_id("wood").expect("wood");
    let home = m.start(0);
    let snap = m.snapshot(0);
    assert_eq!(find_remote(&m.view(0, &snap), home, wood, &[]), Some(across), "the nearest without regions");
    m.river(125..131, None);
    let snap = m.snapshot(0);
    assert_eq!(find_remote(&m.view(0, &snap), home, wood, &[]), Some(this_side));
}

/// A building spot sought from the bank never lands across the water.
#[test]
fn build_spots_stay_on_the_workers_bank() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.river(125..131, None);
    m.tick();
    let house = m.building_id("house");
    let q = SpotQuery { kind: house, anchor: Vec2::new(137.0, 100.0), toward: Vec2::new(-1.0, 0.0), min_r: 12.0, max_r: 30.0, gap: 1.0 };
    let snap = m.snapshot(0);
    let spot = find_build_spot(&m.view(0, &snap), &q, &[], &[]).expect("a spot");
    assert!(spot.pos.x > 131.0, "on the home bank: {}", spot.pos);
}

/// Of two hostile buildings, the one across the river (nearer in a
/// straight line, far round by the ford) is not the first target: the way
/// round decides. Without regions the straight line does.
#[test]
fn attack_targets_are_ranked_by_the_way_round() {
    let mut m = Match::new(&["ironhold", "dune", "sylvan"]);
    m.hall(0);
    let home = m.start(0);
    let across = m.known(1, Vec2::new(105.0, 100.0));
    let this_side = m.known(2, Vec2::new(160.0, 30.0));
    m.tick();
    let mem = AiMemory { known: vec![across, this_side], ..default() };
    let snap = m.snapshot(0);
    assert_eq!(choose_target(&m.view(0, &snap), &mem, home).map(|o| o.pos), Some(across.pos), "straight without regions");
    m.river(125..131, Some(("ford", 185..192)));
    let snap = m.snapshot(0);
    assert_eq!(choose_target(&m.view(0, &snap), &mem, home).map(|o| o.pos), Some(this_side.pos));
}

/// An attack going round by the ford makes progress although it gets no
/// nearer in a straight line: it is not taken for stuck.
#[test]
fn an_attack_round_by_the_ford_is_progress() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    let goal = m.known(1, Vec2::new(110.0, 100.0));
    let group = m.army(0, "spearman", Vec2::new(138.0, 96.0), 6);
    m.river(125..131, Some(("ford", 170..177)));
    m.tick();
    let objective = Objective { player: m.p(1), aim: Aim::Building(goal.entity), pos: goal.pos };
    let mut mem = attacking(&group, objective, 0.0);
    mem.known.push(goal);
    mem.wave_pop = 6;
    m.set_time(1.0);
    m.think(0, &mut mem);
    let AttackState::Attacking { best_dist: first, .. } = mem.attack else { panic!("attacking: {:?}", mem.attack) };
    assert!(first > 120.0, "the way left is round by the ford: {first}");
    // Halfway up the bank to the ford: farther in a straight line, nearer by the way.
    for (k, &u) in group.iter().enumerate() {
        m.app.world_mut().entity_mut(u).insert(Position(Vec2::new(138.0 + (k % 3) as f32, 148.0 + (k / 3) as f32)));
    }
    m.tick();
    m.set_time(40.0);
    m.think(0, &mut mem);
    let AttackState::Attacking { best_dist, progress_at, .. } = mem.attack else { panic!("still attacking: {:?}", mem.attack) };
    assert!(best_dist < first - 20.0 && progress_at == 40.0, "progress: {best_dist} at {progress_at}");
    m.set_time(80.0);
    m.think(0, &mut mem);
    assert!(matches!(mem.attack, AttackState::Attacking { goal: g, .. } if g.pos == goal.pos), "not given up 40 s after its last progress");
    assert!(!mem.is_unreachable(goal.pos));
}

/// Siege on wheels is left out of the army mix while only a shallow
/// crossing leads from home to the middle; a ford brings it back.
#[test]
fn units_with_no_way_to_the_middle_are_left_out_of_the_mix() {
    let mut m = Match::new(&["ironhold", "dune"]);
    m.hall(0);
    m.tick();
    let (ram, spear) = (m.unit_id("battering_ram"), m.unit_id("spearman"));
    let comp = vec![(ram, 1.0), (spear, 2.0)];
    let home = m.start(0);
    m.river(125..131, Some(("shallow_water", 96..104)));
    let snap = m.snapshot(0);
    assert_eq!(crate::plan::reachable_mix(&m.view(0, &snap), home, comp.clone()), vec![(spear, 2.0)]);
    m.river(125..131, Some(("ford", 96..104)));
    let snap = m.snapshot(0);
    assert_eq!(crate::plan::reachable_mix(&m.view(0, &snap), home, comp.clone()), comp);
}

/// Builders are picked among the workers that can walk to the site.
#[test]
fn builders_come_from_the_sites_bank() {
    let mut m = Match::new(&["ironhold", "dune"]);
    let kind = m.data.faction(m.player(0).faction).worker_id;
    let near_across = m.unit(0, kind, Vec2::new(122.0, 100.0));
    let far_this_side = m.unit(0, kind, Vec2::new(160.0, 130.0));
    m.river(125..131, None);
    m.tick();
    let snap = m.snapshot(0);
    let mut crew = Vec::new();
    pick_workers(&m.view(0, &snap), Vec2::new(140.0, 100.0), 1, &[], &mut crew);
    assert_eq!(crew, vec![far_this_side], "not {near_across}");
}

/// The military radii grow with the map beyond the size they were tuned for.
#[test]
fn the_map_scale_grows_with_big_maps() {
    let m = Match::new(&["ironhold", "dune"]);
    let snap = Snapshot::default();
    assert_eq!(m.view(0, &snap).map_scale(), 1.0);
    let mut big = sim_app(test_data());
    let players = flat_world(&mut big, 768.0, &[("ironhold", 0), ("dune", 1)]);
    let w = big.world();
    let view = AiView::new(
        &w.resource::<Data>().0,
        w.resource::<Players>(),
        players[0],
        &w.resource::<Nav>().0,
        w.resource::<SpatialIndex>(),
        w.get_resource::<MapInfo>(),
        0.0,
        w.resource::<FogOfWar>(),
        &snap,
    )
    .expect("player");
    assert_eq!(view.map_scale(), 2.0);
}
