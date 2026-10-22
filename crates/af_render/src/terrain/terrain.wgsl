// The ground. Blends the cells' covers with noise-warped weights from the
// two class images (channels: meadow, sand, marsh, forest floor | rock,
// mountain, scar, water bed) and paints each procedurally in world space
// with the landscape palette (docs/art/style-guide.md, "Landscape"). It
// also paints the water's rim on the bank, so shorelines curve instead of
// following the cells. Vertex colour is data: r = near water, g = near a
// mountain's foot.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::globals,
}
#import af_render::terrain_common::{ground, lin, hash12, hash22, vnoise, fbm, cells, luma, ground_veil, fog_at, unexplored, shallow_water, TAU}

@group(#{MATERIAL_BIND_GROUP}) @binding(103) var classes_a: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var classes_a_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var classes_b: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var classes_b_sampler: sampler;

struct Paint {
    color: vec3<f32>,
    rough: f32,
    glow: vec3<f32>,
    // Height of the fine relief the normal is bumped by.
    bump: f32,
};

fn paint(color: vec3<f32>, rough: f32, bump: f32) -> Paint {
    return Paint(color, rough, vec3<f32>(0.0), bump);
}

// Lush meadow, painted from large to small: broad swathes of deeper green
// (a hundred metres and more), lighter yellow-green and olive meadows
// between them, now and then a field-like patch of pasture, hay or clover
// with faint mowing stripes, then gentle patches, grass clumps and tufts
// (kept soft: the game is seen from 40-66 m), speckles of wildflowers;
// richer and darker toward water. A step below the team colours in
// saturation, so Sylvan green still reads on it.
fn meadow(p: vec2<f32>, wet: f32) -> Paint {
    let deep = lin(vec3<f32>(0.17, 0.32, 0.14));
    let base = lin(vec3<f32>(0.245, 0.44, 0.175));
    let fresh = lin(vec3<f32>(0.33, 0.51, 0.19));
    let olive = lin(vec3<f32>(0.38, 0.45, 0.22));
    let dry = lin(vec3<f32>(0.56, 0.55, 0.33));
    // Swathes and meadows, their outlines warped so they never look like
    // noise blobs.
    let q = p + (vec2<f32>(vnoise(p * 0.017), vnoise(p * 0.017 + vec2<f32>(5.3, 1.9))) - 0.5) * 46.0;
    let vast = fbm(q * 0.006 + 1.3);
    let big = fbm(q * 0.018 + 3.7);
    let meadows = fbm(q * 0.012 + 41.0);
    let tint = fbm(q * 0.03 + 9.0);
    let mid = fbm(p * 0.05 + 7.0);
    var c = mix(deep, base, smoothstep(0.32, 0.56, big + (vast - 0.5) * 0.6));
    c = mix(c, fresh, smoothstep(0.56, 0.72, vast + (big - 0.5) * 0.5) * 0.65);
    c = mix(c, olive, smoothstep(0.6, 0.76, meadows) * 0.45);
    c = mix(c, deep * 0.9, smoothstep(0.64, 0.8, 1.0 - big) * 0.45);
    // Patches a few tens of metres across, a little warmer or cooler.
    c *= mix(vec3<f32>(0.95, 1.0, 1.04), vec3<f32>(1.04, 1.01, 0.94), smoothstep(0.35, 0.65, tint));
    // Within one view (10-20 m): lusher, darker hollows and sunny, lighter
    // yellow-green rises, softly edged.
    let hollows = fbm(q * 0.06 + 23.0);
    // (fbm is about 0.4-0.6 over half the ground, 0.3-0.7 over most.)
    c = mix(c, c * vec3<f32>(0.74, 0.86, 0.76), smoothstep(0.5, 0.66, hollows) * 0.8);
    c = mix(c, mix(c, fresh, 0.5) * 1.15, smoothstep(0.5, 0.34, hollows) * 0.65);
    // Field-like patches: now and then a plot of a coarse grid (rows
    // staggered, the grid turned off the map's axes), one crop each with
    // soft, wandering edges and faint stripes down its length.
    let along = vec2<f32>(0.857, 0.515);
    let g = vec2<f32>(dot(p, along), dot(p, vec2<f32>(-along.y, along.x))) / vec2<f32>(36.0, 23.0);
    let row = floor(g.y);
    let gx = g.x + hash12(vec2<f32>(row, 7.7)) * 0.8;
    let plot = vec2<f32>(floor(gx), row);
    let pick = hash12(plot + 0.31);
    let inner = min(min(fract(gx), 1.0 - fract(gx)) * 36.0, min(fract(g.y), 1.0 - fract(g.y)) * 23.0);
    let is_field = step(pick, 0.06) * smoothstep(1.5, 6.0, inner + (fbm(p * 0.15) - 0.5) * 6.0);
    let crop = hash12(plot + 5.17);
    var field_col = lin(vec3<f32>(0.34, 0.5, 0.2));
    if (crop < 0.35) {
        field_col = lin(vec3<f32>(0.5, 0.49, 0.27));
    } else if (crop < 0.6) {
        field_col = lin(vec3<f32>(0.22, 0.38, 0.165));
    }
    let stripe = sin(g.y * 23.0 * TAU / 2.6) * 0.5 + 0.5;
    field_col *= 0.92 + 0.13 * stripe;
    c = mix(c, field_col, is_field * 0.5 * (1.0 - wet));
    // Patches tens of metres across, a little lighter or darker.
    c *= 0.86 + 0.28 * smoothstep(0.35, 0.65, mid);
    // Dry spots.
    let small = fbm(p * 0.3 + 3.0);
    c = mix(c, dry, smoothstep(0.6, 0.78, small) * smoothstep(0.45, 0.66, mid) * 0.38 * (1.0 - wet));
    // Grass clumps a few decimetres across, and darker tufts: gentle.
    let clump = vnoise(p * 2.7) * 0.6 + vnoise(p * 6.3) * 0.4;
    c *= 0.9 + 0.19 * clump;
    c *= 1.0 - smoothstep(0.62, 0.8, fbm(mat2x2<f32>(0.8, 0.6, -0.6, 0.8) * p * 0.7 + 17.0)) * 0.12;
    // Richer, darker grass along the water.
    c = mix(c, c * vec3<f32>(0.7, 0.84, 0.68), wet * 0.6);
    c = mix(c, vec3<f32>(luma(c)), 0.08);
    // Wildflowers: white, yellow and blue dots in flowery patches.
    let cell = floor(p * 2.4);
    let h = hash12(cell);
    let spot = fract(p * 2.4) - 0.5 - (hash22(cell) - 0.5) * 0.6;
    let flowery = smoothstep(0.55, 0.78, fbm(p * 0.06 + 3.1)) * (1.0 - wet) * (1.0 - is_field);
    let bloom = (1.0 - smoothstep(0.08, 0.14, length(spot))) * step(h, 0.1 * flowery);
    let hue = hash12(cell + 7.3);
    var flower = lin(vec3<f32>(0.93, 0.92, 0.86));
    if (hue < 0.45) {
        flower = lin(vec3<f32>(0.92, 0.8, 0.28));
    } else if (hue < 0.62) {
        flower = lin(vec3<f32>(0.56, 0.6, 0.9));
    }
    c = mix(c, flower, bloom);
    // No bumped relief: at play distances it read only as dark specks.
    return paint(c, 0.92, 0.0);
}

// Warm pale sand: broad swells, wind ripples, a few pebbles, and tufts of
// grass where it borders meadow (`grassy`); beach sand by the water, damp
// at its edge.
fn sand(p: vec2<f32>, wet: f32, grassy: f32) -> Paint {
    let dune = lin(vec3<f32>(0.890, 0.827, 0.635));
    let beach = lin(vec3<f32>(0.847, 0.765, 0.557));
    let swell = fbm(p * 0.07 + 2.0);
    let ripple = sin(dot(p, vec2<f32>(0.83, 0.55)) * 4.2 + vnoise(p * 0.4) * 8.0) * 0.5 + 0.5;
    var c = mix(dune, beach, smoothstep(0.15, 0.7, wet)) * 0.84;
    c *= 0.8 + 0.34 * swell;
    c *= 0.92 + 0.09 * ripple + 0.05 * (vnoise(p * 4.0) - 0.5);
    c = mix(c, c * 0.72, smoothstep(0.82, 0.92, vnoise(p * 5.3 + 2.0)) * 0.5);
    let tuft = smoothstep(0.6, 0.72, vnoise(p * 3.3 + 8.0)) * smoothstep(0.08, 0.4, grassy) * (1.0 - wet);
    c = mix(c, lin(vec3<f32>(0.42, 0.55, 0.25)), tuft * 0.8);
    c = mix(c, c * vec3<f32>(0.72, 0.7, 0.66), smoothstep(0.8, 1.0, wet) * 0.6);
    return paint(c, 0.95, ripple * 0.02 + swell * 0.08);
}
// Dark wet olive ground with puddles of dark water and rushy tufts.
fn marsh(p: vec2<f32>) -> Paint {
    let ground_col = lin(vec3<f32>(0.333, 0.420, 0.227));
    let water = lin(vec3<f32>(0.184, 0.290, 0.290));
    var c = ground_col * (0.75 + 0.4 * fbm(p * 0.3));
    c = mix(c, lin(vec3<f32>(0.45, 0.47, 0.26)), smoothstep(0.62, 0.8, vnoise(p * 2.2)) * 0.4);
    let puddle = smoothstep(0.55, 0.6, fbm(p * 0.3 + 5.0) + (vnoise(p * 2.0) - 0.5) * 0.08);
    c = mix(c, water, puddle);
    return paint(c, mix(0.85, 0.1, puddle), (1.0 - puddle) * 0.04);
}

// Forest floor: dark leaf litter and moss under the brambles. Off the map
// (`off` metres out), where no trees stand, woodland is drawn as the
// canopy seen from above, its crowns (the relief, `bulge`) lit on top
// and dark between.
fn forest_floor(p: vec2<f32>, off: f32, bulge: f32) -> Paint {
    let floor_col = lin(vec3<f32>(0.231, 0.353, 0.165));
    let moss = lin(vec3<f32>(0.306, 0.431, 0.180));
    let litter = lin(vec3<f32>(0.420, 0.329, 0.204));
    var c = mix(floor_col, moss, fbm(p * 0.2));
    let leaves = smoothstep(0.55, 0.75, vnoise(p * 2.3)) * 0.5 + smoothstep(0.6, 0.85, fbm(p * 0.5)) * 0.3;
    c = mix(c, litter, leaves);
    c *= 0.82 + 0.28 * vnoise(p * 5.0);
    // The canopy: soft, green and only gently darker between the crowns,
    // so the far woods read as forest rather than dark stains.
    let leafy = vnoise(p * 1.9) * 0.5 + vnoise(p * 4.3) * 0.5;
    var canopy = mix(lin(vec3<f32>(0.25, 0.39, 0.16)), lin(vec3<f32>(0.36, 0.5, 0.2)), fbm(p * 0.06));
    canopy = mix(canopy, lin(vec3<f32>(0.42, 0.5, 0.22)), smoothstep(0.6, 0.8, fbm(p * 0.13 + 4.0)) * 0.4);
    canopy *= (0.8 + 0.32 * smoothstep(0.25, 0.75, bulge)) * (0.9 + 0.2 * leafy);
    let up = smoothstep(6.0, 9.0, off);
    return paint(mix(c, canopy, up), 0.95, mix(leaves * 0.05, leafy * 0.05, up));
}
// Grey rock: broad tones, warmer and cooler; on the faces, streaks running
// down the fall line and dark joints between blocks; lichen on the flatter
// parts.
fn rock(p: vec2<f32>, y: f32, n: vec3<f32>) -> Paint {
    let light = lin(vec3<f32>(0.643, 0.651, 0.659));
    let mid = lin(vec3<f32>(0.494, 0.506, 0.518));
    let dark = lin(vec3<f32>(0.333, 0.345, 0.361));
    let lichen = lin(vec3<f32>(0.725, 0.745, 0.549));
    let tone = fbm(p * 0.11 + y * 0.05);
    var c = mix(dark, mid, smoothstep(0.25, 0.55, tone));
    c = mix(c, light, smoothstep(0.6, 0.85, tone) * 0.7);
    c *= mix(vec3<f32>(1.05, 1.0, 0.93), vec3<f32>(0.95, 0.99, 1.05), fbm(p * 0.035));
    let steep = 1.0 - smoothstep(0.62, 0.95, n.y);
    // Seen from the side the face looks to (triplanar on x and z): the
    // block joints and the streaks down the fall line.
    let side = smoothstep(0.3, 0.7, abs(n.x) / max(abs(n.x) + abs(n.z), 1e-4));
    let face = mix(vec2<f32>(p.x, y), vec2<f32>(p.y, y), side);
    let blocks = cells(face * vec2<f32>(0.9, 1.4));
    let joint = 1.0 - smoothstep(0.03, 0.12, blocks.y - blocks.x);
    let streak = vnoise(face * vec2<f32>(1.5, 0.3)) * 0.6 + vnoise(face * vec2<f32>(3.7, 0.7)) * 0.4;
    c *= mix(1.0, 0.85 + 0.3 * blocks.z, steep);
    c = mix(c, dark * 0.6, joint * 0.55 * steep);
    c = mix(c, dark * 0.75, smoothstep(0.62, 0.88, streak) * 0.4 * steep);
    // The flatter parts: mottled, pitted, lichen.
    let mottle = fbm(p * 0.55 + 5.0);
    c *= mix(1.0, 0.82 + 0.36 * mottle, 1.0 - steep);
    c = mix(c, dark * 0.7, smoothstep(0.72, 0.86, vnoise(p * 2.3 + 3.0)) * 0.45 * (1.0 - steep));
    c = mix(c, lichen, smoothstep(0.64, 0.8, fbm(p * 0.45 + 11.0)) * 0.25 * smoothstep(0.55, 0.85, n.y));
    return paint(c, 0.85, tone * 0.1 + mottle * 0.05 * (1.0 - steep));
}

// Mountains: rock, lighter on ridges and darker in gullies (`bulge`),
// grass climbing their feet, snow on the gentler slopes near the summits.
fn mountain(p: vec2<f32>, y: f32, n: vec3<f32>, bulge: f32) -> Paint {
    var r = rock(p, y, n);
    r.color *= 0.58 + 0.7 * smoothstep(0.2, 0.8, bulge);
    let grass = lin(vec3<f32>(0.255, 0.44, 0.18)) * (0.75 + 0.35 * vnoise(p * 1.7));
    let foot = (1.0 - smoothstep(0.3, 1.6, y + (vnoise(p * 0.7) - 0.5) * 1.4)) * smoothstep(0.4, 0.8, n.y);
    r.color = mix(r.color, grass, foot * 0.9);
    let snow_line = 7.4 + (fbm(p * 0.15) - 0.5) * 1.4;
    let snow = smoothstep(snow_line - 0.3, snow_line + 0.4, y) * smoothstep(0.35, 0.6, n.y);
    let snow_col = mix(lin(vec3<f32>(0.788, 0.839, 0.902)), lin(vec3<f32>(0.933, 0.953, 0.973)), smoothstep(0.55, 0.95, n.y));
    r.color = mix(r.color, snow_col, snow);
    r.rough = mix(r.rough, 0.6, snow);
    r.bump *= 1.0 - snow * 0.7;
    return r;
}

// Rocky highland: a plateau of grey rock with grass and lichen on top,
// bare scarps at its edge.
fn highland(p: vec2<f32>, y: f32, n: vec3<f32>) -> Paint {
    var r = rock(p, y * 2.5, n);
    let tufts = lin(vec3<f32>(0.3, 0.47, 0.2)) * (0.72 + 0.35 * vnoise(p * 2.1));
    let top = smoothstep(0.93, 0.99, n.y);
    let green = smoothstep(0.42, 0.6, fbm(p * 0.19 + 4.0)) * 0.7 + smoothstep(0.55, 0.7, fbm(p * 0.9 + 1.0)) * 0.3;
    r.color = mix(r.color, tufts, top * green * 0.8);
    // The scarps: darker, steeper-looking rock.
    r.color *= 1.0 - (1.0 - top) * 0.25;
    return r;
}

// The meteor site's floor: dark basalt cracked into plates, glassy in
// places, and a few thin aether veins along the cracks that slowly pulse.
fn scar(p: vec2<f32>) -> Paint {
    let ash = lin(vec3<f32>(0.227, 0.204, 0.212));
    let char_col = lin(vec3<f32>(0.141, 0.122, 0.133));
    let aether = lin(vec3<f32>(0.749, 0.451, 1.0));
    let warp = vec2<f32>(vnoise(p * 0.3), vnoise(p * 0.3 + 9.0)) * 1.2;
    let plates = cells(p * 0.85 + warp);
    var c = mix(char_col, ash, fbm(p * 0.3) * 0.7 + plates.z * 0.3);
    c *= 0.85 + 0.3 * vnoise(p * 3.0);
    let glass = smoothstep(0.72, 0.86, vnoise(p * 1.1));
    c = mix(c, c * 1.6, glass * 0.35);
    let crack = (1.0 - smoothstep(0.02, 0.09, plates.y - plates.x)) * smoothstep(0.2, 0.55, vnoise(p * 0.45 + plates.z * 7.0));
    c = mix(c, char_col * 0.4, crack * 0.85);
    // Veins: the edges of larger cells, where a slow noise lets them show.
    let big = cells(p * 0.2 + warp * 0.3);
    let vein = (1.0 - smoothstep(0.0, 0.05, big.y - big.x)) * smoothstep(0.5, 0.7, fbm(p * 0.05 + 2.0));
    // Pulses every 4 s (a whole number of times an hour), out of step
    // across the site.
    let pulse = 0.5 + 0.5 * sin(globals.time * TAU / 4.0 + big.z * TAU);
    c = mix(c, aether * 0.3, vein);
    var out = paint(c, mix(0.8, 0.3, glass), fbm(p * 0.8) * 0.05 - crack * 0.04);
    out.glow = aether * vein * (0.5 + 1.1 * pulse);
    return out;
}

// What shows of a water bed at the bank, under the rim's water.
fn bed(p: vec2<f32>) -> Paint {
    let mud = lin(vec3<f32>(0.5, 0.45, 0.32));
    return paint(mud * (0.8 + 0.3 * vnoise(p * 1.5)), 0.4, 0.0);
}

fn add(acc: Paint, w: f32, p: Paint) -> Paint {
    return Paint(acc.color + p.color * w, acc.rough + p.rough * w, acc.glow + p.glow * w, acc.bump + p.bump * w);
}

// Bump a normal by a height field, from screen-space derivatives
// (Mikkelsen's surface gradient).
fn bumped(n: vec3<f32>, pos: vec3<f32>, h: f32) -> vec3<f32> {
    let dpx = dpdx(pos);
    let dpy = dpdy(pos);
    let r1 = cross(dpy, n);
    let r2 = cross(n, dpx);
    let det = dot(dpx, r1);
    let grad = sign(det) * (dpdx(h) * r1 + dpdy(h) * r2);
    return normalize(abs(det) * n - grad);
}

// Each present channel's weight nudged by its own noise, so borders
// between covers wander.
fn nudged(w: vec4<f32>, n: vec4<f32>) -> vec4<f32> {
    let present = step(vec4<f32>(0.004), w);
    return (w + (n - 0.5) * 0.4) * present;
}

fn max4(v: vec4<f32>) -> f32 {
    return max(max(v.x, v.y), max(v.z, v.w));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xz;
    let y = in.world_position.y;
    let wet = in.color.r;
    let n = pbr.N;
    let off_map = length(max(max(ground.map.xy - p, p - ground.map.zw), vec2<f32>(0.0)));

    // Which covers: the class images (one blurred texel a cell) sampled
    // through two layers of warp, then sharpened with noise.
    let warp = vec2<f32>(vnoise(p * 0.19), vnoise(p * 0.19 + vec2<f32>(31.7, 11.3))) - 0.5;
    let warp2 = vec2<f32>(vnoise(p * 1.3 + 5.1), vnoise(p * 1.3 + 9.7)) - 0.5;
    let uv = (p + warp * 1.2 + warp2 * 0.35 - ground.field.xy) * ground.field.zw;
    let ra = textureSampleLevel(classes_a, classes_a_sampler, uv, 0.0);
    let rb = textureSampleLevel(classes_b, classes_b_sampler, uv, 0.0);
    let na = vnoise(p * 0.75 + 11.0);
    let nb = vnoise(p * 0.75 + 23.0);
    let sa = nudged(ra, vec4<f32>(na, nb, 1.0 - na, 1.0 - nb));
    let sb = nudged(rb, vec4<f32>(nb, 1.0 - na, na, 0.5));
    // Only covers close to the strongest show: crisp, narrow blends.
    let top = max(max4(sa), max4(sb)) - 0.14;
    let ta = max(sa - vec4<f32>(top), vec4<f32>(0.0));
    let tb = max(sb - vec4<f32>(top), vec4<f32>(0.0));
    let total = max(dot(ta, vec4<f32>(1.0)) + dot(tb, vec4<f32>(1.0)), 1e-4);
    let wa = ta / total;
    let wb = tb / total;

    var acc = Paint(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0);
    if (wa.x > 0.003) { acc = add(acc, wa.x, meadow(p, wet)); }
    if (wa.y > 0.003) { acc = add(acc, wa.y, sand(p, wet, ra.x)); }
    if (wa.z > 0.003) { acc = add(acc, wa.z, marsh(p)); }
    if (wa.w > 0.003) { acc = add(acc, wa.w, forest_floor(p, off_map, in.color.b)); }
    if (wb.x > 0.003) { acc = add(acc, wb.x, highland(p, y, n)); }
    if (wb.y > 0.003) { acc = add(acc, wb.y, mountain(p, y, n, in.color.b)); }
    if (wb.z > 0.003) { acc = add(acc, wb.z, scar(p)); }
    if (wb.w > 0.003) { acc = add(acc, wb.w, bed(p)); }

    // Ground that slopes steeply anywhere but under water shows rock.
    let steep = (1.0 - smoothstep(0.55, 0.8, n.y)) * (1.0 - wb.w) * (1.0 - wa.w);
    if (steep > 0.01) {
        let r = rock(p, y, n);
        acc = Paint(mix(acc.color, r.color, steep), mix(acc.rough, r.rough, steep), acc.glow, mix(acc.bump, r.bump, steep));
    }
    // A darker ring of scree and shade round a mountain's foot.
    let foot = in.color.g * (1.0 - wb.y) * smoothstep(0.75, 0.95, n.y);
    acc.color = mix(acc.color, acc.color * vec3<f32>(0.82, 0.8, 0.76), foot * 0.45);
    // Hollows a little darker, edges a little lighter (the vertex bulge).
    acc.color *= clamp(1.0 + (in.color.b - 0.5) * 0.6, 0.78, 1.18);

    // The water's rim: the bank just above the water surface is painted as
    // the shallow water it borders, with a broken line of foam, so the
    // shore follows a curve rather than the cells under it.
    // Sampled with less warp than the covers, so the painted rim always
    // reaches past the cell edge where the water surface meets the bank.
    let uv_rim = (p + warp * 0.5 + warp2 * 0.35 - ground.field.xy) * ground.field.zw;
    let rim_w = textureSampleLevel(classes_b, classes_b_sampler, uv_rim, 0.0).w + (vnoise(p * 1.7 + 40.0) - 0.5) * 0.08 + (fbm(p * 0.4 + 9.0) - 0.5) * 0.08;
    let rim = smoothstep(0.25, 0.29, rim_w) * step(y, 0.02);
    // Foam lapping at the line, drifting along it (period 20 s).
    let lap = vnoise(p * 2.6 + vec2<f32>(sin(globals.time * TAU / 20.0), cos(globals.time * TAU / 20.0)) * 0.8);
    let foam = (1.0 - smoothstep(0.0, 0.035, abs(rim_w - 0.3))) * smoothstep(0.3, 0.6, lap);
    let water_col = shallow_water(p);
    acc.color = mix(acc.color, water_col, rim);
    acc.color = mix(acc.color, lin(vec3<f32>(0.9, 0.94, 0.95)), foam * rim * 0.45);
    acc.rough = mix(acc.rough, 0.1, rim);
    acc.bump *= 1.0 - rim;

    pbr.material.base_color = vec4<f32>(acc.color, 1.0);
    pbr.material.perceptual_roughness = clamp(acc.rough, 0.05, 1.0);
    pbr.material.reflectance = mix(pbr.material.reflectance, vec3<f32>(0.6), rim);
    pbr.material.emissive = vec4<f32>(acc.glow, 0.0);
    let flat_n = mix(n, vec3<f32>(0.0, 1.0, 0.0), rim);
    pbr.N = bumped(normalize(flat_n), in.world_position.xyz, acc.bump);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    let fog = fog_at(p);
    out.color = vec4<f32>(ground_veil(out.color.rgb, p, fog), out.color.a);
    out.color = unexplored(main_pass_post_lighting_processing(pbr, out.color), fog);
    return out;
}
