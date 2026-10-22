//! The command card (bottom right, 5x3 buttons with Q..B hotkeys) and the
//! tooltip panel above it.
//!
//! The 15 slot buttons are permanent. Their actions are re-laid out only
//! when the card's context changes (selection, page, age, researched
//! techs); availability, queue counts and highlights refresh every frame.
//! In the skin a slot wears its action's family ([`skin::card_skin`]),
//! its hotkey and queued count sit on the frame's two corner plaques, and
//! a press (mouse or hotkey) plays its [`fx`] feedback; a press that turns
//! the card's page plays on the new page's slots instead.

use std::hash::{DefaultHasher, Hash, Hasher};

use af_core::ext::ui::CardPage;
use af_core::prelude::*;
use bevy::prelude::*;
use bevy::text::LineHeight;

use super::overlays::GameOverState;
use super::{HudSet, SelectionInfo};
use crate::card::{self, card_for, Card, CardAction, CardMenu, CardSelection, COLS, HOTKEY_LABELS, ROWS, SLOTS};
use crate::fx::{self, PressFx, PressKind};
use crate::skin::{self, ButtonSkin, SkinButton, SkinLook, SkinPanel, SkinPlaques, Skinned, UiSkin, Usable};
use crate::theme::{self, rgb, set_color, set_display, set_text, text, FONT_LARGE, FONT_SMALL, GLYPH_ADVANCE, TEXT, TEXT_DIM};
use crate::tooltip::{self, Tooltip};

const SLOT_W: f32 = 84.0;
const SLOT_H: f32 = 62.0;
/// Height of the card panel (slots, gaps, padding, border): the bottom
/// row's panels share it and the tooltip sits above it.
pub(super) const CARD_HEIGHT: f32 = ROWS as f32 * SLOT_H + (ROWS as f32 - 1.0) * 4.0 + 18.0;
const CARD_WIDTH: f32 = COLS as f32 * SLOT_W + (COLS as f32 - 1.0) * 4.0 + 18.0;
/// Width of a slot's label (the button less its padding and border), and
/// the font sizes a label may shrink to so its longest word fits (10 px
/// still holds the longest single words in the content, such as
/// "Architecture"; see `theme::fit_label` for anything longer).
const LABEL_WIDTH: f32 = SLOT_W - 8.0;
const LABEL_SIZES: [f32; 4] = [FONT_SMALL, 12.0, 11.0, 10.0];
/// Lines of label that fit under a slot's hotkey row.
const LABEL_LINES: usize = 3;
/// Centres of the slot frame's hotkey and queued-count plaques (logical
/// px from the slot's top-left corner: the art's 2x positions halved).
const HOTKEY_PLAQUE: Vec2 = Vec2::new(10.0, 9.75);
const COUNT_PLAQUE: Vec2 = Vec2::new(74.0, 9.75);
/// Height of a line of `FONT_SMALL` text (Bevy's default line height).
const SMALL_LINE: f32 = FONT_SMALL * 1.2;
/// Hotkeys and counts in the plaques: light and warm in the dark wells.
const PLAQUE_TEXT: Color = Color::srgb(0.95, 0.88, 0.7);

/// What the card shows and which page is open.
#[derive(Resource, Default)]
pub(crate) struct CardState {
    pub menu: CardMenu,
    pub page: usize,
    pub slots: Card,
    /// Context the slots were laid out for.
    key: Option<u64>,
    /// What the selection offers, as of when the menu was opened. Only a
    /// change here resets the menu: a selected builder dying or a unit
    /// being shift-added keeps the build page open.
    selection: CardSelection,
}

#[derive(Component)]
struct CardPanel;

/// Command card button `0..SLOTS`.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct CardSlot(pub usize);

#[derive(Component, Clone, Copy)]
struct SlotLabel(usize);

/// The queued-count text of command card button `0..SLOTS`.
#[derive(Component, Clone, Copy)]
pub(crate) struct SlotCount(pub usize);

#[derive(Component)]
struct TooltipPanel;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum TooltipText {
    Title,
    Time,
    Body,
    Shift,
    Reason,
}

/// Cost chip `i` of the tooltip (its swatch and its amount).
#[derive(Component, Clone, Copy)]
struct CostChipNode(usize);

#[derive(Component, Clone, Copy)]
struct CostChipSwatch(usize);

#[derive(Component, Clone, Copy)]
struct CostChipText(usize);

/// Presses of the card's slots this frame (mouse or hotkey), and how each
/// went: recorded by [`card_input`], played by [`play_card_presses`].
#[derive(Resource, Default)]
pub(crate) struct CardPresses {
    slots: [Option<PressKind>; SLOTS],
    /// A press turned the page (a build page, More, Back). The pressed
    /// slot is laid out again the next frame, gone or holding another
    /// action, so [`rebuild_card`] plays a [`PressKind::Turn`] on the new
    /// page's slots instead.
    turned: bool,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<CardState>()
        .init_resource::<CardPage>()
        .init_resource::<CardPresses>()
        .add_systems(Update, (rebuild_card, card_input, refresh_card, play_card_presses, update_tooltip).chain().in_set(HudSet::Panels));
}

/// A text box `chars` characters wide of `FONT_SMALL`, centred on a
/// plaque (positions are in the slot's padding box, inside its 1 px border).
fn on_plaque(plaque: Vec2, chars: usize) -> Node {
    let width = chars as f32 * FONT_SMALL * GLYPH_ADVANCE;
    Node { position_type: PositionType::Absolute, left: px(plaque.x - 1.0 - width / 2.0), top: px(plaque.y - 1.0 - SMALL_LINE / 2.0), ..default() }
}

pub(super) fn spawn(bottom: &mut ChildSpawnerCommands) {
    bottom
        .spawn((
            CardPanel,
            theme::panel_node(Node {
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::px(COLS as u16, SLOT_W),
                grid_template_rows: RepeatedGridTrack::px(ROWS as u16, SLOT_H),
                row_gap: px(4),
                column_gap: px(4),
                flex_shrink: 0.0,
                ..default()
            }),
            theme::panel_colors(),
            SkinPanel::Panel,
            Visibility::Hidden,
        ))
        .with_children(|grid| {
            for (i, hotkey) in HOTKEY_LABELS.iter().enumerate() {
                grid.spawn((
                    Button,
                    CardSlot(i),
                    // Not clipped: labels are fitted to the button, and a
                    // press scales it past its edges.
                    Node {
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        padding: UiRect::new(px(3), px(3), px(13), px(2)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(theme::BUTTON),
                    BorderColor::all(theme::PANEL_BORDER),
                    Visibility::Hidden,
                    (SkinButton(ButtonSkin::Plain), SkinPlaques, SkinLook::default(), PressFx::card_slot()),
                ))
                .with_children(|b| {
                    b.spawn((text(*hotkey, FONT_SMALL, PLAQUE_TEXT), theme::text_shadow(), on_plaque(HOTKEY_PLAQUE, 1)));
                    b.spawn((text("", FONT_SMALL, PLAQUE_TEXT), theme::text_shadow(), on_plaque(COUNT_PLAQUE, 1), SlotCount(i)));
                    b.spawn((
                        text("", FONT_SMALL, TEXT),
                        // Crisp on the lighter woods (golden oak, pale ash).
                        theme::text_shadow(),
                        TextLayout::justify(Justify::Center),
                        // Three lines fit under the hotkey row. The lines are
                        // broken by `rebuild_card` (see `theme::fit_label`).
                        LineHeight::RelativeToFont(1.0),
                        SlotLabel(i),
                    ));
                });
            }
        });
}

pub(super) fn spawn_tooltip(root: &mut ChildSpawnerCommands) {
    root.spawn((
        TooltipPanel,
        theme::panel_node(Node {
            position_type: PositionType::Absolute,
            right: px(6),
            bottom: px(CARD_HEIGHT + 14.0),
            width: px(CARD_WIDTH),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            display: Display::None,
            ..default()
        }),
        theme::panel_colors(),
        SkinPanel::Panel,
        Pickable::IGNORE,
    ))
    .with_children(|tip| {
        tip.spawn((text("", FONT_LARGE, theme::HIGHLIGHT), TooltipText::Title, Pickable::IGNORE));
        tip.spawn((Node { column_gap: px(10), align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap, ..default() }, Pickable::IGNORE)).with_children(|row| {
            for i in 0..MAX_RESOURCES {
                row.spawn((Node { column_gap: px(4), align_items: AlignItems::Center, display: Display::None, ..default() }, CostChipNode(i), Pickable::IGNORE))
                    .with_children(|chip| {
                        chip.spawn((
                            Node { width: px(12), height: px(12), border: UiRect::all(px(1)), ..default() },
                            BackgroundColor(Color::WHITE),
                            BorderColor::all(Color::BLACK),
                            CostChipSwatch(i),
                            Pickable::IGNORE,
                        ));
                        chip.spawn((text("", FONT_SMALL, TEXT), CostChipText(i), Pickable::IGNORE));
                    });
            }
            row.spawn((text("", FONT_SMALL, TEXT_DIM), TooltipText::Time, Pickable::IGNORE));
        });
        tip.spawn((text("", FONT_SMALL, TEXT), TooltipText::Body, Pickable::IGNORE));
        tip.spawn((text("", FONT_SMALL, TEXT_DIM), TooltipText::Shift, Pickable::IGNORE));
        tip.spawn((text("", FONT_SMALL, theme::TEXT_BAD), TooltipText::Reason, Pickable::IGNORE));
    });
}

/// The local player's state, if there is one.
fn local_state<'a>(local: &LocalPlayer, players: Option<&'a Players>) -> Option<&'a PlayerState> {
    local.0.zip(players).and_then(|(p, ps)| ps.try_get(p))
}

fn context_key(sel: &SelectionInfo, card: &CardState, p: &PlayerState) -> u64 {
    let mut h = DefaultHasher::new();
    (sel.revision, card.menu, card.page, p.faction, p.age).hash(&mut h);
    (p.researched.iter().filter(|r| **r).count(), p.mods.unit_upgrades.len()).hash(&mut h);
    h.finish()
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn rebuild_card(
    sel: Res<SelectionInfo>,
    local: Res<LocalPlayer>,
    players: Option<Res<Players>>,
    data: Res<Data>,
    mut card: ResMut<CardState>,
    mut page: ResMut<CardPage>,
    mut panel: Query<&mut Visibility, (With<CardPanel>, Without<CardSlot>)>,
    mut slots: Query<(&CardSlot, &mut Visibility), Without<CardPanel>>,
    mut labels: Query<(&SlotLabel, &mut Text, &mut TextFont)>,
    mut presses: ResMut<CardPresses>,
) {
    // Asked for by last frame's press, so this frame's layout is the new page.
    let turned = std::mem::take(&mut presses.turned);
    if sel.card != card.selection {
        card.selection = sel.card;
        card.menu = CardMenu::Root;
        card.page = 0;
    }
    // Esc (af_input) asked to step back out of a build page.
    if page.close {
        page.close = false;
        if card.menu != CardMenu::Root {
            card.menu = CardMenu::Root;
            card.page = 0;
        }
    }
    let open = card.menu != CardMenu::Root;
    if page.open != open {
        page.open = open;
    }
    let player = local_state(&local, players.as_deref());
    let key = player.map(|p| context_key(&sel, &card, p));
    if key == card.key {
        return;
    }
    card.key = key;
    card.slots = player.map_or([None; SLOTS], |p| card_for(&data, p, &sel.card, card.menu, card.page));
    let any = card.slots.iter().any(Option::is_some);
    for mut v in &mut panel {
        v.set_if_neq(if any { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (slot, mut v) in &mut slots {
        v.set_if_neq(if card.slots[slot.0].is_some() { Visibility::Inherited } else { Visibility::Hidden });
    }
    if turned {
        for (press, action) in presses.slots.iter_mut().zip(card.slots) {
            *press = action.map(|_| PressKind::Turn);
        }
    }
    let pages = player.map_or(1, |p| card::pages_for(&data, p, &sel.card, card.menu));
    for (label, mut t, mut font) in &mut labels {
        let s = match card.slots[label.0] {
            // Which page is open, so the player knows there are more.
            Some(CardAction::More) => card::more_label(card.page, pages),
            Some(a) => a.label(&data),
            None => String::new(),
        };
        // Broken into lines here, and a word too long for the button
        // ("Arquebusier") shrinks the font rather than being cut off.
        let (size, lines) = theme::fit_label(&s, LABEL_WIDTH, &LABEL_SIZES, LABEL_LINES);
        set_text(&mut t, &lines);
        let size = FontSize::Px(size);
        if font.font_size != size {
            font.font_size = size;
        }
    }
}

/// What one button press needs to know.
struct Press<'a> {
    data: &'a GameData,
    player: &'a PlayerState,
    sel: &'a SelectionInfo,
    shift: bool,
    now: f32,
}

#[allow(clippy::too_many_arguments)]
fn card_input(
    keys: Res<ButtonInput<KeyCode>>,
    pressed: Query<(&Interaction, &CardSlot), Changed<Interaction>>,
    mut card: ResMut<CardState>,
    sel: Res<SelectionInfo>,
    local: Res<LocalPlayer>,
    players: Option<Res<Players>>,
    data: Res<Data>,
    game_over: Res<GameOverState>,
    clock: Res<SimClock>,
    queues: Query<&ProductionQueue>,
    mut mode: ResMut<InputMode>,
    mut inbox: ResMut<CommandInbox>,
    mut notices: ResMut<Notices>,
    mut presses: ResMut<CardPresses>,
) {
    let Some(player) = local_state(&local, players.as_deref()) else { return };
    let mut hit = [false; SLOTS];
    for (interaction, slot) in &pressed {
        if *interaction == Interaction::Pressed {
            hit[slot.0] = true;
        }
    }
    let chord = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::AltLeft, KeyCode::AltRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let placing = matches!(*mode, InputMode::Place { .. } | InputMode::PlaceWall { .. });
    if !chord && !game_over.blocking() {
        for key in keys.get_just_pressed() {
            // R rotates the building being placed (af_input).
            if placing && *key == KeyCode::KeyR {
                continue;
            }
            if let Some(slot) = card::slot_for_key(*key) {
                hit[slot] = true;
            }
        }
    }
    let press = Press { data: &data, player, sel: &sel, shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]), now: clock.elapsed };
    for slot in (0..SLOTS).filter(|s| hit[*s]) {
        let Some(action) = card.slots[slot] else { continue };
        if let Err(reason) = card::action_state(&data, player, sel.card.building, action) {
            let why = tooltip::reason_text(&data, player, sel.card.building, action, reason);
            notices.push(Some(player.id), NoticeKind::Warning, why, None, press.now);
            presses.slots[slot] = Some(PressKind::Refused);
            continue;
        }
        match fx::press_kind(action) {
            PressKind::Turn => presses.turned = true,
            kind => presses.slots[slot] = Some(kind),
        }
        execute(action, &press, &mut card, &mut mode, &mut inbox, &queues);
    }
}

fn execute(action: CardAction, p: &Press, card: &mut CardState, mode: &mut InputMode, inbox: &mut CommandInbox, queues: &Query<&ProductionQueue>) {
    let me = p.player.id;
    let units = || p.sel.own_units.clone();
    // Any other button cancels a pending target click, or the next world
    // click would still issue it. Paging with More keeps it: the armed
    // Rally button stays pinned on every page.
    if action.target_action().is_none() && action != CardAction::More && matches!(*mode, InputMode::Target(_)) {
        *mode = InputMode::Normal;
    }
    match action {
        CardAction::Move | CardAction::Patrol | CardAction::AttackMove | CardAction::Gather | CardAction::Repair | CardAction::Rally => {
            // af_input completes these with the next world click.
            *mode = action.target_action().map_or(InputMode::Normal, InputMode::Target);
        }
        CardAction::Stop => inbox.push(PlayerCommand::new(me, units(), CommandKind::Stop)),
        CardAction::Hold => inbox.push(PlayerCommand::new(me, units(), CommandKind::Hold).queued(p.shift)),
        CardAction::Stance(s) => inbox.push(PlayerCommand::new(me, units(), CommandKind::SetStance(s))),
        CardAction::BuildPage(c) => {
            card.menu = CardMenu::Build(c);
            card.page = 0;
        }
        CardAction::Build(kind) => {
            *mode = if p.data.building(kind).wall {
                InputMode::PlaceWall { kind, start: None }
            } else {
                // Keep the rotation of a building already being placed.
                let facing = if let InputMode::Place { facing, .. } = *mode { facing } else { 0.0 };
                InputMode::Place { kind, facing }
            };
        }
        CardAction::Train(unit) => {
            let count = if p.shift { 5 } else { 1 };
            inbox.push(PlayerCommand::new(me, p.sel.buildings.clone(), CommandKind::Train { unit, count }));
        }
        CardAction::Research(tech) => {
            // One building researches it: the least busy of the selection.
            let at = p.sel.buildings.iter().copied().min_by_key(|e| queues.get(*e).map_or(0, |q| q.items.len()));
            if let Some(at) = at {
                inbox.push(PlayerCommand::new(me, vec![at], CommandKind::Research { tech }));
            }
        }
        CardAction::Upgrade { .. } => inbox.push(PlayerCommand::new(me, p.sel.buildings.clone(), CommandKind::UpgradeBuilding)),
        CardAction::More => card.page += 1,
        CardAction::Back => {
            card.menu = CardMenu::Root;
            card.page = 0;
        }
    }
}

/// Per-frame look of one slot.
#[derive(Clone, Copy, Default)]
struct SlotLook {
    state: Option<Result<(), Reason>>,
    active: bool,
    count: u32,
}

const SOFT_DISABLED_TEXT: Color = Color::srgb(0.86, 0.56, 0.5);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh_card(
    card: Res<CardState>,
    sel: Res<SelectionInfo>,
    local: Res<LocalPlayer>,
    players: Option<Res<Players>>,
    data: Res<Data>,
    mode: Res<InputMode>,
    queues: Query<&ProductionQueue>,
    stances: Query<&Stance>,
    mut slots: Query<(&CardSlot, &Interaction, &mut BackgroundColor, &mut BorderColor, &mut SkinButton, &mut SkinLook, Has<Skinned>)>,
    mut labels: Query<(&SlotLabel, &mut TextColor), Without<SlotCount>>,
    mut counts: Query<(&SlotCount, &mut Text, &mut Node), Without<SlotLabel>>,
) {
    let Some(player) = local_state(&local, players.as_deref()) else { return };
    let mut stance = sel.own_units.first().and_then(|e| stances.get(*e).ok()).copied();
    if sel.own_units.iter().any(|e| stances.get(*e).ok() != stance.as_ref()) {
        stance = None;
    }
    let queued = |k: QueueKind| -> u32 {
        sel.buildings.iter().filter_map(|e| queues.get(*e).ok()).map(|q| q.items.iter().filter(|i| i.kind == k).count() as u32).sum()
    };
    let mut looks = [SlotLook::default(); SLOTS];
    for (look, action) in looks.iter_mut().zip(card.slots) {
        let Some(action) = action else { continue };
        look.state = Some(card::action_state(&data, player, sel.card.building, action));
        look.active = action.is_active(&mode) || matches!(action, CardAction::Stance(s) if Some(s) == stance);
        look.count = match action {
            CardAction::Train(u) => queued(QueueKind::Unit(u)),
            CardAction::Research(t) => queued(QueueKind::Tech(t)),
            CardAction::Upgrade { to } => queued(QueueKind::Upgrade(to)),
            _ => 0,
        };
    }
    for (slot, interaction, mut bg, mut border, mut family, mut skin_look, skinned) in &mut slots {
        let look = looks[slot.0];
        let (Some(state), Some(action)) = (look.state, card.slots[slot.0]) else { continue };
        family.set_if_neq(SkinButton(skin::card_skin(action, card.menu, |b| card::build_category(&data, player.faction, b))));
        let usable = match state {
            Ok(()) => Usable::Yes,
            Err(r) if r.is_soft() => Usable::Soon,
            Err(_) => Usable::No,
        };
        // The selection's stance glows steadily; an armed button pulses.
        let accent = matches!(action, CardAction::Stance(_)).then_some(theme::HIGHLIGHT);
        skin_look.set_if_neq(SkinLook { usable, active: look.active, accent });
        if skinned {
            // The skin draws it (`skin::skin_looks`).
            continue;
        }
        let base = if state.is_ok() { theme::group_color(action.group()) } else { theme::BUTTON_DISABLED };
        bg.set_if_neq(BackgroundColor(theme::shade(base, *interaction)));
        border.set_if_neq(BorderColor::all(if look.active { theme::HIGHLIGHT } else { theme::PANEL_BORDER }));
    }
    for (label, mut color) in &mut labels {
        let c = match looks[label.0].state {
            Some(Err(r)) if r.is_soft() => SOFT_DISABLED_TEXT,
            Some(Err(_)) => TEXT_DIM,
            _ => TEXT,
        };
        set_color(&mut color, c);
    }
    for (count, mut t, mut node) in &mut counts {
        let n = looks[count.0].count;
        let s = if n > 0 { n.to_string() } else { String::new() };
        set_text(&mut t, &s);
        // Centred on its plaque however many digits it has.
        let left = on_plaque(COUNT_PLAQUE, s.len().max(1)).left;
        if node.left != left {
            node.left = left;
        }
    }
}

/// Plays this frame's presses on their slots: the squash or shake and the
/// flash, and for a full press a burst of light from the slot's centre
/// (when the skin is in, and while there is room for more particles); a
/// page turn ripples across the card a column at a time.
fn play_card_presses(
    mut commands: Commands,
    mut presses: ResMut<CardPresses>,
    time: Res<Time<Real>>,
    skin: Option<Res<UiSkin>>,
    mut slots: Query<(&CardSlot, &mut PressFx, &UiGlobalTransform, &ComputedNode)>,
    particles: Query<(), With<fx::Particle>>,
    mut bursts: Local<u32>,
) {
    if presses.slots.iter().all(Option::is_none) {
        return;
    }
    let now = time.elapsed_secs();
    let skin = skin.filter(|s| s.ready());
    let mut live = particles.iter().count();
    for (slot, mut press, at, node) in &mut slots {
        let Some(kind) = presses.slots[slot.0].take() else { continue };
        let start = if kind == PressKind::Turn { fx::turn_start(now, slot.0 % COLS) } else { now };
        press.start(kind, start);
        if let Some(skin) = skin.as_deref()
            && kind == PressKind::Full
            && live < fx::MAX_PARTICLES
        {
            let sparks = fx::SPARKS.min(fx::MAX_PARTICLES - live - 1);
            let scale = node.inverse_scale_factor();
            fx::spawn_burst(&mut commands, skin, at.translation * scale, node.size() * scale, now, *bursts, sparks);
            *bursts = bursts.wrapping_add(1);
            live += sparks + 1;
        }
    }
    presses.slots = [None; SLOTS];
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_tooltip(
    card: Res<CardState>,
    sel: Res<SelectionInfo>,
    local: Res<LocalPlayer>,
    players: Option<Res<Players>>,
    data: Res<Data>,
    slots: Query<(&CardSlot, &Interaction)>,
    mut panel: Query<&mut Node, (With<TooltipPanel>, Without<CostChipNode>)>,
    mut texts: Query<(&TooltipText, &mut Text), Without<CostChipText>>,
    mut chips: Query<(&CostChipNode, &mut Node), Without<TooltipPanel>>,
    mut swatches: Query<(&CostChipSwatch, &mut BackgroundColor)>,
    mut amounts: Query<(&CostChipText, &mut Text, &mut TextColor), Without<TooltipText>>,
    mut shown: Local<Option<Tooltip>>,
) {
    let hovered = slots.iter().find(|(s, i)| **i != Interaction::None && card.slots[s.0].is_some()).map(|(s, _)| s.0);
    let tip = hovered.zip(local_state(&local, players.as_deref())).and_then(|(slot, player)| {
        let action = card.slots[slot]?;
        let mut t = tooltip::for_action(&data, player, sel.card.building, action);
        t.title = format!("{}  [{}]", t.title, HOTKEY_LABELS[slot]);
        Some(t)
    });
    if *shown == tip {
        return;
    }
    for mut node in &mut panel {
        set_display(&mut node, tip.is_some());
    }
    if let Some(t) = &tip {
        for (kind, mut text) in &mut texts {
            let s = match kind {
                TooltipText::Title => t.title.clone(),
                TooltipText::Time => t.time_text(),
                TooltipText::Body => t.lines.iter().filter(|l| !l.is_empty()).cloned().collect::<Vec<_>>().join("\n"),
                TooltipText::Shift => t.shift.unwrap_or_default().to_string(),
                TooltipText::Reason => t.reason.clone().unwrap_or_default(),
            };
            set_text(&mut text, &s);
        }
        for (chip, mut node) in &mut chips {
            set_display(&mut node, chip.0 < t.cost.len());
        }
        for (chip, mut bg) in &mut swatches {
            if let Some(c) = t.cost.get(chip.0) {
                bg.set_if_neq(BackgroundColor(rgb(data.resource(c.resource).color)));
            }
        }
        for (chip, mut text, mut color) in &mut amounts {
            if let Some(c) = t.cost.get(chip.0) {
                set_text(&mut text, &c.amount.to_string());
                set_color(&mut color, if c.affordable { TEXT } else { theme::TEXT_BAD });
            }
        }
    }
    *shown = tip;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::BuildCategory;
    use af_core::testing::test_data;

    /// Every label any card can show, for every faction's content.
    fn every_action(data: &GameData) -> Vec<CardAction> {
        let mut actions = vec![
            CardAction::Move,
            CardAction::Stop,
            CardAction::Hold,
            CardAction::Patrol,
            CardAction::AttackMove,
            CardAction::Gather,
            CardAction::Repair,
            CardAction::Rally,
            CardAction::More,
            CardAction::Back,
        ];
        actions.extend([Stance::Aggressive, Stance::Defensive, Stance::StandGround, Stance::Passive].map(CardAction::Stance));
        actions.extend(BuildCategory::ALL.map(CardAction::BuildPage));
        actions.extend(data.unit_ids().map(CardAction::Train));
        actions.extend(data.building_ids().map(CardAction::Build));
        actions.extend(data.tech_ids().map(CardAction::Research));
        actions.extend(data.buildings.iter().filter_map(|b| b.upgrade.as_ref()).map(|u| CardAction::Upgrade { to: u.to_id }));
        actions
    }

    #[test]
    fn every_card_label_fits_its_button() {
        let data = test_data();
        let actions = every_action(&data);
        assert!(actions.len() > 200, "{} labels: is the content loaded?", actions.len());
        for action in actions {
            let label = action.label(&data);
            let (size, text) = theme::fit_label(&label, LABEL_WIDTH, &LABEL_SIZES, LABEL_LINES);
            assert!(theme::label_fits(size, &text, LABEL_WIDTH, LABEL_LINES), "{label:?} overflows its button as {text:?} at {size} px");
            assert!(size >= LABEL_SIZES[LABEL_SIZES.len() - 1], "{label:?}");
            // Only line breaks were added: no letter is lost or cut.
            let squashed = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            assert_eq!(squashed(&text), squashed(&label), "{label:?} -> {text:?}");
        }
        // The paging button with its page count, up to nine pages.
        for pages in 1..10 {
            let label = card::more_label(pages - 1, pages);
            let (size, text) = theme::fit_label(&label, LABEL_WIDTH, &LABEL_SIZES, LABEL_LINES);
            assert_eq!((size, text.as_str()), (FONT_SMALL, label.as_str()), "one line at full size");
        }
    }

    #[test]
    fn long_words_of_the_content_keep_a_readable_size() {
        let fit = |label: &str| theme::fit_label(label, LABEL_WIDTH, &LABEL_SIZES, LABEL_LINES);
        // The longest single words in the content (12 letters): shrunk, never cut.
        for word in ["Architecture", "Conscription", "Stonewrights"] {
            assert_eq!(fit(word), (10.0, word.to_string()), "{word}");
        }
        assert_eq!(fit("Ferro-Concrete"), (FONT_SMALL, "Ferro-\nConcrete".to_string()));
        assert_eq!(fit("Upgrade Reinforced Wall").0, 12.0);
    }
}
