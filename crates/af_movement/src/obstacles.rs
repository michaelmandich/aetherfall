//! Static obstacles: keeps `Nav` in sync with every `Obstacle` component and
//! keeps a spatial hash of their boxes for the collision push-out.
//!
//! * Added or changed obstacles are registered at the start of
//!   `SimSet::Pathing` (map entities appear on the first tick; a building
//!   upgrade swaps the old footprint for the new one).
//! * Removals go through an `On<Remove, Obstacle>` observer, which also fires
//!   on despawn. Unlike `RemovedComponents`, it cannot miss removals when
//!   several frames pass between sim ticks.
//! * The registry stamps the grid it registered into (`NavGrid::sync_tag`).
//!   A new map brings a fresh, unstamped grid, and the registry starts over,
//!   setting the grid's walk margin ([`WALK_MARGIN`]) first.
//! * The walk margin keeps each open cell centre clear of every obstacle,
//!   but two obstacles at an angle (two turned trees) can leave a slot whose
//!   centre clears both yet is too narrow for a box to pass through. Such
//!   *pinches* ([`PINCH_WIDTH`]) are closed to movers too
//!   (`NavGrid::add_walk_blocks`) and reopen when either obstacle goes, so
//!   no path leads a worker into a slot it would wedge in.

use af_core::prelude::af_geom::Frame;
use af_core::prelude::*;
use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;

/// Side of a static hash cell (metres).
const STATIC_CELL: f32 = 4.0;
/// Movers only use nav cells whose centre is farther than this from every
/// obstacle (`NavGrid::set_walk_margin`): a worker's half-width, so every
/// open cell has room for one at its centre and no path squeezes through a
/// slot between two trees or ore veins that a worker does not fit (metres).
pub(crate) const WALK_MARGIN: f32 = 0.3;
/// A slot between two obstacles narrower than this (metres, box to box) is
/// closed to movers although cell centres in it clear both by the walk
/// margin: a worker's box
/// turned at an angle (0.85 m across the diagonal) or a rider's (0.9 m
/// wide) would wedge in it. A corridor this wide between two buildings
/// stays open, and so does any cell along a single wall.
pub(crate) const PINCH_WIDTH: f32 = 0.95;
/// Cells whose way from one box of such a slot to the other is at most this
/// much longer than the slot is narrow are closed: the slot and its mouths
/// (metres, about a worker's width).
const PINCH_SLACK: f32 = 0.6;

/// A registered obstacle box.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StaticBox {
    pub obb: Obb,
    /// `obb` with its axes worked out (the collision push-out tests it often).
    pub frame: Frame,
    /// Gate owned by this team (its units pass), or a solid obstacle.
    pub gate_team: Option<u8>,
    pub min: Vec2,
    pub max: Vec2,
}

/// Every obstacle registered in `Nav`, by entity, with a uniform hash grid
/// of their boxes.
#[derive(Resource, Debug, Default)]
pub(crate) struct ObstacleRegistry {
    /// `NavGrid::sync_tag` of the grid the entries are registered in (0 = none).
    tag: u64,
    generation: u64,
    slots: EntityHashMap<u32>,
    boxes: Vec<Option<StaticBox>>,
    free: Vec<u32>,
    origin: Vec2,
    cols: i32,
    rows: i32,
    cells: Vec<Vec<u32>>,
    /// Per slot: (other slot, nav cell) of every slot between two obstacles
    /// too narrow to pass (see [`PINCH_WIDTH`]), listed under both.
    pinches: Vec<Vec<(u32, IVec2)>>,
    /// Bounds of obstacles registered this tick: paths crossing them are re-checked.
    pub fresh: Vec<(Vec2, Vec2)>,
}

impl ObstacleRegistry {
    /// Start over on `nav` (a new map, no obstacles registered yet).
    fn reset(&mut self, nav: &mut NavGrid) {
        self.generation += 1;
        self.tag = self.generation;
        nav.set_sync_tag(self.tag);
        nav.set_walk_margin(WALK_MARGIN);
        self.slots.clear();
        self.boxes.clear();
        self.free.clear();
        self.pinches.clear();
        self.fresh.clear();
        self.origin = nav.origin();
        let size = nav.world_size();
        self.cols = (size.x / STATIC_CELL).ceil().max(1.0) as i32;
        self.rows = (size.y / STATIC_CELL).ceil().max(1.0) as i32;
        self.cells = vec![Vec::new(); (self.cols * self.rows) as usize];
    }

    /// Hash cells overlapped by the box `[min, max]`, clamped to the map.
    pub fn cell_range(&self, min: Vec2, max: Vec2) -> (i32, i32, i32, i32) {
        let cell = |p: Vec2| {
            let l = (p - self.origin) / STATIC_CELL;
            (
                (l.x.floor() as i32).clamp(0, (self.cols - 1).max(0)),
                (l.y.floor() as i32).clamp(0, (self.rows - 1).max(0)),
            )
        };
        let (x0, y0) = cell(min);
        let (x1, y1) = cell(max);
        (x0, y0, x1, y1)
    }

    /// Slots of the boxes touching hash cell (x, y).
    pub fn cell(&self, x: i32, y: i32) -> &[u32] {
        self.cells.get((y * self.cols + x) as usize).map_or(&[], |c| c.as_slice())
    }

    pub fn get(&self, slot: u32) -> Option<&StaticBox> {
        self.boxes.get(slot as usize).and_then(|b| b.as_ref())
    }

    /// Upper bound of slot numbers (for per-slot scratch arrays).
    pub fn capacity(&self) -> usize {
        self.boxes.len()
    }

    fn insert(&mut self, nav: &mut NavGrid, entity: Entity, o: &Obstacle) {
        apply(nav, o, true);
        let (min, max) = o.obb.aabb();
        let b = StaticBox { obb: o.obb, frame: o.obb.frame(), gate_team: o.gate_team, min, max };
        let slot = match self.free.pop() {
            Some(s) => {
                self.boxes[s as usize] = Some(b);
                s
            }
            None => {
                self.boxes.push(Some(b));
                self.pinches.push(Vec::new());
                (self.boxes.len() - 1) as u32
            }
        };
        let (x0, y0, x1, y1) = self.cell_range(min, max);
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.cells[(y * self.cols + x) as usize].push(slot);
            }
        }
        if o.gate_team.is_none() {
            self.add_pinches(nav, slot);
        }
        self.slots.insert(entity, slot);
        self.fresh.push((min, max));
    }

    /// Unregister `entity`; its nav cells are only touched when `nav` is the
    /// grid it was registered in.
    fn remove(&mut self, nav: Option<&mut NavGrid>, entity: Entity) {
        let Some(slot) = self.slots.remove(&entity) else { return };
        let Some(b) = self.boxes.get_mut(slot as usize).and_then(Option::take) else { return };
        let (x0, y0, x1, y1) = self.cell_range(b.min, b.max);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let cell = &mut self.cells[(y * self.cols + x) as usize];
                if let Some(i) = cell.iter().position(|s| *s == slot) {
                    cell.swap_remove(i);
                }
            }
        }
        self.free.push(slot);
        let pinched = self.forget_pinches(slot);
        if let Some(nav) = nav.filter(|n| n.sync_tag() == self.tag) {
            nav.add_walk_blocks(&pinched, -1);
            apply(nav, &Obstacle { obb: b.obb, gate_team: b.gate_team }, false);
            if b.gate_team.is_some() {
                self.restore_gates_near(nav, &b);
            }
        }
    }

    /// Close every slot too narrow to pass ([`PINCH_WIDTH`]) that the solid
    /// obstacle in `slot` leaves with a registered neighbour: the nav cells
    /// near the gap between the two boxes (see [`pinched`]). Cells either
    /// box already closes by the walk margin are left out: that holds as
    /// long as both boxes stand.
    fn add_pinches(&mut self, nav: &mut NavGrid, slot: u32) {
        let Some(b) = self.get(slot).copied() else { return };
        let reach = Vec2::splat(PINCH_WIDTH + PINCH_SLACK);
        let (x0, y0, x1, y1) = self.cell_range(b.min - reach, b.max + reach);
        let reg = &*self;
        let mut near: Vec<u32> = (y0..=y1).flat_map(|y| (x0..=x1).flat_map(move |x| reg.cell(x, y).iter().copied())).collect();
        near.sort_unstable();
        near.dedup();
        let margin = nav.walk_margin();
        let mut closed = Vec::new();
        for other in near.into_iter().filter(|s| *s != slot) {
            let Some(a) = self.get(other).copied().filter(|a| a.gate_team.is_none()) else { continue };
            let gap = a.frame.gap(&b.frame);
            if gap >= PINCH_WIDTH {
                continue;
            }
            // Cells near the gap lie within `gap + PINCH_SLACK` of both boxes.
            let grow = Vec2::splat(gap + PINCH_SLACK);
            let (lo, hi) = ((a.min - grow).max(b.min - grow), (a.max + grow).min(b.max + grow));
            if lo.x > hi.x || lo.y > hi.y {
                continue;
            }
            let (c0, c1) = (nav.cell_of(lo), nav.cell_of(hi));
            for y in c0.y..=c1.y {
                for x in c0.x..=c1.x {
                    let c = IVec2::new(x, y);
                    if nav.in_bounds(c) && pinched(&a.frame, &b.frame, gap, nav.cell_center(c), margin) {
                        closed.push(c);
                        self.pinches[slot as usize].push((other, c));
                        self.pinches[other as usize].push((slot, c));
                    }
                }
            }
        }
        nav.add_walk_blocks(&closed, 1);
    }

    /// Drop every pinch `slot` takes part in (from both sides' lists);
    /// returns their cells, to reopen.
    fn forget_pinches(&mut self, slot: u32) -> Vec<IVec2> {
        let mine = std::mem::take(&mut self.pinches[slot as usize]);
        for (other, c) in &mine {
            let theirs = &mut self.pinches[*other as usize];
            if let Some(i) = theirs.iter().position(|p| *p == (slot, *c)) {
                theirs.swap_remove(i);
            }
        }
        mine.into_iter().map(|(_, c)| c).collect()
    }

    /// Clearing a gate's cells also cleared the marks of any gate sharing
    /// them (a gate line placed in one go, rotated gates): mark every gate
    /// overlapping `removed` again.
    fn restore_gates_near(&self, nav: &mut NavGrid, removed: &StaticBox) {
        let (x0, y0, x1, y1) = self.cell_range(removed.min, removed.max);
        let mut near: Vec<u32> = (y0..=y1).flat_map(|y| (x0..=x1).flat_map(move |x| self.cell(x, y).iter().copied())).collect();
        near.sort_unstable();
        near.dedup();
        for sb in near.iter().filter_map(|s| self.get(*s)) {
            let overlaps = sb.min.x <= removed.max.x && removed.min.x <= sb.max.x && sb.min.y <= removed.max.y && removed.min.y <= sb.max.y;
            if let (Some(team), true) = (sb.gate_team, overlaps) {
                nav.set_gate(&sb.obb, Some(team));
            }
        }
    }

    /// Register a new obstacle or swap in a changed one.
    fn upsert(&mut self, nav: &mut NavGrid, entity: Entity, o: &Obstacle) {
        if let Some(old) = self.slots.get(&entity).and_then(|s| self.get(*s)) {
            if old.obb == o.obb && old.gate_team == o.gate_team {
                return;
            }
            self.remove(Some(nav), entity);
        }
        self.insert(nav, entity, o);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// A registry of `obstacles` (standing in for entities 1, 2, ...)
    /// registered in `nav`, for tests of their users.
    #[cfg(test)]
    pub fn with(nav: &mut NavGrid, obstacles: &[Obstacle]) -> ObstacleRegistry {
        let mut reg = ObstacleRegistry::default();
        reg.reset(nav);
        for (i, o) in obstacles.iter().enumerate() {
            reg.insert(nav, Entity::from_raw_u32(i as u32 + 1).expect("entity"), o);
        }
        reg
    }
}

/// Is the cell centre `c` in the slot between boxes `a` and `b`, which lie
/// `gap` apart (less than [`PINCH_WIDTH`])? It is when the way from the
/// one box to the other through `c` is little longer than the gap itself
/// (within [`PINCH_SLACK`]): `c` sits in the gap or at its mouth. Not when
/// either box comes within the walk `margin` of `c` (closed anyway).
fn pinched(a: &Frame, b: &Frame, gap: f32, c: Vec2, margin: f32) -> bool {
    let (da, db) = (a.distance_to_point(c), b.distance_to_point(c));
    gap < PINCH_WIDTH && da > margin && db > margin && da + db < gap + PINCH_SLACK
}

/// Add (`add`) or remove an obstacle's footprint in the grid.
fn apply(nav: &mut NavGrid, o: &Obstacle, add: bool) {
    match o.gate_team {
        Some(team) => nav.set_gate(&o.obb, add.then_some(team)),
        None => nav.add_obstacle(&o.obb, if add { 1 } else { -1 }),
    }
}

/// Register added/changed obstacles in `Nav` (first system of Pathing).
pub(crate) fn sync_obstacles(
    mut nav: ResMut<Nav>,
    mut reg: ResMut<ObstacleRegistry>,
    all: Query<(Entity, &Obstacle)>,
    changed: Query<(Entity, &Obstacle), Changed<Obstacle>>,
) {
    reg.fresh.clear();
    if reg.tag == 0 || nav.sync_tag() != reg.tag {
        reg.reset(&mut nav);
        for (e, o) in &all {
            reg.insert(&mut nav, e, o);
        }
        // Everything is new; no path predates this grid.
        reg.fresh.clear();
        return;
    }
    for (e, o) in &changed {
        reg.upsert(&mut nav, e, o);
    }
}

/// Unregister an obstacle when the component goes away (despawn included).
pub(crate) fn forget_removed(removed: On<Remove, Obstacle>, mut reg: ResMut<ObstacleRegistry>, mut nav: Option<ResMut<Nav>>) {
    let nav = nav.as_deref_mut().map(|n| &mut n.0);
    reg.remove(nav, removed.entity);
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::{flat_world, new_nav, run_ticks, sim_app, test_data, with_commands};

    fn blocked(app: &App, c: IVec2) -> bool {
        !app.world().resource::<Nav>().passable(c, Mobility::of(MoveFlag::Foot).0, 0)
    }

    #[test]
    fn registry_follows_spawns_despawns_and_new_grids() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        app.add_plugins(crate::MovementPlugin);
        flat_world(&mut app, 32.0, &[("ironhold", 0)]);
        let rock = data.node_ids().find(|n| data.node(*n).blocking).expect("a blocking node");
        let nodes: Vec<Entity> = [Vec2::new(8.5, 8.5), Vec2::new(20.5, 20.5)]
            .iter()
            .map(|p| with_commands(&mut app, |c, data, _| spawn_node(c, data, rock, *p, 0.0, None)))
            .collect();
        run_ticks(&mut app, 1);
        assert_eq!(app.world().resource::<ObstacleRegistry>().len(), 2);
        assert!(blocked(&app, IVec2::new(8, 8)) && blocked(&app, IVec2::new(20, 20)));

        // A new map's grid: everything is registered into it on the next tick.
        let grass = data.terrain_id("grass").expect("grass");
        app.world_mut().insert_resource(Nav(new_nav(&data, 32, 32, grass)));
        assert!(!blocked(&app, IVec2::new(8, 8)));
        run_ticks(&mut app, 1);
        assert_eq!(app.world().resource::<ObstacleRegistry>().len(), 2);
        assert!(blocked(&app, IVec2::new(8, 8)), "re-registered into the new grid");

        app.world_mut().entity_mut(nodes[0]).despawn();
        assert_eq!(app.world().resource::<ObstacleRegistry>().len(), 1, "the observer runs on despawn");
        assert!(!blocked(&app, IVec2::new(8, 8)));
        assert!(blocked(&app, IVec2::new(20, 20)));
    }

    /// Removing one of two overlapping gates keeps the other one whole.
    #[test]
    fn removing_a_gate_keeps_an_overlapping_gate_whole() {
        let mut app = sim_app(test_data());
        app.add_plugins(crate::MovementPlugin);
        flat_world(&mut app, 32.0, &[("ironhold", 0)]);
        let gate = |x: f32| Obstacle { obb: Obb::new(Vec2::new(x, 10.0), Vec2::new(2.0, 0.6), 0.0), gate_team: Some(0) };
        // Cells x = 8..=11 and x = 10..=13 (rows 9 and 10): x = 10, 11 shared.
        let first = app.world_mut().spawn(gate(10.0)).id();
        app.world_mut().spawn(gate(12.0));
        run_ticks(&mut app, 1);
        app.world_mut().entity_mut(first).despawn();
        let nav = app.world().resource::<Nav>();
        for x in 10..=13 {
            assert_eq!(nav.gate_team(IVec2::new(x, 10)), Some(0), "cell {x} still belongs to the standing gate");
            assert!(!nav.passable(IVec2::new(x, 10), Mobility::of(MoveFlag::Foot).0, 1), "closed to other teams");
        }
        assert_eq!(nav.gate_team(IVec2::new(8, 10)), None, "the removed gate's own cells are released");
    }

    fn solid(x: f32, y: f32, half: Vec2, yaw: f32) -> Obstacle {
        Obstacle { obb: Obb::new(Vec2::new(x, y), half, yaw), gate_team: None }
    }

    /// Two trees from a generated forest, turned about 35 degrees, leave a
    /// diagonal slot a worker's box wedged in although the cell centre in it
    /// clears both by the walk margin: it is closed, and opens again when a
    /// tree is felled.
    #[test]
    fn a_slot_too_narrow_to_pass_is_closed_until_a_tree_falls() {
        let mut app = sim_app(test_data());
        app.add_plugins(crate::MovementPlugin);
        flat_world(&mut app, 32.0, &[("ironhold", 0)]);
        let (pine, oak) = (solid(14.74, 15.25, Vec2::splat(0.45), -0.57), solid(16.48, 14.65, Vec2::splat(0.5), -0.61));
        let slot = IVec2::new(15, 14);
        let centre = Vec2::new(15.5, 14.5);
        assert!(pine.obb.distance_to_point(centre) > WALK_MARGIN && oak.obb.distance_to_point(centre) > WALK_MARGIN, "the margin alone leaves it open");
        let first = app.world_mut().spawn(pine).id();
        app.world_mut().spawn(oak);
        run_ticks(&mut app, 1);
        assert!(blocked(&app, slot), "too narrow to pass");
        app.world_mut().entity_mut(first).despawn();
        assert!(!blocked(&app, slot), "the pine is gone");
        assert!(app.world().resource::<ObstacleRegistry>().pinches.iter().all(Vec::is_empty));
    }

    /// A corridor wide enough to pass between two buildings stays open, and
    /// so do the cells along a wall made of segments.
    #[test]
    fn corridors_and_walls_are_not_pinches() {
        let mut app = sim_app(test_data());
        app.add_plugins(crate::MovementPlugin);
        flat_world(&mut app, 32.0, &[("ironhold", 0)]);
        // Farms 1.1 m apart: the corridor's cell centres lie 0.55 m from each.
        app.world_mut().spawn(solid(10.0, 8.0, Vec2::splat(3.0), 0.0));
        app.world_mut().spawn(solid(10.0, 15.1, Vec2::splat(3.0), 0.0));
        // Wall segments end to end along y = 25.
        for i in 0..4 {
            app.world_mut().spawn(solid(4.0 + i as f32 * 2.0, 25.0, Vec2::new(1.0, 0.25), 0.0));
        }
        run_ticks(&mut app, 1);
        for x in 8..12 {
            assert!(!blocked(&app, IVec2::new(x, 11)), "corridor cell {x}");
            assert!(!blocked(&app, IVec2::new(x, 25 + 1)) && !blocked(&app, IVec2::new(x, 24 - 1)), "beside the wall at {x}");
        }
    }
}
