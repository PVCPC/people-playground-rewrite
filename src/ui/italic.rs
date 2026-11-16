//! TextMesh Pro's <i>: each glyph quad's top edge shifted by topShear and its bottom edge by bottomShear (TMP_Text.GenerateTextMesh), from the font
//! asset's italicStyle. The UI text of Bevy cannot slant, so an italic run is rasterised here, a glyph at a time, and its rows moved by the same
//! shear: shear = italicStyle * 0.01, midPoint = capLine / 2, topShear = shear * (capLine - midPoint), bottomShear = shear * (0 - midPoint).

use bevy::{
    asset::RenderAssetUsages,
    image::Image,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use swash::{
    scale::{image::Content, Render, ScaleContext, Source},
    shape::ShapeContext,
    zeno::Format,
    FontRef,
};

/// The font asset's italic numbers: italicStyle * 0.01 and capLine over pointSize (the cap height in ems).
#[derive(Clone, Copy, Debug)]
pub struct ItalicFont {
    pub shear: f32,
    pub cap_line: f32,
}

/// A rasterised line: the image and its size and baseline in canvas units.
pub struct Line {
    pub image: Image,
    /// The image's size and its overhang each side of the text's advance, in canvas units.
    pub size: [f32; 2],
    pub margin: f32,
}

/// Rasterise `text` in `font` at `size` canvas units over `scale` pixels per unit, `line` units high, bold by drawing twice a little apart.
pub fn render(font: &[u8], italic: ItalicFont, text: &str, size: f32, line: f32, bold: bool, scale: f32) -> Option<Line> {
    let font = FontRef::from_index(font, 0)?;
    let px = size * scale;
    let metrics = font.metrics(&[]).scale(px);
    let (top_shear, bottom_shear) = {
        let cap = italic.cap_line * px;
        (italic.shear * cap / 2.0, -italic.shear * cap / 2.0)
    };
    // The glyphs, shaped (kerning, advances) on one line.
    let mut shape = ShapeContext::new();
    let mut shaper = shape.builder(font).size(px).build();
    shaper.add_str(text);
    let mut glyphs = Vec::new();
    let mut pen = 0.0f32;
    shaper.shape_with(|cluster| {
        for g in cluster.glyphs {
            glyphs.push((g.id, pen + g.x));
            pen += g.advance;
        }
    });
    // TMP's own spacing for bold (boldSpacing) is not applied: the run is as wide as the regular one.
    let height = (line * scale).ceil().max(1.0);
    let margin = (top_shear.abs().max(bottom_shear.abs()) + 2.0).ceil();
    let width = (pen + 2.0 * margin).ceil().max(1.0);
    // The text's ascent and descent centred in the line box, as a Text does.
    let baseline = ((height - (metrics.ascent + metrics.descent)) / 2.0 + metrics.ascent).round();
    let mut coverage = vec![0.0f32; (width * height) as usize];
    let mut context = ScaleContext::new();
    let mut scaler = context.builder(font).size(px).hint(false).build();
    let mut render = Render::new(&[Source::Outline]);
    render.format(Format::Alpha);
    for (id, x) in glyphs {
        let Some(g) = render.render(&mut scaler, id) else { continue };
        if g.content != Content::Mask || g.placement.width == 0 {
            continue;
        }
        let (gw, gh) = (g.placement.width as i32, g.placement.height as i32);
        // The quad's top edge is placement.top above the baseline, its bottom edge gh below that.
        let top_y = baseline as i32 - g.placement.top;
        for row in 0..gh {
            // From the bottom edge (0) to the top edge (1) of the quad.
            let t = if gh > 1 { 1.0 - row as f32 / (gh - 1) as f32 } else { 0.5 };
            let shift = bottom_shear + t * (top_shear - bottom_shear);
            let y = top_y + row;
            if y < 0 || y >= height as i32 {
                continue;
            }
            for col in 0..gw {
                let a = g.data[(row * gw + col) as usize] as f32 / 255.0;
                if a == 0.0 {
                    continue;
                }
                // The row moved by `shift` pixels: split between the two pixels it lands across.
                let at = margin + x + g.placement.left as f32 + col as f32 + shift;
                let (x0, f) = (at.floor() as i32, at - at.floor());
                for (dx, w) in [(0, 1.0 - f), (1, f)] {
                    for extra in [0.0, 1.2 * scale].into_iter().take(if bold { 2 } else { 1 }) {
                        let xi = x0 + dx + extra.round() as i32;
                        if xi >= 0 && xi < width as i32 {
                            let cell = &mut coverage[y as usize * width as usize + xi as usize];
                            *cell = (*cell + a * w).min(1.0);
                        }
                    }
                }
            }
        }
    }
    let mut data = Vec::with_capacity(coverage.len() * 4);
    for c in coverage {
        data.extend_from_slice(&[255, 255, 255, (c * 255.0).round() as u8]);
    }
    let image = Image::new(Extent3d { width: width as u32, height: height as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    Some(Line { image, size: [width / scale, height / scale], margin: margin / scale })
}

/// The rasterised italic runs by (text, bold, size, line height), made when the art loads.
static RUNS: std::sync::OnceLock<std::collections::HashMap<(String, bool, u32, u32), (bevy::prelude::Handle<Image>, [f32; 2], f32)>> = std::sync::OnceLock::new();

fn key(text: &str, bold: bool, size: f32, line: f32) -> (String, bool, u32, u32) {
    (text.to_string(), bold, (size * 100.0).round() as u32, (line * 100.0).round() as u32)
}

/// Rasterise every italic run of the scenes' texts: pixels per canvas unit.
const SCALE: f32 = 2.0;

pub fn load(font: &[u8], italic: ItalicFont, runs: &[(String, bool, f32, f32)], images: &mut bevy::prelude::Assets<Image>) {
    let mut out = std::collections::HashMap::new();
    for (text, bold, size, line) in runs {
        if let Some(l) = render(font, italic, text, *size, size * line, *bold, SCALE) {
            out.insert(key(text, *bold, *size, *line), (images.add(l.image), l.size, l.margin));
        }
    }
    let _ = RUNS.set(out);
}

/// An italic run's image and size in canvas units, if it was rasterised.
pub fn run(text: &str, bold: bool, size: f32, line: f32) -> Option<(bevy::prelude::Handle<Image>, [f32; 2], f32)> {
    RUNS.get()?.get(&key(text, bold, size, line)).cloned()
}
