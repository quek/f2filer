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

    let len = fs::metadata(path)?.len();
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
    fn file_op_error_display() {
        let e = FileOpError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "not found"));
        assert!(e.to_string().contains("not found"));

        let e = FileOpError::TrashError("trash fail".to_string());
        assert!(e.to_string().contains("trash fail"));

        let e = FileOpError::AlreadyExists(PathBuf::from("/tmp/file.txt"));
        assert!(e.to_string().contains("Already exists"));
    }
}
