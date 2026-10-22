// Shared by the terrain, water and prop shaders: their common uniforms
// (binding 100, `material::GroundParams`), the fog-of-war texture, value
// noise and the look of the fog of war and of the off-map skirt.
#define_import_path af_render::terrain_common

struct GroundParams {
    // xy: the map's min corner, zw: its max corner (world x / z).
    map: vec4<f32>,
    // xy: the class images' min corner, zw: 1 / their size.
    field: vec4<f32>,
    // xy: 1 / the fog grid's size, z: 1 while the fog of war is drawn,
    // w: the skirt's width.
    fog: vec4<f32>,
    // The sky / haze colour (linear) the skirt fades into.
    sky: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> ground: GroundParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var fog_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var fog_sampler: sampler;

const TAU: f32 = 6.2831853;

// sRGB palette colour -> linear.
fn lin(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn hash22(p: vec2<f32>) -> vec2<f32> {
    var p3 = fract(vec3<f32>(p.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.xx + p3.yz) * p3.zy);
}

// Smooth value noise in 0..1, one lattice cell per unit.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash12(i);
    let b = hash12(i + vec2<f32>(1.0, 0.0));
    let c = hash12(i + vec2<f32>(0.0, 1.0));
    let d = hash12(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Three octaves of value noise in 0..1.
fn fbm(p: vec2<f32>) -> f32 {
    let m = mat2x2<f32>(1.6, 1.2, -1.2, 1.6);
    var q = p;
    var sum = vnoise(q) * 0.5714;
    q = m * q + 17.1;
    sum += vnoise(q) * 0.2857;
    q = m * q + 31.7;
    sum += vnoise(q) * 0.1429;
    return sum;
}

// Cellular noise: distance to the nearest of a jittered point per unit
// cell, to the second nearest, and a hash of the nearest's cell. Where the
// two distances meet are the edges of the cells (cracks, plates).
fn cells(p: vec2<f32>) -> vec3<f32> {
    let i = floor(p);
    let f = fract(p);
    var d1 = 8.0;
    var d2 = 8.0;
    var id = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let g = vec2<f32>(f32(x), f32(y));
            let r = g + hash22(i + g) * 0.9 + 0.05 - f;
            let d = dot(r, r);
            if (d < d1) {
                d2 = d1;
                d1 = d;
                id = hash12(i + g + 0.37);
            } else if (d < d2) {
                d2 = d;
            }
        }
    }
    return vec3<f32>(sqrt(d1), sqrt(d2), id);
}

// Ridged noise in 0..1: 1 along thin crests.
fn ridge(p: vec2<f32>) -> f32 {
    return 1.0 - abs(vnoise(p) * 2.0 - 1.0);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The clear turquoise of water a hand deep, where the shore meets it:
// the water shader's colour at the bank and the ground shader's painted
// rim must match.
fn shallow_water(p: vec2<f32>) -> vec3<f32> {
    return lin(vec3<f32>(0.4, 0.72, 0.88)) * (0.84 + 0.08 * vnoise(p * 0.6));
}

// What never-explored ground looks like (linear): near black, the same as
// `terrain::BEYOND_FOG`.
const UNEXPLORED: vec3<f32> = vec3<f32>(0.0014, 0.0017, 0.0024);

// How much of the fog of war lies over world `p`: x = out of sight
// (explored), y = never explored. The texture's 2 m cells are sampled
// through a little warp, so the fog's edge wanders instead of following
// them.
fn fog_at(p: vec2<f32>) -> vec2<f32> {
    if (ground.fog.z < 0.5) {
        return vec2<f32>(0.0);
    }
    let warp = vec2<f32>(vnoise(p * 0.3), vnoise(p * 0.3 + 7.7)) - 0.5;
    let a = textureSampleLevel(fog_texture, fog_sampler, (p + warp * 2.4) * ground.fog.xy, 0.0).a;
    return vec2<f32>(smoothstep(0.03, 0.5, a), smoothstep(0.52, 0.97, a));
}

// Over a lit colour at world `p`, before Bevy's post-lighting step: off
// the map greyed and darkened, fading into the sky (or the dark, while the
// fog of war is drawn) toward the skirt's far edge; out of sight darkened
// and greyed (`fog` from `fog_at`; the fog grid is clamped at the
// edge, so the skirt beyond an unexplored edge stays dark too).
fn ground_veil(color: vec3<f32>, p: vec2<f32>, fog: vec2<f32>) -> vec3<f32> {
    var c = color;
    let out = max(max(ground.map.xy - p, p - ground.map.zw), vec2<f32>(0.0));
    let off = length(out);
    let k = smoothstep(0.0, 4.0, off);
    c = mix(c, vec3<f32>(luma(c)) * vec3<f32>(0.95, 0.98, 1.05), k * 0.45) * (1.0 - 0.3 * k);
    c = mix(c, vec3<f32>(luma(c)) * vec3<f32>(0.9, 0.95, 1.08), fog.x * 0.6) * (1.0 - 0.6 * fog.x);
    let fade = smoothstep(ground.fog.w * 0.55, ground.fog.w, off);
    return mix(c, ground.sky.rgb, fade);
}

// After Bevy's post-lighting step (so neither the haze nor bloom shows
// anything there): never-explored ground is dark.
fn unexplored(color: vec4<f32>, fog: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(mix(color.rgb, UNEXPLORED, fog.y), color.a);
}