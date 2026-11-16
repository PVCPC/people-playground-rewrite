//! The Menu scene's "background loading item status" (BackgroundItemLoaderStatusBehaviour) images: the thumbnail Image
//! with Unlit_UIFramesheet (Unlit/UIFrameSheet) and the "mask" Image, sliced, with Unlit_LoadingShader (Unlit/LoadingShader).
//! Both shaders are from their glcore programs (tools/shaders.py glsl), Blend SrcAlpha OneMinusSrcAlpha (the UiMaterial
//! default); _TextureSampleAdd is (0, 0, 0, 0) for these RGBA textures. A Graphic's vertex colour is Image.color times _Color.
use crate::assets::db::{AssetDb, ObjRef};
use bevy::{asset::embedded_asset, prelude::*, reflect::TypePath, render::render_resource::AsBindGroup, shader::ShaderRef};

/// A sprite as a UI mesh samples it: the texture, DataUtility.GetOuterUV and GetInnerUV (x0, y0, x1, y1, Unity's v up)
/// and the border (left, bottom, right, top) in canvas units (Image.pixelsPerUnit: the sprite's over the canvas's
/// referencePixelsPerUnit 100, times m_PixelsPerUnitMultiplier 1).
/// With the Image's material: its _Color and _FPS (0 when it has none).
#[derive(Clone)]
pub struct UvSprite {
    pub image: Handle<Image>,
    pub outer: [f32; 4],
    pub inner: [f32; 4],
    pub border: [f32; 4],
    pub color: [f32; 4],
    pub fps: f32,
}

pub fn uv_sprite(db: &AssetDb, images: &mut Assets<Image>, textures: &mut crate::play::Textures, r: ObjRef, material: ObjRef) -> Option<UvSprite> {
    let m = crate::assets::components::Material::read(db.data(material)).ok()?;
    let s = db.sprite(r).ok()?;
    let t = db.resolve(r.file, s.texture)?;
    let (image, [w, h]) = crate::play::load_texture(db, images, textures, t)?;
    let [x, y, rw, rh] = s.texture_rect;
    let [l, b, rr, tt] = s.border;
    let unit = s.pixels_to_units / 100.0;
    Some(UvSprite {
        image,
        outer: [x / w, y / h, (x + rw) / w, (y + rh) / h],
        inner: [(x + l) / w, (y + b) / h, (x + rw - rr) / w, (y + rh - tt) / h],
        border: s.border.map(|v| v / unit),
        color: m.color("_Color").unwrap_or([1.0; 4]),
        fps: m.float("_FPS").unwrap_or(0.0),
    })
}

/// Unlit/UIFrameSheet. params[0]: _Time.y, _FPS, _MainTex_TexelSize.z / .w; [1] the vertex colour (linear); [2] the outer uv.
#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct FrameSheetMaterial {
    #[uniform(0)]
    pub params: [Vec4; 3],
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
}

impl UiMaterial for FrameSheetMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/ui/ui_frame_sheet.wesl".into()
    }
}

/// Unlit/LoadingShader on Image.Type Sliced (fillCenter). params[0]: _Time.z; [1] the vertex colour (linear); [2] the outer
/// uv; [3] the inner uv; [4] the border over the rect's size (left, bottom, right, top).
#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct LoadingMaterial {
    #[uniform(0)]
    pub params: [Vec4; 5],
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
}

impl UiMaterial for LoadingMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/ui/ui_loading.wesl".into()
    }
}

/// The colour picker's SaturationValueImage: Unlit/SaturationValueShader. params[0].x: _Hue, which
/// ColorpickerDialogBehaviour.UpdateElements and Update set on its material (the dialog's HSV.x).
#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct SaturationValueMaterial {
    #[uniform(0)]
    pub params: [Vec4; 1],
}

impl UiMaterial for SaturationValueMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/ui/ui_saturation_value.wesl".into()
    }
}

/// Where view.rs puts the saturation value image, with its _Hue; `sync_hue` gives it its material.
#[derive(Component, Clone, Copy)]
pub struct SaturationValueImage(pub f32);

fn sync_hue(mut commands: Commands, added: Query<(Entity, &SaturationValueImage), Added<SaturationValueImage>>, mut materials: ResMut<Assets<SaturationValueMaterial>>) {
    for (e, h) in &added {
        let m = materials.add(SaturationValueMaterial { params: [Vec4::new(h.0, 0.0, 0.0, 0.0)] });
        commands.entity(e).insert(MaterialNode(m));
    }
}

/// Where view.rs puts one of the two images; `sync` gives it its material.
#[derive(Component, Clone, Copy)]
pub enum LoadingImage {
    /// The thumbnail: Image.color.
    Thumbnail { color: [f32; 4], workshop: bool },
    /// The mask: Image.color and the rect's size in canvas units.
    Border { color: [f32; 4], size: [f32; 2] },
}

/// The images' materials, made as view.rs spawns them, with the Menu scene's Time.timeSinceLevelLoad each frame.
fn sync(
    mut commands: Commands,
    added: Query<(Entity, &LoadingImage), Added<LoadingImage>>,
    art: Option<Res<super::art::UiArt>>,
    menu: Option<Res<super::MenuAudio>>,
    images: Res<Assets<Image>>,
    mut sheets: ResMut<Assets<FrameSheetMaterial>>,
    mut borders: ResMut<Assets<LoadingMaterial>>,
) {
    let Some(art) = art else { return };
    let t = menu.map_or(0.0, |m| m.level_time) as f32;
    for (e, part) in &added {
        match *part {
            LoadingImage::Thumbnail { color, workshop } => {
                let Some(s) = (if workshop { &art.workshop_thumbnail } else { &art.loading_thumbnail }) else { continue };
                let ratio = images.get(&s.image).map_or(1.0, |i| i.width() as f32 / i.height() as f32);
                let m = sheets.add(FrameSheetMaterial { params: [Vec4::new(t, s.fps, ratio, 0.0), vertex(color, s), Vec4::from_array(s.outer)], texture: s.image.clone() });
                commands.entity(e).insert(MaterialNode(m));
            }
            LoadingImage::Border { color, size } => {
                let Some(s) = &art.loading_border else { continue };
                let [l, b, r, top] = s.border;
                let frac = Vec4::new(l / size[0], b / size[1], r / size[0], top / size[1]);
                let m = borders.add(LoadingMaterial { params: [Vec4::new(2.0 * t, 0.0, 0.0, 0.0), vertex(color, s), Vec4::from_array(s.outer), Vec4::from_array(s.inner), frac], texture: s.image.clone() });
                commands.entity(e).insert(MaterialNode(m));
            }
        }
    }
    for (_, m) in sheets.iter_mut() {
        m.params[0].x = t;
    }
    for (_, m) in borders.iter_mut() {
        m.params[0].x = 2.0 * t;
    }
}

/// The vertex colour (linear) times _Color (a shader Color property: linear).
fn vertex(color: [f32; 4], s: &UvSprite) -> Vec4 {
    crate::custom_material::linear(color) * crate::custom_material::linear(s.color)
}

pub fn add(app: &mut App) {
    // A headless app (the ui tests) has no asset server to draw with.
    if !app.world().contains_resource::<bevy::asset::io::embedded::EmbeddedAssetRegistry>() {
        return;
    }
    embedded_asset!(app, "ui_frame_sheet.wesl");
    embedded_asset!(app, "ui_loading.wesl");
    embedded_asset!(app, "ui_saturation_value.wesl");
    app.add_plugins((UiMaterialPlugin::<FrameSheetMaterial>::default(), UiMaterialPlugin::<LoadingMaterial>::default(), UiMaterialPlugin::<SaturationValueMaterial>::default())).add_systems(PostUpdate, (sync, sync_hue));
}
