use std::path::Path;

use eframe::egui;

pub struct TextPreview {
    pub title: String,
    pub content: String,
}

impl TextPreview {
    pub fn load(path: &Path) -> Option<Self> {
        let content = read_text_file(path)?;
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        Some(TextPreview { title, content })
    }

    pub fn ui(&self, ui: &mut egui::Ui) {
        ui.label(&self.title);
        ui.separator();
        egui::ScrollArea::both()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.content.as_str())
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY),
                );
            });
    }
}

fn read_text_file(path: &Path) -> Option<String> {
    // Try UTF-8 first
    if let Ok(content) = std::fs::read_to_string(path) {
        return Some(content);
    }

    // Try reading as bytes and attempt Shift-JIS decoding
    if let Ok(bytes) = std::fs::read(path) {
        // Check if it looks like binary
        let sample = &bytes[..bytes.len().min(8192)];
        let null_count = sample.iter().filter(|&&b| b == 0).count();
        if null_count > sample.len() / 10 {
            return None;
        }

        // Try lossy UTF-8
        return Some(String::from_utf8_lossy(&bytes).to_string());
    }

    None
}
