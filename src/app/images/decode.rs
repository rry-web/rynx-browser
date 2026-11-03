//! Turning an image's bytes into pixels: PNG, JPEG, GIF and WebP with the `image` crate's
//! decoders, SVG drawn with resvg, all memory-safe and within limits on size. Big images are
//! scaled down, and transparent ones are put on white, as the pages that use them are: a
//! terminal's text blocks can't show transparency (it came out black), and a diagram's black
//! lines would vanish on a dark terminal's background.

use image::{DynamicImage, RgbImage};
use resvg::{tiny_skia, usvg};
use std::sync::{Arc, OnceLock};

/// Largest picture decoded, in pixels per side (a long comic strip is tall); and how far big
/// ones are scaled down: to this width, and this many pixels in all
const MAX_DECODE_SIDE: u32 = 30_000;
pub(super) const MAX_KEPT_SIDE: u32 = 2048;
pub(super) const MAX_KEPT_PIXELS: u64 = 2048 * 4096;
/// Most memory one image's decoder may use
const MAX_DECODE_ALLOC: u64 = 512 * 1024 * 1024;

/// Decode an image within limits, scaling big ones down and putting transparent ones on white
pub(super) fn decode(data: &[u8]) -> Result<DynamicImage, String> {
    if image::guess_format(data).is_err() && looks_like_svg(data) {
        return render_svg(data);
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| format!("unreadable image: {}", e))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| format!("couldn't decode the image: {}", e))?;
    let (width, height) = kept_size(image.width() as f64, image.height() as f64);
    let image = if (width, height) != (image.width(), image.height()) { image.thumbnail(width, height) } else { image };
    Ok(on_white(image))
}

/// The size an image of `width` by `height` pixels is kept at: scaled down to fit the limits
fn kept_size(width: f64, height: f64) -> (u32, u32) {
    let scale = (MAX_KEPT_SIDE as f64 / width).min((MAX_KEPT_PIXELS as f64 / (width * height)).sqrt()).min(1.0);
    (((width * scale) as u32).max(1), ((height * scale) as u32).max(1))
}

/// An image with any transparency drawn over white (one without is kept as it is)
fn on_white(image: DynamicImage) -> DynamicImage {
    if !image.color().has_alpha() {
        return image;
    }
    let rgba = image.into_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (from, to) in rgba.pixels().zip(out.pixels_mut()) {
        let alpha = from[3] as u32;
        for c in 0..3 {
            to[c] = ((from[c] as u32 * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
        }
    }
    DynamicImage::ImageRgb8(out)
}

/// Whether bytes that aren't a known picture format look like an SVG (or a gzipped .svgz)
fn looks_like_svg(data: &[u8]) -> bool {
    if data.starts_with(&[0x1f, 0x8b]) {
        return true;
    }
    let start = &data[..data.len().min(4096)];
    let text = String::from_utf8_lossy(start);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with('<') && text.contains("<svg")
}

/// The system's fonts, for text in SVGs: found the first time an SVG needs them. Only system
/// folders are read (the sandbox keeps the home folder's fonts out of reach anyway).
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(FONTS.get_or_init(|| {
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        Arc::new(fonts)
    }))
}

/// Draw an SVG at its own size (scaled down if big), on white. Nothing it refers to is loaded:
/// not files on this computer, and not addresses (resvg has no network access); pictures
/// embedded in it are left out.
fn render_svg(data: &[u8]) -> Result<DynamicImage, String> {
    let options = usvg::Options {
        fontdb: fonts(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            ..usvg::ImageHrefResolver::default()
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(data, &options).map_err(|e| format!("couldn't read the SVG: {}", e))?;
    let size = tree.size();
    let (width, height) = (size.width() as f64, size.height() as f64);
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return Err("the SVG has no size".to_string());
    }
    let (columns, rows) = kept_size(width, height);
    let mut pixmap = tiny_skia::Pixmap::new(columns, rows).ok_or("the SVG is too large to draw")?;
    pixmap.fill(tiny_skia::Color::WHITE);
    let transform = tiny_skia::Transform::from_scale(columns as f32 / size.width(), rows as f32 / size.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    // Opaque everywhere (drawn on white), so the premultiplied pixels are the colours themselves
    let pixels = pixmap.data().as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    RgbImage::from_raw(columns, rows, pixels).map(DynamicImage::ImageRgb8).ok_or_else(|| "couldn't draw the SVG".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(image: DynamicImage) -> Vec<u8> {
        let mut data = std::io::Cursor::new(Vec::new());
        image.write_to(&mut data, image::ImageFormat::Png).unwrap();
        data.into_inner()
    }

    #[test]
    fn long_strips_are_kept_sharp() {
        let kept = decode(&png(DynamicImage::new_rgb8(800, 12_000))).unwrap();
        assert!(kept.width() >= 600, "{}x{}", kept.width(), kept.height());
        assert!(kept.width() as u64 * kept.height() as u64 <= MAX_KEPT_PIXELS);
    }

    #[test]
    fn transparent_images_are_put_on_white() {
        // A diagram's black line on a transparent background, and a half-transparent red
        let mut image = image::RgbaImage::from_pixel(4, 1, image::Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 0, image::Rgba([0, 0, 0, 255]));
        image.put_pixel(2, 0, image::Rgba([255, 0, 0, 128]));
        let kept = decode(&png(DynamicImage::ImageRgba8(image))).unwrap();
        assert!(!kept.color().has_alpha());
        let kept = kept.to_rgb8();
        assert_eq!(kept.get_pixel(0, 0).0, [255, 255, 255], "the background is white, not black");
        assert_eq!(kept.get_pixel(1, 0).0, [0, 0, 0]);
        assert_eq!(kept.get_pixel(2, 0).0, [255, 127, 127]);
    }

    #[test]
    fn svgs_are_drawn_on_white() {
        let svg = br#"<?xml version="1.0"?>
            <svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
              <rect x="0" y="0" width="20" height="20" fill="blue"/>
            </svg>"#;
        let image = decode(svg).unwrap().to_rgb8();
        assert_eq!((image.width(), image.height()), (40, 20));
        assert_eq!(image.get_pixel(5, 5).0, [0, 0, 255]);
        assert_eq!(image.get_pixel(30, 5).0, [255, 255, 255]);
    }

    #[test]
    fn big_svgs_are_scaled_down() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100000" height="50000"/>"#;
        let image = decode(svg).unwrap();
        assert_eq!(image.width(), MAX_KEPT_SIDE);
        assert_eq!(image.height(), MAX_KEPT_SIDE / 2);
    }

    #[test]
    fn svgs_cant_read_files_on_this_computer() {
        // An <image> pointing at a picture on disk is left out, not drawn
        let dir = std::env::temp_dir().join(format!("rynx-svg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("red.png");
        std::fs::write(&path, png(DynamicImage::ImageRgb8(RgbImage::from_pixel(10, 10, image::Rgb([255, 0, 0])))))
            .unwrap();
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10" height="10">
                 <image width="10" height="10" xlink:href="{}"/>
               </svg>"#,
            path.display()
        );
        let image = decode(svg.as_bytes()).unwrap().to_rgb8();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(image.get_pixel(5, 5).0, [255, 255, 255]);
    }

    #[test]
    fn what_isnt_an_image_says_so() {
        assert!(decode(b"<!doctype html><title>Not found</title>").is_err());
        assert!(decode(b"<svg").is_err());
        assert!(decode(b"").is_err());
    }
}
