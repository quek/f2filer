use std::path::{Path, PathBuf};

use eframe::egui;

pub struct FileViewer {
    pub path: PathBuf,
    pub content: String,
    pub open: bool,
    pub scroll_offset: f32,
}

impl FileViewer {
    pub fn open(path: &Path) -> Option<Self> {
        let content = read_text_file(path)?;
        Some(FileViewer {
            path: path.to_path_buf(),
            content,
            open: true,
            scroll_offset: 0.0,
        })
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }

        let title = format!(
            "Viewer: {}",
            self.path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        );

        egui::Window::new(&title)
            .collapsible(true)
            .resizable(true)
            .default_size([600.0, 400.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut self.open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{}", self.path.display()));
                    ui.label(format!("({} bytes)", self.content.len()));
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
            });

        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.open = false;
        }
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
            return Some("[Binary file]".to_string());
        }

        // Try lossy UTF-8
        return Some(String::from_utf8_lossy(&bytes).to_string());
    }

    None
}
