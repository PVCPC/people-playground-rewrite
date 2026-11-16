// Unlit/RotaryFanShader's fragment stage, from its glcore program: the uv is pixelated to _Pixelation steps and its angle about
// the centre (the program's atan2 polynomial, written out as compiled) makes _FanFrequency blades turning at _Speed; a blade
// lightens the sprite by 0.045 and a gap darkens it by 0.04 (rgb = a * (rgb - k) + k, alpha 1). See src/custom_material.rs.
struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let frequency = params[0].x;
    let speed = params[1].x;
    let pixelation = params[2].x;
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let p = ceil(uv.yx * vec2<f32>(pixelation)) / vec2<f32>(pixelation) + vec2<f32>(-0.5, -0.5);
    var r = 1.0 / max(abs(p.y), abs(p.x));
    r = r * min(abs(p.y), abs(p.x));
    var q = r * r;
    var s = q * 0.0208350997 + -0.0851330012;
    s = q * s + 0.180141002;
    s = q * s + -0.330299497;
    q = q * s + 0.999866009;
    s = q * r;
    s = s * -2.0 + 1.57079637;
    s = select(0.0, s, abs(p.y) < abs(p.x));
    r = r * q + s;
    r = select(0.0, -3.14159274, p.y < -p.y) + r;
    let lo = min(p.y, p.x);
    let hi = max(p.y, p.x);
    let angle = select(r, -r, (hi >= -hi) && (lo < -lo));
    let blade = sin(angle * frequency + extra.time.y * speed);
    let k = select(vec2<f32>(0.0399999991, -0.0399999991), vec2<f32>(0.0450000018, -0.0450000018), 0.0 < blade);
    let c = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    return vec4<f32>(c.www * (vec3<f32>(k.y) + c.xyz) + vec3<f32>(k.x), 1.0);
}
