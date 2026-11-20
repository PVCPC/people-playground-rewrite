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
    // The commit for crash reports ("unknown" for a download without git).
    let commit = std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    println!("cargo:rustc-env=PPG_COMMIT={}", commit.unwrap_or_else(|| "unknown".into()));
    // Only paths that exist (a missing one would rerun this script, and rebuild the game, on every build).
    for p in [".git/HEAD", ".git/refs/heads"].into_iter().filter(|p| std::path::Path::new(p).exists()) {
        println!("cargo:rerun-if-changed={p}");
    }
}
