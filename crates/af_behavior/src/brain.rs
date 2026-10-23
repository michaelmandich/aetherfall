//! Per-unit behaviour state and order-change bookkeeping.

use af_core::prelude::*;
use bevy::prelude::*;

use crate::tuning::{UNREACHABLE_MEMORY, UNREACHABLE_TICKS};

/// Where a multi-step order is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Just started; the order system sets it up on its first tick.
    #[default]
    Fresh,
    /// Walking to the work target.
    Approach,
    /// At the target, working (gathering, building, repairing).
    Work,
    /// Carrying cargo to a drop site.
    Deliver,
}

/// A unit's brain: what it is doing about its front order and, when it has
/// none, about its stance. Added to every unit when it spawns; only
/// af_behavior writes it.
#[derive(Component, Clone, Debug, Default)]
pub struct Brain {
    /// The front order this brain last started (`None` = no order).
    pub(crate) active: Option<Order>,
    pub(crate) phase: Phase,
    /// Hostile engaged on our own initiative (attack-move, patrol, hold,
    /// stances, retaliation).
    pub(crate) engaged: Option<Entity>,
    /// Hostiles, patients, nodes and drop sites af_movement failed to
    /// reach lately.
    /// It is about the world, not the order, so it outlives order changes.
    pub(crate) unreachable: Unreachable,
    /// Post of an idle unit: where it stopped. Defensive units return here.
    pub(crate) home: Option<Vec2>,
    /// A defensive unit walking back to its post.
    pub(crate) returning: bool,
    /// Resource field whose slot this worker holds.
    pub(crate) field: Option<Entity>,
    /// What the current gather target yields (to find a similar source
    /// once it is gone).
    pub(crate) resource: Option<ResourceId>,
    /// Source of the last gather job this worker started, and what it
    /// yields; `Gatherer::last_pos` is where it stood. Like `unreachable`
    /// it outlives order changes: a Gather queued back on that source, once
    /// it is gone, moves on to a similar one round it.
    pub(crate) last_job: Option<(Entity, ResourceId)>,
    /// Drop site the cargo is being carried to.
    pub(crate) drop_site: Option<Entity>,
    /// Ally a healer is mending.
    pub(crate) heal_target: Option<Entity>,
    /// The healer mended `heal_target` this tick (it is in reach).
    pub(crate) mending: bool,
    /// The patrol leg being walked: from point `patrol_leg` of the route
    /// (`af_core::ext::behavior::patrol_points`) to the next, the last
    /// leading back to the first. 0 (a to b) when the order starts; it goes
    /// with the task when the front order changes.
    pub(crate) patrol_leg: usize,
    /// The point that leg leads to. Points shift-queued onto a running
    /// patrol join the loop and move the legs after them along; the unit
    /// then takes the leg that still ends here, so it walks on to the same
    /// point rather than turning round.
    pub(crate) patrol_to: Option<Vec2>,
    /// Path failures in a row for the current order.
    pub(crate) failures: u8,
    /// Per-unit tick offset so periodic scans spread over ticks.
    stagger: u8,
}

impl Brain {
    fn new(entity: Entity) -> Brain {
        Brain { stagger: (entity.index_u32() % 64) as u8, ..default() }
    }

    /// True when the brain is running the unit's current front order (or,
    /// with no orders, is idle). Order systems only act on synced brains.
    pub(crate) fn synced(&self, orders: &Orders) -> bool {
        orders.current() == self.active.as_ref()
    }

    /// Is a periodic decision with this period due this tick?
    pub(crate) fn due(&self, tick: u64, period: u64) -> bool {
        (tick + self.stagger as u64).is_multiple_of(period)
    }

    /// Give up on an engaged hostile af_movement cannot reach.
    pub(crate) fn give_up_on(&mut self, target: Entity, tick: u64) {
        self.unreachable.remember(target, tick);
        self.engaged = None;
    }

    /// Swap the front order for `order` without restarting (the brain keeps
    /// its phase), e.g. a worker moving on to the next tree.
    pub(crate) fn replace_front(&mut self, orders: &mut Orders, order: Order) {
        match orders.queue.front_mut() {
            Some(front) => *front = order,
            None => orders.queue.push_back(order),
        }
        self.active = Some(order);
    }

    /// Forget everything about the current task.
    pub(crate) fn reset_task(&mut self) {
        let Brain { stagger, home, unreachable, last_job, .. } = *self;
        *self = Brain { stagger, home, unreachable, last_job, ..default() };
    }
}

/// Entities af_movement recently failed to reach. Automatic choices (target
/// acquisition, the next patient, tree or drop site) skip them for
/// [`UNREACHABLE_TICKS`], or, when the terrain keeps them out of reach
/// (another region, see `reach`), until the regions change. A fixed table
/// that never allocates: a new entry takes the slot that expires first.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Unreachable {
    /// (entity, tick until which it is skipped; [`FOR_GOOD`] until the
    /// regions change).
    entries: [(Entity, u64); UNREACHABLE_MEMORY],
    /// Terrain stamp of the regions the for-good entries were found in.
    stamp: u64,
}

/// `Unreachable` entry that lasts until the regions change.
const FOR_GOOD: u64 = u64::MAX;

impl Default for Unreachable {
    fn default() -> Self {
        Unreachable { entries: [(Entity::PLACEHOLDER, 0); UNREACHABLE_MEMORY], stamp: 0 }
    }
}

impl Unreachable {
    /// `e` could not be reached at `tick`.
    pub(crate) fn remember(&mut self, e: Entity, tick: u64) {
        self.put(e, tick + UNREACHABLE_TICKS);
    }

    /// `e` lies out of the unit's reach over the terrain of regions
    /// `stamp` (see `Reach::stamp`): skipped until they change.
    pub(crate) fn remember_for_good(&mut self, e: Entity, stamp: u64) {
        self.sync(stamp);
        self.put(e, FOR_GOOD);
    }

    /// Keep the for-good entries to the regions of terrain `stamp`: those
    /// of other regions (another map, changed terrain) are forgotten.
    pub(crate) fn sync(&mut self, stamp: u64) {
        if self.stamp != stamp {
            self.stamp = stamp;
            for entry in &mut self.entries {
                if entry.1 == FOR_GOOD {
                    *entry = (Entity::PLACEHOLDER, 0);
                }
            }
        }
    }

    fn put(&mut self, e: Entity, until: u64) {
        let slot = match self.entries.iter().position(|&(x, _)| x == e) {
            Some(i) => i,
            None => (0..UNREACHABLE_MEMORY).min_by_key(|&i| self.entries[i].1).unwrap_or(0),
        };
        self.entries[slot] = (e, until);
    }

    /// Should automatic choices skip `e` at `tick`?
    pub(crate) fn skips(&self, e: Entity, tick: u64) -> bool {
        self.entries.iter().any(|&(x, until)| x == e && tick < until)
    }
}

/// Gives every new unit a brain.
pub(crate) fn attach_brain(add: On<Add, Unit>, mut commands: Commands) {
    commands.entity(add.entity).try_insert(Brain::new(add.entity));
}

/// Starts the front order whenever it differs from what the brain is
/// running: drops the old movement and attack intents and resets the task.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_brains(
    mut commands: Commands,
    mut units: Query<(Entity, &Position, &Orders, &mut Brain, Has<MoveGoal>, Has<AttackTarget>), (With<Unit>, Without<Dead>)>,
) {
    for (entity, pos, orders, mut brain, moving, attacking) in &mut units {
        if brain.synced(orders) {
            continue;
        }
        brain.reset_task();
        brain.active = orders.current().copied();
        if brain.active.is_none() {
            brain.home = Some(pos.0);
        }
        let mut e = commands.entity(entity);
        if moving {
            e.try_remove::<MoveGoal>();
        }
        if attacking {
            e.try_remove::<AttackTarget>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(i: u32) -> Entity {
        Entity::from_raw_u32(i).unwrap()
    }

    #[test]
    fn unreachable_entries_expire() {
        let mut table = Unreachable::default();
        table.remember(entity(1), 10);
        assert!(table.skips(entity(1), 10 + UNREACHABLE_TICKS - 1));
        assert!(!table.skips(entity(1), 10 + UNREACHABLE_TICKS));
        assert!(!table.skips(entity(2), 10));
    }

    /// What the terrain keeps out of reach stays skipped until the regions
    /// change, and outlasts timed entries when the table fills up.
    #[test]
    fn terrain_unreachable_entries_last_until_the_regions_change() {
        let mut table = Unreachable::default();
        table.remember_for_good(entity(1), 7);
        assert!(table.skips(entity(1), 1_000_000));
        for i in 2..=UNREACHABLE_MEMORY as u32 + 3 {
            table.remember(entity(i), i as u64);
        }
        assert!(table.skips(entity(1), 20), "timed entries go first");
        table.sync(7);
        assert!(table.skips(entity(1), 20), "the same regions");
        table.sync(8);
        assert!(!table.skips(entity(1), 20), "new regions");
    }

    #[test]
    fn a_full_table_drops_what_expires_first_and_a_repeat_refreshes_in_place() {
        let mut table = Unreachable::default();
        for i in 1..=UNREACHABLE_MEMORY as u32 {
            table.remember(entity(i), i as u64);
        }
        // Refreshing an entry neither duplicates it nor drops another.
        table.remember(entity(1), 10);
        assert!((1..=UNREACHABLE_MEMORY as u32).all(|i| table.skips(entity(i), 10)));
        // Entity 2 now expires first.
        table.remember(entity(99), 11);
        assert!(!table.skips(entity(2), 11));
        assert!(table.skips(entity(99), 11) && table.skips(entity(1), 11) && table.skips(entity(3), 11));
    }
}
