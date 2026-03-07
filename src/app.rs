use std::path::PathBuf;

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
    skip_next_drop: bool,
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
            skip_next_drop: false,
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
        if let Some(distro) = drive.strip_prefix("WSL:") {
            // WSL drives: try \\wsl$ first (more compatible), then \\wsl.localhost
            for base in &[r"\\wsl$", r"\\wsl.localhost"] {
                let path = PathBuf::from(format!(r"{}\{}", base, distro));
                if path.exists() {
                    return path;
                }
            }
            return PathBuf::from(format!(r"\\wsl$\{}", distro));
        }
        if drive.starts_with(r"\\") {
            // Generic UNC path: drive is already "\\server\share"
            return PathBuf::from(format!(r"{}\", drive));
        }
        PathBuf::from(format!("{}\\", drive))
    }

    fn start_background_op(&mut self, ctx: &egui::Context, op_kind: OpKind) {
        let total = match &op_kind {
            OpKind::Copy { sources, .. } => sources.len(),
            OpKind::Move { sources, .. } => sources.len(),
            OpKind::Delete { paths } => paths.len(),
            OpKind::DeletePermanent { paths } => paths.len(),
            OpKind::ZipCompress { sources, .. } => sources.len(),
            OpKind::ZipDecompress { .. } => 1,
        };

        let label = match &op_kind {
            OpKind::Copy { .. } => "Copying",
            OpKind::Move { .. } => "Moving",
            OpKind::Delete { .. } => "Deleting",
            OpKind::DeletePermanent { .. } => "Permanently Deleting",
            OpKind::ZipCompress { .. } => "Compressing",
            OpKind::ZipDecompress { .. } => "Decompressing",
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
            }
            repaint_ctx.request_repaint();
        });

        self.dialog.progress = Some(ProgressDialog {
            handle: progress,
            op_kind,
        });
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
                a: i.key_pressed(egui::Key::A) && !i.modifiers.ctrl,
                ctrl_r: i.key_pressed(egui::Key::R) && i.modifiers.ctrl,
                q: i.key_pressed(egui::Key::Q) && !i.modifiers.ctrl,
                ctrl_q: i.key_pressed(egui::Key::Q) && i.modifiers.ctrl,
                period: i.key_pressed(egui::Key::Period) && i.modifiers.ctrl,
                colon: i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == ":")),
                question: i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "?")),
                p: i.key_pressed(egui::Key::P),
                f: i.key_pressed(egui::Key::F),
                v: i.key_pressed(egui::Key::V) && !i.modifiers.ctrl,
                enter: i.key_pressed(egui::Key::Enter) && !i.modifiers.alt,
                g: i.key_pressed(egui::Key::G) && !i.modifiers.shift,
                shift_g: i.key_pressed(egui::Key::G) && i.modifiers.shift,
                u: i.key_pressed(egui::Key::U) && !i.modifiers.shift && !i.modifiers.ctrl,
                shift_u: i.key_pressed(egui::Key::U) && i.modifiers.shift,
                z: i.key_pressed(egui::Key::Z) && !i.modifiers.shift && !i.modifiers.ctrl,
                shift_z: i.key_pressed(egui::Key::Z) && i.modifiers.shift,
                y: i.key_pressed(egui::Key::Y) && !i.modifiers.shift && !i.modifiers.ctrl,
                shift_x: i.key_pressed(egui::Key::X) && i.modifiers.shift,
                alt_enter: i.key_pressed(egui::Key::Enter) && i.modifiers.alt,
                backslash: i.key_pressed(egui::Key::Backslash),
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

        // a: select all
        if input.a {
            self.active_panel_mut().select_all();
        }

        // Ctrl+R: refresh
        if input.ctrl_r {
            self.active_panel_mut().refresh();
            self.status_message = "Refreshed".to_string();
        }

        // q / Ctrl+Q: quit
        if input.q || input.ctrl_q {
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
                    self.start_background_op(ctx, OpKind::Copy {
                        sources,
                        dest_dir: dest,
                        overwrite: false,
                    });
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
                    self.start_background_op(ctx, OpKind::Move {
                        sources,
                        dest_dir: dest,
                        overwrite: false,
                    });
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
                let is_unc = paths.iter().any(|p| p.to_string_lossy().starts_with(r"\\"));
                let message = if is_unc {
                    format!(
                        "PERMANENTLY delete {} item(s)?\n{}\n\nNetwork path: recycle bin is not available.",
                        names.len(),
                        names.join(", ")
                    )
                } else {
                    format!("Delete {} item(s)?\n{}", names.len(), names.join(", "))
                };
                self.dialog.confirm = Some(ConfirmDialog {
                    title: if is_unc { "Delete (permanent)".to_string() } else { "Delete".to_string() },
                    message,
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

        // Shift+X: open recycle bin
        if input.shift_x {
            let _ = std::process::Command::new("explorer.exe")
                .arg("shell:RecycleBinFolder")
                .spawn();
        }

        // Alt+Enter: file properties
        if input.alt_enter {
            if let Some(entry) = self.active_panel().current_entry() {
                if entry.name != ".." {
                    show_file_properties(&entry.path);
                }
            }
        }

        // \: context menu
        if input.backslash {
            if let Some(entry) = self.active_panel().current_entry().cloned() {
                if entry.name != ".." {
                    show_context_menu(&entry.path);
                    self.active_panel_mut().refresh();
                }
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

        // y: copy current file path to clipboard
        if input.y {
            if let Some(entry) = self.active_panel().current_entry() {
                let path_str = entry.path.to_string_lossy().to_string();
                match arboard::Clipboard::new() {
                    Ok(mut clip) => {
                        if clip.set_text(&path_str).is_ok() {
                            self.status_message = format!("Copied: {}", path_str);
                        } else {
                            self.status_message = "Failed to copy to clipboard".to_string();
                        }
                    }
                    Err(_) => {
                        self.status_message = "Failed to access clipboard".to_string();
                    }
                }
            }
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
Shift+X        :  Open recycle bin
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
                        self.start_background_op(ctx, OpKind::ZipDecompress {
                            zip_path,
                            dest_dir: dest,
                        });
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

        if self.skip_next_drop {
            self.skip_next_drop = false;
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
                    conflicts.join(", ")
                ),
                action: ConfirmAction::CopyOverwrite {
                    sources: dropped_files,
                    dest,
                },
            });
        }
    }

    fn handle_dialog_result(&mut self, ctx: &egui::Context, result: DialogResult) {
        match result {
            DialogResult::ConfirmYes(action) => match action {
                ConfirmAction::Delete(paths) => {
                    self.start_background_op(ctx, OpKind::Delete { paths });
                }
                ConfirmAction::DeletePermanent(paths) => {
                    self.start_background_op(ctx, OpKind::DeletePermanent { paths });
                }
                ConfirmAction::CopyOverwrite { sources, dest } => {
                    self.start_background_op(ctx, OpKind::Copy {
                        sources,
                        dest_dir: dest,
                        overwrite: true,
                    });
                }
                ConfirmAction::MoveOverwrite { sources, dest } => {
                    self.start_background_op(ctx, OpKind::Move {
                        sources,
                        dest_dir: dest,
                        overwrite: true,
                    });
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
                        self.start_background_op(ctx, OpKind::ZipCompress {
                            sources,
                            dest_dir: dest,
                            zip_name: value,
                        });
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
            DialogResult::ProgressFinished => {
                if let Some(progress_dialog) = self.dialog.progress.take() {
                    let state = progress_dialog.handle.state.lock().ok();
                    let (result_message, succeeded_paths, result_path) = match &state {
                        Some(s) => (
                            s.result_message.clone(),
                            s.succeeded_paths.clone(),
                            s.result_path.clone(),
                        ),
                        None => ("Operation failed (mutex poisoned)".to_string(), Vec::new(), None),
                    };
                    drop(state);

                    self.status_message = result_message;

                    if !succeeded_paths.is_empty() {
                        match &progress_dialog.op_kind {
                            OpKind::Copy { dest_dir, .. } => {
                                let created: Vec<PathBuf> = succeeded_paths.iter()
                                    .filter_map(|s| s.file_name().map(|n| dest_dir.join(n)))
                                    .collect();
                                self.undo_history.push(FileOperation::Copy {
                                    sources: succeeded_paths,
                                    dest_dir: dest_dir.clone(),
                                    created,
                                });
                            }
                            OpKind::Move { dest_dir, .. } => {
                                let moves: Vec<(PathBuf, PathBuf)> = succeeded_paths.iter()
                                    .filter_map(|s| s.file_name().map(|n| (s.clone(), dest_dir.join(n))))
                                    .collect();
                                self.undo_history.push(FileOperation::Move { moves });
                            }
                            OpKind::Delete { .. } => {
                                self.undo_history.push(FileOperation::Delete {
                                    paths: succeeded_paths,
                                });
                            }
                            OpKind::DeletePermanent { .. } => {
                                // No undo for permanent delete
                            }
                            OpKind::ZipCompress { .. } => {
                                if let Some(zip_path) = result_path {
                                    self.undo_history.push(FileOperation::Compress {
                                        sources: succeeded_paths,
                                        zip_path,
                                    });
                                }
                            }
                            OpKind::ZipDecompress { zip_path, .. } => {
                                if let Some(extracted_dir) = result_path {
                                    self.undo_history.push(FileOperation::Decompress {
                                        zip_path: zip_path.clone(),
                                        extracted_dir,
                                    });
                                }
                            }
                        }
                    }

                    self.left_panel.refresh();
                    self.right_panel.refresh();
                    self.active_panel_mut().deselect_all();
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
        self.handle_dialog_result(ctx, result);

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

            // Click on inactive panel → switch active panel
            if left_panel.clicked {
                left_panel.clicked = false;
                self.active = ActivePanel::Left;
            }
            if right_panel.clicked {
                right_panel.clicked = false;
                self.active = ActivePanel::Right;
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
            _ if cmd.starts_with("cd ") => {
                let target = cmd[3..].trim();
                let path = PathBuf::from(target);
                if path.is_dir() {
                    self.active_panel_mut().navigate_to(path);
                    self.save_config();
                } else {
                    self.status_message = format!("Directory not found: {}", target);
                }
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

fn first_char_upper(s: &str, fallback: char) -> String {
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

#[cfg(windows)]
fn show_file_properties(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_INVOKEIDLIST};

    let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "properties\0".encode_utf16().collect();

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_INVOKEIDLIST,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(path_wide.as_ptr()),
        ..Default::default()
    };

    unsafe {
        let _ = ShellExecuteExW(&mut sei);
    }
}

#[cfg(windows)]
fn show_context_menu(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::*;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::Common::*;
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let path_wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        if SHParseDisplayName(PCWSTR(path_wide.as_ptr()), None, &mut pidl, 0, None).is_err() {
            return;
        }

        let mut child_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        let folder: IShellFolder =
            match SHBindToParent(pidl, Some(&mut child_pidl as *mut *mut _ as *mut *mut _)) {
                Ok(f) => f,
                Err(_) => {
                    CoTaskMemFree(Some(pidl as _));
                    return;
                }
            };

        let child_pidl = child_pidl as *const ITEMIDLIST;
        let ctx_menu: IContextMenu = match folder.GetUIObjectOf(
            windows::Win32::Foundation::HWND::default(),
            &[child_pidl],
            None,
        ) {
            Ok(m) => m,
            Err(_) => {
                CoTaskMemFree(Some(pidl as _));
                return;
            }
        };

        let hmenu = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => {
                CoTaskMemFree(Some(pidl as _));
                return;
            }
        };

        let first_cmd: u32 = 1;
        if ctx_menu
            .QueryContextMenu(hmenu, 0, first_cmd, 0x7FFF, CMF_NORMAL)
            .is_err()
        {
            let _ = DestroyMenu(hmenu);
            CoTaskMemFree(Some(pidl as _));
            return;
        }

        let hwnd = GetForegroundWindow();
        let mut pt = windows::Win32::Foundation::POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);

        let cmd = TrackPopupMenuEx(
            hmenu,
            TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0,
            pt.x,
            pt.y,
            hwnd,
            None,
        );

        if cmd.0 != 0 {
            let verb = (cmd.0 as u32).wrapping_sub(first_cmd) as usize;
            let info = CMINVOKECOMMANDINFO {
                cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                hwnd,
                lpVerb: windows::core::PCSTR(verb as *const u8),
                nShow: 1,
                ..Default::default()
            };
            let _ = ctx_menu.InvokeCommand(&info);
        }

        let _ = DestroyMenu(hmenu);
        CoTaskMemFree(Some(pidl as _));
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
    a: bool,
    ctrl_r: bool,
    q: bool,
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
    y: bool,
    shift_x: bool,
    alt_enter: bool,
    backslash: bool,
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
