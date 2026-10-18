//! The command card model: which action sits in which of the 15 slots for
//! a selection, how hotkeys map to slots, how build menus split into
//! pages, and whether an action is available. Pure functions over content
//! and player state, shared by the HUD view and the unit tests.

use af_core::prelude::*;
use bevy::prelude::KeyCode;

use crate::format;

/// Card grid columns.
pub const COLS: usize = 5;
/// Card grid rows.
pub const ROWS: usize = 3;
/// Buttons on a card.
pub const SLOTS: usize = COLS * ROWS;

/// Slot hotkeys in reading order: the keyboard's left-hand 5x3 block.
pub const HOTKEYS: [KeyCode; SLOTS] = [
    KeyCode::KeyQ,
    KeyCode::KeyW,
    KeyCode::KeyE,
    KeyCode::KeyR,
    KeyCode::KeyT,
    KeyCode::KeyA,
    KeyCode::KeyS,
    KeyCode::KeyD,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyZ,
    KeyCode::KeyX,
    KeyCode::KeyC,
    KeyCode::KeyV,
    KeyCode::KeyB,
];

/// Printed hotkey of each slot.
pub const HOTKEY_LABELS: [&str; SLOTS] = ["Q", "W", "E", "R", "T", "A", "S", "D", "F", "G", "Z", "X", "C", "V", "B"];

/// The slot a key activates.
pub fn slot_for_key(key: KeyCode) -> Option<usize> {
    HOTKEYS.iter().position(|k| *k == key)
}

/// (row, column) of a slot.
pub fn slot_cell(slot: usize) -> (usize, usize) {
    (slot / COLS, slot % COLS)
}

/// The three build pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuildCategory {
    Economy,
    Military,
    Defence,
}

impl BuildCategory {
    pub const ALL: [BuildCategory; 3] = [BuildCategory::Economy, BuildCategory::Military, BuildCategory::Defence];

    /// Text of the button that opens the page.
    pub fn label(self) -> &'static str {
        match self {
            BuildCategory::Economy => "Economy",
            BuildCategory::Military => "Military",
            BuildCategory::Defence => "Defence",
        }
    }
}

/// Which page a building belongs on, from its tags and what it does:
/// walls, gates and static shooters that train nothing are defence;
/// anything military or training units (except town centres) is military;
/// the rest (halls, houses, drop sites, research) is economy.
pub fn build_category(data: &GameData, faction: FactionId, b: BuildingTypeId) -> BuildCategory {
    let def = data.building(b);
    let town_center = def.has_tag(Tag::TownCenter);
    let trains = !data.trainable_at(b, faction).is_empty();
    let shooter = def.has_tag(Tag::Tower) || !def.weapon_ids.is_empty();
    if def.wall || def.gate || def.has_tag(Tag::Wall) || (shooter && !trains && !town_center) {
        BuildCategory::Defence
    } else if def.has_tag(Tag::Military) || (trains && !town_center) {
        BuildCategory::Military
    } else {
        BuildCategory::Economy
    }
}

/// The player's build menu entries on one page, in faction order.
pub fn build_page(data: &GameData, player: &PlayerState, cat: BuildCategory) -> Vec<BuildingTypeId> {
    rules::build_menu(data, player).into_iter().filter(|b| build_category(data, player.faction, *b) == cat).collect()
}

/// Everything a command card button can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardAction {
    Move,
    Stop,
    Hold,
    Patrol,
    AttackMove,
    Gather,
    Repair,
    Stance(Stance),
    /// Open a build page.
    BuildPage(BuildCategory),
    /// Start placing a building (or a wall line).
    Build(BuildingTypeId),
    Train(UnitTypeId),
    Research(TechId),
    /// Upgrade the selected buildings into `to`.
    Upgrade { to: BuildingTypeId },
    /// Pick a rally point for the selected buildings.
    Rally,
    /// Next page of an overfull card.
    More,
    /// Back to the root card.
    Back,
}

/// Colour family of a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionGroup {
    Command,
    Stance,
    Build,
    Train,
    Research,
    Upgrade,
    Nav,
}

impl CardAction {
    /// Colour family of the button.
    pub fn group(self) -> ActionGroup {
        match self {
            CardAction::Move
            | CardAction::Stop
            | CardAction::Hold
            | CardAction::Patrol
            | CardAction::AttackMove
            | CardAction::Gather
            | CardAction::Repair
            | CardAction::Rally => ActionGroup::Command,
            CardAction::Stance(_) => ActionGroup::Stance,
            CardAction::BuildPage(_) | CardAction::Build(_) => ActionGroup::Build,
            CardAction::Train(_) => ActionGroup::Train,
            CardAction::Research(_) => ActionGroup::Research,
            CardAction::Upgrade { .. } => ActionGroup::Upgrade,
            CardAction::More | CardAction::Back => ActionGroup::Nav,
        }
    }

    /// Short button text.
    pub fn label(self, data: &GameData) -> String {
        match self {
            CardAction::Move => "Move".into(),
            CardAction::Stop => "Stop".into(),
            CardAction::Hold => "Hold".into(),
            CardAction::Patrol => "Patrol".into(),
            CardAction::AttackMove => "Attack".into(),
            CardAction::Gather => "Gather".into(),
            CardAction::Repair => "Repair".into(),
            CardAction::Stance(s) => format::stance_label(s).into(),
            CardAction::BuildPage(c) => c.label().into(),
            CardAction::Build(b) => data.building(b).name.clone(),
            CardAction::Train(u) => data.unit(u).name.clone(),
            CardAction::Research(t) => data.tech(t).name.clone(),
            CardAction::Upgrade { to } => format!("Upgrade {}", data.building(to).name),
            CardAction::Rally => "Rally".into(),
            CardAction::More => "More".into(),
            CardAction::Back => "Back".into(),
        }
    }

    /// The world click this button waits for, if it is a targeted command.
    pub fn target_action(self) -> Option<TargetAction> {
        Some(match self {
            CardAction::Move => TargetAction::Move,
            CardAction::AttackMove => TargetAction::AttackMove,
            CardAction::Patrol => TargetAction::Patrol,
            CardAction::Gather => TargetAction::Gather,
            CardAction::Repair => TargetAction::Repair,
            CardAction::Rally => TargetAction::Rally,
            _ => return None,
        })
    }

    /// Whether `mode` is what this button started (it is drawn highlighted).
    pub fn is_active(self, mode: &InputMode) -> bool {
        match (self, mode) {
            (CardAction::Build(b), InputMode::Place { kind, .. } | InputMode::PlaceWall { kind, .. }) => b == *kind,
            (_, InputMode::Target(t)) => self.target_action() == Some(*t),
            _ => false,
        }
    }
}

/// The 15 slots of a card, in reading order.
pub type Card = [Option<CardAction>; SLOTS];

/// Which page of the card is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CardMenu {
    #[default]
    Root,
    Build(BuildCategory),
}

/// What the card needs to know about the local player's selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CardSelection {
    /// Own units are selected (they take precedence over buildings).
    pub units: bool,
    pub gatherers: bool,
    pub builders: bool,
    pub repairers: bool,
    /// Type of the own complete building(s) the card commands when no
    /// units are selected.
    pub building: Option<BuildingTypeId>,
}

/// The card for a selection on the given page.
pub fn card_for(data: &GameData, player: &PlayerState, sel: &CardSelection, menu: CardMenu, page: usize) -> Card {
    if sel.units {
        match menu {
            CardMenu::Build(cat) if sel.builders => layout(&build_actions(data, player, cat), &[CardAction::Back], page),
            _ => unit_card(data, player, sel),
        }
    } else if let Some(b) = sel.building {
        let (main, pins) = building_actions(data, player, b);
        layout_pinned(&main, &pins, page)
    } else {
        [None; SLOTS]
    }
}

/// How many pages the card for a selection has on the given menu (the
/// `More` button cycles them).
pub fn pages_for(data: &GameData, player: &PlayerState, sel: &CardSelection, menu: CardMenu) -> usize {
    if sel.units {
        match menu {
            CardMenu::Build(cat) if sel.builders => page_count(build_actions(data, player, cat).len(), 1),
            _ => 1,
        }
    } else if let Some(b) = sel.building {
        let (main, pins) = building_actions(data, player, b);
        page_count_pinned(main.len(), &pins)
    } else {
        1
    }
}

/// The label of the `More` button on page `page` (wrapping) of `pages`.
pub fn more_label(page: usize, pages: usize) -> String {
    let pages = pages.max(1);
    format!("More {}/{pages}", page % pages + 1)
}

/// Fixed positions for units: orders on the top row, stances in the
/// middle, build pages and worker commands at the bottom.
pub fn unit_card(data: &GameData, player: &PlayerState, sel: &CardSelection) -> Card {
    let mut c: Card = [None; SLOTS];
    let orders = [CardAction::Move, CardAction::Stop, CardAction::Hold, CardAction::Patrol, CardAction::AttackMove];
    for (slot, a) in c.iter_mut().zip(orders) {
        *slot = Some(a);
    }
    for (i, s) in [Stance::Aggressive, Stance::Defensive, Stance::StandGround, Stance::Passive].into_iter().enumerate() {
        c[5 + i] = Some(CardAction::Stance(s));
    }
    if sel.gatherers {
        c[9] = Some(CardAction::Gather);
    }
    if sel.builders {
        for (i, cat) in BuildCategory::ALL.into_iter().enumerate() {
            if !build_page(data, player, cat).is_empty() {
                c[10 + i] = Some(CardAction::BuildPage(cat));
            }
        }
    }
    if sel.repairers {
        c[13] = Some(CardAction::Repair);
    }
    c
}

/// One build page's buttons.
pub fn build_actions(data: &GameData, player: &PlayerState, cat: BuildCategory) -> Vec<CardAction> {
    build_page(data, player, cat).into_iter().map(CardAction::Build).collect()
}

/// A button pinned to a fixed slot of a card.
pub type Pin = (usize, CardAction);

/// Fixed slots of a building card's pinned buttons, the same on every
/// building of every faction so their hotkeys never move: the next age-up
/// on C, the building's upgrade on V and the rally point on B. (What a
/// building trains and researches fills the card from Q, so its slots
/// differ between factions; these three do not.)
pub const AGE_UP_SLOT: usize = 12;
pub const UPGRADE_SLOT: usize = 13;
pub const RALLY_SLOT: usize = 14;

/// A complete building's buttons: training and research (filling the card
/// from Q), and the pinned ones: the next age-up (only the earliest one not
/// yet researched; the ones after it appear in turn), the upgrade and the
/// rally point, each on its own fixed slot.
pub fn building_actions(data: &GameData, player: &PlayerState, b: BuildingTypeId) -> (Vec<CardAction>, Vec<Pin>) {
    let trains = rules::train_menu(data, player, b);
    let mut main: Vec<CardAction> = trains.iter().map(|u| CardAction::Train(*u)).collect();
    let mut age_up: Option<TechId> = None;
    for t in rules::research_menu(data, player, b) {
        if !data.tech(t).is_age_up() {
            main.push(CardAction::Research(t));
        } else if age_up.is_none_or(|a| data.tech(t).age < data.tech(a).age) {
            age_up = Some(t);
        }
    }
    let mut pins = Vec::new();
    if let Some(t) = age_up {
        pins.push((AGE_UP_SLOT, CardAction::Research(t)));
    }
    if let Some(up) = &data.building(b).upgrade
        && data.faction(player.faction).building_ids.contains(&up.to_id)
    {
        pins.push((UPGRADE_SLOT, CardAction::Upgrade { to: up.to_id }));
    }
    if !trains.is_empty() {
        pins.push((RALLY_SLOT, CardAction::Rally));
    }
    (main, pins)
}

/// `tail` pinned to the last slots, in order (at most `SLOTS - 1` of them).
fn tail_pins(tail: &[CardAction]) -> Vec<Pin> {
    let tail = &tail[..tail.len().min(SLOTS - 1)];
    tail.iter().enumerate().map(|(i, a)| (SLOTS - tail.len() + i, *a)).collect()
}

/// The slots `pins` leave for the other buttons, in reading order.
fn free_slots(pins: &[Pin]) -> Vec<usize> {
    (0..SLOTS).filter(|s| !pins.iter().any(|(p, _)| p == s)).collect()
}

/// Number of pages `main` needs in `free` slots (one of them taken by
/// `More` once it does not fit).
fn pages_in(main: usize, free: usize) -> usize {
    if main <= free { 1 } else { main.div_ceil(free.saturating_sub(1).max(1)) }
}

/// Number of pages `main` needs next to `tail` pinned buttons.
pub fn page_count(main: usize, tail: usize) -> usize {
    pages_in(main, SLOTS - tail.min(SLOTS - 1))
}

/// Number of pages `main` needs around `pins`.
pub fn page_count_pinned(main: usize, pins: &[Pin]) -> usize {
    pages_in(main, free_slots(pins).len())
}

/// Lays `main` out in reading order and pins `tail` to the last slots
/// (see [`layout_pinned`]).
pub fn layout(main: &[CardAction], tail: &[CardAction], page: usize) -> Card {
    layout_pinned(main, &tail_pins(tail), page)
}

/// Lays `main` out in reading order in the slots `pins` leave free. When
/// `main` does not fit it is split into pages and a `More` button, in the
/// last free slot, cycles them (`page` wraps).
pub fn layout_pinned(main: &[CardAction], pins: &[Pin], page: usize) -> Card {
    let mut out: Card = [None; SLOTS];
    let free = free_slots(pins);
    let pages = pages_in(main.len(), free.len());
    let (items, slots) = if pages == 1 {
        (main, &free[..])
    } else {
        let per = free.len().saturating_sub(1).max(1);
        let p = page % pages;
        if let Some(last) = free.last() {
            out[*last] = Some(CardAction::More);
        }
        (&main[(p * per).min(main.len())..((p + 1) * per).min(main.len())], &free[..free.len().saturating_sub(1)])
    };
    for (slot, a) in slots.iter().zip(items) {
        out[*slot] = Some(*a);
    }
    for (slot, a) in pins {
        out[*slot] = Some(*a);
    }
    out
}

/// Whether the player may use `action` right now (resources included);
/// `building` is the type of the selected building for production buttons.
pub fn action_state(data: &GameData, player: &PlayerState, building: Option<BuildingTypeId>, action: CardAction) -> Result<(), Reason> {
    let at = |f: &dyn Fn(BuildingTypeId) -> Result<(), Reason>| building.map_or(Err(Reason::WrongBuilding), f);
    match action {
        CardAction::Train(u) => at(&|b| rules::can_train(data, player, b, u, true)),
        CardAction::Research(t) => at(&|b| rules::can_research(data, player, b, t, true)),
        CardAction::Upgrade { .. } => at(&|b| rules::can_upgrade(data, player, b, true).map(|_| ())),
        CardAction::Build(b) => rules::can_build(data, player, b, true),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::test_data;

    fn player(data: &GameData, faction: &str) -> PlayerState {
        let f = data.faction_id(faction).unwrap();
        PlayerState::new(PlayerId(0), "P".into(), f, [1.0; 3], 0, Controller::Human, data)
    }

    #[test]
    fn hotkeys_map_to_the_grid() {
        assert_eq!(slot_for_key(KeyCode::KeyQ), Some(0));
        assert_eq!(slot_for_key(KeyCode::KeyT), Some(4));
        assert_eq!(slot_for_key(KeyCode::KeyA), Some(5));
        assert_eq!(slot_for_key(KeyCode::KeyG), Some(9));
        assert_eq!(slot_for_key(KeyCode::KeyZ), Some(10));
        assert_eq!(slot_for_key(KeyCode::KeyB), Some(14));
        assert_eq!(slot_for_key(KeyCode::KeyH), None);
        assert_eq!(slot_for_key(KeyCode::Space), None);
        assert_eq!(slot_cell(7), (1, 2));
        for (i, k) in HOTKEYS.iter().enumerate() {
            assert_eq!(format!("Key{}", HOTKEY_LABELS[i]), format!("{k:?}"));
        }
    }

    #[test]
    fn building_categories_from_real_data() {
        let data = test_data();
        let f = data.faction_id("ironhold").unwrap();
        let cat = |id: &str| build_category(&data, f, data.building_id(id).unwrap());
        assert_eq!(cat("town_center"), BuildCategory::Economy);
        assert_eq!(cat("house"), BuildCategory::Economy);
        assert_eq!(cat("mining_camp"), BuildCategory::Economy);
        assert_eq!(cat("farm"), BuildCategory::Economy);
        assert_eq!(cat("barracks"), BuildCategory::Military);
        assert_eq!(cat("castle"), BuildCategory::Military);
        assert_eq!(cat("temple"), BuildCategory::Military);
        assert_eq!(cat("palisade_wall"), BuildCategory::Defence);
        assert_eq!(cat("palisade_gate"), BuildCategory::Defence);
        assert_eq!(cat("watchtower"), BuildCategory::Defence);
        assert_eq!(cat("anti_air_battery"), BuildCategory::Defence);
    }

    #[test]
    fn every_faction_build_menu_splits_into_pages() {
        let data = test_data();
        for f in data.faction_ids() {
            let p = PlayerState::new(PlayerId(0), "P".into(), f, [1.0; 3], 0, Controller::Human, &data);
            let menu = rules::build_menu(&data, &p);
            let pages: Vec<Vec<BuildingTypeId>> = BuildCategory::ALL.iter().map(|c| build_page(&data, &p, *c)).collect();
            assert_eq!(pages.iter().map(Vec::len).sum::<usize>(), menu.len(), "{}", data.faction(f).id);
            for page in &pages {
                assert!(!page.is_empty(), "{} has an empty build page", data.faction(f).id);
            }
            // Every entry reaches a slot on some page of its card.
            for (cat, page) in BuildCategory::ALL.iter().zip(&pages) {
                let actions = build_actions(&data, &p, *cat);
                let n = page_count(actions.len(), 1);
                let shown: usize = (0..n)
                    .map(|pg| layout(&actions, &[CardAction::Back], pg).iter().flatten().filter(|a| matches!(a, CardAction::Build(_))).count())
                    .sum();
                assert_eq!(shown, page.len());
            }
        }
    }

    #[test]
    fn unit_card_has_fixed_positions() {
        let data = test_data();
        let p = player(&data, "ironhold");
        let sel = CardSelection { units: true, gatherers: true, builders: true, repairers: true, building: None };
        let c = unit_card(&data, &p, &sel);
        assert_eq!(c[0], Some(CardAction::Move));
        assert_eq!(c[1], Some(CardAction::Stop));
        assert_eq!(c[2], Some(CardAction::Hold));
        assert_eq!(c[4], Some(CardAction::AttackMove));
        assert_eq!(c[5], Some(CardAction::Stance(Stance::Aggressive)));
        assert_eq!(c[9], Some(CardAction::Gather));
        assert_eq!(c[10], Some(CardAction::BuildPage(BuildCategory::Economy)));
        assert_eq!(c[12], Some(CardAction::BuildPage(BuildCategory::Defence)));
        assert_eq!(c[13], Some(CardAction::Repair));
        let soldiers = unit_card(&data, &p, &CardSelection { units: true, ..default_sel() });
        assert_eq!(soldiers[9], None);
        assert_eq!(soldiers[10], None);
    }

    fn default_sel() -> CardSelection {
        CardSelection::default()
    }

    #[test]
    fn build_page_needs_builders() {
        let data = test_data();
        let p = player(&data, "sylvan");
        let warriors = CardSelection { units: true, ..default_sel() };
        let builders = CardSelection { units: true, builders: true, ..default_sel() };
        let menu = CardMenu::Build(BuildCategory::Economy);
        assert_eq!(card_for(&data, &p, &warriors, menu, 0), unit_card(&data, &p, &warriors));
        let page = card_for(&data, &p, &builders, menu, 0);
        assert_eq!(page[0], Some(CardAction::Build(data.building_id("town_center").unwrap())));
        assert_eq!(page[SLOTS - 1], Some(CardAction::Back));
    }

    #[test]
    fn town_center_card_trains_researches_upgrades_and_rallies() {
        let data = test_data();
        let p = player(&data, "ironhold");
        let tc = data.building_id("town_center").unwrap();
        let card = card_for(&data, &p, &CardSelection { building: Some(tc), ..default_sel() }, CardMenu::Root, 0);
        let peasant = data.unit_id("peasant").unwrap();
        assert_eq!(card[0], Some(CardAction::Train(peasant)));
        assert_eq!(card[AGE_UP_SLOT], Some(CardAction::Research(data.tech_id("to_banner").unwrap())));
        assert!(!card.contains(&Some(CardAction::Research(data.tech_id("to_starfall").unwrap()))), "only the next age-up");
        assert_eq!(card[13], Some(CardAction::Upgrade { to: data.building_id("citadel").unwrap() }));
        assert_eq!(card[14], Some(CardAction::Rally));
        // Peasant: affordable at the start but no houses yet.
        assert_eq!(action_state(&data, &p, Some(tc), CardAction::Train(peasant)), Err(Reason::PopCapped));
        let mut housed = p.clone();
        housed.pop_cap = 15;
        assert_eq!(action_state(&data, &housed, Some(tc), CardAction::Train(peasant)), Ok(()));
        // The citadel needs the Starfall Age.
        let citadel = data.building_id("citadel").unwrap();
        assert_eq!(action_state(&data, &p, Some(tc), CardAction::Upgrade { to: citadel }), Err(Reason::Age(3)));
    }

    #[test]
    fn overfull_cards_page_with_more() {
        let actions: Vec<CardAction> = (0..20).map(|i| CardAction::Train(UnitTypeId(i))).collect();
        assert_eq!(page_count(20, 2), 2);
        let p0 = layout(&actions, &[CardAction::Rally, CardAction::Back], 0);
        assert_eq!(p0[0], Some(CardAction::Train(UnitTypeId(0))));
        assert_eq!(p0[11], Some(CardAction::Train(UnitTypeId(11))));
        assert_eq!(p0[12], Some(CardAction::More));
        assert_eq!(p0[13], Some(CardAction::Rally));
        assert_eq!(p0[14], Some(CardAction::Back));
        let p1 = layout(&actions, &[CardAction::Rally, CardAction::Back], 1);
        assert_eq!(p1[0], Some(CardAction::Train(UnitTypeId(12))));
        assert_eq!(p1[7], Some(CardAction::Train(UnitTypeId(19))));
        assert_eq!(p1[8], None);
        assert_eq!(p1[12], Some(CardAction::More));
        // Pages wrap around.
        assert_eq!(layout(&actions, &[CardAction::Rally, CardAction::Back], 2), p0);
        // Exactly full: no More.
        let full: Vec<CardAction> = (0..13).map(|i| CardAction::Train(UnitTypeId(i))).collect();
        let c = layout(&full, &[CardAction::Rally, CardAction::Back], 0);
        assert!(!c.contains(&Some(CardAction::More)));
        assert_eq!(c[12], Some(CardAction::Train(UnitTypeId(12))));
    }

    #[test]
    fn age_ups_upgrades_and_rally_keep_their_hotkeys_for_every_faction() {
        let data = test_data();
        let tc = data.building_id("town_center").unwrap();
        let (banner, starfall) = (data.tech_id("to_banner").unwrap(), data.tech_id("to_starfall").unwrap());
        for f in data.faction_ids() {
            let id = &data.faction(f).id;
            let mut p = PlayerState::new(PlayerId(0), "P".into(), f, [1.0; 3], 0, Controller::Human, &data);
            if !data.faction(f).building_ids.contains(&tc) {
                continue;
            }
            let card = |p: &PlayerState| card_for(&data, p, &CardSelection { building: Some(tc), ..default_sel() }, CardMenu::Root, 0);
            let hearth = card(&p);
            assert_eq!(hearth[0], Some(CardAction::Train(data.faction(f).worker_id)), "{id}: H then Q trains a worker");
            assert_eq!(hearth[AGE_UP_SLOT], Some(CardAction::Research(banner)), "{id}: the age-up sits on C");
            assert_eq!(HOTKEY_LABELS[AGE_UP_SLOT], "C");
            assert_eq!(hearth[RALLY_SLOT], Some(CardAction::Rally), "{id}");
            // Once Banner is researched, the next age-up takes the same slot.
            p.researched[banner.index()] = true;
            p.age = 2;
            assert_eq!(card(&p)[AGE_UP_SLOT], Some(CardAction::Research(starfall)), "{id}");
            // Nothing else is ever drawn over a pinned slot.
            for b in data.building_ids() {
                let (main, pins) = building_actions(&data, &p, b);
                let laid = layout_pinned(&main, &pins, 0);
                for (slot, a) in &pins {
                    assert_eq!(laid[*slot], Some(*a), "{id} {}", data.building(b).id);
                }
                assert!(main.iter().all(|a| !matches!(a, CardAction::Research(t) if data.tech(*t).is_age_up())), "{id}: age-ups are pinned");
            }
        }
    }

    #[test]
    fn pinned_slots_leave_gaps_for_the_rest() {
        let actions: Vec<CardAction> = (0..15).map(|i| CardAction::Train(UnitTypeId(i))).collect();
        // Rally pinned on B only: fourteen free slots before it, More in the last.
        let pins = [(RALLY_SLOT, CardAction::Rally)];
        assert_eq!(page_count_pinned(15, &pins), 2);
        let p0 = layout_pinned(&actions, &pins, 0);
        assert_eq!(p0[13], Some(CardAction::More));
        assert_eq!(p0[12], Some(CardAction::Train(UnitTypeId(12))));
        assert_eq!(p0[14], Some(CardAction::Rally));
        let p1 = layout_pinned(&actions, &pins, 1);
        assert_eq!(p1[0], Some(CardAction::Train(UnitTypeId(13))));
        // Fits: a free slot between pins is used in reading order.
        let few = layout_pinned(&actions[..3], &[(AGE_UP_SLOT, CardAction::Back), (RALLY_SLOT, CardAction::Rally)], 0);
        assert_eq!((few[2], few[12], few[13], few[14]), (Some(actions[2]), Some(CardAction::Back), None, Some(CardAction::Rally)));
    }

    #[test]
    fn more_says_which_page_is_open() {
        assert_eq!(more_label(0, 2), "More 1/2");
        assert_eq!(more_label(3, 2), "More 2/2", "pages wrap");
        assert_eq!(more_label(0, 0), "More 1/1");
        let data = test_data();
        let p = player(&data, "ironhold");
        let tc = data.building_id("town_center").unwrap();
        assert_eq!(pages_for(&data, &p, &CardSelection { building: Some(tc), ..default_sel() }, CardMenu::Root), 1);
        assert_eq!(pages_for(&data, &p, &CardSelection::default(), CardMenu::Root), 1);
        // Every page count agrees with the layout: the last page still has buttons.
        for f in data.faction_ids() {
            let p = PlayerState::new(PlayerId(0), "P".into(), f, [1.0; 3], 0, Controller::Human, &data);
            for b in data.building_ids() {
                let sel = CardSelection { building: Some(b), ..default_sel() };
                let n = pages_for(&data, &p, &sel, CardMenu::Root);
                let last = card_for(&data, &p, &sel, CardMenu::Root, n - 1);
                let (main, _) = building_actions(&data, &p, b);
                assert!(main.is_empty() || last.iter().flatten().any(|a| main.contains(a)), "{} page {n}", data.building(b).id);
                assert_eq!(n > 1, last.contains(&Some(CardAction::More)), "{}", data.building(b).id);
            }
        }
    }

    #[test]
    fn active_buttons_follow_the_input_mode() {
        let house = BuildingTypeId(3);
        assert!(CardAction::Move.is_active(&InputMode::Target(TargetAction::Move)));
        assert!(!CardAction::Patrol.is_active(&InputMode::Target(TargetAction::Move)));
        assert!(CardAction::Build(house).is_active(&InputMode::Place { kind: house, facing: 0.0 }));
        assert!(CardAction::Build(house).is_active(&InputMode::PlaceWall { kind: house, start: None }));
        assert!(!CardAction::Stop.is_active(&InputMode::Normal));
    }
}
