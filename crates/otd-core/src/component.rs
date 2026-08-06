//! External component files (`.otdc`) and clones.
//!
//! PLAN.md Phase 3 asks for "external component files (.otdc, text) +
//! git-friendly merges as a showcase". Two mechanisms, with one thing in
//! common: a component's *contents* stop being duplicated.
//!
//!  * An **external** component keeps its network in its own file. The
//!    project stores a path and the settings of the instance; the file stores
//!    the network. Two projects using the same component share one file, and
//!    a change to it is a diff in one place.
//!  * A **clone** tracks another component in the same project. Its contents
//!    are replaced by copies of the master's whenever the master changes,
//!    while its own parameter values are left alone — which is the point, or
//!    every instance would be identical.
//!
//! Both preserve the instance's own settings across a re-expansion. Losing
//! the values an artist dialled in because a shared definition changed would
//! make either feature unusable.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::graph::{Graph, NodeId, OpRegistry};
use crate::param::Param;
use crate::project::{NodeEntry, ProjectError, node_entry, restore_node};

pub const COMPONENT_FORMAT_ID: &str = "opentouchdesigner-component";
pub const COMPONENT_FORMAT_VERSION: u32 = 1;

/// A component saved on its own: the operators inside it, plus the custom
/// parameters that make up its API.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub format: String,
    pub version: u32,
    /// What the component was called where it was saved from. Used as the
    /// default name when instantiating.
    pub name: String,
    /// The component's own API. Values here are defaults for a new instance.
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub params: indexmap::IndexMap<String, Param>,
    /// The network inside, with paths relative to the component.
    pub nodes: Vec<NodeEntry>,
}

impl Component {
    /// Extract a component from a graph.
    pub fn from_graph(graph: &Graph, comp: NodeId, registry: &OpRegistry) -> Option<Component> {
        let node = graph.get(comp)?;
        let prefix = graph.path(comp);

        // Path-sorted, like the project format, so a parent is always written
        // before its children and the file is stable.
        let mut entries: BTreeMap<String, NodeEntry> = BTreeMap::new();
        let mut stack: Vec<NodeId> = node.children.clone();
        while let Some(id) = stack.pop() {
            stack.extend(graph.node(id).children.iter().copied());
            let mut entry = node_entry(graph, id, registry);
            entry.path = strip_prefix(&entry.path, &prefix);
            entry.inputs = entry
                .inputs
                .iter()
                .map(|i| {
                    if i.is_empty() {
                        String::new()
                    } else {
                        strip_prefix(i, &prefix)
                    }
                })
                .collect();
            entries.insert(entry.path.clone(), entry);
        }

        Some(Component {
            format: COMPONENT_FORMAT_ID.to_string(),
            version: COMPONENT_FORMAT_VERSION,
            name: node.name.clone(),
            params: node
                .custom_params()
                .map(|(k, p)| (k.clone(), p.clone()))
                .collect(),
            nodes: entries.into_values().collect(),
        })
    }

    /// Fill a component node with this component's contents.
    ///
    /// Existing children are removed first, and the node's own custom
    /// parameter *values* are preserved where the names still match — an
    /// instance keeps its settings when the definition it came from is
    /// re-read.
    pub fn expand_into(
        &self,
        graph: &mut Graph,
        comp: NodeId,
        registry: &OpRegistry,
    ) -> Result<(), ProjectError> {
        if self.format != COMPONENT_FORMAT_ID {
            return Err(ProjectError::NotAProject);
        }
        if self.version > COMPONENT_FORMAT_VERSION {
            return Err(ProjectError::TooNew(self.version));
        }

        let kept: Vec<(String, Param)> = graph
            .get(comp)
            .map(|n| {
                n.custom_params()
                    .map(|(k, p)| (k.clone(), p.clone()))
                    .collect()
            })
            .unwrap_or_default();

        for child in graph.children(comp).to_vec() {
            let _ = graph.remove(child);
        }

        // The API first, so expressions inside can resolve `parent.x` as soon
        // as they are created.
        for (key, param) in &self.params {
            let mut param = param.clone();
            if let Some((_, existing)) = kept.iter().find(|(k, _)| k == key) {
                if existing.value.same_type_as(&param.value) {
                    param.value = existing.value.clone();
                    param.mode = existing.mode;
                    param.expression = existing.expression.clone();
                    param.source = existing.source.clone();
                    param.recompile();
                }
            }
            graph.add_custom_param(comp, key, param);
        }

        let mut by_path: BTreeMap<String, NodeId> = BTreeMap::new();
        by_path.insert(String::new(), comp);

        for entry in &self.nodes {
            let (parent_path, name) = split_relative(&entry.path)
                .ok_or_else(|| ProjectError::BadPath(entry.path.clone()))?;
            let parent = *by_path
                .get(&parent_path)
                .ok_or_else(|| ProjectError::BadPath(entry.path.clone()))?;
            let def = registry
                .get(&entry.op)
                .ok_or_else(|| ProjectError::UnknownOp {
                    op: entry.op.clone(),
                    path: entry.path.clone(),
                })?
                .clone();
            let id = graph
                .create(parent, &def, Some(&name))
                .map_err(|_| ProjectError::BadPath(entry.path.clone()))?;
            restore_node(graph, id, entry);
            by_path.insert(entry.path.clone(), id);
        }

        for entry in &self.nodes {
            let dst = by_path[&entry.path];
            for (i, src_path) in entry.inputs.iter().enumerate() {
                if src_path.is_empty() {
                    continue;
                }
                let src = *by_path
                    .get(src_path)
                    .ok_or_else(|| ProjectError::DanglingInput {
                        path: entry.path.clone(),
                        input: src_path.clone(),
                    })?;
                graph
                    .connect(src, dst, i)
                    .map_err(|_| ProjectError::DanglingInput {
                        path: entry.path.clone(),
                        input: src_path.clone(),
                    })?;
            }
        }
        Ok(())
    }

    pub fn to_ron(&self) -> Result<String, ProjectError> {
        let cfg = ron::ser::PrettyConfig::new()
            .depth_limit(6)
            .indentor("  ")
            .struct_names(false)
            .separate_tuple_members(false)
            .enumerate_arrays(false);
        let mut s = ron::ser::to_string_pretty(self, cfg)
            .map_err(|e| ProjectError::Serialise(e.to_string()))?;
        s.push('\n');
        Ok(s)
    }

    pub fn from_ron(src: &str) -> Result<Component, ProjectError> {
        ron::from_str(src).map_err(|e| ProjectError::Parse(e.to_string()))
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ProjectError> {
        std::fs::write(path, self.to_ron()?)?;
        Ok(())
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Component, ProjectError> {
        Component::from_ron(&std::fs::read_to_string(path)?)
    }
}

fn strip_prefix(path: &str, prefix: &str) -> String {
    path.strip_prefix(prefix).unwrap_or(path).to_string()
}

/// `/level1` -> (``, `level1`); `/inner/level1` -> (`/inner`, `level1`).
fn split_relative(path: &str) -> Option<(String, String)> {
    let trimmed = path.trim_end_matches('/');
    let idx = trimmed.rfind('/')?;
    let name = &trimmed[idx + 1..];
    if name.is_empty() {
        return None;
    }
    Some((trimmed[..idx].to_string(), name.to_string()))
}

impl Graph {
    /// Point a component at a `.otdc` file and fill it from there.
    pub fn attach_external(
        &mut self,
        comp: NodeId,
        file: &str,
        registry: &OpRegistry,
    ) -> Result<(), ProjectError> {
        // The reference is resolved for *reading* but stored exactly as it was
        // given. A bundle holding `components/meter.otdc` has to keep saying
        // that, or the first re-save turns it into an absolute path on one
        // machine and the bundle stops being portable.
        let component = Component::load(self.resolve_external(file))?;
        component.expand_into(self, comp, registry)?;
        if let Some(n) = self.get_mut_raw(comp) {
            n.external = Some(file.to_string());
        }
        Ok(())
    }

    /// Make `comp` track `master`.
    pub fn set_clone(&mut self, comp: NodeId, master_path: Option<&str>) {
        if let Some(n) = self.get_mut_raw(comp) {
            n.clone_of = master_path.map(|s| s.to_string());
            n.clone_synced_at = 0;
        }
    }

    /// Sum of the revisions in a subtree — a cheap "has anything in here
    /// changed" number, so an untouched master costs one walk and no copying.
    pub fn subtree_revision(&self, id: NodeId) -> u64 {
        let mut total = 0u64;
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            let Some(node) = self.get(cur) else { continue };
            total = total.wrapping_add(node.revision());
            stack.extend(node.children.iter().copied());
        }
        total
    }

    /// Re-expand every clone whose master has changed.
    ///
    /// Returns how many were re-synced, which the editor reports so the
    /// behaviour is visible rather than mysterious.
    pub fn sync_clones(&mut self, registry: &OpRegistry) -> usize {
        let clones: Vec<(NodeId, String)> = self
            .walk()
            .into_iter()
            .filter_map(|id| {
                let node = self.get(id)?;
                Some((id, node.clone_of.clone()?))
            })
            .collect();

        let mut synced = 0;
        for (id, master_path) in clones {
            let Some(master) = self.find(&master_path) else {
                continue;
            };
            if master == id || self.is_ancestor(master, id) {
                // Cloning something that contains you would expand forever.
                continue;
            }
            let revision = self.subtree_revision(master);
            if self.get(id).map(|n| n.clone_synced_at) == Some(revision) {
                continue;
            }
            let Some(component) = Component::from_graph(self, master, registry) else {
                continue;
            };
            if component.expand_into(self, id, registry).is_ok() {
                if let Some(n) = self.get_mut_raw(id) {
                    n.clone_synced_at = revision;
                }
                synced += 1;
            }
        }
        synced
    }

    fn is_ancestor(&self, ancestor: NodeId, of: NodeId) -> bool {
        let mut cur = self.get(of).and_then(|n| n.parent);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.get(c).and_then(|n| n.parent);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ops::registry as test_registry;
    use crate::value::Value;

    fn build(graph: &mut Graph, reg: &OpRegistry, parent: NodeId, op: &str, name: &str) -> NodeId {
        let def = reg.get(op).unwrap().clone();
        graph.create(parent, &def, Some(name)).unwrap()
    }

    fn master(graph: &mut Graph, reg: &OpRegistry, name: &str) -> NodeId {
        let root = graph.root();
        let comp = build(graph, reg, root, "container", name);
        graph.add_custom_param(comp, "gain", Param::float(1.0));
        let a = build(graph, reg, comp, "pass", "a");
        let b = build(graph, reg, comp, "pass", "b");
        graph.connect(a, b, 0).unwrap();
        comp
    }

    #[test]
    fn a_component_round_trips_through_its_own_file() {
        let reg = test_registry();
        let mut graph = Graph::new();
        let comp = master(&mut graph, &reg, "rig");
        graph.set_param(comp, "gain", Value::Float(2.0)).unwrap();

        let saved = Component::from_graph(&graph, comp, &reg).unwrap();
        let text = saved.to_ron().unwrap();
        // Paths inside the file are relative, so the same component can be
        // dropped in anywhere.
        assert!(text.contains("\"/a\""), "{text}");
        assert!(!text.contains("/rig/"), "{text}");

        let reloaded = Component::from_ron(&text).unwrap();
        let mut other = Graph::new();
        let root = other.root();
        let target = build(&mut other, &reg, root, "container", "elsewhere");
        reloaded.expand_into(&mut other, target, &reg).unwrap();

        assert!(other.find("/elsewhere/a").is_some());
        assert_eq!(
            other.node(other.find("/elsewhere/b").unwrap()).inputs[0],
            other.find("/elsewhere/a")
        );
        assert_eq!(
            other.node(target).param("gain").unwrap().value,
            Value::Float(2.0)
        );
    }

    #[test]
    fn re_expanding_keeps_the_instance_settings() {
        let reg = test_registry();
        let mut graph = Graph::new();
        let comp = master(&mut graph, &reg, "rig");
        let saved = Component::from_graph(&graph, comp, &reg).unwrap();

        // The artist dials in a value on this instance...
        graph.set_param(comp, "gain", Value::Float(3.5)).unwrap();
        // ...and the shared definition is re-read.
        saved.expand_into(&mut graph, comp, &reg).unwrap();
        assert_eq!(
            graph.node(comp).param("gain").unwrap().value,
            Value::Float(3.5),
            "re-expanding must not reset the instance"
        );
    }

    #[test]
    fn a_clone_follows_its_master_but_keeps_its_own_settings() {
        let reg = test_registry();
        let mut graph = Graph::new();
        let m = master(&mut graph, &reg, "master");
        let root = graph.root();
        let c = build(&mut graph, &reg, root, "container", "copy");
        graph.set_clone(c, Some("/master"));

        assert_eq!(graph.sync_clones(&reg), 1);
        assert!(graph.find("/copy/a").is_some());
        assert!(graph.find("/copy/b").is_some());

        // Its own setting, then a change to the master's structure.
        graph.set_param(c, "gain", Value::Float(9.0)).unwrap();
        assert_eq!(graph.sync_clones(&reg), 0, "an unchanged master is free");

        build(&mut graph, &reg, m, "pass", "c");
        assert_eq!(graph.sync_clones(&reg), 1);
        assert!(graph.find("/copy/c").is_some(), "the clone followed");
        assert_eq!(
            graph.node(c).param("gain").unwrap().value,
            Value::Float(9.0),
            "the clone kept its own value"
        );
    }

    #[test]
    fn a_clone_of_an_ancestor_is_refused_rather_than_expanding_forever() {
        let reg = test_registry();
        let mut graph = Graph::new();
        let outer = master(&mut graph, &reg, "outer");
        let inner = build(&mut graph, &reg, outer, "container", "inner");
        graph.set_clone(inner, Some("/outer"));
        assert_eq!(graph.sync_clones(&reg), 0);
        assert!(graph.find("/outer/inner/inner").is_none());
    }

    #[test]
    fn a_dangling_clone_reference_is_ignored() {
        let reg = test_registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let c = build(&mut graph, &reg, root, "container", "copy");
        graph.set_clone(c, Some("/nowhere"));
        assert_eq!(graph.sync_clones(&reg), 0);
    }
}
