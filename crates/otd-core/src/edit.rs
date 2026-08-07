//! What a script is allowed to change, and when.
//!
//! A parameter expression may read the network and never mutate it — that is
//! what keeps evaluation reentrant with the cook, and it is stated plainly in
//! `otd-py`'s module docs. Callbacks are the case that has to be different: an
//! Execute DAT whose whole job is to turn a knob when a beat lands cannot be
//! read-only and still be worth having.
//!
//! The way out is not to relax the rule but to move the write. A callback
//! *requests* an edit, the requests queue up, and the host applies them
//! between frames where it already holds the graph mutably — the same phase
//! that syncs clones and replicators. So the cook still sees an unchanging
//! graph, a script still cannot corrupt one mid-frame, and PLAN.md §4's
//! "scripts run at a fixed phase of the frame" stays true of the writes as
//! well as the reads.
//!
//! The cost is honest and worth stating: an edit made in a callback lands on
//! the *next* frame, not this one.

use crate::value::Value;

/// A parameter change a script asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamEdit {
    /// Absolute path of the operator to change.
    pub path: String,
    pub param: String,
    pub value: Value,
}

impl ParamEdit {
    pub fn new(path: impl Into<String>, param: impl Into<String>, value: Value) -> ParamEdit {
        ParamEdit {
            path: path.into(),
            param: param.into(),
            value,
        }
    }
}

/// Apply queued edits to a graph, returning how many landed.
///
/// A path that no longer resolves is skipped rather than failing the batch: a
/// callback that outlives the node it was pointing at is a mistake to report,
/// not a reason to drop the other nine edits in the same frame.
pub fn apply(graph: &mut crate::graph::Graph, edits: &[ParamEdit]) -> (usize, Vec<String>) {
    let mut applied = 0;
    let mut problems = Vec::new();
    for edit in edits {
        let Some(id) = graph.find(&edit.path) else {
            problems.push(format!("no operator at `{}`", edit.path));
            continue;
        };
        match graph.set_param(id, &edit.param, edit.value.clone()) {
            Ok(()) => applied += 1,
            Err(e) => problems.push(format!("{}.{}: {e}", edit.path, edit.param)),
        }
    }
    (applied, problems)
}
