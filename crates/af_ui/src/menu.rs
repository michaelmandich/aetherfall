//! The skirmish setup screen (`GameState::Menu`): pick a faction from six
//! cards, set the opponents (count, factions, teams), fog and seed, then
//! start a match or watch an all-AI one, by mouse or keyboard (Left/Right:
//! faction, +/-: opponents, T: teams, F: fog, Enter: start, S: spectate).

use af_core::prelude::*;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;

use crate::format;
use crate::theme::{self, button, rgb, set_text, text, FONT_BODY, FONT_LARGE, FONT_SMALL, TEXT, TEXT_DIM};

/// Most AI opponents the menu offers.
pub const MAX_OPPONENTS: usize = 5;

/// How the menu assigns teams.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TeamMode {
    /// Everyone for themselves.
    #[default]
    FreeForAll,
    /// Slots alternate between team 0 (yours) and team 1.
    TwoTeams,
}

/// The choices on the setup screen. Kept across matches.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct MenuSettings {
    pub faction: FactionId,
    /// One entry per AI opponent; `None` = random faction.
    pub opponents: Vec<Option<FactionId>>,
    pub teams: TeamMode,
    pub fog: bool,
    pub seed: u64,
}

impl Default for MenuSettings {
    fn default() -> Self {
        MenuSettings { faction: FactionId(0), opponents: vec![None], teams: TeamMode::FreeForAll, fog: true, seed: 1 }
    }
}

impl MenuSettings {
    /// Settings that reproduce an existing setup (e.g. one built from the
    /// command line).
    pub fn from_setup(setup: &MatchSetup) -> MenuSettings {
        let me = setup.local_player.unwrap_or(0);
        let faction = setup.players.get(me).map_or(FactionId(0), |p| p.faction);
        let mut opponents: Vec<Option<FactionId>> =
            setup.players.iter().enumerate().filter(|(i, _)| *i != me).map(|(_, p)| Some(p.faction)).collect();
        opponents.truncate(MAX_OPPONENTS);
        if opponents.is_empty() {
            opponents.push(None);
        }
        let shared_team = setup.players.iter().enumerate().any(|(i, a)| setup.players[..i].iter().any(|b| b.team == a.team));
        MenuSettings {
            faction,
            opponents,
            teams: if shared_team { TeamMode::TwoTeams } else { TeamMode::FreeForAll },
            fog: setup.fog,
            seed: setup.seed,
        }
    }

    /// Team of player slot `i`.
    pub fn team_of(&self, slot: usize) -> u8 {
        match self.teams {
            TeamMode::FreeForAll => slot as u8,
            TeamMode::TwoTeams => (slot % 2) as u8,
        }
    }

    /// The match these settings describe. Random opponent factions are
    /// drawn from the seed, so the same settings give the same match.
    /// `spectate` makes every slot AI with no local player and the map revealed.
    pub fn match_setup(&self, data: &GameData, spectate: bool) -> MatchSetup {
        let factions = data.factions.len().max(1) as u32;
        let mut rng = SimRng::new(self.seed ^ 0x00FA_C710);
        let mut players = vec![PlayerSetup {
            name: if spectate { format!("{} (AI)", data.faction(self.faction).name) } else { "You".into() },
            faction: self.faction,
            team: self.team_of(0),
            controller: if spectate { Controller::Ai } else { Controller::Human },
            color: None,
        }];
        for (i, choice) in self.opponents.iter().take(MAX_OPPONENTS).enumerate() {
            let roll = FactionId(rng.below(factions) as u8);
            let faction = choice.unwrap_or(roll);
            players.push(PlayerSetup {
                name: format!("{} (AI)", data.faction(faction).name),
                faction,
                team: self.team_of(i + 1),
                controller: Controller::Ai,
                color: None,
            });
        }
        MatchSetup {
            seed: self.seed,
            players,
            local_player: if spectate { None } else { Some(0) },
            fog: self.fog,
            reveal_map: spectate,
            ..Default::default()
        }
    }
}

/// What a menu button does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum MenuButton {
    Faction(FactionId),
    FewerOpponents,
    MoreOpponents,
    /// Cycle opponent `i`'s faction back / forward through Random + factions.
    OpponentPrev(usize),
    OpponentNext(usize),
    Teams,
    Fog,
    RandomSeed,
    Start,
    Spectate,
}

/// Faction card (highlighted when chosen).
#[derive(Component, Clone, Copy, Debug)]
struct FactionCard(FactionId);

#[derive(Component, Clone, Copy, Debug)]
struct OpponentRow(usize);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum MenuLabel {
    OpponentCount,
    OpponentFaction(usize),
    OpponentTeam(usize),
    YourTeam,
    Teams,
    Fog,
    Seed,
}

#[derive(Component)]
struct MenuRoot;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(GameState::Menu), (init_settings, spawn_menu).chain())
        .add_systems(Update, (scroll_menu, menu_buttons, refresh_menu).chain().in_set(ClientSet::Ui).run_if(in_state(GameState::Menu)));
}

/// Logical pixels one wheel notch scrolls the menu.
const SCROLL_LINE_PX: f32 = 40.0;

/// Mouse wheel scrolls the menu when the window is too small for it.
fn scroll_menu(scroll: Option<Res<AccumulatedMouseScroll>>, mut roots: Query<(&mut ScrollPosition, &ComputedNode), With<MenuRoot>>) {
    let Some(scroll) = scroll else { return };
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
    };
    if lines == 0.0 {
        return;
    }
    for (mut pos, node) in &mut roots {
        // Layout clamps only the offset it draws, never `ScrollPosition`
        // itself: unclamped, scrolling past the end would build up a dead
        // zone of notches that do nothing on the way back.
        let max = ((node.content_size() - node.size()) * node.inverse_scale_factor()).y.max(0.0);
        let y = (pos.y - lines * SCROLL_LINE_PX).clamp(0.0, max);
        if pos.y != y {
            pos.y = y;
        }
    }
}

fn init_settings(mut commands: Commands, settings: Option<Res<MenuSettings>>, setup: Option<Res<MatchSetup>>, data: Res<Data>) {
    if settings.is_some() {
        return;
    }
    let mut s = setup.map_or_else(MenuSettings::default, |s| MenuSettings::from_setup(&s));
    if s.faction.index() >= data.factions.len() {
        s.faction = FactionId(0);
    }
    commands.insert_resource(s);
}

const CARD_BORDER: Color = Color::srgba(0.35, 0.33, 0.3, 0.9);
/// Faction card width: three cards and the settings column fit side by
/// side in a 1600 px window.
const CARD_WIDTH: f32 = 324.0;
/// Settings column width, and the opponent faction name inside it: room
/// for the longest name at `FONT_SMALL` without its article.
const SETTINGS_WIDTH: f32 = 540.0;
const OPPONENT_NAME_WIDTH: f32 = 262.0;
const OPPONENT_NAME_CHARS: usize = (OPPONENT_NAME_WIDTH / (FONT_SMALL * theme::GLYPH_ADVANCE)) as usize;

fn spawn_menu(mut commands: Commands, data: Res<Data>) {
    commands.spawn((Camera2d, DespawnOnExit(GameState::Menu)));
    commands
        .spawn((
            MenuRoot,
            DespawnOnExit(GameState::Menu),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::all(px(16)),
                row_gap: px(4),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.04, 0.05, 0.07)),
        ))
        .with_children(|root| {
            root.spawn(text("AETHERFALL", 52.0, theme::HIGHLIGHT));
            root.spawn(text("Knights who found a spaceship. Hold the fallen star.", FONT_LARGE, TEXT_DIM));
            // Faction cards on the left, match settings on the right; they
            // stack when the window is narrow.
            root.spawn(Node { flex_wrap: FlexWrap::Wrap, justify_content: JustifyContent::Center, column_gap: px(20), row_gap: px(14), margin: UiRect::top(px(6)), ..default() })
                .with_children(|body| {
                    body.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(8), ..default() }).with_children(|left| {
                        left.spawn(text("Choose your faction", FONT_LARGE, TEXT));
                        left.spawn(Node { flex_wrap: FlexWrap::Wrap, justify_content: JustifyContent::Center, column_gap: px(10), row_gap: px(10), max_width: px(3.0 * CARD_WIDTH + 2.0 * 10.0), ..default() })
                            .with_children(|grid| {
                                for f in data.faction_ids() {
                                    spawn_faction_card(grid, data.faction(f), f);
                                }
                            });
                    });
                    body.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(12), width: px(SETTINGS_WIDTH), ..default() }).with_children(|right| {
                        right.spawn(text("Match", FONT_LARGE, TEXT));
                        right.spawn((theme::panel_node(Node { flex_direction: FlexDirection::Column, row_gap: px(8), ..default() }), theme::panel_colors())).with_children(spawn_settings);
                        right.spawn((button(Node { height: px(48), ..default() }, Color::srgb(0.16, 0.36, 0.18)), MenuButton::Start)).with_child(text("Start", FONT_LARGE, TEXT));
                        right.spawn((button(Node { height: px(44), ..default() }, Color::srgb(0.17, 0.25, 0.37)), MenuButton::Spectate))
                            .with_child(text("Spectate AI match", FONT_LARGE, TEXT));
                        right.spawn(text(MENU_KEYS, FONT_SMALL, TEXT_DIM));
                    });
                });
        });
}

fn spawn_settings(panel: &mut ChildSpawnerCommands) {
    panel.spawn(row()).with_children(|r| {
        r.spawn((text("Opponents", FONT_BODY, TEXT), Node { width: px(110), ..default() }));
        small_button(r, "-", MenuButton::FewerOpponents);
        r.spawn((text("1", FONT_BODY, TEXT), MenuLabel::OpponentCount));
        small_button(r, "+", MenuButton::MoreOpponents);
        r.spawn((text("", FONT_SMALL, TEXT_DIM), MenuLabel::YourTeam));
    });
    for i in 0..MAX_OPPONENTS {
        panel.spawn((row(), OpponentRow(i))).with_children(|r| {
            r.spawn((text(format!("  Opponent {}", i + 1), FONT_SMALL, TEXT_DIM), Node { width: px(110), ..default() }));
            small_button(r, "<", MenuButton::OpponentPrev(i));
            r.spawn((
                text("Random", FONT_SMALL, TEXT),
                TextLayout::no_wrap(),
                MenuLabel::OpponentFaction(i),
                Node { width: px(OPPONENT_NAME_WIDTH), overflow: Overflow::clip(), ..default() },
            ));
            small_button(r, ">", MenuButton::OpponentNext(i));
            r.spawn((text("", FONT_SMALL, TEXT_DIM), TextLayout::no_wrap(), Node { flex_shrink: 0.0, ..default() }, MenuLabel::OpponentTeam(i)));
        });
    }
    panel.spawn(row()).with_children(|r| {
        r.spawn((text("Teams", FONT_BODY, TEXT), Node { width: px(110), ..default() }));
        labeled_button(r, MenuButton::Teams, MenuLabel::Teams, 170.0);
    });
    panel.spawn(row()).with_children(|r| {
        r.spawn((text("Fog of war", FONT_BODY, TEXT), Node { width: px(110), ..default() }));
        labeled_button(r, MenuButton::Fog, MenuLabel::Fog, 170.0);
    });
    panel.spawn(row()).with_children(|r| {
        r.spawn((text("Seed", FONT_BODY, TEXT), Node { width: px(110), ..default() }));
        r.spawn((text("1", FONT_BODY, TEXT), MenuLabel::Seed, Node { width: px(70), ..default() }));
        r.spawn((button(Node::default(), theme::BUTTON), MenuButton::RandomSeed)).with_child(text("Randomise", FONT_SMALL, TEXT));
    });
}

fn row() -> Node {
    Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8), ..default() }
}

fn small_button(parent: &mut ChildSpawnerCommands, label: &str, action: MenuButton) {
    parent.spawn((button(Node { width: px(30), height: px(26), padding: UiRect::ZERO, ..default() }, theme::BUTTON), action)).with_child(text(label, FONT_BODY, TEXT));
}

fn labeled_button(parent: &mut ChildSpawnerCommands, action: MenuButton, label: MenuLabel, width: f32) {
    parent.spawn((button(Node { width: px(width), ..default() }, theme::BUTTON), action)).with_child((text("", FONT_BODY, TEXT), label));
}

fn spawn_faction_card(parent: &mut ChildSpawnerCommands, f: &FactionDef, id: FactionId) {
    parent
        .spawn((
            Button,
            FactionCard(id),
            MenuButton::Faction(id),
            Node {
                width: px(CARD_WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                padding: UiRect::all(px(10)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme::PANEL_BG),
            BorderColor::all(CARD_BORDER),
            theme::ShadedButton { base: theme::PANEL_BG },
        ))
        .with_children(|card| {
            // A long name wraps under itself; the swatch stays by its first line.
            card.spawn(Node { align_items: AlignItems::FlexStart, ..row() }).with_children(|r| {
                r.spawn((
                    Node {
                        width: px(22),
                        height: px(22),
                        flex_shrink: 0.0,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(3)),
                        ..default()
                    },
                    BackgroundColor(rgb(f.color)),
                    BorderColor::all(Color::BLACK),
                ));
                r.spawn(text(f.name.clone(), FONT_LARGE, TEXT));
            });
            card.spawn(text(f.epithet.clone(), FONT_SMALL, theme::TEXT_WARN));
            for b in &f.bonuses {
                card.spawn(text(format!("+ {b}"), FONT_SMALL, TEXT));
            }
            card.spawn(text(format!("Terrain: {}", format::faction_traits(f)), FONT_SMALL, theme::TEXT_GOOD));
        });
}

/// The shortcut line under the Start buttons (see [`key_actions`]).
const MENU_KEYS: &str = "Keys: Left/Right faction, +/- opponents, T teams,\nF fog, Enter start, S spectate";

/// The menu's keyboard shortcuts, as the buttons they stand for: Left /
/// Right pick the faction, + / - change the number of opponents, T and F
/// toggle teams and fog, Enter starts and S spectates.
fn key_actions(keys: &ButtonInput<KeyCode>, settings: &MenuSettings, factions: usize) -> Vec<MenuButton> {
    let mut out = Vec::new();
    let n = factions.max(1);
    let step = |by: usize| MenuButton::Faction(FactionId(((settings.faction.index() + by) % n) as u8));
    for key in keys.get_just_pressed() {
        out.push(match key {
            KeyCode::ArrowRight => step(1),
            KeyCode::ArrowLeft => step(n - 1),
            KeyCode::Equal | KeyCode::NumpadAdd => MenuButton::MoreOpponents,
            KeyCode::Minus | KeyCode::NumpadSubtract => MenuButton::FewerOpponents,
            KeyCode::KeyT => MenuButton::Teams,
            KeyCode::KeyF => MenuButton::Fog,
            KeyCode::Enter | KeyCode::NumpadEnter => MenuButton::Start,
            KeyCode::KeyS => MenuButton::Spectate,
            _ => continue,
        });
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn menu_buttons(
    mut commands: Commands,
    buttons: Query<(&Interaction, &MenuButton), Changed<Interaction>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    data: Res<Data>,
    mut settings: ResMut<MenuSettings>,
    mut next: ResMut<NextState<GameState>>,
    mut time: ResMut<Time<Virtual>>,
) {
    let factions = data.factions.len();
    let mut actions: Vec<MenuButton> = buttons.iter().filter(|(i, _)| **i == Interaction::Pressed).map(|(_, a)| *a).collect();
    if let Some(keys) = keys {
        actions.extend(key_actions(&keys, &settings, factions));
    }
    for action in &actions {
        match *action {
            MenuButton::Faction(f) => settings.faction = f,
            MenuButton::FewerOpponents => {
                if settings.opponents.len() > 1 {
                    settings.opponents.pop();
                }
            }
            MenuButton::MoreOpponents => {
                if settings.opponents.len() < MAX_OPPONENTS {
                    settings.opponents.push(None);
                }
            }
            MenuButton::OpponentPrev(i) | MenuButton::OpponentNext(i) => {
                let forward = matches!(action, MenuButton::OpponentNext(_));
                if let Some(slot) = settings.opponents.get_mut(i) {
                    *slot = cycle_faction(*slot, factions, forward);
                }
            }
            MenuButton::Teams => {
                settings.teams = match settings.teams {
                    TeamMode::FreeForAll => TeamMode::TwoTeams,
                    TeamMode::TwoTeams => TeamMode::FreeForAll,
                }
            }
            MenuButton::Fog => settings.fog = !settings.fog,
            MenuButton::RandomSeed => settings.seed = random_seed(),
            MenuButton::Start | MenuButton::Spectate => {
                commands.insert_resource(settings.match_setup(&data, *action == MenuButton::Spectate));
                // A match left paused must not start frozen.
                time.unpause();
                next.set(GameState::Playing);
            }
        }
    }
}

/// Step through Random, faction 0, faction 1, ... and around.
fn cycle_faction(current: Option<FactionId>, factions: usize, forward: bool) -> Option<FactionId> {
    let n = factions + 1;
    let i = current.map_or(0, |f| f.index() + 1);
    let j = if forward { (i + 1) % n } else { (i + n - 1) % n };
    (j > 0).then(|| FactionId((j - 1) as u8))
}

/// A fresh, short seed from the wall clock (menu only; the sim stays seeded).
fn random_seed() -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    SimRng::new(nanos).next_u64() % 1_000_000
}

#[allow(clippy::type_complexity)]
fn refresh_menu(
    settings: Res<MenuSettings>,
    data: Res<Data>,
    fresh: Query<(), Added<MenuRoot>>,
    mut labels: Query<(&MenuLabel, &mut Text)>,
    mut rows: Query<(&OpponentRow, &mut Node)>,
    mut cards: Query<(&FactionCard, &mut BorderColor)>,
) {
    if !settings.is_changed() && fresh.is_empty() {
        return;
    }
    for (label, mut t) in &mut labels {
        let s = match *label {
            MenuLabel::OpponentCount => settings.opponents.len().to_string(),
            MenuLabel::OpponentFaction(i) => match settings.opponents.get(i) {
                Some(Some(f)) => format::elide(format::short_faction(&data.faction(*f).name), OPPONENT_NAME_CHARS),
                _ => "Random".into(),
            },
            MenuLabel::OpponentTeam(i) => format!("Team {}", settings.team_of(i + 1) + 1),
            MenuLabel::YourTeam => format!("(you are team {})", settings.team_of(0) + 1),
            MenuLabel::Teams => match settings.teams {
                TeamMode::FreeForAll => "Free for all".into(),
                TeamMode::TwoTeams => "Two teams".into(),
            },
            MenuLabel::Fog => if settings.fog { "On" } else { "Off" }.into(),
            MenuLabel::Seed => settings.seed.to_string(),
        };
        set_text(&mut t, &s);
    }
    for (r, mut node) in &mut rows {
        theme::set_display(&mut node, r.0 < settings.opponents.len());
    }
    for (card, mut border) in &mut cards {
        let c = if card.0 == settings.faction { rgb(data.faction(card.0).color) } else { CARD_BORDER };
        *border = BorderColor::all(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::test_data;

    #[test]
    fn setup_from_settings() {
        let data = test_data();
        let dune = data.faction_id("dune").unwrap();
        let tide = data.faction_id("tide").unwrap();
        let s = MenuSettings { faction: dune, opponents: vec![Some(tide), None, None], teams: TeamMode::TwoTeams, fog: false, seed: 42 };
        let m = s.match_setup(&data, false);
        assert_eq!(m.players.len(), 4);
        assert_eq!(m.local_player, Some(0));
        assert_eq!(m.players[0].faction, dune);
        assert_eq!(m.players[0].controller, Controller::Human);
        assert_eq!(m.players[1].faction, tide);
        assert!(m.players[1..].iter().all(|p| p.controller == Controller::Ai));
        assert_eq!(m.players.iter().map(|p| p.team).collect::<Vec<_>>(), vec![0, 1, 0, 1]);
        assert!(!m.fog && !m.reveal_map);
        assert_eq!(m.seed, 42);
        // Random factions come from the seed.
        assert_eq!(s.match_setup(&data, false).players[2].faction, m.players[2].faction);
        let ffa = MenuSettings { teams: TeamMode::FreeForAll, ..s.clone() }.match_setup(&data, false);
        assert_eq!(ffa.players.iter().map(|p| p.team).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn spectate_is_all_ai_and_revealed() {
        let data = test_data();
        let m = MenuSettings::default().match_setup(&data, true);
        assert_eq!(m.local_player, None);
        assert!(m.reveal_map);
        assert!(m.players.iter().all(|p| p.controller == Controller::Ai));
    }

    #[test]
    fn settings_round_trip_through_a_setup() {
        let data = test_data();
        let s = MenuSettings { faction: FactionId(3), opponents: vec![Some(FactionId(1)), Some(FactionId(5))], teams: TeamMode::FreeForAll, fog: true, seed: 7 };
        assert_eq!(MenuSettings::from_setup(&s.match_setup(&data, false)), s);
        let teams = MenuSettings { teams: TeamMode::TwoTeams, ..s };
        assert_eq!(MenuSettings::from_setup(&teams.match_setup(&data, false)), teams);
    }

    #[test]
    fn wheel_scrolling_stops_at_both_ends() {
        let mut app = App::new();
        app.add_systems(Update, scroll_menu);
        // 400 physical px of overflow at a scale factor of 2: 200 logical px.
        let computed = ComputedNode { size: Vec2::new(1600.0, 1200.0), content_size: Vec2::new(1600.0, 1600.0), inverse_scale_factor: 0.5, ..default() };
        let root = app.world_mut().spawn((MenuRoot, Node::default(), computed)).id();
        let wheel = |app: &mut App, lines: f32| {
            app.insert_resource(AccumulatedMouseScroll { unit: MouseScrollUnit::Line, delta: Vec2::new(0.0, lines) });
            app.update();
            app.world().get::<ScrollPosition>(root).unwrap().y
        };
        assert_eq!(wheel(&mut app, -2.0), 80.0);
        assert_eq!(wheel(&mut app, -100.0), 200.0);
        // Back up by one notch moves at once: no dead zone past the end.
        assert_eq!(wheel(&mut app, 1.0), 160.0);
        assert_eq!(wheel(&mut app, 100.0), 0.0);
    }

    #[test]
    fn keys_stand_for_menu_buttons() {
        let s = MenuSettings { faction: FactionId(5), ..default() };
        let pressed = |key: KeyCode| {
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(key);
            key_actions(&keys, &s, 6)
        };
        assert_eq!(pressed(KeyCode::ArrowRight), vec![MenuButton::Faction(FactionId(0))], "wraps round");
        assert_eq!(pressed(KeyCode::ArrowLeft), vec![MenuButton::Faction(FactionId(4))]);
        assert_eq!(pressed(KeyCode::Equal), vec![MenuButton::MoreOpponents]);
        assert_eq!(pressed(KeyCode::Minus), vec![MenuButton::FewerOpponents]);
        assert_eq!(pressed(KeyCode::Enter), vec![MenuButton::Start]);
        assert_eq!(pressed(KeyCode::KeyS), vec![MenuButton::Spectate]);
        assert_eq!(pressed(KeyCode::KeyF), vec![MenuButton::Fog]);
        assert!(pressed(KeyCode::KeyQ).is_empty());
        assert!(MENU_KEYS.is_ascii());
    }

    #[test]
    fn faction_cycle_wraps_through_random() {
        assert_eq!(cycle_faction(None, 6, true), Some(FactionId(0)));
        assert_eq!(cycle_faction(Some(FactionId(5)), 6, true), None);
        assert_eq!(cycle_faction(None, 6, false), Some(FactionId(5)));
        assert_eq!(cycle_faction(Some(FactionId(0)), 6, false), None);
    }
}
