//! The TOP operator table.
//!
//! Adding an operator is: one `.wgsl` file, one `params_*` function, one
//! `pack_*` function, one `TopSpec` entry. That ratio is deliberate — PLAN.md
//! §6 names operator breadth as the main treadmill risk, so the per-operator
//! cost has to stay near zero.

use std::sync::OnceLock;

use crate::isf;

use otd_core::indexmap::IndexMap;
use otd_core::{Connector, EvalContext, Family, Node, OpDef, OpRegistry, Param, Value};

pub const COMMON_WGSL: &str = include_str!("shaders/common.wgsl");

/// Four `vec4`s of operator parameters, handed to the shader as a uniform.
pub type PackedParams = [[f32; 4]; 4];

/// How an operator decides its output resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sizing {
    /// From this node's own `resw` / `resh` parameters — generators, and the
    /// Resolution TOP.
    Params,
    /// Inherited from input 0 — the usual filter behaviour.
    Input0,
    /// Input 0 if something is wired there, otherwise `resw` / `resh`. Used by
    /// the GLSL TOP, which is a generator or a filter depending on how it is
    /// patched.
    Input0OrParams,
    /// Decided by the engine — Feedback and Select take the size of whatever
    /// they point at.
    Referenced,
}

pub struct TopSpec {
    pub def: OpDef,
    /// Fragment shader source, appended to [`COMMON_WGSL`].
    pub shader: &'static str,
    pub sizing: Sizing,
    /// Separable operators run their pipeline twice through a scratch texture.
    pub two_pass: bool,
    /// The shader comes from a parameter and is compiled at cook time.
    pub dynamic_shader: bool,
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

// ------------------------------------------------------------ threshold TOP

fn params_threshold() -> IndexMap<String, Param> {
    params! {
        "threshold" => Param::float(0.5).with_label("Threshold").with_range(0.0, 1.0),
        "softness" => Param::float(0.02).with_label("Softness").with_range(0.0, 1.0),
        "source" => Param::menu("luminance", &["luminance", "maximum", "alpha"])
            .with_label("Compare"),
        "invert" => Param::bool(false).with_label("Invert"),
        "below" => Param::rgba([0.0, 0.0, 0.0, 1.0]).with_label("Below Colour"),
        "above" => Param::rgba([1.0, 1.0, 1.0, 1.0]).with_label("Above Colour"),
    }
}

fn pack_threshold(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [
            f(n, c, "threshold"),
            f(n, c, "softness"),
            menu(n, c, "source"),
            b(n, c, "invert"),
        ],
        v4(n, c, "below"),
        v4(n, c, "above"),
        [0.0; 4],
    ]
}

// ----------------------------------------------------------------- edge TOP

fn params_edge() -> IndexMap<String, Param> {
    params! {
        "strength" => Param::float(1.0).with_label("Strength").with_range(0.0, 8.0),
        "width" => Param::float(1.0).with_label("Width (px)").with_range(0.0, 16.0),
        "direction" => Param::menu("both", &["both", "horizontal", "vertical"])
            .with_label("Direction"),
        "keepcolor" => Param::float(0.0).with_label("Keep Colour").with_range(0.0, 1.0),
        "color" => Param::rgba([1.0, 1.0, 1.0, 1.0]).with_label("Edge Colour"),
    }
}

fn pack_edge(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [
            f(n, c, "strength"),
            f(n, c, "width"),
            menu(n, c, "direction"),
            f(n, c, "keepcolor"),
        ],
        v4(n, c, "color"),
        [0.0; 4],
        [0.0; 4],
    ]
}

// ------------------------------------------------------------------ hsv TOP

fn params_hsv() -> IndexMap<String, Param> {
    params! {
        "hue" => Param::float(0.0).with_label("Hue Shift").with_range(-0.5, 0.5),
        "saturation" => Param::float(1.0).with_label("Saturation").with_range(0.0, 4.0),
        "value" => Param::float(1.0).with_label("Value").with_range(0.0, 4.0),
        "contrast" => Param::float(1.0).with_label("Contrast").with_range(0.0, 4.0),
        "rangecentre" => Param::float(0.0).with_label("Range Centre").with_range(0.0, 1.0),
        "rangewidth" => Param::float(1.0).with_label("Range Width").with_range(0.0, 1.0),
    }
}

fn pack_hsv(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [
            f(n, c, "hue"),
            f(n, c, "saturation"),
            f(n, c, "value"),
            f(n, c, "contrast"),
        ],
        [f(n, c, "rangecentre"), f(n, c, "rangewidth"), 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
    ]
}

// ----------------------------------------------------------------- flip TOP

fn params_flip() -> IndexMap<String, Param> {
    params! {
        "flipx" => Param::bool(true).with_label("Flip Horizontally"),
        "flipy" => Param::bool(false).with_label("Flip Vertically"),
        "transpose" => Param::bool(false).with_label("Transpose"),
    }
}

fn pack_flip(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [
            b(n, c, "flipx"),
            b(n, c, "flipy"),
            b(n, c, "transpose"),
            0.0,
        ],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
    ]
}

// --------------------------------------------------------------- mirror TOP

fn params_mirror() -> IndexMap<String, Param> {
    params! {
        "mode" => Param::menu("horizontal", &["horizontal", "vertical", "quad", "radial"])
            .with_label("Mode"),
        "segments" => Param::float(6.0).with_label("Segments").with_range(2.0, 32.0),
        "angle" => Param::float(0.0).with_label("Angle").with_range(-180.0, 180.0),
        "centre" => Param::new(Value::Vec2([0.5, 0.5])).with_label("Centre"),
    }
}

fn pack_mirror(n: &Node, c: &EvalContext) -> PackedParams {
    let centre = val(n, c, "centre").as_vec4_f32();
    [
        [
            menu(n, c, "mode"),
            f(n, c, "segments"),
            f(n, c, "angle").to_radians(),
            centre[0],
        ],
        [centre[1], 0.0, 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
    ]
}

// --------------------------------------------------------------- lookup TOP

fn params_lookup() -> IndexMap<String, Param> {
    params! {
        "source" => Param::menu("luminance", &["luminance", "red", "green", "blue", "alpha"])
            .with_label("Index By"),
    }
}

fn pack_lookup(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [menu(n, c, "source"), 0.0, 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
    ]
}

// --------------------------------------------------------------- circle TOP

fn params_circle() -> IndexMap<String, Param> {
    with_res(params! {
        "centre" => Param::new(Value::Vec2([0.5, 0.5])).with_label("Centre"),
        "radius" => Param::new(Value::Vec2([0.3, 0.3])).with_label("Radius"),
        "softness" => Param::float(0.0).with_label("Softness").with_range(0.0, 1.0),
        "aspect" => Param::bool(true).with_label("Correct Aspect"),
        "fill" => Param::rgba([1.0, 1.0, 1.0, 1.0]).with_label("Fill"),
        "background" => Param::rgba([0.0, 0.0, 0.0, 0.0]).with_label("Background"),
    })
}

fn pack_circle(n: &Node, c: &EvalContext) -> PackedParams {
    let centre = val(n, c, "centre").as_vec4_f32();
    let radius = val(n, c, "radius").as_vec4_f32();
    [
        [centre[0], centre[1], radius[0], radius[1]],
        v4(n, c, "fill"),
        v4(n, c, "background"),
        [f(n, c, "softness"), b(n, c, "aspect"), 0.0, 0.0],
    ]
}

// ------------------------------------------------------------ rectangle TOP

fn params_rectangle() -> IndexMap<String, Param> {
    with_res(params! {
        "centre" => Param::new(Value::Vec2([0.5, 0.5])).with_label("Centre"),
        "size" => Param::new(Value::Vec2([0.5, 0.5])).with_label("Size"),
        "softness" => Param::float(0.0).with_label("Softness").with_range(0.0, 0.5),
        "corner" => Param::float(0.0).with_label("Corner Radius").with_range(0.0, 0.5),
        "border" => Param::float(0.0).with_label("Border Width").with_range(0.0, 0.5),
        "bordercolor" => Param::float(0.0).with_label("Border Grey").with_range(0.0, 1.0),
        "fill" => Param::rgba([1.0, 1.0, 1.0, 1.0]).with_label("Fill"),
        "background" => Param::rgba([0.0, 0.0, 0.0, 0.0]).with_label("Background"),
    })
}

fn pack_rectangle(n: &Node, c: &EvalContext) -> PackedParams {
    let centre = val(n, c, "centre").as_vec4_f32();
    let size = val(n, c, "size").as_vec4_f32();
    [
        // Half extents: a distance field is symmetric about the centre, and
        // halving here means the shader does not do it per pixel.
        [centre[0], centre[1], size[0] * 0.5, size[1] * 0.5],
        v4(n, c, "fill"),
        v4(n, c, "background"),
        [
            f(n, c, "softness"),
            f(n, c, "corner"),
            f(n, c, "border"),
            f(n, c, "bordercolor"),
        ],
    ]
}

// ------------------------------------------------------------ chroma key TOP

fn params_chromakey() -> IndexMap<String, Param> {
    params! {
        "key" => Param::rgba([0.0, 1.0, 0.0, 1.0]).with_label("Key Colour"),
        "tolerance" => Param::float(0.15).with_label("Tolerance").with_range(0.0, 1.0),
        "softness" => Param::float(0.1).with_label("Softness").with_range(0.0, 1.0),
        "despill" => Param::float(1.0).with_label("Despill").with_range(0.0, 1.0),
        "replace" => Param::bool(false).with_label("Replace Rather Than Cut"),
        "replacement" => Param::rgba([0.0, 0.0, 0.0, 1.0]).with_label("Replacement"),
    }
}

fn pack_chromakey(n: &Node, c: &EvalContext) -> PackedParams {
    let key = v4(n, c, "key");
    [
        [key[0], key[1], key[2], f(n, c, "tolerance")],
        [
            f(n, c, "softness"),
            f(n, c, "despill"),
            b(n, c, "replace"),
            0.0,
        ],
        v4(n, c, "replacement"),
        [0.0; 4],
    ]
}

// ----------------------------------------------------------------- math TOP

fn params_math() -> IndexMap<String, Param> {
    params! {
        "operation" => Param::menu(
            "add",
            &["add", "subtract", "multiply", "divide", "minimum", "maximum", "difference", "power"],
        ).with_label("Operation"),
        "gain1" => Param::rgba([1.0, 1.0, 1.0, 1.0]).with_label("Input 1 Gain"),
        "gain2" => Param::rgba([1.0, 1.0, 1.0, 1.0]).with_label("Input 2 Gain"),
        "offset" => Param::rgba([0.0, 0.0, 0.0, 0.0]).with_label("Offset"),
    }
}

fn pack_math(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [menu(n, c, "operation"), 0.0, 0.0, 0.0],
        v4(n, c, "gain1"),
        v4(n, c, "gain2"),
        v4(n, c, "offset"),
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
/// Reads another TOP's output *this* frame, unlike Feedback.
pub const SELECT: &str = "selectTOP";
/// A component's texture input, surfaced as a connector on its node.
pub const IN: &str = "inTOP";
/// A component's texture output.
pub const OUT: &str = "outTOP";
/// Freezes its input on demand.
pub const CACHE: &str = "cacheTOP";
/// Compiles a shader from a parameter at cook time.
pub const GLSL: &str = "glslTOP";
/// Draws the 3D scene. Special-cased by the engine, which owns the pipeline.
pub const RENDER: &str = "renderTOP";
/// Plays an image or a movie. Special-cased by the engine, which owns the
/// decoder threads and the upload.
pub const MOVIE_IN: &str = "moviefileinTOP";
/// A camera. Same decoder, a different ffmpeg input.
pub const VIDEO_DEVICE_IN: &str = "videodeviceinTOP";
/// Channels as pixels. Special-cased by the engine, which owns the upload.
pub const CHOP_TO_TOP: &str = "choptotopTOP";
/// Draws text. Special-cased by the engine, which owns the font cache and
/// the upload.
pub const TEXT: &str = "textTOP";

fn params_text() -> IndexMap<String, Param> {
    with_res(params! {
        "text" => Param::str("OpenTouchDesigner").with_label("Text"),
        // A file reference, so a bundle copies the font with the project. A
        // show that opens on the wrong machine with the wrong face is the
        // whole reason bundles exist.
        "font" => Param::str("").with_label("Font File").as_file_ref(),
        "size" => Param::float(48.0).with_label("Size (px)").with_range(4.0, 512.0),
        "linespacing" => Param::float(1.2).with_label("Line Spacing").with_range(0.5, 4.0),
        "halign" => Param::menu("centre", &["left", "centre", "right"])
            .with_label("Horizontal Align"),
        "valign" => Param::menu("centre", &["top", "centre", "bottom"])
            .with_label("Vertical Align"),
        "wrap" => Param::bool(true).with_label("Word Wrap"),
        "color" => Param::rgba([1.0, 1.0, 1.0, 1.0]).with_label("Color"),
        "background" => Param::rgba([0.0, 0.0, 0.0, 0.0]).with_label("Background"),
    })
}

fn pack_text(n: &Node, c: &EvalContext) -> PackedParams {
    [
        v4(n, c, "color"),
        v4(n, c, "background"),
        [0.0; 4],
        [0.0; 4],
    ]
}

/// Records its input to a movie file. Special-cased by the engine, which owns
/// the readback and the encoder subprocess.
pub const MOVIE_OUT: &str = "moviefileoutTOP";

fn params_movie_out() -> IndexMap<String, Param> {
    params! {
        // Deliberately *not* a file reference: that marks media a project
        // depends on so a bundle can copy it, and this is a destination. A
        // bundle chasing a recording that has not happened yet would report a
        // missing file for something working exactly as intended.
        "file" => Param::str("out.mp4").with_label("File"),
        "record" => Param::bool(false).with_label("Record"),
        "fps" => Param::float(60.0).with_label("Frame Rate").with_range(1.0, 240.0),
        "codec" => Param::menu("h264", &["h264", "h265", "prores"]).with_label("Codec"),
        "quality" => Param::int(75).with_label("Quality").with_range(0.0, 100.0),
    }
}

fn params_chop_to_top() -> IndexMap<String, Param> {
    params! {
        "layout" => Param::menu("mono", &["mono", "rgba"])
            .with_label("Channel Layout"),
    }
}

fn params_movie_in() -> IndexMap<String, Param> {
    params! {
        "file" => Param::str("").with_label("File").as_file_ref(),
        "play" => Param::menu("loop", &["loop", "once", "hold"]).with_label("Play"),
        "speed" => Param::float(1.0).with_label("Speed").with_range(-4.0, 4.0),
        // Not a resolution: the picture's own size wins. This is what to
        // show before the first frame arrives, and if the file is missing.
        "resw" => Param::int(1280).with_label("Fallback W").with_range(1.0, 16384.0),
        "resh" => Param::int(720).with_label("Fallback H").with_range(1.0, 16384.0),
    }
}

fn params_video_device_in() -> IndexMap<String, Param> {
    params! {
        "device" => Param::str("").with_label("Device (blank = default)"),
        // Requested, not commanded: a camera only does the modes it does, so
        // these are negotiated to the nearest one the device reports.
        "resw" => Param::int(1280).with_label("Requested W").with_range(1.0, 16384.0),
        "resh" => Param::int(720).with_label("Requested H").with_range(1.0, 16384.0),
        "fps" => Param::float(30.0).with_label("Requested Frame Rate").with_range(1.0, 240.0),
        "active" => Param::bool(true).with_label("Active"),
    }
}

fn params_render() -> IndexMap<String, Param> {
    with_res(params! {
        "geometry" => Param::str("").with_label("Geometry").as_path_ref(),
        "camera" => Param::str("").with_label("Camera").as_path_ref(),
        "light" => Param::str("").with_label("Light").as_path_ref(),
        "background" => Param::rgba([0.0, 0.0, 0.0, 1.0]).with_label("Background"),
        "ambient" => Param::float(0.12).with_label("Ambient").with_range(0.0, 1.0),
        "wireframe" => Param::bool(false).with_label("Wireframe"),
        "cull" => Param::menu("back", &["back", "front", "none"]).with_label("Cull"),
    })
}

// -------------------------------------------------------------- select TOP

fn params_select() -> IndexMap<String, Param> {
    params! {
        "top" => Param::str("").with_label("TOP"),
    }
}

// --------------------------------------------------------------- cache TOP

fn params_cache() -> IndexMap<String, Param> {
    params! {
        "active" => Param::bool(true).with_label("Active"),
    }
}

// ---------------------------------------------------------- resolution TOP

fn params_resolution() -> IndexMap<String, Param> {
    with_res(params! {
        "filter" => Param::menu("linear", &["linear", "nearest"]).with_label("Filter"),
    })
}

// ------------------------------------------------------------ displace TOP

fn params_displace() -> IndexMap<String, Param> {
    params! {
        "amount" => Param::float(0.1).with_label("Amount").with_range(-1.0, 1.0),
        "sourcex" => Param::menu("r", &["r", "g", "b", "a", "luminance"]).with_label("Source X"),
        "sourcey" => Param::menu("g", &["r", "g", "b", "a", "luminance"]).with_label("Source Y"),
        "offset" => Param::float(-0.5).with_label("Offset").with_range(-1.0, 1.0),
        "extend" => Param::menu("hold", &["zero", "hold", "repeat", "mirror"]).with_label("Extend"),
    }
}

fn pack_displace(n: &Node, c: &EvalContext) -> PackedParams {
    [
        [
            f(n, c, "amount"),
            menu(n, c, "sourcex"),
            menu(n, c, "sourcey"),
            f(n, c, "offset"),
        ],
        [menu(n, c, "extend"), 0.0, 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
    ]
}

// ---------------------------------------------------------------- GLSL TOP

/// The starting shader in a new GLSL TOP — a moving plasma, so the operator
/// does something the moment it is created.
const DEFAULT_WGSL: &str = "\
// WGSL fragment. Available: in.uv, U.time.x (seconds), U.res.xy,
// sample0(uv) / sample1(uv) for the two inputs, U.p0..U.p3 (Uniform 1..4).
let p = (in.uv - 0.5) * vec2<f32>(U.res.x / U.res.y, 1.0);
let d = length(p) * 8.0 - U.time.x * 2.0;
let v = 0.5 + 0.5 * sin(d + atan2(p.y, p.x) * 3.0);
return vec4<f32>(v * U.p0.rgb, 1.0);
";

fn params_glsl() -> IndexMap<String, Param> {
    with_res(params! {
        "language" => Param::menu("wgsl", &["wgsl", "glsl"]).with_label("Language"),
        "source" => Param::str(DEFAULT_WGSL).with_label("Source"),
        "uniform1" => Param::rgba([1.0, 0.6, 0.2, 1.0]).with_label("Uniform 1"),
        "uniform2" => Param::rgba([0.0, 0.0, 0.0, 0.0]).with_label("Uniform 2"),
        "uniform3" => Param::rgba([0.0, 0.0, 0.0, 0.0]).with_label("Uniform 3"),
        "uniform4" => Param::rgba([0.0, 0.0, 0.0, 0.0]).with_label("Uniform 4"),
    })
}

fn pack_glsl(n: &Node, c: &EvalContext) -> PackedParams {
    let plain = || {
        [
            v4(n, c, "uniform1"),
            v4(n, c, "uniform2"),
            v4(n, c, "uniform3"),
            v4(n, c, "uniform4"),
        ]
    };

    // A GLSL TOP with custom parameters — the shape ISF import leaves behind,
    // and one an author can build by hand — packs those instead of the four
    // generic uniforms. `crate::isf::layout` decides where each one lands, and
    // the shader's `#define`s were written from that same function, so the two
    // sides agree by construction rather than by comment.
    let custom: Vec<(&String, &Param)> = n
        .params
        .iter()
        .filter(|(_, p)| p.custom && isf::width(&p.value) > 0)
        .collect();
    if custom.is_empty() {
        return plain();
    }
    let Some(slots) = isf::layout(custom.iter().map(|(_, p)| isf::width(&p.value))) else {
        // More parameters than uniform space. Import refuses this case; a
        // hand-built node can still reach it, and falling back is better than
        // writing values into the wrong slots.
        return plain();
    };

    let mut out = [[0.0f32; 4]; 4];
    for ((key, param), slot) in custom.iter().zip(slots) {
        let value = param.eval(c);
        match slot.width {
            4 => out[slot.vec] = value.as_vec4_f32(),
            2 => {
                let v = value.as_vec4_f32();
                out[slot.vec][slot.component] = v[0];
                out[slot.vec][slot.component + 1] = v[1];
            }
            _ => {
                out[slot.vec][slot.component] = if param.menu.is_some() {
                    menu(n, c, key)
                } else {
                    value.as_f32()
                };
            }
        }
    }
    out
}

/// Whether a video node has been given nothing to open.
///
/// A blank `file` means no clip and nothing to do. A blank `device` means the
/// *default* camera — which is what that parameter's own label says, "blank =
/// default", and what `video::Source::camera` resolves against the platform's
/// first capture input.
///
/// The two were treated the same once, and the result was that a Video Device
/// In produced black for ever the moment you created one. No status on the
/// node, no ffmpeg started, nothing to find: the failure came *before* any
/// question of camera permissions, so every investigation went to the wrong
/// place. Its own function so the distinction can be tested without a camera,
/// a GPU or a cook.
pub fn nothing_to_open(is_camera: bool, source: &str) -> bool {
    !is_camera && source.trim().is_empty()
}

/// The shader source and language a GLSL TOP is currently set to.
pub fn shader_source(node: &Node, ctx: &EvalContext) -> (String, bool) {
    let source = val(node, ctx, "source").as_str();
    let is_glsl = val(node, ctx, "language").as_str() == "glsl";
    (source, is_glsl)
}

// -------------------------------------------------------------- the table

fn specs() -> &'static Vec<TopSpec> {
    static SPECS: OnceLock<Vec<TopSpec>> = OnceLock::new();
    SPECS.get_or_init(|| {
        vec![
            TopSpec {
                def: OpDef {
                    type_name: "constantTOP",
                    input_families: &[],
                    label: "Constant",
                    family: Family::Top,
                    inputs: &[],
                    summary: "A flat colour at a chosen resolution.",
                    time_dependent: false,
                    params: params_constant,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/constant.wgsl"),
                sizing: Sizing::Params,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_constant,
            },
            TopSpec {
                def: OpDef {
                    type_name: "noiseTOP",
                    input_families: &[],
                    label: "Noise",
                    family: Family::Top,
                    inputs: &[],
                    summary: "Fractal value noise. Animate Translate Z to make it move.",
                    time_dependent: false,
                    params: params_noise,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/noise.wgsl"),
                sizing: Sizing::Params,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_noise,
            },
            TopSpec {
                def: OpDef {
                    type_name: "rampTOP",
                    input_families: &[],
                    label: "Ramp",
                    family: Family::Top,
                    inputs: &[],
                    summary: "Linear or radial gradient between two colours.",
                    time_dependent: false,
                    params: params_ramp,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/ramp.wgsl"),
                sizing: Sizing::Params,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_ramp,
            },
            // The engine special-cases both of these: their pixels come from
            // a decoder thread rather than from a shader, and their size
            // comes from the picture rather than from a parameter. The Null
            // shader is what copies the uploaded frame into the output.
            TopSpec {
                def: OpDef {
                    type_name: MOVIE_IN,
                    input_families: &[],
                    label: "Movie File In",
                    family: Family::Top,
                    inputs: &[],
                    summary: "Plays an image or a movie file.",
                    time_dependent: true,
                    params: params_movie_in,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Params,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: VIDEO_DEVICE_IN,
                    input_families: &[],
                    label: "Video Device In",
                    family: Family::Top,
                    inputs: &[],
                    summary: "Frames from a camera or capture device.",
                    time_dependent: true,
                    params: params_video_device_in,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Params,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: "levelTOP",
                    input_families: &[],
                    label: "Level",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "Brightness, contrast, gamma, black/white levels.",
                    time_dependent: false,
                    params: params_level,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/level.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_level,
            },
            TopSpec {
                def: OpDef {
                    type_name: "transformTOP",
                    input_families: &[],
                    label: "Transform",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "Translate, rotate and scale, with an extend mode.",
                    time_dependent: false,
                    params: params_transform,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/transform.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_transform,
            },
            TopSpec {
                def: OpDef {
                    type_name: "blurTOP",
                    input_families: &[],
                    label: "Blur",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "Separable Gaussian blur.",
                    time_dependent: false,
                    params: params_blur,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/blur.wgsl"),
                sizing: Sizing::Input0,
                two_pass: true,
                dynamic_shader: false,
                pack: pack_blur,
            },
            TopSpec {
                def: OpDef {
                    type_name: "compositeTOP",
                    input_families: &[],
                    label: "Composite",
                    family: Family::Top,
                    inputs: &["base", "over"],
                    summary: "Blend two inputs. Input 2 is composited over input 1.",
                    time_dependent: false,
                    params: params_composite,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/composite.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_composite,
            },
            TopSpec {
                def: OpDef {
                    type_name: "switchTOP",
                    input_families: &[],
                    label: "Switch",
                    family: Family::Top,
                    inputs: &["0", "1"],
                    summary: "Select one of two inputs, optionally blending between them.",
                    time_dependent: false,
                    params: params_switch,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/switch.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_switch,
            },
            TopSpec {
                def: OpDef {
                    type_name: NULL,
                    input_families: &[],
                    label: "Null",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "Pass-through. A stable name to reference and to view.",
                    time_dependent: false,
                    params: params_none,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: FEEDBACK,
                    input_families: &[],
                    label: "Feedback",
                    family: Family::Top,
                    // No wired input on purpose: the target is named by
                    // parameter, which is how a feedback loop is expressed
                    // without putting a cycle in the cook graph (PLAN.md §4).
                    inputs: &[],
                    summary: "Last frame's output of the Target TOP.",
                    time_dependent: true,
                    params: params_feedback,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Referenced,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: IN,
                    input_families: &[],
                    label: "In",
                    family: Family::Top,
                    // Fed from outside the component, not by a wire in here.
                    inputs: &[],
                    summary: "A texture input on this component's node.",
                    time_dependent: false,
                    params: params_none,
                    connector: Connector::In,
                },
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Referenced,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: SELECT,
                    input_families: &[],
                    label: "Select",
                    family: Family::Top,
                    // Like Feedback, named by parameter — but this one *is*
                    // declared as a dependency, so it reads the current frame.
                    inputs: &[],
                    summary: "This frame's output of another TOP, by path.",
                    time_dependent: false,
                    params: params_select,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Referenced,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: OUT,
                    input_families: &[],
                    label: "Out",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "This component's texture output.",
                    time_dependent: false,
                    params: params_none,
                    connector: Connector::Out,
                },
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: CACHE,
                    input_families: &[],
                    label: "Cache",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "Holds the last frame it saw when Active is off.",
                    time_dependent: false,
                    params: params_cache,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: "resolutionTOP",
                    input_families: &[],
                    label: "Resolution",
                    family: Family::Top,
                    inputs: &["in"],
                    summary: "Resamples its input to an explicit resolution.",
                    time_dependent: false,
                    params: params_resolution,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Params,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: "displaceTOP",
                    input_families: &[],
                    label: "Displace",
                    family: Family::Top,
                    inputs: &["source", "displace"],
                    summary: "Offsets input 1's lookup by channels of input 2.",
                    time_dependent: false,
                    params: params_displace,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/displace.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_displace,
            },
            TopSpec {
                def: OpDef {
                    type_name: RENDER,
                    input_families: &[],
                    label: "Render",
                    family: Family::Top,
                    inputs: &[],
                    summary: "Draws Geometry components through a Camera.",
                    // The scene it draws is almost always moving, and proving
                    // otherwise would mean tracking every operator it reaches.
                    time_dependent: true,
                    params: params_render,
                    connector: Connector::None,
                },
                shader: "",
                sizing: Sizing::Params,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: "thresholdTOP",
                    label: "Threshold",
                    family: Family::Top,
                    inputs: &["in"],
                    input_families: &[],
                    summary: "Split the image in two at a level, with a soft edge.",
                    time_dependent: false,
                    params: params_threshold,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/threshold.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_threshold,
            },
            TopSpec {
                def: OpDef {
                    type_name: "edgeTOP",
                    label: "Edge",
                    family: Family::Top,
                    inputs: &["in"],
                    input_families: &[],
                    summary: "Sobel edge detection.",
                    time_dependent: false,
                    params: params_edge,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/edge.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_edge,
            },
            TopSpec {
                def: OpDef {
                    type_name: "hsvadjustTOP",
                    label: "HSV Adjust",
                    family: Family::Top,
                    inputs: &["in"],
                    input_families: &[],
                    summary: "Hue, saturation and value, optionally over one band of the wheel.",
                    time_dependent: false,
                    params: params_hsv,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/hsv.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_hsv,
            },
            TopSpec {
                def: OpDef {
                    type_name: "flipTOP",
                    label: "Flip",
                    family: Family::Top,
                    inputs: &["in"],
                    input_families: &[],
                    summary: "Mirror the image about an axis, or transpose it.",
                    time_dependent: false,
                    params: params_flip,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/flip.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_flip,
            },
            TopSpec {
                def: OpDef {
                    type_name: "mirrorTOP",
                    label: "Mirror",
                    family: Family::Top,
                    inputs: &["in"],
                    input_families: &[],
                    summary: "Fold the image onto itself, including a radial kaleidoscope.",
                    time_dependent: false,
                    params: params_mirror,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/mirror.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_mirror,
            },
            TopSpec {
                def: OpDef {
                    type_name: "lookupTOP",
                    label: "Lookup",
                    family: Family::Top,
                    inputs: &["index", "table"],
                    input_families: &[],
                    summary: "Input 1's brightness reads a colour out of input 2.",
                    time_dependent: false,
                    params: params_lookup,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/lookup.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_lookup,
            },
            TopSpec {
                def: OpDef {
                    type_name: "circleTOP",
                    label: "Circle",
                    family: Family::Top,
                    inputs: &[],
                    input_families: &[],
                    summary: "An antialiased disc or ellipse.",
                    time_dependent: false,
                    params: params_circle,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/circle.wgsl"),
                sizing: Sizing::Params,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_circle,
            },
            TopSpec {
                def: OpDef {
                    type_name: "rectangleTOP",
                    label: "Rectangle",
                    family: Family::Top,
                    inputs: &[],
                    input_families: &[],
                    summary: "A rectangle, with rounded corners and an optional border.",
                    time_dependent: false,
                    params: params_rectangle,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/rectangle.wgsl"),
                sizing: Sizing::Params,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_rectangle,
            },
            TopSpec {
                def: OpDef {
                    type_name: "chromakeyTOP",
                    label: "Chroma Key",
                    family: Family::Top,
                    inputs: &["in"],
                    input_families: &[],
                    summary: "Key out one colour, matching on hue rather than brightness.",
                    time_dependent: false,
                    params: params_chromakey,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/chromakey.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_chromakey,
            },
            TopSpec {
                def: OpDef {
                    type_name: "mathTOP",
                    label: "Math",
                    family: Family::Top,
                    inputs: &["a", "b"],
                    input_families: &[],
                    summary: "Arithmetic on two inputs, per channel.",
                    time_dependent: false,
                    params: params_math,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/math.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_math,
            },
            TopSpec {
                def: OpDef {
                    type_name: TEXT,
                    label: "Text",
                    family: Family::Top,
                    inputs: &[],
                    input_families: &[],
                    summary: "Draws text, using a font file or the system's.",
                    time_dependent: false,
                    params: params_text,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/text.wgsl"),
                sizing: Sizing::Params,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_text,
            },
            TopSpec {
                def: OpDef {
                    type_name: MOVIE_OUT,
                    label: "Movie File Out",
                    family: Family::Top,
                    inputs: &["in"],
                    input_families: &[],
                    summary: "Records its input to a movie file. Passes the picture through.",
                    // Whether it cooks decides whether a frame is recorded, so
                    // it cannot be allowed to sit out a frame because nothing
                    // upstream changed.
                    time_dependent: true,
                    params: params_movie_out,
                    connector: Connector::None,
                },
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Input0,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: CHOP_TO_TOP,
                    label: "CHOP to TOP",
                    family: Family::Top,
                    inputs: &["in"],
                    input_families: &[Family::Chop],
                    summary: "Channels as pixels: a row per channel, a column per sample.",
                    // A CHOP is a moving target by assumption — see the
                    // GLSL TOP's note about proving otherwise.
                    time_dependent: true,
                    params: params_chop_to_top,
                    connector: Connector::None,
                },
                // Uploaded, then blitted; there is no operator shader.
                shader: include_str!("shaders/null.wgsl"),
                sizing: Sizing::Referenced,
                two_pass: false,
                dynamic_shader: false,
                pack: pack_none,
            },
            TopSpec {
                def: OpDef {
                    type_name: GLSL,
                    input_families: &[],
                    label: "GLSL",
                    family: Family::Top,
                    inputs: &["in0", "in1"],
                    summary: "Your own shader, compiled live. WGSL or Shadertoy-style GLSL.",
                    // Assumed animated: a user shader almost always reads
                    // time, and there is no cheap way to prove it doesn't.
                    // TouchDesigner's GLSL TOP cooks every frame too.
                    time_dependent: true,
                    params: params_glsl,
                    connector: Connector::None,
                },
                // Unused: the source comes from the `source` parameter.
                shader: "",
                sizing: Sizing::Input0OrParams,
                two_pass: false,
                dynamic_shader: true,
                pack: pack_glsl,
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
    input_families: &[],
    label: "Container",
    family: Family::Comp,
    inputs: &[],
    summary: "A component that holds a sub-network.",
    time_dependent: false,
    params: params_none,
    connector: Connector::None,
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

#[cfg(test)]
mod video_source_tests {
    use super::nothing_to_open;

    #[test]
    fn a_blank_device_means_the_default_camera_not_nothing() {
        // The regression this pins: a Video Device In is created with `device`
        // blank, because its label says blank is the default. Reading that as
        // "no source" made the node the user had just added the one node that
        // could never produce a picture — black, silent, and with no ffmpeg
        // started, so nothing downstream of it and nothing in any log said why.
        assert!(!nothing_to_open(true, ""), "blank device = default camera");
        assert!(!nothing_to_open(true, "   "));
        assert!(!nothing_to_open(true, "1"), "a named device, obviously");

        // A movie with no file really is nothing to open.
        assert!(nothing_to_open(false, ""));
        assert!(nothing_to_open(false, "  \t "));
        assert!(!nothing_to_open(false, "/clips/a.mov"));
    }
}
