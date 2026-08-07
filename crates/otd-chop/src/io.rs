//! Device and network CHOPs: audio in, spectrum, MIDI in, OSC in and out,
//! mouse and keyboard.
//!
//! PLAN.md §4 is explicit that device work does not happen on the cook thread.
//! Audio runs on cpal's callback thread, MIDI on midir's, OSC on its own
//! socket thread; each writes into a small shared buffer, and the cook simply
//! takes whatever is there. Nothing here can block a frame, and a missing
//! device degrades to silence with a message on the node rather than to a
//! failed cook — losing an interface mid-show must not stop the render.

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use otd_core::Param;
use otd_core::indexmap::IndexMap;

use crate::data::{CONTROL_RATE, Channel, ChopData, slice_len};
use crate::ops::{ChopCtx, ChopSpec, spec_animated};

/// Mouse and keyboard state, pushed in by the editor each frame. The CHOPs
/// read it; `otd-chop` never touches a window system itself.
#[derive(Clone, Debug, Default)]
pub struct InputState {
    /// Normalised to -0.5..0.5 with the origin at the centre of the canvas.
    pub mouse: [f32; 2],
    pub buttons: [bool; 3],
    pub wheel: f32,
    /// Names of the keys currently held, lowercase.
    pub keys: Vec<String>,
}

#[derive(Default)]
pub struct Io {
    pub input: InputState,
    /// The directory file parameters resolve against — the project's own,
    /// set when it opens, so `stems/kick.wav` means the same thing on the
    /// machine the project was authored on and the one it plays on.
    pub base_dir: Option<std::path::PathBuf>,
    audio: HashMap<String, AudioIn>,
    audio_out: HashMap<String, AudioOut>,
    files: HashMap<String, AudioFile>,
    midi: HashMap<String, MidiIn>,
    midi_out: HashMap<String, MidiOut>,
    osc_in: HashMap<String, OscIn>,
    osc_out: HashMap<String, OscOut>,
    dmx: HashMap<String, DmxOut>,
    spectra: HashMap<String, Spectrum>,
    /// Per-node message shown on the node body — "no such device", and so on.
    pub status: HashMap<String, String>,
}

impl Io {
    pub fn new() -> Self {
        Self::default()
    }

    fn note(&mut self, path: &str, message: impl Into<String>) {
        self.status.insert(path.to_string(), message.into());
    }

    fn clear_note(&mut self, path: &str) {
        self.status.remove(path);
    }

    pub fn status_of(&self, path: &str) -> Option<&str> {
        self.status.get(path).map(|s| s.as_str())
    }

    /// Release every device. Called when a project is closed.
    pub fn reset(&mut self) {
        self.audio.clear();
        self.audio_out.clear();
        self.files.clear();
        self.midi.clear();
        self.midi_out.clear();
        self.osc_in.clear();
        self.osc_out.clear();
        self.dmx.clear();
        self.spectra.clear();
        self.status.clear();
    }
}

/// cpal 0.18 exposes the human-readable name through a description struct.
fn device_name(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string())
}

// -------------------------------------------------------------- audio in

struct AudioIn {
    /// Kept alive so the callback keeps running; never read directly.
    _stream: cpal::Stream,
    ring: Arc<Mutex<Vec<f32>>>,
    sample_rate: f64,
    device: String,
}

const RING_CAP: usize = 1 << 16;

impl AudioIn {
    fn open(requested: &str) -> Result<AudioIn, String> {
        let host = cpal::default_host();
        let device = if requested.trim().is_empty() {
            host.default_input_device()
                .ok_or("no default audio input device")?
        } else {
            host.input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| device_name(d).map(|n| n == requested).unwrap_or(false))
                .ok_or_else(|| format!("no audio input device called `{requested}`"))?
        };
        let name = device_name(&device).unwrap_or_else(|| "?".to_string());
        let config = device
            .default_input_config()
            .map_err(|e| format!("{name}: {e}"))?;
        let sample_rate = config.sample_rate() as f64;
        let channels = config.channels() as usize;

        let ring: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(RING_CAP)));
        let sink = ring.clone();
        let stream = device
            .build_input_stream(
                config.config(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Downmix to mono here, on the audio thread, so the cook
                    // thread only ever moves one channel of samples.
                    let Ok(mut buf) = sink.lock() else { return };
                    for frame in data.chunks(channels.max(1)) {
                        let mono = frame.iter().sum::<f32>() / channels.max(1) as f32;
                        buf.push(mono);
                    }
                    // Drop the oldest audio rather than grow without bound if
                    // the render stalls.
                    if buf.len() > RING_CAP {
                        let excess = buf.len() - RING_CAP;
                        buf.drain(..excess);
                    }
                },
                move |err| log::warn!("audio input error: {err}"),
                None,
            )
            .map_err(|e| format!("{name}: {e}"))?;
        stream.play().map_err(|e| format!("{name}: {e}"))?;

        Ok(AudioIn {
            _stream: stream,
            ring,
            sample_rate,
            device: name,
        })
    }

    /// Take up to `want` samples. Underrun pads with the last value rather
    /// than with zeros, which clicks far less.
    fn take(&self, want: usize) -> Vec<f32> {
        let Ok(mut buf) = self.ring.lock() else {
            return vec![0.0; want];
        };
        if buf.len() >= want {
            let rest = buf.split_off(want);
            let out = std::mem::replace(&mut *buf, rest);
            return out;
        }
        let mut out = std::mem::take(&mut *buf);
        let pad = out.last().copied().unwrap_or(0.0);
        out.resize(want, pad);
        out
    }
}

fn params_audio_in() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "device".into(),
        Param::str("").with_label("Device (blank = default)"),
    );
    m.insert(
        "volume".into(),
        Param::float(1.0).with_label("Volume").with_range(0.0, 8.0),
    );
    m
}

fn cook_audio_in(c: &mut ChopCtx) -> ChopData {
    let requested = c
        .node
        .param("device")
        .map(|p| p.eval(c.eval).as_str())
        .unwrap_or_default();
    let volume = c
        .node
        .param("volume")
        .map(|p| p.eval(c.eval).as_f32())
        .unwrap_or(1.0);
    let path = c.path.to_string();

    let needs_open = match c.io.audio.get(&path) {
        Some(a) => !requested.trim().is_empty() && a.device != requested,
        None => true,
    };
    if needs_open {
        match AudioIn::open(&requested) {
            Ok(a) => {
                c.io.clear_note(&path);
                c.io.audio.insert(path.clone(), a);
            }
            Err(e) => {
                c.io.note(&path, e);
                c.io.audio.remove(&path);
            }
        }
    }

    let Some(audio) = c.io.audio.get(&path) else {
        // No device: a silent channel at control rate, so downstream
        // operators still have something with the right shape.
        let n = slice_len(CONTROL_RATE, c.time);
        return ChopData::new(vec![Channel::constant("chan1", 0.0, n)], CONTROL_RATE, true);
    };
    let rate = audio.sample_rate;
    let n = slice_len(rate, c.time);
    let samples: Vec<f32> = audio.take(n).into_iter().map(|s| s * volume).collect();
    ChopData::new(vec![Channel::new("chan1", samples)], rate, true)
}

// ----------------------------------------------------------- audio file

struct AudioFile {
    file: String,
    wav: crate::wav::Wav,
}

fn params_audio_file() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert("file".into(), Param::str("").with_label("File (.wav)"));
    m.insert(
        "play".into(),
        Param::menu("loop", &["loop", "once"]).with_label("Play"),
    );
    m.insert(
        "speed".into(),
        Param::float(1.0).with_label("Speed").with_range(-4.0, 4.0),
    );
    m.insert(
        "volume".into(),
        Param::float(1.0).with_label("Volume").with_range(0.0, 8.0),
    );
    m
}

/// Playback is a *function of the timeline*, not a private play head: the
/// sample at time `t` is always `wav[t * speed * rate]`. That is what makes
/// scrubbing the timeline scrub the audio, a loop range loop it, and a
/// headless render read the same samples the editor played.
fn cook_audio_file(c: &mut ChopCtx) -> ChopData {
    let get = |k: &str| c.node.param(k).map(|p| p.eval(c.eval));
    let file = get("file").map(|v| v.as_str()).unwrap_or_default();
    let looped = get("play").map(|v| v.as_str()).unwrap_or_default() != "once";
    let speed = get("speed").map(|v| v.as_f64()).unwrap_or(1.0);
    let volume = get("volume").map(|v| v.as_f32()).unwrap_or(1.0);
    let path = c.path.to_string();

    let silent = |c: &ChopCtx| {
        let n = slice_len(CONTROL_RATE, c.time);
        ChopData::new(vec![Channel::constant("chan1", 0.0, n)], CONTROL_RATE, true)
    };
    if file.trim().is_empty() {
        c.io.files.remove(&path);
        c.io.clear_note(&path);
        return silent(c);
    }

    let stale =
        c.io.files
            .get(&path)
            .map(|f| f.file != file)
            .unwrap_or(true);
    if stale {
        let resolved = match &c.io.base_dir {
            Some(dir) if !std::path::Path::new(&file).is_absolute() => dir.join(&file),
            _ => std::path::PathBuf::from(&file),
        };
        let loaded = std::fs::read(&resolved)
            .map_err(|e| format!("{}: {e}", resolved.display()))
            .and_then(|bytes| crate::wav::parse(&bytes));
        match loaded {
            Ok(wav) => {
                c.io.clear_note(&path);
                c.io.files.insert(
                    path.clone(),
                    AudioFile {
                        file: file.clone(),
                        wav,
                    },
                );
            }
            Err(e) => {
                c.io.note(&path, e);
                c.io.files.remove(&path);
            }
        }
    }

    let Some(loaded) = c.io.files.get(&path) else {
        return silent(c);
    };
    let wav = &loaded.wav;
    if wav.samples.is_empty() {
        return silent(c);
    }
    let len = wav.samples.len() as i64;
    let rate = wav.sample_rate;
    let n = slice_len(rate, c.time);
    let samples: Vec<f32> = crate::data::slice_times(c.time, n)
        .map(|t| {
            let index = (t * speed * rate).floor() as i64;
            let index = if looped {
                index.rem_euclid(len)
            } else if (0..len).contains(&index) {
                index
            } else {
                return 0.0;
            };
            wav.samples[index as usize] * volume
        })
        .collect();
    ChopData::new(vec![Channel::new("chan1", samples)], rate, true)
}

// ------------------------------------------------------------ audio out

struct AudioOut {
    /// Kept alive so the callback keeps running; never read directly.
    _stream: cpal::Stream,
    ring: Arc<Mutex<Vec<f32>>>,
    sample_rate: f64,
    device: String,
}

/// A quarter second of buffered output. Deeper would survive longer stalls;
/// this deep already means the latency stays inside what a live performer
/// stops noticing.
const OUT_CAP_SECONDS: f64 = 0.25;

impl AudioOut {
    fn open(requested: &str) -> Result<AudioOut, String> {
        let host = cpal::default_host();
        let device = if requested.trim().is_empty() {
            host.default_output_device()
                .ok_or("no default audio output device")?
        } else {
            host.output_devices()
                .map_err(|e| e.to_string())?
                .find(|d| device_name(d).map(|n| n == requested).unwrap_or(false))
                .ok_or_else(|| format!("no audio output device called `{requested}`"))?
        };
        let name = device_name(&device).unwrap_or_else(|| "?".to_string());
        let config = device
            .default_output_config()
            .map_err(|e| format!("{name}: {e}"))?;
        let sample_rate = config.sample_rate() as f64;
        let channels = config.channels() as usize;

        let ring: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let source = ring.clone();
        let stream = device
            .build_output_stream(
                config.config(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let Ok(mut buf) = source.lock() else {
                        data.fill(0.0);
                        return;
                    };
                    // Mono fanned to every hardware channel; underrun is
                    // silence, which is the only honest thing to play.
                    for frame in data.chunks_mut(channels.max(1)) {
                        let s = if buf.is_empty() { 0.0 } else { buf.remove(0) };
                        frame.fill(s);
                    }
                },
                move |err| log::warn!("audio output error: {err}"),
                None,
            )
            .map_err(|e| format!("{name}: {e}"))?;
        stream.play().map_err(|e| format!("{name}: {e}"))?;

        Ok(AudioOut {
            _stream: stream,
            ring,
            sample_rate,
            device: name,
        })
    }

    /// Queue a slice for the callback, resampling to the device rate.
    /// Overrun drops the *oldest* audio: latency must not grow all show.
    fn push(&self, samples: &[f32], input_rate: f64) {
        if samples.is_empty() {
            return;
        }
        let Ok(mut buf) = self.ring.lock() else {
            return;
        };
        let ratio = self.sample_rate / input_rate.max(1.0);
        let out_len = ((samples.len() as f64) * ratio).round().max(1.0) as usize;
        for i in 0..out_len {
            // Linear interpolation — for a resample this short, transparent.
            let pos = i as f64 / ratio;
            let a = pos.floor() as usize;
            let frac = (pos - a as f64) as f32;
            let s0 = samples[a.min(samples.len() - 1)];
            let s1 = samples[(a + 1).min(samples.len() - 1)];
            buf.push(s0 + (s1 - s0) * frac);
        }
        let cap = (self.sample_rate * OUT_CAP_SECONDS) as usize;
        if buf.len() > cap {
            let excess = buf.len() - cap;
            buf.drain(..excess);
        }
    }
}

fn params_audio_out() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "device".into(),
        Param::str("").with_label("Device (blank = default)"),
    );
    m.insert(
        "volume".into(),
        Param::float(1.0).with_label("Volume").with_range(0.0, 2.0),
    );
    m.insert("active".into(), Param::bool(true).with_label("Active"));
    m
}

fn cook_audio_out(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let get = |k: &str| c.node.param(k).map(|p| p.eval(c.eval));
    let requested = get("device").map(|v| v.as_str()).unwrap_or_default();
    let volume = get("volume").map(|v| v.as_f32()).unwrap_or(1.0);
    let active = get("active").map(|v| v.as_bool()).unwrap_or(true);
    let path = c.path.to_string();

    if !active || input.num_channels() == 0 {
        return input;
    }

    let needs_open = match c.io.audio_out.get(&path) {
        Some(a) => !requested.trim().is_empty() && a.device != requested,
        None => true,
    };
    if needs_open {
        match AudioOut::open(&requested) {
            Ok(a) => {
                c.io.clear_note(&path);
                c.io.audio_out.insert(path.clone(), a);
            }
            Err(e) => {
                c.io.note(&path, e);
                c.io.audio_out.remove(&path);
            }
        }
    }

    if let Some(out) = c.io.audio_out.get(&path) {
        if let Some(ch) = input.channels.first() {
            if volume == 1.0 {
                out.push(&ch.samples, input.sample_rate);
            } else {
                let scaled: Vec<f32> = ch.samples.iter().map(|s| s * volume).collect();
                out.push(&scaled, input.sample_rate);
            }
        }
    }
    input
}

// -------------------------------------------------------------- MIDI out

struct MidiOut {
    conn: midir::MidiOutputConnection,
    port: String,
    /// The last 7-bit value sent per channel name. MIDI is an event
    /// protocol, unlike DMX: re-sending an unchanged control every frame
    /// floods the port, so only changes go on the wire.
    sent: HashMap<String, u8>,
}

impl MidiOut {
    fn open(requested: &str) -> Result<MidiOut, String> {
        let output = midir::MidiOutput::new("OpenTouchDesigner").map_err(|e| e.to_string())?;
        let ports = output.ports();
        if ports.is_empty() {
            return Err("no MIDI output ports".into());
        }
        let port = if requested.trim().is_empty() {
            ports[0].clone()
        } else {
            ports
                .iter()
                .find(|p| {
                    output
                        .port_name(p)
                        .map(|n| n.contains(requested))
                        .unwrap_or(false)
                })
                .cloned()
                .ok_or_else(|| format!("no MIDI port matching `{requested}`"))?
        };
        let port_name = output.port_name(&port).unwrap_or_else(|_| "?".into());
        let conn = output
            .connect(&port, "otd-midi-out")
            .map_err(|e| e.to_string())?;
        Ok(MidiOut {
            conn,
            port: port_name,
            sent: HashMap::new(),
        })
    }
}

/// What a channel name means on the wire. The naming mirrors MIDI In, so a
/// patch that listens on `n36` can drive another machine's `n36` by wiring
/// the same channels back out.
enum MidiMapping {
    Note(u8),
    Control(u8),
}

fn midi_mapping(name: &str) -> Option<MidiMapping> {
    if let Some(rest) = name.strip_prefix("cc") {
        return rest
            .parse::<u8>()
            .ok()
            .filter(|n| *n < 128)
            .map(MidiMapping::Control);
    }
    if let Some(rest) = name.strip_prefix('n') {
        return rest
            .parse::<u8>()
            .ok()
            .filter(|n| *n < 128)
            .map(MidiMapping::Note);
    }
    None
}

/// The messages a slice's worth of channel values implies, given what was
/// already sent. Pure, so the wire format is testable with no port open.
fn midi_messages(
    channel: u8,
    values: &[(String, f32)],
    sent: &mut HashMap<String, u8>,
) -> Vec<[u8; 3]> {
    let ch = (channel.saturating_sub(1)) & 0x0f;
    let mut out = Vec::new();
    for (name, value) in values {
        let Some(mapping) = midi_mapping(name) else {
            continue;
        };
        let level = (value.clamp(0.0, 1.0) * 127.0).round() as u8;
        let previous = sent.get(name).copied();
        if previous == Some(level) {
            continue;
        }
        match mapping {
            MidiMapping::Note(note) => {
                // A note is a threshold, not a level: crossing zero is the
                // event. Velocity is the value at the moment it crossed.
                let was_on = previous.unwrap_or(0) > 0;
                let is_on = level > 0;
                match (was_on, is_on) {
                    (false, true) => out.push([0x90 | ch, note, level]),
                    (true, false) => out.push([0x80 | ch, note, 0]),
                    // Still held: a change of pressure is not a new note.
                    _ => {}
                }
            }
            MidiMapping::Control(cc) => out.push([0xb0 | ch, cc, level]),
        }
        sent.insert(name.clone(), level);
    }
    out
}

fn params_midi_out() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "device".into(),
        Param::str("").with_label("Port (blank = first)"),
    );
    m.insert(
        "channel".into(),
        Param::int(1).with_label("Channel").with_range(1.0, 16.0),
    );
    m.insert("active".into(), Param::bool(true).with_label("Active"));
    m
}

fn cook_midi_out(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let get = |k: &str| c.node.param(k).map(|p| p.eval(c.eval));
    let requested = get("device").map(|v| v.as_str()).unwrap_or_default();
    let channel = get("channel").map(|v| v.as_i64()).unwrap_or(1).clamp(1, 16) as u8;
    let active = get("active").map(|v| v.as_bool()).unwrap_or(true);
    let path = c.path.to_string();

    if !active || input.num_channels() == 0 {
        return input;
    }

    let needs_open = match c.io.midi_out.get(&path) {
        Some(m) => !requested.trim().is_empty() && !m.port.contains(&requested),
        None => true,
    };
    if needs_open {
        match MidiOut::open(&requested) {
            Ok(m) => {
                c.io.clear_note(&path);
                c.io.midi_out.insert(path.clone(), m);
            }
            Err(e) => {
                c.io.note(&path, e);
                c.io.midi_out.remove(&path);
            }
        }
    }

    if let Some(out) = c.io.midi_out.get_mut(&path) {
        let values: Vec<(String, f32)> = input
            .channels
            .iter()
            .map(|ch| (ch.name.clone(), ch.last()))
            .collect();
        for message in midi_messages(channel, &values, &mut out.sent) {
            if let Err(e) = out.conn.send(&message) {
                c.io.note(&path, format!("MIDI send: {e}"));
                break;
            }
        }
    }
    input
}

// ------------------------------------------------------------- spectrum

struct Spectrum {
    window: Vec<f32>,
    planner: rustfft::FftPlanner<f32>,
}

fn params_spectrum() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "size".into(),
        Param::menu("1024", &["256", "512", "1024", "2048", "4096"]).with_label("FFT Size"),
    );
    m.insert(
        "bands".into(),
        Param::int(4).with_label("Bands").with_range(1.0, 32.0),
    );
    m.insert(
        "gain".into(),
        Param::float(1.0).with_label("Gain").with_range(0.0, 64.0),
    );
    m.insert("decibels".into(), Param::bool(false).with_label("Decibels"));
    m
}

fn cook_spectrum(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let size: usize = c
        .node
        .param("size")
        .map(|p| p.eval(c.eval).as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024);
    let bands = c
        .node
        .param("bands")
        .map(|p| p.eval(c.eval).as_i64())
        .unwrap_or(4)
        .clamp(1, 32) as usize;
    let gain = c
        .node
        .param("gain")
        .map(|p| p.eval(c.eval).as_f32())
        .unwrap_or(1.0);
    let decibels = c
        .node
        .param("decibels")
        .map(|p| p.eval(c.eval).as_bool())
        .unwrap_or(false);

    let path = c.path.to_string();
    let entry = c.io.spectra.entry(path).or_insert_with(|| Spectrum {
        window: Vec::new(),
        planner: rustfft::FftPlanner::new(),
    });

    // Keep a rolling window of the most recent `size` samples across frames,
    // so the transform has enough history even at 60 fps slices.
    if let Some(ch) = input.channels.first() {
        entry.window.extend_from_slice(&ch.samples);
    }
    if entry.window.len() > size {
        let excess = entry.window.len() - size;
        entry.window.drain(..excess);
    }

    let n = slice_len(input.sample_rate.max(CONTROL_RATE), c.time).min(64);
    if entry.window.len() < size {
        let channels = (0..bands)
            .map(|i| Channel::constant(format!("band{}", i + 1), 0.0, n))
            .collect();
        return ChopData::new(channels, CONTROL_RATE, true);
    }

    // Hann window, then a real FFT via the complex one — at these sizes the
    // simplicity is worth more than the factor of two.
    let fft = entry.planner.plan_fft_forward(size);
    let mut buffer: Vec<rustfft::num_complex::Complex<f32>> = entry
        .window
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (size - 1) as f32).cos();
            rustfft::num_complex::Complex::new(s * w, 0.0)
        })
        .collect();
    fft.process(&mut buffer);

    let half = size / 2;
    let magnitudes: Vec<f32> = buffer[..half]
        .iter()
        .map(|c| c.norm() * 2.0 / size as f32)
        .collect();

    // Logarithmic band edges: an even split would put almost everything in
    // the top band, which is useless for driving visuals from music.
    let channels = (0..bands)
        .map(|b| {
            let lo_f = (b as f32 / bands as f32).powf(3.0);
            let hi_f = ((b + 1) as f32 / bands as f32).powf(3.0);
            let lo = (lo_f * half as f32) as usize;
            let hi = ((hi_f * half as f32) as usize).max(lo + 1).min(half);
            let energy: f32 = magnitudes[lo..hi].iter().sum::<f32>() / (hi - lo).max(1) as f32;
            let mut v = energy * gain;
            if decibels {
                v = 20.0 * (v.max(1e-6)).log10();
            }
            Channel::constant(format!("band{}", b + 1), v, n)
        })
        .collect();
    ChopData::new(channels, CONTROL_RATE, true)
}

// --------------------------------------------------------------- MIDI in

struct MidiState {
    notes: Box<[f32; 128]>,
    ccs: Box<[f32; 128]>,
    /// Controls that have actually been touched, in arrival order, so the
    /// CHOP shows the knobs you moved and not 128 empty channels.
    seen_ccs: Vec<u8>,
    last_note: f32,
    velocity: f32,
    pitchbend: f32,
}

impl Default for MidiState {
    fn default() -> Self {
        MidiState {
            notes: Box::new([0.0; 128]),
            ccs: Box::new([0.0; 128]),
            seen_ccs: Vec::new(),
            last_note: 0.0,
            velocity: 0.0,
            pitchbend: 0.0,
        }
    }
}

struct MidiIn {
    _conn: midir::MidiInputConnection<()>,
    state: Arc<Mutex<MidiState>>,
    port: String,
}

impl MidiIn {
    fn open(requested: &str) -> Result<MidiIn, String> {
        let input = midir::MidiInput::new("OpenTouchDesigner").map_err(|e| e.to_string())?;
        let ports = input.ports();
        if ports.is_empty() {
            return Err("no MIDI input ports".into());
        }
        let port = if requested.trim().is_empty() {
            ports[0].clone()
        } else {
            ports
                .iter()
                .find(|p| {
                    input
                        .port_name(p)
                        .map(|n| n.contains(requested))
                        .unwrap_or(false)
                })
                .cloned()
                .ok_or_else(|| format!("no MIDI port matching `{requested}`"))?
        };
        let port_name = input.port_name(&port).unwrap_or_else(|_| "?".into());

        let state = Arc::new(Mutex::new(MidiState::default()));
        let sink = state.clone();
        let conn = input
            .connect(
                &port,
                "otd-midi-in",
                move |_stamp, message, _| {
                    let Ok(mut s) = sink.lock() else { return };
                    apply_midi(&mut s, message);
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        Ok(MidiIn {
            _conn: conn,
            state,
            port: port_name,
        })
    }
}

/// Decode the handful of messages a visual patch actually uses.
fn apply_midi(s: &mut MidiState, message: &[u8]) {
    if message.len() < 2 {
        return;
    }
    let status = message[0] & 0xf0;
    let d1 = message[1] & 0x7f;
    let d2 = message.get(2).copied().unwrap_or(0) & 0x7f;
    match status {
        // Note on with zero velocity is a note off, by convention.
        0x90 if d2 > 0 => {
            s.notes[d1 as usize] = d2 as f32 / 127.0;
            s.last_note = d1 as f32;
            s.velocity = d2 as f32 / 127.0;
        }
        0x80 | 0x90 => {
            s.notes[d1 as usize] = 0.0;
            if s.last_note == d1 as f32 {
                s.velocity = 0.0;
            }
        }
        0xb0 => {
            s.ccs[d1 as usize] = d2 as f32 / 127.0;
            if !s.seen_ccs.contains(&d1) && s.seen_ccs.len() < 32 {
                s.seen_ccs.push(d1);
            }
        }
        0xe0 => {
            let raw = ((d2 as i32) << 7 | d1 as i32) - 8192;
            s.pitchbend = raw as f32 / 8192.0;
        }
        _ => {}
    }
}

fn params_midi_in() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "device".into(),
        Param::str("").with_label("Port (blank = first)"),
    );
    m.insert(
        "notes".into(),
        Param::str("").with_label("Notes (e.g. 36 38 42)"),
    );
    m
}

fn cook_midi_in(c: &mut ChopCtx) -> ChopData {
    let requested = c
        .node
        .param("device")
        .map(|p| p.eval(c.eval).as_str())
        .unwrap_or_default();
    let wanted_notes: Vec<u8> = c
        .node
        .param("notes")
        .map(|p| p.eval(c.eval).as_str())
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    let path = c.path.to_string();
    let n = slice_len(CONTROL_RATE, c.time);

    let needs_open = match c.io.midi.get(&path) {
        Some(m) => !requested.trim().is_empty() && !m.port.contains(&requested),
        None => true,
    };
    if needs_open {
        match MidiIn::open(&requested) {
            Ok(m) => {
                c.io.clear_note(&path);
                c.io.midi.insert(path.clone(), m);
            }
            Err(e) => {
                c.io.note(&path, e);
                c.io.midi.remove(&path);
            }
        }
    }

    let mut channels = Vec::new();
    if let Some(midi) = c.io.midi.get(&path) {
        if let Ok(s) = midi.state.lock() {
            channels.push(Channel::constant("note", s.last_note, n));
            channels.push(Channel::constant("velocity", s.velocity, n));
            channels.push(Channel::constant("pitchbend", s.pitchbend, n));
            for cc in &s.seen_ccs {
                channels.push(Channel::constant(format!("cc{cc}"), s.ccs[*cc as usize], n));
            }
            for note in &wanted_notes {
                channels.push(Channel::constant(
                    format!("n{note}"),
                    s.notes[*note as usize % 128],
                    n,
                ));
            }
        }
    }
    if channels.is_empty() {
        channels.push(Channel::constant("note", 0.0, n));
        channels.push(Channel::constant("velocity", 0.0, n));
        channels.push(Channel::constant("pitchbend", 0.0, n));
        for note in &wanted_notes {
            channels.push(Channel::constant(format!("n{note}"), 0.0, n));
        }
    }
    ChopData::new(channels, CONTROL_RATE, true)
}

// ---------------------------------------------------------------- OSC in

struct OscIn {
    values: Arc<Mutex<Vec<(String, f32)>>>,
    stop: Arc<AtomicBool>,
    port: u16,
}

impl Drop for OscIn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl OscIn {
    fn open(port: u16) -> Result<OscIn, String> {
        let socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port)))
            .map_err(|e| format!("OSC port {port}: {e}"))?;
        socket
            .set_read_timeout(Some(std::time::Duration::from_millis(200)))
            .map_err(|e| e.to_string())?;

        let values: Arc<Mutex<Vec<(String, f32)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = values.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();

        std::thread::Builder::new()
            .name(format!("otd-osc-in-{port}"))
            .spawn(move || {
                let mut buf = [0u8; 4096];
                while !stop_flag.load(Ordering::Relaxed) {
                    let Ok((len, _)) = socket.recv_from(&mut buf) else {
                        continue;
                    };
                    let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..len]) else {
                        continue;
                    };
                    let Ok(mut values) = sink.lock() else { break };
                    collect_osc(&packet, &mut values);
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(OscIn { values, stop, port })
    }
}

/// Flatten a packet into `(channel name, value)` pairs. An address with
/// several arguments becomes `addr_1`, `addr_2`, ...
fn collect_osc(packet: &rosc::OscPacket, out: &mut Vec<(String, f32)>) {
    match packet {
        rosc::OscPacket::Bundle(b) => {
            for p in &b.content {
                collect_osc(p, out);
            }
        }
        rosc::OscPacket::Message(m) => {
            let base = m.addr.trim_start_matches('/').replace('/', "_");
            for (i, arg) in m.args.iter().enumerate() {
                let value = match arg {
                    rosc::OscType::Float(f) => *f,
                    rosc::OscType::Double(d) => *d as f32,
                    rosc::OscType::Int(i) => *i as f32,
                    rosc::OscType::Long(l) => *l as f32,
                    rosc::OscType::Bool(b) => {
                        if *b {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    _ => continue,
                };
                let name = if m.args.len() == 1 {
                    base.clone()
                } else {
                    format!("{base}_{}", i + 1)
                };
                match out.iter_mut().find(|(n, _)| *n == name) {
                    Some(slot) => slot.1 = value,
                    None => out.push((name, value)),
                }
            }
        }
    }
}

fn params_osc_in() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "port".into(),
        Param::int(9000).with_label("Port").with_range(1.0, 65535.0),
    );
    m
}

fn cook_osc_in(c: &mut ChopCtx) -> ChopData {
    let port = c
        .node
        .param("port")
        .map(|p| p.eval(c.eval).as_i64())
        .unwrap_or(9000)
        .clamp(1, 65535) as u16;
    let path = c.path.to_string();
    let n = slice_len(CONTROL_RATE, c.time);

    let needs_open =
        c.io.osc_in
            .get(&path)
            .map(|o| o.port != port)
            .unwrap_or(true);
    if needs_open {
        // Drop the old listener before binding the new port.
        c.io.osc_in.remove(&path);
        match OscIn::open(port) {
            Ok(o) => {
                c.io.clear_note(&path);
                c.io.osc_in.insert(path.clone(), o);
            }
            Err(e) => c.io.note(&path, e),
        }
    }

    let mut channels = Vec::new();
    if let Some(osc) = c.io.osc_in.get(&path) {
        if let Ok(values) = osc.values.lock() {
            for (name, value) in values.iter() {
                channels.push(Channel::constant(name.clone(), *value, n));
            }
        }
    }
    if channels.is_empty() {
        channels.push(Channel::constant("chan1", 0.0, n));
    }
    ChopData::new(channels, CONTROL_RATE, true)
}

// --------------------------------------------------------------- OSC out

struct OscOut {
    socket: UdpSocket,
    target: SocketAddr,
}

fn params_osc_out() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "address".into(),
        Param::str("127.0.0.1").with_label("Address"),
    );
    m.insert(
        "port".into(),
        Param::int(9001).with_label("Port").with_range(1.0, 65535.0),
    );
    m.insert("path".into(), Param::str("/otd").with_label("OSC Path"));
    m.insert("active".into(), Param::bool(true).with_label("Active"));
    m
}

fn cook_osc_out(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let get = |k: &str| c.node.param(k).map(|p| p.eval(c.eval));
    let host = get("address").map(|v| v.as_str()).unwrap_or_default();
    let port = get("port")
        .map(|v| v.as_i64())
        .unwrap_or(9001)
        .clamp(1, 65535) as u16;
    let osc_path = get("path").map(|v| v.as_str()).unwrap_or_default();
    let active = get("active").map(|v| v.as_bool()).unwrap_or(true);
    let path = c.path.to_string();

    if !active || input.num_channels() == 0 {
        return input;
    }
    let Ok(target) = format!("{}:{}", host.trim(), port).parse::<SocketAddr>() else {
        c.io.note(&path, format!("bad OSC address `{host}:{port}`"));
        return input;
    };

    let needs_open =
        c.io.osc_out
            .get(&path)
            .map(|o| o.target != target)
            .unwrap_or(true);
    if needs_open {
        match UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))) {
            Ok(socket) => {
                c.io.clear_note(&path);
                c.io.osc_out.insert(path.clone(), OscOut { socket, target });
            }
            Err(e) => c.io.note(&path, e.to_string()),
        }
    }

    if let Some(out) = c.io.osc_out.get(&path) {
        // One message per frame carrying every channel's current value —
        // sending each sample would flood the socket to no benefit.
        let args: Vec<rosc::OscType> = input
            .channels
            .iter()
            .map(|ch| rosc::OscType::Float(ch.last()))
            .collect();
        let message = rosc::OscPacket::Message(rosc::OscMessage {
            addr: if osc_path.starts_with('/') {
                osc_path.clone()
            } else {
                format!("/{osc_path}")
            },
            args,
        });
        if let Ok(bytes) = rosc::encoder::encode(&message) {
            let _ = out.socket.send_to(&bytes, out.target);
        }
    }
    input
}

// ------------------------------------------------------------- DMX out

struct DmxOut {
    socket: UdpSocket,
    target: SocketAddr,
    sequence: u8,
    cid: [u8; 16],
}

fn params_dmx_out() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "protocol".into(),
        Param::menu("artnet", &["artnet", "sacn"]).with_label("Protocol"),
    );
    m.insert(
        "address".into(),
        Param::str("").with_label("Address (blank = broadcast / multicast)"),
    );
    m.insert(
        "universe".into(),
        Param::int(0)
            .with_label("Universe")
            .with_range(0.0, 32767.0),
    );
    m.insert(
        "start".into(),
        Param::int(1)
            .with_label("Start Channel")
            .with_range(1.0, 512.0),
    );
    m.insert("active".into(), Param::bool(true).with_label("Active"));
    m
}

fn cook_dmx_out(c: &mut ChopCtx) -> ChopData {
    let input = c.input(0).clone();
    let get = |k: &str| c.node.param(k).map(|p| p.eval(c.eval));
    if !get("active").map(|v| v.as_bool()).unwrap_or(true) || input.num_channels() == 0 {
        return input;
    }
    let sacn = c
        .node
        .param("protocol")
        .and_then(|p| {
            let chosen = p.eval(c.eval).as_str();
            p.menu.as_ref().map(|m| m.iter().position(|i| *i == chosen))
        })
        .flatten()
        .unwrap_or(0)
        == 1;
    let universe = get("universe")
        .map(|v| v.as_i64())
        .unwrap_or(0)
        .clamp(0, 32767) as u16;
    let start = get("start").map(|v| v.as_i64()).unwrap_or(1).clamp(1, 512) as usize;
    let host = get("address").map(|v| v.as_str()).unwrap_or_default();
    let path = c.path.to_string();

    // DMX is a state protocol: each frame carries the current level of every
    // slot, which is the last sample of each channel.
    let mut slots = vec![0u8; crate::dmx::UNIVERSE_SIZE];
    for (i, ch) in input.channels.iter().enumerate() {
        let slot = start - 1 + i;
        if slot < slots.len() {
            slots[slot] = crate::dmx::to_slot(ch.last());
        }
    }
    let used = (start - 1 + input.num_channels()).min(crate::dmx::UNIVERSE_SIZE);

    let target: SocketAddr = if host.trim().is_empty() {
        if sacn {
            SocketAddr::from((crate::dmx::sacn_multicast(universe), crate::dmx::SACN_PORT))
        } else {
            // Art-Net's convention is a directed broadcast.
            SocketAddr::from(([255, 255, 255, 255], crate::dmx::ARTNET_PORT))
        }
    } else {
        let port = if sacn {
            crate::dmx::SACN_PORT
        } else {
            crate::dmx::ARTNET_PORT
        };
        match format!("{}:{port}", host.trim()).parse() {
            Ok(a) => a,
            Err(_) => {
                c.io.note(&path, format!("bad DMX address `{host}`"));
                return input;
            }
        }
    };

    if !c.io.dmx.contains_key(&path) {
        match UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))) {
            Ok(socket) => {
                // Art-Net expects broadcast; without this the send fails on
                // every platform that enforces it.
                let _ = socket.set_broadcast(true);
                let mut cid = [0u8; 16];
                for (i, b) in path.bytes().take(16).enumerate() {
                    cid[i] = b;
                }
                c.io.clear_note(&path);
                c.io.dmx.insert(
                    path.clone(),
                    DmxOut {
                        socket,
                        target,
                        sequence: 0,
                        cid,
                    },
                );
            }
            Err(e) => c.io.note(&path, e.to_string()),
        }
    }

    if let Some(out) = c.io.dmx.get_mut(&path) {
        out.target = target;
        out.sequence = out.sequence.wrapping_add(1);
        let packet = if sacn {
            crate::dmx::sacn_packet(
                universe,
                out.sequence,
                "OpenTouchDesigner",
                &out.cid,
                &slots[..used],
            )
        } else {
            crate::dmx::artnet_packet(universe, out.sequence, &slots[..used])
        };
        if let Err(e) = out.socket.send_to(&packet, out.target) {
            c.io.note(&path, format!("DMX send: {e}"));
        }
    }
    input
}

// ------------------------------------------------------- mouse, keyboard

fn params_mouse_in() -> IndexMap<String, Param> {
    IndexMap::new()
}

fn cook_mouse_in(c: &mut ChopCtx) -> ChopData {
    let n = slice_len(CONTROL_RATE, c.time);
    let s = c.io.input.clone();
    ChopData::new(
        vec![
            Channel::constant("tx", s.mouse[0], n),
            Channel::constant("ty", s.mouse[1], n),
            Channel::constant("button1", s.buttons[0] as i32 as f32, n),
            Channel::constant("button2", s.buttons[1] as i32 as f32, n),
            Channel::constant("button3", s.buttons[2] as i32 as f32, n),
            Channel::constant("wheel", s.wheel, n),
        ],
        CONTROL_RATE,
        true,
    )
}

fn params_keyboard_in() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "keys".into(),
        Param::str("a s d f").with_label("Keys (space separated)"),
    );
    m
}

fn cook_keyboard_in(c: &mut ChopCtx) -> ChopData {
    let n = slice_len(CONTROL_RATE, c.time);
    let keys = c
        .node
        .param("keys")
        .map(|p| p.eval(c.eval).as_str())
        .unwrap_or_default();
    let held = c.io.input.keys.clone();
    let channels: Vec<Channel> = keys
        .split_whitespace()
        .map(|k| {
            let down = held.iter().any(|h| h.eq_ignore_ascii_case(k));
            Channel::constant(k, down as i32 as f32, n)
        })
        .collect();
    let channels = if channels.is_empty() {
        vec![Channel::constant("chan1", 0.0, n)]
    } else {
        channels
    };
    ChopData::new(channels, CONTROL_RATE, true)
}

// ------------------------------------------------------------ the table

pub const AUDIO_IN: &str = "audiodeviceinCHOP";
pub const AUDIO_OUT: &str = "audiodeviceoutCHOP";
pub const AUDIO_FILE: &str = "audiofileinCHOP";
pub const MIDI_IN: &str = "midiinCHOP";
pub const MIDI_OUT: &str = "midioutCHOP";
pub const OSC_IN: &str = "oscinCHOP";
pub const OSC_OUT: &str = "oscoutCHOP";
pub const SPECTRUM: &str = "audiospectrumCHOP";
pub const DMX_OUT: &str = "dmxoutCHOP";

pub(crate) fn specs() -> Vec<ChopSpec> {
    vec![
        spec_animated(
            AUDIO_IN,
            "Audio Device In",
            &[],
            "Samples from an audio input, downmixed to mono.",
            params_audio_in,
            cook_audio_in,
        ),
        spec_animated(
            AUDIO_OUT,
            "Audio Device Out",
            &["in"],
            "Plays its first channel on an audio output.",
            params_audio_out,
            cook_audio_out,
        ),
        spec_animated(
            AUDIO_FILE,
            "Audio File In",
            &[],
            "Plays a WAV file, following the timeline.",
            params_audio_file,
            cook_audio_file,
        ),
        spec_animated(
            SPECTRUM,
            "Audio Spectrum",
            &["in"],
            "Energy per frequency band, log spaced.",
            params_spectrum,
            cook_spectrum,
        ),
        spec_animated(
            MIDI_IN,
            "MIDI In",
            &[],
            "Notes, velocity, pitch bend and the controls you have moved.",
            params_midi_in,
            cook_midi_in,
        ),
        spec_animated(
            MIDI_OUT,
            "MIDI Out",
            &["in"],
            "Sends `nNN` channels as notes and `ccNN` channels as controls.",
            params_midi_out,
            cook_midi_out,
        ),
        spec_animated(
            OSC_IN,
            "OSC In",
            &[],
            "Incoming OSC messages, one channel per address argument.",
            params_osc_in,
            cook_osc_in,
        ),
        spec_animated(
            OSC_OUT,
            "OSC Out",
            &["in"],
            "Sends its channels as one OSC message per frame.",
            params_osc_out,
            cook_osc_out,
        ),
        spec_animated(
            DMX_OUT,
            "DMX Out",
            &["in"],
            "Sends its channels as DMX over Art-Net or sACN.",
            params_dmx_out,
            cook_dmx_out,
        ),
        spec_animated(
            "mouseinCHOP",
            "Mouse In",
            &[],
            "Cursor position and buttons.",
            params_mouse_in,
            cook_mouse_in,
        ),
        spec_animated(
            "keyboardinCHOP",
            "Keyboard In",
            &[],
            "One channel per named key, 1 while held.",
            params_keyboard_in,
            cook_keyboard_in,
        ),
    ]
}

/// Every audio input device the host can see, for the parameter menu.
pub fn audio_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|ds| ds.filter_map(|d| device_name(&d)).collect())
        .unwrap_or_default()
}

/// Every audio output device the host can see, for the parameter menu.
pub fn audio_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|ds| ds.filter_map(|d| device_name(&d)).collect())
        .unwrap_or_default()
}

/// Every MIDI output port, for the parameter menu.
pub fn midi_output_ports() -> Vec<String> {
    let Ok(output) = midir::MidiOutput::new("OpenTouchDesigner") else {
        return Vec::new();
    };
    output
        .ports()
        .iter()
        .filter_map(|p| output.port_name(p).ok())
        .collect()
}

/// Every MIDI input port, for the parameter menu.
pub fn midi_input_ports() -> Vec<String> {
    let Ok(input) = midir::MidiInput::new("OpenTouchDesigner") else {
        return Vec::new();
    };
    input
        .ports()
        .iter()
        .filter_map(|p| input.port_name(p).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_notes_and_controls_are_decoded() {
        let mut s = MidiState::default();
        apply_midi(&mut s, &[0x90, 60, 100]);
        assert_eq!(s.last_note, 60.0);
        assert!((s.velocity - 100.0 / 127.0).abs() < 1e-6);
        assert!(s.notes[60] > 0.0);

        // Note on with velocity 0 is a note off, by convention.
        apply_midi(&mut s, &[0x90, 60, 0]);
        assert_eq!(s.notes[60], 0.0);
        assert_eq!(s.velocity, 0.0);

        apply_midi(&mut s, &[0xb0, 74, 127]);
        assert_eq!(s.ccs[74], 1.0);
        assert_eq!(s.seen_ccs, vec![74]);
        // A control already seen is not listed twice.
        apply_midi(&mut s, &[0xb0, 74, 0]);
        assert_eq!(s.seen_ccs, vec![74]);

        // Pitch bend is 14-bit, centred.
        apply_midi(&mut s, &[0xe0, 0, 64]);
        assert!(s.pitchbend.abs() < 1e-6);
        apply_midi(&mut s, &[0xe0, 127, 127]);
        assert!(s.pitchbend > 0.99);
    }

    #[test]
    fn a_short_or_unknown_midi_message_is_ignored() {
        let mut s = MidiState::default();
        apply_midi(&mut s, &[0x90]);
        apply_midi(&mut s, &[]);
        apply_midi(&mut s, &[0xf8, 0, 0]);
        assert_eq!(s.last_note, 0.0);
    }

    #[test]
    fn midi_out_sends_changes_and_only_changes() {
        let mut sent = HashMap::new();

        // A note crossing zero is note-on with the value as velocity.
        let on = midi_messages(1, &[("n36".into(), 1.0)], &mut sent);
        assert_eq!(on, vec![[0x90, 36, 127]]);

        // Held at a different pressure: not a new note, nothing sent.
        assert!(midi_messages(1, &[("n36".into(), 0.5)], &mut sent).is_empty());

        // Released: note-off.
        let off = midi_messages(1, &[("n36".into(), 0.0)], &mut sent);
        assert_eq!(off, vec![[0x80, 36, 0]]);

        // A control sends on change and stays quiet otherwise — MIDI is an
        // event protocol; a value repeated every frame must not be.
        let cc = midi_messages(1, &[("cc74".into(), 0.5)], &mut sent);
        assert_eq!(cc, vec![[0xb0, 74, 64]]);
        assert!(midi_messages(1, &[("cc74".into(), 0.5)], &mut sent).is_empty());

        // Channel 10 lands in the status nibble; unmapped names are ignored.
        let ten = midi_messages(10, &[("cc1".into(), 1.0), ("chan1".into(), 1.0)], &mut sent);
        assert_eq!(ten, vec![[0xb9, 1, 127]]);
    }

    #[test]
    fn midi_out_names_mirror_midi_in() {
        assert!(matches!(midi_mapping("n60"), Some(MidiMapping::Note(60))));
        assert!(matches!(
            midi_mapping("cc127"),
            Some(MidiMapping::Control(127))
        ));
        assert!(midi_mapping("cc128").is_none());
        assert!(midi_mapping("note").is_none());
        assert!(midi_mapping("velocity").is_none());
    }

    #[test]
    fn osc_addresses_become_channel_names() {
        let mut out = Vec::new();
        collect_osc(
            &rosc::OscPacket::Message(rosc::OscMessage {
                addr: "/live/track/2".into(),
                args: vec![rosc::OscType::Float(0.5)],
            }),
            &mut out,
        );
        assert_eq!(out, vec![("live_track_2".to_string(), 0.5)]);

        // Several arguments are numbered; a repeat updates in place rather
        // than growing the channel list forever.
        collect_osc(
            &rosc::OscPacket::Message(rosc::OscMessage {
                addr: "/xy".into(),
                args: vec![rosc::OscType::Float(0.1), rosc::OscType::Int(2)],
            }),
            &mut out,
        );
        assert_eq!(out.len(), 3);
        collect_osc(
            &rosc::OscPacket::Message(rosc::OscMessage {
                addr: "/xy".into(),
                args: vec![rosc::OscType::Float(0.9), rosc::OscType::Int(3)],
            }),
            &mut out,
        );
        assert_eq!(out.len(), 3);
        assert_eq!(out[1], ("xy_1".to_string(), 0.9));
    }
}
