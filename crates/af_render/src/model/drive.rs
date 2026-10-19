//! Per frame: feed each modelled unit's [`Animator`] from the sim, apply
//! what it asks of the animation player, show the right props, turn the
//! model toward its work; play out corpses.

use std::time::Duration;

use af_core::ext::behavior::gather_style;
use af_core::prelude::*;
use bevy::animation::RepeatAnimation;
use bevy::ecs::query::QueryData;
use bevy::prelude::*;

use super::animator::{Command, FADE, Inputs, Shot, Swing, base_clip, corpse_drop};
use super::clips::Clip;
use super::stance::{self, Lean};
use super::library::{LoadedModel, ModelLibrary};
use super::props::{heading_tool, shown_props};
use super::rig::{Corpse, ModelRoot, ModelUnit, Rig, TRACKED_SLOTS};
use crate::parts::{node_base_half, node_scale};

#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct Driven {
    model: &'static mut ModelUnit,
    owner: &'static Owner,
    position: &'static Position,
    facing: &'static Facing,
    activity: &'static Activity,
    velocity: &'static Velocity,
    health: &'static Health,
    footprint: &'static Footprint,
    weapons: &'static Weapons,
    orders: Ref<'static, Orders>,
    gatherer: Option<&'static Gatherer>,
    idle: Has<Idle>,
}

type Players<'w, 's> = Query<'w, 's, (&'static mut AnimationPlayer, &'static mut AnimationTransitions)>;

/// Prop nodes (never sim entities or model roots).
type PropVisibility<'w, 's> = Query<'w, 's, &'static mut Visibility, (Without<Unit>, Without<ModelRoot>)>;

/// Bones of model instances (never sim entities or model roots).
type Bones<'w, 's> = Query<'w, 's, &'static mut Transform, (Without<Unit>, Without<ModelRoot>)>;

/// Model roots (under their unit's visual root).
type Roots<'w, 's> = Query<'w, 's, &'static mut Transform, (With<ModelRoot>, Without<Unit>)>;

/// What a worker can be working on: a resource node, or a building (a
/// foundation, a damaged wall, a field).
type WorkTarget<'a> = (&'a Position, &'a Facing, &'a Footprint, Option<&'a ResourceNode>);
type WorkTargets<'w, 's> = Query<'w, 's, WorkTarget<'static>, Without<Dead>>;

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_models(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    data: Res<Data>,
    library: Res<ModelLibrary>,
    local: Res<LocalPlayer>,
    mut trained: MessageReader<UnitSpawned>,
    mut ages: MessageReader<AgeAdvanced>,
    mut units: Query<Driven, Without<Dead>>,
    rigs: Query<&Rig>,
    mut players: Players,
    mut props: PropVisibility,
    mut bones: Bones,
    mut roots: Roots,
    sources: Query<(Option<&ResourceNode>, Has<ResourceField>), Without<Dead>>,
    targets: WorkTargets,
) {
    for msg in trained.read() {
        if let Ok(mut u) = units.get_mut(msg.entity) {
            u.model.spawned = true;
        }
    }
    let cheering = ages.read().filter(|m| Some(m.player) == local.0).last().map(|m| m.player);
    let dt = time.delta_secs();
    // Sim time since the last tick, which the weapon state is from.
    let since_tick = fixed.overstep_fraction() * SIM_DT;
    let style_of = |e: Entity| sources.get(e).ok().and_then(|(node, field)| gather_style(&data, node, field));

    for mut u in &mut units {
        if cheering == Some(u.owner.0) && u.idle {
            u.model.cheer = true;
        }
        let swing = notice_swing(&data, &mut u.model.windups, u.weapons, since_tick);
        let Ok(rig) = rigs.get(u.model.root) else { continue };
        let model = library.model(u.model.model);
        let Some(loaded) = model.loaded() else { continue };
        let scale = model.scale_for(u.footprint.height);
        // Turned toward the work and stepped up to it (see `stance`).
        let want = work_target(*u.activity, u.orders.current())
            .and_then(|t| targets.get(t).ok())
            .map_or(Lean::default(), |target| work_lean(&data, loaded, *u.activity, (u.position.0, u.facing.0), scale, target));
        let lean = u.model.lean.step(want, dt);
        if lean != u.model.lean
            && let Ok(mut tf) = roots.get_mut(u.model.root)
        {
            u.model.lean = lean;
            let o = lean.offset();
            tf.translation.x = o.x;
            tf.translation.z = o.y;
            tf.rotation = Quat::from_rotation_y(lean.yaw);
        }
        if u.orders.is_changed() || u.model.shown.is_none() {
            u.model.heading = heading_tool(u.orders.current(), style_of);
        }
        let carrying = u.gatherer.and_then(|g| g.carrying.filter(|_| g.amount > 0.0));
        let inputs = Inputs {
            activity: *u.activity,
            speed: u.velocity.0.length(),
            carrying,
            hp: u.health.hp,
            scale,
            swing,
            spawned: std::mem::take(&mut u.model.spawned),
            cheer: std::mem::take(&mut u.model.cheer),
            dt,
        };
        let command = u.model.anim.step(&inputs, &loaded.clips);
        if let Some(Ok((mut player, mut transitions))) = rig.player.map(|p| players.get_mut(p)) {
            apply(command, loaded, &mut player, &mut transitions);
        }
        if command == Command::Rest {
            for (bone, pose) in &rig.bind_pose {
                if let Ok(mut t) = bones.get_mut(*bone) {
                    *t = *pose;
                }
            }
        }
        let swinging = u.model.anim.shot() == Some(Shot::Attack);
        let shown = shown_props(u.model.anim.base(), swinging, u.model.anim.shown_load(carrying), u.model.heading);
        if u.model.shown != Some(shown) {
            for (prop, e) in &rig.props {
                if let Ok(mut vis) = props.get_mut(*e) {
                    vis.set_if_neq(if shown.shows(*prop) { Visibility::Inherited } else { Visibility::Hidden });
                }
            }
            u.model.shown = Some(shown);
        }
    }
}

/// How a unit at `at` (position, facing) doing `activity` on `target`
/// stands to it: facing it, and stepped up by what the work clip reaches
/// (`scale`: the model's display scale).
fn work_lean(data: &GameData, model: &LoadedModel, activity: Activity, at: (Vec2, f32), scale: f32, target: WorkTarget) -> Lean {
    let (pos, facing, footprint, node) = target;
    let footprint = footprint.obb(pos.0, facing.0);
    let surface = match node {
        Some(n) => {
            let def = data.node(n.kind);
            af_geom::Obb::new(pos.0, node_base_half(data, def) * node_scale(n.remaining, def.amount), facing.0)
        }
        None => footprint,
    };
    let clip = model.clips.resolve(base_clip(activity, false, false));
    let reach = clip.and_then(|c| model.clips.info(c)).and_then(|i| i.reach).map(|r| r * scale);
    Lean::toward(at.0, at.1, &stance::WorkTarget { footprint, surface }, reach)
}

/// The entity a unit busy at work works on (its front order's target).
fn work_target(activity: Activity, order: Option<&Order>) -> Option<Entity> {
    match (activity, order?) {
        (Activity::Gather(_), Order::Gather { target }) | (Activity::Repair, Order::Repair { target }) => Some(*target),
        (Activity::Build, Order::Build { site }) => Some(*site),
        _ => None,
    }
}

/// A windup that started since the last frame on any tracked slot: how far
/// into it the sim is now (its windup is from the last tick, `since_tick`
/// seconds ago) and how long it lasts.
fn notice_swing(data: &GameData, last: &mut [f32; TRACKED_SLOTS], weapons: &Weapons, since_tick: f32) -> Option<Swing> {
    let mut swing = None;
    for (i, slot) in weapons.slots.iter().take(TRACKED_SLOTS).enumerate() {
        let started = slot.windup > 0.0 && (last[i] <= 0.0 || slot.windup > last[i] + 1e-4);
        if started {
            let windup = data.weapon(slot.weapon).windup;
            swing = Some(Swing { elapsed: (windup - slot.windup).max(0.0) + since_tick, windup });
        }
        last[i] = slot.windup;
    }
    swing
}

/// Do what the animator asked.
fn apply(command: Command, model: &LoadedModel, player: &mut AnimationPlayer, transitions: &mut AnimationTransitions) {
    match command {
        Command::Keep => {}
        Command::Play { clip, once, rate, seek, fade } => {
            let Some(node) = model.node(clip) else { return };
            let active = transitions.play(player, node, Duration::from_secs_f32(fade));
            active.set_speed(rate).set_repeat(if once { RepeatAnimation::Never } else { RepeatAnimation::Forever });
            if seek > 0.0 {
                active.seek_to(seek);
            }
        }
        Command::Rate(rate) => {
            if let Some(active) = transitions.get_main_animation().and_then(|n| player.animation_mut(n)) {
                active.set_speed(rate);
            }
        }
        Command::Rest => {
            player.stop_all();
            *transitions = AnimationTransitions::new();
        }
    }
}

/// How high a body lying on the ground stands, as a fraction of its height.
const LYING_HEIGHT: f32 = 0.4;

/// Corpses play Death once and hold its last pose, lie still, then sink
/// into the ground and are removed.
pub(crate) fn drive_corpses(
    mut commands: Commands,
    time: Res<Time>,
    library: Res<ModelLibrary>,
    mut corpses: Query<(Entity, &mut Corpse, &ModelRoot, &mut Transform, Option<&Rig>)>,
    mut players: Players,
) {
    let dt = time.delta_secs();
    for (e, mut corpse, root, mut tf, rig) in &mut corpses {
        let model = library.model(root.model);
        if !corpse.started
            && let Some(rig) = rig
        {
            corpse.started = true;
            if let (Some(loaded), Some(Ok((mut player, mut transitions)))) = (model.loaded(), rig.player.map(|p| players.get_mut(p))) {
                match loaded.node(Clip::Death) {
                    Some(node) => {
                        transitions.play(&mut player, node, Duration::from_secs_f32(FADE)).set_repeat(RepeatAnimation::Never);
                    }
                    // No death clip: freeze where it fell (and stop any crossfade).
                    None => {
                        player.pause_all();
                        *transitions = AnimationTransitions::new();
                    }
                }
            }
        }
        corpse.age += dt;
        // A body lying down (Death) needs to sink less far than one left standing.
        let lying = model.loaded().is_some_and(|m| m.node(Clip::Death).is_some());
        let depth = model.manifest.height * tf.scale.y * if lying { LYING_HEIGHT } else { 1.0 };
        match corpse_drop(corpse.age, depth) {
            Some(drop) => tf.translation.y = corpse.rest_y - drop,
            None => commands.entity(e).try_despawn(),
        }
    }
}
