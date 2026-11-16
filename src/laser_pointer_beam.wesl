// Unlit/LaserPointerBeam (the beams of the detectors, Beam Rifle, Sentry Turret and Missile Launcher), from its glcore
// program: _NoiseTexture read at the screen position scrolled by the time, times the vertex colour and _Color, bright in the
// middle of v and fading along u. Blend One One. See src/custom_material.rs and src/lines.rs.
import bevy_sprite_render::mesh2d::{functions::get_world_from_local, view_bindings::view};

struct Extra {
    time: vec4<f32>,
    local: vec4<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: array<vec4<f32>, 16>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> extra: Extra;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var noise_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var noise_sampler: sampler;

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
    let color = params[0];
    let a = -in.uv.yx + vec2<f32>(0.5, 1.0);
    let sign = select(-1.0, 1.0, a.y >= -a.y);
    let across = -abs(a.x) * 2.0 + 1.0;
    let b = vec2<f32>(across, sign) * vec2<f32>(across, a.y);
    let along = fract(b.y) * sign;
    let n = read(noise_tex, noise_sampler, extra.time.zz * vec2<f32>(2.0, 2.0) + in.screen.xy);
    var r = vec4<f32>(n.x) * (in.color * color);
    r = vec4<f32>(b.x) * r;
    return vec4<f32>(along) * r;
}
