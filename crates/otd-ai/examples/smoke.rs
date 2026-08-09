//! A live call to whichever providers have a key in the environment.
//!
//! Not a test: a test that needs a network, a key and somebody's money is a
//! test that fails on a laptop in a train. This is the thing you run by hand
//! when you want to know whether the wire format is still right.
//!
//!     cargo run -p otd-ai --example smoke
//!
//! It prints what came back, never what was sent as a credential.

use otd_ai::{Ask, Keys, Provider, patch};
use otd_core::{Graph, OpRegistry};

fn registry() -> OpRegistry {
    // The real operator set, so the catalogue is the real catalogue.
    otd_engine_registry()
}

fn otd_engine_registry() -> OpRegistry {
    // otd-ai does not depend on otd-engine (that would drag in the GPU), so
    // the example asks for it through the CLI's dependency graph instead.
    otd_engine::registry()
}

fn main() {
    let keys = Keys::load();
    let registry = registry();
    let graph = Graph::new();
    let root = graph.root();

    println!(
        "catalogue: {} chars, {} operators",
        patch::catalogue(&registry).len(),
        registry.iter().count()
    );

    let prompt = "A slow feedback tunnel: contrast a monochrome noise field \
                  down to bright wisps, tint it teal-to-magenta through a ramp, \
                  and loop it with a slight zoom and rotate.";

    for provider in Provider::ALL {
        let Some(key) = keys.get(*provider) else {
            println!("\n{}: no key, skipping", provider.label());
            continue;
        };
        println!(
            "\n{} ({}) key {}",
            provider.label(),
            provider.default_model(),
            key.hint()
        );

        let ask = Ask {
            provider: *provider,
            model: provider.default_model().to_string(),
            prompt: prompt.to_string(),
            image: None,
            graph: &graph,
            parent: root,
            registry: &registry,
        };
        let started = std::time::Instant::now();
        match otd_ai::ask(&ask, &keys) {
            Ok(plan) => {
                println!(
                    "  ok in {:.1}s — {} nodes, {} wires, viewer {:?}",
                    started.elapsed().as_secs_f64(),
                    plan.nodes.len(),
                    plan.connections.len(),
                    plan.viewer
                );
                println!("  notes: {}", plan.notes);
                for node in &plan.nodes {
                    println!(
                        "    {} ({}) params {:?} expr {:?}",
                        node.name,
                        node.op,
                        node.params.keys().collect::<Vec<_>>(),
                        node.expressions.keys().collect::<Vec<_>>()
                    );
                }
                // And prove it is not just well-formed but buildable.
                let mut g = Graph::new();
                let r = g.root();
                match patch::apply(&mut g, r, &registry, &plan) {
                    Ok((applied, viewer)) => println!(
                        "  applied: {} created, {} wired, viewer {}, warnings {:?}",
                        applied.created.len(),
                        applied.wired,
                        viewer.is_some(),
                        applied.warnings
                    ),
                    Err(e) => println!("  apply failed: {e}"),
                }
            }
            Err(e) => println!("  failed in {:.1}s: {e}", started.elapsed().as_secs_f64()),
        }
    }
}
