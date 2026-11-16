// Jaap/GlowingMetal's fragment stage, from its glcore program: alpha cutoff at 0.5, then the colour lerped to colour * _GlowColour by _Progress (clamped).
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
struct Extra { time: vec4<f32>, local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var tex0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var samp0: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let glow = params[0];
    let c = textureSample(tex0, samp0, in_uv);
    if (c.w - 0.5 < 0.0) { discard; }
    let p = clamp(params[1].x, 0.0, 1.0);
    let t = c.xyz * glow.xyz * p - c.xyz;
    return vec4<f32>(p * t + c.xyz, c.w);
}
