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
| `td-pops` | `otd-instancing` | No POPs. Instancing is driven from a CHOP; `particleCHOP` is a CPU emitter. |
| `td-python` | `otd-python` | Expressions, Script DAT and the Execute DATs. The scope is `ch` / `par` / `parent` / `setpar`, not `op()`. |
| `sc-designer` | `otd-audioreactive` | No synthesis engine here. The audio half is analysis → parameters. |
| — | `otd-patch` | The `.otd` (RON) project format the other five all write. |
| — | `otd-feedback` | Feedback TOP loops: trails, tunnels, motion painting, and why a loop goes black or white. |
| — | `otd-geometry` | SOP modelling and deformation, and the stale-normals trap. |
| — | `otd-camera` | Live camera: differencing, optical flow, blob tracking, keying — and what none of them can detect. |
| `hou-python`, `hou-rs`, `hou-vex` | *(none)* | Houdini. Nothing in this repo corresponds; inventing one would be fiction. |

Every operator a skill names is checked against the registry by
`crates/otd-ai/tests/skills.rs`, so a renamed or removed operator fails the
build instead of leaving a skill that teaches something that does not exist.
