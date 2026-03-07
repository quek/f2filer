use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::{Rgba, RgbaImage};
use std::fs::File;
use std::path::Path;

// Icon line color (green)
const LINE_R: u8 = 46;
const LINE_G: u8 = 184;
const LINE_B: u8 = 92;

fn main() {
    let input = Path::new("assets/Duck-ai-image-2026-03-06-23-51.jpeg");
    let output = Path::new("assets/icon.ico");
    let output_png = Path::new("assets/icon.png");

    println!("Loading {}", input.display());
    let img = image::open(input).expect("Failed to open image");
    let rgba = img.to_rgba8();

    // Extract icon mask (dark pixels = icon lines)
    let mask = extract_icon_mask(&rgba);

    // Crop to square (center crop)
    let (w, h) = (mask.width(), mask.height());
    let size = w.min(h);
    let x_offset = (w - size) / 2;
    let y_offset = (h - size) / 2;
    let cropped = image::imageops::crop_imm(&mask, x_offset, y_offset, size, size).to_image();

    // Compose: blue rounded-rect background + white icon lines
    let resized_256 = image::imageops::resize(&cropped, 256, 256, FilterType::Lanczos3);
    let final_icon = compose_icon(&resized_256, 256);

    // Save 256x256 PNG (for runtime window icon)
    final_icon.save(output_png).expect("Failed to save PNG");
    println!("Saved {}", output_png.display());

    // Generate ICO with multiple sizes
    let sizes = [256u32, 48, 32, 16];
    let mut frames = Vec::new();
    for &s in &sizes {
        let resized = image::imageops::resize(&cropped, s, s, FilterType::Lanczos3);
        let composed = compose_icon(&resized, s);
        let frame = IcoFrame::as_png(composed.as_raw(), s, s, image::ColorType::Rgba8.into())
            .expect("Failed to create ICO frame");
        frames.push(frame);
    }

    let file = File::create(output).expect("Failed to create ICO file");
    let encoder = IcoEncoder::new(file);
    encoder.encode_images(&frames).expect("Failed to encode ICO");
    println!("Saved {}", output.display());
    println!("Done!");
}

/// Extract icon lines as white-on-transparent mask from the JPEG.
/// Dark pixels (icon lines) → white with alpha proportional to darkness.
/// Light pixels (background) → fully transparent.
fn extract_icon_mask(img: &RgbaImage) -> RgbaImage {
    let mut result = RgbaImage::new(img.width(), img.height());

    for (x, y, pixel) in img.enumerate_pixels() {
        let [r, g, b, _] = pixel.0;
        let lum = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;

        let alpha = if lum > 240.0 {
            0u8 // Pure background
        } else if lum > 180.0 {
            ((240.0 - lum) / 60.0 * 255.0) as u8 // Anti-alias transition
        } else {
            255 // Icon line
        };

        // Store as white pixel with varying alpha (the mask)
        result.put_pixel(x, y, Rgba([255, 255, 255, alpha]));
    }

    result
}

/// Compose final icon: transparent background + green icon lines.
fn compose_icon(mask: &RgbaImage, _size: u32) -> RgbaImage {
    let mut result = mask.clone();

    for pixel in result.pixels_mut() {
        let a = pixel.0[3];
        if a > 0 {
            // Color the icon lines green, keep alpha
            pixel.0 = [LINE_R, LINE_G, LINE_B, a];
        }
    }

    result
}
