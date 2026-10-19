//! Shared items added by af_behavior (other crates may use them).
//! Only af_behavior edits this file. Never modify items in other af_core files.

use crate::prelude::*;

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
}
