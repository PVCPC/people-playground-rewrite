/* A thin C face over FreeType 2.8.0 for src/tmp/ft.rs: the calls TextCore::FontEngine makes. */
#include <ft2build.h>
#include FT_FREETYPE_H
#include FT_SIZES_H
#include <stdlib.h>

typedef struct {
    FT_Library lib;
    FT_Face face;
} Uft;

typedef struct {
    int width, height, hori_bearing_x, hori_bearing_y, hori_advance;
    int rows, bitmap_width, pitch;
    const unsigned char* buffer;
} UftGlyph;

/* FontEngine::LoadFontFace(Font*, pointSize, faceIndex): the face from the font's bytes, a charmap when none is chosen,
 * then SetPixelSize. Returns 0 and fills `out`, or the FreeType error. */
int uft_open(const unsigned char* data, long len, int face_index, int size, Uft** out, int* num_glyphs) {
    Uft* u = (Uft*)calloc(1, sizeof(Uft));
    FT_Error err = FT_Init_FreeType(&u->lib);
    if (!err) err = FT_New_Memory_Face(u->lib, data, len, face_index, &u->face);
    if (err) { if (u->lib) FT_Done_FreeType(u->lib); free(u); return err; }
    if (!u->face->charmap && u->face->num_charmaps > 0) FT_Set_Charmap(u->face, u->face->charmaps[0]);
    if (u->face->face_flags & FT_FACE_FLAG_SCALABLE) {
        err = FT_Set_Pixel_Sizes(u->face, 0, size);
    } else {
        int best = 0, best_d = 0x7fffffff;
        for (int i = 0; i < u->face->num_fixed_sizes; i++) {
            int d = abs((int)(u->face->available_sizes[i].x_ppem >> 6) - size);
            if (best_d < d) break;
            best = i; best_d = d;
        }
        err = FT_Select_Size(u->face, best);
    }
    if (err) { FT_Done_FreeType(u->lib); free(u); return err; }
    *out = u;
    *num_glyphs = (int)u->face->num_glyphs;
    return 0;
}

void uft_close(Uft* u) {
    FT_Done_Face(u->face);
    FT_Done_FreeType(u->lib);
    free(u);
}

int uft_x_ppem(Uft* u) { return u->face->size->metrics.x_ppem; }
int uft_has_kerning(Uft* u) { return FT_HAS_KERNING(u->face) ? 1 : 0; }
int uft_num_glyphs(Uft* u) { return (int)u->face->num_glyphs; }

/* FontEngine::GetGlyphIndex: the charmap's index, 0 past num_glyphs. */
unsigned uft_glyph_index(Uft* u, unsigned unicode) { return FT_Get_Char_Index(u->face, unicode); }

int uft_load_glyph(Uft* u, unsigned index, int flags, UftGlyph* g) {
    FT_Error err = FT_Load_Glyph(u->face, index, flags);
    FT_GlyphSlot s = u->face->glyph;
    g->width = (int)s->metrics.width;
    g->height = (int)s->metrics.height;
    g->hori_bearing_x = (int)s->metrics.horiBearingX;
    g->hori_bearing_y = (int)s->metrics.horiBearingY;
    g->hori_advance = (int)s->metrics.horiAdvance;
    g->rows = (int)s->bitmap.rows;
    g->bitmap_width = (int)s->bitmap.width;
    g->pitch = s->bitmap.pitch;
    g->buffer = s->bitmap.buffer;
    return err;
}

int uft_kerning(Uft* u, unsigned left, unsigned right, unsigned mode, long* x, long* y) {
    FT_Vector v = {0, 0};
    FT_Error err = FT_Get_Kerning(u->face, left, right, mode, &v);
    *x = v.x; *y = v.y;
    return err;
}
