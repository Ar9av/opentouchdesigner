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
        name: "Motion-tracking HUD",
        keywords: &["hud", "crosshair", "surveillance", "reticle", "viral", "targeting", "boxes"],
        recipes: &["blobhud"],
        operators: &["feedbackTOP", "compositeTOP", "blurTOP", "levelTOP", "blobtrackCHOP", "glslTOP"],
        guidance: "Track MOVEMENT, not brightness: in a bright room the largest bright region is the wall. Downsample to 160x90, difference against a feedbackTOP of that same node, blur so one mover's scattered edges merge, gain with levelTOP, then blobtrackCHOP. Export x/y/width/height straight into a glslTOP that draws the box over a hard black-and-white feed; exports fill all four components with one value, and only uniform1-4 exist. Do not lag the exports: x/y drop to 0 when nothing moves, so a lagged box flies into the corner. Hide the overlay when width*height is 0 or above 0.5. One region only; not person recognition.",
        source: "crates/otd-ai/recipes/blobhud.json",
    },
    Technique {
        name: "ASCII camera",
        keywords: &["ascii", "characters", "letters", "terminal", "matrix", "textmode", "glyphs"],
        recipes: &["ascii"],
        operators: &["glslTOP"],
        guidance: "One glslTOP: quantize to cells, sample the camera at each cell centre, choose a 5x5 bitmap glyph packed in an int by brightness, and draw it with bit tests. Cell size and colour blend belong in uniform1 so they can be turned. There is no font atlas TOP; do not try to build this from textTOPs.",
        source: "crates/otd-ai/recipes/ascii.json",
    },
    Technique {
        name: "Halftone print",
        keywords: &["halftone", "newspaper", "screenprint", "risograph", "print"],
        recipes: &["halftone"],
        operators: &["glslTOP"],
        guidance: "Rotate fragment coordinates by the screen angle, find the cell centre, sample the camera there, and draw an ink dot whose radius is sqrt(1 - luminance) times the cell. Expose spacing and angle. For dithering or 1-bit looks use ditherTOP instead.",
        source: "crates/otd-ai/recipes/halftone.json",
    },
    Technique {
        name: "Camera point cloud",
        keywords: &["pointcloud", "depth", "voxels", "scan", "scanned"],
        recipes: &["pointcloud"],
        operators: &["resolutionTOP", "toptochopCHOP", "geometryCOMP", "renderTOP", "constantMAT"],
        guidance: "resolutionTOP to about 96x54 (at most 128x128), toptochopCHOP layout image gives one sample per pixel with r g b a u v; instance a tiny boxSOP with tx=u ty=v tz=g and cr/cg/cb=r/g/b. geometryCOMP.scale multiplies the dots as well as the spacing, so keep the SOP around 0.01 and scale the COMP. Brightness is not real depth; there is no depth estimation here.",
        source: "crates/otd-ai/recipes/pointcloud.json",
    },
    Technique {
        name: "Particle emitter",
        keywords: &["particles", "particle", "sparks", "fountain", "emitter", "embers", "confetti"],
        recipes: &[],
        operators: &["particleCHOP", "geometryCOMP", "renderTOP", "pointspriteMAT"],
        guidance: "particleCHOP is a stateful CPU emitter: one sample per particle with tx/ty/tz/size/life. Point geometryCOMP.instancechop at it with sx/sy/sz set to size, and use pointspriteMAT or a small sphereSOP. Emitter x/y are 0..1 with y down; tx/ty span 6 scene units, so frame the camera on that. Lifetime and count trade density for trail length; gravity may be negative for rising embers. Max 4096 particles, no collisions or flocking; never invent POP operators.",
        source: "docs/OPERATORS.md",
    },
    Technique {
        name: "Springy motion",
        keywords: &["spring", "springy", "bouncy", "bounce", "elastic", "wobble", "jiggle", "overshoot"],
        recipes: &[],
        operators: &["springCHOP", "lfoCHOP", "triggerCHOP"],
        guidance: "Put springCHOP between a stepping control (trigger, beat, keyboard, a square lfoCHOP) and the exported parameter. It overshoots and settles; damping near 2*sqrt(spring) is critically damped, lower wobbles. Unlike lagCHOP it keeps moving after the input stops.",
        source: "docs/OPERATORS.md",
    },
    Technique {
        name: "Waveform history",
        keywords: &["waveform", "oscilloscope", "scope", "history", "graph", "plot"],
        recipes: &[],
        operators: &["trailCHOP", "geometryCOMP", "choptotopTOP"],
        guidance: "trailCHOP turns a live channel into its last N seconds, oldest first, as one buffer. Draw it by instancing: rename the trail to ty, merge with a patternCHOP ramp as tx of the same length, and instance small spheres or point sprites. Or choptotopTOP it into a 1-row texture for a shader. Window x sample rate is the sample count.",
        source: "docs/OPERATORS.md",
    },
    Technique {
        name: "Deformed geometry",
        keywords: &["twist", "twisted", "bend", "bent", "taper", "faceted", "lowpoly", "sculpt"],
        recipes: &[],
        operators: &["twistSOP", "facetSOP", "noiseSOP", "renderTOP"],
        guidance: "Give the source enough rows along the deform axis (tube rows, grid rows) or twistSOP only moves a few points. Deformers leave normals stale, so end SOP chains with facetSOP: smooth for organic lighting, unique for a faceted low-poly look. Animate twist strength from an lfoCHOP export for motion.",
        source: "docs/OPERATORS.md",
    },
    Technique {
        name: "Bright-region tracking",
        keywords: &["blob", "blobfollow", "track", "tracking", "follow"],
        recipes: &["blobfollow"],
        operators: &["blobtrackCHOP", "resolutionTOP", "glslTOP"],
        guidance: "Downsample before blobtrackCHOP to keep CPU readback small. The largest connected bright region yields present/count/x/y/area/width/height/vx/vy. Coordinates are top-left normalized 0..1; convert y to 1-y for Shadertoy overlays. Gate overlays with present, and expect jumps if another region becomes largest. This does not identify a person. For movement-only tracking, difference the camera against its previous frame before tracking.",
        source: "crates/otd-ai/recipes/blobfollow.json",
    },
    Technique {
        name: "Motion-driven feedback",
        keywords: &["optical", "datamosh", "datamoshing", "directional"],
        recipes: &["datamosh"],
        operators: &["opticalflowTOP", "feedbackTOP", "displaceTOP"],
        guidance: "Downsample camera to 160x90 for local optical flow. Input 0 is current, input 1 is feedback targeting that same downsampled source. RG encodes 0.5 + gain * UV motion per frame. Use a SECOND feedback targeting the final mix for the picture history. Displace that history with amount=-1 and offset=-0.5 to move with the flow, then blend with fresh footage. Estimates small translations, not body identity or large motion.",
        source: "crates/otd-ai/recipes/datamosh.json",
    },
    Technique {
        name: "Chroma-key portal",
        keywords: &["greenscreen", "keying", "portal", "backdrop"],
        recipes: &["greenscreen"],
        operators: &["chromakeyTOP", "compositeTOP"],
        guidance: "Use a keyed alpha mask to place the camera over a new background. Requires a matching solid backdrop; never promise automatic person segmentation. Composite background into input 0 and keyed camera into input 1, operation over. Tune tolerance, softness and despill.",
        source: "crates/otd-ai/recipes/greenscreen.json",
    },
    Technique {
        name: "Night vision",
        keywords: &["night", "nightvision", "grain", "vignette"],
        recipes: &["nightvision"],
        operators: &["glslTOP"],
        guidance: "Sample the camera and map luminance to green, with restrained grain, scanlines and vignette. Expose exposure and grain amount. This is a grade, not improved camera sensitivity.",
        source: "crates/otd-ai/recipes/nightvision.json",
    },
    Technique {
        name: "Image relief",
        keywords: &["relief", "emboss", "embossed", "metallic"],
        recipes: &["relief"],
        operators: &["embossTOP"],
        guidance: "Blur fine noise before directional emboss. Adjust direction, width and strength for apparent lighting. This derives image gradients, not scene depth.",
        source: "crates/otd-ai/recipes/relief.json",
    },
    Technique {
        name: "Pixel mosaic",
        keywords: &["mosaic", "pixelated", "pixelation", "grout"],
        recipes: &["mosaic"],
        operators: &["glslTOP"],
        guidance: "Quantize sample coordinates to tile centres, then quantize source colours and add thin grout. Keep tile aspect square at the output resolution. Expose tile count, grout and colour levels.",
        source: "crates/otd-ai/recipes/mosaic.json",
    },
    Technique {
        name: "Watercolour treatment",
        keywords: &["watercolour", "watercolor", "painterly", "washes"],
        recipes: &["watercolour"],
        operators: &["blurTOP", "toonTOP"],
        guidance: "Diffuse source detail with blur, use many luminance bands and restrained dark ink for a soft painterly treatment. Expose blur size and ink strength. Do not call this a pigment simulation.",
        source: "crates/otd-ai/recipes/watercolour.json",
    },
    Technique {
        name: "Interactive camera freeze",
        keywords: &["freeze", "hold", "frozen"],
        recipes: &["freeze"],
        operators: &["keyboardinCHOP", "mathCHOP", "cacheTOP"],
        guidance: "Cache active=false holds its most recent frame. Invert a held keyboard channel to active so pressing freezes and releasing resumes. A held frame should be static; do not animate it to satisfy a generic movement check.",
        source: "crates/otd-ai/recipes/freeze.json",
    },
    Technique {
        name: "Audio-reactive footage",
        keywords: &["audiocamera", "microphone", "music", "audio"],
        recipes: &["audiocamera"],
        operators: &["audiodeviceinCHOP", "analyzeCHOP", "lagCHOP", "levelTOP"],
        guidance: "For footage, RMS -> lag -> bounded gain/offset -> exports makes the existing camera react to sound. Preserve visible brightness at zero audio. Microphone input uses chan1; select and rename the channel before exporting. Never replace the camera with a procedural generator.",
        source: "crates/otd-ai/recipes/audiocamera.json",
    },
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
