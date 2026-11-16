//! TextMeshPro for world-space texts (TMPro.TextMeshPro, not the UI's TextMeshProUGUI): the font assets read from the
//! game's data, the component's serialized properties, and the mesh TextMeshPro.GenerateTextMesh builds (layout.rs).
//! The rendering side (play.rs) draws the quads with the font's SDF atlas (shaders/tmp_sdf.wesl).
#![allow(non_snake_case)]

pub mod layout;
pub mod draw;
pub mod material;
pub mod ft;
pub mod dynamic;

use std::collections::HashMap;
use std::sync::Arc;

use crate::assets::components::Material;
use crate::assets::db::{AssetDb, ObjRef};
use crate::assets::classes::PPtr;
use crate::assets::mono::Value;

/// UnityEngine.TextCore.FaceInfo
#[derive(Clone, Debug, Default)]
pub struct FaceInfo {
    pub point_size: i32,
    pub scale: f32,
    pub line_height: f32,
    pub ascent_line: f32,
    pub cap_line: f32,
    pub mean_line: f32,
    pub baseline: f32,
    pub descent_line: f32,
    pub superscript_offset: f32,
    pub superscript_size: f32,
    pub subscript_offset: f32,
    pub subscript_size: f32,
    pub underline_offset: f32,
    pub underline_thickness: f32,
    pub strikethrough_offset: f32,
    pub strikethrough_thickness: f32,
    pub tab_width: f32,
}

/// UnityEngine.TextCore.GlyphMetrics
#[derive(Clone, Copy, Debug, Default)]
pub struct GlyphMetrics {
    pub width: f32,
    pub height: f32,
    pub horizontal_bearing_x: f32,
    pub horizontal_bearing_y: f32,
    pub horizontal_advance: f32,
}

/// UnityEngine.TextCore.Glyph: its metrics, its rect in the atlas, scale and atlas index.
#[derive(Clone, Copy, Debug, Default)]
pub struct Glyph {
    pub index: u32,
    pub metrics: GlyphMetrics,
    /// x, y, width, height in atlas pixels (y up from the bottom, as GlyphRect).
    pub rect: [i32; 4],
    pub scale: f32,
    pub atlas_index: i32,
}

/// TMP_Character: m_Unicode -> m_GlyphIndex, m_Scale.
#[derive(Clone, Copy, Debug, Default)]
pub struct Character {
    pub unicode: u32,
    pub glyph_index: u32,
    pub scale: f32,
    /// AddSynthesizedCharacter's character: its glyph is a private zero-size Glyph(0, ...), not a glyph table entry.
    pub synth: bool,
}

/// TMP_GlyphValueRecord
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlyphValueRecord {
    pub x_placement: f32,
    pub y_placement: f32,
    pub x_advance: f32,
    pub y_advance: f32,
}

impl std::ops::Add for GlyphValueRecord {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self { x_placement: self.x_placement + o.x_placement, y_placement: self.y_placement + o.y_placement, x_advance: self.x_advance + o.x_advance, y_advance: self.y_advance + o.y_advance }
    }
}

/// TMP_GlyphPairAdjustmentRecord: the first and second glyphs' value records and m_FeatureLookupFlags.
#[derive(Clone, Copy, Debug, Default)]
pub struct PairAdjustment {
    pub first: GlyphValueRecord,
    pub second: GlyphValueRecord,
    pub flags: i32,
}

/// The material's ShaderUtilities properties GenerateTextMesh and the SDF shader read (TMP_SDF.shader).
#[derive(Clone, Debug, Default)]
pub struct SdfMaterial {
    pub material: Option<ObjRef>,
    pub floats: HashMap<String, f32>,
    pub colors: HashMap<String, [f32; 4]>,
    /// The shader keywords (OUTLINE_ON, UNDERLAY_ON, ...).
    pub keywords: Vec<String>,
    /// The material shader (material::shader_name).
    pub shader: String,
}

impl SdfMaterial {
    /// The Material at `material_ref` (floats, colors, keywords, shader name).
    pub fn read(db: &mut AssetDb, material_ref: Option<ObjRef>) -> Result<Self, String> {
        let mut material = SdfMaterial { material: material_ref, ..Default::default() };
        if let Some(m) = material_ref {
            let (mat, keywords) = Material::read_with_keywords(db.data(m))?;
            material.floats = mat.floats.into_iter().collect();
            material.colors = mat.colors.into_iter().collect();
            material.keywords = keywords.split_whitespace().map(str::to_string).collect();
            let shader = db.resolve(m.file, mat.shader).ok_or("the font material shader")?;
            material.shader = material::shader_name(db.data(shader));
        }
        Ok(material)
    }
    pub fn has(&self, name: &str) -> bool {
        self.floats.contains_key(name)
    }
    pub fn float(&self, name: &str) -> f32 {
        self.floats.get(name).copied().unwrap_or(0.0)
    }
}

/// TMP_FontAsset, a static atlas (m_AtlasPopulationMode 0; a dynamic one is unimplemented).
#[derive(Clone, Debug, Default)]
pub struct FontAsset {
    pub asset: Option<ObjRef>,
    pub name: String,
    pub face: FaceInfo,
    /// m_GlyphTable by glyph index (TMP_FontAsset.glyphLookupTable).
    pub glyphs: HashMap<u32, Glyph>,
    /// m_CharacterTable by unicode (TMP_FontAsset.characterLookupTable).
    pub characters: HashMap<u32, Character>,
    /// m_FontFeatureTable.m_GlyphPairAdjustmentRecords by (second glyph << 16 | first glyph).
    pub pairs: HashMap<u32, PairAdjustment>,
    /// m_FallbackFontAssetTable
    pub fallbacks: Vec<Arc<FontAsset>>,
    /// m_AtlasPopulationMode is Dynamic: TryAddCharacterInternal rasterizes a missing character into the atlas.
    pub dynamic: bool,
    /// The dynamic atlas state (dynamic.rs), shared by every clone.
    pub state: Option<Arc<dynamic::Dynamic>>,
    pub atlas: Option<ObjRef>,
    pub atlas_width: i32,
    pub atlas_height: i32,
    pub atlas_padding: i32,
    pub normal_style: f32,
    pub normal_spacing_offset: f32,
    pub bold_style: f32,
    pub bold_spacing: f32,
    pub italic_style: u8,
    pub tab_size: u8,
    pub material: SdfMaterial,
    /// fontWeightTable[0..10]: each pair's (regularTypeface, italicTypeface).
    pub weight_typefaces: Vec<(Option<Arc<FontAsset>>, Option<Arc<FontAsset>>)>,
}

fn f(v: &Value, n: &str) -> Result<f32, String> {
    v.get(n).and_then(Value::f32).ok_or_else(|| format!("TMP field {n}"))
}
fn i(v: &Value, n: &str) -> Result<i64, String> {
    v.get(n).and_then(Value::i64).ok_or_else(|| format!("TMP field {n}"))
}

impl FontAsset {
    /// The glyph pair adjustment for `key` (second << 16 | first): the serialized table, then the dynamic asset's runtime records.
    pub fn pair(&self, key: u32) -> Option<PairAdjustment> {
        self.pairs.get(&key).copied().or_else(|| self.state.as_ref().and_then(|d| d.pair(key)))
    }

    /// TMP_Character.glyph.
    pub fn glyph(&self, c: &Character) -> Glyph {
        if c.synth {
            Glyph { index: 0, metrics: GlyphMetrics::default(), rect: [0; 4], scale: 1.0, atlas_index: 0 }
        } else {
            self.glyphs[&c.glyph_index]
        }
    }

    /// TMP_FontAsset at `r`, its fallbacks (read once each, through `cache`) and its material.
    pub fn read(db: &mut AssetDb, r: ObjRef, cache: &mut HashMap<ObjRef, Arc<FontAsset>>) -> Result<Arc<FontAsset>, String> {
        if let Some(a) = cache.get(&r) {
            return Ok(a.clone());
        }
        let (_, mb, v) = db.mono(r)?.ok_or("TMP_FontAsset without its script")?;
        // A dynamic asset (m_AtlasPopulationMode 1) starts from its serialized tables; char_from_font_internal stops where a lookup
        // would grow its atlas from the source font.
        let dynamic = i(&v, "m_AtlasPopulationMode")? != 0;
        let face = v.get("m_FaceInfo").ok_or("m_FaceInfo")?;
        let mut face = FaceInfo {
            point_size: i(face, "m_PointSize")? as i32,
            scale: f(face, "m_Scale")?,
            line_height: f(face, "m_LineHeight")?,
            ascent_line: f(face, "m_AscentLine")?,
            cap_line: f(face, "m_CapLine")?,
            mean_line: f(face, "m_MeanLine")?,
            baseline: f(face, "m_Baseline")?,
            descent_line: f(face, "m_DescentLine")?,
            superscript_offset: f(face, "m_SuperscriptOffset")?,
            superscript_size: f(face, "m_SuperscriptSize")?,
            subscript_offset: f(face, "m_SubscriptOffset")?,
            subscript_size: f(face, "m_SubscriptSize")?,
            underline_offset: f(face, "m_UnderlineOffset")?,
            underline_thickness: f(face, "m_UnderlineThickness")?,
            strikethrough_offset: f(face, "m_StrikethroughOffset")?,
            strikethrough_thickness: f(face, "m_StrikethroughThickness")?,
            tab_width: f(face, "m_TabWidth")?,
        };
        let mut glyphs = HashMap::new();
        for g in v.get("m_GlyphTable").and_then(Value::array).ok_or("m_GlyphTable")? {
            let m = g.get("m_Metrics").ok_or("m_Metrics")?;
            let rect = g.get("m_GlyphRect").ok_or("m_GlyphRect")?;
            let glyph = Glyph {
                index: i(g, "m_Index")? as u32,
                metrics: GlyphMetrics { width: f(m, "m_Width")?, height: f(m, "m_Height")?, horizontal_bearing_x: f(m, "m_HorizontalBearingX")?, horizontal_bearing_y: f(m, "m_HorizontalBearingY")?, horizontal_advance: f(m, "m_HorizontalAdvance")? },
                rect: [i(rect, "m_X")? as i32, i(rect, "m_Y")? as i32, i(rect, "m_Width")? as i32, i(rect, "m_Height")? as i32],
                scale: f(g, "m_Scale")?,
                atlas_index: i(g, "m_AtlasIndex")? as i32,
            };
            glyphs.insert(glyph.index, glyph);
        }
        // TMP_FontAsset.InitializeCharacterLookupDictionary: a character whose glyph is missing is left out.
        let mut characters = HashMap::new();
        for c in v.get("m_CharacterTable").and_then(Value::array).ok_or("m_CharacterTable")? {
            let ch = Character { unicode: i(c, "m_Unicode")? as u32, glyph_index: i(c, "m_GlyphIndex")? as u32, scale: f(c, "m_Scale")?, synth: false };
            if glyphs.contains_key(&ch.glyph_index) {
                characters.entry(ch.unicode).or_insert(ch);
            }
        }
        let file = r.file;
        let atlas = v.get("m_AtlasTextures").and_then(Value::array).and_then(|a| a.first()).and_then(Value::pptr).and_then(|p| db.resolve(file, p));
        let state = if dynamic {
            let src = v.get("m_SourceFontFile").and_then(Value::pptr).and_then(|p| db.resolve(file, p)).ok_or("m_SourceFontFile of a dynamic TMP_FontAsset")?;
            let font = dynamic::font_data(db.data(src))?;
            let rects = |n: &str| -> Result<Vec<[i32; 4]>, String> {
                v.get(n).and_then(Value::array).unwrap_or(&[]).iter().map(|g| Ok([i(g, "m_X")? as i32, i(g, "m_Y")? as i32, i(g, "m_Width")? as i32, i(g, "m_Height")? as i32])).collect()
            };
            let readable = match atlas {
                Some(a) => crate::assets::components::Texture2D::read(db.data(a))?.readable,
                None => false,
            };
            let settings = *db.find_scripts("TMP_Settings")?.first().ok_or("TMP_Settings")?;
            let features = db.mono(settings)?.and_then(|(_, _, s)| s.get("m_GetFontFeaturesAtRuntime").and_then(Value::bool)).ok_or("m_GetFontFeaturesAtRuntime")?;
            let mut indexes: Vec<u32> = Vec::new();
            for g in v.get("m_GlyphTable").and_then(Value::array).unwrap_or(&[]) {
                indexes.push(i(g, "m_Index")? as u32);
            }
            Some(dynamic::Dynamic::new(dynamic::Start {
                font: Arc::new(font),
                point_size: face.point_size,
                padding: i(&v, "m_AtlasPadding")? as i32,
                render_mode: i(&v, "m_AtlasRenderMode")? as i32,
                atlas_width: i(&v, "m_AtlasWidth")? as i32,
                atlas_height: i(&v, "m_AtlasHeight")? as i32,
                readable,
                multi_atlas: v.get("m_IsMultiAtlasTexturesEnabled").and_then(Value::bool).ok_or("m_IsMultiAtlasTexturesEnabled")?,
                features_at_runtime: features,
                free: rects("m_FreeGlyphRects")?,
                used: rects("m_UsedGlyphRects")?,
                glyph_indexes: indexes,
            }))
        } else {
            None
        };
        // TMP_FontAsset.AddSynthesizedCharactersAndFaceMetrics (a static font has no source font face loaded).
        // A dynamic asset loads its source face first: a unicode the face has a glyph for is not synthesized (U+0003 and U+0009 are added
        // from the face's own glyph at once).
        for u in [3u32, 9, 10, 11, 13, 1564, 8203, 8206, 8207, 8232, 8233, 8288] {
            if characters.contains_key(&u) {
                continue;
            }
            match &state {
                Some(d) if d.glyph_index(u) != 0 => {
                    if u == 3 || u == 9 {
                        unimplemented!("mod-only: {}'s source font has a glyph for U+{u:04X} (TryGetGlyphWithUnicodeValue)", mb.name);
                    }
                }
                _ => {
                    characters.insert(u, Character { unicode: u, glyph_index: 0, scale: 1.0, synth: true });
                }
            }
        }
        if face.cap_line == 0.0 {
            if let Some(c) = characters.get(&88) {
                face.cap_line = glyphs[&c.glyph_index].metrics.horizontal_bearing_y;
            }
        }
        if face.mean_line == 0.0 {
            if let Some(c) = characters.get(&120) {
                face.mean_line = glyphs[&c.glyph_index].metrics.horizontal_bearing_y;
            }
        }
        let mut pairs = HashMap::new();
        let records = v.get("m_FontFeatureTable").and_then(|t| t.get("m_GlyphPairAdjustmentRecords")).and_then(Value::array).ok_or("m_GlyphPairAdjustmentRecords")?;
        for p in records {
            let rec = |n: &str| -> Result<(u32, GlyphValueRecord), String> {
                let a = p.get(n).ok_or("adjustment record")?;
                let r = a.get("m_GlyphValueRecord").ok_or("m_GlyphValueRecord")?;
                Ok((i(a, "m_GlyphIndex")? as u32, GlyphValueRecord { x_placement: f(r, "m_XPlacement")?, y_placement: f(r, "m_YPlacement")?, x_advance: f(r, "m_XAdvance")?, y_advance: f(r, "m_YAdvance")? }))
            };
            let ((g1, first), (g2, second)) = (rec("m_FirstAdjustmentRecord")?, rec("m_SecondAdjustmentRecord")?);
            pairs.entry((g2 << 16) | g1).or_insert(PairAdjustment { first, second, flags: p.get("m_FeatureLookupFlags").and_then(Value::i64).unwrap_or(0) as i32 });
        }
        if v.get("m_KerningTable").and_then(|k| k.get("kerningPairs")).and_then(Value::array).is_some_and(|a| !a.is_empty()) {
            unimplemented!("mod-only: a TMP_FontAsset with a legacy kerning table ({}): UpgradeFontFeatures converts it", mb.name);
        }
        let material_ref = v.get("material").and_then(Value::pptr).and_then(|p| db.resolve(file, p));
        let material = SdfMaterial::read(db, material_ref)?;
        let mut asset = FontAsset {
            asset: Some(r),
            state,
            name: mb.name.clone(),
            face,
            glyphs,
            characters,
            pairs,
            fallbacks: Vec::new(),
            dynamic,
            atlas,
            atlas_width: i(&v, "m_AtlasWidth")? as i32,
            atlas_height: i(&v, "m_AtlasHeight")? as i32,
            atlas_padding: i(&v, "m_AtlasPadding")? as i32,
            normal_style: f(&v, "normalStyle")?,
            normal_spacing_offset: f(&v, "normalSpacingOffset")?,
            bold_style: f(&v, "boldStyle")?,
            bold_spacing: f(&v, "boldSpacing")?,
            italic_style: i(&v, "italicStyle")? as u8,
            tab_size: i(&v, "tabSize")? as u8,
            material,
            weight_typefaces: Vec::new(),
        };
        // Cycles in the fallback table: TMP's search skips an asset already visited; here a placeholder breaks it (its fontWeightTable
        // pairs are all unassigned, as every shipped font asset's are).
        let table = v.get("m_FontWeightTable").and_then(Value::array).ok_or("TMP field m_FontWeightTable")?;
        asset.weight_typefaces = vec![(None, None); table.len()];
        cache.insert(r, Arc::new(asset.clone()));
        let fallbacks: Vec<ObjRef> = v.get("m_FallbackFontAssetTable").and_then(Value::array).unwrap_or(&[]).iter().filter_map(Value::pptr).filter_map(|p| db.resolve(file, p)).collect();
        for fb in fallbacks {
            asset.fallbacks.push(FontAsset::read(db, fb, cache)?);
        }
        for (k, p) in table.iter().enumerate() {
            let mut face = |n: &str| p.get(n).and_then(Value::pptr).and_then(|q| db.resolve(file, q)).map(|r| FontAsset::read(db, r, cache)).transpose();
            asset.weight_typefaces[k] = (face("regularTypeface")?, face("italicTypeface")?);
        }
        let asset = Arc::new(asset);
        cache.insert(r, asset.clone());
        Ok(asset)
    }
}

/// TMP_SpriteCharacter with its TMP_SpriteGlyph resolved (UpdateLookupTables).
#[derive(Clone, Debug)]
pub struct SpriteCharacter {
    pub unicode: u32,
    pub glyph_index: u32,
    pub scale: f32,
    pub hash_code: i32,
    pub glyph: Glyph,
}

/// TMP_SpriteAsset: the sprite character table (in table order), the unicode and name-hash lookups UpdateLookupTables builds from it,
/// the sprite sheet and its material.
#[derive(Clone, Debug, Default)]
pub struct SpriteAsset {
    pub asset: Option<ObjRef>,
    pub name: String,
    /// m_FaceInfo (pointSize 0 in an asset that never set one: the sprites then scale by the text's font).
    pub face: FaceInfo,
    pub sheet: Option<ObjRef>,
    /// spriteSheet.width and .height.
    pub sheet_size: [i32; 2],
    pub material: SdfMaterial,
    /// m_SpriteCharacterTable in order; None where the character's glyph index has no sprite glyph (it is never resolved).
    pub characters: Vec<Option<SpriteCharacter>>,
    /// spriteCharacterLookupTable: unicode (not 0xFFFE) -> table index, the first entry wins.
    pub by_unicode: HashMap<u32, usize>,
    /// m_NameLookup: hashCode -> table index, the first entry wins.
    pub by_name: HashMap<i32, usize>,
    pub fallbacks: Vec<Arc<SpriteAsset>>,
}

/// A sprite lookup result: the asset that holds it and the index TMP reports (a table position, or a glyph index for the unicode lookups
/// of GetSpriteIndexFromUnicode).
pub type SpriteHit = Option<(Arc<SpriteAsset>, i32)>;

/// TMP_SpriteAsset.SearchFor...Internal(List): each fallback not searched yet, in order, with its own fallbacks.
fn search_list(list: &[Arc<SpriteAsset>], find: &dyn Fn(&SpriteAsset) -> Option<i32>, seen: &mut Vec<*const SpriteAsset>) -> SpriteHit {
    for a in list {
        let p = Arc::as_ptr(a);
        if !seen.contains(&p) {
            seen.push(p);
            if let Some(r) = search_asset(a, find, seen) {
                return Some(r);
            }
        }
    }
    None
}

/// The asset's own lookup, then its fallbacks'.
fn search_asset(a: &Arc<SpriteAsset>, find: &dyn Fn(&SpriteAsset) -> Option<i32>, seen: &mut Vec<*const SpriteAsset>) -> SpriteHit {
    if let Some(i) = find(a) {
        return Some((a.clone(), i));
    }
    search_list(&a.fallbacks, find, seen)
}

impl SpriteAsset {
    /// TMP_FontAssetUtilities.GetSpriteCharacterFromSpriteAsset (includeFallbacks): the character's table position.
    pub fn character(self: &Arc<Self>, unicode: u32) -> SpriteHit {
        search_list(std::slice::from_ref(self), &|a| a.by_unicode.get(&unicode).map(|&n| n as i32), &mut Vec::new())
    }

    /// TMP_SpriteAsset.SearchForSpriteByHashCode(asset, hash, includeFallbacks): the asset, its fallbacks, the default sprite asset, then the
    /// same for the settings' missing-character sprite (whose index is its glyph index).
    pub fn search_by_hash(self: &Arc<Self>, hash: i32, default: Option<&Arc<SpriteAsset>>, missing_unicode: u32) -> SpriteHit {
        let by_name = |a: &SpriteAsset| a.by_name.get(&hash).map(|&n| n as i32);
        if let Some(n) = by_name(self) {
            return Some((self.clone(), n));
        }
        let mut seen = vec![Arc::as_ptr(self)];
        if let Some(r) = search_list(&self.fallbacks, &by_name, &mut seen) {
            return Some(r);
        }
        if let Some(d) = default {
            if let Some(r) = search_asset(d, &by_name, &mut seen) {
                return Some(r);
            }
        }
        let by_unicode = |a: &SpriteAsset| a.by_unicode.get(&missing_unicode).and_then(|&n| a.characters[n].as_ref()).map(|c| c.glyph_index as i32);
        if let Some(i) = by_unicode(self) {
            return Some((self.clone(), i));
        }
        let mut seen = vec![Arc::as_ptr(self)];
        if let Some(r) = search_list(&self.fallbacks, &by_unicode, &mut seen) {
            return Some(r);
        }
        if let Some(d) = default {
            if let Some(r) = search_asset(d, &by_unicode, &mut seen) {
                return Some(r);
            }
        }
        None
    }

    pub fn read(db: &mut AssetDb, r: ObjRef, cache: &mut HashMap<ObjRef, Arc<SpriteAsset>>) -> Result<Arc<SpriteAsset>, String> {
        if let Some(a) = cache.get(&r) {
            return Ok(a.clone());
        }
        let (_, mb, v) = db.mono(r)?.ok_or("sprite asset data")?;
        let file = r.file;
        let fi = v.get("m_FaceInfo").ok_or("m_FaceInfo")?;
        let face = FaceInfo {
            point_size: i(fi, "m_PointSize")? as i32,
            scale: f(fi, "m_Scale")?,
            line_height: f(fi, "m_LineHeight")?,
            ascent_line: f(fi, "m_AscentLine")?,
            cap_line: f(fi, "m_CapLine")?,
            mean_line: f(fi, "m_MeanLine")?,
            baseline: f(fi, "m_Baseline")?,
            descent_line: f(fi, "m_DescentLine")?,
            ..Default::default()
        };
        let mut glyphs: HashMap<u32, Glyph> = HashMap::new();
        for g in v.get("m_SpriteGlyphTable").and_then(Value::array).unwrap_or(&[]) {
            let index = i(g, "m_Index")? as u32;
            let m = g.get("m_Metrics").ok_or("m_Metrics")?;
            let rect = g.get("m_GlyphRect").ok_or("m_GlyphRect")?;
            glyphs.entry(index).or_insert(Glyph {
                index,
                metrics: GlyphMetrics { width: f(m, "m_Width")?, height: f(m, "m_Height")?, horizontal_bearing_x: f(m, "m_HorizontalBearingX")?, horizontal_bearing_y: f(m, "m_HorizontalBearingY")?, horizontal_advance: f(m, "m_HorizontalAdvance")? },
                rect: [i(rect, "m_X")? as i32, i(rect, "m_Y")? as i32, i(rect, "m_Width")? as i32, i(rect, "m_Height")? as i32],
                scale: f(g, "m_Scale")?,
                atlas_index: i(g, "m_AtlasIndex")? as i32,
            });
        }
        let (mut characters, mut by_unicode, mut by_name) = (Vec::new(), HashMap::new(), HashMap::new());
        for (n, c) in v.get("m_SpriteCharacterTable").and_then(Value::array).unwrap_or(&[]).iter().enumerate() {
            let glyph_index = i(c, "m_GlyphIndex")? as u32;
            let Some(&glyph) = glyphs.get(&glyph_index) else {
                characters.push(None);
                continue;
            };
            let (unicode, hash_code) = (i(c, "m_Unicode")? as u32, i(c, "m_HashCode")? as i32);
            by_name.entry(hash_code).or_insert(n);
            if unicode != 65534 {
                by_unicode.entry(unicode).or_insert(n);
            }
            characters.push(Some(SpriteCharacter { unicode, glyph_index, scale: f(c, "m_Scale")?, hash_code, glyph }));
        }
        let sheet = v.get("spriteSheet").and_then(Value::pptr).and_then(|p| db.resolve(file, p));
        let sheet_size = match sheet {
            Some(t) => {
                let t = crate::assets::components::Texture2D::read(db.data(t))?;
                [t.width as i32, t.height as i32]
            }
            None => [0, 0],
        };
        let material_ref = v.get("material").and_then(Value::pptr).and_then(|p| db.resolve(file, p));
        let material = SdfMaterial::read(db, material_ref)?;
        let mut asset = SpriteAsset { asset: Some(r), name: mb.name.clone(), face, sheet, sheet_size, material, characters, by_unicode, by_name, fallbacks: Vec::new() };
        cache.insert(r, Arc::new(asset.clone()));
        let fallbacks: Vec<ObjRef> = v.get("fallbackSpriteAssets").and_then(Value::array).unwrap_or(&[]).iter().filter_map(Value::pptr).filter_map(|p| db.resolve(file, p)).collect();
        for fb in fallbacks {
            asset.fallbacks.push(SpriteAsset::read(db, fb, cache)?);
        }
        let asset = Arc::new(asset);
        cache.insert(r, asset.clone());
        Ok(asset)
    }
}

/// The TMP_Settings asset's values the text layout reads.
#[derive(Clone, Debug, Default)]
pub struct TmpSettings {
    /// missingGlyphCharacter (0 means U+25A1).
    pub missing_glyph: u32,
    pub match_material_preset: bool,
    pub fallback_fonts: Vec<Arc<FontAsset>>,
    pub default_font: Option<Arc<FontAsset>>,
    /// TMP_Settings.defaultSpriteAsset (the text element lookup falls back to its sprite characters).
    pub default_sprite: Option<Arc<SpriteAsset>>,
    /// TMP_Settings.defaultSpriteAssetPath.
    pub sprite_path: String,
    /// The sprite assets in the Resources: lowercased path and asset.
    pub sprite_assets: Vec<(String, Arc<SpriteAsset>)>,
    /// TMP_Settings.missingCharacterSpriteUnicode.
    pub missing_sprite_unicode: u32,
    pub modern_hangul: bool,
}

impl TmpSettings {
    /// From the install's TMP_Settings asset (m_defaultFontAsset, m_missingGlyphCharacter, the fallbacks, the
    /// default sprite asset's characters).
    pub fn read(db: &mut AssetDb, cache: &mut HashMap<ObjRef, Arc<FontAsset>>) -> Result<Self, String> {
        let r = *db.find_scripts("TMP_Settings")?.first().ok_or("TMP_Settings")?;
        let (_, _, v) = db.mono(r)?.ok_or("TMP_Settings data")?;
        let font = |db: &mut AssetDb, cache: &mut HashMap<ObjRef, Arc<FontAsset>>, p: PPtr| match db.resolve(r.file, p) {
            Some(o) => FontAsset::read(db, o, cache).map(Some),
            None => Ok(None),
        };
        let mut default_font = None;
        if let Some(o) = v.get("m_defaultFontAsset").and_then(Value::pptr).and_then(|p| db.resolve(r.file, p)) {
            default_font = Some(FontAsset::read(db, o, cache)?);
        }
        let mut fallback_fonts = Vec::new();
        if let Some(Value::Array(a)) = v.get("m_fallbackFontAssets") {
            for x in a {
                if let Some(f) = x.pptr().map(|p| font(db, cache, p)).transpose()?.flatten() {
                    fallback_fonts.push(f);
                }
            }
        }
        let mut sprites = HashMap::new();
        let mut default_sprite = None;
        if let Some(o) = v.get("m_defaultSpriteAsset").and_then(Value::pptr).and_then(|p| db.resolve(r.file, p)) {
            default_sprite = Some(SpriteAsset::read(db, o, &mut sprites)?);
        }
        // Resources.Load<TMP_SpriteAsset>(defaultSpriteAssetPath + name) of `<sprite="name">`: every sprite asset in the Resources.
        let mut sprite_assets = Vec::new();
        for (path, o) in db.resource_entries(114)? {
            if db.script(o)?.is_some_and(|s| s.class_name == "TMP_SpriteAsset") {
                sprite_assets.push((path, SpriteAsset::read(db, o, &mut sprites)?));
            }
        }
        Ok(Self {
            missing_glyph: i(&v, "m_missingGlyphCharacter")? as u32,
            match_material_preset: v.get("m_matchMaterialPreset").and_then(Value::bool).unwrap_or(true),
            fallback_fonts,
            default_font,
            default_sprite,
            sprite_path: match v.get("m_defaultSpriteAssetPath") {
                Some(Value::Str(s)) => s.clone(),
                _ => String::new(),
            },
            sprite_assets,
            missing_sprite_unicode: i(&v, "m_MissingCharacterSpriteUnicode").unwrap_or(0) as u32,
            modern_hangul: v.get("m_UseModernHangulLineBreakingRules").and_then(Value::bool).unwrap_or(false),
        })
    }
}

/// TMPro.TextMeshPro's serialized properties GenerateTextMesh reads.
#[derive(Clone, Debug, Default)]
pub struct TextProps {
    pub text: String,
    pub font: Arc<FontAsset>,
    /// m_sharedMaterial when it is not the font's own material.
    pub material: SdfMaterial,
    pub font_color: [f32; 4],
    pub enable_vertex_gradient: bool,
    /// m_fontColorGradient: bottomLeft, topLeft, topRight, bottomRight.
    pub font_color_gradient: [[f32; 4]; 4],
    /// m_fontColorGradientPreset is set (a TMP_ColorGradient asset).
    pub has_gradient_preset: bool,
    pub override_html_colors: bool,
    pub font_size: f32,
    pub font_size_base: f32,
    pub font_weight: i32,
    pub enable_auto_sizing: bool,
    pub font_size_min: f32,
    pub font_size_max: f32,
    /// FontStyles flags.
    pub font_style: i32,
    /// HorizontalAlignmentOptions and VerticalAlignmentOptions.
    pub horizontal_alignment: i32,
    pub vertical_alignment: i32,
    pub character_spacing: f32,
    pub word_spacing: f32,
    pub line_spacing: f32,
    pub line_spacing_max: f32,
    pub paragraph_spacing: f32,
    pub char_width_max_adj: f32,
    pub enable_word_wrapping: bool,
    pub word_wrapping_ratios: f32,
    /// TextOverflowModes
    pub overflow_mode: i32,
    pub enable_kerning: bool,
    pub enable_extra_padding: bool,
    pub is_rich_text: bool,
    pub parse_ctrl_characters: bool,
    pub is_orthographic: bool,
    pub is_right_to_left: bool,
    /// m_tintAllSprites.
    pub tint_all_sprites: bool,
    /// m_spriteAsset (the component's own sprite asset) is assigned.
    pub has_sprite_asset: bool,
    pub use_max_visible_descender: bool,
    pub page_to_display: i32,
    /// m_margin: left, top, right, bottom.
    pub margin: [f32; 4],
    /// The RectTransform's rect size and pivot.
    pub rect_size: [f32; 2],
    pub pivot: [f32; 2],
    pub geometry_sorting_order: i32,
    /// transform.lossyScale.y.
    pub lossy_scale_y: f32,
    /// TextInputSources.TextInputBox (the serialized text, never set through the `text` property): `\n` style
    /// escapes are parsed when parse_ctrl_characters is on.
    pub text_input_box: bool,
    pub settings: TmpSettings,
}

impl TextProps {
    /// From the TextMeshPro component's fields, with its font resolved through `fonts`.
    pub fn read(v: &Value, font: Arc<FontAsset>, material: SdfMaterial, rect_size: [f32; 2], pivot: [f32; 2]) -> Result<Self, String> {
        let b = |n: &str| v.get(n).and_then(Value::bool).ok_or_else(|| format!("TMP field {n}"));
        let color = v.get("m_fontColor").ok_or("m_fontColor")?;
        let margin = v.get("m_margin").ok_or("m_margin")?;
        let text = match v.get("m_text") {
            Some(Value::Str(s)) => s.clone(),
            _ => return Err("m_text".into()),
        };
        Ok(Self {
            text,
            font,
            material,
            font_color: [f(color, "r")?, f(color, "g")?, f(color, "b")?, f(color, "a")?],
            enable_vertex_gradient: b("m_enableVertexGradient")?,
            font_color_gradient: {
                let g = v.get("m_fontColorGradient").ok_or("m_fontColorGradient")?;
                let c = |n: &str| -> Result<[f32; 4], String> {
                    let c = g.get(n).ok_or_else(|| format!("m_fontColorGradient.{n}"))?;
                    Ok([f(c, "r")?, f(c, "g")?, f(c, "b")?, f(c, "a")?])
                };
                [c("bottomLeft")?, c("topLeft")?, c("topRight")?, c("bottomRight")?]
            },
            has_gradient_preset: v.get("m_fontColorGradientPreset").and_then(Value::pptr).is_some_and(|p| !p.is_null()),
            override_html_colors: b("m_overrideHtmlColors")?,
            font_size: f(v, "m_fontSize")?,
            font_size_base: f(v, "m_fontSizeBase")?,
            font_weight: i(v, "m_fontWeight")? as i32,
            enable_auto_sizing: b("m_enableAutoSizing")?,
            font_size_min: f(v, "m_fontSizeMin")?,
            font_size_max: f(v, "m_fontSizeMax")?,
            font_style: i(v, "m_fontStyle")? as i32,
            horizontal_alignment: i(v, "m_HorizontalAlignment")? as i32,
            vertical_alignment: i(v, "m_VerticalAlignment")? as i32,
            character_spacing: f(v, "m_characterSpacing")?,
            word_spacing: f(v, "m_wordSpacing")?,
            line_spacing: f(v, "m_lineSpacing")?,
            line_spacing_max: f(v, "m_lineSpacingMax")?,
            paragraph_spacing: f(v, "m_paragraphSpacing")?,
            char_width_max_adj: f(v, "m_charWidthMaxAdj")?,
            enable_word_wrapping: b("m_enableWordWrapping")?,
            word_wrapping_ratios: f(v, "m_wordWrappingRatios")?,
            overflow_mode: i(v, "m_overflowMode")? as i32,
            enable_kerning: b("m_enableKerning")?,
            enable_extra_padding: b("m_enableExtraPadding")?,
            is_rich_text: b("m_isRichText")?,
            parse_ctrl_characters: b("m_parseCtrlCharacters")?,
            is_orthographic: b("m_isOrthographic")?,
            is_right_to_left: b("m_isRightToLeft")?,
            tint_all_sprites: v.get("m_tintAllSprites").and_then(Value::bool).unwrap_or(false),
            has_sprite_asset: v.get("m_spriteAsset").and_then(Value::pptr).is_some_and(|p| !p.is_null()),
            use_max_visible_descender: b("m_useMaxVisibleDescender")?,
            page_to_display: i(v, "m_pageToDisplay")? as i32,
            margin: [f(margin, "x")?, f(margin, "y")?, f(margin, "z")?, f(margin, "w")?],
            rect_size,
            pivot,
            geometry_sorting_order: i(v, "m_geometrySortingOrder")? as i32,
            lossy_scale_y: 1.0,
            text_input_box: true,
            settings: TmpSettings::default(),
        })
    }
}

/// One character quad of the text mesh, in the text's local space: corners bottom-left, top-left, top-right,
/// bottom-right (TMP_CharacterInfo.vertex_BL/TL/TR/BR), atlas UVs (uv0.xy), the SDF scale in uv2 (uv0.w in
/// TMP 3.x: the `xScale` FillCharacterVertexBuffers writes) and the vertex colour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quad {
    pub pos: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    pub uv2: [[f32; 2]; 4],
    pub color: [[u8; 4]; 4],
    /// The font asset's atlas this quad samples (the fallback's for a fallback character).
    pub atlas: Option<ObjRef>,
}

/// What OnPreRenderObject leaves: the mesh, and the values scripts read back (preferred size, the auto-sized font
/// size, truncation).
#[derive(Clone, Debug, Default)]
pub struct TextMesh {
    /// Every quad, material 0's first (its count is `main_len`), then each sub mesh's in material order.
    pub quads: Vec<Quad>,
    /// TMP_Text.m_textInfo.meshInfo[0]: the quads drawn with the text's own material.
    pub main_len: usize,
    /// meshInfo[1..]: a TMP_SubMesh per extra material reference (a fallback font's characters), its material and atlas.
    pub submeshes: Vec<SubMesh>,
    pub font_size: f32,
    pub preferred: [f32; 2],
    pub is_text_truncated: bool,
    /// The TMP_SpriteAnimator.DoSpriteAnimation calls of the last GenerateTextMesh (`<sprite anim=...>`), after its
    /// StopAllAnimations; None when GenerateTextMesh stopped before it (an empty text).
    pub sprite_anims: Option<Vec<SpriteAnim>>,
}

/// One DoSpriteAnimation(currentCharacter, spriteAsset, start, end, framerate).
#[derive(Clone, Debug)]
pub struct SpriteAnim {
    pub character: i32,
    pub asset: Arc<SpriteAsset>,
    pub start: i32,
    pub end: i32,
    pub framerate: i32,
    /// textInfo.characterInfo[currentCharacter] (None past the array: the coroutine throws).
    pub info: Option<AnimChar>,
}

/// What TMP_SpriteAnimator reads of a TMP_CharacterInfo.
#[derive(Clone, Copy, Debug)]
pub struct AnimChar {
    /// materialReferenceIndex, vertexIndex / 4
    pub mat: usize,
    pub quad: usize,
    pub origin: f32,
    pub baseline: f32,
    pub scale: f32,
    pub character: u16,
}

/// A TMP_SubMesh: the child object TextMeshPro makes for a material reference past the first.
#[derive(Clone, Debug, Default)]
pub struct SubMesh {
    pub material: SdfMaterial,
    pub atlas: Option<ObjRef>,
    /// The font's dynamic atlas when it has one (the texture drawn is its live pixels).
    pub dynamic: Option<Arc<dynamic::Dynamic>>,
    pub quads: Vec<Quad>,
}

/// The installed TMP_FontAssets by m_Name (tests: path ids move between game versions).
#[cfg(test)]
pub(crate) fn installed_font(db: &mut AssetDb, name: &str) -> ObjRef {
    let fonts = db.find_scripts("TMP_FontAsset").unwrap();
    let names: Vec<String> = fonts.iter().map(|&r| db.mono(r).unwrap().map(|(_, mb, _)| mb.name).unwrap_or_default()).collect();
    let named: Vec<ObjRef> = fonts.iter().zip(&names).filter(|(_, n)| *n == name).map(|(&r, _)| r).collect();
    match named[..] {
        [r] => r,
        _ => panic!("{} TMP_FontAssets named {name:?}; installed: {names:?} at {fonts:?}", named.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The world texts' four font assets read from the installed game: tables, atlas, material.
    #[test]
    fn installed_font_assets() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let mut cache = HashMap::new();
        let r = installed_font(&mut db, "VCR_OSD_MONO");
        let vcr = FontAsset::read(&mut db, r, &mut cache).unwrap();
        // 1.28.41: VCR OSD Mono [RUS by Daymarius], point size 123 on a 1024 atlas (1.28.32: 81 on 512).
        assert_eq!(vcr.face.point_size, 123);
        assert!(vcr.characters.contains_key(&('A' as u32)) && vcr.atlas.is_some());
        assert_eq!((vcr.atlas_width, vcr.atlas_padding, vcr.bold_spacing), (1024, 5, 7.0));
        // VCR OSD Mono's material is TextMeshPro/Bitmap: no _GradientScale, so GenerateTextMesh adds no style padding.
        assert!(vcr.material.material.is_some() && !vcr.material.has("_GradientScale") && vcr.material.colors.contains_key("_FaceColor"));
        assert_eq!(vcr.fallbacks.len(), 1);
        for name in ["alarm clock SDF", "arial SDF bold", "arial SDF"] {
            let r = installed_font(&mut db, name);
            FontAsset::read(&mut db, r, &mut cache).unwrap();
        }
    }

    /// The default font (LiberationSans SDF) reads with its dynamic fallback: its own 250 characters are served, the
    /// fallback only marks where a lookup would grow the atlas.
    #[test]
    fn default_font_has_a_dynamic_tail() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let s = TmpSettings::read(&mut db, &mut HashMap::new()).unwrap();
        let d = s.default_font.expect("the default font is read");
        assert!(!d.dynamic && d.characters.contains_key(&('A' as u32)) && d.characters.contains_key(&0x25A1));
        assert!(!d.characters.contains_key(&0x44F), "Cyrillic is not in the static table");
        assert!(d.fallbacks.len() == 1 && d.fallbacks[0].dynamic, "{} {:?}", d.fallbacks.len(), d.fallbacks.iter().map(|f| (f.dynamic, f.characters.len())).collect::<Vec<_>>());
    }
}

