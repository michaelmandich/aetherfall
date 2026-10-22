// The water surface (opaque: depth is faked from the per-vertex bed
// depth). Vertex data: colour = (depth / 4 m, river, ford, sea), uv = the
// river course (metres downstream, metres across), uv_b.x = metres from
// the shore, tangent = the flow's direction (xyz) and speed (w, m/s), zero
// on still water. The very edge of the water is painted by the ground
// shader (its rim), in the same shallow colour.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::globals,
}
#import af_render::terrain_common::{ground, lin, vnoise, fbm, ground_veil, fog_at, unexplored, shallow_water, TAU}

// Ripple height around `q`.
fn ripples(q: vec2<f32>) -> f32 {
    return vnoise(q * 0.8) * 0.55 + vnoise(q * 2.1 + 3.0) * 0.3 + vnoise(q * 4.7 + 7.0) * 0.15;
}

// The slope (d/dx, d/dz) of the ripples at `q`.
fn ripple_slope(q: vec2<f32>) -> vec2<f32> {
    let e = 0.07;
    let h = ripples(q);
    return vec2<f32>(ripples(q + vec2<f32>(e, 0.0)) - h, ripples(q + vec2<f32>(0.0, e)) - h) / e;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xz;
    let depth = in.color.r * 4.0;
    let river = in.color.g;
    let ford = in.color.b;
    let sea = in.color.a;
    let course = in.uv;
    let shore = in.uv_b.x;
    let speed = in.world_tangent.w;
    let dir = normalize(in.world_tangent.xz + vec2<f32>(1e-5, 0.0));
    let t = globals.time;

    // Moving patterns: two copies drift along (a breeze on still water,
    // the course on a river), each restarting every 2 s while the other is
    // at full weight, so the motion never jumps (and 2 s divides the hour
    // after which globals.time wraps). A river's patterns live in its own
    // downstream / across coordinates, so they bend with it.
    let period = 2.0;
    let ph0 = fract(t / period);
    let ph1 = fract(t / period + 0.5);
    let w0 = 1.0 - abs(2.0 * ph0 - 1.0);
    let flowing = smoothstep(0.03, 0.15, speed);
    var slope = vec2<f32>(0.0);
    var pattern = 0.0;
    if (flowing < 0.999) {
        let breeze = vec2<f32>(0.16, 0.09);
        let q0 = p - breeze * ph0 * period;
        let q1 = p - breeze * ph1 * period + vec2<f32>(0.43, 0.71);
        slope = mix(ripple_slope(q1), ripple_slope(q0), w0) * (1.0 - flowing);
        pattern = mix(fbm(q1 * 0.6), fbm(q0 * 0.6), w0) * (1.0 - flowing);
    }
    if (flowing > 0.001) {
        let c0 = vec2<f32>(course.x - speed * ph0 * period, course.y);
        let c1 = vec2<f32>(course.x - speed * ph1 * period + 0.43, course.y + 0.71);
        // Ripples a little longer downstream than across.
        let s = mix(ripple_slope(c1 * vec2<f32>(0.7, 1.0)), ripple_slope(c0 * vec2<f32>(0.7, 1.0)), w0) * vec2<f32>(0.7, 1.0);
        let side = vec2<f32>(-dir.y, dir.x);
        slope += (dir * s.x + side * s.y) * flowing;
        // Streaks: long downstream, narrow across.
        let streak = mix(fbm(c1 * vec2<f32>(0.25, 1.4)), fbm(c0 * vec2<f32>(0.25, 1.4)), w0);
        pattern += streak * flowing;
    }
    // The sea: a slow swell rolling in, every 6 s.
    let swell = cos(dot(p, vec2<f32>(0.6, 0.8)) * 0.45 - t * TAU / 6.0);
    slope += vec2<f32>(0.6, 0.8) * swell * 0.35 * sea;
    // Calm at the very edge, where it meets the painted rim.
    let calm = smoothstep(0.05, 0.5, depth);
    let strength = (0.14 + 0.1 * river + 0.2 * ford) * calm;
    pbr.N = normalize(vec3<f32>(-slope.x * strength, 1.0, -slope.y * strength));

    // Colour by depth: clear turquoise over fords and shallows, deep blue
    // in lakes, navy out at sea; rivers a brighter azure.
    let shallow = shallow_water(p);
    let lake = lin(vec3<f32>(0.122, 0.357, 0.659));
    let river_col = lin(vec3<f32>(0.235, 0.608, 0.878));
    let ocean = mix(lin(vec3<f32>(0.090, 0.243, 0.478)), lin(vec3<f32>(0.059, 0.165, 0.341)), smoothstep(1.8, 3.8, depth));
    let d = depth + (fbm(p * 0.15) - 0.5) * 0.35;
    var c = mix(shallow, lake, smoothstep(0.12, 1.2, d));
    c = mix(c, mix(shallow, river_col, smoothstep(0.08, 0.7, d)), river * (1.0 - sea));
    c = mix(c, mix(shallow, ocean, smoothstep(0.15, 1.4, d)), sea);
    // Light dapples.
    c *= 1.0 + 0.12 * (pattern - 0.5) * calm;

    // Foam: thin streaks flowing downstream, broken white water over fords,
    // surf rolling onto the sea's beaches.
    var foam = river * smoothstep(0.68, 0.8, pattern) * smoothstep(0.3, 0.9, speed) * 0.55;
    foam += ford * smoothstep(0.5, 0.68, pattern) * 0.8;
    let surf = smoothstep(0.85, 0.98, sin(shore * 1.3 + t * TAU / 5.0) * 0.5 + 0.5) * (1.0 - smoothstep(1.0, 8.0, shore));
    foam += sea * surf * smoothstep(0.4, 0.62, pattern) * 0.8;
    foam = clamp(foam * calm, 0.0, 1.0);
    let white = lin(vec3<f32>(0.949, 0.973, 0.984));
    c = mix(c, white, foam);

    pbr.material.base_color = vec4<f32>(c, 1.0);
    pbr.material.perceptual_roughness = mix(0.1, 0.6, foam);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    // A touch of sky in the surface, more at grazing angles (Fresnel).
    let fresnel = 0.04 + 0.96 * pow(1.0 - max(dot(pbr.N, pbr.V), 0.0), 5.0);
    out.color = vec4<f32>(mix(out.color.rgb, ground.sky.rgb * 0.8, fresnel * (1.0 - foam) * 0.5 * calm), out.color.a);
    let fog = fog_at(p);
    out.color = vec4<f32>(ground_veil(out.color.rgb, p, fog), out.color.a);
    out.color = unexplored(main_pass_post_lighting_processing(pbr, out.color), fog);
    return out;
}
