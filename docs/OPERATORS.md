# Operator reference

Generated from the operator registry — the same table the editor builds its menus and parameter pages from, so this cannot drift from what the operators actually do.

131 operators.

**CHOP** (46)

- [analyzeCHOP](#analyze--analyzechop) — Reduce each channel to one number.
- [animationCHOP](#animation--animationchop) — Keyframed curves over time.
- [audiodeviceinCHOP](#audio-device-in--audiodeviceinchop) — Samples from an audio input, downmixed to mono.
- [audiodeviceoutCHOP](#audio-device-out--audiodeviceoutchop) — Plays its first channel on an audio output.
- [audiofileinCHOP](#audio-file-in--audiofileinchop) — Plays an audio file, following the timeline.
- [audiospectrumCHOP](#audio-spectrum--audiospectrumchop) — Energy per frequency band, log spaced.
- [beatCHOP](#beat--beatchop) — A tempo clock: ramp, pulse, count and bar.
- [clockCHOP](#clock--clockchop) — Wall-clock time, which keeps running when the timeline is paused.
- [constantCHOP](#constant--constantchop) — Fixed values as channels.
- [countCHOP](#count--countchop) — Count threshold crossings.
- [crossCHOP](#cross--crosschop) — Blend two inputs, with an equal-power option.
- [dattochopCHOP](#dat-to-chop--dattochopchop) — Columns or rows of a table, as channels.
- [delayCHOP](#delay--delaychop) — Play a channel back later, with optional feedback.
- [dmxoutCHOP](#dmx-out--dmxoutchop) — Sends its channels as DMX over Art-Net or sACN.
- [expressionCHOP](#expression--expressionchop) — Apply an expression to every sample. `v` is the sample.
- [filterCHOP](#filter--filterchop) — Low-pass or moving-average smoothing.
- [holdCHOP](#hold--holdchop) — Freeze input 1's value until input 2 fires.
- [inCHOP](#in--inchop) — A channel input on this component's node.
- [keyboardinCHOP](#keyboard-in--keyboardinchop) — One channel per named key, 1 while held.
- [lagCHOP](#lag--lagchop) — Smooth a channel, with separate rise and fall times.
- [lfoCHOP](#lfo--lfochop) — A repeating waveform over time.
- [limitCHOP](#limit--limitchop) — Clamp, wrap, fold or quantise a channel.
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
- [panelCHOP](#panel--panelchop) — Panel widget values as channels.
- [patternCHOP](#pattern--patternchop) — A fixed-length waveform buffer.
- [renameCHOP](#rename--renamechop) — Rename channels by pattern.
- [resampleCHOP](#resample--resamplechop) — Change how many samples a buffer has, keeping its shape.
- [selectCHOP](#select--selectchop) — Pick and rename channels.
- [shuffleCHOP](#shuffle--shufflechop) — Rearrange the channel/sample grid — transpose, reverse, sequence.
- [slopeCHOP](#slope--slopechop) — The rate of change of a channel.
- [soptochopCHOP](#sop-to-chop--soptochopchop) — Point attributes as channels, one sample per point.
- [speedCHOP](#speed--speedchop) — Integrate a channel: velocity becomes position.
- [switchCHOP](#switch--switchchop) — Choose one of two inputs.
- [timerCHOP](#timer--timerchop) — A running timer with fraction, seconds, cycles and done.
- [toptochopCHOP](#top-to-chop--toptochopchop) — Pixels as channels. Reads last frame; costs a GPU sync.
- [triggerCHOP](#trigger--triggerchop) — An attack/decay/sustain/release envelope per channel.

**COMP** (8)

- [buttonCOMP](#button--buttoncomp) — A button on the output. Its state is the Value parameter.
- [cameraCOMP](#camera--cameracomp) — A viewpoint for a Render TOP.
- [containerCOMP](#container--containercomp) — A component that holds a sub-network.
- [fieldCOMP](#field--fieldcomp) — An editable text field on the output.
- [geometryCOMP](#geometry--geometrycomp) — Places a SOP in the scene, optionally instanced.
- [lightCOMP](#light--lightcomp) — A directional light, aimed from its position at the origin.
- [replicatorCOMP](#replicator--replicatorcomp) — Keeps one clone of a master component per row of a table.
- [sliderCOMP](#slider--slidercomp) — A fader on the output. Its position is the Value parameter.

**DAT** (19)

- [chopexecuteDAT](#chop-execute--chopexecutedat) — Python callbacks when a watched channel changes or crosses a threshold.
- [choptodatDAT](#chop-to-dat--choptodatdat) — Channels as a table of numbers.
- [convertDAT](#convert--convertdat) — Between a table and a block of text.
- [executeDAT](#execute--executedat) — Python callbacks at the start and end of a frame.
- [inDAT](#in--indat) — A data input on this component's node.
- [jsonDAT](#json--jsondat) — Parse JSON text into a path/value table.
- [mergeDAT](#merge--mergedat) — Join two DATs by rows or by columns.
- [nullDAT](#null--nulldat) — Pass-through. A stable name to reference.
- [outDAT](#out--outdat) — This component's data output.
- [parameterexecuteDAT](#parameter-execute--parameterexecutedat) — Python callbacks when a watched parameter changes.
- [scriptDAT](#script--scriptdat) — Rows produced by a Python script.
- [selectDAT](#select--selectdat) — Pick rows and columns, by name or index.
- [sortDAT](#sort--sortdat) — Sort rows by a column, numerically or as text.
- [substituteDAT](#substitute--substitutedat) — Fill $name placeholders from a two-column lookup table.
- [tableDAT](#table--tabledat) — A table of text, stored in the project file.
- [textDAT](#text--textdat) — A block of text.
- [transposeDAT](#transpose--transposedat) — Swap rows and columns.
- [udpinDAT](#udp-in--udpindat) — Datagrams received on a port, one message per row.
- [udpoutDAT](#udp-out--udpoutdat) — Sends its input's text as a datagram when it changes.

**MAT** (5)

- [constantMAT](#constant--constantmat) — Flat colour, unaffected by lights.
- [pbrMAT](#pbr--pbrmat) — Base colour, metallic, roughness and emission, with an optional map.
- [phongMAT](#phong--phongmat) — Diffuse and a Blinn highlight, dialled by shininess.
- [pointspriteMAT](#point-sprite--pointspritemat) — Draws every point as a camera-facing quad.
- [wireframeMAT](#wireframe--wireframemat) — Draws the edges rather than the faces.

**SOP** (16)

- [blendSOP](#blend--blendsop) — Morph between two shapes by interpolating point positions.
- [boxSOP](#box--boxsop) — A box with flat-shaded faces.
- [circleSOP](#circle--circlesop) — A disc, ring or arc — filled, or a line to copy along.
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
- [torusSOP](#torus--torussop) — A torus.
- [transformSOP](#transform--transformsop) — Translate, rotate and scale points.
- [tubeSOP](#tube--tubesop) — A cylinder, cone or tapered tube, with optional caps.

**TOP** (37)

- [blurTOP](#blur--blurtop) — Separable Gaussian blur.
- [cacheTOP](#cache--cachetop) — Holds the last frame it saw when Active is off.
- [choptotopTOP](#chop-to-top--choptotoptop) — Channels as pixels: a row per channel, a column per sample.
- [chromakeyTOP](#chroma-key--chromakeytop) — Key out one colour, matching on hue rather than brightness.
- [circleTOP](#circle--circletop) — An antialiased disc or ellipse.
- [compositeTOP](#composite--compositetop) — Blend two inputs. Input 2 is composited over input 1.
- [constantTOP](#constant--constanttop) — A flat colour at a chosen resolution.
- [displaceTOP](#displace--displacetop) — Offsets input 1's lookup by channels of input 2.
- [ditherTOP](#dither--dithertop) — Quantise to few levels, with an ordered or noise dither.
- [edgeTOP](#edge--edgetop) — Sobel edge detection.
- [feedbackTOP](#feedback--feedbacktop) — Last frame's output of the Target TOP.
- [flipTOP](#flip--fliptop) — Mirror the image about an axis, or transpose it.
- [flowTOP](#flow--flowtop) — Advect the picture along a curl-noise field. Loop it for smoke.
- [glslTOP](#glsl--glsltop) — Your own shader, compiled live. WGSL or Shadertoy-style GLSL.
- [hsvadjustTOP](#hsv-adjust--hsvadjusttop) — Hue, saturation and value, optionally over one band of the wheel.
- [inTOP](#in--intop) — A texture input on this component's node.
- [levelTOP](#level--leveltop) — Brightness, contrast, gamma, black/white levels.
- [lookupTOP](#lookup--lookuptop) — Input 1's brightness reads a colour out of input 2.
- [mathTOP](#math--mathtop) — Arithmetic on two inputs, per channel.
- [mirrorTOP](#mirror--mirrortop) — Fold the image onto itself, including a radial kaleidoscope.
- [moviefileinTOP](#movie-file-in--moviefileintop) — Plays an image or a movie file.
- [moviefileoutTOP](#movie-file-out--moviefileouttop) — Records its input to a movie file. Passes the picture through.
- [noiseTOP](#noise--noisetop) — Fractal value noise. Animate Translate Z to make it move.
- [nullTOP](#null--nulltop) — Pass-through. A stable name to reference and to view.
- [outTOP](#out--outtop) — This component's texture output.
- [rampTOP](#ramp--ramptop) — Linear or radial gradient between two colours.
- [rectangleTOP](#rectangle--rectangletop) — A rectangle, with rounded corners and an optional border.
- [renderTOP](#render--rendertop) — Draws Geometry components through a Camera.
- [resolutionTOP](#resolution--resolutiontop) — Resamples its input to an explicit resolution.
- [selectTOP](#select--selecttop) — This frame's output of another TOP, by path.
- [switchTOP](#switch--switchtop) — Select one of two inputs, optionally blending between them.
- [textTOP](#text--texttop) — Draws text, using a font file or the system's.
- [thresholdTOP](#threshold--thresholdtop) — Split the image in two at a level, with a soft edge.
- [toonTOP](#toon--toontop) — Cel shading: flatten the luminance into bands and ink the edges.
- [transformTOP](#transform--transformtop) — Translate, rotate and scale, with an extend mode.
- [videodeviceinTOP](#video-device-in--videodeviceintop) — Frames from a camera or capture device.
- [voronoiTOP](#voronoi--voronoitop) — Cellular noise, as flat cells, edges or a distance field.

---

## CHOP

### Analyze — `analyzeCHOP`

Reduce each channel to one number.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Function | `function` | `average` | `average` · `maximum` · `minimum` · `rms` · `sum` · `length` · `median` |

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

Plays an audio file, following the timeline.

Plays RIFF/WAVE — PCM in 8, 16, 24 or 32 bits, or 32-bit float — which is what a DAW bounces, decoded in-process with no external tool. Anything else — m4a, mp3, ogg, flac, or the soundtrack of a movie file — is decoded by `ffmpeg`, the same one Movie File In uses, so it plays if ffmpeg is installed and the node says so plainly if it is not.

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

### Beat — `beatCHOP`

A tempo clock: ramp, pulse, count and bar.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Tempo (BPM) | `tempo` | `120` | 20 … 300 |
| Beats Per Bar | `beatsperbar` | `4` | 1 … 32 |
| Sample Rate | `rate` | `60` | 1 … 48000 |

### Clock — `clockCHOP`

Wall-clock time, which keeps running when the timeline is paused.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Sample Rate | `rate` | `60` | 1 … 48000 |

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

### Cross — `crossCHOP`

Blend two inputs, with an equal-power option.

**Inputs:** a, b

| Parameter | Name | Default | Range |
|---|---|---|---|
| Cross | `cross` | `0.5` | 0 … 1 |
| Curve | `curve` | `linear` | `linear` · `equalpower` · `smooth` |

### DAT to CHOP — `dattochopCHOP`

Columns or rows of a table, as channels.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Channels From | `layout` | `columns` | `columns` · `rows` |
| First Row/Column Is Names | `names` | `true` |  |

### Delay — `delayCHOP`

Play a channel back later, with optional feedback.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Delay (s) | `delay` | `0.25` | 0 … 10 |
| Mix | `mix` | `1` | 0 … 1 |
| Feedback | `feedback` | `0` | 0 … 0.95 |

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

### Expression — `expressionCHOP`

Apply an expression to every sample. `v` is the sample.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Expression | `expr` | `v` |  |

### Filter — `filterCHOP`

Low-pass or moving-average smoothing.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Type | `type` | `lowpass` | `lowpass` · `movingaverage` |
| Width (s) | `width` | `0.2` | 0 … 4 |

### Hold — `holdCHOP`

Freeze input 1's value until input 2 fires.

**Inputs:** in, trigger

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Trigger Threshold | `threshold` | `0.5` | -10 … 10 |

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

### Limit — `limitCHOP`

Clamp, wrap, fold or quantise a channel.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Type | `type` | `clamp` | `clamp` · `loop` · `zigzag` · `quantise` |
| Minimum | `min` | `0` | -10 … 10 |
| Maximum | `max` | `1` | -10 … 10 |
| Quantise Step | `step` | `0.1` | 0 … 10 |

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

### Panel — `panelCHOP`

Panel widget values as channels.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Panel COMPs (paths) | `ops` | *(empty)* |  |

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

### Rename — `renameCHOP`

Rename channels by pattern.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Rename Channels | `from` | `*` |  |
| To | `to` | `chan[1-]` |  |

### Resample — `resampleCHOP`

Change how many samples a buffer has, keeping its shape.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Length (samples) | `length` | `64` | 1 … 8192 |
| Interpolate | `interp` | `linear` | `linear` · `nearest` |

### Select — `selectCHOP`

Pick and rename channels.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Channel Names | `channels` | `*` |  |
| Rename To | `rename` | *(empty)* |  |

### Shuffle — `shuffleCHOP`

Rearrange the channel/sample grid — transpose, reverse, sequence.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Method | `method` | `transpose` | `transpose` · `reverse` · `swapfirstlast` · `sequence` |

### Slope — `slopeCHOP`

The rate of change of a channel.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Units | `units` | `persecond` | `persecond` · `persample` |

### SOP to CHOP — `soptochopCHOP`

Point attributes as channels, one sample per point.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Attributes (P N uv Cd) | `attrs` | `P` |  |

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

### TOP to CHOP — `toptochopCHOP`

Pixels as channels. Reads last frame; costs a GPU sync.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Active | `active` | `true` |  |
| Read | `layout` | `rows` | `rows` · `columns` · `average` |
| Row / Column | `index` | `0` | 0 … 4096 |

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

### Button — `buttonCOMP`

A button on the output. Its state is the Value parameter.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Value | `value` | `0` | 0 … 1 |
| Mode | `mode` | `toggle` | `toggle` · `momentary` |
| X | `x` | `0.05` | 0 … 1 |
| Y | `y` | `0.05` | 0 … 1 |
| Width | `w` | `0.2` | 0 … 1 |
| Height | `h` | `0.08` | 0 … 1 |
| Label | `label` | *(empty)* |  |

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

### Field — `fieldCOMP`

An editable text field on the output.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Text | `text` | *(empty)* |  |
| X | `x` | `0.05` | 0 … 1 |
| Y | `y` | `0.05` | 0 … 1 |
| Width | `w` | `0.2` | 0 … 1 |
| Height | `h` | `0.08` | 0 … 1 |
| Label | `label` | *(empty)* |  |

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

### Slider — `sliderCOMP`

A fader on the output. Its position is the Value parameter.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Value | `value` | `0` |  |
| Minimum | `min` | `0` |  |
| Maximum | `max` | `1` |  |
| Orientation | `orientation` | `horizontal` | `horizontal` · `vertical` |
| X | `x` | `0.05` | 0 … 1 |
| Y | `y` | `0.05` | 0 … 1 |
| Width | `w` | `0.2` | 0 … 1 |
| Height | `h` | `0.08` | 0 … 1 |
| Label | `label` | *(empty)* |  |

---

## DAT

### CHOP Execute — `chopexecuteDAT`

Python callbacks when a watched channel changes or crosses a threshold.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Active | `active` | `true` |  |
| Watch CHOP | `chop` | *(empty)* |  |
| Channels | `channels` | `*` |  |
| On Threshold | `threshold` | `0.5` | -10 … 10 |
| Callbacks | `source` | *(multi-line)* |  |

### CHOP to DAT — `choptodatDAT`

Channels as a table of numbers.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Channels As | `layout` | `columns` | `columns` · `rows` |
| Include Names | `names` | `true` |  |
| Number Format | `format` | `%.6g` |  |

### Convert — `convertDAT`

Between a table and a block of text.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Convert To | `to` | `table` | `table` · `text` |
| Delimiter | `delimiter` | `tab` | `tab` · `comma` |

### Execute — `executeDAT`

Python callbacks at the start and end of a frame.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Active | `active` | `true` |  |
| Callbacks | `source` | *(multi-line)* |  |

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

### Parameter Execute — `parameterexecuteDAT`

Python callbacks when a watched parameter changes.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Active | `active` | `true` |  |
| Watch Operator | `op` | *(empty)* |  |
| Parameters | `parameters` | `*` |  |
| Callbacks | `source` | *(multi-line)* |  |

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

### Sort — `sortDAT`

Sort rows by a column, numerically or as text.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Column (name or index) | `column` | `0` |  |
| Order | `order` | `ascending` | `ascending` · `descending` |
| Compare As Numbers | `numeric` | `true` |  |
| First Row Is A Header | `header` | `true` |  |

### Substitute — `substituteDAT`

Fill $name placeholders from a two-column lookup table.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Template | `template` | *(empty)* |  |
| Substitute Every Cell Instead | `table` | `false` |  |

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

### Transpose — `transposeDAT`

Swap rows and columns.

**Inputs:** in

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

### Constant — `constantMAT`

Flat colour, unaffected by lights.

**Inputs:** color

| Parameter | Name | Default | Range |
|---|---|---|---|
| Color | `basecolor` | `[1.0, 1.0, 1.0, 1.0]` |  |
| Brightness | `emit` | `1` | 0 … 4 |

### PBR — `pbrMAT`

Base colour, metallic, roughness and emission, with an optional map.

**Inputs:** color

| Parameter | Name | Default | Range |
|---|---|---|---|
| Base Color | `basecolor` | `[0.8, 0.8, 0.85, 1.0]` |  |
| Metallic | `metallic` | `0` | 0 … 1 |
| Roughness | `roughness` | `0.4` | 0.02 … 1 |
| Emit | `emit` | `0` | 0 … 4 |

### Phong — `phongMAT`

Diffuse and a Blinn highlight, dialled by shininess.

**Inputs:** color

| Parameter | Name | Default | Range |
|---|---|---|---|
| Diffuse | `basecolor` | `[0.8, 0.8, 0.85, 1.0]` |  |
| Specular | `specular` | `0.5` | 0 … 4 |
| Shininess | `shininess` | `32` | 1 … 256 |
| Emit | `emit` | `0` | 0 … 4 |

### Point Sprite — `pointspriteMAT`

Draws every point as a camera-facing quad.

**Inputs:** color

| Parameter | Name | Default | Range |
|---|---|---|---|
| Color | `basecolor` | `[1.0, 1.0, 1.0, 1.0]` |  |
| Size (world units) | `size` | `0.1` | 0.001 … 4 |
| Brightness | `emit` | `1` | 0 … 4 |
| Round | `round` | `true` |  |

### Wireframe — `wireframeMAT`

Draws the edges rather than the faces.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Color | `basecolor` | `[0.6, 0.9, 1.0, 1.0]` |  |
| Brightness | `emit` | `1` | 0 … 4 |

---

## SOP

### Blend — `blendSOP`

Morph between two shapes by interpolating point positions.

Interpolating point positions is the whole trick, and it only works when the two shapes agree about which point is which. They almost never do, so the interesting parameter is Match Points, which is how the correspondence gets invented.

`stretch` walks input B proportionally — point 0 of a 100-point shape pairs with point 0 of a 500-point one, point 50 with point 250 — so both surfaces are traversed end to end and a morph between two different primitives moves every point. `index` pairs point *n* with point *n*, which is right when the two are the same topology deformed two ways, and wrong otherwise.

The output keeps input A's topology and point count. Blend is a deformation of A towards B, so at 1 you have A's connectivity holding B's shape; geometry whose triangles rewired themselves halfway through would be a cut, not a morph.

**Inputs:** a, b

| Parameter | Name | Default | Range |
|---|---|---|---|
| Blend | `blend` | `0` | 0 … 1 |
| Match Points | `match` | `stretch` | `stretch` · `index` |
| Blend Normals and Color | `attributes` | `true` |  |

### Box — `boxSOP`

A box with flat-shaded faces.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Size | `size` | `[1.0, 1.0, 1.0]` |  |
| Center | `center` | `[0.0, 0.0, 0.0]` |  |

### Circle — `circleSOP`

A disc, ring or arc — filled, or a line to copy along.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Radius | `radius` | `[1.0, 1.0]` |  |
| Divisions | `divisions` | `32` | 3 … 512 |
| Arc (degrees) | `arc` | `360` | 0 … 360 |
| Fill | `fill` | `true` |  |
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

### Torus — `torusSOP`

A torus.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Outer Radius | `radius1` | `1` | 0 … 8 |
| Inner Radius | `radius2` | `0.3` | 0 … 8 |
| Rows | `rows` | `24` | 3 … 256 |
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

### Tube — `tubeSOP`

A cylinder, cone or tapered tube, with optional caps.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Bottom Radius | `radius1` | `0.5` | 0 … 8 |
| Top Radius | `radius2` | `0.5` | 0 … 8 |
| Height | `height` | `2` | 0 … 16 |
| Columns | `columns` | `24` | 3 … 256 |
| Rows | `rows` | `1` | 1 … 128 |
| Caps | `caps` | `true` |  |
| Center | `center` | `[0.0, 0.0, 0.0]` |  |

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

### CHOP to TOP — `choptotopTOP`

Channels as pixels: a row per channel, a column per sample.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Channel Layout | `layout` | `mono` | `mono` · `rgba` |

### Chroma Key — `chromakeyTOP`

Key out one colour, matching on hue rather than brightness.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Key Colour | `key` | `[0.0, 1.0, 0.0, 1.0]` |  |
| Tolerance | `tolerance` | `0.15` | 0 … 1 |
| Softness | `softness` | `0.1` | 0 … 1 |
| Despill | `despill` | `1` | 0 … 1 |
| Replace Rather Than Cut | `replace` | `false` |  |
| Replacement | `replacement` | `[0.0, 0.0, 0.0, 1.0]` |  |

### Circle — `circleTOP`

An antialiased disc or ellipse.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Centre | `centre` | `[0.5, 0.5]` |  |
| Radius | `radius` | `[0.3, 0.3]` |  |
| Softness | `softness` | `0` | 0 … 1 |
| Correct Aspect | `aspect` | `true` |  |
| Fill | `fill` | `[1.0, 1.0, 1.0, 1.0]` |  |
| Background | `background` | `[0.0, 0.0, 0.0, 0.0]` |  |
| Resolution W | `resw` | `1280` | 1 … 4096 |
| Resolution H | `resh` | `720` | 1 … 4096 |

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

### Dither — `ditherTOP`

Quantise to few levels, with an ordered or noise dither.

Quantising alone gives flat bands. Dithering adds a pattern *before* the quantiser so the rounding error alternates between neighbouring pixels and the eye averages it back into the tone that was there — which is how two colours can look like a gradient.

Which pattern is the whole look: `bayer4` and `bayer8` are the ordered crosshatch of early games and newsprint, `noise` is closer to film grain, and `none` is hard posterisation with no dither at all. Pixel Size above 1 enlarges the matrix, which is what makes it read as chunky rather than as texture.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Levels | `levels` | `4` | 2 … 64 |
| Pattern | `pattern` | `bayer4` | `bayer4` · `bayer8` · `noise` · `none` |
| Strength | `strength` | `1` | 0 … 1 |
| Pixel Size | `scale` | `1` | 1 … 32 |
| Monochrome | `monochrome` | `false` |  |

### Edge — `edgeTOP`

Sobel edge detection.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Strength | `strength` | `1` | 0 … 8 |
| Width (px) | `width` | `1` | 0 … 16 |
| Direction | `direction` | `both` | `both` · `horizontal` · `vertical` |
| Keep Colour | `keepcolor` | `0` | 0 … 1 |
| Edge Colour | `color` | `[1.0, 1.0, 1.0, 1.0]` |  |

### Feedback — `feedbackTOP`

Last frame's output of the Target TOP.

Reads the target TOP's output from the **previous** frame. That is what lets a feedback loop exist without a cycle in the cook graph — the Select TOP, which reads the current frame, would be a cycle and is rejected.

Point `Target TOP` at the node whose output you want to feed back, wire this node's output into that chain, and put a Level TOP in between to decay it. Without the decay the loop saturates within a second.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Target TOP | `target` | *(empty)* |  |

### Flip — `flipTOP`

Mirror the image about an axis, or transpose it.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Flip Horizontally | `flipx` | `true` |  |
| Flip Vertically | `flipy` | `false` |  |
| Transpose | `transpose` | `false` |  |

### Flow — `flowTOP`

Advect the picture along a curl-noise field. Loop it for smoke.

One Flow TOP is a warp: every pixel reads from upstream of a curl-noise field, so the picture leans. The look people mean by *flow* — smoke, ink in water, drifting abstraction — is that warp **in a loop**, where each frame advects the last one a little further:

`source -> flow1 -> compositeTOP` with a Feedback TOP targeting the composite and wired back into it.

The field is the curl of a noise field rather than the noise itself, and that is not a detail. A curl is divergence-free: it swirls without compressing the image into a point or tearing a hole in it, which a plain noise vector field does within a second of being looped. Wire a TOP into the second input and turn on Steer From Input 2 to drive the flow with a picture — a Ramp for a wind direction, a camera difference for something that follows you.

**Inputs:** in, field

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Amount (px) | `amount` | `6` | 0 … 128 |
| Field Scale | `scale` | `3` | 0.1 … 32 |
| Field Speed | `speed` | `0.3` | 0 … 8 |
| Steer From Input 2 | `usefield` | `false` |  |
| Field Mix | `fieldmix` | `1` | 0 … 1 |

### GLSL — `glslTOP`

Your own shader, compiled live. WGSL or Shadertoy-style GLSL.

Write a fragment body; the boilerplate is supplied. In WGSL, `in.uv`, `U.time.x`, `U.res` and `sample0(uv)`/`sample1(uv)` are in scope, along with `U.p0..U.p3` from the four Uniform parameters. In GLSL a Shadertoy `mainImage` with `iTime` and `iResolution` runs unmodified, and the two inputs are `iChannel0`/`iChannel1` — sampled with `texture()`, and already declared, so declaring them yourself is an error. TouchDesigner's `sTD2DInputs` is not a name here.

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

### HSV Adjust — `hsvadjustTOP`

Hue, saturation and value, optionally over one band of the wheel.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Hue Shift | `hue` | `0` | -0.5 … 0.5 |
| Saturation | `saturation` | `1` | 0 … 4 |
| Value | `value` | `1` | 0 … 4 |
| Contrast | `contrast` | `1` | 0 … 4 |
| Range Centre | `rangecentre` | `0` | 0 … 1 |
| Range Width | `rangewidth` | `1` | 0 … 1 |

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

### Lookup — `lookupTOP`

Input 1's brightness reads a colour out of input 2.

**Inputs:** index, table

| Parameter | Name | Default | Range |
|---|---|---|---|
| Index By | `source` | `luminance` | `luminance` · `red` · `green` · `blue` · `alpha` |

### Math — `mathTOP`

Arithmetic on two inputs, per channel.

**Inputs:** a, b

| Parameter | Name | Default | Range |
|---|---|---|---|
| Operation | `operation` | `add` | `add` · `subtract` · `multiply` · `divide` · `minimum` · `maximum` · `difference` · `power` |
| Input 1 Gain | `gain1` | `[1.0, 1.0, 1.0, 1.0]` |  |
| Input 2 Gain | `gain2` | `[1.0, 1.0, 1.0, 1.0]` |  |
| Offset | `offset` | `[0.0, 0.0, 0.0, 0.0]` |  |

### Mirror — `mirrorTOP`

Fold the image onto itself, including a radial kaleidoscope.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Mode | `mode` | `horizontal` | `horizontal` · `vertical` · `quad` · `radial` |
| Segments | `segments` | `6` | 2 … 32 |
| Angle | `angle` | `0` | -180 … 180 |
| Centre | `centre` | `[0.5, 0.5]` |  |

### Movie File In — `moviefileinTOP`

Plays an image or a movie file.

Still images — PNG, JPEG, WebP, BMP, TGA, TIFF — are decoded in-process, with no external tool involved. Everything that moves goes through an `ffmpeg` subprocess, so mp4, mov, mkv, webm, avi and animated GIF all play if ffmpeg is installed. It is looked for on the PATH and in the usual install directories, so a double-clicked app finds it too, and the node says so plainly when it cannot.

Playback is a function of the timeline, not a private play head: the frame shown at time `t` is always the file at `t × speed`. Scrubbing the timeline scrubs the movie, the loop range loops it, and a headless `otd render` writes exactly the frames the editor showed. Seeking backwards restarts the decode at the new time, which is why a scrub is a real scrub rather than a rewind.

The picture's own size wins: `Fallback W`/`H` is only what to show before the first frame arrives, or if the file cannot be read.

`Speed` tracks the timeline exactly at 1× and 2×. Past that it falls behind, and the reason is the transport rather than the decoder: frames cross a pipe as raw RGBA, so 1138×640 is 2.9 MB each, and asking for 8× is asking for well over a gigabyte a second through it. Scrubbing is unaffected — a jump seeks rather than races.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| File | `file` | *(empty)* |  |
| Play | `play` | `loop` | `loop` · `once` · `hold` |
| Speed | `speed` | `1` | -4 … 4 |
| Fallback W | `resw` | `1280` | 1 … 16384 |
| Fallback H | `resh` | `720` | 1 … 16384 |

### Movie File Out — `moviefileoutTOP`

Records its input to a movie file. Passes the picture through.

**Inputs:** in

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| File | `file` | `out.mp4` |  |
| Record | `record` | `false` |  |
| Frame Rate | `fps` | `60` | 1 … 240 |
| Codec | `codec` | `h264` | `h264` · `h265` · `prores` |
| Quality | `quality` | `75` | 0 … 100 |

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

### Rectangle — `rectangleTOP`

A rectangle, with rounded corners and an optional border.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Centre | `centre` | `[0.5, 0.5]` |  |
| Size | `size` | `[0.5, 0.5]` |  |
| Softness | `softness` | `0` | 0 … 0.5 |
| Corner Radius | `corner` | `0` | 0 … 0.5 |
| Border Width | `border` | `0` | 0 … 0.5 |
| Border Grey | `bordercolor` | `0` | 0 … 1 |
| Fill | `fill` | `[1.0, 1.0, 1.0, 1.0]` |  |
| Background | `background` | `[0.0, 0.0, 0.0, 0.0]` |  |
| Resolution W | `resw` | `1280` | 1 … 4096 |
| Resolution H | `resh` | `720` | 1 … 4096 |

### Render — `renderTOP`

Draws Geometry components through a Camera.

Depth output is the same draw with the shading skipped, writing distance from the camera as grey — near white, far black, so an unwritten background is black and the result multiplies straight into a composite as a mask.

It is metric distance between Depth Near and Depth Far, not the depth buffer's own value. That one is `1/z` shaped by the projection and puts almost all of its precision in the first few units, so everything past arm's length reads the same white and anything downstream sees a flat card. Set the near and far to the part of the scene you care about; they are what decide whether the pass looks like anything.

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
| Output | `output` | `color` | `color` · `depth` |
| Depth Near | `depthnear` | `0.1` | 0 … 1000 |
| Depth Far | `depthfar` | `20` | 0.01 … 10000 |
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

### Text — `textTOP`

Draws text, using a font file or the system's.

*No inputs — this is a generator.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Text | `text` | `OpenTouchDesigner` |  |
| Font File | `font` | *(empty)* |  |
| Size (px) | `size` | `48` | 4 … 512 |
| Line Spacing | `linespacing` | `1.2` | 0.5 … 4 |
| Horizontal Align | `halign` | `centre` | `left` · `centre` · `right` |
| Vertical Align | `valign` | `centre` | `top` · `centre` · `bottom` |
| Word Wrap | `wrap` | `true` |  |
| Color | `color` | `[1.0, 1.0, 1.0, 1.0]` |  |
| Background | `background` | `[0.0, 0.0, 0.0, 0.0]` |  |
| Resolution W | `resw` | `1280` | 1 … 4096 |
| Resolution H | `resh` | `720` | 1 … 4096 |

### Threshold — `thresholdTOP`

Split the image in two at a level, with a soft edge.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Threshold | `threshold` | `0.5` | 0 … 1 |
| Softness | `softness` | `0.02` | 0 … 1 |
| Compare | `source` | `luminance` | `luminance` · `maximum` · `alpha` |
| Invert | `invert` | `false` |  |
| Below Colour | `below` | `[0.0, 0.0, 0.0, 1.0]` |  |
| Above Colour | `above` | `[1.0, 1.0, 1.0, 1.0]` |  |

### Toon — `toonTOP`

Cel shading: flatten the luminance into bands and ink the edges.

Cel shading is two ideas that only look like one, and this operator is both because they want to share a threshold — ink drawn where the bands already change is invisible, and ink drawn anywhere else is a mess.

Posterising the *luminance*, and only the luminance, collapses a smooth gradient into flat steps while leaving hue alone, which is what keeps the result looking painted rather than colour-crushed. Flattening luminance washes colour out, so Saturation defaults above 1 to put it back. The ink is a Sobel edge multiplied over the top: multiplied rather than added, because added lines glow, which is the opposite of a drawn line.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Bands | `bands` | `4` | 2 … 32 |
| Ink Strength | `edge` | `2` | 0 … 16 |
| Ink Width (px) | `edgewidth` | `1` | 0 … 8 |
| Saturation | `saturation` | `1.3` | 0 … 4 |
| Ink Color | `inkcolor` | `[0.0, 0.0, 0.0, 1.0]` |  |

### Transform — `transformTOP`

Translate, rotate and scale, with an extend mode.

**Inputs:** in

| Parameter | Name | Default | Range |
|---|---|---|---|
| Translate | `translate` | `[0.0, 0.0]` |  |
| Rotate | `rotate` | `0` | -180 … 180 |
| Scale | `scale` | `[1.0, 1.0]` |  |
| Extend | `extend` | `zero` | `zero` · `hold` · `repeat` · `mirror` |

### Video Device In — `videodeviceinTOP`

Frames from a camera or capture device.

A camera, through ffmpeg. `Requested W`/`H` and the frame rate are requests rather than commands — a capture device only does the modes it does — so they are negotiated to the nearest mode the device actually reports, and the node shows what the device said if none will work.

On macOS the first use raises the system camera-permission prompt, and nothing arrives until it is granted. That prompt only appears for a properly signed bundle: macOS reads the usage description out of a *sealed* `Info.plist`, and a build whose bundle was never codesigned has none to read, so it is refused without ever being asked about. A refused camera is silent — the session opens, no error is printed and no frame ever comes — so the node says so itself after a few seconds rather than staying black with nothing to go on. The picture is always the newest frame decoded, so latency stays at about one frame; pausing the timeline pauses this like everything else in the network.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Device (blank = default) | `device` | *(empty)* |  |
| Requested W | `resw` | `1280` | 1 … 16384 |
| Requested H | `resh` | `720` | 1 … 16384 |
| Requested Frame Rate | `fps` | `30` | 1 … 240 |
| Active | `active` | `true` |  |

### Voronoi — `voronoiTOP`

Cellular noise, as flat cells, edges or a distance field.

Every pixel finds the nearest of a set of points scattered one per cell. What you do with that answer is `output`: `cells` flat-fills each region, which is stained glass; `edges` draws where two regions meet, which is cracked glass and antialiases for free because it is the *difference* between the two nearest distances; `distance` is the raw field, and is what you want when this feeds a Displace TOP rather than being looked at.

A generator, so nothing is wired in. Jitter at 0 is a regular grid and at 1 the points wander on their own phases, so the pattern boils rather than sliding as one sheet.

*No inputs — this is a generator.*

*Time dependent: cooks every frame, and everything downstream of it does too.*

| Parameter | Name | Default | Range |
|---|---|---|---|
| Cells | `scale` | `8` | 1 … 128 |
| Speed | `speed` | `0.4` | 0 … 8 |
| Jitter | `jitter` | `1` | 0 … 1 |
| Output | `output` | `cells` | `cells` · `edges` · `distance` |
| Color 1 | `color1` | `[0.0, 0.0, 0.0, 1.0]` |  |
| Color 2 | `color2` | `[1.0, 1.0, 1.0, 1.0]` |  |
| Resolution W | `resw` | `1280` | 1 … 4096 |
| Resolution H | `resh` | `720` | 1 … 4096 |

