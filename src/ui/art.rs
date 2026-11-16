//! The UI's pictures and font, read from the game's own assets.

use crate::assets::{components::Sprite as UnitySprite, db::{AssetDb, ObjRef}, mono::Value};
use bevy::prelude::*;
use std::{collections::HashMap, path::Path, sync::OnceLock};

/// The UI's font.
pub static FONT: OnceLock<Handle<Font>> = OnceLock::new();

pub type OutlineRect = (Art, [f32; 4], f32, Color);

/// A sprite: its packed region in the image, and where that sits in the sprite's own rect (fractions).
#[derive(Clone)]
pub struct Art {
    pub image: Handle<Image>,
    pub rect: Rect,
    /// left, top, width, height of the packed region inside the sprite's rect.
    pub inner: [f32; 4],
    /// The sprite's width over its height.
    pub aspect: f32,
}

/// A ToolLibrary.Tool: the name, description, Type and Parent (empty for none) the game's own ToolLibrary holds.
#[derive(Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub ty: String,
    pub parent: String,
}

#[derive(Resource, Default)]
pub struct UiArt {
    /// ToolLibrary.Tools and ToolLibrary.Powers, in order.
    pub tools_list: Vec<ToolDef>,
    pub powers_list: Vec<ToolDef>,
    pub categories: Vec<Option<Art>>,
    pub items: HashMap<String, Art>,
    pub tools: HashMap<String, Art>,
    pub maps: HashMap<String, Art>,
    pub named: HashMap<String, Art>,
    /// SpawnableOutlineRect.prefab (the SpawnableOutline's RectPrefab): its Sliced sprite, the sprite's border in pixels and pixels per unit, and the tint.
    pub outline_rect: Option<OutlineRect>,
    /// TMP's "Bravura SDF" fallback atlas (the piano's sharp and flat signs), its distance field turned into coverage.
    pub bravura: Option<Handle<Image>>,
    /// The catalog's ResizableUIElementHandle.CursorSprite.
    pub resize_cursor: Option<Handle<Image>>,
    /// The Menu scene's UI Sound Controller as it loads (each load of the scene starts from it).
    pub menu_audio: Option<super::MenuAudio>,
    /// The Menu scene's BackgroundItemLoaderStatusBehaviour.DefaultThumbnailSprite and its "mask" Image's sprite.
    pub loading_thumbnail: Option<super::loading::UvSprite>,
    /// The Menu scene's SteamContraptionLoadStep.SteamWorkshopThumbnail (in the same Image).
    pub workshop_thumbnail: Option<super::loading::UvSprite>,
    pub loading_border: Option<super::loading::UvSprite>,
    /// The Menu scene's "Panels" (FitCameraBackgroundBehaviour, PanelsBehaviour): its MenuPanels material and the sprite's bounds size.
    pub menu_panels: Option<(crate::lights::Panels, [f32; 2])>,
}

/// The key of an item button's picture in `UiArt::items`: a spawnable's name, a saved contraption's thumbnail under its Name.
pub fn item_key(item: &super::data::Item) -> String {
    match &item.local {
        Some(m) => format!("local:{}", m.name()),
        None => item.name.clone(),
    }
}

/// ContraptionSpriteStorage.GetFor for a contraption button: its thumbnail as a picture (the PNG's rows run bottom up in `Thumbnail`).
pub fn thumbnail_art(images: &mut Assets<Image>, storage: &mut crate::game::contraption::ContraptionSpriteStorage, m: &crate::game::contraption::ContraptionMetaData) -> Option<Art> {
    let t = storage.GetFor(m)?;
    let (w, h) = (t.width as u32, t.height as u32);
    let row = t.width * 4;
    let px: Vec<u8> = t.rgba.chunks(row).rev().flatten().copied().collect();
    let image = Image::new(bevy::render::render_resource::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, bevy::render::render_resource::TextureDimension::D2, px, bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb, bevy::asset::RenderAssetUsages::default());
    Some(Art { image: images.add(image), rect: Rect::new(0.0, 0.0, w as f32, h as f32), inner: [0.0, 0.0, 1.0, 1.0], aspect: w as f32 / h as f32 })
}

/// The sprites the scenes' UI uses by name.
const NAMED: &[&str] = &["Delete", "eye_open", "eye_closed", "Square", "Details", "ThermalVision", "crosshair", "Time", "Framerate", "SelectionBox"];

pub fn sprite(db: &AssetDb, images: &mut Assets<Image>, textures: &mut crate::play::Textures, r: ObjRef) -> Option<Art> {
    let s = db.sprite(r).ok()?;
    let t = db.resolve(r.file, s.texture)?;
    let (image, size) = crate::play::load_texture(db, images, textures, t)?;
    let [x, y, w, h] = s.texture_rect;
    let [rw, rh] = [s.rect[2], s.rect[3]];
    let [ox, oy] = s.texture_rect_offset;
    Some(Art {
        image,
        // Unity's y runs up; an Image's down.
        rect: Rect::new(x, size[1] - y - h, x + w, size[1] - y),
        inner: [ox / rw, 1.0 - (oy + h) / rh, w / rw, h / rh],
        aspect: rw / rh,
    })
}

fn pptr_sprite(db: &AssetDb, images: &mut Assets<Image>, textures: &mut crate::play::Textures, from: usize, v: &Value, field: &str) -> Option<Art> {
    let r = db.resolve(from, v.get(field)?.pptr()?)?;
    sprite(db, images, textures, r)
}

/// LiberationSans's TrueType data: the Font object holds it as a length-prefixed array.
fn font_data(db: &AssetDb) -> Option<Vec<u8>> {
    for r in of_class(db, 128) {
        let data = db.data(r);
        let name_len = u32::from_le_bytes(data.get(..4)?.try_into().ok()?) as usize;
        if data.get(4..4 + name_len) != Some(b"LiberationSans".as_slice()) {
            continue;
        }
        for at in (0..data.len().saturating_sub(8)).step_by(4) {
            let len = u32::from_le_bytes(data[at..at + 4].try_into().ok()?) as usize;
            if data[at + 4..at + 8] == [0, 1, 0, 0] && at + 4 + len <= data.len() {
                return Some(data[at + 4..at + 4 + len].to_vec());
            }
        }
    }
    None
}

/// The Menu scene's FitCameraBackgroundBehaviour object: its SpriteRenderer's material (Unlit/Jaap/PanelsShader) with the
/// sprite as _MainTex, and sprite.bounds.size (FitCameraBackgroundBehaviour.Awake).
fn menu_panels(db: &mut AssetDb, images: &mut Assets<Image>, textures: &mut crate::play::Textures) -> Option<(crate::lights::Panels, [f32; 2])> {
    let menu = db.file_index("level0")?;
    let fit = db.find_scripts("FitCameraBackgroundBehaviour").ok()?.into_iter().find(|r| r.file == menu)?;
    let (_, mb, _) = db.mono(fit).ok()??;
    let go = db.resolve(menu, mb.game_object)?;
    let prefab = db.prefab(go).ok()?;
    let (sprite, material, alpha) = prefab.objects[0].components.iter().find_map(|(_, c)| match c {
        crate::assets::db::Component::SpriteRenderer(r) => Some((r.sprite, *r.materials.first()?, r.color[3])),
        _ => None,
    })?;
    let sprite = db.resolve(menu, sprite)?;
    let s = db.sprite(sprite).ok()?;
    let (main, _) = crate::play::load_texture(db, images, textures, db.resolve(sprite.file, s.texture)?)?;
    let panels = crate::play::panels_material(db, images, textures, db.resolve(menu, material)?, main, alpha)??;
    Some((panels, [s.rect[2] / s.pixels_to_units, s.rect[3] / s.pixels_to_units]))
}

fn of_class(db: &AssetDb, class_id: i32) -> Vec<ObjRef> {
    db.files.iter().enumerate().flat_map(|(file, f)| f.objects.iter().filter(move |o| o.class_id == class_id).map(move |o| ObjRef { file, path_id: o.path_id })).collect()
}

/// Everything the menus and the HUD draw, once at startup.
pub fn load(game: &Path, images: &mut Assets<Image>, fonts: &mut Assets<Font>) -> Result<UiArt, String> {
    let mut db = AssetDb::open(game)?;
    let db = &mut db;
    super::scene::load(db)?;
    let (audio, ui_sound, listener) = crate::game::scene::menu_ui_sound(db)?;
    let menu_audio = Some(super::MenuAudio { audio, ui_sound, listener, data_dir: db.data_dir.clone(), level_time: 0.0 });
    let catalog = super::data::catalog(db)?;
    let mut art = UiArt::default();
    let mut textures = crate::play::Textures::default();
    let data = font_data(db).ok_or("no LiberationSans font in the game's data")?;
    let _ = FONT.set(fonts.add(Font::from_bytes(data.clone())));
    for c in &catalog.categories {
        art.categories.push(c.icon.and_then(|r| sprite(db, images, &mut textures, r)));
    }
    for i in &catalog.items {
        if let Some(a) = i.view.and_then(|r| sprite(db, images, &mut textures, r)) {
            art.items.insert(i.name.clone(), a);
        }
    }
    for m in db.find_scripts("Map")? {
        let Some((_, mb, v)) = db.mono(m)? else { continue };
        if let Some(a) = pptr_sprite(db, images, &mut textures, m.file, &v, "Thumbnail") {
            art.maps.insert(mb.name, a);
        }
    }
    for lib in db.find_scripts("ToolLibrary")? {
        let Some((_, _, v)) = db.mono(lib)? else { continue };
        for list in ["Tools", "Powers"] {
            for tool in v.get(list).and_then(Value::array).unwrap_or(&[]) {
                let Some(Value::Str(name)) = tool.get("Name") else { continue };
                let field = |f: &str| match tool.get(f) {
                    Some(Value::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                let def = ToolDef { name: name.clone(), description: field("Description"), ty: field("Type"), parent: field("Parent") };
                if list == "Tools" { &mut art.tools_list } else { &mut art.powers_list }.push(def);
                if let Some(a) = pptr_sprite(db, images, &mut textures, lib.file, tool, "Icon") {
                    art.tools.insert(name.clone(), a);
                }
            }
        }
    }
    for r in of_class(db, 213) {
        let Ok(s) = UnitySprite::read(db.data(r)) else { continue };
        if NAMED.contains(&s.name.as_str()) && !art.named.contains_key(&s.name) {
            if let Some(a) = sprite(db, images, &mut textures, r) {
                art.named.insert(s.name, a);
            }
        }
    }
    // ItemButton.prefab's button row icons, by the sprites its Images point at.
    for (key, child) in [("ItemInfo", "View info"), ("ItemWorkshop", "ViewOnSteam"), ("ItemPublish", "Publish")] {
        if let Some(r) = super::scene::asset_ref("ItemButton", &format!("button row/{child}"), "Image", "m_Sprite") {
            if let Some(a) = sprite(db, images, &mut textures, r) {
                art.named.insert(key.into(), a);
            }
        }
    }
    // The Menu scene's logo flash Image.
    if let Some(r) = super::scene::asset_ref("Menu", super::view::LOGO_FLASH, "Image", "m_Sprite") {
        if let Some(a) = sprite(db, images, &mut textures, r) {
            art.named.insert("LogoFlash".into(), a);
        }
    }
    // The Mods page filter's reset button.
    if let Some(r) = super::scene::asset_ref("Menu", "Canvas/Mods/Panel/mod list/viewport/content/Filter/reset", "Image", "m_Sprite") {
        if let Some(a) = sprite(db, images, &mut textures, r) {
            art.named.insert("ModsFilterReset".into(), a);
        }
    }
    // TMP line metrics of the dialog text's font asset (DialogBox prefab, "Text (TMP)").
    if let Some(Ok(Some((_, _, v)))) = super::scene::font_asset("Dialog box", "Text (TMP)").map(|a| db.mono(a)) {
        // m_PointSize is an int in this TMP version, the lines floats.
        let face = |n: &str| v.get("m_FaceInfo").and_then(|f| f.get(n)).and_then(|x| x.f32().or(x.i64().map(|i| i as f32)));
        if let (Some(p), Some(l), Some(a), Some(d)) = (face("m_PointSize"), face("m_LineHeight"), face("m_AscentLine"), face("m_DescentLine")) {
            let _ = super::view::TMP_FACE.set([l / p, a / p, d / p]);
        }
    }
    // The dialog text's TextMeshProUGUI, for ItemButtonBehaviour.ShowInfoDialog's GetPreferredValues.
    if let (Some(v), Some(fa)) = (super::scene::mono("Dialog box", "Text (TMP)", "TextMeshProUGUI"), super::scene::font_asset("Dialog box", "Text (TMP)")) {
        let mut fonts = std::collections::HashMap::new();
        if let (Ok(font), Ok(settings)) = (crate::tmp::FontAsset::read(db, fa, &mut fonts), crate::tmp::TmpSettings::read(db, &mut fonts)) {
            let rect = super::scene::rect("Dialog box", "Text (TMP)");
            if let Ok(p) = crate::tmp::TextProps::read(v, font.clone(), font.material.clone(), rect.size_delta, rect.pivot) {
                let _ = super::view::DIALOG_TMP.set(crate::tmp::TextProps { settings, ..p });
            }
        }
    }
    // <i> runs of the scenes' texts, slanted by the shear of the font asset they use.
    if let Some(asset) = super::scene::font_asset("Menu", "Canvas/Information/Info/version info (1)") {
        if let Ok(Some((_, _, v))) = db.mono(asset) {
            let face = v.get("m_FaceInfo");
            let number = |v: Option<&Value>| v.and_then(|x| x.f32().or(x.i64().map(|i| i as f32)));
            if let (Some(style), Some(cap), Some(points)) = (number(v.get("italicStyle")), number(face.and_then(|f| f.get("m_CapLine"))), number(face.and_then(|f| f.get("m_PointSize")))) {
                let runs: Vec<_> = super::scene::texts_containing("<i>").into_iter().flat_map(|(text, size)| super::view::italic_runs(&text, size)).collect();
                super::italic::load(&data, super::italic::ItalicFont { shear: style * 0.01, cap_line: cap / points }, &runs, images);
            }
        }
    }
    art.outline_rect = outline_rect(db, images, &mut textures);
    art.bravura = bravura(db, images);
    // UIStatViewBehaviour.Icon of each Stats tile, under "stat:<scene>:<tile>".
    for scene in ["Menu", "Main"] {
        for tile in super::state::STAT_TILES {
            if let Some(r) = super::scene::asset_ref(scene, &super::state::stat_tile_path(scene, tile), "UIStatViewBehaviour", "Icon") {
                if let Some(a) = sprite(db, images, &mut textures, r) {
                    art.named.insert(format!("stat:{scene}:{tile}"), a);
                }
            }
        }
    }
    if let Some(t) = super::scene::asset_ref("Main", "Canvas/Catalog/resize handle", "ResizableUIElementHandle", "CursorSprite") {
        art.resize_cursor = crate::play::load_texture(db, images, &mut textures, t).map(|(h, _)| h);
    }
    art.menu_audio = menu_audio;
    art.menu_panels = menu_panels(db, images, &mut textures);
    const STATUS: &str = "Canvas/background loading item status";
    let material = |path: &str| super::scene::asset_ref("Menu", &format!("{STATUS}/{path}"), "Image", "m_Material");
    let thumbnail = (super::scene::asset_ref("Menu", STATUS, "BackgroundItemLoaderStatusBehaviour", "DefaultThumbnailSprite"), material("thumbnail"));
    let border = (super::scene::asset_ref("Menu", &format!("{STATUS}/mask"), "Image", "m_Sprite"), material("mask"));
    if let (Some(r), Some(m)) = thumbnail {
        art.loading_thumbnail = super::loading::uv_sprite(db, images, &mut textures, r, m);
    }
    if let (Some(r), Some(m)) = (super::scene::asset_ref("Menu", "background loader", "SteamContraptionLoadStep", "SteamWorkshopThumbnail"), thumbnail.1) {
        art.workshop_thumbnail = super::loading::uv_sprite(db, images, &mut textures, r, m);
    }
    if let (Some(r), Some(m)) = border {
        art.loading_border = super::loading::uv_sprite(db, images, &mut textures, r, m);
    }
    Ok(art)
}

/// SpawnableOutlineBehaviour.RectPrefab's SpriteRenderer, read from the install.
fn outline_rect(db: &AssetDb, images: &mut Assets<Image>, textures: &mut crate::play::Textures) -> Option<OutlineRect> {
    let at = super::scene::asset_ref("Main", "SpawnableOutline", "SpawnableOutlineBehaviour", "RectPrefab")?;
    let go = db.game_object(at).ok()?;
    let renderer = go.components.iter().filter_map(|&c| db.resolve(at.file, c)).find(|&c| db.class(c) == 212)?;
    let r = crate::assets::components::SpriteRenderer::read(db.data(renderer)).ok()?;
    let s = db.resolve(renderer.file, r.sprite)?;
    let sprite_data = db.sprite(s).ok()?;
    let [red, green, blue, alpha] = r.color;
    Some((sprite(db, images, textures, s)?, sprite_data.border, sprite_data.pixels_to_units, Color::srgba(red, green, blue, alpha)))
}

/// "Bravura SDF Atlas" (Alpha8, 128 x 128): white with alpha from the distance field around its 0.5 edge, a little
/// dilated for the labels' bold style.
fn bravura(db: &AssetDb, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    use crate::assets::components::Texture2D;
    let tex = of_class(db, 28).into_iter().filter_map(|r| Texture2D::read(db.data(r)).ok()).find(|t| t.name == "Bravura SDF Atlas")?;
    let rgba = tex.rgba(&db.data_dir).ok()?;
    let row = tex.width as usize * 4;
    let px: Vec<u8> = rgba.chunks(row).rev().flatten().copied().collect::<Vec<u8>>().chunks(4).flat_map(|p| [255, 255, 255, ((p[3] as f32 / 255.0 - 0.45) * 8.0 + 0.5).clamp(0.0, 1.0).mul_add(255.0, 0.0) as u8]).collect();
    let size = bevy::render::render_resource::Extent3d { width: tex.width as u32, height: tex.height as u32, depth_or_array_layers: 1 };
    let image = Image::new(size, bevy::render::render_resource::TextureDimension::D2, px, bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb, bevy::asset::RenderAssetUsages::default());
    Some(images.add(image))
}
