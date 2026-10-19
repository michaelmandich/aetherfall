//! Felled trees: when a tree's wood runs out its model leaves the node,
//! falls over away from the worker who felled it, lies a moment and sinks
//! ([`super::place::fall_pose`]); its stump, when the model has one, stands
//! for [`STUMP_HOLD`] seconds and then sinks too. Both are client-only
//! scene entities ([`RenderScene`]), kept in [`Remnants`] oldest first and
//! capped ([`MAX_FALLING`], [`MAX_STUMPS`]), so a clear-cut never piles
//! entities up and no frame looks at more than the oldest standing stump.

use std::collections::VecDeque;

use af_core::ext::fog::SeenBy;
use af_core::prelude::*;
use bevy::prelude::*;

use super::library::{StaticId, StaticLibrary};
use super::place::{STUMP_HOLD, STUMP_SINK, fall_direction, fall_pose, sink_drop, tipped};
use super::{StaticVisual, spawn_instance, stump_transform};
use crate::assets::{BoxPart, Painter};
use crate::entities::BoxVisual;
use crate::fog::{Viewer, node_in_sight};
use crate::model::rig::TeamTints;
use crate::scene::RenderScene;

/// Trees falling at once, at most; a further one ends the oldest fall.
pub const MAX_FALLING: usize = 32;
/// Stumps standing at once, at most; a further one sinks the oldest early.
pub const MAX_STUMPS: usize = 200;
/// How near (metres) a worker must stand to a tree, if none is ordered to
/// gather it, to count as its feller.
const FELLER_REACH: f32 = 3.0;

/// A felled tree's model, detached from its node, falling.
#[derive(Component, Clone, Copy, Debug)]
pub struct FallingTree {
    /// Seconds since it was felled.
    pub age: f32,
    /// Its world transform standing.
    pub rest: Transform,
    /// The base of its trunk, on the ground: what it tips about.
    pub pivot: Vec3,
    /// Which way it falls (a unit vector on the ground).
    pub dir: Vec2,
    /// How deep it sinks once it lies (metres).
    pub depth: f32,
    /// The stump to plant (its model and world transform), once.
    pub stump: Option<(StaticId, Transform)>,
}

/// A stump left by a felled tree.
#[derive(Component, Clone, Copy, Debug)]
pub struct Stump {
    /// The height it stands at.
    pub rest_y: f32,
    /// How deep it sinks to be gone (metres).
    pub depth: f32,
}

/// A stump sinking, since `Time::elapsed_secs()` was `since`.
#[derive(Component, Clone, Copy, Debug)]
pub struct Sinking {
    pub since: f32,
}

/// The felled trees falling and the stumps standing, oldest first (a stump
/// with when it was planted, in `Time::elapsed_secs()`).
#[derive(Resource, Default, Debug)]
pub struct Remnants {
    falling: VecDeque<Entity>,
    standing: VecDeque<(Entity, f32)>,
}

impl Remnants {
    /// Trees falling (or lying, or sinking) now.
    pub fn falling(&self) -> usize {
        self.falling.len()
    }

    /// Stumps standing (not yet sinking).
    pub fn stumps(&self) -> usize {
        self.standing.len()
    }

    /// Count `tree` as falling; past [`MAX_FALLING`], returns the oldest,
    /// to be removed.
    pub fn fell(&mut self, tree: Entity) -> Option<Entity> {
        self.falling.push_back(tree);
        (self.falling.len() > MAX_FALLING).then(|| self.falling.pop_front()).flatten()
    }

    /// Count `stump`, planted at `at` (`Time::elapsed_secs()`).
    pub fn plant(&mut self, stump: Entity, at: f32) {
        self.standing.push_back((stump, at));
    }

    pub fn reset(&mut self) {
        self.falling.clear();
        self.standing.clear();
    }
}

/// When a tree drawn with its model dies (its wood ran out) where the
/// local team can see it, its model is detached as a [`FallingTree`],
/// falling away from the nearest worker gathering it (else one standing
/// by, else a hashed direction). This runs inside the sim tick, so it only
/// moves an existing client entity and spawns nothing (see
/// `af_core::sim_ids`); the stump is planted by [`drive_falls`].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn fell_tree(
    dead: On<Add, Dead>,
    mut commands: Commands,
    library: Option<Res<StaticLibrary>>,
    viewer: Viewer,
    nodes: Query<(&StaticVisual, &BoxVisual, &Transform, &Position, Option<&SeenBy>), With<ResourceNode>>,
    parts: Query<&Transform, (With<BoxPart>, Without<BoxVisual>)>,
    workers: Query<(&Position, &Orders), (With<Gatherer>, Without<Dead>)>,
    mut remnants: ResMut<Remnants>,
    clock: Option<Res<SimClock>>,
) {
    let Ok((sv, visual, body, pos, seen)) = nodes.get(dead.entity) else { return };
    let Some(library) = library else { return };
    let model = library.model(sv.model);
    if !model.manifest.falls() || !node_in_sight(viewer.team(), seen) {
        return;
    }
    let (Ok(root), Ok(instance), Some(tree)) = (parts.get(visual.root), parts.get(sv.instance), model.variant(sv.variant)) else { return };
    let frame = body.mul_transform(*root);
    let rest = frame.mul_transform(*instance);
    let gathering = workers.iter().filter(|(_, o)| matches!(o.current(), Some(Order::Gather { target }) if *target == dead.entity));
    let feller = nearest(pos.0, gathering.map(|(p, _)| p.0)).or_else(|| nearest(pos.0, workers.iter().map(|(p, _)| p.0).filter(|p| p.distance(pos.0) <= FELLER_REACH)));
    let size = tree.size() * rest.scale;
    let stump = model.stump().map(|s| (sv.model, frame.mul_transform(stump_transform(&sv.rest, tree, s))));
    let fall = FallingTree { age: 0.0, rest, pivot: body.translation, dir: fall_direction(pos.0, feller), depth: size.x.max(size.z) + 0.3, stump };
    let at = clock.map_or(0.0, |c| c.elapsed);
    debug!("{}: a tree felled at {} ({at:.1} s), falling toward {}", model.source, pos.0, fall.dir);
    if let Some(oldest) = remnants.fell(sv.instance) {
        commands.entity(oldest).try_despawn();
    }
    commands.entity(sv.instance).try_remove::<ChildOf>().try_insert((rest, Visibility::Inherited, fall, RenderScene));
}

/// The point of `points` nearest `to`.
fn nearest(to: Vec2, points: impl Iterator<Item = Vec2>) -> Option<Vec2> {
    points.min_by(|a, b| a.distance_squared(to).total_cmp(&b.distance_squared(to)))
}

/// Felled trees fall, lie and sink; each plants its stump as it starts.
pub(crate) fn drive_falls(
    mut commands: Commands,
    time: Res<Time>,
    library: Res<StaticLibrary>,
    mut painter: Painter,
    mut tints: ResMut<TeamTints>,
    mut remnants: ResMut<Remnants>,
    mut falling: Query<(Entity, &mut FallingTree, &mut Transform)>,
) {
    for (e, mut fall, mut tf) in &mut falling {
        if let Some((id, at)) = fall.stump.take()
            && let Some(stump) = library.model(id).stump()
        {
            let depth = stump.size().y * at.scale.y + 0.05;
            let planted = spawn_instance(&mut commands, &mut painter, &mut tints, stump, at, None, (Stump { rest_y: at.translation.y, depth }, RenderScene));
            remnants.plant(planted, time.elapsed_secs());
        }
        fall.age += time.delta_secs();
        match fall_pose(fall.age) {
            Some(pose) => *tf = tipped(fall.rest, fall.pivot, fall.dir, pose.tilt, pose.sink * fall.depth),
            None => {
                remnants.falling.retain(|t| *t != e);
                commands.entity(e).try_despawn();
            }
        }
    }
}

/// Stumps stand [`STUMP_HOLD`] seconds (fewer when more than
/// [`MAX_STUMPS`] stand), then sink and go.
pub(crate) fn sink_stumps(mut commands: Commands, time: Res<Time>, mut remnants: ResMut<Remnants>, mut sinking: Query<(Entity, &Stump, &Sinking, &mut Transform)>) {
    let now = time.elapsed_secs();
    while let Some(&(e, planted)) = remnants.standing.front() {
        if now - planted < STUMP_HOLD && remnants.standing.len() <= MAX_STUMPS {
            break;
        }
        remnants.standing.pop_front();
        commands.entity(e).try_insert(Sinking { since: now });
    }
    for (e, stump, sinking, mut tf) in &mut sinking {
        match sink_drop(now - sinking.since, STUMP_SINK, stump.depth) {
            Some(drop) => tf.translation.y = stump.rest_y - drop,
            None => commands.entity(e).try_despawn(),
        }
    }
}
