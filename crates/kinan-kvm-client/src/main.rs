#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod config;
mod i18n;
mod kinan_web;
mod profiles;
mod session;
mod theme;
mod ui;

use crate::ui::App;

fn main() -> Result<(), eframe::Error> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([960.0, 640.0])
            .with_title("Kinan KVM Client"),
        ..Default::default()
    };
    eframe::run_native(
        "Kinan KVM Client",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
