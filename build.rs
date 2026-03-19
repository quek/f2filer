use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let icon_path = Path::new(&out_dir).join("f2filer.ico");

    // 32x32 32-bit RGBA icon
    let pixels = generate_icon_32x32();
    let ico_data = build_ico(&pixels, 32, 32);
    fs::write(&icon_path, &ico_data).unwrap();

    // Raw RGBA for eframe window icon
    let rgba_path = Path::new(&out_dir).join("f2filer_icon.rgba");
    let mut rgba = Vec::with_capacity(32 * 32 * 4);
    for p in &pixels {
        rgba.extend_from_slice(p);
    }
    fs::write(&rgba_path, &rgba).unwrap();

    // Resource script
    let rc_path = Path::new(&out_dir).join("f2filer.rc");
    fs::write(
        &rc_path,
        format!(
            "1 ICON \"{}\"",
            icon_path.to_str().unwrap().replace('\\', "/")
        ),
    )
    .unwrap();

    let _ = embed_resource::compile(&rc_path, embed_resource::NONE);
}

fn generate_icon_32x32() -> Vec<[u8; 4]> {
    // Dual-pane file manager icon
    let bg: [u8; 4] = [0x1a, 0x1a, 0x2e, 0xFF];
    let border: [u8; 4] = [0x3B, 0x78, 0xFF, 0xFF];
    let titlebar: [u8; 4] = [0x2a, 0x2a, 0x4e, 0xFF];
    let folder: [u8; 4] = [0xFF, 0xC1, 0x07, 0xFF];
    let file_line: [u8; 4] = [0x55, 0x66, 0x88, 0xFF];
    let cursor_bg: [u8; 4] = [0x2B, 0x4E, 0x8C, 0xFF];
    let text_bright: [u8; 4] = [0xCC, 0xDD, 0xFF, 0xFF];
    let transparent: [u8; 4] = [0, 0, 0, 0];
    let btn: [u8; 4] = [0xCC, 0xCC, 0xCC, 0xFF];
    let btn_close: [u8; 4] = [0xE7, 0x48, 0x56, 0xFF];

    let mut pixels = vec![transparent; 32 * 32];

    let set = |pixels: &mut Vec<[u8; 4]>, x: usize, y: usize, c: [u8; 4]| {
        if x < 32 && y < 32 {
            pixels[y * 32 + x] = c;
        }
    };

    // Rounded rectangle background (2px corner radius)
    for y in 0..32 {
        for x in 0..32 {
            let in_corner = (x < 2 && y < 2 && (x + y < 2))
                || (x >= 30 && y < 2 && ((31 - x) + y < 2))
                || (x < 2 && y >= 30 && (x + (31 - y) < 2))
                || (x >= 30 && y >= 30 && ((31 - x) + (31 - y) < 2));

            if !in_corner {
                set(&mut pixels, x, y, if y < 6 { titlebar } else { bg });
            }
        }
    }

    // Top border
    for x in 2..30 {
        set(&mut pixels, x, 0, border);
    }
    for x in 1..31 {
        set(
            &mut pixels,
            x,
            1,
            if x < 2 || x >= 30 { border } else { titlebar },
        );
    }

    // Title bar buttons
    // Close x (x=27..29, y=2..4)
    set(&mut pixels, 27, 2, btn_close);
    set(&mut pixels, 29, 2, btn_close);
    set(&mut pixels, 28, 3, btn_close);
    set(&mut pixels, 27, 4, btn_close);
    set(&mut pixels, 29, 4, btn_close);
    // Maximize [] (x=23..25, y=2..4)
    for dx in 0..3 {
        set(&mut pixels, 23 + dx, 2, btn);
        set(&mut pixels, 23 + dx, 4, btn);
    }
    set(&mut pixels, 23, 3, btn);
    set(&mut pixels, 25, 3, btn);

    // Separator below titlebar (y=6)
    for x in 0..32 {
        if pixels[6 * 32 + x] != transparent {
            set(&mut pixels, x, 6, border);
        }
    }

    // Center divider (x=15, y=7..31)
    for y in 7..32 {
        if pixels[y * 32 + 15] != transparent {
            set(&mut pixels, 15, y, border);
        }
    }

    // === Left pane (x=2..14) ===

    // Folder icon (y=9-11)
    for x in 3..6 {
        set(&mut pixels, x, 9, folder);
    }
    for x in 3..9 {
        set(&mut pixels, x, 10, folder);
        set(&mut pixels, x, 11, folder);
    }

    // File lines (2px thick)
    for &row in &[15, 20, 25] {
        for y in row..row + 2 {
            for x in 3..13 {
                set(&mut pixels, x, y, file_line);
            }
        }
    }

    // === Right pane (x=16..30) ===

    // Cursor highlight bar (y=9-11)
    for y in 9..12 {
        for x in 17..29 {
            set(&mut pixels, x, y, cursor_bg);
        }
    }
    // Bright text on cursor
    for x in 18..26 {
        set(&mut pixels, x, 10, text_bright);
    }

    // File lines (2px thick)
    for &row in &[15, 20, 25] {
        for y in row..row + 2 {
            for x in 17..29 {
                set(&mut pixels, x, y, file_line);
            }
        }
    }

    pixels
}

fn build_ico(pixels: &[[u8; 4]], width: u32, height: u32) -> Vec<u8> {
    let pixel_count = (width * height) as usize;
    let and_mask_row = ((width + 31) / 32 * 4) as usize;
    let and_mask_size = and_mask_row * height as usize;
    let bmp_size = 40 + pixel_count * 4 + and_mask_size;

    let mut data = Vec::new();

    // ICO Header
    data.extend_from_slice(&0u16.to_le_bytes()); // reserved
    data.extend_from_slice(&1u16.to_le_bytes()); // type = icon
    data.extend_from_slice(&1u16.to_le_bytes()); // count = 1

    // Directory Entry
    data.push(width as u8);
    data.push(height as u8);
    data.push(0); // color count
    data.push(0); // reserved
    data.extend_from_slice(&1u16.to_le_bytes()); // planes
    data.extend_from_slice(&32u16.to_le_bytes()); // bpp
    data.extend_from_slice(&(bmp_size as u32).to_le_bytes());
    data.extend_from_slice(&22u32.to_le_bytes()); // offset

    // BITMAPINFOHEADER
    data.extend_from_slice(&40u32.to_le_bytes()); // biSize
    data.extend_from_slice(&(width as i32).to_le_bytes());
    data.extend_from_slice(&((height * 2) as i32).to_le_bytes()); // doubled for ICO
    data.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    data.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    data.extend_from_slice(&0u32.to_le_bytes()); // biCompression
    data.extend_from_slice(&0u32.to_le_bytes()); // biSizeImage
    data.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    data.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    data.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    data.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // Pixel data (bottom-to-top, BGRA)
    for y in (0..height as usize).rev() {
        for x in 0..width as usize {
            let p = pixels[y * width as usize + x];
            data.push(p[2]); // B
            data.push(p[1]); // G
            data.push(p[0]); // R
            data.push(p[3]); // A
        }
    }

    // AND mask (all 0 for 32-bit alpha)
    for _ in 0..and_mask_size {
        data.push(0);
    }

    data
}
