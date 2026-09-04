//! Render a project headlessly to a PNG.
//!
//! ```text
//! cargo run -p otd-gpu --example render_png -- out.png [project.otd] [frame]
//! ```
//!
//! No window, no editor — the same cook engine driving an offscreen device.
//! This is the seed of the headless CLI runtime in PLAN.md Phase 5, the thing
//! TouchDesigner cannot do on a Linux server.

use otd_core::{CookContext, CookEngine, Graph, NodeId, Project, Value};
use otd_gpu::{GpuContext, TopEngine, ops};

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out_path = args.next().unwrap_or_else(|| "otd-frame.png".to_string());
    let project_path = args.next();
    let frame: i64 = args.next().and_then(|f| f.parse().ok()).unwrap_or(90);

    let registry = ops::registry();

    // `render_png --save-starter path.otd` writes the built-in patch out as a
    // project file, which is the easiest way to see what the text format
    // looks like without launching the editor.
    if out_path == "--save-starter" {
        let path = project_path.unwrap_or_else(|| "starter.otd".to_string());
        let (graph, _) = starter_patch(&registry);
        Project::from_graph(&graph, &registry, 60.0)
            .save(&path)
            .map_err(|e| e.to_string())?;
        println!("wrote {path}");
        return Ok(());
    }

    let (graph, viewer) = match &project_path {
        Some(p) => {
            let graph = Project::load(p)
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
        None => starter_patch(&registry),
    };

    let ctx = GpuContext::headless()?;
    let mut engine = TopEngine::new(ctx.clone());
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    // Step to the requested frame so animated branches land where they would
    // in the editor, rather than at t=0.
    for _ in 0..=frame {
        engine.begin_frame();
        cook.cook_frame(&graph, &[viewer], &time, &mut engine)
            .map_err(|e| e.to_string())?;
        engine.end_frame();
        time.advance(1.0 / 60.0);
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

/// The same patch the editor opens with.
fn starter_patch(registry: &otd_core::OpRegistry) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();
    let noise = graph
        .create(root, registry.get("noiseTOP").unwrap(), None)
        .unwrap();
    let level = graph
        .create(root, registry.get("levelTOP").unwrap(), None)
        .unwrap();
    let out = graph
        .create(root, registry.get(ops::NULL).unwrap(), None)
        .unwrap();
    graph.connect(noise, level, 0).unwrap();
    graph.connect(level, out, 0).unwrap();
    graph.set_param(noise, "resw", Value::Int(1280)).unwrap();
    graph.set_param(noise, "resh", Value::Int(720)).unwrap();
    graph
        .set_expression(noise, "translate", "absTime * 0.15")
        .unwrap();
    graph
        .set_expression(level, "contrast", "1.5 + sin(absTime) * 0.5")
        .unwrap();
    (graph, out)
}
