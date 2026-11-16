// Unlit/LED Display Shader's fragment stage, from its glcore program: _LEDMultiply is a strip of square frames (its width over
// its height, rounded, frames) played at _FPS; the uv rounded to _Pixelation steps picks the texel of the current frame, which
// multiplies the sprite times _Tint. See src/custom_material.rs.
struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var led_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var led_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let pixelation = params[0].x;
    let fps = params[1].x;
    let tint = params[2];
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let size = vec2<f32>(textureDimensions(led_tex));
    // _LEDMultiply_TexelSize.z / .w; WGSL's round is round-half-to-even like roundEven.
    let a = round(vec2<f32>(extra.time.y * fps, size.x / size.y));
    var m = a.y * a.x;
    m = select(-a.y, a.y, m >= -m);
    let frame = fract((1.0 / m) * a.x) * m;
    let p = round(uv * vec2<f32>(pixelation)) / vec2<f32>(pixelation);
    let at = vec2<f32>(frame * (1.0 / a.y) + p.x / a.y, p.y);
    let led = textureSample(led_tex, led_sampler, vec2<f32>(at.x, 1.0 - at.y));
    let c = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y)) * tint;
    return led * c;
}
