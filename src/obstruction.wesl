// Unlit/Obstruction (the beam of the Physics Gun), from its glcore program: _Wiggle read at the line's positions bends the
// uv, three reads of _MainTex scrolled by _Scroll blend (a log-sum-exp) with a pulse of _SinTime.w, shaped by sin(pi v)^6 and a
// fade at the end of u, times _Tint. Blend SrcAlpha OneMinusSrcAlpha. See src/custom_material.rs and src/lines.rs.
import bevy_sprite_render::mesh2d::{functions::get_world_from_local, view_bindings::view};

struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var wiggle_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var wiggle_sampler: sampler;

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
    let scroll = params[1].xy;
    let i = vec4<f32>(in.uv, in.given * vec2<f32>(0.5, 0.5));
    let s0 = extra.time.yy * scroll;
    let w = read(wiggle_tex, wiggle_sampler, i.zw * vec2<f32>(0.5, 0.5) + s0.yx * vec2<f32>(0.300000012, 0.300000012));
    var v = (w.xxy * vec3<f32>(2.0, 2.0, 2.0) + vec3<f32>(-0.5, -0.5, -0.5)) * i.xxx;
    var b = v.yz * vec2<f32>(0.00999999978, 0.00999999978);
    v = v * vec3<f32>(0.100000001, 0.00999999978, 0.00999999978) + i.yzw;
    b = i.zw * vec2<f32>(0.5, 0.5) + b;
    b = -s0 * vec2<f32>(0.5, 0.5) + b;
    let c2 = read(main_tex, main_sampler, v.yz * vec2<f32>(0.25, 0.25) + s0);
    let c0 = read(main_tex, main_sampler, b);
    let pulse = sin(extra.time.y) * 0.5 + 0.5;
    let e0 = exp2((pulse * c0.x) * 37.5100708);
    let c1 = read(main_tex, main_sampler, scroll * extra.time.yy + v.yz);
    let t = clamp(v.x, 0.0, 1.0);
    var s = sin(t * 3.14159274);
    s = s * s;
    let e3 = exp2((c2.x + c1.x) * 18.7550354);
    let l = log2(e0 + e3);
    var q = (s * s) * s;
    q = clamp((-i.x + 1.0) * 12.0, 0.0, 1.0) * q;
    var q4 = q * q;
    q4 = q4 * q4;
    q = -q4 * 0.800000012 + q;
    return vec4<f32>((q * l) * 0.0266595073) * tint;
}
