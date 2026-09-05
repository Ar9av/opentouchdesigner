//! Every recipe builds, cleanly, against the real operator set.
//!
//! A recipe is three things at once — a template the editor applies, a worked
//! example the model is shown, and this. One that has rotted would be a
//! button that builds a broken patch and a lesson in how to build one, so
//! the bar is zero warnings, not "applies".

use otd_ai::patch;
use otd_ai::recipes::{self, Needs, SOURCE};
use otd_core::{Graph, Value};
use otd_engine::registry;

fn check_shader(source: &str, is_glsl: bool) -> Result<(), String> {
    if is_glsl {
        otd_gpu::shader::validate_glsl(&otd_gpu::shader::wrap_glsl(source))
    } else {
        otd_gpu::shader::validate_wgsl(&otd_gpu::shader::wrap_wgsl(source))
    }
}

#[test]
fn every_recipe_applies_with_nothing_skipped() {
    let reg = registry();
    let mut failures = Vec::new();
    for recipe in recipes::all() {
        let mut graph = Graph::new();
        let root = graph.root();
        if recipe.needs == Needs::Video {
            let movie = graph
                .create(root, reg.get("moviefileinTOP").unwrap(), Some(SOURCE))
                .unwrap();
            graph
                .set_param(movie, "file", Value::Str("clip.mp4".into()))
                .unwrap();
        }
        let plan = match recipe.plan(&reg) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("{}: does not parse: {e}", recipe.name));
                continue;
            }
        };
        let mut faults: Vec<String> = plan
            .warnings
            .iter()
            .map(|w| format!("parse: {w}"))
            .collect();

        if recipe.needs == Needs::Video {
            if !plan.connections.iter().any(|w| w.from == SOURCE) {
                faults.push(format!("needs video but never wires from {SOURCE}"));
            }
            if plan.nodes.iter().any(|n| n.name == SOURCE) {
                faults.push(format!("creates {SOURCE}, which the canvas supplies"));
            }
        }

        match patch::apply(&mut graph, root, &reg, &plan) {
            Ok((applied, viewer)) => {
                faults.extend(applied.warnings.iter().map(|w| format!("apply: {w}")));
                if viewer.is_none() {
                    faults.push("viewer names no node".into());
                }
                faults.extend(
                    patch::unresolved_refs(&graph, root, &applied.created)
                        .into_iter()
                        .map(|r| format!("dangling ref {r}")),
                );
            }
            Err(e) => faults.push(format!("apply failed: {e}")),
        }
        faults.extend(
            patch::dangling(&plan)
                .into_iter()
                .map(|n| format!("{n} is wired to nothing")),
        );
        faults.extend(
            patch::shader_problems(recipe.value(), check_shader)
                .into_iter()
                .map(|(node, e)| format!("{node}: shader does not compile: {e}")),
        );
        // The chips paste these into the box verbatim.
        if recipe.prompt.len() < 20 || recipe.prompt.ends_with('.') {
            faults.push("prompt does not read as a request".into());
        }
        if !faults.is_empty() {
            failures.push(format!("{}:\n    {}", recipe.name, faults.join("\n    ")));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn a_video_recipe_on_an_empty_canvas_gets_a_stand_in() {
    let reg = registry();
    let recipe = recipes::find("trails").unwrap();
    let mut plan = recipe.plan(&reg).unwrap();
    recipes::stand_in_source(&mut plan);
    let mut graph = Graph::new();
    let root = graph.root();
    let (applied, _) = patch::apply(&mut graph, root, &reg, &plan).unwrap();
    assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);
    assert!(applied.created.contains(&SOURCE.to_string()));
    // The stand-in is upstream of everything, not on top of it.
    let (lo, _) = plan.bounds().unwrap();
    assert_eq!(plan.nodes[0].pos[0], lo[0]);
}

#[test]
fn a_video_recipe_wires_from_the_node_it_is_given() {
    let reg = registry();
    let recipe = recipes::find("glitch").unwrap();
    let mut graph = Graph::new();
    let root = graph.root();
    let cam = graph
        .create(root, reg.get("videodeviceinTOP").unwrap(), Some("camera1"))
        .unwrap();
    let mut plan = recipe.plan(&reg).unwrap();
    recipes::with_source(&mut plan, "camera1");
    let (applied, _) = patch::apply(&mut graph, root, &reg, &plan).unwrap();
    assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);
    let glitch = graph.find_from(root, "glitch1").unwrap();
    assert_eq!(graph.node(glitch).inputs[0], Some(cam));
}

#[test]
fn a_feedback_target_follows_its_node_when_the_name_is_taken() {
    // The harness's clip patch, and most real ones, already end in `out1`.
    // The recipe's own `out1` is renamed on collision; the loop has to
    // follow it, or it closes on the old null and reads the raw clip.
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let movie = graph
        .create(root, reg.get("moviefileinTOP").unwrap(), Some("movie1"))
        .unwrap();
    let old_out = graph
        .create(root, reg.get("nullTOP").unwrap(), Some("out1"))
        .unwrap();
    graph.connect(movie, old_out, 0).unwrap();

    let mut plan = recipes::find("trails").unwrap().plan(&reg).unwrap();
    recipes::with_source(&mut plan, "movie1");
    let (applied, viewer) = patch::apply(&mut graph, root, &reg, &plan).unwrap();
    assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);

    let new_out = viewer.expect("the recipe's null is the viewer");
    assert_ne!(
        new_out, old_out,
        "the recipe's out1 should have been renamed"
    );
    let fb = graph.find_from(root, "fb1").unwrap();
    let target = graph.node(fb).param("target").unwrap().value.as_str();
    assert_eq!(
        graph.find_from(fb, target.trim()),
        Some(new_out),
        "fb1.target = {target:?} does not point at the recipe's own null"
    );
}
