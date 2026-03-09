use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use eframe::egui;

use crate::file_item::{read_directory, read_directory_recursive_streaming, FileItem};
use crate::sort::{sort_entries, SortKey, SortOrder};

/// Display width of a character (2 for CJK/fullwidth, 1 otherwise).
fn char_display_width(c: char) -> usize {
    let cp = c as u32;
    // CJK Unified Ideographs, Hiragana, Katakana, Hangul, fullwidth forms, etc.
    if matches!(cp,
        0x1100..=0x115F   // Hangul Jamo
        | 0x2E80..=0x303E // CJK Radicals, Kangxi, Ideographic Description, CJK Symbols
        | 0x3041..=0x33BF // Hiragana, Katakana, Bopomofo, Hangul Compat, Kanbun, CJK Letters
        | 0x3400..=0x4DBF // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xA000..=0xA4CF // Yi
        | 0xAC00..=0xD7AF // Hangul Syllables
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
        | 0xFE30..=0xFE6F // CJK Compatibility Forms
        | 0xFF01..=0xFF60 // Fullwidth Forms
        | 0xFFE0..=0xFFE6 // Fullwidth Signs
        | 0x20000..=0x2FA1F // CJK Extensions B-F, Compat Supplement
    ) {
        2
    } else {
        1
    }
}

/// Display width of a string (accounts for fullwidth characters).
fn str_display_width(s: &str) -> usize {
    s.chars().map(char_display_width).sum()
}

/// Truncate a string in the middle with "…" if it exceeds max display width.
/// Shows the beginning and end of the string to preserve useful info.
fn truncate_middle(s: &str, max_width: usize) -> String {
    if str_display_width(s) <= max_width {
        return s.to_string();
    }
    if max_width <= 3 {
        let mut result = String::new();
        let mut w = 0;
        for c in s.chars() {
            let cw = char_display_width(c);
            if w + cw > max_width {
                break;
            }
            result.push(c);
            w += cw;
        }
        return result;
    }
    let keep = max_width - 1; // 1 col for "…"
    let front_budget = (keep + 1) / 2;
    let back_budget = keep - front_budget;

    // Build front part
    let mut front_str = String::new();
    let mut front_w = 0;
    for c in s.chars() {
        let cw = char_display_width(c);
        if front_w + cw > front_budget {
            break;
        }
        front_str.push(c);
        front_w += cw;
    }

    // Build back part (collect from end)
    let chars: Vec<char> = s.chars().collect();
    let mut back_chars = Vec::new();
    let mut back_w = 0;
    for &c in chars.iter().rev() {
        let cw = char_display_width(c);
        if back_w + cw > back_budget {
            break;
        }
        back_chars.push(c);
        back_w += cw;
    }
    back_chars.reverse();
    let back_str: String = back_chars.into_iter().collect();

    format!("{}…{}", front_str, back_str)
}

pub struct FilePanel {
    pub current_dir: PathBuf,
    pub entries: Vec<FileItem>,
    pub cursor: usize,
    pub selected: HashSet<usize>,
    pub sort_key: SortKey,
    pub sort_order: SortOrder,
    pub filter: String,
    pub show_hidden: bool,
    filtered_indices: Vec<usize>,
    pub focus_filter: bool,
    pub filter_has_focus: bool,
    pub drag_request: Option<Vec<PathBuf>>,
    pub drop_highlight: bool,
    pub clicked: bool,
    scroll_offset: f32,
    viewport_h: f32,
    pub is_loading: bool,
    loading_result: Arc<Mutex<Option<(u64, Vec<FileItem>, Option<PathBuf>)>>>,
    loading_generation: u64,
    pub(crate) loading_old_name: Option<String>,
    last_dir_modified: Option<SystemTime>,
    last_dir_check: Instant,
    pub cursor_history: HashMap<PathBuf, String>,
    /// Directories whose cursor_history was modified in this session.
    pub(crate) cursor_dirty: HashSet<PathBuf>,
    pub sort_history: HashMap<PathBuf, (SortKey, SortOrder)>,
    pub(crate) sort_dirty: HashSet<PathBuf>,
    pub recursive_filter: bool,
    is_searching: bool,
    search_sink: Arc<Mutex<Vec<FileItem>>>,
    search_done: Arc<AtomicBool>,
}

impl FilePanel {
    pub fn new(dir: PathBuf) -> Self {
        let mut panel = FilePanel {
            current_dir: dir,
            entries: Vec::new(),
            cursor: 0,
            selected: HashSet::new(),
            sort_key: SortKey::Name,
            sort_order: SortOrder::Ascending,
            filter: String::new(),
            show_hidden: true,
            filtered_indices: Vec::new(),
            focus_filter: false,
            filter_has_focus: false,
            drag_request: None,
            drop_highlight: false,
            clicked: false,
            scroll_offset: 0.0,
            viewport_h: 0.0,
            is_loading: false,
            loading_result: Arc::new(Mutex::new(None)),
            loading_generation: 0,
            loading_old_name: None,
            last_dir_modified: None,
            last_dir_check: Instant::now(),
            cursor_history: HashMap::new(),
            cursor_dirty: HashSet::new(),
            sort_history: HashMap::new(),
            sort_dirty: HashSet::new(),
            recursive_filter: false,
            is_searching: false,
            search_sink: Arc::new(Mutex::new(Vec::new())),
            search_done: Arc::new(AtomicBool::new(false)),
        };
        panel.refresh();
        panel
    }

    /// Remove entries whose paths are in the given set (for recursive mode after delete/move).
    pub fn remove_paths(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let set: std::collections::HashSet<&PathBuf> = paths.iter().collect();
        self.entries.retain(|item| !set.contains(&item.path));
        self.rebuild_filter();
        self.selected.clear();
        if self.cursor >= self.visible_count() {
            self.cursor = self.visible_count().saturating_sub(1);
        }
    }

    pub fn refresh(&mut self) {
        if self.recursive_filter {
            // In recursive mode, preserve search results; just clear selection.
            self.selected.clear();
            if self.cursor >= self.visible_count() {
                self.cursor = self.visible_count().saturating_sub(1);
            }
            return;
        }
        self.entries = read_directory(&self.current_dir);
        sort_entries(&mut self.entries, self.sort_key, self.sort_order);
        self.rebuild_filter();
        self.selected.clear();
        if self.cursor >= self.visible_count() {
            self.cursor = self.visible_count().saturating_sub(1);
        }
        self.update_dir_mtime();
    }

    fn rebuild_filter(&mut self) {
        let filter_lower = self.filter.to_lowercase();
        self.filtered_indices = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if !self.show_hidden && item.is_hidden {
                    return false;
                }
                if filter_lower.is_empty() {
                    return true;
                }
                item.name.to_lowercase().contains(&filter_lower)
            })
            .map(|(i, _)| i)
            .collect();
    }

    pub fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    pub fn visible_entry(&self, visible_idx: usize) -> Option<&FileItem> {
        self.filtered_indices
            .get(visible_idx)
            .and_then(|&real_idx| self.entries.get(real_idx))
    }

    pub fn current_entry(&self) -> Option<&FileItem> {
        self.visible_entry(self.cursor)
    }

    fn real_index(&self, visible_idx: usize) -> Option<usize> {
        self.filtered_indices.get(visible_idx).copied()
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.rebuild_filter();
        if self.cursor >= self.visible_count() {
            self.cursor = self.visible_count().saturating_sub(1);
        }
        // Auto-move cursor to first matching file
        if !self.filter.is_empty() && !self.filtered_indices.is_empty() {
            self.cursor = 0;
        }
    }

    pub fn set_sort(&mut self, key: SortKey) {
        if self.sort_key == key {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_key = key;
            self.sort_order = SortOrder::Ascending;
        }
        sort_entries(&mut self.entries, self.sort_key, self.sort_order);
        self.rebuild_filter();
        // Save sort state for this directory
        let dir = self.current_dir.clone();
        self.sort_dirty.insert(dir.clone());
        self.sort_history.insert(dir, (self.sort_key, self.sort_order));
    }

    pub fn start_recursive_search(&mut self, ctx: &egui::Context) {
        self.is_searching = true;
        self.entries.clear();
        self.filtered_indices.clear();
        self.selected.clear();
        self.cursor = 0;

        // Reset shared state
        self.search_sink = Arc::new(Mutex::new(Vec::new()));
        self.search_done = Arc::new(AtomicBool::new(false));

        let root = self.current_dir.clone();
        let show_hidden = self.show_hidden;
        let sink = Arc::clone(&self.search_sink);
        let done = Arc::clone(&self.search_done);
        let repaint_ctx = ctx.clone();
        std::thread::spawn(move || {
            read_directory_recursive_streaming(&root, show_hidden, &sink, &done, &repaint_ctx);
        });
    }

    /// Drain buffered search results from background thread into entries.
    pub fn drain_search_results(&mut self) {
        if !self.recursive_filter {
            return;
        }
        let mut new_items = {
            let mut lock = self.search_sink.lock().unwrap();
            if lock.is_empty() {
                // Check if search finished
                if self.is_searching && self.search_done.load(Ordering::Acquire) {
                    self.is_searching = false;
                }
                return;
            }
            std::mem::take(&mut *lock)
        };
        self.entries.append(&mut new_items);
        self.rebuild_filter();
        if self.search_done.load(Ordering::Acquire) {
            self.is_searching = false;
        }
    }

    pub fn exit_recursive_search(&mut self) {
        self.recursive_filter = false;
        self.is_searching = false;
        self.search_sink = Arc::new(Mutex::new(Vec::new()));
        self.search_done = Arc::new(AtomicBool::new(false));
        self.filter.clear();
        self.refresh();
    }

    /// Find the visible index of an entry by name.
    fn find_visible_by_name(&self, name: &str) -> Option<usize> {
        self.filtered_indices
            .iter()
            .enumerate()
            .find_map(|(i, &idx)| {
                if self.entries[idx].name == name {
                    Some(i)
                } else {
                    None
                }
            })
    }

    /// Restore sort state from history for the current directory.
    pub fn restore_sort_from_history(&mut self) {
        if let Some(&(key, order)) = self.sort_history.get(&self.current_dir) {
            self.sort_key = key;
            self.sort_order = order;
            sort_entries(&mut self.entries, self.sort_key, self.sort_order);
            self.rebuild_filter();
        }
    }

    /// Restore cursor position from history for the current directory.
    pub fn restore_cursor_from_history(&mut self) {
        if let Some(idx) = self
            .cursor_history
            .get(&self.current_dir)
            .and_then(|name| self.find_visible_by_name(name))
        {
            self.cursor = idx;
        }
    }

    /// Save current cursor filename to history.
    pub(crate) fn save_cursor_position(&mut self) {
        if let Some(entry) = self.current_entry() {
            let dir = self.current_dir.clone();
            let name = entry.name.clone();
            self.cursor_dirty.insert(dir.clone());
            self.cursor_history.insert(dir, name);
        }
    }

    pub fn navigate_to(&mut self, dir: PathBuf, ctx: &egui::Context) {
        self.save_cursor_position();
        // Only set loading_old_name when going up to parent directory.
        // This positions the cursor on the directory we came from.
        let is_going_up = self
            .current_dir
            .parent()
            .map_or(false, |p| p == dir.as_path());
        self.loading_old_name = if is_going_up {
            self.current_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        } else {
            None
        };
        self.current_dir = dir.clone();
        self.cursor = 0;
        self.filter.clear();
        self.entries.clear();
        self.filtered_indices.clear();
        self.selected.clear();
        self.is_loading = true;
        self.loading_generation += 1;

        let gen = self.loading_generation;
        let result = Arc::clone(&self.loading_result);
        let repaint_ctx = ctx.clone();
        std::thread::spawn(move || {
            let entries = read_directory(&dir);
            *result.lock().unwrap() = Some((gen, entries, None));
            repaint_ctx.request_repaint();
        });
    }

    /// Navigate with a resolver function that runs in a background thread.
    /// The resolver determines the actual directory path (e.g., drive path resolution
    /// with exists() checks), avoiding UI thread blocking.
    pub fn navigate_to_with_resolver(
        &mut self,
        placeholder_dir: PathBuf,
        resolver: impl FnOnce() -> PathBuf + Send + 'static,
        ctx: &egui::Context,
    ) {
        self.save_cursor_position();
        self.current_dir = placeholder_dir;
        self.cursor = 0;
        self.filter.clear();
        self.entries.clear();
        self.filtered_indices.clear();
        self.selected.clear();
        self.is_loading = true;
        self.loading_generation += 1;
        self.loading_old_name = None;

        let gen = self.loading_generation;
        let result = Arc::clone(&self.loading_result);
        let repaint_ctx = ctx.clone();
        std::thread::spawn(move || {
            let dir = resolver();
            let entries = read_directory(&dir);
            *result.lock().unwrap() = Some((gen, entries, Some(dir)));
            repaint_ctx.request_repaint();
        });
    }

    /// Poll for async directory loading completion. Returns true if loading just completed.
    pub fn check_loading_complete(&mut self) -> bool {
        if !self.is_loading {
            return false;
        }
        let data = self.loading_result.lock().unwrap().take();
        if let Some((gen, entries, resolved_dir)) = data {
            if gen != self.loading_generation {
                return false; // stale result from a superseded navigation
            }
            // Update current_dir if the background thread resolved the actual path
            if let Some(dir) = resolved_dir {
                self.current_dir = dir;
            }
            self.entries = entries;
            // Restore sort state for this directory, or default to name ascending
            if let Some(&(key, order)) = self.sort_history.get(&self.current_dir) {
                self.sort_key = key;
                self.sort_order = order;
            } else {
                self.sort_key = SortKey::Name;
                self.sort_order = SortOrder::Ascending;
            }
            sort_entries(&mut self.entries, self.sort_key, self.sort_order);
            self.rebuild_filter();
            self.selected.clear();
            if self.cursor >= self.visible_count() {
                self.cursor = self.visible_count().saturating_sub(1);
            }
            // Restore cursor position:
            // 1. loading_old_name: going up → position on the directory we came from
            // 2. cursor_history: revisiting → restore last cursor position
            let restored = self
                .loading_old_name
                .take()
                .and_then(|name| self.find_visible_by_name(&name))
                .or_else(|| {
                    self.cursor_history
                        .get(&self.current_dir)
                        .and_then(|name| self.find_visible_by_name(name))
                });
            if let Some(idx) = restored {
                self.cursor = idx;
            }
            self.is_loading = false;
            self.update_dir_mtime();
            return true;
        }
        false
    }

    /// Record the current directory's modification time for auto-refresh.
    fn update_dir_mtime(&mut self) {
        self.last_dir_modified = std::fs::metadata(&self.current_dir)
            .and_then(|m| m.modified())
            .ok();
        self.last_dir_check = Instant::now();
    }

    /// Check if the directory has changed and auto-refresh if needed.
    /// Preserves cursor position and selection by filename.
    pub fn check_auto_refresh(&mut self) {
        if self.is_loading || self.recursive_filter {
            return;
        }
        if self.last_dir_check.elapsed() < std::time::Duration::from_secs(2) {
            return;
        }
        self.last_dir_check = Instant::now();

        let current_mtime = std::fs::metadata(&self.current_dir)
            .and_then(|m| m.modified())
            .ok();
        if current_mtime == self.last_dir_modified {
            return;
        }
        self.last_dir_modified = current_mtime;

        // Save cursor and selection by filename
        let cursor_name = self.current_entry().map(|e| e.name.clone());
        let selected_names: HashSet<String> = self
            .selected
            .iter()
            .filter_map(|&idx| self.entries.get(idx).map(|e| e.name.clone()))
            .collect();

        self.entries = read_directory(&self.current_dir);
        sort_entries(&mut self.entries, self.sort_key, self.sort_order);
        self.rebuild_filter();

        // Restore selection by filename
        self.selected.clear();
        if !selected_names.is_empty() {
            for &idx in &self.filtered_indices {
                if selected_names.contains(&self.entries[idx].name) {
                    self.selected.insert(idx);
                }
            }
        }

        // Restore cursor position by filename
        if let Some(name) = cursor_name {
            for (i, &idx) in self.filtered_indices.iter().enumerate() {
                if self.entries[idx].name == name {
                    self.cursor = i;
                    return;
                }
            }
        }
        if self.cursor >= self.visible_count() {
            self.cursor = self.visible_count().saturating_sub(1);
        }
    }

    pub fn move_cursor(&mut self, delta: i32) {
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        let new = (self.cursor as i32 + delta).rem_euclid(count as i32) as usize;
        self.cursor = new;
    }

    pub fn move_cursor_to_start(&mut self) {
        self.cursor = 0;
    }

    pub fn move_cursor_to_end(&mut self) {
        self.cursor = self.visible_count().saturating_sub(1);
    }

    pub fn page_up(&mut self, page_size: usize) {
        self.move_cursor(-(page_size as i32));
    }

    pub fn page_down(&mut self, page_size: usize) {
        self.move_cursor(page_size as i32);
    }

    pub fn toggle_select(&mut self) {
        if let Some(real_idx) = self.real_index(self.cursor) {
            if self.selected.contains(&real_idx) {
                self.selected.remove(&real_idx);
            } else {
                self.selected.insert(real_idx);
            }
        }
    }

    pub fn select_all(&mut self) {
        self.selected.clear();
        for &idx in &self.filtered_indices {
            self.selected.insert(idx);
        }
    }

    pub fn deselect_all(&mut self) {
        self.selected.clear();
    }

    pub fn is_selected(&self, visible_idx: usize) -> bool {
        self.real_index(visible_idx)
            .is_some_and(|r| self.selected.contains(&r))
    }

    /// Get selected files only (not cursor)
    pub fn get_operation_targets(&self) -> Vec<FileItem> {
        self.selected
            .iter()
            .filter_map(|&idx| self.entries.get(idx).cloned())
            .collect()
    }

    pub fn selected_total_size(&self) -> u64 {
        self.selected
            .iter()
            .filter_map(|&idx| self.entries.get(idx))
            .map(|e| e.size)
            .sum()
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        is_active: bool,
        id_salt: &str,
    ) {
        let panel_id = egui::Id::new(id_salt);

        // Current path (truncated to available width)
        // In recursive mode, show selected file's full path instead
        ui.horizontal(|ui| {
            let path_str = if self.recursive_filter {
                self.current_entry()
                    .map(|e| e.path.to_string_lossy().to_string())
                    .unwrap_or_else(|| self.current_dir.to_string_lossy().to_string())
            } else {
                self.current_dir.to_string_lossy().to_string()
            };
            let font_id = egui::TextStyle::Body.resolve(ui.style());
            let char_width = ui.fonts(|f| f.glyph_width(&font_id, 'W'));
            let available = ui.available_width();
            let max_chars = ((available / char_width) as usize).max(4);
            let display = truncate_middle(&path_str, max_chars);
            ui.strong(display);
        });

        // Filter input
        ui.horizontal(|ui| {
            if self.recursive_filter {
                let total = self.entries.len();
                let shown = self.filtered_indices.len();
                let searching = if self.is_searching { "..." } else { "" };
                if self.filter.is_empty() {
                    ui.label(format!("[R:{total}{searching}] Filter:"));
                } else {
                    ui.label(format!("[R:{shown}/{total}{searching}] Filter:"));
                }
            } else {
                ui.label("Filter:");
            }
            let filter_id = panel_id.with("filter");
            let mut filter = self.filter.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .id(filter_id)
                    .desired_width(ui.available_width()),
            );
            if self.focus_filter {
                response.request_focus();
                self.focus_filter = false;
            }
            self.filter_has_focus = response.has_focus();
            // singleline TextEdit auto-surrenders focus on Enter,
            // so use lost_focus() to detect Enter confirmation
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.filter_has_focus = false;
                if !self.filter.is_empty() && !self.filtered_indices.is_empty() {
                    self.cursor = 0;
                }
            }
            if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                response.surrender_focus();
                self.filter_has_focus = false;
            }
            if response.changed() {
                self.set_filter(filter);
            }
        });

        ui.separator();

        // Column headers — widths shared between headers and rows for alignment
        let mut name_w = 0.0f32;
        let mut ext_w = 0.0f32;
        let mut size_w = 0.0f32;

        let mut sort_clicked: Option<SortKey> = None;
        let cur_sort_key = self.sort_key;
        let cur_sort_order = self.sort_order;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;

            let sort_indicator = |key: SortKey| -> &'static str {
                if cur_sort_key == key {
                    match cur_sort_order {
                        SortOrder::Ascending => " ^",
                        SortOrder::Descending => " v",
                    }
                } else {
                    ""
                }
            };

            // Fixed widths for Ext, Size, Date; Name gets the rest
            ext_w = 90.0;
            size_w = 90.0;
            let date_w = 145.0;
            let w = ui.available_width();
            name_w = (w - ext_w - size_w - date_w).max(100.0);

            if ui
                .add_sized(
                    [name_w, 22.0],
                    egui::Button::new(
                        egui::RichText::new(format!("Name{}", sort_indicator(SortKey::Name)))
                            .strong(),
                    ),
                )
                .clicked()
            {
                sort_clicked = Some(SortKey::Name);
            }

            if ui
                .add_sized(
                    [ext_w, 22.0],
                    egui::Button::new(
                        egui::RichText::new(format!("Ext{}", sort_indicator(SortKey::Extension)))
                            .strong(),
                    ),
                )
                .clicked()
            {
                sort_clicked = Some(SortKey::Extension);
            }

            if ui
                .add_sized(
                    [size_w, 22.0],
                    egui::Button::new(
                        egui::RichText::new(format!("Size{}", sort_indicator(SortKey::Size)))
                            .strong(),
                    ),
                )
                .clicked()
            {
                sort_clicked = Some(SortKey::Size);
            }

            if ui
                .add_sized(
                    [date_w, 22.0],
                    egui::Button::new(
                        egui::RichText::new(format!("Date{}", sort_indicator(SortKey::Date)))
                            .strong(),
                    ),
                )
                .clicked()
            {
                sort_clicked = Some(SortKey::Date);
            }
        });

        if let Some(key) = sort_clicked {
            self.set_sort(key);
        }

        ui.separator();

        // Loading indicator
        if self.is_loading {
            let remaining = ui.available_height();
            ui.add_space((remaining / 2.0 - 12.0).max(0.0));
            ui.vertical_centered(|ui| {
                ui.spinner();
            });
            return;
        }

        // Recursive search: drain streamed results
        self.drain_search_results();

        // File list
        let row_height = 17.0;
        let visible_count = self.visible_count();

        // Keep cursor centered in viewport when possible
        if is_active && visible_count > 0 && self.viewport_h > 0.0 {
            let cursor_center = self.cursor as f32 * row_height + row_height * 0.5;
            let total_h = visible_count as f32 * row_height;
            let max_offset = (total_h - self.viewport_h).max(0.0);
            let ideal_offset = cursor_center - self.viewport_h * 0.5;
            self.scroll_offset = ideal_offset.clamp(0.0, max_offset);
        }

        // Set item_spacing.y = 0 BEFORE show_rows so it uses correct row height
        ui.spacing_mut().item_spacing.y = 0.0;

        let scroll_output = egui::ScrollArea::vertical()
            .id_salt(panel_id.with("scroll"))
            .auto_shrink([false; 2])
            .vertical_scroll_offset(self.scroll_offset)
            .show_rows(ui, row_height, visible_count, |ui, row_range| {
                for vis_idx in row_range {
                    if vis_idx >= visible_count {
                        break;
                    }

                    let is_cursor = vis_idx == self.cursor && is_active;
                    let is_sel = self.is_selected(vis_idx);

                    let entry = match self.visible_entry(vis_idx) {
                        Some(e) => e.clone(),
                        None => continue,
                    };

                    let bg_color = if is_cursor {
                        egui::Color32::from_rgb(50, 80, 140)
                    } else if is_sel {
                        egui::Color32::from_rgb(80, 60, 30)
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    let text_color = if is_sel {
                        egui::Color32::from_rgb(255, 200, 50)
                    } else if entry.is_dir {
                        egui::Color32::from_rgb(100, 180, 255)
                    } else {
                        egui::Color32::from_rgb(220, 220, 220)
                    };

                    let rect = ui.available_rect_before_wrap();
                    let row_rect = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(ui.available_width(), row_height),
                    );

                    ui.painter().rect_filled(row_rect, 0.0, bg_color);

                    // Build display text (strip extension for files since Ext column shows it)
                    let name_display = if entry.is_dir {
                        format!("[{}]", entry.name)
                    } else if !entry.extension.is_empty() {
                        entry.name[..entry.name.len() - entry.extension.len() - 1].to_string()
                    } else {
                        entry.name.clone()
                    };
                    let mark = if is_sel { "*" } else { " " };
                    let full_name = format!("{}{}", mark, name_display);

                    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
                    let char_width = ui.fonts(|f| f.glyph_width(&font_id, 'W'));
                    let col_pad = char_width; // 1 character width padding between columns
                    let max_chars = (((name_w - col_pad) / char_width) as usize).max(4);
                    let name_text = truncate_middle(&full_name, max_chars);

                    let x0 = row_rect.min.x;
                    let y_center = row_rect.center().y;

                    // Column positions with padding
                    let ext_x = x0 + name_w + col_pad;
                    let size_right_x = x0 + name_w + ext_w + size_w - col_pad;
                    let date_x = x0 + name_w + ext_w + size_w + col_pad;

                    // Name column (left-aligned)
                    ui.painter().text(
                        egui::pos2(x0, y_center),
                        egui::Align2::LEFT_CENTER,
                        &name_text,
                        font_id.clone(),
                        text_color,
                    );

                    // Ext column (left-aligned, shows <DIR> for directories)
                    ui.painter().text(
                        egui::pos2(ext_x, y_center),
                        egui::Align2::LEFT_CENTER,
                        entry.formatted_ext(),
                        font_id.clone(),
                        text_color,
                    );

                    // Size column (right-aligned)
                    ui.painter().text(
                        egui::pos2(size_right_x, y_center),
                        egui::Align2::RIGHT_CENTER,
                        entry.formatted_size(),
                        font_id.clone(),
                        text_color,
                    );

                    // Date column (left-aligned)
                    ui.painter().text(
                        egui::pos2(date_x, y_center),
                        egui::Align2::LEFT_CENTER,
                        entry.formatted_date(),
                        font_id.clone(),
                        text_color,
                    );

                    // Advance layout (click to move cursor, drag for OLE drag-and-drop)
                    let response = ui.allocate_rect(row_rect, egui::Sense::click_and_drag());

                    // Click → move cursor to this row
                    if response.clicked() {
                        self.cursor = vis_idx;
                        self.clicked = true;
                    }

                    // Detect drag start → collect paths for OLE drag
                    if response.drag_started() {
                        let mut paths = Vec::new();
                        if !self.selected.is_empty() {
                            // Drag all selected files
                            for &idx in &self.selected {
                                if let Some(item) = self.entries.get(idx) {
                                    paths.push(item.path.clone());
                                }
                            }
                        } else if let Some(item) = self.visible_entry(vis_idx) {
                            // Drag file under cursor
                            paths.push(item.path.clone());
                        }
                        if !paths.is_empty() {
                            self.drag_request = Some(paths);
                        }
                    }

                }
            });

        // Track scroll state for next frame (also captures mouse wheel scrolling)
        self.scroll_offset = scroll_output.state.offset.y;
        self.viewport_h = scroll_output.inner_rect.height();

        // Drop highlight overlay
        if self.drop_highlight {
            let rect = ui.max_rect();
            ui.painter().rect_filled(
                rect,
                0.0,
                egui::Color32::from_rgba_premultiplied(50, 120, 200, 40),
            );
            ui.painter().rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(80, 150, 255)),
                egui::StrokeKind::Outside,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers for recursive search tests ──

    fn make_item(name: &str, path: &str) -> FileItem {
        FileItem {
            name: name.to_string(),
            path: PathBuf::from(path),
            size: 100,
            modified: None,
            is_dir: false,
            is_hidden: false,
            extension: String::new(),
        }
    }

    fn make_hidden_item(name: &str, path: &str) -> FileItem {
        FileItem {
            name: name.to_string(),
            path: PathBuf::from(path),
            size: 100,
            modified: None,
            is_dir: false,
            is_hidden: true,
            extension: String::new(),
        }
    }

    /// Build a FilePanel in recursive search mode with given entries.
    /// No filesystem access — all fields set directly.
    fn make_recursive_panel(entries: Vec<FileItem>) -> FilePanel {
        let mut panel = FilePanel {
            current_dir: PathBuf::from("/test"),
            entries,
            cursor: 0,
            selected: HashSet::new(),
            sort_key: SortKey::Name,
            sort_order: SortOrder::Ascending,
            filter: String::new(),
            show_hidden: true,
            filtered_indices: Vec::new(),
            focus_filter: false,
            filter_has_focus: false,
            drag_request: None,
            drop_highlight: false,
            clicked: false,
            scroll_offset: 0.0,
            viewport_h: 0.0,
            is_loading: false,
            loading_result: Arc::new(Mutex::new(None)),
            loading_generation: 0,
            loading_old_name: None,
            last_dir_modified: None,
            last_dir_check: Instant::now(),
            cursor_history: HashMap::new(),
            cursor_dirty: HashSet::new(),
            sort_history: HashMap::new(),
            sort_dirty: HashSet::new(),
            recursive_filter: true,
            is_searching: false,
            search_sink: Arc::new(Mutex::new(Vec::new())),
            search_done: Arc::new(AtomicBool::new(false)),
        };
        panel.rebuild_filter();
        panel
    }

    // ── remove_paths tests ──

    #[test]
    fn remove_paths_removes_matching_entries() {
        let mut panel = make_recursive_panel(vec![
            make_item("a.txt", "/root/a.txt"),
            make_item("sub/b.txt", "/root/sub/b.txt"),
            make_item("c.txt", "/root/c.txt"),
        ]);
        assert_eq!(panel.visible_count(), 3);

        panel.remove_paths(&[PathBuf::from("/root/sub/b.txt")]);

        assert_eq!(panel.visible_count(), 2);
        assert_eq!(panel.entries[0].name, "a.txt");
        assert_eq!(panel.entries[1].name, "c.txt");
    }

    #[test]
    fn remove_paths_multiple() {
        let mut panel = make_recursive_panel(vec![
            make_item("a.txt", "/root/a.txt"),
            make_item("b.txt", "/root/b.txt"),
            make_item("c.txt", "/root/c.txt"),
        ]);

        panel.remove_paths(&[
            PathBuf::from("/root/a.txt"),
            PathBuf::from("/root/c.txt"),
        ]);

        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].name, "b.txt");
    }

    #[test]
    fn remove_paths_empty_is_noop() {
        let mut panel = make_recursive_panel(vec![make_item("a.txt", "/root/a.txt")]);
        panel.remove_paths(&[]);
        assert_eq!(panel.visible_count(), 1);
    }

    #[test]
    fn remove_paths_nonexistent_is_noop() {
        let mut panel = make_recursive_panel(vec![make_item("a.txt", "/root/a.txt")]);
        panel.remove_paths(&[PathBuf::from("/nonexistent")]);
        assert_eq!(panel.visible_count(), 1);
    }

    #[test]
    fn remove_paths_clears_selection() {
        let mut panel = make_recursive_panel(vec![
            make_item("a.txt", "/root/a.txt"),
            make_item("b.txt", "/root/b.txt"),
        ]);
        panel.selected.insert(0);
        panel.selected.insert(1);

        panel.remove_paths(&[PathBuf::from("/root/a.txt")]);

        assert!(panel.selected.is_empty());
    }

    #[test]
    fn remove_paths_adjusts_cursor() {
        let mut panel = make_recursive_panel(vec![
            make_item("a.txt", "/root/a.txt"),
            make_item("b.txt", "/root/b.txt"),
        ]);
        panel.cursor = 1;

        panel.remove_paths(&[PathBuf::from("/root/b.txt")]);

        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn remove_paths_all_entries() {
        let mut panel = make_recursive_panel(vec![
            make_item("a.txt", "/root/a.txt"),
            make_item("b.txt", "/root/b.txt"),
        ]);
        panel.cursor = 1;

        panel.remove_paths(&[
            PathBuf::from("/root/a.txt"),
            PathBuf::from("/root/b.txt"),
        ]);

        assert_eq!(panel.visible_count(), 0);
        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn remove_paths_rebuilds_filter() {
        let mut panel = make_recursive_panel(vec![
            make_item("a.txt", "/root/a.txt"),
            make_item("b.rs", "/root/b.rs"),
            make_item("c.txt", "/root/c.txt"),
        ]);
        panel.filter = "txt".to_string();
        panel.rebuild_filter();
        assert_eq!(panel.visible_count(), 2); // a.txt, c.txt

        panel.remove_paths(&[PathBuf::from("/root/a.txt")]);

        // b.rs and c.txt remain; filter "txt" shows only c.txt
        assert_eq!(panel.entries.len(), 2);
        assert_eq!(panel.visible_count(), 1);
    }

    // ── refresh in recursive mode tests ──

    #[test]
    fn refresh_recursive_preserves_entries() {
        let mut panel = make_recursive_panel(vec![
            make_item("a.txt", "/root/a.txt"),
            make_item("b.txt", "/root/b.txt"),
        ]);
        panel.selected.insert(0);

        panel.refresh();

        assert_eq!(panel.entries.len(), 2);
        assert!(panel.selected.is_empty());
        assert!(panel.recursive_filter);
    }

    #[test]
    fn refresh_recursive_adjusts_cursor() {
        let mut panel = make_recursive_panel(vec![make_item("a.txt", "/root/a.txt")]);
        panel.cursor = 5;

        panel.refresh();

        assert_eq!(panel.cursor, 0);
    }

    #[test]
    fn remove_paths_with_hidden_filter() {
        let mut panel = make_recursive_panel(vec![
            make_item("a.txt", "/root/a.txt"),
            make_hidden_item(".secret", "/root/.secret"),
            make_item("b.txt", "/root/b.txt"),
        ]);
        panel.show_hidden = false;
        panel.rebuild_filter();
        assert_eq!(panel.visible_count(), 2); // .secret hidden

        panel.remove_paths(&[PathBuf::from("/root/a.txt")]);

        // .secret and b.txt remain; .secret still hidden
        assert_eq!(panel.entries.len(), 2);
        assert_eq!(panel.visible_count(), 1);
    }

    // ── existing tests ──

    #[test]
    fn char_width_ascii() {
        assert_eq!(char_display_width('A'), 1);
        assert_eq!(char_display_width(' '), 1);
    }

    #[test]
    fn char_width_cjk() {
        assert_eq!(char_display_width('あ'), 2);
        assert_eq!(char_display_width('漢'), 2);
        assert_eq!(char_display_width('ア'), 2);
    }

    #[test]
    fn str_width() {
        assert_eq!(str_display_width("hello"), 5);
        assert_eq!(str_display_width("あいう"), 6);
        assert_eq!(str_display_width("aあb"), 4); // 1+2+1
    }

    #[test]
    fn truncate_middle_short_string() {
        assert_eq!(truncate_middle("hello", 10), "hello");
        assert_eq!(truncate_middle("hi", 2), "hi");
    }

    #[test]
    fn truncate_middle_exact_fit() {
        assert_eq!(truncate_middle("hello", 5), "hello");
    }

    #[test]
    fn truncate_middle_needs_truncation() {
        let result = truncate_middle("abcdefghij", 7);
        // keep=6, front=3, back=3 → "abc…hij"
        assert_eq!(result, "abc…hij");
    }

    #[test]
    fn truncate_middle_very_small_max() {
        assert_eq!(truncate_middle("hello", 1), "h");
        assert_eq!(truncate_middle("hello", 2), "he");
        assert_eq!(truncate_middle("hello", 3), "hel");
    }

    #[test]
    fn truncate_middle_japanese() {
        // "あいうえお" = width 10, max_width=7
        let result = truncate_middle("あいうえお", 7);
        // keep=6, front_budget=3 → "あ"(w=2), back_budget=3 → "お"(w=2)
        assert_eq!(result, "あ…お");
    }

    #[test]
    fn truncate_middle_japanese_wider() {
        // "あいうえおかきくけこ" = width 20, max_width=14
        let result = truncate_middle("あいうえおかきくけこ", 14);
        // keep=13, front_budget=7 → "あいう"(w=6), back_budget=6 → "くけこ"(w=6)
        assert_eq!(result, "あいう…くけこ");
    }

    #[test]
    fn truncate_middle_mixed() {
        // "abc漢字def" = 1+1+1+2+2+1+1+1 = 10, max_width=7
        let result = truncate_middle("abc漢字def", 7);
        // keep=6, front_budget=3 → "abc"(w=3), back_budget=3 → "def"(w=3)
        assert_eq!(result, "abc…def");
    }

    #[test]
    fn truncate_middle_japanese_no_truncate() {
        // "あいう" = width 6, max_width=6
        assert_eq!(truncate_middle("あいう", 6), "あいう");
    }

    #[test]
    fn truncate_middle_empty() {
        assert_eq!(truncate_middle("", 5), "");
    }
}
