//! Parameters — the four-mode system from PLAN.md §2.3.
//!
//! Every parameter is one of:
//!   * **Constant**   — a literal value the artist types
//!   * **Expression** — re-evaluated every cook (see [`crate::expr`])
//!   * **Export**     — driven by a CHOP channel (Phase 2; the plumbing is
//!     here, the CHOP side is not yet built)
//!   * **Bind**       — two-way link to another parameter (Phase 2)
//!
//! Modes are stored side by side rather than as an enum payload so that
//! flipping a parameter to Expression and back doesn't discard the constant
//! the artist had dialled in. TD behaves the same way, and it matters a lot in
//! live use.

use serde::{Deserialize, Serialize};

use crate::expr::{EvalContext, Expr};
use crate::value::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamMode {
    #[default]
    Constant,
    Expression,
    Export,
    Bind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Param {
    /// Human label for the parameter panel. Falls back to the key if empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// The constant value, and the declared type of the parameter.
    pub value: Value,
    #[serde(default, skip_serializing_if = "is_default_mode")]
    pub mode: ParamMode,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expression: String,
    /// `op_path:channel` for Export mode, `op_path:param` for Bind mode.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// Soft UI range for sliders. Does not clamp the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<(f64, f64)>,
    /// Menu entries, if this parameter is a menu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<Vec<String>>,
    /// Added by the author rather than by the operator definition. Custom
    /// parameters on a component are that component's API (PLAN.md §2.3), so
    /// they are written to the project file in full — nothing else knows they
    /// exist.
    #[serde(default, skip_serializing_if = "is_false")]
    pub custom: bool,

    #[serde(skip)]
    compiled: Option<Expr>,
    #[serde(skip)]
    error: Option<String>,
    /// The built-in language could not parse this, so it is Python's.
    #[serde(skip)]
    needs_python: bool,
    /// Operator paths this expression mentions, so the cook engine can make
    /// them dependencies.
    #[serde(skip)]
    refs: Vec<String>,
    /// This parameter holds script source rather than a value, so the paths
    /// inside it are references even though the parameter is a constant.
    /// Set by the operator definition, not by the project file.
    #[serde(skip)]
    script: bool,
    /// This parameter's whole value is an operator path — a Render TOP's
    /// camera, a Geometry COMP's SOP. Those have to cook first, so the path
    /// is a dependency exactly like a wire.
    #[serde(skip)]
    path_ref: bool,
}

/// Pull operator paths out of an expression.
///
/// An expression like `ch('/lfo1', 'chan1')` has to make `/lfo1` cook first,
/// and there is no way to know that without looking at the source: the
/// interpreter only finds out when it runs. Quoted strings that look like
/// operator paths are treated as references. Over-reporting is harmless — an
/// extra dependency costs one cached lookup — while under-reporting would
/// read a stale channel.
fn extract_paths(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = source.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let quote = bytes[i];
        if quote != '\'' && quote != '"' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < bytes.len() && bytes[j] != quote {
            j += 1;
        }
        let literal: String = bytes[start..j.min(bytes.len())].iter().collect();
        if literal.starts_with('/') && !out.contains(&literal) {
            out.push(literal);
        }
        i = j + 1;
    }
    out
}

fn is_default_mode(m: &ParamMode) -> bool {
    *m == ParamMode::Constant
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A path parameter's whole value, when it looks like one.
fn own_path(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.starts_with('/') {
        vec![trimmed.to_string()]
    } else {
        Vec::new()
    }
}

/// Keep the parameter's declared type: a Python expression returning a float
/// for an int parameter should round, not change the parameter's type.
fn coerce_like(declared: &Value, produced: Value) -> Value {
    if declared.same_type_as(&produced) {
        produced
    } else {
        declared.coerce_from_f64(produced.as_f64())
    }
}

impl Param {
    pub fn new(value: impl Into<Value>) -> Self {
        Param {
            label: String::new(),
            value: value.into(),
            mode: ParamMode::Constant,
            expression: String::new(),
            source: String::new(),
            range: None,
            menu: None,
            custom: false,
            compiled: None,
            error: None,
            needs_python: false,
            refs: Vec::new(),
            script: false,
            path_ref: false,
        }
    }

    /// Mark this parameter as naming an operator. The path it holds becomes a
    /// cook dependency, so a Render TOP's camera and geometry are guaranteed
    /// to be up to date before it draws.
    pub fn as_path_ref(mut self) -> Self {
        self.path_ref = true;
        self.refs = own_path(&self.value.as_str());
        self
    }

    pub fn is_path_ref(&self) -> bool {
        self.path_ref
    }

    /// Mark this parameter as holding script source. Operator paths quoted
    /// inside it then become cook dependencies, the same as in an expression:
    /// a Script DAT reading `ch('/lfo1', ...)` must make `/lfo1` cook first.
    pub fn as_script(mut self) -> Self {
        self.script = true;
        self.refs = extract_paths(&self.value.as_str());
        self
    }

    pub fn is_script(&self) -> bool {
        self.script
    }

    /// Same as [`Param::as_script`], for a parameter already built.
    pub fn into_script(self) -> Self {
        self.as_script()
    }

    /// Same as [`Param::as_path_ref`], for a parameter already built.
    pub fn into_path_ref(self) -> Self {
        self.as_path_ref()
    }

    /// Mark this as an author-defined parameter on a component.
    pub fn as_custom(mut self) -> Self {
        self.custom = true;
        self
    }

    pub fn with_label(mut self, label: &str) -> Self {
        self.label = label.to_string();
        self
    }

    pub fn with_range(mut self, lo: f64, hi: f64) -> Self {
        self.range = Some((lo, hi));
        self
    }

    pub fn with_menu(mut self, items: &[&str]) -> Self {
        self.menu = Some(items.iter().map(|s| s.to_string()).collect());
        self
    }

    /// Switch to Expression mode with the given source, compiling immediately
    /// so the artist sees a syntax error the moment they type it.
    pub fn set_expression(&mut self, src: &str) {
        self.expression = src.to_string();
        self.mode = ParamMode::Expression;
        self.recompile();
    }

    pub fn set_constant(&mut self, value: Value) {
        self.value = value;
        self.mode = ParamMode::Constant;
        self.refresh_refs();
    }

    /// Recompute what this parameter references from its current value.
    fn refresh_refs(&mut self) {
        if self.script {
            self.refs = extract_paths(&self.value.as_str());
        } else if self.path_ref {
            self.refs = own_path(&self.value.as_str());
        }
    }

    /// Recompile the expression. Must be called after deserialization — the
    /// compiled AST is not part of the project file.
    pub fn recompile(&mut self) {
        self.needs_python = false;
        if self.script || self.path_ref {
            self.refresh_refs();
        } else {
            self.refs.clear();
        }
        if self.mode != ParamMode::Expression || self.expression.trim().is_empty() {
            self.compiled = None;
            self.error = None;
            return;
        }
        match Expr::parse(&self.expression) {
            Ok(e) => {
                self.compiled = Some(e);
                self.error = None;
            }
            // Not an error yet: the built-in language is a fast path for the
            // common case, and anything it cannot parse is handed to Python.
            // Only if Python is absent too does this become a real error.
            Err(_) => {
                self.compiled = None;
                self.error = None;
                self.needs_python = true;
                self.refs = extract_paths(&self.expression);
            }
        }
    }

    /// The last reported evaluation or compilation error, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// True when this parameter's value can change without anyone editing it.
    /// Propagated into the cook engine's `time_dependent` flag.
    pub fn is_time_dependent(&self) -> bool {
        match self.mode {
            // A Python expression is assumed animated: proving otherwise would
            // mean analysing arbitrary Python, and cooking a static parameter
            // every frame is far cheaper than a viewer that never updates.
            ParamMode::Expression if self.needs_python => true,
            ParamMode::Expression => self
                .compiled
                .as_ref()
                .map(|e| e.is_time_dependent())
                .unwrap_or(false),
            // A CHOP export re-evaluates whenever its source CHOP cooks; that
            // is handled as a graph dependency, not as time dependence.
            _ => false,
        }
    }

    /// Resolve to a concrete value for this cook.
    ///
    /// Never fails: a broken expression holds the last good constant and
    /// records the error, because dropping frames in a live show over a typo
    /// is not acceptable behaviour.
    pub fn eval(&self, ctx: &EvalContext) -> Value {
        match self.mode {
            ParamMode::Constant => self.value.clone(),
            ParamMode::Expression => match &self.compiled {
                Some(e) => match e.eval(ctx) {
                    Ok(v) => self.value.coerce_from_f64(v),
                    Err(_) => self.value.clone(),
                },
                None if self.needs_python => ctx
                    .channels
                    .and_then(|c| c.eval_python(&self.expression, ctx, ctx.path.unwrap_or("")))
                    .and_then(|r| r.ok())
                    .map(|v| coerce_like(&self.value, v))
                    .unwrap_or_else(|| self.value.clone()),
                None => self.value.clone(),
            },
            ParamMode::Export => self
                .source_parts()
                .and_then(|(path, channel)| ctx.channels?.channel(path, channel))
                .map(|v| self.value.coerce_from_f64(v as f64))
                .unwrap_or_else(|| self.value.clone()),
            ParamMode::Bind => self
                .source_parts()
                .and_then(|(path, param)| ctx.channels?.param_value(path, param))
                .filter(|v| v.same_type_as(&self.value))
                .unwrap_or_else(|| self.value.clone()),
        }
    }

    /// Split `source` into the operator path and the channel or parameter
    /// name after it: `/lfo1:chan1`.
    pub fn source_parts(&self) -> Option<(&str, &str)> {
        let (path, name) = self.source.rsplit_once(':')?;
        if path.trim().is_empty() || name.trim().is_empty() {
            return None;
        }
        Some((path.trim(), name.trim()))
    }

    /// The operator this parameter reads from, if it is in Export or Bind
    /// mode. The cook engine turns these into dependencies.
    /// Operators this parameter reads from, whatever the mode: an Export or
    /// Bind source, or a path mentioned in a Python expression.
    pub fn referenced_ops(&self) -> impl Iterator<Item = &str> {
        self.source_op()
            .into_iter()
            .chain(self.refs.iter().map(|s| s.as_str()))
    }

    pub fn source_op(&self) -> Option<&str> {
        match self.mode {
            ParamMode::Export | ParamMode::Bind => self.source_parts().map(|(p, _)| p),
            _ => None,
        }
    }

    /// Point this parameter at a CHOP channel.
    pub fn set_export(&mut self, op_path: &str, channel: &str) {
        self.source = format!("{op_path}:{channel}");
        self.mode = ParamMode::Export;
    }

    /// Point this parameter at another operator's parameter.
    pub fn set_bind(&mut self, op_path: &str, param: &str) {
        self.source = format!("{op_path}:{param}");
        self.mode = ParamMode::Bind;
    }

    /// Like [`Param::eval`] but reports evaluation failures, for the UI.
    pub fn eval_checked(&self, ctx: &EvalContext) -> Result<Value, String> {
        if let Some(err) = &self.error {
            return Err(err.clone());
        }
        if self.mode == ParamMode::Expression {
            match &self.compiled {
                Some(e) => e
                    .eval(ctx)
                    .map(|v| self.value.coerce_from_f64(v))
                    .map_err(|e| e.0),
                None if self.needs_python => match ctx
                    .channels
                    .and_then(|c| c.eval_python(&self.expression, ctx, ctx.path.unwrap_or("")))
                {
                    Some(Ok(v)) => Ok(coerce_like(&self.value, v)),
                    Some(Err(e)) => Err(e),
                    None => Err("no Python interpreter in this build".to_string()),
                },
                None => Ok(self.value.clone()),
            }
        } else {
            Ok(self.eval(ctx))
        }
    }
}

/// Convenience constructors so operator definitions read declaratively.
impl Param {
    pub fn float(default: f64) -> Self {
        Param::new(default)
    }
    pub fn int(default: i64) -> Self {
        Param::new(default)
    }
    pub fn bool(default: bool) -> Self {
        Param::new(default)
    }
    pub fn str(default: &str) -> Self {
        Param::new(default)
    }
    pub fn rgba(default: [f64; 4]) -> Self {
        Param::new(default)
    }
    pub fn xyz(default: [f64; 3]) -> Self {
        Param::new(default)
    }
    pub fn menu(default: &str, items: &[&str]) -> Self {
        Param::new(default).with_menu(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expression_mode_keeps_the_constant_it_replaced() {
        let mut p = Param::float(0.25);
        p.set_expression("absTime * 2");
        assert_eq!(p.mode, ParamMode::Expression);
        let v = p.eval(&EvalContext {
            abs_time: 1.5,
            ..Default::default()
        });
        assert_eq!(v.as_f64(), 3.0);

        p.mode = ParamMode::Constant;
        assert_eq!(p.eval(&EvalContext::default()).as_f64(), 0.25);
    }

    #[test]
    fn an_expression_the_fast_path_cannot_parse_is_handed_to_python() {
        // Anything the built-in language does not understand is Python's,
        // whether it is valid Python or not — this crate cannot tell, and
        // guessing would reject working code.
        let mut p = Param::float(7.0);
        p.set_expression("sum(x for x in range(4))");
        assert!(p.needs_python);
        assert!(
            p.is_time_dependent(),
            "an unanalysable expression must be assumed animated"
        );
        assert_eq!(p.referenced_ops().count(), 0);

        // With no interpreter available the constant stands, and asking for
        // the value explicitly says why.
        assert_eq!(p.eval(&EvalContext::default()).as_f64(), 7.0);
        assert!(
            p.eval_checked(&EvalContext::default())
                .unwrap_err()
                .contains("Python")
        );
    }

    #[test]
    fn paths_mentioned_in_an_expression_become_references() {
        let mut p = Param::float(0.0);
        p.set_expression("ch('/lfo1', 'chan1') + ch(\"/audio/level\", 'band1')");
        let refs: Vec<&str> = p.referenced_ops().collect();
        assert_eq!(refs, vec!["/lfo1", "/audio/level"]);

        // A quoted string that is not a path is not a reference.
        p.set_expression("'hello'.upper()");
        assert_eq!(p.referenced_ops().count(), 0);
    }

    struct FakeNetwork;
    impl crate::expr::ChannelSource for FakeNetwork {
        fn channel(&self, op_path: &str, channel: &str) -> Option<f32> {
            (op_path == "/lfo1" && channel == "chan1").then_some(0.75)
        }
        fn param_value(&self, op_path: &str, param: &str) -> Option<Value> {
            (op_path == "/other" && param == "size").then_some(Value::Float(12.0))
        }
    }

    fn ctx_with_network() -> EvalContext<'static> {
        EvalContext {
            channels: Some(&FakeNetwork),
            ..Default::default()
        }
    }

    #[test]
    fn export_mode_reads_a_chop_channel() {
        let mut p = Param::float(0.1);
        p.set_export("/lfo1", "chan1");
        assert_eq!(p.mode, ParamMode::Export);
        assert_eq!(p.source_op(), Some("/lfo1"));
        assert_eq!(p.eval(&ctx_with_network()).as_f64(), 0.75);
        // With no network to read from, the constant stands in.
        assert_eq!(p.eval(&EvalContext::default()).as_f64(), 0.1);
    }

    #[test]
    fn bind_mode_reads_another_parameter() {
        let mut p = Param::float(1.0);
        p.set_bind("/other", "size");
        assert_eq!(p.eval(&ctx_with_network()).as_f64(), 12.0);
    }

    #[test]
    fn a_bind_of_the_wrong_type_is_ignored() {
        let mut p = Param::str("hello");
        p.set_bind("/other", "size");
        assert_eq!(p.eval(&ctx_with_network()).as_str(), "hello");
    }

    #[test]
    fn a_dangling_export_falls_back_to_the_constant() {
        let mut p = Param::float(0.4);
        p.set_export("/gone", "chan1");
        assert_eq!(p.eval(&ctx_with_network()).as_f64(), 0.4);
    }

    #[test]
    fn int_parameters_stay_ints_under_expressions() {
        let mut p = Param::int(0);
        p.set_expression("2.6");
        assert_eq!(p.eval(&EvalContext::default()), Value::Int(3));
    }
}
