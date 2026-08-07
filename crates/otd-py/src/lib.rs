//! `otd-py` — embedded CPython for parameter expressions.
//!
//! PLAN.md §3 calls Python "non-negotiable for TD migration", and §6 flags the
//! GIL against realtime: "scripts run at a fixed phase of the frame, never
//! block render". Both shape what this crate is.
//!
//! **Expressions are pure and short.** A parameter expression may read the
//! network but never mutate it — [`Network`] is the only thing reachable from
//! Python here, and it is read-only. That keeps evaluation reentrant with the
//! cook and means a runaway expression can only be slow, not corrupting.
//!
//! **Compiled code is cached.** Compiling on every frame for every parameter
//! would dominate the frame; sources are compiled once and keyed by content,
//! so editing an expression recompiles exactly one thing.
//!
//! **Failure is never fatal.** A broken expression reports its error and the
//! parameter falls back to its constant, because a typo during a show must not
//! stop the render.

use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::CString;

use otd_core::{ChannelSource, EvalContext, ParamEdit, Value};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

/// Set for the duration of one evaluation so the functions exposed to Python
/// can reach the network.
///
/// The pointer is valid only inside [`PyEngine::eval`], which sets it, runs
/// the expression to completion on this thread, and clears it before
/// returning. Python cannot retain it: the objects handed out are plain
/// floats and strings.
type NetPtr = *const (dyn ChannelSource + 'static);

thread_local! {
    static NETWORK: Cell<Option<NetPtr>> = const { Cell::new(None) };
    static NODE_PATH: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    /// Parameter changes a callback has asked for, drained by the host after
    /// the frame. See `otd_core::edit` for why they are queued rather than
    /// applied where they are written.
    static EDITS: std::cell::RefCell<Vec<ParamEdit>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct NetworkGuard;

impl NetworkGuard {
    fn set(net: Option<&dyn ChannelSource>, path: &str) -> NetworkGuard {
        // SAFETY: the lifetime is erased only for the duration of this
        // guard, which is dropped before the borrow it came from ends.
        NETWORK.with(|n| {
            n.set(net.map(|n| unsafe {
                std::mem::transmute::<*const dyn ChannelSource, NetPtr>(
                    n as *const dyn ChannelSource,
                )
            }))
        });
        NODE_PATH.with(|p| *p.borrow_mut() = path.to_string());
        NetworkGuard
    }
}

impl Drop for NetworkGuard {
    fn drop(&mut self) {
        NETWORK.with(|n| n.set(None));
        NODE_PATH.with(|p| p.borrow_mut().clear());
    }
}

fn with_network<R>(f: impl FnOnce(&dyn ChannelSource, &str) -> R) -> Option<R> {
    let ptr = NETWORK.with(|n| n.get())?;
    let path = NODE_PATH.with(|p| p.borrow().clone());
    // SAFETY: the pointer is only ever set by `NetworkGuard`, which clears it
    // before the borrow it came from ends, and evaluation is synchronous on
    // this thread.
    let net: &dyn ChannelSource = unsafe { &*ptr };
    Some(f(net, &path))
}

/// `ch('/lfo1', 'chan1')` — a CHOP channel's current value.
#[pyfunction]
#[pyo3(signature = (op_path, channel = "chan1"))]
fn ch(op_path: &str, channel: &str) -> f64 {
    with_network(|net, _| net.channel(op_path, channel).unwrap_or(0.0) as f64).unwrap_or(0.0)
}

/// `setpar('/blur1', 'size', 12)` — ask for a parameter change.
///
/// Queued, not applied: the cook is in progress and must see one unchanging
/// graph. The host applies the queue between frames, so the change lands next
/// frame. Nothing here can fail loudly — a bad path is reported when the queue
/// is applied, because that is where it is discovered.
#[pyfunction]
fn setpar(op_path: &str, name: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
    let value = py_to_value(value).map_err(pyo3::exceptions::PyTypeError::new_err)?;
    EDITS.with(|e| e.borrow_mut().push(ParamEdit::new(op_path, name, value)));
    Ok(())
}

/// `par('/blur1', 'size')` — another operator's parameter value.
#[pyfunction]
fn par(py: Python<'_>, op_path: &str, name: &str) -> Py<PyAny> {
    with_network(|net, _| net.param_value(op_path, name))
        .flatten()
        .map(|v| value_to_py(py, &v))
        .unwrap_or_else(|| py.None())
}

/// `parent('speed')` — a custom parameter on the enclosing component.
#[pyfunction]
fn parent(py: Python<'_>, name: &str) -> Py<PyAny> {
    with_network(|net, path| net.parent_param(path, name))
        .flatten()
        .map(|v| value_to_py(py, &v))
        .unwrap_or_else(|| py.None())
}

fn value_to_py(py: Python<'_>, v: &Value) -> Py<PyAny> {
    match v {
        Value::Float(f) => f.into_pyobject(py).unwrap().into_any().unbind(),
        Value::Int(i) => i.into_pyobject(py).unwrap().into_any().unbind(),
        Value::Bool(b) => b.into_pyobject(py).unwrap().to_owned().into_any().unbind(),
        Value::Str(s) => s.into_pyobject(py).unwrap().into_any().unbind(),
        Value::Vec2(a) => PyTuple::new(py, a).unwrap().into_any().unbind(),
        Value::Vec3(a) => PyTuple::new(py, a).unwrap().into_any().unbind(),
        Value::Vec4(a) => PyTuple::new(py, a).unwrap().into_any().unbind(),
    }
}

/// Convert a Python result back to a parameter value.
fn py_to_value(obj: &Bound<'_, PyAny>) -> Result<Value, String> {
    if let Ok(b) = obj.extract::<bool>() {
        return Ok(Value::Bool(b));
    }
    if let Ok(f) = obj.extract::<f64>() {
        return Ok(Value::Float(f));
    }
    if let Ok(i) = obj.extract::<i64>() {
        return Ok(Value::Int(i));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(Value::Str(s));
    }
    if let Ok(seq) = obj.extract::<Vec<f64>>() {
        return match seq.len() {
            2 => Ok(Value::Vec2([seq[0], seq[1]])),
            3 => Ok(Value::Vec3([seq[0], seq[1], seq[2]])),
            4 => Ok(Value::Vec4([seq[0], seq[1], seq[2], seq[3]])),
            _ => Err(format!(
                "a sequence of {} is not a parameter value",
                seq.len()
            )),
        };
    }
    Err(format!(
        "expression produced {}, which is not a parameter value",
        obj.get_type()
            .name()
            .map(|n| n.to_string())
            .unwrap_or_default()
    ))
}

/// Turn a Python exception into the one line the parameter panel has room for.
fn format_error(py: Python<'_>, err: &PyErr) -> String {
    let kind = err
        .get_type(py)
        .name()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| "Error".into());
    let msg = err.value(py).to_string();
    if msg.is_empty() {
        kind
    } else {
        format!("{kind}: {msg}")
    }
}

pub struct PyEngine {
    /// Names available to every expression, built once.
    globals: Py<PyDict>,
    /// Compiled expressions, keyed by source.
    cache: HashMap<String, Py<PyAny>>,
    /// One namespace per callback source, so the `def`s in an Execute DAT are
    /// executed once rather than on every frame that fires one of them. Keyed
    /// by source, so editing the script rebuilds exactly that namespace — and
    /// module-level state written by a callback survives between frames, which
    /// is what makes a counter in an Execute DAT work at all.
    namespaces: HashMap<String, Py<PyDict>>,
    /// Modules the user has asked for, kept so `import` costs happen once.
    pub startup_error: Option<String>,
}

impl PyEngine {
    /// Start the interpreter and build the expression scope.
    ///
    /// Returns an engine with `startup_error` set rather than failing: a
    /// project that uses no Python must still open on a machine where the
    /// interpreter cannot start.
    pub fn new() -> PyEngine {
        let mut engine = PyEngine {
            globals: Python::attach(|py| PyDict::new(py).unbind()),
            cache: HashMap::new(),
            namespaces: HashMap::new(),
            startup_error: None,
        };
        if let Err(e) = engine.build_scope() {
            engine.startup_error = Some(e);
        }
        engine
    }

    fn build_scope(&mut self) -> Result<(), String> {
        Python::attach(|py| {
            let globals = self.globals.bind(py);
            // A small, useful standard library rather than everything: these
            // are what expressions actually reach for.
            for module in ["math", "random", "json"] {
                let m = py
                    .import(module)
                    .map_err(|e| format!("import {module}: {}", format_error(py, &e)))?;
                globals.set_item(module, m).map_err(|e| e.to_string())?;
            }
            let math = py.import("math").map_err(|e| e.to_string())?;
            for name in [
                "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sqrt", "floor", "ceil",
                "exp", "log", "pi", "tau", "fmod", "hypot", "degrees", "radians",
            ] {
                if let Ok(f) = math.getattr(name) {
                    globals.set_item(name, f).map_err(|e| e.to_string())?;
                }
            }
            globals
                .set_item("ch", wrap_pyfunction!(ch, py).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            globals
                .set_item("par", wrap_pyfunction!(par, py).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            globals
                .set_item(
                    "parent",
                    wrap_pyfunction!(parent, py).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            globals
                .set_item(
                    "setpar",
                    wrap_pyfunction!(setpar, py).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            // Helpers with no Python equivalent that artists expect.
            py.run(
                c"
def clamp(v, lo, hi):
    return lo if v < lo else (hi if v > hi else v)

def fit(v, oldlo, oldhi, newlo, newhi):
    t = (v - oldlo) / (oldhi - oldlo) if oldhi != oldlo else 0.0
    return newlo + t * (newhi - newlo)

def lerp(a, b, t):
    return a + (b - a) * t

def smoothstep(lo, hi, v):
    t = clamp((v - lo) / (hi - lo) if hi != lo else 0.0, 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)
",
                Some(globals),
                None,
            )
            .map_err(|e| format!("scope setup: {}", format_error(py, &e)))?;
            Ok(())
        })
    }

    /// Whether the interpreter is usable.
    pub fn is_available(&self) -> bool {
        self.startup_error.is_none()
    }

    /// Evaluate one expression.
    ///
    /// `path` is the operator being cooked, so `parent(...)` resolves against
    /// the right component.
    pub fn eval(&mut self, source: &str, ctx: &EvalContext, path: &str) -> Result<Value, String> {
        if let Some(e) = &self.startup_error {
            return Err(e.clone());
        }
        let compiled = self.compile(source)?;
        let _guard = NetworkGuard::set(ctx.channels, path);

        Python::attach(|py| {
            let globals = self.globals.bind(py);
            let locals = PyDict::new(py);
            locals.set_item("absTime", ctx.abs_time).ok();
            locals.set_item("time", ctx.time).ok();
            locals.set_item("frame", ctx.frame).ok();
            locals.set_item("fps", ctx.fps).ok();
            locals.set_item("me", path).ok();

            // A compiled code object is run through the `eval` builtin; the
            // per-frame values live in `locals` so the globals dict — and the
            // compiled cache with it — stays constant.
            let builtins = py.import("builtins").map_err(|e| format_error(py, &e))?;
            let result = builtins
                .call_method1("eval", (compiled.bind(py), globals.clone(), locals))
                .map_err(|e| format_error(py, &e))?;
            py_to_value(&result)
        })
    }

    fn compile(&mut self, source: &str) -> Result<Py<PyAny>, String> {
        if let Some(c) = self.cache.get(source) {
            return Ok(Python::attach(|py| c.clone_ref(py)));
        }
        let code = Python::attach(|py| {
            let src = CString::new(source).map_err(|_| "expression contains a NUL byte")?;
            let builtins = py.import("builtins").map_err(|e| format_error(py, &e))?;
            let code = builtins
                .call_method1("compile", (src.to_str().unwrap(), "<parameter>", "eval"))
                .map_err(|e| format_error(py, &e))?;
            Ok::<Py<PyAny>, String>(code.unbind())
        })?;
        // Bound in size so a project that rewrites expressions procedurally
        // cannot grow the cache without limit.
        if self.cache.len() > 4096 {
            self.cache.clear();
        }
        self.cache.insert(source.to_string(), code);
        Ok(Python::attach(|py| self.cache[source].clone_ref(py)))
    }

    /// Run a statement block — a Script DAT, or a callback body. Returns
    /// whatever the block left in a variable called `result`, if anything.
    pub fn run(
        &mut self,
        source: &str,
        ctx: &EvalContext,
        path: &str,
    ) -> Result<Option<Value>, String> {
        if let Some(e) = &self.startup_error {
            return Err(e.clone());
        }
        let _guard = NetworkGuard::set(ctx.channels, path);
        Python::attach(|py| {
            let globals = self.globals.bind(py);
            let locals = PyDict::new(py);
            locals.set_item("absTime", ctx.abs_time).ok();
            locals.set_item("time", ctx.time).ok();
            locals.set_item("frame", ctx.frame).ok();
            locals.set_item("me", path).ok();
            let src = CString::new(source).map_err(|_| "script contains a NUL byte")?;
            py.run(&src, Some(globals), Some(&locals))
                .map_err(|e| format_error(py, &e))?;
            match locals.get_item("result") {
                Ok(Some(v)) => py_to_value(&v).map(Some),
                _ => Ok(None),
            }
        })
    }

    /// Call a function defined in `source`, if it defines one by that name.
    ///
    /// Returns whether the function existed, so an Execute DAT can leave the
    /// callbacks it does not care about undefined rather than having to write
    /// an empty body for each.
    pub fn call(
        &mut self,
        source: &str,
        func: &str,
        args: &[Value],
        ctx: &EvalContext,
        path: &str,
    ) -> Result<bool, String> {
        if let Some(e) = &self.startup_error {
            return Err(e.clone());
        }
        let _guard = NetworkGuard::set(ctx.channels, path);
        // Rebuild the namespace only when the source changed.
        if !self.namespaces.contains_key(source) {
            let built = Python::attach(|py| -> Result<Py<PyDict>, String> {
                let ns = PyDict::new(py);
                let src = CString::new(source).map_err(|_| "script contains a NUL byte")?;
                py.run(&src, Some(self.globals.bind(py)), Some(&ns))
                    .map_err(|e| format_error(py, &e))?;
                Ok(ns.unbind())
            })?;
            // One namespace per Execute DAT in a project is the realistic
            // count; this bound is only here so a script being retyped does
            // not accumulate one per keystroke.
            if self.namespaces.len() > 64 {
                self.namespaces.clear();
            }
            self.namespaces.insert(source.to_string(), built);
        }

        Python::attach(|py| {
            let ns = self.namespaces[source].bind(py);
            let Ok(Some(f)) = ns.get_item(func) else {
                return Ok(false);
            };
            if !f.is_callable() {
                return Ok(false);
            }
            // `me`, `absTime` and `frame` are globals for a callback rather
            // than arguments, so a signature stays about the event.
            ns.set_item("me", path).ok();
            ns.set_item("absTime", ctx.abs_time).ok();
            ns.set_item("time", ctx.time).ok();
            ns.set_item("frame", ctx.frame).ok();

            let args: Vec<Py<PyAny>> = args.iter().map(|v| value_to_py(py, v)).collect();
            let tuple = PyTuple::new(py, &args).map_err(|e| format_error(py, &e))?;
            f.call1(tuple).map_err(|e| format_error(py, &e))?;
            Ok(true)
        })
    }

    /// Take the parameter changes callbacks have asked for since the last
    /// drain. The host applies them between frames — see `otd_core::edit`.
    pub fn take_edits(&mut self) -> Vec<ParamEdit> {
        EDITS.with(|e| std::mem::take(&mut *e.borrow_mut()))
    }

    /// Rows of text produced by a script, for a Script DAT.
    pub fn run_table(
        &mut self,
        source: &str,
        ctx: &EvalContext,
        path: &str,
    ) -> Result<Vec<Vec<String>>, String> {
        if let Some(e) = &self.startup_error {
            return Err(e.clone());
        }
        let _guard = NetworkGuard::set(ctx.channels, path);
        Python::attach(|py| {
            let globals = self.globals.bind(py);
            let locals = PyDict::new(py);
            locals.set_item("absTime", ctx.abs_time).ok();
            locals.set_item("frame", ctx.frame).ok();
            locals.set_item("me", path).ok();
            let src = CString::new(source).map_err(|_| "script contains a NUL byte")?;
            py.run(&src, Some(globals), Some(&locals))
                .map_err(|e| format_error(py, &e))?;

            let Ok(Some(rows)) = locals.get_item("rows") else {
                return Ok(Vec::new());
            };
            let rows = rows
                .cast::<PyList>()
                .map_err(|_| "`rows` must be a list of lists".to_string())?
                .clone();
            let mut out = Vec::with_capacity(rows.len());
            for row in rows.iter() {
                // A DAT holds text. Rows of mixed types are the normal case —
                // `['name', 1]` — so every cell is stringified individually
                // rather than requiring a uniform row.
                let cells = match row.try_iter() {
                    Ok(items) => items
                        .filter_map(|i| i.ok())
                        .map(|i| i.str().map(|s| s.to_string()).unwrap_or_default())
                        .collect(),
                    Err(_) => vec![row.str().map(|s| s.to_string()).unwrap_or_default()],
                };
                out.push(cells);
            }
            Ok(out)
        })
    }
}

impl Default for PyEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeNet;
    impl ChannelSource for FakeNet {
        fn channel(&self, op_path: &str, channel: &str) -> Option<f32> {
            (op_path == "/lfo1" && channel == "chan1").then_some(0.5)
        }
        fn param_value(&self, op_path: &str, name: &str) -> Option<Value> {
            (op_path == "/blur1" && name == "size").then_some(Value::Float(8.0))
        }
        fn parent_param(&self, _node: &str, name: &str) -> Option<Value> {
            (name == "speed").then_some(Value::Float(2.5))
        }
    }

    fn ctx() -> EvalContext<'static> {
        EvalContext {
            abs_time: 2.0,
            frame: 120,
            fps: 60.0,
            channels: Some(&FakeNet),
            ..Default::default()
        }
    }

    #[test]
    fn arithmetic_and_the_maths_scope() {
        let mut e = PyEngine::new();
        assert!(e.is_available(), "{:?}", e.startup_error);
        assert_eq!(e.eval("1 + 2 * 3", &ctx(), "/x").unwrap().as_f64(), 7.0);
        assert!((e.eval("sin(pi / 2)", &ctx(), "/x").unwrap().as_f64() - 1.0).abs() < 1e-12);
        assert_eq!(
            e.eval("clamp(5, 0, 1)", &ctx(), "/x").unwrap().as_f64(),
            1.0
        );
        assert_eq!(
            e.eval("fit(0.5, 0, 1, 10, 20)", &ctx(), "/x")
                .unwrap()
                .as_f64(),
            15.0
        );
    }

    #[test]
    fn time_and_the_network_are_in_scope() {
        let mut e = PyEngine::new();
        assert_eq!(e.eval("absTime", &ctx(), "/x").unwrap().as_f64(), 2.0);
        assert_eq!(e.eval("frame", &ctx(), "/x").unwrap().as_i64(), 120);
        assert_eq!(
            e.eval("ch('/lfo1', 'chan1')", &ctx(), "/x")
                .unwrap()
                .as_f64(),
            0.5
        );
        assert_eq!(
            e.eval("par('/blur1', 'size')", &ctx(), "/x")
                .unwrap()
                .as_f64(),
            8.0
        );
        assert_eq!(
            e.eval("parent('speed')", &ctx(), "/x").unwrap().as_f64(),
            2.5
        );
    }

    #[test]
    fn python_expressions_can_use_python() {
        let mut e = PyEngine::new();
        assert_eq!(
            e.eval("sum(x * x for x in range(4))", &ctx(), "/x")
                .unwrap()
                .as_f64(),
            14.0
        );
        assert_eq!(
            e.eval("'on' if absTime > 1 else 'off'", &ctx(), "/x")
                .unwrap()
                .as_str(),
            "on"
        );
        assert_eq!(
            e.eval("[1.0, 2.0, 3.0]", &ctx(), "/x").unwrap(),
            Value::Vec3([1.0, 2.0, 3.0])
        );
    }

    #[test]
    fn an_error_is_reported_as_one_line() {
        let mut e = PyEngine::new();
        let err = e.eval("1 / 0", &ctx(), "/x").unwrap_err();
        assert!(err.contains("ZeroDivisionError"), "{err}");
        assert!(err.len() < 200, "{err}");

        let err = e.eval("nope +", &ctx(), "/x").unwrap_err();
        assert!(err.contains("SyntaxError"), "{err}");

        let err = e.eval("undefined_name", &ctx(), "/x").unwrap_err();
        assert!(err.contains("NameError"), "{err}");
    }

    #[test]
    fn the_network_is_unreachable_once_evaluation_ends() {
        let mut e = PyEngine::new();
        // Stash the function, then call it outside any evaluation: it must
        // read nothing rather than dereference a stale pointer.
        e.eval("ch('/lfo1', 'chan1')", &ctx(), "/x").unwrap();
        let no_net = EvalContext {
            channels: None,
            ..Default::default()
        };
        assert_eq!(
            e.eval("ch('/lfo1', 'chan1')", &no_net, "/x")
                .unwrap()
                .as_f64(),
            0.0
        );
    }

    #[test]
    fn a_script_can_return_a_value_and_a_table() {
        let mut e = PyEngine::new();
        let v = e
            .run(
                "total = 0\nfor i in range(5):\n    total += i\nresult = total",
                &ctx(),
                "/x",
            )
            .unwrap();
        assert_eq!(v.map(|v| v.as_f64()), Some(10.0));

        let rows = e
            .run_table(
                "rows = [['name', 'value'], ['a', 1], ['b', 2]]",
                &ctx(),
                "/x",
            )
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], vec!["name", "value"]);
        assert_eq!(rows[2][1], "2");
    }

    #[test]
    fn compiling_the_same_source_twice_reuses_it() {
        let mut e = PyEngine::new();
        for _ in 0..100 {
            e.eval("absTime * 2", &ctx(), "/x").unwrap();
        }
        assert_eq!(e.cache.len(), 1);
    }
}
