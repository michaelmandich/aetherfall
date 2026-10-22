// Props: vertex-coloured, lit, glowing by the colour's alpha, under the
// fog of war like the ground.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}
#import af_render::terrain_common::{ground_veil, fog_at, unexplored}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    pbr.material.base_color = vec4<f32>(in.color.rgb, 1.0);
    pbr.material.emissive = vec4<f32>(in.color.rgb * in.color.a * 1.5, 0.0);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    let fog = fog_at(in.world_position.xz);
    out.color = vec4<f32>(ground_veil(out.color.rgb, in.world_position.xz, fog), out.color.a);
    out.color = unexplored(main_pass_post_lighting_processing(pbr, out.color), fog);
    return out;
}
