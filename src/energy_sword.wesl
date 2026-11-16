// Unlit/EnergySword (the energy swords' blade overlay sprite and EnergySwordMotionBlur's trail mesh), from its glcore program:
// three animated cellular noises of the position * 0.5 (shifted by 5% of _OverlayTexture there) at scales 4, 8 and 16, summed
// and shaped into a glow of _GlowColour, times _MainTex's alpha and the vertex colour. Blend One One. A sprite's vertex
// colour is its SpriteRenderer.color (params[14]); the trail's mesh (src/energy_blur.rs) gives its own, with LINE_LOCAL its
// in_POSITION0. See src/custom_material.rs.
import bevy_sprite_render::mesh2d::{functions::get_world_from_local, view_bindings::view};

struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var overlay_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var overlay_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @if(LINE_LOCAL)
    @location(4) color: vec4<f32>,
    @if(LINE_LOCAL)
    @location(5) given: vec2<f32>,
}

// vs_TEXCOORD0 (the Unity uv through _MainTex_ST), vs_TEXCOORD1 (in_POSITION0.xy * 0.5) and vs_COLOR0.
struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) uv1: vec2<f32>,
    @location(2) color: vec4<f32>,
}

@vertex
fn vertex(v: Vertex) -> Out {
    var out: Out;
    out.clip = view.clip_from_world * get_world_from_local(v.instance) * vec4<f32>(v.position, 1.0);
    let st = params[15];
@if(LINE_LOCAL) {
    // The mesh's uv is Unity's.
    out.uv0 = v.uv * st.xy + st.zw;
    out.uv1 = v.given * vec2<f32>(0.5, 0.5);
    out.color = v.color;
}
@if(!LINE_LOCAL) {
    out.uv0 = vec2<f32>(v.uv.x, 1.0 - v.uv.y) * st.xy + st.zw;
    out.uv1 = (v.uv * extra.local.xy + extra.local.zw) * vec2<f32>(0.5, 0.5);
    out.color = params[14];
}
    return out;
}

fn read(t: texture_2d<f32>, s: sampler, uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t, s, vec2<f32>(uv.x, 1.0 - uv.y));
}

// One of the program's three unrolled loops: the smallest squared distance (from 8) to the cell points of the 3x3 cells
// around floor(x), each point moving with sin(hash * 6.2831 + _Time.y); its square root.
fn cells(x: vec2<f32>) -> f32 {
    let cell = floor(x);
    let f = fract(x);
    var m = 8.0;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let g = vec2<f32>(f32(i), f32(j));
            let c = cell + g;
            let h = fract(sin(vec2<f32>(dot(c, vec2<f32>(127.099998, 311.700012)), dot(c, vec2<f32>(269.5, 183.300003)))) * vec2<f32>(18.5452995));
            var d = sin(h * vec2<f32>(6.28310013) + extra.time.yy);
            d = d * vec2<f32>(0.5) + (-f + g);
            d = d + vec2<f32>(0.5);
            let q = dot(d, d);
            m = select(m, q, q < m);
        }
    }
    return sqrt(m);
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    let glow = params[0];
    let main = read(main_tex, main_sampler, in.uv0);
    let o = read(overlay_tex, overlay_sampler, in.uv1);
    let p = o.xy * vec2<f32>(0.0500000007) + in.uv1;
    var s = cells(p * vec2<f32>(4.0)) + cells(p * vec2<f32>(8.0) + vec2<f32>(1.23399997));
    s = cells(p * vec2<f32>(16.0) + vec2<f32>(4.21309996)) + s;
    let third = s * 0.333333343;
    var v = (-s * 0.333333343 + 0.5) * 0.5 + third;
    v = v * v;
    v = v * v;
    let c = vec4<f32>(v) * glow;
    return vec4<f32>(main.w) * c * in.color;
}
