//! Headless combat tests against the real content. Units and weapons are
//! picked by their properties (kind, targets, tags), never by id, so the
//! tests follow content changes. Most victims are "dummies": an unarmed,
//! passive infantry body with 10 000 hp and no armour, so damage is easy to
//! read; tests about armour, shields and deaths set those up explicitly.

use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

use af_combat::CombatPlugin;
use af_core::prelude::*;
use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};
use bevy::ecs::message::MessageCursor;
use bevy::prelude::*;

const DT: f32 = SIM_DT;
const DUMMY_HP: f32 = 10_000.0;
/// Facing +X / -X.
const EAST: f32 = FRAC_PI_2;
const WEST: f32 = -FRAC_PI_2;

fn weapon_where(data: &GameData, f: impl Fn(&WeaponDef) -> bool) -> WeaponId {
    let i = data.weapons.iter().position(f).expect("no weapon in the content matches");
    WeaponId(i as u16)
}

fn unit_where(data: &GameData, f: impl Fn(&UnitDef) -> bool) -> UnitTypeId {
    data.unit_ids().find(|u| f(data.unit(*u))).expect("no unit in the content matches")
}

fn building_where(data: &GameData, f: impl Fn(&BuildingDef) -> bool) -> BuildingTypeId {
    data.building_ids().find(|b| f(data.building(*b))).expect("no building in the content matches")
}

/// Reads every message of one type written since the last call.
struct Watch<M: Message>(MessageCursor<M>);

impl<M: Message + Clone> Watch<M> {
    fn new(app: &App) -> Self {
        Watch(app.world().resource::<Messages<M>>().get_cursor())
    }
    fn take(&mut self, app: &App) -> Vec<M> {
        self.0.read(app.world().resource::<Messages<M>>()).cloned().collect()
    }
}

/// A flat 96 m map with three players: 0 and 2 are allies (team 0), 1 is
/// the enemy (team 1).
struct Sim {
    app: App,
    data: Arc<GameData>,
    players: Vec<PlayerId>,
    /// A plain square-footprint infantry type used for shooters and dummies.
    base: UnitTypeId,
}

impl Sim {
    fn new() -> Sim {
        Sim::with_data(test_data())
    }

    fn with_data(data: Arc<GameData>) -> Sim {
        Sim::sized(data, 96.0)
    }

    fn sized(data: Arc<GameData>, size: f32) -> Sim {
        let mut app = sim_app(data.clone());
        app.add_plugins(CombatPlugin);
        let players = flat_world(&mut app, size, &[("ironhold", 0), ("dune", 1), ("sylvan", 0)]);
        let base = unit_where(&data, |u| {
            u.locomotion == MoveFlag::Foot
                && u.has_tag(Tag::Infantry)
                && u.gatherer.is_none()
                && u.shield.is_none()
                && u.size[0] == u.size[2]
                && u.size[0] <= 0.8
        });
        run_ticks(&mut app, 0);
        Sim { app, data, players, base }
    }

    /// Half the side of the base infantry's square footprint.
    fn half(&self) -> f32 {
        self.data.unit(self.base).size[0] * 0.5
    }

    fn unit(&mut self, player: usize, kind: UnitTypeId, pos: Vec2, facing: f32) -> Entity {
        let p = self.players[player];
        with_commands(&mut self.app, |c, data, players| spawn_unit(c, data, players.get(p), kind, pos, facing))
    }

    fn building(&mut self, player: usize, kind: BuildingTypeId, pos: Vec2, complete: bool) -> Entity {
        let p = self.players[player];
        with_commands(&mut self.app, |c, data, players| spawn_building(c, data, players.get(p), kind, pos, 0.0, complete))
    }

    fn arm(&mut self, e: Entity, weapons: &[WeaponId]) {
        self.app.world_mut().get_mut::<Weapons>(e).unwrap().slots = weapons.iter().map(|w| WeaponSlot::new(*w)).collect();
    }

    fn toughen(&mut self, e: Entity) {
        let mut entity = self.app.world_mut().entity_mut(e);
        *entity.get_mut::<Health>().unwrap() = Health::full(DUMMY_HP);
        let mut stats = entity.get_mut::<Stats>().unwrap();
        stats.max_hp = DUMMY_HP;
        stats.armor = Armor::default();
        stats.regen = 0.0;
    }

    /// The base infantry carrying only `weapon`, tough enough to outlive
    /// any return fire.
    fn shooter(&mut self, player: usize, pos: Vec2, facing: f32, weapon: WeaponId) -> Entity {
        let e = self.unit(player, self.base, pos, facing);
        self.arm(e, &[weapon]);
        self.toughen(e);
        e
    }

    /// An unarmed, passive, armourless body with [`DUMMY_HP`].
    fn dummy(&mut self, player: usize, pos: Vec2) -> Entity {
        self.dummy_of(player, self.base, pos)
    }

    fn dummy_of(&mut self, player: usize, kind: UnitTypeId, pos: Vec2) -> Entity {
        let e = self.unit(player, kind, pos, 0.0);
        self.arm(e, &[]);
        self.toughen(e);
        self.set(e, Stance::Passive);
        e
    }

    fn set<C: Component<Mutability = bevy::ecs::component::Mutable>>(&mut self, e: Entity, value: C) {
        *self.app.world_mut().get_mut::<C>(e).unwrap() = value;
    }

    fn attack(&mut self, attacker: Entity, target: Entity, forced: bool) {
        self.app.world_mut().entity_mut(attacker).insert(AttackTarget { entity: target, forced });
    }

    /// Puts a unit under way as af_movement would show it: a goal and a path
    /// it is following (nothing moves it in these tests).
    fn walk(&mut self, e: Entity) {
        let goal = MoveGoal::point(Vec2::new(90.0, 90.0), 0.5);
        let mut entity = self.app.world_mut().entity_mut(e);
        entity.insert(goal);
        *entity.get_mut::<PathState>().unwrap() = PathState { status: PathStatus::Following, goal_seq: goal.seq, ..PathState::default() };
    }

    /// Stops a unit put under way by [`Sim::walk`].
    fn stop(&mut self, e: Entity) {
        let mut entity = self.app.world_mut().entity_mut(e);
        entity.remove::<MoveGoal>();
        *entity.get_mut::<PathState>().unwrap() = PathState::default();
    }

    fn slots(&self, e: Entity) -> Vec<WeaponSlot> {
        self.app.world().get::<Weapons>(e).unwrap().slots.clone()
    }

    fn hp(&self, e: Entity) -> Option<f32> {
        self.app.world().get::<Health>(e).map(|h| h.hp)
    }

    /// Damage a dummy has taken so far.
    fn taken(&self, e: Entity) -> f32 {
        DUMMY_HP - self.hp(e).expect("dummy is alive")
    }

    fn tags(&self, e: Entity) -> TagSet {
        self.app.world().get::<Stats>(e).unwrap().tags
    }

    /// Full damage of one hit of `w` on `e` before armour.
    fn hit_on(&self, w: WeaponId, e: Entity) -> f32 {
        self.data.weapon(w).damage * stats::bonus_multiplier(&self.data, w, self.tags(e))
    }

    fn tick(&mut self, n: u32) {
        run_ticks(&mut self.app, n);
    }

    /// Run up to `max` ticks until `done`; returns the ticks run, or None.
    fn tick_until(&mut self, max: u32, mut done: impl FnMut(&Sim) -> bool) -> Option<u32> {
        for i in 1..=max {
            self.tick(1);
            if done(self) {
                return Some(i);
            }
        }
        None
    }

    fn fx_boxes(&mut self) -> Vec<FxBox> {
        let mut q = self.app.world_mut().query::<&FxBox>();
        q.iter(self.app.world()).copied().collect()
    }

    fn write(&mut self, target: Entity, amount: f32, dtype: DamageType, source: Option<Entity>, source_player: Option<usize>) {
        let source_player = source_player.map(|i| self.players[i]);
        self.app.world_mut().write_message(DamageMsg { target, amount, dtype, source, source_player });
    }
}

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn melee_kill_time_matches_the_weapon_cycle() {
    let mut sim = Sim::new();
    let data = sim.data.clone();
    let square_ground = |u: &UnitDef| !u.is_flying() && u.shield.is_none() && u.size[0] == u.size[2] && u.size[0] <= 1.0;
    let attacker_kind = unit_where(&data, |u| {
        square_ground(u)
            && u.weapon_ids.len() == 1
            && data.weapon(u.weapon_ids[0]).windup > 0.0
            && matches!(data.weapon(u.weapon_ids[0]).kind, WeaponKind::Melee { .. })
    });
    let w = data.unit(attacker_kind).weapon_ids[0];
    let def = data.weapon(w);
    let per_hit = |v: &UnitDef| stats::mitigate(def.damage * data.bonus_multiplier(w, &v.tags), def.damage_type, &v.armor);
    let victim_kind = unit_where(&data, |v| {
        square_ground(v) && v.regen == 0.0 && v.hp < data.unit(attacker_kind).hp && per_hit(v) > 0.0 && (3.0..=12.0).contains(&(v.hp / per_hit(v)).ceil())
    });
    let victim_def = data.unit(victim_kind);
    let hits = (victim_def.hp / per_hit(victim_def)).ceil();
    let expected = def.windup + (hits - 1.0) * def.cooldown;

    let a_half = data.unit(attacker_kind).size[2] * 0.5;
    let v_half = victim_def.size[2] * 0.5;
    let attacker = sim.unit(0, attacker_kind, Vec2::new(40.0, 48.0), EAST);
    let victim = sim.unit(1, victim_kind, Vec2::new(40.0 + a_half + v_half + def.range * 0.5, 48.0), WEST);
    sim.set(victim, Stance::Passive);
    sim.attack(attacker, victim, false);

    let ticks = sim.tick_until(60 * 30, |s| s.hp(victim).is_none()).expect("victim dies");
    let measured = ticks as f32 * DT;
    assert!(close(measured, expected, 3.0 * DT), "killed in {measured:.3}s, expected {expected:.3}s ({hits} hits)");
    assert_eq!(sim.app.world().resource::<Players>().get(sim.players[0]).stats.kills, 1);
}

#[test]
fn bolt_flies_to_its_target_then_damages_it() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| {
        matches!(d.kind, WeaponKind::Bolt { speed } if speed <= 40.0)
            && d.volley == 1
            && d.splash == 0.0
            && d.targets.ground
            && d.min_range == 0.0
            && d.range >= 6.0
    });
    let (range, speed) = match sim.data.weapon(w).kind {
        WeaponKind::Bolt { speed } => (sim.data.weapon(w).range, speed),
        _ => unreachable!(),
    };
    let x0 = 30.0;
    let shooter = sim.shooter(0, Vec2::new(x0, 48.0), EAST, w);
    let target = sim.dummy(1, Vec2::new(x0 + 2.0 * sim.half() + range * 0.8, 48.0));
    sim.attack(shooter, target, false);

    sim.tick_until(90, |s| s.app.world().iter_entities().any(|e| e.contains::<FxBox>())).expect("a bolt is launched");
    assert_eq!(sim.taken(target), 0.0, "no damage at launch");
    let at_launch = sim.fx_boxes()[0];
    sim.tick(1);
    let next = sim.fx_boxes()[0];
    let step = next.pos - at_launch.pos;
    assert!(step.x > 0.0 && close(step.length(), speed * DT, 0.05), "bolt moves at its speed toward the target: {step:?}");

    let flight = sim.tick_until(120, |s| s.taken(target) > 0.0).expect("the bolt lands") + 1;
    let distance = range * 0.8;
    assert!(flight as f32 * DT >= distance / speed * 0.8, "took {flight} ticks for {distance} m at {speed} m/s");
    assert!(close(sim.taken(target), sim.hit_on(w, target), 1e-3));
}

#[test]
fn arc_splash_hits_clustered_enemies_but_spares_a_friend() {
    // An Arc with splash, made exact (no scatter) and without friendly fire.
    let mut data = (*test_data()).clone();
    let w = weapon_where(&data, |d| matches!(d.kind, WeaponKind::Arc { .. }) && d.splash >= 1.2 && d.targets.ground && d.range >= 8.0);
    let def = &mut data.weapons[w.index()];
    def.spread = 0.0;
    def.friendly_fire = false;
    let (min, range, splash) = (def.min_range, def.range, def.splash);
    let mut sim = Sim::with_data(Arc::new(data));

    let half = sim.half();
    let shooter = sim.shooter(0, Vec2::new(20.0, 48.0), EAST, w);
    let x = 20.0 + 2.0 * half + (min + range) * 0.5;
    let centre = sim.dummy(1, Vec2::new(x, 48.0));
    let left = sim.dummy(1, Vec2::new(x, 48.9));
    let right = sim.dummy(1, Vec2::new(x, 47.1));
    let friend = sim.dummy(2, Vec2::new(x + 0.9, 48.0));
    sim.attack(shooter, centre, false);

    sim.tick_until(30 * 6, |s| s.taken(centre) > 0.0).expect("the shell lands");
    let full = sim.hit_on(w, centre);
    assert!(close(sim.taken(centre), full, 1e-2), "direct hit takes full damage");
    let falloff = 1.0 - 0.5 * (0.9 - half) / splash;
    for e in [left, right] {
        assert!(close(sim.taken(e), sim.hit_on(w, e) * falloff, 1e-2), "neighbour takes falloff damage: {}", sim.taken(e));
    }
    assert_eq!(sim.taken(friend), 0.0, "allies are spared without friendly fire");
}

#[test]
fn friendly_fire_splash_hurts_allies_too() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| {
        matches!(d.kind, WeaponKind::Arc { .. }) && d.friendly_fire && d.splash >= 1.5 && d.spread <= 1.0 && d.targets.ground && d.range >= 8.0
    });
    let def = sim.data.weapon(w).clone();
    let shooter = sim.shooter(0, Vec2::new(20.0, 48.0), EAST, w);
    let x = 20.0 + 2.0 * sim.half() + (def.min_range + def.range) * 0.5;
    let target = sim.dummy(1, Vec2::new(x, 48.0));
    let friend = sim.dummy(2, Vec2::new(x, 48.8));
    sim.attack(shooter, target, false);
    sim.tick_until(30 * 6, |s| s.taken(target) > 0.0 || s.taken(friend) > 0.0).expect("the shell lands");
    assert!(sim.taken(friend) > 0.0, "friendly fire splash reaches an ally {} m from the target", 0.8);
    assert_eq!(sim.taken(shooter), 0.0);
}

#[test]
fn beam_deals_its_damage_in_equal_parts_over_its_duration() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Beam { ticks, .. } if ticks >= 3) && d.splash == 0.0 && d.volley == 1 && d.targets.ground);
    let def = sim.data.weapon(w).clone();
    let WeaponKind::Beam { duration, ticks, .. } = def.kind else { unreachable!() };
    let shooter = sim.shooter(0, Vec2::new(30.0, 48.0), EAST, w);
    let target = sim.dummy(1, Vec2::new(30.0 + 2.0 * sim.half() + def.range * 0.5, 48.0));
    sim.attack(shooter, target, false);

    sim.tick_until(90, |s| s.taken(target) > 0.0).expect("the beam opens");
    let mut parts = vec![(0u32, sim.taken(target))];
    let mut last = sim.taken(target);
    for t in 1..=((duration / DT) as u32 + 4) {
        sim.tick(1);
        let now = sim.taken(target);
        if now > last + 1e-4 {
            parts.push((t, now - last));
            last = now;
        }
    }
    let total = sim.hit_on(w, target);
    assert_eq!(parts.len(), ticks as usize, "one damage part per beam tick: {parts:?}");
    for (_, d) in &parts {
        assert!(close(*d, total / ticks as f32, 1e-3), "equal parts: {parts:?}");
    }
    assert!(close(last, total, 1e-2));
    let span = parts.last().unwrap().0 as f32 * DT;
    assert!(close(span, duration * (ticks - 1) as f32 / ticks as f32, 2.0 * DT), "parts spread over the duration: {parts:?}");
}

#[test]
fn armour_mitigation_is_exact_and_clamped() {
    let mut sim = Sim::new();
    let target = sim.dummy(1, Vec2::new(48.0, 48.0));
    sim.app.world_mut().get_mut::<Stats>(target).unwrap().armor = Armor { slash: 0.25, pierce: 0.6, crush: 0.0, fire: 0.95, energy: 0.1 };
    sim.tick(1);

    let cases = [(DamageType::Slash, 100.0, 75.0), (DamageType::Pierce, 50.0, 20.0), (DamageType::Crush, 10.0, 10.0), (DamageType::Fire, 40.0, 4.0), (DamageType::Energy, 30.0, 27.0)];
    for (dtype, amount, expected) in cases {
        let before = sim.taken(target);
        sim.write(target, amount, dtype, None, Some(0));
        sim.tick(1);
        assert!(close(sim.taken(target) - before, expected, 1e-3), "{dtype:?}: {} taken, expected {expected}", sim.taken(target) - before);
    }
}

#[test]
fn shield_absorbs_first_then_recharges_after_its_delay() {
    let mut sim = Sim::new();
    let kind = unit_where(&sim.data, |u| !u.is_flying() && u.shield.as_ref().is_some_and(|s| s.regen > 0.0 && s.energy_resist == 0.0));
    let unit = sim.dummy_of(1, kind, Vec2::new(48.0, 48.0));
    let shield = *sim.app.world().get::<Shield>(unit).unwrap();
    let shield_hp = |s: &Sim| s.app.world().get::<Shield>(unit).unwrap().hp;
    sim.tick(1);

    sim.write(unit, shield.max * 0.5, DamageType::Slash, None, Some(0));
    sim.tick(1);
    assert_eq!(sim.taken(unit), 0.0, "the shield takes it all");
    assert!(close(shield_hp(&sim), shield.max * 0.5, 1e-3));

    sim.tick((shield.delay / DT) as u32 - 3);
    assert!(close(shield_hp(&sim), shield.max * 0.5, 1e-3), "no recharge before the delay");

    sim.tick(3 + 30);
    let recharged = shield_hp(&sim) - shield.max * 0.5;
    assert!(close(recharged, (shield.regen * 1.0).min(shield.max * 0.5), shield.regen * 3.0 * DT), "recharged {recharged} in ~1 s at {}/s", shield.regen);

    let spill = 30.0;
    let current = shield_hp(&sim);
    sim.write(unit, current + spill, DamageType::Pierce, None, Some(0));
    sim.tick(1);
    assert_eq!(shield_hp(&sim), 0.0);
    assert!(close(sim.taken(unit), spill, 1e-3), "the rest goes to hp");
}

#[test]
fn tower_auto_fires_at_an_enemy_in_range_but_not_at_allies() {
    let mut sim = Sim::new();
    let data = sim.data.clone();
    let tower = building_where(&data, |b| b.has_tag(Tag::Tower) && b.weapon_ids.iter().any(|w| data.weapon(*w).targets.ground));
    let w = data.building(tower).weapon_ids[0];
    let reach = data.weapon(w).range;
    let half_tower = data.building(tower).size[0] * 0.5;
    let at = Vec2::new(48.0, 48.0);
    sim.building(0, tower, at, true);
    let offset = half_tower + sim.half() + reach * 0.5;
    let enemy = sim.dummy(1, at + Vec2::new(offset, 0.0));
    let ally = sim.dummy(2, at - Vec2::new(offset, 0.0));
    let own = sim.dummy(0, at + Vec2::new(0.0, offset));

    sim.tick(30 * 5);
    assert!(sim.taken(enemy) > 0.0, "the tower shoots the enemy");
    assert_eq!(sim.taken(ally), 0.0);
    assert_eq!(sim.taken(own), 0.0);
}

#[test]
fn auto_fire_sees_a_big_building_by_its_near_wall_not_its_centre() {
    // The fog contract (af_core::ext::fog): "can team T see this target"
    // tests the target's box, never its centre. A tower in reach of a big
    // enemy building whose near wall is in sight must fire at it although
    // the building's centre cell is dark; with the whole box dark it must not.
    let mut sim = Sim::new();
    let data = sim.data.clone();
    let tower = building_where(&data, |b| b.has_tag(Tag::Tower) && b.weapon_ids.iter().any(|w| data.weapon(*w).targets.ground));
    let w = data.building(tower).weapon_ids[0];
    let def = data.weapon(w).clone();
    let big = data
        .building_ids()
        .filter(|b| {
            let d = data.building(*b);
            d.weapon_ids.is_empty() && !d.has_tag(Tag::Wall) && !d.has_tag(Tag::Gate) && d.size[0] >= 8.0
        })
        .max_by(|a, b| data.building(*a).size[0].total_cmp(&data.building(*b).size[0]))
        .expect("a big unarmed building");
    let half_tower = data.building(tower).size[0] * 0.5;
    let half_big = Vec2::new(data.building(big).size[0], data.building(big).size[2]) * 0.5;
    let at = Vec2::new(30.0, 48.0);
    sim.building(0, tower, at, true);
    let gap = (def.min_range + def.range) * 0.5;
    let centre = at + Vec2::new(half_tower + gap + half_big.x, 0.0);
    let target = sim.building(1, big, centre, true);
    let full = sim.hp(target).unwrap();

    let mut fog = af_core::ext::fog::fog_for_map(Vec2::splat(96.0), 2.0, 2, true);
    sim.app.world_mut().insert_resource(fog.clone());
    sim.tick(30 * 3);
    assert_eq!(sim.hp(target), Some(full), "a building wholly in the dark is not auto-targeted");

    // Light only the strip of cells along the near wall.
    let near = centre.x - half_big.x;
    let (lo, hi) = af_core::ext::fog::fog_cell_rect(&fog, Vec2::new(near, centre.y - half_big.y), Vec2::new(near + 1.0, centre.y + half_big.y)).unwrap();
    for y in lo.y..=hi.y {
        for x in lo.x..=hi.x {
            let i = (y * fog.width + x) as usize;
            fog.teams[0].visible[i] = 1;
            fog.teams[0].explored[i] = true;
        }
    }
    assert!(!fog.visible(0, centre), "the building's centre stays dark");
    assert!(af_core::ext::fog::team_sees_box(&fog, 0, &Obb::new(centre, half_big, 0.0)), "its near wall is in sight");
    sim.app.world_mut().insert_resource(fog);
    sim.tick(30 * 5);
    assert!(sim.hp(target).unwrap() < full, "the tower fires at the building whose wall it sees");
}

#[test]
fn foundations_never_fire() {
    let mut sim = Sim::new();
    let data = sim.data.clone();
    let tower = building_where(&data, |b| b.has_tag(Tag::Tower) && b.weapon_ids.iter().any(|w| data.weapon(*w).targets.ground));
    let reach = data.weapon(data.building(tower).weapon_ids[0]).range;
    let at = Vec2::new(48.0, 48.0);
    let site = sim.building(0, tower, at, false);
    let enemy = sim.dummy(1, at + Vec2::new(data.building(tower).size[0] * 0.5 + sim.half() + reach * 0.5, 0.0));
    sim.tick(30 * 4);
    assert_eq!(sim.taken(enemy), 0.0);
    assert!(sim.app.world().get::<Weapons>(site).unwrap().slots.iter().all(|s| s.target.is_none()));
}

#[test]
fn death_happens_exactly_once_despite_overkill() {
    let mut sim = Sim::new();
    let killer = sim.dummy(0, Vec2::new(40.0, 48.0));
    let victim = sim.dummy(1, Vec2::new(44.0, 48.0));
    sim.set(victim, Health::full(50.0));
    let mut deaths = Watch::<EntityDied>::new(&sim.app);
    sim.tick(1);

    for _ in 0..3 {
        sim.write(victim, 100.0, DamageType::Crush, Some(killer), Some(0));
    }
    sim.tick(1);
    let mut died = deaths.take(&sim.app);
    sim.write(victim, 100.0, DamageType::Crush, Some(killer), Some(0));
    for _ in 0..3 {
        sim.tick(1);
        died.extend(deaths.take(&sim.app));
    }

    assert_eq!(died.len(), 1, "{died:?}");
    assert_eq!(died[0].entity, victim);
    assert_eq!(died[0].killer, Some(sim.players[0]));
    assert!(matches!(died[0].kind, EntityKind::Unit(_)));
    assert!(sim.app.world().get_entity(victim).is_err(), "despawned in Cleanup");
    let players = sim.app.world().resource::<Players>();
    assert_eq!(players.get(sim.players[1]).stats.units_lost, 1);
    assert_eq!(players.get(sim.players[0]).stats.kills, 1);
}

#[test]
fn bomb_falls_from_the_flyer_onto_the_ground_under_its_target() {
    let mut sim = Sim::new();
    let data = sim.data.clone();
    // Scatter stays inside the splash, so the target is always caught.
    let w = weapon_where(&data, |d| matches!(d.kind, WeaponKind::Bomb { .. }) && d.targets.ground && d.splash > d.spread + 0.5);
    let WeaponKind::Bomb { fall_time } = data.weapon(w).kind else { unreachable!() };
    let flyer_kind = unit_where(&data, |u| u.is_flying() && u.shield.is_none());
    let altitude = data.unit(flyer_kind).altitude;
    let at = Vec2::new(48.0, 48.0);
    let flyer = sim.unit(0, flyer_kind, at, 0.0);
    sim.arm(flyer, &[w]);
    sim.toughen(flyer);
    let target = sim.dummy(1, at);

    sim.tick_until(90, |s| s.app.world().iter_entities().any(|e| e.contains::<FxBox>())).expect("a bomb is dropped");
    let released = sim.fx_boxes()[0];
    assert!(released.pos.y > altitude - 1.5, "dropped from the flyer's altitude: {:?}", released.pos);
    let mut lowest = released.pos.y;
    let fall = sim
        .tick_until(90, |s| {
            if let Some(b) = s.app.world().iter_entities().find_map(|e| e.get::<FxBox>().copied()) {
                lowest = lowest.min(b.pos.y);
            }
            s.taken(target) > 0.0
        })
        .expect("the bomb lands");
    assert!(lowest < 2.0, "it reached the ground: lowest {lowest}");
    assert!(close(fall as f32 * DT, fall_time, 3.0 * DT), "fell for {} s, expected {fall_time}", fall as f32 * DT);
    assert_eq!(sim.taken(flyer), 0.0);
}

#[test]
fn nova_hits_every_enemy_around_and_nothing_else() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Nova) && d.targets.ground && !d.friendly_fire && d.range >= 3.0);
    let reach = sim.data.weapon(w).range;
    let at = Vec2::new(48.0, 48.0);
    let shooter = sim.shooter(0, at, 0.0, w);
    let r = 2.0 * sim.half() + reach * 0.5;
    let ring: Vec<Entity> = [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y].into_iter().map(|d| sim.dummy(1, at + d * r)).collect();
    let outside = sim.dummy(1, at + Vec2::new(2.0 * sim.half() + reach + 1.5, 2.0));
    let ally = sim.dummy(2, at + Vec2::new(r, r) * 0.75);

    sim.tick_until(60, |s| ring.iter().any(|e| s.taken(*e) > 0.0)).expect("the nova goes off");
    for e in &ring {
        assert!(close(sim.taken(*e), sim.hit_on(w, *e), 1e-3), "every enemy around is hit once");
    }
    assert_eq!(sim.taken(outside), 0.0);
    assert_eq!(sim.taken(ally), 0.0);
    assert_eq!(sim.taken(shooter), 0.0);
}

#[test]
fn chain_jumps_deal_the_full_hit_every_jump() {
    // The schema documents no falloff and the content is priced on full
    // damage per jump (game-design.md, the faction docs): every enemy the
    // bolt reaches takes the whole hit, the last jump as much as the first.
    let mut sim = Sim::new();
    let data = sim.data.clone();
    let w = (0..data.weapons.len())
        .map(|i| WeaponId(i as u16))
        .filter(|w| matches!(data.weapon(*w).kind, WeaponKind::Chain { jumps, .. } if jumps >= 2) && data.weapon(*w).targets.ground)
        .max_by_key(|w| match data.weapon(*w).kind {
            WeaponKind::Chain { jumps, .. } => jumps,
            _ => 0,
        })
        .expect("a ground chain weapon with several jumps");
    let def = data.weapon(w).clone();
    let WeaponKind::Chain { jumps, jump_range } = def.kind else { unreachable!() };
    let half = sim.half();
    let shooter = sim.shooter(0, Vec2::new(20.0, 48.0), EAST, w);
    let first_x = 20.0 + 2.0 * half + def.range * 0.5;
    let step = 2.0 * half + jump_range * 0.5;
    let line: Vec<Entity> = (0..=jumps).map(|i| sim.dummy(1, Vec2::new(first_x + step * i as f32, 48.0))).collect();
    let beyond = sim.dummy(1, Vec2::new(first_x + step * (jumps + 1) as f32, 48.0));
    let far = sim.dummy(1, Vec2::new(first_x, 48.0 + 2.0 * (jump_range + 2.0 * half) + 1.0));
    let ally = sim.dummy(2, Vec2::new(first_x, 48.0 - 2.0 * half - jump_range * 0.3));
    sim.attack(shooter, line[0], false);

    sim.tick_until(60, |s| s.taken(line[0]) > 0.0).expect("the chain fires");
    for (i, e) in line.iter().enumerate() {
        assert!(close(sim.taken(*e), sim.hit_on(w, *e), 1e-3), "jump {i} deals the full hit: {} of {}", sim.taken(*e), sim.hit_on(w, *e));
    }
    assert_eq!(sim.taken(beyond), 0.0, "no more than `jumps` jumps");
    assert_eq!(sim.taken(far), 0.0);
    assert_eq!(sim.taken(ally), 0.0, "never jumps to allies");
}

#[test]
fn ground_only_weapons_ignore_flyers() {
    let mut sim = Sim::new();
    let data = sim.data.clone();
    let ground_only = weapon_where(&data, |d| !d.targets.air && d.targets.ground && d.min_range == 0.0 && d.range >= 1.0);
    let anti_air = weapon_where(&data, |d| matches!(d.kind, WeaponKind::Bolt { .. }) && d.targets.air && d.targets.ground && d.min_range == 0.0);
    let flyer_kind = unit_where(&data, |u| u.is_flying() && u.shield.is_none());
    let at = Vec2::new(48.0, 48.0);
    let flyer = sim.dummy_of(1, flyer_kind, at);
    let grounded = sim.shooter(0, at + Vec2::new(0.0, 0.2), 0.0, ground_only);
    sim.attack(grounded, flyer, true);
    sim.tick(30 * 3);
    assert_eq!(sim.taken(flyer), 0.0, "a ground-only weapon never fires at a flyer, even when ordered");
    assert!(sim.app.world().get::<Weapons>(grounded).unwrap().slots[0].target.is_none());

    sim.shooter(0, at + Vec2::new(3.0, 0.0), WEST, anti_air);
    sim.tick(30 * 3);
    assert!(sim.taken(flyer) > 0.0, "a weapon that targets air does");
}

#[test]
fn cannon_pierces_its_quota_then_stops_in_the_next_enemy() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| {
        matches!(d.kind, WeaponKind::Cannon { pierce, .. } if pierce >= 1) && d.splash == 0.0 && d.spread == 0.0 && d.targets.ground && d.range >= 10.0
    });
    let def = sim.data.weapon(w).clone();
    let WeaponKind::Cannon { pierce, .. } = def.kind else { unreachable!() };
    let half = sim.half();
    let x0 = 20.0;
    let shooter = sim.shooter(0, Vec2::new(x0, 48.0), EAST, w);
    let first_x = x0 + 2.0 * half + def.min_range + 1.0;
    let line: Vec<Entity> = (0..pierce + 2).map(|i| sim.dummy(1, Vec2::new(first_x + i as f32 * (2.0 * half + 0.3), 48.0))).collect();
    let target = *line.last().unwrap();
    assert!(sim.app.world().get::<Position>(target).unwrap().0.x - x0 < def.range, "test layout fits the range");
    sim.attack(shooter, target, false);

    sim.tick_until(90, |s| s.taken(line[0]) > 0.0).expect("the shot is fired");
    sim.tick(30);
    for (i, e) in line.iter().enumerate() {
        let expected = if i <= pierce as usize { sim.hit_on(w, *e) } else { 0.0 };
        assert!(close(sim.taken(*e), expected, 1e-3), "line[{i}] took {} expected {expected}", sim.taken(*e));
    }
}

#[test]
fn homing_missile_steers_onto_a_moving_target() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Homing { .. }) && d.volley == 1 && d.targets.ground && d.min_range == 0.0);
    let def = sim.data.weapon(w).clone();
    let shooter = sim.shooter(0, Vec2::new(30.0, 40.0), EAST, w);
    let target = sim.dummy(1, Vec2::new(30.0 + 2.0 * sim.half() + def.range * 0.6, 40.0));
    sim.attack(shooter, target, false);
    sim.tick_until(90, |s| s.app.world().iter_entities().any(|e| e.contains::<FxBox>())).expect("a missile is launched");
    // The target walks sideways at 1.5 m/s while the missile is in flight.
    let velocity = Vec2::new(0.0, 1.5);
    sim.set(target, Velocity(velocity));
    let mut hit = false;
    for _ in 0..30 * 8 {
        sim.app.world_mut().get_mut::<Position>(target).unwrap().0 += velocity * DT;
        sim.tick(1);
        if sim.taken(target) > 0.0 {
            hit = true;
            break;
        }
    }
    assert!(hit, "the missile catches its target");
    assert!(sim.taken(target) >= sim.hit_on(w, target) - 1e-3);
}

#[test]
fn spray_hits_the_cone_in_front_but_not_behind() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Spray { cone_deg, .. } if cone_deg >= 35.0) && d.targets.ground && !d.friendly_fire && d.range >= 4.0);
    let def = sim.data.weapon(w).clone();
    let half = sim.half();
    let at = Vec2::new(48.0, 48.0);
    let shooter = sim.shooter(0, at, EAST, w);
    let ahead = sim.dummy(1, at + Vec2::new(2.0 * half + def.range * 0.5, 0.0));
    let beside = sim.dummy(1, at + Vec2::new(2.0 * half + def.range * 0.5, 2.0 * half + 0.1));
    let behind = sim.dummy(1, at - Vec2::new(2.0 * half + def.range * 0.3, 0.0));
    sim.attack(shooter, ahead, false);

    sim.tick_until(60, |s| s.taken(ahead) > 0.0).expect("the spray fires");
    assert!(close(sim.taken(ahead), sim.hit_on(w, ahead), 1e-3));
    assert!(close(sim.taken(beside), sim.hit_on(w, beside), 1e-3), "the cone catches the neighbour once");
    assert_eq!(sim.taken(behind), 0.0);
}

#[test]
fn swing_cleaves_enemies_inside_its_arc() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| {
        matches!(d.kind, WeaponKind::Melee { style: MeleeStyle::Swing, cleave, arc_deg } if cleave >= 1 && arc_deg >= 60.0) && d.targets.ground && d.splash == 0.0
    });
    let def = sim.data.weapon(w).clone();
    let half = sim.half();
    let at = Vec2::new(48.0, 48.0);
    let shooter = sim.shooter(0, at, EAST, w);
    let gap = def.range * 0.5;
    let target = sim.dummy(1, at + Vec2::new(2.0 * half + gap, 0.0));
    let beside = sim.dummy(1, at + Vec2::new(2.0 * half + gap, 2.0 * half + 0.05));
    let behind = sim.dummy(1, at - Vec2::new(2.0 * half + gap, 0.0));
    sim.attack(shooter, target, false);

    sim.tick_until(60, |s| s.taken(target) > 0.0).expect("the swing lands");
    assert!(close(sim.taken(beside), sim.hit_on(w, beside), 1e-3), "cleaves the neighbour in the arc");
    assert_eq!(sim.taken(behind), 0.0);
}

#[test]
fn units_turn_to_face_their_target_before_firing() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Bolt { .. }) && !d.fire_on_move && d.targets.ground && d.min_range == 0.0 && d.range >= 6.0);
    let range = sim.data.weapon(w).range;
    let shooter = sim.shooter(0, Vec2::new(30.0, 48.0), WEST, w);
    let target = sim.dummy(1, Vec2::new(30.0 + 2.0 * sim.half() + range * 0.5, 48.0));
    sim.attack(shooter, target, false);
    let turn_rate = sim.app.world().get::<Mobile>(shooter).unwrap().turn_rate;
    let facing = |s: &Sim| s.app.world().get::<Facing>(shooter).unwrap().0;

    sim.tick(2);
    assert!(facing(&sim) != WEST, "it starts turning");
    let fired = sim.tick_until(90, |s| s.app.world().iter_entities().any(|e| e.contains::<FxBox>())).unwrap() + 2;
    let min_turn = (PI - PI / 6.0) / turn_rate;
    assert!(fired as f32 * DT >= min_turn, "fired after {} s; turning needs {min_turn} s", fired as f32 * DT);
    sim.tick(30);
    assert!(af_geom::wrap_angle(facing(&sim) - EAST).abs() < 0.05, "faces the target");
}

/// Two long riders standing side by side, each turning toward a target on
/// the other's side, swing their fronts into each other: each turns only as
/// far as the other, turned already, leaves room, so together they never
/// cut deeper than one turn's slack (nothing pushes them apart here).
#[test]
fn neighbours_turning_into_each_other_share_the_room() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Bolt { .. }) && !d.fire_on_move && d.targets.ground && d.min_range == 0.0 && d.range >= 6.0);
    let rider = unit_where(&sim.data, |u| u.locomotion == MoveFlag::Mounted && u.size[2] >= 1.6 * u.size[0]);
    let size = sim.data.unit(rider).size;
    let mut riders = Vec::new();
    for (x, target_x) in [(30.0, 80.0), (30.0 + size[0] + 0.3, 5.0)] {
        let e = sim.unit(0, rider, Vec2::new(x, 48.0), 0.0);
        sim.arm(e, &[w]);
        sim.toughen(e);
        let target = sim.dummy(1, Vec2::new(target_x, 48.0));
        sim.attack(e, target, false);
        riders.push(e);
    }
    let obb = |s: &Sim, e: Entity| {
        let world = s.app.world();
        obb_of(world.get::<Position>(e).unwrap(), world.get::<Facing>(e).unwrap(), world.get::<Footprint>(e).unwrap())
    };
    let mut deepest = 0.0f32;
    for _ in 0..60 {
        sim.tick(1);
        deepest = deepest.max(obb(&sim, riders[0]).mtv(&obb(&sim, riders[1])).map_or(0.0, |m| m.length()));
    }
    let turned = |e| sim.app.world().get::<Facing>(e).unwrap().0.abs();
    assert!(turned(riders[0]) > 0.01 && turned(riders[1]) > 0.01, "both turn as far as there is room: {} {}", turned(riders[0]), turned(riders[1]));
    assert!(deepest <= 0.03 + 1e-3, "they cut {deepest} m into each other");
}

#[test]
fn passive_units_hold_fire_unless_ordered() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Bolt { .. }) && d.targets.ground && d.min_range == 0.0 && d.range >= 6.0);
    let range = sim.data.weapon(w).range;
    let shooter = sim.shooter(0, Vec2::new(30.0, 48.0), EAST, w);
    sim.set(shooter, Stance::Passive);
    let target = sim.dummy(1, Vec2::new(30.0 + 2.0 * sim.half() + range * 0.5, 48.0));
    sim.tick(30 * 2);
    assert_eq!(sim.taken(target), 0.0, "passive units never auto-fire");

    sim.attack(shooter, target, false);
    sim.tick(30 * 3);
    assert!(sim.taken(target) > 0.0, "but they obey an attack order");
}

#[test]
fn attack_target_is_dropped_when_its_target_dies() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Bolt { .. }) && d.targets.ground && d.range >= 6.0);
    let shooter = sim.shooter(0, Vec2::new(30.0, 48.0), EAST, w);
    let target = sim.dummy(1, Vec2::new(36.0, 48.0));
    sim.attack(shooter, target, false);
    sim.tick(1);
    sim.write(target, DUMMY_HP * 2.0, DamageType::Crush, None, None);
    sim.tick(2);
    assert!(sim.app.world().get::<AttackTarget>(shooter).is_none());
}

#[test]
fn under_attack_alerts_are_throttled() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Bolt { .. }) && d.targets.ground && d.cooldown <= 2.0 && d.range >= 6.0);
    let range = sim.data.weapon(w).range;
    let mut alerts = Watch::<UnderAttack>::new(&sim.app);
    sim.shooter(0, Vec2::new(30.0, 48.0), EAST, w);
    let target = sim.dummy(1, Vec2::new(30.0 + 2.0 * sim.half() + range * 0.5, 48.0));
    let mut seen = Vec::new();
    for _ in 0..(30 * 4) {
        sim.tick(1);
        seen.extend(alerts.take(&sim.app));
    }
    assert!(sim.taken(target) > sim.hit_on(w, target), "hit several times");
    assert_eq!(seen.len(), 1, "one alert per 5 s and area: {seen:?}");
    assert_eq!(seen[0].victim_owner, sim.players[1]);
    assert_eq!(seen[0].attacker_owner, Some(sim.players[0]));
}

#[test]
fn hp_regenerates_and_last_attacker_is_recorded() {
    let mut sim = Sim::new();
    let attacker = sim.dummy(0, Vec2::new(40.0, 48.0));
    let unit = sim.dummy(1, Vec2::new(44.0, 48.0));
    sim.app.world_mut().get_mut::<Stats>(unit).unwrap().regen = 3.0;
    sim.tick(1);
    sim.write(unit, 100.0, DamageType::Slash, Some(attacker), Some(0));
    sim.tick(1);
    let after_hit = sim.taken(unit);
    let last = *sim.app.world().get::<LastAttacker>(unit).expect("recorded");
    assert_eq!(last.entity, Some(attacker));
    assert_eq!(last.player, Some(sim.players[0]));
    assert_eq!(last.pos, Vec2::new(40.0, 48.0));
    sim.tick(30);
    assert!(close(after_hit - sim.taken(unit), 3.0, 0.15), "regenerated {} hp in 1 s", after_hit - sim.taken(unit));
}

#[test]
fn every_weapon_in_the_content_lands_a_hit() {
    let data = test_data();
    let mut sim = Sim::sized(data.clone(), 300.0);
    let half = sim.half();
    let flyer_kind = unit_where(&data, |u| u.is_flying() && u.shield.is_none());
    let block = building_where(&data, |b| b.weapon_ids.is_empty() && b.shield.is_none() && !b.wall && b.size[0] <= 5.0 && b.requires_node.is_none());
    let mut cases = Vec::new();
    for (i, def) in data.weapons.iter().enumerate() {
        let w = WeaponId(i as u16);
        let at = Vec2::new(15.0 + (i % 11) as f32 * 25.0, 15.0 + (i / 11) as f32 * 25.0);
        let gap = (def.min_range + def.range) * 0.5;
        let target = if matches!(def.kind, WeaponKind::Bomb { .. }) {
            let flyer = sim.unit(0, flyer_kind, at, EAST);
            sim.arm(flyer, &[w]);
            sim.toughen(flyer);
            let target = sim.dummy(1, at);
            sim.attack(flyer, target, false);
            target
        } else {
            let shooter = sim.shooter(0, at, EAST, w);
            let target = if def.targets.ground {
                sim.dummy(1, at + Vec2::new(2.0 * half + gap, 0.0))
            } else if def.targets.air {
                sim.dummy_of(1, flyer_kind, at + Vec2::new(half + data.unit(flyer_kind).size[2] * 0.5 + gap, 0.0))
            } else {
                sim.building(1, block, at + Vec2::new(half + data.building(block).size[0] * 0.5 + gap, 0.0), true)
            };
            sim.attack(shooter, target, false);
            target
        };
        cases.push((def.id.clone(), target, sim.hp(target).unwrap()));
    }
    sim.tick(30 * 6);
    let missed: Vec<&str> = cases.iter().filter(|(_, e, hp)| sim.hp(*e).is_some_and(|now| now >= *hp)).map(|(id, _, _)| id.as_str()).collect();
    assert!(missed.is_empty(), "these weapons never damaged their target: {missed:?}");
}

#[test]
fn the_same_battle_twice_ends_identically() {
    fn battle() -> Vec<(f32, f32)> {
        let mut sim = Sim::new();
        let data = sim.data.clone();
        // One unit per weapon kind present on ground units, facing each other.
        let mut kinds: Vec<UnitTypeId> = Vec::new();
        for u in data.unit_ids() {
            let d = data.unit(u);
            let Some(w) = d.weapon_ids.first() else { continue };
            let kind = std::mem::discriminant(&data.weapon(*w).kind);
            if !d.is_flying() && d.size[0] <= 3.0 && !kinds.iter().any(|k| std::mem::discriminant(&data.weapon(data.unit(*k).weapon_ids[0]).kind) == kind) {
                kinds.push(u);
            }
        }
        let mut all = Vec::new();
        for (i, k) in kinds.iter().enumerate() {
            let z = 20.0 + i as f32 * 4.0;
            all.push(sim.unit(0, *k, Vec2::new(40.0, z), EAST));
            all.push(sim.unit(1, *k, Vec2::new(49.0, z + 1.0), WEST));
        }
        sim.tick(30 * 15);
        let world = sim.app.world();
        // (hp, max) per unit; a dead unit reads (-1, 0).
        all.iter().map(|e| world.get::<Health>(*e).map_or((-1.0, 0.0), |h| (h.hp, h.max))).collect()
    }
    let first = battle();
    assert!(first.iter().filter(|(hp, max)| hp < max).count() >= 2, "both sides fought: {first:?}");
    assert_eq!(first, battle());
}

#[test]
fn attack_reach_reports_what_a_unit_can_engage() {
    use af_core::ext::combat::{attack_reach, TargetClass};
    let mut sim = Sim::new();
    let data = sim.data.clone();
    let melee = weapon_where(&data, |d| matches!(d.kind, WeaponKind::Melee { .. }) && !d.targets.air && d.targets.ground);
    let bow = weapon_where(&data, |d| matches!(d.kind, WeaponKind::Bolt { .. }) && d.targets.air && d.targets.ground && d.range > data.weapon(melee).range);
    let e = sim.shooter(0, Vec2::new(48.0, 48.0), 0.0, melee);
    sim.arm(e, &[melee, bow]);
    let world = sim.app.world();
    let (stats, weapons) = (world.get::<Stats>(e).unwrap(), world.get::<Weapons>(e).unwrap());
    assert_eq!(attack_reach(&data, stats, weapons, TargetClass::Ground), Some(data.weapon(bow).range));
    assert_eq!(attack_reach(&data, stats, weapons, TargetClass::Air), Some(data.weapon(bow).range));
    let melee_only = Weapons { slots: vec![WeaponSlot::new(melee)] };
    assert_eq!(attack_reach(&data, stats, &melee_only, TargetClass::Air), None);
}

#[test]
fn only_fire_on_move_weapons_shoot_while_under_way() {
    let mut sim = Sim::new();
    let data = sim.data.clone();
    let bolt = |d: &WeaponDef| matches!(d.kind, WeaponKind::Bolt { .. }) && d.targets.ground && d.min_range == 0.0 && d.range >= 6.0;
    let fixed = weapon_where(&data, |d| bolt(d) && !d.fire_on_move && d.windup >= 3.0 * DT);
    let mobile = weapon_where(&data, |d| bolt(d) && d.fire_on_move);
    let gap = 2.0 * sim.half() + 3.0;

    // Walking at an ordered target in range, facing it.
    let archer = sim.shooter(0, Vec2::new(30.0, 20.0), EAST, fixed);
    let archer_target = sim.dummy(1, Vec2::new(30.0 + gap, 20.0));
    let rider = sim.shooter(0, Vec2::new(30.0, 45.0), EAST, mobile);
    let rider_target = sim.dummy(1, Vec2::new(30.0 + gap, 45.0));
    for (shooter, target) in [(archer, archer_target), (rider, rider_target)] {
        sim.attack(shooter, target, false);
        sim.walk(shooter);
    }
    sim.tick(30 * 4);
    assert_eq!(sim.taken(archer_target), 0.0, "a weapon without fire_on_move holds fire on the move");
    assert!(sim.taken(rider_target) > 0.0, "a fire_on_move weapon shoots on the move");
    sim.stop(archer);
    sim.tick(30 * 3);
    assert!(sim.taken(archer_target) > 0.0, "and shoots once its host stops");

    // Setting off during the windup wastes the attack.
    let drawer = sim.shooter(0, Vec2::new(30.0, 70.0), EAST, fixed);
    let drawer_target = sim.dummy(1, Vec2::new(30.0 + gap, 70.0));
    sim.attack(drawer, drawer_target, false);
    sim.tick_until(60, |s| s.slots(drawer)[0].windup > 0.0).expect("the attack winds up");
    sim.walk(drawer);
    sim.tick(30 * 3);
    assert_eq!(sim.taken(drawer_target), 0.0, "no shot is released on the move");
    let slot = sim.slots(drawer)[0];
    assert!(slot.windup == 0.0 && slot.target.is_none(), "{slot:?}");
}

#[test]
fn a_jostled_unit_still_counts_as_standing() {
    // Collision push-outs show in Velocity; standing is about intent.
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Bolt { .. }) && !d.fire_on_move && d.targets.ground && d.min_range == 0.0 && d.range >= 6.0);
    let shooter = sim.shooter(0, Vec2::new(30.0, 48.0), WEST, w);
    let target = sim.dummy(1, Vec2::new(30.0 + 2.0 * sim.half() + 3.0, 48.0));
    sim.set(shooter, Velocity(Vec2::new(0.0, 1.0)));
    sim.tick(30 * 3);
    assert!(sim.taken(target) > 0.0, "turns to face and auto-fires while being nudged");
}

#[test]
fn reach_is_measured_corner_to_corner() {
    // A turret weapon (the body never turns), boxes offset diagonally: the
    // separating-axis gap reads 0.92 of the range when the true edge
    // distance is 1.3 times it.
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Bolt { .. }) && d.fire_on_move && d.targets.ground && d.min_range == 0.0);
    let range = sim.data.weapon(w).range;
    let span = 2.0 * sim.half();
    let place = |sim: &mut Sim, at: Vec2, factor: f32| {
        let shooter = sim.shooter(0, at, 0.0, w);
        let offset = span + factor * range / std::f32::consts::SQRT_2;
        let target = sim.dummy(1, at + Vec2::splat(offset));
        sim.set(shooter, Stance::Passive);
        sim.attack(shooter, target, false);
        target
    };
    let beyond = place(&mut sim, Vec2::new(10.0, 10.0), 1.3);
    let within = place(&mut sim, Vec2::new(10.0, 50.0), 0.9);
    sim.tick(30 * 4);
    assert_eq!(sim.taken(beyond), 0.0, "1.3x range on the diagonal is out of reach");
    assert!(sim.taken(within) > 0.0, "0.9x range on the diagonal is in reach");
}

#[test]
fn forced_area_attacks_hit_the_friend_they_are_aimed_at() {
    // Exact shots, so only the forced target and its splash matter.
    let mut data = (*test_data()).clone();
    let arc = weapon_where(&data, |d| matches!(d.kind, WeaponKind::Arc { .. }) && !d.friendly_fire && d.targets.ground && d.range >= 5.0);
    let cannon = weapon_where(&data, |d| matches!(d.kind, WeaponKind::Cannon { .. }) && d.targets.ground && d.range >= 8.0);
    let bomb = weapon_where(&data, |d| matches!(d.kind, WeaponKind::Bomb { .. }) && d.targets.ground);
    for w in [arc, cannon, bomb] {
        data.weapons[w.index()].spread = 0.0;
    }
    let mut sim = Sim::with_data(Arc::new(data));
    let data = sim.data.clone();
    let half = sim.half();
    let gap = |w: WeaponId| 2.0 * half + (data.weapon(w).min_range + data.weapon(w).range) * 0.5;

    // An Arc at an ally, with another ally standing beside it.
    let lobber = sim.shooter(0, Vec2::new(15.0, 20.0), EAST, arc);
    let ally = sim.dummy(2, Vec2::new(15.0 + gap(arc), 20.0));
    let bystander = sim.dummy(2, Vec2::new(15.0 + gap(arc), 20.0 + 2.0 * half + 0.2));
    sim.attack(lobber, ally, true);
    // A Cannon at one of the shooter's own units.
    let gunner = sim.shooter(0, Vec2::new(15.0, 45.0), EAST, cannon);
    let own = sim.dummy(0, Vec2::new(15.0 + gap(cannon), 45.0));
    sim.attack(gunner, own, true);
    // A bomber over an ally.
    let flyer_kind = unit_where(&data, |u| u.is_flying() && u.shield.is_none());
    let bomber = sim.unit(0, flyer_kind, Vec2::new(60.0, 70.0), EAST);
    sim.arm(bomber, &[bomb]);
    sim.toughen(bomber);
    let under = sim.dummy(2, Vec2::new(60.0, 70.0));
    sim.attack(bomber, under, true);

    sim.tick(30 * 8);
    assert!(sim.taken(ally) > 0.0, "a forced Arc hits the ally it targets");
    assert!(sim.taken(own) > 0.0, "a forced Cannon hits the own unit it targets");
    assert!(sim.taken(under) > 0.0, "a forced Bomb hits the ally it targets");
    assert_eq!(sim.taken(bystander), 0.0, "other allies stay spared without friendly fire");
}

#[test]
fn a_missile_that_loses_its_flyer_bursts_in_the_air() {
    let mut data = (*test_data()).clone();
    let w = weapon_where(&data, |d| matches!(d.kind, WeaponKind::Homing { .. }) && d.targets.air && d.min_range == 0.0 && d.range >= 6.0);
    let def = &mut data.weapons[w.index()];
    def.splash = def.splash.max(3.0);
    def.spread = 0.0;
    let range = def.range;
    let mut sim = Sim::with_data(Arc::new(data));
    let data = sim.data.clone();
    let flyer_kind = unit_where(&data, |u| u.is_flying() && u.shield.is_none());
    let flyer_half = data.unit(flyer_kind).size[2] * 0.5;

    let shooter = sim.shooter(0, Vec2::new(30.0, 48.0), EAST, w);
    sim.set(shooter, Stance::Passive);
    let at = Vec2::new(30.0 + sim.half() + flyer_half + range * 0.6, 48.0);
    let flyer = sim.dummy_of(1, flyer_kind, at);
    let wingman = sim.dummy_of(1, flyer_kind, at + Vec2::new(0.0, 1.5));
    let below = sim.dummy(1, at);
    sim.attack(shooter, flyer, false);

    sim.tick_until(90, |s| s.app.world().iter_entities().any(|e| e.contains::<FxBox>())).expect("a missile is launched");
    sim.write(flyer, DUMMY_HP * 10.0, DamageType::Crush, None, None);
    sim.tick(30 * 4);
    assert!(sim.hp(flyer).is_none(), "the flyer died while the missile was in flight");
    assert!(sim.taken(wingman) > 0.0, "the burst at the flyer's height catches the flyer beside it");
    assert_eq!(sim.taken(below), 0.0, "an air burst does not reach the ground under it");
}

#[test]
fn nova_reach_grows_with_range_techs_but_its_area_does_not() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| matches!(d.kind, WeaponKind::Nova) && d.targets.ground && !d.friendly_fire && d.range >= 3.0);
    let area = sim.data.weapon(w).range;
    let at = Vec2::new(48.0, 48.0);
    let shooter = sim.shooter(0, at, 0.0, w);
    sim.app.world_mut().get_mut::<Stats>(shooter).unwrap().range_add = 2.0;
    let span = 2.0 * sim.half();
    let aimed_at = sim.dummy(1, at + Vec2::new(span + area + 1.0, 0.0));
    let farther = sim.dummy(1, at - Vec2::new(span + area + 1.5, 0.0));

    sim.tick_until(60, |s| s.taken(aimed_at) > 0.0).expect("the longer reach acquires a target beyond the area");
    assert!(close(sim.taken(aimed_at), sim.hit_on(w, aimed_at), 1e-3), "the target is hit once");
    assert_eq!(sim.taken(farther), 0.0, "the ring itself does not grow");
}

#[test]
fn a_new_match_starts_with_fresh_combat_state() {
    let mut sim = Sim::new();
    let w = weapon_where(&sim.data, |d| {
        matches!(d.kind, WeaponKind::Bolt { .. }) && d.volley == 1 && d.spread > 0.0 && d.targets.ground && d.cooldown <= 2.0 && d.range >= 6.0
    });
    let range = sim.data.weapon(w).range;
    let mut alerts = Watch::<UnderAttack>::new(&sim.app);
    // One skirmish: the first bolt one tick into its flight, and the alerts.
    let skirmish = |sim: &mut Sim, alerts: &mut Watch<UnderAttack>, secs: u32| {
        let shooter = sim.shooter(0, Vec2::new(30.0, 48.0), EAST, w);
        let target = sim.dummy(1, Vec2::new(30.0 + 2.0 * sim.half() + range * 0.5, 48.0));
        sim.tick_until(90, |s| s.app.world().iter_entities().any(|e| e.contains::<FxBox>())).expect("a bolt is launched");
        sim.tick(1);
        let bolt = sim.fx_boxes()[0];
        let mut seen = 0;
        for _ in 0..30 * secs {
            sim.tick(1);
            seen += alerts.take(&sim.app).len();
        }
        assert!(sim.taken(target) > 0.0);
        sim.app.world_mut().despawn(shooter);
        sim.app.world_mut().despawn(target);
        (bolt, seen)
    };
    let (first_bolt, first_alerts) = skirmish(&mut sim, &mut alerts, 7);
    assert_eq!(first_alerts, 2, "one alert per 5 s");

    // Back to the menu and into a new match, which restarts the clock.
    for state in [GameState::Menu, GameState::Playing] {
        sim.app.world_mut().resource_mut::<NextState<GameState>>().set(state);
        sim.tick(1);
    }
    sim.app.world_mut().insert_resource(SimClock::default());
    alerts.take(&sim.app);

    let (second_bolt, second_alerts) = skirmish(&mut sim, &mut alerts, 3);
    assert_eq!(second_alerts, 1, "the last match's alerts do not mute this one");
    assert_eq!(second_bolt, first_bolt, "effect randomness restarts with the match");
}
