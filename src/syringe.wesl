// Unlit/SyringeShader's fragment stage, from its glcore program: the barrel between _CutoffA and lerp(_CutoffA, _CutoffB,
// _LiquidColour.a) (the fill) shows _LiquidColour, with a band (fmod of 2u -/+ 2 _Time.y, as _Direction pushes or pulls) of black
// or white over it; the rest of the sprite stays, the part past _CutoffB at a quarter alpha. See src/custom_material.rs.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
struct Extra { time: vec4<f32>, local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;

// The fmod(v, 1) of HLSL: fract(abs(v)) carrying v's sign.
fn hlsl_fmod1(v: f32) -> f32 {
    let f = fract(abs(v));
    return select(-f, f, v >= -v);
}

// The band: smoothstep of (|fmod(v, 1)| - 0.85) * 20.
fn band(v: f32) -> f32 {
    let x = clamp((abs(hlsl_fmod1(v)) + -0.850000024) * 20.0000191, 0.0, 1.0);
    return x * x * (x * -2.0 + 3.0);
}

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let cutoff_a = params[0].x;
    let cutoff_b = params[1].x;
    let liquid = params[2];
    let direction = params[3].x;
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let u2 = uv.x + uv.x;
    let push = band(extra.time.y * -2.0 + u2);
    let pull = band(extra.time.y * 2.0 + u2);
    let c = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    let k = select(pull, push, 0.0 < direction) * abs(direction) * c.w * 0.25;
    let toward = select(1.0, 0.0, 0.5 < dot(liquid.xyz, vec3<f32>(0.298999995, 0.587000012, 0.114)));
    var col = vec4<f32>(vec3<f32>(k) * (vec3<f32>(toward) + (-liquid.xyz)) + liquid.xyz, 1.0);
    let fill = liquid.w * (cutoff_b - cutoff_a) + cutoff_a;
    let outside = select(0.0, 1.0, uv.x < fill || cutoff_a < uv.x);
    col = vec4<f32>(outside) * (vec4<f32>(1.0) - col) + col;
    col = col * c;
    let alpha = select(0.25, 1.0, uv.x < cutoff_b || fill < uv.x);
    return vec4<f32>(col.xyz, col.w * alpha);
}
