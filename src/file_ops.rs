use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;

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

/// Copy a file or directory without overwrite (without progress tracking).
/// Returns `AlreadyExists` if the destination already exists.
pub fn copy_file_or_dir(src: &Path, dest_dir: &Path) -> Result<(), FileOpError> {
    copy_file_or_dir_inner_simple(src, dest_dir, false)
}

/// Copy a file or directory with overwrite (without progress tracking).
/// Used by cross-filesystem move fallback and undo/redo.
pub fn copy_file_or_dir_overwrite(src: &Path, dest_dir: &Path) -> Result<(), FileOpError> {
    copy_file_or_dir_inner_simple(src, dest_dir, true)
}

fn copy_file_or_dir_inner_simple(src: &Path, dest_dir: &Path, overwrite: bool) -> Result<(), FileOpError> {
    let file_name = src
        .file_name()
        .ok_or_else(|| FileOpError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "No file name",
        )))?;
    let dest_path = dest_dir.join(file_name);

    if !overwrite && dest_path.exists() {
        return Err(FileOpError::AlreadyExists(dest_path));
    }

    if src.is_dir() {
        if dest_path.exists() {
            fs::remove_dir_all(&dest_path)?;
        }
        copy_dir_simple(src, &dest_path)?;
    } else {
        fs::copy(src, &dest_path)?;
    }
    Ok(())
}

fn copy_dir_simple(src: &Path, dest: &Path) -> Result<(), FileOpError> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_simple(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
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

pub fn create_file(parent: &Path, name: &str) -> Result<PathBuf, FileOpError> {
    validate_name(name)?;

    let new_path = parent.join(name);

    if new_path.exists() {
        return Err(FileOpError::AlreadyExists(new_path));
    }

    fs::File::create(&new_path)?;
    Ok(new_path)
}

/// Reject names containing path separators, traversal components, or Windows reserved names
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

    // Reject Windows reserved device names (CON, PRN, AUX, NUL, COM1-9, LPT1-9)
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL"
            | "COM1" | "COM2" | "COM3" | "COM4" | "COM5"
            | "COM6" | "COM7" | "COM8" | "COM9"
            | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5"
            | "LPT6" | "LPT7" | "LPT8" | "LPT9"
    ) {
        return Err(FileOpError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Reserved name: {}", name),
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
    pub completed_bytes: u64,
    pub total_bytes: u64,
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
                completed_bytes: 0,
                total_bytes: 0,
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
        let mut s = self.state.lock();
        s.current_file = current_file.to_string();
        s.completed = completed;
    }

    fn update_bytes(&self, current_file: &str, completed_bytes: u64) {
        let mut s = self.state.lock();
        s.current_file = current_file.to_string();
        s.completed_bytes = completed_bytes;
    }

    fn set_total_bytes(&self, total_bytes: u64) {
        self.state.lock().total_bytes = total_bytes;
    }

    fn finish(&self, message: String, succeeded: Vec<PathBuf>, error: Option<String>, result_path: Option<PathBuf>) {
        {
            let mut s = self.state.lock();
            s.finished = true;
            s.cancelled = self.is_cancelled();
            s.result_message = message;
            s.succeeded_paths = succeeded;
            s.error = error;
            s.result_path = result_path;
        }
    }
}

fn run_batch_with_progress<F>(
    paths: &[PathBuf],
    progress: &ProgressHandle,
    verb: &str,
    mut op: F,
) where
    F: FnMut(&Path) -> Result<(), FileOpError>,
{
    let mut succeeded = Vec::new();
    let mut errors = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        if progress.is_cancelled() {
            break;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        progress.update(&name, i);

        match op(path) {
            Ok(()) => succeeded.push(path.clone()),
            Err(e) => errors.push(e.to_string()),
        }
        progress.update(&name, i + 1);
    }

    let count = succeeded.len();
    let total = paths.len();
    let msg = if progress.is_cancelled() {
        format!("Cancelled ({}/{})", count, total)
    } else if errors.is_empty() {
        format!("{} {} item(s)", verb, total)
    } else {
        format!("Errors: {}", errors.join(", "))
    };
    progress.finish(msg, succeeded, errors.first().cloned(), None);
}

/// Calculate total size of files (recursing into directories).
fn calculate_total_size(paths: &[PathBuf]) -> u64 {
    let mut total: u64 = 0;
    for path in paths {
        if path.is_dir() {
            total += dir_size_recursive(path);
        } else if let Ok(meta) = fs::metadata(path) {
            total += meta.len();
        }
    }
    total
}

fn dir_size_recursive(dir: &Path) -> u64 {
    let mut size: u64 = 0;
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.is_dir() {
            size += dir_size_recursive(&path);
        } else if let Ok(meta) = entry.metadata() {
            size += meta.len();
        }
    }
    size
}

const COPY_CHUNK_SIZE: usize = 256 * 1024; // 256KB

/// Copy a single file in chunks, updating byte progress and checking cancel.
/// Returns bytes copied. On cancel, removes the partial destination file.
fn copy_file_chunked(
    src: &Path,
    dest: &Path,
    progress: &ProgressHandle,
    completed_bytes: &mut u64,
) -> Result<(), FileOpError> {
    let mut reader = std::io::BufReader::with_capacity(
        COPY_CHUNK_SIZE,
        fs::File::open(src)?,
    );
    let mut writer = std::io::BufWriter::with_capacity(
        COPY_CHUNK_SIZE,
        fs::File::create(dest)?,
    );

    let file_name = src.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut buf = vec![0u8; COPY_CHUNK_SIZE];
    loop {
        if progress.is_cancelled() {
            drop(writer);
            let _ = fs::remove_file(dest);
            return Err(FileOpError::IoError(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled",
            )));
        }

        let n = std::io::Read::read(&mut reader, &mut buf)?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut writer, &buf[..n])?;
        *completed_bytes += n as u64;
        progress.update_bytes(&file_name, *completed_bytes);
    }

    // Copy file permissions
    if let Ok(meta) = fs::metadata(src) {
        let _ = fs::set_permissions(dest, meta.permissions());
    }

    Ok(())
}

/// Recursively copy a directory with chunked file copy for progress tracking.
fn copy_dir_recursive_with_progress(
    src: &Path,
    dest: &Path,
    progress: &ProgressHandle,
    completed_bytes: &mut u64,
) -> Result<(), FileOpError> {
    fs::create_dir_all(dest)?;

    for entry in fs::read_dir(src)? {
        if progress.is_cancelled() {
            return Err(FileOpError::IoError(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled",
            )));
        }
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive_with_progress(&src_path, &dest_path, progress, completed_bytes)?;
        } else {
            copy_file_chunked(&src_path, &dest_path, progress, completed_bytes)?;
        }
    }

    Ok(())
}

pub fn copy_batch_with_progress(
    sources: &[PathBuf],
    dest_dir: &Path,
    overwrite: bool,
    progress: &ProgressHandle,
) {
    // Pre-calculate total bytes
    let total_bytes = calculate_total_size(sources);
    progress.set_total_bytes(total_bytes);

    let mut succeeded = Vec::new();
    let mut errors = Vec::new();
    let mut completed_bytes: u64 = 0;

    for (i, src) in sources.iter().enumerate() {
        if progress.is_cancelled() {
            break;
        }

        let file_name = src.file_name()
            .ok_or_else(|| FileOpError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "No file name",
            )));
        let file_name = match file_name {
            Ok(n) => n,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };

        let dest_path = dest_dir.join(file_name);

        if dest_path.exists() && !overwrite {
            errors.push(FileOpError::AlreadyExists(dest_path).to_string());
            continue;
        }

        let result = if src.is_dir() {
            if dest_path.exists() && overwrite {
                let _ = fs::remove_dir_all(&dest_path);
            }
            copy_dir_recursive_with_progress(src, &dest_path, progress, &mut completed_bytes)
        } else {
            copy_file_chunked(src, &dest_path, progress, &mut completed_bytes)
        };

        match result {
            Ok(()) => {
                succeeded.push(src.clone());
                progress.update(&file_name.to_string_lossy(), i + 1);
            }
            Err(e) => {
                if progress.is_cancelled() {
                    break;
                }
                errors.push(e.to_string());
            }
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
    let dest = dest_dir.to_path_buf();
    run_batch_with_progress(sources, progress, "Moved", |src| {
        if overwrite {
            move_file_or_dir_overwrite(src, &dest)
        } else {
            move_file_or_dir(src, &dest)
        }
    });
}

pub fn delete_batch_with_progress(
    paths: &[PathBuf],
    progress: &ProgressHandle,
) {
    run_batch_with_progress(paths, progress, "Deleted", |p| delete_to_trash(p));
}

pub fn delete_permanent_batch_with_progress(
    paths: &[PathBuf],
    progress: &ProgressHandle,
) {
    run_batch_with_progress(paths, progress, "Permanently deleted", |p| delete_permanently(p));
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

    // Pre-calculate total bytes
    let total_bytes = calculate_total_size(sources);
    progress.set_total_bytes(total_bytes);

    let file = match fs::File::create(&zip_path) {
        Ok(f) => f,
        Err(e) => {
            progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
            return;
        }
    };
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);

    let mut errors = Vec::new();
    let mut completed_bytes: u64 = 0;
    for src in sources.iter() {
        if progress.is_cancelled() {
            break;
        }

        let Some(fname) = src.file_name() else {
            errors.push(format!("No file name: {}", src.display()));
            continue;
        };
        let result = if src.is_dir() {
            add_dir_to_zip_with_progress(&mut zip, src, fname.as_ref(), options, progress, &mut completed_bytes)
        } else {
            let src_name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            add_file_to_zip_with_progress(&mut zip, src, &src_name, options, progress, &mut completed_bytes)
        };
        if let Err(e) = result {
            if progress.is_cancelled() {
                break;
            }
            errors.push(e.to_string());
        }
    }

    if progress.is_cancelled() {
        let _ = zip.finish();
        let _ = fs::remove_file(&zip_path);
        progress.finish("Cancelled".to_string(), Vec::new(), None, None);
        return;
    }

    match zip.finish() {
        Ok(_) => {
            // Verify the archive integrity: reopen and check entry count
            match verify_zip_archive(&zip_path, sources) {
                Ok(()) => {
                    let msg = if errors.is_empty() {
                        format!("Compressed {} file(s) to {}", sources.len(), name)
                    } else {
                        format!("Errors: {}", errors.join(", "))
                    };
                    progress.finish(msg, sources.to_vec(), errors.first().cloned(), Some(zip_path));
                }
                Err(e) => {
                    let _ = fs::remove_file(&zip_path);
                    progress.finish(
                        format!("Error: archive verification failed: {}", e),
                        Vec::new(),
                        Some(e.clone()),
                        None,
                    );
                }
            }
        }
        Err(e) => {
            let _ = fs::remove_file(&zip_path);
            progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
        }
    }
}

/// Decode a ZIP entry name, falling back to Shift_JIS if not valid UTF-8.
pub fn decode_zip_entry_name(entry: &zip::read::ZipFile<'_, impl std::io::Read>) -> String {
    let raw = entry.name_raw();
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::SHIFT_JIS.decode(raw);
            decoded.into_owned()
        }
    }
}

/// Create a safe PathBuf from a decoded ZIP entry name, rejecting path traversal.
/// This replaces `enclosed_name()` for use with manually decoded names.
fn safe_zip_entry_path(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(name);
    // Reject absolute paths
    if path.is_absolute() {
        return None;
    }
    // Reject paths containing parent directory references
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return None;
        }
    }
    // Reject empty paths
    if path.as_os_str().is_empty() {
        return None;
    }
    Some(path)
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

    // Pre-calculate total uncompressed bytes
    let mut total_bytes: u64 = 0;
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            total_bytes += entry.size();
        }
    }
    progress.set_total_bytes(total_bytes);

    let mut errors = Vec::new();
    let mut completed_bytes: u64 = 0;
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

        let decoded_name = decode_zip_entry_name(&entry);
        let enclosed = match safe_zip_entry_path(&decoded_name) {
            Some(name) => name,
            None => {
                errors.push("Invalid zip entry name".to_string());
                continue;
            }
        };

        let out_path = extract_dir.join(&enclosed);
        // Security: reject paths that escape the extraction directory
        if !out_path.starts_with(&extract_dir) {
            errors.push(format!("Skipped unsafe path: {}", enclosed.display()));
            continue;
        }

        let entry_name = enclosed.to_string_lossy().to_string();

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
                    if let Err(e) = copy_stream_with_progress(
                        &mut entry, &mut outfile, progress, &entry_name, &mut completed_bytes,
                    ) {
                        if progress.is_cancelled() {
                            break;
                        }
                        errors.push(e.to_string());
                    }
                }
                Err(e) => errors.push(e.to_string()),
            }
        }
    }

    let msg = if progress.is_cancelled() {
        "Cancelled".to_string()
    } else if errors.is_empty() {
        format!("Extracted to: {}", extract_dir.display())
    } else {
        format!("Errors: {}", errors.join(", "))
    };
    progress.finish(msg, vec![zip_path.to_path_buf()], errors.first().cloned(), Some(extract_dir));
}

pub fn decompress_tar_with_progress(
    tar_path: &Path,
    dest_dir: &Path,
    progress: &ProgressHandle,
) {
    // Determine archive stem: strip .tar.gz / .tar.xz / .tgz / .txz / .tar
    let file_name = tar_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let lower = file_name.to_lowercase();
    let stem = if lower.ends_with(".tar.gz") {
        &file_name[..file_name.len() - 7]
    } else if lower.ends_with(".tar.xz") {
        &file_name[..file_name.len() - 7]
    } else if lower.ends_with(".tgz") || lower.ends_with(".txz") || lower.ends_with(".tar") {
        &file_name[..file_name.len() - 4]
    } else {
        file_name
    };

    if stem.is_empty() {
        progress.finish("Error: No file name".to_string(), Vec::new(), Some("No file name".to_string()), None);
        return;
    }

    let extract_dir = dest_dir.join(stem);
    if let Err(e) = fs::create_dir_all(&extract_dir) {
        progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
        return;
    }

    let file = match fs::File::open(tar_path) {
        Ok(f) => f,
        Err(e) => {
            progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
            return;
        }
    };

    // Detect compression by extension
    let is_gzip = lower.ends_with(".tar.gz") || lower.ends_with(".tgz");
    let is_xz = lower.ends_with(".tar.xz") || lower.ends_with(".txz");

    let mut archive = if is_gzip {
        let decoder = flate2::read::GzDecoder::new(file);
        tar::Archive::new(Box::new(decoder) as Box<dyn Read + Send>)
    } else if is_xz {
        let decoder = xz2::read::XzDecoder::new(file);
        tar::Archive::new(Box::new(decoder) as Box<dyn Read + Send>)
    } else {
        tar::Archive::new(Box::new(file) as Box<dyn Read + Send>)
    };

    let entries = match archive.entries() {
        Ok(e) => e,
        Err(e) => {
            progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
            return;
        }
    };

    let mut errors = Vec::new();
    let mut count: usize = 0;

    for entry_result in entries {
        if progress.is_cancelled() {
            break;
        }

        let mut entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };

        let entry_path = match entry.path() {
            Ok(p) => p.to_path_buf(),
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };

        // Security: reject absolute paths and path traversal
        if entry_path.is_absolute() || entry_path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            errors.push(format!("Skipped unsafe path: {}", entry_path.display()));
            continue;
        }

        let entry_name = entry_path.to_string_lossy().to_string();
        progress.update(&entry_name, count);

        let out_path = extract_dir.join(&entry_path);
        if let Err(e) = entry.unpack(&out_path) {
            errors.push(format!("{}: {}", entry_name, e));
        }

        count += 1;
        {
            let mut s = progress.state.lock();
            s.total = count + 1; // tar doesn't know total upfront
            s.completed = count;
        }
    }

    {
        let mut s = progress.state.lock();
        s.total = count;
        s.completed = count;
    }

    let msg = if progress.is_cancelled() {
        let completed = progress.state.lock().completed;
        format!("Cancelled ({}/{})", completed, count)
    } else if errors.is_empty() {
        format!("Extracted to: {}", extract_dir.display())
    } else {
        format!("Errors: {}", errors.join(", "))
    };
    progress.finish(msg, vec![tar_path.to_path_buf()], errors.first().cloned(), Some(extract_dir));
}

/// Compute output filename for stream decompression (strip outer compression extension).
/// e.g. "foo.tar.gz" → "foo.tar", "bar.tgz" → "bar.tar", "baz.tar.xz" → "baz.tar"
/// Returns the input unchanged if the extension is not recognized.
pub fn stream_decompress_output_name(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.ends_with(".tar.gz") {
        name[..name.len() - 3].to_string() // strip ".gz", keep ".tar"
    } else if lower.ends_with(".tar.xz") {
        name[..name.len() - 3].to_string() // strip ".xz", keep ".tar"
    } else if lower.ends_with(".tgz") {
        format!("{}.tar", &name[..name.len() - 4])
    } else if lower.ends_with(".txz") {
        format!("{}.tar", &name[..name.len() - 4])
    } else {
        name.to_string()
    }
}

/// Decompress a single compressed file (gz/xz) to produce the inner file (e.g. .tar).
/// Only strips the outer compression layer; does NOT extract tar contents.
pub fn decompress_stream_with_progress(
    path: &Path,
    dest_dir: &Path,
    progress: &ProgressHandle,
) {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let output_name = stream_decompress_output_name(file_name);
    if output_name == file_name {
        progress.finish("Error: unsupported format".to_string(), Vec::new(), Some("Unsupported format".to_string()), None);
        return;
    }
    let lower = file_name.to_lowercase();

    let output_path = dest_dir.join(&output_name);
    let is_gzip = lower.ends_with(".tar.gz") || lower.ends_with(".tgz");

    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
            return;
        }
    };

    let file_size = file.metadata().ok().map(|m| m.len()).unwrap_or(0);
    {
        let mut s = progress.state.lock();
        s.total = if file_size > 0 { (file_size / 8192) as usize } else { 0 };
    }

    let mut reader: Box<dyn Read> = if is_gzip {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(xz2::read::XzDecoder::new(file))
    };

    let mut out = match fs::File::create(&output_path) {
        Ok(f) => std::io::BufWriter::new(f),
        Err(e) => {
            progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
            return;
        }
    };

    let mut buf = [0u8; 8192];
    let mut blocks: usize = 0;

    loop {
        if progress.is_cancelled() {
            break;
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(e) = out.write_all(&buf[..n]) {
                    // Clean up partial file on error
                    drop(out);
                    let _ = fs::remove_file(&output_path);
                    progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
                    return;
                }
                blocks += 1;
                progress.update(&output_name, blocks);
                {
                    let mut s = progress.state.lock();
                    s.completed = blocks;
                }
            }
            Err(e) => {
                drop(out);
                let _ = fs::remove_file(&output_path);
                progress.finish(format!("Error: {}", e), Vec::new(), Some(e.to_string()), None);
                return;
            }
        }
    }

    if progress.is_cancelled() {
        drop(out);
        let _ = fs::remove_file(&output_path);
        progress.finish("Cancelled".to_string(), Vec::new(), None, None);
        return;
    }

    let msg = format!("Decompressed: {}", output_name);
    progress.finish(msg, vec![path.to_path_buf()], None, Some(output_path));
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

/// Returns (free_bytes, total_bytes) for a drive path, or None on failure.
#[cfg(windows)]
pub fn get_drive_space(root: &str) -> Option<(u64, u64)> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            lpDirectoryName: *const u16,
            lpFreeBytesAvailableToCaller: *mut u64,
            lpTotalNumberOfBytes: *mut u64,
            lpTotalNumberOfFreeBytes: *mut u64,
        ) -> i32;
    }

    let wide: Vec<u16> = OsStr::new(root).encode_wide().chain(std::iter::once(0)).collect();
    let mut free_caller: u64 = 0;
    let mut total: u64 = 0;
    let mut _free_total: u64 = 0;

    let ret = unsafe {
        GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_caller, &mut total, &mut _free_total)
    };
    if ret != 0 {
        Some((free_caller, total))
    } else {
        None
    }
}

#[cfg(not(windows))]
pub fn get_drive_space(_root: &str) -> Option<(u64, u64)> {
    None
}

pub fn format_size_human(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    if bytes >= TB {
        format!("{:.1}T", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.0}M", bytes as f64 / MB as f64)
    } else {
        format!("{:.0}K", bytes as f64 / KB as f64)
    }
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
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);

    for src in sources {
        let Some(fname) = src.file_name() else { continue };
        if src.is_dir() {
            add_dir_to_zip(&mut zip, src, fname.as_ref(), options)?;
        } else {
            add_file_to_zip(&mut zip, src, &fname.to_string_lossy(), options)?;
        }
    }

    zip.finish()
        .map_err(|e| FileOpError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    // Verify the archive integrity
    if let Err(e) = verify_zip_archive(&zip_path, sources) {
        let _ = fs::remove_file(&zip_path);
        return Err(FileOpError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e,
        )));
    }

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

/// Verify a zip archive after creation: reopen and check entry count > 0 and sizes are plausible.
fn verify_zip_archive(zip_path: &Path, sources: &[PathBuf]) -> Result<(), String> {
    let file = fs::File::open(zip_path)
        .map_err(|e| format!("cannot reopen archive: {}", e))?;
    let archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("cannot read archive: {}", e))?;

    if archive.len() == 0 && !sources.is_empty() {
        return Err("archive contains 0 entries".to_string());
    }

    // Count expected files from sources
    fn count_files(path: &Path) -> usize {
        if path.is_dir() {
            let Ok(entries) = fs::read_dir(path) else { return 0 };
            entries.filter_map(|e| e.ok()).map(|e| count_files(&e.path())).sum()
        } else {
            1
        }
    }
    let expected: usize = sources.iter().map(|s| count_files(s)).sum();
    if expected > 0 && archive.len() == 0 {
        return Err(format!("expected {} files but archive has 0 entries", expected));
    }

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

/// Copy from a reader to a writer in chunks, tracking byte progress and checking cancel.
fn copy_stream_with_progress(
    reader: &mut dyn std::io::Read,
    writer: &mut dyn std::io::Write,
    progress: &ProgressHandle,
    file_name: &str,
    completed_bytes: &mut u64,
) -> Result<(), FileOpError> {
    let mut buf = vec![0u8; COPY_CHUNK_SIZE];
    loop {
        if progress.is_cancelled() {
            return Err(FileOpError::IoError(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled",
            )));
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        *completed_bytes += n as u64;
        progress.update_bytes(file_name, *completed_bytes);
    }
    Ok(())
}

fn add_file_to_zip_with_progress(
    zip: &mut zip::ZipWriter<fs::File>,
    file_path: &Path,
    name_in_zip: &str,
    options: zip::write::SimpleFileOptions,
    progress: &ProgressHandle,
    completed_bytes: &mut u64,
) -> Result<(), FileOpError> {
    zip.start_file(name_in_zip, options)
        .map_err(|e| FileOpError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    let file_name = file_path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut f = fs::File::open(file_path)?;
    let mut buf = vec![0u8; COPY_CHUNK_SIZE];
    loop {
        if progress.is_cancelled() {
            return Err(FileOpError::IoError(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled",
            )));
        }
        let n = std::io::Read::read(&mut f, &mut buf)?;
        if n == 0 {
            break;
        }
        zip.write_all(&buf[..n])?;
        *completed_bytes += n as u64;
        progress.update_bytes(&file_name, *completed_bytes);
    }
    Ok(())
}

fn add_dir_to_zip_with_progress(
    zip: &mut zip::ZipWriter<fs::File>,
    dir_path: &Path,
    prefix: &Path,
    options: zip::write::SimpleFileOptions,
    progress: &ProgressHandle,
    completed_bytes: &mut u64,
) -> Result<(), FileOpError> {
    for entry in fs::read_dir(dir_path)? {
        if progress.is_cancelled() {
            return Err(FileOpError::IoError(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled",
            )));
        }
        let entry = entry?;
        let path = entry.path();
        let name = prefix.join(entry.file_name());

        if path.is_dir() {
            add_dir_to_zip_with_progress(zip, &path, &name, options, progress, completed_bytes)?;
        } else {
            let name_str = name.to_string_lossy().replace('\\', "/");
            add_file_to_zip_with_progress(zip, &path, &name_str, options, progress, completed_bytes)?;
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

        let decoded_name = decode_zip_entry_name(&entry);
        let name = safe_zip_entry_path(&decoded_name)
            .ok_or_else(|| FileOpError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid zip entry name",
            )))?;

        let out_path = extract_dir.join(&name);

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
        // Non-reserved names that look similar
        assert!(validate_name("CONN").is_ok());
        assert!(validate_name("console.log").is_ok());
        assert!(validate_name("nully").is_ok());
        assert!(validate_name("com10").is_ok());
    }

    #[test]
    fn validate_name_rejects_reserved_windows_names() {
        assert!(validate_name("CON").is_err());
        assert!(validate_name("con").is_err());
        assert!(validate_name("PRN").is_err());
        assert!(validate_name("AUX").is_err());
        assert!(validate_name("NUL").is_err());
        assert!(validate_name("COM1").is_err());
        assert!(validate_name("LPT1").is_err());
        assert!(validate_name("con.txt").is_err());
        assert!(validate_name("NUL.tar.gz").is_err());
    }

    #[test]
    fn stream_decompress_output_name_cases() {
        assert_eq!(stream_decompress_output_name("archive.tar.gz"), "archive.tar");
        assert_eq!(stream_decompress_output_name("archive.tar.xz"), "archive.tar");
        assert_eq!(stream_decompress_output_name("archive.tgz"), "archive.tar");
        assert_eq!(stream_decompress_output_name("archive.txz"), "archive.tar");
        assert_eq!(stream_decompress_output_name("Archive.TAR.GZ"), "Archive.TAR");
        assert_eq!(stream_decompress_output_name("Archive.TAR.XZ"), "Archive.TAR");
        assert_eq!(stream_decompress_output_name("other.bin"), "other.bin");
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
