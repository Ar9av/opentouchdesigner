---
name: otd-audioreactive
description: Make an OpenTouchDesigner patch respond to sound or a clock — audio device/file input, spectrum bands, beat, MIDI and OSC, smoothed into parameters. Use when asked for audio-reactive visuals, a music video, VJ material, beat-synced motion, or MIDI/OSC control.
---

# Audio-reactive patches

The upstream `sc-designer` skill is SuperCollider — synthesis. **There is no
synthesis engine here.** OpenTouchDesigner's audio half is *analysis*: turn
sound into channels, then channels into parameters.

## Sources

| Operator | Gives you |
|---|---|
| `audiodeviceinCHOP` | Live input, downmixed to mono. Blank `device` = system default. |
| `audiofileinCHOP` | A file, **as a function of the timeline** — not a private play head. |
| `midiinCHOP` | `nNN` per note, plus velocity, pitch bend, `ccNN` per control moved. |
| `oscinCHOP` | One channel per address argument. |
| `beatCHOP` | A tempo clock with ramp, pulse, count and bar — no audio needed. |
| `clockCHOP` | Wall-clock time, which keeps running when the timeline is paused. |

`audiofileinCHOP` playing from the timeline is what makes a headless render
match what you heard: `otd render` reads exactly the samples the editor played.
WAV decodes in-process; anything else needs `ffmpeg` on `PATH`.

## The analysis chain

```
audiodeviceinCHOP ─► audiospectrumCHOP ─► lagCHOP ─► selectCHOP ─► (parameter)
```

- `audiospectrumCHOP` — energy per band, log spaced. `bands` (1–32, default 4)
  names them `band1`..`bandN`. `size` is the FFT window; `decibels` if you want
  a perceptual curve rather than raw energy.
- **`lagCHOP` is not optional.** Raw spectrum energy jitters every frame and
  anything driven by it looks broken. Asymmetric times are the trick:
  `lagup: 0.01, lagdown: 0.25` — snap on the transient, fall off slowly. This
  single node is the difference between "reactive" and "twitchy".
- `selectCHOP` with `channels` and `rename` gives the band a name that means
  something (`bass`, `high`) so the rest of the patch reads.
- `mathCHOP` to scale into the range the target parameter actually wants.
- `analyzeCHOP` to reduce a channel to one number (rms, maximum, …).
- `triggerCHOP` for an ADSR envelope off a threshold — this is how a kick
  becomes a flash that decays rather than a one-frame spike.
- `speedCHOP` integrates: level becomes accumulated rotation.
- `filterCHOP`, `limitCHOP`, `slopeCHOP`, `holdCHOP`, `countCHOP` for the rest.
- `nullCHOP` at the end of each branch — a stable name to export from, so
  reworking the analysis does not break every parameter downstream.

## Getting it into a parameter

Prefer **Export** over an expression:

```ron
"period": (value: Float(0.25), mode: Export, source: "/bass:bass"),
```

`source` is `op_path:channel`. Use `mode: Expression` with
`ch('/bass', 'bass')` only when you need arithmetic the CHOP chain cannot do.

Into a shader: put the channel on a GLSL TOP's `uniform1` and read `U.p0` — see
`otd-glsl`. Into 3D: name it as an instance channel — see `otd-instancing`.

## What to drive

Not brightness. Driving overall brightness with the bass gives you a strobe,
not a patch. The things worth driving are the ones that change *structure*:

| Target | Effect |
|---|---|
| `transformTOP.scale` inside a feedback loop | the tunnel speeds up |
| `levelTOP.brightness` on the feedback decay | trails lengthen and shorten |
| `noiseTOP.period` | the field breathes |
| `mirrorTOP` segments, on a beat count | the symmetry changes on the bar |
| `blurTOP.size` off a high band | air on the cymbals |
| instance `sy` off a spectrum band | the classic bar field |

Read `docs/GUIDE.md` — the feedback loop is where the look comes from; audio
should modulate the loop, not replace it.

## Output too

`audiodeviceoutCHOP` plays the first channel. `midioutCHOP` sends `nNN` as
notes and `ccNN` as controls; `oscoutCHOP` sends one message per frame;
`dmxoutCHOP` sends Art-Net or sACN. `otd run patch.otd` keeps all of those
sending with no window — that is the show-machine mode.

## Reference patch

`examples/audioreactive.otd` — audio in, spectrum, lag, named bands, MIDI note
select, exported onto a TOP chain. `examples/instances3d.otd` is the 3D
version. Read one before writing a new one.
