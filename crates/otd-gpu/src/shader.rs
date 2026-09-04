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
} U;
#define iResolution vec3(U.res.xy, 1.0)
#define iTime U.time.x
#define iTimeDelta (1.0 / max(U.time.w, 1.0))
#define iFrame int(U.time.z)
#define iMouse vec4(0.0)
#define iDate vec4(0.0)
"#;

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
    out.push_str(source);
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

    #[test]
    fn a_glsl_typo_reports_an_error() {
        let src = "void mainImage(out vec4 fragColor, in vec2 fragCoord) { fragColor = nope; }";
        assert!(validate_glsl(&wrap_glsl(src)).is_err());
    }
}
