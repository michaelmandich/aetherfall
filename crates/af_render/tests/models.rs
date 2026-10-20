//! Modelled units driven headless: the real plugin and content, a model
//! library built by hand (no glTF loader, no GPU), and a stand-in for the
//! scene instance the loader would spawn.

use af_core::prelude::*;
use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};
use af_render::BoxRenderPlugin;
use af_render::entities::{BoxVisual, VisualSig};
use af_render::model::animator::{CORPSE_HOLD, CORPSE_SINK};
use af_render::model::stance::{CLEARANCE, STEP_RATE};
use af_render::overlay::{BarParts, OutlineParts};
use af_render::parts::node_base_half;
use af_render::model::{Clip, ClipTable, Corpse, LoadedModel, ModelLibrary, ModelManifest, ModelRoot, ModelState, ModelUnit, Rig, SceneReady};
use bevy::animation::AnimationTargetId;
use bevy::prelude::*;

const MANIFEST: &str = r#"(
    model: "models/test/peasant.glb",
    height: 1.65,
    clips: {
        "Idle": (duration: 3.0, looping: true),
        "Walk": (duration: 1.08, looping: true, speed: 1.4),
        "Chop": (duration: 1.6, looping: true, reach: 1.3),
        "Attack": (duration: 1.0, looping: false, hit: 0.5),
    },
)"#;

struct Test {
    app: App,
    peasant: Entity,
    spearman: Entity,
    loaded: LoadedModel,
}

/// The ready model: an empty scene, a graph with the manifest's clips, and
/// a white team material.
fn loaded_model(app: &mut App, manifest: &ModelManifest) -> LoadedModel {
    let world = app.world_mut();
    let scene = world.resource_mut::<Assets<WorldAsset>>().add(WorldAsset::new(World::new()));
    let team = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::from(Color::WHITE));
    let mut graph = AnimationGraph::new();
    let mut nodes = [None; Clip::COUNT];
    for clip in Clip::ALL {
        if let Some(spec) = manifest.clip(clip) {
            let mut c = AnimationClip::default();
            c.set_duration(spec.duration);
            let handle = world.resource_mut::<Assets<AnimationClip>>().add(c);
            nodes[clip.index()] = Some(graph.add_clip(handle, 1.0, graph.root));
        }
    }
    let graph = world.resource_mut::<Assets<AnimationGraph>>().add(graph);
    let clips = ClipTable::from_manifest(manifest, |c| nodes[c.index()].is_some());
    LoadedModel { scene, graph, nodes, clips, team_material: Some(team) }
}

fn setup(ready: bool) -> Test {
    setup_with(MANIFEST, ready)
}

fn setup_with(manifest: &str, ready: bool) -> Test {
    let data = test_data();
    let mut app = sim_app(data.clone());
    app.add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<WorldAsset>()
        .init_asset::<AnimationClip>()
        .init_asset::<AnimationGraph>();
    let manifest = ModelManifest::parse(manifest).unwrap_or_else(|e| panic!("{e}"));
    let loaded = loaded_model(&mut app, &manifest);
    let state = if ready { ModelState::Ready(Box::new(loaded.clone())) } else { ModelState::Loading(Handle::default()) };
    let mut library = ModelLibrary::default();
    library.insert(data.unit_id("peasant").unwrap(), "models/test/peasant.model.ron", manifest, state);
    app.insert_resource(library);
    app.add_plugins(BoxRenderPlugin);
    let ids = flat_world(&mut app, 64.0, &[("ironhold", 0), ("dune", 1)]);
    app.world_mut().insert_resource(LocalPlayer(Some(ids[0])));
    let (peasant, spearman) = with_commands(&mut app, |c, d, p| {
        (
            spawn_unit(c, d, p.get(ids[0]), d.unit_id("peasant").unwrap(), Vec2::new(10.0, 10.0), 0.0),
            spawn_unit(c, d, p.get(ids[0]), d.unit_id("spearman").unwrap(), Vec2::new(14.0, 10.0), 0.0),
        )
    });
    run_ticks(&mut app, 2);
    Test { app, peasant, spearman, loaded }
}

fn descendants(app: &mut App, root: Entity) -> Vec<Entity> {
    let mut q = app.world_mut().query::<&Children>();
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if let Ok(children) = q.get(app.world(), e) {
            stack.extend(children.iter());
            out.extend(children.iter());
        }
    }
    out
}

fn boxes_under(app: &mut App, root: Entity) -> usize {
    descendants(app, root).into_iter().filter(|e| app.world().get::<Mesh3d>(*e).is_some()).count()
}

fn model_root(t: &Test) -> Entity {
    t.app.world().get::<ModelUnit>(t.peasant).expect("modelled").root
}

/// What the glTF loader would spawn under the model root: an armature with
/// the animation player, the props, and the team sash.
struct Instance {
    armature: Entity,
    chop: Entity,
    chip: [Entity; 2],
    attack: Entity,
    wood: Entity,
    sash: Entity,
}

fn spawn_instance(t: &mut Test) -> Instance {
    let root = model_root(t);
    let team = t.loaded.team_material.clone().unwrap();
    let world = t.app.world_mut();
    let armature = world.spawn((Name::new("Armature"), AnimationPlayer::default(), Transform::default(), Visibility::default(), ChildOf(root))).id();
    let mut node = |name: &str| world.spawn((Name::new(name.to_string()), Transform::default(), Visibility::default(), ChildOf(armature))).id();
    let chop = node("Tool_chop");
    let chip = [node("Tool_chip"), node("Tool_chip_l")];
    let attack = node("Tool_attack");
    let wood = node("Load_wood");
    let sash = node("Team_sash");
    world.spawn((Name::new("Team_sash.0"), MeshMaterial3d(team), Transform::default(), Visibility::default(), ChildOf(sash)));
    world.entity_mut(root).insert(SceneReady);
    Instance { armature, chop, chip, attack, wood, sash }
}

fn visible(app: &App, e: Entity) -> bool {
    *app.world().get::<Visibility>(e).unwrap() != Visibility::Hidden
}

fn main_clip(t: &Test, armature: Entity) -> Option<Clip> {
    let main = t.app.world().get::<AnimationTransitions>(armature)?.get_main_animation()?;
    Clip::ALL.into_iter().find(|c| t.loaded.node(*c) == Some(main))
}

#[test]
fn a_modelled_unit_gets_its_scene_instead_of_body_boxes() {
    let mut t = setup(true);
    let peasant_kind = t.app.world().get::<Unit>(t.peasant).unwrap().kind;
    let visual = t.app.world().get::<BoxVisual>(t.peasant).expect("visual");
    assert_eq!(visual.sig, VisualSig::Unit { kind: peasant_kind, shield: false, model: true });
    assert!(visual.weapons.is_empty() && visual.cargo.is_none(), "the model holds its own tools and loads");
    let root = visual.root;
    assert_eq!(boxes_under(&mut t.app, root), 0, "no body boxes");
    let model = model_root(&t);
    let scene = t.app.world().get::<WorldAssetRoot>(model).expect("a scene root");
    assert_eq!(scene.0, t.loaded.scene);
    assert_eq!(t.app.world().get::<ChildOf>(model).map(|c| c.parent()), Some(root));
    let scale = t.app.world().get::<Transform>(model).unwrap().scale;
    assert!((scale - Vec3::splat(1.2 / 1.65)).length() < 1e-5, "scaled to the 1.2 m box: {scale}");
    assert!(!visible(&t.app, model), "hidden until rigged");
    assert_eq!(t.app.world().get::<ModelRoot>(model).unwrap().unit, t.peasant);
    // Units without a model keep their boxes.
    let spear = t.app.world().get::<BoxVisual>(t.spearman).unwrap();
    assert!(matches!(spear.sig, VisualSig::Unit { model: false, .. }));
    let spear_root = spear.root;
    assert!(boxes_under(&mut t.app, spear_root) >= 3);
    assert!(t.app.world().get::<ModelUnit>(t.spearman).is_none());
}

#[test]
fn units_are_boxes_until_their_model_loads() {
    let mut t = setup(false);
    let visual = t.app.world().get::<BoxVisual>(t.peasant).unwrap();
    assert!(matches!(visual.sig, VisualSig::Unit { model: false, .. }));
    let boxes_root = visual.root;
    assert!(boxes_under(&mut t.app, boxes_root) >= 3, "boxes meanwhile");
    // The model arrives.
    let peasant = t.app.world().resource::<Data>().unit_id("peasant").unwrap();
    let mut library = ModelLibrary::default();
    let manifest = ModelManifest::parse(MANIFEST).unwrap();
    library.insert(peasant, "models/test/peasant.model.ron", manifest, ModelState::Ready(Box::new(t.loaded.clone())));
    t.app.insert_resource(library);
    run_ticks(&mut t.app, 1);
    let visual = t.app.world().get::<BoxVisual>(t.peasant).unwrap();
    assert!(matches!(visual.sig, VisualSig::Unit { model: true, .. }), "rebuilt with the model");
    assert!(t.app.world().get_entity(boxes_root).is_err(), "the boxes are gone");
    assert!(t.app.world().get::<ModelUnit>(t.peasant).is_some());
}

#[test]
fn the_rig_plays_clips_for_the_activity_and_shows_the_matching_props() {
    let mut t = setup(true);
    let i = spawn_instance(&mut t);
    run_ticks(&mut t.app, 2);
    let model = model_root(&t);
    let rig = t.app.world().get::<Rig>(model).expect("rigged").clone();
    assert_eq!(rig.player, Some(i.armature));
    assert_eq!(rig.props.len(), 5, "tools and the load, not the sash");
    assert!(visible(&t.app, model), "shown once rigged");
    assert!(t.app.world().get::<AnimationGraphHandle>(i.armature).is_some_and(|g| g.0 == t.loaded.graph));
    for e in [i.chop, i.chip[0], i.chip[1], i.attack, i.wood] {
        assert!(!visible(&t.app, e), "props start hidden");
    }
    assert!(visible(&t.app, i.sash));
    // The sash is tinted with the owner's colour (a copy of the team material).
    let sash_mesh = t.app.world().get::<Children>(i.sash).unwrap()[0];
    let tinted = t.app.world().get::<MeshMaterial3d<StandardMaterial>>(sash_mesh).unwrap().0.clone();
    assert_ne!(Some(&tinted), t.loaded.team_material.as_ref());
    let owner = t.app.world().resource::<Players>().list[0].color;
    let base = t.app.world().resource::<Assets<StandardMaterial>>().get(&tinted).unwrap().base_color.to_srgba();
    assert!((base.red - owner[0]).abs() < 0.05 && (base.green - owner[1]).abs() < 0.05 && (base.blue - owner[2]).abs() < 0.05, "{base:?} vs {owner:?}");
    assert_eq!(main_clip(&t, i.armature), Some(Clip::Idle));

    // Chopping: the chop clip, the axe in hand.
    t.app.world_mut().entity_mut(t.peasant).insert(Activity::Gather(WorkStyle::Chop));
    run_ticks(&mut t.app, 1);
    assert_eq!(main_clip(&t, i.armature), Some(Clip::Chop));
    assert!(visible(&t.app, i.chop) && !visible(&t.app, i.attack) && !visible(&t.app, i.wood));

    // Carrying the wood home: the load, no tool; no CarryWalk in this model, so Walk.
    let wood = test_data().resource_id("wood").unwrap();
    let mut g = *t.app.world().get::<Gatherer>(t.peasant).unwrap();
    g.carrying = Some(wood);
    g.amount = 10.0;
    t.app.world_mut().entity_mut(t.peasant).insert((g, Activity::Idle, Velocity(Vec2::new(0.0, 2.4))));
    run_ticks(&mut t.app, 1);
    assert_eq!(main_clip(&t, i.armature), Some(Clip::Walk));
    assert!(visible(&t.app, i.wood) && !visible(&t.app, i.chop));

    // A swing: the attack clip, the weapon in hand.
    g.carrying = None;
    g.amount = 0.0;
    let mut weapons = t.app.world().get::<Weapons>(t.peasant).unwrap().clone();
    weapons.slots[0].windup = 0.5;
    t.app.world_mut().entity_mut(t.peasant).insert((g, Activity::Fight, Velocity(Vec2::ZERO), weapons));
    run_ticks(&mut t.app, 1);
    assert_eq!(main_clip(&t, i.armature), Some(Clip::Attack));
    assert!(visible(&t.app, i.attack) && !visible(&t.app, i.wood));
}

/// At a tree, with wood piling up: the axe, not the load, and the model
/// turned to the trunk and stepped up until the axe reaches it; the
/// selection outline and the bars go with it.
#[test]
fn a_woodcutter_faces_the_trunk_and_steps_up_to_it() {
    let mut t = setup(true);
    let i = spawn_instance(&mut t);
    // A pine 1.6 m east of the peasant (which faces +Z): a 0.7 m gap.
    let tree = with_commands(&mut t.app, |c, d, _| spawn_node(c, d, d.node_id("pine").unwrap(), Vec2::new(11.6, 10.0), 0.0, None));
    let wood = test_data().resource_id("wood").unwrap();
    let mut g = *t.app.world().get::<Gatherer>(t.peasant).unwrap();
    g.carrying = Some(wood);
    g.amount = 4.0;
    let mut orders = t.app.world().get::<Orders>(t.peasant).unwrap().clone();
    orders.set(Order::Gather { target: tree }, false);
    t.app.world_mut().entity_mut(t.peasant).insert((g, orders, Activity::Gather(WorkStyle::Chop), Selected));
    run_ticks(&mut t.app, 2);
    assert!(visible(&t.app, i.chop) && !visible(&t.app, i.wood), "the axe; the wood piling up stays out of sight");

    // Turned east (yaw +90 degrees) at once is too fast: it turns and steps over a few frames.
    run_ticks(&mut t.app, 30);
    let model = model_root(&t);
    let tf = *t.app.world().get::<Transform>(model).unwrap();
    let facing = tf.rotation * Vec3::Z;
    assert!((facing - Vec3::X).length() < 1e-3, "faces the trunk: {facing}");
    // The trunk's face is 1.6 m - half the pine's trunk away; the axe reaches 1.3 model metres.
    let data = test_data();
    let trunk = node_base_half(&data, data.node(data.node_id("pine").unwrap())).x;
    let want = (1.6 - trunk - 1.3 * 1.2 / 1.65).min(1.6 - 0.45 - CLEARANCE);
    assert!(want > 0.2 && (tf.translation - Vec3::X * want).length() < 1e-3, "stepped up {:.3} m, want {want:.3}", tf.translation.x);
    let at = |t: &Test, e: Entity| t.app.world().get::<Transform>(e).unwrap().translation;
    let outline = t.app.world().get::<OutlineParts>(t.peasant).expect("selected: outlined").root;
    let bars = t.app.world().get::<BarParts>(t.peasant).expect("selected: bars").root;
    assert!((at(&t, outline).xz() - tf.translation.xz()).length() < 1e-3, "the outline is under the model");
    assert!((at(&t, bars).xz() - tf.translation.xz()).length() < 1e-3, "the bars are over it");

    // Done: back upright on its own spot.
    t.app.world_mut().entity_mut(t.peasant).insert(Activity::Idle);
    run_ticks(&mut t.app, ((want / STEP_RATE) * SIM_HZ as f32) as u32 + 30);
    let tf = *t.app.world().get::<Transform>(model).unwrap();
    assert!(tf.translation.xz().length() < 1e-4 && (tf.rotation * Vec3::Z - Vec3::Z).length() < 1e-3, "{tf:?}");
}

#[test]
fn a_dead_unit_leaves_its_model_behind_for_a_while() {
    let mut t = setup(true);
    spawn_instance(&mut t);
    run_ticks(&mut t.app, 2);
    let model = model_root(&t);
    t.app.world_mut().entity_mut(t.peasant).insert(Dead);
    run_ticks(&mut t.app, 1);
    assert!(t.app.world().get_entity(t.peasant).is_err(), "the unit is gone");
    assert!(t.app.world().get::<Corpse>(model).is_some(), "its model stays as a corpse");
    assert!(t.app.world().get::<ChildOf>(model).is_none());
    let rest = t.app.world().get::<Transform>(model).unwrap().translation.y;
    run_ticks(&mut t.app, ((CORPSE_HOLD + CORPSE_SINK * 0.5) * SIM_HZ as f32) as u32);
    let sunk = t.app.world().get::<Transform>(model).expect("still sinking").translation.y;
    assert!(sunk < rest - 0.1, "sinking: {rest} -> {sunk}");
    run_ticks(&mut t.app, (CORPSE_SINK * SIM_HZ as f32) as u32);
    assert!(t.app.world().get_entity(model).is_err(), "gone once sunk");
}

/// A rigid model (a ram, a cart: no skeleton, no clips; `art/units/lib/rigid.py`)
/// is drawn as it is: no animation player and nothing to play, its team
/// marker tinted. The unit moves, fights, is hurt and dies without trouble,
/// and its corpse sinks its full height (nothing lies down).
#[test]
fn a_model_without_bones_or_clips_is_drawn_as_it_is() {
    let mut t = setup_with(r#"(model: "models/ram/ram.glb", height: 1.1, clips: {})"#, true);
    let root = model_root(&t);
    let team = t.loaded.team_material.clone().unwrap();
    let world = t.app.world_mut();
    let body = world.spawn((Name::new("battering_ram"), Transform::default(), Visibility::default(), ChildOf(root))).id();
    let pennant = world.spawn((Name::new("Team_pennant"), Transform::default(), Visibility::default(), ChildOf(root))).id();
    let flag = world.spawn((Name::new("Team_pennant.0"), MeshMaterial3d(team.clone()), Transform::default(), Visibility::default(), ChildOf(pennant))).id();
    world.entity_mut(root).insert(SceneReady);
    run_ticks(&mut t.app, 2);
    let rig = t.app.world().get::<Rig>(root).expect("rigged").clone();
    assert_eq!(rig.player, None, "no bones, no animation player");
    assert!(rig.props.is_empty() && rig.bind_pose.is_empty());
    assert!(visible(&t.app, root) && visible(&t.app, body) && visible(&t.app, pennant), "shown once rigged");
    let tinted = t.app.world().get::<MeshMaterial3d<StandardMaterial>>(flag).unwrap().0.clone();
    assert_ne!(tinted, team, "the pennant takes the owner's colour");

    let animator = |t: &Test| t.app.world().get::<ModelUnit>(t.peasant).unwrap().animator().clone();
    t.app.world_mut().entity_mut(t.peasant).insert(Velocity(Vec2::new(2.0, 0.0)));
    run_ticks(&mut t.app, 2);
    assert_eq!(animator(&t).main(), None, "nothing to walk with");
    let mut weapons = t.app.world().get::<Weapons>(t.peasant).unwrap().clone();
    weapons.slots[0].windup = 0.5;
    let mut health = *t.app.world().get::<Health>(t.peasant).unwrap();
    health.hp -= 5.0;
    t.app.world_mut().entity_mut(t.peasant).insert((Activity::Fight, Velocity(Vec2::ZERO), weapons, health));
    run_ticks(&mut t.app, 2);
    let a = animator(&t);
    assert_eq!((a.main(), a.shot()), (None, None), "no attack or hit clip: skipped");
    assert!(visible(&t.app, body));

    t.app.world_mut().entity_mut(t.peasant).insert(Dead);
    run_ticks(&mut t.app, 1);
    let corpse = t.app.world().get::<Corpse>(root).copied().expect("its model stays as a corpse");
    run_ticks(&mut t.app, ((CORPSE_HOLD + CORPSE_SINK * 0.95) * SIM_HZ as f32) as u32);
    let sunk = t.app.world().get::<Transform>(root).expect("still sinking").translation.y;
    assert!(sunk < corpse.rest_y - 1.0, "left standing, it sinks nearly its whole height: {} -> {sunk}", corpse.rest_y);
    run_ticks(&mut t.app, (CORPSE_SINK * SIM_HZ as f32) as u32);
    assert!(t.app.world().get_entity(root).is_err(), "gone once sunk");
}

/// `Gear_*` nodes (a soldier's spear, a shield) are the model's own: never
/// hidden, whatever the unit does; only `Tool_*` and `Load_*` come and go.
#[test]
fn gear_is_always_shown() {
    let mut t = setup(true);
    let i = spawn_instance(&mut t);
    let spear = t.app.world_mut().spawn((Name::new("Gear_spear"), Transform::default(), Visibility::default(), ChildOf(i.armature))).id();
    run_ticks(&mut t.app, 2);
    assert_eq!(t.app.world().get::<Rig>(model_root(&t)).unwrap().props.len(), 5, "gear is not a prop the engine toggles");
    assert!(visible(&t.app, spear), "idle");
    t.app.world_mut().entity_mut(t.peasant).insert(Activity::Gather(WorkStyle::Chop));
    run_ticks(&mut t.app, 1);
    assert!(visible(&t.app, spear) && visible(&t.app, i.chop), "at work");
    let mut weapons = t.app.world().get::<Weapons>(t.peasant).unwrap().clone();
    weapons.slots[0].windup = 0.5;
    t.app.world_mut().entity_mut(t.peasant).insert((Activity::Fight, weapons));
    run_ticks(&mut t.app, 1);
    assert!(visible(&t.app, spear) && visible(&t.app, i.attack), "mid-swing");
}

/// A model that ships nothing but a walk (the raw Meshy export) walks, and
/// stands in its bind pose otherwise.
#[test]
fn without_an_idle_clip_a_standing_unit_takes_the_bind_pose() {
    let mut t = setup_with(r#"(model: "m.glb", height: 1.65, clips: { "Walk": (animation: "Walking", duration: 1.08, looping: true, speed: 1.2) })"#, true);
    let i = spawn_instance(&mut t);
    let rest = Transform::from_xyz(0.0, 0.9, 0.0);
    let hips = t.app.world_mut().spawn((Name::new("mixamorig:Hips"), AnimationTargetId::from_name(&Name::new("mixamorig:Hips")), rest, ChildOf(i.armature))).id();
    run_ticks(&mut t.app, 2);
    assert_eq!(t.app.world().get::<Rig>(model_root(&t)).unwrap().bind_pose, vec![(hips, rest)]);
    assert_eq!(main_clip(&t, i.armature), None, "nothing to play standing");
    t.app.world_mut().entity_mut(t.peasant).insert(Velocity(Vec2::new(2.4, 0.0)));
    run_ticks(&mut t.app, 1);
    assert_eq!(main_clip(&t, i.armature), Some(Clip::Walk));
    // The walk moved the hips (as Bevy's animation would); stopping puts them back.
    t.app.world_mut().entity_mut(hips).insert(Transform::from_xyz(0.1, 0.8, 0.2));
    t.app.world_mut().entity_mut(t.peasant).insert(Velocity(Vec2::ZERO));
    run_ticks(&mut t.app, 1);
    assert_eq!(main_clip(&t, i.armature), None);
    assert_eq!(*t.app.world().get::<Transform>(hips).unwrap(), rest);
}
