use std::path::PathBuf;

use eframe::egui;

use crate::app::{ActivePanel, F2App};
use crate::dialog::*;
use crate::file_ops;
use crate::keybind::Action;

const NAME_LIST_MAX_DISPLAY: usize = 20;

/// Format a list of names for display in dialogs (one per line, truncated).
pub fn format_name_list(names: &[String]) -> String {
    if names.len() <= NAME_LIST_MAX_DISPLAY {
        names.join("\n")
    } else {
        let shown: Vec<&str> = names[..NAME_LIST_MAX_DISPLAY].iter().map(|s| s.as_str()).collect();
        format!("{}\n...and {} more", shown.join("\n"), names.len() - NAME_LIST_MAX_DISPLAY)
    }
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

    let kb = &app.keybindings;

    // Sort chord: s then n/e/s/d
    if app.sort_pending {
        // Wait for a key press event (ignore mouse/pointer events)
        let any_key = ctx.input(|i| {
            i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { pressed: true, .. } | egui::Event::Text(_))
            })
        });
        if !any_key {
            return; // No input yet, keep waiting
        }
        app.sort_pending = false;
        let sort_key = ctx.input(|i| {
            if kb.is_action_pressed(Action::SortByName, i) {
                Some(crate::sort::SortKey::Name)
            } else if kb.is_action_pressed(Action::SortByExtension, i) {
                Some(crate::sort::SortKey::Extension)
            } else if kb.is_action_pressed(Action::SortBySize, i) {
                Some(crate::sort::SortKey::Size)
            } else if kb.is_action_pressed(Action::SortByDate, i) {
                Some(crate::sort::SortKey::Date)
            } else {
                None // Unrecognized key cancels sort mode
            }
        });
        if let Some(key) = sort_key {
            app.active_panel_mut().set_sort(key);
        }
        return;
    }
    // SortMode: enter sort pending mode, skip all other handlers this frame
    if ctx.input(|i| kb.is_action_pressed(Action::SortMode, i)) {
        app.sort_pending = true;
        return;
    }

    // Read all actions for this frame in a single ctx.input() call
    let actions = ctx.input(|i| ActionFlags {
        switch_panel: kb.is_action_pressed(Action::SwitchPanel, i),
        cursor_down: kb.is_action_pressed(Action::CursorDown, i),
        cursor_up: kb.is_action_pressed(Action::CursorUp, i),
        cursor_to_top: kb.is_action_pressed(Action::CursorToTop, i),
        cursor_to_bottom: kb.is_action_pressed(Action::CursorToBottom, i),
        page_up: kb.is_action_pressed(Action::PageUp, i),
        page_down: kb.is_action_pressed(Action::PageDown, i),
        toggle_select: kb.is_action_pressed(Action::ToggleSelect, i),
        toggle_select_all: kb.is_action_pressed(Action::ToggleSelectAll, i),
        open: kb.is_action_pressed(Action::Open, i),
        open_text_editor: kb.is_action_pressed(Action::OpenTextEditor, i),
        parent_dir: kb.is_action_pressed(Action::ParentDir, i),
        copy: kb.is_action_pressed(Action::Copy, i),
        move_: kb.is_action_pressed(Action::Move, i),
        delete: kb.is_action_pressed(Action::Delete, i),
        delete_permanent: kb.is_action_pressed(Action::DeletePermanent, i),
        open_recycle_bin: kb.is_action_pressed(Action::OpenRecycleBin, i),
        file_properties: kb.is_action_pressed(Action::FileProperties, i),
        context_menu: kb.is_action_pressed(Action::ContextMenu, i),
        zip_compress: kb.is_action_pressed(Action::ZipCompress, i),
        decompress: kb.is_action_pressed(Action::Decompress, i),
        rename: kb.is_action_pressed(Action::Rename, i),
        new_directory: kb.is_action_pressed(Action::NewDirectory, i),
        refresh: kb.is_action_pressed(Action::Refresh, i),
        quit: kb.is_action_pressed(Action::Quit, i),
        toggle_hidden: kb.is_action_pressed(Action::ToggleHidden, i),
        toggle_preview: kb.is_action_pressed(Action::TogglePreview, i),
        sync_opposite_panel: kb.is_action_pressed(Action::SyncOppositePanel, i),
        copy_path_clipboard: kb.is_action_pressed(Action::CopyPathClipboard, i),
        show_help: kb.is_action_pressed(Action::ShowHelp, i),
        focus_filter: kb.is_action_pressed(Action::FocusFilter, i),
        recursive_search: kb.is_action_pressed(Action::RecursiveSearch, i),
        exit_recursive_search: kb.is_action_pressed(Action::ExitRecursiveSearch, i),
        drive_select: kb.is_action_pressed(Action::DriveSelect, i),
        registered_dirs: kb.is_action_pressed(Action::RegisteredDirs, i),
        register_dir: kb.is_action_pressed(Action::RegisterDir, i),
        undo: kb.is_action_pressed(Action::Undo, i),
        redo: kb.is_action_pressed(Action::Redo, i),
        font_size_up: kb.is_action_pressed(Action::FontSizeUp, i),
        font_size_down: kb.is_action_pressed(Action::FontSizeDown, i),
        settings: kb.is_action_pressed(Action::Settings, i),
        command_mode: kb.is_action_pressed(Action::CommandMode, i),
    });

    handle_navigation(app, &actions);
    handle_file_operations(app, ctx, &actions);
    handle_edit_operations(app, &actions);
    handle_misc_keys(app, ctx, &actions);

    // Update preview on cursor move
    if app.preview_mode
        && (actions.cursor_down
            || actions.cursor_up
            || actions.page_up
            || actions.page_down
            || actions.cursor_to_top
            || actions.cursor_to_bottom)
    {
        app.update_preview(ctx);
    }
}

struct ActionFlags {
    switch_panel: bool,
    cursor_down: bool,
    cursor_up: bool,
    cursor_to_top: bool,
    cursor_to_bottom: bool,
    page_up: bool,
    page_down: bool,
    toggle_select: bool,
    toggle_select_all: bool,
    open: bool,
    open_text_editor: bool,
    parent_dir: bool,
    copy: bool,
    move_: bool,
    delete: bool,
    delete_permanent: bool,
    open_recycle_bin: bool,
    file_properties: bool,
    context_menu: bool,
    zip_compress: bool,
    decompress: bool,
    rename: bool,
    new_directory: bool,
    refresh: bool,
    quit: bool,
    toggle_hidden: bool,
    toggle_preview: bool,
    sync_opposite_panel: bool,
    copy_path_clipboard: bool,
    show_help: bool,
    focus_filter: bool,
    recursive_search: bool,
    exit_recursive_search: bool,
    drive_select: bool,
    registered_dirs: bool,
    register_dir: bool,
    undo: bool,
    redo: bool,
    font_size_up: bool,
    font_size_down: bool,
    settings: bool,
    command_mode: bool,
}

fn handle_navigation(app: &mut F2App, a: &ActionFlags) {
    // Switch panel
    if a.switch_panel {
        app.active = match app.active {
            ActivePanel::Left => ActivePanel::Right,
            ActivePanel::Right => ActivePanel::Left,
        };
    }

    // Navigation
    if a.cursor_down {
        app.active_panel_mut().move_cursor(1);
    }
    if a.cursor_up {
        app.active_panel_mut().move_cursor(-1);
    }
    if a.cursor_to_top {
        app.active_panel_mut().move_cursor_to_start();
    }
    if a.cursor_to_bottom {
        app.active_panel_mut().move_cursor_to_end();
    }
    if a.page_up {
        app.active_panel_mut().page_up(20);
    }
    if a.page_down {
        app.active_panel_mut().page_down(20);
    }

    // Toggle selection
    if a.toggle_select {
        app.active_panel_mut().toggle_select();
        app.active_panel_mut().move_cursor(1);
    }

    // Toggle select all / deselect all
    if a.toggle_select_all {
        let panel = app.active_panel_mut();
        if panel.selected.is_empty() {
            panel.select_all();
        } else {
            panel.deselect_all();
        }
    }
}

fn handle_file_operations(app: &mut F2App, ctx: &egui::Context, a: &ActionFlags) {
    // Open dir / execute file
    if a.open {
        if let Some(entry) = app.active_panel().current_entry().cloned() {
            if app.active_panel().recursive_filter {
                // In recursive search mode: navigate to file's parent directory
                if let Some(parent) = entry.path.parent() {
                    let parent = parent.to_path_buf();
                    let filename = entry
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string());
                    app.active_panel_mut().exit_recursive_search();
                    app.active_panel_mut().navigate_to(parent, ctx);
                    if let Some(name) = filename {
                        app.active_panel_mut().loading_old_name = Some(name);
                    }
                    app.save_config();
                }
            } else if entry.is_dir {
                let dir = entry.path.clone();
                app.active_panel_mut().navigate_to(dir, ctx);
                app.save_config();
            } else {
                open::that(&entry.path).ok();
            }
        }
    }

    // Open with text editor
    if a.open_text_editor {
        if let Some(entry) = app.active_panel().current_entry() {
            if !entry.is_dir {
                crate::shell::open_with_text_editor(&entry.path);
            }
        }
    }

    // Parent directory
    if a.parent_dir {
        if let Some(parent) = app.active_panel().current_dir.parent().map(|p| p.to_path_buf()) {
            app.active_panel_mut().navigate_to(parent, ctx);
            app.save_config();
        }
    }

    // Copy / move to opposite panel
    if a.copy {
        start_copy_or_move(app, ctx, false);
    }
    if a.move_ {
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
                            format_name_list(&conflicts)
                        ),
                        action,
                    });
                }
            }
        }
    }

    // Delete (with confirmation)
    if a.delete {
        let targets = app.active_panel().get_operation_targets();
        if !targets.is_empty() {
            let names: Vec<String> = targets.iter().map(|t| t.name.clone()).collect();
            let paths: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
            let is_unc = paths.iter().any(|p| p.to_string_lossy().starts_with(r"\\"));
            let list = format_name_list(&names);
            let message = if is_unc {
                format!(
                    "PERMANENTLY delete {} item(s)?\n{}\n\nNetwork path: recycle bin is not available.",
                    names.len(), list
                )
            } else {
                format!("Delete {} item(s)?\n{}", names.len(), list)
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

    // Permanent delete (with confirmation)
    if a.delete_permanent {
        let targets = app.active_panel().get_operation_targets();
        if !targets.is_empty() {
            let names: Vec<String> = targets.iter().map(|t| t.name.clone()).collect();
            let paths: Vec<PathBuf> = targets.iter().map(|t| t.path.clone()).collect();
            let list = format_name_list(&names);
            app.dialog.confirm = Some(ConfirmDialog {
                title: "⚠ Permanent Delete".to_string(),
                message: format!(
                    "PERMANENTLY delete {} item(s)?\n{}\n\nThis cannot be undone!",
                    names.len(), list
                ),
                action: ConfirmAction::DeletePermanent(paths),
            });
        }
    }

    // Open recycle bin
    if a.open_recycle_bin {
        let _ = std::process::Command::new("explorer.exe")
            .arg("shell:RecycleBinFolder")
            .spawn();
    }

    // File properties
    if a.file_properties {
        if let Some(entry) = app.active_panel().current_entry() {
            crate::shell::show_file_properties(&entry.path);
        }
    }

    // Context menu
    if a.context_menu {
        if let Some(entry) = app.active_panel().current_entry().cloned() {
            crate::shell::show_context_menu(&entry.path);
            app.active_panel_mut().refresh();
        }
    }

    // Zip compress selected files
    if a.zip_compress {
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
                select_end: None,
            });
        }
    }

    // Decompress archive at cursor (zip/tar.gz/tgz/tar)
    if a.decompress {
        if let Some(entry) = app.active_panel().current_entry() {
            if !entry.is_dir {
                let name_lower = entry.name.to_lowercase();
                let ext_lower = entry
                    .path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase())
                    .unwrap_or_default();
                let dest = app.inactive_panel().current_dir.clone();
                if ext_lower == "zip" {
                    app.start_background_op(
                        ctx,
                        OpKind::ZipDecompress {
                            zip_path: entry.path.clone(),
                            dest_dir: dest,
                        },
                    );
                } else if name_lower.ends_with(".tar.gz") || ext_lower == "tgz" || ext_lower == "tar" {
                    app.start_background_op(
                        ctx,
                        OpKind::TarDecompress {
                            tar_path: entry.path.clone(),
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
                format_name_list(&conflicts)
            ),
            action,
        });
    }
}

fn handle_edit_operations(app: &mut F2App, a: &ActionFlags) {
    // Rename
    if a.rename {
        if let Some(entry) = app.active_panel().current_entry() {
            let stem_len = if entry.is_dir {
                entry.name.chars().count()
            } else {
                std::path::Path::new(&entry.name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().chars().count())
                    .unwrap_or(entry.name.chars().count())
            };
            app.dialog.input = Some(InputDialog {
                title: "Rename".to_string(),
                value: entry.name.clone(),
                action: InputAction::Rename(entry.path.clone()),
                select_end: Some(stem_len),
            });
        }
    }

    // New directory
    if a.new_directory {
        app.dialog.input = Some(InputDialog {
            title: "New Directory".to_string(),
            value: String::new(),
            action: InputAction::NewDirectory,
            select_end: None,
        });
    }
}

fn handle_misc_keys(app: &mut F2App, ctx: &egui::Context, a: &ActionFlags) {
    // Refresh
    if a.refresh {
        app.active_panel_mut().refresh();
        app.status_message = "Refreshed".to_string();
    }

    // Quit
    if a.quit {
        app.save_config();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // Toggle hidden
    if a.toggle_hidden {
        let show = !app.active_panel().show_hidden;
        app.active_panel_mut().show_hidden = show;
        app.active_panel_mut().refresh();
    }

    // Toggle preview mode
    if a.toggle_preview {
        if app.preview_mode {
            app.preview_mode = false;
            app.clear_all_previews();
        } else {
            app.preview_mode = true;
            app.update_preview(ctx);
        }
    }

    // Sync opposite panel
    if a.sync_opposite_panel {
        let dir = app.active_panel().current_dir.clone();
        app.inactive_panel_mut().navigate_to(dir, ctx);
        app.status_message = "Synced opposite panel".to_string();
        app.save_config();
    }

    // Copy file path to clipboard
    if a.copy_path_clipboard {
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

    // Show help
    if a.show_help {
        app.dialog.message = Some(MessageDialog {
            title: "Keyboard Shortcuts".to_string(),
            message: app.keybindings.help_text(),
        });
    }

    // Focus filter
    if a.focus_filter {
        app.active_panel_mut().focus_filter = true;
    }

    // Recursive search toggle
    if a.recursive_search {
        let panel = app.active_panel_mut();
        if panel.recursive_filter {
            panel.exit_recursive_search();
        } else {
            panel.recursive_filter = true;
            panel.focus_filter = true;
            panel.start_recursive_search(ctx);
        }
    }

    // Exit recursive search mode (when filter doesn't have focus)
    if a.exit_recursive_search && app.active_panel().recursive_filter {
        app.active_panel_mut().exit_recursive_search();
    }

    // Drive selection
    if a.drive_select {
        use crate::file_ops::{get_drive_space, format_size_human};
        let drives_list = app.drives.lock().unwrap().clone();
        let drives = drives_list.iter().map(|name| {
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

    // Registered directories
    if a.registered_dirs {
        app.dialog.registered_dir = Some(RegisteredDirDialog {
            dirs: app.config.registered_dirs.clone(),
            cursor: 0,
        });
    }

    // Register current directory
    if a.register_dir {
        let dir = app.active_panel().current_dir.clone();
        let default_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.to_string_lossy().to_string());
        app.dialog.input = Some(InputDialog {
            title: "Register Directory".to_string(),
            value: default_name,
            action: InputAction::RegisterDirectory(dir),
            select_end: None,
        });
    }

    // Undo
    if a.undo {
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

    // Redo
    if a.redo {
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

    // Font size up/down
    if a.font_size_up || a.font_size_down {
        let current = app.config.font_size.unwrap_or(crate::app::DEFAULT_FONT_SIZE);
        let new_size = if a.font_size_up {
            (current + 1.0).min(40.0)
        } else {
            (current - 1.0).max(8.0)
        };
        app.config.font_size = Some(new_size);
        crate::app::apply_font_size(ctx, app.config.font_size);
        app.config.save();
        app.status_message = format!("Font size: {}", new_size);
    }

    // Settings
    if a.settings {
        let fonts = crate::dialog::enumerate_system_fonts();
        let current_font = app.config.font_path.as_ref()
            .and_then(|p| std::path::Path::new(p).file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "(Default)".to_string());
        app.dialog.settings = Some(SettingsDialog::new(fonts, current_font, &app.keybindings));
    }

    // Command mode
    if a.command_mode {
        app.command_mode = true;
        app.command_line.clear();
    }
}
