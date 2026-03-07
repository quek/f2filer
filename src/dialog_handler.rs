use std::path::PathBuf;

use eframe::egui;

use crate::app::F2App;
use crate::dialog::*;
use crate::file_ops;
use crate::undo::FileOperation;

pub(crate) fn handle_dialog_result(app: &mut F2App, ctx: &egui::Context, result: DialogResult) {
    match result {
        DialogResult::ConfirmYes(action) => match action {
            ConfirmAction::Delete(paths) => {
                app.start_background_op(ctx, OpKind::Delete { paths });
            }
            ConfirmAction::DeletePermanent(paths) => {
                app.start_background_op(ctx, OpKind::DeletePermanent { paths });
            }
            ConfirmAction::CopyOverwrite { sources, dest } => {
                app.start_background_op(
                    ctx,
                    OpKind::Copy {
                        sources,
                        dest_dir: dest,
                        overwrite: true,
                    },
                );
            }
            ConfirmAction::MoveOverwrite { sources, dest } => {
                app.start_background_op(
                    ctx,
                    OpKind::Move {
                        sources,
                        dest_dir: dest,
                        overwrite: true,
                    },
                );
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
                            app.status_message = format!("Renamed to {}", value);
                            app.undo_history.push(FileOperation::Rename {
                                old_path,
                                new_path,
                            });
                            app.active_panel_mut().refresh();
                        }
                        Err(e) => {
                            app.status_message = format!("Rename error: {}", e);
                        }
                    }
                }
                InputAction::NewDirectory => {
                    let dir = app.active_panel().current_dir.clone();
                    match file_ops::create_directory(&dir, &value) {
                        Ok(path) => {
                            app.status_message = format!("Created directory: {}", value);
                            app.undo_history.push(FileOperation::CreateDir { path });
                            app.active_panel_mut().refresh();
                        }
                        Err(e) => {
                            app.status_message = format!("Error: {}", e);
                        }
                    }
                }
                InputAction::RegisterDirectory(path) => {
                    // Step 2: ask for shortcut key (default: first char of name)
                    let default_key = crate::app::first_char_upper(&value, 'A');
                    app.dialog.input = Some(InputDialog {
                        title: format!("Shortcut Key for \"{}\"", value),
                        value: default_key,
                        action: InputAction::RegisterDirectoryKey {
                            path,
                            name: value,
                        },
                    });
                }
                InputAction::RegisterDirectoryKey { path, name } => {
                    let key = crate::app::first_char_upper(&value, '?');
                    let path_str = path.to_string_lossy().to_string();
                    app.status_message = format!("Registered: [{}] {}", key, name);
                    app.config.registered_dirs.push(crate::config::RegisteredDir {
                        key,
                        name,
                        path: path_str,
                    });
                    app.config.save();
                }
                InputAction::EditRegisteredDirKey(idx) => {
                    let new_key = crate::app::first_char_upper(&value, '?');
                    if idx < app.config.registered_dirs.len() {
                        let name = app.config.registered_dirs[idx].name.clone();
                        app.config.registered_dirs[idx].key = new_key.clone();
                        app.config.save();
                        app.status_message =
                            format!("Changed key for \"{}\": [{}]", name, new_key);
                    }
                }
                InputAction::ZipCompress(sources) => {
                    let dest = app.inactive_panel().current_dir.clone();
                    app.start_background_op(
                        ctx,
                        OpKind::ZipCompress {
                            sources,
                            dest_dir: dest,
                            zip_name: value,
                        },
                    );
                }
            }
        }
        DialogResult::DriveSelected(drive) => {
            let path = app.resolve_drive_path(&drive);
            if path.exists() {
                app.active_panel_mut().navigate_to(path);
                app.save_config();
            }
        }
        DialogResult::RegisteredDirSelected(path_str) => {
            let path = PathBuf::from(&path_str);
            if path.exists() {
                app.active_panel_mut().navigate_to(path);
                app.save_config();
                app.status_message = format!("Jumped to {}", path_str);
            } else {
                app.status_message = format!("Directory not found: {}", path_str);
            }
        }
        DialogResult::RegisteredDirDeleted(idx) => {
            if idx < app.config.registered_dirs.len() {
                let removed = app.config.registered_dirs.remove(idx);
                app.config.save();
                app.status_message = format!("Unregistered: {}", removed.name);
            }
        }
        DialogResult::RegisteredDirEditKey(idx) => {
            if idx < app.config.registered_dirs.len() {
                let current_key = app.config.registered_dirs[idx].key.clone();
                app.dialog.input = Some(InputDialog {
                    title: format!(
                        "Change Key for \"{}\"",
                        app.config.registered_dirs[idx].name
                    ),
                    value: current_key,
                    action: InputAction::EditRegisteredDirKey(idx),
                });
            }
        }
        DialogResult::ProgressFinished => {
            if let Some(progress_dialog) = app.dialog.progress.take() {
                let state = progress_dialog.handle.state.lock().ok();
                let (result_message, succeeded_paths, result_path) = match &state {
                    Some(s) => (
                        s.result_message.clone(),
                        s.succeeded_paths.clone(),
                        s.result_path.clone(),
                    ),
                    None => (
                        "Operation failed (mutex poisoned)".to_string(),
                        Vec::new(),
                        None,
                    ),
                };
                drop(state);

                app.status_message = result_message;

                if !succeeded_paths.is_empty() {
                    match &progress_dialog.op_kind {
                        OpKind::Copy { dest_dir, .. } => {
                            let created: Vec<PathBuf> = succeeded_paths
                                .iter()
                                .filter_map(|s| s.file_name().map(|n| dest_dir.join(n)))
                                .collect();
                            app.undo_history.push(FileOperation::Copy {
                                sources: succeeded_paths,
                                dest_dir: dest_dir.clone(),
                                created,
                            });
                        }
                        OpKind::Move { dest_dir, .. } => {
                            let moves: Vec<(PathBuf, PathBuf)> = succeeded_paths
                                .iter()
                                .filter_map(|s| {
                                    s.file_name().map(|n| (s.clone(), dest_dir.join(n)))
                                })
                                .collect();
                            app.undo_history.push(FileOperation::Move { moves });
                        }
                        OpKind::Delete { .. } => {
                            app.undo_history.push(FileOperation::Delete {
                                paths: succeeded_paths,
                            });
                        }
                        OpKind::DeletePermanent { .. } => {
                            // No undo for permanent delete
                        }
                        OpKind::ZipCompress { .. } => {
                            if let Some(zip_path) = result_path {
                                app.undo_history.push(FileOperation::Compress {
                                    sources: succeeded_paths,
                                    zip_path,
                                });
                            }
                        }
                        OpKind::ZipDecompress { zip_path, .. } => {
                            if let Some(extracted_dir) = result_path {
                                app.undo_history.push(FileOperation::Decompress {
                                    zip_path: zip_path.clone(),
                                    extracted_dir,
                                });
                            }
                        }
                    }
                }

                app.left_panel.refresh();
                app.right_panel.refresh();
                app.active_panel_mut().deselect_all();
            }
        }
        _ => {}
    }
}
