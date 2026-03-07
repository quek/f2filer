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
use crate::undo::UndoHistory;
use crate::viewer::TextPreview;

#[derive(PartialEq, Clone, Copy)]
pub enum ActivePanel {
    Left,
    Right,
}

pub struct F2App {
    pub(crate) left_panel: FilePanel,
    pub(crate) right_panel: FilePanel,
    pub(crate) active: ActivePanel,
    pub(crate) dialog: DialogState,
    pub(crate) text_preview: Option<TextPreview>,
    pub(crate) image_preview: Option<ImagePreview>,
    pub(crate) image_cache: ImageCache,
    pub(crate) audio_preview: Option<AudioPreview>,
    pub(crate) video_preview: Option<VideoPreview>,
    pub(crate) preview_mode: bool,
    pub(crate) command_line: String,
    pub(crate) command_mode: bool,
    pub(crate) status_message: String,
    pub(crate) drives: Vec<String>,
    pub(crate) config: Config,
    window_pos: Option<egui::Pos2>,
    window_size: Option<egui::Vec2>,
    pub(crate) undo_history: UndoHistory,
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
            text_preview: None,
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
            self.text_preview = None;
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
            self.text_preview = None;
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
            self.text_preview = None;
            if let Some(ap) = &mut self.audio_preview {
                ap.stop();
            }
            self.audio_preview = None;
            self.stop_video_preview();
            self.image_preview = self.image_cache.get_or_load(ctx, &entry.path);
        } else {
            // Text file (fallback)
            self.image_preview = None;
            self.image_cache.clear_wanted();
            if let Some(ap) = &mut self.audio_preview {
                ap.stop();
            }
            self.audio_preview = None;
            self.stop_video_preview();
            let already_loaded = self.text_preview.as_ref()
                .map(|tp| tp.title == entry.name)
                .unwrap_or(false);
            if !already_loaded {
                self.text_preview = TextPreview::load(&entry.path);
            }
        }
    }

    pub(crate) fn clear_all_previews(&mut self) {
        self.text_preview = None;
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

    pub(crate) fn save_config(&mut self) {
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
    pub(crate) fn resolve_drive_path(&self, drive: &str) -> PathBuf {
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

    pub(crate) fn start_background_op(&mut self, ctx: &egui::Context, op_kind: OpKind) {
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
        crate::keyboard::handle_keyboard(self, ctx);

        // Poll background image loading
        if self.preview_mode {
            if let Some(preview) = self.image_cache.poll_loaded(ctx) {
                self.image_preview = Some(preview);
            }
        }

        // Handle dialog results
        let result = show_dialogs(ctx, &mut self.dialog);
        crate::dialog_handler::handle_dialog_result(self, ctx, result);




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
            let text_preview = &self.text_preview;
            let image_preview = &self.image_preview;
            let audio_preview = &mut self.audio_preview;
            let video_preview = &mut self.video_preview;
            let left_is_inactive = active == ActivePanel::Right;
            let right_is_inactive = active == ActivePanel::Left;
            let has_preview = text_preview.is_some() || image_preview.is_some() || audio_preview.is_some() || video_preview.is_some();

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

    // Increase font sizes
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(12.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(16.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(16.0, egui::FontFamily::Monospace),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(16.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(22.0, egui::FontFamily::Proportional),
    );
    ctx.set_style(style);
}
