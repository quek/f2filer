use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct FileItem {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub is_dir: bool,
    pub is_hidden: bool,
    pub extension: String,
    pub is_symlink: bool,
}

impl FileItem {
    pub fn from_path(path: &Path) -> Option<Self> {
        let metadata = path.symlink_metadata().ok()?;
        let name = path.file_name()?.to_string_lossy().to_string();
        let is_dir = metadata.is_dir();
        let is_symlink = metadata.is_symlink();
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

        Some(FileItem {
            name,
            path: path.to_path_buf(),
            size,
            modified,
            is_dir,
            is_hidden,
            extension,
            is_symlink,
        })
    }

    pub fn parent_entry(parent_path: PathBuf) -> Self {
        FileItem {
            name: "..".to_string(),
            path: parent_path,
            size: 0,
            modified: None,
            is_dir: true,
            is_hidden: false,
            extension: String::new(),
            is_symlink: false,
        }
    }

    pub fn formatted_size(&self) -> String {
        if self.is_dir {
            "<DIR>".to_string()
        } else {
            format_size(self.size)
        }
    }

    pub fn formatted_date(&self) -> String {
        match self.modified {
            Some(time) => {
                let datetime: chrono::DateTime<chrono::Local> = time.into();
                datetime.format("%Y-%m-%d %H:%M").to_string()
            }
            None => String::new(),
        }
    }
}

fn format_size(bytes: u64) -> String {
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

pub fn read_directory(dir: &Path) -> Vec<FileItem> {
    let mut entries = Vec::new();

    if let Some(parent) = dir.parent() {
        entries.push(FileItem::parent_entry(parent.to_path_buf()));
    }

    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            if let Some(item) = FileItem::from_path(&entry.path()) {
                entries.push(item);
            }
        }
    }

    entries
}
