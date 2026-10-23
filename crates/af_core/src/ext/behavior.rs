//! Shared items added by af_behavior (other crates may use them).
//! Only af_behavior edits this file. Never modify items in other af_core files.

use crate::prelude::*;
use bevy::math::Vec2;

/// How a gather target is worked, as `Activity::Gather` reports it: a
/// resource-field building (a farm) is farmed, whatever it yields; a node
/// is worked in the style of its resource (`ResourceDef::work`). `node` is
/// the target's `ResourceNode`, `field` whether it has a `ResourceField`;
/// `None` when it has neither (gone, or not a source).
pub fn gather_style(data: &GameData, node: Option<&ResourceNode>, field: bool) -> Option<WorkStyle> {
    match node {
        Some(n) => Some(data.resource(n.resource).work),
        None => field.then_some(WorkStyle::Farm),
    }
}

/// The points of the patrol route that starts with the first of `orders`
/// (a unit's queue, front first: `&orders.queue`), in order: its `a` and
/// `b`, then the `b` of each `Order::Patrol` queued straight behind it
/// (more patrol points shift-clicked, each queued from the last one's
/// `b`). The unit walks them in turn and from the last back to the first,
/// forever; a single patrol is the two-point a <-> b beat. Orders queued
/// after the patrols are never reached. Empty when the first order is not
/// a patrol. af_behavior walks it; renderers draw it.
pub fn patrol_points<'a>(orders: impl IntoIterator<Item = &'a Order>) -> impl Iterator<Item = Vec2> {
    // Fused: `map_while` would carry on past the first other order.
    let mut patrols = orders
        .into_iter()
        .map_while(|o| match o {
            Order::Patrol { a, b } => Some((*a, *b)),
            _ => None,
        })
        .fuse();
    let first = patrols.next();
    first.map(|(a, _)| a).into_iter().chain(first.map(|(_, b)| b)).chain(patrols.map(|(_, b)| b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_farmed_and_nodes_worked_by_resource() {
        let data = crate::testing::test_data();
        let node = |id: &str| {
            let kind = data.node_id(id).unwrap_or_else(|| panic!("node {id}"));
            ResourceNode { kind, resource: data.node(kind).resource_id, remaining: 1.0, claimed_by: None }
        };
        assert_eq!(gather_style(&data, Some(&node("oak")), false), Some(WorkStyle::Chop));
        assert_eq!(gather_style(&data, Some(&node("berry_bush")), false), Some(WorkStyle::Forage));
        assert_eq!(gather_style(&data, None, true), Some(WorkStyle::Farm));
        assert_eq!(gather_style(&data, None, false), None);
    }

    #[test]
    fn patrol_routes_chain_the_patrols_at_the_front() {
        let p = |x: f32| Vec2::new(x, 0.0);
        let orders = |list: &[Order]| Orders { queue: list.iter().copied().collect() };
        let route = |o: &Orders| patrol_points(&o.queue).collect::<Vec<_>>();
        assert!(route(&orders(&[])).is_empty());
        assert!(route(&orders(&[Order::Move { target: p(1.0) }, Order::Patrol { a: p(1.0), b: p(2.0) }])).is_empty(), "not patrolling yet");
        assert_eq!(route(&orders(&[Order::Patrol { a: p(0.0), b: p(1.0) }])), vec![p(0.0), p(1.0)]);
        let chained = orders(&[
            Order::Patrol { a: p(0.0), b: p(1.0) },
            Order::Patrol { a: p(1.0), b: p(2.0) },
            Order::Patrol { a: p(2.0), b: p(3.0) },
            Order::Move { target: p(9.0) },
            Order::Patrol { a: p(9.0), b: p(8.0) },
        ]);
        assert_eq!(route(&chained), vec![p(0.0), p(1.0), p(2.0), p(3.0)], "a move ends the route");
    }
}
