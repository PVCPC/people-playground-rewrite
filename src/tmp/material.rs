//! The two TextMeshPro shaders the world-space text uses, as one Material2d over text.wesl (written from the game's glcore programs,
//! `python -I tools/shaders.py glsl 842|873`): `text_material` picks Bitmap or Distance Field from the font material's shader.
//! The mesh carries POSITION (world xyz; the entity's Transform stays identity), UV_0 (the atlas uv), ATTRIBUTE_UV2 (TMP's uv2:
//! x the packed face uv, y the SDF scale) and COLOR (colour32 / 255, as it is: Unity does not convert vertex colours). The font
//! material's `_FaceColor` and `_OutlineColor` are Color properties and made linear here; `_ClipRect` stays as saved. A name the
//! material lacks takes the shader's Properties default (`python -I tools/shaders.py props 842|873`), and `_FaceTex` and
//! `_OutlineTex` are the default "white" with scale 1 and offset 0. Unity's v runs up, so the shader flips v when it samples.
use super::SdfMaterial;
use bevy::{
    asset::embedded_asset,
    mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef, VertexFormat},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPipeline, Material2dPlugin},
};

/// TMP's uv2 (Mesh.uv2): x the packed face uv, y the SDF scale.
pub const ATTRIBUTE_UV2: MeshVertexAttribute = MeshVertexAttribute::new("TmpUv2", 0x5057_0002, VertexFormat::Float32x2);

const BITMAP: &str = "TextMeshPro/Bitmap";
const DISTANCE_FIELD: &str = "TextMeshPro/Distance Field";
const MOBILE_BITMAP: &str = "TextMeshPro/Mobile/Bitmap";
const MOBILE_DISTANCE_FIELD: &str = "TextMeshPro/Mobile/Distance Field";
const SPRITE: &str = "TextMeshPro/Sprite";

/// The name of a font material's shader, from the Shader object's bytes; only the ones the game's fonts draw with (another name stops where text is drawn with it: `text_material`).
pub fn shader_name(shader: &[u8]) -> String {
    let has = |name: &str| shader.windows(name.len()).any(|w| w == name.as_bytes());
    if has(MOBILE_DISTANCE_FIELD) {
        MOBILE_DISTANCE_FIELD.to_string()
    } else if has(MOBILE_BITMAP) {
        MOBILE_BITMAP.to_string()
    } else if has(BITMAP) {
        BITMAP.to_string()
    } else if has(DISTANCE_FIELD) {
        DISTANCE_FIELD.to_string()
    } else if has(SPRITE) {
        SPRITE.to_string()
    } else {
        "other".to_string()
    }
}

const CLIP_RECT: Vec4 = Vec4::new(-32767.0, -32767.0, 32767.0, 32767.0);
const ST: Vec4 = Vec4::new(1.0, 1.0, 0.0, 0.0);

#[derive(Clone, Copy, Default, ShaderType)]
pub struct TextParams {
    face_color: Vec4,
    outline_color: Vec4,
    clip_rect: Vec4,
    face_tex_st: Vec4,
    outline_tex_st: Vec4,
    vertex_offset: Vec2,
    mask_softness: Vec2,
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

/// TextMeshPro/Bitmap or TextMeshPro/Distance Field.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(TextKey)]
pub struct TextMaterial {
    #[uniform(0)]
    params: TextParams,
    #[texture(1)]
    #[sampler(2)]
    atlas: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    face: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    outline: Handle<Image>,
}

#[repr(C)]
#[derive(PartialEq, Eq, Hash, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextKey {
    kind: u32,
}

impl From<&TextMaterial> for TextKey {
    fn from(m: &TextMaterial) -> Self {
        Self { kind: m.params.kind }
    }
}

pub fn text_material(m: &SdfMaterial, atlas: Handle<Image>) -> TextMaterial {
    let f = |name: &str, default: f32| m.floats.get(name).copied().unwrap_or(default);
    let v = |name: &str, default: Vec4| m.colors.get(name).map_or(default, |c| Vec4::from_array(*c));
    let linear = |name: &str, default: [f32; 4]| {
        let c = m.colors.get(name).copied().unwrap_or(default);
        Color::srgba(c[0], c[1], c[2], c[3]).to_linear().to_vec4()
    };
    let kind = match m.shader.as_str() {
        BITMAP => 0,
        DISTANCE_FIELD => 1,
        MOBILE_BITMAP => 2,
        MOBILE_DISTANCE_FIELD => 3,
        SPRITE => 4,
        other => unimplemented!("mod-only: font material shader {other:?}"),
    };
    if kind == 1 {
        for speed in ["_FaceUVSpeedX", "_FaceUVSpeedY", "_OutlineUVSpeedX", "_OutlineUVSpeedY"] {
            if f(speed, 0.0) != 0.0 {
                unimplemented!("mod-only: {DISTANCE_FIELD} {speed}: the program scrolls its textures by _Time.y");
            }
        }
    }
    let has = |k: &str| m.keywords.iter().any(|w| w == k);
    if kind == 2 && (has("UNITY_UI_ALPHACLIP") || has("UNITY_UI_CLIP_RECT")) {
        unimplemented!("mod-only: {MOBILE_BITMAP} with UNITY_UI_ALPHACLIP / UNITY_UI_CLIP_RECT (their programs are not rewritten)");
    }
    if kind == 4 && has("UNITY_UI_ALPHACLIP") {
        unimplemented!("mod-only: {SPRITE} with UNITY_UI_ALPHACLIP (only its no-keyword program is rewritten)");
    }
    // A material keyword the shader does not declare picks no variant (a sprite material's ETC1_EXTERNAL_ALPHA on a font material).
    if kind == 3 && m.keywords.iter().any(|w| ["OUTLINE_ON", "UNDERLAY_ON", "UNDERLAY_INNER", "UNITY_UI_CLIP_RECT", "UNITY_UI_ALPHACLIP"].contains(&w.as_str())) {
        unimplemented!("mod-only: {MOBILE_DISTANCE_FIELD} with keywords {:?} (only its no-keyword program is rewritten)", m.keywords);
    }
    let params = TextParams {
        // Mobile/Bitmap's and Sprite's _Color take the face colour's place.
        face_color: if kind == 2 || kind == 4 { linear("_Color", [1.0; 4]) } else { linear("_FaceColor", [1.0; 4]) },
        outline_color: linear("_OutlineColor", [0.0, 0.0, 0.0, 1.0]),
        clip_rect: v("_ClipRect", CLIP_RECT),
        face_tex_st: ST,
        outline_tex_st: ST,
        vertex_offset: Vec2::new(f("_VertexOffsetX", 0.0), f("_VertexOffsetY", 0.0)),
        mask_softness: Vec2::new(f("_MaskSoftnessX", 0.0), f("_MaskSoftnessY", 0.0)),
        face_dilate: f("_FaceDilate", 0.0),
        outline_softness: f("_OutlineSoftness", 0.0),
        outline_width: f("_OutlineWidth", 0.0),
        weight_normal: f("_WeightNormal", 0.0),
        weight_bold: f("_WeightBold", 0.5),
        scale_ratio_a: f("_ScaleRatioA", 1.0),
        gradient_scale: f("_GradientScale", 5.0),
        scale_x: f("_ScaleX", 1.0),
        scale_y: f("_ScaleY", 1.0),
        sharpness: f("_Sharpness", 0.0),
        flags: has("UNITY_UI_ALPHACLIP") as u32 | ((has("UNITY_UI_CLIP_RECT") as u32) << 1),
        kind,
        diffuse_power: f("_DiffusePower", 1.0),
    };
    TextMaterial { params, atlas, face: Handle::default(), outline: Handle::default() }
}

impl Material2d for TextMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://people_playground_bevy/tmp/text.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/tmp/text.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(_: &Material2dPipeline, descriptor: &mut RenderPipelineDescriptor, layout: &MeshVertexBufferLayoutRef, key: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            ATTRIBUTE_UV2.at_shader_location(2),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(3),
        ])?];
        // Bitmap and Mobile/Bitmap: Blend SrcAlpha OneMinusSrcAlpha. Distance Field: Blend One OneMinusSrcAlpha (its output is premultiplied).
        let src = if !matches!(key.bind_group_data.kind, 1 | 3) { BlendFactor::SrcAlpha } else { BlendFactor::One };
        let c = BlendComponent { src_factor: src, dst_factor: BlendFactor::OneMinusSrcAlpha, operation: BlendOperation::Add };
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState { color: c, alpha: c });
            }
        }
        Ok(())
    }
}

pub fn add(app: &mut App) {
    embedded_asset!(app, "text.wesl");
    app.add_plugins(Material2dPlugin::<TextMaterial>::default());
}
