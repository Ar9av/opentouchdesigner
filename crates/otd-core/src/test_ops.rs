//! Minimal fake operators, so the cook engine can be tested without a GPU.

use indexmap::IndexMap;

use crate::cook::{CookContext, CookError, Cooker};
use crate::graph::{Connector, Family, Graph, NodeId, OpDef, OpRegistry};
use crate::param::Param;

pub const TEST_PASS: &str = "gain";

fn one_float() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(TEST_PASS.to_string(), Param::float(1.0));
    m
}

fn no_params() -> IndexMap<String, Param> {
    IndexMap::new()
}

pub fn registry() -> OpRegistry {
    let mut r = OpRegistry::new();
    r.register(OpDef {
        type_name: "pass",
        label: "Pass",
        family: Family::Top,
        inputs: &["in"],
        summary: "test operator",
        time_dependent: false,
        params: one_float,
        connector: Connector::None,
    });
    r.register(OpDef {
        type_name: "comp2",
        label: "Comp2",
        family: Family::Top,
        inputs: &["a", "b"],
        summary: "two-input test operator",
        time_dependent: false,
        params: one_float,
        connector: Connector::None,
    });
    r.register(OpDef {
        type_name: "movie",
        label: "Movie",
        family: Family::Top,
        inputs: &[],
        summary: "intrinsically animated source",
        time_dependent: true,
        params: no_params,
        connector: Connector::None,
    });
    r.register(OpDef {
        type_name: "chop_pass",
        label: "Chop Pass",
        family: Family::Chop,
        inputs: &["in"],
        summary: "test CHOP",
        time_dependent: false,
        params: one_float,
        connector: Connector::None,
    });
    r.register(OpDef {
        type_name: "container",
        label: "Container",
        family: Family::Comp,
        inputs: &[],
        summary: "test COMP",
        time_dependent: false,
        params: no_params,
        connector: Connector::None,
    });
    r
}

/// Records the order and count of cooks.
#[derive(Default)]
pub struct CountingCooker {
    pub log: Vec<String>,
    /// `(node, target)` pairs reported as non-wire dependencies, standing in
    /// for a Select TOP's Target parameter.
    pub references: Vec<(NodeId, NodeId)>,
}

impl Cooker for CountingCooker {
    fn cook(&mut self, graph: &Graph, id: NodeId, _ctx: &CookContext) -> Result<(), CookError> {
        self.log.push(graph.path(id));
        Ok(())
    }

    fn extra_inputs(&self, _graph: &Graph, id: NodeId) -> Vec<NodeId> {
        self.references
            .iter()
            .filter(|(from, _)| *from == id)
            .map(|(_, to)| *to)
            .collect()
    }
}
