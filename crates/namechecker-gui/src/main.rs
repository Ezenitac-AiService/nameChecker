//! Desktop GUI entrypoint for `namechecker-gui`.

use namechecker_gui::app::NameCheckerApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "nameChecker - CJK Filename & Collation Inspector",
        options,
        Box::new(|cc| Ok(Box::new(NameCheckerApp::new(cc)))),
    )
}
