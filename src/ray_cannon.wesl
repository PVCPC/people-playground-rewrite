// Unlit/RayCannonShader (the LineRenderer of the Ray Cannon), from its glcore program: rings of _Frequency / _Stride along u
// moving at _Speed, wobbled by _Noise read at the line's own positions, cut to a beam about v = 0.5 and multiplied with
// _MainTex and _Tint. Blend One One. See src/custom_material.rs and src/lines.rs.
import bevy_sprite_render::mesh2d::{functions::get_world_from_local, view_bindings::view};

struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var noise_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var noise_sampler: sampler;

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
    let frequency = params[0].x;
    let stride = params[1].x;
    let speed = params[2].x;
    let tint = params[3];
    let uv = in.uv * params[15].xy + params[15].zw;
    let tw = extra.time.w;
    let sign = select(-1.0, 1.0, tw >= -tw);
    var x = fract(sign * tw) * sign;
    x = x * speed + uv.x;
    x = (frequency / stride) * x;
    let n = read(noise_tex, noise_sampler, in.given * vec2<f32>(0.200000003, 0.200000003) + vec2<f32>(extra.time.z * 0.100000001));
    let p = (n.xy * vec2<f32>(2.0, 2.0) + vec2<f32>(-1.0, -1.0)) * vec2<f32>(0.100000001, 0.100000001) + vec2<f32>(x, uv.y);
    let s = select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= -p);
    let q = abs(s * fract(s * p)) + vec2<f32>(-0.5, -0.5);
    let ring = -sqrt(dot(q, q)) * 2.0 + 1.0;
    let c = read(main_tex, main_sampler, p);
    let a = -abs(p.y + -0.5) * 3.0 + 1.0;
    let a3 = a * (a * a);
    let ea = exp2(a3 * 2.88539004);
    let er = exp2(ring * 2.88539004);
    let m = max(ring * 0.5, 0.0);
    var v = (ring * er + a3 * ea) / (ea + er);
    v = clamp(v, 0.0, 1.0);
    v = clamp((v + -0.5) * 9.99999809, 0.0, 1.0);
    v = (v * -2.0 + 3.0) * (v * v) + m * (m * m);
    return vec4<f32>(vec3<f32>(v) * c.xyz, c.w) * tint;
}
