//! What the assistant can and cannot be asked for.
//!
//! `smoke` answers "is the wire format still right" for one prompt. This asks
//! the harder question: across the kinds of thing somebody actually types
//! into the box, which come back as a patch that works?
//!
//! Each scenario is a real request against a real graph, and what it prints
//! is what the editor would have built — how many operators, which families
//! they came from, whether it wired into what was already there, and every
//! warning `apply` raised. A scenario that comes back "ok" with a warning is
//! not a pass; the warning is the finding.
//!
//! Not a test, for the same reason `smoke` is not: it needs a network, a
//! login and somebody's money.
//!
//!     cargo run -p otd-ai --example scenarios
//!     cargo run -p otd-ai --example scenarios -- --only extend,tweak
//!     cargo run -p otd-ai --example scenarios -- --repeat 3
//!     cargo run -p otd-ai --example scenarios -- --provider codex
//!     cargo run -p otd-ai --example scenarios -- --image shot.png
//!
//! It prints what came back, never what was sent as a credential.

use otd_ai::{Ask, Keys, Provider, Repair, patch};
use otd_core::{Family, Graph, NodeId, OpRegistry};

fn check_shader(source: &str, is_glsl: bool) -> Result<(), String> {
    if is_glsl {
        otd_gpu::shader::validate_glsl(&otd_gpu::shader::wrap_glsl(source))
    } else {
        otd_gpu::shader::validate_wgsl(&otd_gpu::shader::wrap_wgsl(source))
    }
}

struct Scenario {
    name: &'static str,
    /// What the user types into the box.
    prompt: &'static str,
    /// The patch already on the canvas, and what is selected in it.
    setup: fn(&OpRegistry) -> (Graph, Option<NodeId>),
    /// What this scenario is actually probing, printed when it goes wrong.
    asking: &'static str,
    wants_image: bool,
}

fn empty(_registry: &OpRegistry) -> (Graph, Option<NodeId>) {
    (Graph::new(), None)
}

/// The `video` demo: a clip, a feedback loop tuned by hand, a composite.
fn video_patch(registry: &OpRegistry) -> (Graph, Option<NodeId>) {
    let (graph, _out) = otd_engine::demo::by_name("video", registry).expect("video demo");
    let root = graph.root();
    let selected = graph.find_from(root, "drift1");
    (graph, selected)
}

/// A live camera and a small chain off it — the shape somebody has on screen
/// when they ask for the picture to react to them.
fn camera_patch(registry: &OpRegistry) -> (Graph, Option<NodeId>) {
    let mut graph = Graph::new();
    let root = graph.root();
    let cam = graph
        .create(root, registry.get("videodeviceinTOP").unwrap(), Some("camera1"))
        .unwrap();
    let out = graph
        .create(root, registry.get("nullTOP").unwrap(), Some("out1"))
        .unwrap();
    graph.connect(cam, out, 0).unwrap();
    (graph, Some(cam))
}

/// The `tunnel` demo — nine nodes with numbers somebody chose.
fn tunnel_patch(registry: &OpRegistry) -> (Graph, Option<NodeId>) {
    let (graph, _out) = otd_engine::demo::by_name("tunnel", registry).expect("tunnel demo");
    let root = graph.root();
    let selected = graph.find_from(root, "zoom1");
    (graph, selected)
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "scratch",
        asking: "a whole patch from nothing, in one family",
        prompt: "A slow feedback tunnel: contrast a monochrome noise field down \
                 to bright wisps, tint it teal-to-magenta through a ramp, and loop \
                 it with a slight zoom and rotate.",
        setup: empty,
        wants_image: false,
    },
    Scenario {
        name: "extend",
        asking: "adding to a patch without rebuilding it",
        prompt: "make it run and transform in a cool 3d splat",
        setup: video_patch,
        wants_image: false,
    },
    Scenario {
        name: "tweak",
        asking: "changing numbers on operators that already exist — `set` should \
                 carry this, with no duplicate operators created beside them",
        prompt: "This is too fast and too blue. Slow the zoom right down and \
                 make the whole thing red and orange.",
        setup: tunnel_patch,
        wants_image: false,
    },
    Scenario {
        name: "shader",
        asking: "a glslTOP whose source must survive JSON escaping and a compiler",
        prompt: "Add a raymarched tunnel of glowing points that drifts with time, \
                 as a GLSL shader.",
        setup: empty,
        wants_image: false,
    },
    Scenario {
        name: "vfx",
        asking: "effects ON a clip — the shader has to READ the input, not replace it",
        prompt: "can u make add crazy video effects on top of existing video",
        setup: video_patch,
        wants_image: false,
    },
    Scenario {
        name: "motion",
        asking: "reacting to the person in front of the camera — needs a \
                 difference, a Top To CHOP and an EXPORT, not just absTime",
        prompt: "add cool effects as i move around in the video",
        setup: camera_patch,
        wants_image: false,
    },
    Scenario {
        name: "looks",
        asking: "the named looks — should reach ditherTOP / voronoiTOP / \
                 toonTOP / flowTOP rather than writing a shader for each",
        prompt: "give me a retro dithered 1-bit look, and a separate branch \
                 with cracked-glass cells drifting",
        setup: empty,
        wants_image: false,
    },
    Scenario {
        name: "morph",
        asking: "blendSOP plus a moving blend — a morph that actually animates",
        prompt: "morph a sphere into a torus, back and forth, rendered in 3d",
        setup: empty,
        wants_image: false,
    },
    Scenario {
        name: "depth",
        asking: "renderTOP depth output feeding something, rather than a fake \
                 depth made out of a ramp",
        prompt: "render a 3d scene and use its depth to add fog in the distance",
        setup: empty,
        wants_image: false,
    },
    Scenario {
        name: "simplify",
        asking: "removing rather than adding — does it reach for `delete` and `set`, \
                 and does it leave the source and the output alone",
        prompt: "this is way too much going on, make it simpler",
        setup: tunnel_patch,
        wants_image: false,
    },
    Scenario {
        name: "retune",
        asking: "changing a number on an operator that already exists, via `set` \
                 rather than a duplicate",
        prompt: "slow the zoom right down, it is making me seasick",
        setup: tunnel_patch,
        wants_image: false,
    },
    Scenario {
        name: "audio",
        asking: "reaching the CHOP family and driving a parameter from it",
        prompt: "Make it react to the music: the zoom should lurch on every kick \
                 and the trails should get longer when it is loud.",
        setup: tunnel_patch,
        wants_image: false,
    },
    Scenario {
        name: "3d",
        asking: "SOPs, a Geometry COMP, a camera and a Render TOP together",
        prompt: "A spinning 3d torus, lit, rendered to a texture, with a little bloom.",
        setup: empty,
        wants_image: false,
    },
    Scenario {
        name: "instance",
        asking: "instancing — a CHOP driving thousands of copies of a SOP",
        prompt: "Two thousand glowing points scattered in 3d, instanced from a \
                 noise field, drifting slowly, rendered to a texture.",
        setup: empty,
        wants_image: false,
    },
    Scenario {
        name: "expr",
        asking: "expression mode rather than constants — motion without a CHOP",
        prompt: "Make the colours cycle continuously and the whole image breathe \
                 in and out, driven by time.",
        setup: tunnel_patch,
        wants_image: false,
    },
    Scenario {
        name: "vague",
        asking: "an ask with no technical content — does it degrade or flail",
        prompt: "make it cooler",
        setup: tunnel_patch,
        wants_image: false,
    },
    Scenario {
        name: "media",
        asking: "a file path typed into the prompt — does it reach a file parameter",
        prompt: "can u integratae \
                 /Users/ar9av/Downloads/How_RFC_8693_secures_microservice_token_exchange.m4a",
        setup: tunnel_patch,
        wants_image: false,
    },
    Scenario {
        name: "image",
        // Empty on purpose: with a picture attached the reverse-engineering
        // brief is the whole request, and an empty box is a complete ask.
        asking: "working back from a picture instead of a sentence",
        prompt: "",
        setup: empty,
        wants_image: true,
    },
];

fn main() {
    let keys = Keys::load();
    let registry = otd_engine::registry();

    let mut provider = Provider::ClaudeCode;
    let mut image_path: Option<String> = None;
    let mut only: Vec<String> = Vec::new();
    let mut repeat = 1usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--provider" => {
                provider = args
                    .next()
                    .and_then(|n| Provider::parse(&n))
                    .unwrap_or(provider)
            }
            "--image" => image_path = args.next(),
            "--only" => {
                only = args
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect()
            }
            "--repeat" => repeat = args.next().and_then(|n| n.parse().ok()).unwrap_or(1),
            other => eprintln!("ignoring `{other}`"),
        }
    }
    let image = image_path.as_ref().map(|p| {
        otd_ai::Image::load(p).unwrap_or_else(|e| {
            eprintln!("--image {p}: {e}");
            std::process::exit(1);
        })
    });

    println!("assistant: {} ({})", provider.label(), provider.default_model());
    let key = keys
        .get(provider)
        .cloned()
        .unwrap_or_else(|| otd_ai::Key::new(""));

    for scenario in SCENARIOS {
        if !only.is_empty() && !only.iter().any(|n| n == scenario.name) {
            continue;
        }
        if scenario.wants_image && image.is_none() {
            println!("\n{:9} skipped — needs --image", scenario.name);
            continue;
        }
        println!("\n{:9} {}", scenario.name, scenario.asking);

        for run in 0..repeat {
            let (mut graph, selected) = (scenario.setup)(&registry);
            let root = graph.root();
            let before: Vec<String> = graph
                .node(root)
                .children
                .iter()
                .map(|id| graph.node(*id).name.clone())
                .collect();

            let ask = Ask {
                provider,
                model: provider.default_model().to_string(),
                prompt: scenario.prompt.to_string(),
                image: scenario.wants_image.then(|| image.clone()).flatten(),
                graph: &graph,
                parent: root,
                selected,
                registry: &registry,
                allow_delete: true,
            };
            let request = otd_ai::request_for(&ask);

            let started = std::time::Instant::now();
            let reply = otd_ai::complete_with_repair(&request, &key, &keys, Some(check_shader));
            let took = started.elapsed().as_secs_f32();
            let tag = if repeat > 1 {
                format!("  #{}", run + 1)
            } else {
                String::new()
            };

            let reply = match reply {
                Ok(reply) => reply,
                Err(e) => {
                    println!("  {:>5.1}s  CALL FAILED  {e}{tag}", took);
                    continue;
                }
            };
            let note = match reply.repaired {
                Some(Repair::Json) => "  [json retried]",
                Some(Repair::Shader) => "  [shader retried]",
                None => "",
            };

            let plan = match otd_ai::plan_from_reply(&reply.text, &registry) {
                Ok(plan) => plan,
                Err(e) => {
                    println!("  {:>5.1}s  PLAN REJECTED  {e}{note}{tag}", took);
                    continue;
                }
            };

            // Did it try to change what was there, or only add beside it?
            let duplicated: Vec<String> = plan
                .nodes
                .iter()
                .filter(|n| before.iter().any(|b| b == &n.name))
                .map(|n| n.name.clone())
                .collect();
            let reused: Vec<String> = plan
                .connections
                .iter()
                .flat_map(|w| [w.from.clone(), w.to.clone()])
                .chain(plan.sets.iter().map(|s| s.node.clone()))
                .chain(plan.deletes.iter().cloned())
                .filter(|n| before.contains(n))
                .collect();
            let expressions: usize = plan.nodes.iter().map(|n| n.expressions.len()).sum();
            let exports: usize = plan
                .nodes
                .iter()
                .map(|n| n.exports.len())
                .chain(plan.sets.iter().map(|s| s.exports.len()))
                .sum();

            match patch::apply(&mut graph, root, &registry, &plan) {
                Ok((applied, _viewer)) => {
                    // Which families it actually reached for. A patch that can
                    // only be TOPs is a patch that cannot react or be 3d.
                    let mut families: Vec<&str> = plan
                        .nodes
                        .iter()
                        .filter_map(|n| registry.get(&n.op).map(|d| family_name(d.family)))
                        .collect();
                    families.sort_unstable();
                    families.dedup();

                    println!(
                        "  {:>5.1}s  {} nodes [{}], {} wires, {} expressions, \
                         {} retuned, {} removed, {} EXPORTS{}{}{}",
                        took,
                        applied.created.len(),
                        families.join(" "),
                        applied.wired,
                        expressions,
                        applied.changed.len(),
                        applied.removed.len(),
                        exports,
                        match reused.is_empty() {
                            true => String::new(),
                            false => format!(", reuses [{}]", dedup(&reused).join(" ")),
                        },
                        match duplicated.is_empty() {
                            true => String::new(),
                            false => format!(", REBUILT [{}]", dedup(&duplicated).join(" ")),
                        },
                        format!("{note}{tag}"),
                    );
                    if !before.is_empty() && reused.is_empty() {
                        println!("            ^ built beside the patch, not into it");
                    }
                    // A plan that names an existing node cannot edit it —
                    // `apply` only creates — so the graph renames around the
                    // collision and the original is left exactly as it was.
                    // Show what the user would actually end up with.
                    for name in dedup(&duplicated) {
                        let landed = applied
                            .created
                            .iter()
                            .find(|c| c.starts_with(name.trim_end_matches(char::is_numeric)))
                            .cloned()
                            .unwrap_or_else(|| "?".into());
                        println!("            ^ asked to change {name}, got a new {landed} instead");
                    }
                    if let Ok(json) = patch::extract_json(&reply.text) {
                        for (node, error) in patch::shader_problems(&json, check_shader) {
                            println!("            shader {node}: {error}");
                        }
                    }
                    if !applied.removed.is_empty() {
                        println!("            removed: {}", applied.removed.join(" "));
                    }
                    if !applied.changed.is_empty() {
                        println!("            retuned: {}", applied.changed.join(" "));
                    }
                    // A parameter naming another operator that is not there
                    // is the quiet 3D failure: everything builds, nothing
                    // renders. Worth printing, because no warning covers it.
                    for name in &applied.created {
                        let Some(id) = graph.find_from(root, name) else {
                            continue;
                        };
                        let dangling: Vec<String> = graph
                            .node(id)
                            .params
                            .iter()
                            .filter(|(_, p)| p.is_path_ref())
                            .filter(|(_, p)| !p.value.as_str().trim().is_empty())
                            .filter(|(_, p)| {
                                graph.find_from(id, p.value.as_str().trim()).is_none()
                            })
                            .map(|(k, p)| format!("{k}={}", p.value.as_str()))
                            .collect();
                        if !dangling.is_empty() {
                            println!(
                                "            DANGLING REF {name}: {}",
                                dangling.join(" ")
                            );
                        }
                    }
                    for warning in &applied.warnings {
                        println!("            warn: {warning}");
                    }
                    if !plan.notes.trim().is_empty() {
                        println!("            \"{}\"", clip(&plan.notes));
                    }
                }
                Err(e) => println!("  {:>5.1}s  APPLY FAILED  {e}{note}{tag}", took),
            }
        }
    }
}

fn family_name(family: Family) -> &'static str {
    family.suffix()
}

fn clip(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(110) {
        Some((end, _)) => format!("{}…", &flat[..end]),
        None => flat,
    }
}

fn dedup(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        if !out.contains(n) {
            out.push(n.clone());
        }
    }
    out
}
