//! The DAT operator table.

use std::sync::OnceLock;

use otd_core::indexmap::IndexMap;
use otd_core::{
    Connector, Crossing, Crossings, EvalContext, Family, Node, OpDef, OpRegistry, Param, Value,
};

use crate::{DatData, ScriptHost};

pub struct DatCtx<'a> {
    pub node: &'a Node,
    pub eval: &'a EvalContext<'a>,
    pub inputs: Vec<DatData>,
    /// Inputs that came from another family, for the converter operators.
    pub foreign: Crossings,
    pub scripts: Option<&'a dyn ScriptHost>,
    pub net: &'a mut crate::net::Net,
    /// What this node saw last frame — an Execute DAT's whole memory.
    pub watched: &'a mut Watched,
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
    fn foreign(&self, i: usize) -> Option<&Crossing> {
        self.foreign.get(i).and_then(|c| c.as_ref())
    }
    fn input(&self, i: usize) -> DatData {
        self.inputs.get(i).cloned().unwrap_or_default()
    }
}

/// The previous frame's values, per watched name.
///
/// A callback fires on a *change*, so something has to remember what the
/// value was. Kept as plain data on the engine rather than inside the script,
/// because a script that reloads must not silently re-fire everything.
#[derive(Clone, Debug, Default)]
pub struct Watched {
    pub values: IndexMap<String, f64>,
    /// Set once `onStart` has run, so it runs once per node and not per frame.
    pub started: bool,
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
/// Fires Python callbacks every frame. Always a cook root — see
/// `otd_engine::execute`.
pub const EXECUTE: &str = "executeDAT";

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
            input_families: &[],
            summary,
            time_dependent: false,
            params,
            connector: Connector::None,
        },
        cook,
    }
}

// -------------------------------------------------------------- execute

/// The default body of an Execute DAT: the events, with their signatures, so
/// the operator documents itself the moment it is dropped.
const EXECUTE_SOURCE: &str = "\
# Runs once, the first time this node cooks.
def onStart():
    pass

# Runs every frame, before the rest of the network cooks.
def onFrameStart(frame):
    pass

# Runs every frame, after it has.
def onFrameEnd(frame):
    pass
";

const CHOP_EXECUTE_SOURCE: &str = "\
# A watched channel changed value.
def onValueChange(channel, value, prev):
    pass

# It crossed the threshold upwards, or downwards.
def onOffToOn(channel, value):
    pass

def onOnToOff(channel, value):
    pass
";

const PAR_EXECUTE_SOURCE: &str = "\
# A watched parameter changed. `value` and `prev` are numbers.
def onValueChange(par, value, prev):
    pass
";

fn params_execute() -> IndexMap<String, Param> {
    params! {
        "active" => Param::bool(true).with_label("Active"),
        "source" => Param::str(EXECUTE_SOURCE).with_label("Callbacks").into_script(),
    }
}

fn params_chop_execute() -> IndexMap<String, Param> {
    params! {
        "active" => Param::bool(true).with_label("Active"),
        "chop" => Param::str("").with_label("Watch CHOP").as_path_ref(),
        "channels" => Param::str("*").with_label("Channels"),
        "threshold" => Param::float(0.5).with_label("On Threshold").with_range(-10.0, 10.0),
        "source" => Param::str(CHOP_EXECUTE_SOURCE).with_label("Callbacks").into_script(),
    }
}

fn params_par_execute() -> IndexMap<String, Param> {
    params! {
        "active" => Param::bool(true).with_label("Active"),
        "op" => Param::str("").with_label("Watch Operator").as_path_ref(),
        "parameters" => Param::str("*").with_label("Parameters"),
        "source" => Param::str(PAR_EXECUTE_SOURCE).with_label("Callbacks").into_script(),
    }
}

/// Fire one callback, reporting a script error on the node.
///
/// A failing callback disables nothing and stops nothing: the same rule the
/// rest of the engine follows for a bad expression or a missing device, and
/// for the same reason — a typo during a show must not take the render with
/// it.
fn fire(c: &mut DatCtx, source: &str, func: &str, args: &[Value]) {
    let Some(scripts) = c.scripts else { return };
    if let Err(e) = scripts.call(source, func, args, c.eval, c.path) {
        // Keep the first error of the frame: the later ones are usually the
        // same mistake seen again.
        if c.error.is_none() {
            c.error = Some(e);
        }
    }
}

/// An Execute DAT presents its own source, like a Text DAT. The output is not
/// the point — the callbacks are — but having *something* means it can be
/// viewed, wired into a Text DAT, and diffed like everything else.
fn cook_execute(c: &mut DatCtx) -> DatData {
    let source = c.s("source");
    if !c.b("active") {
        return DatData::text(source);
    }
    if !c.watched.started {
        c.watched.started = true;
        fire(c, &source, "onStart", &[]);
    }
    let frame = Value::Int(c.eval.frame);
    fire(c, &source, "onFrameStart", std::slice::from_ref(&frame));
    // Both edges of the frame fire here, one cook apart in the same cook.
    // A genuine end-of-frame hook would have to run after every other node,
    // and the cook is demand-driven — there is no such moment to hang it on
    // that is not simply "later", so saying so beats pretending.
    fire(c, &source, "onFrameEnd", std::slice::from_ref(&frame));
    DatData::text(source)
}

/// Which of `names` a whitespace-separated pattern selects.
fn selected<'a>(pattern: &str, names: &'a [String]) -> Vec<&'a String> {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern == "*" {
        return names.iter().collect();
    }
    let wanted: Vec<&str> = pattern.split_whitespace().collect();
    names
        .iter()
        .filter(|n| {
            wanted.iter().any(|w| {
                w.strip_suffix('*')
                    .map(|prefix| n.starts_with(prefix))
                    .unwrap_or(*w == n.as_str())
            })
        })
        .collect()
}

fn cook_chop_execute(c: &mut DatCtx) -> DatData {
    let source = c.s("source");
    if !c.b("active") {
        return DatData::text(source);
    }
    let path = c.s("chop");
    let Some(net) = c.eval.channels else {
        return DatData::text(source);
    };
    let names = net.channel_names(path.trim());
    let threshold = c
        .node
        .param("threshold")
        .map(|p| p.eval(c.eval).as_f32())
        .unwrap_or(0.5);
    let pattern = c.s("channels");

    // Collected first, because firing a callback borrows `c` mutably and the
    // network borrow has to be finished by then.
    let mut events: Vec<(String, f64, Option<f64>)> = Vec::new();
    for name in selected(&pattern, &names) {
        let Some(value) = net.channel(path.trim(), name) else {
            continue;
        };
        let value = value as f64;
        let prev = c.watched.values.get(name).copied();
        if prev != Some(value) {
            events.push((name.clone(), value, prev));
        }
    }
    for (name, value, _) in &events {
        c.watched.values.insert(name.clone(), *value);
    }

    for (name, value, prev) in events {
        let args = [
            Value::Str(name.clone()),
            Value::Float(value),
            Value::Float(prev.unwrap_or(value)),
        ];
        fire(c, &source, "onValueChange", &args);
        // The edge callbacks are the ones worth having: "the beat landed" is
        // a different question from "the number moved".
        let Some(prev) = prev else { continue };
        let t = threshold as f64;
        if prev < t && value >= t {
            fire(
                c,
                &source,
                "onOffToOn",
                &[Value::Str(name.clone()), Value::Float(value)],
            );
        } else if prev >= t && value < t {
            fire(
                c,
                &source,
                "onOnToOff",
                &[Value::Str(name), Value::Float(value)],
            );
        }
    }
    DatData::text(source)
}

fn cook_par_execute(c: &mut DatCtx) -> DatData {
    let source = c.s("source");
    if !c.b("active") {
        return DatData::text(source);
    }
    let path = c.s("op");
    let Some(net) = c.eval.channels else {
        return DatData::text(source);
    };
    let pattern = c.s("parameters");
    // Only the parameters named explicitly can be watched: there is no way to
    // enumerate another operator's parameters through the narrow view a script
    // gets, and widening that view to serve one operator would hand every
    // expression the whole graph.
    let names: Vec<String> = pattern
        .split_whitespace()
        .filter(|n| *n != "*")
        .map(|s| s.to_string())
        .collect();
    if names.is_empty() {
        c.error = Some("name the parameters to watch — `*` cannot be enumerated".into());
        return DatData::text(source);
    }

    let mut events = Vec::new();
    for name in &names {
        let Some(value) = net.param_value(path.trim(), name) else {
            continue;
        };
        let value = value.as_f64();
        let prev = c.watched.values.get(name).copied();
        if prev != Some(value) {
            events.push((name.clone(), value, prev));
        }
    }
    for (name, value, _) in &events {
        c.watched.values.insert(name.clone(), *value);
    }
    for (name, value, prev) in events {
        fire(
            c,
            &source,
            "onValueChange",
            &[
                Value::Str(name),
                Value::Float(value),
                Value::Float(prev.unwrap_or(value)),
            ],
        );
    }
    DatData::text(source)
}

// ------------------------------------------------------------------ sort

fn params_sort() -> IndexMap<String, Param> {
    params! {
        "column" => Param::str("0").with_label("Column (name or index)"),
        "order" => Param::menu("ascending", &["ascending", "descending"]).with_label("Order"),
        "numeric" => Param::bool(true).with_label("Compare As Numbers"),
        "header" => Param::bool(true).with_label("First Row Is A Header"),
    }
}

fn column_index(rows: &[Vec<String>], spec: &str, header: bool) -> usize {
    if let Ok(i) = spec.trim().parse::<usize>() {
        return i;
    }
    if header {
        if let Some(head) = rows.first() {
            if let Some(i) = head.iter().position(|c| c == spec.trim()) {
                return i;
            }
        }
    }
    0
}

fn cook_sort(c: &mut DatCtx) -> DatData {
    let input = c.input(0);
    let header = c.b("header");
    let col = column_index(&input.rows, &c.s("column"), header);
    let numeric = c.b("numeric");
    let descending = c.menu("order") == 1;

    let mut rows = input.rows.clone();
    let head = if header && !rows.is_empty() {
        Some(rows.remove(0))
    } else {
        None
    };
    rows.sort_by(|a, b| {
        let (x, y) = (
            a.get(col).map(|s| s.as_str()).unwrap_or(""),
            b.get(col).map(|s| s.as_str()).unwrap_or(""),
        );
        let ord = if numeric {
            // A cell that is not a number sorts as if it were zero rather
            // than falling back to string order for that one row, which
            // would interleave the two orderings unpredictably.
            let (x, y) = (
                x.trim().parse::<f64>().unwrap_or(0.0),
                y.trim().parse::<f64>().unwrap_or(0.0),
            );
            x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            x.cmp(y)
        };
        if descending { ord.reverse() } else { ord }
    });
    if let Some(head) = head {
        rows.insert(0, head);
    }
    DatData::table(rows)
}

// ------------------------------------------------------------- transpose

fn cook_transpose(c: &mut DatCtx) -> DatData {
    let input = c.input(0);
    let cols = input.num_cols();
    let rows = (0..cols)
        .map(|col| {
            input
                .rows
                .iter()
                .map(|r| r.get(col).cloned().unwrap_or_default())
                .collect()
        })
        .collect();
    DatData::table(rows)
}

// ------------------------------------------------------------ substitute

fn params_substitute() -> IndexMap<String, Param> {
    params! {
        "template" => Param::str("").with_label("Template"),
        "table" => Param::bool(false).with_label("Substitute Every Cell Instead"),
    }
}

/// Fill `$name` placeholders from a two-column lookup table.
///
/// This is the operator that turns a cue table into a string somebody else
/// can read — an OSC address, a UDP payload, a line of a show report — without
/// a script. The input's first column names, the second supplies.
fn cook_substitute(c: &mut DatCtx) -> DatData {
    let input = c.input(0);
    let substitute = |text: &str| -> String {
        let mut out = text.to_string();
        for row in &input.rows {
            let (Some(key), Some(value)) = (row.first(), row.get(1)) else {
                continue;
            };
            if key.trim().is_empty() {
                continue;
            }
            out = out.replace(&format!("${key}"), value);
        }
        out
    };

    if !c.b("table") {
        return DatData::text(substitute(&c.s("template")));
    }
    DatData::table(
        input
            .rows
            .iter()
            .map(|r| r.iter().map(|cell| substitute(cell)).collect())
            .collect(),
    )
}

// --------------------------------------------------------------- convert

fn params_convert() -> IndexMap<String, Param> {
    params! {
        "to" => Param::menu("table", &["table", "text"]).with_label("Convert To"),
        "delimiter" => Param::menu("tab", &["tab", "comma"]).with_label("Delimiter"),
    }
}

/// Between the two shapes a DAT can have.
///
/// They are one representation internally — a text DAT is a table with one
/// cell — but the operators reading them are not interchangeable, so the
/// conversion has to be something you can put in a patch rather than a flag
/// hidden on every node.
fn cook_convert(c: &mut DatCtx) -> DatData {
    let input = c.input(0);
    let delim = if c.menu("delimiter") == 1 { ',' } else { '\t' };
    if c.menu("to") == 1 {
        let text = input
            .rows
            .iter()
            .map(|r| r.join(&delim.to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        return DatData::text(text);
    }
    DatData::from_delimited(&input.as_text(), delim)
}

// ------------------------------------------------------- CHOP to DAT

fn params_chop_to() -> IndexMap<String, Param> {
    params! {
        "layout" => Param::menu("columns", &["columns", "rows"])
            .with_label("Channels As"),
        "names" => Param::bool(true).with_label("Include Names"),
        "format" => Param::str("%.6g").with_label("Number Format"),
    }
}

/// Channels as a table — the readable end of a CHOP.
///
/// Mostly this is how you *look* at a CHOP: wire one in and the numbers are
/// text you can select and copy. It is also how a channel leaves the patch,
/// since a UDP Out DAT sends whatever text reaches it, so "send my six
/// smoothed values to the lighting desk" is two nodes rather than a feature.
fn cook_chop_to_dat(c: &mut DatCtx) -> DatData {
    let Some((names, samples, _)) = c.foreign(0).and_then(|f| f.as_channels()) else {
        return DatData::table(Vec::new());
    };
    let fmt = c.s("format");
    let show_names = c.b("names");
    let num = |v: f32| format_number(&fmt, v);

    let length = samples.iter().map(|s| s.len()).max().unwrap_or(0);
    let mut rows: Vec<Vec<String>> = Vec::new();

    if c.menu("layout") == 0 {
        // A column per channel, a row per sample — the spreadsheet shape.
        if show_names {
            rows.push(names.to_vec());
        }
        for i in 0..length {
            rows.push(
                samples
                    .iter()
                    .map(|s| s.get(i).copied().map(num).unwrap_or_default())
                    .collect(),
            );
        }
    } else {
        for (name, s) in names.iter().zip(samples) {
            let mut row = Vec::with_capacity(s.len() + 1);
            if show_names {
                row.push(name.clone());
            }
            row.extend(s.iter().copied().map(num));
            rows.push(row);
        }
    }
    DatData::table(rows)
}

/// `%.6g`-style formatting without pulling in a formatting crate: the only
/// thing that actually varies is how many digits to keep.
fn format_number(spec: &str, v: f32) -> String {
    let digits = spec
        .trim_start_matches(['%', '.'])
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<usize>()
        .unwrap_or(6)
        .min(17);
    if spec.ends_with('f') {
        format!("{v:.digits$}")
    } else {
        // Trailing zeros are noise in a table you are reading.
        let s = format!("{:.*}", digits, v);
        let s = if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        };
        if s == "-0" { "0".into() } else { s }
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

        for (type_name, label, summary, params, cook) in [
            (
                EXECUTE,
                "Execute",
                "Python callbacks at the start and end of a frame.",
                params_execute as fn() -> IndexMap<String, Param>,
                cook_execute as fn(&mut DatCtx) -> DatData,
            ),
            (
                "chopexecuteDAT",
                "CHOP Execute",
                "Python callbacks when a watched channel changes or crosses a threshold.",
                params_chop_execute,
                cook_chop_execute,
            ),
            (
                "parameterexecuteDAT",
                "Parameter Execute",
                "Python callbacks when a watched parameter changes.",
                params_par_execute,
                cook_par_execute,
            ),
        ] {
            let mut ex = spec(type_name, label, &[], summary, params, cook);
            // A callback that only ran when something downstream wanted it
            // would not be a callback. These cook every frame, and the hosts
            // treat them as roots.
            ex.def.time_dependent = true;
            v.push(ex);
        }

        v.push(spec(
            "sortDAT",
            "Sort",
            &["in"],
            "Sort rows by a column, numerically or as text.",
            params_sort,
            cook_sort,
        ));
        v.push(spec(
            "transposeDAT",
            "Transpose",
            &["in"],
            "Swap rows and columns.",
            no_params,
            cook_transpose,
        ));
        v.push(spec(
            "substituteDAT",
            "Substitute",
            &["in"],
            "Fill $name placeholders from a two-column lookup table.",
            params_substitute,
            cook_substitute,
        ));
        v.push(spec(
            "convertDAT",
            "Convert",
            &["in"],
            "Between a table and a block of text.",
            params_convert,
            cook_convert,
        ));

        let mut to_dat = spec(
            "choptodatDAT",
            "CHOP to DAT",
            &["in"],
            "Channels as a table of numbers.",
            params_chop_to,
            cook_chop_to_dat,
        );
        to_dat.def.input_families = &[Family::Chop];
        v.push(to_dat);

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
