//! ISF (Interactive Shader Format) import.
//!
//! PLAN.md §6 names this as the lever against the operator-breadth treadmill:
//! ISF is a published format with thousands of existing effects, and every
//! one of them is a GLSL fragment shader with a JSON header describing its
//! inputs. Importing one turns it into an ordinary GLSL TOP whose inputs are
//! ordinary parameters — after which nothing in the system knows or cares
//! that it came from somewhere else.
//!
//! What is supported is what the format actually uses in practice: `float`,
//! `bool`, `long` (a menu), `color`, `point2D` and `image` inputs, the
//! `IMG_THIS_PIXEL`/`IMG_NORM_PIXEL`/`IMG_PIXEL` accessors, and the standard
//! `isf_FragNormCoord`/`TIME`/`RENDERSIZE` variables. Multi-pass ISF is not:
//! it needs several render targets, which is a Phase 6 concern.

use otd_core::indexmap::IndexMap;
use otd_core::Param;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IsfError {
    #[error("no ISF header: the file must start with a /*{{ ... }}*/ JSON comment")]
    NoHeader,
    #[error("the ISF header is not valid JSON: {0}")]
    BadJson(String),
    #[error("multi-pass ISF shaders are not supported yet")]
    MultiPass,
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
    /// packs them into `U.p0..p3`.
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
            let kind = entry.get("TYPE").and_then(|t| t.as_str()).unwrap_or("float");
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
        source: preamble(&inputs, image_count) + body,
        params,
        inputs,
    })
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
fn preamble(inputs: &[IsfInput], image_count: usize) -> String {
    let mut out = String::from(
        "// --- ISF compatibility ---\n\
         #define isf_FragNormCoord vec2(otd_uv.x, 1.0 - otd_uv.y)\n\
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
    if image_count > 0 {
        out.push_str(
            "#define IMG_NORM_PIXEL(img, uv) texture(img, vec2((uv).x, 1.0 - (uv).y))\n\
             #define IMG_THIS_NORM_PIXEL(img) IMG_NORM_PIXEL(img, isf_FragNormCoord)\n\
             #define IMG_PIXEL(img, px) IMG_NORM_PIXEL(img, (px) / RENDERSIZE)\n\
             #define IMG_THIS_PIXEL(img) IMG_THIS_NORM_PIXEL(img)\n\
             #define IMG_SIZE(img) RENDERSIZE\n",
        );
    }

    // Each declared input becomes a uniform the engine fills.
    let mut image_index = 0;
    let mut slot = 0usize;
    for input in inputs {
        match input.kind {
            IsfKind::Image => {
                // Images are the GLSL TOP's own texture inputs.
                out.push_str(&format!(
                    "#define {} otd_image{}\n",
                    input.name, image_index
                ));
                image_index += 1;
            }
            IsfKind::Color => {
                out.push_str(&format!("#define {} U.p{}\n", input.name, slot.min(3)));
                slot += 1;
            }
            IsfKind::Point2d => {
                out.push_str(&format!("#define {} U.p{}.xy\n", input.name, slot.min(3)));
                slot += 1;
            }
            IsfKind::Bool => {
                out.push_str(&format!(
                    "#define {} (U.p{}.x > 0.5)\n",
                    input.name,
                    slot.min(3)
                ));
                slot += 1;
            }
            IsfKind::Long => {
                out.push_str(&format!("#define {} int(U.p{}.x)\n", input.name, slot.min(3)));
                slot += 1;
            }
            IsfKind::Float => {
                out.push_str(&format!("#define {} U.p{}.x\n", input.name, slot.min(3)));
                slot += 1;
            }
        }
    }
    out.push_str("// --- end ISF compatibility ---\n");
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
        // Inputs map to uniform slots in declaration order.
        assert!(isf.source.contains("#define level U.p0.x"));
        assert!(isf.source.contains("#define tint U.p1"));
        assert!(isf.source.contains("#define flip (U.p2.x > 0.5)"));
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
        assert!(isf.source.contains("#define amount U.p0.x"));
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
        assert!(isf.source.contains("#define mode int(U.p0.x)"));
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
}
