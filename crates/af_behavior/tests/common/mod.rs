//! Headless test harness: the behaviour plugin on a flat map, with a
//! straight-line stand-in for af_movement (af_movement is not a dependency).

use af_behavior::BehaviorPlugin;
use af_core::prelude::*;
use af_core::testing::{command, flat_world, run_ticks, sim_app, test_data, with_commands};
use bevy::prelude::*;

pub const MAP: f32 = 128.0;

/// As in af_movement: an arrived follower only resumes once the gap grows
/// this much beyond `arrive`.
const RESUME_MARGIN: f32 = 0.2;

/// Entities the stand-in path service cannot reach: goals following them fail.
#[derive(Resource, Default)]
struct Unreachable(Vec<Entity>);

pub struct Sim {
    pub app: App,
}

impl Sim {
    /// Players are (faction, team).
    pub fn new(players: &[(&str, u8)]) -> Sim {
        let mut app = sim_app(test_data());
        app.add_plugins(BehaviorPlugin);
        app.init_resource::<Unreachable>();
        app.add_systems(FixedUpdate, walk_to_goals.in_set(SimSet::Movement));
        flat_world(&mut app, MAP, players);
        Sim { app }
    }

    pub fn data(&self) -> std::sync::Arc<GameData> {
        self.app.world().resource::<Data>().0.clone()
    }

    pub fn unit(&mut self, player: u8, id: &str, pos: Vec2) -> Entity {
        with_commands(&mut self.app, |c, data, players| {
            let kind = data.unit_id(id).unwrap_or_else(|| panic!("unit {id}"));
            spawn_unit(c, data, players.get(PlayerId(player)), kind, pos, 0.0)
        })
    }

    pub fn building(&mut self, player: u8, id: &str, pos: Vec2, complete: bool) -> Entity {
        with_commands(&mut self.app, |c, data, players| {
            let kind = data.building_id(id).unwrap_or_else(|| panic!("building {id}"));
            spawn_building(c, data, players.get(PlayerId(player)), kind, pos, 0.0, complete)
        })
    }

    pub fn node(&mut self, id: &str, pos: Vec2, amount: Option<f32>) -> Entity {
        with_commands(&mut self.app, |c, data, _| {
            let kind = data.node_id(id).unwrap_or_else(|| panic!("node {id}"));
            spawn_node(c, data, kind, pos, 0.0, amount)
        })
    }

    /// From now on nothing can path to `e` (it sits on a cliff, behind a wall...).
    pub fn make_unreachable(&mut self, e: Entity) {
        self.app.world_mut().resource_mut::<Unreachable>().0.push(e);
    }

    /// What af_economy does when research ends: the tech's effects join the
    /// player's modifiers (units spawned afterwards get the new stats).
    pub fn research(&mut self, player: u8, id: &str) {
        let data = self.data();
        let tech = data.tech_id(id).unwrap_or_else(|| panic!("tech {id}"));
        let mut players = self.app.world_mut().resource_mut::<Players>();
        let mods = &mut players.list[player as usize].mods;
        for effect in &data.tech(tech).effects {
            mods.apply_effect(&data, effect);
        }
    }

    /// What af_economy does when a foundation reaches full progress.
    pub fn finish(&mut self, site: Entity) {
        self.app.world_mut().entity_mut(site).remove::<UnderConstruction>();
    }

    /// af_movement gives up on `e`'s current goal, as after being stuck in
    /// a crowd for a few seconds.
    pub fn fail_goal(&mut self, e: Entity) {
        let seq = self.get::<MoveGoal>(e).expect("a goal").seq;
        let mut path = self.get::<PathState>(e).expect("path state");
        path.status = PathStatus::Failed;
        path.goal_seq = seq;
        self.set(e, path);
    }

    pub fn order(&mut self, player: u8, entities: &[Entity], kind: CommandKind) {
        command(&mut self.app, PlayerCommand::new(PlayerId(player), entities.to_vec(), kind));
    }

    pub fn queue(&mut self, player: u8, entities: &[Entity], kind: CommandKind) {
        command(&mut self.app, PlayerCommand::new(PlayerId(player), entities.to_vec(), kind).queued(true));
    }

    pub fn run_secs(&mut self, secs: f32) {
        run_ticks(&mut self.app, (secs * SIM_HZ as f32).round() as u32);
    }

    pub fn ticks(&mut self, n: u32) {
        run_ticks(&mut self.app, n);
    }

    pub fn get<T: Component + Clone>(&self, e: Entity) -> Option<T> {
        self.app.world().get::<T>(e).cloned()
    }

    pub fn has<T: Component>(&self, e: Entity) -> bool {
        self.app.world().get::<T>(e).is_some()
    }

    pub fn alive(&self, e: Entity) -> bool {
        self.app.world().get_entity(e).is_ok()
    }

    pub fn set<T: Component>(&mut self, e: Entity, value: T) {
        self.app.world_mut().entity_mut(e).insert(value);
    }

    pub fn pos(&self, e: Entity) -> Vec2 {
        self.get::<Position>(e).expect("position").0
    }

    pub fn current_order(&self, e: Entity) -> Option<Order> {
        self.get::<Orders>(e).and_then(|o| o.current().copied())
    }

    pub fn resource(&self, player: u8, id: &str) -> f32 {
        let r = self.data().resource_id(id).expect("resource");
        self.app.world().resource::<Players>().get(PlayerId(player)).resources.get(r)
    }

    pub fn gathered(&self, player: u8, id: &str) -> f32 {
        let r = self.data().resource_id(id).expect("resource");
        self.app.world().resource::<Players>().get(PlayerId(player)).stats.gathered.get(r)
    }

    pub fn noticed(&self, text: &str) -> bool {
        self.app.world().resource::<Notices>().list.iter().any(|n| n.text == text)
    }

    /// Edge-to-edge distance between two entities' boxes.
    pub fn gap(&self, a: Entity, b: Entity) -> f32 {
        let obb = |e| {
            let w = self.app.world();
            w.get::<Footprint>(e).unwrap().obb(w.get::<Position>(e).unwrap().0, w.get::<Facing>(e).unwrap().0)
        };
        obb(a).gap(&obb(b))
    }
}

/// Stand-in for af_movement: walks each unit straight at its goal at its
/// speed and reports progress in `PathState` for the goal's `seq`, the way
/// the real path service does: an arrived point goal stays arrived, and an
/// arrived follower resumes only past `arrive + RESUME_MARGIN`. No
/// collisions.
#[allow(clippy::type_complexity)]
fn walk_to_goals(
    unreachable: Res<Unreachable>,
    mut steps: Local<Vec<(Entity, Vec2, PathStatus, u32)>>,
    mut set: ParamSet<(
        Query<(Entity, &Position, &Facing, &Footprint, &MoveGoal, &PathState, &Stats), Without<Dead>>,
        Query<(&Position, &Facing, &Footprint)>,
        Query<(&mut Position, &mut PathState)>,
    )>,
) {
    let walkers: Vec<_> = set
        .p0()
        .iter()
        .map(|(e, p, f, fp, g, path, s)| (e, p.0, f.0, *fp, *g, path.goal_seq == g.seq && path.status == PathStatus::Arrived, s.speed))
        .collect();
    steps.clear();
    for (e, pos, facing, fp, goal, arrived, speed) in walkers {
        let stride = speed * SIM_DT;
        let step = match goal.follow {
            Some(t) if unreachable.0.contains(&t) => (pos, PathStatus::Failed),
            Some(t) => match set.p1().get(t) {
                Ok((tp, tf, tfp)) => {
                    let gap = fp.obb(pos, facing).gap(&tfp.obb(tp.0, tf.0));
                    if gap <= goal.arrive || (arrived && gap <= goal.arrive + RESUME_MARGIN) {
                        (pos, PathStatus::Arrived)
                    } else {
                        let dir = (tp.0 - pos).normalize_or_zero();
                        (pos + dir * stride.min(gap - goal.arrive + 0.01), PathStatus::Following)
                    }
                }
                Err(_) => (pos, PathStatus::Failed),
            },
            None => {
                let d = goal.target - pos;
                let dist = d.length();
                if arrived || dist <= goal.arrive {
                    (pos, PathStatus::Arrived)
                } else {
                    (pos + d / dist * stride.min(dist), PathStatus::Following)
                }
            }
        };
        steps.push((e, step.0, step.1, goal.seq));
    }
    let mut q = set.p2();
    for (e, p, status, seq) in steps.drain(..) {
        if let Ok((mut pos, mut path)) = q.get_mut(e) {
            pos.0 = p;
            path.status = status;
            path.goal_seq = seq;
        }
    }
}
