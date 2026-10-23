//! Fortification: towers in front of the main hall, a wall across the
//! approach for defensive factions, and upgrading both along their chains
//! as the ages advance.

use af_core::prelude::*;
use bevy::prelude::*;

use super::{KEEP_RESERVE, Think};
use crate::build::{SpotQuery, pick_workers};
use crate::lanes::{Mover, ground_movers, seals_pocket};
use crate::army::{surplus, surplus_share};
use crate::progress::{cost_total, is_tower, tower_kind, tower_target, wall_kind};

/// Factions below this aggression wall off their approach.
const WALL_AGGRESSION: f32 = 0.5;
/// Wall segments wanted, by age (1..=5); none before towers stand.
const WALL_SEGMENTS: [u32; 5] = [0, 8, 10, 12, 12];
/// The wall runs across the approach this far beyond the main hall's edge.
const WALL_DISTANCE: f32 = 24.0;
/// Open space left in the middle of the wall, so it never seals the base.
const WALL_OPENING: f32 = 7.0;
/// A wall order is not repeated within this many seconds.
const WALL_RESEND: f32 = 60.0;
/// Builders sent to a new wall.
const WALL_BUILDERS: usize = 2;
/// Wall segments ordered to upgrade per think, at most.
const WALL_UPGRADES: usize = 6;
/// Towers the surplus may add beyond the usual number (see
/// `Think::spend_surplus`).
const SURPLUS_TOWERS: u32 = 3;

impl Think<'_, '_> {
    /// One more tower, beyond [`tower_target`] (up to [`SURPLUS_TOWERS`]
    /// more), when the surplus pays for one entirely: the dearest such.
    pub(super) fn surplus_tower(&mut self) {
        let (data, me) = (self.view.data, self.view.me);
        let snap = self.view.snap;
        let tower = |k: BuildingTypeId| is_tower(data, me, k);
        let have = snap.buildings.iter().filter(|b| tower(b.kind)).count() as u32 + self.mem.pending.iter().filter(|p| tower(p.kind)).count() as u32;
        let building = snap.buildings.iter().any(|b| !b.complete && tower(b.kind)) || self.mem.pending.iter().any(|p| tower(p.kind));
        if building || have >= tower_target(me.age, self.view.aggression()) + SURPLUS_TOWERS {
            return;
        }
        let piled = surplus(&self.budget);
        let kind = rules::build_menu(data, me)
            .into_iter()
            .filter(|b| tower(*b) && data.building(*b).requires_node_id.is_none() && rules::can_build(data, me, *b, false).is_ok())
            .filter(|b| surplus_share(&stats::building_cost(data, me, *b), &piled) >= 0.999)
            .max_by(|a, b| cost_total(data, me, *a).total_cmp(&cost_total(data, me, *b)));
        let Some(kind) = kind else { return };
        self.place_tower(kind, have, KEEP_RESERVE);
    }

    /// Put towers in front of the main hall, alternating sides so they
    /// flank the approach (see [`tower_target`]).
    pub(super) fn towers(&mut self) {
        let (data, me) = (self.view.data, self.view.me);
        let target = tower_target(me.age, self.view.aggression());
        if target == 0 {
            return;
        }
        let snap = self.view.snap;
        let tower = |k: BuildingTypeId| is_tower(data, me, k);
        let have = snap.buildings.iter().filter(|b| tower(b.kind)).count() + self.mem.pending.iter().filter(|p| tower(p.kind)).count();
        let building = snap.buildings.iter().any(|b| !b.complete && tower(b.kind)) || self.mem.pending.iter().any(|p| tower(p.kind));
        if have as u32 >= target || building {
            return;
        }
        let Some(kind) = tower_kind(data, me) else { return };
        // The first tower guards the economy; more are a luxury.
        let share = if have == 0 { self.invest_share() } else { KEEP_RESERVE };
        self.place_tower(kind, have as u32, share);
    }

    /// Place a tower of `kind` in front of the main hall, on the side the
    /// `have`-th tower takes.
    fn place_tower(&mut self, kind: BuildingTypeId, have: u32, share: f32) {
        let side = if have.is_multiple_of(2) { 0.6 } else { -0.6 };
        let q = SpotQuery {
            kind,
            anchor: self.plan.home,
            toward: af_geom::dmath::from_angle(side).rotate(self.plan.front),
            min_r: self.plan.hall_radius + self.radius(kind) + 3.0,
            max_r: self.plan.hall_radius + 30.0,
            gap: 2.0,
        };
        self.place(q, 1, share);
    }

    /// Defensive factions run a wall across the approach to the main hall,
    /// in two wings with an opening in the middle, once a tower stands and
    /// the army is at least half its target.
    /// Segments blocked by something are left out; a later order fills the
    /// gaps of the same line. So are segments that would close a ring on
    /// the realm's own units with the buildings beside the line (see
    /// `lanes::seals_pocket`): each wing goes out as the runs of segments
    /// between those.
    pub(super) fn walls(&mut self) {
        let (data, me, now) = (self.view.data, self.view.me, self.now());
        if self.view.aggression() >= WALL_AGGRESSION || self.army_thin() || self.mem.wall_sent.is_some_and(|t| now - t < WALL_RESEND) {
            return;
        }
        let snap = self.view.snap;
        let want = WALL_SEGMENTS[(me.age.clamp(1, MAX_AGE) - 1) as usize];
        let have = snap.buildings.iter().filter(|b| data.building(b.kind).wall).count() as u32;
        if have >= want || !snap.buildings.iter().any(|b| b.complete && is_tower(data, me, b.kind)) {
            return;
        }
        let Some(kind) = wall_kind(data, me) else { return };
        let cost = stats::building_cost(data, me, kind).scaled((want - have) as f32);
        if !self.budget.affords(&cost, KEEP_RESERVE) {
            self.note_shortfall(&cost, KEEP_RESERVE);
            return;
        }
        let segment = data.building(kind).size[0];
        let (from_a, to_a, from_b, to_b) = wall_line(self.plan.home, self.plan.front, self.plan.hall_radius, segment, want);
        let movers = ground_movers(&self.view);
        let mut laid = self.extra.clone();
        let mut runs = self.open_wall_runs(kind, from_a, to_a, &movers, &mut laid);
        runs.extend(self.open_wall_runs(kind, from_b, to_b, &movers, &mut laid));
        if runs.is_empty() {
            self.mem.wall_sent = Some(now);
            return;
        }
        let mut crew = Vec::new();
        pick_workers(&self.view, (from_a + to_b) * 0.5, WALL_BUILDERS, &self.taken, &mut crew);
        if crew.is_empty() {
            return;
        }
        for w in &crew {
            self.claim_worker(*w, Entity::PLACEHOLDER);
        }
        self.budget.spend(&cost);
        self.build_spent.add_all(&cost);
        self.mem.wall_sent = Some(now);
        for (from, to) in runs {
            self.cmd(crew.clone(), CommandKind::PlaceWall { kind, from, to });
        }
    }

    /// The runs (from, to) of the wall line `from`-`to` of `kind` left once
    /// the segments that would close a ring on `movers` are taken out, each
    /// segment tested with those laid before it (`laid`, which collects
    /// the segments that will stand) as af_economy lays them, in order.
    fn open_wall_runs(&self, kind: BuildingTypeId, from: Vec2, to: Vec2, movers: &[Mover], laid: &mut Vec<Obb>) -> Vec<(Vec2, Vec2)> {
        let (data, me) = (self.view.data, self.view.me);
        let segments = wall_segments(data, kind, from, to);
        let seg = data.building(kind).size[0].max(0.5);
        let dir = (to - from).normalize_or_zero();
        let mut runs = Vec::new();
        let mut start = None;
        for (i, (pos, yaw)) in segments.iter().copied().enumerate() {
            let stands = check_placement(data, self.view.nav, self.view.spatial, me, kind, pos, yaw, false, laid).is_ok();
            let obb = Obb::from_size(pos, data.building(kind).size, yaw);
            if stands && seals_pocket(&self.view, movers, &[obb], laid) {
                if let Some(s) = start.take() {
                    runs.push((from + dir * seg * s as f32, from + dir * seg * i as f32));
                }
                continue;
            }
            if stands {
                laid.push(placed_footprint(data, kind, pos, yaw));
            }
            start.get_or_insert(i);
        }
        if let Some(s) = start {
            runs.push((from + dir * seg * s as f32, from + dir * seg * segments.len() as f32));
        }
        runs
    }

    /// Upgrade towers (one per think) and wall segments (a few per think)
    /// along their upgrade chains when the age allows, it is affordable and
    /// the army is at least half its target.
    pub(super) fn upgrade_defences(&mut self) {
        let (data, me, now) = (self.view.data, self.view.me, self.now());
        if self.army_thin() {
            return;
        }
        let snap = self.view.snap;
        let mut towers_done = false;
        let mut walls = Vec::new();
        for b in snap.buildings.iter().filter(|b| b.complete) {
            let wall = data.building(b.kind).wall;
            let room = if wall { walls.len() < WALL_UPGRADES } else { !towers_done && is_tower(data, me, b.kind) };
            if !room {
                continue;
            }
            if self.queue_busy(b) || self.mem.upgrade_sent.iter().any(|(e, _)| *e == b.entity) {
                continue;
            }
            if rules::can_upgrade(data, me, b.kind, false).is_err() {
                continue;
            }
            let cost = data.building(b.kind).upgrade.as_ref().map_or(ResAmounts::ZERO, |u| u.cost.amounts);
            if !self.budget.try_spend(&cost, KEEP_RESERVE) {
                continue;
            }
            self.mem.upgrade_sent.push((b.entity, now));
            self.busy.push(b.entity);
            if wall {
                walls.push(b.entity);
            } else {
                towers_done = true;
                self.cmd(vec![b.entity], CommandKind::UpgradeBuilding);
            }
        }
        self.cmd(walls, CommandKind::UpgradeBuilding);
    }
}

/// The two wings of a wall of `segments` segments of length `segment`
/// across the approach (`front`) to a hall at `home`: (from, to) of the
/// left wing, then of the right wing, with [`WALL_OPENING`] between them.
pub(crate) fn wall_line(home: Vec2, front: Vec2, hall_radius: f32, segment: f32, segments: u32) -> (Vec2, Vec2, Vec2, Vec2) {
    let centre = home + front * (hall_radius + WALL_DISTANCE);
    let side = front.perp();
    let wing = segment * (segments / 2).max(1) as f32;
    let inner = WALL_OPENING * 0.5;
    (centre - side * (inner + wing), centre - side * inner, centre + side * inner, centre + side * (inner + wing))
}
