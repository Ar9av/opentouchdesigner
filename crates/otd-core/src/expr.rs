//! A tiny numeric expression language for parameters in Expression mode.
//!
//! PLAN.md commits to embedded CPython for the real expression engine
//! (Phase 3). That's a heavy dependency to carry through Phase 0/1, and the
//! Phase 1 demo needs *something* live behind the Expression mode, so this
//! module implements a self-contained arithmetic evaluator with the same
//! variable names Python mode will expose (`absTime`, `me.time`, `frame`).
//!
//! When PyO3 lands, this stays as the fallback used by headless builds
//! compiled without the `python` feature — expressions written against it
//! remain valid Python expressions, so projects keep loading.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    Var(String),
    Neg(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

impl BinOp {
    fn precedence(self) -> u8 {
        match self {
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne => 1,
            BinOp::Add | BinOp::Sub => 2,
            BinOp::Mul | BinOp::Div | BinOp::Rem => 3,
            BinOp::Pow => 4,
        }
    }
    fn right_assoc(self) -> bool {
        self == BinOp::Pow
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExprError(pub String);

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ExprError {}

/// Read-only access to the rest of the network, for parameters that are
/// driven by something other than their own value.
///
/// Kept as a trait so `otd-core` still knows nothing about CHOPs: the CHOP
/// engine implements it, and a build with no CHOPs simply passes `None`.
pub trait ChannelSource {
    /// The current value of `channel` on the CHOP at `op_path` — Export mode.
    fn channel(&self, op_path: &str, channel: &str) -> Option<f32>;

    /// The current value of `param` on the operator at `op_path` — Bind mode.
    fn param_value(&self, op_path: &str, param: &str) -> Option<crate::value::Value>;

    /// A custom parameter on the component containing `node_path` — how the
    /// operators inside a component read its API (`parent.speed`).
    fn parent_param(&self, _node_path: &str, _param: &str) -> Option<crate::value::Value> {
        None
    }
}

/// Everything a parameter is allowed to see. Deliberately narrow: parameters
/// must not be able to reach into the graph and mutate it mid-cook.
#[derive(Clone, Copy, Default)]
pub struct EvalContext<'a> {
    /// Frame number since the project started.
    pub frame: i64,
    /// Component-local time in seconds (per-component local time, PLAN.md §2.2).
    pub time: f64,
    /// Wall-clock seconds since the project started, independent of local time.
    pub abs_time: f64,
    /// Nominal frames per second of the timeline.
    pub fps: f64,
    /// Present once CHOPs are cooking; `None` in a pure-TOP build or before
    /// the first frame.
    pub channels: Option<&'a dyn ChannelSource>,
    /// The path of the operator being evaluated, so `parent.x` can be
    /// resolved relative to it.
    pub path: Option<&'a str>,
}

impl fmt::Debug for EvalContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvalContext")
            .field("frame", &self.frame)
            .field("time", &self.time)
            .field("abs_time", &self.abs_time)
            .field("fps", &self.fps)
            .field("channels", &self.channels.is_some())
            .finish()
    }
}

/// Variable names that make an expression re-cook every frame.
const TIME_VARS: [&str; 5] = ["absTime", "time", "frame", "me.time", "absTime.seconds"];

impl Expr {
    pub fn parse(src: &str) -> Result<Expr, ExprError> {
        let tokens = tokenize(src)?;
        let mut p = Parser { tokens, pos: 0 };
        let e = p.parse_expr(0)?;
        if p.pos != p.tokens.len() {
            return Err(ExprError(format!(
                "unexpected trailing input at token {}",
                p.pos
            )));
        }
        Ok(e)
    }

    /// True if the value can change with time even when nothing is edited.
    /// This is what drives `time_dependent` propagation in the cook engine.
    pub fn is_time_dependent(&self) -> bool {
        match self {
            Expr::Num(_) => false,
            Expr::Var(name) => TIME_VARS.contains(&name.as_str()),
            Expr::Neg(e) => e.is_time_dependent(),
            Expr::Bin(_, a, b) => a.is_time_dependent() || b.is_time_dependent(),
            Expr::Call(_, args) => args.iter().any(|a| a.is_time_dependent()),
        }
    }

    pub fn eval(&self, ctx: &EvalContext) -> Result<f64, ExprError> {
        match self {
            Expr::Num(v) => Ok(*v),
            Expr::Var(name) => {
                // `parent.speed` reads a custom parameter on the enclosing
                // component — the mechanism that turns a component into a
                // reusable thing with knobs.
                if let Some(param) = name.strip_prefix("parent.") {
                    if let (Some(net), Some(path)) = (ctx.channels, ctx.path) {
                        if let Some(v) = net.parent_param(path, param) {
                            return Ok(v.as_f64());
                        }
                    }
                    return Err(ExprError(format!(
                        "no custom parameter `{param}` on the parent component"
                    )));
                }
                eval_var(name, ctx)
            }
            Expr::Neg(e) => Ok(-e.eval(ctx)?),
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.eval(ctx)?, b.eval(ctx)?);
                Ok(match op {
                    BinOp::Add => a + b,
                    BinOp::Sub => a - b,
                    BinOp::Mul => a * b,
                    BinOp::Div => a / b,
                    BinOp::Rem => a % b,
                    BinOp::Pow => a.powf(b),
                    BinOp::Lt => bool_f64(a < b),
                    BinOp::Gt => bool_f64(a > b),
                    BinOp::Le => bool_f64(a <= b),
                    BinOp::Ge => bool_f64(a >= b),
                    BinOp::Eq => bool_f64(a == b),
                    BinOp::Ne => bool_f64(a != b),
                })
            }
            Expr::Call(name, args) => {
                let a: Result<Vec<f64>, ExprError> = args.iter().map(|e| e.eval(ctx)).collect();
                eval_call(name, &a?)
            }
        }
    }
}

fn bool_f64(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

fn eval_var(name: &str, ctx: &EvalContext) -> Result<f64, ExprError> {
    Ok(match name {
        "absTime" | "absTime.seconds" => ctx.abs_time,
        "absTime.frame" => (ctx.abs_time * ctx.fps.max(1.0)).floor(),
        "time" | "me.time" => ctx.time,
        "frame" => ctx.frame as f64,
        "fps" => ctx.fps,
        "pi" => std::f64::consts::PI,
        "tau" => std::f64::consts::TAU,
        "e" => std::f64::consts::E,
        "True" | "true" => 1.0,
        "False" | "false" => 0.0,
        other => return Err(ExprError(format!("unknown name `{other}`"))),
    })
}

fn eval_call(name: &str, a: &[f64]) -> Result<f64, ExprError> {
    fn arity(name: &str, a: &[f64], n: usize) -> Result<(), ExprError> {
        if a.len() == n {
            Ok(())
        } else {
            Err(ExprError(format!(
                "{name}() takes {n} argument(s), got {}",
                a.len()
            )))
        }
    }
    Ok(match name {
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "abs" | "floor" | "ceil" | "round"
        | "sqrt" | "exp" | "log" | "sign" | "fract" | "radians" | "degrees" => {
            arity(name, a, 1)?;
            let x = a[0];
            match name {
                "sin" => x.sin(),
                "cos" => x.cos(),
                "tan" => x.tan(),
                "asin" => x.asin(),
                "acos" => x.acos(),
                "atan" => x.atan(),
                "abs" => x.abs(),
                "floor" => x.floor(),
                "ceil" => x.ceil(),
                "round" => x.round(),
                "sqrt" => x.sqrt(),
                "exp" => x.exp(),
                "log" => x.ln(),
                "sign" => x.signum(),
                "fract" => x - x.floor(),
                "radians" => x.to_radians(),
                _ => x.to_degrees(),
            }
        }
        "pow" | "atan2" | "min" | "max" | "step" | "mod" => {
            arity(name, a, 2)?;
            match name {
                "pow" => a[0].powf(a[1]),
                "atan2" => a[0].atan2(a[1]),
                "min" => a[0].min(a[1]),
                "max" => a[0].max(a[1]),
                "mod" => a[0].rem_euclid(a[1]),
                _ => bool_f64(a[1] >= a[0]),
            }
        }
        "clamp" => {
            arity(name, a, 3)?;
            a[0].clamp(a[1], a[2])
        }
        "lerp" | "mix" => {
            arity(name, a, 3)?;
            a[0] + (a[1] - a[0]) * a[2]
        }
        "smoothstep" => {
            arity(name, a, 3)?;
            let t = ((a[2] - a[0]) / (a[1] - a[0])).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }
        "fit" => {
            // fit(value, oldMin, oldMax, newMin, newMax) — the TD workhorse.
            arity(name, a, 5)?;
            let t = (a[0] - a[1]) / (a[2] - a[1]);
            a[3] + t * (a[4] - a[3])
        }
        other => return Err(ExprError(format!("unknown function `{other}`"))),
    })
}

// ---------------------------------------------------------------- tokenizer

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(BinOp),
    LParen,
    RParen,
    Comma,
    Minus,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, ExprError> {
    let mut out = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '+' => {
                out.push(Tok::Op(BinOp::Add));
                i += 1;
            }
            '-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '*' => {
                if chars.get(i + 1) == Some(&'*') {
                    out.push(Tok::Op(BinOp::Pow));
                    i += 2;
                } else {
                    out.push(Tok::Op(BinOp::Mul));
                    i += 1;
                }
            }
            '/' => {
                out.push(Tok::Op(BinOp::Div));
                i += 1;
            }
            '%' => {
                out.push(Tok::Op(BinOp::Rem));
                i += 1;
            }
            '^' => {
                out.push(Tok::Op(BinOp::Pow));
                i += 1;
            }
            '<' | '>' | '=' | '!' => {
                let two = chars.get(i + 1) == Some(&'=');
                let op = match (c, two) {
                    ('<', false) => BinOp::Lt,
                    ('<', true) => BinOp::Le,
                    ('>', false) => BinOp::Gt,
                    ('>', true) => BinOp::Ge,
                    ('=', true) => BinOp::Eq,
                    ('!', true) => BinOp::Ne,
                    _ => return Err(ExprError(format!("stray `{c}`"))),
                };
                out.push(Tok::Op(op));
                i += if two { 2 } else { 1 };
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                // exponent form: 1e-3
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let save = i;
                    i += 1;
                    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                        i += 1;
                    }
                    if i < chars.len() && chars[i].is_ascii_digit() {
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    } else {
                        i = save;
                    }
                }
                let s: String = chars[start..i].iter().collect();
                out.push(Tok::Num(
                    s.parse()
                        .map_err(|_| ExprError(format!("bad number literal `{s}`")))?,
                ));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
                {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect()));
            }
            other => return Err(ExprError(format!("unexpected character `{other}`"))),
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn parse_expr(&mut self, min_prec: u8) -> Result<Expr, ExprError> {
        let mut lhs = self.parse_atom()?;
        while let Some(Tok::Op(op)) = self.peek().cloned() {
            if op.precedence() < min_prec {
                break;
            }
            self.pos += 1;
            let next_min = if op.right_assoc() {
                op.precedence()
            } else {
                op.precedence() + 1
            };
            let rhs = self.parse_expr(next_min)?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_atom(&mut self) -> Result<Expr, ExprError> {
        match self.tokens.get(self.pos).cloned() {
            Some(Tok::Minus) => {
                self.pos += 1;
                // Unary minus binds tighter than * but looser than **.
                Ok(Expr::Neg(Box::new(self.parse_expr(4)?)))
            }
            Some(Tok::Num(v)) => {
                self.pos += 1;
                Ok(Expr::Num(v))
            }
            Some(Tok::LParen) => {
                self.pos += 1;
                let e = self.parse_expr(0)?;
                self.expect(Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                if self.peek() == Some(&Tok::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.parse_expr(0)?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Tok::RParen)?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => Err(ExprError(format!(
                "expected a value, found {}",
                match other {
                    None => "end of input".to_string(),
                    Some(t) => format!("{t:?}"),
                }
            ))),
        }
    }

    fn expect(&mut self, t: Tok) -> Result<(), ExprError> {
        if self.peek() == Some(&t) {
            self.pos += 1;
            Ok(())
        } else {
            Err(ExprError(format!("expected {t:?}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(src: &str) -> f64 {
        Expr::parse(src)
            .expect("parse")
            .eval(&EvalContext {
                frame: 60,
                time: 1.0,
                abs_time: 2.0,
                fps: 60.0,
                channels: None,
                path: None,
            })
            .expect("eval")
    }

    #[test]
    fn arithmetic_precedence() {
        assert_eq!(ev("1 + 2 * 3"), 7.0);
        assert_eq!(ev("(1 + 2) * 3"), 9.0);
        assert_eq!(ev("2 ** 3 ** 2"), 512.0); // right associative
        assert_eq!(ev("-2 + 3"), 1.0);
        assert_eq!(ev("10 % 3"), 1.0);
    }

    #[test]
    fn functions_and_vars() {
        assert!((ev("sin(pi/2)") - 1.0).abs() < 1e-12);
        assert_eq!(ev("clamp(5, 0, 1)"), 1.0);
        assert_eq!(ev("fit(0.5, 0, 1, 10, 20)"), 15.0);
        assert_eq!(ev("frame"), 60.0);
        assert_eq!(ev("absTime"), 2.0);
    }

    #[test]
    fn time_dependence_is_detected() {
        assert!(Expr::parse("sin(absTime * 2)").unwrap().is_time_dependent());
        assert!(Expr::parse("frame % 30").unwrap().is_time_dependent());
        assert!(!Expr::parse("1 + 2 * pi").unwrap().is_time_dependent());
    }

    #[test]
    fn errors_are_reported_not_panicked() {
        assert!(Expr::parse("1 +").is_err());
        assert!(Expr::parse("foo(").is_err());
        assert!(Expr::parse("2 $ 3").is_err());
        assert!(
            Expr::parse("nope")
                .unwrap()
                .eval(&EvalContext::default())
                .is_err()
        );
    }
}
