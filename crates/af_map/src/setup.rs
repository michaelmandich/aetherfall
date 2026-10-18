//! Match setup without the ECS: which setup to build, the player states
//! (colours, start resources, start age) and the fresh fog grids.

use af_core::ext::fog::fog_for_map;
use af_core::prelude::*;
use bevy::prelude::*;

/// The most player slots a match supports.
pub const MAX_PLAYERS: usize = 8;
/// Fog-of-war cell size in metres.
pub const FOG_CELL: f32 = 2.0;
/// Players' colours are at least this far apart (RGB distance) unless the
/// setup chose clashing colours explicitly.
pub const MIN_COLOR_DISTANCE: f32 = 0.2;

/// The default skirmish: the first two factions, slot 0 human against an
/// AI, fog on.
pub fn default_setup(data: &GameData) -> MatchSetup {
    let factions = data.factions.len();
    let slot = |i: usize| {
        let faction = FactionId((i % factions) as u8);
        let human = i == 0;
        PlayerSetup {
            name: if human { "You".to_string() } else { format!("{} (AI)", data.faction(faction).name) },
            faction,
            team: i as u8,
            controller: if human { Controller::Human } else { Controller::Ai },
            color: None,
        }
    };
    let players = if factions == 0 { Vec::new() } else { (0..2).map(slot).collect() };
    MatchSetup { players, ..MatchSetup::default() }
}

/// The setup to build: `setup` (or [`default_setup`] when there is none, or
/// it has no players) made safe to use: at most [`MAX_PLAYERS`] slots, known
/// factions, a valid local slot, an age in range, a sane resource
/// multiplier, and team ids below [`MAX_PLAYERS`] (remapped in order of
/// appearance otherwise, keeping who is allied with whom).
pub fn resolve_setup(data: &GameData, setup: Option<&MatchSetup>) -> MatchSetup {
    let mut s = setup.cloned().unwrap_or_else(|| default_setup(data));
    if s.players.is_empty() {
        s.players = default_setup(data).players;
    }
    if s.players.len() > MAX_PLAYERS {
        warn!("match setup has {} players; keeping the first {MAX_PLAYERS}", s.players.len());
        s.players.truncate(MAX_PLAYERS);
    }
    if data.factions.is_empty() {
        s.players.clear();
    }
    for p in &mut s.players {
        if p.faction.index() >= data.factions.len() {
            warn!("player `{}` has unknown faction {:?}; using {:?}", p.name, p.faction, FactionId(0));
            p.faction = FactionId(0);
        }
    }
    if s.players.iter().any(|p| p.team as usize >= MAX_PLAYERS) {
        let mut seen: Vec<u8> = Vec::new();
        for p in &mut s.players {
            let team = seen.iter().position(|t| *t == p.team).unwrap_or_else(|| {
                seen.push(p.team);
                seen.len() - 1
            });
            p.team = team as u8;
        }
    }
    s.local_player = s.local_player.filter(|i| *i < s.players.len());
    s.start_age = s.start_age.clamp(1, MAX_AGE);
    if !(s.start_resources_mult.is_finite() && s.start_resources_mult >= 0.0) {
        s.start_resources_mult = 1.0;
    }
    s
}

/// A player state per slot of a resolved setup: distinguishable colours,
/// start resources times the setup's multiplier, the start position, and the
/// start age with every earlier age-up researched (effects applied).
pub fn build_players(data: &GameData, setup: &MatchSetup, starts: &[Vec2]) -> Players {
    let colors = player_colors(data, &setup.players);
    let list = setup
        .players
        .iter()
        .zip(colors)
        .enumerate()
        .map(|(i, (ps, color))| {
            let mut p = PlayerState::new(PlayerId(i as u8), ps.name.clone(), ps.faction, color, ps.team, ps.controller, data);
            p.resources = p.resources.scaled(setup.start_resources_mult);
            p.start_pos = starts.get(i).copied().unwrap_or_default();
            advance_to_age(data, &mut p, setup.start_age);
            p
        })
        .collect();
    Players { list }
}

/// Put `p` in `age`, marking every age-up tech of the ages before it
/// researched and applying its effects.
fn advance_to_age(data: &GameData, p: &mut PlayerState, age: u8) {
    let age = age.clamp(1, MAX_AGE);
    for from in 1..age {
        let Some(tech) = data.age_up_tech(p.faction, from) else { continue };
        p.researched[tech.index()] = true;
        for effect in &data.tech(tech).effects {
            p.mods.apply_effect(data, effect);
        }
    }
    p.age = age;
}

/// Explicit setup colours are kept; faction colours are shifted in hue and
/// lightness until they are [`MIN_COLOR_DISTANCE`] from every colour already
/// taken (so duplicate factions stay distinguishable).
fn player_colors(data: &GameData, players: &[PlayerSetup]) -> Vec<[f32; 3]> {
    let mut colors: Vec<Option<[f32; 3]>> = players.iter().map(|p| p.color).collect();
    for i in 0..players.len() {
        if colors[i].is_some() {
            continue;
        }
        let base = data.faction(players[i].faction).color;
        let taken: Vec<[f32; 3]> = colors.iter().flatten().copied().collect();
        let mut best = (f32::NEG_INFINITY, base);
        for &(hue, lightness) in &COLOR_VARIANTS {
            let c = shift_color(base, hue, lightness);
            let nearest = taken.iter().map(|t| color_distance(&c, t)).fold(f32::INFINITY, f32::min);
            if nearest > best.0 {
                best = (nearest, c);
            }
            if nearest >= MIN_COLOR_DISTANCE {
                break;
            }
        }
        colors[i] = Some(best.1);
    }
    colors.into_iter().map(Option::unwrap_or_default).collect()
}

/// (hue degrees, lightness) shifts tried in order; the first is the colour itself.
const COLOR_VARIANTS: [(f32, f32); 15] = [
    (0.0, 0.0),
    (0.0, 0.18),
    (0.0, -0.18),
    (30.0, 0.0),
    (-30.0, 0.0),
    (30.0, 0.15),
    (-30.0, -0.15),
    (-30.0, 0.15),
    (30.0, -0.15),
    (60.0, 0.0),
    (-60.0, 0.0),
    (0.0, 0.32),
    (0.0, -0.32),
    (90.0, 0.1),
    (-90.0, -0.1),
];

fn shift_color(c: [f32; 3], hue: f32, lightness: f32) -> [f32; 3] {
    if hue == 0.0 && lightness == 0.0 {
        // Exactly the original (an HSL round trip would blur the last bits).
        return c;
    }
    let mut hsla = Hsla::from(Srgba::rgb(c[0], c[1], c[2]));
    hsla.hue = (hsla.hue + hue).rem_euclid(360.0);
    hsla.lightness = (hsla.lightness + lightness).clamp(0.12, 0.88);
    let s = Srgba::from(hsla);
    [s.red.clamp(0.0, 1.0), s.green.clamp(0.0, 1.0), s.blue.clamp(0.0, 1.0)]
}

fn color_distance(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    Vec3::from_array(*a).distance(Vec3::from_array(*b))
}

/// Fresh fog grids ([`fog_for_map`]): [`FOG_CELL`] m cells over the map, one
/// zeroed grid per team id up to the highest team in play; with
/// `reveal_map` everything starts explored and visible.
pub fn new_fog(setup: &MatchSetup, map_size: f32, players: &Players) -> FogOfWar {
    let teams = players.iter().map(|p| p.team as usize + 1).max().unwrap_or(0);
    let mut fog = fog_for_map(Vec2::splat(map_size), FOG_CELL, teams, setup.fog);
    if setup.reveal_map {
        for team in &mut fog.teams {
            team.visible.fill(1);
            team.explored.fill(true);
        }
    }
    fog
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_factions_get_distinct_colors() {
        let data = af_core::testing::test_data();
        let faction = data.faction_id("ironhold").unwrap();
        let players: Vec<PlayerSetup> = (0..MAX_PLAYERS)
            .map(|i| PlayerSetup { name: format!("P{i}"), faction, team: i as u8, controller: Controller::Ai, color: None })
            .collect();
        let colors = player_colors(&data, &players);
        assert_eq!(colors[0], data.faction(faction).color, "the first keeps the faction colour");
        for (i, a) in colors.iter().enumerate() {
            assert!(a.iter().all(|v| (0.0..=1.0).contains(v)), "{a:?}");
            for b in &colors[i + 1..] {
                assert!(color_distance(a, b) >= MIN_COLOR_DISTANCE, "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn explicit_colors_are_kept_and_avoided() {
        let data = af_core::testing::test_data();
        let faction = data.faction_id("tide").unwrap();
        let own = data.faction(faction).color;
        let players = vec![
            PlayerSetup { name: "A".into(), faction, team: 0, controller: Controller::Ai, color: None },
            PlayerSetup { name: "B".into(), faction, team: 1, controller: Controller::Ai, color: Some(own) },
        ];
        let colors = player_colors(&data, &players);
        assert_eq!(colors[1], own);
        assert!(color_distance(&colors[0], &own) >= MIN_COLOR_DISTANCE);
    }

    #[test]
    fn resolve_fills_and_sanitises() {
        let data = af_core::testing::test_data();
        let default = resolve_setup(&data, None);
        assert_eq!(default.players.len(), 2);
        assert_eq!(default.players[0].faction, FactionId(0));
        assert_eq!(default.players[1].faction, FactionId(1));
        assert_eq!(default.players[0].controller, Controller::Human);
        assert_eq!(default.local_player, Some(0));
        assert!(default.fog);

        let mut odd = MatchSetup { local_player: Some(40), start_age: 99, start_resources_mult: f32::NAN, ..default.clone() };
        odd.players[0].faction = FactionId(200);
        odd.players[0].team = 30;
        odd.players[1].team = 30;
        let fixed = resolve_setup(&data, Some(&odd));
        assert_eq!(fixed.players[0].faction, FactionId(0));
        assert_eq!((fixed.players[0].team, fixed.players[1].team), (0, 0), "allies stay allied");
        assert_eq!(fixed.local_player, None);
        assert_eq!(fixed.start_age, MAX_AGE);
        assert_eq!(fixed.start_resources_mult, 1.0);
    }

    #[test]
    fn start_age_researches_earlier_age_ups() {
        let data = af_core::testing::test_data();
        let setup = MatchSetup { start_age: 3, start_resources_mult: 2.0, ..resolve_setup(&data, None) };
        let players = build_players(&data, &setup, &[Vec2::ONE, Vec2::splat(2.0)]);
        for p in &players.list {
            assert_eq!(p.age, 3);
            for from in 1..=2 {
                let tech = data.age_up_tech(p.faction, from).expect("age-up exists");
                assert!(p.has_tech(tech), "{} researched", data.tech(tech).id);
            }
            let next = data.age_up_tech(p.faction, 3).expect("age-up exists");
            assert!(!p.has_tech(next));
            let food = data.resource_id("food").unwrap();
            assert_eq!(p.resources.get(food), data.faction(p.faction).start.resources.amounts.get(food) * 2.0);
        }
        assert_eq!(players.list[1].start_pos, Vec2::splat(2.0));
    }

    #[test]
    fn fog_has_a_grid_per_team() {
        let data = af_core::testing::test_data();
        let mut setup = resolve_setup(&data, None);
        setup.players[1].team = 3;
        setup.reveal_map = true;
        let players = build_players(&data, &setup, &[]);
        let fog = new_fog(&setup, 225.0, &players);
        assert_eq!((fog.width, fog.height, fog.cell), (113, 113, FOG_CELL));
        assert_eq!(fog.teams.len(), 4);
        assert!(fog.teams.iter().all(|t| t.explored.len() == 113 * 113 && t.explored.iter().all(|e| *e)));
        assert!(fog.visible(2, Vec2::new(100.0, 100.0)));
    }
}
