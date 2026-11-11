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
    include_str!("../recipes/blobhud.json"),
    include_str!("../recipes/ascii.json"),
    include_str!("../recipes/halftone.json"),
    include_str!("../recipes/pointcloud.json"),
    include_str!("../recipes/blobfollow.json"),
    include_str!("../recipes/datamosh.json"),
    include_str!("../recipes/greenscreen.json"),
    include_str!("../recipes/nightvision.json"),
    include_str!("../recipes/relief.json"),
    include_str!("../recipes/mosaic.json"),
    include_str!("../recipes/watercolour.json"),
    include_str!("../recipes/freeze.json"),
    include_str!("../recipes/audiocamera.json"),
    include_str!("../recipes/motionpaint.json"),
    include_str!("../recipes/chromaecho.json"),
    include_str!("../recipes/motionglitch.json"),
    include_str!("../recipes/cloak.json"),
    include_str!("../recipes/motionpixel.json"),
    include_str!("../recipes/popart.json"),
    include_str!("../recipes/rainbowtrail.json"),
    include_str!("../recipes/sketch.json"),
    include_str!("../recipes/slitscan.json"),
    include_str!("../recipes/thermal.json"),
    include_str!("../recipes/hologram.json"),
    include_str!("../recipes/tunnel.json"),
    include_str!("../recipes/plasma.json"),
    include_str!("../recipes/rings.json"),
    include_str!("../recipes/trails.json"),
    include_str!("../recipes/glitch.json"),
    include_str!("../recipes/vhs.json"),
    include_str!("../recipes/crt.json"),
    include_str!("../recipes/bloom.json"),
    include_str!("../recipes/duotone.json"),
    include_str!("../recipes/fisheye.json"),
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
    include_str!("../recipes/musicsync.json"),
    include_str!("../recipes/beatglitch.json"),
    include_str!("../recipes/beatstrobe.json"),
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

/// The recipes, one line each, for the brief: what the model can `use`.
pub fn menu() -> String {
    let mut out = String::from(
        "RECIPES YOU CAN USE BY NAME
Each is a tested patch. Prefer `use` over rebuilding one by hand: it costs
one line, its shaders are known to compile, and it cannot starve a loop.
- `{\"recipe\": \"vhs\", \"on\": \"camera1\"}` builds vhs fed from camera1.
  A recipe marked [video] needs `on`: the clip or camera to put it on.
- Several in one `use` list CHAIN: each one is fed from the one before, so
  [{\"recipe\": \"crt\", \"on\": \"camera1\"}, {\"recipe\": \"bloom\"}] is a CRT
  with its highlights blooming. Leave `on` off everything after the first.
- `\"params\": {\"<node>\": {\"<param>\": value}}` tunes nodes inside it, by the
  names in the recipe; the notes below say which ones matter.
- Its nodes land named `<recipe>_<node>`, e.g. `vhs_out1`; wire on from
  that, or `set` it in a later request. The last one becomes the viewer.
- Add your own `nodes` alongside when nothing here does what was asked.

",
    );
    for r in all() {
        let video = if r.needs == Needs::Video {
            " [video]"
        } else {
            ""
        };
        out.push_str(&format!("- {}{video}: {}", r.name, r.prompt));
        let knobs = knob_sentences(&r.notes);
        if !knobs.is_empty() {
            out.push_str(&format!(" — {knobs}"));
        }
        out.push('\n');
    }
    out
}

/// The sentences of a recipe's notes that name a `node.param` to turn — the
/// part a model needs to tune it — capped so fifty recipes stay a short list.
fn knob_sentences(notes: &str) -> String {
    let names_a_knob = |s: &str| {
        s.split(|c: char| c.is_whitespace() || c == ';' || c == ',')
            .any(|w| {
                let w = w.trim_end_matches(['.', ')', ':']);
                w.split_once('.').is_some_and(|(a, b)| {
                    a.chars().any(|c| c.is_ascii_alphabetic())
                        && b.starts_with(|c: char| c.is_ascii_lowercase())
                })
            })
    };
    let mut out = String::new();
    for sentence in notes.split_inclusive(". ") {
        if names_a_knob(sentence) && out.len() + sentence.len() <= 200 {
            out.push_str(sentence);
        }
    }
    out.trim().to_string()
}

/// Expand a plan's `use` list: each named recipe becomes nodes in the plan,
/// prefixed so two recipes (or one used twice) cannot collide, fed from
/// `on` or from the previous recipe's viewer, laid out left to right.
pub fn expand_uses(value: &serde_json::Value, plan: &mut Plan, registry: &OpRegistry) {
    let Some(uses) = value.get("use").and_then(|u| u.as_array()) else {
        return;
    };
    let mut previous: Option<String> = None;
    let mut x = plan.nodes.iter().map(|n| n.pos[0]).fold(0.0_f32, f32::max);
    let y = plan.nodes.iter().map(|n| n.pos[1]).fold(0.0_f32, f32::max)
        + if plan.nodes.is_empty() { 0.0 } else { 400.0 };
    let mut taken: Vec<String> = plan.nodes.iter().map(|n| n.name.clone()).collect();
    for (i, entry) in uses.iter().enumerate() {
        let name = entry
            .get("recipe")
            .or_else(|| entry.get("name"))
            .and_then(|n| n.as_str())
            .or_else(|| entry.as_str());
        let Some(name) = name else {
            plan.warnings.push(format!("use {i}: no `recipe` name"));
            continue;
        };
        let Some(recipe) = find(name) else {
            let names: Vec<_> = all().iter().map(|r| r.name.as_str()).collect();
            plan.warnings.push(format!(
                "use: there is no recipe `{name}` — the recipes are {}",
                names.join(", ")
            ));
            continue;
        };
        let mut json = recipe.value().clone();
        if let Some(tune) = entry.get("params").and_then(|p| p.as_object()) {
            for (node, params) in tune {
                let target = json["nodes"]
                    .as_array()
                    .and_then(|ns| ns.iter().position(|n| n["name"] == node.as_str()));
                match (target, params.as_object()) {
                    (Some(at), Some(params)) => {
                        let n = &mut json["nodes"][at];
                        if !n["params"].is_object() {
                            n["params"] = serde_json::json!({});
                        }
                        for (k, v) in params {
                            n["params"][k] = v.clone();
                        }
                    }
                    _ => plan
                        .warnings
                        .push(format!("use {name}: it has no node `{node}` to tune")),
                }
            }
        }
        let mut sub = match patch::parse_plan(&json, registry) {
            Ok(sub) => sub,
            Err(e) => {
                plan.warnings.push(format!("use {name}: {e}"));
                continue;
            }
        };
        plan.warnings.append(&mut sub.warnings);

        let mut prefix = name.to_string();
        let mut k = 2;
        while taken.iter().any(|t| t.starts_with(&format!("{prefix}_"))) {
            prefix = format!("{name}{k}");
            k += 1;
        }
        let on = entry
            .get("on")
            .and_then(|o| o.as_str())
            .map(|s| s.to_string())
            .or_else(|| previous.clone());
        if recipe.needs == Needs::Video && on.is_none() {
            plan.warnings.push(format!(
                "use {name}: it goes on a clip or camera — say which with `on`"
            ));
        }
        let own: Vec<String> = sub.nodes.iter().map(|n| n.name.clone()).collect();
        let rename = |s: &str| -> Option<String> {
            if own.iter().any(|o| o == s) {
                Some(format!("{prefix}_{s}"))
            } else if s == SOURCE {
                on.clone()
            } else {
                None
            }
        };
        let swap = |s: &mut String| {
            if let Some(new) = rename(s) {
                *s = new;
            }
        };
        let min_x = sub.nodes.iter().map(|n| n.pos[0]).fold(f32::MAX, f32::min);
        let max_x = sub.nodes.iter().map(|n| n.pos[0]).fold(f32::MIN, f32::max);
        for node in &mut sub.nodes {
            swap(&mut node.name);
            node.pos[0] += x + 250.0 - min_x;
            node.pos[1] += y;
            for value in node.params.values_mut() {
                if let Value::Str(s) = value {
                    swap(s);
                }
            }
            for target in node.exports.values_mut() {
                if let Some((src, chan)) = target.rsplit_once(':') {
                    if let Some(new) = rename(src) {
                        *target = format!("{new}:{chan}");
                    }
                }
            }
        }
        for wire in &mut sub.connections {
            swap(&mut wire.from);
            swap(&mut wire.to);
        }
        // A source nobody named: keep the convention, and the caller stands
        // one in or points it at the selection, as it does for a template.
        sub.connections.retain(|w| !w.from.is_empty());
        x += 250.0 + (max_x - min_x).max(0.0);
        taken.extend(sub.nodes.iter().map(|n| n.name.clone()));
        if let Some(v) = &mut sub.viewer {
            swap(v);
        }
        previous = sub.viewer.clone();
        plan.nodes.append(&mut sub.nodes);
        plan.connections.append(&mut sub.connections);
        if !recipe.notes.is_empty() {
            if !plan.notes.is_empty() {
                plan.notes.push(' ');
            }
            plan.notes.push_str(&format!("[{name}] {}", recipe.notes));
        }
    }
    if plan.viewer.is_none() || value.get("nodes").is_none() {
        if let Some(v) = previous {
            plan.viewer = Some(v);
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

/// Looks that read at a glance on a live camera, strongest first. The Watch
/// brief shows two of these in full and names the rest.
pub const CAMERA_LOOKS: &[&str] = &[
    "blobhud",
    "ascii",
    "halftone",
    "pointcloud",
    "neon",
    "thermal",
    "datamosh",
    "glitch",
    "kaleidoscope",
    "motionpaint",
    "toon",
    "nightvision",
    "cloak",
    "chromaecho",
    "motionpixel",
    "motionglitch",
    "popart",
];

/// Worked examples for a request made of a camera frame rather than words.
///
/// Word matching finds nothing in an empty prompt, and without examples a
/// model asked to "make it cool" improvises faint trails over a subject that
/// is standing still. So: every known-good camera look by name, and two of
/// them in full — the ones the typed direction points at if there is one,
/// otherwise a pair that moves along with `turn`, so asking twice does not
/// give the same answer twice.
pub fn camera_examples(prompt: &str, turn: usize) -> String {
    let matched = examples_for(prompt, true);
    let mut out = String::from(
        "\n\nKNOWN-GOOD CAMERA LOOKS\nEach of these is tested on real footage. Build one of them, \
         or combine two, adapted to what you see; name which in `notes`.\n",
    );
    for name in CAMERA_LOOKS {
        if let Some(r) = find(name) {
            out.push_str(&format!("- {name}: {}\n", r.prompt));
        }
    }
    if !matched.is_empty() {
        return out + &matched;
    }
    out.push_str(
        "\n\nWORKED EXAMPLES\nTwo of those, exactly as applied. `source1` is the camera \
         already on the canvas: use its real name and never create one. Wire into the \
         existing out1 rather than adding a second.\n",
    );
    for k in 0..2 {
        let name = CAMERA_LOOKS[(turn * 2 + k) % CAMERA_LOOKS.len()];
        if let Some(r) = find(name) {
            out.push_str(&format!("\nREQUEST: {}\nPLAN: {}\n", r.prompt, r.json));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(json: serde_json::Value) -> Plan {
        patch::parse_plan(&json, &otd_engine::registry()).unwrap()
    }

    #[test]
    fn use_chains_recipes_by_name() {
        let plan = used(serde_json::json!({"use": [
            {"recipe": "crt", "on": "camera1", "params": {"crt1": {"uniform1": [0.2, 0, 0, 0]}}},
            {"recipe": "bloom"},
        ]}));
        let wired = |from: &str, to: &str| {
            plan.connections
                .iter()
                .any(|w| w.from == from && w.to == to)
        };
        assert!(wired("camera1", "crt_crt1"), "{:?}", plan.connections);
        // The second is fed from the first's viewer, not from a source1.
        assert!(wired("crt_out1", "bloom_bright1"), "{:?}", plan.connections);
        assert!(!plan.connections.iter().any(|w| w.from == SOURCE));
        assert_eq!(plan.viewer.as_deref(), Some("bloom_out1"));
        let crt = plan.nodes.iter().find(|n| n.name == "crt_crt1").unwrap();
        assert_eq!(crt.params["uniform1"], Value::Vec4([0.2, 0.0, 0.0, 0.0]));
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn use_renames_exports_and_targets_and_says_what_is_wrong() {
        let plan = used(serde_json::json!({"use": [
            {"recipe": "beatglitch", "on": "camera1"},
            {"recipe": "trails"},
            {"recipe": "nonsense"},
        ]}));
        let glitch = plan
            .nodes
            .iter()
            .find(|n| n.name == "beatglitch_glitch1")
            .unwrap();
        assert_eq!(glitch.exports["uniform1"], "beatglitch_amount1:kick");
        let fb = plan.nodes.iter().find(|n| n.name == "trails_fb1").unwrap();
        assert_eq!(fb.params["target"], Value::Str("trails_out1".into()));
        assert!(plan.warnings.iter().any(|w| w.contains("nonsense")));
    }

    #[test]
    fn the_brief_lists_every_recipe() {
        let menu = menu();
        for r in all() {
            assert!(menu.contains(&format!("- {}", r.name)), "{}", r.name);
        }
    }

    #[test]
    fn camera_examples_name_real_recipes_and_rotate() {
        for name in CAMERA_LOOKS {
            assert!(find(name).is_some(), "{name}");
        }
        let (a, b) = (camera_examples("", 0), camera_examples("", 1));
        assert!(a.contains("- blobhud:") && a.contains("PLAN:"));
        assert_ne!(a, b, "asking twice shows different worked examples");
        // Typed direction picks the examples.
        assert!(camera_examples("newspaper halftone", 0).contains(&find("halftone").unwrap().json));
    }

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
