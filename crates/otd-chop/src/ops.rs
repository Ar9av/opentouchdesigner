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
use otd_core::{CookContext, EvalContext, Family, Node, OpDef, OpRegistry, Param, Value};

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

// ------------------------------------------------------------ the table

pub const NULL: &str = "nullCHOP";

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
            summary,
            time_dependent: false,
            params,
        },
        cook,
    }
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
