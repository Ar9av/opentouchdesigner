---
name: otd-render3d
description: Build 3D scenes in OpenTouchDesigner — SOP geometry, Geometry/Camera/Light COMPs, MATs and the Render TOP, including the depth pass. Use when asked for 3D, meshes, lighting, shading, materials, depth-of-field or fog, or when a patch needs a rendered pass rather than a flat image.
---

# 3D and the Render TOP

The upstream `td-glsl-vertex` skill has no equivalent here: **there is no user
vertex-shader operator.** Shading is the five MATs, and geometry is deformed by
SOPs on the CPU or by instancing. Do not tell anyone to write a vertex shader.

## The chain

```
SOP (shape) ──► geometryCOMP ──┐
cameraCOMP ────────────────────┼──► renderTOP ──► (TOP chain)
lightCOMP ─────────────────────┘
MAT ──► geometryCOMP.material
```

`renderTOP` takes no inputs — it is a generator. It references the others by
path: `geometry`, `camera`, `light`, `material` are path parameters, not wires.

## Geometry

16 SOPs: `boxSOP`, `sphereSOP`, `torusSOP`, `tubeSOP`, `gridSOP`, `circleSOP`,
`lineSOP`, plus `noiseSOP` (displace along normals), `transformSOP`, `blendSOP`
(morph two shapes), `copySOP` (compounding-transform stamps), `mergeSOP`,
`colorSOP`, `nullSOP`.

`gridSOP` → `noiseSOP` is the standard displaced-terrain move. `blendSOP` with
an animated blend is how a shape becomes another shape.

## Materials

| MAT | Use |
|---|---|
| `constantMAT` | Flat colour, ignores lights. The right default for graphic looks. |
| `phongMAT` | Diffuse + Blinn highlight, dialled by shininess. |
| `pbrMAT` | Base colour, metallic, roughness, emission, with an optional map. |
| `wireframeMAT` | Edges rather than faces. |
| `pointspriteMAT` | Every point as a camera-facing quad — this is your particle look. |

`lightCOMP` is a single directional light aimed from its position at the
origin. `renderTOP.ambient` (default `0.12`) is the fill; with one light and no
ambient, everything facing away is black.

## The depth pass

`renderTOP.output: depth` re-draws with shading skipped, writing **metric
distance** between `depthnear` and `depthfar` as grey — near white, far black.
Unwritten background is black, so it multiplies straight into a composite as a
mask.

It is deliberately *not* the depth buffer's own `1/z` value, which puts nearly
all its precision in the first few units and reads flat past arm's length.
Set `depthnear`/`depthfar` to the part of the scene you care about — those two
numbers decide whether the pass looks like anything.

Uses: fog (`lookupTOP` or `crossTOP` against the colour pass), depth of field
(`lumablurTOP` with depth as input 2), rim separation, cheap god rays.

## A minimal scene

```ron
(path: "/torus1",  op: "torusSOP",      pos: (-400.0, 0.0)),
(path: "/mat1",    op: "phongMAT",      pos: (-400.0, 120.0)),
(path: "/geo1",    op: "geometryCOMP",  pos: (-220.0, 0.0), params: {
   "sop": (value: Str("/torus1")), "material": (value: Str("/mat1")),
   "rotate": (value: Vec3((0.0,0.0,0.0)), mode: Expression,
              expression: "[0.0, absTime * 30.0, 0.0]"),
}),
(path: "/cam1",    op: "cameraCOMP",    pos: (-220.0, 120.0)),
(path: "/light1",  op: "lightCOMP",     pos: (-220.0, 240.0)),
(path: "/render1", op: "renderTOP",     pos: (-40.0, 0.0), params: {
   "geometry": (value: Str("/geo1")),
   "camera":   (value: Str("/cam1")),
   "light":    (value: Str("/light1")),
}),
```

`examples/instances3d.otd` is the fuller version — read it.

## After the render

A raw render almost never looks finished. The move is `renderTOP` →
`levelTOP` (crush to the highlights) → `blurTOP` (wide) → `addTOP` back over
the original. That is bloom, and it is four nodes.

See the `otd-instancing` skill for many copies, and `otd-patch` for the file
format.
