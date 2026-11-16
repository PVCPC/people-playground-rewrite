// Unlit/TempRayShader (the LineRenderer of the Heatray and Freezeray), from its glcore program: _MainTex read along a sine
// wave of u (_Frequency, _Speed, growing with u), a hash of the uv for grain, and a cos(v - 0.5)^50 core, times _Tint and the
// vertex colour. Blend One One. See src/custom_material.rs and src/lines.rs.
import bevy_sprite_render::mesh2d::{functions::get_world_from_local, view_bindings::view};

struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) given: vec2<f32>,
}

// A LineRenderer's uv is Unity's (u along the line); `given` is the vertex as the line gives it (in_POSITION0).
struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) given: vec2<f32>,
    @location(6) screen: vec4<f32>,
}

@vertex
fn vertex(v: Vertex) -> Out {
    var out: Out;
    out.clip = view.clip_from_world * get_world_from_local(v.instance) * vec4<f32>(v.position, 1.0);
    out.uv = v.uv;
    out.color = v.color;
    out.given = v.given;
    // The glcore program's screen position: clip.xy * 0.5 + clip.w * 0.5, then clip.zw.
    out.screen = vec4<f32>(out.clip.xy * 0.5 + vec2<f32>(out.clip.w * 0.5), out.clip.zw);
    return out;
}

fn read(t: texture_2d<f32>, s: sampler, uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t, s, vec2<f32>(uv.x, 1.0 - uv.y));
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    let tint = params[0];
    let frequency = params[1].x;
    let speed = params[2].x;
    let uv = in.uv * params[15].xy + params[15].zw;
    let tz = extra.time.z;
    let sign = select(-1.0, 1.0, tz >= -tz);
    let wrapped = fract(sign * tz) * sign;
    let wave = sin(tz * speed + (-(uv.x * frequency))) * uv.x;
    let at = vec2<f32>(uv.x, wave * 0.25 + uv.y);
    var h = at * vec2<f32>(249.339996, 249.339996) + vec2<f32>(wrapped, 0.0);
    let c = read(main_tex, main_sampler, at);
    let z = (at.y + -0.5) * 3.14159274;
    h = h * vec2<f32>(0.103100002, 0.103100002);
    let core = exp2(log2(max(cos(z), 0.0)) * 50.0);
    let coloured = c * tint;
    h = fract(h);
    let d = dot(h.xyx, h.yxx + vec3<f32>(33.3300018, 33.3300018, 33.3300018));
    h = vec2<f32>(d) + h;
    let grain = fract(h.x * (h.y + h.x));
    return vec4<f32>(core) * (vec4<f32>(grain) * coloured) * in.color;
}
