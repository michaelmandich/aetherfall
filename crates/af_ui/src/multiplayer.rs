//! The multiplayer screens (`GameState::Menu` with
//! [`MenuScreen::Multiplayer`](crate::menu::MenuScreen)): connect to a
//! server, the casual rooms and their lobby, ranked login and the three
//! ranked slots, the leaderboard.
//!
//! Everything shown comes from `NetSession`; everything done goes through
//! `NetActions` (af_net does the talking). The screen's content is rebuilt
//! whenever what it shows changes; the text fields keep what was typed in
//! [`MpForm`], so a rebuild never loses a half-typed name.

use af_core::ext::net::*;
use af_core::prelude::*;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;

use crate::format;
use crate::menu::MenuScreen;
use crate::theme::{self, button, set_text, text, FONT_BODY, FONT_LARGE, FONT_SMALL, TEXT, TEXT_DIM};

/// A text field of the multiplayer forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldId {
    Server,
    Name,
    RoomName,
    Username,
    Password,
}

impl FieldId {
    fn allows_spaces(self) -> bool {
        matches!(self, FieldId::Name | FieldId::RoomName)
    }

    fn masked(self) -> bool {
        self == FieldId::Password
    }

    /// Tab order.
    fn next(self) -> FieldId {
        match self {
            FieldId::Server => FieldId::Name,
            FieldId::Name => FieldId::Server,
            FieldId::RoomName => FieldId::RoomName,
            FieldId::Username => FieldId::Password,
            FieldId::Password => FieldId::Username,
        }
    }
}

/// Most characters a field takes.
const FIELD_MAX: usize = 64;

/// What was typed, and which field has the keyboard. Kept across rebuilds
/// of the screen and across matches.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MpForm {
    pub server: String,
    pub name: String,
    pub room_name: String,
    pub username: String,
    pub password: String,
    pub focus: Option<FieldId>,
    /// Prefilled from the saved settings once.
    pub seeded: bool,
}

impl MpForm {
    pub fn get(&self, id: FieldId) -> &str {
        match id {
            FieldId::Server => &self.server,
            FieldId::Name => &self.name,
            FieldId::RoomName => &self.room_name,
            FieldId::Username => &self.username,
            FieldId::Password => &self.password,
        }
    }

    fn get_mut(&mut self, id: FieldId) -> &mut String {
        match id {
            FieldId::Server => &mut self.server,
            FieldId::Name => &mut self.name,
            FieldId::RoomName => &mut self.room_name,
            FieldId::Username => &mut self.username,
            FieldId::Password => &mut self.password,
        }
    }
}

/// What a key did to the focused field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEffect {
    Nothing,
    Edited,
    /// Enter: act on the form.
    Submit,
    /// Tab: the next field.
    NextField,
    /// Escape: drop the keyboard.
    Unfocus,
}

/// Apply a typed key to `value`.
pub fn apply_key(value: &mut String, key: &Key, allow_spaces: bool) -> KeyEffect {
    match key {
        Key::Character(s) => {
            let mut edited = false;
            for c in s.chars().filter(|c| !c.is_control()) {
                if value.chars().count() < FIELD_MAX {
                    value.push(c);
                    edited = true;
                }
            }
            if edited { KeyEffect::Edited } else { KeyEffect::Nothing }
        }
        Key::Space if allow_spaces => {
            if value.chars().count() < FIELD_MAX {
                value.push(' ');
                KeyEffect::Edited
            } else {
                KeyEffect::Nothing
            }
        }
        Key::Backspace => {
            if value.pop().is_some() { KeyEffect::Edited } else { KeyEffect::Nothing }
        }
        Key::Enter => KeyEffect::Submit,
        Key::Tab => KeyEffect::NextField,
        Key::Escape => KeyEffect::Unfocus,
        _ => KeyEffect::Nothing,
    }
}

/// What a field shows: its text (masked for passwords) and a caret when it
/// has the keyboard.
pub fn field_text(value: &str, masked: bool, focused: bool) -> String {
    let shown: String = if masked { value.chars().map(|_| '*').collect() } else { value.to_string() };
    if focused { format!("{shown}_") } else { shown }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MpButton {
    Back,
    Connect,
    Disconnect,
    RefreshRooms,
    CreateRoom,
    JoinRoom(u32),
    LeaveLobby,
    FactionPrev,
    FactionNext,
    TeamNext,
    ToggleReady,
    ToggleFog,
    ToggleRule,
    AiFewer,
    AiMore,
    Start,
    LogIn,
    SignUp,
    LogOut,
    JoinRanked(RankedMode),
    Leaderboard,
    Field(FieldId),
}

/// Root of the multiplayer screen (shown by the menu while its screen is up).
#[derive(Component)]
pub(crate) struct MultiplayerRoot;

/// The part rebuilt whenever the session changes.
#[derive(Component)]
struct MpContent;

/// The text node inside a field.
#[derive(Component, Clone, Copy)]
struct FieldText(FieldId);

/// The field's box (its border shows the focus).
#[derive(Component, Clone, Copy)]
struct FieldBox(FieldId);

#[derive(Component)]
struct StatusLine;

const FIELD_WIDTH: f32 = 300.0;
const COLUMN_WIDTH: f32 = 520.0;
/// Characters of a room or player name shown in lists.
const LIST_NAME_CHARS: usize = 22;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<MpForm>()
        .add_systems(OnEnter(GameState::Menu), (seed_form, spawn_root))
        .add_systems(
            Update,
            (mp_buttons, type_into_fields, rebuild_content, render_fields, render_status)
                .chain()
                .in_set(ClientSet::Ui)
                .run_if(in_state(GameState::Menu).and_then(resource_equals(MenuScreen::Multiplayer))),
        );
}

/// Prefill the form from what the client remembers.
fn seed_form(session: Res<NetSession>, mut form: ResMut<MpForm>) {
    if form.seeded {
        return;
    }
    form.seeded = true;
    form.server = session.server.clone();
    form.name = session.name.clone();
    form.username = session.saved_username.clone();
}

fn spawn_root(mut commands: Commands) {
    commands
        .spawn((
            MultiplayerRoot,
            DespawnOnExit(GameState::Menu),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::all(px(16)),
                row_gap: px(8),
                overflow: Overflow::scroll_y(),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgb(0.04, 0.05, 0.07)),
        ))
        .with_children(|root| {
            root.spawn(text("MULTIPLAYER", 44.0, theme::HIGHLIGHT));
            root.spawn((text("", FONT_BODY, TEXT_DIM), StatusLine));
            root.spawn((MpContent, Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(10), ..default() }));
        });
}

/// One line about the connection, for the top of the screen.
pub fn status_text(session: &NetSession) -> String {
    let connection = match &session.status {
        NetStatus::Offline => "Not connected".to_string(),
        NetStatus::Connecting => format!("Connecting to {}...", session.server),
        NetStatus::Online => format!("Connected to {}{}", session.server, if session.motd.is_empty() { String::new() } else { format!("  |  {}", session.motd) }),
        NetStatus::Failed(e) => format!("Connection failed: {e}"),
    };
    match &session.error {
        Some(e) => format!("{connection}\n{e}"),
        None => connection,
    }
}

fn render_status(session: Res<NetSession>, mut line: Query<(&mut Text, &mut TextColor), With<StatusLine>>) {
    for (mut t, mut c) in &mut line {
        set_text(&mut t, &status_text(&session));
        let color = if session.error.is_some() || matches!(session.status, NetStatus::Failed(_)) { theme::TEXT_BAD } else { TEXT_DIM };
        theme::set_color(&mut c, color);
    }
}

/// Everything the content depends on, as one string: a change rebuilds it.
fn content_key(session: &NetSession, data: &GameData) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}",
        session.status, session.rooms, session.lobby, session.account, session.ranked_slots, session.leaderboard, session.auth_busy, session.ranked_enabled, data.factions.len()
    )
}

#[allow(clippy::too_many_arguments)]
fn rebuild_content(
    mut commands: Commands,
    session: Res<NetSession>,
    data: Res<Data>,
    form: Res<MpForm>,
    content: Query<Entity, With<MpContent>>,
    fresh: Query<(), Added<MpContent>>,
    mut last: Local<String>,
) {
    let key = content_key(&session, &data);
    if key == *last && fresh.is_empty() {
        return;
    }
    *last = key;
    let Ok(root) = content.single() else { return };
    commands.entity(root).despawn_children();
    commands.entity(root).with_children(|c| match (&session.status, &session.lobby) {
        (NetStatus::Online, Some(lobby)) => spawn_lobby(c, &data, lobby),
        (NetStatus::Online, None) => spawn_hub(c, &session, &form),
        _ => spawn_connect(c, &session, &form),
    });
}

fn row() -> Node {
    Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8), ..default() }
}

fn column() -> Node {
    Node { flex_direction: FlexDirection::Column, row_gap: px(8), width: px(COLUMN_WIDTH), ..default() }
}

fn label(parent: &mut ChildSpawnerCommands, s: impl Into<String>) {
    parent.spawn((text(s, FONT_BODY, TEXT), Node { width: px(120), ..default() }));
}

fn field(parent: &mut ChildSpawnerCommands, id: FieldId, form: &MpForm) {
    let focused = form.focus == Some(id);
    parent
        .spawn((
            Button,
            FieldBox(id),
            MpButton::Field(id),
            Node {
                width: px(FIELD_WIDTH),
                height: px(30),
                padding: UiRect::axes(px(8), px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.1, 0.11, 0.14)),
            BorderColor::all(if focused { theme::HIGHLIGHT } else { theme::PANEL_BORDER }),
        ))
        .with_child((text(field_text(form.get(id), id.masked(), focused), FONT_BODY, TEXT), TextLayout::no_wrap(), FieldText(id)));
}

fn action(parent: &mut ChildSpawnerCommands, s: &str, b: MpButton, color: Color, width: f32) {
    parent.spawn((button(Node { width: px(width), height: px(34), ..default() }, color), b)).with_child(text(s, FONT_BODY, TEXT));
}

fn small(parent: &mut ChildSpawnerCommands, s: &str, b: MpButton) {
    parent.spawn((button(Node { height: px(26), padding: UiRect::axes(px(8), px(2)), ..default() }, theme::BUTTON), b)).with_child(text(s, FONT_SMALL, TEXT));
}

const GREEN: Color = Color::srgb(0.16, 0.36, 0.18);
const BLUE: Color = Color::srgb(0.17, 0.25, 0.37);

fn spawn_connect(c: &mut ChildSpawnerCommands, session: &NetSession, form: &MpForm) {
    c.spawn((theme::panel_node(column()), theme::panel_colors())).with_children(|p| {
        p.spawn(text("Join a server", FONT_LARGE, TEXT));
        p.spawn(row()).with_children(|r| {
            label(r, "Server");
            field(r, FieldId::Server, form);
        });
        p.spawn(row()).with_children(|r| {
            label(r, "Your name");
            field(r, FieldId::Name, form);
        });
        p.spawn(text("A server is host:port (the port is 7777 unless the host changed it).", FONT_SMALL, TEXT_DIM));
        p.spawn(row()).with_children(|r| {
            let connecting = session.status == NetStatus::Connecting;
            action(r, if connecting { "Connecting..." } else { "Connect" }, MpButton::Connect, GREEN, 160.0);
            action(r, "Back", MpButton::Back, theme::BUTTON, 120.0);
        });
    });
}

fn spawn_hub(c: &mut ChildSpawnerCommands, session: &NetSession, form: &MpForm) {
    c.spawn(Node { flex_wrap: FlexWrap::Wrap, justify_content: JustifyContent::Center, column_gap: px(20), row_gap: px(14), ..default() }).with_children(|body| {
        body.spawn((theme::panel_node(column()), theme::panel_colors())).with_children(|p| {
            p.spawn(text("Casual rooms", FONT_LARGE, TEXT));
            p.spawn(text("Up to six friends, lockstep, no ratings.", FONT_SMALL, TEXT_DIM));
            if session.rooms.is_empty() {
                p.spawn(text("No rooms yet.", FONT_BODY, TEXT_DIM));
            }
            for room in &session.rooms {
                p.spawn(row()).with_children(|r| {
                    r.spawn((text(format::elide(&room.name, LIST_NAME_CHARS), FONT_BODY, TEXT), TextLayout::no_wrap(), Node { width: px(220), overflow: Overflow::clip(), ..default() }));
                    r.spawn((text(format!("{}/{}", room.humans, room.max_humans), FONT_BODY, TEXT_DIM), Node { width: px(50), ..default() }));
                    if room.playing {
                        r.spawn(text("playing", FONT_SMALL, TEXT_DIM));
                    } else if room.humans < room.max_humans {
                        small(r, "Join", MpButton::JoinRoom(room.id));
                    } else {
                        r.spawn(text("full", FONT_SMALL, TEXT_DIM));
                    }
                });
            }
            p.spawn(row()).with_children(|r| {
                label(r, "New room");
                field(r, FieldId::RoomName, form);
            });
            p.spawn(row()).with_children(|r| {
                action(r, "Create room", MpButton::CreateRoom, GREEN, 160.0);
                action(r, "Refresh", MpButton::RefreshRooms, theme::BUTTON, 120.0);
            });
        });
        body.spawn((theme::panel_node(column()), theme::panel_colors())).with_children(|p| {
            p.spawn(text("Ranked", FONT_LARGE, TEXT));
            if !session.ranked_enabled {
                p.spawn(text("This server has no ranked play.", FONT_BODY, TEXT_DIM));
                return;
            }
            match &session.account {
                None => {
                    p.spawn(text("Log in, or sign up with a new username.", FONT_SMALL, TEXT_DIM));
                    p.spawn(row()).with_children(|r| {
                        label(r, "Username");
                        field(r, FieldId::Username, form);
                    });
                    p.spawn(row()).with_children(|r| {
                        label(r, "Password");
                        field(r, FieldId::Password, form);
                    });
                    p.spawn(row()).with_children(|r| {
                        if session.auth_busy {
                            r.spawn(text("Signing in...", FONT_BODY, TEXT_DIM));
                        } else {
                            action(r, "Log in", MpButton::LogIn, GREEN, 140.0);
                            action(r, "Sign up", MpButton::SignUp, BLUE, 140.0);
                        }
                    });
                    p.spawn(text("3-16 letters, digits or underscores; a password of 8 or more.", FONT_SMALL, TEXT_DIM));
                }
                Some(account) => {
                    p.spawn(row()).with_children(|r| {
                        r.spawn(text(account_line(account), FONT_BODY, theme::HIGHLIGHT));
                        small(r, "Log out", MpButton::LogOut);
                    });
                    p.spawn(text("One match of each kind runs at a time. Everyone starts at 1600.", FONT_SMALL, TEXT_DIM));
                    for mode in RankedMode::ALL {
                        let slot = session.ranked_slots.iter().find(|s| s.mode == mode).copied();
                        p.spawn(row()).with_children(|r| {
                            r.spawn((text(mode.label(), FONT_BODY, TEXT), Node { width: px(70), ..default() }));
                            r.spawn((text(ranked_slot_line(mode, slot), FONT_BODY, TEXT_DIM), Node { width: px(200), ..default() }));
                            if slot.is_none_or(|s| !s.playing) {
                                small(r, "Join", MpButton::JoinRanked(mode));
                            }
                        });
                    }
                    p.spawn(row()).with_children(|r| {
                        action(r, "Leaderboard", MpButton::Leaderboard, theme::BUTTON, 160.0);
                    });
                    for (i, row_) in session.leaderboard.iter().enumerate() {
                        p.spawn(row()).with_children(|r| {
                            r.spawn((text(format!("{}.", i + 1), FONT_SMALL, TEXT_DIM), Node { width: px(30), ..default() }));
                            r.spawn((text(format::elide(&row_.username, LIST_NAME_CHARS), FONT_SMALL, TEXT), Node { width: px(200), ..default() }));
                            r.spawn(text(format!("{}  ({}-{})", row_.elo, row_.wins, row_.losses), FONT_SMALL, TEXT_DIM));
                        });
                    }
                }
            }
        });
    });
    c.spawn(row()).with_children(|r| {
        action(r, "Disconnect", MpButton::Disconnect, theme::BUTTON, 140.0);
        action(r, "Back", MpButton::Back, theme::BUTTON, 120.0);
    });
}

/// "Fin  1616  (3-1)"
pub fn account_line(a: &Account) -> String {
    format!("{}  {}  ({}-{})", a.username, a.elo, a.wins, a.losses)
}

/// "waiting 1/2", "playing", "open".
pub fn ranked_slot_line(mode: RankedMode, slot: Option<RankedSlotView>) -> String {
    match slot {
        Some(s) if s.playing => "a match is being played".into(),
        Some(s) if s.waiting > 0 => format!("waiting {}/{}", s.waiting, mode.players()),
        _ => format!("open, {} players", mode.players()),
    }
}

/// The faction name for a slot, or "Random".
fn faction_name(data: &GameData, faction: &Option<String>) -> String {
    match faction.as_deref().and_then(|f| data.faction_id(f)) {
        Some(id) => format::short_faction(&data.faction(id).name).to_string(),
        None => "Random".into(),
    }
}

fn spawn_lobby(c: &mut ChildSpawnerCommands, data: &GameData, lobby: &LobbyView) {
    let ranked = matches!(lobby.kind, LobbyKind::Ranked(_));
    c.spawn((theme::panel_node(Node { width: px(760), ..column() }), theme::panel_colors())).with_children(|p| {
        p.spawn(text(lobby_title(lobby), FONT_LARGE, TEXT));
        if lobby.playing {
            p.spawn(text("A match is being played in this room; it waits here when it ends.", FONT_SMALL, theme::TEXT_WARN));
        }
        for slot in &lobby.slots {
            p.spawn(row()).with_children(|r| {
                let color = if slot.you { theme::HIGHLIGHT } else { TEXT };
                let name = format!("{}{}", format::elide(&slot.name, LIST_NAME_CHARS), if slot.host { " (host)" } else { "" });
                r.spawn((text(name, FONT_BODY, color), TextLayout::no_wrap(), Node { width: px(230), overflow: Overflow::clip(), ..default() }));
                if let Some(elo) = slot.elo {
                    r.spawn((text(elo.to_string(), FONT_BODY, TEXT_DIM), Node { width: px(50), ..default() }));
                }
                if slot.you && !lobby.playing {
                    small(r, "<", MpButton::FactionPrev);
                }
                r.spawn((text(faction_name(data, &slot.faction), FONT_BODY, TEXT), TextLayout::no_wrap(), Node { width: px(150), overflow: Overflow::clip(), ..default() }));
                if slot.you && !lobby.playing {
                    small(r, ">", MpButton::FactionNext);
                }
                if slot.you && !ranked && !lobby.playing {
                    small(r, &format!("Team {}", slot.team + 1), MpButton::TeamNext);
                } else {
                    r.spawn((text(format!("Team {}", slot.team + 1), FONT_BODY, TEXT_DIM), Node { width: px(70), ..default() }));
                }
                if !ranked && !slot.ai {
                    if slot.you && !lobby.playing {
                        small(r, if slot.ready { "Ready" } else { "Not ready" }, MpButton::ToggleReady);
                    } else {
                        r.spawn(text(if slot.ready { "ready" } else { "not ready" }, FONT_SMALL, if slot.ready { theme::TEXT_GOOD } else { TEXT_DIM }));
                    }
                }
            });
        }
        if ranked {
            let missing = lobby.max_humans as usize - lobby.slots.len();
            p.spawn(text(if missing > 0 { format!("Waiting for {missing} more...") } else { "Starting...".into() }, FONT_BODY, TEXT_DIM));
            p.spawn(text(format!("Rule: {}. Fog of war on.", lobby.options.rule.label()), FONT_SMALL, TEXT_DIM));
        } else {
            p.spawn(row()).with_children(|r| {
                r.spawn(text("Fog of war", FONT_BODY, TEXT));
                if lobby.host_is_you {
                    small(r, if lobby.options.fog { "On" } else { "Off" }, MpButton::ToggleFog);
                } else {
                    r.spawn(text(if lobby.options.fog { "On" } else { "Off" }, FONT_BODY, TEXT_DIM));
                }
                r.spawn(text("   Victory", FONT_BODY, TEXT));
                if lobby.host_is_you {
                    small(r, lobby.options.rule.label(), MpButton::ToggleRule);
                } else {
                    r.spawn(text(lobby.options.rule.label(), FONT_BODY, TEXT_DIM));
                }
                r.spawn(text("   Computer players", FONT_BODY, TEXT));
                if lobby.host_is_you {
                    small(r, "-", MpButton::AiFewer);
                }
                r.spawn(text(lobby.options.ai_players.to_string(), FONT_BODY, TEXT_DIM));
                if lobby.host_is_you {
                    small(r, "+", MpButton::AiMore);
                }
            });
            p.spawn(text("Town halls: a team is out when it has no main hall left. Last standing: when nothing of it is left.", FONT_SMALL, TEXT_DIM));
        }
        p.spawn(row()).with_children(|r| {
            if lobby.host_is_you && !lobby.playing {
                action(r, "Start", MpButton::Start, GREEN, 160.0);
            } else if !ranked && !lobby.playing {
                r.spawn(text("The host starts the match.", FONT_BODY, TEXT_DIM));
            }
            action(r, "Leave", MpButton::LeaveLobby, theme::BUTTON, 120.0);
        });
    });
}

/// "Room: friends", "Ranked 1v1".
pub fn lobby_title(lobby: &LobbyView) -> String {
    match lobby.kind {
        LobbyKind::Casual => format!("Room: {}", lobby.name),
        LobbyKind::Ranked(mode) => format!("Ranked {}", mode.label()),
    }
}

/// The faction before or after `current` in the cycle Random, then every
/// faction in content order.
pub fn cycle_faction(data: &GameData, current: &Option<String>, forward: bool) -> Option<String> {
    let ids: Vec<&str> = data.factions.iter().map(|f| f.id.as_str()).collect();
    let n = ids.len() + 1;
    let i = current.as_deref().and_then(|c| ids.iter().position(|f| *f == c)).map_or(0, |p| p + 1);
    let j = if forward { (i + 1) % n } else { (i + n - 1) % n };
    (j > 0).then(|| ids[j - 1].to_string())
}

fn mp_buttons(
    buttons: Query<(&Interaction, &MpButton), Changed<Interaction>>,
    session: Res<NetSession>,
    data: Res<Data>,
    mut form: ResMut<MpForm>,
    mut actions: ResMut<NetActions>,
    mut screen: ResMut<MenuScreen>,
) {
    for (_, b) in buttons.iter().filter(|(i, _)| **i == Interaction::Pressed) {
        press(*b, &session, &data, &mut form, &mut actions, &mut screen);
    }
}

/// What a button does.
fn press(b: MpButton, session: &NetSession, data: &GameData, form: &mut MpForm, actions: &mut NetActions, screen: &mut MenuScreen) {
    let own = session.lobby.as_ref().and_then(|l| l.slots.iter().find(|s| s.you));
    match b {
        MpButton::Back => *screen = MenuScreen::Skirmish,
        MpButton::Field(id) => form.focus = Some(id),
        MpButton::Connect => submit_connect(form, actions),
        MpButton::Disconnect => actions.push(NetAction::Disconnect),
        MpButton::RefreshRooms => {
            actions.push(NetAction::RefreshRooms);
            actions.push(NetAction::RefreshRanked);
        }
        MpButton::CreateRoom => {
            actions.push(NetAction::CreateRoom(form.room_name.clone()));
            form.focus = None;
        }
        MpButton::JoinRoom(id) => actions.push(NetAction::JoinRoom(id)),
        MpButton::LeaveLobby => actions.push(NetAction::LeaveLobby),
        MpButton::FactionPrev | MpButton::FactionNext => {
            if let Some(slot) = own {
                actions.push(NetAction::SetFaction(cycle_faction(data, &slot.faction, b == MpButton::FactionNext)));
            }
        }
        MpButton::TeamNext => {
            if let Some(slot) = own {
                actions.push(NetAction::SetTeam((slot.team + 1) % MAX_MATCH_PLAYERS as u8));
            }
        }
        MpButton::ToggleReady => {
            if let Some(slot) = own {
                actions.push(NetAction::SetReady(!slot.ready));
            }
        }
        MpButton::ToggleFog | MpButton::ToggleRule | MpButton::AiFewer | MpButton::AiMore => {
            if let Some(lobby) = &session.lobby {
                let mut o = lobby.options;
                match b {
                    MpButton::ToggleFog => o.fog = !o.fog,
                    MpButton::ToggleRule => o.rule = match o.rule {
                        VictoryRule::LastStanding => VictoryRule::TownHalls,
                        VictoryRule::TownHalls => VictoryRule::LastStanding,
                    },
                    MpButton::AiFewer => o.ai_players = o.ai_players.saturating_sub(1),
                    _ => o.ai_players = (o.ai_players + 1).min((MAX_MATCH_PLAYERS - lobby.slots.iter().filter(|s| !s.ai).count()) as u8),
                }
                actions.push(NetAction::SetOptions(o));
            }
        }
        MpButton::Start => actions.push(NetAction::Start),
        MpButton::LogIn | MpButton::SignUp => submit_login(form, actions, b == MpButton::SignUp),
        MpButton::LogOut => actions.push(NetAction::LogOut),
        MpButton::JoinRanked(mode) => actions.push(NetAction::JoinRanked(mode)),
        MpButton::Leaderboard => actions.push(NetAction::Leaderboard),
    }
}

fn submit_connect(form: &mut MpForm, actions: &mut NetActions) {
    if !form.name.trim().is_empty() {
        actions.push(NetAction::SetName(form.name.trim().to_string()));
    }
    if !form.server.trim().is_empty() {
        actions.push(NetAction::Connect(form.server.trim().to_string()));
    }
    form.focus = None;
}

fn submit_login(form: &mut MpForm, actions: &mut NetActions, sign_up: bool) {
    let (username, password) = (form.username.trim().to_string(), form.password.clone());
    actions.push(if sign_up { NetAction::SignUp { username, password } } else { NetAction::LogIn { username, password } });
    form.focus = None;
}

/// Keys go to the focused field; Enter acts on its form.
fn type_into_fields(mut keys: MessageReader<KeyboardInput>, mut form: ResMut<MpForm>, mut actions: ResMut<NetActions>) {
    let Some(id) = form.focus else {
        keys.clear();
        return;
    };
    for key in keys.read() {
        if key.state != ButtonState::Pressed {
            continue;
        }
        let value = form.get_mut(id);
        match apply_key(value, &key.logical_key, id.allows_spaces()) {
            KeyEffect::Nothing | KeyEffect::Edited => {}
            KeyEffect::Submit => match id {
                FieldId::Server | FieldId::Name => submit_connect(&mut form, &mut actions),
                FieldId::Username | FieldId::Password => submit_login(&mut form, &mut actions, false),
                FieldId::RoomName => {
                    actions.push(NetAction::CreateRoom(form.room_name.clone()));
                    form.focus = None;
                }
            },
            KeyEffect::NextField => form.focus = Some(id.next()),
            KeyEffect::Unfocus => form.focus = None,
        }
        if form.focus != Some(id) {
            break;
        }
    }
}

fn render_fields(form: Res<MpForm>, mut texts: Query<(&FieldText, &mut Text)>, mut boxes: Query<(&FieldBox, &mut BorderColor)>) {
    if !form.is_changed() {
        return;
    }
    for (f, mut t) in &mut texts {
        set_text(&mut t, &field_text(form.get(f.0), f.0.masked(), form.focus == Some(f.0)));
    }
    for (b, mut border) in &mut boxes {
        let c = if form.focus == Some(b.0) { theme::HIGHLIGHT } else { theme::PANEL_BORDER };
        *border = BorderColor::all(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ch(s: &str) -> Key {
        Key::Character(s.into())
    }

    #[test]
    fn keys_edit_submit_and_move_on() {
        let mut v = String::new();
        assert_eq!(apply_key(&mut v, &ch("a"), false), KeyEffect::Edited);
        assert_eq!(apply_key(&mut v, &ch("b"), false), KeyEffect::Edited);
        assert_eq!(apply_key(&mut v, &Key::Space, false), KeyEffect::Nothing, "no spaces in a username");
        assert_eq!(apply_key(&mut v, &Key::Space, true), KeyEffect::Edited);
        assert_eq!(v, "ab ");
        assert_eq!(apply_key(&mut v, &Key::Backspace, true), KeyEffect::Edited);
        assert_eq!(apply_key(&mut v, &Key::Enter, true), KeyEffect::Submit);
        assert_eq!(apply_key(&mut v, &Key::Tab, true), KeyEffect::NextField);
        assert_eq!(apply_key(&mut v, &Key::Escape, true), KeyEffect::Unfocus);
        assert_eq!(apply_key(&mut v, &Key::ArrowLeft, true), KeyEffect::Nothing);
        assert_eq!(v, "ab");
        let mut long = "x".repeat(FIELD_MAX);
        assert_eq!(apply_key(&mut long, &ch("y"), false), KeyEffect::Nothing, "full");
        let mut empty = String::new();
        assert_eq!(apply_key(&mut empty, &Key::Backspace, false), KeyEffect::Nothing);
    }

    #[test]
    fn fields_show_a_caret_and_hide_passwords() {
        assert_eq!(field_text("abc", false, true), "abc_");
        assert_eq!(field_text("abc", false, false), "abc");
        assert_eq!(field_text("secret", true, true), "******_");
        assert_eq!(FieldId::Username.next(), FieldId::Password);
        assert!(FieldId::Password.masked() && !FieldId::Name.masked());
        assert!(FieldId::Name.allows_spaces() && !FieldId::Username.allows_spaces());
    }

    #[test]
    fn factions_cycle_through_random() {
        let data = af_core::testing::test_data();
        let first = data.factions[0].id.clone();
        let last = data.factions.last().unwrap().id.clone();
        assert_eq!(cycle_faction(&data, &None, true), Some(first.clone()));
        assert_eq!(cycle_faction(&data, &Some(last.clone()), true), None);
        assert_eq!(cycle_faction(&data, &None, false), Some(last));
        assert_eq!(cycle_faction(&data, &Some(first), false), None);
    }

    #[test]
    fn lines_say_what_the_server_said() {
        let mut s = NetSession::default();
        assert_eq!(status_text(&s), "Not connected");
        s.status = NetStatus::Online;
        s.server = "pi:7777".into();
        s.motd = "hi".into();
        assert_eq!(status_text(&s), "Connected to pi:7777  |  hi");
        s.error = Some("that room is full".into());
        assert!(status_text(&s).ends_with("\nthat room is full"));
        s.status = NetStatus::Failed("refused".into());
        assert!(status_text(&s).starts_with("Connection failed: refused"));
        assert_eq!(account_line(&Account { username: "Fin".into(), elo: 1616, wins: 3, losses: 1 }), "Fin  1616  (3-1)");
        assert_eq!(ranked_slot_line(RankedMode::Duel, None), "open, 2 players");
        assert_eq!(ranked_slot_line(RankedMode::Teams, Some(RankedSlotView { mode: RankedMode::Teams, waiting: 3, playing: false })), "waiting 3/4");
        assert_eq!(ranked_slot_line(RankedMode::Triple, Some(RankedSlotView { mode: RankedMode::Triple, waiting: 0, playing: true })), "a match is being played");
        let lobby = LobbyView { id: 1, name: "friends".into(), kind: LobbyKind::Casual, slots: vec![], max_humans: 6, options: RoomOptions::default(), host_is_you: true, playing: false };
        assert_eq!(lobby_title(&lobby), "Room: friends");
        assert_eq!(lobby_title(&LobbyView { kind: LobbyKind::Ranked(RankedMode::Teams), ..lobby }), "Ranked 2v2");
    }

    #[test]
    fn buttons_turn_into_actions() {
        let data = af_core::testing::test_data();
        let mut form = MpForm { server: " pi ".into(), name: "Fin".into(), username: "Fin".into(), password: "password1".into(), ..default() };
        let mut actions = NetActions::default();
        let mut screen = MenuScreen::Multiplayer;
        let mut session = NetSession::default();
        press(MpButton::Connect, &session, &data, &mut form, &mut actions, &mut screen);
        assert_eq!(actions.queue, vec![NetAction::SetName("Fin".into()), NetAction::Connect("pi".into())]);
        actions.queue.clear();
        press(MpButton::SignUp, &session, &data, &mut form, &mut actions, &mut screen);
        assert_eq!(actions.queue, vec![NetAction::SignUp { username: "Fin".into(), password: "password1".into() }]);
        actions.queue.clear();
        let you = SlotView { name: "Fin".into(), faction: None, team: 7, ready: false, host: true, you: true, ai: false, elo: None };
        session.lobby = Some(LobbyView { id: 1, name: "r".into(), kind: LobbyKind::Casual, slots: vec![you], max_humans: 6, options: RoomOptions::default(), host_is_you: true, playing: false });
        press(MpButton::TeamNext, &session, &data, &mut form, &mut actions, &mut screen);
        press(MpButton::ToggleReady, &session, &data, &mut form, &mut actions, &mut screen);
        press(MpButton::AiMore, &session, &data, &mut form, &mut actions, &mut screen);
        press(MpButton::ToggleRule, &session, &data, &mut form, &mut actions, &mut screen);
        assert_eq!(
            actions.queue,
            vec![
                NetAction::SetTeam(0),
                NetAction::SetReady(true),
                NetAction::SetOptions(RoomOptions { ai_players: 1, ..RoomOptions::default() }),
                NetAction::SetOptions(RoomOptions { rule: VictoryRule::TownHalls, ..RoomOptions::default() }),
            ]
        );
        press(MpButton::Back, &session, &data, &mut form, &mut actions, &mut screen);
        assert_eq!(screen, MenuScreen::Skirmish);
        press(MpButton::Field(FieldId::Password), &session, &data, &mut form, &mut actions, &mut screen);
        assert_eq!(form.focus, Some(FieldId::Password));
    }
}
