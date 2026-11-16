// Unlit/UIFrameSheet's fragment stage, from its glcore program: the texture as round(width / height) frames side by side,
// frame round(_Time.y * _FPS) mod that count, times the vertex colour. See loading.rs for `params`.
@group(1) @binding(0) var<uniform> params: array<vec4<f32>, 3>;
@group(1) @binding(1) var main_tex: texture_2d<f32>;
@group(1) @binding(2) var main_sampler: sampler;

@fragment
fn fragment(@location(0) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let outer = params[2];
    // The Simple mesh's uv: the outer uv across the rect (Unity's v up; the node's runs down).
    let uv = vec2<f32>(mix(outer.x, outer.z, in_uv.x), mix(outer.w, outer.y, in_uv.y));
    let r = round(vec2<f32>(params[0].x * params[0].y, params[0].z));
    let n = r.x * r.y;
    let s = select(-r.y, r.y, n >= -n);
    let frame = fract((1.0 / s) * r.x) * s;
    let at = vec2<f32>(frame * (1.0 / r.y) + uv.x / r.y, uv.y);
    let c = textureSample(main_tex, main_sampler, vec2<f32>(at.x, 1.0 - at.y));
    return c * params[1];
}
