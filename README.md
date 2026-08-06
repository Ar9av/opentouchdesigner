# OpenTouchDesigner

An open-source, cross-platform, node-based realtime visual programming
environment in the spirit of TouchDesigner. Written in Rust on wgpu, so
`cargo run` works on macOS, Windows and Linux.

**Status: Phases 0 and 1 complete apart from video I/O.** See
[PLAN.md](PLAN.md) for the research and the full roadmap. What exists today is
a working graph, cook engine, GPU texture pipeline with live shader
compilation, text project format, node editor and projector output — not yet a
tool you would take to a show.

## Try it

```bash
cargo run -p otd-app
```

It opens on the starter patch: `noise1 → level1 → null1`, animated, running
live. Every node shows its own output at frame rate. **File → Examples →
feedback** loads the Phase 1 demo: a Shadertoy shader driving a feedback loop
at 1920×1080.

Render a frame with no window at all — something TouchDesigner cannot do on a
Linux server:

```bash
cargo run -p otd-gpu --example render_png -- frame.png examples/feedback.otd 300
```

### Keys

| | |
|---|---|
| `Tab` | add an operator (auto-wires from the selection) |
| double-click | set a node as the output viewer |
| `B` | bypass the selected node |
| `Space` | play / pause time |
| `H` | reset the view |
| `Delete` | delete the selection (reconnecting the chain around it) |
| drag background | pan · scroll to zoom |
| drag output port → input port | wire · click an input port to unwire |

The **Output** button in the top bar opens a second window showing only the
viewer, letterboxed — drag it to a projector and toggle Fullscreen.

## What works

**Cook engine** (`otd-core`) — demand-driven, pull-based, memoized. A node
cooks only if it changed, an input produced new output, or it is *time
dependent*. Time dependence propagates downstream from animated parameters and
from intrinsically animated operators, so a static branch cooks once and then
costs nothing. Cycles are rejected at connect time; Feedback breaks them by
reading last frame instead. Each node reports its own cook time.

Operators that name a source by parameter rather than by wire — a Select TOP
pointing at `/blur1` — declare it as a dependency, so it cooks first and
propagates dirtiness and animation exactly like a wire. Feedback deliberately
does not, which is precisely what lets a feedback loop exist without a cycle
in the cook graph.

**Parameters** — the four-mode system: Constant, Expression, Export, Bind.
Constant and Expression are live; Export and Bind are visible but inert until
CHOPs land in Phase 2. Expression mode runs a self-contained numeric evaluator
(`absTime`, `frame`, `sin`, `fit`, `clamp`, …) that is a strict subset of the
Python expressions Phase 3 will run, so projects written today keep loading.
Switching a parameter to Expression and back does not lose the constant
underneath.

**TOPs** (`otd-gpu`) — Constant, Noise, Ramp, GLSL, Level, Transform, Blur,
Displace, Composite, Switch, Resolution, Cache, Select, Null, Out, Feedback.
All 16-bit float, no resolution cap. One command encoder per frame; transient
textures are pooled and node outputs are retained for as long as their cache
is valid.

**GLSL TOP** — your own shader, compiled while you type. WGSL is the primary
language: write a fragment body and `in.uv`, `U.time.x`, `U.res` and
`sample0/1(uv)` are already in scope. GLSL is also accepted, wrapped so a
Shadertoy `mainImage` with `iTime` and `iResolution` runs unmodified. Sources
are validated through naga *before* wgpu sees them, so a typo gives you a
message with a line number, the node outlines red, and the last shader that
compiled keeps running — a patch never goes black mid-edit.

**Project format** — text, path-sorted, defaults omitted. Adding a node
appends one block; rewiring changes one line. Multi-line shader sources
survive the round trip unchanged. See
[`examples/feedback.otd`](examples/feedback.otd).

**Editor** (`otd-app`) — node canvas with live per-node viewers, wiring,
parameter panel with a shader code editor, output viewer, a second output
window for a projector, cook/GPU statistics in the top bar. The thumbnails are
the operators' real textures shared with egui — no copy, no readback.

## What doesn't exist yet

**Video and audio I/O** — Movie File In/Out and Video Device In are the one
part of Phase 1 not built. They need GStreamer, which is a system dependency
rather than a crate, and none of it can be verified without it installed.

Everything else is Phases 2–6 of [PLAN.md](PLAN.md): CHOPs, SOPs, DATs, MATs,
component encapsulation, Python, 3D rendering, Spout/Syphon/NDI, timeline,
undo.

## Layout

```
crates/otd-core   graph, cook engine, parameters, project format  (no GPU, no UI)
crates/otd-gpu    wgpu TOP engine, shaders, headless renderer
crates/otd-app    egui editor shell
```

`otd-core` has no GPU or UI dependency on purpose — it is what makes the cook
engine unit-testable and keeps a headless runtime and a future WASM playground
open.

## Tests

```bash
cargo test --workspace
```

GPU tests skip themselves on machines with no adapter. Both phase exit
criteria are asserted rather than claimed:

- [`tests/phase0.rs`](crates/otd-gpu/tests/phase0.rs) — an animated
  `Noise → Level → viewer` chain at 1280×720 sustains 60 fps (1.3 ms/frame here)
- [`tests/phase1.rs`](crates/otd-gpu/tests/phase1.rs) — the Shadertoy feedback
  patch sustains 60 fps at 1920×1080 (2.5 ms/frame here), actually accumulates
  trails, and survives a save/load round trip byte-identically

## Adding an operator

One `.wgsl` file in `crates/otd-gpu/src/shaders/`, one parameter function, one
uniform-packing function, one entry in the table in `crates/otd-gpu/src/ops.rs`.
Operator breadth is the main long-term risk, so the per-operator cost is kept
near zero deliberately. Or skip Rust entirely and paste a shader into a GLSL
TOP.

## License

MIT.
