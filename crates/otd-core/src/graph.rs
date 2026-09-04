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

/// Whether an operator surfaces as a connector on its parent component.
///
/// PLAN.md §2.4: "In/Out ops inside a component surface as typed connectors
/// on the node." That is the whole encapsulation mechanism — a component's
/// shape is defined by what is inside it, not declared separately.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Connector {
    #[default]
    None,
    In,
    Out,
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
    /// The family each input accepts. Usually the node's own family, but a
    /// component's connectors take theirs from its In operators, which is how
    /// one component can accept a TOP and a CHOP.
    pub input_families: Vec<Family>,
    pub connector: Connector,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Editor position. Part of the project file — node layout is authored
    /// information, not incidental UI state.
    pub pos: [f32; 2],
    pub flags: NodeFlags,
    /// Operators that must re-cook every frame regardless of parameter
    /// changes (a movie player, a device input).
    pub intrinsically_time_dependent: bool,
    /// This component's contents come from a `.otdc` file rather than from
    /// the project. The project stores the reference; the file stores the
    /// network (PLAN.md §5, Phase 3).
    pub external: Option<String>,
    /// This component tracks another one in the same project: its contents
    /// are replaced by copies of the master's whenever the master changes.
    pub clone_of: Option<String>,
    /// The master's subtree revision at the last sync, so an unchanged
    /// master costs nothing.
    pub clone_synced_at: u64,
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
        self.params.values().flat_map(|p| p.referenced_ops())
    }

    /// True when some parameter reads a custom parameter on the enclosing
    /// component, which makes this node depend on that component's settings.
    pub fn references_parent(&self) -> bool {
        self.params.values().any(|p| {
            p.mode == crate::param::ParamMode::Expression && p.expression.contains("parent.")
        })
    }

    /// The author-defined parameters on this node, in declaration order.
    pub fn custom_params(&self) -> impl Iterator<Item = (&String, &Param)> {
        self.params.iter().filter(|(_, p)| p.custom)
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
    /// The family each input accepts, when it is not this operator's own.
    ///
    /// Almost every operator wires within one family, which is the rule that
    /// keeps the graph legible (PLAN.md §2.1) — so the common case is `&[]`,
    /// meaning "all mine". The exceptions are the **converter operators**,
    /// which are the sanctioned way to cross a family boundary: a CHOP to TOP
    /// takes a CHOP input and produces a texture. Declaring the exception per
    /// input rather than per operator lets a converter also take inputs of its
    /// own family.
    pub input_families: &'static [Family],
    pub summary: &'static str,
    pub time_dependent: bool,
    pub params: fn() -> IndexMap<String, Param>,
    /// Set for the In and Out operators that give a component its shape.
    pub connector: Connector,
}

impl OpDef {
    /// What each input accepts, one entry per declared input. Falls back to
    /// this operator's own family for any input the definition did not
    /// override, so a converter only has to name the inputs that cross.
    pub fn accepted_families(&self) -> Vec<Family> {
        (0..self.inputs.len())
            .map(|i| self.input_families.get(i).copied().unwrap_or(self.family))
            .collect()
    }
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
    #[error("no parameter `{0}` on that operator")]
    NoSuchParam(String),
}

#[derive(Debug, Clone)]
pub struct Graph {
    nodes: SlotMap<NodeId, Node>,
    root: NodeId,
    /// The directory the project file lives in, when it came from one.
    ///
    /// External component references are stored exactly as authored, which
    /// means a bundle can hold `components/meter.otdc` and stay portable. That
    /// only works if something remembers what the path is relative *to* — the
    /// process's working directory is the wrong answer, and is what makes a
    /// project open by hand and fail from a service.
    base_dir: Option<std::path::PathBuf>,
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
            input_families: Vec::new(),
            connector: Connector::None,
            parent: None,
            children: Vec::new(),
            pos: [0.0, 0.0],
            flags: NodeFlags::default(),
            intrinsically_time_dependent: false,
            external: None,
            clone_of: None,
            clone_synced_at: 0,
            revision: 0,
        });
        Graph {
            nodes,
            root,
            base_dir: None,
        }
    }

    /// The directory external component references resolve against.
    pub fn base_dir(&self) -> Option<&std::path::Path> {
        self.base_dir.as_deref()
    }

    pub fn set_base_dir(&mut self, dir: Option<std::path::PathBuf>) {
        self.base_dir = dir;
    }

    /// An external reference as an openable path.
    pub fn resolve_external(&self, file: &str) -> std::path::PathBuf {
        match &self.base_dir {
            Some(dir) if std::path::Path::new(file).is_relative() => dir.join(file),
            _ => std::path::PathBuf::from(file),
        }
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

    pub(crate) fn get_mut_raw(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id)
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
            input_families: def.accepted_families(),
            connector: def.connector,
            parent: Some(parent),
            children: Vec::new(),
            pos: [0.0, 0.0],
            flags: NodeFlags::default(),
            intrinsically_time_dependent: def.time_dependent,
            external: None,
            clone_of: None,
            clone_synced_at: 0,
            revision: 1,
        };
        let id = self.nodes.insert(node);
        self.nodes[parent].children.push(id);
        // A new In or Out operator changes its component's shape.
        self.sync_connectors(parent);
        Ok(id)
    }

    // -------------------------------------------------- component connectors

    /// The In (or Out) operators of a component, in name order, which is the
    /// order their connectors appear on the node.
    pub fn connector_ops(&self, comp: NodeId, kind: Connector) -> Vec<NodeId> {
        let Some(node) = self.nodes.get(comp) else {
            return Vec::new();
        };
        let mut ops: Vec<NodeId> = node
            .children
            .iter()
            .copied()
            .filter(|c| self.nodes[*c].connector == kind)
            .collect();
        ops.sort_by(|a, b| self.nodes[*a].name.cmp(&self.nodes[*b].name));
        ops
    }

    /// Recompute a component's input connectors from the In operators inside
    /// it. Existing connections keep their index, so adding a second input
    /// does not unwire the first.
    pub fn sync_connectors(&mut self, comp: NodeId) {
        if self.nodes.get(comp).map(|n| n.family) != Some(Family::Comp) {
            return;
        }
        let ins = self.connector_ops(comp, Connector::In);
        let labels: Vec<String> = ins.iter().map(|i| self.nodes[*i].name.clone()).collect();
        let families: Vec<Family> = ins.iter().map(|i| self.nodes[*i].family).collect();

        let node = &mut self.nodes[comp];
        if node.input_labels == labels && node.input_families == families {
            return;
        }
        node.inputs.resize(ins.len(), None);
        node.input_labels = labels;
        node.input_families = families;
        node.revision += 1;
    }

    /// The family a node presents on its output: its own, or — for a
    /// component — that of the Out operator inside it.
    pub fn output_family(&self, id: NodeId) -> Option<Family> {
        let node = self.nodes.get(id)?;
        if node.family != Family::Comp {
            return Some(node.family);
        }
        self.connector_ops(id, Connector::Out)
            .first()
            .map(|o| self.nodes[*o].family)
    }

    /// The node that actually produces what `id` presents, following bypass
    /// flags and stepping out of components into their Out operator.
    pub fn resolve_output(&self, id: NodeId) -> Option<NodeId> {
        let mut cur = id;
        for _ in 0..64 {
            let node = self.nodes.get(cur)?;
            if node.flags.bypass {
                cur = (*node.inputs.first()?)?;
                continue;
            }
            if node.family == Family::Comp {
                cur = *self.connector_ops(cur, Connector::Out).first()?;
                continue;
            }
            return Some(cur);
        }
        None
    }

    /// For an In operator, the node outside the component that feeds it.
    pub fn connector_source(&self, in_op: NodeId) -> Option<NodeId> {
        let node = self.nodes.get(in_op)?;
        if node.connector != Connector::In {
            return None;
        }
        let comp = node.parent?;
        let index = self
            .connector_ops(comp, Connector::In)
            .iter()
            .position(|i| *i == in_op)?;
        let outside = (*self.nodes.get(comp)?.inputs.get(index)?)?;
        self.resolve_output(outside)
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

        let parent = self.nodes[id].parent;
        if let Some(parent) = parent {
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
        // Deleting an In operator removes a connector from its component.
        if let Some(parent) = parent {
            self.sync_connectors(parent);
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
        // Compare against what the source *presents* and what this particular
        // input *accepts*, which for a component are decided by the operators
        // inside it rather than by the node's own family.
        let sf = self
            .output_family(src)
            .ok_or(GraphError::FamilyMismatch("nothing", "an input"))?;
        let df = self.nodes[dst].input_families[input_index];
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

    /// Resolve a path the way an operator's reference parameter reads it:
    /// `/x/y` from the root, anything else from the referencing node's own
    /// component, with `..` stepping out. A component whose Feedback targets
    /// `out1` therefore works in every instance and in every project — an
    /// absolute path would nail it to one address.
    pub fn find_from(&self, from: NodeId, path: &str) -> Option<NodeId> {
        let path = path.trim();
        if path.starts_with('/') {
            return self.find(path);
        }
        let mut cur = self.nodes.get(from)?.parent.unwrap_or(self.root);
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            match seg {
                "." => {}
                ".." => cur = self.nodes[cur].parent.unwrap_or(self.root),
                _ => {
                    cur = *self.nodes[cur]
                        .children
                        .iter()
                        .find(|c| self.nodes[**c].name == seg)?;
                }
            }
        }
        Some(cur)
    }

    // ---------------------------------------------------------- parameters

    pub fn set_param(&mut self, id: NodeId, key: &str, value: Value) -> Result<(), GraphError> {
        let n = self.nodes.get_mut(id).ok_or(GraphError::NoSuchNode)?;
        let custom = n.params.get(key).map(|p| p.custom).unwrap_or(false);
        if let Some(p) = n.params.get_mut(key) {
            p.set_constant(value);
            n.revision += 1;
        }
        if custom {
            self.dirty_descendants(id);
        }
        Ok(())
    }

    /// Add an author-defined parameter. On a component this is how it gets an
    /// API: operators inside read it as `parent.<key>`.
    pub fn add_custom_param(&mut self, id: NodeId, key: &str, param: Param) {
        if let Some(n) = self.nodes.get_mut(id) {
            n.params.insert(key.to_string(), param.as_custom());
            n.revision += 1;
        }
        self.dirty_descendants(id);
    }

    /// Remove an author-defined parameter. Operator-defined ones are left
    /// alone — deleting those would leave the operator unable to cook.
    pub fn remove_custom_param(&mut self, id: NodeId, key: &str) {
        let is_custom = self
            .nodes
            .get(id)
            .and_then(|n| n.params.get(key))
            .map(|p| p.custom)
            .unwrap_or(false);
        if !is_custom {
            return;
        }
        if let Some(n) = self.nodes.get_mut(id) {
            n.params.shift_remove(key);
            n.revision += 1;
        }
        self.dirty_descendants(id);
    }

    /// Mark everything under `id` as changed.
    ///
    /// A component's custom parameters are read by expressions inside it, and
    /// those reads are not wires, so there is nothing for the cook engine to
    /// follow. Touching the subtree on edit is O(subtree) once per edit rather
    /// than per frame, which is the right trade.
    fn dirty_descendants(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            let children = match self.nodes.get(cur) {
                Some(n) => n.children.clone(),
                None => continue,
            };
            for c in children {
                self.nodes[c].revision += 1;
                stack.push(c);
            }
        }
    }

    pub fn set_expression(&mut self, id: NodeId, key: &str, src: &str) -> Result<(), GraphError> {
        let n = self.nodes.get_mut(id).ok_or(GraphError::NoSuchNode)?;
        let custom = n.params.get(key).map(|p| p.custom).unwrap_or(false);
        if let Some(p) = n.params.get_mut(key) {
            p.set_expression(src);
            n.revision += 1;
        }
        if custom {
            self.dirty_descendants(id);
        }
        Ok(())
    }

    /// Put a parameter into Export mode, reading `channel` off the CHOP at
    /// `op_path`.
    ///
    /// The same edit the parameter panel makes when a channel is dragged onto
    /// a row, reached without a pointer — which is what the assistant needs,
    /// since Export is how anything in this program reacts to anything else
    /// and a plan that cannot express it can only build patches that sit
    /// still.
    pub fn set_export(
        &mut self,
        id: NodeId,
        key: &str,
        op_path: &str,
        channel: &str,
    ) -> Result<(), GraphError> {
        let n = self.nodes.get_mut(id).ok_or(GraphError::NoSuchNode)?;
        let custom = n.params.get(key).map(|p| p.custom).unwrap_or(false);
        let Some(p) = n.params.get_mut(key) else {
            return Err(GraphError::NoSuchParam(key.to_string()));
        };
        p.set_export(op_path, channel);
        n.revision += 1;
        if custom {
            self.dirty_descendants(id);
        }
        Ok(())
    }

    /// True when some ancestor of this node is an external component, whose
    /// contents belong to its own file rather than to the project.
    pub fn is_inside_external(&self, id: NodeId) -> bool {
        let mut cur = self.get(id).and_then(|n| n.parent);
        while let Some(c) = cur {
            let Some(node) = self.get(c) else {
                return false;
            };
            if node.external.is_some() {
                return true;
            }
            cur = node.parent;
        }
        false
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
