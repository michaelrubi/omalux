mod app;
mod commands;
mod histogram;
mod hotkeys;
mod theme;

use std::path::PathBuf;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,omalux=info")).init();

    // `omalux <folder>` (Omacull's Ctrl+E), or `omalux <raw>` to start at
    // that frame.
    let open = std::env::args_os().nth(1).map(PathBuf::from);

    // The develop pipeline runs on the GPU, so unlike Omacull this wants
    // the discrete one (wgpu's default high-performance choice).
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("omalux")
            .with_title("OmaLux")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([480.0, 320.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native("omalux", options, Box::new(|cc| Ok(Box::new(app::App::new(cc, open)))))
}
