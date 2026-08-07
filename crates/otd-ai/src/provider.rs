//! The three providers, behind one call.
//!
//! OpenAI and OpenRouter speak the same wire format — OpenRouter exists to be
//! a drop-in for it — so they share a request builder and differ only in host
//! and headers. Anthropic's is close but not the same: the system prompt is a
//! top-level field rather than a message, auth is `x-api-key` rather than a
//! bearer token, and it wants a version header. That is the whole difference,
//! and it is small enough that a trait would cost more than it saved.
//!
//! Everything here is blocking. The caller runs it on a worker thread, which
//! is the same rule every device in this codebase follows: nothing that can
//! wait on the network is allowed anywhere near a frame.

use std::time::Duration;

use crate::keys::{Key, Keys, redact};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provider {
    Anthropic,
    OpenAi,
    OpenRouter,
}

impl Provider {
    pub const ALL: &'static [Provider] =
        &[Provider::Anthropic, Provider::OpenAi, Provider::OpenRouter];

    /// The stable name used in the config file and in the UI.
    pub fn id(&self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
            Provider::OpenRouter => "openrouter",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::OpenAi => "OpenAI",
            Provider::OpenRouter => "OpenRouter",
        }
    }

    pub fn parse(name: &str) -> Option<Provider> {
        Provider::ALL
            .iter()
            .copied()
            .find(|p| p.id().eq_ignore_ascii_case(name.trim()))
    }

    /// The environment variable this provider's key is conventionally in.
    pub fn env_var(&self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::OpenAi => "OPENAI_API_KEY",
            Provider::OpenRouter => "OPENROUTER_API_KEY",
        }
    }

    pub fn endpoint(&self) -> &'static str {
        match self {
            Provider::Anthropic => "https://api.anthropic.com/v1/messages",
            Provider::OpenAi => "https://api.openai.com/v1/chat/completions",
            Provider::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
        }
    }

    /// Where a user goes to get one. Shown next to an empty key field,
    /// because "paste your API key" is useless if you have not got one.
    pub fn console_url(&self) -> &'static str {
        match self {
            Provider::Anthropic => "https://console.anthropic.com/settings/keys",
            Provider::OpenAi => "https://platform.openai.com/api-keys",
            Provider::OpenRouter => "https://openrouter.ai/keys",
        }
    }

    /// A few models worth defaulting to, newest first. Free text is still
    /// accepted — this list is a convenience, not a whitelist, because it
    /// will be out of date before anybody reads it.
    pub fn models(&self) -> &'static [&'static str] {
        match self {
            Provider::Anthropic => &[
                "claude-sonnet-4-5-20250929",
                "claude-opus-4-1-20250805",
                "claude-haiku-4-5-20251001",
            ],
            Provider::OpenAi => &["gpt-5", "gpt-5-mini", "gpt-4.1", "gpt-4o"],
            Provider::OpenRouter => &[
                "anthropic/claude-sonnet-4.5",
                "openai/gpt-5",
                "google/gemini-2.5-pro",
                "meta-llama/llama-4-maverick",
            ],
        }
    }

    pub fn default_model(&self) -> &'static str {
        self.models()[0]
    }
}

/// One completion request. No conversation history: every ask is
/// self-contained, because the thing being edited — the patch — is the state,
/// and it is already in the prompt.
#[derive(Clone, Debug)]
pub struct Request {
    pub provider: Provider,
    pub model: String,
    pub system: String,
    pub user: String,
    pub max_tokens: u32,
    pub timeout: Duration,
}

impl Request {
    pub fn new(provider: Provider, model: impl Into<String>) -> Request {
        Request {
            provider,
            model: model.into(),
            system: String::new(),
            user: String::new(),
            max_tokens: 8192,
            // Long enough for a reasoning model on a cold start, short enough
            // that a hung provider does not leave a thread waiting all show.
            timeout: Duration::from_secs(120),
        }
    }

    pub fn system(mut self, text: impl Into<String>) -> Request {
        self.system = text.into();
        self
    }

    pub fn user(mut self, text: impl Into<String>) -> Request {
        self.user = text.into();
        self
    }

    /// The JSON body for this provider.
    pub fn body(&self) -> serde_json::Value {
        match self.provider {
            Provider::Anthropic => serde_json::json!({
                "model": self.model,
                "max_tokens": self.max_tokens,
                "system": self.system,
                "messages": [{ "role": "user", "content": self.user }],
            }),
            // OpenAI and OpenRouter: the system prompt is a message.
            _ => serde_json::json!({
                "model": self.model,
                "max_completion_tokens": self.max_tokens,
                "messages": [
                    { "role": "system", "content": self.system },
                    { "role": "user", "content": self.user },
                ],
            }),
        }
    }
}

/// Pull the assistant's text out of whichever shape came back.
pub fn extract_text(provider: Provider, body: &serde_json::Value) -> Result<String, String> {
    // A provider that returns 200 with an error object in it is common
    // enough to check for first, whatever the shape.
    if let Some(error) = body.get("error") {
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(message.to_string());
    }

    let text = match provider {
        Provider::Anthropic => body
            .get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default(),
        _ => body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string(),
    };

    if text.trim().is_empty() {
        // A refusal or a length stop reads as an empty reply; say which.
        let reason = finish_reason(provider, body);
        return Err(match reason {
            Some(r) => format!("the model returned nothing (stopped: {r})"),
            None => "the model returned nothing".into(),
        });
    }
    Ok(text)
}

fn finish_reason(provider: Provider, body: &serde_json::Value) -> Option<String> {
    let value = match provider {
        Provider::Anthropic => body.get("stop_reason"),
        _ => body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|c| c.get("finish_reason")),
    };
    value.and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Send a request and return the assistant's text.
///
/// Blocking. Call it from a worker thread.
pub fn complete(request: &Request, key: &Key, keys: &Keys) -> Result<String, String> {
    if key.is_empty() {
        return Err(format!(
            "no API key for {} — paste one, or set {}",
            request.provider.label(),
            request.provider.env_var()
        ));
    }

    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(request.timeout))
        // Handle non-2xx ourselves. The default turns a 401 into an error
        // that has thrown the body away, and the body is where the provider
        // says *which* thing was wrong — worth far more than the number.
        .http_status_as_error(false)
        .build()
        .new_agent();

    let mut call = agent.post(request.provider.endpoint());
    call = match request.provider {
        Provider::Anthropic => call
            .header("x-api-key", key.expose())
            .header("anthropic-version", "2023-06-01"),
        _ => call.header("authorization", &format!("Bearer {}", key.expose())),
    };
    if request.provider == Provider::OpenRouter {
        // OpenRouter attributes traffic by these, and asks that clients send
        // them. Neither identifies the user.
        call = call
            .header("http-referer", "https://github.com/Ar9av/opentouchdesigner")
            .header("x-title", "OpenTouchDesigner");
    }

    let result = call.send_json(request.body());

    // Every path out of here goes through `redact`: providers quote the
    // request back in error bodies often enough to matter.
    let mut response =
        result.map_err(|e| redact(&format!("could not reach the provider: {e}"), keys))?;
    let status = response.status().as_u16();

    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| redact(&format!("could not read the reply: {e}"), keys))?;

    if !(200..300).contains(&status) {
        return Err(redact(&explain_status(status, &text), keys));
    }

    let body: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| redact(&format!("the reply was not JSON: {e}"), keys))?;
    extract_text(request.provider, &body).map_err(|e| redact(&e, keys))
}

/// Turn a failed response into something worth reading.
///
/// The provider's own message first when there is one — "invalid x-api-key"
/// and "your credit balance is too low" are different problems that both
/// arrive as 401-shaped disappointment — then what to do about it.
fn explain_status(code: u16, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message").or(Some(e)))
                .map(|m| match m.as_str() {
                    Some(s) => s.to_string(),
                    None => m.to_string(),
                })
                .or_else(|| v.get("message").and_then(|m| m.as_str()).map(String::from))
        })
        .unwrap_or_else(|| body.chars().take(200).collect());

    let advice = match code {
        401 | 403 => "check the key, and that it is for this provider",
        404 => "check the model name — this provider may not have it",
        413 => "the request was too large; try a smaller network or prompt",
        429 => "rate limited or out of credit; wait, or check billing",
        500..=599 => "the provider is having trouble; try again",
        _ => "the provider refused the request",
    };
    if detail.trim().is_empty() {
        format!("{code}: {advice}")
    } else {
        format!("{code}: {} — {advice}", detail.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_and_openai_bodies_differ_where_they_have_to() {
        let anthropic = Request::new(Provider::Anthropic, "claude-sonnet-4-5-20250929")
            .system("be terse")
            .user("hello");
        let body = anthropic.body();
        // Anthropic takes the system prompt as a field, not a message.
        assert_eq!(body["system"], "be terse");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["max_tokens"], 8192);

        let openai = Request::new(Provider::OpenAi, "gpt-5")
            .system("be terse")
            .user("hello");
        let body = openai.body();
        assert!(body.get("system").is_none());
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["content"], "hello");
    }

    #[test]
    fn openrouter_is_openai_on_the_wire() {
        let a = Request::new(Provider::OpenRouter, "x")
            .system("s")
            .user("u");
        let b = Request::new(Provider::OpenAi, "x").system("s").user("u");
        assert_eq!(a.body(), b.body());
        assert_ne!(a.provider.endpoint(), b.provider.endpoint());
    }

    #[test]
    fn each_shape_of_reply_is_understood() {
        let anthropic = serde_json::json!({
            "content": [
                { "type": "thinking", "thinking": "hmm" },
                { "type": "text", "text": "the answer" }
            ],
            "stop_reason": "end_turn"
        });
        assert_eq!(
            extract_text(Provider::Anthropic, &anthropic).unwrap(),
            "the answer"
        );

        let openai = serde_json::json!({
            "choices": [{ "message": { "content": "the answer" }, "finish_reason": "stop" }]
        });
        assert_eq!(
            extract_text(Provider::OpenAi, &openai).unwrap(),
            "the answer"
        );
        assert_eq!(
            extract_text(Provider::OpenRouter, &openai).unwrap(),
            "the answer"
        );
    }

    #[test]
    fn a_two_hundred_carrying_an_error_is_still_an_error() {
        // Both providers do this, and treating it as an empty reply would
        // report "the model returned nothing" for an expired key.
        let body = serde_json::json!({ "error": { "message": "insufficient_quota" } });
        let e = extract_text(Provider::OpenAi, &body).unwrap_err();
        assert!(e.contains("insufficient_quota"), "{e}");
    }

    #[test]
    fn an_empty_reply_says_why_it_was_empty() {
        let body = serde_json::json!({
            "choices": [{ "message": { "content": "" }, "finish_reason": "length" }]
        });
        let e = extract_text(Provider::OpenAi, &body).unwrap_err();
        assert!(e.contains("length"), "{e}");
    }

    #[test]
    fn a_failure_reports_what_the_provider_said_not_just_the_number() {
        // "invalid x-api-key" and "your credit balance is too low" are
        // different problems that both arrive as 401-shaped disappointment.
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let text = explain_status(401, body);
        assert!(text.contains("invalid x-api-key"), "{text}");
        assert!(text.contains("check the key"), "{text}");

        let openai = r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota"}}"#;
        assert!(explain_status(429, openai).contains("exceeded your current quota"));

        // A body that is not JSON at all — an HTML error page from a proxy —
        // still has to produce something a person can act on.
        let html = explain_status(503, "<html>Service Unavailable</html>");
        assert!(html.contains("503"), "{html}");
        assert!(html.contains("try again"), "{html}");

        assert!(explain_status(404, "").contains("model"));
    }

    #[test]
    fn provider_names_round_trip() {
        for p in Provider::ALL {
            assert_eq!(Provider::parse(p.id()), Some(*p));
            assert_eq!(Provider::parse(&p.id().to_uppercase()), Some(*p));
            assert!(!p.default_model().is_empty());
            assert!(p.endpoint().starts_with("https://"));
        }
        assert_eq!(Provider::parse("gemini"), None);
    }
}
