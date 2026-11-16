// Unlit/SaturationValueShader's fragment stage, from its glcore program (the Square sprite: opaque, uv 0..1, so the
// clip never discards): the _Hue colour clamp(|6h - 3| - 1, 2 - |6h - 2|, 2 - |6h - 4|), mixed in from white by
// sqrt(u), times v squared; alpha 1. params[0].x: _Hue.
@group(1) @binding(0) var<uniform> params: array<vec4<f32>, 1>;

@fragment
fn fragment(@location(0) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let h = params[0].x;
    // Unity's v runs up; the node's runs down.
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y);
    let hue = clamp(abs(vec3<f32>(h) * 6.0 + vec3<f32>(-3.0, -2.0, -4.0)) * vec3<f32>(1.0, -1.0, -1.0) + vec3<f32>(-1.0, 2.0, 2.0), vec3<f32>(0.0), vec3<f32>(1.0));
    let c = (hue - vec3<f32>(1.0)) * sqrt(uv.x) + vec3<f32>(1.0);
    return vec4<f32>(vec3<f32>(uv.y * uv.y) * c, 1.0);
}
