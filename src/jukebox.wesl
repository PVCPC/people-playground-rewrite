// Jaap/Jukebox's fragment stage, from its glcore program: a sprite pixel under half alpha is discarded; where the _GlowMask's red is
// above 0 it is the _Gradient (scrolled by 2 _Time.x along the uv's y) times the mask times _BaseColour times
// max(0.01 (0.5 sin(t) + 0.5), _MusicVolume), elsewhere the sprite; alpha 1. See src/custom_material.rs for `params`.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
struct Extra { time: vec4<f32>, local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var mask_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var mask_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var gradient_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var gradient_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let base_colour = params[0];
    let music_volume = params[1].x;
    // Unity's uv (v up; the mesh and the images run v down).
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let sprite = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    let mask = textureSample(mask_tex, mask_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    // _SinTime.w = sin(t).
    let level = max((sin(extra.time.y) * 0.5 + 0.5) * 0.00999999978, music_volume);
    let g = textureSample(gradient_tex, gradient_sampler, vec2<f32>(extra.time.x * 2.0 + uv.y, 1.0));
    if sprite.w + -0.5 < 0.0 {
        discard;
    }
    let glow = vec3<f32>(level) * (g.xyz * (mask.xyz * base_colour.xyz));
    return vec4<f32>(select(sprite.xyz, glow, 0.0 < mask.x), 1.0);
}
