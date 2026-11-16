// The particle shaders the game's systems use: Particles/Standard Unlit (kinds 0 to 2) and Unlit/AlphaCutoff (kind 3).
// The mesh is in world space, its Transform holding only the sort depth; the output is premultiplied, so one blend state takes the materials' blend modes.
import bevy_sprite_render::mesh2d::{functions::{get_world_from_local, get_tag}, view_bindings::view};

struct Settings {
    color: vec4<f32>,
    st: vec4<f32>,
    color_add: vec2<f32>,
    cutoff: f32,
    kind: u32,
    mode: u32,
    blend_frames: u32,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings: Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var sprite: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var sprite_sampler: sampler;
// Every particle copy's draw this frame (particles.rs `particles`): at 4 x the mesh tag the copy's header (vertex start,
// index start, index count), each vertex 12 words (position, uv, colour, uv2 and blend as f32 bits).
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<storage, read> pulled: array<u32>;

struct Vertex {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv2_blend: vec3<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv2_blend: vec3<f32>,
}

@vertex
fn vertex(v: Vertex) -> Out {
    var out: Out;
    out.clip = view.clip_from_world * get_world_from_local(v.instance) * vec4<f32>(v.position, 1.0);
    out.uv = v.uv;
    out.color = v.color;
    out.uv2_blend = v.uv2_blend;
    return out;
}

// A particle copy's vertex `index` (its mesh only numbers the vertices): read from `pulled` and drawn as `vertex` draws the
// mesh's own; past the copy's index count every corner sits at one point, so those triangles draw nothing.
@vertex
fn vertex_pulled(@builtin(instance_index) instance: u32, @location(0) number: vec3<f32>) -> Out {
    let index = u32(number.x);
    let h = 4u * get_tag(instance);
    if index >= pulled[h + 2u] {
        var none: Out;
        none.clip = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return none;
    }
    let at = pulled[h] + 12u * pulled[pulled[h + 1u] + index];
    let w = array<f32, 12>(
        bitcast<f32>(pulled[at]), bitcast<f32>(pulled[at + 1u]), bitcast<f32>(pulled[at + 2u]), bitcast<f32>(pulled[at + 3u]),
        bitcast<f32>(pulled[at + 4u]), bitcast<f32>(pulled[at + 5u]), bitcast<f32>(pulled[at + 6u]), bitcast<f32>(pulled[at + 7u]),
        bitcast<f32>(pulled[at + 8u]), bitcast<f32>(pulled[at + 9u]), bitcast<f32>(pulled[at + 10u]), bitcast<f32>(pulled[at + 11u]),
    );
    var out: Out;
    out.clip = view.clip_from_world * get_world_from_local(instance) * vec4<f32>(w[0], w[1], w[2], 1.0);
    out.uv = vec2<f32>(w[3], w[4]);
    out.color = vec4<f32>(w[5], w[6], w[7], w[8]);
    out.uv2_blend = vec3<f32>(w[9], w[10], w[11]);
    return out;
}

// Unity's v runs up, an Image's down.
fn main_texel(uv: vec2<f32>) -> vec4<f32> {
    let st = uv * settings.st.xy + settings.st.zw;
    return textureSample(sprite, sprite_sampler, vec2<f32>(st.x, 1.0 - st.y));
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4<f32>(c.bg, k.wz), vec4<f32>(c.gb, k.xy), step(c.b, c.g));
    let q = mix(vec4<f32>(p.xyw, c.r), vec4<f32>(c.r, p.yzx), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;
    return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    let p = abs(fract(c.xxx + k.xyz) * 6.0 - k.www);
    return c.z * mix(k.xxx, clamp(p - k.xxx, vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    var tex = main_texel(in.uv);
    if settings.blend_frames != 0u {
        tex = mix(tex, main_texel(in.uv2_blend.xy), in.uv2_blend.z);
    }
    if settings.kind == 3u {
        if tex.r < settings.cutoff {
            discard;
        }
        return vec4<f32>(in.color.rgb * in.color.rgb, 1.0);
    }
    var albedo = tex * settings.color;
    if settings.kind == 1u {
        albedo = vec4<f32>(albedo.rgb + in.color.rgb * settings.color_add.x, albedo.a * in.color.a);
        albedo = vec4<f32>(mix(albedo.rgb, abs(albedo.rgb), settings.color_add.y), albedo.a);
    } else if settings.kind == 2u {
        let a = rgb_to_hsv(albedo.rgb);
        let b = rgb_to_hsv(in.color.rgb);
        albedo = vec4<f32>(hsv_to_rgb(vec3<f32>(b.x, b.y, a.z)), albedo.a * in.color.a);
    } else {
        albedo = albedo * in.color;
    }
    switch settings.mode {
        // Opaque.
        case 0u: {
            return vec4<f32>(albedo.rgb, 1.0);
        }
        // Cutout.
        case 1u: {
            if albedo.a < settings.cutoff {
                discard;
            }
            return vec4<f32>(albedo.rgb, 1.0);
        }
        // Fade.
        case 2u: {
            return vec4<f32>(albedo.rgb * albedo.a, albedo.a);
        }
        // Transparent.
        case 3u: {
            return albedo;
        }
        // Additive, and Subtractive with its reverse-subtract blend.
        default: {
            return vec4<f32>(albedo.rgb * albedo.a, 0.0);
        }
    }
}
