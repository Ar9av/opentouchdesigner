//! Turning a sentence into operators.
//!
//! Two decisions carry this module.
//!
//! **The model's knowledge of the operators comes from the registry**, not
//! from a hand-written list and not from whatever it remembers about
//! TouchDesigner. [`catalogue`] is generated from the same `OpRegistry` the
//! editor builds its menus from, so an operator added tomorrow is one the
//! assistant can use tomorrow, and one that was renamed cannot be suggested
//! under its old name. The docs generator already proved the pattern; this is
//! the same trick pointed at a different reader.
//!
//! **Nothing a model says touches the graph until it has been checked.** The
//! reply is JSON, it is validated against the real registry — every operator
//! type, every parameter name, every connection endpoint — and a plan with an
//! operator that does not exist is refused whole rather than applied in part.
//! A patch that is half-built is worse than one that was not built, because
//! the user now has to work out which half.

use std::collections::BTreeMap;

use otd_core::{Graph, NodeId, OpRegistry, Value};

/// What the assistant proposes: nodes to create, wires to run between them,
/// and what to look at afterwards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub notes: String,
    pub nodes: Vec<PlannedNode>,
    pub connections: Vec<PlannedWire>,
    /// Name of the node to set as the viewer, if any.
    pub viewer: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlannedNode {
    pub name: String,
    pub op: String,
    pub pos: [f32; 2],
    /// Constant parameter values, by key.
    pub params: BTreeMap<String, Value>,
    /// Parameters to put into Expression mode, by key.
    pub expressions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlannedWire {
    pub from: String,
    pub to: String,
    pub input: usize,
}

/// What a plan did, or would do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    pub created: Vec<String>,
    pub wired: usize,
    /// Things that were skipped, with the reason. Reported rather than
    /// swallowed: a parameter the model invented is worth knowing about.
    pub warnings: Vec<String>,
}

// ------------------------------------------------------------- the catalogue

/// Every operator, compactly, for the model to read.
///
/// Terse on purpose. The full reference is a thousand lines of prose written
/// for a human; this is the same facts at a tenth of the size, which is the
/// difference between a prompt that fits alongside the user's patch and one
/// that does not.
pub fn catalogue(registry: &OpRegistry) -> String {
    let mut by_family: BTreeMap<&'static str, Vec<&otd_core::OpDef>> = BTreeMap::new();
    for def in registry.iter() {
        by_family.entry(def.family.suffix()).or_default().push(def);
    }
    let mut out = String::new();
    for (family, mut defs) in by_family {
        defs.sort_by_key(|d| d.type_name);
        out.push_str(&format!("\n## {family}\n"));
        for def in defs {
            let inputs = if def.inputs.is_empty() {
                "none".to_string()
            } else {
                def.inputs.join(",")
            };
            out.push_str(&format!("{} in[{}]", def.type_name, inputs));
            let params = (def.params)();
            for (key, p) in &params {
                out.push(' ');
                out.push_str(key);
                if let Some(menu) = &p.menu {
                    out.push_str(&format!("={{{}}}", menu.join("|")));
                } else {
                    out.push_str(&format!("={}", short_value(&p.value)));
                }
            }
            out.push('\n');
        }
    }
    out
}

fn short_value(v: &Value) -> String {
    match v {
        Value::Str(s) if s.len() > 12 => "str".into(),
        Value::Str(s) if s.is_empty() => "str".into(),
        Value::Str(s) => s.clone(),
        Value::Float(f) => format!("{f}"),
        Value::Int(i) => format!("{i}"),
        Value::Bool(b) => format!("{b}"),
        Value::Vec2(v) => format!("[{},{}]", v[0], v[1]),
        Value::Vec3(v) => format!("[{},{},{}]", v[0], v[1], v[2]),
        Value::Vec4(v) => format!("[{},{},{},{}]", v[0], v[1], v[2], v[3]),
    }
}

/// What the network being edited currently looks like, so the model can
/// extend it rather than talk past it.
///
/// The node list alone is not enough to build *on top of* a patch, which is
/// the usual request once there is anything on the canvas at all. Three
/// things are here because leaving them out produced a specific bad answer:
///
///  * **The parameters that were changed.** A model that cannot see the
///    feedback loop is already tuned to `scale 1.035` proposes its own
///    numbers over the top of the ones the user spent the session dialling
///    in. Only the settings that differ from the operator's defaults are
///    listed — the catalogue above already gives the defaults, so repeating
///    them costs prompt and says nothing.
///  * **Where the nodes are.** Without positions every plan lays out from
///    the same origin and the new chain lands on top of the old one.
///  * **What is selected.** "Make *it* run" is the common phrasing, and the
///    referent is whatever the user has clicked on.
pub fn describe(
    graph: &Graph,
    parent: NodeId,
    selected: Option<NodeId>,
    registry: &OpRegistry,
) -> String {
    let children = graph.node(parent).children.clone();
    if children.is_empty() {
        return "The network is empty.".into();
    }
    let mut out = String::from(
        "THE NETWORK AS IT STANDS\n\
         These operators already exist and are cooking. Build on them: wire \
         into them, add after them, and change their parameters. Do not \
         rebuild what is already here.\n",
    );
    for id in children {
        let node = graph.node(id);
        out.push_str(&format!(
            "- {} ({}) at [{:.0},{:.0}]",
            node.name, node.op_type, node.pos[0], node.pos[1]
        ));

        let wired: Vec<String> = node
            .inputs
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.map(|src| format!("{}<-{}", i, graph.node(src).name)))
            .collect();
        if !wired.is_empty() {
            out.push_str(&format!(" inputs: {}", wired.join(" ")));
        }

        // Only what was changed. Everything else is in the catalogue.
        let defaults = registry.get(&node.op_type).map(|def| (def.params)());
        for (key, param) in &node.params {
            if !param.expression.is_empty() {
                out.push_str(&format!(" {key}=expr({})", clip(&param.expression)));
                continue;
            }
            let unchanged = defaults
                .as_ref()
                .and_then(|d| d.get(key))
                .map(|d| d.value == param.value)
                .unwrap_or(false);
            if !unchanged {
                out.push_str(&format!(" {key}={}", context_value(&param.value)));
            }
        }

        if let Some(file) = &node.external {
            out.push_str(&format!(" component={}", clip(file)));
        }
        if node.flags.bypass {
            out.push_str(" [bypassed]");
        }
        if node.flags.render {
            out.push_str(" [output]");
        }
        if Some(id) == selected {
            out.push_str(" [SELECTED — \"it\" in the request means this node]");
        }
        out.push('\n');
    }
    out
}

/// A value as the model needs to read it back.
///
/// Unlike [`short_value`], which writes a catalogue of defaults, this is
/// reporting what the user actually set — so a file path or a menu choice is
/// worth its characters where a whole shader is not.
fn context_value(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("\"{}\"", clip(s)),
        other => short_value(other),
    }
}

/// Long text — a shader body, a path — kept to a line.
///
/// Truncated rather than dropped: knowing a glslTOP already has a shader is
/// what stops the model writing a second one, and the first line is usually
/// enough to say what it is.
fn clip(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut cut = flat.char_indices().map(|(i, _)| i);
    match cut.nth(96) {
        Some(end) => format!("{}… ({} chars)", &flat[..end], flat.chars().count()),
        None => flat,
    }
}

/// The instructions the model works to.
pub fn system_prompt(registry: &OpRegistry) -> String {
    format!(
        r#"You build patches for OpenTouchDesigner, a node-based realtime visual tool.

Reply with ONE JSON object and nothing else. No prose, no markdown fences.

{{
  "notes": "one or two sentences on what you built and which parameter to turn",
  "nodes": [
    {{"name": "noise1", "op": "noiseTOP", "pos": [-400, 0],
      "params": {{"period": 0.3, "monochrome": true}},
      "expressions": {{"translate": "absTime * 0.06"}}}}
  ],
  "connections": [{{"from": "noise1", "to": "level1", "input": 0}}],
  "viewer": "out1"
}}

RULES
- Use only operator types from the catalogue below. Never invent one.
- Use only parameter names listed for that operator. Never invent one.
- Wire same-family only: TOP to TOP, CHOP to CHOP. Converters are explicit.
- `pos` is canvas space; lay chains out left to right, about 200 apart, and
  put separate chains on different rows so the result is readable.
- `expressions` puts a parameter in Expression mode. `absTime` is seconds
  since start; `sin`, `cos`, `fit`, `clamp` and arithmetic are available.
- End a TOP chain with a nullTOP and set "viewer" to it.
- "connections" may name a node you created or one that already exists.

BUILDING ON A NETWORK THAT ALREADY EXISTS
The network as it stands is listed below the catalogue. When it is not empty,
the request is almost always "add this to what I have", not "start again".
- Wire into what is there. Name an existing node in "connections" and it is
  found; the nodes you list are only the NEW ones. Never re-create an
  operator that is already in the list to get at its output.
- Leave existing parameters alone unless the request is about them. Numbers
  already set are ones somebody dialled in.
- Read the source off the network, not out of thin air: if there is a
  moviefileinTOP, that clip is what the user means by "it", and the new
  chain hangs off that node rather than off a fresh noiseTOP.
- `pos` must not land on an existing node. Their positions are listed —
  put new work in clear space, about 200 apart, and keep the flow left to
  right.
- A node marked [SELECTED] is what "it", "this" and "that" refer to.

WHAT MAKES A GOOD-LOOKING PATCH
- Motion comes from a feedback loop, not from one operator. The shape is:
  feedbackTOP -> levelTOP (brightness ~0.96, the decay) -> transformTOP
  (scale ~1.03 or rotate ~1, the movement) -> compositeTOP with the source.
  Point the feedbackTOP's `target` parameter at the LAST node in the chain.
- Feed a loop CONTRAST, not a grey wash: a levelTOP with `blacklevel` around
  0.4 leaves only bright wisps, and that is the difference between a tunnel
  and fog.
- Colour a monochrome source by compositing it with a rampTOP on `multiply`.
  Two colours in one node beats three numbers in three.
- Deep blacks are what make bright things look bright.
- For anything a chain of operators cannot do, use a glslTOP. ALWAYS set its
  `language` parameter to "glsl" and write the `source` as a Shadertoy-style
  fragment shader:

      void mainImage(out vec4 fragColor, in vec2 fragCoord) {{
          vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;
          fragColor = vec4(0.5 + 0.5 * cos(iTime + uv.xyx + vec3(0,2,4)), 1.0);
      }}

  `iTime`, `iResolution` and `iFrame` are in scope, and helper functions are
  allowed above `mainImage`. Write GLSL, not WGSL: this is the dialect with
  a million worked examples, and a shader that does not compile is a black
  node. Keep it short and make sure every identifier is one you declared.

OPERATOR CATALOGUE
{}"#,
        catalogue(registry)
    )
}

/// The extra brief for working from a reference image, appended to the user
/// turn when there is one.
///
/// It is separate from [`system_prompt`] on purpose. The system prompt says
/// what a patch is and never changes; this says what to do with the picture
/// sitting immediately above it, and putting it next to the image is what
/// keeps "reproduce this" attached to the thing being reproduced.
///
/// The whole content of it is: look before you build, and build the *recipe*
/// rather than the picture. A model asked to match an image reaches for one
/// enormous glslTOP that hard-codes what it sees, which is a screenshot with
/// extra steps — it cannot be turned, it does not move, and it teaches the
/// person nothing about their own patch.
pub fn reverse_engineer_prompt() -> &'static str {
    r#"WORKING FROM THE REFERENCE IMAGE ABOVE

Build a patch that produces this look. Not a copy of this exact frame — the
recipe that would generate it, and frames either side of it.

Read it first, in `notes`, in one line: what is the base pattern (noise,
ramp, shape, feedback trail), what has been done to its contrast, what are
the two or three colours, and is there evidence of a feedback loop — smearing,
echoes, concentric repeats, anything that looks like the frame before it is
still visible underneath.

Then build that, out of operators:

- Match the STRUCTURE before the colour. Getting "radial, high contrast,
  trailing" right and the hue wrong is close. The reverse is not.
- Colour last, and with a rampTOP composited on `multiply` — pick the ramp
  ends off the image. Two colours in one node beats three numbers in three.
- If it smears, echoes or tunnels, that is a feedback loop, and it is the
  first thing to build, not a garnish at the end.
- Reach for a glslTOP for the base pattern when no operator chain gives it,
  and keep it to the pattern. A single shader that draws the whole image is
  the failure mode here: nothing downstream can adjust it and no parameter
  does anything.
- Leave it TURNABLE. Whoever asked for this wants to push it somewhere else
  next, so the interesting quantities belong in parameters on separate nodes,
  not baked into one shader's constants.

Where the image shows something these operators genuinely cannot do, get as
close as they do reach and say what you left out in `notes`. Do not invent an
operator to cover the gap."#
}

// --------------------------------------------------------------- the reply

/// Pull the JSON object out of a reply that may be wrapped in prose or fences.
///
/// Asking for "JSON and nothing else" gets JSON and nothing else most of the
/// time. Most is not all, and a fence is not a reason to fail.
pub fn extract_json(reply: &str) -> Result<serde_json::Value, String> {
    let trimmed = reply.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(v);
    }
    // Scan for the first balanced `{...}`, ignoring braces inside strings —
    // shader source is full of them.
    let bytes: Vec<char> = trimmed.chars().collect();
    let start = bytes.iter().position(|c| *c == '{').ok_or_else(|| {
        format!(
            "the reply was not JSON: {}",
            trimmed.chars().take(120).collect::<String>()
        )
    })?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for i in start..bytes.len() {
        let c = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let text: String = bytes[start..=i].iter().collect();
                    return serde_json::from_str(&text)
                        .map_err(|e| format!("the reply was not valid JSON: {e}"));
                }
            }
            _ => {}
        }
    }
    Err("the reply had an unterminated JSON object — the model may have been cut off".into())
}

/// Read a plan out of the JSON, checking it against the real registry.
///
/// An unknown operator type fails the whole plan: half a patch is worse than
/// none, because then the user has to work out which half.
pub fn parse_plan(value: &serde_json::Value, registry: &OpRegistry) -> Result<Plan, String> {
    let mut plan = Plan {
        notes: value
            .get("notes")
            .and_then(|n| n.as_str())
            .unwrap_or_default()
            .to_string(),
        viewer: value
            .get("viewer")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        ..Default::default()
    };

    let nodes = value
        .get("nodes")
        .and_then(|n| n.as_array())
        .ok_or("the reply had no `nodes` array")?;

    for (i, entry) in nodes.iter().enumerate() {
        let op = entry
            .get("op")
            .and_then(|o| o.as_str())
            .ok_or_else(|| format!("node {i} has no `op`"))?
            .to_string();
        let def = registry
            .get(&op)
            .ok_or_else(|| format!("`{op}` is not an operator in this build"))?;

        let name = entry
            .get("name")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("node{}", i + 1));

        let pos = entry
            .get("pos")
            .and_then(|p| p.as_array())
            .map(|p| {
                let f = |i: usize| p.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                [f(0), f(1)]
            })
            .unwrap_or([i as f32 * 200.0, 0.0]);

        // Parameters are coerced to the operator's declared type rather than
        // taken as whatever JSON they arrived as, so `1` for a float
        // parameter is 1.0 and not an int that breaks the packing.
        let declared = (def.params)();
        let mut params = BTreeMap::new();
        if let Some(map) = entry.get("params").and_then(|p| p.as_object()) {
            for (key, raw) in map {
                if let Some(param) = declared.get(key.as_str()) {
                    if let Some(v) = json_to_value(raw, &param.value) {
                        params.insert(key.clone(), v);
                    }
                }
            }
        }
        let mut expressions = BTreeMap::new();
        if let Some(map) = entry.get("expressions").and_then(|p| p.as_object()) {
            for (key, raw) in map {
                if declared.contains_key(key.as_str()) {
                    if let Some(text) = raw.as_str() {
                        expressions.insert(key.clone(), text.to_string());
                    }
                }
            }
        }

        plan.nodes.push(PlannedNode {
            name,
            op,
            pos,
            params,
            expressions,
        });
    }

    if let Some(wires) = value.get("connections").and_then(|c| c.as_array()) {
        for wire in wires {
            let from = wire.get("from").and_then(|f| f.as_str());
            let to = wire.get("to").and_then(|t| t.as_str());
            if let (Some(from), Some(to)) = (from, to) {
                plan.connections.push(PlannedWire {
                    from: from.to_string(),
                    to: to.to_string(),
                    input: wire.get("input").and_then(|i| i.as_u64()).unwrap_or(0) as usize,
                });
            }
        }
    }

    if plan.nodes.is_empty() {
        return Err("the plan had no nodes in it".into());
    }
    Ok(plan)
}

/// Coerce JSON into the parameter's declared type.
fn json_to_value(raw: &serde_json::Value, declared: &Value) -> Option<Value> {
    // An array is a vector parameter however the declared type reads.
    if let Some(items) = raw.as_array() {
        let n: Vec<f64> = items.iter().filter_map(|v| v.as_f64()).collect();
        return match (declared, n.len()) {
            (Value::Vec2(_), 2..) => Some(Value::Vec2([n[0], n[1]])),
            (Value::Vec3(_), 3..) => Some(Value::Vec3([n[0], n[1], n[2]])),
            (Value::Vec4(_), 4..) => Some(Value::Vec4([n[0], n[1], n[2], n[3]])),
            // A colour given as three numbers is opaque.
            (Value::Vec4(_), 3) => Some(Value::Vec4([n[0], n[1], n[2], 1.0])),
            _ => None,
        };
    }
    Some(match declared {
        Value::Str(_) => Value::Str(match raw.as_str() {
            Some(s) => s.to_string(),
            None => raw.to_string(),
        }),
        Value::Bool(_) => Value::Bool(raw.as_bool().or_else(|| raw.as_f64().map(|f| f != 0.0))?),
        Value::Int(_) => Value::Int(raw.as_i64().or_else(|| raw.as_f64().map(|f| f as i64))?),
        Value::Float(_) => Value::Float(raw.as_f64()?),
        Value::Vec2(_) | Value::Vec3(_) | Value::Vec4(_) => {
            // A single number broadcast across a vector: `"scale": 1.03`.
            let f = raw.as_f64()?;
            match declared {
                Value::Vec2(_) => Value::Vec2([f, f]),
                Value::Vec3(_) => Value::Vec3([f, f, f]),
                _ => Value::Vec4([f, f, f, 1.0]),
            }
        }
    })
}

// ---------------------------------------------------------------- applying

/// Build a plan into `parent`.
///
/// The caller takes an undo checkpoint first, so the whole thing reverts as
/// one gesture. Anything that cannot be done is collected as a warning rather
/// than abandoning the parts that worked — by this point the plan has already
/// been validated, so what is left are soft failures like a wire between two
/// families.
pub fn apply(
    graph: &mut Graph,
    parent: NodeId,
    registry: &OpRegistry,
    plan: &Plan,
) -> Result<(Applied, Option<NodeId>), String> {
    let mut applied = Applied::default();
    // Plan name -> the node that ended up being created for it. The graph
    // renames on collision, so this cannot be a lookup by name afterwards.
    let mut made: BTreeMap<String, NodeId> = BTreeMap::new();

    for node in &plan.nodes {
        let Some(def) = registry.get(&node.op) else {
            applied.warnings.push(format!("no operator `{}`", node.op));
            continue;
        };
        let id = match graph.create(parent, &def.clone(), Some(&node.name)) {
            Ok(id) => id,
            Err(e) => {
                applied
                    .warnings
                    .push(format!("could not create {}: {e}", node.name));
                continue;
            }
        };
        graph.node_mut_quiet(id).pos = node.pos;
        made.insert(node.name.clone(), id);
        applied.created.push(graph.node(id).name.clone());

        for (key, value) in &node.params {
            if let Err(e) = graph.set_param(id, key, value.clone()) {
                applied
                    .warnings
                    .push(format!("{}.{key}: {e}", graph.node(id).name));
            }
        }
        for (key, source) in &node.expressions {
            if let Err(e) = graph.set_expression(id, key, source) {
                applied
                    .warnings
                    .push(format!("{}.{key}: {e}", graph.node(id).name));
            }
        }
    }

    let resolve = |graph: &Graph, made: &BTreeMap<String, NodeId>, name: &str| -> Option<NodeId> {
        made.get(name).copied().or_else(|| {
            // A plan may wire into what was already there, by name or path.
            graph
                .find_from(parent, name)
                .or_else(|| graph.find_from(parent, name.trim_start_matches('/')))
        })
    };

    for wire in &plan.connections {
        let (Some(from), Some(to)) = (
            resolve(graph, &made, &wire.from),
            resolve(graph, &made, &wire.to),
        ) else {
            applied
                .warnings
                .push(format!("no node named {} or {}", wire.from, wire.to));
            continue;
        };
        match graph.connect(from, to, wire.input) {
            Ok(()) => applied.wired += 1,
            Err(e) => applied
                .warnings
                .push(format!("{} -> {}: {e}", wire.from, wire.to)),
        }
    }

    // A Feedback TOP's target is a path, and the model wrote it before the
    // nodes had paths. Rewrite bare names into real ones.
    for (name, id) in &made {
        let _ = name;
        let target = graph.node(*id).param("target").map(|p| p.value.as_str());
        if let Some(target) = target {
            let bare = target.trim().trim_start_matches('/');
            if !bare.is_empty() {
                if let Some(node) = made.get(bare) {
                    let path = graph.path(*node);
                    let _ = graph.set_param(*id, "target", Value::Str(path));
                }
            }
        }
    }

    let viewer = plan
        .viewer
        .as_ref()
        .and_then(|name| resolve(graph, &made, name));
    Ok((applied, viewer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use otd_core::indexmap::IndexMap;
    use otd_core::{Connector, Family, OpDef, Param};

    /// A two-operator registry of this crate's own. The real one lives in
    /// `otd-engine`, which depends on the GPU; validation logic should not.
    fn test_registry() -> OpRegistry {
        fn pass_params() -> IndexMap<String, Param> {
            let mut m = IndexMap::new();
            m.insert("gain".into(), Param::float(1.0).with_label("Gain"));
            m.insert(
                "mode".into(),
                Param::menu("over", &["over", "add"]).with_label("Mode"),
            );
            m.insert("size".into(), Param::new(Value::Vec2([1.0, 1.0])));
            m
        }
        let mut r = OpRegistry::new();
        r.register(OpDef {
            type_name: "pass",
            input_families: &[],
            label: "Pass",
            family: Family::Top,
            inputs: &["in"],
            summary: "",
            time_dependent: false,
            params: pass_params,
            connector: Connector::None,
        });
        r
    }

    fn plan_json() -> serde_json::Value {
        serde_json::json!({
            "notes": "a chain",
            "nodes": [
                { "name": "a", "op": "pass", "pos": [0, 0] },
                { "name": "b", "op": "pass", "pos": [200, 0] }
            ],
            "connections": [{ "from": "a", "to": "b", "input": 0 }],
            "viewer": "b"
        })
    }

    #[test]
    fn json_survives_prose_and_fences_around_it() {
        let bare = r#"{"nodes":[{"op":"pass"}]}"#;
        assert!(extract_json(bare).is_ok());
        assert!(extract_json(&format!("```json\n{bare}\n```")).is_ok());
        assert!(extract_json(&format!("Sure! Here you go:\n\n{bare}\n\nEnjoy.")).is_ok());

        // Braces inside a string — shader source is full of them — must not
        // end the object early.
        let shady = r#"{"nodes":[{"op":"pass","params":{"source":"fn f() { return 1; }"}}]}"#;
        let v = extract_json(&format!("here:\n{shady}")).unwrap();
        assert_eq!(v["nodes"][0]["params"]["source"], "fn f() { return 1; }");

        assert!(extract_json("no json at all").is_err());
        assert!(
            extract_json(r#"{"nodes": ["#)
                .unwrap_err()
                .contains("cut off")
        );
    }

    #[test]
    fn an_invented_operator_fails_the_whole_plan() {
        let reg = test_registry();
        let bad = serde_json::json!({
            "nodes": [
                { "name": "a", "op": "pass" },
                { "name": "b", "op": "quantumBlurTOP" }
            ]
        });
        // Not "apply the one that works": half a patch is worse than none.
        let e = parse_plan(&bad, &reg).unwrap_err();
        assert!(e.contains("quantumBlurTOP"), "{e}");
    }

    #[test]
    fn invented_parameters_are_dropped_rather_than_set() {
        let reg = test_registry();
        let value = serde_json::json!({
            "nodes": [{
                "name": "a", "op": "pass",
                "params": { "gain": 2.0, "flux_capacitance": 11 }
            }]
        });
        let plan = parse_plan(&value, &reg).unwrap();
        let params = &plan.nodes[0].params;
        assert!(params.contains_key("gain"), "{params:?}");
        assert!(!params.contains_key("flux_capacitance"));
    }

    #[test]
    fn values_are_coerced_to_the_parameters_declared_type() {
        // A float parameter given the integer 1 must become 1.0, and a vec2
        // given a single number must broadcast — both are what a model
        // actually emits.
        assert_eq!(
            json_to_value(&serde_json::json!(1), &Value::Float(0.0)),
            Some(Value::Float(1.0))
        );
        assert_eq!(
            json_to_value(&serde_json::json!(1.03), &Value::Vec2([0.0, 0.0])),
            Some(Value::Vec2([1.03, 1.03]))
        );
        assert_eq!(
            json_to_value(&serde_json::json!([0.1, 0.2, 0.3]), &Value::Vec4([0.0; 4])),
            Some(Value::Vec4([0.1, 0.2, 0.3, 1.0])),
            "a colour given as rgb is opaque"
        );
        assert_eq!(
            json_to_value(&serde_json::json!(1), &Value::Bool(false)),
            Some(Value::Bool(true))
        );
    }

    #[test]
    fn a_plan_builds_nodes_and_wires_them() {
        let reg = test_registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let plan = parse_plan(&plan_json(), &reg).unwrap();
        let (applied, viewer) = apply(&mut graph, root, &reg, &plan).unwrap();

        assert_eq!(applied.created, vec!["a", "b"]);
        assert_eq!(applied.wired, 1);
        assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);
        assert_eq!(viewer, graph.find("/b"));
        let b = graph.find("/b").unwrap();
        assert_eq!(graph.node(b).inputs[0], graph.find("/a"));
    }

    #[test]
    fn a_name_that_is_already_taken_is_renamed_and_still_wired() {
        let reg = test_registry();
        let mut graph = Graph::new();
        let root = graph.root();
        // Apply the same plan twice. The second must not fail, must not
        // overwrite the first, and its wire must join *its own* nodes.
        let plan = parse_plan(&plan_json(), &reg).unwrap();
        apply(&mut graph, root, &reg, &plan).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();

        assert_eq!(applied.wired, 1, "{:?}", applied.warnings);
        assert_ne!(applied.created, vec!["a", "b"], "renamed on collision");
        let second = graph.find(&format!("/{}", applied.created[1])).unwrap();
        let first_of_second = graph.find(&format!("/{}", applied.created[0]));
        assert_eq!(graph.node(second).inputs[0], first_of_second);
    }

    #[test]
    fn the_catalogue_is_generated_from_the_registry() {
        let reg = test_registry();
        let text = catalogue(&reg);
        for def in reg.iter() {
            assert!(text.contains(def.type_name), "{} missing", def.type_name);
        }
        // And it lists parameters, or the model has to guess them.
        assert!(text.contains("gain"), "{text}");
    }

    #[test]
    fn describing_a_network_lists_what_is_wired_to_what() {
        let reg = test_registry();
        let mut graph = Graph::new();
        let root = graph.root();
        assert!(describe(&graph, root, None, &reg).contains("empty"));

        let plan = parse_plan(&plan_json(), &reg).unwrap();
        apply(&mut graph, root, &reg, &plan).unwrap();
        let text = describe(&graph, root, None, &reg);
        assert!(text.contains("a (pass)"), "{text}");
        assert!(text.contains("0<-a"), "{text}");
    }

    #[test]
    fn a_description_carries_what_was_tuned_and_what_is_selected() {
        // Building on a patch means seeing it. A model that cannot read the
        // numbers already dialled in proposes its own over the top of them,
        // and one that cannot tell which node is selected has nothing to
        // attach "make it move" to.
        let reg = test_registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let plan = parse_plan(&plan_json(), &reg).unwrap();
        apply(&mut graph, root, &reg, &plan).unwrap();

        let tuned = graph.find_from(root, "a").unwrap();
        graph.set_param(tuned, "gain", Value::Float(0.35)).unwrap();
        let text = describe(&graph, root, Some(tuned), &reg);

        assert!(text.contains("gain=0.35"), "{text}");
        assert!(text.contains("[SELECTED"), "{text}");
        // Positions are what keep a new chain off the top of an old one.
        assert!(text.contains(" at ["), "{text}");
        // A default is in the catalogue already; repeating it is prompt
        // spent to say nothing.
        let untouched = describe(&graph, root, None, &reg);
        assert!(!untouched.contains("gain=1"), "{untouched}");
    }

    #[test]
    fn a_shader_that_broke_the_json_is_reported_rather_than_swallowed() {
        // Both ways a model mangles JSON do it in the same place: a long
        // shader source, where an escape gets dropped. These have to come
        // back as errors, because the error is what gets handed to the model
        // to fix — see `complete_with_repair`. A parse that quietly returned
        // something half-read would skip the retry entirely.
        let unescaped_quote = "{\"nodes\":[{\"params\":{\"source\":\"// uses \"iTime\"\"}}]}";
        let err = extract_json(unescaped_quote).unwrap_err();
        assert!(err.contains("not valid JSON"), "{err}");

        let literal_newline = "{\"nodes\":[{\"params\":{\"source\":\"void main() {\nreturn;\n}\"}}]}";
        let err = extract_json(literal_newline).unwrap_err();
        assert!(err.contains("not valid JSON"), "{err}");

        // Properly escaped, the same shader is fine — the brace scanner must
        // not be fooled by the braces inside the string.
        let good = "{\"nodes\":[{\"params\":{\"source\":\"void main() {\\n  return;\\n}\"}}]}";
        assert!(extract_json(good).is_ok());
    }

    #[test]
    fn long_text_is_clipped_rather_than_dropped() {
        // Knowing a node already has a shader is what stops the model
        // writing a second one — but the whole body would crowd out the
        // patch it is meant to be extending.
        let long = "x".repeat(400);
        let out = clip(&long);
        assert!(out.contains("400 chars"), "{out}");
        assert!(out.chars().count() < 140, "{out}");
        // Short values pass through untouched.
        assert_eq!(clip("noise1"), "noise1");
    }
}

// ------------------------------------------------------------ shader checks

/// A shader compiler, supplied by the caller.
///
/// `otd-ai` must not depend on the GPU crate — the whole point of it having
/// no GPU dependency is that key handling and validation are testable without
/// one — so the compiler arrives as a function pointer instead. The editor
/// passes naga's front end, which is the same one that would reject the
/// shader a moment later on the node.
pub type ShaderCheck = fn(source: &str, is_glsl: bool) -> Result<(), String>;

/// The operators whose `source` parameter holds a shader, lowercased.
///
/// `source` is not a shader-only parameter name — the script DATs keep their
/// Python in one — and a model will occasionally hang a stray `source` on an
/// operator that has no such parameter at all. Neither is WGSL, and neither
/// should be compiled as if it were.
const SHADER_OPS: &[&str] = &["glsltop"];

/// Every shader in a reply that will not compile, as `(node, error)`.
///
/// Worth doing *before* the nodes exist. A model that writes a shader
/// referencing something it invented produces a node that outlines red and an
/// output that is silently black — the patch looks built and is not.
///
/// Keyed on the node's operator, not on it having a `source`: a stray `source`
/// on a levelTOP is a parameter [`apply`] drops on the floor, and compiling it
/// as WGSL buys nothing but a repair round trip the model cannot act on.
pub fn shader_problems(value: &serde_json::Value, check: ShaderCheck) -> Vec<(String, String)> {
    let mut problems = Vec::new();
    let Some(nodes) = value.get("nodes").and_then(|n| n.as_array()) else {
        return problems;
    };
    for (i, node) in nodes.iter().enumerate() {
        let is_shader = node
            .get("op")
            .and_then(|o| o.as_str())
            .map(|op| SHADER_OPS.contains(&op.to_lowercase().as_str()))
            .unwrap_or(false);
        if !is_shader {
            continue;
        }
        let Some(params) = node.get("params").and_then(|p| p.as_object()) else {
            continue;
        };
        let Some(source) = params.get("source").and_then(|s| s.as_str()) else {
            continue;
        };
        if source.trim().is_empty() {
            continue;
        }
        let is_glsl = params
            .get("language")
            .and_then(|l| l.as_str())
            .map(|l| l.eq_ignore_ascii_case("glsl"))
            .unwrap_or(false);
        if let Err(e) = check(source, is_glsl) {
            let name = node
                .get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("node{}", i + 1));
            problems.push((name, e));
        }
    }
    problems
}

/// Nodes a plan creates that nothing reads and nothing looks at.
///
/// A model asked to add to a patch will sometimes build a whole second chain
/// and forget to join it on. The nodes are real, they cook, and they do
/// nothing — worth saying so rather than leaving them to be found.
pub fn dangling(plan: &Plan) -> Vec<String> {
    plan.nodes
        .iter()
        .filter(|node| {
            let read = plan.connections.iter().any(|w| w.from == node.name);
            let viewed = plan.viewer.as_deref() == Some(node.name.as_str());
            // A node that feeds nothing and is not the output. Feedback is
            // exempt: it is read by its `target`, not by a wire.
            !read && !viewed && !node.op.to_lowercase().contains("feedback")
        })
        .map(|node| node.name.clone())
        .collect()
}

#[cfg(test)]
mod shader_tests {
    use super::*;

    /// Stands in for naga: anything mentioning `ghost` is undefined.
    fn fake_check(source: &str, _is_glsl: bool) -> Result<(), String> {
        if source.contains("ghost") {
            Err("no definition in scope for `ghost`".into())
        } else {
            Ok(())
        }
    }

    #[test]
    fn a_shader_that_will_not_compile_is_found_before_the_node_exists() {
        // The failure this catches is the nasty one: the patch looks built,
        // the node outlines red, and the output is silently black.
        let value = serde_json::json!({
            "nodes": [
                { "name": "zig1", "op": "glslTOP",
                  "params": { "source": "return vec4f(ghost, 0.0, 0.0, 1.0);" } },
                { "name": "ok1", "op": "glslTOP",
                  "params": { "source": "return vec4f(1.0);" } }
            ]
        });
        let problems = shader_problems(&value, fake_check);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].0, "zig1");
        assert!(problems[0].1.contains("ghost"));
    }

    #[test]
    fn nodes_without_shaders_are_not_checked() {
        let value = serde_json::json!({
            "nodes": [
                { "name": "a", "op": "noiseTOP", "params": { "period": 0.3 } },
                { "name": "b", "op": "glslTOP", "params": { "source": "  " } },
                { "name": "c", "op": "textDAT", "params": { "text": "ghost" } }
            ]
        });
        // `c` has a `text` parameter, not a `source` one — checking every
        // string a model wrote would reject prose.
        assert!(shader_problems(&value, fake_check).is_empty());
    }

    #[test]
    fn a_stray_source_param_on_a_non_shader_operator_is_not_checked() {
        // Seen in the wild: the model hangs a `source` on a levelTOP, the
        // string is compiled as WGSL, and a syntax error nobody can act on
        // costs a repair round trip — the model quite correctly answers that
        // the patch has no glslTOP in it. `apply` drops the parameter anyway.
        let value = serde_json::json!({
            "nodes": [
                { "name": "threshold1", "op": "levelTOP",
                  "params": { "source": "ghost" } },
                { "name": "callbacks1", "op": "executeDAT",
                  "params": { "source": "def onCook(dat):\n\tghost()\n" } }
            ]
        });
        assert!(shader_problems(&value, fake_check).is_empty());
    }

    #[test]
    fn a_chain_the_model_forgot_to_join_on_is_reported() {
        // Exactly what a model does when asked to add to an existing patch:
        // builds a whole second loop and never wires it in.
        let plan = Plan {
            nodes: vec![
                node("a", "noiseTOP"),
                node("b", "levelTOP"),
                node("out1", "nullTOP"),
                node("fb2", "feedbackTOP"),
                node("decay2", "levelTOP"),
            ],
            connections: vec![wire("a", "b"), wire("b", "out1"), wire("fb2", "decay2")],
            viewer: Some("out1".into()),
            ..Default::default()
        };
        let loose = dangling(&plan);
        // `decay2` feeds nothing and is not the output. `fb2` feeds decay2,
        // and `out1` is the viewer, so neither is loose.
        assert_eq!(loose, vec!["decay2"]);
    }

    #[test]
    fn a_feedback_node_is_never_called_loose() {
        // Feedback is read through its `target` parameter, not a wire, so
        // the usual "nothing reads it" test would libel it.
        let plan = Plan {
            nodes: vec![node("fb1", "feedbackTOP")],
            ..Default::default()
        };
        assert!(dangling(&plan).is_empty());
    }

    fn node(name: &str, op: &str) -> PlannedNode {
        PlannedNode {
            name: name.into(),
            op: op.into(),
            pos: [0.0, 0.0],
            params: BTreeMap::new(),
            expressions: BTreeMap::new(),
        }
    }

    fn wire(from: &str, to: &str) -> PlannedWire {
        PlannedWire {
            from: from.into(),
            to: to.into(),
            input: 0,
        }
    }
}
