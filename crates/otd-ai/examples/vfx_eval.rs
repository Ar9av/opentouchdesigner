//! Does the assistant actually produce a working video effect?
//!
//! `scenarios` answers "did the plan apply" — how many nodes, which families,
//! what got skipped. That is necessary and it is not the question anybody
//! actually has, because every failure this project has hit lately applied
//! cleanly and looked fine in that report: the shader that would not compile,
//! the camera that was never opened, the render pointed at a bare name. All
//! of them "8 nodes, 7 wires, no warnings", and all of them black.
//!
//! So this one cooks the result on a real GPU against a real clip and looks
//! at the pixels:
//!
//!   * **Is it black?** Mean luminance of the last frame. The single most
//!     common way a generated patch fails.
//!   * **Does it move?** Mean absolute difference between two frames a few
//!     apart. A patch that is a still image when it was asked for an effect
//!     has usually lost the clip and replaced it with a constant.
//!   * **Is it an effect at all?** Difference from the source clip. A chain
//!     that passes the picture through untouched is not what was asked for
//!     either, and reads as success everywhere else.
//!   * **Did anything break?** Shader errors off the engine, dangling path
//!     references, and every warning `apply` raised.
//!
//!     cargo run -p otd-ai --example vfx_eval
//!     cargo run -p otd-ai --example vfx_eval -- --clip /path/to.mov
//!     cargo run -p otd-ai --example vfx_eval -- --only glitch,smoke
//!     cargo run -p otd-ai --example vfx_eval -- --provider anthropic --repeat 2
//!     cargo run -p otd-ai --example vfx_eval -- --save /tmp/out
//!     cargo run -p otd-ai --example vfx_eval -- --recipes
//!
//! `--recipes` cooks the shipped recipes instead of asking a model: no key,
//! no money, and the same ruler. They are the ground truth the model's plans
//! are measured against, and one that fails here is a template that builds
//! a broken patch.
//!
//! `--save` writes each patch as a `.otd`, so anything that scores badly can
//! be opened in the editor and looked at rather than guessed about.

use otd_ai::{Ask, Keys, Provider, patch};
use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Project, Value};
use otd_engine::{Engines, registry};
use otd_gpu::GpuContext;

/// The things people ask a node graph for, in the words they ask in.
///
/// Drawn from what the community actually shows off — glitch, feedback
/// trails, kaleidoscope, cel shading, dithering, cellular patterns, smoke —
/// and phrased the way somebody types rather than the way an operator is
/// named, because a prompt that names the operator is not testing anything.
const PROMPTS: &[(&str, &str)] = &[
    (
        "glitch",
        "make the video glitch out with rgb split and scanlines",
    ),
    (
        "trails",
        "give the video long smeary trails that echo behind the movement",
    ),
    (
        "kaleido",
        "turn the video into a kaleidoscope that slowly rotates",
    ),
    (
        "retro",
        "make it look like an old 1-bit computer, dithered black and white",
    ),
    (
        "toon",
        "make the video look like a cartoon with flat colours and black outlines",
    ),
    ("smoke", "make the video dissolve into drifting smoke"),
    ("cells", "break the video up into cracked glass cells"),
    (
        "displace",
        "displace the video with a noise pattern so it ripples",
    ),
    (
        "neon",
        "find the edges of the video and make them glow neon over a dark background",
    ),
    (
        "psychedelic",
        "make it properly psychedelic — colour cycling, feedback, the works",
    ),
];

struct Measured {
    luma: f64,
    motion: f64,
    from_source: f64,
}

fn main() {
    let keys = Keys::load();
    let reg = registry();

    let mut provider = Provider::ClaudeCode;
    let mut clip = String::from("/Users/ar9av/Downloads/0BF6E6AD-6C60-4D61-B765-7B355B75B8E3.MP4");
    let mut only: Vec<String> = Vec::new();
    let mut repeat = 1usize;
    let mut save: Option<String> = None;
    let mut use_recipes = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--provider" => {
                provider = args
                    .next()
                    .and_then(|n| Provider::parse(&n))
                    .unwrap_or(provider)
            }
            "--clip" => clip = args.next().unwrap_or(clip),
            "--only" => {
                only = args
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect()
            }
            "--repeat" => repeat = args.next().and_then(|n| n.parse().ok()).unwrap_or(1),
            "--save" => save = args.next(),
            "--recipes" => use_recipes = true,
            other => eprintln!("ignoring `{other}`"),
        }
    }

    if !std::path::Path::new(&clip).exists() {
        eprintln!("no clip at {clip} — pass --clip");
        std::process::exit(1);
    }
    let Ok(gpu) = GpuContext::headless() else {
        eprintln!("no GPU available; this example needs one");
        std::process::exit(1);
    };
    if let Some(dir) = &save {
        let _ = std::fs::create_dir_all(dir);
    }

    if use_recipes {
        println!(
            "recipes:   {} shipped, no model",
            otd_ai::recipes::all().len()
        );
    } else {
        println!(
            "assistant: {} ({})",
            provider.label(),
            provider.default_model()
        );
    }
    println!("clip:      {clip}\n");
    println!(
        "{:<11} {:>5} {:>6} {:>7} {:>7}  {}",
        "prompt", "nodes", "luma", "motion", "vs src", "verdict"
    );
    println!("{}", "-".repeat(78));

    let key = keys
        .get(provider)
        .cloned()
        .unwrap_or_else(|| otd_ai::Key::new(""));
    let mut passes = 0usize;
    let mut runs = 0usize;

    // Either the prompts, answered by a model, or the recipes, answered by
    // themselves. Same columns, same verdicts.
    let cases: Vec<(&str, &str, Option<&otd_ai::recipes::Recipe>)> = if use_recipes {
        otd_ai::recipes::all()
            .iter()
            .map(|r| (r.name.as_str(), r.prompt.as_str(), Some(r)))
            .collect()
    } else {
        PROMPTS.iter().map(|(n, p)| (*n, *p, None)).collect()
    };
    let repeat = if use_recipes { 1 } else { repeat };

    for (name, prompt, recipe) in cases {
        if !only.is_empty() && !only.iter().any(|n| n == name) {
            continue;
        }
        for run in 0..repeat {
            runs += 1;
            let (mut graph, source) = clip_patch(&reg, &clip);
            let root = graph.root();

            let plan = if let Some(recipe) = recipe {
                match recipe.plan(&reg) {
                    Ok(mut plan) => {
                        otd_ai::recipes::with_source(&mut plan, "movie1");
                        plan
                    }
                    Err(e) => {
                        println!(
                            "{name:<11} {:>5} {:>6} {:>7} {:>7}  PLAN REJECTED {e}",
                            "-", "-", "-", "-"
                        );
                        continue;
                    }
                }
            } else {
                let ask = Ask {
                    provider,
                    model: provider.default_model().to_string(),
                    prompt: prompt.to_string(),
                    image: None,
                    graph: &graph,
                    parent: root,
                    selected: Some(source),
                    registry: &reg,
                    allow_delete: true,
                    scope: &[],
                };
                let request = otd_ai::request_for(&ask);
                let reply =
                    match otd_ai::complete_with_repair(&request, &key, &keys, Some(check_shader)) {
                        Ok(r) => r,
                        Err(e) => {
                            println!(
                                "{name:<11} {:>5} {:>6} {:>7} {:>7}  CALL FAILED {e}",
                                "-", "-", "-", "-"
                            );
                            continue;
                        }
                    };
                match otd_ai::plan_from_reply(&reply.text, &reg) {
                    Ok(p) => p,
                    Err(e) => {
                        println!(
                            "{name:<11} {:>5} {:>6} {:>7} {:>7}  PLAN REJECTED {e}",
                            "-", "-", "-", "-"
                        );
                        continue;
                    }
                }
            };
            let (applied, viewer) = match patch::apply(&mut graph, root, &reg, &plan) {
                Ok(v) => v,
                Err(e) => {
                    println!(
                        "{name:<11} {:>5} {:>6} {:>7} {:>7}  APPLY FAILED {e}",
                        "-", "-", "-", "-"
                    );
                    continue;
                }
            };

            // What the editor would be showing. Falling back to the last Null
            // TOP is what `open` does, and what the user would see.
            let watched = viewer
                .or_else(|| {
                    graph
                        .walk()
                        .into_iter()
                        .rfind(|id| graph.node(*id).op_type == otd_gpu::ops::NULL)
                })
                .unwrap_or(source);

            let (m, shader_errors) = measure(&gpu, &mut graph, watched, source);

            let tag = if repeat > 1 {
                format!("#{} ", run + 1)
            } else {
                String::new()
            };
            let mut faults: Vec<String> = Vec::new();
            if m.luma < 2.0 {
                faults.push("BLACK".into());
            }
            // The other end of the same failure. A feedback loop that adds a
            // bright clip every frame settles at many times the clip, and
            // solid white passes every other check here — it is not black, it
            // is not the source, and the last few percent of it still moves.
            if m.luma > 248.0 {
                faults.push("BLOWN".into());
            }
            if m.motion < 0.4 {
                faults.push("STILL".into());
            }
            if m.from_source < 2.0 {
                faults.push("PASSTHROUGH".into());
            }
            faults.extend(shader_errors.iter().map(|e| format!("SHADER({e})")));
            faults.extend(dangling_refs(&graph, &applied.created));
            let verdict = if faults.is_empty() {
                passes += 1;
                format!("{tag}ok")
            } else {
                format!("{tag}{}", faults.join(" "))
            };

            println!(
                "{name:<11} {:>5} {:>6.1} {:>7.2} {:>7.1}  {verdict}",
                applied.created.len(),
                m.luma,
                m.motion,
                m.from_source,
            );
            for w in &applied.warnings {
                println!("{:>12}warn: {w}", "");
            }
            if let Some(dir) = &save {
                let path = format!("{dir}/{name}{}.otd", if repeat > 1 { run + 1 } else { 0 });
                let project = Project::from_graph(&graph, &reg, 60.0);
                match project.save(std::path::Path::new(&path)) {
                    Ok(()) => println!("{:>12}saved {path}", ""),
                    Err(e) => println!("{:>12}save failed: {e}", ""),
                }
            }
        }
    }

    println!("{}", "-".repeat(78));
    println!("{passes}/{runs} produced a moving, non-black picture that differs from the source");
}

/// A clip on the canvas and a null after it — the patch somebody has open
/// when they ask for an effect.
fn clip_patch(reg: &OpRegistry, clip: &str) -> (Graph, NodeId) {
    let mut graph = Graph::new();
    let root = graph.root();
    let movie = graph
        .create(root, reg.get("moviefileinTOP").unwrap(), Some("movie1"))
        .unwrap();
    graph
        .set_param(movie, "file", Value::Str(clip.to_string()))
        .unwrap();
    let out = graph
        .create(root, reg.get("nullTOP").unwrap(), Some("out1"))
        .unwrap();
    graph.connect(movie, out, 0).unwrap();
    graph.node_mut(out).pos = [200.0, 0.0];
    (graph, movie)
}

/// Cook a handful of frames and compare them.
///
/// Several frames rather than one: a movie decodes on a worker thread, so the
/// first cook of a fresh patch legitimately has no picture yet, and judging a
/// patch on it would call every one of them black. The frames compared for
/// motion are far enough apart that a slow drift still registers.
fn measure(
    gpu: &GpuContext,
    graph: &mut Graph,
    watched: NodeId,
    source: NodeId,
) -> (Measured, Vec<String>) {
    let mut engines = Engines::new(gpu.clone());
    let mut cook = CookEngine::new();
    let mut time = CookContext::default();

    let mut cook_error: Option<String> = None;
    let mut early: Option<Vec<u8>> = None;
    let mut late: Option<Vec<u8>> = None;
    let mut src_pixels: Option<Vec<u8>> = None;

    // ---- wait for the clip before judging anything
    //
    // A movie decodes on a worker thread, and this loop cooks as fast as the
    // GPU will go — forty frames of it is a few tens of milliseconds, during
    // which ffmpeg has barely started. Measuring there marks a patch black
    // when what is black is the *source*, which is a bug in the ruler rather
    // than in the thing being measured. It is also intermittent, so it grades
    // the same patch differently on consecutive runs, which is worse than
    // being wrong consistently.
    //
    // So: cook with real time passing until the source has an actual picture,
    // and only then start measuring. Bounded, because a clip that genuinely
    // never decodes has to be reported rather than waited on for ever.
    let mut ready = false;
    for _ in 0..200 {
        engines.begin_frame();
        let _ = cook.cook_frame(graph, &[watched, source], &time, &mut engines);
        engines.end_frame();
        time.advance(1.0 / 60.0);
        if let Some(p) = read(gpu, &engines, graph, source) {
            if mean_luma(&p) > 1.0 {
                ready = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    if !ready {
        return (
            Measured {
                luma: 0.0,
                motion: 0.0,
                from_source: 0.0,
            },
            vec!["SOURCE NEVER DECODED".into()],
        );
    }

    // Long enough for the decoder to deliver and for a feedback loop to have
    // built up something to look at.
    for frame in 0..40 {
        engines.begin_frame();
        if let Err(e) = cook.cook_frame(graph, &[watched, source], &time, &mut engines) {
            if cook_error.is_none() {
                cook_error = Some(format!("{e}"));
            }
        }
        engines.end_frame();
        time.advance(1.0 / 60.0);

        if frame == 24 {
            early = read(gpu, &engines, graph, watched);
        }
        if frame == 39 {
            late = read(gpu, &engines, graph, watched);
            src_pixels = read(gpu, &engines, graph, source);
        }
    }

    let mut errors: Vec<String> = graph
        .node(graph.root())
        .children
        .clone()
        .into_iter()
        .filter_map(|id| {
            engines
                .node_status(graph, id)
                .map(|s| format!("{}: {}", graph.node(id).name, first_words(&s)))
        })
        .collect();
    if let Some(e) = cook_error {
        errors.push(format!("COOK {}", first_words(&e)));
    }

    let luma = late.as_ref().map(|p| mean_luma(p)).unwrap_or(0.0);
    let motion = match (&early, &late) {
        (Some(a), Some(b)) => mean_abs_diff(a, b),
        _ => 0.0,
    };
    // Resolutions can differ, in which case "same as the source" is already
    // false and the comparison is skipped rather than faked.
    let from_source = match (&late, &src_pixels) {
        (Some(a), Some(b)) if a.len() == b.len() => mean_abs_diff(a, b),
        (Some(_), Some(_)) => 255.0,
        _ => 0.0,
    };

    (
        Measured {
            luma,
            motion,
            from_source,
        },
        errors,
    )
}

fn read(gpu: &GpuContext, engines: &Engines, graph: &Graph, id: NodeId) -> Option<Vec<u8>> {
    let tex = engines.top.output(graph, id)?.clone();
    otd_gpu::read_pixels_rgba8(gpu, &tex)
        .ok()
        .map(|(_, _, p)| p)
}

fn mean_luma(pixels: &[u8]) -> f64 {
    let sum: f64 = pixels
        .chunks(4)
        .map(|p| 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64)
        .sum();
    sum / (pixels.len() / 4).max(1) as f64
}

fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let sum: f64 = (0..n)
        .map(|i| (a[i] as i32 - b[i] as i32).abs() as f64)
        .sum();
    sum / n as f64
}

/// Parameters naming an operator that is not there — the quiet 3D failure.
fn dangling_refs(graph: &Graph, created: &[String]) -> Vec<String> {
    let root = graph.root();
    let mut out = Vec::new();
    for name in created {
        let Some(id) = graph.find_from(root, name) else {
            continue;
        };
        for (key, p) in &graph.node(id).params {
            if !p.is_path_ref() {
                continue;
            }
            let v = p.value.as_str();
            if !v.trim().is_empty() && graph.find_from(id, v.trim()).is_none() {
                out.push(format!("DANGLING({name}.{key})"));
            }
        }
    }
    out
}

fn first_words(text: &str) -> String {
    text.split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join(" ")
}

fn check_shader(source: &str, is_glsl: bool) -> Result<(), String> {
    if is_glsl {
        otd_gpu::shader::validate_glsl(&otd_gpu::shader::wrap_glsl(source))
    } else {
        otd_gpu::shader::validate_wgsl(&otd_gpu::shader::wrap_wgsl(source))
    }
}
