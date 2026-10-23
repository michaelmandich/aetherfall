//! Selection panel (bottom centre). One entity: name, owner, hp and shield
//! bars, key stats, current order, cargo, construction or production
//! progress and the production queue (click a slot to cancel it). Several:
//! a grid of type counts (click narrows the selection to a type,
//! shift-click removes it).
//!
//! Under fog the panel tells no more than the world view (af_render) shows:
//! a node out of sight keeps its amount to itself (gathering in the fog
//! stays hidden), and an order's target is named only when the viewer sees
//! it, or as the viewer remembers it ([`Sight`]).

use std::fmt::Write as _;

use af_core::ext::fog::{team_sees, LastSeen, SeenBy};
use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::{HudSet, SelectionInfo};
use crate::format::{self, num};
use crate::skin::{ButtonSkin, SkinPanel};
use crate::theme::{self, set_color, set_display, set_text, text, Tip, FONT_LARGE, FONT_SMALL, TEXT, TEXT_DIM};
use crate::tooltip;

/// Type cells in the multi-selection grid.
const MAX_GROUPS: usize = 20;
/// Queue slots after the front item (which is the progress bar).
const QUEUE_SLOTS: usize = ProductionQueue::MAX - 1;
/// Seconds between refreshes of a single selection's numbers.
const REFRESH: f32 = 0.1;

#[derive(Component)]
struct SelectionPanel;

#[derive(Component)]
struct SingleView;

#[derive(Component)]
struct MultiView;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum SingleText {
    Name,
    Owner,
    Stats,
    Status,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum BarKind {
    Hp,
    Shield,
    Progress,
}

#[derive(Component, Clone, Copy)]
struct BarRoot(BarKind);

#[derive(Component, Clone, Copy)]
struct BarFill(BarKind);

#[derive(Component, Clone, Copy)]
struct BarLabel(BarKind);

/// Production queue entry `index` of the inspected building (0 = the
/// front item, shown as the progress bar).
#[derive(Component, Clone, Copy)]
pub(crate) struct QueueSlot(pub usize);

#[derive(Component, Clone, Copy)]
struct QueueSlotLabel(usize);

#[derive(Component, Clone, Copy)]
struct GroupCell(usize);

#[derive(Component, Clone, Copy)]
struct GroupLabel(usize);

#[derive(Component)]
struct GroupOverflow;

/// What a multi-selection cell stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    Unit(UnitTypeId),
    Building(BuildingTypeId),
    Node(NodeTypeId),
}

/// The multi-selection grid's groups, in cell order.
#[derive(Resource, Default)]
struct SelectionGroups(Vec<(GroupKey, u32)>);

/// The production queue as the panel last drew it. The panel redraws at
/// most every [`REFRESH`] seconds of real time and the queue moves on in
/// between (at 8x speed, by many ticks), so a click on a slot cancels the
/// item the player saw there ([`resolve_slot`]), not whatever has moved
/// into that slot since.
#[derive(Resource, Default)]
pub(crate) struct ShownQueue {
    building: Option<Entity>,
    kinds: Vec<QueueKind>,
    /// Seconds done on the front item.
    progress: f32,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<SelectionGroups>().init_resource::<ShownQueue>().add_systems(
        Update,
        (
            (toggle_views, refresh_single, refresh_groups).chain(),
            // Against the queue as drawn last frame, which is what was clicked.
            cancel_queue_slot.before(refresh_single),
            click_group,
        )
            .in_set(HudSet::Panels),
    );
}

pub(super) fn spawn(bottom: &mut ChildSpawnerCommands) {
    bottom
        .spawn((
            SelectionPanel,
            theme::panel_node(Node {
                width: percent(100),
                min_height: px(super::card_view::CARD_HEIGHT),
                max_height: px(300),
                overflow: Overflow::clip(),
                flex_direction: FlexDirection::Column,
                ..default()
            }),
            theme::panel_colors(),
            SkinPanel::Panel,
            // Hidden rather than removed, so the control group bar above stays put.
            Visibility::Hidden,
        ))
        .with_children(|panel| {
            panel.spawn((SingleView, Node { flex_direction: FlexDirection::Column, row_gap: px(3), width: percent(100), ..default() })).with_children(|v| {
                v.spawn(Node { column_gap: px(12), align_items: AlignItems::Baseline, ..default() }).with_children(|r| {
                    r.spawn((text("", FONT_LARGE, theme::HIGHLIGHT), SingleText::Name));
                    // In the owner's colour: the shadow keeps it readable on the planks.
                    r.spawn((text("", FONT_SMALL, TEXT_DIM), theme::text_shadow(), SingleText::Owner));
                });
                spawn_bar(v, BarKind::Hp);
                spawn_bar(v, BarKind::Shield);
                v.spawn((text("", FONT_SMALL, TEXT), SingleText::Stats));
                v.spawn((text("", FONT_SMALL, theme::TEXT_WARN), SingleText::Status));
                spawn_bar(v, BarKind::Progress);
                v.spawn(Node { column_gap: px(4), ..default() }).with_children(|row| {
                    for i in 1..=QUEUE_SLOTS {
                        row.spawn((
                            theme::skinned_button(
                                Node {
                                    width: px(QUEUE_SLOT.x),
                                    height: px(QUEUE_SLOT.y),
                                    padding: UiRect::axes(px(QUEUE_PADDING), px(0)),
                                    overflow: Overflow::clip(),
                                    display: Display::None,
                                    ..default()
                                },
                                theme::group_color(crate::card::ActionGroup::Train),
                                ButtonSkin::Train,
                            ),
                            QueueSlot(i),
                            Tip::default(),
                        ))
                        .with_child((text("", QUEUE_FONT, TEXT), theme::text_shadow(), TextLayout::no_wrap(), QueueSlotLabel(i)));
                    }
                });
            });
            panel
                .spawn((MultiView, Node { flex_wrap: FlexWrap::Wrap, column_gap: px(4), row_gap: px(4), align_content: AlignContent::FlexStart, ..default() }))
                .with_children(|grid| {
                    for i in 0..MAX_GROUPS {
                        grid.spawn((
                            theme::button(Node { width: px(CELL_WIDTH), height: px(30), padding: UiRect::axes(px(CELL_PADDING), px(0)), overflow: Overflow::clip(), display: Display::None, ..default() }, theme::BUTTON),
                            GroupCell(i),
                            Tip("Click: select only these. Shift-click: deselect them.".into()),
                        ))
                        .with_child((text("", FONT_SMALL, TEXT), TextLayout::no_wrap(), GroupLabel(i)));
                    }
                    grid.spawn((text("", FONT_SMALL, TEXT_DIM), theme::text_shadow(), GroupOverflow));
                });
        });
}

fn spawn_bar(parent: &mut ChildSpawnerCommands, kind: BarKind) {
    let fill = match kind {
        BarKind::Hp => theme::HP_GOOD,
        BarKind::Shield => theme::SHIELD,
        BarKind::Progress => theme::PROGRESS,
    };
    let mut bar = parent.spawn((
        BarRoot(kind),
        Node { width: percent(100), height: px(18), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(px(3)), ..default() },
        BackgroundColor(theme::BAR_BG),
    ));
    if kind == BarKind::Progress {
        // The front of the queue: click to cancel it.
        bar.insert((Interaction::default(), QueueSlot(0), Tip::default()));
    }
    bar.with_children(|b| {
        b.spawn((
            BarFill(kind),
            Node { position_type: PositionType::Absolute, left: px(0), top: px(0), bottom: px(0), width: percent(0), border_radius: BorderRadius::all(px(3)), ..default() },
            BackgroundColor(fill),
        ));
        b.spawn((text("", FONT_SMALL, TEXT), theme::text_shadow(), BarLabel(kind)));
    });
}

fn toggle_views(
    sel: Res<SelectionInfo>,
    mut panel: Query<&mut Visibility, With<SelectionPanel>>,
    mut single: Query<&mut Node, (With<SingleView>, Without<MultiView>)>,
    mut multi: Query<&mut Node, (With<MultiView>, Without<SingleView>)>,
) {
    if !sel.is_changed() {
        return;
    }
    let n = sel.entities.len();
    for mut v in &mut panel {
        v.set_if_neq(if n > 0 { Visibility::Inherited } else { Visibility::Hidden });
    }
    for mut node in &mut single {
        set_display(&mut node, n == 1);
    }
    for mut node in &mut multi {
        set_display(&mut node, n > 1);
    }
}

type InspectQuery<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static Unit>,
        Option<&'static Building>,
        Option<&'static ResourceNode>,
        Option<&'static Owner>,
        Option<&'static Health>,
        Option<&'static Shield>,
        Option<&'static Stats>,
        Option<&'static Weapons>,
        Option<&'static Orders>,
        Option<&'static Stance>,
        Option<&'static Gatherer>,
        Option<&'static UnderConstruction>,
        Option<&'static ProductionQueue>,
        Option<&'static ResourceField>,
        Has<Idle>,
    ),
>;

type KindQuery<'w, 's> = Query<'w, 's, (Option<&'static Unit>, Option<&'static Building>, Option<&'static ResourceNode>)>;

type SightQuery<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static Unit>,
        Option<&'static Building>,
        Option<&'static ResourceNode>,
        Option<&'static Owner>,
        Option<&'static Position>,
        Option<&'static Footprint>,
        Option<&'static SeenBy>,
        Option<&'static LastSeen>,
    ),
>;

/// What the local team may know of other entities, by af_render's rules:
/// its own team's things always; other units while some of their box is in
/// sight; buildings and nodes live while in sight, a building out of sight
/// as the team last saw it. Spectators and fog-off matches see everything.
#[derive(SystemParam)]
struct Sight<'w, 's> {
    local: Res<'w, LocalPlayer>,
    players: Option<Res<'w, Players>>,
    fog: Res<'w, FogOfWar>,
    things: SightQuery<'w, 's>,
}

impl Sight<'_, '_> {
    /// The team whose fog hides things, `None` when everything is shown.
    fn team(&self) -> Option<u8> {
        let players = self.players.as_deref()?;
        self.local.0.filter(|_| self.fog.enabled).map(|p| players.team(p))
    }

    /// Is the building or node `e` in the viewer's sight right now?
    fn in_sight(&self, e: Entity) -> bool {
        self.team().is_none_or(|t| self.things.get(e).ok().and_then(|row| row.6).is_some_and(|s| s.visible_to(t)))
    }

    /// The name the viewer may put to `e` (an order's target), if any.
    fn name_of(&self, data: &GameData, e: Entity) -> Option<String> {
        let (unit, building, node, owner, pos, footprint, seen, last) = self.things.get(e).ok()?;
        let live = group_of((unit, building, node))?;
        let known = match (self.team(), self.local.0, self.players.as_deref()) {
            (Some(team), Some(me), Some(players)) if !owner.is_some_and(|o| players.allied(me, o.0)) => match live {
                GroupKey::Unit(_) => pos.zip(footprint).is_some_and(|(p, f)| team_sees(&self.fog, team, p.0, f.half.max_element())).then_some(live),
                GroupKey::Building(_) => match seen {
                    Some(s) if s.visible_to(team) => Some(live),
                    // Remembered: as it was then (an upgrade in the fog stays hidden).
                    Some(s) if s.seen_by(team) => last.and_then(|l| l.by(team)).map(|b| GroupKey::Building(b.kind)),
                    _ => None,
                },
                GroupKey::Node(_) => seen.is_some_and(|s| s.seen_by(team)).then_some(live),
            },
            _ => Some(live),
        };
        known.map(|g| group_name(data, g).to_string())
    }
}

fn group_of((unit, building, node): (Option<&Unit>, Option<&Building>, Option<&ResourceNode>)) -> Option<GroupKey> {
    unit.map(|u| GroupKey::Unit(u.kind)).or(building.map(|b| GroupKey::Building(b.kind))).or(node.map(|n| GroupKey::Node(n.kind)))
}

fn group_name(data: &GameData, g: GroupKey) -> &str {
    match g {
        GroupKey::Unit(u) => &data.unit(u).name,
        GroupKey::Building(b) => &data.building(b).name,
        GroupKey::Node(n) => &data.node(n).name,
    }
}

fn queue_item_name(data: &GameData, k: QueueKind) -> String {
    match k {
        QueueKind::Unit(u) => data.unit(u).name.clone(),
        QueueKind::Tech(t) => data.tech(t).name.clone(),
        QueueKind::Upgrade(b) => format!("Upgrade to {}", data.building(b).name),
    }
}

/// A multi-selection type cell: its width, side padding, and the width
/// left for its text.
const CELL_WIDTH: f32 = 140.0;
const CELL_PADDING: f32 = 4.0;
const CELL_TEXT_WIDTH: f32 = CELL_WIDTH - 2.0 * CELL_PADDING;

/// A type cell's text, "Name xN", at the larger of two font sizes it fits
/// in, the name cut short when it fits neither.
fn cell_label(name: &str, count: u32) -> (f32, String) {
    let suffix = format!(" x{count}");
    for size in [FONT_SMALL, 12.0] {
        let fits = (CELL_TEXT_WIDTH / (size * theme::GLYPH_ADVANCE)) as usize;
        if name.chars().count() + suffix.len() <= fits {
            return (size, format!("{name}{suffix}"));
        }
    }
    let fits = (CELL_TEXT_WIDTH / (12.0 * theme::GLYPH_ADVANCE)) as usize;
    (12.0, format!("{}{suffix}", format::elide(name, fits.saturating_sub(suffix.len()))))
}

/// A queue slot: its size, its side padding, and its label's font size
/// and length, which fit inside the slot's frame.
const QUEUE_SLOT: Vec2 = Vec2::new(60.0, 26.0);
const QUEUE_PADDING: f32 = 3.0;
const QUEUE_FONT: f32 = 12.0;
const QUEUE_CHARS: usize = 7;

/// A queue slot's text: the name cut to fit the button.
fn short_name(name: &str) -> String {
    if name.chars().count() <= QUEUE_CHARS {
        name.to_string()
    } else {
        format!("{}.", name.chars().take(QUEUE_CHARS - 1).collect::<String>().trim_end())
    }
}

/// Everything the single view shows for one entity.
#[derive(Default)]
struct Inspection {
    name: String,
    owner: String,
    owner_color: Option<Color>,
    hp: Option<(f32, f32)>,
    shield: Option<(f32, f32)>,
    stats: String,
    status: String,
    /// Fraction done and label.
    progress: Option<(f32, String)>,
    /// Names of the queue items (front first); empty unless own building.
    queue: Vec<String>,
    /// The queue items' kinds and the front's seconds done ([`ShownQueue`]).
    queue_kinds: Vec<QueueKind>,
    queue_progress: f32,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh_single(
    time: Res<Time<Real>>,
    sel: Res<SelectionInfo>,
    data: Res<Data>,
    sight: Sight,
    inspect: InspectQuery,
    mut shown: ResMut<ShownQueue>,
    mut texts: Query<(&SingleText, &mut Text, &mut TextColor), (Without<BarLabel>, Without<QueueSlotLabel>)>,
    mut bars: Query<(&BarRoot, &mut Node), (Without<BarFill>, Without<QueueSlot>)>,
    mut progress_bar: Query<(&BarRoot, &mut Node, &mut Tip), (With<QueueSlot>, Without<BarFill>)>,
    mut fills: Query<(&BarFill, &mut Node, &mut BackgroundColor), (Without<BarRoot>, Without<QueueSlot>)>,
    mut bar_labels: Query<(&BarLabel, &mut Text), (Without<SingleText>, Without<QueueSlotLabel>)>,
    mut slots: Query<(&QueueSlot, &mut Node, &mut Tip), (Without<BarRoot>, Without<BarFill>)>,
    mut slot_labels: Query<(&QueueSlotLabel, &mut Text), (Without<SingleText>, Without<BarLabel>)>,
    mut last: Local<(u64, f32)>,
) {
    let now = time.elapsed_secs();
    if sel.entities.len() != 1 || (sel.revision == last.0 && now - last.1 < REFRESH) {
        return;
    }
    *last = (sel.revision, now);
    let e = sel.entities[0];
    let Ok(row) = inspect.get(e) else { return };
    let info = inspect_entity(&data, sight.players.as_deref(), sight.local.0, row, sight.in_sight(e), |t| sight.name_of(&data, t));
    let shown = &mut *shown;
    shown.building = Some(e);
    shown.kinds.clone_from(&info.queue_kinds);
    shown.progress = info.queue_progress;

    for (kind, mut t, mut color) in &mut texts {
        match kind {
            SingleText::Name => set_text(&mut t, &info.name),
            SingleText::Owner => {
                set_text(&mut t, &info.owner);
                set_color(&mut color, info.owner_color.unwrap_or(TEXT_DIM));
            }
            SingleText::Stats => set_text(&mut t, &info.stats),
            SingleText::Status => set_text(&mut t, &info.status),
        }
    }
    let value = |kind: BarKind| match kind {
        BarKind::Hp => info.hp.map(|(v, m)| (if m > 0.0 { v / m } else { 0.0 }, format::points(v, m))),
        BarKind::Shield => info.shield.map(|(v, m)| (if m > 0.0 { v / m } else { 0.0 }, format!("Shield {}", format::points(v, m)))),
        BarKind::Progress => info.progress.clone(),
    };
    for (bar, mut node) in &mut bars {
        set_display(&mut node, value(bar.0).is_some());
    }
    for (bar, mut node, mut tip) in &mut progress_bar {
        set_display(&mut node, value(bar.0).is_some());
        let t = info.queue.first().map(|n| format!("{n}: click to cancel")).unwrap_or_default();
        tip.set_if_neq(Tip(t));
    }
    for (fill, mut node, mut bg) in &mut fills {
        let frac = value(fill.0).map_or(0.0, |(f, _)| f.clamp(0.0, 1.0));
        let w = percent(frac * 100.0);
        if node.width != w {
            node.width = w;
        }
        if fill.0 == BarKind::Hp {
            let c = if frac > 0.6 { theme::HP_GOOD } else if frac > 0.3 { theme::HP_HURT } else { theme::HP_LOW };
            bg.set_if_neq(BackgroundColor(c));
        }
    }
    for (label, mut t) in &mut bar_labels {
        let s = value(label.0).map(|(_, s)| s).unwrap_or_default();
        set_text(&mut t, &s);
    }
    for (slot, mut node, mut tip) in &mut slots {
        let item = info.queue.get(slot.0);
        set_display(&mut node, item.is_some());
        if let Some(name) = item {
            tip.set_if_neq(Tip(format!("{name}: click to cancel")));
        }
    }
    for (label, mut t) in &mut slot_labels {
        let s = info.queue.get(label.0).map(|n| short_name(n)).unwrap_or_default();
        set_text(&mut t, &s);
    }
}

type InspectRow<'a> = (
    Option<&'a Unit>,
    Option<&'a Building>,
    Option<&'a ResourceNode>,
    Option<&'a Owner>,
    Option<&'a Health>,
    Option<&'a Shield>,
    Option<&'a Stats>,
    Option<&'a Weapons>,
    Option<&'a Orders>,
    Option<&'a Stance>,
    Option<&'a Gatherer>,
    Option<&'a UnderConstruction>,
    Option<&'a ProductionQueue>,
    Option<&'a ResourceField>,
    bool,
);

/// `in_sight`: the viewer sees the entity now (a node out of sight keeps
/// its amount to itself); `name_of`: what the viewer may call an order's
/// target.
fn inspect_entity(
    data: &GameData,
    players: Option<&Players>,
    me: Option<PlayerId>,
    row: InspectRow,
    in_sight: bool,
    name_of: impl Fn(Entity) -> Option<String>,
) -> Inspection {
    let (unit, building, node, owner, health, shield, stats, weapons, orders, stance, gatherer, foundation, queue, field, idle) = row;
    let mut out = Inspection { hp: health.map(|h| (h.hp, h.max)), shield: shield.map(|s| (s.hp, s.max)), ..default() };
    if let Some(p) = owner.and_then(|o| players.and_then(|ps| ps.try_get(o.0))) {
        let faction = &data.faction(p.faction).name;
        out.owner = if p.name == *faction { format!("{} (team {})", p.name, p.team + 1) } else { format!("{} - {faction} (team {})", p.name, p.team + 1) };
        out.owner_color = Some(theme::player_text(p.color));
    }
    let mut stats_text = String::new();
    let mut status = String::new();
    if let Some(st) = stats {
        // Armour on a line of its own: a full list never wraps mid-way.
        if unit.is_some() {
            let _ = writeln!(stats_text, "Speed {}  Vision {}", num(st.speed), num(st.vision));
        } else {
            let pop = building.map_or(0, |b| data.building(b.kind).pop);
            let _ = write!(stats_text, "Vision {}", num(st.vision));
            if pop > 0 {
                let _ = write!(stats_text, "  Population +{pop}");
            }
            stats_text.push('\n');
        }
        let _ = writeln!(stats_text, "{}", format::armor(&st.armor));
        for slot in weapons.map_or(&[][..], |w| &w.slots) {
            let _ = writeln!(stats_text, "{}", tooltip::weapon_line(data, st, slot.weapon));
        }
    }
    if let Some(u) = unit {
        out.name = data.unit(u.kind).name.clone();
        match orders.and_then(Orders::current) {
            Some(o) => {
                status.push_str(format::order_label(o));
                if let Some(target) = format::order_target(o).and_then(&name_of) {
                    let _ = write!(status, " {target}");
                }
            }
            None => status.push_str(if idle { "Idle" } else { "Ready" }),
        }
        if let Some(s) = stance {
            let _ = write!(status, "  |  Stance: {}", format::stance_label(*s));
        }
        if let Some(g) = gatherer
            && let Some(r) = g.carrying.filter(|_| g.amount > 0.0)
        {
            let _ = write!(status, "  |  Carrying {} / {} {}", num(g.amount.floor()), num(g.capacity), data.resource(r).name);
        }
    } else if let Some(b) = building {
        let def = data.building(b.kind);
        out.name = def.name.clone();
        if def.pop > 0 && stats.is_none() {
            let _ = writeln!(stats_text, "Population +{}", def.pop);
        }
        if !def.drop_site_ids.is_empty() {
            let names: Vec<&str> = def.drop_site_ids.iter().map(|r| data.resource(*r).name.as_str()).collect();
            let _ = writeln!(stats_text, "Drop site: {}", names.join(", "));
        }
        if let Some(f) = field {
            let _ = write!(status, "{} left: {}  Workers {}/{}", data.resource(f.resource).name, num(f.remaining.floor()), f.users, f.slots);
        }
        if let Some(c) = foundation {
            out.name.push_str(" (foundation)");
            out.progress = Some((c.progress, format!("Construction {}%", (c.progress * 100.0).floor())));
        } else if let Some(q) = queue.filter(|_| owner.map(|o| o.0) == me) {
            out.queue = q.items.iter().map(|i| queue_item_name(data, i.kind)).collect();
            out.queue_kinds = q.items.iter().map(|i| i.kind).collect();
            out.queue_progress = q.progress;
            if let Some(front) = q.items.front() {
                let frac = if front.time > 0.0 { q.progress / front.time } else { 0.0 };
                let mut label = format!("{} {}%", out.queue[0], (frac * 100.0).floor());
                if q.pop_blocked {
                    label.push_str(" - need more houses");
                }
                out.progress = Some((frac, label));
            }
        }
    } else if let Some(n) = node {
        let def = data.node(n.kind);
        out.name = def.name.clone();
        out.owner = "Resource".into();
        let resource = &data.resource(n.resource).name;
        status = if in_sight {
            format!("{resource}: {} / {} left", num(n.remaining.floor()), num(def.amount))
        } else {
            format!("{resource}: out of sight")
        };
    }
    out.stats = stats_text.trim_end().to_string();
    out.status = status;
    out
}

fn refresh_groups(
    sel: Res<SelectionInfo>,
    data: Res<Data>,
    kinds: KindQuery,
    mut groups: ResMut<SelectionGroups>,
    mut cells: Query<(&GroupCell, &mut Node)>,
    mut labels: Query<(&GroupLabel, &mut Text, &mut TextFont), Without<GroupOverflow>>,
    mut overflow: Query<&mut Text, (With<GroupOverflow>, Without<GroupLabel>)>,
) {
    if !sel.is_changed() {
        return;
    }
    groups.0.clear();
    for g in sel.entities.iter().filter_map(|e| kinds.get(*e).ok().and_then(group_of)) {
        match groups.0.iter_mut().find(|(k, _)| *k == g) {
            Some((_, n)) => *n += 1,
            None => groups.0.push((g, 1)),
        }
    }
    groups.0.sort_unstable_by_key(|(k, _)| *k);
    for (cell, mut node) in &mut cells {
        set_display(&mut node, cell.0 < groups.0.len().min(MAX_GROUPS));
    }
    for (label, mut t, mut font) in &mut labels {
        let (size, s) = groups.0.get(label.0).map_or((FONT_SMALL, String::new()), |(g, n)| cell_label(group_name(&data, *g), *n));
        set_text(&mut t, &s);
        let size = FontSize::Px(size);
        if font.font_size != size {
            font.font_size = size;
        }
    }
    let extra = groups.0.len().saturating_sub(MAX_GROUPS);
    for mut t in &mut overflow {
        let s = if extra > 0 { format!("+{extra} more types") } else { String::new() };
        set_text(&mut t, &s);
    }
}

fn click_group(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    cells: Query<(&Interaction, &GroupCell), Changed<Interaction>>,
    groups: Res<SelectionGroups>,
    sel: Res<SelectionInfo>,
    kinds: KindQuery,
) {
    let Some(cell) = cells.iter().find(|(i, _)| **i == Interaction::Pressed).map(|(_, c)| c.0) else { return };
    let Some((target, _)) = groups.0.get(cell) else { return };
    // Shift-click drops the type; a plain click keeps only it.
    let drop_type = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for &e in &sel.entities {
        let is_type = kinds.get(e).ok().and_then(group_of) == Some(*target);
        if is_type == drop_type {
            commands.entity(e).try_remove::<Selected>();
        }
    }
}

/// Where the item drawn at `slot` of `shown` (the front then having
/// `shown_progress` seconds done) is in the queue `now` (front at
/// `progress`), or `None` once it is gone (finished or cancelled).
///
/// Items only move toward the front (the front finishes, or an earlier
/// slot is cancelled) and new ones join at the back, so the item is the
/// nearest one of its kind at or before `slot`, and before it once the
/// drawn front has left: another kind is in front now, or the same kind
/// with less done (a new one started).
fn resolve_slot(shown: &[QueueKind], shown_progress: f32, slot: usize, now: &[QueueKind], progress: f32) -> Option<usize> {
    let kind = *shown.get(slot)?;
    let same_front = now.first() == shown.first() && progress >= shown_progress;
    let last = if same_front { slot } else { slot.checked_sub(1)? };
    (0..=last.min(now.len().checked_sub(1)?)).rev().find(|&i| now[i] == kind)
}

/// The kinds in `queue`, and its front's seconds done, once the cancels
/// already waiting in `pending` for `building` have run (af_economy runs
/// the tick's commands in inbox order, each against the first own building
/// it lists).
fn after_pending(queue: &ProductionQueue, pending: &[PlayerCommand], me: PlayerId, building: Entity, owned: impl Fn(Entity) -> bool) -> (Vec<QueueKind>, f32) {
    let mut kinds: Vec<QueueKind> = queue.items.iter().map(|i| i.kind).collect();
    let mut progress = queue.progress;
    for c in pending {
        if let CommandKind::CancelQueue { index } = c.kind
            && c.player == me
            && c.entities.iter().copied().find(|e| owned(*e)) == Some(building)
            && index < kinds.len()
        {
            kinds.remove(index);
            if index == 0 {
                progress = 0.0;
            }
        }
    }
    (kinds, progress)
}

/// A click on a queue slot cancels the item drawn there. The command runs
/// at the start of the next tick, before any production, so its index is
/// resolved against the live queue (less the cancels still waiting).
fn cancel_queue_slot(
    slots: Query<(&Interaction, &QueueSlot), Changed<Interaction>>,
    sel: Res<SelectionInfo>,
    local: Res<LocalPlayer>,
    shown: Res<ShownQueue>,
    owners: Query<(&Owner, &ProductionQueue)>,
    mut inbox: ResMut<CommandInbox>,
) {
    let (Some(me), [building]) = (local.0, sel.entities.as_slice()) else { return };
    if shown.building != Some(*building) {
        return;
    }
    let Ok((owner, queue)) = owners.get(*building) else { return };
    if owner.0 != me {
        return;
    }
    let owned = |e: Entity| owners.get(e).is_ok_and(|(o, _)| o.0 == me);
    for (_, slot) in slots.iter().filter(|(i, _)| **i == Interaction::Pressed) {
        let (now, progress) = after_pending(queue, &inbox.pending, me, *building, owned);
        if let Some(index) = resolve_slot(&shown.kinds, shown.progress, slot.0, &now, progress) {
            inbox.push(PlayerCommand::new(me, vec![*building], CommandKind::CancelQueue { index }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds() -> [QueueKind; 3] {
        let data = af_core::testing::test_data();
        ["peasant", "spearman", "scout_rider"].map(|id| QueueKind::Unit(data.unit_id(id).unwrap()))
    }

    #[test]
    fn a_cancel_finds_the_item_that_was_drawn() {
        let [a, b, c] = kinds();
        let shown = [a, b, c];
        for slot in 0..3 {
            assert_eq!(resolve_slot(&shown, 5.0, slot, &shown, 6.0), Some(slot), "nothing moved");
        }
        // The front finished after the panel was drawn: the rest moved up.
        assert_eq!(resolve_slot(&shown, 5.0, 1, &[b, c], 0.0), Some(0));
        assert_eq!(resolve_slot(&shown, 5.0, 2, &[b, c], 0.0), Some(1));
        assert_eq!(resolve_slot(&shown, 5.0, 0, &[b, c], 0.0), None, "the finished front, not the next item");
        // The same kind in front again, started anew, is not the one drawn.
        assert_eq!(resolve_slot(&[a, a], 9.0, 0, &[a], 0.5), None);
        assert_eq!(resolve_slot(&[a, a], 9.0, 1, &[a], 0.5), Some(0));
        // Twins: the drawn one moved up with the others.
        assert_eq!(resolve_slot(&[a, b, a, a], 1.0, 2, &[b, a, a], 0.0), Some(1));
        // Gone, or never there.
        assert_eq!(resolve_slot(&shown, 5.0, 1, &[c], 0.0), None);
        assert_eq!(resolve_slot(&shown, 5.0, 2, &[], 0.0), None);
        assert_eq!(resolve_slot(&shown, 5.0, 3, &shown, 5.0), None);
        assert_eq!(resolve_slot(&[], 0.0, 0, &shown, 5.0), None);
    }

    #[test]
    fn cancels_still_waiting_run_first() {
        let [a, b, c] = kinds();
        let (tc, other) = (Entity::from_raw_u32(7).unwrap(), Entity::from_raw_u32(8).unwrap());
        let mut queue = ProductionQueue::default();
        queue.items.extend([a, b, c].map(|kind| QueueItem { kind, paid: ResAmounts::ZERO, time: 10.0 }));
        queue.progress = 4.0;
        let cancel = |player: u8, entities: Vec<Entity>, index| PlayerCommand::new(PlayerId(player), entities, CommandKind::CancelQueue { index });
        let owned = |e: Entity| e == tc || e == other;
        let me = PlayerId(0);
        assert_eq!(after_pending(&queue, &[], me, tc, owned), (vec![a, b, c], 4.0));
        // Ours for this building, in order; others' and other buildings' do not count.
        let pending = [cancel(0, vec![tc], 1), cancel(1, vec![tc], 0), cancel(0, vec![other, tc], 0), cancel(0, vec![tc], 9)];
        assert_eq!(after_pending(&queue, &pending, me, tc, owned), (vec![a, c], 4.0));
        assert_eq!(after_pending(&queue, &[cancel(0, vec![tc], 0)], me, tc, owned), (vec![b, c], 0.0));
        // So a second click on the slot already being cancelled does nothing.
        let (now, progress) = after_pending(&queue, &pending[..1], me, tc, owned);
        assert_eq!(resolve_slot(&[a, b, c], 4.0, 1, &now, progress), None);
        assert_eq!(resolve_slot(&[a, b, c], 4.0, 2, &now, progress), Some(1));
    }

    #[test]
    fn queue_names_are_cut_to_fit() {
        assert_eq!(short_name("Peasant"), "Peasant");
        assert_eq!(short_name("Spearman"), "Spearm.");
        assert_eq!(short_name("Raise the Banners"), "Raise.");
        // Inside the slot's padding and border, clear of its frame.
        let room = QUEUE_SLOT.x - 2.0 * (QUEUE_PADDING + 1.0);
        assert!(QUEUE_CHARS as f32 * QUEUE_FONT * theme::GLYPH_ADVANCE <= room, "{room}");
    }

    #[test]
    fn type_cells_shrink_then_cut_long_names() {
        // 132 px of text: 15 characters at 14 px, 18 at 12 px.
        assert_eq!(cell_label("Spearman", 3), (FONT_SMALL, "Spearman x3".to_string()));
        assert_eq!(cell_label("Sunspire Guard", 12), (12.0, "Sunspire Guard x12".to_string()));
        let (size, text) = cell_label("Clockwork Automaton", 2);
        assert_eq!((size, text.as_str()), (12.0, "Clockwork Au... x2"));
        assert!(text.chars().count() <= 18);
    }
}
