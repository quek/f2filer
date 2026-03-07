use std::path::PathBuf;

use eframe::egui;

use crate::app::{ActivePanel, F2App};
use crate::dialog::*;
use crate::file_ops;

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
    e: bool,
    alt_enter: bool,
    backslash: bool,
}

fn read_key_state(ctx: &egui::Context) -> KeyState {
    ctx.input(|i| KeyState {
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
        e: i.key_pressed(egui::Key::E),
        alt_enter: i.key_pressed(egui::Key::Enter) && i.modifiers.alt,
        backslash: i.key_pressed(egui::Key::Backslash),
    })
}

/// Detect Ctrl+C/V using Win32 GetAsyncKeyState, bypassing egui's event system.
#[cfg(windows)]
fn detect_ctrl_cv() -> (bool, bool) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    static PREV_C: AtomicBool = AtomicBool::new(false);
    static PREV_V: AtomicBool = AtomicBool::new(false);

    unsafe {
        let ctrl = GetAsyncKeyState(0x11) < 0; // VK_CONTROL
        let c_down = ctrl && GetAsyncKeyState(0x43) < 0;
        let v_down = ctrl && GetAsyncKeyState(0x56) < 0;

        let prev_c = PREV_C.load(Ordering::Relaxed);
        PREV_C.store(c_down, Ordering::Relaxed);
        let prev_v = PREV_V.load(Ordering::Relaxed);
        PREV_V.store(v_down, Ordering::Relaxed);

        (c_down && !prev_c, v_down && !prev_v)
    }
}

pub(crate) fn handle_keyboard(app: &mut F2App, ctx: &egui::Context) {
    // Don't handle keys when dialog is open or command mode
    if app.dialog.is_open() {
        return;
    }
    if app.command_mode {
        return;
    }
    if app.active_panel().filter_has_focus {
        return;
    }

    let input = read_key_state(ctx);

    handle_navigation(app, &input);
    handle_file_operations(app, ctx, &input);
    handle_edit_operations(app, &input);
    handle_misc_keys(app, ctx, &input);

    // Update preview on cursor move
    if app.preview_mode
        && (input.j
            || input.k
            || input.up
            || input.down
            || input.page_up
            || input.page_down
            || input.home
            || input.end)
    {
        app.update_preview(ctx);
    }
}

fn handle_navigation(app: &mut F2App, input: &KeyState) {
    // Tab: switch panel
    if input.tab {
        app.active = match app.active {
            ActivePanel::Left => ActivePanel::Right,
            ActivePanel::Right => ActivePanel::Left,
        };
    }

    // Navigation
    if input.j || input.down {
        app.active_panel_mut().move_cursor(1);
    }
    if input.k || input.up {
        app.active_panel_mut().move_cursor(-1);
    }
    if input.home {
        app.active_panel_mut().move_cursor_to_start();
    }
    if input.end {
        app.active_panel_mut().move_cursor_to_end();
    }
    if input.page_up {
        app.active_panel_mut().page_up(20);
    }
    if input.page_down {
        app.active_panel_mut().page_down(20);
    }

    // Space/Insert: toggle selection
    if input.space || input.insert {
        app.active_panel_mut().toggle_select();
        app.active_panel_mut().move_cursor(1);
    }

    // a: select all
    if input.a {
        app.active_panel_mut().select_all();
    }
}

fn handle_file_operations(app: &mut F2App, ctx: &egui::Context, input: &KeyState) {
    // l / Enter: open dir / execute file
    if input.l || input.enter {
        if let Some(entry) = app.active_panel().current_entry().cloned() {
            if entry.is_dir {
                let dir = entry.path.clone();
                app.active_panel_mut().navigate_to(dir);
                app.save_config();
            } else {
                open::that(&entry.path).ok();
            }
        }
    }

    // e: open with text editor (.txt association)
    if input.e {
        if let Some(entry) = app.active_panel().current_entry() {
            if !entry.is_dir && entry.name != ".." {
                crate::shell::open_with_text_editor(&entry.path);
            }
        }
    }

    // h: parent directory
    if input.h {
        if let Some(parent) = app.active_panel().current_dir.parent().map(|p| p.to_path_buf()) {
            app.active_panel_mut().navigate_to(parent);
            app.save_config();
        }
    }

    // c / m: copy or move to opposite panel
    if input.c {
        start_copy_or_move(app, ctx, false);
    }
    if input.m {
        start_copy_or_move(app, ctx, true);
    }

    // Ctrl+C / Ctrl+V: clipboard file operations
    let (evt_copy, evt_paste) = detect_ctrl_cv();

    // Consume egui clipboard events so they don't interfere
    ctx.input_mut(|i| {
        i.events.retain(|e| !matches!(e,
            egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_)
        ));
    });

    if evt_copy {
        let targets = app.active_panel().get_operation_targets();
        if !targets.is_empty() {
            let paths: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
            crate::shell::copy_files_to_clipboard(&paths, false);
            app.status_message = format!("Copied {} item(s) to clipboard", paths.len());
        }
    }

    // Ctrl+V: paste files from clipboard
    if evt_paste {
        if let Some((sources, is_cut)) = crate::shell::paste_files_from_clipboard() {
            if !sources.is_empty() {
                let dest = app.active_panel().current_dir.clone();
                let conflicts = file_ops::check_conflicts(&sources, &dest);

                if conflicts.is_empty() {
                    let op = if is_cut {
                        OpKind::Move { sources, dest_dir: dest, overwrite: false }
                    } else {
                        OpKind::Copy { sources, dest_dir: dest, overwrite: false }
                    };
                    app.start_background_op(ctx, op);
                } else {
                    let action = if is_cut {
                        ConfirmAction::MoveOverwrite { sources, dest }
                    } else {
                        ConfirmAction::CopyOverwrite { sources, dest }
                    };
                    app.dialog.confirm = Some(ConfirmDialog {
                        title: "Overwrite?".to_string(),
                        message: format!(
                            "The following files already exist:\n{}\n\nOverwrite?",
                            conflicts.join(", ")
                        ),
                        action,
                    });
                }
            }
        }
    }

    // d: delete (with confirmation)
    if input.d {
        let targets = app.active_panel().get_operation_targets();
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
            app.dialog.confirm = Some(ConfirmDialog {
                title: if is_unc {
                    "Delete (permanent)".to_string()
                } else {
                    "Delete".to_string()
                },
                message,
                action: ConfirmAction::Delete(paths),
            });
        }
    }

    // Shift+D: permanent delete (with confirmation)
    if input.shift_d {
        let targets = app.active_panel().get_operation_targets();
        if !targets.is_empty() {
            let names: Vec<String> = targets.iter().map(|t| t.name.clone()).collect();
            let paths: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
            app.dialog.confirm = Some(ConfirmDialog {
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
        if let Some(entry) = app.active_panel().current_entry() {
            if entry.name != ".." {
                crate::shell::show_file_properties(&entry.path);
            }
        }
    }

    // \: context menu
    if input.backslash {
        if let Some(entry) = app.active_panel().current_entry().cloned() {
            if entry.name != ".." {
                crate::shell::show_context_menu(&entry.path);
                app.active_panel_mut().refresh();
            }
        }
    }

    // Shift+U: zip compress selected files
    if input.shift_u {
        let targets = app.active_panel().get_operation_targets();
        if !targets.is_empty() {
            let sources: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
            let default_name = targets
                .first()
                .map(|t| {
                    PathBuf::from(&t.name)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| t.name.clone())
                })
                .unwrap_or_else(|| "archive".to_string());
            app.dialog.input = Some(InputDialog {
                title: "Zip Compress".to_string(),
                value: default_name,
                action: InputAction::ZipCompress(sources),
            });
        }
    }

    // u: decompress zip at cursor
    if input.u {
        if let Some(entry) = app.active_panel().current_entry() {
            if !entry.is_dir {
                let is_zip = entry
                    .path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase() == "zip")
                    .unwrap_or(false);
                if is_zip {
                    let zip_path = entry.path.clone();
                    let dest = app.inactive_panel().current_dir.clone();
                    app.start_background_op(
                        ctx,
                        OpKind::ZipDecompress {
                            zip_path,
                            dest_dir: dest,
                        },
                    );
                }
            }
        }
    }
}

fn start_copy_or_move(app: &mut F2App, ctx: &egui::Context, is_move: bool) {
    let targets = app.active_panel().get_operation_targets();
    if targets.is_empty() {
        return;
    }
    let dest = app.inactive_panel().current_dir.clone();
    let sources: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
    let conflicts = file_ops::check_conflicts(&sources, &dest);

    if conflicts.is_empty() {
        let op = if is_move {
            OpKind::Move {
                sources,
                dest_dir: dest,
                overwrite: false,
            }
        } else {
            OpKind::Copy {
                sources,
                dest_dir: dest,
                overwrite: false,
            }
        };
        app.start_background_op(ctx, op);
    } else {
        let action = if is_move {
            ConfirmAction::MoveOverwrite { sources, dest }
        } else {
            ConfirmAction::CopyOverwrite { sources, dest }
        };
        app.dialog.confirm = Some(ConfirmDialog {
            title: "Overwrite?".to_string(),
            message: format!(
                "The following files already exist:\n{}\n\nOverwrite?",
                conflicts.join(", ")
            ),
            action,
        });
    }
}

fn handle_edit_operations(app: &mut F2App, input: &KeyState) {
    // r: rename
    if input.r {
        if let Some(entry) = app.active_panel().current_entry() {
            if entry.name != ".." {
                app.dialog.input = Some(InputDialog {
                    title: "Rename".to_string(),
                    value: entry.name.clone(),
                    action: InputAction::Rename(entry.path.clone()),
                });
            }
        }
    }

    // n: new directory
    if input.n {
        app.dialog.input = Some(InputDialog {
            title: "New Directory".to_string(),
            value: String::new(),
            action: InputAction::NewDirectory,
        });
    }
}

fn handle_misc_keys(app: &mut F2App, ctx: &egui::Context, input: &KeyState) {
    // Ctrl+R: refresh
    if input.ctrl_r {
        app.active_panel_mut().refresh();
        app.status_message = "Refreshed".to_string();
    }

    // q / Ctrl+Q: quit
    if input.q || input.ctrl_q {
        app.save_config();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // Ctrl+.: toggle hidden
    if input.period {
        let show = !app.active_panel().show_hidden;
        app.active_panel_mut().show_hidden = show;
        app.active_panel_mut().refresh();
    }

    // v: toggle preview mode
    if input.v {
        if app.preview_mode {
            app.preview_mode = false;
            app.clear_all_previews();
        } else {
            app.preview_mode = true;
            app.update_preview(ctx);
        }
    }

    // o: sync opposite panel to current directory
    if input.o {
        let dir = app.active_panel().current_dir.clone();
        app.inactive_panel_mut().navigate_to(dir);
        app.status_message = "Synced opposite panel".to_string();
        app.save_config();
    }

    // y: copy current file path to clipboard
    if input.y {
        if let Some(entry) = app.active_panel().current_entry() {
            let path_str = entry.path.to_string_lossy().to_string();
            match arboard::Clipboard::new() {
                Ok(mut clip) => {
                    if clip.set_text(&path_str).is_ok() {
                        app.status_message = format!("Copied: {}", path_str);
                    } else {
                        app.status_message = "Failed to copy to clipboard".to_string();
                    }
                }
                Err(_) => {
                    app.status_message = "Failed to access clipboard".to_string();
                }
            }
        }
    }

    // ?: show help
    if input.question {
        app.dialog.message = Some(MessageDialog {
            title: "Keyboard Shortcuts".to_string(),
            message: "\
j / k / ↑ / ↓  :  Cursor move
l              :  Open dir / Execute file
e              :  Open with text editor
h              :  Parent directory
i              :  Switch panel
Space          :  Toggle select
Ctrl+A         :  Select all
f              :  Focus filter
o              :  Sync opposite panel
c              :  Copy selected → opposite
m              :  Move selected → opposite
Ctrl+C         :  Copy selected to clipboard
Ctrl+V         :  Paste from clipboard
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
v              :  Preview (text/image/audio/video)
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
        app.active_panel_mut().focus_filter = true;
    }

    // p: drive selection
    if input.p {
        use crate::file_ops::{get_drive_space, format_size_human};
        let drives = app.drives.iter().map(|name| {
            let root = if name.contains(':') && !name.starts_with("WSL:") {
                format!("{}\\", name)
            } else {
                String::new()
            };
            let space = if !root.is_empty() {
                get_drive_space(&root).map(|(free, total)| {
                    let used_pct = if total > 0 {
                        ((total - free) as f64 / total as f64 * 100.0) as u64
                    } else {
                        0
                    };
                    format!("{} / {} ({}%)", format_size_human(free), format_size_human(total), used_pct)
                }).unwrap_or_default()
            } else {
                String::new()
            };
            (name.clone(), space)
        }).collect();
        app.dialog.drive = Some(DriveDialog { drives, cursor: 0 });
    }

    // g: registered directories
    if input.g {
        app.dialog.registered_dir = Some(RegisteredDirDialog {
            dirs: app.config.registered_dirs.clone(),
            cursor: 0,
        });
    }

    // Shift+G: register current directory
    if input.shift_g {
        let dir = app.active_panel().current_dir.clone();
        let default_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.to_string_lossy().to_string());
        app.dialog.input = Some(InputDialog {
            title: "Register Directory".to_string(),
            value: default_name,
            action: InputAction::RegisterDirectory(dir),
        });
    }

    // z: undo
    if input.z {
        match app.undo_history.undo() {
            Ok(msg) => {
                app.status_message = msg;
                app.left_panel.refresh();
                app.right_panel.refresh();
            }
            Err(msg) => {
                app.status_message = msg;
            }
        }
    }

    // Shift+z: redo
    if input.shift_z {
        match app.undo_history.redo() {
            Ok(msg) => {
                app.status_message = msg;
                app.left_panel.refresh();
                app.right_panel.refresh();
            }
            Err(msg) => {
                app.status_message = msg;
            }
        }
    }

    // :: command mode
    if input.colon {
        app.command_mode = true;
        app.command_line.clear();
    }
}
