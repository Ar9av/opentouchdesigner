//! Beat Detect CHOP: the beats in the music, not a tempo you typed.
//!
//! The Beat CHOP is a clock; it keeps 120 bpm whether or not anything is
//! playing. This listens. Kicks live in the low end, so the input is
//! low-passed, its energy taken every 10 ms, and a hop whose energy jumps
//! well above the last second's average is an onset — unless one fired too
//! recently to be a separate beat. The gaps between onsets give the tempo,
//! and the tempo gives a phase that keeps ramping between beats, so a loop
//! can be locked to the bar rather than merely kicked by it.

use otd_core::Param;
use otd_core::indexmap::IndexMap;

use crate::data::{CONTROL_RATE, Channel, ChopData, slice_len};
use crate::ops::ChopCtx;

pub const BEAT_DETECT: &str = "beatdetectCHOP";

/// One second of 10 ms hops: the average a beat has to stand out from.
const HISTORY: usize = 100;
/// The gaps the tempo is estimated from.
const GAPS: usize = 8;

#[derive(Default)]
pub(crate) struct Detector {
    rate: f64,
    lowpass: f32,
    hop: Vec<f32>,
    full: f64,
    energies: Vec<f32>,
    gaps: Vec<f64>,
    /// Seconds of audio seen.
    clock: f64,
    last_beat: Option<f64>,
    /// Slow-following peaks, so `bass` and `level` read 0–1 at any volume.
    bass_peak: f32,
    level_peak: f32,
    bass: f32,
    level: f32,
    pub count: i64,
}

pub(crate) struct Settings {
    pub sensitivity: f32,
    pub cutoff: f32,
    pub mingap: f64,
}

impl Detector {
    /// Feed one slice of samples; returns how many beats fired in it.
    pub fn feed(&mut self, samples: &[f32], rate: f64, s: &Settings) -> u32 {
        if rate != self.rate {
            *self = Detector {
                rate,
                count: self.count,
                ..Default::default()
            };
        }
        let hop_len = (rate / 100.0).round().max(1.0) as usize;
        let a = 1.0 - (-std::f32::consts::TAU * s.cutoff / rate as f32).exp();
        let mut fired = 0;
        for &x in samples {
            self.lowpass += a * (x - self.lowpass);
            self.hop.push(self.lowpass * self.lowpass);
            self.full += (x * x) as f64;
            self.clock += 1.0 / rate;
            if self.hop.len() < hop_len {
                continue;
            }
            let e = self.hop.iter().sum::<f32>() / hop_len as f32;
            let rms = (self.full / hop_len as f64).sqrt() as f32;
            self.hop.clear();
            self.full = 0.0;

            let mean = if self.energies.is_empty() {
                f32::MAX
            } else {
                self.energies.iter().sum::<f32>() / self.energies.len() as f32
            };
            let rising = self.energies.last().is_none_or(|&p| e > p);
            let apart = self.last_beat.is_none_or(|t| self.clock - t >= s.mingap);
            if rising && apart && e > mean * s.sensitivity && e > 1e-6 {
                if let Some(t) = self.last_beat {
                    self.gaps.push(self.clock - t);
                    if self.gaps.len() > GAPS {
                        self.gaps.remove(0);
                    }
                }
                self.last_beat = Some(self.clock);
                self.count += 1;
                fired += 1;
            }
            self.energies.push(e);
            if self.energies.len() > HISTORY {
                self.energies.remove(0);
            }

            let b = e.sqrt();
            self.bass_peak = (self.bass_peak * 0.999).max(b);
            self.level_peak = (self.level_peak * 0.999).max(rms);
            self.bass = b / self.bass_peak.max(1e-6);
            self.level = rms / self.level_peak.max(1e-6);
        }
        fired
    }

    /// Beats per minute from the median gap, folded into 70–180 so a
    /// detector that hears every other kick still reports the song's tempo.
    pub fn bpm(&self) -> f64 {
        if self.gaps.is_empty() {
            return 0.0;
        }
        let mut g = self.gaps.clone();
        g.sort_by(f64::total_cmp);
        let mut bpm = 60.0 / g[g.len() / 2];
        while bpm > 0.0 && bpm < 70.0 {
            bpm *= 2.0;
        }
        while bpm > 180.0 {
            bpm /= 2.0;
        }
        bpm
    }

    pub fn since_beat(&self) -> Option<f64> {
        self.last_beat.map(|t| self.clock - t)
    }
}

pub(crate) fn params() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "sensitivity".into(),
        Param::float(1.6)
            .with_label("Threshold")
            .with_range(1.05, 4.0),
    );
    m.insert(
        "decay".into(),
        Param::float(0.25)
            .with_label("Kick Decay")
            .with_range(0.02, 2.0),
    );
    m.insert(
        "cutoff".into(),
        Param::float(150.0)
            .with_label("Bass Cutoff")
            .with_range(40.0, 1000.0),
    );
    m.insert(
        "maxbpm".into(),
        Param::float(200.0)
            .with_label("Max BPM")
            .with_range(60.0, 400.0),
    );
    m
}

pub(crate) fn cook(c: &mut ChopCtx) -> ChopData {
    let settings = Settings {
        sensitivity: c.f("sensitivity").max(1.0),
        cutoff: c.f("cutoff").max(10.0),
        mingap: 60.0 / c.f("maxbpm").max(1.0) as f64,
    };
    let decay = c.f("decay").max(1e-3) as f64;
    let input = c.input(0).clone();
    let det = c.io.beats.entry(c.path.to_string()).or_default();
    let fired = match input.channels.first() {
        Some(ch) => det.feed(&ch.samples, input.sample_rate.max(1.0), &settings),
        None => 0,
    };
    let bpm = det.bpm();
    let since = det.since_beat();
    let kick = since.map_or(0.0, |s| (-s / decay).exp()) as f32;
    let phase = match (since, bpm > 0.0) {
        (Some(s), true) => (s * bpm / 60.0).fract(),
        _ => 0.0,
    } as f32;

    let n = slice_len(CONTROL_RATE, c.time);
    let ch = |name: &str, v: f32| Channel::constant(name, v, n);
    let channels = vec![
        ch("beat", if fired > 0 { 1.0 } else { 0.0 }),
        ch("kick", kick),
        ch("bass", det.bass),
        ch("level", det.level),
        ch("bpm", bpm as f32),
        ch("phase", phase),
        ch("count", det.count as f32),
    ];
    ChopData::new(channels, CONTROL_RATE, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four-on-the-floor at `bpm`: a decaying 55 Hz thump on every beat,
    /// over a hi-hat-ish hiss that must not count as a beat.
    fn track(bpm: f64, seconds: f64, rate: f64) -> Vec<f32> {
        let period = 60.0 / bpm;
        let mut seed = 1u32;
        (0..(seconds * rate) as usize)
            .map(|i| {
                let t = i as f64 / rate;
                let since = t % period;
                let kick = (std::f64::consts::TAU * 55.0 * since).sin() * (-since * 18.0).exp();
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let hiss = (seed >> 9) as f64 / (1u32 << 23) as f64 - 0.5;
                (kick * 0.8 + hiss * 0.1) as f32
            })
            .collect()
    }

    #[test]
    fn hears_the_kick_and_the_tempo() {
        let rate = 44100.0;
        let audio = track(128.0, 8.0, rate);
        let s = Settings {
            sensitivity: 1.6,
            cutoff: 150.0,
            mingap: 0.3,
        };
        let mut d = Detector::default();
        // 60 fps slices, the way a cook feeds it.
        for chunk in audio.chunks((rate / 60.0) as usize) {
            d.feed(chunk, rate, &s);
        }
        let beats = 8.0 * 128.0 / 60.0;
        assert!(
            (d.count as f64 - beats).abs() <= 2.0,
            "{} beats, expected ~{beats}",
            d.count
        );
        assert!((d.bpm() - 128.0).abs() < 3.0, "bpm {}", d.bpm());
    }

    #[test]
    fn silence_is_not_a_beat() {
        let s = Settings {
            sensitivity: 1.6,
            cutoff: 150.0,
            mingap: 0.3,
        };
        let mut d = Detector::default();
        d.feed(&vec![0.0; 44100 * 2], 44100.0, &s);
        assert_eq!(d.count, 0);
        assert_eq!(d.bpm(), 0.0);
    }
}
