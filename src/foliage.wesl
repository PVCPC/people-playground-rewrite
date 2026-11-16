// Unlit/FoliageShader (the bushes' and trees' leaves, FoliageBehaviour), from its glcore program. The vertex stage sways each
// vertex by a value noise of its world position and the time; the fragment stage cuts a ragged round leaf shape that shrinks
// as _BurnProgress (FoliageBehaviour's property block) grows, burnt spots going a fifth dark plus a flickering _FireColour,
// all darkened by (1 - _BurnProgress)^3 (at least 0.05) and tinted by _LeafTint. No vertex colour. Blend SrcAlpha
// OneMinusSrcAlpha. See src/custom_material.rs.
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
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    // vs_TEXCOORD0: the Unity uv through _MainTex_ST.
    @location(0) uv0: vec2<f32>,
}

// fract(sin(n) * 43758.5469) at floor(x) and floor(x) + 1, smoothstep between, as the program writes it.
fn noise(x: f32) -> f32 {
    let fl = floor(x);
    let f = fract(x);
    let a = fract(sin(fl) * 43758.5469);
    let b = fract(sin(fl + 1.0) * 43758.5469);
    let s = (-f * 2.0 + 3.0) * (f * f);
    return s * (b - a) + a;
}

@vertex
fn vertex(v: Vertex) -> Out {
    // params: _LeafTint, _SwayIntensity, _SwaySpeed, _BurnProgress, _FireColour.
    let intensity = params[1].x;
    let speed = params[2].x;
    let world_from_local = get_world_from_local(v.instance);
    let w = (world_from_local * vec4<f32>(v.position, 1.0)).xy;
    let t = extra.time.y * speed;
    let n = vec2<f32>(noise(w.x * 2.20000005 - t), noise(w.y * 2.20000005 + t));
    let p = (n * 2.0 - 1.0) * vec2<f32>(intensity) + v.position.xy;
    var out: Out;
    out.clip = view.clip_from_world * world_from_local * vec4<f32>(p, v.position.z, 1.0);
    let st = params[15];
    out.uv0 = vec2<f32>(v.uv.x, 1.0 - v.uv.y) * st.xy + st.zw;
    return out;
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    let tint = params[0];
    let burn = params[3].x;
    let fire_colour = params[4];
    let uv = in.uv0;
    let a = -uv + vec2<f32>(0.5);
    let r = max(-dot(a, a) * 8.0 + 1.0, 0.0);
    let q = round(uv.xyx * vec3<f32>(47.5785255));
    let qxy = q.xy * vec2<f32>(6.80000019);
    let n9 = q.z * 578.836426 - extra.time.z;
    let h = fract(sin(dot(qxy, vec2<f32>(70.9897995, 78.2330017))) * 43758.5469);
    let b2 = burn * burn;
    if r + (-b2 * 5.0 + h * 3.0) < 0.0 {
        discard;
    }
    let fl = floor(n9);
    let f = fract(n9);
    let s = (-f * 2.0 + 3.0) * (f * f);
    let nn = fract(sin(vec2<f32>(fl, fl + 1.0)) * vec2<f32>(43758.5469));
    let flicker = s * (nn.y - nn.x) + nn.x;
    var fire = vec3<f32>(flicker) * fire_colour.xyz;
    let edge = burn - floor(0.5 + burn);
    fire = fire * vec3<f32>(abs(edge) + abs(edge));
    // The texture as Unity reads it (v up).
    let tex = textureSample(main_tex, main_sampler, vec2<f32>(uv.x, 1.0 - uv.y));
    var x = tex.xyz * vec3<f32>(0.200000003) + fire;
    x = -tex.xyz + x;
    let burnt = select(0.0, 1.0, h * 3.0 - burn < 0.0);
    let c = vec3<f32>(burnt) * x + tex.xyz;
    let u1 = 1.0 - burn;
    let k = max(u1 * (u1 * u1), 0.0500000007);
    return vec4<f32>(vec3<f32>(k) * c, tex.w) * tint;
}
