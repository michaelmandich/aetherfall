//! Construction completion (`SimSet::Economy`). Builders (af_behavior) raise
//! `UnderConstruction::progress` and hp; this finishes the foundation.

use af_core::prelude::*;
use bevy::prelude::*;

/// Complete every foundation whose progress reached 1: it keeps the hp its
/// builders put in (at least 1), its shield fills, and it starts working.
pub(crate) fn complete_construction(
    data: Res<Data>,
    clock: Res<SimClock>,
    mut players: ResMut<Players>,
    mut notices: ResMut<Notices>,
    mut commands: Commands,
    mut completed: MessageWriter<BuildingCompleted>,
    mut sites: Query<(Entity, &Building, &Owner, &Position, &UnderConstruction, &mut Health, Option<&mut Shield>), Without<Dead>>,
) {
    for (e, building, owner, pos, site, mut health, shield) in &mut sites {
        if site.progress < 1.0 {
            continue;
        }
        commands.entity(e).try_remove::<UnderConstruction>();
        let hp = health.hp.clamp(1.0, health.max.max(1.0));
        if health.hp != hp {
            health.hp = hp;
        }
        if let Some(mut s) = shield
            && s.hp != s.max
        {
            s.hp = s.max;
        }
        completed.write(BuildingCompleted { entity: e, owner: owner.0, kind: building.kind });
        if let Some(p) = players.list.get_mut(owner.0.index()) {
            p.stats.buildings_built += 1;
        }
        let def = data.building(building.kind);
        if !def.wall {
            notices.push(Some(owner.0), NoticeKind::Info, format!("{} complete", def.name), Some(pos.0), clock.elapsed);
        }
    }
}
