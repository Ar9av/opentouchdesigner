//! The headless runtime, exercised as a user would: build a project, save it,
//! and run the actual binary against it.
//!
//! Testing the library functions directly would miss the things most likely to
//! be wrong about a CLI — that it opens the file, finds the node, writes where
//! it said it would, and fails loudly when it cannot.

use std::path::{Path, PathBuf};
use std::process::Command;

use otd_core::{Project, Value};

const EXE: &str = env!("CARGO_BIN_EXE_otd");

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("otd-cli-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A Noise -> Level chain at a small resolution, with the tail flagged for
/// render so the headless runtime has something to cook.
fn write_project(dir: &Path, flagged: bool) -> PathBuf {
    let registry = otd_engine::registry();
    let mut graph = otd_core::Graph::new();
    let root = graph.root();
    let noise = graph
        .create(root, registry.get("noiseTOP").unwrap(), Some("noise1"))
        .unwrap();
    let level = graph
        .create(root, registry.get("levelTOP").unwrap(), Some("level1"))
        .unwrap();
    graph.connect(noise, level, 0).unwrap();
    // A Level TOP takes its size from its input, so the resolution belongs on
    // the generator.
    graph.set_param(noise, "resw", Value::Int(64)).unwrap();
    graph.set_param(noise, "resh", Value::Int(32)).unwrap();

    // Start from no flags at all, so `flagged: false` really means nothing to
    // cook rather than whatever the defaults happen to be.
    for id in graph.walk() {
        graph.node_mut_quiet(id).flags = Default::default();
    }
    if flagged {
        graph.node_mut_quiet(level).flags.render = true;
    }

    let path = dir.join("project.otd");
    Project::from_graph(&graph, &registry, 60.0)
        .save(&path)
        .unwrap();
    path
}

fn otd(args: &[&str]) -> (bool, String) {
    let out = Command::new(EXE).args(args).output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

/// The GPU is not available everywhere CI runs; a machine without one should
/// skip rather than fail, exactly as the GPU tests do.
fn gpu_missing(text: &str) -> bool {
    if text.contains("no GPU") {
        eprintln!("skipping: {}", text.trim());
        return true;
    }
    false
}

#[test]
fn render_writes_a_numbered_png_sequence() {
    let dir = scratch("render");
    let project = write_project(&dir, true);
    let frames = dir.join("out");

    let (ok, text) = otd(&[
        "render",
        project.to_str().unwrap(),
        "--node",
        "/level1",
        "--frames",
        "3",
        "--out",
        frames.to_str().unwrap(),
    ]);
    if gpu_missing(&text) {
        return;
    }
    assert!(ok, "{text}");

    for i in 0..3 {
        let f = frames.join(format!("{i:05}.png"));
        assert!(f.exists(), "missing {}\n{text}", f.display());
        let img = image::open(&f).unwrap();
        assert_eq!(
            (img.width(), img.height()),
            (64, 32),
            "the file keeps the project's resolution, not a preview's"
        );
    }
    // Numbering starts at zero and stops where it was told to.
    assert!(!frames.join("00003.png").exists());
}

#[test]
fn a_node_named_on_the_command_line_is_rendered_even_if_nothing_is_flagged() {
    let dir = scratch("unflagged");
    let project = write_project(&dir, false);
    let frames = dir.join("out");

    // Naming the node *is* the statement of intent — requiring a flag as well
    // would mean editing the project to render it.
    let (ok, text) = otd(&[
        "render",
        project.to_str().unwrap(),
        "--node",
        "/level1",
        "--out",
        frames.to_str().unwrap(),
    ]);
    if gpu_missing(&text) {
        return;
    }
    assert!(ok, "{text}");
    assert!(frames.join("00000.png").exists(), "{text}");
}

#[test]
fn stats_reports_the_timings_and_whether_the_rate_holds() {
    let dir = scratch("stats");
    let project = write_project(&dir, true);

    let (ok, text) = otd(&["stats", project.to_str().unwrap(), "--frames", "20"]);
    if gpu_missing(&text) {
        return;
    }
    assert!(ok, "{text}");
    for wanted in ["frames", "first frame", "cook", "median", "budget"] {
        assert!(text.contains(wanted), "missing `{wanted}` in:\n{text}");
    }

    // "how long" is not actionable on its own — the report has to say which
    // nodes, and it has to name the per-frame cost separately from the cost
    // per cook, because in a demand-driven engine those are different numbers.
    for wanted in ["ms/frame", "per cook", "/level1", "/noise1"] {
        assert!(text.contains(wanted), "missing `{wanted}` in:\n{text}");
    }
}

#[test]
fn run_stops_after_the_frames_it_was_given() {
    let dir = scratch("run");
    let project = write_project(&dir, true);

    // Without --frames this runs until interrupted, which is the show-machine
    // case; with it, it has to actually stop.
    let (ok, text) = otd(&[
        "run",
        project.to_str().unwrap(),
        "--frames",
        "5",
        "--fps",
        "120",
    ]);
    if gpu_missing(&text) {
        return;
    }
    assert!(ok, "{text}");
    assert!(text.contains("5 frames"), "{text}");
}

#[test]
fn a_project_with_nothing_to_cook_says_so_instead_of_running_quietly() {
    let dir = scratch("nothing");
    let project = write_project(&dir, false);

    let (ok, text) = otd(&["run", project.to_str().unwrap(), "--frames", "1"]);
    if gpu_missing(&text) {
        return;
    }
    assert!(
        !ok,
        "an empty run should be an error, not a success:\n{text}"
    );
    assert!(text.contains("Render flag"), "{text}");
}

#[test]
fn a_bundle_still_runs_after_the_folder_is_moved() {
    let dir = scratch("bundle");
    let project = write_project(&dir, true);

    // Put a component somewhere the bundle is not, and reference it.
    let shared = scratch("bundle-shared");
    let registry = otd_engine::registry();
    let mut g = otd_core::Project::open(&project, &registry).unwrap();
    let comp = g
        .create(
            g.root(),
            registry.get("containerCOMP").unwrap(),
            Some("meter1"),
        )
        .unwrap();
    let otdc = shared.join("meter.otdc");
    otd_core::Component::from_graph(&g, comp, &registry)
        .unwrap()
        .save(&otdc)
        .unwrap();
    g.attach_external(comp, &otdc.to_string_lossy(), &registry)
        .unwrap();
    otd_core::Project::from_graph(&g, &registry, 60.0)
        .save(&project)
        .unwrap();

    let out = dir.join("bundle");
    let (ok, text) = otd(&[
        "bundle",
        project.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    assert!(out.join("components/meter.otdc").exists(), "{text}");

    // The two things that make it a bundle rather than a copy: the shared file
    // it was authored against is gone, and the folder is not where it was
    // written.
    std::fs::remove_dir_all(&shared).unwrap();
    let moved = scratch("bundle-moved");
    let dest = moved.join("show");
    std::fs::rename(&out, &dest).unwrap();

    let (ok, text) = otd(&[
        "run",
        dest.join("project.otd").to_str().unwrap(),
        "--frames",
        "2",
    ]);
    if gpu_missing(&text) {
        return;
    }
    assert!(ok, "a moved bundle must still open:\n{text}");
}

#[test]
fn bundling_a_project_whose_component_is_gone_fails_here_not_at_the_show() {
    let dir = scratch("bundle-broken");
    let project = write_project(&dir, true);

    // A reference to a file that no longer exists — the state a project gets
    // into after a folder is reorganised.
    let registry = otd_engine::registry();
    let mut p = otd_core::Project::load(&project).unwrap();
    p.nodes.push(otd_core::project::NodeEntry {
        path: "/meter1".into(),
        op: "containerCOMP".into(),
        pos: (0.0, 0.0),
        inputs: Vec::new(),
        params: Default::default(),
        flags: Default::default(),
        external: Some("/nowhere/gone.otdc".into()),
        clone: None,
    });
    p.nodes.sort_by(|a, b| a.path.cmp(&b.path));
    p.save(&project).unwrap();
    let _ = registry;

    let out = dir.join("bundle");
    let (ok, text) = otd(&[
        "bundle",
        project.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(!ok, "{text}");
    assert!(text.contains("gone.otdc"), "{text}");
}

#[test]
fn a_missing_file_and_a_missing_node_both_name_what_was_wrong() {
    let dir = scratch("missing");
    let project = write_project(&dir, true);

    let (ok, text) = otd(&["stats", "/nowhere/nothing.otd"]);
    assert!(!ok);
    assert!(text.contains("nothing.otd"), "{text}");

    let (ok, text) = otd(&["render", project.to_str().unwrap(), "--node", "/typo1"]);
    if gpu_missing(&text) {
        return;
    }
    assert!(!ok);
    assert!(text.contains("/typo1"), "{text}");
}
