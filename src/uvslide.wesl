// Unlit/UvSlideSprite's fragment stage, from its glcore program: _MainTex at uv * _Transform.xy + lerp(_OffsetA, _OffsetB, _Offset), times _Tint.
// See src/custom_material.rs for `params`.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
struct Extra { time: vec4<f32>, local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var tex0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var samp0: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let offset_a = params[0].xy;
    let offset_b = params[1].xy;
    let transform = params[2].xy;
    let slide = params[3].x;
    let tint = params[4];
    let uv = in_uv * params[15].xy + params[15].zw;
    let p = uv * transform + (slide * (offset_b - offset_a) + offset_a);
    return textureSample(tex0, samp0, p) * tint;
}
