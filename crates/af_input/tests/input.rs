//! App-level tests: real content, the af_core test world, and input fed
//! through `ButtonInput` / `CursorWorld` (and a fake window + camera for
//! picking and drag selection).

use af_core::ext::input::{ClickMark, ControlGroups, OrderMark, PlacementPreview, RtsCamera, ShowPaths};
use af_core::prelude::*;
use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};
use af_input::{rig_transform, view_axes, RtsInputPlugin, FOV_DEG};
use bevy::camera::{CameraProjection, RenderTargetInfo};
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

const SCREEN: Vec2 = Vec2::new(1280.0, 720.0);

struct Match {
    app: App,
    me: PlayerId,
    enemy: PlayerId,
}

impl Match {
    fn new(spectate: bool) -> Match {
        let mut app = sim_app(test_data());
        app.add_plugins(RtsInputPlugin);
        let ids = flat_world(&mut app, 128.0, &[("ironhold", 0), ("dune", 1)]);
        if !spectate {
            app.world_mut().insert_resource(LocalPlayer(Some(ids[0])));
        }
        run_ticks(&mut app, 1);
        Match { app, me: ids[0], enemy: ids[1] }
    }

    fn data(&self) -> std::sync::Arc<GameData> {
        self.app.world().resource::<Data>().0.clone()
    }

    fn faction(&self, p: PlayerId) -> FactionId {
        self.app.world().resource::<Players>().get(p).faction
    }

    /// The faction's worker type.
    fn worker_kind(&self, p: PlayerId) -> UnitTypeId {
        let data = self.data();
        data.faction(self.faction(p)).worker_id
    }

    /// A ground combat unit type of the faction (no gatherer, no flying).
    fn soldier_kind(&self, p: PlayerId) -> UnitTypeId {
        let data = self.data();
        let f = data.faction(self.faction(p));
        f.unit_ids.iter().copied().find(|u| data.unit(*u).gatherer.is_none() && !data.unit(*u).is_flying()).expect("a ground soldier")
    }

    /// A buildable age-1 building of the faction with `tag` (no wall, no node).
    fn building_kind(&self, p: PlayerId, tag: Tag) -> BuildingTypeId {
        let data = self.data();
        let players = self.app.world().resource::<Players>();
        rules::build_menu(&data, players.get(p))
            .into_iter()
            .find(|b| {
                let d = data.building(*b);
                d.has_tag(tag) && !d.wall && d.requires_node_id.is_none() && d.age == 1
            })
            .expect("building with tag")
    }

    fn wall_kind(&self, p: PlayerId) -> BuildingTypeId {
        let data = self.data();
        let players = self.app.world().resource::<Players>();
        rules::build_menu(&data, players.get(p)).into_iter().find(|b| data.building(*b).wall && !data.building(*b).gate).expect("a wall")
    }

    fn unit(&mut self, owner: PlayerId, kind: UnitTypeId, pos: Vec2) -> Entity {
        with_commands(&mut self.app, |c, data, players| spawn_unit(c, data, players.get(owner), kind, pos, 0.0))
    }

    fn building(&mut self, owner: PlayerId, kind: BuildingTypeId, pos: Vec2) -> Entity {
        with_commands(&mut self.app, |c, data, players| spawn_building(c, data, players.get(owner), kind, pos, 0.0, true))
    }

    /// Run one tick so the spatial index and counts include new entities.
    fn settle(&mut self) {
        run_ticks(&mut self.app, 1);
    }

    /// One frame; then forget this frame's presses (InputPlugin is absent).
    fn step(&mut self) {
        self.app.update();
        let world = self.app.world_mut();
        world.resource_mut::<ButtonInput<KeyCode>>().clear();
        world.resource_mut::<ButtonInput<MouseButton>>().clear();
    }

    fn hold(&mut self, key: KeyCode) {
        self.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
    }

    fn release(&mut self, key: KeyCode) {
        self.app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(key);
    }

    fn tap(&mut self, key: KeyCode) {
        self.hold(key);
        self.step();
        self.release(key);
        self.step();
    }

    fn tap_with(&mut self, modifier: KeyCode, key: KeyCode) {
        self.hold(modifier);
        self.tap(key);
        self.release(modifier);
        self.step();
    }

    fn click(&mut self, button: MouseButton) {
        self.press_mouse(button);
        self.step();
        self.release_mouse(button);
        self.step();
    }

    /// Point the (headless) cursor at a ground spot and maybe an entity.
    fn point(&mut self, ground: Vec2, hovered: Option<Entity>) {
        let mut c = self.app.world_mut().resource_mut::<CursorWorld>();
        c.screen = Some(SCREEN * 0.5);
        c.ground = Some(ground);
        c.hovered = hovered;
    }

    fn click_on(&mut self, ground: Vec2, hovered: Option<Entity>) {
        self.point(ground, hovered);
        self.click(MouseButton::Left);
    }

    fn select(&mut self, entities: &[Entity]) {
        for e in entities {
            self.app.world_mut().entity_mut(*e).insert(Selected);
        }
    }

    fn selected(&mut self) -> Vec<Entity> {
        let world = self.app.world_mut();
        let mut q = world.query_filtered::<Entity, With<Selected>>();
        let mut out: Vec<Entity> = q.iter(world).collect();
        out.sort();
        out
    }

    fn set_mode(&mut self, mode: InputMode) {
        *self.app.world_mut().resource_mut::<InputMode>() = mode;
    }

    fn mode(&self) -> InputMode {
        *self.app.world().resource::<InputMode>()
    }

    fn rig(&self) -> CameraRig {
        *self.app.world().resource::<CameraRig>()
    }

    /// Commands issued in the last couple of frames (still in the inbox or
    /// just drained into this tick's list).
    fn issued(&self) -> Vec<PlayerCommand> {
        let w = self.app.world();
        w.resource::<TickCommands>().list.iter().chain(w.resource::<CommandInbox>().pending.iter()).cloned().collect()
    }

    fn notices(&self) -> Vec<String> {
        self.app.world().resource::<Notices>().list.iter().map(|n| n.text.clone()).collect()
    }

    fn pings(&mut self) -> Vec<ClickPing> {
        let world = self.app.world_mut();
        let mut q = world.query::<&ClickPing>();
        q.iter(world).copied().collect()
    }

    /// Click markers and the order each one shows (nothing despawns them
    /// here: af_render is absent).
    fn marks(&mut self) -> Vec<(ClickPing, ClickMark)> {
        let world = self.app.world_mut();
        let mut q = world.query::<(&ClickPing, &ClickMark)>();
        q.iter(world).map(|(p, k)| (*p, *k)).collect()
    }

    /// The order marked at `pos` (the only marker there).
    fn mark_at(&mut self, pos: Vec2) -> ClickMark {
        let found: Vec<(ClickPing, ClickMark)> = self.marks().into_iter().filter(|(p, _)| p.pos == pos).collect();
        assert_eq!(found.len(), 1, "one marker at {pos}: {found:?}");
        let (ping, mark) = found[0];
        assert_eq!(ping.color, mark.kind.color(), "the ping is coloured for its order");
        assert_eq!(ping.time_left, mark.life());
        mark
    }

    /// Forget the markers made so far.
    fn clear_marks(&mut self) {
        let world = self.app.world_mut();
        let pings: Vec<Entity> = world.query_filtered::<Entity, With<ClickPing>>().iter(world).collect();
        for e in pings {
            world.despawn(e);
        }
    }

    /// Give the app a 1280x720 window and make the RTS camera project like
    /// a rendered one would (camera_system is absent headless).
    fn with_view(&mut self) {
        let world = self.app.world_mut();
        world.spawn((Window::default(), PrimaryWindow));
        let mut cameras = world.query_filtered::<&mut Camera, With<RtsCamera>>();
        let mut camera = cameras.single_mut(world).expect("the RTS camera exists while playing");
        let projection = PerspectiveProjection { fov: FOV_DEG.to_radians(), aspect_ratio: SCREEN.x / SCREEN.y, near: 0.5, far: 2000.0, ..default() };
        camera.computed.clip_from_view = projection.get_clip_from_view();
        camera.computed.target_info = Some(RenderTargetInfo { physical_size: SCREEN.as_uvec2(), scale_factor: 1.0 });
    }

    fn move_cursor(&mut self, screen: Vec2) {
        let world = self.app.world_mut();
        let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows.single_mut(world).unwrap().set_cursor_position(Some(screen));
    }

    /// Where a ground-level world point appears on screen right now.
    fn screen_of(&mut self, ground: Vec2) -> Vec2 {
        let view = GlobalTransform::from(rig_transform(&self.rig()));
        let world = self.app.world_mut();
        let mut cameras = world.query_filtered::<&Camera, With<RtsCamera>>();
        let camera = cameras.single(world).unwrap();
        camera.world_to_viewport(&view, Vec3::new(ground.x, 0.5, ground.y)).expect("on screen")
    }

    fn fog_hides_everything(&mut self) {
        let n = 64 * 64;
        let team = TeamFog { visible: vec![0; n], explored: vec![false; n] };
        self.app.world_mut().insert_resource(FogOfWar { enabled: true, cell: 2.0, width: 64, height: 64, teams: vec![team.clone(), team] });
    }

    /// Dark fog except the cells containing `points`.
    fn fog_lighting(&mut self, points: &[Vec2]) {
        self.fog_hides_everything();
        let mut fog = self.app.world_mut().resource_mut::<FogOfWar>();
        for p in points {
            let i = fog_cell(&fog, *p);
            for team in &mut fog.teams {
                team.visible[i] = 1;
                team.explored[i] = true;
            }
        }
    }

    fn set_resources(&mut self, p: PlayerId, amounts: ResAmounts) {
        self.app.world_mut().resource_mut::<Players>().list[p.index()].resources = amounts;
    }

    fn press_mouse(&mut self, button: MouseButton) {
        self.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(button);
    }

    fn release_mouse(&mut self, button: MouseButton) {
        self.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(button);
    }
}

/// Row-major index of the fog cell containing `p`.
fn fog_cell(fog: &FogOfWar, p: Vec2) -> usize {
    (p.y / fog.cell) as usize * fog.width as usize + (p.x / fog.cell) as usize
}

// ---------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------

#[test]
fn camera_lives_only_while_playing_and_starts_at_home() {
    let mut m = Match::new(false);
    let mut cameras = m.app.world_mut().query_filtered::<(), With<RtsCamera>>();
    assert_eq!(cameras.iter(m.app.world()).count(), 1);
    let start = m.app.world().resource::<Players>().get(m.me).start_pos;
    assert_eq!(m.rig().focus, start);

    m.app.world_mut().resource_mut::<Time<Virtual>>().set_relative_speed(4.0);
    m.app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Menu);
    m.step();
    m.step();
    assert_eq!(cameras.iter(m.app.world()).count(), 0);
    assert_eq!(m.app.world().resource::<Time<Virtual>>().relative_speed(), 1.0, "speed reset for the next match");
}

#[test]
fn arrows_pan_and_wheel_zooms_within_limits() {
    let mut m = Match::new(false);
    let before = m.rig();
    let (forward, right) = view_axes(before.yaw);
    m.hold(KeyCode::ArrowUp);
    for _ in 0..5 {
        m.step();
    }
    m.release(KeyCode::ArrowUp);
    let moved = m.rig().focus - before.focus;
    assert!(moved.dot(forward) > 5.0, "pans forward: {moved}");
    assert!(moved.dot(right).abs() < 1e-3);

    m.app.world_mut().insert_resource(AccumulatedMouseScroll { unit: MouseScrollUnit::Line, delta: Vec2::new(0.0, 30.0) });
    m.step();
    assert_eq!(m.rig().distance, af_input::MIN_DISTANCE);
    m.app.world_mut().insert_resource(AccumulatedMouseScroll { unit: MouseScrollUnit::Line, delta: Vec2::new(0.0, -2.0) });
    m.step();
    assert!(m.rig().distance > af_input::MIN_DISTANCE);

    // A distance set from elsewhere (a dev close-up) holds until the wheel turns.
    m.app.world_mut().insert_resource(AccumulatedMouseScroll::default());
    m.app.world_mut().resource_mut::<CameraRig>().distance = 8.0;
    m.step();
    assert_eq!(m.rig().distance, 8.0);
}

#[test]
fn camera_works_while_paused_and_stays_on_the_map() {
    let mut m = Match::new(false);
    m.tap(KeyCode::F3);
    assert!(m.app.world().resource::<Time<Virtual>>().is_paused());
    let tick = m.app.world().resource::<SimClock>().tick;
    m.hold(KeyCode::ArrowLeft);
    m.hold(KeyCode::ArrowDown);
    for _ in 0..200 {
        m.step();
    }
    assert_eq!(m.app.world().resource::<SimClock>().tick, tick, "the sim is frozen");
    let focus = m.rig().focus;
    assert!(focus.x >= 0.0 && focus.y >= 0.0 && focus.x <= 128.0 && focus.y <= 128.0, "{focus}");
    assert!(focus != m.app.world().resource::<Players>().get(m.me).start_pos, "the camera moved while paused");
    m.tap(KeyCode::Pause);
    assert!(!m.app.world().resource::<Time<Virtual>>().is_paused());
}

#[test]
fn screen_edge_scrolls() {
    let mut m = Match::new(false);
    m.with_view();
    let before = m.rig().focus;
    let (_, right) = view_axes(m.rig().yaw);
    m.move_cursor(Vec2::new(1.0, SCREEN.y * 0.5));
    for _ in 0..3 {
        m.step();
    }
    assert!((m.rig().focus - before).dot(right) < -1.0, "left edge pans left");
}

#[test]
fn middle_drag_grabs_the_ground() {
    let mut m = Match::new(false);
    m.with_view();
    let before = m.rig().focus;
    let (forward, right) = view_axes(m.rig().yaw);
    let centre = SCREEN * 0.5;
    m.move_cursor(centre);
    m.press_mouse(MouseButton::Middle);
    m.step();
    assert_eq!(m.rig().focus, before, "pressing does not move the view");
    m.move_cursor(centre + Vec2::new(100.0, 0.0));
    m.step();
    let moved = m.rig().focus - before;
    assert!(moved.dot(right) < -1.0, "dragging right moves the view left: {moved}");
    assert!(moved.dot(forward).abs() < 1e-3);

    // Out of the window and back in elsewhere: no jump.
    let grabbed = m.rig().focus;
    let world = m.app.world_mut();
    world.query_filtered::<&mut Window, With<PrimaryWindow>>().single_mut(world).unwrap().set_cursor_position(None);
    m.step();
    m.move_cursor(centre + Vec2::new(-300.0, 120.0));
    m.step();
    assert!(m.rig().focus.distance(grabbed) < 1e-4, "re-entering does not jump the view");
    m.move_cursor(centre + Vec2::new(-300.0, 140.0));
    m.step();
    assert!((m.rig().focus - grabbed).dot(forward) > 0.1, "the drag carries on from the new spot");

    // Released: moving the mouse no longer pans.
    m.release_mouse(MouseButton::Middle);
    m.step();
    let released = m.rig().focus;
    m.move_cursor(centre);
    m.step();
    assert_eq!(m.rig().focus, released);
}

// ---------------------------------------------------------------------------
// Picking and drag selection (fake window + camera)
// ---------------------------------------------------------------------------

#[test]
fn cursor_picks_ground_and_visible_entities() {
    let mut m = Match::new(false);
    let focus = m.rig().focus;
    let soldier = m.soldier_kind(m.me);
    let mine = m.unit(m.me, soldier, focus);
    m.settle();
    m.with_view();
    m.move_cursor(SCREEN * 0.5);
    m.step();
    let cursor = *m.app.world().resource::<CursorWorld>();
    assert_eq!(cursor.screen, Some(SCREEN * 0.5));
    assert!(cursor.ground.is_some_and(|g| g.distance(focus) < 0.05), "{:?}", cursor.ground);
    assert_eq!(cursor.hovered, Some(mine));

    // An enemy in the fog cannot be hovered; own units always can.
    let enemy_kind = m.soldier_kind(m.enemy);
    let off = focus + Vec2::new(6.0, 0.0);
    let enemy = m.unit(m.enemy, enemy_kind, off);
    m.fog_hides_everything();
    m.settle();
    let at = m.screen_of(off);
    m.move_cursor(at);
    m.step();
    assert_eq!(m.app.world().resource::<CursorWorld>().hovered, None);
    m.app.world_mut().resource_mut::<FogOfWar>().enabled = false;
    m.step();
    assert_eq!(m.app.world().resource::<CursorWorld>().hovered, Some(enemy));

    // Over UI the world is not pointed at.
    m.app.world_mut().resource_mut::<CursorWorld>().over_ui = true;
    m.step();
    let cursor = *m.app.world().resource::<CursorWorld>();
    assert_eq!((cursor.ground, cursor.hovered), (None, None));
}

#[test]
fn enemy_buildings_partly_in_sight_can_be_targeted() {
    let mut m = Match::new(false);
    let focus = m.rig().focus;
    let house = m.building_kind(m.enemy, Tag::House);
    // The house sits in the middle of a fog cell; only a cell under one of
    // its corners is in sight.
    let pos = (focus / 2.0).floor() * 2.0 + Vec2::ONE;
    let target = m.building(m.enemy, house, pos);
    let soldier = m.unit(m.me, m.soldier_kind(m.me), focus + Vec2::new(0.0, -12.0));
    m.settle();
    let size = m.data().building(house).size;
    let corner = pos + Vec2::new(size[0], size[2]) * 0.5 - Vec2::splat(0.1);
    m.fog_lighting(&[corner]);
    assert_ne!(fog_cell(m.app.world().resource::<FogOfWar>(), corner), fog_cell(m.app.world().resource::<FogOfWar>(), pos));
    m.with_view();
    let at = m.screen_of(pos);
    m.move_cursor(at);
    m.step();
    assert_eq!(m.app.world().resource::<CursorWorld>().hovered, Some(target), "hoverable by its lit corner");

    // Selected for info, it stays selected while that corner is in sight.
    m.select(&[target]);
    m.step();
    assert_eq!(m.selected(), vec![target]);
    m.tap(KeyCode::Escape);

    m.select(&[soldier]);
    m.click(MouseButton::Right);
    let issued = m.issued();
    assert_eq!(issued.len(), 1);
    assert!(matches!(issued[0].kind, CommandKind::Smart { target: Some(t), .. } if t == target), "{issued:?}");

    m.select(&[target]);
    m.fog_hides_everything();
    m.step();
    assert_eq!(m.app.world().resource::<CursorWorld>().hovered, None);
    assert_eq!(m.selected(), vec![soldier], "the house went dark and was deselected");
}

#[test]
fn drag_box_selects_own_units_inside() {
    let mut m = Match::new(false);
    let focus = m.rig().focus;
    let kind = m.soldier_kind(m.me);
    let near = m.unit(m.me, kind, focus);
    let far = m.unit(m.me, kind, focus + Vec2::new(-8.0, 8.0));
    let enemy_kind = m.soldier_kind(m.enemy);
    let _enemy = m.unit(m.enemy, enemy_kind, focus + Vec2::new(1.5, 0.0));
    m.settle();
    m.with_view();

    let centre = m.screen_of(focus);
    m.move_cursor(centre - Vec2::splat(60.0));
    m.press_mouse(MouseButton::Left);
    m.step();
    m.move_cursor(centre + Vec2::splat(60.0));
    m.step();
    let drag = m.app.world().resource::<DragSelect>().0;
    assert!(drag.is_some(), "the box is shown while dragging");
    m.release_mouse(MouseButton::Left);
    m.step();
    assert!(m.app.world().resource::<DragSelect>().0.is_none());
    assert_eq!(m.selected(), vec![near], "own unit inside only; enemies never box-selected");

    // Shift-drag around the other one adds it.
    let other = m.screen_of(focus + Vec2::new(-8.0, 8.0));
    m.hold(KeyCode::ShiftLeft);
    m.move_cursor(other - Vec2::splat(30.0));
    m.press_mouse(MouseButton::Left);
    m.step();
    m.move_cursor(other + Vec2::splat(30.0));
    m.step();
    m.release_mouse(MouseButton::Left);
    m.step();
    let mut both = vec![near, far];
    both.sort();
    assert_eq!(m.selected(), both);
}

#[test]
fn double_click_selects_the_type_on_screen() {
    let mut m = Match::new(false);
    let focus = m.rig().focus;
    let soldier = m.soldier_kind(m.me);
    let worker = m.worker_kind(m.me);
    let a = m.unit(m.me, soldier, focus);
    let b = m.unit(m.me, soldier, focus + Vec2::new(4.0, -3.0));
    let _w = m.unit(m.me, worker, focus + Vec2::new(-3.0, 2.0));
    // Far to the screen's right of the start view, so off screen.
    let _offscreen = m.unit(m.me, soldier, Vec2::new(10.0, 120.0));
    m.settle();
    m.with_view();
    let at = m.screen_of(focus);
    m.move_cursor(at);
    m.click(MouseButton::Left);
    assert_eq!(m.selected(), vec![a]);
    m.click(MouseButton::Left);
    let mut both = vec![a, b];
    both.sort();
    assert_eq!(m.selected(), both);
}

// ---------------------------------------------------------------------------
// Click selection (headless cursor)
// ---------------------------------------------------------------------------

#[test]
fn click_shift_click_and_foreign_selection() {
    let mut m = Match::new(false);
    let soldier = m.soldier_kind(m.me);
    let a = m.unit(m.me, soldier, Vec2::new(90.0, 60.0));
    let b = m.unit(m.me, soldier, Vec2::new(92.0, 60.0));
    let enemy_kind = m.soldier_kind(m.enemy);
    let enemy = m.unit(m.enemy, enemy_kind, Vec2::new(40.0, 60.0));
    m.settle();

    m.click_on(Vec2::new(90.0, 60.0), Some(a));
    assert_eq!(m.selected(), vec![a]);
    m.hold(KeyCode::ShiftLeft);
    m.click_on(Vec2::new(92.0, 60.0), Some(b));
    let mut both = vec![a, b];
    both.sort();
    assert_eq!(m.selected(), both);
    m.click_on(Vec2::new(90.0, 60.0), Some(a));
    assert_eq!(m.selected(), vec![b], "shift-click toggles off");
    m.release(KeyCode::ShiftLeft);

    m.click_on(Vec2::new(40.0, 60.0), Some(enemy));
    assert_eq!(m.selected(), vec![enemy], "enemies can be inspected alone");
    m.click_on(Vec2::new(70.0, 70.0), None);
    assert!(m.selected().is_empty(), "clicking the ground clears");
}

#[test]
fn squads_select_together() {
    let mut m = Match::new(false);
    let soldier = m.soldier_kind(m.me);
    let a = m.unit(m.me, soldier, Vec2::new(90.0, 60.0));
    let b = m.unit(m.me, soldier, Vec2::new(91.0, 60.0));
    let loner = m.unit(m.me, soldier, Vec2::new(95.0, 60.0));
    m.app.world_mut().entity_mut(a).insert(SquadMember { squad: 7 });
    m.app.world_mut().entity_mut(b).insert(SquadMember { squad: 7 });
    m.settle();
    m.click_on(Vec2::new(90.0, 60.0), Some(a));
    let mut squad = vec![a, b];
    squad.sort();
    assert_eq!(m.selected(), squad);
    assert!(!m.selected().contains(&loner));
}

#[test]
fn clicks_over_ui_do_nothing() {
    let mut m = Match::new(false);
    let soldier = m.soldier_kind(m.me);
    let a = m.unit(m.me, soldier, Vec2::new(90.0, 60.0));
    m.settle();
    m.point(Vec2::new(90.0, 60.0), Some(a));
    m.app.world_mut().resource_mut::<CursorWorld>().over_ui = true;
    m.click(MouseButton::Left);
    assert!(m.selected().is_empty());
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[test]
fn right_click_issues_smart_with_ping() {
    let mut m = Match::new(false);
    let soldier = m.soldier_kind(m.me);
    let a = m.unit(m.me, soldier, Vec2::new(90.0, 60.0));
    let enemy_kind = m.soldier_kind(m.enemy);
    let enemy = m.unit(m.enemy, enemy_kind, Vec2::new(40.0, 60.0));
    m.settle();
    m.select(&[a, enemy]);

    m.point(Vec2::new(70.0, 70.0), None);
    m.click(MouseButton::Right);
    let issued = m.issued();
    assert_eq!(issued.len(), 1);
    assert_eq!(issued[0], PlayerCommand::new(m.me, vec![a], CommandKind::Smart { point: Vec2::new(70.0, 70.0), target: None }));
    assert!(m.pings().iter().any(|p| p.pos == Vec2::new(70.0, 70.0) && p.color[1] > p.color[0]), "green move ping");
    assert_eq!(m.mark_at(Vec2::new(70.0, 70.0)), ClickMark::new(OrderMark::Move, None, false));

    m.hold(KeyCode::ShiftLeft);
    m.point(Vec2::new(40.0, 60.0), Some(enemy));
    m.click(MouseButton::Right);
    m.release(KeyCode::ShiftLeft);
    let issued = m.issued();
    assert_eq!(issued.len(), 1);
    assert!(issued[0].queue, "shift queues");
    assert_eq!(issued[0].kind, CommandKind::Smart { point: Vec2::new(40.0, 60.0), target: Some(enemy) });
    assert!(m.pings().iter().any(|p| p.pos == Vec2::new(40.0, 60.0) && p.color[0] > p.color[1]), "red attack ping");
    assert_eq!(m.mark_at(Vec2::new(40.0, 60.0)), ClickMark::new(OrderMark::Attack, Some(enemy), true), "hugs the enemy, queued");
}

/// The right-click marker guesses what `Smart` will do with what is
/// selected: the sim decides per unit, the marker for the selection.
#[test]
fn right_click_markers_show_the_order_it_becomes() {
    let mut m = Match::new(false);
    let me = m.me;
    let worker = m.unit(me, m.worker_kind(me), Vec2::new(90.0, 60.0));
    let soldier = m.unit(me, m.soldier_kind(me), Vec2::new(92.0, 60.0));
    let node_kind = m.data().node_ids().next().expect("a node type");
    let node = with_commands(&mut m.app, |c, data, _| spawn_node(c, data, node_kind, Vec2::new(60.0, 90.0), 0.0, None));
    let house = m.building_kind(me, Tag::House);
    let foundation = with_commands(&mut m.app, |c, data, players| spawn_building(c, data, players.get(me), house, Vec2::new(30.0, 30.0), 0.0, false));
    let damaged = m.building(me, house, Vec2::new(30.0, 60.0));
    let hall_kind = m.building_kind(me, Tag::TownCenter);
    let hall = m.building(me, hall_kind, Vec2::new(100.0, 100.0));
    m.settle();
    m.app.world_mut().get_mut::<Health>(damaged).unwrap().hp *= 0.5;
    let right_click = |m: &mut Match, at: Vec2, on: Option<Entity>| -> ClickMark {
        m.clear_marks();
        m.point(at, on);
        m.click(MouseButton::Right);
        m.mark_at(at)
    };
    m.select(&[worker]);
    let cases = [
        (Vec2::new(60.0, 90.0), Some(node), OrderMark::Gather, true),
        (Vec2::new(30.0, 30.0), Some(foundation), OrderMark::Build, true),
        (Vec2::new(30.0, 60.0), Some(damaged), OrderMark::Repair, true),
        (Vec2::new(92.0, 60.0), Some(soldier), OrderMark::Move, true),
        (Vec2::new(100.0, 100.0), Some(hall), OrderMark::Move, false),
        (Vec2::new(70.0, 70.0), None, OrderMark::Move, false),
    ];
    for (at, on, kind, hugs) in cases {
        let mark = right_click(&mut m, at, on);
        assert_eq!((mark.kind, mark.target), (kind, on.filter(|_| hugs)), "a worker on {on:?}");
    }
    // With cargo the town centre takes, the worker brings it home there.
    let takes = m.app.world().get::<DropSite>(hall).expect("the town centre is a drop site").mask;
    let mut cargo = m.app.world_mut().get_mut::<Gatherer>(worker).expect("a worker gathers");
    (cargo.carrying, cargo.amount) = (Some(ResourceId(takes.trailing_zeros() as u8)), 5.0);
    let mark = right_click(&mut m, Vec2::new(100.0, 100.0), Some(hall));
    assert_eq!((mark.kind, mark.target), (OrderMark::Gather, Some(hall)), "returning cargo");
    // A soldier cannot gather or build: it walks there.
    m.tap(KeyCode::Escape);
    m.select(&[soldier]);
    for (at, on) in [(Vec2::new(60.0, 90.0), node), (Vec2::new(30.0, 30.0), foundation)] {
        assert_eq!(right_click(&mut m, at, Some(on)), ClickMark::new(OrderMark::Move, None, false));
    }
    // Buildings alone set their rally.
    m.tap(KeyCode::Escape);
    m.select(&[hall]);
    assert_eq!(right_click(&mut m, Vec2::new(70.0, 70.0), None), ClickMark::new(OrderMark::Rally, None, false));
    assert_eq!(right_click(&mut m, Vec2::new(60.0, 90.0), Some(node)), ClickMark::new(OrderMark::Rally, Some(node), false));
}

#[test]
fn right_clicks_over_ui_issue_nothing_and_keep_the_mode() {
    let mut m = Match::new(false);
    let a = m.unit(m.me, m.soldier_kind(m.me), Vec2::new(90.0, 60.0));
    m.settle();
    m.select(&[a]);
    m.point(Vec2::new(70.0, 70.0), None);
    m.app.world_mut().resource_mut::<CursorWorld>().over_ui = true;
    m.click(MouseButton::Right);
    assert!(m.issued().is_empty(), "no Smart through the UI");
    assert!(m.pings().is_empty());

    m.set_mode(InputMode::Target(TargetAction::Move));
    m.click(MouseButton::Right);
    m.click(MouseButton::Left);
    assert_eq!(m.mode(), InputMode::Target(TargetAction::Move), "UI clicks neither cancel nor complete the mode");
    assert!(m.issued().is_empty());
}

#[test]
fn own_buildings_take_smart_and_rally_orders() {
    let mut m = Match::new(false);
    let hall_kind = m.building_kind(m.me, Tag::TownCenter);
    let hall = m.building(m.me, hall_kind, Vec2::new(100.0, 30.0));
    let soldier = m.unit(m.me, m.soldier_kind(m.me), Vec2::new(90.0, 60.0));
    m.settle();
    m.select(&[hall, soldier]);
    m.point(Vec2::new(70.0, 70.0), None);
    m.click(MouseButton::Right);
    let mut both = vec![hall, soldier];
    both.sort();
    assert_eq!(m.issued(), vec![PlayerCommand::new(m.me, both, CommandKind::Smart { point: Vec2::new(70.0, 70.0), target: None })], "the hall sets its rally");

    m.tap(KeyCode::Escape);
    m.select(&[hall]);
    m.set_mode(InputMode::Target(TargetAction::Rally));
    m.click_on(Vec2::new(60.0, 60.0), None);
    assert_eq!(m.issued(), vec![PlayerCommand::new(m.me, vec![hall], CommandKind::SetRally { point: Vec2::new(60.0, 60.0), target: None })]);
    assert_eq!(m.mode(), InputMode::Normal);
}

#[test]
fn spectators_select_but_never_command() {
    let mut m = Match::new(true);
    let soldier = m.soldier_kind(m.me);
    let a = m.unit(m.me, soldier, Vec2::new(90.0, 60.0));
    m.settle();
    m.click_on(Vec2::new(90.0, 60.0), Some(a));
    assert_eq!(m.selected(), vec![a]);
    m.point(Vec2::new(70.0, 70.0), None);
    m.click(MouseButton::Right);
    m.tap(KeyCode::Delete);
    assert!(m.issued().is_empty());
    assert!(m.app.world().resource::<CommandInbox>().pending.is_empty());
}

#[test]
fn delete_kills_only_own_selected() {
    let mut m = Match::new(false);
    let soldier = m.soldier_kind(m.me);
    let a = m.unit(m.me, soldier, Vec2::new(90.0, 60.0));
    let enemy_kind = m.soldier_kind(m.enemy);
    let enemy = m.unit(m.enemy, enemy_kind, Vec2::new(40.0, 60.0));
    m.settle();
    m.select(&[a, enemy]);
    m.tap(KeyCode::Delete);
    assert_eq!(m.issued(), vec![PlayerCommand::new(m.me, vec![a], CommandKind::Delete)]);
}

#[test]
fn target_mode_click_issues_the_action() {
    let mut m = Match::new(false);
    let soldier = m.soldier_kind(m.me);
    let a = m.unit(m.me, soldier, Vec2::new(90.0, 60.0));
    let enemy_kind = m.soldier_kind(m.enemy);
    let enemy = m.unit(m.enemy, enemy_kind, Vec2::new(40.0, 60.0));
    m.settle();
    m.select(&[a]);

    m.set_mode(InputMode::Target(TargetAction::Attack));
    m.click_on(Vec2::new(40.0, 60.0), Some(enemy));
    assert_eq!(m.issued(), vec![PlayerCommand::new(m.me, vec![a], CommandKind::Attack(enemy))]);
    assert_eq!(m.mode(), InputMode::Normal);
    assert_eq!(m.selected(), vec![a], "a targeting click does not change the selection");

    m.set_mode(InputMode::Target(TargetAction::Gather));
    m.click_on(Vec2::new(60.0, 60.0), None);
    assert!(m.issued().is_empty());
    assert_eq!(m.mode(), InputMode::Target(TargetAction::Gather), "needs a target; stays");
    assert!(m.notices().iter().any(|t| t.contains("resource")));

    m.point(Vec2::new(60.0, 60.0), None);
    m.click(MouseButton::Right);
    assert_eq!(m.mode(), InputMode::Normal, "right-click cancels");
    assert!(m.issued().is_empty(), "and does not also issue Smart");

    m.hold(KeyCode::ShiftLeft);
    m.set_mode(InputMode::Target(TargetAction::Move));
    m.click_on(Vec2::new(60.0, 61.0), None);
    m.release(KeyCode::ShiftLeft);
    assert_eq!(m.issued(), vec![PlayerCommand::new(m.me, vec![a], CommandKind::Move(Vec2::new(60.0, 61.0))).queued(true)]);
    assert_eq!(m.mode(), InputMode::Target(TargetAction::Move), "shift keeps the mode");
    assert_eq!(m.mark_at(Vec2::new(60.0, 61.0)), ClickMark::new(OrderMark::Move, None, true));
    assert_eq!(m.mark_at(Vec2::new(40.0, 60.0)), ClickMark::new(OrderMark::Attack, Some(enemy), false));

    // Each targeting order marks its kind.
    let node_kind = m.data().node_ids().next().expect("a node type");
    let node = with_commands(&mut m.app, |c, data, _| spawn_node(c, data, node_kind, Vec2::new(60.0, 90.0), 0.0, None));
    m.settle();
    let cases = [
        (TargetAction::Patrol, Vec2::new(61.0, 70.0), None, ClickMark::new(OrderMark::Patrol, None, false)),
        (TargetAction::AttackMove, Vec2::new(62.0, 70.0), None, ClickMark::new(OrderMark::AttackMove, None, false)),
        (TargetAction::Gather, Vec2::new(60.0, 90.0), Some(node), ClickMark::new(OrderMark::Gather, Some(node), false)),
    ];
    for (action, at, on, mark) in cases {
        m.set_mode(InputMode::Target(action));
        m.click_on(at, on);
        assert_eq!(m.mark_at(at), mark, "{action:?}");
    }
}

/// The command card's Attack button starts `TargetAction::AttackMove`: a
/// click on an enemy (a wall too, which attack-move never picks by itself)
/// attacks it; ground, own units and resource nodes attack-move there.
#[test]
fn attack_button_attacks_the_enemy_clicked() {
    let mut m = Match::new(false);
    let a = m.unit(m.me, m.soldier_kind(m.me), Vec2::new(90.0, 60.0));
    let mate = m.unit(m.me, m.soldier_kind(m.me), Vec2::new(92.0, 60.0));
    let enemy = m.unit(m.enemy, m.soldier_kind(m.enemy), Vec2::new(40.0, 60.0));
    let wall = m.building(m.enemy, m.wall_kind(m.enemy), Vec2::new(40.0, 80.0));
    let node_kind = m.data().node_ids().next().expect("a node type");
    let node = with_commands(&mut m.app, |c, data, _| spawn_node(c, data, node_kind, Vec2::new(60.0, 90.0), 0.0, None));
    m.settle();
    m.select(&[a]);

    for (target, at) in [(enemy, Vec2::new(40.0, 60.0)), (wall, Vec2::new(40.0, 80.0))] {
        m.set_mode(InputMode::Target(TargetAction::AttackMove));
        m.click_on(at, Some(target));
        assert_eq!(m.issued(), vec![PlayerCommand::new(m.me, vec![a], CommandKind::Attack(target))]);
        assert_eq!(m.mode(), InputMode::Normal);
        assert!(m.pings().iter().any(|p| p.pos == at && p.color[0] > p.color[1]), "red attack ping");
        assert_eq!(m.mark_at(at), ClickMark::new(OrderMark::Attack, Some(target), false), "hugs what it attacks");
    }
    let ground = Vec2::new(60.0, 70.0);
    for hovered in [None, Some(mate), Some(node)] {
        m.set_mode(InputMode::Target(TargetAction::AttackMove));
        m.click_on(ground, hovered);
        assert_eq!(m.issued(), vec![PlayerCommand::new(m.me, vec![a], CommandKind::AttackMove(ground))], "{hovered:?}");
    }
    // The forced Attack takes anything mortal, own units included, and
    // attack-moves past what cannot be hurt.
    m.set_mode(InputMode::Target(TargetAction::Attack));
    m.click_on(ground, Some(mate));
    assert_eq!(m.issued(), vec![PlayerCommand::new(m.me, vec![a], CommandKind::Attack(mate))]);
    m.set_mode(InputMode::Target(TargetAction::Attack));
    m.click_on(ground, Some(node));
    assert_eq!(m.issued(), vec![PlayerCommand::new(m.me, vec![a], CommandKind::AttackMove(ground))]);
}

/// A building af_fog reports in sight (here: of a player whose buildings are
/// revealed to all) can be hovered although no fog cell is lit; one it
/// reports out of sight cannot, whatever the cells under its bounding
/// rectangle show.
#[test]
fn hover_follows_what_af_fog_reports_for_buildings() {
    use af_core::ext::fog::{team_bit, SeenBy};
    let mut m = Match::new(false);
    let focus = m.rig().focus;
    let house = m.building_kind(m.enemy, Tag::House);
    let pos = (focus / 2.0).floor() * 2.0 + Vec2::ONE;
    let target = m.building(m.enemy, house, pos);
    m.settle();
    m.fog_hides_everything();
    m.with_view();
    let at = m.screen_of(pos);
    m.move_cursor(at);
    m.step();
    assert_eq!(m.app.world().resource::<CursorWorld>().hovered, None, "dark and unreported");
    let team = m.app.world().resource::<Players>().team(m.me);
    m.app.world_mut().entity_mut(target).insert(SeenBy { teams: team_bit(team), visible: team_bit(team) });
    m.step();
    assert_eq!(m.app.world().resource::<CursorWorld>().hovered, Some(target), "revealed");
    m.select(&[target]);
    m.step();
    assert_eq!(m.selected(), vec![target], "and stays selected");

    m.fog_lighting(&[pos]);
    m.app.world_mut().entity_mut(target).insert(SeenBy { teams: team_bit(team), visible: 0 });
    m.step();
    assert_eq!(m.app.world().resource::<CursorWorld>().hovered, None, "af_fog says out of sight");
    assert!(m.selected().is_empty(), "and the selection is dropped");
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

/// With the fog on, the ghost is judged from what the local team knows:
/// unseen enemy buildings and unseen vents do not show through it, while
/// scouted buildings, and dead ones the team still remembers, block.
#[test]
fn the_placement_ghost_shows_nothing_the_fog_hides() {
    use af_core::ext::fog::{team_bit, FogGhost, FogGhosts, SeenBy};
    let mut m = Match::new(false);
    let data = m.data();
    let team = m.app.world().resource::<Players>().team(m.me);
    let house = m.building_kind(m.me, Tag::House);
    let enemy_house = m.building_kind(m.enemy, Tag::House);
    let spot = Vec2::new(40.0, 100.0);
    let hidden = m.building(m.enemy, enemy_house, spot);
    m.settle();
    m.fog_hides_everything();
    m.set_mode(InputMode::Place { kind: house, facing: 0.0 });
    m.point(spot, None);
    let ghost = |m: &mut Match| {
        m.step();
        let ghosts = m.app.world().resource::<PlacementPreview>().ghosts.clone();
        assert_eq!(ghosts.len(), 1);
        ghosts[0]
    };
    assert_eq!(ghost(&mut m).error, None, "an enemy house nobody has seen does not show through");

    m.app.world_mut().entity_mut(hidden).insert(SeenBy { teams: team_bit(team), visible: 0 });
    assert_eq!(ghost(&mut m).error, Some(PlacementError::Blocked), "once scouted, it blocks");

    m.app.world_mut().entity_mut(hidden).despawn();
    m.settle();
    let footprint = Footprint::from_size(data.building(enemy_house).size);
    let remembered = FogGhost { entity: hidden, kind: EntityKind::Building(enemy_house), owner: Some(m.enemy), pos: spot, facing: 0.0, footprint, teams: team_bit(team) };
    m.app.world_mut().insert_resource(FogGhosts { list: vec![remembered] });
    assert_eq!(ghost(&mut m).error, Some(PlacementError::Blocked), "dead but still remembered");
    m.app.world_mut().resource_mut::<FogGhosts>().list.clear();
    assert_eq!(ghost(&mut m).error, None, "seen gone");

    // A node building snaps only onto vents the team has seen.
    let extractor = data.buildings.iter().position(|b| b.requires_node_id.is_some()).map(|i| BuildingTypeId(i as u16)).expect("a node building");
    let vent_kind = data.building(extractor).requires_node_id.unwrap();
    {
        let mut players = m.app.world_mut().resource_mut::<Players>();
        let me = &mut players.list[m.me.index()];
        me.age = data.building(extractor).age.max(me.age);
        me.resources = ResAmounts([1e6; MAX_RESOURCES]);
    }
    let vent_at = Vec2::new(80.0, 40.0);
    let vent = with_commands(&mut m.app, |c, data, _| spawn_node(c, data, vent_kind, vent_at, 0.0, None));
    m.settle();
    m.set_mode(InputMode::Place { kind: extractor, facing: 0.0 });
    m.point(vent_at + Vec2::new(1.5, 0.0), None);
    let g = ghost(&mut m);
    assert_eq!((g.pos, g.error), (vent_at + Vec2::new(1.5, 0.0), Some(PlacementError::NeedsNode)), "an unseen vent is not given away");
    m.app.world_mut().entity_mut(vent).insert(SeenBy { teams: team_bit(team), visible: 0 });
    let g = ghost(&mut m);
    assert_eq!((g.pos, g.error), (vent_at, None), "a scouted one takes the snap");

    // With the fog off everything counts again.
    m.app.world_mut().resource_mut::<FogOfWar>().enabled = false;
    m.app.world_mut().entity_mut(vent).remove::<SeenBy>();
    assert_eq!(ghost(&mut m).pos, vent_at);
}

#[test]
fn placing_a_building_needs_builders_and_room() {
    let mut m = Match::new(false);
    let house = m.building_kind(m.me, Tag::House);
    let worker = m.unit(m.me, m.worker_kind(m.me), Vec2::new(90.0, 100.0));
    let soldier = m.unit(m.me, m.soldier_kind(m.me), Vec2::new(92.0, 100.0));
    m.settle();

    // No builder selected: notice, back to Normal.
    m.select(&[soldier]);
    m.set_mode(InputMode::Place { kind: house, facing: 0.0 });
    m.click_on(Vec2::new(60.0, 100.0), None);
    assert!(m.issued().is_empty());
    assert!(m.notices().iter().any(|t| t == "Select workers to build"));
    assert_eq!(m.mode(), InputMode::Normal);

    // With a worker: the foundation is ordered and the mode ends.
    m.select(&[worker]);
    m.set_mode(InputMode::Place { kind: house, facing: 0.0 });
    m.step();
    let preview = m.app.world().resource::<PlacementPreview>().clone();
    assert!(preview.all_valid(), "{preview:?}");
    m.click_on(Vec2::new(60.0, 100.0), None);
    let issued = m.issued();
    assert_eq!(issued.len(), 1);
    assert_eq!(issued[0].entities, vec![worker], "only builders are sent");
    assert_eq!(issued[0].kind, CommandKind::PlaceBuilding { kind: house, pos: Vec2::new(60.0, 100.0), facing: 0.0 });
    assert_eq!(m.mode(), InputMode::Normal);
    assert_eq!(m.mark_at(Vec2::new(60.0, 100.0)), ClickMark::new(OrderMark::Place { kind: house, facing: 0.0 }, None, false), "the footprint flashes");
    assert!(m.app.world().resource::<PlacementPreview>().ghosts.is_empty());

    // On top of an existing building: refused with the reason, mode kept.
    m.building(m.me, house, Vec2::new(40.0, 100.0));
    m.settle();
    m.set_mode(InputMode::Place { kind: house, facing: 0.0 });
    m.click_on(Vec2::new(40.0, 100.0), None);
    assert!(m.issued().is_empty());
    assert_eq!(m.mode(), InputMode::Place { kind: house, facing: 0.0 });
    assert!(m.notices().iter().any(|t| *t == PlacementError::Blocked.describe(&m.data())));

    // Shift keeps placing.
    m.hold(KeyCode::ShiftLeft);
    m.click_on(Vec2::new(60.0, 80.0), None);
    m.release(KeyCode::ShiftLeft);
    assert_eq!(m.issued().len(), 1);
    assert!(m.issued()[0].queue);
    assert!(matches!(m.mode(), InputMode::Place { .. }));
    assert!(m.mark_at(Vec2::new(60.0, 80.0)).queued);
    assert!(!m.marks().iter().any(|(p, _)| p.pos == Vec2::new(40.0, 100.0)), "a refused placement is not marked");
}

#[test]
fn r_rotates_placement_and_esc_cancels() {
    let mut m = Match::new(false);
    let house = m.building_kind(m.me, Tag::House);
    m.set_mode(InputMode::Place { kind: house, facing: 0.0 });
    m.tap(KeyCode::KeyR);
    let InputMode::Place { facing, .. } = m.mode() else { panic!("still placing") };
    assert!((facing - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
    m.tap_with(KeyCode::ShiftLeft, KeyCode::KeyR);
    let InputMode::Place { facing, .. } = m.mode() else { panic!("still placing") };
    assert!((facing - (std::f32::consts::FRAC_PI_4 + 15f32.to_radians())).abs() < 1e-5);

    let soldier = m.unit(m.me, m.soldier_kind(m.me), Vec2::new(90.0, 60.0));
    m.settle();
    m.select(&[soldier]);
    m.tap(KeyCode::Escape);
    assert_eq!(m.mode(), InputMode::Normal);
    assert_eq!(m.selected(), vec![soldier], "first Esc only leaves the mode");
    m.tap(KeyCode::Escape);
    assert!(m.selected().is_empty(), "second Esc clears the selection");
}

#[test]
fn esc_closes_an_open_card_page_before_the_selection() {
    use af_core::ext::ui::CardPage;
    let mut m = Match::new(false);
    let worker = m.unit(m.me, m.worker_kind(m.me), Vec2::new(40.0, 40.0));
    m.settle();
    m.select(&[worker]);
    // af_ui reports a build page open (it is absent here, so play its part).
    m.app.world_mut().insert_resource(CardPage { open: true, close: false });
    let house = m.building_kind(m.me, Tag::House);
    m.set_mode(InputMode::Place { kind: house, facing: 0.0 });
    m.tap(KeyCode::Escape);
    assert_eq!(m.mode(), InputMode::Normal, "first Esc leaves placing");
    assert!(!m.app.world().resource::<CardPage>().close);
    m.tap(KeyCode::Escape);
    assert!(m.app.world().resource::<CardPage>().close, "second Esc asks af_ui to close the page");
    assert_eq!(m.selected(), vec![worker], "and keeps the selection");
    *m.app.world_mut().resource_mut::<CardPage>() = CardPage::default();
    m.tap(KeyCode::Escape);
    assert!(m.selected().is_empty(), "with the page closed, Esc deselects");
}

#[test]
fn f2_selects_the_whole_army() {
    let mut m = Match::new(false);
    let soldier = m.soldier_kind(m.me);
    let a = m.unit(m.me, soldier, Vec2::new(30.0, 30.0));
    let b = m.unit(m.me, soldier, Vec2::new(100.0, 90.0));
    let worker = m.unit(m.me, m.worker_kind(m.me), Vec2::new(60.0, 60.0));
    let enemy_soldier = m.soldier_kind(m.enemy);
    m.unit(m.enemy, enemy_soldier, Vec2::new(64.0, 64.0));
    m.settle();
    m.select(&[worker]);
    m.tap(KeyCode::F2);
    let mut army = vec![a, b];
    army.sort();
    assert_eq!(m.selected(), army, "own soldiers only, not workers or enemies");
}

#[test]
fn h_selects_and_cycles_the_halls() {
    let mut m = Match::new(false);
    let hall = m.building_kind(m.me, Tag::TownCenter);
    let a = m.building(m.me, hall, Vec2::new(30.0, 30.0));
    let b = m.building(m.me, hall, Vec2::new(100.0, 90.0));
    let enemy_hall = m.building_kind(m.enemy, Tag::TownCenter);
    m.building(m.enemy, enemy_hall, Vec2::new(30.0, 100.0));
    let soldier = m.unit(m.me, m.soldier_kind(m.me), Vec2::new(60.0, 60.0));
    m.settle();
    m.select(&[soldier]);
    let (first, second) = if a < b { (a, b) } else { (b, a) };
    let pos = |e: Entity, m: &Match| m.app.world().get::<Position>(e).unwrap().0;
    m.tap(KeyCode::KeyH);
    assert_eq!(m.selected(), vec![first]);
    assert!(m.rig().focus.distance(pos(first, &m)) < 1e-3, "looks at it");
    m.tap(KeyCode::KeyH);
    assert_eq!(m.selected(), vec![second], "the next hall");
    m.tap(KeyCode::KeyH);
    assert_eq!(m.selected(), vec![first], "wraps round");
    // After selecting something else, H starts from the first hall again.
    m.select(&[soldier]);
    m.app.world_mut().entity_mut(first).remove::<Selected>();
    m.tap(KeyCode::KeyH);
    assert_eq!(m.selected(), vec![first]);
}

#[test]
fn walls_take_two_clicks() {
    let mut m = Match::new(false);
    let wall = m.wall_kind(m.me);
    let worker = m.unit(m.me, m.worker_kind(m.me), Vec2::new(90.0, 100.0));
    m.settle();
    m.select(&[worker]);
    m.set_mode(InputMode::PlaceWall { kind: wall, start: None });
    m.click_on(Vec2::new(40.0, 110.0), None);
    assert_eq!(m.mode(), InputMode::PlaceWall { kind: wall, start: Some(Vec2::new(40.0, 110.0)) });
    assert!(m.issued().is_empty());

    let length = m.data().building(wall).size[0];
    let to = Vec2::new(40.0 + length * 4.0, 110.0);
    m.point(to, None);
    m.step();
    let preview = m.app.world().resource::<PlacementPreview>().clone();
    assert_eq!(preview.ghosts.len(), 4);
    assert!(preview.all_valid(), "{preview:?}");

    m.click_on(to, None);
    assert_eq!(m.issued(), vec![PlayerCommand::new(m.me, vec![worker], CommandKind::PlaceWall { kind: wall, from: Vec2::new(40.0, 110.0), to })]);
    assert_eq!(m.mode(), InputMode::Normal);
    // The marker outlines the line's last segment.
    let last = preview.ghosts[3];
    assert_eq!(m.mark_at(last.pos), ClickMark::new(OrderMark::Place { kind: wall, facing: last.facing }, None, false));
    assert_eq!(m.marks().len(), 1, "one marker for the line");
}

#[test]
fn diagonal_wall_lines_preview_as_valid() {
    let mut m = Match::new(false);
    let wall = m.wall_kind(m.me);
    let length = m.data().building(wall).size[0];
    // Plenty to pay for every segment, so only geometry decides.
    m.app.world_mut().resource_mut::<Players>().list[m.me.index()].resources = ResAmounts([1e6; MAX_RESOURCES]);
    let from = Vec2::new(64.0, 64.0);
    // Segments laid end to end touch exactly; float noise must not make
    // neighbours "overlap" (the economy places all of them).
    for degrees in [45.0f32, 30.0, 17.0, 72.0, 101.0, 163.0, 211.0, 333.0] {
        let dir = Vec2::from_angle(degrees.to_radians());
        m.set_mode(InputMode::PlaceWall { kind: wall, start: Some(from) });
        m.point(from + dir * length * 6.0, None);
        m.step();
        let preview = m.app.world().resource::<PlacementPreview>().clone();
        assert_eq!(preview.ghosts.len(), 6, "{degrees} degrees");
        assert!(preview.all_valid(), "{degrees} degrees: {preview:?}");
    }
}

#[test]
fn wall_lines_are_paid_segment_by_segment() {
    let mut m = Match::new(false);
    let wall = m.wall_kind(m.me);
    let worker = m.unit(m.me, m.worker_kind(m.me), Vec2::new(90.0, 100.0));
    m.settle();
    m.select(&[worker]);
    let cost = stats::building_cost(&m.data(), m.app.world().resource::<Players>().get(m.me), wall);
    assert!(!cost.is_zero(), "walls cost something");
    m.set_resources(m.me, cost.scaled(2.5));

    let from = Vec2::new(40.0, 110.0);
    let to = from + Vec2::X * m.data().building(wall).size[0] * 4.0;
    m.set_mode(InputMode::PlaceWall { kind: wall, start: Some(from) });
    m.point(to, None);
    m.step();
    let errors: Vec<_> = m.app.world().resource::<PlacementPreview>().ghosts.iter().map(|g| g.error).collect();
    let broke = Some(PlacementError::Rule(Reason::CannotAfford));
    assert_eq!(errors, vec![None, None, broke, broke], "two of four are affordable");
    m.click_on(to, None);
    assert_eq!(m.issued().len(), 1, "the affordable part is still ordered");

    // Not even one segment: refused with the reason, and the mode stays.
    m.set_resources(m.me, ResAmounts::ZERO);
    m.set_mode(InputMode::PlaceWall { kind: wall, start: Some(from) });
    m.click_on(to, None);
    assert!(m.issued().is_empty());
    assert!(m.notices().iter().any(|t| *t == PlacementError::Rule(Reason::CannotAfford).describe(&m.data())));
    assert_eq!(m.mode(), InputMode::PlaceWall { kind: wall, start: Some(from) });
}

#[test]
fn a_blocked_segment_past_the_money_still_reads_as_blocked() {
    let mut m = Match::new(false);
    let wall = m.wall_kind(m.me);
    let house = m.building_kind(m.me, Tag::House);
    let length = m.data().building(wall).size[0];
    let from = Vec2::new(30.0, 60.0);
    // A house over the line's fourth (last) segment, clear of the third.
    let house_half = m.data().building(house).size[0] * 0.5;
    m.building(m.me, house, from + Vec2::X * (length * 3.0 + house_half + 0.05));
    m.settle();
    let cost = stats::building_cost(&m.data(), m.app.world().resource::<Players>().get(m.me), wall);
    m.set_resources(m.me, cost.scaled(2.5));
    m.set_mode(InputMode::PlaceWall { kind: wall, start: Some(from) });
    m.point(from + Vec2::X * length * 4.0, None);
    m.step();
    let errors: Vec<_> = m.app.world().resource::<PlacementPreview>().ghosts.iter().map(|g| g.error).collect();
    let broke = Some(PlacementError::Rule(Reason::CannotAfford));
    // Two paid for, the third refused for money, the fourth blocked by the
    // house: red, not orange, as more money would not help.
    assert_eq!(errors, vec![None, None, broke, Some(PlacementError::Blocked)]);
}

#[test]
fn a_cursor_stand_in_replaces_the_mouse() {
    let mut m = Match::new(false);
    m.with_view();
    let target = m.rig().focus + Vec2::new(4.0, 3.0);
    let screen = m.screen_of(target);
    // The real mouse sits at the window's edge, where it would scroll the view.
    m.move_cursor(Vec2::new(1.0, 300.0));
    m.app.world_mut().insert_resource(af_core::ext::input::CursorOverride(Some(screen)));
    let focus = m.rig().focus;
    m.step();
    let cursor = *m.app.world().resource::<CursorWorld>();
    assert_eq!(cursor.screen, Some(screen));
    // `screen_of` aims half a metre above the ground, so the ray lands a little past it.
    assert!(cursor.ground.is_some_and(|g| g.distance(target) < 4.0), "{:?}", cursor.ground);
    assert_eq!(m.rig().focus, focus, "no edge scrolling from the real mouse");
    let distance = m.rig().distance;
    m.app.world_mut().insert_resource(AccumulatedMouseScroll { unit: MouseScrollUnit::Line, delta: Vec2::new(0.0, -3.0) });
    m.step();
    assert_eq!(m.rig().distance, distance, "no zooming by the real wheel");
    m.app.world_mut().insert_resource(AccumulatedMouseScroll::default());
    // Without a stand-in the real mouse is read again.
    m.app.world_mut().insert_resource(af_core::ext::input::CursorOverride(None));
    m.step();
    assert_eq!(m.app.world().resource::<CursorWorld>().screen, Some(Vec2::new(1.0, 300.0)));
}

// ---------------------------------------------------------------------------
// Hotkeys
// ---------------------------------------------------------------------------

#[test]
fn control_groups_set_recall_and_centre() {
    let mut m = Match::new(false);
    let soldier = m.soldier_kind(m.me);
    let a = m.unit(m.me, soldier, Vec2::new(30.0, 30.0));
    let b = m.unit(m.me, soldier, Vec2::new(34.0, 30.0));
    m.settle();
    m.select(&[a, b]);
    m.tap_with(KeyCode::ControlLeft, KeyCode::Digit1);
    let mut both = vec![a, b];
    both.sort();
    assert_eq!(m.app.world().resource::<ControlGroups>().members(1), both.as_slice());

    m.tap(KeyCode::Escape);
    assert!(m.selected().is_empty());
    m.tap(KeyCode::Digit1);
    assert_eq!(m.selected(), both);
    assert!(m.rig().focus.distance(Vec2::new(32.0, 30.0)) > 1.0, "a single tap does not move the camera");
    m.tap(KeyCode::Digit1);
    assert!(m.rig().focus.distance(Vec2::new(32.0, 30.0)) < 1e-3, "double tap centres on the group");

    // Shift+digit adds; dead members drop out on recall.
    let c = m.unit(m.me, soldier, Vec2::new(50.0, 30.0));
    m.settle();
    m.tap(KeyCode::Escape);
    m.select(&[c]);
    m.tap_with(KeyCode::ShiftLeft, KeyCode::Digit1);
    assert_eq!(m.app.world().resource::<ControlGroups>().members(1).len(), 3);
    m.app.world_mut().entity_mut(a).despawn();
    m.tap(KeyCode::Escape);
    m.tap(KeyCode::Digit1);
    let mut rest = vec![b, c];
    rest.sort();
    assert_eq!(m.selected(), rest);
}

#[test]
fn groups_survive_set_or_add_without_an_own_selection() {
    let mut m = Match::new(false);
    let a = m.unit(m.me, m.soldier_kind(m.me), Vec2::new(30.0, 30.0));
    let enemy_kind = m.soldier_kind(m.enemy);
    let enemy = m.unit(m.enemy, enemy_kind, Vec2::new(40.0, 60.0));
    m.settle();
    m.select(&[a]);
    m.tap_with(KeyCode::ControlLeft, KeyCode::Digit1);
    let group = |m: &Match| m.app.world().resource::<ControlGroups>().members(1).to_vec();
    assert_eq!(group(&m), vec![a]);

    // Inspecting an enemy, then reflexively pressing Ctrl+1 / Shift+1.
    m.tap(KeyCode::Escape);
    m.click_on(Vec2::new(40.0, 60.0), Some(enemy));
    assert_eq!(m.selected(), vec![enemy]);
    m.tap_with(KeyCode::ControlLeft, KeyCode::Digit1);
    m.tap_with(KeyCode::ShiftLeft, KeyCode::Digit1);
    assert_eq!(group(&m), vec![a], "a foreign selection does not touch the group");

    m.tap(KeyCode::Escape);
    assert!(m.selected().is_empty());
    m.tap_with(KeyCode::ControlLeft, KeyCode::Digit1);
    assert_eq!(group(&m), vec![a], "an empty selection does not wipe the group");
}

#[test]
fn idle_workers_cycle_and_centre() {
    let mut m = Match::new(false);
    let worker = m.worker_kind(m.me);
    let w1 = m.unit(m.me, worker, Vec2::new(20.0, 20.0));
    let w2 = m.unit(m.me, worker, Vec2::new(40.0, 100.0));
    let enemy_worker = m.worker_kind(m.enemy);
    m.unit(m.enemy, enemy_worker, Vec2::new(70.0, 70.0));
    m.settle();
    m.tap(KeyCode::Period);
    let first = m.selected();
    assert_eq!(first.len(), 1);
    assert!(first[0] == w1 || first[0] == w2);
    let pos = |e: Entity, m: &Match| m.app.world().get::<Position>(e).unwrap().0;
    assert!(m.rig().focus.distance(pos(first[0], &m)) < 1e-3);
    m.tap(KeyCode::F1);
    let second = m.selected();
    assert_eq!(second.len(), 1);
    assert_ne!(second, first);
    m.tap(KeyCode::Period);
    assert_eq!(m.selected(), first, "wraps round");
}

#[test]
fn centre_on_selection_else_main_hall() {
    let mut m = Match::new(false);
    let hall = m.building_kind(m.me, Tag::TownCenter);
    m.building(m.me, hall, Vec2::new(100.0, 30.0));
    let soldier = m.unit(m.me, m.soldier_kind(m.me), Vec2::new(20.0, 110.0));
    m.settle();
    m.tap(KeyCode::Space);
    assert!(m.rig().focus.distance(Vec2::new(100.0, 30.0)) < 1e-3);
    m.select(&[soldier]);
    m.tap(KeyCode::Home);
    assert!(m.rig().focus.distance(Vec2::new(20.0, 110.0)) < 1e-3);
}

#[test]
fn speed_keys_step_the_virtual_clock() {
    let mut m = Match::new(false);
    m.tap(KeyCode::Equal);
    assert_eq!(m.app.world().resource::<Time<Virtual>>().relative_speed(), 1.5);
    assert!(m.notices().iter().any(|t| t == "Game speed 1.5x"));
    m.tap(KeyCode::NumpadSubtract);
    m.tap(KeyCode::Minus);
    assert_eq!(m.app.world().resource::<Time<Virtual>>().relative_speed(), 0.75);
    for _ in 0..10 {
        m.tap(KeyCode::NumpadAdd);
    }
    assert_eq!(m.app.world().resource::<Time<Virtual>>().relative_speed(), 8.0);
}

#[test]
fn p_toggles_unit_paths_and_shift_shows_them_while_held() {
    let mut m = Match::new(false);
    let paths = |m: &Match| *m.app.world().resource::<ShowPaths>();
    assert_eq!(paths(&m), ShowPaths::default(), "hidden to begin with");
    m.tap(KeyCode::KeyP);
    assert!(paths(&m).on && paths(&m).visible());
    assert!(m.notices().iter().any(|t| t == "Unit paths shown (P)"));
    m.tap_with(KeyCode::ControlLeft, KeyCode::KeyP);
    assert!(paths(&m).on, "Ctrl+P is not P");
    m.tap(KeyCode::KeyP);
    assert!(!paths(&m).visible());
    assert!(m.notices().iter().any(|t| t == "Unit paths hidden (P)"));
    m.hold(KeyCode::ShiftRight);
    m.step();
    assert!(paths(&m).held && paths(&m).visible() && !paths(&m).on, "Shift peeks without toggling");
    m.release(KeyCode::ShiftRight);
    m.step();
    assert!(!paths(&m).visible());
    // A preference: the next match keeps it.
    m.tap(KeyCode::KeyP);
    m.app.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Menu);
    m.step();
    m.step();
    assert!(paths(&m).on, "kept after leaving the match");
}

/// A spectator has no routes to show (the Paths button is hidden too), so
/// P neither flips the setting nor says it did.
#[test]
fn p_does_nothing_for_a_spectator() {
    let mut m = Match::new(true);
    m.tap(KeyCode::KeyP);
    assert_eq!(*m.app.world().resource::<ShowPaths>(), ShowPaths::default());
    assert!(m.notices().iter().all(|t| !t.starts_with("Unit paths")), "{:?}", m.notices());
}

#[test]
fn foreign_selection_is_dropped_in_the_fog() {
    let mut m = Match::new(false);
    let enemy_kind = m.soldier_kind(m.enemy);
    let enemy = m.unit(m.enemy, enemy_kind, Vec2::new(40.0, 60.0));
    let mine = m.unit(m.me, m.soldier_kind(m.me), Vec2::new(90.0, 60.0));
    m.settle();
    m.select(&[enemy]);
    m.step();
    assert_eq!(m.selected(), vec![enemy]);
    m.fog_hides_everything();
    m.select(&[mine]);
    m.step();
    assert_eq!(m.selected(), vec![mine], "the enemy went into the fog; own units stay");
}
