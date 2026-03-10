use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use parking_lot::Mutex;
use std::time::SystemTime;

use eframe::egui;

#[derive(Clone, Debug)]
pub struct FileItem {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub is_dir: bool,
    pub is_hidden: bool,
    pub extension: String,
    pub cached_size: String,
    pub cached_date: String,
}

impl FileItem {
    pub fn from_path(path: &Path) -> Option<Self> {
        let metadata = path.symlink_metadata().ok()?;
        let name = path.file_name()?.to_string_lossy().to_string();
        let is_dir = metadata.is_dir();
        let size = if is_dir { 0 } else { metadata.len() };
        let modified = metadata.modified().ok();
        let extension = if is_dir {
            String::new()
        } else {
            path.extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default()
        };

        let is_hidden = is_hidden_file(path, &name);

        let cached_size = if is_dir {
            String::new()
        } else {
            format_size(size)
        };
        let cached_date = match modified {
            Some(time) => {
                let datetime: chrono::DateTime<chrono::Local> = time.into();
                datetime.format("%Y-%m-%d %H:%M").to_string()
            }
            None => String::new(),
        };

        Some(FileItem {
            name,
            path: path.to_path_buf(),
            size,
            modified,
            is_dir,
            is_hidden,
            extension,
            cached_size,
            cached_date,
        })
    }

    pub fn formatted_ext(&self) -> &str {
        if self.is_dir {
            "<DIR>"
        } else {
            &self.extension
        }
    }

    pub fn formatted_size(&self) -> &str {
        &self.cached_size
    }

    pub fn formatted_date(&self) -> &str {
        &self.cached_date
    }
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(windows)]
fn is_hidden_file(_path: &Path, name: &str) -> bool {
    use std::os::windows::fs::MetadataExt;
    if name.starts_with('.') {
        return true;
    }
    if let Ok(metadata) = _path.metadata() {
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
    } else {
        false
    }
}

#[cfg(not(windows))]
fn is_hidden_file(_path: &Path, name: &str) -> bool {
    name.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1), "1 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn format_size_kilobytes() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 1023), "1023.0 KB");
    }

    #[test]
    fn format_size_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 500), "500.0 MB");
    }

    #[test]
    fn format_size_gigabytes() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_size(1024 * 1024 * 1024 * 2), "2.0 GB");
    }

    fn make_dir(name: &str) -> FileItem {
        FileItem {
            name: name.to_string(),
            path: PathBuf::from(name),
            size: 0,
            modified: None,
            is_dir: true,
            is_hidden: false,
            extension: String::new(),
            cached_size: String::new(),
            cached_date: String::new(),
        }
    }

    #[test]
    fn file_item_formatted_ext_dir() {
        let item = make_dir("testdir");
        assert_eq!(item.formatted_ext(), "<DIR>");
    }

    #[test]
    fn file_item_formatted_size_dir() {
        let item = make_dir("testdir");
        assert_eq!(item.formatted_size(), "");
    }

    #[test]
    fn read_directory_no_dotdot() {
        let dir = std::env::current_dir().unwrap();
        let entries = read_directory(&dir);
        // ".." should not be included in directory listing
        assert!(entries.iter().all(|e| e.name != ".."));
    }
}

pub fn read_directory(dir: &Path) -> Vec<FileItem> {
    let mut entries = Vec::new();

    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            if let Some(item) = FileItem::from_path(&entry.path()) {
                entries.push(item);
            }
        }
    }

    entries
}

const RECURSIVE_SEARCH_LIMIT: usize = 100_000;
const RECURSIVE_BATCH_SIZE: usize = 500;

/// Recursively collect files under `root` in BFS order, streaming batches
/// into `sink`. Sets `done` to true when finished. Respects `show_hidden`.
pub fn read_directory_recursive_streaming(
    root: &Path,
    show_hidden: bool,
    sink: &Mutex<Vec<FileItem>>,
    done: &std::sync::atomic::AtomicBool,
    repaint: &egui::Context,
) {
    let mut batch = Vec::with_capacity(RECURSIVE_BATCH_SIZE);
    let mut total = 0usize;
    let mut queue = VecDeque::new();
    queue.push_back(root.to_path_buf());

    while let Some(dir) = queue.pop_front() {
        let Ok(read_dir) = std::fs::read_dir(&dir) else { continue };
        for entry in read_dir.flatten() {
            let path = entry.path();
            let Some(mut item) = FileItem::from_path(&path) else {
                continue;
            };
            if !show_hidden && item.is_hidden {
                continue;
            }
            if item.is_dir {
                queue.push_back(path);
                continue;
            }
            // Set name to relative path from root
            if let Ok(rel) = path.strip_prefix(root) {
                item.name = rel.to_string_lossy().replace('\\', "/");
            }
            batch.push(item);
            total += 1;
            if batch.len() >= RECURSIVE_BATCH_SIZE {
                sink.lock().append(&mut batch);
                repaint.request_repaint();
            }
            if total >= RECURSIVE_SEARCH_LIMIT {
                if !batch.is_empty() {
                    sink.lock().append(&mut batch);
                }
                done.store(true, std::sync::atomic::Ordering::Release);
                repaint.request_repaint();
                return;
            }
        }
    }

    if !batch.is_empty() {
        sink.lock().append(&mut batch);
    }
    done.store(true, std::sync::atomic::Ordering::Release);
    repaint.request_repaint();
}
