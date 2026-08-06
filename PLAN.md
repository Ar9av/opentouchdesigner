# OpenTouchDesigner — Research & Build Plan

An open-source, cross-platform (macOS / Windows / Linux), node-based realtime
visual programming environment in the spirit of TouchDesigner.

*Plan drafted 2026-08-05 from research into TD's architecture, the open-source
landscape, and 2026 implementation stacks.*

---

## 1. Why this project has a lane

- **No cross-platform open TouchDesigner exists.** The closest project, TiXL
  (formerly Tooll 3, ~4.1k★, MIT), is Windows-only and locked to D3D11 via the
  abandoned SharpDX. cables.gl is browser-bound (no NDI/DMX/Spout/heavy video
  I/O). vvvv gamma's editor is closed and Windows-only. Nothing credible exists
  in Rust or modern C++.
- **TD's own pain points are the wedge features:**
  1. **Linux support** (top perennial forum request — servers, installations)
  2. **Text-serializable, git-diffable project format** (.toe/.tox are binary)
  3. **No resolution cap** (TD free tier is limited to 1280×1280 — the single
     most-cited gripe)
  4. **Parallel cook** (TD's main loop is largely single-threaded)
  5. **GPU-first geometry** (TD's SOPs are CPU-bound; Derivative is only now
     shipping POPs)
  6. Free standalone export, headless/CLI deployment, a real timeline.
- **The moat to respect:** TD ≈ 700 polished operators + a decade of tutorials
  + hiring pool. We can't out-breadth them head-on; we need leverage
  (ISF/GLSL shader import, a community operator registry à la ComfyUI custom
  nodes, docs-first culture).
- **Abstraction lesson from the field:** TD's genius is a *small number of
  typed wire families*, not a general-purpose language. vvvv generalized into
  a real language and lost the artist audience. Keep the families.

## 2. What we're actually cloning (the essence of TD)

Five load-bearing ideas, in priority order:

1. **Typed dataflow families** — TOP (GPU textures), CHOP (channels of
   samples), SOP (geometry), DAT (text/tables), MAT (materials), COMP
   (containers). Same-family wiring only; explicit converter ops + parameter
   references bridge families.
2. **Demand-driven cook engine** — pull-based lazy evaluation with dirty
   flags, memoized outputs, and *time-dependence propagation* (animated nodes
   re-cook every frame; static branches cook once and cache). Per-component
   local time.
3. **The four-mode parameter system** — every parameter is Constant |
   Expression (Python) | Export (CHOP channel override) | Bind (two-way).
   Custom parameters on components ARE the component API.
4. **COMP encapsulation** — In/Out ops inside a component surface as typed
   connectors on the node; clones, replicators, external component files.
5. **Live viewers everywhere** — every node renders its actual output on its
   body at frame rate. The network *is* the debugger. This UX detail is the
   product.

## 3. Tech stack (decided)

**Native Rust.** Rationale: `cargo run` works first-try on all three OSes
(order-of-magnitude better contributor onboarding than CMake/vcpkg C++), no
GC in the render/audio path, wgpu gives native Metal on macOS (no MoltenVK),
and the adjacent Rust communities (Bevy, Rerun, Graphite) are exactly the
contributor pool we need. Graphite is the existence proof of a realtime
node-graph tool on wgpu; Rerun proves egui scales to a serious tool UI.

| Concern | Choice | Notes |
|---|---|---|
| GPU | **wgpu** (WGSL via naga) | compute + render; Vulkan/Metal/DX12; ~within 10% of raw Vulkan for texture-graph work |
| Windowing | winit | multi-window output for projectors |
| UI | **egui**, custom node editor (start from `egui-snarl`) | budget for owning this layer — the network editor is the product surface |
| Cook engine | hand-rolled dirty-flag + pull, salsa-style | revision counters + `time_dependent` flag propagation |
| GPU execution | frame-graph pattern | walk dirty graph → DAG of passes → transient texture pool keyed by (size, format); Feedback ops get persistent double buffers |
| Audio | **cpal** + own DSP on a dedicated callback thread | lock-free ring buffers to/from the cook loop |
| Video | **GStreamer-rs** (hardware decode; maintained) | ffmpeg-next is maintenance-mode; ffmpeg subprocess for offline export |
| Scripting | **embedded CPython via PyO3** | non-negotiable for TD migration; bundle python-build-standalone; scripts run at a fixed phase of the frame, never block render |
| Plugins | dylib and/or WASM custom operators | compiled path for perf; registry later |
| Shaders (user) | **WGSL primary, GLSL accepted** (naga frontend) | "paste Shadertoy GLSL and it works" materially eases migration; ISF import for a free library of thousands of effects |
| Project format | **text (RON or JSON) + content-addressed assets** | git-diffable from day one — a headline feature, so design it early |

Crate layout: `otd-core` (graph + cook engine, no UI dep) / `otd-gpu` /
`otd-ops-*` (per family) / `otd-py` / `otd-app` (egui shell). A clean core
keeps a future WASM/WebGPU browser playground open (Graphite's playbook)
without betting the product on the browser sandbox.

## 4. Architecture sketch

- **Graph model:** arena/slotmap of nodes; typed ports; hierarchy of
  components with In/Out ops surfacing as connectors. All state serializable
  to the text format.
- **Cook loop (per frame):** advance time → collect roots (visible viewers,
  output windows, active device outputs) → pull; a node cooks iff dirty or
  time-dependent, recursively pulling inputs first; otherwise returns cached
  output. CHOPs use TD-style *time-slicing* (cook only samples since last
  frame) so audio/control stays continuous across dropped frames.
- **Threads:** render/UI thread (vsync-paced) · cook can parallelize
  independent dirty branches (rayon) — a headline advantage over TD ·
  audio callback thread · media decode threads · Python at a fixed frame
  phase under the GIL.
- **Feedback:** cycles disallowed in the cook graph; Feedback TOP/CHOP break
  them by reading last frame's buffer (persistent double-buffered textures).
- **Parameters:** four modes as an enum on each param; expression engine
  evaluates Python with `op()`, `me`, `absTime` in scope; exports are CHOP
  channel subscriptions; dependency tracking hooks expressions into the dirty
  system.

## 5. Roadmap

### Phase 0 — Skeleton (~weeks)
Repo, CI (3-OS matrix), `otd-core` graph + dirty/pull cook engine with unit
tests, egui app shell with a minimal node canvas, text project format
round-trip. **Exit:** wire Noise TOP → Level TOP → viewer at 60fps.

### Phase 1 — TOP engine + editor MVP (the "toy that's already fun")
- ~20 TOPs: Constant, Noise, Ramp, Movie File In (GStreamer), Video Device In,
  Composite/Over, Level, Transform, Blur, Displace, Feedback, Cache, Switch,
  Select, Null, Out, Movie File Out, **GLSL/WGSL TOP**, Resolution, Render (stub).
- Frame-graph executor with transient texture pooling; live thumbnails on
  nodes; parameter panel with Constant/Expression modes; OP-create dialog
  (Tab, type-ahead); output Window to a second display.
- **Exit criterion / demo:** a Shadertoy-style feedback visual patched live,
  full-resolution, on all three OSes. This is the announceable milestone.

### Phase 2 — CHOPs + parameters complete (the control system)
- ~20 CHOPs: LFO, Noise, Math, Lag, Filter, Logic, Trigger, Timer, Select,
  Merge, Speed, Pattern, Count, Switch, Mouse In, Keyboard In, **Audio Device
  In/Out, Audio File In, Audio Spectrum**, Null.
- Time-slicing, Export + Bind modes (drag channel onto parameter), Python
  expressions everywhere, MIDI In/Out (midir) + OSC In/Out (rosc).
- **Exit:** audio-reactive visual driven by a MIDI controller.

### Phase 3 — Components, Python, persistence maturity
- COMP hierarchy with typed In/Out connectors, custom parameter pages,
  clones, Replicator; embedded CPython: `op()` API, extensions on components,
  callback DATs (Execute, CHOP Execute, Parameter Execute); core DATs (Table,
  Text, Script, Select, Merge, JSON, WebSocket/UDP/Serial).
- External component files (.otdc, text) + git-friendly merges as a showcase.
- **Exit:** a reusable audio-visualizer component instantiated twice with
  different parameters; project diffed meaningfully in git.

### Phase 4 — 3D pipeline
- Geometry/Camera/Light COMPs, Render TOP, PBR + GLSL-style MAT, instancing
  from CHOP/DAT/TOP sources (texture-based instancing = the signature TD
  trick), depth/MRT outputs.
- Geometry **GPU-first from day one** (compute-shader point ops — leapfrog
  TD's CPU SOP legacy rather than reproduce it), with a small CPU SOP set
  (Box, Sphere, Grid, Transform, Copy, Noise, Merge, File In) for
  compatibility of mental model.
- **Exit:** classic TD demo — instanced geometry driven by audio, post-processed
  through a TOP chain.

### Phase 5 — Show I/O + deployment (VJ-ecosystem table stakes)
- **Spout (Win) / Syphon (mac)** texture sharing — mandatory for adoption;
  NDI in/out (SDK licensing caveat: dynamic-load, optional feature);
  DMX/Art-Net/sACN out; Ableton Link; ISF shader import.
- Perform mode (editor off, output only), **headless CLI runtime** (Linux
  server rendering — something TD simply cannot do), project export as a
  self-contained runnable.

### Phase 6 — Polish & community flywheel
Performance monitor (per-node cook ms, GPU mem), timeline/keyframe animation
(TiXL proved demand; TD is weak here), undo/redo everywhere, palette of
prebuilt components, operator-registry infrastructure, docs site with
per-operator pages + examples (docs are half the moat — write them alongside
each operator, enforced in PR review).

## 6. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Node-editor UI effort underestimated (egui-snarl is young) | Accept owning it; it's the product surface. Steal interaction patterns from TD/Houdini/ComfyUI. |
| Operator breadth treadmill (TD has ~700) | ISF + GLSL import for instant effect breadth; plugin registry; ruthless top-20-per-family prioritization. |
| Video I/O complexity | GStreamer behind a trait; ship Movie File In early and harden continuously. |
| Python GIL vs realtime | Scripts run at a fixed frame phase (TD does the same); heavy work pushed to ops/compute. |
| Solo-maintainer burnout (the graveyard: VSXu, Fugio, Vuo) | Small always-shippable milestones; each phase ends in a demoable artifact; consider NLnet-style grants (funded cables.gl) once Phase 1 demos exist. |
| NDI/Spout proprietary bits | Optional features, dynamically loaded, never in the core build. |

## 7. Non-goals (v1)

Feature parity with TD Pro (Notch hosting, genlock, projector calibration),
a general-purpose visual language (vvvv's trap), mobile, collaborative
multi-user editing, browser as the primary target (WASM playground is a
later bonus, not the product).

## 8. First concrete steps

1. `cargo new` workspace: `otd-core`, `otd-gpu`, `otd-app`; CI matrix.
2. Implement the graph + cook engine in `otd-core` with tests (dirty
   propagation, time-dependence, caching, cycles rejected).
3. wgpu frame-graph executor + three TOPs (Constant, Noise, Level).
4. egui shell: node canvas from egui-snarl, live thumbnails, parameter panel.
5. Text serialization round-trip.
6. Ship the Phase 1 demo GIF; announce; recruit.
