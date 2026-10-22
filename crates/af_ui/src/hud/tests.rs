//! Headless tests of the real HUD on a flat test world. Bevy's UI and
//! input plugins are absent, so the tests set `Interaction`s and key
//! states the way those plugins would.

use af_core::ext::fog::{BuildingSighting, FogGhost, FogGhosts, LastSeen, SeenBy};
use af_core::prelude::*;
use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};
use bevy::ecs::entity::EntityHashMap;
use bevy::picking::backend::HitData;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::ui::RelativeCursorPosition;

use super::card_view::{CardSlot, CardState, SlotCount};
use super::minimap::{Minimap, MINIMAP_RES};
use super::overlays::{GameOverScreen, GameOverState, LeavePanel};
use super::selection::QueueSlot;
use super::{HudRoot, HudView, SelectionInfo};
use crate::card::{BuildCategory, CardAction, CardMenu};
use crate::{HudPlugin, MinimapGeometry};

fn add_client_resources(app: &mut App) {
    // Any failing system (e.g. a missing resource) fails the test instead of
    // being skipped with an unseen warning.
    app.insert_resource(bevy::ecs::error::FallbackErrorHandler(bevy::ecs::error::panic));
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.init_asset::<Image>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    app.add_plugins(HudPlugin);
}

/// A match between ironhold (local, team 0) and dune (team 1) with the HUD up.
fn hud_app() -> App {
    let mut app = sim_app(test_data());
    add_client_resources(&mut app);
    flat_world(&mut app, 128.0, &[("ironhold", 0), ("dune", 1)]);
    app.insert_resource(LocalPlayer(Some(PlayerId(0))));
    run_ticks(&mut app, 1);
    app
}

fn spawn_unit_at(app: &mut App, player: u8, id: &str, pos: Vec2) -> Entity {
    with_commands(app, |c, data, players| spawn_unit(c, data, &players.list[player as usize], data.unit_id(id).unwrap(), pos, 0.0))
}

fn spawn_building_at(app: &mut App, player: u8, id: &str, pos: Vec2) -> Entity {
    with_commands(app, |c, data, players| spawn_building(c, data, &players.list[player as usize], data.building_id(id).unwrap(), pos, 0.0, true))
}

fn select(app: &mut App, entities: &[Entity]) {
    for e in entities {
        app.world_mut().entity_mut(*e).insert(Selected);
    }
}

fn texts(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    world.query::<&Text>().iter(world).map(|t| t.0.clone()).collect()
}

fn press_slot(app: &mut App, slot: usize) {
    let world = app.world_mut();
    let e = world.query::<(Entity, &CardSlot)>().iter(world).find(|(_, s)| s.0 == slot).map(|(e, _)| e).unwrap();
    world.entity_mut(e).insert(Interaction::Pressed);
    app.update();
    app.world_mut().entity_mut(e).insert(Interaction::None);
}

/// The button whose label is `label`.
fn button(app: &mut App, label: &str) -> Entity {
    let world = app.world_mut();
    let labels: Vec<(Entity, String)> = world.query::<(&ChildOf, &Text)>().iter(world).map(|(c, t)| (c.parent(), t.0.clone())).collect();
    labels.into_iter().find(|(p, t)| t == label && world.get::<Button>(*p).is_some()).map(|(p, _)| p).unwrap_or_else(|| panic!("no button {label:?}"))
}

fn press_button(app: &mut App, label: &str) {
    let e = button(app, label);
    press(app, e);
}

fn press(app: &mut App, e: Entity) {
    app.world_mut().entity_mut(e).insert(Interaction::Pressed);
    app.update();
}

fn tap(app: &mut App, key: KeyCode) {
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
    app.update();
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(key);
    keys.clear();
}

fn pending(app: &App) -> Vec<PlayerCommand> {
    app.world().resource::<CommandInbox>().pending.clone()
}

#[test]
fn hud_spawns_with_resources_and_idle_workers() {
    let mut app = hud_app();
    spawn_unit_at(&mut app, 0, "peasant", Vec2::new(40.0, 40.0));
    spawn_unit_at(&mut app, 0, "peasant", Vec2::new(42.0, 40.0));
    spawn_unit_at(&mut app, 1, "peasant", Vec2::new(90.0, 90.0));
    run_ticks(&mut app, 2);
    let world = app.world_mut();
    assert_eq!(world.query::<&HudRoot>().iter(world).count(), 1);
    let t = texts(&mut app);
    // Ironhold starts with 300 food and wood, 150 stone, 100 copper.
    assert_eq!(t.iter().filter(|s| *s == "300").count(), 2, "{t:?}");
    assert!(t.contains(&"150".to_string()) && t.contains(&"100".to_string()));
    assert!(t.contains(&"Idle workers: 2".to_string()), "{t:?}");
    assert!(t.contains(&"Hearth Age".to_string()));
    assert!(t.contains(&"2/0".to_string()), "pop used/cap: {t:?}");
}

#[test]
fn town_center_card_trains_on_click() {
    let mut app = hud_app();
    let tc = spawn_building_at(&mut app, 0, "town_center", Vec2::new(40.0, 40.0));
    select(&mut app, &[tc]);
    run_ticks(&mut app, 2);
    let data = test_data();
    let peasant = data.unit_id("peasant").unwrap();
    assert_eq!(app.world().resource::<CardState>().slots[0], Some(CardAction::Train(peasant)));
    press_slot(&mut app, 0);
    let cmds = pending(&app);
    assert_eq!(cmds.len(), 1, "{cmds:?}");
    assert_eq!(cmds[0].kind, CommandKind::Train { unit: peasant, count: 1 });
    assert_eq!(cmds[0].entities, vec![tc]);
    assert_eq!(cmds[0].player, PlayerId(0));
}

#[test]
fn disabled_buttons_explain_instead_of_commanding() {
    let mut app = hud_app();
    // A barracks with no houses: training is pop capped.
    let barracks = spawn_building_at(&mut app, 0, "barracks", Vec2::new(40.0, 40.0));
    app.world_mut().resource_mut::<Players>().list[0].pop_cap = 0;
    select(&mut app, &[barracks]);
    run_ticks(&mut app, 2);
    let slot = app.world().resource::<CardState>().slots.iter().position(|a| matches!(a, Some(CardAction::Train(_)))).unwrap();
    app.world_mut().resource_mut::<CommandInbox>().pending.clear();
    press_slot(&mut app, slot);
    assert!(pending(&app).is_empty());
    let notices = &app.world().resource::<Notices>().list;
    assert_eq!(notices.last().map(|n| (n.player, n.text.as_str())), Some((Some(PlayerId(0)), "Need more houses")));
}

#[test]
fn hotkeys_open_build_pages_and_start_placement() {
    let mut app = hud_app();
    let a = spawn_unit_at(&mut app, 0, "peasant", Vec2::new(40.0, 40.0));
    let b = spawn_unit_at(&mut app, 0, "peasant", Vec2::new(42.0, 40.0));
    select(&mut app, &[a, b]);
    run_ticks(&mut app, 2);
    let info = app.world().resource::<SelectionInfo>();
    assert!(info.card.units && info.card.builders && info.card.gatherers);
    // W = Stop for both workers.
    tap(&mut app, KeyCode::KeyW);
    let cmds = pending(&app);
    assert_eq!(cmds.last().map(|c| (&c.kind, c.entities.len())), Some((&CommandKind::Stop, 2)));
    // Q = Move waits for a target.
    tap(&mut app, KeyCode::KeyQ);
    assert_eq!(*app.world().resource::<InputMode>(), InputMode::Target(TargetAction::Move));
    // Z = economy page; its first entry is the town centre, the house follows.
    tap(&mut app, KeyCode::KeyZ);
    app.update();
    let card = app.world().resource::<CardState>();
    assert_eq!(card.menu, CardMenu::Build(BuildCategory::Economy));
    let house = test_data().building_id("house").unwrap();
    let slot = card.slots.iter().position(|a| *a == Some(CardAction::Build(house))).unwrap();
    tap(&mut app, crate::card::HOTKEYS[slot]);
    assert_eq!(*app.world().resource::<InputMode>(), InputMode::Place { kind: house, facing: 0.0 });
    // B = Back to the root card.
    tap(&mut app, KeyCode::KeyB);
    app.update();
    assert_eq!(app.world().resource::<CardState>().menu, CardMenu::Root);
}

#[test]
fn r_is_left_to_placement_rotation() {
    let mut app = hud_app();
    let a = spawn_unit_at(&mut app, 0, "peasant", Vec2::new(40.0, 40.0));
    select(&mut app, &[a]);
    run_ticks(&mut app, 2);
    let house = test_data().building_id("house").unwrap();
    app.insert_resource(InputMode::Place { kind: house, facing: 1.0 });
    tap(&mut app, KeyCode::KeyR);
    // R would be Patrol on the root card; while placing it must do nothing.
    assert_eq!(*app.world().resource::<InputMode>(), InputMode::Place { kind: house, facing: 1.0 });
}

#[test]
fn multi_selection_narrows_to_a_type() {
    let mut app = hud_app();
    let p1 = spawn_unit_at(&mut app, 0, "peasant", Vec2::new(40.0, 40.0));
    let p2 = spawn_unit_at(&mut app, 0, "peasant", Vec2::new(42.0, 40.0));
    let scout = spawn_unit_at(&mut app, 0, "scout_rider", Vec2::new(44.0, 40.0));
    select(&mut app, &[p1, p2, scout]);
    run_ticks(&mut app, 2);
    let t = texts(&mut app);
    assert!(t.contains(&"Peasant x2".to_string()) && t.contains(&"Scout Rider x1".to_string()), "{t:?}");
    press_button(&mut app, "Scout Rider x1");
    app.update();
    let world = app.world_mut();
    let selected: Vec<Entity> = world.query_filtered::<Entity, With<Selected>>().iter(world).collect();
    assert_eq!(selected, vec![scout]);
}

#[test]
fn single_selection_shows_queue_and_cancels() {
    let mut app = hud_app();
    let data = test_data();
    let tc = spawn_building_at(&mut app, 0, "town_center", Vec2::new(40.0, 40.0));
    let peasant = data.unit_id("peasant").unwrap();
    let item = QueueItem { kind: QueueKind::Unit(peasant), paid: ResAmounts::ZERO, time: 10.0 };
    let mut queue = app.world_mut().get_mut::<ProductionQueue>(tc).unwrap();
    queue.items.extend([item, item, item]);
    queue.progress = 5.0;
    select(&mut app, &[tc]);
    run_ticks(&mut app, 2);
    let t = texts(&mut app);
    assert!(t.contains(&"Town Centre".to_string()), "{t:?}");
    assert!(t.contains(&"Peasant 50%".to_string()), "{t:?}");
    assert!(t.contains(&"2600 / 2600".to_string()), "{t:?}");
    // The peasant button shows how many are queued.
    let train = app.world().resource::<CardState>().slots.iter().position(|a| *a == Some(CardAction::Train(peasant))).unwrap();
    let world = app.world_mut();
    let count = world.query::<(&SlotCount, &Text)>().iter(world).find(|(c, _)| c.0 == train).map(|(_, t)| t.0.clone());
    assert_eq!(count.as_deref(), Some("3"));
    app.world_mut().resource_mut::<CommandInbox>().pending.clear();
    let world = app.world_mut();
    let third = world.query::<(Entity, &QueueSlot)>().iter(world).find(|(_, s)| s.0 == 2).map(|(e, _)| e).unwrap();
    press(&mut app, third);
    let cmds = pending(&app);
    assert_eq!(cmds.len(), 1);
    assert_eq!((&cmds[0].kind, &cmds[0].entities), (&CommandKind::CancelQueue { index: 2 }, &vec![tc]));
}

/// A town centre of player 0 queueing a peasant (half done), a spearman
/// and a scout rider, selected and drawn.
fn town_center_with_queue(app: &mut App) -> Entity {
    let data = test_data();
    let tc = spawn_building_at(app, 0, "town_center", Vec2::new(40.0, 40.0));
    let mut queue = app.world_mut().get_mut::<ProductionQueue>(tc).unwrap();
    for id in ["peasant", "spearman", "scout_rider"] {
        queue.items.push_back(QueueItem { kind: QueueKind::Unit(data.unit_id(id).unwrap()), paid: ResAmounts::ZERO, time: 10.0 });
    }
    queue.progress = 5.0;
    select(app, &[tc]);
    run_ticks(app, 2);
    let t = texts(app);
    assert!(t.contains(&"Peasant 50%".to_string()) && t.contains(&"Spearm.".to_string()) && t.contains(&"Scout.".to_string()), "{t:?}");
    app.world_mut().resource_mut::<CommandInbox>().pending.clear();
    tc
}

fn queue_slot(app: &mut App, slot: usize) -> Entity {
    let world = app.world_mut();
    world.query::<(Entity, &QueueSlot)>().iter(world).find(|(_, s)| s.0 == slot).map(|(e, _)| e).unwrap()
}

/// The front item finishes (as af_economy's production does) before the
/// panel redraws.
fn finish_front(app: &mut App, tc: Entity) {
    let mut queue = app.world_mut().get_mut::<ProductionQueue>(tc).unwrap();
    queue.items.pop_front();
    queue.progress = 0.0;
}

fn cancels(app: &App) -> Vec<CommandKind> {
    pending(app).into_iter().map(|c| c.kind).filter(|k| matches!(k, CommandKind::CancelQueue { .. })).collect()
}

#[test]
fn queue_clicks_cancel_the_item_drawn_after_the_queue_moved_on() {
    // The spearman drawn in slot 1 is now in front: it is the one cancelled
    // (not the scout rider that moved into slot 1).
    let mut app = hud_app();
    let tc = town_center_with_queue(&mut app);
    finish_front(&mut app, tc);
    let spearman = queue_slot(&mut app, 1);
    press(&mut app, spearman);
    assert_eq!(cancels(&app), vec![CommandKind::CancelQueue { index: 0 }]);
    // A click on the progress bar of the peasant that just finished cancels
    // nothing (not the spearman that started meanwhile).
    let mut app = hud_app();
    let tc = town_center_with_queue(&mut app);
    finish_front(&mut app, tc);
    let bar = queue_slot(&mut app, 0);
    press(&mut app, bar);
    assert_eq!(cancels(&app), vec![]);
}

#[test]
fn a_second_click_on_a_slot_being_cancelled_cancels_nothing_more() {
    // Paused, the first cancel waits in the inbox and the panel still shows
    // the spearman; clicking it again must not take the scout rider too.
    let mut app = hud_app();
    let tc = town_center_with_queue(&mut app);
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    let spearman = queue_slot(&mut app, 1);
    press(&mut app, spearman);
    app.world_mut().entity_mut(spearman).insert(Interaction::None);
    run_ticks(&mut app, 4);
    assert_eq!(app.world().get::<ProductionQueue>(tc).unwrap().items.len(), 3, "nothing runs while paused");
    press(&mut app, spearman);
    assert_eq!(cancels(&app), vec![CommandKind::CancelQueue { index: 1 }]);
    // The scout rider, clicked next, is the second item by the time it runs.
    let scout = queue_slot(&mut app, 2);
    press(&mut app, scout);
    assert_eq!(cancels(&app), vec![CommandKind::CancelQueue { index: 1 }, CommandKind::CancelQueue { index: 1 }]);
}

#[test]
fn notices_feed_shows_recent_notices() {
    let mut app = hud_app();
    let now = app.world().resource::<SimClock>().elapsed;
    let mut notices = app.world_mut().resource_mut::<Notices>();
    notices.push(Some(PlayerId(0)), NoticeKind::Alert, "Your town is under attack", None, now);
    notices.push(Some(PlayerId(1)), NoticeKind::Info, "Someone else's news", None, now);
    run_ticks(&mut app, 1);
    let t = texts(&mut app);
    assert!(t.contains(&"Your town is under attack".to_string()));
    assert!(!t.contains(&"Someone else's news".to_string()));
}

/// The minimap's pixels after its next redraw.
fn redrawn_minimap(app: &mut App) -> Vec<u8> {
    // Four redraws a second of real time; the test clock runs at 30 Hz.
    run_ticks(app, 10);
    let handle = app.world().resource::<Minimap>().image.clone();
    app.world().resource::<Assets<Image>>().get(&handle).unwrap().data.clone().unwrap()
}

/// RGB of the minimap pixel over a point of the 128 m test map.
fn rgb_at(pixels: &[u8], pos: Vec2) -> [u8; 3] {
    let px = MinimapGeometry::new(Vec2::splat(128.0), MINIMAP_RES).pixel(pos).unwrap();
    let i = ((px.y * MINIMAP_RES + px.x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

fn player_rgb(app: &App, player: usize) -> [u8; 3] {
    let c = app.world().resource::<Players>().list[player].color;
    [(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8]
}

/// Fog on: team 0 has explored the whole map but sees only the `lit` cells.
fn team0_fog(app: &mut App, lit: &[(usize, usize)]) {
    let mut visible = vec![0; 64 * 64];
    for (x, y) in lit {
        visible[y * 64 + x] = 1;
    }
    let teams = vec![TeamFog { visible, explored: vec![true; 64 * 64] }, TeamFog::default()];
    app.insert_resource(FogOfWar { enabled: true, cell: 2.0, width: 64, height: 64, teams });
}

#[test]
fn minimap_draws_units_in_player_colours() {
    let mut app = hud_app();
    let pos = Vec2::new(64.0, 64.0);
    spawn_unit_at(&mut app, 1, "scout_rider", pos);
    assert_eq!(rgb_at(&redrawn_minimap(&mut app), pos), player_rgb(&app, 1));
}

#[test]
fn hidden_enemies_stay_off_the_minimap() {
    let mut app = hud_app();
    let pos = Vec2::new(64.0, 64.0);
    spawn_unit_at(&mut app, 1, "scout_rider", pos);
    team0_fog(&mut app, &[]);
    assert_ne!(rgb_at(&redrawn_minimap(&mut app), pos), player_rgb(&app, 1));
}

#[test]
fn enemy_units_partly_in_sight_show_on_the_minimap() {
    let mut app = hud_app();
    // The centre is in fog cell (32, 32), out of sight; the left side of
    // the box reaches into the lit cell (31, 32).
    let pos = Vec2::new(64.2, 65.0);
    spawn_unit_at(&mut app, 1, "scout_rider", pos);
    team0_fog(&mut app, &[(31, 32)]);
    assert_eq!(rgb_at(&redrawn_minimap(&mut app), pos), player_rgb(&app, 1));
}

#[test]
fn scouted_buildings_and_ghosts_stay_on_the_minimap() {
    let mut app = hud_app();
    let (scouted, unscouted, razed) = (Vec2::new(30.0, 90.0), Vec2::new(90.0, 90.0), Vec2::new(90.0, 30.0));
    let seen = spawn_building_at(&mut app, 1, "house", scouted);
    let unseen = spawn_building_at(&mut app, 1, "house", unscouted);
    // Team 0 saw the first house once; only team 1 has seen the second.
    app.world_mut().entity_mut(seen).insert(SeenBy { teams: 0b01, visible: 0 });
    app.world_mut().entity_mut(unseen).insert(SeenBy { teams: 0b10, visible: 0 });
    // A third was razed while team 0 was not looking.
    let ghost = FogGhost {
        entity: Entity::PLACEHOLDER,
        kind: EntityKind::Building(test_data().building_id("house").unwrap()),
        owner: Some(PlayerId(1)),
        pos: razed,
        facing: 0.0,
        footprint: Footprint { half: Vec2::splat(2.0), height: 2.0 },
        teams: 0b01,
    };
    app.insert_resource(FogGhosts { list: vec![ghost] });
    team0_fog(&mut app, &[]);
    let pixels = redrawn_minimap(&mut app);
    let enemy = player_rgb(&app, 1);
    assert_eq!(rgb_at(&pixels, scouted), enemy);
    assert_ne!(rgb_at(&pixels, unscouted), enemy);
    assert_eq!(rgb_at(&pixels, razed), enemy);
}

#[test]
fn spectators_switch_players_and_get_no_card() {
    let mut app = hud_app();
    app.insert_resource(LocalPlayer(None));
    let tc = spawn_building_at(&mut app, 0, "town_center", Vec2::new(40.0, 40.0));
    select(&mut app, &[tc]);
    run_ticks(&mut app, 2);
    assert_eq!(*app.world().resource::<HudView>(), HudView { player: Some(PlayerId(0)), spectating: true });
    assert!(app.world().resource::<CardState>().slots.iter().all(Option::is_none));
    tap(&mut app, KeyCode::Tab);
    app.update();
    assert_eq!(app.world().resource::<HudView>().player, Some(PlayerId(1)));
}

#[test]
fn game_over_shows_result_and_returns_to_menu() {
    let mut app = hud_app();
    app.insert_resource(GameOutcome { ended: true, winning_team: Some(0), time: 125.0 });
    run_ticks(&mut app, 1);
    assert!(app.world().resource::<GameOverState>().blocking());
    let t = texts(&mut app);
    assert!(t.contains(&"VICTORY".to_string()), "{t:?}");
    assert!(t.contains(&"Won".to_string()) && t.contains(&"Lost".to_string()), "{t:?}");
    press_button(&mut app, "Back to menu");
    app.update();
    assert_eq!(*app.world().resource::<State<GameState>>().get(), GameState::Menu);
    let world = app.world_mut();
    assert_eq!(world.query::<&HudRoot>().iter(world).count(), 0);
}

/// Whether the full-screen panel marked `M` is shown.
fn panel_up<M: Component>(app: &mut App) -> bool {
    let world = app.world_mut();
    world.query_filtered::<&Node, With<M>>().single(world).unwrap().display != Display::None
}

/// Presses the button labelled `label` for one frame, then lets go.
fn click(app: &mut App, label: &str) {
    let e = button(app, label);
    press(app, e);
    app.world_mut().entity_mut(e).insert(Interaction::None);
}

fn state(app: &App) -> GameState {
    *app.world().resource::<State<GameState>>().get()
}

#[test]
fn keep_watching_still_leads_back_to_the_menu() {
    let mut app = hud_app();
    app.insert_resource(GameOutcome { ended: true, winning_team: Some(1), time: 300.0 });
    run_ticks(&mut app, 1);
    assert!(panel_up::<GameOverScreen>(&mut app));
    click(&mut app, "Keep watching");
    run_ticks(&mut app, 30);
    assert!(!panel_up::<GameOverScreen>(&mut app) && !app.world().resource::<GameOverState>().blocking());
    // F10 brings the screen back, with its way to the menu.
    tap(&mut app, KeyCode::F10);
    assert!(panel_up::<GameOverScreen>(&mut app) && app.world().resource::<GameOverState>().blocking());
    // So does the menu button (never the leave panel: the match is over).
    click(&mut app, "Keep watching");
    assert!(!panel_up::<GameOverScreen>(&mut app));
    click(&mut app, "Menu (F10)");
    assert!(panel_up::<GameOverScreen>(&mut app) && !panel_up::<LeavePanel>(&mut app));
    click(&mut app, "Back to menu");
    app.update();
    assert_eq!(state(&app), GameState::Menu);
}

#[test]
fn a_match_that_goes_on_can_be_left() {
    // A sandbox never ends, and a beaten player may wait on the others.
    let mut app = hud_app();
    run_ticks(&mut app, 2);
    assert!(!panel_up::<LeavePanel>(&mut app) && !panel_up::<GameOverScreen>(&mut app));
    click(&mut app, "Menu (F10)");
    assert!(panel_up::<LeavePanel>(&mut app) && app.world().resource::<GameOverState>().blocking());
    click(&mut app, "Resume");
    assert!(!panel_up::<LeavePanel>(&mut app) && !app.world().resource::<GameOverState>().blocking());
    tap(&mut app, KeyCode::F10);
    assert!(panel_up::<LeavePanel>(&mut app));
    tap(&mut app, KeyCode::F10);
    assert!(!panel_up::<LeavePanel>(&mut app), "F10 again closes it");
    assert_eq!(state(&app), GameState::Playing);
    // The match ending meanwhile puts the game-over screen in its place.
    tap(&mut app, KeyCode::F10);
    app.insert_resource(GameOutcome { ended: true, winning_team: Some(1), time: 300.0 });
    app.update();
    assert!(panel_up::<GameOverScreen>(&mut app) && !panel_up::<LeavePanel>(&mut app));
    // Otherwise "Leave to menu" leaves.
    let mut app = hud_app();
    tap(&mut app, KeyCode::F10);
    click(&mut app, "Leave to menu");
    app.update();
    assert_eq!(state(&app), GameState::Menu);
    let world = app.world_mut();
    assert_eq!(world.query::<&HudRoot>().iter(world).count(), 0);
}

#[test]
fn selection_panel_tells_no_more_than_the_fog_shows() {
    let mut app = hud_app();
    let data = test_data();
    // Team 0 has explored everything but sees only the cell at (40..42, 40..42).
    team0_fog(&mut app, &[(20, 20)]);
    let oak = data.node_id("oak").unwrap();
    let tree = with_commands(&mut app, |c, data, _| spawn_node(c, data, oak, Vec2::new(90.0, 90.0), 0.0, Some(57.0)));
    app.world_mut().entity_mut(tree).insert(SeenBy { teams: 0b01, visible: 0 });
    select(&mut app, &[tree]);
    run_ticks(&mut app, 2);
    // Out of sight, gathering at the tree stays hidden (af_render keeps it at its last-seen size).
    let t = texts(&mut app);
    assert!(t.contains(&"Wood: out of sight".to_string()), "{t:?}");
    assert!(!t.iter().any(|s| s.contains("57")), "{t:?}");
    app.world_mut().entity_mut(tree).insert(SeenBy { teams: 0b01, visible: 0b01 });
    run_ticks(&mut app, 4);
    let t = texts(&mut app);
    assert!(t.contains(&"Wood: 57 / 130 left".to_string()), "{t:?}");
    app.world_mut().entity_mut(tree).remove::<Selected>();

    // A visible enemy worker on its way to a foundation in the fog.
    let builder = spawn_unit_at(&mut app, 1, "peasant", Vec2::new(41.0, 41.0));
    let barracks = data.building_id("barracks").unwrap();
    let site = with_commands(&mut app, |c, data, players| spawn_building(c, data, &players.list[1], barracks, Vec2::new(90.0, 30.0), 0.0, false));
    app.world_mut().get_mut::<Orders>(builder).unwrap().set(Order::Build { site }, false);
    select(&mut app, &[builder]);
    run_ticks(&mut app, 4);
    let order = |app: &mut App| {
        let t = texts(app);
        let status = t.iter().find(|s| s.starts_with("Building")).unwrap_or_else(|| panic!("no order in {t:?}"));
        status.split("  |").next().unwrap().to_string()
    };
    assert_eq!(order(&mut app), "Building", "a site nobody on team 0 has seen");
    // Scouted once, while it was a house plot: named as remembered.
    let mut last = LastSeen::default();
    last.record(0b01, BuildingSighting { kind: data.building_id("house").unwrap(), construction: Some(0.1), hp: 1.0 });
    app.world_mut().entity_mut(site).insert((SeenBy { teams: 0b01, visible: 0 }, last));
    run_ticks(&mut app, 4);
    assert_eq!(order(&mut app), "Building House");
    // In sight: what it is now.
    app.world_mut().entity_mut(site).insert(SeenBy { teams: 0b01, visible: 0b01 });
    run_ticks(&mut app, 4);
    assert_eq!(order(&mut app), format!("Building {}", data.building(barracks).name));
    // Nothing is hidden from a spectator.
    app.world_mut().entity_mut(site).insert(SeenBy::default());
    app.insert_resource(LocalPlayer(None));
    run_ticks(&mut app, 4);
    assert_eq!(order(&mut app), format!("Building {}", data.building(barracks).name));
}

#[test]
fn over_ui_follows_the_hover_map() {
    let mut app = hud_app();
    let node = app.world_mut().spawn(Node::default()).id();
    let mut hits = EntityHashMap::default();
    hits.insert(node, HitData::new(Entity::PLACEHOLDER, 0.0, None, None));
    let mut hover = HoverMap::default();
    hover.insert(PointerId::Mouse, hits);
    app.insert_resource(hover);
    app.update();
    assert!(app.world().resource::<CursorWorld>().over_ui);
    // A stand-in mouse points at the world, whatever the real one is over.
    app.insert_resource(af_core::ext::input::CursorOverride(Some(Vec2::new(400.0, 300.0))));
    app.update();
    assert!(!app.world().resource::<CursorWorld>().over_ui);
    app.insert_resource(af_core::ext::input::CursorOverride(None));
    app.update();
    assert!(app.world().resource::<CursorWorld>().over_ui);
    app.insert_resource(HoverMap::default());
    app.update();
    assert!(!app.world().resource::<CursorWorld>().over_ui);
}

#[test]
fn menu_start_builds_the_match() {
    let mut app = App::new();
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins((MinimalPlugins, StatesPlugin));
    app.insert_state(GameState::Menu);
    app.insert_resource(Data(test_data()));
    app.add_plugins(af_core::CorePlugin);
    add_client_resources(&mut app);
    app.update();
    let t = texts(&mut app);
    assert!(t.contains(&"AETHERFALL".to_string()));
    for f in &test_data().factions {
        assert!(t.contains(&f.name), "missing card for {}", f.id);
    }
    press_button(&mut app, "+");
    press_button(&mut app, "Start");
    app.update();
    let setup = app.world().resource::<MatchSetup>();
    assert_eq!(setup.players.len(), 3);
    assert_eq!(setup.local_player, Some(0));
    assert_eq!(*app.world().resource::<State<GameState>>().get(), GameState::Playing);
}

#[test]
fn control_group_bar_selects_a_group() {
    let mut app = hud_app();
    let a = spawn_unit_at(&mut app, 0, "spearman", Vec2::new(40.0, 40.0));
    let b = spawn_unit_at(&mut app, 0, "spearman", Vec2::new(42.0, 40.0));
    let other = spawn_unit_at(&mut app, 0, "peasant", Vec2::new(44.0, 40.0));
    let mut groups = af_core::ext::input::ControlGroups::default();
    groups.set(3, &[a, b]);
    app.insert_resource(groups);
    select(&mut app, &[other]);
    run_ticks(&mut app, 2);
    assert!(texts(&mut app).contains(&"3  x2".to_string()));
    press_button(&mut app, "3  x2");
    app.update();
    let world = app.world_mut();
    let mut selected: Vec<Entity> = world.query_filtered::<Entity, With<Selected>>().iter(world).collect();
    let mut expected = vec![a, b];
    selected.sort();
    expected.sort();
    assert_eq!(selected, expected);
}

/// The minimap image node.
fn minimap_view(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, With<RelativeCursorPosition>>().single(world).unwrap()
}

/// Moves the mouse to a `RelativeCursorPosition`-style point of the
/// minimap (centre origin, +-0.5 at its edges), with `top` the UI node the
/// picking backend reports under the cursor.
fn move_mouse(app: &mut App, normalized: Vec2, top: Option<Entity>) {
    let view = minimap_view(app);
    let cursor_over = normalized.abs().max_element() <= 0.5;
    app.world_mut().entity_mut(view).insert(RelativeCursorPosition { cursor_over, normalized: Some(normalized) });
    let mut hover = HoverMap::default();
    if let Some(top) = top {
        let mut hits = EntityHashMap::default();
        hits.insert(top, HitData::new(Entity::PLACEHOLDER, 0.0, None, None));
        hover.insert(PointerId::Mouse, hits);
    }
    app.insert_resource(hover);
}

fn mouse(app: &mut App) -> Mut<'_, ButtonInput<MouseButton>> {
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>()
}

/// Clicks at a minimap point with `top` under the cursor (press, release,
/// two frames).
fn click_minimap_under(app: &mut App, normalized: Vec2, button: MouseButton, top: Option<Entity>) {
    move_mouse(app, normalized, top);
    mouse(app).press(button);
    app.update();
    // What InputPlugin does between frames.
    mouse(app).clear();
    mouse(app).release(button);
    app.update();
    mouse(app).clear();
}

/// Clicks the minimap itself at a point.
fn click_minimap(app: &mut App, normalized: Vec2, button: MouseButton) {
    let view = minimap_view(app);
    click_minimap_under(app, normalized, button, Some(view));
}

fn focus(app: &App) -> Vec2 {
    app.world().resource::<CameraRig>().focus
}

/// Commands issued recently: executing this tick or still waiting.
fn issued(app: &App) -> Vec<PlayerCommand> {
    let w = app.world();
    w.resource::<TickCommands>().list.iter().chain(&w.resource::<CommandInbox>().pending).cloned().collect()
}

#[test]
fn minimap_clicks_move_the_camera_and_command() {
    let mut app = hud_app();
    let unit = spawn_unit_at(&mut app, 0, "spearman", Vec2::new(40.0, 40.0));
    select(&mut app, &[unit]);
    run_ticks(&mut app, 2);
    // Left click jumps the camera (the 128 m map fills the minimap).
    click_minimap(&mut app, Vec2::new(0.25, -0.25), MouseButton::Left);
    assert_eq!(focus(&app), Vec2::new(96.0, 32.0));
    // Right click sends the selection there.
    click_minimap(&mut app, Vec2::ZERO, MouseButton::Right);
    let smart = CommandKind::Smart { point: Vec2::splat(64.0), target: None };
    assert!(issued(&app).iter().any(|c| c.kind == smart && c.entities == vec![unit]), "{:?}", issued(&app));
    // A pending attack-move completes on the minimap.
    app.insert_resource(InputMode::Target(TargetAction::AttackMove));
    click_minimap(&mut app, Vec2::new(-0.5, -0.5), MouseButton::Left);
    assert_eq!(*app.world().resource::<InputMode>(), InputMode::Normal);
    assert!(issued(&app).iter().any(|c| c.kind == CommandKind::AttackMove(Vec2::ZERO)));
    // A right click with a mode pending only cancels the mode.
    app.insert_resource(InputMode::Target(TargetAction::Patrol));
    click_minimap(&mut app, Vec2::ZERO, MouseButton::Right);
    assert_eq!(*app.world().resource::<InputMode>(), InputMode::Normal);
    assert!(issued(&app).is_empty(), "{:?}", issued(&app));
    // Clicks on UI drawn over the minimap (a tooltip, say) belong to that UI.
    let tip = app.world_mut().spawn(Node::default()).id();
    click_minimap_under(&mut app, Vec2::new(-0.25, 0.25), MouseButton::Left, Some(tip));
    click_minimap_under(&mut app, Vec2::new(-0.25, 0.25), MouseButton::Right, Some(tip));
    assert_eq!(focus(&app), Vec2::new(96.0, 32.0));
    assert!(issued(&app).is_empty(), "{:?}", issued(&app));
}

#[test]
fn minimap_tap_within_one_frame_moves_the_camera_once() {
    let mut app = hud_app();
    let view = minimap_view(&mut app);
    move_mouse(&mut app, Vec2::new(0.25, -0.25), Some(view));
    // Touchpad tap-to-click or a slow frame: press and release land together.
    mouse(&mut app).press(MouseButton::Left);
    mouse(&mut app).release(MouseButton::Left);
    app.update();
    mouse(&mut app).clear();
    assert_eq!(focus(&app), Vec2::new(96.0, 32.0));
    // The next press elsewhere (a box-select in the world) and its drag
    // leave the camera alone.
    move_mouse(&mut app, Vec2::new(1.4, -0.9), None);
    mouse(&mut app).press(MouseButton::Left);
    app.update();
    mouse(&mut app).clear();
    move_mouse(&mut app, Vec2::new(2.0, -0.6), None);
    app.update();
    assert_eq!(focus(&app), Vec2::new(96.0, 32.0));
}

#[test]
fn minimap_ignores_the_mouse_under_the_game_over_screen() {
    let mut app = hud_app();
    let unit = spawn_unit_at(&mut app, 0, "spearman", Vec2::new(40.0, 40.0));
    select(&mut app, &[unit]);
    run_ticks(&mut app, 2);
    // A camera drag on the minimap is under way when the match ends.
    let view = minimap_view(&mut app);
    move_mouse(&mut app, Vec2::ZERO, Some(view));
    mouse(&mut app).press(MouseButton::Left);
    app.update();
    mouse(&mut app).clear();
    assert_eq!(focus(&app), Vec2::splat(64.0));
    app.insert_resource(GameOutcome { ended: true, winning_team: Some(0), time: 60.0 });
    app.update();
    let world = app.world_mut();
    let screen = world.query_filtered::<Entity, With<GameOverScreen>>().single(world).unwrap();
    // The screen ends the drag.
    move_mouse(&mut app, Vec2::new(0.25, 0.25), Some(screen));
    app.update();
    assert_eq!(focus(&app), Vec2::splat(64.0));
    mouse(&mut app).release(MouseButton::Left);
    app.update();
    mouse(&mut app).clear();
    // Clicks on the screen above the minimap neither jump nor command.
    app.world_mut().resource_mut::<CommandInbox>().pending.clear();
    click_minimap_under(&mut app, Vec2::new(-0.25, 0.25), MouseButton::Left, Some(screen));
    click_minimap_under(&mut app, Vec2::new(-0.25, 0.25), MouseButton::Right, Some(screen));
    assert_eq!(focus(&app), Vec2::splat(64.0));
    assert!(!issued(&app).iter().any(|c| matches!(c.kind, CommandKind::Smart { .. })), "{:?}", issued(&app));
}

#[test]
fn instant_buttons_cancel_a_pending_target() {
    let mut app = hud_app();
    let worker = spawn_unit_at(&mut app, 0, "peasant", Vec2::new(40.0, 40.0));
    select(&mut app, &[worker]);
    run_ticks(&mut app, 2);
    let mode = |app: &App| *app.world().resource::<InputMode>();
    // Q (Move) waits for a target click; W (Stop) instead must disarm it.
    tap(&mut app, KeyCode::KeyQ);
    assert_eq!(mode(&app), InputMode::Target(TargetAction::Move));
    tap(&mut app, KeyCode::KeyW);
    assert_eq!(mode(&app), InputMode::Normal);
    let cmds = pending(&app);
    assert_eq!(cmds.last().map(|c| &c.kind), Some(&CommandKind::Stop));
    // So do a stance (A) and opening a build page (Z).
    for key in [KeyCode::KeyA, KeyCode::KeyZ] {
        tap(&mut app, KeyCode::KeyQ);
        assert_eq!(mode(&app), InputMode::Target(TargetAction::Move));
        tap(&mut app, key);
        assert_eq!(mode(&app), InputMode::Normal, "{key:?}");
    }
}

#[test]
fn build_page_stays_open_until_the_card_changes() {
    let mut app = hud_app();
    let workers: Vec<Entity> = (0..3).map(|i| spawn_unit_at(&mut app, 0, "peasant", Vec2::new(40.0 + 2.0 * i as f32, 40.0))).collect();
    select(&mut app, &workers);
    run_ticks(&mut app, 2);
    let economy = CardMenu::Build(BuildCategory::Economy);
    tap(&mut app, KeyCode::KeyZ);
    app.update();
    assert_eq!(app.world().resource::<CardState>().menu, economy);
    // A selected worker dies: the selection changes, but it still builds.
    app.world_mut().entity_mut(workers[2]).insert(Dead);
    app.update();
    let selected = &app.world().resource::<SelectionInfo>().entities;
    assert!(selected.len() == 2 && !selected.contains(&workers[2]), "{selected:?}");
    assert_eq!(app.world().resource::<CardState>().menu, economy);
    // A town centre has a card of its own.
    let tc = spawn_building_at(&mut app, 0, "town_center", Vec2::new(80.0, 80.0));
    for e in &workers[..2] {
        app.world_mut().entity_mut(*e).remove::<Selected>();
    }
    select(&mut app, &[tc]);
    app.update();
    assert_eq!(app.world().resource::<CardState>().menu, CardMenu::Root);
}

#[test]
fn the_open_build_page_is_reported_and_closes_on_request() {
    use af_core::ext::ui::CardPage;
    let mut app = hud_app();
    let worker = spawn_unit_at(&mut app, 0, "peasant", Vec2::new(40.0, 40.0));
    select(&mut app, &[worker]);
    run_ticks(&mut app, 2);
    assert_eq!(*app.world().resource::<CardPage>(), CardPage::default());
    tap(&mut app, KeyCode::KeyZ);
    app.update();
    assert!(app.world().resource::<CardPage>().open, "the economy page is open");
    // af_input's Esc asks for it to close.
    app.world_mut().resource_mut::<CardPage>().close = true;
    app.update();
    assert_eq!(app.world().resource::<CardState>().menu, CardMenu::Root);
    assert_eq!(*app.world().resource::<CardPage>(), CardPage::default(), "closed and acknowledged");
    assert_eq!(app.world().resource::<SelectionInfo>().entities, vec![worker], "the worker stays selected");
}

#[test]
fn top_bar_names_its_values() {
    let mut app = hud_app();
    let t = texts(&mut app);
    for name in ["Food", "Wood", "Stone", "Copper", "Iron", "Silicon", "Aether", "Adamant", "Pop"] {
        assert!(t.iter().any(|s| s == name), "{name} missing from {t:?}");
    }
}

/// A match on a map with features opens with a notice naming the map.
#[test]
fn the_match_opens_with_the_map_name() {
    use af_core::ext::map::{MapFeatures, MapSize, ResolvedMapOptions, SiteKind, WaterLevel};
    let mut app = sim_app(test_data());
    add_client_resources(&mut app);
    flat_world(&mut app, 128.0, &[("ironhold", 0), ("dune", 1)]);
    app.insert_resource(LocalPlayer(Some(PlayerId(0))));
    app.insert_resource(MapFeatures {
        options: ResolvedMapOptions { size: MapSize::Large, water: WaterLevel::Lakes, rivers: 2, site: SiteKind::Shattered },
        land_min: Vec2::ZERO,
        land_max: Vec2::splat(128.0),
        ocean_edges: Vec::new(),
        sites: Vec::new(),
        rivers: Vec::new(),
        mountains: Vec::new(),
    });
    run_ticks(&mut app, 2);
    let notices = &app.world().resource::<Notices>().list;
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!((notices[0].player, notices[0].kind), (None, NoticeKind::Info), "for everyone");
    assert!(texts(&mut app).contains(&"Large - Lakes - 2 rivers - The Shattered Fall".to_string()));
    // Without features (today's maps, the test world) there is no such notice.
    let mut plain = hud_app();
    run_ticks(&mut plain, 2);
    assert!(plain.world().resource::<Notices>().list.is_empty());
}

#[test]
fn the_hud_hides_for_clean_screenshots_and_comes_back() {
    use af_core::ext::ui::HudHidden;
    let mut app = hud_app();
    let root = |app: &mut App| {
        let world = app.world_mut();
        *world.query_filtered::<&Visibility, With<HudRoot>>().single(world).unwrap()
    };
    assert_eq!(root(&mut app), Visibility::Inherited);
    app.insert_resource(HudHidden(true));
    app.update();
    assert_eq!(root(&mut app), Visibility::Hidden);
    app.insert_resource(HudHidden(false));
    app.update();
    assert_eq!(root(&mut app), Visibility::Inherited);
}

/// The menu's map keys change the options, and Start hands them over.
#[test]
fn menu_map_keys_reach_the_match() {
    use af_core::ext::map::{MapOptions, MapSize, RiverCount, SiteChoice, SiteKind, WaterLevel};
    let mut app = App::new();
    app.set_error_handler(bevy::ecs::error::warn);
    app.add_plugins((MinimalPlugins, StatesPlugin));
    app.insert_state(GameState::Menu);
    app.insert_resource(Data(test_data()));
    app.add_plugins(af_core::CorePlugin);
    add_client_resources(&mut app);
    app.update();
    for key in [KeyCode::KeyM, KeyCode::KeyW, KeyCode::KeyR, KeyCode::KeyR, KeyCode::KeyC] {
        tap(&mut app, key);
    }
    // Shift+C steps the site back again; a second C moves on to the Crater.
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ShiftLeft);
    tap(&mut app, KeyCode::KeyC);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::ShiftLeft);
    tap(&mut app, KeyCode::KeyC);
    let t = texts(&mut app);
    for shown in ["Large", "Coast: one sea edge", "Two", "Starfall Crater"] {
        assert!(t.contains(&shown.to_string()), "{shown} missing from {t:?}");
    }
    tap(&mut app, KeyCode::Enter);
    app.update();
    let want = MapOptions { size: MapSize::Large, water: WaterLevel::Coast, rivers: RiverCount::Two, site: SiteChoice::Kind(SiteKind::Crater) };
    assert_eq!(app.world().get_resource::<MapOptions>(), Some(&want));
    assert_eq!(*app.world().resource::<State<GameState>>().get(), GameState::Playing);
}
