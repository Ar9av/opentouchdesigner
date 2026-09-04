# TODO

Known gaps, with the evidence for each. Everything here has been measured
rather than guessed — where there is a number, the command that produced it is
next to it, so the number can be rechecked rather than believed.

Ordered by value per unit of work within each section.

## ISF import

Currently **225 of 327** published shaders import and compile.

```bash
git clone --depth 1 https://github.com/Vidvox/ISF-Files
cargo run -p otd-gpu --example isf_corpus -- ISF-Files
```

- [x] **More uniform space than four vec4s** — done, 213 → 225. The block is
      twelve `vec4`s wide (`ops::PARAM_VECS`), which is where the widest
      published shader fits: `Multi Gradient` and `RE RGB Gradient Generator`
      want 48 components each. `PackedParams` stayed four vectors, so no
      built-in `pack_*` changed shape; `TopSpec::pack_params` widens them with
      zeros and the GLSL TOP packs the full block instead. Twelve of the
      sixteen now import and compile. The other four went on to fail for
      reasons of their own — three want an audio texture (below) and `Worley
      Cells` fails inside `nearestNodeIndex`, which is a translation bug and
      not about space.

- [ ] **Audio-texture inputs** — 5 shaders. ISF's `audio` and `audioFFT` input
      types are textures a host fills with the live waveform or its spectrum.
      We map the unknown type to a float, so `IMG_NORM_PIXEL` on it becomes
      `texture()` on a scalar and the shader fails to compile. The shader side
      is small — treat them as image inputs — but they are only worth anything
      once a CHOP can be the texture, so it is really "wire audio to a GLSL
      TOP input", not an importer fix.

- [ ] **Multi-pass** — 56 shaders, and the largest single bucket by some way.
      Needs several render targets per node and a pass index the shader can
      read. Also unlocks most of the *persistent buffer* shaders below, since
      they are multi-pass with `PERSISTENT: true`. A real feature; worth
      scoping on its own rather than squeezing in.

- [ ] **Persistent buffers** — ~8 shaders (`feedbackBuffer`, `lastFrame`,
      `bufferVariableNameA`). A pass whose target survives to the next frame.
      Mostly falls out of multi-pass.

- [ ] **Varyings from a companion vertex shader** — 17 shaders. ISF lets a
      filter ship a `.vs` that computes per-vertex values — usually
      neighbouring texel coordinates for a convolution. We run our own
      fullscreen vertex stage and ignore theirs, so those `in` declarations
      are unbound and the shader fails validation. Either run the supplied
      vertex stage, or recognise the handful of standard names and compute
      them in the fragment stage.

- [ ] **A third image input** — 2 shaders want `otd_image2`. The GLSL TOP has
      two texture inputs. Low value on its own; do it if the bind group is
      being touched anyway.

## The assistant

Measured end to end — plans are cooked on a real GPU against a real clip and
the pixels are checked for black, blown out, still, or identical to the source.

```bash
cargo run -p otd-ai --example vfx_eval -- --repeat 2
cargo run -p otd-ai --example scenarios
```

- [ ] **Feedback loops are the weak spot** — about half succeed, against ~95%
      for everything else. Three specific causes have been fixed (closing the
      loop with `over`, saturating an `add` loop with full-brightness video,
      and crushing the source with a `blacklevel` above where it enters), and
      what is left is variance on a fiddly construction rather than one bug.
      Likely next step: a worked, known-good loop in the brief with real
      numbers, rather than a description of one.

- [ ] **A second output null** — the model sometimes parks its work on `out2`
      while `out1` still shows the raw clip, so a correct answer shows the
      user no change. The brief now says to insert before an existing null and
      it mostly complies; it is not reliable. Consider making `apply` rewire
      it, or warning the way the starved-loop check does.

## Camera and packaging

- [ ] **Ad-hoc signing means the camera permission is asked for again after
      every rebuild.** macOS keys the grant to the code identity, and an
      ad-hoc signature changes hash on every build (`Failed to match existing
      code requirement`). A Developer ID would fix it and costs an Apple
      account. Not a bug — worth writing down so it is not re-debugged.

- [ ] **Depth *estimation* is not implemented.** `renderTOP` gained a depth
      output, which is real geometric distance from the camera, but that only
      works for 3D scenes we rendered. Inferring depth from ordinary camera
      footage is a neural net (MiDaS, Depth Anything) and needs an inference
      runtime — ONNX or candle — which is a dependency decision, not a patch.

## Editor

- [ ] **Double-clicking a `.otd` in Finder does not open it.** The bundle
      declares `CFBundleDocumentTypes` for `.otd`/`.otdc`, so it looks like it
      should. `open -a OpenTouchDesigner file.otd` sends an Apple Event the
      app never handles; only `--args` reaches the command-line path added in
      `main.rs`. Needs the open-document event wired up.

- [ ] **No marquee selection.** `Cmd+A` and shift-click exist; dragging a box
      round several nodes does not.

## Operators that do not exist yet

From surveying what people actually build with this kind of tool. Each is a
`.wgsl` file and a table entry, so the cost is low — the question is whether
the look is wanted.

- [ ] **Mesh morphing beyond point interpolation** — `blendSOP` interpolates
      positions with an invented correspondence. Real morphing between
      different topologies wants resampling, and nothing here does that.
- [ ] **Particle simulation** — instancing draws thousands of copies from a
      CHOP, but nothing integrates position over time. Attractors, flocking
      and the rest need state that survives a frame, which is the same
      problem as ISF persistent buffers.
