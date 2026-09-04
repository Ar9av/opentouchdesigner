//! What one family's output looks like to another family's operator.
//!
//! PLAN.md §2.1 keeps the wire families separate on purpose — "explicit
//! converter ops + parameter references bridge families" — but until now only
//! the parameter half existed. This is the other half.
//!
//! The design constraint is the crate graph: `otd-chop` must not depend on
//! `otd-dat`, and `otd-gpu` must not depend on either, or the clean core
//! PLAN.md §3 asks for stops being clean. So a family's output crosses the
//! boundary in *this* neutral, dependency-free form, and the converter
//! operator — which lives in the crate of the family it produces — decides
//! what the bytes mean. `otd-engine` is the only place that fills these in,
//! for the same reason it is the only place a TOP parameter can read a CHOP.
//!
//! Deliberately raw: no naming, no scaling, no interpretation. A SOP arrives
//! as an interleaved buffer and a schema, not as channels called `P(0)`,
//! because choosing that name is the SOP to CHOP operator's job and putting it
//! here would scatter one operator's behaviour across two crates.

/// A CHOP crossing, unpacked: names, samples, sample rate.
pub type ChannelView<'a> = (&'a [String], &'a [Vec<f32>], f64);

/// One input of a foreign family, as the receiving operator sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum Crossing {
    /// A DAT: rows of cells.
    Table(Vec<Vec<String>>),
    /// A CHOP: named channels, all the same length.
    Channels {
        names: Vec<String>,
        samples: Vec<Vec<f32>>,
        sample_rate: f64,
    },
    /// A SOP: points interleaved in `data`, described by `attrs` as
    /// `(name, width)` pairs in buffer order. A point's stride is the sum of
    /// the widths — the same flat layout the renderer's vertex buffer uses.
    Points {
        attrs: Vec<(String, usize)>,
        data: Vec<f32>,
    },
    /// A TOP: linear RGBA, row-major from the top-left.
    Pixels {
        width: u32,
        height: u32,
        rgba: Vec<f32>,
    },
}

impl Crossing {
    pub fn as_table(&self) -> Option<&[Vec<String>]> {
        match self {
            Crossing::Table(rows) => Some(rows),
            _ => None,
        }
    }

    pub fn as_channels(&self) -> Option<ChannelView<'_>> {
        match self {
            Crossing::Channels {
                names,
                samples,
                sample_rate,
            } => Some((names, samples, *sample_rate)),
            _ => None,
        }
    }

    /// Bytes per point in [`Crossing::Points`].
    pub fn point_stride(&self) -> usize {
        match self {
            Crossing::Points { attrs, .. } => attrs.iter().map(|(_, w)| *w).sum(),
            _ => 0,
        }
    }

    /// Where an attribute starts within a point, and how wide it is.
    pub fn attr(&self, name: &str) -> Option<(usize, usize)> {
        let Crossing::Points { attrs, .. } = self else {
            return None;
        };
        let mut offset = 0;
        for (n, w) in attrs {
            if n == name {
                return Some((offset, *w));
            }
            offset += w;
        }
        None
    }
}

/// The foreign inputs of one node, indexed the way its inputs are.
///
/// Sparse on purpose: an operator that mixes families — a converter that also
/// takes an input of its own kind — gets `None` for the slots its own engine
/// already handled.
pub type Crossings = Vec<Option<Crossing>>;
