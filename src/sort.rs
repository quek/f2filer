use crate::file_item::FileItem;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Extension,
    Size,
    Date,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    pub fn toggle(&self) -> Self {
        match self {
            SortOrder::Ascending => SortOrder::Descending,
            SortOrder::Descending => SortOrder::Ascending,
        }
    }
}

pub fn sort_entries(entries: &mut [FileItem], key: SortKey, order: SortOrder) {
    // Keep ".." always at the top
    let start = if entries.first().is_some_and(|e| e.name == "..") {
        1
    } else {
        0
    };

    if start >= entries.len() {
        return;
    }

    let slice = &mut entries[start..];

    slice.sort_by(|a, b| {
        // Directories always come before files
        if a.is_dir != b.is_dir {
            return if a.is_dir {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }

        let cmp = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Extension => a
                .extension
                .to_lowercase()
                .cmp(&b.extension.to_lowercase())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Date => a.modified.cmp(&b.modified),
        };

        match order {
            SortOrder::Ascending => cmp,
            SortOrder::Descending => cmp.reverse(),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn make_file(name: &str, ext: &str, size: u64, secs_ago: u64) -> FileItem {
        FileItem {
            name: name.to_string(),
            path: PathBuf::from(name),
            size,
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - secs_ago)),
            is_dir: false,
            is_hidden: false,
            extension: ext.to_string(),
        }
    }

    fn make_dir(name: &str) -> FileItem {
        FileItem {
            name: name.to_string(),
            path: PathBuf::from(name),
            size: 0,
            modified: Some(SystemTime::UNIX_EPOCH),
            is_dir: true,
            is_hidden: false,
            extension: String::new(),
        }
    }

    #[test]
    fn sort_order_toggle() {
        assert_eq!(SortOrder::Ascending.toggle(), SortOrder::Descending);
        assert_eq!(SortOrder::Descending.toggle(), SortOrder::Ascending);
    }

    #[test]
    fn sort_by_name_ascending() {
        let mut entries = vec![
            make_file("cherry.txt", "txt", 100, 0),
            make_file("apple.txt", "txt", 200, 0),
            make_file("banana.txt", "txt", 150, 0),
        ];
        sort_entries(&mut entries, SortKey::Name, SortOrder::Ascending);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["apple.txt", "banana.txt", "cherry.txt"]);
    }

    #[test]
    fn sort_by_name_descending() {
        let mut entries = vec![
            make_file("apple.txt", "txt", 200, 0),
            make_file("cherry.txt", "txt", 100, 0),
        ];
        sort_entries(&mut entries, SortKey::Name, SortOrder::Descending);
        assert_eq!(entries[0].name, "cherry.txt");
        assert_eq!(entries[1].name, "apple.txt");
    }

    #[test]
    fn sort_by_size() {
        let mut entries = vec![
            make_file("big.txt", "txt", 1000, 0),
            make_file("small.txt", "txt", 10, 0),
            make_file("medium.txt", "txt", 500, 0),
        ];
        sort_entries(&mut entries, SortKey::Size, SortOrder::Ascending);
        assert_eq!(entries[0].name, "small.txt");
        assert_eq!(entries[1].name, "medium.txt");
        assert_eq!(entries[2].name, "big.txt");
    }

    #[test]
    fn sort_by_extension() {
        let mut entries = vec![
            make_file("file.zip", "zip", 100, 0),
            make_file("file.txt", "txt", 100, 0),
            make_file("file.rs", "rs", 100, 0),
        ];
        sort_entries(&mut entries, SortKey::Extension, SortOrder::Ascending);
        assert_eq!(entries[0].extension, "rs");
        assert_eq!(entries[1].extension, "txt");
        assert_eq!(entries[2].extension, "zip");
    }

    #[test]
    fn sort_by_date() {
        let mut entries = vec![
            make_file("old.txt", "txt", 100, 1000),
            make_file("new.txt", "txt", 100, 0),
            make_file("mid.txt", "txt", 100, 500),
        ];
        sort_entries(&mut entries, SortKey::Date, SortOrder::Ascending);
        assert_eq!(entries[0].name, "old.txt");
        assert_eq!(entries[1].name, "mid.txt");
        assert_eq!(entries[2].name, "new.txt");
    }

    #[test]
    fn dirs_before_files() {
        let mut entries = vec![
            make_file("z_file.txt", "txt", 100, 0),
            make_dir("a_dir"),
            make_file("a_file.txt", "txt", 200, 0),
        ];
        sort_entries(&mut entries, SortKey::Name, SortOrder::Ascending);
        assert!(entries[0].is_dir);
        assert!(!entries[1].is_dir);
        assert!(!entries[2].is_dir);
    }

    #[test]
    fn dotdot_stays_at_top() {
        let mut entries = vec![
            FileItem::parent_entry(PathBuf::from("/")),
            make_file("z.txt", "txt", 100, 0),
            make_dir("a_dir"),
        ];
        sort_entries(&mut entries, SortKey::Name, SortOrder::Ascending);
        assert_eq!(entries[0].name, "..");
        assert_eq!(entries[1].name, "a_dir");
        assert_eq!(entries[2].name, "z.txt");
    }

    #[test]
    fn sort_case_insensitive() {
        let mut entries = vec![
            make_file("Banana.txt", "txt", 100, 0),
            make_file("apple.txt", "txt", 100, 0),
        ];
        sort_entries(&mut entries, SortKey::Name, SortOrder::Ascending);
        assert_eq!(entries[0].name, "apple.txt");
        assert_eq!(entries[1].name, "Banana.txt");
    }

    #[test]
    fn sort_empty_slice() {
        let mut entries: Vec<FileItem> = vec![];
        sort_entries(&mut entries, SortKey::Name, SortOrder::Ascending);
        assert!(entries.is_empty());
    }
}
