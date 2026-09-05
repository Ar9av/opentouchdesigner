//! Recipes: known-good plans, in the model's own words.
//!
//! A recipe is a request somebody might actually type, and the plan that
//! answers it — in exactly the JSON the model is asked to produce. One file
//! serves three readers:
//!
//!  * **The editor**, as a template. Click it and [`crate::patch::apply`]
//!    builds it: no key, no round trip, works offline. This is the way in for
//!    anybody who does not know what to type into the box.
//!  * **The model**, as a worked example. The brief describes a feedback loop
//!    in prose; a recipe *is* one, with real numbers, and the loop was the
//!    thing the model got wrong half the time. [`examples_for`] picks the two
//!    closest to the request and appends them to the system prompt.
//!  * **The tests**. Every recipe must parse, apply with no warnings, resolve
//!    every path it names, and compile every shader it carries. A template
//!    that has rotted is worse than none, and the same file being the
//!    example the model learns from is what keeps that from being silent.
//!
//! They are JSON rather than Rust for the second reader: what the model sees
//! is the file, byte for byte, so there is nothing to translate and nothing
//! to drift.
//!
//! A recipe that needs footage does not create it. It names `source1` in its
//! wires — the clip already on the canvas — and the caller says which node
//! that is ([`with_source`]), or stands one in ([`stand_in_source`]). The
//! model reads the same convention as "wire from what is there".

use std::collections::BTreeMap;
use std::sync::OnceLock;

use otd_core::{OpRegistry, Value};

use crate::patch::{self, Plan, PlannedNode};

/// The node a `needs: video` recipe wires from and never creates.
pub const SOURCE: &str = "source1";

/// What a recipe assumes is already on the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    /// A TOP named [`SOURCE`] — the clip or camera the effect goes on.
    Video,
    /// Nothing. The recipe is its own source.
    None,
}

pub struct Recipe {
    pub name: String,
    /// Menu heading: Looks, Video, 3D, Audio.
    pub group: String,
    /// The request, as somebody would type it. Pasted into the box verbatim
    /// by the suggestion chips, so it has to read as one.
    pub prompt: String,
    pub needs: Needs,
    /// What it built and which parameter to turn — what the assistant would
    /// have said in `notes`.
    pub notes: String,
    /// The plan half of the file, compact, for the model to read.
    pub json: String,
    value: serde_json::Value,
}

/// Header keys — everything in the file that is not the plan.
const HEADER: &[&str] = &["name", "group", "prompt", "needs"];

const FILES: &[&str] = &[
    include_str!("../recipes/motionpaint.json"),
    include_str!("../recipes/slitscan.json"),
    include_str!("../recipes/thermal.json"),
    include_str!("../recipes/hologram.json"),
    include_str!("../recipes/tunnel.json"),
    include_str!("../recipes/plasma.json"),
    include_str!("../recipes/rings.json"),
    include_str!("../recipes/trails.json"),
    include_str!("../recipes/glitch.json"),
    include_str!("../recipes/kaleidoscope.json"),
    include_str!("../recipes/retro.json"),
    include_str!("../recipes/toon.json"),
    include_str!("../recipes/smoke.json"),
    include_str!("../recipes/neon.json"),
    include_str!("../recipes/ripple.json"),
    include_str!("../recipes/cells.json"),
    include_str!("../recipes/torus.json"),
    include_str!("../recipes/field.json"),
    include_str!("../recipes/terrain.json"),
    include_str!("../recipes/audio.json"),
    include_str!("../recipes/beat.json"),
];

/// Every recipe, in menu order. Parsed once; a file that does not parse is a
/// build error in the test suite, not a runtime one here.
pub fn all() -> &'static [Recipe] {
    static ALL: OnceLock<Vec<Recipe>> = OnceLock::new();
    ALL.get_or_init(|| {
        FILES
            .iter()
            .filter_map(|text| Recipe::parse(text).ok())
            .collect()
    })
}

pub fn find(name: &str) -> Option<&'static Recipe> {
    all().iter().find(|r| r.name == name)
}

/// Group headings, in the order they first appear.
pub fn groups() -> Vec<&'static str> {
    let mut out: Vec<&str> = Vec::new();
    for r in all() {
        if !out.contains(&r.group.as_str()) {
            out.push(&r.group);
        }
    }
    out
}

impl Recipe {
    pub fn parse(text: &str) -> Result<Recipe, String> {
        let mut value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("recipe is not JSON: {e}"))?;
        let object = value.as_object_mut().ok_or("recipe is not a JSON object")?;
        let field = |object: &serde_json::Map<String, serde_json::Value>, key: &str| {
            object
                .get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| format!("recipe has no `{key}`"))
        };
        let name = field(object, "name")?;
        let group = field(object, "group")?;
        let prompt = field(object, "prompt")?;
        let needs = match field(object, "needs")?.as_str() {
            "video" => Needs::Video,
            "none" => Needs::None,
            other => return Err(format!("{name}: `needs` is {other:?}, not video or none")),
        };
        let notes = field(object, "notes").unwrap_or_default();
        for key in HEADER {
            object.remove(*key);
        }
        let json = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        Ok(Recipe {
            name,
            group,
            prompt,
            needs,
            notes,
            json,
            value,
        })
    }

    /// The plan, checked against the registry like any reply would be.
    pub fn plan(&self, registry: &OpRegistry) -> Result<Plan, String> {
        patch::parse_plan(&self.value, registry)
    }

    /// The plan as JSON, for [`patch::shader_problems`].
    pub fn value(&self) -> &serde_json::Value {
        &self.value
    }
}

/// Point every mention of [`SOURCE`] at a node that already exists.
///
/// Wires, the viewer, exports (`source1:chan`) and any string parameter
/// holding the bare name — which is how `feedbackTOP.target` and
/// `selectTOP.top` refer to a node.
pub fn with_source(plan: &mut Plan, name: &str) {
    let swap = |s: &mut String| {
        if s == SOURCE {
            *s = name.to_string();
        }
    };
    for wire in &mut plan.connections {
        swap(&mut wire.from);
        swap(&mut wire.to);
    }
    if let Some(v) = &mut plan.viewer {
        swap(v);
    }
    for node in &mut plan.nodes {
        for value in node.params.values_mut() {
            if let Value::Str(s) = value {
                swap(s);
            }
        }
        for target in node.exports.values_mut() {
            if let Some(rest) = target.strip_prefix(SOURCE) {
                if rest.starts_with(':') {
                    *target = format!("{name}{rest}");
                }
            }
        }
    }
}

/// Nothing is selected: put a generator where the clip would go, so the
/// template still shows something and there is a node to drop a clip onto.
pub fn stand_in_source(plan: &mut Plan) {
    let first = plan
        .connections
        .iter()
        .find(|w| w.from == SOURCE)
        .and_then(|w| plan.nodes.iter().find(|n| n.name == w.to))
        .map(|n| n.pos)
        .unwrap_or([0.0, 0.0]);
    let min_x = plan.nodes.iter().map(|n| n.pos[0]).fold(first[0], f32::min);
    let mut params = BTreeMap::new();
    params.insert("period".to_string(), Value::Float(0.3));
    params.insert("monochrome".to_string(), Value::Bool(false));
    let mut expressions = BTreeMap::new();
    expressions.insert("translate".to_string(), "absTime * 0.1".to_string());
    plan.nodes.insert(
        0,
        PlannedNode {
            name: SOURCE.into(),
            op: "noiseTOP".into(),
            pos: [min_x - 200.0, first[1]],
            params,
            expressions,
            exports: BTreeMap::new(),
        },
    );
}

/// Up to two relevant executable examples. Unknown requests get no arbitrary
/// fallback; short tokens such as 3D and explicit recipe names carry intent.
pub fn examples_for(prompt: &str, has_source: bool) -> String {
    let asked = crate::knowledge::words(prompt);
    let mut scored: Vec<(i32, &Recipe)> = all()
        .iter()
        .map(|r| {
            let mut text = format!("{} {} {}", r.name, r.prompt, r.notes);
            if let Some(nodes) = r.value.get("nodes").and_then(|n| n.as_array()) {
                for n in nodes {
                    if let Some(op) = n.get("op").and_then(|o| o.as_str()) {
                        text.push(' ');
                        text.push_str(op);
                    }
                }
            }
            let have = crate::knowledge::words(&text);
            let mut score = asked
                .iter()
                .filter(|w| {
                    w.len() >= 4
                        && ![
                            "make",
                            "with",
                            "that",
                            "this",
                            "from",
                            "like",
                            "into",
                            "more",
                            "less",
                            "have",
                            "give",
                            "some",
                            "something",
                        ]
                        .contains(&w.as_str())
                        && have.contains(w)
                })
                .count() as i32;
            if asked.contains(&r.name.to_lowercase()) {
                score += 20;
            }
            for technique in crate::knowledge::TECHNIQUES {
                if crate::knowledge::matches(technique, prompt)
                    && technique.recipes.contains(&r.name.as_str())
                {
                    score += 5;
                }
            }
            if score > 0 && has_source && r.needs == Needs::Video {
                score += 1;
            }
            (score, r)
        })
        .collect();
    // Stable, so a tie keeps menu order.
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let picked: Vec<&Recipe> = scored
        .iter()
        .filter(|(score, _)| *score > 0)
        .take(2)
        .map(|(_, r)| *r)
        .collect();

    if picked.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n\nWORKED EXAMPLES\n\
         Relevant requests and the plan that answered each, exactly as it was \
         applied. Adapt only the relevant parts to the request: a chain that ends in a nullTOP named \
         out1 with `viewer` set to it, where feedback is needed choose its mix deliberately, \
         and the numbers worth turning in `params` rather than in a shader. \
         Where an example wires from `source1`, that is the clip already on \
         the canvas: use the real name of the source node in the network \
         below, and never create one. If the network already has an out1, \
         wire into it rather than adding a second.\n",
    );
    for r in picked {
        out.push_str(&format!("\nREQUEST: {}\nPLAN: {}\n", r.prompt, r.json));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_parses_and_names_are_unique() {
        assert_eq!(all().len(), FILES.len(), "a recipe file failed to parse");
        let mut names: Vec<&str> = all().iter().map(|r| r.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), FILES.len());
    }

    #[test]
    fn the_header_is_stripped_from_what_the_model_sees() {
        for r in all() {
            let v: serde_json::Value = serde_json::from_str(&r.json).unwrap();
            for key in HEADER {
                assert!(
                    v.get(*key).is_none(),
                    "{}: `{key}` leaked into the plan",
                    r.name
                );
            }
            assert!(v.get("nodes").is_some(), "{}: no nodes", r.name);
        }
    }

    #[test]
    fn a_loop_request_is_shown_the_trails_recipe() {
        let text = examples_for("give it long ghostly trails", true);
        assert!(text.contains("REQUEST: give the video long smeary trails"));
        assert_eq!(text.matches("REQUEST:").count(), 2);
    }

    #[test]
    fn a_3d_request_is_shown_3d() {
        let text = examples_for("a spinning torus rendered in 3d", false);
        assert!(text.contains("torusSOP"), "{text}");
    }

    #[test]
    fn with_source_rewrites_every_mention() {
        let mut plan = Plan::default();
        plan.connections.push(patch::PlannedWire {
            from: SOURCE.into(),
            to: "x".into(),
            input: 0,
        });
        let mut params = BTreeMap::new();
        params.insert("target".to_string(), Value::Str(SOURCE.into()));
        let mut exports = BTreeMap::new();
        exports.insert("scale".to_string(), format!("{SOURCE}:r"));
        plan.nodes.push(PlannedNode {
            name: "x".into(),
            op: "feedbackTOP".into(),
            pos: [0.0, 0.0],
            params,
            expressions: BTreeMap::new(),
            exports,
        });
        with_source(&mut plan, "movie1");
        assert_eq!(plan.connections[0].from, "movie1");
        assert_eq!(plan.nodes[0].params["target"], Value::Str("movie1".into()));
        assert_eq!(plan.nodes[0].exports["scale"], "movie1:r");
    }
}
