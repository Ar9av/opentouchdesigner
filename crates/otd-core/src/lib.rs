//! `otd-core` — the graph model, the cook engine, and the project format.
//!
//! This crate deliberately has no GPU, windowing, UI or scripting dependency.
//! Everything here is plain data and scheduling logic, which is what makes the
//! cook engine unit-testable and keeps a future headless/WASM target open
//! (PLAN.md §3).

pub mod cook;
pub mod expr;
pub mod graph;
pub mod param;
pub mod project;
pub mod value;

#[cfg(test)]
mod test_ops;

pub use cook::{CookContext, CookEngine, CookError, Cooker, FrameStats};
pub use expr::{ChannelSource, EvalContext, Expr};
pub use graph::{Family, Graph, GraphError, Node, NodeFlags, NodeId, OpDef, OpRegistry};
pub use param::{Param, ParamMode};
pub use project::Project;
pub use value::Value;

pub use indexmap;
