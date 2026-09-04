//! What a CHOP produces: named channels of samples.
//!
//! PLAN.md §2.1 — CHOPs are "channels of samples", the control and audio
//! family. The load-bearing detail is §4's **time slicing**: a CHOP that is
//! generating over time emits only the samples covering the interval since the
//! last frame, sized from the *real* elapsed time. A frame that takes 33 ms
//! produces twice the samples of one that takes 16 ms, so an LFO stays on
//! pitch and audio stays continuous when the renderer stutters.

use otd_core::CookContext;

/// Control-rate CHOPs default to this; audio CHOPs carry the device rate.
pub const CONTROL_RATE: f64 = 60.0;

/// A safety bound on one frame's slice, so a stalled frame cannot allocate
/// unboundedly.
pub const MAX_SLICE: usize = 1 << 16;

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub name: String,
    pub samples: Vec<f32>,
}

impl Channel {
    pub fn new(name: impl Into<String>, samples: Vec<f32>) -> Self {
        Channel {
            name: name.into(),
            samples,
        }
    }

    /// A channel holding one repeated value.
    pub fn constant(name: impl Into<String>, value: f32, len: usize) -> Self {
        Channel::new(name, vec![value; len.max(1)])
    }

    /// The most recent sample — what a parameter export reads.
    pub fn last(&self) -> f32 {
        self.samples.last().copied().unwrap_or(0.0)
    }

    pub fn min(&self) -> f32 {
        self.samples.iter().copied().fold(f32::INFINITY, f32::min)
    }

    pub fn max(&self) -> f32 {
        self.samples
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChopData {
    pub channels: Vec<Channel>,
    pub sample_rate: f64,
    /// True when this is one frame's worth of a continuous stream, rather
    /// than a standalone buffer like a Pattern CHOP's waveform.
    pub time_sliced: bool,
}

impl ChopData {
    pub fn new(channels: Vec<Channel>, sample_rate: f64, time_sliced: bool) -> Self {
        ChopData {
            channels,
            sample_rate,
            time_sliced,
        }
    }

    pub fn empty() -> Self {
        ChopData {
            channels: Vec::new(),
            sample_rate: CONTROL_RATE,
            time_sliced: true,
        }
    }

    /// Samples per channel. Channels are always the same length within one
    /// CHOP; operators that combine inputs pad to the longest.
    pub fn num_samples(&self) -> usize {
        self.channels.first().map(|c| c.samples.len()).unwrap_or(0)
    }

    pub fn num_channels(&self) -> usize {
        self.channels.len()
    }

    pub fn channel(&self, name: &str) -> Option<&Channel> {
        self.channels.iter().find(|c| c.name == name)
    }

    /// The current value of a channel by name — the Export-mode read.
    pub fn value(&self, name: &str) -> Option<f32> {
        self.channel(name).map(|c| c.last())
    }

    /// Channel by index, tolerating an out-of-range index.
    pub fn nth(&self, index: usize) -> Option<&Channel> {
        self.channels.get(index)
    }

    pub fn names(&self) -> Vec<String> {
        self.channels.iter().map(|c| c.name.clone()).collect()
    }

    /// Make every channel the same length by repeating the last sample.
    pub fn pad_to(&mut self, len: usize) {
        for c in &mut self.channels {
            let last = c.last();
            if c.samples.len() < len {
                c.samples.resize(len, last);
            }
        }
    }
}

/// How many samples cover the time that has actually elapsed.
///
/// Never zero: a CHOP that produced no samples would leave downstream
/// operators — and any parameter exporting from it — with nothing to read.
pub fn slice_len(sample_rate: f64, ctx: &CookContext) -> usize {
    let n = (sample_rate * ctx.dt.max(0.0)).round() as i64;
    n.clamp(1, MAX_SLICE as i64) as usize
}

/// The time of each sample in a slice, ending at the current frame time.
///
/// Sample `n-1` lands exactly on `ctx.time`, so the value a parameter reads
/// is the value *now* rather than one slice in the past.
pub fn slice_times(ctx: &CookContext, n: usize) -> impl Iterator<Item = f64> + use<> {
    let step = if n > 0 { ctx.dt / n as f64 } else { 0.0 };
    let start = ctx.time - ctx.dt;
    (0..n).map(move |i| start + step * (i + 1) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slice_tracks_real_elapsed_time_not_the_nominal_rate() {
        let mut ctx = CookContext {
            dt: 1.0 / 60.0,
            ..Default::default()
        };
        assert_eq!(slice_len(60.0, &ctx), 1);
        assert_eq!(slice_len(44100.0, &ctx), 735);

        // A frame that took twice as long produces twice the samples, which
        // is the whole point: the signal stays continuous.
        ctx.dt = 2.0 / 60.0;
        assert_eq!(slice_len(60.0, &ctx), 2);
        assert_eq!(slice_len(44100.0, &ctx), 1470);
    }

    #[test]
    fn a_slice_is_never_empty() {
        let mut ctx = CookContext {
            dt: 0.0,
            ..Default::default()
        };
        assert_eq!(slice_len(60.0, &ctx), 1);
        ctx.dt = 1e9;
        assert_eq!(slice_len(44100.0, &ctx), MAX_SLICE);
    }

    #[test]
    fn the_last_sample_of_a_slice_is_now() {
        let ctx = CookContext {
            time: 10.0,
            dt: 0.1,
            ..Default::default()
        };
        let times: Vec<f64> = slice_times(&ctx, 4).collect();
        assert_eq!(times.len(), 4);
        assert!((times[3] - 10.0).abs() < 1e-12);
        assert!((times[0] - 9.925).abs() < 1e-12);
    }
}
