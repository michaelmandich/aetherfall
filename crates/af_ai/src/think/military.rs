//! The military half of a think: defence, training, military research,
//! rally points, and the gather / attack / reinforce / retarget cycle.

use std::sync::Arc;

use af_core::ext::fog::team_sees_box;
use af_core::prelude::*;
use bevy::prelude::*;

use super::Think;
use crate::army::{
    Aim, Objective, choose_target, choose_target_for, filler_unit, is_breaker, is_breaker_vs, pick_unit, siege_targets, surplus,
    surplus_share, wanted_unit,
};
use crate::memory::AttackState;
use crate::progress::{is_eco_tech, tech_helps_unit};
use crate::view::{ArmyInfo, BuildingInfo, KnownBuilding};
use crate::{MAX_QUEUE, RESEND_DELAY};

/// Alerts this close to an own building trigger defence ...
const DEFEND_RADIUS: f32 = 30.0;
/// ... and so do alerts this close to the main hall (workers in the fields;
/// this and the other military radii marked so grow with the map, see
/// `AiView::map_scale`).
const HOME_RADIUS: f32 = 70.0;
/// Hostile army this close to the alert counts as the raid's strength.
const THREAT_RADIUS: f32 = 20.0;
/// A raid of at most this much army population (a scout or two) does not
/// hold a wave back, nor the age-up (see `Think::spend_all_on_army`).
pub(super) const POKE_POP: u32 = 4;
/// An attack is recalled when the raid outnumbers the defenders this much,
/// plus [`RECALL_MARGIN`] population (so a lone scout does not count).
const RECALL_RATIO: f32 = 1.5;
const RECALL_MARGIN: u32 = 3;
/// Hostile army this close to the attack group's centre is what it fights.
const BATTLE_RADIUS: f32 = 25.0;
/// An attack group that lost this share of what was sent, and faces more
/// than [`LOSING_ODDS`] times its own strength, falls back to regroup.
const LOSING_LOSS: f32 = 0.5;
const LOSING_ODDS: f32 = 1.3;
/// Defence stays active this long after the last alert.
const DEFEND_TIME: f32 = 20.0;
/// Army units this close to the alert or home respond (grows with the map).
const DEFEND_RANGE: f32 = 90.0;
/// Defenders are re-targeted at most this often.
const DEFEND_RESEND: f32 = 8.0;
/// An attack this far from home is recalled when nobody else can defend
/// (grows with the map).
const RECALL_DISTANCE: f32 = 60.0;
/// Idle army units farther than this from the rally point walk back to it.
const RALLY_RADIUS: f32 = 12.0;
/// Army units this close to the rally point count as gathered for an attack.
const GATHERED_RADIUS: f32 = 2.0 * RALLY_RADIUS;
/// An attacker this close to the objective has arrived.
const ARRIVE_RADIUS: f32 = 12.0;
/// The attack group must get this much closer to its goal (by the way
/// round: the route distance left, see [`Think::way_left`]) ...
const PROGRESS_STEP: f32 = 3.0;
/// ... within this many seconds, or it is stuck.
const STALL_TIME: f32 = 45.0;
/// A stuck group breaches a hostile wall this close to its front unit.
const WALL_SEARCH_RADIUS: f32 = 20.0;
/// A goal the group got stuck short of (with no wall to breach) is skipped this long.
const UNREACHABLE_TIME: f32 = 120.0;
/// After a retreat or recall, the next attack waits this long.
const RETREAT_COOLDOWN: f32 = 60.0;
/// An attack group below this share of what was sent (reinforcements
/// included) retreats.
const RETREAT_SHARE: f32 = 0.3;
/// Reinforcements leave the rally point in groups of this size, or alone
/// after [`REINFORCE_INTERVAL`].
const REINFORCE_GROUP: usize = 3;
/// A faction keeps up to this share of its army home instead of sending it
/// after an attack, scaled down by its aggression (none for a pure rusher).
const HOME_GUARD_SHARE: f32 = 0.4;
const REINFORCE_INTERVAL: f32 = 30.0;
/// An attack group whose front comes this close to a building goal ...
const RAZE_RADIUS: f32 = 30.0;
/// ... and faces no more hostile army there than this share of its own
/// strength on the spot (and never less than [`POKE_POP`]) razes the town
/// (see [`Think::raze`]) ...
const RAZE_ODDS: f32 = 0.25;
/// ... until the hostile army there grows past this share of it.
const RAZE_QUIT_ODDS: f32 = 0.5;
/// Breakers crowding round one building get in each other's way: each takes
/// its width plus this much (metres) of the building's perimeter, and those
/// that find no room go for the owner's next building near the goal.
const BREAKER_GAP: f32 = 1.2;
/// ... and the soldiers that cannot tear a building down wait this far
/// (metres) beyond the goal's walls, out of the breakers' way, when no
/// worker is in sight to hunt (see [`Think::guard_spot`]).
const GUARD_DISTANCE: f32 = 6.0;
/// While razing, the soldiers that are no building breakers hunt the
/// hostile workers in sight this close to the goal: a town without workers
/// cannot rebuild.
const PREY_RADIUS: f32 = 45.0;
/// While razing, the home guard shrinks to this share of its usual size:
/// the realm being razed cannot strike back.
const RAZE_GUARD_SHARE: f32 = 0.5;
/// A building breaker wanted while razing is saved for (see
/// [`Think::train_army`]) once it is this much population below its share.
const HOLD_DEFICIT: f32 = 1.0;
/// Military techs start once the army has this much population (or age 3).
const MIL_TECH_ARMY_POP: u32 = 10;
/// A military tech must improve at least this much army population.
const MIL_TECH_MIN_BENEFIT: u32 = 4;
/// Surplus (see `army::surplus`) worth looking for something to spend on.
const SURPLUS_SINK: f32 = 150.0;

impl Think<'_, '_> {
    /// React to "under attack" alerts near own buildings or the main hall:
    /// send the army that is not attacking, and recall the attack when the
    /// raid outnumbers whoever is home. Whoever attacked is remembered (see
    /// `AiMemory::grudges`).
    pub(super) fn defend(&mut self) {
        let now = self.now();
        let alerts = std::mem::take(&mut self.mem.alerts);
        for a in &alerts {
            if let Some(p) = a.attacker_owner {
                self.mem.note_grudge(p, a.time);
            }
        }
        let snap = self.view.snap;
        let home = self.plan.home;
        let scale = self.view.map_scale();
        let near_base = |at: Vec2| at.distance(home) < HOME_RADIUS * scale || snap.buildings.iter().any(|b| b.pos.distance(at) < DEFEND_RADIUS);
        let fresh = alerts.iter().rev().find(|a| near_base(a.at)).copied();
        if let Some(a) = fresh {
            self.mem.defend_pos = a.attacker;
            self.mem.defend_until = now + DEFEND_TIME;
        }
        self.defending = now < self.mem.defend_until;
        if !self.defending {
            return;
        }
        let at = self.mem.defend_pos;
        let responders: Vec<Entity> = snap
            .army
            .iter()
            .filter(|a| {
                !self.mem.is_attacker(a.entity)
                    && (a.pos.distance(at) < DEFEND_RANGE * scale || a.pos.distance(home) < DEFEND_RANGE * scale)
                    && !self.mem.moved_within(a.entity, now, DEFEND_RESEND)
            })
            .map(|a| a.entity)
            .collect();
        let defenders: u32 = snap.army.iter().filter(|a| !self.mem.is_attacker(a.entity)).map(|a| a.pop).sum();
        if !responders.is_empty() {
            self.move_army(responders, CommandKind::AttackMove(at));
        }
        let threat = self.hostile_army_near(at, THREAT_RADIUS);
        // A scout poking at an empty base is no reason to call the army home.
        let outnumbered = threat as f32 > defenders as f32 * RECALL_RATIO + RECALL_MARGIN as f32;
        if fresh.is_some()
            && outnumbered
            && let AttackState::Attacking { goal, .. } = self.mem.attack
            && goal.pos.distance(home) > RECALL_DISTANCE * scale
        {
            let group = std::mem::take(&mut self.mem.attackers);
            self.mem.attack = AttackState::Gathering;
            self.mem.next_attack_after = now + RETREAT_COOLDOWN;
            self.move_army(group, CommandKind::AttackMove(at));
        }
    }

    /// Hostile army population about the spot defended lately (see
    /// [`Think::defend`]): what the raid on the base fields.
    pub(super) fn raid_strength(&self) -> u32 {
        self.hostile_army_near(self.mem.defend_pos, THREAT_RADIUS)
    }

    /// Hostile army population (workers aside) the team sees within
    /// `radius` of `at`. Units in the fog do not count: what the AI does
    /// not see, it cannot weigh (the box test of `ext::fog`'s sight
    /// convention, as for every other sighting).
    fn hostile_army_near(&self, at: Vec2, radius: f32) -> u32 {
        let (data, me, players) = (self.view.data, self.view.me.id, self.view.players);
        let (fog, team) = (self.view.fog, self.view.me.team);
        let mut pop = 0;
        self.view.spatial.for_each_near(at, radius, |e| {
            let hostile = e.owner.is_some_and(|o| players.hostile(me, o));
            if e.kind == SpatialKind::Unit
                && hostile
                && !e.tags.contains(Tag::Worker)
                && e.pos.distance(at) <= radius
                && team_sees_box(fog, team, &e.obb)
            {
                pop += data.unit(UnitTypeId(e.type_index)).pop;
            }
        });
        pop
    }

    /// Queue army units at production buildings from the composition, the
    /// building whose wanted unit (see `army::wanted_unit`) is furthest below
    /// its share first. When a building cannot afford its wanted unit, what
    /// that unit lacks is remembered as a shortfall, so that gathering turns
    /// to it (an army waiting on iron gets iron gatherers, one waiting for
    /// rams wood and copper). While the army razes (see
    /// `AiMemory::razing_lately`), the first building breaker wanted at
    /// least [`HOLD_DEFICIT`] below its share has its price held back from
    /// the rest of the think, so that cheap units bought first do not eat
    /// the saving for the breakers that tear the town down (archers instead
    /// of rams). Nothing is held back in defence. While the army is short of
    /// its goal, a building the mix trains nothing at (one raised in an
    /// earlier age for the stand-ins of `army::army_composition`, say)
    /// trains a [filler](filler_unit) rather than standing idle.
    pub(super) fn train_army(&mut self) {
        let (data, me) = (self.view.data, self.view.me);
        if self.plan.comp.is_empty() {
            return;
        }
        let share = self.mil_share();
        let short = self.army_short();
        let workers_short = self.plan.workers + self.plan.workers_queued < self.plan.worker_target;
        let snap = self.view.snap;
        let room = |queued: u32| me.pop_cap.min(POP_HARD_CAP).saturating_sub(me.pop_used + queued);
        let mut free: Vec<(f32, &BuildingInfo)> = Vec::new();
        for b in &snap.buildings {
            let k = b.kind.index();
            if !b.complete || !self.plan.trains_army[k] || self.queue_busy(b) || snap.queue(b).len() >= MAX_QUEUE {
                continue;
            }
            // A hall that also trains soldiers makes workers first.
            if self.plan.trains_worker[k] && workers_short {
                continue;
            }
            match wanted_unit(&self.view, &self.plan.comp, &self.plan.pop_by_kind, b.kind, room(self.queued_pop)) {
                Some((_, deficit)) => free.push((deficit, b)),
                // Nothing of the mix trained here: fillers only, after the rest.
                None if short => free.push((f32::NEG_INFINITY, b)),
                None => {}
            }
        }
        free.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.entity.cmp(&b.1.entity)));
        let razing = self.mem.razing_lately(self.now());
        let targets = if razing { siege_targets(data, &self.mem.known, true) } else { Vec::new() };
        let mut held = self.defending || targets.is_empty();
        for (_, b) in free {
            let room = room(self.queued_pop);
            if room == 0 {
                return;
            }
            let wanted = wanted_unit(&self.view, &self.plan.comp, &self.plan.pop_by_kind, b.kind, room);
            let breaker = wanted.is_some_and(|(w, _)| is_breaker_vs(data, w, &targets));
            let mut pick = pick_unit(&self.view, &self.plan.comp, &self.plan.pop_by_kind, b.kind, &self.budget, share, room);
            if let Some((w, deficit)) = wanted
                && pick != Some(w)
            {
                let cost = stats::unit_cost(data, me, w);
                if !self.budget.affords(&cost, share) {
                    self.note_shortfall(&cost, share);
                    if !held && deficit >= HOLD_DEFICIT && breaker {
                        // Save for it: what is left must pay for anything else.
                        held = true;
                        self.budget.spend(&cost);
                        pick = pick_unit(&self.view, &self.plan.comp, &self.plan.pop_by_kind, b.kind, &self.budget, share, room);
                    }
                }
            }
            if wanted.is_none() && short {
                pick = filler_unit(&self.view, b.kind, &self.budget, share, room);
            }
            let Some(u) = pick else { continue };
            let d = data.unit(u);
            let cost = stats::unit_cost(data, me, u);
            self.budget.spend(&cost);
            self.army_spent.add_all(&cost);
            self.queued_pop += d.pop * d.squad;
            self.plan.pop_by_kind[u.index()] += d.pop * d.squad;
            self.cmd(vec![b.entity], CommandKind::Train { unit: u, count: 1 });
        }
    }

    /// Research the tech that improves the most fielded army population.
    pub(super) fn military_techs(&mut self) {
        let (data, me) = (self.view.data, self.view.me);
        if self.plan.army_pop < MIL_TECH_ARMY_POP && me.age < 3 {
            return;
        }
        // Research never touches the age-up reserve.
        let share = 1.0;
        let fielded: Vec<(UnitTypeId, u32)> =
            data.unit_ids().filter_map(|u| self.plan.pop_by_kind.get(u.index()).filter(|p| **p > 0).map(|p| (u, *p))).collect();
        let snap = self.view.snap;
        let mut best: Option<(u32, TechId, Entity)> = None;
        for b in snap.buildings.iter().filter(|b| b.complete && !self.queue_busy(b)) {
            for t in rules::research_menu(data, me, b.kind) {
                let def = data.tech(t);
                if def.is_age_up()
                    || is_eco_tech(def)
                    || self.mem.research_recent(t)
                    || rules::can_research(data, me, b.kind, t, false).is_err()
                {
                    continue;
                }
                let benefit: u32 = fielded.iter().filter(|(u, _)| tech_helps_unit(data, def, *u)).map(|(_, p)| p).sum();
                if benefit >= MIL_TECH_MIN_BENEFIT
                    && best.is_none_or(|(bb, ..)| benefit > bb)
                    && self.budget.affords(&def.cost.amounts, share)
                {
                    best = Some((benefit, t, b.entity));
                }
            }
        }
        if let Some((_, t, b)) = best {
            self.budget.spend(&data.tech(t).cost.amounts);
            self.research(b, t);
        }
    }

    /// Spend what piles up beyond the reserve (see [`surplus`]) and that the
    /// other steps left: one research per think that the surplus pays for
    /// entirely, at a building with an empty queue (the most useful first:
    /// the most fielded population helped, then economy techs, then
    /// anything), and one tower beyond the usual number that the surplus
    /// pays for entirely (the dearest such: late towers eat stone, iron,
    /// silicon and aether).
    pub(super) fn spend_surplus(&mut self) {
        let (data, me) = (self.view.data, self.view.me);
        let piled = surplus(&self.budget);
        if piled.total() < SURPLUS_SINK {
            return;
        }
        let fielded: Vec<(UnitTypeId, u32)> =
            data.unit_ids().filter_map(|u| self.plan.pop_by_kind.get(u.index()).filter(|p| **p > 0).map(|p| (u, *p))).collect();
        let snap = self.view.snap;
        let mut best: Option<(u32, TechId, Entity)> = None;
        for b in snap.buildings.iter().filter(|b| b.complete && snap.queue(b).is_empty() && !self.busy.contains(&b.entity)) {
            for t in rules::research_menu(data, me, b.kind) {
                let def = data.tech(t);
                if def.is_age_up()
                    || self.mem.research_recent(t)
                    || surplus_share(&def.cost.amounts, &piled) < 0.999
                    || rules::can_research(data, me, b.kind, t, false).is_err()
                {
                    continue;
                }
                let helped: u32 = fielded.iter().filter(|(u, _)| tech_helps_unit(data, def, *u)).map(|(_, p)| p).sum();
                let worth = 2 * helped + u32::from(is_eco_tech(def));
                if best.is_none_or(|(w, ..)| worth > w) {
                    best = Some((worth, t, b.entity));
                }
            }
        }
        if let Some((_, t, b)) = best {
            self.budget.spend(&data.tech(t).cost.amounts);
            self.research(b, t);
        }
        self.surplus_tower();
    }

    /// Point army production buildings (not worker halls) whose rally is
    /// elsewhere at the rally point.
    pub(super) fn rallies(&mut self) {
        let (rally, now) = (self.plan.rally, self.now());
        let mut list = Vec::new();
        for b in &self.view.snap.buildings {
            let k = b.kind.index();
            if !b.complete || !self.plan.trains_army[k] || self.plan.trains_worker[k] {
                continue;
            }
            let set = matches!(b.rally, Some(RallyTarget::Point(p)) if p.distance(rally) < 2.0);
            if set || self.mem.rally_sent.iter().any(|(e, _)| *e == b.entity) {
                continue;
            }
            self.mem.rally_sent.push((b.entity, now));
            list.push(b.entity);
        }
        self.cmd(list, CommandKind::SetRally { point: rally, target: None });
    }

    /// Gather at the rally point and launch attacks, or press the current one.
    pub(super) fn army(&mut self) {
        match self.mem.attack {
            AttackState::Gathering => self.gather_and_launch(),
            AttackState::Attacking { goal, since, best_dist, progress_at, razing } => {
                self.press_attack(goal, since, best_dist, progress_at, razing)
            }
        }
    }

    /// Launch with the army gathered at the rally point once it is big
    /// enough (see `Plan::wave_size`), unless a real raid is on (a scout's
    /// poke does not count); units still on their way follow later as
    /// reinforcements.
    fn gather_and_launch(&mut self) {
        let (me, now, rally) = (self.view.me, self.now(), self.plan.rally);
        if self.defending && self.raid_strength() > POKE_POP {
            return;
        }
        self.collect_at_rally();
        let gathered: Vec<_> = self
            .view
            .snap
            .army
            .iter()
            .filter(|a| !self.mem.is_attacker(a.entity) && a.pos.distance(rally) <= GATHERED_RADIUS)
            .collect();
        let pop: u32 = gathered.iter().map(|a| a.pop).sum();
        if now < self.mem.next_attack_after || pop < self.plan.wave_size {
            return;
        }
        let Some(goal) = choose_target(&self.view, self.mem, self.plan.home) else { return };
        let group: Vec<Entity> = gathered.iter().map(|a| a.entity).collect();
        info!(
            target: "af_ai::waves",
            "t={:.0} P{} [{}] age {} launches a wave of {} pop at P{} ({})",
            now,
            me.id.0,
            self.view.faction().id,
            me.age,
            pop,
            goal.player.0,
            aim_label(goal.aim)
        );
        for u in &group {
            self.mem.add_attacker(*u);
        }
        self.mem.attack = AttackState::attacking(goal, now, now);
        self.mem.note_target(goal.player);
        self.mem.last_wave = Some(now);
        self.mem.wave_pop = pop;
        self.mem.last_reinforce = now;
        self.move_army(group, CommandKind::AttackMove(goal.pos));
    }

    /// Idle army units away from the rally point walk (fighting) back to it.
    fn collect_at_rally(&mut self) {
        let (now, rally) = (self.now(), self.plan.rally);
        let strays: Vec<Entity> = self
            .view
            .snap
            .army
            .iter()
            .filter(|a| {
                a.idle
                    && !self.mem.is_attacker(a.entity)
                    && a.pos.distance(rally) > RALLY_RADIUS
                    && !self.mem.moved_within(a.entity, now, 3.0 * RESEND_DELAY)
            })
            .map(|a| a.entity)
            .collect();
        self.move_army(strays, CommandKind::AttackMove(rally));
    }

    /// Keep the attack going: retreat when it is spent (unless it is razing
    /// a town nobody defends); retarget when the goal is gone (or its owner
    /// defeated), reached (a start position found empty, a spot searched) or
    /// no longer worth it (a building of the player hunted for came to
    /// light); break through a wall or give the goal up when the group stops
    /// getting closer; raze the town once what defended it is beaten (see
    /// [`Think::raze`]), else push stopped attackers on; and send
    /// reinforcements.
    fn press_attack(&mut self, goal: Objective, since: f32, best_dist: f32, progress_at: f32, was_razing: bool) {
        let (me, now) = (self.view.me, self.now());
        let snap = self.view.snap;
        let group: Vec<ArmyInfo> = self.mem.attackers.iter().filter_map(|e| snap.army_unit(*e).copied()).collect();
        // A remnant unit hunted down (see `AiMemory::remnants`) is followed
        // wherever it runs; progress is measured from where it went, so one
        // that cannot be closed in on is given up like any goal out of reach.
        let (mut goal, mut best_dist) = (goal, best_dist);
        if let Aim::Remnant(unit) = goal.aim
            && let Some(pos) = self.mem.remnant(unit)
            && pos.distance(goal.pos) > ARRIVE_RADIUS
        {
            goal.pos = pos;
            let straight = group.iter().map(|a| a.pos.distance(pos)).fold(f32::INFINITY, f32::min);
            best_dist = self.way_left(&group, pos).unwrap_or(straight);
            self.mem.attack = AttackState::Attacking { goal, since, best_dist, progress_at, razing: false };
            self.move_army(group.iter().map(|a| a.entity).collect(), CommandKind::AttackMove(pos));
        }
        let group_pop: u32 = group.iter().map(|a| a.pop).sum();
        let front = group.iter().min_by(|a, b| a.pos.distance_squared(goal.pos).total_cmp(&b.pos.distance_squared(goal.pos))).copied();
        let closest = front.map_or(f32::INFINITY, |a| a.pos.distance(goal.pos));
        // Progress is the way left, not the straight line: a group going
        // round by a ford gets closer all the way.
        let left = self.way_left(&group, goal.pos).unwrap_or(closest);
        let mut razing = self.should_raze(goal, &group, front.map(|a| a.pos), was_razing);
        let spent = (group_pop as f32) < self.mem.wave_pop as f32 * RETREAT_SHARE && now - since > 20.0;
        if (spent && !razing) || self.losing(&group, group_pop) {
            let units = std::mem::take(&mut self.mem.attackers);
            self.mem.attack = AttackState::Gathering;
            self.mem.next_attack_after = now + RETREAT_COOLDOWN;
            self.cmd(units, CommandKind::Move(self.plan.rally));
            return;
        }
        if was_razing && !razing {
            // A defending army showed up: everyone fights it (breakers drop their buildings).
            self.mem.raze_focus = None;
            self.mem.raze_sent.clear();
            self.move_army(group.iter().map(|a| a.entity).collect(), CommandKind::AttackMove(goal.pos));
        }
        let arrived = closest < ARRIVE_RADIUS;
        let defeated = self.view.players.try_get(goal.player).is_none_or(|p| p.defeated);
        let lost = defeated
            || match goal.aim {
                Aim::Building(e) => !self.mem.knows(e),
                // Hunting blind while one of the player's buildings is known
                // (and was not given up as out of reach).
                Aim::Start | Aim::Search => {
                    arrived || self.mem.known.iter().any(|k| k.owner == goal.player && !self.mem.is_unreachable(k.pos))
                }
                // Dead, or its owner has a building again.
                Aim::Remnant(unit) => self.mem.remnant(unit).is_none(),
            };
        if lost {
            match goal.aim {
                Aim::Start if arrived && !self.mem.visited.contains(&goal.player) => self.mem.visited.push(goal.player),
                Aim::Search if arrived => self.mem.note_searched(goal.pos, now),
                _ => {}
            }
            // Razing goes on at the next building in reach: the breakers are
            // sent there at once, the rest follow when they stop.
            let keep_razing = razing && !defeated;
            let Some(next) = self.retarget(&group, since, goal.player, keep_razing) else { return };
            if matches!(next.aim, Aim::Search | Aim::Remnant(_)) || next.player != goal.player {
                info!(
                    target: "af_ai::hunt",
                    "t={:.0} P{} wave of {} pop retargets to P{} ({}) at {:.0}",
                    now,
                    me.id.0,
                    group_pop,
                    next.player.0,
                    aim_label(next.aim),
                    next.pos
                );
            }
            razing = keep_razing && next.building().is_some() && front.is_some_and(|a| a.pos.distance(next.pos) < RAZE_RADIUS);
            goal = next;
        } else if arrived || left < best_dist - PROGRESS_STEP {
            self.mem.attack = AttackState::Attacking { goal, since, best_dist: left.min(best_dist), progress_at: now, razing };
        } else if now - progress_at > STALL_TIME {
            // No closer for a long time: break through a wall in the way, or give the goal up.
            match front.and_then(|a| self.hostile_wall_near(a.pos)) {
                Some(wall) => {
                    self.mem.attack = AttackState::Attacking { goal, since, best_dist, progress_at: now, razing };
                    self.move_army(group.iter().map(|a| a.entity).collect(), CommandKind::Attack(wall));
                }
                None => {
                    self.mem.unreachable.push((goal.pos, now + UNREACHABLE_TIME));
                    if let Aim::Remnant(unit) = goal.aim {
                        // It runs where the group cannot follow: hunt the others first.
                        self.mem.lost_prey.push((unit, now + UNREACHABLE_TIME));
                    }
                    let Some(next) = self.retarget(&group, since, goal.player, false) else { return };
                    razing = false;
                    goal = next;
                }
            }
        }
        if let AttackState::Attacking { razing: r, .. } = &mut self.mem.attack {
            *r = razing;
        }
        if razing {
            self.mem.last_razing = Some(now);
            self.mem.raze_victim = Some(goal.player);
            self.raze(&group, goal);
        } else {
            // Attackers that stopped: strike the target building, or push on.
            let (mut near, mut far) = (Vec::new(), Vec::new());
            for a in group.iter().filter(|a| a.idle && !self.mem.moved_within(a.entity, now, RESEND_DELAY)) {
                if goal.building().is_some() && a.pos.distance(goal.pos) < ARRIVE_RADIUS {
                    near.push(a.entity);
                } else {
                    far.push(a.entity);
                }
            }
            if let Some(b) = goal.building() {
                self.move_army(near, CommandKind::Attack(b));
            }
            self.move_army(far, CommandKind::AttackMove(goal.pos));
        }
        if !self.defending {
            self.reinforce(goal.pos, razing);
        }
    }

    /// The route distance left to `goal` for the attack group: the least
    /// over its units, by the way of the unit most of the army is (one
    /// search from the goal, kept in `AiMemory::goal_field` while the goal
    /// stays). Infinite when none of them can get there; `None` without
    /// regions (then the straight line counts).
    fn way_left(&mut self, group: &[ArmyInfo], goal: Vec2) -> Option<f32> {
        let stamp = self.view.regions.map_or(0, |r| r.stamp());
        if !self.mem.goal_field.as_ref().is_some_and(|(g, s, _)| *g == goal && *s == stamp) {
            self.mem.goal_field = self.view.route_field(self.view.lead_unit(), goal).map(|f| (goal, stamp, Arc::new(f)));
        }
        let field = &self.mem.goal_field.as_ref()?.2;
        Some(group.iter().filter_map(|a| field.get(a.pos, ARRIVE_RADIUS)).fold(f32::INFINITY, f32::min))
    }

    /// Should the attack group raze the town of its building goal? Its front
    /// unit (at `front`) must stand within [`RAZE_RADIUS`] of the goal, and
    /// the hostile army about the front and the goal must be weak against
    /// the group's units on the spot: at most [`RAZE_ODDS`] of them to start
    /// (`razing` false), [`RAZE_QUIT_ODDS`] to go on.
    fn should_raze(&self, goal: Objective, group: &[ArmyInfo], front: Option<Vec2>, razing: bool) -> bool {
        let Some(front) = front.filter(|f| goal.building().is_some() && f.distance(goal.pos) <= RAZE_RADIUS) else { return false };
        let reach = RAZE_RADIUS + BATTLE_RADIUS;
        let present: u32 = group.iter().filter(|a| a.pos.distance(goal.pos) <= reach).map(|a| a.pop).sum();
        let threat = self.hostile_army_near(front, BATTLE_RADIUS).max(self.hostile_army_near(goal.pos, BATTLE_RADIUS));
        let odds = if razing { RAZE_QUIT_ODDS } else { RAZE_ODDS };
        present > 0 && threat as f32 <= (present as f32 * odds).max(POKE_POP as f32)
    }

    /// Tear the town down, the goal first: building breakers on the spot
    /// (see `army::is_breaker`) strike it, as many as fit round it (see
    /// [`BREAKER_GAP`]), the others its owner's buildings nearest it (within
    /// [`RAZE_RADIUS`]); each is sent once and again whenever it stops.
    /// Everyone else that stops attack-moves to the nearest hostile worker
    /// in sight (see [`PREY_RADIUS`]), else to a spot beside the goal (see
    /// [`Think::guard_spot`]), so that archers shoot whatever defends the
    /// town and its workers first and pick at buildings otherwise without
    /// crowding the breakers. Breakers still on their way attack-move like
    /// the rest.
    fn raze(&mut self, group: &[ArmyInfo], goal: Objective) {
        let (data, now) = (self.view.data, self.now());
        let Some(target) = goal.building() else { return };
        let Some(kind) = self.mem.known_building(target).map(|k| k.kind) else { return };
        if self.mem.raze_focus != Some(target) {
            self.mem.raze_focus = Some(target);
            self.mem.raze_sent.clear();
        }
        // Buildings to tear down and the perimeter (metres) left round each.
        let perimeter = |k: BuildingTypeId| {
            let s = data.building(k).size;
            2.0 * (s[0] + s[2])
        };
        let mut near: Vec<&KnownBuilding> = self
            .mem
            .known
            .iter()
            .filter(|k| k.owner == goal.player && k.entity != target && k.pos.distance(goal.pos) <= RAZE_RADIUS)
            .collect();
        near.sort_by(|a, b| a.pos.distance_squared(goal.pos).total_cmp(&b.pos.distance_squared(goal.pos)).then(a.entity.cmp(&b.entity)));
        let mut sites: Vec<(Entity, BuildingTypeId, f32)> = vec![(target, kind, perimeter(kind))];
        sites.extend(near.iter().map(|k| (k.entity, k.kind, perimeter(k.kind))));
        let mut breakers: Vec<&ArmyInfo> = group
            .iter()
            .filter(|a| a.pos.distance(goal.pos) <= RAZE_RADIUS + ARRIVE_RADIUS && is_breaker(data, a.kind, kind))
            .collect();
        breakers.sort_by(|a, b| a.pos.distance_squared(goal.pos).total_cmp(&b.pos.distance_squared(goal.pos)).then(a.entity.cmp(&b.entity)));
        let mut on_spot: Vec<Entity> = breakers.iter().map(|a| a.entity).collect();
        on_spot.sort();
        let halted: Vec<Entity> = group.iter().filter(|a| a.idle && !self.mem.moved_within(a.entity, now, RESEND_DELAY)).map(|a| a.entity).collect();
        let stopped = |a: &ArmyInfo| halted.binary_search(&a.entity).is_ok();
        // Breakers busy at a building keep their place round it ...
        let mut free = Vec::new();
        for a in breakers {
            let width = data.unit(a.kind).size[0] + BREAKER_GAP;
            let site = self.mem.raze_target(a.entity).and_then(|t| sites.iter_mut().find(|s| s.0 == t));
            match site {
                Some(site) if !stopped(a) => site.2 -= width,
                _ => free.push((a, width)),
            }
        }
        // ... the others go where there is room, the goal first.
        let mut sends: Vec<(Entity, Entity)> = Vec::new();
        for (a, width) in free {
            if self.mem.raze_target(a.entity).is_some() && !stopped(a) {
                continue;
            }
            let site = sites.iter_mut().find(|s| s.2 >= width && is_breaker(data, a.kind, s.1));
            let to = site.map_or(target, |s| {
                s.2 -= width;
                s.0
            });
            sends.push((to, a.entity));
        }
        sends.sort();
        for (to, unit) in &sends {
            self.mem.note_raze_sent(*unit, *to);
        }
        for chunk in sends.chunk_by(|a, b| a.0 == b.0) {
            self.move_army(chunk.iter().map(|s| s.1).collect(), CommandKind::Attack(chunk[0].0));
        }
        let others: Vec<Entity> = group.iter().filter(|a| stopped(a) && on_spot.binary_search(&a.entity).is_err()).map(|a| a.entity).collect();
        if !others.is_empty() {
            let spot = self.hostile_worker_near(goal.pos, PREY_RADIUS).unwrap_or_else(|| self.guard_spot(group, goal, kind));
            self.move_army(others, CommandKind::AttackMove(spot));
        }
    }

    /// Where the soldiers that cannot tear a building down wait while
    /// breakers strike the goal (of type `kind`): [`GUARD_DISTANCE`] beyond
    /// its walls on the side the group stands, out of the breakers' way.
    fn guard_spot(&self, group: &[ArmyInfo], goal: Objective, kind: BuildingTypeId) -> Vec2 {
        let size = self.view.data.building(kind).size;
        let reach = Vec2::new(size[0], size[2]).length() * 0.5 + GUARD_DISTANCE;
        let centre = if group.is_empty() { self.plan.home } else { group.iter().map(|a| a.pos).sum::<Vec2>() / group.len() as f32 };
        let toward = (centre - goal.pos).normalize_or((self.plan.home - goal.pos).normalize_or(Vec2::Y));
        self.view.clamp(goal.pos + toward * reach, 2.0)
    }

    /// The nearest hostile worker the team sees within `radius` of `at`.
    fn hostile_worker_near(&self, at: Vec2, radius: f32) -> Option<Vec2> {
        let (me, players, fog, team) = (self.view.me.id, self.view.players, self.view.fog, self.view.me.team);
        self.view
            .spatial
            .nearest(at, radius, |e| {
                e.kind == SpatialKind::Unit
                    && e.tags.contains(Tag::Worker)
                    && e.owner.is_some_and(|o| players.hostile(me, o))
                    && fog.visible(team, e.pos)
            })
            .map(|e| e.pos)
    }

    /// The attack group has lost [`LOSING_LOSS`] of what was sent and the
    /// hostile army around it is more than [`LOSING_ODDS`] times stronger.
    fn losing(&self, group: &[ArmyInfo], group_pop: u32) -> bool {
        if group.is_empty() || (group_pop as f32) > self.mem.wave_pop as f32 * (1.0 - LOSING_LOSS) {
            return false;
        }
        let centre = group.iter().map(|a| a.pos).sum::<Vec2>() / group.len() as f32;
        self.hostile_army_near(centre, BATTLE_RADIUS) as f32 > group_pop as f32 * LOSING_ODDS
    }

    /// Aim the attack group at the next target from where it stands
    /// (preferring more of `current`, the player just attacked), or bring it
    /// home when nothing is left to hit or search. Returns the new goal.
    /// While `razing` a town, a new goal in it is not ordered:
    /// [`Think::raze`] sends the breakers on, and the rest follow when they
    /// stop.
    fn retarget(&mut self, group: &[ArmyInfo], since: f32, current: PlayerId, razing: bool) -> Option<Objective> {
        let now = self.now();
        let centre = if group.is_empty() { self.plan.home } else { group.iter().map(|a| a.pos).sum::<Vec2>() / group.len() as f32 };
        match choose_target_for(&self.view, self.mem, centre, Some(current)) {
            Some(goal) => {
                self.mem.attack = AttackState::attacking(goal, since, now);
                let in_town = razing && goal.building().is_some() && centre.distance(goal.pos) < RAZE_RADIUS + BATTLE_RADIUS;
                if !in_town {
                    self.move_army(group.iter().map(|a| a.entity).collect(), CommandKind::AttackMove(goal.pos));
                }
                Some(goal)
            }
            None => {
                self.mem.visited.clear();
                let units = std::mem::take(&mut self.mem.attackers);
                self.mem.attack = AttackState::Gathering;
                self.mem.next_attack_after = now + REINFORCE_INTERVAL;
                self.move_army(units, CommandKind::AttackMove(self.plan.rally));
                None
            }
        }
    }

    /// The nearest hostile wall or gate the team sees within
    /// [`WALL_SEARCH_RADIUS`] of `pos` (part of its box in sight, or its
    /// owner revealed, as `SeenBy::visible` has it). Units never pick walls
    /// on their own, so a walled base has to be breached on purpose; a wall
    /// in the fog is no more known to the AI than to a human.
    fn hostile_wall_near(&self, pos: Vec2) -> Option<Entity> {
        let (me, players) = (self.view.me.id, self.view.players);
        let (fog, team) = (self.view.fog, self.view.me.team);
        self.view
            .spatial
            .nearest(pos, WALL_SEARCH_RADIUS, |e| {
                e.kind == SpatialKind::Building
                    && e.tags.contains(Tag::Wall)
                    && e.owner.is_some_and(|o| players.hostile(me, o) && (team_sees_box(fog, team, &e.obb) || self.view.is_revealed(o)))
            })
            .map(|e| e.entity)
    }

    /// Send idle units from home to the attack, in small groups, keeping a
    /// home guard (see [`HOME_GUARD_SHARE`]; [`RAZE_GUARD_SHARE`] of it while
    /// `razing`).
    fn reinforce(&mut self, to: Vec2, razing: bool) {
        let now = self.now();
        let snap = self.view.snap;
        let home: u32 = snap.army.iter().filter(|a| !self.mem.is_attacker(a.entity)).map(|a| a.pop).sum();
        let share = HOME_GUARD_SHARE * (1.0 - self.view.aggression()) * if razing { RAZE_GUARD_SHARE } else { 1.0 };
        let guard = (self.plan.army_pop as f32 * share).round() as u32;
        let mut spare = home.saturating_sub(guard);
        let mut fresh = Vec::new();
        for a in snap.army.iter().filter(|a| a.idle && !self.mem.is_attacker(a.entity)) {
            if a.pop > spare {
                break;
            }
            spare -= a.pop;
            fresh.push(a.entity);
        }
        if fresh.len() >= REINFORCE_GROUP || (!fresh.is_empty() && now - self.mem.last_reinforce > REINFORCE_INTERVAL) {
            for u in &fresh {
                self.mem.add_attacker(*u);
                self.mem.wave_pop += self.view.snap.army_unit(*u).map_or(0, |a| a.pop);
            }
            self.mem.last_reinforce = now;
            self.move_army(fresh, CommandKind::AttackMove(to));
        }
    }

    /// Order army units and remember when they were moved.
    fn move_army(&mut self, units: Vec<Entity>, kind: CommandKind) {
        let now = self.now();
        for u in &units {
            self.mem.note_moved(*u, now);
        }
        self.cmd(units, kind);
    }
}

/// How an aim reads in the AI logs.
pub(super) fn aim_label(aim: Aim) -> &'static str {
    match aim {
        Aim::Building(_) => "known building",
        Aim::Start => "start position",
        Aim::Search => "search",
        Aim::Remnant(_) => "remnant",
    }
}
