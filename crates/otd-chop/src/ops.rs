//! The CHOP operator table.
//!
//! Same shape as the TOP table in `otd-gpu`: one cook function and one entry
//! per operator, so breadth stays cheap (PLAN.md §6).
//!
//! Two things differ from TOPs. CHOPs are **stateful** — a Lag, a Speed, a
//! Trigger all carry values across frames — so every node gets an [`OpState`]
//! scratchpad. And they are **time sliced**: a generator emits the samples
//! covering the elapsed frame interval rather than a fixed buffer.

use std::sync::OnceLock;

use otd_core::indexmap::IndexMap;
use otd_core::{
    Connector, CookContext, Crossing, Crossings, EvalContext, Family, Node, OpDef, OpRegistry,
    Param, Value,
};

use crate::data::{CONTROL_RATE, Channel, ChopData, slice_len, slice_times};
use crate::io::Io;

/// Per-node scratch that survives between cooks.
#[derive(Clone, Debug, Default)]
pub struct OpState {
    /// Continuous values — filter memory, accumulators, envelope levels.
    pub f: Vec<f32>,
    /// Discrete values — envelope stage, counters.
    pub i: Vec<i64>,
    /// The previous frame's last input sample, for edge detection.
    pub prev: Vec<f32>,
}

impl OpState {
    fn fit(&mut self, n: usize) {
        self.f.resize(n, 0.0);
        self.i.resize(n, 0);
        self.prev.resize(n, 0.0);
    }
}

pub struct ChopCtx<'a> {
    pub node: &'a Node,
    pub eval: &'a EvalContext<'a>,
    pub time: &'a CookContext,
    /// One entry per declared input; unconnected inputs are empty.
    pub inputs: Vec<ChopData>,
    /// Inputs that came from another family, for the converter operators.
    /// Same indexing as `inputs`; `None` for every ordinary CHOP input.
    pub foreign: Crossings,
    pub state: &'a mut OpState,
    pub io: &'a mut Io,
    pub path: &'a str,
}

impl ChopCtx<'_> {
    pub fn val(&self, key: &str) -> Value {
        self.node
            .param(key)
            .map(|p| p.eval(self.eval))
            .unwrap_or(Value::Float(0.0))
    }
    pub fn f(&self, key: &str) -> f32 {
        self.val(key).as_f32()
    }
    pub fn i(&self, key: &str) -> i64 {
        self.val(key).as_i64()
    }
    pub fn b(&self, key: &str) -> bool {
        self.val(key).as_bool()
    }
    pub fn s(&self, key: &str) -> String {
        self.val(key).as_str()
    }
    pub fn menu(&self, key: &str) -> usize {
        let Some(param) = self.node.param(key) else {
            return 0;
        };
        let chosen = param.eval(self.eval).as_str();
        param
            .menu
            .as_ref()
            .and_then(|items| items.iter().position(|i| *i == chosen))
            .unwrap_or(0)
    }
    /// The foreign-family input in a slot, if one is wired there.
    pub fn foreign(&self, index: usize) -> Option<&Crossing> {
        self.foreign.get(index).and_then(|c| c.as_ref())
    }
    pub fn input(&self, index: usize) -> &ChopData {
        static EMPTY: OnceLock<ChopData> = OnceLock::new();
        self.inputs
            .get(index)
            .unwrap_or_else(|| EMPTY.get_or_init(ChopData::empty))
    }
    /// The sample rate a generator should run at.
    pub fn rate(&self) -> f64 {
        let r = self.f("rate") as f64;
        if r > 0.0 { r } else { CONTROL_RATE }
    }
}

pub struct ChopSpec {
    pub def: OpDef,
    pub cook: fn(&mut ChopCtx) -> ChopData,
}

macro_rules! params {
    ($($key:expr => $param:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m: IndexMap<String, Param> = IndexMap::new();
        $( m.insert($key.into(), $param); )*
        m
    }};
}

fn no_params() -> IndexMap<String, Param> {
    params! {}
}

fn with_rate(mut m: IndexMap<String, Param>) -> IndexMap<String, Param> {
    m.insert(
        "rate".into(),
        Param::float(CONTROL_RATE)
            .with_label("Sample Rate")
            .with_range(1.0, 48000.0),
    );
    m
}

/// Copy an input's shape, applying `f` to every sample of every channel.
fn map_samples(input: &ChopData, mut f: impl FnMut(usize, usize, f32) -> f32) -> ChopData {
    let channels = input
        .channels
        .iter()
        .enumerate()
        .map(|(ci, c)| Channel {
            name: c.name.clone(),
            samples: c
                .samples
                .iter()
                .enumerate()
                .map(|(si, v)| f(ci, si, *v))
                .collect(),
        })
        .collect();
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// ------------------------------------------------------------- constant

fn params_constant() -> IndexMap<String, Param> {
    with_rate(params! {
        "channels" => Param::int(1).with_label("Channels").with_range(1.0, 4.0),
        "name" => Param::str("chan").with_label("Name"),
        "value0" => Param::float(0.0).with_label("Value 1"),
        "value1" => Param::float(0.0).with_label("Value 2"),
        "value2" => Param::float(0.0).with_label("Value 3"),
        "value3" => Param::float(0.0).with_label("Value 4"),
    })
}

fn cook_constant(c: &mut ChopCtx) -> ChopData {
    let rate = c.rate();
    let n = slice_len(rate, c.time);
    let count = c.i("channels").clamp(1, 4) as usize;
    let base = c.s("name");
    let channels = (0..count)
        .map(|i| Channel::constant(format!("{base}{}", i + 1), c.f(&format!("value{i}")), n))
        .collect();
    ChopData::new(channels, rate, true)
}

// ------------------------------------------------------------------ LFO

fn params_lfo() -> IndexMap<String, Param> {
    with_rate(params! {
        "type" => Param::menu("sine", &["sine", "triangle", "ramp", "square", "pulse"])
            .with_label("Type"),
        "frequency" => Param::float(1.0).with_label("Frequency").with_range(0.0, 20.0),
        "amplitude" => Param::float(1.0).with_label("Amplitude").with_range(0.0, 4.0),
        "offset" => Param::float(0.0).with_label("Offset").with_range(-2.0, 2.0),
        "phase" => Param::float(0.0).with_label("Phase").with_range(0.0, 1.0),
        "pulsewidth" => Param::float(0.5).with_label("Pulse Width").with_range(0.0, 1.0),
        "name" => Param::str("lfo").with_label("Channel Name"),
    })
}

fn wave(kind: usize, phase: f32, pulse_width: f32) -> f32 {
    let p = phase - phase.floor();
    match kind {
        // triangle
        1 => 1.0 - 4.0 * (p - 0.5).abs(),
        // ramp
        2 => p * 2.0 - 1.0,
        // square
        3 => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        // pulse
        4 => {
            if p < pulse_width.clamp(0.0, 1.0) {
                1.0
            } else {
                -1.0
            }
        }
        _ => (p * std::f32::consts::TAU).sin(),
    }
}

fn cook_lfo(c: &mut ChopCtx) -> ChopData {
    let rate = c.rate();
    let n = slice_len(rate, c.time);
    let (kind, freq, amp, offset, phase, pw) = (
        c.menu("type"),
        c.f("frequency"),
        c.f("amplitude"),
        c.f("offset"),
        c.f("phase"),
        c.f("pulsewidth"),
    );
    let samples = slice_times(c.time, n)
        .map(|t| wave(kind, t as f32 * freq + phase, pw) * amp + offset)
        .collect();
    ChopData::new(vec![Channel::new(c.s("name"), samples)], rate, true)
}

// ------------------------------------------------------------ animation

/// The starting keys in a new Animation CHOP: a one-second ramp, so the
/// operator animates something the moment it is created.
const DEFAULT_KEYS: &str = "\
# channel  time  value  interpolation
chan1      0     0      smooth
chan1      1     1      smooth
";

fn params_animation() -> IndexMap<String, Param> {
    with_rate(params! {
        "keys" => Param::str(DEFAULT_KEYS).with_label("Keys"),
        "play" => Param::menu("timeline", &["timeline", "loop", "hold"]).with_label("Play"),
        "speed" => Param::float(1.0).with_label("Speed").with_range(-4.0, 4.0),
        "offset" => Param::float(0.0).with_label("Offset").with_range(-60.0, 60.0),
    })
}

fn cook_animation(c: &mut ChopCtx) -> ChopData {
    let rate = c.rate();
    let n = slice_len(rate, c.time);
    let (curves, _) = crate::anim::Curves::parse(&c.s("keys"));

    let mode = c.menu("play");
    let (speed, offset) = (c.f("speed") as f64, c.f("offset") as f64);
    // `loop` needs the keyed span; without keys there is nothing to wrap into
    // and the curve is flat anyway.
    let span = curves.range().filter(|(lo, hi)| hi > lo);

    let at = |t: f64| -> f64 {
        let t = t * speed + offset;
        match (mode, span) {
            // 1 == loop. Wrapping with rem_euclid rather than `%` so it works
            // going backwards too, which a negative speed does.
            (1, Some((lo, hi))) => lo + (t - lo).rem_euclid(hi - lo),
            _ => t,
        }
    };

    // One sample per slice sample, so an exported parameter reads the value at
    // *this* instant and a downstream filter sees a continuous signal — the
    // same time-slicing contract every other generator here keeps.
    let times: Vec<f64> = slice_times(c.time, n).map(at).collect();
    let channels = curves
        .0
        .iter()
        .map(|(name, curve)| {
            Channel::new(
                name.clone(),
                times.iter().map(|t| curve.sample(*t)).collect(),
            )
        })
        .collect::<Vec<_>>();

    // A CHOP with no channels at all is awkward downstream; an animation with
    // no keys yet reports one silent channel rather than nothing.
    let channels = if channels.is_empty() {
        vec![Channel::new("chan1".to_string(), vec![0.0; n])]
    } else {
        channels
    };
    ChopData::new(channels, rate, true)
}

// ---------------------------------------------------------------- noise

fn params_noise() -> IndexMap<String, Param> {
    with_rate(params! {
        "type" => Param::menu("smooth", &["smooth", "random"]).with_label("Type"),
        "channels" => Param::int(1).with_label("Channels").with_range(1.0, 8.0),
        "name" => Param::str("noise").with_label("Name"),
        "period" => Param::float(1.0).with_label("Period").with_range(0.01, 10.0),
        "amplitude" => Param::float(1.0).with_label("Amplitude").with_range(0.0, 4.0),
        "offset" => Param::float(0.0).with_label("Offset").with_range(-2.0, 2.0),
        "seed" => Param::float(0.0).with_label("Seed"),
    })
}

fn hash1(x: i64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9e3779b97f4a7c15);
    h ^= h >> 29;
    h = h.wrapping_mul(0xbf58476d1ce4e5b9);
    h ^= h >> 32;
    (h as u32 as f64 / u32::MAX as f64) as f32 * 2.0 - 1.0
}

fn value_noise(t: f32, seed: i64) -> f32 {
    let i = t.floor();
    let f = t - i;
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash1(i as i64 + seed * 7919);
    let b = hash1(i as i64 + 1 + seed * 7919);
    a + (b - a) * u
}

fn cook_noise(c: &mut ChopCtx) -> ChopData {
    let rate = c.rate();
    let n = slice_len(rate, c.time);
    let (kind, count, period, amp, offset, seed) = (
        c.menu("type"),
        c.i("channels").clamp(1, 8) as usize,
        c.f("period").max(1e-4),
        c.f("amplitude"),
        c.f("offset"),
        c.f("seed") as i64,
    );
    let base = c.s("name");
    let times: Vec<f64> = slice_times(c.time, n).collect();
    let channels = (0..count)
        .map(|ch| {
            let samples = times
                .iter()
                .map(|t| {
                    let x = (*t as f32) / period;
                    let v = if kind == 1 {
                        // Uncorrelated per sample.
                        hash1((x * 1_000_000.0) as i64 + seed + ch as i64 * 104_729)
                    } else {
                        value_noise(x, seed + ch as i64 * 31)
                    };
                    v * amp + offset
                })
                .collect();
            Channel::new(format!("{base}{}", ch + 1), samples)
        })
        .collect();
    ChopData::new(channels, rate, true)
}

// -------------------------------------------------------------- pattern

fn params_pattern() -> IndexMap<String, Param> {
    params! {
        "type" => Param::menu("ramp", &["ramp", "sine", "triangle", "square", "gaussian"])
            .with_label("Type"),
        "length" => Param::int(64).with_label("Length").with_range(1.0, 4096.0),
        "periods" => Param::float(1.0).with_label("Periods").with_range(0.1, 32.0),
        "amplitude" => Param::float(1.0).with_label("Amplitude").with_range(0.0, 4.0),
        "offset" => Param::float(0.0).with_label("Offset").with_range(-2.0, 2.0),
        "phase" => Param::float(0.0).with_label("Phase").with_range(0.0, 1.0),
        "name" => Param::str("chan1").with_label("Channel Name"),
    }
}

fn cook_pattern(c: &mut ChopCtx) -> ChopData {
    let len = c.i("length").clamp(1, 4096) as usize;
    let (kind, periods, amp, offset, phase) = (
        c.menu("type"),
        c.f("periods"),
        c.f("amplitude"),
        c.f("offset"),
        c.f("phase"),
    );
    let samples = (0..len)
        .map(|i| {
            let x = i as f32 / len as f32;
            let p = x * periods + phase;
            let v = match kind {
                0 => p - p.floor(),
                4 => {
                    let g = (x - 0.5) * 6.0;
                    (-0.5 * g * g).exp()
                }
                other => wave(other, p, 0.5),
            };
            v * amp + offset
        })
        .collect();
    // Not time sliced: a Pattern CHOP is a fixed buffer, a lookup table.
    ChopData::new(
        vec![Channel::new(c.s("name"), samples)],
        CONTROL_RATE,
        false,
    )
}

// ----------------------------------------------------------------- math

fn params_math() -> IndexMap<String, Param> {
    params! {
        "combine" => Param::menu(
            "add",
            &["add", "subtract", "multiply", "divide", "maximum", "minimum", "average"],
        ).with_label("Combine Inputs"),
        "gain" => Param::float(1.0).with_label("Gain").with_range(-4.0, 4.0),
        "offset" => Param::float(0.0).with_label("Offset").with_range(-4.0, 4.0),
        "clamp" => Param::bool(false).with_label("Clamp"),
        "clampmin" => Param::float(0.0).with_label("Clamp Min"),
        "clampmax" => Param::float(1.0).with_label("Clamp Max"),
    }
}

fn combine(op: usize, a: f32, b: f32) -> f32 {
    match op {
        1 => a - b,
        2 => a * b,
        3 => {
            if b == 0.0 {
                0.0
            } else {
                a / b
            }
        }
        4 => a.max(b),
        5 => a.min(b),
        6 => (a + b) * 0.5,
        _ => a + b,
    }
}

fn cook_math(c: &mut ChopCtx) -> ChopData {
    let op = c.menu("combine");
    let (gain, offset) = (c.f("gain"), c.f("offset"));
    let (do_clamp, lo, hi) = (c.b("clamp"), c.f("clampmin"), c.f("clampmax"));

    let a = c.input(0).clone();
    let b = c.input(1).clone();
    let len = a.num_samples().max(b.num_samples()).max(1);

    let channel_count = a.num_channels().max(b.num_channels());
    let mut channels = Vec::with_capacity(channel_count);
    for ci in 0..channel_count {
        // A single-channel input broadcasts across a multi-channel one, which
        // is what makes `noise * one lfo` do the obvious thing.
        let pick = |d: &ChopData, ci: usize| -> Option<Channel> {
            if d.num_channels() == 1 {
                d.nth(0).cloned()
            } else {
                d.nth(ci).cloned()
            }
        };
        let ca = pick(&a, ci);
        let cb = pick(&b, ci);
        let name = ca
            .as_ref()
            .or(cb.as_ref())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("chan{}", ci + 1));

        let sample_at = |c: &Option<Channel>, i: usize, default: f32| -> f32 {
            match c {
                Some(c) if !c.samples.is_empty() => c.samples[i.min(c.samples.len() - 1)],
                _ => default,
            }
        };
        // Identity element, so a single wired input passes through unchanged.
        let identity = match op {
            2 | 3 => 1.0,
            4 => f32::NEG_INFINITY,
            5 => f32::INFINITY,
            _ => 0.0,
        };
        let samples = (0..len)
            .map(|i| {
                let va = sample_at(&ca, i, identity);
                let vb = sample_at(&cb, i, identity);
                let combined = if cb.is_none() {
                    va
                } else if ca.is_none() {
                    vb
                } else {
                    combine(op, va, vb)
                };
                let mut v = combined * gain + offset;
                if do_clamp {
                    v = v.clamp(lo.min(hi), hi.max(lo));
                }
                v
            })
            .collect();
        channels.push(Channel::new(name, samples));
    }
    let rate = if a.sample_rate > 0.0 {
        a.sample_rate
    } else {
        b.sample_rate.max(CONTROL_RATE)
    };
    ChopData::new(channels, rate, a.time_sliced || b.time_sliced)
}

// ------------------------------------------------------------------ lag

fn params_lag() -> IndexMap<String, Param> {
    params! {
        "lagup" => Param::float(0.1).with_label("Lag Up (s)").with_range(0.0, 2.0),
        "lagdown" => Param::float(0.1).with_label("Lag Down (s)").with_range(0.0, 2.0),
    }
}

/// One-pole coefficient for a time constant, per sample.
fn lag_alpha(tau: f32, sample_rate: f64) -> f32 {
    if tau <= 1e-5 {
        return 1.0;
    }
    let dt = 1.0 / sample_rate.max(1.0) as f32;
    1.0 - (-dt / tau).exp()
}

fn cook_lag(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let rate = input.sample_rate.max(1.0);
    let (up, down) = (c.f("lagup"), c.f("lagdown"));
    let (a_up, a_down) = (lag_alpha(up, rate), lag_alpha(down, rate));
    c.state.fit(input.num_channels());

    let mut channels = Vec::with_capacity(input.num_channels());
    for (ci, ch) in input.channels.iter().enumerate() {
        let mut y = c.state.f[ci];
        let samples: Vec<f32> = ch
            .samples
            .iter()
            .map(|x| {
                let alpha = if *x > y { a_up } else { a_down };
                y += (x - y) * alpha;
                y
            })
            .collect();
        c.state.f[ci] = y;
        channels.push(Channel::new(ch.name.clone(), samples));
    }
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// --------------------------------------------------------------- filter

fn params_filter() -> IndexMap<String, Param> {
    params! {
        "type" => Param::menu("lowpass", &["lowpass", "movingaverage"]).with_label("Type"),
        "width" => Param::float(0.2).with_label("Width (s)").with_range(0.0, 4.0),
    }
}

fn cook_filter(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let rate = input.sample_rate.max(1.0);
    let kind = c.menu("type");
    let width = c.f("width").max(0.0);
    c.state.fit(input.num_channels());

    if kind == 1 {
        // Moving average over the window, using the running value as the
        // history that predates this slice.
        let window = ((width as f64 * rate).round() as usize).max(1);
        let mut channels = Vec::with_capacity(input.num_channels());
        for (ci, ch) in input.channels.iter().enumerate() {
            let mut history = vec![c.state.f[ci]; window];
            let mut cursor = 0usize;
            let samples: Vec<f32> = ch
                .samples
                .iter()
                .map(|x| {
                    history[cursor] = *x;
                    cursor = (cursor + 1) % window;
                    history.iter().sum::<f32>() / window as f32
                })
                .collect();
            c.state.f[ci] = ch.last();
            channels.push(Channel::new(ch.name.clone(), samples));
        }
        return ChopData::new(channels, input.sample_rate, input.time_sliced);
    }

    let alpha = lag_alpha(width, rate);
    let mut channels = Vec::with_capacity(input.num_channels());
    for (ci, ch) in input.channels.iter().enumerate() {
        let mut y = c.state.f[ci];
        let samples: Vec<f32> = ch
            .samples
            .iter()
            .map(|x| {
                y += (x - y) * alpha;
                y
            })
            .collect();
        c.state.f[ci] = y;
        channels.push(Channel::new(ch.name.clone(), samples));
    }
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// ---------------------------------------------------------------- logic

fn params_logic() -> IndexMap<String, Param> {
    params! {
        "convert" => Param::menu(
            "nonzero",
            &["off", "nonzero", "greater", "less", "equal"],
        ).with_label("Convert"),
        "threshold" => Param::float(0.5).with_label("Threshold"),
        "invert" => Param::bool(false).with_label("Invert"),
        "combine" => Param::menu("off", &["off", "and", "or", "xor"]).with_label("Combine Channels"),
    }
}

fn cook_logic(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let (convert, threshold, invert, combine_op) = (
        c.menu("convert"),
        c.f("threshold"),
        c.b("invert"),
        c.menu("combine"),
    );
    let mut out = map_samples(&input, |_, _, v| {
        let on = match convert {
            0 => return v,
            2 => v > threshold,
            3 => v < threshold,
            4 => (v - threshold).abs() < 1e-6,
            _ => v != 0.0,
        };
        let on = on != invert;
        if on { 1.0 } else { 0.0 }
    });

    if combine_op > 0 && out.num_channels() > 1 {
        let len = out.num_samples();
        let samples = (0..len)
            .map(|i| {
                let mut bits = out.channels.iter().map(|c| c.samples[i] != 0.0);
                let result = match combine_op {
                    1 => bits.all(|b| b),
                    2 => bits.any(|b| b),
                    _ => bits.fold(false, |a, b| a ^ b),
                };
                if result { 1.0 } else { 0.0 }
            })
            .collect();
        out = ChopData::new(
            vec![Channel::new("logic", samples)],
            out.sample_rate,
            out.time_sliced,
        );
    }
    out
}

// -------------------------------------------------------------- trigger

fn params_trigger() -> IndexMap<String, Param> {
    params! {
        "threshold" => Param::float(0.5).with_label("Threshold"),
        "attack" => Param::float(0.05).with_label("Attack (s)").with_range(0.0, 2.0),
        "decay" => Param::float(0.2).with_label("Decay (s)").with_range(0.0, 4.0),
        "sustain" => Param::float(0.0).with_label("Sustain Level").with_range(0.0, 1.0),
        "release" => Param::float(0.3).with_label("Release (s)").with_range(0.0, 4.0),
    }
}

const STAGE_IDLE: i64 = 0;
const STAGE_ATTACK: i64 = 1;
const STAGE_SUSTAIN: i64 = 2;
const STAGE_RELEASE: i64 = 3;

fn cook_trigger(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let rate = input.sample_rate.max(1.0);
    let dt = 1.0 / rate as f32;
    let (threshold, attack, decay, sustain, release) = (
        c.f("threshold"),
        c.f("attack").max(1e-4),
        c.f("decay").max(1e-4),
        c.f("sustain").clamp(0.0, 1.0),
        c.f("release").max(1e-4),
    );
    c.state.fit(input.num_channels());

    let mut channels = Vec::with_capacity(input.num_channels());
    for (ci, ch) in input.channels.iter().enumerate() {
        let mut level = c.state.f[ci];
        let mut stage = c.state.i[ci];
        let mut prev = c.state.prev[ci];
        let samples: Vec<f32> = ch
            .samples
            .iter()
            .map(|x| {
                // Rising edge through the threshold starts the envelope.
                if prev <= threshold && *x > threshold {
                    stage = STAGE_ATTACK;
                } else if prev > threshold && *x <= threshold && stage != STAGE_IDLE {
                    stage = STAGE_RELEASE;
                }
                prev = *x;

                match stage {
                    STAGE_ATTACK => {
                        level += dt / attack;
                        if level >= 1.0 {
                            level = 1.0;
                            stage = STAGE_SUSTAIN;
                        }
                    }
                    STAGE_SUSTAIN => {
                        if level > sustain {
                            level -= dt / decay * (1.0 - sustain).max(1e-6);
                            level = level.max(sustain);
                        }
                    }
                    STAGE_RELEASE => {
                        level -= dt / release;
                        if level <= 0.0 {
                            level = 0.0;
                            stage = STAGE_IDLE;
                        }
                    }
                    _ => {}
                }
                level
            })
            .collect();
        c.state.f[ci] = level;
        c.state.i[ci] = stage;
        c.state.prev[ci] = prev;
        channels.push(Channel::new(ch.name.clone(), samples));
    }
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// ---------------------------------------------------------------- timer

fn params_timer() -> IndexMap<String, Param> {
    with_rate(params! {
        "length" => Param::float(10.0).with_label("Length (s)").with_range(0.1, 600.0),
        "cycle" => Param::bool(true).with_label("Cycle"),
        "play" => Param::bool(true).with_label("Play"),
    })
}

fn cook_timer(c: &mut ChopCtx) -> ChopData {
    let rate = c.rate();
    let n = slice_len(rate, c.time);
    let dt = (c.time.dt / n as f64) as f32;
    let (length, cycle, play) = (c.f("length").max(1e-3), c.b("cycle"), c.b("play"));
    c.state.fit(1);

    let mut elapsed = c.state.f[0];
    let mut cycles = c.state.i[0];
    let mut done = 0.0f32;

    let (mut fraction, mut seconds, mut cyc, mut don) = (
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
    );
    for _ in 0..n {
        if play {
            elapsed += dt;
            if elapsed >= length {
                if cycle {
                    elapsed -= length;
                    cycles += 1;
                } else {
                    elapsed = length;
                    done = 1.0;
                }
            }
        }
        fraction.push(elapsed / length);
        seconds.push(elapsed);
        cyc.push(cycles as f32);
        don.push(done);
    }
    c.state.f[0] = elapsed;
    c.state.i[0] = cycles;

    ChopData::new(
        vec![
            Channel::new("timer_fraction", fraction),
            Channel::new("timer_seconds", seconds),
            Channel::new("cycles", cyc),
            Channel::new("done", don),
        ],
        rate,
        true,
    )
}

// ---------------------------------------------------------------- speed

fn params_speed() -> IndexMap<String, Param> {
    params! {
        "gain" => Param::float(1.0).with_label("Gain").with_range(-4.0, 4.0),
        "initial" => Param::float(0.0).with_label("Initial Value"),
        "reset" => Param::bool(false).with_label("Reset"),
        "limit" => Param::menu("off", &["off", "clamp", "wrap"]).with_label("Limit"),
        "min" => Param::float(0.0).with_label("Min"),
        "max" => Param::float(1.0).with_label("Max"),
    }
}

fn cook_speed(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let rate = input.sample_rate.max(1.0);
    let dt = 1.0 / rate as f32;
    let (gain, initial, reset, limit, lo, hi) = (
        c.f("gain"),
        c.f("initial"),
        c.b("reset"),
        c.menu("limit"),
        c.f("min"),
        c.f("max"),
    );
    c.state.fit(input.num_channels());

    let mut channels = Vec::with_capacity(input.num_channels());
    for (ci, ch) in input.channels.iter().enumerate() {
        let mut pos = if reset { initial } else { c.state.f[ci] };
        let samples: Vec<f32> = ch
            .samples
            .iter()
            .map(|v| {
                pos += v * gain * dt;
                match limit {
                    1 => pos = pos.clamp(lo.min(hi), hi.max(lo)),
                    2 => {
                        let span = (hi - lo).abs().max(1e-6);
                        pos = lo + (pos - lo).rem_euclid(span);
                    }
                    _ => {}
                }
                pos
            })
            .collect();
        c.state.f[ci] = pos;
        channels.push(Channel::new(ch.name.clone(), samples));
    }
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// ---------------------------------------------------------------- count

fn params_count() -> IndexMap<String, Param> {
    params! {
        "threshold" => Param::float(0.5).with_label("Threshold"),
        "edge" => Param::menu("rising", &["rising", "falling", "both"]).with_label("Edge"),
        "reset" => Param::bool(false).with_label("Reset"),
        "limit" => Param::menu("off", &["off", "clamp", "wrap"]).with_label("Limit"),
        "min" => Param::float(0.0).with_label("Min"),
        "max" => Param::float(8.0).with_label("Max"),
    }
}

fn cook_count(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let (threshold, edge, reset, limit, lo, hi) = (
        c.f("threshold"),
        c.menu("edge"),
        c.b("reset"),
        c.menu("limit"),
        c.f("min"),
        c.f("max"),
    );
    c.state.fit(input.num_channels());

    let mut channels = Vec::with_capacity(input.num_channels());
    for (ci, ch) in input.channels.iter().enumerate() {
        let mut count = if reset { 0.0 } else { c.state.f[ci] };
        let mut prev = c.state.prev[ci];
        let samples: Vec<f32> = ch
            .samples
            .iter()
            .map(|x| {
                let rising = prev <= threshold && *x > threshold;
                let falling = prev > threshold && *x <= threshold;
                let hit = match edge {
                    1 => falling,
                    2 => rising || falling,
                    _ => rising,
                };
                prev = *x;
                if hit {
                    count += 1.0;
                    match limit {
                        1 => count = count.clamp(lo.min(hi), hi.max(lo)),
                        2 => {
                            let span = (hi - lo).abs().max(1.0);
                            count = lo + (count - lo).rem_euclid(span);
                        }
                        _ => {}
                    }
                }
                count
            })
            .collect();
        c.state.f[ci] = count;
        c.state.prev[ci] = prev;
        channels.push(Channel::new(ch.name.clone(), samples));
    }
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// --------------------------------------------------------------- select

fn params_select() -> IndexMap<String, Param> {
    params! {
        "channels" => Param::str("*").with_label("Channel Names"),
        "rename" => Param::str("").with_label("Rename To"),
    }
}

/// Space-separated names with `*` wildcards, TD style: `chan1 lfo*`.
pub fn name_matches(pattern: &str, name: &str) -> bool {
    pattern.split_whitespace().any(|pat| glob_match(pat, name))
}

fn glob_match(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => {
            name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail)
        }
    }
}

fn cook_select(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let pattern = c.s("channels");
    let rename = c.s("rename");
    let renames: Vec<&str> = rename.split_whitespace().collect();

    let channels: Vec<Channel> = input
        .channels
        .iter()
        .filter(|ch| pattern.trim().is_empty() || name_matches(&pattern, &ch.name))
        .enumerate()
        .map(|(i, ch)| Channel {
            name: renames
                .get(i)
                .map(|s| s.to_string())
                .unwrap_or_else(|| ch.name.clone()),
            samples: ch.samples.clone(),
        })
        .collect();
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// ---------------------------------------------------------------- merge

fn cook_merge(c: &mut ChopCtx) -> ChopData {
    let a = c.input(0).clone();
    let b = c.input(1).clone();
    let len = a.num_samples().max(b.num_samples());
    let mut channels = a.channels.clone();
    channels.extend(b.channels.iter().cloned());

    // Duplicate names would make an Export ambiguous, so number them.
    let mut seen: Vec<String> = Vec::new();
    for ch in &mut channels {
        if seen.contains(&ch.name) {
            let mut i = 1;
            loop {
                let candidate = format!("{}{}", ch.name, i);
                if !seen.contains(&candidate) {
                    ch.name = candidate;
                    break;
                }
                i += 1;
            }
        }
        seen.push(ch.name.clone());
    }
    let mut out = ChopData::new(
        channels,
        a.sample_rate.max(b.sample_rate).max(CONTROL_RATE),
        a.time_sliced || b.time_sliced,
    );
    out.pad_to(len);
    out
}

// --------------------------------------------------------------- switch

fn params_switch() -> IndexMap<String, Param> {
    params! {
        "index" => Param::float(0.0).with_label("Index").with_range(0.0, 1.0),
    }
}

fn cook_switch(c: &mut ChopCtx) -> ChopData {
    let index = c.f("index");
    if index >= 0.5 {
        c.input(1).clone()
    } else {
        c.input(0).clone()
    }
}

// ----------------------------------------------------------------- null

fn cook_null(c: &mut ChopCtx) -> ChopData {
    c.input(0).clone()
}

// ---------------------------------------------------------------- delay

fn params_delay() -> IndexMap<String, Param> {
    params! {
        "delay" => Param::float(0.25).with_label("Delay (s)").with_range(0.0, 10.0),
        "mix" => Param::float(1.0).with_label("Mix").with_range(0.0, 1.0),
        "feedback" => Param::float(0.0).with_label("Feedback").with_range(0.0, 0.95),
    }
}

/// Hold each channel's history and read it back later.
///
/// The buffer is per channel and lives in the node's state, which is what
/// makes this a CHOP rather than a parameter trick: the delay is in *samples*
/// of the input's own rate, so the same node delays a 60 Hz control channel
/// and a 48 kHz audio channel by the same wall-clock time.
fn cook_delay(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let rate = input.sample_rate.max(1.0);
    let want = ((c.f("delay") as f64) * rate).round().max(0.0) as usize;
    let mix = c.f("mix").clamp(0.0, 1.0);
    let fb = c.f("feedback").clamp(0.0, 0.95);

    if want == 0 {
        return input;
    }
    // One ring per channel, laid end to end in the state's float scratch so
    // the existing per-node state carries it with no new machinery. The ring
    // is exactly `want` long: read the oldest slot, overwrite it, then
    // advance, which makes sample *n* come out as sample *n - want* rather
    // than one late.
    let stride = want;
    let needed = input.num_channels() * stride;
    if c.state.f.len() != needed {
        c.state.f.clear();
        c.state.f.resize(needed, 0.0);
        c.state.i.clear();
        c.state.i.resize(input.num_channels().max(1), 0);
    }
    c.state.i.resize(input.num_channels().max(1), 0);

    let mut channels = Vec::with_capacity(input.num_channels());
    for (ci, ch) in input.channels.iter().enumerate() {
        let base = ci * stride;
        let mut head = c.state.i[ci] as usize;
        let samples: Vec<f32> = ch
            .samples
            .iter()
            .map(|x| {
                let delayed = c.state.f[base + head];
                c.state.f[base + head] = x + delayed * fb;
                head = (head + 1) % stride;
                x * (1.0 - mix) + delayed * mix
            })
            .collect();
        c.state.i[ci] = head as i64;
        channels.push(Channel::new(ch.name.clone(), samples));
    }
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// ----------------------------------------------------------- expression

fn params_expression() -> IndexMap<String, Param> {
    params! {
        "expr" => Param::str("v").with_label("Expression"),
    }
}

/// An arbitrary expression applied to every sample.
///
/// `v` is the sample, `i` its index, `c` the channel index and `n` the number
/// of samples; `time`, `frame` and the rest of the evaluator's vocabulary are
/// in scope too, so `v * sin(time)` is a whole operator.
///
/// The expression is parsed **once per cook**, not once per sample. That is
/// the difference between an operator you can put on an audio-rate channel and
/// one you cannot: parsing 512 times a frame would cost more than the
/// arithmetic by two orders of magnitude.
fn cook_expression(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let source = c.s("expr");
    let Ok(expr) = otd_core::Expr::parse(&source) else {
        // A half-typed expression holds the input rather than emptying the
        // chain — the same rule the GLSL TOP follows for a shader mid-edit.
        return input;
    };

    let n = input.num_samples() as f64;
    let mut channels = Vec::with_capacity(input.num_channels());
    for (ci, ch) in input.channels.iter().enumerate() {
        let samples: Vec<f32> = ch
            .samples
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let locals = [
                    ("v", *v as f64),
                    ("i", i as f64),
                    ("c", ci as f64),
                    ("n", n),
                ];
                let eval = otd_core::EvalContext {
                    locals: &locals,
                    ..*c.eval
                };
                expr.eval(&eval).map(|r| r as f32).unwrap_or(*v)
            })
            .collect();
        channels.push(Channel::new(ch.name.clone(), samples));
    }
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// ---------------------------------------------------------------- limit

fn params_limit() -> IndexMap<String, Param> {
    params! {
        "type" => Param::menu("clamp", &["clamp", "loop", "zigzag", "quantise"])
            .with_label("Type"),
        "min" => Param::float(0.0).with_label("Minimum").with_range(-10.0, 10.0),
        "max" => Param::float(1.0).with_label("Maximum").with_range(-10.0, 10.0),
        "step" => Param::float(0.1).with_label("Quantise Step").with_range(0.0, 10.0),
    }
}

fn cook_limit(c: &mut ChopCtx) -> ChopData {
    let (lo, hi) = (c.f("min"), c.f("max"));
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let span = (hi - lo).max(1e-6);
    let step = c.f("step");
    let mode = c.menu("type");
    map_samples(c.input(0), |_, _, v| match mode {
        0 => v.clamp(lo, hi),
        1 => lo + (v - lo).rem_euclid(span),
        2 => {
            // Fold instead of wrap: a value walking past the top comes back
            // down rather than jumping, which is the whole point of zigzag.
            let t = (v - lo).rem_euclid(span * 2.0);
            lo + if t <= span { t } else { span * 2.0 - t }
        }
        _ => {
            if step <= 0.0 {
                v
            } else {
                (v / step).round() * step
            }
        }
    })
}

// ---------------------------------------------------------------- slope

fn params_slope() -> IndexMap<String, Param> {
    params! {
        "units" => Param::menu("persecond", &["persecond", "persample"])
            .with_label("Units"),
    }
}

/// The derivative — the inverse of the Speed CHOP, which integrates.
fn cook_slope(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let scale = if c.menu("units") == 0 {
        input.sample_rate.max(1.0) as f32
    } else {
        1.0
    };
    c.state.fit(input.num_channels());

    let mut channels = Vec::with_capacity(input.num_channels());
    for (ci, ch) in input.channels.iter().enumerate() {
        // Carrying the last sample across the frame boundary is what keeps
        // the slope continuous; without it every frame starts with a spike.
        let mut prev = c.state.prev[ci];
        let samples: Vec<f32> = ch
            .samples
            .iter()
            .map(|x| {
                let d = (x - prev) * scale;
                prev = *x;
                d
            })
            .collect();
        c.state.prev[ci] = prev;
        channels.push(Channel::new(ch.name.clone(), samples));
    }
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// ----------------------------------------------------------------- hold

fn params_hold() -> IndexMap<String, Param> {
    params! {
        "threshold" => Param::float(0.5).with_label("Trigger Threshold").with_range(-10.0, 10.0),
    }
}

/// Sample and hold: input 0's value is frozen until input 1 rises past the
/// threshold. Two inputs rather than a parameter because the thing that says
/// "now" is nearly always another channel — a beat, a MIDI note, a button.
fn cook_hold(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let trigger = c.input(1).clone();
    let threshold = c.f("threshold");
    c.state.fit(input.num_channels() + 1);
    let n = input.num_samples();

    // Sample index of every rising edge in this slice.
    let mut fires = Vec::new();
    if let Some(t) = trigger.channels.first() {
        let mut prev = c.state.prev[input.num_channels()];
        for (i, v) in t.samples.iter().enumerate() {
            if prev < threshold && *v >= threshold {
                fires.push(i);
            }
            prev = *v;
        }
        c.state.prev[input.num_channels()] = prev;
    }

    let mut channels = Vec::with_capacity(input.num_channels());
    for (ci, ch) in input.channels.iter().enumerate() {
        let mut held = c.state.f[ci];
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if fires.contains(&i) {
                held = ch.samples.get(i).copied().unwrap_or(held);
            }
            out.push(held);
        }
        c.state.f[ci] = held;
        channels.push(Channel::new(ch.name.clone(), out));
    }
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// --------------------------------------------------------------- shuffle

fn params_shuffle() -> IndexMap<String, Param> {
    params! {
        "method" => Param::menu(
            "transpose",
            &["transpose", "reverse", "swapfirstlast", "sequence"],
        ).with_label("Method"),
    }
}

/// Rearrange the channel/sample grid.
///
/// Transpose is the one that earns its keep: N channels of one sample become
/// one channel of N samples, which is how a pile of separate values becomes
/// something a CHOP to TOP or an instancing Geometry COMP can consume.
fn cook_shuffle(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let n = input.num_samples();
    let channels = match c.menu("method") {
        0 => {
            let rows: Vec<Channel> = (0..n)
                .map(|s| {
                    Channel::new(
                        format!("chan{}", s + 1),
                        input
                            .channels
                            .iter()
                            .map(|ch| ch.samples.get(s).copied().unwrap_or(0.0))
                            .collect(),
                    )
                })
                .collect();
            rows
        }
        1 => input
            .channels
            .iter()
            .map(|ch| {
                let mut s = ch.samples.clone();
                s.reverse();
                Channel::new(ch.name.clone(), s)
            })
            .collect(),
        2 => {
            let mut out = input.channels.clone();
            if out.len() >= 2 {
                let last = out.len() - 1;
                out.swap(0, last);
            }
            out
        }
        _ => {
            // Every channel end to end in one.
            let mut all = Vec::new();
            for ch in &input.channels {
                all.extend_from_slice(&ch.samples);
            }
            vec![Channel::new("chan1", all)]
        }
    };
    // Transposing a time-sliced stream produces a buffer, not a stream.
    ChopData::new(channels, input.sample_rate, false)
}

// ---------------------------------------------------------------- rename

fn params_rename() -> IndexMap<String, Param> {
    params! {
        "from" => Param::str("*").with_label("Rename Channels"),
        "to" => Param::str("chan[1-]").with_label("To"),
    }
}

/// Rename channels, TD's numbered-pattern style: `chan[1-]` names them
/// `chan1`, `chan2`, … A plain name with no bracket renames every match to
/// the same thing, which is what you want feeding a Merge.
fn cook_rename(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let from = c.s("from");
    let to = c.s("to");
    let patterns: Vec<&str> = from.split_whitespace().collect();

    let (prefix, start, suffix) = match to.find("[").zip(to.find("]")) {
        Some((open, close)) if close > open => {
            let inner = &to[open + 1..close];
            let n: i64 = inner.trim_end_matches('-').parse().unwrap_or(1);
            (&to[..open], Some(n), &to[close + 1..])
        }
        _ => (to.as_str(), None, ""),
    };

    let mut counter = start.unwrap_or(0);
    let channels = input
        .channels
        .iter()
        .map(|ch| {
            if !patterns.iter().any(|p| glob_match(p, &ch.name)) {
                return ch.clone();
            }
            let name = match start {
                Some(_) => {
                    let n = counter;
                    counter += 1;
                    format!("{prefix}{n}{suffix}")
                }
                None => prefix.to_string(),
            };
            Channel::new(name, ch.samples.clone())
        })
        .collect();
    ChopData::new(channels, input.sample_rate, input.time_sliced)
}

// --------------------------------------------------------------- analyze

fn params_analyze() -> IndexMap<String, Param> {
    params! {
        "function" => Param::menu(
            "average",
            &["average", "maximum", "minimum", "rms", "sum", "length", "median"],
        ).with_label("Function"),
    }
}

/// Reduce each channel to one number.
///
/// A whole waveform is rarely what a parameter wants; one number that
/// describes it usually is. Keeping the channel names means a downstream
/// export still reads `ch('/analyze1', 'chan1')` and gets the summary.
fn cook_analyze(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let func = c.menu("function");
    let channels = input
        .channels
        .iter()
        .map(|ch| {
            let s = &ch.samples;
            let n = s.len().max(1) as f32;
            let v = match func {
                0 => s.iter().sum::<f32>() / n,
                1 => ch.max(),
                2 => ch.min(),
                3 => (s.iter().map(|v| v * v).sum::<f32>() / n).sqrt(),
                4 => s.iter().sum::<f32>(),
                5 => s.len() as f32,
                _ => {
                    let mut sorted = s.clone();
                    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    sorted.get(sorted.len() / 2).copied().unwrap_or(0.0)
                }
            };
            Channel::new(ch.name.clone(), vec![v])
        })
        .collect();
    ChopData::new(channels, input.sample_rate, false)
}

// ----------------------------------------------------------------- cross

fn params_cross() -> IndexMap<String, Param> {
    params! {
        "cross" => Param::float(0.5).with_label("Cross").with_range(0.0, 1.0),
        "curve" => Param::menu("linear", &["linear", "equalpower", "smooth"])
            .with_label("Curve"),
    }
}

/// Blend two inputs channel by channel.
///
/// Equal power is the default worth knowing about: a linear crossfade between
/// two uncorrelated signals dips ~3 dB in the middle, which is audible on
/// audio and visible on anything driving brightness.
fn cook_cross(c: &mut ChopCtx) -> ChopData {
    let a = c.input(0).clone();
    let b = c.input(1).clone();
    let t = c.f("cross").clamp(0.0, 1.0);
    let quarter = std::f32::consts::FRAC_PI_2;
    let (wa, wb) = match c.menu("curve") {
        1 => (((1.0 - t) * quarter).sin(), (t * quarter).sin()),
        2 => {
            let s = t * t * (3.0 - 2.0 * t);
            (1.0 - s, s)
        }
        _ => (1.0 - t, t),
    };

    let len = a.num_samples().max(b.num_samples());
    let count = a.num_channels().max(b.num_channels());
    let channels = (0..count)
        .map(|ci| {
            let ca = a.nth(ci);
            let cb = b.nth(ci);
            let name = ca
                .or(cb)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("chan{}", ci + 1));
            let samples = (0..len)
                .map(|i| {
                    let va = ca.map(|c| sample_held(&c.samples, i)).unwrap_or(0.0);
                    let vb = cb.map(|c| sample_held(&c.samples, i)).unwrap_or(0.0);
                    va * wa + vb * wb
                })
                .collect();
            Channel::new(name, samples)
        })
        .collect();
    ChopData::new(channels, a.sample_rate.max(b.sample_rate), a.time_sliced)
}

/// Read a sample, holding the last one past the end.
///
/// This is what lets a single-sample control channel broadcast against a
/// waveform — the same rule instancing uses, so it does not surprise.
fn sample_held(s: &[f32], i: usize) -> f32 {
    s.get(i).or_else(|| s.last()).copied().unwrap_or(0.0)
}

// -------------------------------------------------------------- resample

fn params_resample() -> IndexMap<String, Param> {
    params! {
        "length" => Param::int(64).with_label("Length (samples)").with_range(1.0, 8192.0),
        "interp" => Param::menu("linear", &["linear", "nearest"]).with_label("Interpolate"),
    }
}

/// Change how many samples a buffer has, keeping its shape.
fn cook_resample(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let n = c.i("length").clamp(1, 8192) as usize;
    let nearest = c.menu("interp") == 1;
    let channels = input
        .channels
        .iter()
        .map(|ch| {
            let src = &ch.samples;
            let samples = (0..n)
                .map(|i| {
                    if src.is_empty() {
                        return 0.0;
                    }
                    if src.len() == 1 {
                        return src[0];
                    }
                    let pos = i as f32 * (src.len() - 1) as f32 / (n.max(2) - 1) as f32;
                    let lo = pos.floor() as usize;
                    if nearest {
                        return src[pos.round() as usize % src.len()];
                    }
                    let hi = (lo + 1).min(src.len() - 1);
                    let f = pos - lo as f32;
                    src[lo] * (1.0 - f) + src[hi] * f
                })
                .collect();
            Channel::new(ch.name.clone(), samples)
        })
        .collect();
    // A resampled buffer is no longer one frame's worth of a stream.
    ChopData::new(channels, input.sample_rate, false)
}

// ----------------------------------------------------------------- clock

fn params_clock() -> IndexMap<String, Param> {
    with_rate(params! {})
}

/// Wall-clock time as channels: `second`, `minute`, `hour`, `frame`.
///
/// Distinct from the timeline on purpose. The timeline is the patch's clock
/// and is scrubbable; this is the one that keeps running when the timeline is
/// paused, which is what an installation that has to change at 9pm needs.
fn cook_clock(c: &mut ChopCtx) -> ChopData {
    let n = slice_len(c.rate(), c.time);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let secs = now as i64;
    let vals = [
        (secs % 60) as f32,
        ((secs / 60) % 60) as f32,
        ((secs / 3600) % 24) as f32,
        c.time.frame as f32,
    ];
    let names = ["second", "minute", "hour", "frame"];
    let channels = names
        .iter()
        .zip(vals)
        .map(|(name, v)| Channel::constant(*name, v, n))
        .collect();
    ChopData::new(channels, c.rate(), true)
}

// ------------------------------------------------------------------ beat

fn params_beat() -> IndexMap<String, Param> {
    with_rate(params! {
        "tempo" => Param::float(120.0).with_label("Tempo (BPM)").with_range(20.0, 300.0),
        "beatsperbar" => Param::int(4).with_label("Beats Per Bar").with_range(1.0, 32.0),
    })
}

/// A tempo clock: `ramp` 0..1 within a beat, `pulse` at each beat, `count`,
/// and the same for a bar.
///
/// Derived from the timeline's absolute time rather than from a counter, so a
/// dropped frame does not put the beat permanently behind — the same reason
/// the Animation CHOP and Movie File In are functions of time.
fn cook_beat(c: &mut ChopCtx) -> ChopData {
    let rate = c.rate();
    let n = slice_len(rate, c.time);
    let bps = (c.f("tempo") as f64 / 60.0).max(1e-6);
    let per_bar = c.i("beatsperbar").max(1) as f64;

    let mut ramp = Vec::with_capacity(n);
    let mut pulse = Vec::with_capacity(n);
    let mut count = Vec::with_capacity(n);
    let mut bar_ramp = Vec::with_capacity(n);
    let mut prev = c.state.f.first().copied().unwrap_or(0.0);
    c.state.fit(1);

    for t in slice_times(c.time, n) {
        let beats = t * bps;
        let frac = beats.rem_euclid(1.0) as f32;
        ramp.push(frac);
        // A pulse is one sample wide at the wrap, which is what a Trigger or
        // a Count downstream is looking for.
        pulse.push(if frac < prev { 1.0 } else { 0.0 });
        prev = frac;
        count.push(beats.floor() as f32);
        bar_ramp.push((beats / per_bar).rem_euclid(1.0) as f32);
    }
    c.state.f[0] = prev;

    ChopData::new(
        vec![
            Channel::new("ramp", ramp),
            Channel::new("pulse", pulse),
            Channel::new("count", count),
            Channel::new("bar", bar_ramp),
        ],
        rate,
        true,
    )
}

// -------------------------------------------------------- DAT to CHOP

fn params_dat_to() -> IndexMap<String, Param> {
    params! {
        "layout" => Param::menu("columns", &["columns", "rows"])
            .with_label("Channels From"),
        "names" => Param::bool(true)
            .with_label("First Row/Column Is Names"),
    }
}

/// Numbers out of a table. A cue list, a CSV of positions, a JSON payload
/// already flattened by a JSON DAT — all of them become channels here, which
/// is the point of the DAT family being a family at all.
fn cook_dat_to_chop(c: &mut ChopCtx) -> ChopData {
    let Some(rows) = c.foreign(0).and_then(|f| f.as_table()) else {
        return ChopData::empty();
    };
    let by_columns = c.menu("layout") == 0;
    let named = c.b("names");

    // Work in "lines of cells", where a line is a column or a row depending
    // on the layout, so the two cases share everything below.
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let cell = |r: usize, col: usize| -> &str {
        rows.get(r)
            .and_then(|row| row.get(col))
            .map(|s| s.as_str())
            .unwrap_or("")
    };
    let lines: Vec<Vec<String>> = if by_columns {
        (0..width)
            .map(|col| (0..rows.len()).map(|r| cell(r, col).to_string()).collect())
            .collect()
    } else {
        rows.iter().map(|r| r.to_vec()).collect()
    };

    let channels: Vec<Channel> = lines
        .into_iter()
        .enumerate()
        .map(|(i, mut line)| {
            let name = if named && !line.is_empty() {
                let head = line.remove(0);
                if head.trim().is_empty() {
                    format!("chan{}", i + 1)
                } else {
                    sanitise(&head)
                }
            } else {
                format!("chan{}", i + 1)
            };
            // A cell that is not a number is 0 rather than an error: a table
            // usually has a label column, and refusing to cook because of it
            // would make the common case the broken one.
            let samples = line
                .iter()
                .map(|s| s.trim().parse().unwrap_or(0.0))
                .collect();
            Channel::new(name, samples)
        })
        .filter(|c| !c.samples.is_empty())
        .collect();

    // Not time sliced: this is a standalone buffer, like a Pattern CHOP.
    ChopData::new(channels, CONTROL_RATE, false)
}

/// A channel name has to survive being typed into an expression.
fn sanitise(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    if cleaned.starts_with(|ch: char| ch.is_ascii_digit()) {
        format!("_{cleaned}")
    } else if cleaned.is_empty() {
        "chan".into()
    } else {
        cleaned
    }
}

// -------------------------------------------------------- SOP to CHOP

fn params_sop_to() -> IndexMap<String, Param> {
    params! {
        "attrs" => Param::str("P")
            .with_label("Attributes (P N uv Cd)"),
    }
}

/// Geometry as channels: one sample per point, one channel per component.
///
/// This is what closes the instancing loop. A Copy SOP or a Noise SOP can
/// decide where 256 things go, and the positions come back out here as the
/// `tx`/`ty`/`tz` an instancing Geometry COMP reads — geometry driving
/// geometry, without either end knowing about the other.
fn cook_sop_to_chop(c: &mut ChopCtx) -> ChopData {
    let Some(points) = c.foreign(0) else {
        return ChopData::empty();
    };
    let stride = points.point_stride();
    let Crossing::Points { data, .. } = points else {
        return ChopData::empty();
    };
    if stride == 0 {
        return ChopData::empty();
    }
    let count = data.len() / stride;

    let wanted = c.s("attrs");
    let wanted: Vec<&str> = wanted.split_whitespace().collect();
    let suffix = ["x", "y", "z", "w"];

    let mut channels = Vec::new();
    for name in wanted {
        let Some((offset, width)) = points.attr(name) else {
            continue;
        };
        for comp in 0..width {
            let samples = (0..count)
                .map(|p| data[p * stride + offset + comp])
                .collect();
            let label = if width == 1 {
                name.to_string()
            } else {
                format!("{name}{}", suffix.get(comp).copied().unwrap_or("?"))
            };
            channels.push(Channel::new(label, samples));
        }
    }
    ChopData::new(channels, CONTROL_RATE, false)
}

// -------------------------------------------------------- TOP to CHOP

fn params_top_to() -> IndexMap<String, Param> {
    params! {
        "active" => Param::bool(true).with_label("Active"),
        "layout" => Param::menu("rows", &["rows", "columns", "average"])
            .with_label("Read"),
        "index" => Param::int(0)
            .with_label("Row / Column")
            .with_range(0.0, 4096.0),
    }
}

/// Pixels as channels: `r`, `g`, `b`, `a`, one sample per pixel read.
///
/// Two things about this operator are worth knowing before you patch it, and
/// both are true of TouchDesigner's as well:
///
///  * It reads **last frame**. A readback has to be submitted to the GPU, and
///    this frame's work is still being recorded into an encoder that has not
///    been submitted yet. Waiting for it would mean stalling the frame on the
///    render it is in the middle of building.
///  * It costs a full pipeline sync, and the cost is the *source's*
///    resolution. Put a Resolution TOP in front of it. Reading 1920×1080 back
///    every frame to get four numbers is the classic way to turn a 60 fps
///    patch into a 12 fps one.
fn cook_top_to_chop(c: &mut ChopCtx) -> ChopData {
    if !c.b("active") {
        return ChopData::empty();
    }
    let Some(Crossing::Pixels {
        width,
        height,
        rgba,
    }) = c.foreign(0)
    else {
        return ChopData::empty();
    };
    let (w, h) = (*width as usize, *height as usize);
    if w == 0 || h == 0 {
        return ChopData::empty();
    }
    let at = |x: usize, y: usize, comp: usize| rgba[(y * w + x) * 4 + comp];

    let mut samples = vec![Vec::new(); 4];
    match c.menu("layout") {
        // One row of pixels, left to right.
        0 => {
            let y = (c.i("index").max(0) as usize).min(h - 1);
            for (comp, out) in samples.iter_mut().enumerate() {
                *out = (0..w).map(|x| at(x, y, comp)).collect();
            }
        }
        // One column, top to bottom.
        1 => {
            let x = (c.i("index").max(0) as usize).min(w - 1);
            for (comp, out) in samples.iter_mut().enumerate() {
                *out = (0..h).map(|y| at(x, y, comp)).collect();
            }
        }
        // The whole image as one number per component.
        _ => {
            let n = (w * h) as f32;
            for (comp, out) in samples.iter_mut().enumerate() {
                let sum: f32 = (0..h)
                    .flat_map(|y| (0..w).map(move |x| (x, y)))
                    .map(|(x, y)| at(x, y, comp))
                    .sum();
                *out = vec![sum / n];
            }
        }
    }

    let names = ["r", "g", "b", "a"];
    let channels = samples
        .into_iter()
        .enumerate()
        .map(|(i, s)| Channel::new(names[i], s))
        .collect();
    ChopData::new(channels, CONTROL_RATE, false)
}

// ------------------------------------------------------------ the table

pub const NULL: &str = "nullCHOP";
/// A component's channel input, surfaced as a connector on its node.
pub const IN: &str = "inCHOP";
/// A component's channel output.
pub const OUT: &str = "outCHOP";

fn specs() -> &'static Vec<ChopSpec> {
    static SPECS: OnceLock<Vec<ChopSpec>> = OnceLock::new();
    SPECS.get_or_init(|| {
        let mut v = vec![
            spec(
                "constantCHOP",
                "Constant",
                &[],
                "Fixed values as channels.",
                params_constant,
                cook_constant,
            ),
            spec_animated(
                "lfoCHOP",
                "LFO",
                &[],
                "A repeating waveform over time.",
                params_lfo,
                cook_lfo,
            ),
            spec_animated(
                "animationCHOP",
                "Animation",
                &[],
                "Keyframed curves over time.",
                params_animation,
                cook_animation,
            ),
            spec_animated(
                "noiseCHOP",
                "Noise",
                &[],
                "Smooth or random noise over time.",
                params_noise,
                cook_noise,
            ),
            spec(
                "patternCHOP",
                "Pattern",
                &[],
                "A fixed-length waveform buffer.",
                params_pattern,
                cook_pattern,
            ),
            spec(
                "mathCHOP",
                "Math",
                &["a", "b"],
                "Combine and scale channels.",
                params_math,
                cook_math,
            ),
            spec_animated(
                "lagCHOP",
                "Lag",
                &["in"],
                "Smooth a channel, with separate rise and fall times.",
                params_lag,
                cook_lag,
            ),
            spec_animated(
                "filterCHOP",
                "Filter",
                &["in"],
                "Low-pass or moving-average smoothing.",
                params_filter,
                cook_filter,
            ),
            spec(
                "logicCHOP",
                "Logic",
                &["in"],
                "Turn channels into on/off and combine them.",
                params_logic,
                cook_logic,
            ),
            spec_animated(
                "triggerCHOP",
                "Trigger",
                &["in"],
                "An attack/decay/sustain/release envelope per channel.",
                params_trigger,
                cook_trigger,
            ),
            spec_animated(
                "timerCHOP",
                "Timer",
                &[],
                "A running timer with fraction, seconds, cycles and done.",
                params_timer,
                cook_timer,
            ),
            spec_animated(
                "speedCHOP",
                "Speed",
                &["in"],
                "Integrate a channel: velocity becomes position.",
                params_speed,
                cook_speed,
            ),
            spec_animated(
                "countCHOP",
                "Count",
                &["in"],
                "Count threshold crossings.",
                params_count,
                cook_count,
            ),
            spec(
                "selectCHOP",
                "Select",
                &["in"],
                "Pick and rename channels.",
                params_select,
                cook_select,
            ),
            spec(
                "mergeCHOP",
                "Merge",
                &["a", "b"],
                "Put two CHOPs' channels side by side.",
                no_params,
                cook_merge,
            ),
            spec(
                "switchCHOP",
                "Switch",
                &["0", "1"],
                "Choose one of two inputs.",
                params_switch,
                cook_switch,
            ),
            spec(
                NULL,
                "Null",
                &["in"],
                "Pass-through. A stable name to export from.",
                no_params,
                cook_null,
            ),
            spec_animated(
                "delayCHOP",
                "Delay",
                &["in"],
                "Play a channel back later, with optional feedback.",
                params_delay,
                cook_delay,
            ),
            spec_animated(
                "expressionCHOP",
                "Expression",
                &["in"],
                "Apply an expression to every sample. `v` is the sample.",
                params_expression,
                cook_expression,
            ),
            spec(
                "limitCHOP",
                "Limit",
                &["in"],
                "Clamp, wrap, fold or quantise a channel.",
                params_limit,
                cook_limit,
            ),
            spec_animated(
                "slopeCHOP",
                "Slope",
                &["in"],
                "The rate of change of a channel.",
                params_slope,
                cook_slope,
            ),
            spec_animated(
                "holdCHOP",
                "Hold",
                &["in", "trigger"],
                "Freeze input 1's value until input 2 fires.",
                params_hold,
                cook_hold,
            ),
            spec(
                "shuffleCHOP",
                "Shuffle",
                &["in"],
                "Rearrange the channel/sample grid — transpose, reverse, sequence.",
                params_shuffle,
                cook_shuffle,
            ),
            spec(
                "renameCHOP",
                "Rename",
                &["in"],
                "Rename channels by pattern.",
                params_rename,
                cook_rename,
            ),
            spec(
                "analyzeCHOP",
                "Analyze",
                &["in"],
                "Reduce each channel to one number.",
                params_analyze,
                cook_analyze,
            ),
            spec(
                "crossCHOP",
                "Cross",
                &["a", "b"],
                "Blend two inputs, with an equal-power option.",
                params_cross,
                cook_cross,
            ),
            spec(
                "resampleCHOP",
                "Resample",
                &["in"],
                "Change how many samples a buffer has, keeping its shape.",
                params_resample,
                cook_resample,
            ),
            spec_animated(
                "clockCHOP",
                "Clock",
                &[],
                "Wall-clock time, which keeps running when the timeline is paused.",
                params_clock,
                cook_clock,
            ),
            spec_animated(
                "beatCHOP",
                "Beat",
                &[],
                "A tempo clock: ramp, pulse, count and bar.",
                params_beat,
                cook_beat,
            ),
            converter_spec(
                "dattochopCHOP",
                "DAT to CHOP",
                Family::Dat,
                "Columns or rows of a table, as channels.",
                params_dat_to,
                cook_dat_to_chop,
            ),
            converter_spec(
                "soptochopCHOP",
                "SOP to CHOP",
                Family::Sop,
                "Point attributes as channels, one sample per point.",
                params_sop_to,
                cook_sop_to_chop,
            ),
            // Animated: a readback has to happen every frame to be worth
            // anything, and the source is a moving picture by assumption.
            {
                let mut s = converter_spec(
                    "toptochopCHOP",
                    "TOP to CHOP",
                    Family::Top,
                    "Pixels as channels. Reads last frame; costs a GPU sync.",
                    params_top_to,
                    cook_top_to_chop,
                );
                s.def.time_dependent = true;
                s
            },
            connector_spec(
                IN,
                "In",
                &[],
                "A channel input on this component's node.",
                Connector::In,
            ),
            connector_spec(
                OUT,
                "Out",
                &["in"],
                "This component's channel output.",
                Connector::Out,
            ),
        ];
        v.extend(crate::io::specs());
        v
    })
}

/// Small helper so the table above reads as a table.
pub(crate) fn spec(
    type_name: &'static str,
    label: &'static str,
    inputs: &'static [&'static str],
    summary: &'static str,
    params: fn() -> IndexMap<String, Param>,
    cook: fn(&mut ChopCtx) -> ChopData,
) -> ChopSpec {
    ChopSpec {
        def: OpDef {
            type_name,
            label,
            family: Family::Chop,
            inputs,
            input_families: &[],
            summary,
            time_dependent: false,
            params,
            connector: Connector::None,
        },
        cook,
    }
}

/// The In and Out operators that give a component its channel connectors.
/// An In has no wire of its own; the engine fills it from outside.
pub(crate) fn connector_spec(
    type_name: &'static str,
    label: &'static str,
    inputs: &'static [&'static str],
    summary: &'static str,
    connector: Connector,
) -> ChopSpec {
    let mut s = spec(type_name, label, inputs, summary, no_params, cook_null);
    s.def.connector = connector;
    s
}

/// A converter: one input, of somebody else's family.
///
/// The input is called `in` like any other, because from the patch's point of
/// view it is one — the only thing that differs is which wires will land on
/// it, and the graph enforces that from [`OpDef::input_families`].
pub(crate) fn converter_spec(
    type_name: &'static str,
    label: &'static str,
    from: Family,
    summary: &'static str,
    params: fn() -> IndexMap<String, Param>,
    cook: fn(&mut ChopCtx) -> ChopData,
) -> ChopSpec {
    let mut s = spec(type_name, label, &["in"], summary, params, cook);
    s.def.input_families = match from {
        Family::Dat => &[Family::Dat],
        Family::Sop => &[Family::Sop],
        Family::Top => &[Family::Top],
        Family::Mat => &[Family::Mat],
        Family::Comp => &[Family::Comp],
        Family::Chop => &[Family::Chop],
    };
    s
}

/// Same, for operators that must cook every frame — generators over time and
/// anything reading a device.
pub(crate) fn spec_animated(
    type_name: &'static str,
    label: &'static str,
    inputs: &'static [&'static str],
    summary: &'static str,
    params: fn() -> IndexMap<String, Param>,
    cook: fn(&mut ChopCtx) -> ChopData,
) -> ChopSpec {
    let mut s = spec(type_name, label, inputs, summary, params, cook);
    s.def.time_dependent = true;
    s
}

pub fn spec_for(type_name: &str) -> Option<&'static ChopSpec> {
    specs().iter().find(|s| s.def.type_name == type_name)
}

pub fn all() -> impl Iterator<Item = &'static ChopSpec> {
    specs().iter()
}

pub fn registry() -> OpRegistry {
    let mut r = OpRegistry::new();
    for s in specs() {
        r.register(s.def.clone());
    }
    r
}
