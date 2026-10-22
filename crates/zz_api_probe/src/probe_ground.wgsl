// ExtendedMaterial<StandardMaterial, GroundExt>'s fragment shader: the
// standard PBR input, changed, lit by Bevy, then post-processed.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}
#import zz_api_probe::common::wave

struct GroundParams {
    tint: vec4<f32>,
};

// The extension's bindings: the material bind group, numbers from 100.
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> params: GroundParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var mask_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var mask_sampler: sampler;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Vertex colour times the base colour, plus every StandardMaterial input.
    var pbr = pbr_input_from_standard_material(in, is_front);
    let w = wave(in.world_position.xz);
    // textureSampleLevel works in non-uniform control flow; textureSample
    // and dpdx need uniform control flow.
    let m = textureSampleLevel(mask_texture, mask_sampler, fract(in.world_position.xz * 0.1), 0.0).a;
    pbr.material.base_color = vec4<f32>(pbr.material.base_color.rgb * params.tint.rgb * (1.0 - params.tint.a * w) * m, 1.0);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    // Distance fog, alpha premultiply (and tonemapping when not HDR).
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    return out;
}
