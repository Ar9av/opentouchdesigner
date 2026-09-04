//! OpenTouchDesigner — the editor shell.

mod app;
mod assistant;
mod canvas;
mod media;
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

    // A project named on the command line opens instead of the starter patch,
    // so a `.otd` can be handed to the editor the way any other file is —
    // from a shell, from a script, from a "reopen this and look at it".
    let project = std::env::args().nth(1).map(std::path::PathBuf::from);

    eframe::run_native(
        "OpenTouchDesigner",
        options,
        Box::new(|cc| {
            let mut app = app::OtdApp::new(cc)?;
            if let Some(path) = project {
                app.open(Some(path));
            }
            Ok(Box::new(app))
        }),
    )
}
