//! Unit routes: where the local player's selected units are walking, the
//! orders they have queued, and the loop a selected patrol walks.
//!
//! Two systems in `RenderStep::Overlay`:
//! * [`plan_paths`] turns af_core state (`Orders`, `PathState`, `MoveGoal`,
//!   `AttackTarget`) into a [`RoutePlan`] of ground-plane polylines, each
//!   tagged with what it is for ([`plan_route`], pure). Only the local
//!   player's own units are planned, so selecting an enemy for its stats
//!   never shows where it is going, and an entity target of another team
//!   (an attack, a chase, a gather on a node) is placed where the local
//!   team believes it is, as rally flags are ([`believed_rally_point`]):
//!   a unit as last seen, a building or node once scouted, so nothing in
//!   the fog shows. A chase after an entity out of sight is a straight
//!   line to where it was seen, not the path (which leads where it is).
//! * [`draw_paths`] draws the plan with gizmos ([`PathGizmos`], and
//!   [`PathXrayGizmos`] faint over everything, so a route behind a
//!   building still reads): lines a constant few pixels wide at any zoom
//!   that spawn no entities (nothing touches the units, so the sim's
//!   archetypes stay as they are), cost nothing while no route is planned
//!   and stay cheap for a big army: at most [`MAX_ROUTES`] units are
//!   drawn, those nearest the centre of the view. Gizmos are only
//!   registered with Bevy's `GizmoPlugin`, so headless apps and tests plan
//!   without drawing.
//!
//! With [`ShowPaths`] visible (P, or Shift held), each unit's current leg
//! is drawn from the unit through the waypoints still ahead to its live
//! goal (straight for flyers and while the path is being found; stopped
//! with a cross where a partial path ends), solid with chevrons marching
//! toward the goal, and each queued order chains on as a dimmer dashed
//! leg to its target, coloured by order. A selected unit whose current
//! order is a patrol always shows its loop ([`patrol_points`]), chevrons
//! running round it and the leg being walked bright; that leg is read off
//! the unit's `MoveGoal`, which points at the patrol point it walks to.

use std::f32::consts::FRAC_PI_2;
use std::ops::Range;

use af_core::ext::behavior::patrol_points;
use af_core::ext::input::ShowPaths;
use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::gizmos::AppGizmoBuilder;
use bevy::gizmos::config::{GizmoConfig, GizmoConfigGroup, GizmoConfigStore, GizmoLineConfig, GizmoLineJoint};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::entities::Ground;
use crate::fog::viewing_team;
use crate::overlay::{RallyTargets, Sighting, believed_rally_point};

/// Most selected units whose routes are drawn; past that, the ones nearest
/// the centre of the view.
pub const MAX_ROUTES: usize = 64;

/// Lines and marks float this high above the drawn ground (`Ground::surface`).
const LIFT: f32 = 0.1;
/// Longest straight piece of a solid line, so it follows the terrain.
const STEP: f32 = 1.0;
/// World size of a screen pixel at the camera's focus per metre of camera
/// distance (as `parts::min_line_width` reckons it).
const PIXEL_PER_DISTANCE: f32 = 0.00064;
/// Gizmo line width (px).
const LINE_PX: f32 = 3.0;
/// Pull lines this far toward the camera in depth (see `GizmoConfig`), so
/// a line over a step in the terrain is not cut by its edge, while units
/// and buildings standing on it still hide it.
const DEPTH_BIAS: f32 = -0.01;
/// Dashes of a queued leg: length and gap (px).
const DASH_PX: f32 = 12.0;
const GAP_PX: f32 = 9.0;
/// Chevrons: half their size and the spacing between them (px); close
/// up, never smaller than `MIN_CHEVRON` (m) nor closer than
/// `CHEVRON_SPREAD` of their half sizes.
const CHEVRON_PX: f32 = 6.0;
const CHEVRON_GAP_PX: f32 = 46.0;
const MIN_CHEVRON: f32 = 0.14;
const CHEVRON_SPREAD: f32 = 7.0;
/// How fast dashes and chevrons run toward the goal (px a second).
const FLOW_PX: f32 = 40.0;
/// Radii (px) of a waypoint dot, an order target's ring, a patrol point's
/// diamond and the cross where a path is blocked.
const DOT_PX: f32 = 2.0;
const TARGET_PX: f32 = 8.0;
const PATROL_POINT_PX: f32 = 7.0;
const BLOCKED_PX: f32 = 7.0;
/// Marks (not dots) are never drawn smaller than this (m), so they keep
/// their size against the units when zoomed in.
const MIN_MARK: f32 = 0.25;

/// How bright (a fraction of the order's colour) the leg being walked, a
/// queued leg, and the other legs of a patrol loop (one being walked, one
/// queued) are drawn: darker rather than more transparent, which stands
/// apart better on bright ground; and the x-ray pass's opacity.
const CURRENT_SHADE: f32 = 1.0;
const QUEUED_SHADE: f32 = 0.8;
const LOOP_SHADE: f32 = 0.65;
const QUEUED_LOOP_SHADE: f32 = 0.5;
const XRAY_ALPHA: f32 = 0.3;
/// Where a path is blocked.
const BLOCKED: Rgb = [1.0, 0.2, 0.18];
/// The patrol leg being walked: patrol cyan lit toward white.
const WALKING: Rgb = [0.7, 0.97, 1.0];

/// af_behavior points a patrolling unit's `MoveGoal` at the patrol point it
/// walks to, to within 0.1 m.
const LEG_END_SLACK: f32 = 0.15;
/// The current leg already ends at the front order's target within this
/// distance: a point, or an entity (whose box the unit walks up to).
const POINT_SLACK: f32 = 0.5;
const ENTITY_SLACK: f32 = 3.0;
/// Points closer than this are one.
const SAME_POINT: f32 = 0.05;

/// The route overlay's gizmos, hidden where the world stands in front of
/// them.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct PathGizmos;

/// The same strokes drawn faint over everything (the x-ray pass), so a
/// route behind a building or a hill still reads.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct PathXrayGizmos;

/// Register the route gizmos (an app with Bevy's `GizmoPlugin`).
pub fn register_gizmos(app: &mut App) {
    let line = GizmoLineConfig { width: LINE_PX, joints: GizmoLineJoint::Round(4), ..default() };
    app.insert_gizmo_config(PathGizmos, GizmoConfig { line: line.clone(), depth_bias: DEPTH_BIAS, ..default() })
        .insert_gizmo_config(PathXrayGizmos, GizmoConfig { line, depth_bias: -1.0, ..default() });
}

/// Run condition: the route gizmos are registered (a windowed app).
pub(crate) fn gizmos_ready(store: Option<Res<GizmoConfigStore>>) -> bool {
    store.is_some_and(|s| s.get_config::<PathGizmos>().is_some())
}

/// What a leg is for, which sets its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegKind {
    Move,
    AttackMove,
    /// Attacking (or chasing) a target.
    Attack,
    /// Gathering, or carrying the load to a drop site.
    Gather,
    /// Building or repairing.
    Build,
    Patrol,
    Follow,
}

impl LegKind {
    /// What `order` makes its unit do (`None`: it does not go anywhere).
    pub fn of(order: &Order) -> Option<LegKind> {
        Some(match order {
            Order::Move { .. } => LegKind::Move,
            Order::AttackMove { .. } => LegKind::AttackMove,
            Order::Attack { .. } => LegKind::Attack,
            Order::Gather { .. } | Order::ReturnCargo => LegKind::Gather,
            Order::Build { .. } | Order::Repair { .. } => LegKind::Build,
            Order::Patrol { .. } => LegKind::Patrol,
            Order::Follow { .. } => LegKind::Follow,
            Order::Hold => return None,
        })
    }

    /// Move green, attack-move orange, attack red, gather gold, build and
    /// repair amber, patrol cyan, follow white.
    pub fn color(self) -> Rgb {
        match self {
            LegKind::Move => [0.3, 1.0, 0.35],
            LegKind::AttackMove => [1.0, 0.45, 0.12],
            LegKind::Attack => [1.0, 0.2, 0.17],
            LegKind::Gather => [1.0, 0.86, 0.25],
            LegKind::Build => [0.95, 0.62, 0.25],
            LegKind::Patrol => [0.25, 0.9, 1.0],
            LegKind::Follow => [0.95, 0.95, 0.95],
        }
    }
}

/// Where an order sends its unit.
enum Spot {
    Point(Vec2),
    Entity(Entity),
    /// Nowhere the order itself names (`ReturnCargo`, `Hold`).
    Nowhere,
}

fn spot(order: &Order) -> Spot {
    match *order {
        Order::Move { target } | Order::AttackMove { target } => Spot::Point(target),
        Order::Patrol { a, .. } => Spot::Point(a),
        Order::Attack { target } | Order::Gather { target } | Order::Repair { target } | Order::Follow { target } => Spot::Entity(target),
        Order::Build { site } => Spot::Entity(site),
        Order::ReturnCargo | Order::Hold => Spot::Nowhere,
    }
}

/// The entity an order names, if any.
fn order_entity(order: &Order) -> Option<Entity> {
    match spot(order) {
        Spot::Entity(e) => Some(e),
        _ => None,
    }
}

/// Where the local team believes an entity is, and whether that is where it
/// is now (its own, or in sight).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Located {
    pub pos: Vec2,
    pub live: bool,
}

/// One unit, as its route is planned from it.
#[derive(Clone, Copy, Debug)]
pub struct Walker<'a> {
    pub pos: Vec2,
    pub orders: &'a Orders,
    pub path: &'a PathState,
    pub goal: Option<&'a MoveGoal>,
    /// What it is shooting at or chasing (`AttackTarget`).
    pub attacking: Option<Entity>,
    pub flying: bool,
}

/// A stretch of a route, from where the unit is (or will be) to where an
/// order takes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Leg {
    /// Its points in [`RoutePlan::points`], in walking order.
    pub points: Range<usize>,
    pub kind: LegKind,
    /// Being walked now (else queued).
    pub current: bool,
    /// Ends short of its goal, where no way on was found (a partial path, or
    /// the path service gave up).
    pub blocked: bool,
    /// Ends at an order's target (marked there).
    pub target: bool,
}

/// A patrol route, walked round and round.
#[derive(Clone, Debug, PartialEq)]
pub struct PatrolLoop {
    /// The patrol points in [`RoutePlan::points`], in walking order; the
    /// last leg leads back to the first.
    pub points: Range<usize>,
    /// The patrol is its unit's current order (else it is queued).
    pub active: bool,
    /// The leg being walked: from point `i` to the next.
    pub walking: Option<usize>,
}

/// Routes as polylines on the ground plane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoutePlan {
    pub points: Vec<Vec2>,
    pub legs: Vec<Leg>,
    pub loops: Vec<PatrolLoop>,
}

impl RoutePlan {
    pub fn clear(&mut self) {
        self.points.clear();
        self.legs.clear();
        self.loops.clear();
    }

    pub fn leg_points(&self, leg: &Leg) -> &[Vec2] {
        &self.points[leg.points.clone()]
    }

    pub fn loop_points(&self, patrol: &PatrolLoop) -> &[Vec2] {
        &self.points[patrol.points.clone()]
    }

    /// Append `p` unless it repeats the last point since `from`.
    fn push(&mut self, from: usize, p: Vec2) {
        if self.points.len() == from || self.points.last().is_none_or(|q| q.distance(p) > SAME_POINT) {
            self.points.push(p);
        }
    }

    /// A straight queued leg from `from` to `to` (none when they meet).
    fn straight(&mut self, from: Vec2, to: Vec2, kind: LegKind) {
        if from.distance(to) <= SAME_POINT {
            return;
        }
        let start = self.points.len();
        self.points.extend([from, to]);
        self.legs.push(Leg { points: start..start + 2, kind, current: false, blocked: false, target: true });
    }

    /// The patrol route that starts with the first of `orders`.
    fn patrol<'a>(&mut self, orders: impl IntoIterator<Item = &'a Order>, active: bool, goal: Option<&MoveGoal>) {
        let start = self.points.len();
        self.points.extend(patrol_points(orders));
        let walking = if active { patrol_leg(&self.points[start..], goal) } else { None };
        self.loops.push(PatrolLoop { points: start..self.points.len(), active, walking });
    }
}

/// The leg of a patrol along `points` its unit is walking: the one ending
/// where its point goal is (`None` while it fights, its goal on a foe).
pub fn patrol_leg(points: &[Vec2], goal: Option<&MoveGoal>) -> Option<usize> {
    let goal = goal.filter(|g| g.follow.is_none())?;
    let n = points.len();
    let end = points.iter().position(|p| p.distance(goal.target) <= LEG_END_SLACK)?;
    Some((end + n - 1) % n)
}

/// Add `w`'s route to `plan`: with `full`, the leg it is walking and its
/// queued orders, else only the loop of the patrol it is on (if any).
/// `locate` says where the local team believes an entity is (`None`:
/// unknown, so a leg to it is left out).
pub fn plan_route(w: &Walker, full: bool, mut locate: impl FnMut(Entity) -> Option<Located>, plan: &mut RoutePlan) {
    let queue = &w.orders.queue;
    let front = queue.front();
    if !full {
        if matches!(front, Some(Order::Patrol { .. })) {
            plan.patrol(queue, true, w.goal);
        }
        return;
    }
    // The leg being walked, and whether it ends at the front order's target.
    let mut anchor = w.pos;
    let mut reached = false;
    if let Some(goal) = w.goal {
        let start = plan.points.len();
        let end = match goal.follow {
            Some(e) => locate(e),
            None => Some(Located { pos: goal.target, live: true }),
        };
        // Waypoints of an older goal (the new one not yet planned) are not this leg's.
        let fresh = w.path.goal_seq == goal.seq;
        let blocked = fresh && (w.path.partial || w.path.status == PathStatus::Failed);
        plan.push(start, w.pos);
        if fresh && !w.flying && end.is_some_and(|e| e.live) {
            let next = w.path.next.min(w.path.waypoints.len());
            for p in &w.path.waypoints[next..] {
                plan.push(start, *p);
            }
        }
        match end {
            Some(e) if !blocked => plan.push(start, e.pos),
            _ => {}
        }
        anchor = plan.points[plan.points.len() - 1];
        reached = !blocked
            && match front.map(spot) {
                Some(Spot::Point(p)) => p.distance(anchor) <= POINT_SLACK,
                Some(Spot::Entity(e)) => goal.follow == Some(e) || locate(e).is_some_and(|l| l.pos.distance(anchor) <= ENTITY_SLACK),
                _ => false,
            };
        let kind = match (goal.follow, w.attacking) {
            (Some(f), Some(a)) if f == a => LegKind::Attack,
            _ => front.and_then(LegKind::of).unwrap_or(if goal.follow.is_some() { LegKind::Follow } else { LegKind::Move }),
        };
        if plan.points.len() - start >= 2 || blocked {
            plan.legs.push(Leg { points: start..plan.points.len(), kind, current: true, blocked, target: reached });
        } else {
            plan.points.truncate(start);
        }
    }
    // Then each order in turn, from where the one before leaves the unit.
    for (i, order) in queue.iter().enumerate() {
        if let Order::Patrol { a, .. } = order {
            if i > 0 {
                plan.straight(anchor, *a, LegKind::Patrol);
            }
            plan.patrol(queue.iter().skip(i), i == 0, w.goal);
            // A patrol never ends: what is queued after it is never reached.
            break;
        }
        if i == 0 && reached {
            continue;
        }
        let Some(kind) = LegKind::of(order) else { continue };
        let to = match spot(order) {
            Spot::Point(p) => p,
            Spot::Entity(e) => match locate(e) {
                Some(l) => l.pos,
                None => continue,
            },
            Spot::Nowhere => continue,
        };
        plan.straight(anchor, to, kind);
        anchor = to;
    }
}

/// The routes drawn this frame, and what the overlay keeps between frames.
/// Client-only bookkeeping, so hashing order does not matter.
#[derive(Resource, Default)]
pub struct PathOverlay {
    pub plan: RoutePlan,
    /// Selected own units whose routes were left out this frame (past [`MAX_ROUTES`]).
    pub left_out: usize,
    /// Entity targets of other teams, as the viewing team last saw them.
    sightings: HashMap<Entity, Sighting>,
    /// The viewing team `sightings` belong to.
    team: Option<u8>,
    /// Scratch: where the team believes each entity the local player's
    /// units have an order on or chase is this frame (`None`: unknown).
    believed: HashMap<Entity, Option<Vec2>>,
    /// The patrol leg each drawn unit was last seen walking, with the first
    /// point of its route: shown while it fights (its goal then on a foe).
    walking: HashMap<Entity, (Vec2, usize)>,
    /// Scratch: the units drawn, by distance from the view's centre.
    drawn: Vec<(f32, Entity)>,
}

type Walkers<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Owner,
        &'static Position,
        &'static Orders,
        &'static PathState,
        Option<&'static MoveGoal>,
        Option<&'static AttackTarget>,
        Option<&'static Mobile>,
        Has<Selected>,
    ),
    (With<Unit>, Without<Dead>),
>;

/// Plan the routes of the local player's selected units (see the module docs).
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_paths(
    mut overlay: ResMut<PathOverlay>,
    show: Option<Res<ShowPaths>>,
    local: Res<LocalPlayer>,
    players: Res<Players>,
    fog: Res<FogOfWar>,
    rig: Res<CameraRig>,
    units: Walkers,
    targets: RallyTargets,
) {
    let PathOverlay { plan, left_out, sightings, team: seen_by, believed, walking, drawn } = &mut *overlay;
    plan.clear();
    *left_out = 0;
    drawn.clear();
    let team = viewing_team(&fog, &local, &players);
    if *seen_by != team {
        sightings.clear();
        *seen_by = team;
    }
    let Some(me) = local.0 else {
        walking.clear();
        return;
    };
    // Every entity the local player's units have orders on or chase,
    // selected or not, so one selected later shows what the team last saw.
    believed.clear();
    for (_, owner, _, orders, _, goal, ..) in &units {
        if owner.0 != me {
            continue;
        }
        for t in orders.queue.iter().filter_map(order_entity).chain(goal.and_then(|g| g.follow)) {
            if !believed.contains_key(&t) {
                let at = believed_rally_point(t, team, &fog, &players, &targets, sightings);
                believed.insert(t, at);
            }
        }
    }
    sightings.retain(|t, _| believed.contains_key(t));
    let full = show.is_some_and(|s| s.visible());
    for (e, owner, pos, orders, _, _, _, _, selected) in &units {
        if selected && owner.0 == me && (full || matches!(orders.current(), Some(Order::Patrol { .. }))) {
            drawn.push((pos.0.distance_squared(rig.focus), e));
        }
    }
    if drawn.len() > MAX_ROUTES {
        drawn.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        *left_out = drawn.len() - MAX_ROUTES;
        drawn.truncate(MAX_ROUTES);
    }
    let locate = |e: Entity| {
        let pos = (*believed.get(&e)?)?;
        Some(Located { pos, live: targets.get(e).is_ok_and(|t| t.0.0 == pos) })
    };
    for &(_, e) in drawn.iter() {
        let Ok((_, _, pos, orders, path, goal, attack, mobile, _)) = units.get(e) else { continue };
        let w = Walker { pos: pos.0, orders, path, goal, attacking: attack.map(|a| a.entity), flying: mobile.is_some_and(|m| m.flying) };
        let first = plan.loops.len();
        plan_route(&w, full, &locate, plan);
        for l in plan.loops[first..].iter_mut().filter(|l| l.active) {
            let start = plan.points[l.points.start];
            match l.walking {
                Some(leg) => {
                    walking.insert(e, (start, leg));
                }
                None => {
                    l.walking = walking.get(&e).filter(|(s, leg)| *s == start && *leg < l.points.len()).map(|(_, leg)| *leg);
                }
            }
        }
    }
    walking.retain(|e, _| drawn.iter().any(|(_, d)| d == e));
}

/// Sizes in metres for the current zoom, and where the flow is.
struct Scale {
    /// One pixel.
    px: f32,
    /// How far dashes and chevrons have run (m).
    flow: f32,
}

impl Scale {
    /// `n` pixels, but at least [`MIN_MARK`] (marks).
    fn mark(&self, n: f32) -> f32 {
        (self.px * n).max(MIN_MARK)
    }

    fn dash_period(&self) -> f32 {
        self.px * (DASH_PX + GAP_PX)
    }

    /// A chevron's half size and the spacing between chevrons.
    fn chevron(&self) -> (f32, f32) {
        let h = (self.px * CHEVRON_PX).max(MIN_CHEVRON);
        (h, (self.px * CHEVRON_GAP_PX).max(h * CHEVRON_SPREAD))
    }
}

/// `rgb` at `shade` of its brightness.
fn color(rgb: Rgb, shade: f32) -> Color {
    Color::srgb(rgb[0] * shade, rgb[1] * shade, rgb[2] * shade)
}

/// `c` as the x-ray pass draws it.
fn faint(c: Color) -> Color {
    c.with_alpha(c.alpha() * XRAY_ALPHA)
}

/// Draw the planned routes (see the module docs).
pub(crate) fn draw_paths(
    overlay: Res<PathOverlay>,
    ground: Ground,
    rig: Res<CameraRig>,
    time: Res<Time<Real>>,
    mut pens: Pens,
    mut strip: Local<Vec<Vec3>>,
) {
    let plan = &overlay.plan;
    if plan.legs.is_empty() && plan.loops.is_empty() {
        return;
    }
    let px = rig.distance.max(1.0) * PIXEL_PER_DISTANCE;
    let s = Scale { px, flow: time.elapsed_secs() * FLOW_PX * px };
    let mut d = Draw { pens: &mut pens, ground: &ground, strip: &mut strip, s };
    for l in &plan.loops {
        d.patrol_loop(plan.loop_points(l), l);
    }
    for leg in &plan.legs {
        d.leg(plan.leg_points(leg), leg);
    }
}

/// The route gizmos: the lit pass and the x-ray pass.
#[derive(SystemParam)]
pub(crate) struct Pens<'w, 's> {
    lit: Gizmos<'w, 's, PathGizmos>,
    xray: Gizmos<'w, 's, PathXrayGizmos>,
}

/// Draws on the ground with the route gizmos: every stroke goes to both
/// passes, the x-ray one faint.
struct Draw<'a, 'w, 's> {
    pens: &'a mut Pens<'w, 's>,
    ground: &'a Ground<'a>,
    strip: &'a mut Vec<Vec3>,
    s: Scale,
}

impl Draw<'_, '_, '_> {
    /// `p` on the drawn ground, lifted clear of it.
    fn lift(&self, p: Vec2) -> Vec3 {
        Vec3::new(p.x, self.ground.surface(p) + LIFT, p.y)
    }

    fn leg(&mut self, pts: &[Vec2], leg: &Leg) {
        let rgb = leg.kind.color();
        if leg.current {
            let c = color(rgb, CURRENT_SHADE);
            self.solid(pts, c);
            self.chevrons(pts, c);
            // A dot at each waypoint still ahead.
            for p in pts.iter().skip(1).take(pts.len().saturating_sub(2)) {
                self.ring(*p, self.s.px * DOT_PX, c);
            }
        } else {
            self.dashed(pts, color(rgb, QUEUED_SHADE));
        }
        let Some(&end) = pts.last() else { return };
        if leg.blocked {
            self.cross(end, self.s.mark(BLOCKED_PX), color(BLOCKED, CURRENT_SHADE));
        } else if leg.target {
            self.ring(end, self.s.mark(TARGET_PX), color(rgb, if leg.current { CURRENT_SHADE } else { QUEUED_SHADE }));
        }
    }

    fn patrol_loop(&mut self, pts: &[Vec2], l: &PatrolLoop) {
        let rgb = LegKind::Patrol.color();
        let n = pts.len();
        for i in 0..n {
            let leg = [pts[i], pts[(i + 1) % n]];
            let c = match (l.walking == Some(i), l.active) {
                (true, _) => color(WALKING, CURRENT_SHADE),
                (false, true) => color(rgb, LOOP_SHADE),
                (false, false) => color(rgb, QUEUED_LOOP_SHADE),
            };
            if l.active {
                self.solid(&leg, c);
                self.chevrons(&leg, c);
            } else {
                self.dashed(&leg, c);
            }
        }
        for (i, p) in pts.iter().enumerate() {
            // The point being walked to stands out.
            let next = l.walking.is_some_and(|w| (w + 1) % n == i);
            let r = self.s.mark(if next { PATROL_POINT_PX * 1.4 } else { PATROL_POINT_PX });
            self.diamond(*p, r, if next { color(WALKING, CURRENT_SHADE) } else { color(rgb, QUEUED_SHADE) });
        }
    }

    /// A solid line along `pts`, in pieces of at most [`STEP`] so it
    /// follows the ground.
    fn solid(&mut self, pts: &[Vec2], c: Color) {
        self.strip.clear();
        for w in pts.windows(2) {
            let pieces = (w[0].distance(w[1]) / STEP).ceil().clamp(1.0, 256.0) as usize;
            for k in 0..pieces {
                let p = self.lift(w[0].lerp(w[1], k as f32 / pieces as f32));
                self.strip.push(p);
            }
        }
        if let Some(&last) = pts.last() {
            let p = self.lift(last);
            self.strip.push(p);
        }
        if self.strip.len() >= 2 {
            self.pens.lit.linestrip(self.strip.iter().copied(), c);
            self.pens.xray.linestrip(self.strip.iter().copied(), faint(c));
        }
    }

    /// Dashes running along `pts`.
    fn dashed(&mut self, pts: &[Vec2], c: Color) {
        let (period, len) = (self.s.dash_period(), self.s.px * DASH_PX);
        let phase = self.s.flow.rem_euclid(period);
        dashes(pts, period, len, phase, |a, b| {
            let (a, b) = (self.lift(a), self.lift(b));
            self.line(a, b, c);
        });
    }

    /// Chevrons pointing along `pts`, running toward its end.
    fn chevrons(&mut self, pts: &[Vec2], c: Color) {
        let (h, gap) = self.s.chevron();
        along(pts, gap, self.s.flow.rem_euclid(gap), |p, dir| {
            let y = self.ground.surface(p) + LIFT;
            let side = dir.perp() * h * 1.2;
            let at = |q: Vec2| Vec3::new(q.x, y, q.y);
            self.polyline([at(p - dir * h + side), at(p + dir * h), at(p - dir * h - side)], c);
        });
    }

    fn ring(&mut self, p: Vec2, r: f32, c: Color) {
        let at = Isometry3d::new(self.lift(p), Quat::from_rotation_x(FRAC_PI_2));
        self.pens.lit.circle(at, r, c).resolution(16);
        self.pens.xray.circle(at, r, faint(c)).resolution(16);
    }

    fn diamond(&mut self, p: Vec2, r: f32, c: Color) {
        let at = self.lift(p);
        let (x, z) = (Vec3::X * r, Vec3::Z * r);
        self.polyline([at + x, at + z, at - x, at - z, at + x], c);
    }

    fn cross(&mut self, p: Vec2, r: f32, c: Color) {
        let at = self.lift(p);
        let (a, b) = (Vec3::new(r, 0.0, r), Vec3::new(r, 0.0, -r));
        self.line(at - a, at + a, c);
        self.line(at - b, at + b, c);
    }

    fn line(&mut self, a: Vec3, b: Vec3, c: Color) {
        self.pens.lit.line(a, b, c);
        self.pens.xray.line(a, b, faint(c));
    }

    fn polyline<const N: usize>(&mut self, pts: [Vec3; N], c: Color) {
        self.pens.lit.linestrip(pts, c);
        self.pens.xray.linestrip(pts, faint(c));
    }
}

/// Calls `f` with the ends of each dash along `pts`: `len` long, one every
/// `period`, the first starting `phase` in (`phase` in `[0, period)`);
/// a dash over a corner is cut in two there.
pub fn dashes(pts: &[Vec2], period: f32, len: f32, phase: f32, mut f: impl FnMut(Vec2, Vec2)) {
    if period <= 0.0 || len <= 0.0 {
        return;
    }
    let mut s0 = 0.0;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = a.distance(b);
        if seg <= f32::EPSILON {
            continue;
        }
        let (dir, s1) = ((b - a) / seg, s0 + seg);
        let mut start = phase + ((s0 - phase - len) / period).floor() * period;
        while start < s1 {
            let (lo, hi) = (start.max(s0), (start + len).min(s1));
            if hi > lo {
                f(a + dir * (lo - s0), a + dir * (hi - s0));
            }
            start += period;
        }
        s0 = s1;
    }
}

/// Calls `f` with a point and the direction of travel there every
/// `spacing` along `pts`, the first `phase` in.
pub fn along(pts: &[Vec2], spacing: f32, phase: f32, mut f: impl FnMut(Vec2, Vec2)) {
    if spacing <= 0.0 {
        return;
    }
    let mut s0 = 0.0;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = a.distance(b);
        if seg <= f32::EPSILON {
            continue;
        }
        let (dir, s1) = ((b - a) / seg, s0 + seg);
        let mut s = phase + ((s0 - phase) / spacing).ceil() * spacing;
        while s < s1 {
            f(a + dir * (s - s0), dir);
            s += spacing;
        }
        s0 = s1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    fn entity(i: u32) -> Entity {
        Entity::from_raw_u32(i).unwrap()
    }

    fn orders(list: &[Order]) -> Orders {
        Orders { queue: list.iter().copied().collect() }
    }

    fn goal(target: Vec2, seq: u32) -> MoveGoal {
        MoveGoal { target, arrive: 0.35, follow: None, seq }
    }

    fn following(path: Vec<Vec2>, next: usize, seq: u32) -> PathState {
        PathState { waypoints: path, next, status: PathStatus::Following, goal_seq: seq, ..default() }
    }

    /// The plan of one walker, with entities located by `at`.
    fn plan_of(w: &Walker, full: bool, at: &[(Entity, Located)]) -> RoutePlan {
        let mut plan = RoutePlan::default();
        plan_route(w, full, |e| at.iter().find(|(x, _)| *x == e).map(|(_, l)| *l), &mut plan);
        plan
    }

    fn legs(plan: &RoutePlan) -> Vec<(Vec<Vec2>, LegKind, bool)> {
        plan.legs.iter().map(|l| (plan.leg_points(l).to_vec(), l.kind, l.current)).collect()
    }

    #[test]
    fn the_current_leg_runs_through_the_waypoints_ahead_then_queued_orders_chain_on() {
        let target = entity(7);
        let o = orders(&[Order::Move { target: p(10.0, 0.0) }, Order::AttackMove { target: p(10.0, 10.0) }, Order::Attack { target }, Order::Hold]);
        let path = following(vec![p(2.0, 2.0), p(6.0, 2.0), p(10.0, 0.0)], 1, 4);
        let g = goal(p(10.0, 0.0), 4);
        let w = Walker { pos: p(3.0, 2.0), orders: &o, path: &path, goal: Some(&g), attacking: None, flying: false };
        let plan = plan_of(&w, true, &[(target, Located { pos: p(0.0, 10.0), live: true })]);
        assert_eq!(
            legs(&plan),
            vec![
                (vec![p(3.0, 2.0), p(6.0, 2.0), p(10.0, 0.0)], LegKind::Move, true),
                (vec![p(10.0, 0.0), p(10.0, 10.0)], LegKind::AttackMove, false),
                (vec![p(10.0, 10.0), p(0.0, 10.0)], LegKind::Attack, false),
            ],
            "waypoints already passed are left out; Hold goes nowhere"
        );
        assert!(plan.legs.iter().all(|l| l.target && !l.blocked));
        assert!(plan.loops.is_empty());
        assert!(plan_of(&w, false, &[]).legs.is_empty(), "with paths hidden only patrols show");
    }

    #[test]
    fn pending_flying_and_stale_paths_go_straight_to_the_goal() {
        let o = orders(&[Order::Move { target: p(9.0, 9.0) }]);
        let g = goal(p(9.0, 9.0), 5);
        let straight = vec![(vec![p(0.0, 0.0), p(9.0, 9.0)], LegKind::Move, true)];
        let pending = PathState { status: PathStatus::Pending, goal_seq: 5, ..default() };
        let w = Walker { pos: p(0.0, 0.0), orders: &o, path: &pending, goal: Some(&g), attacking: None, flying: false };
        assert_eq!(legs(&plan_of(&w, true, &[])), straight, "being planned");
        let older = following(vec![p(0.0, 5.0), p(3.0, 3.0)], 0, 4);
        assert_eq!(legs(&plan_of(&Walker { path: &older, ..w }, true, &[])), straight, "the waypoints are another goal's");
        let flyer = following(vec![p(9.0, 9.0)], 0, 5);
        let mut detour = flyer.clone();
        detour.waypoints = vec![p(0.0, 9.0), p(9.0, 9.0)];
        assert_eq!(legs(&plan_of(&Walker { path: &detour, flying: true, ..w }, true, &[])), straight, "flyers fly straight");
    }

    #[test]
    fn a_partial_path_stops_where_it_is_blocked() {
        let o = orders(&[Order::Move { target: p(20.0, 0.0) }, Order::Move { target: p(20.0, 5.0) }]);
        let mut path = following(vec![p(5.0, 0.0), p(8.0, 1.0)], 0, 2);
        path.partial = true;
        let g = goal(p(20.0, 0.0), 2);
        let w = Walker { pos: p(0.0, 0.0), orders: &o, path: &path, goal: Some(&g), attacking: None, flying: false };
        let plan = plan_of(&w, true, &[]);
        assert_eq!(plan.leg_points(&plan.legs[0]), &[p(0.0, 0.0), p(5.0, 0.0), p(8.0, 1.0)]);
        assert!(plan.legs[0].blocked && !plan.legs[0].target);
        // The order's own target, then the next order, chain on from there.
        assert_eq!(plan.leg_points(&plan.legs[1]), &[p(8.0, 1.0), p(20.0, 0.0)]);
        assert_eq!(plan.leg_points(&plan.legs[2]), &[p(20.0, 0.0), p(20.0, 5.0)]);
    }

    #[test]
    fn chases_and_follows_end_at_the_entity_as_believed() {
        let foe = entity(3);
        let o = orders(&[Order::AttackMove { target: p(30.0, 0.0) }]);
        let path = following(vec![p(4.0, 4.0), p(8.0, 4.0)], 0, 9);
        let g = MoveGoal { target: p(8.0, 4.0), arrive: 1.0, follow: Some(foe), seq: 9 };
        let w = Walker { pos: p(0.0, 0.0), orders: &o, path: &path, goal: Some(&g), attacking: Some(foe), flying: false };
        let seen = plan_of(&w, true, &[(foe, Located { pos: p(9.0, 5.0), live: true })]);
        assert_eq!(
            legs(&seen),
            vec![(vec![p(0.0, 0.0), p(4.0, 4.0), p(8.0, 4.0), p(9.0, 5.0)], LegKind::Attack, true), (vec![p(9.0, 5.0), p(30.0, 0.0)], LegKind::AttackMove, false)],
            "engaged on the way, then on to the attack-move's end"
        );
        // Out of sight, the path (which leads where it really is) is not drawn.
        let unseen = plan_of(&w, true, &[(foe, Located { pos: p(5.0, 9.0), live: false })]);
        assert_eq!(plan_points(&unseen, 0), vec![p(0.0, 0.0), p(5.0, 9.0)]);
        // Never seen: nothing past the unit.
        let unknown = plan_of(&w, true, &[]);
        assert_eq!(legs(&unknown), vec![(vec![p(0.0, 0.0), p(30.0, 0.0)], LegKind::AttackMove, false)]);
        // A fresh unit walking to its rally point has a goal but no orders.
        let none = Orders::default();
        let rally = goal(p(3.0, 3.0), 1);
        let fresh = Walker { orders: &none, goal: Some(&rally), attacking: None, path: &PathState::default(), ..w };
        assert_eq!(legs(&plan_of(&fresh, true, &[])), vec![(vec![p(0.0, 0.0), p(3.0, 3.0)], LegKind::Move, true)]);
    }

    fn plan_points(plan: &RoutePlan, leg: usize) -> Vec<Vec2> {
        plan.leg_points(&plan.legs[leg]).to_vec()
    }

    #[test]
    fn work_orders_end_the_current_leg_at_their_target() {
        let (tree, hall) = (entity(1), entity(2));
        let o = orders(&[Order::Gather { target: tree }]);
        let path = following(vec![p(5.0, 0.0)], 0, 3);
        // Carrying the load to the hall first, then back to the tree.
        let g = MoveGoal { target: p(5.0, 0.0), arrive: 1.0, follow: Some(hall), seq: 3 };
        let w = Walker { pos: p(0.0, 0.0), orders: &o, path: &path, goal: Some(&g), attacking: None, flying: false };
        let at = [(tree, Located { pos: p(-6.0, 0.0), live: true }), (hall, Located { pos: p(6.0, 0.0), live: true })];
        let plan = plan_of(&w, true, &at);
        assert_eq!(legs(&plan), vec![(vec![p(0.0, 0.0), p(5.0, 0.0), p(6.0, 0.0)], LegKind::Gather, true), (vec![p(6.0, 0.0), p(-6.0, 0.0)], LegKind::Gather, false)]);
        assert!(!plan.legs[0].target);
        // Walking to the tree itself: one leg, ending at its target.
        let to_tree = MoveGoal { follow: Some(tree), ..g };
        let plan = plan_of(&Walker { goal: Some(&to_tree), ..w }, true, &at);
        assert_eq!(plan.legs.len(), 1);
        assert!(plan.legs[0].target);
    }

    #[test]
    fn patrols_show_their_loop_and_the_leg_being_walked() {
        let (a, b, c) = (p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0));
        let o = orders(&[Order::Patrol { a, b }, Order::Patrol { a: b, b: c }, Order::Move { target: p(50.0, 50.0) }]);
        let path = following(vec![c], 0, 6);
        let g = goal(c, 6);
        let w = Walker { pos: p(10.0, 4.0), orders: &o, path: &path, goal: Some(&g), attacking: None, flying: false };
        // Paths hidden: only the loop, walking b -> c.
        let plan = plan_of(&w, false, &[]);
        assert!(plan.legs.is_empty());
        assert_eq!(plan.loops, vec![PatrolLoop { points: 0..3, active: true, walking: Some(1) }]);
        assert_eq!(plan.loop_points(&plan.loops[0]), &[a, b, c]);
        // Shown: the leg being walked too; the move after the patrols never comes.
        let plan = plan_of(&w, true, &[]);
        assert_eq!(legs(&plan), vec![(vec![p(10.0, 4.0), c], LegKind::Patrol, true)]);
        assert_eq!(plan.loops.len(), 1);
        // Fighting on its beat: the goal is on the foe, so the leg is unknown here.
        let foe = MoveGoal { follow: Some(entity(5)), ..g };
        assert_eq!(plan_of(&Walker { goal: Some(&foe), ..w }, false, &[]).loops[0].walking, None);
        assert_eq!(patrol_leg(&[a, b, c], Some(&goal(a, 1))), Some(2), "c back to a");
        assert_eq!(patrol_leg(&[a, b], Some(&goal(b + Vec2::splat(0.05), 1))), Some(0));
        assert_eq!(patrol_leg(&[a, b], Some(&goal(p(5.0, 5.0), 1))), None);
    }

    #[test]
    fn a_queued_patrol_hangs_off_the_orders_before_it() {
        let o = orders(&[Order::Move { target: p(5.0, 0.0) }, Order::Patrol { a: p(5.0, 0.0), b: p(5.0, 8.0) }]);
        let g = goal(p(5.0, 0.0), 1);
        let path = following(vec![p(5.0, 0.0)], 0, 1);
        let w = Walker { pos: p(0.0, 0.0), orders: &o, path: &path, goal: Some(&g), attacking: None, flying: false };
        let plan = plan_of(&w, true, &[]);
        assert_eq!(legs(&plan), vec![(vec![p(0.0, 0.0), p(5.0, 0.0)], LegKind::Move, true)]);
        assert_eq!(plan.loops, vec![PatrolLoop { points: 2..4, active: false, walking: None }]);
        assert!(plan_of(&w, false, &[]).loops.is_empty(), "not patrolling yet: hidden with the paths");
    }

    #[test]
    fn dashes_march_and_cut_at_corners() {
        let mut out = Vec::new();
        dashes(&[p(0.0, 0.0), p(5.0, 0.0), p(5.0, 5.0)], 4.0, 2.0, 1.0, |a, b| out.push((a, b)));
        assert_eq!(out, vec![(p(1.0, 0.0), p(3.0, 0.0)), (p(5.0, 0.0), p(5.0, 2.0)), (p(5.0, 4.0), p(5.0, 5.0))]);
        // A dash started before the corner goes on round it.
        let mut out = Vec::new();
        dashes(&[p(0.0, 0.0), p(5.0, 0.0), p(5.0, 5.0)], 4.0, 2.0, 0.0, |a, b| out.push((a, b)));
        assert_eq!(out, vec![(p(0.0, 0.0), p(2.0, 0.0)), (p(4.0, 0.0), p(5.0, 0.0)), (p(5.0, 0.0), p(5.0, 1.0)), (p(5.0, 3.0), p(5.0, 5.0))]);
        let mut n = 0;
        dashes(&[p(0.0, 0.0), p(1.0, 0.0)], 0.0, 1.0, 0.0, |_, _| n += 1);
        assert_eq!(n, 0, "no period, no dashes");
    }

    #[test]
    fn chevrons_are_spaced_along_the_route_and_point_along_it() {
        let mut out = Vec::new();
        along(&[p(0.0, 0.0), p(3.0, 0.0), p(3.0, 4.0)], 2.0, 0.5, |at, dir| out.push((at, dir)));
        assert_eq!(out, vec![(p(0.5, 0.0), Vec2::X), (p(2.5, 0.0), Vec2::X), (p(3.0, 1.5), Vec2::Y), (p(3.0, 3.5), Vec2::Y)]);
    }

    #[test]
    fn every_order_has_its_colour() {
        let kinds = [LegKind::Move, LegKind::AttackMove, LegKind::Attack, LegKind::Gather, LegKind::Build, LegKind::Patrol, LegKind::Follow];
        for (i, a) in kinds.iter().enumerate() {
            for b in &kinds[i + 1..] {
                assert_ne!(a.color(), b.color(), "{a:?} and {b:?}");
            }
        }
        assert_eq!(LegKind::of(&Order::ReturnCargo), Some(LegKind::Gather));
        assert_eq!(LegKind::of(&Order::Repair { target: entity(1) }), Some(LegKind::Build));
        assert_eq!(LegKind::of(&Order::Hold), None);
    }
}
