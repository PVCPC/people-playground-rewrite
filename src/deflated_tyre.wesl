// Unlit/DeflatedTyreShader's fragment stage, from its glcore program: the uv moves along -_Direction by _SagIntensity times
// k * (0.5 - 2k), k the clamped dot of -_Direction with (0.5 - uv); outside the unit square or under _AlphaCutoff it is
// discarded; the colour is the texel times the vertex colour squared, alpha 1. See src/custom_material.rs.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let cutoff = params[0].x;
    let sag = params[1].x;
    let direction = params[2].xy;
    let uv0 = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let k = clamp(dot(-direction, vec2<f32>(0.5) - uv0), 0.0, 1.0);
    let uv = vec2<f32>(sag) * (-direction) * (k * (k * -2.0 + 0.5)) + uv0;
    let edge = min(uv.yx, vec2<f32>(1.0) - abs(uv.yx));
    let c = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    if (edge.x < 0.0) { discard; }
    if (edge.y < 0.0) { discard; }
    if (c.w - cutoff < 0.0) { discard; }
    let tint = params[14];
    return c * vec4<f32>(tint.xyz * tint.xyz, 1.0);
}
