//! OpenTouchDesigner — the editor shell.

mod app;
mod canvas;
mod params;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 980.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("OpenTouchDesigner"),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "OpenTouchDesigner",
        options,
        Box::new(|cc| Ok(Box::new(app::OtdApp::new(cc)?))),
    )
}
