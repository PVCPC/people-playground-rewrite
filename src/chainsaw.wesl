// Unlit/ChainsawShader's fragment stage, from its glcore program: alpha cutoff at 0.5 on _MainTex; its red times _ChainFreq minus
// _Position is the u at which _Chain is read (v 0), times the vertex colour. See src/custom_material.rs.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var chain_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var chain_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let position = params[0].x;
    let freq = params[1].x;
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let c = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    let x = select(0.0, c.x * freq + (-position), 0.0 < c.w);
    let chain = textureSample(chain_tex, chain_sampler, vec2<f32>(x, 1.0));
    if (c.w + -0.5 < 0.0) { discard; }
    return chain * params[14];
}
