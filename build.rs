// FreeType 2.8.0 (third_party/freetype): the rasterizer the game's TextCore FontEngine uses for TMP's dynamic atlases.
fn main() {
    let ft = "third_party/freetype";
    let mut b = cc::Build::new();
    b.include(format!("{ft}/include")).define("FT2_BUILD_LIBRARY", None).opt_level(2).warnings(false);
    for f in [
        "src/base/ftbase.c", "src/base/ftbitmap.c", "src/base/ftdebug.c", "src/base/ftinit.c", "src/base/ftsystem.c",
        "src/sfnt/sfnt.c", "src/truetype/truetype.c", "src/psnames/psnames.c", "src/smooth/smooth.c",
    ] {
        b.file(format!("{ft}/{f}"));
    }
    b.file(format!("{ft}/shim.c"));
    b.compile("freetype");
    println!("cargo:rerun-if-changed={ft}");
    println!("cargo:rerun-if-changed=build.rs");
}
