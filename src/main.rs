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

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 700.0])
            .with_min_inner_size([800.0, 400.0])
            .with_title("f2filer"),
        ..Default::default()
    };

    eframe::run_native(
        "f2filer",
        options,
        Box::new(|cc| Ok(Box::new(F2App::new(cc)))),
    )
}
