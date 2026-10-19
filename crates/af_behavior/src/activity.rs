//! The shared [`Activity`] of every unit, from what its brain did this tick.

use af_core::ext::behavior::gather_style;
use af_core::prelude::*;
use bevy::prelude::*;

use crate::brain::{Brain, Phase};

/// What a unit with these facts is busy with. Work at a target (gathering,
/// building, repairing) wins over mending, mending over fighting.
///
/// * `front`: the unit's current order, if its brain is running it;
/// * `phase`: where that order is;
/// * `source`: for a Gather order, how its target is worked;
/// * `engaged`: the unit has a hostile to fight (an `AttackTarget`, a
///   self-picked foe, or a forced Attack order).
pub(crate) fn activity_of(front: Option<&Order>, phase: Phase, source: Option<WorkStyle>, mending: bool, engaged: bool) -> Activity {
    let working = phase == Phase::Work;
    match front {
        Some(Order::Gather { .. }) if working => {
            if let Some(style) = source {
                return Activity::Gather(style);
            }
        }
        Some(Order::Build { .. }) if working => return Activity::Build,
        Some(Order::Repair { .. }) if working => return Activity::Repair,
        _ => {}
    }
    if mending {
        Activity::Heal
    } else if engaged {
        Activity::Fight
    } else {
        Activity::Idle
    }
}

/// Runs last in `SimSet::Orders`, once every order system and the stances
/// have had their say (and their commands are applied).
#[allow(clippy::type_complexity)]
pub(crate) fn update_activity(
    data: Res<Data>,
    mut units: Query<(&Orders, &Brain, Has<AttackTarget>, &mut Activity), (With<Unit>, Without<Dead>)>,
    sources: Query<(Option<&ResourceNode>, Has<ResourceField>), Without<Dead>>,
) {
    for (orders, brain, attacking, mut activity) in &mut units {
        let front = orders.current().filter(|_| brain.synced(orders));
        let source = match front {
            Some(Order::Gather { target }) => sources.get(*target).ok().and_then(|(node, field)| gather_style(&data, node, field)),
            _ => None,
        };
        let engaged = attacking || brain.engaged.is_some() || matches!(front, Some(Order::Attack { .. }));
        activity.set_if_neq(activity_of(front, brain.phase, source, brain.mending, engaged));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity() -> Entity {
        Entity::from_raw_u32(7).unwrap()
    }

    #[test]
    fn work_at_the_target_names_the_task() {
        let gather = Order::Gather { target: entity() };
        let build = Order::Build { site: entity() };
        let repair = Order::Repair { target: entity() };
        assert_eq!(activity_of(Some(&gather), Phase::Work, Some(WorkStyle::Chop), false, false), Activity::Gather(WorkStyle::Chop));
        assert_eq!(activity_of(Some(&build), Phase::Work, None, false, false), Activity::Build);
        assert_eq!(activity_of(Some(&repair), Phase::Work, None, false, false), Activity::Repair);
        // Walking there, or home with the load, is no work yet.
        for phase in [Phase::Fresh, Phase::Approach, Phase::Deliver] {
            assert_eq!(activity_of(Some(&gather), phase, Some(WorkStyle::Chop), false, false), Activity::Idle, "{phase:?}");
            assert_eq!(activity_of(Some(&build), phase, None, false, false), Activity::Idle, "{phase:?}");
        }
        // A gather target that vanished this tick.
        assert_eq!(activity_of(Some(&gather), Phase::Work, None, false, false), Activity::Idle);
    }

    #[test]
    fn work_beats_mending_beats_fighting() {
        let gather = Order::Gather { target: entity() };
        assert_eq!(activity_of(Some(&gather), Phase::Work, Some(WorkStyle::Mine), true, true), Activity::Gather(WorkStyle::Mine));
        assert_eq!(activity_of(None, Phase::Fresh, None, true, true), Activity::Heal);
        assert_eq!(activity_of(None, Phase::Fresh, None, false, true), Activity::Fight);
        assert_eq!(activity_of(Some(&Order::Hold), Phase::Work, None, false, false), Activity::Idle);
    }
}
