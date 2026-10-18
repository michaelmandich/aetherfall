//! Tooltip content for command card buttons, built from the content defs
//! and the player's effective (tech-modified) numbers.

use af_core::ext::combat::{area_reach, weapon_reach};
use af_core::prelude::*;

use crate::card::{self, CardAction};
use crate::format::{self, num};

/// One price entry of a tooltip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CostChip {
    pub resource: ResourceId,
    pub amount: u32,
    pub affordable: bool,
}

/// What the tooltip panel shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tooltip {
    pub title: String,
    pub lines: Vec<String>,
    pub cost: Vec<CostChip>,
    /// Seconds to train / build / research.
    pub time: Option<f32>,
    /// What `time` is spent on ("train", "build", ...).
    pub time_verb: &'static str,
    /// What holding Shift does with this button, if anything.
    pub shift: Option<&'static str>,
    /// Why the button is disabled.
    pub reason: Option<String>,
}

impl Tooltip {
    /// "14 s to train", or nothing without a time.
    pub fn time_text(&self) -> String {
        self.time.map(|s| format!("{} s to {}", num(s), self.time_verb)).unwrap_or_default()
    }
}

/// What Shift does with a card button.
pub fn shift_hint(action: CardAction) -> Option<&'static str> {
    match action {
        CardAction::Train(_) => Some("Shift: queue 5 at once"),
        CardAction::Build(_) => Some("Shift: keep placing after this one"),
        CardAction::Move | CardAction::AttackMove | CardAction::Patrol | CardAction::Gather | CardAction::Repair | CardAction::Hold => {
            Some("Shift: do it after the current orders")
        }
        _ => None,
    }
}

/// What `action` costs `player` (with `building` selected), if anything.
pub fn action_cost(data: &GameData, player: &PlayerState, building: Option<BuildingTypeId>, action: CardAction) -> Option<ResAmounts> {
    match action {
        CardAction::Train(u) => Some(stats::unit_cost(data, player, u)),
        CardAction::Build(b) => Some(stats::building_cost(data, player, b)),
        CardAction::Research(t) => Some(data.tech(t).cost.amounts),
        CardAction::Upgrade { .. } => building.and_then(|b| data.building(b).upgrade.as_ref()).map(|u| u.cost.amounts),
        _ => None,
    }
}

/// "Need 30 more Wood, 20 more Stone": what `player` still lacks to pay
/// `cost`, in whole units, or `None` when it is affordable.
pub fn shortfall(data: &GameData, player: &PlayerState, cost: &ResAmounts) -> Option<String> {
    let parts: Vec<String> = data
        .resource_ids()
        .filter_map(|r| {
            let missing = format::whole(cost.get(r) - player.resources.get(r).max(0.0));
            (missing > 0).then(|| format!("{missing} more {}", data.resource(r).name))
        })
        .collect();
    (!parts.is_empty()).then(|| format!("Need {}", parts.join(", ")))
}

/// Why `action` is refused, for its tooltip and the notice a press posts:
/// the rule's reason, and for a missing payment exactly what is missing.
pub fn reason_text(data: &GameData, player: &PlayerState, building: Option<BuildingTypeId>, action: CardAction, reason: Reason) -> String {
    let short = (reason == Reason::CannotAfford).then(|| action_cost(data, player, building, action)).flatten().and_then(|c| shortfall(data, player, &c));
    short.unwrap_or_else(|| reason.describe(data))
}

/// The tooltip of a card button for `player` with `building` selected.
pub fn for_action(data: &GameData, player: &PlayerState, building: Option<BuildingTypeId>, action: CardAction) -> Tooltip {
    let mut t = match action {
        CardAction::Train(u) => unit(data, player, u),
        CardAction::Build(b) => building_type(data, player, b),
        CardAction::Research(t) => tech(data, player, t),
        CardAction::Upgrade { to } => upgrade(data, player, building, to),
        other => command(other),
    };
    t.time_verb = match action {
        CardAction::Train(_) => "train",
        CardAction::Build(_) => "build",
        CardAction::Research(_) => "research",
        _ => "upgrade",
    };
    t.shift = shift_hint(action);
    t.reason = card::action_state(data, player, building, action).err().map(|r| reason_text(data, player, building, action, r));
    t
}

fn chips(data: &GameData, player: &PlayerState, amounts: &ResAmounts) -> Vec<CostChip> {
    format::cost_entries(data, amounts)
        .into_iter()
        .map(|(resource, amount)| CostChip { resource, amount, affordable: player.resources.get(resource) + 1e-4 >= amounts.get(resource) })
        .collect()
}

/// Damage per second of one weapon on an entity with `stats`.
pub fn weapon_dps(data: &GameData, stats: &Stats, w: WeaponId) -> f32 {
    let d = data.weapon(w);
    d.damage * d.volley as f32 * stats.damage_mult / stats::weapon_cooldown(data, stats, w)
}

/// "Short Spear: 4.2 dps, range 1 (pierce); x2 vs Cavalry". The range is
/// the reach af_combat engages at ([`weapon_reach`]), after the minimum
/// range if there is one ("range 3-15"). Range techs do not grow a Spray's
/// cone or a Nova's ring ([`area_reach`]), so once the two differ the area
/// follows ("range 6, cone 5").
pub fn weapon_line(data: &GameData, stats: &Stats, w: WeaponId) -> String {
    let d = data.weapon(w);
    let reach = num(weapon_reach(data, stats, w));
    let mut range = if d.min_range > 0.0 { format!("{}-{reach}", num(d.min_range)) } else { reach.clone() };
    let area = num(area_reach(d));
    match d.kind {
        WeaponKind::Spray { .. } if area != reach => range.push_str(&format!(", cone {area}")),
        WeaponKind::Nova if area != reach => range.push_str(&format!(", radius {area}")),
        _ => {}
    }
    let mut s = format!("{}: {} dps, range {range} ({})", d.name, num(weapon_dps(data, stats, w)), format::damage_type(d.damage_type));
    if d.splash > 0.0 {
        s.push_str(&format!(", splash {}", num(d.splash)));
    }
    if !d.bonus.is_empty() {
        let bonus: Vec<String> = d.bonus.iter().map(|(tag, m)| format!("x{} vs {tag:?}", num(*m))).collect();
        s.push_str("; ");
        s.push_str(&bonus.join(", "));
    }
    s
}

fn unit(data: &GameData, player: &PlayerState, u: UnitTypeId) -> Tooltip {
    let def = data.unit(u);
    let st = stats::unit_stats(data, player, u);
    let mut lines = vec![def.description.clone()];
    let mut head = format!("HP {}", num(st.max_hp));
    if st.shield_max > 0.0 {
        head.push_str(&format!(" + {} shield", num(st.shield_max)));
    }
    head.push_str(&format!("  Speed {}  Vision {}  Pop {}", num(st.speed), num(st.vision), def.pop * def.squad));
    lines.push(head);
    if def.squad > 1 {
        lines.push(format!("Squad of {} that moves as one", def.squad));
    }
    lines.push(format::armor(&st.armor));
    lines.extend(def.weapon_ids.iter().map(|w| weapon_line(data, &st, *w)));
    if let Some(g) = &def.gatherer {
        let mut s = format!("Worker: carries {}", num(stats::carry_capacity(data, player, u)));
        if g.builds {
            s.push_str(", builds");
        }
        if g.repair > 0.0 {
            s.push_str(&format!(", repairs {} hp/s", num(g.repair)));
        }
        lines.push(s);
    }
    if let Some(h) = &def.healer {
        lines.push(format!("Heals {} hp/s within {} m", num(h.rate), num(h.range)));
    }
    let tags: Vec<String> = def.tags.iter().map(|t| format!("{t:?}")).collect();
    lines.push(format!("Tags: {}", tags.join(", ")));
    Tooltip {
        title: def.name.clone(),
        lines,
        cost: chips(data, player, &stats::unit_cost(data, player, u)),
        time: Some(stats::unit_train_time(data, player, u)),
        ..Default::default()
    }
}

fn names<T>(ids: impl IntoIterator<Item = T>, name: impl Fn(T) -> String) -> String {
    ids.into_iter().map(name).collect::<Vec<_>>().join(", ")
}

fn building_type(data: &GameData, player: &PlayerState, b: BuildingTypeId) -> Tooltip {
    let def = data.building(b);
    let st = stats::building_stats(data, player, b);
    let mut lines = vec![def.description.clone()];
    let mut head = format!("HP {}", num(st.max_hp));
    if st.shield_max > 0.0 {
        head.push_str(&format!(" + {} shield", num(st.shield_max)));
    }
    if def.pop > 0 {
        head.push_str(&format!("  Pop +{}", def.pop));
    }
    lines.push(head);
    lines.push(format::armor(&st.armor));
    if !def.drop_site_ids.is_empty() {
        lines.push(format!("Drop site: {}", names(def.drop_site_ids.iter().copied(), |r| data.resource(r).name.clone())));
    }
    let trains = data.trainable_at(b, player.faction);
    if !trains.is_empty() {
        lines.push(format!("Trains: {}", names(trains, |u| data.unit(u).name.clone())));
    }
    lines.extend(def.weapon_ids.iter().map(|w| weapon_line(data, &st, *w)));
    if !def.trickle_amounts.is_zero() {
        let income: Vec<String> = data
            .resource_ids()
            .filter(|r| def.trickle_amounts.get(*r) > 0.0)
            .map(|r| format!("{} {}/s", num_fine(def.trickle_amounts.get(r)), data.resource(r).name))
            .collect();
        lines.push(format!("Income: {}", income.join(", ")));
    }
    if let Some(f) = &def.resource_field {
        lines.push(format!("Field: {} {}, {} worker(s)", num(f.amount), data.resource(f.resource_id).name, f.slots));
    }
    if def.gate {
        lines.push("Gate: your team's units pass through".into());
    } else if def.wall {
        lines.push("Wall: click its start, then its end, to place a line of segments".into());
    }
    if let Some(n) = def.requires_node_id {
        lines.push(format!("Must be built on a {}", data.node(n).name));
    }
    if let Some(limit) = def.limit {
        lines.push(format!("Limit {limit} per player"));
    }
    Tooltip {
        title: def.name.clone(),
        lines,
        cost: chips(data, player, &stats::building_cost(data, player, b)),
        time: Some(stats::building_build_time(data, player, b)),
        ..Default::default()
    }
}

fn tech(data: &GameData, player: &PlayerState, t: TechId) -> Tooltip {
    let def = data.tech(t);
    let mut lines = vec![def.description.clone()];
    // Descriptions often already spell an effect out ("Advance to the Banner Age: ...").
    lines.extend(def.effects.iter().map(|e| effect_line(data, def, e)).filter(|l| !def.description.contains(l.as_str())));
    Tooltip { title: def.name.clone(), lines, cost: chips(data, player, &def.cost.amounts), time: Some(def.time), ..Default::default() }
}

fn upgrade(data: &GameData, player: &PlayerState, from: Option<BuildingTypeId>, to: BuildingTypeId) -> Tooltip {
    let to_def = data.building(to);
    let mut lines = vec![to_def.description.clone()];
    let mut t = Tooltip { title: format!("Upgrade to {}", to_def.name), ..Default::default() };
    if let Some((f, u)) = from.and_then(|f| data.building(f).upgrade.as_ref().map(|u| (f, u))) {
        let before = stats::building_stats(data, player, f);
        let after = stats::building_stats(data, player, to);
        lines.push(format!("HP {} -> {}", num(before.max_hp), num(after.max_hp)));
        lines.extend(to_def.weapon_ids.iter().map(|w| weapon_line(data, &after, *w)));
        t.cost = chips(data, player, &u.cost.amounts);
        t.time = Some(u.time);
    }
    t.lines = lines;
    t
}

fn command(action: CardAction) -> Tooltip {
    let (title, line) = match action {
        CardAction::Move => ("Move", "Click a destination. Right-clicking the ground does the same."),
        CardAction::Stop => ("Stop", "Drop every order and stand still."),
        CardAction::Hold => ("Hold position", "Stay put and shoot what comes in range; never chase."),
        CardAction::Patrol => ("Patrol", "Walk back and forth to a point, engaging enemies met on the way."),
        CardAction::AttackMove => ("Attack-move", "Move to a point, fighting anything met on the way. Click an enemy to attack it."),
        CardAction::Gather => ("Gather", "Click a resource or a farm to work it."),
        CardAction::Repair => ("Repair", "Click a damaged friendly building or machine to mend it."),
        CardAction::Stance(Stance::Aggressive) => ("Aggressive", "Chase enemies that come into sight."),
        CardAction::Stance(Stance::Defensive) => ("Defensive", "Engage enemies in sight, then return to post."),
        CardAction::Stance(Stance::StandGround) => ("Stand ground", "Shoot what is in range; never move."),
        CardAction::Stance(Stance::Passive) => ("Passive", "Never attack on their own."),
        CardAction::BuildPage(c) => (c.label(), "Open this page of buildings."),
        CardAction::Rally => ("Set rally point", "Click where new units should gather."),
        CardAction::More => ("More", "Show the next page of buttons."),
        CardAction::Back => ("Back", "Return to the main commands."),
        // Production buttons have their own tooltips (see `for_action`).
        CardAction::Build(_) | CardAction::Train(_) | CardAction::Research(_) | CardAction::Upgrade { .. } => ("", ""),
    };
    Tooltip { title: title.into(), lines: vec![line.into()], ..Default::default() }
}

/// Up to two decimals, for small rates such as a trickle of adamant
/// (where [`num`]'s one decimal would round a few hundredths to 0 or 0.1).
fn num_fine(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// "+15%" / "-10%" for a multiplier.
fn percent_change(mult: f32) -> String {
    let pct = ((mult - 1.0) * 100.0).round() as i32;
    if pct >= 0 { format!("+{pct}%") } else { format!("{pct}%") }
}

fn selector(data: &GameData, s: &Selector) -> String {
    match s {
        Selector::AllUnits => "All units".into(),
        Selector::AllBuildings => "All buildings".into(),
        Selector::Unit(id) => data.unit_id(id).map_or_else(|| id.clone(), |u| data.unit(u).name.clone()),
        Selector::Building(id) => data.building_id(id).map_or_else(|| id.clone(), |b| data.building(b).name.clone()),
        Selector::Tag(t) => format!("{t:?}"),
    }
}

fn stat_name(s: Stat) -> String {
    match s {
        Stat::Hp => "hp".into(),
        Stat::Armor(t) => format!("{} armour", format::damage_type(t)),
        Stat::Damage => "damage".into(),
        Stat::Range => "range".into(),
        Stat::Cooldown => "attack time".into(),
        Stat::Speed => "speed".into(),
        Stat::Vision => "vision".into(),
        Stat::TrainTime => "train time".into(),
        Stat::BuildTime => "build time".into(),
        Stat::ShieldHp => "shield".into(),
        Stat::Regen => "regeneration".into(),
    }
}

/// One readable line per tech effect.
pub fn effect_line(data: &GameData, tech: &TechDef, e: &Effect) -> String {
    let resource = |id: &str| data.resource_id(id).map_or_else(|| id.to_string(), |r| data.resource(r).name.clone());
    let unit = |id: &str| data.unit_id(id).map_or_else(|| id.to_string(), |u| data.unit(u).name.clone());
    match e {
        Effect::AdvanceAge => format!("Advance to the {}", data.age(tech.age.saturating_add(1)).name),
        Effect::Stat { target, stat, op, value } => {
            let change = match (op, stat) {
                (Op::Mul, _) => percent_change(*value),
                // Armour is a fraction: +0.1 reads as +10%.
                (Op::Add, Stat::Armor(_)) => format!("{:+}%", (value * 100.0).round() as i32),
                (Op::Add, _) => {
                    let n = num(*value);
                    if *value >= 0.0 { format!("+{n}") } else { n }
                }
            };
            format!("{}: {} {change}", selector(data, target), stat_name(*stat))
        }
        Effect::GatherRate { resource: r, mult } => format!("{} gathering {}", resource(r), percent_change(*mult)),
        Effect::CarryCapacity { add } => format!("Workers carry +{}", num(*add)),
        Effect::CostMult { target, mult } => format!("{} cost {}", selector(data, target), percent_change(*mult)),
        Effect::UpgradeUnit { from, to } => format!("Upgrades every {} to {}", unit(from), unit(to)),
        Effect::Trickle { resource: r, per_sec } => format!("+{} {} per second", num_fine(*per_sec), resource(r)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::test_data;

    fn player(data: &GameData) -> PlayerState {
        let f = data.faction_id("ironhold").unwrap();
        PlayerState::new(PlayerId(0), "P".into(), f, [1.0; 3], 0, Controller::Human, data)
    }

    #[test]
    fn unit_tooltip_from_real_data() {
        let data = test_data();
        let p = player(&data);
        let peasant = data.unit_id("peasant").unwrap();
        let tc = data.building_id("town_center").unwrap();
        let t = for_action(&data, &p, Some(tc), CardAction::Train(peasant));
        assert_eq!(t.title, "Peasant");
        let food = data.resource_id("food").unwrap();
        assert_eq!(t.cost, vec![CostChip { resource: food, amount: 50, affordable: true }]);
        assert_eq!(t.time, Some(14.0));
        assert!(t.lines.iter().any(|l| l.starts_with("HP 30  Speed 2.4  Vision 8  Pop 1")), "{:?}", t.lines);
        assert!(t.lines.iter().any(|l| l == "Armour: pierce 5%"), "{:?}", t.lines);
        // Work axe: 3 slash every 1.5 s.
        assert!(t.lines.iter().any(|l| l == "Work Axe: 2 dps, range 0.6 (slash)"), "{:?}", t.lines);
        assert!(t.lines.iter().any(|l| l == "Worker: carries 10, builds, repairs 3 hp/s"), "{:?}", t.lines);
        // No houses yet at the start.
        assert_eq!(t.reason.as_deref(), Some("Need more houses"));
    }

    #[test]
    fn weapon_ranges_match_what_combat_engages_at() {
        let data = test_data();
        let mut p = player(&data);
        let line = |p: &PlayerState, unit: &str| {
            let u = data.unit_id(unit).unwrap();
            weapon_line(&data, &stats::unit_stats(&data, p, u), data.unit(u).weapon_ids[0])
        };
        let mangonel = line(&p, "mangonel");
        assert!(mangonel.starts_with("Mangonel Stone:") && mangonel.contains("range 3-15 (crush)"), "{mangonel}");
        // The blunderbuss is a Spray: its cone matches its range at first.
        let hearthguard = line(&p, "ironhold_hearthguard");
        assert!(hearthguard.starts_with("Blunderbuss:") && hearthguard.contains("range 5 (pierce)"), "{hearthguard}");
        // +1 range for ranged units and machines.
        let tech = data.tech(data.tech_id("targeting_computers").unwrap());
        for e in &tech.effects {
            p.mods.apply_effect(&data, e);
        }
        let archer = line(&p, "archer");
        assert!(archer.contains("range 9 (pierce)"), "{archer}");
        // It now engages from further off, but the cone stays as long.
        let hearthguard = line(&p, "ironhold_hearthguard");
        assert!(hearthguard.contains("range 6, cone 5 (pierce)"), "{hearthguard}");
    }

    #[test]
    fn unaffordable_costs_are_flagged() {
        let data = test_data();
        let mut p = player(&data);
        p.resources = ResAmounts::ZERO;
        let spearman = data.unit_id("spearman").unwrap();
        let barracks = data.building_id("barracks").unwrap();
        let t = for_action(&data, &p, Some(barracks), CardAction::Train(spearman));
        assert_eq!(t.cost.len(), 2);
        assert!(t.cost.iter().all(|c| !c.affordable));
        // The reason says exactly what is missing, in bar order.
        let want: Vec<String> = t.cost.iter().map(|c| format!("{} more {}", c.amount, data.resource(c.resource).name)).collect();
        assert_eq!(t.reason, Some(format!("Need {}", want.join(", "))));
    }

    #[test]
    fn shortfalls_count_only_what_is_missing() {
        let data = test_data();
        let mut p = player(&data);
        let (food, wood) = (data.resource_id("food").unwrap(), data.resource_id("wood").unwrap());
        let mut cost = ResAmounts::ZERO;
        cost.set(food, 50.0);
        cost.set(wood, 25.0);
        p.resources = ResAmounts::ZERO;
        p.resources.set(food, 80.0);
        p.resources.set(wood, 4.5);
        // Enough food; 20.5 wood short reads as 21: the whole unit still to find.
        assert_eq!(shortfall(&data, &p, &cost).as_deref(), Some("Need 21 more Wood"));
        p.resources.set(wood, 25.0);
        assert_eq!(shortfall(&data, &p, &cost), None);
        // Other refusals keep the rule's wording.
        let tc = data.building_id("town_center").unwrap();
        let peasant = CardAction::Train(data.unit_id("peasant").unwrap());
        assert_eq!(reason_text(&data, &p, Some(tc), peasant, Reason::PopCapped), "Need more houses");
        assert_eq!(reason_text(&data, &p, Some(tc), CardAction::Stop, Reason::CannotAfford), "Not enough resources");
    }

    #[test]
    fn building_tooltip_from_real_data() {
        let data = test_data();
        let p = player(&data);
        let house = data.building_id("house").unwrap();
        let t = for_action(&data, &p, None, CardAction::Build(house));
        assert_eq!(t.title, "House");
        assert_eq!(t.time, Some(25.0));
        assert_eq!(t.cost, vec![CostChip { resource: data.resource_id("wood").unwrap(), amount: 50, affordable: true }]);
        assert!(t.lines.iter().any(|l| l == "HP 550  Pop +10"), "{:?}", t.lines);
        assert_eq!(t.reason, None);
        let tc = for_action(&data, &p, None, CardAction::Build(data.building_id("town_center").unwrap()));
        assert!(tc.lines.iter().any(|l| l.starts_with("Drop site: Food, Wood, Stone")), "{:?}", tc.lines);
        assert!(tc.lines.iter().any(|l| l.starts_with("Trains: Peasant, Scout Rider")), "{:?}", tc.lines);
        let wall = for_action(&data, &p, None, CardAction::Build(data.building_id("palisade_wall").unwrap()));
        assert!(wall.lines.iter().any(|l| l.starts_with("Wall:")));
    }

    #[test]
    fn tech_and_upgrade_tooltips_from_real_data() {
        let data = test_data();
        let p = player(&data);
        let axe = data.tech_id("double_bit_axe").unwrap();
        let camp = data.building_id("lumber_camp").unwrap();
        let t = for_action(&data, &p, Some(camp), CardAction::Research(axe));
        assert_eq!(t.title, "Double-Bit Axe");
        assert_eq!(t.time, Some(25.0));
        assert!(t.lines.contains(&"Wood gathering +15%".to_string()), "{:?}", t.lines);
        let to_banner = data.tech_id("to_banner").unwrap();
        let tc = data.building_id("town_center").unwrap();
        let age = for_action(&data, &p, Some(tc), CardAction::Research(to_banner));
        assert_eq!(effect_line(&data, data.tech(to_banner), &Effect::AdvanceAge), "Advance to the Banner Age");
        // The description already says so; the effect line is not repeated.
        assert_eq!(age.lines.iter().filter(|l| l.contains("Advance to the Banner Age")).count(), 1, "{:?}", age.lines);
        let citadel = data.building_id("citadel").unwrap();
        let up = for_action(&data, &p, Some(tc), CardAction::Upgrade { to: citadel });
        assert_eq!(up.title, "Upgrade to Citadel");
        // Numbers come from the content (other tracks tune them).
        let up_def = data.building(tc).upgrade.as_ref().unwrap();
        assert_eq!(up.time, Some(up_def.time));
        assert!(format::cost(&data, &up_def.cost.amounts).contains("Stone"), "a citadel costs stone");
        assert_eq!(up.cost.len(), format::cost_entries(&data, &up_def.cost.amounts).len());
        let hp = |b| num(stats::building_stats(&data, &p, b).max_hp);
        assert!(up.lines.contains(&format!("HP {} -> {}", hp(tc), hp(citadel))), "{:?}", up.lines);
        assert_eq!(up.reason.as_deref(), Some("Requires the Starfall Age"));
    }

    #[test]
    fn times_say_what_they_are_for_and_shift_is_explained() {
        let data = test_data();
        let p = player(&data);
        let tc = data.building_id("town_center").unwrap();
        let peasant = for_action(&data, &p, Some(tc), CardAction::Train(data.unit_id("peasant").unwrap()));
        assert_eq!(peasant.time_text(), format!("{} s to train", num(peasant.time.unwrap())));
        assert_eq!(peasant.shift, Some("Shift: queue 5 at once"));
        let house = for_action(&data, &p, None, CardAction::Build(data.building_id("house").unwrap()));
        assert_eq!(house.time_text(), format!("{} s to build", num(house.time.unwrap())));
        let banner = for_action(&data, &p, Some(tc), CardAction::Research(data.tech_id("to_banner").unwrap()));
        assert!(banner.time_text().ends_with(" s to research"), "{}", banner.time_text());
        assert_eq!(banner.shift, None);
        let citadel = for_action(&data, &p, Some(tc), CardAction::Upgrade { to: data.building_id("citadel").unwrap() });
        assert_eq!(citadel.time_text(), format!("{} s to upgrade", num(data.building(tc).upgrade.as_ref().unwrap().time)));
        let stop = for_action(&data, &p, None, CardAction::Stop);
        assert_eq!((stop.time_text().as_str(), stop.shift), ("", None));
        assert!(for_action(&data, &p, None, CardAction::AttackMove).shift.is_some());
    }

    #[test]
    fn small_rates_keep_two_decimals() {
        assert_eq!(num_fine(0.06), "0.06");
        assert_eq!(num_fine(0.6), "0.6");
        assert_eq!(num_fine(2.0), "2");
        assert_eq!(num_fine(0.127), "0.13");
    }

    #[test]
    fn every_tech_effect_has_a_line() {
        let data = test_data();
        for t in &data.techs {
            for e in &t.effects {
                let line = effect_line(&data, t, e);
                assert!(!line.is_empty() && !line.contains('_'), "{}: {line}", t.id);
            }
        }
    }

    #[test]
    fn command_tooltips_have_titles() {
        let data = test_data();
        let p = player(&data);
        for a in [CardAction::Move, CardAction::Stop, CardAction::Rally, CardAction::Stance(Stance::Passive), CardAction::Back] {
            let t = for_action(&data, &p, None, a);
            assert!(!t.title.is_empty() && t.reason.is_none(), "{a:?}");
        }
    }
}
