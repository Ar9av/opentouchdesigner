//! Operator reference, generated from the registry.
//!
//! PLAN.md §6 calls docs half the moat and says to write them alongside each
//! operator, "enforced in PR review". Review is a weak enforcement mechanism —
//! it works until the week somebody is busy. So the reference is *generated*
//! from the same `OpRegistry` the editor builds its menus and parameter pages
//! from, and a test fails the build when an operator or a parameter arrives
//! without prose.
//!
//! What that buys is docs that cannot drift. A renamed parameter, a changed
//! default, a new menu entry — the page says whatever the operator actually
//! does, because it is reading the operator. What it cannot generate is the
//! part that matters most, the *why* and the worked example, so those live in
//! `NOTES` here, keyed by operator, and are woven into the generated page.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use otd_core::{Family, OpRegistry, Param, Value};

/// Hand-written notes for operators that need more than a one-line summary.
///
/// Deliberately not exhaustive. A Constant TOP does not need a paragraph; the
/// operators here are the ones with a behaviour you would otherwise have to
/// discover by being surprised by it.
const NOTES: &[(&str, &str)] = &[
    (
        "feedbackTOP",
        "Reads the target TOP's output from the **previous** frame. That is what \
         lets a feedback loop exist without a cycle in the cook graph — the Select \
         TOP, which reads the current frame, would be a cycle and is rejected.\n\n\
         Point `Target TOP` at the node whose output you want to feed back, wire \
         this node's output into that chain, and put a Level TOP in between to \
         decay it. Without the decay the loop saturates within a second.",
    ),
    (
        "selectTOP",
        "Reads another TOP's output from the **current** frame, so the source \
         cooks first. Use it to fan a texture out to several branches without \
         drawing wires across the whole network. For a loop, use Feedback instead: \
         a Select pointing back at its own chain is a cycle and is refused.",
    ),
    (
        "glslTOP",
        "Write a fragment body; the boilerplate is supplied. In WGSL, `in.uv`, \
         `U.time.x`, `U.res` and `sample0(uv)`/`sample1(uv)` are in scope, along \
         with `U.p0..U.p3` from the four Uniform parameters. In GLSL a Shadertoy \
         `mainImage` with `iTime` and `iResolution` runs unmodified.\n\n\
         Sources are validated before the GPU sees them, so a typo gives a line \
         number and the last shader that compiled keeps running — the patch never \
         goes black mid-edit. *Import ISF…* loads a published ISF shader and turns \
         its inputs into parameters on this node.",
    ),
    (
        "animationCHOP",
        "The keys are text — `channel time value interpolation`, \
         one per line — so they diff cleanly and twenty evenly spaced keys are \
         faster typed than dragged. The parameter panel draws the same data as an \
         editable curve.\n\n\
         Interpolation is `constant` (hold, then jump), `linear`, `smooth` (ease \
         in and out) or `spline` (Catmull-Rom, continuous through the keys). \
         Outside the keyed range the curve holds its end values rather than \
         extrapolating.",
    ),
    (
        "moviefileinTOP",
        "Still images — PNG, JPEG, WebP, BMP, TGA, TIFF — are decoded \
         in-process, with no external tool involved. Everything that moves \
         goes through an `ffmpeg` subprocess, so mp4, mov, mkv, webm, avi and \
         animated GIF all play if ffmpeg is installed. It is looked for on the \
         PATH and in the usual install directories, so a double-clicked app \
         finds it too, and the node says so plainly when it cannot.\n\n\
         Playback is a function of the timeline, not a private play head: the \
         frame shown at time `t` is always the file at `t × speed`. Scrubbing \
         the timeline scrubs the movie, the loop range loops it, and a \
         headless `otd render` writes exactly the frames the editor showed. \
         Seeking backwards restarts the decode at the new time, which is why \
         a scrub is a real scrub rather than a rewind.\n\n\
         The picture's own size wins: `Fallback W`/`H` is only what to show \
         before the first frame arrives, or if the file cannot be read.\n\n\
         `Speed` tracks the timeline exactly at 1× and 2×. Past that it falls \
         behind, and the reason is the transport rather than the decoder: \
         frames cross a pipe as raw RGBA, so 1138×640 is 2.9 MB each, and \
         asking for 8× is asking for well over a gigabyte a second through \
         it. Scrubbing is unaffected — a jump seeks rather than races.",
    ),
    (
        "videodeviceinTOP",
        "A camera, through ffmpeg. `Requested W`/`H` and the frame rate are \
         requests rather than commands — a capture device only does the modes \
         it does — so they are negotiated to the nearest mode the device \
         actually reports, and the node shows what the device said if none \
         will work.\n\n\
         On macOS the first use raises the system camera-permission prompt, \
         and nothing arrives until it is granted. The picture is always the \
         newest frame decoded, so latency stays at about one frame; pausing \
         the timeline pauses this like everything else in the network.",
    ),
    (
        "audiofileinCHOP",
        "Plays RIFF/WAVE — PCM in 8, 16, 24 or 32 bits, or 32-bit float — \
         which is what a DAW bounces, decoded in-process with no external \
         tool. Anything else — m4a, mp3, ogg, flac, or the soundtrack of a \
         movie file — is decoded by `ffmpeg`, the same one Movie File In \
         uses, so it plays if ffmpeg is installed and the node says so \
         plainly if it is not.\n\n\
         Playback is a function of the timeline, not a private play head: the \
         sample at time `t` is always the file at `t × speed`. Scrubbing the \
         timeline scrubs the audio, a loop range loops it, and a headless \
         render reads exactly the samples the editor played.",
    ),
    (
        "midioutCHOP",
        "Channel names mirror MIDI In: `n60` is note 60, `cc74` is control \
         74; anything else is ignored. A note fires when its value crosses \
         zero, with the value at that moment as velocity — while it is held, \
         changes of value are not new notes.\n\n\
         Unlike DMX, MIDI is an event protocol, so only *changes* go on the \
         wire; an unchanged control costs nothing per frame.",
    ),
    (
        "dmxoutCHOP",
        "Sends each channel as a DMX slot over Art-Net or sACN (E1.31). Values are \
         expected in 0..1 and are clamped rather than wrapped — a light jumping \
         from full to black on an overshoot is much worse than one that saturates.\n\n\
         DMX is a state protocol, so each frame sends the current value of each \
         channel, which is the last sample of the slice.",
    ),
    (
        "replicatorCOMP",
        "Watches a template DAT and keeps one clone of its master component \
         per data row. The first column names each replicant; any other \
         column whose header matches a custom parameter on the master sets \
         that parameter on the replicant — so the table *is* the population, \
         and adding a row is adding an instance.\n\n\
         Replicants are ordinary clones: they follow the master's network as \
         it is edited, and anything you place inside the replicator by hand \
         is yours and is left alone.",
    ),
    (
        "renderTOP",
        "Draws the 3D scene into an ordinary texture, so everything after it is a \
         normal TOP chain that neither knows nor cares a camera was involved.\n\n\
         It finds its Geometry, Camera and Light COMPs by parameter rather than by \
         wire, and those references are real cook dependencies: they cook first, \
         and their animation propagates here.",
    ),
    (
        "cacheTOP",
        "Freezes its input. Turn `Active` off and the texture stops updating, \
         which is how you hold a frame — or stop paying for a branch you are not \
         currently looking at.",
    ),
];

/// One operator's page.
pub fn operator_page(def: &otd_core::OpDef) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "### {} — `{}`\n", def.label, def.type_name);
    let _ = writeln!(out, "{}\n", def.summary);

    if let Some((_, note)) = NOTES.iter().find(|(name, _)| *name == def.type_name) {
        let _ = writeln!(out, "{note}\n");
    }

    if def.inputs.is_empty() {
        let _ = writeln!(out, "*No inputs — this is a generator.*\n");
    } else {
        let _ = writeln!(out, "**Inputs:** {}\n", def.inputs.join(", "));
    }
    if def.time_dependent {
        let _ = writeln!(
            out,
            "*Time dependent: cooks every frame, and everything downstream of it does too.*\n"
        );
    }

    let params = (def.params)();
    if !params.is_empty() {
        let _ = writeln!(out, "| Parameter | Name | Default | Range |");
        let _ = writeln!(out, "|---|---|---|---|");
        for (key, p) in &params {
            let _ = writeln!(
                out,
                "| {} | `{key}` | {} | {} |",
                if p.label.is_empty() { key } else { &p.label },
                describe_default(&p.value),
                describe_range(p),
            );
        }
        let _ = writeln!(out);
    }
    out
}

fn describe_default(v: &Value) -> String {
    match v {
        Value::Str(s) if s.is_empty() => "*(empty)*".into(),
        Value::Str(s) if s.contains('\n') => "*(multi-line)*".into(),
        Value::Str(s) => format!("`{s}`"),
        Value::Float(f) => format!("`{f}`"),
        Value::Int(i) => format!("`{i}`"),
        Value::Bool(b) => format!("`{b}`"),
        Value::Vec2(v) => format!("`{v:?}`"),
        Value::Vec3(v) => format!("`{v:?}`"),
        Value::Vec4(v) => format!("`{v:?}`"),
    }
}

fn describe_range(p: &Param) -> String {
    if let Some(menu) = &p.menu {
        return menu
            .iter()
            .map(|m| format!("`{m}`"))
            .collect::<Vec<_>>()
            .join(" · ");
    }
    match p.range {
        Some((lo, hi)) => format!("{lo} … {hi}"),
        None => String::new(),
    }
}

/// The whole reference: an index and a page per operator, grouped by family.
pub fn reference(registry: &OpRegistry) -> String {
    let mut by_family: BTreeMap<&'static str, Vec<&otd_core::OpDef>> = BTreeMap::new();
    for def in registry.iter() {
        by_family.entry(def.family.suffix()).or_default().push(def);
    }
    for defs in by_family.values_mut() {
        defs.sort_by_key(|d| d.type_name);
    }

    let mut out = String::from(
        "# Operator reference\n\n\
         Generated from the operator registry — the same table the editor builds \
         its menus and parameter pages from, so this cannot drift from what the \
         operators actually do.\n\n",
    );

    let total: usize = by_family.values().map(|v| v.len()).sum();
    let _ = writeln!(out, "{total} operators.\n");

    // Index first: the thing somebody scanning for "is there an X" reads.
    for (family, defs) in &by_family {
        let _ = writeln!(out, "**{family}** ({})\n", defs.len());
        for def in defs {
            let _ = writeln!(
                out,
                "- [{}](#{}) — {}",
                def.type_name,
                anchor(def),
                def.summary
            );
        }
        let _ = writeln!(out);
    }

    for (family, defs) in &by_family {
        let _ = writeln!(out, "---\n\n## {family}\n");
        for def in defs {
            out.push_str(&operator_page(def));
        }
    }
    out
}

/// The GitHub-flavoured anchor for an operator's heading.
fn anchor(def: &otd_core::OpDef) -> String {
    // `### Feedback — `feedbackTOP`` becomes `feedback--feedbacktop`.
    format!(
        "{}--{}",
        def.label.to_lowercase().replace(' ', "-"),
        def.type_name.to_lowercase()
    )
}

/// The hand-written note for an operator, if it has one.
///
/// The editor shows this too, so the reference and the parameter panel are
/// reading one source rather than two that can disagree.
pub fn note(type_name: &str) -> Option<&'static str> {
    NOTES
        .iter()
        .find(|(name, _)| *name == type_name)
        .map(|(_, note)| *note)
}

/// Operators or parameters with nothing written about them.
///
/// Returned rather than printed so a test can be the enforcement, which is
/// the point: "enforced in PR review" holds until the week somebody is busy.
pub fn undocumented(registry: &OpRegistry) -> Vec<String> {
    let mut missing = Vec::new();
    for def in registry.iter() {
        if def.summary.trim().is_empty() {
            missing.push(format!("{}: no summary", def.type_name));
        } else if !def.summary.trim().ends_with('.') {
            // A summary is a sentence. Without this they drift into labels.
            missing.push(format!(
                "{}: summary should be a sentence ending in a full stop",
                def.type_name
            ));
        }
        if def.label.trim().is_empty() {
            missing.push(format!("{}: no label", def.type_name));
        }
        for (key, p) in (def.params)() {
            if p.label.trim().is_empty() {
                missing.push(format!("{}.{key}: no label", def.type_name));
            }
        }
    }
    missing
}

/// Family descriptions for the top of the reference.
pub fn family_summary(family: Family) -> &'static str {
    match family {
        Family::Top => "Texture operators — everything on the GPU.",
        Family::Chop => "Channel operators — control signals and audio, sampled over time.",
        Family::Sop => "Surface operators — geometry.",
        Family::Dat => "Data operators — text and tables.",
        Family::Mat => "Materials.",
        Family::Comp => "Components — containers, and the 3D scene objects.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operator_and_parameter_is_documented() {
        // The enforcement PLAN.md wanted from review. An operator arriving
        // without prose fails the build rather than being noticed, or not.
        let missing = undocumented(&crate::registry());
        assert!(
            missing.is_empty(),
            "{} undocumented:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }

    #[test]
    fn the_reference_covers_every_registered_operator() {
        let reg = crate::registry();
        let text = reference(&reg);
        for def in reg.iter() {
            assert!(
                text.contains(def.type_name),
                "{} is missing from the reference",
                def.type_name
            );
        }
        // And it is a document, not a dump: an index, headings, parameter
        // tables.
        assert!(text.contains("# Operator reference"));
        assert!(text.contains("| Parameter | Name | Default | Range |"));
    }

    #[test]
    fn a_page_reports_what_the_editor_would_show() {
        let reg = crate::registry();
        let def = reg.get("levelTOP").unwrap();
        let page = operator_page(def);
        assert!(page.contains("`levelTOP`"));
        assert!(page.contains("Brightness"), "{page}");
        assert!(page.contains("`brightness`"), "{page}");
        // Defaults and ranges come from the operator itself, so they cannot
        // disagree with the parameter panel.
        assert!(page.contains("0 … 4"), "{page}");
    }

    #[test]
    fn a_menu_parameter_lists_its_choices() {
        let reg = crate::registry();
        let page = operator_page(reg.get("rampTOP").unwrap());
        assert!(page.contains("`horizontal`"), "{page}");
        assert!(page.contains("`radial`"), "{page}");
    }

    #[test]
    fn index_links_point_at_headings_that_exist() {
        let reg = crate::registry();
        let text = reference(&reg);
        // A reference whose links go nowhere is worse than one without links.
        for def in reg.iter() {
            let link = format!("](#{})", anchor(def));
            assert!(text.contains(&link), "no index link for {}", def.type_name);
            let heading = format!("### {} — `{}`", def.label, def.type_name);
            assert!(text.contains(&heading), "no heading for {}", def.type_name);
        }
    }

    #[test]
    fn time_dependent_operators_say_so() {
        let reg = crate::registry();
        // Whether an operator cooks every frame is the single fact that most
        // often explains "why is my patch slow", so the page has to state it.
        assert!(operator_page(reg.get("lfoCHOP").unwrap()).contains("Time dependent"));
        assert!(!operator_page(reg.get("levelTOP").unwrap()).contains("Time dependent"));
    }
}
