//! Which nodes cook whether or not anything wants them.
//!
//! The cook is demand-driven, which is the whole design: a node cooks because
//! something downstream asked. An Execute DAT has nothing downstream by
//! definition — its output is a side effect — so under that rule it would
//! never run at all.
//!
//! The existing answer to "cook this anyway" is the render flag, and requiring
//! it on every Execute DAT would be a trap: a callback that silently does
//! nothing until you find the right checkbox is worse than no callback. So the
//! operator declares it instead, and both hosts add these to their roots.

use otd_core::{Graph, NodeId};

/// Every node that must cook each frame regardless of demand.
pub fn roots(graph: &Graph) -> Vec<NodeId> {
    graph
        .walk()
        .into_iter()
        .filter(|id| *id != graph.root())
        .filter(|id| {
            graph
                .get(*id)
                .map(|n| is_execute(&n.op_type))
                .unwrap_or(false)
        })
        .collect()
}

/// Whether an operator type is one of the callback DATs.
pub fn is_execute(op_type: &str) -> bool {
    matches!(
        op_type,
        otd_dat::ops::EXECUTE | "chopexecuteDAT" | "parameterexecuteDAT"
    )
}
