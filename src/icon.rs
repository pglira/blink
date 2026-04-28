//! Eye icons for the tray and the viewer window.
//!
//! Sources are Lucide's `eye` and `eye-off` SVGs (ISC-licensed; see
//! `assets/icons/LUCIDE_LICENSE`). They are rendered at runtime via `resvg`,
//! so any caller-requested size renders crisply with proper anti-aliasing.

use resvg::{tiny_skia, usvg};

/// Per-pixel byte order of the returned buffer.
pub enum ByteOrder {
    /// `[A, R, G, B]` per pixel — what ksni wants on the wire.
    Argb,
    /// `[R, G, B, A]` per pixel, straight (non-premultiplied) alpha — what
    /// eframe / egui `IconData` wants.
    Rgba,
}

const EYE_SVG: &str = include_str!("../assets/icons/eye.svg");
const EYE_OFF_SVG: &str = include_str!("../assets/icons/eye-off.svg");

/// A vivid blue picked to read well on dark and light panels alike.
const ACTIVE_STROKE: &str = "#4DA3FF";
/// A muted neutral grey for the paused state.
const PAUSED_STROKE: &str = "#9AA0A6";

pub fn active(size: u32, order: ByteOrder) -> Vec<u8> {
    render(EYE_SVG, ACTIVE_STROKE, size, order)
}

pub fn paused(size: u32, order: ByteOrder) -> Vec<u8> {
    render(EYE_OFF_SVG, PAUSED_STROKE, size, order)
}

fn render(svg_src: &str, stroke: &str, size: u32, order: ByteOrder) -> Vec<u8> {
    // Lucide ships the SVGs with `stroke="currentColor"`; bake in our colour
    // before parsing so we don't need to walk the tree to recolour nodes.
    let svg = svg_src.replace("currentColor", stroke);

    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(&svg, &opt).expect("bundled SVG must parse");

    let svg_size = tree.size();
    let scale = (size as f32 / svg_size.width()).min(size as f32 / svg_size.height());
    let transform = tiny_skia::Transform::from_scale(scale, scale);

    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("non-zero pixmap size");
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // tiny-skia stores premultiplied RGBA; both consumers want straight alpha.
    let mut data = pixmap.take();
    unpremultiply(&mut data);
    match order {
        ByteOrder::Rgba => data,
        ByteOrder::Argb => rgba_to_argb_inplace(data),
    }
}

fn unpremultiply(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3];
        if a == 0 || a == 255 {
            continue;
        }
        let inv = 255.0 / a as f32;
        px[0] = ((px[0] as f32 * inv).round() as u32).min(255) as u8;
        px[1] = ((px[1] as f32 * inv).round() as u32).min(255) as u8;
        px[2] = ((px[2] as f32 * inv).round() as u32).min(255) as u8;
    }
}

fn rgba_to_argb_inplace(mut rgba: Vec<u8>) -> Vec<u8> {
    for px in rgba.chunks_exact_mut(4) {
        let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
        px[0] = a;
        px[1] = r;
        px[2] = g;
        px[3] = b;
    }
    rgba
}
