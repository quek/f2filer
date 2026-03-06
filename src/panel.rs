use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui;

use crate::file_item::{read_directory, FileItem};
use crate::sort::{sort_entries, SortKey, SortOrder};

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
            show_hidden: false,
            filtered_indices: Vec::new(),
            focus_filter: false,
            filter_has_focus: false,
        };
        panel.refresh();
        panel
    }

    pub fn refresh(&mut self) {
        self.entries = read_directory(&self.current_dir);
        sort_entries(&mut self.entries, self.sort_key, self.sort_order);
        self.rebuild_filter();
        self.selected.clear();
        if self.cursor >= self.visible_count() {
            self.cursor = self.visible_count().saturating_sub(1);
        }
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
                if item.name == ".." {
                    return true;
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
    }

    pub fn navigate_to(&mut self, dir: PathBuf) {
        let old_dir_name = self.current_dir.file_name().map(|n| n.to_string_lossy().to_string());
        self.current_dir = dir;
        self.cursor = 0;
        self.filter.clear();
        self.refresh();

        // If going up, try to position cursor on the directory we came from
        if let Some(old_name) = old_dir_name {
            for (i, idx) in self.filtered_indices.iter().enumerate() {
                if self.entries[*idx].name == old_name {
                    self.cursor = i;
                    break;
                }
            }
        }
    }

    pub fn move_cursor(&mut self, delta: i32) {
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        let new = (self.cursor as i32 + delta).clamp(0, count as i32 - 1) as usize;
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
            // Don't allow selecting ".."
            if self.entries[real_idx].name == ".." {
                return;
            }
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
            if self.entries[idx].name != ".." {
                self.selected.insert(idx);
            }
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

        // Current path
        ui.horizontal(|ui| {
            ui.strong(self.current_dir.to_string_lossy().to_string());
        });

        // Filter input
        ui.horizontal(|ui| {
            ui.label("Filter:");
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
                if !self.filter.is_empty() {
                    for (vis_idx, &real_idx) in self.filtered_indices.iter().enumerate() {
                        if let Some(entry) = self.entries.get(real_idx) {
                            if entry.name != ".." {
                                self.cursor = vis_idx;
                                break;
                            }
                        }
                    }
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

        // Column headers
        let mut sort_clicked: Option<SortKey> = None;
        let cur_sort_key = self.sort_key;
        let cur_sort_order = self.sort_order;

        ui.horizontal(|ui| {
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

            let w = ui.available_width();
            let name_w = w * 0.40;
            let ext_w = w * 0.10;
            let size_w = w * 0.18;

            if ui
                .add_sized(
                    [name_w, 18.0],
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
                    [ext_w, 18.0],
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
                    [size_w, 18.0],
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
                    [ui.available_width(), 18.0],
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

        // File list
        let row_height = 20.0;
        let visible_count = self.visible_count();

        egui::ScrollArea::vertical()
            .id_salt(panel_id.with("scroll"))
            .auto_shrink([false; 2])
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

                    ui.horizontal(|ui| {
                        let w = ui.available_width();
                        let name_w = w * 0.40;
                        let ext_w = w * 0.10;
                        let size_w = w * 0.18;

                        let name_display = if entry.is_dir && entry.name != ".." {
                            format!("[{}]", entry.name)
                        } else {
                            entry.name.clone()
                        };

                        // Mark selected items
                        let mark = if is_sel { "*" } else { " " };
                        let name_text = format!("{}{}", mark, name_display);

                        ui.add_sized(
                            [name_w, row_height],
                            egui::Label::new(
                                egui::RichText::new(name_text).color(text_color).monospace(),
                            ),
                        );

                        ui.add_sized(
                            [ext_w, row_height],
                            egui::Label::new(
                                egui::RichText::new(&entry.extension)
                                    .color(text_color)
                                    .monospace(),
                            ),
                        );

                        ui.add_sized(
                            [size_w, row_height],
                            egui::Label::new(
                                egui::RichText::new(entry.formatted_size())
                                    .color(text_color)
                                    .monospace(),
                            ),
                        );

                        ui.add_sized(
                            [ui.available_width(), row_height],
                            egui::Label::new(
                                egui::RichText::new(entry.formatted_date())
                                    .color(text_color)
                                    .monospace(),
                            ),
                        );
                    });

                    // Scroll to cursor
                    if is_cursor {
                        ui.scroll_to_rect(row_rect, Some(egui::Align::Center));
                    }
                }
            });
    }
}
