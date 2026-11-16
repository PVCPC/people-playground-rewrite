//! TMP_FontAsset's dynamic atlas population (AtlasPopulationMode.Dynamic): TryAddCharacterInternal over UnityPlayer.dll's
//! TextCore::FontEngine, written from the managed TMP_FontAsset and the native functions (addresses below) with FreeType 2.8.0
//! (ft.rs) as the glyph source.
//!
//! The render mode is SDFAA_HINTED (4169), the only one the game's fonts use: FT_Load_Glyph(FT_LOAD_RENDER) on the hinted outline, then
//! Generate_3X3AAEDT (anti-aliased Euclidean distance transform) into the atlas. Another render mode is `mod-only` (its native
//! function is not written). Native functions, all UnityPlayer.dll:
//!   TryAddGlyphToTexture 0x180c85cf0 / TryAddGlyphToTextureSingleThread 0x180c86800, TryPackGlyph 0x180c87810,
//!   GlyphPacker::Insert 0x180c81b30, FindPositionForNode_BestShortSideFit 0x180c7a2a0, SplitFreeNode 0x180c85ab0,
//!   PruneFreeList 0x180c84240, Generate_3X3AAEDT 0x180c7a670, Calculate3x3AAEDT 0x180c78e80, ApproximateEdgeDelta 0x180c78d30,
//!   ComputeEdgeGradient 0x180c79780, ResetAtlasTexture 0x180c85880, LoadFontFace(Font*, int, int) 0x180c82ae0,
//!   GetGlyphIndex 0x180c7be40, PopulatePairAdjustmentRecordMarshallingArray(uint[], int&) 0x180c83c20 (FT_Get_Kerning over every
//!   pair of the glyph index list), FreeType's module list UNITY_FT_Add_Default_Modules 0x180f04740 and version UNITY_FT_New_Library
//!   0x180f37540 (2.8.0).
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use super::ft::Face;
use super::{Character, Glyph, GlyphMetrics, GlyphValueRecord, PairAdjustment};

/// GlyphRenderMode.SDFAA_HINTED.
const SDFAA_HINTED: i32 = 4169;

/// One dynamic font asset's mutable state, shared by every clone of its FontAsset.
pub struct Dynamic {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for Dynamic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dynamic")
    }
}

struct Inner {
    font: Arc<Vec<u8>>,
    face: Option<Face>,
    /// m_FaceInfo.pointSize.
    point_size: i32,
    padding: i32,
    render_mode: i32,
    atlas_width: i32,
    atlas_height: i32,
    /// The atlas Texture2D is readable (TryAddCharacterInternal refuses a texture that is not).
    readable: bool,
    multi_atlas: bool,
    features_at_runtime: bool,
    /// The atlas Texture2D's Alpha8 data, row 0 at the bottom (Unity's order); empty while the texture is 0x0.
    pixels: Vec<u8>,
    tex_size: (i32, i32),
    /// m_FreeGlyphRects / m_UsedGlyphRects: x, y, width, height.
    free: Vec<[i32; 4]>,
    used: Vec<[i32; 4]>,
    /// The glyphs and characters added at run time (the asset's serialized ones stay in the FontAsset).
    glyphs: HashMap<u32, Glyph>,
    characters: HashMap<u32, Character>,
    /// m_GlyphIndexList: every glyph index of the asset, the serialized ones first.
    glyph_index_list: Vec<u32>,
    /// m_MissingUnicodesFromFontFile.
    missing: HashSet<u32>,
    pairs: HashMap<u32, PairAdjustment>,
    /// RegisterFontAssetForFontFeatureUpdate has queued this asset.
    features_queued: bool,
    /// Bumped whenever the atlas pixels change (Texture2D.Apply).
    version: u64,
}

/// The font asset's serialized state a Dynamic starts from.
pub struct Start {
    pub font: Arc<Vec<u8>>,
    pub point_size: i32,
    pub padding: i32,
    pub render_mode: i32,
    pub atlas_width: i32,
    pub atlas_height: i32,
    pub readable: bool,
    pub multi_atlas: bool,
    pub features_at_runtime: bool,
    pub free: Vec<[i32; 4]>,
    pub used: Vec<[i32; 4]>,
    pub glyph_indexes: Vec<u32>,
}

/// The atlas as the renderer samples it.
pub struct AtlasImage {
    pub width: i32,
    pub height: i32,
    /// Alpha8, bottom row first.
    pub pixels: Vec<u8>,
    pub version: u64,
}

/// TMP_FontAsset.k_FontAssets_FontFeaturesUpdateQueue.
static FEATURE_QUEUE: Mutex<Vec<Arc<Dynamic>>> = Mutex::new(Vec::new());

/// TMP_FontAsset.UpdateFontFeaturesForFontAssetsInQueue: TextMeshPro.OnPreRenderObject calls it after ParseInputText.
pub fn update_font_features_in_queue() {
    let queue = std::mem::take(&mut *FEATURE_QUEUE.lock().unwrap());
    for d in queue {
        d.update_glyph_adjustment_records();
    }
}

/// Font.m_FontData of a Font object (class 128, Unity 2020.3): the array of bytes holding the TrueType file. The object's fields before
/// it are fixed-size except the character rect and kerning arrays, so the array is found as the length-prefixed block that starts with an sfnt
/// header whose table directory fits inside it.
pub fn font_data(object: &[u8]) -> Result<Vec<u8>, String> {
    let u32_at = |p: usize| object.get(p..p + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    for p in (0..object.len().saturating_sub(12)).step_by(4) {
        let Some(len) = u32_at(p).map(|l| l as usize) else { continue };
        let start = p + 4;
        if len < 12 || start + len > object.len() || !matches!(&object[start..start + 4], [0, 1, 0, 0] | b"true" | b"OTTO" | b"ttcf") {
            continue;
        }
        let tables = u16::from_be_bytes([object[start + 4], object[start + 5]]) as usize;
        if tables == 0 || 12 + tables * 16 > len {
            continue;
        }
        let fits = (0..tables).all(|t| {
            let e = start + 12 + t * 16;
            let off = u32::from_be_bytes(object[e + 8..e + 12].try_into().unwrap()) as usize;
            let size = u32::from_be_bytes(object[e + 12..e + 16].try_into().unwrap()) as usize;
            off.checked_add(size).is_some_and(|end| end <= len)
        });
        if fits {
            return Ok(object[start..start + len].to_vec());
        }
    }
    Err("Font.m_FontData".into())
}

impl Dynamic {
    pub fn new(s: Start) -> Arc<Dynamic> {
        if s.render_mode != SDFAA_HINTED {
            unimplemented!("mod-only: TMP dynamic atlas render mode {} (only SDFAA_HINTED's TextCore::FontEngine path is written)", s.render_mode);
        }
        Arc::new(Dynamic {
            inner: Mutex::new(Inner {
                font: s.font,
                face: None,
                point_size: s.point_size,
                padding: s.padding,
                render_mode: s.render_mode,
                atlas_width: s.atlas_width,
                atlas_height: s.atlas_height,
                readable: s.readable,
                multi_atlas: s.multi_atlas,
                features_at_runtime: s.features_at_runtime,
                pixels: Vec::new(),
                tex_size: (0, 0),
                free: s.free,
                used: s.used,
                glyphs: HashMap::new(),
                characters: HashMap::new(),
                glyph_index_list: s.glyph_indexes,
                missing: HashSet::new(),
                pairs: HashMap::new(),
                features_queued: false,
                version: 0,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap()
    }

    /// A character this asset added earlier (characterLookupTable), with its glyph.
    pub fn character(&self, unicode: u32) -> Option<(Character, Glyph)> {
        let g = self.lock();
        let c = *g.characters.get(&unicode)?;
        Some((c, g.glyphs[&c.glyph_index]))
    }

    /// A glyph this asset added (glyphLookupTable).
    pub fn glyph(&self, index: u32) -> Option<Glyph> {
        self.lock().glyphs.get(&index).copied()
    }

    /// The glyph pair adjustment UpdateGlyphAdjustmentRecords added for `key` (second << 16 | first).
    pub fn pair(&self, key: u32) -> Option<PairAdjustment> {
        self.lock().pairs.get(&key).copied()
    }

    /// The atlas texture, or None while it is still 0x0.
    pub fn atlas(&self) -> Option<AtlasImage> {
        let g = self.lock();
        (g.tex_size.0 > 0).then(|| AtlasImage { width: g.tex_size.0, height: g.tex_size.1, pixels: g.pixels.clone(), version: g.version })
    }

    pub fn atlas_version(&self) -> u64 {
        self.lock().version
    }

    /// TMP_FontAsset.AddSynthesizedCharactersAndFaceMetrics' face check: the font's glyph index for `unicode` (0 when the face does not
    /// load).
    pub fn glyph_index(&self, unicode: u32) -> u32 {
        let mut g = self.lock();
        if g.load_face() { g.face.as_ref().unwrap().glyph_index(unicode) } else { 0 }
    }

    /// TMP_FontAsset.TryAddCharacterInternal: the character and its glyph, added to the atlas when it is new. `serialized` finds a glyph
    /// of the asset's serialized glyph table by index (m_GlyphLookupDictionary).
    pub fn try_add_character(self: &Arc<Self>, unicode: u32, serialized: &dyn Fn(u32) -> Option<Glyph>) -> Option<(Character, Glyph)> {
        let mut g = self.lock();
        if g.missing.contains(&unicode) {
            return None;
        }
        if !g.load_face() {
            return None;
        }
        let face = g.face.as_ref().unwrap();
        let mut glyph_index = face.glyph_index(unicode);
        if glyph_index == 0 {
            glyph_index = match unicode {
                160 => face.glyph_index(32),
                173 | 8209 => face.glyph_index(45),
                _ => 0,
            };
            if glyph_index == 0 {
                g.missing.insert(unicode);
                return None;
            }
        }
        if let Some(glyph) = g.glyphs.get(&glyph_index).copied().or_else(|| serialized(glyph_index)) {
            let c = Character { unicode, glyph_index, scale: 1.0, synth: false };
            g.characters.insert(unicode, c);
            return Some((c, glyph));
        }
        if !g.readable {
            return None;
        }
        if g.tex_size.0 == 0 || g.tex_size.1 == 0 {
            // Texture2D.Resize(m_AtlasWidth, m_AtlasHeight) then FontEngine.ResetAtlasTexture: Alpha8, all zero.
            g.tex_size = (g.atlas_width, g.atlas_height);
            g.pixels = vec![0; (g.atlas_width * g.atlas_height) as usize];
        }
        let glyph = match g.try_add_glyph_to_texture(glyph_index) {
            Some(glyph) => glyph,
            None => {
                if g.multi_atlas {
                    unimplemented!("mod-only: TMP dynamic atlas with m_IsMultiAtlasTexturesEnabled (SetupNewAtlasTexture's second texture)");
                }
                return None;
            }
        };
        g.glyphs.insert(glyph_index, glyph);
        let c = Character { unicode, glyph_index, scale: 1.0, synth: false };
        g.characters.insert(unicode, c);
        g.glyph_index_list.push(glyph_index);
        if g.features_at_runtime && !g.features_queued {
            g.features_queued = true;
            drop(g);
            FEATURE_QUEUE.lock().unwrap().push(self.clone());
        }
        Some((c, glyph))
    }

    /// TMP_FontAsset.UpdateGlyphAdjustmentRecords(): the kerning of every pair of the glyph index list not in the table yet.
    fn update_glyph_adjustment_records(&self) {
        let mut g = self.lock();
        g.features_queued = false;
        if !g.load_face() {
            return;
        }
        let face = g.face.as_ref().unwrap();
        // PopulatePairAdjustmentRecordMarshallingArray (0x180c83c20): not a kerning face, no records.
        if !face.has_kerning() {
            return;
        }
        let ppem = face.x_ppem();
        let list = g.glyph_index_list.clone();
        let mut new = Vec::new();
        for &first in &list {
            if first == 0 {
                break;
            }
            for &second in &list {
                if second == 0 {
                    break;
                }
                let (x, y) = face.kerning(first, second);
                if x != 0 || y != 0 {
                    // (float)(ppem / size): an integer division.
                    let scale = (ppem / g.point_size) as f32;
                    let rec = PairAdjustment {
                        first: GlyphValueRecord { x_placement: 0.0, y_placement: 0.0, x_advance: (x as f32 * 0.015625) / scale, y_advance: (y as f32 * 0.015625) / scale },
                        second: GlyphValueRecord::default(),
                        flags: 0,
                    };
                    new.push(((second << 16) | first, rec));
                }
            }
        }
        for (key, rec) in new {
            g.pairs.entry(key).or_insert(rec);
        }
    }
}

impl Inner {
    /// FontEngine.LoadFontFace(sourceFontFile, pointSize).
    fn load_face(&mut self) -> bool {
        if self.face.is_none() {
            match Face::open(self.font.clone(), self.point_size) {
                Ok(f) => self.face = Some(f),
                Err(_) => return false,
            }
        }
        true
    }

    /// TextCore::FontEngine::TryAddGlyphToTexture for SDFAA_HINTED (0x180c85cf0 -> TryAddGlyphToTextureSingleThread 0x180c86800):
    /// load the glyph hinted and rendered, pack its rect, run Generate_3X3AAEDT into the atlas.
    fn try_add_glyph_to_texture(&mut self, glyph_index: u32) -> Option<Glyph> {
        debug_assert_eq!(self.render_mode, SDFAA_HINTED);
        let padding = self.padding;
        let face = self.face.as_ref().unwrap();
        // FT_LOAD_RENDER.
        let (_err, slot) = face.load_glyph(glyph_index, 4);
        let scale = 1; // iVar4: the SDF oversampling, 1 for SDFAA.
        let unit = (scale << 6) as f32;
        let (bw, bh) = (slot.bitmap_width, slot.rows);
        let mut glyph = Glyph {
            index: glyph_index,
            metrics: GlyphMetrics {
                width: slot.width as f32 / unit,
                height: slot.height as f32 / unit,
                horizontal_bearing_x: slot.hori_bearing_x as f32 / unit,
                horizontal_bearing_y: slot.hori_bearing_y as f32 / unit,
                horizontal_advance: slot.hori_advance as f32 / unit,
            },
            rect: [0, 0, bw, bh],
            scale: 1.0,
            atlas_index: 0,
        };
        if bw == 0 || bh == 0 {
            // An empty glyph (a space): nothing to pack, the glyph keeps its zero rect.
            return Some(glyph);
        }
        let (tex_w, tex_h) = self.tex_size;
        let packed = pack_glyph(&mut glyph, padding, tex_w, tex_h, &mut self.free, &mut self.used);
        if !packed {
            return None;
        }
        // Generate_3X3AAEDT(texture + ((padding - 1 + y + h) * texW - padding + x), texW, w, h, bitmap, w, h, scale, padding): the rows
        // go down the texture from the glyph rect's top padding row.
        let origin = (padding - 1 + glyph.rect[1] + bh) * tex_w - padding + glyph.rect[0];
        generate_3x3_aaedt(&mut self.pixels, origin, tex_w, &slot.bitmap, bw, bh, padding);
        self.version += 1;
        Some(glyph)
    }
}

// ---- GlyphPacker (BestShortSideFit) ----

fn is_contained_in(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] >= b[0] && a[1] >= b[1] && a[0] + a[2] <= b[0] + b[2] && a[1] + a[3] <= b[1] + b[3]
}

/// FontEngine::TryPackGlyph (0x180c87810) with GlyphPacker::Insert (0x180c81b30): the rect of the glyph's bitmap plus its padding goes in the
/// free rect with the best short side fit; the glyph rect is that node moved by the padding.
fn pack_glyph(glyph: &mut Glyph, padding: i32, _tex_w: i32, _tex_h: i32, free: &mut Vec<[i32; 4]>, used: &mut Vec<[i32; 4]>) -> bool {
    // m_RenderModePaddingModifier = ~(renderMode >> 4) & 1: 1 for SDFAA_HINTED (bit 4 clear).
    let modifier = !(SDFAA_HINTED >> 4) & 1;
    let node_padding = modifier + padding;
    let w = glyph.rect[2] + modifier + padding * 2;
    let h = glyph.rect[3] + modifier + padding * 2;
    // FindPositionForNode_BestShortSideFit (0x180c7a2a0); rotations are off (m_AllowRotations = false).
    let mut best = [0i32; 4];
    let mut best_short = i32::MAX;
    let best_long = 0; // Insert passes a zeroed bestLongSideFit, so a tie on the short side never replaces.
    for r in free.iter() {
        if w <= r[2] && h <= r[3] {
            let (dw, dh) = ((r[2] - w).abs(), (r[3] - h).abs());
            let (short, long) = (dw.min(dh), dw.max(dh));
            if short < best_short || (short == best_short && long < best_long) {
                best = [r[0], r[1], w, h];
                best_short = short;
            }
        }
    }
    if best[3] == 0 {
        return false;
    }
    // Split every free rect the node overlaps (SplitFreeNode 0x180c85ab0), the new pieces appended and visited by the same loop.
    let mut i = 0;
    while i < free.len() {
        let r = free[i];
        if split_free_node(r, best, free) {
            free.remove(i);
        } else {
            i += 1;
        }
    }
    glyph.rect[0] = best[0] + node_padding;
    glyph.rect[1] = best[1] + node_padding;
    prune_free_list(free);
    used.push(best);
    true
}

/// GlyphPacker::SplitFreeNode: the pieces of `free_node` the used node leaves, pushed on `free`; false when they do not overlap.
fn split_free_node(f: [i32; 4], u: [i32; 4], free: &mut Vec<[i32; 4]>) -> bool {
    let (fx2, fy2) = (f[0] + f[2], f[1] + f[3]);
    if !(u[0] < fx2 && f[0] < u[0] + u[2]) || !(u[1] < fy2 && f[1] < u[1] + u[3]) {
        return false;
    }
    if f[1] < u[1] {
        free.push([f[0], f[1], f[2], u[1] - f[1]]);
    }
    if u[1] + u[3] < fy2 {
        free.push([f[0], u[1] + u[3], f[2], f[3] - (u[1] + u[3] - f[1])]);
    }
    if u[1] < fy2 && f[1] < u[1] + u[3] {
        if f[0] < u[0] && u[0] < fx2 {
            free.push([f[0], f[1], u[0] - f[0], f[3]]);
        }
        if u[0] + u[2] < fx2 {
            free.push([u[0] + u[2], f[1], f[0] + f[2] - (u[0] + u[2]), f[3]]);
        }
    }
    true
}

/// GlyphPacker::PruneFreeList: drop every free rect contained in another.
fn prune_free_list(free: &mut Vec<[i32; 4]>) {
    let mut i = 0;
    while i < free.len() {
        let mut j = i + 1;
        let mut erased_i = false;
        while j < free.len() {
            if is_contained_in(free[i], free[j]) {
                free.remove(i);
                erased_i = true;
                break;
            }
            if is_contained_in(free[j], free[i]) {
                free.remove(j);
                continue;
            }
            j += 1;
        }
        if !erased_i {
            i += 1;
        }
    }
}

// ---- Generate_3X3AAEDT ----

#[derive(Clone, Copy, Default)]
struct Pixel {
    alpha: f32,
    distance: f32,
    distance_sq: f32,
    delta: f32,
    gx: f32,
    gy: f32,
    dx: i32,
    dy: i32,
}

/// TextCore::ApproximateEdgeDelta (0x180c78d30).
fn approximate_edge_delta(x: f32, y: f32, a: f32) -> f32 {
    if x == 0.0 || y == 0.0 {
        return 0.5 - a;
    }
    let len = (x * x + y * y).sqrt();
    let (ax, ay) = ((x / len).abs(), (y / len).abs());
    let (big, small) = if ax < ay { (ay, ax) } else { (ax, ay) };
    let a2 = (small * 0.5) / big;
    if a2 <= a {
        if 1.0 - a2 <= a {
            ((big + big) * small * (1.0 - a)).sqrt() - (big + small) * 0.5
        } else {
            (0.5 - a) * big
        }
    } else {
        (big + small) * 0.5 - ((big + big) * small * a).sqrt()
    }
}

/// TextCore::ComputeEdgeGradient (0x180c79780): the gradient of the source around linear index `i` (reads outside [0, total) are 0).
fn compute_edge_gradient(px: &mut Pixel, src: &[u8], i: i32, w: i32, total: i32) {
    let sqrt2 = f32::from_bits(0x3FB5_04F3);
    let s = |k: i32| if k < 0 || k >= total { 0.0f32 } else { src[k as usize] as f32 };
    let up = i + w;
    let down = i - w;
    // fVar10 and fVar7 as the native adds them, in order.
    let mut gy = s(up - 1) + 0.0;
    gy = gy + s(up) * sqrt2 + s(up + 1);
    let left = s(i - 1) * sqrt2;
    let right = s(i + 1) * sqrt2;
    let mut gx = (((((0.0 - s(up - 1)) + s(up + 1)) - left) + right) - s(down - 1)) + s(down + 1);
    gy = gy - s(down - 1);
    gy = (gy - s(down) * sqrt2) - s(down + 1);
    let len = (gy * gy + gx * gx).sqrt();
    if 0.0 < len {
        px.gx = gx / len;
        px.gy = gy / len;
    }
    let _ = &mut gx;
}

/// TextCore::Calculate3x3AAEDT (0x180c78e80): the two passes over the padded glyph; `invert` is the job's mask flag (the distance to the
/// outside instead of the inside).
fn calculate_3x3_aaedt(src: &[u8], src_w: i32, src_h: i32, spread: i32, invert: bool, px: &mut [Pixel]) {
    let w = spread * 2 + src_w;
    let h = spread * 2 + src_h;
    let max = (w * w + 1 + h * h) as f32;
    let at = |x: i32, y: i32| (y * w + x) as usize;
    for y in 0..h {
        for x in 0..w {
            let idx = at(x, y);
            if x >= spread && y >= spread && x < w - spread && y < h - spread {
                px[idx].alpha = src[((y - spread) * src_w + (x - spread)) as usize] as f32 / 255.0;
            }
            let mut a = px[idx].alpha;
            if invert {
                a = 1.0 - a;
            }
            px[idx].alpha = a;
            let (mut dx, mut dy) = (0i32, 0i32);
            if a == 0.0 {
                let (mut best_sq, mut best_total, mut best_delta) = (max, max, 0.0f32);
                let mut consider = |px: &[Pixel], n: usize, ndx: i32, ndy: i32, strict_max: bool| {
                    let _ = strict_max;
                    let delta = approximate_edge_delta(ndx as f32, ndy as f32, px[at(x + ndx, y + ndy)].alpha);
                    let sq = (ndy * ndy + ndx * ndx) as f32;
                    let total = sq + delta;
                    let _ = n;
                    if total < best_total {
                        best_sq = sq;
                        best_total = total;
                        dx = ndx;
                        dy = ndy;
                        best_delta = delta;
                    }
                };
                if y != 0 && px[idx - w as usize].distance != max {
                    let n = px[idx - w as usize];
                    consider(px, 0, n.dx, n.dy - 1, true);
                }
                if x != 0 {
                    if px[idx - 1].distance != max {
                        let n = px[idx - 1];
                        consider(px, 0, n.dx - 1, n.dy, false);
                    }
                    if y != 0 && px[idx - w as usize - 1].distance != max {
                        let n = px[idx - w as usize - 1];
                        consider(px, 0, n.dx - 1, n.dy - 1, false);
                    }
                }
                if x < w - 1 && y != 0 && px[idx - w as usize + 1].distance != max {
                    let n = px[idx - w as usize + 1];
                    consider(px, 0, n.dx + 1, n.dy - 1, false);
                }
                px[idx].distance = best_total;
                px[idx].delta = best_delta;
                px[idx].distance_sq = best_sq;
            } else {
                if 1.0 <= a {
                    px[idx].distance = 0.0;
                    px[idx].delta = 0.0;
                } else {
                    compute_edge_gradient(&mut px[idx], src, (y - spread) * src_w + (x - spread), src_w, src_w * src_h);
                    let d = approximate_edge_delta(px[idx].gx, px[idx].gy, px[idx].alpha);
                    px[idx].distance = d;
                    px[idx].delta = d;
                }
                px[idx].distance_sq = 0.0;
            }
            px[idx].dx = dx;
            px[idx].dy = dy;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let idx = at(x, y);
            let p = px[idx];
            if !((p.alpha <= 0.0 || 1.0 <= p.alpha) && p.distance != 0.0) {
                continue;
            }
            let (mut dist, mut sq, mut delta, mut dx, mut dy) = (p.distance, p.distance_sq, p.delta, p.dx, p.dy);
            let mut consider = |px: &[Pixel], ndx: i32, ndy: i32, or_equal: bool| {
                let nd = approximate_edge_delta(ndx as f32, ndy as f32, px[at(x + ndx, y + ndy)].alpha);
                let nsq = (ndy * ndy + ndx * ndx) as f32;
                let total = nsq + nd;
                if if or_equal { total <= dist } else { total < dist } {
                    dist = total;
                    sq = nsq;
                    dx = ndx;
                    dy = ndy;
                    delta = nd;
                }
            };
            let below = y < h - 1;
            if below && px[at(x, y + 1)].distance != max {
                let n = px[at(x, y + 1)];
                consider(px, n.dx, n.dy + 1, true);
            }
            if x < w - 1 {
                if px[idx + 1].distance != max {
                    let n = px[idx + 1];
                    consider(px, n.dx + 1, n.dy, false);
                }
                if below && px[at(x + 1, y + 1)].distance != max {
                    let n = px[at(x + 1, y + 1)];
                    consider(px, n.dx + 1, n.dy + 1, false);
                }
            }
            if x != 0 && below && px[at(x - 1, y + 1)].distance != max {
                let n = px[at(x - 1, y + 1)];
                consider(px, n.dx - 1, n.dy + 1, false);
            }
            px[idx].distance = dist;
            px[idx].distance_sq = sq;
            px[idx].delta = delta;
            px[idx].dx = dx;
            px[idx].dy = dy;
        }
    }
}

/// TextCore::Generate_3X3AAEDT (0x180c7a670): both passes (the glyph's inside and its outside), then the bytes
/// `127.5 +- clamp((sqrt(distance squared) + delta) * 255 / (2 * spread + 2), 0, 127.5)` rounded down after +0.5, written row after row
/// down the texture from `origin` (the byte offset of the first row's first pixel).
fn generate_3x3_aaedt(texture: &mut [u8], origin: i32, stride: i32, src: &[u8], src_w: i32, src_h: i32, spread: i32) {
    let w = spread * 2 + src_w;
    let h = spread * 2 + src_h;
    let n = (w * h) as usize;
    let mut inside = vec![Pixel::default(); n];
    let mut outside = vec![Pixel::default(); n];
    calculate_3x3_aaedt(src, src_w, src_h, spread, false, &mut inside);
    calculate_3x3_aaedt(src, src_w, src_h, spread, true, &mut outside);
    let scale = 255.0 / (spread * 2 + 2) as f32;
    let mut row_start = origin as i64;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let v = if inside[i].distance <= outside[i].distance {
                let mut d = (outside[i].distance_sq.sqrt() + outside[i].delta) * scale;
                if 127.5 <= d {
                    d = 127.5;
                }
                if d <= 0.0 {
                    d = 0.0;
                }
                d + 127.5
            } else {
                let mut d = (inside[i].distance_sq.sqrt() + inside[i].delta) * scale;
                if 127.5 <= d {
                    d = 127.5;
                }
                if d <= 0.0 {
                    d = 0.0;
                }
                127.5 - d
            };
            texture[(row_start + x as i64) as usize] = (v + 0.5) as i32 as u8;
        }
        row_start -= stride as i64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::db::{AssetDb, ObjRef};
    use crate::tmp::{FontAsset, TmpSettings};

    /// "LiberationSans SDF - Fallback" (file 5, path 1678): a dynamic asset, empty tables, a 0x0 readable atlas.
    fn fallback() -> Option<(Arc<FontAsset>, TmpSettings)> {
        let game = crate::game_assets::find_game(None).ok()?;
        let mut db = AssetDb::open(&game).unwrap();
        let mut cache = HashMap::new();
        let f = FontAsset::read(&mut db, ObjRef { file: 5, path_id: 1678 }, &mut cache).unwrap();
        let s = TmpSettings::read(&mut db, &mut cache).unwrap();
        Some((f, s))
    }

    #[test]
    fn cyrillic_and_greek_land_in_the_atlas() {
        let Some((f, _)) = fallback() else { return };
        let d = f.state.clone().expect("dynamic");
        assert!(d.atlas().is_none(), "the atlas starts 0x0");
        let face = Face::open(Arc::new(font_of(&f)), 86).unwrap();
        let mut rects = Vec::new();
        for ch in "ПриветΩμέγα".chars() {
            let (c, g) = d.try_add_character(ch as u32, &|_| None).unwrap_or_else(|| panic!("{ch}"));
            assert_eq!(c.glyph_index, face.glyph_index(ch as u32));
            let (_, slot) = face.load_glyph(c.glyph_index, 4);
            assert_eq!(g.metrics.width, slot.width as f32 / 64.0);
            assert_eq!(g.metrics.horizontal_advance, slot.hori_advance as f32 / 64.0);
            assert_eq!(g.rect[2], slot.bitmap_width);
            assert_eq!(g.rect[3], slot.rows);
            rects.push(g.rect);
        }
        // No two glyph rects (with their padding) overlap.
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let apart = a[0] + a[2] + 9 <= b[0] - 9 || b[0] + b[2] + 9 <= a[0] - 9 || a[1] + a[3] + 9 <= b[1] - 9 || b[1] + b[3] + 9 <= a[1] - 9;
                assert!(apart || a == b, "{a:?} {b:?}");
            }
        }
        let atlas = d.atlas().unwrap();
        assert_eq!((atlas.width, atlas.height), (512, 512));
        // The distance field: inside a stem above the 127.5 edge, the padding's far corner at 0.
        let (_, g) = d.try_add_character('П' as u32, &|_| None).unwrap();
        let px = |x: i32, y: i32| atlas.pixels[(y * 512 + x) as usize];
        assert!(px(g.rect[0] + 3, g.rect[1] + g.rect[3] / 2) > 150, "inside the stem (4.5 px from the edge: 127.5 + 4.5 * 255 / 20)");
        assert!(px(g.rect[0] - 9, g.rect[1] - 9) < 20, "the padding's far corner");
    }

    fn font_of(f: &FontAsset) -> Vec<u8> {
        let game = crate::game_assets::find_game(None).unwrap();
        let db = AssetDb::open(&game).unwrap();
        let _ = f;
        font_data(db.data(ObjRef { file: 5, path_id: 231 })).unwrap()
    }

    #[test]
    fn missing_character_lands_on_u25a1() {
        let Some((f, s)) = fallback() else { return };
        let d = f.state.clone().unwrap();
        // The default sprite asset owns the common emoji (a sprite is drawn instead, a separate path); pick an emoji code point it lacks.
        let emoji = (0x1F900u32..0x1FA00).find(|u| !s.default_sprite.as_ref().is_some_and(|a| a.by_unicode.contains_key(u)) && d.glyph_index(*u) == 0).unwrap();
        assert!(d.try_add_character(emoji, &|_| None).is_none());
        assert!(d.try_add_character(emoji, &|_| None).is_none(), "the second lookup is the missing set's");
        // TMP_Text.ParseInputText: a character no asset has becomes the missing glyph (U+25A1 when m_missingGlyphCharacter is 0).
        let text = char::from_u32(emoji).unwrap().to_string();
        let props = |t: &str| crate::tmp::TextProps { text: t.into(), font: f.clone(), font_size: 1.0, font_size_base: 1.0, font_size_max: 1.0, font_size_min: 0.1, rect_size: [20.0, 5.0], pivot: [0.5, 0.5], lossy_scale_y: 1.0, enable_kerning: true, settings: s.clone(), ..Default::default() };
        let m = crate::tmp::layout::generate(&props(&text));
        let square = crate::tmp::layout::generate(&props("\u{25A1}"));
        assert_eq!(m.quads.len(), 1);
        assert_eq!(m.quads[0].pos, square.quads[0].pos);
        assert_eq!(m.quads[0].uv, square.quads[0].uv);
    }

    #[test]
    fn typed_text_lays_out_through_the_dynamic_atlas() {
        let Some((f, s)) = fallback() else { return };
        let p = crate::tmp::TextProps { text: "Привет Ωμέγα".into(), font: f.clone(), font_size: 1.0, font_size_base: 1.0, font_size_max: 1.0, font_size_min: 0.1, rect_size: [20.0, 5.0], pivot: [0.5, 0.5], lossy_scale_y: 1.0, enable_kerning: true, settings: s, ..Default::default() };
        let m = crate::tmp::layout::generate(&p);
        assert_eq!(m.quads.len(), 11);
        let d = f.state.as_ref().unwrap();
        assert!(d.atlas_version() >= 11);
        // UpdateGlyphAdjustmentRecords ran (the font's legacy kern table gives the pairs; none when it has none).
        let g = |c: char| d.character(c as u32).unwrap().0.glyph_index;
        let face = Face::open(Arc::new(font_of(&f)), 86).unwrap();
        let (kx, _) = face.kerning(g('Ω'), g('μ'));
        assert_eq!(d.pair((g('μ') << 16) | g('Ω')).is_some(), kx != 0);
    }

    /// Writes each test glyph's FreeType bitmap (width, height, rows) for tools/native_sdf.py to run UnityPlayer.dll's own
    /// Generate_3X3AAEDT on: `cargo test dump_sdf_inputs -- --ignored`.
    #[test]
    #[ignore]
    fn dump_sdf_inputs() {
        let Some((f, _)) = fallback() else { return };
        let face = Face::open(Arc::new(font_of(&f)), 86).unwrap();
        std::fs::write("scratch/sdf/font.ttf", font_of(&f)).unwrap();
        for ch in "ПриветΩμέγα\u{25A1}".chars() {
            let (_, slot) = face.load_glyph(face.glyph_index(ch as u32), 4);
            let mut out = Vec::new();
            out.extend((slot.bitmap_width).to_le_bytes());
            out.extend((slot.rows).to_le_bytes());
            out.extend(&slot.bitmap);
            std::fs::write(format!("scratch/sdf/{:04X}.in", ch as u32), out).unwrap();
        }
    }

    /// Generate_3X3AAEDT's 512x512 output for each glyph (FreeType bitmap at spread 9, glyph rect (10, 10)) as UnityPlayer.dll's own machine
    /// code made it (tools/native_sdf.py maps the DLL and calls the function), by FNV-1a 64 of the whole buffer.
    #[test]
    fn sdf_bytes_match_unityplayer() {
        let Some((f, _)) = fallback() else { return };
        let face = Face::open(Arc::new(font_of(&f)), 86).unwrap();
        let native: [(char, u64); 12] = [
            ('\u{3A9}', 0xcb175630fe15f48e), ('\u{3AD}', 0x91c7e600497d3ca4), ('\u{3B1}', 0xccdfbf2d16ba7785), ('\u{3B3}', 0x322cf1575d9ff830),
            ('\u{3BC}', 0xd04065f270904455), ('\u{41F}', 0xf07dfb2ed438ce59), ('\u{432}', 0xc6eefd224745deff), ('\u{435}', 0xf43cfe98a5c148d7),
            ('\u{438}', 0x1cb92461c55a58bf), ('\u{440}', 0xf7b7bb71ffb1031f), ('\u{442}', 0xc6866fc98de2e395), ('\u{25A1}', 0xc72962547b9beeed),
        ];
        for (ch, want) in native {
            let (_, slot) = face.load_glyph(face.glyph_index(ch as u32), 4);
            let mut tex = vec![0u8; 512 * 512];
            let origin = (9 - 1 + 10 + slot.rows) * 512 - 9 + 10;
            generate_3x3_aaedt(&mut tex, origin, 512, &slot.bitmap, slot.bitmap_width, slot.rows, 9);
            let got = tex.iter().fold(0xcbf29ce484222325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3));
            assert_eq!(got, want, "U+{:04X}", ch as u32);
        }
    }
}
