---
name: otd-camera
description: Build live-camera patches in OpenTouchDesigner — webcam input, frame differencing, optical flow, bright-blob tracking, chroma key, freeze frames — and be honest about what each can detect. Use for interactive installations, mirror effects, motion-reactive visuals, or anything with videodeviceinTOP.
---

# Camera patches

`videodeviceinTOP` is the source. Everything below reads it. None of it
recognises people — say so rather than promising body tracking, and never
invent a segmentation or pose operator.

## Source

- `videodeviceinTOP`: `device` blank = default camera; `resw`/`resh`/`fps` are
  *requests* negotiated to a real mode.
- macOS: the first use raises the permission prompt, and an ad-hoc-signed
  rebuild asks again. A camera that was refused is silent; the node says so
  after a few seconds.
- For analysis, **downsample first**: `resolutionTOP` `resw: 160`, `resh: 90`.
  Full-resolution readback or flow is wasted work and noisier.

## Watch: the assistant looks at the camera

The assistant bar has a **Watch** button (or type `/watch make it moody`). It
adds a camera if there is none, waits for a real frame, and sends that frame
with a brief that says *this is the input — design for it* (`Seen::Camera`,
`patch::camera_prompt`), so the model sees whether it is a face close up or a
dark room before choosing. After the plan lands it reads the viewer and the
camera twice, measures them locally (`otd_ai::vision::Measured`: BLACK, BLOWN,
STILL, PASSTHROUGH — the same ruler as `vfx_eval`) and, only if something is
wrong, sends the viewer back once with exactly what was measured
(`Seen::Result`). Code: `crates/otd-app/src/watch.rs`.

## What you can detect

| Want | Chain | Limits |
|---|---|---|
| Where something moved | `feedbackTOP(target: <source>)` → `compositeTOP difference` with live → `levelTOP` gain | camera shake and light changes also count |
| Which way it moved | `opticalflowTOP` (in0 current, in1 `feedbackTOP` of the *same downsampled* node) | small motion only; RG = 0.5 + gain·UV/frame |
| A bright object's position | `resolutionTOP` → `blobtrackCHOP` | largest bright region, not identity; in a bright room that is the wall |
| A moving thing's position | `resolutionTOP` → difference with its `feedbackTOP` → `blurTOP` → `levelTOP` → `blobtrackCHOP` | largest mover; vanishes when still |
| Every pixel as a point | `resolutionTOP` (≤128×128) → `toptochopCHOP` `layout: image` | channels `r g b a u v`, one frame late |
| Cut out a backdrop | `chromakeyTOP` → `compositeTOP over` a new background | needs a real solid-colour backdrop |
| Hold a frame | `cacheTOP` with `active` off | |

`blobtrackCHOP` channels: `present count x y area width height vx vy`, 0..1
with origin **top-left**. Shaders use bottom-left: pass `1 - y`. Gate any
overlay on `present` so it disappears instead of parking at 0,0.

## Known-good recipes

All in `crates/otd-ai/recipes/`, each with notes naming its two useful knobs:
`blobhud` (the viral tracking-box look), `ascii`, `halftone`, `pointcloud`,
`motionpaint`, `datamosh`, `blobfollow`, `greenscreen`, `freeze`,
`nightvision`, `thermal`, `slitscan`, `audiocamera`, and for any footage
`glitch` (RGB split), `neon` (edge glow), `kaleidoscope`, `trails`.

Two traps these recipes already avoid:

- **Exports fill all four components** of a `glslTOP` uniform with one channel,
  and only `uniform1`–`uniform4` exist — four exported numbers per shader.
- **Do not lag tracker positions.** `blobtrackCHOP` reports x, y = 0 when it
  loses the subject, so a lagged box slides into the top-left corner.

## What people build that this cannot

From surveying current TouchDesigner webcam tutorials: MediaPipe hand/pose/
face tracking, background segmentation (NVIDIA Background TOP), Depth Anything
and StreamDiffusion all need an ML runtime this project does not ship; Time
Machine slit-scan and time displacement need a frame-history TOP that does not
exist yet (the `slitscan` recipe scrolls instead). Say so rather than faking
them. Start from one of these
rather than a blank canvas; feedback-based ones are explained in the
`otd-feedback` skill.

## Interaction

- `keyboardinCHOP` (`keys: "f"`) → `mathCHOP` (`gain -1`, `offset 1`) →
  export to `cacheTOP.active`: hold F to freeze.
- `blobtrackCHOP.x/y` → `springCHOP` → a transform: the follow overshoots
  and settles instead of snapping. `lagCHOP` if it should never overshoot.
- `particleCHOP.emitx/emity` ← `blobtrackCHOP.x/y` puts an emitter on the
  tracked object (both are 0..1, top-left).

## Testing without a camera

`vfx_eval` in `crates/otd-ai/examples` cooks plans against a real clip and
flags black / blown-out / still output. Use a `moviefileinTOP` in place of
the camera to make a patch reproducible.
