//! TextMeshPro.OnPreRenderObject / GenerateTextMesh, ParseInputText and the rich-text tags (TMP_Text.ValidateHtmlTag),
//! rewritten from the decompiled TMP 3.x (TextMeshPro.cs, TMP_Text.cs). Names follow the C# fields (`m_xAdvance` ->
//! `x_advance`), floats are f32 in the C# operation order, `Mathf.Round` is banker's rounding.
//!
//! Stopped with `unimplemented!`: vertex gradients with a preset, Linked / Page / ScrollRect overflow, right-to-left text,
//! highlights, CJK line breaking, atlas index > 0 glyphs, non-default texture mappings,
//! `<style>` and every rich-text tag but the listed ones.
//! Constants TMP leaves at their defaults here: maxVisibleCharacters / Words / Lines = 99999, firstVisibleCharacter = 0,
//! lineHeight = -32767 (no tag changes it), no FX matrix, the "Normal" text style (inserts nothing).
//! Not emitted: the zero-filled gap quads TMP leaves in a mesh between its character quads and its underline quads.
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::{Character, FontAsset, Glyph, GlyphValueRecord, Quad, SdfMaterial, SpriteAsset, SpriteCharacter, SubMesh, TextMesh, TextProps};
use crate::assets::db::ObjRef;

const LARGE_POS: f32 = 32767.0;
const LARGE_NEG: f32 = -32767.0;
/// TMP_Text.m_lineHeight's default: no <line-height> in effect.
const LINE_HEIGHT_UNSET: f32 = -32767.0;
const LARGE_POS_V2: f32 = 2.1474836E+09;
const LARGE_NEG_V2: f32 = -2.1474836E+09;
const AUTO_SIZE_MAX_ITERATIONS: i32 = 100;
const MAX_VISIBLE: i32 = 99999;

// FontStyles
const BOLD: i32 = 1;
const ITALIC: i32 = 2;
const UNDERLINE: i32 = 4;
const LOWER_CASE: i32 = 8;
const UPPER_CASE: i32 = 0x10;
const SMALL_CAPS: i32 = 0x20;
const STRIKETHROUGH: i32 = 0x40;
const SUPERSCRIPT: i32 = 0x80;
const SUBSCRIPT: i32 = 0x100;
const HIGHLIGHT: i32 = 0x200;
// HorizontalAlignmentOptions
const H_LEFT: i32 = 1;
const H_CENTER: i32 = 2;
const H_RIGHT: i32 = 4;
const H_JUSTIFIED: i32 = 8;
const H_FLUSH: i32 = 0x10;
const H_GEOMETRY: i32 = 0x20;
// VerticalAlignmentOptions
const V_TOP: i32 = 0x100;
const V_MIDDLE: i32 = 0x200;
const V_BOTTOM: i32 = 0x400;
const V_BASELINE: i32 = 0x800;
const V_GEOMETRY: i32 = 0x1000;
const V_CAPLINE: i32 = 0x2000;
// TextOverflowModes
const OVERFLOW: i32 = 0;
const ELLIPSIS: i32 = 1;
const MASKING: i32 = 2;
const TRUNCATE: i32 = 3;
const SCROLL_RECT: i32 = 4;
// FontWeight
const WEIGHT_REGULAR: i32 = 400;
const WEIGHT_BOLD: i32 = 700;

type V3 = [f32; 3];

/// Mathf.Max / Mathf.Min (a plain comparison, so NaN behaves as in C#).
fn fmax(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
fn fmin(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}
/// Mathf.Round: banker's rounding.
fn mathf_round(v: f32) -> f32 {
    v.round_ties_even()
}
fn next_pow2(v: usize) -> usize {
    if v == 0 { 1 } else { v.next_power_of_two() }
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct C32 {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl C32 {
    const WHITE: C32 = C32::new(255, 255, 255, 255);
    const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    /// Color -> Color32: each channel `(byte)Mathf.Round(Mathf.Clamp01(c) * 255f)`.
    fn from_color(c: [f32; 4]) -> Self {
        let q = |v: f32| mathf_round(v.clamp(0.0, 1.0) * 255.0) as u8;
        Self::new(q(c[0]), q(c[1]), q(c[2]), q(c[3]))
    }
    fn arr(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// TMP_TextUtilities.ToUpperASCIIFast: the lookup string "-"x32 + " !-#$%&-()*+,-./0123456789:;<=>?@A-Z[-]^_`A-Z{|}~-".
fn to_upper_ascii_fast(c: u32) -> u32 {
    const T: &[u8] = b"-------------------------------- !-#$%&-()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[-]^_`ABCDEFGHIJKLMNOPQRSTUVWXYZ{|}~-";
    if c as usize > T.len() - 1 {
        return c;
    }
    T[c as usize] as u32
}

fn hex_to_int(c: u16) -> i32 {
    match c {
        0x30..=0x39 => (c - 0x30) as i32,
        0x41..=0x46 => (c - 0x41) as i32 + 10,
        0x61..=0x66 => (c - 0x61) as i32 + 10,
        _ => 15,
    }
}

/// char.IsWhiteSpace on a UTF-16 unit.
fn is_white_space(u: i32) -> bool {
    matches!(u, 9..=13 | 0x20 | 0x85 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000)
}
/// char.IsSeparator: Zs, Zl, Zp.
fn is_separator(u: i32) -> bool {
    matches!(u, 0x20 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000)
}
/// char.IsControl: Cc.
fn is_control(u: i32) -> bool {
    matches!(u, 0..=0x1f | 0x7f..=0x9f)
}
fn char_of(u: i32) -> char {
    char::from_u32(u as u32 & 0xffff).unwrap_or('\u{fffd}')
}
fn char_is_lower(u: i32) -> bool {
    char_of(u).is_lowercase()
}
fn char_is_upper(u: i32) -> bool {
    char_of(u).is_uppercase()
}
fn char_to_upper(u: i32) -> i32 {
    let mut it = char_of(u).to_uppercase();
    match (it.next(), it.next()) {
        (Some(x), None) => x as i32,
        _ => u,
    }
}
fn char_to_lower(u: i32) -> i32 {
    let mut it = char_of(u).to_lowercase();
    match (it.next(), it.next()) {
        (Some(x), None) => x as i32,
        _ => u,
    }
}

/// TMP_TextProcessingStack<T>; a struct copy shares the array (as in C#), `index` is copied.
#[derive(Clone)]
struct Stack<T: Copy + Default> {
    items: Rc<RefCell<Vec<T>>>,
    index: usize,
}

impl<T: Copy + Default> Stack<T> {
    fn new(n: usize) -> Self {
        Self { items: Rc::new(RefCell::new(vec![T::default(); n])), index: 0 }
    }
    fn set_default(&mut self, item: T) {
        self.items.borrow_mut()[0] = item;
        self.index = 1;
    }
    fn add(&mut self, item: T) {
        let mut v = self.items.borrow_mut();
        if self.index < v.len() {
            v[self.index] = item;
            self.index += 1;
        }
    }
    /// `index--; if (index <= 0) { index = 1; return itemStack[0]; } return itemStack[index - 1];`
    fn remove(&mut self) -> T {
        self.index = self.index.wrapping_sub(1);
        if self.index == 0 || self.index > 1 << 30 {
            self.index = 1;
            return self.items.borrow()[0];
        }
        self.items.borrow()[self.index - 1]
    }
    fn peek(&self) -> T {
        if self.index == 0 {
            return T::default();
        }
        self.items.borrow()[self.index - 1]
    }
}

/// TMP_FontStyleStack (a plain value struct).
#[derive(Clone, Copy, Default)]
struct FontStyleStack {
    bold: u8,
    italic: u8,
    underline: u8,
}

impl FontStyleStack {
    fn slot(&mut self, style: i32) -> &mut u8 {
        match style {
            BOLD => &mut self.bold,
            ITALIC => &mut self.italic,
            UNDERLINE => &mut self.underline,
            _ => unimplemented!("mod-only: TMP_FontStyleStack for style {style} (strikethrough, highlight: no shipped text has <s> or <mark>)"),
        }
    }
    fn add(&mut self, style: i32) -> u8 {
        let s = self.slot(style);
        *s = s.wrapping_add(1);
        *s
    }
    fn remove(&mut self, style: i32) -> u8 {
        let s = self.slot(style);
        *s = if *s > 1 { *s - 1 } else { 0 };
        *s
    }
}

/// RichTextTagAttribute
#[derive(Clone, Copy, Default)]
struct Attr {
    name_hash: i32,
    value_hash: i32,
    /// TagValueType: 0 none, 1 numerical, 2 string, 4 color.
    value_type: u8,
    value_start: usize,
    value_len: usize,
    /// TagUnitType: 0 pixels, 1 font units, 2 percentage.
    unit: u8,
}

/// A TMP_Character together with the font asset that owns it and its glyph.
#[derive(Clone)]
struct Elem {
    font: Arc<FontAsset>,
    ch: Character,
    glyph: Glyph,
    /// The sprite asset a TMP_SpriteCharacter was found in (None for a font character).
    sprite: Option<Arc<SpriteAsset>>,
    /// isAlternativeTypeface: found in the font's fontWeightTable typeface for the style and weight.
    alt: bool,
}

/// The TMP_SpriteCharacter at `index` of the asset's character table: List<T>'s indexer throws for an index outside it
/// (a `<sprite=-2>` or `<sprite anim=...>` start TMP does not range-check), and a character whose glyph was never
/// resolved throws a NullReferenceException where its glyph is read.
pub(crate) fn sprite_character(sa: &SpriteAsset, index: i32) -> SpriteCharacter {
    match usize::try_from(index).ok().and_then(|i| sa.characters.get(i)) {
        None => crate::game::menu::throw("ArgumentOutOfRangeException: Index was out of range. Must be non-negative and less than the size of the collection.
Parameter name: index"),
        Some(None) => crate::game::menu::npe(),
        Some(Some(c)) => c.clone(),
    }
}

/// A sprite character as the text element TMP stores for it (its glyph index, scale and glyph).
fn sprite_elem(font: &Arc<FontAsset>, sa: &Arc<SpriteAsset>, c: &SpriteCharacter) -> Elem {
    Elem { font: font.clone(), ch: Character { unicode: c.unicode, glyph_index: c.glyph_index, scale: c.scale, synth: false }, glyph: c.glyph, sprite: Some(sa.clone()), alt: false }
}

fn same_font(a: &FontAsset, b: &FontAsset) -> bool {
    std::ptr::eq(a, b) || (a.asset.is_some() && a.asset == b.asset)
}

/// characterLookupTable.TryGetValue, then for a dynamic asset TryAddCharacterInternal (the character from its source font).
fn char_in_font(unicode: u32, src: &Arc<FontAsset>) -> Option<Elem> {
    if let Some(c) = src.characters.get(&unicode) {
        return Some(Elem { font: src.clone(), ch: *c, glyph: src.glyph(c), sprite: None, alt: false });
    }
    let d = src.state.as_ref()?;
    let (c, glyph) = d.character(unicode).or_else(|| d.try_add_character(unicode, &|i| src.glyphs.get(&i).copied()))?;
    Some(Elem { font: src.clone(), ch: c, glyph, sprite: None, alt: false })
}

/// TMP_FontAssetUtilities.GetCharacterFromFontAsset_Internal.
fn char_from_font_internal(unicode: u32, src: &Arc<FontAsset>, include_fallbacks: bool, style: i32, weight: i32, searched: &mut Vec<Arc<FontAsset>>) -> Option<Elem> {
    if (style & ITALIC == ITALIC) || weight != WEIGHT_REGULAR {
        let n = match weight {
            100 => 1,
            200 => 2,
            300 => 3,
            400 => 4,
            500 => 5,
            600 => 6,
            700 => 7,
            800 => 8,
            900 => 9,
            _ => 4,
        };
        let italic = style & ITALIC == ITALIC;
        // fontWeightTable[num]: an IndexOutOfRangeException in TMP when the table is shorter.
        let pair = src.weight_typefaces.get(n).unwrap_or_else(|| crate::game::menu::throw(format!("IndexOutOfRangeException: fontWeightTable[{n}] of {}", src.name)));
        if let Some(face) = if italic { &pair.1 } else { &pair.0 } {
            if let Some(e) = char_in_font(unicode, face) {
                return Some(Elem { alt: true, ..e });
            }
        }
    }
    if let Some(e) = char_in_font(unicode, src) {
        return Some(e);
    }
    if include_fallbacks {
        for fb in &src.fallbacks {
            if !searched.iter().any(|s| same_font(s, fb)) {
                searched.push(fb.clone());
                if let Some(r) = char_from_font_internal(unicode, fb, true, style, weight, searched) {
                    return Some(r);
                }
            }
        }
    }
    None
}

/// TMP_FontAssetUtilities.GetCharacterFromFontAsset.
fn char_from_font(unicode: u32, src: &Arc<FontAsset>, include_fallbacks: bool, style: i32, weight: i32) -> Option<Elem> {
    char_from_font_internal(unicode, src, include_fallbacks, style, weight, &mut Vec::new())
}

/// TMP_FontAssetUtilities.GetCharacterFromFontAssets.
fn char_from_fonts(unicode: u32, fonts: &[Arc<FontAsset>], include_fallbacks: bool, style: i32, weight: i32) -> Option<Elem> {
    let mut searched = Vec::new();
    for f in fonts {
        if let Some(e) = char_from_font_internal(unicode, f, include_fallbacks, style, weight, &mut searched) {
            return Some(e);
        }
    }
    None
}

/// Identity of a Material instance: a font's material, or the fallback clone TMP_MaterialManager makes of a source
/// material for a target atlas.
#[derive(Clone, Copy, PartialEq, Debug)]
enum MKey {
    Obj(Option<ObjRef>),
    Fallback(usize, Option<ObjRef>),
}

/// A material with ShaderUtilities.UpdateShaderRatios applied and the padding GetPadding gives it.
struct ArenaMat {
    key: MKey,
    mat: SdfMaterial,
    padding: f32,
}

/// TMP_Text.m_materialReferences entry.
#[derive(Clone)]
struct MatRef {
    arena: usize,
    font: Arc<FontAsset>,
    ref_count: i32,
    /// MaterialReference.spriteAsset: set for a sprite asset's material (its font is the primary one).
    sprite: Option<Arc<SpriteAsset>>,
}

/// ShaderUtilities.UpdateShaderRatios.
fn update_shader_ratios(m: &mut SdfMaterial) {
    let ratios_off = m.keywords.iter().any(|k| k == "RATIOS_OFF");
    let flag = !ratios_off;
    let clamp = 1.0f32;
    if m.has("_GradientScale") && m.has("_FaceDilate") {
        let num4 = m.float("_GradientScale");
        let num5 = m.float("_FaceDilate");
        let num6 = m.float("_OutlineWidth");
        let num7 = m.float("_OutlineSoftness");
        let num8 = fmax(m.float("_WeightNormal"), m.float("_WeightBold")) / 4.0;
        let mut num9 = fmax(1.0, num8 + num5 + num6 + num7);
        let num = if flag { (num4 - clamp) / (num4 * num9) } else { 1.0 };
        m.floats.insert("_ScaleRatioA".into(), num);
        if m.has("_GlowOffset") {
            let num10 = m.float("_GlowOffset");
            let num11 = m.float("_GlowOuter");
            let num12 = (num8 + num5) * (num4 - clamp);
            num9 = fmax(1.0, num10 + num11);
            let num2 = if flag { fmax(0.0, num4 - clamp - num12) / (num4 * num9) } else { 1.0 };
            m.floats.insert("_ScaleRatioB".into(), num2);
        }
        if m.has("_UnderlayOffsetX") {
            let f = m.float("_UnderlayOffsetX");
            let f2 = m.float("_UnderlayOffsetY");
            let num13 = m.float("_UnderlayDilate");
            let num14 = m.float("_UnderlaySoftness");
            let num15 = (num8 + num5) * (num4 - clamp);
            num9 = fmax(1.0, fmax(f.abs(), f2.abs()) + num13 + num14);
            let num3 = if flag { fmax(0.0, num4 - clamp - num15) / (num4 * num9) } else { 1.0 };
            m.floats.insert("_ScaleRatioC".into(), num3);
        }
    }
}

/// ShaderUtilities.GetPadding(material, enableExtraPadding, isBold) (isBold is unused by it).
fn shader_padding(m: &mut SdfMaterial, extra_padding: bool) -> f32 {
    let num = if extra_padding { 4 } else { 0 };
    if !m.has("_GradientScale") {
        let mut num = num;
        if m.has("_Padding") {
            num += m.float("_Padding") as i32;
        }
        return num as f32 + 1.0;
    }
    update_shader_ratios(m);
    let kw = |m: &SdfMaterial, k: &str| m.keywords.iter().any(|x| x == k);
    let mut zero = [0.0f32; 4];
    let (mut num2, mut num3, mut num4, mut num5, mut num6, mut num7, mut num8, mut num9) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    if m.has("_ScaleRatioA") {
        num5 = m.float("_ScaleRatioA");
    }
    if m.has("_FaceDilate") {
        num2 = m.float("_FaceDilate") * num5;
    }
    if m.has("_OutlineSoftness") {
        num3 = m.float("_OutlineSoftness") * num5;
    }
    if m.has("_OutlineWidth") {
        num4 = m.float("_OutlineWidth") * num5;
    }
    let mut num10 = num4 + num3 + num2;
    if m.has("_GlowOffset") && kw(m, "GLOW_ON") {
        if m.has("_ScaleRatioB") {
            num6 = m.float("_ScaleRatioB");
        }
        num8 = m.float("_GlowOffset") * num6;
        num9 = m.float("_GlowOuter") * num6;
    }
    num10 = fmax(num10, num2 + num8 + num9);
    if m.has("_UnderlaySoftness") && kw(m, "UNDERLAY_ON") {
        if m.has("_ScaleRatioC") {
            num7 = m.float("_ScaleRatioC");
        }
        let num11 = m.float("_UnderlayOffsetX") * num7;
        let num12 = m.float("_UnderlayOffsetY") * num7;
        let num13 = m.float("_UnderlayDilate") * num7;
        let num14 = m.float("_UnderlaySoftness") * num7;
        zero[0] = fmax(zero[0], num2 + num13 + num14 - num11);
        zero[1] = fmax(zero[1], num2 + num13 + num14 - num12);
        zero[2] = fmax(zero[2], num2 + num13 + num14 + num11);
        zero[3] = fmax(zero[3], num2 + num13 + num14 + num12);
    }
    for z in &mut zero {
        *z = fmax(*z, num10);
        *z += num as f32;
        *z = fmin(*z, 1.0);
    }
    let num15 = m.float("_GradientScale");
    for z in &mut zero {
        *z *= num15;
    }
    num10 = fmax(zero[0], zero[1]);
    num10 = fmax(zero[2], num10);
    num10 = fmax(zero[3], num10);
    num10 + 1.25
}

/// TMP_MaterialManager.GetFallbackMaterial(sourceMaterial, targetMaterial).
fn fallback_material(source: &SdfMaterial, target: &SdfMaterial) -> SdfMaterial {
    if source.has("_GradientScale") && target.has("_GradientScale") {
        let mut m = source.clone();
        for k in ["_GradientScale", "_TextureWidth", "_TextureHeight", "_WeightNormal", "_WeightBold"] {
            m.floats.insert(k.into(), target.float(k));
        }
        m
    } else {
        target.clone()
    }
}

/// TMP_LineInfo (the fields layout reads).
#[derive(Clone, Copy, Default)]
struct LineInfo {
    control_character_count: i32,
    character_count: i32,
    visible_character_count: i32,
    space_count: i32,
    first_character_index: i32,
    first_visible_character_index: i32,
    last_character_index: i32,
    last_visible_character_index: i32,
    length: f32,
    line_height: f32,
    ascender: f32,
    baseline: f32,
    descender: f32,
    max_advance: f32,
    width: f32,
    margin_left: f32,
    margin_right: f32,
    alignment: i32,
    ext_min: [f32; 2],
    ext_max: [f32; 2],
}

impl LineInfo {
    /// A fresh `new TMP_LineInfo[n]` slot after ResizeLineExtents.
    fn resized() -> Self {
        Self { ascender: LARGE_NEG, descender: LARGE_POS, ext_min: [LARGE_POS_V2, LARGE_POS_V2], ext_max: [LARGE_NEG_V2, LARGE_NEG_V2], ..Default::default() }
    }
    /// TMP_TextInfo.ClearLineInfo.
    fn clear(&mut self) {
        self.character_count = 0;
        self.space_count = 0;
        self.control_character_count = 0;
        self.width = 0.0;
        self.ascender = LARGE_NEG;
        self.descender = LARGE_POS;
        self.margin_left = 0.0;
        self.margin_right = 0.0;
        self.ext_min = [LARGE_POS, LARGE_POS];
        self.ext_max = [LARGE_NEG, LARGE_NEG];
        self.max_advance = 0.0;
    }
}

#[derive(Clone, Copy, Default)]
struct Vertex {
    pos: V3,
    uv: [f32; 2],
    uv2: [f32; 2],
    color: C32,
}

/// TMP_CharacterInfo (the fields GenerateTextMesh reads back).
#[derive(Clone)]
struct CI {
    elem: Option<Elem>,
    /// characterInfo.isUsingAlternateTypeface (set with a font character's element in SetArraySizes).
    alt: bool,
    /// characterInfo.spriteAsset and spriteIndex of an elementType Sprite character.
    sprite: Option<(Arc<SpriteAsset>, i32)>,
    font: Arc<FontAsset>,
    /// Arena index of `material`.
    material: usize,
    mat_idx: usize,
    character: u16,
    /// characterInfo.vertexIndex / 4: the character's quad in its material's mesh (0 until it is drawn).
    quad: usize,
    scale: f32,
    point_size: f32,
    color: C32,
    underline_color: C32,
    strikethrough_color: C32,
    style: i32,
    is_visible: bool,
    line_number: i32,
    bl: V3,
    tl: V3,
    tr: V3,
    br: V3,
    origin: f32,
    baseline: f32,
    ascender: f32,
    descender: f32,
    adjusted_ascender: f32,
    adjusted_descender: f32,
    x_advance: f32,
    aspect_ratio: f32,
    /// vertex_BL, vertex_TL, vertex_TR, vertex_BR.
    v: [Vertex; 4],
}

impl CI {
    fn new(font: &Arc<FontAsset>) -> Self {
        Self {
            elem: None,
            alt: false,
            sprite: None,
            font: font.clone(),
            material: 0,
            mat_idx: 0,
            character: 0,
            quad: 0,
            scale: 0.0,
            point_size: 0.0,
            color: C32::default(),
            underline_color: C32::default(),
            strikethrough_color: C32::default(),
            style: 0,
            is_visible: false,
            line_number: 0,
            bl: [0.0; 3],
            tl: [0.0; 3],
            tr: [0.0; 3],
            br: [0.0; 3],
            origin: 0.0,
            baseline: 0.0,
            ascender: 0.0,
            descender: 0.0,
            adjusted_ascender: 0.0,
            adjusted_descender: 0.0,
            x_advance: 0.0,
            aspect_ratio: 0.0,
            v: [Vertex::default(); 4],
        }
    }
}

/// WordWrapState (the stacks share their arrays with the live ones, as the C# struct copies do).
#[derive(Clone)]
struct WordWrapState {
    previous_word_break: i32,
    total_character_count: i32,
    visible_character_count: i32,
    first_character_index: i32,
    first_visible_character_index: i32,
    last_visible_char_index: i32,
    line_number: i32,
    max_cap_height: f32,
    max_ascender: f32,
    max_descender: f32,
    start_of_line_ascender: f32,
    max_line_ascender: f32,
    max_line_descender: f32,
    page_ascender: f32,
    horizontal_alignment: i32,
    margin_left: f32,
    margin_right: f32,
    x_advance: f32,
    preferred_width: f32,
    preferred_height: f32,
    font_style: i32,
    italic_angle: i32,
    font_scale_multiplier: f32,
    current_font_size: f32,
    baseline_offset: f32,
    line_offset: f32,
    is_driven_line_spacing: bool,
    glyph_horizontal_advance_adjustment: f32,
    c_space: f32,
    m_space: f32,
    line_info: LineInfo,
    vertex_color: C32,
    underline_color: C32,
    strikethrough_color: C32,
    basic_style_stack: FontStyleStack,
    italic_angle_stack: Stack<i32>,
    color_stack: Stack<C32>,
    underline_color_stack: Stack<C32>,
    size_stack: Stack<f32>,
    font_weight_stack: Stack<i32>,
    line_justification_stack: Stack<i32>,
    current_font: Arc<FontAsset>,
    current_material: usize,
    current_mat_idx: usize,
    mesh_ext_min: [f32; 2],
    mesh_ext_max: [f32; 2],
    tag_no_parsing: bool,
    is_non_breaking_space: bool,
}

impl WordWrapState {
    /// `default(WordWrapState)`: zeros, and stacks without an array (using them is a NullReferenceException in C#).
    fn blank(font: &Arc<FontAsset>) -> Self {
        Self {
            previous_word_break: 0,
            total_character_count: 0,
            visible_character_count: 0,
            first_character_index: 0,
            first_visible_character_index: 0,
            last_visible_char_index: 0,
            line_number: 0,
            max_cap_height: 0.0,
            max_ascender: 0.0,
            max_descender: 0.0,
            start_of_line_ascender: 0.0,
            max_line_ascender: 0.0,
            max_line_descender: 0.0,
            page_ascender: 0.0,
            horizontal_alignment: 0,
            margin_left: 0.0,
            margin_right: 0.0,
            x_advance: 0.0,
            preferred_width: 0.0,
            preferred_height: 0.0,
            font_style: 0,
            italic_angle: 0,
            font_scale_multiplier: 0.0,
            current_font_size: 0.0,
            baseline_offset: 0.0,
            line_offset: 0.0,
            is_driven_line_spacing: false,
            glyph_horizontal_advance_adjustment: 0.0,
            c_space: 0.0,
            m_space: 0.0,
            line_info: LineInfo::default(),
            vertex_color: C32::default(),
            underline_color: C32::default(),
            strikethrough_color: C32::default(),
            basic_style_stack: FontStyleStack::default(),
            italic_angle_stack: Stack::new(16),
            color_stack: Stack::new(16),
            underline_color_stack: Stack::new(16),
            size_stack: Stack::new(16),
            font_weight_stack: Stack::new(16),
            line_justification_stack: Stack::new(16),
            current_font: font.clone(),
            current_material: 0,
            current_mat_idx: 0,
            mesh_ext_min: [0.0; 2],
            mesh_ext_max: [0.0; 2],
            tag_no_parsing: false,
            is_non_breaking_space: false,
        }
    }
}

/// TMP_TextProcessingStack<WordWrapState>(8, 8): m_EllipsisInsertionCandidateStack, a rollover stack.
struct RolloverStack {
    items: Vec<Option<WordWrapState>>,
    index: usize,
    count: usize,
}

impl RolloverStack {
    fn new() -> Self {
        Self { items: vec![None; 8], index: 0, count: 0 }
    }
    fn clear(&mut self) {
        self.index = 0;
        self.count = 0;
    }
    fn push(&mut self, item: WordWrapState) {
        self.items[self.index] = Some(item);
        self.index = (self.index + 1) % 8;
        self.count = if self.count < 8 { self.count + 1 } else { 8 };
    }
    fn pop(&mut self, blank: &WordWrapState) -> WordWrapState {
        self.index = (self.index + 7) % 8;
        let r = self.items[self.index].take();
        self.count = self.count.saturating_sub(1);
        r.unwrap_or_else(|| blank.clone())
    }
}

/// TMP_Text.m_Ellipsis / m_Underline (SpecialCharacter).
#[derive(Clone)]
struct Special {
    elem: Elem,
    material: usize,
    mat_idx: usize,
}

fn name_hash_step(h: i32, c: i32) -> i32 {
    h.wrapping_shl(3).wrapping_sub(h).wrapping_add(c)
}
fn value_hash_step(h: i32, c: i32) -> i32 {
    h.wrapping_shl(5).wrapping_add(h) ^ c
}

/// The tag name hash ValidateHtmlTag builds (case sensitive).
#[cfg(test)]
fn tag_name_hash(s: &str) -> i32 {
    s.chars().fold(0, |h, c| name_hash_step(h, c as i32))
}
#[cfg(test)]
fn tag_value_hash(s: &str) -> i32 {
    s.chars().fold(0, |h, c| value_hash_step(h, c as i32))
}

/// TextMeshPro's instance state across OnPreRenderObject and the preferred-size queries.
struct G<'a> {
    p: &'a TextProps,
    font0: Arc<FontAsset>,
    // component state
    font_size: f32,
    max_font_size: f32,
    min_font_size: f32,
    line_spacing_delta: f32,
    char_width_adj_delta: f32,
    is_text_truncated: bool,
    auto_size_iteration_count: i32,
    is_auto_size_point_size_set: bool,
    overflow_mode: i32,
    margin_width: f32,
    margin_height: f32,
    corners: [V3; 4],
    // the text and what SetArraySizes made of it
    tp: Vec<i32>,
    total_character_count: i32,
    chars: Vec<CI>,
    arena: Vec<ArenaMat>,
    mats: Vec<MatRef>,
    line_info: Vec<LineInfo>,
    ellipsis: Option<Special>,
    is_sdf_shader: bool,
    // tag parsing
    html_tag: [u16; 128],
    xml: [Attr; 8],
    is_calculating_preferred_values: bool,
    // parse / layout state (the m_ fields)
    cur_font: Arc<FontAsset>,
    cur_material: usize,
    cur_mat_idx: usize,
    font_style_internal: i32,
    font_weight_internal: i32,
    font_style_stack: FontStyleStack,
    font_weight_stack: Stack<i32>,
    line_justification: i32,
    line_justification_stack: Stack<i32>,
    size_stack: Stack<f32>,
    color_stack: Stack<C32>,
    underline_color_stack: Stack<C32>,
    italic_angle_stack: Stack<i32>,
    italic_angle: i32,
    html_color: C32,
    font_color32: C32,
    underline_color: C32,
    strikethrough_color: C32,
    current_font_size: f32,
    font_scale_multiplier: f32,
    baseline_offset: f32,
    line_offset: f32,
    c_spacing: f32,
    mono_spacing: f32,
    x_advance: f32,
    tag_no_parsing: bool,
    is_non_breaking_space: bool,
    character_count: i32,
    first_character_of_line: i32,
    last_character_of_line: i32,
    first_visible_character_of_line: i32,
    last_visible_character_of_line: i32,
    max_line_ascender: f32,
    max_line_descender: f32,
    line_number: i32,
    start_of_line_ascender: f32,
    line_visible_character_count: i32,
    line_height: f32,
    is_driven_line_spacing: bool,
    first_overflow_character_index: i32,
    margin_left: f32,
    margin_right: f32,
    width: f32,
    mesh_ext_min: [f32; 2],
    mesh_ext_max: [f32; 2],
    max_cap_height: f32,
    max_text_ascender: f32,
    element_ascender: f32,
    element_descender: f32,
    page_ascender: f32,
    glyph_horizontal_advance_adjustment: f32,
    preferred_width: f32,
    preferred_height: f32,
    // saved states
    saved_word_wrap: WordWrapState,
    saved_line: WordWrapState,
    saved_ellipsis: WordWrapState,
    saved_last_valid: WordWrapState,
    saved_soft_line_break: WordWrapState,
    ellipsis_stack: RolloverStack,
    // output: character quads per material reference, then the underline / strikethrough quads (material 0)
    quads: Vec<Vec<Quad>>,
    underline_quads: Vec<Quad>,
    // sprites: m_textElementType (0 character, 1 sprite), m_spriteColor, m_tintSprite, m_spriteIndex, m_currentSpriteAsset, m_isParsingText
    text_element_type: u8,
    sprite_color: C32,
    tint_sprite: bool,
    sprite_index: i32,
    current_sprite: Option<Arc<SpriteAsset>>,
    is_parsing_text: bool,
    /// TMP_SpriteAnimator.DoSpriteAnimation calls since the last StopAllAnimations (None: GenerateTextMesh never reached it).
    sprite_anims: Option<Vec<super::SpriteAnim>>,
}

impl<'a> G<'a> {
    fn new(p: &'a TextProps) -> Self {
        let font0 = p.font.clone();
        let primary = if p.material.material.is_some() { p.material.clone() } else { font0.material.clone() };
        // The RectTransform's rect: x = -pivot.x * w, xMax = x + w (GetLocalCorners).
        let (w, h) = (p.rect_size[0], p.rect_size[1]);
        let (x_min, y_min) = (-p.pivot[0] * w, -p.pivot[1] * h);
        let (x_max, y_max) = (x_min + w, y_min + h);
        let mut g = Self {
            p,
            font0: font0.clone(),
            font_size: p.font_size,
            max_font_size: 0.0,
            min_font_size: 0.0,
            line_spacing_delta: 0.0,
            char_width_adj_delta: 0.0,
            is_text_truncated: false,
            auto_size_iteration_count: 0,
            is_auto_size_point_size_set: false,
            overflow_mode: p.overflow_mode,
            // ComputeMarginSize
            margin_width: w - p.margin[0] - p.margin[2],
            margin_height: h - p.margin[1] - p.margin[3],
            corners: [[x_min, y_min, 0.0], [x_min, y_max, 0.0], [x_max, y_max, 0.0], [x_max, y_min, 0.0]],
            tp: vec![0],
            total_character_count: 0,
            chars: Vec::new(),
            arena: Vec::new(),
            mats: Vec::new(),
            line_info: vec![LineInfo::default(); 2],
            ellipsis: None,
            is_sdf_shader: primary.has("_WeightNormal"),
            html_tag: [0; 128],
            xml: [Attr::default(); 8],
            is_calculating_preferred_values: false,
            cur_font: font0.clone(),
            cur_material: 0,
            cur_mat_idx: 0,
            font_style_internal: p.font_style,
            font_weight_internal: p.font_weight,
            font_style_stack: FontStyleStack::default(),
            font_weight_stack: Stack::new(8),
            line_justification: p.horizontal_alignment,
            line_justification_stack: Stack::new(16),
            size_stack: Stack::new(16),
            color_stack: Stack::new(16),
            underline_color_stack: Stack::new(16),
            italic_angle_stack: Stack::new(16),
            italic_angle: 0,
            html_color: C32::default(),
            font_color32: C32::from_color(p.font_color),
            underline_color: C32::default(),
            strikethrough_color: C32::default(),
            current_font_size: 0.0,
            font_scale_multiplier: 1.0,
            baseline_offset: 0.0,
            line_offset: 0.0,
            c_spacing: 0.0,
            mono_spacing: 0.0,
            x_advance: 0.0,
            tag_no_parsing: false,
            is_non_breaking_space: false,
            character_count: 0,
            first_character_of_line: 0,
            last_character_of_line: 0,
            first_visible_character_of_line: 0,
            last_visible_character_of_line: 0,
            max_line_ascender: LARGE_NEG,
            max_line_descender: LARGE_POS,
            line_number: 0,
            start_of_line_ascender: 0.0,
            line_visible_character_count: 0,
            line_height: LINE_HEIGHT_UNSET,
            is_driven_line_spacing: false,
            first_overflow_character_index: -1,
            margin_left: 0.0,
            margin_right: 0.0,
            width: -1.0,
            mesh_ext_min: [LARGE_POS_V2, LARGE_POS_V2],
            mesh_ext_max: [LARGE_NEG_V2, LARGE_NEG_V2],
            max_cap_height: 0.0,
            max_text_ascender: 0.0,
            element_ascender: 0.0,
            element_descender: 0.0,
            page_ascender: 0.0,
            glyph_horizontal_advance_adjustment: 0.0,
            preferred_width: 0.0,
            preferred_height: 0.0,
            saved_word_wrap: WordWrapState::blank(&font0),
            saved_line: WordWrapState::blank(&font0),
            saved_ellipsis: WordWrapState::blank(&font0),
            saved_last_valid: WordWrapState::blank(&font0),
            saved_soft_line_break: WordWrapState::blank(&font0),
            ellipsis_stack: RolloverStack::new(),
            quads: Vec::new(),
            underline_quads: Vec::new(),
            text_element_type: 0,
            sprite_color: C32::WHITE,
            tint_sprite: false,
            sprite_index: -1,
            current_sprite: None,
            is_parsing_text: false,
            sprite_anims: None,
        };
        let m = g.arena_material(MKey::Obj(primary.material), || primary.clone());
        g.cur_material = m;
        g.mats = vec![MatRef { arena: m, font: font0, ref_count: 0, sprite: None }];
        g
    }

    /// The arena entry of a material instance (created, with UpdateShaderRatios applied and its padding, on first use).
    fn arena_material(&mut self, key: MKey, make: impl FnOnce() -> SdfMaterial) -> usize {
        if let Some(i) = self.arena.iter().position(|a| a.key == key) {
            return i;
        }
        let mut mat = make();
        let padding = shader_padding(&mut mat, self.p.enable_extra_padding);
        self.arena.push(ArenaMat { key, mat, padding });
        self.arena.len() - 1
    }

    /// The arena entry of a font asset's own material.
    fn font_material(&mut self, font: &Arc<FontAsset>) -> usize {
        let f = font.clone();
        self.arena_material(MKey::Obj(f.material.material), || f.material.clone())
    }

    /// TMP_MaterialManager.GetFallbackMaterial(source, font.material), as an arena entry.
    fn fallback_material_of(&mut self, source: usize, target: &Arc<FontAsset>) -> usize {
        let src = self.arena[source].mat.clone();
        let t = target.clone();
        self.arena_material(MKey::Fallback(source, t.atlas), || fallback_material(&src, &t.material))
    }

    /// MaterialReference.AddMaterialReference (an existing entry is returned as is).
    fn add_material_reference(&mut self, arena: usize, font: &Arc<FontAsset>) -> usize {
        if let Some(i) = self.mats.iter().position(|m| m.arena == arena) {
            return i;
        }
        self.mats.push(MatRef { arena, font: font.clone(), ref_count: 0, sprite: None });
        self.mats.len() - 1
    }

    /// MaterialReference.AddMaterialReference(material, spriteAsset): an existing entry for the material is returned as is.
    fn add_sprite_material_reference(&mut self, asset: &Arc<SpriteAsset>) -> usize {
        let m = asset.material.clone();
        let arena = self.arena_material(MKey::Obj(asset.material.material), || m);
        if let Some(i) = self.mats.iter().position(|m| m.arena == arena) {
            return i;
        }
        self.mats.push(MatRef { arena, font: self.font0.clone(), ref_count: 0, sprite: Some(asset.clone()) });
        self.mats.len() - 1
    }

    fn padding0(&self) -> f32 {
        self.arena[self.mats[0].arena].padding
    }

    // ---- ParseInputText ----

    /// TMP_Text.GetMarkupTagHashCode.
    fn markup_tag_hash(t: &[u32], mut read: usize) -> i32 {
        let mut num = 0i32;
        let end = read + 16;
        while read < end {
            let c = t.get(read).copied().unwrap_or(0);
            if c == 62 || c == 61 || c == 32 {
                return num;
            }
            num = value_hash_step(num, to_upper_ascii_fast(c) as i32);
            read += 1;
        }
        num
    }

    /// TMP_Text.PopulateTextProcessingArray.
    fn populate_text_processing_array(&mut self) {
        let t: Vec<u32> = self.p.text.encode_utf16().map(u32::from).collect();
        let count = t.len();
        let at = |i: usize| t.get(i).copied().unwrap_or(0);
        let hx = |i: usize| hex_to_int(at(i) as u16);
        let utf16 = |i: usize| (hx(i) << 12) + (hx(i + 1) << 8) + (hx(i + 2) << 4) + hx(i + 3);
        let utf32 = |i: usize| (hx(i) << 28) + (hx(i + 1) << 24) + (hx(i + 2) << 20) + (hx(i + 3) << 16) + (hx(i + 4) << 12) + (hx(i + 5) << 8) + (hx(i + 6) << 4) + hx(i + 7);
        let ctrl = self.p.parse_ctrl_characters;
        let mut out: Vec<i32> = Vec::new();
        let mut i = 0usize;
        while i < count {
            let num = at(i);
            if num == 0 {
                break;
            }
            if self.p.text_input_box && num == 92 && i + 1 < count {
                match at(i + 1) {
                    92 if ctrl && count > i + 2 => {
                        out.push(at(i + 1) as i32);
                        out.push(at(i + 2) as i32);
                        i += 3;
                        continue;
                    }
                    110 if ctrl => {
                        out.push(10);
                        i += 2;
                        continue;
                    }
                    114 if ctrl => {
                        out.push(13);
                        i += 2;
                        continue;
                    }
                    116 if ctrl => {
                        out.push(9);
                        i += 2;
                        continue;
                    }
                    118 if ctrl => {
                        out.push(11);
                        i += 2;
                        continue;
                    }
                    117 if count > i + 5 => {
                        out.push(utf16(i + 2));
                        i += 6;
                        continue;
                    }
                    85 if count > i + 9 => {
                        out.push(utf32(i + 2));
                        i += 10;
                        continue;
                    }
                    _ => {}
                }
            }
            if (55296..=56319).contains(&num) && count > i + 1 && (56320..=57343).contains(&at(i + 1)) {
                // TMP_TextParsingUtilities.ConvertToUTF32
                out.push(((num - 55296) * 1024 + (at(i + 1) - 56320 + 65536)) as i32);
                i += 2;
                continue;
            }
            if num == 60 && self.p.is_rich_text {
                match Self::markup_tag_hash(&t, i + 1) {
                    2256 => {
                        out.push(10);
                        i += 4;
                        continue;
                    }
                    2869039 => {
                        out.push(160);
                        i += 6;
                        continue;
                    }
                    3288238 => {
                        out.push(8203);
                        i += 6;
                        continue;
                    }
                    100252951 | 1927738392 => unimplemented!("mod-only: TMP <style> / </style> (text style sheets): no shipped text or script string has the tag"),
                    _ => {}
                }
            }
            out.push(num as i32);
            i += 1;
        }
        out.push(0);
        self.tp = out;
    }

    /// TMP_Text.ParseInputText.
    fn parse_input_text(&mut self) {
        self.populate_text_processing_array();
        self.set_array_sizes();
    }

    // ---- character lookup ----

    /// TMP_Text.GetTextElement.
    fn get_text_element(&mut self, unicode: u32, font: &Arc<FontAsset>, style: i32, weight: i32) -> Option<Elem> {
        let p = self.p;
        let s = &p.settings;
        let mut c = char_from_font(unicode, font, false, style, weight);
        if c.is_some() {
            return c;
        }
        if !font.fallbacks.is_empty() {
            c = char_from_fonts(unicode, &font.fallbacks, true, style, weight);
        }
        if c.is_some() {
            return c;
        }
        let font0 = self.font0.clone();
        if !same_font(font, &font0) {
            c = char_from_font(unicode, &font0, false, style, weight);
            if c.is_some() {
                self.cur_mat_idx = 0;
                self.cur_material = self.mats[0].arena;
                return c;
            }
            if !font0.fallbacks.is_empty() {
                c = char_from_fonts(unicode, &font0.fallbacks, true, style, weight);
            }
            if c.is_some() {
                return c;
            }
        }
        if p.has_sprite_asset {
            unimplemented!("mod-only: a TextMeshPro with its own sprite asset (m_spriteAsset): no shipped component assigns one");
        }
        if !s.fallback_fonts.is_empty() {
            c = char_from_fonts(unicode, &s.fallback_fonts, true, style, weight);
        }
        if c.is_some() {
            return c;
        }
        if let Some(d) = &s.default_font {
            c = char_from_font(unicode, d, true, style, weight);
        }
        if c.is_some() {
            return c;
        }
        // TMP_Settings.defaultSpriteAsset's sprite characters (with its fallbacks).
        if let Some(d) = &s.default_sprite {
            if let Some((asset, n)) = d.character(unicode) {
                let c = sprite_character(&asset, n);
                return Some(sprite_elem(font, &asset, &c));
            }
        }
        None
    }

    /// TMP_Text.GetEllipsisSpecialCharacter (keeps the previous one when the character is not found).
    fn get_ellipsis_special_character(&mut self, font: &Arc<FontAsset>) {
        let (style, weight) = (self.font_style_internal, self.font_weight_internal);
        let p = self.p;
        let s = &p.settings;
        let mut c = char_from_font(8230, font, false, style, weight);
        if c.is_none() && !font.fallbacks.is_empty() {
            c = char_from_fonts(8230, &font.fallbacks, true, style, weight);
        }
        if c.is_none() && !s.fallback_fonts.is_empty() {
            c = char_from_fonts(8230, &s.fallback_fonts, true, style, weight);
        }
        if c.is_none() {
            if let Some(d) = &s.default_font {
                c = char_from_font(8230, d, true, style, weight);
            }
        }
        if let Some(elem) = c {
            let material = self.font_material(&elem.font);
            self.ellipsis = Some(Special { elem, material, mat_idx: 0 });
        }
    }

    // ---- SetArraySizes ----

    /// TextMeshPro.SetArraySizes.
    fn set_array_sizes(&mut self) -> i32 {
        let p = self.p;
        self.total_character_count = 0;
        self.is_parsing_text = false;
        self.text_element_type = 0;
        self.tag_no_parsing = false;
        self.font_style_internal = p.font_style;
        self.font_style_stack = FontStyleStack::default();
        self.font_weight_internal = if self.font_style_internal & BOLD == BOLD { WEIGHT_BOLD } else { p.font_weight };
        self.font_weight_stack.set_default(self.font_weight_internal);
        self.cur_font = self.font0.clone();
        // m_currentMaterial = m_sharedMaterial
        let primary = self.mats[0].arena;
        self.cur_material = primary;
        self.cur_mat_idx = 0;
        self.mats = vec![MatRef { arena: primary, font: self.font0.clone(), ref_count: 0, sprite: None }];
        if self.overflow_mode == ELLIPSIS {
            let f = self.cur_font.clone();
            self.get_ellipsis_special_character(&f);
            if let Some(e) = self.ellipsis.clone() {
                if !same_font(&e.elem.font, &self.cur_font) {
                    let own = self.font_material(&e.elem.font);
                    let material = if p.settings.match_material_preset && self.arena[self.cur_material].key != self.arena[own].key {
                        let cur = self.cur_material;
                        self.fallback_material_of(cur, &e.elem.font)
                    } else {
                        own
                    };
                    let mat_idx = self.add_material_reference(material, &e.elem.font);
                    self.mats[mat_idx].ref_count = 0;
                    self.ellipsis = Some(Special { elem: e.elem, material, mat_idx });
                }
            } else {
                self.overflow_mode = TRUNCATE;
            }
        }
        if self.overflow_mode == 6 {
            unimplemented!("mod-only: TMP Linked overflow (shipped texts use overflow modes 0-4)");
        }
        let mut i = 0usize;
        while i < self.tp.len() && self.tp[i] != 0 {
            let total = self.total_character_count as usize;
            while self.chars.len() <= total {
                self.chars.push(CI::new(&self.font0));
            }
            let mut num2 = self.tp[i];
            if p.is_rich_text && num2 == 60 {
                let saved_idx = self.cur_mat_idx;
                if let Some(end) = self.validate_html_tag(i + 1) {
                    if self.text_element_type == 1 {
                        // A <sprite> tag is a character of its own.
                        let sa = self.current_sprite.clone().expect("sprite asset");
                        let sidx = self.sprite_index;
                        let sc = sprite_character(&sa, sidx);
                        self.mats[self.cur_mat_idx].ref_count += 1;
                        let (mi, cf) = (self.cur_mat_idx, self.cur_font.clone());
                        let e = sprite_elem(&cf, &sa, &sc);
                        let ci = &mut self.chars[total];
                        ci.character = (57344 + sidx) as u16;
                        ci.sprite = Some((sa, sidx));
                        ci.font = cf;
                        ci.mat_idx = mi;
                        ci.elem = Some(e);
                        self.text_element_type = 0;
                        self.cur_mat_idx = saved_idx;
                        self.total_character_count += 1;
                    }
                    i = end + 1;
                    continue;
                }
            }
            let mut flag = false;
            let cur_font_save = self.cur_font.clone();
            let cur_material_save = self.cur_material;
            let cur_idx_save = self.cur_mat_idx;
            if self.font_style_internal & UPPER_CASE == UPPER_CASE {
                if char_is_lower(num2) {
                    num2 = char_to_upper(num2);
                }
            } else if self.font_style_internal & LOWER_CASE == LOWER_CASE {
                if char_is_upper(num2) {
                    num2 = char_to_lower(num2);
                }
            } else if self.font_style_internal & SMALL_CAPS == SMALL_CAPS && char_is_lower(num2) {
                num2 = char_to_upper(num2);
            }
            let (style, weight) = (self.font_style_internal, self.font_weight_internal);
            let cf = self.cur_font.clone();
            let mut elem = self.get_text_element(num2 as u32, &cf, style, weight);
            if elem.is_none() {
                let s = &p.settings;
                num2 = if s.missing_glyph == 0 { 9633 } else { s.missing_glyph as i32 };
                self.tp[i] = num2;
                elem = char_from_font(num2 as u32, &cf, true, style, weight);
                if elem.is_none() && !s.fallback_fonts.is_empty() {
                    elem = char_from_fonts(num2 as u32, &s.fallback_fonts, true, style, weight);
                }
                if elem.is_none() {
                    if let Some(d) = &s.default_font {
                        elem = char_from_font(num2 as u32, d, true, style, weight);
                    }
                }
                if elem.is_none() {
                    num2 = 32;
                    self.tp[i] = 32;
                    elem = char_from_font(32, &cf, true, style, weight);
                }
                if elem.is_none() {
                    num2 = 3;
                    self.tp[i] = 3;
                    elem = char_from_font(3, &cf, true, style, weight);
                }
            }
            let elem = elem.expect("TMP: no character, space or U+0003 in the font asset (NullReferenceException in TMP)");
            if let Some(sa) = elem.sprite.clone() {
                // A character of a sprite asset: the sprite's material, spriteIndex = the character's glyph index.
                {
                    let cf = self.cur_font.clone();
                    let ci = &mut self.chars[total];
                    ci.elem = Some(elem.clone());
                    ci.character = num2 as u16;
                    ci.font = cf;
                    ci.sprite = Some((sa.clone(), elem.ch.glyph_index as i32));
                }
                let mi = self.add_sprite_material_reference(&sa);
                self.mats[mi].ref_count += 1;
                self.chars[total].mat_idx = mi;
                self.total_character_count += 1;
                i += 1;
                continue;
            }
            if !same_font(&elem.font, &self.cur_font) {
                flag = true;
                self.cur_font = elem.font.clone();
            }
            {
                let cf = self.cur_font.clone();
                let ci = &mut self.chars[total];
                ci.elem = Some(elem.clone());
                ci.alt = elem.alt;
                ci.sprite = None;
                ci.character = num2 as u16;
                ci.font = cf;
            }
            if flag && !same_font(&self.cur_font, &self.font0) {
                let cf = self.cur_font.clone();
                self.cur_material = if p.settings.match_material_preset {
                    let cur = self.cur_material;
                    self.fallback_material_of(cur, &cf)
                } else {
                    self.font_material(&cf)
                };
                let m = self.cur_material;
                self.cur_mat_idx = self.add_material_reference(m, &cf);
            }
            if elem.glyph.atlas_index > 0 {
                unimplemented!("mod-only: TMP glyph in atlas texture {} (multi atlas fallback material)", elem.glyph.atlas_index);
            }
            if !is_white_space(num2) && num2 != 8203 {
                if self.mats[self.cur_mat_idx].ref_count < 16383 {
                    self.mats[self.cur_mat_idx].ref_count += 1;
                } else {
                    unimplemented!("mod-only: TMP more than 16383 characters in one material (new Material copy)");
                }
            }
            {
                let (m, mi) = (self.cur_material, self.cur_mat_idx);
                let ci = &mut self.chars[total];
                ci.material = m;
                ci.mat_idx = mi;
            }
            if flag {
                self.cur_font = cur_font_save;
                self.cur_material = cur_material_save;
                self.cur_mat_idx = cur_idx_save;
            }
            self.total_character_count += 1;
            i += 1;
        }
        if self.is_calculating_preferred_values {
            self.is_calculating_preferred_values = false;
            return self.total_character_count;
        }
        self.total_character_count
    }

    // ---- rich text ----

    /// TMP_Text.ConvertToFloat (-32768 when it fails).
    fn convert_to_float(&self, start: usize, length: usize) -> f32 {
        self.convert_to_float_last(start, length).0
    }

    /// TMP_Text.GetAttributeParameters into the static 16-entry m_attributeParameterValues (a 17th value throws).
    fn get_attribute_parameters(&self, start: usize, length: usize) -> Vec<f32> {
        let (mut start, mut length) = (start as i64, length as i64);
        let mut last = start;
        let mut out = Vec::new();
        while last < start + length {
            if out.len() == 16 {
                crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.");
            }
            let (v, l) = self.convert_to_float_last(start as usize, length as usize);
            out.push(v);
            last = l as i64;
            length -= last - start + 1;
            start = last + 1;
        }
        out
    }

    /// TMP_Text.ConvertToFloat(chars, startIndex, length, out lastIndex).
    fn convert_to_float_last(&self, start: usize, length: usize) -> (f32, usize) {
        let mut start = start;
        if start == 0 {
            return (-32768.0, 0);
        }
        let num = start + length;
        let mut flag = true;
        let mut num2 = 0.0f32;
        let mut num3: i32 = 1;
        if self.html_tag[start] == b'+' as u16 {
            num3 = 1;
            start += 1;
        } else if self.html_tag[start] == b'-' as u16 {
            num3 = -1;
            start += 1;
        }
        let mut num4 = 0.0f32;
        let mut i = start;
        while i < num {
            let c = self.html_tag[i] as u32;
            if c < 48 || c > 57 {
                match c {
                    46 => {}
                    44 => {
                        let last = if i + 1 < num && self.html_tag[i + 1] == b' ' as u16 { i + 1 } else { i };
                        return (if num4 > 32767.0 { -32768.0 } else { num4 }, last);
                    }
                    _ => {
                        i += 1;
                        continue;
                    }
                }
            }
            if c == 46 {
                flag = false;
                num2 = 0.1;
            } else if flag {
                num4 = num4 * 10.0 + ((c as i32 - 48) * num3) as f32;
            } else {
                num4 += (c - 48) as f32 * num2 * num3 as f32;
                num2 *= 0.1;
            }
            i += 1;
        }
        (if num4 > 32767.0 { -32768.0 } else { num4 }, num)
    }

    /// TMP_Text.HexCharsToColor(char[] hexChars, int tagCount).
    fn hex_chars_to_color(&self, tag_count: usize) -> C32 {
        let h = |i: usize| hex_to_int(self.html_tag[i]);
        let b = |hi: usize, lo: usize| (h(hi) * 16 + h(lo)) as u8;
        match tag_count {
            4 => C32::new(b(1, 1), b(2, 2), b(3, 3), 255),
            5 => C32::new(b(1, 1), b(2, 2), b(3, 3), b(4, 4)),
            7 => C32::new(b(1, 2), b(3, 4), b(5, 6), 255),
            9 => C32::new(b(1, 2), b(3, 4), b(5, 6), b(7, 8)),
            10 => C32::new(b(7, 7), b(8, 8), b(9, 9), 255),
            11 => C32::new(b(7, 7), b(8, 8), b(9, 9), b(10, 10)),
            13 => C32::new(b(7, 8), b(9, 10), b(11, 12), 255),
            15 => C32::new(b(7, 8), b(9, 10), b(11, 12), b(13, 14)),
            _ => C32::WHITE,
        }
    }

    fn reset_attr(&mut self, n: usize) {
        self.xml[n] = Attr::default();
    }

    /// TMP_Text.ValidateHtmlTag: `Some(endIndex)` (index of the '>') when the text at `start_index` is a tag.
    fn validate_html_tag(&mut self, start_index: usize) -> Option<usize> {
        let mut num = 0usize;
        let mut b = 0u8;
        let mut num2 = 0usize;
        self.reset_attr(0);
        let mut tag_value_type = 0u8;
        let mut tag_unit = 0u8;
        for k in 1..=4 {
            self.xml[k].name_hash = 0;
        }
        let mut end_index = start_index;
        let mut flag = false;
        let mut flag2 = false;
        let mut i = start_index;
        while i < self.tp.len() && self.tp[i] != 0 {
            if num >= self.html_tag.len() {
                break;
            }
            if self.tp[i] == 60 {
                break;
            }
            let unicode = self.tp[i];
            if unicode == 62 {
                flag2 = true;
                end_index = i;
                self.html_tag[num] = 0;
                break;
            }
            self.html_tag[num] = unicode as u16;
            num += 1;
            if b == 1 {
                match tag_value_type {
                    0 => match unicode {
                        43 | 45 | 46 | 48..=57 => {
                            tag_unit = 0;
                            tag_value_type = 1;
                            self.xml[num2].value_type = 1;
                            self.xml[num2].value_start = num - 1;
                            self.xml[num2].value_len += 1;
                        }
                        35 => {
                            tag_unit = 0;
                            tag_value_type = 4;
                            self.xml[num2].value_type = 4;
                            self.xml[num2].value_start = num - 1;
                            self.xml[num2].value_len += 1;
                        }
                        34 => {
                            tag_unit = 0;
                            tag_value_type = 2;
                            self.xml[num2].value_type = 2;
                            self.xml[num2].value_start = num;
                        }
                        _ => {
                            tag_unit = 0;
                            tag_value_type = 2;
                            self.xml[num2].value_type = 2;
                            self.xml[num2].value_start = num - 1;
                            self.xml[num2].value_hash = value_hash_step(self.xml[num2].value_hash, unicode);
                            self.xml[num2].value_len += 1;
                        }
                    },
                    1 => {
                        if unicode == 112 || unicode == 101 || unicode == 37 || unicode == 32 {
                            b = 2;
                            tag_value_type = 0;
                            tag_unit = match unicode {
                                101 => {
                                    self.xml[num2].unit = 1;
                                    1
                                }
                                37 => {
                                    self.xml[num2].unit = 2;
                                    2
                                }
                                _ => {
                                    self.xml[num2].unit = 0;
                                    0
                                }
                            };
                            num2 += 1;
                            self.reset_attr(num2);
                        } else if b != 2 {
                            self.xml[num2].value_len += 1;
                        }
                    }
                    4 => {
                        if unicode != 32 {
                            self.xml[num2].value_len += 1;
                        } else {
                            b = 2;
                            tag_value_type = 0;
                            tag_unit = 0;
                            num2 += 1;
                            self.reset_attr(num2);
                        }
                    }
                    _ => {
                        if unicode != 34 {
                            self.xml[num2].value_hash = value_hash_step(self.xml[num2].value_hash, unicode);
                            self.xml[num2].value_len += 1;
                        } else {
                            b = 2;
                            tag_value_type = 0;
                            tag_unit = 0;
                            num2 += 1;
                            self.reset_attr(num2);
                        }
                    }
                }
            }
            if unicode == 61 {
                b = 1;
            }
            if b == 0 && unicode == 32 {
                if flag {
                    return None;
                }
                flag = true;
                b = 2;
                tag_value_type = 0;
                tag_unit = 0;
                num2 += 1;
                self.reset_attr(num2);
            }
            if b == 0 {
                self.xml[num2].name_hash = name_hash_step(self.xml[num2].name_hash, unicode);
            }
            if b == 2 && unicode == 32 {
                b = 0;
            }
            i += 1;
        }
        let _ = tag_value_type;
        if !flag2 {
            return None;
        }
        let name = self.xml[0].name_hash;
        if self.tag_no_parsing && name != 53822163 && name != 49429939 {
            return None;
        }
        if name == 53822163 || name == 49429939 {
            self.tag_no_parsing = false;
            return Some(end_index);
        }
        if self.html_tag[0] == b'#' as u16 && matches!(num, 4 | 5 | 7 | 9) {
            self.html_color = self.hex_chars_to_color(num);
            self.color_stack.add(self.html_color);
            return Some(end_index);
        }
        let ortho = if self.p.is_orthographic { 1.0f32 } else { 0.1f32 };
        match name {
            66 | 98 => {
                self.font_style_internal |= BOLD;
                self.font_style_stack.add(BOLD);
                self.font_weight_internal = WEIGHT_BOLD;
                Some(end_index)
            }
            395 | 427 => {
                if self.p.font_style & BOLD != BOLD && self.font_style_stack.remove(BOLD) == 0 {
                    self.font_style_internal &= !BOLD;
                    self.font_weight_internal = self.font_weight_stack.peek();
                }
                Some(end_index)
            }
            73 | 105 => {
                self.font_style_internal |= ITALIC;
                self.font_style_stack.add(ITALIC);
                if self.xml[1].name_hash == 276531 || self.xml[1].name_hash == 186899 {
                    self.italic_angle = self.convert_to_float(self.xml[1].value_start, self.xml[1].value_len) as i32;
                    if self.italic_angle < -180 || self.italic_angle > 180 {
                        return None;
                    }
                } else {
                    self.italic_angle = self.cur_font.italic_style as i32;
                }
                self.italic_angle_stack.add(self.italic_angle);
                Some(end_index)
            }
            402 | 434 => {
                if self.p.font_style & ITALIC != ITALIC {
                    self.italic_angle = self.italic_angle_stack.remove();
                    if self.font_style_stack.remove(ITALIC) == 0 {
                        self.font_style_internal &= !ITALIC;
                    }
                }
                Some(end_index)
            }
            32745 | 45545 => {
                let num3 = self.convert_to_float(self.xml[0].value_start, self.xml[0].value_len);
                if num3 == -32768.0 {
                    return None;
                }
                match tag_unit {
                    0 => {
                        if self.html_tag[5] == b'+' as u16 || self.html_tag[5] == b'-' as u16 {
                            self.current_font_size = self.font_size + num3;
                        } else {
                            self.current_font_size = num3;
                        }
                    }
                    1 => self.current_font_size = self.font_size * num3,
                    2 => self.current_font_size = self.font_size * num3 / 100.0,
                    _ => return None,
                }
                self.size_stack.add(self.current_font_size);
                Some(end_index)
            }
            145592 | 158392 => {
                self.current_font_size = self.size_stack.remove();
                Some(end_index)
            }
            186285 | 275917 => {
                let j = match self.xml[0].value_hash {
                    3774683 => H_LEFT,
                    136703040 => H_RIGHT,
                    -458210101 => H_CENTER,
                    -523808257 => H_JUSTIFIED,
                    122383428 => H_FLUSH,
                    _ => return None,
                };
                self.line_justification = j;
                self.line_justification_stack.add(j);
                Some(end_index)
            }
            976214 | 1065846 => {
                self.line_justification = self.line_justification_stack.remove();
                Some(end_index)
            }
            192323 | 281955 => {
                if self.html_tag[6] == b'#' as u16 && matches!(num, 10 | 11 | 13 | 15) {
                    self.html_color = self.hex_chars_to_color(num);
                    self.color_stack.add(self.html_color);
                    return Some(end_index);
                }
                let c = match self.xml[0].value_hash {
                    125395 => C32::new(255, 0, 0, 255),
                    -992792864 => C32::new(173, 216, 230, 255),
                    3573310 => C32::new(0, 0, 255, 255),
                    3680713 => C32::new(128, 128, 128, 255),
                    117905991 => C32::new(0, 0, 0, 255),
                    121463835 => C32::new(0, 255, 0, 255),
                    140357351 => C32::new(255, 255, 255, 255),
                    26556144 => C32::new(255, 128, 0, 255),
                    -36881330 => C32::new(160, 32, 240, 255),
                    554054276 => C32::from_color([1.0, 0.92156863, 0.015686275, 1.0]),
                    _ => return None,
                };
                self.html_color = c;
                self.color_stack.add(c);
                Some(end_index)
            }
            982252 | 1071884 => {
                self.html_color = self.color_stack.remove();
                Some(end_index)
            }
            1524585 | 2152041 => {
                let num3 = self.convert_to_float(self.xml[0].value_start, self.xml[0].value_len);
                if num3 == -32768.0 {
                    return None;
                }
                match tag_unit {
                    0 => self.mono_spacing = num3 * ortho,
                    1 => self.mono_spacing = num3 * ortho * self.current_font_size,
                    _ => return None,
                }
                Some(end_index)
            }
            7054088 | 7681544 => {
                self.mono_spacing = 0.0;
                Some(end_index)
            }
            85 | 117 => {
                self.font_style_internal |= UNDERLINE;
                self.font_style_stack.add(UNDERLINE);
                if matches!(self.xml[1].name_hash, 281955 | 192323) {
                    unimplemented!("mod-only: TMP <u color=...> (the shipped texts only have a bare <u>)");
                }
                self.underline_color = self.html_color;
                self.underline_color_stack.add(self.underline_color);
                Some(end_index)
            }
            414 | 446 => {
                if self.p.font_style & UNDERLINE != UNDERLINE {
                    self.underline_color = self.underline_color_stack.remove();
                    if self.font_style_stack.remove(UNDERLINE) == 0 {
                        self.font_style_internal &= !UNDERLINE;
                    }
                }
                self.underline_color = self.underline_color_stack.remove();
                Some(end_index)
            }
            186622 | 276254 => {
                if self.xml[0].value_len != 3 {
                    return None;
                }
                let a = (hex_to_int(self.html_tag[7]) * 16 + hex_to_int(self.html_tag[8])) as u8;
                self.html_color = C32::new(self.html_color.r, self.html_color.g, self.html_color.b, a);
                Some(end_index)
            }
            -842693512 | 1109349752 => {
                let num3 = self.convert_to_float(self.xml[0].value_start, self.xml[0].value_len);
                if num3 == -32768.0 {
                    return None;
                }
                let ortho = if self.p.is_orthographic { 1.0 } else { 0.1 };
                self.line_height = match tag_unit {
                    0 => num3 * ortho,
                    1 => num3 * ortho * self.current_font_size,
                    _ => {
                        let f = &self.cur_font.face;
                        let num4 = self.current_font_size / f.point_size as f32 * f.scale * ortho;
                        self.font0.face.line_height * num3 / 100.0 * num4
                    }
                };
                Some(end_index)
            }
            -445573839 | 1897350193 => {
                self.line_height = LINE_HEIGHT_UNSET;
                Some(end_index)
            }
            1619421 | 2246877 => {
                if self.sprite_tag() {
                    Some(end_index)
                } else {
                    None
                }
            }
            h => {
                let tag: String = self.html_tag.iter().take(num).map(|&c| char_of(c as i32)).collect();
                unimplemented!("mod-only: TMP tag <{tag}> (name hash {h}): no shipped text or script string has a tag the rewrite does not handle")
            }
        }
    }

    /// TMP_Text.HexCharsToColor(chars, startIndex, length): #RRGGBB or #RRGGBBAA at the attribute's value, else white.
    fn hex_chars_to_color_at(&self, start: usize, length: usize) -> C32 {
        let h = |i: usize| hex_to_int(self.html_tag[start + i]);
        let b = |i: usize| (h(i) * 16 + h(i + 1)) as u8;
        match length {
            7 => C32::new(b(1), b(3), b(5), 255),
            9 => C32::new(b(1), b(3), b(5), b(7)),
            _ => C32::WHITE,
        }
    }

    /// TMP_Text.ValidateHtmlTag's `<sprite>` case: true when the tag names a sprite (m_currentSpriteAsset, m_spriteIndex, m_spriteColor and
    /// m_tintSprite are set, its material is current and m_textElementType is Sprite).
    fn sprite_tag(&mut self) -> bool {
        let p = self.p;
        let s = &p.settings;
        let (vt, start, len) = (self.xml[0].value_type, self.xml[0].value_start, self.xml[0].value_len);
        self.sprite_index = -1;
        if vt == 0 || vt == 1 {
            if p.has_sprite_asset {
                unimplemented!("mod-only: a TextMeshPro with its own sprite asset (m_spriteAsset): no shipped component assigns one");
            }
            let Some(d) = &s.default_sprite else {
                unimplemented!("mod-only: TMP_Settings without a default sprite asset");
            };
            self.current_sprite = Some(d.clone());
        } else {
            // <sprite="name">: MaterialReferenceManager, then Resources.Load<TMP_SpriteAsset>(defaultSpriteAssetPath + name) (every sprite
            // asset of the Resources is in the settings; no script subscribes OnSpriteAssetRequest).
            let name: String = (0..len).map(|k| char_of(self.html_tag[start + k] as i32)).collect();
            let path = format!("{}{}", s.sprite_path, name).to_lowercase();
            match s.sprite_assets.iter().find(|(p, _)| *p == path) {
                Some((_, a)) => self.current_sprite = Some(a.clone()),
                None => return false,
            }
        }
        if vt == 1 {
            let n = self.convert_to_float(start, len) as i32;
            if n == -32768 {
                return false;
            }
            if n > self.current_sprite.as_ref().expect("sprite asset").characters.len() as i32 - 1 {
                return false;
            }
            self.sprite_index = n;
        }
        self.sprite_color = C32::WHITE;
        self.tint_sprite = false;
        for l in 0..self.xml.len() {
            let nh = self.xml[l].name_hash;
            if nh == 0 {
                break;
            }
            match nh {
                30547 | 43347 => {
                    // name=
                    let cur = self.current_sprite.clone().expect("sprite asset");
                    match cur.search_by_hash(self.xml[l].value_hash, s.default_sprite.as_ref(), s.missing_sprite_unicode) {
                        Some((a, i)) => {
                            self.current_sprite = Some(a);
                            self.sprite_index = i;
                        }
                        None => {
                            self.current_sprite = None;
                            return false;
                        }
                    }
                }
                205930 | 295562 => {
                    // index= (TMP reads attribute 1 whatever its position)
                    let n = self.convert_to_float(self.xml[1].value_start, self.xml[1].value_len) as i32;
                    if n == -32768 {
                        return false;
                    }
                    if n > self.current_sprite.as_ref().expect("sprite asset").characters.len() as i32 - 1 {
                        return false;
                    }
                    self.sprite_index = n;
                }
                33019 | 45819 => {
                    // tint=
                    self.tint_sprite = self.convert_to_float(self.xml[l].value_start, self.xml[l].value_len) != 0.0;
                }
                192323 | 281955 => {
                    // color=
                    self.sprite_color = self.hex_chars_to_color_at(self.xml[l].value_start, self.xml[l].value_len);
                }
                26705 | 39505 => {
                    // anim=start,end,framerate
                    let params = self.get_attribute_parameters(self.xml[l].value_start, self.xml[l].value_len);
                    if params.len() != 3 {
                        return false;
                    }
                    self.sprite_index = params[0] as i32;
                    if self.is_parsing_text {
                        // spriteAnimator.DoSpriteAnimation: one animation per character until StopAllAnimations.
                        let (character, asset) = (self.character_count, self.current_sprite.clone().expect("sprite asset"));
                        let anims = self.sprite_anims.get_or_insert_with(Vec::new);
                        if !anims.iter().any(|a| a.character == character) {
                            anims.push(super::SpriteAnim { character, asset, start: self.sprite_index, end: params[1] as i32, framerate: params[2] as i32, info: None });
                        }
                    }
                }
                _ => {
                    if nh != 2246877 && nh != 1619421 {
                        return false;
                    }
                }
            }
        }
        if self.sprite_index == -1 {
            return false;
        }
        let sa = self.current_sprite.clone().expect("sprite asset");
        self.cur_mat_idx = self.add_sprite_material_reference(&sa);
        self.text_element_type = 1;
        true
    }

    // ---- word wrapping state ----

    /// TMP_Text.SaveWordWrappingState (`old` is the state being overwritten: its lineInfo survives when the line number
    /// is past the line array).
    fn snapshot(&self, index: i32, count: i32, old: &WordWrapState) -> WordWrapState {
        WordWrapState {
            previous_word_break: index,
            total_character_count: count,
            visible_character_count: self.line_visible_character_count,
            first_character_index: self.first_character_of_line,
            first_visible_character_index: self.first_visible_character_of_line,
            last_visible_char_index: self.last_visible_character_of_line,
            line_number: self.line_number,
            max_cap_height: self.max_cap_height,
            max_ascender: self.max_text_ascender,
            max_descender: self.element_descender,
            start_of_line_ascender: self.start_of_line_ascender,
            max_line_ascender: self.max_line_ascender,
            max_line_descender: self.max_line_descender,
            page_ascender: self.page_ascender,
            horizontal_alignment: self.line_justification,
            margin_left: self.margin_left,
            margin_right: self.margin_right,
            x_advance: self.x_advance,
            preferred_width: self.preferred_width,
            preferred_height: self.preferred_height,
            font_style: self.font_style_internal,
            italic_angle: self.italic_angle,
            font_scale_multiplier: self.font_scale_multiplier,
            current_font_size: self.current_font_size,
            baseline_offset: self.baseline_offset,
            line_offset: self.line_offset,
            is_driven_line_spacing: self.is_driven_line_spacing,
            glyph_horizontal_advance_adjustment: self.glyph_horizontal_advance_adjustment,
            c_space: self.c_spacing,
            m_space: self.mono_spacing,
            line_info: if (self.line_number as usize) < self.line_info.len() { self.line_info[self.line_number as usize] } else { old.line_info },
            vertex_color: self.html_color,
            underline_color: self.underline_color,
            strikethrough_color: self.strikethrough_color,
            basic_style_stack: self.font_style_stack,
            italic_angle_stack: self.italic_angle_stack.clone(),
            color_stack: self.color_stack.clone(),
            underline_color_stack: self.underline_color_stack.clone(),
            size_stack: self.size_stack.clone(),
            font_weight_stack: self.font_weight_stack.clone(),
            line_justification_stack: self.line_justification_stack.clone(),
            current_font: self.cur_font.clone(),
            current_material: self.cur_material,
            current_mat_idx: self.cur_mat_idx,
            mesh_ext_min: self.mesh_ext_min,
            mesh_ext_max: self.mesh_ext_max,
            tag_no_parsing: self.tag_no_parsing,
            is_non_breaking_space: self.is_non_breaking_space,
        }
    }

    fn save_word_wrap(&mut self, i: i32, c: i32) {
        self.saved_word_wrap = self.snapshot(i, c, &self.saved_word_wrap);
    }
    fn save_line(&mut self, i: i32, c: i32) {
        self.saved_line = self.snapshot(i, c, &self.saved_line);
    }
    fn save_ellipsis(&mut self, i: i32, c: i32) {
        self.saved_ellipsis = self.snapshot(i, c, &self.saved_ellipsis);
    }
    fn save_last_valid(&mut self, i: i32, c: i32) {
        self.saved_last_valid = self.snapshot(i, c, &self.saved_last_valid);
    }
    fn save_soft_line_break(&mut self, i: i32, c: i32) {
        self.saved_soft_line_break = self.snapshot(i, c, &self.saved_soft_line_break);
    }

    /// TMP_Text.RestoreWordWrappingState.
    fn restore(&mut self, s: &WordWrapState) -> i32 {
        self.cur_font = s.current_font.clone();
        self.cur_material = s.current_material;
        self.cur_mat_idx = s.current_mat_idx;
        self.character_count = s.total_character_count + 1;
        self.line_visible_character_count = s.visible_character_count;
        self.first_character_of_line = s.first_character_index;
        self.first_visible_character_of_line = s.first_visible_character_index;
        self.last_visible_character_of_line = s.last_visible_char_index;
        self.font_style_internal = s.font_style;
        self.italic_angle = s.italic_angle;
        self.font_scale_multiplier = s.font_scale_multiplier;
        self.current_font_size = s.current_font_size;
        self.x_advance = s.x_advance;
        self.max_cap_height = s.max_cap_height;
        self.max_text_ascender = s.max_ascender;
        self.element_descender = s.max_descender;
        self.start_of_line_ascender = s.start_of_line_ascender;
        self.max_line_ascender = s.max_line_ascender;
        self.max_line_descender = s.max_line_descender;
        self.page_ascender = s.page_ascender;
        self.preferred_width = s.preferred_width;
        self.preferred_height = s.preferred_height;
        self.mesh_ext_min = s.mesh_ext_min;
        self.mesh_ext_max = s.mesh_ext_max;
        self.line_number = s.line_number;
        self.line_offset = s.line_offset;
        self.baseline_offset = s.baseline_offset;
        self.is_driven_line_spacing = s.is_driven_line_spacing;
        self.glyph_horizontal_advance_adjustment = s.glyph_horizontal_advance_adjustment;
        self.c_spacing = s.c_space;
        self.mono_spacing = s.m_space;
        self.line_justification = s.horizontal_alignment;
        self.margin_left = s.margin_left;
        self.margin_right = s.margin_right;
        self.html_color = s.vertex_color;
        self.underline_color = s.underline_color;
        self.strikethrough_color = s.strikethrough_color;
        self.is_non_breaking_space = s.is_non_breaking_space;
        self.tag_no_parsing = s.tag_no_parsing;
        self.font_style_stack = s.basic_style_stack;
        self.italic_angle_stack = s.italic_angle_stack.clone();
        self.color_stack = s.color_stack.clone();
        self.underline_color_stack = s.underline_color_stack.clone();
        self.size_stack = s.size_stack.clone();
        self.font_weight_stack = s.font_weight_stack.clone();
        self.line_justification_stack = s.line_justification_stack.clone();
        if (self.line_number as usize) < self.line_info.len() {
            self.line_info[self.line_number as usize] = s.line_info;
        }
        s.previous_word_break
    }

    /// TMP_Text.AdjustLineOffset.
    fn adjust_line_offset(&mut self, start: i32, end: i32, offset: f32) {
        for i in start..=end {
            let c = &mut self.chars[i as usize];
            for v in [&mut c.bl, &mut c.tl, &mut c.tr, &mut c.br] {
                v[1] -= offset;
            }
            c.ascender -= offset;
            c.baseline -= offset;
            c.descender -= offset;
            if c.is_visible {
                for v in &mut c.v {
                    v.pos[1] -= offset;
                }
            }
        }
    }

    /// TMP_Text.ResizeLineExtents.
    fn resize_line_extents(&mut self, size: usize) {
        let size = if size > 1024 { size + 256 } else { next_pow2(size + 1) };
        while self.line_info.len() < size {
            self.line_info.push(LineInfo::resized());
        }
    }

    /// TMP_Text.InsertNewLine.
    #[allow(clippy::too_many_arguments)]
    fn insert_new_line(&mut self, i: i32, base_scale: f32, current_element_scale: f32, current_em_scale: f32, glyph_adjustment: f32, bold_spacing_adjustment: f32, character_spacing_adjustment: f32, width: f32, line_gap: f32, is_max_visible_descender_set: &mut bool, max_visible_descender: &mut f32) {
        let num = self.max_line_ascender - self.start_of_line_ascender;
        if self.line_offset > 0.0 && num.abs() > 0.01 && !self.is_driven_line_spacing {
            self.adjust_line_offset(self.first_character_of_line, self.character_count, num);
            self.element_descender -= num;
            self.line_offset += num;
        }
        let num2 = self.max_line_ascender - self.line_offset;
        let num3 = self.max_line_descender - self.line_offset;
        self.element_descender = if self.element_descender < num3 { self.element_descender } else { num3 };
        if !*is_max_visible_descender_set {
            *max_visible_descender = self.element_descender;
        }
        // m_useMaxVisibleDescender with m_maxVisibleCharacters / m_maxVisibleLines at 99999 never triggers.
        let ln = self.line_number as usize;
        let cc = self.character_count;
        self.line_info[ln].first_character_index = self.first_character_of_line;
        self.first_visible_character_of_line = if self.first_character_of_line > self.first_visible_character_of_line { self.first_character_of_line } else { self.first_visible_character_of_line };
        self.line_info[ln].first_visible_character_index = self.first_visible_character_of_line;
        self.last_character_of_line = if cc - 1 > 0 { cc - 1 } else { 0 };
        let num4 = self.last_character_of_line;
        self.line_info[ln].last_character_index = num4;
        self.last_visible_character_of_line = if self.last_visible_character_of_line < self.first_visible_character_of_line { self.first_visible_character_of_line } else { self.last_visible_character_of_line };
        self.line_info[ln].last_visible_character_index = self.last_visible_character_of_line;
        self.line_info[ln].character_count = self.line_info[ln].last_character_index - self.line_info[ln].first_character_index + 1;
        self.line_info[ln].visible_character_count = self.line_visible_character_count;
        self.line_info[ln].ext_min = [self.chars[self.first_visible_character_of_line as usize].bl[0], num3];
        self.line_info[ln].ext_max = [self.chars[self.last_visible_character_of_line as usize].tr[0], num2];
        self.line_info[ln].length = self.line_info[ln].ext_max[0];
        self.line_info[ln].width = width;
        let num5 = (glyph_adjustment * current_element_scale + (self.cur_font.normal_spacing_offset + character_spacing_adjustment + bold_spacing_adjustment) * current_em_scale - self.c_spacing) * (1.0 - self.char_width_adj_delta);
        let x_advance = self.chars[self.last_visible_character_of_line as usize].x_advance + (if self.p.is_right_to_left { num5 } else { 0.0 - num5 });
        self.line_info[ln].max_advance = x_advance;
        self.chars[num4 as usize].x_advance = x_advance;
        self.line_info[ln].baseline = 0.0 - self.line_offset;
        self.line_info[ln].ascender = num2;
        self.line_info[ln].descender = num3;
        self.line_info[ln].line_height = num2 - num3 + line_gap * base_scale;
        self.first_character_of_line = cc;
        self.line_visible_character_count = 0;
        self.save_line(i, cc - 1);
        self.line_number += 1;
        if self.line_number as usize >= self.line_info.len() {
            self.resize_line_extents(self.line_number as usize);
        }
        if self.line_height == LINE_HEIGHT_UNSET {
            let adjusted_ascender = self.chars[cc as usize].adjusted_ascender;
            let num6 = 0.0 - self.max_line_descender + adjusted_ascender + (line_gap + self.line_spacing_delta) * base_scale + self.p.line_spacing * current_em_scale;
            self.line_offset += num6;
            self.start_of_line_ascender = adjusted_ascender;
        } else {
            self.line_offset += self.line_height + self.p.line_spacing * current_em_scale;
        }
        self.max_line_ascender = LARGE_NEG;
        self.max_line_descender = LARGE_POS;
        self.x_advance = 0.0 + 0.0;
    }

    /// The auto-size font size reduction (`m_maxFontSize = m_fontSize; ...`).
    fn shrink_font_size(&mut self) {
        self.max_font_size = self.font_size;
        let n = fmax((self.font_size - self.min_font_size) / 2.0, 0.05);
        self.font_size -= n;
        self.font_size = fmax((((self.font_size * 20.0 + 0.5) as i32) as f32) / 20.0, self.p.font_size_min);
    }

    /// The auto-size character width adjustment (`m_charWidthAdjDelta += ...`).
    fn widen_chars(&mut self, num35: f32, num13: f32, flag11: bool) {
        let mut n = num35;
        if self.char_width_adj_delta > 0.0 {
            n /= 1.0 - self.char_width_adj_delta;
        }
        let m = num35 - (num13 - 0.0001) * (if flag11 { 1.05 } else { 1.0 });
        self.char_width_adj_delta += m / n;
        self.char_width_adj_delta = fmin(self.char_width_adj_delta, self.p.char_width_max_adj / 100.0);
    }

    /// The Ellipsis overflow case: restart from the beginning, or restore the last ellipsis insertion candidate.
    fn ellipsis_overflow(&mut self, i: &mut i32, subst: &mut (i32, u32), num15: &mut i32) {
        if self.ellipsis_stack.count == 0 {
            *i = -1;
            self.character_count = 0;
            *subst = (0, 3);
            self.first_character_of_line = 0;
        } else {
            let blank = WordWrapState::blank(&self.font0);
            let state = self.ellipsis_stack.pop(&blank);
            *i = self.restore(&state);
            *i -= 1;
            self.character_count -= 1;
            *subst = (self.character_count, 8230);
            *num15 += 1;
        }
    }

    /// TMP_Text.GetUnderlineSpecialCharacter / SaveGlyphVertexInfo / FillCharacterVertexBuffers helpers are below;
    /// this is TextMeshPro.GenerateTextMesh.
    fn generate_text_mesh(&mut self) {
        let p = self.p;
        if self.tp.is_empty() || self.tp[0] == 0 {
            self.quads.clear();
            self.underline_quads.clear();
            self.preferred_width = 0.0;
            self.preferred_height = 0.0;
            self.is_auto_size_point_size_set = true;
            return;
        }
        // m_spriteAnimator.StopAllAnimations()
        self.sprite_anims = Some(Vec::new());
        if p.enable_vertex_gradient && p.has_gradient_preset {
            unimplemented!("mod-only: TMP vertex gradient with a TMP_ColorGradient preset (the shipped gradients are the component's own four colours)");
        }
        if p.is_right_to_left {
            unimplemented!("mod-only: TMP right-to-left text (every shipped TMP component has m_isRightToLeft 0)");
        }
        // ScrollRect lays out like Overflow in TextMeshPro (its only case label shares InsertNewLine with Overflow/Masking).
        if matches!(self.overflow_mode, 5 | 6) {
            unimplemented!("mod-only: TMP Page / Linked overflow (shipped texts use overflow modes 0-4)");
        }
        self.cur_font = self.font0.clone();
        self.cur_material = self.mats[0].arena;
        self.cur_mat_idx = 0;
        let total_character_count = self.total_character_count;
        let ortho = if p.is_orthographic { 1.0f32 } else { 0.1f32 };
        let face0_point = self.font0.face.point_size as f32;
        let face0_scale = self.font0.face.scale;
        let num = self.font_size / face0_point * face0_scale * ortho;
        let mut num2: f32;
        let num3 = self.font_size * 0.01 * ortho;
        self.font_scale_multiplier = 1.0;
        self.current_font_size = self.font_size;
        self.size_stack.set_default(self.current_font_size);
        let mut num5: i32 = 0;
        self.font_style_internal = p.font_style;
        self.font_weight_internal = if self.font_style_internal & BOLD == BOLD { WEIGHT_BOLD } else { p.font_weight };
        self.font_weight_stack.set_default(self.font_weight_internal);
        self.font_style_stack = FontStyleStack::default();
        self.line_justification = p.horizontal_alignment;
        self.line_justification_stack.set_default(self.line_justification);
        let mut num7: f32;
        let mut num8: f32;
        self.baseline_offset = 0.0;
        let mut underline_active = false; // flag
        let mut underline_start = [0.0f32; 3]; // start
        let mut underline_zero = [0.0f32; 3]; // zero
        let _ = (&mut underline_active, &mut underline_start, &mut underline_zero);
        self.font_color32 = C32::from_color(p.font_color);
        self.html_color = self.font_color32;
        self.underline_color = self.html_color;
        self.strikethrough_color = self.html_color;
        self.color_stack.set_default(self.html_color);
        self.underline_color_stack.set_default(self.underline_color);
        self.line_height = LINE_HEIGHT_UNSET;
        self.italic_angle = self.font0.italic_style as i32;
        self.italic_angle_stack.set_default(self.italic_angle);
        self.line_offset = 0.0;
        let num9 = self.font0.face.line_height - (self.font0.face.ascent_line - self.font0.face.descent_line);
        self.c_spacing = 0.0;
        self.mono_spacing = 0.0;
        self.x_advance = 0.0;
        self.tag_no_parsing = false;
        self.character_count = 0;
        self.first_character_of_line = 0;
        self.last_character_of_line = 0;
        self.first_visible_character_of_line = 0;
        self.last_visible_character_of_line = 0;
        self.max_line_ascender = LARGE_NEG;
        self.max_line_descender = LARGE_POS;
        self.line_number = 0;
        self.start_of_line_ascender = 0.0;
        self.line_visible_character_count = 0;
        let mut flag4 = true;
        self.is_driven_line_spacing = false;
        self.first_overflow_character_index = -1;
        let margin = p.margin;
        let num11 = if self.margin_width > 0.0 { self.margin_width } else { 0.0 };
        let num12 = if self.margin_height > 0.0 { self.margin_height } else { 0.0 };
        self.margin_left = 0.0;
        self.margin_right = 0.0;
        self.width = -1.0;
        let mut num13 = num11 + 0.0001 - self.margin_left - self.margin_right;
        self.mesh_ext_min = [LARGE_POS_V2, LARGE_POS_V2];
        self.mesh_ext_max = [LARGE_NEG_V2, LARGE_NEG_V2];
        for l in &mut self.line_info {
            l.clear();
        }
        self.max_cap_height = 0.0;
        self.max_text_ascender = 0.0;
        self.element_descender = 0.0;
        self.page_ascender = 0.0;
        let mut max_visible_descender = 0.0f32;
        let mut is_max_visible_descender_set = false;
        self.is_non_breaking_space = false;
        let mut flag5 = true;
        let flag6 = false;
        let mut num14: i32 = 0;
        let mut subst: (i32, u32) = (-1, 0);
        let mut flag7 = false;
        self.save_word_wrap(-1, -1);
        self.save_line(-1, -1);
        self.save_ellipsis(-1, -1);
        self.save_last_valid(-1, -1);
        self.save_soft_line_break(-1, -1);
        self.ellipsis_stack.clear();
        let mut num15 = 0i32;
        let mut i: i32 = -1;
        loop {
            i += 1;
            if !((i as usize) < self.tp.len() && self.tp[i as usize] != 0) {
                break;
            }
            num5 = self.tp[i as usize];
            if num15 > 5 {
                subst = (self.character_count, 3);
            }
            if p.is_rich_text && num5 == 60 {
                self.is_parsing_text = true;
                self.text_element_type = 0;
                if let Some(end) = self.validate_html_tag(i as usize + 1) {
                    i = end as i32;
                    if self.text_element_type == 0 {
                        continue;
                    }
                }
            } else {
                let c = &self.chars[self.character_count as usize];
                self.text_element_type = c.sprite.is_some() as u8;
                self.cur_mat_idx = c.mat_idx;
                self.cur_font = c.font.clone();
            }
            self.is_parsing_text = false;
            let mut flag8 = false;
            let cc = self.character_count as usize;
            if subst.0 == self.character_count {
                num5 = subst.1 as i32;
                self.text_element_type = 0;
                flag8 = true;
                match num5 {
                    3 => {
                        let f = self.cur_font.clone();
                        // characterLookupTable[3]: a font without U+0003 throws.
                        let ch = *f.characters.get(&3).unwrap_or_else(|| crate::game::menu::throw("KeyNotFoundException: The given key was not present in the dictionary."));
                        self.chars[cc].elem = Some(Elem { font: f.clone(), ch, glyph: f.glyph(&ch), sprite: None, alt: false });
                        self.is_text_truncated = true;
                    }
                    8230 => {
                        let e = self.ellipsis.clone().expect("ellipsis character");
                        self.chars[cc].elem = Some(e.elem.clone());
                        self.chars[cc].font = e.elem.font.clone();
                        self.chars[cc].material = e.material;
                        self.chars[cc].mat_idx = e.mat_idx;
                        self.is_text_truncated = true;
                        subst = (self.character_count + 1, 3);
                    }
                    _ => {}
                }
            }
            let mut num16 = 1.0f32;
            let is_sprite = self.text_element_type == 1;
            if !is_sprite {
                if self.font_style_internal & UPPER_CASE == UPPER_CASE {
                    if char_is_lower(num5) {
                        num5 = char_to_upper(num5);
                    }
                } else if self.font_style_internal & LOWER_CASE == LOWER_CASE {
                    if char_is_upper(num5) {
                        num5 = char_to_lower(num5);
                    }
                } else if self.font_style_internal & SMALL_CAPS == SMALL_CAPS && char_is_lower(num5) {
                    num16 = 0.8;
                    num5 = char_to_upper(num5);
                }
            }
            let mut elem = self.chars[cc].elem.clone().expect("text element");
            self.cur_font = self.chars[cc].font.clone();
            if !is_sprite {
                // A sprite leaves m_currentMaterial as it was (the previous character's).
                self.cur_material = self.chars[cc].material;
            }
            self.cur_mat_idx = self.chars[cc].mat_idx;
            let cur_font = self.cur_font.clone();
            let face = &cur_font.face;
            let (num17, num18, num19, mut num6);
            if is_sprite {
                let (sa, sidx) = self.chars[cc].sprite.clone().expect("sprite character");
                let sc = sprite_character(&sa, sidx);
                if num5 == 60 {
                    num5 = 57344 + sidx;
                } else {
                    self.sprite_color = C32::WHITE;
                }
                let fs = self.current_font_size;
                let num20 = fs / face.point_size as f32 * face.scale * ortho;
                if sa.face.point_size > 0 {
                    let num21 = fs / sa.face.point_size as f32 * sa.face.scale * ortho;
                    num2 = sc.scale * sc.glyph.scale * num21;
                    num18 = sa.face.ascent_line;
                    num17 = sa.face.baseline * num20 * self.font_scale_multiplier * sa.face.scale;
                    num19 = sa.face.descent_line;
                } else {
                    let num22 = fs / face.point_size as f32 * face.scale * ortho;
                    num2 = face.ascent_line / sc.glyph.metrics.height * sc.scale * sc.glyph.scale * num22;
                    let num23 = num22 / num2;
                    num18 = face.ascent_line * num23;
                    num17 = face.baseline * num20 * self.font_scale_multiplier * face.scale;
                    num19 = face.descent_line * num23;
                }
                elem = sprite_elem(&cur_font, &sa, &sc);
                self.current_sprite = Some(sa);
                self.sprite_index = sidx;
                num6 = 0.0;
            } else {
                let num24 = if !flag8 || self.tp[i as usize] != 10 || self.character_count == self.first_character_of_line {
                    self.current_font_size * num16 / face.point_size as f32 * face.scale * ortho
                } else {
                    self.chars[cc - 1].point_size * num16 / face.point_size as f32 * face.scale * ortho
                };
                (num18, num19) = if flag8 && num5 == 8230 { (0.0f32, 0.0f32) } else { (face.ascent_line, face.descent_line) };
                num2 = num24 * self.font_scale_multiplier * elem.ch.scale * elem.glyph.scale;
                num17 = face.baseline * num24 * self.font_scale_multiplier * face.scale;
                self.chars[cc].sprite = None;
                num6 = if self.cur_mat_idx == 0 { self.padding0() } else { self.arena[self.mats[self.cur_mat_idx].arena].padding };
            }
            self.chars[cc].scale = num2;
            let num25 = num2;
            if num5 == 173 || num5 == 3 {
                num2 = 0.0;
            }
            {
                let (h, u, s, st, fs) = (self.html_color, self.underline_color, self.strikethrough_color, self.font_style_internal, self.current_font_size);
                let c = &mut self.chars[cc];
                c.character = num5 as u16;
                c.point_size = fs;
                c.color = h;
                c.underline_color = u;
                c.strikethrough_color = s;
                c.style = st;
            }
            let metrics = elem.glyph.metrics;
            let flag9 = num5 <= 65535 && is_white_space(num5);
            let mut gvr = GlyphValueRecord::default();
            let mut num26 = p.character_spacing;
            self.glyph_horizontal_advance_adjustment = 0.0;
            if p.enable_kerning {
                let glyph_index = elem.ch.glyph_index;
                if self.character_count < total_character_count - 1 {
                    let next = self.chars[cc + 1].elem.as_ref().expect("next element").ch.glyph_index;
                    let key = (next << 16) | glyph_index;
                    if let Some(v) = cur_font.pair(key) {
                        gvr = v.first;
                        num26 = if v.flags & 0x100 == 0x100 { 0.0 } else { num26 };
                    }
                }
                if self.character_count >= 1 {
                    let prev = self.chars[cc - 1].elem.as_ref().expect("previous element").ch.glyph_index;
                    let key2 = (glyph_index << 16) | prev;
                    if let Some(v) = cur_font.pair(key2) {
                        gvr = gvr + v.second;
                        num26 = if v.flags & 0x100 == 0x100 { 0.0 } else { num26 };
                    }
                }
                self.glyph_horizontal_advance_adjustment = gvr.x_advance;
            }
            let mut num27 = 0.0f32;
            if self.mono_spacing != 0.0 {
                num27 = (self.mono_spacing / 2.0 - (metrics.width / 2.0 + metrics.horizontal_bearing_x) * num2) * (1.0 - self.char_width_adj_delta);
                self.x_advance += num27;
            }
            let cur_mat = &self.arena[self.cur_material].mat;
            if !is_sprite && !self.chars[self.character_count as usize].alt && self.font_style_internal & BOLD == BOLD {
                if cur_mat.has("_GradientScale") {
                    let num28 = cur_mat.float("_GradientScale");
                    num7 = cur_font.bold_style / 4.0 * num28 * cur_mat.float("_ScaleRatioA");
                    if num7 + num6 > num28 {
                        num6 = num28 - num7;
                    }
                } else {
                    num7 = 0.0;
                }
                num8 = cur_font.bold_spacing;
            } else {
                if cur_mat.has("_GradientScale") && cur_mat.has("_ScaleRatioA") {
                    let num29 = cur_mat.float("_GradientScale");
                    num7 = cur_font.normal_style / 4.0 * num29 * cur_mat.float("_ScaleRatioA");
                    if num7 + num6 > num29 {
                        num6 = num29 - num7;
                    }
                } else {
                    num7 = 0.0;
                }
                num8 = 0.0;
            }
            let cwd = 1.0 - self.char_width_adj_delta;
            let mut v2 = [0.0f32; 3]; // top left
            let mut v3 = [0.0f32; 3]; // bottom left
            let mut v4 = [0.0f32; 3]; // top right
            let mut v5 = [0.0f32; 3]; // bottom right
            v2[0] = self.x_advance + (metrics.horizontal_bearing_x - num6 - num7 + gvr.x_placement) * num2 * cwd;
            v2[1] = num17 + (metrics.horizontal_bearing_y + num6 + gvr.y_placement) * num2 - self.line_offset + self.baseline_offset;
            v3[0] = v2[0];
            v3[1] = v2[1] - (metrics.height + num6 * 2.0) * num2;
            v4[0] = v3[0] + (metrics.width + num6 * 2.0 + num7 * 2.0) * num2 * cwd;
            v4[1] = v2[1];
            v5[0] = v4[0];
            v5[1] = v3[1];
            if !is_sprite && !self.chars[self.character_count as usize].alt && self.font_style_internal & ITALIC == ITALIC {
                let num30 = self.italic_angle as f32 * 0.01;
                let v6 = num30 * ((metrics.horizontal_bearing_y + num6 + num7) * num2);
                let v7 = num30 * ((metrics.horizontal_bearing_y - metrics.height - num6 - num7) * num2);
                let v8 = (v6 - v7) / 2.0;
                v2[0] = v2[0] + v6 - v8;
                v3[0] = v3[0] + v7 - v8;
                v4[0] = v4[0] + v6 - v8;
                v5[0] = v5[0] + v7 - v8;
            }
            {
                let (xa, lo, bo) = (self.x_advance, self.line_offset, self.baseline_offset);
                let c = &mut self.chars[cc];
                c.bl = v3;
                c.tl = v2;
                c.tr = v4;
                c.br = v5;
                c.origin = xa;
                c.baseline = num17 - lo + bo;
                c.aspect_ratio = (v4[0] - v3[0]) / (v2[1] - v3[1]);
            }
            let (num31, num32) = if is_sprite {
                (num18 * num2 + self.baseline_offset, num19 * num2 + self.baseline_offset)
            } else {
                (num18 * num2 / num16 + self.baseline_offset, num19 * num2 / num16 + self.baseline_offset)
            };
            let mut num33 = num31;
            let mut num34 = num32;
            let flag10 = self.character_count == self.first_character_of_line;
            if flag10 || !flag9 {
                if self.baseline_offset != 0.0 {
                    num33 = fmax((num31 - self.baseline_offset) / self.font_scale_multiplier, num33);
                    num34 = fmin((num32 - self.baseline_offset) / self.font_scale_multiplier, num34);
                }
                self.max_line_ascender = fmax(num33, self.max_line_ascender);
                self.max_line_descender = fmin(num34, self.max_line_descender);
            }
            if flag10 || !flag9 {
                self.chars[cc].adjusted_ascender = num33;
                self.chars[cc].adjusted_descender = num34;
                let a = num31 - self.line_offset;
                self.chars[cc].ascender = a;
                self.element_ascender = a;
                let d = num32 - self.line_offset;
                self.chars[cc].descender = d;
                self.element_descender = d;
            } else {
                self.chars[cc].adjusted_ascender = self.max_line_ascender;
                self.chars[cc].adjusted_descender = self.max_line_descender;
                let a = self.max_line_ascender - self.line_offset;
                self.chars[cc].ascender = a;
                self.element_ascender = a;
                let d = self.max_line_descender - self.line_offset;
                self.chars[cc].descender = d;
                self.element_descender = d;
            }
            if self.line_number == 0 && (flag10 || !flag9) {
                self.max_text_ascender = self.max_line_ascender;
                self.max_cap_height = fmax(self.max_cap_height, cur_font.face.cap_line * num2 / num16);
            }
            if self.line_offset == 0.0 && (flag10 || !flag9) {
                self.page_ascender = if self.page_ascender > num31 { self.page_ascender } else { num31 };
            }
            self.chars[cc].is_visible = false;
            let flag11 = self.line_justification & H_FLUSH == H_FLUSH || self.line_justification & H_JUSTIFIED == H_JUSTIFIED;
            let mut margin_left = 0.0f32;
            let mut margin_right = 0.0f32;
            if num5 == 9 || (!flag9 && num5 != 8203 && num5 != 173 && num5 != 3) || (num5 == 173 && !flag7) || is_sprite {
                self.chars[cc].is_visible = true;
                margin_left = self.margin_left;
                margin_right = self.margin_right;
                if flag8 {
                    margin_left = self.line_info[self.line_number as usize].margin_left;
                    margin_right = self.line_info[self.line_number as usize].margin_right;
                }
                num13 = if self.width != -1.0 { fmin(num11 + 0.0001 - margin_left - margin_right, self.width) } else { num11 + 0.0001 - margin_left - margin_right };
                let num35 = self.x_advance.abs() + metrics.horizontal_advance * (1.0 - self.char_width_adj_delta) * (if num5 == 173 { num25 } else { num2 });
                let num36 = self.max_text_ascender - (self.max_line_descender - self.line_offset) + (if self.line_offset > 0.0 && !self.is_driven_line_spacing { self.max_line_ascender - self.start_of_line_ascender } else { 0.0 });
                let character_count = self.character_count;
                if num36 > num12 + 0.0001 {
                    if self.first_overflow_character_index == -1 {
                        self.first_overflow_character_index = self.character_count;
                    }
                    if p.enable_auto_sizing {
                        if self.line_spacing_delta > p.line_spacing_max && self.line_offset > 0.0 && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
                            let num37 = (num12 - num36) / self.line_number as f32;
                            self.line_spacing_delta = fmax(self.line_spacing_delta + num37 / num, p.line_spacing_max);
                            return;
                        }
                        if self.font_size > p.font_size_min && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
                            self.shrink_font_size();
                            return;
                        }
                    }
                    match self.overflow_mode {
                        TRUNCATE => {
                            let s = self.saved_last_valid.clone();
                            i = self.restore(&s);
                            subst = (character_count, 3);
                            continue;
                        }
                        ELLIPSIS => {
                            self.ellipsis_overflow(&mut i, &mut subst, &mut num15);
                            continue;
                        }
                        _ => {}
                    }
                }
                if num35 > num13 * (if flag11 { 1.05 } else { 1.0 }) {
                    if p.enable_word_wrapping && self.character_count != self.first_character_of_line {
                        let s = self.saved_word_wrap.clone();
                        i = self.restore(&s);
                        let cc = self.character_count as usize;
                        let num39 = if self.line_height == LINE_HEIGHT_UNSET {
                            let adjusted_ascender = self.chars[cc].adjusted_ascender;
                            (if self.line_offset > 0.0 && !self.is_driven_line_spacing { self.max_line_ascender - self.start_of_line_ascender } else { 0.0 }) - self.max_line_descender + adjusted_ascender + (num9 + self.line_spacing_delta) * num + p.line_spacing * num3
                        } else {
                            self.is_driven_line_spacing = true;
                            self.line_height + p.line_spacing * num3
                        };
                        let num40 = self.max_text_ascender + num39 + self.line_offset - self.chars[cc].adjusted_descender;
                        if self.chars[cc - 1].character == 0xad && !flag7 && (self.overflow_mode == OVERFLOW || num40 < num12 + 0.0001) {
                            subst = (self.character_count - 1, 45);
                            i -= 1;
                            self.character_count -= 1;
                            continue;
                        }
                        flag7 = false;
                        if self.chars[cc].character == 0xad {
                            flag7 = true;
                            continue;
                        }
                        if p.enable_auto_sizing && flag5 {
                            if self.char_width_adj_delta < p.char_width_max_adj / 100.0 && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
                                self.widen_chars(num35, num13, flag11);
                                return;
                            }
                            if self.font_size > p.font_size_min && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
                                self.shrink_font_size();
                                return;
                            }
                        }
                        let previous_word_break = self.saved_soft_line_break.previous_word_break;
                        if flag5 && previous_word_break != -1 && previous_word_break != num14 {
                            let s = self.saved_soft_line_break.clone();
                            i = self.restore(&s);
                            num14 = previous_word_break;
                            let cc = self.character_count as usize;
                            if self.chars[cc - 1].character == 0xad {
                                subst = (self.character_count - 1, 45);
                                i -= 1;
                                self.character_count -= 1;
                                continue;
                            }
                        }
                        if !(num40 > num12 + 0.0001) {
                            let (gh, n8, n26) = (self.glyph_horizontal_advance_adjustment, num8, num26);
                            self.insert_new_line(i, num, num2, num3, gh, n8, n26, num13, num9, &mut is_max_visible_descender_set, &mut max_visible_descender);
                            flag4 = true;
                            flag5 = true;
                            continue;
                        }
                        if self.first_overflow_character_index == -1 {
                            self.first_overflow_character_index = self.character_count;
                        }
                        if p.enable_auto_sizing {
                            if self.line_spacing_delta > p.line_spacing_max && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
                                let num44 = (num12 - num40) / (self.line_number + 1) as f32;
                                self.line_spacing_delta = fmax(self.line_spacing_delta + num44 / num, p.line_spacing_max);
                                return;
                            }
                            if self.char_width_adj_delta < p.char_width_max_adj / 100.0 && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
                                self.widen_chars(num35, num13, flag11);
                                return;
                            }
                            if self.font_size > p.font_size_min && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
                                self.shrink_font_size();
                                return;
                            }
                        }
                        match self.overflow_mode {
                            OVERFLOW | MASKING | SCROLL_RECT => {
                                let (gh, n8, n26) = (self.glyph_horizontal_advance_adjustment, num8, num26);
                                self.insert_new_line(i, num, num2, num3, gh, n8, n26, num13, num9, &mut is_max_visible_descender_set, &mut max_visible_descender);
                                flag4 = true;
                                flag5 = true;
                                continue;
                            }
                            TRUNCATE => {
                                let s = self.saved_last_valid.clone();
                                i = self.restore(&s);
                                subst = (character_count, 3);
                                continue;
                            }
                            ELLIPSIS => {
                                self.ellipsis_overflow(&mut i, &mut subst, &mut num15);
                                continue;
                            }
                            _ => unreachable!("overflow mode checked at the start of the pass"),
                        }
                    } else {
                        if p.enable_auto_sizing && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
                            if self.char_width_adj_delta < p.char_width_max_adj / 100.0 {
                                self.widen_chars(num35, num13, flag11);
                                return;
                            }
                            if self.font_size > p.font_size_min {
                                self.shrink_font_size();
                                return;
                            }
                        }
                        match self.overflow_mode {
                            TRUNCATE => {
                                let s = self.saved_word_wrap.clone();
                                i = self.restore(&s);
                                subst = (character_count, 3);
                                continue;
                            }
                            ELLIPSIS => {
                                self.ellipsis_overflow(&mut i, &mut subst, &mut num15);
                                continue;
                            }
                            _ => {}
                        }
                    }
                }
                let cc = self.character_count as usize;
                match num5 {
                    9 => {
                        self.chars[cc].is_visible = false;
                        self.last_visible_character_of_line = self.character_count;
                        self.line_info[self.line_number as usize].space_count += 1;
                    }
                    173 => {
                        self.chars[cc].is_visible = false;
                    }
                    _ => {
                        let vertex_color = if !p.override_html_colors { self.html_color } else { self.font_color32 };
                        if is_sprite {
                            self.save_sprite_vertex_info(vertex_color);
                        } else {
                            self.save_glyph_vertex_info(num6, num7, vertex_color);
                        }
                        if flag4 {
                            flag4 = false;
                            self.first_visible_character_of_line = self.character_count;
                        }
                        self.line_visible_character_count += 1;
                        self.last_visible_character_of_line = self.character_count;
                        let ln = self.line_number as usize;
                        self.line_info[ln].margin_left = margin_left;
                        self.line_info[ln].margin_right = margin_right;
                    }
                }
            } else if (num5 == 10 || num5 == 11 || num5 == 160 || num5 == 8199 || num5 == 8232 || num5 == 8233 || is_separator(num5)) && num5 != 173 && num5 != 8203 && num5 != 8288 {
                self.line_info[self.line_number as usize].space_count += 1;
                if num5 == 160 {
                    self.line_info[self.line_number as usize].control_character_count += 1;
                }
            } else if num5 == 160 {
                self.line_info[self.line_number as usize].control_character_count += 1;
            }
            let cc = self.character_count as usize;
            if self.overflow_mode == ELLIPSIS && (!flag8 || num5 == 45) {
                let e = self.ellipsis.clone().expect("ellipsis character");
                let ef = &e.elem.font.face;
                let mut num52 = self.current_font_size / ef.point_size as f32 * ef.scale * ortho * self.font_scale_multiplier * e.elem.ch.scale * e.elem.glyph.scale;
                let mut margin_left2 = self.margin_left;
                let mut margin_right2 = self.margin_right;
                if num5 == 10 && self.character_count != self.first_character_of_line {
                    num52 = self.chars[cc - 1].point_size / ef.point_size as f32 * ef.scale * ortho * self.font_scale_multiplier * e.elem.ch.scale * e.elem.glyph.scale;
                    margin_left2 = self.line_info[self.line_number as usize].margin_left;
                    margin_right2 = self.line_info[self.line_number as usize].margin_right;
                }
                let num53 = self.max_text_ascender - (self.max_line_descender - self.line_offset) + (if self.line_offset > 0.0 && !self.is_driven_line_spacing { self.max_line_ascender - self.start_of_line_ascender } else { 0.0 });
                let num54 = self.x_advance.abs() + e.elem.glyph.metrics.horizontal_advance * (1.0 - self.char_width_adj_delta) * num52;
                let num55 = if self.width != -1.0 { fmin(num11 + 0.0001 - margin_left2 - margin_right2, self.width) } else { num11 + 0.0001 - margin_left2 - margin_right2 };
                if num54 < num55 * (if flag11 { 1.05 } else { 1.0 }) && num53 < num12 + 0.0001 {
                    self.save_ellipsis(i, self.character_count);
                    let s = self.saved_ellipsis.clone();
                    self.ellipsis_stack.push(s);
                }
            }
            self.chars[cc].line_number = self.line_number;
            if (num5 != 10 && num5 != 11 && num5 != 13 && !flag8) || self.line_info[self.line_number as usize].character_count == 1 {
                self.line_info[self.line_number as usize].alignment = self.line_justification;
            }
            if num5 == 9 {
                let num56 = cur_font.face.tab_width * cur_font.tab_size as f32 * num2;
                let num57 = (self.x_advance / num56).ceil() * num56;
                self.x_advance = if num57 > self.x_advance { num57 } else { self.x_advance + num56 };
            } else if self.mono_spacing != 0.0 {
                self.x_advance += (self.mono_spacing - num27 + (cur_font.normal_spacing_offset + num26) * num3 + self.c_spacing) * (1.0 - self.char_width_adj_delta);
                if flag9 || num5 == 8203 {
                    self.x_advance += p.word_spacing * num3;
                }
            } else {
                self.x_advance += ((metrics.horizontal_advance * 1.0 + gvr.x_advance) * num2 + (cur_font.normal_spacing_offset + num26 + num8) * num3 + self.c_spacing) * (1.0 - self.char_width_adj_delta);
                if flag9 || num5 == 8203 {
                    self.x_advance += p.word_spacing * num3;
                }
            }
            self.chars[cc].x_advance = self.x_advance;
            if num5 == 13 {
                self.x_advance = 0.0 + 0.0;
            }
            if num5 == 10 || num5 == 11 || num5 == 3 || num5 == 8232 || num5 == 8233 || (num5 == 45 && flag8) || self.character_count == total_character_count - 1 {
                let num59 = self.max_line_ascender - self.start_of_line_ascender;
                if self.line_offset > 0.0 && num59.abs() > 0.01 && !self.is_driven_line_spacing {
                    self.adjust_line_offset(self.first_character_of_line, self.character_count, num59);
                    self.element_descender -= num59;
                    self.line_offset += num59;
                    if self.saved_ellipsis.line_number == self.line_number {
                        let blank = WordWrapState::blank(&self.font0);
                        self.saved_ellipsis = self.ellipsis_stack.pop(&blank);
                        self.saved_ellipsis.start_of_line_ascender += num59;
                        self.saved_ellipsis.line_offset += num59;
                        let s = self.saved_ellipsis.clone();
                        self.ellipsis_stack.push(s);
                    }
                }
                let num60 = self.max_line_ascender - self.line_offset;
                let num61 = self.max_line_descender - self.line_offset;
                self.element_descender = if self.element_descender < num61 { self.element_descender } else { num61 };
                if !is_max_visible_descender_set {
                    max_visible_descender = self.element_descender;
                }
                let ln = self.line_number as usize;
                self.line_info[ln].first_character_index = self.first_character_of_line;
                self.first_visible_character_of_line = if self.first_character_of_line > self.first_visible_character_of_line { self.first_character_of_line } else { self.first_visible_character_of_line };
                self.line_info[ln].first_visible_character_index = self.first_visible_character_of_line;
                self.last_character_of_line = self.character_count;
                self.line_info[ln].last_character_index = self.character_count;
                self.last_visible_character_of_line = if self.last_visible_character_of_line < self.first_visible_character_of_line { self.first_visible_character_of_line } else { self.last_visible_character_of_line };
                self.line_info[ln].last_visible_character_index = self.last_visible_character_of_line;
                self.line_info[ln].character_count = self.line_info[ln].last_character_index - self.line_info[ln].first_character_index + 1;
                self.line_info[ln].visible_character_count = self.line_visible_character_count;
                self.line_info[ln].ext_min = [self.chars[self.first_visible_character_of_line as usize].bl[0], num61];
                self.line_info[ln].ext_max = [self.chars[self.last_visible_character_of_line as usize].tr[0], num60];
                self.line_info[ln].length = self.line_info[ln].ext_max[0] - num6 * num2;
                self.line_info[ln].width = num13;
                if self.line_info[ln].character_count == 1 {
                    self.line_info[ln].alignment = self.line_justification;
                }
                let num62 = ((cur_font.normal_spacing_offset + num26 + num8) * num3 - self.c_spacing) * (1.0 - self.char_width_adj_delta);
                if self.chars[self.last_visible_character_of_line as usize].is_visible {
                    self.line_info[ln].max_advance = self.chars[self.last_visible_character_of_line as usize].x_advance + (0.0 - num62);
                } else {
                    self.line_info[ln].max_advance = self.chars[self.last_character_of_line as usize].x_advance + (0.0 - num62);
                }
                self.line_info[ln].baseline = 0.0 - self.line_offset;
                self.line_info[ln].ascender = num60;
                self.line_info[ln].descender = num61;
                self.line_info[ln].line_height = num60 - num61 + num9 * num;
                match num5 {
                    10 | 11 | 45 | 8232 | 8233 => {
                        self.save_line(i, self.character_count);
                        self.line_number += 1;
                        flag4 = true;
                        flag5 = true;
                        self.first_character_of_line = self.character_count + 1;
                        self.line_visible_character_count = 0;
                        if self.line_number as usize >= self.line_info.len() {
                            self.resize_line_extents(self.line_number as usize);
                        }
                        let adjusted_ascender2 = self.chars[cc].adjusted_ascender;
                        let para = if num5 == 10 || num5 == 8233 { p.paragraph_spacing } else { 0.0 };
                        if self.line_height == LINE_HEIGHT_UNSET {
                            let num63 = 0.0 - self.max_line_descender + adjusted_ascender2 + (num9 + self.line_spacing_delta) * num + (p.line_spacing + para) * num3;
                            self.line_offset += num63;
                            self.is_driven_line_spacing = false;
                        } else {
                            self.line_offset += self.line_height + (p.line_spacing + para) * num3;
                            self.is_driven_line_spacing = true;
                        }
                        self.max_line_ascender = LARGE_NEG;
                        self.max_line_descender = LARGE_POS;
                        self.start_of_line_ascender = adjusted_ascender2;
                        self.x_advance = 0.0 + 0.0 + 0.0;
                        self.save_word_wrap(i, self.character_count);
                        self.save_last_valid(i, self.character_count);
                        self.character_count += 1;
                        continue;
                    }
                    3 => {
                        i = self.tp.len() as i32;
                    }
                    _ => {}
                }
            }
            if self.chars[cc].is_visible {
                self.mesh_ext_min[0] = fmin(self.mesh_ext_min[0], self.chars[cc].bl[0]);
                self.mesh_ext_min[1] = fmin(self.mesh_ext_min[1], self.chars[cc].bl[1]);
                self.mesh_ext_max[0] = fmax(self.mesh_ext_max[0], self.chars[cc].tr[0]);
                self.mesh_ext_max[1] = fmax(self.mesh_ext_max[1], self.chars[cc].tr[1]);
            }
            if p.enable_word_wrapping || self.overflow_mode == TRUNCATE || self.overflow_mode == ELLIPSIS {
                if (flag9 || num5 == 8203 || num5 == 45 || num5 == 173) && (!self.is_non_breaking_space | flag6) && num5 != 160 && num5 != 8199 && num5 != 8209 && num5 != 8239 && num5 != 8288 {
                    self.save_word_wrap(i, self.character_count);
                    flag5 = false;
                    self.saved_soft_line_break.previous_word_break = -1;
                } else if !self.is_non_breaking_space
                    && ((((num5 > 4352 && num5 < 4607) || (num5 > 43360 && num5 < 43391) || (num5 > 44032 && num5 < 55295)) && !p.settings.modern_hangul)
                        || (num5 > 11904 && num5 < 40959)
                        || (num5 > 63744 && num5 < 64255)
                        || (num5 > 65072 && num5 < 65103)
                        || (num5 > 65280 && num5 < 65519))
                {
                    unimplemented!("mod-only: TMP CJK / Hangul line breaking rules (U+{num5:04X}): no shipped text or script has such a character");
                } else if flag5 {
                    if flag9 || (num5 == 173 && !flag7) {
                        self.save_soft_line_break(i, self.character_count);
                    }
                    self.save_word_wrap(i, self.character_count);
                }
            }
            self.save_last_valid(i, self.character_count);
            self.character_count += 1;
        }
        let num4 = self.max_font_size - self.min_font_size;
        if p.enable_auto_sizing && num4 > 0.051 && self.font_size < p.font_size_max && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
            if self.char_width_adj_delta < p.char_width_max_adj / 100.0 {
                self.char_width_adj_delta = 0.0;
            }
            self.min_font_size = self.font_size;
            let num65 = fmax((self.max_font_size - self.font_size) / 2.0, 0.05);
            self.font_size += num65;
            self.font_size = fmin((((self.font_size * 20.0 + 0.5) as i32) as f32) / 20.0, p.font_size_max);
            return;
        }
        self.is_auto_size_point_size_set = true;
        if self.character_count == 0 || (self.character_count == 1 && num5 == 3) {
            self.quads.clear();
            self.underline_quads.clear();
            return;
        }
        // ---- vertical alignment ----
        self.quads = vec![Vec::new(); self.mats.len()];
        self.underline_quads.clear();
        let corners = self.corners;
        let add = |a: V3, b: V3| [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
        let half = |a: V3| [a[0] / 2.0, a[1] / 2.0, a[2] / 2.0];
        let mid = add(corners[0], corners[1]);
        let vector10: V3 = match p.vertical_alignment {
            V_TOP => add(corners[1], [0.0 + margin[0], 0.0 - self.max_text_ascender - margin[1], 0.0]),
            V_MIDDLE => add(half(mid), [0.0 + margin[0], 0.0 - (self.max_text_ascender + margin[1] + max_visible_descender - margin[3]) / 2.0, 0.0]),
            V_BOTTOM => add(corners[0], [0.0 + margin[0], 0.0 - max_visible_descender + margin[3], 0.0]),
            V_BASELINE => add(half(mid), [0.0 + margin[0], 0.0, 0.0]),
            V_GEOMETRY => add(half(mid), [0.0 + margin[0], 0.0 - (self.mesh_ext_max[1] + margin[1] + self.mesh_ext_min[1] - margin[3]) / 2.0, 0.0]),
            V_CAPLINE => add(half(mid), [0.0 + margin[0], 0.0 - (self.max_cap_height - margin[1] - margin[3]) / 2.0, 0.0]),
            _ => [0.0; 3],
        };
        let mut vector11: V3 = [0.0; 3];
        let mut num67: i32 = 0;
        let mut flag13 = false;
        let f = p.lossy_scale_y;
        let mut underline_color = C32::WHITE; // `color`
        let mut strike_color = C32::WHITE; // `underlineColor`
        let mut u_active = false; // flag
        let mut s_active = false; // flag2
        let mut u_start: V3 = [0.0; 3];
        let mut s_start: V3 = [0.0; 3];
        let mut num70 = 0.0f32;
        let mut num71 = 0.0f32;
        let mut num72 = 0.0f32;
        let mut num74 = 0.0f32;
        let mut num75 = LARGE_POS;
        let mut num77 = 0.0f32;
        let mut num78 = 0.0f32;
        let mut b = 0.0f32;
        let underline_index_start = self.mats[0].ref_count * 4;
        let _ = underline_index_start;
        for j in 0..self.character_count {
            let ju = j as usize;
            let font_asset = self.chars[ju].font.clone();
            let character = self.chars[ju].character as i32;
            let line_number = self.chars[ju].line_number;
            let li = self.line_info[line_number as usize];
            let alignment = li.alignment;
            match alignment {
                H_LEFT => vector11 = [0.0 + li.margin_left, 0.0, 0.0],
                H_CENTER => vector11 = [li.margin_left + li.width / 2.0 - li.max_advance / 2.0, 0.0, 0.0],
                H_GEOMETRY => vector11 = [li.margin_left + li.width / 2.0 - (li.ext_min[0] + li.ext_max[0]) / 2.0, 0.0, 0.0],
                H_RIGHT => vector11 = [li.margin_left + li.width - li.max_advance, 0.0, 0.0],
                H_JUSTIFIED | H_FLUSH => {
                    if !(character == 10 || character == 0xad || character == 0x200b || character == 0x2060 || character == 3) {
                        let character2 = self.chars[li.last_character_index as usize].character as i32;
                        let flag15 = alignment & H_FLUSH == H_FLUSH;
                        if ((!is_control(character2) && line_number < self.line_number) | flag15) || li.max_advance > li.width {
                            if line_number != num67 || j == 0 {
                                vector11 = [li.margin_left, 0.0, 0.0];
                                flag13 = is_separator(character);
                            } else {
                                let num79 = li.width - li.max_advance;
                                let mut num80 = li.visible_character_count - 1 + li.control_character_count;
                                let mut num81 = (if self.chars[li.last_character_index as usize].is_visible { li.space_count } else { li.space_count - 1 }) - li.control_character_count;
                                if flag13 {
                                    num81 -= 1;
                                    num80 += 1;
                                }
                                let num82 = if num81 > 0 { p.word_wrapping_ratios } else { 1.0 };
                                if num81 < 1 {
                                    num81 = 1;
                                }
                                if character != 0xa0 && (character == 9 || is_separator(character)) {
                                    vector11[0] += num79 * (1.0 - num82) / num81 as f32;
                                } else {
                                    vector11[0] += num79 * num82 / num80 as f32;
                                }
                            }
                        } else {
                            vector11 = [li.margin_left, 0.0, 0.0];
                        }
                    }
                }
                _ => {}
            }
            let zero3 = add(vector10, vector11);
            if self.chars[ju].is_visible {
                // TextureMappingOptions.Character in both directions (uv2 = 0..1 across the character); a sprite's uv2 is never written.
                if self.chars[ju].sprite.is_none() {
                let c = &mut self.chars[ju];
                c.v[0].uv2[0] = 0.0;
                c.v[1].uv2[0] = 0.0;
                c.v[2].uv2[0] = 1.0;
                c.v[3].uv2[0] = 1.0;
                c.v[0].uv2[1] = 0.0;
                c.v[1].uv2[1] = 1.0;
                c.v[2].uv2[1] = 1.0;
                c.v[3].uv2[1] = 0.0;
                num70 = c.scale * f.abs() * (1.0 - self.char_width_adj_delta);
                if !c.alt && c.style & BOLD == BOLD {
                    num70 *= -1.0;
                }
                let mut x = c.v[0].uv2[0];
                let mut y = c.v[0].uv2[1];
                let mut x2 = c.v[2].uv2[0];
                let mut y2 = c.v[2].uv2[1];
                let num86 = (x as i32) as f32;
                let num87 = (y as i32) as f32;
                x -= num86;
                x2 -= num86;
                y -= num87;
                y2 -= num87;
                let pack = |x: f32, y: f32| -> f32 {
                    let a = (x * 511.0) as i32 as f64;
                    let b = (y * 511.0) as i32 as f64;
                    (a * 4096.0 + b) as f32
                };
                c.v[0].uv2 = [pack(x, y), num70];
                c.v[1].uv2 = [pack(x, y2), num70];
                c.v[2].uv2 = [pack(x2, y2), num70];
                c.v[3].uv2 = [pack(x2, y), num70];
                }
                let c = &mut self.chars[ju];
                // maxVisibleCharacters / Words / Lines are at their 99999 defaults: every character stays visible.
                for v in &mut c.v {
                    v.pos = add(v.pos, zero3);
                }
                // FillCharacterVertexBuffers
                let q = Quad {
                    pos: [c.v[0].pos, c.v[1].pos, c.v[2].pos, c.v[3].pos],
                    uv: [c.v[0].uv, c.v[1].uv, c.v[2].uv, c.v[3].uv],
                    uv2: [c.v[0].uv2, c.v[1].uv2, c.v[2].uv2, c.v[3].uv2],
                    color: [c.v[0].color.arr(), c.v[1].color.arr(), c.v[2].color.arr(), c.v[3].color.arr()],
                    atlas: match &c.sprite {
                        Some((sa, _)) => sa.sheet,
                        None => c.font.atlas,
                    },
                };
                let mi = c.mat_idx;
                c.quad = self.quads[mi].len();
                self.quads[mi].push(q);
            }
            {
                let c = &mut self.chars[ju];
                c.bl = add(c.bl, zero3);
                c.tl = add(c.tl, zero3);
                c.tr = add(c.tr, zero3);
                c.br = add(c.br, zero3);
                c.origin += zero3[0];
                c.x_advance += zero3[0];
                c.ascender += zero3[1];
                c.descender += zero3[1];
                c.baseline += zero3[1];
            }
            if line_number != num67 || j == self.character_count - 1 {
                if line_number != num67 {
                    let l = num67 as usize;
                    self.line_info[l].baseline += zero3[1];
                    self.line_info[l].ascender += zero3[1];
                    self.line_info[l].descender += zero3[1];
                    self.line_info[l].max_advance += zero3[0];
                    self.line_info[l].ext_min = [self.chars[self.line_info[l].first_character_index as usize].bl[0], self.line_info[l].descender];
                    self.line_info[l].ext_max = [self.chars[self.line_info[l].last_visible_character_index as usize].tr[0], self.line_info[l].ascender];
                }
                if j == self.character_count - 1 {
                    let l = line_number as usize;
                    self.line_info[l].baseline += zero3[1];
                    self.line_info[l].ascender += zero3[1];
                    self.line_info[l].descender += zero3[1];
                    self.line_info[l].max_advance += zero3[0];
                    self.line_info[l].ext_min = [self.chars[self.line_info[l].first_character_index as usize].bl[0], self.line_info[l].descender];
                    self.line_info[l].ext_max = [self.chars[self.line_info[l].last_visible_character_index as usize].tr[0], self.line_info[l].ascender];
                }
            }
            // (word counting only feeds maxVisibleWords = 99999 and TMP_WordInfo: not needed)
            let style = self.chars[ju].style;
            let cnt = self.character_count;
            if style & UNDERLINE == UNDERLINE {
                if !is_white_space(character) && character != 0x200b {
                    num74 = fmax(num74, self.chars[ju].scale);
                    num71 = fmax(num71, num70.abs());
                    // pageNumber is always 0 (no Page overflow), so the previous page's offset is never discarded.
                    num75 = fmin(num75, self.chars[ju].baseline + self.font0.face.underline_offset * num74);
                }
                if !u_active && j <= li.last_visible_character_index && character != 10 && character != 11 && character != 13 && (j != li.last_visible_character_index || !is_separator(character)) {
                    u_active = true;
                    num72 = self.chars[ju].scale;
                    if num74 == 0.0 {
                        num74 = num72;
                        num71 = num70;
                    }
                    u_start = [self.chars[ju].bl[0], num75, 0.0];
                    underline_color = self.chars[ju].underline_color;
                }
                let mut draw: Option<(V3, f32)> = None;
                if u_active && cnt == 1 {
                    u_active = false;
                    draw = Some(([self.chars[ju].tr[0], num75, 0.0], self.chars[ju].scale));
                } else if u_active && (j == li.last_character_index || j >= li.last_visible_character_index) {
                    if is_white_space(character) || character == 0x200b {
                        let l = li.last_visible_character_index as usize;
                        draw = Some(([self.chars[l].tr[0], num75, 0.0], self.chars[l].scale));
                    } else {
                        draw = Some(([self.chars[ju].tr[0], num75, 0.0], self.chars[ju].scale));
                    }
                    u_active = false;
                } else if u_active && j < cnt - 1 && underline_color != self.chars[ju + 1].underline_color {
                    u_active = false;
                    draw = Some(([self.chars[ju].tr[0], num75, 0.0], self.chars[ju].scale));
                }
                if let Some((end, num73)) = draw {
                    self.draw_underline_mesh(u_start, end, num72, num73, num74, num71, underline_color);
                    num74 = 0.0;
                    num71 = 0.0;
                    num75 = LARGE_POS;
                }
            } else if u_active {
                u_active = false;
                let end = [self.chars[ju - 1].tr[0], num75, 0.0];
                let num73 = self.chars[ju - 1].scale;
                self.draw_underline_mesh(u_start, end, num72, num73, num74, num71, underline_color);
                num74 = 0.0;
                num71 = 0.0;
                num75 = LARGE_POS;
            }
            let strikethrough_offset = font_asset.face.strikethrough_offset;
            if style & STRIKETHROUGH == STRIKETHROUGH {
                if !s_active && j <= li.last_visible_character_index && character != 10 && character != 11 && character != 13 && (j != li.last_visible_character_index || !is_separator(character)) {
                    s_active = true;
                    num77 = self.chars[ju].point_size;
                    num78 = self.chars[ju].scale;
                    s_start = [self.chars[ju].bl[0], self.chars[ju].baseline + strikethrough_offset * num78, 0.0];
                    strike_color = self.chars[ju].strikethrough_color;
                    b = self.chars[ju].baseline;
                }
                let mut draw: Option<V3> = None;
                if s_active && cnt == 1 {
                    s_active = false;
                    draw = Some([self.chars[ju].tr[0], self.chars[ju].baseline + strikethrough_offset * num78, 0.0]);
                } else if s_active && j == li.last_character_index {
                    s_active = false;
                    if is_white_space(character) || character == 0x200b {
                        let l = li.last_visible_character_index as usize;
                        draw = Some([self.chars[l].tr[0], self.chars[l].baseline + strikethrough_offset * num78, 0.0]);
                    } else {
                        draw = Some([self.chars[ju].tr[0], self.chars[ju].baseline + strikethrough_offset * num78, 0.0]);
                    }
                } else if s_active && j < cnt && (self.chars[ju + 1].point_size != num77 || !((b - 0.0001) < (self.chars[ju + 1].baseline + zero3[1]) && (self.chars[ju + 1].baseline + zero3[1]) < b + 0.0001)) {
                    s_active = false;
                    let l = li.last_visible_character_index as usize;
                    draw = Some(if j <= li.last_visible_character_index {
                        [self.chars[ju].tr[0], self.chars[ju].baseline + strikethrough_offset * num78, 0.0]
                    } else {
                        [self.chars[l].tr[0], self.chars[l].baseline + strikethrough_offset * num78, 0.0]
                    });
                } else if s_active && j < cnt && !same_font(&font_asset, &self.chars[ju + 1].font) {
                    s_active = false;
                    draw = Some([self.chars[ju].tr[0], self.chars[ju].baseline + strikethrough_offset * num78, 0.0]);
                }
                if let Some(end) = draw {
                    self.draw_underline_mesh(s_start, end, num78, num78, num78, num70, strike_color);
                }
            } else if s_active {
                s_active = false;
                let end = [self.chars[ju - 1].tr[0], self.chars[ju - 1].baseline + strikethrough_offset * num78, 0.0];
                self.draw_underline_mesh(s_start, end, num78, num78, num78, num70, strike_color);
            }
            if style & HIGHLIGHT == HIGHLIGHT {
                unimplemented!("mod-only: TMP highlights (<mark>): no shipped text or script has the tag nor the Highlight font style");
            }
            num67 = line_number;
        }
        // SortGeometry(Reverse) covers the character quads only (the underline quads sit past vertexCount).
        if p.geometry_sorting_order != 0 {
            for q in &mut self.quads {
                q.reverse();
            }
        }
        let ul = std::mem::take(&mut self.underline_quads);
        self.quads[0].extend(ul);
    }

    /// TMP_Text.SaveGlyphVertexInfo (no vertex gradient).
    fn save_glyph_vertex_info(&mut self, padding: f32, style_padding: f32, vertex_color: C32) {
        let cc = self.character_count as usize;
        let mut color = vertex_color;
        color.a = if self.font_color32.a < color.a { self.font_color32.a } else { color.a };
        let style_padding = if !self.is_sdf_shader { 0.0 } else { style_padding };
        let font = self.cur_font.clone();
        let c = &mut self.chars[cc];
        let g = c.elem.as_ref().expect("text element").glyph;
        let (aw, ah) = (font.atlas_width as f32, font.atlas_height as f32);
        let r = g.rect;
        let uv = [(r[0] as f32 - padding - style_padding) / aw, (r[1] as f32 - padding - style_padding) / ah];
        let uv2 = [uv[0], (r[1] as f32 + padding + style_padding + r[3] as f32) / ah];
        let uv3 = [(r[0] as f32 + padding + style_padding + r[2] as f32) / aw, uv2[1]];
        let uv4 = [uv3[0], uv[1]];
        c.v[0].pos = c.bl;
        c.v[1].pos = c.tl;
        c.v[2].pos = c.tr;
        c.v[3].pos = c.br;
        for v in &mut c.v {
            v.color = color;
        }
        if self.p.enable_vertex_gradient && (self.p.override_html_colors || self.color_stack.index <= 1) {
            // m_fontColorGradient * vertexColor: Color * Color32 -> Color -> Color32.
            let mul = |g: f32, b: u8| ((g * (b as f32 / 255.0)).clamp(0.0, 1.0) * 255.0).round() as u8;
            for (v, g) in c.v.iter_mut().zip(self.p.font_color_gradient.iter()) {
                v.color = C32::new(mul(g[0], color.r), mul(g[1], color.g), mul(g[2], color.b), mul(g[3], color.a));
            }
        }
        c.v[0].uv = uv;
        c.v[1].uv = uv2;
        c.v[2].uv = uv3;
        c.v[3].uv = uv4;
    }

    /// TMP_Text.SaveSpriteVertexInfo (no vertex gradient preset).
    fn save_sprite_vertex_info(&mut self, vertex_color: C32) {
        let cc = self.character_count as usize;
        if self.p.tint_all_sprites {
            self.tint_sprite = true;
        }
        let mul = |a: C32, b: C32| {
            let m = |x: u8, y: u8| (x as f32 / 255.0 * (y as f32 / 255.0) * 255.0) as u8;
            C32::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), m(a.a, b.a))
        };
        let mut color = if self.tint_sprite { mul(self.sprite_color, vertex_color) } else { self.sprite_color };
        color.a = if color.a < self.font_color32.a {
            color.a = if color.a < vertex_color.a { color.a } else { vertex_color.a };
            color.a
        } else {
            self.font_color32.a
        };
        let mut cols = [color; 4];
        if self.p.enable_vertex_gradient {
            for (c, g) in cols.iter_mut().zip(self.p.font_color_gradient.iter()) {
                if self.tint_sprite {
                    *c = mul(*c, C32::from_color(*g));
                }
            }
        }
        let (sa, _) = self.chars[cc].sprite.clone().expect("sprite character");
        let c = &mut self.chars[cc];
        let r = c.elem.as_ref().expect("text element").glyph.rect;
        let (w, h) = (sa.sheet_size[0] as f32, sa.sheet_size[1] as f32);
        let uv = [r[0] as f32 / w, r[1] as f32 / h];
        let uv2 = [uv[0], (r[1] + r[3]) as f32 / h];
        let uv3 = [(r[0] + r[2]) as f32 / w, uv2[1]];
        let uv4 = [uv3[0], uv[1]];
        c.v[0].pos = c.bl;
        c.v[1].pos = c.tl;
        c.v[2].pos = c.tr;
        c.v[3].pos = c.br;
        for (v, col) in c.v.iter_mut().zip(cols) {
            v.color = col;
        }
        c.v[0].uv = uv;
        c.v[1].uv = uv2;
        c.v[2].uv = uv3;
        c.v[3].uv = uv4;
    }

    /// TMP_Text.DrawUnderlineMesh: three quads from the primary font's '_' glyph.
    fn draw_underline_mesh(&mut self, start: V3, end: V3, start_scale: f32, end_scale: f32, max_scale: f32, sdf_scale: f32, underline_color: C32) {
        let font = self.font0.clone();
        let Some(ch) = font.characters.get(&95) else { return };
        let glyph = font.glyph(ch);
        let mut start = start;
        let mut end = end;
        start[1] = fmin(start[1], end[1]);
        end[1] = fmin(start[1], end[1]);
        let m = glyph.metrics;
        let r = glyph.rect;
        let padding = self.padding0();
        let mut num2 = m.width / 2.0 * max_scale;
        if end[0] - start[0] < m.width * max_scale {
            num2 = (end[0] - start[0]) / 2.0;
        }
        let num3 = padding * start_scale / max_scale;
        let num4 = padding * end_scale / max_scale;
        let thickness = font.face.underline_thickness;
        let add = |a: V3, x: f32, y: f32| [a[0] + x, a[1] + y, a[2] + 0.0];
        let mut v = [[0.0f32; 3]; 12];
        v[0] = add(start, 0.0, 0.0 - (thickness + padding) * max_scale);
        v[1] = add(start, 0.0, padding * max_scale);
        v[2] = add(v[1], num2, 0.0);
        v[3] = add(v[0], num2, 0.0);
        v[4] = v[3];
        v[5] = v[2];
        v[6] = add(end, 0.0 - num2, padding * max_scale);
        v[7] = add(end, 0.0 - num2, (0.0 - (thickness + padding)) * max_scale);
        v[8] = v[7];
        v[9] = v[6];
        v[10] = add(end, 0.0, padding * max_scale);
        v[11] = add(end, 0.0, (0.0 - (thickness + padding)) * max_scale);
        let (aw, ah) = (font.atlas_width as f32, font.atlas_height as f32);
        let (rx, ry, rw, rh) = (r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32);
        let vec1 = [(rx - num3) / aw, (ry - padding) / ah];
        let vec2 = [vec1[0], ((r[1] + r[3]) as f32 + padding) / ah];
        let vec3 = [(rx - num3 + rw / 2.0) / aw, vec2[1]];
        let vec4 = [vec3[0], vec1[1]];
        let vec5 = [(rx + num4 + rw / 2.0) / aw, vec2[1]];
        let vec6 = [vec5[0], vec1[1]];
        let vec7 = [(rx + num4 + rw) / aw, vec2[1]];
        let vec8 = [vec7[0], vec1[1]];
        let uvs = [
            vec1,
            vec2,
            vec3,
            vec4,
            [vec3[0] - vec3[0] * 0.001, vec1[1]],
            [vec3[0] - vec3[0] * 0.001, vec2[1]],
            [vec3[0] + vec3[0] * 0.001, vec2[1]],
            [vec3[0] + vec3[0] * 0.001, vec1[1]],
            vec6,
            vec5,
            vec7,
            vec8,
        ];
        let scale = sdf_scale.abs();
        let pack = |x: f32, y: f32| -> [f32; 2] {
            let mut rx = (x * 511.0) as i32 as f32;
            let ry = (y * 511.0) as i32 as f32;
            rx = rx * 4096.0 + ry;
            [rx, scale]
        };
        let den = end[0] - start[0];
        let x = (v[2][0] - start[0]) / den;
        let mut uv2 = [[0.0f32; 2]; 12];
        uv2[0] = pack(0.0, 0.0);
        uv2[1] = pack(0.0, 1.0);
        uv2[2] = pack(x, 1.0);
        uv2[3] = pack(x, 0.0);
        let n5 = (v[4][0] - start[0]) / den;
        let x = (v[6][0] - start[0]) / den;
        uv2[4] = pack(n5, 0.0);
        uv2[5] = pack(n5, 1.0);
        uv2[6] = pack(x, 1.0);
        uv2[7] = pack(x, 0.0);
        let n5 = (v[8][0] - start[0]) / den;
        uv2[8] = pack(n5, 0.0);
        uv2[9] = pack(n5, 1.0);
        uv2[10] = pack(1.0, 1.0);
        uv2[11] = pack(1.0, 0.0);
        let mut color = underline_color;
        color.a = if self.font_color32.a < color.a { self.font_color32.a } else { color.a };
        for k in 0..3 {
            let o = k * 4;
            self.underline_quads.push(Quad {
                pos: [v[o], v[o + 1], v[o + 2], v[o + 3]],
                uv: [uvs[o], uvs[o + 1], uvs[o + 2], uvs[o + 3]],
                uv2: [uv2[o], uv2[o + 1], uv2[o + 2], uv2[o + 3]],
                color: [color.arr(); 4],
                atlas: font.atlas,
            });
        }
    }

    // ---- preferred size ----

    /// TMP_Text.CalculatePreferredValues.
    fn calculate_preferred_values(&mut self, font_size: &mut f32, margin_size: [f32; 2], auto_sizing: bool, word_wrapping: bool) -> [f32; 2] {
        let p = self.p;
        if self.tp.is_empty() || self.tp[0] == 0 {
            self.is_auto_size_point_size_set = true;
            return [0.0, 0.0];
        }
        // ScrollRect lays out like Overflow in TextMeshPro (its only case label shares InsertNewLine with Overflow/Masking).
        if matches!(self.overflow_mode, 5 | 6) {
            unimplemented!("mod-only: TMP Page / Linked overflow (shipped texts use overflow modes 0-4)");
        }
        self.cur_font = self.font0.clone();
        self.cur_material = self.mats[0].arena;
        self.cur_mat_idx = 0;
        let total_character_count = self.total_character_count;
        // m_internalCharacterInfo: only the fields CalculatePreferredValues reads back.
        let mut internal_char: Vec<(u16, f32)> = vec![(0, 0.0); self.chars.len().max(total_character_count as usize + 2)];
        let ortho = if p.is_orthographic { 1.0f32 } else { 0.1f32 };
        let num = *font_size / self.font0.face.point_size as f32 * self.font0.face.scale * ortho;
        let mut num2: f32;
        let num3 = *font_size * 0.01 * ortho;
        self.font_scale_multiplier = 1.0;
        self.current_font_size = *font_size;
        self.size_stack.set_default(self.current_font_size);
        self.font_style_internal = p.font_style;
        self.line_justification = p.horizontal_alignment;
        self.line_justification_stack.set_default(self.line_justification);
        self.baseline_offset = 0.0;
        self.line_offset = 0.0;
        let num5 = self.font0.face.line_height - (self.font0.face.ascent_line - self.font0.face.descent_line);
        self.c_spacing = 0.0;
        self.mono_spacing = 0.0;
        self.x_advance = 0.0;
        let mut a = 0.0f32;
        self.tag_no_parsing = false;
        self.character_count = 0;
        self.first_character_of_line = 0;
        self.max_line_ascender = LARGE_NEG;
        self.max_line_descender = LARGE_POS;
        self.line_number = 0;
        self.start_of_line_ascender = 0.0;
        self.is_driven_line_spacing = false;
        self.line_height = LINE_HEIGHT_UNSET;
        let x = margin_size[0];
        self.margin_left = 0.0;
        self.margin_right = 0.0;
        let mut num6 = 0.0f32;
        let mut num7 = 0.0f32;
        self.width = -1.0;
        let mut num8: f32;
        let mut num9 = 0.0f32;
        let mut num10 = 0.0f32;
        let mut num11 = 0.0f32;
        self.is_calculating_preferred_values = true;
        self.max_cap_height = 0.0;
        self.max_text_ascender = 0.0;
        self.element_descender = 0.0;
        let mut flag2 = true;
        self.is_non_breaking_space = false;
        let mut subst: (i32, u32) = (-1, 0);
        let mut flag4 = false;
        let mut state = WordWrapState::blank(&self.font0);
        let mut state2 = WordWrapState::blank(&self.font0);
        let mut state3 = WordWrapState::blank(&self.font0);
        self.auto_size_iteration_count += 1;
        let mut i: i32 = -1;
        loop {
            i += 1;
            if !((i as usize) < self.tp.len() && self.tp[i as usize] != 0) {
                break;
            }
            let mut num12 = self.tp[i as usize];
            if p.is_rich_text && num12 == 60 {
                self.is_parsing_text = true;
                self.text_element_type = 0;
                if let Some(end) = self.validate_html_tag(i as usize + 1) {
                    i = end as i32;
                    if self.text_element_type == 0 {
                        continue;
                    }
                }
            } else {
                let c = &self.chars[self.character_count as usize];
                self.text_element_type = c.sprite.is_some() as u8;
                self.cur_mat_idx = c.mat_idx;
                self.cur_font = c.font.clone();
            }
            self.is_parsing_text = false;
            let mut flag5 = false;
            let cc = self.character_count as usize;
            if subst.0 == self.character_count {
                num12 = subst.1 as i32;
                self.text_element_type = 0;
                flag5 = true;
                match num12 {
                    3 => {
                        self.is_text_truncated = true;
                    }
                    8230 => {
                        self.is_text_truncated = true;
                        subst = (self.character_count + 1, 3);
                    }
                    _ => {}
                }
            }
            let mut num13 = 1.0f32;
            let is_sprite = self.text_element_type == 1;
            if !is_sprite {
                if self.font_style_internal & UPPER_CASE == UPPER_CASE {
                    if char_is_lower(num12) {
                        num12 = char_to_upper(num12);
                    }
                } else if self.font_style_internal & LOWER_CASE == LOWER_CASE {
                    if char_is_upper(num12) {
                        num12 = char_to_lower(num12);
                    }
                } else if self.font_style_internal & SMALL_CAPS == SMALL_CAPS && char_is_lower(num12) {
                    num13 = 0.8;
                    num12 = char_to_upper(num12);
                }
            }
            let mut elem = self.chars[cc].elem.clone().expect("text element");
            self.cur_mat_idx = self.chars[cc].mat_idx;
            let cur_font = self.cur_font.clone();
            let face = &cur_font.face;
            let (num14, num15);
            if is_sprite {
                let (sa, sidx) = self.chars[cc].sprite.clone().expect("sprite character");
                let sc = sprite_character(&sa, sidx);
                if num12 == 60 {
                    num12 = 57344 + sidx;
                }
                let fs = self.current_font_size;
                if sa.face.point_size > 0 {
                    let num16 = fs / sa.face.point_size as f32 * sa.face.scale * ortho;
                    num2 = sc.scale * sc.glyph.scale * num16;
                    num14 = sa.face.ascent_line;
                    num15 = sa.face.descent_line;
                } else {
                    let num17 = fs / face.point_size as f32 * face.scale * ortho;
                    num2 = face.ascent_line / sc.glyph.metrics.height * sc.scale * sc.glyph.scale * num17;
                    let num18 = num17 / num2;
                    num14 = face.ascent_line * num18;
                    num15 = face.descent_line * num18;
                }
                elem = sprite_elem(&cur_font, &sa, &sc);
            } else {
                let num19 = if !flag5 || self.tp[i as usize] != 10 || self.character_count == self.first_character_of_line {
                    self.current_font_size * num13 / face.point_size as f32 * face.scale * ortho
                } else {
                    self.chars[cc - 1].point_size * num13 / face.point_size as f32 * face.scale * ortho
                };
                (num14, num15) = if flag5 && num12 == 8230 { (0.0f32, 0.0f32) } else { (face.ascent_line, face.descent_line) };
                num2 = num19 * self.font_scale_multiplier * elem.ch.scale;
            }
            let num20 = num2;
            if num12 == 173 || num12 == 3 {
                num2 = 0.0;
            }
            internal_char[cc].0 = num12 as u16;
            let metrics = elem.glyph.metrics;
            let flag6 = num12 <= 65535 && is_white_space(num12);
            let mut gvr = GlyphValueRecord::default();
            let mut num21 = p.character_spacing;
            self.glyph_horizontal_advance_adjustment = 0.0;
            if p.enable_kerning {
                let glyph_index = elem.ch.glyph_index;
                if self.character_count < total_character_count - 1 {
                    let next = self.chars[cc + 1].elem.as_ref().expect("next element").ch.glyph_index;
                    if let Some(v) = cur_font.pair(((next << 16) | glyph_index)) {
                        gvr = v.first;
                        num21 = if v.flags & 0x100 == 0x100 { 0.0 } else { num21 };
                    }
                }
                if self.character_count >= 1 {
                    let prev = self.chars[cc - 1].elem.as_ref().expect("previous element").ch.glyph_index;
                    if let Some(v) = cur_font.pair(((glyph_index << 16) | prev)) {
                        gvr = gvr + v.second;
                        num21 = if v.flags & 0x100 == 0x100 { 0.0 } else { num21 };
                    }
                }
                self.glyph_horizontal_advance_adjustment = gvr.x_advance;
            }
            let mut num22 = 0.0f32;
            if self.mono_spacing != 0.0 {
                num22 = (self.mono_spacing / 2.0 - (metrics.width / 2.0 + metrics.horizontal_bearing_x) * num2) * (1.0 - self.char_width_adj_delta);
                self.x_advance += num22;
            }
            let mut num23 = 0.0f32;
            if !is_sprite && !self.chars[self.character_count as usize].alt && self.font_style_internal & BOLD == BOLD {
                num23 = cur_font.bold_spacing;
            }
            let (num24, num25) = if is_sprite {
                (num14 * num2 + self.baseline_offset, num15 * num2 + self.baseline_offset)
            } else {
                (num14 * num2 / num13 + self.baseline_offset, num15 * num2 / num13 + self.baseline_offset)
            };
            let mut num26 = num24;
            let mut num27 = num25;
            let flag7 = self.character_count == self.first_character_of_line;
            if flag7 || !flag6 {
                if self.baseline_offset != 0.0 {
                    num26 = fmax((num24 - self.baseline_offset) / self.font_scale_multiplier, num26);
                    num27 = fmin((num25 - self.baseline_offset) / self.font_scale_multiplier, num27);
                }
                self.max_line_ascender = fmax(num26, self.max_line_ascender);
                self.max_line_descender = fmin(num27, self.max_line_descender);
            }
            if flag7 || !flag6 {
                internal_char[cc].1 = num26;
                self.element_descender = num25 - self.line_offset;
            } else {
                internal_char[cc].1 = self.max_line_ascender;
                self.element_descender = self.max_line_descender - self.line_offset;
            }
            if self.line_number == 0 && (flag7 || !flag6) {
                self.max_text_ascender = self.max_line_ascender;
                self.max_cap_height = fmax(self.max_cap_height, cur_font.face.cap_line * num2 / num13);
            }
            if self.line_offset == 0.0 && (!flag6 || self.character_count == self.first_character_of_line) {
                self.page_ascender = if self.page_ascender > num24 { self.page_ascender } else { num24 };
            }
            let flag8 = self.line_justification & H_FLUSH == H_FLUSH || self.line_justification & H_JUSTIFIED == H_JUSTIFIED;
            if num12 == 9 || (!flag6 && num12 != 8203 && num12 != 173 && num12 != 3) || (num12 == 173 && !flag4) || is_sprite {
                num8 = if self.width != -1.0 { fmin(x + 0.0001 - self.margin_left - self.margin_right, self.width) } else { x + 0.0001 - self.margin_left - self.margin_right };
                num11 = self.x_advance.abs() + metrics.horizontal_advance * (1.0 - self.char_width_adj_delta) * (if num12 == 173 { num20 } else { num2 });
                if num11 > num8 * (if flag8 { 1.05 } else { 1.0 }) && word_wrapping && self.character_count != self.first_character_of_line {
                    i = self.restore(&state);
                    let cc = self.character_count as usize;
                    if internal_char[cc - 1].0 == 0xad && !flag4 && self.overflow_mode == OVERFLOW {
                        subst = (self.character_count - 1, 45);
                        i -= 1;
                        self.character_count -= 1;
                        continue;
                    }
                    flag4 = false;
                    if internal_char[cc].0 == 0xad {
                        flag4 = true;
                        continue;
                    }
                    if auto_sizing & flag2 {
                        if self.char_width_adj_delta < p.char_width_max_adj / 100.0 && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
                            self.widen_chars(num11, num8, flag8);
                            return [0.0, 0.0];
                        }
                        if *font_size > p.font_size_min && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
                            self.max_font_size = *font_size;
                            let n = fmax((*font_size - self.min_font_size) / 2.0, 0.05);
                            *font_size -= n;
                            *font_size = fmax((((*font_size * 20.0 + 0.5) as i32) as f32) / 20.0, p.font_size_min);
                            return [0.0, 0.0];
                        }
                    }
                    let num31 = self.max_line_ascender - self.start_of_line_ascender;
                    if self.line_offset > 0.0 && num31.abs() > 0.01 && !self.is_driven_line_spacing {
                        self.element_descender -= num31;
                        self.line_offset += num31;
                    }
                    let num32 = self.max_line_ascender - self.line_offset;
                    let num33 = self.max_line_descender - self.line_offset;
                    self.element_descender = if self.element_descender < num33 { self.element_descender } else { num33 };
                    self.first_character_of_line = self.character_count;
                    self.line_visible_character_count = 0;
                    num9 += self.x_advance;
                    num10 = if !word_wrapping { fmax(num10, num32 - num33) } else { self.max_text_ascender - self.element_descender };
                    state2 = self.snapshot(i, self.character_count - 1, &state2);
                    self.line_number += 1;
                    let adjusted_ascender = internal_char[self.character_count as usize].1;
                    if self.line_height == LINE_HEIGHT_UNSET {
                        self.line_offset += 0.0 - self.max_line_descender + adjusted_ascender + (num5 + self.line_spacing_delta) * num + p.line_spacing * num3;
                        self.is_driven_line_spacing = false;
                    } else {
                        self.line_offset += self.line_height + p.line_spacing * num3;
                        self.is_driven_line_spacing = true;
                    }
                    self.max_line_ascender = LARGE_NEG;
                    self.max_line_descender = LARGE_POS;
                    self.start_of_line_ascender = adjusted_ascender;
                    self.x_advance = 0.0 + 0.0;
                    flag2 = true;
                    continue;
                }
                num6 = self.margin_left;
                num7 = self.margin_right;
            }
            let cc = self.character_count as usize;
            if num12 == 9 {
                let num34 = cur_font.face.tab_width * cur_font.tab_size as f32 * num2;
                let num35 = (self.x_advance / num34).ceil() * num34;
                self.x_advance = if num35 > self.x_advance { num35 } else { self.x_advance + num34 };
            } else if self.mono_spacing != 0.0 {
                self.x_advance += (self.mono_spacing - num22 + (cur_font.normal_spacing_offset + num21) * num3 + self.c_spacing) * (1.0 - self.char_width_adj_delta);
                if flag6 || num12 == 8203 {
                    self.x_advance += p.word_spacing * num3;
                }
            } else {
                self.x_advance += ((metrics.horizontal_advance + gvr.x_advance) * num2 + (cur_font.normal_spacing_offset + num21 + num23) * num3 + self.c_spacing) * (1.0 - self.char_width_adj_delta);
                if flag6 || num12 == 8203 {
                    self.x_advance += p.word_spacing * num3;
                }
            }
            if num12 == 13 {
                a = fmax(a, num9 + self.x_advance);
                num9 = 0.0;
                self.x_advance = 0.0 + 0.0;
            }
            if num12 == 10 || num12 == 11 || num12 == 3 || num12 == 8232 || num12 == 8233 || self.character_count == total_character_count - 1 {
                let num36 = self.max_line_ascender - self.start_of_line_ascender;
                if self.line_offset > 0.0 && num36.abs() > 0.01 && !self.is_driven_line_spacing {
                    self.element_descender -= num36;
                    self.line_offset += num36;
                }
                let num37 = self.max_line_descender - self.line_offset;
                self.element_descender = if self.element_descender < num37 { self.element_descender } else { num37 };
                if self.character_count == total_character_count - 1 {
                    num9 = fmax(a, num9 + num11 + num6 + num7);
                } else {
                    a = fmax(a, num9 + num11 + num6 + num7);
                    num9 = 0.0;
                }
                num10 = self.max_text_ascender - self.element_descender;
                match num12 {
                    10 | 11 | 45 | 8232 | 8233 => {
                        state2 = self.snapshot(i, self.character_count, &state2);
                        state = self.snapshot(i, self.character_count, &state);
                        self.line_number += 1;
                        self.first_character_of_line = self.character_count + 1;
                        let adjusted_ascender2 = internal_char[cc].1;
                        let para = if num12 == 10 || num12 == 8233 { p.paragraph_spacing } else { 0.0 };
                        let num38 = 0.0 - self.max_line_descender + adjusted_ascender2 + (num5 + self.line_spacing_delta) * num + (p.line_spacing + para) * num3;
                        if self.line_height == LINE_HEIGHT_UNSET {
                            self.line_offset += num38;
                            self.is_driven_line_spacing = false;
                        } else {
                            self.line_offset += self.line_height + (p.line_spacing + para) * num3;
                            self.is_driven_line_spacing = true;
                        }
                        self.max_line_ascender = LARGE_NEG;
                        self.max_line_descender = LARGE_POS;
                        self.start_of_line_ascender = adjusted_ascender2;
                        self.x_advance = 0.0 + 0.0 + 0.0;
                        self.character_count += 1;
                        continue;
                    }
                    3 => {
                        i = self.tp.len() as i32;
                    }
                    _ => {}
                }
            }
            if word_wrapping || self.overflow_mode == TRUNCATE || self.overflow_mode == ELLIPSIS {
                if (flag6 || num12 == 8203 || num12 == 45 || num12 == 173) && !self.is_non_breaking_space && num12 != 160 && num12 != 8199 && num12 != 8209 && num12 != 8239 && num12 != 8288 {
                    state = self.snapshot(i, self.character_count, &state);
                    flag2 = false;
                    state3.previous_word_break = -1;
                } else if !self.is_non_breaking_space
                    && ((((num12 > 4352 && num12 < 4607) || (num12 > 43360 && num12 < 43391) || (num12 > 44032 && num12 < 55295)) && !p.settings.modern_hangul)
                        || (num12 > 11904 && num12 < 40959)
                        || (num12 > 63744 && num12 < 64255)
                        || (num12 > 65072 && num12 < 65103)
                        || (num12 > 65280 && num12 < 65519))
                {
                    unimplemented!("mod-only: TMP CJK / Hangul line breaking rules (U+{num12:04X}): no shipped text or script has such a character");
                } else if flag2 {
                    if flag6 || (num12 == 173 && !flag4) {
                        state3 = self.snapshot(i, self.character_count, &state3);
                    }
                    state = self.snapshot(i, self.character_count, &state);
                }
            }
            self.character_count += 1;
        }
        let num4 = self.max_font_size - self.min_font_size;
        if auto_sizing && num4 > 0.051 && *font_size < p.font_size_max && self.auto_size_iteration_count < AUTO_SIZE_MAX_ITERATIONS {
            if self.char_width_adj_delta < p.char_width_max_adj / 100.0 {
                self.char_width_adj_delta = 0.0;
            }
            self.min_font_size = *font_size;
            let n = fmax((self.max_font_size - *font_size) / 2.0, 0.05);
            *font_size += n;
            *font_size = fmin((((*font_size * 20.0 + 0.5) as i32) as f32) / 20.0, p.font_size_max);
            return [0.0, 0.0];
        }
        self.is_auto_size_point_size_set = true;
        self.is_calculating_preferred_values = false;
        let m = p.margin;
        num9 += if m[0] > 0.0 { m[0] } else { 0.0 };
        num9 += if m[2] > 0.0 { m[2] } else { 0.0 };
        num10 += if m[1] > 0.0 { m[1] } else { 0.0 };
        num10 += if m[3] > 0.0 { m[3] } else { 0.0 };
        num9 = (((num9 * 100.0 + 1.0) as i32) as f32) / 100.0;
        num10 = (((num10 * 100.0 + 1.0) as i32) as f32) / 100.0;
        [num9, num10]
    }

    /// TMP_Text.GetPreferredWidth().
    fn get_preferred_width(&mut self) -> f32 {
        let p = self.p;
        let mut num = if p.enable_auto_sizing { p.font_size_max } else { self.font_size };
        self.min_font_size = p.font_size_min;
        self.max_font_size = p.font_size_max;
        self.char_width_adj_delta = 0.0;
        self.is_calculating_preferred_values = true;
        self.parse_input_text();
        self.auto_size_iteration_count = 0;
        self.calculate_preferred_values(&mut num, [LARGE_POS_V2, LARGE_POS_V2], false, false)[0]
    }

    /// TMP_Text.GetPreferredHeight().
    fn get_preferred_height(&mut self) -> f32 {
        let p = self.p;
        let mut num = if p.enable_auto_sizing { p.font_size_max } else { self.font_size };
        self.min_font_size = p.font_size_min;
        self.max_font_size = p.font_size_max;
        self.char_width_adj_delta = 0.0;
        let margin = [if self.margin_width != 0.0 { self.margin_width } else { LARGE_POS }, LARGE_POS];
        self.is_calculating_preferred_values = true;
        self.parse_input_text();
        self.is_auto_size_point_size_set = false;
        self.auto_size_iteration_count = 0;
        let mut result = 0.0;
        while !self.is_auto_size_point_size_set {
            result = self.calculate_preferred_values(&mut num, margin, p.enable_auto_sizing, p.enable_word_wrapping)[1];
            self.auto_size_iteration_count += 1;
        }
        result
    }
}

/// TMP_Text.GetPreferredValues(): GetPreferredWidth() and GetPreferredHeight() of `props`.
pub fn preferred_values(props: &TextProps) -> [f32; 2] {
    [G::new(props).get_preferred_width(), G::new(props).get_preferred_height()]
}

/// TextMeshPro.OnPreRenderObject: the text mesh a TextMeshPro component builds for `props`.
pub fn generate(props: &TextProps) -> TextMesh {
    generate_with(props, &mut false, &mut None)
}

/// `generate`, with `parsed` set once ParseInputText returned (OnPreRenderObject then clears m_havePropertiesChanged, so a
/// later exception does not leave the text to regenerate). An exception in GenerateTextMesh leaves `partial`: TMP's
/// meshInfo and characterInfo as far as they were filled, and the animations already started on them.
pub fn generate_with(props: &TextProps, parsed: &mut bool, partial: &mut Option<TextMesh>) -> TextMesh {
    let mut g = G::new(props);
    g.parse_input_text();
    *parsed = true;
    super::dynamic::update_font_features_in_queue();
    if props.enable_auto_sizing {
        let v = props.font_size_base;
        g.font_size = if v < props.font_size_min {
            props.font_size_min
        } else if v > props.font_size_max {
            props.font_size_max
        } else {
            v
        };
    }
    g.max_font_size = props.font_size_max;
    g.min_font_size = props.font_size_min;
    g.line_spacing_delta = 0.0;
    g.char_width_adj_delta = 0.0;
    g.is_text_truncated = false;
    g.is_auto_size_point_size_set = false;
    g.auto_size_iteration_count = 0;
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        while !g.is_auto_size_point_size_set {
            g.generate_text_mesh();
            g.auto_size_iteration_count += 1;
        }
    }));
    if let Err(e) = run {
        *partial = Some(g.mesh(true));
        std::panic::resume_unwind(e);
    }
    // Preferred sizes are only computed in TMP when read, so their DoSpriteAnimation calls are not taken.
    let mut m = g.mesh(false);
    m.preferred = [g.get_preferred_width(), g.get_preferred_height()];
    // (CalculatePreferredValues can set it.)
    m.is_text_truncated = g.is_text_truncated;
    m
}

impl G<'_> {
    /// The text mesh from meshInfo as it stands, with the animations the last GenerateTextMesh started and the
    /// characterInfo their coroutines read. `thrown`: GenerateTextMesh threw in its layout loop, before the alignment pass
    /// fills the vertices: every material SetArraySizes counted has its meshInfo, still empty.
    fn mesh(&mut self, thrown: bool) -> TextMesh {
        let mut sprite_anims = self.sprite_anims.take();
        for a in sprite_anims.iter_mut().flatten() {
            a.info = self.chars.get(a.character as usize).map(|c| super::AnimChar { mat: c.mat_idx, quad: c.quad, origin: c.origin, baseline: c.baseline, scale: c.scale, character: c.character });
        }
        let quads: Vec<Quad> = self.quads.iter().flatten().copied().collect();
        let main_len = self.quads.first().map_or(0, |q| q.len());
        // TMP_Text.GenerateTextMesh: each material reference past the first fills a TMP_SubMesh with its own material and font atlas.
        let count = if thrown { self.mats.len() } else { self.quads.len() };
        let submeshes = (1..count)
            .map(|i| {
                let m = &self.mats[i];
                // A sprite asset's material samples its sprite sheet (never a dynamic atlas).
                let (atlas, dynamic) = match &m.sprite {
                    Some(sa) => (sa.sheet, None),
                    None => (m.font.atlas, m.font.state.clone()),
                };
                SubMesh { material: self.arena[m.arena].mat.clone(), atlas, dynamic, quads: self.quads.get(i).cloned().unwrap_or_default() }
            })
            .collect();
        TextMesh { quads, main_len, submeshes, font_size: self.font_size, preferred: [0.0; 2], is_text_truncated: self.is_text_truncated, sprite_anims }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::db::AssetDb;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn vcr() -> Option<(Arc<super::super::FontAsset>, super::super::TmpSettings)> {
        let game = crate::game_assets::find_game(None).ok()?;
        let mut db = AssetDb::open(&game).unwrap();
        let mut cache = HashMap::new();
        let r = super::super::installed_font(&mut db, "VCR_OSD_MONO");
        let f = super::super::FontAsset::read(&mut db, r, &mut cache).unwrap();
        let s = super::super::TmpSettings::read(&mut db, &mut cache).unwrap();
        Some((f, s))
    }

    /// A font with U+0003 and U+2026: GenerateTextMesh indexes characterLookupTable[3] after an ellipsis (where a font
    /// lacks U+0003 the real component throws KeyNotFoundException). Since 1.28.41 none of the world texts' four fonts has
    /// U+2026, so the TMP default font (LiberationSans SDF) is tried last.
    fn ellipsis_font() -> Option<(Arc<super::super::FontAsset>, super::super::TmpSettings)> {
        let game = crate::game_assets::find_game(None).ok()?;
        let mut db = AssetDb::open(&game).unwrap();
        let mut cache = HashMap::new();
        let s = super::super::TmpSettings::read(&mut db, &mut cache).unwrap();
        for name in ["VCR_OSD_MONO", "alarm clock SDF", "arial SDF bold", "arial SDF", "LiberationSans SDF"] {
            let r = super::super::installed_font(&mut db, name);
            let f = super::super::FontAsset::read(&mut db, r, &mut cache).unwrap();
            if f.characters.contains_key(&3) && f.characters.contains_key(&8230) {
                return Some((f, s));
            }
        }
        panic!("no installed font has U+2026");
    }

    fn props(text: &str, size: f32, font: Arc<super::super::FontAsset>, s: super::super::TmpSettings) -> TextProps {
        TextProps {
            text: text.into(),
            font,
            font_color: [1.0; 4],
            font_size: size,
            font_size_base: size,
            font_size_min: 0.1,
            font_size_max: size,
            horizontal_alignment: 1,
            vertical_alignment: 1024,
            char_width_max_adj: 0.0,
            enable_kerning: true,
            is_rich_text: true,
            parse_ctrl_characters: true,
            rect_size: [20.0, 5.0],
            pivot: [0.5, 0.5],
            lossy_scale_y: 1.0,
            text_input_box: true,
            settings: s,
            ..Default::default()
        }
    }

    fn bounds(m: &TextMesh) -> ([f32; 2], [f32; 2]) {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for q in &m.quads {
            for c in q.pos {
                for k in 0..2 {
                    lo[k] = lo[k].min(c[k]);
                    hi[k] = hi[k].max(c[k]);
                }
            }
        }
        (lo, hi)
    }

    /// "65 BPM" at 0.8, left/bottom: corners from the glyph metrics by hand.
    #[test]
    fn bpm_left_bottom() {
        let Some((f, s)) = vcr() else { return };
        let m = generate(&props("65 BPM", 0.8, f.clone(), s));
        let face = &f.face;
        let scale0 = 0.8f32 / face.point_size as f32 * face.scale * 0.1;
        let num3 = 0.8f32 * 0.01 * 0.1;
        // Bitmap shader (no _GradientScale): padding 1 plus the material's _Padding.
        let pad = 1.0f32 + if f.material.has("_Padding") { f.material.float("_Padding") } else { 0.0 };
        // 5 visible glyphs (the space has no quad).
        assert_eq!(m.quads.len(), 5, "{:?}", m.quads.len());
        let mut x = -20.0f32 * 0.5;
        let base = -5.0f32 * 0.5 - face.descent_line * scale0;
        let text: Vec<char> = "65 BPM".chars().collect();
        let mut qi = 0;
        for (k, &ch) in text.iter().enumerate() {
            let c = f.characters[&(ch as u32)];
            let g = &f.glyph(&c);
            let sc = scale0 * 1.0 * c.scale;
            let mut gvr = GlyphValueRecord::default();
            let mut sp = 0.0f32;
            if k + 1 < text.len() {
                let n = f.characters[&(text[k + 1] as u32)].glyph_index;
                if let Some(v) = f.pair(((n << 16) | c.glyph_index)) {
                    gvr = v.first;
                    sp = if v.flags & 0x100 != 0 { 0.0 } else { sp };
                }
            }
            if k >= 1 {
                let p = f.characters[&(text[k - 1] as u32)].glyph_index;
                if let Some(v) = f.pair((c.glyph_index << 16) | p) {
                    gvr = gvr + v.second;
                }
            }
            if ch != ' ' {
                let q = &m.quads[qi];
                qi += 1;
                let bl = [x + (g.metrics.horizontal_bearing_x - pad) * sc + gvr.x_placement * sc, base + (g.metrics.horizontal_bearing_y - g.metrics.height - pad) * sc + gvr.y_placement * sc];
                assert!((q.pos[0][0] - bl[0]).abs() < 1e-5 && (q.pos[0][1] - bl[1]).abs() < 1e-5, "char {ch}: {:?} vs {bl:?}", q.pos[0]);
                let tr = [bl[0] + (g.metrics.width + 2.0 * pad) * sc, bl[1] + (g.metrics.height + 2.0 * pad) * sc];
                assert!((q.pos[2][0] - tr[0]).abs() < 1e-5 && (q.pos[2][1] - tr[1]).abs() < 1e-5, "char {ch}: {:?} vs {tr:?}", q.pos[2]);
            }
            x += ((g.metrics.horizontal_advance + gvr.x_advance) * sc + (f.normal_spacing_offset + sp) * num3) * 1.0;
        }
        assert_eq!(m.font_size, 0.8);
        assert!(!m.is_text_truncated);
    }

    /// An emoji in the default sprite asset and `<sprite=N>` lay out through the sprite path: one quad in the sprite material's
    /// sub mesh, uv from the glyph rect over the sheet, size from the glyph metrics at the sprite scale.
    #[test]
    fn sprite_characters() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let s = super::super::TmpSettings::read(&mut db, &mut HashMap::new()).unwrap();
        let f = s.default_font.clone().expect("default font");
        let d = s.default_sprite.clone().expect("default sprite asset");
        let (&unicode, &table) = d.by_unicode.iter().min_by_key(|&(_, &i)| i).expect("a sprite character");
        let sc = d.characters[table].clone().unwrap();
        let emoji = generate(&props(&char::from_u32(unicode).unwrap().to_string(), 0.8, f.clone(), s.clone()));
        assert_eq!(emoji.quads.len(), 1);
        assert_eq!(emoji.main_len, 0);
        assert_eq!(emoji.submeshes.len(), 1);
        let q = &emoji.quads[0];
        assert_eq!(q.atlas, d.sheet);
        assert_eq!(emoji.submeshes[0].atlas, d.sheet);
        let r = sc.glyph.rect;
        let (w, h) = (d.sheet_size[0] as f32, d.sheet_size[1] as f32);
        assert_eq!(q.uv[0], [r[0] as f32 / w, r[1] as f32 / h]);
        assert_eq!(q.uv[2], [(r[0] + r[2]) as f32 / w, (r[1] + r[3]) as f32 / h]);
        let (qw, qh) = (q.pos[2][0] - q.pos[0][0], q.pos[1][1] - q.pos[0][1]);
        let m = sc.glyph.metrics;
        assert!((qw / qh - m.width / m.height).abs() < 1e-4, "{qw} {qh} {m:?}");
        assert_eq!(q.color, [[255; 4]; 4]);
        // The same sprite by its tag index.
        let tag = generate(&props(&format!("<sprite={table}>"), 0.8, f.clone(), s.clone()));
        assert_eq!(tag.quads.len(), 1);
        if sc.glyph_index as usize == table {
            assert_eq!(tag.quads[0], *q);
        }
        // <sprite color=#FF000080 tint=0> keeps the sprite colour untouched by the text colour.
        let c = generate(&props(&format!("<sprite={table} color=#FF000080>"), 0.8, f.clone(), s.clone()));
        assert_eq!(c.quads[0].color[0], [255, 0, 0, 128]);
        // A missing index is not a tag: it stays as text.
        let bad = generate(&props("<sprite=9999>", 0.8, f, s));
        assert!(bad.submeshes.is_empty());
    }

    /// Wrapping plus ellipsis in a small rect.
    #[test]
    fn wrap_and_ellipsis() {
        let Some((f, s)) = ellipsis_font() else { return };
        let mut p = props("HELLO WORLD AGAIN AND AGAIN", 0.8, f, s);
        p.rect_size = [0.5, 0.1];
        p.overflow_mode = 1;
        p.enable_word_wrapping = true;
        let m = generate(&p);
        assert!(m.is_text_truncated);
        let (lo, hi) = bounds(&m);
        assert!(lo[0] >= -0.25 - 1e-4 && hi[0] <= 0.25 + 1e-3, "{lo:?} {hi:?}");
        assert!(m.quads.len() < 24);
        p.overflow_mode = 0;
        assert!(!generate(&p).is_text_truncated);
    }

    /// Auto-size shrinks until the text fits.
    #[test]
    fn auto_size() {
        let Some((f, s)) = vcr() else { return };
        let mut p = props("HELLO WORLD", 4.0, f, s);
        p.rect_size = [1.0, 0.5];
        p.enable_auto_sizing = true;
        p.enable_word_wrapping = true;
        p.font_size_min = 0.5;
        p.font_size_max = 4.0;
        let m = generate(&p);
        assert!(m.font_size < 4.0 && m.font_size >= 0.5, "{}", m.font_size);
        let (lo, hi) = bounds(&m);
        assert!(lo[0] >= -0.5 - 1e-3 && hi[0] <= 0.5 + 1e-3 && hi[1] - lo[1] <= 0.5 + 1e-3, "{lo:?} {hi:?} {}", m.font_size);
    }

    /// The tags the shipped texts use beyond colour/size/b/i: <alpha=#xx>, <u>, <line-height>, and a vertex gradient.
    #[test]
    fn alpha_underline_line_height_gradient() {
        let Some((f, s)) = vcr() else { return };
        let plain = generate(&props("AB", 0.8, f.clone(), s.clone()));
        let a = generate(&props("<alpha=#5A>AB", 0.8, f.clone(), s.clone()));
        assert_eq!(a.quads.len(), plain.quads.len());
        assert!(a.quads.iter().all(|q| q.color.iter().all(|c| c[3] == 0x5a)), "{:?}", a.quads[0].color);
        let u = generate(&props("<u>AB</u>", 0.8, f.clone(), s.clone()));
        assert!(u.quads.len() > plain.quads.len(), "underline quads: {} vs {}", u.quads.len(), plain.quads.len());
        let one = generate(&props("A
B", 0.8, f.clone(), s.clone()));
        let two = generate(&props("<line-height=2>A
B", 0.8, f.clone(), s.clone()));
        let dy = |m: &TextMesh| m.quads[0].pos[0][1] - m.quads[1].pos[0][1];
        assert!((dy(&two) - 0.2).abs() < 1e-5 && (dy(&one) - 0.2).abs() > 1e-4, "{} {}", dy(&one), dy(&two));
        let mut g = props("AB", 0.8, f, s);
        g.enable_vertex_gradient = true;
        g.font_color_gradient = [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0, 0.5]];
        let m = generate(&g);
        assert_eq!(m.quads[0].color[0], [255, 0, 0, 255]);
        assert_eq!(m.quads[0].color[3][3], 128);
    }

    /// <mspace=0.057>: glyph centres step by the monospace plus the font's spacing offset.
    #[test]
    fn mspace() {
        let Some((f, s)) = vcr() else { return };
        let m = generate(&props("<mspace=0.057>ABCD", 0.8, f.clone(), s));
        assert_eq!(m.quads.len(), 4);
        let step = 0.057f32 * 0.1 + f.normal_spacing_offset * (0.8 * 0.01 * 0.1);
        for w in m.quads.windows(2) {
            let c = |q: &Quad| (q.pos[0][0] + q.pos[2][0]) / 2.0;
            assert!((c(&w[1]) - c(&w[0]) - step).abs() < 1e-4, "{} vs {step}", c(&w[1]) - c(&w[0]));
        }
    }
}
