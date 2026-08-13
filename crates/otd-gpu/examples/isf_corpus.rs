//! How much of the real ISF world actually imports?
//!
//! `Import ISF…` is one of the better-value features here — there are a
//! couple of hundred published shaders that are supposed to run unmodified —
//! and until now it was tested against shaders written for the test. That
//! proves the happy path and nothing else.
//!
//! Point this at a checkout of a real collection and it reports what fraction
//! import, compile, and would actually cook, grouped by why the rest do not.
//! The grouping is the point: "62% pass" is a number, and "the other 38% are
//! four fixable things and one that is genuinely out of scope" is a plan.
//!
//!     git clone --depth 1 https://github.com/Vidvox/ISF-Files
//!     cargo run -p otd-gpu --example isf_corpus -- ISF-Files
//!     cargo run -p otd-gpu --example isf_corpus -- ISF-Files --show MultiPass
//!
//! Not a test: it needs a corpus that is not in this repository.

use std::collections::BTreeMap;
use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().unwrap_or_else(|| {
        eprintln!("usage: isf_corpus <dir-of-.fs-files> [--show <reason>] [--list]");
        std::process::exit(1);
    });
    let mut show: Option<String> = None;
    let mut list = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--show" => show = args.next(),
            "--list" => list = true,
            _ => {}
        }
    }

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    collect(Path::new(&root), &mut files);
    files.sort();
    if files.is_empty() {
        eprintln!("no .fs files under {root}");
        std::process::exit(1);
    }

    let mut reasons: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut ok = 0usize;

    for path in &files {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let Ok(source) = std::fs::read_to_string(path) else {
            reasons.entry("unreadable".into()).or_default().push(name);
            continue;
        };

        // Stage 1: does the header parse and the shader convert?
        let isf = match otd_gpu::isf::import(&source) {
            Ok(isf) => isf,
            Err(e) => {
                reasons.entry(bucket(&e.to_string())).or_default().push(name);
                continue;
            }
        };

        // Stage 2: does what comes out of the converter actually compile?
        // Importing a shader that then fails on the node is worse than
        // refusing it, because the refusal at least says why.
        match otd_gpu::shader::validate_glsl(&otd_gpu::shader::wrap_glsl(&isf.source)) {
            Ok(()) => {
                ok += 1;
                if list {
                    println!("ok    {name}");
                }
            }
            Err(e) => {
                reasons
                    .entry(format!("compile: {}", bucket(&e)))
                    .or_default()
                    .push(format!("{name}  |  {}", one_line(&e)));
            }
        }
    }

    let total = files.len();
    println!("\n{ok}/{total} imported and compiled ({}%)\n", ok * 100 / total);

    let mut rows: Vec<(&String, &Vec<String>)> = reasons.iter().collect();
    rows.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    for (reason, names) in &rows {
        println!("{:>4}  {reason}", names.len());
        if show.as_deref().map(|s| reason.contains(s)).unwrap_or(false) {
            for n in names.iter() {
                println!("        {n}");
            }
        }
    }
    if show.is_none() {
        println!("\n(--show <substring of a reason> lists the files in that group)");
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(90).collect()
}

fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().map(|x| x == "fs").unwrap_or(false) {
            out.push(p);
        }
    }
}

/// Collapse an error to the class of error, so the report is a handful of
/// buckets rather than three hundred unique strings.
fn bucket(message: &str) -> String {
    let m = message.to_lowercase();
    for key in [
        "multi-pass",
        "multipass",
        "unknown variable",
        "no definition in scope",
        "not implemented",
        "too many arguments",
        "expected",
        "cannot apply",
        "type mismatch",
        "no matching function",
        "unknown type",
        "no header",
        "json",
        "parameters",
    ] {
        if m.contains(key) {
            return key.to_string();
        }
    }
    message.chars().take(48).collect()
}
