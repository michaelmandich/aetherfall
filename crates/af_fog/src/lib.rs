//! Aetherfall fog of war: per-team visibility and exploration.
//!
//! af_map inserts [`FogOfWar`] at match start (size, team slots, enabled);
//! this crate keeps it current in `SimSet::Vision`, on the first tick and
//! then every [`VISION_INTERVAL`] ticks:
//!
//! 1. Every team's `visible` grid is cleared, then each live unit, building
//!    and foundation stamps a filled disc of its sight into its owner's
//!    team grid (allies share a team, so they share vision). The disc is
//!    measured from the viewer's real position and reaches
//!    [`sight_radius`](af_core::ext::fog::sight_radius): `Stats::vision`
//!    beyond its box, like edge-to-edge weapon ranges. `explored`
//!    accumulates `visible`.
//! 2. Buildings and resource nodes carry
//!    [`SeenBy`](af_core::ext::fog::SeenBy): which teams see part of their
//!    box now and which have ever seen them (AoE-style memory for the
//!    renderer and the AI). Every team sees the buildings of a player
//!    af_economy has revealed (one who lost every production building, see
//!    [`RevealedPlayers`](af_core::ext::economy::RevealedPlayers)), as if
//!    they stood in its sight. Buildings also carry
//!    [`LastSeen`](af_core::ext::fog::LastSeen): how each other team last
//!    saw them.
//! 3. Buildings and nodes that die while a team that had seen them is not
//!    looking leave a [`FogGhost`](af_core::ext::fog::FogGhost) in
//!    [`FogGhosts`] until that team looks again.
//!
//! With `FogOfWar::enabled` false nothing is computed (`FogOfWar::visible`
//! answers true); with `MatchSetup::reveal_map` every cell is lit once and
//! left so. In both cases every `SeenBy` is `SeenBy::ALL`, and every player,
//! computer players included, sees the whole map.

mod grid;
mod memory;
mod vision;

use af_core::ext::fog::FogGhosts;
use af_core::prelude::*;
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;

pub use grid::SIGHT_SLACK_CELLS;
pub use vision::VISION_INTERVAL;

/// Fog-of-war simulation (headless-safe).
pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FogGhosts>()
            .init_resource::<memory::GhostBoxes>()
            .init_resource::<vision::Vision>()
            .add_systems(OnEnter(GameState::Playing), memory::reset_memory)
            .add_systems(OnExit(GameState::Playing), memory::reset_memory)
            .add_systems(
                FixedUpdate,
                (memory::remember_the_dead, vision_update().run_if(vision::vision_due))
                    .chain()
                    .in_set(SimSet::Vision),
            );
    }
}

/// One vision update: tag new buildings and nodes, recompute the grids,
/// then the memory derived from them.
fn vision_update() -> ScheduleConfigs<ScheduleSystem> {
    (memory::track_seeables, vision::update_grids, memory::update_seen_by, memory::update_last_seen, memory::update_ghosts).chain()
}

#[cfg(test)]
mod bench {
    use std::f32::consts::TAU;
    use std::time::Instant;

    use af_core::ext::fog::fog_for_map;
    use af_core::testing::*;

    use super::*;

    /// The budget case from the design brief: 6 teams x 400 viewers (a
    /// quarter of them buildings) and 3000 nodes on a 192 x 192 cell grid.
    #[test]
    #[ignore = "timing benchmark: cargo test -p af_fog --release -- --ignored --nocapture"]
    fn six_teams_of_400_on_a_192_cell_grid() {
        const MAP: f32 = 384.0;
        let mut app = sim_app(test_data());
        app.add_plugins(FogPlugin);
        let teams = [("ironhold", 0), ("sylvan", 1), ("dune", 2), ("tide", 3), ("steppe", 4), ("starborn", 5)];
        let players = flat_world(&mut app, MAP, &teams);
        app.insert_resource(fog_for_map(Vec2::splat(MAP), 2.0, teams.len(), true));
        let mut rng = SimRng::new(7);
        with_commands(&mut app, |commands, data, list| {
            for p in &players {
                let player = list.get(*p);
                let faction = data.faction(player.faction);
                for i in 0..400 {
                    let pos = Vec2::new(rng.range(0.0, MAP), rng.range(0.0, MAP));
                    if i % 4 == 0 {
                        let kind = faction.building_ids[i / 4 % faction.building_ids.len()];
                        spawn_building(commands, data, player, kind, pos, rng.range(0.0, TAU), true);
                    } else {
                        spawn_unit(commands, data, player, faction.unit_ids[i % faction.unit_ids.len()], pos, 0.0);
                    }
                }
            }
            let nodes: Vec<NodeTypeId> = data.node_ids().collect();
            for i in 0..3000 {
                let pos = Vec2::new(rng.range(0.0, MAP), rng.range(0.0, MAP));
                spawn_node(commands, data, nodes[i % nodes.len()], pos, rng.range(0.0, TAU), None);
            }
        });
        let mut schedule = Schedule::default();
        schedule.add_systems(vision_update());
        // The first run tags 5400 entities and builds the disc cache.
        schedule.run(app.world_mut());

        let runs = 200;
        let start = Instant::now();
        for _ in 0..runs {
            schedule.run(app.world_mut());
        }
        let per_update = start.elapsed() / runs;
        let fog = app.world().resource::<FogOfWar>();
        let lit: usize = fog.teams.iter().map(|t| t.visible.iter().filter(|v| **v != 0).count()).sum();
        println!("vision update: {per_update:?} ({lit} team-cells lit of {})", 6 * fog.width * fog.height);
        assert!(lit > 0);
    }
}
