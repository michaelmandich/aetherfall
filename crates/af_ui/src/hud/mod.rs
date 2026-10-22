//! The in-match HUD (`GameState::Playing`): top bar, idle workers, notices,
//! control groups, selection panel, command card with its tooltip, minimap,
//! drag box, placement hint, the unit paths toggle, and the pause and
//! game-over overlays.
//!
//! The node tree is spawned once on entering the match and updated in
//! place; panels hide with `Display::None`/`Visibility::Hidden` instead of
//! respawning. Two summaries computed first each frame feed every panel:
//! [`HudView`] (whose eyes we look through) and [`SelectionInfo`].

pub(crate) mod card_view;
pub(crate) mod groups;
pub(crate) mod minimap;
pub(crate) mod notices;
pub(crate) mod overlays;
pub(crate) mod paths;
pub(crate) mod selection;
pub(crate) mod topbar;

#[cfg(test)]
mod tests;

use af_core::prelude::*;
use bevy::prelude::*;

use crate::card::CardSelection;

/// Whose resources, notices and fog the HUD shows: the local player, or
/// the player a spectator picked.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct HudView {
    pub player: Option<PlayerId>,
    /// No local player: the switcher is shown and there is no command card.
    pub spectating: bool,
}

/// Index of the player a spectator is looking at.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub(crate) struct SpectatorChoice(pub usize);

/// The local selection, summarised when it changes.
#[derive(Resource, Debug, Default)]
pub(crate) struct SelectionInfo {
    /// Everything selected, in entity order.
    pub entities: Vec<Entity>,
    /// Bumped whenever the selection (or a selected entity's type or
    /// completion) changes.
    pub revision: u64,
    /// The local player's selected units.
    pub own_units: Vec<Entity>,
    /// The local player's selected complete buildings of the type the
    /// command card commands (`card.building`).
    pub buildings: Vec<Entity>,
    pub card: CardSelection,
    signature: Vec<(Entity, u32)>,
}

/// Ordering inside `ClientSet::Ui`: summaries first, then the panels.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HudSet {
    Prepare,
    Panels,
}

/// Root of the HUD tree.
#[derive(Component)]
pub(crate) struct HudRoot;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<HudView>()
        .init_resource::<SpectatorChoice>()
        .init_resource::<SelectionInfo>()
        .configure_sets(Update, (HudSet::Prepare, HudSet::Panels).chain().in_set(ClientSet::Ui).run_if(in_state(GameState::Playing)))
        .add_systems(OnEnter(GameState::Playing), (reset_hud_state, spawn_hud).chain())
        .add_systems(Update, (update_view, track_selection).in_set(HudSet::Prepare));
    topbar::plugin(app);
    groups::plugin(app);
    notices::plugin(app);
    selection::plugin(app);
    card_view::plugin(app);
    minimap::plugin(app);
    overlays::plugin(app);
    paths::plugin(app);
}

/// Fresh per-match HUD state (the previous match's selection, card page,
/// game-over flag and so on must not leak into the next one).
fn reset_hud_state(mut commands: Commands) {
    commands.insert_resource(HudView::default());
    commands.insert_resource(SpectatorChoice::default());
    commands.insert_resource(SelectionInfo::default());
    commands.insert_resource(card_view::CardState::default());
    commands.insert_resource(af_core::ext::ui::CardPage::default());
    commands.insert_resource(overlays::GameOverState::default());
    commands.insert_resource(selection::ShownQueue::default());
    commands.insert_resource(topbar::IdleWorkers::default());
}

fn spawn_hud(mut commands: Commands, data: Res<Data>, mut images: ResMut<Assets<Image>>) {
    let minimap = minimap::create(&mut commands, &mut images);
    commands
        .spawn((
            HudRoot,
            DespawnOnExit(GameState::Playing),
            Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, ..default() },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            topbar::spawn(root, &data);
            notices::spawn(root);
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    bottom: px(0),
                    padding: UiRect::all(px(6)),
                    column_gap: px(6),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::FlexEnd,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|bottom| {
                minimap::spawn(bottom, minimap);
                bottom
                    .spawn((
                        Node { width: px(600), min_width: px(360), flex_shrink: 1.0, flex_direction: FlexDirection::Column, row_gap: px(4), ..default() },
                        Pickable::IGNORE,
                    ))
                    .with_children(|middle| {
                        groups::spawn(middle);
                        selection::spawn(middle);
                    });
                card_view::spawn(bottom);
            });
            card_view::spawn_tooltip(root);
            overlays::spawn(root);
            paths::spawn(root);
        });
}

fn update_view(local: Res<LocalPlayer>, players: Option<Res<Players>>, choice: Res<SpectatorChoice>, mut view: ResMut<HudView>) {
    let next = match (local.0, players) {
        (Some(p), _) => HudView { player: Some(p), spectating: false },
        (None, Some(players)) if !players.list.is_empty() => {
            HudView { player: Some(players.list[choice.0 % players.list.len()].id), spectating: true }
        }
        (None, _) => HudView { player: None, spectating: true },
    };
    if *view != next {
        *view = next;
    }
}

type SelectedQuery<'w, 's> =
    Query<'w, 's, (Entity, Option<&'static Owner>, Option<&'static Unit>, Option<&'static Building>, Has<UnderConstruction>), (With<Selected>, Without<Dead>)>;

fn track_selection(selected: SelectedQuery, local: Res<LocalPlayer>, data: Res<Data>, mut info: ResMut<SelectionInfo>, mut scratch: Local<Vec<(Entity, u32)>>) {
    scratch.clear();
    for (e, _, unit, building, foundation) in &selected {
        // Type and completion are part of the signature: an upgrade or a
        // finished foundation changes what the card offers.
        let code = unit.map_or(0, |u| 1 + u.kind.0 as u32) | building.map_or(0, |b| (1 + b.kind.0 as u32) << 16) | (foundation as u32) << 31;
        scratch.push((e, code));
    }
    scratch.sort_unstable();
    if *scratch == info.signature {
        return;
    }
    let info = &mut *info;
    std::mem::swap(&mut info.signature, &mut *scratch);
    info.revision += 1;
    info.entities.clear();
    info.entities.extend(info.signature.iter().map(|(e, _)| *e));
    info.own_units.clear();
    info.buildings.clear();
    info.card = CardSelection::default();
    let Some(me) = local.0 else { return };
    let mut card_building = None;
    for &e in &info.entities {
        let Ok((_, Some(owner), unit, building, foundation)) = selected.get(e) else { continue };
        if owner.0 != me {
            continue;
        }
        if let Some(u) = unit {
            info.own_units.push(e);
            if let Some(g) = &data.unit(u.kind).gatherer {
                info.card.gatherers = true;
                info.card.builders |= g.builds;
                info.card.repairers |= g.repair > 0.0;
            }
        } else if let Some(b) = building
            && !foundation
            && card_building.is_none_or(|k| k == b.kind)
        {
            card_building = Some(b.kind);
            info.buildings.push(e);
        }
    }
    info.card.units = !info.own_units.is_empty();
    info.card.building = card_building.filter(|_| !info.card.units);
}
