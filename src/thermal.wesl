// Unlit/ThermalVisionReplacementShader's fragment stage, disassembled: the RG16 heat texel's heat times its alpha picks
// the gradient's colour at v 0, everything times that alpha. Blend OneMinusDstColor One.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var heat: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var heat_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var gradient: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var gradient_sampler: sampler;

@fragment
fn fragment(@location(2) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let h = textureSample(heat, heat_sampler, uv);
    // Both images run top row first: the shader's v 0 is the gradient's bottom row, here v 1.
    let g = textureSample(gradient, gradient_sampler, vec2<f32>(h.g * h.r, 1.0));
    return h.g * vec4<f32>(g.rgb, h.g);
}
