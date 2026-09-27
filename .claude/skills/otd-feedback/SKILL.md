---
name: otd-feedback
description: Build feedback loops in OpenTouchDesigner — trails, echoes, tunnels, smears, motion painting, slit-scan — with the Feedback TOP. Use when a patch needs motion that accumulates over frames, or when a loop goes black, white or grey.
---

# Feedback loops

Motion in most good patches is not in any one operator — it is a loop that
reads last frame's picture, changes it slightly, and lays the new frame on
top. `docs/GUIDE.md` is the long version; this is the construction.

## The only shape that works

```
source ─────────────────────► mix (compositeTOP) ─► out1 (nullTOP)
                                 ▲ input 1
feedbackTOP(target: out1) ─► decay/move ┘
```

- `feedbackTOP.target` names the **end** of the chain (`out1`), and reads it
  **one frame late**. That is why it is not a graph cycle. A `selectTOP` in the
  same place *is* a cycle and is refused.
- `feedbackTOP` has **no input wire**. Wiring something into it does nothing.
- If you rename `out1`, retarget the feedback. A loop pointed at a node that
  no longer exists reads black forever.

## Keeping it alive

Every loop needs exactly one of these, or it saturates in about a second:

| Close the loop with | Decay comes from | Trail length knob |
|---|---|---|
| `compositeTOP` `over`, `opacity` 0.7–0.92 | the fresh frame covering history | `opacity` |
| `compositeTOP` `add` or `maximum` | a `levelTOP` `brightness` 0.9–0.97 on the history | `brightness` |

The `trails` recipe (`crates/otd-ai/recipes/trails.json`) is the known-good
crossfade version; copy its numbers before inventing your own.

## Making it move

Put **one** small transform between the feedback and the mix:

- `transformTOP` `scale` 1.01–1.05 → tunnel / zoom trails; `rotate` 0.3–2° → swirl.
- `displaceTOP` fed by a slow `noiseTOP` → drifting ink; keep `amount` ≤ 0.02.
- `flowTOP` → advection. One flow warps one image; it is not a fluid solver.
- `blurTOP` on the history → soft glow trails.

Small numbers compound: 1.05 per frame is 18× in a second.

## Motion-driven loops (camera)

- **Motion painting** — `feedbackTOP(target: source1)` is the *previous camera
  frame*; `compositeTOP difference` with the live frame isolates movement;
  gain it with `levelTOP`, then accumulate into a second loop with `maximum`
  and a decaying `levelTOP`. See `recipes/motionpaint.json`.
- **Datamosh** — `opticalflowTOP` (inputs: current, previous) → `displaceTOP`
  with `amount -1`, `offset -0.5` on a history loop. Two separate feedback
  nodes: one for the previous camera frame, one for the picture history.
  See `recipes/datamosh.json`.
- **Slit-scan** — stamp a thin mask of the live frame, scroll the canvas with
  `transformTOP` `extend: zero`, close with `maximum`. `recipes/slitscan.json`.

## When it goes wrong

| You see | Cause | Fix |
|---|---|---|
| White blow-out | `add` with no decay, or decay ≥ 1 | `levelTOP.brightness` < 1, or switch to `over` |
| Black | target misspelled, or the decay is before the source enters | check `target`; decay only the history branch |
| Grey mud | low-contrast source, blur inside the loop every frame | contrast *before* the loop, blur sparingly |
| Output is the raw clip | work parked on `out2` while the viewer shows `out1` | insert before the existing null |
| Frozen | nothing time-dependent upstream and a still source | add a slow `transformTOP` or `noiseTOP` drift |

Verify headlessly: `otd render patch.otd --node /out1 --frames 120 --out frames/`
and look at frame 1 against frame 120 — a healthy loop differs, is neither
all-black nor all-white, and still shows the source.
