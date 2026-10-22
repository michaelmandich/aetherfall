// A shader module other shaders import: `#import zz_api_probe::common::wave`.
// Its `Shader` asset must be loaded (and kept) before importers compile.
#define_import_path zz_api_probe::common

#import bevy_pbr::mesh_view_bindings::globals

// A slow wave in 0..1. globals.time wraps after an hour: periods that
// divide 3600 s never jump.
fn wave(p: vec2<f32>) -> f32 {
    return sin(p.x * 1.3 + p.y * 0.7 + globals.time * 6.2831853 / 4.0) * 0.5 + 0.5;
}
