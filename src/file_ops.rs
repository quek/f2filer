use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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
    trash::delete(path).map_err(|e| FileOpError::TrashError(e.to_string()))
}

pub fn rename_file(old_path: &Path, new_name: &str) -> Result<PathBuf, FileOpError> {
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
    let new_path = parent.join(name);

    if new_path.exists() {
        return Err(FileOpError::AlreadyExists(new_path));
    }

    fs::create_dir(&new_path)?;
    Ok(new_path)
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
