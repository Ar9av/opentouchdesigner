//! `otd-ai` — describe a patch in a sentence and get operators.
//!
//! Three providers behind one call — Anthropic, OpenAI, OpenRouter — and one
//! skill: turn a request into nodes, wires and parameter values in the
//! network you are looking at.
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

pub mod keys;
pub mod patch;
pub mod provider;

pub use keys::{Key, Keys};
pub use patch::{Applied, Plan};
pub use provider::{Provider, Request};

use otd_core::{Graph, NodeId, OpRegistry};

/// Everything one question needs.
pub struct Ask<'a> {
    pub provider: Provider,
    pub model: String,
    pub prompt: String,
    pub graph: &'a Graph,
    pub parent: NodeId,
    pub registry: &'a OpRegistry,
}

/// Build the request for an ask.
///
/// Separate from sending it because a `Request` is owned and `Send`, while an
/// [`Ask`] borrows the graph and the registry. The editor builds the request
/// on the UI thread, hands it to a worker, and parses the reply back on the
/// UI thread where the graph lives — so nothing that touches the network ever
/// touches a frame.
pub fn request_for(ask: &Ask) -> Request {
    Request::new(ask.provider, &ask.model)
        .system(patch::system_prompt(ask.registry))
        .user(format!(
            "{}\n\nThe request:\n{}",
            patch::describe(ask.graph, ask.parent),
            ask.prompt.trim()
        ))
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
    /// A shader came back broken and was sent back to be fixed.
    pub repaired: bool,
}

/// Send a request, and if the reply contains a shader that will not compile,
/// hand the compiler's own error back to the model once.
///
/// This is the difference between "it built you a patch" and "it built you a
/// patch with a red node in it". The error is the most useful thing anybody
/// has — a compiler saying exactly which identifier does not exist — and
/// throwing it away to show a user a broken node is a waste of it.
///
/// Once, not until it works: a model that cannot fix its own shader on the
/// second go is not going to on the fifth, and the user is waiting.
pub fn complete_with_repair(
    request: &Request,
    key: &Key,
    keys: &Keys,
    check: Option<patch::ShaderCheck>,
) -> Result<Reply, String> {
    let text = provider::complete(request, key, keys)?;
    let Some(check) = check else {
        return Ok(Reply {
            text,
            repaired: false,
        });
    };
    let Ok(json) = patch::extract_json(&text) else {
        // Not JSON: let the caller report that, rather than asking the model
        // to fix shaders in something that has no shaders in it.
        return Ok(Reply {
            text,
            repaired: false,
        });
    };
    let problems = patch::shader_problems(&json, check);
    if problems.is_empty() {
        return Ok(Reply {
            text,
            repaired: false,
        });
    }

    let complaints = problems
        .iter()
        .map(|(name, error)| format!("- {name}: {error}"))
        .collect::<Vec<_>>()
        .join("\n");
    let retry = Request {
        user: format!(
            "{}\n\nYou replied with this:\n{}\n\nThe shader compiler rejected it:\n{}\n\n\
             Reply with the same JSON again, with those shaders fixed. Keep everything \
             else identical. If you cannot fix a shader, replace that node's source with \
             something simple that compiles.",
            request.user, text, complaints
        ),
        ..request.clone()
    };
    match provider::complete(&retry, key, keys) {
        // Take the second answer even if it is still imperfect: the caller
        // validates it again and reports what is left.
        Ok(second) => Ok(Reply {
            text: second,
            repaired: true,
        }),
        // The retry failed outright — the first answer is still better than
        // an error, and its broken shader will be reported as a warning.
        Err(_) => Ok(Reply {
            text,
            repaired: false,
        }),
    }
}
