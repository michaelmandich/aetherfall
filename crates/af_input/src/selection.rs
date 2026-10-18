//! Selecting with the left mouse button: click, shift-click toggle,
//! double-click or ctrl-click for every entity of a type on screen, and the
//! drag box. The `Selected` marker is the selection.

use af_core::ext::input::RtsCamera;
use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::cursor::Projector;
use crate::viewer::Viewer;

/// The cursor must move this far (logical px) before a press becomes a drag.
const DRAG_THRESHOLD_PX: f32 = 5.0;
/// Two clicks on the same type within this many seconds are a double-click.
const DOUBLE_CLICK_SECS: f64 = 0.35;

/// The in-progress left-button gesture.
#[derive(Resource, Default)]
pub(crate) struct Gesture {
    /// Screen position of a press that started in the world in Normal mode.
    press: Option<Vec2>,
    dragging: bool,
    last_click: Option<(TypeKey, f64)>,
}

impl Gesture {
    /// Drop a press or drag in progress. Returns whether there was one.
    pub fn cancel(&mut self) -> bool {
        let active = self.press.is_some();
        self.press = None;
        self.dragging = false;
        active
    }
}

/// What kind of entity something is, for "select all of this type".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypeKey {
    Unit(UnitTypeId),
    Building(BuildingTypeId),
}

/// What a click landed on.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClickTarget {
    /// The entity plus its squad mates (own units) or just the entity.
    pub group: Vec<Entity>,
    /// Selectable as part of the viewer's own selection.
    pub own: bool,
}

/// The selection after a click, or `None` to leave it unchanged.
/// `current_own` is the current selection when it is the viewer's own (empty
/// when nothing or a single foreign entity is selected).
pub(crate) fn click_select(current_own: &[Entity], target: Option<&ClickTarget>, shift: bool) -> Option<Vec<Entity>> {
    let Some(target) = target else {
        return if shift { None } else { Some(Vec::new()) };
    };
    if !target.own {
        // Foreign entities are shown alone, never mixed into an own selection.
        return if shift && !current_own.is_empty() { None } else { Some(target.group.clone()) };
    }
    if !shift {
        return Some(target.group.clone());
    }
    if target.group.iter().all(|e| current_own.contains(e)) {
        Some(current_own.iter().copied().filter(|e| !target.group.contains(e)).collect())
    } else {
        Some(union(current_own, &target.group))
    }
}

/// The selection after a drag box: own units inside, else own buildings
/// inside; shift adds to the current own selection.
pub(crate) fn box_select(current_own: &[Entity], units: &[Entity], buildings: &[Entity], shift: bool) -> Vec<Entity> {
    let picked = if units.is_empty() { buildings } else { units };
    if shift { union(current_own, picked) } else { picked.to_vec() }
}

/// The screen area a drag from `start` to `end` covers, whichever way it went.
pub(crate) fn drag_area(start: Vec2, end: Vec2) -> Rect {
    Rect::from_corners(start, end)
}

/// Is a box centre projected to `screen` inside `area` (edges included)?
/// A point behind the camera (`None`) never is.
pub(crate) fn in_area(area: Rect, screen: Option<Vec2>) -> bool {
    screen.is_some_and(|s| area.contains(s))
}

/// `a` followed by the members of `b` not already in it.
fn union(a: &[Entity], b: &[Entity]) -> Vec<Entity> {
    let mut out = a.to_vec();
    for e in b {
        if !out.contains(e) {
            out.push(*e);
        }
    }
    out
}

/// Make `next` the selection: remove `Selected` from the rest of `current`,
/// add it where missing.
fn apply_selection(commands: &mut Commands, current: impl IntoIterator<Item = Entity>, next: &[Entity]) {
    let wanted: HashSet<Entity> = next.iter().copied().collect();
    let mut kept = HashSet::new();
    for e in current {
        if wanted.contains(&e) {
            kept.insert(e);
        } else {
            commands.entity(e).try_remove::<Selected>();
        }
    }
    for e in next {
        if !kept.contains(e) {
            commands.entity(*e).try_insert(Selected);
        }
    }
}

/// Selectable entities and the current selection, as the viewer sees them.
#[derive(SystemParam)]
pub(crate) struct Selectables<'w, 's> {
    pub viewer: Viewer<'w, 's>,
    units: Query<'w, 's, (Entity, &'static Unit, &'static Owner, &'static Position, &'static Altitude, &'static Footprint), Without<Dead>>,
    buildings: Query<'w, 's, (Entity, &'static Building, &'static Owner, &'static Position, &'static Footprint), Without<Dead>>,
    nodes: Query<'w, 's, (), (With<ResourceNode>, Without<Dead>)>,
    squads: Query<'w, 's, (Entity, &'static SquadMember), Without<Dead>>,
    selected: Query<'w, 's, (Entity, Option<&'static Owner>), With<Selected>>,
}

impl Selectables<'_, '_> {
    /// Everything currently selected.
    pub fn current(&self) -> impl Iterator<Item = Entity> + '_ {
        self.selected.iter().map(|(e, _)| e)
    }

    /// The current selection if it is the viewer's own, sorted; empty when
    /// it is a foreign entity shown for information.
    pub fn current_own(&self) -> Vec<Entity> {
        let mut own: Vec<Entity> = self
            .selected
            .iter()
            .filter(|(_, o)| o.is_some_and(|o| self.viewer.owns(o.0)))
            .map(|(e, _)| e)
            .collect();
        own.sort();
        own
    }

    /// Replace the selection with `next`.
    pub fn select(&self, commands: &mut Commands, next: &[Entity]) {
        apply_selection(commands, self.current(), next);
    }

    fn owner(&self, e: Entity) -> Option<PlayerId> {
        self.units.get(e).map(|u| u.2.0).or_else(|_| self.buildings.get(e).map(|b| b.2.0)).ok()
    }

    fn type_key(&self, e: Entity) -> Option<TypeKey> {
        if let Ok(u) = self.units.get(e) {
            Some(TypeKey::Unit(u.1.kind))
        } else {
            self.buildings.get(e).ok().map(|b| TypeKey::Building(b.1.kind))
        }
    }

    /// The click target for `e`, or `None` if it is nothing selectable.
    pub fn click_target(&self, e: Entity) -> Option<ClickTarget> {
        match self.owner(e) {
            Some(owner) if self.viewer.owns(owner) => {
                let mut group = vec![e];
                self.add_squad_mates(&mut group);
                Some(ClickTarget { group, own: true })
            }
            Some(_) => Some(ClickTarget { group: vec![e], own: false }),
            None if self.nodes.contains(e) => Some(ClickTarget { group: vec![e], own: false }),
            None => None,
        }
    }

    /// Extend `group` with every living member of the squads it touches; sorts it.
    pub fn add_squad_mates(&self, group: &mut Vec<Entity>) {
        let squads: HashSet<u32> = group.iter().filter_map(|e| self.squads.get(*e).ok()).map(|(_, s)| s.squad).collect();
        if !squads.is_empty() {
            group.extend(self.squads.iter().filter(|(_, s)| squads.contains(&s.squad)).map(|(e, _)| e));
        }
        group.sort();
        group.dedup();
    }

    /// Own units, else own buildings, whose box centre is inside `rect` on screen.
    pub fn in_rect(&self, projector: &Projector, rect: Rect) -> (Vec<Entity>, Vec<Entity>) {
        let mut units: Vec<Entity> = self
            .units
            .iter()
            .filter(|(_, _, o, ..)| self.viewer.owns(o.0))
            .filter(|(_, _, _, p, alt, fp)| in_area(rect, projector.box_centre(p.0, alt.0, fp.height)))
            .map(|(e, ..)| e)
            .collect();
        self.add_squad_mates(&mut units);
        let mut buildings: Vec<Entity> = if units.is_empty() {
            self.buildings
                .iter()
                .filter(|(_, _, o, ..)| self.viewer.owns(o.0))
                .filter(|(_, _, _, p, fp)| in_area(rect, projector.box_centre(p.0, 0.0, fp.height)))
                .map(|(e, ..)| e)
                .collect()
        } else {
            Vec::new()
        };
        buildings.sort();
        (units, buildings)
    }

    /// Every own entity of the same type as `e` that is on screen (always
    /// including `e` itself).
    pub fn same_type_on_screen(&self, e: Entity, projector: &Projector) -> Vec<Entity> {
        let (Some(key), Some(screen)) = (self.type_key(e), projector.viewport()) else { return vec![e] };
        let mut out = vec![e];
        match key {
            TypeKey::Unit(kind) => out.extend(
                self.units
                    .iter()
                    .filter(|(_, u, o, ..)| u.kind == kind && self.viewer.owns(o.0))
                    .filter(|(_, _, _, p, alt, fp)| in_area(screen, projector.box_centre(p.0, alt.0, fp.height)))
                    .map(|(e, ..)| e),
            ),
            TypeKey::Building(kind) => out.extend(
                self.buildings
                    .iter()
                    .filter(|(_, b, o, ..)| b.kind == kind && self.viewer.owns(o.0))
                    .filter(|(_, _, _, p, fp)| in_area(screen, projector.box_centre(p.0, 0.0, fp.height)))
                    .map(|(e, ..)| e),
            ),
        }
        self.add_squad_mates(&mut out);
        out
    }
}

/// Left-button selection in Normal mode: press, drag (shown via
/// `DragSelect`) and release.
pub(crate) fn selection_gesture(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mode: Res<InputMode>,
    cursor: Res<CursorWorld>,
    rig: Res<CameraRig>,
    cameras: Query<&Camera, With<RtsCamera>>,
    sel: Selectables,
    mut gesture: ResMut<Gesture>,
    mut drag: ResMut<DragSelect>,
    mut commands: Commands,
) {
    if mouse.just_pressed(MouseButton::Left)
        && *mode == InputMode::Normal
        && !cursor.over_ui
        && let Some(screen) = cursor.screen
    {
        gesture.press = Some(screen);
        gesture.dragging = false;
    }
    let Some(start) = gesture.press else { return };
    let end = cursor.screen.or(drag.0.map(|(_, end)| end)).unwrap_or(start);
    if !gesture.dragging && end.distance(start) >= DRAG_THRESHOLD_PX {
        gesture.dragging = true;
    }
    if gesture.dragging {
        drag.0 = Some((start, end));
    }
    if mouse.pressed(MouseButton::Left) {
        return;
    }

    // Released: finish the gesture.
    let dragged = gesture.dragging;
    gesture.cancel();
    drag.0 = None;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let projector = cameras.single().ok().map(|c| Projector::new(c, &rig));

    let next = if dragged {
        let Some(projector) = projector else { return };
        let (units, buildings) = sel.in_rect(&projector, drag_area(start, end));
        Some(box_select(&sel.current_own(), &units, &buildings, shift))
    } else {
        let now = time.elapsed_secs_f64();
        click_selection(&sel, &mut gesture.last_click, cursor.hovered, projector.as_ref(), (shift, ctrl), now)
    };
    if let Some(next) = next {
        sel.select(&mut commands, &next);
    }
}

/// The selection after a click on `hovered` (`None` = unchanged). A
/// double-click or ctrl-click on an own entity takes every own entity of
/// its type on screen. `last_click` remembers the previous own click.
fn click_selection(
    sel: &Selectables,
    last_click: &mut Option<(TypeKey, f64)>,
    hovered: Option<Entity>,
    projector: Option<&Projector>,
    (shift, ctrl): (bool, bool),
    now: f64,
) -> Option<Vec<Entity>> {
    let current_own = sel.current_own();
    let target = hovered.and_then(|e| sel.click_target(e).map(|t| (e, t)));
    let (e, t) = match target {
        Some((e, t)) if t.own => (e, t),
        other => {
            *last_click = None;
            return click_select(&current_own, other.as_ref().map(|(_, t)| t), shift);
        }
    };
    let key = sel.type_key(e);
    let double = last_click.is_some_and(|(k, at)| Some(k) == key && now - at <= DOUBLE_CLICK_SECS);
    *last_click = if double { None } else { key.map(|k| (k, now)) };
    match projector {
        Some(p) if double || ctrl => {
            let all = sel.same_type_on_screen(e, p);
            Some(if shift { union(&current_own, &all) } else { all })
        }
        _ => click_select(&current_own, Some(&t), shift),
    }
}

/// Deselect foreign entities that went out of sight (into the fog).
pub(crate) fn prune_selection(
    viewer: Viewer,
    selected: Query<(Entity, Option<&Owner>, &Position, &Facing, &Footprint, Has<Unit>, Has<Building>), With<Selected>>,
    mut commands: Commands,
) {
    for (e, owner, pos, facing, footprint, unit, building) in &selected {
        let owner = owner.map(|o| o.0);
        if owner.is_some_and(|o| viewer.owns(o)) {
            continue;
        }
        let kind = if unit {
            SpatialKind::Unit
        } else if building {
            SpatialKind::Building
        } else {
            SpatialKind::Node
        };
        if !viewer.sees(e, owner, kind, &footprint.obb(pos.0, facing.0)) {
            commands.entity(e).try_remove::<Selected>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ents(n: usize) -> Vec<Entity> {
        let mut world = World::new();
        (0..n).map(|_| world.spawn_empty().id()).collect()
    }

    fn own(group: &[Entity]) -> ClickTarget {
        ClickTarget { group: group.to_vec(), own: true }
    }

    #[test]
    fn plain_click_replaces_and_empty_click_clears() {
        let e = ents(3);
        assert_eq!(click_select(&[e[0]], Some(&own(&[e[1], e[2]])), false), Some(vec![e[1], e[2]]));
        assert_eq!(click_select(&[e[0]], None, false), Some(vec![]));
        assert_eq!(click_select(&[e[0]], None, true), None, "shift-click on nothing keeps the selection");
    }

    #[test]
    fn shift_click_toggles_whole_groups() {
        let e = ents(3);
        let added = click_select(&[e[0]], Some(&own(&[e[1], e[2]])), true).unwrap();
        assert_eq!(added, vec![e[0], e[1], e[2]]);
        let removed = click_select(&added, Some(&own(&[e[1], e[2]])), true).unwrap();
        assert_eq!(removed, vec![e[0]]);
    }

    #[test]
    fn foreign_entities_are_selected_alone() {
        let e = ents(3);
        let enemy = ClickTarget { group: vec![e[2]], own: false };
        assert_eq!(click_select(&[e[0], e[1]], Some(&enemy), false), Some(vec![e[2]]));
        assert_eq!(click_select(&[e[0], e[1]], Some(&enemy), true), None, "shift does not mix in enemies");
        assert_eq!(click_select(&[], Some(&enemy), true), Some(vec![e[2]]));
    }

    #[test]
    fn box_prefers_units_and_shift_adds() {
        let e = ents(4);
        assert_eq!(box_select(&[e[0]], &[e[1]], &[e[2]], false), vec![e[1]]);
        assert_eq!(box_select(&[e[0]], &[], &[e[2]], false), vec![e[2]]);
        assert_eq!(box_select(&[e[0]], &[e[1], e[0]], &[], true), vec![e[0], e[1]]);
        assert_eq!(box_select(&[e[0]], &[], &[], false), Vec::<Entity>::new());
        assert_eq!(box_select(&[e[0]], &[], &[], true), vec![e[0]]);
    }

    #[test]
    fn drag_box_works_from_any_corner() {
        let (a, b) = (Vec2::new(20.0, 50.0), Vec2::new(100.0, 90.0));
        let corners = [(a, b), (b, a), (Vec2::new(a.x, b.y), Vec2::new(b.x, a.y)), (Vec2::new(b.x, a.y), Vec2::new(a.x, b.y))];
        for (start, end) in corners {
            let area = drag_area(start, end);
            assert!(in_area(area, Some(Vec2::new(60.0, 70.0))), "{start} -> {end}");
            assert!(in_area(area, Some(Vec2::new(20.0, 90.0))), "edges count");
            assert!(!in_area(area, Some(Vec2::new(10.0, 70.0))));
            assert!(!in_area(area, Some(Vec2::new(60.0, 95.0))));
        }
    }

    #[test]
    fn points_behind_the_camera_are_never_in_the_box() {
        let everything = drag_area(Vec2::splat(-1e6), Vec2::splat(1e6));
        assert!(!in_area(everything, None));
    }
}
