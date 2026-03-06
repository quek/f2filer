use std::path::Path;

use eframe::egui;

pub struct ImagePreview {
    pub title: String,
    texture: egui::TextureHandle,
    image_size: [f32; 2],
}

impl ImagePreview {
    pub fn load(ctx: &egui::Context, path: &Path) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        let img = image::load_from_memory(&data).ok()?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();

        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            rgba.as_raw(),
        );

        let texture = ctx.load_texture(
            path.to_string_lossy(),
            color_image,
            egui::TextureOptions::LINEAR,
        );

        Some(ImagePreview {
            title: path.file_name()?.to_string_lossy().to_string(),
            texture,
            image_size: [w as f32, h as f32],
        })
    }

    pub fn ui(&self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.strong(&self.title);
            ui.separator();
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
                    self.texture.id(),
                    display_size,
                ));
            });
        });
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
