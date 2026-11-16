// Jaap/ForcefieldShader (the ImmobilityField's capture circle), from its glcore program: a GrabPass "_BackgroundTexture"
// (src/grab_pass.rs) refracted by _NormalTexture at screen scale, divided by _Gradient, tinted from _Left to _Right across the
// screen and scaled by 0.2. Alpha-tested at 0.5 on _MainTex. Blend One Zero. See src/custom_material.rs.
import bevy_sprite_render::mesh2d::{functions::get_world_from_local, view_bindings::view};

struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
    screen: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var normal_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var background_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var background_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var gradient_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var gradient_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

// vs_TEXCOORD0 (the Unity uv through _NormalTexture_ST) and vs_TEXCOORD1 (ComputeGrabScreenPos without the flip: GL's
// bottom-up screen position, xy = clip.xy * 0.5 + clip.w * 0.5, zw = clip.zw).
struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) screen: vec4<f32>,
}

@vertex
fn vertex(v: Vertex) -> Out {
    var out: Out;
    let p = view.clip_from_world * get_world_from_local(v.instance) * vec4<f32>(v.position, 1.0);
    out.clip = p;
    let st = params[15];
    out.uv0 = vec2<f32>(v.uv.x, 1.0 - v.uv.y) * st.xy + st.zw;
    let h = p.xyw * vec3<f32>(0.5, 0.5, 0.5);
    out.screen = vec4<f32>(h.zz + h.xy, p.zw);
    return out;
}

// texture() at a GL (bottom-up) coordinate.
fn read(t: texture_2d<f32>, s: sampler, uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t, s, vec2<f32>(uv.x, 1.0 - uv.y));
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    let left = params[0];
    let right = params[1];
    // Every sample before the discard: textureSample needs uniform control flow.
    let main = read(main_tex, main_sampler, in.uv0);
    var u0x = extra.time.x + extra.time.x;
    let u2 = in.screen.xy * extra.screen.xy * vec2<f32>(0.00200000009, 0.00200000009);
    let n = read(normal_tex, normal_sampler, u2);
    var u0 = n.xy * vec2<f32>(0.5, 0.5) + vec2<f32>(u0x, u0x);
    u0 = n.ww * vec2<f32>(3.5, 3.5) + u0;
    let u1 = n.xyw * vec3<f32>(0.00999999978, 0.00999999978, 0.00999999978) + in.screen.xyw;
    let u4 = u1.xy / u1.zz;
    let background = read(background_tex, background_sampler, u4);
    let gradient = read(gradient_tex, gradient_sampler, u0);
    if main.w + -0.5 < 0.0 {
        discard;
    }
    var c = background / gradient;
    let tint = in.screen.xxxx * (-left + right) + left;
    c = c * tint;
    return c * vec4<f32>(0.199999809, 0.199999809, 0.199999809, 0.199999809);
}
