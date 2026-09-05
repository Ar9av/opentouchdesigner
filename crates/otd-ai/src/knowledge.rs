//! Curated technique choices, translated to this engine rather than copied TD patches.
//! No live retrieval: these small cards and the executable recipes ship offline.
use otd_core::OpRegistry;

pub struct Technique {
    pub name: &'static str,
    pub keywords: &'static [&'static str],
    pub recipes: &'static [&'static str],
    pub operators: &'static [&'static str],
    pub guidance: &'static str,
    pub source: &'static str,
}

pub const TECHNIQUES: &[Technique] = &[
    Technique {
        name: "Camera motion painting",
        keywords: &["movement", "motion", "painting", "lightpainting", "motionpaint"],
        recipes: &["motionpaint"],
        operators: &["feedbackTOP", "compositeTOP", "lookupTOP"],
        guidance: "Difference the live source against a feedbackTOP targeting that SOURCE, not the difference output. Gain and colour the difference, then accumulate it in a separate decaying feedback loop. This detects changed pixels, not a person or pose. Camera motion and lighting changes also trigger it. Keep colour grading outside the history target.",
        source: "crates/otd-ai/recipes/motionpaint.json",
    },
    Technique {
        name: "Camera false colour",
        keywords: &["thermal", "infrared", "heatmap"],
        recipes: &["thermal"],
        operators: &["glslTOP"],
        guidance: "Map source luminance to a dark violet, pink and golden palette. Always sample the source image. Expose exposure and effect mix. This mimics thermal colour only; an ordinary webcam cannot measure temperature.",
        source: "crates/otd-ai/recipes/thermal.json",
    },
    Technique {
        name: "Camera hologram",
        keywords: &["hologram", "holographic", "scanlines"],
        recipes: &["hologram"],
        operators: &["glslTOP", "feedbackTOP", "compositeTOP"],
        guidance: "Combine source-preserving cyan grading and scanlines with restrained frame-history blending. Keep facial and scene structure legible. Do not claim background removal, depth or body tracking without an actual mask or tracking input.",
        source: "crates/otd-ai/recipes/hologram.json",
    },
    Technique {
        name: "Temporal trails",
        keywords: &[
            "trail", "trails", "echo", "ghost", "feedback", "tunnel", "smear", "smeary",
        ],
        recipes: &["trails", "tunnel"],
        operators: &["feedbackTOP", "transformTOP", "compositeTOP"],
        guidance: "Use image feedback for temporal persistence, tunnels and echoes. Contrast the source, transform the previous frame, then mix it with fresh input. Feedback reads its target without an input wire. Decay and source injection must keep dark detail visible over many frames. This is image history, not a physical particle simulation.",
        source: "https://forum.derivative.ca/t/particles-with-feedback/119748",
    },
    Technique {
        name: "Organic flow",
        keywords: &[
            "organic",
            "fluid",
            "liquid",
            "ink",
            "smoke",
            "wisps",
            "advection",
            "flow",
        ],
        recipes: &["smoke", "plasma"],
        operators: &["flowTOP", "feedbackTOP", "compositeTOP"],
        guidance: "For drifting ink use flow inside a feedback loop with sparse fresh input. One flow operator only warps one image. For a procedural organic surface use animated domain warping instead. These are visual approximations; do not promise fluid conservation or a solver. Keep palette grading outside the recurrence.",
        source: "docs/GUIDE.md",
    },
    Technique {
        name: "Instanced geometry",
        keywords: &[
            "3d",
            "instances",
            "instancing",
            "particles",
            "swarm",
            "flock",
            "spheres",
            "dots",
            "field",
        ],
        recipes: &["field", "torus"],
        operators: &["geometryCOMP", "renderTOP", "patternCHOP", "mergeCHOP"],
        guidance: "For many repeated objects use one SOP and geometryCOMP instancing. Merge equal-length tx, ty, tz channels: each sample is one instance. Supply camera, light, material and render references. Animate positions or shape; a repeated field is not a flock simulation. Do not translate POP tutorials into nonexistent POP operators.",
        source: "https://derivative.ca/community-post/tutorial/dancing-dots/63777",
    },
    Technique {
        name: "Audio response",
        keywords: &[
            "audio",
            "music",
            "microphone",
            "mic",
            "bass",
            "sound",
            "reactive",
        ],
        recipes: &["audio"],
        operators: &[
            "audiodeviceinCHOP",
            "audiospectrumCHOP",
            "lagCHOP",
            "mathCHOP",
        ],
        guidance: "Use actual audio -> spectrum -> band selection -> lag -> bounded math -> exports. Fast attack and slower release make an envelope readable. Map the envelope to a useful baseline and safe range, not directly to zero scale. A clock is not audio analysis; silent input should leave a composed resting image.",
        source: "docs/GUIDE.md",
    },
    Technique {
        name: "Tempo choreography",
        keywords: &["beat", "bpm", "tempo", "rhythm", "pulse", "metronome"],
        recipes: &["beat"],
        operators: &["beatCHOP", "triggerCHOP"],
        guidance: "Use beatCHOP and a trigger envelope for deliberate BPM choreography without a microphone. This is a tempo clock, not beat detection from music. Layer slow continuous movement with a restrained rhythmic accent.",
        source: "crates/otd-ai/recipes/beat.json",
    },
    Technique {
        name: "Surface and footage styling",
        keywords: &[
            "cells",
            "cellular",
            "voronoi",
            "cracks",
            "glass",
            "retro",
            "dither",
            "comic",
            "toon",
            "kaleidoscope",
            "glitch",
        ],
        recipes: &["cells", "retro", "toon", "kaleidoscope", "glitch"],
        operators: &["voronoiTOP", "ditherTOP", "toonTOP"],
        guidance: "Choose the specific native operator for the requested surface or footage treatment. Preserve the existing input when styling footage. Voronoi distance is useful as a displacement field; edges give cracks. Expose scale, strength and palette as controls instead of hiding them in shader constants.",
        source: "docs/GUIDE.md",
    },
];

pub fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn matches(t: &Technique, prompt: &str) -> bool {
    let words = words(prompt);
    t.keywords.iter().any(|k| words.iter().any(|w| w == k))
}

pub fn context_for(prompt: &str, registry: &OpRegistry) -> String {
    let mut out = String::from(
        "\n\nTECHNIQUE SELECTION\nChoose a construction for the requested visual behavior, not just a matching noun. Combine a source, motion, palette and finish deliberately. Keep one focal structure, negative space, and a few useful controls. For refinements preserve the user's composition and retune it. Explain the technique and two concrete controls in notes. Only the current catalogue defines supported operators and parameters. References are provenance, not fetched content; never claim to have browsed or rendered the result.\n",
    );
    for t in TECHNIQUES.iter().filter(|t| matches(t, prompt)).take(3) {
        if t.operators.iter().all(|op| registry.get(op).is_some()) {
            out.push_str(&format!(
                "\n{}: {}\nReference: {}\n",
                t.name, t.guidance, t.source
            ));
        }
    }
    out
}
