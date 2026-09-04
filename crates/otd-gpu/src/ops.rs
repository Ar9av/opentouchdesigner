//! The TOP operator table.
//!
//! Adding an operator is: one `.wgsl` file, one `params_*` function, one
//! `pack_*` function, one `TopSpec` entry. That ratio is deliberate — PLAN.md
//! §6 names operator breadth as the main treadmill risk, so the per-operator
//! cost has to stay near zero.

use std::sync::OnceLock;

use otd_core::indexmap::IndexMap;
use otd_core::{EvalContext, Family, Node, OpDef, OpRegistry, Param, Value};

pub const COMMON_WGSL: &str = include_str!("shaders/common.wgsl");

/// Four `vec4`s of operator parameters, handed to the shader as a uniform.
pub type PackedParams = [[f32; 4]; 4];

pub struct TopSpec {
    pub def: OpDef,
    /// Fragment shader source, appended to [`COMMON_WGSL`].
    pub shader: &'static str,
    /// Generators size themselves from `resw`/`resh`; filters inherit input 0.
    pub generator: bool,
    /// Separable operators run their pipeline twice through a scratch texture.
    pub two_pass: bool,
    pub pack: fn(&Node, &EvalContext) -> PackedParams,
}

// ------------------------------------------------------------ param helpers

fn val(node: &Node, ctx: &EvalContext, key: &str) -> Value {
    node.param(key)
        .map(|p| p.eval(ctx))
        .unwrap_or(Value::Float(0.0))
}

fn f(node: &Node, ctx: &EvalContext, key: &str) -> f32 {
    val(node, ctx, key).as_f32()
}

fn b(node: &Node, ctx: &EvalContext, key: &str) -> f32 {
    if val(node, ctx, key).as_bool() {
        1.0
    } else {
        0.0
    }
}

fn v4(node: &Node, ctx: &EvalContext, key: &str) -> [f32; 4] {
    val(node, ctx, key).as_vec4_f32()
}

/// Menu parameters store their label; the shader wants the index.
fn menu(node: &Node, ctx: &EvalContext, key: &str) -> f32 {
    let Some(param) = node.param(key) else {
        return 0.0;
    };
    let chosen = param.eval(ctx).as_str();
    param
        .menu
        .as_ref()
        .and_then(|items| items.iter().position(|i| *i == chosen))
        .unwrap_or(0) as f32
}

/// Output resolution of a generator.
pub fn generator_size(node: &Node, ctx: &EvalContext) -> (u32, u32) {
    let w = val(node, ctx, "resw").as_i64().clamp(1, 16384) as u32;
    let h = val(node, ctx, "resh").as_i64().clamp(1, 16384) as u32;
    (w, h)
}

fn with_res(mut m: IndexMap<String, Param>) -> IndexMap<String, Param> {
    m.insert(
        "resw".into(),
        Param::int(1280)
            .with_label("Resolution W")
            .with_range(1.0, 4096.0),
    );
    m.insert(
        "resh".into(),
        Param::int(720)
            .with_label("Resolution H")
            .with_range(1.0, 4096.0),
    );
    m
}

macro_rules! params {
    ($($key:expr => $param:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m: IndexMap<String, Param> = IndexMap::new();
        $( m.insert($key.into(), $param); )*
        m
    }};
}

// ------------------------------------------------------------- constant TOP

fn params_constant() -> IndexMap<String, Param> {
    with_res(params! {
        "color" => Param::rgba([0.0, 0.0, 0.0, 1.0]).with_label("Color"),
    })
}

fn pack_constant(n: &Node, c: &EvalContext) -> PackedParams {
    [v4(n, c, "color"), [0.0; 4], [0.0; 4], [0.0; 4]]
}

// ---------------------------------------------------------------- noise TOP

fn params_noise() -> IndexMap<String, Param> {
    with_res(params! {
        "period" => Param::float(0.25).with_label("Period").with_range(0.01, 4.0),
        "harmonics" => Param::int(3).with_label("Harmonics").with_range(1.0, 10.0),
        "roughness" => Param::float(0.5).with_label("Roughness").with_range(0.0, 1.0),
        "exponent" => Param::float(1.0).with_label("Exponent").with_range(0.1, 8.0),
        "translate" => Param::xyz([0.0, 0.0, 0.0]).with_label("Translate"),
        "monochrome" => Param::bool(true).with_label("Monochrome"),
        "amplitude" => Param::float(1.0).with_label("Amplitude").with_range(0.0, 4.0),
        "offset" => Param::float(0.0).with_label("Offset").with_range(-1.0, 1.0),
    })
}

fn pack_noise(n: &Node, c: &EvalContext) -> PackedParams {
    let t = val(n, c, "translate");
    let t = t.as_vec4_f32();
    [
        [
            f(n, c, "period"),
            f(n, c, "harmonics"),
            f(n, c, "roughness"),
            f(n, c, "exponent"),
        ],
        [t[0], t[1], t[2], b(n, c, "monochrome")],
        [f(n, c, "amplitude"), f(n, c, "offset"), 0.0, 0.0],
        [0.0; 4],
    ]
}

// ----------------------------------------------------------------- ramp TOP

fn params_ramp() -> IndexMap<String, Param> {
    with_res(params! {
        "type" => Param::menu("horizontal", &["horizontal", "vertical", "radial"]).with_label("Type"),
        "phase" => Param::float(0.0).with_label("Phase").with_range(0.0, 1.0),
        "color1" => Param::rgba([0.0, 0.0, 0.0, 1.0]).with_label("Color 1"),
        "color2" => Param::rgba([1.0, 1.0, 1.0, 1.0]).with_label("Color 2"),
    })
}

fn pack_ramp(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [menu(n, c, "type"), f(n, c, "phase"), 0.0, 0.0],
        v4(n, c, "color1"),
        v4(n, c, "color2"),
        [0.0; 4],
    ]
}

// ---------------------------------------------------------------- level TOP

fn params_level() -> IndexMap<String, Param> {
    params! {
        "brightness" => Param::float(1.0).with_label("Brightness").with_range(0.0, 4.0),
        "contrast" => Param::float(1.0).with_label("Contrast").with_range(0.0, 4.0),
        "gamma" => Param::float(1.0).with_label("Gamma").with_range(0.1, 4.0),
        "opacity" => Param::float(1.0).with_label("Opacity").with_range(0.0, 1.0),
        "blacklevel" => Param::float(0.0).with_label("Black Level").with_range(0.0, 1.0),
        "whitelevel" => Param::float(1.0).with_label("White Level").with_range(0.0, 1.0),
        "invert" => Param::float(0.0).with_label("Invert").with_range(0.0, 1.0),
    }
}

fn pack_level(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [
            f(n, c, "brightness"),
            f(n, c, "contrast"),
            f(n, c, "gamma"),
            f(n, c, "opacity"),
        ],
        [
            f(n, c, "blacklevel"),
            f(n, c, "whitelevel"),
            f(n, c, "invert"),
            0.0,
        ],
        [0.0; 4],
        [0.0; 4],
    ]
}

// ------------------------------------------------------------ transform TOP

fn params_transform() -> IndexMap<String, Param> {
    params! {
        "translate" => Param::new(Value::Vec2([0.0, 0.0])).with_label("Translate"),
        "rotate" => Param::float(0.0).with_label("Rotate").with_range(-180.0, 180.0),
        "scale" => Param::new(Value::Vec2([1.0, 1.0])).with_label("Scale"),
        "extend" => Param::menu("zero", &["zero", "hold", "repeat", "mirror"]).with_label("Extend"),
    }
}

fn pack_transform(n: &Node, c: &EvalContext) -> PackedParams {
    let t = val(n, c, "translate").as_vec4_f32();
    let s = val(n, c, "scale").as_vec4_f32();
    [
        [t[0], t[1], f(n, c, "rotate").to_radians(), 0.0],
        [s[0], s[1], menu(n, c, "extend"), 0.0],
        [0.0; 4],
        [0.0; 4],
    ]
}

// ----------------------------------------------------------------- blur TOP

fn params_blur() -> IndexMap<String, Param> {
    params! {
        "size" => Param::float(8.0).with_label("Size (px)").with_range(0.0, 128.0),
    }
}

fn pack_blur(n: &Node, c: &EvalContext) -> PackedParams {
    // .y is the pass direction, filled in by the engine on the second pass.
    [
        [f(n, c, "size"), 0.0, 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
    ]
}

// ------------------------------------------------------------ composite TOP

fn params_composite() -> IndexMap<String, Param> {
    params! {
        "operation" => Param::menu(
            "over",
            &["over", "add", "multiply", "screen", "difference", "subtract", "maximum", "minimum"],
        ).with_label("Operation"),
        "opacity" => Param::float(1.0).with_label("Opacity").with_range(0.0, 1.0),
    }
}

fn pack_composite(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [menu(n, c, "operation"), f(n, c, "opacity"), 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
    ]
}

// --------------------------------------------------------------- switch TOP

fn params_switch() -> IndexMap<String, Param> {
    params! {
        "index" => Param::float(0.0).with_label("Index").with_range(0.0, 1.0),
        "blend" => Param::bool(false).with_label("Blend"),
    }
}

fn pack_switch(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [f(n, c, "index"), b(n, c, "blend"), 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
    ]
}

// ----------------------------------------------------- null / feedback TOPs

fn params_none() -> IndexMap<String, Param> {
    params! {}
}

fn pack_none(_n: &Node, _c: &EvalContext) -> PackedParams {
    [[0.0; 4]; 4]
}

fn params_feedback() -> IndexMap<String, Param> {
    params! {
        "target" => Param::str("").with_label("Target TOP"),
    }
}

/// The operator type name of the Feedback TOP, special-cased by the engine.
pub const FEEDBACK: &str = "feedbackTOP";
/// Used for the Feedback blit and for any pass that is a straight copy.
pub const NULL: &str = "nullTOP";

// -------------------------------------------------------------- the table

fn specs() -> &'static Vec<TopSpec> {
    static SPECS: OnceLock<Vec<TopSpec>> = OnceLock::new();
    SPECS.get_or_init(|| {
        vec![
            TopSpec {
                def: OpDef {
                    type_name: "constantTOP",
                    label: "Constant",
                    family: Family::Top,
                    inputs: &[],
                    summary: "A flat colour at a chosen resolution.",
                    time_dependent: false,
                    params: params_constant,
                },
                shader: include_str!("shaders/constant.wgsl"),
                generator: true,
                two_pass: false,
                pack: pack_constant,
            },
            TopSpec {
                def: OpDef {
                    type_name: "noiseTOP",
                    label: "Noise",
                    family: Family::Top,
                    inputs: &[],
                    summary: "Fractal value noise. Animate Translate Z to make it move.",
                    time_dependent: false,
                    params: params_noise,
                },
                shader: include_str!("shaders/noise.wgsl"),
                generator: true,
                two_pass: false,
                pack: pack_noise,
            },
            TopSpec {
                def: OpDef {
                    type_name: "rampTOP",
                    label: "Ramp",
                    family: Family::Top,
                    inputs: &[],
                    summary: "Linear or radial gradient between two colours.",
                    time_dependent: false,
                    params: params_ramp,
                },
                shader: include_str!("shaders/ramp.wgsl"),
                generator: true,
                two_pass: false,
                pack: pack_ramp,
            },
            TopSpec {
                def: OpDef {
                    type_name: "levelTOP",
                    label: "Level",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "Brightness, contrast, gamma, black/white levels.",
                    time_dependent: false,
                    params: params_level,
                },
                shader: include_str!("shaders/level.wgsl"),
                generator: false,
                two_pass: false,
                pack: pack_level,
            },
            TopSpec {
                def: OpDef {
                    type_name: "transformTOP",
                    label: "Transform",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "Translate, rotate and scale, with an extend mode.",
                    time_dependent: false,
                    params: params_transform,
                },
                shader: include_str!("shaders/transform.wgsl"),
                generator: false,
                two_pass: false,
                pack: pack_transform,
            },
            TopSpec {
                def: OpDef {
                    type_name: "blurTOP",
                    label: "Blur",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "Separable Gaussian blur.",
                    time_dependent: false,
                    params: params_blur,
                },
                shader: include_str!("shaders/blur.wgsl"),
                generator: false,
                two_pass: true,
                pack: pack_blur,
            },
            TopSpec {
                def: OpDef {
                    type_name: "compositeTOP",
                    label: "Composite",
                    family: Family::Top,
                    inputs: &["base", "over"],
                    summary: "Blend two inputs. Input 2 is composited over input 1.",
                    time_dependent: false,
                    params: params_composite,
                },
                shader: include_str!("shaders/composite.wgsl"),
                generator: false,
                two_pass: false,
                pack: pack_composite,
            },
            TopSpec {
                def: OpDef {
                    type_name: "switchTOP",
                    label: "Switch",
                    family: Family::Top,
                    inputs: &["0", "1"],
                    summary: "Select one of two inputs, optionally blending between them.",
                    time_dependent: false,
                    params: params_switch,
                },
                shader: include_str!("shaders/switch.wgsl"),
                generator: false,
                two_pass: false,
                pack: pack_switch,
            },
            TopSpec {
                def: OpDef {
                    type_name: NULL,
                    label: "Null",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "Pass-through. A stable name to reference and to view.",
                    time_dependent: false,
                    params: params_none,
                },
                shader: include_str!("shaders/null.wgsl"),
                generator: false,
                two_pass: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: FEEDBACK,
                    label: "Feedback",
                    family: Family::Top,
                    // No wired input on purpose: the target is named by
                    // parameter, which is how a feedback loop is expressed
                    // without putting a cycle in the cook graph (PLAN.md §4).
                    inputs: &[],
                    summary: "Last frame's output of the Target TOP.",
                    time_dependent: true,
                    params: params_feedback,
                },
                shader: include_str!("shaders/null.wgsl"),
                generator: false,
                two_pass: false,
                pack: pack_none,
            },
        ]
    })
}

pub fn spec(type_name: &str) -> Option<&'static TopSpec> {
    specs().iter().find(|s| s.def.type_name == type_name)
}

pub fn all() -> impl Iterator<Item = &'static TopSpec> {
    specs().iter()
}

/// A container COMP, so the hierarchy is exercised from Phase 0 rather than
/// bolted on in Phase 3.
pub const CONTAINER: OpDef = OpDef {
    type_name: "containerCOMP",
    label: "Container",
    family: Family::Comp,
    inputs: &[],
    summary: "A component that holds a sub-network.",
    time_dependent: false,
    params: params_none,
};

/// Every operator this build knows about.
pub fn registry() -> OpRegistry {
    let mut r = OpRegistry::new();
    for s in specs() {
        r.register(s.def.clone());
    }
    r.register(CONTAINER);
    r
}
