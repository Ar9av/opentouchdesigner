//! `otd-chop` — the CHOP (channel operator) family.
//!
//! Channels of samples: the control and audio side of the network. No GPU, no
//! UI; device work happens on its own threads and hands buffers to the cook
//! (PLAN.md §4).

pub mod data;
pub mod engine;
pub mod io;
pub mod ops;

pub use data::{CONTROL_RATE, Channel, ChopData};
pub use engine::{ChannelStore, ChopEngine, ChopHost, Network};
pub use io::{InputState, Io};
