// Decor models: their own material (base colour texture, cutout), lit,
// then under the fog of war and greyed off the map like the ground.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}
#import af_render::terrain_common::{ground_veil, fog_at, unexplored}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    pbr.material.base_color = alpha_discard(pbr.material, pbr.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    let fog = fog_at(in.world_position.xz);
    out.color = vec4<f32>(ground_veil(out.color.rgb, in.world_position.xz, fog), out.color.a);
    out.color = unexplored(main_pass_post_lighting_processing(pbr, out.color), fog);
    return out;
}
