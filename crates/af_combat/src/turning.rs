//! Room to turn. Standing units turn to face their targets in
//! `SimSet::Combat`, after af_movement has resolved the tick's collisions,
//! so a turn must not swing the box into a neighbour: boxes never overlap.
//! An elongated rider or engine squeezed into a crowd or against a wall
//! turns only as far as it does not cut deeper into anything than
//! [`TURN_SLACK`] (or than it already did); next tick the collision solver
//! makes the room it needs for the next bit of the turn.

use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// Deepest overlap with a neighbour a turn may leave for the collision
/// solver to clear next tick (metres).
const TURN_SLACK: f32 = 0.03;
/// Fractions of the wanted turn tried, largest first.
const TURN_FRACTIONS: [f32; 3] = [1.0, 0.5, 0.25];
/// How far a unit may have moved since the spatial index was rebuilt at the
/// start of the tick (metres).
const MOVE_MARGIN: f32 = 0.5;

/// What a turning unit may bump into.
#[derive(SystemParam)]
pub(crate) struct Room<'w, 's> {
    spatial: Res<'w, SpatialIndex>,
    players: Res<'w, Players>,
    gates: Query<'w, 's, (), With<Gate>>,
}

impl Room<'_, '_> {
    /// The yaw a ground unit (`me`, owned by `owner`, its box `body` facing
    /// the way it does now) turning toward `to` may take this tick: `to`, or
    /// the largest part of the turn that leaves room. `unit_box` gives a
    /// neighbouring unit's current box (`None`: ignore it, e.g. a flyer).
    pub(crate) fn turn(&self, me: Entity, owner: PlayerId, body: Obb, to: f32, unit_box: impl Fn(Entity) -> Option<Obb>) -> f32 {
        let (pos, half, from) = (body.center, body.half, body.yaw);
        let team = self.players.team(owner);
        let mut near: Vec<Obb> = Vec::new();
        let reach = half.length() + MOVE_MARGIN;
        self.spatial.for_each_near(pos, reach, |e| {
            if e.entity == me {
                return;
            }
            let obb = match e.kind {
                SpatialKind::Unit if !e.flying => unit_box(e.entity),
                SpatialKind::Building if e.team == team && self.gates.contains(e.entity) => None,
                SpatialKind::Building => Some(e.obb),
                SpatialKind::Node if e.blocking => Some(e.obb),
                _ => None,
            };
            near.extend(obb);
        });
        if near.is_empty() {
            return to;
        }
        let depth = |yaw: f32| near.iter().filter_map(|o| Obb::new(pos, half, yaw).mtv(o)).map(|m| m.length()).fold(0.0f32, f32::max);
        let allowed = depth(from).max(TURN_SLACK);
        let delta = af_geom::wrap_angle(to - from);
        TURN_FRACTIONS
            .iter()
            .map(|f| if *f == 1.0 { to } else { af_geom::wrap_angle(from + delta * f) })
            .find(|yaw| depth(*yaw) <= allowed)
            .unwrap_or(from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};

    /// Spawns a rider beside a wall of houses and asks how far it may turn.
    fn turn_beside(gap: f32, from: f32, to: f32) -> f32 {
        let data = test_data();
        let mut app = sim_app(data.clone());
        let p = flat_world(&mut app, 64.0, &[("ironhold", 0)]);
        let house = data.building_id("house").expect("house");
        let half_house = data.building(house).size[0] * 0.5;
        with_commands(&mut app, |c, data, players| spawn_building(c, data, players.get(p[0]), house, Vec2::new(30.0, 30.0), 0.0, true));
        // A long box standing north-south, its side `gap` from the house's east wall.
        let half = Vec2::new(0.45, 0.95);
        let pos = Vec2::new(30.0 + half_house + half.x + gap, 30.0);
        run_ticks(&mut app, 1);
        let mut state = bevy::ecs::system::SystemState::<Room>::new(app.world_mut());
        let room = state.get(app.world()).expect("room");
        room.turn(Entity::PLACEHOLDER, p[0], Obb::new(pos, half, from), to, |_| None)
    }

    #[test]
    fn a_turn_into_a_wall_stops_short() {
        let quarter = std::f32::consts::FRAC_PI_2;
        // Room to spare: the whole turn.
        assert_eq!(turn_beside(1.0, 0.0, 0.3), 0.3);
        // Flush against the wall: turning across would swing the long box in.
        // Flush against the wall: either way, a corner swings into it.
        assert_eq!(turn_beside(0.0, 0.0, quarter * 0.3), 0.0);
        assert_eq!(turn_beside(0.0, 0.0, -quarter * 0.3), 0.0);
        // A hand's breadth from it: part of the turn.
        let yaw = turn_beside(0.1, 0.0, quarter * 0.3);
        assert!(yaw > 0.0 && yaw < quarter * 0.3, "turned to {yaw}");
    }
}
