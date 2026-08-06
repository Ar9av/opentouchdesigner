//! Render a project headlessly to a PNG.
//!
//! ```text
//! cargo run -p otd-gpu --example render_png -- out.png [project.otd | starter | feedback] [frame]
//! cargo run -p otd-gpu --example render_png -- --save-starter path.otd
//! ```
//!
//! No window, no editor — the same cook engine driving an offscreen device.
//! This is the seed of the headless CLI runtime in PLAN.md Phase 5, the thing
//! TouchDesigner cannot do on a Linux server.

use otd_core::{CookContext, CookEngine, Project};
use otd_gpu::{GpuContext, TopEngine, demo, ops};

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out_path = args.next().unwrap_or_else(|| "otd-frame.png".to_string());
    let source = args.next();
    let registry = ops::registry();

    // `--save-starter path.otd` writes a built-in patch out as a project file,
    // which is the easiest way to see what the text format looks like without
    // launching the editor.
    if out_path == "--save-starter" || out_path == "--save-patch" {
        let path = source.unwrap_or_else(|| "starter.otd".to_string());
        let name = args.next().unwrap_or_else(|| "starter".to_string());
        let (graph, _) = demo::by_name(&name, &registry)
            .ok_or_else(|| format!("no built-in patch called `{name}`"))?;
        Project::from_graph(&graph, &registry, 60.0)
            .save(&path)
            .map_err(|e| e.to_string())?;
        println!("wrote {path}");
        return Ok(());
    }

    // Parsed after the save branch, which uses the same positional slot for
    // the patch name.
    let frame: i64 = args.next().and_then(|f| f.parse().ok()).unwrap_or(90);

    let (graph, viewer) = match source.as_deref() {
        None => demo::starter(&registry),
        Some(name) if demo::NAMES.contains(&name) => demo::by_name(name, &registry).unwrap(),
        Some(path) => {
            let graph = Project::load(path)
                .map_err(|e| e.to_string())?
                .to_graph(&registry)
                .map_err(|e| e.to_string())?;
            let viewer = graph
                .walk()
                .into_iter()
                .rfind(|id| graph.node(*id).op_type == ops::NULL)
                .ok_or("project has no Null TOP to render")?;
            (graph, viewer)
        }
    };

    let ctx = GpuContext::headless()?;
    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    // Step to the requested frame so animated branches — and anything with a
    // feedback loop, which only exists as an accumulation over frames — land
    // where they would in the editor rather than at t=0.
    for _ in 0..=frame {
        engine.begin_frame();
        cook.cook_frame(&graph, &[viewer], &time, &mut engine)
            .map_err(|e| e.to_string())?;
        engine.end_frame();
        time.advance(1.0 / 60.0);
    }

    for id in graph.walk() {
        if let Some(err) = engine.shader_error(id) {
            eprintln!("warning: {} shader error: {err}", graph.path(id));
        }
    }

    let tex = engine
        .output(&graph, viewer)
        .ok_or("viewer produced no texture")?
        .clone();
    let (w, h, pixels) = otd_gpu::read_pixels_rgba8(&ctx, &tex)?;
    image::save_buffer(&out_path, &pixels, w, h, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    println!("wrote {out_path} ({w}x{h}, frame {frame})");
    Ok(())
}
