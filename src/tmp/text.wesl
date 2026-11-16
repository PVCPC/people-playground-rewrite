// TextMeshPro/Sprite (kind 4, glcore program 145), TextMeshPro/Mobile/Distance Field (kind 3, its no-keyword variant; glcore program 911), TextMeshPro/Bitmap (kind 0), TextMeshPro/Distance Field (kind 1) and TextMeshPro/Mobile/Bitmap (kind 2, without its
// ALPHACLIP / CLIP_RECT variants), from their glcore programs (tools/shaders.py glsl 842 / 873 / 847):
// both stages, with the UNITY_UI_ALPHACLIP and UNITY_UI_CLIP_RECT variants as `flags` bits (1 and 2). The SDF program's
// perspective branch (a perspective projection) is not here: the game's camera is orthographic. See material.rs.
import bevy_sprite_render::mesh2d::view_bindings::view;

struct Params {
    face_color: vec4<f32>,
    outline_color: vec4<f32>,
    clip_rect: vec4<f32>,
    face_tex_st: vec4<f32>,
    outline_tex_st: vec4<f32>,
    vertex_offset: vec2<f32>,
    mask_softness: vec2<f32>,
    face_dilate: f32,
    outline_softness: f32,
    outline_width: f32,
    weight_normal: f32,
    weight_bold: f32,
    scale_ratio_a: f32,
    gradient_scale: f32,
    scale_x: f32,
    scale_y: f32,
    sharpness: f32,
    flags: u32,
    kind: u32,
    diffuse_power: f32,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: Params;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var main_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var main_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var face_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var face_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var outline_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var outline_sampler: sampler;

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) uv2: vec2<f32>,
    @location(3) color: vec4<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    // SDF: the threshold of the outline edge, the scale, the threshold of the face edge.
    @location(2) edge: vec4<f32>,
    // The clip rect's distance (xy) and scale (zw).
    @location(3) mask: vec4<f32>,
    // The face and outline texture coordinates.
    @location(4) tex_uv: vec4<f32>,
}

// uv2.x packs the face uv: 4096 * row + column, 512 steps.
fn face_uv(packed: f32) -> vec2<f32> {
    let row = floor(packed * 0.000244140625);
    return vec2<f32>(row, packed - row * 4096.0) * 0.001953125;
}

fn clip_limits() -> vec4<f32> {
    return min(max(params.clip_rect, vec4<f32>(-2.0e10)), vec4<f32>(2.0e10));
}

// The vertices are in world space already (the text's transform applied when the mesh was built).
@vertex
fn vertex(v: Vertex) -> Out {
    var out: Out;
    let screen = view.viewport.zw;
    out.uv = v.uv;
    let rect = clip_limits();
    if params.kind == 4u {
        // TextMeshPro/Sprite: no pixel snap or vertex offset; colour = in_COLOR0 * _Color.
        out.clip = view.clip_from_world * vec4<f32>(v.position, 1.0);
        out.color = v.color * params.face_color;
        return out;
    }
    if params.kind == 2u {
        // Mobile/Bitmap: the same pixel snap; colour = in_COLOR0 * _Color with rgb * _DiffusePower.
        let p = (1.0 * 0.5) / screen + (v.position.xy + params.vertex_offset);
        let clip = view.clip_from_world * vec4<f32>(p, v.position.z, 1.0);
        let half = screen * 0.5;
        let snapped = round(clip.xy / clip.w * half) / half;
        out.clip = vec4<f32>(clip.w * snapped, clip.zw);
        let c = v.color * params.face_color;
        out.color = vec4<f32>(c.xyz * params.diffuse_power, c.w);
        return out;
    }
    if params.kind == 0u {
        // A Vector3 position reaches the program with w = 1.
        let p = v.position.xy + params.vertex_offset + (1.0 * 0.5) / screen;
        let clip = view.clip_from_world * vec4<f32>(p, v.position.z, 1.0);
        let half = screen * 0.5;
        let snapped = round(clip.xy / clip.w * half) / half;
        out.clip = vec4<f32>(clip.w * snapped, clip.zw);
        out.color = v.color * params.face_color;
        out.tex_uv = vec4<f32>(face_uv(v.uv2.x) * params.face_tex_st.xy + params.face_tex_st.zw, 0.0, 0.0);
        let scale = screen * vec2<f32>(view.clip_from_view[0].x, view.clip_from_view[1].y);
        let d = clip.w / abs(scale) + params.mask_softness * 0.25;
        out.mask = vec4<f32>(p * 2.0 - rect.xy - rect.zw, 0.25 / d);
        return out;
    }
    let p = v.position.xy + params.vertex_offset;
    let clip = view.clip_from_world * vec4<f32>(p, v.position.z, 1.0);
    out.clip = clip;
    out.color = v.color;
    let proj = abs(view.clip_from_view[0].xy * screen.x + view.clip_from_view[1].xy * screen.y) * vec2<f32>(params.scale_x, params.scale_y);
    let size = clip.w / proj;
    let mask_size = size + params.mask_softness * 0.25;
    let inv = inverseSqrt(dot(size, size));
    let scale = abs(v.uv2.y) * params.gradient_scale * (params.sharpness + 1.0) * inv;
    let bold = select(0.0, 1.0, 0.0 >= v.uv2.y);
    let weight = (bold * (params.weight_bold - params.weight_normal) + params.weight_normal) * 0.25 + params.face_dilate;
    let dilate = weight * params.scale_ratio_a;
    let half_inv = 0.5 / scale;
    let outline = (1.0 - params.outline_width * params.scale_ratio_a - params.outline_softness * params.scale_ratio_a) * 0.5 - half_inv;
    out.edge = vec4<f32>(outline - dilate * 0.5, scale, half_inv + (0.5 - dilate * 0.5), 0.0);
    out.mask = vec4<f32>(p * 2.0 - rect.xy - rect.zw, 0.25 / mask_size);
    let face = face_uv(v.uv2.x);
    out.tex_uv = vec4<f32>(face * params.face_tex_st.xy + params.face_tex_st.zw, face * params.outline_tex_st.xy + params.outline_tex_st.zw);
    return out;
}

// Unity's v runs up, an Image's down.
fn flip(uv: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(uv.x, 1.0 - uv.y);
}

fn rect_coverage(mask: vec4<f32>) -> f32 {
    if (params.flags & 2u) == 0u {
        return 1.0;
    }
    let r = clamp((params.clip_rect.zw - params.clip_rect.xy - abs(mask.xy)) * mask.zw, vec2<f32>(0.0), vec2<f32>(1.0));
    return r.x * r.y;
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    let atlas = textureSample(main_tex, main_sampler, flip(in.uv));
    let face_sample = textureSample(face_tex, face_sampler, flip(in.tex_uv.xy));
    let outline_sample = textureSample(outline_tex, outline_sampler, flip(in.tex_uv.zw));
    if params.kind == 4u {
        // _TextureSampleAdd is never set (zero).
        return atlas * in.color;
    }
    if params.kind == 2u {
        return vec4<f32>(in.color.xyz, atlas.w * in.color.w);
    }
    let coverage = rect_coverage(in.mask);
    if params.kind == 0u {
        let alpha = atlas.w * in.color.w;
        if (params.flags & 1u) != 0u && alpha * coverage - 0.001 < 0.0 {
            discard;
        }
        return coverage * vec4<f32>(face_sample.xyz * in.color.xyz, alpha);
    }
    if atlas.w - in.edge.x < 0.0 {
        discard;
    }
    let to_face = in.edge.z - atlas.w;
    let outline_width = params.outline_width * params.scale_ratio_a * in.edge.y;
    let half = outline_width * 0.5;
    let mix_face = sqrt(min(outline_width, 1.0)) * clamp(to_face * in.edge.y + half, 0.0, 1.0);
    var outline = outline_sample * params.outline_color;
    outline = vec4<f32>(outline.xyz * outline.w, outline.w);
    let face_alpha = face_sample.w * params.face_color.w;
    let face = vec4<f32>(in.color.xyz * params.face_color.xyz * face_sample.xyz * face_alpha, face_alpha);
    let colour = mix(face, outline, mix_face);
    let soft = params.outline_softness * params.scale_ratio_a * in.edge.y;
    let fade = 1.0 - clamp((soft * 0.5 + to_face * in.edge.y - half) / (soft + 1.0), 0.0, 1.0);
    let shaded = fade * colour;
    if (params.flags & 1u) != 0u && shaded.w * coverage - 0.001 < 0.0 {
        discard;
    }
    return shaded * coverage * in.color.w;
}
