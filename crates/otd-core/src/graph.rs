//! The graph model: an arena of nodes, typed by family, arranged in a
//! component hierarchy. No GPU, no UI, no Python — everything in here is
//! plain data so the cook engine can be unit tested (PLAN.md §3, "a clean
//! core").

use indexmap::IndexMap;
use slotmap::{Key, SecondaryMap, SlotMap};
use std::collections::HashSet;

use crate::param::Param;
use crate::value::Value;

slotmap::new_key_type! {
    /// Stable handle to a node. Generational, so a stale id from a deleted
    /// node resolves to `None` rather than to whatever took its slot.
    pub struct NodeId;
}

/// The typed wire families. Same-family wiring only; crossing families is an
/// explicit converter operator or a parameter reference (PLAN.md §2.1).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Family {
    /// GPU textures.
    Top,
    /// Channels of samples.
    Chop,
    /// Geometry.
    Sop,
    /// Text and tables.
    Dat,
    /// Materials.
    Mat,
    /// Containers.
    Comp,
}

impl Family {
    pub fn suffix(self) -> &'static str {
        match self {
            Family::Top => "TOP",
            Family::Chop => "CHOP",
            Family::Sop => "SOP",
            Family::Dat => "DAT",
            Family::Mat => "MAT",
            Family::Comp => "COMP",
        }
    }

    /// Wire tint, matching the family colours artists already have muscle
    /// memory for. sRGB, 0-255.
    pub fn color(self) -> [u8; 3] {
        match self {
            Family::Top => [102, 140, 196],
            Family::Chop => [126, 190, 126],
            Family::Sop => [175, 130, 190],
            Family::Dat => [190, 170, 110],
            Family::Mat => [190, 120, 120],
            Family::Comp => [140, 140, 150],
        }
    }

    pub fn all() -> &'static [Family] {
        &[
            Family::Top,
            Family::Chop,
            Family::Sop,
            Family::Dat,
            Family::Mat,
            Family::Comp,
        ]
    }
}

/// Per-node toggles that live on the node body in the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NodeFlags {
    /// Show the live viewer on the node body.
    pub display: bool,
    /// Pass input 0 straight through without cooking this operator.
    pub bypass: bool,
    /// This node is a cook root even if nothing downstream wants it.
    pub render: bool,
}

impl Default for NodeFlags {
    fn default() -> Self {
        NodeFlags {
            display: true,
            bypass: false,
            render: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub op_type: String,
    pub family: Family,
    pub params: IndexMap<String, Param>,
    /// One slot per declared input; `None` means unconnected.
    pub inputs: Vec<Option<NodeId>>,
    pub input_labels: Vec<String>,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Editor position. Part of the project file — node layout is authored
    /// information, not incidental UI state.
    pub pos: [f32; 2],
    pub flags: NodeFlags,
    /// Operators that must re-cook every frame regardless of parameter
    /// changes (a movie player, a device input).
    pub intrinsically_time_dependent: bool,
    /// Bumped on every mutation. The cook engine compares against the value
    /// it last cooked at — a revision counter rather than a dirty bit, so a
    /// node edited twice between cooks still only cooks once (PLAN.md §3).
    revision: u64,
}

impl Node {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn param(&self, key: &str) -> Option<&Param> {
        self.params.get(key)
    }

    /// True when any parameter is animated by an expression.
    pub fn has_time_dependent_param(&self) -> bool {
        self.params.values().any(|p| p.is_time_dependent())
    }

    /// Operator paths this node's parameters read from — Export and Bind
    /// sources. The cook engine turns these into dependencies so the source
    /// cooks first and its animation propagates here.
    pub fn param_sources(&self) -> impl Iterator<Item = &str> {
        self.params.values().filter_map(|p| p.source_op())
    }
}

/// A static description of an operator type. Registered once at startup;
/// `Graph::create` stamps out nodes from it.
#[derive(Clone)]
pub struct OpDef {
    pub type_name: &'static str,
    pub label: &'static str,
    pub family: Family,
    pub inputs: &'static [&'static str],
    pub summary: &'static str,
    pub time_dependent: bool,
    pub params: fn() -> IndexMap<String, Param>,
}

#[derive(Default, Clone)]
pub struct OpRegistry {
    defs: IndexMap<String, OpDef>,
}

impl OpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, def: OpDef) {
        self.defs.insert(def.type_name.to_string(), def);
    }

    pub fn get(&self, type_name: &str) -> Option<&OpDef> {
        self.defs.get(type_name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &OpDef> {
        self.defs.values()
    }

    /// Operators of one family, for the Tab create-dialog.
    pub fn of_family(&self, family: Family) -> impl Iterator<Item = &OpDef> {
        self.defs.values().filter(move |d| d.family == family)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GraphError {
    #[error("no such node")]
    NoSuchNode,
    #[error("input index {0} out of range")]
    BadInputIndex(usize),
    #[error("cannot wire a {0} output into a {1} input")]
    FamilyMismatch(&'static str, &'static str),
    #[error("that connection would create a cycle; use a Feedback operator instead")]
    WouldCycle,
    #[error("unknown operator type `{0}`")]
    UnknownOpType(String),
    #[error("no node at path `{0}`")]
    BadPath(String),
}

#[derive(Debug)]
pub struct Graph {
    nodes: SlotMap<NodeId, Node>,
    root: NodeId,
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

impl Graph {
    pub fn new() -> Self {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert(Node {
            name: "root".into(),
            op_type: "container".into(),
            family: Family::Comp,
            params: IndexMap::new(),
            inputs: Vec::new(),
            input_labels: Vec::new(),
            parent: None,
            children: Vec::new(),
            pos: [0.0, 0.0],
            flags: NodeFlags::default(),
            intrinsically_time_dependent: false,
            revision: 0,
        });
        Graph { nodes, root }
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(id)
    }

    /// Mutable access. Bumps the node's revision, so the next cook sees it as
    /// dirty — conservative by design: an extra cook is cheap, a missed one
    /// shows up as a frozen viewer.
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        let n = &mut self.nodes[id];
        n.revision += 1;
        n
    }

    /// Read-write access that does *not* dirty the node. For things the cook
    /// does not depend on, like dragging a node around the canvas.
    pub fn node_mut_quiet(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id]
    }

    pub fn ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys()
    }

    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter()
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id].children
    }

    // ------------------------------------------------------------- creation

    pub fn create(
        &mut self,
        parent: NodeId,
        def: &OpDef,
        name: Option<&str>,
    ) -> Result<NodeId, GraphError> {
        if !self.nodes.contains_key(parent) {
            return Err(GraphError::NoSuchNode);
        }
        // An explicitly requested name is honoured exactly when it is free —
        // loading a project must not rename `myBlur` to `myBlur1`. Only a
        // collision, or a create with no name at all, gets auto-numbered.
        let name = match name {
            Some(n) if !self.name_taken(parent, n) => n.to_string(),
            Some(n) => self.unique_name(parent, n),
            // `noiseTOP` creates `noise1`, not `noiseTOP1` — the family is
            // already obvious from the node's colour and its wires.
            None => {
                let base = def
                    .type_name
                    .strip_suffix(def.family.suffix())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(def.type_name);
                self.unique_name(parent, base)
            }
        };
        let node = Node {
            name,
            op_type: def.type_name.to_string(),
            family: def.family,
            params: (def.params)(),
            inputs: vec![None; def.inputs.len()],
            input_labels: def.inputs.iter().map(|s| s.to_string()).collect(),
            parent: Some(parent),
            children: Vec::new(),
            pos: [0.0, 0.0],
            flags: NodeFlags::default(),
            intrinsically_time_dependent: def.time_dependent,
            revision: 1,
        };
        let id = self.nodes.insert(node);
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    pub fn name_taken(&self, parent: NodeId, name: &str) -> bool {
        self.nodes[parent]
            .children
            .iter()
            .any(|c| self.nodes[*c].name == name)
    }

    /// Auto-numbered names, TD style: `noise1`, `noise2`, ...
    pub fn unique_name(&self, parent: NodeId, base: &str) -> String {
        let base = base.trim_end_matches(|c: char| c.is_ascii_digit());
        let taken: HashSet<&str> = self.nodes[parent]
            .children
            .iter()
            .map(|c| self.nodes[*c].name.as_str())
            .collect();
        for i in 1.. {
            let candidate = format!("{base}{i}");
            if !taken.contains(candidate.as_str()) {
                return candidate;
            }
        }
        unreachable!()
    }

    /// Delete a node and everything under it, unwiring any downstream inputs.
    pub fn remove(&mut self, id: NodeId) -> Result<(), GraphError> {
        if id == self.root {
            return Err(GraphError::NoSuchNode);
        }
        if !self.nodes.contains_key(id) {
            return Err(GraphError::NoSuchNode);
        }
        let mut doomed = vec![id];
        let mut i = 0;
        while i < doomed.len() {
            let cur = doomed[i];
            doomed.extend_from_slice(&self.nodes[cur].children.clone());
            i += 1;
        }
        let doomed_set: HashSet<NodeId> = doomed.iter().copied().collect();

        if let Some(parent) = self.nodes[id].parent {
            self.nodes[parent].children.retain(|c| *c != id);
        }
        for d in &doomed {
            self.nodes.remove(*d);
        }
        // Unwire anything that pointed at a deleted node.
        for (_, node) in self.nodes.iter_mut() {
            let mut touched = false;
            for slot in node.inputs.iter_mut() {
                if slot.map(|s| doomed_set.contains(&s)).unwrap_or(false) {
                    *slot = None;
                    touched = true;
                }
            }
            if touched {
                node.revision += 1;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- wiring

    pub fn connect(
        &mut self,
        src: NodeId,
        dst: NodeId,
        input_index: usize,
    ) -> Result<(), GraphError> {
        if !self.nodes.contains_key(src) || !self.nodes.contains_key(dst) {
            return Err(GraphError::NoSuchNode);
        }
        if input_index >= self.nodes[dst].inputs.len() {
            return Err(GraphError::BadInputIndex(input_index));
        }
        let (sf, df) = (self.nodes[src].family, self.nodes[dst].family);
        if sf != df {
            return Err(GraphError::FamilyMismatch(sf.suffix(), df.suffix()));
        }
        if src == dst || self.reaches_upstream(src, dst) {
            return Err(GraphError::WouldCycle);
        }
        let n = &mut self.nodes[dst];
        n.inputs[input_index] = Some(src);
        n.revision += 1;
        Ok(())
    }

    pub fn disconnect(&mut self, dst: NodeId, input_index: usize) -> Result<(), GraphError> {
        let n = self.nodes.get_mut(dst).ok_or(GraphError::NoSuchNode)?;
        if input_index >= n.inputs.len() {
            return Err(GraphError::BadInputIndex(input_index));
        }
        n.inputs[input_index] = None;
        n.revision += 1;
        Ok(())
    }

    /// Is `target` anywhere upstream of `from` (following inputs)?
    fn reaches_upstream(&self, from: NodeId, target: NodeId) -> bool {
        let mut stack = vec![from];
        let mut seen = HashSet::new();
        while let Some(cur) = stack.pop() {
            if cur == target {
                return true;
            }
            if !seen.insert(cur) {
                continue;
            }
            for input in self.nodes[cur].inputs.iter().flatten() {
                stack.push(*input);
            }
        }
        false
    }

    /// Every node that takes `id` as an input.
    pub fn consumers(&self, id: NodeId) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.inputs.iter().flatten().any(|i| *i == id))
            .map(|(k, _)| k)
            .collect()
    }

    // --------------------------------------------------------------- paths

    /// `/geo1/noise1` — the address artists and scripts use.
    pub fn path(&self, id: NodeId) -> String {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            if c == self.root {
                break;
            }
            let n = &self.nodes[c];
            parts.push(n.name.clone());
            cur = n.parent;
        }
        parts.reverse();
        format!("/{}", parts.join("/"))
    }

    pub fn find(&self, path: &str) -> Option<NodeId> {
        let mut cur = self.root;
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            let next = self.nodes[cur]
                .children
                .iter()
                .find(|c| self.nodes[**c].name == seg)?;
            cur = *next;
        }
        Some(cur)
    }

    // ---------------------------------------------------------- parameters

    pub fn set_param(&mut self, id: NodeId, key: &str, value: Value) -> Result<(), GraphError> {
        let n = self.nodes.get_mut(id).ok_or(GraphError::NoSuchNode)?;
        if let Some(p) = n.params.get_mut(key) {
            p.set_constant(value);
            n.revision += 1;
        }
        Ok(())
    }

    pub fn set_expression(&mut self, id: NodeId, key: &str, src: &str) -> Result<(), GraphError> {
        let n = self.nodes.get_mut(id).ok_or(GraphError::NoSuchNode)?;
        if let Some(p) = n.params.get_mut(key) {
            p.set_expression(src);
            n.revision += 1;
        }
        Ok(())
    }

    /// Depth-first walk of the whole hierarchy, parents before children.
    pub fn walk(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![self.root];
        while let Some(cur) = stack.pop() {
            out.push(cur);
            for c in self.nodes[cur].children.iter().rev() {
                stack.push(*c);
            }
        }
        out
    }

    /// Build a `SecondaryMap` sized for this graph. Used by the cook engine
    /// and by the GPU crate to hang per-node state off node ids.
    pub fn secondary<T>(&self) -> SecondaryMap<NodeId, T> {
        SecondaryMap::new()
    }

    /// Debug helper — a stable, human-readable id for logging.
    pub fn debug_id(&self, id: NodeId) -> String {
        if self.nodes.contains_key(id) {
            self.path(id)
        } else {
            format!("<dead {:?}>", id.data())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ops::{TEST_PASS, registry};

    fn two_chain() -> (Graph, NodeId, NodeId) {
        let mut g = Graph::new();
        let reg = registry();
        let root = g.root();
        let a = g.create(root, reg.get("pass").unwrap(), None).unwrap();
        let b = g.create(root, reg.get("pass").unwrap(), None).unwrap();
        g.connect(a, b, 0).unwrap();
        (g, a, b)
    }

    #[test]
    fn names_are_auto_numbered() {
        let (g, a, b) = two_chain();
        assert_eq!(g.node(a).name, "pass1");
        assert_eq!(g.node(b).name, "pass2");
        assert_eq!(g.path(b), "/pass2");
        assert_eq!(g.find("/pass2"), Some(b));
    }

    #[test]
    fn an_explicit_name_is_kept_exactly() {
        let mut g = Graph::new();
        let reg = registry();
        let root = g.root();
        let a = g
            .create(root, reg.get("pass").unwrap(), Some("myBlur"))
            .unwrap();
        assert_eq!(g.node(a).name, "myBlur");
        // ...unless it collides, in which case it is numbered.
        let b = g
            .create(root, reg.get("pass").unwrap(), Some("myBlur"))
            .unwrap();
        assert_eq!(g.node(b).name, "myBlur1");
    }

    #[test]
    fn cycles_are_rejected() {
        let (mut g, a, b) = two_chain();
        assert_eq!(g.connect(b, a, 0), Err(GraphError::WouldCycle));
        assert_eq!(g.connect(a, a, 0), Err(GraphError::WouldCycle));
    }

    #[test]
    fn families_must_match() {
        let mut g = Graph::new();
        let reg = registry();
        let root = g.root();
        let top = g.create(root, reg.get("pass").unwrap(), None).unwrap();
        let chop = g.create(root, reg.get("chop_pass").unwrap(), None).unwrap();
        assert!(matches!(
            g.connect(top, chop, 0),
            Err(GraphError::FamilyMismatch(..))
        ));
    }

    #[test]
    fn deleting_a_node_unwires_its_consumers() {
        let (mut g, a, b) = two_chain();
        g.remove(a).unwrap();
        assert!(!g.contains(a));
        assert_eq!(g.node(b).inputs[0], None);
    }

    #[test]
    fn deleting_a_component_takes_its_children() {
        let mut g = Graph::new();
        let reg = registry();
        let root = g.root();
        let comp = g.create(root, reg.get("container").unwrap(), None).unwrap();
        let inner = g.create(comp, reg.get("pass").unwrap(), None).unwrap();
        assert_eq!(g.path(inner), "/container1/pass1");
        g.remove(comp).unwrap();
        assert!(!g.contains(inner));
    }

    #[test]
    fn mutation_bumps_revision() {
        let (mut g, a, _) = two_chain();
        let before = g.node(a).revision();
        g.set_param(a, TEST_PASS, Value::Float(2.0)).unwrap();
        assert!(g.node(a).revision() > before);
    }
}
