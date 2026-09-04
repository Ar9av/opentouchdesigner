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
    /// Operators already in the network to retune. Separate from `nodes`
    /// because they are not being created: naming an existing operator under
    /// `nodes` used to get you a renamed duplicate beside the original and the
    /// original left exactly as it was, which is the opposite of what "make
    /// this slower" means.
    pub sets: Vec<PlannedSet>,
    /// Operators already in the network to remove, by name. Only direct
    /// children of the network being edited — see [`apply`].
    pub deletes: Vec<String>,
    /// Name of the node to set as the viewer, if any.
    pub viewer: Option<String>,
}

/// A change to an operator that already exists.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedSet {
    pub node: String,
    /// Left as raw JSON: coercing a value needs the parameter's declared type,
    /// and that comes off the node in the graph, which the parser cannot see.
    /// [`apply`] does it once it has both.
    pub params: BTreeMap<String, serde_json::Value>,
    pub expressions: BTreeMap<String, String>,
    pub exports: BTreeMap<String, String>,
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
    /// Parameters to put into Export mode, by key: `node:channel`.
    pub exports: BTreeMap<String, String>,
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
    /// Parameters retuned on operators that already existed, as `node.key`.
    pub changed: Vec<String>,
    /// Operators removed, by the name they had.
    pub removed: Vec<String>,
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
  "set": [{{"node": "level1", "params": {{"brightness": 0.9}},
            "exports": {{"contrast": "lag1:r"}}}}],
  "delete": ["blur2", "oldramp1"],
  "viewer": "out1"
}}

`nodes` creates, `set` retunes what is already there, `delete` removes it.
All four lists are optional; a plan may be nothing but a `set`.

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
- To CHANGE one of those numbers, use `set` — do not list the operator under
  `nodes`. A name that already exists gets renamed rather than overwritten,
  so `nodes` would leave you a duplicate beside the original and the original
  untouched.
- To REMOVE an operator, name it in `delete`. Only operators in the list
  below, and only ones you did not just create. When you delete out of the
  middle of a chain its input is spliced into whatever it fed, so the chain
  stays joined without you rewiring it.

WHEN THE REQUEST IS TO SIMPLIFY
"Make it simpler", "clean this up", "that is too much" means fewer operators
doing the same job — a `delete` list, sometimes with a `set` to compensate.
- Say in `notes` what you took out and why it was safe.
- Take out whole redundant branches rather than one node from each. Two
  chains compositing into the same node where one would do is the usual find.
- Do NOT delete the source. A moviefileinTOP, a camera, an audiofileinTOP:
  that is the material, and a patch without it has nothing to process.
- Do NOT delete the node the viewer is set to, or the last nullTOP in a
  chain. If a chain must lose its end, wire the new end up in `connections`.
- Deleting everything is refused. If the patch really is beyond saving, say
  so in `notes` and suggest `/clear`.
- Read the source off the network, not out of thin air: if there is a
  moviefileinTOP, that clip is what the user means by "it", and the new
  chain hangs off that node rather than off a fresh noiseTOP.
- `pos` must not land on an existing node. Their positions are listed —
  put new work in clear space, about 200 apart, and keep the flow left to
  right.
- IF THE NETWORK ALREADY ENDS IN A nullTOP, THAT IS THE OUTPUT. Insert your
  chain in FRONT of it — wire the last operator you add into that existing
  null — and set `viewer` to it. Do not add a second nullTOP beside it. A
  patch with `out1` still showing the raw clip and your work parked on an
  `out2` nobody is looking at is the commonest way an answer that is
  entirely correct still shows the user no change at all.
- A node marked [SELECTED] is what "it", "this" and "that" refer to.

OPERATORS NAMED BY PARAMETER, NOT BY WIRE
Some operators reach another one through a parameter holding its name rather
than through a connection. Write the plain node name; it is turned into a real
path for you. These are easy to forget and the patch looks built without them:

- `feedbackTOP.target` — the node whose last frame comes back round.
- `selectTOP.top` — pull any TOP's output in from anywhere, no wire.
- `renderTOP.geometry`, `.camera`, `.light` — a Render TOP has NO inputs.
  It renders the Geometry COMP, Camera COMP and Light COMP you name here.
  Leave any of them blank and you get an empty frame.
- `geometryCOMP.sop` — which SOP it draws. `.material` — which MAT shades it.
- `geometryCOMP.instancechop` — one instance per sample of that CHOP, with
  `instancing` true. `tx`/`ty`/`tz` name which of its channels are position.
- `replicatorCOMP.master` / `.template`.

So the shape of a 3D patch is four nodes that reference each other and only
one wire between them:

  torus1 (torusSOP)                       geo1.sop        = torus1
  geo1   (geometryCOMP)                   render1.geometry = geo1
  cam1   (cameraCOMP), light1 (lightCOMP) render1.camera   = cam1
  render1 (renderTOP) -> out1             render1.light    = light1

MAKING A PATCH REACT TO SOMETHING
`expressions` animates on a clock, and a clock is not a reaction — the patch
does the same thing whether anybody is in front of it or not. Reacting is
`exports`: a parameter reads a live channel off a CHOP, which is the same
mechanism as dragging a channel onto a parameter in the editor.

    "exports": {{"scale": "lag1:r"}}

The value is `node:channel`. The node is looked up like any other name, so it
can be one you created in this same plan. ONLY A CHOP HAS CHANNELS — exporting
from a TOP is refused. Use `average` on a Top To CHOP and the channels are
`r`, `g`, `b`, `a`.

To react to a CAMERA or a MOVIE — someone moving in frame, the picture getting
brighter — the chain is:

  the picture -> toptochopCHOP (layout "average", so it is one sample, not
  a million) -> analyzeCHOP (function "average") -> lagCHOP -> exports

MOVEMENT specifically is not brightness, it is *change* in brightness, so the
picture you measure has to be a difference rather than the camera itself:

  camera1 ------------------------> compositeTOP (operation "difference")
  camera1 -> feedbackTOP (target that compositeTOP) --^

That composite is near-black when nothing moves and bright where something
did. Measure THAT, not the camera. Then:
- `lagCHOP` is what turns a twitchy number into something worth watching. Set
  `lagup` small and `lagdown` large — fast to rise, slow to fall — or every
  parameter you drive will jitter.
- The channel out of `analyzeCHOP` on a difference image is SMALL, often
  under 0.05. Put a `mathCHOP` after it and scale it up, or nothing visibly
  moves and the patch looks broken.
- Export to two or three parameters, not ten. `transformTOP.scale`,
  `levelTOP.brightness` and a `hsvadjustTOP` hue are the ones that read.

The same chain with an audiodeviceinCHOP in place of the picture is how a
patch reacts to sound; there is no Top To CHOP in that one because audio is
already channels.

WHAT MAKES A GOOD-LOOKING PATCH
- Motion comes from a feedback loop, not from one operator. The shape is:
  feedbackTOP -> levelTOP (brightness ~0.96, the decay) -> transformTOP
  (scale ~1.03 or rotate ~1, the movement) -> compositeTOP with the source.
  Point the feedbackTOP's `target` parameter at the LAST node in the chain.

- A LOOP THAT ADDS FULL-BRIGHTNESS VIDEO SATURATES TO WHITE. `add` and
  `screen` accumulate, and a loop that keeps `brightness` of what it had
  settles at roughly `source / (1 - brightness)` — so decay 0.95 with a
  bright clip going in every frame is twenty times the clip, which is solid
  white within a second. The wisps recipe above gets away with 0.96 because
  what it feeds in is a few dim highlights, not a photograph.
  Feeding a CAMERA or a MOVIE into an `add` loop, do one of these:
  - put a levelTOP on the SOURCE before the composite, `brightness` 0.1-0.2,
    so only a suggestion of each frame enters the loop; or
  - decay much harder — `brightness` 0.80-0.88 on the loop's own levelTOP; or
  - use `maximum`, which keeps the brightest of the two and cannot run away.
  Pick ONE of those three. Doing two at once kills the loop instead of
  taming it, and the arithmetic says exactly why: a source dimmed to 0.15
  arrives below a `blacklevel` of 0.35, so the decay stage crushes it to
  zero on its first lap and the loop starves. Whatever `blacklevel` the
  loop's levelTOP has must be BELOW the brightness the source enters at, or
  the source might as well not be wired in. If in doubt leave `blacklevel`
  at 0 and simply dim the source — that always works.

- THE COMPOSITE THAT CLOSES A FEEDBACK LOOP MUST NOT BE `over`. A
  compositeTOP's inputs are `base` then `over`, and input 1 is drawn over
  input 0. Video and most generators are fully opaque, so `over` means input
  1 wins outright and input 0 contributes NOTHING. Wire the source to input 0
  and the feedback to input 1 with the default operation and you have built a
  loop the source cannot get into: it looks right for about a second, then
  the decay eats what was already circulating and the whole thing fades to
  black. Use `add`, `screen` or `maximum`, all of which let both sides
  through. This is the single most common way a generated feedback patch
  dies, and it dies slowly enough to look fine at first.
  A feedbackTOP has NO INPUTS — it reads through `target`, so never list a
  connection into one. Wiring `x -> feedback1` is rejected as an input index
  out of range and the loop silently does not close.
- Feed a loop CONTRAST, not a grey wash: a levelTOP with `blacklevel` around
  0.4 leaves only bright wisps, and that is the difference between a tunnel
  and fog.
- Colour a monochrome source by compositing it with a rampTOP on `multiply`.
  Two colours in one node beats three numbers in three.
- Deep blacks are what make bright things look bright.
- Some looks have their own operator and do NOT need a shader. Reach for
  these before a glslTOP, because they have parameters somebody can turn:
  - `ditherTOP` — retro/1-bit/newsprint/posterised. `levels` 2-4 and
    `pattern` bayer4 is the crosshatch look; `scale` above 1 makes it chunky.
  - `voronoiTOP` — cells, cracked glass, organic tiling. A generator.
    `output` edges is the crack pattern, `distance` is what to feed a
    displaceTOP with.
  - `toonTOP` — cel shading, comic, flat-colour. Bands the luminance and
    inks the edges in one node; do not build it from levelTOP and edgeTOP.
  - `flowTOP` — smoke, ink in water, drifting abstraction. ONE flowTOP is
    only a warp. The look comes from looping it:
    `source -> flow1 -> composite -> out`, with a feedbackTOP targeting the
    composite wired back into it. Wire a TOP into the flowTOP's second
    input and set `usefield` to steer the flow with a picture.
  - `blendSOP` — morphing one shape into another. Two SOPs in, `blend` 0..1.
    Animate `blend` with an expression or export to make the morph move.
  - `renderTOP` with `output` set to depth gives distance from the camera as
    grey, near white and far black. Feed it to a displaceTOP for parallax,
    or composite it on multiply for fog. `depthnear`/`depthfar` are in scene
    units and are what actually decide whether it looks like anything.

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

- A glslTOP READS ITS INPUTS as `iChannel0` (input 0) and `iChannel1`
  (input 1), exactly as Shadertoy does. This is the ONLY way to get the
  incoming picture, and it is what "an effect on top of the video" means —
  a shader that never mentions `iChannel0` has thrown the clip away and
  drawn over the top of it:

      void mainImage(out vec4 fragColor, in vec2 fragCoord) {{
          vec2 uv = fragCoord / iResolution.xy;
          uv.x += 0.02 * sin(uv.y * 20.0 + iTime * 2.0);
          fragColor = texture(iChannel0, uv);
      }}

  Declare nothing to get them: `iChannel0` is already there, and writing
  `uniform sampler2D iChannel0;` yourself is a compile error.

  These are TouchDesigner's names, and this is not TouchDesigner. None of
  them exist here and every one of them is a black node:
  `sTD2DInputs[...]`, `sIn0`, `vUV`, `uniform1`, `uTD*`. Nor does the
  ancient `texture2D(...)` — the function is `texture(...)`.

  The node's `uniform1` .. `uniform4` parameters arrive in the shader as
  `U.p0` .. `U.p3`, four vec4s — `U.p0.x` is the first component of
  `uniform1`. Use them for the one or two quantities somebody will want to
  turn, and set their starting values in `params`. Everything else is a
  `const float` at the top of the source.

OPERATOR CATALOGUE
{}"#,
        catalogue(registry)
    )
}

/// Appended to the system prompt when the editor has removal switched off.
///
/// The prompt above teaches `delete` unconditionally, because most of the time
/// it is the right answer — "make it simpler" is a delete list. But a delete
/// is the one thing a plan does that the user cannot see coming, so the editor
/// makes it opt-in and this is what it says when it is off. Belt and braces:
/// the caller drops any `delete` that comes back anyway, since a rule in a
/// prompt is a request rather than a guarantee.
pub const NO_DELETE_RULE: &str = "\n\nREMOVAL IS OFF FOR THIS REQUEST\n\
     Do not use `delete`. Every operator now in the network stays. Add to it, \
     rewire it, retune it with `set` — but remove nothing, and do not work \
     around this by emptying an operator out or wiring it to nothing. If what \
     was asked for can only be done by removing something, do the part that \
     can be done and say in `notes` what would have to go and that removal is \
     switched off.";

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

    // Optional: a plan that only retunes or only deletes creates nothing, and
    // demanding an empty array for it is a rule the model has to remember for
    // no gain. The "nothing in it at all" check is at the bottom.
    let nodes = value
        .get("nodes")
        .and_then(|n| n.as_array())
        .cloned()
        .unwrap_or_default();

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

        let exports = string_map(entry.get("exports"));

        plan.nodes.push(PlannedNode {
            name,
            op,
            pos,
            params,
            expressions,
            exports,
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

    if let Some(sets) = value.get("set").and_then(|s| s.as_array()) {
        for entry in sets {
            let Some(node) = entry.get("node").and_then(|n| n.as_str()) else {
                continue;
            };
            let params = entry
                .get("params")
                .and_then(|p| p.as_object())
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            plan.sets.push(PlannedSet {
                node: node.to_string(),
                params,
                expressions: string_map(entry.get("expressions")),
                exports: string_map(entry.get("exports")),
            });
        }
    }

    if let Some(gone) = value.get("delete").and_then(|d| d.as_array()) {
        for entry in gone {
            if let Some(name) = entry.as_str() {
                plan.deletes.push(name.to_string());
            }
        }
    }

    // A plan used to have to build something to be a plan. It no longer does:
    // "delete the second feedback loop" and "slow the zoom down" are complete
    // requests that create nothing, and rejecting them sent the model back to
    // invent a node it did not need.
    if plan.nodes.is_empty() && plan.sets.is_empty() && plan.deletes.is_empty() {
        return Err("the plan had nothing in it — no nodes, no changes, no deletions".into());
    }
    Ok(plan)
}

/// A JSON object of strings, or nothing. Used for `expressions` and
/// `exports`, which are both `{parameter: text}`.
fn string_map(value: Option<&serde_json::Value>) -> BTreeMap<String, String> {
    value
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Split an export target — `analyze1:r`, or `/path/to/lag1:chan1`.
///
/// The node part is resolved against the network like any other name, so a
/// plan can export from a CHOP it created this turn or one that was already
/// there. Everything after the LAST colon is the channel, so a path
/// containing one does not confuse it.
fn split_export(target: &str) -> Option<(&str, &str)> {
    let (node, channel) = target.trim().rsplit_once(':')?;
    let (node, channel) = (node.trim(), channel.trim());
    (!node.is_empty() && !channel.is_empty()).then_some((node, channel))
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

    // Parameters that name another operator hold a *path*, and the model
    // wrote them before any node had one — and before the graph renamed
    // anything that collided. Rewrite bare names into real paths.
    //
    // Every path-reference parameter, not just a Feedback TOP's `target`,
    // which is all this used to cover. The others are `renderTOP.camera`,
    // `.light` and `.geometry`, `geometryCOMP.sop`, `.material` and
    // `.instancechop`, `selectTOP.top`, `replicatorCOMP.master` — which is
    // to say the entire 3D and instancing half of the program. A plan that
    // built a torus, a camera and a light wired nothing up wrong; it just
    // pointed the Render TOP at `cam1` when the node had become `/cam1`, and
    // rendered an empty frame. `is_path_ref` is the registry's own marker for
    // these, so operators added later are covered without a list here.
    let touched: Vec<NodeId> = made.values().copied().collect();
    for id in touched {
        let refs: Vec<(String, String)> = graph
            .node(id)
            .params
            .iter()
            .filter(|(_, p)| p.is_path_ref())
            .map(|(key, p)| (key.clone(), p.value.as_str()))
            .collect();
        for (key, value) in refs {
            let bare = value.trim().trim_start_matches('/');
            if bare.is_empty() {
                continue;
            }
            if let Some(node) = made.get(bare) {
                let path = graph.path(*node);
                let _ = graph.set_param(id, &key, Value::Str(path));
            }
        }
    }

    // ---- point parameters at CHOP channels
    //
    // After the wiring, and after every node exists, because an export names
    // an operator by path and the whole CHOP chain it reads from is usually
    // being created by this same plan.
    let mut export_targets: Vec<(NodeId, String, String)> = Vec::new();
    for node in &plan.nodes {
        if let Some(id) = made.get(&node.name) {
            for (key, target) in &node.exports {
                export_targets.push((*id, key.clone(), target.clone()));
            }
        }
    }
    for change in &plan.sets {
        if let Some(id) = resolve(graph, &made, &change.node) {
            for (key, target) in &change.exports {
                export_targets.push((id, key.clone(), target.clone()));
            }
        }
    }
    for (id, key, target) in export_targets {
        let Some((source, channel)) = split_export(&target) else {
            applied.warnings.push(format!(
                "{}.{key}: `{target}` is not `node:channel`",
                graph.node(id).name
            ));
            continue;
        };
        let Some(source_id) = resolve(graph, &made, source) else {
            applied.warnings.push(format!(
                "{}.{key}: no operator named {source} to export from",
                graph.node(id).name
            ));
            continue;
        };
        // Export reads channels, and only a CHOP has any. Pointing a
        // parameter at a TOP silently reads nothing and leaves a node that
        // does not move with no clue why — the same class of failure as the
        // black shader, so it is reported rather than accepted.
        if graph.node(source_id).family != otd_core::Family::Chop {
            applied.warnings.push(format!(
                "{}.{key}: {source} is a {} — only a CHOP has channels to export",
                graph.node(id).name,
                graph.node(source_id).family.suffix()
            ));
            continue;
        }
        let path = graph.path(source_id);
        match graph.set_export(id, &key, &path, channel) {
            Ok(()) => applied.changed.push(format!("{}.{key}", graph.node(id).name)),
            Err(e) => applied
                .warnings
                .push(format!("{}.{key}: {e}", graph.node(id).name)),
        }
    }

    // ---- retune what already exists
    //
    // After the wiring, so a plan can create a node, join it on and set a
    // number on the operator it now feeds, all in one answer.
    for change in &plan.sets {
        let Some(id) = resolve(graph, &made, &change.node) else {
            applied
                .warnings
                .push(format!("no node named {} to change", change.node));
            continue;
        };
        for (key, raw) in &change.params {
            // The declared type comes off the node, which is the only place
            // it exists — `set` names an operator the model did not create
            // and whose type it is only asserting.
            let Some(declared) = graph.node(id).param(key).map(|p| p.value.clone()) else {
                applied
                    .warnings
                    .push(format!("{} has no parameter `{key}`", change.node));
                continue;
            };
            let Some(value) = json_to_value(raw, &declared) else {
                applied
                    .warnings
                    .push(format!("{}.{key}: {raw} is not a {declared:?}", change.node));
                continue;
            };
            match graph.set_param(id, key, value) {
                Ok(()) => applied.changed.push(format!("{}.{key}", change.node)),
                Err(e) => applied
                    .warnings
                    .push(format!("{}.{key}: {e}", change.node)),
            }
        }
        for (key, source) in &change.expressions {
            if graph.node(id).param(key).is_none() {
                applied
                    .warnings
                    .push(format!("{} has no parameter `{key}`", change.node));
                continue;
            }
            match graph.set_expression(id, key, source) {
                Ok(()) => applied.changed.push(format!("{}.{key}", change.node)),
                Err(e) => applied
                    .warnings
                    .push(format!("{}.{key}: {e}", change.node)),
            }
        }
    }

    // ---- remove what was asked to go
    //
    // Last, so everything above could rewire around it first, and fenced in
    // three ways. A model that has just been handed deletion is a model that
    // can empty somebody's patch on a misread, and none of these cost
    // anything when it has read correctly:
    //
    //  * **Direct children of this network only.** `resolve` will happily
    //    find a path into a component the user is not looking at; a delete
    //    that reaches somewhere off screen is not one they can see happen.
    //  * **Nothing it created this turn.** Creating a node and deleting it in
    //    the same answer is a model talking to itself, and honouring it makes
    //    the reported node count a lie.
    //  * **Never the whole network.** Clearing the canvas is `/clear`, one
    //    word, undoable and obviously deliberate. Arriving at it through
    //    "make it simpler" is not what anybody meant.
    let children: Vec<NodeId> = graph.node(parent).children.clone();
    let mut doomed: Vec<NodeId> = Vec::new();
    for name in &plan.deletes {
        let bare = name.trim().trim_start_matches('/');
        if made.contains_key(bare) {
            applied
                .warnings
                .push(format!("{bare} was created by this plan; not deleting it"));
            continue;
        }
        match graph
            .find_from(parent, bare)
            .filter(|id| children.contains(id))
        {
            Some(id) if !doomed.contains(&id) => doomed.push(id),
            Some(_) => {}
            None => applied
                .warnings
                .push(format!("no node named {bare} in this network to delete")),
        }
    }
    let survivors = children.len() + made.len() - doomed.len();
    if !doomed.is_empty() && survivors == 0 {
        applied.warnings.push(format!(
            "refused to delete all {} operators — use /clear if that is what you meant",
            doomed.len()
        ));
        doomed.clear();
    }
    // Every splice is worked out against the graph as it stands, then all of
    // them are removed, then the wires are run. Healing one node at a time
    // reads a graph the previous removal already changed: deleting `b` and
    // `c` out of `a -> b -> c -> d` would look up `c`'s input *after* `b` was
    // gone, find nothing, and leave `d` with an empty input.
    let splices = splices_for(graph, &doomed);
    for id in &doomed {
        applied.removed.push(graph.node(*id).name.clone());
        let _ = graph.remove(*id);
    }
    for (src, consumer, slot) in splices {
        let _ = graph.connect(src, consumer, slot);
    }

    for name in starved_loops(graph, &made) {
        applied.warnings.push(format!(
            "{name} closes a feedback loop with `over`, which hides its other \
             input — the picture will fade to black. Use add, screen or maximum."
        ));
    }

    let viewer = plan
        .viewer
        .as_ref()
        .and_then(|name| resolve(graph, &made, name));
    Ok((applied, viewer))
}

/// Composites that close a feedback loop in a way the source cannot survive.
///
/// A Composite TOP draws input 1 over input 0, and video is opaque, so `over`
/// means input 1 wins outright. Wire the source to input 0 and the feedback
/// to input 1 — the obvious way round, and the way a model writes it — and
/// the loop is sealed against its own source: it looks correct for about a
/// second, then the decay consumes what was already circulating and the whole
/// patch fades to black.
///
/// Worth a warning rather than a silent fix, because `over` on a feedback
/// branch is occasionally what somebody means — a matte with real alpha
/// composites exactly this way. Found by rendering the frames: it passes
/// every structural check there is.
fn starved_loops(graph: &Graph, made: &BTreeMap<String, NodeId>) -> Vec<String> {
    let mut out = Vec::new();
    for id in made.values().copied() {
        let node = graph.node(id);
        // Found by shape rather than by name: anything with two inputs and an
        // `operation` menu offering `over` blends the same way and starves a
        // loop the same way, and a check keyed on `compositeTOP` would stop
        // being true the day a second such operator is added.
        let Some(operation_param) = node.param("operation") else {
            continue;
        };
        let offers_over = operation_param
            .menu
            .as_ref()
            .map(|m| m.iter().any(|o| o.eq_ignore_ascii_case("over")))
            .unwrap_or(false);
        if !offers_over || node.inputs.len() < 2 {
            continue;
        }
        let operation = operation_param.value.as_str();
        // Empty means the default, which is `over`.
        if !(operation.is_empty() || operation.eq_ignore_ascii_case("over")) {
            continue;
        }
        // Input 0 has to be carrying something, or there is nothing to hide.
        if node.inputs.first().copied().flatten().is_none() {
            continue;
        }
        let Some(over) = node.inputs.get(1).copied().flatten() else {
            continue;
        };
        if reaches_feedback(graph, over, 6) {
            out.push(node.name.clone());
        }
    }
    out
}

/// Whether a Feedback TOP is within `depth` hops upstream. The decay and the
/// transform usually sit between the feedback and the composite, so this has
/// to look further than one wire.
fn reaches_feedback(graph: &Graph, from: NodeId, depth: usize) -> bool {
    if graph.node(from).op_type.to_lowercase().contains("feedback") {
        return true;
    }
    if depth == 0 {
        return false;
    }
    graph
        .node(from)
        .inputs
        .iter()
        .flatten()
        .any(|up| reaches_feedback(graph, *up, depth - 1))
}

/// The wires that keep a chain joined when a run of nodes is taken out of it.
///
/// Without this, "make it simpler" deletes three operators out of the middle
/// of a chain and leaves the survivors with an empty input 0 — a black
/// texture, and a patch that looks like the delete broke it.
///
/// Read-only, and worked out for the whole `doomed` set at once, because the
/// answer depends on the graph *before* any of them are removed. Returns
/// `(source, consumer, slot)` triples to run once they are gone.
pub fn splices_for(graph: &Graph, doomed: &[NodeId]) -> Vec<(NodeId, NodeId, usize)> {
    let mut out = Vec::new();
    for id in doomed {
        // Walk back past anything that is also going, so the two surviving
        // ends meet rather than one of them joining a corpse.
        let mut source = graph.node(*id).inputs.first().copied().flatten();
        while let Some(src) = source {
            if !doomed.contains(&src) {
                break;
            }
            source = graph.node(src).inputs.first().copied().flatten();
        }
        let Some(src) = source else { continue };
        for consumer in graph.consumers(*id) {
            if doomed.contains(&consumer) {
                continue;
            }
            for (slot, wired) in graph.node(consumer).inputs.iter().enumerate() {
                if *wired == Some(*id) {
                    out.push((src, consumer, slot));
                }
            }
        }
    }
    out
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
            // A path reference, the marker `apply` rewrites bare names for.
            m.insert("camera".into(), Param::str("").with_label("Camera").as_path_ref());
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
        // A CHOP, so exports have something legitimate to read from — and so
        // the "that is a TOP" refusal has something to contrast with.
        fn beat_params() -> IndexMap<String, Param> {
            let mut m = IndexMap::new();
            m.insert("rate".into(), Param::float(1.0).with_label("Rate"));
            m
        }
        r.register(OpDef {
            type_name: "beat",
            input_families: &[],
            label: "Beat",
            family: Family::Chop,
            inputs: &[],
            summary: "",
            time_dependent: true,
            params: beat_params,
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

    /// `a -> b -> c`, the shape every simplify test needs.
    fn chain(reg: &OpRegistry) -> (Graph, NodeId) {
        let mut graph = Graph::new();
        let root = graph.root();
        let value = serde_json::json!({
            "nodes": [
                { "name": "a", "op": "pass" },
                { "name": "b", "op": "pass" },
                { "name": "c", "op": "pass" }
            ],
            "connections": [
                { "from": "a", "to": "b", "input": 0 },
                { "from": "b", "to": "c", "input": 0 }
            ]
        });
        let plan = parse_plan(&value, reg).unwrap();
        apply(&mut graph, root, reg, &plan).unwrap();
        (graph, root)
    }

    #[test]
    fn set_retunes_an_operator_that_already_exists() {
        // The whole point of `set`: naming it under `nodes` used to get a
        // renamed duplicate beside the original, with the original untouched.
        let reg = test_registry();
        let (mut graph, root) = chain(&reg);
        let value = serde_json::json!({
            "set": [{ "node": "b", "params": { "gain": 0.25, "mode": "add" } }]
        });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();

        assert!(applied.created.is_empty(), "set creates nothing");
        assert_eq!(applied.changed.len(), 2, "{:?}", applied.warnings);
        let b = graph.find("/b").unwrap();
        assert_eq!(graph.node(b).param("gain").unwrap().value, Value::Float(0.25));
        // Still three nodes: no duplicate got made.
        assert_eq!(graph.node(root).children.len(), 3);
    }

    #[test]
    fn set_coerces_and_reports_a_parameter_that_is_not_there() {
        let reg = test_registry();
        let (mut graph, root) = chain(&reg);
        let value = serde_json::json!({
            // An int for a float parameter, and one that does not exist.
            "set": [{ "node": "b", "params": { "gain": 2, "warp_factor": 9 } }]
        });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();

        let b = graph.find("/b").unwrap();
        assert_eq!(graph.node(b).param("gain").unwrap().value, Value::Float(2.0));
        assert!(
            applied.warnings.iter().any(|w| w.contains("warp_factor")),
            "{:?}",
            applied.warnings
        );
    }

    #[test]
    fn delete_removes_the_node_and_joins_the_chain_back_up() {
        // Deleting from the middle without this leaves `c` with an empty
        // input, which is a black texture and looks like the delete broke it.
        let reg = test_registry();
        let (mut graph, root) = chain(&reg);
        let value = serde_json::json!({ "delete": ["b"] });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();

        assert_eq!(applied.removed, vec!["b"]);
        assert!(graph.find("/b").is_none());
        let c = graph.find("/c").unwrap();
        assert_eq!(graph.node(c).inputs[0], graph.find("/a"), "chain healed");
    }

    #[test]
    fn deleting_a_run_of_nodes_joins_the_two_surviving_ends() {
        // `a -> b -> c` with both b and c going: nothing to heal onto, but the
        // walk back must not reconnect through a node that is also leaving.
        let reg = test_registry();
        let (mut graph, root) = chain(&reg);
        let extra = serde_json::json!({
            "nodes": [{ "name": "d", "op": "pass" }],
            "connections": [{ "from": "c", "to": "d", "input": 0 }]
        });
        let plan = parse_plan(&extra, &reg).unwrap();
        apply(&mut graph, root, &reg, &plan).unwrap();

        let value = serde_json::json!({ "delete": ["b", "c"] });
        let plan = parse_plan(&value, &reg).unwrap();
        apply(&mut graph, root, &reg, &plan).unwrap();

        let d = graph.find("/d").unwrap();
        assert_eq!(
            graph.node(d).inputs[0],
            graph.find("/a"),
            "d should have been spliced onto a, not left empty or wired to a corpse"
        );
    }

    #[test]
    fn a_plan_may_be_nothing_but_a_change() {
        // "Slow the zoom down" creates nothing. Rejecting it for having no
        // `nodes` sent the model back to invent one it did not need.
        let reg = test_registry();
        let value = serde_json::json!({ "set": [{ "node": "b", "params": { "gain": 0.5 } }] });
        assert!(parse_plan(&value, &reg).is_ok());
        let empty = serde_json::json!({ "notes": "nothing to do" });
        assert!(parse_plan(&empty, &reg).is_err());
    }

    #[test]
    fn every_path_reference_is_rewritten_not_only_feedbacks_target() {
        // The model names another operator before any node has a path, and
        // before the graph has renamed anything that collided. Only `target`
        // used to be rewritten, which left `renderTOP.camera` and friends
        // pointing at a bare name — the entire 3D and instancing half of the
        // program building correctly and rendering an empty frame.
        let reg = test_registry();
        let mut graph = Graph::new();
        let root = graph.root();

        // Occupy the names first, so the plan's nodes get renamed and a
        // rewrite that merely prepended a slash would point at the wrong one.
        let squatter =
            parse_plan(&serde_json::json!({"nodes":[{"name":"cam1","op":"beat"}]}), &reg).unwrap();
        apply(&mut graph, root, &reg, &squatter).unwrap();

        let value = serde_json::json!({
            "nodes": [
                { "name": "cam1", "op": "beat" },
                { "name": "render1", "op": "pass", "params": { "camera": "cam1" } }
            ]
        });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();

        let render = graph.find_from(root, &applied.created[1]).unwrap();
        let pointed = graph.node(render).param("camera").unwrap().value.as_str();
        assert_eq!(
            pointed,
            format!("/{}", applied.created[0]),
            "should point at the camera this plan made, renamed and all"
        );
    }

    #[test]
    fn an_export_points_a_parameter_at_a_chop_channel() {
        // Export is the only way anything in this program reacts to anything
        // else. Without it in the plan format the assistant could animate on
        // a clock and nothing more: asked to react to a camera it built a
        // patch that looked busy and ignored the room.
        let reg = test_registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let value = serde_json::json!({
            "nodes": [
                { "name": "lag1", "op": "beat" },
                { "name": "zoom1", "op": "pass", "exports": { "gain": "lag1:chan1" } }
            ]
        });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();

        assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);
        let zoom = graph.find("/zoom1").unwrap();
        let param = graph.node(zoom).param("gain").unwrap();
        assert_eq!(param.mode, otd_core::param::ParamMode::Export);
        assert_eq!(param.source, "/lag1:chan1");
    }

    #[test]
    fn exporting_from_a_top_is_refused_rather_than_silently_reading_nothing() {
        // A parameter pointed at something with no channels does not error at
        // cook time — it just never changes, which looks exactly like a patch
        // that does not react and gives nobody anywhere to look.
        let reg = test_registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let value = serde_json::json!({
            "nodes": [
                { "name": "src1", "op": "pass" },
                { "name": "zoom1", "op": "pass", "exports": { "gain": "src1:r" } }
            ]
        });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();

        assert!(
            applied.warnings.iter().any(|w| w.contains("only a CHOP")),
            "{:?}",
            applied.warnings
        );
        let zoom = graph.find("/zoom1").unwrap();
        assert_ne!(
            graph.node(zoom).param("gain").unwrap().mode,
            otd_core::param::ParamMode::Export
        );
    }

    #[test]
    fn a_malformed_export_target_is_reported() {
        let reg = test_registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let value = serde_json::json!({
            "nodes": [{ "name": "a", "op": "pass", "exports": { "gain": "nocolon" } }]
        });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();
        assert!(
            applied.warnings.iter().any(|w| w.contains("node:channel")),
            "{:?}",
            applied.warnings
        );
        // A path with its own colon still splits on the last one.
        assert_eq!(split_export("/rack/lag1:chan1"), Some(("/rack/lag1", "chan1")));
        assert_eq!(split_export("  lag1 : r "), Some(("lag1", "r")));
        assert_eq!(split_export("lag1:"), None);
    }

    #[test]
    fn a_feedback_loop_closed_with_over_is_warned_about() {
        // The failure that rendering frames found and every structural check
        // missed: source into input 0, feedback into input 1, default `over`.
        // Opaque input 1 wins outright, the source never enters the loop, and
        // the patch fades to black over about a second — long enough to look
        // like it worked.
        let reg = feedback_registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let value = serde_json::json!({
            "nodes": [
                { "name": "src1", "op": "pass" },
                { "name": "fb1", "op": "feedback" },
                { "name": "decay1", "op": "pass" },
                { "name": "comp1", "op": "comp" }
            ],
            "connections": [
                { "from": "src1", "to": "comp1", "input": 0 },
                { "from": "fb1", "to": "decay1", "input": 0 },
                { "from": "decay1", "to": "comp1", "input": 1 }
            ]
        });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();
        assert!(
            applied.warnings.iter().any(|w| w.contains("fade to black")),
            "{:?}",
            applied.warnings
        );

        // `add` lets both sides through, so it is not warned about.
        let mut graph = Graph::new();
        let root = graph.root();
        let mut ok = value.clone();
        ok["nodes"][3]["params"] = serde_json::json!({ "operation": "add" });
        let plan = parse_plan(&ok, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();
        assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);
    }

    /// `pass`, plus a two-input composite and a feedback to build a loop from.
    fn feedback_registry() -> OpRegistry {
        fn comp_params() -> IndexMap<String, Param> {
            let mut m = IndexMap::new();
            m.insert(
                "operation".into(),
                Param::menu("over", &["over", "add", "screen"]).with_label("Operation"),
            );
            m
        }
        fn none() -> IndexMap<String, Param> {
            IndexMap::new()
        }
        let mut r = test_registry();
        r.register(OpDef {
            type_name: "comp",
            input_families: &[],
            label: "Comp",
            family: Family::Top,
            inputs: &["base", "over"],
            summary: "",
            time_dependent: false,
            params: comp_params,
            connector: Connector::None,
        });
        r.register(OpDef {
            type_name: "feedback",
            input_families: &[],
            label: "Feedback",
            family: Family::Top,
            inputs: &[],
            summary: "",
            time_dependent: true,
            params: none,
            connector: Connector::None,
        });
        r
    }

    #[test]
    fn deleting_the_whole_network_is_refused() {
        // A model handed deletion can empty a patch on one misread. Clearing
        // the canvas is `/clear` — one word, obviously deliberate.
        let reg = test_registry();
        let (mut graph, root) = chain(&reg);
        let value = serde_json::json!({ "delete": ["a", "b", "c"] });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();

        assert!(applied.removed.is_empty());
        assert_eq!(graph.node(root).children.len(), 3, "nothing was removed");
        assert!(
            applied.warnings.iter().any(|w| w.contains("/clear")),
            "{:?}",
            applied.warnings
        );
    }

    #[test]
    fn a_node_the_plan_just_created_is_not_deleted_by_it() {
        // A model talking to itself. Honouring it makes the reported node
        // count a lie.
        let reg = test_registry();
        let (mut graph, root) = chain(&reg);
        let value = serde_json::json!({
            "nodes": [{ "name": "fresh", "op": "pass" }],
            "connections": [{ "from": "c", "to": "fresh", "input": 0 }],
            "delete": ["fresh"]
        });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();

        assert!(applied.removed.is_empty(), "{:?}", applied.removed);
        assert!(graph.find("/fresh").is_some());
        assert!(
            applied.warnings.iter().any(|w| w.contains("fresh")),
            "{:?}",
            applied.warnings
        );
    }

    #[test]
    fn delete_cannot_reach_outside_the_network_being_edited() {
        // `resolve` will happily find a path into a component the user is not
        // looking at. A delete they cannot see happen is not one they can
        // catch.
        let reg = test_registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let outer = parse_plan(
            &serde_json::json!({ "nodes": [{ "name": "keep", "op": "pass" }] }),
            &reg,
        )
        .unwrap();
        apply(&mut graph, root, &reg, &outer).unwrap();

        let value = serde_json::json!({
            "nodes": [{ "name": "new1", "op": "pass" }],
            "delete": ["/keep/nested1", "somewhere_else"]
        });
        let plan = parse_plan(&value, &reg).unwrap();
        let (applied, _) = apply(&mut graph, root, &reg, &plan).unwrap();
        assert!(applied.removed.is_empty(), "{:?}", applied.removed);
        assert_eq!(applied.warnings.len(), 2, "{:?}", applied.warnings);
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
            exports: BTreeMap::new(),
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
