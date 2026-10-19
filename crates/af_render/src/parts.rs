//! Pure box layouts: which boxes make up a unit, building, node, selection
//! outline or bar. Coordinates are in the owner's local frame: x = right,
//! y = up from the box's base, z = forward (the sim convention), so a
//! weapon `look.offset` can be used as is.
//!
//! Everything here is plain data computed from content defs, so it is unit
//! tested without an app; `entities.rs` turns the layouts into entities.

use af_core::prelude::*;
use bevy::prelude::*;

use crate::look::{BLACK, Look, lighten, mix, scale};
use crate::motion::WeaponAnim;

/// How strongly a unit's accent colour tints the owner's colour on its body.
pub const ACCENT_MIX: f32 = 0.35;
/// Stone grey that buildings without their own colour are mixed toward.
const STONE: Rgb = [0.62, 0.6, 0.56];
/// Bark brown mixed into tree trunks.
const BARK: Rgb = [0.36, 0.25, 0.15];
/// Leaf green mixed into berry bushes.
const LEAF: Rgb = [0.2, 0.42, 0.17];
/// Charred rock of fallen-star (adamant) nodes.
const METEOR_ROCK: Rgb = [0.16, 0.14, 0.15];
/// Scaffolding on foundations.
const SCAFFOLD: Rgb = [0.85, 0.7, 0.45];
/// Energy shields.
pub const SHIELD: Rgb = [0.55, 0.8, 1.0];
/// Shield opacity when idle and right after a hit.
pub const SHIELD_ALPHA: (f32, f32) = (0.12, 0.45);
/// Shadow boxes under flyers.
const SHADOW_ALPHA: f32 = 0.3;
/// Units flying higher than this (metres) cast no sun shadow: their shadow
/// box marks the ground under them instead.
pub const HIGH_FLYER_ALTITUDE: f32 = 2.0;

/// One box: centre and full size in the parent's local frame, its look,
/// and whether it casts shadows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub center: Vec3,
    pub size: Vec3,
    pub look: Look,
    pub shadow: bool,
}

impl Part {
    pub fn new(center: Vec3, size: Vec3, look: Look) -> Part {
        Part { center, size: size.max(Vec3::splat(0.01)), look, shadow: true }
    }

    /// A box centred on (`x`, `z`) spanning heights `y0..y1`.
    pub fn column(x: f32, z: f32, width: f32, depth: f32, y0: f32, y1: f32, look: Look) -> Part {
        Part::new(Vec3::new(x, (y0 + y1) * 0.5, z), Vec3::new(width, y1 - y0, depth), look)
    }

    pub fn no_shadow(mut self) -> Part {
        self.shadow = false;
        self
    }

    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.center).with_scale(self.size)
    }

    /// Lowest and highest local height covered.
    pub fn y_range(&self) -> (f32, f32) {
        (self.center.y - self.size.y * 0.5, self.center.y + self.size.y * 0.5)
    }
}

/// A weapon box on a pivot: the pivot sits at `pivot` and animates; the box
/// is `part` in the pivot's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponMount {
    /// Index into `Weapons::slots`.
    pub slot: usize,
    pub pivot: Vec3,
    pub part: Part,
    pub anim: WeaponAnim,
}

/// The pivot and box for a weapon's `look` on a building (`on_building`)
/// or a unit. Swings and smashes pivot at the grip (the rear end of the
/// box) so the box sweeps; the rest pivot at the box centre. At rest the
/// box centre is exactly `look.offset`.
pub fn weapon_mount(slot: usize, def: &WeaponDef, on_building: bool) -> WeaponMount {
    let size = Vec3::from(def.look.size).max(Vec3::splat(0.02));
    let center = Vec3::from(def.look.offset);
    let anim = WeaponAnim::of(def, on_building);
    let (pivot, local) = if anim.pivots_at_grip() { (center - Vec3::Z * size.z * 0.5, Vec3::Z * size.z * 0.5) } else { (center, Vec3::ZERO) };
    WeaponMount { slot, pivot, part: Part::new(local, size, Look::lit(def.look.color)), anim }
}

fn weapon_mounts(data: &GameData, weapons: &[WeaponId], on_building: bool) -> Vec<WeaponMount> {
    weapons.iter().enumerate().map(|(i, w)| weapon_mount(i, data.weapon(*w), on_building)).collect()
}

/// The boxes of one unit.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitLayout {
    /// Static boxes (base, body, facing accent, wings/treads).
    pub parts: Vec<Part>,
    pub weapons: Vec<WeaponMount>,
    /// Load on the back of a gatherer (recoloured and toggled at runtime).
    pub cargo: Option<Part>,
    /// Translucent shell around a shielded unit.
    pub shield: Option<Part>,
    /// Flat dark box on the ground under a flyer / hover unit (its local y
    /// follows the ground at runtime: see [`shadow_y`]).
    pub shadow: Option<Part>,
}

/// Per-entity facts a unit layout depends on beyond its def.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UnitTraits {
    pub gatherer: bool,
    pub shield: bool,
    pub altitude: f32,
}

/// Body colour of a unit: the owner's colour tinted by the def's accent.
pub fn unit_body_color(def: &UnitDef, owner: Rgb) -> Rgb {
    def.color.map_or(owner, |accent| mix(owner, accent, ACCENT_MIX))
}

/// Local height of the shadow box of an entity drawn with its base at
/// `base_y` over ground at `ground`.
pub fn shadow_y(ground: f32, base_y: f32) -> f32 {
    ground - base_y + 0.04
}

/// The boxes of a unit. Everything but its weapons (placed by the content's
/// `look.offset`) and its shield shell stays inside its footprint, so units
/// whose footprints touch never draw into each other.
pub fn unit_layout(data: &GameData, def: &UnitDef, owner: Rgb, traits: UnitTraits) -> UnitLayout {
    let [w, h, d] = def.size.map(|v| v.max(0.05));
    let body = unit_body_color(def, owner);
    let dark = scale(body, 0.5);
    let mut parts = Vec::with_capacity(6);
    // Locomotion decides the lower part, where the body starts and its
    // width and depth.
    let (body_y0, body_w, body_d) = match def.locomotion {
        MoveFlag::Wheeled | MoveFlag::Tracked => {
            let tread_h = h * 0.28;
            let tread_w = w * 0.24;
            let x = (w - tread_w) * 0.5;
            let tread = Look::lit(scale(dark, 0.6));
            parts.push(Part::column(-x, 0.0, tread_w, d, 0.0, tread_h, tread));
            parts.push(Part::column(x, 0.0, tread_w, d, 0.0, tread_h, tread));
            (tread_h, w * 0.94, d * 0.9)
        }
        MoveFlag::Hover => {
            let skirt_h = h * 0.1;
            parts.push(Part::column(0.0, 0.0, w * 0.9, d * 0.9, 0.0, skirt_h, Look::glow(lighten(body, 0.5))));
            (skirt_h, w, d * 0.92)
        }
        MoveFlag::Flying => {
            // A slim fuselage with a wing either side reaching the
            // footprint's edge, never through the fuselage.
            let base_h = h * 0.15;
            let body_w = w * 0.56;
            parts.push(Part::column(0.0, 0.0, body_w, d, 0.0, base_h, Look::lit(dark)));
            let span = (w - body_w) * 0.5;
            let x = (body_w + span) * 0.5;
            let wing = Look::lit(scale(body, 0.75));
            let (y0, y1) = (h * 0.45, h * 0.53);
            parts.push(Part::column(-x, 0.0, span, d * 0.4, y0, y1, wing));
            parts.push(Part::column(x, 0.0, span, d * 0.4, y0, y1, wing));
            (base_h, body_w, d * 0.92)
        }
        _ => {
            let base_h = h * 0.18;
            parts.push(Part::column(0.0, 0.0, w, d, 0.0, base_h, Look::lit(dark)));
            (base_h, w * 0.92, d * 0.92)
        }
    };
    // A gatherer's body is slimmer front to back, leaving room behind it
    // for its load.
    let pack = if traits.gatherer { (d * 0.12).clamp(0.04, 0.3) } else { 0.0 };
    let body_d = body_d.min(d - 2.0 * pack);
    parts.push(Part::column(0.0, 0.0, body_w, body_d, body_y0, h, Look::lit(body)));
    // Facing marker on the front of the body near the top, in the room
    // left between the body and the footprint's front edge.
    let room = ((d - body_d) * 0.5).max(0.0);
    let accent = lighten(def.color.unwrap_or(owner), 0.45);
    let t = room.min((d * 0.1).clamp(0.04, 0.2));
    parts.push(Part::new(Vec3::new(0.0, h * 0.8, body_d * 0.5 + t * 0.5), Vec3::new(w * 0.5, h * 0.14, t), Look::lit(accent)));

    let cargo = traits.gatherer.then(|| {
        Part::new(Vec3::new(0.0, h * 0.55, -(body_d * 0.5 + pack * 0.5)), Vec3::new(w * 0.5, h * 0.28, pack), Look::lit(body))
    });
    let shield = traits.shield.then(|| {
        let size = Vec3::new(w * 1.15 + 0.15, h * 1.08 + 0.1, d * 1.15 + 0.15);
        Part::new(Vec3::new(0.0, h * 0.5, 0.0), size, Look::unlit(SHIELD, SHIELD_ALPHA.0)).no_shadow()
    });
    let shadow = (traits.altitude > 0.05).then(|| {
        Part::new(Vec3::new(0.0, shadow_y(0.0, traits.altitude), 0.0), Vec3::new(w * 0.95, 0.02, d * 0.95), Look::unlit(BLACK, SHADOW_ALPHA)).no_shadow()
    });
    let mut weapons = weapon_mounts(data, &def.weapon_ids, false);
    let mut cargo = cargo;
    // High flyers show where they are by the box straight under them; a
    // sun-cast shadow as well would put a second one off to the side.
    if traits.altitude > HIGH_FLYER_ALTITUDE {
        for p in parts.iter_mut().chain(cargo.iter_mut()).chain(weapons.iter_mut().map(|m| &mut m.part)) {
            p.shadow = false;
        }
    }
    UnitLayout { parts, weapons, cargo, shield, shadow }
}

/// The boxes of one building.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildingLayout {
    pub parts: Vec<Part>,
    pub weapons: Vec<WeaponMount>,
    /// Foundations: the built-so-far box at full height; scale it with
    /// [`progress_transform`].
    pub progress: Option<Part>,
    /// Translucent shell for shielded buildings.
    pub shield: Option<Part>,
}

/// Wall/roof colour of a building: its own colour, or the owner's mixed
/// toward stone.
pub fn building_body_color(def: &BuildingDef, owner: Rgb) -> Rgb {
    def.color.unwrap_or_else(|| mix(owner, STONE, 0.5))
}

/// Layout of a building, or of its foundation when `progress` is `Some`.
/// Everything but weapons and the shield shell stays inside its footprint.
pub fn building_layout(data: &GameData, def: &BuildingDef, owner: Rgb, progress: Option<f32>, shield: bool) -> BuildingLayout {
    let [w, h, d] = def.size.map(|v| v.max(0.05));
    let body = building_body_color(def, owner);
    let trim = scale(mix(body, owner, 0.5), 0.8);
    let mut parts = Vec::with_capacity(6);
    let shield = shield.then(|| {
        let size = Vec3::new(w + 0.6, h + 0.5, d + 0.6);
        Part::new(Vec3::new(0.0, size.y * 0.5, 0.0), size, Look::unlit(SHIELD, SHIELD_ALPHA.0)).no_shadow()
    });
    if progress.is_some() {
        let pad_h = 0.06;
        parts.push(Part::column(0.0, 0.0, w, d, 0.0, pad_h, Look::lit(scale(body, 0.5))).no_shadow());
        parts.push(Part::column(0.0, 0.0, w, d, 0.0, h, Look::unlit(SCAFFOLD, 0.22)).no_shadow());
        let built = Part::column(0.0, 0.0, w * 0.94, d * 0.94, pad_h, h, Look::lit(body));
        return BuildingLayout { parts, weapons: Vec::new(), progress: Some(built), shield };
    }
    if def.gate {
        let post_w = (w * 0.14).clamp(0.3, 1.0);
        let x = (w - post_w) * 0.5;
        let span = w - 2.0 * post_w;
        let post = Look::lit(scale(body, 0.8));
        for side in [-x, x] {
            parts.push(Part::column(side, 0.0, post_w, d, 0.0, h * 1.15, post));
            parts.push(Part::column(side, 0.0, post_w, d, h * 1.15, h * 1.15 + 0.15, Look::lit(owner)));
        }
        parts.push(Part::column(0.0, 0.0, span, d * 0.45, 0.0, h * 0.8, Look::lit(scale(mix(body, owner, 0.5), 0.7))));
        parts.push(Part::column(0.0, 0.0, span, d, h * 0.84, h, Look::lit(body)));
    } else if def.wall {
        parts.push(Part::column(0.0, 0.0, w, d, 0.0, h, Look::lit(body)));
        parts.push(Part::column(0.0, 0.0, w, d * 0.8, h, h + 0.14, Look::lit(owner)));
    } else if h < 1.0 {
        // Flat (fields): a slab with an owner-coloured kerb at the front.
        parts.push(Part::column(0.0, 0.0, w, d * 0.94, 0.0, h, Look::lit(body)));
        parts.push(Part::column(0.0, d * 0.47 + d * 0.015, w, d * 0.03, 0.0, h * 1.5, Look::lit(owner)));
    } else {
        // The walls stop short of the footprint's front and back, so the
        // door stands proud of the front wall without leaving the footprint.
        let wall_h = h * 0.72;
        let door_d = (d * 0.06).min(0.12);
        let wall_d = d - 2.0 * door_d;
        parts.push(Part::column(0.0, 0.0, w, wall_d, 0.0, wall_h, Look::lit(body)));
        parts.push(Part::column(0.0, 0.0, w * 0.86, wall_d * 0.86, wall_h, h, Look::lit(trim)));
        let door_w = (w * 0.22).clamp(0.5, 2.2);
        let door_h = (wall_h * 0.6).min(2.4);
        parts.push(Part::new(Vec3::new(0.0, door_h * 0.5, (d - door_d) * 0.5), Vec3::new(door_w, door_h, door_d), Look::lit(scale(body, 0.35))));
    }
    BuildingLayout { parts, weapons: weapon_mounts(data, &def.weapon_ids, true), progress: None, shield }
}

/// Transform of a foundation's built part at `progress` (0..1): the full
/// box squashed to a progress-scaled height, standing on its base.
pub fn progress_transform(full: &Part, progress: f32) -> Transform {
    let (y0, y1) = full.y_range();
    let height = (y1 - y0) * progress.clamp(0.03, 1.0);
    Transform::from_translation(Vec3::new(full.center.x, y0 + height * 0.5, full.center.z)).with_scale(Vec3::new(full.size.x, height, full.size.z))
}

/// What a resource node is drawn as, by its resource and shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeShape {
    /// Wood, tall and slim: trunk and a two-tier canopy.
    TallTree,
    /// Wood: trunk and canopy.
    Tree,
    /// Food: a bush with berries.
    Bush,
    /// Aether that needs a building: a glowing pad.
    Vent,
    /// Aether: a glowing crystal with shards.
    Crystal,
    /// Adamant: a charred meteor with glowing ore.
    Meteor,
    /// Anything else: an ore heap with chunks.
    Heap,
}

impl NodeShape {
    fn of(data: &GameData, def: &NodeDef) -> NodeShape {
        let [w, h, d] = def.size.map(|v| v.max(0.05));
        match data.resource(def.resource_id).id.as_str() {
            "wood" if h / w.max(d) > 5.0 => NodeShape::TallTree,
            "wood" => NodeShape::Tree,
            "food" => NodeShape::Bush,
            "aether" if def.requires_building.is_some() => NodeShape::Vent,
            "aether" => NodeShape::Crystal,
            "adamant" => NodeShape::Meteor,
            _ => NodeShape::Heap,
        }
    }

    /// The share of the footprint's width and depth its base box covers.
    fn base(self) -> f32 {
        match self {
            NodeShape::TallTree => 0.3,
            NodeShape::Tree => 0.35,
            NodeShape::Bush | NodeShape::Vent => 1.0,
            NodeShape::Crystal => 0.55,
            NodeShape::Meteor => 0.85,
            NodeShape::Heap => 0.8,
        }
    }
}

/// Half extents (x, z) of the box a node stands on, the trunk, bush,
/// crystal or rock a worker works at, at full size (scale it by
/// [`node_scale`]): the first part of [`node_layout`].
pub fn node_base_half(data: &GameData, def: &NodeDef) -> Vec2 {
    let [w, _, d] = def.size.map(|v| v.max(0.05));
    Vec2::new(w, d) * NodeShape::of(data, def).base() * 0.5
}

/// The boxes of a resource node, chosen by the node's resource and shape:
/// wood is a tree (trunk + canopy, two-tier when tall and slim), food a
/// bush with berries, aether a glowing crystal (a glowing pad for vents
/// that need a building), adamant a charred meteor with glowing ore,
/// anything else an ore heap with chunks. The first part is the base
/// ([`node_base_half`]).
pub fn node_layout(data: &GameData, def: &NodeDef) -> Vec<Part> {
    let [w, h, d] = def.size.map(|v| v.max(0.05));
    let c = def.color;
    let shape = NodeShape::of(data, def);
    let (bw, bd) = (w * shape.base(), d * shape.base());
    match shape {
        NodeShape::TallTree | NodeShape::Tree => {
            let trunk = Look::lit(mix(c, BARK, 0.8));
            if shape == NodeShape::TallTree {
                vec![
                    Part::column(0.0, 0.0, bw, bd, 0.0, h * 0.35, trunk),
                    Part::column(0.0, 0.0, w, d, h * 0.35, h * 0.7, Look::lit(c)),
                    Part::column(0.0, 0.0, w * 0.6, d * 0.6, h * 0.7, h, Look::lit(lighten(c, 0.08))),
                ]
            } else {
                vec![Part::column(0.0, 0.0, bw, bd, 0.0, h * 0.4, trunk), Part::column(0.0, 0.0, w, d, h * 0.4, h, Look::lit(c))]
            }
        }
        NodeShape::Bush => {
            let top = h * 0.7;
            let s = w.min(d) * 0.2;
            let berry = Look::lit(c);
            let mut parts = vec![Part::column(0.0, 0.0, bw, bd, 0.0, top, Look::lit(mix(c, LEAF, 0.7)))];
            for (x, z) in [(-0.25, -0.2), (0.25, -0.15), (0.0, 0.25)] {
                parts.push(Part::column(x * w, z * d, s, s, top, top + s, berry));
            }
            parts
        }
        NodeShape::Vent => vec![
            Part::column(0.0, 0.0, bw, bd, 0.0, h, Look::glow(scale(c, 0.7))),
            Part::column(0.0, 0.0, w * 0.4, d * 0.4, h, h * 1.6, Look::glow(lighten(c, 0.3))),
        ],
        NodeShape::Crystal => {
            let shard = Look::glow(lighten(c, 0.2));
            vec![
                Part::column(0.0, 0.0, bw, bd, 0.0, h, Look::glow(c)),
                Part::column(-0.39 * w, 0.2 * d, w * 0.22, d * 0.22, 0.0, h * 0.5, shard),
                Part::column(0.39 * w, -0.2 * d, w * 0.22, d * 0.22, 0.0, h * 0.6, shard),
            ]
        }
        NodeShape::Meteor => {
            // A fallen star: a charred rock with glowing ore breaking out of
            // its top (a heap in the ore's own colour would read as sand).
            let top = h * 0.7;
            let ore = Look::glow(c);
            vec![
                Part::column(0.0, 0.0, bw, bd, 0.0, top, Look::lit(mix(c, METEOR_ROCK, 0.8))),
                Part::column(0.12 * w, -0.08 * d, w * 0.34, d * 0.34, top, h, ore),
                Part::column(-0.22 * w, 0.18 * d, w * 0.2, d * 0.2, top, top + h * 0.15, ore),
            ]
        }
        NodeShape::Heap => {
            let top = h * 0.75;
            vec![
                Part::column(0.0, 0.0, bw, bd, 0.0, top, Look::lit(c)),
                Part::column(0.15 * w, -0.1 * d, w * 0.3, d * 0.3, top, h, Look::lit(lighten(c, 0.2))),
                Part::column(-0.2 * w, 0.15 * d, w * 0.22, d * 0.22, top, top + h * 0.2, Look::lit(scale(c, 0.8))),
            ]
        }
    }
}

/// Uniform scale of a node's boxes for the fraction of its resource left:
/// full size when untouched, 70% when nearly gone.
pub fn node_scale(remaining: f32, amount: f32) -> f32 {
    let f = if amount > 0.0 { (remaining / amount).clamp(0.0, 1.0) } else { 1.0 };
    0.7 + 0.3 * f
}

/// Thinnest drawn width of a bar or selection outline per metre of camera
/// distance (about 2.5 px at any zoom; see [`min_line_width`]).
const LINE_WIDTH_PER_DISTANCE: f32 = 0.0016;

/// The thinnest (metres) bars and selection outlines are drawn with the
/// camera `camera_distance` metres from its focus, so they stay readable
/// when zoomed out.
pub fn min_line_width(camera_distance: f32) -> f32 {
    camera_distance.max(0.0) * LINE_WIDTH_PER_DISTANCE
}

/// The four flat boxes of a selection outline around a footprint with half
/// extents `half`, at local height `y`, sides at least `min_width` wide:
/// (centre, size) for front, back, left, right. They touch at the corners
/// and never overlap.
pub fn outline_boxes(half: Vec2, y: f32, min_width: f32) -> [(Vec3, Vec3); 4] {
    let small = half.min_element();
    let t = (small * 0.15).clamp(0.06, 0.25).max(min_width);
    let m = (small * 0.15).clamp(0.08, 0.4);
    let (ox, oz) = (half.x + m, half.y + m);
    let h = 0.04;
    [
        (Vec3::new(0.0, y, oz + t * 0.5), Vec3::new(2.0 * (ox + t), h, t)),
        (Vec3::new(0.0, y, -(oz + t * 0.5)), Vec3::new(2.0 * (ox + t), h, t)),
        (Vec3::new(-(ox + t * 0.5), y, 0.0), Vec3::new(t, h, 2.0 * oz)),
        (Vec3::new(ox + t * 0.5, y, 0.0), Vec3::new(t, h, 2.0 * oz)),
    ]
}

/// Geometry of an entity's status bars. The bar root sits `y` above the
/// entity's base (straight up in the world) and turns to face the camera;
/// rows stack along the root's (camera-facing) up axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarSpec {
    /// Full bar width (metres).
    pub width: f32,
    /// Thickness of one bar.
    pub height: f32,
    /// Height of the bar root above the entity's base.
    pub y: f32,
}

impl BarSpec {
    /// Bars above a box of footprint `fp`, at least `min_height` thick
    /// (see [`min_line_width`]).
    pub fn for_footprint(fp: &Footprint, min_height: f32) -> BarSpec {
        let width = (fp.half.x.max(fp.half.y) * 2.0 * 1.1).clamp(0.8, 7.0);
        let height = (width * 0.05).clamp(0.09, 0.22).max(min_height);
        BarSpec { width, height, y: fp.height + 0.35 + width * 0.08 }
    }

    /// The bar root's local transform before it is turned to the camera.
    pub fn root(&self) -> Transform {
        Transform::from_xyz(0.0, self.y, 0.0)
    }

    /// Offset of bar row `row` from the root (0 = health, 1 = above it, -1 = below).
    pub fn row_y(&self, row: f32) -> f32 {
        row * self.height * 1.6
    }

    /// Background box of a row (slightly larger than the fill, behind it).
    pub fn background(&self, row: f32) -> Transform {
        Transform::from_xyz(0.0, self.row_y(row), 0.0).with_scale(Vec3::new(self.width + 0.06, self.height + 0.06, 0.03))
    }

    /// Fill box of a row at `fraction` full, left-aligned, in front of its background.
    pub fn fill(&self, row: f32, fraction: f32) -> Transform {
        let f = fraction.clamp(0.0, 1.0);
        let w = (self.width * f).max(0.001);
        Transform::from_xyz(-(self.width - w) * 0.5, self.row_y(row), 0.03).with_scale(Vec3::new(w, self.height, 0.03))
    }
}

/// A thin box from `a` to `b` (rally lines, wall previews): centred
/// between them, its +Z along the segment, `width` x `height` across.
pub fn segment_transform(a: Vec3, b: Vec3, width: f32, height: f32) -> Transform {
    let d = b - a;
    let len = d.length().max(0.01);
    Transform::from_translation((a + b) * 0.5).with_rotation(crate::motion::rotation_for_dir(d)).with_scale(Vec3::new(width, height, len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::test_data;

    fn overlap(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> bool {
        let (amin, amax) = (a.0 - a.1 * 0.5, a.0 + a.1 * 0.5);
        let (bmin, bmax) = (b.0 - b.1 * 0.5, b.0 + b.1 * 0.5);
        (0..3).all(|i| amin[i] < bmax[i] - 1e-5 && bmin[i] < amax[i] - 1e-5)
    }

    /// Does `part` stay within the footprint of a box of `size`?
    fn inside(part: &Part, size: BoxSize) -> bool {
        part.center.x.abs() + part.size.x * 0.5 <= size[0] * 0.5 + 1e-4 && part.center.z.abs() + part.size.z * 0.5 <= size[2] * 0.5 + 1e-4
    }

    #[test]
    fn every_unit_has_a_body_accent_and_weapons() {
        let data = test_data();
        let owner = [0.2, 0.3, 0.9];
        for def in &data.units {
            let l = unit_layout(&data, def, owner, UnitTraits { gatherer: def.gatherer.is_some(), shield: def.shield.is_some(), altitude: def.altitude });
            assert!(l.parts.len() >= 3, "{}", def.id);
            assert_eq!(l.weapons.len(), def.weapon_ids.len(), "{}", def.id);
            assert_eq!(l.cargo.is_some(), def.gatherer.is_some());
            assert_eq!(l.shield.is_some(), def.shield.is_some());
            assert_eq!(l.shadow.is_some(), def.altitude > 0.05, "{}", def.id);
            // Static parts never intersect each other, and the body reaches the top.
            for (i, a) in l.parts.iter().enumerate() {
                for b in &l.parts[i + 1..] {
                    assert!(!overlap((a.center, a.size), (b.center, b.size)), "{}: {a:?} vs {b:?}", def.id);
                }
            }
            let top = l.parts.iter().map(|p| p.y_range().1).fold(0.0, f32::max);
            assert!((top - def.size[1]).abs() < 1e-4, "{} top {top}", def.id);
            // The accent sits in front of the centre so facing reads.
            let accent = l.parts.last().unwrap();
            assert!(accent.center.z > 0.0, "{}", def.id);
            // Nothing but weapons and the shield shell leaves the footprint,
            // so touching units never draw into each other.
            for p in l.parts.iter().chain(&l.cargo).chain(&l.shadow) {
                assert!(inside(p, def.size), "{} stays in its footprint: {p:?}", def.id);
            }
            if let Some(cargo) = &l.cargo {
                assert!(l.parts.iter().all(|p| !overlap((p.center, p.size), (cargo.center, cargo.size))), "{}: load clear of the body", def.id);
                assert!(cargo.center.z < 0.0, "{}: load on the back", def.id);
            }
        }
    }

    #[test]
    fn flyer_wings_stay_beside_the_fuselage() {
        let data = test_data();
        let def = data.units.iter().find(|u| u.locomotion == MoveFlag::Flying).expect("content has a flyer");
        let l = unit_layout(&data, def, [0.5; 3], UnitTraits { altitude: def.altitude, ..default() });
        let half_w = def.size[0] * 0.5;
        let reach = l.parts.iter().map(|p| p.center.x.abs() + p.size.x * 0.5).fold(0.0, f32::max);
        assert!((reach - half_w).abs() < 1e-4, "wings reach the footprint's edge and no further: {reach} vs {half_w}");
    }

    #[test]
    fn only_their_ground_box_shadows_high_flyers() {
        let data = test_data();
        let mut seen = [false; 2];
        for def in &data.units {
            let l = unit_layout(&data, def, [0.5; 3], UnitTraits { altitude: def.altitude, ..default() });
            let casts = l.parts.iter().chain(l.weapons.iter().map(|m| &m.part)).any(|p| p.shadow);
            let high = def.altitude > HIGH_FLYER_ALTITUDE;
            assert_eq!(casts, !high, "{}", def.id);
            assert_eq!(l.shadow.is_some(), def.altitude > 0.05, "{}", def.id);
            seen[high as usize] = true;
        }
        assert_eq!(seen, [true; 2], "the content has both ground units and high flyers");
    }

    #[test]
    fn unit_colours_mix_owner_and_accent() {
        let data = test_data();
        let owner = [0.0, 0.0, 1.0];
        let with = data.units.iter().find(|u| u.color.is_some()).expect("some unit has an accent");
        let without = data.units.iter().find(|u| u.color.is_none()).expect("some unit has none");
        assert_eq!(unit_body_color(without, owner), owner);
        assert_eq!(unit_body_color(with, owner), mix(owner, with.color.unwrap(), ACCENT_MIX));
    }

    #[test]
    fn weapon_boxes_rest_at_their_offset() {
        let data = test_data();
        for (i, def) in data.weapons.iter().enumerate() {
            for on_building in [false, true] {
                let m = weapon_mount(i, def, on_building);
                let rest = m.pivot + m.part.center;
                assert!((rest - Vec3::from(def.look.offset)).length() < 1e-5, "{}", def.id);
                assert_eq!(m.part.size, Vec3::from(def.look.size).max(Vec3::splat(0.02)));
                assert_eq!(m.anim, WeaponAnim::of(def, on_building));
            }
        }
    }

    #[test]
    fn buildings_walls_gates_and_foundations() {
        let data = test_data();
        let owner = [0.9, 0.1, 0.1];
        let mut kinds = [false; 4];
        for def in &data.buildings {
            let l = building_layout(&data, def, owner, None, false);
            assert!(!l.parts.is_empty() && l.progress.is_none(), "{}", def.id);
            assert_eq!(l.weapons.len(), def.weapon_ids.len());
            let top = l.parts.iter().map(|p| p.y_range().1).fold(0.0, f32::max);
            assert!(top >= def.size[1] - 1e-4, "{} reaches its height", def.id);
            assert!(l.parts.iter().all(|p| inside(p, def.size)), "{} stays in its footprint", def.id);
            if def.gate {
                kinds[0] = true;
                let wall_like = building_layout(&data, &BuildingDef { gate: false, ..def.clone() }, owner, None, false);
                assert_ne!(l.parts, wall_like.parts, "gates look different from walls");
            } else if def.wall {
                kinds[1] = true;
                assert!(l.parts.len() <= 2, "walls are plain");
            } else if def.size[1] >= 1.0 {
                kinds[2] = true;
                // A door standing on the ground against the front wall, flush with the footprint's front.
                let front = l.parts[0].center.z + l.parts[0].size.z * 0.5;
                let door = l.parts.iter().any(|p| {
                    let (z0, z1) = (p.center.z - p.size.z * 0.5, p.center.z + p.size.z * 0.5);
                    (z0 - front).abs() < 1e-4 && (z1 - def.size[2] * 0.5).abs() < 1e-4 && p.y_range().0.abs() < 1e-4
                });
                assert!(door, "{} has a door", def.id);
            }
            let f = building_layout(&data, def, owner, Some(0.5), false);
            assert!(f.parts.iter().chain(&f.progress).all(|p| inside(p, def.size)), "{} foundation stays in its footprint", def.id);
            let built = f.progress.expect("foundations have a progress box");
            assert!(f.weapons.is_empty());
            let t = progress_transform(&built, 0.5);
            let full = built.y_range().1 - built.y_range().0;
            assert!((t.scale.y - full * 0.5).abs() < 1e-4);
            assert!((t.translation.y - t.scale.y * 0.5 - built.y_range().0).abs() < 1e-4, "stands on its base");
            kinds[3] = true;
        }
        assert_eq!(kinds, [true; 4]);
    }

    #[test]
    fn nodes_have_shapes_by_resource() {
        let data = test_data();
        for def in &data.nodes {
            let parts = node_layout(&data, def);
            assert!(parts.len() >= 2, "{}", def.id);
            let res = data.resource(def.resource_id).id.as_str();
            let glows = parts.iter().any(|p| p.look.shade == crate::look::Shade::Glow);
            assert_eq!(glows, res == "aether" || res == "adamant", "{}", def.id);
            for p in &parts {
                assert!(p.center.x.abs() + p.size.x * 0.5 <= def.size[0] * 0.5 + 1e-4, "{} stays in its footprint", def.id);
                assert!(p.center.z.abs() + p.size.z * 0.5 <= def.size[2] * 0.5 + 1e-4, "{} stays in its footprint", def.id);
            }
            // The base box workers work at is the first part, centred on the node.
            let base = &parts[0];
            assert_eq!((base.center.x, base.center.z), (0.0, 0.0), "{}", def.id);
            assert!((base.size.xz() * 0.5 - node_base_half(&data, def)).length() < 1e-5, "{}", def.id);
        }
        assert_eq!(node_scale(100.0, 100.0), 1.0);
        assert!((node_scale(0.0, 100.0) - 0.7).abs() < 1e-6);
        assert!(node_scale(50.0, 100.0) < 1.0 && node_scale(50.0, 100.0) > 0.7);
    }

    #[test]
    fn outline_surrounds_the_footprint_without_overlap() {
        let half = Vec2::new(1.5, 0.6);
        let boxes = outline_boxes(half, 0.05, 0.0);
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                assert!(!overlap(*a, *b), "{a:?} {b:?}");
            }
            // Every side lies outside the footprint.
            let (min, max) = (a.0 - a.1 * 0.5, a.0 + a.1 * 0.5);
            assert!(min.x >= half.x || max.x <= -half.x || min.z >= half.y || max.z <= -half.y);
        }
    }

    #[test]
    fn bars_and_outlines_keep_a_least_width_when_zoomed_out() {
        let half = Vec2::splat(0.3);
        let close = outline_boxes(half, 0.0, min_line_width(25.0));
        let far = outline_boxes(half, 0.0, min_line_width(150.0));
        assert!(far[0].1.z > close[0].1.z, "sides widen with the camera distance");
        assert!((far[0].1.z - min_line_width(150.0)).abs() < 1e-6);
        let fp = Footprint { half, height: 1.0 };
        assert!(BarSpec::for_footprint(&fp, min_line_width(150.0)).height > BarSpec::for_footprint(&fp, 0.0).height);
        assert_eq!(min_line_width(-3.0), 0.0);
    }

    #[test]
    fn bar_fill_is_left_aligned() {
        let spec = BarSpec { width: 2.0, height: 0.1, y: 3.0 };
        let full = spec.fill(0.0, 1.0);
        assert!((full.scale.x - 2.0).abs() < 1e-6 && full.translation.x.abs() < 1e-6);
        let half = spec.fill(0.0, 0.5);
        assert!((half.scale.x - 1.0).abs() < 1e-6);
        assert!((half.translation.x - half.scale.x * 0.5 - (-1.0)).abs() < 1e-6, "left edge stays put");
        assert!(spec.row_y(1.0) > spec.row_y(0.0) && spec.row_y(-1.0) < spec.row_y(0.0));
        let fp = Footprint { half: Vec2::new(0.3, 0.3), height: 1.2 };
        let s = BarSpec::for_footprint(&fp, 0.0);
        assert!(s.y > fp.height && s.width >= 0.8);
        assert_eq!(s.root().translation, Vec3::Y * s.y, "bars sit straight above the entity");
    }

    #[test]
    fn segments_span_their_endpoints() {
        let a = Vec3::new(1.0, 0.1, 1.0);
        let b = Vec3::new(4.0, 0.1, 5.0);
        let t = segment_transform(a, b, 0.1, 0.05);
        assert!((t.scale.z - 5.0).abs() < 1e-5);
        let end = t.transform_point(Vec3::new(0.0, 0.0, 0.5));
        assert!((end - b).length() < 1e-4, "{end:?}");
    }
}
