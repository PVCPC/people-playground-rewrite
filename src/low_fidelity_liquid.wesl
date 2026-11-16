// Unlit/LowFidelityLiquid's fragment stage, from its glcore program: a uv outside _UvBounds or above _Fill is discarded; the rest
// is _MainTex times _LiquidColor (clear below a 0.001 _Fill). The vertex colour is not used. See src/custom_material.rs.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let c = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    let bounds = params[2];
    if (uv.x < bounds.x || uv.y < bounds.y || bounds.z < uv.x || bounds.w < uv.y) {
        discard;
    }
    let fill = params[1].x;
    let col = select(params[0], vec4<f32>(0.0), fill < 0.00100000005);
    if (fill + (-uv.y) < 0.0) {
        discard;
    }
    return col * c;
}
