//! The custom Unity shaders sprites use, behind one layout: a `params` uniform (one vec4 per shader property, in the order the
//! shader's entry lists them; 14 at most, then the SpriteRenderer.color in linear at 14 and `_MainTex_ST` at 15) and up to five
//! textures in the order of its `textures`. Each shader is a .wesl file written from the game's glcore program (tools/shaders.py
//! prints it, with the Blend) and one entry in `custom_shaders!`. Nothing of the game's data is kept here: the material's saved
//! values and textures are read from the install when a sprite is made, and the scripts' MaterialPropertyBlock (game/props.rs)
//! is laid over them every frame.
use crate::game::props::Prop;
use crate::game::visual::SpriteAt;
use crate::play::{load_texture, ObjectSprite, Play, RendererSprite, Textures};
use bevy::{
    asset::embedded_asset,
    mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef, VertexFormat},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, ShaderType, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor, SpecializedMeshPipelineError},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPipeline, Material2dPlugin},
};

/// How a shader declares a property (the Properties block's type): a Color is linear in Unity's linear colour space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Float,
    Vector,
    Color,
    /// A float2 array (SetVectorArray, x and y of each): two elements to a slot, from its own slot up to slot 13. Not a
    /// Property, so 0 until set.
    Vec2Array,
}

/// One shader of `custom_shaders!`.
pub struct ShaderDef {
    /// The Shader object's path id in the install.
    pub path_id: i64,
    /// Name, kind and default (the Properties block's) of each property; the slot is the position.
    pub props: &'static [(&'static str, Kind, [f32; 4])],
    /// The texture properties, in binding order. Their Properties default must be "white".
    pub textures: &'static [&'static str],
    /// The shader reads `_Time`: the material is written again each frame.
    pub timed: bool,
    /// The texture whose `_ST` the vertex stage applies to the uv (slot 15).
    pub st: &'static str,
}

/// What every custom shader is also given: `_Time` (Time.time / 20, the time, twice, three times), the uv to local position
/// map, `_ScreenParams` (the main camera target's width and height, 1 + 1/width, 1 + 1/height) and `unity_OrthoParams` (the
/// main camera's orthographic half width and size, 0, 1).
#[derive(ShaderType, Clone, Copy, Default)]
pub struct Extra {
    pub time: Vec4,
    pub local: Vec4,
    pub screen: Vec4,
    pub ortho: Vec4,
}

/// A LineRenderer's positions as given (world or local, as its useWorldSpace says): the line shaders' `in_POSITION0`, the
/// mesh itself being in world space. Its presence also gives the line shaders their layout (position, uv, colour, this).
pub const LINE_LOCAL: MeshVertexAttribute = MeshVertexAttribute::new("LineLocal", 0x5057_0003, VertexFormat::Float32x2);

/// Marks a LineRenderer drawn with a custom shader (lines.rs): no SpriteRenderer colour or property block, white at slot 14.
#[derive(Component)]
pub struct LineSprite;

pub const TINT_SLOT: usize = 14;
pub const ST_SLOT: usize = 15;

/// A sprite drawn with a custom shader; `custom_sync` fills its material.
#[derive(Component)]
pub struct CustomSprite {
    path_id: i64,
    /// The shader's defaults and the .mat's saved values, per slot.
    base: [Vec4; 14],
    /// The textures (the sprite's own for `_MainTex`, the .mat's, else white).
    tex: [Handle<Image>; 5],
    st: Vec4,
    /// The scale and offset from the mesh's uv to its vertex positions (the shader's `in_POSITION0.xy`).
    local: Vec4,
    applied: Option<(u32, [u32; 4], u32)>,
}

/// GammaToLinearSpace (UnityPlayer 180228fb0) on r, g and b, as a linear-space upload of a Color does: HDR values above 1 take
/// pow(v, 2.2), not the sRGB curve.
pub fn linear(c: [f32; 4]) -> Vec4 {
    let g = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else if v < 1.0 {
            ((v + 0.055) / 1.055).powf(2.4)
        } else if v == 1.0 {
            1.0
        } else {
            v.powf(2.2)
        }
    };
    Vec4::new(g(c[0]), g(c[1]), g(c[2]), c[3])
}

fn slot(kind: Kind, p: &Prop) -> Vec4 {
    match (kind, p) {
        (Kind::Float, Prop::Float(v)) => Vec4::new(*v, 0.0, 0.0, 0.0),
        (Kind::Vector, Prop::Vector(v)) => Vec4::from_array(*v),
        (Kind::Color, Prop::Color(c) | Prop::Vector(c)) => linear(*c),
        // SetInt stores a float (MaterialPropertyBlock.SetInt is SetFloat in 2020.3); an int uniform takes it whole.
        _ => unimplemented!("mod-only: a {p:?} set on a {kind:?} shader property"),
    }
}

/// The renderer's material for `def`, from the .mat `m` of file `file`: the shader's defaults, then the saved values.
pub fn new_sprite(def: &ShaderDef, db: &crate::assets::db::AssetDb, images: &mut Assets<Image>, textures: &mut Textures, file: usize, m: &crate::assets::components::Material, sprite_texture: Handle<Image>, local: Vec4) -> Option<CustomSprite> {
    assert!(def.props.len() <= TINT_SLOT && def.textures.len() <= 5);
    let mut base = [Vec4::ZERO; 14];
    for (i, (name, kind, default)) in def.props.iter().enumerate() {
        base[i] = match kind {
            Kind::Float => Vec4::new(m.float(name).unwrap_or(default[0]), 0.0, 0.0, 0.0),
            // A texture's `_ST` (no Property: the .mat's scale and offset of that texture).
            Kind::Vector if name.ends_with("_ST") => Vec4::from_array(m.textures.iter().find(|t| Some(t.0.as_str()) == name.strip_suffix("_ST")).map_or(*default, |t| t.2)),
            Kind::Vector => Vec4::from_array(m.color(name).unwrap_or(*default)),
            Kind::Color => slot(*kind, &Prop::Color(m.color(name).unwrap_or(*default))),
            Kind::Vec2Array => Vec4::ZERO,
        };
    }
    let mut tex: [Handle<Image>; 5] = Default::default();
    let mut st = Vec4::new(1.0, 1.0, 0.0, 0.0);
    for (i, name) in def.textures.iter().enumerate() {
        let saved = m.textures.iter().find(|(n, ..)| n == name);
        if *name == def.st {
            if let Some((_, _, s)) = saved {
                st = Vec4::from_array(*s);
            }
        }
        if *name == "_MainTex" {
            // The SpriteRenderer sets the sprite's texture; the .mat's scale and offset stay.
            tex[i] = sprite_texture.clone();
        } else if let Some((_, ptr, _)) = saved {
            if let Some(t) = db.resolve(file, *ptr) {
                tex[i] = load_texture(db, images, textures, t)?.0;
            }
        }
    }
    Some(CustomSprite { path_id: def.path_id, base, tex, st, local, applied: None })
}

pub trait Custom: Material2d {
    const DEF: ShaderDef;
    fn new(params: [Vec4; 16], extra: Extra, tex: [Handle<Image>; 5]) -> Self;
    fn set(&mut self, params: [Vec4; 16], extra: Extra, tex: [Handle<Image>; 5]);
}

/// Lays the renderer's material overrides and then its block over the sprite's base, and uploads what changed.
fn custom_sync<T: Custom>(
    mut commands: Commands,
    play: Option<Res<Play>>,
    mut assets: ResMut<Assets<T>>,
    mut images: ResMut<Assets<Image>>,
    mut textures: ResMut<Textures>,
    mut sprites: Query<(Entity, &mut CustomSprite, Option<&ObjectSprite>, Option<&RendererSprite>, Option<&LineSprite>, Option<&MeshMaterial2d<T>>)>,
    grab: Option<Res<crate::grab_pass::GrabTexture>>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    settings: Option<Res<crate::ui::Settings>>,
    camera: Option<Single<&Projection, With<crate::lights::MainCamera>>>,
) {
    // No game yet (the menus): nothing to sync.
    let Some(play) = play else { return };
    // GrabPass "_BackgroundTexture": the frame so far, copied before the first of its users draws (grab_pass.rs).
    let background = T::DEF.textures.iter().position(|&t| t == "_BackgroundTexture");
    // _ScreenParams: the scene target's size (lights::render_size).
    let (w, h) = window.map_or((1.0, 1.0), |w| {
        let (w, h, _) = crate::lights::render_size(&w, settings.as_deref());
        (w as f32, h as f32)
    });
    let screen = Vec4::new(w, h, 1.0 + 1.0 / w, 1.0 + 1.0 / h);
    let size = camera.and_then(|c| if let Projection::Orthographic(o) = *c { Some(o.scale) } else { None }).unwrap_or(1.0);
    let ortho = Vec4::new(size * w / h, size, 0.0, 1.0);
    let g = &play.game;
    let Some(db) = g.assets.as_deref() else { return };
    for (entity, mut sprite, object, renderer, line, material) in &mut sprites {
        if sprite.path_id != T::DEF.path_id {
            continue;
        }
        let (at, color) = match (object, renderer, line) {
            (Some(o), ..) => (Some(SpriteAt::Physical(o.0)), g.physicals[o.0].sprite.map_or([1.0; 4], |s| s.1)),
            (_, Some(r), _) => (Some(SpriteAt::Renderer(r.0)), g.renderers[r.0].sprite.1),
            (.., Some(_)) => (None, [1.0; 4]),
            _ => continue,
        };
        let props = at.and_then(|at| g.props.get(&at));
        let timed = T::DEF.timed.then(|| g.time.time.to_bits()).unwrap_or(0);
        let key = (props.map_or(0, |p| p.version), color.map(f32::to_bits), timed);
        if material.is_some() && sprite.applied == Some(key) {
            continue;
        }
        let mut params = [Vec4::ZERO; 16];
        params[..14].copy_from_slice(&sprite.base);
        params[TINT_SLOT] = linear(color);
        params[ST_SLOT] = sprite.st;
        // ponytail: a version flag in an unused slot of Unlit/BrokenElectronics (1.28.57 changed its program); a shader def if more shaders branch.
        if T::DEF.path_id == 143 && crate::assets::il::is_1_28_45() {
            params[13].w = 1.0;
        }
        let mut tex = sprite.tex.clone();
        for layer in props.iter().flat_map(|p| [&p.material, &p.block]) {
            for (name, value) in &layer.0 {
                if let Some(i) = T::DEF.props.iter().position(|p| p.0 == name) {
                    if let (Kind::Vec2Array, Prop::VectorArray(v)) = (T::DEF.props[i].1, value) {
                        assert!(v.len() <= 2 * (TINT_SLOT - i), "{name}: {} elements past the params", v.len());
                        for (k, e) in v.iter().enumerate() {
                            let s = &mut params[i + k / 2];
                            if k % 2 == 0 {
                                (s.x, s.y) = (e[0], e[1]);
                            } else {
                                (s.z, s.w) = (e[0], e[1]);
                            }
                        }
                        continue;
                    }
                    params[i] = slot(T::DEF.props[i].1, value);
                } else if let Some(i) = T::DEF.textures.iter().position(|t| t == name) {
                    let Prop::Texture(t) = value else { unimplemented!("mod-only: a non-texture {value:?} set on texture property {name}") };
                    let Some((h, _)) = load_texture(db, &mut images, &mut textures, *t) else { continue };
                    tex[i] = h;
                }
                // Else a property the shader doesn't have: Unity ignores it too.
            }
        }
        let t = g.time.time;
        let extra = Extra { time: Vec4::new(t / 20.0, t, t * 2.0, t * 3.0), local: sprite.local, screen, ortho };
        if let (Some(i), Some(grab)) = (background, grab.as_ref()) {
            tex[i] = grab.0.clone();
            commands.entity(entity).insert(crate::grab_pass::GrabPassUser);
        }
        match material {
            Some(m) => {
                if let Some(mut a) = assets.get_mut(&m.0) {
                    a.set(params, extra, tex);
                }
            }
            None => {
                commands.entity(entity).insert(MeshMaterial2d(assets.add(T::new(params, extra, tex))));
            }
        }
        sprite.applied = Some(key);
    }
}

macro_rules! custom_shaders {
    ($($name:ident = $id:literal, $file:literal, $blend:expr, timed $timed:literal, st $st:literal,
       props [$(($p:literal, $k:ident, $def:expr)),* $(,)?], textures [$($t:literal),* $(,)?] $(, vertex $vertex:literal)?;)*) => {
        $(
            #[derive(Asset, TypePath, AsBindGroup, Clone)]
            pub struct $name {
                #[uniform(0)]
                params: [Vec4; 16],
                #[uniform(9)]
                extra: Extra,
                #[texture(1)]
                #[sampler(2)]
                t0: Handle<Image>,
                #[texture(3)]
                #[sampler(4)]
                t1: Handle<Image>,
                #[texture(5)]
                #[sampler(6)]
                t2: Handle<Image>,
                #[texture(7)]
                #[sampler(8)]
                t3: Handle<Image>,
                #[texture(10)]
                #[sampler(11)]
                t4: Handle<Image>,
            }

            impl Material2d for $name {
                fn fragment_shader() -> ShaderRef {
                    concat!("embedded://people_playground_bevy/", $file).into()
                }

                $(
                    /// The shader's own vertex stage, `vertex` in the same file (it moves the vertices).
                    fn vertex_shader() -> ShaderRef {
                        let _ = $vertex;
                        concat!("embedded://people_playground_bevy/", $file).into()
                    }
                )?

                fn alpha_mode(&self) -> AlphaMode2d {
                    if Self::BLEND.is_some() { AlphaMode2d::Blend } else { AlphaMode2d::Opaque }
                }

                fn specialize(_: &Material2dPipeline, descriptor: &mut RenderPipelineDescriptor, layout: &MeshVertexBufferLayoutRef, _: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
                    if layout.0.contains(LINE_LOCAL) {
                        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
                            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
                            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
                            Mesh::ATTRIBUTE_COLOR.at_shader_location(4),
                            LINE_LOCAL.at_shader_location(5),
                        ])?];
                        // A LineRenderer's quads face either way.
                        descriptor.primitive.cull_mode = None;
                        // A shader drawing both sprites and these meshes reads the colour and in_POSITION0 with it.
                        descriptor.vertex.shader_defs.push("LINE_LOCAL".into());
                        if let Some(f) = descriptor.fragment.as_mut() {
                            f.shader_defs.push("LINE_LOCAL".into());
                        }
                    }
                    if let (Some((src, dst)), Some(Some(target))) = (Self::BLEND, descriptor.fragment.as_mut().and_then(|f| f.targets.first_mut())) {
                        let c = BlendComponent { src_factor: src, dst_factor: dst, operation: BlendOperation::Add };
                        target.blend = Some(BlendState { color: c, alpha: c });
                    }
                    Ok(())
                }
            }

            impl $name {
                /// The first pass's RT0 Blend (None: Blend Off), the alpha factors equal to the colour's.
                const BLEND: Option<(BlendFactor, BlendFactor)> = $blend;
            }

            impl Custom for $name {
                const DEF: ShaderDef = ShaderDef { path_id: $id, props: &[$(($p, Kind::$k, $def)),*], textures: &[$($t),*], timed: $timed, st: $st };

                fn new(params: [Vec4; 16], extra: Extra, tex: [Handle<Image>; 5]) -> Self {
                    let [t0, t1, t2, t3, t4] = tex;
                    Self { params, extra, t0, t1, t2, t3, t4 }
                }

                fn set(&mut self, params: [Vec4; 16], extra: Extra, tex: [Handle<Image>; 5]) {
                    *self = Self::new(params, extra, tex);
                }
            }
        )*

        /// The shader a Material uses, when it is one of these.
        pub fn def_for(path_id: i64) -> Option<&'static ShaderDef> {
            $(if path_id == $id { return Some(&<$name as Custom>::DEF); })*
            None
        }

        /// A rebuilt sprite (play.rs resprite) drops its custom material.
        pub fn remove(e: &mut EntityCommands) {
            e.remove::<CustomSprite>();
            $(e.remove::<MeshMaterial2d<$name>>();)*
        }

        pub fn add(app: &mut App) {
            $(
                embedded_asset!(app, $file);
                app.add_plugins(Material2dPlugin::<$name>::default()).add_systems(PostUpdate, custom_sync::<$name>);
            )*
        }
    };
}

use BlendFactor::{One, OneMinusSrcAlpha, SrcAlpha, Zero};

// Name = Shader path id, file, Blend (src, dst) or None, properties (name, kind, Properties default), textures.
custom_shaders! {
    // sharedassets0 Unlit/GeneratorShader
    GeneratorMaterial = 912, "generator.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_MainTex",
        props [("_Progress", Float, [1.0, 0.0, 0.0, 0.0]), ("_MeterColour", Color, [1.0, 0.0, 1.0, 1.0])],
        textures ["_MainTex", "_Mask"];
    // sharedassets0 Jaap/Jukebox
    JukeboxMaterial = 892, "jukebox.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed true, st "_MainTex",
        props [("_BaseColour", Color, [1.0, 0.0, 1.0, 1.0]), ("_MusicVolume", Float, [0.0, 0.0, 0.0, 0.0])],
        textures ["_MainTex", "_GlowMask", "_Gradient"];
    // sharedassets0 Unlit/UvSlideSprite
    UvSlideMaterial = 904, "uvslide.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_MainTex",
        props [("_OffsetA", Vector, [0.0; 4]), ("_OffsetB", Vector, [1.0, 0.0, 0.0, 0.0]), ("_Transform", Vector, [1.0, 1.0, 0.0, 0.0]), ("_Offset", Float, [0.0; 4]), ("_Tint", Color, [1.0, 1.0, 1.0, 0.0])],
        textures ["_MainTex"];
    // sharedassets0 Unlit/DissolveMirror
    DissolveMirrorMaterial = 899, "dissolve_mirror.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_MainTex",
        props [("_MapThreshold", Vector, [0.0, 0.0, 0.5, 0.0]), ("_Progress", Float, [0.0; 4])],
        textures ["_MainTex", "_DissolveMap"];
    // sharedassets0 Jaap/GlowingMetal
    GlowingMetalMaterial = 900, "glowing_metal.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_MainTex",
        props [("_GlowColour", Color, [1.0, 0.0, 1.0, 1.0]), ("_Progress", Float, [1.0, 0.0, 0.0, 0.0])],
        textures ["_MainTex"];
    // sharedassets0 Unlit/BurnedShader
    BurnedMaterial = 882, "burned.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed true, st "_MainTex",
        props [("_Progress", Float, [0.0; 4])],
        textures ["_MainTex", "_BurnProgress"];
    // sharedassets0 Jaap/GlowingSprite
    GlowingSpriteMaterial = 902, "glowing_sprite.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_MainTex",
        props [("_GlowColour", Color, [1.0, 0.0, 1.0, 1.0]), ("_GlowIntensity", Float, [1.0, 0.0, 0.0, 0.0])],
        textures ["_MainTex", "_GlowMask"];
    // sharedassets0 Unlit/MinigunBarrelShader
    MinigunBarrelMaterial = 844, "minigun_barrel.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_ActualSprite",
        props [("_Rotation", Float, [0.0; 4]), ("_GlowColour", Color, [1.0, 0.0, 1.0, 1.0]), ("_Progress", Float, [1.0, 0.0, 0.0, 0.0])],
        textures ["_MainTex", "_ActualSprite", "_Mask"];
    // sharedassets0 Unlit/SyringeShader
    SyringeMaterial = 871, "syringe.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed true, st "_MainTex",
        props [("_CutoffA", Float, [0.1, 0.0, 0.0, 0.0]), ("_CutoffB", Float, [0.1, 0.0, 0.0, 0.0]), ("_LiquidColour", Color, [1.0, 0.0, 1.0, 1.0]), ("_Direction", Float, [0.0; 4])],
        textures ["_MainTex"];
    // sharedassets0 Unlit/WaterShader (WaterBehaviour's water; Blend One Zero; GrabPass "_BackgroundTexture": grab_pass.rs; the
    // _ST of _CausticsTex and _Shimmering are no Property: the .mat's)
    WaterMaterial = 848, "water.wesl", Some((One, Zero)), timed true, st "_MainTex",
        props [("_Tint", Color, [0.0; 4]), ("_DepthColour", Color, [0.0; 4]), ("_CausticsSpeed", Float, [1.0, 0.0, 0.0, 0.0]), ("_Skew", Float, [1.0, 0.0, 0.0, 0.0]),
            ("_DistortIntensity", Float, [1.0, 0.0, 0.0, 0.0]), ("_Brightness", Float, [1.0, 0.0, 0.0, 0.0]), ("_DepthFade", Float, [1.0, 0.0, 0.0, 0.0]),
            ("_DepthMultiplier", Float, [1.0, 0.0, 0.0, 0.0]), ("_Temperature", Float, [18.0, 0.0, 0.0, 0.0]), ("_CausticsTex_ST", Vector, [1.0, 1.0, 0.0, 0.0]),
            ("_Shimmering_ST", Vector, [1.0, 1.0, 0.0, 0.0])],
        textures ["_MainTex", "_IceTexture", "_Shimmering", "_BackgroundTexture", "_CausticsTex"], vertex true;
    // sharedassets0 Unlit/DeflatedTyreShader
    DeflatedTyreMaterial = 874, "deflated_tyre.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_MainTex",
        props [("_AlphaCutoff", Float, [0.5, 0.0, 0.0, 0.0]), ("_SagIntensity", Float, [0.10000000149011612, 0.0, 0.0, 0.0]), ("_Direction", Vector, [0.0, -1.0, 0.0, 0.0])],
        textures ["_MainTex"];
    // sharedassets0 Unlit/ChainsawShader
    ChainsawMaterial = 884, "chainsaw.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_MainTex",
        props [("_Position", Float, [0.0; 4]), ("_ChainFreq", Float, [20.0, 0.0, 0.0, 0.0])],
        textures ["_MainTex", "_Chain"];
    // sharedassets0 Unlit/ResizableSliceShader (_ObjectScale is set by ResizableSliceBehaviour, not a Property)
    ResizableSliceMaterial = 910, "resizable_slice.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_MainTex",
        props [("_Tint", Color, [1.0, 1.0, 1.0, 1.0]), ("_NoiseIntensity", Float, [0.0024999999441206455, 0.0, 0.0, 0.0]), ("_ObjectScale", Vector, [0.0; 4])],
        textures ["_MainTex"];
    // sharedassets0 Unlit/nixie_digit_shader
    NixieMaterial = 148, "nixie.wesl", Some((One, One)), timed true, st "_MainTex",
        props [("_Color", Color, [1.0, 1.0, 1.0, 1.0]), ("_GlowColour", Color, [1.0, 0.0, 1.0, 1.0]), ("_GlowIntensity", Float, [1.0, 0.0, 0.0, 0.0]), ("_DigitIndex", Float, [1.0, 0.0, 0.0, 0.0])],
        textures ["_MainTex", "_Digits"];
    // sharedassets0 Unlit/JetEngineShader (its own vertex stage)
    JetEngineMaterial = 857, "jet_engine.wesl", Some((One, One)), timed true, st "_MainTex",
        props [("_Tint", Color, [1.0, 0.0, 1.0, 1.0]), ("_ConeCount", Float, [5.0, 0.0, 0.0, 0.0])],
        textures ["_MainTex", "_Gradient", "_RGBANoise"], vertex true;
    // sharedassets0 Unlit/RotaryFanShader
    RotaryFanMaterial = 877, "rotary_fan.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed true, st "_MainTex",
        props [("_FanFrequency", Float, [4.0, 0.0, 0.0, 0.0]), ("_Speed", Float, [1.0, 0.0, 0.0, 0.0]), ("_Pixelation", Float, [35.0, 0.0, 0.0, 0.0])],
        textures ["_MainTex"];
    // sharedassets0 Unlit/LED Display Shader
    LedDisplayMaterial = 885, "led_display.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed true, st "_MainTex",
        props [("_Pixelation", Float, [8.0, 0.0, 0.0, 0.0]), ("_FPS", Float, [12.0, 0.0, 0.0, 0.0]), ("_Tint", Color, [1.0, 1.0, 1.0, 1.0])],
        textures ["_MainTex", "_LEDMultiply"];
    // sharedassets0 Unlit/EnergySword (the blade overlay sprites and EnergySwordMotionBlur's trail, energy_blur.rs)
    EnergySwordMaterial = 905, "energy_sword.wesl", Some((One, One)), timed true, st "_MainTex",
        props [("_GlowColour", Color, [1.0, 0.0, 1.0, 1.0])],
        textures ["_MainTex", "_OverlayTexture"], vertex true;
    // sharedassets0 Unlit/FoliageShader (FoliageBehaviour's leaves; its own vertex stage)
    FoliageMaterial = 879, "foliage.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed true, st "_MainTex",
        props [("_LeafTint", Color, [1.0, 0.0, 1.0, 1.0]), ("_SwayIntensity", Float, [0.05000000074505806, 0.0, 0.0, 0.0]), ("_SwaySpeed", Float, [1.0, 0.0, 0.0, 0.0]), ("_BurnProgress", Float, [1.0, 0.0, 0.0, 0.0]), ("_FireColour", Color, [1.0, 0.0, 0.0, 1.0])],
        textures ["_MainTex"], vertex true;
    // sharedassets0 Unlit/TempRayShader (a LineRenderer's, lines.rs)
    TempRayMaterial = 878, "temp_ray.wesl", Some((One, One)), timed true, st "_MainTex",
        props [("_Tint", Color, [1.0, 0.0, 0.0, 1.0]), ("_Frequency", Float, [0.10000000149011612, 0.0, 0.0, 0.0]), ("_Speed", Float, [1.0, 0.0, 0.0, 0.0])],
        textures ["_MainTex"], vertex true;
    // sharedassets0 Unlit/RayCannonShader (a LineRenderer's, lines.rs)
    RayCannonMaterial = 846, "ray_cannon.wesl", Some((One, One)), timed true, st "_MainTex",
        props [("_Frequency", Float, [1.0, 0.0, 0.0, 0.0]), ("_Stride", Float, [1.0, 0.0, 0.0, 0.0]), ("_Speed", Float, [1.0, 0.0, 0.0, 0.0]), ("_Tint", Color, [0.0, 1.0, 1.0, 1.0])],
        textures ["_MainTex", "_Noise"], vertex true;
    // sharedassets0 Unlit/ReactorDisplay (no _MainTex_ST in the program; ReactorA5DisplayBehaviour sets the colours)
    ReactorDisplayMaterial = 845, "reactor_display.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "",
        props [("_Wire1", Color, [1.0; 4]), ("_Wire2", Color, [1.0; 4]), ("_Wire3", Color, [1.0; 4]), ("_Wire4", Color, [1.0; 4]), ("_VacuumChamber", Color, [1.0; 4]), ("_Connector", Color, [1.0; 4]), ("_Base", Color, [1.0; 4])],
        textures ["_MainTex"];
    // sharedassets0 Unlit/LaserPointerBeam (a LineRenderer's, lines.rs; no _MainTex_ST in the program)
    LaserPointerBeamMaterial = 858, "laser_pointer_beam.wesl", Some((One, One)), timed true, st "",
        props [("_Color", Color, [0.0, 1.0, 0.0, 1.0])],
        textures ["_NoiseTexture"], vertex true;
    // sharedassets0 Unlit/Obstruction (a LineRenderer's, lines.rs; no _MainTex_ST in the program)
    ObstructionMaterial = 880, "obstruction.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed true, st "",
        props [("_Tint", Color, [1.0, 1.0, 1.0, 1.0]), ("_Scroll", Vector, [1.0, 0.0, 0.0, 0.0])],
        textures ["_MainTex", "_Wiggle"], vertex true;
    // resources Unlit/BrokenElectronics (DamagableMachineryBehaviour.SetAsBroken's Materials/BrokenMachine; no _MainTex_ST in the program)
    BrokenElectronicsMaterial = 143, "broken_electronics.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "",
        props [("_Progress", Float, [0.0; 4]), ("_GlowColour", Color, [1.0, 0.0, 1.0, 1.0]), ("_Offset", Vector, [0.0; 4])],
        textures ["_MainTex", "_DamageMap", "_BrokenElectronics"];
    // sharedassets0 Jaap/ForcefieldShader (the ImmobilityField's capture circle; Blend One Zero; GrabPass "_BackgroundTexture",
    // which is no Property: grab_pass.rs)
    ForcefieldMaterial = 870, "forcefield.wesl", Some((One, Zero)), timed true, st "_NormalTexture",
        props [("_Left", Color, [1.0; 4]), ("_Right", Color, [1.0; 4])],
        textures ["_MainTex", "_NormalTexture", "_BackgroundTexture", "_Gradient"], vertex true;
    // sharedassets0 Unlit/LiquidContainerShader (LiquidContainerController's block sets all but _MainTex; _Particles last, it takes 8 slots)
    LiquidContainerMaterial = 861, "liquid_container.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_MainTex",
        props [("_LiquidColor", Color, [0.0, 0.25, 1.0, 1.0]), ("_UvBounds", Vector, [0.0, 0.0, 1.0, 1.0]), ("_ClipThreshold", Float, [0.0; 4]), ("_ParticleCount", Float, [0.0; 4]), ("_Particles", Vec2Array, [0.0; 4])],
        textures ["_MainTex"];
    // sharedassets0 Unlit/LowFidelityLiquid (LiquidContainerController's sharedMaterial without FancyEffects)
    LowFidelityLiquidMaterial = 895, "low_fidelity_liquid.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed false, st "_MainTex",
        props [("_LiquidColor", Color, [1.0, 0.0, 1.0, 1.0]), ("_Fill", Float, [1.0, 0.0, 0.0, 0.0]), ("_UvBounds", Vector, [0.0, 0.0, 1.0, 1.0])],
        textures ["_MainTex"];
    // sharedassets0 Unlit/FogShader
    FogMaterial = 906, "fog.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed true, st "_MainTex",
        props [("_SnowDirection", Vector, [0.0, 1.0, 0.0, 0.0]), ("_NoiseDirection", Vector, [1.0, 0.0, 0.0, 0.0]), ("_Intensity", Float, [1.0, 0.0, 0.0, 0.0]), ("_Tiling", Float, [1.0, 0.0, 0.0, 0.0]), ("_TilingN", Float, [1.0, 0.0, 0.0, 0.0]), ("_BlackIsAlpha", Float, [0.0; 4]), ("_Density", Float, [1.0, 0.0, 0.0, 0.0]), ("_Tint", Color, [1.0, 0.0, 1.0, 1.0]), ("_FadeColour", Color, [1.0, 0.0, 1.0, 1.0])],
        textures ["_MainTex", "_Noise"];
    // sharedassets0 Unlit/SnowOverlay
    SnowOverlayMaterial = 843, "snow_overlay.wesl", Some((SrcAlpha, OneMinusSrcAlpha)), timed true, st "_MainTex",
        props [("_SnowDirection", Vector, [0.0, 1.0, 0.0, 0.0]), ("_NoiseDirection", Vector, [1.0, 0.0, 0.0, 0.0]), ("_Intensity", Float, [1.0, 0.0, 0.0, 0.0]), ("_Tiling", Float, [1.0, 0.0, 0.0, 0.0]), ("_TilingN", Float, [1.0, 0.0, 0.0, 0.0]), ("_BlackIsAlpha", Float, [0.0; 4]), ("_Visibility", Float, [1.0, 0.0, 0.0, 0.0]), ("_Density", Float, [1.0, 0.0, 0.0, 0.0]), ("_Tint", Color, [1.0, 0.0, 1.0, 1.0])],
        textures ["_MainTex", "_Noise"];
}

