//! One think of one AI player: run every decision against the view and
//! turn the results into `PlayerCommand`s, spending a running budget.
//!
//! Order matters: defence first, then the age-up (it may claim the main
//! hall's queue), workers, housing, the economy (after army production
//! while the army is short or its first building is due), fortifications, the army,
//! research, what piles up, and finally the waves (see [`military`] and
//! [`fortify`]).

mod fortify;
mod military;

use af_core::prelude::*;
use bevy::prelude::*;

use crate::budget::Budget;
use crate::build::{SpotQuery, builders_for, find_build_spot, pick_workers};
use crate::economy::{
    FIELD_RING, FieldSpot, Sources, Supply, WORKERS_PER_NODE, apportion, desired_gatherers, drop_site_plan, field_kind, field_spot,
    house_kind, needs_house, search_remote,
};
use crate::lanes::{ground_movers, seals_pocket};
use crate::memory::{AiMemory, BAD_SPOT_TIME, PendingBuild, RemoteSearch, SiteWatch, WorkerOrder};
use crate::plan::Plan;
use crate::army::{best_breaker, siege_targets};
use crate::progress::{
    AgeUpStep, cost_total, eco_tech_useful, first_production_building, is_eco_tech, max_production, missing_trainer, nearest_free_node,
    next_production_building, node_claimers, production_targets, vent_target,
};
use crate::view::{AiView, BuildingInfo, WorkerInfo, WorkerTask};
use crate::{MAX_QUEUE, REORDER_DELAY};

/// Share of the age-up reserve that economy purchases leave alone: housing
/// that is not urgent, drop sites and fields pay for themselves, so they may
/// borrow half of the saving; so may economy techs, vents, production up to
/// the age's targets and the first tower until the age-up falls behind
/// schedule (see [`Think::invest_share`]). The age-up's own step, workers up
/// to the age's target (until the saving is all but complete, see
/// [`Think::age_up_near`], or past half the target once the age-up is behind
/// schedule), urgent housing, the first army building (see
/// [`Think::first_production_due`]), a lost town centre, the army in the
/// cases of [`Think::spend_all_on_army`] and, against the last enemy, the
/// workshop of the breakers that raze its town may spend all of it. The
/// age-up's own building, the first army building and that workshop have
/// their price held back from the rest of the think while they cannot be
/// paid for, so that what may borrow half of the saving does not keep them
/// waiting for ever.
const ECO_RESERVE_SHARE: f32 = 0.5;
/// Everything else keeps the whole reserve: production beyond the targets,
/// fortifications, upgrades, research, and army training unless the army is
/// short, defending or razing (see [`Think::mil_share`]).
const KEEP_RESERVE: f32 = 1.0;
/// Army training below the army's goal or in defence keeps this share of
/// the reserve (all of it once the age-up is behind schedule).
const AGE_UP_FLOOR: f32 = 0.8;
/// Workers stop eating into the age-up saving once it holds this share of
/// every resource the rest of the age-up path costs ...
const AGE_UP_NEAR: f32 = 0.8;
/// ... and the realm has this share of the age's worker target.
const NEAR_WORKER_SHARE: f32 = 0.6;
/// Housing with less population room than this left is urgent.
const URGENT_ROOM: u32 = 3;
/// While raided with a short army, worker training pauses above this many workers.
const RAIDED_MIN_WORKERS: u32 = 12;
/// Construction slots urgent placements may use beyond the usual limit.
const URGENT_SLOTS: u32 = 2;
/// A staffed foundation whose progress does not grow for this long is deleted.
const SITE_STALL_TIME: f32 = 60.0;
/// Vents tried per think when claims keep failing.
const MAX_VENT_TRIES: usize = 4;
/// A gatherer is not moved to another resource more often than this.
const REBALANCE_DELAY: f32 = 20.0;
/// Workers moved between resources per think, at most.
const MAX_REBALANCE: u32 = 2;
/// A worker idle again this soon after a gather order failed at its target.
const FAILED_ORDER_WINDOW: f32 = 30.0;
/// How long a failed target is avoided.
const BAD_TARGET_TIME: f32 = 45.0;
/// Each worker already on a node counts as this many metres of extra walk.
const CROWD_PENALTY: f32 = 6.0;
/// Remote node searches are refreshed this often.
const REMOTE_REFRESH: f32 = 10.0;
/// With fewer nodes of a resource than this near home, remote clusters are used too.
const THIN_HOME_NODES: u32 = 6;
/// Remote clusters found mined out are searched again after this long at the earliest.
const MINED_OUT_RETRY: f32 = 2.0;
/// After a failed spot search, the same building is not searched again for this long.
const NO_SPOT_RETRY: f32 = 10.0;
/// Per think, remembered shortfalls fade by this factor (half-life ~17 s).
const SHORTFALL_DECAY: f32 = 0.98;
/// In the first age, army production waits for this many workers (plus up
/// to [`HEARTH_TURTLE_WORKERS`] more for a turtle), so the opening economy
/// comes first.
const HEARTH_PRODUCTION_WORKERS: f32 = 15.0;
const HEARTH_TURTLE_WORKERS: f32 = 8.0;
/// Spendable resources the army buys with, beyond which saturated
/// production gets another building.
const SURPLUS_FOR_PRODUCTION: f32 = 500.0;
/// A new field is only built for this many more gatherers than already
/// work its resource (fields wait for their farmers, not for a plan).
const FIELD_LEAD: u32 = 2;
/// Vents are not claimed while this much of every resource they trickle
/// (and that is of use in this age) is spendable beyond the reserve.
const VENT_SURPLUS: f32 = 300.0;
/// Seconds between two status lines of one AI (see [`Think::log_status`]).
const STATUS_INTERVAL: f32 = 60.0;

/// Think once for the view's player, appending commands to `out`.
pub fn run(view: &AiView, mem: &mut AiMemory, out: &mut Vec<PlayerCommand>) {
    mem.prune(view);
    mem.observe_income(view.time, &view.me.stats.gathered);
    mem.note_age(view.me.age, view.time);
    let plan = Plan::new(view, mem);
    mem.home_way = Some(plan.home_way.clone());
    let mut t = Think::new(*view, plan, mem, out);
    t.defend();
    t.abandon_stalled_sites();
    t.age_up();
    t.train_workers();
    t.houses();
    t.rebuild_hall();
    t.assign_workers();
    t.staff_foundations();
    // While the army is short, or the first army building is due, its
    // production comes before the economy's one-offs: a base with no army is
    // a raid's dinner.
    let army_first = t.army_short() || t.first_production_due();
    if army_first {
        t.production_buildings();
    }
    t.drop_sites();
    t.fields();
    t.eco_techs();
    if !army_first {
        t.production_buildings();
    }
    t.claim_nodes();
    t.towers();
    t.walls();
    t.upgrade_defences();
    t.train_army();
    t.military_techs();
    t.spend_surplus();
    t.rallies();
    t.army();
    t.log_status();
    t.finish();
}

struct Think<'m, 'v> {
    view: AiView<'v>,
    plan: Plan,
    mem: &'m mut AiMemory,
    out: &'m mut Vec<PlayerCommand>,
    budget: Budget,
    sources: Sources,
    /// Own drop sites: (position, accepted resources mask).
    drops: Vec<(Vec2, u16)>,
    /// Workers given a job this think.
    taken: Vec<Entity>,
    /// Buildings given research or an upgrade this think (their snapshot
    /// queue does not show it yet).
    busy: Vec<Entity>,
    /// Foundations deleted this think.
    abandoned: Vec<Entity>,
    /// Workers gathering each resource, updated as jobs are given.
    gathering: [u32; MAX_RESOURCES],
    desired: [u32; MAX_RESOURCES],
    /// Workers per node/field, updated as jobs are given.
    crowd: Vec<(Entity, u32)>,
    /// Footprints placed but not yet in the spatial index.
    extra: Vec<Obb>,
    /// Resources that blocked purchases (buildings, a short army) this think.
    missing: ResAmounts,
    /// What army training and building placements spent this think.
    army_spent: ResAmounts,
    build_spent: ResAmounts,
    /// Foundations plus pending placements.
    constructions: u32,
    queued_pop: u32,
    defending: bool,
}

impl<'m, 'v> Think<'m, 'v> {
    fn new(view: AiView<'v>, plan: Plan, mem: &'m mut AiMemory, out: &'m mut Vec<PlayerCommand>) -> Self {
        let now = view.time;
        let mut sources = Sources::home(&view, plan.home, |e| mem.is_bad_target(e));
        for r in view.data.resource_ids() {
            // Remote clusters too, when what is left near home is thin.
            if sources.home_count(r) >= THIN_HOME_NODES {
                continue;
            }
            // Use the cached search; search again when it is old, or when
            // the clusters it found are mined out.
            let cached = mem.remote[r.index()].filter(|s| now - s.at <= REMOTE_REFRESH);
            if let Some(found) = cached {
                for pos in found.found() {
                    sources.add_remote(&view, r, pos, |e| mem.is_bad_target(e));
                }
            }
            let mined_out = cached.is_some_and(|s| s.found().next().is_some() && now - s.at > MINED_OUT_RETRY)
                && sources.remote_count(r) == 0;
            if cached.is_some() && !mined_out {
                continue;
            }
            let field = mem.home_field(&view, plan.home);
            let found = search_remote(&view, plan.home, r, field.as_deref());
            mem.remote[r.index()] = Some(RemoteSearch { clusters: found, at: now });
            for pos in found.iter().flatten() {
                sources.add_remote(&view, r, *pos, |e| mem.is_bad_target(e));
            }
        }
        let snap = view.snap;
        let mut gathering = [0; MAX_RESOURCES];
        let mut crowd: Vec<(Entity, u32)> = Vec::new();
        for w in &snap.workers {
            if let Some(r) = w.task.resource() {
                gathering[r.index()] += 1;
            }
            if let WorkerTask::Gather { target: Some(t), .. } = w.task {
                bump(&mut crowd, t, 1);
            }
        }
        let drops = snap.buildings.iter().filter(|b| b.complete && b.drop_mask != 0).map(|b| (b.pos, b.drop_mask)).collect();
        // Wall segments are quick and many: they do not hold construction slots.
        let foundations = snap.buildings.iter().filter(|b| !b.complete && !view.data.building(b.kind).wall).count();
        let constructions = foundations as u32 + mem.pending.len() as u32;
        Think {
            budget: Budget::new(view.me.resources, plan.reserve),
            queued_pop: plan.queued_pop,
            extra: mem.pending_obbs().collect(),
            view,
            plan,
            mem,
            out,
            sources,
            drops,
            taken: Vec::new(),
            busy: Vec::new(),
            abandoned: Vec::new(),
            gathering,
            desired: [0; MAX_RESOURCES],
            crowd,
            constructions,
            missing: ResAmounts::ZERO,
            army_spent: ResAmounts::ZERO,
            build_spent: ResAmounts::ZERO,
            defending: false,
        }
    }

    fn now(&self) -> f32 {
        self.view.time
    }

    fn cmd(&mut self, entities: Vec<Entity>, kind: CommandKind) {
        if !entities.is_empty() {
            self.out.push(PlayerCommand::new(self.view.me.id, entities, kind));
        }
    }

    /// The share of the age-up reserve army training keeps: none of it when
    /// the army must come first (see [`Think::spend_all_on_army`]); all of it
    /// once the army reaches its goal, and all of it once the age-up is
    /// behind schedule; below the goal or in defence only [`AGE_UP_FLOOR`] of
    /// it. Saving never leaves the base bare, and an ordinary fight never
    /// takes the whole saving: a realm under pressure still ages up.
    fn mil_share(&self) -> f32 {
        if self.spend_all_on_army() {
            return 0.0;
        }
        if self.plan.behind_schedule || (!self.defending && !self.army_short()) { KEEP_RESERVE } else { AGE_UP_FLOOR }
    }

    /// May army training spend the whole age-up saving? For a realm raided
    /// with an army below a quarter of its target (no age is worth the
    /// town), and for one razing the town of the last enemy left (finishing
    /// it ends the match), but only while the age-up keeps to
    /// [`AGE_SCHEDULE`](crate::AGE_SCHEDULE): once it falls behind, the
    /// saving is kept and the next age comes, for the raided realm and for
    /// the one razing alike. Two cases go on regardless: a raid in earnest
    /// (more hostile army in the base than a scout's poke, see
    /// [`military::POKE_POP`]), where the town itself is at stake, and a
    /// last enemy all but dead (it lost every production building, see
    /// `AiView::is_revealed`), where the next age would come too late to matter.
    fn spend_all_on_army(&self) -> bool {
        let on_schedule = !self.plan.behind_schedule;
        let raided = self.defending
            && self.plan.army_pop < self.plan.army_target / 4
            && (on_schedule || self.raid_strength() > military::POKE_POP);
        let razing = self.mem.razing_lately(self.now())
            && self.last_enemy()
            && (on_schedule || self.mem.raze_victim.is_some_and(|p| self.view.is_revealed(p)));
        raided || razing
    }

    /// Has the saving all but reached the age-up? The stockpile holds at
    /// least [`AGE_UP_NEAR`] of every resource the rest of the age-up path
    /// still costs, and the realm has [`NEAR_WORKER_SHARE`] of its workers.
    fn age_up_near(&self) -> bool {
        let (cost, have) = (&self.plan.age_cost, &self.view.me.resources);
        let workers = (self.plan.workers + self.plan.workers_queued) as f32 >= self.plan.worker_target as f32 * NEAR_WORKER_SHARE;
        workers && cost.total() > 0.0 && self.view.data.resource_ids().all(|r| cost.get(r) <= 0.0 || have.get(r) >= cost.get(r) * AGE_UP_NEAR)
    }

    /// Only one hostile player is left in the game.
    pub(super) fn last_enemy(&self) -> bool {
        let me = self.view.me.id;
        self.view.players.iter().filter(|p| !p.defeated && self.view.players.hostile(me, p.id)).count() == 1
    }

    /// Does an army production building (worker halls aside) stand, or is
    /// one laid out or placed?
    fn has_army_production(&self) -> bool {
        let (army, worker) = (&self.plan.trains_army, &self.plan.trains_worker);
        let trains = |k: BuildingTypeId| army[k.index()] && !worker[k.index()];
        self.view.snap.buildings.iter().any(|b| trains(b.kind)) || self.mem.pending.iter().any(|p| trains(p.kind))
    }

    /// Is the first army production building due, whatever else waits? From
    /// the second age on whenever none stands or is on its way (no army
    /// without one), and in the first age once the age-up is being
    /// researched: nothing is saved for then, and the building stands when
    /// the new age begins however few workers took the realm there. The
    /// cheaper buildings bought as wood trickles in (fields, houses, towers)
    /// would otherwise keep it from ever being affordable (see
    /// [`Think::production_buildings`]).
    fn first_production_due(&self) -> bool {
        let due = self.view.me.age >= 2 || self.plan.age_up == AgeUpStep::InProgress;
        due && !self.has_army_production()
    }

    /// The army, alive and queued, is below its goal (see `Plan::army_goal`).
    fn army_short(&self) -> bool {
        self.plan.army_pop + self.plan.army_queued < self.plan.army_goal
    }

    /// The army is below half its target: too thin to afford fortifications
    /// or to keep training workers under a raid.
    fn army_thin(&self) -> bool {
        self.plan.army_pop < self.plan.army_target / 2
    }

    /// The reserve share for investments that pay off later (economy techs,
    /// vents, production up to the age's targets, the first tower):
    /// [`ECO_RESERVE_SHARE`], or all of the reserve once the age-up is
    /// behind schedule. Fields, drop sites and housing always borrow: the
    /// saving itself depends on them.
    fn invest_share(&self) -> f32 {
        if self.plan.behind_schedule { KEEP_RESERVE } else { ECO_RESERVE_SHARE }
    }

    fn max_constructions(&self) -> u32 {
        2 + self.view.me.age as u32
    }

    /// The building's queue holds research or an upgrade, or got one this think.
    fn queue_busy(&self, b: &BuildingInfo) -> bool {
        self.view.snap.queue_busy(b) || self.busy.contains(&b.entity)
    }

    /// Order research at a building and remember it is busy with it.
    fn research(&mut self, building: Entity, tech: TechId) {
        self.mem.research_sent.push((tech, self.now()));
        self.busy.push(building);
        self.cmd(vec![building], CommandKind::Research { tech });
    }

    /// Some worker can still be given a job this think.
    fn has_free_worker(&self) -> bool {
        self.view.snap.workers.iter().any(|w| w.task.available() && !self.taken.contains(&w.entity))
    }

    /// Radius of a building type's footprint.
    fn radius(&self, kind: BuildingTypeId) -> f32 {
        let s = self.view.data.building(kind).size;
        Vec2::new(s[0], s[2]).length() * 0.5
    }

    /// Search a spot and place `q.kind` there with up to `builders`
    /// workers, if affordable keeping `reserve_share` of the reserve.
    fn place(&mut self, q: SpotQuery, builders: usize, reserve_share: f32) -> bool {
        self.place_with_slots(q, builders, reserve_share, 0)
    }

    /// [`Think::place`] for something urgent (the age-up's building, housing
    /// when nearly capped), which may use [`URGENT_SLOTS`] construction slots
    /// beyond the usual limit.
    fn place_urgent(&mut self, q: SpotQuery, builders: usize, reserve_share: f32) -> bool {
        self.place_with_slots(q, builders, reserve_share, URGENT_SLOTS)
    }

    fn place_with_slots(&mut self, q: SpotQuery, builders: usize, reserve_share: f32, extra_slots: u32) -> bool {
        let (data, me, now) = (self.view.data, self.view.me, self.now());
        if self.constructions >= self.max_constructions() + extra_slots || self.mem.no_spot.iter().any(|(k, _)| *k == q.kind) {
            return false;
        }
        let cost = stats::building_cost(data, me, q.kind);
        if !self.budget.affords(&cost, reserve_share) {
            self.note_shortfall(&cost, reserve_share);
            return false;
        }
        // The spot search is the expensive part: skip it when nobody could build.
        if !self.has_free_worker() {
            return false;
        }
        let avoid: Vec<(Vec2, f32)> = self.mem.bad_spots.iter().filter(|s| s.0 == q.kind).map(|s| (s.1, 3.0)).collect();
        match find_build_spot(&self.view, &q, &self.extra, &avoid) {
            Some(p) => self.commit_place(q.kind, p, builders, cost),
            None => {
                self.mem.no_spot.push((q.kind, now + NO_SPOT_RETRY));
                false
            }
        }
    }

    /// Remember what blocked a purchase, so gathering leans toward it.
    fn note_shortfall(&mut self, cost: &ResAmounts, reserve_share: f32) {
        let short = self.budget.shortfall(cost, reserve_share);
        for (m, s) in self.missing.0.iter_mut().zip(short.0.iter()) {
            *m = m.max(*s);
        }
    }

    /// Every [`STATUS_INTERVAL`] seconds, log the army's and the saving's
    /// state at debug level under the `af_ai::status` target
    /// (`RUST_LOG=warn,af_ai::status=debug`).
    fn log_status(&mut self) {
        let now = self.now();
        if now - self.mem.status_logged < STATUS_INTERVAL || !bevy::log::tracing::enabled!(target: "af_ai::status", bevy::log::Level::DEBUG) {
            return;
        }
        self.mem.status_logged = now;
        let (data, me, plan) = (self.view.data, self.view.me, &self.plan);
        let fmt = |a: &ResAmounts| {
            data.resource_ids().filter(|r| a.get(*r) >= 1.0).map(|r| format!("{}={:.0}", data.resource(r).id, a.get(r))).collect::<Vec<_>>().join(" ")
        };
        let comp: Vec<String> = plan.comp.iter().map(|(u, w)| format!("{}:{:.2}", data.unit(*u).id, w)).collect();
        let attack = match self.mem.attack {
            crate::memory::AttackState::Gathering => "none".to_string(),
            crate::memory::AttackState::Attacking { goal, razing, .. } => {
                let group = self.mem.attackers.iter().filter_map(|e| self.view.snap.army_unit(*e));
                let (pop, near) = group.fold((0, f32::INFINITY), |(p, d), a| (p + a.pop, d.min(a.pos.distance(goal.pos))));
                let state = if razing { " razing" } else { "" };
                format!("{} pop at P{} {} {:.0} m off{}", pop, goal.player.0, military::aim_label(goal.aim), near, state)
            }
        };
        let split: Vec<String> = data
            .resource_ids()
            .filter(|r| self.gathering[r.index()] + self.desired[r.index()] > 0)
            .map(|r| format!("{}={}/{}", data.resource(r).id, self.gathering[r.index()], self.desired[r.index()]))
            .collect();
        let fields = self.view.snap.buildings.iter().filter(|b| b.field.is_some()).count();
        let army_halls = || self.view.snap.buildings.iter().filter(|b| b.complete && plan.trains_army[b.kind.index()] && !plan.trains_worker[b.kind.index()]);
        let idle = army_halls().filter(|b| self.view.snap.queue(b).is_empty()).count();
        debug!(
            target: "af_ai::status",
            "t={:.0} P{} [{}] age {} step {:?} behind {} all-in {} | workers {}/{} army {}/{} attack@{} attacking {} defending {} prod {} (idle {}, queued {}) | income {:.1} army {:.1} build {:.1} /s | bank {} | reserve {} | gather {} | fields {} | short {} | comp {}",
            now,
            me.id.0,
            self.view.faction().id,
            me.age,
            plan.age_up,
            plan.behind_schedule,
            self.spend_all_on_army(),
            plan.workers,
            plan.worker_target,
            plan.army_pop,
            plan.army_goal,
            if plan.wave_size == u32::MAX { 0 } else { plan.wave_size },
            attack,
            self.defending,
            plan.production,
            idle,
            plan.army_queued,
            plan.income.total(),
            plan.army_spend.total(),
            plan.build_spend.total(),
            fmt(&me.resources),
            fmt(&plan.reserve),
            split.join(" "),
            fields,
            fmt(&self.missing),
            comp.join(" ")
        );
    }

    /// Carry this think's shortfalls (fading older ones) and army spending
    /// into memory.
    fn finish(&mut self) {
        for (old, new) in self.mem.shortfall.0.iter_mut().zip(self.missing.0.iter()) {
            *old = (*old * SHORTFALL_DECAY).max(*new);
        }
        self.mem.observe_spending(self.view.time, &self.army_spent, &self.build_spent);
    }

    /// Issue a placement already validated by `check_placement`.
    fn commit_place(&mut self, kind: BuildingTypeId, p: Placement, builders: usize, cost: ResAmounts) -> bool {
        let mut crew = Vec::new();
        pick_workers(&self.view, p.pos, builders, &self.taken, &mut crew);
        if crew.is_empty() {
            self.mem.no_spot.push((kind, self.now() + NO_SPOT_RETRY));
            return false;
        }
        for w in &crew {
            self.claim_worker(*w, Entity::PLACEHOLDER);
        }
        self.budget.spend(&cost);
        self.build_spent.add_all(&cost);
        let obb = Obb::from_size(p.pos, self.view.data.building(kind).size, p.facing);
        self.extra.push(obb);
        self.mem.pending.push(PendingBuild { kind, pos: p.pos, obb, time: self.now() });
        self.constructions += 1;
        self.cmd(crew, CommandKind::PlaceBuilding { kind, pos: p.pos, facing: p.facing });
        true
    }

    /// Mark a worker as given a job (target = node/field, or PLACEHOLDER for construction).
    fn claim_worker(&mut self, w: Entity, target: Entity) {
        if let Some(info) = self.view.snap.workers.iter().find(|x| x.entity == w) {
            if let Some(r) = info.task.resource() {
                self.gathering[r.index()] = self.gathering[r.index()].saturating_sub(1);
            }
            if let Some(t) = info.task.target() {
                bump(&mut self.crowd, t, -1);
            }
        }
        self.taken.push(w);
        self.mem.worker_orders.insert(w, WorkerOrder { target, time: self.now() });
    }

    fn give_job(&mut self, w: Entity, target: Entity, r: ResourceId) {
        self.claim_worker(w, target);
        self.gathering[r.index()] += 1;
        bump(&mut self.crowd, target, 1);
        self.cmd(vec![w], CommandKind::Gather(target));
    }

    fn crowd_of(&self, e: Entity) -> u32 {
        self.crowd.iter().find(|c| c.0 == e).map_or(0, |c| c.1)
    }

    fn drop_dist(&self, r: ResourceId, pos: Vec2) -> f32 {
        let bit = 1u16 << r.0;
        self.drops.iter().filter(|(_, m)| m & bit != 0).map(|(p, _)| p.distance(pos)).fold(f32::INFINITY, f32::min)
    }

    /// Best node or own field of `r` for a worker at `from`: short walk, close
    /// to a drop site, not crowded.
    fn pick_source(&self, r: ResourceId, from: Vec2) -> Option<(Entity, Vec2)> {
        let mut best: Option<(f32, Entity, Vec2)> = None;
        let mut consider = |score: f32, e: Entity, pos: Vec2| {
            if best.is_none_or(|(b, ..)| score < b) {
                best = Some((score, e, pos));
            }
        };
        for n in self.sources.of(r) {
            if self.mem.is_bad_target(n.entity) {
                continue;
            }
            let drop = self.drop_dist(r, n.pos).min(200.0);
            consider(from.distance(n.pos) + 0.7 * drop + CROWD_PENALTY * self.crowd_of(n.entity) as f32, n.entity, n.pos);
        }
        for b in &self.view.snap.buildings {
            let Some(f) = b.field.filter(|f| b.complete && f.resource == r) else { continue };
            if f.users.max(self.crowd_of(b.entity)) >= f.slots || self.mem.is_bad_target(b.entity) {
                continue;
            }
            consider(from.distance(b.pos) + 0.7 * self.drop_dist(r, b.pos).min(200.0), b.entity, b.pos);
        }
        best.map(|(_, e, p)| (e, p))
    }

    // -----------------------------------------------------------------------
    // Steps
    // -----------------------------------------------------------------------

    /// Take the next step toward the next age. What gets paid is released
    /// from the reserve, so the rest of the think may spend the surplus; a
    /// building the age-up needs and cannot pay for yet has its price held
    /// back the same way.
    fn age_up(&mut self) {
        let (data, me, now) = (self.view.data, self.view.me, self.now());
        let paid = match self.plan.age_up {
            AgeUpStep::Research { tech, building } => {
                let cost = data.tech(tech).cost.amounts;
                let ok = !self.mem.research_recent(tech) && self.budget.try_spend(&cost, 0.0);
                if ok {
                    self.research(building, tech);
                }
                ok.then_some(cost)
            }
            AgeUpStep::Upgrade { building } => {
                let Some(b) = self.view.snap.building(building) else { return };
                let cost = data.building(b.kind).upgrade.as_ref().map_or(ResAmounts::ZERO, |u| u.cost.amounts);
                let ok = !self.mem.upgrade_sent.iter().any(|(e, _)| *e == building) && self.budget.try_spend(&cost, 0.0);
                if ok {
                    self.mem.upgrade_sent.push((building, now));
                    self.busy.push(building);
                    self.cmd(vec![building], CommandKind::UpgradeBuilding);
                }
                ok.then_some(cost)
            }
            AgeUpStep::Build(kind) => {
                let cost = stats::building_cost(data, me, kind);
                if !self.budget.affords(&cost, 0.0) {
                    self.note_shortfall(&cost, 0.0);
                    if self.first_production_due() {
                        // A realm with no army building raises one first:
                        // holding this price back could keep it from ever
                        // affording one (it lacks something else entirely,
                        // e.g. stone for the foundry, while wood idles).
                        None
                    } else {
                        // Held back from the rest of the think as if paid:
                        // the fields and houses that may borrow half of the
                        // saving would otherwise take the wood as it
                        // trickles in and keep the building from ever being
                        // affordable.
                        self.budget.spend(&cost);
                        Some(cost)
                    }
                } else {
                    let q = SpotQuery {
                        kind,
                        anchor: self.plan.home,
                        toward: self.plan.front.perp(),
                        min_r: self.plan.hall_radius + self.radius(kind) + 2.0,
                        max_r: 50.0,
                        gap: 2.0,
                    };
                    self.place_urgent(q, builders_for(data, kind), 0.0).then_some(cost)
                }
            }
            _ => None,
        };
        if let Some(cost) = paid {
            self.budget.release(&cost);
        }
    }

    /// Queue workers at idle halls until the age's worker target, out of the
    /// age-up reserve too: workers pay for themselves within minutes and
    /// every later purchase, the age-up included, rides on them; but not
    /// once the saving has all but reached the age-up (see
    /// [`Think::age_up_near`]), where a worker would only put the next age
    /// off, nor past half the target once the age-up is behind schedule.
    /// Paused while raided with a thin army.
    fn train_workers(&mut self) {
        let (data, me) = (self.view.data, self.view.me);
        // Raided with too small an army: soldiers first, or the raiders
        // just keep killing the new workers.
        if self.defending && self.army_thin() && self.plan.workers >= RAIDED_MIN_WORKERS {
            return;
        }
        let mut short = self.plan.worker_target.saturating_sub(self.plan.workers + self.plan.workers_queued);
        let kind = self.plan.worker_kind;
        let def = data.unit(kind);
        let pop = def.pop * def.squad;
        let cost = stats::unit_cost(data, me, kind);
        let half = (self.plan.workers + self.plan.workers_queued) * 2 >= self.plan.worker_target;
        let share = if self.age_up_near() || (half && self.plan.behind_schedule) { KEEP_RESERVE } else { 0.0 };
        let snap = self.view.snap;
        for b in &snap.buildings {
            if short == 0 {
                break;
            }
            if !b.complete || !self.plan.trains_worker[b.kind.index()] || self.queue_busy(b) || snap.queue(b).len() >= MAX_QUEUE {
                continue;
            }
            if rules::can_train(data, me, b.kind, kind, false).is_err() {
                continue;
            }
            if me.pop_used + self.queued_pop + pop > me.pop_cap.min(POP_HARD_CAP) || !self.budget.try_spend(&cost, share) {
                break;
            }
            self.queued_pop += pop;
            short = short.saturating_sub(def.squad.max(1));
            self.cmd(vec![b.entity], CommandKind::Train { unit: kind, count: 1 });
        }
    }

    /// Build housing before population room runs out (one at a time unless nearly full).
    fn houses(&mut self) {
        let (data, me) = (self.view.data, self.view.me);
        let snap = self.view.snap;
        let planned = snap.buildings.iter().filter(|b| !b.complete).map(|b| b.kind).chain(self.mem.pending.iter().map(|p| p.kind));
        let (mut pending_pop, mut in_progress) = (0, 0);
        for kind in planned {
            let pop = data.building(kind).pop;
            if pop > 0 {
                pending_pop += pop;
                in_progress += 1;
            }
        }
        if !needs_house(me, self.queued_pop, pending_pop, self.plan.production) {
            return;
        }
        let room = me.pop_cap.saturating_sub(me.pop_used + self.queued_pop);
        let urgent = room < URGENT_ROOM;
        if in_progress >= if urgent { 2 } else { 1 } {
            return;
        }
        let Some(kind) = house_kind(data, me) else { return };
        let q = SpotQuery {
            kind,
            anchor: self.plan.home,
            toward: -self.plan.front,
            min_r: self.plan.hall_radius + self.radius(kind) + 2.0,
            max_r: 45.0,
            gap: 1.5,
        };
        if urgent {
            self.place_urgent(q, 1, 0.0);
        } else {
            self.place(q, 1, ECO_RESERVE_SHARE);
        }
    }

    /// Without any town centre, raise a new one at the start position.
    fn rebuild_hall(&mut self) {
        let (data, me) = (self.view.data, self.view.me);
        if self.plan.hall.is_some() || self.mem.pending.iter().any(|p| data.building(p.kind).has_tag(Tag::TownCenter)) {
            return;
        }
        let kind = rules::build_menu(data, me)
            .into_iter()
            .filter(|b| data.building(*b).has_tag(Tag::TownCenter) && rules::can_build(data, me, *b, false).is_ok())
            .min_by(|a, b| cost_total(data, me, *a).total_cmp(&cost_total(data, me, *b)));
        let Some(kind) = kind else { return };
        let q = SpotQuery { kind, anchor: me.start_pos, toward: self.plan.front, min_r: 0.0, max_r: 40.0, gap: 3.0 };
        self.place_urgent(q, 4, 0.0);
    }

    /// Give idle workers the resource furthest below its share, then rebalance.
    fn assign_workers(&mut self) {
        let (data, me, now) = (self.view.data, self.view.me, self.now());
        let snap = self.view.snap;
        let mut fields = [false; MAX_RESOURCES];
        for r in data.resource_ids() {
            let fielded = snap.buildings.iter().any(|b| b.field.is_some_and(|f| f.resource == r));
            fields[r.index()] = fielded || field_kind(data, me, r).is_some();
        }
        let supply = Supply::new(&self.sources, &fields);
        let total = snap.workers.iter().filter(|w| w.task.available() && !self.taken.contains(&w.entity)).count() as u32;
        let wanted = desired_gatherers(&self.view, &self.plan, &supply, total);
        // Only the workers the plans want are given a resource; the others
        // carry on with what they do.
        let smooth = self.mem.smooth_split(now, &wanted);
        let planned = (smooth.iter().sum::<f32>().round() as u32).min(total);
        self.desired = apportion(&smooth, planned);

        let idle: Vec<WorkerInfo> =
            snap.workers.iter().filter(|w| w.task == WorkerTask::Idle && !self.taken.contains(&w.entity)).copied().collect();
        for w in idle {
            if self.mem.ordered_within(w.entity, now, REORDER_DELAY) {
                continue;
            }
            // Idle again soon after a gather order: that target did not work
            // out. Count the failure once, however long the worker stays idle.
            if let Some(prev) = self.mem.worker_orders.get_mut(&w.entity)
                && prev.target != Entity::PLACEHOLDER
                && now - prev.time < FAILED_ORDER_WINDOW
            {
                let target = std::mem::replace(&mut prev.target, Entity::PLACEHOLDER);
                if !self.mem.is_bad_target(target) {
                    self.mem.bad_targets.push((target, now + BAD_TARGET_TIME));
                }
            }
            for r in self.by_deficit() {
                if let Some((target, _)) = self.pick_source(r, w.pos) {
                    self.give_job(w.entity, target, r);
                    break;
                }
            }
        }
        self.rebalance();
    }

    /// Resources with a desired share, largest shortfall first.
    fn by_deficit(&self) -> Vec<ResourceId> {
        let mut rs: Vec<ResourceId> = self.view.data.resource_ids().filter(|r| self.desired[r.index()] > 0).collect();
        rs.sort_by_key(|r| (std::cmp::Reverse(self.desired[r.index()] as i32 - self.gathering[r.index()] as i32), r.0));
        rs
    }

    /// Move a few gatherers from the most over-served resource to the most under-served.
    fn rebalance(&mut self) {
        let now = self.now();
        let snap = self.view.snap;
        for _ in 0..MAX_REBALANCE {
            let gap = |r: &ResourceId| self.desired[r.index()] as i32 - self.gathering[r.index()] as i32;
            let to = self.view.data.resource_ids().max_by_key(|r| (gap(r), std::cmp::Reverse(r.0)));
            let from = self.view.data.resource_ids().min_by_key(|r| (gap(r), r.0));
            let (Some(to), Some(from)) = (to, from) else { return };
            let (need, spare) = (gap(&to), -gap(&from));
            // Only move for a clear imbalance, so single workers do not
            // ping-pong; but nobody on a resource the age-up lacks is one.
            let stalls_age_up =
                self.gathering[to.index()] == 0 && self.plan.age_cost.get(to) > self.view.me.resources.get(to);
            if need < 1 || spare < 1 || (need + spare < 3 && !stalls_age_up) {
                return;
            }
            let Some((_, hint)) = self.pick_source(to, self.plan.home) else { return };
            let mover = snap
                .workers
                .iter()
                .filter(|w| {
                    w.task.resource() == Some(from)
                        && !self.taken.contains(&w.entity)
                        && !self.mem.ordered_within(w.entity, now, REBALANCE_DELAY)
                })
                .min_by(|a, b| a.pos.distance_squared(hint).total_cmp(&b.pos.distance_squared(hint)));
            let Some(w) = mover.copied() else { return };
            let Some((target, _)) = self.pick_source(to, w.pos) else { return };
            self.give_job(w.entity, target, to);
        }
    }

    /// Delete foundations that builders were sent to but that have not
    /// grown for [`SITE_STALL_TIME`] (builders cannot reach them, or keep
    /// getting killed there): they would hold a construction slot forever.
    /// The spot is avoided for a while.
    fn abandon_stalled_sites(&mut self) {
        let now = self.now();
        let snap = self.view.snap;
        for b in snap.buildings.iter().filter(|b| !b.complete) {
            let staffed = snap.workers.iter().any(|w| w.task == WorkerTask::Build { site: b.entity })
                || self.mem.builder_sent.iter().any(|(s, _)| *s == b.entity);
            let Some(i) = self.mem.site_watch.iter().position(|w| w.site == b.entity) else {
                if staffed {
                    self.mem.site_watch.push(SiteWatch { site: b.entity, progress: b.progress, since: now });
                }
                continue;
            };
            let watch = &mut self.mem.site_watch[i];
            if b.progress > watch.progress + 1e-4 {
                watch.progress = b.progress;
                watch.since = now;
            } else if now - watch.since > SITE_STALL_TIME {
                self.mem.site_watch.swap_remove(i);
                self.mem.builder_sent.retain(|(s, _)| *s != b.entity);
                self.mem.bad_spots.push((b.kind, b.pos, now + BAD_SPOT_TIME));
                self.abandoned.push(b.entity);
                self.constructions = self.constructions.saturating_sub(1);
                self.cmd(vec![b.entity], CommandKind::Delete);
            }
        }
    }

    /// Send a builder to foundations nobody is working on.
    fn staff_foundations(&mut self) {
        let now = self.now();
        let snap = self.view.snap;
        let mut sent = 0;
        for b in snap.buildings.iter().filter(|b| !b.complete) {
            if sent >= 3 {
                break;
            }
            let staffed = snap.workers.iter().any(|w| w.task == WorkerTask::Build { site: b.entity });
            if staffed || self.abandoned.contains(&b.entity) || self.mem.builder_sent.iter().any(|(s, _)| *s == b.entity) {
                continue;
            }
            let mut crew = Vec::new();
            pick_workers(&self.view, b.pos, 1, &self.taken, &mut crew);
            let Some(&w) = crew.first() else { break };
            self.claim_worker(w, Entity::PLACEHOLDER);
            self.mem.builder_sent.push((b.entity, now));
            self.cmd(crew, CommandKind::Build(b.entity));
            sent += 1;
        }
    }

    /// Place a drop site by the cluster [`drop_site_plan`] picks, one at a time.
    fn drop_sites(&mut self) {
        let data = self.view.data;
        let is_drop = |k: BuildingTypeId| {
            let d = data.building(k);
            !d.drop_site_ids.is_empty() && !d.has_tag(Tag::TownCenter) && d.requires_node_id.is_none()
        };
        if self.view.snap.buildings.iter().any(|b| !b.complete && is_drop(b.kind)) || self.mem.pending.iter().any(|p| is_drop(p.kind)) {
            return;
        }
        let plan = drop_site_plan(&self.view, self.plan.home, &self.sources, &self.desired, &self.mem.pending, |e| self.crowd_of(e) > 0);
        let Some(dp) = plan else { return };
        let q = SpotQuery {
            kind: dp.kind,
            anchor: dp.anchor,
            toward: (self.plan.home - dp.anchor).normalize_or(self.plan.front),
            min_r: self.radius(dp.kind) + 1.0,
            max_r: self.radius(dp.kind) + 12.0,
            gap: 1.0,
        };
        self.place(q, 1, ECO_RESERVE_SHARE);
    }

    /// Build fields (farms) for resources whose home nodes cannot employ the
    /// wanted gatherers, packed round drop sites (see [`field_spot`]), with a
    /// new drop site as the next hub once the others are ringed. A field is
    /// only built when every complete one is taken and it would be worked
    /// soon (no more than [`FIELD_LEAD`] gatherers beyond those already on
    /// the resource): idle fields are wood the economy lacks.
    fn fields(&mut self) {
        let (data, me) = (self.view.data, self.view.me);
        let snap = self.view.snap;
        for r in data.resource_ids() {
            let want = self.desired[r.index()].min(self.gathering[r.index()] + FIELD_LEAD);
            if want == 0 {
                continue;
            }
            let Some(kind) = field_kind(data, me, r) else { continue };
            let node_room = self.sources.home_count(r) * WORKERS_PER_NODE;
            let produces = |k: BuildingTypeId| data.building(k).resource_field.as_ref().filter(|f| f.resource_id == r).map(|f| f.slots);
            let (mut slots, mut in_progress, mut free) = (0, 0, 0);
            for b in &snap.buildings {
                if let Some(s) = produces(b.kind) {
                    slots += s;
                    in_progress += u32::from(!b.complete);
                    if let Some(f) = b.field.filter(|_| b.complete) {
                        free += f.slots.saturating_sub(f.users.max(self.crowd_of(b.entity)));
                    }
                }
            }
            for p in &self.mem.pending {
                if let Some(s) = produces(p.kind) {
                    slots += s;
                    in_progress += 1;
                }
            }
            if node_room + slots >= want || in_progress >= 2 || free > 0 {
                continue;
            }
            let (center, near) = match field_spot(&self.view, r, &self.mem.pending) {
                Some(FieldSpot::NewHub(hub)) => {
                    // A new drop site behind the base, with room for its own ring of fields.
                    let q = SpotQuery {
                        kind: hub,
                        anchor: self.plan.home,
                        toward: -self.plan.front,
                        min_r: self.plan.hall_radius + self.radius(hub) + FIELD_RING,
                        max_r: self.plan.hall_radius + 50.0,
                        gap: self.radius(kind) * 1.5,
                    };
                    if self.place(q, 1, ECO_RESERVE_SHARE) {
                        return;
                    }
                    (self.plan.home, self.plan.hall_radius)
                }
                Some(FieldSpot::Around { pos, radius }) => (pos, radius),
                None => (self.plan.home, self.plan.hall_radius),
            };
            // The ring next to the drop site first, farther out only when it is full.
            if self.mem.no_spot.iter().any(|(k, _)| *k == kind) {
                continue;
            }
            let min_r = near + self.radius(kind) + 1.0;
            let rings = [near + FIELD_RING, near + 30.0];
            for (i, max_r) in rings.into_iter().enumerate() {
                let q = SpotQuery { kind, anchor: center, toward: -self.plan.front, min_r, max_r, gap: 0.8 };
                if self.place(q, 1, ECO_RESERVE_SHARE) {
                    return;
                }
                let no_spot = self.mem.no_spot.iter().any(|(k, _)| *k == kind);
                if !no_spot || i + 1 == rings.len() {
                    break;
                }
                // The tight ring is full: forget that, and search wider.
                self.mem.no_spot.retain(|(k, _)| *k != kind);
            }
        }
    }

    /// Research one useful gather-rate, carry or income tech.
    fn eco_techs(&mut self) {
        let (data, me) = (self.view.data, self.view.me);
        let snap = self.view.snap;
        for b in snap.buildings.iter().filter(|b| b.complete) {
            if self.queue_busy(b) {
                continue;
            }
            for t in rules::research_menu(data, me, b.kind) {
                let def = data.tech(t);
                if !is_eco_tech(def) || !eco_tech_useful(data, def, &self.gathering, self.plan.workers) {
                    continue;
                }
                if self.mem.research_recent(t) || rules::can_research(data, me, b.kind, t, false).is_err() {
                    continue;
                }
                if self.budget.try_spend(&def.cost.amounts, self.invest_share()) {
                    self.research(b.entity, t);
                    return;
                }
            }
        }
    }

    /// Build the next missing military production building, one at a time;
    /// once every target is met, one more whenever the existing ones cannot
    /// spend the bank (see [`Think::extra_production`]). The first one, when
    /// due (see [`Think::first_production_due`]), is the cheapest that
    /// trains the mix, and like the breakers' workshop it has its price
    /// held back from the rest of the think until it can be placed.
    fn production_buildings(&mut self) {
        let data = self.view.data;
        let first = self.first_production_due();
        let min_workers = HEARTH_PRODUCTION_WORKERS + (1.0 - self.view.aggression()) * HEARTH_TURTLE_WORKERS;
        if self.view.me.age == 1 && !first && (self.plan.workers as f32) < min_workers {
            return;
        }
        let trains = &self.plan.trains_army;
        let busy = self.view.snap.buildings.iter().any(|b| !b.complete && trains[b.kind.index()])
            || self.mem.pending.iter().any(|p| trains[p.kind.index()]);
        if busy {
            return;
        }
        let breakers = self.breaker_trainer();
        let next = breakers.or_else(|| {
            if first {
                first_production_building(&self.view, &self.plan.comp)
            } else {
                next_production_building(&self.view, &self.plan.comp, &self.mem.pending)
            }
        });
        let extra = next.is_none();
        let Some(kind) = next.or_else(|| self.extra_production()) else { return };
        let q = SpotQuery {
            kind,
            anchor: self.plan.home,
            toward: self.plan.front,
            min_r: self.plan.hall_radius + self.radius(kind) + 4.0,
            max_r: 55.0,
            gap: 2.5,
        };
        // The first army building may spend everything (no army without
        // it), and so may the workshop of the breakers a razed town waits
        // for; production up to the age's targets is an investment, more
        // only spends a surplus.
        let share = if extra {
            KEEP_RESERVE
        } else if (breakers.is_some() && self.last_enemy()) || first {
            0.0
        } else {
            self.invest_share()
        };
        if breakers.is_some() || first {
            // What it lacks is held back from the rest of the think: fields,
            // houses or archers bought first would keep it from ever standing.
            let cost = stats::building_cost(data, self.view.me, kind);
            if !self.budget.affords(&cost, share) {
                self.note_shortfall(&cost, share);
                self.budget.spend(&cost);
                return;
            }
            self.place_urgent(q, builders_for(data, kind), share);
        } else {
            self.place(q, builders_for(data, kind), share);
        }
    }

    /// While the army razes (see `AiMemory::razing_lately`), the building
    /// that trains the mix's best building breaker, when none stands: the
    /// rams a town is torn down with need their workshop first (and what it
    /// costs is saved for, see [`Think::production_buildings`]).
    fn breaker_trainer(&self) -> Option<BuildingTypeId> {
        let (data, me) = (self.view.data, self.view.me);
        if !self.mem.razing_lately(self.now()) {
            return None;
        }
        let targets = siege_targets(data, &self.mem.known, true);
        let units: Vec<UnitTypeId> = self.plan.comp.iter().map(|c| c.0).collect();
        let breaker = best_breaker(data, me, &units, &targets)?;
        missing_trainer(&self.view, breaker, &self.mem.pending)
    }

    /// Put extractors on free vents (any building that must sit on a node),
    /// unless what they would trickle is already piling up (see
    /// [`VENT_SURPLUS`]). Vents where a claim failed lately are skipped in
    /// favour of the next.
    fn claim_nodes(&mut self) {
        let (data, me, now) = (self.view.data, self.view.me, self.now());
        if self.constructions >= self.max_constructions() || !self.has_free_worker() {
            return;
        }
        for (kind, node) in node_claimers(data, me) {
            // One claim at a time, up to the age's target.
            let under_way = self.mem.pending_count(kind) > 0 || self.view.snap.buildings.iter().any(|b| b.kind == kind && !b.complete);
            if under_way || self.view.count_any(kind) >= vent_target(me.age) || self.mem.no_spot.iter().any(|(k, _)| *k == kind) {
                continue;
            }
            if self.trickle_piling_up(kind) {
                continue;
            }
            let cost = stats::building_cost(data, me, kind);
            let share = self.invest_share();
            if !self.budget.affords(&cost, share) {
                self.note_shortfall(&cost, share);
                continue;
            }
            let mut avoid: Vec<Vec2> = self.mem.bad_spots.iter().filter(|s| s.0 == kind).map(|s| s.1).collect();
            let movers = ground_movers(&self.view);
            let field = self.mem.home_field(&self.view, self.plan.home);
            for _ in 0..MAX_VENT_TRIES {
                let Some(vent) = nearest_free_node(&self.view, node, self.plan.home, &avoid, field.as_deref()) else {
                    self.mem.no_spot.push((kind, now + NO_SPOT_RETRY));
                    break;
                };
                // An extractor that would close a ring on the realm's units counts as a failed claim.
                let placed = check_placement(data, self.view.nav, self.view.spatial, me, kind, vent, 0.0, false, &self.extra)
                    .ok()
                    .filter(|p| !seals_pocket(&self.view, &movers, &[Obb::from_size(p.pos, data.building(kind).size, p.facing)], &self.extra));
                match placed {
                    Some(p) => {
                        self.commit_place(kind, p, 2, cost);
                        return;
                    }
                    None => {
                        self.mem.bad_spots.push((kind, vent, now + BAD_SPOT_TIME));
                        avoid.push(vent);
                    }
                }
            }
        }
    }

    /// Is every resource a `kind` building trickles that is of use in this
    /// age already spendable beyond the reserve by [`VENT_SURPLUS`]?
    fn trickle_piling_up(&self, kind: BuildingTypeId) -> bool {
        let (data, me) = (self.view.data, self.view.me);
        let trickle = &data.building(kind).trickle_amounts;
        let (have, keep) = (&self.budget.have, &self.budget.reserve);
        let mut useful = data.resource_ids().filter(|r| trickle.get(*r) > 0.0 && data.resource(*r).tier <= me.age).peekable();
        useful.peek().is_some() && useful.all(|r| have.get(r) - keep.get(r) > VENT_SURPLUS)
    }

    /// A production building beyond the targets, when the army cannot spend
    /// what is banked: every army building's queue is full, there is room
    /// to train, and more than [`SURPLUS_FOR_PRODUCTION`] of what the army
    /// buys is spendable beyond the reserve. The most important type that
    /// can be built, up to [`max_production`] buildings.
    fn extra_production(&self) -> Option<BuildingTypeId> {
        let (data, me, snap) = (self.view.data, self.view.me, self.view.snap);
        if self.plan.production >= max_production(me.age) || me.pop_room() < 2 * URGENT_ROOM {
            return None;
        }
        let (army, worker) = (&self.plan.trains_army, &self.plan.trains_worker);
        let saturated = snap
            .buildings
            .iter()
            .filter(|b| b.complete && army[b.kind.index()] && !worker[b.kind.index()])
            .all(|b| snap.queue(b).len() >= MAX_QUEUE);
        if !saturated {
            return None;
        }
        let mut used = [false; MAX_RESOURCES];
        for (u, _) in &self.plan.comp {
            for (flag, c) in used.iter_mut().zip(stats::unit_cost(data, me, *u).0) {
                *flag |= c > 0.0;
            }
        }
        let (have, keep) = (&self.budget.have.0, &self.budget.reserve.0);
        let spendable: f32 = (0..MAX_RESOURCES).filter(|i| used[*i]).map(|i| (have[i] - keep[i]).max(0.0)).sum();
        if spendable < SURPLUS_FOR_PRODUCTION {
            return None;
        }
        let menu = rules::build_menu(data, me);
        production_targets(&self.view, &self.plan.comp)
            .into_iter()
            .map(|(b, ..)| b)
            .find(|b| menu.contains(b) && rules::can_build(data, me, *b, false).is_ok())
    }
}

/// Add `delta` to the count of `e` in a small (entity, count) list.
fn bump(list: &mut Vec<(Entity, u32)>, e: Entity, delta: i32) {
    match list.iter_mut().find(|c| c.0 == e) {
        Some(c) => c.1 = c.1.saturating_add_signed(delta),
        None if delta > 0 => list.push((e, delta as u32)),
        None => {}
    }
}
