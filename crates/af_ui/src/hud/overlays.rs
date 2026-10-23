//! Screen overlays: the drag-select box, the floating hover tip, the
//! cursor hint (what a targeting or placing click will do, or why the
//! building cannot go there), the pause banner, the game-over screen
//! (title, per-player stats, back to menu or keep watching), and the way
//! out of a match at any time: the "Menu" button under the top bar and
//! [`MENU_KEY`]. Once the match is over they bring back the game-over
//! screen after "Keep watching"; before that (a one-player sandbox never
//! ends, a defeated player may wait on the others) they open the leave
//! panel, which asks before going back to the menu.

use af_core::ext::input::{CursorOverride, PlacementGhost, PlacementPreview};
use af_core::prelude::*;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::window::PrimaryWindow;

use super::HudSet;
use crate::format;
use crate::skin::{ButtonSkin, SkinPanel};
use crate::tooltip;
use crate::theme::{self, set_display, set_text, text, Tip, FONT_BODY, FONT_LARGE, FONT_SMALL, TEXT, TEXT_DIM};

/// Opens the leave panel, or brings back the dismissed game-over screen
/// (the classic RTS game-menu key; af_input leaves it alone).
pub(crate) const MENU_KEY: KeyCode = KeyCode::F10;

/// Which full-screen panel is up: the game-over screen (once the match has
/// ended, until "Keep watching") or the leave panel. Either blocks command
/// hotkeys and the minimap.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub(crate) struct GameOverState {
    /// The match ended and its screen was filled in.
    shown: bool,
    /// "Keep watching" put the game-over screen away (the menu button and
    /// [`MENU_KEY`] bring it back).
    dismissed: bool,
    /// The leave panel is open (only while the match goes on).
    leaving: bool,
}

impl GameOverState {
    pub fn blocking(&self) -> bool {
        self.game_over_up() || self.leaving
    }

    fn game_over_up(&self) -> bool {
        self.shown && !self.dismissed
    }

    /// The menu button or key: toggles the game-over screen once the match
    /// is over, else the leave panel.
    fn toggle_menu(&mut self) {
        if self.shown {
            self.dismissed = !self.dismissed;
        } else {
            self.leaving = !self.leaving;
        }
    }
}

#[derive(Component)]
struct DragBox;

#[derive(Component)]
struct HoverTip;

/// What the next click does, or why the building at the cursor cannot be
/// placed ([`cursor_hint`]).
#[derive(Component)]
struct PlacementHint;

#[derive(Component)]
struct PauseBanner;

#[derive(Component)]
pub(crate) struct GameOverScreen;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum GameOverText {
    Title,
    Subtitle,
}

/// Grid the per-player rows are added to when the match ends.
#[derive(Component)]
struct StatsTable;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum GameOverButton {
    BackToMenu,
    KeepWatching,
}

/// The always-present "Menu" button under the top bar ([`MENU_KEY`]).
#[derive(Component)]
struct MenuButton;

/// "Leave the match?": back to the menu, or resume.
#[derive(Component)]
pub(crate) struct LeavePanel;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum LeaveButton {
    Leave,
    Resume,
}

const STATS_COLUMNS: [&str; 10] = ["Player", "Faction", "Team", "Result", "Gathered", "Trained", "Lost", "Built", "Razed", "Kills"];
/// The stats table's name columns (player, faction) hold this many
/// characters, so the table fits its panel in a 1280 px window.
const STATS_NAME_CHARS: usize = 30;
/// Which columns hold numbers (right-aligned); the rest are words.
const STATS_NUMERIC: [bool; 10] = [false, false, true, false, true, true, true, true, true, true];
const STATS_COLUMN_GAP: f32 = 18.0;
/// Padding of the game-over panel (the table keeps clear of its border).
const GAME_OVER_PADDING: f32 = 24.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<GameOverState>()
        .add_systems(
            Update,
            (update_drag_box, update_hover_tip, update_placement_hint, update_pause_banner, (show_game_over, game_menu, show_panels).chain()).in_set(HudSet::Panels),
        );
}

pub(super) fn spawn(root: &mut ChildSpawnerCommands) {
    root.spawn((
        DragBox,
        Node { position_type: PositionType::Absolute, border: UiRect::all(px(1)), display: Display::None, ..default() },
        BackgroundColor(Color::srgba(0.3, 1.0, 0.35, 0.08)),
        BorderColor::all(Color::srgb(0.3, 1.0, 0.35)),
        Pickable::IGNORE,
    ));
    root.spawn((
        HoverTip,
        theme::panel_node(Node { position_type: PositionType::Absolute, max_width: px(360), padding: UiRect::axes(px(8), px(4)), display: Display::None, ..default() }),
        theme::panel_colors(),
        SkinPanel::Bar,
        Pickable::IGNORE,
    ))
    .with_child((text("", FONT_SMALL, TEXT), Pickable::IGNORE));
    root.spawn((
        PlacementHint,
        Node { position_type: PositionType::Absolute, padding: UiRect::axes(px(6), px(2)), display: Display::None, ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        Pickable::IGNORE,
    ))
    .with_child((text("", FONT_SMALL, theme::TEXT_BAD), Pickable::IGNORE));
    root.spawn((
        PauseBanner,
        Node { position_type: PositionType::Absolute, top: px(110), width: percent(100), justify_content: JustifyContent::Center, display: Display::None, ..default() },
        Pickable::IGNORE,
    ))
    .with_children(|row| {
        // Only an indicator: world clicks and the wheel pass through it.
        row.spawn((theme::panel_node(Node { padding: UiRect::axes(px(28), px(10)), ..default() }), theme::panel_colors(), SkinPanel::Bar, Pickable::IGNORE))
            .with_child((text("PAUSED", 32.0, theme::HIGHLIGHT), Pickable::IGNORE));
    });
    // Top right, under the bar (the idle-worker button is on the left).
    root.spawn((
        theme::button(Node { position_type: PositionType::Absolute, top: px(42), right: px(8), ..default() }, theme::BUTTON),
        MenuButton,
        Tip("Leave the match, or bring back the game-over screen (F10)".into()),
    ))
    .with_child(text("Menu (F10)", FONT_SMALL, TEXT));
    root.spawn((
        GameOverScreen,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        // Swallow clicks so the HUD underneath cannot be used meanwhile.
        FocusPolicy::Block,
    ))
    .with_children(|screen| {
        screen
            .spawn((
                theme::panel_node(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(12),
                    padding: UiRect::all(px(GAME_OVER_PADDING)),
                    max_width: percent(96),
                    ..default()
                }),
                // Opaque: the world's bars and boxes must not show through the table.
                BackgroundColor(theme::PANEL_BG.with_alpha(1.0)),
                BorderColor::all(theme::PANEL_BORDER),
                SkinPanel::Panel,
            ))
            .with_children(|panel| {
                panel.spawn((text("", 48.0, theme::HIGHLIGHT), GameOverText::Title));
                panel.spawn((text("", FONT_LARGE, TEXT), GameOverText::Subtitle));
                panel.spawn((
                    StatsTable,
                    Node {
                        display: Display::Grid,
                        grid_template_columns: vec![RepeatedGridTrack::auto(STATS_COLUMNS.len() as u16)],
                        column_gap: px(STATS_COLUMN_GAP),
                        row_gap: px(4),
                        // Never wider than the panel: names are cut to fit
                        // (see `stats_row`), and anything left over is clipped
                        // inside the padding rather than drawn past the border.
                        max_width: percent(100),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ));
                panel.spawn(Node { column_gap: px(16), margin: UiRect::top(px(8)), ..default() }).with_children(|r| {
                    r.spawn((theme::skinned_button(Node { width: px(200), height: px(44), ..default() }, Color::srgb(0.17, 0.25, 0.37), ButtonSkin::Command), GameOverButton::BackToMenu))
                        .with_child(text("Back to menu", FONT_LARGE, TEXT));
                    r.spawn((theme::button(Node { width: px(200), height: px(44), ..default() }, theme::BUTTON), GameOverButton::KeepWatching))
                        .with_child(text("Keep watching", FONT_LARGE, TEXT));
                });
            });
    });
    root.spawn((
        LeavePanel,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        // Like the game-over screen: the HUD and the world wait underneath.
        FocusPolicy::Block,
    ))
    .with_children(|screen| {
        screen
            .spawn((
                theme::panel_node(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(12),
                    padding: UiRect::all(px(GAME_OVER_PADDING)),
                    ..default()
                }),
                BackgroundColor(theme::PANEL_BG.with_alpha(1.0)),
                BorderColor::all(theme::PANEL_BORDER),
                SkinPanel::Panel,
            ))
            .with_children(|panel| {
                panel.spawn(text("Leave the match?", 32.0, theme::HIGHLIGHT));
                panel.spawn((text("The match goes on without you.", FONT_BODY, TEXT_DIM), theme::text_shadow()));
                panel.spawn(Node { column_gap: px(16), margin: UiRect::top(px(8)), ..default() }).with_children(|r| {
                    r.spawn((theme::skinned_button(Node { width: px(200), height: px(44), ..default() }, Color::srgb(0.17, 0.25, 0.37), ButtonSkin::Command), LeaveButton::Leave))
                        .with_child(text("Leave to menu", FONT_LARGE, TEXT));
                    r.spawn((theme::button(Node { width: px(200), height: px(44), ..default() }, theme::BUTTON), LeaveButton::Resume))
                        .with_child(text("Resume", FONT_LARGE, TEXT));
                });
            });
    });
}

fn update_drag_box(drag: Res<DragSelect>, mut boxes: Query<&mut Node, With<DragBox>>) {
    for mut node in &mut boxes {
        set_display(&mut node, drag.0.is_some());
        if let Some((a, b)) = drag.0 {
            let (min, size) = (a.min(b), (a - b).abs());
            let (left, top, width, height) = (px(min.x), px(min.y), px(size.x), px(size.y));
            if node.left != left || node.top != top || node.width != width || node.height != height {
                node.left = left;
                node.top = top;
                node.width = width;
                node.height = height;
            }
        }
    }
}

/// The tip of the hovered node, below-right of the mouse (or of its
/// stand-in, `CursorOverride`, in a scripted run).
fn update_hover_tip(
    tips: Query<(&Interaction, &Tip)>,
    window: Query<&Window, With<PrimaryWindow>>,
    stand_in: Option<Res<CursorOverride>>,
    mut panel: Query<(&mut Node, &Children), With<HoverTip>>,
    mut texts: Query<&mut Text>,
) {
    let tip = tips.iter().find(|(i, t)| **i == Interaction::Hovered && !t.0.is_empty()).map(|(_, t)| t);
    let window = window.single().ok();
    let cursor = stand_in.and_then(|o| o.0).or_else(|| window.and_then(Window::cursor_position));
    for (mut node, children) in &mut panel {
        let (Some(tip), Some(cursor)) = (tip, cursor) else {
            set_display(&mut node, false);
            continue;
        };
        set_display(&mut node, true);
        // Below-right of the cursor, flipped left near the right edge.
        let width = window.map_or(f32::MAX, Window::width);
        let left = if cursor.x + 380.0 > width { (cursor.x - 370.0).max(0.0) } else { cursor.x + 14.0 };
        let (l, t) = (px(left), px(cursor.y + 20.0));
        if node.left != l || node.top != t {
            node.left = l;
            node.top = t;
        }
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child) {
                set_text(&mut text, &tip.0);
            }
        }
    }
}

/// The colour of a placement hint: warning yellow when the spot is fine and
/// only the cost is missing (the amber ghost), alert red when it cannot go
/// there at all (the red, crossed ghost).
pub(crate) fn hint_color(error: PlacementError) -> Color {
    match error {
        PlacementError::Rule(Reason::CannotAfford) => theme::TEXT_WARN,
        _ => theme::TEXT_BAD,
    }
}

/// What the next world click does in `mode`, as the line or two shown by
/// the cursor: why a placement would fail (from the preview's `ghosts`,
/// coloured by [`hint_color`]; for a wall line of which only some segments
/// fit, how many will be built), else what to click and how to get out.
/// `None` in Normal mode, where a click just selects.
pub(crate) fn cursor_hint(data: &GameData, player: Option<&PlayerState>, mode: &InputMode, ghosts: &[PlacementGhost]) -> Option<(String, Color)> {
    const CANCEL: &str = "Right-click or Esc: cancel";
    const QUEUE: &str = "Shift: queue it. Right-click or Esc: cancel";
    // A missing payment says what is missing: for one building, or for
    // every segment of a wall line that has room (the rest of the line).
    let describe = |g: &PlacementGhost, e: PlacementError| match (e, player) {
        (PlacementError::Rule(Reason::CannotAfford), Some(p)) => {
            let roomy = ghosts.iter().filter(|o| matches!(o.error, None | Some(PlacementError::Rule(Reason::CannotAfford)))).count();
            let cost = stats::building_cost(data, p, g.kind).scaled(roomy.max(1) as f32);
            let rest = if roomy > 1 { " for the rest" } else { "" };
            tooltip::shortfall(data, p, &cost).map_or_else(|| e.describe(data), |s| format!("{s}{rest}"))
        }
        _ => e.describe(data),
    };
    if matches!(mode, InputMode::Place { .. } | InputMode::PlaceWall { .. }) {
        let blocked = ghosts.iter().filter(|g| g.error.is_some()).count();
        if let Some((g, first)) = ghosts.iter().find_map(|g| g.error.map(|e| (g, e))) {
            if blocked < ghosts.len() {
                // A wall line: the click builds the segments that fit.
                let fit = ghosts.len() - blocked;
                return Some((format!("{fit} of {} segments will be built\n{}", ghosts.len(), describe(g, first)), theme::TEXT_WARN));
            }
            return Some((describe(g, first), hint_color(first)));
        }
    }
    let text = match *mode {
        InputMode::Normal => return None,
        InputMode::Place { kind, .. } => format!("Click to place the {}\nR: rotate. Shift: place more. {CANCEL}", data.building(kind).name),
        InputMode::PlaceWall { kind, start: None } => format!("Click where the {} starts\n{CANCEL}", data.building(kind).name),
        InputMode::PlaceWall { start: Some(_), .. } => format!("Click where the wall ends\nShift: go on from there. {CANCEL}"),
        InputMode::Target(action) => {
            let what = match action {
                TargetAction::Move => "Move: click a destination",
                TargetAction::AttackMove => "Attack: click an enemy, or ground to attack-move",
                TargetAction::Attack => "Attack: click an enemy",
                TargetAction::Patrol => "Patrol: click the far end of the route (Shift-click more points for a loop)",
                TargetAction::Rally => "Rally: click where new units should go",
                TargetAction::Gather => "Gather: click a resource or a farm",
                TargetAction::Repair => "Repair: click a damaged building or machine",
            };
            format!("{what}\n{QUEUE}")
        }
    };
    Some((text, TEXT))
}

/// Logical px between the cursor and its hint.
const HINT_OFFSET: f32 = 18.0;

/// What the next click does (or why a building cannot go there), next to
/// the cursor as af_input sees it (`CursorWorld::screen`: the mouse, or its
/// stand-in), kept inside the window. Hidden over the HUD.
#[allow(clippy::too_many_arguments)]
fn update_placement_hint(
    preview: Option<Res<PlacementPreview>>,
    data: Res<Data>,
    mode: Res<InputMode>,
    local: Res<LocalPlayer>,
    players: Option<Res<Players>>,
    pointer: Res<CursorWorld>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut hint: Query<(&mut Node, &Children), With<PlacementHint>>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let ghosts = preview.as_deref().map_or(&[][..], |p| p.ghosts.as_slice());
    let player = local.0.zip(players.as_deref()).and_then(|(p, ps)| ps.try_get(p));
    let shown = local.0.and(pointer.screen.filter(|_| !pointer.over_ui)).zip(cursor_hint(&data, player, &mode, ghosts));
    let width = window.single().map_or(f32::MAX, Window::width);
    for (mut node, children) in &mut hint {
        let Some((cursor, (s, color))) = &shown else {
            set_display(&mut node, false);
            continue;
        };
        set_display(&mut node, true);
        // Above-right of the cursor; flipped left near the right edge.
        let lines = s.lines().count().max(1) as f32;
        let w = s.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * FONT_SMALL * theme::GLYPH_ADVANCE + 12.0;
        let left = if cursor.x + HINT_OFFSET + w > width { (cursor.x - HINT_OFFSET - w).max(0.0) } else { cursor.x + HINT_OFFSET };
        let top = (cursor.y - HINT_OFFSET - lines * FONT_SMALL * 1.2).max(0.0);
        let (l, t) = (px(left), px(top));
        if node.left != l || node.top != t {
            node.left = l;
            node.top = t;
        }
        for child in children.iter() {
            if let Ok((mut text, mut c)) = texts.get_mut(child) {
                set_text(&mut text, s);
                theme::set_color(&mut c, *color);
            }
        }
    }
}

fn update_pause_banner(time: Res<Time<Virtual>>, state: Res<GameOverState>, mut banner: Query<&mut Node, With<PauseBanner>>) {
    for mut node in &mut banner {
        set_display(&mut node, time.is_paused() && !state.blocking());
    }
}

/// "VICTORY" / "DEFEAT" for a player, "Team N wins" for spectators.
pub fn outcome_title(local_team: Option<u8>, winning_team: Option<u8>) -> String {
    match (local_team, winning_team) {
        (Some(t), Some(w)) if t == w => "VICTORY".into(),
        (Some(_), _) => "DEFEAT".into(),
        (None, Some(w)) => format!("Team {} wins", w + 1),
        (None, None) => "No winner".into(),
    }
}

/// One stats row per player: the cells under `STATS_COLUMNS`, the names
/// cut to [`STATS_NAME_CHARS`] so the table fits its panel.
fn stats_row(data: &GameData, p: &PlayerState, winner: Option<u8>) -> [String; 10] {
    let st = &p.stats;
    let result = if Some(p.team) == winner { "Won" } else if p.defeated { "Defeated" } else { "Lost" };
    [
        format::short_player(&p.name, STATS_NAME_CHARS),
        format::elide(format::short_faction(&data.faction(p.faction).name), STATS_NAME_CHARS),
        (p.team + 1).to_string(),
        result.into(),
        format!("{}", st.gathered.total().floor()),
        st.units_trained.to_string(),
        st.units_lost.to_string(),
        st.buildings_built.to_string(),
        st.buildings_lost.to_string(),
        st.kills.to_string(),
    ]
}

#[allow(clippy::too_many_arguments)]
fn show_game_over(
    mut commands: Commands,
    outcome: Res<GameOutcome>,
    players: Option<Res<Players>>,
    local: Res<LocalPlayer>,
    data: Res<Data>,
    mut state: ResMut<GameOverState>,
    mut texts: Query<(&GameOverText, &mut Text)>,
    table: Query<Entity, With<StatsTable>>,
) {
    if !outcome.ended || state.shown {
        return;
    }
    let Some(players) = players else { return };
    // The screen goes up in place of the leave panel ([`show_panels`]).
    *state = GameOverState { shown: true, dismissed: false, leaving: false };
    let local_team = local.0.map(|p| players.team(p));
    let winners: Vec<&str> = players.iter().filter(|p| Some(p.team) == outcome.winning_team).map(|p| p.name.as_str()).collect();
    for (kind, mut t) in &mut texts {
        let s = match kind {
            GameOverText::Title => outcome_title(local_team, outcome.winning_team),
            GameOverText::Subtitle if winners.is_empty() => format!("Match length {}", format::clock(outcome.time)),
            GameOverText::Subtitle => format!("Winners: {}  |  Match length {}", winners.join(", "), format::clock(outcome.time)),
        };
        set_text(&mut t, &s);
    }
    let Ok(table) = table.single() else { return };
    commands.entity(table).with_children(|grid| {
        for (i, h) in STATS_COLUMNS.into_iter().enumerate() {
            grid.spawn((text(h, FONT_SMALL, TEXT_DIM), theme::text_shadow(), stats_cell(i)));
        }
        // Shadowed: names in player colours stay readable on the planks.
        for p in players.iter() {
            for (i, cell) in stats_row(&data, p, outcome.winning_team).into_iter().enumerate() {
                let color = if i == 0 { theme::player_text(p.color) } else { TEXT };
                grid.spawn((text(cell, FONT_BODY, color), theme::text_shadow(), TextLayout::no_wrap(), stats_cell(i)));
            }
        }
    });
}

/// Layout of a stats table cell in column `i`: names left, numbers right.
fn stats_cell(i: usize) -> Node {
    let justify_self = if STATS_NUMERIC[i] { JustifySelf::End } else { JustifySelf::Start };
    Node { justify_self, ..default() }
}

type MenuButtons<'w, 's> =
    Query<'w, 's, (&'static Interaction, Option<&'static GameOverButton>, Option<&'static LeaveButton>, Has<MenuButton>), Changed<Interaction>>;

/// The game-over screen's and the leave panel's buttons, the menu button
/// and [`MENU_KEY`].
fn game_menu(keys: Res<ButtonInput<KeyCode>>, buttons: MenuButtons, mut state: ResMut<GameOverState>, mut next: ResMut<NextState<GameState>>) {
    let mut toggle = keys.just_pressed(MENU_KEY);
    for (_, over, leave, menu) in buttons.iter().filter(|(i, ..)| **i == Interaction::Pressed) {
        match (over, leave) {
            (Some(GameOverButton::BackToMenu), _) | (_, Some(LeaveButton::Leave)) => next.set(GameState::Menu),
            (Some(GameOverButton::KeepWatching), _) => state.dismissed = true,
            (_, Some(LeaveButton::Resume)) => state.leaving = false,
            (None, None) => toggle |= menu,
        }
    }
    if toggle {
        state.toggle_menu();
    }
}

type FullScreenPanels<'w, 's> = Query<'w, 's, (&'static mut Node, Has<GameOverScreen>), Or<(With<GameOverScreen>, With<LeavePanel>)>>;

/// Puts up whichever full-screen panel [`GameOverState`] has open.
fn show_panels(state: Res<GameOverState>, mut panels: FullScreenPanels) {
    for (mut node, game_over) in &mut panels {
        set_display(&mut node, if game_over { state.game_over_up() } else { state.leaving });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_titles() {
        assert_eq!(outcome_title(Some(1), Some(1)), "VICTORY");
        assert_eq!(outcome_title(Some(0), Some(1)), "DEFEAT");
        assert_eq!(outcome_title(Some(0), None), "DEFEAT");
        assert_eq!(outcome_title(None, Some(2)), "Team 3 wins");
        assert_eq!(outcome_title(None, None), "No winner");
    }

    /// Width the stats table needs for `rows` (headers at the small font,
    /// cells at the body font), gaps included.
    fn stats_table_width(rows: &[[String; 10]]) -> f32 {
        let glyph = FONT_BODY * theme::GLYPH_ADVANCE;
        let widths = (0..STATS_COLUMNS.len()).map(|i| {
            let header = STATS_COLUMNS[i].chars().count() as f32 * FONT_SMALL * theme::GLYPH_ADVANCE;
            rows.iter().map(|r| r[i].chars().count() as f32 * glyph).fold(header, f32::max)
        });
        widths.sum::<f32>() + STATS_COLUMN_GAP * (STATS_COLUMNS.len() - 1) as f32
    }

    #[test]
    fn stats_table_fits_its_panel_with_every_faction() {
        let data = af_core::testing::test_data();
        // Eight AI players of the longest-named factions, with big numbers.
        let mut factions: Vec<FactionId> = data.faction_ids().collect();
        factions.sort_by_key(|f| std::cmp::Reverse(data.faction(*f).name.len()));
        let rows: Vec<[String; 10]> = (0..8)
            .map(|i| {
                let f = factions[i % factions.len()];
                let name = format!("{} (AI)", data.faction(f).name);
                let mut p = PlayerState::new(PlayerId(i as u8), name, f, [1.0; 3], i as u8, Controller::Ai, &data);
                p.defeated = true;
                p.stats.gathered.set(data.resource_id("food").unwrap(), 987_654.0);
                p.stats.units_trained = 9999;
                p.stats.kills = 9999;
                stats_row(&data, &p, Some(99))
            })
            .collect();
        for row in &rows {
            assert!(row[0].chars().count() <= STATS_NAME_CHARS && row[1].chars().count() <= STATS_NAME_CHARS, "{row:?}");
            assert!(row[0].ends_with("(AI)"), "{}", row[0]);
        }
        // The smallest window the HUD is laid out for, less the panel's margin.
        let room = 1280.0 * 0.96 - 2.0 * GAME_OVER_PADDING - 2.0;
        let width = stats_table_width(&rows);
        assert!(width <= room, "the table needs {width} px of {room}");
    }

    #[test]
    fn placement_hints_warn_or_alert() {
        assert_eq!(hint_color(PlacementError::Rule(Reason::CannotAfford)), theme::TEXT_WARN);
        assert_eq!(hint_color(PlacementError::Blocked), theme::TEXT_BAD);
        assert_eq!(hint_color(PlacementError::Rule(Reason::LimitReached)), theme::TEXT_BAD);
    }

    #[test]
    fn cursor_hints_say_what_the_click_does() {
        let data = af_core::testing::test_data();
        let house = data.building_id("house").unwrap();
        let wall = data.building_id("palisade_wall").unwrap();
        let ghost = |kind, error| PlacementGhost { kind, pos: Vec2::ZERO, facing: 0.0, error };
        assert_eq!(cursor_hint(&data, None, &InputMode::Normal, &[]), None);
        let place = InputMode::Place { kind: house, facing: 0.0 };
        let (s, c) = cursor_hint(&data, None, &place, &[ghost(house, None)]).unwrap();
        assert!(s.starts_with("Click to place the House\nR: rotate") && c == TEXT, "{s}");
        let (s, c) = cursor_hint(&data, None, &place, &[ghost(house, Some(PlacementError::Blocked))]).unwrap();
        assert_eq!((s.as_str(), c), ("Cannot build there", theme::TEXT_BAD));
        let poor = [ghost(house, Some(PlacementError::Rule(Reason::CannotAfford)))];
        let (s, c) = cursor_hint(&data, None, &place, &poor).unwrap();
        assert_eq!((s.as_str(), c), ("Not enough resources", theme::TEXT_WARN));
        // With the player known, exactly what is missing.
        let wood = data.resource_id("wood").unwrap();
        let mut p = PlayerState::new(PlayerId(0), "P".into(), data.faction_id("ironhold").unwrap(), [1.0; 3], 0, Controller::Human, &data);
        p.resources = ResAmounts::ZERO;
        p.resources.set(wood, 20.0);
        let (s, c) = cursor_hint(&data, Some(&p), &place, &poor).unwrap();
        let need = format::whole(stats::building_cost(&data, &p, house).get(wood) - 20.0);
        assert!(s.starts_with("Need ") && s.contains(&format!("{need} more Wood")) && c == theme::TEXT_WARN, "{s}");
        // A wall line past the money: what the rest of the line still needs.
        let afford = |e| ghost(wall, e);
        let poor_line = [afford(None), afford(None), afford(Some(PlacementError::Rule(Reason::CannotAfford))), afford(Some(PlacementError::Blocked))];
        let (s, _) = cursor_hint(&data, Some(&p), &InputMode::PlaceWall { kind: wall, start: Some(Vec2::ZERO) }, &poor_line).unwrap();
        let need = format::whole(stats::building_cost(&data, &p, wall).get(wood) * 3.0 - 20.0);
        assert!(s.starts_with("2 of 4 segments will be built\n"), "{s}");
        if need > 0 {
            assert!(s.contains(&format!("{need} more Wood for the rest")), "{s}");
        }
        // A wall line that only partly fits still builds the rest.
        let line = [ghost(wall, None), ghost(wall, Some(PlacementError::Blocked)), ghost(wall, None)];
        let (s, c) = cursor_hint(&data, None, &InputMode::PlaceWall { kind: wall, start: Some(Vec2::ZERO) }, &line).unwrap();
        assert!(s.starts_with("2 of 3 segments will be built\nCannot build there") && c == theme::TEXT_WARN, "{s}");
        let (s, _) = cursor_hint(&data, None, &InputMode::PlaceWall { kind: wall, start: None }, &[ghost(wall, None)]).unwrap();
        assert_eq!(s.lines().next(), Some(format!("Click where the {} starts", data.building(wall).name).as_str()));
        for action in [TargetAction::Move, TargetAction::AttackMove, TargetAction::Attack, TargetAction::Patrol, TargetAction::Rally, TargetAction::Gather, TargetAction::Repair] {
            let (s, c) = cursor_hint(&data, None, &InputMode::Target(action), &[]).unwrap();
            assert!(s.contains(": click") && s.contains("Right-click or Esc: cancel") && c == TEXT, "{action:?}: {s}");
            // Ascii only: the default font has nothing else.
            assert!(s.is_ascii(), "{s}");
        }
    }

    #[test]
    fn indicators_never_take_the_pointer() {
        let mut world = World::new();
        world.commands().spawn(Node::default()).with_children(spawn);
        world.flush();
        let mut stack: Vec<Entity> = world
            .query_filtered::<Entity, Or<(With<DragBox>, With<HoverTip>, With<PlacementHint>, With<PauseBanner>)>>()
            .iter(&world)
            .collect();
        assert_eq!(stack.len(), 4);
        // Every node of these overlays, or it would set `over_ui` and eat world clicks.
        while let Some(e) = stack.pop() {
            assert_eq!(world.get::<Pickable>(e), Some(&Pickable::IGNORE), "{e:?}");
            stack.extend(world.get::<Children>(e).into_iter().flat_map(|c| c.iter()));
        }
    }
}
