# Operator reference

Generated from the operator registry — the same table the editor builds its menus and parameter pages from, so this cannot drift from what the operators actually do.

77 operators.

**CHOP** (30)

- [animationCHOP](#animation--animationchop) — Keyframed curves over time.
- [audiodeviceinCHOP](#audio-device-in--audiodeviceinchop) — Samples from an audio input, downmixed to mono.
- [audiodeviceoutCHOP](#audio-device-out--audiodeviceoutchop) — Plays its first channel on an audio output.
- [audiofileinCHOP](#audio-file-in--audiofileinchop) — Plays a WAV file, following the timeline.
- [audiospectrumCHOP](#audio-spectrum--audiospectrumchop) — Energy per frequency band, log spaced.
- [constantCHOP](#constant--constantchop) — Fixed values as channels.
- [countCHOP](#count--countchop) — Count threshold crossings.
- [dmxoutCHOP](#dmx-out--dmxoutchop) — Sends its channels as DMX over Art-Net or sACN.
- [filterCHOP](#filter--filterchop) — Low-pass or moving-average smoothing.
- [inCHOP](#in--inchop) — A channel input on this component's node.
- [keyboardinCHOP](#keyboard-in--keyboardinchop) — One channel per named key, 1 while held.
- [lagCHOP](#lag--lagchop) — Smooth a channel, with separate rise and fall times.
- [lfoCHOP](#lfo--lfochop) — A repeating waveform over time.
- [logicCHOP](#logic--logicchop) — Turn channels into on/off and combine them.
- [mathCHOP](#math--mathchop) — Combine and scale channels.
- [mergeCHOP](#merge--mergechop) — Put two CHOPs' channels side by side.
- [midiinCHOP](#midi-in--midiinchop) — Notes, velocity, pitch bend and the controls you have moved.
- [midioutCHOP](#midi-out--midioutchop) — Sends `nNN` channels as notes and `ccNN` channels as controls.
- [mouseinCHOP](#mouse-in--mouseinchop) — Cursor position and buttons.
- [noiseCHOP](#noise--noisechop) — Smooth or random noise over time.
- [nullCHOP](#null--nullchop) — Pass-through. A stable name to export from.
- [oscinCHOP](#osc-in--oscinchop) — Incoming OSC messages, one channel per address argument.
- [oscoutCHOP](#osc-out--oscoutchop) — Sends its channels as one OSC message per frame.
- [outCHOP](#out--outchop) — This component's channel output.
- [patternCHOP](#pattern--patternchop) — A fixed-length waveform buffer.
- [selectCHOP](#select--selectchop) — Pick and rename channels.
- [speedCHOP](#speed--speedchop) — Integrate a channel: velocity becomes position.
- [switchCHOP](#switch--switchchop) — Choose one of two inputs.
- [timerCHOP](#timer--timerchop) — A running timer with fraction, seconds, cycles and done.
- [triggerCHOP](#trigger--triggerchop) — An attack/decay/sustain/release envelope per channel.

**COMP** (5)

- [cameraCOMP](#camera--cameracomp) — A viewpoint for a Render TOP.
- [containerCOMP](#container--containercomp) — A component that holds a sub-network.
- [geometryCOMP](#geometry--geometrycomp) — Places a SOP in the scene, optionally instanced.
- [lightCOMP](#light--lightcomp) — A directional light, aimed from its position at the origin.
- [replicatorCOMP](#replicator--replicatorcomp) — Keeps one clone of a master component per row of a table.

**DAT** (11)

- [inDAT](#in--indat) — A data input on this component's node.
- [jsonDAT](#json--jsondat) — Parse JSON text into a path/value table.
- [mergeDAT](#merge--mergedat) — Join two DATs by rows or by columns.
- [nullDAT](#null--nulldat) — Pass-through. A stable name to reference.
- [outDAT](#out--outdat) — This component's data output.
- [scriptDAT](#script--scriptdat) — Rows produced by a Python script.
- [selectDAT](#select--selectdat) — Pick rows and columns, by name or index.
- [tableDAT](#table--tabledat) — A table of text, stored in the project file.
- [textDAT](#text--textdat) — A block of text.
- [udpinDAT](#udp-in--udpindat) — Datagrams received on a port, one message per row.
- [udpoutDAT](#udp-out--udpoutdat) — Sends its input's text as a datagram when it changes.

**MAT** (1)

- [pbrMAT](#pbr--pbrmat) — Base colour, metallic, roughness and emission, with an optional map.

**SOP** (12)

- [boxSOP](#box--boxsop) — A box with flat-shaded faces.
- [colorSOP](#color--colorsop) — Set the colour carried by every point.
- [copySOP](#copy--copysop) — Stamp copies with a compounding transform.
- [gridSOP](#grid--gridsop) — A flat grid of quads — the usual thing to displace.
- [inSOP](#in--insop) — A geometry input on this component's node.
- [lineSOP](#line--linesop) — A run of points between two positions.
- [mergeSOP](#merge--mergesop) — Combine two pieces of geometry.
- [noiseSOP](#noise--noisesop) — Displace points by value noise, along their normals or freely.
- [nullSOP](#null--nullsop) — Pass-through. A stable name to reference.
- [outSOP](#out--outsop) — This component's geometry output.
- [sphereSOP](#sphere--spheresop) — A UV sphere.
- [transformSOP](#transform--transformsop) — Translate, rotate and scale points.

**TOP** (18)

- [blurTOP](#blur--blurtop) — Separable Gaussian blur.
- [cacheTOP](#cache--cachetop) — Holds the last frame it saw when Active is off.
- [compositeTOP](#composite--compositetop) — Blend two inputs. Input 2 is composited over input 1.
- [constantTOP](#constant--constanttop) — A flat colour at a chosen resolution.
- [displaceTOP](#displace--displacetop) — Offsets input 1's lookup by channels of input 2.
- [feedbackTOP](#feedback--feedbacktop) — Last frame's output of the Target TOP.
- [glslTOP](#glsl--glsltop) — Your own shader, compiled live. WGSL or Shadertoy-style GLSL.
- [inTOP](#in--intop) — A texture input on this component's node.
- [levelTOP](#level--leveltop) — Brightness, contrast, gamma, black/white levels.
- [noiseTOP](#noise--noisetop) — Fractal value noise. Animate Translate Z to make it move.
- [nullTOP](#null--nulltop) — Pass-through. A stable name to reference and to view.
- [outTOP](#out--outtop) — This component's texture output.
- [rampTOP](#ramp--ramptop) — Linear or radial gradient between two colours.
- [renderTOP](#render--rendertop) — Draws Geometry components through a Camera.
- [resolutionTOP](#resolution--resolutiontop) — Resamples its input to an explicit resolution.
- [selectTOP](#select--selecttop) — This frame's output of another TOP, by path.
- [switchTOP](#switch--switchtop) — Select one of two inputs, optionally blending between them.
- [transformTOP](#transform--transformtop) — Translate, rotate and scale, with an extend mode.

---

## CHOP

### Animation — `animationCHOP`

Keyframed curves over time.

The keys are text — `channel time value interpolation`, one per line — so they diff cleanly and twenty evenly spaced keys are faster typed than dragged. The parameter panel draws the same data as an editable curve.

Interpolation is `constant` (hold, then jump), `linear`, `smooth` (ease in and out) or `spline` (Catmull-Rom, continuous through the keys). Outside the keyed range the curve holds its end values rather than extrapolating.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Keys | `keys` | *(multi-line)* |  |
| Play | `play` | `timeline` | `timeline` · `loop` · `hold` |
| Speed | `speed` | `1` | -4 … 4 |
| Offset | `offset` | `0` | -60 … 60 |
| Sample Rate | `rate` | `60` | 1 … 48000 |

### Audio Device In — `audiodeviceinCHOP`

Samples from an audio input, downmixed to mono.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Device (blank = default) | `device` | *(empty)* |  |
| Volume | `volume` | `1` | 0 … 8 |

### Audio Device Out — `audiodeviceoutCHOP`

Plays its first channel on an audio output.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Device (blank = default) | `device` | *(empty)* |  |
| Volume | `volume` | `1` | 0 … 2 |
| Active | `active` | `true` |  |

### Audio File In — `audiofileinCHOP`

Plays a WAV file, following the timeline.

Plays RIFF/WAVE — PCM in 8, 16, 24 or 32 bits, or 32-bit float — which is what a DAW bounces. Compressed formats wait for the video layer and its real media stack.

Playback is a function of the timeline, not a private play head: the sample at time `t` is always the file at `t × speed`. Scrubbing the timeline scrubs the audio, a loop range loops it, and a headless render reads exactly the samples the editor played.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| File (.wav) | `file` | *(empty)* |  |
| Play | `play` | `loop` | `loop` · `once` |
| Speed | `speed` | `1` | -4 … 4 |
| Volume | `volume` | `1` | 0 … 8 |

### Audio Spectrum — `audiospectrumCHOP`

Energy per frequency band, log spaced.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| FFT Size | `size` | `1024` | `256` · `512` · `1024` · `2048` · `4096` |
| Bands | `bands` | `4` | 1 … 32 |
| Gain | `gain` | `1` | 0 … 64 |
| Decibels | `decibels` | `false` |  |

### Constant — `constantCHOP`

Fixed values as channels.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Channels | `channels` | `1` | 1 … 4 |
| Name | `name` | `chan` |  |
| Value 1 | `value0` | `0` |  |
| Value 2 | `value1` | `0` |  |
| Value 3 | `value2` | `0` |  |
| Value 4 | `value3` | `0` |  |
| Sample Rate | `rate` | `60` | 1 … 48000 |

### Count — `countCHOP`

Count threshold crossings.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Threshold | `threshold` | `0.5` |  |
| Edge | `edge` | `rising` | `rising` · `falling` · `both` |
| Reset | `reset` | `false` |  |
| Limit | `limit` | `off` | `off` · `clamp` · `wrap` |
| Min | `min` | `0` |  |
| Max | `max` | `8` |  |

### DMX Out — `dmxoutCHOP`

Sends its channels as DMX over Art-Net or sACN.

Sends each channel as a DMX slot over Art-Net or sACN (E1.31). Values are expected in 0..1 and are clamped rather than wrapped — a light jumping from full to black on an overshoot is much worse than one that saturates.

DMX is a state protocol, so each frame sends the current value of each channel, which is the last sample of the slice.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Protocol | `protocol` | `artnet` | `artnet` · `sacn` |
| Address (blank = broadcast / multicast) | `address` | *(empty)* |  |
| Universe | `universe` | `0` | 0 … 32767 |
| Start Channel | `start` | `1` | 1 … 512 |
| Active | `active` | `true` |  |

### Filter — `filterCHOP`

Low-pass or moving-average smoothing.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Type | `type` | `lowpass` | `lowpass` · `movingaverage` |
| Width (s) | `width` | `0.2` | 0 … 4 |

### In — `inCHOP`

A channel input on this component's node.

*No inputs — this is a generator.*

### Keyboard In — `keyboardinCHOP`

One channel per named key, 1 while held.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Keys (space separated) | `keys` | `a s d f` |  |

### Lag — `lagCHOP`

Smooth a channel, with separate rise and fall times.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Lag Up (s) | `lagup` | `0.1` | 0 … 2 |
| Lag Down (s) | `lagdown` | `0.1` | 0 … 2 |

### LFO — `lfoCHOP`

A repeating waveform over time.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Type | `type` | `sine` | `sine` · `triangle` · `ramp` · `square` · `pulse` |
| Frequency | `frequency` | `1` | 0 … 20 |
| Amplitude | `amplitude` | `1` | 0 … 4 |
| Offset | `offset` | `0` | -2 … 2 |
| Phase | `phase` | `0` | 0 … 1 |
| Pulse Width | `pulsewidth` | `0.5` | 0 … 1 |
| Channel Name | `name` | `lfo` |  |
| Sample Rate | `rate` | `60` | 1 … 48000 |

### Logic — `logicCHOP`

Turn channels into on/off and combine them.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Convert | `convert` | `nonzero` | `off` · `nonzero` · `greater` · `less` · `equal` |
| Threshold | `threshold` | `0.5` |  |
| Invert | `invert` | `false` |  |
| Combine Channels | `combine` | `off` | `off` · `and` · `or` · `xor` |

### Math — `mathCHOP`

Combine and scale channels.

**Inputs:** a, b

| Parameter | Name | Default | Range |
|---|---|---|---|
| Combine Inputs | `combine` | `add` | `add` · `subtract` · `multiply` · `divide` · `maximum` · `minimum` · `average` |
| Gain | `gain` | `1` | -4 … 4 |
| Offset | `offset` | `0` | -4 … 4 |
| Clamp | `clamp` | `false` |  |
| Clamp Min | `clampmin` | `0` |  |
| Clamp Max | `clampmax` | `1` |  |

### Merge — `mergeCHOP`

Put two CHOPs' channels side by side.

**Inputs:** a, b

### MIDI In — `midiinCHOP`

Notes, velocity, pitch bend and the controls you have moved.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Port (blank = first) | `device` | *(empty)* |  |
| Notes (e.g. 36 38 42) | `notes` | *(empty)* |  |

### MIDI Out — `midioutCHOP`

Sends `nNN` channels as notes and `ccNN` channels as controls.

Channel names mirror MIDI In: `n60` is note 60, `cc74` is control 74; anything else is ignored. A note fires when its value crosses zero, with the value at that moment as velocity — while it is held, changes of value are not new notes.

Unlike DMX, MIDI is an event protocol, so only *changes* go on the wire; an unchanged control costs nothing per frame.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Port (blank = first) | `device` | *(empty)* |  |
| Channel | `channel` | `1` | 1 … 16 |
| Active | `active` | `true` |  |

### Mouse In — `mouseinCHOP`

Cursor position and buttons.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

### Noise — `noiseCHOP`

Smooth or random noise over time.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Type | `type` | `smooth` | `smooth` · `random` |
| Channels | `channels` | `1` | 1 … 8 |
| Name | `name` | `noise` |  |
| Period | `period` | `1` | 0.01 … 10 |
| Amplitude | `amplitude` | `1` | 0 … 4 |
| Offset | `offset` | `0` | -2 … 2 |
| Seed | `seed` | `0` |  |
| Sample Rate | `rate` | `60` | 1 … 48000 |

### Null — `nullCHOP`

Pass-through. A stable name to export from.

**Inputs:** in

### OSC In — `oscinCHOP`

Incoming OSC messages, one channel per address argument.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Port | `port` | `9000` | 1 … 65535 |

### OSC Out — `oscoutCHOP`

Sends its channels as one OSC message per frame.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Address | `address` | `127.0.0.1` |  |
| Port | `port` | `9001` | 1 … 65535 |
| OSC Path | `path` | `/otd` |  |
| Active | `active` | `true` |  |

### Out — `outCHOP`

This component's channel output.

**Inputs:** in

### Pattern — `patternCHOP`

A fixed-length waveform buffer.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Type | `type` | `ramp` | `ramp` · `sine` · `triangle` · `square` · `gaussian` |
| Length | `length` | `64` | 1 … 4096 |
| Periods | `periods` | `1` | 0.1 … 32 |
| Amplitude | `amplitude` | `1` | 0 … 4 |
| Offset | `offset` | `0` | -2 … 2 |
| Phase | `phase` | `0` | 0 … 1 |
| Channel Name | `name` | `chan1` |  |

### Select — `selectCHOP`

Pick and rename channels.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Channel Names | `channels` | `*` |  |
| Rename To | `rename` | *(empty)* |  |

### Speed — `speedCHOP`

Integrate a channel: velocity becomes position.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Gain | `gain` | `1` | -4 … 4 |
| Initial Value | `initial` | `0` |  |
| Reset | `reset` | `false` |  |
| Limit | `limit` | `off` | `off` · `clamp` · `wrap` |
| Min | `min` | `0` |  |
| Max | `max` | `1` |  |

### Switch — `switchCHOP`

Choose one of two inputs.

**Inputs:** 0, 1

| Parameter | Name | Default | Range |
|---|---|---|---|
| Index | `index` | `0` | 0 … 1 |

### Timer — `timerCHOP`

A running timer with fraction, seconds, cycles and done.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Length (s) | `length` | `10` | 0.1 … 600 |
| Cycle | `cycle` | `true` |  |
| Play | `play` | `true` |  |
| Sample Rate | `rate` | `60` | 1 … 48000 |

### Trigger — `triggerCHOP`

An attack/decay/sustain/release envelope per channel.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Threshold | `threshold` | `0.5` |  |
| Attack (s) | `attack` | `0.05` | 0 … 2 |
| Decay (s) | `decay` | `0.2` | 0 … 4 |
| Sustain Level | `sustain` | `0` | 0 … 1 |
| Release (s) | `release` | `0.3` | 0 … 4 |

---

## COMP

### Camera — `cameraCOMP`

A viewpoint for a Render TOP.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Translate | `translate` | `[0.0, 0.0, 5.0]` |  |
| Rotate | `rotate` | `[0.0, 0.0, 0.0]` |  |
| Look At | `lookat` | *(empty)* |  |
| Projection | `projection` | `perspective` | `perspective` · `orthographic` |
| Field of View | `fov` | `45` | 5 … 170 |
| Ortho Height | `orthosize` | `4` | 0.1 … 100 |
| Near | `near` | `0.1` | 0.001 … 10 |
| Far | `far` | `200` | 1 … 10000 |

### Container — `containerCOMP`

A component that holds a sub-network.

*No inputs — this is a generator.*

### Geometry — `geometryCOMP`

Places a SOP in the scene, optionally instanced.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| SOP | `sop` | *(empty)* |  |
| Material | `material` | *(empty)* |  |
| Translate | `translate` | `[0.0, 0.0, 0.0]` |  |
| Rotate | `rotate` | `[0.0, 0.0, 0.0]` |  |
| Scale | `scale` | `[1.0, 1.0, 1.0]` |  |
| Instancing | `instancing` | `false` |  |
| Instance CHOP | `instancechop` | *(empty)* |  |
| Instance TOP | `instancetop` | *(empty)* |  |
| Instance Scale | `instancescale` | `1` | 0 … 10 |
| Instance Count (TOP) | `instancecount` | `1024` | 1 … 1000000 |
| Translate X Channel | `tx` | `tx` |  |
| Translate Y Channel | `ty` | `ty` |  |
| Translate Z Channel | `tz` | `tz` |  |
| Scale X Channel | `sx` | *(empty)* |  |
| Scale Y Channel | `sy` | *(empty)* |  |
| Scale Z Channel | `sz` | *(empty)* |  |
| Rotate X Channel | `rx` | *(empty)* |  |
| Rotate Y Channel | `ry` | *(empty)* |  |
| Rotate Z Channel | `rz` | *(empty)* |  |
| Red Channel | `cr` | *(empty)* |  |
| Green Channel | `cg` | *(empty)* |  |
| Blue Channel | `cb` | *(empty)* |  |
| Alpha Channel | `ca` | *(empty)* |  |

### Light — `lightCOMP`

A directional light, aimed from its position at the origin.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Translate | `translate` | `[3.0, 5.0, 4.0]` |  |
| Color | `color` | `[1.0, 1.0, 1.0, 1.0]` |  |
| Intensity | `intensity` | `1` | 0 … 8 |

### Replicator — `replicatorCOMP`

Keeps one clone of a master component per row of a table.

Watches a template DAT and keeps one clone of its master component per data row. The first column names each replicant; any other column whose header matches a custom parameter on the master sets that parameter on the replicant — so the table *is* the population, and adding a row is adding an instance.

Replicants are ordinary clones: they follow the master's network as it is edited, and anything you place inside the replicator by hand is yours and is left alone.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Master (path of the component to copy) | `master` | *(empty)* |  |
| Template DAT (one replicant per row) | `template` | *(empty)* |  |
| First Row/Column Are Names | `byname` | `true` |  |

---

## DAT

### In — `inDAT`

A data input on this component's node.

*No inputs — this is a generator.*

### JSON — `jsonDAT`

Parse JSON text into a path/value table.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| JSON Pointer (e.g. /items/0) | `pointer` | *(empty)* |  |

### Merge — `mergeDAT`

Join two DATs by rows or by columns.

**Inputs:** a, b

| Parameter | Name | Default | Range |
|---|---|---|---|
| Merge By | `how` | `rows` | `rows` · `columns` |

### Null — `nullDAT`

Pass-through. A stable name to reference.

**Inputs:** in

### Out — `outDAT`

This component's data output.

**Inputs:** in

### Script — `scriptDAT`

Rows produced by a Python script.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Source | `source` | *(multi-line)* |  |

### Select — `selectDAT`

Pick rows and columns, by name or index.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Rows (names, indices or *) | `rows` | `*` |  |
| Columns | `cols` | `*` |  |
| First Row/Column Are Names | `byname` | `true` |  |

### Table — `tableDAT`

A table of text, stored in the project file.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Contents | `text` | *(multi-line)* |  |
| Delimiter | `delimiter` | `tab` | `tab` · `comma` |

### Text — `textDAT`

A block of text.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Text | `text` | *(empty)* |  |

### UDP In — `udpinDAT`

Datagrams received on a port, one message per row.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Port | `port` | `7000` | 1 … 65535 |
| Rows Kept | `keep` | `20` | 1 … 1000 |

### UDP Out — `udpoutDAT`

Sends its input's text as a datagram when it changes.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Address | `address` | `127.0.0.1` |  |
| Port | `port` | `7001` | 1 … 65535 |
| Active | `active` | `true` |  |

---

## MAT

### PBR — `pbrMAT`

Base colour, metallic, roughness and emission, with an optional map.

**Inputs:** color

| Parameter | Name | Default | Range |
|---|---|---|---|
| Base Color | `basecolor` | `[0.8, 0.8, 0.85, 1.0]` |  |
| Metallic | `metallic` | `0` | 0 … 1 |
| Roughness | `roughness` | `0.4` | 0.02 … 1 |
| Emit | `emit` | `0` | 0 … 4 |

---

## SOP

### Box — `boxSOP`

A box with flat-shaded faces.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Size | `size` | `[1.0, 1.0, 1.0]` |  |
| Center | `center` | `[0.0, 0.0, 0.0]` |  |

### Color — `colorSOP`

Set the colour carried by every point.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Color | `color` | `[1.0, 1.0, 1.0, 1.0]` |  |

### Copy — `copySOP`

Stamp copies with a compounding transform.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Copies | `count` | `3` | 1 … 512 |
| Translate Each | `translate` | `[1.0, 0.0, 0.0]` |  |
| Rotate Each (deg) | `rotate` | `[0.0, 0.0, 0.0]` |  |
| Scale Each | `scale` | `[1.0, 1.0, 1.0]` |  |

### Grid — `gridSOP`

A flat grid of quads — the usual thing to displace.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Size | `size` | `[2.0, 2.0]` |  |
| Rows | `rows` | `10` | 2 … 512 |
| Columns | `columns` | `10` | 2 … 512 |
| Orientation | `orientation` | `xy` | `xy` · `xz` · `yz` |

### In — `inSOP`

A geometry input on this component's node.

*No inputs — this is a generator.*

### Line — `lineSOP`

A run of points between two positions.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| From | `from` | `[-1.0, 0.0, 0.0]` |  |
| To | `to` | `[1.0, 0.0, 0.0]` |  |
| Points | `points` | `2` | 2 … 4096 |

### Merge — `mergeSOP`

Combine two pieces of geometry.

**Inputs:** a, b

### Noise — `noiseSOP`

Displace points by value noise, along their normals or freely.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Amplitude | `amplitude` | `0.2` | 0 … 4 |
| Period | `period` | `1` | 0.01 … 10 |
| Offset | `offset` | `[0.0, 0.0, 0.0]` |  |
| Displace Along | `along` | `normal` | `normal` · `xyz` |

### Null — `nullSOP`

Pass-through. A stable name to reference.

**Inputs:** in

### Out — `outSOP`

This component's geometry output.

**Inputs:** in

### Sphere — `sphereSOP`

A UV sphere.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Radius | `radius` | `0.5` | 0.01 … 10 |
| Rows | `rows` | `16` | 3 … 128 |
| Columns | `columns` | `24` | 3 … 256 |
| Center | `center` | `[0.0, 0.0, 0.0]` |  |

### Transform — `transformSOP`

Translate, rotate and scale points.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Translate | `translate` | `[0.0, 0.0, 0.0]` |  |
| Rotate (deg) | `rotate` | `[0.0, 0.0, 0.0]` |  |
| Scale | `scale` | `[1.0, 1.0, 1.0]` |  |

---

## TOP

### Blur — `blurTOP`

Separable Gaussian blur.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Size (px) | `size` | `8` | 0 … 128 |

### Cache — `cacheTOP`

Holds the last frame it saw when Active is off.

Freezes its input. Turn `Active` off and the texture stops updating, which is how you hold a frame — or stop paying for a branch you are not currently looking at.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Active | `active` | `true` |  |

### Composite — `compositeTOP`

Blend two inputs. Input 2 is composited over input 1.

**Inputs:** base, over

| Parameter | Name | Default | Range |
|---|---|---|---|
| Operation | `operation` | `over` | `over` · `add` · `multiply` · `screen` · `difference` · `subtract` · `maximum` · `minimum` |
| Opacity | `opacity` | `1` | 0 … 1 |

### Constant — `constantTOP`

A flat colour at a chosen resolution.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Color | `color` | `[0.0, 0.0, 0.0, 1.0]` |  |
| Resolution W | `resw` | `1280` | 1 … 4096 |
| Resolution H | `resh` | `720` | 1 … 4096 |

### Displace — `displaceTOP`

Offsets input 1's lookup by channels of input 2.

**Inputs:** source, displace

| Parameter | Name | Default | Range |
|---|---|---|---|
| Amount | `amount` | `0.1` | -1 … 1 |
| Source X | `sourcex` | `r` | `r` · `g` · `b` · `a` · `luminance` |
| Source Y | `sourcey` | `g` | `r` · `g` · `b` · `a` · `luminance` |
| Offset | `offset` | `-0.5` | -1 … 1 |
| Extend | `extend` | `hold` | `zero` · `hold` · `repeat` · `mirror` |

### Feedback — `feedbackTOP`

Last frame's output of the Target TOP.

Reads the target TOP's output from the **previous** frame. That is what lets a feedback loop exist without a cycle in the cook graph — the Select TOP, which reads the current frame, would be a cycle and is rejected.

Point `Target TOP` at the node whose output you want to feed back, wire this node's output into that chain, and put a Level TOP in between to decay it. Without the decay the loop saturates within a second.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Target TOP | `target` | *(empty)* |  |

### GLSL — `glslTOP`

Your own shader, compiled live. WGSL or Shadertoy-style GLSL.

Write a fragment body; the boilerplate is supplied. In WGSL, `in.uv`, `U.time.x`, `U.res` and `sample0(uv)`/`sample1(uv)` are in scope, along with `U.p0..U.p3` from the four Uniform parameters. In GLSL a Shadertoy `mainImage` with `iTime` and `iResolution` runs unmodified.

Sources are validated before the GPU sees them, so a typo gives a line number and the last shader that compiled keeps running — the patch never goes black mid-edit. *Import ISF…* loads a published ISF shader and turns its inputs into parameters on this node.

**Inputs:** in0, in1

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Language | `language` | `wgsl` | `wgsl` · `glsl` |
| Source | `source` | *(multi-line)* |  |
| Uniform 1 | `uniform1` | `[1.0, 0.6, 0.2, 1.0]` |  |
| Uniform 2 | `uniform2` | `[0.0, 0.0, 0.0, 0.0]` |  |
| Uniform 3 | `uniform3` | `[0.0, 0.0, 0.0, 0.0]` |  |
| Uniform 4 | `uniform4` | `[0.0, 0.0, 0.0, 0.0]` |  |
| Resolution W | `resw` | `1280` | 1 … 4096 |
| Resolution H | `resh` | `720` | 1 … 4096 |

### In — `inTOP`

A texture input on this component's node.

*No inputs — this is a generator.*

### Level — `levelTOP`

Brightness, contrast, gamma, black/white levels.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Brightness | `brightness` | `1` | 0 … 4 |
| Contrast | `contrast` | `1` | 0 … 4 |
| Gamma | `gamma` | `1` | 0.1 … 4 |
| Opacity | `opacity` | `1` | 0 … 1 |
| Black Level | `blacklevel` | `0` | 0 … 1 |
| White Level | `whitelevel` | `1` | 0 … 1 |
| Invert | `invert` | `0` | 0 … 1 |

### Noise — `noiseTOP`

Fractal value noise. Animate Translate Z to make it move.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Period | `period` | `0.25` | 0.01 … 4 |
| Harmonics | `harmonics` | `3` | 1 … 10 |
| Roughness | `roughness` | `0.5` | 0 … 1 |
| Exponent | `exponent` | `1` | 0.1 … 8 |
| Translate | `translate` | `[0.0, 0.0, 0.0]` |  |
| Monochrome | `monochrome` | `true` |  |
| Amplitude | `amplitude` | `1` | 0 … 4 |
| Offset | `offset` | `0` | -1 … 1 |
| Resolution W | `resw` | `1280` | 1 … 4096 |
| Resolution H | `resh` | `720` | 1 … 4096 |

### Null — `nullTOP`

Pass-through. A stable name to reference and to view.

**Inputs:** in

### Out — `outTOP`

This component's texture output.

**Inputs:** in

### Ramp — `rampTOP`

Linear or radial gradient between two colours.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Type | `type` | `horizontal` | `horizontal` · `vertical` · `radial` |
| Phase | `phase` | `0` | 0 … 1 |
| Color 1 | `color1` | `[0.0, 0.0, 0.0, 1.0]` |  |
| Color 2 | `color2` | `[1.0, 1.0, 1.0, 1.0]` |  |
| Resolution W | `resw` | `1280` | 1 … 4096 |
| Resolution H | `resh` | `720` | 1 … 4096 |

### Render — `renderTOP`

Draws Geometry components through a Camera.

Draws the 3D scene into an ordinary texture, so everything after it is a normal TOP chain that neither knows nor cares a camera was involved.

It finds its Geometry, Camera and Light COMPs by parameter rather than by wire, and those references are real cook dependencies: they cook first, and their animation propagates here.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Geometry | `geometry` | *(empty)* |  |
| Camera | `camera` | *(empty)* |  |
| Light | `light` | *(empty)* |  |
| Background | `background` | `[0.0, 0.0, 0.0, 1.0]` |  |
| Ambient | `ambient` | `0.12` | 0 … 1 |
| Wireframe | `wireframe` | `false` |  |
| Cull | `cull` | `back` | `back` · `front` · `none` |
| Resolution W | `resw` | `1280` | 1 … 4096 |
| Resolution H | `resh` | `720` | 1 … 4096 |

### Resolution — `resolutionTOP`

Resamples its input to an explicit resolution.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Filter | `filter` | `linear` | `linear` · `nearest` |
| Resolution W | `resw` | `1280` | 1 … 4096 |
| Resolution H | `resh` | `720` | 1 … 4096 |

### Select — `selectTOP`

This frame's output of another TOP, by path.

Reads another TOP's output from the **current** frame, so the source cooks first. Use it to fan a texture out to several branches without drawing wires across the whole network. For a loop, use Feedback instead: a Select pointing back at its own chain is a cycle and is refused.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| TOP | `top` | *(empty)* |  |

### Switch — `switchTOP`

Select one of two inputs, optionally blending between them.

**Inputs:** 0, 1

| Parameter | Name | Default | Range |
|---|---|---|---|
| Index | `index` | `0` | 0 … 1 |
| Blend | `blend` | `false` |  |

### Transform — `transformTOP`

Translate, rotate and scale, with an extend mode.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Translate | `translate` | `[0.0, 0.0]` |  |
| Rotate | `rotate` | `0` | -180 … 180 |
| Scale | `scale` | `[1.0, 1.0]` |  |
| Extend | `extend` | `zero` | `zero` · `hold` · `repeat` · `mirror` |

