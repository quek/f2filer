use std::path::Path;

use eframe::egui;

use crate::file_item::format_size;

pub struct ArchivePreview {
    pub title: String,
    content: String,
}

impl ArchivePreview {
    pub fn load(path: &Path) -> Option<Self> {
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let content = list_zip_contents(path)?;

        Some(ArchivePreview { title, content })
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

const ARCHIVE_EXTENSIONS: &[&str] = &["zip"];

pub fn is_archive_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| ARCHIVE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn list_zip_contents(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    let mut lines = Vec::new();
    let mut total_size: u64 = 0;
    let file_count = archive.len();

    for i in 0..file_count {
        let entry = archive.by_index(i).ok()?;
        let name = entry.name().to_string();
        let size = entry.size();
        total_size += size;

        let date_str = entry
            .last_modified()
            .map(|dt| {
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}",
                    dt.year(),
                    dt.month(),
                    dt.day(),
                    dt.hour(),
                    dt.minute(),
                )
            })
            .unwrap_or_else(|| "                ".to_string());

        let size_str = if entry.is_dir() {
            "     <DIR>".to_string()
        } else {
            format!("{:>10}", format_size(size))
        };

        lines.push(format!("{}  {}  {}", date_str, size_str, name));
    }

    let header = format!("{} files, {}", file_count, format_size(total_size));
    let separator = "─".repeat(60);

    let mut result = String::new();
    result.push_str(&header);
    result.push('\n');
    result.push_str(&separator);
    result.push('\n');
    for line in &lines {
        result.push_str(line);
        result.push('\n');
    }

    Some(result)
}
