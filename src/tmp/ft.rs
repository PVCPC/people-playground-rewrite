//! FreeType 2.8.0 (third_party/freetype, built by build.rs), the library UnityPlayer.dll's TextCore FontEngine rasterizes with:
//! one face loaded from font bytes (FontEngine::LoadFontFace(Font*, size, 0)), its glyph indexes, glyph loads and kerning.
use std::ffi::{c_int, c_long, c_uchar, c_uint, c_void};
use std::sync::Arc;

#[repr(C)]
struct RawGlyph {
    width: c_int,
    height: c_int,
    hori_bearing_x: c_int,
    hori_bearing_y: c_int,
    hori_advance: c_int,
    rows: c_int,
    bitmap_width: c_int,
    pitch: c_int,
    buffer: *const c_uchar,
}

unsafe extern "C" {
    fn uft_open(data: *const c_uchar, len: c_long, face_index: c_int, size: c_int, out: *mut *mut c_void, num_glyphs: *mut c_int) -> c_int;
    fn uft_close(face: *mut c_void);
    fn uft_x_ppem(face: *mut c_void) -> c_int;
    fn uft_has_kerning(face: *mut c_void) -> c_int;
    fn uft_glyph_index(face: *mut c_void, unicode: c_uint) -> c_uint;
    fn uft_load_glyph(face: *mut c_void, index: c_uint, flags: c_int, out: *mut RawGlyph) -> c_int;
    fn uft_kerning(face: *mut c_void, left: c_uint, right: c_uint, mode: c_uint, x: *mut c_long, y: *mut c_long) -> c_int;
}

/// FT_GlyphSlot after FT_Load_Glyph: the metrics in 26.6 units and the rendered gray bitmap (rows of `width` bytes).
pub struct LoadedGlyph {
    pub width: i32,
    pub height: i32,
    pub hori_bearing_x: i32,
    pub hori_bearing_y: i32,
    pub hori_advance: i32,
    pub rows: i32,
    pub bitmap_width: i32,
    pub bitmap: Vec<u8>,
}

pub struct Face {
    raw: *mut c_void,
    // FT_New_Memory_Face does not copy the font.
    _data: Arc<Vec<u8>>,
    pub num_glyphs: i32,
}

// The face is only used behind the owner's Mutex.
unsafe impl Send for Face {}

impl Face {
    /// FT_New_Memory_Face, a charmap when none is chosen, FT_Set_Pixel_Sizes(0, size) (FontEngine::LoadFontFace).
    pub fn open(data: Arc<Vec<u8>>, size: i32) -> Result<Face, i32> {
        let mut raw = std::ptr::null_mut();
        let mut num_glyphs = 0;
        let err = unsafe { uft_open(data.as_ptr(), data.len() as c_long, 0, size, &mut raw, &mut num_glyphs) };
        if err != 0 {
            return Err(err);
        }
        Ok(Face { raw, _data: data, num_glyphs })
    }

    pub fn x_ppem(&self) -> i32 {
        unsafe { uft_x_ppem(self.raw) }
    }

    /// FT_HAS_KERNING (face_flags & 0x40).
    pub fn has_kerning(&self) -> bool {
        unsafe { uft_has_kerning(self.raw) != 0 }
    }

    /// FontEngine::GetGlyphIndex.
    pub fn glyph_index(&self, unicode: u32) -> u32 {
        unsafe { uft_glyph_index(self.raw, unicode) }
    }

    /// FT_Load_Glyph. The bitmap is copied out of the slot.
    pub fn load_glyph(&self, index: u32, flags: i32) -> (i32, LoadedGlyph) {
        let mut g = RawGlyph { width: 0, height: 0, hori_bearing_x: 0, hori_bearing_y: 0, hori_advance: 0, rows: 0, bitmap_width: 0, pitch: 0, buffer: std::ptr::null() };
        let err = unsafe { uft_load_glyph(self.raw, index, flags, &mut g) };
        let mut bitmap = Vec::new();
        if !g.buffer.is_null() && g.rows > 0 && g.bitmap_width > 0 {
            assert_eq!(g.pitch, g.bitmap_width, "FreeType's gray bitmap pitch is its width (FontEngine reads it as rows of `width`)");
            bitmap = unsafe { std::slice::from_raw_parts(g.buffer, (g.rows * g.bitmap_width) as usize) }.to_vec();
        }
        (err, LoadedGlyph { width: g.width, height: g.height, hori_bearing_x: g.hori_bearing_x, hori_bearing_y: g.hori_bearing_y, hori_advance: g.hori_advance, rows: g.rows, bitmap_width: g.bitmap_width, bitmap })
    }

    /// FT_Get_Kerning(left, right, FT_KERNING_UNFITTED): the kerning vector in 26.6 pixels.
    pub fn kerning(&self, left: u32, right: u32) -> (i32, i32) {
        let (mut x, mut y) = (0 as c_long, 0 as c_long);
        unsafe { uft_kerning(self.raw, left, right, 1, &mut x, &mut y) };
        (x as i32, y as i32)
    }
}

impl Drop for Face {
    fn drop(&mut self) {
        unsafe { uft_close(self.raw) }
    }
}
