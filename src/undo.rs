use std::path::PathBuf;

use crate::file_ops;

/// Represents a file operation that can be undone/redone
pub enum FileOperation {
    /// Files copied from sources to dest_dir
    Copy {
        sources: Vec<PathBuf>,
        dest_dir: PathBuf,
        created: Vec<PathBuf>,
    },
    /// Files moved: (original_path, new_path) pairs
    Move {
        moves: Vec<(PathBuf, PathBuf)>,
    },
    /// Files deleted to trash
    Delete {
        paths: Vec<PathBuf>,
    },
    /// File or directory renamed
    Rename {
        old_path: PathBuf,
        new_path: PathBuf,
    },
    /// Directory created
    CreateDir {
        path: PathBuf,
    },
    /// Files compressed into a zip
    Compress {
        sources: Vec<PathBuf>,
        zip_path: PathBuf,
    },
    /// Zip decompressed into a directory
    Decompress {
        zip_path: PathBuf,
        extracted_dir: PathBuf,
    },
}

impl FileOperation {
    pub fn description(&self) -> String {
        match self {
            FileOperation::Copy { created, .. } => {
                format!("Copy {} item(s)", created.len())
            }
            FileOperation::Move { moves } => {
                format!("Move {} item(s)", moves.len())
            }
            FileOperation::Delete { paths } => {
                format!("Delete {} item(s)", paths.len())
            }
            FileOperation::Rename { old_path, new_path } => {
                let old_name = old_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                let new_name = new_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                format!("Rename {} → {}", old_name, new_name)
            }
            FileOperation::CreateDir { path } => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                format!("Create dir: {}", name)
            }
            FileOperation::Compress { zip_path, .. } => {
                let name = zip_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                format!("Compress: {}", name)
            }
            FileOperation::Decompress { extracted_dir, .. } => {
                let name = extracted_dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                format!("Decompress: {}", name)
            }
        }
    }
}

pub struct UndoHistory {
    undo_stack: Vec<FileOperation>,
    redo_stack: Vec<FileOperation>,
}

const MAX_UNDO_SIZE: usize = 50;

impl UndoHistory {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Record a new operation (clears redo stack)
    pub fn push(&mut self, op: FileOperation) {
        self.undo_stack.push(op);
        self.redo_stack.clear();
        if self.undo_stack.len() > MAX_UNDO_SIZE {
            self.undo_stack.remove(0);
        }
    }

    #[cfg(test)]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    #[cfg(test)]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Undo the last operation
    pub fn undo(&mut self) -> Result<String, String> {
        let op = self.undo_stack.pop().ok_or_else(|| "Nothing to undo".to_string())?;
        let desc = op.description();

        let result = execute_undo(&op);
        if let Err(e) = result {
            // Put it back on the stack since undo failed
            self.undo_stack.push(op);
            return Err(e);
        }

        self.redo_stack.push(op);
        Ok(format!("Undo: {}", desc))
    }

    /// Redo the last undone operation
    pub fn redo(&mut self) -> Result<String, String> {
        let op = self.redo_stack.pop().ok_or_else(|| "Nothing to redo".to_string())?;
        let desc = op.description();

        let result = execute_redo(&op);
        if let Err(e) = result {
            self.redo_stack.push(op);
            return Err(e);
        }

        self.undo_stack.push(op);
        Ok(format!("Redo: {}", desc))
    }
}

fn execute_undo(op: &FileOperation) -> Result<(), String> {
    match op {
        FileOperation::Copy { created, .. } => {
            for path in created {
                if path.exists() {
                    file_ops::delete_to_trash(path)
                        .map_err(|e| format!("Undo copy failed: {}", e))?;
                }
            }
        }
        FileOperation::Move { moves } => {
            for (original, current) in moves {
                if current.exists() {
                    if let Some(orig_dir) = original.parent() {
                        file_ops::move_file_or_dir(current, orig_dir)
                            .map_err(|e| format!("Undo move failed: {}", e))?;
                    }
                }
            }
        }
        FileOperation::Delete { paths } => {
            restore_from_trash(paths)?;
        }
        FileOperation::Rename { old_path, new_path } => {
            if new_path.exists() {
                let old_name = old_path
                    .file_name()
                    .ok_or_else(|| "Invalid path".to_string())?
                    .to_string_lossy();
                file_ops::rename_file(new_path, &old_name)
                    .map_err(|e| format!("Undo rename failed: {}", e))?;
            }
        }
        FileOperation::CreateDir { path } => {
            if path.exists() {
                file_ops::delete_to_trash(path)
                    .map_err(|e| format!("Undo mkdir failed: {}", e))?;
            }
        }
        FileOperation::Compress { zip_path, .. } => {
            if zip_path.exists() {
                file_ops::delete_to_trash(zip_path)
                    .map_err(|e| format!("Undo compress failed: {}", e))?;
            }
        }
        FileOperation::Decompress { extracted_dir, .. } => {
            if extracted_dir.exists() {
                file_ops::delete_to_trash(extracted_dir)
                    .map_err(|e| format!("Undo decompress failed: {}", e))?;
            }
        }
    }
    Ok(())
}

/// Find items in trash matching the given original paths and restore them
fn restore_from_trash(paths: &[PathBuf]) -> Result<(), String> {
    let trash_items = trash::os_limited::list()
        .map_err(|e| format!("Failed to list trash: {}", e))?;

    // For each target path, find the most recently deleted matching trash item index
    let mut restore_indices = Vec::new();
    for target_path in paths {
        let mut best_idx: Option<usize> = None;
        let mut best_time: i64 = i64::MIN;
        for (i, item) in trash_items.iter().enumerate() {
            if item.original_path() == *target_path && item.time_deleted > best_time {
                best_idx = Some(i);
                best_time = item.time_deleted;
            }
        }
        if let Some(idx) = best_idx {
            restore_indices.push(idx);
        }
    }

    if restore_indices.is_empty() {
        return Err("Items not found in trash".to_string());
    }

    // Collect owned TrashItems by consuming the list
    // Sort indices descending to remove from end first
    restore_indices.sort_unstable();
    restore_indices.dedup();

    let mut trash_items = trash_items;
    let mut to_restore = Vec::new();
    // Remove in reverse order to preserve indices
    for &idx in restore_indices.iter().rev() {
        to_restore.push(trash_items.swap_remove(idx));
    }

    trash::os_limited::restore_all(to_restore)
        .map_err(|e| format!("Failed to restore from trash: {}", e))?;

    Ok(())
}

fn execute_redo(op: &FileOperation) -> Result<(), String> {
    match op {
        FileOperation::Copy { sources, dest_dir, .. } => {
            for src in sources {
                if src.exists() {
                    file_ops::copy_file_or_dir_overwrite(src, dest_dir)
                        .map_err(|e| format!("Redo copy failed: {}", e))?;
                }
            }
        }
        FileOperation::Move { moves } => {
            for (original, dest) in moves {
                if original.exists() {
                    if let Some(dest_dir) = dest.parent() {
                        file_ops::move_file_or_dir_overwrite(original, dest_dir)
                            .map_err(|e| format!("Redo move failed: {}", e))?;
                    }
                }
            }
        }
        FileOperation::Delete { paths } => {
            for path in paths {
                if path.exists() {
                    file_ops::delete_to_trash(path)
                        .map_err(|e| format!("Redo delete failed: {}", e))?;
                }
            }
        }
        FileOperation::Rename { old_path, new_path } => {
            if old_path.exists() {
                let new_name = new_path
                    .file_name()
                    .ok_or_else(|| "Invalid path".to_string())?
                    .to_string_lossy();
                file_ops::rename_file(old_path, &new_name)
                    .map_err(|e| format!("Redo rename failed: {}", e))?;
            }
        }
        FileOperation::CreateDir { path } => {
            if let Some(parent) = path.parent() {
                let name = path
                    .file_name()
                    .ok_or_else(|| "Invalid path".to_string())?
                    .to_string_lossy();
                file_ops::create_directory(parent, &name)
                    .map_err(|e| format!("Redo mkdir failed: {}", e))?;
            }
        }
        FileOperation::Compress { sources, zip_path } => {
            if let (Some(dest_dir), Some(name)) = (zip_path.parent(), zip_path.file_name()) {
                file_ops::compress_to_zip(sources, dest_dir, &name.to_string_lossy())
                    .map_err(|e| format!("Redo compress failed: {}", e))?;
            }
        }
        FileOperation::Decompress { zip_path, extracted_dir } => {
            if let Some(dest_dir) = extracted_dir.parent() {
                file_ops::decompress_zip(zip_path, dest_dir)
                    .map_err(|e| format!("Redo decompress failed: {}", e))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn undo_history_push_and_can_undo() {
        let mut history = UndoHistory::new();
        assert!(!history.can_undo());
        assert!(!history.can_redo());

        history.push(FileOperation::CreateDir {
            path: PathBuf::from("/tmp/test"),
        });
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn push_clears_redo_stack() {
        let mut history = UndoHistory::new();

        // Simulate: push op, undo it (manually move to redo), then push new op
        history.redo_stack.push(FileOperation::CreateDir {
            path: PathBuf::from("/tmp/old"),
        });
        assert!(history.can_redo());

        history.push(FileOperation::CreateDir {
            path: PathBuf::from("/tmp/new"),
        });
        assert!(!history.can_redo());
    }

    #[test]
    fn max_undo_size() {
        let mut history = UndoHistory::new();
        for i in 0..(MAX_UNDO_SIZE + 10) {
            history.push(FileOperation::CreateDir {
                path: PathBuf::from(format!("/tmp/dir{}", i)),
            });
        }
        assert_eq!(history.undo_stack.len(), MAX_UNDO_SIZE);
    }

    #[test]
    fn undo_empty_returns_error() {
        let mut history = UndoHistory::new();
        assert!(history.undo().is_err());
    }

    #[test]
    fn redo_empty_returns_error() {
        let mut history = UndoHistory::new();
        assert!(history.redo().is_err());
    }

    #[test]
    fn undo_redo_create_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir_path = tmp.path().join("testdir");
        fs::create_dir(&dir_path).unwrap();
        assert!(dir_path.exists());

        let mut history = UndoHistory::new();
        history.push(FileOperation::CreateDir {
            path: dir_path.clone(),
        });

        // Undo: should delete dir (to trash)
        let result = history.undo();
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Undo"));
        assert!(!dir_path.exists());
        assert!(history.can_redo());

        // Redo: should recreate dir
        let result = history.redo();
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Redo"));
        assert!(dir_path.exists());
    }

    #[test]
    fn undo_redo_rename() {
        let tmp = tempfile::tempdir().unwrap();
        let old_path = tmp.path().join("old.txt");
        let new_path = tmp.path().join("new.txt");
        fs::write(&old_path, "data").unwrap();
        fs::rename(&old_path, &new_path).unwrap();

        let mut history = UndoHistory::new();
        history.push(FileOperation::Rename {
            old_path: old_path.clone(),
            new_path: new_path.clone(),
        });

        // Undo: rename back
        let result = history.undo();
        assert!(result.is_ok());
        assert!(old_path.exists());
        assert!(!new_path.exists());

        // Redo: rename forward
        let result = history.redo();
        assert!(result.is_ok());
        assert!(!old_path.exists());
        assert!(new_path.exists());
    }

    #[test]
    fn undo_redo_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("source.txt");
        fs::write(&src, "hello").unwrap();

        let dest_dir = tmp.path().join("dest");
        fs::create_dir(&dest_dir).unwrap();
        let created = dest_dir.join("source.txt");

        // Simulate copy
        fs::copy(&src, &created).unwrap();
        assert!(created.exists());

        let mut history = UndoHistory::new();
        history.push(FileOperation::Copy {
            sources: vec![src.clone()],
            dest_dir: dest_dir.clone(),
            created: vec![created.clone()],
        });

        // Undo: delete copy
        let result = history.undo();
        assert!(result.is_ok());
        assert!(!created.exists());
        assert!(src.exists()); // source still there

        // Redo: re-copy
        let result = history.redo();
        assert!(result.is_ok());
        assert!(created.exists());
    }

    #[test]
    fn undo_redo_move() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("srcdir");
        fs::create_dir(&src_dir).unwrap();
        let original = src_dir.join("file.txt");
        fs::write(&original, "data").unwrap();

        let dest_dir = tmp.path().join("destdir");
        fs::create_dir(&dest_dir).unwrap();
        let moved = dest_dir.join("file.txt");

        // Simulate move
        fs::rename(&original, &moved).unwrap();
        assert!(!original.exists());
        assert!(moved.exists());

        let mut history = UndoHistory::new();
        history.push(FileOperation::Move {
            moves: vec![(original.clone(), moved.clone())],
        });

        // Undo: move back
        let result = history.undo();
        assert!(result.is_ok());
        assert!(original.exists());
        assert!(!moved.exists());

        // Redo: move forward
        let result = history.redo();
        assert!(result.is_ok());
        assert!(!original.exists());
        assert!(moved.exists());
    }

    #[test]
    fn description_formats() {
        let op = FileOperation::Copy {
            sources: vec![],
            dest_dir: PathBuf::from("/tmp"),
            created: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        };
        assert_eq!(op.description(), "Copy 2 item(s)");

        let op = FileOperation::Move {
            moves: vec![(PathBuf::from("/a"), PathBuf::from("/b"))],
        };
        assert_eq!(op.description(), "Move 1 item(s)");

        let op = FileOperation::Rename {
            old_path: PathBuf::from("/tmp/old.txt"),
            new_path: PathBuf::from("/tmp/new.txt"),
        };
        assert!(op.description().contains("old.txt"));
        assert!(op.description().contains("new.txt"));

        let op = FileOperation::CreateDir {
            path: PathBuf::from("/tmp/mydir"),
        };
        assert!(op.description().contains("mydir"));

        let op = FileOperation::Delete {
            paths: vec![PathBuf::from("/a"), PathBuf::from("/b"), PathBuf::from("/c")],
        };
        assert_eq!(op.description(), "Delete 3 item(s)");
    }
}
