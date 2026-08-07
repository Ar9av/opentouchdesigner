//! `otd` — OpenTouchDesigner without a window.
//!
//! PLAN.md Phase 5 asks for deployment: a show machine that boots into a
//! project, and a way to render a project to files. Both are the editor's
//! frame loop with the editor removed, so both live here.
//!
//! Argument parsing is by hand. A CLI with five subcommands and four flags
//! does not need a dependency, and the error messages here can say what to do
//! next, which is worth more than the flag parsing is worth saving.

mod runtime;

use std::path::PathBuf;
use std::process::ExitCode;

use runtime::{Runtime, summarize};

const USAGE: &str = "\
otd — OpenTouchDesigner, headless

USAGE:
    otd run <project.otd> [--frames N] [--fps N] [--node PATH]
    otd render <project.otd> --node <path> [--frames N] [--out DIR] [--fps N]
    otd stats <project.otd> [--frames N] [--fps N] [--node PATH]
    otd bundle <project.otd> --out <dir>
    otd docs [--out FILE]

COMMANDS:
    run       Cook the project in realtime. Output CHOPs (DMX, OSC) keep
              sending; nothing is displayed. Runs until interrupted unless
              --frames is given.
    render    Write a node's output to a numbered PNG sequence.
    stats     Cook a fixed number of frames and report the timings.
    bundle    Copy the project and every component it references into one
              folder, with the references rewritten to be relative to it.
    docs      Write the operator reference, generated from the registry.

OPTIONS:
    --frames N   How many frames to run. `run` defaults to forever.
    --fps N      Frame rate. Defaults to the project's own.
    --node PATH  Cook this node, e.g. /out1, whether or not it is flagged for
                 render. Required by `render`, which reads its output.
    --out DIR    Where to write frames. Defaults to ./frames.
";

/// Why we stopped. Asking for help is not a failure, and exiting non-zero for
/// it breaks any script that runs `otd --help` to check the binary works.
#[derive(Debug)]
enum Fail {
    Help,
    Err(String),
}

impl From<String> for Fail {
    fn from(e: String) -> Fail {
        Fail::Err(e)
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fail::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(Fail::Err(e)) => {
            eprintln!("otd: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
struct Args {
    command: String,
    project: PathBuf,
    frames: Option<u64>,
    fps: Option<f64>,
    node: Option<String>,
    out: PathBuf,
    /// Whether `--out` was actually given, as opposed to defaulted. `docs`
    /// prints to stdout without it, so a sentinel comparison against the
    /// default would break the day somebody asks for `--out frames`.
    out_given: bool,
}

fn parse(argv: Vec<String>) -> Result<Args, Fail> {
    let mut it = argv.into_iter().skip(1);
    let command = it.next().ok_or(Fail::Help)?;
    if command == "-h" || command == "--help" || command == "help" {
        return Err(Fail::Help);
    }
    if !["run", "render", "stats", "bundle", "docs"].contains(&command.as_str()) {
        return Err(format!("unknown command `{command}`\n\n{USAGE}").into());
    }
    // `docs` reads the operator registry, not a project — it is about the
    // build, not about anything an artist made.
    let project = if command == "docs" {
        PathBuf::new()
    } else {
        PathBuf::from(
            it.next()
                .ok_or_else(|| Fail::Err(format!("`{command}` needs a project file\n\n{USAGE}")))?,
        )
    };

    let mut args = Args {
        command,
        project,
        frames: None,
        fps: None,
        node: None,
        out: PathBuf::from("frames"),
        out_given: false,
    };
    while let Some(flag) = it.next() {
        let mut value = || -> Result<String, Fail> {
            it.next()
                .ok_or_else(|| Fail::Err(format!("`{flag}` needs a value\n\n{USAGE}")))
        };
        match flag.as_str() {
            "--frames" => {
                let v = value()?;
                args.frames = Some(
                    v.parse()
                        .map_err(|_| format!("--frames: `{v}` is not a whole number"))?,
                );
            }
            "--fps" => {
                let v = value()?;
                let fps: f64 = v
                    .parse()
                    .map_err(|_| format!("--fps: `{v}` is not a number"))?;
                if fps <= 0.0 {
                    return Err(Fail::Err("--fps must be greater than zero".into()));
                }
                args.fps = Some(fps);
            }
            "--node" => args.node = Some(value()?),
            "--out" => {
                args.out = PathBuf::from(value()?);
                args.out_given = true;
            }
            other => return Err(format!("unknown option `{other}`\n\n{USAGE}").into()),
        }
    }
    if args.command == "render" && args.node.is_none() {
        return Err(Fail::Err("`render` needs --node, e.g. --node /out1".into()));
    }
    if args.command == "bundle" && !args.out_given {
        return Err(Fail::Err(
            "`bundle` needs --out <dir>, the folder to write the bundle into".into(),
        ));
    }
    Ok(args)
}

fn run() -> Result<(), Fail> {
    let args = parse(std::env::args().collect())?;

    // Bundling is a file operation. Going through the runtime would demand a
    // GPU, and packaging a show on a build server is exactly the case that
    // does not have one.
    if args.command == "bundle" {
        return Ok(bundle(&args)?);
    }
    if args.command == "docs" {
        return Ok(docs(&args)?);
    }

    let mut rt = Runtime::open(&args.project)?;
    if let Some(fps) = args.fps {
        rt.time.fps = fps;
    }
    let dt = 1.0 / rt.time.fps;

    match args.command.as_str() {
        "render" => render(&mut rt, &args, dt),
        "stats" => stats(
            &mut rt,
            args.frames.unwrap_or(120),
            args.node.as_deref(),
            dt,
        ),
        _ => realtime(&mut rt, args.frames, args.node.as_deref(), dt),
    }?;
    Ok(())
}

/// The roots to cook: the project's render-flagged nodes, plus anything named
/// with `--node`.
///
/// Naming a node on the command line has to be enough on its own. Requiring a
/// flag as well would mean opening the editor and re-saving the project just
/// to profile or render one branch of it.
fn roots(rt: &Runtime, node: Option<&str>) -> Result<Vec<otd_core::NodeId>, String> {
    let mut roots = rt.roots();
    if let Some(path) = node {
        let id = rt.find(path)?;
        if !roots.contains(&id) {
            roots.push(id);
        }
    }
    if roots.is_empty() {
        return Err(
            "nothing to cook: no node in this project has its Render flag set.\n\
             The render flag is what marks an output when there is no viewer to look at.\n\
             Set one in the editor, or pass `--node <path>` to name one directly."
                .into(),
        );
    }
    Ok(roots)
}

/// Cook in realtime. This is the show-machine mode: no pixels leave the
/// process, but output operators — DMX, OSC — do their work every frame.
fn realtime(
    rt: &mut Runtime,
    frames: Option<u64>,
    node: Option<&str>,
    dt: f64,
) -> Result<(), String> {
    let roots = roots(rt, node)?;
    eprintln!(
        "running {} node(s) at {:.0} fps{}",
        roots.len(),
        rt.time.fps,
        match frames {
            Some(n) => format!(", {n} frames"),
            None => ", until interrupted".into(),
        }
    );

    let started = std::time::Instant::now();
    let mut n = 0u64;
    let mut reported = false;
    while frames.is_none_or(|f| n < f) {
        let timing = rt.frame(&roots, dt)?;
        n += 1;

        if !reported {
            reported = true;
            report_shader_errors(rt);
        }
        // Pace to the target rate. Without this, a light project spins the CPU
        // and every time-sliced CHOP runs at whatever the machine can manage.
        let target = std::time::Duration::from_secs_f64(dt * n as f64);
        if let Some(slack) = target.checked_sub(started.elapsed()) {
            std::thread::sleep(slack);
        } else if timing.wall_ms > dt * 1000.0 * 2.0 {
            eprintln!(
                "frame {n}: {:.1} ms — behind the frame rate",
                timing.wall_ms
            );
        }
    }
    Ok(())
}

/// Write a numbered PNG sequence.
fn render(rt: &mut Runtime, args: &Args, dt: f64) -> Result<(), String> {
    let node = rt.find(args.node.as_deref().unwrap())?;
    let frames = args.frames.unwrap_or(1);
    std::fs::create_dir_all(&args.out).map_err(|e| format!("{}: {e}", args.out.display()))?;
    let targets = roots(rt, args.node.as_deref())?;

    for i in 0..frames {
        rt.frame(&targets, dt)?;
        if i == 0 {
            report_shader_errors(rt);
        }
        let (w, h, pixels) = rt.read(node)?;
        let path = args.out.join(format!("{:05}.png", i));
        image::save_buffer(&path, &pixels, w, h, image::ColorType::Rgba8)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    eprintln!(
        "wrote {frames} frame(s) to {}/00000.png..",
        args.out.display()
    );
    Ok(())
}

/// Write the operator reference.
fn docs(args: &Args) -> Result<(), String> {
    let text = otd_engine::docs::reference(&otd_engine::registry());
    // No --out: straight to stdout, so it composes with a pipe.
    if !args.out_given {
        print!("{text}");
        return Ok(());
    }
    if let Some(parent) = args.out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(&args.out, &text).map_err(|e| format!("{}: {e}", args.out.display()))?;
    eprintln!("wrote {}", args.out.display());
    Ok(())
}

/// Copy the project and everything it references into one folder.
fn bundle(args: &Args) -> Result<(), String> {
    let registry = otd_engine::registry();
    let fps = otd_core::Project::load(&args.project)
        .map_err(|e| format!("{}: {e}", args.project.display()))?
        .fps;
    let graph = otd_core::Project::open(&args.project, &registry)
        .map_err(|e| format!("{}: {e}", args.project.display()))?;
    let name = args
        .project
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "show".into());

    let out = otd_core::bundle::export(&graph, &registry, fps, &args.out, &name)
        .map_err(|e| format!("{}: {e}", args.out.display()))?;
    println!(
        "{}  ({} component(s))",
        out.project.display(),
        out.components.len()
    );
    for (path, file, reason) in &out.missing {
        eprintln!("missing component — {path} refers to `{file}`: {reason}");
    }
    // A bundle with a missing component will fail on the show machine. Better
    // to fail here, where somebody is watching.
    if out.missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} component(s) could not be copied",
            out.missing.len()
        ))
    }
}

/// Cook a fixed number of frames as fast as possible and report what it cost.
fn stats(rt: &mut Runtime, frames: u64, node: Option<&str>, dt: f64) -> Result<(), String> {
    let roots = roots(rt, node)?;

    // Shader compilation and the first texture allocation are not steady
    // state, so they are measured separately rather than smeared into the
    // numbers that matter.
    let warmup = rt.frame(&roots, dt)?;
    report_shader_errors(rt);

    let mut cook = Vec::with_capacity(frames as usize);
    let mut wall = Vec::with_capacity(frames as usize);
    let mut last = warmup;
    for _ in 0..frames {
        last = rt.frame(&roots, dt)?;
        cook.push(last.cook_ms);
        wall.push(last.wall_ms);
    }

    let (cook_med, cook_p95, cook_max) = summarize(cook);
    let (wall_med, wall_p95, wall_max) = summarize(wall.clone());
    println!("frames        {frames} at {:.0} fps target", rt.time.fps);
    println!(
        "nodes         {} cooked, {} cached (last frame)",
        last.cooked, last.cached
    );
    println!(
        "first frame   {:.2} ms  (compilation and allocation)",
        warmup.wall_ms
    );
    println!("cook    ms    median {cook_med:.2}   p95 {cook_p95:.2}   max {cook_max:.2}");
    println!("frame   ms    median {wall_med:.2}   p95 {wall_p95:.2}   max {wall_max:.2}");

    let budget = 1000.0 / rt.time.fps;
    let over = wall.iter().filter(|ms| **ms > budget).count();
    println!(
        "budget  {budget:.2} ms   {over} frame(s) over{}",
        if over == 0 { " — holds the rate" } else { "" }
    );

    // Which nodes, not just how long. Ranked by cost per *frame*: a node that
    // takes 4 ms but only cooks when a parameter changes is not why a patch
    // drops frames, and ranking by cook time alone would put it first.
    let mut rows: Vec<(String, f64, f64, f64)> = rt
        .graph
        .walk()
        .into_iter()
        .filter(|id| *id != rt.graph.root())
        .map(|id| {
            (
                rt.graph.path(id),
                rt.cook.frame_cost_ms(id),
                rt.cook.avg_cook_ms(id),
                rt.cook.cook_rate(id),
            )
        })
        .filter(|(_, _, avg, _)| *avg > 0.0)
        .collect();
    rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if !rows.is_empty() {
        println!();
        println!(
            "{:<32} {:>10} {:>10} {:>7}",
            "node", "ms/frame", "per cook", "cooks"
        );
        let shown = rows.len().min(15);
        for (path, cost, avg, rate) in rows.iter().take(shown) {
            println!("{path:<32} {cost:>10.3} {avg:>10.3} {:>6.0}%", rate * 100.0);
        }
        // Never let a cap read as "that was everything".
        if rows.len() > shown {
            println!("... and {} more", rows.len() - shown);
        }
    }
    Ok(())
}

fn report_shader_errors(rt: &Runtime) {
    for err in rt.shader_errors() {
        eprintln!("shader error — {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        std::iter::once("otd".to_string())
            .chain(s.split_whitespace().map(String::from))
            .collect()
    }

    #[test]
    fn flags_are_parsed_in_any_order() {
        let a = parse(argv("render p.otd --out shots --frames 30 --node /out1")).unwrap();
        assert_eq!(a.command, "render");
        assert_eq!(a.project, PathBuf::from("p.otd"));
        assert_eq!(a.frames, Some(30));
        assert_eq!(a.node.as_deref(), Some("/out1"));
        assert_eq!(a.out, PathBuf::from("shots"));
    }

    #[test]
    fn defaults_do_not_silently_pick_a_frame_rate() {
        // Left alone, the frame rate comes from the project rather than from
        // a number invented here.
        let a = parse(argv("run p.otd")).unwrap();
        assert_eq!(a.fps, None);
        assert_eq!(a.frames, None, "`run` runs until interrupted");
    }

    #[test]
    fn a_bad_argument_says_what_was_wrong() {
        for (args, wanted) in [
            ("render p.otd", "--node"),
            ("run p.otd --frames soon", "not a whole number"),
            ("run p.otd --fps 0", "greater than zero"),
            ("run p.otd --fps", "needs a value"),
            ("dance p.otd", "unknown command"),
            ("run", "needs a project file"),
            ("run p.otd --loud", "unknown option"),
        ] {
            let Err(Fail::Err(err)) = parse(argv(args)) else {
                panic!("`{args}` should have been rejected");
            };
            assert!(err.contains(wanted), "`{args}` -> {err}");
        }
    }

    #[test]
    fn asking_for_help_is_not_a_failure() {
        // `otd --help` in a script must not exit non-zero, and it must not
        // complain about a project file it was never given.
        for args in ["--help", "-h", "help", ""] {
            assert!(
                matches!(parse(argv(args)), Err(Fail::Help)),
                "`otd {args}` should print usage and succeed"
            );
        }
    }
}
