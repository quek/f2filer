use std::io::Read;
use std::path::Path;

use eframe::egui;

use crate::file_item::format_size;

/// Maximum number of entries to list in archive preview.
const MAX_ARCHIVE_ENTRIES: usize = 10_000;

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

        let name_lower = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_lowercase())
            .unwrap_or_default();
        let content = if name_lower.ends_with(".tar.gz") || name_lower.ends_with(".tgz")
            || name_lower.ends_with(".tar.xz") || name_lower.ends_with(".txz")
            || name_lower.ends_with(".tar") {
            list_tar_contents(path)?
        } else {
            list_zip_contents(path)?
        };

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

pub fn is_archive_file(path: &Path) -> bool {
    let ext_lower = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if matches!(ext_lower.as_str(), "zip" | "tgz" | "txz" | "tar") {
        return true;
    }
    // Check for double extensions (.tar.gz, .tar.xz)
    let name_lower = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_lowercase())
        .unwrap_or_default();
    name_lower.ends_with(".tar.gz") || name_lower.ends_with(".tar.xz")
}

fn list_zip_contents(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    let mut lines = Vec::new();
    let mut total_size: u64 = 0;
    let file_count = archive.len();

    let display_count = file_count.min(MAX_ARCHIVE_ENTRIES);
    for i in 0..display_count {
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

    let header = if file_count > display_count {
        format!("{} files (先頭{}件のみ表示)", file_count, MAX_ARCHIVE_ENTRIES)
    } else {
        format!("{} files, {}", file_count, format_size(total_size))
    };
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

fn list_tar_contents(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;

    let name_lower = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_lowercase())
        .unwrap_or_default();
    let is_gzip = name_lower.ends_with(".tar.gz") || name_lower.ends_with(".tgz");
    let is_xz = name_lower.ends_with(".tar.xz") || name_lower.ends_with(".txz");

    let mut archive = if is_gzip {
        let decoder = flate2::read::GzDecoder::new(file);
        tar::Archive::new(Box::new(decoder) as Box<dyn Read>)
    } else if is_xz {
        let decoder = xz2::read::XzDecoder::new(file);
        tar::Archive::new(Box::new(decoder) as Box<dyn Read>)
    } else {
        tar::Archive::new(Box::new(file) as Box<dyn Read>)
    };

    let entries = archive.entries().ok()?;

    let mut lines = Vec::new();
    let mut total_size: u64 = 0;
    let mut file_count: usize = 0;

    let mut truncated = false;
    for entry_result in entries {
        let Ok(entry) = entry_result else { continue };
        let Ok(entry_path) = entry.path().map(|p| p.to_string_lossy().to_string()) else { continue };

        let size = entry.size();
        total_size += size;
        file_count += 1;

        if file_count > MAX_ARCHIVE_ENTRIES {
            truncated = true;
            break;
        }

        let mtime = entry.header().mtime().unwrap_or(0);
        let date_str = if mtime > 0 {
            let dt = chrono::DateTime::from_timestamp(mtime as i64, 0);
            match dt {
                Some(d) => d.format("%Y-%m-%d %H:%M").to_string(),
                None => "                ".to_string(),
            }
        } else {
            "                ".to_string()
        };

        let is_dir = entry.header().entry_type().is_dir();
        let size_str = if is_dir {
            "     <DIR>".to_string()
        } else {
            format!("{:>10}", format_size(size))
        };

        lines.push(format!("{}  {}  {}", date_str, size_str, entry_path));
    }

    let header = if truncated {
        format!("{}+ files (先頭{}件のみ表示)", file_count, MAX_ARCHIVE_ENTRIES)
    } else {
        format!("{} files, {}", file_count, format_size(total_size))
    };
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
