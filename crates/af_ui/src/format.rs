//! Pure text formatting shared by the HUD panels and tooltips.

use af_core::ext::map::ResolvedMapOptions;
use af_core::prelude::*;
use bevy::prelude::Entity;

/// `mm:ss`, or `h:mm:ss` past the first hour, of a duration in seconds.
pub fn clock(secs: f32) -> String {
    let total = secs.max(0.0).floor() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// `s` cut to at most `max` characters, ending in "..." when it was cut
/// (the default font has no ellipsis glyph).
pub fn elide(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(3)).collect();
    format!("{}...", kept.trim_end())
}

/// A faction name without its leading article ("The Ironhold Thanes" ->
/// "Ironhold Thanes"), for tight spots.
pub fn short_faction(name: &str) -> &str {
    name.strip_prefix("The ").unwrap_or(name)
}

/// A player name in at most `max` characters, for a table cell: without a
/// leading article, cut short with "..." but keeping a trailing "(AI)" so
/// computer players stay recognisable ("Khaganate of the... (AI)").
pub fn short_player(name: &str, max: usize) -> String {
    let name = short_faction(name);
    match name.strip_suffix(" (AI)") {
        Some(stem) if name.chars().count() > max => format!("{} (AI)", elide(stem, max.saturating_sub(5))),
        _ => elide(name, max),
    }
}

/// A number with at most one decimal; whole numbers drop the ".0".
pub fn num(v: f32) -> String {
    let r = (v * 10.0).round() / 10.0;
    if (r - r.round()).abs() < 1e-3 { format!("{}", r.round() as i64) } else { format!("{r:.1}") }
}

/// "75 / 108" for `value` of `max` points (health, shield), in whole
/// points: a damaged value rounds up (anything alive shows at least 1) but
/// never reaches the maximum, and a full one shows exactly the maximum
/// (techs leave fractional maxima such as 108.04).
pub fn points(value: f32, max: f32) -> String {
    let top = max.max(0.0).round();
    let shown = if value >= max {
        top
    } else if value > 0.0 {
        value.ceil().min(top - 1.0).max(1.0)
    } else {
        0.0
    };
    format!("{} / {}", num(shown), num(top))
}

/// Whole units needed to pay `v` (a fraction always costs one more unit).
pub fn whole(v: f32) -> u32 {
    (v - 1e-3).ceil().max(0.0) as u32
}

/// Non-zero entries of an amount in resource (UI bar) order.
pub fn cost_entries(data: &GameData, amounts: &ResAmounts) -> Vec<(ResourceId, u32)> {
    data.resource_ids()
        .filter_map(|r| {
            let v = whole(amounts.get(r));
            (v > 0).then_some((r, v))
        })
        .collect()
}

/// "50 Food, 20 Wood", or "Free".
pub fn cost(data: &GameData, amounts: &ResAmounts) -> String {
    let parts: Vec<String> = cost_entries(data, amounts).into_iter().map(|(r, v)| format!("{v} {}", data.resource(r).name)).collect();
    if parts.is_empty() { "Free".into() } else { parts.join(", ") }
}

const DAMAGE_TYPES: [(DamageType, &str); 5] = [
    (DamageType::Slash, "slash"),
    (DamageType::Pierce, "pierce"),
    (DamageType::Crush, "crush"),
    (DamageType::Fire, "fire"),
    (DamageType::Energy, "energy"),
];

/// Lower-case name of a damage type.
pub fn damage_type(t: DamageType) -> &'static str {
    DAMAGE_TYPES.iter().find(|(d, _)| *d == t).map_or("", |(_, n)| n)
}

/// "Armour: slash 10%, pierce 20%" listing non-zero resistances.
pub fn armor(a: &Armor) -> String {
    let parts: Vec<String> = DAMAGE_TYPES
        .iter()
        .filter_map(|(t, name)| {
            let pct = (a.get(*t) * 100.0).round() as i32;
            (pct != 0).then(|| format!("{name} {pct}%"))
        })
        .collect();
    if parts.is_empty() { "Armour: none".into() } else { format!("Armour: {}", parts.join(", ")) }
}

/// The top bar's game speed text.
pub fn speed(relative: f32, paused: bool) -> String {
    if paused { "PAUSED".into() } else { format!("x{}", num(relative)) }
}

/// What a unit is doing, from its current order.
pub fn order_label(order: &Order) -> &'static str {
    match order {
        Order::Move { .. } => "Moving",
        Order::AttackMove { .. } => "Attack-moving",
        Order::Attack { .. } => "Attacking",
        Order::Gather { .. } => "Gathering",
        Order::ReturnCargo => "Returning cargo",
        Order::Build { .. } => "Building",
        Order::Repair { .. } => "Repairing",
        Order::Patrol { .. } => "Patrolling",
        Order::Follow { .. } => "Following",
        Order::Hold => "Holding position",
    }
}

/// The entity an order points at, if any (for "Attacking Spearman").
pub fn order_target(order: &Order) -> Option<Entity> {
    match order {
        Order::Attack { target } | Order::Gather { target } | Order::Repair { target } | Order::Follow { target } => Some(*target),
        Order::Build { site } => Some(*site),
        _ => None,
    }
}

/// Display name of a stance.
pub fn stance_label(s: Stance) -> &'static str {
    match s {
        Stance::Aggressive => "Aggressive",
        Stance::Defensive => "Defensive",
        Stance::StandGround => "Stand ground",
        Stance::Passive => "Passive",
    }
}

/// How many notice lines the feed shows.
pub const NOTICE_LINES: usize = 6;
/// Seconds a notice stays in the feed.
pub const NOTICE_LIFETIME: f32 = 12.0;
/// Seconds over which a notice fades out at the end of its life.
pub const NOTICE_FADE: f32 = 3.0;

/// Fills `out` with the indices into `list` of the notices the feed shows
/// for `viewer` at sim time `now`: global ones and the viewer's own, posted
/// in the last [`NOTICE_LIFETIME`] seconds, at most [`NOTICE_LINES`], oldest first.
pub fn recent_notices(list: &[Notice], viewer: Option<PlayerId>, now: f32, out: &mut Vec<usize>) {
    out.clear();
    for (i, n) in list.iter().enumerate().rev() {
        if out.len() == NOTICE_LINES || now - n.time > NOTICE_LIFETIME {
            break;
        }
        // A notice from the future belongs to a previous match's clock.
        let relevant = n.player.is_none() || n.player == viewer;
        if relevant && n.time <= now + 0.5 {
            out.push(i);
        }
    }
    out.reverse();
}

/// Opacity of a notice `age` seconds old.
pub fn notice_alpha(age: f32) -> f32 {
    ((NOTICE_LIFETIME - age) / NOTICE_FADE).clamp(0.0, 1.0)
}

/// The map's name from its options: "Large - Lakes - 2 rivers - The
/// Shattered Fall".
pub fn map_title(o: &ResolvedMapOptions) -> String {
    let rivers = if o.rivers == 1 { "1 river".to_string() } else { format!("{} rivers", o.rivers) };
    format!("{} - {} - {rivers} - {}", o.size.name(), o.water.name(), o.site.name())
}

/// "Climber (Foot); Marshwalker (Mounted)", or "No terrain trait".
pub fn faction_traits(f: &FactionDef) -> String {
    if f.traits.is_empty() {
        return "No terrain trait".into();
    }
    let parts: Vec<String> = f
        .traits
        .iter()
        .map(|t| {
            let to: Vec<String> = t.applies_to.iter().map(|m| format!("{m:?}")).collect();
            format!("{:?} ({})", t.flag, to.join(", "))
        })
        .collect();
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::test_data;
    use bevy::prelude::Vec2;

    #[test]
    fn maps_are_named_by_their_options() {
        use af_core::ext::map::{MapSize, SiteKind, WaterLevel};
        let o = ResolvedMapOptions { size: MapSize::Large, water: WaterLevel::Lakes, rivers: 2, site: SiteKind::Shattered };
        assert_eq!(map_title(&o), "Large - Lakes - 2 rivers - The Shattered Fall");
        let one = ResolvedMapOptions { size: MapSize::Small, water: WaterLevel::Dry, rivers: 1, site: SiteKind::Crater };
        assert_eq!(map_title(&one), "Small - Dry - 1 river - The Starfall Crater");
        for size in MapSize::ALL {
            for water in WaterLevel::ALL {
                for site in SiteKind::ALL {
                    let t = map_title(&ResolvedMapOptions { size, water, rivers: 2, site });
                    // The default font has ASCII; the feed is 620 px wide.
                    assert!(t.is_ascii() && t.len() <= 60, "{t}");
                }
            }
        }
    }

    #[test]
    fn clock_formats_minutes_and_hours() {
        assert_eq!(clock(0.0), "00:00");
        assert_eq!(clock(65.9), "01:05");
        assert_eq!(clock(3599.0), "59:59");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(-3.0), "00:00");
    }

    #[test]
    fn long_names_are_cut_and_lose_their_article() {
        assert_eq!(elide("The Ironhold Thanes (AI)", 40), "The Ironhold Thanes (AI)");
        assert_eq!(elide("The Khaganate of the Endless Steppe (AI)", 24), "The Khaganate of the...");
        assert_eq!(elide("abcdef", 6), "abcdef");
        assert_eq!(elide("abcdefg", 6), "abc...");
        assert_eq!(short_faction("The Sylvan Wardens"), "Sylvan Wardens");
        assert_eq!(short_faction("Theocracy"), "Theocracy");
    }

    #[test]
    fn table_player_names_fit_and_keep_the_ai_tag() {
        assert_eq!(short_player("You", 24), "You");
        assert_eq!(short_player("The Sylvan Wardens (AI)", 24), "Sylvan Wardens (AI)");
        let long = short_player("The Khaganate of the Endless Steppe (AI)", 24);
        assert_eq!(long, "Khaganate of the... (AI)");
        assert!(long.chars().count() <= 24);
        assert_eq!(short_player("A very long human player name", 12), "A very lo...");
    }

    #[test]
    fn points_never_read_more_than_the_maximum() {
        assert_eq!(points(108.04, 108.04), "108 / 108", "full shows the maximum, not one over it");
        assert_eq!(points(107.9, 108.04), "107 / 108", "hurt stays below the maximum");
        assert_eq!(points(0.2, 30.0), "1 / 30", "alive reads at least 1");
        assert_eq!(points(-5.0, 30.0), "0 / 30");
        assert_eq!(points(2600.0, 2600.0), "2600 / 2600");
    }

    #[test]
    fn numbers_drop_trailing_zero() {
        assert_eq!(num(5.0), "5");
        assert_eq!(num(2.45), "2.5");
        assert_eq!(num(0.04), "0");
        assert_eq!(num(12.34), "12.3");
    }

    #[test]
    fn cost_lists_resources_in_bar_order() {
        let data = test_data();
        let house = data.building_id("house").unwrap();
        let tc = data.building_id("town_center").unwrap();
        assert_eq!(cost(&data, &data.building(house).cost.amounts), "50 Wood");
        assert_eq!(cost(&data, &data.building(tc).cost.amounts), "300 Wood, 200 Stone");
        assert_eq!(cost(&data, &ResAmounts::ZERO), "Free");
        // Fractions (cost techs) round up: you need the whole unit.
        let mut a = ResAmounts::ZERO;
        a.set(data.resource_id("food").unwrap(), 42.5);
        assert_eq!(cost_entries(&data, &a), vec![(data.resource_id("food").unwrap(), 43)]);
    }

    #[test]
    fn armor_lists_nonzero_resistances() {
        let a = Armor { slash: 0.6, pierce: 0.8, crush: 0.0, fire: 0.1, energy: 0.0 };
        assert_eq!(armor(&a), "Armour: slash 60%, pierce 80%, fire 10%");
        assert_eq!(armor(&Armor::default()), "Armour: none");
    }

    #[test]
    fn speed_text() {
        assert_eq!(speed(1.0, false), "x1");
        assert_eq!(speed(1.5, false), "x1.5");
        assert_eq!(speed(2.0, true), "PAUSED");
    }

    fn notice(player: Option<u8>, time: f32) -> Notice {
        Notice { player: player.map(PlayerId), text: format!("{player:?}@{time}"), kind: NoticeKind::Info, pos: None, time }
    }

    #[test]
    fn notice_feed_filters_by_viewer_age_and_count() {
        let list: Vec<Notice> = vec![
            notice(None, 1.0),     // too old at t=20
            notice(Some(0), 9.0),  // shown
            notice(Some(1), 10.0), // someone else's
            notice(None, 11.0),
            notice(Some(0), 12.0),
            notice(Some(0), 13.0),
            notice(Some(0), 14.0),
            notice(Some(0), 15.0),
            notice(None, 16.0),
        ];
        let mut out = Vec::new();
        recent_notices(&list, Some(PlayerId(0)), 20.0, &mut out);
        // Six newest relevant ones, oldest first; index 1 (t=9) drops out.
        assert_eq!(out, vec![3, 4, 5, 6, 7, 8]);
        recent_notices(&list, Some(PlayerId(1)), 20.0, &mut out);
        assert_eq!(out, vec![2, 3, 8]);
        recent_notices(&list, None, 20.0, &mut out);
        assert_eq!(out, vec![3, 8]);
    }

    #[test]
    fn notices_fade_at_the_end() {
        assert_eq!(notice_alpha(0.0), 1.0);
        assert_eq!(notice_alpha(NOTICE_LIFETIME - NOTICE_FADE), 1.0);
        assert!((notice_alpha(NOTICE_LIFETIME - NOTICE_FADE * 0.5) - 0.5).abs() < 1e-4);
        assert_eq!(notice_alpha(NOTICE_LIFETIME + 1.0), 0.0);
    }

    #[test]
    fn faction_trait_lines_from_data() {
        let data = test_data();
        let iron = data.faction(data.faction_id("ironhold").unwrap());
        assert_eq!(faction_traits(iron), "Climber (Foot)");
        let star = data.faction(data.faction_id("starborn").unwrap());
        assert_eq!(faction_traits(star), "No terrain trait");
    }

    #[test]
    fn order_labels_and_targets() {
        let e = Entity::from_raw_u32(7).unwrap();
        assert_eq!(order_label(&Order::Gather { target: e }), "Gathering");
        assert_eq!(order_target(&Order::Build { site: e }), Some(e));
        assert_eq!(order_target(&Order::Move { target: Vec2::ZERO }), None);
        assert_eq!(order_label(&Order::Hold), "Holding position");
    }
}
