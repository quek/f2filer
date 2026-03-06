use std::path::{Path, PathBuf};

use eframe::egui;

use crate::config::Config;
use crate::dialog::*;
use crate::file_item;
use crate::file_ops;
use crate::audio_viewer::{self, AudioPreview};
use crate::image_viewer::{self, ImageCache, ImagePreview};
use crate::video_viewer::{self, VideoPreview};
use crate::panel::FilePanel;
use crate::undo::{FileOperation, UndoHistory};
use crate::viewer::FileViewer;

#[derive(PartialEq, Clone, Copy)]
pub enum ActivePanel {
    Left,
    Right,
}

pub struct F2App {
    left_panel: FilePanel,
    right_panel: FilePanel,
    active: ActivePanel,
    dialog: DialogState,
    viewer: Option<FileViewer>,
    image_preview: Option<ImagePreview>,
    image_cache: ImageCache,
    audio_preview: Option<AudioPreview>,
    video_preview: Option<VideoPreview>,
    preview_mode: bool,
    command_line: String,
    command_mode: bool,
    status_message: String,
    drives: Vec<String>,
    config: Config,
    window_pos: Option<egui::Pos2>,
    window_size: Option<egui::Vec2>,
    undo_history: UndoHistory,
}

impl F2App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load HackGen font
        setup_fonts(&cc.egui_ctx);

        let config = Config::load();

        let left_dir = restore_dir(&config.last_left_dir).unwrap_or_else(default_dir);
        let right_dir = restore_dir(&config.last_right_dir).unwrap_or_else(default_dir);

        let drives = file_ops::get_drives();

        F2App {
            left_panel: FilePanel::new(left_dir),
            right_panel: FilePanel::new(right_dir),
            active: ActivePanel::Left,
            dialog: DialogState::default(),
            viewer: None,
            image_preview: None,
            image_cache: ImageCache::new(),
            audio_preview: None,
            video_preview: None,
            preview_mode: false,
            command_line: String::new(),
            command_mode: false,
            status_message: String::new(),
            drives,
            window_pos: None,
            window_size: None,
            config,
            undo_history: UndoHistory::new(),
        }
    }

    fn active_panel(&self) -> &FilePanel {
        match self.active {
            ActivePanel::Left => &self.left_panel,
            ActivePanel::Right => &self.right_panel,
        }
    }

    fn active_panel_mut(&mut self) -> &mut FilePanel {
        match self.active {
            ActivePanel::Left => &mut self.left_panel,
            ActivePanel::Right => &mut self.right_panel,
        }
    }

    fn inactive_panel(&self) -> &FilePanel {
        match self.active {
            ActivePanel::Left => &self.right_panel,
            ActivePanel::Right => &self.left_panel,
        }
    }

    fn inactive_panel_mut(&mut self) -> &mut FilePanel {
        match self.active {
            ActivePanel::Left => &mut self.right_panel,
            ActivePanel::Right => &mut self.left_panel,
        }
    }

    fn update_preview(&mut self, ctx: &egui::Context) {
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

        if audio_viewer::is_audio_file(&entry.path) {
            // Audio file
            self.image_preview = None;
            self.image_cache.clear_wanted();
            self.stop_video_preview();
            // Only reload if different file
            let already_loaded = self.audio_preview.as_ref()
                .map(|ap| ap.title == entry.name)
                .unwrap_or(false);
            if !already_loaded {
                if let Some(ap) = &mut self.audio_preview {
                    ap.stop();
                }
                self.audio_preview = audio_viewer::load(&entry.path, ctx);
            }
        } else if video_viewer::is_video_file(&entry.path) {
            // Video file
            self.image_preview = None;
            self.image_cache.clear_wanted();
            if let Some(ap) = &mut self.audio_preview {
                ap.stop();
            }
            self.audio_preview = None;
            // Only reload if different file
            let already_loaded = self.video_preview.as_ref()
                .map(|vp| vp.title == entry.name)
                .unwrap_or(false);
            if !already_loaded {
                self.stop_video_preview();
                self.video_preview = video_viewer::load(&entry.path, ctx);
            }
        } else if image_viewer::is_image_file(&entry.path) {
            // Image file
            if let Some(ap) = &mut self.audio_preview {
                ap.stop();
            }
            self.audio_preview = None;
            self.stop_video_preview();
            self.image_preview = self.image_cache.get_or_load(ctx, &entry.path);
        } else {
            // None
            self.clear_all_previews();
        }
    }

    fn clear_all_previews(&mut self) {
        self.image_preview = None;
        self.image_cache.clear_wanted();
        if let Some(ap) = &mut self.audio_preview {
            ap.stop();
        }
        self.audio_preview = None;
        self.stop_video_preview();
    }

    fn stop_video_preview(&mut self) {
        if let Some(vp) = &mut self.video_preview {
            vp.stop();
        }
        self.video_preview = None;
    }

    fn save_config(&mut self) {
        self.config.last_left_dir =
            Some(self.left_panel.current_dir.to_string_lossy().to_string());
        self.config.last_right_dir =
            Some(self.right_panel.current_dir.to_string_lossy().to_string());
        // Save per-drive last directory
        for panel_dir in [&self.left_panel.current_dir, &self.right_panel.current_dir] {
            if let Some(drive) = drive_letter(panel_dir) {
                self.config.drive_dirs.insert(
                    drive,
                    panel_dir.to_string_lossy().to_string(),
                );
            }
        }
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

    /// Resolve drive path: use saved per-drive directory if available, otherwise drive root.
    fn resolve_drive_path(&self, drive: &str) -> PathBuf {
        if let Some(saved) = self.config.drive_dirs.get(drive) {
            let path = PathBuf::from(saved);
            if path.exists() {
                return path;
            }
        }
        PathBuf::from(format!("{}\\", drive))
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        // Don't handle keys when dialog is open or viewer is active or command mode
        if self.dialog.is_open() {
            return;
        }
        if self.viewer.as_ref().is_some_and(|v| v.open) {
            return;
        }
        if self.command_mode {
            return;
        }
        if self.active_panel().filter_has_focus {
            return;
        }

        let input = ctx.input(|i| {
            KeyState {
                tab: i.key_pressed(egui::Key::I),
                j: i.key_pressed(egui::Key::J),
                k: i.key_pressed(egui::Key::K),
                h: i.key_pressed(egui::Key::H),
                l: i.key_pressed(egui::Key::L),
                up: i.key_pressed(egui::Key::ArrowUp),
                down: i.key_pressed(egui::Key::ArrowDown),
                space: i.key_pressed(egui::Key::Space),
                insert: i.key_pressed(egui::Key::Insert),
                home: i.key_pressed(egui::Key::Home),
                end: i.key_pressed(egui::Key::End),
                page_up: i.key_pressed(egui::Key::PageUp),
                page_down: i.key_pressed(egui::Key::PageDown),
                f3: i.key_pressed(egui::Key::F3),
                c: i.key_pressed(egui::Key::C) && !i.modifiers.ctrl,
                m: i.key_pressed(egui::Key::M),
                d: i.key_pressed(egui::Key::D) && !i.modifiers.shift,
                shift_d: i.key_pressed(egui::Key::D) && i.modifiers.shift,
                r: i.key_pressed(egui::Key::R) && !i.modifiers.ctrl,
                n: i.key_pressed(egui::Key::N),
                o: i.key_pressed(egui::Key::O),
                ctrl_a: i.key_pressed(egui::Key::A) && i.modifiers.ctrl,
                ctrl_r: i.key_pressed(egui::Key::R) && i.modifiers.ctrl,
                ctrl_q: i.key_pressed(egui::Key::Q) && i.modifiers.ctrl,
                period: i.key_pressed(egui::Key::Period) && i.modifiers.ctrl,
                colon: i.key_pressed(egui::Key::Semicolon),
                question: i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "?")),
                p: i.key_pressed(egui::Key::P),
                f: i.key_pressed(egui::Key::F),
                v: i.key_pressed(egui::Key::V) && !i.modifiers.ctrl,
                enter: i.key_pressed(egui::Key::Enter),
                g: i.key_pressed(egui::Key::G) && !i.modifiers.shift,
                shift_g: i.key_pressed(egui::Key::G) && i.modifiers.shift,
                u: i.key_pressed(egui::Key::U) && !i.modifiers.shift && !i.modifiers.ctrl,
                shift_u: i.key_pressed(egui::Key::U) && i.modifiers.shift,
                z: i.key_pressed(egui::Key::Z) && !i.modifiers.shift && !i.modifiers.ctrl,
                shift_z: i.key_pressed(egui::Key::Z) && i.modifiers.shift,
            }
        });

        // Tab: switch panel
        if input.tab {
            self.active = match self.active {
                ActivePanel::Left => ActivePanel::Right,
                ActivePanel::Right => ActivePanel::Left,
            };
        }

        // Navigation
        if input.j || input.down {
            self.active_panel_mut().move_cursor(1);
        }
        if input.k || input.up {
            self.active_panel_mut().move_cursor(-1);
        }
        if input.home {
            self.active_panel_mut().move_cursor_to_start();
        }
        if input.end {
            self.active_panel_mut().move_cursor_to_end();
        }
        if input.page_up {
            self.active_panel_mut().page_up(20);
        }
        if input.page_down {
            self.active_panel_mut().page_down(20);
        }

        // l / Enter: open dir / execute file
        if input.l || input.enter {
            if let Some(entry) = self.active_panel().current_entry().cloned() {
                if entry.is_dir {
                    let dir = entry.path.clone();
                    self.active_panel_mut().navigate_to(dir);
                    self.save_config();
                } else {
                    open::that(&entry.path).ok();
                }
            }
        }

        // h: parent directory
        if input.h {
            if let Some(parent) = self.active_panel().current_dir.parent().map(|p| p.to_path_buf())
            {
                self.active_panel_mut().navigate_to(parent);
                self.save_config();
            }
        }

        // Space/Insert: toggle selection
        if input.space || input.insert {
            self.active_panel_mut().toggle_select();
            self.active_panel_mut().move_cursor(1);
        }

        // Ctrl+A: select all
        if input.ctrl_a {
            self.active_panel_mut().select_all();
        }

        // Ctrl+R: refresh
        if input.ctrl_r {
            self.active_panel_mut().refresh();
            self.status_message = "Refreshed".to_string();
        }

        // Ctrl+Q: quit
        if input.ctrl_q {
            self.save_config();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Ctrl+.: toggle hidden
        if input.period {
            let show = !self.active_panel().show_hidden;
            self.active_panel_mut().show_hidden = show;
            self.active_panel_mut().refresh();
        }

        // F3: viewer
        if input.f3 {
            if let Some(entry) = self.active_panel().current_entry() {
                if !entry.is_dir {
                    self.viewer = FileViewer::open(&entry.path);
                }
            }
        }

        // v: toggle preview mode
        if input.v {
            if self.preview_mode {
                self.preview_mode = false;
                self.clear_all_previews();
            } else {
                self.preview_mode = true;
                self.update_preview(ctx);
            }
        }

        // Update preview on cursor move
        if self.preview_mode && (input.j || input.k || input.up || input.down
            || input.page_up || input.page_down || input.home || input.end)
        {
            self.update_preview(ctx);
        }

        // c: copy
        if input.c {
            let targets = self.active_panel().get_operation_targets();
            if !targets.is_empty() {
                let dest = self.inactive_panel().current_dir.clone();
                let sources: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
                let conflicts = file_ops::check_conflicts(&sources, &dest);

                if conflicts.is_empty() {
                    let (msg, succeeded) = batch_op(&sources, "Copied", |s| file_ops::copy_file_or_dir(s, &dest));
                    self.status_message = msg;
                    if !succeeded.is_empty() {
                        let created: Vec<PathBuf> = succeeded.iter()
                            .filter_map(|s| s.file_name().map(|n| dest.join(n)))
                            .collect();
                        self.undo_history.push(FileOperation::Copy {
                            sources: succeeded,
                            dest_dir: dest,
                            created,
                        });
                    }
                    self.active_panel_mut().deselect_all();
                    self.inactive_panel_mut().refresh();
                } else {
                    self.dialog.confirm = Some(ConfirmDialog {
                        title: "Overwrite?".to_string(),
                        message: format!(
                            "The following files already exist:\n{}\n\nOverwrite?",
                            conflicts.join(", ")
                        ),
                        action: ConfirmAction::CopyOverwrite { sources, dest },
                    });
                }
            }
        }

        // m: move
        if input.m {
            let targets = self.active_panel().get_operation_targets();
            if !targets.is_empty() {
                let dest = self.inactive_panel().current_dir.clone();
                let sources: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
                let conflicts = file_ops::check_conflicts(&sources, &dest);

                if conflicts.is_empty() {
                    let (msg, succeeded) = batch_op(&sources, "Moved", |s| file_ops::move_file_or_dir(s, &dest));
                    self.status_message = msg;
                    if !succeeded.is_empty() {
                        let moves: Vec<(PathBuf, PathBuf)> = succeeded.iter()
                            .filter_map(|s| s.file_name().map(|n| (s.clone(), dest.join(n))))
                            .collect();
                        self.undo_history.push(FileOperation::Move { moves });
                    }
                    self.active_panel_mut().refresh();
                    self.inactive_panel_mut().refresh();
                } else {
                    self.dialog.confirm = Some(ConfirmDialog {
                        title: "Overwrite?".to_string(),
                        message: format!(
                            "The following files already exist:\n{}\n\nOverwrite?",
                            conflicts.join(", ")
                        ),
                        action: ConfirmAction::MoveOverwrite { sources, dest },
                    });
                }
            }
        }

        // d: delete (with confirmation)
        if input.d {
            let targets = self.active_panel().get_operation_targets();
            if !targets.is_empty() {
                let names: Vec<String> = targets.iter().map(|t| t.name.clone()).collect();
                let paths: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
                self.dialog.confirm = Some(ConfirmDialog {
                    title: "Delete".to_string(),
                    message: format!("Delete {} item(s)?\n{}", names.len(), names.join(", ")),
                    action: ConfirmAction::Delete(paths),
                });
            }
        }

        // Shift+D: permanent delete (with confirmation)
        if input.shift_d {
            let targets = self.active_panel().get_operation_targets();
            if !targets.is_empty() {
                let names: Vec<String> = targets.iter().map(|t| t.name.clone()).collect();
                let paths: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
                self.dialog.confirm = Some(ConfirmDialog {
                    title: "⚠ Permanent Delete".to_string(),
                    message: format!(
                        "PERMANENTLY delete {} item(s)?\n{}\n\nThis cannot be undone!",
                        names.len(),
                        names.join(", ")
                    ),
                    action: ConfirmAction::DeletePermanent(paths),
                });
            }
        }

        // r: rename
        if input.r {
            if let Some(entry) = self.active_panel().current_entry() {
                if entry.name != ".." {
                    self.dialog.input = Some(InputDialog {
                        title: "Rename".to_string(),
                        value: entry.name.clone(),
                        action: InputAction::Rename(entry.path.clone()),
                    });
                }
            }
        }

        // n: new directory
        if input.n {
            self.dialog.input = Some(InputDialog {
                title: "New Directory".to_string(),
                value: String::new(),
                action: InputAction::NewDirectory,
            });
        }

        // o: sync opposite panel to current directory
        if input.o {
            let dir = self.active_panel().current_dir.clone();
            self.inactive_panel_mut().navigate_to(dir);
            self.status_message = "Synced opposite panel".to_string();
            self.save_config();
        }

        // ?: show help
        if input.question {
            self.dialog.message = Some(MessageDialog {
                title: "Keyboard Shortcuts".to_string(),
                message: "\
j / k / ↑ / ↓  :  Cursor move
l              :  Open dir / Execute file
h              :  Parent directory
i              :  Switch panel
Space          :  Toggle select
Ctrl+A         :  Select all
f              :  Focus filter
o              :  Sync opposite panel
c              :  Copy selected → opposite
m              :  Move selected → opposite
d              :  Delete selected (trash)
Shift+D        :  Permanent delete (no undo)
r              :  Rename
n              :  New directory
p              :  Drive select
g              :  Registered directories
Shift+G        :  Register current directory
Shift+U        :  Zip compress selected
u              :  Zip extract at cursor
z              :  Undo last operation
Shift+Z        :  Redo
v              :  Image/Audio preview
F3             :  Text viewer
Ctrl+R         :  Refresh
Ctrl+.         :  Toggle hidden files
Ctrl+Q         :  Quit
Home / End     :  Jump to top / bottom
PgUp / PgDn    :  Page scroll
?              :  This help"
                    .to_string(),
            });
        }

        // f: focus filter
        if input.f {
            self.active_panel_mut().focus_filter = true;
        }

        // p: drive selection
        if input.p {
            self.dialog.drive = Some(DriveDialog {
                drives: self.drives.clone(),
            });
        }

        // g: registered directories
        if input.g {
            self.dialog.registered_dir = Some(RegisteredDirDialog {
                dirs: self.config.registered_dirs.clone(),
                cursor: 0,
            });
        }

        // Shift+G: register current directory
        if input.shift_g {
            let dir = self.active_panel().current_dir.clone();
            let default_name = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| dir.to_string_lossy().to_string());
            self.dialog.input = Some(InputDialog {
                title: "Register Directory".to_string(),
                value: default_name,
                action: InputAction::RegisterDirectory(dir),
            });
        }

        // Shift+Z: zip compress selected files
        if input.shift_u {
            let targets = self.active_panel().get_operation_targets();
            if !targets.is_empty() {
                let sources: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
                let default_name = targets
                    .first()
                    .map(|t| {
                        // Strip extension for default zip name
                        PathBuf::from(&t.name)
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| t.name.clone())
                    })
                    .unwrap_or_else(|| "archive".to_string());
                self.dialog.input = Some(InputDialog {
                    title: "Zip Compress".to_string(),
                    value: default_name,
                    action: InputAction::ZipCompress(sources),
                });
            }
        }

        // Z: decompress zip at cursor
        if input.u {
            if let Some(entry) = self.active_panel().current_entry() {
                if !entry.is_dir {
                    let is_zip = entry.path.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_lowercase() == "zip")
                        .unwrap_or(false);
                    if is_zip {
                        let zip_path = entry.path.clone();
                        let dest = self.inactive_panel().current_dir.clone();
                        match file_ops::decompress_zip(&zip_path, &dest) {
                            Ok(extract_dir) => {
                                self.status_message = format!("Extracted to: {}", extract_dir.display());
                                self.undo_history.push(FileOperation::Decompress {
                                    zip_path,
                                    extracted_dir: extract_dir,
                                });
                                self.inactive_panel_mut().refresh();
                            }
                            Err(e) => {
                                self.status_message = format!("Error: {}", e);
                            }
                        }
                    }
                }
            }
        }

        // z: undo
        if input.z {
            match self.undo_history.undo() {
                Ok(msg) => {
                    self.status_message = msg;
                    self.left_panel.refresh();
                    self.right_panel.refresh();
                }
                Err(msg) => {
                    self.status_message = msg;
                }
            }
        }

        // Shift+z: redo
        if input.shift_z {
            match self.undo_history.redo() {
                Ok(msg) => {
                    self.status_message = msg;
                    self.left_panel.refresh();
                    self.right_panel.refresh();
                }
                Err(msg) => {
                    self.status_message = msg;
                }
            }
        }

        // :: command mode
        if input.colon {
            self.command_mode = true;
            self.command_line.clear();
        }
    }

    fn handle_file_drop(&mut self, ctx: &egui::Context) {
        // Determine which panel the pointer is over (left half vs right half)
        let screen_rect = ctx.screen_rect();
        let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
        let is_left_half = pointer_pos
            .map(|p| p.x < screen_rect.center().x)
            .unwrap_or(true);

        // Hover highlight
        let hovered_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
        self.left_panel.drop_highlight = hovered_files && is_left_half;
        self.right_panel.drop_highlight = hovered_files && !is_left_half;

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

        let dest_panel = if is_left_half {
            &mut self.left_panel
        } else {
            &mut self.right_panel
        };
        let dest = dest_panel.current_dir.clone();

        let conflicts = file_ops::check_conflicts(&dropped_files, &dest);
        if conflicts.is_empty() {
            let (msg, succeeded) = batch_op(&dropped_files, "Dropped", |s| file_ops::copy_file_or_dir(s, &dest));
            self.status_message = msg;
            if !succeeded.is_empty() {
                let created: Vec<PathBuf> = succeeded.iter()
                    .filter_map(|s| s.file_name().map(|n| dest.join(n)))
                    .collect();
                self.undo_history.push(FileOperation::Copy {
                    sources: succeeded,
                    dest_dir: dest,
                    created,
                });
            }
            dest_panel.refresh();
        } else {
            self.dialog.confirm = Some(ConfirmDialog {
                title: "Overwrite?".to_string(),
                message: format!(
                    "The following files already exist:\n{}\n\nOverwrite?",
                    conflicts.join(", ")
                ),
                action: ConfirmAction::CopyOverwrite {
                    sources: dropped_files,
                    dest,
                },
            });
        }
    }

    fn handle_dialog_result(&mut self, result: DialogResult) {
        match result {
            DialogResult::ConfirmYes(action) => match action {
                ConfirmAction::Delete(paths) => {
                    let (msg, succeeded) = batch_op(&paths, "Deleted", file_ops::delete_to_trash);
                    self.status_message = msg;
                    if !succeeded.is_empty() {
                        self.undo_history.push(FileOperation::Delete {
                            paths: succeeded,
                        });
                    }
                    self.active_panel_mut().refresh();
                }
                ConfirmAction::DeletePermanent(paths) => {
                    let (msg, _) = batch_op(&paths, "Permanently deleted", file_ops::delete_permanently);
                    self.status_message = msg;
                    self.active_panel_mut().refresh();
                }
                ConfirmAction::CopyOverwrite { sources, dest } => {
                    let (msg, succeeded) = batch_op(&sources, "Copied", |s| file_ops::copy_file_or_dir_overwrite(s, &dest));
                    self.status_message = msg;
                    if !succeeded.is_empty() {
                        let created: Vec<PathBuf> = succeeded.iter()
                            .filter_map(|s| s.file_name().map(|n| dest.join(n)))
                            .collect();
                        self.undo_history.push(FileOperation::Copy {
                            sources: succeeded,
                            dest_dir: dest,
                            created,
                        });
                    }
                    self.active_panel_mut().deselect_all();
                    self.inactive_panel_mut().refresh();
                }
                ConfirmAction::MoveOverwrite { sources, dest } => {
                    let (msg, succeeded) = batch_op(&sources, "Moved", |s| file_ops::move_file_or_dir_overwrite(s, &dest));
                    self.status_message = msg;
                    if !succeeded.is_empty() {
                        let moves: Vec<(PathBuf, PathBuf)> = succeeded.iter()
                            .filter_map(|s| s.file_name().map(|n| (s.clone(), dest.join(n))))
                            .collect();
                        self.undo_history.push(FileOperation::Move { moves });
                    }
                    self.active_panel_mut().refresh();
                    self.inactive_panel_mut().refresh();
                }
            },
            DialogResult::InputOk(value, action) => {
                if value.is_empty() {
                    return;
                }
                match action {
                    InputAction::Rename(old_path) => {
                        match file_ops::rename_file(&old_path, &value) {
                            Ok(new_path) => {
                                self.status_message = format!("Renamed to {}", value);
                                self.undo_history.push(FileOperation::Rename {
                                    old_path,
                                    new_path,
                                });
                                self.active_panel_mut().refresh();
                            }
                            Err(e) => {
                                self.status_message = format!("Rename error: {}", e);
                            }
                        }
                    }
                    InputAction::NewDirectory => {
                        let dir = self.active_panel().current_dir.clone();
                        match file_ops::create_directory(&dir, &value) {
                            Ok(path) => {
                                self.status_message = format!("Created directory: {}", value);
                                self.undo_history.push(FileOperation::CreateDir { path });
                                self.active_panel_mut().refresh();
                            }
                            Err(e) => {
                                self.status_message = format!("Error: {}", e);
                            }
                        }
                    }
                    InputAction::RegisterDirectory(path) => {
                        // Step 2: ask for shortcut key (default: first char of name)
                        let default_key = first_char_upper(&value, 'A');
                        self.dialog.input = Some(InputDialog {
                            title: format!("Shortcut Key for \"{}\"", value),
                            value: default_key,
                            action: InputAction::RegisterDirectoryKey {
                                path,
                                name: value,
                            },
                        });
                    }
                    InputAction::RegisterDirectoryKey { path, name } => {
                        let key = first_char_upper(&value, '?');
                        let path_str = path.to_string_lossy().to_string();
                        self.status_message = format!("Registered: [{}] {}", key, name);
                        self.config.registered_dirs.push(
                            crate::config::RegisteredDir {
                                key,
                                name,
                                path: path_str,
                            },
                        );
                        self.config.save();
                    }
                    InputAction::EditRegisteredDirKey(idx) => {
                        let new_key = first_char_upper(&value, '?');
                        if idx < self.config.registered_dirs.len() {
                            let name = self.config.registered_dirs[idx].name.clone();
                            self.config.registered_dirs[idx].key = new_key.clone();
                            self.config.save();
                            self.status_message =
                                format!("Changed key for \"{}\": [{}]", name, new_key);
                        }
                    }
                    InputAction::ZipCompress(sources) => {
                        let dest = self.inactive_panel().current_dir.clone();
                        match file_ops::compress_to_zip(&sources, &dest, &value) {
                            Ok(zip_path) => {
                                let name = zip_path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                self.status_message =
                                    format!("Compressed {} file(s) to {}", sources.len(), name);
                                self.undo_history.push(FileOperation::Compress {
                                    sources,
                                    zip_path,
                                });
                                self.active_panel_mut().refresh();
                                self.inactive_panel_mut().refresh();
                            }
                            Err(e) => {
                                self.status_message = format!("Zip error: {}", e);
                            }
                        }
                    }
                }
            }
            DialogResult::DriveSelected(drive) => {
                let path = self.resolve_drive_path(&drive);
                if path.exists() {
                    self.active_panel_mut().navigate_to(path);
                    self.save_config();
                }
            }
            DialogResult::RegisteredDirSelected(path_str) => {
                let path = PathBuf::from(&path_str);
                if path.exists() {
                    self.active_panel_mut().navigate_to(path);
                    self.save_config();
                    self.status_message = format!("Jumped to {}", path_str);
                } else {
                    self.status_message = format!("Directory not found: {}", path_str);
                }
            }
            DialogResult::RegisteredDirDeleted(idx) => {
                if idx < self.config.registered_dirs.len() {
                    let removed = self.config.registered_dirs.remove(idx);
                    self.config.save();
                    self.status_message = format!("Unregistered: {}", removed.name);
                }
            }
            DialogResult::RegisteredDirEditKey(idx) => {
                if idx < self.config.registered_dirs.len() {
                    let current_key = self.config.registered_dirs[idx].key.clone();
                    self.dialog.input = Some(InputDialog {
                        title: format!(
                            "Change Key for \"{}\"",
                            self.config.registered_dirs[idx].name
                        ),
                        value: current_key,
                        action: InputAction::EditRegisteredDirKey(idx),
                    });
                }
            }
            _ => {}
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

        // Apply dark mode
        ctx.set_visuals(egui::Visuals::dark());

        // Handle keyboard input
        self.handle_keyboard(ctx);

        // Poll background image loading
        if self.preview_mode {
            if let Some(preview) = self.image_cache.poll_loaded(ctx) {
                self.image_preview = Some(preview);
            }
        }

        // Handle dialog results
        let result = show_dialogs(ctx, &mut self.dialog);
        self.handle_dialog_result(result);

        // Show viewer if open
        if let Some(viewer) = &mut self.viewer {
            viewer.ui(ctx);
            if !viewer.open {
                self.viewer = None;
            }
        }


        // Top panel: drive buttons
        egui::TopBottomPanel::top("drives").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for drive in &self.drives.clone() {
                    if ui.button(drive).clicked() {
                        let path = self.resolve_drive_path(drive);
                        if path.exists() {
                            self.active_panel_mut().navigate_to(path);
                            self.save_config();
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label("f2filer");
                });
            });
        });

        // Bottom panel: status bar + command line
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
                        self.execute_command();
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

                ui.label(format!(
                    "{} items | {} selected | {}",
                    total_files,
                    selected_count,
                    file_item::format_size(selected_size),
                ));

                if !self.status_message.is_empty() {
                    ui.separator();
                    ui.label(&self.status_message);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label("[h]Up [l]Open [c]Copy [m]Move [d]Del [r]Ren [n]NewDir [o]Sync [i]Switch");
                });
            });
        });

        // Handle external file drops
        self.handle_file_drop(ctx);

        // Central panel: two file panels side by side
        egui::CentralPanel::default().show(ctx, |ui| {
            let active = self.active;
            let left_panel = &mut self.left_panel;
            let right_panel = &mut self.right_panel;
            let image_preview = &self.image_preview;
            let audio_preview = &mut self.audio_preview;
            let video_preview = &mut self.video_preview;
            let left_is_inactive = active == ActivePanel::Right;
            let right_is_inactive = active == ActivePanel::Left;
            let has_preview = image_preview.is_some() || audio_preview.is_some() || video_preview.is_some();

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

            // Handle outbound drag (App → External)
            #[cfg(windows)]
            {
                let drag_paths = left_panel
                    .drag_request
                    .take()
                    .or_else(|| right_panel.drag_request.take());
                if let Some(paths) = drag_paths {
                    let was_move = crate::drag_drop::start_drag(&paths);
                    if was_move {
                        left_panel.refresh();
                        right_panel.refresh();
                    }
                }
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_config();
    }
}

impl F2App {
    fn execute_command(&mut self) {
        let cmd = self.command_line.trim().to_string();
        match cmd.as_str() {
            "q" | "quit" => {
                self.save_config();
            }
            "refresh" | "r" => {
                self.active_panel_mut().refresh();
                self.status_message = "Refreshed".to_string();
            }
            "hidden" => {
                let show = !self.active_panel().show_hidden;
                self.active_panel_mut().show_hidden = show;
                self.active_panel_mut().refresh();
                self.status_message = format!(
                    "Hidden files: {}",
                    if show { "shown" } else { "hidden" }
                );
            }
            _ => {
                self.status_message = format!("Unknown command: {}", cmd);
            }
        }
        self.command_line.clear();
    }
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

/// Extract drive letter (e.g. "C:") from a path like "C:\Users\foo".
fn drive_letter(path: &std::path::Path) -> Option<String> {
    let s = path.to_string_lossy();
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        Some(s[..2].to_uppercase())
    } else {
        None
    }
}

fn restore_dir(saved: &Option<String>) -> Option<PathBuf> {
    saved.as_ref().and_then(|p| {
        let path = PathBuf::from(p);
        if path.exists() { Some(path) } else { None }
    })
}

fn first_char_upper(s: &str, fallback: char) -> String {
    s.chars()
        .next()
        .unwrap_or(fallback)
        .to_uppercase()
        .next()
        .unwrap_or(fallback)
        .to_string()
}

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

#[cfg(test)]
mod tests {
    use super::*;

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

struct KeyState {
    tab: bool,
    j: bool,
    k: bool,
    h: bool,
    l: bool,
    up: bool,
    down: bool,
    space: bool,
    insert: bool,
    home: bool,
    end: bool,
    page_up: bool,
    page_down: bool,
    f3: bool,
    c: bool,
    m: bool,
    d: bool,
    r: bool,
    n: bool,
    o: bool,
    ctrl_a: bool,
    ctrl_r: bool,
    ctrl_q: bool,
    period: bool,
    colon: bool,
    question: bool,
    p: bool,
    f: bool,
    v: bool,
    enter: bool,
    g: bool,
    shift_g: bool,
    u: bool,
    shift_u: bool,
    shift_d: bool,
    z: bool,
    shift_z: bool,
}

fn setup_fonts(ctx: &egui::Context) {
    let font_path = std::path::Path::new(
        r"C:\Users\ancient\AppData\Local\Microsoft\Windows\Fonts\HackGenConsoleNF-Regular.ttf",
    );

    if let Ok(font_data) = std::fs::read(font_path) {
        let mut fonts = egui::FontDefinitions::default();

        fonts.font_data.insert(
            "HackGen".to_string(),
            egui::FontData::from_owned(font_data).into(),
        );

        // Set HackGen as the primary font for both proportional and monospace
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "HackGen".to_string());

        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "HackGen".to_string());

        ctx.set_fonts(fonts);
    }
}
