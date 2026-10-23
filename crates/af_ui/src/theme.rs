//! Colours, sizes and small spawn helpers shared by the menu and the HUD:
//! dark translucent panels, the default font at 14-18 px, buttons that
//! shade on hover, and hover tips. These flat colours are what the UI
//! looks like until (or unless) the painted skin ([`crate::skin`]) loads.

use bevy::prelude::*;

use crate::card::ActionGroup;
use crate::fx::PressFx;
use crate::skin::{ButtonSkin, SkinButton, Skinned};

/// Labels, tooltips, panel details.
pub const FONT_SMALL: f32 = 14.0;
/// Top bar values, notices, settings.
pub const FONT_BODY: f32 = 16.0;
/// Names and headings.
pub const FONT_LARGE: f32 = 18.0;

/// Width of one character of the default font (Fira Mono) per pixel of
/// font size: every glyph of a monospaced font advances this far.
pub const GLYPH_ADVANCE: f32 = 0.6;

/// Dark panel fill: just translucent enough to hint at the world behind,
/// opaque enough that boxes behind it never compete with its text.
pub const PANEL_BG: Color = Color::srgba(0.05, 0.06, 0.08, 0.97);
/// Bronze panel and button border.
pub const PANEL_BORDER: Color = Color::srgba(0.58, 0.49, 0.32, 0.9);
/// Normal text.
pub const TEXT: Color = Color::srgb(0.93, 0.92, 0.88);
/// Secondary text (labels, hotkeys, disabled buttons).
pub const TEXT_DIM: Color = Color::srgb(0.62, 0.62, 0.6);
/// Errors, alerts, unaffordable costs, a full population.
pub const TEXT_BAD: Color = Color::srgb(1.0, 0.4, 0.34);
/// Warnings and unit status.
pub const TEXT_WARN: Color = Color::srgb(1.0, 0.82, 0.3);
/// Healthy bars and positive traits.
pub const TEXT_GOOD: Color = Color::srgb(0.55, 0.92, 0.5);
/// Titles, the age name and active buttons' borders.
pub const HIGHLIGHT: Color = Color::srgb(1.0, 0.84, 0.35);
/// Background of progress / health bars.
pub const BAR_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
/// Shield bar fill.
pub const SHIELD: Color = Color::srgb(0.2, 0.5, 0.85);
/// Construction / production bar fill.
pub const PROGRESS: Color = Color::srgb(0.66, 0.5, 0.16);
/// Health bar fills: healthy, hurt, nearly dead. Deep enough that the
/// light label on top stays readable.
pub const HP_GOOD: Color = Color::srgb(0.22, 0.58, 0.24);
pub const HP_HURT: Color = Color::srgb(0.72, 0.56, 0.1);
pub const HP_LOW: Color = Color::srgb(0.7, 0.18, 0.14);
/// Neutral button fill.
pub const BUTTON: Color = Color::srgb(0.2, 0.22, 0.26);
/// Fill of a command button that cannot be used now.
pub const BUTTON_DISABLED: Color = Color::srgb(0.12, 0.12, 0.13);

/// sRGB colour from a content `Rgb`.
pub fn rgb(c: [f32; 3]) -> Color {
    Color::srgb(c[0], c[1], c[2])
}

/// A player's colour lifted for text on a dark panel (dark faction
/// colours such as cobalt blue are hard to read as is).
pub fn player_text(c: [f32; 3]) -> Color {
    rgb(c).mix(&Color::WHITE, 0.35)
}

/// A text node.
pub fn text(s: impl Into<String>, size: f32, color: Color) -> (Text, TextFont, TextColor) {
    (Text::new(s), TextFont::from_font_size(size), TextColor(color))
}

/// A tight dark drop shadow for text drawn over coloured fills (bars).
pub fn text_shadow() -> TextShadow {
    TextShadow { offset: Vec2::splat(1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) }
}

/// Background and border of a panel.
pub fn panel_colors() -> (BackgroundColor, BorderColor) {
    (BackgroundColor(PANEL_BG), BorderColor::all(PANEL_BORDER))
}

/// `node`'s padding, or `default` where the caller left it unset (a zero
/// padding means unset: no panel or button wants its content on its frame).
fn padding_or(node: &Node, default: UiRect) -> UiRect {
    if node.padding == UiRect::DEFAULT { default } else { node.padding }
}

/// The usual panel box: bordered, rounded and padded (8 px unless `node`
/// asks for its own padding).
pub fn panel_node(node: Node) -> Node {
    Node { padding: padding_or(&node, UiRect::all(px(8))), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(4)), ..node }
}

/// Base colour of a command card button of this group.
pub fn group_color(g: ActionGroup) -> Color {
    match g {
        ActionGroup::Command => Color::srgb(0.17, 0.25, 0.37),
        ActionGroup::Stance => Color::srgb(0.22, 0.22, 0.32),
        ActionGroup::Build => Color::srgb(0.34, 0.25, 0.13),
        ActionGroup::Train => Color::srgb(0.15, 0.32, 0.18),
        ActionGroup::Research => Color::srgb(0.31, 0.18, 0.37),
        ActionGroup::Upgrade => Color::srgb(0.4, 0.33, 0.1),
        ActionGroup::Nav => Color::srgb(0.25, 0.25, 0.27),
    }
}

/// A colour lightened for the hover / pressed state.
pub fn shade(base: Color, interaction: Interaction) -> Color {
    match interaction {
        Interaction::Pressed => base.lighter(0.16),
        Interaction::Hovered => base.lighter(0.08),
        Interaction::None => base,
    }
}

/// A plain button whose background follows hover/press from `base`.
#[derive(Component, Clone, Copy, Debug)]
pub struct ShadedButton {
    pub base: Color,
}

/// A button: centred content, rounded border, padded (10 by 5 px unless
/// `node` asks for its own padding), shaded on hover, squashed when
/// pressed; in the skin, a [`ButtonSkin::Plain`] one.
pub fn button(node: Node, base: Color) -> impl Bundle {
    skinned_button(node, base, ButtonSkin::Plain)
}

/// A [`button`] that wears `skin` (and `base` until the skin loads).
pub fn skinned_button(node: Node, base: Color, skin: ButtonSkin) -> impl Bundle {
    (flat_button(node, base), SkinButton(skin))
}

/// A [`button`] that keeps its flat `base` colour in the skin too: a
/// colour swatch whose colour is what it says (the menu's water scale),
/// too small for a frame.
pub fn flat_button(node: Node, base: Color) -> impl Bundle {
    (
        Button,
        Node {
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: padding_or(&node, UiRect::axes(px(10), px(5))),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(4)),
            ..node
        },
        BackgroundColor(base),
        BorderColor::all(PANEL_BORDER),
        ShadedButton { base },
        PressFx::on_click(),
    )
}

/// Text shown in the floating hover tip while the cursor is over this
/// node (which needs an `Interaction`, e.g. a `Button`).
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Tip(pub String);

/// Flat buttons follow hover and press; skinned ones are tinted by the skin.
#[allow(clippy::type_complexity)]
pub(crate) fn shade_buttons(mut q: Query<(&Interaction, &ShadedButton, &mut BackgroundColor), (Changed<Interaction>, Without<Skinned>)>) {
    for (interaction, b, mut bg) in &mut q {
        bg.0 = shade(b.base, *interaction);
    }
}

/// The pieces a word may be broken into: after each hyphen inside it
/// ("Ferro-Concrete" -> "Ferro-", "Concrete"), so a hyphenated word can
/// wrap like two words without losing its hyphen.
fn word_pieces(word: &str) -> impl Iterator<Item = &str> {
    word.split_inclusive('-')
}

/// `label` broken into lines that hold `per_line` characters, between
/// words or after a hyphen inside a word ("Ferro-" / "Concrete"). A piece
/// longer than a line gets a line of its own (see [`fit_label`] for when
/// even that is too long).
pub fn wrap_words(label: &str, per_line: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in label.split_whitespace() {
        for (i, piece) in word_pieces(word).enumerate() {
            // The first piece of a word follows a space; the rest join on.
            let sep = usize::from(i == 0);
            match lines.last_mut() {
                Some(line) if !line.is_empty() && line.chars().count() + sep + piece.chars().count() <= per_line => {
                    if sep == 1 {
                        line.push(' ');
                    }
                    line.push_str(piece);
                }
                _ => lines.push(piece.to_string()),
            }
        }
    }
    lines
}

/// Characters of the default font at `size` px that fit `width` px.
pub fn chars_that_fit(width: f32, size: f32) -> usize {
    (width / (size * GLYPH_ADVANCE)).floor().max(1.0) as usize
}

/// `lines` with every line longer than `per_line` cut into hyphenated
/// chunks ("Architec-", "ture"): the last resort of [`fit_label`].
fn hyphenate(lines: Vec<String>, per_line: usize) -> Vec<String> {
    let room = per_line.saturating_sub(1).max(1);
    let mut out = Vec::new();
    for line in lines {
        let mut chars: Vec<char> = line.chars().collect();
        while chars.len() > per_line {
            let rest = chars.split_off(room);
            out.push(chars.iter().collect::<String>() + "-");
            chars = rest;
        }
        out.push(chars.into_iter().collect());
    }
    out
}

/// A button label laid out ahead of Bevy's text layout: the largest of
/// `sizes` (in order of preference) at which `label` wraps into at most
/// `max_lines` lines that each fit `width` px, and the text with those line
/// breaks. Lines break between words and after hyphens. When no size
/// manages that, the smallest one is used and any word still too long is
/// cut with a hyphen ("Architec-" / "ture"), so nothing is drawn past the
/// button's edge. Breaking the lines here keeps the label's measured
/// height equal to what is drawn, so it centres properly in its button.
pub fn fit_label(label: &str, width: f32, sizes: &[f32], max_lines: usize) -> (f32, String) {
    let Some(&smallest) = sizes.last() else { return (FONT_SMALL, label.to_string()) };
    for &size in sizes {
        let per_line = chars_that_fit(width, size);
        let lines = wrap_words(label, per_line);
        if lines.len() <= max_lines && lines.iter().all(|l| l.chars().count() <= per_line) {
            return (size, lines.join("\n"));
        }
    }
    let per_line = chars_that_fit(width, smallest);
    (smallest, hyphenate(wrap_words(label, per_line), per_line).join("\n"))
}

/// Whether a label laid out by [`fit_label`] fits: at most `max_lines`
/// lines, none wider than `width` px at `size`.
pub fn label_fits(size: f32, text: &str, width: f32, max_lines: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    lines.len() <= max_lines && lines.iter().all(|l| l.chars().count() as f32 * size * GLYPH_ADVANCE <= width + 1e-3)
}

/// Sets `text` only when it differs (keeps change detection quiet).
pub fn set_text(text: &mut Mut<Text>, s: &str) {
    if text.0 != s {
        text.0.clear();
        text.0.push_str(s);
    }
}

/// Sets a colour component only when it differs.
pub fn set_color(color: &mut Mut<TextColor>, c: Color) {
    if color.0 != c {
        color.0 = c;
    }
}

/// Sets a node's `display` only when it differs.
pub fn set_display(node: &mut Mut<Node>, visible: bool) {
    let d = if visible { Display::Flex } else { Display::None };
    if node.display != d {
        node.display = d;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_keep_the_padding_asked_for() {
        let asked = UiRect::axes(px(28), px(10));
        assert_eq!(panel_node(Node { padding: asked, ..default() }).padding, asked);
        assert_eq!(panel_node(Node { width: px(50), ..default() }).padding, UiRect::all(px(8)), "unset: the default");
        let mut world = World::new();
        let mut padding = |node: Node| {
            let e = world.spawn(button(node, BUTTON)).id();
            world.get::<Node>(e).map(|n| n.padding)
        };
        assert_eq!(padding(Node { padding: UiRect::axes(px(8), px(1)), ..default() }), Some(UiRect::axes(px(8), px(1))));
        assert_eq!(padding(Node::default()), Some(UiRect::axes(px(10), px(5))));
    }

    #[test]
    fn words_wrap_greedily() {
        assert_eq!(wrap_words("Seek the Fallen Star", 9), vec!["Seek the", "Fallen", "Star"]);
        assert_eq!(wrap_words("  Raise   the Banners ", 9), vec!["Raise the", "Banners"]);
        assert_eq!(wrap_words("Arquebusier", 9), vec!["Arquebusier"], "a long word keeps its own line");
        assert!(wrap_words("", 9).is_empty());
    }

    #[test]
    fn hyphenated_words_break_after_the_hyphen() {
        assert_eq!(wrap_words("Ferro-Concrete", 9), vec!["Ferro-", "Concrete"]);
        assert_eq!(wrap_words("Ferro-Concrete", 14), vec!["Ferro-Concrete"], "whole when it fits");
        assert_eq!(wrap_words("Man-at-Arms", 9), vec!["Man-at-", "Arms"]);
        assert_eq!(wrap_words("Steam-Forged Plate", 9), vec!["Steam-", "Forged", "Plate"]);
        assert_eq!(wrap_words("A Thunder-Rune", 12), vec!["A Thunder-", "Rune"]);
    }

    #[test]
    fn labels_shrink_until_they_fit() {
        let sizes = [14.0, 12.0, 11.0, 10.0];
        // 76 px holds 9 characters at 14 px, 10 at 12 px, 11 at 11 px and 12 at 10 px.
        assert_eq!(fit_label("Stand ground", 76.0, &sizes, 3), (14.0, "Stand\nground".to_string()), "short words wrap");
        assert_eq!(fit_label("Aggressive", 76.0, &sizes, 3), (12.0, "Aggressive".to_string()));
        assert_eq!(fit_label("Arquebusier", 76.0, &sizes, 3).0, 11.0);
        assert_eq!(fit_label("Upgrade Reinforced Wall", 76.0, &sizes, 3), (12.0, "Upgrade\nReinforced\nWall".to_string()));
        assert_eq!(fit_label("Architecture", 76.0, &sizes, 3), (10.0, "Architecture".to_string()));
        assert_eq!(fit_label("Ferro-Concrete", 76.0, &sizes, 3), (14.0, "Ferro-\nConcrete".to_string()), "hyphens break at full size");
        assert_eq!(fit_label("", 76.0, &sizes, 3), (14.0, String::new()));
    }

    #[test]
    fn words_too_long_for_any_size_are_hyphenated() {
        let sizes = [14.0, 12.0, 10.0];
        let (size, text) = fit_label("Counterbombardment", 76.0, &sizes, 3);
        assert_eq!((size, text.as_str()), (10.0, "Counterbomb-\nardment"));
        assert!(label_fits(size, &text, 76.0, 3));
        // Too many lines at every size: the smallest, still never too wide.
        assert_eq!(fit_label("A b c d e", 10.0, &sizes, 3), (10.0, "A\nb\nc\nd\ne".to_string()));
        assert!(!label_fits(14.0, "Arquebusiers", 76.0, 3));
        assert!(label_fits(14.0, "Stand\nground", 76.0, 3));
        assert!(!label_fits(14.0, "a\nb\nc\nd", 76.0, 3));
    }
}
