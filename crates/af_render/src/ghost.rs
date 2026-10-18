//! Placement ghosts: the translucent building (or wall line) af_input's
//! `PlacementPreview` says a click would place. Three states that read
//! apart over any terrain: green (valid), amber (only unaffordable) and a
//! denser red crossed by an X on its top (cannot go there), so a blocked
//! spot never passes for one that only needs more resources.

use af_core::ext::input::{PlacementGhost, PlacementPreview};
use af_core::prelude::*;
use bevy::prelude::*;

use crate::assets::{Painter, no_shadows};
use crate::entities::Ground;
use crate::look::Look;
use crate::motion::ground_transform;
use crate::scene::RenderScene;

/// Mint green: lighter and bluer than grass, so it shows over it.
const VALID: Rgb = [0.25, 1.0, 0.65];
/// Yellow-amber: well clear of the invalid red even when blended with grass.
const UNAFFORDABLE: Rgb = [1.0, 0.8, 0.1];
const INVALID: Rgb = [0.95, 0.0, 0.12];
/// The X across an invalid ghost's top: near black, so it stands out on the red.
const CROSS: Rgb = [0.28, 0.0, 0.02];
/// Body opacity per state: the invalid body is denser, so its red is not
/// washed into orange by green or sandy ground showing through.
const VALID_ALPHA: f32 = 0.5;
const UNAFFORDABLE_ALPHA: f32 = 0.45;
const INVALID_ALPHA: f32 = 0.66;
const FRONT_ALPHA: f32 = 0.75;
const CROSS_ALPHA: f32 = 0.95;

/// Whether a placement would be accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhostState {
    Valid,
    /// Placeable, but the player cannot pay for it right now.
    Unaffordable,
    Invalid,
}

impl GhostState {
    /// The state of an af_input ghost. `check_placement` tests the cost
    /// last, and af_input pays a wall line off segment by segment, so a
    /// ghost whose only error is the cost stands on a valid spot.
    pub fn of(ghost: &PlacementGhost) -> GhostState {
        match ghost.error {
            None => GhostState::Valid,
            Some(PlacementError::Rule(Reason::CannotAfford)) => GhostState::Unaffordable,
            Some(_) => GhostState::Invalid,
        }
    }

    pub fn color(self) -> Rgb {
        match self {
            GhostState::Valid => VALID,
            GhostState::Unaffordable => UNAFFORDABLE,
            GhostState::Invalid => INVALID,
        }
    }

    /// Opacity of the ghost's body.
    pub fn alpha(self) -> f32 {
        match self {
            GhostState::Valid => VALID_ALPHA,
            GhostState::Unaffordable => UNAFFORDABLE_ALPHA,
            GhostState::Invalid => INVALID_ALPHA,
        }
    }

    /// Whether the ghost is marked with an X (it cannot be placed at all).
    pub fn crossed(self) -> bool {
        self == GhostState::Invalid
    }
}

/// Marker on ghost entities.
#[derive(Component)]
pub struct GhostPart;

/// One ghost: a root at the spot, a body box, a front-face marker and the
/// two bars of the X shown when the spot is invalid.
#[derive(Clone, Copy, Debug)]
struct Ghost {
    root: Entity,
    body: Entity,
    front: Entity,
    cross: [Entity; 2],
}

/// Ghost entities, reused frame to frame.
#[derive(Resource, Default)]
pub struct GhostPool {
    ghosts: Vec<Ghost>,
}

type GhostQuery<'w, 's> =
    Query<'w, 's, (&'static mut Transform, &'static mut Visibility, Option<&'static mut MeshMaterial3d<StandardMaterial>>), With<GhostPart>>;

fn spawn_ghost(commands: &mut Commands, painter: &mut Painter) -> Ghost {
    let root = commands.spawn((Transform::default(), Visibility::Hidden, GhostPart, RenderScene)).id();
    let mut child = || {
        let look = painter.material(Look::unlit(VALID, VALID_ALPHA));
        commands.spawn((Mesh3d(painter.cube()), MeshMaterial3d(look), Transform::default(), Visibility::Inherited, no_shadows(), GhostPart, ChildOf(root))).id()
    };
    Ghost { root, body: child(), front: child(), cross: [child(), child()] }
}

/// Body and front-face marker transforms (local) for a building of `size`.
fn ghost_boxes(size: BoxSize) -> (Transform, Transform) {
    let [w, h, d] = size;
    let body = Transform::from_xyz(0.0, h * 0.5, 0.0).with_scale(Vec3::new(w, h, d));
    let front = Transform::from_xyz(0.0, h * 0.35, d * 0.5 + 0.06).with_scale(Vec3::new(w * 0.5, h * 0.4, 0.12));
    (body, front)
}

/// The two bars of the X lying on the top of a ghost of `size` (local),
/// corner to corner, slightly inset and thick enough to read from afar.
fn cross_boxes(size: BoxSize) -> [Transform; 2] {
    let [w, h, d] = size;
    let length = (w * w + d * d).sqrt() * 0.86;
    let thickness = (w.min(d) * 0.14).clamp(0.12, 0.6);
    // Yaw that turns a bar's +Z onto the (w, d) diagonal.
    let yaw = w.atan2(d);
    [yaw, -yaw].map(|a| Transform::from_xyz(0.0, h + 0.05, 0.0).with_rotation(Quat::from_rotation_y(a)).with_scale(Vec3::new(thickness, 0.1, length)))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_ghosts(
    mut commands: Commands,
    mut painter: Painter,
    mut pool: ResMut<GhostPool>,
    preview: Res<PlacementPreview>,
    cursor: Res<CursorWorld>,
    data: Res<Data>,
    ground: Ground,
    mut ghosts: GhostQuery,
) {
    let mut shown = 0;
    if !cursor.over_ui {
        for ghost in &preview.ghosts {
            if shown == pool.ghosts.len() {
                let g = spawn_ghost(&mut commands, &mut painter);
                pool.ghosts.push(g);
            }
            let def = data.building(ghost.kind);
            let (state, pos, facing) = (GhostState::of(ghost), ghost.pos, ghost.facing);
            let g = pool.ghosts[shown];
            let (body, front) = ghost_boxes(def.size);
            set_ghost(&mut painter, &mut ghosts, g.root, Some(ground_transform(pos, ground.at(pos), facing)), None);
            set_ghost(&mut painter, &mut ghosts, g.body, Some(body), Some(Look::unlit(state.color(), state.alpha())));
            // Walls have no front worth marking.
            let front = (!def.wall).then_some(front);
            set_ghost(&mut painter, &mut ghosts, g.front, front, Some(Look::unlit(state.color(), FRONT_ALPHA)));
            for (bar, transform) in g.cross.into_iter().zip(cross_boxes(def.size)) {
                set_ghost(&mut painter, &mut ghosts, bar, state.crossed().then_some(transform), Some(Look::unlit(CROSS, CROSS_ALPHA)));
            }
            shown += 1;
        }
    }
    for g in &pool.ghosts[shown..] {
        set_ghost(&mut painter, &mut ghosts, g.root, None, None);
    }
}

/// Show a ghost entity at `transform` (hide it for `None`), recoloured to `look`.
fn set_ghost(painter: &mut Painter, ghosts: &mut GhostQuery, e: Entity, transform: Option<Transform>, look: Option<Look>) {
    let Ok((mut tf, mut vis, mat)) = ghosts.get_mut(e) else { return };
    let Some(transform) = transform else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    vis.set_if_neq(Visibility::Inherited);
    tf.set_if_neq(transform);
    if let (Some(look), Some(mut mat)) = (look, mat) {
        let handle = painter.material(look);
        if mat.0 != handle {
            mat.0 = handle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghost_colours_follow_the_preview_errors() {
        let ghost = |error| PlacementGhost { kind: BuildingTypeId(0), pos: Vec2::ZERO, facing: 0.0, error };
        assert_eq!(GhostState::of(&ghost(None)), GhostState::Valid);
        assert_eq!(GhostState::of(&ghost(Some(PlacementError::Rule(Reason::CannotAfford)))), GhostState::Unaffordable);
        for hard in [PlacementError::Blocked, PlacementError::OutOfBounds, PlacementError::NeedsNode, PlacementError::Rule(Reason::LimitReached), PlacementError::Rule(Reason::Age(3))] {
            assert_eq!(GhostState::of(&ghost(Some(hard))), GhostState::Invalid, "{hard:?}");
        }
        assert_ne!(GhostState::Valid.color(), GhostState::Unaffordable.color());
        assert_ne!(GhostState::Unaffordable.color(), GhostState::Invalid.color());
        assert!(GhostState::Invalid.crossed() && !GhostState::Unaffordable.crossed() && !GhostState::Valid.crossed());
    }

    /// Hue in degrees (0..360) of an sRGB colour.
    fn hue(c: Rgb) -> f32 {
        let (r, g, b) = (c[0], c[1], c[2]);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = max - min;
        if d <= 0.0 {
            return 0.0;
        }
        let h = if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h * 60.0
    }

    /// `c` at `alpha` over `under`, blended in sRGB like the ghost on screen.
    fn over(c: Rgb, alpha: f32, under: Rgb) -> Rgb {
        [0, 1, 2].map(|i| c[i] * alpha + under[i] * (1.0 - alpha))
    }

    /// Degrees between two hues, the short way round.
    fn hue_gap(a: f32, b: f32) -> f32 {
        let d = (a - b).abs() % 360.0;
        d.min(360.0 - d)
    }

    #[test]
    fn invalid_and_unaffordable_read_apart_over_any_ground() {
        // Grass, sand, dirt and rock under the ghost.
        for ground in [[0.42, 0.62, 0.3], [0.8, 0.72, 0.5], [0.45, 0.35, 0.25], [0.5, 0.5, 0.52]] {
            let seen = |s: GhostState| hue(over(s.color(), s.alpha(), ground));
            let (bad, poor, good) = (seen(GhostState::Invalid), seen(GhostState::Unaffordable), seen(GhostState::Valid));
            // Red stays red: orange starts around 20 degrees.
            assert!(hue_gap(bad, 0.0) < 16.0, "invalid reads as hue {bad} over {ground:?}");
            assert!(hue_gap(poor, bad) > 25.0, "unaffordable {poor} too close to invalid {bad} over {ground:?}");
            assert!(hue_gap(good, poor) > 25.0, "valid {good} too close to unaffordable {poor} over {ground:?}");
        }
        // A valid ghost on grass does not melt into it.
        let grass = [0.42, 0.62, 0.3];
        let valid = hue(over(GhostState::Valid.color(), GhostState::Valid.alpha(), grass));
        assert!(hue_gap(valid, hue(grass)) > 30.0, "valid {valid} vs grass {}", hue(grass));
    }

    #[test]
    fn the_cross_lies_on_top_corner_to_corner() {
        let [a, b] = cross_boxes([4.0, 3.0, 2.0]);
        for bar in [a, b] {
            assert!((bar.translation.y - 3.05).abs() < 1e-4, "on the roof");
            // The bar's long axis points along a diagonal of the 4 x 2 top.
            let axis = bar.rotation * Vec3::Z;
            assert!((axis.x.abs() / axis.z.abs() - 2.0).abs() < 1e-3, "{axis}");
            assert!(bar.scale.z < 20.0f32.sqrt(), "inset from the corners");
        }
        assert!((a.rotation * Vec3::Z).x * (b.rotation * Vec3::Z).x < 0.0, "the two bars cross");
        // Thin wall segments still get a visible bar.
        assert!(cross_boxes([2.0, 2.5, 0.4])[0].scale.x >= 0.12);
    }
}
