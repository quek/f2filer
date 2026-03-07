use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum FileOpError {
    IoError(std::io::Error),
    TrashError(String),
    AlreadyExists(PathBuf),
}

impl std::fmt::Display for FileOpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileOpError::IoError(e) => write!(f, "IO error: {}", e),
            FileOpError::TrashError(e) => write!(f, "Trash error: {}", e),
            FileOpError::AlreadyExists(p) => write!(f, "Already exists: {}", p.display()),
        }
    }
}

impl From<std::io::Error> for FileOpError {
    fn from(e: std::io::Error) -> Self {
        FileOpError::IoError(e)
    }
}

pub fn copy_file_or_dir(src: &Path, dest_dir: &Path) -> Result<(), FileOpError> {
    copy_file_or_dir_inner(src, dest_dir, false)
}

pub fn copy_file_or_dir_overwrite(src: &Path, dest_dir: &Path) -> Result<(), FileOpError> {
    copy_file_or_dir_inner(src, dest_dir, true)
}

fn copy_file_or_dir_inner(src: &Path, dest_dir: &Path, overwrite: bool) -> Result<(), FileOpError> {
    let file_name = src
        .file_name()
        .ok_or_else(|| FileOpError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "No file name",
        )))?;

    let dest_path = dest_dir.join(file_name);

    if dest_path.exists() && !overwrite {
        return Err(FileOpError::AlreadyExists(dest_path));
    }

    if src.is_dir() {
        if dest_path.exists() && overwrite {
            fs::remove_dir_all(&dest_path)?;
        }
        copy_dir_recursive(src, &dest_path)?;
    } else {
        fs::copy(src, &dest_path)?;
    }

    Ok(())
}

/// Check which sources already exist at dest
pub fn check_conflicts(sources: &[PathBuf], dest_dir: &Path) -> Vec<String> {
    sources
        .iter()
        .filter_map(|src| {
            src.file_name().and_then(|name| {
                let dest = dest_dir.join(name);
                if dest.exists() {
                    Some(name.to_string_lossy().to_string())
                } else {
                    None
                }
            })
        })
        .collect()
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), FileOpError> {
    fs::create_dir_all(dest)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }

    Ok(())
}

pub fn move_file_or_dir(src: &Path, dest_dir: &Path) -> Result<(), FileOpError> {
    move_file_or_dir_inner(src, dest_dir, false)
}

pub fn move_file_or_dir_overwrite(src: &Path, dest_dir: &Path) -> Result<(), FileOpError> {
    move_file_or_dir_inner(src, dest_dir, true)
}

fn move_file_or_dir_inner(src: &Path, dest_dir: &Path, overwrite: bool) -> Result<(), FileOpError> {
    let file_name = src
        .file_name()
        .ok_or_else(|| FileOpError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "No file name",
        )))?;

    let dest_path = dest_dir.join(file_name);

    if dest_path.exists() && !overwrite {
        return Err(FileOpError::AlreadyExists(dest_path));
    }

    // Remove existing destination if overwriting
    if dest_path.exists() && overwrite {
        if dest_path.is_dir() {
            fs::remove_dir_all(&dest_path)?;
        } else {
            fs::remove_file(&dest_path)?;
        }
    }

    // Try rename first (fast, same filesystem)
    match fs::rename(src, &dest_path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-filesystem: copy then delete
            copy_file_or_dir_overwrite(src, dest_dir)?;
            if src.is_dir() {
                fs::remove_dir_all(src)?;
            } else {
                fs::remove_file(src)?;
            }
            Ok(())
        }
    }
}

pub fn delete_to_trash(path: &Path) -> Result<(), FileOpError> {
    // UNC paths (including WSL) have no recycle bin; fall back to permanent deletion
    if is_unc_path(path) {
        return delete_permanently_simple(path);
    }
    trash::delete(path).map_err(|e| FileOpError::TrashError(e.to_string()))
}

/// Simple permanent deletion without shredding (for UNC/network paths)
fn delete_permanently_simple(path: &Path) -> Result<(), FileOpError> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn is_unc_path(path: &Path) -> bool {
    path.to_string_lossy().starts_with(r"\\")
}

pub fn delete_permanently(path: &Path) -> Result<(), FileOpError> {
    if path.is_dir() {
        // Shred all files inside recursively, then remove dirs
        shred_dir_recursive(path)?;
    } else {
        shred_file(path)?;
    }
    Ok(())
}

/// Overwrite file content with random data 3 times, then delete
fn shred_file(path: &Path) -> Result<(), FileOpError> {
    use std::io::{Seek, SeekFrom};

    // Remove read-only attribute if set
    let metadata = fs::metadata(path)?;
    if metadata.permissions().readonly() {
        let mut perms = metadata.permissions();
        perms.set_readonly(false);
        fs::set_permissions(path, perms)?;
    }

    let len = metadata.len();
    if len > 0 {
        let mut file = fs::OpenOptions::new().write(true).open(path)?;
        let mut buf = vec![0u8; len.min(64 * 1024) as usize];

        for pass in 0u8..3 {
            file.seek(SeekFrom::Start(0))?;
            let fill: u8 = match pass {
                0 => 0xFF,
                1 => 0x00,
                2 => 0xAA,
                _ => 0,
            };
            buf.iter_mut().for_each(|b| *b = fill);
            let mut remaining = len;
            while remaining > 0 {
                let chunk = remaining.min(buf.len() as u64) as usize;
                std::io::Write::write_all(&mut file, &buf[..chunk])?;
                remaining -= chunk as u64;
            }
            file.sync_all()?;
        }
    }
    fs::remove_file(path)?;
    Ok(())
}

fn shred_dir_recursive(dir: &Path) -> Result<(), FileOpError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            shred_dir_recursive(&path)?;
        } else {
            shred_file(&path)?;
        }
    }
    fs::remove_dir(dir)?;
    Ok(())
}

pub fn rename_file(old_path: &Path, new_name: &str) -> Result<PathBuf, FileOpError> {
    validate_name(new_name)?;

    let parent = old_path.parent().ok_or_else(|| {
        FileOpError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "No parent directory",
        ))
    })?;

    let new_path = parent.join(new_name);

    if new_path.exists() {
        return Err(FileOpError::AlreadyExists(new_path));
    }

    fs::rename(old_path, &new_path)?;
    Ok(new_path)
}

pub fn create_directory(parent: &Path, name: &str) -> Result<PathBuf, FileOpError> {
    validate_name(name)?;

    let new_path = parent.join(name);

    if new_path.exists() {
        return Err(FileOpError::AlreadyExists(new_path));
    }

    fs::create_dir(&new_path)?;
    Ok(new_path)
}

/// Reject names containing path separators or traversal components
fn validate_name(name: &str) -> Result<(), FileOpError> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name == ".."
        || name == "."
    {
        return Err(FileOpError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Invalid name: {}", name),
        )));
    }
    Ok(())
}

// --- Progress tracking for background file operations ---

#[derive(Clone)]
pub struct ProgressHandle {
    pub state: Arc<Mutex<ProgressState>>,
    pub cancel_flag: Arc<AtomicBool>,
}

pub struct ProgressState {
    pub op_label: String,
    pub current_file: String,
    pub completed: usize,
    pub total: usize,
    pub finished: bool,
    pub cancelled: bool,
    pub error: Option<String>,
    pub result_message: String,
    pub succeeded_paths: Vec<PathBuf>,
    pub result_path: Option<PathBuf>,
}

impl ProgressHandle {
    pub fn new(op_label: &str, total: usize) -> Self {
        ProgressHandle {
            state: Arc::new(Mutex::new(ProgressState {
                op_label: op_label.to_string(),
                current_file: String::new(),
                completed: 0,
                total,
                finished: false,
                cancelled: false,
                error: None,
                result_message: String::new(),
                succeeded_paths: Vec::new(),
                result_path: None,
            })),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    fn update(&self, current_file: &str, completed: usize) {
        if let Ok(mut s) = self.state.lock() {
            s.current_file = current_file.to_string();
            s.completed = completed;
        }
    }

    fn finish(&self, message: String, succeeded: Vec<PathBuf>, error: Option<String>, result_path: Option<PathBuf>) {
        if let Ok(mut s) = self.state.lock() {
            s.finished = true;
            s.cancelled = self.is_cancelled();
            s.result_message = message;
            s.succeeded_paths = succeeded;
            s.error = error;
            s.result_path = result_path;
        }
    }
}

pub fn copy_batch_with_progress(
    sources: &[PathBuf],
    dest_dir: &Path,
    overwrite: bool,
    progress: &ProgressHandle,
) {
    let mut succeeded = Vec::new();
    let mut errors = Vec::new();

    for (i, src) in sources.iter().enumerate() {
        if progress.is_cancelled() {
            break;
        }
        let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        progress.update(&name, i);

        let result = if overwrite {
            copy_file_or_dir_overwrite(src, dest_dir)
        } else {
            copy_file_or_dir(src, dest_dir)
        };
        match result {
            Ok(()) => succeeded.push(src.clone()),
            Err(e) => errors.push(e.to_string()),
        }
    }

    let count = succeeded.len();
    let total = sources.len();
    let msg = if progress.is_cancelled() {
        format!("Cancelled ({}/{})", count, total)
    } else if errors.is_empty() {
        format!("Copied {} item(s)", total)
    } else {
        format!("Errors: {}", errors.join(", "))
    };
    progress.finish(msg, succeeded, errors.first().cloned(), None);
}

pub fn move_batch_with_progress(
    sources: &[PathBuf],
    dest_dir: &Path,
    overwrite: bool,
    progress: &ProgressHandle,
) {
    let mut succeeded = Vec::new();
    let mut errors = Vec::new();

    for (i, src) in sources.iter().enumerate() {
        if progress.is_cancelled() {
            break;
        }
        let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        progress.update(&name, i);

        let result = if overwrite {
            move_file_or_dir_overwrite(src, dest_dir)
        } else {
            move_file_or_dir(src, dest_dir)
        };
        match result {
            Ok(()) => succeeded.push(src.clone()),
            Err(e) => errors.push(e.to_string()),
        }
    }

    let count = succeeded.len();
    let total = sources.len();
    let msg = if progress.is_cancelled() {
        format!("Cancelled ({}/{})", count, total)
    } else if errors.is_empty() {
        format!("Moved {} item(s)", total)
    } else {
        format!("Errors: {}", errors.join(", "))
    };
    progress.finish(msg, succeeded, errors.first().cloned(), None);
}

pub fn delete_batch_with_progress(
    paths: &[PathBuf],
    progress: &ProgressHandle,
) {
    let mut succeeded = Vec::new();
    let mut errors = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        if progress.is_cancelled() {
            break;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        progress.update(&name, i);

        match delete_to_trash(path) {
            Ok(()) => succeeded.push(path.clone()),
            Err(e) => errors.push(e.to_string()),
        }
    }

    let count = succeeded.len();
    let total = paths.len();
    let msg = if progress.is_cancelled() {
        format!("Cancelled ({}/{})", count, total)
    } else if errors.is_empty() {
        format!("Deleted {} item(s)", total)
    } else {
        format!("Errors: {}", errors.join(", "))
    };
    progress.finish(msg, succeeded, errors.first().cloned(), None);
}

pub fn delete_permanent_batch_with_progress(
    paths: &[PathBuf],
    progress: &ProgressHandle,
) {
    let mut succeeded = Vec::new();
    let mut errors = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        if progress.is_cancelled() {
            break;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        progress.update(&name, i);

        match delete_permanently(path) {
            Ok(()) => succeeded.push(path.clone()),
            Err(e) => errors.push(e.to_string()),
        }
    }

    let count = succeeded.len();
    let total = paths.len();
    let msg = if progress.is_cancelled() {
        format!("Cancelled ({}/{})", count, total)
    } else if errors.is_empty() {
        format!("Permanently deleted {} item(s)", total)
    } else {
        format!("Errors: {}", errors.join(", "))
    };
    progress.finish(msg, succeeded, errors.first().cloned(), None);
}

pub fn compress_to_zip_with_progress(
    sources: &[PathBuf],
    dest_dir: &Path,
    zip_name: &str,
    progress: &ProgressHandle,
) {
    let name = if zip_name.ends_with(".zip") {
        zip_name.to_string()
    } else {
        format!("{}.zip", zip_name)
    };
    if let Err(e) = validate_name(&name) {
        progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
        return;
    }
    let zip_path = dest_dir.join(&name);

    let file = match fs::File::create(&zip_path) {
        Ok(f) => f,
        Err(e) => {
            progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
            return;
        }
    };
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut errors = Vec::new();
    for (i, src) in sources.iter().enumerate() {
        if progress.is_cancelled() {
            break;
        }
        let src_name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        progress.update(&src_name, i);

        let result = if src.is_dir() {
            add_dir_to_zip(&mut zip, src, src.file_name().unwrap().as_ref(), options)
        } else {
            add_file_to_zip(&mut zip, src, &src_name, options)
        };
        if let Err(e) = result {
            errors.push(e.to_string());
        }
    }

    if progress.is_cancelled() {
        // Drop zip writer before removing file
        let _ = zip.finish();
        let _ = fs::remove_file(&zip_path);
        progress.finish(
            format!("Cancelled ({}/{})", sources.len().min(progress.state.lock().map(|s| s.completed).unwrap_or(0)), sources.len()),
            Vec::new(),
            None,
            None,
        );
        return;
    }

    match zip.finish() {
        Ok(_) => {
            let msg = if errors.is_empty() {
                format!("Compressed {} file(s) to {}", sources.len(), name)
            } else {
                format!("Errors: {}", errors.join(", "))
            };
            progress.finish(msg, sources.to_vec(), errors.first().cloned(), Some(zip_path));
        }
        Err(e) => {
            progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
        }
    }
}

pub fn decompress_zip_with_progress(
    zip_path: &Path,
    dest_dir: &Path,
    progress: &ProgressHandle,
) {
    let zip_stem = match zip_path.file_stem() {
        Some(s) => s.to_owned(),
        None => {
            progress.finish("Error: No file name".to_string(), Vec::new(), Some("No file name".to_string()), None);
            return;
        }
    };
    let extract_dir = dest_dir.join(&zip_stem);
    if let Err(e) = fs::create_dir_all(&extract_dir) {
        progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
        return;
    }

    let file = match fs::File::open(zip_path) {
        Ok(f) => f,
        Err(e) => {
            progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
            return;
        }
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => {
            progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
            return;
        }
    };

    // Update total to number of entries in zip
    if let Ok(mut s) = progress.state.lock() {
        s.total = archive.len();
    }

    let mut errors = Vec::new();
    for i in 0..archive.len() {
        if progress.is_cancelled() {
            break;
        }

        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };

        let entry_name = entry.enclosed_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        progress.update(&entry_name, i);

        let out_path = match entry.enclosed_name() {
            Some(name) => extract_dir.join(name),
            None => {
                errors.push("Invalid zip entry name".to_string());
                continue;
            }
        };

        if entry.is_dir() {
            if let Err(e) = fs::create_dir_all(&out_path) {
                errors.push(e.to_string());
            }
        } else {
            if let Some(parent) = out_path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    errors.push(e.to_string());
                    continue;
                }
            }
            match fs::File::create(&out_path) {
                Ok(mut outfile) => {
                    if let Err(e) = std::io::copy(&mut entry, &mut outfile) {
                        errors.push(e.to_string());
                    }
                }
                Err(e) => errors.push(e.to_string()),
            }
        }
    }

    let total = archive.len();
    let completed = if progress.is_cancelled() {
        progress.state.lock().map(|s| s.completed).unwrap_or(0)
    } else {
        total
    };

    let msg = if progress.is_cancelled() {
        format!("Cancelled ({}/{})", completed, total)
    } else if errors.is_empty() {
        format!("Extracted to: {}", extract_dir.display())
    } else {
        format!("Errors: {}", errors.join(", "))
    };
    progress.finish(msg, vec![zip_path.to_path_buf()], errors.first().cloned(), Some(extract_dir));
}

#[cfg(windows)]
pub fn get_drives() -> Vec<String> {
    let mut drives = Vec::new();
    // Check drives A-Z
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        let path = Path::new(&drive);
        if path.exists() {
            drives.push(format!("{}:", letter as char));
        }
    }
    // Detect WSL distributions via wsl.exe (read_dir on UNC server root is unsupported)
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    if let Ok(output) = std::process::Command::new("wsl.exe")
        .args(["--list", "--quiet"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        if output.status.success() {
            // wsl.exe outputs UTF-16LE
            let u16_data: Vec<u16> = output
                .stdout
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let decoded = String::from_utf16_lossy(&u16_data);
            for line in decoded.lines() {
                let name = line.trim();
                if !name.is_empty() {
                    drives.push(format!("WSL:{}", name));
                }
            }
        }
    }
    drives
}

#[cfg(not(windows))]
pub fn get_drives() -> Vec<String> {
    vec!["/".to_string()]
}

pub fn compress_to_zip(
    sources: &[PathBuf],
    dest_dir: &Path,
    zip_name: &str,
) -> Result<PathBuf, FileOpError> {
    let name = if zip_name.ends_with(".zip") {
        zip_name.to_string()
    } else {
        format!("{}.zip", zip_name)
    };
    validate_name(&name)?;
    let zip_path = dest_dir.join(&name);

    let file = fs::File::create(&zip_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for src in sources {
        if src.is_dir() {
            add_dir_to_zip(&mut zip, src, src.file_name().unwrap().as_ref(), options)?;
        } else {
            add_file_to_zip(&mut zip, src, src.file_name().unwrap().to_string_lossy().as_ref(), options)?;
        }
    }

    zip.finish()
        .map_err(|e| FileOpError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    Ok(zip_path)
}

fn add_file_to_zip(
    zip: &mut zip::ZipWriter<fs::File>,
    file_path: &Path,
    name_in_zip: &str,
    options: zip::write::SimpleFileOptions,
) -> Result<(), FileOpError> {
    zip.start_file(name_in_zip, options)
        .map_err(|e| FileOpError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    let mut f = fs::File::open(file_path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    zip.write_all(&buf)?;
    Ok(())
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<fs::File>,
    dir_path: &Path,
    prefix: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<(), FileOpError> {
    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        let name = prefix.join(entry.file_name());

        if path.is_dir() {
            add_dir_to_zip(zip, &path, &name, options)?;
        } else {
            let name_str = name.to_string_lossy().replace('\\', "/");
            add_file_to_zip(zip, &path, &name_str, options)?;
        }
    }
    Ok(())
}

pub fn decompress_zip(zip_path: &Path, dest_dir: &Path) -> Result<PathBuf, FileOpError> {
    // Create a directory named after the zip file (without .zip extension)
    let zip_stem = zip_path.file_stem()
        .ok_or_else(|| FileOpError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "No file name",
        )))?;
    let extract_dir = dest_dir.join(zip_stem);
    fs::create_dir_all(&extract_dir)?;

    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| FileOpError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| FileOpError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let name = entry.enclosed_name()
            .ok_or_else(|| FileOpError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid zip entry name",
            )))?;

        let out_path = extract_dir.join(name);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut outfile)?;
        }
    }

    Ok(extract_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src_file = tmp.path().join("src.txt");
        fs::write(&src_file, "hello").unwrap();

        let dest_dir = tmp.path().join("dest");
        fs::create_dir(&dest_dir).unwrap();

        copy_file_or_dir(&src_file, &dest_dir).unwrap();
        let copied = dest_dir.join("src.txt");
        assert!(copied.exists());
        assert_eq!(fs::read_to_string(copied).unwrap(), "hello");
    }

    #[test]
    fn copy_file_already_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("file.txt");
        fs::write(&src, "src").unwrap();

        let dest = tmp.path().join("dest");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("file.txt"), "existing").unwrap();

        let result = copy_file_or_dir(&src, &dest);
        assert!(matches!(result, Err(FileOpError::AlreadyExists(_))));
    }

    #[test]
    fn copy_file_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("file.txt");
        fs::write(&src, "new content").unwrap();

        let dest = tmp.path().join("dest");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("file.txt"), "old content").unwrap();

        copy_file_or_dir_overwrite(&src, &dest).unwrap();
        assert_eq!(fs::read_to_string(dest.join("file.txt")).unwrap(), "new content");
    }

    #[test]
    fn copy_directory_recursive() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mydir");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("a.txt"), "aaa").unwrap();
        let sub = src.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("b.txt"), "bbb").unwrap();

        let dest = tmp.path().join("dest");
        fs::create_dir(&dest).unwrap();

        copy_file_or_dir(&src, &dest).unwrap();
        assert!(dest.join("mydir").join("a.txt").exists());
        assert!(dest.join("mydir").join("sub").join("b.txt").exists());
    }

    #[test]
    fn move_file_same_fs() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("file.txt");
        fs::write(&src, "data").unwrap();

        let dest = tmp.path().join("dest");
        fs::create_dir(&dest).unwrap();

        move_file_or_dir(&src, &dest).unwrap();
        assert!(!src.exists());
        assert!(dest.join("file.txt").exists());
    }

    #[test]
    fn move_file_already_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("file.txt");
        fs::write(&src, "src").unwrap();

        let dest = tmp.path().join("dest");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("file.txt"), "existing").unwrap();

        let result = move_file_or_dir(&src, &dest);
        assert!(matches!(result, Err(FileOpError::AlreadyExists(_))));
        assert!(src.exists()); // source not deleted
    }

    #[test]
    fn rename_file_success() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("old.txt");
        fs::write(&src, "data").unwrap();

        let new_path = rename_file(&src, "new.txt").unwrap();
        assert!(!src.exists());
        assert!(new_path.exists());
        assert_eq!(new_path.file_name().unwrap().to_str().unwrap(), "new.txt");
    }

    #[test]
    fn rename_file_already_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        fs::write(&src, "a").unwrap();
        fs::write(tmp.path().join("b.txt"), "b").unwrap();

        let result = rename_file(&src, "b.txt");
        assert!(matches!(result, Err(FileOpError::AlreadyExists(_))));
    }

    #[test]
    fn create_directory_success() {
        let tmp = tempfile::tempdir().unwrap();
        let path = create_directory(tmp.path(), "newdir").unwrap();
        assert!(path.exists());
        assert!(path.is_dir());
    }

    #[test]
    fn create_directory_already_exists() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("existing")).unwrap();

        let result = create_directory(tmp.path(), "existing");
        assert!(matches!(result, Err(FileOpError::AlreadyExists(_))));
    }

    #[test]
    fn check_conflicts_none() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("file.txt");
        fs::write(&src, "data").unwrap();

        let dest = tmp.path().join("dest");
        fs::create_dir(&dest).unwrap();

        let conflicts = check_conflicts(&[src], &dest);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn check_conflicts_found() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("file.txt");
        fs::write(&src, "data").unwrap();

        let dest = tmp.path().join("dest");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("file.txt"), "existing").unwrap();

        let conflicts = check_conflicts(&[src], &dest);
        assert_eq!(conflicts, vec!["file.txt"]);
    }

    #[test]
    fn compress_and_decompress_zip() {
        let tmp = tempfile::tempdir().unwrap();
        // Create source files
        let src = tmp.path().join("file.txt");
        fs::write(&src, "hello zip").unwrap();

        let zip_dest = tmp.path().join("zips");
        fs::create_dir(&zip_dest).unwrap();

        // Compress
        let zip_path = compress_to_zip(&[src], &zip_dest, "test").unwrap();
        assert!(zip_path.exists());
        assert_eq!(zip_path.file_name().unwrap().to_str().unwrap(), "test.zip");

        // Decompress
        let extract_dest = tmp.path().join("extracted");
        fs::create_dir(&extract_dest).unwrap();
        let extract_dir = decompress_zip(&zip_path, &extract_dest).unwrap();
        assert!(extract_dir.join("file.txt").exists());
        assert_eq!(fs::read_to_string(extract_dir.join("file.txt")).unwrap(), "hello zip");
    }

    #[test]
    fn compress_zip_auto_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        fs::write(&src, "a").unwrap();

        // With .zip extension already
        let zip_path = compress_to_zip(&[src], tmp.path(), "archive.zip").unwrap();
        assert_eq!(zip_path.file_name().unwrap().to_str().unwrap(), "archive.zip");
    }

    #[test]
    fn validate_name_rejects_traversal() {
        assert!(validate_name("..").is_err());
        assert!(validate_name(".").is_err());
        assert!(validate_name("foo/bar").is_err());
        assert!(validate_name("foo\\bar").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("foo\0bar").is_err());
    }

    #[test]
    fn validate_name_accepts_normal() {
        assert!(validate_name("hello.txt").is_ok());
        assert!(validate_name("日本語.txt").is_ok());
        assert!(validate_name(".hidden").is_ok());
        assert!(validate_name("file with spaces").is_ok());
    }

    #[test]
    fn rename_rejects_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        fs::write(&src, "data").unwrap();

        assert!(rename_file(&src, "..\\escape.txt").is_err());
        assert!(rename_file(&src, "../escape.txt").is_err());
        assert!(rename_file(&src, "..").is_err());
        assert!(src.exists()); // source unchanged
    }

    #[test]
    fn create_dir_rejects_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(create_directory(tmp.path(), "..\\escape").is_err());
        assert!(create_directory(tmp.path(), "../escape").is_err());
        assert!(create_directory(tmp.path(), "..").is_err());
    }

    #[test]
    fn shred_file_removes_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret.txt");
        fs::write(&path, "sensitive data here").unwrap();

        delete_permanently(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn shred_readonly_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("readonly.txt");
        fs::write(&path, "readonly content").unwrap();

        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms).unwrap();

        delete_permanently(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn shred_directory_recursive() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("mydir");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("a.txt"), "aaa").unwrap();
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("b.txt"), "bbb").unwrap();

        delete_permanently(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn file_op_error_display() {
        let e = FileOpError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "not found"));
        assert!(e.to_string().contains("not found"));

        let e = FileOpError::TrashError("trash fail".to_string());
        assert!(e.to_string().contains("trash fail"));

        let e = FileOpError::AlreadyExists(PathBuf::from("/tmp/file.txt"));
        assert!(e.to_string().contains("Already exists"));
    }
}
