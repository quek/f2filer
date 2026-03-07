#![windows_subsystem = "windows"]

mod app;
mod config;
mod dialog;
mod dialog_handler;
#[cfg(windows)]
mod drag_drop;
mod file_item;
mod file_ops;
mod keyboard;
mod panel;
#[cfg(windows)]
mod shell;
mod sort;
mod undo;
mod audio_viewer;
mod image_viewer;
mod video_viewer;
mod viewer;

use app::F2App;
use config::Config;

fn load_icon() -> eframe::egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon.png");
    let img = image::load_from_memory(icon_bytes)
        .expect("Failed to load icon")
        .into_rgba8();
    let (w, h) = img.dimensions();
    eframe::egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    }
}

fn main() -> eframe::Result<()> {
    let config = Config::load();

    let icon = load_icon();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_min_inner_size([800.0, 400.0])
        .with_title("f2filer")
        .with_icon(icon)
        .with_drag_and_drop(true);

    // Restore window size
    let width = config.window_width.unwrap_or(1200.0);
    let height = config.window_height.unwrap_or(700.0);
    viewport = viewport.with_inner_size([width, height]);

    // Restore window position
    if let (Some(x), Some(y)) = (config.window_x, config.window_y) {
        viewport = viewport.with_position([x, y]);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "f2filer",
        options,
        Box::new(|cc| Ok(Box::new(F2App::new(cc)))),
    )
}
