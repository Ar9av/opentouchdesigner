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
