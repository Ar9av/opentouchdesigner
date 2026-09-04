# OpenTouchDesigner

An open-source, cross-platform, node-based realtime visual programming
environment in the spirit of TouchDesigner. Written in Rust on wgpu, so
`cargo run` works on macOS, Windows and Linux.

**Status: Phase 0 complete, Phase 1 in progress.** See [PLAN.md](PLAN.md) for
the research and the full roadmap. What exists today is a working graph, cook
engine, GPU texture pipeline, text project format and node editor — not yet a
tool you would take to a show.

## Try it

```bash
cargo run -p otd-app
```

It opens on the starter patch: `noise1 → level1 → null1`, animated, running
live. Every node shows its own output at frame rate.

Render a frame with no window at all — something TouchDesigner cannot do on a
Linux server:

```bash
cargo run -p otd-gpu --example render_png -- frame.png examples/starter.otd 120
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

## What works

**Cook engine** (`otd-core`) — demand-driven, pull-based, memoized. A node
cooks only if it changed, an input produced new output, or it is *time
dependent*. Time dependence propagates downstream from animated parameters and
from intrinsically animated operators, so a static branch cooks once and then
costs nothing. Cycles are rejected at connect time; Feedback breaks them by
reading last frame instead. Each node reports its own cook time.

**Parameters** — the four-mode system: Constant, Expression, Export, Bind.
Constant and Expression are live; Export and Bind are visible but inert until
CHOPs land in Phase 2. Expression mode runs a self-contained numeric evaluator
(`absTime`, `frame`, `sin`, `fit`, `clamp`, …) that is a strict subset of the
Python expressions Phase 3 will run, so projects written today keep loading.
Switching a parameter to Expression and back does not lose the constant
underneath.

**TOPs** (`otd-gpu`) — Constant, Noise, Ramp, Level, Transform, Blur,
Composite, Switch, Null, Feedback. All 16-bit float, no resolution cap. One
command encoder per frame; transient textures are pooled and node outputs are
retained for as long as their cache is valid.

**Project format** — text, path-sorted, defaults omitted. Adding a node
appends one block; rewiring changes one line. See
[`examples/starter.otd`](examples/starter.otd).

**Editor** (`otd-app`) — node canvas with live per-node viewers, wiring,
parameter panel, output viewer, cook/GPU statistics in the top bar. The
thumbnails are the operators' real textures shared with egui — no copy, no
readback.

## What doesn't exist yet

CHOPs, SOPs, DATs, MATs, COMponent encapsulation, Python, video and audio I/O,
3D rendering, Spout/Syphon/NDI, timeline, undo. Roughly Phases 2–6 of
[PLAN.md](PLAN.md).

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

GPU tests skip themselves on machines with no adapter. The Phase 0 exit
criterion is asserted in
[`crates/otd-gpu/tests/phase0.rs`](crates/otd-gpu/tests/phase0.rs): an animated
`Noise → Level → viewer` chain at 1280×720 must sustain 60 fps.

## Adding an operator

One `.wgsl` file in `crates/otd-gpu/src/shaders/`, one parameter function, one
uniform-packing function, one entry in the table in `crates/otd-gpu/src/ops.rs`.
Operator breadth is the main long-term risk, so the per-operator cost is kept
near zero deliberately.

## License

MIT.
