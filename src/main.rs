mod app;
mod config;
mod dialog;
mod file_item;
mod file_ops;
mod panel;
mod sort;
mod image_viewer;
mod viewer;

use app::F2App;
use config::Config;

fn main() -> eframe::Result<()> {
    let config = Config::load();

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_min_inner_size([800.0, 400.0])
        .with_title("f2filer");

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
