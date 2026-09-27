---
name: otd-geometry
description: Model and deform 3D geometry in OpenTouchDesigner with SOPs — generators, noise, twist/bend/taper, copies, blends and normals — and get it on screen through a Geometry COMP and Render TOP. Use when a patch needs a shaped mesh rather than a flat image, or when a displaced mesh lights wrong.
---

# Geometry (SOPs)

SOPs here are per-point functions over a flat vertex buffer (`crates/otd-sop`).
There is no mesh surgery — no extrude, boolean, or subdivide. What exists
composes well; do not write patches that assume the missing ones.

## Generators

| Op | Notes |
|---|---|
| `boxSOP` | flat-shaded faces |
| `sphereSOP` | UV sphere |
| `tubeSOP` | cylinder / cone / tapered tube; `rows` matters for deforming |
| `torusSOP` | |
| `circleSOP` | disc, ring, arc, or a line |
| `gridSOP` | `rows` × `columns` quads; the usual thing to displace |
| `lineSOP` | `points` between `from` and `to` |

## Filters

| Op | Notes |
|---|---|
| `transformSOP` | translate / rotate (deg) / scale |
| `noiseSOP` | value noise; `along: normal` or `xyz` |
| `twistSOP` | `operation`: `twist` (about the axis), `bend` (curl onto an arc, length preserved), `taper` (scale the cross-section, `strength` is percent per `length`) |
| `facetSOP` | recompute normals: `smooth` or `unique` (faceted) |
| `colorSOP` | one colour on every point |
| `copySOP` | stamp copies with a compounding transform |
| `mergeSOP`, `blendSOP` | combine, or morph by point index |

## Two rules that save an hour

1. **Resolution along the deform axis.** `twistSOP` moves points, so a
   `tubeSOP` with `rows: 1` has two rings and cannot twist visibly. Give it
   32+ rows along `axis`.
2. **Deformers leave normals stale.** `noiseSOP` and `twistSOP` taper do not
   recompute normals, so the mesh lights like the shape it used to be. End the
   chain with `facetSOP`. `unique` for a low-poly look, `smooth` otherwise.

```
tubeSOP(rows: 48) ─► twistSOP(strength: 180) ─► noiseSOP ─► facetSOP ─► null1
```

## Getting it on screen

`geometryCOMP.sop` → `/null1`, a material (`phongMAT` / `pbrMAT` /
`wireframeMAT`), a `cameraCOMP`, a `lightCOMP`, and a `renderTOP` that names
them. The `otd-render3d` skill has the wiring and the parameter names.

Animate by exporting a CHOP into a SOP parameter (e.g. an `lfoCHOP` into
`twistSOP.strength`) — the SOP recooks when its parameter changes. For many
copies, instance instead of `copySOP`: see `otd-instancing`.

## Checking

`cargo test -p otd-sop` covers each operator's geometry. For a patch,
`otd stats patch.otd --frames 120` shows whether a SOP is recooking every
frame (it will, if anything time-dependent feeds its parameters).
