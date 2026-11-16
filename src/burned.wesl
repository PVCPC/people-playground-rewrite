// Unlit/BurnedShader's fragment stage, from its glcore program: _BurnProgress (read at the vertex position * 0.5) gives the burn
// front (x), a flicker (y, with z the phase of a sine of _Time.z) and the char; below _Progress the sprite darkens to black and its
// edge glows red-orange. See src/custom_material.rs for `params`.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
struct Extra { time: vec4<f32>, local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var burn_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var burn_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let progress = params[0].x;
    let pos = (in_uv * extra.local.xy + extra.local.zw) * 0.5;
    // A Unity uv: the images run v down.
    let burn = textureSample(burn_tex, burn_sampler, vec2<f32>(pos.x, 1.0 - pos.y));
    let phase = sin(extra.time.z * 1.5 + burn.z * 512.0) * 0.5 + 0.5;
    var glow = burn.y * phase;
    var front = clamp((burn.x - progress) * (1.0 / (progress * -0.899999976)), 0.0, 1.0);
    front = -(front * -2.0 + 3.0) * (front * front) + 1.0;
    glow = (1.0 - front) * glow;
    glow = glow * glow;
    let c = textureSample(main_tex, main_sampler, in_uv) * params[14];
    return vec4<f32>(glow * 64.0 + front * c.x, glow * 16.0 + front * c.y, front * c.z, c.w);
}
