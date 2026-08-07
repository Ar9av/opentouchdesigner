//! Keyframe curves.
//!
//! PLAN.md Phase 6 asks for timeline/keyframe animation and notes that TD is
//! weak here. The design decision that matters is *where keyframes live*, and
//! the answer here is: in an ordinary CHOP, whose keys are an ordinary text
//! parameter.
//!
//! That falls out of what already exists. A keyframed value is a value over
//! time, which is what a channel is; making it a CHOP means it can be exported
//! to any parameter, filtered, merged, lagged and maths'd with everything
//! else, with no new mechanism anywhere. And keeping the keys as text means
//! they land in the project file as text — the git-diffable project format is
//! a headline feature, and a binary keyframe blob would quietly undo it.
//!
//! The format is one keyframe per line:
//!
//! ```text
//! tx  0    0.0  linear
//! tx  2    1.0  smooth
//! ty  0    1.0  constant
//! ```
//!
//! `channel time value interpolation`. Whitespace-separated, sorted by time
//! when written, and readable enough to edit by hand in a text editor — which
//! is a real workflow when you want twenty keys at regular intervals.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// How a key blends into the next one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Interp {
    /// Hold this value until the next key, then jump. Steps and cue lists.
    Constant,
    #[default]
    Linear,
    /// Ease in and out — the smoothstep everyone reaches for.
    Smooth,
    /// Catmull-Rom through the neighbouring keys: continuous velocity across
    /// the whole curve rather than a corner at every key.
    Spline,
}

impl Interp {
    pub fn name(self) -> &'static str {
        match self {
            Interp::Constant => "constant",
            Interp::Linear => "linear",
            Interp::Smooth => "smooth",
            Interp::Spline => "spline",
        }
    }

    pub fn parse(s: &str) -> Option<Interp> {
        match s.trim().to_ascii_lowercase().as_str() {
            "constant" | "step" | "hold" => Some(Interp::Constant),
            "linear" => Some(Interp::Linear),
            "smooth" | "ease" => Some(Interp::Smooth),
            "spline" | "cubic" => Some(Interp::Spline),
            _ => None,
        }
    }

    pub const ALL: [Interp; 4] = [
        Interp::Constant,
        Interp::Linear,
        Interp::Smooth,
        Interp::Spline,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub time: f64,
    pub value: f32,
    /// How the segment *starting* at this key behaves.
    pub interp: Interp,
}

/// One named channel's keys, kept sorted by time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve {
    pub keys: Vec<Key>,
}

impl Curve {
    /// The value at `t`.
    ///
    /// Outside the keyed range the curve holds its end values rather than
    /// extrapolating. Extrapolation is almost never what was meant, and a
    /// value that shoots off to infinity three seconds before the cue is a
    /// much worse failure than one that sits still.
    pub fn sample(&self, t: f64) -> f32 {
        match self.keys.len() {
            0 => 0.0,
            1 => self.keys[0].value,
            _ => {
                if t <= self.keys[0].time {
                    return self.keys[0].value;
                }
                let last = self.keys.len() - 1;
                if t >= self.keys[last].time {
                    return self.keys[last].value;
                }
                // The segment containing `t`: the last key at or before it.
                let i = self
                    .keys
                    .partition_point(|k| k.time <= t)
                    .saturating_sub(1)
                    .min(last - 1);
                let (a, b) = (self.keys[i], self.keys[i + 1]);
                let span = b.time - a.time;
                // Two keys at the same time is a hard cut, not a division by
                // zero.
                if span <= 0.0 {
                    return b.value;
                }
                let u = ((t - a.time) / span) as f32;
                match a.interp {
                    Interp::Constant => a.value,
                    Interp::Linear => a.value + (b.value - a.value) * u,
                    Interp::Smooth => {
                        let s = u * u * (3.0 - 2.0 * u);
                        a.value + (b.value - a.value) * s
                    }
                    Interp::Spline => {
                        // Catmull-Rom, with the ends duplicated so the first
                        // and last segments behave rather than needing a
                        // special case at every call site.
                        let p0 = self.keys[i.saturating_sub(1)].value;
                        let p3 = self.keys[(i + 2).min(last)].value;
                        catmull_rom(p0, a.value, b.value, p3, u)
                    }
                }
            }
        }
    }

    /// Insert a key, replacing any existing one at the same time.
    pub fn set(&mut self, time: f64, value: f32, interp: Interp) {
        match self.keys.iter().position(|k| (k.time - time).abs() < 1e-9) {
            Some(i) => {
                self.keys[i].value = value;
                self.keys[i].interp = interp;
            }
            None => {
                self.keys.push(Key {
                    time,
                    value,
                    interp,
                });
                self.sort();
            }
        }
    }

    pub fn remove_at(&mut self, time: f64) -> bool {
        let before = self.keys.len();
        self.keys.retain(|k| (k.time - time).abs() >= 1e-9);
        self.keys.len() != before
    }

    fn sort(&mut self) {
        self.keys.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// The time of the first and last key.
    pub fn range(&self) -> Option<(f64, f64)> {
        Some((self.keys.first()?.time, self.keys.last()?.time))
    }
}

fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, u: f32) -> f32 {
    let (u2, u3) = (u * u, u * u * u);
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * u
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u3)
}

/// A set of named curves — what one Animation CHOP holds.
///
/// `BTreeMap` so the text form is deterministic: the same keys always write
/// the same file, which is what makes the format diffable rather than merely
/// textual.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curves(pub BTreeMap<String, Curve>);

impl Curves {
    /// Parse the `keys` parameter.
    ///
    /// Unreadable lines are skipped rather than failing the whole parse: this
    /// text is hand-editable, and losing every keyframe in the project because
    /// of one typo halfway down would be an unreasonable punishment. What is
    /// skipped is returned so the editor can say so.
    pub fn parse(text: &str) -> (Curves, Vec<String>) {
        let mut out: BTreeMap<String, Curve> = BTreeMap::new();
        let mut bad = Vec::new();

        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (Some(name), Some(time), Some(value)) = (parts.next(), parts.next(), parts.next())
            else {
                bad.push(format!(
                    "line {}: expected `channel time value [interp]`",
                    n + 1
                ));
                continue;
            };
            let (Ok(time), Ok(value)) = (time.parse::<f64>(), value.parse::<f32>()) else {
                bad.push(format!(
                    "line {}: `{time}` and `{value}` must be numbers",
                    n + 1
                ));
                continue;
            };
            let interp = match parts.next() {
                None => Interp::Linear,
                Some(word) => match Interp::parse(word) {
                    Some(i) => i,
                    None => {
                        bad.push(format!("line {}: unknown interpolation `{word}`", n + 1));
                        Interp::Linear
                    }
                },
            };
            out.entry(name.to_string())
                .or_default()
                .set(time, value, interp);
        }
        (Curves(out), bad)
    }

    /// Write the `keys` parameter back out, sorted and aligned.
    pub fn to_text(&self) -> String {
        let width = self.0.keys().map(|n| n.len()).max().unwrap_or(4).max(4);
        let mut out = String::new();
        for (name, curve) in &self.0 {
            for key in &curve.keys {
                let _ = writeln!(
                    out,
                    "{name:<width$}  {:<8} {:<10} {}",
                    trim_float(key.time),
                    trim_float(key.value as f64),
                    key.interp.name(),
                );
            }
        }
        out
    }

    /// Every channel's value at `t`, in name order.
    pub fn sample(&self, t: f64) -> Vec<(String, f32)> {
        self.0
            .iter()
            .map(|(name, curve)| (name.clone(), curve.sample(t)))
            .collect()
    }

    /// The span covered by every key in every channel.
    pub fn range(&self) -> Option<(f64, f64)> {
        let mut span: Option<(f64, f64)> = None;
        for curve in self.0.values() {
            if let Some((lo, hi)) = curve.range() {
                span = Some(match span {
                    Some((a, b)) => (a.min(lo), b.max(hi)),
                    None => (lo, hi),
                });
            }
        }
        span
    }
}

/// `2` rather than `2.0000000000`, `0.25` rather than `0.25000001`.
///
/// The project file is read by people. A keyframe written as `1.0000000149`
/// because a float round-tripped is noise in every diff that touches it.
fn trim_float(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(keys: &[(f64, f32, Interp)]) -> Curve {
        Curve {
            keys: keys
                .iter()
                .map(|(time, value, interp)| Key {
                    time: *time,
                    value: *value,
                    interp: *interp,
                })
                .collect(),
        }
    }

    #[test]
    fn a_linear_segment_is_a_straight_line() {
        let c = curve(&[(0.0, 0.0, Interp::Linear), (2.0, 10.0, Interp::Linear)]);
        assert_eq!(c.sample(0.0), 0.0);
        assert_eq!(c.sample(1.0), 5.0);
        assert_eq!(c.sample(2.0), 10.0);
    }

    #[test]
    fn outside_the_keys_the_curve_holds_rather_than_extrapolating() {
        let c = curve(&[(1.0, 3.0, Interp::Linear), (2.0, 4.0, Interp::Linear)]);
        // A value shooting off to infinity three seconds before the cue is a
        // far worse failure than one that sits still.
        assert_eq!(c.sample(-100.0), 3.0);
        assert_eq!(c.sample(100.0), 4.0);
    }

    #[test]
    fn constant_holds_until_the_next_key_then_jumps() {
        let c = curve(&[(0.0, 0.0, Interp::Constant), (1.0, 1.0, Interp::Linear)]);
        assert_eq!(c.sample(0.0), 0.0);
        assert_eq!(c.sample(0.99), 0.0, "no creeping before the jump");
        assert_eq!(c.sample(1.0), 1.0);
    }

    #[test]
    fn smooth_is_flat_at_both_ends_and_halfway_in_the_middle() {
        let c = curve(&[(0.0, 0.0, Interp::Smooth), (1.0, 1.0, Interp::Linear)]);
        assert_eq!(c.sample(0.5), 0.5);
        // The point of an ease: it leaves and arrives slowly.
        assert!(c.sample(0.1) < 0.1, "{}", c.sample(0.1));
        assert!(c.sample(0.9) > 0.9, "{}", c.sample(0.9));
    }

    #[test]
    fn a_spline_passes_through_its_keys() {
        let c = curve(&[
            (0.0, 0.0, Interp::Spline),
            (1.0, 5.0, Interp::Spline),
            (2.0, 2.0, Interp::Spline),
            (3.0, 8.0, Interp::Spline),
        ]);
        // Interpolation, not approximation: whatever it does between the keys,
        // it has to hit the values that were actually authored.
        for (t, v) in [(0.0, 0.0), (1.0, 5.0), (2.0, 2.0), (3.0, 8.0)] {
            assert!((c.sample(t) - v).abs() < 1e-4, "at {t}: {}", c.sample(t));
        }
        // And it does not simply join the keys with straight lines: the
        // midpoint of the 5 -> 2 segment sits off the chord, pulled by the
        // neighbouring keys. That continuity is the reason to choose a spline.
        let chord_midpoint = 3.5;
        assert!(
            (c.sample(1.5) - chord_midpoint).abs() > 0.05,
            "a spline that lies on the chord is just linear: {}",
            c.sample(1.5)
        );
    }

    #[test]
    fn an_empty_or_single_key_curve_does_not_divide_by_zero() {
        assert_eq!(Curve::default().sample(1.0), 0.0);
        let one = curve(&[(5.0, 2.5, Interp::Linear)]);
        assert_eq!(one.sample(0.0), 2.5);
        assert_eq!(one.sample(99.0), 2.5);
    }

    #[test]
    fn two_keys_at_the_same_time_are_a_hard_cut() {
        let c = curve(&[
            (0.0, 0.0, Interp::Linear),
            (1.0, 1.0, Interp::Linear),
            (1.0, 5.0, Interp::Linear),
            (2.0, 5.0, Interp::Linear),
        ]);
        assert_eq!(c.sample(0.5), 0.5);
        assert!(c.sample(1.5).is_finite(), "no NaN from a zero-length span");
        assert_eq!(c.sample(1.5), 5.0);
    }

    #[test]
    fn the_text_form_round_trips() {
        let text = "\
tx  0   0.0   linear
tx  2   1.0   smooth
ty  0   1.0   constant
";
        let (curves, bad) = Curves::parse(text);
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(curves.0.len(), 2);
        assert_eq!(curves.0["tx"].keys.len(), 2);
        assert_eq!(curves.0["tx"].keys[1].interp, Interp::Smooth);
        assert_eq!(curves.0["ty"].keys[0].interp, Interp::Constant);

        // Writing and re-reading has to give back the same curves, or a save
        // and load would quietly change the animation.
        let (again, bad) = Curves::parse(&curves.to_text());
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(again, curves);
    }

    #[test]
    fn the_text_form_stays_readable_rather_than_full_of_float_noise() {
        let mut curves = Curves::default();
        curves
            .0
            .entry("tx".into())
            .or_default()
            .set(2.0, 1.0, Interp::Linear);
        let text = curves.to_text();
        assert!(text.contains("2 "), "{text}");
        assert!(
            !text.contains("2.000000"),
            "a diff full of trailing zeroes is noise: {text}"
        );
    }

    #[test]
    fn a_bad_line_is_skipped_and_reported_not_fatal() {
        let (curves, bad) = Curves::parse(
            "\
tx 0 0 linear
this line is nonsense
tx 1 wobble linear
tx 2 1 sideways
tx 3 1
# a comment
tx 4 2 linear   # and a trailing one
",
        );
        // Losing every keyframe in the project to one typo halfway down would
        // be an unreasonable punishment.
        assert_eq!(curves.0["tx"].keys.len(), 4, "the good lines survived");
        assert_eq!(bad.len(), 3, "{bad:?}");
        assert!(bad[2].contains("sideways"), "{bad:?}");
        // An unknown interpolation still keeps the key, at linear.
        assert_eq!(curves.0["tx"].keys[2].interp, Interp::Linear);
    }

    #[test]
    fn keys_are_sorted_however_they_were_written() {
        let (curves, _) = Curves::parse("tx 5 5 linear\ntx 1 1 linear\ntx 3 3 linear\n");
        let times: Vec<f64> = curves.0["tx"].keys.iter().map(|k| k.time).collect();
        assert_eq!(times, vec![1.0, 3.0, 5.0]);
        // And sampling relies on that ordering.
        assert_eq!(curves.0["tx"].sample(2.0), 2.0);
    }

    #[test]
    fn setting_a_key_at_an_existing_time_replaces_it() {
        let mut c = Curve::default();
        c.set(1.0, 1.0, Interp::Linear);
        c.set(1.0, 9.0, Interp::Smooth);
        assert_eq!(c.keys.len(), 1);
        assert_eq!(c.keys[0].value, 9.0);
        assert_eq!(c.keys[0].interp, Interp::Smooth);
        assert!(c.remove_at(1.0));
        assert!(!c.remove_at(1.0));
    }
}
