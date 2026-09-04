//! ISF (Interactive Shader Format) import.
//!
//! PLAN.md §6 names this as the lever against the operator-breadth treadmill:
//! ISF is a published format with thousands of existing effects, and every
//! one of them is a GLSL fragment shader with a JSON header describing its
//! inputs. Importing one turns it into an ordinary GLSL TOP whose inputs are
//! ordinary parameters — after which nothing in the system knows or cares
//! that it came from somewhere else.
//!
//! Measured against the published collection at Vidvox/ISF-Files — 327 real
//! shaders — rather than against shaders written for the test, which only
//! ever proves the happy path. `cargo run -p otd-gpu --example isf_corpus`
//! reports the current figure and groups what fails by why. It stands at 225
//! of 327, and the remaining buckets are: multi-pass (56, a real feature and
//! a real absence), shaders whose varyings come from a companion vertex
//! shader we do not run (17), names we do not define — persistent buffers
//! mostly, which are multi-pass by another route (18), and shaders wanting an
//! audio texture (5), which needs a sound source before it needs a shader
//! change.
//!
//! Uniform space used to be a bucket of sixteen and is now none: the block is
//! twelve `vec4`s wide, which is where the widest published shader fits. See
//! [`crate::ops::PARAM_VECS`].
//!
//! What is supported is what the format actually uses in practice: `float`,
//! `bool`, `long` (a menu), `color`, `point2D` and `image` inputs, the
//! `IMG_THIS_PIXEL`/`IMG_NORM_PIXEL`/`IMG_PIXEL` accessors, and the standard
//! `isf_FragNormCoord`/`TIME`/`RENDERSIZE` variables. Multi-pass ISF is not:
//! it needs several render targets, which is a Phase 6 concern.

use otd_core::indexmap::IndexMap;
use otd_core::{Graph, NodeId, Param, Value};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IsfError {
    #[error("no ISF header: the file must start with a /*{{ ... }}*/ JSON comment")]
    NoHeader,
    #[error("the ISF header is not valid JSON: {0}")]
    BadJson(String),
    #[error("multi-pass ISF shaders are not supported yet")]
    MultiPass,
    #[error("this shader needs more uniform space than a GLSL TOP has (12 vec4s)")]
    TooManyInputs,
}

/// An imported shader: the parameters it wants, and the GLSL to run.
#[derive(Debug, Clone)]
pub struct Isf {
    pub name: String,
    pub description: String,
    pub categories: Vec<String>,
    /// Parameters to put on the node, in the order the header declares them.
    pub params: IndexMap<String, Param>,
    /// Which uniform each parameter feeds, in declaration order. The engine
    /// packs them into `U.p0..p11`.
    pub inputs: Vec<IsfInput>,
    /// GLSL ready to hand to the GLSL TOP.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IsfInput {
    pub name: String,
    pub kind: IsfKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsfKind {
    Float,
    Bool,
    Long,
    Color,
    Point2d,
    /// An image input, wired rather than dialled.
    Image,
}

impl IsfKind {
    /// Uniform components this kind occupies. Images occupy none — they are
    /// textures.
    pub fn width(self) -> usize {
        match self {
            IsfKind::Float | IsfKind::Bool | IsfKind::Long => 1,
            IsfKind::Point2d => 2,
            IsfKind::Color => 4,
            IsfKind::Image => 0,
        }
    }
}

/// Split an ISF file into its JSON header and its GLSL body.
fn split(source: &str) -> Result<(&str, &str), IsfError> {
    let start = source.find("/*").ok_or(IsfError::NoHeader)?;
    let end = source[start..].find("*/").ok_or(IsfError::NoHeader)? + start;
    let header = source[start + 2..end].trim();
    // The header is a JSON object; some files wrap it in `{{ }}` and some do
    // not, so find the object rather than assuming.
    let obj_start = header.find('{').ok_or(IsfError::NoHeader)?;
    let obj_end = header.rfind('}').ok_or(IsfError::NoHeader)?;
    if obj_end <= obj_start {
        return Err(IsfError::NoHeader);
    }
    Ok((&header[obj_start..=obj_end], &source[end + 2..]))
}

/// Convert an ISF file into something the GLSL TOP can run.
pub fn import(source: &str) -> Result<Isf, IsfError> {
    let (header_text, body) = split(source)?;
    let header: serde_json::Value =
        serde_json::from_str(header_text).map_err(|e| IsfError::BadJson(e.to_string()))?;

    if header
        .get("PASSES")
        .and_then(|p| p.as_array())
        .map(|p| p.len() > 1)
        .unwrap_or(false)
    {
        return Err(IsfError::MultiPass);
    }

    let mut params = IndexMap::new();
    let mut inputs = Vec::new();
    let mut image_count = 0usize;

    if let Some(list) = header.get("INPUTS").and_then(|i| i.as_array()) {
        for entry in list {
            let Some(name) = entry.get("NAME").and_then(|n| n.as_str()) else {
                continue;
            };
            let kind = entry
                .get("TYPE")
                .and_then(|t| t.as_str())
                .unwrap_or("float");
            let label = entry
                .get("LABEL")
                .and_then(|l| l.as_str())
                .unwrap_or(name)
                .to_string();

            let (param, kind) = match kind {
                "bool" => (
                    Param::bool(
                        entry
                            .get("DEFAULT")
                            .and_then(|d| d.as_f64())
                            .map(|d| d != 0.0)
                            .unwrap_or(false),
                    ),
                    IsfKind::Bool,
                ),
                "long" => {
                    // A `long` is ISF's menu: LABELS for display, VALUES for
                    // what the shader sees.
                    let labels: Vec<String> = entry
                        .get("LABELS")
                        .and_then(|l| l.as_array())
                        .map(|l| {
                            l.iter()
                                .map(|s| s.as_str().unwrap_or_default().to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                    let items: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
                    let first = items.first().copied().unwrap_or("0");
                    if items.is_empty() {
                        (Param::int(0), IsfKind::Long)
                    } else {
                        (Param::menu(first, &items), IsfKind::Long)
                    }
                }
                "color" => {
                    let d = number_array(entry.get("DEFAULT"), 4, [1.0, 1.0, 1.0, 1.0]);
                    (Param::rgba(d), IsfKind::Color)
                }
                "point2D" => {
                    let d = number_array(entry.get("DEFAULT"), 2, [0.0, 0.0, 0.0, 0.0]);
                    (
                        Param::new(otd_core::Value::Vec2([d[0], d[1]])),
                        IsfKind::Point2d,
                    )
                }
                "image" => {
                    image_count += 1;
                    (Param::str(""), IsfKind::Image)
                }
                _ => {
                    let default = entry.get("DEFAULT").and_then(|d| d.as_f64()).unwrap_or(0.0);
                    let min = entry.get("MIN").and_then(|d| d.as_f64());
                    let max = entry.get("MAX").and_then(|d| d.as_f64());
                    let p = Param::float(default);
                    let p = match (min, max) {
                        (Some(lo), Some(hi)) if hi > lo => p.with_range(lo, hi),
                        _ => p,
                    };
                    (p, IsfKind::Float)
                }
            };

            // Image inputs are wires, not dials; they do not appear on the
            // parameter page.
            if kind != IsfKind::Image {
                params.insert(name.to_string(), param.with_label(&label));
            }
            inputs.push(IsfInput {
                name: name.to_string(),
                kind,
            });
        }
    }

    Ok(Isf {
        name: header
            .get("CREDIT")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        description: header
            .get("DESCRIPTION")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string(),
        categories: header
            .get("CATEGORIES")
            .and_then(|c| c.as_array())
            .map(|c| {
                c.iter()
                    .filter_map(|s| s.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        source: {
            let (prelude, assignments) = preamble(&inputs, image_count)?;
            prelude + &wrap_entry_point(body, &assignments)
        },
        params,
        inputs,
    })
}

/// Load an imported shader onto an existing GLSL TOP.
///
/// The node keeps being an ordinary `glslTOP` afterwards: the shader is in its
/// `source` parameter and the ISF inputs are custom parameters like any other.
/// Nothing downstream knows the difference, which is the whole point — an
/// imported effect can be parameter-bound, exported to, animated and saved
/// exactly like a hand-written one.
pub fn apply(graph: &mut Graph, id: NodeId, isf: &Isf) {
    // Drop any parameters a previous import left behind, so re-importing a
    // different shader onto the same node does not accumulate dead dials.
    let stale: Vec<String> = graph
        .node(id)
        .params
        .iter()
        .filter(|(k, p)| p.custom && !isf.params.contains_key(k.as_str()))
        .map(|(k, _)| k.clone())
        .collect();
    for key in stale {
        graph.remove_custom_param(id, &key);
    }

    let _ = graph.set_param(id, "language", Value::Str("glsl".into()));
    let _ = graph.set_param(id, "source", Value::Str(isf.source.clone()));
    for (key, param) in &isf.params {
        graph.add_custom_param(id, key, param.clone());
    }
}

/// Where one parameter lands in the `U.p0..p11` uniform block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// Which `U.p*` — 0..`ops::PARAM_VECS`.
    pub vec: usize,
    /// First component within it — 0..3, i.e. `.x`..`.w`.
    pub component: usize,
    /// How many components it occupies: 1, 2 or 4.
    pub width: usize,
}

impl Slot {
    /// The GLSL swizzle that selects this slot, e.g. `.zw` or nothing at all
    /// for a full vector.
    pub fn swizzle(&self) -> &'static str {
        const NAMES: [&str; 4] = ["x", "y", "z", "w"];
        match (self.width, self.component) {
            (4, _) => "",
            (2, 0) => ".xy",
            (2, 2) => ".zw",
            (_, c) => match NAMES[c] {
                "x" => ".x",
                "y" => ".y",
                "z" => ".z",
                _ => ".w",
            },
        }
    }
}

/// How many uniform components a value of this type occupies.
///
/// Packing floats into components rather than giving each its own vector is
/// what stretches the block as far as it goes: a shader with eight dials is
/// entirely ordinary, and eight `vec4`s of them would be four times the room
/// they need.
pub fn width(value: &otd_core::Value) -> usize {
    use otd_core::Value::*;
    match value {
        Float(_) | Int(_) | Bool(_) => 1,
        Vec2(_) => 2,
        Vec3(_) | Vec4(_) => 4,
        // Strings are not uniforms — an ISF `image` keeps its wire here.
        Str(_) => 0,
    }
}

/// Assign uniform slots to a list of parameter widths, in declaration order.
///
/// Nothing straddles a vector boundary: a `vec4` starts a vector and a `vec2`
/// starts on an even component. That wastes a component here and there and is
/// worth it — a value split across two uniforms would need the shader to
/// reassemble it, which is exactly the kind of invisible contract that breaks
/// silently later.
///
/// `None` when the parameters do not fit in the vectors available.
pub fn layout(widths: impl IntoIterator<Item = usize>) -> Option<Vec<Slot>> {
    let capacity = crate::ops::PARAM_VECS * 4;
    let mut out = Vec::new();
    let mut cursor = 0usize; // in components, 0..capacity
    for w in widths {
        if w == 0 {
            continue;
        }
        // Align: a vec4 to a vector, a vec2 to an even component.
        let align = if w == 4 { 4 } else { w.min(2) };
        cursor = cursor.next_multiple_of(align);
        if cursor + w > capacity {
            return None;
        }
        out.push(Slot {
            vec: cursor / 4,
            component: cursor % 4,
            width: w,
        });
        cursor += w;
    }
    Some(out)
}

fn number_array(value: Option<&serde_json::Value>, n: usize, fallback: [f64; 4]) -> [f64; 4] {
    let mut out = fallback;
    if let Some(a) = value.and_then(|v| v.as_array()) {
        for (i, v) in a.iter().take(n).enumerate() {
            if let Some(f) = v.as_f64() {
                out[i] = f;
            }
        }
    }
    out
}

/// The GLSL that turns ISF's vocabulary into ours.
///
/// ISF shaders are written against a small set of globals and macros. Rather
/// than rewriting the body — which would mean parsing GLSL — this declares
/// them in terms of what the GLSL TOP already provides.
fn preamble(
    inputs: &[IsfInput],
    image_count: usize,
) -> Result<(String, Vec<String>), IsfError> {
    let mut out = String::from(
        // `__VERSION__` is defined because published ISF filters routinely
        // bracket their varyings in `#if __VERSION__ <= 120`, and naga leaves
        // it undefined — which evaluates to 0, takes the GLSL 1.x branch, and
        // fails the whole shader on `varying` declarations that are illegal
        // at 450. Twenty of the two hundred published filters died on that,
        // none of them for a reason to do with the shader.
        "// --- ISF compatibility ---\n\
         #define __VERSION__ 450\n\
         #define isf_FragNormCoord vec2(otd_uv.x, 1.0 - otd_uv.y)\n\
         #define vv_FragNormCoord isf_FragNormCoord\n\
         #define gl_FragColor otd_frag\n\
         #define TIME iTime\n\
         #define TIMEDELTA iTimeDelta\n\
         #define FRAMEINDEX iFrame\n\
         #define RENDERSIZE iResolution.xy\n\
         #define DATE iDate\n\
         #define PASSINDEX 0\n",
    );

    // ISF's image accessors, in terms of the two texture inputs a GLSL TOP
    // has. A shader asking for more images than that gets the dummy, which
    // reads as transparent black rather than as a compile error.
    //
    // Emitted whether or not the header declares an image, because some
    // published shaders call `IMG_NORM_PIXEL` against a persistent buffer or
    // a target they never listed as an INPUT. Defining a macro nothing uses
    // costs nothing; leaving it out costs the whole shader.
    {
        let _ = image_count;
        out.push_str(
            "#define IMG_NORM_PIXEL(img, uv) texture(img, vec2((uv).x, 1.0 - (uv).y))\n\
             #define IMG_THIS_NORM_PIXEL(img) IMG_NORM_PIXEL(img, isf_FragNormCoord)\n\
             #define IMG_PIXEL(img, px) IMG_NORM_PIXEL(img, (px) / RENDERSIZE)\n\
             #define IMG_THIS_PIXEL(img) IMG_THIS_NORM_PIXEL(img)\n\
             #define IMG_SIZE(img) RENDERSIZE\n",
        );
    }

    // Each declared input becomes a uniform the engine fills. The slots come
    // from the same `layout` the engine packs with, so the two cannot drift.
    let slots = layout(inputs.iter().map(|i| i.kind.width())).ok_or(IsfError::TooManyInputs)?;

    let mut assignments: Vec<String> = Vec::new();
    let mut image_index = 0;
    let mut slot_index = 0;
    for input in inputs {
        if input.kind == IsfKind::Image {
            // Images are the GLSL TOP's own texture inputs, not uniforms.
            out.push_str(&format!(
                "#define {} otd_image{}\n",
                input.name, image_index
            ));
            image_index += 1;
            continue;
        }
        let slot = slots[slot_index];
        slot_index += 1;
        let u = format!("U.p{}{}", slot.vec, slot.swizzle());
        // A global, not a `#define`.
        //
        // A macro rewrites the name *everywhere*, including where the shader
        // declares something of its own with that name — and ISF inputs are
        // called `size`, `color`, `angle`, `level`, so the collision is not
        // exotic. `Heart.fs` declares `bool inHeart(vec2 p, vec2 c, float
        // size)`, the macro turns that parameter into `float U.p0.x`, and the
        // shader dies on a syntax error nowhere near anything its author
        // wrote.
        //
        // A global declaration is the same substitution where the author
        // meant one, and correct GLSL shadowing where they did not: the
        // parameter wins inside the function, exactly as intended. The value
        // is assigned in `main` because a GLSL global cannot be initialised
        // from a uniform.
        let (ty, expression) = match input.kind {
            IsfKind::Bool => ("bool", format!("{u} > 0.5")),
            IsfKind::Long => ("int", format!("int({u})")),
            IsfKind::Color => ("vec4", u.clone()),
            IsfKind::Point2d => ("vec2", u.clone()),
            _ => ("float", u.clone()),
        };
        out.push_str(&format!("{ty} {};\n", input.name));
        assignments.push(format!("    {} = {expression};\n", input.name));
    }
    out.push_str("// --- end ISF compatibility ---\n");
    Ok((out, assignments))
}

/// Wrap an ISF body so its inputs are assigned before it runs.
///
/// The body brings its own `void main()`, so the assignments cannot simply be
/// prepended — they have to happen *inside* it, before the first line that
/// might read one. Renaming the body's entry point and calling it from ours
/// is the least invasive way to get there: no GLSL parsing, and the body is
/// otherwise untouched.
fn wrap_entry_point(body: &str, assignments: &[String]) -> String {
    if assignments.is_empty() {
        return body.to_string();
    }
    let renamed = rename_main(body);
    if renamed == body {
        // No `void main` to rename — the shader is a bare body and the GLSL
        // TOP will supply the entry point, so the assignments can go first.
        return format!("{}{body}", assignments.concat());
    }
    format!(
        "{renamed}\n\nvoid main() {{\n{}    isf_main();\n}}\n",
        assignments.concat()
    )
}

/// Rename the body's `void main(` to `void isf_main(`, once.
fn rename_main(body: &str) -> String {
    let mut out = String::with_capacity(body.len() + 8);
    let mut rest = body;
    let mut done = false;
    while let Some(i) = rest.find("main") {
        let (before, after) = rest.split_at(i);
        let following = &after[4..];
        // A declaration or definition: `main` followed by optional space and
        // an open paren, and preceded by something that is not an identifier
        // character, so `mainImage` and `domain` are left alone.
        let is_call_site = following.trim_start().starts_with('(');
        let preceded_ok = before
            .chars()
            .last()
            .map(|c| !c.is_alphanumeric() && c != '_')
            .unwrap_or(true);
        out.push_str(before);
        if !done && is_call_site && preceded_ok {
            out.push_str("isf_main");
            done = true;
        } else {
            out.push_str("main");
        }
        rest = following;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &str = r#"/*{
        "DESCRIPTION": "A test effect",
        "CREDIT": "nobody",
        "CATEGORIES": ["Color Effect"],
        "INPUTS": [
            { "NAME": "level", "TYPE": "float", "DEFAULT": 0.5, "MIN": 0.0, "MAX": 2.0 },
            { "NAME": "tint", "TYPE": "color", "DEFAULT": [1.0, 0.5, 0.0, 1.0] },
            { "NAME": "flip", "TYPE": "bool", "DEFAULT": 1 }
        ]
    }*/

void main() {
    gl_FragColor = vec4(isf_FragNormCoord, level, 1.0) * tint;
}
"#;

    #[test]
    fn the_header_becomes_parameters() {
        let isf = import(SIMPLE).unwrap();
        assert_eq!(isf.description, "A test effect");
        assert_eq!(isf.categories, vec!["Color Effect"]);

        let level = &isf.params["level"];
        assert_eq!(level.value, otd_core::Value::Float(0.5));
        assert_eq!(level.range, Some((0.0, 2.0)));

        assert_eq!(
            isf.params["tint"].value,
            otd_core::Value::Vec4([1.0, 0.5, 0.0, 1.0])
        );
        assert_eq!(isf.params["flip"].value, otd_core::Value::Bool(true));
    }

    #[test]
    fn the_body_is_kept_and_the_vocabulary_is_defined() {
        let isf = import(SIMPLE).unwrap();
        assert!(isf.source.contains("void main()"), "the body survived");
        assert!(isf.source.contains("#define isf_FragNormCoord"));
        assert!(isf.source.contains("#define TIME iTime"));
        // Inputs map to uniform slots in declaration order. `level` takes one
        // component; `tint` is a vec4 so it starts a fresh vector; `flip`
        // follows it.
        assert!(
            isf.source.contains("float level;") && isf.source.contains("level = U.p0.x;"),
            "{}",
            isf.source
        );
        assert!(
            isf.source.contains("vec4 tint;") && isf.source.contains("tint = U.p1;"),
            "{}",
            isf.source
        );
        assert!(
            isf.source.contains("bool flip;") && isf.source.contains("flip = U.p2.x > 0.5;"),
            "{}",
            isf.source
        );
    }

    #[test]
    fn scalars_share_a_vector_instead_of_each_taking_one() {
        let src = r#"/*{ "INPUTS": [
            { "NAME": "a", "TYPE": "float" },
            { "NAME": "b", "TYPE": "float" },
            { "NAME": "c", "TYPE": "float" },
            { "NAME": "d", "TYPE": "float" },
            { "NAME": "e", "TYPE": "float" }
        ] }*/
void main() {}"#;
        let isf = import(src).unwrap();
        // Four dials fit one vec4; the fifth starts the next. A shader with
        // eight knobs is entirely ordinary, and would not fit at all if each
        // one took a whole vector.
        for (name, u) in [
            ("a", "U.p0.x"),
            ("b", "U.p0.y"),
            ("c", "U.p0.z"),
            ("d", "U.p0.w"),
            ("e", "U.p1.x"),
        ] {
            assert!(
                isf.source.contains(&format!("{name} = {u};")),
                "{name} -> {u}\n{}",
                isf.source
            );
        }
    }

    #[test]
    fn nothing_straddles_a_vector_boundary() {
        // x, then a point2D: the point cannot start at component 1, so it
        // aligns to 2 and leaves a hole.
        let slots = layout([1, 2, 4, 1]).unwrap();
        assert_eq!(
            slots[0],
            Slot {
                vec: 0,
                component: 0,
                width: 1
            }
        );
        assert_eq!(
            slots[1],
            Slot {
                vec: 0,
                component: 2,
                width: 2
            }
        );
        assert_eq!(
            slots[2],
            Slot {
                vec: 1,
                component: 0,
                width: 4
            }
        );
        assert_eq!(
            slots[3],
            Slot {
                vec: 2,
                component: 0,
                width: 1
            }
        );
        assert_eq!(slots[1].swizzle(), ".zw");
        assert_eq!(slots[2].swizzle(), "");
    }

    #[test]
    fn a_shader_that_does_not_fit_is_refused_rather_than_aliased() {
        // One dial past the end does not fit. Silently folding the extras onto
        // the last slot would look like it worked.
        let capacity = crate::ops::PARAM_VECS * 4;
        assert_eq!(layout(std::iter::repeat_n(1, capacity + 1)), None);
        assert!(layout(std::iter::repeat_n(1, capacity)).is_some());

        let colors = |n: usize| {
            let inputs: String = (0..n)
                .map(|i| format!(r#"{{ "NAME": "c{i}", "TYPE": "color" }}"#))
                .collect::<Vec<_>>()
                .join(",");
            format!("/*{{ \"INPUTS\": [{inputs}] }}*/\nvoid main() {{}}")
        };
        assert_eq!(
            import(&colors(crate::ops::PARAM_VECS + 1)).unwrap_err(),
            IsfError::TooManyInputs
        );

        // Five colours were the old ceiling, and the published corpus has
        // sixteen shaders past it — `Corner Colors` is one, and it is the
        // whole reason the block is twelve vectors wide rather than four.
        assert!(import(&colors(5)).is_ok(), "five colours have to fit now");
        assert!(import(&colors(crate::ops::PARAM_VECS)).is_ok());
    }

    /// A parameter in the widened part of the block reaches the shader by the
    /// same name it had in the header.
    #[test]
    fn an_input_past_the_first_four_vectors_gets_a_slot() {
        let inputs: String = (0..6)
            .map(|i| format!(r#"{{ "NAME": "c{i}", "TYPE": "color" }}"#))
            .collect::<Vec<_>>()
            .join(",");
        let src = format!("/*{{ \"INPUTS\": [{inputs}] }}*/\nvoid main() {{}}");
        let isf = import(&src).unwrap();
        assert!(isf.source.contains("c4 = U.p4;"), "{}", isf.source);
        assert!(isf.source.contains("c5 = U.p5;"));
        crate::shader::validate_glsl(&crate::shader::wrap_glsl(&isf.source))
            .expect("a six-colour shader should compile");
    }

    #[test]
    fn image_inputs_become_texture_inputs_not_parameters() {
        let src = r#"/*{
            "INPUTS": [
                { "NAME": "inputImage", "TYPE": "image" },
                { "NAME": "amount", "TYPE": "float", "DEFAULT": 1.0 }
            ]
        }*/
void main() { gl_FragColor = IMG_THIS_PIXEL(inputImage) * amount; }"#;
        let isf = import(src).unwrap();
        assert!(
            !isf.params.contains_key("inputImage"),
            "an image is a wire, not a dial"
        );
        assert!(isf.params.contains_key("amount"));
        assert!(isf.source.contains("#define inputImage otd_image0"));
        assert!(isf.source.contains("#define IMG_THIS_PIXEL"));
        // The float still gets slot 0: images do not consume uniform slots.
        assert!(isf.source.contains("amount = U.p0.x;"));
    }

    #[test]
    fn a_long_input_becomes_a_menu() {
        let src = r#"/*{
            "INPUTS": [
                { "NAME": "mode", "TYPE": "long",
                  "LABELS": ["add", "multiply"], "VALUES": [0, 1], "DEFAULT": 0 }
            ]
        }*/
void main() { gl_FragColor = vec4(float(mode)); }"#;
        let isf = import(src).unwrap();
        assert_eq!(
            isf.params["mode"].menu.as_deref(),
            Some(["add".to_string(), "multiply".to_string()].as_slice())
        );
        assert!(isf.source.contains("mode = int(U.p0.x);"));
    }

    #[test]
    fn a_file_without_a_header_is_refused_clearly() {
        assert_eq!(import("void main() {}").unwrap_err(), IsfError::NoHeader);
        assert!(matches!(
            import("/*{ not json }*/ void main() {}").unwrap_err(),
            IsfError::BadJson(_)
        ));
    }

    #[test]
    fn multi_pass_is_refused_rather_than_half_run() {
        let src = r#"/*{
            "PASSES": [ { "TARGET": "a" }, { } ]
        }*/
void main() {}"#;
        assert_eq!(import(src).unwrap_err(), IsfError::MultiPass);
    }

    /// Compile what the importer produced, the way the node would.
    fn compiles(isf: &Isf) -> Result<(), String> {
        crate::shader::validate_glsl(&crate::shader::wrap_glsl(&isf.source))
    }

    #[test]
    fn an_input_named_like_a_local_does_not_rewrite_the_local() {
        // Straight out of the published collection: `Heart.fs` declares an
        // input called `size` and also `bool inHeart(vec2 p, vec2 c, float
        // size)`. As a `#define` the parameter became `float U.p0.x` and the
        // shader died on a syntax error nowhere near anything its author
        // wrote. As a global it shadows correctly, which is what GLSL is for.
        // Eleven of the two hundred published filters failed this way.
        let src = r#"/*{
            "INPUTS": [ { "NAME": "size", "TYPE": "float", "DEFAULT": 0.5 } ]
        }*/
        bool big(float size) { return size > 0.5; }
        void main() {
            gl_FragColor = vec4(vec3(big(size) ? 1.0 : 0.0), 1.0);
        }"#;
        let isf = import(src).unwrap();
        assert!(
            isf.source.contains("float size;"),
            "the input should be a global:\n{}",
            isf.source
        );
        assert!(
            isf.source.contains("bool big(float size)"),
            "the function parameter must survive untouched:\n{}",
            isf.source
        );
        if let Err(e) = compiles(&isf) {
            panic!("{e}\n---\n{}", isf.source);
        }
    }

    #[test]
    fn a_shader_guarded_on_the_glsl_version_takes_the_modern_branch() {
        // Published filters bracket their varyings in `#if __VERSION__ <=
        // 120`. Undefined, that is zero, which is <= 120 — so the legacy
        // branch is taken and a pile of `varying` declarations that are
        // illegal at 450 fail the shader. Twenty filters died on this.
        let src = r#"/*{
            "INPUTS": [ { "NAME": "level", "TYPE": "float" } ]
        }*/
        #if __VERSION__ <= 120
        varying vec2 legacy_coord;
        #endif
        void main() {
            gl_FragColor = vec4(vec3(level), 1.0);
        }"#;
        let isf = import(src).unwrap();
        if let Err(e) = compiles(&isf) {
            panic!("the modern branch should have been taken: {e}\n---\n{}", isf.source);
        }
    }

    #[test]
    fn the_image_accessors_exist_even_with_no_image_input_declared() {
        // Some published shaders reach for IMG_NORM_PIXEL against something
        // they never listed in INPUTS. A macro nothing uses costs nothing;
        // leaving it out costs the whole shader.
        let src = r#"/*{ "INPUTS": [] }*/
        void main() { gl_FragColor = IMG_NORM_PIXEL(otd_image0, isf_FragNormCoord); }"#;
        let isf = import(src).unwrap();
        assert!(isf.source.contains("#define IMG_NORM_PIXEL"));
        compiles(&isf).unwrap();
    }

    #[test]
    fn renaming_the_entry_point_leaves_similar_names_alone() {
        // `mainImage` and `domain` both contain `main`, and a careless rename
        // would break a shader that has neither problem.
        assert_eq!(rename_main("void main() {}"), "void isf_main() {}");
        assert_eq!(
            rename_main("float domain(float x) { return x; }\nvoid main(void) {}"),
            "float domain(float x) { return x; }\nvoid isf_main(void) {}"
        );
        assert_eq!(
            rename_main("void mainImage(out vec4 c) {}"),
            "void mainImage(out vec4 c) {}",
            "mainImage is not the entry point and must not be renamed"
        );
        // Only the first one, so a shader that mentions main() in a comment
        // after its definition is unaffected.
        assert_eq!(
            rename_main("void main() {}\n// main() again"),
            "void isf_main() {}\n// main() again"
        );
    }
}
