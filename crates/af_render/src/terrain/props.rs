//! Decoration grown from the terrain: brambles over thickets, reeds along
//! shores and in marsh, scree and rock spurs at mountain feet and highland
//! edges, charred meteor fragments in the meteor sites and the crystal
//! Starspire. Placed from hashes of the cell (the same for every player,
//! never touching the simulation), only on ground nothing can be built on,
//! and merged into one flat-shaded, vertex-coloured mesh per chunk.
//!
//! Nothing here may look like something a worker gathers (the style
//! guide's node colours): no round grey boulder piles (stone), no red
//! berries (berry bushes), no bright violet crystals (aether), no gold in
//! charred rock (adamant), no free-standing trees (wood).

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::field::{Cover, Field};
use super::mesh::ChunkRect;
use super::noise::cell_hash;

/// A merged prop mesh under construction: rock and crystal faces flat
/// (their own three vertices), bushes smooth (shared vertices). The
/// material draws both sides, so blades are single triangles. Vertex colour
/// alpha is how much the colour glows.
#[derive(Default)]
pub struct PropMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// An sRGB palette colour as linear vertex colour with a glow amount.
fn lin(srgb: u32, glow: f32) -> [f32; 4] {
    let c = Color::srgb_u8((srgb >> 16) as u8, (srgb >> 8) as u8, srgb as u8).to_linear();
    [c.red, c.green, c.blue, glow]
}

fn scale_rgb(c: [f32; 4], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

fn mix_rgb(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

/// A tiny deterministic random stream for one prop.
struct Dice(u32);

impl Dice {
    fn new(x: i32, z: i32, salt: u32) -> Dice {
        Dice(crate::look::hash32((x as u32).wrapping_mul(0x27d4_eb2d) ^ (z as u32).wrapping_mul(0x1656_67b1) ^ salt))
    }

    /// Next value in 0..1.
    fn next(&mut self) -> f32 {
        self.0 = crate::look::hash32(self.0.wrapping_add(0x9e37_79b9));
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

impl PropMesh {
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    fn vertex(&mut self, p: Vec3, n: Vec3, color: [f32; 4]) -> u32 {
        self.positions.push(p.to_array());
        self.normals.push(n.to_array());
        self.colors.push(color);
        (self.positions.len() - 1) as u32
    }

    /// One flat triangle (counter-clockwise seen from its front), one colour
    /// per corner.
    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, colors: [[f32; 4]; 3]) {
        let n = (b - a).cross(c - a).normalize_or(Vec3::Y);
        for (p, col) in [a, b, c].into_iter().zip(colors) {
            let i = self.vertex(p, n, col);
            self.indices.push(i);
        }
    }

    /// A lumpy low-poly dome: `segments` around, rings at a few heights,
    /// each vertex pushed in or out a little, sunk slightly into the
    /// ground; flat faces coloured from `shades` (darker low down: the
    /// dome shades itself), like the trees' facets.
    fn lump(&mut self, base: Vec3, radius: Vec3, segments: usize, dice: &mut Dice, shades: &[[f32; 4]]) {
        let rings = [(-0.15f32, 0.95f32), (0.45, 0.95), (0.85, 0.5)];
        let turn = dice.next() * std::f32::consts::TAU;
        let verts: Vec<Vec<Vec3>> = rings
            .iter()
            .enumerate()
            .map(|(k, (h, r))| {
                (0..segments)
                    .map(|s| {
                        let a = turn + (s as f32 + 0.5 * (k % 2) as f32) / segments as f32 * std::f32::consts::TAU;
                        let wobble = dice.range(0.75, 1.15);
                        base + Vec3::new(a.cos() * r * wobble, h * dice.range(0.85, 1.1), a.sin() * r * wobble) * radius
                    })
                    .collect()
            })
            .collect();
        let top = base + Vec3::Y * radius.y * dice.range(0.95, 1.1);
        let mut pick = |k: usize| scale_rgb(shades[(dice.next() * shades.len() as f32) as usize % shades.len()], 0.7 + 0.15 * k as f32);
        for k in 0..rings.len() - 1 {
            for s in 0..segments {
                let t = (s + 1) % segments;
                let (a, b, c, d) = (verts[k][s], verts[k][t], verts[k + 1][s], verts[k + 1][t]);
                // Odd rings are turned half a segment: zig-zag between them.
                if k % 2 == 0 {
                    self.tri(a, c, b, [pick(k); 3]);
                    self.tri(b, c, d, [pick(k); 3]);
                } else {
                    self.tri(a, d, b, [pick(k); 3]);
                    self.tri(a, c, d, [pick(k); 3]);
                }
            }
        }
        let last = rings.len() - 1;
        for s in 0..segments {
            self.tri(verts[last][s], top, verts[last][(s + 1) % segments], [pick(last + 1); 3]);
        }
    }
    /// An angular rock shard: a ground polygon rising to a ridge leaning
    /// along `lean`, sunk a little into the ground.
    fn shard(&mut self, base: Vec3, size: f32, height: f32, lean: Vec2, dice: &mut Dice, shades: &[[f32; 4]]) {
        let n = 5;
        let turn = dice.next() * std::f32::consts::TAU;
        let foot: Vec<Vec3> = (0..n)
            .map(|i| {
                let a = turn + i as f32 / n as f32 * std::f32::consts::TAU;
                base + Vec3::new(a.cos(), 0.0, a.sin()) * size * dice.range(0.6, 1.1) - Vec3::Y * 0.15
            })
            .collect();
        let tip = base + Vec3::new(lean.x * size * 0.6, height, lean.y * size * 0.6);
        let shoulder = base + Vec3::new(-lean.x * size * 0.2 + dice.range(-0.2, 0.2) * size, height * dice.range(0.45, 0.7), -lean.y * size * 0.2 + dice.range(-0.2, 0.2) * size);
        for i in 0..n {
            let (a, b) = (foot[i], foot[(i + 1) % n]);
            let top = if i % 2 == 0 { tip } else { shoulder };
            let c = shades[(dice.next() * shades.len() as f32) as usize % shades.len()];
            self.tri(a, top, b, [scale_rgb(c, 0.8), c, scale_rgb(c, 0.8)]);
            if i % 2 == 1 {
                self.tri(b, top, tip, [c; 3]);
            } else {
                self.tri(a, shoulder, top, [scale_rgb(c, 0.9); 3]);
            }
        }
    }

    /// A six-sided crystal with a pointed tip, standing at `base`, tilted
    /// toward `tilt`.
    fn crystal(&mut self, base: Vec3, radius: f32, height: f32, tilt: Vec3, color: [f32; 4], tip_color: [f32; 4]) {
        let up = (Vec3::Y + tilt).normalize();
        let side = up.cross(Vec3::Z).normalize_or(Vec3::X);
        let fwd = side.cross(up);
        let ring = |h: f32, r: f32| -> Vec<Vec3> {
            (0..6)
                .map(|i| {
                    let a = i as f32 / 6.0 * std::f32::consts::TAU;
                    base + up * h + (side * a.cos() + fwd * a.sin()) * r
                })
                .collect()
        };
        let (low, high) = (ring(-0.5, radius), ring(height * 0.78, radius * 0.92));
        let tip = base + up * height;
        for i in 0..6 {
            let j = (i + 1) % 6;
            let shade = 0.82 + 0.18 * ((i as f32 * 1.7).sin() * 0.5 + 0.5);
            let c = scale_rgb(color, shade);
            self.tri(low[i], high[i], low[j], [c, mix_rgb(c, tip_color, 0.5), c]);
            self.tri(low[j], high[i], high[j], [c, mix_rgb(c, tip_color, 0.5), mix_rgb(c, tip_color, 0.5)]);
            self.tri(high[i], tip, high[j], [mix_rgb(c, tip_color, 0.5), tip_color, mix_rgb(c, tip_color, 0.5)]);
        }
    }

    /// A long six-sided crystal: a prism from `base` along `axis` (unit),
    /// `radius` to its edges and `length` long, ending in a pointed tip,
    /// turned `twist` about its axis. Each side is one flat facet colour,
    /// darker toward the foot like smoky quartz, the tip lighter; the foot
    /// glows in `glow` (its alpha) fading out a little way up.
    #[allow(clippy::too_many_arguments)]
    fn prism(&mut self, base: Vec3, axis: Vec3, radius: f32, length: f32, twist: f32, facets: &[[f32; 4]; 6], tip: [f32; 4], glow: [f32; 4]) {
        let side = axis.cross(if axis.y.abs() > 0.99 { Vec3::X } else { Vec3::Y }).normalize();
        let fwd = axis.cross(side);
        let ring = |h: f32, r: f32| -> [Vec3; 6] {
            std::array::from_fn(|i| {
                let a = twist + i as f32 / 6.0 * std::f32::consts::TAU;
                base + axis * h + (side * a.cos() + fwd * a.sin()) * r
            })
        };
        let glow_h = 0.2 + 0.06 * length;
        let (r0, r1, r2) = (ring(-0.35, radius * 0.98), ring(glow_h, radius), ring(length * 0.8, radius * 0.94));
        let apex = base + axis * length;
        for i in 0..6 {
            let j = (i + 1) % 6;
            let c = facets[i];
            let foot = scale_rgb(c, 0.7);
            let shoulder = mix_rgb(c, tip, 0.3);
            self.tri(r0[i], r0[j], r1[j], [glow, glow, foot]);
            self.tri(r0[i], r1[j], r1[i], [glow, foot, foot]);
            self.tri(r1[i], r1[j], r2[j], [foot, foot, c]);
            self.tri(r1[i], r2[j], r2[i], [foot, c, c]);
            self.tri(r2[i], r2[j], apex, [shoulder, shoulder, tip]);
        }
    }

    /// Dense dark brambles: two or three thorny lumps.
    fn bramble(&mut self, at: Vec3, dice: &mut Dice) {
        let leaves = [lin(0x2c4a23, 0.0), lin(0x35562a, 0.0), lin(0x3e5f2b, 0.0), lin(0x263f1e, 0.0), lin(0x46652f, 0.0)];
        let thorn = lin(0x4a3a2a, 0.0);
        let lumps = if dice.next() < 0.6 { 1 } else { 2 };
        for k in 0..lumps {
            let off = if k == 0 { Vec3::ZERO } else { Vec3::new(dice.range(-0.45, 0.45), 0.0, dice.range(-0.45, 0.45)) };
            let r = dice.range(0.45, 0.68) * if k == 0 { 1.0 } else { 0.75 };
            let base = at + off;
            self.lump(base, Vec3::new(r, r * dice.range(1.0, 1.6), r), 5, dice, &leaves);
            // Thorny canes poking out of the top half.
            for _ in 0..2 {
                let a = dice.range(0.0, std::f32::consts::TAU);
                let from = base + Vec3::new(a.cos() * r * 0.6, r * dice.range(0.7, 1.2), a.sin() * r * 0.6);
                let to = from + Vec3::new(a.cos() * 0.35, dice.range(0.05, 0.3), a.sin() * 0.35);
                let w = Vec3::new(-a.sin(), 0.0, a.cos()) * 0.04;
                self.tri(from - w, from + w, to, [thorn; 3]);
            }
        }
    }

    /// A clump of reeds and rushes, a cattail or two.
    fn reeds(&mut self, at: Vec3, dice: &mut Dice) {
        let (low, high) = (lin(0x71823f, 0.0), lin(0xbcae66, 0.0));
        let blades = 9 + (dice.next() * 7.0) as usize;
        for _ in 0..blades {
            let a = dice.range(0.0, std::f32::consts::TAU);
            let foot = at + Vec3::new(a.cos(), 0.0, a.sin()) * dice.range(0.0, 0.4) - Vec3::Y * 0.1;
            let h = dice.range(0.55, 1.25);
            let lean = Vec3::new(a.cos(), 0.0, a.sin()) * dice.range(0.05, 0.35);
            let w = Vec3::new(-a.sin(), 0.0, a.cos()) * dice.range(0.04, 0.07);
            let tip = mix_rgb(low, high, dice.range(0.5, 1.0));
            self.tri(foot - w, foot + w, foot + Vec3::Y * h + lean, [low, low, tip]);
        }
        if dice.next() < 0.6 {
            let head = lin(0x5b4330, 0.0);
            let top = at + Vec3::new(dice.range(-0.15, 0.15), dice.range(0.9, 1.25), dice.range(-0.15, 0.15));
            let w = 0.05;
            self.crystal(top - Vec3::Y * 0.3, w, 0.32, Vec3::ZERO, head, head);
        }
    }
}

/// Mountain greys (and a lichen) for scree and spurs.
fn rock_shades() -> [[f32; 4]; 4] {
    [lin(0x8c8e90, 0.0), lin(0x7e8184, 0.0), lin(0x686b6f, 0.0), lin(0x8f9178, 0.0)]
}

/// Charred, ore-less meteor rock.
fn char_shades() -> [[f32; 4]; 3] {
    [lin(0x3a3436, 0.0), lin(0x2b2628, 0.0), lin(0x4a4446, 0.0)]
}

/// Heaved, scorched rim rock: darker greys warmed by soot.
fn rim_shades() -> [[f32; 4]; 4] {
    [lin(0x6e6a68, 0.0), lin(0x57534f, 0.0), lin(0x7f7b77, 0.0), lin(0x403b3b, 0.0)]
}

/// The drawn ground's height at a world point (bilinear between its
/// cell's corners: close enough to set a stone on).
fn surface(field: &Field, p: Vec2) -> f32 {
    let g = (p - field.origin) / field.cell;
    let (x, z) = (g.x.floor() as i32, g.y.floor() as i32);
    let (fx, fz) = (g.x - x as f32, g.y - z as f32);
    let h = |dx: i32, dz: i32| field.height(x + dx, z + dz);
    (h(0, 0) * (1.0 - fx) + h(1, 0) * fx) * (1.0 - fz) + (h(0, 1) * (1.0 - fx) + h(1, 1) * fx) * fz
}

/// Is the cell next to (4-neighbourhood) a cell `pred` holds for?
fn next_to(field: &Field, x: i32, z: i32, pred: impl Fn(Cover) -> bool) -> Option<Vec2> {
    let mut away = Vec2::ZERO;
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        if pred(field.cover_at(x + dx, z + dz)) {
            away += Vec2::new(dx as f32, dz as f32);
        }
    }
    (away != Vec2::ZERO).then(|| away.normalize_or(Vec2::X))
}

/// The props of one map chunk.
pub fn chunk_props(field: &Field, r: ChunkRect) -> PropMesh {
    let mut m = PropMesh::default();
    let salt = field.seed;
    for (x, z) in r.cells() {
        let cover = field.cover_at(x, z);
        let centre = field.world_cell(x, z);
        let jitter = |dice: &mut Dice| centre + Vec2::new(dice.range(-0.3, 0.3), dice.range(-0.3, 0.3)) * field.cell;
        let on = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
        let in_site = field.in_site(centre);
        let in_spire = field.spire.is_some_and(|(c, rad)| centre.distance(c) < rad + 3.0);
        match cover {
            Cover::Thicket => {
                let mut dice = Dice::new(x, z, salt ^ 0xb7a3);
                let p = jitter(&mut dice);
                m.bramble(on(p, 0.0), &mut dice);
            }
            Cover::Marsh if cell_hash(x, z, salt ^ 0x4ee1) < 0.3 => {
                let mut dice = Dice::new(x, z, salt ^ 0x4ee2);
                let p = jitter(&mut dice);
                m.reeds(on(p, -0.08), &mut dice);
            }
            Cover::Shallows if cell_hash(x, z, salt ^ 0x4ee3) < 0.45 && next_to(field, x, z, Cover::is_land).is_some() => {
                let mut dice = Dice::new(x, z, salt ^ 0x4ee4);
                let p = jitter(&mut dice);
                m.reeds(on(p, -0.3), &mut dice);
            }
            Cover::Mountain if in_spire => {}
            Cover::Mountain | Cover::Highland if in_site && cell_hash(x, z, salt ^ 0x5c01) < 0.035 => {
                let mut dice = Dice::new(x, z, salt ^ 0x5c02);
                let p = jitter(&mut dice);
                let lean = Vec2::new(dice.range(-1.0, 1.0), dice.range(-1.0, 1.0)).normalize_or(Vec2::X);
                let size = dice.range(0.3, 0.6);
                let y = field.height(x, z).min(field.height(x + 1, z + 1));
                m.shard(on(p, y), size, size * dice.range(0.5, 0.9), lean, &mut dice, &char_shades());
            }
            Cover::Highland if in_site => {
                let mut dice = Dice::new(x, z, salt ^ 0x5c07);
                if let Some(away) = next_to(field, x, z, |c| c != Cover::Highland) {
                    // Scree down the rim's flanks: a few angular stones.
                    for _ in 0..1 + (dice.next() * 3.0) as usize {
                        let p = centre + Vec2::new(dice.range(-0.45, 0.45), dice.range(-0.45, 0.45)) * field.cell + away * 0.12;
                        let size = dice.range(0.14, 0.3);
                        m.shard(on(p, surface(field, p) + 0.06), size, size * dice.range(0.5, 0.9), away, &mut dice, &rim_shades());
                    }
                } else if field.rim[field.index(x, z)] >= 2.0 && dice.next() < 0.16 {
                    // Jagged spurs along the crest.
                    let p = jitter(&mut dice);
                    let lean = Vec2::new(dice.range(-1.0, 1.0), dice.range(-1.0, 1.0)).normalize_or(Vec2::X);
                    let size = dice.range(0.3, 0.55);
                    m.shard(on(p, surface(field, p)), size, size * dice.range(0.9, 1.5), lean, &mut dice, &rim_shades());
                }
            }
            Cover::Mountain => {
                if let Some(away) = next_to(field, x, z, Cover::is_land)
                    && cell_hash(x, z, salt ^ 0x5c03) < 0.3
                {
                    let mut dice = Dice::new(x, z, salt ^ 0x5c04);
                    let p = jitter(&mut dice) - away * 0.3;
                    let size = dice.range(0.45, 0.95);
                    m.shard(on(p, 0.0), size, size * dice.range(0.6, 1.1), away, &mut dice, &rock_shades());
                }
            }
            Cover::Highland => {
                if let Some(away) = next_to(field, x, z, |c| c.is_land() && c != Cover::Highland)
                    && cell_hash(x, z, salt ^ 0x5c05) < 0.22
                {
                    let mut dice = Dice::new(x, z, salt ^ 0x5c06);
                    let p = jitter(&mut dice) - away * 0.15;
                    let size = dice.range(0.25, 0.5);
                    m.shard(on(p, 0.3), size, size * dice.range(0.8, 1.4), away, &mut dice, &rock_shades());
                }
            }
            _ => {}
        }
    }
    m
}

/// The Starspire of a `Spire` meteor site, if the map has one: its own
/// mesh, so it can cast its tall shadow (other props do not).
pub fn spire_props(field: &Field) -> PropMesh {
    let mut m = PropMesh::default();
    if let Some((c, radius)) = field.spire {
        spire(&mut m, Vec3::new(c.x, 0.9, c.y), radius, field.seed);
    }
    m
}

/// The Starspire: a giant dormant crystal, a cluster of long six-sided
/// prisms with pointed tips leaning out from a common base (three tall
/// ones at the heart, a ring of shorter ones round them, small ones
/// splaying out low), pale smoky white-violet with darker facets, rather
/// than the bright violet of an aether node; only their feet glow a soft
/// violet (well under a twentieth of the surface). Charred splinters lie
/// round its rim.
pub fn spire(m: &mut PropMesh, at: Vec3, radius: f32, seed: u32) {
    use std::f32::consts::TAU;
    let facets = [lin(0xd2c9e4, 0.0), lin(0xa497c4, 0.0), lin(0x665a8c, 0.0), lin(0xbcb1d6, 0.0), lin(0x807399, 0.0), lin(0xddd6ec, 0.0)];
    let (tip, glow) = (lin(0xede8f6, 0.0), lin(0xa16ff0, 1.0));
    let mut dice = Dice(seed ^ 0x5717);
    let top = af_core::ext::render::MAX_TERRAIN_HEIGHT - 0.3 - at.y;
    let thick = (radius * 0.13).clamp(0.9, 2.4);
    let lean = |out: Vec3, angle: f32| (Vec3::Y * angle.cos() + out * angle.sin()).normalize();
    let turn = dice.next() * TAU;
    // The heart: three tall prisms leaning a little apart.
    for (k, (tilt, size, length)) in [(0.05f32, 0.78f32, 1.0f32), (0.2, 0.6, 0.84), (0.27, 0.55, 0.74)].into_iter().enumerate() {
        let a = turn + k as f32 / 3.0 * TAU + dice.range(-0.3, 0.3);
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        let axis = lean(out, tilt);
        let foot = at + out * thick * if k == 0 { 0.1 } else { 0.7 };
        m.prism(foot, axis, thick * size, top * length / axis.y, dice.range(0.0, TAU), &facets, tip, glow);
    }
    // A ring of shorter prisms round them, leaning out.
    let ring = 8;
    for i in 0..ring {
        let a = turn + (i as f32 + 0.5) / ring as f32 * TAU + dice.range(-0.22, 0.22);
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        let foot = at + out * (thick * 1.1 + radius * dice.range(0.03, 0.1)) + Vec3::Y * 0.15;
        let axis = lean(out, dice.range(0.32, 0.58));
        m.prism(foot, axis, thick * dice.range(0.36, 0.48), top * dice.range(0.5, 0.74), dice.range(0.0, TAU), &facets, tip, glow);
    }
    // Slimmer ones fanning out low from the same base.
    let splay = 10;
    for i in 0..splay {
        let a = turn + i as f32 / splay as f32 * TAU + dice.range(-0.25, 0.25);
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        let foot = at + out * radius * dice.range(0.14, 0.3) + Vec3::Y * 0.15;
        let axis = lean(out, dice.range(0.62, 0.92));
        m.prism(foot, axis, thick * dice.range(0.24, 0.34), top * dice.range(0.3, 0.5), dice.range(0.0, TAU), &facets, tip, glow);
    }
    for i in 0..10 {
        let a = i as f32 / 10.0 * std::f32::consts::TAU + dice.range(-0.3, 0.3);
        let foot = at + Vec3::new(a.cos(), 0.0, a.sin()) * radius * dice.range(0.55, 0.9);
        let size = dice.range(0.5, 1.0);
        m.shard(foot, size, size * dice.range(0.6, 1.1), Vec2::new(a.cos(), a.sin()), &mut dice, &char_shades());
    }
}

/// The merged mesh and its bounds, or `None` when the chunk has no props.
pub fn prop_mesh(props: PropMesh) -> Option<(Mesh, Aabb)> {
    if props.is_empty() {
        return None;
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &props.positions {
        lo = lo.min(Vec3::from(*p));
        hi = hi.max(Vec3::from(*p));
    }
    let n = props.positions.len();
    let indices = if n <= u16::MAX as usize { Indices::U16(props.indices.iter().map(|i| *i as u16).collect()) } else { Indices::U32(props.indices) };
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, props.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, props.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, props.colors)
        .with_inserted_indices(indices);
    Some((mesh, Aabb::from_min_max(lo, hi)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::field::FieldSource;
    use crate::terrain::mesh::chunks;
    use af_core::prelude::TerrainId;
    use af_core::testing::test_data;

    #[test]
    fn props_grow_only_on_ground_nothing_is_built_on() {
        let data = test_data();
        let nav = crate::terrain::field::tests::painted(&data, 128);
        let f = Field::build(&FieldSource { data: &data, nav: &nav, seed: 5, features: None, crater: Some((Vec2::new(60.0, 85.0), 10.0)) });
        let mut total = 0;
        for (r, on_map) in chunks(&f) {
            if !on_map {
                continue;
            }
            let props = chunk_props(&f, r);
            total += props.positions.len();
            assert!(props.indices.iter().all(|i| (*i as usize) < props.positions.len()));
            for tri in props.indices.chunks(3) {
                let centre = tri.iter().map(|i| Vec3::from(props.positions[*i as usize])).sum::<Vec3>() / 3.0;
                // Every prop stands within reach of an unbuildable cell.
                let c = f.cell_of(centre.xz());
                let near_unbuildable = (-2..=2).any(|dz| {
                    (-2..=2).any(|dx| {
                        let m = IVec2::new(c.x + dx - f.skirt, c.y + dz - f.skirt);
                        !nav.in_bounds(m) || !data.terrain_def(TerrainId(nav.terrain_at(m))).buildable
                    })
                });
                assert!(near_unbuildable, "prop at {centre} on buildable ground");
                assert!(centre.y < 10.6);
            }
        }
        assert!(total > 1000, "brambles, reeds and scree: {total} vertices");
        // Deterministic.
        let again: usize = chunks(&f).iter().filter(|c| c.1).map(|(r, _)| chunk_props(&f, *r).positions.len()).sum();
        assert_eq!(again, total);
    }
}
