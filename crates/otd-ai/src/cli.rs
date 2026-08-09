//! The two providers that are not a URL: Claude Code and Codex, run as
//! subprocesses against the login already on this machine.
//!
//! The point is billing. A Claude Pro/Max or ChatGPT Plus/Pro subscription
//! entitles you to *those tools*, not to an API key, and the supported way to
//! use them from another program is their own non-interactive mode — `claude
//! -p`, `codex exec`. So that is what this does: spawn the binary the user
//! already logged in with, put the prompt on its stdin, read the reply off
//! its stdout. No token is read out of anybody's credential store and no
//! request is made to an API endpoint on a subscription's behalf, because
//! both of those are somebody else's terms of service.
//!
//! Four things this file is careful about, all of them learned the hard way:
//!
//! **The binary is usually not on `PATH`.** A `.app` launched from Finder
//! inherits `/usr/bin:/bin:/usr/sbin:/sbin` and nothing else, and both of
//! these install to `~/.local/bin`. Looking only at `PATH` means the feature
//! works from a terminal and is invisible to every user who double-clicks the
//! icon. See [`binary`].
//!
//! **The agent is turned off.** Both CLIs are coding agents by default: they
//! read files, run commands, and load whatever `CLAUDE.md`/`AGENTS.md` is
//! lying around. We want one JSON object back, so tools are denied, the
//! sandbox is read-only, customisations are off, and the working directory is
//! a temp dir with nothing in it. A patch generator that can read the user's
//! home directory is not a patch generator, it is a liability.
//!
//! **Nothing is left behind.** `--no-session-persistence`/`--ephemeral`: a
//! prompt box that quietly writes a transcript for every ask is a surprise.
//!
//! **A hung child gets killed.** [`Request::timeout`] is enforced here, the
//! same as it is on the HTTP path, because a subprocess that never exits
//! holds a worker thread for the rest of the session.
//!
//! One cost worth knowing before you wonder where your rate limit went:
//! Claude Code sends its tool schemas and preamble whether or not the tools
//! are allowed, so a trivial ask still bills around 28k input tokens. It is
//! free in money and not free in weekly quota.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::provider::{Provider, Request};

/// How often the parent looks to see whether the child has finished. Small
/// enough to not add latency worth measuring to a multi-second call, large
/// enough that the loop is free.
const POLL: Duration = Duration::from_millis(20);

/// Run a request through the provider's CLI and return the assistant's text.
///
/// Blocking, and slower to start than an HTTP call — process spawn plus the
/// CLI's own preamble is a second or two before any token moves. Call it from
/// a worker thread, which is what [`crate::complete_with_repair`] does.
pub fn complete(request: &Request) -> Result<String, String> {
    let provider = request.provider;
    let bin = binary(provider).ok_or_else(|| not_found(provider))?;

    let mut command = Command::new(&bin);
    let stdin_text = match provider {
        Provider::ClaudeCode => {
            command.args([
                // Non-interactive. Both formats are stream-json — see
                // `claude_turn` for why it is that rather than the simpler
                // `--output-format json`.
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                // Which stream-json output insists on, and which costs us
                // nothing: the extra lines are ignored.
                "--verbose",
                // No CLAUDE.md, skills, plugins, hooks, MCP servers or custom
                // agents. Auth and model selection still work normally, which
                // is the whole reason this is `--safe-mode` and not `--bare`:
                // `--bare` refuses to read the OAuth login and demands an API
                // key, which is exactly the thing we are here to avoid.
                "--safe-mode",
                "--no-session-persistence",
                // Deny every tool. It still has none to reach for given the
                // prompt, but "none to reach for" is a property of the prompt
                // and this is a property of the process.
                "--allowed-tools",
                "",
            ]);
            command.args(["--system-prompt", &request.system]);
            claude_turn(request)
        }
        Provider::Codex => {
            command.args([
                "exec",
                // JSONL events on stdout; the reply is the last agent message.
                "--json",
                // It cannot write, and it will not refuse to start because a
                // temp directory is not a git repository.
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "--ephemeral",
                "--color",
                "never",
            ]);
            // Codex takes an image as a path rather than as data, so the
            // processed bytes are written out and handed over by name. The
            // original file is not used even when there is one: this way the
            // shrink applies, and a pasted image works the same as a dropped
            // one.
            if let Some(image) = &request.image {
                command.arg("-i").arg(image.write_temp()?);
            }
            // `codex exec` has no system-prompt flag, so the system prompt
            // goes in front of the user turn. `patch::system_prompt` reads as
            // instructions either way — it never relied on being a separate
            // role — and the reply is validated against the registry
            // regardless of what the model was told.
            format!("{}\n\n{}", request.system, request.user)
        }
        _ => return Err("not a CLI provider".into()),
    };

    // Only pass a model when one was asked for. An empty model means "use
    // whatever this CLI is configured to use", which is the safe default:
    // Codex hard-fails on a model name the signed-in account cannot use, and
    // we cannot know that account's list from here.
    let model = request.model.trim();
    if !model.is_empty() {
        // Spelled the same either way, which is luck rather than design.
        command.args(["--model", model]);
    }

    // Nothing of the user's is in reach. Codex takes its workspace root as a
    // flag as well, because `-C` is what it reads rather than the process's
    // own directory.
    let scratch = std::env::temp_dir();
    command.current_dir(&scratch);
    if provider == Provider::Codex {
        command.arg("-C").arg(&scratch);
        // `-` is "read the prompt from stdin", and it has to come last.
        command.arg("-");
    }

    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", bin.display()))?;

    // Writer and readers on their own threads. A prompt with a catalogue in
    // it is larger than a pipe buffer, so writing it all before reading any
    // output deadlocks: the child blocks writing its preamble to a full
    // stdout while we block writing the tail of the prompt to a full stdin.
    let mut stdin = child.stdin.take().expect("piped");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(stdin_text.as_bytes());
        // Dropping it closes the pipe, which is how the child knows the
        // prompt has ended.
    });
    let mut out_pipe = child.stdout.take().expect("piped");
    let stdout = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = out_pipe.read_to_string(&mut buf);
        buf
    });
    let mut err_pipe = child.stderr.take().expect("piped");
    let stderr = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = err_pipe.read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + request.timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return Err(format!("lost track of {}: {e}", bin.display())),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{} did not answer within {}s",
                provider.label(),
                request.timeout.as_secs()
            ));
        }
        std::thread::sleep(POLL);
    };

    let _ = writer.join();
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();

    if !status.success() && out.trim().is_empty() {
        // Nothing on stdout to parse, so stderr is the whole story.
        let detail = tail(&err);
        // A flag it has never heard of means the binary we found is older
        // than the flags this file was written against — which happens when a
        // machine has two installs and the stale one won. Naming the path is
        // the whole of the fix: the user can see which one it picked.
        if detail.contains("unknown option") || detail.contains("unexpected argument") {
            return Err(format!(
                "{} at {} is too old for this — update it, or point {} at a newer one ({detail})",
                provider.label(),
                bin.display(),
                override_var(provider).unwrap_or("the override"),
            ));
        }
        return Err(match detail.is_empty() {
            true => format!("{} exited with {status}", provider.label()),
            false => format!("{}: {detail}", provider.label()),
        });
    }

    match provider {
        Provider::ClaudeCode => parse_claude(&out, &err),
        _ => parse_codex(&out, &err),
    }
}

/// The user turn, as the one line of stream-json Claude Code reads.
///
/// Plain `-p "text"` cannot carry an image — there is no flag for a local
/// one — but `--input-format stream-json` takes ordinary Anthropic content
/// blocks, which can. So every turn goes out this way, image or not: one
/// format to build and one to parse beats two of each that differ only in
/// whether a picture is attached.
fn claude_turn(request: &Request) -> String {
    let content = match &request.image {
        // Image first, same as on the wire: the instructions after it are
        // about the picture.
        Some(image) => serde_json::json!([
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": image.media_type(),
                    "data": image.base64(),
                },
            },
            { "type": "text", "text": request.user },
        ]),
        None => serde_json::json!([{ "type": "text", "text": request.user }]),
    };
    serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": content },
    })
    .to_string()
}

/// Claude Code's stream-json output is one object per line, ending in a
/// `result`. That last object is exactly what `--output-format json` would
/// have returned on its own — the reply in `result`, an error the same field
/// with `is_error` set — so the parsing below is the same either way, and
/// only the finding of it differs.
fn parse_claude(out: &str, err: &str) -> Result<String, String> {
    let last = out
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
        .rfind(|event| event.get("type").and_then(|t| t.as_str()) == Some("result"));

    let Some(body) = last else {
        return Err(format!(
            "Claude Code did not return JSON: {}",
            tail(out.trim())
        ));
    };

    let text = body
        .get("result")
        .and_then(|r| r.as_str())
        .unwrap_or_default();

    if body.get("is_error").and_then(|e| e.as_bool()) == Some(true) {
        return Err(explain(text, err, body.get("subtype")));
    }
    if text.trim().is_empty() {
        let reason = body
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .unwrap_or("no reason given");
        return Err(format!("Claude Code returned nothing (stopped: {reason})"));
    }
    Ok(text.to_string())
}

/// `codex exec --json` streams one JSON object per line. The reply is the
/// last completed `agent_message`; a failure arrives as a top-level `error`
/// or a `turn.failed`, both of which can appear *after* useful lines, so the
/// whole stream is read before deciding.
fn parse_codex(out: &str, err: &str) -> Result<String, String> {
    let mut answer: Option<String> = None;
    let mut failure: Option<String> = None;

    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match event.get("type").and_then(|t| t.as_str()) {
            Some("item.completed") => {
                let item = event.get("item");
                let kind = item
                    .and_then(|i| i.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or_default();
                if kind == "agent_message" {
                    if let Some(text) = item.and_then(|i| i.get("text")).and_then(|t| t.as_str()) {
                        answer = Some(text.to_string());
                    }
                }
                // An `error` item is a warning in this stream — a model
                // metadata note, a truncated skill list — and the turn
                // usually completes anyway. Only a fatal one is recorded.
            }
            Some("error") | Some("turn.failed") => {
                let message = event
                    .get("message")
                    .or_else(|| event.get("error").and_then(|e| e.get("message")))
                    .and_then(|m| m.as_str())
                    .unwrap_or("the turn failed");
                failure = Some(unwrap_nested(message));
            }
            _ => {}
        }
    }

    match answer {
        // An answer that arrived is worth more than a late warning.
        Some(text) if !text.trim().is_empty() => Ok(text),
        _ => Err(format!(
            "Codex: {}",
            failure.unwrap_or_else(|| {
                let detail = tail(err);
                match detail.is_empty() {
                    true => "the turn produced no reply".into(),
                    false => detail,
                }
            })
        )),
    }
}

/// Codex wraps the upstream error body in its own message as a JSON string.
/// One level of that is worth unwrapping, because the sentence inside is the
/// one a person can act on — "not supported when using Codex with a ChatGPT
/// account" rather than a brace-laden quotation of it.
fn unwrap_nested(message: &str) -> String {
    let trimmed = message.trim();
    if !trimmed.starts_with('{') {
        return trimmed.to_string();
    }
    serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .or_else(|| v.get("message"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| trimmed.to_string())
}

/// Say what went wrong and, where the CLI has a known failure mode, what to
/// do about it. A subscription that has run out for the week is the common
/// one and it does not read as anything in particular on its own.
fn explain(text: &str, err: &str, subtype: Option<&serde_json::Value>) -> String {
    let detail = match text.trim().is_empty() {
        false => text.trim().to_string(),
        true => tail(err),
    };
    let kind = subtype.and_then(|s| s.as_str()).unwrap_or_default();
    let advice = if detail.contains("login") || detail.contains("authenticat") || kind == "auth" {
        Some("run `claude` once in a terminal and sign in")
    } else if detail.contains("limit") || detail.contains("quota") {
        Some("the subscription's limit — wait for the reset, or use an API key provider")
    } else {
        None
    };
    match advice {
        Some(advice) if !detail.is_empty() => format!("Claude Code: {detail} — {advice}"),
        Some(advice) => format!("Claude Code: {advice}"),
        None if !detail.is_empty() => format!("Claude Code: {detail}"),
        None => "Claude Code failed without saying why".into(),
    }
}

/// The last few lines of a stream, for an error message. The end is where the
/// reason is; the beginning is startup noise.
fn tail(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("; ")
        .chars()
        .take(300)
        .collect()
}

fn not_found(provider: Provider) -> String {
    format!(
        "{} is not installed on this machine — install it and sign in, or pick a provider that takes an API key",
        provider.label()
    )
}

/// The command a provider runs, if it can be found.
fn command_name(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::ClaudeCode => Some("claude"),
        Provider::Codex => Some("codex"),
        _ => None,
    }
}

/// The environment variable that overrides the search, for a machine where
/// the binary is somewhere nobody would guess.
fn override_var(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::ClaudeCode => Some("OTD_CLAUDE_BIN"),
        Provider::Codex => Some("OTD_CODEX_BIN"),
        _ => None,
    }
}

/// Find the provider's binary.
///
/// `PATH` first, then the handful of places these two actually install to.
/// The fallback list is not belt and braces: a GUI application launched from
/// Finder or the Start menu gets a `PATH` that contains none of them, so for
/// most users the fallback *is* the lookup.
pub fn binary(provider: Provider) -> Option<PathBuf> {
    let name = command_name(provider)?;

    if let Some(var) = override_var(provider) {
        if let Some(value) = std::env::var_os(var) {
            let path = PathBuf::from(value);
            if is_executable(&path) {
                return Some(path);
            }
        }
    }

    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if let Some(hit) = probe(&dir, name) {
            return Some(hit);
        }
    }

    // Order matters, and not in the way you would guess. Both CLIs' own
    // installers put them in `~/.local/bin` and keep them updated there; a
    // copy in `/opt/homebrew/bin` or `/usr/local/bin` is usually an older
    // install somebody has forgotten about. Preferring the system directories
    // finds a version from months ago on a machine that has a current one
    // three lines further down the list — so the per-user paths go first.
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    {
        candidates.extend([
            home.join(".local/bin"),
            home.join(".claude/local"),
            home.join(".bun/bin"),
            home.join(".volta/bin"),
            home.join(".npm-global/bin"),
            home.join("AppData/Roaming/npm"),
        ]);
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    candidates.into_iter().find_map(|dir| probe(&dir, name))
}

/// Look for `name` in `dir`, including the extensions Windows needs.
fn probe(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    let plain = dir.join(name);
    if is_executable(&plain) {
        return Some(plain);
    }
    if cfg!(windows) {
        for ext in ["cmd", "exe", "bat"] {
            let with_ext = dir.join(format!("{name}.{ext}"));
            if is_executable(&with_ext) {
                return Some(with_ext);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}

/// Whether the CLI is there, and which version — the equivalent of "has a
/// key" for a provider that does not take one.
///
/// Spawns a process, so call it when the settings window opens rather than
/// once a frame.
pub fn detect(provider: Provider) -> Result<String, String> {
    let bin = binary(provider).ok_or_else(|| not_found(provider))?;
    let out = Command::new(&bin)
        .arg("--version")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run {}: {e}", bin.display()))?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match version.is_empty() {
        true => Ok(bin.display().to_string()),
        false => Ok(version),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claude_reply_is_the_result_field() {
        let out = r#"{"is_error":false,"stop_reason":"end_turn","result":"{\"ops\":[]}","type":"result"}"#;
        assert_eq!(parse_claude(out, "").unwrap(), "{\"ops\":[]}");
    }

    #[test]
    fn a_claude_failure_says_what_to_do_about_it() {
        let out =
            r#"{"is_error":true,"result":"Invalid API key · Please run /login","type":"result"}"#;
        let e = parse_claude(out, "").unwrap_err();
        assert!(e.contains("/login"), "{e}");
        assert!(e.contains("sign in"), "{e}");

        let limited = r#"{"is_error":true,"result":"5-hour limit reached","type":"result"}"#;
        let e = parse_claude(limited, "").unwrap_err();
        assert!(
            e.contains("API key provider"),
            "the way out is offered: {e}"
        );
    }

    #[test]
    fn the_result_is_found_among_the_stream_that_precedes_it() {
        // stream-json emits a system line, assistant lines and then the
        // result. Taking the first JSON object, or the last line blindly,
        // both get this wrong.
        let out = concat!(
            r#"{"type":"system","subtype":"init","session_id":"x"}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"partial"}]}}"#,
            "\n",
            r#"{"is_error":false,"stop_reason":"end_turn","result":"{\"ops\":[]}","type":"result"}"#,
            "\n",
        );
        assert_eq!(parse_claude(out, "").unwrap(), "{\"ops\":[]}");
    }

    #[test]
    fn a_turn_carries_an_image_as_content_blocks() {
        let buffer = image::RgbaImage::from_pixel(8, 8, image::Rgba([9, 9, 9, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let image = crate::vision::Image::decode(&png.into_inner()).unwrap();

        let request = Request::new(Provider::ClaudeCode, "sonnet")
            .user("rebuild this")
            .image(image);
        let line = claude_turn(&request);
        // One line, because stream-json is line-delimited and a pretty-printed
        // message would be several messages as far as the CLI is concerned.
        assert!(!line.contains('\n'), "{line:.80}");

        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["type"], "user");
        assert_eq!(parsed["message"]["role"], "user");
        let content = &parsed["message"]["content"];
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["source"]["media_type"], "image/jpeg");
        assert_eq!(content[1]["text"], "rebuild this");
    }

    #[test]
    fn a_turn_without_an_image_is_still_a_content_block() {
        // One envelope either way: the CLI only reads stream-json, so there is
        // no plain-text path to keep working.
        let request = Request::new(Provider::ClaudeCode, "sonnet").user("a blue tunnel");
        let parsed: serde_json::Value = serde_json::from_str(&claude_turn(&request)).unwrap();
        let content = &parsed["message"]["content"];
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "a blue tunnel");
        assert!(content[1].is_null());
    }

    #[test]
    fn a_claude_reply_that_is_not_json_is_reported_as_itself() {
        // The usual cause is the CLI printing an update notice or a crash
        // before it gets as far as producing a result object.
        let e = parse_claude("command not found: node\n", "").unwrap_err();
        assert!(e.contains("did not return JSON"), "{e}");
        assert!(e.contains("node"), "the actual text survives: {e}");
    }

    #[test]
    fn an_empty_claude_reply_says_why_it_was_empty() {
        let out = r#"{"is_error":false,"stop_reason":"max_tokens","result":"","type":"result"}"#;
        let e = parse_claude(out, "").unwrap_err();
        assert!(e.contains("max_tokens"), "{e}");
    }

    #[test]
    fn a_codex_reply_is_the_last_agent_message() {
        // Warnings arrive as completed `error` items in the same stream and
        // are not failures — this exact shape comes back on every run.
        let out = concat!(
            r#"{"type":"thread.started","thread_id":"x"}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"i0","type":"error","message":"Skill descriptions were shortened"}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"{\"ops\":[]}"}}"#,
            "\n",
            r#"{"type":"turn.completed","usage":{"input_tokens":15638}}"#,
        );
        assert_eq!(parse_codex(out, "").unwrap(), "{\"ops\":[]}");
    }

    #[test]
    fn a_codex_failure_unwraps_the_error_it_was_handed() {
        // Codex quotes the upstream body verbatim into its own message. The
        // sentence inside is the only part worth showing.
        let out = concat!(
            r#"{"type":"turn.started"}"#,
            "\n",
            r#"{"type":"error","message":"{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'gpt-5.1-codex' model is not supported when using Codex with a ChatGPT account.\"}}"}"#,
        );
        let e = parse_codex(out, "").unwrap_err();
        assert!(e.contains("not supported when using Codex"), "{e}");
        assert!(
            !e.contains("invalid_request_error"),
            "unwrapped, not quoted: {e}"
        );
    }

    #[test]
    fn a_codex_stream_with_no_reply_falls_back_to_stderr() {
        let e = parse_codex("", "codex: command failed\n").unwrap_err();
        assert!(e.contains("command failed"), "{e}");
        // And with nothing anywhere, it still says something.
        assert!(!parse_codex("", "").unwrap_err().is_empty());
    }

    #[test]
    fn only_the_cli_providers_have_a_command() {
        assert_eq!(command_name(Provider::ClaudeCode), Some("claude"));
        assert_eq!(command_name(Provider::Codex), Some("codex"));
        assert_eq!(command_name(Provider::Anthropic), None);
        assert!(binary(Provider::OpenAi).is_none());
    }

    #[test]
    fn the_tail_of_a_stream_is_the_end_of_it() {
        let text = "starting\nloading\nline one\nline two\nthe actual error";
        let tail = tail(text);
        assert!(tail.contains("the actual error"), "{tail}");
        assert!(!tail.contains("starting"), "{tail}");
        // Order is preserved: the reason usually spans the last few lines.
        assert!(tail.find("line two").unwrap() < tail.find("the actual").unwrap());
    }
}
