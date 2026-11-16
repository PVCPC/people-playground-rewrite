// Jaap/GlowingSprite's fragment stage, from its glcore program: alpha cutoff at 0.5; where the _GlowMask's red is above 0 the colour
// is lerped from the sprite to mask * _GlowColour by _GlowIntensity (clamped).
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var mask_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var mask_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let glow_colour = params[0];
    let uv = in_uv * params[15].xy + params[15].zw;
    let c = textureSample(main_tex, main_sampler, uv);
    if (c.w - 0.5 < 0.0) { discard; }
    let mask = textureSample(mask_tex, mask_sampler, uv);
    let g = mask * glow_colour;
    let i = clamp(params[1].x, 0.0, 1.0);
    let lit = (1.0 - i) * (c - g * i) + i * g;
    return select(c, lit, 0.0 < mask.x);
}
