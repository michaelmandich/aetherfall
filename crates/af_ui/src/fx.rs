//! Press feedback in the manner of a card game: a pressed button squashes
//! and springs back past its size, flashes, and (for the command card's
//! build, train, research and upgrade buttons) sweeps a shine across
//! itself and throws a ring of light and sparks; a refused press shakes
//! and flashes red; a press that turns the card's page springs the new
//! page's buttons in, column by column. Everything runs on real time, so
//! it plays while the game is paused.
//!
//! * [`PressFx`] on a button holds the press playing on it. Plain buttons
//!   start one when clicked ([`press_plain_buttons`]); the command card
//!   starts its slots' presses itself (mouse and hotkeys alike).
//! * [`animate_presses`] moves the button (`UiTransform`: scale and shake
//!   need no layout) and its shine; [`crate::skin`] draws the flash on the
//!   glow; [`spawn_burst`] and [`animate_particles`] handle the ring and the
//!   sparks, which live on a root node of their own, over the HUD, until
//!   they fade (at most [`MAX_PARTICLES`] at a time).
//! * The curves are pure functions of the time since the press.
//!
//! Without the skin's textures only the squash and the shake play.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::card::CardAction;
use crate::skin::{Skinned, UiSkin};
use crate::theme::HIGHLIGHT;

/// What a press looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PressKind {
    /// A command card press that makes something (build, train, research,
    /// upgrade): deep squash, flash, shine, ring and sparks.
    Full,
    /// Any other command card press (orders, stances): a lighter squash
    /// and the flash.
    Light,
    /// A press that turned the command card's page (a build page, More,
    /// Back), played on the new page's buttons: they appear squashed and
    /// spring out with a faint flash, a column at a time
    /// ([`turn_start`]).
    Turn,
    /// A command card press that was refused (unavailable, unaffordable):
    /// a shake and a red flash.
    Refused,
    /// A plain button: a light squash.
    #[default]
    Plain,
}

/// The press feedback of a command card action carried out.
pub fn press_kind(action: CardAction) -> PressKind {
    match action {
        CardAction::Build(_) | CardAction::Train(_) | CardAction::Research(_) | CardAction::Upgrade { .. } => PressKind::Full,
        CardAction::BuildPage(_) | CardAction::More | CardAction::Back => PressKind::Turn,
        _ => PressKind::Light,
    }
}

/// Seconds to squash down.
const SQUASH_TIME: f32 = 0.06;
/// Seconds from the press until the button is still again.
const SETTLE_TIME: f32 = 0.35;
/// The spring back: how fast it calms (1/s) and how fast it swings
/// (rad/s). Their ratio sets the overshoot: `exp(-PI * DECAY / SWING)` of
/// the squash depth, 0.6 here (a 0.1 squash peaks at 1.06).
const SPRING_DECAY: f32 = 10.0;
const SPRING_SWING: f32 = 61.5;
/// A refused press: shaken this far (logical px) at this rate for this long.
const SHAKE_PX: f32 = 4.0;
const SHAKE_HZ: f32 = 18.0;
const SHAKE_TIME: f32 = 0.25;
/// The flash fades over this long.
const FLASH_TIME: f32 = 0.3;
/// The shine starts this late and crosses the button in this long.
const SHINE_DELAY: f32 = 0.03;
const SHINE_TIME: f32 = 0.3;
/// Width of the shine streak (logical px; its height follows the button).
const SHINE_WIDTH: f32 = 36.0;
/// Colour of a refused press's flash.
const REFUSED: Color = Color::srgb(1.0, 0.26, 0.2);
/// A page turn reaches each column of the card this much later than the
/// one before.
const TURN_RIPPLE: f32 = 0.025;

impl PressKind {
    /// How far the button squashes (fraction of its size).
    fn depth(self) -> f32 {
        match self {
            PressKind::Full => 0.1,
            PressKind::Light => 0.07,
            PressKind::Turn => 0.06,
            PressKind::Plain => 0.05,
            PressKind::Refused => 0.0,
        }
    }

    /// Peak strength of the flash.
    fn flash(self) -> f32 {
        match self {
            PressKind::Full => 0.95,
            PressKind::Light => 0.6,
            PressKind::Turn => 0.3,
            PressKind::Refused => 0.8,
            PressKind::Plain => 0.0,
        }
    }

    /// Seconds until every part of the press is over.
    pub fn duration(self) -> f32 {
        match self {
            PressKind::Refused => SHAKE_TIME.max(FLASH_TIME),
            PressKind::Full => SETTLE_TIME.max(SHINE_DELAY + SHINE_TIME),
            PressKind::Light | PressKind::Turn | PressKind::Plain => SETTLE_TIME,
        }
    }
}

/// Scale of a button `t` s after a press of `depth`: eased down to
/// `1 - depth` in [`SQUASH_TIME`], then a damped spring back that
/// overshoots and settles by [`SETTLE_TIME`].
pub fn press_scale(t: f32, depth: f32) -> f32 {
    if !(0.0..SETTLE_TIME).contains(&t) {
        return 1.0;
    }
    if t < SQUASH_TIME {
        let u = t / SQUASH_TIME;
        return 1.0 - depth * u * u * (3.0 - 2.0 * u);
    }
    // Released from rest at `1 - depth`.
    let tau = t - SQUASH_TIME;
    let (a, w) = (SPRING_DECAY, SPRING_SWING);
    1.0 - depth * (-a * tau).exp() * ((w * tau).cos() + a / w * (w * tau).sin())
}

/// Sideways offset (logical px) of a refused button `t` s after the press.
pub fn shake(t: f32) -> f32 {
    if !(0.0..SHAKE_TIME).contains(&t) {
        return 0.0;
    }
    let fade = 1.0 - t / SHAKE_TIME;
    SHAKE_PX * (TAU * SHAKE_HZ * t).sin() * fade * fade
}

/// Strength of a press's flash `t` s after it: at once to its peak, then
/// fading out over [`FLASH_TIME`].
pub fn flash(kind: PressKind, t: f32) -> f32 {
    if !(0.0..FLASH_TIME).contains(&t) {
        return 0.0;
    }
    let fade = 1.0 - t / FLASH_TIME;
    kind.flash() * fade * fade
}

/// When a page turn starts on a button in card column `column`, for a
/// turn at real time `now`: timed so that the first column's spring is let
/// go at once and each next one's a little later. Until its spring is let
/// go a button holds squashed ([`button_scale`]): the new page's buttons
/// appear small, never at full size first.
pub fn turn_start(now: f32, column: usize) -> f32 {
    now - SQUASH_TIME + column as f32 * TURN_RIPPLE
}

/// Scale of a button `t` s into a press of `kind` ([`press_scale`]); the
/// buttons of a page turn hold squashed until their spring is let go
/// ([`turn_start`]), however long that is.
pub fn button_scale(kind: PressKind, t: f32) -> f32 {
    if kind == PressKind::Turn && t < SQUASH_TIME {
        return 1.0 - kind.depth();
    }
    press_scale(t, kind.depth())
}

/// How far across the button the shine of a full press is (0 to 1), and
/// whether it is crossing at all.
pub fn shine(t: f32) -> Option<f32> {
    let u = (t - SHINE_DELAY) / SHINE_TIME;
    (0.0..1.0).contains(&u).then_some(1.0 - (1.0 - u) * (1.0 - u))
}

/// The press feedback of one button.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PressFx {
    /// Starts a [`PressKind::Plain`] press whenever the button is clicked.
    on_click: bool,
    /// Has the shine for [`PressKind::Full`] presses (the skin adds it).
    bursts: bool,
    /// The press playing: its kind and when it started (real seconds).
    playing: Option<(PressKind, f32)>,
}

impl PressFx {
    /// A plain button: a light squash on every click.
    pub fn on_click() -> PressFx {
        PressFx { on_click: true, ..default() }
    }

    /// A command card slot: presses are started by the card ([`PressFx::start`]),
    /// full ones with a shine.
    pub fn card_slot() -> PressFx {
        PressFx { bursts: true, ..default() }
    }

    pub fn bursts(&self) -> bool {
        self.bursts
    }

    /// Plays `kind` from real time `now` (a press playing is cut short).
    pub fn start(&mut self, kind: PressKind, now: f32) {
        self.playing = Some((kind, now));
    }

    /// Colour and strength of the press's flash at real time `now`.
    pub fn flash(&self, now: f32) -> Option<(Color, f32)> {
        let (kind, start) = self.playing?;
        let f = flash(kind, now - start);
        let color = if kind == PressKind::Refused { REFUSED } else { HIGHLIGHT };
        (f > 0.0).then_some((color, f))
    }
}

/// Starts the light squash of a plain button when it is clicked.
pub(crate) fn press_plain_buttons(time: Res<Time<Real>>, mut buttons: Query<(&Interaction, &mut PressFx), Changed<Interaction>>) {
    for (interaction, mut fx) in &mut buttons {
        if fx.on_click && *interaction == Interaction::Pressed {
            fx.start(PressKind::Plain, time.elapsed_secs());
        }
    }
}

/// The shine streak swept across a button by a full press, hidden until
/// then; a child of a clipping node over the button ([`crate::skin`]).
pub(crate) fn spawn_shine(commands: &mut Commands, skin: &UiSkin) -> Entity {
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: px(0), top: percent(-30), width: px(SHINE_WIDTH), height: percent(160), ..default() },
            ImageNode { image: skin.shine.clone(), image_mode: NodeImageMode::Stretch, color: Color::NONE, ..default() },
            Pickable::IGNORE,
        ))
        .id()
}

/// Plays the presses: scale and shake on the button, the shine's sweep.
pub(crate) fn animate_presses(
    time: Res<Time<Real>>,
    mut buttons: Query<(&mut PressFx, &mut UiTransform, &ComputedNode, Option<&Skinned>)>,
    mut shines: Query<(&mut UiTransform, &mut ImageNode), Without<PressFx>>,
) {
    let now = time.elapsed_secs();
    for (mut fx, mut transform, node, parts) in &mut buttons {
        let Some((kind, start)) = fx.playing else { continue };
        let t = now - start;
        let done = t >= kind.duration();
        if done {
            fx.playing = None;
        }
        let shift = if kind == PressKind::Refused { shake(t) } else { 0.0 };
        let next = UiTransform { translation: Val2::px(shift, 0.0), scale: Vec2::splat(button_scale(kind, t)), ..UiTransform::IDENTITY };
        transform.set_if_neq(next);
        let Some((mut shine_at, mut image)) = parts.and_then(|p| p.shine).and_then(|s| shines.get_mut(s).ok()) else { continue };
        let across = shine(t).filter(|_| kind == PressKind::Full && !done);
        let width = node.size().x * node.inverse_scale_factor();
        let (x, alpha) = across.map_or((-SHINE_WIDTH, 0.0), |u| (-SHINE_WIDTH + (width + SHINE_WIDTH) * u, 0.85 * (PI * u).sin()));
        shine_at.set_if_neq(UiTransform::from_translation(Val2::px(x, 0.0)));
        let color = Color::WHITE.with_alpha(alpha);
        if image.color != color {
            image.color = color;
        }
    }
}

/// Most ring and spark nodes alive at once.
pub const MAX_PARTICLES: usize = 64;
/// Sparks thrown by one press.
pub const SPARKS: usize = 10;
/// Draw order of the bursts: over the HUD's panels and tooltips.
const BURST_Z: i32 = 50;
/// The ring: its life, and its scale over it (of a node 1.6 button widths wide).
const RING_LIFE: f32 = 0.45;
const RING_SCALE: (f32, f32) = (0.45, 1.25);
/// A spark's life and speed (logical px/s) range, and its fall (px/s²).
const SPARK_LIFE: (f32, f32) = (0.45, 0.7);
const SPARK_SPEED: (f32, f32) = (150.0, 280.0);
const SPARK_GRAVITY: f32 = 560.0;
/// The longest a burst lives.
const BURST_LIFE: f32 = SPARK_LIFE.1;

/// One burst of a press: a root node at the button's centre whose
/// children are its ring and sparks; despawned when they are done.
#[derive(Component, Clone, Copy, Debug)]
pub struct Burst {
    start: f32,
}

/// A ring or a spark of a burst.
#[derive(Component, Clone, Copy, Debug)]
pub struct Particle {
    start: f32,
    motion: Motion,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Motion {
    Ring,
    Spark { from: Vec2, velocity: Vec2, life: f32, spin: f32 },
}

/// Ring scale and strength `t` s into a burst, while it lives.
fn ring(t: f32) -> Option<(f32, f32)> {
    let u = t / RING_LIFE;
    (0.0..1.0).contains(&u).then(|| {
        let ease = 1.0 - (1.0 - u).powi(3);
        (RING_SCALE.0 + (RING_SCALE.1 - RING_SCALE.0) * ease, 0.9 * (1.0 - u).powf(1.5))
    })
}

/// Where a spark is (offset from the burst's centre), its scale and its
/// strength `t` s into its flight, while it lives.
fn spark(from: Vec2, velocity: Vec2, life: f32, t: f32) -> Option<(Vec2, f32, f32)> {
    let u = t / life;
    (0.0..1.0).contains(&u).then(|| {
        let at = from + velocity * t + Vec2::new(0.0, 0.5 * SPARK_GRAVITY * t * t);
        // A quick pop in, then shrinking and fading as it flies.
        let pop = (t / 0.05).min(1.0);
        (at, pop * (1.0 - 0.6 * u), (1.0 - u * u).min(pop))
    })
}

/// A pseudo-random number in `0..1` for spark `i` of burst `seed` (looks
/// only; the simulation never sees it).
fn noise(seed: u32, i: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ i.wrapping_mul(0x85EB_CA6B) ^ 0x2545_F491;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Throws a ring and `sparks` sparks out of a button whose centre is at
/// `centre` and whose size is `size` (logical px), at real time `now`.
pub(crate) fn spawn_burst(commands: &mut Commands, skin: &UiSkin, centre: Vec2, size: Vec2, now: f32, seed: u32, sparks: usize) {
    let ring_px = size.x.max(size.y) * 1.6;
    let gold = HIGHLIGHT;
    commands
        .spawn((
            Burst { start: now },
            Node { position_type: PositionType::Absolute, left: px(centre.x), top: px(centre.y), width: px(0), height: px(0), ..default() },
            GlobalZIndex(BURST_Z),
            Pickable::IGNORE,
        ))
        .with_children(|b| {
            b.spawn((
                Particle { start: now, motion: Motion::Ring },
                Node { position_type: PositionType::Absolute, left: px(-ring_px / 2.0), top: px(-ring_px / 2.0), width: px(ring_px), height: px(ring_px), ..default() },
                ImageNode { image: skin.burst_ring.clone(), image_mode: NodeImageMode::Stretch, color: gold.with_alpha(0.0), ..default() },
                UiTransform::from_scale(Vec2::splat(RING_SCALE.0)),
                Pickable::IGNORE,
            ));
            for i in 0..sparks as u32 {
                let r = |k: u32| noise(seed, i * 8 + k);
                // Spread round the button, flying outward (a little upward).
                let angle = (i as f32 + 0.6 * r(0)) / sparks as f32 * TAU;
                let dir = Vec2::new(angle.cos(), angle.sin());
                let from = dir * size * 0.32;
                let speed = SPARK_SPEED.0 + (SPARK_SPEED.1 - SPARK_SPEED.0) * r(1);
                let velocity = dir * speed - Vec2::new(0.0, 90.0);
                let life = SPARK_LIFE.0 + (SPARK_LIFE.1 - SPARK_LIFE.0) * r(2);
                let px_size = 14.0 + 10.0 * r(3);
                let tint = gold.mix(&Color::WHITE, 0.1 + 0.4 * r(4));
                b.spawn((
                    Particle { start: now, motion: Motion::Spark { from, velocity, life, spin: (r(5) - 0.5) * 12.0 } },
                    Node { position_type: PositionType::Absolute, left: px(-px_size / 2.0), top: px(-px_size / 2.0), width: px(px_size), height: px(px_size), ..default() },
                    ImageNode { image: skin.spark.clone(), image_mode: NodeImageMode::Stretch, color: tint.with_alpha(0.0), ..default() },
                    UiTransform::from_translation(Val2::px(from.x, from.y)),
                    Pickable::IGNORE,
                ));
            }
        });
}

/// Moves and fades the rings and sparks; despawns finished bursts.
pub(crate) fn animate_particles(
    mut commands: Commands,
    time: Res<Time<Real>>,
    bursts: Query<(Entity, &Burst)>,
    mut particles: Query<(&Particle, &mut UiTransform, &mut ImageNode)>,
) {
    let now = time.elapsed_secs();
    for (e, burst) in &bursts {
        if now - burst.start > BURST_LIFE {
            commands.entity(e).despawn();
        }
    }
    for (p, mut transform, mut image) in &mut particles {
        let t = now - p.start;
        let (next, alpha) = match p.motion {
            Motion::Ring => ring(t).map_or((UiTransform::IDENTITY, 0.0), |(s, a)| (UiTransform::from_scale(Vec2::splat(s)), a)),
            Motion::Spark { from, velocity, life, spin } => spark(from, velocity, life, t).map_or((UiTransform::IDENTITY, 0.0), |(at, s, a)| {
                (UiTransform { translation: Val2::px(at.x, at.y), scale: Vec2::splat(s), rotation: Rot2::radians(spin * t) }, a)
            }),
        };
        *transform = next;
        image.color.set_alpha(alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::prelude::*;

    fn samples(f: impl Fn(f32) -> f32) -> Vec<(f32, f32)> {
        (0..=500).map(|i| i as f32 * 0.001).map(|t| (t, f(t))).collect()
    }

    #[test]
    fn a_press_squashes_overshoots_and_settles() {
        let s = samples(|t| press_scale(t, 0.1));
        assert_eq!(press_scale(0.0, 0.1), 1.0);
        assert!((press_scale(SQUASH_TIME, 0.1) - 0.9).abs() < 1e-3, "squashed to 0.9 in {SQUASH_TIME} s");
        let (t_min, min) = s.iter().cloned().fold((0.0, 2.0), |a, b| if b.1 < a.1 { b } else { a });
        assert!((min - 0.9).abs() < 1e-3 && t_min <= SQUASH_TIME + 0.002, "{t_min} {min}");
        let (t_max, max) = s.iter().cloned().fold((0.0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
        assert!((1.05..1.07).contains(&max), "overshoots to about 1.06: {max}");
        assert!((0.08..0.16).contains(&t_max), "peaks soon after the release: {t_max}");
        // Still by the settle time, and exactly 1 from then on.
        for &(t, v) in &s {
            if t > 0.3 {
                assert!((v - 1.0).abs() < 0.01, "{t} {v}");
            }
            if t >= SETTLE_TIME {
                assert_eq!(v, 1.0);
            }
        }
        // A lighter press squashes less; no press, no squash.
        assert!(press_scale(SQUASH_TIME, 0.05) > press_scale(SQUASH_TIME, 0.1));
        assert_eq!(press_scale(0.1, 0.0), 1.0);
        assert_eq!(press_scale(-0.1, 0.1), 1.0);
    }

    #[test]
    fn a_refused_press_shakes_then_stops() {
        let s = samples(shake);
        assert!(s.iter().all(|(_, x)| x.abs() <= SHAKE_PX));
        assert!(s.iter().any(|(_, x)| *x > 2.0) && s.iter().any(|(_, x)| *x < -2.0), "both ways");
        assert!(s.iter().filter(|(t, _)| *t >= SHAKE_TIME).all(|(_, x)| *x == 0.0));
        assert_eq!(shake(0.0), 0.0);
        assert_eq!(PressKind::Refused.depth(), 0.0, "no squash");
    }

    #[test]
    fn flashes_fade() {
        assert_eq!(flash(PressKind::Full, 0.0), PressKind::Full.flash());
        assert!(flash(PressKind::Full, 0.1) > flash(PressKind::Full, 0.2));
        assert_eq!(flash(PressKind::Full, FLASH_TIME), 0.0);
        assert!(flash(PressKind::Full, 0.0) > flash(PressKind::Light, 0.0), "a light press flashes less");
        assert_eq!(flash(PressKind::Plain, 0.0), 0.0);
        let mut fx = PressFx::card_slot();
        assert_eq!(fx.flash(1.0), None);
        fx.start(PressKind::Refused, 1.0);
        let (c, f) = fx.flash(1.05).unwrap();
        assert!(c == REFUSED && f > 0.3);
        assert_eq!(fx.flash(1.0 + FLASH_TIME + 1e-3), None);
        // Every part of a press is over by its duration.
        for kind in [PressKind::Full, PressKind::Light, PressKind::Turn, PressKind::Refused, PressKind::Plain] {
            let d = kind.duration();
            assert_eq!((button_scale(kind, d), shake(d), flash(kind, d)), (1.0, 0.0, 0.0), "{kind:?}");
        }
        assert!(shine(PressKind::Full.duration()).is_none());
    }

    #[test]
    fn the_shine_crosses_once() {
        assert_eq!(shine(0.0), None);
        let a = shine(SHINE_DELAY + 0.05).unwrap();
        let b = shine(SHINE_DELAY + 0.2).unwrap();
        assert!(0.0 < a && a < b && b < 1.0);
        assert_eq!(shine(SHINE_DELAY + SHINE_TIME), None);
    }

    #[test]
    fn particles_burn_out() {
        assert!(ring(0.0).is_some_and(|(s, a)| s == RING_SCALE.0 && a > 0.8));
        let (s1, a1) = ring(0.2).unwrap();
        let (s2, a2) = ring(0.4).unwrap();
        assert!(s1 < s2 && s2 <= RING_SCALE.1 && a2 < a1, "grows and fades");
        assert_eq!(ring(RING_LIFE), None);
        let (from, v) = (Vec2::new(10.0, 0.0), Vec2::new(200.0, -90.0));
        let (p1, _, _) = spark(from, v, 0.6, 0.1).unwrap();
        let (p2, s2, a2) = spark(from, v, 0.6, 0.5).unwrap();
        assert!(p2.x > p1.x && p1.x > from.x, "flies out");
        assert!(p2.y > p1.y, "and falls");
        assert!(s2 < 1.0 && a2 < 0.5, "shrinking and fading");
        assert_eq!(spark(from, v, 0.6, 0.6), None);
        // Every spark is gone before its burst is despawned.
        for seed in 0..20 {
            for i in 0..SPARKS as u32 * 8 {
                let n = noise(seed, i);
                assert!((0.0..1.0).contains(&n));
                assert!(SPARK_LIFE.0 + (SPARK_LIFE.1 - SPARK_LIFE.0) * n <= BURST_LIFE);
            }
        }
        const { assert!(RING_LIFE <= BURST_LIFE) };
    }

    #[test]
    fn making_things_gets_the_full_press() {
        assert_eq!(press_kind(CardAction::Build(BuildingTypeId(0))), PressKind::Full);
        assert_eq!(press_kind(CardAction::Train(UnitTypeId(0))), PressKind::Full);
        assert_eq!(press_kind(CardAction::Research(TechId(0))), PressKind::Full);
        assert_eq!(press_kind(CardAction::Upgrade { to: BuildingTypeId(0) }), PressKind::Full);
        for a in [CardAction::Move, CardAction::Stop, CardAction::Stance(Stance::Defensive), CardAction::Rally] {
            assert_eq!(press_kind(a), PressKind::Light, "{a:?}");
        }
        // Paging presses play on the page they open.
        for a in [CardAction::BuildPage(crate::card::BuildCategory::Economy), CardAction::More, CardAction::Back] {
            assert_eq!(press_kind(a), PressKind::Turn, "{a:?}");
        }
    }

    #[test]
    fn a_page_turn_ripples_across_the_card() {
        let squashed = 1.0 - PressKind::Turn.depth();
        // The scale of the button in `column` `dt` s after a turn at 1 s.
        let at = |column: usize, dt: f32| button_scale(PressKind::Turn, 1.0 + dt - turn_start(1.0, column));
        // Every column appears squashed, none at full size first ...
        for column in 0..crate::card::COLS {
            assert!((at(column, 0.0) - squashed).abs() < 1e-4, "column {column} starts at {}", at(column, 0.0));
            // ... and holds so until its spring is let go.
            let release = column as f32 * TURN_RIPPLE;
            assert!((0..20).map(|i| release * i as f32 / 20.0).all(|dt| at(column, dt) <= squashed + 1e-4), "column {column} pops");
        }
        // It springs out a column at a time: each one is back to full size
        // later than the one before, and the whole card within half a second.
        let back = |column: usize| (0..500).map(|i| i as f32 * 0.001).find(|dt| at(column, *dt) >= 1.0).unwrap();
        let backs: Vec<f32> = (0..crate::card::COLS).map(back).collect();
        assert!(backs.windows(2).all(|w| w[0] < w[1]), "{backs:?}");
        const { assert!(SETTLE_TIME - SQUASH_TIME + 4.0 * TURN_RIPPLE < 0.5) };
        assert!(flash(PressKind::Turn, 0.0) < flash(PressKind::Light, 0.0), "a faint flash");
        assert_eq!(PressKind::Turn.duration(), SETTLE_TIME);
    }
}
