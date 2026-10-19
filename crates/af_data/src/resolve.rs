//! Merge content packs, resolve string references to typed ids, validate.

use std::collections::{HashMap, HashSet};

use crate::ids::*;
use crate::schema::*;
use crate::{DataErrors, GameData};

struct Ctx {
    errors: Vec<String>,
}

impl Ctx {
    fn err(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && id.chars().next().is_some_and(|c| c.is_ascii_lowercase())
}

/// Build an id index for one category, reporting bad and duplicate ids.
fn index<T, I: Copy>(
    ctx: &mut Ctx,
    sources: &mut HashMap<String, String>,
    category: &str,
    items: &[(String, T)],
    id_of: impl Fn(&T) -> &str,
    make: impl Fn(usize) -> I,
) -> HashMap<String, I> {
    let mut map = HashMap::new();
    for (i, (src, item)) in items.iter().enumerate() {
        let id = id_of(item);
        if !valid_id(id) {
            ctx.err(format!("{src}: {category} id `{id}` must be snake_case ([a-z][a-z0-9_]*)"));
        }
        let key = format!("{category}:{id}");
        if let Some(prev) = sources.get(&key) {
            ctx.err(format!("{src}: duplicate {category} id `{id}` (first defined in {prev})"));
            continue;
        }
        sources.insert(key, src.clone());
        map.insert(id.to_string(), make(i));
    }
    map
}

fn check_color(ctx: &mut Ctx, what: &str, c: &Rgb) {
    if c.iter().any(|v| !(0.0..=1.0).contains(v)) {
        ctx.err(format!("{what}: colour components must be in 0..=1, got {c:?}"));
    }
}

fn check_size(ctx: &mut Ctx, what: &str, s: &BoxSize) {
    if s.iter().any(|v| *v <= 0.0 || !v.is_finite()) {
        ctx.err(format!("{what}: size components must be > 0, got {s:?}"));
    }
}

fn check_age(ctx: &mut Ctx, what: &str, age: u8) {
    if !(1..=MAX_AGE).contains(&age) {
        ctx.err(format!("{what}: age must be 1..={MAX_AGE}, got {age}"));
    }
}

fn check_armor(ctx: &mut Ctx, what: &str, a: &Armor) {
    for (n, v) in [("slash", a.slash), ("pierce", a.pierce), ("crush", a.crush), ("fire", a.fire), ("energy", a.energy)] {
        if !(0.0..=0.9).contains(&v) {
            ctx.err(format!("{what}: armor.{n} must be in 0.0..=0.9 (fractional resist), got {v}"));
        }
    }
}

fn resolve_cost(ctx: &mut Ctx, what: &str, cost: &mut Cost, res: &HashMap<String, ResourceId>) {
    let mut amounts = ResAmounts::ZERO;
    for (k, v) in &cost.entries {
        match res.get(k) {
            Some(r) => amounts.set(*r, *v as f32),
            None => ctx.err(format!("{what}: cost uses unknown resource `{k}`")),
        }
    }
    cost.amounts = amounts;
}

fn look<I: Copy>(ctx: &mut Ctx, what: &str, kind: &str, map: &HashMap<String, I>, id: &str) -> Option<I> {
    let r = map.get(id).copied();
    if r.is_none() {
        ctx.err(format!("{what}: unknown {kind} `{id}`"));
    }
    r
}

fn look_all<I: Copy>(ctx: &mut Ctx, what: &str, kind: &str, map: &HashMap<String, I>, ids: &[String]) -> Vec<I> {
    ids.iter().filter_map(|id| look(ctx, what, kind, map, id)).collect()
}

fn check_selector(ctx: &mut Ctx, what: &str, sel: &Selector, units: &HashMap<String, UnitTypeId>, buildings: &HashMap<String, BuildingTypeId>) {
    match sel {
        Selector::Unit(u) => {
            look(ctx, what, "unit", units, u);
        }
        Selector::Building(b) => {
            look(ctx, what, "building", buildings, b);
        }
        Selector::AllUnits | Selector::AllBuildings | Selector::Tag(_) => {}
    }
}

pub(crate) fn build(packs: Vec<(String, ContentPack)>) -> Result<GameData, DataErrors> {
    let mut ctx = Ctx { errors: Vec::new() };
    let mut sources = HashMap::new();

    // ---- merge, remembering each def's source file ----
    let mut resources = Vec::new();
    let mut ages = Vec::new();
    let mut terrain = Vec::new();
    let mut nodes = Vec::new();
    let mut weapons = Vec::new();
    let mut units = Vec::new();
    let mut buildings = Vec::new();
    let mut techs = Vec::new();
    let mut factions = Vec::new();
    for (src, p) in packs {
        resources.extend(p.resources.into_iter().map(|d| (src.clone(), d)));
        ages.extend(p.ages.into_iter().map(|d| (src.clone(), d)));
        terrain.extend(p.terrain.into_iter().map(|d| (src.clone(), d)));
        nodes.extend(p.nodes.into_iter().map(|d| (src.clone(), d)));
        weapons.extend(p.weapons.into_iter().map(|d| (src.clone(), d)));
        units.extend(p.units.into_iter().map(|d| (src.clone(), d)));
        buildings.extend(p.buildings.into_iter().map(|d| (src.clone(), d)));
        techs.extend(p.techs.into_iter().map(|d| (src.clone(), d)));
        factions.extend(p.factions.into_iter().map(|d| (src.clone(), d)));
    }

    // Drop duplicates (keeping the first) so indices stay consistent.
    fn dedup<T>(items: Vec<(String, T)>, id_of: impl Fn(&T) -> String) -> Vec<(String, T)> {
        let mut seen = HashSet::new();
        items.into_iter().filter(|(_, d)| seen.insert(id_of(d))).collect()
    }

    // ---- counts ----
    if resources.is_empty() {
        ctx.err("no resources defined");
    }
    if resources.len() > MAX_RESOURCES {
        ctx.err(format!("{} resources defined; the engine supports at most {MAX_RESOURCES}", resources.len()));
    }
    if terrain.is_empty() {
        ctx.err("no terrain defined");
    }
    if terrain.len() > 255 {
        ctx.err("more than 255 terrain types");
    }
    if factions.is_empty() {
        ctx.err("no factions defined");
    }

    // ---- duplicate / malformed id detection on the raw lists ----
    let _ = index(&mut ctx, &mut sources, "resource", &resources, |d| &d.id, |i| i);
    let _ = index(&mut ctx, &mut sources, "terrain", &terrain, |d| &d.id, |i| i);
    let _ = index(&mut ctx, &mut sources, "age", &ages, |d| &d.id, |i| i);
    let _ = index(&mut ctx, &mut sources, "node", &nodes, |d| &d.id, |i| i);
    let _ = index(&mut ctx, &mut sources, "weapon", &weapons, |d| &d.id, |i| i);
    let _ = index(&mut ctx, &mut sources, "unit", &units, |d| &d.id, |i| i);
    let _ = index(&mut ctx, &mut sources, "building", &buildings, |d| &d.id, |i| i);
    let _ = index(&mut ctx, &mut sources, "tech", &techs, |d| &d.id, |i| i);
    let _ = index(&mut ctx, &mut sources, "faction", &factions, |d| &d.id, |i| i);

    // Keep the first of any duplicate so indices stay consistent, then index.
    let resources = dedup(resources, |d| d.id.clone());
    let terrain = dedup(terrain, |d| d.id.clone());
    let nodes = dedup(nodes, |d| d.id.clone());
    let weapons = dedup(weapons, |d| d.id.clone());
    let units = dedup(units, |d| d.id.clone());
    let buildings = dedup(buildings, |d| d.id.clone());
    let techs = dedup(techs, |d| d.id.clone());
    let factions = dedup(factions, |d| d.id.clone());
    fn ix<T, I>(items: &[(String, T)], id_of: impl Fn(&T) -> &str, make: impl Fn(usize) -> I) -> HashMap<String, I> {
        items.iter().enumerate().map(|(i, (_, d))| (id_of(d).to_string(), make(i))).collect()
    }
    let resource_ix = ix(&resources, |d| &d.id, |i| ResourceId(i as u8));
    let terrain_ix = ix(&terrain, |d| &d.id, |i| TerrainId(i as u8));
    let node_ix = ix(&nodes, |d| &d.id, |i| NodeTypeId(i as u16));
    let weapon_ix = ix(&weapons, |d| &d.id, |i| WeaponId(i as u16));
    let unit_ix = ix(&units, |d| &d.id, |i| UnitTypeId(i as u16));
    let building_ix = ix(&buildings, |d| &d.id, |i| BuildingTypeId(i as u16));
    let tech_ix = ix(&techs, |d| &d.id, |i| TechId(i as u16));
    let faction_ix = ix(&factions, |d| &d.id, |i| FactionId(i as u8));

    // ---- ages ----
    let mut ages: Vec<AgeDef> = ages.into_iter().map(|(_, d)| d).collect();
    ages.sort_by_key(|a| a.index);
    let indices: Vec<u8> = ages.iter().map(|a| a.index).collect();
    let expected: Vec<u8> = (1..=MAX_AGE).collect();
    if indices != expected {
        ctx.err(format!("ages must have indices exactly 1..={MAX_AGE} once each, got {indices:?}"));
    }

    // ---- resources ----
    let resources: Vec<ResourceDef> = resources
        .into_iter()
        .map(|(src, d)| {
            let what = format!("{src}: resource `{}`", d.id);
            check_color(&mut ctx, &what, &d.color);
            if d.work == WorkStyle::Farm {
                ctx.err(format!(
                    "{what}: work `Farm` is how resource-field buildings are worked; give its nodes a hand style (Forage, Chop, Mine, Dig, Chip or Smash)"
                ));
            }
            d
        })
        .collect();

    // ---- terrain ----
    let mut terrain_masks = Vec::new();
    let terrain: Vec<TerrainDef> = terrain
        .into_iter()
        .map(|(src, d)| {
            let what = format!("{src}: terrain `{}`", d.id);
            check_color(&mut ctx, &what, &d.color);
            if d.speed <= 0.0 {
                ctx.err(format!("{what}: speed must be > 0"));
            }
            for (_, m) in &d.speed_by {
                if *m <= 0.0 {
                    ctx.err(format!("{what}: speed_by multipliers must be > 0"));
                }
            }
            let mut mask = Mobility::of(MoveFlag::Flying);
            for f in &d.passable {
                mask = mask.with(*f);
            }
            terrain_masks.push(mask);
            d
        })
        .collect();

    // ---- nodes ----
    let nodes: Vec<NodeDef> = nodes
        .into_iter()
        .map(|(src, mut d)| {
            let what = format!("{src}: node `{}`", d.id);
            check_color(&mut ctx, &what, &d.color);
            check_size(&mut ctx, &what, &d.size);
            if d.amount <= 0.0 {
                ctx.err(format!("{what}: amount must be > 0"));
            }
            if let Some(r) = look(&mut ctx, &what, "resource", &resource_ix, &d.resource) {
                d.resource_id = r;
            }
            if let Some(b) = d.requires_building.clone() {
                d.requires_building_id = look(&mut ctx, &what, "building", &building_ix, &b);
                if d.blocking {
                    ctx.err(format!("{what}: nodes with requires_building must set blocking: false (the building is placed on top)"));
                }
            }
            d
        })
        .collect();

    // ---- weapons ----
    let weapons: Vec<WeaponDef> = weapons
        .into_iter()
        .map(|(src, d)| {
            let what = format!("{src}: weapon `{}`", d.id);
            if d.damage < 0.0 {
                ctx.err(format!("{what}: damage must be >= 0"));
            }
            if d.range <= 0.0 {
                ctx.err(format!("{what}: range must be > 0"));
            }
            if d.min_range < 0.0 || d.min_range >= d.range {
                ctx.err(format!("{what}: min_range must be in 0..range"));
            }
            if d.cooldown <= 0.0 {
                ctx.err(format!("{what}: cooldown must be > 0"));
            }
            if d.windup < 0.0 || d.windup > d.cooldown {
                ctx.err(format!("{what}: windup must be in 0..=cooldown"));
            }
            if d.volley == 0 {
                ctx.err(format!("{what}: volley must be >= 1"));
            }
            if d.splash < 0.0 {
                ctx.err(format!("{what}: splash must be >= 0"));
            }
            if !d.targets.ground && !d.targets.air && !d.targets.buildings {
                ctx.err(format!("{what}: targets nothing"));
            }
            for (_, m) in &d.bonus {
                if *m <= 0.0 {
                    ctx.err(format!("{what}: bonus multipliers must be > 0"));
                }
            }
            check_size(&mut ctx, &format!("{what} look"), &d.look.size);
            check_size(&mut ctx, &format!("{what} projectile"), &d.projectile.size);
            check_color(&mut ctx, &format!("{what} look"), &d.look.color);
            check_color(&mut ctx, &format!("{what} projectile"), &d.projectile.color);
            match &d.kind {
                WeaponKind::Melee { arc_deg, .. } => {
                    if *arc_deg < 0.0 || *arc_deg > 360.0 {
                        ctx.err(format!("{what}: arc_deg must be in 0..=360"));
                    }
                    if d.range > 4.0 {
                        ctx.err(format!("{what}: melee range {} is suspiciously long (max 4)", d.range));
                    }
                }
                WeaponKind::Bolt { speed } | WeaponKind::Arc { speed, .. } | WeaponKind::Cannon { speed, .. } => {
                    if *speed <= 0.0 {
                        ctx.err(format!("{what}: projectile speed must be > 0"));
                    }
                }
                WeaponKind::Homing { speed, turn_rate_deg } => {
                    if *speed <= 0.0 || *turn_rate_deg <= 0.0 {
                        ctx.err(format!("{what}: homing speed and turn_rate_deg must be > 0"));
                    }
                }
                WeaponKind::Beam { duration, width, ticks } => {
                    if *duration <= 0.0 || *width <= 0.0 || *ticks == 0 {
                        ctx.err(format!("{what}: beam duration/width must be > 0 and ticks >= 1"));
                    }
                }
                WeaponKind::Bomb { fall_time } => {
                    if *fall_time <= 0.0 {
                        ctx.err(format!("{what}: fall_time must be > 0"));
                    }
                }
                WeaponKind::Spray { cone_deg, particles } => {
                    if *cone_deg <= 0.0 || *particles == 0 {
                        ctx.err(format!("{what}: spray cone_deg must be > 0 and particles >= 1"));
                    }
                }
                WeaponKind::Chain { jump_range, .. } => {
                    if *jump_range <= 0.0 {
                        ctx.err(format!("{what}: chain jump_range must be > 0"));
                    }
                }
                WeaponKind::Nova => {}
            }
            d
        })
        .collect();

    // ---- units ----
    let units: Vec<UnitDef> = units
        .into_iter()
        .map(|(src, mut d)| {
            let what = format!("{src}: unit `{}`", d.id);
            check_age(&mut ctx, &what, d.age);
            check_size(&mut ctx, &what, &d.size);
            check_armor(&mut ctx, &what, &d.armor);
            if let Some(c) = &d.color {
                check_color(&mut ctx, &what, c);
            }
            if d.hp <= 0.0 {
                ctx.err(format!("{what}: hp must be > 0"));
            }
            if d.speed < 0.0 {
                ctx.err(format!("{what}: speed must be >= 0"));
            }
            if d.turn_rate <= 0.0 {
                ctx.err(format!("{what}: turn_rate must be > 0"));
            }
            if !d.locomotion.is_locomotion() {
                ctx.err(format!("{what}: locomotion must be Foot/Mounted/Wheeled/Tracked/Hover/Flying, got {:?}", d.locomotion));
            }
            if d.locomotion == MoveFlag::Flying && d.altitude <= 0.0 {
                ctx.err(format!("{what}: Flying units need altitude > 0"));
            }
            if d.vision <= 0.0 {
                ctx.err(format!("{what}: vision must be > 0"));
            }
            if d.train_time <= 0.0 {
                ctx.err(format!("{what}: train_time must be > 0"));
            }
            if d.squad == 0 {
                ctx.err(format!("{what}: squad must be >= 1"));
            }
            if d.trained_at.is_empty() {
                ctx.err(format!("{what}: trained_at is empty"));
            }
            resolve_cost(&mut ctx, &what, &mut d.cost, &resource_ix);
            d.weapon_ids = look_all(&mut ctx, &what, "weapon", &weapon_ix, &d.weapons);
            d.trained_at_ids = look_all(&mut ctx, &what, "building", &building_ix, &d.trained_at);
            if let Some(g) = &mut d.gatherer {
                if g.capacity <= 0.0 {
                    ctx.err(format!("{what}: gatherer.capacity must be > 0"));
                }
                let mut rates = ResAmounts::ZERO;
                for (r, v) in &g.rates {
                    if *v <= 0.0 {
                        ctx.err(format!("{what}: gather rate for `{r}` must be > 0"));
                    }
                    if let Some(id) = resource_ix.get(r) {
                        rates.set(*id, *v);
                    } else {
                        ctx.err(format!("{what}: gatherer rate for unknown resource `{r}`"));
                    }
                }
                g.rate_by_resource = rates;
            }
            if let Some(h) = &d.healer {
                if h.rate <= 0.0 || h.range <= 0.0 || h.tags.is_empty() {
                    ctx.err(format!("{what}: healer needs rate > 0, range > 0 and tags"));
                }
            }
            if let Some(s) = &d.shield {
                if s.hp <= 0.0 || s.regen < 0.0 || !(0.0..=1.0).contains(&s.energy_resist) {
                    ctx.err(format!("{what}: shield needs hp > 0, regen >= 0, energy_resist 0..=1"));
                }
            }
            d
        })
        .collect();

    // ---- buildings ----
    let buildings: Vec<BuildingDef> = buildings
        .into_iter()
        .map(|(src, mut d)| {
            let what = format!("{src}: building `{}`", d.id);
            check_age(&mut ctx, &what, d.age);
            check_size(&mut ctx, &what, &d.size);
            check_armor(&mut ctx, &what, &d.armor);
            if let Some(c) = &d.color {
                check_color(&mut ctx, &what, c);
            }
            if d.hp <= 0.0 {
                ctx.err(format!("{what}: hp must be > 0"));
            }
            if d.build_time <= 0.0 {
                ctx.err(format!("{what}: build_time must be > 0"));
            }
            if d.vision <= 0.0 {
                ctx.err(format!("{what}: vision must be > 0"));
            }
            if d.gate && !d.wall {
                ctx.err(format!("{what}: gate buildings must also be wall: true"));
            }
            resolve_cost(&mut ctx, &what, &mut d.cost, &resource_ix);
            d.drop_site_ids = look_all(&mut ctx, &what, "resource", &resource_ix, &d.drop_site);
            d.weapon_ids = look_all(&mut ctx, &what, "weapon", &weapon_ix, &d.weapons);
            let mut trickle = ResAmounts::ZERO;
            for (r, v) in &d.trickle {
                if let Some(id) = look(&mut ctx, &what, "resource", &resource_ix, r) {
                    trickle.add(id, *v);
                }
            }
            d.trickle_amounts = trickle;
            if let Some(n) = d.requires_node.clone() {
                d.requires_node_id = look(&mut ctx, &what, "node", &node_ix, &n);
            }
            if let Some(f) = &mut d.resource_field {
                if f.amount <= 0.0 || f.slots == 0 {
                    ctx.err(format!("{what}: resource_field needs amount > 0 and slots >= 1"));
                }
                if let Some(r) = resource_ix.get(&f.resource) {
                    f.resource_id = *r;
                } else {
                    ctx.err(format!("{what}: resource_field unknown resource `{}`", f.resource));
                }
            }
            let self_id = d.id.clone();
            if let Some(u) = &mut d.upgrade {
                if u.to == self_id {
                    ctx.err(format!("{what}: upgrade.to cannot be itself"));
                }
                if let Some(t) = building_ix.get(&u.to) {
                    u.to_id = *t;
                } else {
                    ctx.err(format!("{what}: upgrade.to unknown building `{}`", u.to));
                }
                let mut c = u.cost.clone();
                resolve_cost(&mut ctx, &format!("{what} upgrade"), &mut c, &resource_ix);
                u.cost = c;
                if u.time <= 0.0 {
                    ctx.err(format!("{what}: upgrade.time must be > 0"));
                }
            }
            d
        })
        .collect();

    // ---- techs ----
    let techs: Vec<TechDef> = techs
        .into_iter()
        .map(|(src, mut d)| {
            let what = format!("{src}: tech `{}`", d.id);
            check_age(&mut ctx, &what, d.age);
            if d.time <= 0.0 {
                ctx.err(format!("{what}: time must be > 0"));
            }
            if d.effects.is_empty() {
                ctx.err(format!("{what}: no effects"));
            }
            if d.is_age_up() && d.age >= MAX_AGE {
                ctx.err(format!("{what}: an AdvanceAge tech must have age < {MAX_AGE}"));
            }
            resolve_cost(&mut ctx, &what, &mut d.cost, &resource_ix);
            if let Some(b) = look(&mut ctx, &what, "building", &building_ix, &d.researched_at) {
                d.researched_at_id = b;
            }
            d.requires_ids = look_all(&mut ctx, &what, "tech", &tech_ix, &d.requires);
            d.requires_building_ids = look_all(&mut ctx, &what, "building", &building_ix, &d.requires_buildings);
            for e in &d.effects {
                match e {
                    Effect::AdvanceAge => {}
                    Effect::Stat { target, value, .. } => {
                        check_selector(&mut ctx, &what, target, &unit_ix, &building_ix);
                        if !value.is_finite() {
                            ctx.err(format!("{what}: stat value must be finite"));
                        }
                    }
                    Effect::GatherRate { resource, mult } => {
                        look(&mut ctx, &what, "resource", &resource_ix, resource);
                        if *mult <= 0.0 {
                            ctx.err(format!("{what}: GatherRate mult must be > 0"));
                        }
                    }
                    Effect::CarryCapacity { .. } => {}
                    Effect::CostMult { target, mult } => {
                        check_selector(&mut ctx, &what, target, &unit_ix, &building_ix);
                        if *mult <= 0.0 {
                            ctx.err(format!("{what}: CostMult mult must be > 0"));
                        }
                    }
                    Effect::UpgradeUnit { from, to } => {
                        look(&mut ctx, &what, "unit", &unit_ix, from);
                        look(&mut ctx, &what, "unit", &unit_ix, to);
                    }
                    Effect::Trickle { resource, .. } => {
                        look(&mut ctx, &what, "resource", &resource_ix, resource);
                    }
                }
            }
            d
        })
        .collect();

    // ---- factions ----
    let factions: Vec<FactionDef> = factions
        .into_iter()
        .map(|(src, mut d)| {
            let what = format!("{src}: faction `{}`", d.id);
            check_color(&mut ctx, &what, &d.color);
            for t in &d.traits {
                if t.flag.is_locomotion() {
                    ctx.err(format!("{what}: trait flag {:?} must be a terrain trait, not a locomotion", t.flag));
                }
                for a in &t.applies_to {
                    if !a.is_locomotion() {
                        ctx.err(format!("{what}: trait applies_to {:?} must be a locomotion", a));
                    }
                }
            }
            let mut mult = ResAmounts([1.0; MAX_RESOURCES]);
            for (r, m) in &d.gather_bonus {
                if let Some(id) = look(&mut ctx, &what, "resource", &resource_ix, r) {
                    mult.set(id, *m);
                }
            }
            d.gather_mult = mult;
            d.unit_ids = look_all(&mut ctx, &what, "unit", &unit_ix, &d.units);
            d.building_ids = look_all(&mut ctx, &what, "building", &building_ix, &d.buildings);
            d.tech_ids = look_all(&mut ctx, &what, "tech", &tech_ix, &d.techs);
            if let Some(w) = look(&mut ctx, &what, "worker unit", &unit_ix, &d.worker) {
                d.worker_id = w;
            }
            let mut sc = d.start.resources.clone();
            resolve_cost(&mut ctx, &format!("{what} start"), &mut sc, &resource_ix);
            d.start.resources = sc;
            for (u, _) in &d.start.units {
                look(&mut ctx, &what, "start unit", &unit_ix, u);
            }
            for b in &d.start.buildings {
                look(&mut ctx, &what, "start building", &building_ix, b);
                if !d.buildings.contains(b) {
                    ctx.err(format!("{what}: start building `{b}` is not in the faction's buildings list"));
                }
            }
            if d.start.buildings.is_empty() {
                ctx.err(format!("{what}: start.buildings is empty (need a main hall)"));
            }
            for (u, _) in &d.ai.army {
                if !d.units.contains(u) {
                    ctx.err(format!("{what}: ai.army unit `{u}` is not in the faction's units list"));
                }
            }
            d
        })
        .collect();

    // ---- cross-reference checks needing the resolved tables ----
    let mut trains_at = vec![Vec::new(); buildings.len()];
    for (i, u) in units.iter().enumerate() {
        for b in &u.trained_at_ids {
            trains_at[b.index()].push(UnitTypeId(i as u16));
        }
    }
    let mut researches_at = vec![Vec::new(); buildings.len()];
    for (i, t) in techs.iter().enumerate() {
        if t.researched_at_id.index() < buildings.len() {
            researches_at[t.researched_at_id.index()].push(TechId(i as u16));
        }
    }

    for f in &factions {
        let what = format!("faction `{}`", f.id);
        // Worker must gather and build.
        if f.worker_id.index() < units.len() {
            let w = &units[f.worker_id.index()];
            match &w.gatherer {
                Some(g) if g.builds => {}
                _ => ctx.err(format!("{what}: worker `{}` needs a gatherer with builds: true", w.id)),
            }
            if !f.unit_ids.contains(&f.worker_id) {
                ctx.err(format!("{what}: worker `{}` is not in the faction's units list", w.id));
            }
        }
        // Main hall.
        if let Some(first) = f.start.buildings.first().and_then(|b| building_ix.get(b)) {
            if !buildings[first.index()].has_tag(Tag::TownCenter) {
                ctx.err(format!("{what}: first start building should have the TownCenter tag"));
            }
        }
        // Every unit is trainable somewhere the faction can build.
        for u in &f.unit_ids {
            let ud = &units[u.index()];
            if !ud.trained_at_ids.iter().any(|b| f.building_ids.contains(b)) {
                ctx.err(format!("{what}: unit `{}` is trained_at {:?}, none of which the faction can build", ud.id, ud.trained_at));
            }
        }
        // Every tech is researchable somewhere the faction can build.
        for t in &f.tech_ids {
            let td = &techs[t.index()];
            if !f.building_ids.contains(&td.researched_at_id) {
                ctx.err(format!("{what}: tech `{}` is researched_at `{}`, which the faction cannot build", td.id, td.researched_at));
            }
            for r in &td.requires_ids {
                if !f.tech_ids.contains(r) {
                    ctx.err(format!("{what}: tech `{}` requires `{}`, which the faction cannot research", td.id, techs[r.index()].id));
                }
            }
        }
        // Age-ups for every age transition.
        for age in 1..MAX_AGE {
            let found = f.tech_ids.iter().any(|t| {
                let td = &techs[t.index()];
                td.age == age && td.is_age_up()
            });
            if !found {
                ctx.err(format!("{what}: no AdvanceAge tech with age {age} (needed to reach age {})", age + 1));
            }
        }
        // Drop sites for everything the worker gathers.
        if f.worker_id.index() < units.len() {
            if let Some(g) = &units[f.worker_id.index()].gatherer {
                for r in 0..resources.len() {
                    let rid = ResourceId(r as u8);
                    if g.rate_by_resource.get(rid) > 0.0 {
                        let has_drop = f.building_ids.iter().any(|b| buildings[b.index()].drop_site_ids.contains(&rid));
                        if !has_drop {
                            ctx.err(format!("{what}: worker gathers `{}` but no faction building accepts it as a drop site", resources[r].id));
                        }
                    }
                }
            }
        }
        // Building upgrades must stay inside the faction.
        for b in &f.building_ids {
            if let Some(u) = &buildings[b.index()].upgrade {
                if !f.building_ids.contains(&u.to_id) {
                    ctx.err(format!("{what}: building `{}` upgrades to `{}`, which is not in the faction's buildings list", buildings[b.index()].id, u.to));
                }
            }
        }
    }

    if !ctx.errors.is_empty() {
        return Err(DataErrors(ctx.errors));
    }

    Ok(GameData {
        resources,
        ages,
        terrain,
        nodes,
        weapons,
        units,
        buildings,
        techs,
        factions,
        sources,
        resource_ix,
        terrain_ix,
        node_ix,
        weapon_ix,
        unit_ix,
        building_ix,
        tech_ix,
        faction_ix,
        terrain_masks,
        trains_at,
        researches_at,
    })
}
