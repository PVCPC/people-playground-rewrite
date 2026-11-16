// Unlit/LiquidContainerShader's fragment stage, from its glcore program: a uv outside _UvBounds is discarded; the distance to the
// first _ParticleCount _Particles (local positions, as the vertex position) is blended into one value with exp2(-17.3123398 x)
// weights, and where 1 - that is below _ClipThreshold the fragment is discarded; the rest is _MainTex times _LiquidColor.
// The vertex colour is not used. See src/custom_material.rs (slots: _LiquidColor, _UvBounds, _ClipThreshold, _ParticleCount,
// then _Particles two to a slot).
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
struct Extra { time: vec4<f32>, local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    // Unity's uv (v up; the mesh and the images run v down) through _MainTex_ST, and the vertex position.
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let pos = in_uv * extra.local.xy + extra.local.zw;
    let c = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    let bounds = params[1];
    if (uv.x < bounds.x || uv.y < bounds.y || bounds.z < uv.x || bounds.w < uv.y) {
        discard;
    }
    var x = 1.0;
    let n = i32(params[3].x);
    for (var i = 0; i < n; i++) {
        // Past the 16 SetVectorArray gives, the array holds zeros.
        var p = vec2<f32>(0.0);
        if (i < 20) {
            let s = params[4 + i / 2];
            p = select(s.zw, s.xy, i % 2 == 0);
        }
        let v = p - pos;
        let dist = sqrt(dot(v, v));
        let a = exp2(x * -17.3123398);
        let w = exp2(dist * -17.3123398);
        x = (x * a + dist * w) / (a + w);
    }
    if ((1.0 - x) + (-params[2].x) < 0.0) {
        discard;
    }
    return c * params[0];
}
