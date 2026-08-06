//! Render a project headlessly to a PNG.
//!
//! ```text
//! cargo run -p otd-engine --example render_png -- out.png [project.otd | starter | feedback | audioreactive] [frame]
//! cargo run -p otd-engine --example render_png -- --save-patch path.otd <patch-name>
//! ```
//!
//! No window, no editor — the same cook engine and the same operators,
//! driving an offscreen device. This is the seed of the headless CLI runtime
//! in PLAN.md Phase 5, the thing TouchDesigner cannot do on a Linux server.
//!
//! It cooks both families, so a patch whose visual is driven by CHOPs renders
//! exactly as it would in the editor — minus whatever a device would have
//! been feeding it.

use otd_core::{CookContext, CookEngine, Project};
use otd_engine::{Engines, demo, registry};
use otd_gpu::{GpuContext, ops};

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out_path = args.next().unwrap_or_else(|| "otd-frame.png".to_string());
    let source = args.next();
    let registry = registry();

    // `--save-patch <file> <name>` writes a built-in patch out as a project
    // file, the easiest way to see the text format without the editor.
    if out_path == "--save-patch" || out_path == "--save-starter" {
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
    let mut engines = Engines::new(ctx.clone());
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    // Step to the requested frame so animated branches — and anything with a
    // feedback loop or a CHOP integrating over time, which only exists as an
    // accumulation over frames — land where they would in the editor.
    for _ in 0..=frame {
        engines.begin_frame();
        cook.cook_frame(&graph, &[viewer], &time, &mut engines)
            .map_err(|e| e.to_string())?;
        engines.end_frame();
        time.advance(1.0 / 60.0);
    }

    for id in graph.walk() {
        if let Some(status) = engines.node_status(&graph, id) {
            eprintln!("warning: {}: {status}", graph.path(id));
        }
    }

    let tex = engines
        .top
        .output(&graph, viewer)
        .ok_or("viewer produced no texture")?
        .clone();
    let (w, h, pixels) = otd_gpu::read_pixels_rgba8(&ctx, &tex)?;
    image::save_buffer(&out_path, &pixels, w, h, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    println!("wrote {out_path} ({w}x{h}, frame {frame})");
    Ok(())
}
