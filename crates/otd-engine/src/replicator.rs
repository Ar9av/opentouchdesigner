//! The Replicator: one component per row of a table.
//!
//! PLAN.md §5 Phase 3 lists it with clones, and it is built *out of* clones:
//! the Replicator watches a template DAT and keeps one clone of its master
//! component per data row, named by the row, with the row's other columns
//! poured into the clone's custom parameters. Everything downstream of that
//! is machinery that already exists — clone syncing keeps the copies
//! tracking the master, and custom parameters are already how a component
//! instance differs from its siblings.
//!
//! Replication runs alongside clone syncing, before the frame cooks — it
//! *edits the graph*, which a cook must never do. It reads the template's
//! most recent cooked contents, so a growing table (a UDP In, a script)
//! grows the replicants a frame later, which is also when a human can first
//! see the new row.

use otd_core::indexmap::IndexMap;
use otd_core::{Connector, Family, Graph, NodeId, OpDef, Param, Value};
use otd_dat::DatStore;

pub const REPLICATOR: &str = "replicatorCOMP";

fn params_replicator() -> IndexMap<String, Param> {
    let mut m = IndexMap::new();
    m.insert(
        "master".into(),
        Param::str("").with_label("Master (path of the component to copy)"),
    );
    m.insert(
        "template".into(),
        Param::str("").with_label("Template DAT (one replicant per row)"),
    );
    m.insert(
        "byname".into(),
        Param::bool(true).with_label("First Row/Column Are Names"),
    );
    m
}

pub const DEF: OpDef = OpDef {
    type_name: REPLICATOR,
    label: "Replicator",
    family: Family::Comp,
    inputs: &[],
    summary: "Keeps one clone of a master component per row of a table.",
    time_dependent: false,
    params: params_replicator,
    connector: Connector::None,
};

/// A row name has to be a node name. Anything that is not one becomes `_`,
/// so `Cue 12!` and `Cue 12?` collide — visibly, in the editor, rather than
/// silently making two nodes that print identically.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cleaned.trim_matches('_').to_string()
}

/// The template's rows: the cooked contents when the DAT has cooked, else a
/// Table DAT's stored text — so a replicator is right on the first frame of
/// the common case instead of one frame late.
fn template_rows(graph: &Graph, dats: &DatStore, id: NodeId) -> Vec<Vec<String>> {
    if let Some(data) = dats.get(id) {
        return data.rows.clone();
    }
    let node = graph.node(id);
    if node.op_type == "tableDAT" {
        let text = node
            .param("text")
            .map(|p| p.value.as_str())
            .unwrap_or_default();
        let delimiter = match node.param("delimiter").map(|p| p.value.as_str()) {
            Some(d) if d == "comma" => ',',
            _ => '\t',
        };
        return otd_dat::DatData::from_delimited(&text, delimiter).rows;
    }
    Vec::new()
}

/// What one replicant should be: its name and its parameter values.
struct Wanted {
    name: String,
    values: Vec<(String, Value)>,
}

fn wanted(rows: &[Vec<String>], by_name: bool, master_params: &[String]) -> Vec<Wanted> {
    let headers: Vec<String> = if by_name {
        rows.first().cloned().unwrap_or_default()
    } else {
        Vec::new()
    };
    let data = if by_name && !rows.is_empty() {
        &rows[1..]
    } else {
        rows
    };

    data.iter()
        .enumerate()
        .map(|(i, row)| {
            let name = row
                .first()
                .map(|c| sanitize(c))
                .filter(|n| by_name && !n.is_empty())
                .unwrap_or_else(|| format!("item{}", i + 1));
            // Columns whose header names a custom parameter on the master
            // become that parameter on the replicant. Numbers stay numbers.
            let values = headers
                .iter()
                .zip(row)
                .skip(1)
                .filter(|(h, _)| master_params.iter().any(|p| p == *h))
                .map(|(h, cell)| {
                    let value = match cell.trim().parse::<f64>() {
                        Ok(f) => Value::Float(f),
                        Err(_) => Value::Str(cell.clone()),
                    };
                    (h.clone(), value)
                })
                .collect();
            Wanted { name, values }
        })
        .collect()
}

/// Bring every Replicator's children in line with its template.
///
/// Returns how many nodes were created or removed, which the editor reports
/// so the behaviour is visible rather than mysterious.
pub fn sync(graph: &mut Graph, dats: &DatStore) -> usize {
    let replicators: Vec<NodeId> = graph
        .walk()
        .into_iter()
        .filter(|id| graph.node(*id).op_type == REPLICATOR)
        .collect();

    let mut changed = 0;
    for rep in replicators {
        let master_path = graph
            .node(rep)
            .param("master")
            .map(|p| p.value.as_str())
            .unwrap_or_default();
        let template_path = graph
            .node(rep)
            .param("template")
            .map(|p| p.value.as_str())
            .unwrap_or_default();
        let by_name = graph
            .node(rep)
            .param("byname")
            .map(|p| p.value.as_bool())
            .unwrap_or(true);

        let Some(master) = graph.find(master_path.trim()) else {
            continue;
        };
        let Some(template) = graph.find(template_path.trim()) else {
            continue;
        };
        if master == rep {
            // A replicator of itself would expand forever.
            continue;
        }

        let master_params: Vec<String> = graph
            .node(master)
            .params
            .iter()
            .filter(|(_, p)| p.custom)
            .map(|(k, _)| k.clone())
            .collect();
        let rows = template_rows(graph, dats, template);
        let want = wanted(&rows, by_name, &master_params);

        // Remove clones this replicator made whose row is gone. Anything a
        // person put inside by hand — not a clone of this master — is theirs
        // and stays.
        let stale: Vec<NodeId> = graph
            .node(rep)
            .children
            .iter()
            .copied()
            .filter(|c| {
                let node = graph.node(*c);
                node.clone_of.as_deref() == Some(master_path.trim())
                    && !want.iter().any(|w| w.name == node.name)
            })
            .collect();
        for id in stale {
            if graph.remove(id).is_ok() {
                changed += 1;
            }
        }

        for (i, w) in want.iter().enumerate() {
            let path = format!("{}/{}", graph.path(rep), w.name);
            let id = match graph.find(&path) {
                Some(id) => id,
                None => {
                    let Ok(id) = graph.create(rep, &otd_gpu::ops::CONTAINER, Some(&w.name)) else {
                        continue;
                    };
                    graph.set_clone(id, Some(master_path.trim()));
                    // Give each replicant its own place instead of a stack.
                    graph.node_mut_quiet(id).pos = [(i % 4) as f32 * 180.0, (i / 4) as f32 * 140.0];
                    changed += 1;
                    id
                }
            };
            // The clone machinery copies the master's custom parameters; the
            // row is authoritative for its own columns. Set only what
            // differs — an unchanged row must not dirty the subtree.
            for (key, value) in &w.values {
                let current = graph.node(id).param(key).map(|p| p.value.clone());
                match current {
                    Some(v) if v == *value => {}
                    Some(_) => {
                        let _ = graph.set_param(id, key, value.clone());
                    }
                    // Not there yet: the clone has not synced. Add it now so
                    // the row's value exists before the first cook; the sync
                    // keeps existing values, so nothing is lost later.
                    None => {
                        let param = graph
                            .node(master)
                            .param(key)
                            .cloned()
                            .unwrap_or_else(|| Param::float(0.0));
                        graph.add_custom_param(id, key, param);
                        let _ = graph.set_param(id, key, value.clone());
                    }
                }
            }
        }
    }
    changed
}
