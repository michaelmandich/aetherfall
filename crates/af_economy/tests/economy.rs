//! Headless economy tests against the real content.

use std::sync::Arc;

use af_core::ext::economy::{EconomyIntake, RevealedPlayers};
use af_core::prelude::*;
use af_core::spawn::SpawnStats;
use af_core::testing::{command, flat_world, run_ticks, sim_app, test_data, with_commands};
use af_economy::EconomyPlugin;
use bevy::prelude::*;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

/// Messages seen by a probe system running after the economy.
#[derive(Resource, Default)]
struct Seen {
    placed: u32,
    spawned: u32,
    completed: Vec<BuildingTypeId>,
    techs: Vec<TechId>,
    ages: Vec<u8>,
}

fn record_messages(
    mut seen: ResMut<Seen>,
    mut placed: MessageReader<BuildingPlaced>,
    mut spawned: MessageReader<UnitSpawned>,
    mut completed: MessageReader<BuildingCompleted>,
    mut techs: MessageReader<TechResearched>,
    mut ages: MessageReader<AgeAdvanced>,
) {
    seen.placed += placed.read().count() as u32;
    seen.spawned += spawned.read().count() as u32;
    seen.completed.extend(completed.read().map(|m| m.kind));
    seen.techs.extend(techs.read().map(|m| m.tech));
    seen.ages.extend(ages.read().map(|m| m.age));
}

fn new_match(players: &[(&str, u8)]) -> App {
    let mut app = sim_app(test_data());
    app.add_plugins(EconomyPlugin)
        .init_resource::<Seen>()
        .add_systems(FixedUpdate, record_messages.in_set(SimSet::Vision));
    flat_world(&mut app, 128.0, players);
    app
}

fn data(app: &App) -> Arc<GameData> {
    app.world().resource::<Data>().0.clone()
}

fn unit_id(app: &App, id: &str) -> UnitTypeId {
    data(app).unit_id(id).unwrap_or_else(|| panic!("unit {id}"))
}

fn building_id(app: &App, id: &str) -> BuildingTypeId {
    data(app).building_id(id).unwrap_or_else(|| panic!("building {id}"))
}

fn tech_id(app: &App, id: &str) -> TechId {
    data(app).tech_id(id).unwrap_or_else(|| panic!("tech {id}"))
}

fn player(app: &App, p: PlayerId) -> &PlayerState {
    app.world().resource::<Players>().get(p)
}

fn edit_player(app: &mut App, p: PlayerId, f: impl FnOnce(&mut PlayerState)) {
    f(app.world_mut().resource_mut::<Players>().get_mut(p));
}

fn res(app: &App, p: PlayerId, r: &str) -> f32 {
    player(app, p).resources.get(data(app).resource_id(r).unwrap())
}

/// Plenty of everything.
fn make_rich(app: &mut App, p: PlayerId) {
    edit_player(app, p, |s| s.resources = ResAmounts([5000.0; MAX_RESOURCES]));
}

fn spawn_building_at(app: &mut App, p: PlayerId, id: &str, pos: Vec2, complete: bool) -> Entity {
    let kind = building_id(app, id);
    with_commands(app, |c, d, ps| spawn_building(c, d, ps.get(p), kind, pos, 0.0, complete))
}

fn spawn_unit_at(app: &mut App, p: PlayerId, id: &str, pos: Vec2) -> Entity {
    let kind = unit_id(app, id);
    with_commands(app, |c, d, ps| spawn_unit(c, d, ps.get(p), kind, pos, 0.0))
}

fn orders(app: &App, e: Entity) -> Vec<Order> {
    app.world().get::<Orders>(e).map(|o| o.queue.iter().copied().collect()).unwrap_or_default()
}

/// Where a unit walks with no order (a fresh unit heading to its rally).
fn walk_target(app: &App, e: Entity) -> Option<Vec2> {
    app.world().get::<MoveGoal>(e).filter(|g| g.follow.is_none()).map(|g| g.target)
}

/// Stands in for af_behavior's unit command intake, which this crate does
/// not depend on. Like it, it runs after the economy's pass and applies the
/// tick's `Move`s (replacing the orders, or shift-queued after them) and
/// `Delete`s in inbox order.
fn unit_intake_stub(
    tick: Res<TickCommands>,
    mut commands: Commands,
    mut units: Query<(&Owner, &mut Orders), Without<Dead>>,
    owners: Query<&Owner, Without<Dead>>,
) {
    for cmd in &tick.list {
        for &e in &cmd.entities {
            match cmd.kind {
                CommandKind::Move(target) => {
                    if let Ok((owner, mut orders)) = units.get_mut(e)
                        && owner.0 == cmd.player
                    {
                        orders.set(Order::Move { target }, cmd.queue);
                    }
                }
                CommandKind::Delete if owners.get(e).is_ok_and(|o| o.0 == cmd.player) => {
                    commands.entity(e).insert(Dead);
                }
                _ => {}
            }
        }
    }
}

fn with_unit_intake(mut app: App) -> App {
    app.add_systems(FixedUpdate, unit_intake_stub.in_set(SimSet::Commands).after(EconomyIntake));
    app
}

fn units_of(app: &mut App, kind: UnitTypeId) -> Vec<Entity> {
    let world = app.world_mut();
    let mut q = world.query::<(Entity, &Unit)>();
    q.iter(world).filter(|(_, u)| u.kind == kind).map(|(e, _)| e).collect()
}

fn buildings_of(app: &mut App, kind: BuildingTypeId) -> Vec<Entity> {
    let world = app.world_mut();
    let mut q = world.query::<(Entity, &Building)>();
    q.iter(world).filter(|(_, b)| b.kind == kind).map(|(e, _)| e).collect()
}

fn queue(app: &App, e: Entity) -> ProductionQueue {
    app.world().get::<ProductionQueue>(e).cloned().expect("building with a queue")
}

/// Notices addressed to `p` (or to everyone), as (kind, text).
fn notices(app: &App, p: PlayerId) -> Vec<(NoticeKind, String)> {
    let list = &app.world().resource::<Notices>().list;
    list.iter().filter(|n| n.player.is_none_or(|x| x == p)).map(|n| (n.kind, n.text.clone())).collect()
}

fn warned(app: &App, p: PlayerId, text: &str) -> bool {
    notices(app, p).iter().any(|(k, t)| *k == NoticeKind::Warning && t == text)
}

fn seconds(app: &mut App, s: f32) {
    run_ticks(app, (s * SIM_HZ as f32).round() as u32);
}

fn cmd(p: PlayerId, entities: Vec<Entity>, kind: CommandKind) -> PlayerCommand {
    PlayerCommand::new(p, entities, kind)
}

#[test]
fn placing_a_building_pays_and_sends_the_builders() {
    let mut app = new_match(&[("starborn", 0)]);
    let a = spawn_unit_at(&mut app, P0, "peasant", Vec2::new(40.0, 40.0));
    let b = spawn_unit_at(&mut app, P0, "peasant", Vec2::new(41.0, 40.0));
    let scout = spawn_unit_at(&mut app, P0, "scout_rider", Vec2::new(42.0, 40.0));
    let house = building_id(&app, "house");
    let wood = res(&app, P0, "wood");

    command(&mut app, cmd(P0, vec![a, b, scout], CommandKind::PlaceBuilding { kind: house, pos: Vec2::new(50.0, 50.0), facing: 0.3 }));
    run_ticks(&mut app, 1);

    let sites = buildings_of(&mut app, house);
    assert_eq!(sites.len(), 1);
    let site = sites[0];
    assert!(app.world().get::<UnderConstruction>(site).is_some(), "a foundation");
    assert_eq!(app.world().get::<Facing>(site).map(|f| f.0), Some(0.3));
    assert_eq!(res(&app, P0, "wood"), wood - data(&app).building(house).cost.amounts.get(data(&app).resource_id("wood").unwrap()));
    assert_eq!(orders(&app, a), vec![Order::Build { site }]);
    assert_eq!(orders(&app, b), vec![Order::Build { site }]);
    assert!(orders(&app, scout).is_empty(), "a scout cannot build");
    assert_eq!(app.world().resource::<Seen>().placed, 1);
}

#[test]
fn an_invalid_placement_charges_nothing_and_says_why() {
    let mut app = new_match(&[("starborn", 0)]);
    let peasant = spawn_unit_at(&mut app, P0, "peasant", Vec2::new(40.0, 40.0));
    spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 60.0), true);
    let house = building_id(&app, "house");
    let castle = building_id(&app, "castle");
    let before = player(&app, P0).resources;

    for (kind, pos) in [(house, Vec2::new(1.0, 1.0)), (house, Vec2::new(61.0, 60.0)), (castle, Vec2::new(30.0, 90.0))] {
        command(&mut app, cmd(P0, vec![peasant], CommandKind::PlaceBuilding { kind, pos, facing: 0.0 }));
    }
    run_ticks(&mut app, 1);

    assert_eq!(player(&app, P0).resources, before);
    assert!(buildings_of(&mut app, house).is_empty() && buildings_of(&mut app, castle).is_empty());
    assert!(orders(&app, peasant).is_empty());
    assert!(warned(&app, P0, "Outside the map"));
    assert!(warned(&app, P0, "Cannot build there"));
    let castle_age = data(&app).building(castle).age;
    assert!(warned(&app, P0, &Reason::Age(castle_age).describe(&data(&app))));
}

#[test]
fn training_spawns_the_unit_after_its_train_time_and_counts_its_pop() {
    let mut app = new_match(&[("starborn", 0)]);
    let tc = spawn_building_at(&mut app, P0, "town_center", Vec2::new(60.0, 60.0), true);
    let peasant = unit_id(&app, "peasant");
    let mut after_paying = player(&app, P0).resources;
    after_paying.sub_all(&stats::unit_cost(&data(&app), player(&app, P0), peasant));
    let train_ticks = (data(&app).unit(peasant).train_time * SIM_HZ as f32).round() as u32;

    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: peasant, count: 1 }));
    run_ticks(&mut app, 1);
    assert_eq!(player(&app, P0).resources, after_paying);
    assert_eq!(queue(&app, tc).items.len(), 1);

    run_ticks(&mut app, train_ticks - 10);
    assert!(units_of(&mut app, peasant).is_empty(), "not before its train time");
    run_ticks(&mut app, 10);
    let trained = units_of(&mut app, peasant);
    assert_eq!(trained.len(), 1);
    assert!(queue(&app, tc).items.is_empty());
    let pos = app.world().get::<Position>(trained[0]).unwrap().0;
    let tc_box = app.world().get::<Footprint>(tc).unwrap().obb(Vec2::new(60.0, 60.0), 0.0);
    assert!(tc_box.distance_to_point(pos) > 0.0, "spawned outside the town centre");

    run_ticks(&mut app, 1);
    let p = player(&app, P0);
    assert_eq!((p.pop_used, p.pop_cap), (1, data(&app).building(building_id(&app, "town_center")).pop));
    assert_eq!(p.stats.units_trained, 1);
    assert_eq!(app.world().resource::<Seen>().spawned, 1);
}

#[test]
fn the_population_cap_holds_training_until_there_is_room() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    let militia = unit_id(&app, "militia");
    let def = data(&app).unit(militia).clone();
    let house_pop = data(&app).building(building_id(&app, "house")).pop;
    assert!(def.squad > 1, "militia train as a squad");
    spawn_building_at(&mut app, P0, "house", Vec2::new(30.0, 30.0), true);
    // Leave room for exactly one squad.
    let settlers = house_pop - def.pop * def.squad - 1;
    for i in 0..settlers {
        spawn_unit_at(&mut app, P0, "peasant", Vec2::new(20.0 + i as f32, 50.0));
    }
    let east = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 60.0), true);
    let west = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 90.0), true);

    for b in [east, west] {
        command(&mut app, cmd(P0, vec![b], CommandKind::Train { unit: militia, count: 1 }));
    }
    run_ticks(&mut app, 1);
    assert!(!queue(&app, east).items.is_empty() && !queue(&app, west).items.is_empty(), "each squad fits on its own");
    seconds(&mut app, def.train_time + 1.0);

    // Both finished in the same tick; only the first fit.
    assert_eq!(units_of(&mut app, militia).len(), def.squad as usize);
    let blocked: Vec<bool> = [east, west].iter().map(|b| queue(&app, *b).pop_blocked).collect();
    assert_eq!(blocked.iter().filter(|b| **b).count(), 1, "{blocked:?}");
    assert!(warned(&app, P0, &Reason::PopCapped.describe(&data(&app))));
    let idle = if blocked[0] { west } else { east };
    command(&mut app, cmd(P0, vec![idle], CommandKind::Train { unit: militia, count: 1 }));
    run_ticks(&mut app, 1);
    assert!(queue(&app, idle).items.is_empty(), "no room left to queue another squad");

    spawn_building_at(&mut app, P0, "house", Vec2::new(30.0, 100.0), true);
    run_ticks(&mut app, 2);
    let army = units_of(&mut app, militia);
    assert_eq!(army.len(), 2 * def.squad as usize);
    assert!(!queue(&app, east).pop_blocked && !queue(&app, west).pop_blocked);
    let mut squads: Vec<u32> = army.iter().map(|e| app.world().get::<SquadMember>(*e).expect("squad member").squad).collect();
    squads.sort_unstable();
    squads.dedup();
    assert_eq!(squads.len(), 2, "one squad id per training");
}

#[test]
fn research_applies_a_stat_effect_to_existing_and_new_units() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    edit_player(&mut app, P0, |p| p.age = 2);
    spawn_building_at(&mut app, P0, "town_center", Vec2::new(30.0, 30.0), true);
    let barracks = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 60.0), true);
    let veteran = spawn_unit_at(&mut app, P0, "militia", Vec2::new(70.0, 70.0));
    let militia = unit_id(&app, "militia");
    let base_hp = data(&app).unit(militia).hp;
    app.world_mut().get_mut::<Health>(veteran).unwrap().hp = base_hp * 0.5;
    let drill = tech_id(&app, "militia_drill");

    command(&mut app, cmd(P0, vec![barracks], CommandKind::Research { tech: drill }));
    run_ticks(&mut app, 1);
    assert!(player(&app, P0).researching[drill.index()]);
    let wait = data(&app).tech(drill).time;
    seconds(&mut app, wait);

    let p = player(&app, P0);
    assert!(p.has_tech(drill) && !p.researching[drill.index()]);
    assert_eq!(p.stats.techs, 1);
    assert_eq!(app.world().resource::<Seen>().techs, vec![drill]);
    let boosted = stats::unit_stats(&data(&app), player(&app, P0), militia).max_hp;
    assert!(boosted > base_hp, "the drill raises militia hp");
    let h = *app.world().get::<Health>(veteran).unwrap();
    assert!((h.max - boosted).abs() < 1e-3 && (h.fraction() - 0.5).abs() < 1e-4, "{h:?}");
    assert!((app.world().get::<Stats>(veteran).unwrap().max_hp - boosted).abs() < 1e-3);

    command(&mut app, cmd(P0, vec![barracks], CommandKind::Train { unit: militia, count: 1 }));
    let wait = data(&app).unit(militia).train_time + 1.0;
    seconds(&mut app, wait);
    let recruits: Vec<Entity> = units_of(&mut app, militia).into_iter().filter(|e| *e != veteran).collect();
    assert!(!recruits.is_empty());
    for e in recruits {
        let h = *app.world().get::<Health>(e).unwrap();
        assert!((h.max - boosted).abs() < 1e-3 && (h.hp - boosted).abs() < 1e-3, "{h:?}");
    }
}

#[test]
fn a_line_upgrade_converts_units_in_the_field_and_in_queues() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    edit_player(&mut app, P0, |p| p.age = 2);
    spawn_building_at(&mut app, P0, "town_center", Vec2::new(30.0, 30.0), true);
    let academy = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 60.0), true);
    let drill_yard = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 90.0), true);
    let spearman = unit_id(&app, "spearman");
    let pikeman = unit_id(&app, "pikeman");
    let veteran = spawn_unit_at(&mut app, P0, "spearman", Vec2::new(80.0, 80.0));
    let spear_hp = data(&app).unit(spearman).hp;
    app.world_mut().get_mut::<Health>(veteran).unwrap().hp = spear_hp * 0.5;
    let pike_drill = tech_id(&app, "pike_drill");

    command(&mut app, cmd(P0, vec![academy], CommandKind::Research { tech: pike_drill }));
    command(&mut app, cmd(P0, vec![drill_yard], CommandKind::Train { unit: spearman, count: 3 }));
    let wait = data(&app).tech(pike_drill).time + 0.1;
    seconds(&mut app, wait);

    assert_eq!(app.world().get::<Unit>(veteran).unwrap().kind, pikeman);
    assert_eq!(app.world().get::<Name>(veteran).unwrap().as_str(), data(&app).unit(pikeman).name);
    let h = *app.world().get::<Health>(veteran).unwrap();
    assert!((h.max - data(&app).unit(pikeman).hp).abs() < 1e-3 && (h.fraction() - 0.5).abs() < 1e-4, "{h:?}");
    let queued = queue(&app, drill_yard);
    let pike_time = stats::unit_train_time(&data(&app), player(&app, P0), pikeman);
    assert_ne!(pike_time, stats::unit_train_time(&data(&app), player(&app, P0), spearman), "the content makes the times differ");
    let spear_cost = data(&app).unit(spearman).cost.amounts;
    assert!(!queued.items.is_empty());
    for item in &queued.items {
        assert_eq!(item.kind, QueueKind::Unit(pikeman));
        assert_eq!(item.time, pike_time, "queued trainings take the new unit's time");
        assert_eq!(item.paid, spear_cost, "and still refund what was paid");
    }

    command(&mut app, cmd(P0, vec![academy], CommandKind::Train { unit: spearman, count: 1 }));
    run_ticks(&mut app, 1);
    assert_eq!(queue(&app, academy).items.back().map(|i| i.kind), Some(QueueKind::Unit(pikeman)));
}

#[test]
fn an_age_up_advances_the_age_and_is_only_offered_in_its_age() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    let tc = spawn_building_at(&mut app, P0, "town_center", Vec2::new(60.0, 60.0), true);
    spawn_building_at(&mut app, P0, "forge", Vec2::new(80.0, 80.0), true);
    let to_banner = tech_id(&app, "to_banner");
    let to_starfall = tech_id(&app, "to_starfall");
    let rich = player(&app, P0).resources;

    command(&mut app, cmd(P0, vec![tc], CommandKind::Research { tech: to_starfall }));
    run_ticks(&mut app, 1);
    assert_eq!(player(&app, P0).resources, rich, "nothing charged");
    assert!(queue(&app, tc).items.is_empty());
    assert!(warned(&app, P0, &Reason::Age(2).describe(&data(&app))));

    command(&mut app, cmd(P0, vec![tc], CommandKind::Research { tech: to_banner }));
    command(&mut app, cmd(P0, vec![tc], CommandKind::Research { tech: to_banner }));
    run_ticks(&mut app, 1);
    assert_eq!(queue(&app, tc).items.len(), 1, "an age-up cannot be queued twice");
    assert!(warned(&app, P0, &Reason::InProgress.describe(&data(&app))));
    let wait = data(&app).tech(to_banner).time;
    seconds(&mut app, wait);

    assert_eq!(player(&app, P0).age, 2);
    assert_eq!(app.world().resource::<Seen>().ages, vec![2]);
    let banner = data(&app).age(2).name.clone();
    let announced = app.world().resource::<Notices>().list.iter().any(|n| n.player.is_none() && n.kind == NoticeKind::Info && n.text.contains(banner.as_str()));
    assert!(announced, "everyone hears of the new age");
    let town_center = building_id(&app, "town_center");
    assert_eq!(rules::can_research(&data(&app), player(&app, P0), town_center, to_banner, true), Err(Reason::AlreadyResearched));
    assert_eq!(rules::can_research(&data(&app), player(&app, P0), town_center, to_starfall, true), Ok(()));
}

#[test]
fn a_building_upgrade_changes_the_kind_and_keeps_the_hp_fraction() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    edit_player(&mut app, P0, |p| p.age = 2);
    let wall = spawn_building_at(&mut app, P0, "palisade_wall", Vec2::new(60.0, 60.0), true);
    let palisade = building_id(&app, "palisade_wall");
    let stone_wall = building_id(&app, "stone_wall");
    let half = data(&app).building(palisade).hp * 0.5;
    app.world_mut().get_mut::<Health>(wall).unwrap().hp = half;
    let stone = res(&app, P0, "stone");
    let up = data(&app).building(palisade).upgrade.clone().unwrap();

    command(&mut app, cmd(P0, vec![wall], CommandKind::UpgradeBuilding));
    command(&mut app, cmd(P0, vec![wall], CommandKind::UpgradeBuilding));
    run_ticks(&mut app, 1);
    assert_eq!(queue(&app, wall).items.len(), 1, "one upgrade at a time");
    assert_eq!(res(&app, P0, "stone"), stone - up.cost.amounts.get(data(&app).resource_id("stone").unwrap()));
    seconds(&mut app, up.time);

    assert_eq!(app.world().get::<Building>(wall).unwrap().kind, stone_wall);
    assert_eq!(app.world().get::<Name>(wall).unwrap().as_str(), data(&app).building(stone_wall).name);
    let h = *app.world().get::<Health>(wall).unwrap();
    assert_eq!(h.max, data(&app).building(stone_wall).hp);
    assert!((h.fraction() - 0.5).abs() < 1e-4, "{h:?}");
    assert_eq!(app.world().get::<Stats>(wall).unwrap().max_hp, h.max);
    assert!(app.world().get::<Wall>(wall).is_some());
    assert_eq!(app.world().resource::<Seen>().completed, vec![stone_wall]);
}

#[test]
fn a_house_upgrade_keeps_its_obstacle_and_raises_the_population_cap() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    edit_player(&mut app, P0, |p| p.age = 3);
    let home = spawn_building_at(&mut app, P0, "house", Vec2::new(60.0, 60.0), true);
    let manor = building_id(&app, "manor");
    run_ticks(&mut app, 1);
    let obstacle = app.world().get::<Obstacle>(home).unwrap().obb;

    command(&mut app, cmd(P0, vec![home], CommandKind::UpgradeBuilding));
    let time = data(&app).building(building_id(&app, "house")).upgrade.as_ref().unwrap().time;
    seconds(&mut app, time + 0.1);

    assert_eq!(app.world().get::<Building>(home).unwrap().kind, manor);
    assert_eq!(app.world().get::<Footprint>(home).unwrap().height, data(&app).building(manor).size[1]);
    assert_eq!(app.world().get::<Obstacle>(home).unwrap().obb, obstacle, "same footprint, untouched obstacle");
    assert_eq!(player(&app, P0).pop_cap, data(&app).building(manor).pop);
}

#[test]
fn cancelling_refunds_exactly() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    let tc = spawn_building_at(&mut app, P0, "town_center", Vec2::new(60.0, 60.0), true);
    let peasant = unit_id(&app, "peasant");
    let to_banner = tech_id(&app, "to_banner");
    let before = player(&app, P0).resources;

    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: peasant, count: 2 }));
    command(&mut app, cmd(P0, vec![tc], CommandKind::Research { tech: to_banner }));
    run_ticks(&mut app, 5);
    assert_eq!(queue(&app, tc).items.len(), 3);
    assert!(queue(&app, tc).progress > 0.0);

    command(&mut app, cmd(P0, vec![tc], CommandKind::CancelQueue { index: 2 }));
    run_ticks(&mut app, 1);
    assert!(!player(&app, P0).researching[to_banner.index()], "the tech can be queued again");
    command(&mut app, cmd(P0, vec![tc], CommandKind::CancelQueue { index: 0 }));
    run_ticks(&mut app, 1);
    let q = queue(&app, tc);
    assert_eq!(q.items.len(), 1);
    assert!(q.progress <= SIM_DT + 1e-6, "the next item starts from scratch");
    command(&mut app, cmd(P0, vec![tc], CommandKind::CancelQueue { index: 0 }));
    command(&mut app, cmd(P0, vec![tc], CommandKind::CancelQueue { index: 0 }));
    run_ticks(&mut app, 1);

    assert!(queue(&app, tc).items.is_empty());
    assert_eq!(player(&app, P0).resources, before);
}

#[test]
fn trickle_income_accrues_from_buildings_and_techs() {
    let mut app = new_match(&[("starborn", 0)]);
    let extractor = building_id(&app, "aether_extractor");
    spawn_building_at(&mut app, P0, "aether_extractor", Vec2::new(60.0, 60.0), true);
    spawn_building_at(&mut app, P0, "aether_extractor", Vec2::new(80.0, 80.0), false);
    let food = data(&app).resource_id("food").unwrap();
    edit_player(&mut app, P0, |p| p.mods.trickle.add(food, 1.0));
    let start = player(&app, P0).resources;

    seconds(&mut app, 2.0);

    let d = data(&app);
    let p = player(&app, P0);
    let per_sec = d.building(extractor).trickle_amounts;
    for r in d.resource_ids() {
        let tech = if r == food { 1.0 } else { 0.0 };
        let expect = 2.0 * (per_sec.get(r) + tech);
        assert!((p.resources.get(r) - start.get(r) - expect).abs() < 5e-3, "{}", d.resource(r).id);
        assert!((p.stats.gathered.get(r) - expect).abs() < 1e-3);
    }
}

#[test]
fn a_wall_drag_lays_the_right_number_of_segments() {
    let mut app = new_match(&[("starborn", 0)]);
    let peasant = spawn_unit_at(&mut app, P0, "peasant", Vec2::new(40.0, 40.0));
    let palisade = building_id(&app, "palisade_wall");
    let seg = data(&app).building(palisade).size[0];
    let wood_each = data(&app).building(palisade).cost.amounts.get(data(&app).resource_id("wood").unwrap());
    let wood = res(&app, P0, "wood");
    let (from, to) = (Vec2::new(20.3, 30.7), Vec2::new(20.3, 30.7) + Vec2::new(0.6, 0.8) * seg * 10.0);

    command(&mut app, cmd(P0, vec![peasant], CommandKind::PlaceWall { kind: palisade, from, to }));
    run_ticks(&mut app, 1);

    let mut segments: Vec<(Entity, f32)> = buildings_of(&mut app, palisade).into_iter().map(|e| (e, app.world().get::<Position>(e).unwrap().0.x)).collect();
    segments.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(segments.len(), 10);
    assert_eq!(res(&app, P0, "wood"), wood - 10.0 * wood_each);
    let sites: Vec<Order> = segments.iter().map(|(e, _)| Order::Build { site: *e }).collect();
    assert_eq!(orders(&app, peasant), sites, "builders take the segments in order along the line");

    // Money for three and a half segments: three are laid, then it stops.
    edit_player(&mut app, P0, |p| p.resources = ResAmounts::ZERO);
    let wood_id = data(&app).resource_id("wood").unwrap();
    edit_player(&mut app, P0, |p| p.resources.set(wood_id, wood_each * 3.5));
    command(&mut app, cmd(P0, vec![peasant], CommandKind::PlaceWall { kind: palisade, from: Vec2::new(20.0, 60.0), to: Vec2::new(20.0 + seg * 6.0, 60.0) }));
    run_ticks(&mut app, 1);
    assert_eq!(buildings_of(&mut app, palisade).len(), 13);
    assert!(warned(&app, P0, &Reason::CannotAfford.describe(&data(&app))));
    assert!((res(&app, P0, "wood") - wood_each * 0.5).abs() < 1e-4);
}

#[test]
fn an_extractor_claims_its_vent_until_it_is_gone() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    edit_player(&mut app, P0, |p| p.age = 3);
    let vent_kind = data(&app).node_id("aether_vent").unwrap();
    let vent = with_commands(&mut app, |c, d, _| spawn_node(c, d, vent_kind, Vec2::new(60.0, 60.0), 0.4, None));
    let extractor = building_id(&app, "aether_extractor");
    let before = player(&app, P0).resources;

    command(&mut app, cmd(P0, vec![], CommandKind::PlaceBuilding { kind: extractor, pos: Vec2::new(61.0, 60.5), facing: 0.0 }));
    run_ticks(&mut app, 1);
    let site = buildings_of(&mut app, extractor)[0];
    assert_eq!(app.world().get::<Position>(site).unwrap().0, Vec2::new(60.0, 60.0), "snapped onto the vent");
    assert_eq!(app.world().get::<ResourceNode>(vent).unwrap().claimed_by, Some(site));
    assert_eq!(app.world().get::<ClaimsNode>(site).map(|c| c.0), Some(vent));

    command(&mut app, cmd(P0, vec![], CommandKind::PlaceBuilding { kind: extractor, pos: Vec2::new(60.0, 60.0), facing: 0.0 }));
    run_ticks(&mut app, 1);
    assert_eq!(buildings_of(&mut app, extractor).len(), 1);
    assert!(warned(&app, P0, "Already claimed"));
    let mut paid_once = before;
    paid_once.sub_all(&data(&app).building(extractor).cost.amounts);
    assert_eq!(player(&app, P0).resources, paid_once);

    app.world_mut().entity_mut(site).despawn();
    assert_eq!(app.world().get::<ResourceNode>(vent).unwrap().claimed_by, None);
}

#[test]
fn rally_points_send_fresh_units_to_work() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    let tc = spawn_building_at(&mut app, P0, "town_center", Vec2::new(60.0, 60.0), true);
    let bush_kind = data(&app).node_id("berry_bush").unwrap();
    let bush = with_commands(&mut app, |c, d, _| spawn_node(c, d, bush_kind, Vec2::new(60.0, 80.0), 0.0, None));
    let peasant = unit_id(&app, "peasant");
    let scout = unit_id(&app, "scout_rider");

    command(&mut app, cmd(P0, vec![tc], CommandKind::Smart { point: Vec2::new(60.0, 80.0), target: Some(bush) }));
    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: peasant, count: 1 }));
    let wait = data(&app).unit(peasant).train_time + 0.1;
    seconds(&mut app, wait);
    let worker = units_of(&mut app, peasant)[0];
    assert_eq!(orders(&app, worker), vec![Order::Gather { target: bush }]);
    assert!(app.world().get::<Idle>(worker).is_none());
    let pos = app.world().get::<Position>(worker).unwrap().0;
    assert!(pos.y > 60.0, "spawned on the side facing the rally: {pos:?}");

    let point = Vec2::new(20.0, 20.0);
    command(&mut app, cmd(P0, vec![tc], CommandKind::SetRally { point, target: None }));
    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: scout, count: 1 }));
    let wait = data(&app).unit(scout).train_time + 0.1;
    seconds(&mut app, wait);
    let rider = units_of(&mut app, scout)[0];
    assert!(orders(&app, rider).is_empty(), "a walk to the rally is no order: {:?}", orders(&app, rider));
    assert_eq!(walk_target(&app, rider), Some(point));
    assert!(app.world().get::<Idle>(rider).is_none());
    assert_eq!(app.world().get::<Rally>(tc).unwrap().0, Some(RallyTarget::Point(point)));
}

#[test]
fn a_soldier_walks_to_its_rally_point_without_an_order_so_it_can_hit_back() {
    // af_behavior's stances (retaliation included) run only for units with
    // no orders; a Move order would make the soldier walk on under fire.
    let mut app = new_match(&[("ironhold", 0)]);
    make_rich(&mut app, P0);
    spawn_building_at(&mut app, P0, "town_center", Vec2::new(30.0, 30.0), true);
    let barracks = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 60.0), true);
    let spearman = unit_id(&app, "spearman");
    let point = Vec2::new(100.0, 60.0);

    command(&mut app, cmd(P0, vec![barracks], CommandKind::SetRally { point, target: None }));
    command(&mut app, cmd(P0, vec![barracks], CommandKind::Train { unit: spearman, count: 1 }));
    let wait = data(&app).unit(spearman).train_time + 0.1;
    seconds(&mut app, wait);
    let spear = units_of(&mut app, spearman)[0];
    assert!(orders(&app, spear).is_empty(), "{:?}", orders(&app, spear));
    assert_eq!(app.world().get::<Stance>(spear), Some(&Stance::Aggressive));
    assert_eq!(walk_target(&app, spear), Some(point));
    assert!(app.world().get::<Idle>(spear).is_none(), "walking is not idle");
}

#[test]
fn rallied_units_that_cannot_hurt_the_enemy_walk_up_to_it() {
    let mut app = new_match(&[("ironhold", 0), ("sylvan", 1)]);
    make_rich(&mut app, P0);
    edit_player(&mut app, P0, |p| p.age = 2);
    spawn_building_at(&mut app, P0, "town_center", Vec2::new(20.0, 20.0), true);
    let temple = spawn_building_at(&mut app, P0, "temple", Vec2::new(40.0, 40.0), true);
    let barracks = spawn_building_at(&mut app, P0, "barracks", Vec2::new(40.0, 90.0), true);
    let raider_at = Vec2::new(80.0, 40.0);
    let raider = spawn_unit_at(&mut app, P1, "peasant", raider_at);
    let balloon_at = Vec2::new(80.0, 90.0);
    let balloon = spawn_unit_at(&mut app, P1, "war_balloon", balloon_at);
    let (monk, spearman) = (unit_id(&app, "monk"), unit_id(&app, "spearman"));
    assert!(data(&app).unit(monk).weapon_ids.is_empty(), "the monk is unarmed");

    // An unarmed monk onto an enemy peasant; a ground-only spear onto a flyer.
    command(&mut app, cmd(P0, vec![temple], CommandKind::SetRally { point: raider_at, target: Some(raider) }));
    command(&mut app, cmd(P0, vec![barracks], CommandKind::SetRally { point: balloon_at, target: Some(balloon) }));
    command(&mut app, cmd(P0, vec![temple], CommandKind::Train { unit: monk, count: 1 }));
    command(&mut app, cmd(P0, vec![barracks], CommandKind::Train { unit: spearman, count: 1 }));
    let wait = data(&app).unit(monk).train_time.max(data(&app).unit(spearman).train_time) + 0.1;
    seconds(&mut app, wait);
    for (kind, target, target_at) in [(monk, raider, raider_at), (spearman, balloon, balloon_at)] {
        let fresh = units_of(&mut app, kind)[0];
        assert!(orders(&app, fresh).is_empty(), "no Attack it cannot carry out: {:?}", orders(&app, fresh));
        let walk = walk_target(&app, fresh).expect("walking to the enemy");
        let target_box = app.world().get::<Footprint>(target).unwrap().obb(target_at, 0.0);
        let gap = target_box.distance_to_point(walk);
        assert!(gap > 0.0 && gap < 3.0 && walk.x < target_at.x, "beside the enemy, on the building's side: {walk:?}");
    }

    // A spear can hit the peasant: that rally still attacks.
    command(&mut app, cmd(P0, vec![barracks], CommandKind::SetRally { point: raider_at, target: Some(raider) }));
    command(&mut app, cmd(P0, vec![barracks], CommandKind::Train { unit: spearman, count: 1 }));
    let wait = data(&app).unit(spearman).train_time + 0.1;
    seconds(&mut app, wait);
    let second = units_of(&mut app, spearman).into_iter().find(|e| orders(&app, *e).len() == 1).expect("a second spearman");
    assert_eq!(orders(&app, second), vec![Order::Attack { target: raider }]);
}

#[test]
fn victory_is_declared_when_one_side_is_wiped_out() {
    let mut app = new_match(&[("starborn", 0), ("sylvan", 1)]);
    spawn_building_at(&mut app, P0, "town_center", Vec2::new(30.0, 30.0), true);
    spawn_unit_at(&mut app, P0, "peasant", Vec2::new(40.0, 40.0));
    let hall = spawn_building_at(&mut app, P1, "town_center", Vec2::new(90.0, 90.0), true);
    let worker = spawn_unit_at(&mut app, P1, "peasant", Vec2::new(100.0, 100.0));
    spawn_building_at(&mut app, P1, "palisade_wall", Vec2::new(90.0, 110.0), true);

    seconds(&mut app, 12.0);
    assert!(!app.world().resource::<GameOutcome>().ended);
    assert!(!player(&app, P1).defeated);

    for e in [hall, worker] {
        app.world_mut().entity_mut(e).insert(Dead);
    }
    seconds(&mut app, 2.0);

    assert!(player(&app, P1).defeated, "walls alone do not keep a player alive");
    assert!(!player(&app, P0).defeated);
    let outcome = app.world().resource::<GameOutcome>().clone();
    assert!(outcome.ended);
    assert_eq!(outcome.winning_team, Some(0));
    assert!(outcome.time >= 12.0);
    let name = player(&app, P1).name.clone();
    assert!(notices(&app, P0).iter().any(|(_, t)| t.contains(&name) && t.contains("defeated")));
}

#[test]
fn farms_walls_and_foundations_do_not_keep_a_beaten_player_alive() {
    let mut app = new_match(&[("starborn", 0), ("sylvan", 1)]);
    spawn_building_at(&mut app, P0, "town_center", Vec2::new(30.0, 30.0), true);
    let hall = spawn_building_at(&mut app, P1, "town_center", Vec2::new(90.0, 90.0), true);
    for (i, id) in ["farm", "farm", "palisade_wall", "palisade_gate"].into_iter().enumerate() {
        spawn_building_at(&mut app, P1, id, Vec2::new(70.0 + 8.0 * i as f32, 110.0), true);
    }
    spawn_building_at(&mut app, P1, "barracks", Vec2::new(110.0, 70.0), false);
    seconds(&mut app, 12.0);
    assert!(!player(&app, P1).defeated, "the hall keeps it in the game");

    app.world_mut().entity_mut(hall).insert(Dead);
    seconds(&mut app, 2.0);
    assert!(player(&app, P1).defeated, "two farms, a wall, a gate and a foundation nobody can build are nothing");
    let outcome = app.world().resource::<GameOutcome>().clone();
    assert!(outcome.ended && outcome.winning_team == Some(0));
}

#[test]
fn a_house_or_a_harbour_keeps_a_player_in_the_game() {
    let mut app = new_match(&[("starborn", 0), ("tide", 1), ("sylvan", 2)]);
    spawn_building_at(&mut app, P0, "town_center", Vec2::new(30.0, 30.0), true);
    spawn_building_at(&mut app, P1, "tide_harbor", Vec2::new(90.0, 90.0), true);
    spawn_building_at(&mut app, PlayerId(2), "house", Vec2::new(30.0, 100.0), true);
    seconds(&mut app, 12.0);
    assert!(!player(&app, P1).defeated, "a harbour is a field, but it trains, researches and takes deposits");
    assert!(!player(&app, PlayerId(2)).defeated, "a house is not a field");
    assert!(!app.world().resource::<GameOutcome>().ended);
}

#[test]
fn losing_every_production_building_reveals_the_rest() {
    let mut app = new_match(&[("starborn", 0), ("sylvan", 1)]);
    spawn_building_at(&mut app, P0, "town_center", Vec2::new(30.0, 30.0), true);
    let hall = spawn_building_at(&mut app, P1, "town_center", Vec2::new(90.0, 90.0), true);
    spawn_building_at(&mut app, P1, "house", Vec2::new(110.0, 90.0), true);
    spawn_building_at(&mut app, P1, "lumber_camp", Vec2::new(90.0, 110.0), true);
    spawn_unit_at(&mut app, P1, "peasant", Vec2::new(100.0, 100.0));
    seconds(&mut app, 12.0);
    let revealed = |app: &App| app.world().resource::<RevealedPlayers>().list.clone();
    assert!(revealed(&app).is_empty(), "nobody is revealed while they hold a hall");

    app.world_mut().entity_mut(hall).insert(Dead);
    seconds(&mut app, 2.0);
    assert_eq!(revealed(&app), vec![P1], "a house and a camp are no production building");
    assert!(!player(&app, P1).defeated, "the house and the worker keep it in the game");
    let name = player(&app, P1).name.clone();
    assert!(notices(&app, P0).iter().any(|(k, t)| *k == NoticeKind::Alert && t.contains(&name) && t.contains("revealed")));

    // A new production building hides it again.
    spawn_building_at(&mut app, P1, "barracks", Vec2::new(70.0, 90.0), false);
    seconds(&mut app, 2.0);
    assert_eq!(revealed(&app), vec![P1], "a foundation trains nothing yet");
    spawn_building_at(&mut app, P1, "barracks", Vec2::new(70.0, 110.0), true);
    seconds(&mut app, 2.0);
    assert!(revealed(&app).is_empty());
    assert!(notices(&app, P0).iter().any(|(_, t)| t.contains(&name) && t.contains("hidden again")));
}

#[test]
fn defeated_players_are_not_revealed_and_reveals_reset_with_the_match() {
    let mut app = new_match(&[("starborn", 0), ("sylvan", 1), ("tide", 2)]);
    spawn_building_at(&mut app, P0, "town_center", Vec2::new(30.0, 30.0), true);
    spawn_building_at(&mut app, PlayerId(2), "town_center", Vec2::new(30.0, 100.0), true);
    spawn_building_at(&mut app, P1, "farm", Vec2::new(90.0, 90.0), true);
    seconds(&mut app, 12.0);
    assert!(player(&app, P1).defeated);
    assert!(app.world().resource::<RevealedPlayers>().is_empty(), "out of the game, not revealed");

    app.world_mut().resource_mut::<RevealedPlayers>().set(P0, true);
    app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Menu);
    run_ticks(&mut app, 1);
    assert!(app.world().resource::<RevealedPlayers>().is_empty(), "forgotten when the match ends");
}

#[test]
fn a_one_player_match_never_ends() {
    let mut app = new_match(&[("starborn", 0)]);
    seconds(&mut app, 15.0);
    assert!(!app.world().resource::<GameOutcome>().ended);
}

#[test]
fn rallying_onto_an_enemy_attacks_and_onto_a_foundation_builds() {
    let mut app = new_match(&[("starborn", 0), ("sylvan", 1)]);
    make_rich(&mut app, P0);
    let tc = spawn_building_at(&mut app, P0, "town_center", Vec2::new(40.0, 40.0), true);
    let raider = spawn_unit_at(&mut app, P1, "peasant", Vec2::new(70.0, 40.0));
    let site = spawn_building_at(&mut app, P0, "house", Vec2::new(40.0, 70.0), false);
    let scout = unit_id(&app, "scout_rider");
    let peasant = unit_id(&app, "peasant");

    command(&mut app, cmd(P0, vec![tc], CommandKind::SetRally { point: Vec2::new(70.0, 40.0), target: Some(raider) }));
    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: scout, count: 1 }));
    let wait = data(&app).unit(scout).train_time + 0.1;
    seconds(&mut app, wait);
    let rider = units_of(&mut app, scout)[0];
    assert_eq!(orders(&app, rider), vec![Order::Attack { target: raider }]);

    command(&mut app, cmd(P0, vec![tc], CommandKind::SetRally { point: Vec2::new(40.0, 70.0), target: Some(site) }));
    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: peasant, count: 1 }));
    let wait = data(&app).unit(peasant).train_time + 0.1;
    seconds(&mut app, wait);
    let own: Vec<Entity> = units_of(&mut app, peasant).into_iter().filter(|e| *e != raider).collect();
    assert_eq!(own.len(), 1);
    assert_eq!(orders(&app, own[0]), vec![Order::Build { site }]);
}

#[test]
fn a_finished_foundation_completes_with_the_hp_its_builders_gave_it() {
    let mut app = new_match(&[("starborn", 0)]);
    let house = building_id(&app, "house");
    let home = spawn_building_at(&mut app, P0, "house", Vec2::new(40.0, 40.0), false);
    let wall = spawn_building_at(&mut app, P0, "palisade_wall", Vec2::new(60.0, 60.0), false);
    for e in [home, wall] {
        let world = app.world_mut();
        world.get_mut::<UnderConstruction>(e).unwrap().progress = 1.0;
        world.get_mut::<Health>(e).unwrap().hp = 120.0;
    }
    run_ticks(&mut app, 2);

    for e in [home, wall] {
        assert!(app.world().get::<UnderConstruction>(e).is_none());
        assert_eq!(app.world().get::<Health>(e).unwrap().hp, 120.0);
    }
    let p = player(&app, P0);
    assert_eq!(p.stats.buildings_built, 2);
    assert_eq!(p.pop_cap, data(&app).building(house).pop);
    assert_eq!(app.world().resource::<Seen>().completed.len(), 2);
    let infos: Vec<String> = notices(&app, P0).into_iter().filter(|(k, _)| *k == NoticeKind::Info).map(|(_, t)| t).collect();
    assert_eq!(infos, vec![format!("{} complete", data(&app).building(house).name)], "no notice per wall segment");
}

#[test]
fn placements_with_unusable_coordinates_are_refused() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    let peasant = spawn_unit_at(&mut app, P0, "peasant", Vec2::new(40.0, 40.0));
    let tc = spawn_building_at(&mut app, P0, "town_center", Vec2::new(60.0, 60.0), true);
    let house = building_id(&app, "house");
    let palisade = building_id(&app, "palisade_wall");
    let before = player(&app, P0).resources;
    let ok = Vec2::new(30.0, 30.0);
    let far = Vec2::new(f32::MAX, 20.0);

    for (pos, facing) in [(Vec2::new(f32::NAN, 30.0), 0.0), (ok, f32::NAN), (Vec2::new(f32::INFINITY, 30.0), 0.0)] {
        command(&mut app, cmd(P0, vec![peasant], CommandKind::PlaceBuilding { kind: house, pos, facing }));
    }
    for (from, to) in [(ok, Vec2::new(f32::INFINITY, 20.0)), (ok, Vec2::new(20.0, f32::NAN)), (Vec2::NAN, ok), (far, Vec2::new(-f32::MAX, f32::MAX))] {
        command(&mut app, cmd(P0, vec![peasant], CommandKind::PlaceWall { kind: palisade, from, to }));
    }
    // Only wall types are laid in lines.
    command(&mut app, cmd(P0, vec![peasant], CommandKind::PlaceWall { kind: house, from: ok, to: ok + Vec2::new(40.0, 0.0) }));
    command(&mut app, cmd(P0, vec![tc], CommandKind::SetRally { point: Vec2::new(f32::NAN, 1.0), target: None }));
    run_ticks(&mut app, 1);

    assert_eq!(player(&app, P0).resources, before, "nothing charged");
    assert!(buildings_of(&mut app, house).is_empty() && buildings_of(&mut app, palisade).is_empty());
    assert!(orders(&app, peasant).is_empty());
    assert!(warned(&app, P0, &PlacementError::OutOfBounds.describe(&data(&app))));
    let list = &app.world().resource::<Notices>().list;
    assert!(list.iter().all(|n| n.pos.is_none_or(|p| p.is_finite())), "notices only point at real places");
    assert_eq!(app.world().get::<Rally>(tc).unwrap().0, None, "a NaN rally point is ignored");
}

#[test]
fn a_wall_drag_far_past_the_map_lays_only_what_fits() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    let palisade = building_id(&app, "palisade_wall");
    let seg = data(&app).building(palisade).size[0];
    let from = Vec2::new(20.0, 20.0);

    command(&mut app, cmd(P0, vec![], CommandKind::PlaceWall { kind: palisade, from, to: Vec2::new(1e30, 20.0) }));
    run_ticks(&mut app, 1);

    let laid = buildings_of(&mut app, palisade);
    assert_eq!(laid.len() as f32, ((128.0 - from.x) / seg).floor(), "every segment inside the map, end to end");
    for e in laid {
        let pos = app.world().get::<Position>(e).unwrap().0;
        assert!(pos.x > from.x && pos.x < 128.0 && (pos.y - from.y).abs() < 1e-4, "{pos:?}");
    }
    assert!(warned(&app, P0, &PlacementError::OutOfBounds.describe(&data(&app))));
}

#[test]
fn commands_of_different_kinds_spend_in_inbox_order() {
    let mut app = new_match(&[("starborn", 0)]);
    let barracks = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 60.0), true);
    spawn_building_at(&mut app, P0, "house", Vec2::new(90.0, 90.0), true);
    let militia = unit_id(&app, "militia");
    let palisade = building_id(&app, "palisade_wall");
    let militia_cost = stats::unit_cost(&data(&app), player(&app, P0), militia);
    let wall_cost = stats::building_cost(&data(&app), player(&app, P0), palisade);
    assert!(militia_cost.covers(&wall_cost), "a cancelled training pays for a wall segment");
    edit_player(&mut app, P0, |p| p.resources = militia_cost);
    let place = |x: f32| cmd(P0, vec![], CommandKind::PlaceBuilding { kind: palisade, pos: Vec2::new(x, 20.0), facing: 0.0 });

    // Money for one: the training was issued first and gets it.
    command(&mut app, cmd(P0, vec![barracks], CommandKind::Train { unit: militia, count: 1 }));
    command(&mut app, place(20.0));
    run_ticks(&mut app, 1);
    assert_eq!(queue(&app, barracks).items.len(), 1);
    assert!(buildings_of(&mut app, palisade).is_empty());
    assert!(warned(&app, P0, &Reason::CannotAfford.describe(&data(&app))));

    // The refund of a cancel issued first pays for the placement after it.
    command(&mut app, cmd(P0, vec![barracks], CommandKind::CancelQueue { index: 0 }));
    command(&mut app, place(30.0));
    run_ticks(&mut app, 1);
    assert!(queue(&app, barracks).items.is_empty());
    assert_eq!(buildings_of(&mut app, palisade).len(), 1);
    let mut left = militia_cost;
    left.sub_all(&wall_cost);
    assert_eq!(player(&app, P0).resources, left);
}

#[test]
fn a_later_unit_command_in_the_same_tick_overrides_the_build_order() {
    let mut app = with_unit_intake(new_match(&[("starborn", 0)]));
    make_rich(&mut app, P0);
    let [first, last, queued, shifted] = [40.0, 44.0, 48.0, 52.0].map(|x| spawn_unit_at(&mut app, P0, "peasant", Vec2::new(x, 40.0)));
    let house = building_id(&app, "house");
    let place = |builder: Entity, x: f32| cmd(P0, vec![builder], CommandKind::PlaceBuilding { kind: house, pos: Vec2::new(x, 70.0), facing: 0.0 });
    let away = Vec2::new(10.0, 10.0);
    let walk = |builder: Entity| cmd(P0, vec![builder], CommandKind::Move(away));
    let far = Vec2::new(100.0, 100.0);
    app.world_mut().get_mut::<Orders>(shifted).unwrap().queue = [Order::Move { target: far }].into();

    // Placed, then sent elsewhere: the walk is the last word.
    command(&mut app, place(first, 20.0));
    command(&mut app, walk(first));
    // Sent elsewhere, then placed: the placement is.
    command(&mut app, walk(last));
    command(&mut app, place(last, 40.0));
    // A queued command after the placement does not cancel it, and comes
    // after it.
    command(&mut app, place(queued, 60.0));
    command(&mut app, walk(queued).queued(true));
    // Shift-placed, then shift-sent: both queue, in that order, after the
    // walk the builder was already on.
    command(&mut app, place(shifted, 80.0).queued(true));
    command(&mut app, walk(shifted).queued(true));
    run_ticks(&mut app, 1);

    let sites: Vec<(Entity, f32)> = buildings_of(&mut app, house).into_iter().map(|e| (e, app.world().get::<Position>(e).unwrap().0.x)).collect();
    assert_eq!(sites.len(), 4, "every placement is paid and laid");
    let site_at = |x: f32| sites.iter().find(|(_, sx)| *sx == x).map(|(e, _)| *e).unwrap();
    // The stub applies the walks as af_behavior would; economy gives the Build orders.
    assert_eq!(orders(&app, first), vec![Order::Move { target: away }]);
    assert_eq!(orders(&app, last), vec![Order::Build { site: site_at(40.0) }]);
    assert!(app.world().get::<Idle>(last).is_none(), "a builder is no longer idle");
    assert_eq!(orders(&app, queued), vec![Order::Build { site: site_at(60.0) }, Order::Move { target: away }]);
    assert_eq!(
        orders(&app, shifted),
        vec![Order::Move { target: far }, Order::Build { site: site_at(80.0) }, Order::Move { target: away }]
    );
}

#[test]
fn a_building_deleted_earlier_in_the_tick_takes_no_economy_commands() {
    let mut app = with_unit_intake(new_match(&[("starborn", 0)]));
    make_rich(&mut app, P0);
    spawn_building_at(&mut app, P0, "town_center", Vec2::new(30.0, 30.0), true);
    let barracks = spawn_building_at(&mut app, P0, "barracks", Vec2::new(70.0, 70.0), true);
    let militia = unit_id(&app, "militia");
    let before = player(&app, P0).resources;

    // Run one by one, the deleted barracks could not train: nothing is paid
    // for a queue that dies with it.
    command(&mut app, cmd(P0, vec![barracks], CommandKind::Delete));
    command(&mut app, cmd(P0, vec![barracks], CommandKind::Train { unit: militia, count: 1 }));
    command(&mut app, cmd(P0, vec![barracks], CommandKind::SetRally { point: Vec2::new(90.0, 90.0), target: None }));
    run_ticks(&mut app, 1);
    assert!(app.world().get::<Building>(barracks).is_none(), "deleted");
    assert_eq!(player(&app, P0).resources, before);
}

#[test]
fn training_spreads_over_the_selected_buildings_by_shortest_queue() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    let east = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 60.0), true);
    let west = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 90.0), true);
    spawn_building_at(&mut app, P0, "house", Vec2::new(20.0, 20.0), true);
    let spearman = unit_id(&app, "spearman");

    command(&mut app, cmd(P0, vec![east], CommandKind::Train { unit: spearman, count: 1 }));
    command(&mut app, cmd(P0, vec![east, west], CommandKind::Train { unit: spearman, count: 5 }));
    run_ticks(&mut app, 1);

    assert_eq!((queue(&app, east).items.len(), queue(&app, west).items.len()), (3, 3));
}

#[test]
fn a_full_queue_refuses_more() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    let tc = spawn_building_at(&mut app, P0, "town_center", Vec2::new(60.0, 60.0), true);
    let peasant = unit_id(&app, "peasant");
    let mut paid_for_a_full_queue = player(&app, P0).resources;
    paid_for_a_full_queue.sub_all(&stats::unit_cost(&data(&app), player(&app, P0), peasant).scaled(ProductionQueue::MAX as f32));

    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: peasant, count: ProductionQueue::MAX as u32 + 3 }));
    run_ticks(&mut app, 1);

    assert_eq!(queue(&app, tc).items.len(), ProductionQueue::MAX);
    assert!(warned(&app, P0, "Production queue is full"));
    let p = player(&app, P0);
    for r in data(&app).resource_ids() {
        assert!((p.resources.get(r) - paid_for_a_full_queue.get(r)).abs() < 1e-3, "only what was queued is paid");
    }
}

#[test]
fn the_refusal_shown_is_the_one_that_matters() {
    let mut app = new_match(&[("starborn", 0)]);
    let house = spawn_building_at(&mut app, P0, "house", Vec2::new(30.0, 30.0), true);
    let barracks = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 60.0), true);
    let foundation = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 90.0), false);
    let militia = unit_id(&app, "militia");
    let drill = tech_id(&app, "militia_drill");
    edit_player(&mut app, P0, |p| {
        p.resources = ResAmounts::ZERO;
        p.age = 2;
    });

    command(&mut app, cmd(P0, vec![house, barracks], CommandKind::Train { unit: militia, count: 1 }));
    command(&mut app, cmd(P0, vec![house, foundation], CommandKind::Research { tech: drill }));
    run_ticks(&mut app, 1);

    // Not the house's "Not here" in either case.
    let warnings: Vec<String> = notices(&app, P0).into_iter().filter(|(k, _)| *k == NoticeKind::Warning).map(|(_, t)| t).collect();
    let d = data(&app);
    assert_eq!(warnings, vec![Reason::CannotAfford.describe(&d), Reason::UnderConstruction.describe(&d)]);
}

#[test]
fn upgrading_a_mixed_selection_pays_for_what_it_can() {
    let mut app = new_match(&[("starborn", 0)]);
    edit_player(&mut app, P0, |p| p.age = 2);
    let gate = spawn_building_at(&mut app, P0, "palisade_gate", Vec2::new(60.0, 40.0), true);
    let walls = [60.0, 70.0].map(|y| spawn_building_at(&mut app, P0, "palisade_wall", Vec2::new(60.0, y), true));
    let d = data(&app);
    let wall_up = d.building(building_id(&app, "palisade_wall")).upgrade.clone().unwrap().cost.amounts;
    let gate_up = d.building(building_id(&app, "palisade_gate")).upgrade.clone().unwrap().cost.amounts;
    let mut budget = wall_up;
    budget.add_all(&wall_up);
    assert!(!budget.covers(&gate_up), "the content makes the gate dearer than two walls");
    edit_player(&mut app, P0, |p| p.resources = budget);

    command(&mut app, cmd(P0, vec![gate, walls[0], walls[1]], CommandKind::UpgradeBuilding));
    run_ticks(&mut app, 1);

    assert!(queue(&app, gate).items.is_empty());
    for w in walls {
        assert!(matches!(queue(&app, w).items.front().map(|i| i.kind), Some(QueueKind::Upgrade(_))));
    }
    assert_eq!(player(&app, P0).resources, ResAmounts::ZERO);
    assert!(warned(&app, P0, &Reason::CannotAfford.describe(&d)));
}

#[test]
fn losing_a_building_frees_its_queued_research() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    edit_player(&mut app, P0, |p| p.age = 2);
    let doomed = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 60.0), true);
    let spare = spawn_building_at(&mut app, P0, "barracks", Vec2::new(60.0, 90.0), true);
    let drill = tech_id(&app, "militia_drill");

    command(&mut app, cmd(P0, vec![doomed], CommandKind::Research { tech: drill }));
    run_ticks(&mut app, 1);
    assert!(player(&app, P0).researching[drill.index()]);

    app.world_mut().entity_mut(doomed).despawn();
    assert!(!player(&app, P0).researching[drill.index()], "the research died with its building");
    command(&mut app, cmd(P0, vec![spare], CommandKind::Research { tech: drill }));
    run_ticks(&mut app, 1);
    assert_eq!(queue(&app, spare).items.front().map(|i| i.kind), Some(QueueKind::Tech(drill)));
}

#[test]
fn an_extractor_drains_its_vent_and_keeps_seeping() {
    let mut app = new_match(&[("starborn", 0)]);
    let extractor = building_id(&app, "aether_extractor");
    let vent_kind = data(&app).node_id("aether_vent").unwrap();
    let aether = data(&app).resource_id("aether").unwrap();
    let per_sec = data(&app).building(extractor).trickle_amounts;
    let seep: Vec<ResourceId> = data(&app).resource_ids().filter(|r| *r != aether && per_sec.get(*r) > 0.0).collect();
    assert!(per_sec.get(aether) > 0.0 && !seep.is_empty(), "the extractor yields aether and a seep");
    let held = per_sec.get(aether) * 1.5;
    let pos = Vec2::new(60.0, 60.0);
    let vent = with_commands(&mut app, |c, d, _| spawn_node(c, d, vent_kind, pos, 0.0, Some(held)));
    let site = spawn_building_at(&mut app, P0, "aether_extractor", pos, true);
    app.world_mut().entity_mut(site).insert(ClaimsNode(vent));
    let start = player(&app, P0).resources;

    seconds(&mut app, 1.0);
    let node = *app.world().get::<ResourceNode>(vent).unwrap();
    assert!((node.remaining - per_sec.get(aether) * 0.5).abs() < 1e-3, "{node:?}");

    seconds(&mut app, 3.0);
    let p = player(&app, P0);
    assert_eq!(app.world().get::<ResourceNode>(vent).unwrap().remaining, 0.0);
    assert!((p.resources.get(aether) - start.get(aether) - held).abs() < 1e-3, "no more aether than the vent held");
    for r in seep {
        assert!((p.resources.get(r) - start.get(r) - 4.0 * per_sec.get(r)).abs() < 1e-3, "the seep never stops");
    }
    let exhausted = format!("{} exhausted", data(&app).node(vent_kind).name);
    assert_eq!(notices(&app, P0).iter().filter(|(_, t)| *t == exhausted).count(), 1);
}

#[test]
fn rallies_fall_back_on_the_click_and_stop_beside_buildings() {
    let mut app = new_match(&[("starborn", 0)]);
    make_rich(&mut app, P0);
    let tc = spawn_building_at(&mut app, P0, "town_center", Vec2::new(60.0, 60.0), true);
    let home_pos = Vec2::new(60.0, 90.0);
    let home = spawn_building_at(&mut app, P0, "house", home_pos, true);
    let bush_kind = data(&app).node_id("berry_bush").unwrap();
    let click = Vec2::new(30.0, 60.0);
    let bush = with_commands(&mut app, |c, d, _| spawn_node(c, d, bush_kind, click, 0.0, None));
    let scout = unit_id(&app, "scout_rider");
    let wait = data(&app).unit(scout).train_time + 0.1;

    command(&mut app, cmd(P0, vec![tc], CommandKind::Smart { point: click, target: Some(bush) }));
    run_ticks(&mut app, 1);
    app.world_mut().entity_mut(bush).despawn();
    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: scout, count: 1 }));
    seconds(&mut app, wait);
    let rider = units_of(&mut app, scout)[0];
    assert!(orders(&app, rider).is_empty(), "{:?}", orders(&app, rider));
    assert_eq!(walk_target(&app, rider), Some(click), "the bush is gone: go where the player clicked");
    assert!(app.world().get::<Position>(rider).unwrap().0.x < 60.0, "and leave on that side");

    command(&mut app, cmd(P0, vec![tc], CommandKind::SetRally { point: home_pos, target: Some(home) }));
    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: scout, count: 1 }));
    seconds(&mut app, wait);
    let fresh = units_of(&mut app, scout).into_iter().find(|e| *e != rider).unwrap();
    assert!(orders(&app, fresh).is_empty(), "{:?}", orders(&app, fresh));
    let target = walk_target(&app, fresh).expect("walking to the house");
    let house_box = app.world().get::<Footprint>(home).unwrap().obb(home_pos, 0.0);
    let gap = house_box.distance_to_point(target);
    assert!(gap > 0.0 && gap < 3.0 && target.y < home_pos.y, "beside the house, on the town centre's side: {target:?}");
}

/// Boxes of every unit, with their entities.
fn unit_boxes(app: &mut App) -> Vec<(Entity, Obb)> {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(Entity, &Position, &Facing, &Footprint), With<Unit>>();
    q.iter(world).map(|(e, p, f, fp)| (e, fp.obb(p.0, f.0))).collect()
}

/// Another player's peasants (they take none of P0's population) standing
/// on a `step`-metre lattice over the square of side `side` around
/// `center`, outside `keep_out`.
fn crowd(app: &mut App, center: Vec2, side: f32, step: f32, keep_out: &Obb) -> Vec<Entity> {
    let n = (side / step) as i32;
    let mut out = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let at = center - Vec2::splat(side * 0.5) + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * step;
            if keep_out.distance_to_point(at) > 0.5 {
                out.push(spawn_unit_at(app, P1, "peasant", at));
            }
        }
    }
    out
}

/// Trained units, squads and two trainings finishing in the same tick at
/// one exit never appear on the units crowding it: every box keeps clear
/// of every other (round-4 bug: a ram trained onto a lancer standing there
/// started 0.93 m deep in it).
#[test]
fn trained_units_appear_clear_of_the_crowd_at_the_exit() {
    let mut app = new_match(&[("starborn", 0), ("dune", 1)]);
    make_rich(&mut app, P0);
    spawn_building_at(&mut app, P1, "house", Vec2::new(10.0, 10.0), true);
    let tc = spawn_building_at(&mut app, P0, "town_center", Vec2::new(60.0, 60.0), true);
    let barracks = spawn_building_at(&mut app, P0, "barracks", Vec2::new(72.0, 60.0), true);
    run_ticks(&mut app, 1);
    // Idle workers (another player's) packed in front of both, the town
    // centre's exit side.
    let tc_box = app.world().get::<Footprint>(tc).unwrap().obb(Vec2::new(60.0, 60.0), 0.0);
    let b_box = app.world().get::<Footprint>(barracks).unwrap().obb(Vec2::new(72.0, 60.0), 0.0);
    for x in 0..8 {
        for y in 0..3 {
            spawn_unit_at(&mut app, P1, "peasant", Vec2::new(57.0 + x as f32 * 0.9, 65.2 + y as f32 * 0.9));
        }
    }
    let peasant = unit_id(&app, "peasant");
    let militia = unit_id(&app, "militia");
    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: peasant, count: 3 }));
    command(&mut app, cmd(P0, vec![barracks], CommandKind::Train { unit: militia, count: 2 }));
    let longest = data(&app).unit(peasant).train_time * 3.0 + data(&app).unit(militia).train_time * 2.0;
    seconds(&mut app, longest + 1.0);

    let boxes = unit_boxes(&mut app);
    let squad = data(&app).unit(militia).squad as usize;
    assert_eq!(boxes.len(), 24 + 3 + 2 * squad, "everything trained");
    for (i, (a, x)) in boxes.iter().enumerate() {
        assert!(!x.overlaps(&tc_box) && !x.overlaps(&b_box), "{a} inside a building");
        for (b, y) in &boxes[i + 1..] {
            assert!(!x.overlaps(y), "{a} and {b} overlap by {:?}", x.mtv(y));
        }
    }
    let stats = *app.world().resource::<SpawnStats>();
    assert_eq!((stats.spawned, stats.waited), (5, 0), "room was found at once");
}

/// A building boxed in by units over the whole search area keeps its
/// finished training waiting (shown finished, population not taken)
/// instead of stacking it on them, and sets it down once there is room.
#[test]
fn a_training_with_no_room_waits_for_it() {
    let mut app = new_match(&[("starborn", 0), ("dune", 1)]);
    make_rich(&mut app, P0);
    spawn_building_at(&mut app, P1, "house", Vec2::new(10.0, 10.0), true);
    let at = Vec2::new(64.0, 64.0);
    let tc = spawn_building_at(&mut app, P0, "town_center", at, true);
    run_ticks(&mut app, 1);
    let tc_box = app.world().get::<Footprint>(tc).unwrap().obb(at, 0.0);
    let packed = crowd(&mut app, at, 40.0, 1.0, &tc_box);
    let peasant = unit_id(&app, "peasant");
    command(&mut app, cmd(P0, vec![tc], CommandKind::Train { unit: peasant, count: 1 }));
    let train = data(&app).unit(peasant).train_time;
    seconds(&mut app, train + 2.0);

    assert_eq!(unit_boxes(&mut app).len(), packed.len(), "nothing appeared on the crowd");
    let q = queue(&app, tc);
    assert_eq!((q.items.len(), q.progress), (1, q.items[0].time), "waits, shown finished");
    let stats = *app.world().resource::<SpawnStats>();
    assert!(stats.waited >= 2 && stats.waited <= 6 && stats.spawned == 0, "looks again now and then: {stats:?}");

    for e in packed {
        app.world_mut().entity_mut(e).despawn();
    }
    seconds(&mut app, 1.0);
    let trained = units_of(&mut app, peasant);
    assert_eq!(trained.len(), 1);
    assert!(queue(&app, tc).items.is_empty());
    let pos = app.world().get::<Position>(trained[0]).unwrap().0;
    assert!(pos.y > at.y + tc_box.half.y && pos.distance(at) < 8.0, "at the exit once it is clear: {pos}");
}
