//! The painted UI skin (worn-ornate medieval): panels of dark wooden
//! planks in a thin riveted metal frame, and buttons in ornate metal
//! frames over stained wood whose colours say what they do.
//!
//! * Textures (`assets/ui/`, 2x: a texture pixel is half a logical
//!   pixel, so corners are drawn at `0.5` scale): `panel_wood.png`
//!   (tiled), `panel_frame.png` and `bar_frame.png` (9-sliced frames over
//!   panels and thin strips), `button_<skin>.png` per [`ButtonSkin`],
//!   `glow.png` (hover, armed and press light) and the press effects'
//!   `burst_ring.png`, `spark.png` and `shine.png` ([`crate::fx`]).
//! * Spawn functions only mark nodes: [`SkinPanel`] or [`SkinButton`]
//!   (plus [`SkinGlow`] for a panel that lights up on hover). Once every
//!   texture has loaded, [`apply_skin`] gives each marked node its image
//!   and decoration children (frame, glow, the press shine) and
//!   [`Skinned`]; its flat `BackgroundColor`/`BorderColor` go transparent.
//!   Until then, or for good when a texture is missing (tests have no PNG
//!   loader), the flat colours of [`crate::theme`] stay.
//! * States: [`skin_looks`] tints skinned buttons (normal, hover, pressed,
//!   disabled) and drives their glow from [`SkinLook`] (written by the
//!   command card and the menu), the hover and the press flash.
//!   `theme::shade_buttons` and the card's flat colours leave skinned nodes
//!   alone.
//! * One look for every age for now: [`UiSkin::load`] reads a folder of
//!   the asset root, so a per-age skin is another folder with the same
//!   file names.

use af_core::prelude::BuildingTypeId;
use bevy::asset::LoadState;
use bevy::prelude::*;

use crate::card::{ActionGroup, BuildCategory, CardAction, CardMenu};
use crate::fx::PressFx;
use crate::theme::HIGHLIGHT;

/// The skin's folder under the asset root.
pub const SKIN_FOLDER: &str = "ui";

/// Draw order of a panel's frame among the panel's children: above its
/// content, so the metal holds the planks and its shadow falls on them.
const FRAME_Z: i32 = 10;

/// The family a button belongs to: each has its own metal and wood.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonSkin {
    /// Move, Stop, Hold, Patrol, Attack, Gather, Repair, Rally: steel and blue.
    Command,
    /// The four stances: blackened iron and slate.
    Stance,
    /// The Economy build page and its buildings: brass and golden oak.
    Economy,
    /// The Military build page and its buildings: spiked iron and crimson.
    Military,
    /// The Defence build page, walls and towers: silver-steel and pale ash.
    Defence,
    /// Training units: bronze and green.
    Train,
    /// Researching techs: gold filigree and purple.
    Research,
    /// Upgrading a building or the age: ornate gold and amber.
    Upgrade,
    /// More and Back: plain iron and walnut.
    Nav,
    /// Every other button (menus, top bar, overlays): bronze and brown.
    Plain,
}

impl ButtonSkin {
    pub const ALL: [ButtonSkin; 10] = [
        ButtonSkin::Command,
        ButtonSkin::Stance,
        ButtonSkin::Economy,
        ButtonSkin::Military,
        ButtonSkin::Defence,
        ButtonSkin::Train,
        ButtonSkin::Research,
        ButtonSkin::Upgrade,
        ButtonSkin::Nav,
        ButtonSkin::Plain,
    ];

    /// Texture file of the skin.
    pub fn file(self) -> &'static str {
        match self {
            ButtonSkin::Command => "button_command.png",
            ButtonSkin::Stance => "button_stance.png",
            ButtonSkin::Economy => "button_economy.png",
            ButtonSkin::Military => "button_military.png",
            ButtonSkin::Defence => "button_defence.png",
            ButtonSkin::Train => "button_train.png",
            ButtonSkin::Research => "button_research.png",
            ButtonSkin::Upgrade => "button_upgrade.png",
            ButtonSkin::Nav => "button_nav.png",
            ButtonSkin::Plain => "button_plain.png",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

impl From<BuildCategory> for ButtonSkin {
    fn from(c: BuildCategory) -> ButtonSkin {
        match c {
            BuildCategory::Economy => ButtonSkin::Economy,
            BuildCategory::Military => ButtonSkin::Military,
            BuildCategory::Defence => ButtonSkin::Defence,
        }
    }
}

/// The skin of a command card button: a build page button and its
/// buildings wear the page's colours (a building on the root card the
/// colours of the page it belongs on, from `category`), everything else
/// its action group's.
pub fn card_skin(action: CardAction, menu: CardMenu, category: impl FnOnce(BuildingTypeId) -> BuildCategory) -> ButtonSkin {
    match (action, action.group()) {
        (CardAction::BuildPage(c), _) => c.into(),
        (CardAction::Build(b), _) => match menu {
            CardMenu::Build(c) => c.into(),
            CardMenu::Root => category(b).into(),
        },
        (_, ActionGroup::Command) => ButtonSkin::Command,
        (_, ActionGroup::Stance) => ButtonSkin::Stance,
        (_, ActionGroup::Train) => ButtonSkin::Train,
        (_, ActionGroup::Research) => ButtonSkin::Research,
        (_, ActionGroup::Upgrade) => ButtonSkin::Upgrade,
        (_, ActionGroup::Nav) => ButtonSkin::Nav,
        (_, ActionGroup::Build) => ButtonSkin::Plain,
    }
}

/// A panel to draw as planks in a metal frame.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinPanel {
    /// A box (`panel_frame.png`).
    Panel,
    /// A strip under about 80 px tall: the top bar, one-line tips
    /// (`bar_frame.png`, whose corners keep their size on a thin node).
    Bar,
}

/// A button to draw in the frame and wood of a [`ButtonSkin`]. The command
/// card changes it with the slot's action.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkinButton(pub ButtonSkin);

/// A skinned panel that is also a button (a faction card): it gets the
/// buttons' glow on hover and while chosen ([`SkinLook`]).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SkinGlow;

/// Whether a button can be used now (the command card's rules).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Usable {
    #[default]
    Yes,
    /// Not now: missing resources or population room (it may be soon).
    Soon,
    /// Not available (tech, age, limit, ...).
    No,
}

/// How a skinned button should look beyond its hover and press: set by
/// whoever knows (the command card every frame, the menu on a change).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct SkinLook {
    pub usable: Usable,
    /// Armed or chosen: the mode this button started, the selection's
    /// stance, the chosen faction.
    pub active: bool,
    /// Colour of a lasting choice (the chosen faction): a steady glow in
    /// it. Without one, an active button is armed: a pulsing gold glow.
    pub accent: Option<Color>,
}

/// A node the skin has been applied to, with its decoration children.
#[derive(Component, Clone, Copy, Debug)]
pub struct Skinned {
    /// The glow drawn over a button (hover, armed, press flash).
    pub glow: Option<Entity>,
    /// The light streak a full press sweeps across the button.
    pub shine: Option<Entity>,
}

/// Where the skin's textures are in loading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SkinState {
    #[default]
    Loading,
    /// Every texture is in `Assets<Image>`: skinned nodes are drawn with them.
    Ready,
    /// A texture could not be loaded: the UI keeps its flat colours.
    Failed,
}

/// The skin's textures.
#[derive(Resource, Clone, Debug)]
pub struct UiSkin {
    pub panel_wood: Handle<Image>,
    pub panel_frame: Handle<Image>,
    pub bar_frame: Handle<Image>,
    pub buttons: [Handle<Image>; ButtonSkin::ALL.len()],
    pub glow: Handle<Image>,
    pub burst_ring: Handle<Image>,
    pub spark: Handle<Image>,
    pub shine: Handle<Image>,
    pub state: SkinState,
}

impl UiSkin {
    /// The skin with each texture file's handle from `handle` (by file name).
    pub fn new(mut handle: impl FnMut(&str) -> Handle<Image>) -> UiSkin {
        UiSkin {
            panel_wood: handle("panel_wood.png"),
            panel_frame: handle("panel_frame.png"),
            bar_frame: handle("bar_frame.png"),
            buttons: ButtonSkin::ALL.map(|s| handle(s.file())),
            glow: handle("glow.png"),
            burst_ring: handle("burst_ring.png"),
            spark: handle("spark.png"),
            shine: handle("shine.png"),
            state: SkinState::Loading,
        }
    }

    /// The skin in `folder` of the asset root.
    pub fn load(server: &AssetServer, folder: &str) -> UiSkin {
        UiSkin::new(|file| server.load(format!("{folder}/{file}")))
    }

    pub fn button(&self, skin: ButtonSkin) -> Handle<Image> {
        self.buttons[skin.index()].clone()
    }

    pub fn ready(&self) -> bool {
        self.state == SkinState::Ready
    }

    fn handles(&self) -> impl Iterator<Item = &Handle<Image>> {
        [&self.panel_wood, &self.panel_frame, &self.bar_frame, &self.glow, &self.burst_ring, &self.spark, &self.shine].into_iter().chain(&self.buttons)
    }
}

/// Ordering inside `ClientSet::Ui`: after every system that sets a
/// button's state (the HUD panels, the menu, `theme::shade_buttons`).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SkinSet;

pub(crate) fn plugin(app: &mut App) {
    // In `build`, not at Startup: the first state's `OnEnter` (which spawns
    // the menu or, with `--quick`, the HUD) runs before Startup systems.
    if let Some(server) = app.world().get_resource::<AssetServer>() {
        let skin = UiSkin::load(server, SKIN_FOLDER);
        app.insert_resource(skin);
    }
    app.configure_sets(Update, SkinSet.in_set(af_core::prelude::ClientSet::Ui)).add_systems(
        Update,
        (
            track_skin_load,
            apply_skin.run_if(skin_ready),
            swap_button_images,
            crate::fx::press_plain_buttons,
            crate::fx::animate_presses,
            skin_looks,
            crate::fx::animate_particles,
        )
            .chain()
            .in_set(SkinSet),
    );
}

/// Whether the skin is there and loaded.
pub fn skin_ready(skin: Option<Res<UiSkin>>) -> bool {
    skin.is_some_and(|s| s.ready())
}

/// Watches the textures load: ready once all are in, failed (with one
/// warning) as soon as one cannot be loaded.
fn track_skin_load(skin: Option<ResMut<UiSkin>>, server: Res<AssetServer>, images: Res<Assets<Image>>) {
    let Some(mut skin) = skin else { return };
    if skin.state != SkinState::Loading {
        return;
    }
    let failed = skin.handles().find(|h| matches!(server.load_state(*h), LoadState::Failed(_))).map(|h| h.path().map(ToString::to_string));
    if let Some(path) = failed {
        warn!("UI skin: {} did not load; the UI keeps its flat colours", path.unwrap_or_default());
        skin.state = SkinState::Failed;
    } else if skin.handles().all(|h| images.contains(h)) {
        skin.state = SkinState::Ready;
    }
}

fn slicer(border: BorderRect, sides: SliceScaleMode) -> NodeImageMode {
    NodeImageMode::Sliced(TextureSlicer { border, center_scale_mode: SliceScaleMode::Stretch, sides_scale_mode: sides, max_corner_scale: 0.5 })
}

/// A button texture (or the glow) sliced, everything but the corners
/// stretched. The corners are cut 40 px deep, not at the frame's 32: the
/// hotkey plaques (centred at 22, 20 and up to 30 px across) reach a few
/// px past 32, and a stretched button would smear them along its edges. A
/// command card slot is drawn at exactly half the texture's size, where
/// the cut makes no difference.
fn button_mode() -> NodeImageMode {
    slicer(BorderRect::all(40.0), SliceScaleMode::Stretch)
}

/// A frame sliced with its sides tiled at the textures' 2x.
fn frame_mode(kind: SkinPanel) -> NodeImageMode {
    let border = match kind {
        SkinPanel::Panel => BorderRect::all(40.0),
        SkinPanel::Bar => BorderRect::axes(40.0, 20.0),
    };
    slicer(border, SliceScaleMode::Tile { stretch_value: 0.5 })
}

/// A child node exactly over its parent's border box (absolute children
/// are placed in the padding box, so the border is undone).
fn over_border_box(parent: &Node) -> Node {
    let out = |v: Val| match v {
        Val::Px(p) => px(-p),
        _ => px(0),
    };
    Node {
        position_type: PositionType::Absolute,
        left: out(parent.border.left),
        right: out(parent.border.right),
        top: out(parent.border.top),
        bottom: out(parent.border.bottom),
        ..default()
    }
}

/// Tint of a button: `ImageNode::color`, multiplied with the texture.
fn grey(v: f32) -> Color {
    Color::srgb(v, v, v)
}

type Unskinned<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Node,
        Option<&'static SkinPanel>,
        Option<&'static SkinButton>,
        Has<SkinGlow>,
        Option<&'static PressFx>,
        Option<&'static mut BackgroundColor>,
        Option<&'static mut BorderColor>,
    ),
    (Or<(With<SkinPanel>, With<SkinButton>)>, Without<Skinned>),
>;

/// Dresses every marked node not dressed yet: its image, a frame over a
/// panel, a glow over a button, the shine of a button with press bursts.
pub(crate) fn apply_skin(mut commands: Commands, skin: Res<UiSkin>, mut nodes: Unskinned) {
    for (e, mut node, panel, button, glows, press, bg, border) in &mut nodes {
        // The decorations cover the border box: clip only outside it.
        if node.overflow != Overflow::visible() {
            node.overflow_clip_margin = OverflowClipMargin::border_box();
        }
        if let Some(mut bg) = bg {
            bg.0 = Color::NONE;
        }
        if let Some(mut border) = border {
            *border = BorderColor::all(Color::NONE);
        }
        let place = over_border_box(&node);
        let mut entity = commands.entity(e);
        let mut parts = Skinned { glow: None, shine: None };
        if let Some(&kind) = panel {
            entity.insert(ImageNode {
                image: skin.panel_wood.clone(),
                image_mode: NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 0.5 },
                visual_box: VisualBox::BorderBox,
                ..default()
            });
            let frame = if kind == SkinPanel::Bar { &skin.bar_frame } else { &skin.panel_frame };
            entity.with_child((
                place.clone(),
                ImageNode { image: frame.clone(), image_mode: frame_mode(kind), ..default() },
                ZIndex(FRAME_Z),
                Pickable::IGNORE,
            ));
        } else if let Some(&SkinButton(kind)) = button {
            entity.insert(ImageNode { image: skin.button(kind), image_mode: button_mode(), visual_box: VisualBox::BorderBox, color: grey(NORMAL), ..default() });
        }
        if button.is_some() || glows {
            // Under the label, over the wood: hover, armed and press light.
            let glow = entity.commands().spawn((place.clone(), ImageNode { image: skin.glow.clone(), image_mode: button_mode(), color: Color::NONE, ..default() }, ZIndex(-1), Pickable::IGNORE)).id();
            entity.add_child(glow);
            parts.glow = Some(glow);
        }
        if press.is_some_and(PressFx::bursts) {
            let mut clip = place;
            clip.overflow = Overflow::clip();
            let shine = crate::fx::spawn_shine(&mut entity.commands(), &skin);
            let holder = entity.commands().spawn((clip, ZIndex(-1), Pickable::IGNORE)).add_child(shine).id();
            entity.add_child(holder);
            parts.shine = Some(shine);
        }
        entity.insert(parts);
    }
}

/// A command card slot changed family with its action: its texture follows.
#[allow(clippy::type_complexity)]
fn swap_button_images(skin: Option<Res<UiSkin>>, mut buttons: Query<(&SkinButton, &mut ImageNode), (Changed<SkinButton>, With<Skinned>)>) {
    let Some(skin) = skin else { return };
    for (b, mut image) in &mut buttons {
        let handle = skin.button(b.0);
        if image.image != handle {
            image.image = handle;
        }
    }
}

/// Tints of a skinned button, as `sRGB` greys multiplied with its texture.
const NORMAL: f32 = 0.88;
const HOVERED: f32 = 1.0;
const PRESSED: f32 = 0.78;
const DISABLED: f32 = 0.42;
/// A button unusable only for now (resources, population) stays warmer.
const SOON: Color = Color::srgb(0.54, 0.46, 0.4);
const SOON_HOVERED: Color = Color::srgb(0.6, 0.52, 0.46);
/// Glow of a hovered button.
const HOVER_GLOW: f32 = 0.35;
/// Glow of an armed button: between these, a pulse every [`PULSE`] seconds.
const ARMED_GLOW: (f32, f32) = (0.45, 0.85);
const PULSE: f32 = 1.2;
/// Glow of a lasting choice in its accent colour.
const CHOSEN_GLOW: f32 = 0.75;

/// A button's tint and glow colour (alpha = strength) at real time `now`,
/// given its look, its hover / press state and the flash of a press
/// (colour and strength) if one is playing.
pub fn appearance(look: SkinLook, interaction: Interaction, now: f32, flash: Option<(Color, f32)>) -> (Color, Color) {
    let (tint, mut glow) = match (look.usable, interaction) {
        (Usable::Yes, Interaction::Pressed) => (grey(PRESSED), HIGHLIGHT.with_alpha(HOVER_GLOW)),
        (Usable::Yes, Interaction::Hovered) => (grey(HOVERED), HIGHLIGHT.with_alpha(HOVER_GLOW)),
        (Usable::Yes, Interaction::None) => (grey(NORMAL), Color::NONE),
        (Usable::Soon, i) => (if i == Interaction::None { SOON } else { SOON_HOVERED }, Color::NONE),
        (Usable::No, i) => (grey(if i == Interaction::None { DISABLED } else { DISABLED + 0.06 }), Color::NONE),
    };
    if look.active {
        let chosen = match look.accent {
            Some(c) => c.with_alpha(CHOSEN_GLOW),
            None => {
                let wave = 0.5 - 0.5 * (now * std::f32::consts::TAU / PULSE).cos();
                HIGHLIGHT.with_alpha(ARMED_GLOW.0 + (ARMED_GLOW.1 - ARMED_GLOW.0) * wave)
            }
        };
        if chosen.alpha() > glow.alpha() {
            glow = chosen;
        }
    }
    if let Some((c, f)) = flash
        && f > glow.alpha()
    {
        glow = c.with_alpha(f);
    }
    (tint, glow)
}

type LookQuery<'w, 's> = Query<
    'w,
    's,
    (&'static Skinned, Option<&'static Interaction>, Option<&'static SkinLook>, Option<&'static PressFx>, Has<SkinButton>, &'static mut ImageNode),
    Or<(With<SkinButton>, With<SkinGlow>)>,
>;

/// Tints skinned buttons and lights their glow (see [`appearance`]). A
/// few dozen nodes a frame, written only when they change.
pub(crate) fn skin_looks(time: Res<Time<Real>>, mut nodes: LookQuery, mut glows: Query<&mut ImageNode, Without<Skinned>>) {
    let now = time.elapsed_secs();
    for (parts, interaction, look, press, button, mut image) in &mut nodes {
        let flash = press.and_then(|p| p.flash(now));
        let (tint, glow) = appearance(look.copied().unwrap_or_default(), interaction.copied().unwrap_or_default(), now, flash);
        if button && image.color != tint {
            image.color = tint;
        }
        if let Some(mut g) = parts.glow.and_then(|g| glows.get_mut(g).ok())
            && g.color != glow
        {
            g.color = glow;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::prelude::*;

    #[test]
    fn card_buttons_wear_their_family() {
        let house = BuildingTypeId(3);
        let none = |_| panic!("not asked when the page is known");
        assert_eq!(card_skin(CardAction::BuildPage(BuildCategory::Military), CardMenu::Root, none), ButtonSkin::Military);
        assert_eq!(card_skin(CardAction::Build(house), CardMenu::Build(BuildCategory::Defence), none), ButtonSkin::Defence);
        // On the root card, a building goes by the page it belongs on.
        assert_eq!(card_skin(CardAction::Build(house), CardMenu::Root, |_| BuildCategory::Economy), ButtonSkin::Economy);
        assert_eq!(card_skin(CardAction::Move, CardMenu::Root, none), ButtonSkin::Command);
        assert_eq!(card_skin(CardAction::Rally, CardMenu::Root, none), ButtonSkin::Command);
        assert_eq!(card_skin(CardAction::Stance(Stance::Passive), CardMenu::Root, none), ButtonSkin::Stance);
        assert_eq!(card_skin(CardAction::Train(UnitTypeId(0)), CardMenu::Root, none), ButtonSkin::Train);
        assert_eq!(card_skin(CardAction::Research(TechId(0)), CardMenu::Root, none), ButtonSkin::Research);
        assert_eq!(card_skin(CardAction::Upgrade { to: house }, CardMenu::Root, none), ButtonSkin::Upgrade);
        assert_eq!(card_skin(CardAction::More, CardMenu::Build(BuildCategory::Economy), none), ButtonSkin::Nav);
        assert_eq!(card_skin(CardAction::Back, CardMenu::Build(BuildCategory::Economy), none), ButtonSkin::Nav);
    }

    #[test]
    fn every_skin_has_its_own_texture() {
        let files: std::collections::HashSet<&str> = ButtonSkin::ALL.iter().map(|s| s.file()).collect();
        assert_eq!(files.len(), ButtonSkin::ALL.len());
        for (i, s) in ButtonSkin::ALL.iter().enumerate() {
            assert_eq!(s.index(), i, "{s:?}");
        }
    }

    #[test]
    fn looks_follow_state() {
        let ready = SkinLook::default();
        let (tint, glow) = appearance(ready, Interaction::None, 0.0, None);
        assert_eq!((tint, glow.alpha()), (grey(NORMAL), 0.0));
        let (tint, glow) = appearance(ready, Interaction::Hovered, 0.0, None);
        assert_eq!(tint, grey(HOVERED));
        assert!((glow.alpha() - HOVER_GLOW).abs() < 1e-6);
        assert_eq!(appearance(ready, Interaction::Pressed, 0.0, None).0, grey(PRESSED));
        // Disabled buttons darken (soon-usable ones less so, and warmer) and never glow on hover.
        let no = SkinLook { usable: Usable::No, ..default() };
        let soon = SkinLook { usable: Usable::Soon, ..default() };
        let luminance = |c: Color| c.luminance();
        assert!(luminance(appearance(no, Interaction::None, 0.0, None).0) < luminance(appearance(soon, Interaction::None, 0.0, None).0));
        assert!(luminance(appearance(soon, Interaction::None, 0.0, None).0) < luminance(grey(NORMAL)));
        let warm = appearance(soon, Interaction::None, 0.0, None).0.to_srgba();
        assert!(warm.red > warm.blue);
        assert_eq!(appearance(no, Interaction::Hovered, 0.0, None).1.alpha(), 0.0);
        // Armed: a gold pulse; chosen: a steady glow in its accent.
        let armed = SkinLook { active: true, ..default() };
        let a: Vec<f32> = (0..12).map(|i| appearance(armed, Interaction::None, i as f32 * 0.1, None).1.alpha()).collect();
        assert!(a.iter().all(|x| (ARMED_GLOW.0 - 1e-4..=ARMED_GLOW.1 + 1e-4).contains(x)), "{a:?}");
        assert!(a.iter().cloned().fold(0.0, f32::max) - a.iter().cloned().fold(1.0, f32::min) > 0.3, "it pulses: {a:?}");
        let red = Color::srgb(1.0, 0.0, 0.0);
        let chosen = SkinLook { active: true, accent: Some(red), ..default() };
        assert_eq!(appearance(chosen, Interaction::None, 0.3, None).1, red.with_alpha(CHOSEN_GLOW));
        assert_eq!(appearance(chosen, Interaction::None, 0.9, None).1, red.with_alpha(CHOSEN_GLOW));
        // A press flash outshines the hover glow while it is the brighter.
        assert_eq!(appearance(ready, Interaction::Hovered, 0.0, Some((red, 0.9))).1, red.with_alpha(0.9));
        assert_eq!(appearance(ready, Interaction::Hovered, 0.0, Some((red, 0.1))).1.alpha(), HOVER_GLOW);
    }
}
