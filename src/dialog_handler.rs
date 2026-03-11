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
            ConfirmAction::ZipCompressOverwrite { sources, dest_dir, zip_name } => {
                app.start_background_op(
                    ctx,
                    OpKind::ZipCompress {
                        sources,
                        dest_dir,
                        zip_name,
                    },
                );
            }
            ConfirmAction::ZipDecompressOverwrite { zip_path, dest_dir } => {
                app.start_background_op(
                    ctx,
                    OpKind::ZipDecompress {
                        zip_path,
                        dest_dir,
                    },
                );
            }
            ConfirmAction::TarDecompressOverwrite { tar_path, dest_dir } => {
                app.start_background_op(
                    ctx,
                    OpKind::TarDecompress {
                        tar_path,
                        dest_dir,
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
                            app.set_status(format!("Renamed to {}", value));
                            app.undo_history.push(FileOperation::Rename {
                                old_path,
                                new_path,
                            });
                            app.active_panel_mut().refresh();
                        }
                        Err(e) => {
                            app.set_status_error(format!("Rename error: {}", e));
                        }
                    }
                }
                InputAction::NewDirectory => {
                    let dir = app.active_panel().current_dir.clone();
                    match file_ops::create_directory(&dir, &value) {
                        Ok(path) => {
                            app.set_status(format!("Created directory: {}", value));
                            app.undo_history.push(FileOperation::CreateDir { path });
                            app.active_panel_mut().refresh();
                        }
                        Err(e) => {
                            app.set_status_error(format!("Error: {}", e));
                        }
                    }
                }
                InputAction::NewFile => {
                    let dir = app.active_panel().current_dir.clone();
                    match file_ops::create_file(&dir, &value) {
                        Ok(path) => {
                            app.set_status(format!("Created file: {}", value));
                            app.undo_history.push(FileOperation::CreateFile { path });
                            app.active_panel_mut().refresh();
                        }
                        Err(e) => {
                            app.set_status_error(format!("Error: {}", e));
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
                        select_end: None,
                    });
                }
                InputAction::RegisterDirectoryKey { path, name } => {
                    let key = crate::app::first_char_upper(&value, '?');
                    let path_str = path.to_string_lossy().to_string();
                    app.set_status(format!("Registered: [{}] {}", key, name));
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
                        app.set_status(format!("Changed key for \"{}\": [{}]", name, new_key));
                    }
                }
                InputAction::ZipCompress(sources) => {
                    let dest = app.inactive_panel().current_dir.clone();
                    let zip_name = if value.ends_with(".zip") {
                        value.clone()
                    } else {
                        format!("{}.zip", value)
                    };
                    let zip_path = dest.join(&zip_name);
                    if zip_path.exists() {
                        app.dialog.confirm = Some(ConfirmDialog {
                            title: "Overwrite?".to_string(),
                            message: format!(
                                "\"{}\" already exists.\n\nOverwrite?",
                                zip_name
                            ),
                            action: ConfirmAction::ZipCompressOverwrite {
                                sources,
                                dest_dir: dest,
                                zip_name: value,
                            },
                        });
                    } else {
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
        }
        DialogResult::DriveSelected(drive) => {
            let saved_dir = app.config.drive_dirs.get(&drive).cloned();
            let drive_clone = drive.clone();
            // Show placeholder path immediately while resolving in background
            let placeholder = if drive.starts_with("WSL:") || drive.starts_with(r"\\") {
                PathBuf::from(&drive)
            } else {
                PathBuf::from(format!("{}\\", drive))
            };
            app.active_panel_mut().navigate_to_with_resolver(
                placeholder,
                move || crate::app::resolve_drive_path_bg(saved_dir, &drive_clone),
                ctx,
            );
        }
        DialogResult::RegisteredDirSelected(path_str) => {
            let path = PathBuf::from(&path_str);
            if path.exists() {
                app.active_panel_mut().navigate_to(path, ctx);
                app.save_config();
                app.set_status(format!("Jumped to {}", path_str));
            } else {
                app.set_status_error(format!("Directory not found: {}", path_str));
            }
        }
        DialogResult::RegisteredDirDeleted(idx) => {
            if idx < app.config.registered_dirs.len() {
                let removed = app.config.registered_dirs.remove(idx);
                app.config.save();
                app.set_status(format!("Unregistered: {}", removed.name));
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
                    select_end: None,
                });
            }
        }
        DialogResult::HistorySelected(index) => {
            app.active_panel_mut().go_back_to(index, ctx);
        }
        DialogResult::KeybindingChanged(action, bindings) => {
            // Update keybindings in app
            app.keybindings.bindings.insert(action, bindings.clone());
            // Save override to config
            let overrides = app.config.keybindings_override.get_or_insert_with(std::collections::HashMap::new);
            overrides.insert(action, bindings);
            app.config.save();
            app.set_status(format!("Updated keybinding: {}", action.description()));
        }
        DialogResult::KeybindingBatchChanged(changes) => {
            // Apply all changes (conflict resolution + target update)
            let overrides = app.config.keybindings_override.get_or_insert_with(std::collections::HashMap::new);
            let mut last_action_desc = String::new();
            for (action, bindings) in changes {
                app.keybindings.bindings.insert(action, bindings.clone());
                overrides.insert(action, bindings);
                last_action_desc = action.description().to_string();
            }
            app.config.save();
            app.set_status(format!("Updated keybinding: {}", last_action_desc));
        }
        DialogResult::KeybindingReset(action) => {
            // Reset to default
            let defaults = crate::keybind::KeyBindings::defaults();
            if let Some(default_bindings) = defaults.bindings.get(&action) {
                app.keybindings.bindings.insert(action, default_bindings.clone());
            }
            // Remove from overrides
            if let Some(overrides) = &mut app.config.keybindings_override {
                overrides.remove(&action);
                if overrides.is_empty() {
                    app.config.keybindings_override = None;
                }
            }
            app.config.save();
            app.set_status(format!("Reset keybinding: {}", action.description()));
        }
        DialogResult::FontSelected(font_path) => {
            app.config.font_path = font_path.clone();
            crate::app::setup_fonts(ctx, app.config.font_path.as_deref(), app.config.font_size);
            app.config.save();
            let name = font_path
                .as_ref()
                .and_then(|p| {
                    std::path::Path::new(p)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                })
                .unwrap_or_else(|| "(Default)".to_string());
            app.set_status(format!("Font: {}", name));
        }
        DialogResult::ProgressFinished => {
            if let Some(progress_dialog) = app.dialog.progress.take() {
                let (result_message, succeeded_paths, result_path, has_error) = {
                    let s = progress_dialog.handle.state.lock();
                    (
                        s.result_message.clone(),
                        s.succeeded_paths.clone(),
                        s.result_path.clone(),
                        s.error.is_some(),
                    )
                };

                if has_error {
                    app.set_status_error(result_message);
                } else {
                    app.set_status(result_message);
                }

                // For delete/move: remove operated files from recursive search results
                if matches!(
                    &progress_dialog.op_kind,
                    OpKind::Delete { .. } | OpKind::DeletePermanent { .. } | OpKind::Move { .. }
                ) {
                    app.left_panel.remove_paths(&succeeded_paths);
                    app.right_panel.remove_paths(&succeeded_paths);
                }

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
                        OpKind::TarDecompress { tar_path, .. } => {
                            if let Some(extracted_dir) = result_path {
                                app.undo_history.push(FileOperation::Decompress {
                                    zip_path: tar_path.clone(),
                                    extracted_dir,
                                });
                            }
                        }
                    }
                }

                app.left_panel.refresh();
                app.right_panel.refresh();
                app.active_panel_mut().deselect_all();
                if app.preview_mode {
                    app.update_preview(ctx);
                }
            }
        }
        _ => {}
    }
}
