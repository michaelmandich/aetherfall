//! Passive income (`SimSet::Economy`): complete buildings' `Trickle` plus
//! each player's tech trickle, credited every tick. A building on a node
//! (an extractor on its vent) draws that node's resource out of it.

use af_core::prelude::*;
use bevy::prelude::*;

pub(crate) fn trickle_income(
    data: Res<Data>,
    clock: Res<SimClock>,
    mut players: ResMut<Players>,
    mut notices: ResMut<Notices>,
    sources: Query<(&Owner, &Building, &Trickle, Option<&ClaimsNode>), (Without<UnderConstruction>, Without<Dead>)>,
    mut nodes: Query<(&mut ResourceNode, &Position), Without<Dead>>,
) {
    let data: &GameData = &data;
    for (owner, building, trickle, claim) in &sources {
        let Some(p) = players.list.get_mut(owner.0.index()) else { continue };
        let mut income = trickle.0.scaled(SIM_DT);
        if let (Some(claim), Some(node_kind)) = (claim, data.building(building.kind).requires_node_id) {
            let r = data.node(node_kind).resource_id;
            let take = match nodes.get_mut(claim.0) {
                Ok((mut node, pos)) => {
                    let take = income.get(r).min(node.remaining).max(0.0);
                    if take > 0.0 {
                        node.remaining -= take;
                        if node.remaining <= 0.0 {
                            let text = format!("{} exhausted", data.node(node_kind).name);
                            notices.push(Some(p.id), NoticeKind::Info, text, Some(pos.0), clock.elapsed);
                        }
                    }
                    take
                }
                // The node is gone, and its resource with it.
                Err(_) => 0.0,
            };
            // The building's other trickles (the extractor's adamant seep) flow on.
            income.set(r, take);
        }
        credit(p, income);
    }
    for p in players.list.iter_mut().filter(|p| !p.defeated && !p.mods.trickle.is_zero()) {
        let income = p.mods.trickle.scaled(SIM_DT);
        credit(p, income);
    }
}

fn credit(p: &mut PlayerState, income: ResAmounts) {
    p.resources.add_all(&income);
    p.stats.gathered.add_all(&income);
}
