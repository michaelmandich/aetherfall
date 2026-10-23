//! The skirmish setup screen (`GameState::Menu`): pick a faction from six
//! cards, set the opponents (count, factions, teams), fog and seed, and the
//! map (size, water from dry to seas, rivers, the meteor site), then start
//! a match or watch an all-AI one, by mouse or keyboard (Left/Right:
//! faction, +/-: opponents, T: teams, F: fog, M: map size, W: water, R:
//! rivers, C: meteor site, with Shift stepping back, Enter: start, S:
//! spectate). Start and Spectate insert the `MatchSetup` and the
//! `MapOptions` next to it.

use af_core::ext::map::{MapOptions, MapSize, RiverCount, SiteChoice, SiteKind, WaterLevel};
use af_core::prelude::*;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;

use crate::format;
use crate::fx::PressFx;
use crate::skin::{ButtonSkin, SkinGlow, SkinLook, SkinPanel};
use crate::theme::{self, button, rgb, set_text, skinned_button, text, FONT_BODY, FONT_LARGE, FONT_SMALL, TEXT, TEXT_DIM};

/// Most AI opponents the menu offers.
pub const MAX_OPPONENTS: usize = 5;

/// The land sides a `MatchSetup::map_size` can give, in metres: af_map
/// rounds it to the metre and clamps it to this range (the range
/// `MapSize::land_side` keeps to as well).
pub const MAP_METRES_RANGE: (f32, f32) = (160.0, 1024.0);

/// The land side af_map builds for a `MatchSetup::map_size` of `metres`;
/// 0 when it does not set one (0, below 0, or not a number), so the size
/// option decides.
pub fn map_metres_used(metres: f32) -> f32 {
    if metres > 0.0 { metres.round().clamp(MAP_METRES_RANGE.0, MAP_METRES_RANGE.1) } else { 0.0 }
}

/// Which screen the menu shows: the skirmish setup, or the multiplayer
/// screens (`crate::multiplayer`). Entering the menu while connected to a
/// server goes straight back to the multiplayer screen.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuScreen {
    #[default]
    Skirmish,
    Multiplayer,
}

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
    /// How the map is generated.
    pub map: MapOptions,
    /// A map side in metres that overrides `map.size` (`--map-size`), as
    /// af_map will build it ([`map_metres_used`]); 0 = by `map.size`.
    /// Stepping the map size clears it.
    pub map_metres: f32,
    /// The start age and starting-resource multiplier (debug options the
    /// menu does not show, carried through from the command line).
    pub start_age: u8,
    pub res_mult: f32,
}

impl Default for MenuSettings {
    fn default() -> Self {
        let setup = MatchSetup::default();
        MenuSettings {
            faction: FactionId(0),
            opponents: vec![None],
            teams: TeamMode::FreeForAll,
            fog: true,
            seed: 1,
            map: MapOptions::default(),
            map_metres: setup.map_size,
            start_age: setup.start_age,
            res_mult: setup.start_resources_mult,
        }
    }
}

impl MenuSettings {
    /// Settings that reproduce an existing setup and its map options (e.g.
    /// ones built from the command line), so a run that starts on the menu
    /// keeps every option it was given.
    pub fn from_setup(setup: &MatchSetup, map: MapOptions) -> MenuSettings {
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
            map,
            // The side the match will have, which the menu then shows.
            map_metres: map_metres_used(setup.map_size),
            start_age: setup.start_age,
            res_mult: setup.start_resources_mult,
        }
    }

    /// Team of player slot `i`.
    pub fn team_of(&self, slot: usize) -> u8 {
        match self.teams {
            TeamMode::FreeForAll => slot as u8,
            TeamMode::TwoTeams => (slot % 2) as u8,
        }
    }

    /// The match these settings describe (its map options are `self.map`).
    /// Random opponent factions are drawn from the seed, so the same
    /// settings give the same match. `spectate` makes every slot AI with no
    /// local player and the map revealed.
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
            map_size: self.map_metres,
            players,
            local_player: if spectate { None } else { Some(0) },
            fog: self.fog,
            reveal_map: spectate,
            start_age: self.start_age,
            start_resources_mult: self.res_mult,
            ..Default::default()
        }
    }

    /// Step the map size (a set side in metres gives way to the presets).
    fn step_map_size(&mut self, forward: bool) {
        self.map.size = cycle(&MapSize::ALL, self.map.size, forward);
        self.map_metres = 0.0;
    }

    /// Step the water level, a scale that stops at both ends.
    fn step_water(&mut self, forward: bool) {
        let level = self.map.water.level();
        self.map.water = WaterLevel::from_level(if forward { level + 1 } else { level.saturating_sub(1) });
    }
}

/// The meteor site choices in menu order.
const SITES: [SiteChoice; 6] = [
    SiteChoice::Random,
    SiteChoice::Kind(SiteKind::Crater),
    SiteChoice::Kind(SiteKind::Shattered),
    SiteChoice::Kind(SiteKind::Lake),
    SiteChoice::Kind(SiteKind::Scar),
    SiteChoice::Kind(SiteKind::Spire),
];

/// The value after (or before) `current` in `all`, wrapping round.
fn cycle<T: Copy + PartialEq>(all: &[T], current: T, forward: bool) -> T {
    let n = all.len();
    let i = all.iter().position(|v| *v == current).unwrap_or(0);
    all[if forward { (i + 1) % n } else { (i + n - 1) % n }]
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
    /// Step a map option forward (`true`) or back.
    MapSize(bool),
    Water(bool),
    /// A segment of the water scale: that level.
    WaterLevel(WaterLevel),
    Rivers(bool),
    Site(bool),
    Start,
    Spectate,
    /// The multiplayer screens.
    Multiplayer,
}

/// Faction card (highlighted when chosen).
#[derive(Component, Clone, Copy, Debug)]
struct FactionCard(FactionId);

#[derive(Component, Clone, Copy, Debug)]
struct OpponentRow(usize);

/// One segment of the water scale, filled up to the chosen level.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct WaterSegment(WaterLevel);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum MenuLabel {
    OpponentCount,
    OpponentFaction(usize),
    OpponentTeam(usize),
    YourTeam,
    Teams,
    Fog,
    Seed,
    MapSize,
    MapSizeNote,
    WaterNote,
    Rivers,
    RiversNote,
    Site,
    SiteNote,
}

#[derive(Component)]
struct MenuRoot;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<MenuScreen>()
        .add_systems(OnEnter(GameState::Menu), (init_settings, pick_screen, spawn_menu).chain())
        .add_systems(
            Update,
            (
                (scroll_menu, menu_buttons, refresh_menu).chain().run_if(resource_equals(MenuScreen::Skirmish)),
                show_screen,
            )
                .chain()
                .in_set(ClientSet::Ui)
                .before(crate::skin::SkinSet)
                .run_if(in_state(GameState::Menu)),
        );
}

/// Back from a networked match (or still connected): the multiplayer
/// screen, where the room or the result is.
fn pick_screen(session: Option<Res<af_core::ext::net::NetSession>>, mut screen: ResMut<MenuScreen>) {
    let online = session.is_some_and(|s| s.status == af_core::ext::net::NetStatus::Online);
    if online {
        *screen = MenuScreen::Multiplayer;
    }
}

/// Only the chosen screen's root is laid out.
fn show_screen(screen: Res<MenuScreen>, mut roots: Query<(&mut Node, Has<MenuRoot>), Or<(With<MenuRoot>, With<crate::multiplayer::MultiplayerRoot>)>>) {
    for (mut node, skirmish) in &mut roots {
        theme::set_display(&mut node, skirmish == (*screen == MenuScreen::Skirmish));
    }
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

fn init_settings(mut commands: Commands, settings: Option<Res<MenuSettings>>, setup: Option<Res<MatchSetup>>, map: Option<Res<MapOptions>>, data: Res<Data>) {
    if settings.is_some() {
        return;
    }
    let map = map.map_or_else(MapOptions::default, |m| *m);
    let mut s = setup.map_or_else(|| MenuSettings { map, ..default() }, |s| MenuSettings::from_setup(&s, map));
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
/// Padding of the settings panel: its first and last rows clear of the
/// frame's corner plates, with an opponent's row still fitting across.
const SETTINGS_PADDING: UiRect = UiRect::axes(Val::Px(10.0), Val::Px(12.0));

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
                        right.spawn((theme::panel_node(Node { flex_direction: FlexDirection::Column, row_gap: px(8), padding: SETTINGS_PADDING, ..default() }), theme::panel_colors(), SkinPanel::Panel)).with_children(spawn_settings);
                        right.spawn(text("Map", FONT_LARGE, TEXT));
                        right.spawn((theme::panel_node(Node { flex_direction: FlexDirection::Column, row_gap: px(8), padding: SETTINGS_PADDING, ..default() }), theme::panel_colors(), SkinPanel::Panel)).with_children(spawn_map_settings);
                        right.spawn((skinned_button(Node { height: px(48), ..default() }, Color::srgb(0.16, 0.36, 0.18), ButtonSkin::Train), MenuButton::Start)).with_child(text("Start", FONT_LARGE, TEXT));
                        right.spawn((skinned_button(Node { height: px(44), ..default() }, Color::srgb(0.17, 0.25, 0.37), ButtonSkin::Command), MenuButton::Spectate))
                            .with_child(text("Spectate AI match", FONT_LARGE, TEXT));
                        right.spawn((skinned_button(Node { height: px(44), ..default() }, Color::srgb(0.3, 0.2, 0.38), ButtonSkin::Research), MenuButton::Multiplayer))
                            .with_child(text("Multiplayer", FONT_LARGE, TEXT));
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
        r.spawn((text("", FONT_SMALL, TEXT_DIM), theme::text_shadow(), MenuLabel::YourTeam));
    });
    for i in 0..MAX_OPPONENTS {
        panel.spawn((row(), OpponentRow(i))).with_children(|r| {
            r.spawn((text(format!("  Opponent {}", i + 1), FONT_SMALL, TEXT_DIM), theme::text_shadow(), Node { width: px(110), ..default() }));
            small_button(r, "<", MenuButton::OpponentPrev(i));
            r.spawn((
                text("Random", FONT_SMALL, TEXT),
                TextLayout::no_wrap(),
                MenuLabel::OpponentFaction(i),
                Node { width: px(OPPONENT_NAME_WIDTH), overflow: Overflow::clip(), ..default() },
            ));
            small_button(r, ">", MenuButton::OpponentNext(i));
            r.spawn((text("", FONT_SMALL, TEXT_DIM), theme::text_shadow(), TextLayout::no_wrap(), Node { flex_shrink: 0.0, ..default() }, MenuLabel::OpponentTeam(i)));
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

/// Width of a map option's value, and of the water scale in its place.
const MAP_VALUE_WIDTH: f32 = 150.0;
/// One water scale segment, and the gap between two.
const WATER_SEGMENT: f32 = 26.0;
const WATER_GAP: f32 = (MAP_VALUE_WIDTH - 5.0 * WATER_SEGMENT) / 4.0;

/// The map panel: one `< value >` row per option with a short note after
/// it, the water as a five-step scale, and what the chosen meteor site is.
fn spawn_map_settings(panel: &mut ChildSpawnerCommands) {
    let option = |panel: &mut ChildSpawnerCommands, name: &str, back: MenuButton, forward: MenuButton, value: MenuLabel, note: MenuLabel| {
        panel.spawn(row()).with_children(|r| {
            r.spawn((text(name, FONT_BODY, TEXT), Node { width: px(110), ..default() }));
            small_button(r, "<", back);
            r.spawn((text("", FONT_BODY, TEXT), TextLayout::no_wrap(), value, Node { width: px(MAP_VALUE_WIDTH), overflow: Overflow::clip(), ..default() }));
            small_button(r, ">", forward);
            r.spawn((text("", FONT_SMALL, TEXT_DIM), TextLayout::no_wrap(), note));
        });
    };
    option(panel, "Map size", MenuButton::MapSize(false), MenuButton::MapSize(true), MenuLabel::MapSize, MenuLabel::MapSizeNote);
    panel.spawn(row()).with_children(|r| {
        r.spawn((text("Water", FONT_BODY, TEXT), Node { width: px(110), ..default() }));
        small_button(r, "<", MenuButton::Water(false));
        r.spawn(Node { width: px(MAP_VALUE_WIDTH), column_gap: px(WATER_GAP), ..default() }).with_children(|scale| {
            for level in WaterLevel::ALL {
                scale.spawn((
                    // Colour swatches, not framed buttons: flat in the skin too.
                    theme::flat_button(Node { width: px(WATER_SEGMENT), height: px(20), ..default() }, theme::BUTTON),
                    MenuButton::WaterLevel(level),
                    WaterSegment(level),
                ));
            }
        });
        small_button(r, ">", MenuButton::Water(true));
        r.spawn((text("", FONT_SMALL, TEXT_DIM), TextLayout::no_wrap(), MenuLabel::WaterNote));
    });
    option(panel, "Rivers", MenuButton::Rivers(false), MenuButton::Rivers(true), MenuLabel::Rivers, MenuLabel::RiversNote);
    option(panel, "Meteor site", MenuButton::Site(false), MenuButton::Site(true), MenuLabel::Site, MenuLabel::SiteNote);
}

/// The map size's value and note: a side set in metres (as af_map will
/// build it), or the preset.
fn map_size_text(s: &MenuSettings) -> (String, &'static str) {
    let metres = map_metres_used(s.map_metres);
    if metres > 0.0 {
        return (format!("{metres:.0} m"), "set by --map-size");
    }
    let note = match s.map.size {
        MapSize::Small => "0.8x the land",
        MapSize::Medium => "the usual land",
        MapSize::Large => "1.3x the land",
        MapSize::Huge => "1.6x the land",
    };
    (s.map.size.name().into(), note)
}

/// What a water level means, after its name.
fn water_note(w: WaterLevel) -> String {
    let what = match w {
        WaterLevel::Dry => "rivers only",
        WaterLevel::Ponds => "a pond or two",
        WaterLevel::Lakes => "a few lakes",
        WaterLevel::Coast => "one sea edge",
        WaterLevel::Seas => "two sea edges",
    };
    format!("{}: {what}", w.name())
}

fn rivers_text(r: RiverCount) -> (&'static str, &'static str) {
    match r {
        RiverCount::Random => ("Random", "one or two"),
        RiverCount::One => ("One", "crossed at fords"),
        RiverCount::Two => ("Two", "crossed at fords"),
    }
}

/// A meteor site's name without its article (it fits the value box), and
/// what it is.
fn site_text(s: SiteChoice) -> (&'static str, &'static str) {
    let name = s.name();
    let short = name.strip_prefix("The ").unwrap_or(name);
    let what = match s {
        SiteChoice::Random => "one of five, by seed",
        SiteChoice::Kind(SiteKind::Crater) => "a rim with ramps",
        SiteChoice::Kind(SiteKind::Shattered) => "spread in craters",
        SiteChoice::Kind(SiteKind::Lake) => "flooded, an island",
        SiteChoice::Kind(SiteKind::Scar) => "three walled basins",
        SiteChoice::Kind(SiteKind::Spire) => "a spire, open ring",
    };
    (short, what)
}

/// Fill colour of a water scale segment at `level` when the scale reaches
/// it: from dry tan to deep sea blue.
fn water_color(level: WaterLevel) -> Color {
    match level {
        WaterLevel::Dry => Color::srgb(0.6, 0.5, 0.3),
        WaterLevel::Ponds => Color::srgb(0.34, 0.6, 0.74),
        WaterLevel::Lakes => Color::srgb(0.22, 0.49, 0.72),
        WaterLevel::Coast => Color::srgb(0.15, 0.37, 0.64),
        WaterLevel::Seas => Color::srgb(0.1, 0.26, 0.52),
    }
}

fn row() -> Node {
    Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8), ..default() }
}

fn small_button(parent: &mut ChildSpawnerCommands, label: &str, action: MenuButton) {
    parent.spawn((button(Node { width: px(30), height: px(26), padding: UiRect::all(px(2)), ..default() }, theme::BUTTON), action)).with_child(text(label, FONT_BODY, TEXT));
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
            // In the skin: a framed panel that glows on hover and while chosen.
            (SkinPanel::Panel, SkinGlow, SkinLook::default(), PressFx::on_click()),
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

/// The shortcut lines under the Start buttons (see [`key_actions`]).
const MENU_KEYS: &str = "Keys: Left/Right faction, +/- opponents, T teams, F fog,\nM map size, W water, R rivers, C meteor site (Shift: back),\nEnter start, S spectate, O multiplayer";

/// The menu's keyboard shortcuts, as the buttons they stand for: Left /
/// Right pick the faction, + / - change the number of opponents, T and F
/// toggle teams and fog, M, W, R and C step the map size, water, rivers
/// and meteor site forward (back with Shift held), Enter starts, S
/// spectates and O opens the multiplayer screens.
fn key_actions(keys: &ButtonInput<KeyCode>, settings: &MenuSettings, factions: usize) -> Vec<MenuButton> {
    let mut out = Vec::new();
    let n = factions.max(1);
    let step = |by: usize| MenuButton::Faction(FactionId(((settings.faction.index() + by) % n) as u8));
    let forward = !keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for key in keys.get_just_pressed() {
        out.push(match key {
            KeyCode::ArrowRight => step(1),
            KeyCode::ArrowLeft => step(n - 1),
            KeyCode::Equal | KeyCode::NumpadAdd => MenuButton::MoreOpponents,
            KeyCode::Minus | KeyCode::NumpadSubtract => MenuButton::FewerOpponents,
            KeyCode::KeyT => MenuButton::Teams,
            KeyCode::KeyF => MenuButton::Fog,
            KeyCode::KeyM => MenuButton::MapSize(forward),
            KeyCode::KeyW => MenuButton::Water(forward),
            KeyCode::KeyR => MenuButton::Rivers(forward),
            KeyCode::KeyC => MenuButton::Site(forward),
            KeyCode::Enter | KeyCode::NumpadEnter => MenuButton::Start,
            KeyCode::KeyS => MenuButton::Spectate,
            KeyCode::KeyO => MenuButton::Multiplayer,
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
    mut screen: ResMut<MenuScreen>,
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
            MenuButton::MapSize(forward) => settings.step_map_size(forward),
            MenuButton::Water(forward) => settings.step_water(forward),
            MenuButton::WaterLevel(level) => settings.map.water = level,
            MenuButton::Rivers(forward) => settings.map.rivers = cycle(&RiverCount::ALL, settings.map.rivers, forward),
            MenuButton::Site(forward) => settings.map.site = cycle(&SITES, settings.map.site, forward),
            MenuButton::Start | MenuButton::Spectate => {
                commands.insert_resource(settings.match_setup(&data, *action == MenuButton::Spectate));
                commands.insert_resource(settings.map);
                // A match left paused must not start frozen.
                time.unpause();
                next.set(GameState::Playing);
            }
            MenuButton::Multiplayer => *screen = MenuScreen::Multiplayer,
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
    mut cards: Query<(&FactionCard, &mut BorderColor, &mut SkinLook)>,
    mut segments: Query<(&WaterSegment, &Interaction, &mut theme::ShadedButton, &mut BackgroundColor)>,
) {
    if !settings.is_changed() && fresh.is_empty() {
        return;
    }
    let (size, size_note) = map_size_text(&settings);
    let (rivers, rivers_note) = rivers_text(settings.map.rivers);
    let (site, site_note) = site_text(settings.map.site);
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
            MenuLabel::MapSize => size.clone(),
            MenuLabel::MapSizeNote => size_note.into(),
            MenuLabel::WaterNote => water_note(settings.map.water),
            MenuLabel::Rivers => rivers.into(),
            MenuLabel::RiversNote => rivers_note.into(),
            MenuLabel::Site => site.into(),
            MenuLabel::SiteNote => site_note.into(),
        };
        set_text(&mut t, &s);
    }
    for (r, mut node) in &mut rows {
        theme::set_display(&mut node, r.0 < settings.opponents.len());
    }
    // The scale fills up to the chosen level, each segment in its own blue.
    for (segment, interaction, mut shaded, mut bg) in &mut segments {
        let base = if segment.0 <= settings.map.water { water_color(segment.0) } else { theme::BUTTON };
        if shaded.base != base {
            shaded.base = base;
            bg.0 = theme::shade(base, *interaction);
        }
    }
    for (card, mut border, mut look) in &mut cards {
        let chosen = card.0 == settings.faction;
        let c = if chosen { rgb(data.faction(card.0).color) } else { CARD_BORDER };
        *border = BorderColor::all(c);
        look.set_if_neq(SkinLook { active: chosen, accent: Some(c), ..default() });
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
        let s = MenuSettings { faction: dune, opponents: vec![Some(tide), None, None], teams: TeamMode::TwoTeams, fog: false, seed: 42, ..default() };
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
        let map = MapOptions { size: MapSize::Huge, water: WaterLevel::Seas, rivers: RiverCount::Two, site: SiteChoice::Kind(SiteKind::Spire) };
        let s = MenuSettings {
            faction: FactionId(3),
            opponents: vec![Some(FactionId(1)), Some(FactionId(5))],
            teams: TeamMode::FreeForAll,
            fog: true,
            seed: 7,
            map,
            map_metres: 0.0,
            start_age: 1,
            res_mult: 1.0,
        };
        assert_eq!(MenuSettings::from_setup(&s.match_setup(&data, false), s.map), s);
        let teams = MenuSettings { teams: TeamMode::TwoTeams, ..s.clone() };
        assert_eq!(MenuSettings::from_setup(&teams.match_setup(&data, false), teams.map), teams);
        let debug = MenuSettings { map_metres: 300.0, start_age: 3, res_mult: 2.5, map: MapOptions::default(), ..s };
        assert_eq!(MenuSettings::from_setup(&debug.match_setup(&data, false), debug.map), debug);
    }

    /// A run that starts on the menu keeps what the command line asked for:
    /// the map options, a map side in metres, the start age and resources.
    #[test]
    fn command_line_options_survive_the_menu() {
        let data = test_data();
        let slot = |i: u8| PlayerSetup { name: format!("P{i}"), faction: FactionId(i), team: i, controller: Controller::Ai, color: None };
        let cli = MatchSetup { seed: 9, map_size: 416.0, players: vec![slot(0), slot(1)], start_age: 3, start_resources_mult: 4.0, ..default() };
        let map = MapOptions { size: MapSize::Large, water: WaterLevel::Coast, rivers: RiverCount::One, site: SiteChoice::Kind(SiteKind::Lake) };
        let s = MenuSettings::from_setup(&cli, map);
        assert_eq!(s.map, map);
        let again = s.match_setup(&data, false);
        assert_eq!((again.map_size, again.start_age, again.start_resources_mult, again.seed), (416.0, 3, 4.0, 9));
        assert_eq!(map_size_text(&s).0, "416 m", "the menu shows the side it will use");
        // Stepping the size goes back to the presets.
        let mut stepped = s.clone();
        stepped.step_map_size(true);
        assert_eq!((stepped.map.size, stepped.map_metres), (MapSize::Huge, 0.0));
        assert_eq!(stepped.match_setup(&data, false).map_size, 0.0);
    }

    /// af_map rounds a side set in metres and keeps it to 160-1024 m; the
    /// menu shows (and starts) that side, not the one asked for.
    #[test]
    fn a_side_in_metres_shows_as_the_map_will_have_it() {
        let shown = |metres: f32| {
            let s = MenuSettings::from_setup(&MatchSetup { map_size: metres, ..default() }, MapOptions::default());
            (map_size_text(&s).0, s.match_setup(&test_data(), false).map_size)
        };
        assert_eq!(shown(2000.0), ("1024 m".to_string(), 1024.0));
        assert_eq!(shown(100.0), ("160 m".to_string(), 160.0));
        assert_eq!(shown(416.5), ("417 m".to_string(), 417.0), "rounded as af_map rounds");
        assert_eq!(shown(300.0), ("300 m".to_string(), 300.0));
        // No side set (0, below 0, not a number): the size option.
        for none in [0.0, -50.0, f32::NAN] {
            assert_eq!(shown(none), ("Medium".to_string(), 0.0), "{none}");
        }
        // Settings made some other way show the side too.
        let direct = MenuSettings { map_metres: 5000.0, ..default() };
        assert_eq!(map_size_text(&direct).0, "1024 m");
    }

    #[test]
    fn map_options_step_through_their_values() {
        let mut s = MenuSettings::default();
        assert_eq!(s.map, MapOptions::default());
        s.step_map_size(true);
        assert_eq!(s.map.size, MapSize::Large);
        s.step_map_size(true);
        s.step_map_size(true);
        assert_eq!(s.map.size, MapSize::Small, "sizes wrap round");
        // The water is a scale: it stops at both ends.
        s.step_water(true);
        s.step_water(true);
        s.step_water(true);
        assert_eq!(s.map.water, WaterLevel::Seas);
        for _ in 0..6 {
            s.step_water(false);
        }
        assert_eq!(s.map.water, WaterLevel::Dry);
        assert_eq!(cycle(&RiverCount::ALL, RiverCount::Two, true), RiverCount::Random);
        assert_eq!(cycle(&SITES, SiteChoice::Random, false), SiteChoice::Kind(SiteKind::Spire));
        let mut site = SiteChoice::Random;
        let mut kinds = Vec::new();
        for _ in 0..SITES.len() {
            site = cycle(&SITES, site, true);
            if let SiteChoice::Kind(k) = site {
                kinds.push(k);
            }
        }
        assert_eq!((site, kinds), (SiteChoice::Random, SiteKind::ALL.to_vec()), "every site kind once, then round to Random");
        // Every value has its words, short enough for the panel.
        for size in MapSize::ALL {
            let (name, note) = map_size_text(&MenuSettings { map: MapOptions { size, ..default() }, ..default() });
            assert!(name.len() * 10 <= MAP_VALUE_WIDTH as usize && note.len() <= 20 && note.is_ascii(), "{name}: {note}");
        }
        for w in WaterLevel::ALL {
            assert!(water_note(w).len() <= 20, "{}", water_note(w));
        }
        for site in SITES {
            let (name, note) = site_text(site);
            assert!(name.len() * 10 <= MAP_VALUE_WIDTH as usize && note.len() <= 20 && !name.starts_with("The "), "{name}: {note}");
        }
    }

    /// Clicking a segment of the water scale sets that level, and the scale
    /// fills up to it.
    #[test]
    fn the_water_scale_fills_up_to_the_clicked_level() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin));
        app.insert_state(GameState::Menu);
        app.insert_resource(Data(test_data()));
        plugin(&mut app);
        app.update();
        let filled = |app: &mut App| {
            let world = app.world_mut();
            let mut segments: Vec<(WaterLevel, bool)> =
                world.query::<(&WaterSegment, &BackgroundColor)>().iter(world).map(|(s, bg)| (s.0, bg.0 != theme::BUTTON)).collect();
            segments.sort_by_key(|(level, _)| *level);
            segments.into_iter().map(|(_, f)| f).collect::<Vec<_>>()
        };
        assert_eq!(filled(&mut app), [true, true, true, false, false], "Lakes, the default");
        for (level, want) in [(WaterLevel::Seas, [true; 5]), (WaterLevel::Dry, [true, false, false, false, false])] {
            let world = app.world_mut();
            let segment = world.query::<(Entity, &MenuButton)>().iter(world).find(|(_, b)| **b == MenuButton::WaterLevel(level)).map(|(e, _)| e).unwrap();
            world.entity_mut(segment).insert(Interaction::Pressed);
            app.update();
            assert_eq!(app.world().resource::<MenuSettings>().map.water, level);
            assert_eq!(filled(&mut app), want, "{level:?}");
            app.world_mut().entity_mut(segment).insert(Interaction::None);
        }
    }

    /// Start and Spectate put the map options next to the match setup.
    #[test]
    fn starting_inserts_the_setup_and_the_map_options() {
        for (key, spectate) in [(KeyCode::Enter, false), (KeyCode::KeyS, true)] {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin));
            app.insert_state(GameState::Menu);
            app.insert_resource(Data(test_data()));
            let map = MapOptions { size: MapSize::Small, water: WaterLevel::Dry, rivers: RiverCount::Two, site: SiteChoice::Kind(SiteKind::Scar) };
            app.insert_resource(MenuSettings { seed: 5, map, ..default() });
            app.init_resource::<MenuScreen>();
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(key);
            app.insert_resource(keys);
            app.add_systems(Update, menu_buttons);
            app.update();
            assert_eq!(app.world().get_resource::<MapOptions>(), Some(&map));
            let setup = app.world().resource::<MatchSetup>();
            assert_eq!((setup.seed, setup.local_player.is_none()), (5, spectate));
        }
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
        assert_eq!(pressed(KeyCode::KeyM), vec![MenuButton::MapSize(true)]);
        assert_eq!(pressed(KeyCode::KeyW), vec![MenuButton::Water(true)]);
        assert_eq!(pressed(KeyCode::KeyR), vec![MenuButton::Rivers(true)]);
        assert_eq!(pressed(KeyCode::KeyC), vec![MenuButton::Site(true)]);
        assert!(pressed(KeyCode::KeyQ).is_empty());
        assert!(MENU_KEYS.is_ascii());
        // Shift held steps the map options back.
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ShiftLeft);
        keys.clear();
        keys.press(KeyCode::KeyC);
        assert_eq!(key_actions(&keys, &s, 6), vec![MenuButton::Site(false)]);
    }

    #[test]
    fn faction_cycle_wraps_through_random() {
        assert_eq!(cycle_faction(None, 6, true), Some(FactionId(0)));
        assert_eq!(cycle_faction(Some(FactionId(5)), 6, true), None);
        assert_eq!(cycle_faction(None, 6, false), Some(FactionId(5)));
        assert_eq!(cycle_faction(Some(FactionId(0)), 6, false), None);
    }
}
