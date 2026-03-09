use eframe::egui;

use crate::config::RegisteredDir;
use crate::file_ops;

#[derive(Default)]
pub struct DialogState {
    pub confirm: Option<ConfirmDialog>,
    pub input: Option<InputDialog>,
    pub message: Option<MessageDialog>,
    pub drive: Option<DriveDialog>,
    pub registered_dir: Option<RegisteredDirDialog>,
    pub progress: Option<ProgressDialog>,
    pub settings: Option<SettingsDialog>,
}

impl DialogState {
    pub fn is_open(&self) -> bool {
        self.confirm.is_some()
            || self.input.is_some()
            || self.message.is_some()
            || self.drive.is_some()
            || self.registered_dir.is_some()
            || self.progress.is_some()
            || self.settings.is_some()
    }
}

#[derive(Clone)]
pub enum OpKind {
    Copy { sources: Vec<std::path::PathBuf>, dest_dir: std::path::PathBuf, overwrite: bool },
    Move { sources: Vec<std::path::PathBuf>, dest_dir: std::path::PathBuf, overwrite: bool },
    Delete { paths: Vec<std::path::PathBuf> },
    DeletePermanent { paths: Vec<std::path::PathBuf> },
    ZipCompress { sources: Vec<std::path::PathBuf>, dest_dir: std::path::PathBuf, zip_name: String },
    ZipDecompress { zip_path: std::path::PathBuf, dest_dir: std::path::PathBuf },
    TarDecompress { tar_path: std::path::PathBuf, dest_dir: std::path::PathBuf },
}

pub struct ProgressDialog {
    pub handle: file_ops::ProgressHandle,
    pub op_kind: OpKind,
}

pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Clone)]
pub enum ConfirmAction {
    Delete(Vec<std::path::PathBuf>),
    DeletePermanent(Vec<std::path::PathBuf>),
    CopyOverwrite {
        sources: Vec<std::path::PathBuf>,
        dest: std::path::PathBuf,
    },
    MoveOverwrite {
        sources: Vec<std::path::PathBuf>,
        dest: std::path::PathBuf,
    },
}

pub struct InputDialog {
    pub title: String,
    pub value: String,
    pub action: InputAction,
    /// If set, select characters 0..N on the first frame.
    pub select_end: Option<usize>,
}

#[derive(Clone)]
pub enum InputAction {
    Rename(std::path::PathBuf),
    NewDirectory,
    RegisterDirectory(std::path::PathBuf), // path to register
    RegisterDirectoryKey {
        path: std::path::PathBuf,
        name: String,
    },
    EditRegisteredDirKey(usize),
    ZipCompress(Vec<std::path::PathBuf>),
}

pub struct MessageDialog {
    pub title: String,
    pub message: String,
}

pub struct DriveDialog {
    /// (drive_name, space_label) e.g. ("C:", "120.5G / 500.0G")
    pub drives: Vec<(String, String)>,
    pub cursor: usize,
}

pub struct RegisteredDirDialog {
    pub dirs: Vec<RegisteredDir>,
    pub cursor: usize,
}

pub struct SettingsDialog {
    pub fonts: Vec<(String, String)>, // (display_name, full_path)
    pub cursor: usize,
    pub filter: String,
    pub filter_has_focus: bool,
    pub current_font: String, // display name of current font
}

pub enum DialogResult {
    None,
    ConfirmYes(ConfirmAction),
    InputOk(String, InputAction),
    DriveSelected(String),
    RegisteredDirSelected(String),
    RegisteredDirDeleted(usize),
    RegisteredDirEditKey(usize),
    FontSelected(Option<String>), // None = default, Some(path) = custom font
    ProgressFinished,
    Closed,
}

pub fn show_dialogs(ctx: &egui::Context, state: &mut DialogState) -> DialogResult {
    let mut result = DialogResult::None;

    // Confirm dialog
    if let Some(dialog) = &state.confirm {
        let title = dialog.title.clone();
        let message = dialog.message.clone();
        let mut open = true;

        let screen = ctx.screen_rect();
        egui::Window::new(&title)
            .collapsible(false)
            .resizable(true)
            .constrain(true)
            .vscroll(true)
            .default_pos(screen.center())
            .pivot(egui::Align2::CENTER_CENTER)
            .default_width(300.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(&message);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Yes (y)").clicked() {
                        result = DialogResult::ConfirmYes(
                            state.confirm.as_ref().unwrap().action.clone(),
                        );
                    }
                    if ui.button("No (n)").clicked() {
                        result = DialogResult::Closed;
                    }
                });
            });

        // Handle keyboard shortcuts for confirm dialog
        if ctx.input(|i| i.key_pressed(egui::Key::Y) || i.key_pressed(egui::Key::Space)) {
            result = DialogResult::ConfirmYes(state.confirm.as_ref().unwrap().action.clone());
        }
        if ctx.input(|i| i.key_pressed(egui::Key::N) || i.key_pressed(egui::Key::Escape)) {
            result = DialogResult::Closed;
        }

        if !open {
            result = DialogResult::Closed;
        }
    }

    // Input dialog
    if let Some(dialog) = &mut state.input {
        let title = dialog.title.clone();
        let mut open = true;

        egui::Window::new(&title)

            .collapsible(false)
            .resizable(true)
            .constrain(true)
            .default_pos(ctx.screen_rect().center())
            .pivot(egui::Align2::CENTER_CENTER)
            .open(&mut open)
            .show(ctx, |ui| {
                let output = egui::TextEdit::singleline(&mut dialog.value)
                    .desired_width(300.0)
                    .show(ui);
                let response = &output.response;

                // Auto-focus the text input
                if !response.has_focus() {
                    response.request_focus();
                }

                // Apply initial text selection (e.g., filename stem for rename)
                if let Some(end) = dialog.select_end.take() {
                    let mut state = output.state;
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(end),
                    )));
                    state.store(ui.ctx(), response.id);
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        let value = dialog.value.clone();
                        let action = dialog.action.clone();
                        result = DialogResult::InputOk(value, action);
                    }
                    if ui.button("Cancel").clicked() {
                        result = DialogResult::Closed;
                    }
                });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            result = DialogResult::Closed;
        }

        if !open {
            result = DialogResult::Closed;
        }
    }

    // Message dialog
    if let Some(dialog) = &state.message {
        let title = dialog.title.clone();
        let message = dialog.message.clone();
        let mut open = true;

        let screen = ctx.screen_rect();
        egui::Window::new(&title)
            .collapsible(false)
            .resizable(true)
            .constrain(true)
            .vscroll(true)
            .default_pos(screen.center())
            .pivot(egui::Align2::CENTER_CENTER)
            .default_width(500.0)
            .default_height(screen.height() * 0.8)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(&message);
                ui.add_space(10.0);
                if ui.button("OK").clicked() {
                    result = DialogResult::Closed;
                }
            });

        if ctx.input(|i| {
            i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)
        }) {
            result = DialogResult::Closed;
        }

        if !open {
            result = DialogResult::Closed;
        }
    }

    // Drive dialog
    if let Some(dialog) = &mut state.drive {
        let drives = dialog.drives.clone();
        let mut open = true;

        egui::Window::new("Select Drive")

            .collapsible(false)
            .resizable(true)
            .constrain(true)
            .default_pos(ctx.screen_rect().center())
            .pivot(egui::Align2::CENTER_CENTER)
            .open(&mut open)
            .show(ctx, |ui| {
                // Assign number keys to non-drive-letter items
                let mut number_index: u32 = 1;
                for (i, (name, space)) in drives.iter().enumerate() {
                    let is_cursor = i == dialog.cursor;
                    // Non-drive-letter items get a number prefix
                    let is_drive_letter = name.len() == 2 && name.ends_with(':');
                    let display_name = if !is_drive_letter && number_index <= 9 {
                        let label = format!("[{}] {}", number_index, name);
                        number_index += 1;
                        label
                    } else {
                        name.clone()
                    };
                    ui.horizontal(|ui| {
                        let btn_text = if is_cursor {
                            egui::RichText::new(&display_name).color(egui::Color32::from_rgb(100, 180, 255)).strong()
                        } else {
                            egui::RichText::new(&display_name)
                        };
                        if ui.button(btn_text).clicked() {
                            result = DialogResult::DriveSelected(name.clone());
                        }
                        if !space.is_empty() {
                            let space_text = if is_cursor {
                                egui::RichText::new(space).color(egui::Color32::from_rgb(100, 180, 255))
                            } else {
                                egui::RichText::new(space)
                            };
                            ui.label(space_text);
                        }
                    });
                }
            });

        // j/k cursor navigation
        if !drives.is_empty() {
            if ctx.input(|i| i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown)) {
                dialog.cursor = (dialog.cursor + 1) % drives.len();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp)) {
                dialog.cursor = (dialog.cursor + drives.len() - 1) % drives.len();
            }
            // Space or Enter to select cursor
            if ctx.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)) {
                if let Some((name, _)) = drives.get(dialog.cursor) {
                    result = DialogResult::DriveSelected(name.clone());
                }
            }
        }

        // Drive letter key shortcuts (e.g. press 'c' for "C:")
        // Exclude J/K (used for cursor navigation)
        if let Some(letter) = pressed_letter_key(ctx) {
            if letter != 'J' && letter != 'K' {
                let drive_name = format!("{}:", letter);
                if drives.iter().any(|(n, _)| n == &drive_name) {
                    result = DialogResult::DriveSelected(drive_name);
                }
            }
        }

        // Number key shortcuts for non-drive-letter items (1-9)
        let number_keys = [
            egui::Key::Num1, egui::Key::Num2, egui::Key::Num3,
            egui::Key::Num4, egui::Key::Num5, egui::Key::Num6,
            egui::Key::Num7, egui::Key::Num8, egui::Key::Num9,
        ];
        for (key_idx, key) in number_keys.iter().enumerate() {
            if ctx.input(|i| i.key_pressed(*key)) {
                // Find the (key_idx+1)-th non-drive-letter item
                let mut count = 0u32;
                for (name, _) in &drives {
                    let is_drive_letter = name.len() == 2 && name.ends_with(':');
                    if !is_drive_letter {
                        count += 1;
                        if count == (key_idx as u32 + 1) {
                            result = DialogResult::DriveSelected(name.clone());
                            break;
                        }
                    }
                }
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            result = DialogResult::Closed;
        }

        if !open {
            result = DialogResult::Closed;
        }
    }

    // Registered directory dialog
    if let Some(dialog) = &mut state.registered_dir {
        let mut open = true;

        egui::Window::new("Registered Directories")
            .collapsible(false)
            .resizable(true)
            .constrain(true)
            .default_pos(ctx.screen_rect().center())
            .pivot(egui::Align2::CENTER_CENTER)
            .open(&mut open)
            .show(ctx, |ui| {
                if dialog.dirs.is_empty() {
                    ui.label("No registered directories.\nPress Shift+G to register current directory.");
                } else {
                    for (i, dir) in dialog.dirs.iter().enumerate() {
                        let is_cursor = i == dialog.cursor;
                        ui.horizontal(|ui| {
                            let label = format!("[{}] {} — {}", dir.key, dir.name, dir.path);
                            let text = if is_cursor {
                                egui::RichText::new(&label)
                                    .color(egui::Color32::from_rgb(100, 180, 255))
                                    .strong()
                            } else {
                                egui::RichText::new(&label)
                            };
                            if ui.add(egui::Label::new(text).sense(egui::Sense::click())).clicked() {
                                result = DialogResult::RegisteredDirSelected(dir.path.clone());
                            }
                            if ui.small_button("✎").clicked() {
                                result = DialogResult::RegisteredDirEditKey(i);
                            }
                            if ui.small_button("×").clicked() {
                                result = DialogResult::RegisteredDirDeleted(i);
                            }
                        });
                    }
                }
            });

        // Keyboard shortcuts
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            result = DialogResult::Closed;
        }

        // Enter to select current cursor
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(dir) = dialog.dirs.get(dialog.cursor) {
                result = DialogResult::RegisteredDirSelected(dir.path.clone());
            }
        }

        // Shortcut key matching (A-Z letters)
        if let Some(letter) = pressed_letter_key(ctx) {
            let letter_str = letter.to_string();
            if let Some(dir) = dialog.dirs.iter().find(|d| d.key == letter_str) {
                result = DialogResult::RegisteredDirSelected(dir.path.clone());
            }
        }

        if !open {
            result = DialogResult::Closed;
        }
    }

    // Progress dialog
    if let Some(progress) = &state.progress {
        let (op_label, current_file, completed, total, finished) = {
            match progress.handle.state.lock() {
                Ok(s) => (
                    s.op_label.clone(),
                    s.current_file.clone(),
                    s.completed,
                    s.total,
                    s.finished,
                ),
                Err(_) => {
                    // Mutex poisoned — treat as finished with error
                    result = DialogResult::ProgressFinished;
                    ("Error".to_string(), String::new(), 0, 0, true)
                }
            }
        };

        if finished {
            result = DialogResult::ProgressFinished;
        } else {
            egui::Window::new(&op_label)
                .collapsible(false)
                .resizable(true)
                .constrain(true)
                .default_pos(ctx.screen_rect().center())
                .pivot(egui::Align2::CENTER_CENTER)
                .show(ctx, |ui| {
                    ui.set_min_width(300.0);
                    ui.label(format!("{} / {}", completed, total));
                    if !current_file.is_empty() {
                        ui.label(&current_file);
                    }
                    let fraction = if total > 0 {
                        completed as f32 / total as f32
                    } else {
                        0.0
                    };
                    ui.add(egui::ProgressBar::new(fraction).show_percentage());
                    ui.add_space(8.0);
                    if ui.button("Cancel (Esc)").clicked() {
                        progress.handle.cancel();
                    }
                });

            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                progress.handle.cancel();
            }

            ctx.request_repaint();
        }
    }

    // Settings dialog (font selection)
    if let Some(dialog) = &mut state.settings {
        let mut open = true;

        let screen = ctx.screen_rect();
        egui::Window::new("Settings")
            .collapsible(false)
            .resizable(true)
            .constrain(true)
            .default_pos(screen.center())
            .pivot(egui::Align2::CENTER_CENTER)
            .default_width(400.0)
            .default_height(screen.height() * 0.7)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(format!("Font: {}", dialog.current_font));

                // Filter input
                let filter_response = ui.add(
                    egui::TextEdit::singleline(&mut dialog.filter)
                        .hint_text("Filter...")
                        .desired_width(380.0),
                );
                if dialog.filter_has_focus {
                    filter_response.request_focus();
                    dialog.filter_has_focus = false;
                }
                let filter_focused = filter_response.has_focus();

                ui.add_space(4.0);

                // Build filtered list: (Default) + matching fonts
                let filter_lower = dialog.filter.to_lowercase();
                let filtered: Vec<(usize, &str, &str)> = std::iter::once((usize::MAX, "(Default)", ""))
                    .chain(dialog.fonts.iter().enumerate().map(|(i, (name, path))| (i, name.as_str(), path.as_str())))
                    .filter(|(_, name, _)| filter_lower.is_empty() || name.to_lowercase().contains(&filter_lower))
                    .collect();

                // Clamp cursor
                if dialog.cursor >= filtered.len() {
                    dialog.cursor = filtered.len().saturating_sub(1);
                }

                // Dynamically compute scroll area height from available window space
                let scroll_h = (ui.available_height() - 8.0).max(100.0);
                egui::ScrollArea::vertical()
                    .max_height(scroll_h)
                    .show(ui, |ui| {
                        for (list_idx, (_, name, path)) in filtered.iter().enumerate() {
                            let is_cursor = list_idx == dialog.cursor;
                            let text = if is_cursor {
                                egui::RichText::new(*name)
                                    .color(egui::Color32::from_rgb(100, 180, 255))
                                    .strong()
                            } else {
                                egui::RichText::new(*name)
                            };
                            if ui.button(text).clicked() {
                                let font_path = if path.is_empty() { None } else { Some(path.to_string()) };
                                result = DialogResult::FontSelected(font_path);
                            }
                        }
                    });

                // Keyboard navigation (only when filter is not focused)
                if !filter_focused && !filtered.is_empty() {
                    if ctx.input(|i| i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown)) {
                        dialog.cursor = (dialog.cursor + 1) % filtered.len();
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp)) {
                        dialog.cursor = (dialog.cursor + filtered.len() - 1) % filtered.len();
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)) {
                        if let Some((_, _, path)) = filtered.get(dialog.cursor) {
                            let font_path = if path.is_empty() { None } else { Some(path.to_string()) };
                            result = DialogResult::FontSelected(font_path);
                        }
                    }
                }
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            result = DialogResult::Closed;
        }

        if !open {
            result = DialogResult::Closed;
        }
    }

    // Clean up closed dialogs
    match &result {
        DialogResult::ConfirmYes(_)
        | DialogResult::Closed
        | DialogResult::DriveSelected(_)
        | DialogResult::RegisteredDirSelected(_)
        | DialogResult::RegisteredDirEditKey(_)
        | DialogResult::FontSelected(_) => {
            state.confirm = None;
            state.input = None;
            state.message = None;
            state.drive = None;
            state.registered_dir = None;
            state.settings = None;
        }
        DialogResult::RegisteredDirDeleted(idx) => {
            // Remove from dialog's local list and adjust cursor
            if let Some(dialog) = &mut state.registered_dir {
                let idx = *idx;
                if idx < dialog.dirs.len() {
                    dialog.dirs.remove(idx);
                    if dialog.cursor >= dialog.dirs.len() && dialog.dirs.len() > 0 {
                        dialog.cursor = dialog.dirs.len() - 1;
                    }
                }
            }
        }
        DialogResult::InputOk(_, _) => {
            state.input = None;
        }
        DialogResult::ProgressFinished => {
            // Don't clear here — handle_dialog_result takes it via .take()
        }
        DialogResult::None => {}
    }

    result
}

fn pressed_letter_key(ctx: &egui::Context) -> Option<char> {
    let keys = [
        (egui::Key::A, 'A'), (egui::Key::B, 'B'), (egui::Key::C, 'C'),
        (egui::Key::D, 'D'), (egui::Key::E, 'E'), (egui::Key::F, 'F'),
        (egui::Key::G, 'G'), (egui::Key::H, 'H'), (egui::Key::I, 'I'),
        (egui::Key::J, 'J'), (egui::Key::K, 'K'), (egui::Key::L, 'L'),
        (egui::Key::M, 'M'), (egui::Key::N, 'N'), (egui::Key::O, 'O'),
        (egui::Key::P, 'P'), (egui::Key::Q, 'Q'), (egui::Key::R, 'R'),
        (egui::Key::S, 'S'), (egui::Key::T, 'T'), (egui::Key::U, 'U'),
        (egui::Key::V, 'V'), (egui::Key::W, 'W'), (egui::Key::X, 'X'),
        (egui::Key::Y, 'Y'), (egui::Key::Z, 'Z'),
    ];
    for (key, letter) in &keys {
        if ctx.input(|inp| inp.key_pressed(*key)) {
            return Some(*letter);
        }
    }
    None
}

/// Enumerate system font files (.ttf, .otf, .ttc) and return (display_name, full_path) sorted by name.
pub fn enumerate_system_fonts() -> Vec<(String, String)> {
    let mut fonts = Vec::new();

    let mut dirs: Vec<std::path::PathBuf> = Vec::new();

    #[cfg(windows)]
    {
        // System fonts
        if let Ok(windir) = std::env::var("WINDIR") {
            dirs.push(std::path::PathBuf::from(windir).join("Fonts"));
        }
        // User-installed fonts
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            dirs.push(
                std::path::PathBuf::from(localappdata)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    }

    #[cfg(not(windows))]
    {
        dirs.push(std::path::PathBuf::from("/usr/share/fonts"));
        dirs.push(std::path::PathBuf::from("/usr/local/share/fonts"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(std::path::PathBuf::from(home).join(".local/share/fonts"));
        }
    }

    for dir in &dirs {
        collect_font_files(dir, &mut fonts);
    }

    fonts.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    fonts.dedup_by(|a, b| a.1 == b.1);
    fonts
}

fn collect_font_files(dir: &std::path::Path, fonts: &mut Vec<(String, String)>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_font_files(&path, fonts);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            if ext_lower == "ttf" || ext_lower == "otf" || ext_lower == "ttc" {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !name.is_empty() {
                    fonts.push((name, path.to_string_lossy().to_string()));
                }
            }
        }
    }
}
