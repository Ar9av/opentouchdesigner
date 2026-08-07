//! `otd-dat` — the DAT (data operator) family: text and tables.
//!
//! PLAN.md §2.1 lists DATs alongside TOPs and CHOPs as one of the load-bearing
//! wire families. They are the family that carries *structure* — a table of
//! cue points, a JSON payload from a control surface, a script.
//!
//! Two decisions worth stating. Content lives in a parameter rather than in a
//! side file, so a table is part of the project's text and shows up in a diff
//! like everything else. And a table is a grid of strings, not of typed
//! values: a DAT is text, and the operator reading it decides what the text
//! means.

pub mod net;
pub mod ops;

use otd_core::{ChannelSource, CookContext, CookError, Cooker, EvalContext, Family, Graph, NodeId};
use slotmap::SecondaryMap;

/// A DAT's contents: rows of cells.
///
/// A "text" DAT is simply one cell; keeping one representation means Select,
/// Merge and the viewers do not need two code paths.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DatData {
    pub rows: Vec<Vec<String>>,
    /// Presented as free text rather than as a grid.
    pub is_text: bool,
}

impl DatData {
    pub fn text(s: impl Into<String>) -> Self {
        DatData {
            rows: vec![vec![s.into()]],
            is_text: true,
        }
    }

    pub fn table(rows: Vec<Vec<String>>) -> Self {
        DatData {
            rows,
            is_text: false,
        }
    }

    pub fn num_rows(&self) -> usize {
        self.rows.len()
    }

    pub fn num_cols(&self) -> usize {
        self.rows.iter().map(|r| r.len()).max().unwrap_or(0)
    }

    pub fn cell(&self, row: usize, col: usize) -> &str {
        self.rows
            .get(row)
            .and_then(|r| r.get(col))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// The whole thing as text — a text DAT's single cell, or the table as
    /// tab-separated rows.
    pub fn as_text(&self) -> String {
        if self.is_text {
            return self.cell(0, 0).to_string();
        }
        self.rows
            .iter()
            .map(|r| r.join("\t"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Parse tab- or comma-separated text into a table.
    pub fn from_delimited(text: &str, delimiter: char) -> Self {
        let rows: Vec<Vec<String>> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split(delimiter).map(|c| c.trim().to_string()).collect())
            .collect();
        DatData::table(rows)
    }

    /// Look a value up by its row heading, the way a cue table is used.
    pub fn lookup(&self, key: &str, column: usize) -> Option<&str> {
        self.rows
            .iter()
            .find(|r| r.first().map(|c| c == key).unwrap_or(false))
            .and_then(|r| r.get(column))
            .map(|s| s.as_str())
    }
}

/// Every DAT's most recent output.
#[derive(Default)]
pub struct DatStore {
    data: SecondaryMap<NodeId, DatData>,
}

impl DatStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&self, id: NodeId) -> Option<&DatData> {
        self.data.get(id)
    }
    pub fn insert(&mut self, id: NodeId, data: DatData) {
        self.data.insert(id, data);
    }
    pub fn remove(&mut self, id: NodeId) {
        self.data.remove(id);
    }
    pub fn clear(&mut self) {
        self.data.clear();
    }
}

/// What a Script DAT needs from the host to run its source.
pub trait ScriptHost {
    /// Run a script and collect the `rows` it leaves behind.
    fn run_table(
        &self,
        source: &str,
        ctx: &EvalContext,
        path: &str,
    ) -> Result<Vec<Vec<String>>, String>;
}

#[derive(Default)]
pub struct DatEngine {
    /// Errors from Script DATs, shown on the node.
    pub status: std::collections::HashMap<String, String>,
    /// Open sockets for the network DATs, keyed by node path.
    pub net: net::Net,
}

impl DatEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn status_of(&self, path: &str) -> Option<&str> {
        self.status.get(path).map(|s| s.as_str())
    }

    pub fn reset(&mut self) {
        self.status.clear();
        self.net.reset();
    }

    /// Cook one DAT and return its contents.
    pub fn cook_node(
        &mut self,
        graph: &Graph,
        id: NodeId,
        _ctx: &CookContext,
        eval: &EvalContext,
        store: &DatStore,
        scripts: Option<&dyn ScriptHost>,
    ) -> Result<DatData, CookError> {
        let node = graph.get(id).ok_or(CookError::NoSuchNode)?;
        let path = graph.path(id);

        if node.connector == otd_core::Connector::In {
            return Ok(graph
                .connector_source(id)
                .and_then(|src| store.get(src))
                .cloned()
                .unwrap_or_default());
        }

        let spec = ops::spec_for(&node.op_type)
            .ok_or_else(|| CookError::op(&path, format!("unknown DAT `{}`", node.op_type)))?;

        let inputs: Vec<DatData> = node
            .inputs
            .iter()
            .map(|slot| {
                slot.and_then(|src| store.get(src))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();

        let eval = &EvalContext {
            path: Some(&path),
            ..*eval
        };
        let mut cx = ops::DatCtx {
            node,
            eval,
            inputs,
            scripts,
            net: &mut self.net,
            path: &path,
            error: None,
        };
        let out = (spec.cook)(&mut cx);
        match cx.error {
            Some(e) => {
                self.status.insert(path, e);
            }
            None => {
                self.status.remove(&path);
            }
        }
        Ok(out)
    }
}

/// A DAT-only host, for tests and for a control-only runtime.
#[derive(Default)]
pub struct DatHost {
    pub engine: DatEngine,
    pub store: DatStore,
}

impl DatHost {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn data(&self, id: NodeId) -> Option<&DatData> {
        self.store.get(id)
    }
}

impl Cooker for DatHost {
    fn cook(&mut self, graph: &Graph, id: NodeId, ctx: &CookContext) -> Result<(), CookError> {
        if graph.get(id).map(|n| n.family) != Some(Family::Dat) {
            return Ok(());
        }
        let DatHost { engine, store } = self;
        let eval = ctx.eval_ctx();
        let data = engine.cook_node(graph, id, ctx, &eval, store, None)?;
        store.insert(id, data);
        Ok(())
    }
}

/// A read-only view of DAT contents, so a parameter can read a table cell.
pub struct DatNetwork<'a> {
    pub graph: &'a Graph,
    pub dats: &'a DatStore,
}

impl ChannelSource for DatNetwork<'_> {
    fn channel(&self, _op_path: &str, _channel: &str) -> Option<f32> {
        None
    }
    fn param_value(&self, _op_path: &str, _param: &str) -> Option<otd_core::Value> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_round_trips_through_delimited_text() {
        let d = DatData::from_delimited("name\tvalue\na\t1\nb\t2", '\t');
        assert_eq!(d.num_rows(), 3);
        assert_eq!(d.num_cols(), 2);
        assert_eq!(d.cell(1, 0), "a");
        assert_eq!(d.lookup("b", 1), Some("2"));
        assert_eq!(d.as_text(), "name\tvalue\na\t1\nb\t2");
    }

    #[test]
    fn ragged_rows_read_as_empty_rather_than_panicking() {
        let d = DatData::table(vec![vec!["a".into()], vec!["b".into(), "c".into()]]);
        assert_eq!(d.num_cols(), 2);
        assert_eq!(d.cell(0, 1), "");
        assert_eq!(d.cell(9, 9), "");
    }

    #[test]
    fn a_text_dat_is_one_cell() {
        let d = DatData::text("hello\nworld");
        assert!(d.is_text);
        assert_eq!(d.num_rows(), 1);
        assert_eq!(d.as_text(), "hello\nworld");
    }
}
