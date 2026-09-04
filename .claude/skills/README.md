# OpenTouchDesigner skills

Mapped from [rheadsh/audiovisual-production-skills](https://github.com/rheadsh/audiovisual-production-skills)
onto this repo. That set targets TouchDesigner, Houdini and SuperCollider; the
names below are the OpenTouchDesigner equivalents, rewritten against this
repo's actual operator registry (`docs/OPERATORS.md`), shader wrapper
(`crates/otd-gpu/src/shader.rs`) and Python scope (`crates/otd-py/src/lib.rs`)
rather than translated word for word.

| Upstream | Here | What changed |
|---|---|---|
| `td-glsl` | `otd-glsl` | WGSL is the primary language; GLSL is Shadertoy-shaped. No `sTD2DInputs`, no `TDOutputSwizzle`. |
| `td-glsl-vertex` | `otd-render3d` | There is no user vertex-shader operator. The 3D pipeline is SOP → Geometry → Render TOP with a MAT. |
| `td-pops` | `otd-instancing` | There is no GPU particle operator. Instancing is driven from a CHOP or a TOP. |
| `td-python` | `otd-python` | Expressions, Script DAT and the Execute DATs. The scope is `ch` / `par` / `parent` / `setpar`, not `op()`. |
| `sc-designer` | `otd-audioreactive` | No synthesis engine here. The audio half is analysis → parameters. |
| — | `otd-patch` | The `.otd` (RON) project format the other five all write. |
| `hou-python`, `hou-rs`, `hou-vex` | *(none)* | Houdini. Nothing in this repo corresponds; inventing one would be fiction. |
