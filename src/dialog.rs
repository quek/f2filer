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
}

impl DialogState {
    pub fn is_open(&self) -> bool {
        self.confirm.is_some()
            || self.input.is_some()
            || self.message.is_some()
            || self.drive.is_some()
            || self.registered_dir.is_some()
            || self.progress.is_some()
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
    pub drives: Vec<String>,
}

pub struct RegisteredDirDialog {
    pub dirs: Vec<RegisteredDir>,
    pub cursor: usize,
}

pub enum DialogResult {
    None,
    ConfirmYes(ConfirmAction),
    InputOk(String, InputAction),
    DriveSelected(String),
    RegisteredDirSelected(String),
    RegisteredDirDeleted(usize),
    RegisteredDirEditKey(usize),
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

        egui::Window::new(&title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .max_height(400.0)
            .show(ctx, |ui| {
                ui.set_min_width(300.0);
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        ui.label(&message);
                    });
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
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut dialog.value)
                        .desired_width(300.0),
                );

                // Auto-focus the text input
                if !response.has_focus() {
                    response.request_focus();
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

        egui::Window::new(&title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .max_height(400.0)
            .show(ctx, |ui| {
                ui.set_min_width(300.0);
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        ui.label(&message);
                    });
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
    if let Some(dialog) = &state.drive {
        let drives = dialog.drives.clone();
        let mut open = true;

        egui::Window::new("Select Drive")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for drive in drives.iter() {
                        if ui.button(drive).clicked() {
                            result = DialogResult::DriveSelected(drive.clone());
                        }
                    }
                });
            });

        // Drive letter key shortcuts (e.g. press 'c' for "C:")
        if let Some(letter) = pressed_letter_key(ctx) {
            let drive_name = format!("{}:", letter);
            if drives.iter().any(|d| d == &drive_name) {
                result = DialogResult::DriveSelected(drive_name);
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
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
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
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
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

    // Clean up closed dialogs
    match &result {
        DialogResult::ConfirmYes(_)
        | DialogResult::Closed
        | DialogResult::DriveSelected(_)
        | DialogResult::RegisteredDirSelected(_)
        | DialogResult::RegisteredDirEditKey(_) => {
            state.confirm = None;
            state.input = None;
            state.message = None;
            state.drive = None;
            state.registered_dir = None;
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
