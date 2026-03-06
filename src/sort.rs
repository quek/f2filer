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
