# Making things that look good

The operator reference tells you what every parameter does.
[`OPERATORS.md`](OPERATORS.md) is that. This is the other half: *why* a patch
looks like a screensaver instead of like mud, and what the handful of moves
are that most good-looking realtime visuals are actually made of.

If you opened the editor and saw grey noise, you were looking at
`starter` — the smallest patch that proves the engine works, which is a
different job from looking good. Open **File → Examples → tunnel** instead.

---

## The one idea

**Almost nothing interesting comes from a single operator. It comes from a
loop.**

A Noise TOP makes noise. A Blur TOP blurs. Wire ten of them in a row and you
get slightly fancier noise. The step change is when the chain's *output*
becomes its own *input* — then each frame is built on the last one, and
structure accumulates over time that no single operator could produce.

That is what the **Feedback TOP** is for. It reads a TOP's output from the
*previous* frame, which is what lets the loop exist without being a cycle in
the cook graph. The pattern is always the same four nodes:

```
                     ┌─────────────────────────────────┐
                     │                                 │
  source ──────────► composite ──► out ────────────────┘
                          ▲                 (feedback reads `out`,
  feedback ► decay ► move ┘                  one frame behind)
```

- **feedback** points at the end of your chain
- **decay** is a Level TOP with `brightness` slightly *below* 1
- **move** is a Transform TOP that shifts, scales or rotates slightly
- **composite** lays this frame's source over the accumulated result

Change `move` and you change what the loop *is*:

| Transform | What you get |
|---|---|
| `scale` 1.03 | flying forward — a tunnel |
| `scale` 0.97 | falling inward — a drain |
| `rotate` 1° | spirals |
| `translate` x 0.002 | motion smear, like a long exposure |
| scale **and** rotate | the classic warp spiral |

And `decay`'s brightness is how long the trails last. `0.99` is a smear that
takes seconds to fade; `0.85` is a short tail; `1.0` never fades and the
image saturates to white within a second or two. There is no correct value —
this is the dial you sit and turn.

## Recipe 0 — your own footage

**Drag a video, an image or a WAV onto the network.** That is the whole
gesture. A movie becomes a Movie File In already playing and already on the
viewer; a WAV becomes an Audio File In whose channels you can drag onto any
parameter; a `.fs` becomes a GLSL TOP with the shader's inputs as parameters;
a `.otd` opens. No camera? **Media → Use Webcam**.

Then put things after it. With the clip selected, **Media → Add Effect** — or
right-click the node — gives you a Level, a Blur, an HSV Adjust, a
kaleidoscope Mirror and the rest, wired in and viewed in one click. Everything
in Recipes 1–5 works on your clip exactly as it does on a Noise TOP: it is a
texture the moment it is decoded, and nothing downstream knows the difference.

Three things worth knowing:

- **Drop onto an existing node**, not onto empty canvas, and the file lands
  *there* — on a Movie File In it swaps the clip; on a Blur it makes the
  player upstream and wires it in.
- **Movies need `ffmpeg`** (`brew install ffmpeg`). It is looked for on your
  `PATH` and in the usual install directories, so it is found whether the app
  was started from a shell or double-clicked. Stills do not need it — PNG and
  JPEG are decoded in-process. A node that cannot find ffmpeg says so on its
  own body.
- **Keep media beside the project** and paths are stored relative, so
  *File → Export Bundle…* can copy the whole show to another machine.

## Recipe 1 — the tunnel (this is what `tunnel` is)

Nine nodes, no shader. Open it and follow along — or **✦ Recipes → Looks →
tunnel** on the assistant bar builds it into whatever you have open.

```
  noise1 ──► wisps1 ──┐
  palette1 ───────────┴─► tint1 ──┐
                                  ├─► mix1 ──► out1
  fb1 ──► decay1 ──► zoom1 ───────┘
```

1. **noise1** — a Noise TOP, monochrome, with `translate` set to the
   expression `absTime * 0.06` so the field drifts.
2. **wisps1** — a Level TOP with **`blacklevel` at 0.42**. This is the trick
   the whole patch turns on. It throws away the middle of the noise and
   leaves only the brightest streaks, so the loop is fed *sparse highlights*
   instead of a grey wash. Grey wash in, fog out. Sparse highlights in,
   tunnel out.
3. **palette1** — a Ramp TOP, teal to magenta, `phase` on `absTime * 0.05`.
4. **tint1** — a Composite TOP set to `multiply`. Colouring a monochrome
   source through a ramp means the palette is *one node* to change, instead
   of three numbers buried in the noise operator.
5. **fb1 → decay1 → zoom1** — the loop. 0.965 brightness, 1.035 scale,
   0.8° rotate.
6. **mix1** — Composite on `maximum`, so the brightest of (new wisps, old
   frame) wins. `add` also works and blows out sooner; `over` respects alpha.

**Things to try immediately**, since the point of a node graph is that you
can:

- Turn `zoom1`'s scale down to 1.005. The tunnel becomes a slow bloom.
- Set `zoom1`'s rotate to 6. Now it is a vortex.
- Change `palette1`'s two colours. That is the entire look, in two swatches.
- Put a **Blur TOP** between `decay1` and `zoom1` — the trails go soft and
  smoky.
- Drop `wisps1`'s `blacklevel` to 0.1 and watch it turn into fog. That is the
  failure mode, and it is worth seeing on purpose so you recognise it later.

## Recipe 2 — displacement

*✦ Recipes → Video → ripple, or cells.*

The Displace TOP pushes one image around using the pixels of another. It is
the cheapest way to make something look organic rather than computed.

```
  yourimage ─┐
             ├─► displace1 ──► out
  noise1 ────┘
```

Set `amount` to about 0.05 and animate the noise's `translate`. Everything
ripples like heat haze. Feed the *feedback loop* through it instead and the
trails themselves start flowing.

## Recipe 3 — make it react to something

*✦ Recipes → Audio → audio, or beat for a clock instead of a microphone.*

A visual that reacts is the difference between a wallpaper and a performance.
Any CHOP channel can drive any parameter — that is what **Export** mode is.

1. Add an **Audio Device In** CHOP and an **Audio Spectrum** CHOP after it
   (or take `audiolevel` from **File → Palette**, which is those two plus
   smoothing, already wired).
2. Add a **Lag** CHOP. Set `lagup` small and `lagdown` large: fast to rise,
   slow to fall, which is what reads as "reacting to music" rather than as
   jitter.
3. Select the node with the parameter you want to drive, then **drag a
   channel from the channel list in the parameter panel onto that
   parameter**.

Good things to drive, roughly in order of how much they pay back:

| Parameter | Effect |
|---|---|
| `zoom1.scale` | the tunnel lurches forward on every kick |
| `decay1.brightness` | trails get longer when it's loud |
| `wisps1.blacklevel` | more of the image survives on a peak |
| `palette1.phase` | colour shifts with the music |
| `transform.rotate` | spins on a hit |

Put a **Math** CHOP between the channel and the parameter to map the range,
and turn its `clamp` on. A loud room should not be able to drive your visual
somewhere useless.

No audio interface? The `lfo` example does the same thing with an LFO CHOP,
and everything above works identically.

## Recipe 4 — steal a shader

*✦ Recipes → Looks → plasma and rings, and Video → glitch, are each one GLSL
TOP: read the source to see the shape.*

The ceiling of a patch is not the operator list.

- **Paste Shadertoy GLSL.** Set a GLSL TOP's `language` to `glsl` and paste a
  `mainImage` function. `iTime`, `iResolution` and `iFrame` are in scope. Most
  single-buffer Shadertoy shaders run unmodified.
- **Feed a shader your picture.** Wire something into the GLSL TOP and read it
  as `iChannel0` — `texture(iChannel0, uv)` — with the second input on
  `iChannel1`. This is what turns a shader from a generator into an effect. The
  four Uniform parameters arrive as `U.p0`..`U.p3`, so the numbers worth
  turning can stay on the node instead of being buried in the source.
- **Import ISF.** *Import ISF…* on a GLSL TOP loads an
  [Interactive Shader Format](https://isf.video) shader — thousands exist —
  and turns its JSON header into real parameters on the node, which you can
  then export to, bind and animate like any others.
- **Write WGSL.** The default is a fragment *body*: `in.uv`, `U.time.x`,
  `U.res` and `sample0(uv)`/`sample1(uv)` are already in scope, plus
  `U.p0..U.p3` from the four Uniform parameters. If you need helper
  functions, **declare your own `@fragment fn fs_main`** — the source is then
  compiled as written instead of being wrapped, and the prelude is still
  prepended. The `plasma` example is exactly that, and is a domain-warped
  fbm worth reading.

Sources are validated before the GPU sees them, so a typo gives you a line
number, the node outlines red, and the last shader that compiled keeps
running. You can type in a live patch without blacking it out.

## Recipe 5 — 3D, and instancing

*✦ Recipes → 3D → torus, field (instanced), terrain.*

Wire SOPs → a Geometry COMP, add a Camera and a Light, point a **Render TOP**
at them. Its output is an ordinary texture, so everything downstream neither
knows nor cares a camera was involved — bloom it, feed it back, displace it.

The move that pays off is **instancing**. On the Geometry COMP, turn
`instancing` on and point `instancechop` at a CHOP. Every *sample* of that
CHOP becomes an instance: a Pattern CHOP of 2000 samples is 2000 objects in
one draw call, positioned by channels you name. A single-sample channel
broadcasts to all of them, which is how one audio band makes an entire grid
breathe at once. See `instances3d`.

## Why your patch looks like grey mud

Nearly always one of these:

- **No contrast before the loop.** Feedback amplifies whatever it is given. Feed
  it a mid-grey field and you get a mid-grey field, only blurrier. Raise
  `blacklevel` on a Level TOP until only highlights survive.
- **Decay at exactly 1.0.** Nothing ever fades, so everything saturates to
  white. Try 0.95.
- **Everything is moving.** If the source, the palette, the zoom and the
  rotation all animate, the eye has nothing to hold on to. Animate one or two
  things and leave the rest still.
- **Too many colours.** The rainbow default of a colour Noise TOP looks like a
  1998 screensaver. Go monochrome and multiply by a two-colour Ramp instead.
- **Nothing is black.** Contrast is what makes a bright thing look bright.
  Deep blacks in most of the frame are why the `tunnel` example reads as
  glowing.

## Where to look next

- **File → Palette** — `trails`, `bloom`, `vignette`, `audiolevel`. Each is an
  ordinary component you can step *inside* with `I`, so opening one up is the
  tutorial on how it was built.
- **File → Examples** — `tunnel` and `plasma` for looks, `feedback` for the
  Shadertoy path, `audioreactive` for exports, `instances3d` for 3D,
  `keyframes` for the timeline, `video` for movie input.
- **The Perf window** — when it starts to chug, this tells you which node is
  actually costing you, ranked by cost per *frame* rather than per cook.
- [`OPERATORS.md`](OPERATORS.md) — all 79, with every parameter, generated
  from the same table the editor builds its menus from.
