use std::io::Read;
use std::path::Path;

use eframe::egui;

/// Maximum bytes to read for text preview (1 MB).
const MAX_PREVIEW_BYTES: u64 = 1_024 * 1024;

pub struct TextPreview {
    pub title: String,
    pub content: String,
    truncated: bool,
}

impl TextPreview {
    pub fn load(path: &Path) -> Option<Self> {
        let (content, truncated) = read_text_file(path)?;
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        Some(TextPreview { title, content, truncated })
    }

    pub fn ui(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.title);
            if self.truncated {
                ui.label(egui::RichText::new("(先頭1MBのみ表示)").weak());
            }
        });
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

fn read_text_file(path: &Path) -> Option<(String, bool)> {
    let file = std::fs::File::open(path).ok()?;
    let file_size = file.metadata().ok()?.len();
    let truncated = file_size > MAX_PREVIEW_BYTES;
    let read_limit = file_size.min(MAX_PREVIEW_BYTES);

    let mut buf = Vec::with_capacity(read_limit as usize);
    file.take(read_limit).read_to_end(&mut buf).ok()?;

    // Check if it looks like binary
    let sample = &buf[..buf.len().min(8192)];
    let null_count = sample.iter().filter(|&&b| b == 0).count();
    if null_count > sample.len() / 10 {
        return None;
    }

    let content = String::from_utf8_lossy(&buf).to_string();
    Some((content, truncated))
}
