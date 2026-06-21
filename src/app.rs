use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::Mutex;

use crate::archive_viewer;
use eframe::egui;

use crate::config::Config;
use crate::dialog::*;
use crate::file_item;
use crate::file_ops;
use crate::audio_viewer::{self, AudioPreview};
use crate::image_viewer::{self, ImageCache, ImagePreview};
use crate::keybind::KeyBindings;
use crate::video_viewer::{self, VideoPreview};
use crate::panel::FilePanel;
use crate::undo::UndoHistory;
use crate::viewer::TextPreview;

#[derive(PartialEq)]
enum PreviewKind {
    None,
    Text,
    Archive,
    Image,
    Audio,
    Video,
}

#[derive(PartialEq, Clone, Copy)]
pub enum ActivePanel {
    Left,
    Right,
}

pub struct TabState {
    pub(crate) left_panel: FilePanel,
    pub(crate) right_panel: FilePanel,
    pub(crate) active: ActivePanel,
    pub(crate) text_preview: Option<TextPreview>,
    pub(crate) archive_preview: Option<archive_viewer::ArchivePreview>,
    pub(crate) image_preview: Option<ImagePreview>,
    pub(crate) image_cache: ImageCache,
    pub(crate) audio_preview: Option<AudioPreview>,
    pub(crate) video_preview: Option<VideoPreview>,
    pub(crate) preview_mode: bool,
    preview_mtime: Option<std::time::SystemTime>,
}

impl TabState {
    fn new(
        left_dir: PathBuf,
        right_dir: PathBuf,
        cursor_history: &std::collections::HashMap<PathBuf, String>,
        sort_history: &std::collections::HashMap<PathBuf, (crate::sort::SortKey, crate::sort::SortOrder)>,
    ) -> Self {
        let mut left_panel = FilePanel::new(left_dir);
        let mut right_panel = FilePanel::new(right_dir);
        left_panel.cursor_history = cursor_history.clone();
        right_panel.cursor_history = cursor_history.clone();
        left_panel.restore_cursor_from_history();
        right_panel.restore_cursor_from_history();
        left_panel.sort_history = sort_history.clone();
        right_panel.sort_history = sort_history.clone();
        left_panel.restore_sort_from_history();
        right_panel.restore_sort_from_history();
        TabState {
            left_panel,
            right_panel,
            active: ActivePanel::Left,
            text_preview: None,
            archive_preview: None,
            image_preview: None,
            image_cache: ImageCache::new(),
            audio_preview: None,
            video_preview: None,
            preview_mode: false,
            preview_mtime: None,
        }
    }

    pub(crate) fn active_panel(&self) -> &FilePanel {
        match self.active {
            ActivePanel::Left => &self.left_panel,
            ActivePanel::Right => &self.right_panel,
        }
    }

    pub(crate) fn active_panel_mut(&mut self) -> &mut FilePanel {
        match self.active {
            ActivePanel::Left => &mut self.left_panel,
            ActivePanel::Right => &mut self.right_panel,
        }
    }

    pub(crate) fn inactive_panel(&self) -> &FilePanel {
        match self.active {
            ActivePanel::Left => &self.right_panel,
            ActivePanel::Right => &self.left_panel,
        }
    }

    pub(crate) fn inactive_panel_mut(&mut self) -> &mut FilePanel {
        match self.active {
            ActivePanel::Left => &mut self.right_panel,
            ActivePanel::Right => &mut self.left_panel,
        }
    }

    pub(crate) fn update_preview(&mut self, ctx: &egui::Context) {
        if !self.preview_mode {
            return;
        }
        let entry = self.active_panel().current_entry()
            .filter(|e| !e.is_dir)
            .cloned();

        let entry = match entry {
            Some(e) => e,
            None => {
                self.clear_all_previews();
                return;
            }
        };

        let mtime_changed = self.preview_mtime != entry.modified;

        if audio_viewer::is_audio_file(&entry.path) {
            self.clear_previews_except(PreviewKind::Audio);
            let already_loaded = self.audio_preview.as_ref()
                .is_some_and(|ap| ap.title == entry.name);
            if !already_loaded || mtime_changed {
                if let Some(ap) = &mut self.audio_preview {
                    ap.stop();
                }
                self.audio_preview = audio_viewer::load(&entry.path, ctx);
                self.preview_mtime = entry.modified;
            }
        } else if video_viewer::is_video_file(&entry.path) {
            self.clear_previews_except(PreviewKind::Video);
            let already_loaded = self.video_preview.as_ref()
                .is_some_and(|vp| vp.title == entry.name);
            if !already_loaded || mtime_changed {
                self.stop_video_preview();
                self.video_preview = video_viewer::load(&entry.path, ctx);
                self.preview_mtime = entry.modified;
            }
        } else if image_viewer::is_image_file(&entry.path) {
            self.clear_previews_except(PreviewKind::Image);
            self.image_preview = self.image_cache.get_or_load(ctx, &entry.path, entry.modified);
            self.preview_mtime = entry.modified;
        } else if archive_viewer::is_archive_file(&entry.path) {
            self.clear_previews_except(PreviewKind::Archive);
            let already_loaded = self.archive_preview.as_ref()
                .is_some_and(|ap| ap.title == entry.name);
            if !already_loaded || mtime_changed {
                self.archive_preview = archive_viewer::ArchivePreview::load(&entry.path);
                self.preview_mtime = entry.modified;
            }
        } else {
            self.clear_previews_except(PreviewKind::Text);
            let already_loaded = self.text_preview.as_ref()
                .is_some_and(|tp| tp.title == entry.name);
            if !already_loaded || mtime_changed {
                self.text_preview = TextPreview::load(&entry.path);
                self.preview_mtime = entry.modified;
            }
        }
    }

    pub(crate) fn clear_all_previews(&mut self) {
        self.clear_previews_except(PreviewKind::None);
    }

    fn clear_previews_except(&mut self, keep: PreviewKind) {
        if keep != PreviewKind::Text {
            self.text_preview = None;
        }
        if keep != PreviewKind::Archive {
            self.archive_preview = None;
        }
        if keep != PreviewKind::Image {
            self.image_preview = None;
            self.image_cache.clear_wanted();
        }
        if keep != PreviewKind::Audio {
            if let Some(ap) = &mut self.audio_preview {
                ap.stop();
            }
            self.audio_preview = None;
        }
        if keep != PreviewKind::Video {
            self.stop_video_preview();
        }
    }

    fn stop_video_preview(&mut self) {
        if let Some(vp) = &mut self.video_preview {
            vp.stop();
        }
        self.video_preview = None;
    }

    /// Stop all media playback (for tab switching).
    pub(crate) fn stop_media(&mut self) {
        if let Some(ap) = &mut self.audio_preview {
            ap.stop();
        }
        self.stop_video_preview();
    }
}

pub struct F2App {
    pub(crate) tabs: Vec<TabState>,
    pub(crate) active_tab: usize,
    pub(crate) dialog: DialogState,
    pub(crate) command_line: String,
    pub(crate) command_mode: bool,
    pub(crate) status_message: String,
    pub(crate) status_is_error: bool,
    pub(crate) drives: Arc<Mutex<Vec<String>>>,
    pub(crate) config: Config,
    window_pos: Option<egui::Pos2>,
    window_size: Option<egui::Vec2>,
    pub(crate) undo_history: UndoHistory,
    skip_next_drop: bool,
    pub(crate) sort_pending: bool,
    pub(crate) keybindings: KeyBindings,
    operation_log: VecDeque<String>,
    #[cfg(windows)]
    foreground_hook_installed: bool,
}

impl F2App {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        setup_fonts(&cc.egui_ctx, config.font_path.as_deref(), config.font_size);

        let left_dir = restore_dir(&config.last_left_dir).unwrap_or_else(default_dir);
        let right_dir = restore_dir(&config.last_right_dir).unwrap_or_else(default_dir);

        let drives = Arc::new(Mutex::new(Vec::new()));
        {
            let drives_handle = Arc::clone(&drives);
            let repaint_ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let result = file_ops::get_drives();
                *drives_handle.lock() = result;
                repaint_ctx.request_repaint();
            });
        }

        // Load cursor history from config
        let cursor_history: std::collections::HashMap<PathBuf, String> = config
            .cursor_dirs
            .iter()
            .map(|(k, v)| (PathBuf::from(k), v.clone()))
            .collect();

        // Load sort history from config
        let sort_history: std::collections::HashMap<PathBuf, (crate::sort::SortKey, crate::sort::SortOrder)> = config
            .sort_dirs
            .iter()
            .filter_map(|(k, v)| {
                crate::sort::sort_from_string(v).map(|sort| (PathBuf::from(k), sort))
            })
            .collect();

        // Build tab dir pairs: from saved tabs, or fallback to last_left_dir/last_right_dir
        let tab_dirs: Vec<(PathBuf, PathBuf)> = if config.tabs.is_empty() {
            vec![(left_dir, right_dir)]
        } else {
            config.tabs.iter().map(|tc| {
                let l = restore_dir(&Some(tc.left_dir.clone())).unwrap_or_else(default_dir);
                let r = restore_dir(&Some(tc.right_dir.clone())).unwrap_or_else(default_dir);
                (l, r)
            }).collect()
        };

        let tabs: Vec<TabState> = tab_dirs.into_iter().map(|(l, r)| {
            TabState::new(l, r, &cursor_history, &sort_history)
        }).collect();

        let active_tab = config.active_tab.min(tabs.len().saturating_sub(1));

        let keybindings = match &config.keybindings_override {
            Some(overrides) => KeyBindings::merge_with_defaults(overrides),
            None => KeyBindings::defaults(),
        };

        F2App {
            tabs,
            active_tab,
            dialog: DialogState::default(),
            command_line: String::new(),
            command_mode: false,
            status_message: String::new(),
            status_is_error: false,
            drives,
            window_pos: None,
            window_size: None,
            config,
            undo_history: UndoHistory::new(),
            skip_next_drop: false,
            sort_pending: false,
            keybindings,
            operation_log: VecDeque::new(),
            #[cfg(windows)]
            foreground_hook_installed: false,
        }
    }

    pub(crate) fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
        self.status_is_error = false;
    }

    pub(crate) fn set_status_error(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
        self.status_is_error = true;
    }

    pub(crate) fn tab(&self) -> &TabState {
        &self.tabs[self.active_tab]
    }

    pub(crate) fn tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab]
    }

    pub(crate) fn active_panel(&self) -> &FilePanel {
        self.tab().active_panel()
    }

    pub(crate) fn active_panel_mut(&mut self) -> &mut FilePanel {
        self.tab_mut().active_panel_mut()
    }

    pub(crate) fn inactive_panel(&self) -> &FilePanel {
        self.tab().inactive_panel()
    }

    pub(crate) fn inactive_panel_mut(&mut self) -> &mut FilePanel {
        self.tab_mut().inactive_panel_mut()
    }

    pub(crate) fn update_preview(&mut self, ctx: &egui::Context) {
        self.tab_mut().update_preview(ctx);
    }

    pub(crate) fn clear_all_previews(&mut self) {
        self.tab_mut().clear_all_previews();
    }

    pub(crate) fn refresh_both_panels(&mut self) {
        let tab = &mut self.tabs[self.active_tab];
        tab.left_panel.refresh();
        tab.right_panel.refresh();
    }


    pub(crate) fn new_tab(&mut self, ctx: &egui::Context) {
        let current = &self.tabs[self.active_tab];
        let left_dir = current.left_panel.current_dir.clone();
        let right_dir = current.right_panel.current_dir.clone();

        let cursor_history: std::collections::HashMap<PathBuf, String> = self.config
            .cursor_dirs
            .iter()
            .map(|(k, v)| (PathBuf::from(k), v.clone()))
            .collect();
        let sort_history: std::collections::HashMap<PathBuf, (crate::sort::SortKey, crate::sort::SortOrder)> = self.config
            .sort_dirs
            .iter()
            .filter_map(|(k, v)| {
                crate::sort::sort_from_string(v).map(|sort| (PathBuf::from(k), sort))
            })
            .collect();

        self.tabs.push(TabState::new(left_dir, right_dir, &cursor_history, &sort_history));
        self.active_tab = self.tabs.len() - 1;
        self.set_status(format!("Tab {} created", self.tabs.len()));
        ctx.request_repaint();
    }

    pub(crate) fn close_tab_at(&mut self, idx: usize) {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return;
        }
        self.tabs[idx].stop_media();
        self.tabs.remove(idx);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if self.active_tab > idx {
            self.active_tab -= 1;
        }
        self.set_status(format!("Tab closed ({} remaining)", self.tabs.len()));
    }

    pub(crate) fn switch_to_tab(&mut self, index: usize, ctx: &egui::Context) {
        if index == self.active_tab || index >= self.tabs.len() {
            return;
        }
        self.tabs[self.active_tab].stop_media();
        self.active_tab = index;
        self.update_preview(ctx);
    }

    pub(crate) fn save_config(&mut self) {
        // Save state from all tabs
        for tab in &mut self.tabs {
            tab.left_panel.save_cursor_position();
            tab.right_panel.save_cursor_position();
        }

        // Use active tab for last_left_dir / last_right_dir (backward compat)
        let tab = &self.tabs[self.active_tab];
        self.config.last_left_dir =
            Some(tab.left_panel.current_dir.to_string_lossy().to_string());
        self.config.last_right_dir =
            Some(tab.right_panel.current_dir.to_string_lossy().to_string());

        // Save per-drive last directory from all tabs
        for tab in &self.tabs {
            for panel_dir in [&tab.left_panel.current_dir, &tab.right_panel.current_dir] {
                if let Some(drive) = drive_letter(panel_dir) {
                    self.config.drive_dirs.insert(
                        drive,
                        panel_dir.to_string_lossy().to_string(),
                    );
                }
            }
        }
        // Save per-directory cursor positions (only entries modified this session)
        for tab in &self.tabs {
            for panel in [&tab.left_panel, &tab.right_panel] {
                for dir in &panel.cursor_dirty {
                    let dir_str = dir.to_string_lossy().to_string();
                    if let Some(name) = panel.cursor_history.get(dir as &PathBuf) {
                        self.config.cursor_dirs.insert(dir_str.clone(), name.clone());
                    }
                    self.config.touch_dir(&dir_str);
                }
            }
        }
        // Save per-directory sort state (only entries modified this session)
        for tab in &self.tabs {
            for panel in [&tab.left_panel, &tab.right_panel] {
                for dir in &panel.sort_dirty {
                    let dir_str = dir.to_string_lossy().to_string();
                    if let Some(&(key, order)) = panel.sort_history.get(dir) {
                        self.config.sort_dirs.insert(
                            dir_str.clone(),
                            crate::sort::sort_to_string(key, order),
                        );
                    }
                    self.config.touch_dir(&dir_str);
                }
            }
        }
        // Save tab state
        self.config.tabs = self.tabs.iter().map(|tab| {
            crate::config::TabConfig {
                left_dir: tab.left_panel.current_dir.to_string_lossy().to_string(),
                right_dir: tab.right_panel.current_dir.to_string_lossy().to_string(),
            }
        }).collect();
        self.config.active_tab = self.active_tab;
        // Trim old directory history entries
        self.config.trim_dir_history();
        // Save window position and size
        if let Some(pos) = self.window_pos {
            self.config.window_x = Some(pos.x);
            self.config.window_y = Some(pos.y);
        }
        if let Some(size) = self.window_size {
            self.config.window_width = Some(size.x);
            self.config.window_height = Some(size.y);
        }
        self.config.save();
    }

    pub(crate) fn start_background_op(&mut self, ctx: &egui::Context, op_kind: OpKind) {
        #[cfg(windows)]
        fn elevated_op(
            sources: &[PathBuf],
            dest_dir: &std::path::Path,
            is_move: bool,
            overwrite: bool,
            handle: &file_ops::ProgressHandle,
        ) {
            let verb = if is_move { "Moved" } else { "Copied" };
            match crate::shell::elevated_copy_or_move(sources, dest_dir, is_move, overwrite) {
                Ok(()) => {
                    let succeeded: Vec<PathBuf> = sources.to_vec();
                    for src in sources {
                        let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        handle.log(format!("{} {} (admin)", verb, name));
                    }
                    handle.finish(
                        format!("{} {} item(s) (admin)", verb, sources.len()),
                        succeeded,
                        None,
                        None,
                    );
                }
                Err(e) => {
                    handle.log(format!("Error: {}", e));
                    handle.finish(e.clone(), Vec::new(), Some(e), None);
                }
            }
        }
        #[cfg(not(windows))]
        fn elevated_op(
            _sources: &[PathBuf],
            _dest_dir: &std::path::Path,
            _is_move: bool,
            _overwrite: bool,
            handle: &file_ops::ProgressHandle,
        ) {
            let msg = "Elevated operations are only supported on Windows".to_string();
            handle.finish(msg.clone(), Vec::new(), Some(msg), None);
        }

        let total = match &op_kind {
            OpKind::Copy { sources, .. } => sources.len(),
            OpKind::Move { sources, .. } => sources.len(),
            OpKind::Delete { paths } => paths.len(),
            OpKind::DeletePermanent { paths } => paths.len(),
            OpKind::ZipCompress { sources, .. } => sources.len(),
            OpKind::ZipDecompress { .. } => 1,
            OpKind::TarDecompress { .. } => 1,
            OpKind::StreamDecompress { .. } => 1,
            OpKind::ElevatedCopy { sources, .. } => sources.len(),
            OpKind::ElevatedMove { sources, .. } => sources.len(),
            OpKind::ElevatedDelete { paths } => paths.len(),
        };

        let label = match &op_kind {
            OpKind::Copy { .. } => "Copying",
            OpKind::Move { .. } => "Moving",
            OpKind::Delete { .. } => "Deleting",
            OpKind::DeletePermanent { .. } => "Permanently Deleting",
            OpKind::ZipCompress { .. } => "Compressing",
            OpKind::ZipDecompress { .. } => "Decompressing",
            OpKind::TarDecompress { .. } => "Decompressing",
            OpKind::StreamDecompress { .. } => "Decompressing",
            OpKind::ElevatedCopy { .. } => "Copying (Admin)",
            OpKind::ElevatedMove { .. } => "Moving (Admin)",
            OpKind::ElevatedDelete { .. } => "Deleting (Admin)",
        };

        let progress = file_ops::ProgressHandle::new(label, total);
        let handle_clone = progress.clone();
        let op_kind_clone = op_kind.clone();
        let repaint_ctx = ctx.clone();

        std::thread::spawn(move || {
            match op_kind_clone {
                OpKind::Copy { sources, dest_dir, overwrite } => {
                    file_ops::copy_batch_with_progress(&sources, &dest_dir, overwrite, &handle_clone);
                }
                OpKind::Move { sources, dest_dir, overwrite } => {
                    file_ops::move_batch_with_progress(&sources, &dest_dir, overwrite, &handle_clone);
                }
                OpKind::Delete { paths } => {
                    file_ops::delete_batch_with_progress(&paths, &handle_clone);
                }
                OpKind::DeletePermanent { paths } => {
                    file_ops::delete_permanent_batch_with_progress(&paths, &handle_clone);
                }
                OpKind::ZipCompress { sources, dest_dir, zip_name } => {
                    file_ops::compress_to_zip_with_progress(&sources, &dest_dir, &zip_name, &handle_clone);
                }
                OpKind::ZipDecompress { zip_path, dest_dir } => {
                    file_ops::decompress_zip_with_progress(&zip_path, &dest_dir, &handle_clone);
                }
                OpKind::TarDecompress { tar_path, dest_dir } => {
                    file_ops::decompress_tar_with_progress(&tar_path, &dest_dir, &handle_clone);
                }
                OpKind::StreamDecompress { path, dest_dir } => {
                    file_ops::decompress_stream_with_progress(&path, &dest_dir, &handle_clone);
                }
                OpKind::ElevatedCopy { sources, dest_dir, overwrite } => {
                    let is_move = false;
                    elevated_op(&sources, &dest_dir, is_move, overwrite, &handle_clone);
                }
                OpKind::ElevatedMove { sources, dest_dir, overwrite } => {
                    let is_move = true;
                    elevated_op(&sources, &dest_dir, is_move, overwrite, &handle_clone);
                }
                OpKind::ElevatedDelete { paths } => {
                    #[cfg(windows)]
                    {
                        match crate::shell::elevated_delete(&paths) {
                            Ok(()) => {
                                let succeeded: Vec<PathBuf> = paths.clone();
                                for p in &paths {
                                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                    handle_clone.log(format!("Deleted {} (admin)", name));
                                }
                                handle_clone.finish(
                                    format!("Deleted {} item(s) (admin)", paths.len()),
                                    succeeded,
                                    None,
                                    None,
                                );
                            }
                            Err(e) => {
                                handle_clone.log(format!("Error: {}", e));
                                handle_clone.finish(e.clone(), Vec::new(), Some(e), None);
                            }
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        let msg = "Elevated operations are only supported on Windows".to_string();
                        handle_clone.finish(msg.clone(), Vec::new(), Some(msg), None);
                    }
                }
            }
            repaint_ctx.request_repaint();
        });

        self.dialog.progress.push(ProgressDialog {
            handle: progress,
            op_kind,
            source_tab: self.active_tab,
            log_synced: 0,
        });
    }

    fn handle_file_drop(&mut self, ctx: &egui::Context) {
        // Determine which panel the pointer is over (left half vs right half)
        let screen_rect = ctx.content_rect();
        let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
        let is_left_half = pointer_pos
            .map(|p| p.x < screen_rect.center().x)
            .unwrap_or(true);

        // Hover highlight
        let hovered_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let tab = &mut self.tabs[self.active_tab];
        tab.left_panel.drop_highlight = hovered_files && is_left_half;
        tab.right_panel.drop_highlight = hovered_files && !is_left_half;

        // Process dropped files
        let dropped_files: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });

        if dropped_files.is_empty() {
            return;
        }

        if self.skip_next_drop {
            self.skip_next_drop = false;
            return;
        }

        let tab = &mut self.tabs[self.active_tab];
        let dest_panel = if is_left_half {
            &mut tab.left_panel
        } else {
            &mut tab.right_panel
        };
        let dest = dest_panel.current_dir.clone();

        let conflicts = file_ops::check_conflicts(&dropped_files, &dest);
        if conflicts.is_empty() {
            self.start_background_op(ctx, OpKind::Copy {
                sources: dropped_files,
                dest_dir: dest,
                overwrite: false,
            });
        } else {
            self.dialog.confirm = Some(ConfirmDialog {
                title: "Overwrite?".to_string(),
                message: format!(
                    "The following files already exist:\n{}\n\nOverwrite?",
                    crate::keyboard::format_name_list(&conflicts)
                ),
                action: ConfirmAction::CopyOverwrite {
                    sources: dropped_files,
                    dest,
                },
            });
        }
    }

}

impl eframe::App for F2App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Track window position and size
        ctx.input(|i| {
            if let Some(rect) = i.viewport().outer_rect {
                self.window_pos = Some(rect.min);
            }
            if let Some(rect) = i.viewport().inner_rect {
                self.window_size = Some(rect.size());
            }
        });

        // Force window to visual foreground on focus gain.
        // 外部プロセス（AutoHotkey等）が SetForegroundWindow を呼ぶと、OS上はフォアグラウンドに
        // なるが winit/egui が視覚的なZ順序を更新しないため、ウィンドウが裏に隠れたままになる。
        //
        // egui の ctx.input(|i| i.focused) は外部からのアクティベーションを反映しない。
        // GetForegroundWindow() のポーリングは、非フォーカス時に eframe が update() を
        // 呼ばないため検出が遅れる（200ms間隔のリペイント要求が必要になり無駄）。
        //
        // 解決策: SetWinEventHook(EVENT_SYSTEM_FOREGROUND) でOSレベルのイベントを監視。
        // フォアグラウンド変更の瞬間にコールバックが呼ばれ、AtomicBool フラグをセットし
        // InvalidateRect で eframe を起こす。ポーリング不要。
        //
        // HWND_TOPMOST → HWND_NOTOPMOST: SetForegroundWindow 単体ではZ順序が更新されない
        // ことがあるため、一時的に最前面にしてから解除することでZ順序の再計算を強制する。
        #[cfg(windows)]
        {
            if !self.foreground_hook_installed {
                self.foreground_hook_installed = true;
                crate::focus::install_foreground_hook();
            }

            // フォーカス追従（マウスホバー）で「ウィンドウを前面に移動しない」設定の場合は
            // Z 順序を持ち上げない。キーボードフォーカスは OS のホバー有効化に委ねる。
            if crate::focus::take_foreground_flag() && crate::focus::should_raise_on_foreground()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);

                use windows::Win32::UI::WindowsAndMessaging::{
                    GetForegroundWindow, SetForegroundWindow,
                    SetWindowPos, HWND_TOPMOST, HWND_NOTOPMOST,
                    SWP_NOMOVE, SWP_NOSIZE,
                };
                let fg = unsafe { GetForegroundWindow() };
                if !fg.is_invalid() {
                    unsafe {
                        let _ = SetForegroundWindow(fg);
                        let flags = SWP_NOMOVE | SWP_NOSIZE;
                        let _ = SetWindowPos(fg, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
                        let _ = SetWindowPos(fg, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
                    }
                }
            }
        }

        // Apply dark mode
        ctx.set_visuals(egui::Visuals::dark());

        // Handle keyboard input
        crate::keyboard::handle_keyboard(self, ctx);

        // Check for async directory loading completion
        {
            let tab = &mut self.tabs[self.active_tab];
            let left_loaded = tab.left_panel.check_loading_complete();
            let right_loaded = tab.right_panel.check_loading_complete();
            if left_loaded || right_loaded {
                self.save_config();
                self.update_preview(ctx);
            }
        }

        // Auto-refresh directories when filesystem changes
        {
            let tab = &mut self.tabs[self.active_tab];
            let left_refreshed = tab.left_panel.check_auto_refresh();
            let right_refreshed = tab.right_panel.check_auto_refresh();
            if left_refreshed || right_refreshed {
                self.update_preview(ctx);
            }
        }

        // Check deferred preview update requests from panel UI (sort click, filter Enter)
        if self.active_panel().preview_needs_update {
            self.active_panel_mut().preview_needs_update = false;
            self.update_preview(ctx);
        }

        // Poll background image loading
        {
            let tab = &mut self.tabs[self.active_tab];
            if tab.preview_mode {
                if let Some(preview) = tab.image_cache.poll_loaded(ctx) {
                    tab.image_preview = Some(preview);
                }
            }
        }

        // Handle dialog results
        let result = show_dialogs(ctx, &mut self.dialog);
        crate::dialog_handler::handle_dialog_result(self, ctx, result);

        // Sync log entries from all progress handles
        const MAX_LOG_ENTRIES: usize = 10_000;
        for prog in &mut self.dialog.progress {
            let s = prog.handle.state.lock();
            let sync_idx = prog.log_synced.min(s.log_entries.len());
            for entry in &s.log_entries[sync_idx..] {
                if self.operation_log.len() >= MAX_LOG_ENTRIES {
                    self.operation_log.pop_front();
                }
                self.operation_log.push_back(entry.clone());
            }
            prog.log_synced = s.log_entries.len();
        }

        // Handle finished progress operations (drain completed ones)
        while let Some(idx) = self
            .dialog
            .progress
            .iter()
            .position(|p| p.handle.state.lock().finished)
        {
            // Flush remaining log entries before removing
            {
                let s = self.dialog.progress[idx].handle.state.lock();
                let sync_idx = self.dialog.progress[idx].log_synced.min(s.log_entries.len());
                for entry in &s.log_entries[sync_idx..] {
                    if self.operation_log.len() >= MAX_LOG_ENTRIES {
                        self.operation_log.pop_front();
                    }
                    self.operation_log.push_back(entry.clone());
                }
            }
            let finished = self.dialog.progress.remove(idx);
            crate::dialog_handler::handle_progress_finished(self, ctx, finished);
        }

        // Bottom panel: status bar + command line (drawn first = bottommost)
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            // Command line
            if self.command_mode {
                ui.horizontal(|ui| {
                    ui.label(":");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.command_line)
                            .desired_width(ui.available_width()),
                    );
                    response.request_focus();

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.command_mode = false;
                        self.command_line.clear();
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.execute_command(ctx);
                        self.command_mode = false;
                    }
                });
            }

            // Status bar
            ui.horizontal(|ui| {
                let panel = self.active_panel();
                let total_files = panel.visible_count();
                let selected_count = panel.selected.len();
                let selected_size = panel.selected_total_size();

                let mut left_text = format!(
                    "{} items | {} selected | {}",
                    total_files,
                    selected_count,
                    file_item::format_size(selected_size),
                );
                if let Some(ref full) = panel.cursor_full_name {
                    left_text.push_str(&format!(" | {}", full));
                }
                ui.label(left_text);

                if !self.status_message.is_empty() {
                    ui.separator();
                    let reserved_for_time = 200.0;
                    let max_width = (ui.available_width() - reserved_for_time).max(50.0);
                    let text = if self.status_is_error {
                        egui::RichText::new(&self.status_message)
                            .color(egui::Color32::from_rgb(255, 80, 80))
                            .strong()
                    } else {
                        egui::RichText::new(&self.status_message)
                    };
                    let response = ui.add_sized(
                        [max_width, ui.spacing().interact_size.y],
                        egui::Label::new(text).truncate().sense(egui::Sense::click()),
                    );
                    if response.clicked() {
                        if let Ok(mut clip) = arboard::Clipboard::new() {
                            if clip.set_text(&self.status_message).is_ok() {
                                self.set_status("Copied to clipboard");
                            }
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let now = chrono::Local::now();
                    let weekday_en = now.format("%a").to_string();
                    let weekday = match weekday_en.as_str() {
                        "Mon" => "月", "Tue" => "火", "Wed" => "水",
                        "Thu" => "木", "Fri" => "金", "Sat" => "土",
                        "Sun" => "日", other => other,
                    };
                    ui.label(format!("{}{}{}", now.format("%Y/%m/%d"), weekday, now.format("%H:%M:%S")));
                    ctx.request_repaint_after(std::time::Duration::from_secs(1));
                });
            });
        });

        // Operation log panel (above status bar, shown when log is non-empty or progress active)
        let show_log = !self.operation_log.is_empty() || !self.dialog.progress.is_empty();
        if show_log {
            let line_height = ctx.style().text_styles[&egui::TextStyle::Body].size + 4.0;
            egui::TopBottomPanel::bottom("operation_log")
                .resizable(true)
                .default_height(line_height * 5.0)
                .height_range(line_height * 2.0..=line_height * 30.0)
                .show(ctx, |ui| {
                    // Progress bars (fixed at top, outside scroll)
                    for progress in &self.dialog.progress {
                        let s = progress.handle.state.lock();
                        let fraction = if s.total_bytes > 0 {
                            s.completed_bytes as f32 / s.total_bytes as f32
                        } else if s.total > 0 {
                            s.completed as f32 / s.total as f32
                        } else {
                            0.0
                        };
                        let size_text = if s.total_bytes > 0 {
                            format!(
                                "{} / {}",
                                file_item::format_size(s.completed_bytes),
                                file_item::format_size(s.total_bytes),
                            )
                        } else {
                            format!("{} / {}", s.completed, s.total)
                        };
                        let op_label = s.op_label.as_str();
                        ui.horizontal(|ui| {
                            ui.label(op_label);
                            ui.add(
                                egui::ProgressBar::new(fraction)
                                    .desired_width(150.0)
                                    .show_percentage(),
                            );
                            ui.label(size_text);
                            if ui.small_button("Cancel").clicked() {
                                progress.handle.cancel();
                            }
                        });
                    }
                    if !self.dialog.progress.is_empty() {
                        ui.separator();
                        ctx.request_repaint();
                    }

                    // Scrollable log (virtualized)
                    let log_len = self.operation_log.len();
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show_rows(ui, line_height, log_len, |ui, row_range| {
                            for row in row_range {
                                if let Some(entry) = self.operation_log.get(row) {
                                    ui.label(entry.as_str());
                                }
                            }
                        });
                });

            // Escape key cancellation (cancel all active operations)
            if !self.dialog.progress.is_empty()
                && ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                for p in &self.dialog.progress {
                    p.handle.cancel();
                }
            }
        }

        // Handle external file drops
        self.handle_file_drop(ctx);

        // Tab bar (only shown when multiple tabs exist)
        if self.tabs.len() > 1 {
            egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let mut switch_to = None;
                    let mut close_idx = None;
                    for i in 0..self.tabs.len() {
                        let is_active = i == self.active_tab;
                        let tab = &self.tabs[i];
                        let label = tab.active_panel()
                            .current_dir
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| tab.active_panel().current_dir.to_string_lossy().to_string());
                        let tab_text = format!(" {} ", label);

                        let response = ui.selectable_label(is_active, &tab_text);
                        if response.clicked() && !is_active {
                            switch_to = Some(i);
                        }
                        // Middle-click to close tab
                        if response.middle_clicked() {
                            close_idx = Some(i);
                        }
                        // Right-click close button via secondary click
                        response.context_menu(|ui| {
                            if ui.button("Close tab").clicked() {
                                close_idx = Some(i);
                                ui.close();
                            }
                        });
                    }

                    // "+" button for new tab
                    if ui.small_button("+").clicked() {
                        self.new_tab(ctx);
                    }

                    if let Some(idx) = switch_to {
                        self.switch_to_tab(idx, ctx);
                    }
                    if let Some(idx) = close_idx {
                        self.close_tab_at(idx);
                    }
                });
            });
        }

        // Central panel: two file panels side by side
        egui::CentralPanel::default().show(ctx, |ui| {
            let tab = &mut self.tabs[self.active_tab];
            let active = tab.active;
            let left_panel = &mut tab.left_panel;
            let right_panel = &mut tab.right_panel;
            let text_preview = &tab.text_preview;
            let archive_preview = &tab.archive_preview;
            let image_preview = &tab.image_preview;
            let audio_preview = &mut tab.audio_preview;
            let video_preview = &mut tab.video_preview;
            let left_is_inactive = active == ActivePanel::Right;
            let right_is_inactive = active == ActivePanel::Left;
            let has_preview = text_preview.is_some() || archive_preview.is_some() || image_preview.is_some() || audio_preview.is_some() || video_preview.is_some();

            ui.columns(2, |columns| {
                // Left panel
                let is_active = active == ActivePanel::Left;
                egui::Frame::default()
                    .inner_margin(4.0)
                    .stroke(egui::Stroke::new(
                        if is_active { 2.0 } else { 1.0 },
                        if is_active {
                            egui::Color32::from_rgb(80, 120, 200)
                        } else {
                            egui::Color32::from_rgb(60, 60, 60)
                        },
                    ))
                    .show(&mut columns[0], |ui| {
                        if left_is_inactive && has_preview {
                            if let Some(vp) = video_preview.as_mut() {
                                vp.ui(ui);
                            } else if let Some(ap) = audio_preview.as_mut() {
                                ap.ui(ui);
                            } else if let Some(ip) = image_preview.as_ref() {
                                ip.ui(ui);
                            } else if let Some(arp) = archive_preview.as_ref() {
                                arp.ui(ui);
                            } else if let Some(tp) = text_preview.as_ref() {
                                tp.ui(ui);
                            }
                            // Drop highlight on preview panel
                            if left_panel.drop_highlight {
                                paint_drop_highlight(ui);
                            }
                        } else {
                            left_panel.ui(ui, is_active, "left_panel");
                        }
                    });

                // Right panel
                let is_active = active == ActivePanel::Right;
                egui::Frame::default()
                    .inner_margin(4.0)
                    .stroke(egui::Stroke::new(
                        if is_active { 2.0 } else { 1.0 },
                        if is_active {
                            egui::Color32::from_rgb(80, 120, 200)
                        } else {
                            egui::Color32::from_rgb(60, 60, 60)
                        },
                    ))
                    .show(&mut columns[1], |ui| {
                        if right_is_inactive && has_preview {
                            if let Some(vp) = video_preview.as_mut() {
                                vp.ui(ui);
                            } else if let Some(ap) = audio_preview.as_mut() {
                                ap.ui(ui);
                            } else if let Some(ip) = image_preview.as_ref() {
                                ip.ui(ui);
                            } else if let Some(arp) = archive_preview.as_ref() {
                                arp.ui(ui);
                            } else if let Some(tp) = text_preview.as_ref() {
                                tp.ui(ui);
                            }
                            // Drop highlight on preview panel
                            if right_panel.drop_highlight {
                                paint_drop_highlight(ui);
                            }
                        } else {
                            right_panel.ui(ui, is_active, "right_panel");
                        }
                    });
            });

            // Click on inactive panel → switch active panel
            let mut switch_to = None;
            if left_panel.clicked {
                left_panel.clicked = false;
                switch_to = Some(ActivePanel::Left);
            }
            if right_panel.clicked {
                right_panel.clicked = false;
                switch_to = Some(ActivePanel::Right);
            }

            // Handle outbound drag (App → External)
            #[cfg(windows)]
            {
                let drag_paths = left_panel
                    .drag_request
                    .take()
                    .or_else(|| right_panel.drag_request.take());
                if let Some(paths) = drag_paths {
                    let was_move = crate::drag_drop::start_drag(&paths);
                    // After OLE drag completes, ignore the next drop event
                    // (it may be the same files dropped back onto this window)
                    self.skip_next_drop = true;
                    if was_move {
                        left_panel.refresh();
                        right_panel.refresh();
                    }
                }
            }

            if let Some(panel) = switch_to {
                self.tabs[self.active_tab].active = panel;
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for tab in &mut self.tabs {
            tab.stop_media();
        }
        self.save_config();
    }
}

impl F2App {
    fn execute_command(&mut self, ctx: &egui::Context) {
        let cmd = self.command_line.trim().to_string();
        match cmd.as_str() {
            "q" | "quit" => {
                self.save_config();
            }
            "refresh" | "r" => {
                self.active_panel_mut().refresh();
                self.set_status("Refreshed");
            }
            "hidden" => {
                let show = !self.active_panel().show_hidden;
                self.active_panel_mut().show_hidden = show;
                self.active_panel_mut().refresh();
                self.set_status(format!(
                    "Hidden files: {}",
                    if show { "shown" } else { "hidden" }
                ));
            }
            _ if cmd.starts_with("cd ") => {
                let target = cmd[3..].trim();
                let path = PathBuf::from(target);
                if path.is_dir() {
                    self.active_panel_mut().navigate_to(path, ctx);
                    self.save_config();
                } else {
                    self.set_status_error(format!("Directory not found: {}", target));
                }
            }
            _ => {
                self.set_status_error(format!("Unknown command: {}", cmd));
            }
        }
        self.command_line.clear();
    }
}

/// Resolve drive path in background thread (no UI blocking).
/// Extracted from F2App::resolve_drive_path for use in background threads.
pub(crate) fn resolve_drive_path_bg(saved_dir: Option<String>, drive: &str) -> PathBuf {
    if let Some(saved) = saved_dir {
        let path = PathBuf::from(&saved);
        if path.exists() {
            return path;
        }
    }
    if let Some(distro) = drive.strip_prefix("WSL:") {
        for base in &[r"\\wsl$", r"\\wsl.localhost"] {
            let path = PathBuf::from(format!(r"{}\{}", base, distro));
            if path.exists() {
                return path;
            }
        }
        return PathBuf::from(format!(r"\\wsl$\{}", distro));
    }
    if drive.starts_with(r"\\") {
        return PathBuf::from(format!(r"{}\", drive));
    }
    PathBuf::from(format!("{}\\", drive))
}

fn default_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| {
        #[cfg(windows)]
        {
            PathBuf::from("C:\\")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("/")
        }
    })
}

fn paint_drop_highlight(ui: &mut egui::Ui) {
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

/// Extract drive identifier from a path.
/// Returns "C:" for regular drives, "WSL:distro" for WSL UNC paths,
/// or "\\\\server\share" for other UNC paths.
fn drive_letter(path: &std::path::Path) -> Option<String> {
    use std::path::Component;
    for comp in path.components() {
        if let Component::Prefix(prefix) = comp {
            match prefix.kind() {
                std::path::Prefix::UNC(server, share) => {
                    let server = server.to_string_lossy();
                    let share = share.to_string_lossy();
                    // WSL paths: use "WSL:distro" as drive identifier
                    if server.eq_ignore_ascii_case("wsl.localhost")
                        || server.eq_ignore_ascii_case("wsl$")
                    {
                        return Some(format!("WSL:{}", share));
                    }
                    // Generic UNC: use "\\server\share" as drive identifier
                    return Some(format!(r"\\{}\{}", server, share));
                }
                std::path::Prefix::Disk(letter) => {
                    return Some(format!("{}:", (letter as char).to_ascii_uppercase()));
                }
                _ => return None,
            }
        }
    }
    None
}

fn restore_dir(saved: &Option<String>) -> Option<PathBuf> {
    saved.as_ref().and_then(|p| {
        let path = PathBuf::from(p);
        if path.exists() { Some(path) } else { None }
    })
}

pub(crate) fn first_char_upper(s: &str, fallback: char) -> String {
    s.chars()
        .next()
        .unwrap_or(fallback)
        .to_uppercase()
        .next()
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn batch_op<F, E>(paths: &[PathBuf], verb: &str, op: F) -> (String, Vec<PathBuf>)
    where
        F: Fn(&Path) -> Result<(), E>,
        E: std::fmt::Display,
    {
        let mut succeeded = Vec::new();
        let mut errors = Vec::new();
        for p in paths {
            match op(p) {
                Ok(()) => succeeded.push(p.clone()),
                Err(e) => errors.push(e.to_string()),
            }
        }
        let msg = if errors.is_empty() {
            format!("{} {} item(s)", verb, paths.len())
        } else {
            format!("Errors: {}", errors.join(", "))
        };
        (msg, succeeded)
    }

    #[test]
    fn first_char_upper_normal() {
        assert_eq!(first_char_upper("hello", 'X'), "H");
        assert_eq!(first_char_upper("world", 'X'), "W");
    }

    #[test]
    fn first_char_upper_already_upper() {
        assert_eq!(first_char_upper("Hello", 'X'), "H");
    }

    #[test]
    fn first_char_upper_empty() {
        assert_eq!(first_char_upper("", 'X'), "X");
    }

    #[test]
    fn first_char_upper_japanese() {
        assert_eq!(first_char_upper("あいう", 'X'), "あ");
    }

    #[test]
    fn batch_op_all_success() {
        let paths = vec![PathBuf::from("a"), PathBuf::from("b")];
        let (msg, succeeded) = batch_op(&paths, "Processed", |_| Ok::<(), String>(()));
        assert_eq!(msg, "Processed 2 item(s)");
        assert_eq!(succeeded.len(), 2);
    }

    #[test]
    fn batch_op_with_errors() {
        let paths = vec![PathBuf::from("a"), PathBuf::from("b")];
        let (msg, succeeded) = batch_op(&paths, "Processed", |p| {
            if p == Path::new("a") {
                Err("fail".to_string())
            } else {
                Ok(())
            }
        });
        assert!(msg.starts_with("Errors:"));
        assert!(msg.contains("fail"));
        assert_eq!(succeeded.len(), 1);
        assert_eq!(succeeded[0], PathBuf::from("b"));
    }

    #[test]
    fn batch_op_empty() {
        let paths: Vec<PathBuf> = vec![];
        let (msg, succeeded) = batch_op(&paths, "Done", |_| Ok::<(), String>(()));
        assert_eq!(msg, "Done 0 item(s)");
        assert!(succeeded.is_empty());
    }

    #[test]
    fn drive_letter_windows_path() {
        assert_eq!(drive_letter(Path::new("C:\\Users\\foo")), Some("C:".to_string()));
        assert_eq!(drive_letter(Path::new("D:\\data")), Some("D:".to_string()));
    }

    #[test]
    fn drive_letter_no_drive() {
        assert_eq!(drive_letter(Path::new("/home/user")), None);
        assert_eq!(drive_letter(Path::new("")), None);
    }

    #[test]
    fn restore_dir_none() {
        assert!(restore_dir(&None).is_none());
    }

    #[test]
    fn restore_dir_nonexistent() {
        let saved = Some("/nonexistent/path/12345".to_string());
        assert!(restore_dir(&saved).is_none());
    }

    #[test]
    fn restore_dir_exists() {
        let dir = std::env::current_dir().unwrap();
        let saved = Some(dir.to_string_lossy().to_string());
        assert_eq!(restore_dir(&saved), Some(dir));
    }
}

pub(crate) const DEFAULT_FONT_SIZE: f32 = 16.0;

pub(crate) fn setup_fonts(ctx: &egui::Context, font_path: Option<&str>, font_size: Option<f32>) {
    let mut fonts = egui::FontDefinitions::default();
    let mut needs_update = false;

    // Load custom font if specified
    if let Some(path) = font_path {
        if let Ok(font_data) = std::fs::read(path) {
            fonts.font_data.insert(
                "CustomFont".to_string(),
                egui::FontData::from_owned(font_data).into(),
            );
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().insert(0, "CustomFont".to_string());
            }
            needs_update = true;
        }
    }

    // Load Japanese system font as primary (or after custom font).
    // When a custom font is set: CustomFont → JapaneseFont → built-in
    // When no custom font:       JapaneseFont → built-in
    let jp_font_candidates = [
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\YuGothR.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ];
    for jp_path in &jp_font_candidates {
        if let Ok(data) = std::fs::read(jp_path) {
            fonts.font_data.insert(
                "JapaneseFont".to_string(),
                egui::FontData::from_owned(data).into(),
            );
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                let list = fonts.families.entry(family).or_default();
                let pos = list.iter().position(|n| n == "CustomFont").map_or(0, |i| i + 1);
                list.insert(pos, "JapaneseFont".to_string());
            }
            needs_update = true;
            break;
        }
    }

    if needs_update {
        ctx.set_fonts(fonts);
    }

    apply_font_size(ctx, font_size);
}

pub(crate) fn apply_font_size(ctx: &egui::Context, font_size: Option<f32>) {
    let size = font_size.unwrap_or(DEFAULT_FONT_SIZE);
    let small = (size * 0.75).round();
    let heading = (size * 1.375).round();

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(small, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(size, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(size, egui::FontFamily::Monospace),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(size, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(heading, egui::FontFamily::Proportional),
    );
    ctx.set_style(style);
}
