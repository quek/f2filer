use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use eframe::egui;

const MAX_PREVIEW_SIZE: u32 = 800;
const CACHE_CAPACITY: usize = 20;

struct DecodedFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    delay_ms: u32,
}

struct DecodedImage {
    path: PathBuf,
    frames: Vec<DecodedFrame>,
}

struct AnimFrame {
    texture: egui::TextureHandle,
    delay_ms: u32,
}

pub struct ImagePreview {
    pub title: String,
    frames: Vec<AnimFrame>,
    image_size: [f32; 2],
    start_time: Instant,
    total_duration_ms: u32,
}

pub struct ImageCache {
    entries: HashMap<PathBuf, CacheEntry>,
    order: Vec<PathBuf>,
    loading: Arc<Mutex<Option<DecodedImage>>>,
    loading_path: Option<PathBuf>,
    wanted_path: Option<PathBuf>,
}

struct CacheEntry {
    frames: Vec<AnimFrame>,
    image_size: [f32; 2],
    total_duration_ms: u32,
}

impl ImageCache {
    pub fn new() -> Self {
        ImageCache {
            entries: HashMap::new(),
            order: Vec::new(),
            loading: Arc::new(Mutex::new(None)),
            loading_path: None,
            wanted_path: None,
        }
    }

    pub fn clear_wanted(&mut self) {
        self.wanted_path = None;
    }

    /// Request an image. Returns immediately from cache, or starts background load.
    pub fn get_or_load(&mut self, ctx: &egui::Context, path: &Path) -> Option<ImagePreview> {
        let key = path.to_path_buf();
        self.wanted_path = Some(key.clone());

        // Cache hit
        if let Some(entry) = self.entries.get(&key) {
            self.order.retain(|p| p != &key);
            self.order.push(key);

            let frames: Vec<AnimFrame> = entry
                .frames
                .iter()
                .map(|f| AnimFrame {
                    texture: f.texture.clone(),
                    delay_ms: f.delay_ms,
                })
                .collect();

            return Some(ImagePreview {
                title: path.file_name()?.to_string_lossy().to_string(),
                frames,
                image_size: entry.image_size,
                start_time: Instant::now(),
                total_duration_ms: entry.total_duration_ms,
            });
        }

        // Already loading this path
        if self.loading_path.as_ref() == Some(&key) {
            return None;
        }

        // Start background load
        self.loading_path = Some(key.clone());
        let loading = Arc::clone(&self.loading);
        let path_clone = key;
        let repaint_ctx = ctx.clone();

        thread::spawn(move || {
            let result = decode_image(&path_clone);
            if let Ok(mut lock) = loading.lock() {
                *lock = result;
            }
            repaint_ctx.request_repaint();
        });

        None
    }

    /// Check if background loading finished. Call each frame.
    /// Only returns the preview if the loaded path matches what is currently wanted.
    pub fn poll_loaded(&mut self, ctx: &egui::Context) -> Option<ImagePreview> {
        let decoded = {
            let mut lock = self.loading.lock().ok()?;
            lock.take()?
        };

        let path = decoded.path.clone();
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if decoded.frames.is_empty() {
            self.loading_path = None;
            return None;
        }

        let image_size = [
            decoded.frames[0].width as f32,
            decoded.frames[0].height as f32,
        ];

        let mut anim_frames = Vec::with_capacity(decoded.frames.len());
        let mut total_duration_ms = 0u32;

        for (i, frame) in decoded.frames.iter().enumerate() {
            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.rgba,
            );

            let texture = ctx.load_texture(
                format!("{}#frame{}", path.to_string_lossy(), i),
                color_image,
                egui::TextureOptions::LINEAR,
            );

            total_duration_ms += frame.delay_ms;
            anim_frames.push(AnimFrame {
                texture,
                delay_ms: frame.delay_ms,
            });
        }

        // Cache it
        if self.order.len() >= CACHE_CAPACITY {
            if let Some(oldest) = self.order.first().cloned() {
                self.entries.remove(&oldest);
                self.order.remove(0);
            }
        }

        let cache_frames: Vec<AnimFrame> = anim_frames
            .iter()
            .map(|f| AnimFrame {
                texture: f.texture.clone(),
                delay_ms: f.delay_ms,
            })
            .collect();

        self.entries.insert(
            path.clone(),
            CacheEntry {
                frames: cache_frames,
                image_size,
                total_duration_ms,
            },
        );
        self.order.push(path.clone());
        self.loading_path = None;

        // Only return if this is still what the user wants
        if self.wanted_path.as_ref() == Some(&path) {
            Some(ImagePreview {
                title,
                frames: anim_frames,
                image_size,
                start_time: Instant::now(),
                total_duration_ms,
            })
        } else {
            None
        }
    }
}

fn decode_image(path: &Path) -> Option<DecodedImage> {
    let data = std::fs::read(path).ok()?;

    // Check if GIF with animation
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    if ext.as_deref() == Some("gif") {
        if let Some(decoded) = decode_gif_frames(path, &data) {
            if decoded.frames.len() > 1 {
                return Some(decoded);
            }
        }
    }

    // Static image (or single-frame GIF)
    let img = image::load_from_memory(&data).ok()?;
    let img = if img.width() > MAX_PREVIEW_SIZE || img.height() > MAX_PREVIEW_SIZE {
        img.thumbnail(MAX_PREVIEW_SIZE, MAX_PREVIEW_SIZE)
    } else {
        img
    };

    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    Some(DecodedImage {
        path: path.to_path_buf(),
        frames: vec![DecodedFrame {
            width: w,
            height: h,
            rgba: rgba.into_raw(),
            delay_ms: 0,
        }],
    })
}

fn decode_gif_frames(path: &Path, data: &[u8]) -> Option<DecodedImage> {
    use image::codecs::gif::GifDecoder;
    use image::AnimationDecoder;

    let decoder = GifDecoder::new(Cursor::new(data)).ok()?;
    let raw_frames: Vec<_> = decoder.into_frames().collect_frames().ok()?;

    if raw_frames.is_empty() {
        return None;
    }

    let mut frames = Vec::with_capacity(raw_frames.len());

    for frame in &raw_frames {
        let (num, den) = frame.delay().numer_denom_ms();
        let delay_ms = if den > 0 { num / den } else { 100 };
        // GIF spec: 0 delay means "as fast as possible", use 100ms default
        let delay_ms = if delay_ms == 0 { 100 } else { delay_ms };

        let rgba_image = frame.buffer();
        let (w, h) = rgba_image.dimensions();

        // Resize if needed
        let (final_w, final_h, final_rgba) =
            if w > MAX_PREVIEW_SIZE || h > MAX_PREVIEW_SIZE {
                let img = image::DynamicImage::ImageRgba8(rgba_image.clone());
                let resized = img.thumbnail(MAX_PREVIEW_SIZE, MAX_PREVIEW_SIZE);
                let buf = resized.to_rgba8();
                let (rw, rh) = buf.dimensions();
                (rw, rh, buf.into_raw())
            } else {
                (w, h, rgba_image.as_raw().clone())
            };

        frames.push(DecodedFrame {
            width: final_w,
            height: final_h,
            rgba: final_rgba,
            delay_ms,
        });
    }

    Some(DecodedImage {
        path: path.to_path_buf(),
        frames,
    })
}

impl ImagePreview {
    pub fn ui(&self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.strong(&self.title);
            ui.separator();

            let frame_idx = self.current_frame_index();
            let frame = &self.frames[frame_idx];

            egui::ScrollArea::both().show(ui, |ui| {
                let available = ui.available_size();
                let scale_x = available.x / self.image_size[0];
                let scale_y = available.y / self.image_size[1];
                let scale = scale_x.min(scale_y).min(1.0);

                let display_size = egui::vec2(
                    self.image_size[0] * scale,
                    self.image_size[1] * scale,
                );

                ui.image(egui::load::SizedTexture::new(
                    frame.texture.id(),
                    display_size,
                ));
            });

            // Request repaint for animation
            if self.frames.len() > 1 {
                let next_delay = self.ms_until_next_frame();
                ui.ctx().request_repaint_after(
                    std::time::Duration::from_millis(next_delay as u64),
                );
            }
        });
    }

    fn current_frame_index(&self) -> usize {
        if self.frames.len() <= 1 || self.total_duration_ms == 0 {
            return 0;
        }

        let elapsed_ms = self.start_time.elapsed().as_millis() as u32;
        let elapsed_in_cycle = elapsed_ms % self.total_duration_ms;

        let mut accum = 0u32;
        for (i, frame) in self.frames.iter().enumerate() {
            accum += frame.delay_ms;
            if elapsed_in_cycle < accum {
                return i;
            }
        }
        self.frames.len() - 1
    }

    fn ms_until_next_frame(&self) -> u32 {
        if self.frames.len() <= 1 || self.total_duration_ms == 0 {
            return 100;
        }

        let elapsed_ms = self.start_time.elapsed().as_millis() as u32;
        let elapsed_in_cycle = elapsed_ms % self.total_duration_ms;

        let mut accum = 0u32;
        for frame in &self.frames {
            accum += frame.delay_ms;
            if elapsed_in_cycle < accum {
                return (accum - elapsed_in_cycle).max(1);
            }
        }
        1
    }
}

const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico",
];

pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}
