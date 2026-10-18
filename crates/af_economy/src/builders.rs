//! Builders sent to the foundations their command placed.
//!
//! The tick's commands must act as if run one by one in inbox order, but
//! they are applied in two passes: [`EconomyIntake`] (placement) first, then
//! af_behavior's unit command intake, which applies every unit command of
//! the tick in inbox order. So a placing command's builders get their Build
//! orders right in the economy pass ([`send_builders`]): every later unit
//! command of the tick then reaches them after the Build orders, replacing
//! them or queueing behind them, exactly as in order.
//!
//! That only holds when no earlier command of the tick orders the builder:
//! af_behavior applies that one after the economy pass, where it would
//! replace or follow the Build orders instead of coming before them. Such a
//! builder gets its Build orders in `SimSet::Economy` instead, once
//! af_behavior has applied the tick ([`assign_builders`]), unless a later
//! command of the tick gave it a fresh order (which is then the last word).
//! The one sequence this cannot place exactly is an order before the
//! placement and a shift-queued one after it, all for one builder within
//! one tick: the Build orders then go after both (or replace both, for an
//! unqueued placement).
//!
//! [`EconomyIntake`]: af_core::ext::economy::EconomyIntake

use af_core::ext::economy::PlacedSites;
use af_core::prelude::*;
use bevy::prelude::*;

/// Units that may be sent to build, as the economy intake and
/// [`assign_builders`] see them.
pub(crate) type BuilderQuery<'w, 's> = Query<'w, 's, (&'static Owner, &'static Unit, &'static mut Orders), Without<Dead>>;

/// Give the builder units of `list[index]`, the command that just placed
/// `sites` (in placement order), their Build orders now, in the economy
/// intake pass: all but those an earlier command of the tick also ordered,
/// which [`assign_builders`] sends later.
pub(crate) fn send_builders(
    data: &GameData,
    commands: &mut Commands,
    builders: &mut BuilderQuery,
    list: &[PlayerCommand],
    index: usize,
    sites: impl Iterator<Item = Entity> + Clone,
) {
    let cmd = &list[index];
    for (k, &e) in cmd.entities.iter().enumerate() {
        if cmd.entities[..k].contains(&e) || ordered_earlier(&list[..index], cmd.player, e) {
            continue;
        }
        give_build_orders(data, commands, builders, cmd, e, sites.clone());
    }
}

/// Give the builders [`send_builders`] left out, those an earlier command of
/// the tick also ordered, their Build orders for this tick's sites, now
/// that af_behavior has applied the tick's unit commands.
pub(crate) fn assign_builders(
    data: Res<Data>,
    tick: Res<TickCommands>,
    sites: Res<PlacedSites>,
    mut commands: Commands,
    mut builders: BuilderQuery,
) {
    for group in sites.list.chunk_by(|a, b| a.0 == b.0) {
        let index = group[0].0;
        let Some(cmd) = tick.list.get(index) else { continue };
        let (earlier, later) = (&tick.list[..index], &tick.list[index + 1..]);
        for (k, &e) in cmd.entities.iter().enumerate() {
            if cmd.entities[..k].contains(&e) || !ordered_earlier(earlier, cmd.player, e) || reordered_later(later, cmd.player, e) {
                continue;
            }
            give_build_orders(data.as_ref(), &mut commands, &mut builders, cmd, e, group.iter().map(|s| s.1));
        }
    }
}

/// Send unit `e` of the placing command `cmd` to build `sites`, in order,
/// if it is the player's own and builds.
fn give_build_orders(
    data: &GameData,
    commands: &mut Commands,
    builders: &mut BuilderQuery,
    cmd: &PlayerCommand,
    e: Entity,
    sites: impl Iterator<Item = Entity>,
) {
    let Ok((owner, unit, mut orders)) = builders.get_mut(e) else { return };
    if owner.0 != cmd.player || !data.unit(unit.kind).gatherer.as_ref().is_some_and(|g| g.builds) {
        return;
    }
    for (i, site) in sites.enumerate() {
        orders.set(Order::Build { site }, cmd.queue || i > 0);
    }
    // af_behavior refreshes the marker in `SimSet::Orders`, which has
    // already run this tick for the builders sent late: without this the
    // AI would see them idle next tick.
    commands.entity(e).try_remove::<Idle>();
}

/// Did one of the `earlier` commands of the tick give `unit` orders (which
/// af_behavior applies only after the economy pass)?
fn ordered_earlier(earlier: &[PlayerCommand], player: PlayerId, unit: Entity) -> bool {
    earlier.iter().any(|c| c.player == player && orders_units(&c.kind) && c.entities.contains(&unit))
}

/// Did one of the `later` commands give `unit` an order that replaces its
/// queue (af_behavior has already applied it)? Stop and Delete end the
/// orders whether shift-queued or not.
fn reordered_later(later: &[PlayerCommand], player: PlayerId, unit: Entity) -> bool {
    later.iter().any(|c| {
        c.player == player
            && c.entities.contains(&unit)
            && (matches!(c.kind, CommandKind::Stop | CommandKind::Delete) || (!c.queue && orders_units(&c.kind)))
    })
}

/// Unit commands af_behavior turns into orders for the units listed (new
/// ones, or none for Stop and Delete).
fn orders_units(kind: &CommandKind) -> bool {
    matches!(
        kind,
        CommandKind::Smart { .. }
            | CommandKind::Move(_)
            | CommandKind::AttackMove(_)
            | CommandKind::Attack(_)
            | CommandKind::Patrol(_)
            | CommandKind::Follow(_)
            | CommandKind::Stop
            | CommandKind::Hold
            | CommandKind::Gather(_)
            | CommandKind::Build(_)
            | CommandKind::Repair(_)
            | CommandKind::Delete
    )
}
