//! `otd-ai` — describe a patch in a sentence and get operators.
//!
//! Five providers behind one call — Anthropic, OpenAI and OpenRouter over
//! HTTP with a key; Claude Code and Codex as subprocesses on a subscription
//! this machine is already signed in to — and one skill: turn a request into
//! nodes, wires and parameter values in the network you are looking at.
//!
//! The two things worth knowing before reading further:
//!
//! * **The model is told about the operators by the registry**, not by a
//!   hand-written list. See [`patch::catalogue`].
//! * **Nothing it says reaches the graph unchecked.** The reply is JSON,
//!   validated against that same registry, and a plan naming an operator that
//!   does not exist is refused whole rather than applied in part.
//!
//! No GPU, no UI, and no network call on the cook thread: [`ask`] blocks, and
//! the editor runs it on a worker.

pub mod cli;
pub mod keys;
pub mod patch;
pub mod provider;
pub mod vision;

pub use keys::{Key, Keys};
pub use patch::{Applied, Plan};
pub use provider::{Provider, Request};
pub use vision::Image;

use otd_core::{Graph, NodeId, OpRegistry};

/// Everything one question needs.
pub struct Ask<'a> {
    pub provider: Provider,
    pub model: String,
    pub prompt: String,
    /// A still to work back from, if there is one. With an image attached the
    /// request grows a second brief — see [`patch::reverse_engineer_prompt`] —
    /// and the prompt itself becomes optional, because pointing at a picture
    /// is a complete request on its own.
    pub image: Option<vision::Image>,
    pub graph: &'a Graph,
    pub parent: NodeId,
    /// What the user has clicked on, if anything. This is the referent of
    /// "it" in a request, and the node a new chain most likely hangs off.
    pub selected: Option<NodeId>,
    pub registry: &'a OpRegistry,
    /// Whether the plan may remove operators. Off in the editor unless the
    /// user turns it on for the request — see [`patch::NO_DELETE_RULE`].
    pub allow_delete: bool,
    /// Operators the request is confined to. Empty means the whole network is
    /// fair game, which is the usual case.
    ///
    /// Non-empty and the rest of the patch is locked: see [`patch::SCOPE_RULE`]
    /// for what the model is told and [`patch::gate_scope`] for what is
    /// enforced regardless of what it was told.
    pub scope: &'a [NodeId],
}

/// Build the request for an ask.
///
/// Separate from sending it because a `Request` is owned and `Send`, while an
/// [`Ask`] borrows the graph and the registry. The editor builds the request
/// on the UI thread, hands it to a worker, and parses the reply back on the
/// UI thread where the graph lives — so nothing that touches the network ever
/// touches a frame.
pub fn request_for(ask: &Ask) -> Request {
    let network = patch::describe(ask.graph, ask.parent, ask.selected, ask.scope, ask.registry);
    let prompt = ask.prompt.trim();

    let user = match &ask.image {
        // With an image, the reverse-engineering brief carries the request and
        // anything typed is a refinement on top of it — "like this but slower",
        // "these colours, no feedback". An empty box is a complete ask here,
        // which it never is without a picture.
        Some(_) => {
            let mut text = format!("{network}\n\n{}", patch::reverse_engineer_prompt());
            if !prompt.is_empty() {
                text.push_str(&format!(
                    "\n\nAND WHAT THEY ASKED FOR ON TOP OF THAT\n\
                     Where this conflicts with the image, this wins:\n{prompt}"
                ));
            }
            text
        }
        None => format!("{network}\n\nThe request:\n{prompt}"),
    };

    let mut system = patch::system_prompt(ask.registry);
    if !ask.allow_delete {
        system += patch::NO_DELETE_RULE;
    }
    if !ask.scope.is_empty() {
        system += patch::SCOPE_RULE;
    }

    Request::new(ask.provider, &ask.model)
        .system(system)
        .user(user)
        .image(ask.image.clone())
}

/// Turn a raw reply into a validated plan.
pub fn plan_from_reply(reply: &str, registry: &OpRegistry) -> Result<Plan, String> {
    patch::parse_plan(&patch::extract_json(reply)?, registry)
}

/// Ask for a patch, start to finish. Blocking — call it on a worker thread.
///
/// Errors are safe to show a user: every path runs through key redaction.
pub fn ask(ask: &Ask, keys: &Keys) -> Result<Plan, String> {
    let key = keys
        .get(ask.provider)
        .cloned()
        .unwrap_or_else(|| Key::new(""));
    let reply = provider::complete(&request_for(ask), &key, keys)?;
    plan_from_reply(&reply, ask.registry)
}

/// What a completion produced, and whether it took two goes.
pub struct Reply {
    pub text: String,
    /// What came back broken and was sent back to be fixed, if anything.
    pub repaired: Option<Repair>,
}

/// Which of the two things a reply can get wrong was handed back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repair {
    /// The reply was not JSON the parser would take.
    Json,
    /// A shader in it would not compile.
    Shader,
}

impl Repair {
    /// For the status line, after a "·".
    pub fn label(self) -> &'static str {
        match self {
            Repair::Json => "reformatted on retry",
            Repair::Shader => "shader fixed on retry",
        }
    }
}

/// Ask again, quoting what came back and what was wrong with it.
///
/// The reply is included in full rather than described: a model correcting
/// its own output needs the output, and asking for "the same JSON with the
/// error fixed" without showing it gets a fresh answer to a different
/// question.
fn repair_request(request: &Request, reply: &str, complaint: &str) -> Request {
    Request {
        user: format!(
            "{}\n\nYou replied with this:\n{reply}\n\n{complaint}",
            request.user
        ),
        ..request.clone()
    }
}

/// Send a request, and hand back whatever the reply got wrong — once each.
///
/// Two things can come back broken, and both have the same answer: the error
/// is the most useful thing anybody has, and throwing it away to show the
/// user a failure is a waste of it.
///
///  * **It is not JSON.** A shader full of braces and quotes inside a JSON
///    string is exactly where a model drops an escape, and the reply dies at
///    the parser with a column number. That column number is the fix — the
///    model gets its own output back with the parse error against it.
///  * **A shader will not compile.** The compiler says which identifier does
///    not exist; that goes back too. This is the difference between "it built
///    you a patch" and "it built you a patch with a red node in it".
///
/// Once each, not until it works: a model that cannot fix its own output on
/// the second go is not going to on the fifth, and the user is waiting. The
/// two are separate failures, so a reply that arrives malformed and comes
/// back with a bad shader still gets the shader looked at.
pub fn complete_with_repair(
    request: &Request,
    key: &Key,
    keys: &Keys,
    check: Option<patch::ShaderCheck>,
) -> Result<Reply, String> {
    let first = provider::complete(request, key, keys)?;

    // ---- Is it JSON at all? Nothing else can be asked until it is.
    let (text, json, repaired) = match patch::extract_json(&first) {
        Ok(json) => (first, json, None),
        Err(problem) => {
            let retry = repair_request(
                request,
                &first,
                &format!(
                    "That could not be parsed: {problem}\n\n\
                     Reply with the same patch again as ONE valid JSON object and \
                     nothing else. Mind the strings: a JSON string cannot contain a \
                     literal newline, tab or unescaped double quote, so shader source \
                     must have its newlines written as \\n and its quotes as \\\". \
                     Change nothing about the patch itself.",
                ),
            );
            let Ok(second) = provider::complete(&retry, key, keys) else {
                // The retry failed outright. The first answer is no worse for
                // having been tried, and the caller reports its parse error.
                return Ok(Reply {
                    text: first,
                    repaired: None,
                });
            };
            match patch::extract_json(&second) {
                Ok(json) => (second, json, Some(Repair::Json)),
                // Still not JSON. Hand back the second attempt and let the
                // caller report it: two goes is the budget.
                Err(_) => {
                    return Ok(Reply {
                        text: second,
                        repaired: Some(Repair::Json),
                    });
                }
            }
        }
    };

    // ---- It parses. Do the shaders in it compile?
    let Some(check) = check else {
        return Ok(Reply { text, repaired });
    };
    let problems = patch::shader_problems(&json, check);
    if problems.is_empty() {
        return Ok(Reply { text, repaired });
    }

    let complaints = problems
        .iter()
        .map(|(name, error)| format!("- {name}: {error}"))
        .collect::<Vec<_>>()
        .join("\n");
    let retry = repair_request(
        request,
        &text,
        &format!(
            "The shader compiler rejected it:\n{complaints}\n\n\
             `Unknown variable` is almost always a name from TouchDesigner or \
             from a desktop GL version, and neither is what compiles here. The \
             input images are `iChannel0` and `iChannel1` — not `sTD2DInputs`, \
             not `sIn0`, and not declared with `uniform sampler2D`. The node's \
             uniform1..4 parameters are `U.p0`..`U.p3`. Sampling is `texture()`, \
             never `texture2D()`.\n\n\
             Reply with the same JSON again, with those shaders fixed. Keep \
             everything else identical. If you cannot fix a shader, replace that \
             node's source with something simple that compiles — and if it had an \
             input wired, that fallback must still pass the input through with \
             `texture(iChannel0, fragCoord / iResolution.xy)` rather than \
             drawing over it."
        ),
    );
    match provider::complete(&retry, key, keys) {
        // Take the second answer even if it is still imperfect: the caller
        // validates it again and reports what is left.
        Ok(second) => Ok(Reply {
            text: second,
            repaired: Some(Repair::Shader),
        }),
        // The retry failed outright — the first answer is still better than
        // an error, and its broken shader will be reported as a warning.
        Err(_) => Ok(Reply { text, repaired }),
    }
}
