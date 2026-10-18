//! Bring a player's existing entities in line with newly researched techs
//! (`SimSet::Economy`, after production): effective stats, max hp with the
//! hp fraction kept, shields, carry capacity, build and train times, and
//! AoE-style line upgrades that convert units in the field and in
//! production queues.

use af_core::prelude::*;
use bevy::ecs::query::QueryData;
use bevy::prelude::*;

#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct UnitView {
    entity: Entity,
    owner: &'static Owner,
    unit: &'static mut Unit,
    name: &'static mut Name,
    footprint: &'static mut Footprint,
    altitude: &'static mut Altitude,
    mobile: &'static mut Mobile,
    weapons: &'static mut Weapons,
    stats: &'static mut Stats,
    health: &'static mut Health,
    shield: Option<&'static mut Shield>,
    gatherer: Option<&'static mut Gatherer>,
}

#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct BuildingView {
    entity: Entity,
    owner: &'static Owner,
    building: &'static Building,
    stats: &'static mut Stats,
    health: &'static mut Health,
    shield: Option<&'static mut Shield>,
    construction: Option<&'static mut UnderConstruction>,
    queue: Option<&'static mut ProductionQueue>,
}

/// Refresh every entity of each player who completed research this tick.
pub(crate) fn refresh_researched(
    data: Res<Data>,
    players: Res<Players>,
    mut commands: Commands,
    mut researched: MessageReader<TechResearched>,
    mut dirty: Local<Vec<bool>>,
    mut units: Query<UnitView, Without<Dead>>,
    mut buildings: Query<BuildingView, (Without<Unit>, Without<Dead>)>,
) {
    dirty.clear();
    dirty.resize(players.list.len(), false);
    let mut any = false;
    for msg in researched.read() {
        if let Some(d) = dirty.get_mut(msg.player.index()) {
            *d = true;
            any = true;
        }
    }
    if !any {
        return;
    }
    let data: &GameData = &data;
    let refreshed = |owner: &Owner| if dirty.get(owner.0.index()).copied().unwrap_or(false) { players.try_get(owner.0) } else { None };
    for u in &mut units {
        if let Some(p) = refreshed(u.owner) {
            refresh_unit(&mut commands, data, p, u);
        }
    }
    for b in &mut buildings {
        if let Some(p) = refreshed(b.owner) {
            refresh_building(&mut commands, data, p, b);
        }
    }
}

fn refresh_unit(commands: &mut Commands, data: &GameData, p: &PlayerState, mut u: UnitViewItem) {
    let kind = p.mods.upgraded_unit(u.unit.kind);
    let def = data.unit(kind);
    if kind != u.unit.kind {
        u.unit.kind = kind;
        *u.name = Name::new(def.name.clone());
        *u.footprint = Footprint::from_size(def.size);
        u.altitude.0 = def.altitude;
        // The same values spawn_unit derives from the def.
        *u.mobile = Mobile {
            mobility: data.unit_mobility(kind, p.faction),
            radius: u.footprint.half.x.max(0.2),
            turn_rate: def.turn_rate.to_radians(),
            flying: def.is_flying(),
        };
        u.weapons.slots = def.weapon_ids.iter().map(|w| WeaponSlot::new(*w)).collect();
    }
    let st = stats::unit_stats(data, p, kind);
    rescale_health(&mut u.health, st.max_hp);
    sync_shield(&mut commands.entity(u.entity), u.shield, def.shield.as_ref(), st.shield_max, true);
    if let Some(mut g) = u.gatherer {
        let capacity = stats::carry_capacity(data, p, kind);
        if g.capacity != capacity {
            g.capacity = capacity;
        }
    }
    if *u.stats != st {
        *u.stats = st;
    }
}

fn refresh_building(commands: &mut Commands, data: &GameData, p: &PlayerState, mut b: BuildingViewItem) {
    let kind = b.building.kind;
    let st = stats::building_stats(data, p, kind);
    rescale_health(&mut b.health, st.max_hp);
    let complete = b.construction.is_none();
    sync_shield(&mut commands.entity(b.entity), b.shield, data.building(kind).shield.as_ref(), st.shield_max, complete);
    if let Some(mut site) = b.construction {
        let build_time = stats::building_build_time(data, p, kind);
        if site.build_time != build_time {
            site.build_time = build_time;
        }
    }
    // Queued trainings follow line upgrades and train-time techs; what was
    // paid stays, so cancelling still refunds exactly that.
    let refreshed = |item: &QueueItem| match item.kind {
        QueueKind::Unit(u) => {
            let u = p.mods.upgraded_unit(u);
            Some(QueueItem { kind: QueueKind::Unit(u), time: stats::unit_train_time(data, p, u), ..*item })
        }
        QueueKind::Tech(_) | QueueKind::Upgrade(_) => None,
    };
    if let Some(mut queue) = b.queue
        && queue.items.iter().any(|item| refreshed(item).is_some_and(|new| new != *item))
    {
        for item in queue.items.iter_mut() {
            if let Some(new) = refreshed(item) {
                *item = new;
            }
        }
    }
    if *b.stats != st {
        *b.stats = st;
    }
}

/// Set `health.max`, keeping the hp fraction.
pub(crate) fn rescale_health(health: &mut Mut<Health>, max: f32) {
    if health.max != max {
        let fraction = health.fraction();
        health.max = max;
        health.hp = fraction * max;
    }
}

/// Bring a shield to `max` keeping its fill fraction (and to the def's regen
/// parameters), add one when the entity gains a shield (full when
/// `full_when_new`, else empty, like a foundation's) and remove it when
/// `max` drops to zero.
pub(crate) fn sync_shield(entity: &mut EntityCommands, shield: Option<Mut<Shield>>, def: Option<&ShieldDef>, max: f32, full_when_new: bool) {
    let fresh = |hp: f32| Shield {
        hp,
        max,
        regen: def.map_or(0.0, |s| s.regen),
        delay: def.map_or(4.0, |s| s.delay),
        energy_resist: def.map_or(0.0, |s| s.energy_resist),
        since_hit: 999.0,
    };
    match shield {
        Some(mut s) if max > 0.0 => {
            let hp = if s.max == max {
                s.hp
            } else if s.max > 0.0 {
                s.hp / s.max * max
            } else {
                max
            };
            let want = Shield { since_hit: s.since_hit, ..fresh(hp.clamp(0.0, max)) };
            if *s != want {
                *s = want;
            }
        }
        Some(_) => {
            entity.try_remove::<Shield>();
        }
        None if max > 0.0 => {
            entity.try_insert(fresh(if full_when_new { max } else { 0.0 }));
        }
        None => {}
    }
}
