// Unlit/GeneratorShader's fragment stage, from its glcore program: where the _Mask's red is at most _Progress (and its alpha
// is above 0) the pixel is _MeterColour * the mask's alpha, elsewhere the sprite. See src/custom_material.rs for `params`.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var mask_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var mask_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let progress = params[0].x;
    let meter_colour = params[1];
    let uv = in_uv * params[15].xy + params[15].zw;
    let mask = textureSample(mask_tex, mask_sampler, uv);
    let meter = mask.wwww * meter_colour;
    let inside = progress >= mask.x && 0.0 < mask.w;
    let sprite = textureSample(main_tex, main_sampler, uv);
    return select(sprite, meter, inside);
}
