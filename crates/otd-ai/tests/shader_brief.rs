//! The GLSL brief in the system prompt, checked against the real compiler.
//!
//! The failure this exists to stop is the quiet one. The prompt told the model
//! about `iTime` and `iResolution` and said nothing about how to read the
//! operator's input, so a model asked for "effects on top of the video"
//! reached for the names it knows from TouchDesigner — `sTD2DInputs[0]`,
//! `sIn0`, a bare `uniform1` — none of which exist here. Every one of them is
//! a shader that will not compile, and a glslTOP that will not compile is a
//! black texture that every operator downstream of it faithfully passes on.
//! The patch looks built. The viewer is black.
//!
//! So the brief has to name the sampler, and the names it promises have to be
//! the ones naga takes. Those are two files that would otherwise drift apart
//! silently — the prompt is prose and the compiler is code, and nothing but
//! this test makes them agree.

use otd_ai::patch;

fn compiles(source: &str) -> Result<(), String> {
    otd_gpu::shader::validate_glsl(&otd_gpu::shader::wrap_glsl(source))
}

/// Wrapped in `mainImage` so each snippet is a whole shader.
fn shader(body: &str) -> String {
    format!("void mainImage(out vec4 fragColor, in vec2 fragCoord) {{\n{body}\n}}")
}

#[test]
fn the_names_the_brief_promises_are_the_ones_that_compile() {
    for (what, body) in [
        ("iChannel0", "fragColor = texture(iChannel0, fragCoord / iResolution.xy);"),
        ("iChannel1", "fragColor = texture(iChannel1, fragCoord / iResolution.xy);"),
        ("U.p0 (uniform1)", "fragColor = vec4(U.p0.rgb, 1.0);"),
        ("iTime", "fragColor = vec4(vec3(sin(iTime)), 1.0);"),
        ("iFrame", "fragColor = vec4(vec3(float(iFrame)), 1.0);"),
    ] {
        if let Err(e) = compiles(&shader(body)) {
            panic!("the brief promises {what} and it does not compile: {e}");
        }
    }
}

#[test]
fn the_names_the_brief_warns_off_really_are_rejected() {
    // If one of these ever starts working, the warning is misinformation and
    // should come out of the prompt rather than sit there costing tokens.
    for (what, body) in [
        ("sTD2DInputs[0]", "fragColor = texture(sTD2DInputs[0], fragCoord / iResolution.xy);"),
        ("sIn0", "fragColor = texture(sIn0, fragCoord / iResolution.xy);"),
        ("a bare uniform1", "fragColor = vec4(uniform1.rgb, 1.0);"),
        ("texture2D", "fragColor = texture2D(iChannel0, fragCoord / iResolution.xy);"),
    ] {
        assert!(
            compiles(&shader(body)).is_err(),
            "the brief warns off {what}, but it compiles — fix the brief"
        );
    }
    // Declaring the sampler yourself, the other habit worth naming.
    assert!(
        compiles(&format!(
            "uniform sampler2D iChannel0;\n{}",
            shader("fragColor = texture(iChannel0, fragCoord / iResolution.xy);")
        ))
        .is_err(),
        "the brief says not to declare iChannel0; it apparently now works"
    );
}

#[test]
fn the_worked_example_in_the_prompt_compiles() {
    // Pulled out of the prompt itself rather than retyped: an example that
    // does not compile teaches the model to write shaders that do not.
    let prompt = patch::system_prompt(&otd_engine::registry());
    let example = prompt
        .split("void mainImage")
        .nth(2)
        .expect("the prompt should carry a worked input-sampling example");
    let body = format!(
        "void mainImage{}",
        example.split("\n\n").next().unwrap_or_default()
    );
    // Un-indent: it sits in a prose block.
    let body: String = body
        .lines()
        .map(|l| l.strip_prefix("      ").unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n");
    if let Err(e) = compiles(&body) {
        panic!("the prompt's own example does not compile: {e}\n---\n{body}");
    }
    assert!(
        body.contains("iChannel0"),
        "the example is meant to show reading the input:\n{body}"
    );
}

#[test]
fn the_brief_tells_the_model_how_to_read_its_input() {
    let prompt = patch::system_prompt(&otd_engine::registry());
    assert!(prompt.contains("iChannel0"), "no input sampler named at all");
    assert!(
        prompt.contains("sTD2DInputs"),
        "the TouchDesigner name is the one a model reaches for; say it is wrong"
    );
}
