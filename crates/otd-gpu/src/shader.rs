//! User shader handling for the GLSL TOP.
//!
//! PLAN.md §3: "WGSL primary, GLSL accepted — paste Shadertoy GLSL and it
//! works." Two things make that practical:
//!
//!  * **Wrapping.** The artist writes a fragment *body* in WGSL, or a
//!    Shadertoy `mainImage` in GLSL. The boilerplate — bindings, the
//!    fullscreen vertex stage, the `iTime`/`iResolution` names — is supplied.
//!    A source that already declares its own entry point is passed through
//!    untouched, so nothing is locked out.
//!  * **Validating before wgpu sees it.** naga is run over the source first.
//!    A typo then produces a message with a line number instead of a device
//!    error, which is the difference between a usable live-coding surface and
//!    an unusable one.

use crate::ops::COMMON_WGSL;

/// The prelude on its own, used as the vertex stage when the fragment stage
/// was written in GLSL and therefore lives in a separate module.
pub const VERTEX_ONLY_WGSL: &str = COMMON_WGSL;

/// The fullscreen triangle again, in GLSL.
///
/// A GLSL fragment stage cannot be paired with the WGSL vertex stage: naga's
/// GLSL front-end emits interpolation sampling `None` for a plain `in`, WGSL
/// emits `Center`, and wgpu rejects the mismatch. Compiling both stages
/// through the same front-end sidesteps it.
pub const VERTEX_GLSL: &str = r#"#version 450 core
layout(location = 0) out vec2 otd_uv;
void main() {
    float x = float((gl_VertexIndex << 1) & 2);
    float y = float(gl_VertexIndex & 2);
    otd_uv = vec2(x, y);
    gl_Position = vec4(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}
"#;

/// Wrap a WGSL fragment body, unless it declares its own entry point.
pub fn wrap_wgsl(source: &str) -> String {
    if source.contains("@fragment") {
        format!("{COMMON_WGSL}\n{source}")
    } else {
        format!(
            "{COMMON_WGSL}\n@fragment\nfn fs_main(in: VOut) -> @location(0) vec4<f32> {{\n{source}\n}}\n"
        )
    }
}

const GLSL_PREAMBLE: &str = r#"#version 450 core
layout(location = 0) in vec2 otd_uv;
layout(location = 0) out vec4 otd_frag;
layout(set = 0, binding = 0) uniform Uniforms {
    vec4 res;
    vec4 time;
    vec4 p0;
    vec4 p1;
    vec4 p2;
    vec4 p3;
    vec4 p4;
    vec4 p5;
    vec4 p6;
    vec4 p7;
    vec4 p8;
    vec4 p9;
    vec4 p10;
    vec4 p11;
} U;
#define iResolution vec3(U.res.xy, 1.0)
#define iTime U.time.x
#define iTimeDelta (1.0 / max(U.time.w, 1.0))
#define iFrame int(U.time.z)
#define iMouse vec4(0.0)
#define iDate vec4(0.0)
"#;

/// Texture inputs, declared only when a shader actually samples them.
///
/// naga's GLSL front-end maps a combined `sampler2D` onto our separate
/// texture and sampler bindings, which is what lets an imported ISF shader
/// call `texture(img, uv)` unchanged.
const GLSL_SAMPLERS: &str = r#"layout(set = 0, binding = 1) uniform sampler otd_samp;
layout(set = 0, binding = 2) uniform texture2D otd_tex0;
layout(set = 0, binding = 3) uniform texture2D otd_tex1;
#define otd_image0 sampler2D(otd_tex0, otd_samp)
#define otd_image1 sampler2D(otd_tex1, otd_samp)
#define iChannel0 otd_image0
#define iChannel1 otd_image1
"#;

/// Shadertoy samples its channels bottom-up, like the `fragCoord` it hands
/// out. Our textures are top-down.
///
/// So a shader doing the most ordinary thing there is —
/// `texture(iChannel0, fragCoord / iResolution.xy)` — got the picture upside
/// down, silently, in a way that reads as a deliberate effect until you put a
/// camera on it and watch yourself hang from the ceiling. Every generated
/// shader that read its input had it, and so did two of the camera recipes.
///
/// Flipping `fragCoord` instead would fix the passthrough and mirror every
/// generative shader copied off shadertoy.com, so the flip goes where the
/// mismatch actually is: the lookup, and only for our own channels.
///
/// Rewritten at the call site rather than wrapped in a helper, because naga's
/// GLSL front end will not take a `sampler2D` as a function parameter — and
/// not as `#define texture(...)` either, because its preprocessor leaves the
/// builtin alone, so that one compiled, did nothing, and left the picture
/// upside down, which is worse than not trying.
fn flip_channel_lookups(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let call: Vec<char> = "texture(".chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        // `texture(` cannot be the tail of `textureLod(` or `textureGrad(`,
        // but it can be the tail of an identifier somebody declared, so the
        // character before it has to be a non-identifier one.
        let starts_here = chars[i..].starts_with(&call)
            && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_'));
        let rewritten = starts_here
            .then(|| matching_paren(&chars, i + call.len() - 1))
            .flatten()
            .and_then(|close| {
                let inner: String = chars[i + call.len()..close].iter().collect();
                let args = split_args(&inner);
                // Two arguments, and the sampler is one of ours. Anything
                // else is left exactly as it was written.
                let ours = args.first().is_some_and(|a| {
                    let a = a.trim();
                    a.starts_with("iChannel") || a.starts_with("otd_image")
                });
                (args.len() == 2 && ours).then(|| {
                    (
                        format!(
                            "texture({}, vec2(({1}).x, 1.0 - ({1}).y))",
                            args[0].trim(),
                            args[1].trim()
                        ),
                        close,
                    )
                })
            });
        match rewritten {
            Some((text, close)) => {
                out.push_str(&text);
                i = close + 1;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

/// The index of the `)` closing the `(` at `open`.
fn matching_paren(chars: &[char], open: usize) -> Option<usize> {
    if chars.get(open) != Some(&'(') {
        return None;
    }
    let mut depth = 0i32;
    for (at, c) in chars.iter().enumerate().skip(open) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split an argument list on the commas that are not inside brackets.
fn split_args(inner: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in inner.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                args.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() || !args.is_empty() {
        args.push(current);
    }
    args
}
/// Shadertoy's fragCoord has its origin bottom-left; ours is top-left.
const GLSL_MAIN: &str = r#"
void main() {
    vec2 fragCoord = vec2(otd_uv.x, 1.0 - otd_uv.y) * U.res.xy;
    mainImage(otd_frag, fragCoord);
}
"#;

/// Wrap Shadertoy-style GLSL. A source that already has `void main()` is
/// assembled without the shim.
pub fn wrap_glsl(source: &str) -> String {
    let mut out = String::from(GLSL_PREAMBLE);
    // Declaring samplers a shader never uses would still bind them, so only
    // pay for them when the source mentions one.
    let flip_channels = source.contains("iChannel");
    if source.contains("otd_image") || flip_channels {
        out.push_str(GLSL_SAMPLERS);
    }
    // ISF has its own accessors and already flips inside them, so only a
    // source written against Shadertoy's names is rewritten.
    let body = match flip_channels {
        true => std::borrow::Cow::Owned(flip_channel_lookups(source)),
        false => std::borrow::Cow::Borrowed(source),
    };
    out.push_str(&body);
    if !source.contains("void main(") {
        out.push_str(GLSL_MAIN);
    }
    out
}

pub fn validate_wgsl(source: &str) -> Result<(), String> {
    use wgpu::naga;
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| trim_location(&e.emit_to_string(source)))?;
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .map_err(|e| trim_location(&e.emit_to_string(source)))?;
    Ok(())
}

pub fn validate_glsl(source: &str) -> Result<(), String> {
    use wgpu::naga;
    let mut frontend = naga::front::glsl::Frontend::default();
    let options = naga::front::glsl::Options::from(naga::ShaderStage::Fragment);
    let module = frontend
        .parse(&options, source)
        .map_err(|e| trim_location(&e.emit_to_string(source)))?;
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .map_err(|e| trim_location(&e.emit_to_string(source)))?;
    Ok(())
}

/// naga's diagnostics are several lines of source excerpt. The parameter
/// panel has one line, so keep the first two and drop the ASCII art.
fn trim_location(message: &str) -> String {
    let useful: Vec<&str> = message
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('|') && !l.chars().all(|c| "-^ ".contains(c)))
        .take(3)
        .collect();
    if useful.is_empty() {
        message.trim().to_string()
    } else {
        useful.join(" — ")
    }
}

#[cfg(test)]
mod channel_flip {
    use super::*;

    #[test]
    fn our_channels_are_flipped_and_nothing_else_is() {
        let out = flip_channel_lookups(
            "texture(iChannel0, uv) + texture(iChannel1, vec2(u.x, f(a, b))) \
             + textureLod(iChannel0, uv, 0.0) + texture(other, uv) \
             + texture(iChannel0, uv, 0.5) + mytexture(iChannel0, uv)",
        );
        assert!(out.contains("texture(iChannel0, vec2((uv).x, 1.0 - (uv).y))"));
        // A comma inside the coordinate expression is not an argument break.
        assert!(out.contains(
            "texture(iChannel1, vec2((vec2(u.x, f(a, b))).x, 1.0 - (vec2(u.x, f(a, b))).y))"
        ));
        // Left alone: a different function, a different sampler, the
        // three-argument form, and an identifier that merely ends in it.
        assert!(out.contains("textureLod(iChannel0, uv, 0.0)"));
        assert!(out.contains("texture(other, uv)"));
        assert!(out.contains("texture(iChannel0, uv, 0.5)"));
        assert!(out.contains("mytexture(iChannel0, uv)"));
    }

    #[test]
    fn a_shadertoy_passthrough_compiles_after_the_rewrite() {
        let src = "void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n\
                   vec2 uv = fragCoord / iResolution.xy;\n\
                   fragColor = texture(iChannel0, uv);\n}";
        let full = wrap_glsl(src);
        assert!(full.contains("vec2((uv).x, 1.0 - (uv).y)"));
        validate_glsl(&full).expect("the rewritten passthrough must still compile");
    }

    #[test]
    fn an_isf_source_is_left_alone() {
        // ISF flips inside IMG_NORM_PIXEL already; doing it again is the same
        // bug the other way up.
        let src = "void main() { gl_FragColor = texture(otd_image0, isf_FragNormCoord); }";
        let full = wrap_glsl(src);
        assert!(full.contains("texture(otd_image0, isf_FragNormCoord)"));
        assert!(!full.contains("1.0 - (isf_FragNormCoord).y"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wgsl_body_is_wrapped_and_compiles() {
        let src = "return vec4<f32>(in.uv, 0.0, 1.0);";
        validate_wgsl(&wrap_wgsl(src)).expect("wrapped body should compile");
    }

    #[test]
    fn a_full_wgsl_shader_is_passed_through() {
        let src =
            "@fragment\nfn fs_main(in: VOut) -> @location(0) vec4<f32> { return sample0(in.uv); }";
        validate_wgsl(&wrap_wgsl(src)).expect("full shader should compile");
    }

    #[test]
    fn a_wgsl_typo_reports_an_error_rather_than_panicking() {
        let err = validate_wgsl(&wrap_wgsl("return vec4<f32>(nope);")).unwrap_err();
        assert!(!err.is_empty());
        assert!(err.len() < 400, "error should be a summary, got: {err}");
    }

    #[test]
    fn shadertoy_glsl_compiles() {
        let src = "void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n\
                   vec2 uv = fragCoord / iResolution.xy;\n\
                   fragColor = vec4(uv, 0.5 + 0.5 * sin(iTime), 1.0);\n\
                   }";
        if let Err(e) = validate_glsl(&wrap_glsl(src)) {
            panic!("Shadertoy-style GLSL failed to compile: {e}");
        }
    }

    /// The uniform block is declared three times — the Rust struct, the WGSL
    /// prelude and the GLSL one — and a shader reading `U.p9` from a block
    /// that stops at `p3` is a validation error a long way from its cause.
    #[test]
    fn both_uniform_blocks_declare_every_parameter_vector() {
        let last = crate::ops::PARAM_VECS - 1;
        for (language, source) in [("GLSL", GLSL_PREAMBLE), ("WGSL", COMMON_WGSL)] {
            assert!(
                source.contains(&format!("p{last}")),
                "the {language} uniform block stops short of p{last}"
            );
            assert!(
                !source.contains(&format!("p{}", last + 1)),
                "the {language} uniform block declares more than ops::PARAM_VECS vectors"
            );
        }
    }

    #[test]
    fn a_glsl_typo_reports_an_error() {
        let src = "void mainImage(out vec4 fragColor, in vec2 fragCoord) { fragColor = nope; }";
        assert!(validate_glsl(&wrap_glsl(src)).is_err());
    }
}
