//! The text project format.
//!
//! PLAN.md calls a git-diffable project format a headline feature, so it is
//! designed here rather than retrofitted. Two decisions carry that:
//!
//!  * **Nodes are a flat, path-sorted list**, not a nested tree. Adding a node
//!    to a component appends one block instead of re-indenting a subtree.
//!  * **Everything defaultable is omitted.** A freshly created operator writes
//!    only its path, type and position; parameters appear in the file only
//!    once the artist has touched them. Diffs then read as a list of the
//!    actual authoring decisions.
//!
//! Wiring is stored on the *consumer* as input paths, which keeps a rewire to
//! a single changed line.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::graph::{Graph, NodeFlags, NodeId, OpRegistry};
use crate::param::Param;

pub const FORMAT_ID: &str = "opentouchdesigner-project";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("not an OpenTouchDesigner project file")]
    NotAProject,
    #[error("project format version {0} is newer than this build understands ({FORMAT_VERSION})")]
    TooNew(u32),
    #[error("unknown operator type `{op}` at {path}")]
    UnknownOp { op: String, path: String },
    #[error("{path} refers to input `{input}`, which does not exist")]
    DanglingInput { path: String, input: String },
    #[error("bad path `{0}`")]
    BadPath(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("serialise error: {0}")]
    Serialise(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeEntry {
    pub path: String,
    pub op: String,
    #[serde(default, skip_serializing_if = "is_origin")]
    pub pos: (f32, f32),
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub params: IndexMap<String, Param>,
    #[serde(default, skip_serializing_if = "is_default_flags")]
    pub flags: NodeFlags,
}

fn is_origin(p: &(f32, f32)) -> bool {
    p.0 == 0.0 && p.1 == 0.0
}
fn is_default_flags(f: &NodeFlags) -> bool {
    *f == NodeFlags::default()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub format: String,
    pub version: u32,
    #[serde(default = "default_fps")]
    pub fps: f64,
    pub nodes: Vec<NodeEntry>,
}

fn default_fps() -> f64 {
    60.0
}

impl Project {
    /// Snapshot a graph. Parameters left at their operator defaults are not
    /// written — see the module docs.
    pub fn from_graph(graph: &Graph, registry: &OpRegistry, fps: f64) -> Project {
        // BTreeMap gives a deterministic, path-sorted file, and guarantees a
        // parent is written before its children on load.
        let mut entries: BTreeMap<String, NodeEntry> = BTreeMap::new();

        for id in graph.walk() {
            if id == graph.root() {
                continue;
            }
            let node = graph.node(id);
            let path = graph.path(id);
            let defaults = registry.get(&node.op_type).map(|d| (d.params)());

            let mut params = IndexMap::new();
            for (key, param) in &node.params {
                let is_default = defaults
                    .as_ref()
                    .and_then(|d| d.get(key))
                    .map(|d| param_matches_default(param, d))
                    .unwrap_or(false);
                // A custom parameter has no operator definition behind it, so
                // it is written in full — it *is* the definition.
                if param.custom {
                    params.insert(key.clone(), param.clone());
                } else if !is_default {
                    // Labels, ranges and menus belong to the operator
                    // definition, not to the project. Writing them would put
                    // UI metadata in every diff and make an operator's
                    // relabelling look like a project change.
                    let mut param = param.clone();
                    param.label = String::new();
                    param.range = None;
                    param.menu = None;
                    params.insert(key.clone(), param);
                }
            }

            let inputs: Vec<String> = node
                .inputs
                .iter()
                .map(|slot| slot.map(|s| graph.path(s)).unwrap_or_default())
                .collect();
            // Trailing unconnected inputs carry no information.
            let inputs = trim_trailing_empty(inputs);

            entries.insert(
                path.clone(),
                NodeEntry {
                    path,
                    op: node.op_type.clone(),
                    pos: (node.pos[0], node.pos[1]),
                    inputs,
                    params,
                    flags: node.flags,
                },
            );
        }

        Project {
            format: FORMAT_ID.to_string(),
            version: FORMAT_VERSION,
            fps,
            nodes: entries.into_values().collect(),
        }
    }

    /// Rebuild a graph. Fails loudly on unknown operators and dangling wires
    /// rather than silently dropping a patch's connections.
    pub fn to_graph(&self, registry: &OpRegistry) -> Result<Graph, ProjectError> {
        if self.format != FORMAT_ID {
            return Err(ProjectError::NotAProject);
        }
        if self.version > FORMAT_VERSION {
            return Err(ProjectError::TooNew(self.version));
        }

        let mut graph = Graph::new();
        let mut by_path: BTreeMap<String, NodeId> = BTreeMap::new();

        // Pass 1 — create every node. Entries are path-sorted, so a parent
        // always exists by the time its children are read.
        for entry in &self.nodes {
            let (parent_path, name) =
                split_path(&entry.path).ok_or_else(|| ProjectError::BadPath(entry.path.clone()))?;
            let parent = if parent_path.is_empty() {
                graph.root()
            } else {
                *by_path
                    .get(&parent_path)
                    .ok_or_else(|| ProjectError::BadPath(entry.path.clone()))?
            };
            let def = registry
                .get(&entry.op)
                .ok_or_else(|| ProjectError::UnknownOp {
                    op: entry.op.clone(),
                    path: entry.path.clone(),
                })?;
            let id = graph
                .create(parent, def, Some(&name))
                .map_err(|_| ProjectError::BadPath(entry.path.clone()))?;

            {
                let node = graph.node_mut(id);
                node.pos = [entry.pos.0, entry.pos.1];
                node.flags = entry.flags;
                for (key, saved) in &entry.params {
                    // A custom parameter exists only in the file; restore it
                    // whole, including its type, label and range.
                    if saved.custom {
                        let mut saved = saved.clone();
                        saved.recompile();
                        node.params.insert(key.clone(), saved);
                        continue;
                    }
                    if let Some(slot) = node.params.get_mut(key) {
                        let mut saved = saved.clone();
                        // Keep the operator's declared type authoritative;
                        // a hand-edited file can't turn a float into a string.
                        if !slot.value.same_type_as(&saved.value) {
                            saved.value = slot.value.clone();
                        }
                        saved.label = slot.label.clone();
                        // Whether a parameter holds script source is part of
                        // the operator definition, not of the project.
                        if slot.is_script() {
                            saved = saved.into_script();
                        }
                        saved.range = slot.range.or(saved.range);
                        saved.menu = slot.menu.clone().or(saved.menu);
                        saved.recompile();
                        *slot = saved;
                    }
                }
            }
            by_path.insert(entry.path.clone(), id);
        }

        // Pass 2 — wire.
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

        Ok(graph)
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

    pub fn from_ron(src: &str) -> Result<Project, ProjectError> {
        ron::from_str(src).map_err(|e| ProjectError::Parse(e.to_string()))
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ProjectError> {
        std::fs::write(path, self.to_ron()?)?;
        Ok(())
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Project, ProjectError> {
        Project::from_ron(&std::fs::read_to_string(path)?)
    }
}

fn param_matches_default(a: &Param, b: &Param) -> bool {
    a.mode == b.mode && a.value == b.value && a.expression == b.expression && a.source == b.source
}

fn trim_trailing_empty(mut v: Vec<String>) -> Vec<String> {
    while v.last().map(|s| s.is_empty()).unwrap_or(false) {
        v.pop();
    }
    v
}

/// `/a/b/c` -> (`/a/b`, `c`)
fn split_path(path: &str) -> Option<(String, String)> {
    let trimmed = path.trim_end_matches('/');
    let idx = trimmed.rfind('/')?;
    let name = &trimmed[idx + 1..];
    if name.is_empty() {
        return None;
    }
    Some((trimmed[..idx].to_string(), name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_ops::{TEST_PASS, registry};
    use crate::value::Value;

    fn sample() -> (Graph, OpRegistry) {
        let reg = registry();
        let mut g = Graph::new();
        let root = g.root();
        let comp = g.create(root, reg.get("container").unwrap(), None).unwrap();
        let a = g.create(comp, reg.get("pass").unwrap(), None).unwrap();
        let b = g.create(root, reg.get("comp2").unwrap(), None).unwrap();
        let m = g.create(root, reg.get("movie").unwrap(), None).unwrap();
        g.connect(m, b, 0).unwrap();
        g.set_param(a, TEST_PASS, Value::Float(0.375)).unwrap();
        g.set_expression(b, TEST_PASS, "sin(absTime) * 0.5")
            .unwrap();
        g.node_mut(a).pos = [120.0, -40.0];
        g.node_mut(b).flags.bypass = true;
        (g, reg)
    }

    #[test]
    fn round_trips_through_text() {
        let (g, reg) = sample();
        let text = Project::from_graph(&g, &reg, 60.0).to_ron().unwrap();
        let g2 = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();

        assert_eq!(g.len(), g2.len());
        for id in g.walk() {
            let path = g.path(id);
            let id2 = g2.find(&path).unwrap_or_else(|| panic!("missing {path}"));
            let (n, n2) = (g.node(id), g2.node(id2));
            assert_eq!(n.op_type, n2.op_type);
            assert_eq!(n.pos, n2.pos);
            assert_eq!(n.flags, n2.flags);
            for (k, p) in &n.params {
                let p2 = &n2.params[k];
                assert_eq!(p.mode, p2.mode, "{path}.{k} mode");
                assert_eq!(p.value, p2.value, "{path}.{k} value");
                assert_eq!(p.expression, p2.expression, "{path}.{k} expression");
            }
            for (i, slot) in n.inputs.iter().enumerate() {
                assert_eq!(
                    slot.map(|s| g.path(s)),
                    n2.inputs[i].map(|s| g2.path(s)),
                    "{path} input {i}"
                );
            }
        }

        // Second round trip must be byte-identical: no churn in git.
        let text2 = Project::from_graph(&g2, &reg, 60.0).to_ron().unwrap();
        assert_eq!(text, text2);
    }

    #[test]
    fn expressions_are_live_again_after_loading() {
        let (g, reg) = sample();
        let text = Project::from_graph(&g, &reg, 60.0).to_ron().unwrap();
        let g2 = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();
        let b = g2.find("/comp1").unwrap();
        assert!(
            g2.node(b).has_time_dependent_param(),
            "a loaded expression must re-arm time dependence"
        );
    }

    #[test]
    fn untouched_parameters_are_not_written() {
        let (g, reg) = sample();
        let text = Project::from_graph(&g, &reg, 60.0).to_ron().unwrap();
        let movie_block = text
            .lines()
            .skip_while(|l| !l.contains("path: \"/movie1\""))
            .take(4)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !movie_block.contains("params"),
            "default params leaked into the file:\n{text}"
        );
    }

    #[test]
    fn an_unknown_operator_is_an_error_not_a_silent_drop() {
        let (g, reg) = sample();
        let text = Project::from_graph(&g, &reg, 60.0)
            .to_ron()
            .unwrap()
            .replace("\"movie\"", "\"nonesuch\"");
        let err = Project::from_ron(&text)
            .unwrap()
            .to_graph(&reg)
            .unwrap_err();
        assert!(matches!(err, ProjectError::UnknownOp { .. }), "{err}");
    }

    #[test]
    fn a_future_format_version_is_refused() {
        let (g, reg) = sample();
        let mut p = Project::from_graph(&g, &reg, 60.0);
        p.version = FORMAT_VERSION + 1;
        assert!(matches!(
            p.to_graph(&reg).unwrap_err(),
            ProjectError::TooNew(_)
        ));
    }
}
