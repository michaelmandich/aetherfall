//! What teams remember: `SeenBy` on buildings and nodes, `LastSeen` on
//! buildings, and ghosts of the ones that died out of sight.

use af_core::ext::economy::RevealedPlayers;
use af_core::ext::fog::{team_bit, team_ids, BuildingSighting, FogBox, FogGhost, FogGhosts, LastSeen, SeenBy};
use af_core::prelude::*;
use bevy::prelude::*;

use crate::vision::{FogMode, Vision};

/// The fog box of a building or node. Cached because these almost never
/// move, and recomputing rotated boxes for thousands of nodes every update
/// would cost more trigonometry than the vision itself.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct SightBox(FogBox);

impl SightBox {
    fn of(pos: &Position, facing: &Facing, footprint: &Footprint) -> SightBox {
        SightBox(FogBox::new(&footprint.obb(pos.0, facing.0)))
    }
}

/// The boxes of `FogGhosts::list`, entry for entry (same reason as
/// [`SightBox`]).
#[derive(Resource, Debug, Default)]
pub(crate) struct GhostBoxes(Vec<(Entity, FogBox)>);

impl GhostBoxes {
    /// Line the boxes up with `ghosts`. Only af_fog is meant to write the
    /// ghost list, but it is public: recompute any entry that does not match.
    fn sync(&mut self, ghosts: &[FogGhost]) {
        self.0.truncate(ghosts.len());
        for (i, ghost) in ghosts.iter().enumerate() {
            if self.0.get(i).is_some_and(|(e, _)| *e == ghost.entity) {
                continue;
            }
            let entry = (ghost.entity, FogBox::new(&ghost.footprint.obb(ghost.pos, ghost.facing)));
            match self.0.get_mut(i) {
                Some(slot) => *slot = entry,
                None => self.0.push(entry),
            }
        }
    }
}

type Unseen = (Or<(With<Building>, With<ResourceNode>)>, Without<SeenBy>, Without<Dead>);
type Moved = Or<(Changed<Position>, Changed<Facing>, Changed<Footprint>)>;

/// Give new buildings and nodes their `SeenBy` (buildings also `LastSeen`)
/// and cached box, and refresh the box of any that moved or changed shape.
pub(crate) fn track_seeables(
    mut commands: Commands,
    fresh: Query<(Entity, &Position, &Facing, &Footprint, Has<Building>), Unseen>,
    mut moved: Query<(&Position, &Facing, &Footprint, &mut SightBox), Moved>,
) {
    for (entity, pos, facing, footprint, building) in &fresh {
        let mut entity = commands.entity(entity);
        entity.try_insert((SeenBy::default(), SightBox::of(pos, facing, footprint)));
        if building {
            entity.try_insert(LastSeen::default());
        }
    }
    for (pos, facing, footprint, mut sight) in &mut moved {
        sight.set_if_neq(SightBox::of(pos, facing, footprint));
    }
}

/// Mark which teams see each building and node now, and remember every team
/// that ever has. The buildings of a [revealed](RevealedPlayers) player are
/// seen by every team. With fog off or the map revealed, everyone sees
/// everything.
pub(crate) fn update_seen_by(
    fog: Res<FogOfWar>,
    vision: Res<Vision>,
    revealed: Option<Res<RevealedPlayers>>,
    mut seeables: Query<(&mut SeenBy, &SightBox, Option<&Owner>), Without<Dead>>,
) {
    if vision.mode == FogMode::Live {
        let revealed = revealed.as_deref().filter(|r| !r.is_empty());
        for (mut seen, sight, owner) in &mut seeables {
            let mut visible = vision.teams_seeing(&fog, &sight.0);
            if let (Some(revealed), Some(owner)) = (revealed, owner)
                && revealed.contains(owner.0)
            {
                visible |= vision.team_mask;
            }
            seen.set_if_neq(SeenBy { teams: seen.teams | visible, visible });
        }
    } else {
        for (mut seen, ..) in &mut seeables {
            seen.set_if_neq(SeenBy { teams: SeenBy::ALL, visible: SeenBy::ALL });
        }
    }
}

/// Record how every other team that sees a building sees it now, for when
/// it no longer does.
#[allow(clippy::type_complexity)]
pub(crate) fn update_last_seen(
    vision: Res<Vision>,
    players: Res<Players>,
    mut buildings: Query<(&SeenBy, &Building, Option<&Owner>, Option<&Health>, Option<&UnderConstruction>, &mut LastSeen), Without<Dead>>,
) {
    if vision.mode != FogMode::Live {
        return;
    }
    for (seen, building, owner, health, construction, mut last) in &mut buildings {
        let own = owner.map_or(0, |o| team_bit(players.team(o.0)));
        let teams = seen.visible & vision.team_mask & !own;
        if teams == 0 {
            continue;
        }
        let now = BuildingSighting {
            kind: building.kind,
            construction: construction.map(|c| c.progress.clamp(0.0, 1.0)),
            hp: health.map_or(1.0, Health::fraction),
        };
        // Read first: writing through `Mut` would flag a change every update.
        if !last.is_current(teams, &now) {
            last.record(teams, now);
        }
    }
}

/// When a building or node dies, every team that had seen it but is not
/// looking keeps a ghost of it, of the type it last saw. Runs every tick so
/// no death is missed; the dying entity still exists until
/// `SimSet::Cleanup`.
pub(crate) fn remember_the_dead(
    mut died: MessageReader<EntityDied>,
    vision: Res<Vision>,
    seeables: Query<(&SeenBy, &Position, &Facing, &Footprint, Option<&LastSeen>)>,
    mut ghosts: ResMut<FogGhosts>,
    mut boxes: ResMut<GhostBoxes>,
) {
    let live = vision.mode == FogMode::Live;
    let before = ghosts.list.len();
    for death in died.read().filter(|_| live) {
        let Ok((seen, pos, facing, footprint, last)) = seeables.get(death.entity) else { continue };
        let teams = seen.teams & !seen.visible & vision.team_mask;
        // Ghosts from earlier ticks cannot be this entity: it is despawned in
        // the tick it dies.
        if teams == 0 || ghosts.list[before..].iter().any(|g| g.entity == death.entity) {
            continue;
        }
        let area = FogBox::new(&footprint.obb(pos.0, facing.0));
        let first = ghosts.list.len();
        for team in team_ids(teams) {
            let kind = last.and_then(|l| l.by(team)).map_or(death.kind, |s| EntityKind::Building(s.kind));
            match ghosts.list[first..].iter_mut().find(|g| g.kind == kind) {
                Some(ghost) => ghost.teams |= team_bit(team),
                None => {
                    let ghost = FogGhost {
                        entity: death.entity,
                        kind,
                        owner: death.owner,
                        pos: pos.0,
                        facing: facing.0,
                        footprint: *footprint,
                        teams: team_bit(team),
                    };
                    ghosts.list.push(ghost);
                    boxes.0.push((death.entity, area));
                }
            }
        }
    }
}

/// A team forgets a ghost once it sees any of the ghost's box again.
pub(crate) fn update_ghosts(fog: Res<FogOfWar>, vision: Res<Vision>, mut ghosts: ResMut<FogGhosts>, mut boxes: ResMut<GhostBoxes>) {
    if vision.mode != FogMode::Live && !ghosts.list.is_empty() {
        ghosts.list.clear();
    }
    if ghosts.list.is_empty() {
        boxes.0.clear();
        return;
    }
    let list = &mut ghosts.bypass_change_detection().list;
    boxes.sync(list);
    let mut changed = false;
    let mut kept = 0;
    for i in 0..list.len() {
        let ghost = &mut list[i];
        let teams = ghost.teams & vision.team_mask & !vision.teams_seeing(&fog, &boxes.0[i].1);
        changed |= teams != ghost.teams;
        ghost.teams = teams;
        if teams != 0 {
            // Compact in place, keeping the oldest-first order.
            list.swap(kept, i);
            boxes.0.swap(kept, i);
            kept += 1;
        }
    }
    list.truncate(kept);
    boxes.0.truncate(kept);
    if changed {
        ghosts.set_changed();
    }
}

/// Forget everything at the start and end of a match.
pub(crate) fn reset_memory(mut ghosts: ResMut<FogGhosts>, mut boxes: ResMut<GhostBoxes>, mut vision: ResMut<Vision>) {
    ghosts.list.clear();
    boxes.0.clear();
    *vision = Vision::default();
}
