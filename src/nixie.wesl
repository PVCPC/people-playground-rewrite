// Unlit/nixie_digit_shader's fragment stage, from its glcore program: digit Floor(_DigitIndex) of the ten-wide _Digits strip (its
// red), pulsing with sin(22 _Time.w), times the sprite, the vertex colour (times _Color) twice in alpha, _GlowColour and
// _GlowIntensity; drawn One One. See src/custom_material.rs.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
struct Extra { time: vec4<f32>, local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var digits_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var digits_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let colour = params[14] * params[0];
    let glow_colour = params[1];
    let intensity = params[2].x;
    let index = params[3].x;
    // Unity's uv (v up; the mesh and the images run v down).
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let at = vec2<f32>((floor(index) + uv.x) * 0.100000001, uv.y);
    let d = textureSample(digits_tex, digits_sampler, vec2<f32>(at.x, 1.0 - at.y));
    let pulse = (sin(extra.time.w * 22.0) + 1.0) * 0.25 + 0.5;
    let c = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y)) * colour;
    return vec4<f32>(d.x * pulse) * c * glow_colour * vec4<f32>(intensity) * colour.wwww;
}
