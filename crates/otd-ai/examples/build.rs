//! Ask a provider for a patch and write it out as a project file.
//!
//!     cargo run -p otd-ai --example build -- openrouter "a slow blue tunnel" out.otd
//!
//! The point of it is the last step: the result is an ordinary `.otd` that
//! `otd render` will draw. A patch that parses but does not render is not a
//! patch, and this is how that claim gets checked rather than assumed.

use otd_ai::{Ask, Keys, Provider};
use otd_core::{Graph, Project};

/// The same compiler the editor checks with.
fn check_shader(source: &str, is_glsl: bool) -> Result<(), String> {
    if is_glsl {
        otd_gpu::shader::validate_glsl(&otd_gpu::shader::wrap_glsl(source))
    } else {
        otd_gpu::shader::validate_wgsl(&otd_gpu::shader::wrap_wgsl(source))
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: build <provider> <prompt> <out.otd> [model]");
        std::process::exit(2);
    }
    let Some(provider) = Provider::parse(&args[0]) else {
        eprintln!("unknown provider `{}`", args[0]);
        std::process::exit(2);
    };
    let model = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| provider.default_model().to_string());

    let keys = Keys::load();
    let registry = otd_engine::registry();
    let graph = Graph::new();
    let root = graph.root();

    let ask = Ask {
        provider,
        model: model.clone(),
        prompt: args[1].clone(),
        graph: &graph,
        parent: root,
        registry: &registry,
    };

    eprintln!("asking {} ({model})…", provider.label());
    let started = std::time::Instant::now();
    let key = keys.get(provider).cloned().unwrap_or_default();
    let request = otd_ai::request_for(&ask);
    let reply = match otd_ai::complete_with_repair(&request, &key, &keys, Some(check_shader)) {
        Ok(reply) => reply,
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    };
    if reply.repaired {
        eprintln!("  (a shader came back broken and was sent back to be fixed)");
    }
    let plan = match otd_ai::plan_from_reply(&reply.text, &registry) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    };
    if let Ok(json) = otd_ai::patch::extract_json(&reply.text) {
        for (node, error) in otd_ai::patch::shader_problems(&json, check_shader) {
            eprintln!("  STILL BROKEN {node}: {error}");
        }
    }
    for loose in otd_ai::patch::dangling(&plan) {
        eprintln!("  loose (wired to nothing): {loose}");
    }
    eprintln!("{:.1}s — {}", started.elapsed().as_secs_f64(), plan.notes);

    let mut graph = Graph::new();
    let root = graph.root();
    let (applied, viewer) = otd_ai::patch::apply(&mut graph, root, &registry, &plan).unwrap();
    for warning in &applied.warnings {
        eprintln!("  skipped: {warning}");
    }
    if let Some(viewer) = viewer {
        eprintln!("  viewer: {}", graph.path(viewer));
    }
    eprintln!(
        "  {} node(s), {} wire(s)",
        applied.created.len(),
        applied.wired
    );

    let project = Project::from_graph(&graph, &registry, 60.0);
    project.save(&args[2]).expect("write the project");
    println!("{}", args[2]);
}
