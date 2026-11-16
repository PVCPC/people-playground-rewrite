// Unlit/JetEngineShader, from its glcore program. The vertex stage shakes each vertex by a value noise of its local position
// and the time (scaled by the vertex colour's alpha); the fragment stage reads _RGBANoise scrolled by the time, looks the
// result up in _Gradient and shapes it into _ConeCount cones that fade in along u. Blend One One. The vertex colour is the
// SpriteRenderer.color (params[14]); JetEngineBehaviour sets it to (0, 0, 0, intensity). See src/custom_material.rs.
import bevy_sprite_render::mesh2d::{functions::get_world_from_local, view_bindings::view};

struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var gradient_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var gradient_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var noise_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var noise_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;

struct Vertex {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

// The program's value noise: fract(sin(n) * 43758.5469) at floor(x) and floor(x) + 1, smoothstep between.
fn noise(x: f32) -> f32 {
    let f = floor(x);
    let a = fract(sin(f) * 43758.5469);
    let b = fract(sin(f + 1.0) * 43758.5469);
    let t = fract(x);
    let s = (-t * 2.0 + 3.0) * (t * t);
    return s * (b - a) + a;
}

@vertex
fn vertex(v: Vertex) -> Out {
    let alpha = params[14].w;
    let t = extra.time.w * 25.5300007;
    let shake = (vec2<f32>(noise(-t + v.position.x), noise(t + v.position.y)) * 2.0 - 1.0) * alpha;
    // The program also moves z by -0.02 * alpha, which the orthographic camera does not show.
    let p = vec3<f32>(shake * 0.0199999996 + v.position.xy, v.position.z);
    var out: Out;
    out.clip = view.clip_from_world * get_world_from_local(v.instance) * vec4<f32>(p, 1.0);
    out.uv = v.uv;
    return out;
}

fn read(t: texture_2d<f32>, s: sampler, uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t, s, vec2<f32>(uv.x, 1.0 - uv.y));
}

@fragment
fn fragment(@location(2) in_uv: vec2<f32>) -> @location(0) vec4<f32> {
    let tint = params[0];
    let cones = params[1].x;
    let colour = params[14];
    let uv = vec2<f32>(in_uv.x, 1.0 - in_uv.y) * params[15].xy + params[15].zw;
    let ty = extra.time.y;
    let a = ty * 132.0;
    let k = select(vec2<f32>(-132.0, -0.0075757578), vec2<f32>(132.0, 0.0075757578), a >= -a);
    let scroll = fract(k.y * ty) * k.x;
    let n = read(noise_tex, noise_sampler, vec2<f32>(scroll * 3.0 + uv.x * 0.0400000028, uv.y * 0.100000001 + extra.time.x));
    var r = read(gradient_tex, gradient_sampler, n.xy);
    r = r * ((colour.w * colour.w) * vec4<f32>(0.0, 1.0, 1.0, 0.0) + vec4<f32>(1.0, 0.0, 0.0, 1.0));
    r = r * read(main_tex, main_sampler, uv);
    r = r * tint;
    r = r * (colour.x * 5.0 + 1.0);
    let fade = clamp(uv.x - (-colour.w * 0.5 + 0.5), 0.0, 1.0);
    r = fade * (fade * (fade * (r * fade)));
    let edge = min(abs(uv.y + -0.5) * 0.400000006 + 0.200000003, -abs(uv.y * 2.0 + -1.0) + 1.0) * 2.5;
    r = edge * (r * edge);
    let c = colour.w * cones;
    let w = abs(uv.x * c - floor(uv.x * c + 0.5)) * 2.0;
    r = r * (w * w * w + 1.0);
    return r * colour.w;
}
