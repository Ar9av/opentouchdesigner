# OpenTouchDesigner

An open-source, cross-platform, node-based realtime visual programming
environment in the spirit of TouchDesigner. Written in Rust on wgpu, so
`cargo run` works on macOS, Windows and Linux.

**Status: Phases 0–3 complete apart from video I/O.** See [PLAN.md](PLAN.md)
for the research and the full roadmap. What exists today is a working graph,
cook engine, GPU texture pipeline with live shader compilation, a channel
pipeline with audio, MIDI and OSC input, the four-mode parameter system, a
text project format, component encapsulation with embedded Python, a node
editor and projector output — not yet a tool you would take to a show.

## Try it

```bash
cargo run -p otd-app
```

It opens on the starter patch: `noise1 → level1 → null1`, animated, running
live. Every node shows its own output at frame rate — a TOP shows its texture,
a CHOP shows its waveform.

**File → Examples** has the demos each phase was built to reach:

| | |
|---|---|
| `starter` | `noise → level → viewer`, animated by expressions |
| `feedback` | a Shadertoy shader driving a feedback loop at 1920×1080 |
| `audioreactive` | audio spectrum and MIDI notes driving a visual through Exports |
| `lfo` | the smallest thing that shows a channel driving a parameter |
| `components` | one visualiser component used twice, listening to different bands |

Render a frame with no window at all — something TouchDesigner cannot do on a
Linux server:

```bash
cargo run -p otd-engine --example render_png -- frame.png examples/feedback.otd 300
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
| `I` / double-click a COMP | step inside a component |
| `U` | step back out (or click the breadcrumb) |
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

**Parameters** — the four-mode system, all four live: **Constant**,
**Expression**, **Export** (driven by a CHOP channel) and **Bind** (following
another parameter). Drag a channel from the panel onto a parameter to export
it; click the mode button to release it, keeping whatever value it was showing
so the picture doesn't jump. Expression mode runs a self-contained numeric
evaluator (`absTime`, `frame`, `sin`, `fit`, `clamp`, …), falling through to
Python for anything more. Switching a parameter to Expression and back does
not lose the constant underneath.

**Components** — In and Out operators inside a component surface as typed
connectors on its node, so a component's shape is defined by what is in it.
Custom parameters are its API: operators inside read them as `parent.gain`,
and the same network behaves differently in each instance without any of it
being duplicated. The boundary is a dependency in both directions, so
dirtiness and animation cross it by exactly the rules a wire uses.

A component can be saved as a `.otdc` file and shared: the project then holds
a reference and the instance's settings while the file holds the network, so
two projects using one component share one definition and editing it is one
diff. A component can instead **clone** another one in the same project,
following its structure while keeping its own parameter values. Either way,
re-reading a shared definition never resets the values an artist dialled in.

**Python** (`otd-py`) — expressions are evaluated in two tiers. A small
built-in language handles the common case with no interpreter and no GIL;
anything it cannot parse goes to embedded CPython, with `math`, `random`,
`json`, TD's `clamp`/`fit`/`lerp`, and read-only access to the network through
`ch()`, `par()` and `parent()`. Compiled code is cached by source. An operator
path quoted inside an expression or a script becomes a cook dependency, so
`ch('/lfo1', 'chan1')` makes `/lfo1` cook first rather than reading it stale.
A failing expression keeps its constant and reports one line.

**DATs** (`otd-dat`) — Table, Text, Select, Merge, JSON, Script and Null. A
table's contents live in the project file, so a cue list is versioned with the
patch that uses it. Script DATs run Python and return rows.

**CHOPs** (`otd-chop`) — Constant, LFO, Noise, Pattern, Math, Lag, Filter,
Logic, Trigger, Timer, Speed, Count, Select, Merge, Switch, Null, plus Audio
Device In, Audio Spectrum, MIDI In, OSC In, OSC Out, Mouse In and Keyboard In.

Channels are **time sliced** the way TD does it: a generator emits however
many samples cover the frame interval that *actually* elapsed, so an LFO stays
on pitch and audio stays continuous when the renderer stutters. Devices run on
their own threads — cpal's audio callback, midir's MIDI thread, a socket
thread for OSC — and hand buffers to the cook; nothing device-related can
block a frame. A missing interface reports itself on the node and produces
silence rather than failing the cook, because losing a device mid-show must
not stop the render.

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
parameter panel with a shader code editor and a draggable channel list, output
viewer, a second output window for a projector, cook/GPU statistics in the top
bar. The thumbnails are the operators' real textures shared with egui — no
copy, no readback.

## What doesn't exist yet

**Video I/O** — Movie File In/Out and Video Device In are the one part of
Phase 1 not built. They need GStreamer, which is a system dependency rather
than a crate, and none of it can be verified without it installed.

Everything else is Phases 4–6 of [PLAN.md](PLAN.md): SOPs, MATs, 3D
rendering, Spout/Syphon/NDI, timeline, undo.

## Layout

```
crates/otd-core     graph, cook engine, parameters, project format  (no GPU, no UI)
crates/otd-chop     channels, time slicing, audio/MIDI/OSC          (no GPU, no UI)
crates/otd-dat      text and tables                                 (no GPU, no UI)
crates/otd-py       embedded CPython for expressions and scripts
crates/otd-gpu      wgpu TOP engine, shaders
crates/otd-engine   the cross-family cook, demo patches, headless renderer
crates/otd-app      egui editor shell
```

`otd-gpu` and `otd-chop` know nothing about each other. `otd-engine` is the
only place they meet, and it exists for one reason: a TOP parameter in Export
mode has to read a CHOP channel cooked in the same frame.

`otd-core` has no GPU or UI dependency on purpose — it is what makes the cook
engine unit-testable and keeps a headless runtime and a future WASM playground
open.

## Tests

```bash
cargo test --workspace
```

GPU tests skip themselves on machines with no adapter. Every phase exit
criterion is asserted rather than claimed:

- [`otd-gpu/tests/phase0.rs`](crates/otd-gpu/tests/phase0.rs) — an animated
  `Noise → Level → viewer` chain at 1280×720 sustains 60 fps (1.3 ms/frame here)
- [`otd-gpu/tests/phase1.rs`](crates/otd-gpu/tests/phase1.rs) — the Shadertoy
  feedback patch sustains 60 fps at 1920×1080 (2.5 ms/frame here), actually
  accumulates trails, and survives a save/load round trip byte-identically
- [`otd-engine/tests/phase2.rs`](crates/otd-engine/tests/phase2.rs) — a channel
  drives a texture parameter within the same frame, a note reaches a
  transform, and the audio-reactive patch runs with nothing plugged in
- [`otd-engine/tests/phase3.rs`](crates/otd-engine/tests/phase3.rs) — one
  component used twice renders two different pictures from one definition,
  and turning one of its knobs changes exactly one line of the project file
- [`otd-engine/tests/external.rs`](crates/otd-engine/tests/external.rs) — two
  projects share one `.otdc`, an edit to it reaches both, and neither project
  file contains the shared network

The OSC test is a real UDP loopback and the spectrum test is a real FFT, so
those paths are exercised rather than mocked.

## Adding an operator

One `.wgsl` file in `crates/otd-gpu/src/shaders/`, one parameter function, one
uniform-packing function, one entry in the table in `crates/otd-gpu/src/ops.rs`.
Operator breadth is the main long-term risk, so the per-operator cost is kept
near zero deliberately. Or skip Rust entirely and paste a shader into a GLSL
TOP.

## License

MIT.
