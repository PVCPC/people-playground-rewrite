// Unlit/ResizableSliceShader's fragment stage, from its glcore program: a nine-slice of the sprite by _ObjectScale (the uv times the
// scale, measured from the far edge past the middle, the middle held at 0.5), less a per-texel hash of the vertex position times
// _NoiseIntensity, times _Tint. The vertex colour and _MainTex_ST are not used. See src/custom_material.rs.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
struct Extra { time: vec4<f32>, local: vec4<f32> }
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let tint = params[0];
    let noise = params[1].x;
    let scale = params[2].xy;
    // Unity's uv (v up; the mesh and the images run v down) and the vertex position.
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y);
    let pos = in_uv * extra.local.xy + extra.local.zw;
    var h = fract(floor(pos * scale * 35.0) * 0.627885759);
    let t = h.yxx + vec3<f32>(33.3300018);
    h = h + vec2<f32>(dot(h.xyx, t));
    let n = fract(h.x * (h.y + h.x));
    let far = uv * scale + (-(scale + vec2<f32>(-1.0)));
    var at = select(uv * scale, far, vec2<f32>(0.5) < uv);
    let edge = vec2<f32>(0.5) / scale;
    at = select(at, vec2<f32>(0.5), (edge < uv) & (uv < vec2<f32>(1.0) - edge));
    var c = textureSample(main_tex, main_sampler, vec2<f32>(at.x, 1.0 - at.y));
    c = vec4<f32>(vec3<f32>(-n) * noise + c.xyz, c.w);
    return c * tint;
}
