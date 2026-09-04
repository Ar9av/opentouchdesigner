---
name: otd-instancing
description: Draw many copies of a shape in OpenTouchDesigner by instancing a Geometry COMP from a CHOP — particle-like fields, point clouds, grids of objects, audio-driven swarms. Use when a patch needs hundreds or thousands of copies rather than one mesh.
---

# Instancing

The upstream `td-pops` skill targets TouchDesigner's GPU particle operators.
**There is no particle operator here.** The equivalent is instancing: one SOP
drawn many times, with a per-instance transform built from a CHOP.

## How it works

`geometryCOMP` with `instancing: true` reads the CHOP at `instancechop`.
**Each *sample* is one instance.** A Pattern CHOP with 2000 samples is 2000
instances. Which channel drives what is named by parameter:

| Parameter | Default channel | Drives |
|---|---|---|
| `tx` `ty` `tz` | `tx` `ty` `tz` | translate |
| `sx` `sy` `sz` | *(empty)* | scale |
| `rx` `ry` `rz` | *(empty)* | rotate |
| `cr` `cg` `cb` `ca` | *(empty)* | colour |

Empty means "not used". The instance count is the length of the longest
channel actually referenced; if none resolve, you get one identity instance —
which is why a typo'd channel name looks like "instancing did nothing".

**A single-sample channel broadcasts to every instance.** That is the whole
trick behind "one LFO scales all of them": wire a 1-sample `lfoCHOP` into the
merge and name it as `sx`, and every copy breathes together.

`instancescale` multiplies every instance's scale uniformly.

## Building the point field

There is no point-simulation operator, so the CHOP *is* the simulation:

- `patternCHOP` — a fixed-length buffer. The usual source of N samples.
- `noiseCHOP` — per-sample noise. Three of these merged is a random cloud.
- `soptochopCHOP` — point attributes of a SOP as channels, one sample per
  point. Scatter with a SOP, instance onto its points.
- `speedCHOP` — integrates: a velocity channel becomes a position channel.
  This is how you get motion that accumulates rather than resets.
- `mergeCHOP` — put the channels side by side.
- `renameCHOP` / `selectCHOP` — get the names to match the parameters above.
- `lagCHOP` — smooth. An instance field that snaps looks like a bug.

A field that responds to audio: `audiospectrumCHOP` → `lagCHOP` →
`selectCHOP(rename: "sy")` merged into the position channels. See the
`otd-audioreactive` skill and `examples/instances3d.otd`.

## Materials for many copies

`pointspriteMAT` draws every point as a camera-facing quad — that is the
particle look, and it is far cheaper than instancing a sphere. Instance a
`lineSOP` or a `boxSOP` when you want the copies to have orientation.

## Known gap

`instancetop` and `instancecount` exist as parameters on `geometryCOMP`, but
texture-based instancing is **not implemented** — `scene::instances()` reads
only `instancechop`. Setting `instancetop` does nothing. Use a CHOP; do not
write a patch that depends on the TOP path, and do not document it as working.

## Cost

Instances are uploaded per frame from the CHOP, capped at 2^20. The CHOP side
is CPU work — a `noiseCHOP` of 100k samples costs every frame whether or not
anything changed. `otd stats patch.otd --frames 300` tells you which node it
is before you start guessing.
