---
name: otd-glsl
description: Write shaders for the OpenTouchDesigner GLSL TOP — WGSL fragment bodies (the default) or Shadertoy-style GLSL. Use when authoring pixel shaders, generative patterns, image processing, feedback effects, or importing ISF/Shadertoy sources into a `.otd` patch.
---

# Shaders for the GLSL TOP

One operator, `glslTOP`, two languages, chosen by the `language` parameter:
`wgsl` (default) or `glsl`. Source goes in the `source` parameter. It is
validated with naga before the GPU sees it, so a typo gives a line number and
**the last shader that compiled keeps running** — the patch never goes black
mid-edit.

TouchDesigner names do not exist here. No `sTD2DInputs`, no `TDOutputSwizzle`,
no `vUV`, no `TDSimplexNoise`. Writing them is an error, not a fallback.

## WGSL — write a fragment body

The prelude is prepended (`crates/otd-gpu/src/shaders/common.wgsl`). In scope:

```wgsl
in.uv        // vec2<f32>, (0,0) top-left → (1,1) bottom-right
U.res        // x,y = pixels; z,w = 1/pixels
U.time       // x = absolute seconds, y = local, z = frame, w = fps
U.p0 .. U.p3 // the four Uniform parameters, as vec4<f32>
sample0(uv)  // input 0
sample1(uv)  // input 1
```

Body only — no `@fragment`, no signature:

```wgsl
let uv = in.uv - vec2f(0.5);
let d = length(uv) - 0.2 + 0.05 * sin(U.time.x * 2.0);
return vec4f(vec3f(smoothstep(0.01, 0.0, d)), 1.0);
```

Need helper functions? Declare `@fragment` yourself and the source is used as
written, prelude still prepended:

```wgsl
fn fbm(p: vec2f) -> f32 { /* ... */ }

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  return vec4f(vec3f(fbm(in.uv * 6.0 + U.time.x * 0.1)), 1.0);
}
```

`examples/plasma.otd` is exactly this shape — read it before writing a new one.

## GLSL — Shadertoy, unmodified

Set `language: "glsl"` and paste a `mainImage`. Defined for you:
`iResolution`, `iTime`, `iTimeDelta`, `iFrame`, `iMouse`, `iDate`, and
`iChannel0` / `iChannel1` for the two inputs — sampled with `texture()`.

```glsl
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    fragColor = vec4(uv, 0.5 + 0.5 * sin(iTime), 1.0);
}
```

`iChannel0`/`iChannel1` are **already declared** — declaring them yourself is a
redefinition error. Samplers are only bound when the source mentions one, so a
generator costs nothing for inputs it does not use.

A source with its own `void main()` is assembled without the Shadertoy shim.
Shadertoy's `fragCoord` origin is bottom-left; the shim flips for you.

## Uniforms

Four `vec4` parameters — `uniform1`..`uniform4` on the node, `U.p0`..`U.p3` in
the shader. That is the whole mechanism: nothing to configure on a parameter
page, no name matching. For anything time-varying, put an expression or a CHOP
export on the uniform parameter rather than hard-coding it in the shader — the
point of a patch is that the interesting quantities are turnable.

Imported ISF shaders get up to twelve vectors (`PARAM_VECS`) and their declared
inputs become real parameters — that is what *Import ISF…* does.

## Resolution

`sizing` is `Input0OrParams`: with something wired to in0 the GLSL TOP is a
filter and inherits its size; with nothing wired it is a generator at
`resw` × `resh`.

## When not to write a shader

Read `docs/GUIDE.md` first. One enormous GLSL TOP that hard-codes a look is a
screenshot with extra steps — it cannot be turned and it teaches nothing about
the patch. Most good-looking output here is a **feedback loop** of ordinary
TOPs, not a shader. Reach for `glslTOP` for the thing no operator does.

## Verify

```bash
cargo run -p otd-cli -- render patch.otd --node /out1 --frames 1 --out /tmp/f
```

A compile error is reported on the node with a line number; the previous
shader stays live, so "it still looks right" is not proof the edit compiled.
Check the render actually changed.
