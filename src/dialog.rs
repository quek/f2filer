use eframe::egui;

#[derive(Default)]
pub struct DialogState {
    pub confirm: Option<ConfirmDialog>,
    pub input: Option<InputDialog>,
    pub message: Option<MessageDialog>,
    pub drive: Option<DriveDialog>,
}

impl DialogState {
    pub fn is_open(&self) -> bool {
        self.confirm.is_some() || self.input.is_some() || self.message.is_some() || self.drive.is_some()
    }
}

pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Clone)]
pub enum ConfirmAction {
    Delete(Vec<std::path::PathBuf>),
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
}

pub struct MessageDialog {
    pub title: String,
    pub message: String,
}

pub struct DriveDialog {
    pub drives: Vec<String>,
}

pub enum DialogResult {
    None,
    ConfirmYes(ConfirmAction),
    InputOk(String, InputAction),
    DriveSelected(String),
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
        let letter_keys = [
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
        for (key, letter) in &letter_keys {
            if ctx.input(|inp| inp.key_pressed(*key)) {
                let drive_name = format!("{}:", letter);
                if drives.iter().any(|d| d == &drive_name) {
                    result = DialogResult::DriveSelected(drive_name);
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

    // Clean up closed dialogs
    match &result {
        DialogResult::ConfirmYes(_) | DialogResult::Closed | DialogResult::DriveSelected(_) => {
            state.confirm = None;
            state.input = None;
            state.message = None;
            state.drive = None;
        }
        DialogResult::InputOk(_, _) => {
            state.input = None;
        }
        DialogResult::None => {}
    }

    result
}
