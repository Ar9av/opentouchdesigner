//! The DAT operator table.

use std::sync::OnceLock;

use otd_core::indexmap::IndexMap;
use otd_core::{Connector, EvalContext, Family, Node, OpDef, OpRegistry, Param, Value};

use crate::{DatData, ScriptHost};

pub struct DatCtx<'a> {
    pub node: &'a Node,
    pub eval: &'a EvalContext<'a>,
    pub inputs: Vec<DatData>,
    pub scripts: Option<&'a dyn ScriptHost>,
    pub net: &'a mut crate::net::Net,
    pub path: &'a str,
    /// Set by an operator that could not do its job — a bad script, malformed
    /// JSON — and shown on the node.
    pub error: Option<String>,
}

impl DatCtx<'_> {
    fn val(&self, key: &str) -> Value {
        self.node
            .param(key)
            .map(|p| p.eval(self.eval))
            .unwrap_or(Value::Str(String::new()))
    }
    fn s(&self, key: &str) -> String {
        self.val(key).as_str()
    }
    fn b(&self, key: &str) -> bool {
        self.val(key).as_bool()
    }
    fn menu(&self, key: &str) -> usize {
        let Some(p) = self.node.param(key) else {
            return 0;
        };
        let chosen = p.eval(self.eval).as_str();
        p.menu
            .as_ref()
            .and_then(|m| m.iter().position(|i| *i == chosen))
            .unwrap_or(0)
    }
    fn input(&self, i: usize) -> DatData {
        self.inputs.get(i).cloned().unwrap_or_default()
    }
}

pub struct DatSpec {
    pub def: OpDef,
    pub cook: fn(&mut DatCtx) -> DatData,
}

macro_rules! params {
    ($($key:expr => $param:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m: IndexMap<String, Param> = IndexMap::new();
        $( m.insert($key.into(), $param); )*
        m
    }};
}

fn no_params() -> IndexMap<String, Param> {
    params! {}
}

// ----------------------------------------------------------------- text

fn params_text() -> IndexMap<String, Param> {
    params! {
        "text" => Param::str("").with_label("Text"),
    }
}

fn cook_text(c: &mut DatCtx) -> DatData {
    DatData::text(c.s("text"))
}

// ---------------------------------------------------------------- table

fn params_table() -> IndexMap<String, Param> {
    params! {
        // The table's contents live in the project file, so a cue list is
        // versioned with the patch that uses it.
        "text" => Param::str("name\tvalue\n").with_label("Contents"),
        "delimiter" => Param::menu("tab", &["tab", "comma"]).with_label("Delimiter"),
    }
}

fn delimiter_of(c: &DatCtx) -> char {
    if c.menu("delimiter") == 1 { ',' } else { '\t' }
}

fn cook_table(c: &mut DatCtx) -> DatData {
    DatData::from_delimited(&c.s("text"), delimiter_of(c))
}

// --------------------------------------------------------------- select

fn params_select() -> IndexMap<String, Param> {
    params! {
        "rows" => Param::str("*").with_label("Rows (names, indices or *)"),
        "cols" => Param::str("*").with_label("Columns"),
        "byname" => Param::bool(true).with_label("First Row/Column Are Names"),
    }
}

/// Resolve a selection string against headings and a count.
fn selection(spec: &str, headings: &[String], count: usize) -> Vec<usize> {
    let spec = spec.trim();
    if spec.is_empty() || spec == "*" {
        return (0..count).collect();
    }
    let mut out = Vec::new();
    for token in spec.split_whitespace() {
        if let Ok(i) = token.parse::<usize>() {
            if i < count && !out.contains(&i) {
                out.push(i);
            }
            continue;
        }
        for (i, h) in headings.iter().enumerate() {
            if crate::ops::glob(token, h) && !out.contains(&i) {
                out.push(i);
            }
        }
    }
    out
}

pub fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => {
            name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail)
        }
    }
}

fn cook_select(c: &mut DatCtx) -> DatData {
    let input = c.input(0);
    let by_name = c.b("byname");

    let col_headings: Vec<String> = if by_name {
        input.rows.first().cloned().unwrap_or_default()
    } else {
        Vec::new()
    };
    let row_headings: Vec<String> = if by_name {
        input
            .rows
            .iter()
            .map(|r| r.first().cloned().unwrap_or_default())
            .collect()
    } else {
        Vec::new()
    };

    let rows = selection(&c.s("rows"), &row_headings, input.num_rows());
    let cols = selection(&c.s("cols"), &col_headings, input.num_cols());

    let picked = rows
        .iter()
        .map(|r| {
            cols.iter()
                .map(|col| input.cell(*r, *col).to_string())
                .collect()
        })
        .collect();
    DatData::table(picked)
}

// ---------------------------------------------------------------- merge

fn params_merge() -> IndexMap<String, Param> {
    params! {
        "how" => Param::menu("rows", &["rows", "columns"]).with_label("Merge By"),
    }
}

fn cook_merge(c: &mut DatCtx) -> DatData {
    let (a, b) = (c.input(0), c.input(1));
    if c.menu("how") == 1 {
        // Side by side, padding the shorter one.
        let n = a.num_rows().max(b.num_rows());
        let rows = (0..n)
            .map(|r| {
                let mut row: Vec<String> = a.rows.get(r).cloned().unwrap_or_default();
                row.resize(a.num_cols(), String::new());
                row.extend(b.rows.get(r).cloned().unwrap_or_default());
                row
            })
            .collect();
        DatData::table(rows)
    } else {
        let mut rows = a.rows.clone();
        rows.extend(b.rows.iter().cloned());
        DatData::table(rows)
    }
}

// ----------------------------------------------------------------- JSON

fn params_json() -> IndexMap<String, Param> {
    params! {
        "pointer" => Param::str("").with_label("JSON Pointer (e.g. /items/0)"),
    }
}

/// Flatten JSON into `path`/`value` rows.
///
/// A table is what the rest of the network can use; keeping the flattened
/// path as the key means a Select DAT can pull one field out by name.
fn flatten(prefix: &str, v: &serde_json::Value, out: &mut Vec<Vec<String>>) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                flatten(&format!("{prefix}/{k}"), v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                flatten(&format!("{prefix}/{i}"), v, out);
            }
        }
        serde_json::Value::Null => out.push(vec![prefix.to_string(), String::new()]),
        serde_json::Value::Bool(b) => out.push(vec![prefix.to_string(), b.to_string()]),
        serde_json::Value::Number(n) => out.push(vec![prefix.to_string(), n.to_string()]),
        serde_json::Value::String(s) => out.push(vec![prefix.to_string(), s.clone()]),
    }
}

fn cook_json(c: &mut DatCtx) -> DatData {
    let text = c.input(0).as_text();
    if text.trim().is_empty() {
        return DatData::table(vec![vec!["path".into(), "value".into()]]);
    }
    let parsed: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            c.error = Some(format!("JSON: {e}"));
            return DatData::table(vec![vec!["path".into(), "value".into()]]);
        }
    };
    let pointer = c.s("pointer");
    let root = if pointer.trim().is_empty() {
        &parsed
    } else {
        match parsed.pointer(pointer.trim()) {
            Some(v) => v,
            None => {
                c.error = Some(format!("no JSON at `{}`", pointer.trim()));
                return DatData::table(vec![vec!["path".into(), "value".into()]]);
            }
        }
    };
    let mut rows = vec![vec!["path".to_string(), "value".to_string()]];
    flatten("", root, &mut rows);
    DatData::table(rows)
}

// --------------------------------------------------------------- script

fn params_script() -> IndexMap<String, Param> {
    params! {
        "source" => Param::str(
            "# Leave a list of lists in `rows`.\nrows = [['frame'], [frame]]\n",
        ).with_label("Source").as_script(),
    }
}

fn cook_script(c: &mut DatCtx) -> DatData {
    let source = c.s("source");
    let Some(host) = c.scripts else {
        c.error = Some("no Python interpreter in this build".into());
        return DatData::default();
    };
    match host.run_table(&source, c.eval, c.path) {
        Ok(rows) => DatData::table(rows),
        Err(e) => {
            c.error = Some(e);
            DatData::default()
        }
    }
}

// ------------------------------------------------------------------- UDP

fn params_udp_in() -> IndexMap<String, Param> {
    params! {
        "port" => Param::int(7000).with_label("Port").with_range(1.0, 65535.0),
        "keep" => Param::int(20).with_label("Rows Kept").with_range(1.0, 1000.0),
    }
}

/// Received datagrams as a table, one message per row, newest last —
/// the shape a Select DAT or a script naturally reads a log in.
fn cook_udp_in(c: &mut DatCtx) -> DatData {
    let port = c.val("port").as_i64().clamp(1, 65535) as u16;
    let keep = c.val("keep").as_i64().clamp(1, crate::net::KEEP_CAP as i64) as usize;
    let path = c.path.to_string();

    let needs_open = c
        .net
        .udp_in
        .get(&path)
        .map(|u| u.port != port)
        .unwrap_or(true);
    if needs_open {
        // Drop the old listener before binding the new port.
        c.net.udp_in.remove(&path);
        match crate::net::UdpIn::open(port) {
            Ok(u) => {
                c.net.udp_in.insert(path.clone(), u);
            }
            Err(e) => {
                c.error = Some(e);
                return DatData::table(vec![vec!["message".into()]]);
            }
        }
    }

    let mut rows = vec![vec!["message".to_string()]];
    if let Some(listener) = c.net.udp_in.get(&path) {
        if let Ok(messages) = listener.messages.lock() {
            let start = messages.len().saturating_sub(keep);
            rows.extend(messages[start..].iter().map(|m| vec![m.clone()]));
        }
    }
    DatData::table(rows)
}

fn params_udp_out() -> IndexMap<String, Param> {
    params! {
        "address" => Param::str("127.0.0.1").with_label("Address"),
        "port" => Param::int(7001).with_label("Port").with_range(1.0, 65535.0),
        "active" => Param::bool(true).with_label("Active"),
    }
}

/// Sends its input's text as one datagram — when it *changes*. A DAT cooks
/// whenever anything upstream does; resending an unchanged payload every
/// cook would turn a cook into a broadcast.
fn cook_udp_out(c: &mut DatCtx) -> DatData {
    let input = c.input(0);
    let host = c.s("address");
    let port = c.val("port").as_i64().clamp(1, 65535) as u16;
    let active = c.b("active");
    let path = c.path.to_string();

    if !active {
        return input;
    }
    let Ok(target) = format!("{}:{}", host.trim(), port).parse::<std::net::SocketAddr>() else {
        c.error = Some(format!("bad UDP address `{host}:{port}`"));
        return input;
    };

    let needs_open = c
        .net
        .udp_out
        .get(&path)
        .map(|u| u.target != target)
        .unwrap_or(true);
    if needs_open {
        match crate::net::UdpOut::open(target) {
            Ok(u) => {
                c.net.udp_out.insert(path.clone(), u);
            }
            Err(e) => {
                c.error = Some(e);
                return input;
            }
        }
    }

    if let Some(out) = c.net.udp_out.get_mut(&path) {
        let text = input.as_text();
        if out.sent.as_deref() != Some(text.as_str()) && !text.is_empty() {
            if let Err(e) = out.socket.send_to(text.as_bytes(), out.target) {
                c.error = Some(format!("UDP send: {e}"));
            } else {
                out.sent = Some(text);
            }
        }
    }
    input
}

// ------------------------------------------------------------ pass-through

fn cook_null(c: &mut DatCtx) -> DatData {
    c.input(0)
}

// ------------------------------------------------------------- the table

pub const NULL: &str = "nullDAT";
pub const TABLE: &str = "tableDAT";
pub const TEXT: &str = "textDAT";
pub const SCRIPT: &str = "scriptDAT";
pub const IN: &str = "inDAT";
pub const OUT: &str = "outDAT";

fn spec(
    type_name: &'static str,
    label: &'static str,
    inputs: &'static [&'static str],
    summary: &'static str,
    params: fn() -> IndexMap<String, Param>,
    cook: fn(&mut DatCtx) -> DatData,
) -> DatSpec {
    DatSpec {
        def: OpDef {
            type_name,
            label,
            family: Family::Dat,
            inputs,
            summary,
            time_dependent: false,
            params,
            connector: Connector::None,
        },
        cook,
    }
}

fn specs() -> &'static Vec<DatSpec> {
    static SPECS: OnceLock<Vec<DatSpec>> = OnceLock::new();
    SPECS.get_or_init(|| {
        let mut v = vec![
            spec(
                TABLE,
                "Table",
                &[],
                "A table of text, stored in the project file.",
                params_table,
                cook_table,
            ),
            spec(
                TEXT,
                "Text",
                &[],
                "A block of text.",
                params_text,
                cook_text,
            ),
            spec(
                "selectDAT",
                "Select",
                &["in"],
                "Pick rows and columns, by name or index.",
                params_select,
                cook_select,
            ),
            spec(
                "mergeDAT",
                "Merge",
                &["a", "b"],
                "Join two DATs by rows or by columns.",
                params_merge,
                cook_merge,
            ),
            spec(
                "jsonDAT",
                "JSON",
                &["in"],
                "Parse JSON text into a path/value table.",
                params_json,
                cook_json,
            ),
            spec(
                NULL,
                "Null",
                &["in"],
                "Pass-through. A stable name to reference.",
                no_params,
                cook_null,
            ),
        ];
        // UDP In re-cooks every frame: messages arrive whether or not
        // anything in the graph changed.
        let mut udp_in = spec(
            "udpinDAT",
            "UDP In",
            &[],
            "Datagrams received on a port, one message per row.",
            params_udp_in,
            cook_udp_in,
        );
        udp_in.def.time_dependent = true;
        v.push(udp_in);
        v.push(spec(
            "udpoutDAT",
            "UDP Out",
            &["in"],
            "Sends its input's text as a datagram when it changes.",
            params_udp_out,
            cook_udp_out,
        ));

        // A Script DAT re-runs every frame: its source may read time, and
        // there is no way to know it doesn't.
        let mut script = spec(
            SCRIPT,
            "Script",
            &["in"],
            "Rows produced by a Python script.",
            params_script,
            cook_script,
        );
        script.def.time_dependent = true;
        v.push(script);

        let mut in_dat = spec(
            IN,
            "In",
            &[],
            "A data input on this component's node.",
            no_params,
            cook_null,
        );
        in_dat.def.connector = Connector::In;
        v.push(in_dat);

        let mut out_dat = spec(
            OUT,
            "Out",
            &["in"],
            "This component's data output.",
            no_params,
            cook_null,
        );
        out_dat.def.connector = Connector::Out;
        v.push(out_dat);
        v
    })
}

pub fn spec_for(type_name: &str) -> Option<&'static DatSpec> {
    specs().iter().find(|s| s.def.type_name == type_name)
}

pub fn all() -> impl Iterator<Item = &'static DatSpec> {
    specs().iter()
}

pub fn registry() -> OpRegistry {
    let mut r = OpRegistry::new();
    for s in specs() {
        r.register(s.def.clone());
    }
    r
}
