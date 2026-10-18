//! Observers that undo a building's economic footprint when it leaves the
//! world, whatever removed it (destroyed, deleted, match teardown). They run
//! while af_core's `Cleanup` applies the despawn, so the components are
//! still readable.

use af_core::prelude::*;
use bevy::prelude::*;

/// Free the node a building claimed (an extractor's vent) so it can be built on again.
pub(crate) fn release_claim(remove: On<Remove, ClaimsNode>, claims: Query<&ClaimsNode>, mut nodes: Query<&mut ResourceNode>) {
    let Ok(claim) = claims.get(remove.entity) else { return };
    if let Ok(mut node) = nodes.get_mut(claim.0)
        && node.claimed_by == Some(remove.entity)
    {
        node.claimed_by = None;
    }
}

/// Research queued in a lost building is lost with it (no refund, as with
/// units in its queue), but the tech may be queued again elsewhere.
pub(crate) fn forget_lost_research(remove: On<Remove, ProductionQueue>, queues: Query<(&Owner, &ProductionQueue)>, players: Option<ResMut<Players>>) {
    let (Ok((owner, queue)), Some(mut players)) = (queues.get(remove.entity), players) else { return };
    if !queue.items.iter().any(|i| matches!(i.kind, QueueKind::Tech(_))) {
        return;
    }
    let Some(p) = players.list.get_mut(owner.0.index()) else { return };
    for item in &queue.items {
        if let QueueKind::Tech(t) = item.kind
            && let Some(busy) = p.researching.get_mut(t.index())
        {
            *busy = false;
        }
    }
}
