use std::sync::Arc;

use eframe::egui;
use parking_lot::Mutex;

use crate::config::RegisteredDir;
use crate::file_ops;
use crate::keybind::{Action, KeyBinding, KeyBindings, ACTION_DISPLAY_ORDER};

#[derive(Default)]
pub struct DialogState {
    pub confirm: Option<ConfirmDialog>,
    pub input: Option<InputDialog>,
    pub message: Option<MessageDialog>,
    pub drive: Option<DriveDialog>,
    pub registered_dir: Option<RegisteredDirDialog>,
    pub progress: Vec<ProgressDialog>,
    pub settings: Option<SettingsDialog>,
    pub history: Option<HistoryDialog>,
}

impl DialogState {
    pub fn is_open(&self) -> bool {
        self.confirm.is_some()
            || self.input.is_some()
            || self.message.is_some()
            || self.drive.is_some()
            || self.registered_dir.is_some()
            || self.settings.is_some()
            || self.history.is_some()
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
    StreamDecompress { path: std::path::PathBuf, dest_dir: std::path::PathBuf },
    ElevatedCopy { sources: Vec<std::path::PathBuf>, dest_dir: std::path::PathBuf, overwrite: bool },
    ElevatedMove { sources: Vec<std::path::PathBuf>, dest_dir: std::path::PathBuf, overwrite: bool },
    ElevatedDelete { paths: Vec<std::path::PathBuf> },
}

pub struct ProgressDialog {
    pub handle: file_ops::ProgressHandle,
    pub op_kind: OpKind,
    pub source_tab: usize,
    /// Number of log entries already synced to operation_log
    pub log_synced: usize,
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
    ZipCompressOverwrite {
        sources: Vec<std::path::PathBuf>,
        dest_dir: std::path::PathBuf,
        zip_name: String,
    },
    ZipDecompressOverwrite {
        zip_path: std::path::PathBuf,
        dest_dir: std::path::PathBuf,
    },
    TarDecompressOverwrite {
        tar_path: std::path::PathBuf,
        dest_dir: std::path::PathBuf,
    },
    StreamDecompressOverwrite {
        path: std::path::PathBuf,
        dest_dir: std::path::PathBuf,
    },
    ElevatedCopy {
        sources: Vec<std::path::PathBuf>,
        dest_dir: std::path::PathBuf,
        overwrite: bool,
    },
    ElevatedMove {
        sources: Vec<std::path::PathBuf>,
        dest_dir: std::path::PathBuf,
        overwrite: bool,
    },
    ElevatedDelete {
        paths: Vec<std::path::PathBuf>,
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
    NewFile,
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
    pub drives: Arc<Mutex<Vec<(String, String)>>>,
    pub cursor: usize,
}

pub struct RegisteredDirDialog {
    pub dirs: Vec<RegisteredDir>,
    pub cursor: usize,
}

pub struct HistoryDialog {
    /// Back stack entries in reverse order (most recent first) with existence flag.
    pub entries: Vec<(std::path::PathBuf, bool)>,
    pub cursor: usize,
}

#[derive(Default, PartialEq)]
pub enum SettingsSection {
    Font,
    #[default]
    Keybindings,
}

/// Pending conflict info: the user pressed a key that's already bound to another action.
pub struct KbConflict {
    pub binding: KeyBinding,
    pub target_idx: usize,                  // action index where we want to add
    pub conflicts: Vec<(usize, String)>,    // (action_idx, action_description)
}

pub struct SettingsDialog {
    pub section: SettingsSection,
    // --- Font section ---
    pub fonts: Vec<(String, String)>, // (display_name, full_path)
    pub cursor: usize,
    pub filter: String,
    pub filter_has_focus: bool,
    pub current_font: String, // display name of current font
    // --- Keybindings section ---
    pub kb_actions: Vec<(Action, String, Vec<KeyBinding>)>, // (action, description, current_bindings)
    pub kb_cursor: usize,
    pub kb_filter: String,
    pub kb_editing: bool,           // waiting for key input (capture mode)
    pub kb_customized: Vec<bool>,   // per-action: true if differs from default
    pub kb_expanded: bool,          // true if showing individual bindings for selected action
    pub kb_sub_cursor: usize,       // cursor within expanded bindings (last = "Add new")
    pub kb_conflict: Option<KbConflict>,  // pending conflict confirmation
}

impl SettingsDialog {
    pub fn new(fonts: Vec<(String, String)>, current_font: String, keybindings: &KeyBindings) -> Self {
        let kb_actions: Vec<(Action, String, Vec<KeyBinding>)> = ACTION_DISPLAY_ORDER
            .iter()
            .map(|&action| {
                let desc = action.description().to_string();
                let bindings = keybindings.bindings.get(&action).cloned().unwrap_or_default();
                (action, desc, bindings)
            })
            .collect();
        let kb_customized: Vec<bool> = ACTION_DISPLAY_ORDER
            .iter()
            .map(|&action| keybindings.is_customized(action))
            .collect();
        SettingsDialog {
            section: SettingsSection::default(),
            fonts,
            cursor: 0,
            filter: String::new(),
            filter_has_focus: true,
            current_font,
            kb_actions,
            kb_cursor: 0,
            kb_filter: String::new(),
            kb_editing: false,
            kb_customized,
            kb_expanded: false,
            kb_sub_cursor: 0,
            kb_conflict: None,
        }
    }

    /// Get display string for bindings of an action at the given index.
    fn bindings_display(bindings: &[KeyBinding]) -> String {
        bindings.iter().map(|b| b.display()).collect::<Vec<_>>().join(" / ")
    }
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
    KeybindingChanged(Action, Vec<KeyBinding>),
    KeybindingBatchChanged(Vec<(Action, Vec<KeyBinding>)>),
    KeybindingReset(Action),
    HistorySelected(usize),
    Closed,
}

pub fn show_dialogs(ctx: &egui::Context, state: &mut DialogState) -> DialogResult {
    let mut result = DialogResult::None;

    if let Some(dialog) = &state.confirm {
        result = show_confirm_dialog(ctx, dialog);
    }
    if let Some(dialog) = &mut state.input {
        result = show_input_dialog(ctx, dialog);
    }
    if let Some(dialog) = &state.message {
        result = show_message_dialog(ctx, dialog);
    }
    if let Some(dialog) = &mut state.drive {
        result = show_drive_dialog(ctx, dialog);
    }
    if let Some(dialog) = &mut state.registered_dir {
        result = show_registered_dir_dialog(ctx, dialog);
    }
    if let Some(dialog) = &mut state.history {
        result = show_history_dialog(ctx, dialog);
    }
    if let Some(dialog) = &mut state.settings {
        result = show_settings_main(ctx, dialog);
    }

    cleanup_dialog_state(state, &result);

    result
}

fn show_confirm_dialog(ctx: &egui::Context, dialog: &ConfirmDialog) -> DialogResult {
    let mut result = DialogResult::None;
    let title = dialog.title.clone();
    let message = dialog.message.clone();
    let action = dialog.action.clone();
    let mut open = true;

    let screen = ctx.content_rect();
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
                    result = DialogResult::ConfirmYes(action.clone());
                }
                if ui.button("No (n)").clicked() {
                    result = DialogResult::Closed;
                }
            });
        });

    if ctx.input(|i| i.key_pressed(egui::Key::Y) || i.key_pressed(egui::Key::Space)) {
        result = DialogResult::ConfirmYes(action);
    }
    if ctx.input(|i| i.key_pressed(egui::Key::N) || i.key_pressed(egui::Key::Escape)) {
        result = DialogResult::Closed;
    }
    if !open {
        result = DialogResult::Closed;
    }
    result
}

fn show_input_dialog(ctx: &egui::Context, dialog: &mut InputDialog) -> DialogResult {
    let mut result = DialogResult::None;
    let title = dialog.title.clone();
    let mut open = true;

    egui::Window::new(&title)
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .default_pos(ctx.content_rect().center())
        .pivot(egui::Align2::CENTER_CENTER)
        .open(&mut open)
        .show(ctx, |ui| {
            let output = egui::TextEdit::singleline(&mut dialog.value)
                .desired_width(300.0)
                .show(ui);
            let response = &output.response;

            if !response.has_focus() {
                response.request_focus();
            }

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
    result
}

fn show_message_dialog(ctx: &egui::Context, dialog: &MessageDialog) -> DialogResult {
    let mut result = DialogResult::None;
    let title = dialog.title.clone();
    let message = dialog.message.clone();
    let mut open = true;

    let screen = ctx.content_rect();
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

    if ctx.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
        result = DialogResult::Closed;
    }
    if !open {
        result = DialogResult::Closed;
    }
    result
}

fn show_drive_dialog(ctx: &egui::Context, dialog: &mut DriveDialog) -> DialogResult {
    let mut result = DialogResult::None;
    let drives = dialog.drives.lock().clone();
    let mut open = true;

    egui::Window::new("Select Drive")
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .default_pos(ctx.content_rect().center())
        .pivot(egui::Align2::CENTER_CENTER)
        .open(&mut open)
        .show(ctx, |ui| {
            let mut number_index: u32 = 1;
            for (i, (name, space)) in drives.iter().enumerate() {
                let is_cursor = i == dialog.cursor;
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

    if !drives.is_empty() {
        if ctx.input(|i| i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown)) {
            dialog.cursor = (dialog.cursor + 1) % drives.len();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp)) {
            dialog.cursor = (dialog.cursor + drives.len() - 1) % drives.len();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)) {
            if let Some((name, _)) = drives.get(dialog.cursor) {
                result = DialogResult::DriveSelected(name.clone());
            }
        }
    }

    if let Some(letter) = pressed_letter_key(ctx) {
        if letter != 'J' && letter != 'K' {
            let drive_name = format!("{}:", letter);
            if drives.iter().any(|(n, _)| n == &drive_name) {
                result = DialogResult::DriveSelected(drive_name);
            }
        }
    }

    let number_keys = [
        egui::Key::Num1, egui::Key::Num2, egui::Key::Num3,
        egui::Key::Num4, egui::Key::Num5, egui::Key::Num6,
        egui::Key::Num7, egui::Key::Num8, egui::Key::Num9,
    ];
    for (key_idx, key) in number_keys.iter().enumerate() {
        if ctx.input(|i| i.key_pressed(*key)) {
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
    result
}

fn show_registered_dir_dialog(ctx: &egui::Context, dialog: &mut RegisteredDirDialog) -> DialogResult {
    let mut result = DialogResult::None;
    let mut open = true;

    egui::Window::new("Registered Directories")
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .default_pos(ctx.content_rect().center())
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

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        result = DialogResult::Closed;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        if let Some(dir) = dialog.dirs.get(dialog.cursor) {
            result = DialogResult::RegisteredDirSelected(dir.path.clone());
        }
    }
    if let Some(letter) = pressed_letter_key(ctx) {
        let letter_str = letter.to_string();
        if let Some(dir) = dialog.dirs.iter().find(|d| d.key == letter_str) {
            result = DialogResult::RegisteredDirSelected(dir.path.clone());
        }
    }
    if !open {
        result = DialogResult::Closed;
    }
    result
}

fn show_history_dialog(ctx: &egui::Context, dialog: &mut HistoryDialog) -> DialogResult {
    let mut result = DialogResult::None;
    let mut open = true;

    egui::Window::new("History")
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .vscroll(true)
        .default_pos(ctx.content_rect().center())
        .pivot(egui::Align2::CENTER_CENTER)
        .open(&mut open)
        .show(ctx, |ui| {
            if dialog.entries.is_empty() {
                ui.label("No history.");
            } else {
                for (i, (path, exists)) in dialog.entries.iter().enumerate() {
                    let is_cursor = i == dialog.cursor;
                    let label = path.to_string_lossy();
                    let text = if !exists {
                        egui::RichText::new(label.as_ref())
                            .color(egui::Color32::from_rgb(100, 100, 100))
                            .strikethrough()
                    } else if is_cursor {
                        egui::RichText::new(label.as_ref())
                            .color(egui::Color32::from_rgb(100, 180, 255))
                            .strong()
                    } else {
                        egui::RichText::new(label.as_ref())
                    };
                    if ui
                        .add(egui::Label::new(text).sense(egui::Sense::click()))
                        .clicked()
                        && *exists
                    {
                        result = DialogResult::HistorySelected(i);
                    }
                }
            }
        });

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        result = DialogResult::Closed;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown)) {
        if !dialog.entries.is_empty() {
            dialog.cursor = (dialog.cursor + 1) % dialog.entries.len();
        }
    }
    if ctx.input(|i| i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp)) {
        if !dialog.entries.is_empty() {
            dialog.cursor = (dialog.cursor + dialog.entries.len() - 1) % dialog.entries.len();
        }
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::L)) {
        if let Some((_, exists)) = dialog.entries.get(dialog.cursor) {
            if *exists {
                result = DialogResult::HistorySelected(dialog.cursor);
            }
        }
    }
    if !open {
        result = DialogResult::Closed;
    }
    result
}

fn show_settings_main(ctx: &egui::Context, dialog: &mut SettingsDialog) -> DialogResult {
    let mut result = DialogResult::None;
    let mut open = true;

    let screen = ctx.content_rect();
    egui::Window::new("Settings")
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .vscroll(true)
        .default_pos(screen.center())
        .pivot(egui::Align2::CENTER_CENTER)
        .default_width(500.0)
        .default_height(screen.height() * 0.7)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(dialog.section == SettingsSection::Keybindings, "Keybindings").clicked() {
                    dialog.section = SettingsSection::Keybindings;
                }
                if ui.selectable_label(dialog.section == SettingsSection::Font, "Font").clicked() {
                    dialog.section = SettingsSection::Font;
                    dialog.filter_has_focus = true;
                }
            });
            ui.separator();

            if !dialog.kb_editing {
                if ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
                    dialog.section = match dialog.section {
                        SettingsSection::Font => SettingsSection::Keybindings,
                        SettingsSection::Keybindings => {
                            dialog.filter_has_focus = true;
                            SettingsSection::Font
                        }
                    };
                }
            }

            match dialog.section {
                SettingsSection::Font => {
                    show_settings_font_tab(ctx, ui, dialog, &mut result);
                }
                SettingsSection::Keybindings => {
                    show_settings_keybindings_tab(ctx, ui, dialog, &mut result);
                }
            }
        });

    if !dialog.kb_editing && dialog.kb_conflict.is_none()
        && ctx.input(|i| i.key_pressed(egui::Key::Escape))
    {
        result = DialogResult::Closed;
    }
    if !open {
        result = DialogResult::Closed;
    }
    result
}

fn cleanup_dialog_state(state: &mut DialogState, result: &DialogResult) {
    match result {
        DialogResult::ConfirmYes(_)
        | DialogResult::Closed
        | DialogResult::DriveSelected(_)
        | DialogResult::RegisteredDirSelected(_)
        | DialogResult::RegisteredDirEditKey(_)
        | DialogResult::FontSelected(_)
        | DialogResult::HistorySelected(_) => {
            state.confirm = None;
            state.input = None;
            state.message = None;
            state.drive = None;
            state.registered_dir = None;
            state.settings = None;
            state.history = None;
        }
        DialogResult::RegisteredDirDeleted(idx) => {
            if let Some(dialog) = &mut state.registered_dir {
                let idx = *idx;
                if idx < dialog.dirs.len() {
                    dialog.dirs.remove(idx);
                    if dialog.cursor >= dialog.dirs.len() && !dialog.dirs.is_empty() {
                        dialog.cursor = dialog.dirs.len() - 1;
                    }
                }
            }
        }
        DialogResult::InputOk(_, _) => {
            state.input = None;
        }
        DialogResult::KeybindingChanged(_, _)
        | DialogResult::KeybindingBatchChanged(_)
        | DialogResult::KeybindingReset(_) => {}
        DialogResult::None => {}
    }
}

fn show_settings_font_tab(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    dialog: &mut SettingsDialog,
    result: &mut DialogResult,
) {
    ui.label(format!("Font: {}", dialog.current_font));

    // Filter input
    let filter_response = ui.add(
        egui::TextEdit::singleline(&mut dialog.filter)
            .hint_text("Filter...")
            .desired_width(ui.available_width() - 8.0),
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

    for (list_idx, (_, name, path)) in filtered.iter().enumerate() {
        let is_cursor = list_idx == dialog.cursor;
        let text = if is_cursor {
            egui::RichText::new(*name)
                .color(egui::Color32::from_rgb(100, 180, 255))
                .strong()
        } else {
            egui::RichText::new(*name)
        };
        let btn_response = ui.button(text);
        if is_cursor {
            btn_response.scroll_to_me(Some(egui::Align::Center));
        }
        if btn_response.clicked() {
            let font_path = if path.is_empty() { None } else { Some(path.to_string()) };
            *result = DialogResult::FontSelected(font_path);
        }
    }

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
                *result = DialogResult::FontSelected(font_path);
            }
        }
    }
}

fn show_settings_keybindings_tab(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    dialog: &mut SettingsDialog,
    result: &mut DialogResult,
) {
    // --- Conflict confirmation mode ---
    if let Some(conflict) = &dialog.kb_conflict {
        let conflict_names: Vec<&str> = conflict.conflicts.iter().map(|(_, desc)| desc.as_str()).collect();
        ui.label(
            egui::RichText::new(format!(
                "「{}」は以下に割り当て済みです:\n  {}\n\n除去して割り当てますか？",
                conflict.binding.display(),
                conflict_names.join(", ")
            ))
            .strong(),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Enter: はい  /  Esc: キャンセル");
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            // Confirmed: remove from conflicts, add to target
            let Some(conflict) = dialog.kb_conflict.take() else { return; };
            let mut batch: Vec<(Action, Vec<KeyBinding>)> = Vec::new();
            for (conflict_idx, _) in &conflict.conflicts {
                dialog.kb_actions[*conflict_idx].2.retain(|b| b != &conflict.binding);
                dialog.kb_customized[*conflict_idx] = true;
                let action = dialog.kb_actions[*conflict_idx].0;
                let cloned = dialog.kb_actions[*conflict_idx].2.clone();
                batch.push((action, cloned));
            }
            if !dialog.kb_actions[conflict.target_idx].2.contains(&conflict.binding) {
                dialog.kb_actions[conflict.target_idx].2.push(conflict.binding);
            }
            dialog.kb_customized[conflict.target_idx] = true;
            dialog.kb_sub_cursor = dialog.kb_actions[conflict.target_idx].2.len() - 1;
            let target_action = dialog.kb_actions[conflict.target_idx].0;
            let target_bindings = dialog.kb_actions[conflict.target_idx].2.clone();
            batch.push((target_action, target_bindings));
            *result = DialogResult::KeybindingBatchChanged(batch);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            dialog.kb_conflict = None;
        }
        return;
    }

    // --- Key capture mode ---
    if dialog.kb_editing {
        ui.label(egui::RichText::new("キーを押してください (Esc: キャンセル)").strong());
        ui.add_space(4.0);

        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            dialog.kb_editing = false;
            return;
        }

        let new_binding = ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                    return Some(KeyBinding::from_key_event(*key, modifiers));
                }
            }
            None
        });

        if let Some(binding) = new_binding {
            dialog.kb_editing = false;
            let filter_lower = dialog.kb_filter.to_lowercase();
            let filtered_indices: Vec<usize> = kb_filtered_indices(&dialog.kb_actions, &filter_lower);
            if let Some(&actual_idx) = filtered_indices.get(dialog.kb_cursor) {
                // Check for conflicts with other actions
                let conflicts: Vec<(usize, String)> = dialog.kb_actions.iter().enumerate()
                    .filter(|(idx, (_, _, bindings))| {
                        *idx != actual_idx && bindings.contains(&binding)
                    })
                    .map(|(idx, (_, desc, _))| (idx, desc.clone()))
                    .collect();

                if conflicts.is_empty() {
                    // No conflict: add directly
                    let action = dialog.kb_actions[actual_idx].0;
                    let bindings = &mut dialog.kb_actions[actual_idx].2;
                    if !bindings.contains(&binding) {
                        bindings.push(binding);
                    }
                    dialog.kb_customized[actual_idx] = true;
                    dialog.kb_sub_cursor = bindings.len() - 1;
                    *result = DialogResult::KeybindingChanged(action, bindings.clone());
                } else {
                    // Conflict detected: show confirmation
                    dialog.kb_conflict = Some(KbConflict {
                        binding,
                        target_idx: actual_idx,
                        conflicts,
                    });
                }
            }
        }
        return;
    }

    // --- Usage instructions ---
    ui.label(
        egui::RichText::new("j/k:選択  Enter:展開  d:デフォルトに戻す  Esc:閉じる")
            .small()
            .color(egui::Color32::from_rgb(140, 140, 140)),
    );
    ui.label(
        egui::RichText::new("展開中: x/Del:削除  Enter(+Add):追加  * はカスタム")
            .small()
            .color(egui::Color32::from_rgb(140, 140, 140)),
    );
    ui.add_space(2.0);

    // --- Filter input ---
    let filter_response = ui.add(
        egui::TextEdit::singleline(&mut dialog.kb_filter)
            .hint_text("Filter...")
            .desired_width(ui.available_width() - 8.0),
    );
    let filter_focused = filter_response.has_focus();

    ui.add_space(4.0);

    // Build filtered list
    let filter_lower = dialog.kb_filter.to_lowercase();
    let filtered_indices: Vec<usize> = kb_filtered_indices(&dialog.kb_actions, &filter_lower);

    // Clamp cursor
    if dialog.kb_cursor >= filtered_indices.len() {
        dialog.kb_cursor = filtered_indices.len().saturating_sub(1);
    }

    for (list_idx, &actual_idx) in filtered_indices.iter().enumerate() {
        let is_cursor = list_idx == dialog.kb_cursor;
        let (_, desc, bindings) = &dialog.kb_actions[actual_idx];
        let customized = dialog.kb_customized[actual_idx];
        let keys_display = SettingsDialog::bindings_display(bindings);
        let is_expanded = is_cursor && dialog.kb_expanded;

        // Main action row
        let marker = if customized { "* " } else { "  " };
        let label = format!("{}{:<30} {}", marker, desc, keys_display);
        let text = if is_cursor && customized {
            egui::RichText::new(&label)
                .color(egui::Color32::from_rgb(100, 180, 255))
                .strong()
        } else if is_cursor {
            egui::RichText::new(&label)
                .color(egui::Color32::from_rgb(100, 180, 255))
        } else if customized {
            egui::RichText::new(&label).strong()
        } else {
            egui::RichText::new(&label)
        };
        let row_response = ui.label(text);
        if is_cursor {
            row_response.scroll_to_me(Some(egui::Align::Center));
        }

        // Expanded view: show individual bindings + "Add new"
        if is_expanded {
            let total_sub_items = bindings.len() + 1; // bindings + "Add new"
            for (bind_idx, binding) in bindings.iter().enumerate() {
                let is_sub_cursor = dialog.kb_sub_cursor == bind_idx;
                let bind_label = format!("      {} {}", if is_sub_cursor { ">" } else { " " }, binding.display());
                let bind_text = if is_sub_cursor {
                    egui::RichText::new(&bind_label)
                        .color(egui::Color32::from_rgb(255, 200, 100))
                } else {
                    egui::RichText::new(&bind_label)
                        .color(egui::Color32::from_rgb(180, 180, 180))
                };
                ui.label(bind_text);
            }
            // "Add new" row
            let is_add_cursor = dialog.kb_sub_cursor == bindings.len();
            let add_label = format!("      {} + Add new binding", if is_add_cursor { ">" } else { " " });
            let add_text = if is_add_cursor {
                egui::RichText::new(&add_label)
                    .color(egui::Color32::from_rgb(255, 200, 100))
            } else {
                egui::RichText::new(&add_label)
                    .color(egui::Color32::from_rgb(120, 180, 120))
            };
            ui.label(add_text);

            // Clamp sub_cursor
            if dialog.kb_sub_cursor >= total_sub_items {
                dialog.kb_sub_cursor = total_sub_items.saturating_sub(1);
            }
        }
    }

    // --- Keyboard navigation ---
    if !filter_focused && !filtered_indices.is_empty() {
        if dialog.kb_expanded {
            // Expanded mode navigation
            let actual_idx = filtered_indices[dialog.kb_cursor];
            let bindings_len = dialog.kb_actions[actual_idx].2.len();
            let total_sub = bindings_len + 1; // bindings + "Add new"

            if ctx.input(|i| i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown)) {
                dialog.kb_sub_cursor = (dialog.kb_sub_cursor + 1) % total_sub;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp)) {
                dialog.kb_sub_cursor = (dialog.kb_sub_cursor + total_sub - 1) % total_sub;
            }
            // Enter: add new binding (if on "Add new" row)
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                if dialog.kb_sub_cursor == bindings_len {
                    dialog.kb_editing = true;
                }
            }
            // Delete / x: remove selected binding
            if ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::X)) {
                if dialog.kb_sub_cursor < bindings_len && bindings_len > 0 {
                    let action = dialog.kb_actions[actual_idx].0;
                    dialog.kb_actions[actual_idx].2.remove(dialog.kb_sub_cursor);
                    dialog.kb_customized[actual_idx] = true;
                    let new_len = dialog.kb_actions[actual_idx].2.len();
                    if dialog.kb_sub_cursor >= new_len && new_len > 0 {
                        dialog.kb_sub_cursor = new_len - 1;
                    } else if new_len == 0 {
                        dialog.kb_sub_cursor = 0;
                    }
                    *result = DialogResult::KeybindingChanged(action, dialog.kb_actions[actual_idx].2.clone());
                }
            }
            // Escape: collapse back to action list
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                dialog.kb_expanded = false;
            }
            // d: reset to default
            if ctx.input(|i| i.key_pressed(egui::Key::D)) {
                let action = dialog.kb_actions[actual_idx].0;
                let defaults = KeyBindings::defaults();
                let default_bindings = defaults.bindings.get(&action).cloned().unwrap_or_default();
                dialog.kb_actions[actual_idx].2 = default_bindings;
                dialog.kb_customized[actual_idx] = false;
                dialog.kb_sub_cursor = 0;
                *result = DialogResult::KeybindingReset(action);
            }
        } else {
            // Action list navigation
            if ctx.input(|i| i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown)) {
                dialog.kb_cursor = (dialog.kb_cursor + 1) % filtered_indices.len();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp)) {
                dialog.kb_cursor = (dialog.kb_cursor + filtered_indices.len() - 1) % filtered_indices.len();
            }
            // Enter: expand to show individual bindings
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                dialog.kb_expanded = true;
                dialog.kb_sub_cursor = 0;
            }
            // d: reset to default
            if ctx.input(|i| i.key_pressed(egui::Key::D)) {
                if let Some(&actual_idx) = filtered_indices.get(dialog.kb_cursor) {
                    let action = dialog.kb_actions[actual_idx].0;
                    let defaults = KeyBindings::defaults();
                    let default_bindings = defaults.bindings.get(&action).cloned().unwrap_or_default();
                    dialog.kb_actions[actual_idx].2 = default_bindings;
                    dialog.kb_customized[actual_idx] = false;
                    *result = DialogResult::KeybindingReset(action);
                }
            }
        }
    }
}

/// Get filtered indices of kb_actions matching the filter.
fn kb_filtered_indices(
    kb_actions: &[(Action, String, Vec<KeyBinding>)],
    filter_lower: &str,
) -> Vec<usize> {
    kb_actions.iter().enumerate()
        .filter(|(_, (_, desc, bindings))| {
            let keys_display = SettingsDialog::bindings_display(bindings);
            filter_lower.is_empty()
                || desc.to_lowercase().contains(filter_lower)
                || keys_display.to_lowercase().contains(filter_lower)
        })
        .map(|(i, _)| i)
        .collect()
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
    let Ok(entries) = std::fs::read_dir(dir) else { return };
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
